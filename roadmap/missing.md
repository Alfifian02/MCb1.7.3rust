# What is still missing from b1.7.3 (audit of 2026-10-25)

Part of the plan in [`README.md`](../README.md) (the root `ROADMAP.md` with the milestone tables was not in `Repo.zip`, so this file was made from the code).

How it was made: the decompiled `minecraft/src` (675 classes: 84 blocks, 103 items, 58 entities, 45 Gui, the recipe classes, `EntityList`, `BiomeGenHell`) was compared with `mc-rs/src` by reading and grepping. Nothing was compiled or run. A gap here means "no counterpart found in the Rust"; check the code before starting one. Ordered easiest first. Updated after entry (19), against `Repo_1.zip`.

## Still missing (vanilla b1.7.3)

| # | What | Java | State in mc-rs | Size |
|---|------|------|----------------|------|
| 1 | Minecart with chest: the 27 slots | `EntityMinecart` type 1 (`IInventory`) | `Cart` kind 1 is placed, drawn and dropped, but tapping it does nothing and a broken one loses its contents | medium (a chest screen over a cart, not a tile) |
| 2 | Ghast, fireball, Nether spawns | `EntityGhast`, `EntityFireball`, `BiomeGenHell.spawnableMonsterList` (Ghast 10, PigZombie 10) | `mobs::Kind` has 13 kinds, no Ghast; `Mobs::no_spawn` stops every spawn in the Nether, so the pig zombie (which exists) never appears there; sounds for both are already in the audio table | medium |
| 3 | Map item | `ItemMap`, `MapData`, `MapItemRenderer`, `MapStorage`, recipe 358 = paper ring round a compass | nothing | medium to large (per-map saved data, a renderer) |
| 4 | Chat | `GuiChat`, `ChatLine` | nothing (a single-player port has no sender except commands; the overlay is the deferred rest of M14) | medium |
| 5 | Achievements and Statistics | `AchievementList`, `GuiAchievements`, `GuiAchievement` (toast), `StatList`, `GuiStats` | both buttons drawn greyed out (`menu.rs`, `options.rs`); no stat system | large (counters everywhere, saved, two screens) |
| 6 | Mods and Texture Packs | `TexturePackList`, `GuiTexturePacks` | button drawn greyed out | large, and arguably off-goal |
| 7 | Multiplayer (M10) | `Packet*` (59 classes), `GuiMultiplayer`, `EntityOtherPlayerMP` | nothing, out of scope on purpose (README) | out of scope |

## Smaller gaps already written down in the code or the changelogs

Mob spawner: the spinning mob cage is not drawn. Sound: mining hit sound, wolf shake, fishing, item and arrow pops. Nether: animated portal texture, Nether light table. Sleeping: only the "You can only sleep at night" message exists (no too-far / monsters-near messages, no `performSleepSpawning`). These come from the README rows and `roadmap/gameplay.md`; they were not re-checked against the code.

## Checked and NOT missing

Charcoal torch (`code()` ignores coal damage). All 58 `CraftingManager` recipes except compass / clock / map (now in, see below), the 16 dye recipes and the 8 smelting entries. Pathfinding (`world/path.rs`), wolf taming (but wild wolves never spawned: b1.7.3 puts them in the forest and the taiga; they do since entry 20), saddles, thrown snowballs and eggs, dungeon spawners, beds and sleeping, redstone (wire, torch, repeater, lever, button, plates, dispenser, note block, jukebox, pistons), rails (all three), minecarts (empty and furnace), boats, paintings, signs, trapdoor, cake, portal + Nether terrain.
`ChunkProviderSky` (Skylands) exists in the Java but no dimension or world type reaches it in b1.7.3, so it is not counted.

## Done in these passes

Compass (345), clock (347) and minecart-with-chest (342) recipes; the chest cart can be placed, is drawn and breaks into the cart and a chest (entry 13). The furnace cart recipe is shaped like the Java (furnace over minecart); the compass and clock already animated (`render/anim.rs`, an earlier version of this file said they did not) and now spin in the Nether like `TextureCompassFX` / `TextureWatchFX`; Rename world works (`GuiRenameWorld`, name only, the folder keeps its name) (entry 14). (20) Natural wolves in the forest and taiga: the audit had missed that no wild wolf ever spawned. All written without a toolchain.

## Not b1.7.3 (do not count as fidelity)

Apples from leaves, the six Blooming biome identities (entry 20), immersive pack, satchel and recipe book, shoulder-surf view, smart animals, stamina, sprint, the atmosphere editor, the Photon / look shaders, Structory, Tectonic and the river port. The rules for adding more of these are in [`design.md`](design.md).
