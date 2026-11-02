//! M22 boats: `EntityBoat` (+ `ItemBoat` in `lib.rs`). A 1.5 x 0.6 box that floats (buoyancy from 5 slices of its height), is paddled
//! by the rider's own walking (`motion += rider.motion * 0.2`, clamped to 0.4 a tick), turns towards where it is going (at most
//! 20 degrees a tick), shoves other boats, wipes snow layers off its path and is smashed by a wall hit while it still moves faster
//! than 0.15 a tick along the wall (3 planks and 2 sticks drop): `moveEntity` zeroes the motion of the axis that hit, so a head-on
//! crash stops it and only a slanting one breaks it, exactly as in b1.7.3.
//! Units are the Java ones: blocks and blocks per 20 Hz tick, yaw in degrees (the direction the boat came from, `atan2(dz, dx)`).
//! Driving (NOT b1.7.3, whose boat is paddled by the rider's walking): forward / back is the throttle, sideways only turns the hull (`Boat::drive`); the
//! velocity is pulled onto the heading by a keel grip so a turn is an arc, and the input is low-passed so a d-pad press eases in. `yaw` is the heading + 90 degrees.
//! ponytail: no wake splashes, no `handleWaterMovement` (flowing water does not push it; a stream's current from `gen::hydro` does), a move is made in 5 cm steps instead of
//! the exact `moveEntity` snap, a ridden boat does not smash on a wall (it slides), boats are not saved, the paddles of the modern model (`render::items::push_boat`, Bedrock `boat_oak.png`) rest and do not row.

use glam::Vec3;

use crate::world::items::ItemStack;
use crate::world::physics::{aabb_hits_solid, BlockQuery};

/// Half of the 1.5 x 0.6 x 1.5 box.
pub const HALF: Vec3 = Vec3::new(0.75, 0.3, 0.75);
const TICK: f32 = 1.0 / 20.0;
pub const PLANKS: u16 = 5;
pub const STICK: u16 = 280;
/// Top speed ahead / astern (blocks a tick), the hull's turn at full lock (degrees a tick), and the fraction of sideways speed the keel keeps each tick.
const MAX_AHEAD: f32 = 0.30;
const MAX_ASTERN: f32 = 0.12;
const TURN: f32 = 3.5;
const KEEL: f32 = 0.82;

pub enum Event {
    /// An item the boat drops where it was (`dropItemWithOffset`).
    Drop(ItemStack, Vec3),
    /// A snow layer (78) in its way is wiped.
    Snow((i32, i32, i32)),
}

/// `Entity.moveEntity` for a box with no step height: y, then x, then z, each as far as it goes (in 5 cm steps). Returns the new
/// centre and which axes hit something.
pub fn sweep(pos: Vec3, half: Vec3, d: Vec3, get: BlockQuery<'_>) -> (Vec3, [bool; 3]) {
    let (mut p, mut hit) = (pos, [false; 3]);
    for a in [1usize, 0, 2] {
        let n = (d[a].abs() / 0.05).ceil() as i32;
        for _ in 0..n {
            let mut q = p;
            q[a] += d[a] / n as f32;
            if aabb_hits_solid(q, half, get) {
                hit[a] = true;
                break;
            }
            p = q;
        }
    }
    (p, hit)
}

#[derive(Clone)]
pub struct Boat {
    /// The centre of the box (`posY` is `minY + height / 2`).
    pub pos: Vec3,
    pub prev: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    /// `yaw` one tick ago, so the frame can draw (and the rider can sit) between the two.
    pub prev_yaw: f32,
    /// The rider's input after easing: forward (-1 back .. 1 ahead) and turn (-1 left .. 1 right).
    throttle: f32,
    steer: f32,
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
        Self { pos, prev: pos, vel: Vec3::ZERO, yaw: 0.0, prev_yaw: 0.0, throttle: 0.0, steer: 0.0, damage: 0, hit_time: 0, rock: 1, on_ground: false, collided: false, ridden: false }
    }

    /// `attackEntityFrom`: it rocks and takes `dmg * 10`; true when that is over 40 and it breaks (the caller drops and removes it).
    pub fn hurt(&mut self, dmg: i32) -> bool {
        self.rock = -self.rock;
        self.hit_time = 10;
        self.damage += dmg * 10;
        self.damage > 40
    }

    /// Where the bow points, as the camera's yaw (radians: 0 looks along -z, turning right adds). `yaw` is the direction the boat came from.
    pub fn heading(&self) -> f32 {
        (self.yaw - 90.0).to_radians()
    }

    /// `heading` `a` of the way from the last tick to this one.
    pub fn heading_at(&self, a: f32) -> f32 {
        (self.yaw_at(a) - 90.0).to_radians()
    }

    pub fn yaw_at(&self, a: f32) -> f32 {
        self.prev_yaw + (self.yaw - self.prev_yaw) * a
    }

    /// Turn the bow to `h` (a camera yaw) at once, for a rider who gets in: the boat faces where he looks and does not spin to it.
    pub fn set_heading(&mut self, h: f32) {
        self.yaw = h.to_degrees() + 90.0;
        self.prev_yaw = self.yaw;
        (self.throttle, self.steer) = (0.0, 0.0);
    }

    /// `updateRiderPosition`: where the rider's body (box centre) goes: 0.4 behind the middle, feet 0.8 under it
    /// (`getMountedYOffset` -0.3, the player's `getYOffset` 1.12, eye 1.62 over the feet). `a` of the way between the last tick and this one,
    /// as the boat is drawn, so the rider does not step at 20 Hz while the hull glides.
    pub fn rider_pos(&self, rider_half_y: f32, a: f32) -> Vec3 {
        let (p, r) = (self.prev.lerp(self.pos, a), self.yaw_at(a).to_radians());
        Vec3::new(p.x + r.cos() * 0.4, p.y - 0.3 + 1.12 - 1.62 + rider_half_y, p.z + r.sin() * 0.4)
    }

    /// One tick of driving: `fwd` / `side` are the rider's stick or d-pad (-1..1; a diagonal is 0.71 of each). Ahead / astern is the throttle, the speed eases to
    /// `throttle * top speed`; the side only turns the bow (slowly when the boat is still, at full lock once it moves); the sideways part of the velocity is damped
    /// (the keel), so the boat follows its bow along an arc instead of sliding.
    fn drive(&mut self, fwd: f32, side: f32) {
        self.throttle += (fwd - self.throttle) * 0.25;
        self.steer += (side - self.steer) * 0.25;
        let (sh, ch) = self.heading().sin_cos();
        let along = self.vel.x * sh - self.vel.z * ch;
        self.yaw += self.steer * TURN * (0.4 + 0.6 * (along.abs() / 0.15).min(1.0));
        let (sh, ch) = self.heading().sin_cos();
        let (mut along, mut across) = (self.vel.x * sh - self.vel.z * ch, self.vel.x * ch + self.vel.z * sh);
        let target = self.throttle * if self.throttle >= 0.0 { MAX_AHEAD } else { MAX_ASTERN };
        along += (target - along) * if target.abs() > along.abs() { 0.06 } else { 0.035 };
        across *= KEEL;
        self.vel.x = sh * along + ch * across;
        self.vel.z = -ch * along + sh * across;
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

    /// `moveEntity`: the motion of an axis that hit is lost, `collided` and `on_ground` are set from what hit.
    fn move_by(&mut self, get: BlockQuery<'_>, d: Vec3) {
        let (p, hit) = sweep(self.pos, HALF, d, get);
        self.pos = p;
        for a in 0..3 {
            if hit[a] {
                self.vel[a] = 0.0;
            }
        }
        self.collided = hit[0] || hit[2];
        self.on_ground = hit[1] && d.y < 0.0;
    }

    /// `EntityBoat.onUpdate`, one 20 Hz tick. `rider` is the rider's (forward, side) input, if any. False: it broke.
    fn tick(&mut self, get: BlockQuery<'_>, rider: Option<(f32, f32)>, out: &mut Vec<Event>) -> bool {
        self.hit_time = (self.hit_time - 1).max(0);
        self.damage = (self.damage - 1).max(0);
        self.prev = self.pos;
        self.prev_yaw = self.yaw;
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
        // A stream's current pushes a boat afloat (blocks a tick; a rider's paddling is ten times stronger, so it can go upstream).
        if wet > 0.0 {
            if let Some(c) = crate::world::gen::hydro::current(self.pos.x, self.pos.z) {
                self.vel.x += c[0] / 20.0 * 0.015;
                self.vel.z += c[1] / 20.0 * 0.015;
            }
        }
        match rider {
            Some((f, s)) => self.drive(f, s),
            None => (self.throttle, self.steer) = (0.0, 0.0),
        }
        self.vel.x = self.vel.x.clamp(-0.4, 0.4);
        self.vel.z = self.vel.z.clamp(-0.4, 0.4);
        if self.on_ground {
            self.vel *= 0.5;
        }
        self.move_by(get, self.vel);
        let speed = (self.vel.x * self.vel.x + self.vel.z * self.vel.z).sqrt();
        if self.collided && speed > 0.15 && rider.is_none() {
            out.push(Event::Drop(ItemStack { id: PLANKS, count: 3, damage: 0 }, self.pos));
            out.push(Event::Drop(ItemStack { id: STICK, count: 2, damage: 0 }, self.pos));
            return false;
        }
        self.vel *= if rider.is_some() { Vec3::new(1.0, 0.95, 1.0) } else { Vec3::new(0.99, 0.95, 0.99) };
        // Turn towards where it came from (the model's front is the other way), at most 20 degrees a tick; a driven boat is turned by `drive`.
        if rider.is_none() {
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
        }
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
        // The boat the player sits in is not a target (the eye is inside its box).
        self.list.iter().enumerate().filter(|(_, b)| !b.ridden).filter_map(|(i, b)| b.hit(eye, dir, reach).map(|t| (i, t))).min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Run the 20 Hz ticks `dt` covers. `rider` is the (forward, side) input of the player in a boat.
    pub fn update(&mut self, dt: f32, get: BlockQuery<'_>, rider: (f32, f32)) -> Vec<Event> {
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

    fn run(b: &mut Boats, ticks: usize, rider: (f32, f32), get: BlockQuery<'_>) -> Vec<Event> {
        (0..ticks).flat_map(|_| b.update(TICK, get, rider)).collect()
    }

    /// Dropped into the water it bobs up and settles with the water line through its box.
    #[test]
    fn floats_and_settles() {
        let mut b = Boats { list: vec![Boat::new(32.0, 61.0, 32.0)], acc: 0.0 };
        run(&mut b, 200, (0.0, 0.0), &pond);
        let y = b.list[0].pos.y;
        assert!((63.5..64.5).contains(&y) && b.list[0].vel.length() < 0.05, "settled at the surface: y = {y}, v = {}", b.list[0].vel);
    }

    /// A seated boat bows to -z (heading 0). Forward drives it along the bow and eases to a stop, back reverses slower, the side alone
    /// only turns it on the spot (right = a larger heading), and forward + right is an arc: the velocity follows the bow within a few
    /// degrees' lag. Python twin: `tools/boat_proto.py` numbers (80 ticks ahead = 18.4 blocks, 40 ticks of lock = 52 degrees).
    #[test]
    fn forward_drives_back_reverses_and_the_side_only_turns() {
        let seat = |h: f32| {
            let mut b = Boats { list: vec![Boat::new(32.0, 63.0, 32.0)], acc: 0.0 };
            run(&mut b, 100, (0.0, 0.0), &pond);
            b.list[0].ridden = true;
            b.list[0].set_heading(h);
            b
        };
        let start = seat(0.0).list[0].pos;
        let mut b = seat(0.0);
        run(&mut b, 80, (1.0, 0.0), &pond);
        let (d, v) = (b.list[0].pos - start, b.list[0].vel);
        assert!(d.z < -15.0 && d.x.abs() < 0.05, "ahead along -z: {d}");
        assert!(v.length() > 0.25 && v.length() <= MAX_AHEAD + 1e-3, "near top speed: {}", v.length());
        run(&mut b, 100, (0.0, 0.0), &pond);
        assert!(b.list[0].vel.length() < 0.02, "it coasts to a stop: {}", b.list[0].vel.length());
        let mut b = seat(0.0);
        run(&mut b, 80, (-1.0, 0.0), &pond);
        let (d, v) = (b.list[0].pos - start, b.list[0].vel);
        assert!(d.z > 5.0 && d.z < 9.0 && v.length() <= MAX_ASTERN + 1e-3, "astern, slower: {d}, {}", v.length());
        let mut b = seat(0.0);
        run(&mut b, 40, (0.0, 1.0), &pond);
        let turned = b.list[0].heading().to_degrees();
        assert!((40.0..60.0).contains(&turned) && Vec3::new(b.list[0].pos.x - start.x, 0.0, b.list[0].pos.z - start.z).length() < 0.05, "turns on the spot, to the right: {turned}");
        let mut b = seat(0.0);
        run(&mut b, 120, (0.7071, 0.7071), &pond);
        let (h, v) = (b.list[0].heading(), b.list[0].vel);
        let course = v.x.atan2(-v.z);
        let lag = ((h - course).sin()).asin().to_degrees().abs();
        assert!(h > 3.0 && b.list[0].pos.x > start.x + 2.0 && lag < 20.0, "an arc to the right that the hull follows: heading {h}, lag {lag}");
    }

    /// The speed never jumps: a full d-pad press from rest changes it by under 0.01 a tick, and the drawn yaw lies between the two ticks.
    #[test]
    fn input_eases_in_and_the_frame_can_interpolate() {
        let mut b = Boats { list: vec![Boat::new(32.0, 63.0, 32.0)], acc: 0.0 };
        run(&mut b, 100, (0.0, 0.0), &pond);
        b.list[0].ridden = true;
        b.list[0].set_heading(0.0);
        let mut last = 0.0f32;
        for _ in 0..60 {
            run(&mut b, 1, (0.7071, 0.7071), &pond);
            let s = (b.list[0].vel.x.powi(2) + b.list[0].vel.z.powi(2)).sqrt();
            assert!((s - last).abs() < 0.01, "a step of {}", (s - last).abs());
            last = s;
        }
        let bt = &b.list[0];
        assert!(bt.yaw != bt.prev_yaw && (bt.yaw_at(0.5) - (bt.yaw + bt.prev_yaw) / 2.0).abs() < 1e-4 && bt.yaw_at(0.0) == bt.prev_yaw && (bt.yaw_at(1.0) - bt.yaw).abs() < 1e-3);
    }

    /// The boat the player sits in is not a target of the crosshair; a driven boat slides along a wall instead of smashing.
    #[test]
    fn a_ridden_boat_is_not_picked_and_does_not_smash() {
        let mut b = Boats { list: vec![Boat::new(30.0, 63.0, 32.0)], acc: 0.0 };
        assert!(b.pick(Vec3::new(20.0, 63.5, 32.0), Vec3::X, 100.0).is_some());
        b.list[0].ridden = true;
        assert!(b.pick(Vec3::new(20.0, 63.5, 32.0), Vec3::X, 100.0).is_none());
        let walled = |x: i32, y: i32, z: i32| if x >= 40 && y >= 63 { Some(1) } else { pond(x, y, z) };
        let mut b = Boats { list: vec![Boat::new(34.0, 63.9, 32.0)], acc: 0.0 };
        b.list[0].ridden = true;
        b.list[0].set_heading(std::f32::consts::FRAC_PI_2); // bow to +x
        run(&mut b, 120, (1.0, 0.0), &walled);
        assert_eq!(b.list.len(), 1, "a driven boat is not smashed by a wall");
        assert!(b.list[0].pos.x + HALF.x <= 40.0 + 1e-3, "it stopped at the wall: {}", b.list[0].pos.x);
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
        run(&mut b, 10, (0.0, 0.0), &air);
        assert!(b.list[0].damage == 0 && b.list[0].hit_time == 0);
    }

    /// A slanting crash at over 0.15 along the wall smashes it into 3 planks and 2 sticks; a head-on one (the wall zeroes the only
    /// motion it has) and a gentle one just stop it.
    #[test]
    fn a_hard_slanting_crash_breaks_it_and_others_do_not() {
        let walled = |x: i32, y: i32, z: i32| if x >= 40 && y >= 64 { Some(1) } else { pond(x, y, z) };
        let mut b = Boats { list: vec![Boat::new(36.0, 63.9, 32.0)], acc: 0.0 };
        b.list[0].vel = Vec3::new(0.3, 0.0, 0.3);
        let ev = run(&mut b, 40, (0.0, 0.0), &walled);
        assert!(b.list.is_empty(), "smashed");
        let planks: u8 = ev.iter().filter_map(|e| if let Event::Drop(s, _) = e { Some(s) } else { None }).filter(|s| s.id == PLANKS).map(|s| s.count).sum();
        let sticks: u8 = ev.iter().filter_map(|e| if let Event::Drop(s, _) = e { Some(s) } else { None }).filter(|s| s.id == STICK).map(|s| s.count).sum();
        assert_eq!((planks, sticks), (3, 2));
        let mut b = Boats { list: vec![Boat::new(36.0, 63.9, 32.0)], acc: 0.0 };
        b.list[0].vel.x = 0.3;
        run(&mut b, 200, (0.0, 0.0), &walled);
        assert_eq!(b.list.len(), 1, "a head-on crash leaves it whole");
        let mut b = Boats { list: vec![Boat::new(36.0, 63.9, 32.0)], acc: 0.0 };
        b.list[0].vel = Vec3::new(0.1, 0.0, 0.1);
        run(&mut b, 200, (0.0, 0.0), &walled);
        assert_eq!(b.list.len(), 1, "a gentle bump leaves it whole");
        assert!(b.list[0].pos.x + HALF.x <= 40.0 + 1e-3, "it stopped at the wall: {}", b.list[0].pos.x);
    }

    /// Two boats overlapping are pushed apart; the ray finds the nearer; the rider sits 0.4 behind the middle.
    #[test]
    fn boats_push_each_other_and_can_be_picked() {
        let mut b = Boats { list: vec![Boat::new(30.0, 63.0, 32.0), Boat::new(30.8, 63.0, 32.0)], acc: 0.0 };
        run(&mut b, 30, (0.0, 0.0), &pond);
        assert!(b.list[1].pos.x - b.list[0].pos.x > 0.8 + 0.05, "pushed apart: {}", b.list[1].pos.x - b.list[0].pos.x);
        let t = b.pick(Vec3::new(20.0, b.list[0].pos.y, 32.0), Vec3::X, 100.0);
        assert!(matches!(t, Some((0, d)) if (d - (b.list[0].pos.x - HALF.x - 20.0)).abs() < 1e-3));
        let mut one = Boat::new(10.0, 64.0, 10.0);
        one.yaw = 0.0;
        assert!((one.rider_pos(0.9, 1.0) - Vec3::new(10.4, 64.3 + 0.1, 10.0)).length() < 1e-4);
    }
}
