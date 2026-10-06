//! M1 fixed orbit camera. Looks at the chunk from one corner.
//! M2 will turn this into a real first-person camera with movement.

use glam::{Mat4, Vec3};

pub struct Camera {
    pub eye: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub fov_y: f32,
    pub aspect: f32,
    pub znear: f32,
    pub zfar: f32,
}

impl Camera {
    /// Camera placed looking at a 16x16x128 chunk centered on the origin.
    /// (Chunk occupies x,z in [0,16], y in [0,64] when half-filled with stone.)
    pub fn default_orbit(aspect: f32) -> Self {
        Self {
            eye: Vec3::new(24.0, 48.0, 32.0),
            target: Vec3::new(8.0, 24.0, 8.0),
            up: Vec3::Y,
            fov_y: 60_f32.to_radians(),
            aspect,
            znear: 0.1,
            zfar: 512.0,
        }
    }

    pub fn build_view_proj(&self) -> (Mat4, Mat4) {
        let view = Mat4::look_at_rh(self.eye, self.target, self.up);
        let proj = Mat4::perspective_rh(self.fov_y, self.aspect, self.znear, self.zfar);
        (view, proj)
    }
}
