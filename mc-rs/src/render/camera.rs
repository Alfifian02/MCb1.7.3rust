//! M2 first-person camera.
//!
//! Right-handed, yaw=0 looks toward -Z, matching the chunk mesh's +Z-front convention.
//! Touch-drag and mouse-look both feed into `yaw` / `pitch` directly.

use glam::{Mat4, Vec3};

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
            pitch: -0.3,
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
