# Cortés Combat port (NOT b1.7.3)

Port of the NeoForge 1.21.1 mod `cortes-combat` 1.4.12 (GPL-3.0-only; the port is GPL-3.0 too, the owner's permission is on record: answer 4B) into `mc-rs`. Everything here follows [`design.md`](design.md): two hooks minimum, nothing old becomes pointless, marked NOT b1.7.3, **exclusive to the Immersive Pack**: every Cortes Combat feature is on only in a world created with the Immersive Pack ON (`pack_rule`, the 4th line of `<world>/info`), and with it OFF the b1.7.3 combat plays as before (owner's decision, 2026-10-31; it replaces the `combatRules` option this plan had). Written without a toolchain: not compiled, not run (answer 31B); each module carries one `#[test]` for when it is.

## Decisions (from the 2026-10-30 questionnaire)

| Topic | Decision |
|---|---|
| Scope | Core first, then step by step; order: weapons, cone, cooldown, charge, guard, special, the rest |
| Families | ropera, alabarda, martillo, daga, shotel now; arcabuz later (C8) |
| Tiers | b1.7.3 only: wood, stone, iron, diamond, gold (no copper, no netherite); 20 items, ids 400..=424 |
| Old tools | sword / axe / pickaxe / shovel / hoe join the new system through their profiles (C2); hoe already exists. Mace and trident: profile rows only, no item |
| Damage | health points as `f32`; the b1.7.3 `i32` path is changed at its root in C2 (every caller of `Mob::damage` / `hit` checked) |
| Cooldown / charge / guard / special | full port; touch HUD: hold the attack button to charge, a guard button, a special button shown only when the weapon has one |
| Knockback / iframes | per the mod (`IFRAME_TICKS` 3 for weapon hits instead of b1.7.3's 10) |
| Hunger | not ported as in the mod; a food bar that only pays for healing, joined to `Stamina` (design.md "Sprint and stamina vs food") |
| Dropped | enchantments (and their system), Nether Reactor + Firestone, YAML weapon packs, `/cc` commands, copper and netherite gear |
| Visuals | simple matrix swing per family, mod PNGs in the item atlas, slash particles + light camera shake, no third-person poses; swing / hit / charged sounds into `audio/table.rs`; tooltip with reach, arc, cooldown |
| Code | `mc-rs/src/world/combat/`, pure types; small hooks in `lib.rs`, `mobs.rs`, `craft.rs`, `items.rs` |
| Device | OPPO A5i (low-end Android, touch only) |

## Milestones

| # | What | State |
|---|---|---|
| C1 | `combat/{cone,profile,catalog}.rs`: cone math, profile tables, tempo / charge arithmetic, 20 weapons (ids, names, damage, durability, recipes in `craft::recipes()` so the recipe book lists them), `mobs::attack` gives them their damage (rounded) | **written, not compiled** (+3 tests) |
| C2 | Immersive Pack gate (`combat::rules`); `f32` damage; cone targeting replaces the single ray for the held item (the bare hand too); attack cooldown per family; old tools get their profile; 3-tick damage window | written 2026-10-31, **UNVERIFIED** (not compiled, not run) |
| C3 | Charge (hold the attack button), HUD bar and cooldown mark | |
| C4 | Guard + stamina, eat interrupt, healing food bar | |
| C5 | Specials: lunge (ropera), multi_hit (alabarda, hoe), parry + double cut (shotel), streak (martillo) | |
| C6 | Item sprites (mod PNGs into `items.rgba`, `tools/`), held-item model, tooltip lines | |
| C7 | Slash particles, camera shake, sounds | |
| C8 | Arcabuz | |

## C1 notes and gaps

* Item ids are `400 + 5 * family + tier`; they stay under 512 because `craft::shaped` packs `id & 511` and `id >> 9` (damage).
* Recipes: the mod's `WeaponShape` grids. b1.7.3 has no nuggets or emerald, so the ropera tip is flint (stone, as in the mod) or the tier's own ingot / diamond. UNVERIFIED cost. The martillo's block is an iron block for every tier, as in the mod.
* The mod's `src/generated` recipe JSON is older than `WeaponShape` (it shows a vertical ropera and an off-centre martillo); the code was followed.
* Damage is `family + tier bonus` (`CortesWeaponItem.totalAttackDamage`): ropera 6.5, alabarda 4, martillo 3, daga 1.6, shotel 2.7 plus wood 0 / stone 1 / iron 2 / diamond 3 / gold 0.
* Not done yet, found while hooking: `world/pack.rs` (satchel categories) and `render/player.rs` / `lib.rs` (held model, HUD shape) only know `craft::tool`, so the weapons show as a flat tier colour and sit in no satchel category until C6.
* Vanilla 1.21 attack speeds for sword / axe / pickaxe / shovel / hoe are **from memory, UNVERIFIED** (owner accepted it). The mod has no `AxeBalance` class, only `AxeBalanceTest`: the axe keeps its damage and only gets `TEMPO[AXE]` 0.85. `SwordRebalance` (sword one lower) and `HoeRebalance` (+1.5) are applied in `combat::rules`. The bare hand is 4 ticks (`4.0 * 1.25`), not `UNARMED_INTERVAL` (12, the fallback for a non-positive speed).
* C2 leaves out knockback per profile (not in any milestone).
* C2 gate: one process-wide flag (`rules::set`, from `enter_world`), hidden recipes in `craft::find` and `book::in_cat`; `Mob.health` is `f32` and `Mobs::strike` / `Mobs::cone` are new.
