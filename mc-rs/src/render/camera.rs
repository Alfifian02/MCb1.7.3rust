//! M2 first-person camera.
//!
//! Right-handed, yaw=0 looks toward -Z, matching the chunk mesh's +Z-front convention.
//! Touch-drag and mouse-look both feed into `yaw` / `pitch` directly.

use glam::{Mat4, Vec3, Vec4};

pub struct FirstPersonCamera {
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub fov_y: f32,
    pub aspect: f32,
    pub znear: f32,
    pub zfar: f32,
}

impl FirstPersonCamera {
    /// Spawn standing on top of the stone pillar at y=64, facing -Z, looking slightly down.
    pub fn spawn_on_top_of_chunk() -> Self {
        Self::spawn_at(8.0, 64.0 + 0.9 + 1.62, 8.0)
    }

    /// Spawn at a specific (x, y, z) with default yaw/pitch/fov.
    pub fn spawn_at(x: f32, y: f32, z: f32) -> Self {
        Self {
            pos: Vec3::new(x, y, z),
            yaw: 0.0,
            pitch: 0.3, // positive pitch = look down (forward.y = -sin(pitch))
            fov_y: 70_f32.to_radians(),
            aspect: 1.0,
            znear: 0.05,
            zfar: 256.0,
        }
    }

    /// Yaw=0 -> forward = -Z. Pitch=0 -> horizontal.
    pub fn forward(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        Vec3::new(sy * cp, -sp, -cy * cp).normalize()
    }

    pub fn eye(&self) -> Vec3 {
        self.pos
    }

    pub fn add_yaw(&mut self, dy: f32) {
        self.yaw += dy;
    }
    pub fn add_pitch(&mut self, dp: f32) {
        // Clamp to ±~89° to avoid the gimbal flip at the poles.
        let max = 89.0_f32.to_radians();
        self.pitch = (self.pitch + dp).clamp(-max, max);
    }

    pub fn build_view_proj(&self) -> (Mat4, Mat4) {
        let fwd = self.forward();
        let view = Mat4::look_at_rh(self.pos, self.pos + fwd, Vec3::Y);
        let proj = Mat4::perspective_rh(self.fov_y, self.aspect, self.znear, self.zfar);
        (view, proj)
    }
}

/// ponytail: no shadows past 64 blocks (= the render ring); upgrade = cascades.
pub const SHADOW_RADIUS: f32 = 64.0;

/// The sun's view-projection (`shadowProjection * shadowModelView`): an orthographic box `SHADOW_RADIUS` blocks to each side of
/// `center`, looking along the sun's rays from `Z` blocks toward the sun. Depth 0..1 like `Mat4::perspective_rh`, so
/// `Frustum` culls with it too. The sun moves in the YZ plane, so `X` is always a valid up vector.
pub fn shadow_view_proj(center: Vec3, sun: Vec3) -> Mat4 {
    const Z: f32 = 128.0; // the world is 128 tall
    Mat4::orthographic_rh(-SHADOW_RADIUS, SHADOW_RADIUS, -SHADOW_RADIUS, SHADOW_RADIUS, 0.0, 2.0 * Z) * Mat4::look_at_rh(center + sun * Z, center, Vec3::X)
}

/// The shadow map is drawn from `center` along `light` and read back with the same matrix, so it stays right for the geometry it
/// saw when the eye walks off or the sun creeps on. It is redrawn only when that error shows (`refresh`), not every frame.
pub struct ShadowCache {
    pub center: Vec3,
    pub light: Vec3,
    mesh_gen: u32,
    valid: bool,
}

impl ShadowCache {
    /// Eye travel (blocks) and light turn (cos 0.5 degrees) after which the map is redrawn. At 4 blocks the distorted map's
    /// texels near the eye are still fine; 0.5 degrees moves a 10-block tree's shadow by ~9 cm.
    const MOVE: f32 = 4.0;
    const COS: f32 = 0.99996;

    pub fn new() -> Self {
        Self { center: Vec3::ZERO, light: Vec3::Y, mesh_gen: 0, valid: false }
    }

    /// True when the shadow pass has to run this frame; then `center` / `light` are the new ones. `want` false (no shadow
    /// consumer this frame) drops the cache, because the map is not drawn and goes stale.
    pub fn refresh(&mut self, want: bool, eye: Vec3, light: Vec3, mesh_gen: u32) -> bool {
        let fresh = want
            && self.valid
            && self.mesh_gen == mesh_gen
            && (eye - self.center).length_squared() <= Self::MOVE * Self::MOVE
            && light.dot(self.light) >= Self::COS;
        self.valid = want;
        if want && !fresh {
            *self = Self { center: eye, light, mesh_gen, valid: true };
        }
        want && !fresh
    }
}

/// M13 view frustum: the six planes of `proj * view` (Gribb-Hartmann), for culling whole chunks before they are drawn.
/// Depth is 0..1 (`Mat4::perspective_rh`, wgpu), so the near plane is row 2 alone.
pub struct Frustum {
    /// `(a, b, c, d)` with the normal `(a, b, c)` pointing into the frustum, normalised so `d` is a distance.
    planes: [Vec4; 6],
}

impl Frustum {
    pub fn from_view_proj(m: Mat4) -> Self {
        let (r0, r1, r2, r3) = (m.row(0), m.row(1), m.row(2), m.row(3));
        let planes = [r3 + r0, r3 - r0, r3 + r1, r3 - r1, r2, r3 - r2].map(|p| p / p.truncate().length());
        Self { planes }
    }

    /// False only when the box `min..max` lies wholly outside one plane (conservative: a box near a corner
    /// can pass without being visible, never the other way round).
    pub fn intersects_aabb(&self, min: Vec3, max: Vec3) -> bool {
        self.planes.iter().all(|p| {
            // The corner furthest along the plane normal; if even that is behind the plane the box is out.
            let v = Vec3::new(if p.x >= 0.0 { max.x } else { min.x }, if p.y >= 0.0 { max.y } else { min.y }, if p.z >= 0.0 { max.z } else { min.z });
            p.truncate().dot(v) + p.w >= 0.0
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rays toward the sun share one shadow-map texel, the box edge is NDC 1, and the box centre sits mid-depth.
    #[test]
    fn shadow_matrix_follows_the_sun() {
        let (c, sun) = (Vec3::new(10.0, 70.0, -5.0), Vec3::new(0.0, 0.8, 0.6));
        let vp = shadow_view_proj(c, sun);
        let (a, b) = (vp.project_point3(c + Vec3::new(3.0, 1.0, 2.0)), vp.project_point3(c + Vec3::new(3.0, 1.0, 2.0) + sun * 20.0));
        assert!((a.truncate() - b.truncate()).length() < 1e-4 && b.z < a.z, "same texel, nearer the sun is shallower");
        assert!((vp.project_point3(c) - Vec3::new(0.0, 0.0, 0.5)).length() < 1e-4);
        assert!((vp.project_point3(c + Vec3::X * 64.0).y.abs() - 1.0).abs() < 1e-4);
    }

    /// The cached map is reused until the eye, the light or the geometry moves enough, and is dropped when nobody reads it.
    #[test]
    fn shadow_cache_redraws_only_when_stale() {
        let (eye, l) = (Vec3::new(5.0, 70.0, 5.0), Vec3::new(0.0, 0.8, 0.6).normalize());
        let mut c = ShadowCache::new();
        assert!(c.refresh(true, eye, l, 0), "first frame draws");
        assert!(!c.refresh(true, eye + Vec3::new(3.0, 0.0, 0.0), l, 0), "small step reuses");
        assert_eq!(c.center, eye, "and keeps the old centre");
        assert!(c.refresh(true, eye + Vec3::new(5.0, 0.0, 0.0), l, 0), "walked 5 blocks");
        let turned = Vec3::new(0.0, 0.6f32.atan2(0.8) + 0.02, 0.0); // ~1.1 degrees
        let l2 = Vec3::new(0.0, turned.y.cos(), turned.y.sin());
        assert!(c.refresh(true, c.center, l2, 0), "light turned");
        assert!(c.refresh(true, c.center, l2, 1), "geometry changed");
        assert!(!c.refresh(false, c.center, l2, 1) && c.refresh(true, c.center, l2, 1), "dropped while unused");
    }

    #[test]
    fn frustum_culls_boxes_outside_the_view() {
        // At the origin looking down -Z (yaw 0, pitch 0), 70 degrees tall, 2:1, far plane 256.
        let mut cam = FirstPersonCamera::spawn_at(0.0, 0.0, 0.0);
        cam.pitch = 0.0;
        cam.aspect = 2.0;
        let (view, proj) = cam.build_view_proj();
        let f = Frustum::from_view_proj(proj * view);
        let cube = |c: Vec3| f.intersects_aabb(c - Vec3::splat(8.0), c + Vec3::splat(8.0));
        assert!(cube(Vec3::new(0.0, 0.0, -50.0)), "straight ahead");
        assert!(!cube(Vec3::new(0.0, 0.0, 50.0)), "behind");
        assert!(!cube(Vec3::new(200.0, 0.0, -20.0)), "far to the right");
        assert!(!cube(Vec3::new(-200.0, 0.0, -20.0)), "far to the left");
        assert!(!cube(Vec3::new(0.0, 200.0, -20.0)) && !cube(Vec3::new(0.0, -200.0, -20.0)), "above and below");
        assert!(!cube(Vec3::new(0.0, 0.0, -400.0)), "beyond the far plane");
        assert!(cube(Vec3::ZERO), "the box the camera is inside of");
        // A 2:1 view is wider than tall: 40 blocks out, 30 to the side is in view, 60 up is not (the half-height at
        // 48 blocks is 33.6, so even the box's lowest far corner, y=52, is above it; a box at y=40 would still clip in).
        assert!(cube(Vec3::new(30.0, 0.0, -40.0)) && !cube(Vec3::new(0.0, 60.0, -40.0)) && cube(Vec3::new(0.0, 40.0, -40.0)));
        // Turning 90 degrees right (yaw +pi/2 looks toward +X) moves the view onto the box that was off to the right.
        cam.yaw = std::f32::consts::FRAC_PI_2;
        let (view, proj) = cam.build_view_proj();
        let f = Frustum::from_view_proj(proj * view);
        assert!(f.intersects_aabb(Vec3::new(40.0, -8.0, -8.0), Vec3::new(56.0, 8.0, 8.0)) && !f.intersects_aabb(Vec3::new(-56.0, -8.0, -8.0), Vec3::new(-40.0, 8.0, 8.0)));
    }
}
