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
| C5 | Specials: lunge (ropera), multi_hit (alabarda, hoe), parry (shotel), streak (martillo). Double cut: **not done, see C5 notes** | written 2026-10-31, **UNVERIFIED** (not compiled, not run; +1 test in `combat::special`) |
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

## C3 fix: the rise during the cooldown

* Bug: `charge_tick` returned at once while the cooldown ran, so `charge` stayed 0 and `charge_phase` gave `None`: a held finger showed no rise until the cooldown was over. Now a still finger on a mob in the cone of a weapon that can charge sets `App::cd_rise` during the cooldown and the hand rises with the cooldown itself (`1 - swing_cd / interval`); the charge builds from the end of it, and its own rises follow. A guard up or staggered still shows nothing. The dig stays held back while `cd_rise` is set, like while `charge > 0`. Reading of the report (UNVERIFIED): "the equip animation does not appear when the cooldown charge happens" = a hold during the cooldown. One line to change if the rise should show for every swing, held or not: drop the `cd_rise` test in `charge_phase`.

## C4 notes and gaps

* **Guard** (`combat/guard.rs`, port of `GuardProfile` / `GuardState` / `GuardRules`): only the sword family guards (`GuardProfile.SWORD`: 1 point, 90 degrees, a point back every 40 ticks, 100 ticks of stagger when it is spent); none of the five new families does (`WeaponFamily.guard`), and b1.7.3 has no shield. The roadmap's "stamina" is the mod's guard points; the sprint `Stamina` is joined to the food bar instead (below). Holding is free, a blocked hit costs a point, the last one breaks the guard and the charge, raising it ends a charge, and a raised or staggered sword cannot swing (`AttackCooldown.beginCharge`).
* Input: a guard button on the touch HUD above the jump button, drawn and hit only while the held item can guard (`TouchUi::guard_shown`), with one orb a point. It comes up on the press, never by itself when a stagger ends. No pad bind yet (X and Down are free).
* Block: `Ev::Hurt` now carries the source's position (`Ev::Hurt(damage, from)`); `App::guarded` measures the arc on the ground plane (the mod's is 3D). Deviation: a block opens b1.7.3's 10-tick damage window and hits inside it are free, instead of the mod's one point per attacker per tick (`GuardStamp`), so a slime touching every tick costs one point. Explosions, fire, falls and the cactus are not blocked (the mod blocks explosions at half, UNVERIFIED here).
* **Food** (`combat/survival.rs`, `SurvivalRules.step` with `SurvivalTuning.DEFAULTS`): 20 points, a hurt player with 6 or more pays 1 for 1 health point every 20 ticks, 1 point back every 160 ticks while not full. Eating adds the meal's `heal_amount` as food points (UNVERIFIED mapping) and no longer heals on the tap. Starvation is not ported (b1.7.3 has no difficulty to scale it): an empty bar stops healing and refilling until a meal. The bar is 10 plain squares on the hearts' row, right-aligned, until C6's sprites.
* **Joined to Stamina**: `Vitals` owns both and refills both on a respawn; a food bar under the healing floor (6) slows the stamina's recovery to x0.75 (`Stamina::hungry`; my number, not the mod's, UNVERIFIED by play).
* **Eat interrupt** (`EatInterruptHandler`): a meal takes 32 ticks (`App::eating`, progress shown on the bar under the crosshair); damage taken ends it unless the guard is up, as do a slot change, a screen or death. The "guard and eat at once" rule is in the code but dormant: it needs an off hand, and b1.7.3 has one item in hand. A tap starts the meal and it runs by itself (the mod holds the button).
* Hooks: `Vitals` (+`food`, `guard`), `Stamina` (+`hungry`), `mobs::Ev::Hurt` (+position), `touch_ui` (`PointerRole::Guard`), `lib.rs` (`survival_tick`, `eat`, the HUD).

## C5 notes and gaps

* **Code**: `combat/special.rs` (numbers and state, pure), `Mobs::{cone_n, in_box, reflect}`, `touch_ui` (`PointerRole::Special`, `TouchUi::{special_shown, take_special}`, `LayoutRects::special`), `lib.rs` (`special_tick`, `special_press`, `lunge_tick`, `parried`, the movement and HUD lines). The button is where the guard's is (no weapon has both: only the sword guards, and it has no special); it is drawn only while the held item has one, with an icon (arrow, fan, cross) and a veil that sinks as the price is paid. No pad bind yet.
* **Lunge** (ropera): 8 swings of cooldown, `6 x (1 + 0.02 x tier)` blocks, at most 20 ticks, ends on a wall or under 0.02 blocks a tick. While it runs the body is driven along the facing (over the walk, the sprint and any push) and the player cannot be hurt (`LungeImmunity`). Each tick the cube of side `reach` round the player strikes every mob it meets once, nearest first: `0.75 x damage x min(4, 1 + 0.375 x n)`, n = the target's place in that tick. Deviations: a fixed pace of 18 blocks a second instead of the mod's impulse and drag (UNVERIFIED by play); no status effect, no armour piercing, no knockback of its own.
* **Sweep** (alabarda, hoe: `MultiHit`): 4 swings of cooldown, spent even on air, every mob in a cone 1.35 x as long and wide (same height) takes the weapon's flat damage, no charge. Deviation: the knockback is `Mobs::strike`'s, not the sweep's 0.8 share; no item wear (the mod's `hurt` call does not wear it either).
* **Parry** (shotel): a 12 tick window, a 100 tick lockout from the press, the walk x0.4 while the blade is out (the mod applies Slowness amplifier 3 = level IV; its javadoc says 75 %, the code was followed). A hit in the window (`Ev::Hurt`) does nothing, closes the window and goes back to the mob standing at its source at the same strength, through a fresh damage window; an arrow is parried too and nobody answers it. Not through the attack cooldown. Deviations: the slowness ends with the window (the mod's effect outlives a parry that was used); explosions and falls are not `Ev::Hurt`, so they are not parried, as in the mod (only a living attacker is).
* **Streak** (martillo): `+0.15` of the multiplier per hit of a live streak (80 ticks since the last), counted before the blow being swung, added to the charge multiplier; one gold orb a hit on the HUD (nine at most). Deviation: every blow that connects counts, even one the damage window refused (the martillo's 19 ticks are longer than the 3 of the window, so it never happens). The streak's pitching hit sound is C7.
* **Not done: Corte Gemelo (the shotel's "double cut")**. In the mod it is an *enchantment* (`CORTE_GEMELO`, level 1 every 4 s, level 2 always, `CORTE_GEMELO_SHARE` of the damage); enchantments were dropped (decisions table), so no part of it is in the sabre's own code. The roadmap row listed it anyway: **owner's decision needed** (port it as an intrinsic of the shotel, at which level, or leave it out). The other enchantments of the special classes (Audacia, Retirada, Barredor, Segunda Pasada, Reflejo de Acero, Capacitor, Vampirico, Ritmo de Hierro, Sentencia) are out for the same reason.
* Also out: the arcabuz's `Fusee` (C8), sounds and particles (C7), the third-person pose, and a lunge that keeps its mob indices when one is removed mid-lunge (`ponytail`: one mob skipped or struck twice at worst).
