# PGO profile

`recon.profdata` is not committed yet, so release builds currently run without PGO. When the file exists in
this folder, release builds use it automatically.

The profile has to be regenerated after any change to `core/` and when the NDK version changes. `core/` was
updated to the SeedlessDS 1.0.2 core, so a profile made before that would be stale for the 2D compositor, the
3D rasterizer bands and the pacer.

## Generate

1. `./gradlew :app:assembleRelease -PseedlessPgoMode=GENERATE`, then install the APK. Use a Release build;
   Debug skips PGO.
2. Play 3-5 different games for 10+ minutes each (include one 3D-heavy and one 2D-heavy title), then close
   each game normally. The profile is written on close.
3. `./tools/pgo/collect.sh <ndk-dir>` with the same NDK that built the APK (r25c, `25.2.9519653`).
4. Commit `pgo/recon.profdata`.

The GitHub workflow can build step 1: run "Android build" with `pgo_mode = GENERATE`.

## Optional stopgap

`core/pgo/android-arm64-clang14.profdata` ships with the 1.0.2 core (indexed profile, clang 14 = NDK r25c).
It was recorded against the upstream code, not this fork, so functions the fork changed (frame stats, thread
registry, async save, thermal cap) fall back to no profile data. It has not been measured on this fork. To try
it without committing anything:

    ./gradlew :app:assembleRelease -PseedlessPgoProfile="$(pwd)/core/pgo/android-arm64-clang14.profdata"

Use an absolute path: CMake checks it from the build directory.

Compare it against a build with `-PseedlessPgoMode=OFF` on the same device and game before keeping it.
