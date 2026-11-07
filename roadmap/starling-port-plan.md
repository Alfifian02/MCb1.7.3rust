## Starling techniques to port (plan, 2026-11-07)

Part of [`ROADMAP.md`](../ROADMAP.md); goes into `roadmap/performance.md`.

Source: Starling / Horizon Android (MIT, `docs/android-performance.md`, `docs/graphics.md`, `crates/horizon-client-render/src/gfx/`). Nothing here ports the engine: Starling is tied to Minecraft 26.3 (`gfx.rs` alone is ~10,000 lines). Only techniques are taken, one per session, each measured before and after. Credit Starling/Horizon in `THIRD_PARTY_NOTICES` for anything that is copied rather than re-derived.

### Progress (updated 2026-11-07)

Status words: `done` = code written; `verified` = measured on a device; `skipped` = left out on purpose. Nothing below is compiled or measured yet (no toolchain, no device in the session).

| Phase | # | Item | Status | Note |
|---|---|---|---|---|
| 0 | 1-3 | Recorder, fixed scenario, GLES/Vulkan baseline | skipped | Owner chose to go straight to Phase 1 after playing Starling; no baseline numbers, so no change can be called an improvement yet. |
| 0 | 4 | Audit of `gpu/` and `render/` against phases 1-2 | partly done | Phase 1 only (see "Phase 1 result"); the Phase 2 questions (cave culling cost, upload budget inputs) are still open. |
| 1 | 1 | Sky after solid terrain | already there | Both renderers; nothing changed. |
| 1 | 2 | Solid without `discard`, cutout in its own layer | already there | Both renderers; nothing changed. |
| 1 | 3 | Group draws by page in 64-block distance bands | done, not verified | `world/chunks.rs`: `BAND`, `band_breaks`, `for_each_run`. Keep only if `set_vertex_buffer` calls or GPU ms move. |
| 2 | 4-7 | Upload budget input, cave culling off-thread, Android memory, background I/O | not started | |
| 3 | 8-9 | Flat-index batching, thermal hints | not started | Decide with data. |

### What the screenshots say (same phone, Adreno 610, render distance 8)

| | Starling 26.3 | mc-rs b1.7.3 |
|---|---|---|
| FPS | 43-46 | 47-59 |
| GPU ms | 21.7-23.3 | 8.5-9.2 |
| CPU ms | 7.8-9 | 8-14 |
| Scene size | 1604x720 | 1075x482 (67%) |

mc-rs draws ~2.2x fewer pixels and its GPU time is ~2.5x lower, so GPU cost per pixel is about equal. The scenes differ (grassland vs beach), Starling was hot (CPU 72 C, GPU 79 C, CPU 1536/2112 MHz) and vsync-capped at 60, and mc-rs ran GLES in one shot and Vulkan in two. This is not a controlled benchmark.

mc-rs already has most of the same techniques (arena, 12-byte vertex, Sodium-style culling, upload budget, dynamic resolution, worker meshing). The gap is measurement and a few Adreno-specific details. **UNVERIFIED:** the mc-rs `gpu/` and `render/` code was not audited for this plan; phase 0 does that.

### Rules

- One change per session. Measure with the same scenario before and after.
- A change that does not move the numbers is dropped.
- Compare GLES and Vulkan separately; never average them.
- Check thermal status before every run; wait until it is below moderate.
- Keep recording off in the final comparison (recording adds overhead).

### Phase 0: measure and audit (required first)

1. **Recorder.** Per-second summary: FPS, frame/CPU/GPU ms, p99 and worst, meshing and upload queue lengths, uploaded bytes, draw calls, resident sections, scene size, thermal status (Android 10+ `getCurrentThermalStatus`). Starling's format: `docs/android-performance.md`.
2. **Fixed scenario.** Same seed, position, camera path (hold still + a 360 degree sweep + 10 s forward flight), resolution, render distance. Reject runs with thermal status 2 or higher.
3. **Baseline.** Run it on GLES and on Vulkan; save the numbers in this file.
4. **Audit** `gpu/` and `render/` against phases 1-2: is the sky drawn before terrain? does the solid layer discard? are draws grouped by page? what does cave culling cost per frame?

### Phase 1: GPU (cheap, safe)

| # | Change | Starling evidence | Check in mc-rs first |
|---|---|---|---|
| 1 | Draw the sky after solid terrain, reading depth at the far plane without writing it | GPU -8% stationary, -19% turning (Adreno 710, 2560x1600) | Does `sky.rs` draw first? Does the MSAA scene pass allow it? |
| 2 | Solid terrain without `discard`; cutout in its own layer | GPU -25 to -27% (Adreno 710) | Roadmap says `solid` has no `discard`; verify on GLES too |
| 3 | Group draws by arena page within 64-block distance bands | Fewer vertex-buffer switches; keeps front-to-back order | Count `set_vertex_buffer` calls per frame |

Sky and terrain must stay pixel-identical: capture paired screenshots (day, sunset, night, sunrise) and diff them.

#### Phase 1 result (2026-11-07; read from the code, nothing compiled or measured)

Audit of `backend.rs`, `gpu/pipeline.rs`, `gles/renderer.rs`, `gles/shaders.rs` and `world/chunks.rs` against Starling's `gfx.rs` (`LateSky`, `opaque_terrain`) and `gfx/page_runs.rs`:

| # | Result |
|---|---|
| 1 | **Already there, both renderers.** `Backend::draw_world` runs `draw_opaque` first, then `sky.draw` (depth-tested LessEqual, no depth write) and `draw_extras`; GLES does the same (`DepthMask(FALSE)`, `LEQUAL` after the cut-outs). The 4x MSAA scene pass allows it. Nothing to change. |
| 2 | **Already there, both renderers.** `fs_solid` calls `frag_chunk(in, false)` (no `discard`); cut-outs are their own layer (`Layer::Cutout`, `fs_cut`, alpha-to-coverage); GLSL `FS_CUT` guards the `discard` the same way. Starling needs a second pipeline pair and an Android-Vulkan-only switch because it had one shader; mc-rs split them from the start. Nothing to change. |
| 3 | **Changed.** `for_each_run` went page by page, each page near to far, so a far chunk in page 0 was drawn before a near one in page 1. It now goes band by band (64 blocks, `BAND`, Starling's width), near to far, and page by page inside a band (`band_breaks`, `ChunkManager::bands`, one pass over `order` when there is one page). Both renderers share it. Water is unchanged (far to near across pages). Re-derived from the idea, no code copied, so no `THIRD_PARTY_NOTICES` entry. |

Still to do on a device: paired screenshots are not needed for 3 (opaque draws write depth, sections share no faces; only a tie between equal-depth fragments could differ), but the rule stands: measure `set_vertex_buffer` calls per frame and GPU ms before and after on the same scenario, and drop the change if the numbers do not move. Open: with one page (small render distance) the change is a no-op.

### Phase 2: stutter and CPU

4. **Frame-aware upload budget.** Halve the byte budget and cut the time budget when the last frame passed ~18 ms or the camera/menu is being touched; clamp to ~1 MiB and 1 ms past ~22 ms (Starling `upload_budget.rs`: 2-4 MiB and 2-3 ms otherwise). mc-rs already has a Sodium-style budget; add the frame-state input.
5. **Cave culling off the render thread.** Run the `embeddium::cull` walk on a worker over its own copy of the visibility data; the frame uses the last finished result and a section meshed in between propagates immediately. Today it runs once per frame on the render thread (CPU was 14 ms in the GLES shot).
6. **Memory behaviour on Android.** Reuse empty mesh buffers (Starling caps this at 16 MiB per pool) and grow arena pages in steps of at most 32 MiB on 64-bit.
7. **Background I/O and ticking.** Write unloaded chunks from a background thread (loads must see queued records first); tick mobs and dropped items only inside the simulation distance, outside it only check despawning.

### Phase 3: risky, decide with data

8. **Flat-index batching** (compute pass builds one index list; one draw per page run). Starling measured 46.05 to 22.30 ms GPU at 2560x1600, 16 chunks, but against per-section indirect draws. mc-rs draws runs directly, so the gain is unproven. Needs storage buffers in the vertex shader; check Adreno 610 limits. Starling only enables it past 512 sections.
9. **Thermal hints** (ADPF `PerformanceHintManager`) to keep clocks steady. Measure over 10+ minutes, not 60 s.

### Not ported

World generation, protocol, HDR, TAA, shader packs, LOD (mc-rs has its own), GPU occlusion culling (Starling disables it on Android), mods, launcher.

### Open questions

- Is the resolution scale the reason mc-rs wins? Run Starling at 67% in the same scene before drawing conclusions.
- Does the 16-byte vs 12-byte vertex difference (Starling's is 24 bytes) show up in GPU time? Measure vertex bytes per frame in both.
- On Adreno 610, does MSAA stay off (`present::weak_gpu`) at every scale?
