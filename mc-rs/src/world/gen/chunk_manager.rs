//! `WorldChunkManager`: where each block column's biome and climate come from.
//!
//! Biomes are placed the way Minecraft 1.18+ does (the Overworld section of the wiki's "World generation"): five smooth noise
//! fields (temperature, humidity, continentalness, erosion, weirdness) read through a domain shift, then looked up in
//! `Biome::from_params`: sea, coast and mountain biomes from continentalness / erosion / weirdness, the rest from the temperature
//! x humidity table. Temperature runs cold to hot in bands, so a desert (the hot end) can only touch the savanna /
//! shrubland / rainforest band, never plains or forest: no more desert patches in the middle of plains. The b1.7.3 version (a 2D
//! temperature x humidity table) is kept for the golden tests only (`WorldChunkManager::legacy`).
//!
//! The terrain follows the same parameters (`shape`: a sea floor where continentalness is low, a coast, hills and mountains where
//! erosion is low), so the sea, beach and mountain biomes lie where the land is shaped for them; the rivers are `gen::hydro`'s. A world whose terrain comes from elsewhere (Tectonic) turns that off (`shaped(false)`) and gets the temperature x humidity
//! biomes only.
//!
//! The terrain, snow and tints keep reading `temperature` / `humidity` (0..1, x-major `x * 16 + z` for a 16 x 16 chunk); both now
//! come from the same noise as the biome, so ice, snow and grass colour agree with it.
//!
//! UNVERIFIED (written from the wiki and from memory of `OverworldBiomeBuilder` / `NormalNoise`, no vanilla jar at hand): the
//! octave lists, the table and the noise seeding are Minecraft's in shape, not checked against the game. `tools/biomes.py` is the
//! same arithmetic in Python (it gave the distribution quoted in the ROADMAP).

use crate::world::biome::Biome;
use crate::world::gen::noise::{JavaRandom, PerlinNoise};
#[cfg(test)]
use crate::world::gen::noise::SimplexOctaves;

/// Biome size: 1.0 is the game's (a climate band is thousands of blocks, a biome patch about 450 on average); 2.0 halves every
/// feature. One knob, since 128 high / 768 of LoD is a smaller world than the game's.
const SCALE: f64 = 1.0;

/// Minecraft's `NormalNoise` on this crate's Perlin octaves: two stacks, the second read at 1.0181268882175227 x the coordinates,
/// octave `i` at frequency `2^(first + i)` and weight `amp * 2^(n-1) / (2^n - 1) / 2^i`, scaled so the result spans about -1..1.
struct Normal {
    a: Vec<PerlinNoise>,
    b: Vec<PerlinNoise>,
    /// (octave, weight, frequency) of the octaves whose amplitude is not 0.
    oct: Vec<(usize, f64, f64)>,
    factor: f64,
}

impl Normal {
    fn new(seed: i64, first: i32, amps: &[f64]) -> Self {
        let mut r = JavaRandom::new(seed);
        let a: Vec<PerlinNoise> = amps.iter().map(|_| PerlinNoise::new(&mut r)).collect();
        let b: Vec<PerlinNoise> = amps.iter().map(|_| PerlinNoise::new(&mut r)).collect();
        let n = amps.len() as i32;
        let pers = 2f64.powi(n - 1) / (2f64.powi(n) - 1.0);
        let oct = amps
            .iter()
            .enumerate()
            .filter(|&(_, &m)| m != 0.0)
            .map(|(i, &m)| (i, m * pers / 2f64.powi(i as i32), 2f64.powi(first + i as i32)))
            .collect();
        let lo = amps.iter().position(|&m| m != 0.0).unwrap_or(0);
        let hi = amps.iter().rposition(|&m| m != 0.0).unwrap_or(0);
        let factor = (1.0 / 6.0) / (0.1 * (1.0 + 1.0 / (hi - lo + 1) as f64));
        Self { a, b, oct, factor }
    }

    fn get(&self, x: f64, z: f64) -> f64 {
        const K: f64 = 1.0181268882175227;
        let mut t = 0.0;
        for &(i, w, f) in &self.oct {
            t += w * (self.a[i].sample(x * f, 0.0, z * f) + self.b[i].sample(x * f * K, 0.0, z * f * K));
        }
        t * self.factor
    }
}

/// Piecewise-linear `v` through `pts` (sorted by x), flat past both ends.
fn through(v: f64, pts: &[(f64, f64)]) -> f64 {
    if v <= pts[0].0 {
        return pts[0].1;
    }
    for p in pts.windows(2) {
        if v <= p[1].0 {
            return p[0].1 + (p[1].1 - p[0].1) * (v - p[0].0) / (p[1].0 - p[0].0);
        }
    }
    pts[pts.len() - 1].1
}

/// Temperature parameter -> the 0..1 temperature the rest of the game reads. The snow line (0.5) sits on the edge of the cold band,
/// so ice, snow and the snowy biomes start together; the cool band is just above it, so only high ground snows there.
const TEMPERATURE: [(f64, f64); 6] = [(-1.0, 0.0), (-0.45, 0.5), (-0.15, 0.65), (0.2, 0.8), (0.55, 0.95), (1.0, 1.0)];
/// Humidity parameter -> the 0..1 humidity (same use: grass and foliage colour, terrain roughness).
const HUMIDITY: [(f64, f64); 6] = [(-1.0, 0.0), (-0.35, 0.25), (-0.1, 0.45), (0.1, 0.6), (0.3, 0.75), (1.0, 1.0)];

/// Surface height (blocks above or below b1.7.3's 68) by continentalness: sea floor, coast, inland rise. Sea level is 64.
const LAND: [(f64, f64); 7] = [(-1.2, -40.0), (-0.455, -30.0), (-0.19, -10.0), (-0.11, -3.0), (0.03, 0.0), (0.3, 4.0), (1.0, 10.0)];

/// What the terrain wants at a point, from continentalness `c`, erosion `e` and weirdness `w`: (offset of the surface in blocks,
/// steepness factor of the hills, 0.25 flat .. 1.6 mountains). Continentalness gives sea, coast and land; low erosion inland lifts
/// hills and, on a high ridge, mountains; high erosion keeps the land flat. Rivers are not here: `gen::hydro` routes the water over
/// whatever this makes. The numbers are eyeballed, in the shape of the game's terrain splines.
fn terrain(c: f64, e: f64, w: f64) -> (f64, f64) {
    let smooth = |x: f64| {
        let x = x.clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    };
    let inland = smooth((c + 0.11) / 0.41);
    let pv = 1.0 - (3.0 * w.abs() - 2.0).abs();
    let ero = ((0.05 - e) / 0.8).clamp(0.0, 1.0) * inland;
    let peak = ((pv - 0.2) / 0.5).clamp(0.0, 1.0);
    let off = through(c, &LAND) + inland * 4.0 * pv + 28.0 * ero * (0.5 + 0.5 * peak);
    (off, 0.25 + 1.35 * ero)
}

/// One quart: its biome, 0..1 climate and the terrain wanted there.
#[derive(Copy, Clone)]
struct Cell {
    biome: Biome,
    t: f64,
    h: f64,
    off: f64,
    steep: f64,
}

pub struct WorldChunkManager {
    /// The terrain follows the parameters (see `shaped`).
    shaped: bool,
    shift: Normal,
    temp: Normal,
    humid: Normal,
    cont: Normal,
    erosion: Normal,
    ridge: Normal,
    /// Climate of the last `load_block_generator_data` call, x-major
    /// (`x * 16 + z` for a 16x16 chunk). Terrain generation reads these.
    pub temperature: Vec<f64>,
    pub humidity: Vec<f64>,
    #[cfg(test)]
    legacy: Option<[SimplexOctaves; 3]>,
}

impl WorldChunkManager {
    pub fn new(seed: i64) -> Self {
        // One seed per field; the constant is 2^64 / golden ratio (as a signed number), the usual way to spread a seed.
        let mk = |salt: i64, first: i32, amps: &[f64]| Normal::new(seed.wrapping_mul(-7046029254386353131).wrapping_add(salt), first, amps);
        Self {
            shaped: true,
            shift: mk(6, -3, &[1.0, 1.0, 1.0, 0.0]),
            temp: mk(1, -10, &[1.5, 0.0, 1.0, 0.0, 0.0, 0.0]),
            humid: mk(2, -8, &[1.0, 1.0, 0.0, 0.0, 0.0, 0.0]),
            cont: mk(3, -9, &[1.0, 1.0, 2.0, 2.0, 2.0, 1.0, 1.0, 1.0, 1.0]),
            erosion: mk(4, -9, &[1.0, 1.0, 0.0, 1.0, 1.0]),
            ridge: mk(5, -7, &[1.0, 2.0, 1.0, 0.0, 0.0, 0.0]),
            temperature: Vec::new(),
            humidity: Vec::new(),
            #[cfg(test)]
            legacy: None,
        }
    }

    /// This manager for a world whose terrain does not follow the parameters (`false`: the Tectonic world type): inland, flat ground
    /// everywhere, so the biomes are the temperature x humidity table's and `shape` is `None`.
    pub fn shaped(mut self, on: bool) -> Self {
        self.shaped = on;
        self
    }

    /// Biome, 0..1 climate and wanted terrain of one quart (4 x 4 blocks), as the game samples them. The coordinates are in quarts;
    /// the shift bends the borders (about 16 blocks) so they do not follow the noise's grid.
    fn cell(&self, qx: i32, qz: i32) -> Cell {
        let (u, v) = (qx as f64 * SCALE, qz as f64 * SCALE);
        let (u, v) = (u + self.shift.get(u, v) * 4.0, v + self.shift.get(v, u) * 4.0);
        let (t, h) = (self.temp.get(u, v), self.humid.get(u, v));
        let (c, e, w) = if self.shaped {
            // ponytail: the spawn search only looks 32 blocks around the origin, so continentalness is lifted around it (land out to
            // about 300 blocks) instead of searching for land as the game does. Upgrade: search the climate for a land point.
            let d = (qx as f64).hypot(qz as f64) * 4.0;
            (self.cont.get(u, v) + 1.3 * (1.0 - d / 400.0).max(0.0), self.erosion.get(u, v), self.ridge.get(u, v))
        } else {
            (0.5, 0.0, 0.45)
        };
        let (off, steep) = terrain(c, e, w);
        Cell { biome: Biome::from_params(t, h, c, e, w), t: through(t, &TEMPERATURE), h: through(h, &HUMIDITY), off, steep }
    }

    /// (0..1 temperature, 0..1 humidity, surface offset, steepness) at a block: what `OverworldGenerator::probe` needs to read the
    /// terrain's height without making the chunk.
    pub fn probe_at(&self, x: i32, z: i32) -> (f64, f64, f64, f64) {
        let c = self.cell(x >> 2, z >> 2);
        (c.t, c.h, c.off, c.steep)
    }

    /// The terrain wanted at the 5 x 5 density columns of chunk (cx, cz), x outer, z inner: (offset of the surface in blocks,
    /// steepness factor of the hills). `None` when the terrain does not follow the parameters (Tectonic, the b1.7.3 manager).
    pub fn shape(&self, cx: i32, cz: i32) -> Option<Vec<(f64, f64)>> {
        #[cfg(test)]
        {
            if self.legacy.is_some() {
                return None;
            }
        }
        self.shaped.then(|| {
            (0..5).flat_map(|i| (0..5).map(move |j| (i, j))).map(|(i, j)| self.cell(cx * 4 + i, cz * 4 + j)).map(|c| (c.off, c.steep)).collect()
        })
    }

    /// loadBlockGeneratorData for a `w` x `d` block area at (x, z): the biomes, x outer and z inner (`a * d + b`), and the climate
    /// in `temperature` / `humidity` in the same order.
    pub fn load_block_generator_data(&mut self, x: i32, z: i32, w: usize, d: usize) -> Vec<Biome> {
        #[cfg(test)]
        {
            if self.legacy.is_some() {
                return self.load_legacy(x, z, w, d);
            }
        }
        // One sample per quart cell the area touches, then every column reads its cell.
        let (qx0, qz0) = (x >> 2, z >> 2);
        let nx = (((x + w as i32 - 1) >> 2) - qx0 + 1) as usize;
        let nz = (((z + d as i32 - 1) >> 2) - qz0 + 1) as usize;
        let cells: Vec<Cell> =
            (0..nx).flat_map(|i| (0..nz).map(move |j| (i, j))).map(|(i, j)| self.cell(qx0 + i as i32, qz0 + j as i32)).collect();
        let mut biomes = Vec::with_capacity(w * d);
        self.temperature.clear();
        self.humidity.clear();
        for a in 0..w as i32 {
            for b in 0..d as i32 {
                let (i, j) = ((((x + a) >> 2) - qx0) as usize, (((z + b) >> 2) - qz0) as usize);
                let c = cells[i * nz + j];
                biomes.push(c.biome);
                self.temperature.push(c.t);
                self.humidity.push(c.h);
            }
        }
        biomes
    }

    /// The b1.7.3 manager (temperature / humidity simplex noise, `Biome::from_climate`), kept so the Java golden tests of the
    /// terrain, the surface and `populate` still prove the generator itself.
    #[cfg(test)]
    pub fn legacy(seed: i64) -> Self {
        let mut m = Self::new(seed);
        let mut r = JavaRandom::new(seed.wrapping_mul(9871));
        let temp = SimplexOctaves::new(&mut r, 4);
        let mut r = JavaRandom::new(seed.wrapping_mul(39811));
        let humid = SimplexOctaves::new(&mut r, 4);
        let mut r = JavaRandom::new(seed.wrapping_mul(543321));
        let extra = SimplexOctaves::new(&mut r, 2);
        m.legacy = Some([temp, humid, extra]);
        m
    }

    /// loadBlockGeneratorData as in b1.7.3. Like the Java, the noise is sampled `w` x `w`, so only w == d is meaningful.
    #[cfg(test)]
    fn load_legacy(&mut self, x: i32, z: i32, w: usize, d: usize) -> Vec<Biome> {
        let [temp_noise, humid_noise, extra_noise] = self.legacy.as_ref().expect("legacy manager");
        let (xf, zf) = (x as f64, z as f64);
        // The Java passes `(double)0.025F` / `(double)0.05F`: float literals widened to
        // double, which are NOT the same numbers as the double literals 0.025 / 0.05.
        let (ts, hs) = (0.025_f32 as f64, 0.05_f32 as f64);
        let mut temp = temp_noise.generate(xf, zf, w, w, ts, ts, 0.25);
        let mut humid = humid_noise.generate(xf, zf, w, w, hs, hs, 1.0 / 3.0);
        let extra = extra_noise.generate(xf, zf, w, w, 0.25, 0.25, 0.5882352941176471);
        let mut biomes = Vec::with_capacity(w * d);
        let mut i = 0;
        for _ in 0..w {
            for _ in 0..d {
                let e = extra[i] * 1.1 + 0.5;
                let mut t = (temp[i] * 0.15 + 0.7) * (1.0 - 0.01) + e * 0.01;
                let mut h = (humid[i] * 0.15 + 0.5) * (1.0 - 0.002) + e * 0.002;
                t = 1.0 - (1.0 - t) * (1.0 - t);
                t = t.clamp(0.0, 1.0);
                h = h.clamp(0.0, 1.0);
                temp[i] = t;
                humid[i] = h;
                biomes.push(Biome::from_climate(t, h));
                i += 1;
            }
        }
        self.temperature = temp;
        self.humidity = humid;
        biomes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The biome of 128 x 128 columns 32 blocks apart (4096 blocks a side).
    fn scan(seed: i64) -> Vec<Vec<Biome>> {
        let mut cm = WorldChunkManager::new(seed);
        (0..128).map(|i| (0..128).map(|j| cm.load_block_generator_data((i - 64) * 32, (j - 64) * 32, 1, 1)[0]).collect()).collect()
    }

    /// The point of the rewrite: a desert never touches plains, forest, taiga, tundra, seasonal forest or swamp: only the warm
    /// transition band (savanna, shrubland, rainforest), water, sand and rock.
    #[test]
    fn desert_borders_only_the_warm_band() {
        let g = scan(0xCAFEBABE);
        let cool = [Biome::Plains, Biome::Forest, Biome::Taiga, Biome::Tundra, Biome::SeasonalForest, Biome::Swamp];
        let (mut deserts, mut kinds) = (0, std::collections::HashSet::new());
        for i in 0..128 {
            for j in 0..128 {
                kinds.insert(g[i][j]);
                if g[i][j] != Biome::Desert {
                    continue;
                }
                deserts += 1;
                for (di, dj) in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
                    let (a, b) = (i as i32 + di, j as i32 + dj);
                    if (0..128).contains(&a) && (0..128).contains(&b) {
                        let n = g[a as usize][b as usize];
                        assert!(!cool.contains(&n), "desert at ({i}, {j}) next to {n:?}");
                    }
                }
            }
        }
        assert!(deserts > 0, "no desert in 4096 x 4096 blocks");
        assert!(kinds.len() >= 9, "only {} biomes in 4096 x 4096 blocks", kinds.len());
    }

    /// Deterministic, climate in 0..1, and a 16 x 16 load is the 1 x 1 loads of its columns (x-major).
    #[test]
    fn chunk_load_matches_column_loads() {
        let (mut a, mut b) = (WorldChunkManager::new(7), WorldChunkManager::new(7));
        for (cx, cz) in [(0, 0), (-3, 5), (40, -17)] {
            let (bx, bz) = (cx * 16, cz * 16);
            let biomes = a.load_block_generator_data(bx, bz, 16, 16);
            assert_eq!((biomes.len(), a.temperature.len(), a.humidity.len()), (256, 256, 256));
            for k in 0..256 {
                let (x, z) = (bx + (k / 16) as i32, bz + (k % 16) as i32);
                assert_eq!(b.load_block_generator_data(x, z, 1, 1)[0], biomes[k], "biome ({x}, {z})");
                assert_eq!((b.temperature[0], b.humidity[0]), (a.temperature[k], a.humidity[k]), "climate ({x}, {z})");
                assert!((0.0..=1.0).contains(&a.temperature[k]) && (0.0..=1.0).contains(&a.humidity[k]));
            }
        }
    }

    /// Climate and biome agree: tundra lies under the snow line (temperature < 0.5), desert at the hot end.
    #[test]
    fn snow_line_follows_the_cold_band() {
        let mut cm = WorldChunkManager::new(1234);
        for i in -40..40 {
            for j in -40..40 {
                let biome = cm.load_block_generator_data(i * 64, j * 64, 1, 1)[0];
                let t = cm.temperature[0];
                assert!(biome != Biome::Tundra || t < 0.5, "tundra at t = {t}");
                assert!(biome != Biome::Desert || t > 0.9, "desert at t = {t}");
            }
        }
    }

    /// The wanted terrain: sea floor, coast, flat land, mountains.
    #[test]
    fn terrain_follows_the_parameters() {
        let (sea, _) = terrain(-0.5, 0.5, 0.45);
        let (coast, _) = terrain(-0.15, 0.5, 0.45);
        let (plain, flat) = terrain(0.5, 0.8, 0.45);
        let (mount, steep) = terrain(0.5, -0.7, 0.45);
        assert!(sea < -25.0, "sea {sea}");
        assert!((-10.0..-3.0).contains(&coast), "coast {coast}");
        assert!((0.0..12.0).contains(&plain), "plain {plain}");
        assert!(mount > plain + 15.0 && steep > 3.0 * flat, "mountain {mount} / {steep} vs {plain} / {flat}");
        // Beta's 68 plus the offset is the surface: the sea floor is under 64, the coast is not far over it.
        assert!(68.0 + sea < 64.0 - 20.0 && 68.0 + coast < 66.0);
    }

    /// `shaped(false)` (Tectonic): no wanted terrain, and none of the biomes that need it.
    #[test]
    fn unshaped_manager_keeps_to_the_middle_table() {
        let mut cm = WorldChunkManager::new(99).shaped(false);
        assert!(cm.shape(0, 0).is_none());
        let gone = [Biome::Ocean, Biome::Beach, Biome::Mountains, Biome::Swamp];
        for i in -30..30 {
            for j in -30..30 {
                let b = cm.load_block_generator_data(i * 128, j * 128, 1, 1)[0];
                assert!(!gone.contains(&b), "{b:?} in an unshaped world");
            }
        }
    }

    /// The spawn search looks 32 blocks around the origin: there is land there whatever the seed.
    #[test]
    fn origin_is_land() {
        for seed in 0..40 {
            let mut cm = WorldChunkManager::new(seed);
            for (x, z) in [(8, 8), (24, 8), (8, 24), (24, 24)] {
                let b = cm.load_block_generator_data(x, z, 1, 1)[0];
                assert!(b != Biome::Ocean && b != Biome::Beach, "seed {seed}: {b:?} at ({x}, {z})");
            }
        }
    }
}
