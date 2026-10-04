//! Minimal column-major 4x4 matrix helpers.
pub type Mat4 = [f32; 16];

pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut o = [0.0; 16];
    for c in 0..4 {
        for r in 0..4 {
            o[c * 4 + r] = (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum();
        }
    }
    o
}

pub fn perspective(fovy: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let f = 1.0 / (fovy * 0.5).tan();
    let mut m = [0.0; 16];
    m[0] = f / aspect;
    m[5] = f;
    m[10] = (far + near) / (near - far);
    m[11] = -1.0;
    m[14] = 2.0 * far * near / (near - far);
    m
}

pub fn forward(yaw: f32, pitch: f32) -> [f32; 3] {
    [yaw.sin() * pitch.cos(), pitch.sin(), -yaw.cos() * pitch.cos()]
}

pub fn right(yaw: f32) -> [f32; 3] {
    [yaw.cos(), 0.0, yaw.sin()]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

pub fn view(eye: [f32; 3], yaw: f32, pitch: f32) -> Mat4 {
    let f = forward(yaw, pitch);
    let s = cross(f, [0.0, 1.0, 0.0]);
    let l = dot(s, s).sqrt();
    let s = [s[0] / l, s[1] / l, s[2] / l];
    let u = cross(s, f);
    [
        s[0], u[0], -f[0], 0.0,
        s[1], u[1], -f[1], 0.0,
        s[2], u[2], -f[2], 0.0,
        -dot(s, eye), -dot(u, eye), dot(f, eye), 1.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_mul() {
        let i = { let mut m = [0.0; 16]; for k in 0..4 { m[k * 5] = 1.0; } m };
        let p = perspective(1.0, 1.5, 0.1, 100.0);
        assert_eq!(mul(&i, &p), p);
    }
    #[test]
    fn point_in_front_is_visible() {
        let m = mul(&perspective(1.2, 1.0, 0.1, 100.0), &view([0.0, 0.0, 0.0], 0.0, 0.0));
        // point at (0,0,-5): clip.w must be positive and |x|,|y| <= w
        let (x, y, z, w) = (m[12] + -5.0 * m[8], m[13] + -5.0 * m[9], m[14] + -5.0 * m[10], m[15] + -5.0 * m[11]);
        assert!(w > 0.0 && x.abs() <= w && y.abs() <= w && z.abs() <= w);
    }
}
