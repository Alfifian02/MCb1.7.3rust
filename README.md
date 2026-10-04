# mcrs (Step 2)
- `bcore/`  : dependency-free voxel core (chunks, worldgen, mesher). `cargo test -p bcore`
- `bgame/`  : renderer (OpenGL ES 2.0 via glow/glutin/winit). Desktop dev run: `cargo run --release -p bgame`
- `android/`: Gradle shell (NativeActivity, no Java). APK is built by `.github/workflows/android.yml`

Push to GitHub -> Actions tab -> "Build APK" -> download artifact `bgame-apk` -> install.
Controls: left half of screen = move stick (drag), right half = look (drag). Fly camera for now.

## Textures
`bgame/assets/terrain.rgba` is converted from a Faithful 32x `terrain.png` by `tools/convert_atlas.py`
(grass/leaves are pre-tinted). Faithful is derived from Mojang's art, so keep the APK for personal use
and do not publish it. To swap packs: `python3 tools/convert_atlas.py <terrain.png> bgame/assets/terrain.rgba`.
