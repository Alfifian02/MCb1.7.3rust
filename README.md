# mc-cpp: Minecraft b1.7.3, first slice of a C++ port of `mc-rs`

C++17, OpenGL ES 3.0, Android NativeActivity. Terrain is the original b1.7.3 generator and is checked bit for bit against
the Java classes (the golden hashes come from `tools/golden/` in the Rust repo).

## Status

| Piece | State |
|---|---|
| `JavaRandom`, Perlin / simplex octaves | **verified** against the Java golden vectors |
| b1.7.3 climate (temperature / humidity) and 2D biome table | **verified** (biome hashes + climate bit patterns, 4 chunks) |
| `generateTerrain`, `replaceBlocksForBiome` | **verified** (terrain + surface hashes, 4 chunks) |
| `MapGenCaves` (`core/caves.cpp`), `MathHelper` sin/cos table | **verified** (RAW hashes of 11 cases x 4 chunks = terrain + surface + caves, 3 seeds, bit for bit) |
| Chunk streaming (worker threads, nearest-first, unload) | **tested** headless, clean under ASan / UBSan / TSan |
| Mesher (face culling, ambient occlusion, water layer) | **tested** headless, looked at via `softshot` |
| Player physics (swept AABB, step-up, jump, swim) | **tested** headless (`sim_test`) |
| GLES 3 renderer, Android entry, touch controls | **written, NOT built or run**: no NDK / GL in the sandbox; only syntax-checked against hand-written stubs |
| `CMakeLists.txt`, `android/` Gradle skeleton | **unverified locally**; the `mc-cpp` GitHub workflow is the first real build |

## Build and test (host)

    make test      # golden test + streaming/physics simulation
    make shot      # CPU-raycast screenshot -> shot.ppm

Android: open `android/` in Android Studio (NDK 26.1 + CMake 3.22.1 installed), or build `CMakeLists.txt` with the NDK toolchain file.

### APK on GitHub

`.github/workflows/mc-cpp.yml` (repo root) runs on every push / PR touching `mc-cpp/`, and by hand (Actions > mc-cpp > Run workflow):

1. `test`: `make test` (golden + simulation), then the same under ASan + UBSan.
2. `apk` (after `test`): installs NDK + CMake, runs `gradle :app:assembleRelease`, signs with the committed `keystore/debug.keystore`
   (same identity as the Rust APK, so `adb install -r` upgrades in place), verifies the signature and that `libmcpp.so` is inside,
   uploads the artifact `MinecraftB173Cpp-apk` and, on pushes, a prerelease `mc-cpp-build-<n>` with `MinecraftB173Cpp.apk`.

Local equivalent: `cd android && gradle :app:assembleRelease` (Gradle >= 8.7; there is no wrapper).
Controls: left 40% of the screen is a floating move stick, drag on the right to look, round button = jump.

## Differences from mc-rs

* Biomes use b1.7.3's original climate table, not mc-rs's multi-noise placement, so a seed gives different land than mc-rs.
* Flat per-face block colours instead of `terrain.png`; fixed lighting (face shade + AO), no light engine, no day/night.
* Not ported yet: `populate` (trees, ores, lakes...), block edit, inventory, mobs, redstone, save, audio, menus, options,
  gamepad, LoD, Tectonic / Cascades / Structory world types, Nether, sky / weather, Vulkan / wgpu path.

## Next steps (mc-rs file -> here)

1. ~~`caves.rs` -> `core/caves.cpp`~~ done. Next: `world/gen/populate.rs` -> `core/populate.cpp` (needs block metadata nibbles first; golden: the POP / META lines of `G.java`, the RAW table in `src/tools/golden_raw.inc` already covers its inputs)
2. `world/chunks/light.rs` -> `core/light.cpp`, then feed the mesher
3. `render/atlas.rs` + `assets/*.rgba` -> texture atlas, UVs in `Vertex`
4. `world/pick.rs`, `dig.rs`, `items.rs`, `craft.rs` -> block interaction + inventory
5. `world/save.rs`, `menu.rs`, `options.rs`, `audio/`
