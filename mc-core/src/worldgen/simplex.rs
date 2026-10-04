//! Port NoiseGenerator2 + NoiseGeneratorOctaves2 (simplex 2D), dipakai WorldChunkManager untuk biome.

use crate::jrandom::JRandom;

const GRAD: [[i32; 3]; 12] = [
    [1, 1, 0], [-1, 1, 0], [1, -1, 0], [-1, -1, 0],
    [1, 0, 1], [-1, 0, 1], [1, 0, -1], [-1, 0, -1],
    [0, 1, 1], [0, -1, 1], [0, 1, -1], [0, -1, -1],
];

#[inline]
fn wrap(v: f64) -> i32 {
    // Java: v > 0 ? (int)v : (int)v - 1 (bukan floor sejati; dipertahankan)
    if v > 0.0 { v as i32 } else { v as i32 - 1 }
}

#[inline]
fn dot(g: &[i32; 3], x: f64, y: f64) -> f64 {
    g[0] as f64 * x + g[1] as f64 * y
}

pub struct Simplex {
    perm: [i32; 512],
    xo: f64,
    yo: f64,
    #[allow(dead_code)]
    zo: f64,
}

impl Simplex {
    pub fn new(rng: &mut JRandom) -> Self {
        let xo = rng.next_double() * 256.0;
        let yo = rng.next_double() * 256.0;
        let zo = rng.next_double() * 256.0;
        let mut perm = [0i32; 512];
        for i in 0..256 {
            perm[i] = i as i32;
        }
        for i in 0..256usize {
            let j = rng.next_int_bound((256 - i) as i32) as usize + i;
            perm.swap(i, j);
            perm[i + 256] = perm[i];
        }
        Self { perm, xo, yo, zo }
    }

    /// Port func_4157_a: menambah ke `out` (urutan x luar, z dalam).
    #[allow(clippy::too_many_arguments)]
    pub fn add_to(&self, out: &mut [f64], ox: f64, oz: f64, sx: usize, sz: usize, fx: f64, fz: f64, amp: f64) {
        let f2 = 0.5 * (3.0f64.sqrt() - 1.0);
        let g2 = (3.0 - 3.0f64.sqrt()) / 6.0;
        let p = &self.perm;
        let mut n = 0usize;
        for i in 0..sx {
            let xin = (ox + i as f64) * fx + self.xo;
            for j in 0..sz {
                let yin = (oz + j as f64) * fz + self.yo;
                let s = (xin + yin) * f2;
                let ii = wrap(xin + s);
                let jj = wrap(yin + s);
                let t = (ii + jj) as f64 * g2;
                let x0 = xin - (ii as f64 - t);
                let y0 = yin - (jj as f64 - t);
                let (i1, j1) = if x0 > y0 { (1usize, 0usize) } else { (0usize, 1usize) };
                let x1 = x0 - i1 as f64 + g2;
                let y1 = y0 - j1 as f64 + g2;
                let x2 = x0 - 1.0 + 2.0 * g2;
                let y2 = y0 - 1.0 + 2.0 * g2;
                let iu = (ii & 255) as usize;
                let ju = (jj & 255) as usize;
                let gi0 = (p[iu + p[ju] as usize] % 12) as usize;
                let gi1 = (p[iu + i1 + p[ju + j1] as usize] % 12) as usize;
                let gi2 = (p[iu + 1 + p[ju + 1] as usize] % 12) as usize;

                let mut t0 = 0.5 - x0 * x0 - y0 * y0;
                let n0 = if t0 < 0.0 { 0.0 } else { t0 *= t0; t0 * t0 * dot(&GRAD[gi0], x0, y0) };
                let mut t1 = 0.5 - x1 * x1 - y1 * y1;
                let n1 = if t1 < 0.0 { 0.0 } else { t1 *= t1; t1 * t1 * dot(&GRAD[gi1], x1, y1) };
                let mut t2 = 0.5 - x2 * x2 - y2 * y2;
                let n2 = if t2 < 0.0 { 0.0 } else { t2 *= t2; t2 * t2 * dot(&GRAD[gi2], x2, y2) };

                out[n] += 70.0 * (n0 + n1 + n2) * amp;
                n += 1;
            }
        }
    }
}

pub struct SimplexOctaves {
    gens: Vec<Simplex>,
}

impl SimplexOctaves {
    pub fn new(rng: &mut JRandom, octaves: usize) -> Self {
        Self { gens: (0..octaves).map(|_| Simplex::new(rng)).collect() }
    }

    /// Port func_4112_a (persistensi 0.5).
    pub fn generate(&self, out: &mut Vec<f64>, ox: f64, oz: f64, sx: usize, sz: usize, fx: f64, fz: f64, lacunarity: f64) {
        self.generate_ex(out, ox, oz, sx, sz, fx, fz, lacunarity, 0.5);
    }

    /// Port func_4111_a
    #[allow(clippy::too_many_arguments)]
    pub fn generate_ex(
        &self, out: &mut Vec<f64>, ox: f64, oz: f64, sx: usize, sz: usize,
        fx: f64, fz: f64, lacunarity: f64, persistence: f64,
    ) {
        let fx = fx / 1.5;
        let fz = fz / 1.5;
        out.clear();
        out.resize(sx * sz, 0.0);
        let mut pers_acc = 1.0f64;
        let mut freq_acc = 1.0f64;
        for g in &self.gens {
            g.add_to(out, ox, oz, sx, sz, fx * freq_acc, fz * freq_acc, 0.55 / pers_acc);
            freq_acc *= lacunarity;
            pers_acc *= persistence;
        }
    }
}
