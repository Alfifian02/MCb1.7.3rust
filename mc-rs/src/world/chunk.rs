//! Chunk storage layout. Vanilla b1.7.3 dimensions: 16 wide, 128 tall, 16 deep.
//! Volume = 32768 cells. A chunk is a `Vec<u8>` of block ids (0 = air) with index
//! `(x<<11) | (z<<7) | y`; the chunks themselves live in `world::chunks::ChunkManager`.

pub const W: usize = 16;
pub const H: usize = 128;
pub const D: usize = 16;
pub const VOLUME: usize = W * H * D;

/// Index of a cell within the chunk volume.
#[inline]
pub const fn idx(x: usize, y: usize, z: usize) -> usize {
    (x << 11) | (z << 7) | y
}
