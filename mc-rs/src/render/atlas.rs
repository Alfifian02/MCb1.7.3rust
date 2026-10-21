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
/// Rows: `terrain.png`, the flat strip, then `gui/items.png` (`ITEMS_Y`).
pub const ITEMS_Y: usize = 256 + TILES_H;
/// Under them `art/kz.png` (the paintings, 16 px pieces = tiles `ART_TILE0 + row * 16 + col`) and `item/sign.png` (64 x 32 at `SIGN_Y`).
pub const ART_Y: usize = ITEMS_Y + 256;
pub const SIGN_Y: usize = ART_Y + 256;
/// `mob/char.png` (64 x 32, the player skin) at `CHAR_Y`.
pub const CHAR_Y: usize = SIGN_Y + 32;
pub const ATLAS_H: usize = ART_Y + 320;
/// Mip levels of the atlas (a 16 px tile has 16, 8, 4 px: three levels never mix two tiles at the middle of a tile).
pub const MIPS: u32 = 3;
/// `gui/items.png` as RGBA (256 x 256, 16 x 16 px sprites): `Item.iconIndex` picks the sprite (`item_icon`).
pub const ITEMS: &[u8] = include_bytes!("../../assets/items.rgba");
/// A mesh tile number from here up is an items.png sprite (`ITEM_TILE0 + icon`), see `item_uv`.
pub const ITEM_TILE0: u16 = 1000;
/// A mesh tile number from here up is a 16 px piece of `art/kz.png` (`ART_TILE0 + row * 16 + col`), see `art_uv`.
pub const ART_TILE0: u16 = 2000;
/// `art/kz.png` + `item/sign.png` + `mob/char.png` as RGBA, 256 x 320 (`tools/gen_terrain.py`).
pub const ART: &[u8] = include_bytes!("../../assets/art.rgba");
pub const TERRAIN: &[u8] = include_bytes!("../../assets/terrain.rgba");

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

/// Terrain tiles with a see-through texel (plants, glass, ladders, leaves...): they need the alpha test, so their faces are drawn in the
/// `cutout` layer; every other tile (and the flat strip below the png) is opaque and goes through the `solid` one (no `discard`).
fn cutout_tiles() -> &'static [bool; 256] {
    static T: std::sync::OnceLock<[bool; 256]> = std::sync::OnceLock::new();
    T.get_or_init(|| std::array::from_fn(|t| (0..256).any(|i| TERRAIN[((t / 16 * 16 + i / 16) * ATLAS_W + t % 16 * 16 + i % 16) * 4 + 3] < 128)))
}

/// Whether terrain tile `t` has a see-through texel (a cut-out tile); the flat strip below the png is opaque.
pub fn tile_is_cutout(t: u16) -> bool {
    cutout_tiles().get(t as usize).copied().unwrap_or(false)
}

/// Whether a quad whose texel centre reads `uv` (atlas coordinates, 0..1) shows a cut-out terrain tile.
pub fn is_cutout(uv: [f32; 2]) -> bool {
    let (x, y) = (uv[0] * ATLAS_W as f32, uv[1] * ATLAS_H as f32);
    y < 256.0 && cutout_tiles()[(y as usize / 16) * 16 + (x as usize / 16).min(15)]
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
    v.extend_from_slice(ITEMS);
    v.extend_from_slice(ART);
    v
}

/// Level `l` (1, 2) of the atlas from level 0: 2x2 box filter per level, colour weighted by alpha so a cut-out's clear texels
/// do not darken its edge (the alpha itself is averaged: distant plants thin out a little).
pub fn mip(src: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(w / 2 * (h / 2) * 4);
    for y in 0..h / 2 {
        for x in 0..w / 2 {
            let px = |dx: usize, dy: usize| &src[((2 * y + dy) * w + 2 * x + dx) * 4..][..4];
            let (mut c, mut a) = ([0u32; 3], 0u32);
            for p in [px(0, 0), px(1, 0), px(0, 1), px(1, 1)] {
                (0..3).for_each(|i| c[i] += p[i] as u32 * p[3] as u32);
                a += p[3] as u32;
            }
            let d = a.max(1);
            out.extend([(c[0] / d) as u8, (c[1] / d) as u8, (c[2] / d) as u8, (a / 4) as u8]);
        }
    }
    out
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

/// Atlas UV of items.png sprite `icon` at (u, v) in 0..1, v up (same inset as `terrain_uv`).
pub fn item_uv(icon: u8, u: f32, v: f32) -> (f32, f32) {
    let (x, y) = ((icon % 16) as f32 * 16.0 + 0.02 + u * 15.96, ITEMS_Y as f32 + (icon / 16) as f32 * 16.0 + 0.02 + (1.0 - v) * 15.96);
    (x / ATLAS_W as f32, y / ATLAS_H as f32)
}

/// Atlas UV of piece `n` (`row * 16 + col`) of `art/kz.png` at (u, v) in 0..1, v up (same inset as `terrain_uv`).
pub fn art_uv(n: u8, u: f32, v: f32) -> (f32, f32) {
    let (x, y) = ((n % 16) as f32 * 16.0 + 0.02 + u * 15.96, ART_Y as f32 + (n / 16) as f32 * 16.0 + 0.02 + (1.0 - v) * 15.96);
    (x / ATLAS_W as f32, y / ATLAS_H as f32)
}

/// Atlas UV of pixel (`x`, `y`) of `item/sign.png` (y down like the png).
pub fn sign_px(x: f32, y: f32) -> (f32, f32) {
    (x / ATLAS_W as f32, (SIGN_Y as f32 + y) / ATLAS_H as f32)
}

/// Atlas UV of pixel (`x`, `y`) of `mob/char.png` (y down like the png).
pub fn char_px(x: f32, y: f32) -> (f32, f32) {
    (x / ATLAS_W as f32, (CHAR_Y as f32 + y) / ATLAS_H as f32)
}

/// `Item.iconIndex` of an item id (256 and up), `None` for blocks and items the Java table does not have.
pub fn item_icon(id: u16) -> Option<u8> {
    super::icons::ICONS.binary_search_by_key(&id, |&(i, _)| i).ok().map(|k| super::icons::ICONS[k].1)
}

/// `BlockDoor.getBlockTextureFromSideAndMetadata`: (tile, mirrored). The Java returns the tile negated for a mirrored face; the top
/// half (metadata bit 8) is the tile 16 lower (97 wood / 98 iron: 81 / 82 on top).
pub const fn door_face(id: u8, meta: u8, side: u8) -> (u8, bool) {
    let base = if id == 71 { 98 } else { 97 };
    if side <= 1 {
        return (base, false);
    }
    let state = if meta & 4 == 0 { meta.wrapping_sub(1) & 3 } else { meta & 3 };
    if ((state == 0 || state == 2) as u8 ^ (side <= 3) as u8) != 0 {
        return (base, false);
    }
    let n = state / 2 + ((side & 1) ^ state) + (meta & 4) / 4;
    (base - (meta & 8) * 2, n & 1 != 0)
}

/// `BlockBed.getBlockTextureFromSideAndMetadata` with `ModelBed.bedDirection`: the head (metadata bit 8 clear) and the foot.
pub const fn bed_tile(meta: u8, side: u8) -> u8 {
    const DIR: [[u8; 6]; 4] = [[1, 0, 3, 2, 5, 4], [1, 0, 5, 4, 2, 3], [1, 0, 2, 3, 4, 5], [1, 0, 4, 5, 3, 2]];
    if side == 0 {
        return 4;
    }
    let d = DIR[(meta & 3) as usize][side as usize];
    if meta & 8 != 0 {
        if d == 2 { 152 } else if d == 4 || d == 5 { 151 } else { 135 }
    } else if d == 3 {
        149
    } else if d == 4 || d == 5 {
        150
    } else {
        134
    }
}

/// Atlas UV of the pixel (`px`, `py`) (0..16, y down like the png) of terrain tile `t`.
pub fn terrain_px(t: u8, px: f32, py: f32) -> (f32, f32) {
    (((t % 16) as f32 * 16.0 + px) / ATLAS_W as f32, ((t / 16) as f32 * 16.0 + py) / ATLAS_H as f32)
}

/// `Block.getBlockTextureFromSideAndMetadata`: the `terrain.png` tile of a block's face, `side` as in the Java (0 bottom, 1 top,
/// 2 north -Z, 3 south +Z, 4 west -X, 5 east +X); `None` = no tile known (flat colour). Blocks with a facing in the Java
/// (furnace, pumpkin) get their front from the metadata (`onBlockPlacedBy`), a chest from its neighbours. 31 tall grass is 55 dead
/// shrub / 39 grass / 56 fern by metadata; wool, log, leaves and sapling pick by metadata like `tile_of` does.
pub const fn terrain_tile(id: u8, meta: u8, side: u8) -> Option<u8> {
    let (top, bottom) = (side == 1, side == 0);
    Some(match id {
        1 => 1,
        2 => if top { 0 } else if bottom { 2 } else { 3 },
        3 => 2,
        4 | 67 => 16,
        5 | 53 | 85 => 4,
        59 => 88 + (meta & 7),
        6 => match meta & 3 { 1 => 63, 2 => 79, _ => 15 },
        7 => 17,
        8 | 9 => if side <= 1 { 205 } else { 206 },   // `BlockFluid.getBlockTextureFromSide`: top/bottom still, sides flowing
        10 | 11 => if side <= 1 { 237 } else { 238 },
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
        51 => 31, // fire_0
        // BlockBed.getBlockTextureFromSideAndMetadata: base tile 134, `ModelBed.bedDirection` says which face is the end, a side or the top.
        26 => {
            const DIR: [[u8; 6]; 4] = [[1, 0, 3, 2, 5, 4], [1, 0, 5, 4, 2, 3], [1, 0, 2, 3, 4, 5], [1, 0, 4, 5, 3, 2]];
            let d = DIR[(meta & 3) as usize][side as usize];
            let side_face = d == 4 || d == 5;
            if bottom { 4 } else if meta & 8 != 0 {
                if d == 2 { 134 + 2 + 16 } else if !side_face { 134 + 1 } else { 134 + 1 + 16 }
            } else if d == 3 { 134 - 1 + 16 } else if !side_face { 134 } else { 134 + 16 }
        }
        // BlockDoor.getBlockTextureFromSideAndMetadata (97 wood, 98 iron; the upper half is 16 tiles up). ponytail: the
        // Java mirrors some faces (negative index); the leaf is drawn unmirrored.
        64 | 71 => {
            let base = if id == 64 { 97 } else { 98 };
            let s = crate::world::chunk::door_state(meta);
            if top || bottom || (s == 0 || s == 2) != (side <= 3) { base } else { base - (meta & 8) * 2 }
        }
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
        50 => 80,
        56 => 50,
        57 => 24,
        58 => if top { 43 } else if bottom { 4 } else if side == 2 || side == 4 { 60 } else { 59 },
        60 => if top { if meta > 0 { 86 } else { 87 } } else { 2 },
        61 | 62 => if side <= 1 { 62 } else if side != meta { 45 } else if id == 62 { 61 } else { 44 },
        73 | 74 => 51,
        93 | 94 => match side { 1 => if id == 94 { 147 } else { 131 }, 0 => if id == 94 { 99 } else { 115 }, _ => 5 }, // `BlockRedstoneRepeater.getBlockTextureFromSide`
        // `BlockPistonBase` / `BlockPistonExtension.getBlockTextureFromSideAndMetadata`: the face the metadata names, the opposite one
        // and the four sides (106 sticky face, 107 face, 108 side, 109 back, 110 the face of an extended base).
        33 | 29 => {
            let f = meta & 7;
            if f > 5 { if id == 29 { 106 } else { 107 } }
            else if side == f { if meta & 8 != 0 { 110 } else if id == 29 { 106 } else { 107 } }
            else if side == [1, 0, 3, 2, 5, 4][f as usize] { 109 } else { 108 }
        }
        34 => {
            let f = meta & 7;
            if f > 5 { 108 } else if side == f { if meta & 8 != 0 { 106 } else { 107 } } else if side == [1, 0, 3, 2, 5, 4][f as usize] { 107 } else { 108 }
        }
        69 => 96,      // lever (ponytail: the lever sprite on its box, not `renderBlockLever`'s base + handle)
        70 | 77 => 1,  // stone pressure plate, button
        72 => 4,       // wooden pressure plate
        79 => 67,
        80 => 66,
        78 => 66,
        81 => if top { 69 } else if bottom { 71 } else { 70 },
        82 => 72,
        83 => 73,
        84 => if top { 75 } else { 74 },
        86 | 91 => if side <= 1 { 102 } else if side == [3, 4, 2, 5][(meta & 3) as usize] { 119 + (id == 91) as u8 } else { 118 },
        87 => 103,
        88 => 104,
        89 => 105,
        96 => 84, // trapdoor
        // BlockCake.getBlockTextureFromSideAndMetadata (base tile 121): top, sides, the bitten face (-X), bottom
        92 => if top { 121 } else if bottom { 124 } else if meta > 0 && side == 4 { 123 } else { 122 },
        // Blocks nothing above gave a tile: the dispenser (no facing here yet: the front is +Z), the note block (the jukebox's side),
        // cobweb, the portal, the locked chest (the chest's). Rails, wire, torches, the lever, the ladder... are drawn by the mesher.
        23 => if side <= 1 { 62 } else if side != meta { 45 } else { 46 }, // `BlockDispenser`: the face the metadata names
        25 => 74,
        30 => 11,
        90 => 14,
        95 => if side <= 1 { 25 } else if side == 3 { 27 } else { 26 },
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
        assert_eq!(ART.len(), 256 * 320 * 4, "kz.png over sign.png over char.png");
        assert!(art_uv(255, 1.0, 0.0).1 * (ATLAS_H as f32) < (ART_Y + 256) as f32, "the last piece stays above the sign rows");
        assert_ne!(tile_color(260), tile_color(261));
        assert_ne!(tile_color(256), tile_color(17));
        assert!(tile_color(274)[3] == 255 && (25..=26).contains(&tile_color(274)[0])); // black wool
    }

    /// Every tile `terrain_tile` hands out, for any block, damage and face, holds a picture in `assets/terrain.rgba` (the Legacy Restoration
    /// pack, `tools/gen_terrain_pack.py`; the grass overlay, tile 38, is the only one meant to be mostly transparent, and it is not handed out).
    #[test]
    fn every_handed_out_tile_has_a_picture() {
        for id in 1..=96u8 {
            for meta in 0..16 {
                for side in 0..6 {
                    let Some(t) = terrain_tile(id, meta, side) else { continue };
                    let (x, y) = ((t % 16) as usize * 16, (t / 16) as usize * 16);
                    let seen = (0..16 * 16).any(|i| TERRAIN[((y + i / 16) * 256 + x + i % 16) * 4 + 3] > 0);
                    assert!(seen, "block {id} meta {meta} side {side}: tile {t} is empty");
                }
            }
        }
    }

    /// Faces follow the Java: grass is top 0 / side 3 / bottom 2, a log has end grain on top, wool colours come from the
    /// `BlockCloth` formula (red 14 is tile 129), a flat block keeps its tile on every face, and no face is outside the png.
    #[test]
    fn faces_pick_terrain_tiles() {
        assert_eq!([1, 3, 0].map(|s| terrain_tile(2, 0, s)), [Some(0), Some(3), Some(2)]);
        assert_eq!((terrain_tile(17, 0, 1), terrain_tile(17, 0, 3), terrain_tile(17, 2, 3)), (Some(21), Some(20), Some(117)));
        assert_eq!((terrain_tile(35, 0, 2), terrain_tile(35, 14, 2), terrain_tile(35, 15, 2)), (Some(64), Some(129), Some(113)));
        assert_eq!((terrain_tile(61, 3, 3), terrain_tile(62, 3, 3), terrain_tile(86, 0, 3), terrain_tile(91, 0, 3)), (Some(44), Some(61), Some(119), Some(120)));
        // facing: a furnace shows its front only on the side its metadata names, a pumpkin on [S, W, N, E] for 0..3
        assert_eq!((terrain_tile(61, 3, 2), terrain_tile(61, 0, 3), terrain_tile(86, 2, 2), terrain_tile(86, 2, 3)), (Some(45), Some(45), Some(119), Some(118)));
        // doors: the top half is the tile 16 lower; a bed is planks underneath; the generated icon table is sorted
        assert_eq!((door_face(64, 1, 2).0, door_face(64, 9, 2).0, door_face(71, 1, 2).0), (97, 81, 98));
        assert_eq!((bed_tile(0, 0), bed_tile(0, 1), bed_tile(8, 1)), (4, 134, 135));
        assert!(super::super::icons::ICONS.windows(2).all(|w| w[0].0 < w[1].0) && item_icon(256) == Some(82) && item_icon(1) == None);
        assert_eq!(terrain_tile(255, 0, 0), None);
        // Terrain UVs stay inside their own tile and the flat strip stays under the png.
        let (u, v) = terrain_uv(255, 1.0, 0.0);
        assert!(u < 1.0 && v < 256.0 / ATLAS_H as f32);
        assert!(atlas_uv(0, 0.0, 0.0).1 > 256.0 / ATLAS_H as f32);
        assert_ne!(face_uv(2, 0, 2, 0.0, 0.0), face_uv(2, 0, 3, 0.0, 0.0));
    }
}

#[cfg(test)]
mod cutout_tests {
    use super::*;

    /// Stone is opaque (solid layer), the sapling (tile 15) and glass (tile 49) are see-through (cutout layer), the flat strip is solid.
    #[test]
    fn cutout_layer_follows_the_tile_alpha() {
        let uv = |t: usize| [((t % 16) as f32 * 16.0 + 8.0) / ATLAS_W as f32, ((t / 16) as f32 * 16.0 + 8.0) / ATLAS_H as f32];
        assert!(!is_cutout(uv(1)) && is_cutout(uv(15)) && is_cutout(uv(49)));
        assert!(!is_cutout([0.5, 270.0 / ATLAS_H as f32]));
    }
}
