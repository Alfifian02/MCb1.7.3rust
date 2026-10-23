//! The 12-byte terrain vertex: Sodium's `CompactChunkVertex` (20 bytes: 20-bit positions, 15-bit texture coordinates, light, material
//! and section index) cut down for a host that draws one chunk per origin, keeps its materials in separate pipelines and addresses its
//! atlas by tile. Three little-endian `u32` words (`Uint32x3`, one attribute):
//!
//! ```text
//! word 0   bits  0..9   x      (v + 8) * 16   : -8 <= v < 24 in steps of 1/16
//!          bits  9..18  z      (v + 8) * 16
//!          bits 18..31  y      (v + 8) * 32   : -8 <= v < 248 in steps of 1/32
//!          bit  31      repeat 1: the quad tiles its texture (a merged run of faces), 0: it shows the tile once
//! word 1   bits  0..10  tile   atlas tile number (row * 16 + column of the 16 x 16 texel tiles)
//!          bits 10..21  u      (texel + 240) * 4 : texels inside the tile, in quarter texels; a repeating quad runs past 0..16
//!          bits 21..32  v      (texel + 240) * 4
//! word 2   bits  0..5   red    5 bits   } sRGB colour multiplier (R5 G6 B5; white is exact)
//!          bits  5..11  green  6 bits   }
//!          bits 11..16  blue   5 bits   }
//!          bits 16..22  sky    quarter levels 0..=63 (four cells' sky levels summed; a flat face is `sky * 4`)
//!          bits 22..27  block  half levels 0..=31 (a flat face is `block * 2`)
//!          bits 27..32  shade  `face | ao << 2`: face shade 1, 0.8, 0.6, 0.5 times `1 - 0.2 * ao` (see `shade_value`)
//! ```
//!
//! Positions are relative to the chunk origin, which the host supplies per draw (Sodium's `u_RegionOffset`; a dynamic-offset uniform
//! here, push constants are not on GLES). x and z cover -8 <= v < 24 (Sodium's `MODEL_ORIGIN` 8 and `MODEL_RANGE` 32, so a model may
//! stick out of its chunk by 8 blocks each side) in steps of 1/16, the finest step of a block model; y covers -8 <= v < 248 in steps of
//! 1/32, so a fluid's corner is within 1/64 of its height. The 16-byte format this replaces had 1/2048 and 1/256 steps and a 16-bit
//! atlas coordinate per axis: nothing the mesher emits needs them, and 4 bytes less per vertex is 25 % less vertex fetch and binning
//! traffic on a tile-based GPU.
//!
//! The atlas is addressed by tile plus texel position inside it, so a texel position is a few bits and the host's shader can tile a
//! texture across a merged quad (`repeat`: it takes the fraction of the position, in tile units). Light is the two numbers the lighting
//! reads (sky and block level), shade the product of a face direction and an ambient occlusion step, both from tiny tables the shader
//! knows: the mesher's own flat and smooth light are the same encoding (a flat face is just a smooth one whose four corners agree).
//!
//! Not taken from Sodium: the texture centroid bias (`encodeTexture`) and the material and section bytes: the host's fragment shader
//! clamps every tap into its own tile, there is one pipeline per render layer and one origin per chunk.

pub const STRIDE: usize = 12;
/// One encoded vertex.
pub type Packed = [u8; STRIDE];

const ORIGIN: f32 = 8.0;
/// Steps per block in x and z, and in y.
pub const XZ_SCALE: f32 = 16.0;
pub const Y_SCALE: f32 = 32.0;
/// Quarter texels, biased so a repeating quad may run 15 tiles either way from the tile it starts in: `(texel + UV_BIAS) * UV_SCALE`.
pub const UV_SCALE: f32 = 4.0;
pub const UV_BIAS: f32 = 240.0;
/// The shade table: `FACE_SHADE[face] * (1 - AO_STEP * ao)`.
pub const FACE_SHADE: [f32; 4] = [1.0, 0.8, 0.6, 0.5];
pub const AO_STEP: f32 = 0.2;

pub struct Vertex {
    /// Relative to the chunk's origin in x and z, absolute in y: -8 <= x, z < 24 and -8 <= y < 248 (outside clamps).
    pub pos: [f32; 3],
    /// Atlas tile number, 0..1024.
    pub tile: u16,
    /// Texel position inside the tile, -240 <= v < 272 (0..=16 for a face that shows the tile once).
    pub uv: [f32; 2],
    /// The quad tiles its texture: the shader takes the fraction of `uv / 16`.
    pub repeat: bool,
    /// Sky light in quarter levels, 0..=63.
    pub sky: u8,
    /// Block light in half levels, 0..=31.
    pub block: u8,
    /// `face | ao << 2`, 0..=31.
    pub shade: u8,
    /// Colour multiplier, sRGB bytes (stored as R5 G6 B5).
    pub color: [u8; 3],
}

pub fn encode(v: &Vertex) -> Packed {
    let w0 = quantize(v.pos[0], XZ_SCALE, 511) | quantize(v.pos[2], XZ_SCALE, 511) << 9 | quantize(v.pos[1], Y_SCALE, 8191) << 18 | (v.repeat as u32) << 31;
    let w1 = (v.tile as u32 & 1023) | texel(v.uv[0]) << 10 | texel(v.uv[1]) << 21;
    let (r, g, b) = ((v.color[0] as u32 * 31 + 127) / 255, (v.color[1] as u32 * 63 + 127) / 255, (v.color[2] as u32 * 31 + 127) / 255);
    let w2 = r | g << 5 | b << 11 | (v.sky.min(63) as u32) << 16 | (v.block.min(31) as u32) << 22 | (v.shade.min(31) as u32) << 27;
    let mut out = [0u8; STRIDE];
    out[0..4].copy_from_slice(&w0.to_le_bytes());
    out[4..8].copy_from_slice(&w1.to_le_bytes());
    out[8..12].copy_from_slice(&w2.to_le_bytes());
    out
}

/// What the vertex shader reads back (the encoding's own rounding included): used by the host's tests.
pub fn decode(b: &Packed) -> Vertex {
    let w = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let (w0, w1, w2) = (w(0), w(4), w(8));
    let c = |v: u32, bits: u32| (v * 255 + ((1 << bits) - 1) / 2) / ((1 << bits) - 1);
    Vertex {
        pos: [dequantize(w0 & 511, XZ_SCALE), dequantize(w0 >> 18 & 8191, Y_SCALE), dequantize(w0 >> 9 & 511, XZ_SCALE)],
        tile: (w1 & 1023) as u16,
        uv: [(w1 >> 10 & 2047) as f32 / UV_SCALE - UV_BIAS, (w1 >> 21) as f32 / UV_SCALE - UV_BIAS],
        repeat: w0 >> 31 != 0,
        sky: (w2 >> 16 & 63) as u8,
        block: (w2 >> 22 & 31) as u8,
        shade: (w2 >> 27) as u8,
        color: [c(w2 & 31, 5) as u8, c(w2 >> 5 & 63, 6) as u8, c(w2 >> 11 & 31, 5) as u8],
    }
}

/// The factor a shade code stands for: `FACE_SHADE[code & 3] * (1 - AO_STEP * (code >> 2))`.
pub fn shade_value(code: u8) -> f32 {
    FACE_SHADE[(code & 3) as usize] * (1.0 - AO_STEP * (code >> 2) as f32)
}

/// The code whose factor is nearest to `v` (the mesher's face shade times a mean of cells weighing 1 or 0.2 is exactly one of them).
pub fn shade_code(v: f32) -> u8 {
    (0..20u8)
        .min_by(|&a, &b| (shade_value(a) - v).abs().total_cmp(&(shade_value(b) - v).abs()))
        .unwrap()
}

/// `(v + 8) * scale`, rounded to the nearest step; the float to integer cast saturates (negative is 0, NaN is 0), `max` is the top.
fn quantize(v: f32, scale: f32, max: u32) -> u32 {
    (((ORIGIN + v) * scale + 0.5) as u32).min(max)
}

/// A position component as the GPU decodes it: `value / scale - 8`. Both factors are powers of two, so this is exact.
pub fn dequantize(v: u32, scale: f32) -> f32 {
    v as f32 / scale - ORIGIN
}

fn texel(v: f32) -> u32 {
    (((v + UV_BIAS) * UV_SCALE + 0.5) as u32).min(2047)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vx() -> Vertex {
        Vertex { pos: [0.0, 8.5, -8.0], tile: 700, uv: [0.5, 15.75], repeat: false, sky: 60, block: 3, shade: 6, color: [255, 130, 0] }
    }

    #[test]
    fn layout_and_round_trip() {
        let b = encode(&vx());
        let w = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        assert_eq!(w(0), 128 | 0 << 9 | 528 << 18, "x (8 + 0) * 16, z 0, y (8 + 8.5) * 32");
        assert_eq!(w(4), 700 | 962 << 10 | 1023 << 21, "(0.5 + 240) * 4, (15.75 + 240) * 4");
        assert_eq!(w(8) >> 16, 60 | 3 << 6 | 6 << 11);
        let d = decode(&b);
        assert_eq!((d.pos, d.tile, d.uv, d.repeat, d.sky, d.block, d.shade), ([0.0, 8.5, -8.0], 700, [0.5, 15.75], false, 60, 3, 6));
        assert_eq!(d.color, [255, 130, 0], "the ends are exact, 130 is a step of the 6-bit green");
        assert_eq!(decode(&encode(&Vertex { repeat: true, ..vx() })).repeat, true);
        assert_eq!(decode(&encode(&Vertex { color: [255; 3], ..vx() })).color, [255; 3]);
    }

    /// The error of a position is at most half a step, and every multiple of 1/16 (block models, tile windows) is exact.
    #[test]
    fn position_error_is_half_a_step_and_sixteenths_are_exact() {
        for i in 0..2000 {
            let x = -8.0 + i as f32 * 0.0159;
            assert!((dequantize(quantize(x, XZ_SCALE, 511), XZ_SCALE) - x).abs() <= 0.51 / XZ_SCALE);
            let y = -8.0 + i as f32 * 0.127;
            assert!((dequantize(quantize(y, Y_SCALE, 8191), Y_SCALE) - y).abs() <= 0.51 / Y_SCALE);
        }
        for k in -16..(16 * 24) {
            let v = k as f32 / 16.0;
            assert_eq!(dequantize(quantize(v, XZ_SCALE, 511), XZ_SCALE), v);
            assert_eq!(dequantize(quantize(v, Y_SCALE, 8191), Y_SCALE), v);
        }
        assert_eq!(dequantize(quantize(127.0 + 1.0 / 9.0, Y_SCALE, 8191), Y_SCALE), 127.125); // a fluid corner lands on the 1/32 grid
        assert_eq!(dequantize(quantize(23.9999, XZ_SCALE, 511), XZ_SCALE), 23.9375, "the top step is 511 / 16 - 8");
    }

    /// Out of range clamps (a model sticking out by more than 8 blocks), it does not wrap onto the other side of the chunk.
    #[test]
    fn out_of_range_clamps() {
        assert_eq!(quantize(-100.0, XZ_SCALE, 511), 0);
        assert_eq!(quantize(100.0, XZ_SCALE, 511), 511);
        assert_eq!(quantize(f32::NAN, Y_SCALE, 8191), 0);
        assert_eq!(texel(-1000.0), 0);
        assert_eq!(texel(1000.0), 2047);
    }

    /// A face's texel positions (0 and 16, the 0.02 inset of `atlas::terrain_uv`) and a merged run's (+- 15 tiles) survive the bias.
    #[test]
    fn texel_positions_cover_a_merged_run() {
        for t in [-224.0f32, -16.0, 0.0, 0.02, 15.98, 16.0, 255.98] {
            let d = decode(&encode(&Vertex { uv: [t, t], ..vx() })).uv[0];
            assert!((d - t).abs() <= 0.126, "{t} -> {d}");
        }
    }

    /// Every shade the mesher can make is one code: four face shades times a mean of cells weighing 1 or 0.2.
    #[test]
    fn shade_codes_are_exact() {
        for (f, face) in FACE_SHADE.iter().enumerate() {
            for ao in 0..5 {
                let want = face * (1.0 - 0.2 * ao as f32);
                let code = shade_code(want);
                assert!((shade_value(code) - want).abs() < 1e-6, "{want}: code {code}");
            }
            assert_eq!(shade_value(f as u8), *face);
        }
        assert_eq!(shade_code(1.0), 0);
        assert!(shade_value(shade_code(0.48)) - 0.48 < 1e-6);
    }
}
