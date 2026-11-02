//! C4 reworked (2026-11-01): how a hurt player gets better with NO hunger. **NOT b1.7.3**, only with the Immersive Pack ON. UNVERIFIED: not compiled.
//!
//! The mod's healing food bar (`survival/SurvivalRules`) is gone: there is no food bar, no starvation, and a meal heals on the spot like b1.7.3's
//! (`items::heal_amount`). Healing has three other doors, each with its own cost, so none makes another pointless (design.md rule 2):
//!
//! * **Hearth**: out of combat (`Stamina::in_combat`, 15 s) with a fire at hand (`App::heat_at_hand`: block light of a torch, furnace or lava),
//!   1 health point every `HEARTH_TICKS`, up to `HEARTH_CAP` only. Free, slow, and it needs a quiet place. Above the cap only food, a bandage
//!   or a bed heals; the cap is above the stamina's lock (20% health) so a rest brings the sprint back.
//! * **Bandage** (`catalog::BANDAGE`): put on (`BANDAGE_USE` ticks, `App::eating`), then `BANDAGE_HEAL` points, one every `BANDAGE_BEAT` ticks.
//!   Any health lost takes it off (a blocked hit costs none, so a sword can guard while it works). Cheap, steady, fragile.
//! * **Bed**: waking in the morning heals everything (`App::sleep_tick`). Not here: it is one line there.
//!
//! The numbers are mine, UNVERIFIED by play. `Vitals::restore` gives the points (not `Vitals::heal`, which also cuts the damage window to half:
//! a trickle would keep shortening it).

use crate::world::vitals::MAX_HEALTH;

/// Hearth: one point every 2 s, to 12 of 20.
pub const HEARTH_TICKS: u32 = 40;
pub const HEARTH_CAP: i32 = 12;
/// Bandage: 1 s to put on, then 6 points, one a second.
pub const BANDAGE_USE: u32 = 20;
pub const BANDAGE_HEAL: u32 = 6;
pub const BANDAGE_BEAT: u32 = 20;

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Mend {
    /// Quiet ticks counted towards the hearth's next point.
    rest: u32,
    /// Ticks the bandage still has to run, and the ticks since its last point.
    left: u32,
    beat: u32,
    /// The health of the last tick: a fall under it takes the bandage off.
    line: i32,
}

impl Mend {
    /// A bandage is working.
    pub fn bandaged(&self) -> bool {
        self.left > 0
    }

    /// The hearth is ticking (Gauges, UNVERIFIED): quiet, a fire at hand and under the cap. `tick` keeps its own test, which also counts the point
    /// a bandage gives in the same tick.
    pub fn hearth_on(health: i32, calm: bool, warm: bool) -> bool {
        calm && warm && health < HEARTH_CAP
    }

    /// The bandage is on (the caller used one up).
    pub fn apply(&mut self, health: i32) {
        (self.left, self.beat, self.line) = (BANDAGE_HEAL * BANDAGE_BEAT, 0, health);
    }

    /// One 20 Hz tick: the health points to give now (0, 1 or 2). `calm`: out of combat; `warm`: a fire at hand.
    pub fn tick(&mut self, health: i32, calm: bool, warm: bool) -> i32 {
        let mut give = 0;
        if self.left > 0 {
            if health < self.line || health >= MAX_HEALTH {
                self.left = 0; // a hit came through, or nothing is left to mend
            } else {
                self.left -= 1;
                self.beat += 1;
                if self.beat >= BANDAGE_BEAT {
                    (self.beat, give) = (0, 1);
                }
            }
        }
        if calm && warm && health + give < HEARTH_CAP {
            self.rest += 1;
            if self.rest >= HEARTH_TICKS {
                (self.rest, give) = (0, give + 1);
            }
        } else {
            self.rest = 0;
        }
        self.line = health + give;
        give
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run `n` ticks from `h` health, feeding the points back; returns the health after.
    fn run(m: &mut Mend, mut h: i32, n: u32, calm: bool, warm: bool) -> i32 {
        for _ in 0..n {
            h += m.tick(h, calm, warm);
        }
        h
    }

    // The hearth needs calm AND a fire and stops at its cap; a bandage gives 6 in 6 s anywhere, and any health lost takes it off.
    #[test]
    fn hearth_rests_to_its_cap_and_a_bandage_runs_until_a_hit() {
        let mut m = Mend::default();
        assert_eq!((run(&mut m, 5, 200, true, false), run(&mut m, 5, 200, false, true)), (5, 5), "no fire, or no quiet: nothing");
        assert_eq!(run(&mut m, 5, 39, true, true), 5);
        assert_eq!(run(&mut m, 5, 40, true, true), 6, "one a 40 ticks");
        assert_eq!(run(&mut Mend::default(), 5, 1000, true, true), HEARTH_CAP, "and no further");
        assert!(Mend::hearth_on(5, true, true) && !Mend::hearth_on(HEARTH_CAP, true, true) && !Mend::hearth_on(5, false, true));
        let mut m = Mend::default();
        m.apply(10);
        assert!(m.bandaged());
        assert_eq!(run(&mut m, 10, 120, false, false), 16, "6 points in 6 s, in the middle of a fight");
        assert!(!m.bandaged());
        let mut m = Mend::default();
        m.apply(10);
        assert_eq!(run(&mut m, 10, 30, false, false), 11);
        assert_eq!(m.tick(8, false, false), 0, "a hit took it off");
        assert!(!m.bandaged());
    }
}
