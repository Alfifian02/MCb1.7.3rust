//! Effects of the player and the world, from b1.7.3: the water particles (`Entity.onEntityUpdate` splash, `EntityBubbleFX`,
//! `EntitySplashFX`), the bubbles of a drowning `EntityLiving`, suffocation (`Entity.isEntityInsideOpaqueBlock`), thunderstorms
//! (`World.tick`, `EntityLightningBolt`) and the dungeon `TileEntityMobSpawner`. CPU side only; `render/items.rs` draws the
//! particles and bolts as small flat boxes.
//! UNVERIFIED: `EntityFlameFX` / `EntitySmokeFX` motion is from memory (marked below); the bolt's column is straight (vanilla draws a
//! jagged one from `field_27029_a`); a bolt's strike column and spawner kind are not drawn from the world's `field_9437_g` LCG.
//! ponytail: no sounds (thunder, splash), no fire from a bolt (no fire block yet), no charged creeper / pigman conversion, the
//! spawner's spinning mob cage is not drawn, mobs do not suffocate (only the player).

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
}

pub struct Effects {
    pub rng: JavaRandom,
    pub particles: Vec<Particle>,
    pub bolts: Vec<Bolt>,
    pub spawners: HashMap<Pos, Spawner>,
    /// `World.field_27172_i`: 2 while a bolt strikes, fades one a tick; the sky is pulled towards a flash colour (`sky::lightning_flash`).
    pub flash: i32,
}

impl Effects {
    pub fn new(seed: i64) -> Self {
        Self { rng: JavaRandom::new(seed ^ 0x4658), particles: Vec::new(), bolts: Vec::new(), spawners: HashMap::new(), flash: 0 }
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

    /// `EntityLiving.onEntityUpdate` when the air runs out: 8 bubbles around `pos` (the eye of the player).
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

    /// One 20 Hz tick of the particles and bolts. Returns the positions of the bolts that strike now: the caller hurts what
    /// `struck` says is in their box (`Entity.onStruckByLightning`).
    pub fn tick(&mut self, get: BlockQuery<'_>) -> Vec<Vec3> {
        self.flash = (self.flash - 1).max(0);
        let mut hits = Vec::new();
        for b in &mut self.bolts {
            if b.tick(&mut self.rng) {
                hits.push(b.pos);
                self.flash = 2;
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
            if !loaded((x, y, z)) || (c.player - centre).length() > 16.0 {
                continue;
            }
            let at = Vec3::new(x as f32 + self.rng.next_float(), y as f32 + self.rng.next_float(), z as f32 + self.rng.next_float());
            let (sm, fl) = (Particle::smoke(&mut self.rng, at), Particle::flame(&mut self.rng, at));
            self.add(sm);
            self.add(fl);
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
