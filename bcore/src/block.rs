//! Block ids (classic beta-era numbering, 1 byte per block) and rendering rules.
pub const AIR: u8 = 0;
pub const STONE: u8 = 1;
pub const GRASS: u8 = 2;
pub const DIRT: u8 = 3;
pub const COBBLE: u8 = 4;
pub const PLANKS: u8 = 5;
pub const BEDROCK: u8 = 7;
pub const WATER: u8 = 9;
pub const SAND: u8 = 12;
pub const GRAVEL: u8 = 13;
pub const LOG: u8 = 17;
pub const LEAVES: u8 = 18;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Face { PosX = 0, NegX = 1, PosY = 2, NegY = 3, PosZ = 4, NegZ = 5 }

pub const FACES: [Face; 6] = [Face::PosX, Face::NegX, Face::PosY, Face::NegY, Face::PosZ, Face::NegZ];

/// Opaque blocks hide the faces of their neighbours.
#[inline]
pub fn is_opaque(id: u8) -> bool {
    !matches!(id, AIR | WATER | LEAVES)
}

/// Atlas tile index (16x16 grid, row-major) for a block face.
pub fn tile(id: u8, face: Face) -> u8 {
    match (id, face) {
        (GRASS, Face::PosY) => 0,
        (GRASS, Face::NegY) => 2,
        (GRASS, _) => 3,
        (STONE, _) => 1,
        (DIRT, _) => 2,
        (COBBLE, _) => 16,
        (PLANKS, _) => 4,
        (BEDROCK, _) => 17,
        (WATER, _) => 205,
        (SAND, _) => 18,
        (GRAVEL, _) => 19,
        (LOG, Face::PosY | Face::NegY) => 21,
        (LOG, _) => 20,
        (LEAVES, _) => 52,
        _ => 1,
    }
}
