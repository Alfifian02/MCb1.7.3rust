//! Weight: what heaviness costs and buys. **NOT b1.7.3**, Immersive Pack worlds only. **UNVERIFIED: not compiled, not run, every number is
//! a first guess.** Pure numbers, like the rest of `combat`; `lib.rs` reads them.
//!
//! One idea, three readers (design.md, "Weight and commitment"): a heavy weapon is slow to draw (`draw_ticks`), a heavy hit staggers its target
//! (`stagger_ticks`), and heavy armor takes some of a blow's shove (`knock_scale`). The weight of a weapon is `Profile::weight`, the load of the
//! armor is `App::armor_load`; neither is new, this only gives them a second use.

/// Ticks of lock after a hotbar swap for a weapon of weight 1.0 (the old fixed 6).
pub const DRAW_BASE: u32 = 6;
/// The axe's stagger on a mob: a plain hit and a charged one, in ticks (no steering and no bite).
pub const STAGGER: u8 = 10;
pub const STAGGER_CHARGED: u8 = 16;
/// Share of a hit's horizontal shove one point of armor load takes away (a full metal set is load 20: -30%), and the least that gets through.
pub const KNOCK_PER_LOAD: f32 = 0.015;
pub const KNOCK_FLOOR: f32 = 0.5;

/// The lock after drawing a weapon: `DRAW_BASE x weight`, never longer than the weapon's own interval (a quick weapon is not locked longer than
/// it takes to swing) and never 0 (a swap and a swing cannot share a frame).
pub fn draw_ticks(weight: f32, interval: u32) -> u32 {
    ((DRAW_BASE as f32 * weight.max(0.0)).round() as u32).clamp(1, interval.max(1))
}

/// Ticks of stagger an axe blow gives (`charged`: a blow over 1.0 of multiplier).
pub fn stagger_ticks(charged: bool) -> u8 {
    if charged { STAGGER_CHARGED } else { STAGGER }
}

/// What is left of a hit's horizontal shove on a player wearing armor of `load` (`App::armor_load`).
pub fn knock_scale(load: f32) -> f32 {
    (1.0 - KNOCK_PER_LOAD * load.max(0.0)).clamp(KNOCK_FLOOR, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::combat::profile::Profile;

    // The heavy pay more to draw, the light less, nobody waits longer than the swing itself or less than a tick.
    #[test]
    fn draw_follows_weight_and_is_bounded() {
        assert_eq!(draw_ticks(Profile::Dagger.weight(), 6), 4);
        assert_eq!(draw_ticks(Profile::Sword.weight(), 12), 6);
        assert_eq!(draw_ticks(Profile::Axe.weight(), 26), 6);
        assert_eq!(draw_ticks(Profile::Halberd.weight(), 23), 9);
        assert_eq!(draw_ticks(Profile::Warhammer.weight(), 19), 10);
        assert_eq!(draw_ticks(Profile::Unarmed.weight(), 4), 4); // capped by the bare hand's 4-tick swing
        assert_eq!(draw_ticks(0.0, 12), 1);
    }

    #[test]
    fn armor_takes_a_share_of_the_shove_and_never_all() {
        assert_eq!(knock_scale(0.0), 1.0);
        assert!((knock_scale(20.0) - 0.7).abs() < 1e-6);
        assert_eq!(knock_scale(1000.0), KNOCK_FLOOR);
        assert!(knock_scale(8.0) > knock_scale(20.0)); // leather (load 8 at most) holds less than metal
        assert!(stagger_ticks(true) > stagger_ticks(false));
    }
}
