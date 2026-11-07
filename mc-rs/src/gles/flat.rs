//! Flat chunk draws for the OpenGL ES renderer: the frame's chunk runs are copied by a compute pass into one index list, in draw order, and
//! drawn with one `glDrawElements` per run of chunks in the same arena page (`Pages` in `world::chunks`), instead of one `DrawElements` and
//! one `Uniform3f` per chunk. The GL twin of `gpu::flat` (Vulkan); both come from Starling's `gfx/flat.rs` (`THIRD_PARTY_NOTICES.md`).
//!
//! What is shared with `gpu::flat`: `plan` (the draw calls and each draw's range of the list), `Draw`, `Call`, `Range`, and the page layout
//! (`Pages::put` gives a mesh whole blocks of `GRAIN` vertices when flat draws are on). What differs, because this is ES 3.0-safe code that
//! takes ES 3.1 only for the one thing that needs it:
//!
//! - The compute pass (`FLATTEN_CS`, `#version 310 es`) is `flatten.wgsl` line for line. It is the only ES 3.1 part, so `Gl::flat` is 0 on a
//!   3.0 context and the per-chunk draws stay.
//! - A vertex finds its chunk's origin in the page's **uniform block** `B` (`blocks[gl_VertexID >> 8]`, a `vec4` per block, written by
//!   `Pages::put`), not in a storage buffer: ES 3.1 lets a driver have no vertex-stage storage blocks, uniform blocks it always has. One
//!   level, not wgpu's block -> slot -> origin: a 4096-slot origin table would be a second 64 KiB block, and the origin is 12 bytes anyway.
//! - The index list is an `ELEMENT_ARRAY_BUFFER` too (one VAO per page and list, `Renderer::bind_vao`), and `MemoryBarrier` makes the
//!   compute writes visible to the index fetch.
//!
//! Left out, as in `gpu::flat`: GPU occlusion culling, shader packs, sorted translucent indices. Off by default (`flatDraws` in options.txt).
use super::sys::*;
use super::{Buf, Gl, Prog};
use crate::gpu::flat::{plan, Call, Draw, Plan, Range, GROUP_LIMIT};
use crate::world::chunks::{ChunkManager, Layer};

/// Blocks of a page (`PAGE_VERTS / GRAIN`; `world::chunks` asserts it): the entries of the block table.
pub const BLOCKS: u32 = 1 << 12;
/// A page's block table: one `vec4` origin per block. 64 KiB, so `Gl::new` asks `MAX_UNIFORM_BLOCK_SIZE` (ES 3.0 only promises 16 KiB).
pub const BLOCK_TABLE_BYTES: u32 = BLOCKS * 16;

/// The longest index list a compute shader may write: `MAX_SHADER_STORAGE_BLOCK_SIZE` is at least 128 MiB on every ES 3.1 device.
const MAX_LIST_BYTES: u64 = 1 << 27;

/// `flatten.wgsl` for ES 3.1: run `r` is (index count, base vertex, first index in the list), and its indices are the quads of the shared
/// quad index buffer (`embeddium::quad_index`: v, v+1, v+2, v+2, v+3, v) offset by the base vertex. One workgroup a run, 64 lanes.
pub const FLATTEN_CS: &str = r#"#version 310 es
layout(local_size_x = 64) in;
layout(std430, binding = 0) readonly buffer Ranges { uint ranges[]; };
layout(std430, binding = 1) writeonly buffer Indices { uint indices[]; };
// A signed uniform: `glUniform1i` is the only setter `sys.rs` lists, and a uint uniform would want `glUniform1ui`.
uniform int u_count;
uint quad(uint k) {
    uint c = k % 6u;
    uint q = (k / 6u) * 4u;
    if (c < 3u) { return q + c; }
    if (c == 5u) { return q; }
    return q + c - 1u;
}
void main() {
    uint r = gl_WorkGroupID.y * 65535u + gl_WorkGroupID.x;
    if (r >= uint(u_count)) { return; }
    uint count = ranges[r * 3u];
    uint base = ranges[r * 3u + 1u];
    uint dst = ranges[r * 3u + 2u];
    for (uint k = gl_LocalInvocationIndex; k < count; k += 64u) {
        indices[dst + k] = base + quad(k);
    }
}
"#;

pub struct Flat {
    /// The compute program (`loc[0]`: `u_count`).
    prog: Prog,
    /// The frame's index list, an SSBO for the compute pass and the element buffer of the draws.
    indices: Buf,
    index_capacity: u64,
    ranges: Buf,
    range_capacity: u64,
    range_data: Vec<Range>,
    draw_data: Vec<Draw>,
    /// This frame's draw calls: solid ones, then cut-out, then water.
    calls: Vec<Call>,
    cutout_calls: usize,
    water_calls: usize,
    /// `prepare` built this frame's list: the draw loop uses it (off: the per-chunk draws).
    active: bool,
    min_sections: usize,
}

impl Flat {
    /// The compute program and the first (small) buffers; an error leaves the per-chunk draws in charge.
    pub fn new(gl: &Gl) -> Result<Self, String> {
        let prog = gl.compute_program(FLATTEN_CS, &["u_count"])?;
        Ok(Self {
            prog,
            indices: gl.buffer(16, &[], DYNAMIC_DRAW),
            index_capacity: 4,
            ranges: gl.buffer(16, &[], DYNAMIC_DRAW),
            range_capacity: 1,
            range_data: Vec::new(),
            draw_data: Vec::new(),
            calls: Vec::new(),
            cutout_calls: 0,
            water_calls: 0,
            active: false,
            min_sections: crate::gpu::flat::min_sections(),
        })
    }

    /// Whether `prepare` built this frame's index list.
    pub fn active(&self) -> bool {
        self.active
    }

    /// The compute program, left current by `prepare` (the renderer's program cache must know).
    pub fn program(&self) -> u32 {
        self.prog.id
    }

    /// The frame's index list as the element buffer of the draws.
    pub fn indices(&self) -> &Buf {
        &self.indices
    }

    /// `layer`'s draw calls.
    pub fn calls(&self, layer: Layer) -> &[Call] {
        match layer {
            Layer::Solid => &self.calls[..self.cutout_calls],
            Layer::Cutout => &self.calls[self.cutout_calls..self.water_calls],
            Layer::Water => &self.calls[self.water_calls..],
        }
    }

    /// Builds the frame's index list from the chunk runs of the three layers (`ChunkManager::for_each_run`, the draws the per-chunk loop
    /// would make) and dispatches the compute pass that fills it. Call it before the scene pass: the dispatch is not part of the tiled pass.
    /// False when the frame is drawn the per-chunk way instead (too few sections, nothing to draw, a page without a block table, a list too
    /// big for a storage block).
    pub fn prepare(&mut self, gl: &Gl, chunks: &ChunkManager) -> bool {
        self.active = false;
        self.calls.clear();
        self.cutout_calls = 0;
        self.water_calls = 0;
        let Some(compute) = gl.compute else { return false };
        if chunks.sections_drawn() < self.min_sections {
            return false;
        }
        let mut draws = std::mem::take(&mut self.draw_data);
        draws.clear();
        let mut layers = [0usize; 2];
        for (n, layer) in [Layer::Solid, Layer::Cutout, Layer::Water].into_iter().enumerate() {
            if n > 0 {
                layers[n - 1] = draws.len();
            }
            // A run of `first..end` index elements of the shared quad index buffer draws the quads from vertex `first / 6 * 4` on.
            chunks.for_each_run(layer, |r| draws.push(Draw { page: r.page, count: r.end - r.first, base: r.first / 6 * 4 }));
        }
        let Plan { cutout_calls, water_calls, total } = plan(&draws, layers, &mut self.calls, &mut self.range_data);
        self.draw_data = draws;
        // Every drawn page needs its block table (a page made before flat draws were on has none), and the list must fit a storage block.
        let all_tables = self.calls.iter().all(|c| chunks.page_blocks(c.page).is_some());
        if total == 0 || !all_tables || total.next_power_of_two() * 4 > MAX_LIST_BYTES {
            self.calls.clear();
            return false;
        }
        self.cutout_calls = cutout_calls;
        self.water_calls = water_calls;
        if self.range_data.len() as u64 > self.range_capacity {
            self.range_capacity = (self.range_data.len() as u64).next_power_of_two();
            self.ranges = gl.buffer(self.range_capacity as usize * 12, &[], DYNAMIC_DRAW);
        }
        if total > self.index_capacity {
            self.index_capacity = total.next_power_of_two();
            self.indices = gl.buffer(self.index_capacity as usize * 4, &[], DYNAMIC_DRAW);
        }
        gl.write(&self.ranges, 0, bytemuck::cast_slice(&self.range_data));
        let count = self.range_data.len() as u32;
        let f = gl.f();
        // SAFETY: the compute program of `new`, two storage buffers of this struct bound to the points the shader names, a dispatch of
        // one workgroup per run (the shader returns for a group past `u_count`), then the barrier that makes the list visible to the
        // index fetch of the draws.
        unsafe {
            (f.UseProgram)(self.prog.id);
            (f.Uniform1i)(self.prog.loc[0], count as i32);
            (f.BindBufferBase)(SHADER_STORAGE_BUFFER, 0, self.ranges.id);
            (f.BindBufferBase)(SHADER_STORAGE_BUFFER, 1, self.indices.id);
            (compute.dispatch)(count.min(GROUP_LIMIT), count.div_ceil(GROUP_LIMIT), 1);
            (compute.barrier)(ELEMENT_ARRAY_BARRIER_BIT);
        }
        self.active = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gles::shaders::{self, Chunk};

    /// The compute shader's `quad`, in Rust: the same arithmetic, checked against the shared quad index buffer it stands in for.
    fn quad(k: u32) -> u32 {
        let (c, q) = (k % 6, (k / 6) * 4);
        if c < 3 {
            q + c
        } else if c == 5 {
            q
        } else {
            q + c - 1
        }
    }

    /// The shader text spells the same arithmetic (`flatten.wgsl`'s `quad`), the group limit and the lane count the Rust side assumes.
    #[test]
    fn flatten_shader_matches_the_shared_quad_indices() {
        let shared = embeddium::quad_index::indices(64);
        for (k, &want) in shared.iter().enumerate() {
            assert_eq!(quad(k as u32), want, "index {k}");
        }
        assert_eq!(GROUP_LIMIT, 65535);
        for text in ["#version 310 es", "local_size_x = 64", "gl_WorkGroupID.y * 65535u", "k += 64u", "uint(u_count)", "if (c < 3u) { return q + c; }", "if (c == 5u) { return q; }", "return q + c - 1u;"] {
            assert!(FLATTEN_CS.contains(text), "missing {text:?}");
        }
    }

    /// The flat chunk vertex shader reads the page's block table by vertex number; the per-chunk one keeps its origin uniform, so the
    /// two programs of a layer differ only there.
    #[test]
    fn flat_vertex_shader_reads_the_block_table() {
        for c in [Chunk::Solid, Chunk::Cutout, Chunk::Water] {
            let (flat, per_chunk) = (shaders::chunk_with(c, true, true).vs, shaders::chunk(c, true).vs);
            assert!(flat.contains("uniform B { vec4 blocks[4096]; }"), "{c:?}");
            assert!(flat.contains("blocks[uint(gl_VertexID) >> 8u].xyz"), "{c:?}");
            assert!(!flat.contains("uniform vec3 u_origin"), "{c:?}");
            assert!(per_chunk.contains("uniform vec3 u_origin") && !per_chunk.contains("uniform B"), "{c:?}");
            assert!(!flat.contains('@'), "{c:?}: a placeholder was left");
        }
        assert_eq!(BLOCKS, 4096);
        assert_eq!(BLOCK_TABLE_BYTES, 65536);
    }
}
