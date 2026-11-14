# Controlify port, phase 5: vibration and reach-around

Source: Controlify `rumble/*`, `controller/rumble/*`, `mixins/feature/rumble/*`, `reacharound/*`, `mixins/feature/reacharound/*`.
Code: `mc-rs/src/input/rumble_fx.rs` (engine, effects), `input/reach.rs`, `rumble.rs` (Android side), the hooks in `lib.rs`, the rows in
`input/gamepad.rs` (`PadCfg`) and `options.rs`. Status: **not compiled, not run on a device.**

Marks: **PORTED**, **DEVIATES** (ported with a difference, said why), **NOT PORTED** (b1.7.3 has no such thing, or Android has no API, or it was left out),
**UNVERIFIED** (written from the sources, never run).

## Vibration

| Controlify | mc-rs | |
|---|---|---|
| `RumbleManager` (queue, `tick`, `setSilent`, `clearEffects`, one `NONE` when idle) | `rumble_fx::Manager` | PORTED |
| `BasicRumbleEffect` (`constant`, `byTick`, `byTime`, `empty`, `seq`, `repeat`, `prioritised`, `earlyFinish(finishOnScreenChange)`) | `rumble_fx::Effect` | PORTED. `priority` is kept but, as in Java, changes nothing (the states are combined with `max`) |
| `ContinuousRumbleEffect` (`timeout`, `minTime`, `stop`, `heartbeat`) | `Effect::continuous` + `Shape` | DEVIATES: the state function is a fixed `Shape` (constant, block break, slow block, guard) instead of a lambda; `inWorld(...)` and `stopCondition(...)` are not ported (no mixin uses them but the server one) |
| `RumbleSource` (master, player, world, interaction, gui) and `applyRumbleStrength` (master x source) | `Source`, `Strengths::apply` | PORTED |
| Settings: Allow vibration, strength per source (0 to 200 %) | Controls > Pad tuning: `Allow Vibration`, `Vibration` (master, keeps the old `pad_rumble` line), `Vib Player/World/Interaction/GUI` | PORTED. DEVIATES: turning vibration off stops a running motor at once (Java leaves it to the 5 s SDL timeout) |
| Test vibration button | `Test Vibration` row, `fx::test`, ends when the screen changes | PORTED |
| Silent while `isPaused && !isConfigScreen` or in keyboard/mouse mode | `rumble_step`: paused outside the options page, or `Pad::kbm` | DEVIATES: touch counts as "pad" (no input-mode switch for a finger); `outOfFocus` is not needed (the loop stops in the background) |
| `damage/ClientPacketListenerMixin` incoming | `Pad::watch_health` -> `fx::damage_taken` | PORTED (a drop in health; Java fires on the damage packet) |
| outgoing (you hit something) | `fx::damage_dealt` at the three melee sites in `lib.rs` | PORTED for mobs. NOT PORTED: hitting a boat, a minecart, a painting |
| `blockbreak/MultiPlayerGameModeMixin` | `Pad::rumble_dig` from the dig loop | PORTED (hardness curve, instabreak buzzes one tick). Starts when `blockHitWait` ends |
| `explosion/ClientPacketListenerMixin` | `fx::explosion` from `App::explode` | PORTED (distance from the feet, radius = power) |
| `explosion/LightningBoltMixin` | `fx::lightning` at the strike | PORTED (full strength, not distance scaled). UNVERIFIED: Java only sees bolts the client tracks (about 256 blocks); mc-rs only strikes near chunks |
| `itembreak/LocalPlayerMixin` | `fx::item_break` from `inv.broke` | PORTED |
| `waterland/LocalPlayerMixin` | `fx::splash` from the splash sound's impact | PORTED for the walking player. NOT PORTED: a rider's impact (0.9 instead of 0.2) |
| `slowblock/LocalPlayerMixin` | `Pad::rumble_slow`, soul sand only | PORTED. b1.7.3's only slowing block is soul sand (honey does not exist; a web is not a speed factor in Controlify either) |
| `useitem` EAT / DRINK | `fx::eating` while the Cortes Combat meal runs (`App::eating`) | DEVIATES: vanilla b1.7.3 eating is instant, so it only buzzes with the Immersive Pack ON |
| `useitem` BLOCK | `fx::guard` while the Cortes Combat guard is raised | DEVIATES: same, only with the pack ON |
| `useitem` BOW | none | NOT PORTED: b1.7.3's bow has no draw (`BowItem` charge shape is not written) |
| `useitem` SPEAR, TRIDENT, CROSSBOW, SPYGLASS, TOOT_HORN | none | NOT PORTED: not in b1.7.3 |
| `fishing/FishingHookMixin` | `fx::bite` while `Hook::bite()` | DEVIATES: mc-rs's bite is the "reel in now" window, Java's is the whole nibble |
| `levelevents`: anvil (GUI source), ender dragon death, wither spawn | none | NOT PORTED: not in b1.7.3 (so the `Vib GUI` slider has nothing to scale) |
| `MinecraftMixin.disconnect` clears the effects | `Pad::rumble_clear` on entering a world and on quit to title | PORTED |
| Server vibration (`VibrationPacket`, `/vibratecontroller`, `allowServerRumble`) | none | NOT PORTED: there is no server |
| `TriggerRumbleState` / impulse triggers | none | NOT PORTED (cannot): Android has no API for them |
| DualSense HD haptics, adaptive triggers (`TriggerEffectManager`) | none | NOT PORTED (cannot): left out with the HID drivers |
| SDL `SDL_RumbleGamepad(strong, weak, 5000)` | `rumble.rs`: `VibrationEffect.createOneShot(ms, amplitude)` per motor | UNVERIFIED, see below |

### Android side (`rumble.rs`), UNVERIFIED

* A pad that exposes two vibrators gets strong on `getVibratorIds()[0]` and weak on `[1]`. The order is an assumption; swap them in `managed` if a pad feels
  backwards. One vibrator gets `max(strong, weak)`. API < 31 uses `InputDevice.getVibrator` (one motor).
* A level is sent when it changes, and again after 4 s (`Wire`), as a 5 s one-shot. Amplitudes are 1..255; a motor at 0 is cancelled.
* The `jni 0.22` shapes most likely to need a touch when it is built: `call_static_method` with a found class, and the `int[]` of `getVibratorIds`
  (`cast_local`, `len`, `get_region`). `android.permission.VIBRATE` was added to `Cargo.toml` in case the framework wants it.
* A pad without amplitude control runs at its one speed; the weak-only levels (0.05) may not be felt on some pads.

## Reach-around

| Controlify | mc-rs | |
|---|---|---|
| `ReachAroundHandler.canReachAround` / `getReachAroundHitResult` | `reach::reach_around` | PORTED: needs a pick that found nothing, on the ground, not riding, pitch >= 45 degrees; the block under the feet, its side facing the look, the block's centre |
| `getOnPos()` (the supporting block) | `reach::supporting_block` | DEVIATES: the solid cell under the box's footprint nearest to the feet (what `mainSupportingBlockPos` finds), tried in x, z order |
| `MinecraftMixin` replaces the pick for everything | `App` target code | PORTED, so an attack there digs the block you stand on (Controlify's behaviour) |
| Setting `reachAround` (`ReachAroundMode`: off, singleplayer, singleplayer and LAN, everywhere) | `Reach Around` ON/OFF, off by default | DEVIATES: mc-rs is singleplayer, the four values are one switch |
| `ServerPolicies.REACH_AROUND` and its toast | none | NOT PORTED: no server |
| The warning tooltip about unfair advantage | none | NOT PORTED: it is about other people's servers |

## What was checked, and how

* Controlify's own `BasicRumbleEffect`, `ContinuousRumbleEffect`, `RumbleManager`, `RumbleState` were compiled with stubs for Minecraft and run:
  the per-tick outputs of ten scenarios (damage, idle, silent, instabreak, heartbeat timeout, vibration off, test vibration with a screen change,
  lightning, two effects, guard) are the ones `rumble_fx.rs`'s tests assert. The one difference is the deliberate "vibration off" stop above.
* The explosion, splash and lightning numbers were read against the mixins' sources again.
* The Rust itself (including the tests in `rumble_fx.rs`, `reach.rs` and `gamepad.rs`) was **not compiled or run**: there is no Rust toolchain
  where this was written. Braces of every edited file balance; types were matched by hand.
