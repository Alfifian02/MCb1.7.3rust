//! Visual polish on top of the Console-Edition look, bounded and switchable. Pure CPU (the only GPU parts are `GlowMesh`'s two buffers),
//! computed once a frame in `Look::new` and read by BOTH renderers (`WorldDraw::look`), so the wgpu and the GLES path cannot drift.
//!
//! * One dial: `vibrance` (Video Settings, 0 = the grade is off) scales the whole grade of the final pass: vibrance (not saturation: the
//!   already vivid colours stay put), a gentle S-curve and a warm-highlight / cool-shadow split. Conservative default, bounded top end.
//! * Five switches (Video Settings > Effects...), each can be turned off: `warm` (golden low sun, bluer shade, a little more face
//!   shading), `water` (depth gradient + sun glint), `glow` (soft additive halos on lava, torches, glowstone and the sun only),
//!   `motion` (off / low / on: leaf sway, a few drifting motes near flowers and water, light rays at a low sun), `haze` (aerial perspective).
//! * Caps: at most `MAX_GLOW` halos, `MOTE_CAP` motes and `MAX_QUADS` sprites a frame, whatever the world holds.
//!
//! Nothing here is b1.7.3; every number is a taste value. UNVERIFIED on a device: all of them (by eye only).

use crate::gpu::gfx::{Buf, Gfx};
use crate::render::mesh::VF;
use crate::world::chunks::ChunkManager;
use glam::{Mat4, Vec3};
use std::collections::HashMap;

/// The Vibrance dial's default: about a third of its range, a light touch.
pub const VIBRANCE_DEFAULT: f32 = 0.3;
// The grade at dial 1.0 (the dial scales all three together).
const GRADE_VIB: f32 = 0.5; // vibrance amount: a dull colour gains up to +50%, a vivid one almost nothing
const GRADE_CURVE: f32 = 0.35; // S-curve blend
const GRADE_SPLIT: f32 = 0.05; // warm / cool split, per channel
// Light tones (multipliers of the lit colour, in the frame's linear space).
const DAY_WARM: [f32; 3] = [1.015, 1.0, 0.985];
const GOLD: [f32; 3] = [1.10, 0.97, 0.76];
const DAY_COOL: [f32; 3] = [0.96, 0.985, 1.04];
const DUSK_COOL: [f32; 3] = [0.90, 0.96, 1.09];
/// Extra exponent of the face shades (1 + this): the sides and the underside of a block sink a little deeper.
const FACE_BOOST: f32 = 0.10;
/// How much of the distance tint the fog factor brings (`AER` in the shaders), 0..1.
const AERIAL: f32 = 1.0;
/// The water's pull towards its turquoise / deep blue, and the glint's brightness (sun; the moon's is `MOON_GLINT` of it).
const WATER_DEPTH: f32 = 0.55;
const GLINT: f32 = 0.9;
const MOON_GLINT: f32 = 0.25;
/// Leaf sway, blocks: low / on.
const SWAY: [f32; 3] = [0.0, 0.025, 0.04];
/// Motes alive at once: off / low / on.
const MOTE_CAP: [usize; 3] = [0, 10, 24];

/// What the player chose (`Settings::look`, `look_*:` lines of options.txt).
#[derive(Clone, Debug, PartialEq)]
pub struct LookCfg {
    /// 0..1; 0 turns the grade off.
    pub vibrance: f32,
    pub warm: bool,
    pub water: bool,
    pub glow: bool,
    /// 0 off, 1 low, 2 on.
    pub motion: u8,
    pub haze: bool,
}

impl Default for LookCfg {
    fn default() -> Self {
        Self { vibrance: VIBRANCE_DEFAULT, warm: true, water: true, glow: true, motion: 1, haze: true }
    }
}

impl LookCfg {
    /// One `look_*` line; a bad one is skipped.
    pub fn load(&mut self, k: &str, v: &str) {
        let (b, n) = (v == "true", v.parse::<f32>().ok());
        match (k, n) {
            ("look_vibrance", Some(f)) => self.vibrance = f.clamp(0.0, 1.0),
            ("look_motion", Some(f)) if (0.0..=2.0).contains(&f) => self.motion = f as u8,
            ("look_warm", _) => self.warm = b,
            ("look_water", _) => self.water = b,
            ("look_glow", _) => self.glow = b,
            ("look_haze", _) => self.haze = b,
            _ => {}
        }
    }

    pub fn save_into(&self, t: &mut String) {
        t.push_str(&format!("look_vibrance:{:.3}\nlook_warm:{}\nlook_water:{}\nlook_glow:{}\nlook_motion:{}\nlook_haze:{}\n", self.vibrance, self.warm, self.water, self.glow, self.motion, self.haze));
    }
}

/// What a frame knows that the look depends on.
pub struct Env {
    /// Unit vector towards the sun (the moon is the opposite one).
    pub sun: Vec3,
    pub rain: f32,
    pub seconds: f32,
    pub nether: bool,
    /// The eye is in water: no halos, motes or glints, no warm light (the underwater fog rules).
    pub underwater: bool,
}

/// The frame's look: the uniform values both renderers write (`grade` to the final pass, the rest to the chunk shaders) plus what the CPU
/// needs for sprites. `Look::OFF` changes nothing in any shader.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Look {
    /// Final pass: x vibrance amount, y S-curve blend, z warm / cool split (all 0 = the grade is skipped).
    pub grade: [f32; 4],
    /// Chunk shaders: rgb tone of a lit surface; w = extra face-shade exponent.
    pub warm: [f32; 4],
    /// rgb tone of a shaded surface; w = aerial tint strength.
    pub cool: [f32; 4],
    /// xyz: direction to the sun (the moon at night); w: glint strength (0 = none).
    pub sun: [f32; 4],
    /// x: seconds; y: water depth gradient strength; z: leaf sway in blocks; w: unused.
    pub fx: [f32; 4],
    /// Light rays fan out of a low sun (`sky::fan_vertices`).
    pub rays: bool,
    /// 0..1 how golden the light is (low sun, dry): the sun halo's colour and the rays follow it.
    pub gold: f32,
    /// Halos and motes are drawn.
    pub glow: bool,
    pub motes: usize,
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

impl Look {
    pub const OFF: Look = Look { grade: [0.0; 4], warm: [1.0, 1.0, 1.0, 0.0], cool: [1.0, 1.0, 1.0, 0.0], sun: [0.0, 1.0, 0.0, 0.0], fx: [0.0; 4], rays: false, gold: 0.0, glow: false, motes: 0 };

    pub fn new(c: &LookCfg, e: &Env) -> Self {
        let mut l = Self::OFF;
        let v = c.vibrance.clamp(0.0, 1.0);
        l.grade = [GRADE_VIB * v, GRADE_CURVE * v, GRADE_SPLIT * v, 0.0];
        l.fx[0] = e.seconds;
        let dry = 1.0 - e.rain;
        let y = e.sun.y;
        // Gold peaks with the sun just above the horizon and is gone by 45% of the way up (or in rain).
        l.gold = smooth(-0.12, 0.05, y) * (1.0 - smooth(0.05, 0.45, y)) * dry;
        if e.nether {
            return l; // the Nether keeps its own colours; only the grade (and the sprite switches' defaults) apply
        }
        if c.warm && !e.underwater {
            let (w, k) = (lerp3(DAY_WARM, GOLD, l.gold), lerp3(DAY_COOL, DUSK_COOL, l.gold));
            l.warm = [w[0], w[1], w[2], FACE_BOOST];
            l.cool = [k[0], k[1], k[2], 0.0];
        }
        if c.haze && !e.underwater {
            l.cool[3] = AERIAL * (1.0 - 0.5 * e.rain);
        }
        if c.water && !e.underwater {
            l.fx[1] = WATER_DEPTH;
            let (d, s) = if y >= 0.0 { (e.sun, GLINT) } else { (-e.sun, MOON_GLINT * GLINT) };
            l.sun = [d.x, d.y, d.z, s * smooth(0.02, 0.2, d.y) * dry];
        }
        l.fx[2] = SWAY[c.motion.min(2) as usize];
        l.rays = c.motion > 0 && !e.underwater;
        l.glow = c.glow && !e.underwater;
        l.motes = if e.underwater { 0 } else { MOTE_CAP[c.motion.min(2) as usize] };
        l
    }
}

// ---- Sprites: soft additive discs (halos and motes), one pipeline in each renderer (`fs_glow`) ----

/// Quads a frame can hold: the buffers' size.
pub const MAX_QUADS: usize = 80;
/// Halos drawn at once (the nearest).
const MAX_GLOW: usize = 36;
/// Halos fade out between these distances, blocks.
const GLOW_FADE: (f32, f32) = (24.0, 40.0);

/// A soft disc facing the camera. `nofog`: the sun's, drawn at the far plane where the fog would erase it.
#[derive(Copy, Clone, Debug)]
pub struct Quad {
    pub pos: Vec3,
    pub half: f32,
    /// Colour as seen (gamma), the vertex stage linearises it like any tint.
    pub rgb: [f32; 3],
    pub a: f32,
    pub nofog: bool,
}

/// The block ids the scan keeps: lava (flowing, still), torch, glowstone, then the two flowers.
const SCAN_IDS: [u8; 6] = [10, 11, 50, 89, 37, 38];
const SCAN_R: i32 = 2;
/// Cells kept per chunk and kind (halo emitters / flowers).
const PER_CHUNK: usize = 16;

fn is_flower(id: i32) -> bool {
    matches!(id, 37 | 38)
}

/// The emitters and flowers around the player: one chunk a frame, round robin over the (2 * SCAN_R + 1)^2 around it, so a torch placed
/// or broken shows within about half a second and the cost is one pass over 32 KB a frame.
#[derive(Default)]
pub struct Scan {
    cache: HashMap<(i32, i32), Vec<[i32; 4]>>,
    tick: usize,
}

impl Scan {
    /// The chunk to rescan this frame, given the player's chunk.
    fn next_key(&mut self, (cx, cz): (i32, i32)) -> (i32, i32) {
        let n = 2 * SCAN_R + 1;
        let i = (self.tick % (n * n) as usize) as i32;
        self.tick += 1;
        (cx + i % n - SCAN_R, cz + i / n - SCAN_R)
    }

    /// Keep what a scan of chunk `key` found (`[x, y, z, id]`): lava once per 4 blocks (a lake is one halo a few cells apart), the
    /// rest as it comes, `PER_CHUNK` of each kind at most; drop chunks that are out of reach.
    fn absorb(&mut self, key: (i32, i32), player: (i32, i32), cells: impl Iterator<Item = [i32; 4]>) {
        let mut kept: Vec<[i32; 4]> = Vec::new();
        let (mut glows, mut flowers) = (0, 0);
        for c in cells {
            let lava = matches!(c[3], 10 | 11);
            if is_flower(c[3]) {
                flowers += 1;
                if flowers > PER_CHUNK {
                    continue;
                }
            } else {
                if glows >= PER_CHUNK || (lava && kept.iter().any(|k| matches!(k[3], 10 | 11) && (k[0] - c[0]).abs() + (k[1] - c[1]).abs() + (k[2] - c[2]).abs() < 4)) {
                    continue;
                }
                glows += 1;
            }
            kept.push(c);
        }
        if kept.is_empty() {
            self.cache.remove(&key);
        } else {
            self.cache.insert(key, kept);
        }
        self.cache.retain(|k, _| (k.0 - player.0).abs() <= SCAN_R + 1 && (k.1 - player.1).abs() <= SCAN_R + 1);
    }

    /// Rescan one chunk. Lava only counts where it meets air above (a halo on a buried lava cell would shine through the ground).
    pub fn update(&mut self, chunks: &ChunkManager, player: (i32, i32)) {
        let key = self.next_key(player);
        let mut found = Vec::new();
        chunks.find_blocks(key.0, key.1, &SCAN_IDS, |x, y, z, id| {
            if !matches!(id, 10 | 11) || chunks.block_loaded(x, y + 1, z) == Some(0) {
                found.push([x, y, z, id as i32]);
            }
        });
        self.absorb(key, player, found.into_iter());
    }

    fn cells(&self) -> impl Iterator<Item = [i32; 4]> + '_ {
        self.cache.values().flatten().copied()
    }
}

/// The halo of an emitter: (colour, half size in blocks, strength, height of its centre above the cell's floor).
fn halo(id: u8) -> ([f32; 3], f32, f32, f32) {
    match id {
        10 | 11 => ([1.0, 0.45, 0.12], 1.7, 0.42, 1.05),
        50 => ([1.0, 0.62, 0.22], 1.0, 0.50, 0.75),
        _ => ([1.0, 0.80, 0.42], 1.5, 0.46, 0.5), // glowstone
    }
}

/// The halos of the emitters in `scan` nearest to `eye`, at most `MAX_GLOW`, faded out with distance. Each is pulled half a block
/// towards the eye so a wall behind a torch does not cut it in half.
pub fn halos(scan: &Scan, eye: Vec3, out: &mut Vec<Quad>) {
    let mut near: Vec<(f32, [i32; 4])> = scan.cells().filter(|c| !is_flower(c[3])).map(|c| ((Vec3::new(c[0] as f32 + 0.5, c[1] as f32 + 0.5, c[2] as f32 + 0.5) - eye).length_squared(), c)).filter(|(d, _)| *d < GLOW_FADE.1 * GLOW_FADE.1).collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (d2, c) in near.into_iter().take(MAX_GLOW) {
        let (rgb, half, a, up) = halo(c[3] as u8);
        let centre = Vec3::new(c[0] as f32 + 0.5, c[1] as f32 + up, c[2] as f32 + 0.5);
        let to_eye = eye - centre;
        let d = d2.sqrt();
        if to_eye.length() < 0.8 {
            continue; // the camera is inside the halo
        }
        let fade = 1.0 - smooth(GLOW_FADE.0, GLOW_FADE.1, d);
        out.push(Quad { pos: centre + to_eye.normalize() * 0.5, half, rgb, a: a * fade, nofog: false });
    }
}

/// The sun's halo: at 92% of the far plane in its direction (so terrain in front still hides it), soft, golden when the sun is low.
pub fn sun_halo(l: &Look, sun: Vec3, eye: Vec3, far: f32, rain: f32) -> Option<Quad> {
    let up = smooth(-0.10, 0.05, sun.y);
    (l.glow && up > 0.0 && rain < 1.0).then(|| {
        let d = far * 0.92;
        Quad { pos: eye + sun * d, half: d * 0.22, rgb: lerp3([1.0, 0.95, 0.80], [1.0, 0.72, 0.35], l.gold), a: 0.30 * up * (1.0 - rain), nofog: true }
    })
}

/// A drifting speck: pollen by day above a flower, a firefly at night above water or a flower.
#[derive(Copy, Clone, Debug)]
struct Mote {
    p: Vec3,
    age: f32,
    life: f32,
    phase: f32,
    fly: bool,
}

#[derive(Default)]
pub struct Motes {
    list: Vec<Mote>,
    seed: u32,
    since: f32,
}

const SPAWN_EVERY: f32 = 0.35;
const MOTE_RANGE: f32 = 14.0;

impl Motes {
    fn rand(&mut self) -> f32 {
        // xorshift32; the seed starts at 0, which would stay 0.
        self.seed = if self.seed == 0 { 0x9E37_79B9 } else { self.seed };
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed >> 8) as f32 / (1u32 << 24) as f32
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// Move the motes `dt` seconds, retire the old and far ones and, every `SPAWN_EVERY`, start one if under `cap`. `night` 0..1;
    /// `surface(x, y, z)`: is that cell water with air above it (the firefly's place).
    pub fn update(&mut self, dt: f32, eye: Vec3, cap: usize, night: f32, scan: &Scan, surface: &mut impl FnMut(i32, i32, i32) -> bool) {
        for m in &mut self.list {
            m.age += dt;
            let t = m.age + m.phase;
            let v = if m.fly { Vec3::new((t * 0.9).sin() * 0.35, (t * 1.7).sin() * 0.15, (t * 0.7).cos() * 0.35) } else { Vec3::new((t * 0.7).sin() * 0.15, 0.12, (t * 0.5).cos() * 0.15) };
            m.p += v * dt;
        }
        self.list.retain(|m| m.age < m.life && (m.p - eye).length_squared() < (MOTE_RANGE + 8.0).powi(2));
        self.list.truncate(cap);
        self.since += dt;
        if cap == 0 || self.list.len() >= cap || self.since < SPAWN_EVERY {
            return;
        }
        self.since = 0.0;
        let fly = night > 0.5;
        let phase = self.rand() * 6.28;
        let spawn = if !fly || self.rand() < 0.5 {
            // Above a flower within range.
            let near: Vec<[i32; 4]> = scan.cells().filter(|c| is_flower(c[3]) && (Vec3::new(c[0] as f32, c[1] as f32, c[2] as f32) - eye).length_squared() < MOTE_RANGE * MOTE_RANGE).collect();
            (!near.is_empty()).then(|| near[(self.rand() * near.len() as f32) as usize % near.len()]).map(|c| Vec3::new(c[0] as f32 + 0.2 + 0.6 * self.rand(), c[1] as f32 + 0.5 + 0.6 * self.rand(), c[2] as f32 + 0.2 + 0.6 * self.rand()))
        } else {
            // Above water: a few random probes round the player.
            (0..6).find_map(|_| {
                let (dx, dz, dy) = ((self.rand() - 0.5) * 2.0 * MOTE_RANGE, (self.rand() - 0.5) * 2.0 * MOTE_RANGE, (self.rand() - 0.5) * 6.0);
                let c = (eye + Vec3::new(dx, dy, dz)).floor();
                surface(c.x as i32, c.y as i32, c.z as i32).then(|| c + Vec3::new(0.5, 1.2 + 1.2 * self.rand(), 0.5))
            })
        };
        if let Some(p) = spawn {
            let life = 5.0 + 4.0 * self.rand();
            self.list.push(Mote { p, age: 0.0, life, phase, fly });
        }
    }

    pub fn quads(&self, out: &mut Vec<Quad>) {
        for m in &self.list {
            let env = m.age.min(m.life - m.age).clamp(0.0, 1.0);
            let q = if m.fly {
                let pulse = 0.35 + 0.65 * (0.5 + 0.5 * (m.age * 4.0 + m.phase).sin());
                Quad { pos: m.p, half: 0.07, rgb: [0.70, 1.0, 0.30], a: 0.9 * env * pulse, nofog: false }
            } else {
                Quad { pos: m.p, half: 0.045, rgb: [1.0, 0.92, 0.55], a: 0.7 * env, nofog: false }
            };
            out.push(q);
        }
    }
}

/// `quads` as float vertices of the entity layout (`mesh::VF`: position, uv, light, lm, tint): the corner (-1..1) is the uv, the light slot
/// carries the strength (+ 10 for `nofog`), the tint the packed colour. `right` / `up` are the camera's axes.
pub fn vertices(quads: &[Quad], right: Vec3, up: Vec3) -> Vec<f32> {
    let mut v = Vec::with_capacity(quads.len().min(MAX_QUADS) * 4 * VF);
    for q in quads.iter().take(MAX_QUADS) {
        let tint = ((q.rgb[0].clamp(0.0, 1.0) * 255.0) as u32) << 16 | ((q.rgb[1].clamp(0.0, 1.0) * 255.0) as u32) << 8 | (q.rgb[2].clamp(0.0, 1.0) * 255.0) as u32;
        for (cu, cv) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let p = q.pos + (right * cu + up * cv) * q.half;
            v.extend_from_slice(&[p.x, p.y, p.z, cu, cv, q.a + if q.nofog { 10.0 } else { 0.0 }, 0.0, tint as f32]);
        }
    }
    v
}

/// The camera's right and up axes in the world, from a view matrix (the rows of its rotation).
pub fn axes(view: &Mat4) -> (Vec3, Vec3) {
    (Vec3::new(view.x_axis.x, view.y_axis.x, view.z_axis.x), Vec3::new(view.x_axis.y, view.y_axis.y, view.z_axis.y))
}

/// The sprite buffers: `MAX_QUADS` quads of the entity vertex, and their (fixed) indices.
pub struct GlowMesh {
    pub vbuf: Buf,
    pub ibuf: Buf,
}

impl GlowMesh {
    pub fn new(gfx: Gfx) -> Self {
        let idx: Vec<u32> = (0..MAX_QUADS as u32).flat_map(|i| [0, 1, 2, 2, 3, 0].map(|k| i * 4 + k)).collect();
        Self { vbuf: gfx.writable((MAX_QUADS * 4 * VF * 4) as u64, false), ibuf: gfx.index(bytemuck::cast_slice(&idx)) }
    }

    /// Upload `verts` (from `vertices`); returns the index count to draw.
    pub fn update(&self, gfx: Gfx, verts: &[f32]) -> u32 {
        if verts.is_empty() {
            return 0;
        }
        gfx.write(&self.vbuf, 0, bytemuck::cast_slice(verts));
        (verts.len() / (4 * VF) * 6) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(sun_y: f32) -> Env {
        Env { sun: Vec3::new((1.0 - sun_y * sun_y).sqrt(), sun_y, 0.0), rain: 0.0, seconds: 3.0, nether: false, underwater: false }
    }

    /// Options.txt round trip, a bad line is skipped and the dial stays inside 0..1.
    #[test]
    fn config_round_trips_and_stays_bounded() {
        let mut c = LookCfg { vibrance: 0.8, warm: false, water: true, glow: false, motion: 2, haze: false };
        let mut t = String::new();
        c.save_into(&mut t);
        let mut back = LookCfg::default();
        t.lines().filter_map(|l| l.split_once(':')).for_each(|(k, v)| back.load(k, v));
        assert_eq!(back, c);
        c.load("look_vibrance", "7");
        c.load("look_motion", "9");
        c.load("look_vibrance", "nope");
        assert!(c.vibrance == 1.0 && c.motion == 2);
    }

    /// Dial 0 is no grade at all; every effect off is `Look::OFF` apart from the clock; the default is a light touch.
    #[test]
    fn off_changes_nothing_and_the_default_is_conservative() {
        let off = LookCfg { vibrance: 0.0, warm: false, water: false, glow: false, motion: 0, haze: false };
        let l = Look::new(&off, &env(0.05));
        assert_eq!((l.grade, l.warm, l.cool, l.sun, l.rays, l.motes), (Look::OFF.grade, Look::OFF.warm, Look::OFF.cool, Look::OFF.sun, false, 0));
        assert_eq!((l.fx[1], l.fx[2]), (0.0, 0.0));
        let d = Look::new(&LookCfg::default(), &env(0.9));
        assert!(d.grade[0] > 0.0 && d.grade[0] <= 0.2 && d.grade[1] <= 0.12 && d.grade[2] <= 0.02, "{:?}", d.grade);
        let top = Look::new(&LookCfg { vibrance: 1.0, ..LookCfg::default() }, &env(0.9));
        assert!(top.grade[0] <= GRADE_VIB && top.grade[1] <= GRADE_CURVE && top.grade[2] <= GRADE_SPLIT);
    }

    /// The light is golden with the sun low and plain at noon; shade is bluer than light; rain and the Nether take it away.
    #[test]
    fn light_is_golden_at_a_low_sun_only() {
        let c = LookCfg::default();
        let (low, noon) = (Look::new(&c, &env(0.05)), Look::new(&c, &env(0.95)));
        assert!(low.gold > 0.95 && noon.gold == 0.0);
        assert!(low.warm[0] > low.warm[2] + 0.25 && (noon.warm[0] - noon.warm[2]).abs() < 0.05);
        assert!(low.cool[2] > low.cool[0] && noon.cool[2] > noon.cool[0]);
        let mut wet = env(0.05);
        wet.rain = 1.0;
        assert_eq!(Look::new(&c, &wet).gold, 0.0);
        let mut hell = env(0.05);
        hell.nether = true;
        let n = Look::new(&c, &hell);
        assert_eq!((n.warm, n.cool, n.rays, n.glow), (Look::OFF.warm, Look::OFF.cool, false, false));
    }

    /// The glint needs the sun (or moon) above the horizon and dry weather; under water nothing but the grade is left.
    #[test]
    fn glint_and_underwater() {
        let c = LookCfg::default();
        assert!(Look::new(&c, &env(0.6)).sun[3] > 0.5 && Look::new(&c, &env(-0.6)).sun[3] < 0.3 && Look::new(&c, &env(-0.6)).sun[1] > 0.0);
        assert_eq!(Look::new(&c, &env(-0.01)).sun[3], 0.0);
        let mut w = env(0.3);
        w.underwater = true;
        let u = Look::new(&c, &w);
        assert_eq!((u.warm, u.cool[3], u.sun[3], u.fx[1], u.glow, u.motes), (Look::OFF.warm, 0.0, 0.0, 0.0, false, 0));
        assert!(u.grade[0] > 0.0);
    }

    /// A lake of lava is one halo per 4 blocks, a chunk keeps at most `PER_CHUNK` of each kind, and chunks out of reach are dropped.
    #[test]
    fn scan_dedupes_lava_and_caps_per_chunk() {
        let mut s = Scan::default();
        let lava = (0..8).flat_map(|x| (0..8).map(move |z| [x, 10, z, 11]));
        let torches = (0..40).map(|i| [i % 16, 20, i / 16, 50]);
        s.absorb((0, 0), (0, 0), lava.chain(torches));
        let c = &s.cache[&(0, 0)];
        let (l, t) = (c.iter().filter(|c| c[3] == 11).count(), c.iter().filter(|c| c[3] == 50).count());
        let lava: Vec<_> = c.iter().filter(|c| c[3] == 11).collect();
        assert!(l >= 1 && l < 16 && t + l <= PER_CHUNK, "{l} lava {t} torches");
        assert!(lava.iter().enumerate().all(|(i, a)| lava[i + 1..].iter().all(|b| (a[0] - b[0]).abs() + (a[1] - b[1]).abs() + (a[2] - b[2]).abs() >= 4)));
        s.absorb((9, 9), (9, 9), std::iter::empty()); // the player moved far away: (0, 0) goes
        assert!(s.cache.is_empty());
        // round robin visits every chunk of the square before repeating
        let seen: std::collections::HashSet<_> = (0..25).map(|_| s.next_key((5, 5))).collect();
        assert_eq!(seen.len(), 25);
    }

    /// Halos are the nearest `MAX_GLOW`, faded with distance, none inside the camera; the sun's sits at the far plane and is hidden below the horizon.
    #[test]
    fn halos_are_capped_and_fade() {
        let mut s = Scan::default();
        s.absorb((0, 0), (0, 0), (0..16).map(|i| [i, 70, 3, 89]));
        s.absorb((1, 0), (0, 0), (0..16).map(|i| [16 + i, 70, 3, 50]));
        s.absorb((-1, 0), (0, 0), (0..16).map(|i| [-16 + i, 70, 3, 11]));
        let mut out = Vec::new();
        halos(&s, Vec3::new(8.0, 70.0, 8.0), &mut out);
        assert!(!out.is_empty() && out.len() <= MAX_GLOW);
        let mut far = Vec::new();
        halos(&s, Vec3::new(500.0, 70.0, 8.0), &mut far);
        assert!(far.is_empty(), "nothing within reach");
        let l = Look::new(&LookCfg::default(), &env(0.3));
        let q = sun_halo(&l, Vec3::new(0.0, 0.5, 0.75f32.sqrt()), Vec3::ZERO, 1000.0, 0.0).unwrap();
        assert!((q.pos.length() - 920.0).abs() < 1e-2 && q.nofog && q.a > 0.0);
        assert!(sun_halo(&l, Vec3::new(1.0, -0.4, 0.0), Vec3::ZERO, 1000.0, 0.0).is_none());
        assert!(sun_halo(&Look::OFF, Vec3::Y, Vec3::ZERO, 1000.0, 0.0).is_none());
    }

    /// Motes never exceed the cap, come only from flowers by day, fade in and out, and `cap` 0 clears them.
    #[test]
    fn motes_are_capped_and_local() {
        let mut s = Scan::default();
        s.absorb((0, 0), (0, 0), (0..6).map(|i| [i, 64, 2, 38]));
        let mut m = Motes::default();
        let eye = Vec3::new(3.0, 66.0, 3.0);
        let mut water = |_: i32, _: i32, _: i32| false;
        for _ in 0..400 {
            m.update(0.1, eye, 5, 0.0, &s, &mut water);
            assert!(m.len() <= 5);
        }
        assert!(m.len() > 0, "flowers feed pollen by day");
        let mut q = Vec::new();
        m.quads(&mut q);
        assert!(q.iter().all(|q| (q.pos - eye).length() < MOTE_RANGE + 8.0 && q.a >= 0.0 && q.a <= 0.7 && !q.nofog));
        let mut none = Scan::default();
        none.absorb((0, 0), (0, 0), std::iter::empty());
        let mut dry = Motes::default();
        for _ in 0..100 {
            dry.update(0.1, eye, 5, 1.0, &none, &mut water); // night, no flowers, no water: nothing starts
        }
        assert_eq!(dry.len(), 0);
        let mut shore = |_: i32, _: i32, _: i32| true;
        for _ in 0..100 {
            dry.update(0.1, eye, 5, 1.0, &none, &mut shore);
        }
        assert!(dry.len() > 0, "fireflies over water at night");
        m.update(0.1, eye, 0, 0.0, &s, &mut water);
        assert_eq!(m.len(), 0);
    }

    /// The quad vertices fill the entity layout, carry the strength in the light slot and stay within the buffer.
    #[test]
    fn vertices_fit_the_layout() {
        let q = Quad { pos: Vec3::new(1.0, 2.0, 3.0), half: 0.5, rgb: [1.0, 0.5, 0.0], a: 0.4, nofog: true };
        let v = vertices(&[q, q], Vec3::X, Vec3::Y);
        assert_eq!(v.len(), 2 * 4 * VF);
        assert_eq!((v[0], v[1], v[3], v[4]), (0.5, 1.5, -1.0, -1.0));
        assert!((v[5] - 10.4).abs() < 1e-4 && v[6] == 0.0 && v[7] == (255u32 << 16 | 127 << 8) as f32);
        let many = vec![q; MAX_QUADS + 20];
        assert_eq!(vertices(&many, Vec3::X, Vec3::Y).len(), MAX_QUADS * 4 * VF);
        let (r, u) = axes(&Mat4::IDENTITY);
        assert_eq!((r, u), (Vec3::X, Vec3::Y));
    }
}
