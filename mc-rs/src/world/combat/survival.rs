//! C4: the healing food bar (`survival/SurvivalRules`, `SurvivalTuning.DEFAULTS`, `SurvivalTicker`). **NOT b1.7.3.** UNVERIFIED: not compiled.
//!
//! "Running, mining and swinging cost nothing now. Food is only ever spent to heal, and it regenerates very slowly." The bar is 20
//! points; a hurt player with at least 6 pays 1 for 1 health point every 20 ticks, and it refills 1 point every 160 ticks while it is
//! not full. Eating adds to it (`Food::eat`, the meal's `heal_amount` in food points: UNVERIFIED mapping). Joined to `Stamina`: it lives
//! in `Vitals` beside it, ticks with it and is refilled with it, and a bar under the healing floor slows the stamina's recovery
//! (`Stamina::hungry`, NOT the mod's).
//!
//! Not ported: starvation (`SurvivalRules.starve`: b1.7.3 has no difficulty to scale it by; an empty bar just stops healing and refilling
//! until a meal), the regeneration gamerule and the vanilla `FoodData` (b1.7.3 has none).

pub const MAX_FOOD: i32 = 20;
/// `SurvivalTuning.DEFAULTS`: heal interval, food per health point, the floor to heal at, ticks per point of refill.
pub const HEAL_INTERVAL: u32 = 20;
pub const HEAL_COST: i32 = 1;
pub const MIN_TO_HEAL: i32 = 6;
pub const CYCLE: u32 = 160;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Food {
    pub level: i32,
    heal: u32,
    cycle: u32,
}

/// Full, like a respawn (`Vitals::default()` is every respawn and reset).
impl Default for Food {
    fn default() -> Self {
        Self { level: MAX_FOOD, heal: 0, cycle: 0 }
    }
}

impl Food {
    /// A meal: `n` food points, to the top at most.
    pub fn eat(&mut self, n: i32) {
        self.level = (self.level + n).clamp(0, MAX_FOOD);
    }

    /// Under the healing floor: the stamina recovers slower (`Stamina::hungry`).
    pub fn hungry(&self) -> bool {
        self.level < MIN_TO_HEAL
    }

    /// `SurvivalRules.step` for one 20 Hz tick: the health points to give now (0 or 1).
    pub fn tick(&mut self, hurt: bool) -> i32 {
        let mut heal = 0;
        if hurt && self.level >= MIN_TO_HEAL && self.level >= HEAL_COST {
            self.heal += 1;
            if self.heal >= HEAL_INTERVAL {
                (heal, self.level, self.heal) = (1, self.level - HEAL_COST, 0);
            }
        } else {
            self.heal = 0;
        }
        if self.level >= MAX_FOOD {
            self.cycle = 0;
        } else if self.level > 0 {
            self.cycle += 1;
            if self.cycle >= CYCLE {
                (self.level, self.cycle) = (self.level + 1, 0);
            }
        }
        heal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Healing pays 1 food a health point every 20 ticks from 6 up, refills 1 a 160 ticks, and a meal tops it up.
    #[test]
    fn food_only_pays_for_healing_and_refills_slowly() {
        let mut f = Food::default();
        assert_eq!((0..100).map(|_| f.tick(false)).sum::<i32>(), 0, "unhurt: nothing is spent");
        assert_eq!(f.level, MAX_FOOD);
        let healed: i32 = (0..60).map(|_| f.tick(true)).sum();
        assert_eq!((healed, f.level), (3, 17), "one every 20 ticks");
        f.level = 5;
        assert_eq!((0..100).map(|_| f.tick(true)).sum::<i32>(), 0, "under 6 does not heal");
        assert!(f.hungry());
        let mut f = Food { level: 5, ..Food::default() };
        (0..160).for_each(|_| { f.tick(false); });
        assert_eq!(f.level, 6, "160 ticks a point");
        f.eat(100);
        assert_eq!(f.level, MAX_FOOD);
    }
}
