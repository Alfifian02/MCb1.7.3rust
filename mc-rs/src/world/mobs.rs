//! M8 mobs: pig, cow, sheep, chicken, wolf, squid, zombie, zombie pigman, giant, skeleton, creeper, spider, slime.
//! One `Mob` and one AI for all: `EntityCreature.updatePlayerActionState` (pick a cell with `func_31026_E`, walk to it) over
//! `EntityLiving.updatePlayerActionState` (random turning, look at a player within 8, `func_27021_X` despawn), at 20 Hz, with
//! the body moved every frame by `physics::step_box`. The kinds differ by `spec` (size, health, `moveSpeed`, `attackStrength`),
//! `loot` (`dropFewItems`) and the few special cases below: `EntityMob.attackEntity` melee, skeleton arrows (`EntityArrow`),
//! creeper fuse and `Explosion`, spider leap and wall climbing, slime hopping and splitting, squid swimming, chicken egg and
//! slow fall, zombie/skeleton burning in daylight, zombie pigman and wolf anger.
//! ponytail: no A* (`Pathfinder`): a "path" is a straight walk to the chosen cell (or to the player), and a blocked mob hops,
//! so it climbs one-block steps but cannot go around walls. Light is not read by the wander weights. No sounds, no death
//! animation, no fall/lava damage, no riding/taming/shearing/milking, mobs are not saved. Walk speed is the terminal speed
//! of `moveFlying` (`moveSpeed` x 0.1 x 0.98 against 0.546 drag), not accumulated momentum. Explosions have no exposure
//! test, and knock the player back not at all. Normal difficulty. Ghast (needs the Nether) and the pig-zombie spawn are not here.
//! UNVERIFIED: spawn rules and rates (vanilla spawns animals while generating chunks and every tick per chunk for monsters),
//! the arrow gravity 0.03, the model boxes (the spider/wolf/squid ones from memory) and every colour (flat stand-ins).

use glam::Vec3;

use crate::world::chunk::brightness;
use crate::world::craft::{self, Kind as Tool};
use crate::world::dig;
use crate::world::gen::noise::JavaRandom;
use crate::world::items::{Drops, ItemStack};
use crate::world::names::RESIST;
use crate::world::physics::{self, BlockQuery, Player};

/// Render buffer sizes: most mobs and arrows alive at once.
pub const MAX_MOBS: usize = 48;
pub const MAX_ARROWS: usize = 16;
const CAP_ANIMALS: usize = 12;
const CAP_MONSTERS: usize = 10;
const CAP_SQUID: usize = 3;
const TICK: f32 = 0.05;
/// m/s per unit of `moveSpeed`: 0.7 walks at 3 m/s.
const WALK: f32 = 3.0 / 0.7;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind { Pig, Cow, Sheep, Chicken, Wolf, Squid, Zombie, PigZombie, Giant, Skeleton, Creeper, Spider, Slime }
use Kind::*;

/// (width, height, health, moveSpeed, attackStrength, flat colour = wool metadata).
fn spec(k: Kind) -> (f32, f32, i32, f32, i32, u8) {
    match k {
        Pig => (0.9, 0.9, 10, 0.7, 0, 6),
        Cow => (0.9, 1.3, 10, 0.7, 0, 12),
        Sheep => (0.9, 1.3, 10, 0.7, 0, 0),
        Chicken => (0.3, 0.4, 4, 0.7, 0, 0),
        Wolf => (0.8, 0.8, 8, 1.1, 2, 8),
        Squid => (0.95, 0.95, 10, 0.7, 0, 11),
        Zombie => (0.6, 1.8, 20, 0.5, 5, 13),
        PigZombie => (0.6, 1.8, 20, 0.5, 5, 6),
        Giant => (3.6, 10.8, 20, 0.5, 50, 13),
        Skeleton => (0.6, 1.8, 20, 0.7, 2, 8),
        Creeper => (0.6, 1.8, 20, 0.7, 0, 5),
        Spider => (1.4, 0.9, 20, 0.8, 2, 15),
        Slime => (0.6, 0.6, 1, 0.7, 0, 5),
    }
}

/// The flat stand-in colour of a kind (wool metadata), for the renderer.
pub fn colour(k: Kind) -> u8 {
    spec(k).5
}

pub struct Mob {
    pub kind: Kind,
    /// `pos` is the centre of the box.
    pub body: Player,
    /// `rotationYaw`, degrees; the mob moves along (-sin, cos).
    pub yaw: f32,
    pub health: i32,
    /// `field_704_R` / `field_705_Q`: leg swing phase and amount.
    pub limb: f32,
    pub limb_amt: f32,
    /// `hurtTime`: ticks of red flash left.
    pub hurt: u8,
    /// Model scale: slime size 1/2/4, giant 6.
    pub size: f32,
    /// Sheep fleece colour (cloth metadata).
    pub color: u8,
    /// Creeper: ticks of fuse burnt.
    pub fuse: u8,
    pub half: Vec3,
    age: u32,
    yaw_vel: f32,
    look: Option<i32>,
    goal: Option<(f32, f32)>,
    fspeed: f32,
    jumping: bool,
    wet: bool,
    blocked: bool,
    life: u8,
    last: i32,
    target: bool,
    shot: bool,
    lit: bool,
    anger: i32,
    attack_time: u8,
    hop: i32,
    egg: i32,
    fire: u16,
    free: u8,
    swim: Vec3,
}

pub enum Ev {
    /// Damage to the player (before armor).
    Hurt(i32),
    /// A creeper went off here.
    Boom(Vec3),
    Shoot(Vec3, Vec3),
    Egg(Vec3),
}

pub struct Ctx<'a> {
    pub get: BlockQuery<'a>,
    pub light: &'a dyn Fn(i32, i32, i32) -> u8,
    /// `World.isDaytime`.
    pub day: bool,
    /// The player's box centre and eye.
    pub player: Vec3,
    pub eye: Vec3,
}

fn norm(mut a: f32) -> f32 {
    while a < -180.0 { a += 360.0; }
    while a >= 180.0 { a -= 360.0; }
    a
}

fn cell(p: Vec3) -> (i32, i32, i32) {
    (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32)
}

fn solid(b: Option<u8>) -> bool {
    b.map_or(false, |b| b > 0 && !(8..=11).contains(&b))
}

/// `EntityLiving.canEntityBeSeen`: no solid block on the segment.
fn sees(get: BlockQuery<'_>, a: Vec3, b: Vec3) -> bool {
    let d = b - a;
    let n = (d.length() * 2.0) as i32;
    (1..n).all(|i| {
        let (x, y, z) = cell(a + d * (i as f32 / n as f32));
        !solid(get(x, y, z))
    })
}

impl Mob {
    /// `feet` is the cell the mob stands in (its bottom face is the cell's floor).
    pub fn new(kind: Kind, x: f32, feet: f32, z: f32, yaw: f32, rng: &mut JavaRandom) -> Self {
        let (mut w, mut h, mut hp, ..) = spec(kind);
        let size = match kind {
            Slime => (1 << rng.next_int_bound(3)) as f32,
            Giant => 6.0,
            _ => 1.0,
        };
        if kind == Slime {
            (w, h, hp) = (0.6 * size, 0.6 * size, (size * size) as i32);
        }
        let half = Vec3::new(w / 2.0, h / 2.0, w / 2.0);
        let color = if kind == Sheep {
            let r = rng.next_int_bound(100);
            if r < 5 { 15 } else if r < 10 { 7 } else if r < 15 { 8 } else if r < 18 { 12 } else if rng.next_int_bound(500) == 0 { 6 } else { 0 }
        } else {
            0
        };
        let pos = Vec3::new(x, feet + half.y + 0.01, z);
        Self { kind, body: Player { pos, vel: Vec3::ZERO, on_ground: false }, yaw, health: hp, limb: 0.0, limb_amt: 0.0, hurt: 0, size, color,
            fuse: 0, half, age: 0, yaw_vel: 0.0, look: None, goal: None, fspeed: 0.0, jumping: false, wet: false, blocked: false, life: 0,
            last: 0, target: false, shot: false, lit: false, anger: 0, attack_time: 0, hop: 0, egg: rng.next_int_bound(6000) + 6000, fire: 0,
            free: 0, swim: Vec3::ZERO }
    }

    pub fn aabb(&self) -> (Vec3, Vec3) {
        (self.body.pos - self.half, self.body.pos + self.half)
    }

    /// `EntityLiving.func_27021_X`: false when it despawns (more than 128 blocks away, or 1/800 after 600 ticks beyond 32).
    fn old(&mut self, rng: &mut JavaRandom, d2: f32) -> bool {
        self.age += 1;
        if d2 > 16384.0 {
            return true;
        }
        if self.age > 600 && rng.next_int_bound(800) == 0 {
            if d2 < 1024.0 { self.age = 0 } else { return true }
        }
        false
    }

    /// One 20 Hz tick of AI. Returns false when the mob despawns (or, for a creeper, has gone off).
    fn tick(&mut self, rng: &mut JavaRandom, c: &Ctx, ev: &mut Vec<Ev>) -> bool {
        self.life = self.life.saturating_sub(1);
        self.hurt = self.hurt.saturating_sub(1);
        self.free = self.free.saturating_sub(1);
        self.attack_time = self.attack_time.saturating_sub(1);
        let (k, pos) = (self.kind, self.body.pos);
        let (d, (cx, cy, cz)) = (c.player - pos, cell(pos));
        let (d2, dist) = (d.length_squared(), d.length());
        // EntityZombie / EntitySkeleton.onLivingUpdate: daylight on bare sky sets fire; fire is 1 damage a second.
        if matches!(k, Zombie | Skeleton) {
            let b = brightness((c.light)(cx, cy, cz));
            if c.day && b > 0.5 && (cy + 1..128).all(|y| matches!((c.get)(cx, y, cz), Some(0))) && rng.next_float() * 30.0 < (b - 0.4) * 2.0 {
                self.fire = 160;
            }
        }
        if self.fire > 0 {
            self.fire -= 1;
            if self.wet { self.fire = 0 } else if self.fire % 20 == 0 { self.health -= 1; self.hurt = 10 }
        }
        // EntityCreature.findPlayerToAttack per kind; anger (zombie pigman, wolf) sets `target` itself.
        self.anger = (self.anger - 1).max(0);
        let see = sees(c.get, pos, c.eye);
        if self.target {
            self.target = d2 <= 1024.0 && (self.anger > 0 || !matches!(k, PigZombie | Wolf));
        } else {
            self.target = see && d2 < 256.0 && match k {
                Zombie | Giant | Skeleton | Creeper | Slime => true,
                Spider => (c.light)(cx, cy, cz) < 8,
                _ => false,
            };
        }
        let alive = match k {
            Squid => self.ai_squid(rng, d2),
            Slime => self.ai_slime(rng, c, d2),
            _ => self.ai_walk(rng, c, d2),
        };
        if !alive {
            return false;
        }
        if k == Chicken {
            self.egg -= 1;
            if self.egg <= 0 {
                self.egg = rng.next_int_bound(6000) + 6000;
                ev.push(Ev::Egg(pos));
            }
        }
        if !self.target {
            self.lit = false;
            self.fuse = self.fuse.saturating_sub(1);
            self.shot = false;
            return true;
        }
        // attackEntity
        self.shot = false;
        let face = (d.z.atan2(d.x)).to_degrees() - 90.0;
        match k {
            Skeleton if see && dist < 10.0 => {
                if self.attack_time == 0 {
                    let from = Vec3::new(pos.x, pos.y - self.half.y + 1.4, pos.z);
                    let mut dir = c.eye - Vec3::Y * 0.2 - from;
                    dir.y += dist * 0.2; // `setArrowHeading(dx, dy + dist x 0.2, dz, 0.6, 12.0)`
                    let noise = Vec3::new(rng.next_float() - 0.5, rng.next_float() - 0.5, rng.next_float() - 0.5) * 0.3;
                    ev.push(Ev::Shoot(from, (dir.normalize() + noise) * 0.6));
                    self.attack_time = 30;
                }
                self.yaw = face;
                self.shot = true;
            }
            Creeper => {
                if see && ((!self.lit && dist < 3.0) || (self.lit && dist < 7.0)) {
                    self.lit = true;
                    self.fuse += 1;
                    self.shot = true;
                    if self.fuse >= 30 {
                        ev.push(Ev::Boom(pos));
                        return false;
                    }
                } else {
                    self.lit = false;
                    self.fuse = self.fuse.saturating_sub(1);
                }
            }
            Slime if self.size > 1.0 && dist < 0.6 * self.size + 0.4 && see => ev.push(Ev::Hurt(self.size as i32)),
            Zombie | PigZombie | Giant | Wolf | Spider => {
                let bright = (c.light)(cx, cy, cz) >= 8;
                if k == Spider && bright && rng.next_int_bound(100) == 0 {
                    self.target = false; // a spider gives up in the light
                } else if k == Spider && dist > 2.0 && dist < 6.0 && rng.next_int_bound(10) == 0 {
                    if self.body.on_ground {
                        // The leap: 0.4 up, 0.4 along, in blocks per tick.
                        let h = (d.x * d.x + d.z * d.z).sqrt().max(1e-4);
                        self.body.vel = Vec3::new(d.x / h * 8.0 + self.body.vel.x * 0.2, 8.0, d.z / h * 8.0 + self.body.vel.z * 0.2);
                        self.free = 10;
                    }
                } else if dist < 2.0 + (self.half.x - 0.3).max(0.0) && self.attack_time == 0 {
                    self.attack_time = 20;
                    ev.push(Ev::Hurt(spec(k).4));
                }
            }
            _ => {}
        }
        true
    }

    /// The pig AI (see the module doc), plus walking after the player when `target` is set.
    fn ai_walk(&mut self, rng: &mut JavaRandom, c: &Ctx, d2: f32) -> bool {
        let (k, pos, get) = (self.kind, self.body.pos, c.get);
        if self.target && self.shot {
            (self.goal, self.fspeed, self.jumping) = (None, 0.0, false); // ranged attackers and a lit creeper stand and face
            return true;
        }
        if self.target {
            self.goal = Some((c.player.x, c.player.z));
        } else if (self.goal.is_none() && rng.next_int_bound(80) == 0) || rng.next_int_bound(80) == 0 {
            // `func_31026_E`: of 10 random cells within 6 x 3 x 6 the best weight, grass 10.
            let mut best: Option<(f32, (i32, i32))> = None;
            for _ in 0..10 {
                let x = (pos.x + rng.next_int_bound(13) as f32 - 6.0).floor() as i32;
                let y = (pos.y - self.half.y + rng.next_int_bound(7) as f32 - 3.0).floor() as i32;
                let z = (pos.z + rng.next_int_bound(13) as f32 - 6.0).floor() as i32;
                let w = if get(x, y - 1, z) == Some(2) { 10.0 } else { -0.5 };
                if best.map_or(true, |b| w > b.0) {
                    best = Some((w, (x, z)));
                }
            }
            // The cell's height is ignored (no path-finder to climb to it); only a blocked mob hops.
            self.goal = best.map(|(_, (x, z))| (x as f32 + 0.5, z as f32 + 0.5));
        }
        let speed = match k {
            PigZombie if self.target => 0.95,
            _ => spec(k).3,
        };
        if let (Some((gx, gz)), true) = (self.goal, self.goal.is_some() && (self.target || rng.next_int_bound(100) != 0)) {
            self.jumping = false;
            let (dx, dz) = (gx - pos.x, gz - pos.z);
            if dx * dx + dz * dz < (self.half.x * 4.0).powi(2) {
                self.goal = None; // the path is finished
                self.fspeed = 0.0;
            } else {
                self.yaw += norm(dz.atan2(dx).to_degrees() - 90.0 - self.yaw).clamp(-30.0, 30.0);
                self.fspeed = speed * WALK;
            }
            // ponytail: vanilla hops when `isCollidedHorizontally && !hasPath()`; with no path-finder a blocked mob hops.
            self.jumping |= self.blocked;
            self.jumping |= rng.next_float() < 0.8 && self.wet;
        } else {
            // EntityLiving.updatePlayerActionState: stand, look at a player within 8, or turn at random.
            self.goal = None;
            if self.old(rng, d2) {
                return false;
            }
            self.fspeed = 0.0;
            if rng.next_float() < 0.02 {
                if d2 < 64.0 { self.look = Some(10 + rng.next_int_bound(20)) } else { self.yaw_vel = (rng.next_float() - 0.5) * 20.0 }
            }
            if let Some(t) = self.look {
                let d = c.player - pos;
                self.yaw += norm((d.z.atan2(d.x)).to_degrees() - 90.0 - self.yaw).clamp(-10.0, 10.0);
                self.look = (t > 0 && d2 <= 64.0).then_some(t - 1);
            } else {
                if rng.next_float() < 0.05 { self.yaw_vel = (rng.next_float() - 0.5) * 20.0 }
                self.yaw += self.yaw_vel;
            }
            // Vanilla leaves `isJumping` as it was; a mob that last hopped would hop for ever, so it is cleared here.
            self.jumping = self.wet && rng.next_float() < 0.8;
        }
        self.yaw_vel *= 0.9;
        true
    }

    /// `EntitySquid.onLivingUpdate`: a new random swim vector (blocks per tick) every ~50 ticks, or when out of water.
    fn ai_squid(&mut self, rng: &mut JavaRandom, d2: f32) -> bool {
        if rng.next_int_bound(50) == 0 || !self.wet || self.swim == Vec3::ZERO {
            let f = rng.next_float() * std::f32::consts::TAU;
            self.swim = Vec3::new(f.cos() * 0.2, -0.1 + rng.next_float() * 0.2, f.sin() * 0.2);
        }
        self.yaw = (-self.swim.x).atan2(self.swim.z).to_degrees();
        !self.old(rng, d2)
    }

    /// `EntitySlime.updatePlayerActionState`: face the player, and hop every 10..30 ticks (a third of that when hunting).
    fn ai_slime(&mut self, rng: &mut JavaRandom, c: &Ctx, d2: f32) -> bool {
        if self.old(rng, d2) {
            return false;
        }
        if self.target {
            let d = c.player - self.body.pos;
            self.yaw += norm((d.z.atan2(d.x)).to_degrees() - 90.0 - self.yaw).clamp(-10.0, 10.0);
        } else if rng.next_float() < 0.05 {
            self.yaw += (rng.next_float() - 0.5) * 20.0;
        }
        if self.body.on_ground {
            self.hop -= 1;
            if self.hop <= 0 {
                self.hop = rng.next_int_bound(20) + 10;
                if self.target { self.hop /= 3 }
                self.jumping = true;
                self.fspeed = 1.5 + 0.5 * self.size; // ponytail: `moveFlying` in the air accumulates; this is a fixed hop speed
            } else {
                self.jumping = false;
                self.fspeed = 0.0;
            }
        }
        true
    }

    /// Per-frame movement along the heading the last tick chose.
    fn step(&mut self, dt: f32, get: BlockQuery<'_>) {
        let ticks = dt * 20.0;
        let (s, c) = self.yaw.to_radians().sin_cos();
        if self.free > 0 {
            let drag = 0.546f32.powf(ticks); // knocked back or leaping: no steering until it ends
            self.body.vel.x *= drag;
            self.body.vel.z *= drag;
        } else if self.kind == Squid {
            self.body.vel = if self.wet { self.swim * 20.0 } else { Vec3::new(0.0, self.body.vel.y, 0.0) };
        } else {
            (self.body.vel.x, self.body.vel.z) = (-s * self.fspeed, c * self.fspeed);
        }
        if self.kind == Spider && self.blocked && self.target {
            self.body.vel.y = 4.0; // `isOnLadder` while against a wall: climbs 0.2 a tick
        }
        let (vx, vz) = (self.body.vel.x, self.body.vel.z);
        let before = self.body.pos;
        let (w, l) = physics::step_box(&mut self.body, self.half, dt, self.jumping, get);
        self.wet = w || l;
        self.blocked = self.fspeed > 0.0 && ((vx != 0.0 && self.body.vel.x == 0.0) || (vz != 0.0 && self.body.vel.z == 0.0));
        if self.kind == Chicken && self.body.vel.y < -2.3 {
            self.body.vel.y = -2.3; // `motionY *= 0.6` while falling
        }
        let moved = ((self.body.pos.x - before.x).powi(2) + (self.body.pos.z - before.z).powi(2)).sqrt() / ticks;
        self.limb_amt += ((moved * 4.0).min(1.0) - self.limb_amt) * (1.0 - 0.6f32.powf(ticks));
        self.limb += self.limb_amt * ticks;
    }

    /// `EntityLiving.attackEntityFrom`: a hit inside the 10-tick window only pays the difference; the first one knocks back.
    /// A zombie pigman or wolf hit turns on its attacker.
    pub fn damage(&mut self, dmg: i32, from: Vec3, rng: &mut JavaRandom) {
        if self.health <= 0 {
            return;
        }
        self.age = 0;
        if matches!(self.kind, PigZombie | Wolf) {
            self.anger = if self.kind == Wolf { 1 << 20 } else { 400 + rng.next_int_bound(400) };
            self.target = true;
        }
        if self.life > 10 {
            if dmg <= self.last { return }
            self.health -= dmg - self.last;
            self.last = dmg;
        } else {
            self.last = dmg;
            self.life = 20;
            self.health -= dmg;
            self.hurt = 10;
            let (dx, dz) = (from.x - self.body.pos.x, from.z - self.body.pos.z);
            let d = (dx * dx + dz * dz).sqrt();
            if d > 1e-4 {
                // `knockBack`: motion / 2 - direction x 0.4 (blocks per tick), up 0.4 at most.
                self.body.vel.x = self.body.vel.x / 2.0 - dx / d * 8.0;
                self.body.vel.z = self.body.vel.z / 2.0 - dz / d * 8.0;
                self.body.vel.y = (self.body.vel.y / 2.0 + 8.0).min(8.0);
                self.free = 10;
            }
        }
    }

    /// `dropFewItems`: `getDropItemId` x `rand(3)`; skeleton also bones, sheep one wool, squid 1..=3 ink, slime balls only at size 1.
    fn loot(&self, rng: &mut JavaRandom) -> Vec<ItemStack> {
        let s = |id: u16, damage: u16, n: i32| (0..n).map(move |_| ItemStack { id, count: 1, damage });
        let n = rng.next_int_bound(3);
        match self.kind {
            Pig => s(319, 0, n).collect(),
            PigZombie => s(320, 0, n).collect(),
            Cow => s(334, 0, n).collect(),
            Chicken | Zombie => s(288, 0, n).collect(),
            Creeper => s(289, 0, n).collect(),
            Spider => s(287, 0, n).collect(),
            Slime if self.size == 1.0 => s(341, 0, n).collect(),
            Skeleton => s(262, 0, n).chain(s(352, 0, rng.next_int_bound(3))).collect(),
            Sheep => s(35, self.color as u16, 1).collect(),
            Squid => s(351, 0, n + 1).collect(),
            _ => Vec::new(),
        }
    }
}

/// `EntityArrow` fired by a skeleton: blocks per tick, 4 damage on the player, gone when it meets a block.
pub struct Arrow {
    pub pos: Vec3,
    vel: Vec3,
    age: u16,
}

impl Arrow {
    fn tick(&mut self, c: &Ctx, ev: &mut Vec<Ev>) -> bool {
        self.age += 1;
        self.pos += self.vel;
        let (x, y, z) = cell(self.pos);
        if self.age > 600 || solid((c.get)(x, y, z)) {
            return false; // ponytail: vanilla sticks in the block (and a player's arrows can be picked up again)
        }
        let d = self.pos - c.player;
        if d.x.abs() < 0.4 && d.z.abs() < 0.4 && d.y.abs() < 1.0 {
            ev.push(Ev::Hurt(4));
            return false;
        }
        self.vel = self.vel * 0.99 - Vec3::Y * 0.03;
        true
    }
}

/// (`getAttackStrength`, wear) of hitting with `held`: `ItemSword` 4 + material, `ItemTool` 2 + material, anything else 1
/// (`Item.getDamageVsEntity`); material bonus wood 0, stone 1, iron 2, diamond 3, gold 0. Swords wear 1, tools 2, a hoe none.
pub fn attack(held: Option<u16>) -> (i32, u16) {
    match held.and_then(craft::tool) {
        Some((Tool::Sword, m)) => (4 + [0, 1, 2, 3, 0][m], 1),
        Some((Tool::Hoe, _)) => (1, 0),
        Some((_, m)) => (2 + [0, 1, 2, 3, 0][m], 2),
        None => (1, 0),
    }
}

/// Distance along the ray `o + t d` to the box `lo..hi`, if it hits within `t >= 0`.
fn ray_box(o: Vec3, d: Vec3, lo: Vec3, hi: Vec3) -> Option<f32> {
    let (mut t0, mut t1) = (0.0f32, f32::MAX);
    for i in 0..3 {
        if d[i].abs() < 1e-8 {
            if o[i] < lo[i] || o[i] > hi[i] { return None }
        } else {
            let (a, b) = ((lo[i] - o[i]) / d[i], (hi[i] - o[i]) / d[i]);
            t0 = t0.max(a.min(b));
            t1 = t1.min(a.max(b));
        }
    }
    (t0 <= t1).then_some(t0)
}

/// Grass with two air cells above, in column (x, z): the y of the grass.
fn grass_top(get: BlockQuery<'_>, x: i32, z: i32) -> Option<i32> {
    let y = (1..127).rev().find(|&y| get(x, y, z) != Some(0))?;
    (get(x, y, z) == Some(2) && get(x, y + 1, z) == Some(0) && get(x, y + 2, z) == Some(0)).then_some(y)
}

/// `Block.blockResistance` (hardness x 5 unless `setResistance` was called).
fn resistance(id: u8) -> f32 {
    match RESIST.binary_search_by_key(&id, |e| e.0) {
        Ok(i) => RESIST[i].1,
        _ if (8..=11).contains(&id) => 500.0,
        _ => dig::hardness(id).max(0.0) * 5.0,
    }
}

pub struct Mobs {
    pub list: Vec<Mob>,
    pub arrows: Vec<Arrow>,
    pub rng: JavaRandom,
    seed: i64,
    acc: f32,
    clock: u32,
    burst: u32,
}

impl Mobs {
    pub fn new(seed: i64) -> Self {
        Self { list: Vec::new(), arrows: Vec::new(), rng: JavaRandom::new(seed ^ 0x5049_47), seed, acc: 0.0, clock: 0, burst: 200 }
    }

    /// `Chunk.func_997_a(987234911)`: slimes spawn in one chunk in ten.
    fn slime_chunk(&self, cx: i32, cz: i32) -> bool {
        let s = self.seed
            .wrapping_add(cx.wrapping_mul(cx).wrapping_mul(4987142) as i64)
            .wrapping_add(cx.wrapping_mul(5947611) as i64)
            .wrapping_add((cz.wrapping_mul(cz) as i64).wrapping_mul(4392871))
            .wrapping_add(cz.wrapping_mul(389711) as i64);
        JavaRandom::new(s ^ 987234911).next_int_bound(10) == 0
    }

    /// `getCanSpawnHere` per kind at the cell (x, y, z) (the cell the mob would stand in).
    fn can_spawn(&mut self, c: &Ctx, kind: Kind, x: i32, y: i32, z: i32) -> bool {
        let (below, feet, head) = ((c.get)(x, y - 1, z), (c.get)(x, y, z), (c.get)(x, y + 1, z));
        let light = (c.light)(x, y, z);
        match kind {
            Pig | Cow | Sheep | Chicken | Wolf => below == Some(2) && feet == Some(0) && head == Some(0) && light > 8,
            Squid => matches!(feet, Some(8 | 9)) && matches!(head, Some(8 | 9)) && (46..63).contains(&y),
            _ => {
                let dark = light as i32 <= self.rng.next_int_bound(8);
                let ground = solid(below) && feet == Some(0) && head == Some(0);
                dark && ground && (kind != Slime || (self.rng.next_int_bound(10) == 0 && self.slime_chunk(x >> 4, z >> 4) && y < 16))
            }
        }
    }

    /// A pack of 1..=4 of `kind` around the cell (x, y, z).
    fn pack(&mut self, c: &Ctx, kind: Kind, (x, y, z): (i32, i32, i32), cap: usize) {
        for _ in 0..1 + self.rng.next_int_bound(4) {
            let (px, pz) = (x + self.rng.next_int_bound(6) - self.rng.next_int_bound(6), z + self.rng.next_int_bound(6) - self.rng.next_int_bound(6));
            let py = match kind {
                Pig | Cow | Sheep | Chicken | Wolf => grass_top(c.get, px, pz).map_or(-1, |g| g + 1),
                _ => y,
            };
            if py < 1 || (c.player - Vec3::new(px as f32, py as f32, pz as f32)).length() < 24.0 || !self.can_spawn(c, kind, px, py, pz) {
                continue;
            }
            let kinds: &[Kind] = match kind {
                Pig | Cow | Sheep | Chicken | Wolf => &[Pig, Cow, Sheep, Chicken, Wolf],
                Squid => &[Squid],
                _ => &[Zombie, Skeleton, Creeper, Spider, Slime, Giant, PigZombie],
            };
            if self.list.iter().filter(|m| kinds.contains(&m.kind)).count() < cap && self.list.len() < MAX_MOBS {
                let yaw = self.rng.next_float() * 360.0;
                let m = Mob::new(kind, px as f32 + 0.5, py as f32, pz as f32 + 0.5, yaw, &mut self.rng);
                self.list.push(m);
            }
        }
    }

    /// `SpawnerAnimals`: animals and squid every 400 ticks (and a burst at start), monsters every 40, 24..48 blocks away.
    fn spawn(&mut self, c: &Ctx) {
        let n = |l: &[Mob], ks: &[Kind]| l.iter().filter(|m| ks.contains(&m.kind)).count();
        let ring = |r: &mut JavaRandom| {
            let (a, d) = (r.next_float() * std::f32::consts::TAU, 24.0 + r.next_float() * 24.0);
            ((c.player.x + a.cos() * d).floor() as i32, (c.player.z + a.sin() * d).floor() as i32)
        };
        if self.burst > 0 || self.clock % 400 == 0 {
            self.burst = self.burst.saturating_sub(1);
            if n(&self.list, &[Pig, Cow, Sheep, Chicken, Wolf]) < CAP_ANIMALS {
                let (x, z) = ring(&mut self.rng);
                // Weights of the plains list: sheep 12, pig 10, chicken 10, cow 8 (wolves live in forests: not spawned here).
                let kind = match self.rng.next_int_bound(40) { 0..=11 => Sheep, 12..=21 => Pig, 22..=31 => Chicken, _ => Cow };
                if let Some(y) = grass_top(c.get, x, z) {
                    self.pack(c, kind, (x, y + 1, z), CAP_ANIMALS);
                }
            }
            if n(&self.list, &[Squid]) < CAP_SQUID {
                let (x, z) = ring(&mut self.rng);
                let y = 46 + self.rng.next_int_bound(17);
                self.pack(c, Squid, (x, y, z), CAP_SQUID);
            }
        }
        if self.clock % 40 == 0 && n(&self.list, &[Zombie, Skeleton, Creeper, Spider, Slime]) < CAP_MONSTERS {
            for _ in 0..8 {
                let (x, z) = ring(&mut self.rng);
                let y = ((c.player.y as i32) + self.rng.next_int_bound(41) - 20).clamp(1, 126);
                let kind = [Spider, Zombie, Skeleton, Creeper, Slime][self.rng.next_int_bound(5) as usize];
                let before = self.list.len();
                self.pack(c, kind, (x, y, z), CAP_MONSTERS);
                if self.list.len() > before {
                    break;
                }
            }
        }
    }

    /// Advance `dt` seconds: AI at 20 Hz, movement per frame, spawns, arrows and the drops of the dead. Returns what the
    /// player must suffer or the world must do (`Hurt`, `Boom`).
    pub fn update(&mut self, dt: f32, c: &Ctx, drops: &mut Drops) -> Vec<Ev> {
        let mut ev = Vec::new();
        self.acc += dt;
        while self.acc >= TICK {
            self.acc -= TICK;
            self.clock += 1;
            let rng = &mut self.rng;
            self.list.retain_mut(|m| m.tick(rng, c, &mut ev));
            self.arrows.retain_mut(|a| a.tick(c, &mut ev));
            self.spawn(c);
        }
        for m in &mut self.list {
            m.step(dt, c.get);
        }
        let mut out = Vec::new();
        for e in ev {
            match e {
                Ev::Shoot(pos, vel) if self.arrows.len() < MAX_ARROWS => self.arrows.push(Arrow { pos, vel, age: 0 }),
                Ev::Egg(p) => drops.spawn_stack(ItemStack { id: 344, count: 1, damage: 0 }, cell(p)),
                Ev::Hurt(_) | Ev::Boom(_) => out.push(e),
                Ev::Shoot(..) => {}
            }
        }
        let mut i = 0;
        while i < self.list.len() {
            if self.list[i].health > 0 {
                i += 1;
                continue;
            }
            let m = self.list.swap_remove(i);
            let p = m.body.pos;
            for s in m.loot(&mut self.rng) {
                drops.spawn_stack(s, cell(p));
            }
            if m.kind == Slime && m.size > 1.0 {
                // `EntitySlime.setEntityDead`: four of half the size.
                for j in 0..4 {
                    if self.list.len() < MAX_MOBS {
                        let (ox, oz) = ((j % 2) as f32 - 0.5, (j / 2) as f32 - 0.5);
                        let mut s = Mob::new(Slime, p.x + ox * m.size / 4.0, p.y - m.half.y, p.z + oz * m.size / 4.0, self.rng.next_float() * 360.0, &mut self.rng);
                        let size = m.size / 2.0;
                        (s.size, s.health, s.half) = (size, (size * size) as i32, Vec3::new(0.3 * size, 0.3 * size, 0.3 * size));
                        self.list.push(s);
                    }
                }
            }
        }
        out
    }

    /// The nearest mob on the ray within `reach`: (index, distance).
    pub fn pick(&self, eye: Vec3, dir: Vec3, reach: f32) -> Option<(usize, f32)> {
        self.list.iter().enumerate().filter_map(|(i, m)| {
            let (lo, hi) = m.aabb();
            ray_box(eye, dir, lo, hi).filter(|&t| t <= reach).map(|t| (i, t))
        }).min_by(|a, b| a.1.total_cmp(&b.1))
    }

    pub fn hit(&mut self, i: usize, dmg: i32, from: Vec3) {
        self.list[i].damage(dmg, from, &mut self.rng);
    }

    /// `Explosion.doExplosionA`: 16 x 16 x 16 rays from the surface of a cube, each losing strength to the blocks it crosses
    /// (resistance / 5 + 0.3, x 0.3) and 0.225 a step; the blocks left with strength are destroyed. Also hurts the mobs
    /// in range (`(impact^2 + impact) / 2 x 8 x power + 1`, no exposure test). Returns the cells to clear.
    pub fn explode(&mut self, get: BlockQuery<'_>, at: Vec3, power: f32) -> Vec<(i32, i32, i32)> {
        let mut cells = std::collections::BTreeSet::new();
        for i in 0..16 {
            for j in 0..16 {
                for k in 0..16 {
                    if !(matches!(i, 0 | 15) || matches!(j, 0 | 15) || matches!(k, 0 | 15)) {
                        continue;
                    }
                    let d = Vec3::new(i as f32, j as f32, k as f32) / 15.0 * 2.0 - Vec3::ONE;
                    let (d, mut p) = (d / d.length() * 0.3, at);
                    let mut s = power * (0.7 + self.rng.next_float() * 0.6);
                    while s > 0.0 {
                        let c = cell(p);
                        if let Some(id) = get(c.0, c.1, c.2).filter(|&b| b > 0) {
                            s -= (resistance(id) / 5.0 + 0.3) * 0.3;
                        }
                        if s > 0.0 {
                            cells.insert(c);
                        }
                        p += d;
                        s -= 0.225;
                    }
                }
            }
        }
        let rng = &mut self.rng;
        for m in &mut self.list {
            let impact = 1.0 - (m.body.pos - at).length() / (power * 2.0);
            if impact > 0.0 {
                let dmg = ((impact * impact + impact) / 2.0 * 8.0 * power + 1.0) as i32;
                m.life = 0;
                m.damage(dmg, at, rng);
            }
        }
        cells.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bedrock at y = 0, stone 1..=8, grass at 9, air above, 64 x 64 wide.
    fn flat(x: i32, y: i32, z: i32) -> Option<u8> {
        if !(0..64).contains(&x) || !(0..64).contains(&z) || y < 0 { return None }
        Some(match y { 0 => 7, 1..=8 => 1, 9 => 2, _ => 0 })
    }

    fn ctx<'a>(player: Vec3, light: &'a dyn Fn(i32, i32, i32) -> u8) -> Ctx<'a> {
        Ctx { get: &flat, light, day: false, player, eye: player + Vec3::Y * 0.6 }
    }

    /// A pig on grass settles, wanders off its start and stays on the ground; a hit hurts, knocks back and kills after
    /// 10 damage (swords: 4 + material); the ray finds it; a distant player despawns it.
    #[test]
    fn pig_wanders_is_hit_and_dies() {
        let mut rng = JavaRandom::new(7);
        let l = |_, _, _| 15u8;
        let c = ctx(Vec3::new(32.0, 11.0, 52.0), &l);
        let mut p = Mob::new(Pig, 32.5, 10.0, 32.5, 0.0, &mut rng);
        let (start, mut far, mut ev) = (p.body.pos, 0.0f32, Vec::new());
        for _ in 0..2000 {
            assert!(p.tick(&mut rng, &c, &mut ev));
            for _ in 0..3 { p.step(TICK / 3.0, &flat) }
            far = far.max((p.body.pos - start).length());
            assert!(p.body.pos.y > 9.9 && p.body.pos.y < 11.0, "y = {}", p.body.pos.y);
        }
        assert!(far > 1.0, "never moved");
        assert!(ev.is_empty());
        assert_eq!(ray_box(Vec3::new(0.0, 0.0, -2.0), Vec3::Z, Vec3::splat(-0.45), Vec3::splat(0.45)), Some(1.55));
        assert!(ray_box(Vec3::new(2.0, 0.0, -2.0), Vec3::Z, Vec3::splat(-0.45), Vec3::splat(0.45)).is_none());
        assert_eq!(attack(None), (1, 0));
        assert_eq!(attack(Some(268)), (4, 1)); // wooden sword
        assert_eq!(attack(Some(257)), (4, 2)); // iron pickaxe: 2 + 2
        p.damage(4, p.body.pos + Vec3::X, &mut rng);
        assert_eq!(p.health, 6);
        assert!(p.body.vel.x < 0.0 && p.hurt == 10);
        p.damage(4, p.body.pos, &mut rng); // inside the window, not bigger: ignored
        assert_eq!(p.health, 6);
        p.damage(6, p.body.pos, &mut rng); // bigger: pays the difference
        assert_eq!(p.health, 4);
        let far_player = ctx(Vec3::new(232.0, 10.0, 32.0), &l);
        let mut q = Mob::new(Pig, 32.5, 10.0, 32.5, 0.0, &mut rng);
        assert!((0..50).any(|_| !q.tick(&mut rng, &far_player, &mut ev)));
    }

    /// Hostiles: a skeleton in range shoots, a creeper next to the player lights its fuse and goes off after 30 ticks,
    /// a zombie bites for 5; an explosion clears dirt-like blocks but not bedrock.
    #[test]
    fn hostiles_attack_and_explode() {
        let mut rng = JavaRandom::new(3);
        let l = |_, _, _| 0u8;
        let c = ctx(Vec3::new(32.5, 11.0, 36.5), &l);
        let mut s = Mob::new(Skeleton, 32.5, 10.0, 32.5, 0.0, &mut rng);
        let mut ev = Vec::new();
        s.tick(&mut rng, &c, &mut ev);
        assert!(ev.iter().any(|e| matches!(e, Ev::Shoot(..))));
        let mut z = Mob::new(Zombie, 32.5, 10.0, 35.0, 0.0, &mut rng);
        ev.clear();
        z.tick(&mut rng, &c, &mut ev);
        assert!(ev.iter().any(|e| matches!(e, Ev::Hurt(5))));
        let near = ctx(Vec3::new(32.5, 11.0, 33.5), &l);
        let mut k = Mob::new(Creeper, 32.5, 10.0, 32.5, 0.0, &mut rng);
        ev.clear();
        let gone = (0..40).any(|_| !k.tick(&mut rng, &near, &mut ev));
        assert!(gone && ev.iter().any(|e| matches!(e, Ev::Boom(_))));
        let mut m = Mobs::new(1);
        let cells = m.explode(&flat, Vec3::new(32.5, 1.5, 32.5), 3.0);
        assert!(!cells.is_empty() && cells.iter().all(|c| c.1 > 0), "{} cells", cells.len()); // bedrock at y 0 holds
        assert!(m.explode(&flat, Vec3::new(32.5, 20.5, 32.5), 3.0).len() > 20); // open air: a sphere of cells
    }

    /// Slimes split into four halves, and a sheep drops its colour of wool.
    #[test]
    fn slime_splits_and_sheep_drop_wool() {
        let mut rng = JavaRandom::new(5);
        let mut m = Mobs::new(1);
        m.burst = 0;
        let mut s = Mob::new(Slime, 32.5, 10.0, 32.5, 0.0, &mut rng);
        (s.size, s.health) = (4.0, 0);
        m.list.push(s);
        let l = |_, _, _| 15u8;
        let c = ctx(Vec3::new(5.0, 11.0, 5.0), &l);
        m.update(TICK, &c, &mut Drops::new(1));
        assert_eq!(m.list.len(), 4);
        assert!(m.list.iter().all(|s| s.kind == Slime && s.size == 2.0 && s.health == 4));
        let mut sheep = Mob::new(Sheep, 1.5, 10.0, 1.5, 0.0, &mut rng);
        sheep.color = 6;
        assert_eq!(sheep.loot(&mut rng).iter().map(|s| (s.id, s.damage)).collect::<Vec<_>>(), vec![(35, 6)]);
    }
}
