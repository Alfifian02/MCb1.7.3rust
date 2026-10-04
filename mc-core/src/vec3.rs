//! Port Vec3D. Pool statis Java (createVector) dibuang: struct Copy di stack, tanpa GC/alokasi.
//! Catatan: `subtract` mengikuti Java apa adanya: hasil = other - self (terbalik dari konvensi umum).

use crate::math;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    #[inline]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// Java: this.subtract(v) = v - this
    #[inline]
    pub fn subtract(self, v: Vec3) -> Vec3 {
        Vec3::new(v.x - self.x, v.y - self.y, v.z - self.z)
    }

    pub fn normalize(self) -> Vec3 {
        let len = math::sqrt_double(self.x * self.x + self.y * self.y + self.z * self.z) as f64;
        if len < 1.0e-4 {
            Vec3::new(0.0, 0.0, 0.0)
        } else {
            Vec3::new(self.x / len, self.y / len, self.z / len)
        }
    }

    pub fn cross(self, v: Vec3) -> Vec3 {
        Vec3::new(
            self.y * v.z - self.z * v.y,
            self.z * v.x - self.x * v.z,
            self.x * v.y - self.y * v.x,
        )
    }

    #[inline]
    pub fn add(self, dx: f64, dy: f64, dz: f64) -> Vec3 {
        Vec3::new(self.x + dx, self.y + dy, self.z + dz)
    }

    pub fn distance_to(self, v: Vec3) -> f64 {
        let (dx, dy, dz) = (v.x - self.x, v.y - self.y, v.z - self.z);
        math::sqrt_double(dx * dx + dy * dy + dz * dz) as f64
    }

    #[inline]
    pub fn square_distance_to(self, v: Vec3) -> f64 {
        let (dx, dy, dz) = (v.x - self.x, v.y - self.y, v.z - self.z);
        dx * dx + dy * dy + dz * dz
    }

    #[inline]
    pub fn square_distance_to_xyz(self, x: f64, y: f64, z: f64) -> f64 {
        let (dx, dy, dz) = (x - self.x, y - self.y, z - self.z);
        dx * dx + dy * dy + dz * dz
    }

    pub fn length(self) -> f64 {
        math::sqrt_double(self.x * self.x + self.y * self.y + self.z * self.z) as f64
    }

    // Ambang Java: (double)1.0E-7F (float dinaikkan ke double)
    const EPS: f64 = 1.0e-7f32 as f64;

    pub fn intermediate_with_x(self, to: Vec3, x: f64) -> Option<Vec3> {
        let (dx, dy, dz) = (to.x - self.x, to.y - self.y, to.z - self.z);
        if dx * dx < Self::EPS {
            return None;
        }
        let t = (x - self.x) / dx;
        if (0.0..=1.0).contains(&t) {
            Some(Vec3::new(self.x + dx * t, self.y + dy * t, self.z + dz * t))
        } else {
            None
        }
    }

    pub fn intermediate_with_y(self, to: Vec3, y: f64) -> Option<Vec3> {
        let (dx, dy, dz) = (to.x - self.x, to.y - self.y, to.z - self.z);
        if dy * dy < Self::EPS {
            return None;
        }
        let t = (y - self.y) / dy;
        if (0.0..=1.0).contains(&t) {
            Some(Vec3::new(self.x + dx * t, self.y + dy * t, self.z + dz * t))
        } else {
            None
        }
    }

    pub fn intermediate_with_z(self, to: Vec3, z: f64) -> Option<Vec3> {
        let (dx, dy, dz) = (to.x - self.x, to.y - self.y, to.z - self.z);
        if dz * dz < Self::EPS {
            return None;
        }
        let t = (z - self.z) / dz;
        if (0.0..=1.0).contains(&t) {
            Some(Vec3::new(self.x + dx * t, self.y + dy * t, self.z + dz * t))
        } else {
            None
        }
    }

    pub fn rotate_around_x(&mut self, angle: f32) {
        let c = math::cos(angle) as f64;
        let s = math::sin(angle) as f64;
        let (x, y, z) = (self.x, self.y * c + self.z * s, self.z * c - self.y * s);
        *self = Vec3::new(x, y, z);
    }

    pub fn rotate_around_y(&mut self, angle: f32) {
        let c = math::cos(angle) as f64;
        let s = math::sin(angle) as f64;
        let (x, y, z) = (self.x * c + self.z * s, self.y, self.z * c - self.x * s);
        *self = Vec3::new(x, y, z);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtract_terbalik_seperti_java() {
        let a = Vec3::new(1.0, 2.0, 3.0);
        let b = Vec3::new(4.0, 6.0, 8.0);
        assert_eq!(a.subtract(b), Vec3::new(3.0, 4.0, 5.0));
    }

    #[test]
    fn intermediate_di_luar_segmen() {
        let a = Vec3::new(0.0, 0.0, 0.0);
        let b = Vec3::new(10.0, 0.0, 0.0);
        assert!(a.intermediate_with_x(b, 20.0).is_none());
        assert_eq!(a.intermediate_with_x(b, 5.0), Some(Vec3::new(5.0, 0.0, 0.0)));
        assert!(a.intermediate_with_y(b, 0.0).is_none()); // dy ~ 0
    }
}
