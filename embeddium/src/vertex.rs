//! The 16-byte terrain vertex: Sodium's `CompactChunkVertex` (20 bytes: 20-bit positions, 15-bit texture coordinates, light,
//! material and section index) cut down for a host that draws one chunk per origin and keeps its materials in separate pipelines.
//!
//! ```text
//! offset  size  wgpu format  content
//!      0     2  Uint16x4     x, y, z (fixed point, see below), light (16 bits, the host's own packing)
//!      8     4  Uint16x2     u, v (atlas coordinates x 65536)
//!     12     4  Unorm8x4     r, g, b (colour multiplier), shade (a free 0..1 factor, the host's: ambient occlusion)
//! ```
//!
//! Positions are relative to the chunk origin, which the host supplies per draw (Sodium's `u_RegionOffset`; a dynamic-offset
//! uniform here, push constants are not on GLES). x and z cover -8 <= v < 24 in steps of 1/2048 (Sodium's `MODEL_ORIGIN` 8 and
//! `MODEL_RANGE` 32, so a model may stick out of its chunk by 8 blocks each side); y covers -8 <= v < 248 in steps of 1/256,
//! enough for a 128-block column and exact for every multiple of 1/16, the finest step of a block model. Texture coordinates are
//! the whole 16 bits: tile edges (multiples of 1/16 over a 256-wide atlas) are exact. Every attribute sits on an offset that is a
//! multiple of its size and the stride is a multiple of 4, which is all Vulkan and GLES vertex fetch ask for. Little endian.
//!
//! Not taken from Sodium: the texture centroid bias (`encodeTexture` shrinks a coordinate by one unit towards its quad's centre and
//! stores the sign, so the shader can use a far smaller epsilon): the host's fragment shader clamps every tap into its own tile
//! instead. The material and section bytes: one pipeline per render layer, one origin per chunk.

pub const STRIDE: usize = 16;
/// One encoded vertex.
pub type Packed = [u8; STRIDE];

const ORIGIN: f32 = 8.0;
/// 65536 / 32 blocks.
pub const XZ_SCALE: f32 = 2048.0;
/// 65536 / 256 blocks.
pub const Y_SCALE: f32 = 256.0;
pub const UV_SCALE: f32 = 65536.0;

pub struct Vertex {
    /// Relative to the chunk's origin in x and z, absolute in y: -8 <= x, z < 24 and -8 <= y < 248 (outside clamps).
    pub pos: [f32; 3],
    /// Atlas coordinates, 0..=1 (1.0 stores as 65535 / 65536).
    pub uv: [f32; 2],
    /// Sixteen bits for the host's shader.
    pub light: u16,
    /// Colour multiplier, sRGB bytes.
    pub color: [u8; 3],
    /// A factor 0..=255 for the host's shader (`/ 255`).
    pub shade: u8,
}

pub fn encode(v: &Vertex) -> Packed {
    let mut b = [0u8; STRIDE];
    b[0..2].copy_from_slice(&quantize(v.pos[0], XZ_SCALE).to_le_bytes());
    b[2..4].copy_from_slice(&quantize(v.pos[1], Y_SCALE).to_le_bytes());
    b[4..6].copy_from_slice(&quantize(v.pos[2], XZ_SCALE).to_le_bytes());
    b[6..8].copy_from_slice(&v.light.to_le_bytes());
    b[8..10].copy_from_slice(&texture(v.uv[0]).to_le_bytes());
    b[10..12].copy_from_slice(&texture(v.uv[1]).to_le_bytes());
    b[12..15].copy_from_slice(&v.color);
    b[15] = v.shade;
    b
}

/// What the vertex shader reads back (the encoding's own rounding included): used by the host's tests.
pub fn decode(b: &Packed) -> Vertex {
    let u = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    Vertex {
        pos: [dequantize(u(0), XZ_SCALE), dequantize(u(2), Y_SCALE), dequantize(u(4), XZ_SCALE)],
        uv: [u(8) as f32 / UV_SCALE, u(10) as f32 / UV_SCALE],
        light: u(6),
        color: [b[12], b[13], b[14]],
        shade: b[15],
    }
}

/// `(v + 8) * scale`, rounded to the nearest step; the float to integer cast saturates (out of range clamps instead of wrapping,
/// NaN is 0).
fn quantize(v: f32, scale: f32) -> u16 {
    ((ORIGIN + v) * scale + 0.5) as u16
}

/// A position component as the GPU decodes it: `value / scale - 8`. Both factors are powers of two, so this is exact.
pub fn dequantize(v: u16, scale: f32) -> f32 {
    v as f32 / scale - ORIGIN
}

fn texture(v: f32) -> u16 {
    (v * UV_SCALE + 0.5) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_and_round_trip() {
        let b = encode(&Vertex { pos: [0.0, 8.5, -8.0], uv: [0.5, 0.0625], light: 0x8123, color: [1, 2, 3], shade: 200 });
        assert_eq!(u16::from_le_bytes([b[0], b[1]]), 16384); // (8 + 0) * 2048
        assert_eq!(u16::from_le_bytes([b[2], b[3]]), 4224); // (8 + 8.5) * 256
        assert_eq!(u16::from_le_bytes([b[4], b[5]]), 0);
        assert_eq!(u16::from_le_bytes([b[6], b[7]]), 0x8123);
        assert_eq!(u16::from_le_bytes([b[8], b[9]]), 32768);
        assert_eq!(u16::from_le_bytes([b[10], b[11]]), 4096); // 1/16 of the atlas
        assert_eq!(&b[12..16], &[1, 2, 3, 200]);
        let d = decode(&b);
        assert_eq!((d.pos, d.uv, d.light, d.color, d.shade), ([0.0, 8.5, -8.0], [0.5, 0.0625], 0x8123, [1, 2, 3], 200));
    }

    /// The error of a position is at most half a step, and every multiple of 1/16 (block models, tile windows) is exact.
    #[test]
    fn position_error_is_half_a_step_and_sixteenths_are_exact() {
        for i in 0..2000 {
            let x = -8.0 + i as f32 * 0.0159;
            assert!((dequantize(quantize(x, XZ_SCALE), XZ_SCALE) - x).abs() <= 0.51 / XZ_SCALE);
            let y = -8.0 + i as f32 * 0.127;
            assert!((dequantize(quantize(y, Y_SCALE), Y_SCALE) - y).abs() <= 0.51 / Y_SCALE);
        }
        for k in -16..(16 * 24) {
            let v = k as f32 / 16.0;
            assert_eq!(dequantize(quantize(v, XZ_SCALE), XZ_SCALE), v);
            assert_eq!(dequantize(quantize(v, Y_SCALE), Y_SCALE), v);
        }
        assert_eq!(dequantize(quantize(127.0 + 1.0 / 9.0, Y_SCALE), Y_SCALE), 127.109375); // a fluid corner lands on the 1/256 grid
    }

    /// Out of range clamps (a model sticking out by more than 8 blocks), it does not wrap onto the other side of the chunk.
    #[test]
    fn out_of_range_clamps() {
        assert_eq!(quantize(-100.0, XZ_SCALE), 0);
        assert_eq!(quantize(100.0, XZ_SCALE), u16::MAX);
        assert_eq!(quantize(f32::NAN, Y_SCALE), 0);
        assert_eq!(texture(1.0), u16::MAX);
    }

    /// Tile edges of the 256-wide atlas are exact, so the shader's `u32(uv.x * 16)` tile test never lands one tile short.
    #[test]
    fn tile_edges_are_exact() {
        for t in 0..16u32 {
            let u = t as f32 / 16.0;
            assert_eq!(texture(u) as f32 / UV_SCALE, u);
        }
    }
}
