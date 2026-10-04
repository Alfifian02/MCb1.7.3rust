//! Port ChunkProviderGenerate: provideChunk (terrain 5x17x5 + interpolasi, permukaan biome, gua).
//! Tahap `populate` (pohon, bijih, danau, dll.) ada di fase 5b.

use super::biome::Biome;
use super::caves::MapGenCaves;
use super::chunk_manager::WorldChunkManager;
use crate::chunk::{Chunk, VOLUME};
use crate::jrandom::JRandom;
use crate::noise::Octaves;
use crate::provider::ChunkSource;

const STONE: u8 = 1;
const BEDROCK: u8 = 7;
const WATER_STILL: u8 = 9;
const SAND: u8 = 12;
const GRAVEL: u8 = 13;
const SANDSTONE: u8 = 24;
const ICE: u8 = 79;

/// Tahap antara pembuatan chunk, untuk pengujian.
pub enum Stage<'a> {
    /// suhu dan kelembapan 16x16 (setelah dinormalkan)
    Climate(&'a [f64], &'a [f64]),
    /// blok setelah generateTerrain
    Terrain(&'a [u8]),
    /// blok setelah replaceBlocksForBiome (sebelum gua)
    Surface(&'a [u8]),
}

pub struct OverworldGenerator {
    pub seed: i64,
    pub rand: JRandom,
    pub manager: WorldChunkManager,
    caves: MapGenCaves,
    n_low: Octaves,      // field_912_k (16)
    n_high: Octaves,     // field_911_l (16)
    n_select: Octaves,   // field_910_m (8)
    n_sand: Octaves,     // field_909_n (4)
    n_stone: Octaves,    // field_908_o (4)
    n_scale: Octaves,    // field_922_a (10)
    n_depth: Octaves,    // field_921_b (16)
    pub n_spawner: Octaves, // mobSpawnerNoise (8)
    grid: Vec<f64>,
    // buffer kerja agar tidak ada alokasi per chunk
    b_select: Vec<f64>,
    b_low: Vec<f64>,
    b_high: Vec<f64>,
    b_scale: Vec<f64>,
    b_depth: Vec<f64>,
    b_sand: Vec<f64>,
    b_gravel: Vec<f64>,
    b_stone: Vec<f64>,
}

impl OverworldGenerator {
    pub fn new(seed: i64) -> Self {
        let mut rand = JRandom::new(seed);
        // Urutan konstruksi harus sama dengan Java (setiap Octaves menghabiskan RNG).
        let n_low = Octaves::new(&mut rand, 16);
        let n_high = Octaves::new(&mut rand, 16);
        let n_select = Octaves::new(&mut rand, 8);
        let n_sand = Octaves::new(&mut rand, 4);
        let n_stone = Octaves::new(&mut rand, 4);
        let n_scale = Octaves::new(&mut rand, 10);
        let n_depth = Octaves::new(&mut rand, 16);
        let n_spawner = Octaves::new(&mut rand, 8);
        Self {
            seed,
            rand,
            manager: WorldChunkManager::new(seed),
            caves: MapGenCaves::new(),
            n_low, n_high, n_select, n_sand, n_stone, n_scale, n_depth, n_spawner,
            grid: Vec::new(),
            b_select: Vec::new(), b_low: Vec::new(), b_high: Vec::new(),
            b_scale: Vec::new(), b_depth: Vec::new(),
            b_sand: Vec::new(), b_gravel: Vec::new(), b_stone: Vec::new(),
        }
    }

    /// Java: provideChunk. Skylight awal dihitung saat chunk dimasukkan ke ChunkMap.
    pub fn generate(&mut self, cx: i32, cz: i32) -> Chunk {
        self.generate_with(cx, cz, &mut |_| {})
    }

    /// Seperti `generate`, tetapi memanggil `hook` setelah tiap tahap (untuk uji golden bertahap).
    pub fn generate_with(&mut self, cx: i32, cz: i32, hook: &mut dyn FnMut(Stage)) -> Chunk {
        self.rand.set_seed((cx as i64).wrapping_mul(341873128712).wrapping_add((cz as i64).wrapping_mul(132897987541)));
        let mut blocks = vec![0u8; VOLUME];
        let biomes = self.manager.load_block_generator_data(cx * 16, cz * 16, 16, 16);
        hook(Stage::Climate(&self.manager.temperature, &self.manager.humidity));
        self.generate_terrain(cx, cz, &mut blocks);
        hook(Stage::Terrain(&blocks));
        self.replace_blocks_for_biome(cx, cz, &mut blocks, &biomes);
        hook(Stage::Surface(&blocks));
        self.caves.generate(self.seed, cx, cz, &mut blocks);
        Chunk::new(cx, cz, blocks)
    }

    /// Java: func_4061_a. Grid kasar 5 x 17 x 5 (x, y, z) sebagai indeks (x * 5 + z) * 17 + y.
    fn build_grid(&mut self, ox: i32, oy: i32, oz: i32, sx: usize, sy: usize, sz: usize) {
        let (c1, c2) = (684.412f64, 684.412f64);
        self.n_scale.generate_2d(&mut self.b_scale, ox, oz, sx, sz, 1.121, 1.121);
        self.n_depth.generate_2d(&mut self.b_depth, ox, oz, sx, sz, 200.0, 200.0);
        self.n_select.generate(&mut self.b_select, ox as f64, oy as f64, oz as f64, sx, sy, sz, c1 / 80.0, c2 / 160.0, c1 / 80.0);
        self.n_low.generate(&mut self.b_low, ox as f64, oy as f64, oz as f64, sx, sy, sz, c1, c2, c1);
        self.n_high.generate(&mut self.b_high, ox as f64, oy as f64, oz as f64, sx, sy, sz, c1, c2, c1);

        self.grid.clear();
        self.grid.resize(sx * sy * sz, 0.0);
        let temp = &self.manager.temperature;
        let humid = &self.manager.humidity;
        let mut idx = 0usize;
        let mut idx2 = 0usize;
        let step = (16 / sx) as i32;
        for i in 0..sx {
            let ti = (i as i32 * step + step / 2) as usize;
            for k in 0..sz {
                let tk = (k as i32 * step + step / 2) as usize;
                let t = temp[ti * 16 + tk];
                let hh = humid[ti * 16 + tk] * t;
                let mut inv = 1.0 - hh;
                inv *= inv;
                inv *= inv;
                inv = 1.0 - inv;
                let mut scale = (self.b_scale[idx2] + 256.0) / 512.0;
                scale *= inv;
                if scale > 1.0 {
                    scale = 1.0;
                }
                let mut depth = self.b_depth[idx2] / 8000.0;
                if depth < 0.0 {
                    depth = -depth * 0.3;
                }
                depth = depth * 3.0 - 2.0;
                if depth < 0.0 {
                    depth /= 2.0;
                    if depth < -1.0 {
                        depth = -1.0;
                    }
                    depth /= 1.4;
                    depth /= 2.0;
                    scale = 0.0;
                } else {
                    if depth > 1.0 {
                        depth = 1.0;
                    }
                    depth /= 8.0;
                }
                if scale < 0.0 {
                    scale = 0.0;
                }
                scale += 0.5;
                depth = depth * sy as f64 / 16.0;
                let base = sy as f64 / 2.0 + depth * 4.0;
                idx2 += 1;

                for y in 0..sy {
                    let mut dens;
                    let mut off = (y as f64 - base) * 12.0 / scale;
                    if off < 0.0 {
                        off *= 4.0;
                    }
                    let lo = self.b_low[idx] / 512.0;
                    let hi = self.b_high[idx] / 512.0;
                    let sel = (self.b_select[idx] / 10.0 + 1.0) / 2.0;
                    if sel < 0.0 {
                        dens = lo;
                    } else if sel > 1.0 {
                        dens = hi;
                    } else {
                        dens = lo + (hi - lo) * sel;
                    }
                    dens -= off;
                    if y > sy - 4 {
                        let f = ((y - (sy - 4)) as f32 / 3.0f32) as f64;
                        dens = dens * (1.0 - f) + -10.0 * f;
                    }
                    self.grid[idx] = dens;
                    idx += 1;
                }
            }
        }
    }

    /// Java: generateTerrain
    fn generate_terrain(&mut self, cx: i32, cz: i32, blocks: &mut [u8]) {
        let cells = 4usize;
        let sea = 64usize;
        let gx = cells + 1;
        let gy = 17usize;
        let gz = cells + 1;
        self.build_grid(cx * cells as i32, 0, cz * cells as i32, gx, gy, gz);
        let g = &self.grid;
        let temp = &self.manager.temperature;
        for i in 0..cells {
            for j in 0..cells {
                for k in 0..16usize {
                    let q = 0.125f64;
                    let mut d1 = g[((i) * gz + j) * gy + k];
                    let mut d2 = g[((i) * gz + j + 1) * gy + k];
                    let mut d3 = g[((i + 1) * gz + j) * gy + k];
                    let mut d4 = g[((i + 1) * gz + j + 1) * gy + k];
                    let s1 = (g[((i) * gz + j) * gy + k + 1] - d1) * q;
                    let s2 = (g[((i) * gz + j + 1) * gy + k + 1] - d2) * q;
                    let s3 = (g[((i + 1) * gz + j) * gy + k + 1] - d3) * q;
                    let s4 = (g[((i + 1) * gz + j + 1) * gy + k + 1] - d4) * q;
                    for l in 0..8usize {
                        let q2 = 0.25f64;
                        let mut e1 = d1;
                        let mut e2 = d2;
                        let t1 = (d3 - d1) * q2;
                        let t2 = (d4 - d2) * q2;
                        for m in 0..4usize {
                            let mut idx = ((m + i * 4) << 11) | ((j * 4) << 7) | (k * 8 + l);
                            let q3 = 0.25f64;
                            let mut v = e1;
                            let sv = (e2 - e1) * q3;
                            for n in 0..4usize {
                                let t = temp[(i * 4 + m) * 16 + j * 4 + n];
                                let mut id = 0u8;
                                let y = k * 8 + l;
                                if y < sea {
                                    if t < 0.5 && y >= sea - 1 {
                                        id = ICE;
                                    } else {
                                        id = WATER_STILL;
                                    }
                                }
                                if v > 0.0 {
                                    id = STONE;
                                }
                                blocks[idx] = id;
                                idx += 128;
                                v += sv;
                            }
                            e1 += t1;
                            e2 += t2;
                        }
                        d1 += s1;
                        d2 += s2;
                        d3 += s3;
                        d4 += s4;
                    }
                }
            }
        }
    }

    /// Java: replaceBlocksForBiome
    fn replace_blocks_for_biome(&mut self, cx: i32, cz: i32, blocks: &mut [u8], biomes: &[Biome]) {
        let sea = 64i32;
        let s = 1.0f64 / 32.0f64;
        self.n_sand.generate(&mut self.b_sand, (cx * 16) as f64, (cz * 16) as f64, 0.0, 16, 16, 1, s, s, 1.0);
        self.n_sand.generate(&mut self.b_gravel, (cx * 16) as f64, 109.0134, (cz * 16) as f64, 16, 1, 16, s, 1.0, s);
        self.n_stone.generate(&mut self.b_stone, (cx * 16) as f64, (cz * 16) as f64, 0.0, 16, 16, 1, s * 2.0, s * 2.0, s * 2.0);

        for a in 0..16usize {
            for b in 0..16usize {
                let biome = biomes[a + b * 16];
                let props = biome.props();
                let sand = self.b_sand[a + b * 16] + self.rand.next_double() * 0.2 > 0.0;
                let gravel = self.b_gravel[a + b * 16] + self.rand.next_double() * 0.2 > 3.0;
                let depth = (self.b_stone[a + b * 16] / 3.0 + 3.0 + self.rand.next_double() * 0.25) as i32;
                let mut run = -1i32;
                let mut top = props.top_block;
                let mut fill = props.filler_block;
                for y in (0..=127i32).rev() {
                    let idx = (b * 16 + a) * 128 + y as usize;
                    if y <= self.rand.next_int_bound(5) {
                        blocks[idx] = BEDROCK;
                    } else {
                        let cur = blocks[idx];
                        if cur == 0 {
                            run = -1;
                        } else if cur == STONE {
                            if run == -1 {
                                if depth <= 0 {
                                    top = 0;
                                    fill = STONE;
                                } else if y >= sea - 4 && y <= sea + 1 {
                                    top = props.top_block;
                                    fill = props.filler_block;
                                    if gravel {
                                        top = 0;
                                    }
                                    if gravel {
                                        fill = GRAVEL;
                                    }
                                    if sand {
                                        top = SAND;
                                    }
                                    if sand {
                                        fill = SAND;
                                    }
                                }
                                if y < sea && top == 0 {
                                    top = WATER_STILL;
                                }
                                run = depth;
                                if y >= sea - 1 {
                                    blocks[idx] = top;
                                } else {
                                    blocks[idx] = fill;
                                }
                            } else if run > 0 {
                                run -= 1;
                                blocks[idx] = fill;
                                if run == 0 && fill == SAND {
                                    run = self.rand.next_int_bound(4);
                                    fill = SANDSTONE;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Adaptor agar dapat dipasang sebagai `World::chunk_source`.
impl ChunkSource for OverworldGenerator {
    fn provide(&mut self, cx: i32, cz: i32) -> Chunk {
        self.generate(cx, cz)
    }
}
