//! The atlas texture is 256 x 288: the real b1.7.3 `terrain.png` (16x16 tiles of 16x16 px, grass/foliage tint baked in by
//! `tools/gen_terrain.py`) on top, and under it a 16x32 strip with one flat-colour pixel per old tile (block ids 0..=255,
//! then the metadata variants 256.. of `tile_of`) for everything that has no terrain tile: dropped items, mobs, falling
//! blocks and blocks `terrain_tile` does not know. Texture: 256 x 288 RGBA = 288 KB.

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

/// Flat-colour strip size in tiles (one texel each): 256 block-id tiles, then the variants.
pub const TILES_W: usize = 16;
pub const TILES_H: usize = 32;
/// The whole texture: `terrain.png` (256 px) with the strip under it.
pub const ATLAS_W: usize = 256;
pub const ATLAS_H: usize = 256 + TILES_H;
const TERRAIN: &[u8] = include_bytes!("../../assets/terrain.rgba");

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

/// The whole atlas, row by row: `terrain.png`, then the flat strip (tile `t` is strip texel `t`; the rest of a strip row is clear).
pub fn atlas_rgba() -> Vec<u8> {
    let mut v = TERRAIN.to_vec();
    for t in 0..(TILES_W * TILES_H) as u16 {
        let c = tile_color(t);
        v.extend([c[0], c[1], c[2], 255]); // opaque: the shader cuts out alpha < 0.5
        if t as usize % TILES_W == TILES_W - 1 {
            v.resize(v.len() + (ATLAS_W - TILES_W) * 4, 0);
        }
    }
    v
}

/// Atlas UV of a flat-colour tile: the centre of its strip texel (Nearest sampler), so every UV in the tile samples one colour.
pub fn atlas_uv(tile: u16, _u: f32, _v: f32) -> (f32, f32) {
    (((tile % 16) as f32 + 0.5) / ATLAS_W as f32, (256.0 + (tile / 16) as f32 + 0.5) / ATLAS_H as f32)
}

/// Atlas UV of terrain tile `t` at (u, v) in 0..1, v up like the mesher's faces (the png's rows run down). Inset by a fiftieth
/// of a texel so a Nearest sampler never reads the neighbouring tile.
pub fn terrain_uv(t: u8, u: f32, v: f32) -> (f32, f32) {
    let (x, y) = ((t % 16) as f32 * 16.0 + 0.02 + u * 15.96, (t / 16) as f32 * 16.0 + 0.02 + (1.0 - v) * 15.96);
    (x / ATLAS_W as f32, y / ATLAS_H as f32)
}

/// `Block.getBlockTextureFromSideAndMetadata`: the `terrain.png` tile of a block's face, `side` as in the Java (0 bottom, 1 top,
/// 2 north -Z, 3 south +Z, 4 west -X, 5 east +X); `None` = no tile known (flat colour). Blocks with a facing in the Java
/// (furnace, pumpkin, chest) get their front on +Z, because this port does not keep the facing yet. 31 tall grass is 55 dead
/// shrub / 39 grass / 56 fern by metadata; wool, log, leaves and sapling pick by metadata like `tile_of` does.
pub const fn terrain_tile(id: u8, meta: u8, side: u8) -> Option<u8> {
    let (top, bottom) = (side == 1, side == 0);
    Some(match id {
        1 => 1,
        2 => if top { 0 } else if bottom { 2 } else { 3 },
        3 => 2,
        4 => 16,
        5 | 85 => 4,
        6 => match meta & 3 { 1 => 63, 2 => 79, _ => 15 },
        7 => 17,
        8 | 9 => 205,
        10 | 11 => 237,
        12 => 18,
        13 => 19,
        14 => 32,
        15 => 33,
        16 => 34,
        17 => if top || bottom { 21 } else { match meta { 1 => 116, 2 => 117, _ => 20 } },
        18 => match meta & 3 { 1 => 133, 2 => 255, _ => 53 },
        19 => 48,
        20 => 49,
        21 => 160,
        22 => 144,
        24 => if top { 176 } else if bottom { 208 } else { 192 },
        31 => match meta { 1 => 39, 2 => 56, _ => 55 },
        32 => 55,
        35 => if meta == 0 { 64 } else { let m = !(meta & 15); 113 + ((m & 8) >> 3) + (m & 7) * 16 },
        37 => 13,
        38 => 12,
        39 => 29,
        40 => 28,
        41 => 23,
        42 => 22,
        43 | 44 => match meta { 0 => if side <= 1 { 6 } else { 5 }, 1 => if bottom { 208 } else if top { 176 } else { 192 }, 2 => 4, 3 => 16, _ => 6 },
        45 => 7,
        46 => if bottom { 10 } else if top { 9 } else { 8 },
        47 => if side <= 1 { 4 } else { 35 },
        48 => 36,
        49 => 37,
        52 => 65,
        54 => if side <= 1 { 25 } else if side == 3 { 27 } else { 26 },
        56 => 50,
        57 => 24,
        58 => if top { 43 } else if bottom { 4 } else if side == 2 || side == 4 { 60 } else { 59 },
        60 => if top { if meta > 0 { 86 } else { 87 } } else { 2 },
        61 => if side <= 1 { 62 } else if side == 3 { 44 } else { 45 },
        62 => if side <= 1 { 62 } else if side == 3 { 61 } else { 45 },
        73 | 74 => 51,
        79 => 67,
        80 => 66,
        81 => if top { 69 } else if bottom { 71 } else { 70 },
        82 => 72,
        83 => 73,
        84 => if top { 75 } else { 74 },
        86 | 91 => if side <= 1 { 102 } else if side == 3 { if id == 91 { 120 } else { 119 } } else { 118 },
        87 => 103,
        88 => 104,
        89 => 105,
        _ => return None,
    })
}

/// UV of a corner of a block face: its terrain tile, or the flat colour of `tile_of` where there is none. `face` is the
/// mesher's order (+X, -X, +Y, -Y, +Z, -Z).
pub fn face_uv(id: u8, meta: u8, face: usize, u: f32, v: f32) -> (f32, f32) {
    match terrain_tile(id, meta, [5, 4, 1, 0, 3, 2][face]) {
        Some(t) => terrain_uv(t, u, v),
        None => atlas_uv(tile_of(id, meta), u, v),
    }
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
        assert_eq!(atlas_rgba().len(), ATLAS_W * ATLAS_H * 4);
        assert_eq!(TERRAIN.len(), 256 * 256 * 4);
        assert_ne!(tile_color(260), tile_color(261));
        assert_ne!(tile_color(256), tile_color(17));
        assert!(tile_color(274)[3] == 255 && (25..=26).contains(&tile_color(274)[0])); // black wool
    }

    /// Faces follow the Java: grass is top 0 / side 3 / bottom 2, a log has end grain on top, wool colours come from the
    /// `BlockCloth` formula (red 14 is tile 129), a flat block keeps its tile on every face, and no face is outside the png.
    #[test]
    fn faces_pick_terrain_tiles() {
        assert_eq!([1, 3, 0].map(|s| terrain_tile(2, 0, s)), [Some(0), Some(3), Some(2)]);
        assert_eq!((terrain_tile(17, 0, 1), terrain_tile(17, 0, 3), terrain_tile(17, 2, 3)), (Some(21), Some(20), Some(117)));
        assert_eq!((terrain_tile(35, 0, 2), terrain_tile(35, 14, 2), terrain_tile(35, 15, 2)), (Some(64), Some(129), Some(113)));
        assert_eq!((terrain_tile(61, 0, 3), terrain_tile(62, 0, 3), terrain_tile(86, 0, 3), terrain_tile(91, 0, 3)), (Some(44), Some(61), Some(119), Some(120)));
        assert_eq!(terrain_tile(255, 0, 0), None);
        // Terrain UVs stay inside their own tile and the flat strip stays under the png.
        let (u, v) = terrain_uv(255, 1.0, 0.0);
        assert!(u < 1.0 && v < 256.0 / ATLAS_H as f32);
        assert!(atlas_uv(0, 0.0, 0.0).1 > 256.0 / ATLAS_H as f32);
        assert_ne!(face_uv(2, 0, 2, 0.0, 0.0), face_uv(2, 0, 3, 0.0, 0.0));
    }
}
