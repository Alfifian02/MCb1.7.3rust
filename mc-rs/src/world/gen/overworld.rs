//! Overworld chunk generator: port of ChunkProviderGenerate (b1.7.3). `generate` is provideChunk
//! (terrain, biome surface, caves); populate() (trees, ores, ...) lives in `populate.rs`.
//!
//! Verified against the real Java classes: see `matches_java_reference`, `generate_matches_java`
//! and tools/golden/.

use crate::world::biome::Biome;
use crate::world::gen::caves;
use crate::world::gen::chunk_manager::WorldChunkManager;
use crate::world::gen::hydro::{Hydro, CELL};
use crate::world::gen::noise::{JavaRandom, OctaveNoise};
use std::collections::HashMap;
use std::sync::Arc;

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

/// The world type chosen on the create screen and saved in the world's `info`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WorldType {
    /// b1.7.3 terrain and biomes, on the 1.18+ placement.
    Vanilla,
    /// The `tectonic` crate's terrain.
    Tectonic,
    /// Vanilla, plus six biomes of the Blooming Biosphere datapack (`Biome::blooming`).
    Blooming,
}

impl WorldType {
    /// The word in the world's `info` file.
    pub fn name(self) -> &'static str {
        match self {
            WorldType::Vanilla => "vanilla",
            WorldType::Tectonic => "tectonic",
            WorldType::Blooming => "blooming",
        }
    }

    /// `name` back; anything else (an old or hand-edited file) is Vanilla.
    pub fn parse(s: &str) -> Self {
        [WorldType::Tectonic, WorldType::Blooming].into_iter().find(|t| t.name() == s.trim()).unwrap_or(WorldType::Vanilla)
    }

    /// The create screen's button: the next type, round and round.
    pub fn next(self) -> Self {
        match self {
            WorldType::Vanilla => WorldType::Tectonic,
            WorldType::Tectonic => WorldType::Blooming,
            WorldType::Blooming => WorldType::Vanilla,
        }
    }

    /// The generator of a played overworld of this type.
    pub fn generator(self, seed: i64) -> OverworldGenerator {
        match self {
            WorldType::Vanilla => OverworldGenerator::world(seed),
            WorldType::Tectonic => OverworldGenerator::world_tectonic(seed),
            WorldType::Blooming => OverworldGenerator::world_blooming(seed),
        }
    }
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
    /// Noise columns by absolute grid (x, z): a chunk's 5x5 grid shares its border columns with its
    /// neighbours, so a column sampled once serves up to 4 chunks (Noisium: cached generation shape).
    cols: HashMap<(i32, i32), Col>,
    max_cols: usize,
    /// The second world type (the `tectonic` crate): its density replaces the density pass; surface, caves and populate are unchanged.
    tectonic: Option<tectonic::Tectonic>,
    /// The Nether (`nether.rs`): when set, `generate` and `populate` are its own.
    pub(super) nether: Option<crate::world::gen::nether::Nether>,
    /// Rivers, streams and lakes (`gen::hydro`), carved after the surface pass: set by `world` / `world_tectonic`, so the goldens
    /// that build the generator with `new` stay b1.7.3 bit for bit.
    hydro: Option<Arc<Hydro>>,
    /// The Blooming world type: `manager` makes the chunk manager that places the Blooming Biosphere biomes.
    blooming: bool,
    /// Structory's structures (`structory.rs`), Blooming only; taken out while `populate` runs (it needs `&mut self` for the ground).
    pub(super) structory: Option<crate::world::gen::structory::Structory>,
}

/// The five noise samples of one density grid column (17 high), in the order `density` reads them.
struct Col {
    main: f64,
    height: f64,
    base: [f64; 17],
    lim: [f64; 17],
    low: [f64; 17],
}

/// ponytail: past this many columns (~0.4 KB each) the cache is dropped whole; upgrade is an LRU.
const MAX_COLS: usize = 4096;

/// The 17 densities of one grid column, from its noise samples, `h` (humidity x temperature there) and, when the terrain follows the
/// biome parameters, the wanted `(surface offset in blocks, hill steepness)`: `func_4061_a`'s inner loop.
fn column_density(c: &Col, h: f64, shape: Option<(f64, f64)>, out: &mut [f64]) {
    const SY: usize = 17;
    let mut v25 = 1.0 - h;
    v25 *= v25;
    v25 *= v25;
    v25 = 1.0 - v25;
    let mut v27 = (c.main + 256.0) / 512.0;
    v27 *= v25;
    if v27 > 1.0 { v27 = 1.0; }
    let mut v29 = c.height / 8000.0;
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
    // Shaped: the surface is where the parameters put it (8 blocks a cell, Beta's 68 is cell 8.5) and the hills are
    // Beta's `main` noise and climate roughness, scaled by how steep the parameters want the ground.
    let (v27, v31) = match shape {
        Some((off, steep)) => {
            (0.5 + ((c.main + 256.0) / 512.0 * v25 * steep).clamp(0.0, 1.0), SY as f64 / 2.0 + off / 8.0)
        }
        None => (v27, v31),
    };
    for iy in 0..SY {
        let mut v36 = (iy as f64 - v31) * 12.0 / v27;
        if v36 < 0.0 { v36 *= 4.0; }
        let lim = c.lim[iy] / 512.0;
        let low = c.low[iy] / 512.0;
        let mix = (c.base[iy] / 10.0 + 1.0) / 2.0;
        let mut v34 = if mix < 0.0 { lim } else if mix > 1.0 { low } else { lim + (low - lim) * mix };
        v34 -= v36;
        if iy > SY - 4 {
            // The Java divides in float here.
            let v44 = ((iy - (SY - 4)) as f32 / 3.0_f32) as f64;
            v34 = v34 * (1.0 - v44) + -10.0 * v44;
        }
        out[iy] = v34;
    }
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
        Self { seed, rand, noise_lim, noise_low, noise_base, noise_sand, noise_stone, noise_main, noise_height, noise_spawner, cols: HashMap::new(), max_cols: MAX_COLS, tectonic: None, nether: None, hydro: None, blooming: false, structory: None }
    }

    /// The Nether (`gen::nether`): `generate` and `populate` hand over to it, everything else here stays unused.
    pub fn new_nether(seed: i64) -> Self {
        let mut g = Self::new(seed);
        g.nether = Some(crate::world::gen::nether::Nether::new(seed));
        g
    }

    /// `new` on the Tectonic world type.
    pub fn new_tectonic(seed: i64) -> Self {
        Self::new(seed).tectonic()
    }

    /// This generator on the Tectonic world type: the mod's terrain, fitted to this 128-high world with the sea at `SEA_LEVEL`.
    pub fn tectonic(mut self) -> Self {
        self.tectonic = Some(tectonic::Tectonic::new(self.seed, tectonic::Config::compact(H as i32, SEA_LEVEL)));
        self
    }

    /// The generator of a played overworld: `new` plus rivers, streams and lakes.
    pub fn world(seed: i64) -> Self {
        Self::new(seed).with_hydro()
    }

    /// `world` on the Tectonic world type.
    pub fn world_tectonic(seed: i64) -> Self {
        Self::new_tectonic(seed).with_hydro()
    }

    /// `world` on the Blooming world type: Vanilla terrain, rivers and lakes, with the Blooming Biosphere biomes.
    pub fn world_blooming(seed: i64) -> Self {
        let mut g = Self::world(seed);
        g.blooming = true;
        g.structory = crate::world::gen::structory::Structory::new(seed);
        g
    }

    fn with_hydro(mut self) -> Self {
        self.hydro = Some(Hydro::of(self.seed, self.tectonic.is_some()));
        self
    }

    /// The hydrology of this generator's world, if it has one (the main thread reads currents from it).
    pub fn hydro(&self) -> Option<Arc<Hydro>> {
        self.hydro.clone()
    }

    /// (surface height, humidity 0..1) of grid node (gx, gz) at block (12 gx, 12 gz) (`hydro::CELL`), read without making the chunk: the vanilla
    /// density column (Beta's noise at this one grid column and the zero crossing of its 17 densities) or Tectonic's density.
    fn probe(&mut self, gx: i32, gz: i32, cm: &WorldChunkManager) -> (f64, f64) {
        let (bx, bz) = (gx * CELL, gz * CELL);
        let (t, h, off, steep) = cm.probe_at(bx, bz);
        if let Some(tc) = self.tectonic.as_ref() {
            return (f64::from(tc.surface(bx, bz)) + 0.5, h);
        }
        let (cx, cz) = (bx / 4, bz / 4);
        self.column(cx, cz);
        let mut d = [0.0; 17];
        column_density(&self.cols[&(cx, cz)], h * t, Some((off, steep)), &mut d);
        let y = match (0..17).rev().find(|&i| d[i] > 0.0) {
            None => 0.0,
            Some(16) => 128.0,
            Some(i) => 8.0 * i as f64 + 8.0 * d[i] / (d[i] - d[i + 1]),
        };
        (y, h)
    }

    /// The chunk manager for this generator's world: only the vanilla terrain follows its climate parameters (`shape`); Tectonic's
    /// comes from elsewhere. Every place that makes a generator makes its manager with this.
    pub fn manager(&self) -> WorldChunkManager {
        WorldChunkManager::new(self.seed).shaped(self.tectonic.is_none()).blooming(self.blooming)
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
        if let Some(n) = self.nether.as_mut() {
            return n.generate(chunk_x, chunk_z);
        }
        let mut blocks = self.generate_far(chunk_x, chunk_z, cm);
        caves::carve(self.seed, chunk_x, chunk_z, &mut blocks);
        blocks
    }

    /// `generate` without the caves: the land as seen from far away (`world::far`); they rarely reach the surface and cost the most.
    pub fn generate_far(&mut self, chunk_x: i32, chunk_z: i32, cm: &mut WorldChunkManager) -> Vec<u8> {
        self.rand.set_seed(chunk_seed(chunk_x, chunk_z));
        let biomes = cm.load_block_generator_data(chunk_x * 16, chunk_z * 16, 16, 16);
        let mut blocks = vec![block::AIR; VOLUME];
        if let Some(t) = self.tectonic.as_ref() {
            // Stone where Tectonic's final density is above 0; below the sea the rest is water, iced over where it is cold.
            let d = t.chunk(chunk_x, chunk_z);
            for x in 0..16usize {
                for z in 0..16usize {
                    let cold = cm.temperature[x * 16 + z] < 0.5;
                    let col = &mut blocks[(x << 11) | (z << 7)..][..H];
                    for (y, c) in col.iter_mut().enumerate() {
                        *c = if d[(x * 16 + z) * H + y] > 0.0 {
                            block::STONE
                        } else if (y as i32) < SEA_LEVEL {
                            if cold && y as i32 >= SEA_LEVEL - 1 { block::ICE } else { block::WATER }
                        } else {
                            block::AIR
                        };
                    }
                }
            }
        } else {
            let shape = cm.shape(chunk_x, chunk_z);
            self.generate_terrain(chunk_x, chunk_z, &mut blocks, &cm.temperature, &cm.humidity, shape.as_deref());
        }
        self.replace_blocks_for_biome(chunk_x, chunk_z, &mut blocks, &biomes);
        if let Some(h) = self.hydro.clone() {
            let region = h.region(chunk_x, chunk_z, &mut |gx, gz| self.probe(gx, gz, cm));
            region.carve(chunk_x, chunk_z, &mut blocks, &biomes, &cm.temperature);
        }
        blocks
    }

    /// The noise columns of absolute grid cell (gx, gz). One column is the same arithmetic as the
    /// Java's batch (`add` restarts its y cache at iy = 0), so every value is bit for bit the same.
    fn column(&mut self, gx: i32, gz: i32) {
        if self.cols.contains_key(&(gx, gz)) {
            return;
        }
        let (xf, zf) = (gx as f64, gz as f64);
        let mut c = Col { main: 0.0, height: 0.0, base: [0.0; 17], lim: [0.0; 17], low: [0.0; 17] };
        self.noise_main.generate_2d_into(std::slice::from_mut(&mut c.main), gx, gz, 1, 1, 1.121, 1.121);
        self.noise_height.generate_2d_into(std::slice::from_mut(&mut c.height), gx, gz, 1, 1, 200.0, 200.0);
        self.noise_base.generate_into(&mut c.base, xf, 0.0, zf, 1, 17, 1, 684.412 / 80.0, 684.412 / 160.0, 684.412 / 80.0);
        // C2ME lazy noise: the blend in `density` takes `lim` alone where `mix < 0` and `low` alone where `mix > 1` (about 88% of the
        // points need only one of the two), and `mix` comes from `base`, a property of this column alone, so the mask is the same
        // for every chunk that reads it and the cache stays valid. Same expression as the blend (a NaN needs both). The unread
        // samples stay 0.0 and are never read.
        let mix = |b: f64| (b / 10.0 + 1.0) / 2.0;
        let need_lim: [bool; 17] = std::array::from_fn(|i| !(mix(c.base[i]) > 1.0));
        let need_low: [bool; 17] = std::array::from_fn(|i| !(mix(c.base[i]) < 0.0));
        self.noise_lim.generate_masked_into(&mut c.lim, xf, 0.0, zf, 1, 17, 1, 684.412, 684.412, 684.412, &need_lim);
        self.noise_low.generate_masked_into(&mut c.low, xf, 0.0, zf, 1, 17, 1, 684.412, 684.412, 684.412, &need_low);
        self.cols.insert((gx, gz), c);
    }

    /// func_4061_a: the 5x17x5 density grid.
    /// `shape` (the 5 x 5 columns' wanted `(surface offset in blocks, hill steepness)`, from `WorldChunkManager::shape`) replaces
    /// Beta's own continent term (`height`, which made its valleys and lowlands) and hill term; `None` is b1.7.3 bit for bit.
    fn density(&mut self, x: i32, z: i32, temps: &[f64], hums: &[f64], shape: Option<&[(f64, f64)]>) -> Vec<f64> {
        const SX: usize = 5;
        const SY: usize = 17;
        const SZ: usize = 5;
        // Dropped before the fill, never inside it: a chunk's 25 columns must all survive until it is done.
        if self.cols.len() + SX * SZ > self.max_cols {
            self.cols.clear();
        }
        for ix in 0..SX as i32 {
            for iz in 0..SZ as i32 {
                self.column(x + ix, z + iz);
            }
        }
        let mut out = vec![0.0; SX * SY * SZ];
        let step = 16 / SX;
        let mut i = 0;
        for ix in 0..SX {
            let bx = ix * step + step / 2;
            for iz in 0..SZ {
                let bz = iz * step + step / 2;
                let t = temps[bx * 16 + bz];
                let h = hums[bx * 16 + bz] * t;
                let c = &self.cols[&(x + ix as i32, z + iz as i32)];
                column_density(c, h, shape.map(|sh| sh[ix * SZ + iz]), &mut out[i..i + SY]);
                i += SY;
            }
        }
        out
    }

    /// generateTerrain: interpolate the density grid into stone/water/ice.
    /// The incremental `+=` interpolation is kept as in Java so the sign of
    /// the density (stone vs not) matches bit for bit.
    fn generate_terrain(&mut self, chunk_x: i32, chunk_z: i32, blocks: &mut [u8], temps: &[f64], hums: &[f64], shape: Option<&[(f64, f64)]>) {
        let d = self.density(chunk_x * 4, chunk_z * 4, temps, hums, shape);
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
        let (mut sand_n, mut gravel_n, mut stone_n) = ([0.0; 256], [0.0; 256], [0.0; 256]);
        self.noise_sand.generate_into(&mut sand_n, bx, bz, 0.0, 16, 16, 1, s, s, 1.0);
        self.noise_sand.generate_into(&mut gravel_n, bx, 109.0134, bz, 16, 1, 16, s, 1.0, s);
        self.noise_stone.generate_into(&mut stone_n, bx, bz, 0.0, 16, 16, 1, s * 2.0, s * 2.0, s * 2.0);
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
                // One column as a slice: no per-block index arithmetic or bounds checks (FastSurface's packed column walk).
                let col = &mut blocks[(b * 16 + a) * 128..][..H];
                for y in (0..H).rev() {
                    // The draw is made for every y, as in the Java: it advances the Random.
                    if y as i32 <= self.rand.next_int_bound(5) {
                        col[y] = block::BEDROCK;
                        continue;
                    }
                    let cur = col[y];
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
                            col[y] = if y as i32 >= sea - 1 { top } else { filler };
                        } else if depth > 0 {
                            depth -= 1;
                            col[y] = filler;
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

    /// The generator against the real classes, on the b1.7.3 climate (`WorldChunkManager::legacy`): the biome source is another one now.
    #[test]
    fn matches_java_reference() {
        for &(seed, cx, cz, want) in GOLDEN {
            let at = format!("seed {seed} chunk ({cx}, {cz})");
            let mut cm = WorldChunkManager::legacy(seed);
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
            g.generate_terrain(cx, cz, &mut blocks, &cm.temperature, &cm.humidity, None);
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

    /// A generator that keeps its noise columns (neighbours share border columns) must give the
    /// same chunks as a fresh one for every chunk.
    #[test]
    fn cached_columns_match_a_fresh_generator() {
        let mut cm = WorldChunkManager::new(7);
        let mut shared = OverworldGenerator::new(7);
        shared.max_cols = 60; // two chunks' worth: the cache is dropped again and again
        for (cx, cz) in [(0, 0), (1, 0), (0, 1), (1, 1), (-1, 0), (0, -1), (1, 0), (2, 0), (3, 0), (3, 1), (2, 1), (0, 0)] {
            let fresh = OverworldGenerator::new(7).generate(cx, cz, &mut cm);
            assert_eq!(shared.generate(cx, cz, &mut cm), fresh, "chunk ({cx}, {cz})");
        }
    }

    /// The lazy column (the unread lim / low samples left out) holds, at every sample the blend reads, the plain full-column value
    /// bit for bit; `matches_java_reference` then covers terrain made through it against the real game.
    #[test]
    fn lazy_columns_match_the_plain_noise() {
        let mut g = OverworldGenerator::new(12345);
        let mut skipped = 0;
        for (gx, gz) in [(0, 0), (-1, 0), (3, -5), (7, 7), (-9, 12), (20, -20), (100, 100), (-300, 50)] {
            g.column(gx, gz);
            let c = &g.cols[&(gx, gz)];
            let (mut lim, mut low) = ([0.0; 17], [0.0; 17]);
            g.noise_lim.generate_into(&mut lim, gx as f64, 0.0, gz as f64, 1, 17, 1, 684.412, 684.412, 684.412);
            g.noise_low.generate_into(&mut low, gx as f64, 0.0, gz as f64, 1, 17, 1, 684.412, 684.412, 684.412);
            for i in 0..17 {
                let mix = (c.base[i] / 10.0 + 1.0) / 2.0;
                if !(mix > 1.0) {
                    assert_eq!(c.lim[i].to_bits(), lim[i].to_bits(), "lim ({gx}, {gz}) y {i}");
                } else {
                    skipped += 1;
                }
                if !(mix < 0.0) {
                    assert_eq!(c.low[i].to_bits(), low[i].to_bits(), "low ({gx}, {gz}) y {i}");
                } else {
                    skipped += 1;
                }
            }
        }
        assert!(skipped > 0, "no sample was left out: the mask did nothing");
    }

    /// The Tectonic world type is deterministic, has relief and sea (9 chunks 24 apart, seed 0xCAFEBABE: 54..77 in the Python
    /// reference `tools/tectonic.py`), fills whole chunks, and is another world than the vanilla one.
    #[test]
    fn tectonic_world_is_deterministic_hilly_and_wet() {
        const SEED: i64 = 0xCAFEBABE;
        let mut g = OverworldGenerator::new(SEED).tectonic();
        let mut cm = WorldChunkManager::new(SEED);
        let (mut lo, mut hi, mut water) = (i32::MAX, i32::MIN, false);
        let mut first = Vec::new();
        for cz in [-24, 0, 24] {
            for cx in [-24, 0, 24] {
                let b = g.generate(cx, cz, &mut cm);
                assert_eq!(b.len(), VOLUME);
                if (cx, cz) == (0, 0) {
                    first = b.clone();
                }
                water |= b.contains(&block::WATER);
                for x in 0..16 {
                    for z in 0..16 {
                        let t = OverworldGenerator::top_block(&b, x, z, 16);
                        lo = lo.min(t);
                        hi = hi.max(t);
                    }
                }
            }
        }
        assert!(hi - lo >= 12, "relief {lo}..{hi}");
        assert!(water, "no water in 9 chunks");
        let mut again = OverworldGenerator::new(SEED).tectonic();
        assert_eq!(again.generate(0, 0, &mut WorldChunkManager::new(SEED)), first);
        assert_ne!(OverworldGenerator::new(SEED).generate(0, 0, &mut WorldChunkManager::new(SEED)), first);
    }

    /// The terrain is where the biome parameters put it: sea floors under the sea biome, mountains over the sea and the plains.
    #[test]
    fn terrain_matches_the_biomes() {
        const SEED: i64 = 0xCAFEBABE;
        let mut cm = WorldChunkManager::new(SEED);
        let mut g = OverworldGenerator::new(SEED);
        // sum and count of the highest solid block: ocean, other land, mountains
        let (mut sum, mut n) = ([0i64; 3], [0i64; 3]);
        for cz in -5..=5 {
            for cx in -5..=5 {
                let (cx, cz) = (cx * 24, cz * 24);
                let biomes = cm.load_block_generator_data(cx * 16, cz * 16, 16, 16);
                let blocks = g.generate_far(cx, cz, &mut cm);
                for x in (0..16).step_by(4) {
                    for z in (0..16).step_by(4) {
                        let k = match biomes[x * 16 + z] {
                            Biome::Ocean => 0,
                            Biome::Mountains => 2,
                            _ => 1,
                        };
                        sum[k] += i64::from(OverworldGenerator::top_block(&blocks, x, z, 16));
                        n[k] += 1;
                    }
                }
            }
        }
        let mean = |k: usize| sum[k] as f64 / n[k].max(1) as f64;
        assert!(n[0] > 0 && n[1] > 0, "no sea or no land in 4000 x 4000 blocks: {n:?}");
        assert!(mean(0) < SEA_LEVEL as f64 - 6.0, "sea floor at {}", mean(0));
        assert!(mean(1) > SEA_LEVEL as f64, "land at {}", mean(1));
        if n[2] > 0 {
            assert!(mean(2) > mean(1) + 6.0, "mountains at {} over land at {}", mean(2), mean(1));
        }
    }
}
