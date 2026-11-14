//! Animated terrain tiles: `TextureWaterFX`, `TextureWaterFlowFX`, `TextureLavaFX`, `TextureLavaFlowFX`, `TextureFlamesFX` (x2) and `TexturePortalFX`,
//! 16 x 16 RGBA tiles redrawn every 20 Hz tick and written over their atlas tile (`ChunkPipeline::animate`).
//! `Math.random()` is a xorshift here (the Java's is unseeded too). The anaglyph branches are left out.

pub const WATER: u8 = 205;
pub const WATER_FLOW: u8 = 206;
pub const LAVA: u8 = 237;
pub const LAVA_FLOW: u8 = 238;
pub const FIRE: [u8; 2] = [31, 47];
/// `Block.portal.blockIndexInTexture`.
pub const PORTAL: u8 = 14;

pub type Tile = [u8; 1024];

/// Item tiles of `gui/items.png` that animate (`Item.compass` icon (6, 3), `Item.pocketSundial` (6, 4)).
pub const COMPASS: u8 = 54;
pub const CLOCK: u8 = 70;
/// `misc/dial.png` (16 x 16 RGBA): the sky painted into the clock's magenta face (`TextureWatchFX`); `tools/gen_terrain.py` writes it.
const DIAL: &[u8] = include_bytes!("../../assets/dial.rgba");

/// `TexturePortalFX`'s constructor: the 32 frames of the portal (`Random(100)`, two swirling layers wrapped on a 16 x 16 torus, `MathHelper.sin`),
/// each 16 x 16 RGBA. The Java stores the four channels as `(byte)` casts; the values stay under 255 (the sum is at most 0.78 here), so the
/// casts never wrap. The anaglyph branch is left out.
fn portal_frames() -> Vec<Tile> {
    use crate::world::gen::noise::{mh_sin, JavaRandom};
    use std::f32::consts::PI;
    let mut rng = JavaRandom::new(100);
    // `if d < -1 { d += 2 }` then `if d >= 1 { d -= 2 }`, one after the other as in the Java.
    let wrap = |mut d: f32| {
        if d < -1.0 {
            d += 2.0;
        }
        if d >= 1.0 {
            d -= 2.0;
        }
        d
    };
    (0..32)
        .map(|frame| {
            let mut t = [0u8; 1024];
            for x in 0..16 {
                for y in 0..16 {
                    let mut v = 0.0f32;
                    for layer in 0..2 {
                        let off = (layer * 8) as f32;
                        let (a, b) = (wrap((x as f32 - off) / 16.0 * 2.0), wrap((y as f32 - off) / 16.0 * 2.0));
                        let r2 = a * a + b * b;
                        let ang = (b as f64).atan2(a as f64) as f32 + (frame as f32 / 32.0 * PI * 2.0 - r2 * 10.0 + (layer * 2) as f32) * (layer * 2 - 1) as f32;
                        v += (mh_sin(ang) + 1.0) / 2.0 / (r2 + 1.0) * 0.5;
                    }
                    v += rng.next_float() * 0.1;
                    let i = (y * 16 + x) * 4;
                    let (lo, hi) = ((v * 100.0 + 155.0) as i32 as u8, (v * v * v * v * 255.0) as i32 as u8);
                    t[i..i + 4].copy_from_slice(&[(v * v * 200.0 + 55.0) as i32 as u8, hi, lo, lo]);
                }
            }
            t
        })
        .collect()
}

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

/// The 16 x 16 picture of an item tile, straight from `items.png`.
fn items_tile(t: u8) -> Tile {
    let mut o = [0u8; 1024];
    for r in 0..16 {
        let src = ((t as usize / 16 * 16 + r) * 256 + t as usize % 16 * 16) * 4;
        o[r * 64..][..64].copy_from_slice(&crate::render::atlas::ITEMS[src..src + 64]);
    }
    o
}

/// The spring both gauges use: the shown angle `s.0` chases `target` (the difference wrapped to +-pi, then to +-1), `s.1` is its
/// speed (`+= diff * 0.1`, `*= 0.8`).
fn chase(s: &mut (f64, f64), target: f64) {
    use std::f64::consts::{PI, TAU};
    let mut d = target - s.0;
    while d < -PI { d += TAU }
    while d >= PI { d -= TAU }
    d = d.clamp(-1.0, 1.0);
    s.1 = (s.1 + d * 0.1) * 0.8;
    s.0 += s.1;
}

/// `TextureCompassFX.onTick`: the needle is drawn over the plain icon: a grey tail (9 dots), then 25 dots along the needle, grey
/// behind the pivot and red in front of it.
fn compass(base: &Tile, a: f64) -> Tile {
    let mut t = *base;
    let (s, c) = a.sin_cos();
    let mut put = |x: f64, y: f64, rgba: [u8; 4]| {
        let (x, y) = (x as i32, y as i32);
        if (0..16).contains(&x) && (0..16).contains(&y) { t[(y * 16 + x) as usize * 4..][..4].copy_from_slice(&rgba) }
    };
    for i in -4..=4 {
        put(8.5 + c * i as f64 * 0.3, 7.5 - s * i as f64 * 0.3 * 0.5, [100, 100, 100, 255]);
    }
    for i in -8..=16 {
        put(8.5 + s * i as f64 * 0.3, 7.5 + c * i as f64 * 0.3 * 0.5, if i >= 0 { [255, 20, 20, 255] } else { [100, 100, 100, 255] });
    }
    t
}

/// `TextureWatchFX.onTick`: every magenta pixel of the clock icon (red = blue, no green) shows the dial turned by `a`, as bright
/// as that pixel was.
fn watch(base: &Tile, a: f64) -> Tile {
    let mut t = *base;
    let (s, c) = a.sin_cos();
    for i in 0..256 {
        let [r, g, b, _] = [base[i * 4], base[i * 4 + 1], base[i * 4 + 2], base[i * 4 + 3]];
        if r == b && g == 0 && b > 0 {
            let x = -((i % 16) as f64 / 15.0 - 0.5);
            let y = (i / 16) as f64 / 15.0 - 0.5;
            let (u, v) = (((x * c + y * s + 0.5) * 16.0) as i32, ((y * c - x * s + 0.5) * 16.0) as i32);
            let d = ((u & 15) + (v & 15) * 16) as usize * 4;
            let k = r as u32;
            t[i * 4..][..4].copy_from_slice(&[(DIAL[d] as u32 * k / 255) as u8, (DIAL[d + 1] as u32 * k / 255) as u8, (DIAL[d + 2] as u32 * k / 255) as u8, DIAL[d + 3]]);
        }
    }
    t
}

pub struct Anim {
    /// Shown angle and speed of the compass and clock hands (`field_4229_i` / `field_4228_j`, `field_4222_j` / `field_4221_k`).
    compass: (f64, f64),
    clock: (f64, f64),
    rng: Rng,
    tick: i32,
    water: Field,
    water_flow: Field,
    lava: Field,
    lava_flow: Field,
    fire: [[f32; 320]; 2],
    /// `TexturePortalFX.portalTextureData`: the 32 frames, shown in turn (`portalTickCounter & 31`).
    portal: Vec<Tile>,
}

impl Anim {
    pub fn new() -> Self {
        Self { compass: (0.0, 0.0), clock: (0.0, 0.0), rng: Rng(0x9E37_79B9_7F4A_7C15), tick: 0, water: Field::new(), water_flow: Field::new(), lava: Field::new(), lava_flow: Field::new(), fire: [[0.0; 320]; 2], portal: portal_frames() }
    }

    /// One 20 Hz tick of `TextureCompassFX` and `TextureWatchFX`: the new pictures of the compass and clock items,
    /// `(items.png tile, RGBA)`. `yaw` is `rotationYaw` in degrees, `pos` the player's x / z, `spawn` the world spawn x / z and
    /// `celestial` `World.getCelestialAngle(1.0)`. `nether`: `worldProvider.isNether`, where both hands aim at a new random angle
    /// (`Math.random() * PI * 2`) every tick and so spin. ponytail: no anaglyph.
    pub fn gauges(&mut self, yaw: f32, pos: (f64, f64), spawn: (f64, f64), celestial: f32, nether: bool) -> [(u8, Tile); 2] {
        let (compass_to, clock_to) = if nether {
            (self.rng.f() as f64 * std::f64::consts::TAU, self.rng.f() as f64 * std::f64::consts::TAU)
        } else {
            ((yaw as f64 - 90.0).to_radians() - (spawn.1 - pos.1).atan2(spawn.0 - pos.0), -(celestial as f64) * std::f64::consts::TAU)
        };
        chase(&mut self.compass, compass_to);
        chase(&mut self.clock, clock_to);
        [(COMPASS, compass(&items_tile(COMPASS), self.compass.0)), (CLOCK, watch(&items_tile(CLOCK), self.clock.0))]
    }

    /// One 20 Hz tick: the new picture of every animated tile, `(terrain tile, RGBA)`.
    pub fn tick(&mut self) -> [(u8, Tile); 7] {
        self.tick += 1;
        let (t, r) = (self.tick, &mut self.rng);
        [
            (WATER, water(&mut self.water, r, t, false)),
            (WATER_FLOW, water(&mut self.water_flow, r, t, true)),
            (LAVA, lava(&mut self.lava, r, t, false)),
            (LAVA_FLOW, lava(&mut self.lava_flow, r, t, true)),
            (FIRE[0], flames(&mut self.fire[0], r)),
            (FIRE[1], flames(&mut self.fire[1], r)),
            (PORTAL, self.portal[(t & 31) as usize]),
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
                PORTAL => assert!((0..256).all(|k| p(k)[3] >= 155 && p(k)[3] == p(k)[2] && p(k)[0] >= 55)),
                _ => assert!((0..256).all(|k| p(k)[3] == 0 || p(k)[3] == 255) && (0..256).any(|k| p(k)[3] == 255)),
            }
        }
        assert_ne!(a.tick()[0].1[..], last[0].1[..]); // it moves
    }

    /// The portal plays its 32 frames in a loop (the counter is pre-incremented: the first tick shows frame 1), each frame differs from the next,
    /// and frame 0 starts with the pixel of the Java's constructor worked out by hand (`Random(100)`; UNVERIFIED against a real run).
    #[test]
    fn the_portal_loops_through_32_frames() {
        let frames = portal_frames();
        assert_eq!(frames.len(), 32);
        assert_eq!(frames[0][..4], [75, 2, 187, 187]);
        assert!(frames.windows(2).all(|w| w[0] != w[1]));
        let mut a = Anim::new();
        let shown: Vec<Tile> = (0..64).map(|_| a.tick()[6].1).collect();
        assert!(shown[0] == frames[1] && shown[31] == frames[0] && shown[32] == frames[1]);
    }

    /// The compass hand settles on the spawn direction and stays; the red tip is drawn; the clock face differs between noon and
    /// midnight and the parts of the icon that are not magenta never change.
    #[test]
    fn compass_and_clock_follow_their_targets() {
        let mut a = Anim::new();
        let mut last = a.gauges(0.0, (0.0, 0.0), (10.0, 0.0), 0.0, false);
        for _ in 0..200 {
            last = a.gauges(0.0, (0.0, 0.0), (10.0, 0.0), 0.0, false);
        }
        let again = a.gauges(0.0, (0.0, 0.0), (10.0, 0.0), 0.0, false);
        assert!(last[0].1 == again[0].1 && last[1].1 == again[1].1, "both hands have settled");
        assert!(again[0].1.chunks(4).any(|p| p == [255, 20, 20, 255]), "red needle tip");
        let (base, noon) = (items_tile(CLOCK), again[1].1);
        let midnight = {
            let mut b = Anim::new();
            (0..200).map(|_| b.gauges(0.0, (0.0, 0.0), (10.0, 0.0), 0.5, false)).last().unwrap()[1].1
        };
        assert!(noon != midnight, "the dial turns with the sky");
        assert!((0..256).filter(|&i| !(base[i * 4] == base[i * 4 + 2] && base[i * 4 + 1] == 0 && base[i * 4 + 2] > 0)).all(|i| noon[i * 4..][..4] == base[i * 4..][..4]));
    }

    /// In the Nether neither hand settles: the target is a new random angle every tick, so the pictures keep changing.
    #[test]
    fn the_nether_spins_both_hands() {
        let mut a = Anim::new();
        let shots: Vec<[(u8, Tile); 2]> = (0..60).map(|_| a.gauges(0.0, (0.0, 0.0), (10.0, 0.0), 0.0, true)).collect();
        assert!(shots[30..].windows(2).filter(|w| w[0][0].1 != w[1][0].1).count() > 10, "the needle keeps moving");
        assert!(shots[30..].windows(2).filter(|w| w[0][1].1 != w[1][1].1).count() > 10, "the dial keeps moving");
    }
}
