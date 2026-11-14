# Third-party notices

Code in this repository that was copied (and then adapted) from another project is listed here, with that project's license.
Techniques that were only re-derived from an idea, with no code copied, are not listed. The ports of C2ME, Embeddium / Sodium,
ScalableLux and the others keep their own `COPYING*` files in their crates (`embeddium/`, `starlight/`).

## Starling / Horizon Android

Source: `starling-android` (Horizon, `crates/horizon-client-render`), MIT. The plan is `roadmap/starling-port-plan.md`.

What was copied, where it went, and what was changed:

| From Starling | In this repository | Changed |
|---|---|---|
| `gfx/upload_budget.rs`, `UploadBudget::with_frame_state`: the 22 ms / 18 ms / interactive tiers and their 1 MiB, 1 ms, 2 ms, `max_upload` constants | `embeddium/src/upload_budget.rs`, `frame_state_limits` | `Duration` became nanoseconds. A smooth frame keeps this repository's Sodium budget (Starling sets 2 ms); the slow tiers take the smaller of the two and never raise a value. |
| `streaming.rs`, `MeshPool` (`take`, `put`, the 128 count and 16 MiB caps, a rejected mesh handed back to be dropped outside the lock) | `mc-rs/src/world/chunks.rs`, `MeshPool` | Pools `Vec<Packed>` (the vertex buffer of `Built`), not Starling's whole `SectionMesh`. Starling's Android-specific cache budget (`android_runtime::mesh_cache_budget`) is not used: the 16 MiB is its default. |
| `streaming.rs`, `Job::Free` and `Streaming::free` | `chunks.rs`, `Job::Free`, `ChunkManager::free` | The job queue here is first in, first out, so a free waits behind the jobs already queued; Starling's goes to the front. |
| `mesher.rs`, `Mesher::mesh` / `Mesher::mesh_reusing` (the split, and `out.clear()` first) | `chunks.rs`, `pack` / `pack_reusing`, `build_job` / `build_job_reusing` | `pack` and `build_job` are kept for the tests only (`#[cfg(test)]`). The capacity estimate the old `Vec::with_capacity` had is kept, as a `reserve`. |
| `streaming.rs` tests `mesh_pool_reuses_capacity_with_clean_geometry_state`, `mesh_pool_caps_retained_count_and_bytes` | `chunks.rs`, the tests of the same names | Rewritten for `Vec<Packed>`. |
| `gfx/occlusion.rs`, the whole file: `Columns`, `Graph` (`search`, `seed_outside`, `propagate`, `sees_camera`, `walk`), the search `Worker`, and `Occlusion` with its partial update (`propagate_changes`), and every test in it | `mc-rs/src/world/chunks/occlusion.rs` | The `vis` bit layout: this repository's `embeddium::vis` is `from * 8 + to` (Sodium's), Starling's is `from * 6 + to` (vanilla's), so the face's six bits are read at `face * 8`, `ALL_VISIBLE` is `embeddium::vis::FULL`, and the tests' `connects` and `reference` use `* 8` too. Two `let` chains and an `is_none_or` are written out (edition 2021, Starling's is 2024). `SectionKey` is defined here, not imported; one log line says `occlusion:` instead of `Horizon:`; `clear` carries `#[allow(dead_code)]` (a `ChunkManager` is made per world). The rest is as copied (`diff` against the source shows only these). |
| `gfx/flat.rs`: the file's structure and code: `Call`, `plan` (draw calls as runs of one page, ranges in draw order), `storage` / `buffer`, `Flat::new`, `Flat::prepare` (index and range buffers grown to a power of two, the compute pass, a bind group per page), `Flat::draw`, `wanted` / `supported`, `MIN_SECTIONS`, and the `plan` and `flatten` tests | `mc-rs/src/gpu/flat.rs` | wgpu 26 (`bind_group_layouts: &[&..]`, `push_constant_ranges`), not Starling's newer API. A draw is a chunk run of the shared quad index buffer (`Run`: base vertex = `first / 6 * 4`), not a section's `DrawIndexedIndirectArgs`; the layers (solid, cut-out, water) replace phase 1 / phase 2 / shadow calls in `plan`. A page's block table holds the origin slot of the chunk (`DrawOrigins`), written when a mesh is put, not an id per section rebuilt each frame; the origins are the world-space ones the per-chunk draws use. Left out: phase 2 (`flatten_seen`, `draw_phase2`, the phase 2 call buffer), shader packs (`draw_with`, `draw_geometry`, the shadow calls), the sorted-index source and its `empty` buffer, `Range.first`, the range padding for segment offsets, `for_vanilla`, `is_broken_intel_vulkan`, the `HORIZON_FLAT_DRAWS` environment variable (`flatDraws` in options.txt). Binding 1 and 2 of group 1 instead of 0 and 1, so the per-chunk shader keeps its `draw` uniform at 0. |
| `gfx/page_runs.rs`: `BAND`, `PageRuns::push` (the key `band << 32 \| page`), `PageRuns::regroup` (stable sort, the one-page shortcut), and its three tests | `mc-rs/src/world/chunks/page_runs.rs`, called from `ChunkManager::for_each_run` | `regroup` takes one slice of whatever a draw is (Starling's are the draws and their pages, two parallel arrays) and returns nothing; `Run.tag`, `clear` and the scratch buffer kept in the struct are gone. The distance comes from `ChunkManager::order`, which now keeps each chunk's squared distance. |
| `gfx/flatten.wgsl`, `Range`, `flatten`, `quad` | `mc-rs/src/gpu/flatten.wgsl` | `quad` follows `embeddium::quad_index` (v, v+1, v+2, v+2, v+3, v), not 0 1 2 0 2 3. `sorted`, `QUADS`, `flatten_seen` and its bindings removed. |
| `terrain.wgsl`, `vs_terrain_flat`: `slot_origins[block_slots[vertex >> 8u]]` | `mc-rs/src/gpu/pipeline.rs`, `flat_origin`, `vs_chunk_flat`, `vs_chunk_water_flat` | The per-chunk vertex code became `chunk_vs` / `chunk_water_vs` over an origin argument, so both entry points share one body. |
| `gfx/arena.rs`, `GRAIN` (ranges start at multiples of it and fill whole blocks) | `mc-rs/src/gpu/flat.rs` `GRAIN`, `Pages::put` | Only when flat draws are on: the length handed to the arena is rounded up to `GRAIN`. `embeddium::arena` is unchanged. |
| `gfx/flat.rs` and `gfx/flatten.wgsl`, again, for the OpenGL ES renderer: `plan`, the compute pass, the block-table idea | `mc-rs/src/gles/flat.rs` (`Flat`, `FLATTEN_CS`), `shaders.rs` (`chunk_with`, `FLAT`) | `plan`, `Draw`, `Call` are `gpu/flat.rs`'s, shared. The compute pass is `flatten.wgsl` rewritten in GLSL ES 3.10 (`std430` buffers, a `u_count` uniform instead of `arrayLength`). A vertex reads its chunk's origin from a **uniform block** (`vec4 blocks[4096]`, one origin a block, written by `Pages::put`), not from storage buffers (ES 3.1 lets a driver have none in the vertex stage), so there is no slot table. The list is also the element buffer. |

Tests that are this repository's own, not Starling's, say so in their doc comment (`slow_frames_and_touch_shrink_the_budget_and_a_smooth_frame_keeps_it`,
`a_recycled_buffer_packs_the_same_quads`).

Starling's license:

```
MIT License

Copyright (c) 2026 Ludovic Marie and the Horizon contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
