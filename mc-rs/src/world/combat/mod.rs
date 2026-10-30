//! Cortés Combat port (C1: the pure core). **NOT b1.7.3.**
//!
//! Source: the NeoForge 1.21.1 mod `cortes-combat` 1.4.12 (GPL-3.0-only), `core/weapon/*`, `reach/ConeMath`,
//! `item/WeaponTables`, `item/WeaponFamily`, `attack/AttackTiming`. Nothing here touches the world, the renderer or
//! `lib.rs`: it is numbers and geometry, so a host test can open it (the mod's own rule for its `core` package).
//!
//! * `cone`    - reach is a cone: distance, horizontal and vertical half-angle, with the close-range leash.
//! * `profile` - the per-profile tables (reach, charge, knockback, special) and the tempo / charge arithmetic.
//! * `catalog` - the five new melee families, their ids (400..=424), names, damage and recipes.
//!
//! Hooks into the b1.7.3 base (design.md rule 1): `craft::recipes()` (recipe book included), `craft::max_damage`,
//! `items::{name, max_stack, tile}` and `mobs::attack`. The switch (`combatRules`, default off, design.md rule 5)
//! arrives with C2, the first milestone that changes how an old weapon plays.

pub mod catalog;
pub mod cone;
pub mod profile;
