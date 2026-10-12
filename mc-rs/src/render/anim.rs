//! Animated terrain tiles: `TextureWaterFX`, `TextureWaterFlowFX`, `TextureLavaFX`, `TextureLavaFlowFX` and `TextureFlamesFX` (x2),
//! 16 x 16 RGBA tiles redrawn every 20 Hz tick and written over their atlas tile (`ChunkPipeline::animate`).
//! `Math.random()` is a xorshift here (the Java's is unseeded too). The anaglyph branches are left out.

pub const WATER: u8 = 205;
pub const WATER_FLOW: u8 = 206;
pub const LAVA: u8 = 237;
pub const LAVA_FLOW: u8 = 238;
pub const FIRE: [u8; 2] = [31, 47];

pub type Tile = [u8; 1024];

struct Rng(u64);
impl Rng {
    fn f(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// The four 16 x 16 float fields of the water and lava classes: `g` (shown), `h` (next), `i` (height), `j` (velocity).
#[derive(Clone)]
struct Field { g: [f32; 256], h: [f32; 256], i: [f32; 256], j: [f32; 256] }
impl Field {
    fn new() -> Self { Self { g: [0.0; 256], h: [0.0; 256], i: [0.0; 256], j: [0.0; 256] } }
}

fn at(x: i32, y: i32) -> usize { ((x & 15) + (y & 15) * 16) as usize }

/// `TextureWaterFX` (`flow` = false: 3 wide, `/ 3.3`, `j -= 0.1`, 5 %) and `TextureWaterFlowFX` (`flow`: the 3 cells above, `/ 3.2`,
/// `j -= 0.3`, 20 %, and the picture scrolls down 1 row per tick). Returns the tile.
fn water(f: &mut Field, rng: &mut Rng, tick: i32, flow: bool) -> Tile {
    for x in 0..16 {
        for y in 0..16 {
            let sum: f32 = if flow { (y - 2..=y).map(|k| f.g[at(x, k)]).sum() } else { (x - 1..=x + 1).map(|k| f.g[at(k, y)]).sum() };
            f.h[at(x, y)] = sum / if flow { 3.2 } else { 3.3 } + f.i[at(x, y)] * 0.8;
        }
    }
    for k in 0..256 {
        f.i[k] = (f.i[k] + f.j[k] * 0.05).max(0.0);
        f.j[k] -= if flow { 0.3 } else { 0.1 };
        if rng.f() < if flow { 0.2 } else { 0.05 } {
            f.j[k] = 0.5;
        }
    }
    std::mem::swap(&mut f.g, &mut f.h);
    let mut t = [0u8; 1024];
    for k in 0..256i32 {
        let v = f.g[if flow { (k - tick * 16) & 255 } else { k } as usize].clamp(0.0, 1.0);
        let q = v * v;
        t[k as usize * 4..][..4].copy_from_slice(&[(32.0 + q * 32.0) as u8, (50.0 + q * 64.0) as u8, 255, (146.0 + q * 50.0) as u8]);
    }
    t
}

/// `TextureLavaFX` / `TextureLavaFlowFX` (`flow`: the picture scrolls 1 row every 3 ticks). The smoothing wobbles by `sin * 1.2`
/// (truncated) and the height / velocity update runs inside the same loop as the smoothing, like the Java.
fn lava(f: &mut Field, rng: &mut Rng, tick: i32, flow: bool) -> Tile {
    for x in 0..16 {
        for y in 0..16 {
            let a = ((y as f32 * std::f32::consts::PI * 2.0 / 16.0).sin() * 1.2) as i32;
            let b = ((x as f32 * std::f32::consts::PI * 2.0 / 16.0).sin() * 1.2) as i32;
            let mut sum = 0.0;
            for u in x - 1..=x + 1 {
                for v in y - 1..=y + 1 {
                    sum += f.g[at(u + a, v + b)];
                }
            }
            f.h[at(x, y)] = sum / 10.0 + (f.i[at(x, y)] + f.i[at(x + 1, y)] + f.i[at(x + 1, y + 1)] + f.i[at(x, y + 1)]) / 4.0 * 0.8;
            let k = at(x, y);
            f.i[k] = (f.i[k] + f.j[k] * 0.01).max(0.0);
            f.j[k] -= 0.06;
            if rng.f() < 0.005 {
                f.j[k] = 1.5;
            }
        }
    }
    std::mem::swap(&mut f.g, &mut f.h);
    let mut t = [0u8; 1024];
    for k in 0..256i32 {
        let v = (f.g[if flow { (k - tick / 3 * 16) & 255 } else { k } as usize] * 2.0).clamp(0.0, 1.0);
        t[k as usize * 4..][..4].copy_from_slice(&[(v * 100.0 + 155.0) as u8, (v * v * 255.0) as u8, (v * v * v * v * 128.0) as u8, 255]);
    }
    t
}

/// `TextureFlamesFX`: a 16 x 20 field, each cell the weighted mean of the cells below it; the bottom row is fed noise.
fn flames(g: &mut [f32; 320], rng: &mut Rng) -> Tile {
    let mut h = [0.0f32; 320];
    for x in 0..16i32 {
        for y in 0..20i32 {
            let (mut n, mut sum) = (18.0f32, g[(x + (y + 1) % 20 * 16) as usize] * 18.0);
            for u in x - 1..=x + 1 {
                for v in y..=y + 1 {
                    if (0..16).contains(&u) && (0..20).contains(&v) {
                        sum += g[(u + v * 16) as usize];
                    }
                    n += 1.0; // the Java counts every cell of the 3 x 2 window, also outside the field
                }
            }
            h[(x + y * 16) as usize] = sum / (n * 1.06);
            if y >= 19 {
                h[(x + y * 16) as usize] = rng.f() * rng.f() * rng.f() * 4.0 + rng.f() * 0.1 + 0.2;
            }
        }
    }
    *g = h;
    let mut t = [0u8; 1024];
    for k in 0..256 {
        let v = (g[k] * 1.8).clamp(0.0, 1.0);
        t[k * 4..][..4].copy_from_slice(&[(v * 155.0 + 100.0) as u8, (v * v * 255.0) as u8, (v.powi(10) * 255.0) as u8, if v < 0.5 { 0 } else { 255 }]);
    }
    t
}

pub struct Anim {
    rng: Rng,
    tick: i32,
    water: Field,
    water_flow: Field,
    lava: Field,
    lava_flow: Field,
    fire: [[f32; 320]; 2],
}

impl Anim {
    pub fn new() -> Self {
        Self { rng: Rng(0x9E37_79B9_7F4A_7C15), tick: 0, water: Field::new(), water_flow: Field::new(), lava: Field::new(), lava_flow: Field::new(), fire: [[0.0; 320]; 2] }
    }

    /// One 20 Hz tick: the new picture of every animated tile, `(terrain tile, RGBA)`.
    pub fn tick(&mut self) -> [(u8, Tile); 6] {
        self.tick += 1;
        let (t, r) = (self.tick, &mut self.rng);
        [
            (WATER, water(&mut self.water, r, t, false)),
            (WATER_FLOW, water(&mut self.water_flow, r, t, true)),
            (LAVA, lava(&mut self.lava, r, t, false)),
            (LAVA_FLOW, lava(&mut self.lava_flow, r, t, true)),
            (FIRE[0], flames(&mut self.fire[0], r)),
            (FIRE[1], flames(&mut self.fire[1], r)),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// After a few seconds every tile has the Java's colour ranges: water is blue and translucent, lava opaque and red, fire cuts out.
    #[test]
    fn tiles_stay_in_their_ranges() {
        let mut a = Anim::new();
        let mut last = a.tick();
        for _ in 0..200 {
            last = a.tick();
        }
        for (tile, px) in &last {
            let p = |k: usize| &px[k * 4..k * 4 + 4];
            match *tile {
                WATER | WATER_FLOW => assert!((0..256).all(|k| p(k)[2] == 255 && (146..=196).contains(&p(k)[3]) && p(k)[0] >= 32 && p(k)[0] <= 64)),
                LAVA | LAVA_FLOW => assert!((0..256).all(|k| p(k)[3] == 255 && p(k)[0] >= 155)),
                _ => assert!((0..256).all(|k| p(k)[3] == 0 || p(k)[3] == 255) && (0..256).any(|k| p(k)[3] == 255)),
            }
        }
        assert_ne!(a.tick()[0].1[..], last[0].1[..]); // it moves
    }
}
