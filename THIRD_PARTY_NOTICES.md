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
