//! Overworld chunk generator. Direct port of net.minecraft.src.ChunkProviderGenerate
//! from the b1.7.3 decomp. Vanilla equivalent: net.minecraft.world.level.levelgen.
//!
//! Generates a 16x128x16 chunk as a Vec<u8> of 32768 block ids. The chunk is
//! computed against a single-chunk (5x17x5) density sample grid. The full Beta-1.7
//! generator uses a 4x4 chunk neighborhood and biomes; this M3b version operates
//! on one chunk in isolation with a fixed-plains biome so the player can spawn
//! and walk. Biomes + multi-chunk coverage come in M3c.

use crate::world::gen::noise::{JavaRandom, OctaveNoise};

pub const W: usize = 16;
pub const H: usize = 128;
pub const D: usize = 16;
pub const VOLUME: usize = W * H * D;
pub const SEA_LEVEL: i32 = 64;

/// Block ids (Beta-1.7 numbering, vanilla uses the same).
pub mod block {
    pub const AIR: u8 = 0;
    pub const STONE: u8 = 1;
    pub const GRASS: u8 = 2;
    pub const DIRT: u8 = 3;
    pub const BEDROCK: u8 = 7;
    pub const WATER: u8 = 8;
    pub const SAND: u8 = 12;
}

pub struct OverworldGenerator {
    noise_lim: OctaveNoise,
    noise_low: OctaveNoise,
    noise_base: OctaveNoise,
}

impl OverworldGenerator {
    pub fn new(world_seed: u64) -> Self {
        let mut rng = JavaRandom::new(world_seed);
        Self {
            noise_lim:  OctaveNoise::new(rng.next_u63(), 16),
            noise_low:  OctaveNoise::new(rng.next_u63(), 16),
            noise_base: OctaveNoise::new(rng.next_u63(), 8),
        }
    }

    /// Generate one 16x128x16 chunk. Returns flat Vec<u8> of length 32768,
    /// index (x << 11) | (z << 7) | y.
    pub fn generate(&self, chunk_x: i32, chunk_z: i32) -> Vec<u8> {
        let mut out = vec![block::AIR; VOLUME];

        // For each (x,z) compute the heightmap column height via a simple
        // Perlin-driven density test. This is the M3b simplified single-chunk
        // version of ChunkProviderGenerate.func_4061_a's interpolation loop.
        for z in 0..D {
            for x in 0..W {
                let wx = (chunk_x * W as i32 + x as i32) as f64;
                let wz = (chunk_z * D as i32 + z as i32) as f64;
                // Continent shape: low-freq + medium-freq octaves.
                let continent = self.noise_base.sample2d(wx * 0.015, wz * 0.015) * 0.5
                              + self.noise_base.sample2d(wx * 0.04,  wz * 0.04) * 0.25;
                // Surface height relative to sea level 64. Continent range ~ -8..+8.
                let height = SEA_LEVEL as f64 + continent * 16.0;
                for y in 0..H {
                    let yi = y as i32;
                    let blk = if (yi as f64) < height - 1.0 {
                        block::STONE
                    } else if (yi as f64) < height {
                        // Top layer: grass if above sea level, sand at beaches.
                        if yi >= SEA_LEVEL { block::GRASS } else { block::SAND }
                    } else if yi < SEA_LEVEL {
                        block::WATER
                    } else {
                        block::AIR
                    };
                    out[(x << 11) | (z << 7) | y] = blk;
                }
                // Bedrock floor (y=0).
                out[(x << 11) | (z << 7) | 0] = block::BEDROCK;
                // Fill dirt under grass down to y = height - 4.
                for y in 1..(H.min((height - 3.0).max(1.0) as usize)) {
                    if out[(x << 11) | (z << 7) | y] == block::STONE {
                        out[(x << 11) | (z << 7) | y] = block::DIRT;
                    }
                }
            }
        }
        out
    }

    /// Highest solid block at column (x, z), or -1 if none.
    pub fn top_block(&self, blocks: &[u8], x: usize, z: usize) -> i32 {
        for y in (0..H).rev() {
            let b = blocks[(x << 11) | (z << 7) | y];
            if b != block::AIR && b != block::WATER {
                return y as i32;
            }
        }
        -1
    }
}
