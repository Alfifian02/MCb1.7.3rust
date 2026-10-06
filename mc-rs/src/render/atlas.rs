//! M3e-atlas: 16x16 block-id atlas, 16x16 pixels per tile.
//! Total texture: 256x256 pixels = 64 KB RGBA (fits in any GPU).
//! Block id -> tile coords: (id % 16, id / 16) = (tile_x, tile_y).
//! Each tile is filled with the block's color, with a slight noise pattern
//! for visual variety.

pub type Pixel = [u8; 4];

/// Block id -> RGBA color. Vanilla-Beta-1.7 colors (approximate).
/// 0 (air) and unknown ids return a sentinel; the mesher shouldn't draw those.
pub fn block_color(id: u8) -> Pixel {
    match id {
        0 => [0, 0, 0, 0],
        1 => [125, 125, 125, 255],   // stone (grey)
        2 => [110, 170, 70, 255],    // grass (green)
        3 => [134, 96, 67, 255],     // dirt (brown)
        4 => [200, 200, 200, 255],   // cobblestone
        5 => [220, 200, 100, 255],   // planks (light wood)
        7 => [55, 55, 55, 255],      // bedrock (dark)
        8 => [60, 110, 200, 255],    // water (blue)
        10 => [220, 90, 30, 255],    // lava (orange-red)
        12 => [225, 215, 160, 255],  // sand
        13 => [140, 130, 120, 255],  // gravel
        14 => [240, 220, 60, 255],   // gold ore (yellow flecks)
        15 => [200, 170, 130, 255],  // iron ore (tan flecks)
        16 => [60, 60, 60, 255],     // coal ore (black)
        17 => [110, 80, 50, 255],    // log (brown)
        21 => [50, 80, 180, 255],    // lapis (deep blue)
        56 => [180, 240, 240, 255],  // diamond (cyan)
        73 => [200, 60, 60, 255],    // redstone (red)
        79 => [180, 200, 220, 255],  // ice (light blue)
        _ => [180, 30, 200, 255],    // unknown = magenta (debug)
    }
}

/// 256x256 atlas, 16 tiles wide, each tile 16x16. Each tile is filled with
/// the block color, with a per-pixel ±8 luminance variation for texture.
pub fn atlas_rgba() -> Vec<u8> {
    const W: usize = 16 * 16; // 256
    const H: usize = 16 * 16; // 256
    let mut out = vec![0u8; W * H * 4];
    for id in 0u8..=u8::MAX {
        let tile_x = (id % 16) as usize;
        let tile_y = (id / 16) as usize;
        let base = block_color(id);
        for dy in 0..16 {
            for dx in 0..16 {
                let x = tile_x * 16 + dx;
                let y = tile_y * 16 + dy;
                let off = (y * W + x) * 4;
                // Per-pixel dither: darken by 0/4/8 depending on (dx*dy) % 3.
                let dither = ((dx.wrapping_mul(13) ^ dy.wrapping_mul(7)) % 16) as i32 - 8;
                for c in 0..3 {
                    let v = base[c] as i32 + dither;
                    out[off + c] = v.clamp(0, 255) as u8;
                }
                out[off + 3] = base[3];
            }
        }
    }
    out
}

/// Convert block id + face UV (0..1 in tile space) to atlas UV (0..1 in atlas).
pub fn atlas_uv(block_id: u8, u: f32, v: f32) -> (f32, f32) {
    let tile_x = (block_id % 16) as f32;
    let tile_y = (block_id / 16) as f32;
    let u = (tile_x + u) / 16.0;
    let v = (tile_y + v) / 16.0;
    (u, v)
}
