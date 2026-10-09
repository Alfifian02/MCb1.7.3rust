//! M8, first mob: the pig. Ports of `EntityPig`, `EntityAnimal`, `EntityCreature.updatePlayerActionState`,
//! `EntityLiving.updatePlayerActionState` / `attackEntityFrom` / `dropFewItems` / `func_27021_X` (despawn), driven at 20 Hz,
//! with the body moved every frame by `physics::step_box`.
//! ponytail: no A* (`Pathfinder`): a "path" is a straight walk to the chosen cell, and a pig that is blocked hops, so it
//! climbs one-block steps but walks into walls and cannot go around them. Light is not read (`getBlockPathWeight` counts
//! it as 0). No saddle/riding, no sounds, no death animation, no fire or fall damage, pigs are not saved. Walk speed is
//! the terminal speed of `moveFlying` (0.7 x 0.98 x 0.1 against 0.546 drag = 3 m/s), not accumulated momentum.
//! UNVERIFIED: spawning. Vanilla spawns animals while generating chunks (`SpawnerAnimals.performWorldGenSpawning`) and
//! every 400 ticks (`performSpawning`); here a short burst at start and then one group per 400 ticks, 24..48 blocks away.

use glam::Vec3;

use crate::world::craft::{self, Kind};
use crate::world::gen::noise::JavaRandom;
use crate::world::items::{Drops, ItemStack};
use crate::world::physics::{self, BlockQuery, Player};

/// `EntityPig.setSize(0.9, 0.9)`, as half-extents.
pub const HALF: Vec3 = Vec3::new(0.45, 0.45, 0.45);
/// Most pigs in the world (and the render buffer size).
pub const MAX_PIGS: usize = 16;
const CAP: usize = 8;
const PORK: u16 = 319;
const MOVE_SPEED: f32 = 0.7;
const WALK: f32 = 3.0;
const TICK: f32 = 0.05;

pub struct Pig {
    /// `pos` is the centre of the 0.9 box.
    pub body: Player,
    /// `rotationYaw`, degrees; the pig moves along (-sin, cos).
    pub yaw: f32,
    pub health: i32,
    /// `field_704_R` / `field_705_Q`: leg swing phase and amount.
    pub limb: f32,
    pub limb_amt: f32,
    /// `hurtTime`: ticks of red flash left.
    pub hurt: u8,
    age: u32,
    yaw_vel: f32,
    look: Option<i32>,
    goal: Option<(f32, f32)>,
    forward: f32,
    jumping: bool,
    wet: bool,
    blocked: bool,
    life: u8,
    last: i32,
}

fn norm(mut a: f32) -> f32 {
    while a < -180.0 { a += 360.0; }
    while a >= 180.0 { a -= 360.0; }
    a
}

impl Pig {
    fn new(x: f32, y: i32, z: f32, yaw: f32) -> Self {
        let pos = Vec3::new(x, y as f32 + HALF.y + 0.01, z);
        Self { body: Player { pos, vel: Vec3::ZERO, on_ground: false }, yaw, health: 10, limb: 0.0, limb_amt: 0.0, hurt: 0, age: 0,
            yaw_vel: 0.0, look: None, goal: None, forward: 0.0, jumping: false, wet: false, blocked: false, life: 0, last: 0 }
    }

    pub fn aabb(&self) -> (Vec3, Vec3) {
        (self.body.pos - HALF, self.body.pos + HALF)
    }

    /// One 20 Hz tick of AI. Returns false when the pig despawns.
    fn tick(&mut self, rng: &mut JavaRandom, player: Vec3, get: BlockQuery<'_>) -> bool {
        self.life = self.life.saturating_sub(1);
        self.hurt = self.hurt.saturating_sub(1);
        let pos = self.body.pos;
        // EntityCreature: pick somewhere to go (`func_31026_E`): of 10 random cells within 6 x 3 x 6 the best weight, grass 10.
        if (self.goal.is_none() && rng.next_int_bound(80) == 0) || rng.next_int_bound(80) == 0 {
            let mut best: Option<(f32, (i32, i32, i32))> = None;
            for _ in 0..10 {
                let x = (pos.x + rng.next_int_bound(13) as f32 - 6.0).floor() as i32;
                let y = (pos.y - HALF.y + rng.next_int_bound(7) as f32 - 3.0).floor() as i32;
                let z = (pos.z + rng.next_int_bound(13) as f32 - 6.0).floor() as i32;
                let w = if get(x, y - 1, z) == Some(2) { 10.0 } else { -0.5 };
                if best.map_or(true, |b| w > b.0) {
                    best = Some((w, (x, y, z)));
                }
            }
            // The cell's height is ignored (no path-finder to climb to it); only a blocked pig hops.
            self.goal = best.map(|(_, (x, _, z))| (x as f32 + 0.5, z as f32 + 0.5));
        }
        if let (Some((gx, gz)), true) = (self.goal, self.goal.is_some() && rng.next_int_bound(100) != 0) {
            self.jumping = false;
            let (dx, dz) = (gx - pos.x, gz - pos.z);
            if dx * dx + dz * dz < 1.8 * 1.8 {
                self.goal = None; // the path is finished
            } else {
                self.yaw += norm(dz.atan2(dx).to_degrees() - 90.0 - self.yaw).clamp(-30.0, 30.0);
                self.forward = MOVE_SPEED;
            }
            // ponytail: vanilla hops when `isCollidedHorizontally && !hasPath()`; with no path-finder a blocked pig hops.
            self.jumping |= self.blocked;
            self.jumping |= rng.next_float() < 0.8 && self.wet;
        } else {
            // EntityLiving.updatePlayerActionState: stand, look at a player within 8, or turn at random.
            self.goal = None;
            self.age += 1;
            let d2 = (player - pos).length_squared();
            if d2 > 16384.0 {
                return false;
            }
            if self.age > 600 && rng.next_int_bound(800) == 0 {
                if d2 < 1024.0 { self.age = 0 } else { return false }
            }
            self.forward = 0.0;
            if rng.next_float() < 0.02 {
                if d2 < 64.0 { self.look = Some(10 + rng.next_int_bound(20)) } else { self.yaw_vel = (rng.next_float() - 0.5) * 20.0 }
            }
            if let Some(t) = self.look {
                let d = player - pos;
                self.yaw += norm((d.z.atan2(d.x)).to_degrees() - 90.0 - self.yaw).clamp(-10.0, 10.0);
                self.look = (t > 0 && d2 <= 64.0).then_some(t - 1);
            } else {
                if rng.next_float() < 0.05 { self.yaw_vel = (rng.next_float() - 0.5) * 20.0 }
                self.yaw += self.yaw_vel;
            }
            // Vanilla leaves `isJumping` as it was; a pig that last hopped would hop for ever, so it is cleared here.
            self.jumping = self.wet && rng.next_float() < 0.8;
        }
        self.yaw_vel *= 0.9;
        true
    }

    /// Per-frame movement along the heading the last tick chose.
    fn step(&mut self, dt: f32, get: BlockQuery<'_>) {
        let ticks = dt * 20.0;
        let f = self.forward * (WALK / MOVE_SPEED);
        let (s, c) = self.yaw.to_radians().sin_cos();
        let (vx, vz) = if self.hurt > 0 {
            let drag = 0.546f32.powf(ticks); // knocked back: no steering until the flash ends
            (self.body.vel.x * drag, self.body.vel.z * drag)
        } else {
            (-s * f, c * f)
        };
        (self.body.vel.x, self.body.vel.z) = (vx, vz);
        let before = self.body.pos;
        let (w, l) = physics::step_box(&mut self.body, HALF, dt, self.jumping, get);
        self.wet = w || l;
        self.blocked = f > 0.0 && ((vx != 0.0 && self.body.vel.x == 0.0) || (vz != 0.0 && self.body.vel.z == 0.0));
        let moved = ((self.body.pos.x - before.x).powi(2) + (self.body.pos.z - before.z).powi(2)).sqrt() / ticks;
        self.limb_amt += ((moved * 4.0).min(1.0) - self.limb_amt) * (1.0 - 0.6f32.powf(ticks));
        self.limb += self.limb_amt * ticks;
    }

    /// `EntityLiving.attackEntityFrom`: a hit inside the 10-tick window only pays the difference; the first one knocks back.
    pub fn damage(&mut self, dmg: i32, from: Vec3) {
        if self.health <= 0 {
            return;
        }
        self.age = 0;
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
            }
        }
    }
}

/// (damage, wear) of hitting with `held`: `ItemSword` 4 + material, `ItemTool` 2 + material, anything else 1
/// (`Item.getDamageVsEntity`); material bonus wood 0, stone 1, iron 2, diamond 3, gold 0. Swords wear 1, tools 2, a hoe none.
pub fn attack(held: Option<u16>) -> (i32, u16) {
    match held.and_then(craft::tool) {
        Some((Kind::Sword, m)) => (4 + [0, 1, 2, 3, 0][m], 1),
        Some((Kind::Hoe, _)) => (1, 0),
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

pub struct Mobs {
    pub pigs: Vec<Pig>,
    rng: JavaRandom,
    acc: f32,
    clock: u32,
    burst: u32,
}

impl Mobs {
    pub fn new(seed: i64) -> Self {
        Self { pigs: Vec::new(), rng: JavaRandom::new(seed ^ 0x5049_47), acc: 0.0, clock: 0, burst: 200 }
    }

    /// A pack of 1..=4 on grass 24..48 blocks from `player`, if the column there has any.
    fn spawn(&mut self, get: BlockQuery<'_>, player: Vec3) {
        let (a, r) = (self.rng.next_float() * std::f32::consts::TAU, 24.0 + self.rng.next_float() * 24.0);
        let (x, z) = ((player.x + a.cos() * r).floor() as i32, (player.z + a.sin() * r).floor() as i32);
        if grass_top(get, x, z).is_none() {
            return;
        }
        for _ in 0..1 + self.rng.next_int_bound(4) {
            let (px, pz) = (x + self.rng.next_int_bound(6) - self.rng.next_int_bound(6), z + self.rng.next_int_bound(6) - self.rng.next_int_bound(6));
            if let (Some(y), true) = (grass_top(get, px, pz), self.pigs.len() < CAP) {
                let yaw = self.rng.next_float() * 360.0;
                self.pigs.push(Pig::new(px as f32 + 0.5, y + 1, pz as f32 + 0.5, yaw));
            }
        }
    }

    /// Advance `dt` seconds: AI at 20 Hz, movement per frame, spawns, and the drops of the dead (`dropFewItems`: 0..=2 raw pork).
    pub fn update(&mut self, dt: f32, get: BlockQuery<'_>, player: Vec3, drops: &mut Drops) {
        self.acc += dt;
        while self.acc >= TICK {
            self.acc -= TICK;
            self.clock += 1;
            let rng = &mut self.rng;
            self.pigs.retain_mut(|p| p.tick(rng, player, get));
            if self.pigs.len() < CAP && (self.burst > 0 || self.clock % 400 == 0) {
                self.burst = self.burst.saturating_sub(1);
                self.spawn(get, player);
            }
        }
        for p in &mut self.pigs {
            p.step(dt, get);
        }
        let mut i = 0;
        while i < self.pigs.len() {
            if self.pigs[i].health > 0 {
                i += 1;
                continue;
            }
            let p = self.pigs.swap_remove(i).body.pos;
            for _ in 0..self.rng.next_int_bound(3) {
                drops.spawn_stack(ItemStack { id: PORK, count: 1, damage: 0 }, (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32));
            }
        }
    }

    /// The nearest pig on the ray within `reach`: (index, distance).
    pub fn pick(&self, eye: Vec3, dir: Vec3, reach: f32) -> Option<(usize, f32)> {
        self.pigs.iter().enumerate().filter_map(|(i, p)| {
            let (lo, hi) = p.aabb();
            ray_box(eye, dir, lo, hi).filter(|&t| t <= reach).map(|t| (i, t))
        }).min_by(|a, b| a.1.total_cmp(&b.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Grass floor at y = 9 over stone, air above, 64 x 64 wide.
    fn flat(x: i32, y: i32, z: i32) -> Option<u8> {
        if !(0..64).contains(&x) || !(0..64).contains(&z) || y < 0 { return None }
        Some(match y { 0..=8 => 1, 9 => 2, _ => 0 })
    }

    /// A pig on grass settles, wanders off its start and stays on the ground; a hit hurts, knocks back and kills after
    /// 10 damage (swords: 4 + material); the ray finds it; a distant player despawns it.
    #[test]
    fn pig_wanders_is_hit_and_dies() {
        let mut rng = JavaRandom::new(7);
        let mut p = Pig::new(32.5, 10, 32.5, 0.0);
        let start = p.body.pos;
        let mut far = 0.0f32;
        for _ in 0..2000 {
            assert!(p.tick(&mut rng, Vec3::new(32.0, 10.0, 32.0 + 20.0), &flat));
            for _ in 0..3 { p.step(TICK / 3.0, &flat) }
            far = far.max((p.body.pos - start).length());
            assert!(p.body.pos.y > 9.9 && p.body.pos.y < 11.0, "y = {}", p.body.pos.y);
        }
        assert!(far > 1.0, "never moved");
        assert!(ray_box(Vec3::new(0.0, 0.0, -2.0), Vec3::Z, Vec3::splat(-0.45), Vec3::splat(0.45)) == Some(1.55));
        assert!(ray_box(Vec3::new(2.0, 0.0, -2.0), Vec3::Z, Vec3::splat(-0.45), Vec3::splat(0.45)).is_none());
        assert_eq!(attack(None), (1, 0));
        assert_eq!(attack(Some(268)), (4, 1)); // wooden sword
        assert_eq!(attack(Some(257)), (4, 2)); // iron pickaxe: 2 + 2
        p.damage(4, p.body.pos + Vec3::X);
        assert_eq!(p.health, 6);
        assert!(p.body.vel.x < 0.0 && p.hurt == 10);
        p.damage(4, p.body.pos); // inside the window, not bigger: ignored
        assert_eq!(p.health, 6);
        p.damage(6, p.body.pos); // bigger: pays the difference
        assert_eq!(p.health, 4);
        // Despawn: a player 200 blocks away.
        let mut q = Pig::new(32.5, 10, 32.5, 0.0);
        assert!((0..50).any(|_| !q.tick(&mut rng, Vec3::new(232.0, 10.0, 32.0), &flat)));
    }
}
