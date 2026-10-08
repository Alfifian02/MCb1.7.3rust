# Roadmap: Minecraft b1.7.3 -> Android in Rust

Fresh start. One milestone per session. Vanilla names. Vulkan-only.
McRegion save format. Client-only networking.

## Milestones
| #   | Name | Deliverable |
|-----|------|-------------|
| M0  | Skeleton | Buildable APK, solid color on screen, lifecycle correct |
| M1  | Blocks + chunks | Hand-built 16x16x128 stone chunk visible |
| M2  | Camera + physics | First-person walk, jump, AABB collision |
| M3  | Overworld gen | Beta-1.7 noise: grass/dirt/stone, sea level 64, ores |
| M4  | Biomes + trees + caves | All b1.7.3 biomes + worldgen populate step |
| M5  | Block interaction | Raycast pick, break, place, light update (done, not compiled: see Changelog) |
| M6  | Inventory + crafting | Survival inv, hotbar, crafting grid, recipes |
| M7  | Save (McRegion) | Read/write .mcr, new-world / save / load |
| M8  | Entities + AI | Mobs + item entities |
| M9  | Audio | Positional OGG, music stubs |
| M10 | Multiplayer | Full b1.7.3 client protocol |
| M11 | Nether | Hell biomes, portals, ghast, zombie pigman |
| M12 | Touch UX | Landscape; move stick, look drag, jump button, hotbar tap, pause menu |
| M13 | Optimization | Frustum culling, greedy meshing, profiler |
| M14 | Polish | Splash, main menu, settings, lang |

## Build
- Workspace at `mc-rs/`
- `cargo test --lib` runs the 22 unit tests (Android host such as Termux; see README).
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

M6: inventory + crafting (M5 is written but must first compile and be checked on a device). M4 is done: caves and populate match the real classes bit for
bit (`tools/golden/`). Still approximate in populate (see README): light model, no metadata, no tile entities, no block
ticks (liquids/sand). Sapling growth, fluid flow and falling sand belong with M5's block updates. Ice Desert exists in
BiomeGenBase but climate never selects it; b1.7.3 has no ravines.

## Changelog
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
