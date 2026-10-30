# What is still missing from b1.7.3 (audit of 2026-10-25, rechecked 2026-10-30)

Part of the plan in [`README.md`](../README.md) (the root `ROADMAP.md` with the milestone tables was not in `Repo.zip`, so this file was made from the code).

How it was made: the decompiled `minecraft/src` (675 classes: 84 blocks, 103 items, 58 entities, 45 Gui, the recipe classes, `EntityList`, `BiomeGenHell`) was compared with `mc-rs/src` by reading and grepping. Nothing was compiled or run. A gap here means "no counterpart found in the Rust"; check the code before starting one. Ordered easiest first. Updated after entry (19), against `Repo_1.zip`; rechecked 2026-10-30 against the code with grep and by reading `ItemDye`, `use_held`, `ticks.rs`, `render/items.rs`, `render/anim.rs` (still not compiled, and not a class-by-class comparison); the ghast / fireball / explosion parts after entry (34) (that pass was compiled and tested, see `gameplay.md`).

## Still missing (vanilla b1.7.3)

| # | What | Java | State in mc-rs | Size |
|---|------|------|----------------|------|
| 3 | ~~Map item~~ | `ItemMap`, `MapData`, `MapItemRenderer`, `MapStorage`, recipe 358 = paper ring round a compass | **done 2026-10-30 (gameplay entry 36)**, not compiled; loaded chunks only, flat HUD sheet in hand | done |
| 8 | Bone meal on blocks | `ItemDye.onItemUse` (damage 15): sapling -> `BlockSapling.growTree`, crops -> `BlockCrops.fertilize` (metadata 7), grass -> spreads tall grass and flowers (`ItemDye` lines 22 on) | only the recipe (bone -> bone meal) and dyeing sheep (`gear::use_on_mob`); `lib.rs` `use_held` and the block-use path have no branch for it. `ChunkManager::grow_tree` (used by the sapling tick) exists to call | small (found 2026-10-30; not in any earlier audit) |
| 4 | Chat | `GuiChat`, `ChatLine` | nothing (a single-player port has no sender except commands; the overlay is the deferred rest of M14) | medium |
| 5 | Achievements and Statistics | `AchievementList`, `GuiAchievements`, `GuiAchievement` (toast), `StatList`, `GuiStats` | both buttons drawn greyed out (`menu.rs`, `options.rs`); no stat system | large (counters everywhere, saved, two screens) |
| 6 | Mods and Texture Packs | `TexturePackList`, `GuiTexturePacks` | button drawn greyed out | large, and arguably off-goal |
| 7 | Multiplayer (M10) | `Packet*` (59 classes), `GuiMultiplayer`, `EntityOtherPlayerMP` | nothing, out of scope on purpose (README) | out of scope |

## Smaller gaps already written down in the code or the changelogs

Sound: `random.drr` (in `audio/table.rs`, no call site found), the 8 smoke puffs of water poured in the Nether (no trace in the code). Sleeping: done in entry (35) (`performSleepSpawning`, `tile.bed.notValid`); the "too far" and "monsters near" messages do not exist in b1.7.3 (checked in `EntityPlayer.sleepInBedAt` / `BlockBed`), so they are not a gap. Still open there: the Nether's exploding bed and the lying pose. These come from the README rows and `roadmap/gameplay.md`; the sound, smoke and bed items were only grepped, not read.

Stale notes removed on 2026-10-30: the mob spawner's spinning mob is drawn (`render/items.rs`, `TileEntityMobSpawnerRenderer`), and the portal texture is animated (`render/anim.rs`, `PORTAL`). The README row of M6b that says "no fire, mushroom spread, snow/ice, rain on farmland" is also out of date: `world/ticks.rs` has `fire`, `mushroom`, `snow_ice` and `rained_on`.

## Known gaps left by entries (33) and (34) (explosions, fireballs, corpses)

Found while porting them, not an audit of the rest of the entity code:
- A falling sand or gravel block is `canBeCollidedWith` in the Java (`EntityFallingSand`) and does not stop a fireball here; a lit TNT does (`Mobs::targets` only takes `Falling` id 46).
- An explosion does not break a painting inside its radius (`attackEntityFrom` on it); it only goes when its wall does. A fireball's direct hit does break it.
- An explosion pushes mobs, boats, carts and the player only: not dropped items, a lit TNT, falling blocks or arrows (`motion +=` in `doExplosionA` reaches every entity).
- A creeper's blast on the player is not scaled by the difficulty like the other mob hits (`EntityMob` source); a TNT or fireball one is not in the Java either.
- A corpse (`Mob::death` 1..=20) is not picked by a tap, passed by an arrow and not hit by a fireball; the Java keeps it `canBeCollidedWith` for the 20 ticks. A fireball hit on another fireball does nothing if the ghast that fired the first is gone from the list (the Java uses its last look).
- Soul sand slows the move of a box (`physics::contact`) instead of keeping the motion x0.4 a tick.
- Slime: the Java splits only at `health == 0` exactly; here any `health <= 0` splits.

## Checked and NOT missing

Charcoal torch (`code()` ignores coal damage). All 58 `CraftingManager` recipes except compass / clock / map (now in, see below), the 16 dye recipes and the 8 smelting entries. Pathfinding (`world/path.rs`), wolf taming (but wild wolves never spawned: b1.7.3 puts them in the forest and the taiga; they do since entry 20), saddles, thrown snowballs and eggs, dungeon spawners, beds and sleeping, redstone (wire, torch, repeater, lever, button, plates, dispenser, note block, jukebox, pistons), rails (all three), minecarts (empty and furnace), boats, paintings, signs, trapdoor, cake, portal + Nether terrain.
`ChunkProviderSky` (Skylands) exists in the Java but no dimension or world type reaches it in b1.7.3, so it is not counted.

## Done in these passes

Nether light table: `WorldProviderHell.generateLightBrightnessTable` keeps the curve and lifts the floor from 0.05 to 0.1. `world::chunk::{AMBIENT, NETHER_AMBIENT, brightness_in}` is the CPU table; the shaders' single `brightness` (WGSL in `gpu/pipeline.rs`, GLSL in `gles/shaders.rs`) reads the lift from `u.fx.w`, which `Look::new` sets to 0.05 in the Nether and 0 elsewhere (both renderers already take `fx` from the `Look`). Terrain, drops and mobs all take their light through that one function. Written without a toolchain. Left alone: the mesher's baked `light` (`render/mesh.rs`, `table`) still uses 0.05, but the shaders do not read it for lit blocks (only water's depth packing does, and there is no water in the Nether), and the overworld-only users of `brightness` (zombie burning, rain streaks, LoD water tint).

Portal texture: `TexturePortalFX` in `render/anim.rs` (`portal_frames`: 32 frames from `Random(100)` and `MathHelper.sin`, shown by `tick & 31` over atlas tile 14 with its mips, like the water and lava tiles). Written without a toolchain; the 32 frames were also worked out in a Python port to check the value ranges (all four channels stay under 255, so the Java's byte casts never wrap).

Mob spawner cage: `Spawner::{yaw, yaw2, spin}` (`TileEntityMobSpawner.updateEntity`: the cage turns `1000 / (delay + 200)` a tick while the player is within 16 blocks) and `spawner_cage` in `render/items.rs` (`TileEntityMobSpawnerRenderer`: the mob at 7/16 scale, tilted -30 degrees about X, turned `spin x 10` degrees about Y, pivot 0.4 above the floor). Written without a toolchain and not compiled; two host tests added.

Death, the player's blast, fliers and fireball hits (entry 34): `Mob::death` (`deathTime`, the corpse topples under the red overlay for 20 ticks, loot at once, slime splits at the end); the blast works out boats, carts and the player before the cells go, the player from its eye with exposure, damage and push (`App::kick`); `step_flying` meets web, soul sand and a stream's current (`physics::contact`); a fireball stops on a painting, a lit TNT, a boat, a cart and another fireball, with the boat's or cart's own `hurt(0)` wobble, the painting breaking and the hit fireball turned along its ghast's look.

Ghast and fireball (entries 31, 32): `Kind::Ghast`, its AI and spawns, `Fireball` with the fiery explosion and the punch back, `ModelGhast` with the charge squash, both skins, the sounds and the smoke.

Nether spawns (entry 30): `Mobs::nether`, pig zombies only, no light test.

Minecart with chest, the 27 slots (entry 29): tap with an empty hand opens a `Minecart` screen, a broken cart spills them.

Compass (345), clock (347) and minecart-with-chest (342) recipes; the chest cart can be placed, is drawn and breaks into the cart and a chest (entry 13). The furnace cart recipe is shaped like the Java (furnace over minecart); the compass and clock already animated (`render/anim.rs`, an earlier version of this file said they did not) and now spin in the Nether like `TextureCompassFX` / `TextureWatchFX`; Rename world works (`GuiRenameWorld`, name only, the folder keeps its name) (entry 14). (20) Natural wolves in the forest and taiga: the audit had missed that no wild wolf ever spawned. All written without a toolchain.

## Not b1.7.3 (do not count as fidelity)

Apples from leaves, the six Blooming biome identities (entry 20), immersive pack, satchel and recipe book, shoulder-surf view, smart animals, stamina, sprint, the atmosphere editor, the Photon / look shaders, Structory, Tectonic and the river port. The rules for adding more of these are in [`design.md`](design.md).
