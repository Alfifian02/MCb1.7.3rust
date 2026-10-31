//! C2: the world rule and the glue between the pure core and the b1.7.3 base. **NOT b1.7.3.**
//! **UNVERIFIED: written without compiling or running anything.**
//!
//! Everything from Cortes Combat is exclusive to a world with the Immersive Pack ON (`menu::Info::pack`, set by
//! `enter_world`). With it OFF, `swing` is `None` and the b1.7.3 combat plays exactly as before: the 20 weapons
//! cannot be crafted (`craft::find`, the recipe book), and the old tools keep their b1.7.3 damage, the ray target,
//! the 10-tick damage window and no cooldown.

use super::catalog;
use super::profile::{interval_ticks, Profile, Reach};
use crate::world::craft::{self, Kind};
use std::sync::atomic::{AtomicBool, Ordering};

// ponytail: one flag for the whole process, because the game holds one world at a time. Two worlds at once would need it
// threaded through `mobs::attack`, `craft::find` and the recipe book instead.
static ON: AtomicBool = AtomicBool::new(false);

/// `enter_world`: the world's Immersive Pack rule.
pub fn set(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

/// Is the loaded world playing with Cortes Combat?
pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// What one swing of the held item is: its profile and cone, the ticks until the next swing, the damage in health points
/// (the player's own point included) and the wear it costs.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Swing {
    pub profile: Profile,
    pub reach: Reach,
    pub interval: u32,
    pub damage: f32,
    pub wear: u16,
}

/// The bare hand: attack speed 4.0 (the player's own) at `WeaponStats.TEMPO[UNARMED]` 1.25, which is 4 ticks.
const BARE_SPEED: f32 = 4.0;
const TEMPO_UNARMED: f32 = 1.25;
/// `WeaponStats.TEMPO[AXE]`: 0.85 of vanilla's speed (the sword's row was deleted; `SwordTempoTest`).
const TEMPO_AXE: f32 = 0.85;
/// `SwordRebalance`: the sword's modifier is lowered by one. `HoeRebalance.ADDED`: the hoe's is raised by 1.5.
const SWORD_LOWERED: f32 = 1.0;
const HOE_ADDED: f32 = 1.5;

/// Vanilla 1.21 attack speed (swings per second) of an old tool, by `craft::MATERIALS` order wood, stone, iron, diamond, gold.
/// UNVERIFIED: from memory of 1.21, not read from the game's files (the user accepted that).
fn old_speed(kind: Kind, m: usize) -> f32 {
    match kind {
        Kind::Sword => 1.6,
        Kind::Pickaxe => 1.2,
        Kind::Shovel => 1.0,
        Kind::Axe => [0.8, 0.8, 0.9, 1.0, 1.0][m],
        Kind::Hoe => [1.0, 2.0, 3.0, 4.0, 1.0][m],
    }
}

fn old_profile(kind: Kind) -> Profile {
    match kind {
        Kind::Sword => Profile::Sword,
        Kind::Axe => Profile::Axe,
        Kind::Pickaxe => Profile::Pickaxe,
        Kind::Shovel => Profile::Shovel,
        Kind::Hoe => Profile::Hoe,
    }
}

/// The swing of `held` (`None` = the bare hand), whatever the world rule. `None` for an item that is not a weapon or a tool.
/// Damage is the b1.7.3 total (`ItemSword` 4 + material, `ItemTool` 2 + material, the hand 1), with the mod's own changes on
/// top: the sword one lower, the hoe 1.5 higher, the axe and the others as they were. The 20 weapons are the catalog's.
pub fn swing_of(held: Option<u16>) -> Option<Swing> {
    let Some(id) = held else {
        let profile = Profile::Unarmed;
        return Some(Swing { profile, reach: profile.reach(), interval: interval_ticks(BARE_SPEED * TEMPO_UNARMED), damage: 1.0, wear: 0 });
    };
    if let Some((family, _)) = catalog::get(id) {
        let profile = family.profile();
        return Some(Swing { profile, reach: profile.reach(), interval: family.interval(), damage: catalog::damage(id)?, wear: 1 });
    }
    let (kind, m) = craft::tool(id)?;
    let bonus = [0.0, 1.0, 2.0, 3.0, 0.0][m];
    let (damage, wear) = match kind {
        Kind::Sword => (4.0 + bonus - SWORD_LOWERED, 1),
        Kind::Hoe => (1.0 + HOE_ADDED, 0),
        _ => (2.0 + bonus, 2),
    };
    let tempo = if kind == Kind::Axe { TEMPO_AXE } else { 1.0 };
    let profile = old_profile(kind);
    Some(Swing { profile, reach: profile.reach(), interval: interval_ticks(old_speed(kind, m) * tempo), damage, wear })
}

/// `swing_of` for a world with the Immersive Pack ON, and `None` for one with it OFF: the one gate of the combat.
pub fn swing(held: Option<u16>) -> Option<Swing> {
    if on() { swing_of(held) } else { None }
}

/// Is `id` one of the 20 weapons (an item that exists only in an Immersive Pack world)?
pub fn is_weapon(id: u16) -> bool {
    catalog::get(id).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swings_follow_the_mod_and_the_old_tools_keep_their_base() {
        // The bare hand: 4 ticks (`IframesTest`: bareHand == 4), 1 damage, no wear.
        let hand = swing_of(None).unwrap();
        assert_eq!((hand.interval, hand.damage, hand.wear), (4, 1.0, 0));
        // A wooden sword (268): 12 ticks (1.6), 4 - 1 = 3 damage. An iron axe (258): 0.9 * 0.85 = 0.765 -> 26 ticks, 2 + 2 = 4.
        let sword = swing_of(Some(268)).unwrap();
        assert_eq!((sword.interval, sword.damage, sword.wear), (12, 3.0, 1));
        assert_eq!(swing_of(Some(258)).map(|s| (s.interval, s.damage)), Some((26, 4.0)));
        // A wooden hoe (290): 1.0 + 1.5 = 2.5 damage, no wear. A diamond pickaxe (278): 1.2 -> 17 ticks, 2 + 3 = 5.
        assert_eq!(swing_of(Some(290)).map(|s| (s.damage, s.wear)), Some((2.5, 0)));
        assert_eq!(swing_of(Some(278)).map(|s| (s.interval, s.damage)), Some((17, 5.0)));
        // The first weapon of the catalog is the wooden dagger, and a bone is not a weapon.
        assert_eq!(swing_of(Some(catalog::FIRST_ID)).map(|s| s.profile), Some(Profile::Dagger));
        assert_eq!(swing_of(Some(352)), None);
    }
}
