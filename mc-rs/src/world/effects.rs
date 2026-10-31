//! Effects of the player and the world, from b1.7.3: the water particles (`Entity.onEntityUpdate` splash, `EntityBubbleFX`,
//! `EntitySplashFX`), the bubbles of a drowning `EntityLiving`, suffocation (`Entity.isEntityInsideOpaqueBlock`), thunderstorms
//! (`World.tick`, `EntityLightningBolt`) and the dungeon `TileEntityMobSpawner`. CPU side only; `render/items.rs` draws the
//! particles and bolts as small flat boxes.
//! UNVERIFIED: `EntityFlameFX` / `EntitySmokeFX` motion is from memory (marked below); the bolt's column is straight (vanilla draws a
//! jagged one from `field_27029_a`); a bolt's strike column and spawner kind are not drawn from the world's `field_9437_g` LCG.
//! ponytail: no splash sound (thunder is heard: `Effects::clap` / `heard`), no fire from a bolt (no fire block yet), no charged creeper / pigman conversion,
//! mobs do not suffocate (only the player). The spawner's spinning mob cage is drawn by `render/items.rs` from `Spawner::spin`.

use std::collections::HashMap;

use glam::Vec3;

use crate::world::chest::Pos;
use crate::world::chunk::{is_plant, normal_cube};
use crate::world::gen::hydro;
use crate::world::gen::noise::JavaRandom;
use crate::world::mobs::{Ctx, Kind, Mob, Mobs, MAX_MOBS};
use crate::world::physics::BlockQuery;

pub const MAX_PARTICLES: usize = 256;
pub const MAX_BOLTS: usize = 4;
/// Blocks above the strike the bolt's light shines from (`Effects::light`).
const BOLT_LIGHT_Y: f32 = 6.0;
/// Sound covers about 17 blocks a tick (340 m/s at 20 ticks/s): a far bolt is seen first and heard later.
const SOUND_PER_TICK: f32 = 17.0;
/// How much of a sound outdoors gets through `roof` blocks of cover (the blocks from the head up to the top of the column, <= 0: open):
/// half under a roof, down to 0.3 deep under the ground (thunder is still felt there, as in the game).
pub fn muffle(roof: f32) -> f32 {
    if roof <= 0.0 { 1.0 } else { (0.55 - roof * 0.02).max(0.3) }
}

/// Thunderclaps on their way at once.
const MAX_CLAPS: usize = 8;

/// A thunderclap whose sound has not reached the player yet.
pub struct Clap {
    pub at: Vec3,
    /// Blocks from the player when it struck.
    pub dist: f32,
    wait: i32,
}
/// `Block.mobSpawner`.
pub const SPAWNER_ID: u8 = 52;

fn cell(p: Vec3) -> (i32, i32, i32) {
    (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32)
}

fn solid(get: BlockQuery<'_>, c: (i32, i32, i32)) -> bool {
    get(c.0, c.1, c.2).is_some_and(|b| b > 0 && !(8..=11).contains(&b) && !is_plant(b))
}

/// `(nextFloat * 2 - 1) * k`.
fn jitter(r: &mut JavaRandom, k: f32) -> f32 {
    (r.next_float() * 2.0 - 1.0) * k
}

/// `nextFloat - nextFloat`.
fn diff(r: &mut JavaRandom) -> f32 {
    r.next_float() - r.next_float()
}

/// `(int)(8 / (random * 0.8 + 0.2))`: the age of the water particles.
fn life(r: &mut JavaRandom) -> u16 {
    (8.0 / (r.next_double() * 0.8 + 0.2)) as u16
}

/// `EntityFX.particleScale`: `(nextFloat * 0.5 + 0.5) * 2`.
fn scale(r: &mut JavaRandom) -> f32 {
    (r.next_float() * 0.5 + 0.5) * 2.0
}

/// The motion `EntityFX`'s constructor gives (without its `+ 0.1` on y): `v` plus a random 0.4, rescaled to a speed of
/// `(random + random + 1) * 0.15 * 0.4`.
fn base_motion(r: &mut JavaRandom, v: Vec3) -> Vec3 {
    let m = v + Vec3::new(jitter(r, 0.4), jitter(r, 0.4), jitter(r, 0.4));
    let k = (r.next_double() + r.next_double() + 1.0) as f32 * 0.15;
    m / m.length().max(1e-6) * k * 0.4
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fx {
    Bubble,
    Splash,
    Flame,
    Smoke,
    /// `EntityDiggingFX`: a piece of a broken block, drawn with this tile of the block atlas.
    Dig(u16),
    /// `EntityHeartFX`: shown when a wolf is tamed.
    Heart,
    /// Spray where a stream steps down.
    Spray,
    /// A leaf or a twig adrift on a stream's current.
    Leaf,
    Twig,
    /// Cortes Combat C7 (NOT b1.7.3): a speck of a weapon's slash (white) and of a full charge's burst (gold).
    Slash,
    Crit,
}

#[derive(Clone)]
pub struct Particle {
    pub kind: Fx,
    pub pos: Vec3,
    pub prev: Vec3,
    /// Blocks per tick.
    vel: Vec3,
    /// Ticks left (`particleMaxAge` counted down).
    max: u16,
    /// Half the quad size (`0.1 * particleScale`), drawn as a cube this big.
    pub half: f32,
}

impl Particle {
    /// `EntityBubbleFX`.
    fn bubble(r: &mut JavaRandom, pos: Vec3, v: Vec3) -> Self {
        let s = scale(r) * (r.next_float() * 0.6 + 0.2);
        let vel = v * 0.2 + Vec3::new(jitter(r, 0.02), jitter(r, 0.02), jitter(r, 0.02));
        Self { kind: Fx::Bubble, pos, prev: pos, vel, max: life(r), half: 0.1 * s }
    }

    /// `EntitySplashFX` over `EntityRainFX`: it flies up and falls with gravity 0.04.
    fn splash(r: &mut JavaRandom, pos: Vec3, v: Vec3) -> Self {
        let m = base_motion(r, v);
        let mut vel = Vec3::new(m.x * 0.3, r.next_float() * 0.2 + 0.1, m.z * 0.3);
        if v.y == 0.0 && (v.x != 0.0 || v.z != 0.0) {
            vel = Vec3::new(v.x, v.y + 0.1, v.z);
        }
        Self { kind: Fx::Splash, pos, prev: pos, vel, max: life(r), half: 0.1 * scale(r) }
    }

    /// Spray: thrown up a little and carried on by the water's motion, falling with gravity like a splash.
    fn spray(r: &mut JavaRandom, pos: Vec3, v: Vec3) -> Self {
        let vel = Vec3::new(v.x * 0.6 + jitter(r, 0.03), r.next_float() * 0.15 + 0.08, v.z * 0.6 + jitter(r, 0.03));
        Self { kind: Fx::Spray, pos, prev: pos, vel, max: life(r), half: 0.1 * scale(r) * 0.7 }
    }

    /// A leaf or a twig on the water: it lives 20 to 40 seconds and goes where the current goes.
    fn drift(r: &mut JavaRandom, kind: Fx, pos: Vec3) -> Self {
        Self { kind, pos, prev: pos, vel: Vec3::ZERO, max: 400 + (r.next_float() * 400.0) as u16, half: if kind == Fx::Leaf { 0.16 } else { 0.12 } }
    }

    /// `EntityDiggingFX` (`EntityFX`'s constructor): thrown out from the cell centre by `v`, plus 0.1 up; half-size `0.1 * scale / 2`,
    /// life `4 / (nextFloat * 0.9 + 0.1)` ticks.
    fn dig(r: &mut JavaRandom, pos: Vec3, v: Vec3, tile: u16) -> Self {
        let vel = base_motion(r, v) + Vec3::Y * 0.1;
        let half = 0.1 * scale(r) * 0.5;
        let max = (4.0 / (r.next_float() * 0.9 + 0.1)) as u16;
        Self { kind: Fx::Dig(tile), pos, prev: pos, vel, max, half }
    }

    /// UNVERIFIED (from memory): `EntityFlameFX` hardly moves (motion * 0.01) and lives a few ticks longer.
    fn flame(r: &mut JavaRandom, pos: Vec3) -> Self {
        let vel = (base_motion(r, Vec3::ZERO) + Vec3::Y * 0.1) * 0.01;
        Self { kind: Fx::Flame, pos, prev: pos, vel, max: life(r) + 4, half: 0.05 * scale(r) }
    }

    /// UNVERIFIED (from memory): `EntitySmokeFX` drifts up (`motionY += 0.004`).
    fn smoke(r: &mut JavaRandom, pos: Vec3) -> Self {
        let vel = base_motion(r, Vec3::ZERO) * 0.1;
        Self { kind: Fx::Smoke, pos, prev: pos, vel, max: life(r) * 2, half: 0.05 * scale(r) }
    }

    /// `EntityHeartFX`: no gravity, up 0.1 a tick, slowing by 0.86; scale `(nextFloat * 0.5 + 0.5) * 2 * 0.75 * 2`, 16 ticks.
    fn heart(r: &mut JavaRandom, pos: Vec3) -> Self {
        let vel = base_motion(r, Vec3::ZERO) * 0.01 + Vec3::Y * 0.1;
        Self { kind: Fx::Heart, pos, prev: pos, vel, max: 16, half: 0.05 * scale(r) * 0.75 * 2.0 }
    }

    /// C7: a speck of a slash (`SlashTiming.STRIP_TICKS` long, nearly still) or of a crit burst (`vel` as given, a few ticks longer, falls a little).
    fn spark(r: &mut JavaRandom, kind: Fx, pos: Vec3, vel: Vec3) -> Self {
        let (max, half) = if kind == Fx::Slash { (crate::world::combat::fx::SLASH_TICKS, 0.1) } else { (8 + r.next_int_bound(8) as u16, 0.05 + r.next_float() * 0.04) };
        Self { kind, pos, prev: pos, vel, max, half }
    }

    /// One 20 Hz `onUpdate`; false when the particle is gone.
    fn tick(&mut self, get: BlockQuery<'_>, r: &mut JavaRandom) -> bool {
        self.prev = self.pos;
        match self.kind {
            Fx::Bubble => {
                self.vel.y += 0.002;
                self.pos += self.vel;
                self.vel *= 0.85;
                if !matches!(get(cell(self.pos).0, cell(self.pos).1, cell(self.pos).2), Some(8 | 9)) {
                    return false;
                }
            }
            Fx::Splash | Fx::Spray | Fx::Dig(_) => {
                self.vel.y -= 0.04;
                let mut ground = false;
                let ny = self.pos.y + self.vel.y;
                if solid(get, cell(Vec3::new(self.pos.x, ny, self.pos.z))) {
                    ground = self.vel.y < 0.0;
                    self.vel.y = 0.0;
                } else {
                    self.pos.y = ny;
                }
                let nxz = Vec3::new(self.pos.x + self.vel.x, self.pos.y, self.pos.z + self.vel.z);
                if !solid(get, cell(nxz)) {
                    (self.pos.x, self.pos.z) = (nxz.x, nxz.z);
                }
                self.vel *= 0.98;
                if ground {
                    if matches!(self.kind, Fx::Splash | Fx::Spray) && r.next_double() < 0.5 {
                        return false;
                    }
                    self.vel.x *= 0.7;
                    self.vel.z *= 0.7;
                }
                // Inside a liquid or solid cell below its surface (`BlockFluid.getPercentAir`; UNVERIFIED: flowing levels read as a source).
                let c = cell(self.pos);
                if matches!(self.kind, Fx::Splash | Fx::Spray) && get(c.0, c.1, c.2).is_some_and(|b| b > 0 && !is_plant(b)) && self.pos.y < (c.1 + 1) as f32 - 1.0 / 9.0 {
                    return false;
                }
            }
            Fx::Leaf | Fx::Twig => {
                // Carried by the stream, at the surface; gone when it leaves the water.
                let Some(c) = hydro::current(self.pos.x, self.pos.z) else { return false };
                self.vel.x += (c[0] / 20.0 - self.vel.x) * 0.15;
                self.vel.z += (c[1] / 20.0 - self.vel.z) * 0.15;
                self.pos.x += self.vel.x;
                self.pos.z += self.vel.z;
                let at = cell(self.pos - Vec3::new(0.0, 0.2, 0.0));
                if !matches!(get(at.0, at.1, at.2), Some(8 | 9)) {
                    return false;
                }
            }
            Fx::Flame => {
                self.pos += self.vel;
                self.vel *= 0.96;
            }
            Fx::Heart => {
                self.pos += self.vel;
                self.vel *= 0.86;
            }
            Fx::Smoke => {
                self.vel.y += 0.004;
                self.pos += self.vel;
                self.vel *= 0.96;
            }
            Fx::Slash | Fx::Crit => {
                if self.kind == Fx::Crit {
                    self.vel.y -= 0.015;
                }
                self.pos += self.vel;
                self.vel *= 0.8;
            }
        }
        if self.max == 0 {
            return false;
        }
        self.max -= 1;
        true
    }
}

/// `EntityLightningBolt`.
pub struct Bolt {
    pub pos: Vec3,
    /// `field_27028_b`: 2 when it appears, counts down; flashes while >= 0.
    life: i32,
    /// `field_27030_c`: strikes left after the first.
    strikes: i32,
    dead: bool,
    /// `field_27029_a`: the seed of its jagged shape (not drawn: ponytail).
    pub seed: i64,
}

impl Bolt {
    fn new(pos: Vec3, r: &mut JavaRandom) -> Self {
        let seed = r.next_long();
        Self { pos, life: 2, strikes: r.next_int_bound(3) + 1, dead: false, seed }
    }

    /// `onUpdate`; true while it strikes (shows, flashes the sky and hurts what is in its box).
    fn tick(&mut self, r: &mut JavaRandom) -> bool {
        self.life -= 1;
        if self.life < 0 {
            if self.strikes == 0 {
                self.dead = true;
            } else if self.life < -r.next_int_bound(10) {
                self.strikes -= 1;
                self.life = 1;
                self.seed = r.next_long();
            }
        }
        self.life >= 0
    }

    pub fn visible(&self) -> bool {
        self.life >= 0
    }
}

/// Is a box (`centre`, `half`) inside the bolt's box, `(x - 3, y - 3, z - 3)..(x + 3, y + 9, z + 3)` (`onUpdate`)?
pub fn struck(bolt: Vec3, centre: Vec3, half: Vec3) -> bool {
    let (lo, hi) = (bolt - Vec3::splat(3.0), bolt + Vec3::new(3.0, 9.0, 3.0));
    (centre - half).cmplt(hi).all() && (centre + half).cmpgt(lo).all()
}

/// The dungeon spawner's mob (`WorldGenDungeons.pickMobSpawner`: 0 skeleton, 1 and 2 zombie, 3 spider).
pub fn dungeon_kind(i: u8) -> Kind {
    match i {
        0 => Kind::Skeleton,
        3 => Kind::Spider,
        _ => Kind::Zombie,
    }
}

/// `TileEntityMobSpawner`: `kind` is the index `pickMobSpawner` drew, `delay` the ticks to the next batch (starts at 20).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Spawner {
    pub kind: u8,
    pub delay: i32,
    /// `TileEntityMobSpawner.yaw` / `yaw2`: the cage's turn in 10ths of a degree, now and a tick ago (never saved, as in the Java).
    pub yaw: f64,
    pub yaw2: f64,
}

impl Spawner {
    pub fn new(kind: u8, delay: i32) -> Self {
        Self { kind, delay, yaw: 0.0, yaw2: 0.0 }
    }

    /// The part of `updateEntity` that turns the cage while the player is near: `yaw` gains `1000 / (delay + 200)` and wraps past 360,
    /// taking `yaw2` (the value a tick ago) with it so the interpolation never jumps. Call `yaw2 = yaw` first, every tick.
    fn spin_tick(&mut self) {
        self.yaw += (1000.0 / (self.delay as f32 + 200.0)) as f64;
        while self.yaw > 360.0 {
            self.yaw -= 360.0;
            self.yaw2 -= 360.0;
        }
    }

    /// `TileEntityMobSpawnerRenderer`: the cage's turn in degrees `partial` of the way from last tick to this one (`(yaw2 + (yaw - yaw2) * partial) * 10`).
    pub fn spin(&self, partial: f32) -> f32 {
        ((self.yaw2 + (self.yaw - self.yaw2) * partial as f64) * 10.0) as f32
    }
}

pub struct Effects {
    pub rng: JavaRandom,
    pub particles: Vec<Particle>,
    pub bolts: Vec<Bolt>,
    pub spawners: HashMap<Pos, Spawner>,
    /// `World.field_27172_i`: 2 while a bolt strikes, fades one a tick; the sky is pulled towards a flash colour (`sky::lightning_flash`).
    pub flash: i32,
    /// Where the last bolt struck: the light that flash casts on the terrain comes from here (`light`), still while the flash fades after the bolt is gone.
    pub struck_at: Vec3,
    pub claps: Vec<Clap>,
    /// Ticks until the next rain clip.
    rain_wait: i32,
}

impl Effects {
    pub fn new(seed: i64) -> Self {
        Self { rng: JavaRandom::new(seed ^ 0x4658), particles: Vec::new(), bolts: Vec::new(), spawners: HashMap::new(), flash: 0, struck_at: Vec3::ZERO, claps: Vec::new(), rain_wait: 0 }
    }

    /// A bolt struck at `bolt` and the player's ears are at `ears`: queue its thunder, late by the sound's travel time.
    pub fn clap(&mut self, bolt: Vec3, ears: Vec3) {
        let dist = (bolt - ears).length();
        if self.claps.len() < MAX_CLAPS {
            self.claps.push(Clap { at: bolt, dist, wait: (dist / SOUND_PER_TICK) as i32 });
        }
    }

    /// One 20 Hz tick of the rain's sound: `Some((volume, pitch))` when a patter clip is due. `rain` is the rain strength, `roof` the blocks
    /// from the head up to the top of the column (<= 0: open sky). Open: a bright patter; under a roof: the same, quieter and lower, as on a
    /// roof; more than 12 blocks under the surface: silence. A clip every 8 to 15 ticks, so a few overlap (the mixer has 32 voices).
    pub fn patter(&mut self, rain: f32, roof: f32) -> Option<(f32, f32)> {
        self.rain_wait -= 1;
        if rain <= 0.2 || roof > 12.0 || self.rain_wait > 0 {
            return None;
        }
        self.rain_wait = 8 + self.rng.next_int_bound(8);
        let r = self.rng.next_float();
        Some(if roof <= 0.0 { (0.22 * rain, 0.9 + 0.2 * r) } else { (0.08 * rain, 0.5 + 0.1 * r) })
    }

    /// One 20 Hz tick: the claps whose sound arrives now (the caller plays them).
    pub fn heard(&mut self) -> Vec<Clap> {
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.claps).into_iter().partition(|c| c.wait <= 0);
        self.claps = later.into_iter().map(|c| Clap { wait: c.wait - 1, ..c }).collect();
        due
    }

    fn add(&mut self, p: Particle) {
        if self.particles.len() < MAX_PARTICLES {
            self.particles.push(p);
        }
    }

    /// `Entity.onEntityUpdate` on entering water: `1 + width * 20` bubbles, then as many splashes, at the water surface
    /// (`pos.y` = floor(box bottom) + 1) within `width` of the axis. `v` = the entity's motion per tick.
    /// A puff of smoke at a point (the furnace cart's `largesmoke`; the same sprite as any smoke).
    pub fn puff(&mut self, pos: Vec3) {
        let p = Particle::smoke(&mut self.rng, pos);
        self.particles.push(p);
    }

    pub fn splash(&mut self, pos: Vec3, width: f32, v: Vec3) {
        let n = {
            let mut c = 0;
            while (c as f32) < 1.0 + width * 20.0 {
                c += 1;
            }
            c
        };
        for kind in [Fx::Bubble, Fx::Splash] {
            for _ in 0..n {
                let (dx, dz) = (jitter(&mut self.rng, width), jitter(&mut self.rng, width));
                let at = Vec3::new(pos.x + dx, pos.y, pos.z + dz);
                let p = if kind == Fx::Bubble {
                    let dv = self.rng.next_float() * 0.2;
                    Particle::bubble(&mut self.rng, at, Vec3::new(v.x, v.y - dv, v.z))
                } else {
                    Particle::splash(&mut self.rng, at, v)
                };
                self.add(p);
            }
        }
    }

    /// Cortes Combat C7 (NOT b1.7.3, UNVERIFIED): a slash or a sweep's mark, one speck on each of `points` (`combat::fx::{slash,sweep}_points`).
    /// ponytail: capped at 3000 live particles like `dig`; the mod's 5-frame sprite strip is not drawn.
    pub fn slash(&mut self, points: &[Vec3]) {
        if self.particles.len() > 3000 {
            return;
        }
        for &p in points {
            let v = Vec3::new(jitter(&mut self.rng, 0.01), 0.0, jitter(&mut self.rng, 0.01));
            let s = Particle::spark(&mut self.rng, Fx::Slash, p, v);
            self.particles.push(s);
        }
    }

    /// C7 (`SlashParticles.burst`): 30 gold specks over a mob's box, thrown a little outward. `half` = half the box's size.
    pub fn crit(&mut self, centre: Vec3, half: Vec3) {
        if self.particles.len() > 3000 {
            return;
        }
        let spread = half.max(Vec3::new(0.2, 0.3, 0.2));
        for _ in 0..crate::world::combat::fx::CRIT_SPECKS {
            let o = Vec3::new(jitter(&mut self.rng, spread.x), jitter(&mut self.rng, spread.y * 0.7), jitter(&mut self.rng, spread.z));
            let s = Particle::spark(&mut self.rng, Fx::Crit, centre + o, o.normalize_or_zero() * 0.12 + Vec3::Y * 0.04);
            self.particles.push(s);
        }
    }

    /// `EntityLiving.onEntityUpdate` when the air runs out: 8 bubbles around `pos` (the eye of the player).
    /// Retro Sprint's `EntityMixin.addParticles` (its `BlockDustParticle`, a `tilecrack_` piece): one speck of the block under the feet per tick
    /// while sprinting, at the feet within the box's width, thrown against the run (`v` = the player's motion in blocks/tick: `-v * 4`, 1.5 up).
    pub fn sprint_dust(&mut self, feet: Vec3, v: Vec3, tile: u16) {
        if self.particles.len() > 3000 {
            return;
        }
        let at = feet + Vec3::new((self.rng.next_float() - 0.5) * 0.6, 0.1, (self.rng.next_float() - 0.5) * 0.6);
        let p = Particle::dig(&mut self.rng, at, Vec3::new(-v.x * 4.0, 1.5, -v.z * 4.0), tile);
        self.particles.push(p);
    }

    /// `EffectRenderer.addBlockDestroyEffects`: 4 x 4 x 4 pieces, each flung away from the cell centre. ponytail: capped at 3000
    /// live particles; the whole block tile is drawn (vanilla crops a random 4 x 4 pixel patch), untinted, not lit by the cell.
    pub fn dig(&mut self, (x, y, z): (i32, i32, i32), tile: u16) {
        if self.particles.len() > 3000 {
            return;
        }
        for i in 0..4 {
            for j in 0..4 {
                for k in 0..4 {
                    let v = Vec3::new((i as f32 + 0.5) / 4.0 - 0.5, (j as f32 + 0.5) / 4.0 - 0.5, (k as f32 + 0.5) / 4.0 - 0.5);
                    let pos = Vec3::new(x as f32, y as f32, z as f32) + v + Vec3::splat(0.5);
                    self.particles.push(Particle::dig(&mut self.rng, pos, v, tile));
                }
            }
        }
    }

    /// `EntityWolf.showHeartsOrSmokeFX`: 7 hearts (tamed) or 7 smoke puffs over the wolf's box. `feet` is its `posY`.
    pub fn hearts(&mut self, feet: Vec3, width: f32, height: f32, tamed: bool) {
        for _ in 0..7 {
            let (a, b, c) = (self.rng.next_float(), self.rng.next_float(), self.rng.next_float());
            let pos = Vec3::new(feet.x + a * width * 2.0 - width, feet.y + 0.5 + b * height, feet.z + c * width * 2.0 - width);
            let p = if tamed { Particle::heart(&mut self.rng, pos) } else { Particle::smoke(&mut self.rng, pos) };
            self.particles.push(p);
        }
    }

    pub fn breath(&mut self, pos: Vec3, v: Vec3) {
        for _ in 0..8 {
            let o = Vec3::new(diff(&mut self.rng), diff(&mut self.rng), diff(&mut self.rng));
            let p = Particle::bubble(&mut self.rng, pos + o, v);
            self.add(p);
        }
    }

    /// 20 smoke + 20 flame puffs around a spawner (`TileEntityMobSpawner`, after it spawned).
    fn burst(&mut self, (x, y, z): Pos) {
        for _ in 0..20 {
            let at = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5) + Vec3::new(diff(&mut self.rng), diff(&mut self.rng), diff(&mut self.rng));
            let (s, f) = (Particle::smoke(&mut self.rng, at), Particle::flame(&mut self.rng, at));
            self.add(s);
            self.add(f);
        }
    }

    /// `World.tick`'s lightning roll: each chunk in the 19 x 19 square round the player's chunk rolls 1 in 100000 (the caller only
    /// calls this while `thunder > 0.9` and `rain > 0.2`). Returns the columns picked; the caller keeps those that are rained on
    /// and `strike`s at their top (`canBlockBeRainedOn`). UNVERIFIED: the column is drawn from this RNG, the Java uses `field_9437_g`.
    pub fn storm(&mut self, (cx, cz): (i32, i32)) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        for dx in -9..=9 {
            for dz in -9..=9 {
                if self.rng.next_int_bound(100000) == 0 {
                    out.push(((cx + dx) * 16 + self.rng.next_int_bound(16), (cz + dz) * 16 + self.rng.next_int_bound(16)));
                }
            }
        }
        out
    }

    pub fn strike(&mut self, pos: Vec3) {
        if self.bolts.len() < MAX_BOLTS {
            let b = Bolt::new(pos, &mut self.rng);
            self.bolts.push(b);
        }
    }

    /// The life of the streams round `eye` (the player): leaves and twigs adrift on the current, and spray where the water steps
    /// down. Three random points within 24 blocks a tick; nothing where there is no stream. ponytail: the debris is a small flat
    /// colour box like every particle here (no texture, no model that lies on the water), at most 48 at a time.
    pub fn river_life(&mut self, eye: Vec3, get: BlockQuery<'_>) {
        for _ in 0..3 {
            let (x, z) = (eye.x + (self.rng.next_float() - 0.5) * 48.0, eye.z + (self.rng.next_float() - 0.5) * 48.0);
            let Some(c) = hydro::current(x, z) else { continue };
            let Some(top) = water_top(get, x, z, eye.y + 12.0) else { continue };
            let len = c[0].hypot(c[1]).max(1e-3);
            let (nx, nz) = (x + c[0] / len * 1.5, z + c[1] / len * 1.5);
            let step = water_top(get, nx, nz, top as f32 + 1.0).is_some_and(|n| n < top);
            if step {
                for _ in 0..3 {
                    let p = Particle::spray(&mut self.rng, Vec3::new(nx, top as f32 + 0.9, nz), Vec3::new(c[0] / 20.0, 0.0, c[1] / 20.0));
                    self.add(p);
                }
            } else if self.rng.next_float() < 0.04 && self.particles.iter().filter(|p| matches!(p.kind, Fx::Leaf | Fx::Twig)).count() < 48 {
                let kind = if self.rng.next_float() < 0.7 { Fx::Leaf } else { Fx::Twig };
                let p = Particle::drift(&mut self.rng, kind, Vec3::new(x, top as f32 + 0.9, z));
                self.add(p);
            }
        }
    }

    /// The bolt's light on the terrain, for the chunk shaders (`render::look::Look::bolt`): xyz = where it shines from (a few blocks above the
    /// strike, so the ground round it and the trunks and walls above it are lit), w = how bright now, 1 at the strike fading to 0 over the two ticks
    /// of `flash` (the same fade as the sky's, `sky::lightning_flash`); 0 = no flash, the shaders skip it.
    /// ponytail: one light, the last bolt to strike; two bolts at once light the terrain round the later one only.
    pub fn light(&self, partial: f32) -> [f32; 4] {
        let k = if self.flash > 0 { (self.flash as f32 - partial).clamp(0.0, 1.0) } else { 0.0 };
        (self.struck_at + Vec3::Y * BOLT_LIGHT_Y).extend(k).to_array()
    }

    /// One 20 Hz tick of the particles and bolts. Returns the positions of the bolts that strike now: the caller hurts what
    /// `struck` says is in their box (`Entity.onStruckByLightning`).
    pub fn tick(&mut self, get: BlockQuery<'_>) -> Vec<Vec3> {
        self.flash = (self.flash - 1).max(0);
        let mut hits = Vec::new();
        for b in &mut self.bolts {
            if b.tick(&mut self.rng) {
                hits.push(b.pos);
                self.flash = 2;
                self.struck_at = b.pos;
            }
        }
        self.bolts.retain(|b| !b.dead);
        self.particles.retain_mut(|p| p.tick(get, &mut self.rng));
        hits
    }

    /// `TileEntityMobSpawner.updateEntity` for every spawner in a loaded chunk: only while the player is within 16 blocks it
    /// puffs smoke and flames and counts `delay` down; at 0 it tries 4 times to spawn its mob 0..4 blocks around it (not with 6 or
    /// more of that kind within 8 x 4 x 8), then waits 200..800 ticks.
    pub fn tick_spawners(&mut self, mobs: &mut Mobs, c: &Ctx<'_>, loaded: &dyn Fn(Pos) -> bool) {
        let mut all = std::mem::take(&mut self.spawners);
        for (&(x, y, z), s) in all.iter_mut() {
            let centre = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
            s.yaw2 = s.yaw;
            if !loaded((x, y, z)) || (c.player - centre).length() > 16.0 {
                continue;
            }
            let at = Vec3::new(x as f32 + self.rng.next_float(), y as f32 + self.rng.next_float(), z as f32 + self.rng.next_float());
            let (sm, fl) = (Particle::smoke(&mut self.rng, at), Particle::flame(&mut self.rng, at));
            self.add(sm);
            self.add(fl);
            s.spin_tick();
            if s.delay == -1 {
                s.delay = 200 + self.rng.next_int_bound(600);
            }
            if s.delay > 0 {
                s.delay -= 1;
                continue;
            }
            let kind = dungeon_kind(s.kind);
            for _ in 0..4 {
                let (lo, hi) = (Vec3::new(x as f32 - 8.0, y as f32 - 4.0, z as f32 - 8.0), Vec3::new(x as f32 + 9.0, y as f32 + 5.0, z as f32 + 9.0));
                let near = mobs.list.iter().filter(|m| m.kind == kind && { let (a, b) = m.aabb(); a.cmplt(hi).all() && b.cmpgt(lo).all() }).count();
                if near >= 6 {
                    s.delay = 200 + self.rng.next_int_bound(600);
                    break;
                }
                let mx = x as f32 + (self.rng.next_double() - self.rng.next_double()) as f32 * 4.0;
                let my = y + self.rng.next_int_bound(3) - 1;
                let mz = z as f32 + (self.rng.next_double() - self.rng.next_double()) as f32 * 4.0;
                let yaw = self.rng.next_float() * 360.0;
                if mobs.list.len() < MAX_MOBS && mobs.can_spawn(c, kind, mx.floor() as i32, my, mz.floor() as i32) {
                    let m = Mob::new(kind, mx, my as f32, mz, yaw, &mut mobs.rng);
                    mobs.list.push(m);
                    self.burst((x, y, z));
                    s.delay = 200 + self.rng.next_int_bound(600);
                }
            }
        }
        self.spawners = all;
    }
}

/// `Entity.isEntityInsideOpaqueBlock`: any of the 8 corners of a `width` wide, 0.1 high box round (x, y, z) in a normal cube.
/// Callers pass the head: `EntityPlayer.getEyeHeight` is 0.12 above its eye. An unloaded cell is not a wall.
/// Y of the top water block of column (x, z) at or below `from` (within 40), with air above it.
fn water_top(get: BlockQuery<'_>, x: f32, z: f32, from: f32) -> Option<i32> {
    let (bx, bz, hi) = (x.floor() as i32, z.floor() as i32, from.floor() as i32);
    (hi - 40..=hi).rev().find(|&y| matches!(get(bx, y, bz), Some(8 | 9)) && get(bx, y + 1, bz) == Some(0))
}

pub fn inside_opaque(get: BlockQuery<'_>, x: f32, y: f32, z: f32, width: f32) -> bool {
    (0..8).any(|i| {
        let dx = ((i % 2) as f32 - 0.5) * width * 0.9;
        let dy = (((i >> 1) % 2) as f32 - 0.5) * 0.1;
        let dz = (((i >> 2) % 2) as f32 - 0.5) * width * 0.9;
        let c = cell(Vec3::new(x + dx, y + dy, z + dz));
        get(c.0, c.1, c.2).is_some_and(normal_cube)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cage turns `1000 / (delay + 200)` a tick (a fresh spawner, delay 20: 4.545), and across the wrap past 360 each tick still starts
    /// where the last one ended (the drawn turn differs by whole turns only), so the cage never jumps.
    #[test]
    fn a_spawner_cage_turns_smoothly_across_the_wrap() {
        let mut s = Spawner::new(1, 20);
        let step = 1000.0 / 220.0;
        let mut end = s.spin(1.0);
        for _ in 0..200 {
            s.yaw2 = s.yaw;
            s.spin_tick();
            assert!(s.yaw > 0.0 && s.yaw <= 360.0, "yaw {}", s.yaw);
            assert!((s.spin(1.0) - s.spin(0.0) - step * 10.0).abs() < 0.05, "turn per tick {}", s.spin(1.0) - s.spin(0.0));
            let gap = (s.spin(0.0) - end).rem_euclid(360.0);
            assert!(gap < 0.05 || gap > 359.95, "this tick starts {gap} degrees from where the last ended");
            end = s.spin(1.0);
        }
    }

    /// A splash makes as many splashes as bubbles; bubbles die out of water; a wall at the head is found; a bolt strikes on its
    /// first two ticks and always ends.
    /// A broken block throws 64 pieces that start inside its cell, fly outwards, fall under gravity 0.04, and are all gone within
    /// `4 / 0.1 = 40` ticks.
    #[test]
    fn digging_pieces_fly_out_fall_and_die() {
        let mut fx = Effects::new(3);
        fx.dig((10, 64, 10), 4);
        assert_eq!(fx.particles.len(), 64);
        assert!(fx.particles.iter().all(|p| p.kind == Fx::Dig(4) && (10.0..11.0).contains(&p.pos.x) && (64.0..65.0).contains(&p.pos.y)));
        let top = fx.particles.iter().map(|p| p.pos.y).fold(0.0, f32::max);
        let floor = |_: i32, y: i32, _: i32| Some(if y < 64 { 1u8 } else { 0 });
        for _ in 0..3 {
            fx.tick(&floor);
        }
        assert!(fx.particles.iter().any(|p| p.pos.y > top - 0.5), "they are thrown up and out first");
        assert!(fx.particles.iter().all(|p| p.pos.y >= 64.0 - 1e-3), "none sinks into the floor");
        for _ in 0..60 {
            fx.tick(&floor);
        }
        assert!(fx.particles.is_empty());
    }

    #[test]
    fn effects_follow_java() {
        let mut fx = Effects::new(1);
        fx.splash(Vec3::new(0.5, 64.0, 0.5), 0.6, Vec3::ZERO);
        assert!(fx.particles.len() >= 26 && fx.particles.len() % 2 == 0);
        fx.tick(&|_, _, _| Some(0));
        assert!(fx.particles.iter().all(|p| p.kind != Fx::Bubble));

        let wall = |_: i32, y: i32, _: i32| Some(if y == 65 { 1u8 } else { 0 });
        assert!(inside_opaque(&wall, 0.5, 65.5, 0.5, 0.6));
        assert!(!inside_opaque(&wall, 0.5, 67.5, 0.5, 0.6));

        let mut r = JavaRandom::new(5);
        let mut b = Bolt::new(Vec3::ZERO, &mut r);
        assert!(b.tick(&mut r) && b.tick(&mut r));
        for _ in 0..200 {
            b.tick(&mut r);
        }
        assert!(b.dead);
        assert!(struck(Vec3::ZERO, Vec3::new(1.0, 4.0, 0.0), Vec3::new(0.3, 0.9, 0.3)) && !struck(Vec3::ZERO, Vec3::new(5.0, 0.0, 0.0), Vec3::splat(0.3)));
    }

    /// Cover takes the loudness down in steps and never to nothing.
    #[test]
    fn a_roof_muffles_but_never_silences() {
        assert_eq!(muffle(-2.0), 1.0);
        assert!(muffle(1.0) < 0.6 && muffle(1.0) > muffle(6.0) && muffle(6.0) > muffle(12.0));
        assert_eq!(muffle(60.0), 0.3);
    }

    /// Rain patters only in rain, louder in the open than under a roof, not at all deep underground, and not every tick.
    #[test]
    fn rain_patters_on_a_beat_and_less_under_a_roof() {
        let mut fx = Effects::new(4);
        let count = |fx: &mut Effects, rain: f32, roof: f32| (0..200).filter_map(|_| fx.patter(rain, roof)).collect::<Vec<_>>();
        assert!(count(&mut fx, 0.1, -3.0).is_empty(), "no rain, no sound");
        let open = count(&mut fx, 1.0, -3.0);
        assert!(open.len() >= 200 / 15 - 1 && open.len() <= 200 / 8 + 1, "{} clips in 10 s", open.len());
        let roofed = count(&mut fx, 1.0, 3.0);
        assert!(!roofed.is_empty() && roofed.iter().all(|c| c.0 < 0.1 && c.1 < 0.65) && open.iter().all(|c| c.0 > 0.2 && c.1 > 0.85));
        assert!(count(&mut fx, 1.0, 20.0).is_empty(), "deep underground");
    }

    /// Thunder is heard after the flash by the distance over 17 blocks a tick, and at most `MAX_CLAPS` wait at once.
    #[test]
    fn thunder_arrives_after_the_flash() {
        let mut fx = Effects::new(3);
        fx.clap(Vec3::new(170.0, 0.0, 0.0), Vec3::ZERO); // 10 ticks away
        fx.clap(Vec3::ZERO, Vec3::ZERO);
        let now = fx.heard();
        assert!(now.len() == 1 && now[0].dist == 0.0);
        for _ in 0..9 {
            assert!(fx.heard().is_empty());
        }
        let late = fx.heard();
        assert!(late.len() == 1 && late[0].dist == 170.0);
        for _ in 0..20 {
            fx.clap(Vec3::ZERO, Vec3::ZERO);
        }
        assert_eq!(fx.claps.len(), MAX_CLAPS);
    }

    /// A strike lights the terrain from just above where it hit, at full strength, and the light is gone with the flash two ticks later.
    #[test]
    fn a_strike_lights_the_terrain_and_fades() {
        let mut fx = Effects::new(5);
        assert_eq!(fx.light(0.0)[3], 0.0);
        fx.bolts.push(Bolt::new(Vec3::new(10.0, 64.0, 20.0), &mut JavaRandom::new(1)));
        fx.tick(&|_, _, _| Some(0));
        let l = fx.light(0.0);
        assert_eq!((l[0], l[1], l[2], l[3]), (10.0, 64.0 + BOLT_LIGHT_Y, 20.0, 1.0));
        assert!(fx.light(0.5)[3] > 0.0);
        fx.bolts.clear();
        fx.tick(&|_, _, _| Some(0));
        fx.tick(&|_, _, _| Some(0));
        assert_eq!(fx.light(0.0)[3], 0.0);
    }

    /// Taming throws 7 hearts (or 7 smoke puffs) over the wolf; a heart drifts up and is gone after 16 ticks.
    #[test]
    fn hearts_rise_and_fade() {
        let mut fx = Effects::new(9);
        fx.hearts(Vec3::new(5.0, 10.0, 5.0), 0.8, 0.8, true);
        fx.hearts(Vec3::new(5.0, 10.0, 5.0), 0.8, 0.8, false);
        assert_eq!(fx.particles.iter().filter(|p| p.kind == Fx::Heart).count(), 7);
        assert_eq!(fx.particles.len(), 14);
        let y0: f32 = fx.particles.iter().filter(|p| p.kind == Fx::Heart).map(|p| p.pos.y).sum();
        let air = |_: i32, _: i32, _: i32| Some(0u8);
        fx.tick(&air);
        assert!(fx.particles.iter().filter(|p| p.kind == Fx::Heart).map(|p| p.pos.y).sum::<f32>() > y0);
        for _ in 0..20 {
            fx.tick(&air);
        }
        assert!(fx.particles.iter().all(|p| p.kind != Fx::Heart));
    }
}
