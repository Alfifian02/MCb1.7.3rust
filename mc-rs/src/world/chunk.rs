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
/// reeds, fire. Drawn as crossed quads (`cross_shape`), walked through, never hide a neighbour's face.
pub const fn is_plant(id: u8) -> bool {
    matches!(id, 6 | 31 | 32 | 37..=40 | 51 | 83)
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
        51 => Some((0.5, 1.0)),        // fire (ponytail: crossed quads on tile 31, not `renderBlockFire`'s flames)
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
        50 | 75 | 76 => match meta & 7 {
            1 => [0.0, 0.2, 0.35, 0.3, 0.8, 0.65],
            2 => [0.7, 0.2, 0.35, 1.0, 0.8, 0.65],
            3 => [0.35, 0.2, 0.0, 0.65, 0.8, 0.3],
            4 => [0.35, 0.2, 0.7, 0.65, 0.8, 1.0],
            _ => [0.4, 0.0, 0.4, 0.6, 0.6, 0.6],
        },
        // `BlockSign`: the post is 0.5 wide and one block high; a wall sign (metadata 2..=5 = +Z, -Z, +X, -X wall) is 2/16 thick,
        // y 9/32..25/32 (`setBlockBoundsBasedOnState`); any other metadata keeps the full cell, as the Java does.
        63 => [0.25, 0.0, 0.25, 0.75, 1.0, 0.75],
        68 => {
            const LO: f32 = 9.0 / 32.0;
            const HI: f32 = 25.0 / 32.0;
            const T: f32 = 2.0 / 16.0;
            match meta {
                2 => [0.0, LO, 1.0 - T, 1.0, HI, 1.0],
                3 => [0.0, LO, 0.0, 1.0, HI, T],
                4 => [1.0 - T, LO, 0.0, 1.0, HI, 1.0],
                5 => [0.0, LO, 0.0, T, HI, 1.0],
                _ => [0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            }
        }
        // M21: `BlockRedstoneWire` 1/16 high; pressure plates 1/16 in from the sides, 1/16 high (1/32 pressed); `BlockLever`
        // (3/16 either side of the middle, 0.2..0.8 high on a wall, a 0.25 post on the floor); `BlockButton` (6/16..10/16 high, 2/16
        // deep, 1/16 pressed). Metadata 1 / 2 / 3 / 4 = attached on the -x / +x / -z / +z side.
        55 => [0.0, 0.0, 0.0, 1.0, 1.0 / 16.0, 1.0],
        93 | 94 => [0.0, 0.0, 0.0, 1.0, 2.0 / 16.0, 1.0], // BlockRedstoneRepeater
        70 | 72 => [1.0 / 16.0, 0.0, 1.0 / 16.0, 15.0 / 16.0, if meta == 1 { 1.0 / 32.0 } else { 1.0 / 16.0 }, 15.0 / 16.0],
        69 => match meta & 7 {
            1 => [0.0, 0.2, 0.5 - 3.0 / 16.0, 6.0 / 16.0, 0.8, 0.5 + 3.0 / 16.0],
            2 => [1.0 - 6.0 / 16.0, 0.2, 0.5 - 3.0 / 16.0, 1.0, 0.8, 0.5 + 3.0 / 16.0],
            3 => [0.5 - 3.0 / 16.0, 0.2, 0.0, 0.5 + 3.0 / 16.0, 0.8, 6.0 / 16.0],
            4 => [0.5 - 3.0 / 16.0, 0.2, 1.0 - 6.0 / 16.0, 0.5 + 3.0 / 16.0, 0.8, 1.0],
            _ => [0.25, 0.0, 0.25, 0.75, 0.6, 0.75],
        },
        77 => {
            let d = if meta & 8 != 0 { 1.0 / 16.0 } else { 2.0 / 16.0 };
            match meta & 7 {
                1 => [0.0, 6.0 / 16.0, 0.5 - 3.0 / 16.0, d, 10.0 / 16.0, 0.5 + 3.0 / 16.0],
                2 => [1.0 - d, 6.0 / 16.0, 0.5 - 3.0 / 16.0, 1.0, 10.0 / 16.0, 0.5 + 3.0 / 16.0],
                3 => [0.5 - 3.0 / 16.0, 6.0 / 16.0, 0.0, 0.5 + 3.0 / 16.0, 10.0 / 16.0, d],
                _ => [0.5 - 3.0 / 16.0, 6.0 / 16.0, 1.0 - d, 0.5 + 3.0 / 16.0, 10.0 / 16.0, 1.0],
            }
        }
        59 => [0.0, 0.0, 0.0, 1.0, 0.25, 1.0],                             // crops (pick / outline only: drawn as 4 quads)
        60 => [0.0, 0.0, 0.0, 1.0, 15.0 / 16.0, 1.0],                      // farmland
        78 => [0.0, 0.0, 0.0, 1.0, 2.0 * (1 + (meta & 7)) as f32 / 16.0, 1.0], // snow layer
        81 => [0.0, 0.0, 0.0, 1.0, 1.0, 1.0],                              // cactus
        96 => trap_box(meta),                                              // trapdoor (`BlockTrapDoor`)
        92 => [(1 + 2 * meta.min(5)) as f32 / 16.0, 0.0, 1.0 / 16.0, 15.0 / 16.0, 0.5, 15.0 / 16.0], // cake: a bitten slice is gone from -X
        26 => [0.0, 0.0, 0.0, 1.0, 9.0 / 16.0, 1.0],                       // bed (`BlockBed`)
        // `BlockDoor.setDoorRotation` (3/16 thick, one block high per half) from `getState`
        64 | 71 => match if meta & 4 == 0 { meta.wrapping_sub(1) & 3 } else { meta & 3 } {
            0 => [0.0, 0.0, 0.0, 1.0, 1.0, 3.0 / 16.0],
            1 => [13.0 / 16.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            2 => [0.0, 0.0, 13.0 / 16.0, 1.0, 1.0, 1.0],
            _ => [0.0, 0.0, 0.0, 3.0 / 16.0, 1.0, 1.0],
        },
        // `BlockLadder.setBlockBoundsBasedOnState`: 2/16 thick against the wall (metadata 2..=5 = +Z, -Z, +X, -X)
        65 => match meta {
            2 => [0.0, 0.0, 14.0 / 16.0, 1.0, 1.0, 1.0],
            3 => [0.0, 0.0, 0.0, 1.0, 1.0, 2.0 / 16.0],
            4 => [14.0 / 16.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            5 => [0.0, 0.0, 0.0, 2.0 / 16.0, 1.0, 1.0],
            _ => [0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        },
        // `BlockRail.setBlockBoundsBasedOnState`: 2/16 high, 10/16 on an ascending shape (drawn as a flat quad, `mesh::push_flat`)
        66 | 27 | 28 => [0.0, 0.0, 0.0, 1.0, if (2..=5).contains(&(meta & 7)) { 10.0 / 16.0 } else { 2.0 / 16.0 }, 1.0],
        // M21 pistons: the head is its 4/16 plate (the shaft is not drawn), an extended base is 12/16 long (`BlockPistonBase`,
        // `BlockPistonExtension.setBlockBoundsBasedOnState`); facing 0 -y, 1 +y, 2 -z, 3 +z, 4 -x, 5 +x.
        34 => match meta & 7 {
            0 => [0.0, 0.0, 0.0, 1.0, 0.25, 1.0],
            1 => [0.0, 0.75, 0.0, 1.0, 1.0, 1.0],
            2 => [0.0, 0.0, 0.0, 1.0, 1.0, 0.25],
            3 => [0.0, 0.0, 0.75, 1.0, 1.0, 1.0],
            4 => [0.0, 0.0, 0.0, 0.25, 1.0, 1.0],
            _ => [0.75, 0.0, 0.0, 1.0, 1.0, 1.0],
        },
        29 | 33 if meta & 8 != 0 => match meta & 7 {
            0 => [0.0, 0.25, 0.0, 1.0, 1.0, 1.0],
            1 => [0.0, 0.0, 0.0, 1.0, 0.75, 1.0],
            2 => [0.0, 0.0, 0.25, 1.0, 1.0, 1.0],
            3 => [0.0, 0.0, 0.0, 1.0, 1.0, 0.75],
            4 => [0.25, 0.0, 0.0, 1.0, 1.0, 1.0],
            _ => [0.0, 0.0, 0.0, 0.75, 1.0, 1.0],
        },
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

/// Pseudo ids (unused block ids) for what depends on metadata: `STAIRS_PHYS + (meta & 3)` and `DOOR_PHYS + door_state`.
pub const STAIRS_PHYS: u8 = 100;
pub const DOOR_PHYS: u8 = 104;
/// `TRAP_PHYS + (meta & 7)`: a trapdoor's flap (facing + open bit).
pub const TRAP_PHYS: u8 = 108;
/// `CAKE_PHYS + meta` (0..=5, slices eaten): the cake's collision box moves with its bites.
pub const CAKE_PHYS: u8 = 116;

/// The id a physics query (`BlockQuery`) reads for a cell: its own, except a snow layer of 3+ layers (`SNOW_DEEP`), a stair
/// (`STAIRS_PHYS`, its facing) and a door (`DOOR_PHYS`, its leaf's position).
pub fn physics_id(id: u8, meta: u8) -> u8 {
    match id {
        78 if meta & 7 >= 3 => SNOW_DEEP,
        53 | 67 => STAIRS_PHYS + (meta & 3),
        64 | 71 => DOOR_PHYS + door_state(meta),
        96 => TRAP_PHYS + (meta & 7),
        92 => CAKE_PHYS + meta.min(5),
        _ => id,
    }
}

/// Does `physics_id` read the metadata of this id? A physics query then skips the second chunk lookup for every other cell
/// (`meta_only_where_physics_reads_it` fails when `physics_id` grows an arm this list does not know).
pub const fn meta_matters(id: u8) -> bool {
    matches!(id, 78 | 53 | 67 | 64 | 71 | 96 | 92)
}

/// `BlockDoor.getState`: which edge of the cell the leaf stands on (0 -Z, 1 +X, 2 +Z, 3 -X); an open door turns a quarter.
pub const fn door_state(meta: u8) -> u8 {
    if meta & 4 == 0 { meta.wrapping_sub(1) & 3 } else { meta & 3 }
}

/// `BlockDoor.setDoorRotation`: the 3/16 thick leaf, one cell high per half.
pub fn door_box(state: u8) -> [f32; 6] {
    const T: f32 = 3.0 / 16.0;
    match state {
        0 => [0.0, 0.0, 0.0, 1.0, 1.0, T],
        1 => [1.0 - T, 0.0, 0.0, 1.0, 1.0, 1.0],
        2 => [0.0, 0.0, 1.0 - T, 1.0, 1.0, 1.0],
        _ => [0.0, 0.0, 0.0, T, 1.0, 1.0],
    }
}

/// `BlockTrapDoor.setBlockBoundsForBlockRender`: 3/16 flat on the floor; open (bit 4) it stands against the wall it hangs on
/// (`meta & 3` = support on +Z, -Z, +X, -X).
pub fn trap_box(meta: u8) -> [f32; 6] {
    const T: f32 = 3.0 / 16.0;
    if meta & 4 == 0 {
        return [0.0, 0.0, 0.0, 1.0, T, 1.0];
    }
    match meta & 3 {
        0 => [0.0, 0.0, 1.0 - T, 1.0, 1.0, 1.0],
        1 => [0.0, 0.0, 0.0, 1.0, 1.0, T],
        2 => [1.0 - T, 0.0, 0.0, 1.0, 1.0, 1.0],
        _ => [0.0, 0.0, 0.0, T, 1.0, 1.0],
    }
}

/// `BlockStairs.getCollidingBoundingBoxes` (also what the mesher draws): the lower half and the raised half, by facing.
pub fn stair_boxes(meta: u8) -> [[f32; 6]; 2] {
    match meta & 3 {
        0 => [[0.0, 0.0, 0.0, 0.5, 0.5, 1.0], [0.5, 0.0, 0.0, 1.0, 1.0, 1.0]],
        1 => [[0.0, 0.0, 0.0, 0.5, 1.0, 1.0], [0.5, 0.0, 0.0, 1.0, 0.5, 1.0]],
        2 => [[0.0, 0.0, 0.0, 1.0, 0.5, 0.5], [0.0, 0.0, 0.5, 1.0, 1.0, 1.0]],
        _ => [[0.0, 0.0, 0.0, 1.0, 1.0, 0.5], [0.0, 0.0, 0.5, 1.0, 0.5, 1.0]],
    }
}

/// Every collision box of a (physics) id, and how many: two for a stair, else `collision`'s one or none.
pub fn collisions(id: u8) -> ([[f32; 6]; 2], usize) {
    match id {
        STAIRS_PHYS..=103 => (stair_boxes(id - STAIRS_PHYS), 2),
        _ => collision(id).map_or(([[0.0; 6]; 2], 0), |b| ([b, [0.0; 6]], 1)),
    }
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
        0 | 8..=11 | 27 | 28 | 30 | 36 | 50 | 51 | 55 | 59 | 63 | 65 | 66 | 68 | 69 | 70 | 72 | 75 | 76 | 77 | 78 | 90 | 93 | 94 => None, // rails have none (`getCollisionBoundingBoxFromPool` = null); ponytail: a ladder's thin box needs metadata (physics reads ids), none for now
        26 => Some([0.0, 0.0, 0.0, 1.0, 9.0 / 16.0, 1.0]), // ponytail: a door stays a full cell until the query carries metadata
        _ if is_plant(id) => None,
        44 | SNOW_DEEP => Some([0.0, 0.0, 0.0, 1.0, 0.5, 1.0]),
        DOOR_PHYS..=107 => Some(door_box(id - DOOR_PHYS)),
        TRAP_PHYS..=115 => Some(trap_box(id - TRAP_PHYS)),
        // `BlockCake.getCollisionBoundingBoxFromPool`: 1/16 lower than the drawn box (0.5 - 1/16 high)
        CAKE_PHYS..=121 => Some([(1 + 2 * (id - CAKE_PHYS)) as f32 / 16.0, 0.0, 1.0 / 16.0, 15.0 / 16.0, 0.5 - 1.0 / 16.0, 15.0 / 16.0]),
        81 => Some([I, 0.0, I, 1.0 - I, 1.0 - I, 1.0 - I]),
        85 => Some([0.0, 0.0, 0.0, 1.0, 1.5, 1.0]),
        _ => Some([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),
    }
}

/// `isOpaqueCube`: hides the faces behind it. Not the plants, fluids and boxes above; leaves, glass and ice stay in, as the
/// mesher draws them as opaque cubes.
pub fn opaque(id: u8) -> bool {
    id != 0 && !is_plant(id) && !is_fluid(id) && !matches!(id, 26 | 27 | 28 | 44 | 50 | 51 | 53 | 59 | 60 | 63..=68 | 71 | 78 | 81 | 85 | 92 | 96)
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

#[cfg(test)]
mod phys_tests {
    use super::*;

    #[test]
    fn meta_only_where_physics_reads_it() {
        for id in 0..=255u8 {
            if !meta_matters(id) {
                assert!((0..16).all(|m| physics_id(id, m) == id), "id {id} reads its metadata");
            }
        }
    }

    #[test]
    fn stairs_and_doors_have_their_java_boxes() {
        assert_eq!(physics_id(53, 2), STAIRS_PHYS + 2);
        assert_eq!(collisions(physics_id(67, 0)), (stair_boxes(0), 2));
        assert_eq!(stair_boxes(3)[1], [0.0, 0.0, 0.5, 1.0, 0.5, 1.0]);
        // closed: meta - 1 & 3, open: meta & 3
        assert_eq!((door_state(0), door_state(1), door_state(4), door_state(5)), (3, 0, 0, 1));
        assert_eq!(collision(physics_id(64, 1)), Some([0.0, 0.0, 0.0, 1.0, 1.0, 3.0 / 16.0]));
        assert_eq!(collisions(physics_id(71, 8)).1, 1, "the upper half has the same leaf");
        assert_eq!(collisions(1), ([[0.0, 0.0, 0.0, 1.0, 1.0, 1.0], [0.0; 6]], 1));
        assert_eq!(collisions(0).1, 0);
    }

    #[test]
    fn trapdoor_lies_flat_or_stands_on_its_wall() {
        use crate::world::pick::trap_meta;
        assert_eq!(collision(physics_id(96, 2)), Some([0.0, 0.0, 0.0, 1.0, 3.0 / 16.0, 1.0]), "closed: flat whatever the facing");
        assert_eq!(collision(physics_id(96, 4)), Some([0.0, 0.0, 13.0 / 16.0, 1.0, 1.0, 1.0]), "open on a +Z wall");
        assert_eq!(box_bounds(96, 7), Some([0.0, 0.0, 0.0, 3.0 / 16.0, 1.0, 1.0]), "open on a -X wall");
        assert_eq!((trap_meta(2, true), trap_meta(5, true), trap_meta(1, true), trap_meta(0, true), trap_meta(3, false)), (Some(0), Some(3), None, None, None));
        assert!(!opaque(96));
        // cake: the box loses 2/16 from -X per slice; the collision is 1/16 lower than the drawn box
        assert_eq!(box_bounds(92, 2), Some([5.0 / 16.0, 0.0, 1.0 / 16.0, 15.0 / 16.0, 0.5, 15.0 / 16.0]));
        assert_eq!(collision(physics_id(92, 0)), Some([1.0 / 16.0, 0.0, 1.0 / 16.0, 15.0 / 16.0, 7.0 / 16.0, 15.0 / 16.0]));
    }
}
