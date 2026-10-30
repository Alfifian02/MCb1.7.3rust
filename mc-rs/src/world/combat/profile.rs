//! Weapon profiles and their default tables (`WeaponProfile`, `WeaponTables`, `ReachProfile`, `ChargeProfile`,
//! `KnockbackProfile`, `AttackTiming`). **NOT b1.7.3.**
//!
//! The mod keeps one `HashMap` per table and a test that every profile has a row. Here each table is an exhaustive
//! `match`, so a profile without a row does not compile (the invariant of `WeaponTablesTest`, checked by rustc).
//! Not ported: the data-component override layer and the YAML packs (answer 28A: static tables).

/// `WeaponProfile`. `Mace` and `Trident` have rows (mod completeness) but no item in b1.7.3 (answer 8C).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Profile {
    None,
    Unarmed,
    Sword,
    Axe,
    Pickaxe,
    Shovel,
    Hoe,
    Mace,
    Trident,
    Ropera,
    Halberd,
    Warhammer,
    Dagger,
    Sabre,
    Arcabuz,
}

pub const ALL: [Profile; 15] = {
    use Profile::*;
    [None, Unarmed, Sword, Axe, Pickaxe, Shovel, Hoe, Mace, Trident, Ropera, Halberd, Warhammer, Dagger, Sabre, Arcabuz]
};

/// `ReachProfile`: blocks, and the horizontal / vertical half-angle in degrees.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Reach {
    pub dist: f32,
    pub h: f32,
    pub v: f32,
}

/// `ChargeProfile`: `bonus` is the extra multiplier at a full charge, `ticks` the hold that gives it (0 = cannot charge).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Charge {
    pub bonus: f32,
    pub ticks: u32,
}

/// `KnockbackProfile`, blocks per tick.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Knockback {
    pub h: f32,
    pub v: f32,
}

/// `SpecialType`, only the values some profile's default uses (`slam`, `shove`, `throw` have no row in the mod's tables).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Special {
    None,
    MultiHit,
    Parry,
    Lunge,
    Fusee,
}

const fn r(dist: f32, h: f32, v: f32) -> Reach {
    Reach { dist, h, v }
}
const NO_CHARGE: Charge = Charge { bonus: 0.0, ticks: 0 };

impl Profile {
    /// `WeaponTables.reach`: what the left button hits with. Vertical is tighter than horizontal so a swing does not
    /// reach a mob at the feet or overhead.
    pub fn reach(self) -> Reach {
        match self {
            Profile::None => r(3.0, 25.0, 20.0),
            Profile::Unarmed => r(2.5, 28.0, 22.0),
            Profile::Sword => r(3.6, 35.0, 30.0),
            Profile::Axe => r(3.4, 35.0, 25.0),
            Profile::Pickaxe | Profile::Shovel => r(3.0, 35.0, 30.0),
            Profile::Hoe => r(3.2, 60.0, 50.0),
            Profile::Mace => r(3.4, 30.0, 40.0),
            Profile::Trident => r(5.3, 15.0, 25.0),
            Profile::Ropera => r(3.6, 13.0, 16.0),
            Profile::Halberd => r(5.0, 60.0, 35.0),
            Profile::Warhammer => r(3.4, 36.0, 36.0),
            Profile::Dagger => r(2.9, 49.0, 31.0),
            Profile::Sabre => r(2.25, 75.0, 25.0),
            Profile::Arcabuz => r(2.6, 46.0, 35.0),
        }
    }

    /// `WeaponTables.shotReach`: what the right button hits with; only the arcabuz has one (distance 0 = none).
    pub fn shot_reach(self) -> Reach {
        if self == Profile::Arcabuz { r(6.5, 32.0, 24.0) } else { r(0.0, 0.0, 0.0) }
    }

    /// `WeaponTables.charge`.
    pub fn charge(self) -> Charge {
        let c = |bonus, ticks| Charge { bonus, ticks };
        match self {
            Profile::Unarmed => c(0.3, 12),
            Profile::Sword | Profile::Trident => c(0.5, 25),
            Profile::Axe => c(2.0, 60),
            Profile::Shovel => c(0.4, 33),
            Profile::Halberd => c(0.6, 36),
            Profile::Warhammer => c(0.6, 32),
            Profile::Sabre => c(1.0, 20),
            Profile::None | Profile::Pickaxe | Profile::Hoe | Profile::Mace | Profile::Ropera | Profile::Dagger | Profile::Arcabuz => NO_CHARGE,
        }
    }

    /// `WeaponTables.knockback`, already clamped to the mod's ceilings (`KnockbackProfile.clampedHorizontal`).
    pub fn knockback(self) -> Knockback {
        let (h, v) = match self {
            Profile::None => (0.12, 0.02),
            Profile::Unarmed => (0.10, 0.02),
            Profile::Sword => (0.55, 0.10),
            Profile::Axe => (0.34, 0.08),
            Profile::Pickaxe => (0.22, 0.05),
            Profile::Shovel => (1.365, 0.156),
            Profile::Hoe => (0.06, 0.01),
            Profile::Mace => (0.30, 0.10),
            Profile::Trident => (0.30, 0.12),
            Profile::Ropera => (0.16, 0.03),
            Profile::Dagger => (0.14, 0.03),
            Profile::Halberd => (0.45, 0.14),
            Profile::Warhammer => (0.55, 0.30),
            Profile::Sabre => (0.38, 0.06),
            Profile::Arcabuz => (1.10, 0.18),
        };
        Knockback { h: h.clamp(0.0, KNOCK_MAX_H), v: v.clamp(0.0, KNOCK_MAX_V) }
    }

    /// `WeaponTables.special`: the right button.
    pub fn special(self) -> Special {
        match self {
            Profile::Hoe | Profile::Halberd => Special::MultiHit,
            Profile::Ropera => Special::Lunge,
            Profile::Sabre => Special::Parry,
            Profile::Arcabuz => Special::Fusee,
            Profile::None | Profile::Unarmed | Profile::Sword | Profile::Axe | Profile::Pickaxe | Profile::Shovel | Profile::Mace | Profile::Trident | Profile::Dagger | Profile::Warhammer => Special::None,
        }
    }
}

/// `KnockbackProfile.MAX_HORIZONTAL` / `MAX_VERTICAL`.
pub const KNOCK_MAX_H: f32 = 2.5;
pub const KNOCK_MAX_V: f32 = 0.70;

// ---- AttackTiming ----

/// Ticks of a swing with nothing in hand (`UNARMED_INTERVAL`), the longest interval, the invulnerability window a
/// weapon hit leaves (`IFRAME_TICKS`, answer 17B; b1.7.3's own window is 10) and the hold below which a release is a
/// plain tap (`MIN_CHARGE_HOLD_TICKS`).
pub const UNARMED_INTERVAL: u32 = 12;
pub const MAX_INTERVAL: u32 = 40;
pub const IFRAME_TICKS: u32 = 3;
pub const MIN_CHARGE_HOLD: u32 = 4;

/// `AttackTiming.ticksFor`: swings per second -> ticks between swings, `round(20 / speed)` in 1..=40; 12 for a speed that is not positive.
pub fn interval_ticks(speed: f32) -> u32 {
    if speed.is_nan() || speed <= 0.0 {
        return UNARMED_INTERVAL;
    }
    ((20.0 / speed as f64).round() as u32).clamp(1, MAX_INTERVAL)
}

/// `AttackTiming.chargeRatioFor`: 0 under 4 ticks of hold, then linear up to 1 at `full` ticks.
pub fn charge_ratio(held: u32, full: u32) -> f32 {
    if full == 0 || held < MIN_CHARGE_HOLD {
        0.0
    } else if full <= MIN_CHARGE_HOLD {
        1.0
    } else {
        ((held - MIN_CHARGE_HOLD) as f32 / (full - MIN_CHARGE_HOLD) as f32).min(1.0)
    }
}

/// `AttackTiming.chargeMultiplier`: `1 + bonus * ratio`; a charge never subtracts.
pub fn charge_multiplier(bonus: f32, ratio: f32) -> f32 {
    1.0 + bonus.max(0.0) * ratio.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // One check for the arithmetic and the tables' sanity (the mod's AttackTimingTest + WeaponTablesTest).
    #[test]
    fn tempo_charge_and_tables_match_the_mod() {
        assert_eq!(interval_ticks(2.0), 10); // ropera, shotel
        assert_eq!(interval_ticks(0.88), 23); // alabarda: 22.7
        assert_eq!(interval_ticks(1.05), 19); // martillo: 19.05
        assert_eq!(interval_ticks(3.2), 6); // dagger
        assert_eq!(interval_ticks(1.6), 12); // vanilla sword: the f32 1.6 widens to 1.6000000238, so 12.4999998 (the Java widens the same way)
        assert_eq!(interval_ticks(0.1), 40); // capped
        assert_eq!(interval_ticks(0.0), 12);
        assert_eq!(charge_ratio(3, 25), 0.0); // under the minimum hold
        assert_eq!(charge_ratio(4, 25), 0.0);
        assert_eq!(charge_ratio(14, 25), 10.0 / 21.0);
        assert_eq!(charge_ratio(99, 25), 1.0);
        assert_eq!(charge_multiplier(2.0, 0.5), 2.0);
        assert_eq!(charge_multiplier(-1.0, 1.0), 1.0);
        for p in ALL {
            let (k, c, re) = (p.knockback(), p.charge(), p.reach());
            assert!(k.h <= KNOCK_MAX_H && k.v <= KNOCK_MAX_V, "{p:?}");
            assert!(re.dist > 0.0 && re.h > 0.0 && re.v > 0.0 && re.h < 90.0 && re.v < 90.0, "{p:?}");
            assert_eq!(c.ticks == 0, c.bonus == 0.0, "{p:?}: a charge has both a bonus and a hold, or neither");
            assert_eq!(p.shot_reach().dist > 0.0, p.special() == Special::Fusee, "{p:?}");
        }
        assert_eq!(Profile::Ropera.special(), Special::Lunge);
        assert_eq!(Profile::Dagger.charge(), NO_CHARGE); // "can't charge"
    }
}
