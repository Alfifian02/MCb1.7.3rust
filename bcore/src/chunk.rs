//! A chunk is 16 x 128 x 16 blocks, one byte each (32 KiB), y-major for fast vertical scans.
use crate::block::AIR;

pub const W: usize = 16;
pub const H: usize = 128;
pub const VOLUME: usize = W * W * H;

#[derive(Clone)]
pub struct Chunk {
    blocks: Vec<u8>,
}

#[inline]
pub const fn index(x: usize, y: usize, z: usize) -> usize {
    y + z * H + x * H * W
}

impl Chunk {
    pub fn new() -> Self {
        Self { blocks: vec![AIR; VOLUME] }
    }

    /// Out-of-range coordinates read as air.
    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> u8 {
        if (x as u32) >= W as u32 || (y as u32) >= H as u32 || (z as u32) >= W as u32 {
            return AIR;
        }
        self.blocks[index(x as usize, y as usize, z as usize)]
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, z: usize, id: u8) {
        self.blocks[index(x, y, z)] = id;
    }

    pub fn as_bytes(&self) -> &[u8] { &self.blocks }
}

impl Default for Chunk {
    fn default() -> Self { Self::new() }
}
