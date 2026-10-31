//! C6: the weapon tooltip (`WeaponTooltip.linesFor`, `WeaponCategory`, `ConeSize`). **NOT b1.7.3.**
//! **UNVERIFIED: written without compiling or running anything.**
//!
//! One coloured line per fact under the item's name, only in a world with the Immersive Pack ON (`rules::swing` is `None`
//! otherwise) and only for what `rules::swing_of` calls a weapon or a tool. Every number is read from the code the swing runs
//! (`Swing`, `Profile::charge`, `special`'s constants), never recomputed, which is the mod's own rule for this class.
//!
//! Left out on purpose: the knockback lines (C2 does not apply a profile's knockback, so the number would be one the game does
//! not produce), `Targets` (the sweep has no cap here), the `Fusee` and arcabuz lines (C8), and the shotel's double cut (not
//! ported, so its charged damage is one cut). A pickaxe gets its category and durability only, as in the mod.

use super::profile::{charge_multiplier, Profile, Special};
use super::{rules, special};
use crate::world::craft;
use crate::world::items::ItemStack;

/// One line: its text and its colour (`TooltipColor`, as vanilla's `ChatFormatting` RGB).
pub type Line = (String, [f32; 4]);

fn rgb(c: u32) -> [f32; 4] {
    [(c >> 16 & 255) as f32 / 255.0, (c >> 8 & 255) as f32 / 255.0, (c & 255) as f32 / 255.0, 1.0]
}

const CATEGORY: u32 = 0x5555FF; // blue
const DAMAGE: u32 = 0xFF5555; // red
const CHARGED: u32 = 0xAA0000; // dark red
const SPEED: u32 = 0xFFFF55; // yellow
const RANGE: u32 = 0x55FFFF; // aqua
const SIZE: u32 = 0xAAAAAA; // grey
const SPECIAL: u32 = 0xFFAA00; // gold
const GOOD: u32 = 0x55FF55; // green
const LOW: u32 = 0xFF5555; // red
const NOTE: u32 = 0x808080; // dim

/// `WeaponCategory.forWeapon` (`LONG_REACH` 5, `HEAVY_TICKS` 19 with `HEAVY_REACH` 3.4, `SHORT_REACH` 3), tools first.
fn category(p: Profile, dist: f32, interval: u32) -> &'static str {
    if matches!(p, Profile::Pickaxe | Profile::Shovel | Profile::Hoe) {
        "Tool"
    } else if dist >= 5.0 {
        "Long Weapon"
    } else if interval >= 19 && dist >= 3.4 {
        "Heavy Weapon"
    } else if dist < 3.0 {
        "Short Weapon"
    } else {
        "Medium Weapon"
    }
}

/// `ConeSize.forAngle`: the horizontal half-angle as a radius band.
fn size(h: f32) -> &'static str {
    match h {
        h if h < 20.0 => "Very Small Radius",
        h if h < 29.0 => "Small Radius",
        h if h < 37.5 => "Medium Radius",
        h if h < 50.0 => "Wide Radius",
        h if h < 65.0 => "Very Wide Radius",
        _ => "Maximum Radius",
    }
}

/// The lines under the name of `s` (empty when the rule is off or the item is no weapon).
pub fn lines(s: ItemStack) -> Vec<Line> {
    of(s, rules::swing(Some(s.id)))
}

/// `lines` for a given swing (the host test has no world rule).
fn of(s: ItemStack, swing: Option<rules::Swing>) -> Vec<Line> {
    let Some(sw) = swing else { return Vec::new() };
    let mut v: Vec<Line> = vec![(category(sw.profile, sw.reach.dist, sw.interval).into(), rgb(CATEGORY))];
    if sw.profile != Profile::Pickaxe {
        v.push((format!("{:.1} Damage", sw.damage), rgb(DAMAGE)));
        let c = sw.profile.charge();
        if c.ticks > 0 {
            v.push((format!("{:.1} Charged Damage", sw.damage * charge_multiplier(c.bonus, 1.0)), rgb(CHARGED)));
        }
        v.push((format!("{:.1} Attacks per Second", 20.0 / sw.interval.max(1) as f32), rgb(SPEED)));
        v.push((format!("{:.1} Reach", sw.reach.dist), rgb(RANGE)));
        v.push((size(sw.reach.h).into(), rgb(SIZE)));
        match sw.profile.special() {
            Special::Lunge => v.push(("Right Click: forward lunge".into(), rgb(SPECIAL))),
            Special::MultiHit => v.push(("Right Click: hits everything in the cone".into(), rgb(SPECIAL))),
            Special::Parry => v.push(("Right Click: parry, returns the hit you take".into(), rgb(SPECIAL))),
            _ => {}
        }
    }
    if let Some(max) = craft::max_damage(s.id).filter(|&m| m > 0) {
        let left = max.saturating_sub(s.damage);
        let low = u32::from(left) * 5 <= u32::from(max); // vanilla's bar turns red at a fifth
        v.push((format!("Durability: {left}/{max}"), rgb(if low { LOW } else { GOOD })));
    }
    // `mechanicLines`: the aside of the four families that have one, with the numbers of the code that runs them.
    let secs = |t: u32| t as f32 / 20.0;
    match sw.profile {
        Profile::Ropera => v.push(("Right click: forward lunge, and it passes through several".into(), rgb(NOTE))),
        Profile::Halberd => v.push(("Right click: sweeps the whole arc, and costs the same cooldown as a hit".into(), rgb(NOTE))),
        Profile::Sabre => v.push((format!("Right click: parry. A {:.1} s window, a {:.1} s cooldown", secs(special::PARRY_WINDOW), secs(special::PARRY_COOLDOWN)), rgb(NOTE))),
        Profile::Warhammer => v.push((format!("Escalates with each consecutive hit; the streak lasts {:.1} s", secs(special::STREAK_WINDOW)), rgb(NOTE))),
        _ => {}
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::combat::catalog::{self, Family};

    // One check: an iron ropera reads from the swing (8.5 damage, 2.0 a second, no charge row, a lunge, 250 uses) and a dull item,
    // a world with the rule off and a pickaxe say what the doc comment promises.
    #[test]
    fn the_tooltip_reads_the_swing() {
        let id = catalog::id(Family::Ropera, 2).unwrap();
        let t: Vec<String> = of(ItemStack { id, count: 1, damage: 50 }, rules::swing_of(Some(id))).into_iter().map(|l| l.0).collect();
        assert_eq!(t[0], "Medium Weapon");
        assert!(t.contains(&"8.5 Damage".to_string()) && t.contains(&"2.0 Attacks per Second".to_string()));
        assert!(t.iter().any(|l| l.starts_with("Durability: 200/250")) && t.iter().any(|l| l == "Right Click: forward lunge"));
        assert!(!t.iter().any(|l| l.contains("Charged"))); // the ropera cannot charge
        assert!(of(ItemStack { id: 4, count: 1, damage: 0 }, rules::swing_of(Some(4))).is_empty()); // cobblestone
        assert!(of(ItemStack { id, count: 1, damage: 0 }, None).is_empty()); // the rule is off
        let pick = 257; // iron pickaxe: category and durability only
        let p = of(ItemStack { id: pick, count: 1, damage: 0 }, rules::swing_of(Some(pick)));
        assert_eq!(p.iter().map(|l| l.0.as_str()).collect::<Vec<_>>(), ["Tool", "Durability: 250/250"]);
    }
}
