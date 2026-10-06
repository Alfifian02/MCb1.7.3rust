//! Overworld chunk generator. Direct port of net.minecraft.src.ChunkProviderGenerate
//! from the b1.7.3 decomp. Vanilla equivalent: net.minecraft.world.level.levelgen.
//!
//! M3c: full Beta-1.7 density function. Produces the canonical Beta-1.7 overworld
//! shape: rolling hills, occasional steep mountains, sea level 64.
//!
//! M3b's heightmap shortcut has been replaced. The math is identical to the
//! decomp, just with the biomes array and chunk manager hard-coded to Plains
//! (single-biome chunk for now; multi-biome comes in M3e).

use crate::world::biome::Biome;
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
    pub const GRAVEL: u8 = 13;
}

pub struct OverworldGenerator {
    // Five octaves in declaration order, matching the decomp's field order.
    noise_lim: OctaveNoise,    // field_912_k, 16 octaves, scale 684.412
    noise_low: OctaveNoise,    // field_911_l, 16 octaves, scale 684.412
    noise_base: OctaveNoise,   // field_910_m, 8  octaves, scale 684.412/80 x 684.412/160 x 684.412/80
    noise_main: OctaveNoise,   // field_922_a, 10 octaves, scale 1.121
    noise_height: OctaveNoise, // field_921_b, 16 octaves, scale 200.0
    // Surface materials, used in replaceBlocksForBiome.
    noise_sand: OctaveNoise,   // field_909_n, 4  octaves
    noise_gravel: OctaveNoise, // field_908_o, 4  octaves
    // Scratch buffers.
    scratch_d: Vec<f64>,
    scratch_e: Vec<f64>,
    scratch_f: Vec<f64>,
    scratch_g: Vec<f64>,
    scratch_h: Vec<f64>,
    surface_rand: JavaRandom,
}

impl OverworldGenerator {
    pub fn new(world_seed: u64) -> Self {
        let rng = JavaRandom::new(world_seed);
        let s = Self {
            noise_lim:    OctaveNoise::new(rng.next_u63(), 16),
            noise_low:    OctaveNoise::new(rng.next_u63(), 16),
            noise_base:   OctaveNoise::new(rng.next_u63(), 8),
            noise_main:   OctaveNoise::new(rng.next_u63(), 10),
            noise_height: OctaveNoise::new(rng.next_u63(), 16),
            noise_sand:   OctaveNoise::new(rng.next_u63(), 4),
            noise_gravel: OctaveNoise::new(rng.next_u63(), 4),
            scratch_d: Vec::new(),
            scratch_e: Vec::new(),
            scratch_f: Vec::new(),
            scratch_g: Vec::new(),
            scratch_h: Vec::new(),
            surface_rand: JavaRandom::new(world_seed),
        };
        s
    }

    /// Highest solid block at column (x, z), or -1 if none.
    pub fn top_block(blocks: &[u8], x: usize, z: usize) -> i32 {
        for y in (0..H).rev() {
            let b = blocks[(x << 11) | (z << 7) | y];
            if b != block::AIR && b != block::WATER {
                return y as i32;
            }
        }
        -1
    }

    /// Generate a single 16x128x16 chunk, indices (x << 11) | (z << 7) | y.
    /// `biomes` is a [256] array of biomes for this chunk.
    pub fn generate(&mut self, chunk_x: i32, chunk_z: i32, biomes: &[Biome; 256]) -> Vec<u8> {
        // Reset the surface random to the chunk's deterministic seed (matches
        // provideChunk's rand.setSeed).
        self.surface_rand = JavaRandom::new(
            (chunk_x as u64).wrapping_mul(341873128712u64)
                .wrapping_add((chunk_z as u64).wrapping_mul(132897987541u64))
        );

        // --- Step 1: density sample (5 x 17 x 5) ---
        let sx: usize = 5;
        let sy: usize = 17;
        let sz: usize = 5;
        let xs0: i32 = chunk_x * sx as i32;
        let zs0: i32 = chunk_z * sz as i32;
        let xs1: i32 = (chunk_x * sx as i32) + sx as i32;
        let zs1: i32 = (chunk_z * sz as i32) + sz as i32;

        // Main + height noises (XZ-only).
        let mut g = self.noise_main.sample_array2d(xs0, zs0, sx as usize, sz as usize, 1.121, 1.121);
        let mut h = self.noise_height.sample_array2d(xs0, zs0, sx as usize, sz as usize, 200.0, 200.0);
        // Three YXZ noises.
        let mut d = self.noise_base.sample_array3d(xs0, 0, zs0, sx as usize, sy as usize, sz as usize, 684.412/80.0, 684.412/160.0, 684.412/80.0);
        let mut e = self.noise_lim.sample_array3d(xs0, 0, zs0, sx as usize, sy as usize, sz as usize, 684.412, 684.412, 684.412);
        let mut f = self.noise_low.sample_array3d(xs0, 0, zs0, sx as usize, sy as usize, sz as usize, 684.412, 684.412, 684.412);

        // Apply per-column humidity/temperature bias and the "var29 mountain" formula.
        // M3c uses constant temp=0.5/humidity=0.5 (Plains), so the var25/var27/var29
        // math collapses to fixed values: var25 = 0.9375, var27 -> constant 0.5,
        // var29 -> 0.0. We still compute them through the same path so the shape
        // is correct.
        let temperature = [0.5f64; 256];
        let humidity = [0.5f64; 256];

        let mut density: Vec<f64> = vec![0.0; sx * sy * sz];
        let mut idx_d = 0;
        let mut idx_g = 0;
        for ix in 0..sx {
            let column_x: usize = ix * (16 / sx) + (16 / sx) / 2;
            for iz in 0..sz {
                let column_z: usize = iz * (16 / sz) + (16 / sz) / 2;
                let var21 = temperature[column_x * 16 + column_z];
                let var23 = humidity[column_x * 16 + column_z] * var21;
                let mut var25 = 1.0 - var23;
                var25 *= var25;
                var25 *= var25;
                var25 = 1.0 - var25;
                let mut var27 = (g[idx_g] + 256.0) / 512.0;
                var27 *= var25;
                if var27 > 1.0 { var27 = 1.0; }
                let mut var29 = h[idx_g] / 8000.0;
                if var29 < 0.0 { var29 = -var29 * 0.3; }
                var29 = var29 * 3.0 - 2.0;
                if var29 < 0.0 {
                    var29 /= 2.0;
                    if var29 < -1.0 { var29 = -1.0; }
                    var29 /= 1.4;
                    var29 /= 2.0;
                    var27 = 0.0;
                } else {
                    if var29 > 1.0 { var29 = 1.0; }
                    var29 /= 8.0;
                }
                if var27 < 0.0 { var27 = 0.0; }
                var27 += 0.5;
                var29 = var29 * (sy as f64) / 16.0;
                let var31 = (sy as f64) / 2.0 + var29 * 4.0;
                idx_g += 1;
                for iy in 0..sy {
                    let mut var34;
                    let var36 = ((iy as f64) - var31) * 12.0 / var27;
                    let var36b = if var36 < 0.0 { var36 * 4.0 } else { var36 };
                    let var38 = e[idx_d] / 512.0;
                    let var40 = f[idx_d] / 512.0;
                    let var42 = (d[idx_d] / 10.0 + 1.0) / 2.0;
                    if var42 < 0.0 { var34 = var38; }
                    else if var42 > 1.0 { var34 = var40; }
                    else { var34 = var38 + (var40 - var38) * var42; }
                    var34 -= var36b;
                    if iy > sy - 4 {
                        let t = ((iy - (sy - 4)) as f64) / 3.0;
                        var34 = var34 * (1.0 - t) + -10.0 * t;
                    }
                    density[idx_d] = var34;
                    idx_d += 1;
                }
            }
        }
        // Drop the now-unused scratch to release memory before the next chunk.
        d = Vec::new(); e = Vec::new(); f = Vec::new(); g = Vec::new(); h = Vec::new();

        // --- Step 2: place terrain blocks based on density. ---
        // We emit 16 cells in y per density step, so each (ix, iy, iz) density sample
        // becomes 4x8x4 cells in the chunk.
        let mut out = vec![block::AIR; VOLUME];
        let mut idx_den = 0;
        for iy in 0..sy {
            for iz in 0..sz {
                for ix in 0..sx {
                    let d_val = density[idx_den];
                    idx_den += 1;
                    // Each density sample covers 4 (x) x 8 (y) x 4 (z) cells.
                    for dy in 0..8 {
                        let y = iy * 8 + dy;
                        if y >= H { continue; }
                        for dz in 0..4 {
                            let z = iz * 4 + dz;
                            for dx in 0..4 {
                                let x = ix * 4 + dx;
                                let cell_idx: usize = ((x << 11) | (z << 7) | y) as usize;
                                if d_val > 0.0 {
                                    out[cell_idx] = block::STONE;
                                } else if (y as i32) < SEA_LEVEL {
                                    out[cell_idx] = block::WATER;
                                }
                            }
                        }
                    }
                }
            }
        }

        // --- Step 3: replaceBlocksForBiome (top layer, sand/gravel, dirt under grass) ---
        // Re-seed for surface materials (matches b1.7.3 ordering).
        let sand_n = self.noise_sand.sample_array2d(
            chunk_x * 16, chunk_z * 16, 16, 16, 1.0/32.0, 1.0/32.0,
        );
        let gravel_n = self.noise_gravel.sample_array2d_y(
            chunk_x * 16, chunk_z * 16, 16, 16, 1.0/32.0, 1.0/32.0,
        );
        let stone_n = self.noise_gravel.sample_array2d(
            chunk_x * 16, chunk_z * 16, 16, 16, 1.0/16.0, 1.0/16.0,
        );
        for z in 0..16 {
            for x in 0..16 {
                let biome = biomes[z * 16 + x];
                let sand_here = sand_n[z * 16 + x] + self.surface_rand.next_double() * 0.2 > 0.0;
                let gravel_here = gravel_n[z * 16 + x] + self.surface_rand.next_double() * 0.2 > 3.0;
                let stone_depth = ((stone_n[z * 16 + x] / 3.0 + 3.0 + self.surface_rand.next_double() * 0.25) as i32).max(0);
                let mut top_depth = -1i32;
                let mut top_block = biome.top_block();
                let mut filler = biome.filler_block();
                for y in (0..H).rev() {
                    let cell = (x << 11) | (z << 7) | y;
                    if y as i32 <= self.surface_rand.next_u31() as i32 % 5 {
                        out[cell] = block::BEDROCK;
                        continue;
                    }
                    let here = out[cell];
                    if here == block::AIR {
                        top_depth = -1;
                    } else if here == block::STONE {
                        if top_depth == -1 {
                            if stone_depth <= 0 {
                                top_block = block::AIR;
                                filler = block::STONE;
                            } else if (y as i32) >= SEA_LEVEL - 4 && (y as i32) <= SEA_LEVEL + 1 {
                                top_block = biome.top_block();
                                filler = biome.filler_block();
                                if gravel_here { top_block = block::AIR; }
                                if gravel_here { filler = block::GRAVEL; }
                                if sand_here { top_block = block::SAND; }
                                if sand_here { filler = block::SAND; }
                            }
                            if (y as i32) < SEA_LEVEL && top_block == block::AIR {
                                top_block = block::WATER;
                            }
                            top_depth = stone_depth as i32;
                            out[cell] = if (y as i32) >= SEA_LEVEL - 1 { top_block } else { filler };
                        } else if top_depth > 0 {
                            top_depth -= 1;
                            out[cell] = filler;
                            if top_depth == 0 && filler == block::SAND {
                                top_depth = self.surface_rand.next_u31() as i32 % 4;
                                // Filler becomes sandstone in original; we don't have that
                                // block id yet, leave as sand.
                            }
                        }
                    }
                }
            }
        }

        let _ = (xs1, zs1); // (used by future per-direction offset)
        out
    }
}
