//! The shoulder-surf view: a cinematic over-the-shoulder camera (third view button, second tap).
//!
//! NOT b1.7.3 (the Java has only the 4-block-behind third-person view, `third_pos` in `lib.rs`): this is a camera rig in the style of
//! a modern third-person game. Everything here is pure (no GPU, no world, no clock: the caller passes `dt` and a `solid` probe), so
//! `cargo test --lib shoulder` runs on a plain host, and `tools/shoulder_proto.py` is the Python twin of the maths.
//!
//! What it does, per frame (`Rig::update`):
//! - **Dolly in**: a critically damped blend 0 -> 1 glides the camera from the eye out to the shoulder; the field of view whips
//!   wider on the way (speed feel) and settles.
//! - **Shoulder anchor + lag**: the camera orbits a pivot a shoulder-width right of the eye. The pivot follows the player through
//!   springs (slower on xz than on y), so a sprint or a stop leaves the camera a little behind, then catches up. Capped at 0.6 blocks.
//! - **Aim convergence**: the camera sits off the eye's line, so the crosshair would miss what the pick ray (from the eye) hits.
//!   A small view-space turn (`Shot::view`) aims the camera's centre at the point the eye ray reaches at the aimed block's distance,
//!   the distance itself is spring-eased (a rack-focus, not a snap).
//! - **Shoulder swap**: a wall within 0.7 blocks of the preferred side and clear air on the other flips the side (0.9 s cool-down);
//!   the offset slides across behind the head.
//! - **Collision**: five rays (centre + 4 around, radius 0.28) from the eye to the wanted place; the camera stops short at once,
//!   eases back out at ~10 / s, and a sphere push-out (radius 0.3) keeps it off walls it slides along. Squeezed within 0.8 blocks of
//!   the head it reports `Shot::close` and the caller draws the hand instead of the model.
//! - **Shoulder swap**: also by hand (`Rig::swap`), swinging out wide behind the head while it crosses.
//! - **Bank**: roll into a strafe or a turn (4.6 degrees at most), spring-eased.
//! - **Impacts**: a landing dips the pivot (under-damped spring, scaled by the fall speed), a jump lifts it, a hit kicks pitch and roll
//!   with a side that alternates.
//! - **Stance**: sneak tucks in and down; zoom (the zoom button) closes to an aiming shot (nearer, wider offset); sprint pulls back
//!   and widens the view a touch.
//! - **Hand-held sway**: a few slow sines in yaw / pitch / roll, calmer the faster the player moves.
//!
//! ponytail: the swap test is two rays on each side (head and waist), not a swept sphere; the pull-in is one segment from the eye to
//! the wanted place, so it can cut a corner a swept capsule would clear. Upgrade: sphere-cast the 5 rays against the voxel boxes.

use glam::{Mat4, Vec3};

/// Substep of the under-damped spring (s): semi-implicit Euler is stable while `omega * h` stays well under 1.
const SUB: f32 = 1.0 / 240.0;
/// Ray / probe step (blocks).
const STEP: f32 = 0.1;
/// The camera's collision radius (blocks): the ray margin and the push-out reach. Over the near plane's corner at 70 degrees.
const RADIUS: f32 = 0.3;

/// One scalar spring (position `x`, velocity `v`).
#[derive(Clone, Copy, Default, Debug)]
pub struct Spring {
    pub x: f32,
    pub v: f32,
}

impl Spring {
    pub fn at(x: f32) -> Self {
        Self { x, v: 0.0 }
    }

    /// Critically damped toward `target` with natural frequency `omega` (rad/s): the exact solution `(c1 + c2 t) e^(-omega t)`, so
    /// it is right at any `dt` and never overshoots a target it starts still from.
    pub fn crit(&mut self, target: f32, omega: f32, dt: f32) {
        let d = self.x - target;
        let e = (-omega * dt).exp();
        let c2 = self.v + omega * d;
        self.x = target + (d + c2 * dt) * e;
        self.v = (self.v - omega * c2 * dt) * e;
    }

    /// Under-damped (`zeta` < 1) toward `target`: rings and settles. Substeps of `SUB`.
    pub fn bounce(&mut self, target: f32, omega: f32, zeta: f32, dt: f32) {
        let n = (dt / SUB).ceil().max(1.0);
        let h = dt / n;
        for _ in 0..n as u32 {
            self.v += (omega * omega * (target - self.x) - 2.0 * zeta * omega * self.v) * h;
            self.x += self.v * h;
        }
    }
}

fn b(x: bool) -> f32 {
    x as u8 as f32
}

/// Free distance (blocks, up to `max`) from `from` along the unit `dir` before a solid cell.
fn free(solid: &dyn Fn(Vec3) -> bool, from: Vec3, dir: Vec3, max: f32) -> f32 {
    let mut t = 0.0;
    while t < max {
        t += STEP;
        if solid(from + dir * t) {
            // Bisect the last step, so a camera pushed off a wall moves smoothly instead of in 0.1 jumps.
            let (mut lo, mut hi) = (t - STEP, t);
            for _ in 0..4 {
                let m = (lo + hi) * 0.5;
                if solid(from + dir * m) { hi = m } else { lo = m }
            }
            return lo.max(0.0);
        }
    }
    max
}

/// What the rig reads from the game each frame.
pub struct Frame<'a> {
    /// Seconds since the last frame (cut at 0.1 inside).
    pub dt: f32,
    /// The player's eye (what picking and the hand use).
    pub eye: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    /// The player's velocity in blocks/s.
    pub vel: Vec3,
    pub on_ground: bool,
    pub sneak: bool,
    pub sprint: bool,
    /// The zoom button is held: the aiming shot.
    pub zoom: bool,
    /// Health points now; a drop since the last frame is a hit.
    pub health: i32,
    /// Distance from the eye to the block the crosshair is on, if any.
    pub aim: Option<f32>,
    /// Is the cell around this world point a normal cube (the camera cannot go there)?
    pub solid: &'a dyn Fn(Vec3) -> bool,
}

/// What the renderer takes from the rig.
pub struct Shot {
    /// Where the camera sits.
    pub pos: Vec3,
    /// Multiplier for `camera.fov_y` this frame.
    pub fov_scale: f32,
    /// The camera is so close to the head (tight space, or the start of the dolly) that the player model would fill the screen:
    /// the caller leaves it out and draws the first-person hand. Hysteresis 0.8 / 1.1 blocks.
    pub close: bool,
    /// A view-space rotation (roll, convergence, kicks, sway), to go in `camera.bob`: `view = shot.view * look_at`.
    pub view: Mat4,
}

pub struct Rig {
    blend: Spring,
    pivot: [Spring; 3],
    primed: bool,
    /// +1 = camera over the right shoulder, -1 = the left.
    side: f32,
    side_s: Spring,
    swap_cool: f32,
    /// Fraction of the eye -> wanted-place segment that is free.
    frac: f32,
    focus: Spring,
    roll: Spring,
    land: Spring,
    /// `[pitch, roll]` kicks of a hit.
    kick: [Spring; 2],
    kick_sign: f32,
    sneak_s: Spring,
    zoom_s: Spring,
    sprint_s: Spring,
    speed_s: Spring,
    fov: Spring,
    clock: f32,
    prev_yaw: f32,
    prev_vy: f32,
    was_ground: bool,
    /// -1 = unknown (just reset).
    prev_health: i32,
    hidden: bool,
}

impl Default for Rig {
    fn default() -> Self {
        Self::new()
    }
}

impl Rig {
    pub fn new() -> Self {
        Self {
            blend: Spring::default(),
            pivot: [Spring::default(); 3],
            primed: false,
            side: 1.0,
            side_s: Spring::at(1.0),
            swap_cool: 0.0,
            frac: 1.0,
            focus: Spring::at(28.0),
            roll: Spring::default(),
            land: Spring::default(),
            kick: [Spring::default(); 2],
            kick_sign: 1.0,
            sneak_s: Spring::default(),
            zoom_s: Spring::default(),
            sprint_s: Spring::default(),
            speed_s: Spring::default(),
            fov: Spring::at(1.0),
            clock: 0.0,
            prev_yaw: 0.0,
            prev_vy: 0.0,
            was_ground: true,
            prev_health: -1,
            hidden: false,
        }
    }

    /// Back to the start: the next `update` dollies out from the eye again. Called every frame the view is not the shoulder one.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Over to the other shoulder now (the view button's tap in this view). The wall test leaves the choice alone for 1.2 s.
    pub fn swap(&mut self) {
        self.side = -self.side;
        self.swap_cool = 1.2;
    }

    fn pivot_vec(&self) -> Vec3 {
        Vec3::new(self.pivot[0].x, self.pivot[1].x, self.pivot[2].x)
    }

    pub fn update(&mut self, f: &Frame) -> Shot {
        let dt = f.dt.clamp(0.0, 0.1);
        self.clock += dt;
        let (sy, cy) = f.yaw.sin_cos();
        let (sp, cp) = f.pitch.sin_cos();
        let fwd = Vec3::new(sy * cp, -sp, -cy * cp).normalize();
        let right = Vec3::new(cy, 0.0, sy);
        let up = right.cross(fwd);

        // Dolly in, stance, speed.
        self.blend.crit(1.0, 8.0, dt);
        let w = self.blend.x.clamp(0.0, 1.0);
        let speed = Vec3::new(f.vel.x, 0.0, f.vel.z).length();
        self.sneak_s.crit(b(f.sneak), 9.0, dt);
        self.zoom_s.crit(b(f.zoom), 9.0, dt);
        self.sprint_s.crit(b(f.sprint && speed > 2.0), 6.0, dt);
        self.speed_s.crit((speed / 5.6).min(1.0), 6.0, dt);
        let (sn, z, spr, spd) = (self.sneak_s.x.clamp(0.0, 1.0), self.zoom_s.x.clamp(0.0, 1.0), self.sprint_s.x.clamp(0.0, 1.0), self.speed_s.x.clamp(0.0, 1.0));

        // Events: landing, jump, hit.
        if f.on_ground && !self.was_ground && self.prev_vy < -5.0 {
            self.land.v -= (-self.prev_vy - 5.0).min(20.0) * 0.35;
        }
        if !f.on_ground && self.was_ground && f.vel.y > 1.0 {
            self.land.v += 0.9;
        }
        if self.prev_health >= 0 && f.health < self.prev_health {
            self.kick_sign = -self.kick_sign;
            self.kick[0].v -= 1.1;
            self.kick[1].v += 1.9 * self.kick_sign;
            self.land.v -= 0.8;
        }
        (self.was_ground, self.prev_vy, self.prev_health) = (f.on_ground, f.vel.y, f.health);
        self.land.bounce(0.0, 20.0, 0.32, dt);
        self.kick[0].bounce(0.0, 24.0, 0.3, dt);
        self.kick[1].bounce(0.0, 24.0, 0.3, dt);

        // Shoulder swap: a wall close on the preferred side, air on the other.
        self.swap_cool = (self.swap_cool - dt).max(0.0);
        let probe = |dir: Vec3| [0.1_f32, -0.7].iter().map(|dy| free(f.solid, f.eye + Vec3::Y * *dy, dir, 1.2)).fold(1.2, f32::min);
        let (pref, other) = (probe(right * self.side), probe(-right * self.side));
        if self.swap_cool == 0.0 && pref < 0.7 && other > pref + 0.5 {
            self.side = -self.side;
            self.swap_cool = 0.9;
        }
        self.side_s.crit(self.side, 10.0, dt);

        // The shot's shape (blocks): behind, a shoulder to the side, a little above the eye.
        let mut dist = 2.6 + 0.8 * spr - 0.35 * sn;
        dist += (1.2 - dist) * z;
        dist += 0.5 * (1.0 - self.side_s.x.abs().min(1.0)); // crossing over: swing out wide behind the head, not through it
        let mut shoulder = 0.62 - 0.05 * sn;
        shoulder += (0.8 - shoulder) * z;
        let mut rise = 0.2 - 0.25 * sn;
        rise += (0.08 - rise) * z;

        // The pivot follows the wanted anchor through springs (xz slower than y), capped at 0.6 blocks behind it.
        let want = f.eye + right * (self.side_s.x * shoulder * w) + Vec3::Y * (rise * w);
        if !self.primed || (self.pivot_vec() - want).length() > 6.0 {
            self.pivot = [Spring::at(want.x), Spring::at(want.y), Spring::at(want.z)];
            self.primed = true;
        }
        self.pivot[0].crit(want.x, 16.0, dt);
        self.pivot[1].crit(want.y, 26.0, dt);
        self.pivot[2].crit(want.z, 16.0, dt);
        let off = self.pivot_vec() - want;
        if off.length() > 0.6 {
            let c = want + off.normalize() * 0.6;
            (self.pivot[0].x, self.pivot[1].x, self.pivot[2].x) = (c.x, c.y, c.z);
        }
        let pivot = self.pivot_vec() + Vec3::Y * self.land.x * w;

        // Where the camera wants to be, and how much of the way from the eye is free.
        let desired = pivot - fwd * (dist * w);
        let to = desired - f.eye;
        let len = to.length();
        let clear = if len < 1e-3 {
            1.0
        } else {
            let dir = to / len;
            let best = [(0.0, 0.0), (0.28, 0.0), (-0.28, 0.0), (0.0, 0.28), (0.0, -0.28)]
                .iter()
                .map(|&(a, c)| free(f.solid, f.eye + right * a + up * c, dir, len))
                .fold(len, f32::min);
            ((best - RADIUS).max(0.0) / len).min(1.0)
        };
        if clear < self.frac {
            self.frac = clear;
        } else {
            self.frac += (clear - self.frac) * (1.0 - (-10.0 * dt).exp());
        }
        let mut pos = f.eye + to * self.frac;
        // Push the camera's sphere out of whatever it still grazes (a ray along a wall stops at the wall, not RADIUS short of it).
        for _ in 0..2 {
            for dir in [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z] {
                let d = free(f.solid, pos, dir, RADIUS);
                if d < RADIUS {
                    pos -= dir * (RADIUS - d);
                }
            }
        }
        if (f.solid)(pos) {
            pos = f.eye; // squeezed into a cell: the eye is the one place known to be free
        }
        let near = (pos - f.eye).length();
        self.hidden = near < 0.8 || (self.hidden && near < 1.1);

        // Convergence: aim the camera's centre at the point the eye ray reaches at the aimed block's distance (rack-focused).
        self.focus.crit(f.aim.unwrap_or(28.0).clamp(4.0, 64.0), 5.0, dt);
        let d = f.eye + fwd * self.focus.x - pos;
        let (df, dr, du) = (d.dot(fwd), d.dot(right), d.dot(up));
        let (conv_yaw, conv_pitch) = (dr.atan2(df) * w, du.atan2(dr.hypot(df)) * w);

        // Bank into a strafe or a turn.
        let yaw_rate = if dt > 1e-4 { ((f.yaw - self.prev_yaw) / dt).clamp(-6.0, 6.0) } else { 0.0 };
        self.prev_yaw = f.yaw;
        self.roll.crit((f.vel.dot(right) * 0.010 + yaw_rate * 0.012).clamp(-0.08, 0.08), 7.0, dt);

        // Hand-held sway, calmer at speed.
        let (t, calm) = (self.clock, (1.0 - 0.6 * spd) * w);
        let sway_y = (0.004 * (0.7 * t).sin() + 0.0025 * (1.9 * t + 1.3).sin()) * calm;
        let sway_p = (0.0035 * (0.9 * t + 0.4).sin() + 0.002 * (2.3 * t).sin()) * calm;
        let sway_r = 0.003 * (0.55 * t + 2.0).sin() * calm;
        // `view = Rz(roll) * Rx(-pitch) * Ry(yaw)`: positive roll leans right, the pitch / yaw turn the view toward the aim point.
        let view = Mat4::from_rotation_z(self.roll.x * w + self.kick[1].x + sway_r)
            * Mat4::from_rotation_x(-(conv_pitch + self.kick[0].x + sway_p))
            * Mat4::from_rotation_y(conv_yaw + sway_y);

        // Field of view: a touch wider at a sprint, a whip while the dolly runs (peaks halfway).
        self.fov.crit(1.0 + 0.045 * spr + 0.03 * spd, 6.0, dt);
        let fov_scale = self.fov.x + 0.10 * 4.0 * w * (1.0 - w);

        Shot { pos, fov_scale, close: self.hidden, view }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame<'a>(solid: &'a dyn Fn(Vec3) -> bool, aim: Option<f32>) -> Frame<'a> {
        Frame { dt: 1.0 / 60.0, eye: Vec3::ZERO, yaw: 0.0, pitch: 0.0, vel: Vec3::ZERO, on_ground: true, sneak: false, sprint: false, zoom: false, health: 20, aim, solid }
    }

    fn settle(rig: &mut Rig, f: &Frame) -> Shot {
        let mut s = rig.update(f);
        for _ in 0..600 {
            s = rig.update(f);
        }
        s
    }

    /// The critically damped spring is exact at any step and never passes its target.
    #[test]
    fn spring_is_exact_and_does_not_overshoot() {
        let (mut a, mut c) = (Spring { x: 0.0, v: 2.0 }, Spring { x: 0.0, v: 2.0 });
        a.crit(1.0, 7.0, 0.5);
        (0..50).for_each(|_| c.crit(1.0, 7.0, 0.01));
        assert!((a.x - c.x).abs() < 1e-4 && (a.v - c.v).abs() < 1e-3, "{a:?} {c:?}");
        let mut s = Spring::default();
        for _ in 0..600 {
            s.crit(1.0, 5.5, 1.0 / 60.0);
            assert!(s.x <= 1.0 + 1e-6);
        }
        assert!((s.x - 1.0).abs() < 1e-4);
    }

    /// In open air the aim point sits on the screen centre (within the hand-held sway), whatever the look direction.
    #[test]
    fn crosshair_stays_on_the_aim_point() {
        let open = |_: Vec3| false;
        for (yaw, pitch) in [(0.0_f32, 0.0_f32), (1.2, -0.6), (-2.5, 0.9)] {
            let mut rig = Rig::new();
            let f = Frame { yaw, pitch, ..frame(&open, Some(20.0)) };
            let s = settle(&mut rig, &f);
            assert!(s.pos.length() > 2.0, "the camera is out on the shoulder: {:?}", s.pos);
            let (sy, cy) = yaw.sin_cos();
            let (sp, cp) = pitch.sin_cos();
            let fwd = Vec3::new(sy * cp, -sp, -cy * cp);
            let view = s.view * Mat4::look_at_rh(s.pos, s.pos + fwd, Vec3::Y);
            let p = view.transform_point3(fwd * 20.0);
            assert!(p.x.atan2(-p.z).abs() < 0.02 && p.y.atan2(-p.z).abs() < 0.02, "{yaw} {pitch}: {p:?}");
        }
    }

    /// A wall behind pulls the camera in at once; it never ends inside the wall.
    #[test]
    fn a_wall_behind_pulls_the_camera_in() {
        let wall = |p: Vec3| p.z > 1.0; // yaw 0 looks toward -z, so behind is +z
        let mut rig = Rig::new();
        let s = settle(&mut rig, &frame(&wall, None));
        assert!(s.pos.is_finite() && s.pos.z < 0.9, "{:?}", s.pos);
    }

    /// A wall on the right shoulder flips the camera to the left one.
    #[test]
    fn a_wall_on_one_side_swaps_the_shoulder() {
        let wall = |p: Vec3| p.x > 0.5; // yaw 0: right is +x
        let mut rig = Rig::new();
        settle(&mut rig, &frame(&wall, None));
        assert!(rig.side_s.x < -0.9, "{}", rig.side_s.x);
    }

    /// A manual swap flips the side and the camera never goes through the head; squeezed into a tunnel the model is left out.
    #[test]
    fn swap_by_hand_and_hide_when_squeezed() {
        let open = |_: Vec3| false;
        let mut rig = Rig::new();
        let s = settle(&mut rig, &frame(&open, None));
        assert!(!s.close && rig.side_s.x > 0.9);
        rig.swap();
        let f = frame(&open, None);
        for _ in 0..600 {
            let s = rig.update(&f);
            assert!(s.pos.is_finite());
        }
        assert!(rig.side_s.x < -0.9);
        let tunnel = |p: Vec3| p.z > 0.35 || p.x.abs() > 0.5 || p.y > 0.5 || p.y < -1.0;
        let mut rig = Rig::new();
        let s = settle(&mut rig, &frame(&tunnel, None));
        assert!(s.close && s.pos.is_finite() && !tunnel(s.pos), "{:?}", s.pos);
    }

    /// A hard landing dips the pivot and it rings back to rest; a hit kicks and settles; `reset` starts the dolly over.
    #[test]
    fn impacts_ring_and_settle() {
        let open = |_: Vec3| false;
        let mut rig = Rig::new();
        let mut f = Frame { on_ground: false, vel: Vec3::new(0.0, -15.0, 0.0), ..frame(&open, None) };
        rig.update(&f);
        f.on_ground = true;
        f.vel = Vec3::ZERO;
        rig.update(&f);
        assert!(rig.land.v < -1.0, "landing speed went into the spring: {}", rig.land.v);
        f.health = 14;
        rig.update(&f);
        assert!(rig.kick[1].v.abs() > 0.5, "a hit kicks the roll");
        settle(&mut rig, &f);
        assert!(rig.land.x.abs() < 1e-3 && rig.kick[1].x.abs() < 1e-3);
        rig.reset();
        assert!(rig.blend.x == 0.0 && rig.prev_health == -1);
    }
}
