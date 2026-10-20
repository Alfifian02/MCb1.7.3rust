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
| M12 Touch UX         | done (d-pad: compiled and ran, not yet to be tested) | Landscape-only. PE-style d-pad (Stone D-pad sprites) with 8 directions: an arrow shows its two corners, the centre is sneak (toggle); look drag anywhere else, jump button, hotbar tap, pause menu. Sneak (b1.7.3): x0.3 speed, no walking off edges, holds on ladders, eye 0.08 lower. **Pregen** button (pause menu, top-left) makes the tiles around the player with trees, nearest first, on 1-3 background threads of its own (not starved by the view any more), and keeps them in `<world>/lod/` (Voxy World Gen). See ROADMAP changelog. |
| M4a-c Faithful terrain + biomes | done (biome placement replaced 2026-10-18, see ROADMAP) | java.util.Random, Perlin/simplex noise, WorldChunkManager (was climate -> 10 biomes, now multi-noise like Minecraft 1.18+), generateTerrain and replaceBlocksForBiome. Bit-exact against the real b1.7.3 classes on 18 chunks (3 seeds), golden test passes (`tools/golden/`). |
| M4e Fix pass | done (compiled and ran on an Android device) | 48x48 stitching index, jump ground probe, eye height, aspect/resize sync, immersive nav bar, climate float constants. See the changelog in ROADMAP.md. |
| M4f Chunk manager | done (now compiles and its tests pass on a Linux host; compiled and ran on an Android device) | Chunks keyed by (cx, cz), one mesh per chunk, circular render-distance ring (4 chunks) that loads/unloads as you walk, terrain generated on 1-2 worker threads, cross-chunk face culling. Replaces the 48x48 super-chunk. |
| M4d Caves, trees, populate | done (compiled + golden-tested on a Linux host; compiled and ran on an Android device) | MapGenCaves in `generate`, full `populate()` (lakes, dungeons, clay, dirt/gravel/ores, oak/birch/big/taiga trees, flowers, grass, reeds, pumpkins, cactus, springs, snow). Bit-exact vs the real Java on 11 cases (`tools/golden/`). Chunk manager gained the populated/final state. |
| M5 Block interaction | compiled and ran on an Android device | Raycast pick, break (hold), place (tap), light engine port, outline + crosshair. See ROADMAP changelog. |
| Day/night            | compiled and ran on an Android device | 20-minute cycle: sky light 0..11 subtracted in the mesher, sky colour from sun angle + climate. Sun, moon, stars and weather: next row. |
| Sun, moon, stars, weather | compiled and ran, not yet to be tested | Sky pass drawn behind the terrain with the real sun/moon textures, sunrise/sunset glow, stars, fog-coloured clear; rain/thunder timers (`World.updateWeather`) that grey the sky and fog, darken the sky light and fade sun and stars. Rain/snow streaks, lightning, clouds and terrain fog are not drawn yet. See ROADMAP changelog. |
| Digging              | compiled and ran on an Android device | Hardness-based survival digging, hold to dig, progress bar. No tools yet. |
| Drops + inventory    | compiled and ran on an Android device (logic also tested on a Linux host) | A broken block drops its `idDropped` items as entities (20 Hz motion, pickup after 10 ticks). The hotbar holds real stacks with counts and starts empty; placing uses one up. See ROADMAP changelog. |
| M6 Crafting + tools  | compiled and ran on an Android device (logic also tested on a Linux host) | 36-slot inventory, 2x2 inventory crafting and 3x3 workbench crafting, furnace smelting (`TileEntityFurnace`, 8 smelting recipes), 31 recipes (wood/stone/iron/diamond/gold pickaxe, axe, shovel, sword, hoe + shears + planks, sticks, workbench, chest, furnace, torch, ...), tool speed and durability, and `canHarvestBlock`: stone without a pickaxe breaks and drops nothing, like the original. See ROADMAP changelog. |
| Block metadata       | compiled and ran on an Android device | 4-bit `Nibbles` per chunk (= `NibbleArray`, the McRegion `Data` tag as is), `meta` / `set_block_meta`, worldgen writes it (birch + taiga species, grass type, pumpkin facing), drops carry `damageDropped`, items place `getPlacedBlockMetadata`, the atlas shows log/leaf species and the 15 wool colours. Block shapes (slab, stairs, door, bed) are not part of it. Java golden regenerated: old lines unchanged, new `META` hashes. See ROADMAP changelog. |
| M6b Block updates    | compiled and ran, not yet to be tested | `world/ticks.rs`: scheduled ticks (`scheduleBlockUpdate`, 1000 per tick) + 80 random ticks per chunk within 9 chunks, `onBlockAdded`/`onBlockRemoval`/`onNeighborBlockChange` replayed from a change log of the notifying setters. Water and lava flow (`BlockFlowing`: levels, falling, source making, lava slow + hardening to obsidian/cobblestone), sand and gravel fall (`EntityFallingSand` as a flat box), leaf decay, sapling growth (reuses the populate tree generators), plants uproot without ground, grass spread, crops, farmland, reeds, cactus. No fire, mushroom spread, snow/ice, rain on farmland. See ROADMAP changelog. |
| Health + damage      | compiled and ran on an Android device (56 host tests) | 20 health, fall damage, drowning, lava + fire, void, death drops the inventory and respawns. Fluids are not solid and the player swims. Mushroom stew (bowl + 2 mushrooms) is the only food, tap to eat. See ROADMAP changelog. |
| M7 Save              | compiled and ran on an Android device | Own simple format, not McRegion: one run-length-coded file per edited chunk + a `level` file (player, inventory, furnaces, dropped items, time). Autosave every 5 s, full save on pause/exit, resume on start. Saves live in the app's private storage, one folder per seed. +2 tests. See ROADMAP changelog. |
| Block textures       | compiled and ran, not yet to be tested | Real `terrain.png` tiles on every block face (`assets/terrain.rgba`, grass/foliage tint baked for one climate), per-face tiles from `getBlockTextureFromSideAndMetadata`, cut-out plants and glass. Items, mobs and the HUD stay flat colour. See ROADMAP changelog. |
| Real-time shadows    | compiled and ran, not yet to be tested | Port of shaderLABS/Shadow-Tutorial to wgpu, no Iris/OptiFine: sun depth pass (1024^2, distortion 0.10, foliage excluded) + per-pixel compare in the chunk shader, replaces the mesher's baked shadow ray. See ROADMAP changelog. |
| Volumetric light     | compiled and ran, not yet to be tested | Port of AstraLex's light shafts (`render/vl.rs`): half-resolution ray march through the sun/moon shadow map + blurred blend over the frame; the shadow pass now also runs at sunrise, sunset, in rain and at night for it. Surface shadows come from this pass too (face normal rebuilt from depth, normal + slope bias) and the mesher no longer lets leaves dim the sky light, so a tree is shaded once, by the shadow map. See ROADMAP changelog. |
| Photon lighting      | compiled and ran, not yet to be tested | Milestone 1 of the photon port (`render/photon.rs`): G-buffer, HDR frame, deferred lighting (sun/sky/block light); the tone map and grade are `render/post.rs` (M5). Sky, shadows, fog, clouds, post follow (see ROADMAP changelog). |
| M8 Mobs              | compiled and ran, not yet to be tested | 13 mobs on one AI (`EntityCreature`/`EntityLiving` wander at 20 Hz, 0.9..3.6 box physics via `physics::step_box`): pig, cow, sheep (fleece colours), chicken (eggs, slow fall), wolf (angry when hit), squid, zombie, zombie pigman, giant, skeleton (arrows), creeper (fuse + `Explosion`), spider (leap, climbs), slime (hops, splits). Boxy models with swinging limbs, tap to hit (swords 4+, tools 2+, hand 1, 10-tick hurt window, knockback), `dropFewItems` loot, zombies/skeletons burn in daylight, spawns by light/grass/water. No path-finder, ghast, taming, shearing, sound, save. See ROADMAP changelog. |
| Chest storage        | compiled and ran, not yet to be tested | `world/chest.rs`: `TileEntityChest` (27 slots), `BlockChest` (`canPlaceBlockAt`, `blockActivated`: tap opens, a normal cube above blocks it, `getBlockTexture`: front toward the open side, large-chest halves 41/42/57/58 from the neighbours, no metadata), `InventoryLargeChest` (two chests = one 54-slot screen, upper half first), `GuiChest` geometry (`114 + 18 * rows` tall) and titles, contents saved in `level` (older saves still load), a removed chest spills its slots, dungeon chests now hold their loot. No shift-click. See ROADMAP changelog. |
| Photon sky (port 2/6) | compiled and ran, not yet to be tested | Photon's atmosphere LUT sky, stars, sun, moon, sky map + SH (`render/sky.rs`, `sky.wgsl`). Licence: `LICENSE-PHOTON`. See ROADMAP changelog. |
| Photon M5 post chain | compiled and ran, not yet to be tested | `render/post.rs` + `post.wgsl`: line-by-line port of Photon's `c2_dof`, `c4_taa_exposure`, `c5`..`c18` bloom, `c19_color_grading` (ACES RRT+ODT, rec. 2020 working space), `c21_fxaa`, `final` (CAS + dither). The world now draws into `Post`'s scene texture, the light shafts blend into it, the chain ends on the surface and the HUD is drawn over it. See ROADMAP changelog. |
| Photon port M6 (GTAO, water, weather) | compiled and ran, not yet to be tested, not wired into `lib.rs` | `render/gtao.rs`, `render/water.rs`, `render/weather.rs`, `render/noise.rs`: ports of Photon's `d3_ao`, translucent water + `c1_blend_layers`, `gbuffers_weather`, and b1.7.3 `renderRainSnow`. Waits for the HDR target of milestone 1. See ROADMAP changelog. |
| Rendering gaps       | compiled and ran, not yet to be tested | Biome tint per block, grass side overlay, animated water/lava/fire (`render/anim.rs`), mipmaps, item sprites (HUD + drops), back-to-front water chunks, door / bed / ladder / rail models. See ROADMAP changelog. |
| Player + world effects | compiled and ran, not yet to be tested | `world/effects.rs`: splash + drowning bubbles, suffocation, lightning (thunderstorm rolls, bolt, damage, sky flash), dungeon mob spawners as tile entities (saved), rain/snow streaks wired into the frame (`render/weather.rs`), entities lit by their own cell. See ROADMAP changelog. |
| Gear + shift-click | compiled and ran, not yet to be tested | `world/gear.rs`: empty/full buckets (water + lava sources, milk from a cow), bow + arrows (player arrows hurt mobs, land as pick-up items), fishing rod + bobber (`EntityFish`, raw fish), shears and dye on sheep (bare for good, no regrowth in b1.7.3), bread / golden apple / bow / arrow / rod recipes, armor bar in the HUD, shift-click (UNVERIFIED: not in b1.7.3) behind a toggle button. See ROADMAP changelog. |
| Trapdoor (M19)       | compiled and ran on an Android device, host tests not run (+1 test not yet to be tested) | `BlockTrapDoor`: flat 3/16 flap or standing against its wall, tap flips it, placed on a side face of a normal cube, drops itself when the wall goes, 6 planks -> 2. No sound, no redstone. See ROADMAP changelog. |
| Cake (M19)           | compiled and ran on an Android device, host tests not run (+1 test, +2 asserts not yet to be tested) | `BlockCake` + `ItemReed` use: place on solid ground, tap eats a slice (heals 3 below full health, 6 slices), bitten box + tiles, gone with its ground, recipe gives the milk buckets back as empty ones. Needs `seeds-reeds.patch` (M15). Sugar has no recipe until M16. See ROADMAP changelog. |
| Sign + painting (M19), cactus on mobs (M17), TNT recipe + flash (M18) | compiled and ran on an Android device, host tests not run | `world/decor.rs`: `EntityPainting` / `EnumArt` / `ItemPainting` (motive drawn among those that fit, tap to break, re-check every 101 ticks), `BlockSign` / `ItemSign` / `TileEntitySign` with an on-screen-keyboard `GuiEditSign`, post and wall models from `sign.png`, text in world, saved in the `decor` file; `art/kz.png` + `sign.png` joined the atlas. See ROADMAP changelog. |
| Piston animation (M21), boats (M22), speed ports | compiled and ran on an Android device, host tests not run (the piston patch was `cargo check`ed and tested in a Linux scratch crate) | Pistons slide half a block a tick through a moving block 36 and carry entities (`ticks.rs`); `world/boat.rs`: `EntityBoat` + boat item + recipe, paddled by walking, planks model. Speed: chunk files are written by an I/O thread (C2ME), collision boxes and explosion rays are cached (Radium), terrain sections hidden behind terrain are not drawn (`embeddium/` crate, LGPL-3.0-or-later, `COPYING*`). See ROADMAP changelog. |
| Nether (M11 part 1) | written without a toolchain: not compiled, not run (+6 tests) | `world/gen/nether.rs` (`ChunkProviderHell`, `MapGenCavesHell`, populate), `world/portal.rs` (`BlockPortal`, `Teleporter`): fire on an obsidian frame lights a portal, 4 s inside it swaps to the Nether (own folder `<world>/nether`, coordinates x8 / /8, death returns to the overworld), no sky light, hell fog, soul sand slows. Missing: ghast, fireball, pig-zombie, Nether light table, animated portal. |
| Tectonic world type | written without a toolchain: not compiled, not run (+4 tests in `tectonic`, +1 in `gen::overworld`); the Python reference ran | `tectonic/` crate: Tectonic's terrain datapack (continents, erosion, ridges, splines, regions; `terrain.json`) evaluated by a Rust density-function interpreter with Minecraft's noise, fitted to 128 blocks; it replaced the Realistic world type (`realistic.rs` is gone). The pause menu's `World: ...` button (two taps) saves, switches the type and resets. Caves, rivers and lava tunnels of the mod are not ported. See ROADMAP changelog. |
| M14 menus (part 1)   | written without a toolchain: not compiled, not run (+4 tests in `menu.rs`, 1 in `touch_ui.rs`) | `menu.rs`: b1.7.3's `GuiMainMenu` (dirt, `mclogo.png`, rotated splash), `GuiSelectWorld` (scrolling list, double tap plays), `GuiCreateWorld` (name, seed, world type), delete confirm, "Loading level"; textures from the original jar (`tools/gen_menu.py`). Worlds live in `saves/<folder>/`; the pause menu got "Save and quit to title". The renderer button is gone (Vulkan first, GLES fallback stays automatic). See ROADMAP changelog. |
| M14b Options + game menu | written without a toolchain: not compiled, not run (+5 tests in `options.rs`, 1 in `touch_ui.rs`) | `options.rs`: b1.7.3's `GameSettings` (`options.txt`), `GuiOptions`, `GuiVideoSettings`, `GuiControls`, `GuiSlider` and `GuiIngameMenu`, from the pause button and the title screen's "Options...". Replaces the old pause menu (its Reset world button is gone); Pregen, Fly and LoD debug stay top-left; new `LoD: ON/OFF` button in Options. Sensitivity, invert, render distance, GUI scale, performance cap and difficulty act; music, sound, bobbing, anaglyph, advanced OpenGL, graphics, smooth lighting and the keys are stored only. See ROADMAP changelog. |
| Lazy noise (C2ME idea 5) | written without a toolchain: not compiled; the generator tests incl. the golden `matches_java_reference` pass in a Linux scratch crate | The unread samples of the two big density stacks are not computed (`OctaveNoise::generate_masked_into`, masked per cached column); bit for bit the same terrain. See ROADMAP changelog. |
| Distant terrain (height columns, not drawn) | written without a toolchain: not compiled, not run (+7 tests) | `lod/` crate + `world/far.rs`: the overworld drawn out to 14 chunks as height-column tiles (mip quadtree, 4 levels), generated without caves on a worker thread, saved in `<world>/far`; none in the Nether. See ROADMAP changelog. Superseded as the drawn far terrain by the Voxel LoD row below; `lod/` and `world/far.rs` stay in the tree, unused by `App`. |
| Voxel LoD (Voxy style) | written without a toolchain: not compiled, not run (10 tests in `world::lod`, rewritten 2026-10-17) | `world/lod.rs`: far terrain out to 768 blocks as real 3D block columns (runs of equal blocks) of 64 x 64 tiles, meshed as voxels at 1 / 2 / 4 / 8 / 16 blocks after a Voxy-style 2x2x2 mip (`Mipper`), faces only where a solid voxel meets a see-through one; same atlas, tints, face shades and fog as the chunks. **Real chunks first** (Voxy's ingest): `lod::columns`, `ChunkManager::take_ingest`, `Lod::ingest`. The cut under the ring only happens where a real chunk mesh exists (`ChunkManager::drawn`), so no hole to the sky. Trees are real trunk + crown voxels up to 420 blocks and a leaf carpet over dense forest beyond (2026-10-18), the terrain fog starts at 88% of the view, pre-generation runs from the start; terrain rounds to the step instead of bloating. See ROADMAP changelog. |
| River steps (organic slope) | written without a toolchain: not compiled, not run (+3 tests: `hydro::tests::a_lower_river_does_not_overwrite_a_higher_one`, `ticks::tests::woken_edge_water_spreads_into_levels`, `chunks::tests::edge_water_is_a_surface_source_with_air_beside_it`) | A river from a neighbouring plate is no longer cut by a lower river's bank (`Region::plan_at`), and the still water at a step or a cut bank is woken like the mod's `scheduleTick` (`ChunkManager::edge_water`, `Ticks::wake_water`) so the fluid sim writes the slope as real levels. See ROADMAP changelog. |
| M14                  | pending | Menus (part 1: title, world list, create world; see above), options, chat, real mob models, first-person hand: see ROADMAP.md. M9 audio, M10 multiplayer and M13 optimization are out of scope on purpose. |

Latest commit on `main`: see `git log -1`. Latest released APK: see the
`MinecraftB173Rust-apk` artifact on the GitHub Actions run.

## Layout

```
mc-rs/
  src/
    lib.rs                android_main + App (input, frame, render, build_hud, sync_size)
    immersive.rs          hides status + nav bars and extends into the notch, via JNI (Java main thread)
    gpu/
      context.rs          wgpu instance; Vulkan first, GLES as the fallback
    menu.rs               M14: title screen, world list, world creation (b1.7.3 `GuiMainMenu` / `GuiSelectWorld` / `GuiCreateWorld`)
    options.rs            M14b: `GameSettings` + Options / Video Settings / Controls + the game menu (b1.7.3 `GuiOptions` / `GuiIngameMenu`)
      pipeline.rs         chunk pipeline + atlas upload + uniforms
    render/
      shadows.rs          photon shadow port (M3 of 6): distortion, bias, PCSS as WGSL + group 2 uniforms + blue noise
      post.rs, post.wgsl  Photon post chain: TAA + exposure, bloom, DOF, ACES grading, FXAA, CAS (milestone 5 of the shader rewrite)
      gtao.rs / water.rs / weather.rs / noise.rs   Photon port milestone 6 (see above)
      vl.rs               volumetric light (AstraLex port): CPU `params` + march pass + blend pass
      photon.rs           photon port: CPU light colours, G-buffer target + deferred pass (the HDR scene and tone map are `post.rs`)
      sky.rs, sky.wgsl,   sky: Photon port (d0 sky map, d4a SH, stars, sun, moon) over the atmosphere LUT (assets/scattering.dat)
      sky_sh.wgsl
      atlas.rs            256x288 RGBA atlas: real terrain.png tiles (`terrain_tile` per block face) + a flat-colour strip for items/mobs/unknown blocks
      camera.rs           FirstPersonCamera: yaw/pitch/look_at/perspective
      hud.rs              2D orthographic overlay pipeline (M12)
      mobs.rs             M8: Mob (13 kinds), Mobs (AI tick, spawn, hit, loot, arrows, explosions, ray pick)
      items.rs            dropped items: one small flat-colour cube each, bobbing
      mesh.rs             per-chunk mesher, culls against the 4 neighbouring chunks
    input/
      touch_ui.rs         region hit-test + per-pointer state machine, 8-way d-pad + sneak button (M12)
    world/
      chunk.rs            16x16x128 cell layout, idx = (x<<11)|(z<<7)|y; `Nibbles` = block metadata (NibbleArray)
      chunks.rs           ChunkManager: HashMap<(cx,cz), chunk + mesh>, ring streaming, worker threads
                          (not WorldChunkManager below, which is the vanilla climate/biome class)
      biome.rs            the 10 biomes + `from_params`, the Minecraft 1.18+ placement folded onto them (`from_climate`, the Beta table, is test-only)
      physics.rs          swept AABB, gravity, on_ground
      pick.rs             M5 ray trace + place rules
      dig.rs              hardness table + dig-time maths (PlayerControllerSP) + canHarvestBlock gate
      items.rs            ItemStack, 36-slot Inventory (hotbar = 0..9), idDropped/quantityDropped rules, EntityItem physics + pickup + throw
      boat.rs             M22: `EntityBoat` (buoyancy, paddling, wall smash), the boat list
      decor.rs            M19: paintings (`EntityPainting`, `EnumArt`) and signs (`TileEntitySign`, the editor's keyboard), `decor` save codec
      chest.rs            chest storage: tile entity, placement/open rules, large-chest grouping, face tiles, reach
      craft.rs            M6: tools (EnumToolMaterial, getStrVsBlock, canHarvestBlock), recipes (CraftingManager), inventory/workbench screen (Container clicks) + its GUI geometry
      sky.rs              day/night + weather: sun angle, skylight subtracted, sky/fog/sunrise colours, star brightness + positions, `Weather` (rain/thunder timers)
      save.rs             M7: chunk file codec (RLE), `Level` codec (player, inventory, furnaces, drops), atomic write
      effects.rs          particles (splash, bubbles), lightning, suffocation, dungeon spawners
      vitals.rs           health, air, fire, fall damage (EntityLiving.attackEntityFrom and friends)
      chunks/light.rs     M5 light engine (port of World/Chunk lighting)
      lod.rs              far terrain: column store + mip + textured cell mesher on its own thread (Voxy ideas), cut out where the ring draws
      ticks.rs            M6b block updates: scheduled + random ticks, notifications, water/lava, sand, leaves, saplings, crops
      gen/
        mod.rs            module list
        caves.rs          MapGenBase + MapGenCaves
        populate.rs       Region (2x2 chunks) + populate() and every WorldGen* class
        noise.rs          PerlinNoise + OctaveNoise, java.util.Random clone
        overworld.rs      ChunkProviderGenerate port (density + biome surface)
embeddium/               std-only crate (Sodium/Embeddium port): section occlusion culling (`vis`, `cull`), buffer arena, compact vertex; `cargo test -p embeddium` runs on a plain host
tectonic/                std-only crate (Tectonic port, MIT): `terrain.json` (the datapack bundled by `tools/tectonic.py`) + a density-function interpreter and Minecraft's noise; `cargo test -p tectonic` runs on a plain host
lod/                     std-only crate: distant terrain (height-column tiles, mip quadtree, selection, mesher, save file); `cargo test -p lod` runs on a plain host
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
  not flow, sand does not fall). Order dependence: vanilla populates in load order; here it is nearest-first, with up to one populate per worker out at once (jobs in flight never share a chunk, so each sees its neighbours' finished populates).
- `world::chunk::Nibbles` — `NibbleArray` (same cell index, even cell = low nibble); `Chunk.setBlockID` clears a cell's
  metadata when the id changes, `setBlockIDWithMetadata` writes both (`ChunkManager::set_block` / `set_block_meta`,
  `populate::Region::set` / `set_meta`). `populate` writes metadata where the Java does (`WorldGenForest` birch 2,
  `WorldGenTaiga1/2` spruce 1, `WorldGenTallGrass` 1 or 2, `WorldGenPumpkin` facing); golden-tested bit for bit on the
  packed bytes (`META` lines). Nothing reads metadata for behaviour yet except drops and colours: leaf decay, sapling
  growth, slab/stair/door/bed shapes and furnace facing are still to do.
- `world::items::damage_dropped` / `placed_meta` — `Block.damageDropped` (sapling and leaves `& 3`, log, wool, slabs as
  placed, lapis ore 4) and `Item.getPlacedBlockMetadata` (`ItemSapling`, `ItemLog`, `ItemCloth`, `ItemSlab` pass the damage,
  `ItemLeaves` adds bit 8). `render::atlas::FLEECE` is `EntitySheep.fleeceColorTable`, indexed by the cloth metadata.
- `world::gen::hydro` — port of the Fabric mod Streams Reflowing 2.15.2, read from its class files: jittered 768-block plates (bit for bit the mod's `PlateLattice` + `Hashing`), a 16-block grid probed from the generator (Vanilla density or Tectonic), priority flood, accumulation, wetness-pruned headwaters, separation, lakes, Chaikin + meander, the mod's hydraulic width (slope / climate / variation / braids / delta mouths), depth, inset and flow-speed formulas, `Region::carve`, `current`. Every river drains to the sea (traced on past the window or culled). `tools/hydro_proto.py` is the Python reference.
- `world::gen::chunk_manager` — `shape` gives the vanilla terrain's wanted surface per density column (`OverworldGenerator::density`; `shaped(false)` for Tectonic). Biomes come from Minecraft 1.18+ style multi-noise (temperature, humidity, continentalness,
  erosion, weirdness, domain shift) and `Biome::from_params`; `temperature` / `humidity` (0..1) come from the same noise. The b1.7.3
  manager is `WorldChunkManager::legacy` (tests only: the terrain / surface / populate goldens run on it; its climate noise scales
  are `(double)0.025F` and `(double)0.05F`, float widened to double, not the double literals). `tools/biomes.py` is the Python reference.
- `world::biome::Biome` — the 10 overworld biomes from `BiomeGenBase` plus `Ocean`, `Beach`, `Mountains`, with
  `top_block` / `filler_block` ids; placed by `from_params` (the 1.18+ table, `MIDDLE`).
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
  horizontally, 22 px above the bottom edge). The d-pad / look drag /
  jump / pause buttons are UNVERIFIED — b1.7.3 PC has no touch UX.

## Build

### Tests (host, fast)
```
cargo test --lib
```
Last full run: 56 unit tests (47 before block metadata, 52 with it; swimming, health, food and frustum culling add 4), all passing on a Linux host (rustc 1.85, the whole crate built against a small stand-in for `android-activity`, which does not build there; `tools/` must sit next to the crate for the golden `include_str!`). That includes the 18-chunk terrain golden, `populate_matches_java` (11 raw+populated 2x2 cases), `block_tables_match_java` and the M6 tests (`recipes_match_like_java`, `clicks_follow_container_rules`, `tool_tables`, `furnace_smelts_like_java`, `drop_one_spreads_the_cursor_stack`, `tools_gate_harvest_and_speed_up_digging`, `open_screen_turns_presses_into_taps`). Block metadata adds 5 tests (`nibbles_pack_like_java`, `metadata_follows_chunk_setters`, `metadata_survives_break_and_place`, The sun/moon/weather work then added 5 tests (`sky_matches_java`, `weather_matches_java`, `stars_match_java`, `rain_and_thunder_dim_the_sky`, `sun_and_moon_trade_places`), compiled and ran, not yet to be tested; the first three compare against new `CEL`/`SUBL`/`WEAT`/`WEAH`/`STARS` lines in `golden.txt`, which come from the real classes.
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
  ~113 generated (radius 6) and ~149 kept). the light engine runs on its own thread (`chunk-light`, `world/chunks/light.rs`: its own copy of the blocks, the light comes back in reports); populate and chunk meshes are built on the workers (`Job::Populate` on a copy of its 2x2 block, `Job::Mesh`; ideas of C2ME's scheduler): at most one populate per worker is out, and at most 2 meshes are uploaded per frame. One draw call per chunk run (sections of a chunk that touch), from a few shared vertex/index pages (`Pages` in `chunks.rs`), bound once per page. Memory is
  32 KB of blocks per loaded chunk. No device numbers yet for this path; the old 48x48 figure
  (~16 ms/frame on a Pixel 4a) no longer applies.
- Shadows + light shafts: the shadow map is reused between redraws (`render::camera::ShadowCache`), the shadow pass culls back faces, the shafts' blend shares the HUD pass. See the ROADMAP changelog for the levers still open. Unmeasured.
- Shadows + light shafts + glare: the shadow map is reused between redraws (`render::camera::ShadowCache`), the shadow pass culls back faces, the shafts' blend shares the HUD pass, the glare quad is smaller. See the ROADMAP changelog for the levers still open. Unmeasured.
- Legacy frame (`LEGACY` in `lib.rs`): one scene pass (terrain lit in `fs_main`, sky, water) then the post chain (exposure 1x1, grade, final; TAA is skipped while it is off). No G-buffer, depth copy or deferred pass; those exist only for the Photon look. Unmeasured on a device.
- Internal resolution is `gpu::context::RENDER_SCALE` (0.67) of the screen, upscaled by the final pass; the surface shadow is looked up in the deferred pass (`render/photon.rs`), the light shafts are `render/vl.rs` only. The FPS log line shows CPU ms per frame. Unmeasured on a device.
- M13 frustum culling is in (`render::camera::Frustum`, filtered through `ChunkManager::meshes_where`); M13 still adds
  greedy meshing.
- No JNI calls except the one `android_main` entry point; everything
  inside the game loop is pure Rust.

## What M12 is not (yet)

- Text exists only as the item-name tooltip (real `font/default.png` glyphs, `render/font.rs`): hold a finger on an inventory/workbench/furnace slot, or on a picked-up stack, to see its name (`GuiContainer`'s hover tooltip; no menus or chat use it yet). Hotbar slots show a flat colour square per item (counts are seven-segment digits),
  not item sprites. M14 will add the font + GUI atlas.
- The d-pad's look is PE's (Stone D-pad sprites, `tools/gen_dpad.py`); its sizes and places, like the look / jump /
  pause geometry, were invented for this port and are flagged UNVERIFIED in
  the code. If you have a PE 0.x reference, please open an issue
  with the on-screen rectangles so we can diff against them.
- Display cutouts: the window now draws into the notch (`layoutInDisplayCutoutMode =
  SHORT_EDGES`, Android 9+, set in `immersive.rs`), but HUD margins are a fixed fraction of
  the screen height and do NOT avoid the notch. If a control ends up under the camera
  (e.g. the pause button after flipping the phone 180 degrees), add display-cutout insets.
- The nav bar is hidden with `setSystemUiVisibility` (deprecated since API 30 but still
  honoured on 11-14). Not yet verified on a real device.
- Saplings, flowers, tall grass, mushrooms, dead bush and reeds are drawn as textured crossed quads (not solid); slabs, farmland, snow layer and cactus are boxes with their real bounds and collision, torches (also on a wall), fences, stairs and crops have their real shapes (stairs collide as a full cell; no door or bed shape yet), water and lava have a real surface height and flowing tops turned by flow direction (water is translucent, lava opaque); leaves are drawn as opaque cubes. Real models/textures are M14.
