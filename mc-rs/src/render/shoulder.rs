//! The shoulder-surf view: a cinematic over-the-shoulder camera (third view button, second tap).
//!
//! NOT b1.7.3 (the Java has only the 4-block-behind third-person view, `third_pos` in `lib.rs`): this is a camera rig in the style of
//! a modern third-person game. Everything here is pure (no GPU, no world, no clock: the caller passes `dt` and a `solid` probe), so
//! `cargo test --lib shoulder` runs on a plain host, and `tools/shoulder_proto.py` is the Python twin of the maths.
//!
//! What it does, per frame (`Rig::update`):
//! - **Glide in**: a critically damped blend 0 -> 1 (omega 16, about 0.3 s) moves the camera from the vanilla third-person place (4 blocks behind
//!   the eye, `THIRD`) in to the shoulder shot. No dolly out from the head: the view button's cycle runs first person -> third -> this one.
//! - **Shoulder anchor + lag**: the camera orbits a pivot a shoulder-width right of the eye. The pivot follows the player through
//!   springs (slower on xz than on y), so a sprint or a stop leaves the camera a little behind, then catches up. Capped at 0.6 blocks, and the
//!   lag never takes the camera nearer the head's line than standing (strafing toward the shoulder must not put the player on the crosshair).
//! - **Aim convergence**: the camera sits off the eye's line, so the crosshair would miss what the pick ray (from the eye) hits.
//!   A small view-space turn (`Shot::view`) aims the camera's centre at the point the eye ray reaches at the aimed block's distance,
//!   the distance itself is spring-eased (a rack-focus, not a snap).
//! - **Squeeze, never swap**: a wall close on the camera's side does not flip the shoulder (only the player does); the offset closes
//!   up toward the head and opens again, eased.
//! - **Collision**: five rays (centre + 4 around, radius 0.28) from the eye to the wanted place; the camera stops short at once,
//!   eases back out at ~10 / s, and a sphere push-out (radius 0.3) keeps it off walls it slides along. Squeezed within 0.8 blocks of
//!   the head it reports `Shot::close` and the caller draws the hand instead of the model.
//! - **Shoulder swap**: by hand only (`Rig::swap`), swinging out wide behind the head while it crosses.
//! - **Bank**: roll into a strafe or a turn (4.6 degrees at most), spring-eased.
//! - **Impacts**: a landing dips the pivot (under-damped spring, scaled by the fall speed), a jump lifts it, a hit kicks pitch and roll
//!   with a side that alternates.
//! - **Stance**: sneak tucks in and down; zoom (the zoom button) closes to an aiming shot (nearer, wider offset); sprint pulls back
//!   and widens the view a touch.
//! - **Hand-held sway**: a few slow sines in yaw / pitch / roll, calmer the faster the player moves.
//!
//! ponytail: the side-room test is two rays (head and waist), not a swept sphere; the pull-in is one segment from the eye to
//! the wanted place, so it can cut a corner a swept capsule would clear. Upgrade: sphere-cast the 5 rays against the voxel boxes.

use glam::{Mat4, Vec3};

/// Substep of the under-damped spring (s): semi-implicit Euler is stable while `omega * h` stays well under 1.
const SUB: f32 = 1.0 / 240.0;
/// Ray / probe step (blocks).
const STEP: f32 = 0.1;
/// The camera's collision radius (blocks): the ray margin and the push-out reach. Over the near plane's corner at 70 degrees.
const RADIUS: f32 = 0.3;
/// Where the glide starts, as a distance behind the eye: the vanilla third-person view (`third_pos` in `lib.rs`, b1.7.3's 4 blocks) that comes
/// before this view in the view button's cycle. The pull-in below stops `RADIUS` short of the place asked for, in open air too, so asking
/// for `4 + RADIUS` puts the camera on the vanilla spot to the block.
const THIRD: f32 = 4.0 + RADIUS;

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
    /// The camera is so close to the head (a tight space) that the player model would fill the screen:
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
    /// Room (blocks) to the camera's side of the head, eased: the shoulder offset never goes past it.
    room: Spring,
    /// Smoothed turn rate (rad/s), the bank's input.
    turn: Spring,
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
            room: Spring::at(1.0),
            turn: Spring::default(),
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

    /// Over to the other shoulder (the view button's tap in this view). Only the player does this: a wall never flips the side.
    pub fn swap(&mut self) {
        self.side = -self.side;
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

        // The glide from the vanilla third-person place to the shoulder, stance, speed.
        self.blend.crit(1.0, 16.0, dt); // ~0.3 s to the shoulder
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

        // A wall close on the camera's side does not flip the shoulder: the offset closes up toward the head, smoothly, and opens again.
        let room = [0.1_f32, -0.7].iter().map(|dy| free(f.solid, f.eye + Vec3::Y * *dy, right * self.side, 1.2)).fold(1.2, f32::min);
        self.room.crit((room - 0.35).max(0.0), 12.0, dt);
        self.side_s.crit(self.side, 10.0, dt);

        // The shot's shape (blocks): behind, a shoulder to the side, a little above the eye.
        let mut dist = 2.6 + 0.8 * spr - 0.35 * sn;
        dist += (1.2 - dist) * z;
        dist += 0.5 * (1.0 - self.side_s.x.abs().min(1.0)); // crossing over: swing out wide behind the head, not through it
        let mut shoulder = 0.62 - 0.05 * sn;
        shoulder += (0.8 - shoulder) * z;
        shoulder = shoulder.min(self.room.x.max(0.0));
        let mut rise = 0.2 - 0.25 * sn;
        rise += (0.08 - rise) * z;

        // The pivot follows the wanted anchor through springs (xz slower than y), capped at 0.6 blocks behind it.
        let lateral = self.side_s.x * shoulder * w; // the camera's offset along `right` from the eye's line
        let want = f.eye + right * lateral + Vec3::Y * (rise * w);
        let prime = !self.primed || (self.pivot_vec() - want).length() > 6.0;
        if prime {
            self.pivot = [Spring::at(want.x), Spring::at(want.y), Spring::at(want.z)];
            self.primed = true;
        }
        self.pivot[0].crit(want.x, 12.0, dt);
        self.pivot[1].crit(want.y, 16.0, dt);
        self.pivot[2].crit(want.z, 12.0, dt);
        // The lag never takes the camera nearer the head's line than standing: strafing toward the camera's shoulder let the 0.6 lag eat the
        // whole 0.62 offset and put the player on the crosshair. Lag toward the far side (the player drifting off) stays. Off during a swap
        // (the side is crossing the head then, and the clamp would snap the pivot over).
        let lat = (self.pivot_vec() - want).dot(right);
        if (self.side_s.x - self.side).abs() < 0.05 && lat * lateral < 0.0 {
            self.pivot[0].x -= right.x * lat;
            self.pivot[2].x -= right.z * lat;
        }
        let off = self.pivot_vec() - want;
        if off.length() > 0.6 {
            let c = want + off.normalize() * 0.6;
            (self.pivot[0].x, self.pivot[1].x, self.pivot[2].x) = (c.x, c.y, c.z);
        }
        let pivot = self.pivot_vec() + Vec3::Y * self.land.x * w;

        // Where the camera wants to be, and how much of the way from the eye is free.
        let desired = pivot - fwd * (THIRD + (dist - THIRD) * w); // w = 0 is the vanilla third-person place
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
        // In fast (the push-out below catches what is left), out slowly; neither is a snap.
        let rate = if clear < self.frac { 30.0 } else { 8.0 };
        if prime {
            self.frac = clear; // the first frame starts where the vanilla view was (already pulled in by a wall), not at a free 4 blocks
        } else {
            self.frac += (clear - self.frac) * (1.0 - (-rate * dt).exp());
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
        self.turn.crit(yaw_rate, 14.0, dt); // a touch drag arrives in lumps: the bank follows its envelope
        self.roll.crit((f.vel.dot(right) * 0.010 + self.turn.x * 0.012).clamp(-0.08, 0.08), 7.0, dt);

        // Hand-held sway, calmer at speed.
        let (t, calm) = (self.clock, (1.0 - 0.6 * spd) * w);
        let sway_y = (0.004 * (0.7 * t).sin() + 0.0025 * (1.9 * t + 1.3).sin()) * calm;
        let sway_p = (0.0035 * (0.9 * t + 0.4).sin() + 0.002 * (2.3 * t).sin()) * calm;
        let sway_r = 0.003 * (0.55 * t + 2.0).sin() * calm;
        // `view = Rz(roll) * Rx(-pitch) * Ry(yaw)`: positive roll leans right, the pitch / yaw turn the view toward the aim point.
        let view = Mat4::from_rotation_z(self.roll.x * w + self.kick[1].x + sway_r)
            * Mat4::from_rotation_x(-(conv_pitch + self.kick[0].x + sway_p))
            * Mat4::from_rotation_y(conv_yaw + sway_y);

        // Field of view: a touch wider at a sprint.
        self.fov.crit(1.0 + 0.045 * spr + 0.03 * spd, 6.0, dt);

        Shot { pos, fov_scale: self.fov.x, close: self.hidden, view }
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

    /// A wall on the right shoulder never flips the camera to the left one: it closes up toward the head and stays out of the wall.
    #[test]
    fn a_wall_on_one_side_squeezes_but_never_swaps() {
        let wall = |p: Vec3| p.x > 0.5; // yaw 0: right is +x
        let mut rig = Rig::new();
        let s = settle(&mut rig, &frame(&wall, None));
        assert!(rig.side_s.x > 0.9, "still the right shoulder: {}", rig.side_s.x);
        assert!(s.pos.x < 0.5 && s.pos.is_finite(), "{:?}", s.pos);
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

    /// The glide starts on the vanilla third-person place (4 blocks straight behind the eye), not at the head, and ends on the shoulder shot.
    #[test]
    fn the_glide_starts_where_the_vanilla_view_was() {
        let open = |_: Vec3| false;
        let mut rig = Rig::new();
        let s = rig.update(&frame(&open, None)); // yaw 0 looks toward -z: behind is +z
        assert!(s.pos.z > 3.85 && s.pos.z < 4.0 && s.pos.x.abs() < 0.05 && !s.close, "first frame: {:?}", s.pos);
        assert!((s.fov_scale - 1.0).abs() < 1e-3, "no field-of-view whip: {}", s.fov_scale);
        let s = settle(&mut rig, &frame(&open, None));
        assert!(s.pos.z < 3.0 && s.pos.x > 0.4, "settled on the right shoulder: {:?}", s.pos);
    }

    /// Strafing toward the camera's shoulder never brings the player nearer the crosshair than standing does (the lag used to eat the offset).
    #[test]
    fn strafing_toward_the_shoulder_keeps_the_player_off_the_crosshair() {
        let open = |_: Vec3| false;
        let mut rig = Rig::new();
        let mut f = frame(&open, None);
        let stand = settle(&mut rig, &f).pos.x; // eye at the origin, yaw 0: right is +x, the camera's side offset is its x
        f.vel = Vec3::new(4.3, 0.0, 0.0);
        let mut least = f32::MAX;
        for _ in 0..180 {
            f.eye.x += 4.3 / 60.0;
            least = least.min(rig.update(&f).pos.x - f.eye.x);
        }
        assert!(least > stand - 0.02, "strafing {least} vs standing {stand}");
    }

    /// A hard landing dips the pivot and it rings back to rest; a hit kicks and settles; `reset` starts the glide over.
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
