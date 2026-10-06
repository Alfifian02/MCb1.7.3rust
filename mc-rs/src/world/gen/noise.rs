//! Beta-1.7 noise primitives. Direct port of net.minecraft.src.NoiseGeneratorPerlin
//! and NoiseGeneratorOctaves from the b1.7.3 decomp. Vanilla equivalent:
//! net.minecraft.world.level.levelgen.synth.PerlinNoise / PerlinSimplexNoise.

use std::cell::Cell;
use crate::world::biome::Biome;

pub struct PerlinNoise {
    permutations: [u8; 512],
    x_coord: f64,
    y_coord: f64,
    z_coord: f64,
}

impl PerlinNoise {
    pub fn from_seed(seed: u64) -> Self {
        let rng = JavaRandom::new(seed);
        let mut p = [0u8; 256];
        for i in 0..256 { p[i] = i as u8; }
        for i in 0..256 {
            let j = (rng.next_u31() as usize) % (256 - i) + i;
            p.swap(i, j);
        }
        let mut permutations = [0u8; 512];
        permutations[..256].copy_from_slice(&p);
        permutations[256..].copy_from_slice(&p);
        Self {
            permutations,
            x_coord: rng.next_double() * 256.0,
            y_coord: rng.next_double() * 256.0,
            z_coord: rng.next_double() * 256.0,
        }
    }

    /// Single Perlin value at (x, y, z).
    pub fn generate_noise(&self, x: f64, y: f64, z: f64) -> f64 {
        let xa = x + self.x_coord;
        let ya = y + self.y_coord;
        let za = z + self.z_coord;
        let xi = xa.floor() as i32;
        let yi = ya.floor() as i32;
        let zi = za.floor() as i32;
        let xb = (xi & 255) as usize;
        let yb = (yi & 255) as usize;
        let zb = (zi & 255) as usize;
        let xf = xa - xi as f64;
        let yf = ya - yi as f64;
        let zf = za - zi as f64;
        let u = fade(xf);
        let v = fade(yf);
        let w = fade(zf);
        let p = &self.permutations;
        let aa = p[xb] as usize + yb;
        let ab = p[aa] as usize + zb;
        let ac = p[aa + 1] as usize + zb;
        let ba = p[xb + 1] as usize + yb;
        let bb = p[ba] as usize + zb;
        let bc = p[ba + 1] as usize + zb;
        let x1 = lerp(u, grad(p[ab] as i32, xf, yf, zf),     grad(p[bb] as i32, xf - 1.0, yf, zf));
        let x2 = lerp(u, grad(p[ac] as i32, xf, yf - 1.0, zf), grad(p[bc] as i32, xf - 1.0, yf - 1.0, zf));
        let y1 = lerp(v, x1, x2);
        let x3 = lerp(u, grad(p[ab + 1] as i32, xf, yf, zf - 1.0), grad(p[bb + 1] as i32, xf - 1.0, yf, zf - 1.0));
        let x4 = lerp(u, grad(p[ac + 1] as i32, xf, yf - 1.0, zf - 1.0), grad(p[bc + 1] as i32, xf - 1.0, yf - 1.0, zf - 1.0));
        let y2 = lerp(v, x3, x4);
        lerp(w, y1, y2)
    }
}

/// Octave stack: `octaves` independent Perlin noises, summed with halving amplitude.
/// Direct port of net.minecraft.src.NoiseGeneratorOctaves.
pub struct OctaveNoise {
    octaves: Vec<PerlinNoise>,
}

impl OctaveNoise {
    pub fn new(seed: u64, octaves: usize) -> Self {
        let rng = JavaRandom::new(seed);
        let mut octs = Vec::with_capacity(octaves);
        for _ in 0..octaves {
            octs.push(PerlinNoise::from_seed(rng.next_u63()));
        }
        Self { octaves: octs }
    }

    /// 2D noise: sum of 1/x-weighted octaves. Vanilla `func_806_a` in decomp.
    pub fn sample2d(&self, x: f64, y: f64) -> f64 {
        let mut total = 0.0;
        let mut scale = 1.0;
        for o in &self.octaves {
            total += o.generate_noise(x * scale, y * scale, 0.0) / scale;
            scale /= 2.0;
        }
        total
    }

    /// 2D batch: sample w*h points at (x0+ix*sx, y0+iy*sy) with octave halving.
    /// Vanilla `generateNoiseOctaves` with ny=1, dy=1.0.
    pub fn sample_array2d(&self, x0: i32, y0: i32, w: usize, h: usize, sx: f64, sy: f64) -> Vec<f64> {
        let mut out = vec![0.0; w * h];
        let mut scale = 1.0;
        for o in &self.octaves {
            for iy in 0..h {
                let yy = (y0 as f64 + iy as f64) * sy * scale;
                for ix in 0..w {
                    let xx = (x0 as f64 + ix as f64) * sx * scale;
                    out[iy * w + ix] += o.generate_noise(xx, yy, 0.0) / scale;
                }
            }
            scale /= 2.0;
        }
        out
    }

    /// 2D batch at constant y, with the y-scale unused.
    pub fn sample_array2d_y(&self, x0: i32, z0: i32, w: usize, h: usize, sx: f64, sz: f64) -> Vec<f64> {
        self.sample_array2d(x0, z0, w, h, sx, sz)
    }

    /// 3D batch. Vanilla `generateNoiseOctaves` with full 3D.
    pub fn sample_array3d(&self, x0: i32, y0: i32, z0: i32, w: usize, h: usize, d: usize, sx: f64, sy: f64, sz: f64) -> Vec<f64> {
        let mut out = vec![0.0; w * h * d];
        let mut scale = 1.0;
        for o in &self.octaves {
            for iz in 0..d {
                let zz = (z0 as f64 + iz as f64) * sz * scale;
                for iy in 0..h {
                    let yy = (y0 as f64 + iy as f64) * sy * scale;
                    for ix in 0..w {
                        let xx = (x0 as f64 + ix as f64) * sx * scale;
                        out[(iz * h + iy) * w + ix] += o.generate_noise(xx, yy, zz) / scale;
                    }
                }
            }
            scale /= 2.0;
        }
        out
    }
}

#[inline] fn fade(t: f64) -> f64 { t * t * t * (t * (t * 6.0 - 15.0) + 10.0) }
#[inline] fn lerp(t: f64, a: f64, b: f64) -> f64 { a + t * (b - a) }

#[inline]
fn grad(hash: i32, x: f64, y: f64, z: f64) -> f64 {
    let h = hash & 15;
    let u = if h < 8 { x } else { y };
    let v = if h < 4 { y } else if h == 12 || h == 14 { x } else { z };
    let a = if (h & 1) == 0 { u } else { -u };
    let b = if (h & 2) == 0 { v } else { -v };
    a + b
}

/// Minimal java.util.Random(seed) clone. next(bits) and next_double() only.
pub struct JavaRandom {
    seed: Cell<u64>,
}

impl JavaRandom {
    pub fn new(seed: u64) -> Self {
        Self { seed: Cell::new((seed ^ 0x5DEECE66Du64) & ((1u64 << 48) - 1)) }
    }
    #[inline]
    pub fn next(&self, bits: u32) -> u32 {
        let s = self.seed.get();
        let next = s.wrapping_mul(0x5DEECE66Du64).wrapping_add(0xBu64) & ((1u64 << 48) - 1);
        self.seed.set(next);
        (next >> (48 - bits)) as u32
    }
    pub fn next_u31(&self) -> u32 { self.next(31) }
    pub fn next_long(&self) -> i64 {
        ((self.next(32) as i64) << 32) + (self.next(32) as i64)
    }
    pub fn next_u63(&self) -> u64 {
        ((self.next(31) as u64) << 32) | (self.next(31) as u64) | 1
    }
    pub fn next_double(&self) -> f64 {
        let hi = self.next(26) as i64;
        let lo = self.next(26) as i64;
        ((hi << 26) | lo) as f64 / ((1i64 << 53) as f64)
    }
}

// Stub type so the world module can re-export it later; biome use is reserved for
// worldgen temperatures, which M3d will add. Avoid dead-code here.
#[allow(dead_code)]
fn _biome_keepalive(_b: Biome) {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn perlin_in_range() {
        let n = PerlinNoise::from_seed(12345);
        for i in 0..100 {
            let v = n.generate_noise(i as f64 * 0.13, 64.0, i as f64 * 0.27);
            assert!(v > -1.5 && v < 1.5, "perlin out of range: {v}");
        }
    }
    #[test]
    fn octave_deterministic() {
        let a = OctaveNoise::new(12345, 4);
        let b = OctaveNoise::new(12345, 4);
        assert_eq!(a.sample2d(0.0, 0.0), b.sample2d(0.0, 0.0));
    }
}
