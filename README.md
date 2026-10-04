# mcrs (Step 3)
- `bcore/`  : dependency-free core: chunks, worldgen, mesher, player physics, voxel raycast. `cargo test -p bcore`
- `bgame/`  : engine + renderer (OpenGL ES 2.0). `cargo test -p bgame`; desktop dev run: `cargo run --release -p bgame`
- `android/`: Gradle shell (NativeActivity, no Java). APK is built by `.github/workflows/android.yml`

Push to GitHub -> Actions -> "Build APK" -> download artifact `bgame-apk` -> install.

## Controls
| Action | Gamepad | Touch | Desktop test keys |
|---|---|---|---|
| Move | Left stick | Drag left half | WASD |
| Look | Right stick | Drag right half | Arrow keys |
| Jump / fly up | A | White button | Space |
| Sneak / fly down | B | - | Shift |
| Sprint | L3 (latches) | Push stick fully | R |
| Break (hold) | RT / R2 | Red button | LMB / Z |
| Place (hold) | LT / L2 / X | Green button | RMB / X |
| Hotbar prev/next | LB/RB, D-pad left/right | Tap a slot | 1-9 |
| Toggle fly | Y / D-pad up | Cyan button | F |

Gamepad: deadzones, quadratic look curve, analog triggers or trigger buttons, hat-axis or key D-pad.
The touch overlay hides while a gamepad is in use and returns when you touch the screen.
Remap in `bgame/src/gamepad.rs` (`sample`) and `bgame/src/android.rs` (`pad_button`).

## Textures
`bgame/assets/terrain.rgba` is converted from a Faithful 32x `terrain.png` by `tools/convert_atlas.py`.
Faithful is derived from Mojang's art: keep the APK for personal use, do not publish it.
