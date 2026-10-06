//! M1 chunk storage. Vanilla b1.7.3 dimensions: 16 wide, 128 tall, 16 deep.
//! Volume = 32768 cells. Stored as `Vec<u8>` with index `(x<<11) | (z<<7) | y`.

pub const W: usize = 16;
pub const H: usize = 128;
pub const D: usize = 16;
pub const VOLUME: usize = W * H * D;

/// Index of a cell within the chunk volume.
#[inline]
pub const fn idx(x: usize, y: usize, z: usize) -> usize {
    (x << 11) | (z << 7) | y
}

/// A single 16x16x128 chunk. Cell value is a block id (0 = air).
#[derive(Clone)]
pub struct Chunk {
    pub blocks: Vec<u8>,
}

impl Chunk {
    /// Fill every cell with `block_id`. Used by M1 to make a hand-built chunk.
    pub fn filled(block_id: u8) -> Self {
        Self { blocks: vec![block_id; VOLUME] }
    }

    /// Build a chunk that is `stone` everywhere below y=64 and `air` above.
    pub fn stone_pillar() -> Self {
        let mut blocks = vec![0u8; VOLUME];
        for y in 0..64 {
            for z in 0..D {
                for x in 0..W {
                    blocks[idx(x, y, z)] = 1; // block id 1 = stone
                }
            }
        }
        Self { blocks }
    }

    /// Look up a block by local cell index. Panics on out-of-bounds.
    #[inline]
    pub fn get(&self, x: usize, y: usize, z: usize) -> u8 {
        self.blocks[idx(x, y, z)]
    }
}
