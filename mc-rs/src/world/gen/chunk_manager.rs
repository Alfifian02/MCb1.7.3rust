//! Port of WorldChunkManager: temperature/humidity noise -> biomes.

use crate::world::biome::Biome;
use crate::world::gen::noise::{JavaRandom, SimplexOctaves};

pub struct WorldChunkManager {
    temp_noise: SimplexOctaves,
    humid_noise: SimplexOctaves,
    extra_noise: SimplexOctaves,
    /// Climate of the last `load_block_generator_data` call, x-major
    /// (`x * 16 + z` for a 16x16 chunk). Terrain generation reads these.
    pub temperature: Vec<f64>,
    pub humidity: Vec<f64>,
}

impl WorldChunkManager {
    pub fn new(seed: i64) -> Self {
        let mut r = JavaRandom::new(seed.wrapping_mul(9871));
        let temp_noise = SimplexOctaves::new(&mut r, 4);
        let mut r = JavaRandom::new(seed.wrapping_mul(39811));
        let humid_noise = SimplexOctaves::new(&mut r, 4);
        let mut r = JavaRandom::new(seed.wrapping_mul(543321));
        let extra_noise = SimplexOctaves::new(&mut r, 2);
        Self { temp_noise, humid_noise, extra_noise, temperature: Vec::new(), humidity: Vec::new() }
    }

    /// loadBlockGeneratorData for a `w` x `d` block area at (x, z). Like the
    /// Java, the noise is sampled `w` x `w`, so only w == d is meaningful.
    pub fn load_block_generator_data(&mut self, x: i32, z: i32, w: usize, d: usize) -> Vec<Biome> {
        let (xf, zf) = (x as f64, z as f64);
        let mut temp = self.temp_noise.generate(xf, zf, w, w, 0.025, 0.025, 0.25);
        let mut humid = self.humid_noise.generate(xf, zf, w, w, 0.05, 0.05, 1.0 / 3.0);
        let extra = self.extra_noise.generate(xf, zf, w, w, 0.25, 0.25, 0.5882352941176471);
        let mut biomes = Vec::with_capacity(w * d);
        let mut i = 0;
        for _ in 0..w {
            for _ in 0..d {
                let e = extra[i] * 1.1 + 0.5;
                let mut t = (temp[i] * 0.15 + 0.7) * (1.0 - 0.01) + e * 0.01;
                let mut h = (humid[i] * 0.15 + 0.5) * (1.0 - 0.002) + e * 0.002;
                t = 1.0 - (1.0 - t) * (1.0 - t);
                if t < 0.0 { t = 0.0; }
                if h < 0.0 { h = 0.0; }
                if t > 1.0 { t = 1.0; }
                if h > 1.0 { h = 1.0; }
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
