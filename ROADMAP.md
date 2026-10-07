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
| M5  | Block interaction | Raycast pick, break, place, light update |
| M6  | Inventory + crafting | Survival inv, hotbar, crafting grid, recipes |
| M7  | Save (McRegion) | Read/write .mcr, new-world / save / load |
| M8  | Entities + AI | Mobs + item entities |
| M9  | Audio | Positional OGG, music stubs |
| M10 | Multiplayer | Full b1.7.3 client protocol |
| M11 | Nether | Hell biomes, portals, ghast, zombie pigman |
| M12 | Touch UX | D-pad, look stick, hotbar tap, pause menu |
| M13 | Optimization | Frustum culling, greedy meshing, profiler |
| M14 | Polish | Splash, main menu, settings, lang |

## Build
- Workspace at `mc-rs/`
- `cargo test` runs mc-core unit tests
- APK via GitHub Actions `cargo apk build` -> `MinecraftB173Rust.apk`

## Changelog
- `aef24f7` M3e-atlas-fix (1/2): 16x16 atlas + panic catcher in init.
- `2a5497f` M3e-atlas-fix (2/2): Uint32 indices. **Black screen root cause**: the
  48x128x48 super-chunk pushes the per-frame index count past `u16::MAX`, so
  `IndexFormat::Uint16` silently clipped the draw and wgpu presented black.
  Atlas size was a red herring.
- `pending` M12 (WIP): d-pad + look-stick + pause button + hotbar tap on top
  of the M2/M3 baseline. Files: `src/input/{mod,touch_ui}.rs`,
  `src/render/hud.rs`, `lib.rs`. Build status:
  - `cargo test --lib`: 15/15 pass on the host.
  - `cargo build --target aarch64-linux-android --lib`: INTERRUPTED before
    completion. Not verified.
  - APK build on the CI runner: not done.
  Verified against b1.7.3 sources:
    - Hotbar cell count (9), 20 px cell width, 22 px tall, centred horizontally.
      Match against `GuiIngame.java` lines 58-62 (drawTexturedModalRect calls).
    - `moveStrafe` / `moveForward` semantics ({-1,0,1}). Match against
      `MovementInputFromOptions.java` lines 67-83.
    - Jump velocity 8.4 m/s when on_ground. M2 baseline value, preserved.
  UNVERIFIED (no b1.7.3 source exists for these):
    - D-pad layout (centre position, arm length, button radius).
    - Look-stick radius and anchor.
    - Pause button rectangle.
    - Tap-to-jump on empty screen space (M2 behaviour; the guide says
      M12 should add a dedicated jump button, but the source-of-truth
      b1.7.3 PC game has no touch UX at all — Pocket Edition is a
      separate code base not provided here).
  Follow-up before declaring M12 done: cross-check d-pad/look-stick/
  pause-button rectangles against Pocket Edition 0.x or any later
  touch-based Minecraft client. Run a real Android build (`cargo apk
  build --release`) on the CI runner and verify on a phone.
