//! C5: the right button (`SpecialHandler`, `LungeDamage`, `HalberdSweep`, `ParryState`, `StreakState`). **NOT b1.7.3.**
//! **UNVERIFIED: written without compiling or running anything.**
//!
//! Four specials, one price each, all numbers from the mod: the ropera's lunge (8 swings of cooldown), the alabarda's and the hoe's sweep
//! (`MultiHit`, 4 swings: everything in a wider arc, no charge), the shotel's parry (a 12 tick window, a 100 tick lockout, the walk at
//! x0.4) and the martillo's streak (+0.15 of its damage per hit landed inside 80 ticks of the last). Not ported: the enchantments
//! (Audacia, Retirada, Barredor, Segunda Pasada, Corte Gemelo, Reflejo de Acero, Capacitor, Vampirico, Ritmo de Hierro, Sentencia), the lunge's
//! status effect, armour piercing and sounds / particles (C7), and the arcabuz's `Fusee` (C8).

use super::profile::Reach;
use glam::Vec3;

/// `LungeDamage`: a lunge hit is 0.75 of the weapon's damage, +0.375 a target already struck in the same tick, at most x4.
const LUNGE_SHARE: f32 = 0.75;
const LUNGE_STEP: f32 = 0.375;
const LUNGE_MAX: f32 = 4.0;
/// Swings of cooldown the lunge and the sweep cost (`COOLDOWN_FACTOR`), on the attack's own cooldown.
const LUNGE_COST: f32 = 8.0;
const SWEEP_COST: f32 = 4.0;
/// The lunge lasts at most 20 ticks; it covers `6 * (1 + 0.02 * tier)` blocks (`estimatedDistance`) and ends under 0.02 blocks a tick (`STILL`).
pub const LUNGE_WINDOW: u32 = 20;
const LUNGE_BASE: f32 = 1.0;
const LUNGE_PER_TIER: f32 = 0.02;
const LUNGE_BLOCKS: f32 = 6.0;
pub const STILL: f32 = 0.02;
/// ponytail: the mod pushes an impulse and lets the drag decide the pace; here a fixed pace in blocks per second (0.9 a tick) decides the
/// time and the distance stays the mod's. UNVERIFIED by play.
pub const LUNGE_SPEED: f32 = 18.0;
/// `HalberdSweep.RANGE_SCALE` / `WIDTH_SCALE`: the arc reaches and opens 1.35 x; the vertical angle stays.
const SWEEP_SCALE: f32 = 1.35;
/// `ParryState`: the window, the lockout from the press, and the walk's factor while the blade is out. The mod applies Slowness amplifier 3
/// (level IV, -15 % a level = x0.4); its javadoc says "75 % off", which is one level more. The code was followed, not the comment.
const PARRY_WINDOW: u32 = 12;
const PARRY_COOLDOWN: u32 = 100;
const PARRY_SLOW: f32 = 0.4;
/// `StreakState.WINDOW_TICKS` and `DamageResolver.WARHAMMER_STEP`.
const STREAK_WINDOW: u32 = 80;
const STREAK_STEP: f32 = 0.15;

/// `LungeDamage.damageFor`: the damage of the `index`-th target (0 = first) a tick of the lunge reaches.
pub fn lunge_damage(base: f32, index: usize) -> f32 {
    base * LUNGE_SHARE * (1.0 + index as f32 * LUNGE_STEP).min(LUNGE_MAX)
}

/// Blocks a lunge covers for a tier (`craft::MATERIALS` order, as `catalog`): the mod's rank is its tier ladder, the same order.
pub fn lunge_distance(tier: usize) -> f32 {
    (LUNGE_BASE + tier as f32 * LUNGE_PER_TIER) * LUNGE_BLOCKS
}

/// Ticks the lunge / the sweep keep the weapon from swinging (`interval x factor`, at least 1).
pub fn lunge_cost(interval: u32) -> u32 {
    ((interval as f32 * LUNGE_COST).round() as u32).max(1)
}
pub fn sweep_cost(interval: u32) -> u32 {
    ((interval as f32 * SWEEP_COST).round() as u32).max(1)
}

/// `HalberdSweep.reachFor`: farther and wider, as tall as before.
pub fn sweep_reach(r: Reach) -> Reach {
    Reach { dist: r.dist * SWEEP_SCALE, h: (r.h * SWEEP_SCALE).min(89.0), v: r.v }
}

/// `LungeDamage.cube`: the box a lunge hits in, `reach / 2` each way round the player's middle (`pos` is the box's centre here).
pub fn lunge_cube(pos: Vec3, reach: f32) -> (Vec3, Vec3) {
    let h = Vec3::splat(reach * 0.5);
    (pos - h, pos + h)
}

/// One lunge in progress. `done` holds the mobs already struck (`Lunge.wasHit`), by index in `Mobs::list`.
/// ponytail: an index can shift if a mob is removed mid-lunge (at most 20 ticks); the worst case is one mob skipped or struck twice.
#[derive(Clone, Debug)]
pub struct Lunge {
    pub dir: Vec3,
    pub start: Vec3,
    pub last: Vec3,
    pub dist: f32,
    pub left: u32,
    pub done: Vec<usize>,
    pub ticks: u32,
}

/// The right button's state: the parry (`ParryState`), the lunge (`LungeState`) and the hammer's streak (`StreakState`).
#[derive(Clone, Debug, Default)]
pub struct State {
    parry: u32,
    parry_cd: u32,
    pub lunge: Option<Lunge>,
    hits: u32,
    since: u32,
}

impl State {
    /// A shotel press: opens the window if it is not up and the lockout is over (`ParryState.canActivate`). True when it opened.
    pub fn parry_press(&mut self) -> bool {
        let ok = self.parry == 0 && self.parry_cd == 0;
        if ok {
            (self.parry, self.parry_cd) = (PARRY_WINDOW, PARRY_COOLDOWN);
        }
        ok
    }

    /// `ParryHandler`: a hit lands inside the window: it is refused and the window closes (the lockout runs on). True when it parried.
    pub fn parry_take(&mut self) -> bool {
        std::mem::take(&mut self.parry) > 0
    }

    pub fn parrying(&self) -> bool {
        self.parry > 0
    }

    /// Ticks of lockout left and its full length, for the button's cooldown mark.
    pub fn parry_cooldown(&self) -> (u32, u32) {
        (self.parry_cd, PARRY_COOLDOWN)
    }

    /// The walk's factor while the blade is out.
    pub fn slow(&self) -> f32 {
        if self.parry > 0 { PARRY_SLOW } else { 1.0 }
    }

    /// `StreakState.isActive`.
    pub fn streak(&self) -> u32 {
        if self.since < STREAK_WINDOW { self.hits } else { 0 }
    }

    /// `DamageResolver`: the multiplier a live streak adds (`hits x 0.15`, before the hit that is being swung is counted).
    pub fn streak_bonus(&self) -> f32 {
        self.streak() as f32 * STREAK_STEP
    }

    /// `StreakState.onHit`: a martillo blow connected.
    pub fn streak_hit(&mut self) {
        self.hits = self.streak() + 1;
        self.since = 0;
    }

    /// 20 Hz.
    pub fn tick(&mut self) {
        self.parry = self.parry.saturating_sub(1);
        self.parry_cd = self.parry_cd.saturating_sub(1);
        self.since = (self.since + 1).min(STREAK_WINDOW);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // One check for the numbers of the four specials (the mod's LungeDamageTest, HalberdSweepTest, ParryTest, StreakWindowTest).
    #[test]
    fn the_specials_cost_and_pay_what_the_mod_says() {
        // Lunge: 0.75 of the damage for the first target, +0.375 a target, capped at x4; 8 swings; 6 blocks at wood, 6.12 at diamond.
        assert_eq!(lunge_damage(8.0, 0), 6.0);
        assert_eq!(lunge_damage(8.0, 2), 8.0 * 0.75 * 1.75);
        assert_eq!(lunge_damage(8.0, 99), 8.0 * 0.75 * 4.0);
        assert_eq!((lunge_cost(10), sweep_cost(23)), (80, 92));
        assert_eq!(lunge_distance(0), 6.0);
        assert!((lunge_distance(3) - 6.36).abs() < 1e-5);
        // Sweep: the alabarda's 5.0 / 60 / 35 becomes 6.75 / 81 / 35, and a hoe's 60 degrees stays under a right angle.
        let r = sweep_reach(Reach { dist: 5.0, h: 60.0, v: 35.0 });
        assert_eq!((r.dist, r.h, r.v), (6.75, 81.0, 35.0));
        assert_eq!(sweep_reach(Reach { dist: 3.0, h: 80.0, v: 10.0 }).h, 89.0);
        // Parry: opens once, a second press inside the window or the lockout does nothing, a hit closes the window, 100 ticks reopen it.
        let mut s = State::default();
        assert!(s.parry_press() && !s.parry_press() && s.slow() == 0.4);
        assert!(s.parry_take() && !s.parry_take() && s.slow() == 1.0 && !s.parry_press());
        (0..99).for_each(|_| s.tick());
        assert!(!s.parry_press());
        s.tick();
        assert!(s.parry_press());
        // Streak: the first blow adds nothing, the next 0.15, and 80 ticks without a hit end it.
        assert_eq!(s.streak_bonus(), 0.0);
        s.streak_hit();
        s.streak_hit();
        assert!((s.streak_bonus() - 0.3).abs() < 1e-6);
        (0..79).for_each(|_| s.tick());
        assert_eq!(s.streak(), 2);
        s.tick();
        assert_eq!((s.streak(), s.streak_bonus()), (0, 0.0));
        s.streak_hit();
        assert_eq!(s.streak(), 1); // it starts over from one
    }
}
