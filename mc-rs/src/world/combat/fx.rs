//! C7: the camera shake table and the geometry of the slash (`ShakeKind`, `Shake`, `CameraShake`, `ShakePhase`, `SlashParticles`,
//! `SlashTiming`). **NOT b1.7.3.** **UNVERIFIED: written without compiling or running anything.**
//!
//! Pure: numbers and geometry, so a host test can open it. `lib.rs` feeds `Rig` (20 Hz `tick`, `add` where the mod's `Shake.to` is
//! called) and composes `Rig::angles` into `camera.bob`; `Effects::{slash, crit}` draw the points this module makes.
//!
//! Deviations:
//! * The mod's slash is a 1.8-block quad playing a 5-frame strip (`textures/particle/<weapon>_hit.png`, 32 x 160). Particles here are small
//!   flat boxes (`render/items.rs`), so the slash is a diagonal streak of white specks (a flat arc for the sweep) and a full charge is a
//!   burst of gold ones (the mod's `CRIT`, 30 of them). No per-weapon sprite, no doubled cut (the shotel's double cut is an enchantment, dropped).
//! * Not ported: the damage indicator (the mod deleted its call), `EXPLOSION` / `NUCLEAR` shakes (a creeper's or TNT's blast shakes
//!   nothing here; the reactor is dropped), the client's `CAMERA_SHAKE` switch (no options row: the table is already small), the
//!   arcabuz's recoil (C8).

use glam::Vec3;

/// `ShakeKind`, the rows this port fires. The scale is the mod's: `Kill` at 2.4 degrees reads, and every other row is measured against it;
/// the most frequent moment (`Hurt`) is the smallest.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Kill,
    GuardBreak,
    Parry,
    ChargedHit,
    Guarded,
    Hurt,
}

pub const ALL: [Kind; 6] = [Kind::Kill, Kind::GuardBreak, Kind::Parry, Kind::ChargedHit, Kind::Guarded, Kind::Hurt];

impl Kind {
    /// Degrees at point blank and the ticks the impulse lives.
    pub fn row(self) -> (f32, u32) {
        match self {
            Kind::Kill => (2.4, 6),
            Kind::GuardBreak => (2.0, 8),
            Kind::Parry => (1.8, 6),
            Kind::ChargedHit => (1.6, 7),
            Kind::Guarded => (1.2, 5),
            Kind::Hurt => (0.8, 5),
        }
    }
}

/// `ShakeKind.COMMON_REACH`: past it an impulse is zero. The amplitude falls with the square of the distance (`CameraShake.amplitudeAt`).
pub const REACH: f32 = 40.0;
/// `CameraShake.PITCH_HZ` / `ROLL_HZ`: radians a tick, not hertz (the mod's names); pitch is a sine, roll a cosine.
const PITCH_RATE: f32 = 3.1;
const ROLL_RATE: f32 = 4.7;
/// `ShakePhase.MAX_TOTAL_DEGREES`: the sum of the live impulses is cut here, so five mobs hitting at once is not a dizzy spell.
pub const MAX_TOTAL: f32 = 10.0;
/// ponytail: at most this many impulses live at once (the mod has no cap, only the sum's); the oldest goes first. 6 ticks each, so a
/// crowd cannot fill it in play.
const MAX_LIVE: usize = 16;

#[derive(Clone, Copy, Debug)]
struct Impulse {
    at: Vec3,
    degrees: f32,
    born: f32,
    dies: f32,
}

/// The live impulses, on a clock of 20 Hz ticks.
#[derive(Clone, Debug, Default)]
pub struct Rig {
    live: Vec<Impulse>,
    now: f32,
}

impl Rig {
    /// `Shake.to`: one impulse of `kind` born at `at` (the place it happened; the camera's distance to it sets the strength).
    pub fn add(&mut self, kind: Kind, at: Vec3) {
        let (degrees, ticks) = kind.row();
        if self.live.len() >= MAX_LIVE {
            self.live.remove(0);
        }
        self.live.push(Impulse { at, degrees, born: self.now, dies: self.now + ticks as f32 });
    }

    /// One 20 Hz tick: the clock moves on and the spent impulses go.
    pub fn tick(&mut self) {
        self.now += 1.0;
        let now = self.now;
        self.live.retain(|i| i.dies >= now);
    }

    pub fn clear(&mut self) {
        self.live.clear();
    }

    /// `CameraShake.onCameraAngles`: the (pitch, roll) in degrees to add to the view at `partial` of a tick, the camera being at `cam`.
    /// Each impulse is `amplitude x life left`, pitch on a sine and roll on a cosine of its age; the sums are clamped to `MAX_TOTAL`.
    pub fn angles(&self, partial: f32, cam: Vec3) -> (f32, f32) {
        let time = self.now + partial;
        let (mut pitch, mut roll) = (0.0, 0.0);
        for i in &self.live {
            let a = amplitude(cam.distance(i.at), i.degrees);
            if a <= 0.0 {
                continue;
            }
            let (age, life) = (time - i.born, ((i.dies - time) / (i.dies - i.born)).max(0.0));
            pitch += a * life * (age * PITCH_RATE).rem_euclid(std::f32::consts::TAU).sin();
            roll += a * life * (age * ROLL_RATE).rem_euclid(std::f32::consts::TAU).cos();
        }
        (pitch.clamp(-MAX_TOTAL, MAX_TOTAL), roll.clamp(-MAX_TOTAL, MAX_TOTAL))
    }
}

/// `CameraShake.amplitudeAt`: `max x (1 - distance / reach)^2`, zero at and past the reach.
pub fn amplitude(distance: f32, max: f32) -> f32 {
    if distance >= REACH {
        return 0.0;
    }
    let t = 1.0 - distance / REACH;
    max * t * t
}

/// Specks in a slash and in a sweep, and in a full charge's burst (`SlashParticles.burst`: 30 `CRIT`).
pub const SLASH_SPECKS: usize = 9;
pub const SWEEP_SPECKS: usize = 25;
pub const CRIT_SPECKS: usize = 30;
/// `SlashTiming.STRIP_TICKS`: the slash lives 7 ticks.
pub const SLASH_TICKS: u16 = 7;
/// `HalberdSweep.ARC_OFFSET_SHARE`: the sweep's mark is centred this share of its reach ahead of the player.
const ARC_SHARE: f32 = 0.3;

/// The two screen axes of a look: right and up (`look` need not be level; a look straight up or down falls back to the world's X).
fn axes(look: Vec3) -> (Vec3, Vec3) {
    let right = look.cross(Vec3::Y).try_normalize().unwrap_or(Vec3::X);
    (right, right.cross(look).normalize_or_zero())
}

/// A hit's slash: a diagonal streak, upper left to lower right as seen from the player, 0.9 blocks long, through the contact point.
pub fn slash_points(contact: Vec3, look: Vec3) -> [Vec3; SLASH_SPECKS] {
    let (right, up) = axes(look);
    std::array::from_fn(|k| {
        let t = k as f32 / (SLASH_SPECKS - 1) as f32 * 2.0 - 1.0; // -1 (upper left) .. 1 (lower right)
        contact + right * (t * 0.45) - up * (t * 0.45)
    })
}

/// The sweep's mark (`SlashParticles.sweep`): a flat arc across the cone, `reach` blocks out at its middle, `half_deg` either side of
/// the look on the ground plane, at the eye's height. The mod puts one quad `ARC_SHARE x reach` ahead; the arc is that quad's width.
pub fn sweep_points(eye: Vec3, look: Vec3, reach: f32, half_deg: f32) -> [Vec3; SWEEP_SPECKS] {
    let flat = Vec3::new(look.x, 0.0, look.z).try_normalize().unwrap_or(Vec3::NEG_Z);
    let radius = reach * ARC_SHARE * 2.0;
    std::array::from_fn(|k| {
        let t = k as f32 / (SWEEP_SPECKS - 1) as f32 * 2.0 - 1.0;
        let a = t * half_deg.min(80.0).to_radians();
        let (s, c) = a.sin_cos();
        eye + Vec3::new(flat.x * c - flat.z * s, 0.0, flat.x * s + flat.z * c) * radius
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // The mod's `ShakeKindTest` / `ShakePhaseTest` / `SlashTimingTest` in one check: the table's scale, the square falloff, the clamp,
    // the impulse's life, and that the streak and the arc have the specks they say and keep their centre.
    #[test]
    fn shake_follows_the_table_and_the_slash_keeps_its_centre() {
        assert!(ALL.iter().all(|k| k.row().0 < MAX_TOTAL && k.row().1 > 0));
        assert_eq!(ALL.iter().map(|k| k.row().0).fold(0.0, f32::max), Kind::Kill.row().0); // the kill is the largest, as in the mod's table
        assert_eq!(Kind::Hurt.row().0, ALL.iter().map(|k| k.row().0).fold(f32::MAX, f32::min)); // the most frequent is the smallest
        assert_eq!(amplitude(0.0, 2.4), 2.4);
        assert_eq!(amplitude(20.0, 2.4), 0.6); // half way: a quarter
        assert_eq!(amplitude(REACH, 2.4), 0.0);
        // An impulse shakes, then is gone after its ticks; the sum never passes the clamp, even with sixteen at once.
        let mut rig = Rig::default();
        rig.add(Kind::Kill, Vec3::ZERO);
        let (p, r) = rig.angles(0.5, Vec3::ZERO);
        assert!(p.abs() > 0.0 && r.abs() > 0.0 && p.abs() <= 2.4 && r.abs() <= 2.4);
        for _ in 0..7 {
            rig.tick();
        }
        assert_eq!(rig.angles(0.0, Vec3::ZERO), (0.0, 0.0));
        for _ in 0..40 {
            rig.add(Kind::Kill, Vec3::ZERO);
        }
        assert!(rig.live.len() <= MAX_LIVE);
        let (p, r) = rig.angles(1.0, Vec3::ZERO);
        assert!(p.abs() <= MAX_TOTAL && r.abs() <= MAX_TOTAL);
        // A streak of 9 is centred on the contact; a sweep of 25 is level, equally far from the eye and symmetric about the look.
        let look = Vec3::new(0.0, 0.0, -1.0);
        let c = Vec3::new(3.0, 2.0, -5.0);
        let mid = slash_points(c, look).iter().fold(Vec3::ZERO, |a, p| a + *p) / SLASH_SPECKS as f32;
        assert!(mid.distance(c) < 1e-5);
        let arc = sweep_points(Vec3::new(0.0, 1.6, 0.0), look, 6.75, 81.0);
        assert!(arc.iter().all(|p| p.y == 1.6 && ((p.x.hypot(p.z) - 6.75 * 0.6).abs() < 1e-4)));
        assert!((arc[0].x + arc[SWEEP_SPECKS - 1].x).abs() < 1e-4 && (arc[0].z - arc[SWEEP_SPECKS - 1].z).abs() < 1e-4);
    }
}
