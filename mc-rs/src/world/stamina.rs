//! Stamina: a port of Stamina (insane96mcp, NeoForge 26.1.2), the parts mc-rs has. Read from its sources: `StaminaFeature` (the tick: consume,
//! regenerate, lock, slowdown, jump), `StaminaHandler` (maximum, lock), `StaminaFeatureClient` (the hearts overlay). Nothing here touches the
//! world: `Vitals` owns one (its `hurt` marks combat), `lib.rs` feeds `tick` once per 20 Hz tick and reads the numbers back, so the whole
//! state machine runs on a plain host (`cargo test -p mc-rs --lib stamina`). The numbers are the mod's defaults.
//!
//! Not ported: Vigour / Curse of Weariness, the Vigour effect and the `bonus_stamina` attribute (no enchanting, no potions, no attributes),
//! swimming (a sprint ends in water here), mining (`consumption$mine` is 0 by default), the hunger spend (`lock$consumeHungerRatio` is 0 and
//! b1.7.3 has no food bar), `lock$belowMaxHealth` (max health is a constant 20), creative / spectator (the debug Fly button stands in),
//! the config, the `/stamina` command, the network sync, the Mobs Properties Randomness and Rune Enchanting hooks.

use crate::world::biomelife::Air;
use crate::world::vitals::MAX_HEALTH;

/// `stamina$perHalfHeart`: 10 per health point (the mod counts half hearts), so full is 200 = 200 ticks of sprint at 1 a tick.
const PER_HALF_HEART: f32 = 10.0;
const FULL: f32 = MAX_HEALTH as f32 * PER_HALF_HEART;
/// `consumption$sprint`: a tick of sprinting; `consumption$jump`: a jump.
const SPRINT: f32 = 1.0;
const JUMP: f32 = 10.0;
/// `consumption$outOfCombatMultiplier`: x0.8 on the sprint when not hit or hurt in the last 300 ticks (15 s).
const CALM: f32 = 0.8;
const COMBAT: u16 = 300;
/// `regen$perTick`, `regen$modifierWhenLocked`.
const REGEN: f32 = 2.0;
const REGEN_LOCKED: f32 = 0.6;
/// `stamina$percentageReductionPerArmorPoint` and `regen$reductionPerArmorPoint` (both 0.025): armor shrinks the maximum and the regeneration.
const ARMOR: f32 = 0.025;
/// `lock$belowHealthRatio`: at or under this health ratio the stamina is emptied and locked; `lock$unlockAtHealthRatio`: it opens again at this stamina ratio.
const LOCK_BELOW: f32 = 0.20;
const UNLOCK_AT: f32 = 0.40;
/// `slowdown$sprinting$threshold` / `thresholdFlat` / `amount`, `slowdown$whenLocked$amount`.
const SLOW_BELOW: f32 = 0.20;
const SLOW_BELOW_FLAT: f32 = 40.0;
const SLOW: f32 = 0.15;
const SLOW_LOCKED: f32 = 0.10;
/// NOT Stamina's (`biomelife::air`, `biomeRules`): a sprint costs `1 + HOT * hot` times, and recovery is `1 - COLD * cold` times.
const HOT: f32 = 0.6;
const COLD: f32 = 0.5;
/// NOT the mod's (C4, `Food::hungry`): the recovery while the food bar is under the healing floor. UNVERIFIED by play.
const HUNGRY: f32 = 0.75;

#[derive(Clone, Copy, Debug)]
pub struct Stamina {
    pub cur: f32,
    /// `getMaxStamina` for the health and armor of the last tick: `max possible x health / max health`.
    pub max: f32,
    /// `STAMINA_LOCKED`: no sprinting until it regenerates to `UNLOCK_AT` of the maximum.
    pub locked: bool,
    /// Ticks left of `isInCombat` (`LAST_HIT` / `LAST_HURT`).
    combat: u16,
    /// NOT the mod's: the healing food bar is under its floor (`Food::hungry`, set by `lib.rs` each tick), so the recovery is x0.75.
    pub hungry: bool,
}

/// Full, like the mod's respawn (`setStamina(Float.MAX_VALUE)`): `Vitals::default()` is every respawn and reset.
impl Default for Stamina {
    fn default() -> Self {
        Self { cur: FULL, max: FULL, locked: false, combat: 0, hungry: false }
    }
}

impl Stamina {
    /// `getMaxPossibleStamina`: the maximum at full health.
    fn possible(armor: i32) -> f32 {
        (FULL * (1.0 - armor as f32 * ARMOR)).max(0.0)
    }

    /// `consumeStamina`: the lock closes when it runs out.
    fn take(&mut self, n: f32) {
        self.cur = (self.cur - n).max(0.0).min(self.max);
        self.locked |= self.cur <= 0.0;
    }

    /// `LivingDamageEvent.Post`: a hit dealt or taken puts the player in combat for 15 s.
    pub fn hit(&mut self) {
        self.combat = COMBAT;
    }

    /// `onPlayerJump`: a jump costs 10.
    pub fn jump(&mut self) {
        self.take(JUMP);
    }

    /// `StaminaHandler.canSprint` (the hunger spend is off).
    pub fn can_sprint(&self) -> bool {
        !self.locked
    }

    /// One 20 Hz tick of `StaminaFeature.onPlayerTick`: a sprint consumes; otherwise it regenerates (slower while locked); a low health empties and locks it.
    pub fn tick(&mut self, sprinting: bool, health: i32, armor: i32) {
        self.tick_in(sprinting, health, armor, Air::default());
    }

    /// `tick` in the heat or the cold of a biome (`Air::default()` is the mod's own).
    pub fn tick_in(&mut self, sprinting: bool, health: i32, armor: i32, air: Air) {
        self.combat = self.combat.saturating_sub(1);
        if health <= 0 {
            return;
        }
        let ratio = health.min(MAX_HEALTH) as f32 / MAX_HEALTH as f32;
        self.max = Self::possible(armor) * ratio;
        self.cur = self.cur.min(self.max); // `setStamina` clamps to the maximum, which falls with the health
        if sprinting {
            self.take(SPRINT * if self.combat == 0 { CALM } else { 1.0 } * (1.0 + HOT * air.hot));
        } else if self.cur != self.max && ratio > LOCK_BELOW {
            let (was, locked) = (self.cur / self.max, self.locked); // the unlock test reads the stamina of before this tick's regen, as the mod does
            self.cur = (self.cur + REGEN * (1.0 - armor as f32 * ARMOR) * if locked { REGEN_LOCKED } else { 1.0 } * (1.0 - COLD * air.cold) * if self.hungry { HUNGRY } else { 1.0 }).min(self.max);
            self.locked = locked && was < UNLOCK_AT;
        } else if !self.locked && ratio <= LOCK_BELOW {
            (self.cur, self.locked) = (0.0, true);
        }
    }

    /// `slowdownLocked` x `slowdownSprinting` (both `ADD_MULTIPLIED_TOTAL`, so they multiply): x0.9 while locked, x0.85 more while sprinting on
    /// under 40 or under 20% of the maximum. Multiplies the walking speed.
    pub fn speed(&self, sprinting: bool) -> f32 {
        let low = sprinting && (self.cur < SLOW_BELOW_FLAT || self.cur < SLOW_BELOW * self.max);
        (if self.locked { 1.0 - SLOW_LOCKED } else { 1.0 }) * (if low { 1.0 - SLOW } else { 1.0 })
    }

    /// `renderStamina`: of the `health` half hearts, how many the stamina still covers (`ceil(stamina / (max / health))`). The rest, counted
    /// from the right, are drawn grey over the hearts.
    pub fn halves(&self, health: i32) -> i32 {
        if health <= 0 || self.max <= 0.0 { 0 } else { ((self.cur * health as f32 / self.max).ceil() as i32).min(health) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ticks of sprinting until the lock closes, from full.
    fn run_dry(s: &mut Stamina) -> u32 {
        (1..).find(|_| { s.tick(true, MAX_HEALTH, 0); s.locked }).unwrap()
    }

    /// A sprint costs 1 a tick in combat (200 ticks from full) and 0.8 out of it (250); the last point closes the lock and nothing sprints.
    #[test]
    fn a_sprint_costs_a_tick_and_the_empty_bar_locks() {
        let mut s = Stamina::default();
        s.hit();
        for _ in 0..199 {
            s.tick(true, MAX_HEALTH, 0);
            s.hit();
        }
        assert!(!s.locked && s.can_sprint() && (s.cur - 1.0).abs() < 1e-3, "{s:?}");
        s.tick(true, MAX_HEALTH, 0);
        assert!(s.locked && !s.can_sprint() && s.cur == 0.0);
        assert_eq!(run_dry(&mut Stamina::default()), 250, "out of combat: x0.8");
        let mut s = Stamina::default();
        s.hit();
        s.tick(true, MAX_HEALTH, 0);
        (0..300).for_each(|_| s.tick(false, MAX_HEALTH, 0));
        s.cur = FULL;
        assert_eq!(run_dry(&mut s), 250, "the 15 s of combat run out after 300 ticks");
    }

    /// Locked it regenerates at 2 x 0.6 a tick and opens at 40% of the maximum (80, a tick late: the test reads the stamina before the regen);
    /// then it is 2 a tick, up to the maximum, and armor slows both the maximum and the regeneration.
    #[test]
    fn regeneration_unlocks_at_forty_percent() {
        let mut s = Stamina::default();
        run_dry(&mut s);
        let n = (1..).find(|_| { s.tick(false, MAX_HEALTH, 0); !s.locked }).unwrap();
        assert!((67..=69).contains(&n) && s.cur >= 80.0, "{n} {s:?}");
        (0..200).for_each(|_| s.tick(false, MAX_HEALTH, 0));
        assert_eq!(s.cur, FULL);
        let mut s = Stamina::default();
        s.tick(false, MAX_HEALTH, 20);
        assert_eq!(s.max, FULL / 2.0, "20 armor points: -50%");
        s.cur = 0.0;
        s.tick(false, MAX_HEALTH, 20);
        assert!((s.cur - 1.0).abs() < 1e-4, "2 x (1 - 0.5): {}", s.cur);
    }

    /// A jump costs 10: 19 leave 10, the 20th empties it and locks (the mod locks on every empty bar, jump or not); a jump costs nothing at 0.
    #[test]
    fn a_jump_costs_ten_and_the_last_ten_lock() {
        let mut s = Stamina::default();
        (0..19).for_each(|_| s.jump());
        assert!(!s.locked && s.cur == 10.0);
        s.jump();
        assert!(s.locked && s.cur == 0.0);
    }

    /// The maximum follows the health; at 20% of it the bar is emptied and locked, and it fills again once the health is above that.
    #[test]
    fn low_health_shrinks_the_bar_and_locks_it() {
        let mut s = Stamina::default();
        s.tick(false, 10, 0);
        assert_eq!((s.max, s.cur, s.locked), (100.0, 100.0, false));
        s.tick(false, 4, 0);
        assert_eq!((s.cur, s.locked), (0.0, true));
        s.tick(false, 4, 0);
        assert_eq!(s.cur, 0.0, "no regeneration at 20% health");
        s.tick(false, 5, 0);
        assert!(s.cur > 0.0 && s.locked, "{s:?}");
    }

    /// x0.9 locked, x0.85 sprinting under 40 or under 20% (100 of 200 is fine, 39 is not), the two multiply; the grey half hearts are the
    /// missing part of the bar counted from the right.
    #[test]
    fn slowdown_and_the_hearts_overlay() {
        let mut s = Stamina::default();
        assert_eq!((s.speed(false), s.speed(true)), (1.0, 1.0));
        s.cur = 39.0;
        assert_eq!((s.speed(false), s.speed(true)), (1.0, 0.85));
        s.locked = true;
        assert!((s.speed(true) - 0.9 * 0.85).abs() < 1e-6 && s.speed(false) == 0.9);
        let mut s = Stamina::default();
        assert_eq!(s.halves(20), 20, "full: no overlay");
        s.cur = 101.0;
        assert_eq!(s.halves(20), 11);
        s.cur = 0.0;
        assert_eq!(s.halves(20), 0, "empty: every heart grey");
        s.tick(false, 10, 0);
        s.cur = 50.0;
        assert_eq!(s.halves(10), 5, "at 5 hearts of health the bar is 100 and 50 covers 5 half hearts");
    }

    /// NOT Stamina's: the desert's sun makes a sprint dearer, the tundra's cold makes the rest slower; a still air is the mod's own.
    #[test]
    fn heat_and_cold_move_the_bar() {
        let (mut calm, mut hot) = (Stamina::default(), Stamina::default());
        let (dry_calm, dry_hot) = (run_dry(&mut calm), (1..).find(|_| { hot.tick_in(true, MAX_HEALTH, 0, Air { hot: 1.0, cold: 0.0 }); hot.locked }).unwrap());
        assert!(dry_hot < dry_calm && dry_hot as f32 > dry_calm as f32 / 1.7, "{dry_hot} < {dry_calm}");
        let (mut warm, mut cold) = (Stamina { cur: 50.0, ..Stamina::default() }, Stamina { cur: 50.0, ..Stamina::default() });
        warm.tick_in(false, MAX_HEALTH, 0, Air::default());
        cold.tick_in(false, MAX_HEALTH, 0, Air { hot: 0.0, cold: 1.0 });
        assert!((warm.cur - 50.0 - REGEN).abs() < 1e-4 && (cold.cur - 50.0 - REGEN * 0.5).abs() < 1e-4);
        let mut hungry = Stamina { cur: 50.0, hungry: true, ..Stamina::default() };
        hungry.tick_in(false, MAX_HEALTH, 0, Air::default());
        assert!((hungry.cur - 50.0 - REGEN * HUNGRY).abs() < 1e-4, "a hungry bar recovers slower");
    }
}
