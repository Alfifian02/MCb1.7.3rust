//! Chunk storage layout. Vanilla b1.7.3 dimensions: 16 wide, 128 tall, 16 deep.
//! Volume = 32768 cells. A chunk is a `Vec<u8>` of block ids (0 = air) with index
//! `(x<<11) | (z<<7) | y`, plus a `Nibbles` of block metadata with the same index; the chunks themselves
//! live in `world::chunks::ChunkManager`.

pub const W: usize = 16;
pub const H: usize = 128;
pub const D: usize = 16;
pub const VOLUME: usize = W * H * D;

/// Index of a cell within the chunk volume.
#[inline]
pub const fn idx(x: usize, y: usize, z: usize) -> usize {
    (x << 11) | (z << 7) | y
}

/// `NibbleArray`: 4 bits per cell, two cells to a byte (an even cell index is the low nibble), indexed like the
/// blocks (`idx`). A chunk's block metadata (`Chunk.data`: log species, wool colour, slab type, stair and door
/// facing, ...) lives in one, and its bytes are the McRegion `Data` tag as they are, so saving (M7) needs no
/// conversion. `Default` is the empty placeholder `mem::take` leaves behind; make a real one with `new`.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Nibbles(Vec<u8>);

impl Nibbles {
    pub fn new() -> Self {
        Self(vec![0; VOLUME / 2])
    }

    pub fn get(&self, x: usize, y: usize, z: usize) -> u8 {
        let i = idx(x, y, z);
        self.0[i >> 1] >> ((i & 1) * 4) & 15
    }

    /// Only the low 4 bits of `v` count (`NibbleArray.setNibble`).
    pub fn set(&mut self, x: usize, y: usize, z: usize, v: u8) {
        let i = idx(x, y, z);
        let s = (i & 1) * 4;
        let b = &mut self.0[i >> 1];
        *b = *b & !(15u8 << s) | (v & 15) << s;
    }

    pub fn bytes(&self) -> &[u8] {
        &self.0
    }

    /// The packed bytes back (a loaded save); the caller has checked the length.
    pub fn from_bytes(b: Vec<u8>) -> Self {
        debug_assert_eq!(b.len(), VOLUME / 2);
        Self(b)
    }
}

/// Plants (render type 1, `getCollisionBoundingBoxFromPool` null): sapling, tall grass, dead bush, flowers, mushrooms,
/// reeds. Drawn as crossed quads (`cross_shape`), walked through, never hide a neighbour's face.
pub const fn is_plant(id: u8) -> bool {
    matches!(id, 6 | 31 | 32 | 37..=40 | 83)
}

/// `BlockFluid`: water 8 (flowing) / 9 (still), lava 10 / 11. No collision, drawn with `mesh`'s own fluid shape.
pub const fn is_fluid(id: u8) -> bool {
    matches!(id, 8..=11)
}

/// Plants drawn as two crossed quads (render type 1): (half width, height) of the block's bounds (`setBlockBounds`).
/// The quads themselves are full cell (`renderCrossedSquares`, the texture's cut-out shapes them); these bounds size
/// the pick ray and the selection outline.
pub const fn cross_shape(id: u8) -> Option<(f32, f32)> {
    match id {
        6 => Some((0.4, 0.8)),         // sapling
        37 | 38 => Some((0.2, 0.6)),   // flowers
        39 | 40 => Some((0.2, 0.4)),   // mushrooms
        31 | 32 => Some((0.4, 0.8)),   // tall grass, dead bush
        83 => Some((0.375, 1.0)),      // reeds
        _ => None,
    }
}

/// The 1/16 `BlockCactus` pulls its four side faces in (`renderBlockCactus`) and its collision box and outline shrink by.
pub const CACTUS_INSET: f32 = 1.0 / 16.0;

/// Blocks drawn as a (non-full) box: `setBlockBounds` as [min x, y, z, max x, y, z] inside the cell. The faces are the
/// box's, with the matching window of the tile (`renderEastFace` and friends). The snow layer reads its metadata
/// (`BlockSnow.setBlockBoundsBasedOnState`: 2 x (1 + layers) sixteenths). The cactus is a full cell here: the Java never sets
/// its bounds, only its collision / outline / faces are inset (`CACTUS_INSET`).
pub fn box_bounds(id: u8, meta: u8) -> Option<[f32; 6]> {
    Some(match id {
        44 => [0.0, 0.0, 0.0, 1.0, 0.5, 1.0],                              // single slab
        // `BlockTorch.collisionRayTrace`: 1..=4 hangs on the -X, +X, -Z, +Z wall (0.3 wide, 0.2..0.8 high), else it stands.
        50 => match meta & 7 {
            1 => [0.0, 0.2, 0.35, 0.3, 0.8, 0.65],
            2 => [0.7, 0.2, 0.35, 1.0, 0.8, 0.65],
            3 => [0.35, 0.2, 0.0, 0.65, 0.8, 0.3],
            4 => [0.35, 0.2, 0.7, 0.65, 0.8, 1.0],
            _ => [0.4, 0.0, 0.4, 0.6, 0.6, 0.6],
        },
        59 => [0.0, 0.0, 0.0, 1.0, 0.25, 1.0],                             // crops (pick / outline only: drawn as 4 quads)
        60 => [0.0, 0.0, 0.0, 1.0, 15.0 / 16.0, 1.0],                      // farmland
        78 => [0.0, 0.0, 0.0, 1.0, 2.0 * (1 + (meta & 7)) as f32 / 16.0, 1.0], // snow layer
        81 => [0.0, 0.0, 0.0, 1.0, 1.0, 1.0],                              // cactus
        _ => return None,
    })
}

/// What the crosshair can hit (`Block.collisionRayTrace`): the block's bounds; fluids and air are skipped
/// (`canCollideCheck`).
pub fn pick_bounds(id: u8, meta: u8) -> Option<[f32; 6]> {
    if id == 0 || is_fluid(id) {
        return None;
    }
    Some(match cross_shape(id) {
        Some((w, h)) => [0.5 - w, 0.0, 0.5 - w, 0.5 + w, h, 0.5 + w],
        None => box_bounds(id, meta).unwrap_or([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),
    })
}

/// The selection outline (`getSelectedBoundingBoxFromPool`): the pick box, except the cactus, which is inset on x and z.
pub fn outline_bounds(id: u8, meta: u8) -> Option<[f32; 6]> {
    let i = CACTUS_INSET;
    if id == 81 { Some([i, 0.0, i, 1.0 - i, 1.0, 1.0 - i]) } else { pick_bounds(id, meta) }
}

/// The snow layer collides only from 3 layers up (`BlockSnow.getCollisionBoundingBoxFromPool`, `meta & 7 >= 3`, 0.5 high), but the
/// physics queries carry ids, not metadata: `physics_id` turns such a layer into the pseudo id `SNOW_DEEP`, which `collision` knows.
pub const SNOW_DEEP: u8 = 255;

/// The id a physics query (`BlockQuery`) reads for a cell: its own, except a snow layer of 3+ layers (`SNOW_DEEP`).
pub fn physics_id(id: u8, meta: u8) -> u8 {
    if id == 78 && meta & 7 >= 3 { SNOW_DEEP } else { id }
}

/// `getCollisionBoundingBoxFromPool`: what the player and mobs stand on or bump into; `None` = walk through. The
/// farmland is a full cell here although it is drawn 15/16 high (`BlockFarmland`), a fence is 1.5 high, the cactus is
/// inset by `CACTUS_INSET` and one sixteenth lower; a snow layer of 3+ layers is `SNOW_DEEP` (0.5 high).
/// `World.isBlockNormalCube`: an opaque full cube (stone, planks, furnace, ...), what holds a wall torch. Not lava, a slab or a
/// chest, which are opaque in the light table but not `renderAsNormalBlock`; stairs (53, 67) and the fence are not either.
pub const fn normal_cube(id: u8) -> bool {
    light_opacity(id) == 255 && !matches!(id, 10 | 11 | 44 | 53 | 54 | 67 | 85)
}

pub fn collision(id: u8) -> Option<[f32; 6]> {
    const I: f32 = CACTUS_INSET;
    match id {
        0 | 8..=11 | 30 | 50 | 51 | 55 | 59 | 63 | 68 | 78 | 90 => None,
        _ if is_plant(id) => None,
        44 | SNOW_DEEP => Some([0.0, 0.0, 0.0, 1.0, 0.5, 1.0]),
        81 => Some([I, 0.0, I, 1.0 - I, 1.0 - I, 1.0 - I]),
        85 => Some([0.0, 0.0, 0.0, 1.0, 1.5, 1.0]),
        _ => Some([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),
    }
}

/// `isOpaqueCube`: hides the faces behind it. Not the plants, fluids and boxes above; leaves, glass and ice stay in, as the
/// mesher draws them as opaque cubes.
pub fn opaque(id: u8) -> bool {
    id != 0 && !is_plant(id) && !is_fluid(id) && !matches!(id, 44 | 50 | 53 | 59 | 60 | 67 | 78 | 81 | 85)
}

/// `Block.lightOpacity`: how much light a block eats (255 = fully opaque). Default is 255 for
/// opaque cubes and 0 for everything else; the overrides are `setLightOpacity` calls in Block.java
/// (water 3, lava 255, leaves 1, web 1, ice 3). Slabs (44) and wool (35) are not generated by
/// worldgen and read as opaque, which matches the 255 default for wool. 44 (single slab) and 60 (farmland) call
/// `setLightOpacity(255)` themselves, though they are not opaque cubes (golden `TAB 60 0 255`).
pub const fn light_opacity(id: u8) -> u8 {
    match id {
        8 | 9 | 79 => 3,
        18 | 30 => 1,
        10 | 11 => 255,
        0 | 6 | 20 | 26..=29 | 31..=34 | 36..=40 | 50..=53 | 55 | 59 | 63..=72 | 75..=78 | 81 | 83 | 85 | 90 | 92..=94 | 96 => 0,
        _ => 255,
    }
}

/// `Block.lightValue` = `(int)(15.0F * setLightValue)`: block ids that emit light.
pub const fn light_value(id: u8) -> u8 {
    match id {
        10 | 11 | 51 | 89 | 91 | 95 => 15, // lava, fire, glowstone, jack-o-lantern, locked chest
        50 => 14,                          // torch (15/16)
        62 => 13,                          // lit furnace (14/16)
        90 => 11,                          // portal (12/16)
        74 | 94 => 9,                      // glowing redstone ore, lit repeater (10/16)
        76 => 7,                           // redstone torch (0.5)
        39 => 1,                           // brown mushroom (2/16)
        _ => 0,
    }
}

/// `WorldProvider.generateLightBrightnessTable`: light level 0..=15 -> brightness 0.05..=1.0.
pub fn brightness(level: u8) -> f32 {
    let f = 1.0 - level as f32 / 15.0;
    (1.0 - f) / (f * 3.0 + 1.0) * (1.0 - 0.05) + 0.05
}

/// `Chunk.heightMap` for one chunk, indexed `z << 4 | x`: the y of the first cell, scanning down from
/// the top, whose block below it is not fully transparent (`generateHeightMap`).
pub fn height_map(blocks: &[u8]) -> [u8; 256] {
    let mut h = [0u8; 256];
    for x in 0..W {
        for z in 0..D {
            let base = idx(x, 0, z);
            let mut y = H - 1;
            while y > 0 && light_opacity(blocks[base + y - 1]) == 0 {
                y -= 1;
            }
            h[z << 4 | x] = y as u8;
        }
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same layout as the Java `NibbleArray`: low nibble first, cell index `x << 11 | z << 7 | y`, neighbours untouched.
    #[test]
    fn nibbles_pack_like_java() {
        let mut n = Nibbles::new();
        n.set(0, 0, 0, 5);
        n.set(0, 1, 0, 9); // cell 1: the high nibble of byte 0
        n.set(15, 127, 15, 0xAB); // the last cell; only the low 4 bits count
        assert_eq!(n.bytes()[0], 0x95);
        assert_eq!(n.bytes()[VOLUME / 2 - 1], 0xB0);
        assert_eq!((n.get(0, 0, 0), n.get(0, 1, 0), n.get(15, 127, 15), n.get(1, 0, 0)), (5, 9, 11, 0));
        n.set(0, 0, 0, 0);
        assert_eq!(n.bytes()[0], 0x90);
    }

    /// A snow layer collides from 3 layers up (`meta & 7 >= 3`), half a block high, like the Java; 1-2 layers are walked through.
    #[test]
    fn snow_collides_from_three_layers() {
        assert_eq!(collision(physics_id(78, 1)), None);
        assert_eq!(collision(physics_id(78, 2)), None);
        assert_eq!(collision(physics_id(78, 3)), Some([0.0, 0.0, 0.0, 1.0, 0.5, 1.0]));
        assert_eq!(collision(physics_id(78, 8 | 2)), None, "bit 8 is not a layer");
        assert_eq!(physics_id(1, 5), 1);
    }
}
