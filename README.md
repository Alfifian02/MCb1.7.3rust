# Minecraft b1.7.3 in Rust (Android)

A from-scratch port of Minecraft Beta 1.7.3 to idiomatic, safe Rust, targeted
at Android (with desktop runs as a quick smoke). One milestone per session.
The full plan lives in [`ROADMAP.md`](ROADMAP.md); the rules I follow while
porting live in `GPT-5.5 Guide Porting Minecraft Beta 1.7.3 to Rust.md` (in the
parent `github/` directory).

The repo is **not** b1.7.3-as-mod: it's a rewrite whose source of truth is
the b1.7.3 Java sources and the decompiled `minecraft/` tree. Every worldgen
constant, every block id, every movement vector is supposed to match the
original. Anything that cannot be derived from the b1.7.3 sources is marked
`// UNVERIFIED:` in the code with a one-line reason.

## Where we are

| Milestone | State | Notes |
|---|---|---|
| M0  Skeleton         | done | Workspace + wgpu surface + solid-colour clear + no panic on Android. |
| M1  Blocks + chunks  | done | Hand-built 16x16x128 stone chunk, neighbour-culled mesher, 32-bit indices. |
| M2  Camera + physics | done | First-person walk + jump + swept AABB collision. Jump is now a button (see M12). |
| M3  Overworld gen    | done | 48x128x48 super-chunk, sea level 64, 6 ore types (coal, iron, gold, diamond, redstone, lapis). Terrain was rebuilt in M4a-c, see below. |
| M12 Touch UX         | done (this commit) | Landscape-only. Floating analog move stick (left half), look drag (right half), jump button, hotbar tap, pause menu. |
| M4a-c Faithful terrain + biomes | done | java.util.Random, Perlin/simplex noise, WorldChunkManager (climate -> 10 biomes), generateTerrain and replaceBlocksForBiome. Bit-exact against the real b1.7.3 classes on 18 chunks (3 seeds), golden test passes (`tools/golden/`). |
| M4e Fix pass | done (compiled and ran on an Android device) | 48x48 stitching index, jump ground probe, eye height, aspect/resize sync, immersive nav bar, climate float constants. See the changelog in ROADMAP.md. |
| M4f Chunk manager | done (now compiles and its tests pass on a Linux host; compiled and ran on an Android device) | Chunks keyed by (cx, cz), one mesh per chunk, circular render-distance ring (4 chunks) that loads/unloads as you walk, terrain generated on 1-2 worker threads, cross-chunk face culling. Replaces the 48x48 super-chunk. |
| M4d Caves, trees, populate | done (compiled + golden-tested on a Linux host; compiled and ran on an Android device) | MapGenCaves in `generate`, full `populate()` (lakes, dungeons, clay, dirt/gravel/ores, oak/birch/big/taiga trees, flowers, grass, reeds, pumpkins, cactus, springs, snow). Bit-exact vs the real Java on 11 cases (`tools/golden/`). Chunk manager gained the populated/final state. |
| M5 Block interaction | compiled and ran on an Android device | Raycast pick, break (hold), place (tap), light engine port, outline + crosshair. See ROADMAP changelog. |
| Day/night            | compiled and ran on an Android device | 20-minute cycle: sky light 0..11 subtracted in the mesher, sky colour from sun angle + climate. Sun, moon, stars and weather: next row. |
| Sun, moon, stars, weather | compiled and ran on an Android device (written without a Rust toolchain; the 5 new tests are not confirmed run, their Java reference numbers were) | Sky pass drawn behind the terrain with the real sun/moon textures, sunrise/sunset glow, stars, fog-coloured clear; rain/thunder timers (`World.updateWeather`) that grey the sky and fog, darken the sky light and fade sun and stars. Rain/snow streaks, lightning, clouds and terrain fog are not drawn yet. See ROADMAP changelog. |
| Digging              | compiled and ran on an Android device | Hardness-based survival digging, hold to dig, progress bar. No tools yet. |
| Drops + inventory    | compiled and ran on an Android device (logic also tested on a Linux host) | A broken block drops its `idDropped` items as entities (20 Hz motion, pickup after 10 ticks). The hotbar holds real stacks with counts and starts empty; placing uses one up. See ROADMAP changelog. |
| M6 Crafting + tools  | compiled and ran on an Android device (logic also tested on a Linux host) | 36-slot inventory, 2x2 inventory crafting and 3x3 workbench crafting, furnace smelting (`TileEntityFurnace`, 8 smelting recipes), 31 recipes (wood/stone/iron/diamond/gold pickaxe, axe, shovel, sword, hoe + shears + planks, sticks, workbench, chest, furnace, torch, ...), tool speed and durability, and `canHarvestBlock`: stone without a pickaxe breaks and drops nothing, like the original. See ROADMAP changelog. |
| Block metadata       | compiled and ran on an Android device | 4-bit `Nibbles` per chunk (= `NibbleArray`, the McRegion `Data` tag as is), `meta` / `set_block_meta`, worldgen writes it (birch + taiga species, grass type, pumpkin facing), drops carry `damageDropped`, items place `getPlacedBlockMetadata`, the atlas shows log/leaf species and the 15 wool colours. Block shapes (slab, stairs, door, bed) are not part of it. Java golden regenerated: old lines unchanged, new `META` hashes. See ROADMAP changelog. |
| M6b Block updates    | written WITHOUT a Rust toolchain: not compiled, not run | `world/ticks.rs`: scheduled ticks (`scheduleBlockUpdate`, 1000 per tick) + 80 random ticks per chunk within 9 chunks, `onBlockAdded`/`onBlockRemoval`/`onNeighborBlockChange` replayed from a change log of the notifying setters. Water and lava flow (`BlockFlowing`: levels, falling, source making, lava slow + hardening to obsidian/cobblestone), sand and gravel fall (`EntityFallingSand` as a flat box), leaf decay, sapling growth (reuses the populate tree generators), plants uproot without ground, grass spread, crops, farmland, reeds, cactus. No fire, mushroom spread, snow/ice, rain on farmland. See ROADMAP changelog. |
| Health + damage      | compiled and ran on an Android device (56 host tests) | 20 health, fall damage, drowning, lava + fire, void, death drops the inventory and respawns. Fluids are not solid and the player swims. Mushroom stew (bowl + 2 mushrooms) is the only food, tap to eat. See ROADMAP changelog. |
| M7 Save              | compiled and ran on an Android device | Own simple format, not McRegion: one run-length-coded file per edited chunk + a `level` file (player, inventory, furnaces, dropped items, time). Autosave every 5 s, full save on pause/exit, resume on start. Saves live in the app's private storage, one folder per seed. +2 tests. See ROADMAP changelog. |
| Block textures       | written WITHOUT a Rust toolchain: not compiled, not run | Real `terrain.png` tiles on every block face (`assets/terrain.rgba`, grass/foliage tint baked for one climate), per-face tiles from `getBlockTextureFromSideAndMetadata`, cut-out plants and glass. Items, mobs and the HUD stay flat colour. See ROADMAP changelog. |
| Real-time shadows    | written; shader + pass tested headless on lavapipe (Vulkan) on a Linux host, NOT run on a device, `lib.rs` not compiled | Port of shaderLABS/Shadow-Tutorial to wgpu, no Iris/OptiFine: sun depth pass (1024^2, distortion 0.10, foliage excluded) + per-pixel compare in the chunk shader, replaces the mesher's baked shadow ray. See ROADMAP changelog. |
| Volumetric light     | written WITHOUT a Rust toolchain: not compiled, not run | Port of AstraLex's light shafts (`render/vl.rs`): half-resolution ray march through the sun/moon shadow map + blurred blend over the frame; the shadow pass now also runs at sunrise, sunset, in rain and at night for it. See ROADMAP changelog. |
| M8 Mobs              | written WITHOUT a Rust toolchain: not compiled, 3 mob tests not run | 13 mobs on one AI (`EntityCreature`/`EntityLiving` wander at 20 Hz, 0.9..3.6 box physics via `physics::step_box`): pig, cow, sheep (fleece colours), chicken (eggs, slow fall), wolf (angry when hit), squid, zombie, zombie pigman, giant, skeleton (arrows), creeper (fuse + `Explosion`), spider (leap, climbs), slime (hops, splits). Boxy models with swinging limbs, tap to hit (swords 4+, tools 2+, hand 1, 10-tick hurt window, knockback), `dropFewItems` loot, zombies/skeletons burn in daylight, spawns by light/grass/water. No path-finder, ghast, taming, shearing, sound, save. See ROADMAP changelog. |
| M9..M14              | pending | Everything else: see ROADMAP.md for the order. |

Latest commit on `main`: see `git log -1`. Latest released APK: see the
`MinecraftB173Rust-apk` artifact on the GitHub Actions run.

## Layout

```
mc-rs/
  src/
    lib.rs                android_main + App (input, frame, render, build_hud, sync_size)
    immersive.rs          hides status + nav bars and extends into the notch, via JNI (Java main thread)
    gpu/
      context.rs          wgpu instance + Vulkan/GL fallback for Android
      pipeline.rs         chunk pipeline + atlas upload + uniforms
    render/
      vl.rs               volumetric light (AstraLex port): CPU `params` + march pass + blend pass
      sky.rs              sky pass: dome, sunrise glow, sun, moon, stars, under-plane (assets/sky.rgba = real sun.png + moon.png)
      atlas.rs            256x288 RGBA atlas: real terrain.png tiles (`terrain_tile` per block face) + a flat-colour strip for items/mobs/unknown blocks
      camera.rs           FirstPersonCamera: yaw/pitch/look_at/perspective
      hud.rs              2D orthographic overlay pipeline (M12)
      mobs.rs             M8: Mob (13 kinds), Mobs (AI tick, spawn, hit, loot, arrows, explosions, ray pick)
      items.rs            dropped items: one small flat-colour cube each, bobbing
      mesh.rs             per-chunk mesher, culls against the 4 neighbouring chunks
    input/
      touch_ui.rs         region hit-test + per-pointer state machine (M12)
    world/
      chunk.rs            16x16x128 cell layout, idx = (x<<11)|(z<<7)|y; `Nibbles` = block metadata (NibbleArray)
      chunks.rs           ChunkManager: HashMap<(cx,cz), chunk + mesh>, ring streaming, worker threads
                          (not WorldChunkManager below, which is the vanilla climate/biome class)
      biome.rs            Beta-1.7 climate -> biome table (10 reachable biomes)
      physics.rs          swept AABB, gravity, on_ground
      pick.rs             M5 ray trace + place rules
      dig.rs              hardness table + dig-time maths (PlayerControllerSP) + canHarvestBlock gate
      items.rs            ItemStack, 36-slot Inventory (hotbar = 0..9), idDropped/quantityDropped rules, EntityItem physics + pickup + throw
      craft.rs            M6: tools (EnumToolMaterial, getStrVsBlock, canHarvestBlock), recipes (CraftingManager), inventory/workbench screen (Container clicks) + its GUI geometry
      sky.rs              day/night + weather: sun angle, skylight subtracted, sky/fog/sunrise colours, star brightness + positions, `Weather` (rain/thunder timers)
      save.rs             M7: chunk file codec (RLE), `Level` codec (player, inventory, furnaces, drops), atomic write
      vitals.rs           health, air, fire, fall damage (EntityLiving.attackEntityFrom and friends)
      chunks/light.rs     M5 light engine (port of World/Chunk lighting)
      ticks.rs            M6b block updates: scheduled + random ticks, notifications, water/lava, sand, leaves, saplings, crops
      gen/
        mod.rs            module list
        caves.rs          MapGenBase + MapGenCaves
        populate.rs       Region (2x2 chunks) + populate() and every WorldGen* class
        noise.rs          PerlinNoise + OctaveNoise, java.util.Random clone
        overworld.rs      ChunkProviderGenerate port (density + biome surface)
keystore/
  debug.keystore         committed long-lived keystore (CI signs release APKs)
.github/workflows/
  ci.yml                 test + apk + signed APK + GitHub release
```

## Source-of-truth notes

The things below are pulled directly from the b1.7.3 Java sources and are
known to match. Anything *not* on this list should be treated as
UNVERIFIED until you can show a reference.

- `world::gen::noise::JavaRandom` — exact 48-bit LCG from `java.util.Random`
  (multiplier `0x5DEECE66D`, increment `0xB`, mask `(1<<48)-1`).
- `world::gen::noise::OctaveNoise` — port of `NoiseGeneratorOctaves`,
  1/x amplitude halving across octaves.
- `world::gen::overworld::OverworldGenerator` — port of
  `ChunkProviderGenerate.provideChunk`. 5x17x5 density grid, trilinear
  interpolation across cells, then `replaceBlocksForBiome` for the
  grass/dirt/sand/gravel surface pass.
- `world::gen::caves` — port of `MapGenBase`/`MapGenCaves` (incl. the `y` off-by-one quirk), run at the end of `generate`. Golden-tested.
- `world::gen::populate` — port of `populate()` and the `WorldGen*` classes, same Random draw order. Golden-tested
  against the real classes on a fake `World` (`tools/golden/G.java`). Known simplifications, none of which touch a
  Random draw: light is the column model (no lateral spread, no block light), no tile entities (chest loot / spawner mob are drawn and dropped), no block ticks (springs do
  not flow, sand does not fall). Order dependence: vanilla populates in load order; here it is nearest-first.
- `world::chunk::Nibbles` — `NibbleArray` (same cell index, even cell = low nibble); `Chunk.setBlockID` clears a cell's
  metadata when the id changes, `setBlockIDWithMetadata` writes both (`ChunkManager::set_block` / `set_block_meta`,
  `populate::Region::set` / `set_meta`). `populate` writes metadata where the Java does (`WorldGenForest` birch 2,
  `WorldGenTaiga1/2` spruce 1, `WorldGenTallGrass` 1 or 2, `WorldGenPumpkin` facing); golden-tested bit for bit on the
  packed bytes (`META` lines). Nothing reads metadata for behaviour yet except drops and colours: leaf decay, sapling
  growth, slab/stair/door/bed shapes and furnace facing are still to do.
- `world::items::damage_dropped` / `placed_meta` — `Block.damageDropped` (sapling and leaves `& 3`, log, wool, slabs as
  placed, lapis ore 4) and `Item.getPlacedBlockMetadata` (`ItemSapling`, `ItemLog`, `ItemCloth`, `ItemSlab` pass the damage,
  `ItemLeaves` adds bit 8). `render::atlas::FLEECE` is `EntitySheep.fleeceColorTable`, indexed by the cloth metadata.
- `world::gen::chunk_manager` — climate noise scales are `(double)0.025F` and
  `(double)0.05F` (float widened to double), not the double literals.
- `world::biome::Biome` — the 8 overworld biomes from `BiomeGenBase`, with
  `top_block` / `filler_block` ids.
- `render::atlas::block_color` — approximate vanilla-Beta-1.7 colours per
  block id (16x16 1-pixel-per-id atlas; the mesher scales UVs to the texel
  centre so a Nearest sampler gives flat colour).
- `world::physics` — `Player.pos` is the CENTRE of the AABB (0.6 x 1.8 x 0.6, same as
  `EntityPlayer.setSize`); the ground probe sits under the feet. The camera is 1.62 above
  the feet (`EntityPlayer.yOffset`), i.e. `1.62 - 0.9` above `pos`. Gravity 23 m/s²,
  jump velocity 8.4 m/s, walk speed 4.3 m/s (the source-of-truth numbers
  from `EntityPlayerSP` / `MovementInputFromOptions`).
- `world::craft` — `EnumToolMaterial` numbers (harvest level, uses, efficiency), the `blocksEffectiveAgainst` lists and
  `canHarvestBlock` of `ItemPickaxe`/`ItemSpade`, `RecipesTools` + `RecipesCrafting` + the `CraftingManager` entries it
  carries, `ShapedRecipes.matches` (anywhere in the grid, mirrored), `SlotCrafting.onPickupFromSlot`, the slot positions
  of `ContainerPlayer`/`ContainerWorkbench`, and the left/right click branches of `Container.func_27280_a`. Not ported:
  bow, arrow, shift-click, the rest of `CraftingManager` (one line each in `recipes()`). Armor: `armor` / `armor_fits` /
  `armor_value` (`ItemArmor`, `SlotArmor`, `InventoryPlayer.getTotalArmorValue`), slots `Inv(36..40)` = `armorInventory`.
  Recipes of `RecipesFood`/`Dyes`/`Ingots`/`Armor` are in (shapeless ones: `w` 0, wool and dye match by colour); fish smelts, the lava bucket fuels.
- `world::save` — NOT b1.7.3: it replaces `McRegionChunkLoader`/`NBTTagCompound`/`level.dat` on purpose. Nothing in it is
  derived from the Java, so nothing in it is a fidelity claim. What it keeps is what the port keeps: a chunk's block ids and
  `Nibbles` bytes (the `Data` tag), whether populate ran on it, and the player/world state this port has.
- `world::dig::can_harvest` — `InventoryPlayer.canHarvestBlock` (material half + held-item half), used by `lib.rs` the way
  `PlayerControllerSP.sendBlockRemoved` does: read before the tool wears, so a tool that breaks on a block still harvests it.
- `input::touch_ui::LayoutRects::for_surface` — hotbar matches
  `GuiIngame.java` lines 58-62 (9 cells, 20 px wide, 22 px tall, centred
  horizontally, 22 px above the bottom edge). The move stick / look drag /
  jump / pause buttons are UNVERIFIED — b1.7.3 PC has no touch UX.

## Build

### Tests (host, fast)
```
cargo test --lib
```
Last full run: 56 unit tests (47 before block metadata, 52 with it; swimming, health, food and frustum culling add 4), all passing on a Linux host (rustc 1.85, the whole crate built against a small stand-in for `android-activity`, which does not build there; `tools/` must sit next to the crate for the golden `include_str!`). That includes the 18-chunk terrain golden, `populate_matches_java` (11 raw+populated 2x2 cases), `block_tables_match_java` and the M6 tests (`recipes_match_like_java`, `clicks_follow_container_rules`, `tool_tables`, `furnace_smelts_like_java`, `drop_one_spreads_the_cursor_stack`, `tools_gate_harvest_and_speed_up_digging`, `open_screen_turns_presses_into_taps`). Block metadata adds 5 tests (`nibbles_pack_like_java`, `metadata_follows_chunk_setters`, `metadata_survives_break_and_place`, The sun/moon/weather work then added 5 tests (`sky_matches_java`, `weather_matches_java`, `stars_match_java`, `rain_and_thunder_dim_the_sky`, `sun_and_moon_trade_places`), written without a Rust toolchain and NOT run; the first three compare against new `CEL`/`SUBL`/`WEAT`/`WEAH`/`STARS` lines in `golden.txt`, which come from the real classes.
`metadata_picks_the_tile`, `metadata_picks_the_face_tile`) and makes `populate_matches_java` compare the `META` hashes; the metadata code was first run in the 56-test pass, which also fixed a wrong expectation in `metadata_picks_the_tile` (wool metadata 1 is tile 260).
`ring_loads_then_unloads_when_walking` and the golden tests generate real chunks: use `--release`.
The crate depends on `android-activity` -> `ndk-sys`, which only compiles for Android, so
`cargo test` works on an Android host (e.g. Termux) but not on a plain Linux runner. The CI
`test` job pipes through `tail` without `pipefail`, so a failure there is NOT reported.

### Desktop smoke (Linux / macOS / Windows)
```
# Default backend (Metal on macOS, Vulkan on Windows, Vulkan on Linux).
cargo run
# Force OpenGL (Linux only out of the box).
WGPU_BACKEND=gl cargo run --features=angle
```
On a desktop build, the only input is keyboard + mouse (the M12 HUD is
visible but the touch regions don't accept pointer events the way they
do on Android).

### Android APK
```
# Build the signed release APK. Needs Android SDK + NDK + cargo-apk.
# The CI workflow installs all three on a fresh ubuntu-22.04 runner.
cargo apk build --release
```
The committed `keystore/debug.keystore` is what signs the APK; if you'd
rather use your own, copy it to `~/.android/debug.keystore` and set
`CARGO_APK_RELEASE_KEYSTORE_PASSWORD=android` (the password the keystore
was generated with). The CI workflow does this for you.

## Crates used

| Crate | Version | Role |
|---|---|---|
| `wgpu`         | 26   | Vulkan/GL rendering |
| `glam`         | 0.27 | Vec3 / Mat4 math |
| `bytemuck`     | 1    | Pod for vertex/uniform layouts |
| `android-activity` | 0.6 | `android_main` + input events |
| `pollster`     | 0.4  | block_on for `wgpu` init from a non-async context |
| `log`          | 0.4  | logging facade |
| `env_logger`   | 0.11 | host-side logger |
| `android_logger` | 0.14 | device-side logger |

## Performance notes

- World streaming: render distance is `RENDER_DIST` in `lib.rs` (4 chunks, circular, so ~49 chunks drawn,
  ~113 generated (radius 6) and ~149 kept). populate() runs on the render thread, one chunk per frame. One draw call per chunk, at most 2 chunk meshes built per frame. Memory is
  32 KB of blocks per loaded chunk. No device numbers yet for this path; the old 48x48 figure
  (~16 ms/frame on a Pixel 4a) no longer applies.
- Shadows + light shafts: the shadow map is reused between redraws (`render::camera::ShadowCache`), the shadow pass culls back faces, the shafts' blend shares the HUD pass. See the ROADMAP changelog for the levers still open. Unmeasured.
- Shadows + light shafts + glare: the shadow map is reused between redraws (`render::camera::ShadowCache`), the shadow pass culls back faces, the shafts' blend shares the HUD pass, the glare quad is smaller. See the ROADMAP changelog for the levers still open. Unmeasured.
- M13 frustum culling is in (`render::camera::Frustum`, filtered through `ChunkManager::meshes_where`); M13 still adds
  greedy meshing.
- No JNI calls except the one `android_main` entry point; everything
  inside the game loop is pure Rust.

## What M12 is not (yet)

- Text exists only as the item-name tooltip (real `font/default.png` glyphs, `render/font.rs`): hold a finger on an inventory/workbench/furnace slot, or on a picked-up stack, to see its name (`GuiContainer`'s hover tooltip; no menus or chat use it yet). Hotbar slots show a flat colour square per item (counts are seven-segment digits),
  not item sprites. M14 will add the font + GUI atlas.
- No Minecraft PE touch UX anchor — move-stick / look / jump / pause
  geometry was invented for this port and is flagged UNVERIFIED in
  the code. If you have a PE 0.x reference, please open an issue
  with the on-screen rectangles so we can diff against them.
- Display cutouts: the window now draws into the notch (`layoutInDisplayCutoutMode =
  SHORT_EDGES`, Android 9+, set in `immersive.rs`), but HUD margins are a fixed fraction of
  the screen height and do NOT avoid the notch. If a control ends up under the camera
  (e.g. the pause button after flipping the phone 180 degrees), add display-cutout insets.
- The nav bar is hidden with `setSystemUiVisibility` (deprecated since API 30 but still
  honoured on 11-14). Not yet verified on a real device.
- Flowers, tall grass, mushrooms, dead bush and reeds are drawn as textured crossed quads (not solid); the snow layer is
  neither drawn nor solid yet; leaves, water and lava are drawn as opaque flat-colour cubes. Real models/textures are M14.
