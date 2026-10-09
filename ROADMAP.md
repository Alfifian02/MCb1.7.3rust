# Roadmap: Minecraft b1.7.3 -> Android in Rust

Fresh start. One milestone per session. Vanilla names. Vulkan-only.
Own simple save format (M7 dropped McRegion). Client-only networking.

## Milestones
| #   | Name | Deliverable |
|-----|------|-------------|
| M0  | Skeleton | Buildable APK, solid color on screen, lifecycle correct |
| M1  | Blocks + chunks | Hand-built 16x16x128 stone chunk visible |
| M2  | Camera + physics | First-person walk, jump, AABB collision |
| M3  | Overworld gen | Beta-1.7 noise: grass/dirt/stone, sea level 64, ores |
| M4  | Biomes + trees + caves | All b1.7.3 biomes + worldgen populate step |
| M5  | Block interaction | Raycast pick, break, place, light update (done, not compiled: see Changelog) |
| M6  | Inventory + crafting | Survival inv, hotbar, crafting grid, recipes (done, not run on a device: see Changelog) |
| M7  | Save | Own format (not McRegion, by decision): per-chunk RLE files + `level`; autosave, save on pause/exit, resume (written, not compiled: see Changelog) |
| M8  | Entities + AI | Mobs + item entities |
| M9  | Audio | Positional OGG, music stubs |
| M10 | Multiplayer | Full b1.7.3 client protocol |
| M11 | Nether | Hell biomes, portals, ghast, zombie pigman |
| M12 | Touch UX | Landscape; move stick, look drag, jump button, hotbar tap, pause menu |
| M13 | Optimization | Frustum culling, greedy meshing, profiler |
| M14 | Polish | Splash, main menu, settings, lang |

## Build
- Workspace at `mc-rs/`
- `cargo test --lib` runs the unit tests: 47 before block metadata, 52 with it, 56 with swimming, health, food and frustum culling (Android host such as Termux; see README).
- `cargo run` boots a desktop window (Metal / Vulkan / GL depending on host).
- APK via GitHub Actions `cargo apk build --release` -> `MinecraftB173Rust.apk`.
- A long-lived debug keystore at `keystore/debug.keystore` signs the release
  APK in CI. The same keystore is what `cargo apk build --release` expects at
  `~/.android/debug.keystore` with password `android` if you build locally.

## Source-of-truth rule

Anything that cannot be diffed against the b1.7.3 Java sources or the
decompiled `minecraft/` tree is marked `// UNVERIFIED:` in the code. PRs
that fix UNVERIFIED items should drop the marker and link the source line
they matched.

## Next step

M8 (entities + AI). M7 (save) is written but not compiled: run `cargo test --lib` first. Furnaces smelt (see the furnace entry), so the whole tool chain up to diamond is reachable. M6 is written and the crate type-checks and tests on a host, but nothing has run on a device (M5 included). M4 is done: caves and populate match the real classes bit for
bit (`tools/golden/`). Still approximate in populate (see README): light model, no metadata, no tile entities, no block
ticks (liquids/sand). Sapling growth, fluid flow and falling sand belong with M5's block updates. Ice Desert exists in
BiomeGenBase but climate never selects it; b1.7.3 has no ravines.

## Changelog
- `done` M7 fix: autosave no longer interrupts play every 5 s (not compiled, not run on a device). `App::save` called `close_screen()`
  unconditionally, and `close_screen` also runs `touch.set_screen(false)`, so each autosave (screen closed) reset the touch state and cut
  the held move stick / look drag / dig. Now `save` closes a screen only when one is open (`lib.rs`, one `if`). Pause and exit saves are
  unchanged (they still turn an open screen into dropped items first).
    - UNVERIFIED: `input/touch_ui.rs` was not in the zip, so that `set_screen(false)` resets the pointer state is inferred from the call
      site, not read. If actions still drop, look there first.
    - Still open: autosave writes the level + up to 24 chunks on the render thread; on a slow phone that can hitch a frame. If so, lower
      `AUTOSAVE_CHUNKS` or move the writes to a thread.
- `done` M7 save (written WITHOUT a Rust toolchain: not compiled, tests not run, expect a compile fix or two; +2 tests). Decision: it does
  not follow b1.7.3 (no McRegion, NBT, zlib): the aim is fewer lines and a smaller, faster save, not a loadable vanilla world.
    - `world/save.rs` (new): `encode_chunk`/`decode_chunk` = populated flag + run-length-coded block ids + metadata nibbles (runs of
      1..=255), `Level` encode/decode (time, position, spawn, look, hotbar slot, health/air/fire, 36 slots, furnaces, dropped items),
      `write` = temp file + rename, so a kill mid-write keeps the old file. Decoding checks every length, stack size and float and
      answers `None` for anything off (chunk: generated again; level: new game); it never panics.
    - Only chunks that differ from the seed are written: `Entry.dirty` is set by an edit and by `populate` (on all 4 chunks it writes
      into), cleared by a write. Untouched chunks are regenerated, light and height map are recomputed on load. `ChunkManager::with_dir`
      scans the folder once into `saved`; `stream` reads saved chunks (8 per frame) instead of asking a worker; `flush(budget)` writes
      dirty ones; unloading writes a dirty chunk first. `preload(cx, cz)` now takes the centre chunk (it was `lo, hi`).
    - `lib.rs`: the world resumes where the player stood (`Level` -> `App::restore`); a new world still searches a spawn (`find_spawn`).
      Autosave every 5 s of play (level + 24 chunks, skipped with a screen open), and a full save on `Pause` and `TerminateWindow`,
      which matters: `TerminateWindow` drops the whole `App`, so before this a trip to the home screen lost the world. An open screen is
      closed first, so its cursor and grid become dropped items. Folder: `internal_data_path()/world-<seed hex>/` (`c.<cx>.<cz>`, `level`).
      To start a new world, clear the app's data (no menu until M14).
    - This also ends the old ponytail note in `stream` (an unloaded chunk was populated again on return).
    - Not saved: pause state, mining progress, velocity of dropped items (they come back at rest), the sky/light, anything a menu would
      choose. Furnace facing is still lost when a furnace lights (`set_block` clears metadata).
    - Known ceiling: a hard kill (not a normal pause/exit) can lose up to 5 s of play, and can leave a chunk border with a tree cut off
      if a populated chunk was written before its neighbour. Chunk writes and reads run on the render thread (a few small files per frame).
    - Tests: `chunk_and_level_round_trip` (codec, run splitting, every corrupt case), `edits_survive_a_restart` (flush, new manager loads
      it, corrupt file forgotten).
- `done` M13 frustum culling (host-checked: type-checks, 48 tests pass; NOT run on a device, so the FPS gain is unmeasured):
    - `render/camera.rs`: `Frustum::from_view_proj(proj * view)` takes the six planes (Gribb-Hartmann, depth 0..1 so the near plane is row 2),
      `intersects_aabb` is the usual conservative test against the corner furthest along each plane normal. Test covers ahead, behind,
      sides, above/below, past the far plane, the camera inside a box, the wide 2:1 shape and a 90 degree turn.
    - `world/chunks.rs`: `meshes_where(visible)` yields the meshes whose box (16 x `H` x 16 at the chunk key) passes the test. `lib.rs` draws
      only those. Only the terrain draw is culled: dropped items, the outline and the HUD are tiny.
    - Not done on purpose: tightening the box to each chunk's real height (it is the full 128), occlusion culling, greedy meshing, a profiler.
      Culling also trims only draw calls and GPU vertex work; chunk meshing and generation still run for the whole ring.
- `done` sword, hoe, shears (host-checked like M6: crate type-checks, 47 tests pass, NOT run on a device):
    - `craft.rs`: swords (ids 267/268/272/276/283, 1.5x on everything, 15x and harvest on web, 2 wear per block), hoes (290..294, no wear
      from digging, 1 per tilling), shears (359, 238 uses: 15x and harvest on web, 15x on leaves, 5x on wool, wear only on leaves and web).
      Recipes `RecipesWeapons` x 5, hoe x 5 from `RecipesTools`, shears from two iron ingots. Wear is `wear_on_break(held, block)`.
    - `lib.rs`: a hoe tills dirt, or grass with air above and not from below, into farmland (60) and wears 1 (`ItemHoe.onItemUse`);
      shears on leaves drop the leaves block itself (`BlockLeaves.harvestBlock`) via `Drops::spawn_stack`.
    - Not done: farmland reverting to dirt, its 15/16 height, crops (so a hoe only makes farmland), sheep, bow and arrow.
- `done` mushroom stew + eating (`world/items.rs`, `world/craft.rs`, `world/vitals.rs`, `lib.rs`; written WITHOUT a Rust toolchain: not compiled, tests not run, +1 test and extra asserts):
    `ItemFood.onItemRightClick` / `ItemSoup`: a tap that neither places a block nor opens a workbench/furnace uses the held item (as
    `Minecraft.clickMouse` -> `sendUseItem` does, aimed at a block or not). Food uses up one and `Vitals::heal`s (`EntityLiving.heal`: nothing when dead,
    capped at 20, damage window back to 10); eating at full health still uses it up. Stew (282) heals 10, stacks to 1 and leaves an empty bowl (281).
    Recipes: bowl (3 planks in a V, x4) and stew (red + brown mushroom + bowl, either order). Only the stew is in `heal_amount`; apple, bread, pork, golden
    apple and fish are one line each (ids in the comment) once mobs, crops or chests exist. Not done: eating animation and sound, the hold-to-eat delay
    (b1.7.3 has none: eating is instant).
- `done` health, damage, death (`world/vitals.rs`, `lib.rs`; written WITHOUT a Rust toolchain: not compiled, test not run, +1 test):
    `Vitals` = `EntityLiving.attackEntityFrom` (20 health, the 10-tick damage window: an equal or smaller hit inside it is ignored, a bigger one
    pays the difference), `Entity.updateFallState` + `EntityLiving.fall` (`ceil(fall - 3)`, water cancels it), drowning (300 air, then 2 damage every
    20 ticks once it runs out, `isInsideOfMaterial`), lava (4 damage per window + 600 ticks of fire, 1 damage per second while burning, water puts
    it out) and the void (eye below -64). Frame-based fall check, everything else on the 20 Hz tick. `physics::step` now returns (water, lava).
    HUD: 10 hearts above the hotbar, air bubbles while the eye is under water. Death (`App::die`): the open screen and the whole inventory drop where
    he died, the pause menu turns red and its resume button respawns at the first spawn point with full health and a clean `Vitals`.
    b1.7.3 has no hunger and no natural regeneration (only Peaceful heals), so health stays down until food is ported. Not done: suffocation in
    blocks, hurt flash/blink, knockback, burning and hurt sounds, fire overlay, death camera roll, items burning in lava, per-difficulty damage,
    bed spawn, health in the save (M7).
- `done` fluids are not solid + swimming (`world/physics.rs`; written WITHOUT a Rust toolchain: not compiled, test not run, +1 test):
    the player used to stand on water and lava because physics treated every id > 0 as solid. Ids 8..=11 are now passable; `step` takes the
    held jump flag and ports `Entity.handleWaterMovement`/`handleLavaMovement` (box shrunk 0.4 top and bottom) and
    `EntityLiving.moveEntityWithHeading`: no gravity in a fluid, vertical drag 0.8 (lava 0.5) and 0.02 blocks/tick sink, held jump +0.04/tick,
    wall hop 0.3/tick when 0.6 higher is free. Simplified: every fluid cell is a source block (flow levels need metadata), horizontal speed is
    scaled to the terminal swim speed (2.0 m/s water, 0.8 lava) instead of accumulating. Not done: no flow push, no splash/bubbles, no breath
    or lava damage (next: health), dropped items still treat fluids as solid.
- `done` block metadata (written WITHOUT a Rust toolchain: not compiled, tests not run; the Java side was run). Foundation for wood
  and leaf species, wool colours, slabs, stairs, doors and the bed, so the chunk format does not change again when they arrive:
    - `world/chunk.rs`: `Nibbles` = `NibbleArray` (4 bits per cell, even cell index = low nibble, same index as the blocks), 16 KB
      per chunk, bytes identical to the McRegion `Data` tag (M7 writes `bytes()` as is). `world/chunks.rs`: `Entry.data`,
      `ChunkManager::meta`, `set_block_meta` (= `Chunk.setBlockIDWithMetadata`) and `set_block` (= `setBlockID`: nothing on the same
      id, metadata cleared when the id changes). `render/mesh.rs` reads only its own chunk's metadata.
    - `world/gen/populate.rs`: `Region` carries the 2x2 metadata; `set` clears it like `setBlock`, `set_meta` writes it like
      `setBlockAndMetadata`. Written where the Java writes: birch leaves/logs 2 (`WorldGenForest`), spruce 1 (`WorldGenTaiga1/2`), tall
      grass 1 or 2 (rainforest, `WorldGenTallGrass`), pumpkin facing 0..3. Oak and big trees stay 0. No Random draw changed.
    - Golden: `tools/golden/G.java`'s fake `World` keeps a `NibbleArray` per chunk with the Java setter semantics and prints a `META` line
      (FNV of each chunk's packed nibbles) after every `POP` line. Regenerated: every old line is byte for byte unchanged; 6 of the 11
      cases have non-zero metadata. `populate_matches_java` compares the Rust bytes with those hashes.
    - `world/items.rs`: `damage_dropped(block, meta)` (`Block.damageDropped`), `spawn_block(block, meta, pos)` (lib.rs reads the
      metadata before it clears the cell), `placed_meta` (`Item.getPlacedBlockMetadata`; leaves get bit 8), `stack_tile`; double
      slab drops 2 single slabs (`BlockStep`). `lib.rs` places with `set_block_meta`.
    - `render/atlas.rs`: the atlas is 16x32 (`TILES_W`/`TILES_H`, `gpu/pipeline.rs` follows); tiles 256.. are variants picked by
      `tile_of(id, meta)`: spruce/birch logs and leaves, the 15 coloured wools (`EntitySheep.fleeceColorTable`). Hotbar, inventory and
      dropped items use `stack_tile`, so a red wool stack is red. Log and leaf colours are UNVERIFIED stand-ins until M14 textures.
    - Not done: slab/stair/door/bed shapes, rotation on placement (no `onBlockPlacedBy`: furnace and pumpkin facing is stored by worldgen
      only), leaf decay and sapling growth, saving the data (M7), blocks the player cannot make yet (no wool dye or slab recipes).
      Note for M7: `ChunkManager::set_block` clears metadata, so the furnace lit/unlit flip loses the facing once furnaces have one.
- `done` touch spread (fix from a device report: with only tap = left click, a stack could not be split across a grid):
  a picked-up stack floats above the finger (`TouchUi::cursor_pos`), and dragging it over other slots puts one item into each
  slot it enters, the start slot included (`Screen::drop_one`: empty or same item with room, never a swap, never an output
  slot). A press that lifts where it began is still a tap, and the "1" toggle stays. The yellow cursor box is gone. The
  gesture state machine is in `lib.rs` (`on_screen_event`) and only `drop_one` is unit-tested; 47 tests pass, not run on a device.
- `done` furnace + smelting (same host checks as M6: crate type-checks, 46 tests pass, NOT run on a device):
    - `world/craft.rs`: `Furnace` = `TileEntityFurnace.updateEntity` (fuel used up when the fire starts, 200 ticks per item, output stacks
      to 64, relights from the next fuel without a flip), `FurnaceRecipes` (iron/gold/diamond ore, sand -> glass, cobble -> stone, clay -> brick,
      cactus -> green dye, log -> charcoal) and `getItemBurnTime` (wood blocks 300, stick/sapling 100, coal 1600). `ContainerFurnace` slots and the
      `SlotFurnace` rule: the output takes nothing, right click takes half.
    - `lib.rs`: furnaces live in a map by block position, created on first use, ticked at 20 Hz only while their chunk is loaded; the block swaps
      61 <-> 62 (lit, light 13) when the fire flips. Using a furnace opens its screen (flame and arrow bars). Breaking one spills its three slots.
    - Atlas colours for the new blocks (glass, wool, metal blocks, workbench, furnaces, snow block, glowstone); iron/gold/diamond tools had none.
    - Not done: the furnace contents are not saved (M7), furnace facing (no metadata), fuel/lava bucket, raw pork and fish.
- `done` M6 crafting + tools (written on a Linux host: the whole crate type-checks and its 45 tests pass against a stubbed
  `android-activity`; NOT run on a device, so the touch layout of the new screen is untested by hand):
    - `world/craft.rs` (new): `EnumToolMaterial` (wood/stone/iron/diamond/gold), pickaxe/axe/shovel ids 256..=286, `getStrVsBlock`,
      `ItemPickaxe.canHarvestBlock` (obsidian level 3, diamond/gold/redstone level 2, iron/lapis level 1, else rock/iron),
      `ItemSpade.canHarvestBlock` (snow). 26 shaped recipes (`RecipesTools` loop for 5 materials x 3 tools, planks, sticks,
      workbench, chest, furnace, torch, sandstone, snow/clay blocks, glowstone, wool); the matcher is `ShapedRecipes.matches`.
    - `Screen`: `ContainerPlayer` (2x2) and `ContainerWorkbench` (3x3). Touch has no mouse, so a tap is a left click and a toggle
      button ("1") turns taps into right clicks (put one, take half). Result slot as in `SlotCrafting`: refuses input, a take uses up
      one of each ingredient. Closing (or tapping outside the panel with the cursor full) throws the cursor stack and the grid out
      along the look direction (`dropPlayerItem`, 40 tick pickup delay).
    - `items.rs`: `Inventory` is 36 slots (`mainInventory`; pickups already fill the hotbar first), `ItemStack.damage` is `u16` (tool
      wear), tools stack to 1, `Inventory::damage` = `ItemStack.damageItem` (breaks past maxUses), `Drops::throw`.
    - `dig.rs`: `strength` and `Dig::tick` take the held item: `canHarvestBlock` false = the 1/100 rate, else tool efficiency / hardness / 30,
      with the /5 for water and air. `can_harvest` = material half (`harvestable_by_hand`) or the held tool. `lib.rs` breaks the block,
      wears the tool, and spawns the drops only if `can_harvest`: **stone by hand now drops nothing**, like the original.
    - Touch: inventory button left of pause; using a workbench (tap) opens the 3x3 screen instead of placing. While a screen is open the move
      stick, look, jump, hotbar and crosshair are hidden and every press is a tap. `hud.rs` quad capacity 512 -> 2048 for the screen.
    - Not done: smelting (so no iron/gold ingots), hoes, swords, shears (web and leaves still need them), armor slots, shift-click,
      item sprites (tools are a handle plus a head shape in the material colour), the other ~100 `CraftingManager` recipes.
    - The `ponytail` note in the drops entry below ("bare hands harvest everything") is superseded by this entry.
- `done` drops + hotbar inventory (`world/items.rs`, `render/items.rs`; the logic is unit-tested on a Linux host in a scratch crate,
  `lib.rs`/wgpu NOT compiled here: expect a compile fix or two):
    - Drops port `Block.dropBlockAsItem`: `quantityDropped`, then per item one `nextFloat` (chance 1.0) and `idDropped`, then the 3
      position draws, in the Java order on one `JavaRandom`. Table covers every block worldgen makes plus the metadata-free extras
      (stone -> cobblestone, grass/farmland -> dirt, gravel -> flint 1/10, coal/diamond/lapis(4-8 dye:4)/redstone(4-5) ores, leaves ->
      sapling 1/20, tall grass -> seeds 1/8, web, clay x4, snow block x4, glowstone dust 2-4, reeds, furnace, signs, redstone torch;
      none for fluids, ice, glass, bookshelf, TNT, spawner, snow layer, dead bush). Not yet: doors, bed, crops, slabs, stairs (need metadata).
    - `EntityItem`: 0.25 box, 20 Hz, gravity 0.04, drag 0.98, ground friction 0.588 (ice 0.9604), 10 tick pickup delay, 6000 tick life, pickup when
      the player's box grown 1.0 in x/z touches it. Collision tests the box centre line (ponytail, see items.rs). Max 256 entities.
    - `Inventory`: 9 hotbar slots, `addItemStackToInventory` (top up a matching stack, else first empty, partial adds), max stack 64 (16 snowball, 1 sign/door/bed).
      Starts empty; a tap places the selected item if it is a block (id < 256) and uses one up. Full hotbar leaves the item on the ground.
    - Bare hands harvest everything for now (vanilla `canHarvestBlock` would drop nothing from stone without a pickaxe; no tools/crafting yet).
    - Open: placing plants has no `canBlockStay` check; items are drawn as cubes, full brightness, no spin; no 27-slot inventory screen (M6).
- `done` hardness-based digging (not compiled): `world/dig.rs` ports `Block.blockStrength` and the `PlayerControllerSP` damage
  counter at 20 Hz (hardness table from Block.java; bare hand: rock/iron/snow/web dig 3.3x slower; 5x slower when airborne or
  head under water; instant for hardness 0; 5-tick wait after a break; bedrock/portal unbreakable). Touch: hold still on the
  right half for 0.4 s to start digging, keep holding to finish (aiming elsewhere restarts the damage); progress bar under the
  crosshair (vanilla's crack textures need M14). Tap still places. No tools or drops yet (inventory).
- `done` faster world loading (not compiled): chunk map uses a cheap integer hasher instead of SipHash; light work is bounded by
  10 ms per frame instead of a cell count and lights up to 2 chunks per frame; seam recomputation skips cells above the tallest
  column and block light when neither chunk has any; `App::init` (off the render thread) lights and meshes the spawn area
  (>= 24 chunk meshes or 8 s) before the first frame. Still open: populate and meshing run on the render thread.
- `done` plants drawn (written WITHOUT a Rust toolchain: not compiled, tests not run): flowers, mushrooms, tall grass, dead bush
  and reeds are two crossed double-sided quads (`chunk::cross_shape`, sized from each block's bounds; flat colours until M14
  textures), lit by their own cell, and pickable/breakable as a whole cell. Still not solid. Snow layer (78) is still not drawn.
- `done` day/night cycle (written WITHOUT a Rust toolchain: not compiled, tests not run):
    - `world/sky.rs`: ports of `calculateCelestialAngle`, `calculateSkylightSubtracted`, the sky colour (`func_4079_a` +
      `getSkyColorByTemp`, AWT HSB maths). 24000 ticks per day at 20 ticks/s, new world starts at tick 0 (sunrise). 2 tests.
    - `ChunkManager::set_sky_sub` (0..=11) marks every mesh stale when it changes; `mesh::build` takes `sky_sub` and uses
      `max(sky - sky_sub, block)` per cell, so night is dark but lava/torches stay lit. `temperature_at` feeds the sky colour.
    - Not done: sun, moon and stars are not drawn, no fog, no rain/thunder terms, time is not saved (M7) and not adjustable.
- `done` M5 block interaction (written WITHOUT a Rust toolchain: not compiled, tests not run, expect a compile fix or two):
    - `world/pick.rs`: port of `World.func_28105_a` + `Block.collisionRayTrace` (f64, 4.0 reach), place cell/replaceable/player-overlap rules. 3 tests.
    - `world/chunks/light.rs`: port of the b1.7.3 light engine (sky + block light, region queue, `MetadataChunkBlock` relaxation,
      `Chunk.func_1003_g` relight, `generateSkylightMap`). Chunks light once final; edits go through `ChunkManager::set_block`. 1 test.
    - `chunk.rs`: `light_opacity`/`light_value` tables derived from Block.java, `brightness`, `height_map`. `mesh.rs`: faces use the
      neighbour cell's brightness x vanilla face shade (1.0/0.5/0.8/0.6); `push_box`. `render/outline.rs`: selection outline.
    - Touch: tap on the right half places, hold breaks (repeat 0.25 s), drag looks. Crosshair added. Hotbar slot = block placed.
    - Simplifications: instant break, no drops/consumption (M6), no day/night (`skylightSubtracted` = 0), plants/liquids not pickable,
      light seams across a not-yet-final neighbour are fixed by the seam strips when it is lit.
- `done` M4d caves, trees, populate (compiled and tested on a Linux host with rustc 1.85; not run on a device):
    - `gen/caves.rs`, `gen/populate.rs` (new), `OverworldGenerator::generate` now ends with the caves; `populate_ores`
      and `next_u31` deleted. `mobSpawnerNoise` is the 8th noise stack (after the 7 terrain ones).
    - `world/chunks.rs`: raw ring is now `RENDER_DIST + 2`, populate (1 chunk/frame, nearest first) needs the 2x2 raw
      chunks, a chunk is meshed once populate ran on it and its -X/-Z/-X-Z neighbours (`is_final`); a populate that
      touches meshed chunks marks them for a rebuild. Spawn area is `preload(-1, 2)`.
    - Golden: `G.java` has a fake `World` (2x2 chunks, column light model, no-op springs) and prints RAW/POP hashes plus a
      block table; 3 seeds, 11 cases (taiga+snow, forest, lava lake, dungeon, pumpkin, mushrooms, cactus, clay, big trees).
    - Plants are neither meshed nor solid (`is_plant`); atlas colours for the new blocks. Spawn search skips trees.
    - The M4f chunk manager compiled first try on rustc 1.85; its 3 tests pass.
- `done` M4f chunk manager (`world/chunks.rs`; written without a Rust toolchain, NOT compiled, tested or run on
  a device yet, so expect a compile fix or two on the first CI run):
    - Replaces the 48x48 super-chunk (gone from `lib.rs`, `mesh.rs`, `populate_ores`). Chunks live in a
      `HashMap<(cx, cz), Entry>`; each has its own vertex/index buffer (empty chunks have none).
    - Ring: circle of `RENDER_DIST` (4) chunks is meshed and drawn, `RENDER_DIST + 1` is generated (a chunk is
      meshed only when its 4 neighbours exist, so seams cull correctly), unload beyond `RENDER_DIST + 2`.
    - Generation runs on 1-2 `std::thread` workers, each with its own generator (result does not depend on
      the worker: `generate()` reseeds per chunk). At most `2 x workers` requests in flight, rebuilt nearest-first
      from the player position every frame. Meshing + upload stay on the render thread, 2 chunks per frame.
    - Physics reads blocks through `ChunkManager::block`; an unloaded chunk reads as solid (invisible wall,
      no falling into void). Spawn: the 3x3 chunks around the origin are generated in `App::init`.
    - `populate_ores` is chunk-local (veins clipped at chunk edges; still UNVERIFIED, still replaced in M4d).
    - Tests: mesher edge culling (4 sides), block lookup across negative chunk coords, ring load/unload.
    - Not done on purpose: frustum culling (M13; the draw loop has a marker comment), greedy meshing, mesh on
      workers, saving chunks.
- `aef24f7` M3e-atlas-fix (1/2): 16x16 atlas + panic catcher in init.
- `2a5497f` M3e-atlas-fix (2/2): Uint32 indices. **Black screen root cause**: the
  48x128x48 super-chunk pushes the per-frame index count past `u16::MAX`, so
  `IndexFormat::Uint16` silently clipped the draw and wgpu presented black.
  Atlas size was a red herring.
- `ad33e0d` M12: Touch UX overlay (HUD + d-pad + look-stick + pause menu).
  Files: `src/input/{mod,touch_ui}.rs`, `src/render/hud.rs`,
  `src/lib.rs`, `ROADMAP.md`. Region-based touch UI state machine plus a
  separate 2D orthographic wgpu pass for the HUD overlay (alpha-blended,
  depth-less, runs after the chunk pass). 9 unit tests, 15/15 pass.
  Build status:
    - `cargo test --lib`: 15/15 pass on the host.
    - `cargo build --target aarch64-linux-android --lib --release`: clean,
      4.5 MB `libmc_rs.so`, ELF confirmed AArch64.
    - `cargo apk build --release`: not run locally (this host is aarch64
      Android, the SDK build-tools are x86_64 glibc ELFs). The CI workflow
      at `.github/workflows/ci.yml` runs the same command on a GitHub-hosted
      ubuntu-22.04 runner — that APK build is the end-to-end check.
  Verified against b1.7.3 sources:
    - Hotbar cell count (9), 20 px cell width, 22 px tall, centred horizontally.
      `GuiIngame.java` lines 58-62.
    - `moveStrafe` / `moveForward` semantics ({-1,0,1}). `MovementInputFromOptions.java`
      lines 67-83.
    - Jump velocity 8.4 m/s when on_ground. Preserved from the M2 baseline.
  UNVERIFIED (no b1.7.3 source — b1.7.3 PC has no touch UX at all):
    - Move-stick radius, look-zone split (left/right half), jump button position.
    - Pause and resume button rectangles.
  Follow-up: cross-check the touch rectangles against Pocket Edition 0.x
  or any later touch-based Minecraft client. Run `cargo apk build --release`
  on the CI runner and verify on a phone.
- `done` M4a-c: faithful Random/noise, climate biomes, terrain + surface. Golden test now
  passes (it failed before M4e because of the float constants below).
- `done` M4e fix pass (built and unit-tested on a host with a stubbed android-activity;
  NOT yet run on a device):
    - 48x48 super-chunk was indexed with the 16-deep `(x<<11)|(z<<7)|y` shifts; the bits
      overlap, so 1536 of 2304 columns overwrote each other (scrambled terrain). Now
      `(x*depth+z)*128+y` in lib.rs, mesh.rs, overworld.rs. Test: single block = one cube.
    - Jump never fired: `ground_test` probed under the box centre, not the feet. Test:
      standing on a floor is on_ground and can jump.
    - Camera sat 2.52 above the feet (1.62 added to the box centre). Now 1.62 above feet.
    - `camera.aspect` was never initialised (1.0); set at init, and the swapchain is
      re-synced to the native window size on resize/insets/focus events.
    - Status + nav bars hidden via JNI (`immersive.rs`) on window creation and focus gain.
    - Climate noise scales: `(double)0.025F` / `(double)0.05F` as in the Java.
  Confirmed on a device (screenshot): terrain renders correctly and blocks are square, so the
  stretch is gone. Still open: caves; populate; faithful ores; HUD does not yet avoid the
  display cutout.
    - Black strip at the left edge (display cutout area): fixed in `immersive.rs` with
      `layoutInDisplayCutoutMode = SHORT_EDGES` (Android 9+). Not yet seen on a device.
- `pending` M4d: caves, trees, populate (next, see "Next step" above).
