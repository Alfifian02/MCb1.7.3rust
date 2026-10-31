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
//! * `guard`   - C4: the sword's guard (points, arc, stagger) and the block. UNVERIFIED: not compiled.
//! * `survival` - C4: the healing food bar.
//! * `rules`   - C2: the world rule and the swing of whatever is held (cone, cooldown, damage); C3: `charged`, the multiplier of a held-then-released blow. UNVERIFIED: not compiled.
//!
//! **Exclusive to the Immersive Pack.** Every feature from Cortes Combat is on only in a world created with the Immersive
//! Pack ON (`pack_rule`, `rules::set` from `enter_world`); with it OFF the b1.7.3 combat plays as before. This replaces
//! the `combatRules` option the roadmap planned. Hooks into the base (design.md rule 1): `craft::find` and the recipe
//! book (the 20 weapons), `craft::max_damage`, `items::{name, max_stack, tile}`, `mobs::{attack, strike, cone}`.

pub mod catalog;
pub mod cone;
pub mod guard;
pub mod profile;
pub mod rules;
pub mod survival;
