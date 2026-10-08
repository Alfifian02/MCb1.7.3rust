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
| M4e Fix pass | done (not yet run on a device) | 48x48 stitching index, jump ground probe, eye height, aspect/resize sync, immersive nav bar, climate float constants. See the changelog in ROADMAP.md. |
| M4f Chunk manager | done (now compiles and its tests pass on a Linux host; not run on a device) | Chunks keyed by (cx, cz), one mesh per chunk, circular render-distance ring (4 chunks) that loads/unloads as you walk, terrain generated on 1-2 worker threads, cross-chunk face culling. Replaces the 48x48 super-chunk. |
| M4d Caves, trees, populate | done (compiled + golden-tested on a Linux host; not run on a device) | MapGenCaves in `generate`, full `populate()` (lakes, dungeons, clay, dirt/gravel/ores, oak/birch/big/taiga trees, flowers, grass, reeds, pumpkins, cactus, springs, snow). Bit-exact vs the real Java on 11 cases (`tools/golden/`). Chunk manager gained the populated/final state. |
| M5 Block interaction | written, NOT compiled | Raycast pick, break (hold), place (tap), light engine port, outline + crosshair. See ROADMAP changelog. |
| M6..M14              | pending | See ROADMAP.md for the order. |

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
      atlas.rs            16x16 RGBA block-id atlas, vanilla-Beta-1.7 colours
      camera.rs           FirstPersonCamera: yaw/pitch/look_at/perspective
      hud.rs              2D orthographic overlay pipeline (M12)
      mesh.rs             per-chunk mesher, culls against the 4 neighbouring chunks
    input/
      touch_ui.rs         region hit-test + per-pointer state machine (M12)
    world/
      chunk.rs            16x16x128 cell layout, idx = (x<<11)|(z<<7)|y
      chunks.rs           ChunkManager: HashMap<(cx,cz), chunk + mesh>, ring streaming, worker threads
                          (not WorldChunkManager below, which is the vanilla climate/biome class)
      biome.rs            Beta-1.7 climate -> biome table (10 reachable biomes)
      physics.rs          swept AABB, gravity, on_ground
      pick.rs             M5 ray trace + place rules
      chunks/light.rs     M5 light engine (port of World/Chunk lighting)
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
  Random draw: light is the column model (no lateral spread, no block light), no block metadata (leaf/log species,
  tall-grass type), no tile entities (chest loot / spawner mob are drawn and dropped), no block ticks (springs do
  not flow, sand does not fall). Order dependence: vanilla populates in load order; here it is nearest-first.
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
- `input::touch_ui::LayoutRects::for_surface` — hotbar matches
  `GuiIngame.java` lines 58-62 (9 cells, 20 px wide, 22 px tall, centred
  horizontally, 22 px above the bottom edge). The move stick / look drag /
  jump / pause buttons are UNVERIFIED — b1.7.3 PC has no touch UX.

## Build

### Tests (host, fast)
```
cargo test --lib
```
Currently 14 unit tests, all passing on a Linux host (rustc 1.85, built in a scratch crate that `#[path]`-includes the sources, because `android-activity` does not build there). That includes the 18-chunk terrain golden, `populate_matches_java` (11 raw+populated 2x2 cases) and `block_tables_match_java`. `ring_loads_then_unloads_when_walking` and the golden tests generate real chunks: use `--release`.
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
- M13 adds frustum culling (a filter over `ChunkManager::meshes()`, bounds come from the chunk key) and
  greedy meshing.
- No JNI calls except the one `android_main` entry point; everything
  inside the game loop is pure Rust.

## What M12 is not (yet)

- No text rendering. The hotbar slots are coloured squares (one per
  block id), not item sprites. M14 will add the font + GUI atlas.
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
- Plants (flowers, tall grass, mushrooms, reeds, snow layer) are not drawn and not solid yet (`is_plant`); leaves, water and
  lava are drawn as opaque flat-colour cubes. Real models/textures are M14.
