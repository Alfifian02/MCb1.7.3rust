## Starling techniques to port (plan, 2026-11-07)

Part of [`ROADMAP.md`](../ROADMAP.md); goes into `roadmap/performance.md`.

Source: Starling / Horizon Android (MIT, `docs/android-performance.md`, `docs/graphics.md`, `crates/horizon-client-render/src/gfx/`). Nothing here ports the engine: Starling is tied to Minecraft 26.3 (`gfx.rs` alone is ~10,000 lines). Only techniques are taken, one per session, each measured before and after. Credit Starling/Horizon in `THIRD_PARTY_NOTICES` for anything that is copied rather than re-derived (`../THIRD_PARTY_NOTICES.md` exists since Phase 2: it lists each copied piece, where it went and what was changed).

### Progress (updated 2026-11-07, Phase 2)

Status words: `done` = code written; `verified` = measured on a device; `skipped` = left out on purpose. Nothing below is compiled or measured yet (no toolchain, no device in the session).

| Phase | # | Item | Status | Note |
|---|---|---|---|---|
| 0 | 1-3 | Recorder, fixed scenario, GLES/Vulkan baseline | skipped | Owner chose to go straight to Phase 1 after playing Starling; no baseline numbers, so no change can be called an improvement yet. |
| 0 | 4 | Audit of `gpu/` and `render/` against phases 1-2 | done for 1-2 | Read from the code (see "Phase 1 result", "Phase 2 result"). The one question the code cannot answer: what the 14 ms of CPU in the GLES shot is made of (see Open questions). |
| 1 | 1 | Sky after solid terrain | already there | Both renderers; nothing changed. |
| 1 | 2 | Solid without `discard`, cutout in its own layer | already there | Both renderers; nothing changed. |
| 1 | 3 | Group draws by page in 64-block distance bands | done 2026-11-08, not verified | Copied from Starling's `gfx/page_runs.rs`: `world/chunks/page_runs.rs` (`PageRuns`, `BAND`), called from `ChunkManager::for_each_run`. **Correction:** this row and "Phase 1 result" said it was done on 2026-11-07, with `band_breaks` and `ChunkManager::bands`; none of that was in the code (checked when the Phase 2 patch was made). Keep only if `set_vertex_buffer` calls or GPU ms move. |
| 2 | 4 | Frame-aware upload budget | done, not verified | Copied from Starling: `embeddium/src/upload_budget.rs` `frame_state_limits`, called from `ChunkManager::mesh_pending`; `ChunkManager::update` takes `interactive`, `lib.rs` passes it. |
| 2 | 5 | Cave culling off the render thread, partial update | done, not verified | The walk already ran on a worker (`wide.rs`, Sodium's `embeddium::cull`). The owner asked for Starling's partial update, which only works on Starling's own search, so `world/chunks/occlusion.rs` is a copy of Starling's `gfx/occlusion.rs` and replaces `wide.rs`. **What is culled changes** (vanilla's rules instead of Sodium's; see "Phase 2 result"). |
| 2 | 6 | Reuse empty mesh buffers | done, not verified | Copied from Starling: `MeshPool`, `Job::Free`, `ChunkManager::free`, `pack_reusing` / `build_job_reusing` in `world/chunks.rs`. |
| 2 | 6 | Arena pages grow in steps of at most 32 MiB | not applicable | mc-rs pages are fixed, 12 MiB (`PAGE_VERTS`), and never grow, so no step exceeds 32 MiB. |
| 2 | 7 | Write unloaded chunks from a background thread | already there | `Writer` (thread `chunk-io`); `load` looks at `inflight` first. |
| 2 | 7 | Tick mobs and dropped items only inside the simulation distance | not ported | mc-rs has no simulation distance; owner decision needed (see below). |
| 3 | 8 | Flat-index batching | done, off by default, not verified | Copied from Starling's `gfx/flat.rs` / `flatten.wgsl`: `mc-rs/src/gpu/flat.rs`, `flatten.wgsl`. Vulkan only; `flatDraws:1` (past 512 sections) or `flatDraws:2` (every frame) in options.txt. See "Phase 3 #8 result". |
| 3 | 8 | Flat-index batching on the OpenGL ES renderer | done 2026-11-08, off by default, not verified | `mc-rs/src/gles/flat.rs`; ES 3.1 compute + a uniform-block origin table. See "Phase 3 #8 result, GLES". |
| 3 | 9 | Thermal hints | not started | Next patch: Starling's `android/device_runtime.rs` (ADPF). |

**Switches (2026-11-08):** every ported technique with a sensible \"off\" has a button in Settings > Performance (Optimizations: Cave Culling, Draw Bands, Upload Pacing, Mesh Reuse; Next start: Flat Draws, Frame Latency), so the \"one change, measure before and after\" rule can be followed on a device without editing options.txt. See the changelog of `performance.md`.

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
| 3 | **Changed (2026-11-08; the 2026-11-07 text described code that was never there).** `for_each_run` went page by page, each page near to far, so a far chunk in page 0 was drawn before a near one in page 1. It now walks `order` once, near to far (`order` keeps each chunk's squared distance, `update_visible`), and regroups the runs by (band of 64 blocks, page) with Starling's `PageRuns` (stable sort, `BAND`, the one-page shortcut): one `set_vertex_buffer` per page and band. Both renderers share it. Water is unchanged (far to near across pages). **Copied from Starling's `gfx/page_runs.rs`** (see `THIRD_PARTY_NOTICES`); the list `for_each_run` builds holds plain values (page, slot, origin, first, end), looked up again when handed out. UNVERIFIED: that it compiles; the number of `set_vertex_buffer` calls (not counted anywhere yet). Tests: Starling's three `page_runs` tests, adapted. |

Still to do on a device: paired screenshots are not needed for 3 (opaque draws write depth, sections share no faces; only a tie between equal-depth fragments could differ), but the rule stands: measure `set_vertex_buffer` calls per frame and GPU ms before and after on the same scenario, and drop the change if the numbers do not move. Open: with one page (small render distance) the change is a no-op.

### Phase 2: stutter and CPU

4. **Frame-aware upload budget.** Halve the byte budget and cut the time budget when the last frame passed ~18 ms or the camera/menu is being touched; clamp to ~1 MiB and 1 ms past ~22 ms (Starling `upload_budget.rs`: 2-4 MiB and 2-3 ms otherwise). mc-rs already has a Sodium-style budget; add the frame-state input.
5. **Cave culling off the render thread.** Run the `embeddium::cull` walk on a worker over its own copy of the visibility data; the frame uses the last finished result and a section meshed in between propagates immediately. Today it runs once per frame on the render thread (CPU was 14 ms in the GLES shot).
6. **Memory behaviour on Android.** Reuse empty mesh buffers (Starling caps this at 16 MiB per pool) and grow arena pages in steps of at most 32 MiB on 64-bit.
7. **Background I/O and ticking.** Write unloaded chunks from a background thread (loads must see queued records first); tick mobs and dropped items only inside the simulation distance, outside it only check despawning.

#### Phase 2 result (2026-11-07; read from the code and written WITHOUT a Rust toolchain: not compiled, not run, not measured)

Audit of `world/chunks.rs`, `world/chunks/wide.rs` (removed since), `embeddium/src/{upload_budget,cull,arena}.rs`, `world/mobs.rs`, `world/items.rs` and `lib.rs` against Starling's `gfx/upload_budget.rs`, `gfx/occlusion.rs`, `gfx/arena.rs`, `streaming.rs`, `mesher.rs` and `game/creatures.rs`. Every piece of code that was added is a copy of Starling's, adapted only where the types differ; `../THIRD_PARTY_NOTICES.md` lists them.

| # | Result |
|---|---|
| 4 | **Done, not verified.** `frame_state_limits(duration, size, frame_ns, interactive)` is Starling's `with_frame_state`: past 22 ms the budget is 1 ms and at most 1 MiB; past 18 ms, or while `interactive`, at most 2 ms and `MAX_UPLOAD / 2` (1 MiB); otherwise the size is capped at 2 MiB. `ChunkManager` feeds it the last frame's length (`last_frame_ns`, measured in `update`; the 5% average the Sodium budget uses is too slow to react) and `interactive` = menu or screen open, or a finger on the look area (`touch.pointers`, `PointerRole::Look`). **Changed from Starling:** a smooth frame keeps mc-rs's Sodium budget (30% of the frame, at least 2 ms); Starling sets 2 ms there. **Consequence to know:** mc-rs's size limit is already 1 MiB (`UPLOAD_SIZE_LIMIT`), which is what Starling's tiers give, so the tiers cut the *time* (5 ms to 2 ms at 60 Hz, 1 ms when slow) but not the bytes; the plan's "halve the byte budget" would need a size below 1 MiB, which Starling never uses, so it was not done. The first mesh of a frame is always uploaded, as before. Not counted as touching: the gamepad's look stick, a mouse. |
| 5 | **Done, not verified (copied, and it replaces the old walk).** Before: `WideCull` ran the `embeddium::cull` walk on its own thread over an `Arc` snapshot and the frame used the last finished walk, so the plan's "today it runs once per frame on the render thread" was out of date; what was missing was the partial update (a section meshed between two full searches propagates into the graph the frame has, so what it opens shows at once; what it closes goes with the next search). Starling's partial update (`Occlusion::propagate_changes`) works on its own search, not on Sodium's walk, so the whole of `gfx/occlusion.rs` was copied to `world/chunks/occlusion.rs` (search, worker, partial update, all its tests) and `wide.rs` and its five tests were removed. Changes to the copy: the `vis` layout (`face * 8`, `ALL_VISIBLE` = `embeddium::vis::FULL`), two `let` chains and an `is_none_or` written out for edition 2021, `SectionKey` defined in the file (`THIRD_PARTY_NOTICES.md` has the list; a `diff` against Starling shows nothing else). Wiring in `ChunkManager`: field `occlusion` (was `wide` + `graph`); `update_visible` calls `occlusion.update(eye, rh / 16 + 2)` and, for each loaded chunk that passes the range and frustum test of its box, keeps the sections for which `occlusion.contains((x, y, z))` holds (then the per-section range and frustum test as before); `mesh_pending` calls `set` for each of the 24 sections when a mesh brings other `vis` (`set` ignores one that did not change); a chunk that loads or unloads calls `remove_column`. **What changes on screen, to look for on a device:** (1) the rules are vanilla's: a section is led on through every face it was entered through, and beyond 60 blocks a step is taken only if the line back to the camera stays in reached sections (Sodium's walk has no such test, so sections around the edge of a wall or a hill that the old walk showed can go); a section hidden wrongly would show as terrain missing at cave mouths and hill edges, popping in as the camera moves; (2) a column with no mesh (not loaded, or not meshed yet) lets the view through, where the old walk stopped at a hole in the loaded ring; (3) until the first search has landed, or when the eye is somewhere the last graph never reached (a teleport), the frame culls by frustum alone for a frame or two, where the old code walked in the frame; (4) the search box is square, `rh` blocks and two chunks (the shape and `rv` are tested per section after it); above the highest non-air section, open sky is kept whole. `embeddium::cull` is no longer called by mc-rs (the module stays in the crate). |
| 6 | **Mesh buffers: done, not verified.** What crosses threads is `Built::verts` (a `Vec<Packed>`): a worker built it, the frame uploaded it and freed it. Now the frame sends the uploaded ones (and those of chunks unloaded meanwhile) to a worker in one `Job::Free` per frame; the worker puts them in `MeshPool` (Starling's caps: 128 buffers, 16 MiB, a rejected one is dropped outside the lock); the next `Job::Mesh` takes one and packs into it (`pack_reusing`, `clear()` first, the old capacity estimate kept as a `reserve`). The mesher's own scratch (`build_split_tinted`'s three vectors) is built and dropped on the worker and is not pooled. **Changed from Starling:** the job queue here is first in, first out, so a free waits behind queued jobs (Starling's goes first); the buffers wait in the channel meanwhile (a few per frame, never more than the uploads of the frames since the workers last looked). **Arena growth: not applicable.** Starling caps a growing page at 32 MiB on 64-bit Android; mc-rs pages are 12 MiB and fixed (`Arena::grow` is unused, by design: the shared index buffer names vertices by their absolute number in the page, so `draw_indexed` needs no `base_vertex`, which a GLES 3.0 fallback lacks; the price is no compaction). |
| 7 | **Writes: already there.** `Writer` writes chunk files on thread `chunk-io` (atomic `save::write`), in queue order, and keeps the newest queued bytes of each chunk in `inflight` until its write is answered; `load` reads `inflight` before the file, which is "loads must see queued records first". Failed writes mark the chunk dirty again. **Not ported, and not asked for by the plan's wording:** the encoding (`save::encode_chunk`) and the reads of loads still run on the render thread (`LOADS_PER_FRAME`, a ponytail in the code); Starling does both on workers. Moving the encoding changes what `inflight` can hold (bytes), so it is a redesign, not a copy. **Ticking inside the simulation distance: not ported.** Starling's `creatures.rs` skips `tick` for a mob outside the entity-ticking area and only runs `despawn_check`, with the area coming from its chunk-ticket system. mc-rs has no simulation distance and no tickets; its mobs tick wherever the chunk is loaded and already despawn by distance (`Mob::old`: beyond 128 blocks, or 1 in 800 per tick past 32), and a dropped item ages 6000 ticks (`LIFE`). A distance under 128 blocks would freeze mobs and item ages that b1.7.3 runs, so it needs a decision on the value and on whether the parity cost is worth it; above 128 it changes nothing. No cost has been measured for mob or item ticking. |

Not copied because the plan did not ask for it: Starling's `IDLE_RELEASE` (a worker idle for 5 s drops its scratch buffers and the last one hands freed memory back to the OS).

On a device, in this order: (a) `slow chunk work` and the frame-time tail while flying at a larger render distance, before and after items 4 and 6 (one change per session; these two are in one session here, so if the numbers move, switch one off to tell which); (b) heap size over ten minutes (the pool holds at most 16 MiB); (c) whether the longer wait of a free in the first-in-first-out queue ever fills the pool slower than the meshes drain it (count `Mesh` jobs that found the pool empty).

### Phase 3: risky, decide with data

8. **Flat-index batching** (compute pass builds one index list; one draw per page run). Starling measured 46.05 to 22.30 ms GPU at 2560x1600, 16 chunks, but against per-section indirect draws. mc-rs draws runs directly, so the gain is unproven. Needs storage buffers in the vertex shader; check Adreno 610 limits. Starling only enables it past 512 sections.
9. **Thermal hints** (ADPF `PerformanceHintManager`) to keep clocks steady. Measure over 10+ minutes, not 60 s.

#### Phase 3 #8 result (2026-11-08; written WITHOUT a Rust toolchain: not compiled, not run, not measured)

What it does: with `flatDraws` on, `Flat::prepare` (before the scene pass, in `Wg::draw_world`) walks the same `for_each_run` the per-chunk loop walks, for the three layers, and a compute pass (`flatten.wgsl`) copies the runs' indices into one list; the layers are then drawn from it with one `draw_indexed` per run of one page (`Flat::draw`), through `vs_chunk_flat` / `vs_chunk_water_flat`, which find a chunk's origin from the vertex number: `slot_origins[block_slots[vertex >> 8u]]`. Off, nothing is drawn differently (the shader refactor is `chunk_vs(w, origin)`; the pipelines, the allocation and the draw loop are the same).

What it needed from mc-rs: pages hold a block table (`Page::blocks`, one origin slot per 256 vertices); `Pages::put` rounds a mesh up to whole blocks when flat draws are on (a few hundred spare vertices a chunk); `DrawOrigins` mirrors every origin into a storage buffer (`flat_buffer`).

What was left out of Starling's file, and why: phase 2 / GPU occlusion culling (mc-rs has none; Starling disables it on Android), shader packs and the shadow camera, the sorted-index source (mc-rs's water is quads of the shared index buffer), `for_vanilla` and the Intel Gen11/12 workaround (desktop). Starling's own default on Android Vulkan is OFF "to prevent chunk displacement" (`HORIZON_FLAT_DRAWS=1` turns it on); that is why this one is off too. Its 46.05 to 22.30 ms is against per-section indirect draws; mc-rs's baseline is one `draw_indexed` per run of a chunk plus one origin bind per chunk, so the gain is unproven. Gating: `DownlevelFlags::VERTEX_STORAGE | COMPUTE_SHADERS`, 2+ storage buffers per stage, the default storage binding size.

To measure (Phase 0 is still skipped, so there is no baseline): the same spot and view distance with `flatDraws:0`, `1` and `2`; compare draw calls and `set_bind_group` calls per frame, CPU ms in `render`, and GPU ms. **The GPU timer times the scene pass only** (`GpuTimer`), so the compute pass is not in `Drs`'s ms: for an honest A/B take the frame time, not that number.

UNVERIFIED, all of it: that it compiles; `naga` accepting the vertex entry points' storage bindings next to the `draw` uniform (they use different bindings, 1 and 2 against 0); that the picture is pixel-identical to the per-chunk draws (the `flatten` test checks the indices against the shared index buffer, on a GPU or lavapipe, skipped without one); that Adreno 610's Vulkan driver is happy with two read-only storage buffers in the vertex stage; that a page's bind group follows a page that was dropped and its id never reused (ids only grow).

Tests added: `plan_draws_runs_of_a_page_in_order`, `plan_skips_empty_layers`, `flatten_writes_the_indices_of_the_per_chunk_draws` (from Starling's `plan` / `flatten` tests, adapted), `flat_meshes_take_whole_blocks`, `flat_draws_option_is_off_by_default_and_round_trips` (mine); the shader test now also checks `vs_chunk_flat` and `vs_chunk_water_flat`.

#### Phase 3 #8 result, GLES (2026-11-08; written WITHOUT a Rust toolchain: not compiled, not run, not measured)

What it does: with `flatDraws` on and a context that can, `gles::flat::Flat::prepare` (before the scene pass, in `Renderer::draw_world`) walks the same `for_each_run` for the three layers, `plan` (shared with `gpu::flat`) lays out the calls, and a compute pass (`FLATTEN_CS`, GLSL ES 3.10, `flatten.wgsl` line for line) writes one index list; `Renderer::flat_runs` draws it with one `DrawElements` per run of one page, binding the page's vertex buffer (the list is the element buffer of its VAO) and its block table once per page. No `Uniform3f` per chunk.

How it differs from the Vulkan one, and why:

| | Vulkan (`gpu/flat.rs`) | GLES (`gles/flat.rs`) |
|---|---|---|
| Needs | vertex storage, compute, 2 storage buffers per stage | ES 3.1 context (`glDispatchCompute`, `glMemoryBarrier`), `MAX_UNIFORM_BLOCK_SIZE` >= 64 KiB |
| Origin lookup | `slot_origins[block_slots[vertex >> 8]]`, storage buffers | `blocks[gl_VertexID >> 8].xyz`, one uniform block `B` (point 2), a `vec4` origin a block (4096 blocks, 64 KiB a page) |
| Why | storage buffers are core | ES 3.1 lets a driver have no vertex-stage storage blocks; uniform blocks it always has. One level: a slot table would be a second 64 KiB block |
| Context 3.0 / no compute | n/a | `Gl::flat` = 0, per-chunk draws, logged |

What `Pages::put` does for GL now: rounds a mesh up to whole `GRAIN` blocks and writes the chunk's origin into every block of it (`Page::blocks`, 16 bytes a block) when `Gl::flat` is on. `Gl::flat` is read at context creation from `flatDraws`; if the flat programs then fail to build, pages stay in flat layout (a few hundred spare vertices a chunk) and the per-chunk draws are used.

Left out: GPU occlusion culling, shader packs, sorted translucent indices (as in the Vulkan port). Unlike Vulkan, no indirect draws: ES 3.1's `DrawElementsIndirect` has no multi-draw, so one draw per page run it is.

UNVERIFIED, all of it: that it compiles; that a driver accepts `gl_VertexID` as an index into a uniform array in the vertex shader of the compact chunk vertex (it should, ES 3.0 allows dynamic indexing of uniform blocks); that `glDrawElements` over a buffer the compute pass wrote needs only `ELEMENT_ARRAY_BARRIER_BIT`; **that the dispatch before the scene pass does not cost a tile-memory flush or a stall on Adreno / Mali** (the one real risk of putting compute in a GLES frame: the driver may serialise the previous frame's reads of the list); that the picture is pixel-identical to the per-chunk draws. Origins are f32 of `chunk * 16`, the same value the per-chunk path sends as `u_origin`, so the maths is the same.

To measure (Phase 0 is still skipped): the same spot and view distance with `flatDraws:0`, `1`, `2` on GLES; count `DrawElements` and `Uniform3f` calls per frame (not counted anywhere yet), CPU ms in `draw_world`, frame time (the GL path has no GPU timer for the compute pass).

Tests added (CPU only): `flatten_shader_matches_the_shared_quad_indices` (the shader's `quad` arithmetic against `embeddium::quad_index`, and the text of the shader), `flat_vertex_shader_reads_the_block_table`.

### Not ported

World generation, protocol, HDR, TAA, shader packs, LOD (mc-rs has its own), GPU occlusion culling (Starling disables it on Android), mods, launcher.

From Phase 2 (reasons in "Phase 2 result"): growing arena pages (mc-rs pages are fixed), the simulation-distance ticking of mobs and dropped items (no such distance in mc-rs; owner decision), moving chunk encoding and loading to workers (a redesign of `inflight`), `IDLE_RELEASE`, pooling of the mesher's own scratch vectors.

### Open questions

- Is the resolution scale the reason mc-rs wins? Run Starling at 67% in the same scene before drawing conclusions.
- Does the 16-byte vs 12-byte vertex difference (Starling's is 24 bytes) show up in GPU time? Measure vertex bytes per frame in both.
- On Adreno 610, does MSAA stay off (`present::weak_gpu`) at every scale?
- What is the 14 ms of CPU in the GLES shot made of? Cave culling was already on a worker (`wide.rs`, now `occlusion.rs`), so it is not that walk; the Phase 0 recorder (skipped) would say. Until then Phase 2 has no target number.
- Simulation distance for mobs and dropped items: is a value below 128 blocks acceptable against b1.7.3 parity? (Item 7, not ported.)
- Is vanilla's occlusion what mc-rs wants? Its rules differ from the Sodium walk it replaced (the 60-block line test removes sections the old walk showed; how the two compare otherwise has not been checked), and b1.7.3 had no cave culling at all. If sections go missing on a device, the old walk is in `world/chunks/wide.rs` of the archive this change was made on (2026-10-23 changelog), without the partial update.
- Should the upload budget's byte limit go below 1 MiB while the frame is late or touched? Starling never does; the first mesh of a frame is always uploaded anyway.
