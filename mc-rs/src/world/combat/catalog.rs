//! The five new melee families (`WeaponFamily`, `WeaponShape`, `CortesTier`). **NOT b1.7.3.**
//!
//! Ids are `400 + 5 * family + tier`, so there is no table to keep in step: `get(id)` is arithmetic plus `exists`.
//! They stay under 512 because `craft::shaped` packs a result as `id & 511` and `id >> 9` = damage.
//! Tiers use the order of `craft::MATERIALS`: wood 0, stone 1, iron 2, diamond 3, gold 4 (no copper, no netherite:
//! answer 6A). Of the mod's 7 tiers that leaves 20 items: dagger x5, ropera / alabarda / shotel x4 (no wood: the mod's
//! `isRapierMaterial`), martillo x3 (iron, diamond, gold: the mod's `isWarhammerMaterial` is rank >= copper, and stone
//! ranks below copper).
//!
//! Recipes: the grid rows are the mod's `WeaponShape` (`#` = empty). b1.7.3 has no nuggets and no emerald, so the
//! ropera's tip (`*`) is flint for stone (as in the mod) and the tier's own ingot or diamond for iron, diamond and
//! gold. // UNVERIFIED: the cost of that substitution is a guess, not the mod's.

use super::profile::{interval_ticks, Profile};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    Daga,
    Ropera,
    Alabarda,
    Martillo,
    Shotel,
}

pub const FAMILIES: [Family; 5] = [Family::Daga, Family::Ropera, Family::Alabarda, Family::Martillo, Family::Shotel];

/// First item id (a free id: b1.7.3 items stop at 359 and the records start at 2256).
pub const FIRST_ID: u16 = 400;
const TIERS: usize = 5;
/// `CortesTier.getAttackDamageBonus` in `craft::MATERIALS` order (wood, stone, iron, diamond, gold).
const BONUS: [f32; TIERS] = [0.0, 1.0, 2.0, 3.0, 0.0];
/// Body ingredient per tier: planks, cobblestone, iron ingot, diamond, gold ingot (`RecipesTools`' heads).
const HEAD: [u16; TIERS] = [5, 4, 265, 264, 266];
const IRON_BLOCK: u16 = 42;
const FLINT: u16 = 318;
const STICK: u16 = 280;

impl Family {
    fn index(self) -> u16 {
        self as u16
    }

    pub fn profile(self) -> Profile {
        match self {
            Family::Daga => Profile::Dagger,
            Family::Ropera => Profile::Ropera,
            Family::Alabarda => Profile::Halberd,
            Family::Martillo => Profile::Warhammer,
            Family::Shotel => Profile::Sabre,
        }
    }

    /// `WeaponFamily.attackDamage`: health points, before the tier's bonus.
    pub fn damage(self) -> f32 {
        match self {
            Family::Daga => 1.6,
            Family::Ropera => 6.5,
            Family::Alabarda => 4.0,
            Family::Martillo => 3.0,
            Family::Shotel => 2.7,
        }
    }

    /// `WeaponFamily.attackSpeed`: swings per second.
    pub fn speed(self) -> f32 {
        match self {
            Family::Daga => 3.2,
            Family::Ropera => 2.0,
            Family::Alabarda => 0.88,
            Family::Martillo => 1.05,
            Family::Shotel => 2.0,
        }
    }

    /// Ticks between two swings (`WeaponFamily.swingInterval`).
    pub fn interval(self) -> u32 {
        interval_ticks(self.speed())
    }

    /// Does the tier exist for this family?
    pub fn exists(self, tier: usize) -> bool {
        tier < TIERS
            && match self {
                Family::Daga => true,
                Family::Martillo => matches!(tier, 2..=4),
                _ => tier >= 1,
            }
    }

    /// `WeaponShape.rows` with `#` as a space: `X` body, `/` stick, `*` extra.
    fn rows(self) -> &'static [&'static str] {
        match self {
            Family::Daga => &["X", "/"],
            Family::Ropera => &["*  ", " X ", "  /"],
            Family::Alabarda => &["XX/", "X/X", "/  "],
            Family::Martillo => &["X*X", " / ", " / "],
            Family::Shotel => &["X  ", "XX/"],
        }
    }

    /// `WeaponFamily.extraIngredient` (`*`), 0 when the shape has none.
    fn extra(self, tier: usize) -> u16 {
        match self {
            Family::Ropera if tier == 1 => FLINT,
            Family::Ropera => HEAD[tier],
            Family::Martillo => IRON_BLOCK,
            _ => 0,
        }
    }

    fn name(self, tier: usize) -> &'static str {
        const N: [[&str; TIERS]; 5] = [
            ["Wooden Dagger", "Stone Dagger", "Iron Dagger", "Diamond Dagger", "Golden Dagger"],
            ["", "Stone Ropera", "Iron Ropera", "Diamond Ropera", "Golden Ropera"],
            ["", "Stone Alabarda", "Iron Alabarda", "Diamond Alabarda", "Golden Alabarda"],
            ["", "", "Iron War Hammer", "Diamond War Hammer", "Golden War Hammer"],
            ["", "Stone Shotel", "Iron Shotel", "Diamond Shotel", "Golden Shotel"],
        ];
        N[self as usize][tier]
    }
}

/// The item id of a family at a tier, `None` if that combination does not exist.
pub fn id(f: Family, tier: usize) -> Option<u16> {
    f.exists(tier).then(|| FIRST_ID + f.index() * TIERS as u16 + tier as u16)
}

/// The family and tier of an item id, `None` for any other item.
pub fn get(id: u16) -> Option<(Family, usize)> {
    let n = id.checked_sub(FIRST_ID)? as usize;
    let f = *FAMILIES.get(n / TIERS)?;
    f.exists(n % TIERS).then_some((f, n % TIERS))
}

/// Every (family, tier) that exists, family by family, weakest tier first.
pub fn all() -> impl Iterator<Item = (Family, usize)> {
    FAMILIES.into_iter().flat_map(|f| (0..TIERS).filter(move |&t| f.exists(t)).map(move |t| (f, t)))
}

/// Total damage of an item in health points: `WeaponFamily.attackDamage + CortesTier.getAttackDamageBonus`
/// (`CortesWeaponItem.totalAttackDamage`; the player's own base point is b1.7.3's hand, not added).
pub fn damage(id: u16) -> Option<f32> {
    get(id).map(|(f, t)| f.damage() + BONUS[t])
}

/// `Item.getItemName` translated, for `items::name`.
pub fn name(id: u16) -> Option<&'static str> {
    get(id).map(|(f, t)| f.name(t))
}

/// The recipe of an item: grid rows, the (key, item) pairs and the result id, for `craft::recipes()`.
pub fn recipe(f: Family, tier: usize) -> Option<(&'static [&'static str], [(char, u16); 3], u16)> {
    let out = id(f, tier)?; // first: `HEAD` / `extra` index by tier
    Some((f.rows(), [('X', HEAD[tier]), ('/', STICK), ('*', f.extra(tier))], out))
}

#[cfg(test)]
mod tests {
    use super::*;

    // One check for the catalogue: 20 distinct ids that round-trip, under 512, every recipe grid rectangular and
    // the numbers of the mod's WeaponCatalogueTest / WeaponKindsTest.
    #[test]
    fn twenty_weapons_with_round_tripping_ids() {
        let items: Vec<_> = all().collect();
        assert_eq!(items.len(), 5 + 4 + 4 + 3 + 4);
        let mut seen = std::collections::HashSet::new();
        for &(f, t) in &items {
            let i = id(f, t).unwrap();
            assert!(seen.insert(i) && (FIRST_ID..512).contains(&i), "{i}");
            assert_eq!(get(i), Some((f, t)));
            assert!(!name(i).unwrap().is_empty());
            let (rows, key, out) = recipe(f, t).unwrap();
            assert_eq!(out, i);
            assert!(rows.iter().all(|r| r.len() == rows[0].len()), "{f:?} grid is ragged");
            let cells: String = rows.concat();
            // every symbol the grid uses is defined (the mod's ShapedRecipePattern check), none is left unused
            for (c, item) in key {
                assert_eq!(cells.contains(c), item != 0, "{f:?} {t} key {c}");
            }
        }
        assert_eq!(get(399), None);
        assert_eq!(get(FIRST_ID + 1), None); // wooden ropera does not exist
        assert_eq!(get(425), None);
        assert_eq!(damage(id(Family::Ropera, 2).unwrap()), Some(8.5)); // 6.5 + iron's 2
        assert_eq!(damage(id(Family::Daga, 0).unwrap()), Some(1.6));
        assert_eq!(Family::Alabarda.interval(), 23);
        assert_eq!(Family::Daga.interval(), 6);
    }
}
