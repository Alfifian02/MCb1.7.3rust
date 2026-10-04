//! Port WorldChunkManager: suhu, kelembapan, dan biome dari tiga noise simplex.

use super::biome::Biome;
use super::simplex::SimplexOctaves;
use crate::jrandom::JRandom;

pub struct WorldChunkManager {
    temp_noise: SimplexOctaves,
    humid_noise: SimplexOctaves,
    detail_noise: SimplexOctaves,
    /// Hasil `load_block_generator_data` terakhir (dipakai ChunkProviderGenerate).
    pub temperature: Vec<f64>,
    pub humidity: Vec<f64>,
    detail: Vec<f64>,
}

impl WorldChunkManager {
    pub fn new(seed: i64) -> Self {
        Self {
            temp_noise: SimplexOctaves::new(&mut JRandom::new(seed.wrapping_mul(9871)), 4),
            humid_noise: SimplexOctaves::new(&mut JRandom::new(seed.wrapping_mul(39811)), 4),
            detail_noise: SimplexOctaves::new(&mut JRandom::new(seed.wrapping_mul(543321)), 2),
            temperature: Vec::new(),
            humidity: Vec::new(),
            detail: Vec::new(),
        }
    }

    /// Java: loadBlockGeneratorData. Perhatikan keanehan asli: noise dihitung dengan ukuran (w, w).
    pub fn load_block_generator_data(&mut self, x: i32, z: i32, w: usize, h: usize) -> Vec<Biome> {
        let (xd, zd) = (x as f64, z as f64);
        self.temp_noise.generate(&mut self.temperature, xd, zd, w, w, 0.025f32 as f64, 0.025f32 as f64, 0.25);
        self.humid_noise.generate(&mut self.humidity, xd, zd, w, w, 0.05f32 as f64, 0.05f32 as f64, 1.0 / 3.0);
        self.detail_noise.generate(&mut self.detail, xd, zd, w, w, 0.25, 0.25, 0.5882352941176471);
        let mut out = Vec::with_capacity(w * h);
        let mut i = 0usize;
        for _ in 0..w {
            for _ in 0..h {
                let d = self.detail[i] * 1.1 + 0.5;
                let mut k = 0.01;
                let mut m = 1.0 - k;
                let mut t = (self.temperature[i] * 0.15 + 0.7) * m + d * k;
                k = 0.002;
                m = 1.0 - k;
                let mut hu = (self.humidity[i] * 0.15 + 0.5) * m + d * k;
                t = 1.0 - (1.0 - t) * (1.0 - t);
                if t < 0.0 {
                    t = 0.0;
                }
                if hu < 0.0 {
                    hu = 0.0;
                }
                if t > 1.0 {
                    t = 1.0;
                }
                if hu > 1.0 {
                    hu = 1.0;
                }
                self.temperature[i] = t;
                self.humidity[i] = hu;
                out.push(Biome::from_lookup(t, hu));
                i += 1;
            }
        }
        out
    }

    pub fn biome_at(&mut self, x: i32, z: i32) -> Biome {
        self.load_block_generator_data(x, z, 1, 1)[0]
    }
}
