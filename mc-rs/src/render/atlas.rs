//! M3e-atlas (minimal): 16x32 atlas, 1 pixel per tile. Tiles 0..=255 are the block ids, tiles 256.. the
//! metadata variants that look different (log species, leaf kinds, wool colours; see `tile_of`).
//! Each tile is a single solid color. No fancy dither.
//! Texture: 16x32 RGBA = 2 KB. Safe on any GPU.

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
        60 => [95, 62, 38, 255],     // farmland (tilled by a hoe)
        61 => [105, 105, 105, 255],  // furnace
        62 => [230, 130, 40, 255],   // lit furnace
        80 => [250, 252, 255, 255],  // snow block
        89 => [245, 220, 120, 255],  // glowstone
        83 => [140, 190, 100, 255],  // reeds
        _ => [180, 30, 200, 255],    // unknown = magenta
    }
}

/// Atlas size in tiles (one texel each): 256 block-id tiles, then the variants.
pub const TILES_W: usize = 16;
pub const TILES_H: usize = 32;

/// `EntitySheep.fleeceColorTable`: the wool colours, indexed by the cloth metadata (0 white .. 15 black).
const FLEECE: [[f32; 3]; 16] = [
    [1.0, 1.0, 1.0], [0.95, 0.7, 0.2], [0.9, 0.5, 0.85], [0.6, 0.7, 0.95], [0.9, 0.9, 0.2], [0.5, 0.8, 0.1],
    [0.95, 0.7, 0.8], [0.3, 0.3, 0.3], [0.6, 0.6, 0.6], [0.3, 0.6, 0.7], [0.7, 0.4, 0.9], [0.2, 0.4, 0.8],
    [0.5, 0.4, 0.3], [0.4, 0.5, 0.2], [0.8, 0.3, 0.3], [0.1, 0.1, 0.1],
];

/// Tile of a block with its metadata: the block id's own tile, or one of the tiles from 256 up for a variant that
/// looks different. Log 17: 1 spruce (256), 2 birch (257), else oak (BlockLog textures). Leaves 18, `meta & 3`
/// (bit 8 is the placed-by-player mark): 1 spruce (258), 2 birch (259). Wool 35: 1..=15 are 260..=274, 0 is white.
pub fn tile_of(id: u8, meta: u8) -> u16 {
    match (id, meta) {
        (17, 1..=2) => 255 + meta as u16,
        (18, _) if matches!(meta & 3, 1 | 2) => 257 + (meta & 3) as u16,
        (35, 1..=15) => 259 + meta as u16,
        _ => id as u16,
    }
}

/// Tile -> RGBA. Tiles nothing hands out are magenta.
// UNVERIFIED: the log and leaf variants (vanilla uses textures; its leaves are tinted by `ColorizerFoliage`:
// the spruce and birch leaf colours are 0x619961 and 0x80A755 scaled by 0.7, the rest is a guess).
pub fn tile_color(tile: u16) -> Pixel {
    match tile {
        0..=255 => block_color(tile as u8),
        256 => [75, 55, 35, 255],    // spruce log
        257 => [215, 215, 205, 255], // birch log
        258 => [68, 107, 68, 255],   // spruce leaves
        259 => [90, 117, 60, 255],   // birch leaves
        260..=274 => {
            let c = FLEECE[(tile - 259) as usize];
            [(c[0] * 255.0 + 0.5) as u8, (c[1] * 255.0 + 0.5) as u8, (c[2] * 255.0 + 0.5) as u8, 255]
        }
        _ => [180, 30, 200, 255],
    }
}

/// The whole atlas, row by row (tile `t` is texel `t`): `TILES_W * TILES_H * 4` bytes of RGBA.
pub fn atlas_rgba() -> Vec<u8> {
    (0..(TILES_W * TILES_H) as u16).flat_map(tile_color).collect()
}

/// Atlas UV of a tile. The atlas is 1 texel per tile, so every UV inside a tile samples the same color:
/// snap to the texel center for a Nearest sampler, `(tile % 16 + 0.5) / 16`, `(tile / 16 + 0.5) / 32`.
/// Both face corners (u, v in {0, 1}) end up at the same sample, which is what a flat-shaded atlas wants.
pub fn atlas_uv(tile: u16, _u: f32, _v: f32) -> (f32, f32) {
    (((tile % 16) as f32 + 0.5) / TILES_W as f32, ((tile / 16) as f32 + 0.5) / TILES_H as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Metadata picks the tile: species and wool colours get their own, everything else keeps the block id's tile.
    #[test]
    fn metadata_picks_the_tile() {
        assert_eq!((tile_of(17, 0), tile_of(17, 1), tile_of(17, 2), tile_of(17, 3)), (17, 256, 257, 17));
        assert_eq!((tile_of(18, 0), tile_of(18, 1 | 8), tile_of(18, 2 | 8), tile_of(18, 3)), (18, 258, 259, 18));
        assert_eq!((tile_of(35, 0), tile_of(35, 1), tile_of(35, 15), tile_of(1, 7)), (35, 260, 274, 1));
        // Every tile `tile_of` hands out has its own colour, and the texture covers all of them.
        assert_eq!(atlas_rgba().len(), TILES_W * TILES_H * 4);
        assert_ne!(tile_color(260), tile_color(261));
        assert_ne!(tile_color(256), tile_color(17));
        assert!(tile_color(274)[3] == 255 && (25..=26).contains(&tile_color(274)[0])); // black wool
    }
}
