//! java.util.Random and the b1.7.3 noise generators, ported from the decomp:
//! NoiseGeneratorPerlin / NoiseGeneratorOctaves (3D, terrain) and
//! NoiseGenerator2 / NoiseGeneratorOctaves2 (2D simplex, biome climate).
//!
//! The Java generators all draw from ONE shared Random in construction
//! order, so `new` takes `&mut JavaRandom`. Array layout is x-major:
//! `(ix * sz + iz) * sy + iy`. Checked against the real classes by the
//! golden vectors in tools/golden/ (see the tests below).

const MULT: u64 = 0x5DEECE66D;
const MASK: u64 = (1u64 << 48) - 1;

/// java.util.Random (48-bit LCG), same algorithm as the JDK.
pub struct JavaRandom {
    seed: u64,
}

impl JavaRandom {
    pub fn new(seed: i64) -> Self {
        Self { seed: ((seed as u64) ^ MULT) & MASK }
    }

    pub fn set_seed(&mut self, seed: i64) {
        self.seed = ((seed as u64) ^ MULT) & MASK;
    }

    fn next(&mut self, bits: u32) -> i32 {
        self.seed = self.seed.wrapping_mul(MULT).wrapping_add(0xB) & MASK;
        (self.seed >> (48 - bits)) as i32
    }

    pub fn next_int(&mut self) -> i32 {
        self.next(32)
    }

    /// `nextInt(bound)`: uniform in [0, bound).
    pub fn next_int_bound(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "bound must be positive");
        if (bound & bound.wrapping_neg()) == bound {
            return ((bound as i64 * self.next(31) as i64) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let val = bits % bound;
            if bits.wrapping_sub(val).wrapping_add(bound - 1) >= 0 {
                return val;
            }
        }
    }

    pub fn next_long(&mut self) -> i64 {
        let hi = self.next(32) as i64;
        let lo = self.next(32) as i64;
        (hi << 32).wrapping_add(lo)
    }

    pub fn next_double(&mut self) -> f64 {
        let hi = self.next(26) as i64;
        let lo = self.next(27) as i64;
        ((hi << 27) + lo) as f64 / (1u64 << 53) as f64
    }

    pub fn next_float(&mut self) -> f32 {
        self.next(24) as f32 / (1u32 << 24) as f32
    }

    pub fn next_boolean(&mut self) -> bool {
        self.next(1) != 0
    }
}

/// MathHelper.floor_double: Java `(int)v` followed by the decomp's floor correction.
#[inline]
pub fn ifloor(v: f64) -> i32 {
    let i = v as i32;
    if v < i as f64 { i - 1 } else { i }
}

/// MathHelper.sin / cos: the 65536-entry float table, NOT `f32::sin` (the table is coarser).
fn sin_table() -> &'static [f32] {
    static T: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
    T.get_or_init(|| (0..65536).map(|i| (i as f64 * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32).collect())
}

pub fn mh_sin(x: f32) -> f32 {
    sin_table()[((x * 10430.378_f32) as i32 & 0xffff) as usize]
}

pub fn mh_cos(x: f32) -> f32 {
    sin_table()[((x * 10430.378_f32 + 16384.0_f32) as i32 & 0xffff) as usize]
}

#[inline]
fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

#[inline]
fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

fn grad(h: i32, x: f64, y: f64, z: f64) -> f64 {
    let h = h & 15;
    let u = if h < 8 { x } else { y };
    let v = if h < 4 { y } else if h != 12 && h != 14 { z } else { x };
    (if h & 1 == 0 { u } else { -u }) + (if h & 2 == 0 { v } else { -v })
}

/// NoiseGeneratorPerlin.func_4110_a: the x/z-only gradient the 2D path uses.
fn grad_xz(h: i32, x: f64, z: f64) -> f64 {
    let h = h & 15;
    let u = (1 - ((h & 8) >> 3)) as f64 * x;
    let v = if h < 4 { 0.0 } else if h != 12 && h != 14 { z } else { x };
    (if h & 1 == 0 { u } else { -u }) + (if h & 2 == 0 { v } else { -v })
}

/// One Perlin octave (NoiseGeneratorPerlin).
pub struct PerlinNoise {
    perm: [i32; 512],
    xc: f64,
    yc: f64,
    zc: f64,
}

impl PerlinNoise {
    pub fn new(rng: &mut JavaRandom) -> Self {
        let xc = rng.next_double() * 256.0;
        let yc = rng.next_double() * 256.0;
        let zc = rng.next_double() * 256.0;
        let mut perm = [0i32; 512];
        for (i, p) in perm.iter_mut().take(256).enumerate() {
            *p = i as i32;
        }
        for i in 0..256usize {
            let j = rng.next_int_bound(256 - i as i32) as usize + i;
            perm.swap(i, j);
            perm[i + 256] = perm[i];
        }
        Self { perm, xc, yc, zc }
    }

    #[inline]
    fn p(&self, i: i32) -> i32 {
        self.perm[i as usize]
    }

    /// NoiseGeneratorPerlin.generateNoise(x, y, z): one 3D sample, via the batch path (2 in y so
    /// the 3D branch runs; `dx = 1` and `amp = 1` make it the plain sample).
    pub fn sample(&self, x: f64, y: f64, z: f64) -> f64 {
        let mut out = [0.0; 2];
        self.add(&mut out, x, y, z, 1, 2, 1, 1.0, 1.0, 1.0, 1.0);
        out[0]
    }

    /// Add this octave's noise into `out` (func_805_a). `amp` is the octave
    /// scale from the stack; samples are divided by it.
    #[allow(clippy::too_many_arguments)]
    fn add(&self, out: &mut [f64], x: f64, y: f64, z: f64, sx: usize, sy: usize, sz: usize,
           dx: f64, dy: f64, dz: f64, amp: f64) {
        let inv = 1.0 / amp;
        let mut idx = 0;
        if sy == 1 {
            for ix in 0..sx {
                let mut xf = (x + ix as f64) * dx + self.xc;
                let xi = ifloor(xf);
                let xb = xi & 255;
                xf -= xi as f64;
                let u = fade(xf);
                for iz in 0..sz {
                    let mut zf = (z + iz as f64) * dz + self.zc;
                    let zi = ifloor(zf);
                    let zb = zi & 255;
                    zf -= zi as f64;
                    let w = fade(zf);
                    let a = self.p(xb);
                    let aa = self.p(a) + zb;
                    let b = self.p(xb + 1);
                    let ba = self.p(b) + zb;
                    let l1 = lerp(u, grad_xz(self.p(aa), xf, zf), grad(self.p(ba), xf - 1.0, 0.0, zf));
                    let l2 = lerp(u,
                        grad(self.p(aa + 1), xf, 0.0, zf - 1.0),
                        grad(self.p(ba + 1), xf - 1.0, 0.0, zf - 1.0));
                    out[idx] += lerp(w, l1, l2) * inv;
                    idx += 1;
                }
            }
            return;
        }
        let mut last_y = -1;
        let (mut d1, mut d2, mut d3, mut d4) = (0.0, 0.0, 0.0, 0.0);
        for ix in 0..sx {
            let mut xf = (x + ix as f64) * dx + self.xc;
            let xi = ifloor(xf);
            let xb = xi & 255;
            xf -= xi as f64;
            let u = fade(xf);
            for iz in 0..sz {
                let mut zf = (z + iz as f64) * dz + self.zc;
                let zi = ifloor(zf);
                let zb = zi & 255;
                zf -= zi as f64;
                let w = fade(zf);
                for iy in 0..sy {
                    let mut yf = (y + iy as f64) * dy + self.yc;
                    let yi = ifloor(yf);
                    let yb = yi & 255;
                    yf -= yi as f64;
                    let v = fade(yf);
                    if iy == 0 || yb != last_y {
                        last_y = yb;
                        let a = self.p(xb) + yb;
                        let aa = self.p(a) + zb;
                        let ab = self.p(a + 1) + zb;
                        let b = self.p(xb + 1) + yb;
                        let ba = self.p(b) + zb;
                        let bb = self.p(b + 1) + zb;
                        d1 = lerp(u, grad(self.p(aa), xf, yf, zf), grad(self.p(ba), xf - 1.0, yf, zf));
                        d2 = lerp(u, grad(self.p(ab), xf, yf - 1.0, zf), grad(self.p(bb), xf - 1.0, yf - 1.0, zf));
                        d3 = lerp(u, grad(self.p(aa + 1), xf, yf, zf - 1.0), grad(self.p(ba + 1), xf - 1.0, yf, zf - 1.0));
                        d4 = lerp(u,
                            grad(self.p(ab + 1), xf, yf - 1.0, zf - 1.0),
                            grad(self.p(bb + 1), xf - 1.0, yf - 1.0, zf - 1.0));
                    }
                    let l1 = lerp(v, d1, d2);
                    let l2 = lerp(v, d3, d4);
                    out[idx] += lerp(w, l1, l2) * inv;
                    idx += 1;
                }
            }
        }
    }
}

/// A stack of Perlin octaves (NoiseGeneratorOctaves).
pub struct OctaveNoise {
    gens: Vec<PerlinNoise>,
}

impl OctaveNoise {
    pub fn new(rng: &mut JavaRandom, octaves: usize) -> Self {
        Self { gens: (0..octaves).map(|_| PerlinNoise::new(rng)).collect() }
    }

    /// generateNoiseOctaves: `sx*sy*sz` samples starting at (x, y, z), steps
    /// (dx, dy, dz). Each octave halves the frequency and doubles the weight.
    #[allow(clippy::too_many_arguments)]
    pub fn generate(&self, x: f64, y: f64, z: f64, sx: usize, sy: usize, sz: usize,
                    dx: f64, dy: f64, dz: f64) -> Vec<f64> {
        let mut out = vec![0.0; sx * sy * sz];
        let mut amp = 1.0;
        for g in &self.gens {
            g.add(&mut out, x, y, z, sx, sy, sz, dx * amp, dy * amp, dz * amp, amp);
            amp /= 2.0;
        }
        out
    }

    /// NoiseGeneratorOctaves.func_806_a: one (x, z) sample summed over the octaves.
    pub fn sample(&self, x: f64, z: f64) -> f64 {
        let (mut v, mut s) = (0.0, 1.0);
        for g in &self.gens {
            v += g.sample(x * s, z * s, 0.0) / s;
            s /= 2.0;
        }
        v
    }

    /// func_4109_a: the x/z-only variant (sy = 1, y fixed at 10).
    pub fn generate_2d(&self, x: i32, z: i32, sx: usize, sz: usize, dx: f64, dz: f64) -> Vec<f64> {
        self.generate(x as f64, 10.0, z as f64, sx, 1, sz, dx, 1.0, dz)
    }
}

const GRAD3: [[i32; 3]; 12] = [
    [1, 1, 0], [-1, 1, 0], [1, -1, 0], [-1, -1, 0],
    [1, 0, 1], [-1, 0, 1], [1, 0, -1], [-1, 0, -1],
    [0, 1, 1], [0, -1, 1], [0, 1, -1], [0, -1, -1],
];

fn wrap(v: f64) -> i32 {
    if v > 0.0 { v as i32 } else { v as i32 - 1 }
}

fn corner(gi: i32, x: f64, y: f64) -> f64 {
    let t = 0.5 - x * x - y * y;
    if t < 0.0 {
        return 0.0;
    }
    let t = t * t;
    let g = &GRAD3[gi as usize];
    t * t * (g[0] as f64 * x + g[1] as f64 * y)
}

/// One 2D simplex octave (NoiseGenerator2).
pub struct SimplexNoise {
    perm: [i32; 512],
    xo: f64,
    yo: f64,
}

impl SimplexNoise {
    pub fn new(rng: &mut JavaRandom) -> Self {
        let xo = rng.next_double() * 256.0;
        let yo = rng.next_double() * 256.0;
        let _zo = rng.next_double() * 256.0; // drawn, never used
        let mut perm = [0i32; 512];
        for (i, p) in perm.iter_mut().take(256).enumerate() {
            *p = i as i32;
        }
        for i in 0..256usize {
            let j = rng.next_int_bound(256 - i as i32) as usize + i;
            perm.swap(i, j);
            perm[i + 256] = perm[i];
        }
        Self { perm, xo, yo }
    }

    #[allow(clippy::too_many_arguments)]
    fn add(&self, out: &mut [f64], x: f64, z: f64, sx: usize, sz: usize, dx: f64, dz: f64, amp: f64) {
        let f2 = 0.5 * (3.0_f64.sqrt() - 1.0);
        let g2 = (3.0 - 3.0_f64.sqrt()) / 6.0;
        let p = |i: i32| self.perm[i as usize];
        let mut idx = 0;
        for i in 0..sx {
            let xin = (x + i as f64) * dx + self.xo;
            for j in 0..sz {
                let yin = (z + j as f64) * dz + self.yo;
                let s = (xin + yin) * f2;
                let ii = wrap(xin + s);
                let jj = wrap(yin + s);
                let t = (ii + jj) as f64 * g2;
                let x0 = xin - (ii as f64 - t);
                let y0 = yin - (jj as f64 - t);
                let (i1, j1) = if x0 > y0 { (1, 0) } else { (0, 1) };
                let x1 = x0 - i1 as f64 + g2;
                let y1 = y0 - j1 as f64 + g2;
                let x2 = x0 - 1.0 + 2.0 * g2;
                let y2 = y0 - 1.0 + 2.0 * g2;
                let ib = ii & 255;
                let jb = jj & 255;
                let gi0 = p(ib + p(jb)) % 12;
                let gi1 = p(ib + i1 + p(jb + j1)) % 12;
                let gi2 = p(ib + 1 + p(jb + 1)) % 12;
                out[idx] += 70.0 * (corner(gi0, x0, y0) + corner(gi1, x1, y1) + corner(gi2, x2, y2)) * amp;
                idx += 1;
            }
        }
    }
}

/// A stack of simplex octaves (NoiseGeneratorOctaves2).
pub struct SimplexOctaves {
    gens: Vec<SimplexNoise>,
}

impl SimplexOctaves {
    pub fn new(rng: &mut JavaRandom, octaves: usize) -> Self {
        Self { gens: (0..octaves).map(|_| SimplexNoise::new(rng)).collect() }
    }

    /// func_4112_a: `sx*sz` samples; `lacunarity` is the per-octave frequency
    /// multiplier, persistence is fixed at 0.5.
    pub fn generate(&self, x: f64, z: f64, sx: usize, sz: usize, dx: f64, dz: f64, lacunarity: f64) -> Vec<f64> {
        let (dx, dz) = (dx / 1.5, dz / 1.5);
        let mut out = vec![0.0; sx * sz];
        let (mut freq, mut amp) = (1.0, 1.0);
        for g in &self.gens {
            g.add(&mut out, x, z, sx, sz, dx * freq, dz * freq, 0.55 / amp);
            freq *= lacunarity;
            amp *= 0.5;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: &[f64], b: &[f64]) {
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b) {
            assert!((x - y).abs() < 1e-9, "{x} != {y}");
        }
    }

    // Golden values come from the real b1.7.3 classes (tools/golden/G.java).
    #[test]
    fn octaves_match_java() {
        let mut r = JavaRandom::new(0xCAFEBABEi64);
        let n = OctaveNoise::new(&mut r, 4);
        close(&n.generate(10.0, 20.0, 30.0, 2, 3, 2, 0.05, 0.07, 0.09), &[
            -4.720342015445862, -4.649583033718352, -4.582807977351463, -4.952773590280636,
            -4.894379140217165, -4.837783983592656, -4.7247700475690415, -4.661145753241822,
            -4.60062249470231, -4.957584999336169, -4.909536679806759, -4.862025341300867,
        ]);
        close(&n.generate(-7.0, 10.0, 5.0, 3, 1, 2, 0.31, 1.0, 0.17), &[
            0.5407650752434064, 0.32118726397480146, -0.5542925765758715,
            -0.7991061672487361, -1.5576167699260337, -1.750857779583505,
        ]);
        // Same Random keeps going after the noise was built.
        assert_eq!(r.next_int_bound(1000), 822);
        assert_eq!(r.next_int_bound(64), 9);
        assert_eq!(r.next_long(), 2035750212453278413);
        assert!((r.next_double() - 0.0075145035611901).abs() < 1e-15);
    }

    #[test]
    fn simplex_matches_java() {
        let mut r = JavaRandom::new(777);
        let s = SimplexOctaves::new(&mut r, 3);
        close(&s.generate(12.0, -30.0, 3, 2, 0.025, 0.05, 0.25), &[
            0.865514441206283, 0.8893549615037963, 0.8918411455319262,
            0.9154464514247099, 0.9194438467113024, 0.9427534071340529,
        ]);
    }

    #[test]
    fn random_matches_java() {
        let mut r = JavaRandom::new(-99);
        assert_eq!(r.next_int(), 1191266223);
        assert_eq!(r.next_int_bound(5), 0);
        assert_eq!(r.next_int_bound(16), 1);
        assert_eq!(r.next_int_bound(100), 40);
        assert_eq!(r.next_long(), 998464440063232087);
        assert_eq!(r.next_float(), 0.88211465_f32);
        assert!(!r.next_boolean());
        assert_eq!(r.next_double(), 0.026367317307998128);
    }
}
