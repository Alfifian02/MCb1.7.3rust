//! Port NoiseGeneratorPerlin + NoiseGeneratorOctaves (ChunkProviderGenerate, biome, dll).
//! Urutan konsumsi JRandom di konstruktor harus sama persis dengan Java.

use crate::jrandom::JRandom;

#[inline(always)]
fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

#[inline(always)]
fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

#[inline(always)]
fn grad(h: i32, x: f64, y: f64, z: f64) -> f64 {
    let h = h & 15;
    let u = if h < 8 { x } else { y };
    let v = if h < 4 { y } else if h != 12 && h != 14 { z } else { x };
    (if h & 1 == 0 { u } else { -u }) + (if h & 2 == 0 { v } else { -v })
}

/// grad 2D (func_4110_a di decomp), dipakai cabang y == 1 lapis.
#[inline(always)]
fn grad2(h: i32, x: f64, z: f64) -> f64 {
    let h = h & 15;
    let a = (1 - ((h & 8) >> 3)) as f64 * x;
    let b = if h < 4 { 0.0 } else if h != 12 && h != 14 { z } else { x };
    (if h & 1 == 0 { a } else { -a }) + (if h & 2 == 0 { b } else { -b })
}

#[inline(always)]
fn floor_i(v: f64) -> i32 {
    // Java: (int)v lalu kurangi 1 bila v < (double)hasil
    let mut i = v as i32;
    if v < i as f64 {
        i -= 1;
    }
    i
}

pub struct Perlin {
    perm: [i32; 512],
    x_off: f64,
    y_off: f64,
    z_off: f64,
}

impl Perlin {
    pub fn new(rng: &mut JRandom) -> Self {
        let x_off = rng.next_double() * 256.0;
        let y_off = rng.next_double() * 256.0;
        let z_off = rng.next_double() * 256.0;
        let mut perm = [0i32; 512];
        for i in 0..256 {
            perm[i] = i as i32;
        }
        for i in 0..256 {
            let j = (rng.next_int_bound((256 - i) as i32) as usize) + i;
            perm.swap(i, j);
            perm[i + 256] = perm[i];
        }
        Self { perm, x_off, y_off, z_off }
    }

    pub fn noise3(&self, x: f64, y: f64, z: f64) -> f64 {
        let mut x = x + self.x_off;
        let mut y = y + self.y_off;
        let mut z = z + self.z_off;
        let xi = floor_i(x);
        let yi = floor_i(y);
        let zi = floor_i(z);
        let (xa, ya, za) = ((xi & 255) as usize, (yi & 255) as usize, (zi & 255) as usize);
        x -= xi as f64;
        y -= yi as f64;
        z -= zi as f64;
        let (u, v, w) = (fade(x), fade(y), fade(z));
        let p = &self.perm;
        let a = p[xa] as usize + ya;
        let aa = p[a] as usize + za;
        let ab = p[a + 1] as usize + za;
        let b = p[xa + 1] as usize + ya;
        let ba = p[b] as usize + za;
        let bb = p[b + 1] as usize + za;
        lerp(
            w,
            lerp(
                v,
                lerp(u, grad(p[aa], x, y, z), grad(p[ba], x - 1.0, y, z)),
                lerp(u, grad(p[ab], x, y - 1.0, z), grad(p[bb], x - 1.0, y - 1.0, z)),
            ),
            lerp(
                v,
                lerp(u, grad(p[aa + 1], x, y, z - 1.0), grad(p[ba + 1], x - 1.0, y, z - 1.0)),
                lerp(u, grad(p[ab + 1], x, y - 1.0, z - 1.0), grad(p[bb + 1], x - 1.0, y - 1.0, z - 1.0)),
            ),
        )
    }

    pub fn noise2(&self, x: f64, z: f64) -> f64 {
        self.noise3(x, z, 0.0)
    }

    /// Port func_805_a: mengisi/menambah `out` dengan noise grid (sx*sy*sz), diskalakan 1/amp.
    #[allow(clippy::too_many_arguments)]
    pub fn fill(
        &self, out: &mut [f64],
        ox: f64, oy: f64, oz: f64,
        sx: usize, sy: usize, sz: usize,
        fx: f64, fy: f64, fz: f64, amp: f64,
    ) {
        let p = &self.perm;
        let inv = 1.0 / amp;
        let mut n = 0usize;

        if sy == 1 {
            for i in 0..sx {
                let mut x = (ox + i as f64) * fx + self.x_off;
                let xi = floor_i(x);
                let xa = (xi & 255) as usize;
                x -= xi as f64;
                let u = fade(x);
                for k in 0..sz {
                    let mut z = (oz + k as f64) * fz + self.z_off;
                    let zi = floor_i(z);
                    let za = (zi & 255) as usize;
                    z -= zi as f64;
                    let w = fade(z);
                    let a = p[xa] as usize;
                    let aa = p[a] as usize + za;
                    let b = p[xa + 1] as usize;
                    let ba = p[b] as usize + za;
                    let r1 = lerp(u, grad2(p[aa], x, z), grad(p[ba], x - 1.0, 0.0, z));
                    let r2 = lerp(u, grad(p[aa + 1], x, 0.0, z - 1.0), grad(p[ba + 1], x - 1.0, 0.0, z - 1.0));
                    out[n] += lerp(w, r1, r2) * inv;
                    n += 1;
                }
            }
            return;
        }

        let mut last_y: i32 = -1;
        let (mut d1, mut d2, mut d3, mut d4) = (0.0, 0.0, 0.0, 0.0);
        for i in 0..sx {
            let mut x = (ox + i as f64) * fx + self.x_off;
            let xi = floor_i(x);
            let xa = (xi & 255) as usize;
            x -= xi as f64;
            let u = fade(x);
            for k in 0..sz {
                let mut z = (oz + k as f64) * fz + self.z_off;
                let zi = floor_i(z);
                let za = (zi & 255) as usize;
                z -= zi as f64;
                let w = fade(z);
                for j in 0..sy {
                    let mut y = (oy + j as f64) * fy + self.y_off;
                    let yi = floor_i(y);
                    let ya = (yi & 255) as i32;
                    y -= yi as f64;
                    let v = fade(y);
                    if j == 0 || ya != last_y {
                        last_y = ya;
                        let a = p[xa] as usize + ya as usize;
                        let aa = p[a] as usize + za;
                        let ab = p[a + 1] as usize + za;
                        let b = p[xa + 1] as usize + ya as usize;
                        let ba = p[b] as usize + za;
                        let bb = p[b + 1] as usize + za;
                        d1 = lerp(u, grad(p[aa], x, y, z), grad(p[ba], x - 1.0, y, z));
                        d2 = lerp(u, grad(p[ab], x, y - 1.0, z), grad(p[bb], x - 1.0, y - 1.0, z));
                        d3 = lerp(u, grad(p[aa + 1], x, y, z - 1.0), grad(p[ba + 1], x - 1.0, y, z - 1.0));
                        d4 = lerp(u, grad(p[ab + 1], x, y - 1.0, z - 1.0), grad(p[bb + 1], x - 1.0, y - 1.0, z - 1.0));
                    }
                    let l1 = lerp(v, d1, d2);
                    let l2 = lerp(v, d3, d4);
                    out[n] += lerp(w, l1, l2) * inv;
                    n += 1;
                }
            }
        }
    }
}

pub struct Octaves {
    gens: Vec<Perlin>,
}

impl Octaves {
    pub fn new(rng: &mut JRandom, octaves: usize) -> Self {
        let gens = (0..octaves).map(|_| Perlin::new(rng)).collect();
        Self { gens }
    }

    pub fn noise2(&self, x: f64, z: f64) -> f64 {
        let mut sum = 0.0;
        let mut amp = 1.0;
        for g in &self.gens {
            sum += g.noise2(x * amp, z * amp) / amp;
            amp /= 2.0;
        }
        sum
    }

    /// Port generateNoiseOctaves. `out` dipakai ulang (nol-kan lalu isi), tanpa alokasi per panggilan.
    #[allow(clippy::too_many_arguments)]
    pub fn generate(
        &self, out: &mut Vec<f64>,
        ox: f64, oy: f64, oz: f64,
        sx: usize, sy: usize, sz: usize,
        fx: f64, fy: f64, fz: f64,
    ) {
        let len = sx * sy * sz;
        out.clear();
        out.resize(len, 0.0);
        let mut amp = 1.0;
        for g in &self.gens {
            g.fill(out, ox, oy, oz, sx, sy, sz, fx * amp, fy * amp, fz * amp, amp);
            amp /= 2.0;
        }
    }

    /// Port func_4109_a (varian 2D: y=10, 1 lapis).
    pub fn generate_2d(
        &self, out: &mut Vec<f64>, ox: i32, oz: i32, sx: usize, sz: usize, fx: f64, fz: f64,
    ) {
        self.generate(out, ox as f64, 10.0, oz as f64, sx, 1, sz, fx, 1.0, fz);
    }
}
