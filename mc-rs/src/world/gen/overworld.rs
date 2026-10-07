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
    // Ores (Beta-1.7 numbering, vanilla matches).
    pub const ORE_COAL: u8 = 16;
    pub const ORE_IRON: u8 = 15;
    pub const ORE_GOLD: u8 = 14;
    pub const ORE_DIAMOND: u8 = 56;
    pub const ORE_REDSTONE: u8 = 73;
    pub const ORE_LAPIS: u8 = 21;
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
        let xs0: i32 = chunk_x * 4;
        let zs0: i32 = chunk_z * 4;
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
        // Beta 1.7.3 samples density on a 5 x 17 x 5 grid (4 x 16 x 4 *cells*
        // of samples) and trilinearly interpolates between neighbouring samples:
        // each cell covers 4 (x) x 8 (y) x 4 (z) blocks. The old code expanded
        // all 5 samples to 4 blocks each (20 wide in a 16-wide chunk), which
        // indexed out of bounds and panicked during init.
        // `density` layout (from step 1): ((ix * sz) + iz) * sy + iy.
        let dens = |ix: usize, iy: usize, iz: usize| -> f64 { density[(ix * sz + iz) * sy + iy] };
        let mut out = vec![block::AIR; VOLUME];
        for ix in 0..(sx - 1) {
            for iz in 0..(sz - 1) {
                for iy in 0..(sy - 1) {
                    let c000 = dens(ix, iy, iz);
                    let c001 = dens(ix, iy, iz + 1);
                    let c100 = dens(ix + 1, iy, iz);
                    let c101 = dens(ix + 1, iy, iz + 1);
                    let c010 = dens(ix, iy + 1, iz);
                    let c011 = dens(ix, iy + 1, iz + 1);
                    let c110 = dens(ix + 1, iy + 1, iz);
                    let c111 = dens(ix + 1, iy + 1, iz + 1);
                    for dy in 0..8usize {
                        let y = iy * 8 + dy;
                        if y >= H { continue; }
                        let fy = dy as f64 / 8.0;
                        let x0z0 = c000 + (c010 - c000) * fy;
                        let x0z1 = c001 + (c011 - c001) * fy;
                        let x1z0 = c100 + (c110 - c100) * fy;
                        let x1z1 = c101 + (c111 - c101) * fy;
                        for dx in 0..4usize {
                            let fx = dx as f64 / 4.0;
                            let z0 = x0z0 + (x1z0 - x0z0) * fx;
                            let z1 = x0z1 + (x1z1 - x0z1) * fx;
                            let x = ix * 4 + dx;
                            for dz in 0..4usize {
                                let fz = dz as f64 / 4.0;
                                let d_val = z0 + (z1 - z0) * fz;
                                let z = iz * 4 + dz;
                                let cell_idx: usize = (x << 11) | (z << 7) | y;
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

impl OverworldGenerator {
    /// M3e-ores: populate a 16x16 chunk with ore veins. Beta-1.7's
    /// WorldGenMinable algorithm: an ellipsoid of stone->ore blocks.
    /// `blocks` is in absolute world coords (the same layout the mesher
    /// uses). For the super-chunk 48x48, pass `origin = (chunk_x*16, chunk_z*16)`.
    pub fn populate_ores(&mut self, blocks: &mut [u8], origin: (i32, i32)) {
        // Re-seed per the decomp's populate() formula.
        // Java's long is i64; we mirror that here.
        let world_seed: i64 = 0xCAFEBABEu64 as i64;
        let mut rng = JavaRandom::new(world_seed as u64);
        let _ = rng.next_long();
        let _ = rng.next_long();
        // Chunk-relative seed.
        let chunk_seed: i64 = ((origin.0 as i64).wrapping_mul(341873128712i64))
            .wrapping_add((origin.1 as i64).wrapping_mul(132897987541i64))
            ^ world_seed;
        let mut rng = JavaRandom::new(chunk_seed as u64);
        // Vein counts from decomp populate().
        let veins: &[((u8, i32), i32)] = &[
            ((block::ORE_COAL, 16), 20),
            ((block::ORE_IRON, 8),  20),
            ((block::ORE_GOLD, 8),  2),
            ((block::ORE_DIAMOND, 7), 1),
            ((block::ORE_REDSTONE, 7), 8),
            ((block::ORE_LAPIS, 6), 1),
        ];
        for &((ore, size), count) in veins {
            for _ in 0..count {
                let cx = origin.0 + (rng.next_u31() as i32 % 16) + 8;
                let cy = rng.next_u31() as i32 % 128;
                let cz = origin.1 + (rng.next_u31() as i32 % 16) + 8;
                // Some ores have y-bounds tighter than 128.
                let max_y = match ore {
                    block::ORE_IRON => 64,
                    block::ORE_GOLD => 32,
                    block::ORE_DIAMOND => 16,
                    block::ORE_REDSTONE => 16,
                    _ => 128,
                };
                let cy = cy.min(max_y - 1);
                self.place_vein(blocks, &mut rng, ore, size, cx, cy, cz, origin);
            }
        }
    }

    fn place_vein(&self, blocks: &mut [u8], rng: &mut JavaRandom, ore: u8, size: i32, x0: i32, y0: i32, z0: i32, _origin: (i32, i32)) {
        // Direct port of WorldGenMinable.generate(). Only places ore inside
        // the super-chunk's 48x48 footprint.
        let angle = rng.next_u31() as f64 / 4294967295.0 * std::f64::consts::PI;
        let sin_a = angle.sin();
        let cos_a = angle.cos();
        let dx0 = (x0 as f64 + 8.0) + sin_a * (size as f64) / 8.0;
        let dx1 = (x0 as f64 + 8.0) - sin_a * (size as f64) / 8.0;
        let dz0 = (z0 as f64 + 8.0) + cos_a * (size as f64) / 8.0;
        let dz1 = (z0 as f64 + 8.0) - cos_a * (size as f64) / 8.0;
        let dy0 = y0 as f64 + (rng.next_u31() as i32 % 3 + 2) as f64;
        let dy1 = y0 as f64 + (rng.next_u31() as i32 % 3 + 2) as f64;
        for i in 0..=size {
            let t = i as f64 / size as f64;
            let cx = dx0 + (dx1 - dx0) * t;
            let cy = dy0 + (dy1 - dy0) * t;
            let cz = dz0 + (dz1 - dz0) * t;
            let r = (rng.next_u31() as f64 / 4294967295.0) * (size as f64) / 16.0;
            let swell = ((i as f64 * std::f64::consts::PI / size as f64).sin() + 1.0) * r + 1.0;
            let x_lo = (cx - swell / 2.0).floor() as i32;
            let x_hi = (cx + swell / 2.0).floor() as i32;
            let y_lo = (cy - swell / 2.0).floor() as i32;
            let y_hi = (cy + swell / 2.0).floor() as i32;
            let z_lo = (cz - swell / 2.0).floor() as i32;
            let z_hi = (cz + swell / 2.0).floor() as i32;
            for bx in x_lo..=x_hi {
                let dxn = (bx as f64 + 0.5 - cx) / (swell / 2.0);
                if dxn * dxn >= 1.0 { continue; }
                for by in y_lo..=y_hi {
                    let dyn_ = (by as f64 + 0.5 - cy) / (swell / 2.0);
                    if dxn * dxn + dyn_ * dyn_ >= 1.0 { continue; }
                    for bz in z_lo..=z_hi {
                        let dzn = (bz as f64 + 0.5 - cz) / (swell / 2.0);
                        if dxn * dxn + dyn_ * dyn_ + dzn * dzn >= 1.0 { continue; }
                        // Only place if inside the 48x48 super-chunk.
                        if bx < 0 || bx >= 48 || bz < 0 || bz >= 48 || by < 0 || by >= 128 { continue; }
                        let idx = (bx as usize) << 11 | (bz as usize) << 7 | (by as usize);
                        if blocks[idx] == block::STONE {
                            blocks[idx] = ore;
                        }
                    }
                }
            }
        }
    }
}
