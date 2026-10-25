//! M22 minecarts: `EntityMinecart` (empty, type 0, with a chest, type 1, and with a furnace, type 2) and `ItemMinecart` (in `lib.rs`).
//! On a rail the cart follows the rail's line exactly (`func_514_g` finds the point on it, the speed is turned to run along it,
//! slopes pull it by 1/128 a tick, a powered rail that is on adds 0.06 and one that is off brakes it, a rider slows it to 75%);
//! off a rail it falls and slides like a boat on land. Units are the Java ones: blocks and blocks per 20 Hz tick, yaw in degrees.
//! The furnace cart is pushed from the side it was touched on (`pushX/Z`), burns 1 fuel in 4 ticks while it pushes and is fed coal
//! (+1200). Two carts push each other with the quirks of the Java (`prevPosX` in the closing-speed test, so they hardly ever
//! interact away from x = 0).
//! ponytail: the chest cart (type 1) is not made (its 27 slots need a screen that is not keyed by a block), mobs are not picked
//! up by a passing cart, no pitch on slopes (the cart is drawn level), carts are not saved, `largesmoke` is an ordinary puff.

use glam::Vec3;

use crate::world::boat::sweep;
use crate::world::chunk::normal_cube;
use crate::world::gen::noise::JavaRandom;
use crate::world::items::ItemStack;
use crate::world::physics::BlockQuery;

/// Half of the 0.98 x 0.7 x 0.98 box; `yOffset` is half the height.
pub const HALF: Vec3 = Vec3::new(0.49, 0.35, 0.49);
const Y_OFF: f32 = 0.35;
const TICK: f32 = 1.0 / 20.0;
pub const EMPTY: u16 = 328;
pub const WITH_FURNACE: u16 = 343;
pub const WITH_CHEST: u16 = 342;
pub const CHEST: u16 = 54;
pub const COAL: u16 = 263;
pub const FURNACE: u16 = 61;

/// `field_855_j`: the two ends of each rail shape as offsets `(x, y, z)` from the rail's cell.
const SHAPE: [[[i32; 3]; 2]; 10] = [
    [[0, 0, -1], [0, 0, 1]],
    [[-1, 0, 0], [1, 0, 0]],
    [[-1, -1, 0], [1, 0, 0]],
    [[-1, 0, 0], [1, -1, 0]],
    [[0, 0, -1], [0, -1, 1]],
    [[0, -1, -1], [0, 0, 1]],
    [[0, 0, 1], [1, 0, 0]],
    [[0, 0, 1], [-1, 0, 0]],
    [[0, 0, -1], [-1, 0, 0]],
    [[0, 0, -1], [1, 0, 0]],
];

pub enum Event {
    Drop(ItemStack, Vec3),
    Smoke(Vec3),
}

/// What a cart sees: the solid cells for its box, and the raw `(block id, metadata)` of every cell for the rails.
pub struct Env<'a> {
    pub get: BlockQuery<'a>,
    pub raw: &'a dyn Fn(i32, i32, i32) -> (u8, u8),
}

fn is_rail(id: u8) -> bool {
    matches!(id, 27 | 28 | 66)
}

impl Env<'_> {
    fn rail(&self, x: i32, y: i32, z: i32) -> bool {
        is_rail((self.raw)(x, y, z).0)
    }
    fn normal(&self, x: i32, y: i32, z: i32) -> bool {
        normal_cube((self.raw)(x, y, z).0)
    }
}

fn fl(v: f32) -> i32 {
    v.floor() as i32
}

/// `EntityMinecart.func_514_g`: the point of the rail under `p` that is on its line (the height is the rail's, +0.5 for a flat one).
pub fn rail_pos(e: &Env, p: Vec3) -> Option<Vec3> {
    let (x, mut y, z) = (fl(p.x), fl(p.y), fl(p.z));
    if e.rail(x, y - 1, z) {
        y -= 1;
    }
    let (id, mut meta) = (e.raw)(x, y, z);
    if !is_rail(id) {
        return None;
    }
    if matches!(id, 27 | 28) {
        meta &= 7;
    }
    let s = SHAPE[meta as usize];
    let end = |i: usize| Vec3::new(x as f32 + 0.5 + s[i][0] as f32 * 0.5, y as f32 + 0.5 + s[i][1] as f32 * 0.5, z as f32 + 0.5 + s[i][2] as f32 * 0.5);
    let (a, b) = (end(0), end(1));
    let (vx, vy, vz) = (b.x - a.x, (b.y - a.y) * 2.0, b.z - a.z);
    let (mut px, mut pz) = (p.x, p.z);
    let t = if vx == 0.0 {
        px = x as f32 + 0.5;
        pz - z as f32
    } else if vz == 0.0 {
        pz = z as f32 + 0.5;
        px - x as f32
    } else {
        ((px - a.x) * vx + (pz - a.z) * vz) * 2.0
    };
    let mut ry = a.y + vy * t;
    if vy < 0.0 {
        ry += 1.0;
    }
    if vy > 0.0 {
        ry += 0.5;
    }
    Some(Vec3::new(a.x + vx * t, ry, a.z + vz * t))
}

#[derive(Clone)]
pub struct Cart {
    pub pos: Vec3,
    pub prev: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    flip: bool,
    /// 0 empty, 2 with a furnace.
    pub kind: u8,
    pub fuel: i32,
    push: (f32, f32),
    pub damage: i32,
    pub hit_time: i32,
    pub rock: i32,
    pub ridden: bool,
    on_ground: bool,
}

impl Cart {
    /// `EntityMinecart(world, x, y, z, type)`: `(x, y, z)` is half a block over the rail's cell; the centre is `yOffset` (0.35) above that.
    pub fn new(x: f32, y: f32, z: f32, kind: u8) -> Self {
        let pos = Vec3::new(x, y + Y_OFF, z);
        Self { pos, prev: pos, vel: Vec3::ZERO, yaw: 0.0, flip: false, kind, fuel: 0, push: (0.0, 0.0), damage: 0, hit_time: 0, rock: 1, ridden: false, on_ground: false }
    }

    /// `attackEntityFrom`: rocks, takes `dmg * 10`; true when over 40 and it breaks.
    pub fn hurt(&mut self, dmg: i32) -> bool {
        self.rock = -self.rock;
        self.hit_time = 10;
        self.damage += dmg * 10;
        self.damage > 40
    }

    /// What it drops when it breaks: the cart, and for a furnace cart the furnace, for a chest cart the chest
    /// (`EntityMinecart.attackEntityFrom`; the chest's 27 slots are not kept yet).
    pub fn drops(&self) -> Vec<ItemStack> {
        let one = |id| ItemStack { id, count: 1, damage: 0 };
        let mut v = vec![one(EMPTY)];
        if self.kind == 2 {
            v.push(one(FURNACE));
        }
        if self.kind == 1 {
            v.push(one(CHEST));
        }
        v
    }

    /// `interact` of the furnace cart: coal is burnt (+1200) and the cart is pushed away from `player` (x, z). True: the coal went.
    pub fn push_from(&mut self, player: Vec3, coal: bool) -> bool {
        if coal {
            self.fuel += 1200;
        }
        self.push = (self.pos.x - player.x, self.pos.z - player.z);
        coal
    }

    /// `updateRiderPosition`: the rider's body centre, 0.3 under the cart's centre plus the player's 1.12 offset, eye 1.62 over the feet.
    pub fn rider_pos(&self, rider_half_y: f32) -> Vec3 {
        Vec3::new(self.pos.x, self.pos.y - 0.3 + 1.12 - 1.62 + rider_half_y, self.pos.z)
    }

    pub fn hit(&self, eye: Vec3, dir: Vec3, reach: f32) -> Option<f32> {
        let (lo, hi) = (self.pos - HALF, self.pos + HALF);
        let (mut t0, mut t1) = (0.0f32, reach);
        for a in 0..3 {
            if dir[a].abs() < 1e-8 {
                if eye[a] < lo[a] || eye[a] > hi[a] {
                    return None;
                }
            } else {
                let (mut a0, mut a1) = ((lo[a] - eye[a]) / dir[a], (hi[a] - eye[a]) / dir[a]);
                if a0 > a1 {
                    std::mem::swap(&mut a0, &mut a1);
                }
                (t0, t1) = (t0.max(a0), t1.min(a1));
                if t0 > t1 {
                    return None;
                }
            }
        }
        Some(t0)
    }

    /// Does the box overlap the part of a rail cell a detector rail looks at (`x + 2/16 .. x + 14/16`, `y .. y + 0.25`).
    pub fn over_detector(&self, (x, y, z): (i32, i32, i32)) -> bool {
        let (lo, hi) = (self.pos - HALF, self.pos + HALF);
        let (x, y, z) = (x as f32, y as f32, z as f32);
        hi.x > x + 0.125 && lo.x < x + 0.875 && hi.y > y && lo.y < y + 0.25 && hi.z > z + 0.125 && lo.z < z + 0.875
    }

    /// `moveEntity`: the motion of an axis that hit is lost.
    fn move_by(&mut self, e: &Env, d: Vec3) {
        let (p, hit) = sweep(self.pos, HALF, d, e.get);
        self.pos = p;
        for a in 0..3 {
            if hit[a] {
                self.vel[a] = 0.0;
            }
        }
        self.on_ground = hit[1] && d.y < 0.0;
    }

    /// `EntityMinecart.onUpdate`, one 20 Hz tick.
    fn tick(&mut self, e: &Env, rng: &mut JavaRandom, out: &mut Vec<Event>) {
        self.hit_time = (self.hit_time - 1).max(0);
        self.damage = (self.damage - 1).max(0);
        let start_yaw = self.yaw;
        self.prev = self.pos;
        self.vel.y -= 0.04;
        let (x0, mut y0, z0) = (fl(self.pos.x), fl(self.pos.y), fl(self.pos.z));
        if e.rail(x0, y0 - 1, z0) {
            y0 -= 1;
        }
        let cap = 0.4;
        let mut smoke = false;
        let (id, mut meta) = (e.raw)(x0, y0, z0);
        if is_rail(id) {
            let before = rail_pos(e, self.pos);
            self.pos.y = y0 as f32;
            let (on, off) = if id == 27 { (meta & 8 != 0, meta & 8 == 0) } else { (false, false) };
            if matches!(id, 27 | 28) {
                meta &= 7;
            }
            if (2..=5).contains(&meta) {
                self.pos.y = (y0 + 1) as f32;
            }
            let slope = 1.0 / 128.0;
            match meta {
                2 => self.vel.x -= slope,
                3 => self.vel.x += slope,
                4 => self.vel.z += slope,
                5 => self.vel.z -= slope,
                _ => {}
            }
            let s = SHAPE[meta as usize];
            let (mut dx, mut dz) = ((s[1][0] - s[0][0]) as f32, (s[1][2] - s[0][2]) as f32);
            let len = (dx * dx + dz * dz).sqrt();
            if self.vel.x * dx + self.vel.z * dz < 0.0 {
                (dx, dz) = (-dx, -dz);
            }
            let speed = (self.vel.x * self.vel.x + self.vel.z * self.vel.z).sqrt();
            self.vel.x = speed * dx / len;
            self.vel.z = speed * dz / len;
            if off {
                let v = (self.vel.x * self.vel.x + self.vel.z * self.vel.z).sqrt();
                if v < 0.03 {
                    self.vel = Vec3::ZERO;
                } else {
                    self.vel = Vec3::new(self.vel.x * 0.5, 0.0, self.vel.z * 0.5);
                }
            }
            // On the rail's line.
            let a = (x0 as f32 + 0.5 + s[0][0] as f32 * 0.5, z0 as f32 + 0.5 + s[0][2] as f32 * 0.5);
            let b = (x0 as f32 + 0.5 + s[1][0] as f32 * 0.5, z0 as f32 + 0.5 + s[1][2] as f32 * 0.5);
            let (rx, rz) = (b.0 - a.0, b.1 - a.1);
            let t = if rx == 0.0 {
                self.pos.x = x0 as f32 + 0.5;
                self.pos.z - z0 as f32
            } else if rz == 0.0 {
                self.pos.z = z0 as f32 + 0.5;
                self.pos.x - x0 as f32
            } else {
                ((self.pos.x - a.0) * rx + (self.pos.z - a.1) * rz) * 2.0
            };
            self.pos = Vec3::new(a.0 + rx * t, self.pos.y + Y_OFF, a.1 + rz * t);
            let (mut mx, mut mz) = (self.vel.x, self.vel.z);
            if self.ridden {
                mx *= 0.75;
                mz *= 0.75;
            }
            self.move_by(e, Vec3::new(mx.clamp(-cap, cap), 0.0, mz.clamp(-cap, cap)));
            if s[0][1] != 0 && fl(self.pos.x) - x0 == s[0][0] && fl(self.pos.z) - z0 == s[0][2] {
                self.pos.y += s[0][1] as f32;
            } else if s[1][1] != 0 && fl(self.pos.x) - x0 == s[1][0] && fl(self.pos.z) - z0 == s[1][2] {
                self.pos.y += s[1][1] as f32;
            }
            if self.ridden {
                self.vel = Vec3::new(self.vel.x * 0.997, 0.0, self.vel.z * 0.997);
            } else {
                if self.kind == 2 {
                    let p = (self.push.0 * self.push.0 + self.push.1 * self.push.1).sqrt();
                    if p > 0.01 {
                        smoke = true;
                        self.push = (self.push.0 / p, self.push.1 / p);
                        self.vel = Vec3::new(self.vel.x * 0.8 + self.push.0 * 0.04, 0.0, self.vel.z * 0.8 + self.push.1 * 0.04);
                    } else {
                        self.vel = Vec3::new(self.vel.x * 0.9, 0.0, self.vel.z * 0.9);
                    }
                }
                self.vel = Vec3::new(self.vel.x * 0.96, 0.0, self.vel.z * 0.96);
            }
            if let (Some(after), Some(before)) = (rail_pos(e, self.pos), before) {
                let climb = (before.y - after.y) * 0.05;
                let v = (self.vel.x * self.vel.x + self.vel.z * self.vel.z).sqrt();
                if v > 0.0 {
                    self.vel.x = self.vel.x / v * (v + climb);
                    self.vel.z = self.vel.z / v * (v + climb);
                }
                self.pos.y = after.y;
            }
            let (nx, nz) = (fl(self.pos.x), fl(self.pos.z));
            if nx != x0 || nz != z0 {
                let v = (self.vel.x * self.vel.x + self.vel.z * self.vel.z).sqrt();
                self.vel.x = v * (nx - x0) as f32;
                self.vel.z = v * (nz - z0) as f32;
            }
            if self.kind == 2 {
                let p = (self.push.0 * self.push.0 + self.push.1 * self.push.1).sqrt();
                if p > 0.01 && self.vel.x * self.vel.x + self.vel.z * self.vel.z > 0.001 {
                    self.push = (self.push.0 / p, self.push.1 / p);
                    self.push = if self.push.0 * self.vel.x + self.push.1 * self.vel.z < 0.0 { (0.0, 0.0) } else { (self.vel.x, self.vel.z) };
                }
            }
            if on {
                let v = (self.vel.x * self.vel.x + self.vel.z * self.vel.z).sqrt();
                if v > 0.01 {
                    self.vel.x += self.vel.x / v * 0.06;
                    self.vel.z += self.vel.z / v * 0.06;
                } else if meta == 1 {
                    if e.normal(x0 - 1, y0, z0) {
                        self.vel.x = 0.02;
                    } else if e.normal(x0 + 1, y0, z0) {
                        self.vel.x = -0.02;
                    }
                } else if meta == 0 {
                    if e.normal(x0, y0, z0 - 1) {
                        self.vel.z = 0.02;
                    } else if e.normal(x0, y0, z0 + 1) {
                        self.vel.z = -0.02;
                    }
                }
            }
        } else {
            self.vel.x = self.vel.x.clamp(-cap, cap);
            self.vel.z = self.vel.z.clamp(-cap, cap);
            if self.on_ground {
                self.vel *= 0.5;
            }
            self.move_by(e, self.vel);
            if !self.on_ground {
                self.vel *= 0.95;
            }
        }
        // Face the way it came from; a turn of nearly half a circle flips it instead of spinning it.
        let (dx, dz) = (self.prev.x - self.pos.x, self.prev.z - self.pos.z);
        if dx * dx + dz * dz > 0.001 {
            self.yaw = dz.atan2(dx).to_degrees();
            if self.flip {
                self.yaw += 180.0;
            }
        }
        let mut d = self.yaw - start_yaw;
        while d >= 180.0 {
            d -= 360.0;
        }
        while d < -180.0 {
            d += 360.0;
        }
        if !(-170.0..170.0).contains(&d) {
            self.yaw += 180.0;
            self.flip = !self.flip;
        }
        if smoke && rng.next_int_bound(4) == 0 {
            self.fuel -= 1;
            if self.fuel < 0 {
                self.push = (0.0, 0.0);
            }
            out.push(Event::Smoke(Vec3::new(self.pos.x, self.pos.y + 0.8, self.pos.z)));
        }
    }
}

/// `EntityMinecart.applyEntityCollision`: `s` is the cart that was run into, `t` the one that ran into it.
fn collide(s: &mut Cart, t: &mut Cart) {
    let (mut dx, mut dz) = (t.pos.x - s.pos.x, t.pos.z - s.pos.z);
    let d2 = dx * dx + dz * dz;
    if d2 < 1.0e-4 {
        return;
    }
    let d = d2.sqrt();
    let k = (1.0 / d).min(1.0);
    (dx, dz) = (dx / d * k * 0.1 * 0.5, dz / d * k * 0.1 * 0.5);
    // The closing-speed test of the Java, with its `prevPosX` (a world coordinate) where a velocity was meant.
    let c = ((t.pos.x - s.pos.x) * t.vel.z + (t.pos.z - s.pos.z) * t.prev.x).powi(2);
    if c > 5.0 {
        return;
    }
    let (mut vx, mut vz) = (t.vel.x + s.vel.x, t.vel.z + s.vel.z);
    if t.kind == 2 && s.kind != 2 {
        s.vel.x *= 0.2;
        s.vel.z *= 0.2;
        s.vel += Vec3::new(t.vel.x - dx, 0.0, t.vel.z - dz);
        t.vel.x *= 0.7;
        t.vel.z *= 0.7;
    } else if t.kind != 2 && s.kind == 2 {
        t.vel.x *= 0.2;
        t.vel.z *= 0.2;
        t.vel += Vec3::new(s.vel.x + dx, 0.0, s.vel.z + dz);
        s.vel.x *= 0.7;
        s.vel.z *= 0.7;
    } else {
        vx /= 2.0;
        vz /= 2.0;
        s.vel.x *= 0.2;
        s.vel.z *= 0.2;
        s.vel += Vec3::new(vx - dx, 0.0, vz - dz);
        t.vel.x *= 0.2;
        t.vel.z *= 0.2;
        t.vel += Vec3::new(vx + dx, 0.0, vz + dz);
    }
}

pub struct Carts {
    pub list: Vec<Cart>,
    acc: f32,
    rng: JavaRandom,
}

impl Default for Carts {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Carts {
    pub fn new(seed: i64) -> Self {
        Self { list: Vec::new(), acc: 0.0, rng: JavaRandom::new(seed) }
    }

    pub fn alpha(&self) -> f32 {
        (self.acc / TICK).clamp(0.0, 1.0)
    }

    pub fn ridden(&self) -> Option<&Cart> {
        self.list.iter().find(|c| c.ridden)
    }

    pub fn pick(&self, eye: Vec3, dir: Vec3, reach: f32) -> Option<(usize, f32)> {
        self.list.iter().enumerate().filter_map(|(i, c)| c.hit(eye, dir, reach).map(|t| (i, t))).min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Is any cart over the detector rail at this cell?
    pub fn over_detector(&self, cell: (i32, i32, i32)) -> bool {
        self.list.iter().any(|c| c.over_detector(cell))
    }

    /// The cells (x, y, z) a cart's box reaches, one below included: where a detector rail could be.
    pub fn cells(&self) -> Vec<(i32, i32, i32)> {
        let mut v = Vec::new();
        for c in &self.list {
            let (lo, hi) = (c.pos - HALF, c.pos + HALF);
            for x in fl(lo.x)..=fl(hi.x) {
                for y in fl(lo.y) - 1..=fl(hi.y) {
                    for z in fl(lo.z)..=fl(hi.z) {
                        v.push((x, y, z));
                    }
                }
            }
        }
        v
    }

    /// Run the 20 Hz ticks `dt` covers.
    pub fn update(&mut self, dt: f32, e: &Env) -> Vec<Event> {
        let mut out = Vec::new();
        self.acc = (self.acc + dt).min(2.0 * TICK);
        while self.acc >= TICK {
            self.acc -= TICK;
            for i in 0..self.list.len() {
                self.list[i].tick(e, &mut self.rng, &mut out);
                // `getEntitiesWithinAABBExcludingEntity(this, box.expand(0.2, 0, 0.2))`: every other cart whose box meets it.
                for j in 0..self.list.len() {
                    if j == i {
                        continue;
                    }
                    let (a, b) = (&self.list[i], &self.list[j]);
                    let near = (a.pos.x - b.pos.x).abs() < 2.0 * HALF.x + 0.2 && (a.pos.y - b.pos.y).abs() < 2.0 * HALF.y && (a.pos.z - b.pos.z).abs() < 2.0 * HALF.z + 0.2;
                    if near && !self.list[j].ridden {
                        let (s, t) = if j < i {
                            let (l, r) = self.list.split_at_mut(i);
                            (&mut l[j], &mut r[0])
                        } else {
                            let (l, r) = self.list.split_at_mut(j);
                            (&mut r[0], &mut l[i])
                        };
                        collide(s, t);
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Stone floor at y = 9 (x, z in 0..32), rails where the test puts them: `(id, meta)` at y = 10.
    struct Track(HashMap<(i32, i32), (u8, u8)>);

    impl Track {
        fn raw(&self, x: i32, y: i32, z: i32) -> (u8, u8) {
            if y == 9 && (0..32).contains(&x) && (0..32).contains(&z) {
                return (1, 0);
            }
            if y == 10 { self.0.get(&(x, z)).copied().unwrap_or((0, 0)) } else { (0, 0) }
        }
        fn run(&self, carts: &mut Carts, ticks: usize) -> Vec<Event> {
            let raw = |x: i32, y: i32, z: i32| self.raw(x, y, z);
            let get = |x: i32, y: i32, z: i32| Some(match raw(x, y, z).0 { 27 | 28 | 66 => 0, b => b });
            let e = Env { get: &get, raw: &raw };
            (0..ticks).flat_map(|_| carts.update(TICK, &e)).collect()
        }
    }

    fn line_z(id: u8, meta: u8, x: i32, z0: i32, z1: i32) -> Track {
        Track((z0..=z1).map(|z| ((x, z), (id, meta))).collect())
    }

    /// A pushed cart rolls straight down a rail, stays on its line at the rail's height and slows by 4% a tick.
    #[test]
    fn a_cart_rolls_down_a_straight_rail() {
        let t = line_z(66, 0, 5, 0, 30);
        let mut c = Carts::new(1);
        let mut cart = Cart::new(5.5, 10.5, 2.5, 0);
        cart.vel.z = 0.2;
        c.list.push(cart);
        t.run(&mut c, 40);
        let k = &c.list[0];
        assert!((k.pos.x - 5.5).abs() < 1e-3 && k.pos.z > 5.0, "on the line and moving: {:?}", k.pos);
        assert!((k.pos.y - 10.5).abs() < 0.2, "at the rail's height: {}", k.pos.y);
        assert!(k.vel.z < 0.2 * 0.96f32.powi(30) + 0.01 && k.vel.z > 0.0, "slowing: {}", k.vel.z);
    }

    /// A curve (shape 8: north and west) turns the cart from south-bound to west-bound without leaving the rail.
    #[test]
    fn a_curve_turns_the_cart() {
        let mut m: HashMap<(i32, i32), (u8, u8)> = (0..5).map(|z| ((5, z), (66, 0))).collect();
        m.insert((5, 5), (66, 8));
        (0..5).for_each(|x| {
            m.insert((x, 5), (66, 1));
        });
        let t = Track(m);
        let mut c = Carts::new(1);
        let mut cart = Cart::new(5.5, 10.5, 1.5, 0);
        cart.vel.z = 0.3;
        c.list.push(cart);
        t.run(&mut c, 80);
        let k = &c.list[0];
        assert!(k.pos.x < 4.0 && (k.pos.z - 5.5).abs() < 0.15, "went round the bend and west along z = 5: {:?}", k.pos);
    }

    /// An unpowered powered rail brakes a cart to a halt; a powered one speeds it up.
    #[test]
    fn powered_rails_brake_and_boost() {
        let off = line_z(27, 0, 5, 0, 30);
        let mut c = Carts::new(1);
        let mut cart = Cart::new(5.5, 10.5, 2.5, 0);
        cart.vel.z = 0.2;
        c.list.push(cart.clone());
        off.run(&mut c, 20);
        assert!(c.list[0].vel.length() < 0.01, "braked: {}", c.list[0].vel);
        let on = line_z(27, 8, 5, 0, 30);
        let mut c = Carts::new(1);
        c.list.push(cart);
        on.run(&mut c, 10);
        assert!(c.list[0].vel.z > 0.2, "boosted: {}", c.list[0].vel.z);
    }

    /// A furnace cart pushed from the side runs until the fuel is gone, then its push is zero and it coasts to a stop; coal tops it up.
    #[test]
    fn a_furnace_cart_burns_fuel_and_coasts() {
        let t = line_z(66, 0, 5, 0, 30);
        let mut c = Carts::new(7);
        let mut cart = Cart::new(5.5, 10.5, 2.5, 2);
        cart.fuel = 10;
        assert!(!cart.push_from(Vec3::new(5.5, 10.9, 0.0), false));
        c.list.push(cart);
        let ev = t.run(&mut c, 200);
        let k = &c.list[0];
        assert!(k.pos.z > 5.0, "it drove itself: z = {}", k.pos.z);
        assert!(k.fuel < 0 && k.push == (0.0, 0.0), "fuel gone: {} {:?}", k.fuel, k.push);
        assert!(ev.iter().any(|e| matches!(e, Event::Smoke(_))), "it smoked");
        let mut k = k.clone();
        assert!(k.push_from(Vec3::new(5.5, 10.9, 0.0), true) && k.fuel > 1000);
    }

    /// Five bare-hand hits break it (4 x 10 is not over 40); a furnace cart gives the cart and the furnace.
    #[test]
    fn hits_break_it_and_the_pieces_drop() {
        let mut k = Cart::new(5.5, 10.5, 5.5, 2);
        assert!((0..4).all(|_| !k.hurt(1)));
        assert!(k.hurt(1));
        assert_eq!(k.drops().iter().map(|s| s.id).collect::<Vec<_>>(), vec![EMPTY, FURNACE]);
        assert_eq!(Cart::new(0.0, 0.0, 0.0, 0).drops().len(), 1);
    }

    /// `func_514_g`: flat rails are 0.5 over the cell, a slope runs from the cell's floor to the top of the next.
    #[test]
    fn rail_positions_follow_the_shape() {
        let t = Track([((5, 5), (66, 0)), ((6, 5), (66, 2))].into_iter().collect());
        let raw = |x: i32, y: i32, z: i32| t.raw(x, y, z);
        let air = |_: i32, _: i32, _: i32| Some(0u8);
        let e = Env { get: &air, raw: &raw };
        let p = rail_pos(&e, Vec3::new(5.9, 10.5, 5.5)).unwrap();
        assert!((p.x - 5.5).abs() < 1e-5 && (p.y - 10.5).abs() < 1e-5, "{p:?}");
        let (lo, hi) = (rail_pos(&e, Vec3::new(6.1, 11.0, 5.5)).unwrap(), rail_pos(&e, Vec3::new(6.9, 11.0, 5.5)).unwrap());
        assert!(hi.y > lo.y, "meta 2 ascends towards +x: {} -> {}", lo.y, hi.y);
        assert!(rail_pos(&e, Vec3::new(9.5, 10.5, 9.5)).is_none());
    }

    /// The detector rail sees a cart over it, not one beside it.
    #[test]
    fn detector_sees_the_cart_over_it_only() {
        let k = Cart::new(5.5, 10.5, 5.5, 0);
        assert!(k.over_detector((5, 10, 5)) && !k.over_detector((7, 10, 5)));
    }
}
