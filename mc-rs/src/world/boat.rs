//! M22 boats: `EntityBoat` (+ `ItemBoat` in `lib.rs`). A 1.5 x 0.6 box that floats (buoyancy from 5 slices of its height), is paddled
//! by the rider's own walking (`motion += rider.motion * 0.2`, clamped to 0.4 a tick), turns towards where it is going (at most
//! 20 degrees a tick), shoves other boats, wipes snow layers off its path and is smashed by a wall hit while it still moves faster
//! than 0.15 a tick along the wall (3 planks and 2 sticks drop): `moveEntity` zeroes the motion of the axis that hit, so a head-on
//! crash stops it and only a slanting one breaks it, exactly as in b1.7.3.
//! Units are the Java ones: blocks and blocks per 20 Hz tick, yaw in degrees (the direction the boat came from, `atan2(dz, dx)`).
//! ponytail: no wake splashes, no `handleWaterMovement` (flowing water does not push it), a move is made in 5 cm steps instead of
//! the exact `moveEntity` snap, boats are not saved, no boat.png (the model is drawn in planks).

use glam::Vec3;

use crate::world::items::ItemStack;
use crate::world::physics::{aabb_hits_solid, BlockQuery};

/// Half of the 1.5 x 0.6 x 1.5 box.
pub const HALF: Vec3 = Vec3::new(0.75, 0.3, 0.75);
const TICK: f32 = 1.0 / 20.0;
pub const PLANKS: u16 = 5;
pub const STICK: u16 = 280;

pub enum Event {
    /// An item the boat drops where it was (`dropItemWithOffset`).
    Drop(ItemStack, Vec3),
    /// A snow layer (78) in its way is wiped.
    Snow((i32, i32, i32)),
}

#[derive(Clone)]
pub struct Boat {
    /// The centre of the box (`posY` is `minY + height / 2`).
    pub pos: Vec3,
    pub prev: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub damage: i32,
    pub hit_time: i32,
    pub rock: i32,
    pub on_ground: bool,
    pub collided: bool,
    pub ridden: bool,
}

impl Boat {
    /// `EntityBoat(world, x, y, z)`: the bottom of the box rests at `y`.
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        let pos = Vec3::new(x, y + HALF.y, z);
        Self { pos, prev: pos, vel: Vec3::ZERO, yaw: 0.0, damage: 0, hit_time: 0, rock: 1, on_ground: false, collided: false, ridden: false }
    }

    /// `attackEntityFrom`: it rocks and takes `dmg * 10`; true when that is over 40 and it breaks (the caller drops and removes it).
    pub fn hurt(&mut self, dmg: i32) -> bool {
        self.rock = -self.rock;
        self.hit_time = 10;
        self.damage += dmg * 10;
        self.damage > 40
    }

    /// `updateRiderPosition`: where the rider's body (box centre) goes: 0.4 behind the middle, feet 0.8 under it
    /// (`getMountedYOffset` -0.3, the player's `getYOffset` 1.12, eye 1.62 over the feet).
    pub fn rider_pos(&self, rider_half_y: f32) -> Vec3 {
        let a = self.yaw.to_radians();
        Vec3::new(self.pos.x + a.cos() * 0.4, self.pos.y - 0.3 + 1.12 - 1.62 + rider_half_y, self.pos.z + a.sin() * 0.4)
    }

    /// A ray against the box: the distance along `dir` (a unit vector) where it is entered, within `reach`.
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

    /// `World.isAABBInMaterial(slice, water)`: the slice overlaps a water cell (the Java test reduces to that).
    fn slice_in_water(&self, get: BlockQuery<'_>, lo_y: f32, hi_y: f32) -> bool {
        let (lo, hi) = (self.pos - HALF, self.pos + HALF);
        let fl = |v: f32| v.floor() as i32;
        (fl(lo.x)..fl(hi.x + 1.0)).any(|x| (fl(lo_y)..fl(hi_y + 1.0)).any(|y| (fl(lo.z)..fl(hi.z + 1.0)).any(|z| matches!(get(x, y, z), Some(8 | 9)))))
    }

    /// `moveEntity` for a box with no step height: y, then x, then z, each as far as it goes.
    fn move_by(&mut self, get: BlockQuery<'_>, d: Vec3) {
        let (mut hit_xz, mut hit_y) = (false, false);
        for a in [1usize, 0, 2] {
            let n = (d[a].abs() / 0.05).ceil() as i32;
            for _ in 0..n {
                let mut p = self.pos;
                p[a] += d[a] / n as f32;
                if aabb_hits_solid(p, HALF, get) {
                    self.vel[a] = 0.0;
                    if a == 1 { hit_y = true } else { hit_xz = true }
                    break;
                }
                self.pos = p;
            }
        }
        self.collided = hit_xz;
        self.on_ground = hit_y && d.y < 0.0;
    }

    /// `EntityBoat.onUpdate`, one 20 Hz tick. `rider` is the rider's own motion this tick (blocks per tick). False: it broke.
    fn tick(&mut self, get: BlockQuery<'_>, rider: Option<Vec3>, out: &mut Vec<Event>) -> bool {
        self.hit_time = (self.hit_time - 1).max(0);
        self.damage = (self.damage - 1).max(0);
        self.prev = self.pos;
        let min_y = self.pos.y - HALF.y;
        let wet = (0..5).filter(|&i| self.slice_in_water(get, min_y + 0.6 * i as f32 / 5.0 - 0.125, min_y + 0.6 * (i + 1) as f32 / 5.0 - 0.125)).count() as f32 / 5.0;
        if wet < 1.0 {
            self.vel.y += 0.04 * (wet * 2.0 - 1.0);
        } else {
            if self.vel.y < 0.0 {
                self.vel.y /= 2.0;
            }
            self.vel.y += 0.007;
        }
        if let Some(r) = rider {
            self.vel.x += r.x * 0.2;
            self.vel.z += r.z * 0.2;
        }
        self.vel.x = self.vel.x.clamp(-0.4, 0.4);
        self.vel.z = self.vel.z.clamp(-0.4, 0.4);
        if self.on_ground {
            self.vel *= 0.5;
        }
        self.move_by(get, self.vel);
        let speed = (self.vel.x * self.vel.x + self.vel.z * self.vel.z).sqrt();
        if self.collided && speed > 0.15 {
            out.push(Event::Drop(ItemStack { id: PLANKS, count: 3, damage: 0 }, self.pos));
            out.push(Event::Drop(ItemStack { id: STICK, count: 2, damage: 0 }, self.pos));
            return false;
        }
        self.vel *= Vec3::new(0.99, 0.95, 0.99);
        // Turn towards where it came from (the model's front is the other way), at most 20 degrees a tick.
        let (dx, dz) = (self.prev.x - self.pos.x, self.prev.z - self.pos.z);
        let mut target = self.yaw;
        if dx * dx + dz * dz > 0.001 {
            target = dz.atan2(dx).to_degrees();
        }
        let mut d = target - self.yaw;
        while d >= 180.0 {
            d -= 360.0;
        }
        while d < -180.0 {
            d += 360.0;
        }
        self.yaw += d.clamp(-20.0, 20.0);
        for i in 0..4 {
            let cell = ((self.pos.x + ((i % 2) as f32 - 0.5) * 0.8).floor() as i32, self.pos.y.floor() as i32, (self.pos.z + ((i / 2) as f32 - 0.5) * 0.8).floor() as i32);
            if get(cell.0, cell.1, cell.2) == Some(78) {
                out.push(Event::Snow(cell));
            }
        }
        true
    }
}

#[derive(Default)]
pub struct Boats {
    pub list: Vec<Boat>,
    acc: f32,
}

impl Boats {
    /// How far into the next tick the frame is, for drawing between `prev` and `pos`.
    pub fn alpha(&self) -> f32 {
        (self.acc / TICK).clamp(0.0, 1.0)
    }

    pub fn ridden(&self) -> Option<&Boat> {
        self.list.iter().find(|b| b.ridden)
    }

    /// The nearest boat the ray enters within `reach`: its index and the distance.
    pub fn pick(&self, eye: Vec3, dir: Vec3, reach: f32) -> Option<(usize, f32)> {
        self.list.iter().enumerate().filter_map(|(i, b)| b.hit(eye, dir, reach).map(|t| (i, t))).min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Run the 20 Hz ticks `dt` covers. `rider` is the motion of the player in a boat (blocks per tick), or none.
    pub fn update(&mut self, dt: f32, get: BlockQuery<'_>, rider: Vec3) -> Vec<Event> {
        let mut out = Vec::new();
        self.acc = (self.acc + dt).min(2.0 * TICK);
        while self.acc >= TICK {
            self.acc -= TICK;
            let mut i = 0;
            while i < self.list.len() {
                let r = self.list[i].ridden.then_some(rider);
                if self.list[i].tick(get, r, &mut out) {
                    self.shove(i);
                    i += 1;
                } else {
                    self.list.remove(i);
                }
            }
        }
        out
    }

    /// `Entity.applyEntityCollision` for every other boat within 0.2 of this one's box (sideways): the pair is pushed apart.
    fn shove(&mut self, i: usize) {
        for j in 0..self.list.len() {
            if j == i {
                continue;
            }
            let (a, b) = (&self.list[i], &self.list[j]);
            let near = (a.pos.x - b.pos.x).abs() < 2.0 * HALF.x + 0.2 && (a.pos.y - b.pos.y).abs() < 2.0 * HALF.y && (a.pos.z - b.pos.z).abs() < 2.0 * HALF.z + 0.2;
            if !near {
                continue;
            }
            let (mut dx, mut dz) = (a.pos.x - b.pos.x, a.pos.z - b.pos.z);
            let m = dx.abs().max(dz.abs());
            if m >= 0.01 {
                let m = m.sqrt();
                let k = (1.0 / m).min(1.0) * 0.05 / m;
                (dx, dz) = (dx * k, dz * k);
                self.list[i].vel += Vec3::new(dx, 0.0, dz);
                self.list[j].vel -= Vec3::new(dx, 0.0, dz);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stone below y = 60, water in 60..64, air above: a pond 64 deep in x and z.
    fn pond(x: i32, y: i32, z: i32) -> Option<u8> {
        if !(0..64).contains(&x) || !(0..64).contains(&z) {
            return Some(1);
        }
        Some(match y {
            ..=59 => 1,
            60..=63 => 9,
            _ => 0,
        })
    }

    fn run(b: &mut Boats, ticks: usize, rider: Vec3, get: BlockQuery<'_>) -> Vec<Event> {
        (0..ticks).flat_map(|_| b.update(TICK, get, rider)).collect()
    }

    /// Dropped into the water it bobs up and settles with the water line through its box; a rider's walking paddles it and it
    /// turns to face where it goes.
    #[test]
    fn floats_settles_and_is_paddled() {
        let mut b = Boats { list: vec![Boat::new(32.0, 61.0, 32.0)], acc: 0.0 };
        run(&mut b, 200, Vec3::ZERO, &pond);
        let y = b.list[0].pos.y;
        assert!((63.5..64.5).contains(&y) && b.list[0].vel.length() < 0.05, "settled at the surface: y = {y}, v = {}", b.list[0].vel);
        b.list[0].ridden = true;
        run(&mut b, 60, Vec3::new(0.0, 0.0, 0.2), &pond);
        assert!(b.list[0].pos.z > 34.0 && b.list[0].vel.z <= 0.4 + 1e-6, "paddled along +z: z = {}", b.list[0].pos.z);
        assert!((b.list[0].yaw - 90.0).abs() > 60.0, "and turned its back to the way it came (|yaw| near 90 from the z axis): {}", b.list[0].yaw);
    }

    /// Hit five times with bare hands (4 x 10 = 40 is not over 40) it breaks; the damage heals a point a tick.
    #[test]
    fn five_punches_break_it_and_damage_heals() {
        let mut b = Boat::new(5.0, 64.0, 5.0);
        assert!((0..4).all(|_| !b.hurt(1)));
        assert!(b.hurt(1));
        let mut b = Boats { list: vec![Boat::new(5.0, 64.0, 5.0)], acc: 0.0 };
        b.list[0].hurt(1);
        let air = |_: i32, y: i32, _: i32| Some(if y < 40 { 1u8 } else { 0 });
        run(&mut b, 10, Vec3::ZERO, &air);
        assert!(b.list[0].damage == 0 && b.list[0].hit_time == 0);
    }

    /// A slanting crash at over 0.15 along the wall smashes it into 3 planks and 2 sticks; a head-on one (the wall zeroes the only
    /// motion it has) and a gentle one just stop it.
    #[test]
    fn a_hard_slanting_crash_breaks_it_and_others_do_not() {
        let walled = |x: i32, y: i32, z: i32| if x >= 40 && y >= 64 { Some(1) } else { pond(x, y, z) };
        let mut b = Boats { list: vec![Boat::new(36.0, 63.9, 32.0)], acc: 0.0 };
        b.list[0].vel = Vec3::new(0.3, 0.0, 0.3);
        let ev = run(&mut b, 40, Vec3::ZERO, &walled);
        assert!(b.list.is_empty(), "smashed");
        let planks: u8 = ev.iter().filter_map(|e| if let Event::Drop(s, _) = e { Some(s) } else { None }).filter(|s| s.id == PLANKS).map(|s| s.count).sum();
        let sticks: u8 = ev.iter().filter_map(|e| if let Event::Drop(s, _) = e { Some(s) } else { None }).filter(|s| s.id == STICK).map(|s| s.count).sum();
        assert_eq!((planks, sticks), (3, 2));
        let mut b = Boats { list: vec![Boat::new(36.0, 63.9, 32.0)], acc: 0.0 };
        b.list[0].vel.x = 0.3;
        run(&mut b, 200, Vec3::ZERO, &walled);
        assert_eq!(b.list.len(), 1, "a head-on crash leaves it whole");
        let mut b = Boats { list: vec![Boat::new(36.0, 63.9, 32.0)], acc: 0.0 };
        b.list[0].vel = Vec3::new(0.1, 0.0, 0.1);
        run(&mut b, 200, Vec3::ZERO, &walled);
        assert_eq!(b.list.len(), 1, "a gentle bump leaves it whole");
        assert!(b.list[0].pos.x + HALF.x <= 40.0 + 1e-3, "it stopped at the wall: {}", b.list[0].pos.x);
    }

    /// Two boats overlapping are pushed apart; the ray finds the nearer; the rider sits 0.4 behind the middle.
    #[test]
    fn boats_push_each_other_and_can_be_picked() {
        let mut b = Boats { list: vec![Boat::new(30.0, 63.0, 32.0), Boat::new(30.8, 63.0, 32.0)], acc: 0.0 };
        run(&mut b, 30, Vec3::ZERO, &pond);
        assert!(b.list[1].pos.x - b.list[0].pos.x > 0.8 + 0.05, "pushed apart: {}", b.list[1].pos.x - b.list[0].pos.x);
        let t = b.pick(Vec3::new(20.0, b.list[0].pos.y, 32.0), Vec3::X, 100.0);
        assert!(matches!(t, Some((0, d)) if (d - (b.list[0].pos.x - HALF.x - 20.0)).abs() < 1e-3));
        let mut one = Boat::new(10.0, 64.0, 10.0);
        one.yaw = 0.0;
        assert!((one.rider_pos(0.9) - Vec3::new(10.4, 64.3 + 0.1, 10.0)).length() < 1e-4);
    }
}
