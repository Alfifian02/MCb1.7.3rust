//! Port MathHelper. Tabel sin 65536 entri dipertahankan agar hasil identik dengan Java
//! (bukan f32::sin), karena dipakai fisika, AI, dan render.

use std::sync::OnceLock;

fn sin_table() -> &'static [f32; 65536] {
    static T: OnceLock<Box<[f32; 65536]>> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = Box::new([0f32; 65536]);
        for (i, v) in t.iter_mut().enumerate() {
            *v = (i as f64 * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32;
        }
        t
    })
}

#[inline]
pub fn sin(x: f32) -> f32 {
    sin_table()[((x * 10430.378f32) as i32 & 0xFFFF) as usize]
}

#[inline]
pub fn cos(x: f32) -> f32 {
    sin_table()[((x * 10430.378f32 + 16384.0f32) as i32 & 0xFFFF) as usize]
}

#[inline]
pub fn sqrt_float(x: f32) -> f32 {
    (x as f64).sqrt() as f32
}

/// Java: (float)Math.sqrt(double). Hasil dipotong ke f32, penting untuk kesamaan perilaku.
#[inline]
pub fn sqrt_double(x: f64) -> f32 {
    x.sqrt() as f32
}

#[inline]
pub fn floor_float(x: f32) -> i32 {
    let i = x as i32;
    if x < i as f32 { i - 1 } else { i }
}

#[inline]
pub fn floor_double(x: f64) -> i32 {
    let i = x as i32;
    if x < i as f64 { i - 1 } else { i }
}

#[inline]
pub fn abs(x: f32) -> f32 {
    if x >= 0.0 { x } else { -x }
}

#[inline]
pub fn abs_max(a: f64, b: f64) -> f64 {
    let a = if a < 0.0 { -a } else { a };
    let b = if b < 0.0 { -b } else { b };
    if a > b { a } else { b }
}

/// Pembagian ke bawah (floor) untuk bucket chunk; sama dengan Java bucketInt.
#[inline]
pub fn bucket_int(a: i32, b: i32) -> i32 {
    if a < 0 { -((-a - 1) / b) - 1 } else { a / b }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_negatif() {
        assert_eq!(floor_double(-0.5), -1);
        assert_eq!(floor_double(1.9), 1);
        assert_eq!(floor_float(-1.0), -1);
    }

    #[test]
    fn bucket() {
        assert_eq!(bucket_int(-1, 16), -1);
        assert_eq!(bucket_int(-16, 16), -1);
        assert_eq!(bucket_int(-17, 16), -2);
        assert_eq!(bucket_int(15, 16), 0);
    }

    #[test]
    fn sin_cos_dasar() {
        assert!(sin(0.0).abs() < 1e-4);
        assert!((cos(0.0) - 1.0).abs() < 1e-4);
    }
}
