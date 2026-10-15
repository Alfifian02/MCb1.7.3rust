//! `CompactChunkVertex`: 20 bytes per terrain vertex (the host's float vertex is 32).
//! Positions are 16-bit fixed point over a 32-block range centred on the section (so a model may stick out of its section by
//! up to 8 blocks each side), texture coordinates 15-bit fixed point. All little endian; every attribute sits on an offset that is a
//! multiple of its size and the stride is a multiple of 4, which is all Vulkan and GLES vertex fetch ask for.
//!
//! wgpu layout: `Uint16x4` @0 (xyz + material, section index), `Unorm8x4` @8 (colour), `Uint16x2` @12 (texture),
//! `Uint16x2` @16 (light).

pub const STRIDE: usize = 20;
const MODEL_ORIGIN: f32 = 8.0;
const MODEL_RANGE: f32 = 32.0;
const POSITION_MAX: f32 = 65536.0;
const TEXTURE_MAX: f32 = 32768.0;

pub struct Vertex {
    /// Relative to the section origin: -8.0 <= v < 24.0.
    pub pos: [f32; 3],
    /// `0..1`-ish atlas coordinates (up to 1.0 is fine).
    pub uv: [f32; 2],
    /// Colour (ABGR in memory order r, g, b, a) times shade.
    pub color: u32,
    /// Block light and sky light, two 16-bit values.
    pub light: u32,
    pub material: u8,
    /// Index of the section inside its region; the host may leave it 0.
    pub section: u8,
}

pub fn encode(v: &Vertex) -> [u8; STRIDE] {
    let mut b = [0u8; STRIDE];
    for i in 0..3 {
        b[i * 2..i * 2 + 2].copy_from_slice(&position(v.pos[i]).to_le_bytes());
    }
    b[6] = v.material;
    b[7] = v.section;
    b[8..12].copy_from_slice(&v.color.to_le_bytes());
    b[12..14].copy_from_slice(&texture(v.uv[0]).to_le_bytes());
    b[14..16].copy_from_slice(&texture(v.uv[1]).to_le_bytes());
    b[16..20].copy_from_slice(&v.light.to_le_bytes());
    b
}

/// A position component as the GPU decodes it: `value / 2048 - 8`.
pub fn decode_position(v: u16) -> f32 {
    v as f32 / (POSITION_MAX / MODEL_RANGE) - MODEL_ORIGIN
}

fn position(v: f32) -> u16 {
    ((MODEL_ORIGIN + v) * (POSITION_MAX / MODEL_RANGE)) as u16 // saturating: out of range clamps instead of wrapping
}

fn texture(v: f32) -> u16 {
    (v * TEXTURE_MAX + 0.5).floor() as i64 as u16 // Math.round, wrapped like `& 0xFFFF`
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_and_round_trip() {
        let b = encode(&Vertex { pos: [0.0, 8.5, -8.0], uv: [0.5, 1.0], color: 0x04030201, light: 0x0008_0004, material: 7, section: 9 });
        assert_eq!(u16::from_le_bytes([b[0], b[1]]), 16384); // (8 + 0) * 2048
        assert_eq!(decode_position(u16::from_le_bytes([b[2], b[3]])), 8.5);
        assert_eq!(u16::from_le_bytes([b[4], b[5]]), 0);
        assert_eq!((b[6], b[7]), (7, 9));
        assert_eq!(&b[8..12], &[1, 2, 3, 4]);
        assert_eq!(u16::from_le_bytes([b[12], b[13]]), 16384);
        assert_eq!(u16::from_le_bytes([b[14], b[15]]), 32768);
        assert_eq!(u32::from_le_bytes([b[16], b[17], b[18], b[19]]), 0x0008_0004);
    }

    #[test]
    fn position_error_is_under_one_2048th_of_a_block() {
        for i in 0..1000 {
            let v = -8.0 + i as f32 * 0.0317;
            assert!((decode_position(position(v)) - v).abs() < 1.0 / 2048.0);
        }
    }
}
