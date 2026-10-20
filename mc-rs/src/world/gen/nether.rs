//! The Nether: `ChunkProviderHell` (terrain, surface), `MapGenCavesHell` and `ChunkProviderHell.populate` (lava, fire, glowstone,
//! mushrooms) of b1.7.3, written from the decompiled classes (no golden file yet: the hashes need `tools/golden/G.java` to run
//! `ChunkProviderHell`; the tests below only check what the Java guarantees by construction).
//!
//! Same layout as the overworld: a chunk is `(x << 11) | (z << 7) | y`.
//!
//! - `func_4057_a`'s `field_4172_f` / `field_4171_g` (the 10- and 16-octave stacks fed by 1.0 / 100.0 scales) only feed a variable that
//!   stays 0.0 in the Java, so they are not sampled here; the stacks are still built, because their octaves draw from the same Random.
//! - UNVERIFIED: vanilla's `populate` does not reseed `hellRNG` (it continues from whatever chunk was generated last, so the Nether's
//!   decoration depends on load order). Here populate seeds it from the chunk like `provideChunk` does, so a chunk always gets the same
//!   decoration whichever worker runs it.
//! - No block ticks in populate (as for the overworld's springs): a lava source placed by `WorldGenHellLava` does not flow until it is
//!   updated, fire is placed but not ticked.

use crate::world::gen::noise::{ifloor, mh_cos, mh_sin, JavaRandom, OctaveNoise};
use crate::world::gen::populate::Region;
use std::f32::consts::PI;

pub const NETHERRACK: u8 = 87;
pub const SOUL_SAND: u8 = 88;
pub const GLOWSTONE: u8 = 89;
const AIR: u8 = 0;
const BEDROCK: u8 = 7;
const DIRT: u8 = 3;
const GRASS: u8 = 2;
const GRAVEL: u8 = 13;
const LAVA_MOVING: u8 = 10;
const LAVA: u8 = 11;
const FIRE: u8 = 51;
const MUSH_BROWN: u8 = 39;
const MUSH_RED: u8 = 40;
/// MapGenBase.field_1306_a.
const RANGE: i32 = 8;

pub struct Nether {
    seed: i64,
    rng: JavaRandom,
    lim: OctaveNoise,
    low: OctaveNoise,
    base: OctaveNoise,
    soul: OctaveNoise,
    depth: OctaveNoise,
}

impl Nether {
    pub fn new(seed: i64) -> Self {
        // The constructor draws every stack from one Random, in this order.
        let mut rng = JavaRandom::new(seed);
        let lim = OctaveNoise::new(&mut rng, 16);
        let low = OctaveNoise::new(&mut rng, 16);
        let base = OctaveNoise::new(&mut rng, 8);
        let soul = OctaveNoise::new(&mut rng, 4);
        let depth = OctaveNoise::new(&mut rng, 4);
        let _ = OctaveNoise::new(&mut rng, 10); // field_4177_a, field_4176_b: see the module docs
        let _ = OctaveNoise::new(&mut rng, 16);
        Self { seed, rng, lim, low, base, soul, depth }
    }

    fn chunk_seed(cx: i32, cz: i32) -> i64 {
        (cx as i64).wrapping_mul(341873128712).wrapping_add((cz as i64).wrapping_mul(132897987541))
    }

    /// `provideChunk`: terrain, surface, caves.
    pub fn generate(&mut self, cx: i32, cz: i32) -> Vec<u8> {
        self.rng.set_seed(Self::chunk_seed(cx, cz));
        let mut blocks = vec![AIR; 32768];
        self.terrain(cx, cz, &mut blocks);
        self.surface(cx, cz, &mut blocks);
        carve(self.seed, cx, cz, &mut blocks);
        blocks
    }

    /// `func_4057_a`: the 5 x 17 x 5 density grid of a chunk, index `(x * 5 + z) * 17 + y`.
    fn density(&self, x: i32, z: i32) -> Vec<f64> {
        const SY: usize = 17;
        let (xf, zf) = (x as f64, z as f64);
        let (s1, s2) = (684.412, 2053.236);
        let main = self.base.generate(xf, 0.0, zf, 5, SY, 5, s1 / 80.0, s2 / 60.0, s1 / 80.0);
        let a = self.lim.generate(xf, 0.0, zf, 5, SY, 5, s1, s2, s1);
        let b = self.low.generate(xf, 0.0, zf, 5, SY, 5, s1, s2, s1);
        let mut bias = [0.0f64; SY];
        for (i, v) in bias.iter_mut().enumerate() {
            *v = (i as f64 * std::f64::consts::PI * 6.0 / SY as f64).cos() * 2.0;
            let mut t = i as f64;
            if i > SY / 2 {
                t = (SY - 1 - i) as f64;
            }
            if t < 4.0 {
                t = 4.0 - t;
                *v -= t * t * t * 10.0;
            }
        }
        let mut out = vec![0.0; 5 * SY * 5];
        for (i, o) in out.iter_mut().enumerate() {
            let y = i % SY;
            let (n1, n2) = (a[i] / 512.0, b[i] / 512.0);
            let m = (main[i] / 10.0 + 1.0) / 2.0;
            let mut v = if m < 0.0 { n1 } else if m > 1.0 { n2 } else { n1 + (n2 - n1) * m };
            v -= bias[y];
            if y > SY - 4 {
                let t = ((y - (SY - 4)) as f32 / 3.0f32) as f64;
                v = v * (1.0 - t) + -10.0 * t;
            }
            *o = v;
        }
        out
    }

    /// `func_4059_a`: trilinear fill, netherrack where the density is positive, lava below y 32.
    fn terrain(&self, cx: i32, cz: i32, blocks: &mut [u8]) {
        let d = self.density(cx * 4, cz * 4);
        let at = |x: usize, z: usize, y: usize| d[(x * 5 + z) * 17 + y];
        for x in 0..4 {
            for z in 0..4 {
                for y in 0..16 {
                    let (mut c00, mut c01, mut c10, mut c11) = (at(x, z, y), at(x, z + 1, y), at(x + 1, z, y), at(x + 1, z + 1, y));
                    let s00 = (at(x, z, y + 1) - c00) * 0.125;
                    let s01 = (at(x, z + 1, y + 1) - c01) * 0.125;
                    let s10 = (at(x + 1, z, y + 1) - c10) * 0.125;
                    let s11 = (at(x + 1, z + 1, y + 1) - c11) * 0.125;
                    for sy in 0..8 {
                        let (mut a, mut b) = (c00, c01);
                        let (da, db) = ((c10 - c00) * 0.25, (c11 - c01) * 0.25);
                        for sx in 0..4 {
                            let mut i = ((x * 4 + sx) << 11) | ((z * 4) << 7) | (y * 8 + sy);
                            let mut v = a;
                            let dv = (b - a) * 0.25;
                            for _ in 0..4 {
                                let mut id = AIR;
                                if y * 8 + sy < 32 {
                                    id = LAVA;
                                }
                                if v > 0.0 {
                                    id = NETHERRACK;
                                }
                                blocks[i] = id;
                                i += 128;
                                v += dv;
                            }
                            a += da;
                            b += db;
                        }
                        c00 += s00;
                        c01 += s01;
                        c10 += s10;
                        c11 += s11;
                    }
                }
            }
        }
    }

    /// `func_4058_b`: bedrock floor and ceiling, soul sand / gravel patches, the lava shore. Every column draws 3 doubles, then
    /// one `nextInt(5)` per cell (a second one only when the first test fails), in the Java's order.
    fn surface(&mut self, cx: i32, cz: i32, blocks: &mut [u8]) {
        let s = 1.0 / 32.0;
        let (ox, oz) = ((cx * 16) as f64, (cz * 16) as f64);
        let n1 = self.soul.generate(ox, oz, 0.0, 16, 16, 1, s, s, 1.0);
        let n2 = self.soul.generate(ox, 109.0134, oz, 16, 1, 16, s, 1.0, s);
        let n3 = self.depth.generate(ox, oz, 0.0, 16, 16, 1, s * 2.0, s * 2.0, s * 2.0);
        let r = &mut self.rng;
        for i7 in 0..16usize {
            for i8 in 0..16usize {
                let soul = n1[i7 + i8 * 16] + r.next_double() * 0.2 > 0.0;
                let gravel = n2[i7 + i8 * 16] + r.next_double() * 0.2 > 0.0;
                let depth = (n3[i7 + i8 * 16] / 3.0 + 3.0 + r.next_double() * 0.25) as i32;
                let (mut left, mut top, mut fill) = (-1i32, NETHERRACK, NETHERRACK);
                for y in (0..128i32).rev() {
                    let i = (i8 * 16 + i7) * 128 + y as usize;
                    if y >= 127 - r.next_int_bound(5) || y <= r.next_int_bound(5) {
                        blocks[i] = BEDROCK;
                        continue;
                    }
                    match blocks[i] {
                        AIR => left = -1,
                        NETHERRACK if left == -1 => {
                            if depth <= 0 {
                                top = AIR;
                                fill = NETHERRACK;
                            } else if (60..=65).contains(&y) {
                                (top, fill) = (NETHERRACK, NETHERRACK);
                                if gravel {
                                    top = GRAVEL;
                                }
                                if soul {
                                    (top, fill) = (SOUL_SAND, SOUL_SAND);
                                }
                            }
                            if y < 64 && top == AIR {
                                top = LAVA;
                            }
                            left = depth;
                            blocks[i] = if y >= 63 { top } else { fill };
                        }
                        NETHERRACK if left > 0 => {
                            left -= 1;
                            blocks[i] = fill;
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// `ChunkProviderHell.populate` for chunk (cx, cz) on the 2x2 `Region` that starts there.
    pub fn populate(&mut self, w: &mut Region, cx: i32, cz: i32) {
        self.rng.set_seed(Self::chunk_seed(cx, cz)); // UNVERIFIED: see the module docs
        let (bx, bz) = (cx * 16, cz * 16);
        let r = &mut self.rng;
        for _ in 0..8 {
            let (x, y, z) = (bx + r.next_int_bound(16) + 8, r.next_int_bound(120) + 4, bz + r.next_int_bound(16) + 8);
            hell_lava(w, x, y, z);
        }
        let n = r.next_int_bound(10) + 1;
        for _ in 0..r.next_int_bound(n) + 1 {
            let (x, y, z) = (bx + r.next_int_bound(16) + 8, r.next_int_bound(120) + 4, bz + r.next_int_bound(16) + 8);
            fire(w, r, x, y, z);
        }
        let n = r.next_int_bound(10) + 1;
        for _ in 0..r.next_int_bound(n) {
            let (x, y, z) = (bx + r.next_int_bound(16) + 8, r.next_int_bound(120) + 4, bz + r.next_int_bound(16) + 8);
            glowstone(w, r, x, y, z);
        }
        for _ in 0..10 {
            let (x, y, z) = (bx + r.next_int_bound(16) + 8, r.next_int_bound(128), bz + r.next_int_bound(16) + 8);
            glowstone(w, r, x, y, z); // WorldGenGlowStone2 is the same class body as WorldGenGlowStone1
        }
        for id in [MUSH_BROWN, MUSH_RED] {
            if r.next_int_bound(1) == 0 {
                let (x, y, z) = (bx + r.next_int_bound(16) + 8, r.next_int_bound(128), bz + r.next_int_bound(16) + 8);
                crate::world::gen::populate::plants(w, r, id, x, y, z);
            }
        }
    }
}

/// `WorldGenHellLava`: a lava source in a netherrack wall (4 netherrack sides + roof... exactly 4 of the 5 neighbours, 1 air).
fn hell_lava(w: &mut Region, x: i32, y: i32, z: i32) {
    let here = w.get(x, y, z);
    if w.get(x, y + 1, z) != NETHERRACK || (here != AIR && here != NETHERRACK) {
        return;
    }
    let n = [(x - 1, y, z), (x + 1, y, z), (x, y, z - 1), (x, y, z + 1), (x, y - 1, z)];
    let rack = n.iter().filter(|&&(a, b, c)| w.get(a, b, c) == NETHERRACK).count();
    let air = n.iter().filter(|&&(a, b, c)| w.get(a, b, c) == AIR).count();
    if rack == 4 && air == 1 {
        w.set(x, y, z, LAVA_MOVING);
    }
}

/// `WorldGenFire`: 64 tries round the point, fire on netherrack.
fn fire(w: &mut Region, r: &mut JavaRandom, x: i32, y: i32, z: i32) {
    for _ in 0..64 {
        let x2 = x + r.next_int_bound(8) - r.next_int_bound(8);
        let y2 = y + r.next_int_bound(4) - r.next_int_bound(4);
        let z2 = z + r.next_int_bound(8) - r.next_int_bound(8);
        if w.get(x2, y2, z2) == AIR && w.get(x2, y2 - 1, z2) == NETHERRACK {
            w.set(x2, y2, z2, FIRE);
        }
    }
}

/// `WorldGenGlowStone1` / `2`: a seed on a netherrack ceiling, then 1500 tries below it that grow the cluster where an air cell
/// touches exactly one glowstone.
fn glowstone(w: &mut Region, r: &mut JavaRandom, x: i32, y: i32, z: i32) {
    if w.get(x, y, z) != AIR || w.get(x, y + 1, z) != NETHERRACK {
        return;
    }
    w.set(x, y, z, GLOWSTONE);
    for _ in 0..1500 {
        let x2 = x + r.next_int_bound(8) - r.next_int_bound(8);
        let y2 = y - r.next_int_bound(12);
        let z2 = z + r.next_int_bound(8) - r.next_int_bound(8);
        if w.get(x2, y2, z2) == AIR {
            let n = [(-1, 0, 0), (1, 0, 0), (0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1)];
            if n.iter().filter(|&&(a, b, c)| w.get(x2 + a, y2 + b, z2 + c) == GLOWSTONE).count() == 1 {
                w.set(x2, y2, z2, GLOWSTONE);
            }
        }
    }
}

/// `MapGenBase.func_867_a` + `MapGenCavesHell.func_868_a`: tunnels that start within 8 chunks and reach this one.
fn carve(seed: i64, cx: i32, cz: i32, blocks: &mut [u8]) {
    let mut rand = JavaRandom::new(seed);
    let a = rand.next_long() / 2 * 2 + 1;
    let b = rand.next_long() / 2 * 2 + 1;
    for x in cx - RANGE..=cx + RANGE {
        for z in cz - RANGE..=cz + RANGE {
            rand.set_seed((x as i64).wrapping_mul(a).wrapping_add((z as i64).wrapping_mul(b)) ^ seed);
            starts(&mut rand, x, z, cx, cz, blocks);
        }
    }
}

fn starts(r: &mut JavaRandom, sx: i32, sz: i32, cx: i32, cz: i32, blocks: &mut [u8]) {
    let a = r.next_int_bound(10) + 1;
    let b = r.next_int_bound(a) + 1;
    let mut count = r.next_int_bound(b);
    if r.next_int_bound(5) != 0 {
        count = 0;
    }
    for _ in 0..count {
        let x = (sx * 16 + r.next_int_bound(16)) as f64;
        let y = r.next_int_bound(128) as f64;
        let z = (sz * 16 + r.next_int_bound(16)) as f64;
        let mut n = 1;
        if r.next_int_bound(4) == 0 {
            let radius = 1.0 + r.next_float() * 6.0; // func_4129_a: a round room
            tunnel(r, cx, cz, blocks, (x, y, z), radius, 0.0, 0.0, -1, -1, 0.5);
            n += r.next_int_bound(4);
        }
        for _ in 0..n {
            let yaw = r.next_float() * PI * 2.0;
            let pitch = (r.next_float() - 0.5) * 2.0 / 8.0;
            let radius = r.next_float() * 2.0 + r.next_float();
            tunnel(r, cx, cz, blocks, (x, y, z), radius * 2.0, yaw, pitch, 0, 0, 0.5);
        }
    }
}

/// `MapGenCavesHell.func_4128_a`: walk one tunnel (or room: `step` -1) and clear netherrack, dirt and grass inside chunk (cx, cz).
/// Unlike the overworld's caves a tunnel stops at lava, never makes lava, and may use y 1..120.
#[allow(clippy::too_many_arguments)]
fn tunnel(main: &mut JavaRandom, cx: i32, cz: i32, blocks: &mut [u8], (mut x, mut y, mut z): (f64, f64, f64), radius: f32, mut yaw: f32,
          mut pitch: f32, mut step: i32, mut steps: i32, squash: f64) {
    let (mid_x, mid_z) = ((cx * 16 + 8) as f64, (cz * 16 + 8) as f64);
    let (mut dyaw, mut dpitch) = (0.0f32, 0.0f32);
    let mut r = JavaRandom::new(main.next_long());
    if steps <= 0 {
        let m = RANGE * 16 - 16;
        steps = m - r.next_int_bound(m / 4);
    }
    let mut room = false;
    if step == -1 {
        step = steps / 2;
        room = true;
    }
    let split_at = r.next_int_bound(steps / 2) + steps / 4;
    let steep = r.next_int_bound(6) == 0;
    while step < steps {
        let rad = 1.5 + (mh_sin(step as f32 * PI / steps as f32) * radius * 1.0) as f64;
        let rad_y = rad * squash;
        let c = mh_cos(pitch);
        x += (mh_cos(yaw) * c) as f64;
        y += mh_sin(pitch) as f64;
        z += (mh_sin(yaw) * c) as f64;
        pitch *= if steep { 0.92 } else { 0.7 };
        pitch += dpitch * 0.1;
        yaw += dyaw * 0.1;
        dpitch *= 0.9;
        dyaw *= 12.0 / 16.0;
        dpitch += (r.next_float() - r.next_float()) * r.next_float() * 2.0;
        dyaw += (r.next_float() - r.next_float()) * r.next_float() * 4.0;
        if !room && step == split_at && radius > 1.0 {
            let a = r.next_float() * 0.5 + 0.5;
            tunnel(main, cx, cz, blocks, (x, y, z), a, yaw - PI * 0.5, pitch / 3.0, step, steps, 1.0);
            let a = r.next_float() * 0.5 + 0.5;
            tunnel(main, cx, cz, blocks, (x, y, z), a, yaw + PI * 0.5, pitch / 3.0, step, steps, 1.0);
            return;
        }
        if room || r.next_int_bound(4) != 0 {
            let (dx, dz) = (x - mid_x, z - mid_z);
            let left = (steps - step) as f64;
            let reach = (radius + 2.0 + 16.0) as f64;
            if dx * dx + dz * dz - left * left > reach * reach {
                return;
            }
            if x >= mid_x - 16.0 - rad * 2.0 && z >= mid_z - 16.0 - rad * 2.0 && x <= mid_x + 16.0 + rad * 2.0 && z <= mid_z + 16.0 + rad * 2.0 {
                let x0 = (ifloor(x - rad) - cx * 16 - 1).max(0);
                let x1 = (ifloor(x + rad) - cx * 16 + 1).min(16);
                let y0 = (ifloor(y - rad_y) - 1).max(1);
                let y1 = (ifloor(y + rad_y) + 1).min(120);
                let z0 = (ifloor(z - rad) - cz * 16 - 1).max(0);
                let z1 = (ifloor(z + rad) - cz * 16 + 1).min(16);
                if !touches_lava(blocks, (x0, x1, y0, y1, z0, z1)) {
                    for ix in x0..x1 {
                        let nx = ((ix + cx * 16) as f64 + 0.5 - x) / rad;
                        for iz in z0..z1 {
                            let nz = ((iz + cz * 16) as f64 + 0.5 - z) / rad;
                            // Java quirk kept (as in the overworld's caves): the index starts one cell above `iy`.
                            let mut i = ((ix * 16 + iz) * 128 + y1) as usize;
                            for iy in (y0..y1).rev() {
                                let ny = (iy as f64 + 0.5 - y) / rad_y;
                                if ny > -0.7 && nx * nx + ny * ny + nz * nz < 1.0 && matches!(blocks[i], NETHERRACK | DIRT | GRASS) {
                                    blocks[i] = AIR;
                                }
                                i -= 1;
                            }
                        }
                    }
                    if room {
                        break;
                    }
                }
            }
        }
        step += 1;
    }
}

/// The Java's "is there lava in or on the shell of the box" scan (its jump included, as `caves::touches_water`).
fn touches_lava(blocks: &[u8], (x0, x1, y0, y1, z0, z1): (i32, i32, i32, i32, i32, i32)) -> bool {
    for ix in x0..x1 {
        for iz in z0..z1 {
            let mut iy = y1 + 1;
            while iy >= y0 - 1 {
                if (0..128).contains(&iy) {
                    let b = blocks[((ix * 16 + iz) * 128 + iy) as usize];
                    if b == LAVA_MOVING || b == LAVA {
                        return true;
                    }
                    if iy != y0 - 1 && ix != x0 && ix != x1 - 1 && iz != z0 && iz != z1 - 1 {
                        iy = y0;
                    }
                }
                iy -= 1;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::chunk::Nibbles;

    fn count(b: &[u8], id: u8) -> usize {
        b.iter().filter(|&&v| v == id).count()
    }

    /// What `provideChunk` guarantees by construction: bedrock on the whole ceiling (y 127) and a floor, lava only below the shore,
    /// netherrack as the rest, and the same chunk twice gives the same bytes.
    #[test]
    fn nether_chunk_has_its_shape() {
        let mut g = Nether::new(0xCAFEBABE);
        let a = g.generate(3, -2);
        assert_eq!(a, Nether::new(0xCAFEBABE).generate(3, -2), "deterministic, whichever generator makes it");
        for i in 0..256 {
            let col = &a[i * 128..(i + 1) * 128];
            assert_eq!(col[127], BEDROCK, "ceiling");
            assert_eq!(col[0], BEDROCK, "floor");
            assert!(col[1..127].iter().all(|&b| matches!(b, AIR | BEDROCK | NETHERRACK | LAVA | GRAVEL | SOUL_SAND)));
        }
        assert!(count(&a, NETHERRACK) > 4000 && count(&a, LAVA) > 0 && count(&a, AIR) > 4000);
    }

    /// Chunks differ, and the height map the light engine reads sees no sky anywhere (the ceiling is solid).
    #[test]
    fn nether_chunks_differ_and_are_roofed() {
        let mut g = Nether::new(7);
        let (a, b) = (g.generate(0, 0), g.generate(1, 0));
        assert_ne!(a, b);
        assert!(crate::world::chunk::height_map(&a).iter().all(|&h| h == 128));
    }

    /// Populate stays inside the netherrack: glowstone hangs under a ceiling, fire stands on netherrack, and it is repeatable.
    #[test]
    fn nether_populate_places_glowstone_and_fire_on_netherrack() {
        let mut glow = 0;
        // A cluster needs an air cell under a netherrack ceiling out of ~10 random tries a chunk, so one seed can have none: try a few.
        for seed in 11..31 {
            let mut g = Nether::new(seed);
            let mk = |g: &mut Nether| {
                let b = [g.generate(0, 0), g.generate(0, 1), g.generate(1, 0), g.generate(1, 1)];
                Region::new(0, 0, b, std::array::from_fn(|_| Nibbles::new()))
            };
            let (mut w1, mut w2) = (mk(&mut g), mk(&mut g));
            g.populate(&mut w1, 0, 0);
            g.populate(&mut w2, 0, 0);
            let (b1, _) = w1.into_parts();
            let (b2, _) = w2.into_parts();
            assert_eq!(b1, b2, "repeatable (seed {seed})");
            glow += b1.iter().map(|b| count(b, GLOWSTONE)).sum::<usize>();
            for b in &b1 {
                for i in 0..b.len() {
                    if b[i] == FIRE {
                        assert_eq!(b[i - 1], NETHERRACK, "fire stands on netherrack");
                    }
                }
            }
        }
        assert!(glow > 0, "some seed has a glowstone cluster");
    }
}
