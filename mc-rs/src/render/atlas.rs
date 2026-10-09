//! M3e-atlas (minimal): 16x16 block-id atlas, 1 pixel per block id.
//! Each block gets a single solid color. No fancy dither.
//! Texture: 16x16 RGBA = 1 KB. Safe on any GPU.

pub type Pixel = [u8; 4];

/// Block id -> RGBA color. Vanilla-Beta-1.7 colors (approximate).
pub fn block_color(id: u8) -> Pixel {
    match id {
        0 => [0, 0, 0, 0],
        1 => [125, 125, 125, 255],   // stone
        2 => [110, 170, 70, 255],    // grass
        3 => [134, 96, 67, 255],     // dirt
        4 => [200, 200, 200, 255],   // cobblestone
        5 => [220, 200, 100, 255],   // planks
        7 => [55, 55, 55, 255],      // bedrock
        8 | 9 => [60, 110, 200, 255], // water (moving, still)
        10 | 11 => [220, 90, 30, 255], // lava (moving, still)
        12 => [225, 215, 160, 255],  // sand
        13 => [140, 130, 120, 255],  // gravel
        14 => [240, 220, 60, 255],   // gold ore
        15 => [200, 170, 130, 255],  // iron ore
        16 => [60, 60, 60, 255],     // coal ore
        17 => [110, 80, 50, 255],    // log
        18 => [60, 130, 40, 255],    // leaves
        21 => [50, 80, 180, 255],    // lapis
        24 => [219, 211, 160, 255],  // sandstone
        48 => [110, 140, 110, 255],  // mossy cobblestone
        52 => [30, 40, 50, 255],     // spawner
        54 => [150, 100, 40, 255],   // chest
        81 => [20, 120, 40, 255],    // cactus
        82 => [160, 165, 180, 255],  // clay
        86 => [215, 130, 20, 255],   // pumpkin
        56 => [180, 240, 240, 255],  // diamond
        73 => [200, 60, 60, 255],    // redstone
        79 => [180, 200, 220, 255],  // ice
        31 => [90, 140, 50, 255],    // tall grass
        32 => [120, 90, 40, 255],    // dead bush
        37 => [240, 230, 60, 255],   // dandelion
        38 => [200, 30, 30, 255],    // rose
        39 => [140, 105, 80, 255],   // brown mushroom
        40 => [200, 40, 40, 255],    // red mushroom
        78 => [245, 250, 255, 255],  // snow layer
        // Blocks and item stand-ins M6 can make; UNVERIFIED colours, like the rest until M14 textures.
        20 => [200, 230, 240, 255],  // glass
        35 => [235, 235, 235, 255],  // wool
        41 => [250, 236, 80, 255],   // gold block (also gold tools)
        42 => [225, 225, 225, 255],  // iron block (also iron tools)
        57 => [100, 230, 215, 255],  // diamond block (also diamond tools)
        58 => [150, 105, 60, 255],   // workbench
        61 => [105, 105, 105, 255],  // furnace
        62 => [230, 130, 40, 255],   // lit furnace
        80 => [250, 252, 255, 255],  // snow block
        89 => [245, 220, 120, 255],  // glowstone
        83 => [140, 190, 100, 255],  // reeds
        _ => [180, 30, 200, 255],    // unknown = magenta
    }
}

/// 16x16 atlas, 1 pixel per block id. Returns 16*16*4 = 1024 bytes RGBA.
pub fn atlas_rgba() -> Vec<u8> {
    const W: usize = 16;
    const H: usize = 16;
    let mut out = vec![0u8; W * H * 4];
    for id in 0u16..256 {
        let id = id as u8;
        let x = (id % 16) as usize;
        let y = (id / 16) as usize;
        let c = block_color(id);
        let off = (y * W + x) * 4;
        out[off] = c[0];
        out[off + 1] = c[1];
        out[off + 2] = c[2];
        out[off + 3] = c[3];
    }
    out
}

/// Convert block id + face UV (0..1 in tile space) to atlas UV.
/// In the 1px-per-block atlas, the UVs of each face are all 0.0 or 1.0
/// (corners of the 1x1 tile), and `atlas_uv` just remaps to the
/// (block_id%16, block_id/16) tile origin.
pub fn atlas_uv(block_id: u8, _u: f32, _v: f32) -> (f32, f32) {
    // The atlas is 1 texel per block id, so any UV inside this block's tile
    // samples the same color. We snap to the texel center for a Nearest
    // sampler: (tile_x + 0.5) / 16. Both face corners (u, v in {0, 1}) end
    // up at the same sample, which is what we want for a flat-shaded atlas.
    let tile_x = (block_id % 16) as f32;
    let tile_y = (block_id / 16) as f32;
    let au = (tile_x + 0.5) / 16.0;
    let av = (tile_y + 0.5) / 16.0;
    (au, av)
}
