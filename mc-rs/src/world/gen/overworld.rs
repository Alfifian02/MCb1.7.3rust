//! Overworld chunk generator: port of ChunkProviderGenerate (b1.7.3). `generate` is provideChunk
//! (terrain, biome surface, caves); populate() (trees, ores, ...) lives in `populate.rs`.
//!
//! Verified against the real Java classes: see `matches_java_reference`, `generate_matches_java`
//! and tools/golden/.

use crate::world::biome::Biome;
use crate::world::gen::caves;
use crate::world::gen::chunk_manager::WorldChunkManager;
use crate::world::gen::noise::{JavaRandom, OctaveNoise};

pub const W: usize = 16;
pub const H: usize = 128;
pub const D: usize = 16;
pub const VOLUME: usize = W * H * D;
pub const SEA_LEVEL: i32 = 64;

/// Block ids (Beta-1.7 numbering).
pub mod block {
    pub const AIR: u8 = 0;
    pub const STONE: u8 = 1;
    pub const GRASS: u8 = 2;
    pub const DIRT: u8 = 3;
    pub const COBBLE: u8 = 4;
    pub const BEDROCK: u8 = 7;
    /// Block.waterMoving / waterStill (terrain uses 9; caves and springs use the moving ones).
    pub const WATER_MOVING: u8 = 8;
    pub const WATER: u8 = 9;
    pub const LAVA_MOVING: u8 = 10;
    pub const LAVA: u8 = 11;
    pub const SAND: u8 = 12;
    pub const GRAVEL: u8 = 13;
    pub const LOG: u8 = 17;
    pub const LEAVES: u8 = 18;
    pub const SANDSTONE: u8 = 24;
    pub const TALL_GRASS: u8 = 31;
    pub const DEAD_BUSH: u8 = 32;
    pub const FLOWER_Y: u8 = 37;
    pub const FLOWER_R: u8 = 38;
    pub const MUSH_BROWN: u8 = 39;
    pub const MUSH_RED: u8 = 40;
    pub const MOSSY: u8 = 48;
    pub const SPAWNER: u8 = 52;
    pub const CHEST: u8 = 54;
    pub const SNOW: u8 = 78;
    pub const ICE: u8 = 79;
    pub const CACTUS: u8 = 81;
    pub const CLAY: u8 = 82;
    pub const REED: u8 = 83;
    pub const PUMPKIN: u8 = 86;
    // Ores (Beta-1.7 numbering, vanilla matches).
    pub const ORE_COAL: u8 = 16;
    pub const ORE_IRON: u8 = 15;
    pub const ORE_GOLD: u8 = 14;
    pub const ORE_DIAMOND: u8 = 56;
    pub const ORE_REDSTONE: u8 = 73;
    pub const ORE_LAPIS: u8 = 21;
}

/// One column of LOD terrain: the height of its top face and the block that is drawn there.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct LodCol {
    pub top: u8,
    pub block: u8,
}

pub struct OverworldGenerator {
    /// World seed (MapGenBase and populate re-seed from it).
    pub(super) seed: i64,
    /// Shared Random: seeds the noise stacks in order, then is reseeded per
    /// chunk and drives the surface pass.
    rand: JavaRandom,
    noise_lim: OctaveNoise,    // field_912_k, 16 octaves
    noise_low: OctaveNoise,    // field_911_l, 16
    noise_base: OctaveNoise,   // field_910_m, 8
    noise_sand: OctaveNoise,   // field_909_n, 4 (sand and gravel)
    noise_stone: OctaveNoise,  // field_908_o, 4
    noise_main: OctaveNoise,   // field_922_a, 10
    noise_height: OctaveNoise, // field_921_b, 16
    /// mobSpawnerNoise, 8 octaves, last in the constructor's order: populate's tree count.
    pub(super) noise_spawner: OctaveNoise,
}

impl OverworldGenerator {
    pub fn new(seed: i64) -> Self {
        // Order matters: every stack draws from the same Random.
        let mut rand = JavaRandom::new(seed);
        let noise_lim = OctaveNoise::new(&mut rand, 16);
        let noise_low = OctaveNoise::new(&mut rand, 16);
        let noise_base = OctaveNoise::new(&mut rand, 8);
        let noise_sand = OctaveNoise::new(&mut rand, 4);
        let noise_stone = OctaveNoise::new(&mut rand, 4);
        let noise_main = OctaveNoise::new(&mut rand, 10);
        let noise_height = OctaveNoise::new(&mut rand, 16);
        let noise_spawner = OctaveNoise::new(&mut rand, 8);
        Self { seed, rand, noise_lim, noise_low, noise_base, noise_sand, noise_stone, noise_main, noise_height, noise_spawner }
    }

    /// Highest solid block at column (x, z) of a grid `depth` blocks deep in Z, or -1 if none.
    pub fn top_block(blocks: &[u8], x: usize, z: usize, depth: usize) -> i32 {
        for y in (0..H).rev() {
            let b = blocks[(x * depth + z) * H + y];
            if b != block::AIR && b != block::WATER {
                return y as i32;
            }
        }
        -1
    }

    /// provideChunk: one 16x128x16 chunk (terrain, surface, caves), indexed `(x << 11) | (z << 7) | y`.
    pub fn generate(&mut self, chunk_x: i32, chunk_z: i32, cm: &mut WorldChunkManager) -> Vec<u8> {
        self.rand.set_seed(chunk_seed(chunk_x, chunk_z));
        let biomes = cm.load_block_generator_data(chunk_x * 16, chunk_z * 16, 16, 16);
        let mut blocks = vec![block::AIR; VOLUME];
        self.generate_terrain(chunk_x, chunk_z, &mut blocks, &cm.temperature, &cm.humidity);
        self.replace_blocks_for_biome(chunk_x, chunk_z, &mut blocks, &biomes);
        caves::carve(self.seed, chunk_x, chunk_z, &mut blocks);
        blocks
    }

    /// func_4061_a: the 5x17x5 density grid of a chunk (`x`, `z` = chunk * 4). The climate is read at the block
    /// `ix * 3 + 1` of the chunk, as the Java does (`16 / 5` is 3).
    fn density(&self, x: i32, z: i32, temps: &[f64], hums: &[f64]) -> Vec<f64> {
        self.density_grid(x as f64, z as f64, 5, 1.0, |ix, iz| {
            let b = (ix * 3 + 1) * 16 + iz * 3 + 1;
            (temps[b], hums[b])
        })
    }

    /// The density grid, `n` x 17 x `n` nodes: node (ix, iz) sits at (`x` + ix * `s`, `z` + iz * `s`) in node units (4 blocks);
    /// `s` = 1.0 and `n` = 5 is the vanilla chunk grid, bit for bit (every `* 1.0` and `/ 1.0` is exact). A larger `s`
    /// strides the same noise, which is what the LOD terrain uses. `clim(ix, iz)` = (temperature, humidity) of a column.
    fn density_grid(&self, x: f64, z: f64, n: usize, s: f64, clim: impl Fn(usize, usize) -> (f64, f64)) -> Vec<f64> {
        const SY: usize = 17;
        let (sx, sz) = (n, n);
        let (xf, zf) = (x / s, z / s);
        let d_main = self.noise_main.generate(xf, 10.0, zf, sx, 1, sz, 1.121 * s, 1.0, 1.121 * s);
        let d_height = self.noise_height.generate(xf, 10.0, zf, sx, 1, sz, 200.0 * s, 1.0, 200.0 * s);
        let d_base = self.noise_base.generate(xf, 0.0, zf, sx, SY, sz, 684.412 / 80.0 * s, 684.412 / 160.0, 684.412 / 80.0 * s);
        let d_lim = self.noise_lim.generate(xf, 0.0, zf, sx, SY, sz, 684.412 * s, 684.412, 684.412 * s);
        let d_low = self.noise_low.generate(xf, 0.0, zf, sx, SY, sz, 684.412 * s, 684.412, 684.412 * s);

        let mut out = vec![0.0; sx * SY * sz];
        let mut i = 0;
        let mut col = 0;
        for ix in 0..sx {
            for iz in 0..sz {
                let (t, hum) = clim(ix, iz);
                let h = hum * t;
                let mut v25 = 1.0 - h;
                v25 *= v25;
                v25 *= v25;
                v25 = 1.0 - v25;
                let mut v27 = (d_main[col] + 256.0) / 512.0;
                v27 *= v25;
                if v27 > 1.0 { v27 = 1.0; }
                let mut v29 = d_height[col] / 8000.0;
                if v29 < 0.0 { v29 = -v29 * 0.3; }
                v29 = v29 * 3.0 - 2.0;
                if v29 < 0.0 {
                    v29 /= 2.0;
                    if v29 < -1.0 { v29 = -1.0; }
                    v29 /= 1.4;
                    v29 /= 2.0;
                    v27 = 0.0;
                } else {
                    if v29 > 1.0 { v29 = 1.0; }
                    v29 /= 8.0;
                }
                if v27 < 0.0 { v27 = 0.0; }
                v27 += 0.5;
                v29 = v29 * SY as f64 / 16.0;
                let v31 = SY as f64 / 2.0 + v29 * 4.0;
                col += 1;
                for iy in 0..SY {
                    let mut v36 = (iy as f64 - v31) * 12.0 / v27;
                    if v36 < 0.0 { v36 *= 4.0; }
                    let lim = d_lim[i] / 512.0;
                    let low = d_low[i] / 512.0;
                    let mix = (d_base[i] / 10.0 + 1.0) / 2.0;
                    let mut v34 = if mix < 0.0 { lim } else if mix > 1.0 { low } else { lim + (low - lim) * mix };
                    v34 -= v36;
                    if iy > SY - 4 {
                        // The Java divides in float here.
                        let v44 = ((iy - (SY - 4)) as f32 / 3.0_f32) as f64;
                        v34 = v34 * (1.0 - v44) + -10.0 * v44;
                    }
                    out[i] = v34;
                    i += 1;
                }
            }
        }
        out
    }

    /// LOD terrain (`render::lod`): the top of the ground for an `n` x `n` grid of `cell`-block cells, the first one with its
    /// corner at block (`x0`, `z0`); index `ix * n + iz`. Only the density grid is evaluated, at the middle of each cell (the
    /// vanilla grid is every 4 blocks, so `cell` = 4 is as fine as the terrain gets): no chunk, no caves, no populate.
    /// The surface is the highest node with density > 0, put where the density crosses 0 on the line to the node above, which
    /// is where `generate_terrain`'s trilinear blend crosses it too (that one is linear in y).
    // UNVERIFIED: not diffed against `generate_terrain` bit for bit (cells are sampled at their middle, not on the node
    // grid); `lod_follows_the_terrain` bounds the error. Left out: trees, gravel, the jitter on the sand test, caves.
    pub fn lod_columns(&self, cm: &mut WorldChunkManager, x0: i32, z0: i32, n: usize, cell: i32) -> Vec<LodCol> {
        let c = cell as f64;
        let (px, pz) = (x0 as f64 + c / 2.0, z0 as f64 + c / 2.0);
        let biomes = cm.climate(px, pz, n, n, c);
        let (temps, hums) = (&cm.temperature, &cm.humidity);
        let d = self.density_grid(px / 4.0, pz / 4.0, n, c / 4.0, |ix, iz| (temps[ix * n + iz], hums[ix * n + iz]));
        // noise_sand as `replace_blocks_for_biome` reads it (x and z in the first two slots, scale 1/32), strided like the rest.
        let sand = self.noise_sand.generate(px / c, pz / c, 0.0, n, n, 1, c / 32.0, c / 32.0, 1.0);
        (0..n * n).map(|i| {
            let col = &d[i * 17..i * 17 + 17];
            // Node 16 is always <= 0 (the fade at the top of the grid), so k + 1 <= 16 is not above ground.
            let top = (0..16).rev().find(|&k| col[k] > 0.0).map_or(1, |k| {
                let y = k as f64 * 8.0 + 8.0 * col[k] / (col[k] - col[k + 1]);
                (y.floor() as i32 + 1).clamp(1, H as i32)
            });
            let cold = temps[i] < 0.5;
            if top < SEA_LEVEL {
                return LodCol { top: SEA_LEVEL as u8, block: if cold { block::ICE } else { block::WATER } };
            }
            let mut b = biomes[i].top_block();
            // The beach band of `replace_blocks_for_biome`: top block within sea - 4 ..= sea + 1.
            if (SEA_LEVEL - 4..=SEA_LEVEL + 1).contains(&(top - 1)) && sand[i] > 0.0 { b = block::SAND; }
            LodCol { top: top as u8, block: if cold { 80 } else { b } } // 80 = snow block: populate's snow layer, flat
        }).collect()
    }

    /// generateTerrain: interpolate the density grid into stone/water/ice.
    /// The incremental `+=` interpolation is kept as in Java so the sign of
    /// the density (stone vs not) matches bit for bit.
    fn generate_terrain(&self, chunk_x: i32, chunk_z: i32, blocks: &mut [u8], temps: &[f64], hums: &[f64]) {
        let d = self.density(chunk_x * 4, chunk_z * 4, temps, hums);
        let at = |i: usize, j: usize, k: usize| d[(i * 5 + j) * 17 + k];
        for i in 0..4usize {
            for j in 0..4usize {
                for k in 0..16usize {
                    let mut v16 = at(i, j, k);
                    let mut v18 = at(i, j + 1, k);
                    let mut v20 = at(i + 1, j, k);
                    let mut v22 = at(i + 1, j + 1, k);
                    let v24 = (at(i, j, k + 1) - v16) * 0.125;
                    let v26 = (at(i, j + 1, k + 1) - v18) * 0.125;
                    let v28 = (at(i + 1, j, k + 1) - v20) * 0.125;
                    let v30 = (at(i + 1, j + 1, k + 1) - v22) * 0.125;
                    for l in 0..8usize {
                        let y = k * 8 + l;
                        let mut v35 = v16;
                        let mut v37 = v18;
                        let v39 = (v20 - v16) * 0.25;
                        let v41 = (v22 - v18) * 0.25;
                        for m in 0..4usize {
                            let mut pos = ((m + i * 4) << 11) | ((j * 4) << 7) | y;
                            let mut v48 = v35;
                            let v50 = (v37 - v35) * 0.25;
                            for n in 0..4usize {
                                let t = temps[(i * 4 + m) * 16 + j * 4 + n];
                                let mut b = block::AIR;
                                if (y as i32) < SEA_LEVEL {
                                    b = if t < 0.5 && y as i32 >= SEA_LEVEL - 1 { block::ICE } else { block::WATER };
                                }
                                if v48 > 0.0 {
                                    b = block::STONE;
                                }
                                blocks[pos] = b;
                                pos += 128;
                                v48 += v50;
                            }
                            v35 += v39;
                            v37 += v41;
                        }
                        v16 += v24;
                        v18 += v26;
                        v20 += v28;
                        v22 += v30;
                    }
                }
            }
        }
    }

    /// replaceBlocksForBiome: grass/dirt/sand/gravel skin and bedrock floor.
    /// `biomes` is x-major (`x * 16 + z`), as from the chunk manager.
    fn replace_blocks_for_biome(&mut self, chunk_x: i32, chunk_z: i32, blocks: &mut [u8], biomes: &[Biome]) {
        let s = 1.0 / 32.0;
        let (bx, bz) = ((chunk_x * 16) as f64, (chunk_z * 16) as f64);
        // Sand and gravel both use noise_sand; gravel samples an x/z plane,
        // sand an x/y plane (z fixed at 0). Quirks kept from the Java.
        let sand_n = self.noise_sand.generate(bx, bz, 0.0, 16, 16, 1, s, s, 1.0);
        let gravel_n = self.noise_sand.generate(bx, 109.0134, bz, 16, 1, 16, s, 1.0, s);
        let stone_n = self.noise_stone.generate(bx, bz, 0.0, 16, 16, 1, s * 2.0, s * 2.0, s * 2.0);
        let sea = SEA_LEVEL;
        for a in 0..16usize {
            for b in 0..16usize {
                let biome = biomes[a + b * 16];
                let sand = sand_n[a + b * 16] + self.rand.next_double() * 0.2 > 0.0;
                let gravel = gravel_n[a + b * 16] + self.rand.next_double() * 0.2 > 3.0;
                let stone = (stone_n[a + b * 16] / 3.0 + 3.0 + self.rand.next_double() * 0.25) as i32;
                let mut depth = -1;
                let mut top = biome.top_block();
                let mut filler = biome.filler_block();
                for y in (0..H).rev() {
                    let idx = (b * 16 + a) * 128 + y;
                    if y as i32 <= self.rand.next_int_bound(5) {
                        blocks[idx] = block::BEDROCK;
                        continue;
                    }
                    let cur = blocks[idx];
                    if cur == block::AIR {
                        depth = -1;
                    } else if cur == block::STONE {
                        if depth == -1 {
                            if stone <= 0 {
                                top = block::AIR;
                                filler = block::STONE;
                            } else if y as i32 >= sea - 4 && y as i32 <= sea + 1 {
                                top = biome.top_block();
                                filler = biome.filler_block();
                                if gravel { top = block::AIR; }
                                if gravel { filler = block::GRAVEL; }
                                if sand { top = block::SAND; }
                                if sand { filler = block::SAND; }
                            }
                            if (y as i32) < sea && top == block::AIR {
                                top = block::WATER;
                            }
                            depth = stone;
                            blocks[idx] = if y as i32 >= sea - 1 { top } else { filler };
                        } else if depth > 0 {
                            depth -= 1;
                            blocks[idx] = filler;
                            if depth == 0 && filler == block::SAND {
                                depth = self.rand.next_int_bound(4);
                                filler = block::SANDSTONE;
                            }
                        }
                    }
                }
            }
        }
    }
}

/// provideChunk's per-chunk Random seed.
fn chunk_seed(chunk_x: i32, chunk_z: i32) -> i64 {
    (chunk_x as i64).wrapping_mul(341873128712).wrapping_add((chunk_z as i64).wrapping_mul(132897987541))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fnv(bytes: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }

    /// (seed, chunk x, chunk z, [biome hash, terrain hash, surface hash,
    /// temp[0] bits, humidity[0] bits, temp[255] bits, humidity[255] bits]),
    /// produced by the real b1.7.3 ChunkProviderGenerate / WorldChunkManager
    /// (tools/golden/G.java). Terrain and surface are hashed after
    /// generateTerrain and after replaceBlocksForBiome; caves are not run.
    #[allow(clippy::type_complexity)]
    const GOLDEN: &[(i64, i32, i32, [u64; 7])] = &[
        (3405691582, 0, 0, [0x1327a4fb26c0378a, 0x3967fa67a187f317, 0x9a56ede6c16cd66b, 0x3feae9d10fe31574, 0x3fd096af2aa2654b, 0x3fea30ff0dc61801, 0x3fc4e7839aba6def]),
        (3405691582, -1, 0, [0x8faa464d6523b860, 0xec8a1c3f37cf5e73, 0xfc6e42d5df332ea1, 0x3febf64daf796031, 0x3fc4b5a474e86e7e, 0x3fea1e99191a70cb, 0x3fc7093585cd1d7e]),
        (3405691582, 3, -5, [0x683f678012279f2c, 0x0ba3057b77f361b6, 0x0e735f70424439c2, 0x3febc224a46d4046, 0x3fd4cf0088b65297, 0x3fe979e85d51756d, 0x3fcb537d10815712]),
        (3405691582, 7, 7, [0xc8727fb8ed51d025, 0x6b09a800ea91114d, 0x1cbcf19473960aa7, 0x3fec14a3a633f4cf, 0x3fe3d9ee7893a3d0, 0x3fed87af244c3f78, 0x3fe0b4927eebc9c7]),
        (3405691582, -9, 12, [0xc8727fb8ed51d025, 0xdab56ccd58b1fcc5, 0xbc75dc0940e2ea88, 0x3fee3a7f389b51b5, 0x3fd96702c6b4913d, 0x3fed4852a7cd85c8, 0x3fdbb3e37dabbb1a]),
        (3405691582, 20, -20, [0xc8727fb8ed51d025, 0x55c7686520d178cd, 0x41f3e604c1007e4e, 0x3fe84fdb5be4711d, 0x3fec5650603fe3cb, 0x3fe7f209f60e52a8, 0x3fea8bcb2072fe81]),
        (12345, 0, 0, [0x6d851e31644b63d9, 0x89fd438a9df0c135, 0xed5f3c87f3c8df60, 0x3fef189e7c134589, 0x0000000000000000, 0x3fee271e1a29ffcf, 0x0000000000000000]),
        (12345, -1, 0, [0x295f1101e4692025, 0x7c44e7cfe08ba9ea, 0x7d715d7ac61d409c, 0x3fef76a014c9e1ee, 0x0000000000000000, 0x3fef01afbcd40ed7, 0x0000000000000000]),
        (12345, 3, -5, [0x295f1101e4692025, 0x851e2fc766a4336d, 0xdface7a7d01407e6, 0x3fef0dbcb26e96f1, 0x0000000000000000, 0x3fef73be938f8508, 0x0000000000000000]),
        (12345, 7, 7, [0x295f1101e4692025, 0xc7a406f41ec9ee9e, 0x9a7f11f6f047f8ed, 0x3fefe590ce0388c5, 0x3faf0876f6159bf4, 0x3feff4600d822d7f, 0x3f9c63f177c8c1c8]),
        (12345, -9, 12, [0x9c0e1f6aa8bc6325, 0xd8b8171f0e537465, 0x90a0926da3bd22ab, 0x3fefed1bed638966, 0x3fe65abd6a73b9f3, 0x3feffb77a92ee4d9, 0x3fe49d2764fe3ab5]),
        (12345, 20, -20, [0x9c0e1f6aa8bc6325, 0x7cdca60de33f86b6, 0xa310fd6d438f5808, 0x3fefff7baffafaa5, 0x3fe389a50c7ebfcf, 0x3fefb0e58b9c9a6a, 0x3fe19200cc8f9804]),
        (-4172144997902289642, 0, 0, [0x8bfa194563a8f2e6, 0x5ded6519db30a109, 0x0133331e8c6e75b2, 0x3fefcf1dfcc1e0e5, 0x3fd43c4352020db4, 0x3fef9d98e01e00a6, 0x3fc82c38ae5597f3]),
        (-4172144997902289642, -1, 0, [0xa6d462c883e1b85a, 0xea4ec455eb8764f4, 0x06037b55bb252193, 0x3fefffed9fb5435c, 0x3fdd168412869dac, 0x3fef9b89a3018163, 0x3fc75c44e82d3bd4]),
        (-4172144997902289642, 3, -5, [0x9c0e1f6aa8bc6325, 0xb6745edd68f89a69, 0xea5bfd643f4df4b0, 0x3fefe077749c032c, 0x3fe8c2ee5a718d7c, 0x3feffe7c47ef27a0, 0x3fe36f7c15bd5455]),
        (-4172144997902289642, 7, 7, [0x625fcbb3471847fa, 0x1d4d17f848620803, 0x7a6f2c451bdf45ad, 0x3fef14348077e4ff, 0x3fc590dc5fded887, 0x3fee272b89616484, 0x3f805451b6c600b3]),
        (-4172144997902289642, -9, 12, [0xc8727fb8ed51d025, 0x962d5e969105894d, 0x4357efb289b05be6, 0x3fee9b2c199d1697, 0x3fe364981c5a5245, 0x3feecea5c3aaa326, 0x3fe3bf38c95cf33d]),
        (-4172144997902289642, 20, -20, [0x9c0e1f6aa8bc6325, 0x6b173dd57b841225, 0x65810d927c5461d7, 0x3fefffc5557a61d0, 0x3fe0501aa1b22356, 0x3feff68079d6f263, 0x3fe6455f9e848ce0]),
    ];

    #[test]
    fn matches_java_reference() {
        for &(seed, cx, cz, want) in GOLDEN {
            let at = format!("seed {seed} chunk ({cx}, {cz})");
            let mut cm = WorldChunkManager::new(seed);
            let mut g = OverworldGenerator::new(seed);
            g.rand.set_seed(chunk_seed(cx, cz));
            let biomes = cm.load_block_generator_data(cx * 16, cz * 16, 16, 16);
            let codes: Vec<u8> = biomes.iter().map(|b| b.java_code()).collect();
            assert_eq!(fnv(&codes), want[0], "biomes, {at}");
            assert_eq!(cm.temperature[0].to_bits(), want[3], "temperature[0], {at}");
            assert_eq!(cm.humidity[0].to_bits(), want[4], "humidity[0], {at}");
            assert_eq!(cm.temperature[255].to_bits(), want[5], "temperature[255], {at}");
            assert_eq!(cm.humidity[255].to_bits(), want[6], "humidity[255], {at}");
            let mut blocks = vec![block::AIR; VOLUME];
            g.generate_terrain(cx, cz, &mut blocks, &cm.temperature, &cm.humidity);
            assert_eq!(fnv(&blocks), want[1], "terrain, {at}");
            g.replace_blocks_for_biome(cx, cz, &mut blocks, &biomes);
            assert_eq!(fnv(&blocks), want[2], "surface, {at}");
        }
    }

    #[test]
    fn generate_is_deterministic() {
        let mut cm = WorldChunkManager::new(42);
        let mut g = OverworldGenerator::new(42);
        let a = g.generate(2, -3, &mut cm);
        let b = g.generate(2, -3, &mut cm);
        assert_eq!(a, b);
    }
}
