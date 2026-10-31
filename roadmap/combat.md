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
| C3 | Charge (hold the attack button), HUD bar and cooldown mark | written 2026-10-31, **UNVERIFIED** (not compiled, not run; +1 test in `combat::rules`) |
| C4 | Guard + stamina, eat interrupt, healing food bar | written 2026-10-31, **UNVERIFIED** (not compiled, not run; +3 tests: `combat::{guard,survival}`, `stamina`) |
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

## C3 notes and gaps

* Charge: a look finger (or the pad's attack) held still on a mob in the cone of a weapon with a `Profile::charge` row builds `App::charge` at 20 Hz, up to the row's `ticks`, and only once the cooldown is over. Lifting the finger delivers one blow with `rules::charged` (`1 + bonus * ratio`, `None` under `MIN_CHARGE_HOLD`): `touch.take_release` (a still hold of `HOLD_SECS` or more that ends) makes `place` true for the usual attack path, which multiplies the damage by `App::blow`. A shorter press is the plain tap, as before. Dragging the finger cancels the charge.
* While `charge > 0` the dig is held back, so a hold on a mob no longer digs the block behind it (only for weapons that can charge).
* HUD (`lib.rs`, under the crosshair, only with the rule on): the charge bar (white, gold when full, a red tick at the minimum hold) and, outside a charge, a thin cooldown bar that fills until the next swing.
* Gaps: the minimum hold is `HOLD_SECS` (0.4 s = 8 ticks) on touch, so a tap and a charge never overlap but `charge_ratio` starts at 0.19 for the sword; the release uses the previous frame's block target for the wall test (ponytail); no sound or particles for a full charge (C7); `Mobs::strike` knockback still ignores the charge.

## C3 addendum: the charge animation

* While a charge builds the first-person hand repeats the hotbar switch (`App::charge_phase` overrides `Hand.equip`): the weapon starts low and rises, one rise per cooldown of the weapon (`Swing::interval`), looping while the hold goes on; at a full charge it stays raised. Owner's request, 2026-10-31. One line to change if one rise over the whole charge is wanted instead.

## C4 notes and gaps

* **Guard** (`combat/guard.rs`, port of `GuardProfile` / `GuardState` / `GuardRules`): only the sword family guards (`GuardProfile.SWORD`: 1 point, 90 degrees, a point back every 40 ticks, 100 ticks of stagger when it is spent); none of the five new families does (`WeaponFamily.guard`), and b1.7.3 has no shield. The roadmap's "stamina" is the mod's guard points; the sprint `Stamina` is joined to the food bar instead (below). Holding is free, a blocked hit costs a point, the last one breaks the guard and the charge, raising it ends a charge, and a raised or staggered sword cannot swing (`AttackCooldown.beginCharge`).
* Input: a guard button on the touch HUD above the jump button, drawn and hit only while the held item can guard (`TouchUi::guard_shown`), with one orb a point. It comes up on the press, never by itself when a stagger ends. No pad bind yet (X and Down are free).
* Block: `Ev::Hurt` now carries the source's position (`Ev::Hurt(damage, from)`); `App::guarded` measures the arc on the ground plane (the mod's is 3D). Deviation: a block opens b1.7.3's 10-tick damage window and hits inside it are free, instead of the mod's one point per attacker per tick (`GuardStamp`), so a slime touching every tick costs one point. Explosions, fire, falls and the cactus are not blocked (the mod blocks explosions at half, UNVERIFIED here).
* **Food** (`combat/survival.rs`, `SurvivalRules.step` with `SurvivalTuning.DEFAULTS`): 20 points, a hurt player with 6 or more pays 1 for 1 health point every 20 ticks, 1 point back every 160 ticks while not full. Eating adds the meal's `heal_amount` as food points (UNVERIFIED mapping) and no longer heals on the tap. Starvation is not ported (b1.7.3 has no difficulty to scale it): an empty bar stops healing and refilling until a meal. The bar is 10 plain squares on the hearts' row, right-aligned, until C6's sprites.
* **Joined to Stamina**: `Vitals` owns both and refills both on a respawn; a food bar under the healing floor (6) slows the stamina's recovery to x0.75 (`Stamina::hungry`; my number, not the mod's, UNVERIFIED by play).
* **Eat interrupt** (`EatInterruptHandler`): a meal takes 32 ticks (`App::eating`, progress shown on the bar under the crosshair); damage taken ends it unless the guard is up, as do a slot change, a screen or death. The "guard and eat at once" rule is in the code but dormant: it needs an off hand, and b1.7.3 has one item in hand. A tap starts the meal and it runs by itself (the mod holds the button).
* Hooks: `Vitals` (+`food`, `guard`), `Stamina` (+`hungry`), `mobs::Ev::Hurt` (+position), `touch_ui` (`PointerRole::Guard`), `lib.rs` (`survival_tick`, `eat`, the HUD).
