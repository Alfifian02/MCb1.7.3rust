//! The frame's colour space: linear Rec. 2020 (what the sky and chunk shaders write, and `present` converts back for the screen).

/// Column-major 3x3 as GLSL's `mat3(a, b, c, ...)` is written; `vm` is GLSL's `v * M`.
type M3 = [f64; 9];
const XYZ_TO_REC2020: M3 = [1.7166084, -0.3556621, -0.2533601, -0.6666829, 1.6164776, 0.0157685, 0.0176422, -0.0427763, 0.94222867];
const REC709_TO_XYZ: M3 = [0.4124, 0.3576, 0.1805, 0.2126, 0.7152, 0.0722, 0.0193, 0.1192, 0.9505];

fn vm(v: [f64; 3], m: &M3) -> [f64; 3] {
    std::array::from_fn(|j| v[0] * m[3 * j] + v[1] * m[3 * j + 1] + v[2] * m[3 * j + 2])
}

/// A vanilla (gamma) colour as the frame's linear Rec. 2020: `pow(x, 2.2)`, then Rec. 709 -> 2020.
pub fn legacy_lin(c: [f32; 3]) -> [f32; 3] {
    vm(vm(c.map(|x| f64::from(x).powf(2.2)), &REC709_TO_XYZ), &XYZ_TO_REC2020).map(|x| x as f32)
}
