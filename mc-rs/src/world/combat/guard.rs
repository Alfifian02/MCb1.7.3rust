//! C4: the guard (`core/weapon/GuardProfile`, `state/GuardState`, `guard/GuardRules`). **NOT b1.7.3.** UNVERIFIED: not compiled.
//!
//! In the mod a guard is a shield or a sword, and none of the five new families guards (`WeaponFamily.guard`); b1.7.3 has no shield, so
//! here only the sword family does: 1 point, a 90 degree arc, a point back every 40 ticks, 100 ticks of stagger when the last point is
//! spent (`GuardProfile.SWORD`). "Stamina" in the roadmap's "guard + stamina" is the mod's own: the guard's points. Holding the guard
//! up is free, only a blocked hit costs a point, and raising it ends a charge.
//!
//! Deviation: the mod charges one point per attacker per game tick (`GuardStamp`). b1.7.3's damage window (10 ticks) already makes a hit
//! one hit, so a block opens the same window instead: hits inside it are absorbed free (a slime that touches every tick costs one point).

use super::profile::Profile;
use glam::Vec3;

/// `GuardProfile`: points, total arc in degrees, ticks between one point coming back, ticks of stagger after the last point.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct GuardProfile {
    pub points: u32,
    pub arc: f32,
    pub regen: u32,
    pub stun: u32,
}

pub const NONE: GuardProfile = GuardProfile { points: 0, arc: 0.0, regen: 0, stun: 0 };
pub const SWORD: GuardProfile = GuardProfile { points: 1, arc: 90.0, regen: 40, stun: 100 };
/// b1.7.3's damage window, the ticks a block keeps absorbing for free.
const WINDOW: u32 = 10;

/// What a profile's guard is worth: the sword's, nothing for anything else.
pub fn profile_of(p: Profile) -> GuardProfile {
    if p == Profile::Sword { SWORD } else { NONE }
}

/// `GuardRules.isFromFront`: is `from` inside the arc around `look`, seen from `me`? A source on top of the player counts as in front.
pub fn from_front(look: Vec3, me: Vec3, from: Vec3, arc: f32) -> bool {
    let d = from - me;
    if d.length_squared() < 1e-12 {
        return true;
    }
    d.normalize().dot(look.normalize_or_zero()).clamp(-1.0, 1.0).acos().to_degrees() <= arc / 2.0
}

/// `GuardState` plus the stagger the mod keeps as an item cooldown.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Guard {
    pub held: bool,
    pub max: u32,
    pub points: u32,
    /// Ticks until the next point, ticks of stagger left, ticks of the damage window left.
    regen: u32,
    pub stun: u32,
    window: u32,
    /// The guard input of the last tick: a guard comes up on the press, never by itself when a stagger ends (`tickGuard`).
    was_down: bool,
}

impl Guard {
    /// `withMax`: the held item decides the size, read every tick. A guard that first appears is full.
    pub fn sync(&mut self, p: GuardProfile) {
        if self.max == 0 && p.points > 0 {
            self.points = p.points;
        }
        self.max = p.points;
        self.points = self.points.min(p.points);
        if p.points == 0 {
            self.held = false;
        }
    }

    /// `GuardState.isActive`, and not staggered.
    pub fn active(&self) -> bool {
        self.held && self.points > 0 && self.stun == 0
    }

    /// The guard input for this tick: raised on the press when usable (`guardIsUsable`, `mayRaiseGuard`: not exhausted, not eating), lowered
    /// on the release. True when it was just raised (the caller ends a charge).
    pub fn input(&mut self, down: bool, eating: bool) -> bool {
        let press = down && !self.was_down;
        self.was_down = down;
        if !down {
            self.held = false;
        } else if press && self.max > 0 && self.points > 0 && self.stun == 0 && !eating {
            self.held = true;
            return true;
        }
        false
    }

    /// A hit from `from` while `look`ing: absorbed if the guard is up and the source is inside the arc. A new block costs a point (the
    /// last one breaks it: stagger, lowered); one inside the window is free. `None` = not blocked, `Some(broke)` = blocked.
    pub fn block(&mut self, p: GuardProfile, look: Vec3, me: Vec3, from: Vec3) -> Option<bool> {
        if !self.active() || !from_front(look, me, from, p.arc) {
            return None;
        }
        if self.window > 0 {
            return Some(false);
        }
        self.window = WINDOW;
        self.points -= 1;
        self.regen = p.regen.max(1);
        if self.points == 0 {
            (self.held, self.stun) = (false, p.stun);
            return Some(true);
        }
        Some(false)
    }

    /// `GuardState.tick` + `tickGuard`: a point comes back every `regen` ticks unless the guard is up or staggered (`regenPaused`).
    pub fn tick(&mut self, p: GuardProfile) {
        let staggered = self.stun > 0;
        if staggered {
            self.held = false;
        }
        self.window = self.window.saturating_sub(1);
        if self.max > 0 && self.points < self.max && !self.held && !staggered {
            self.regen = self.regen.saturating_sub(1);
            if self.regen == 0 {
                self.points += 1;
                self.regen = p.regen.max(1);
            }
        }
        self.stun = self.stun.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The sword's guard: free to hold, one point per block, the last one staggers for 100 ticks, then 40 ticks to get it back.
    #[test]
    fn a_blocked_hit_costs_a_point_and_the_last_one_staggers() {
        let (look, me) = (Vec3::NEG_Z, Vec3::ZERO);
        let (front, behind, side) = (Vec3::new(0.0, 0.0, -2.0), Vec3::new(0.0, 0.0, 2.0), Vec3::new(2.0, 0.0, -1.9));
        assert!(from_front(look, me, front, 90.0) && !from_front(look, me, behind, 90.0));
        assert!(!from_front(look, me, side, 90.0), "46 degrees is outside a 90 degree arc");
        assert!(from_front(look, me, Vec3::new(1.9, 0.0, -2.0), 90.0), "43 degrees is inside");
        let sword = profile_of(Profile::Sword);
        assert_eq!(profile_of(Profile::Dagger), NONE);
        let mut g = Guard::default();
        g.sync(sword);
        assert!(g.input(true, true) == false && !g.held, "not while eating");
        assert!(g.input(false, false) == false);
        assert!(g.input(true, false) && g.active());
        assert_eq!(g.block(sword, look, me, behind), None, "from behind is not blocked");
        assert_eq!(g.block(sword, look, me, front), Some(true), "the one point is the last one");
        assert!(!g.held && g.stun == 100 && g.points == 0);
        assert_eq!(g.block(sword, look, me, front), None, "a lowered guard blocks nothing");
        assert!(!g.input(true, false) || !g.active(), "no raising in the stagger");
        for _ in 0..139 {
            g.input(false, false);
            g.tick(sword);
        }
        assert_eq!(g.points, 0);
        g.tick(sword);
        assert_eq!(g.points, 1, "100 ticks of stagger, then 40 for the point");
    }
}
