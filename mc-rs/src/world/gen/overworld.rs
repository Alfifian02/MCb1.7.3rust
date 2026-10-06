//! Overworld chunk generator. Direct port of net.minecraft.src.ChunkProviderGenerate
//! from the b1.7.3 decomp. Vanilla equivalent: net.minecraft.world.level.levelgen.
//!
//! Generates a 16x128x16 chunk as a Vec<u8> of 32768 block ids. Caller supplies
//! the biome per column via `biomes[16*16]`. The overworld is grass+stone+water
//! above bedrock with sea level 64.

use crate::world::biome::Biome;
use crate::world::gen::noise::{OctaveNoise, JavaRandom};

pub const W: usize = 16;
pub const H: usize = 128;
pub const D: usize = 16;
pub const VOLUME: usize = W * H * D;
pub const SEA_LEVEL: i32 = 64;

/// Block ids (Beta-1.7 numbering, vanilla uses the same). Stable across versions.
pub mod block {
    pub const AIR: u8 = 0;
    pub const STONE: u8 = 1;
    pub const GRASS: u8 = 2;
    pub const DIRT: u8 = 3;
    pub const BEDROCK: u8 = 7;
    pub const WATER: u8 = 8;     // still water
    pub const LAVA: u8 = 10;     // still lava
    pub const SAND: u8 = 12;
    pub const GRAVEL: u8 = 13;
    pub const ICE: u8 = 79;
}

pub struct OverworldGenerator {
    // Five octaves used by Beta-1.7 ChunkProviderGenerate, in field-declaration order.
    noise_lim: OctaveNoise,   // field_912_k, 16 octaves
    noise_low: OctaveNoise,   // field_911_l, 16 octaves
    noise_base: OctaveNoise,  // field_910_m, 8 octaves
    noise_sand: OctaveNoise,  // field_909_n, 4 octaves
    noise_gravel: OctaveNoise,// field_908_o, 4 octaves
    noise_main: OctaveNoise,  // field_922_a, 10 octaves (used as base scale)
    noise_height: OctaveNoise,// field_921_b, 16 octaves (large-scale hills/mountains)
}

impl OverworldGenerator {
    pub fn new(world_seed: u64) -> Self {
        let mut rng = JavaRandom::new(world_seed);
        Self {
            noise_lim:    OctaveNoise::new(rng.next_u63(), 16),
            noise_low:    OctaveNoise::new(rng.next_u63(), 16),
            noise_base:   OctaveNoise::new(rng.next_u63(), 8),
            noise_sand:   OctaveNoise::new(rng.next_u63(), 4),
            noise_gravel: OctaveNoise::new(rng.next_u63(), 4),
            noise_main:   OctaveNoise::new(rng.next_u63(), 10),
            noise_height: OctaveNoise::new(rng.next_u63(), 16),
        }
    }

    /// Generate one 16x128x16 chunk. `biomes` is indexed [z*16 + x].
    /// Returns a flat Vec<u8> of length 32768, index (x << 11) | (z << 7) | y.
    pub fn generate(&self, chunk_x: i32, chunk_z: i32, biomes: &[Biome; 256]) -> Vec<u8> {
        // Single-chunk overworld: temperature and humidity are placeholders
        // (real values come from GenLayer in b1.7.3, which M3e will add).
        let mut temperature = [0.5f64; 16 * 16];
        let mut humidity = [0.5f64; 16 * 16];

        // Compute terrain densities for the 5x17x5 sample grid (Beta-1.7 sampling).
        // This is the 684.412 * 200.0 scale formula from decomp.
        let mut noise = [0.0f64; 5 * 17 * 5];
        let x0 = chunk_x * 4;
        let z0 = chunk_z * 4;
        let h = 684.412;
        let v = 684.412;
        // Beta-1.7 calls generateNoiseOctaves with scale 684.412/80, 684.412/160, 684.412/80 for base,
        // and full 684.412 for the lim/low noises, y-scale 200.0 for height noise.
        // For M3a we just need the stone-or-not test: density > 0 -> stone.
        // Sampling simplified: 5x5 at y=0 for an all-stone chunk approximation.
        // Real implementation is in M3d; this stub produces a flat stone terrain
        // matching Chunk::stone_pillar() so M3a is a no-op visible to the player.
        for xi in 0..5 {
            for zi in 0..5 {
                let wx = (x0 + xi as i32) as f64;
                let wz = (z0 + zi as i32) as f64;
                let v = self.noise_lim.sample2d(wx, wz) * 1.0
                      + self.noise_low.sample2d(wx, wz) * 1.0
                      + self.noise_base.sample2d(wx, wz) * 1.0;
                noise[zi * 5 + xi] = if v > -0.2 { 1.0 } else { 0.0 };
            }
        }

        // Build a flat bedrock-floor + stone half-chunk like the M1 demo, so the
        // player can see something while we land M3a. The full Beta-1.7 heightmap
        // and biome-aware top layers come in M3d.
        let mut out = vec![0u8; VOLUME];
        for y in 0..64 {
            for z in 0..D {
                for x in 0..W {
                    out[(x << 11) | (z << 7) | y] = block::STONE;
                }
            }
        }
        // Bedrock layer (y=0..5 random; simplified to y=0 only).
        for z in 0..D {
            for x in 0..W {
                out[(x << 11) | (z << 7) | 0] = block::BEDROCK;
            }
        }
        let _ = (biomes, temperature, humidity, h, v, noise);
        out
    }
}
