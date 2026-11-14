//! Flat chunk draws: the frame's chunk runs are copied by a compute pass into one index list, in draw order, and drawn with one draw call
//! per run of chunks in the same arena page (`Pages` in `world::chunks`), instead of one `draw_indexed` and one origin bind per chunk.
//!
//! Ported from Starling's `gfx/flat.rs` (`THIRD_PARTY_NOTICES.md`). The picture is the one the per-chunk draws give: the same triangles from
//! the same vertices in the same order, through the same vertex and fragment code. Only where a vertex finds its chunk's origin changes: from
//! the dynamic-offset uniform of group 1 to a table. A chunk's vertices fill whole blocks of `GRAIN` vertices of its page (`Pages::put`
//! rounds every range up), so each block names one chunk: its origin slot (`DrawOrigins`), which indexes `DrawOrigins::flat_buffer`.
//!
//! Off by default (`flatDraws` in options.txt, `set_want`): Starling keeps it off on Android Vulkan too, and mc-rs already draws one
//! `draw_indexed` per run of a chunk, not per section. This is for measuring.
//!
//! Left out of Starling's file, because mc-rs has nothing behind it: phase 2 of GPU occlusion culling (`flatten_seen`, `draw_phase2`,
//! the phase 2 call buffer), shader packs (`draw_with`, `draw_geometry`, the shadow camera's calls), sorted translucent indices (water is
//! quads of the shared index buffer like everything else) and `for_vanilla` / `is_broken_intel_vulkan` (Metal and desktop Intel).
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};

use wgpu::{BindGroup, BindGroupLayout, Buffer, CommandEncoder, ComputePipeline, Device, Queue, RenderPipeline};

use crate::gpu::pipeline::DrawOrigins;
use crate::world::chunks::{ChunkManager, Layer};

/// Vertices per block of a page, as Starling's `arena::GRAIN`: a mesh starts at a multiple of it and fills whole blocks.
pub const GRAIN: u32 = 256;
/// `vertex >> 8` in the vertex shader.
const _: () = assert!(GRAIN == 1 << 8);

/// A frame with fewer sections than this keeps the per-chunk draws: flat draws cost a compute pass and an index list, and gain nothing on
/// a small view. Starling's Android bar (its desktop one is 1600). `flatDraws:2` ignores it, for A/B checks.
pub const MIN_SECTIONS: usize = 512;

static WANT: AtomicU8 = AtomicU8::new(0);

/// `flatDraws` of options.txt, as set at start (the GL renderer reads it too, `gles::flat`).
pub fn want() -> u8 {
    WANT.load(Ordering::Relaxed)
}

/// `flatDraws` of options.txt: 0 off (the default), 1 on past `MIN_SECTIONS`, 2 on for every frame. Set once, before the pipelines.
pub fn set_want(v: u8) {
    WANT.store(v.min(2), Ordering::Relaxed);
}

/// Whether the pipelines of flat draws are built: asked for, and the device has what they read. Logs why not.
pub fn wanted(adapter: &wgpu::Adapter, device: &Device) -> bool {
    let want = WANT.load(Ordering::Relaxed);
    if want == 0 {
        return false;
    }
    let ok = supported(adapter, device);
    log::info!("flat chunk draws: asked for ({want}), {}", if ok { "on" } else { "this device cannot (no vertex storage / compute / storage buffers), off" });
    ok
}

/// The `min_sections` of `Flat::new` for the `flatDraws` value.
pub fn min_sections() -> usize {
    if WANT.load(Ordering::Relaxed) >= 2 { 0 } else { MIN_SECTIONS }
}

/// Whether flat draws exist: vertex shaders can read storage buffers large enough for an index page.
fn supported(adapter: &wgpu::Adapter, device: &Device) -> bool {
    let flags = adapter.get_downlevel_capabilities().flags;
    let limits = device.limits();
    // Two storage buffers in the vertex stage (block table, origins) and two in the compute pass (ranges, index list).
    flags.contains(wgpu::DownlevelFlags::VERTEX_STORAGE | wgpu::DownlevelFlags::COMPUTE_SHADERS)
        && limits.max_storage_buffers_per_shader_stage >= 2
        && limits.max_storage_buffer_binding_size >= wgpu::Limits::default().max_storage_buffer_binding_size
}

/// One chunk run for `flatten.wgsl`: index count, base vertex, first index in the list.
pub(crate) type Range = [u32; 3];

/// A run of the frame's index list drawn from one arena page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Call {
    pub page: u32,
    pub first: u32,
    pub count: u32,
}

/// One draw of the per-chunk loop (`Run`): the page it reads, its index count, and the first vertex of its quads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Draw {
    pub page: u32,
    pub count: u32,
    pub base: u32,
}

/// The frame's draw calls, as `plan` lays them out: where the cut-out and the water calls begin (the solid ones come first).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Plan {
    pub cutout_calls: usize,
    pub water_calls: usize,
    pub total: u64,
}

pub(crate) const GROUP_LIMIT: u32 = 65535;

pub struct Flat {
    /// The chunk pipelines as the per-chunk ones, reading the block table in group 1 instead of the origin uniform.
    solid: RenderPipeline,
    cutout: RenderPipeline,
    water: RenderPipeline,
    flatten: ComputePipeline,
    compute_layout: BindGroupLayout,
    /// The layout of a page's bind group 1: its block table, and the origins by slot.
    draw_layout: BindGroupLayout,
    /// Group 1 of each page that was drawn.
    groups: HashMap<u32, BindGroup>,
    /// The frame's index list.
    indices: Buffer,
    index_capacity: u64,
    ranges: Buffer,
    range_capacity: u64,
    range_data: Vec<Range>,
    draw_data: Vec<Draw>,
    /// This frame's draw calls: solid ones, then cut-out, then water.
    calls: Vec<Call>,
    cutout_calls: usize,
    water_calls: usize,
    /// `prepare` built this frame's list: `draw` uses it (off: the per-chunk draws).
    active: bool,
    min_sections: usize,
}

fn storage(binding: u32, stage: wgpu::ShaderStages, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: stage,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}

fn buffer(device: &Device, label: &str, size: u64, usage: wgpu::BufferUsages) -> Buffer {
    device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size.max(16), usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false })
}

/// Lays out the frame's index list: the draw calls, one per run of draws in the same vertex page (a layer's first draw starts a call of its
/// own, `layers` = the draw where the cut-out and the water layers begin), and each draw's range of the list (`ranges`, in draw order).
pub(crate) fn plan(draws: &[Draw], layers: [usize; 2], calls: &mut Vec<Call>, ranges: &mut Vec<Range>) -> Plan {
    calls.clear();
    ranges.clear();
    let mut layer_calls = [None; 2];
    let mut total = 0u64;
    for (i, d) in draws.iter().enumerate() {
        for (k, &from) in layers.iter().enumerate() {
            if i == from {
                layer_calls[k].get_or_insert(calls.len());
            }
        }
        let starts = layers.contains(&i);
        match calls.last_mut() {
            Some(last) if last.page == d.page && !starts => last.count += d.count,
            _ => calls.push(Call { page: d.page, first: total as u32, count: d.count }),
        }
        ranges.push([d.count, d.base, total as u32]);
        total += u64::from(d.count);
    }
    Plan { cutout_calls: layer_calls[0].unwrap_or(calls.len()), water_calls: layer_calls[1].unwrap_or(calls.len()), total }
}

impl Flat {
    /// `pipeline(layout, label, vertex entry, fragment entry, water)` builds a chunk pipeline as the per-chunk ones, over `layout`
    /// (the chunk bind group, then `draw_layout`).
    pub fn new(
        device: &Device,
        terrain_layout: &BindGroupLayout,
        min_sections: usize,
        pipeline: impl Fn(&wgpu::PipelineLayout, &str, &str, &str, bool) -> RenderPipeline,
    ) -> Self {
        let vertex = wgpu::ShaderStages::VERTEX;
        let compute = wgpu::ShaderStages::COMPUTE;
        // Bindings 1 and 2: the shader's `draw` uniform of the per-chunk entry points keeps binding 0 of group 1.
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("Flat chunk draws"), entries: &[storage(1, vertex, true), storage(2, vertex, true)] });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("Flat chunk layout"), bind_group_layouts: &[terrain_layout, &draw_layout], push_constant_ranges: &[] });
        let compute_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("Flatten chunk draws"), entries: &[storage(0, compute, true), storage(1, compute, false)] });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("Flatten chunk draws"), source: wgpu::ShaderSource::Wgsl(include_str!("flatten.wgsl").into()) });
        let flatten = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Flatten chunk draws"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("Flatten chunk draws"), bind_group_layouts: &[&compute_layout], push_constant_ranges: &[] })),
            module: &module,
            entry_point: Some("flatten"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self {
            solid: pipeline(&layout, "chunk_solid_flat", "vs_chunk_flat", "fs_solid", false),
            cutout: pipeline(&layout, "chunk_cutout_flat", "vs_chunk_flat", "fs_cut", false),
            water: pipeline(&layout, "water_flat", "vs_chunk_water_flat", "fs_water", true),
            flatten,
            compute_layout,
            draw_layout,
            groups: HashMap::new(),
            indices: buffer(device, "Flat chunk indices", 16, wgpu::BufferUsages::INDEX | wgpu::BufferUsages::STORAGE),
            index_capacity: 4,
            ranges: buffer(device, "Flat chunk ranges", 16, wgpu::BufferUsages::STORAGE),
            range_capacity: 1,
            range_data: Vec::new(),
            draw_data: Vec::new(),
            calls: Vec::new(),
            cutout_calls: 0,
            water_calls: 0,
            active: false,
            min_sections,
        }
    }

    /// Whether `prepare` built this frame's index list.
    pub fn active(&self) -> bool {
        self.active
    }

    /// Builds the frame's index list from the chunk runs of the three layers (`ChunkManager::for_each_run`, the draws the per-chunk loop
    /// would make) and records the compute pass that fills it into `encoder`, before the scene pass. False when the frame is drawn the
    /// per-chunk way instead (too few sections, nothing to draw, a list too big for a storage binding).
    pub fn prepare(&mut self, device: &Device, queue: &Queue, encoder: &mut CommandEncoder, chunks: &ChunkManager, origins: &DrawOrigins) -> bool {
        self.active = false;
        self.calls.clear();
        self.cutout_calls = 0;
        self.water_calls = 0;
        let Some(origin_buffer) = origins.flat_buffer() else { return false };
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
        // Every drawn page needs its block table; a page without one (made before flat draws were on) cannot be drawn this way.
        let all_tables = self.calls.iter().all(|c| chunks.page_blocks(c.page).is_some());
        if total == 0 || !all_tables || total * 4 > u64::from(device.limits().max_storage_buffer_binding_size) {
            self.calls.clear();
            return false;
        }
        self.cutout_calls = cutout_calls;
        self.water_calls = water_calls;
        if self.range_data.len() as u64 > self.range_capacity {
            self.range_capacity = (self.range_data.len() as u64).next_power_of_two();
            self.ranges = buffer(device, "Flat chunk ranges", self.range_capacity * 12, wgpu::BufferUsages::STORAGE);
        }
        if total > self.index_capacity {
            self.index_capacity = total.next_power_of_two();
            self.indices = buffer(device, "Flat chunk indices", self.index_capacity * 4, wgpu::BufferUsages::INDEX | wgpu::BufferUsages::STORAGE);
        }
        queue.write_buffer(&self.ranges, 0, bytemuck::cast_slice(&self.range_data));
        let count = self.range_data.len() as u32;
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Flatten chunk draws"),
            layout: &self.compute_layout,
            entries: &[
                // Exactly the ranges of the frame: the shader takes `arrayLength` for their number.
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &self.ranges, offset: 0, size: wgpu::BufferSize::new(u64::from(count) * 12) }),
                },
                wgpu::BindGroupEntry { binding: 1, resource: self.indices.as_entire_binding() },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("Flatten chunk draws"), timestamp_writes: None });
        pass.set_pipeline(&self.flatten);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(count.min(GROUP_LIMIT), count.div_ceil(GROUP_LIMIT), 1);
        drop(pass);
        // Forget the tables of pages the arena released, make those of the pages this frame draws.
        self.groups.retain(|&page, _| chunks.has_page(page));
        for page in self.calls.iter().map(|c| c.page) {
            if let Some(blocks) = chunks.page_blocks(page) {
                self.groups.entry(page).or_insert_with(|| {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("Flat chunk draws"),
                        layout: &self.draw_layout,
                        entries: &[
                            wgpu::BindGroupEntry { binding: 1, resource: blocks.wgpu().as_entire_binding() },
                            wgpu::BindGroupEntry { binding: 2, resource: origin_buffer.as_entire_binding() },
                        ],
                    })
                });
            }
        }
        self.active = true;
        true
    }

    /// Draws `layer`'s calls (the chunk bind group, group 0, is set already, as for `draw_ranges`).
    pub fn draw(&self, rp: &mut wgpu::RenderPass<'_>, layer: Layer, chunks: &ChunkManager) {
        let (calls, pipeline) = match layer {
            Layer::Solid => (&self.calls[..self.cutout_calls], &self.solid),
            Layer::Cutout => (&self.calls[self.cutout_calls..self.water_calls], &self.cutout),
            Layer::Water => (&self.calls[self.water_calls..], &self.water),
        };
        if calls.is_empty() {
            return;
        }
        rp.set_pipeline(pipeline);
        rp.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        let mut page = None;
        for call in calls {
            if page != Some(call.page) {
                let (Some(vbuf), Some(group)) = (chunks.page_vbuf(call.page), self.groups.get(&call.page)) else { continue };
                rp.set_vertex_buffer(0, vbuf.wgpu().slice(..));
                rp.set_bind_group(1, group, &[]);
                page = Some(call.page);
            }
            rp.draw_indexed(call.first..call.first + call.count, 0, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Draws as `prepare` collects them: solid (page, quads at a base vertex), then cut-out, then water.
    fn sample() -> (Vec<Draw>, [usize; 2]) {
        let d = |page, quads: u32, base| Draw { page, count: quads * 6, base };
        // Solid: pages 0, 0, 1, 0 | cut-out: 0, 0 | water: 1, 0.
        (vec![d(0, 3, 0), d(0, 2, 512), d(1, 1, 256), d(0, 4, 1024), d(0, 2, 8), d(0, 1, 40), d(1, 2, 0), d(0, 1, 64)], [4, 6])
    }

    #[test]
    fn plan_draws_runs_of_a_page_in_order() {
        let (draws, layers) = sample();
        let (mut calls, mut ranges) = (Vec::new(), Vec::new());
        let plan = plan(&draws, layers, &mut calls, &mut ranges);
        assert_eq!(plan.total, 6 * 16);
        let call = |page, first, count| Call { page, first, count };
        // Solid: 0, 0 | 1 | 0, then cut-out 0, 0 (never merged with the solid call before them), then water 1 | 0.
        assert_eq!(calls, [call(0, 0, 30), call(1, 30, 6), call(0, 36, 24), call(0, 60, 18), call(1, 78, 12), call(0, 90, 6)]);
        assert_eq!((plan.cutout_calls, plan.water_calls), (3, 4));
        assert_eq!(ranges.len(), draws.len());
        assert_eq!(ranges[3], [24, 1024, 36]);
        assert_eq!(ranges[7], [6, 64, 90]);
    }

    /// A layer with no runs has no calls, and the layers after it start where it would have.
    #[test]
    fn plan_skips_empty_layers() {
        let (draws, _) = sample();
        let (mut calls, mut ranges) = (Vec::new(), Vec::new());
        // No cut-out runs: the cut-out layer begins where the water begins (draw 6), no water at all: past the end.
        let p = plan(&draws[..6], [6, 6], &mut calls, &mut ranges);
        assert_eq!((p.cutout_calls, p.water_calls), (calls.len(), calls.len()));
        let p = plan(&[], [0, 0], &mut calls, &mut ranges);
        assert_eq!((p.cutout_calls, p.water_calls, p.total), (0, 0, 0));
        assert!(calls.is_empty() && ranges.is_empty());
    }

    /// The compute pass writes exactly the indices the shared quad index buffer holds for the same runs, plus nothing: base + the
    /// pattern of `embeddium::quad_index`, for every run, in draw order.
    #[test]
    fn flatten_writes_the_indices_of_the_per_chunk_draws() {
        use wgpu::util::DeviceExt;
        let instance = wgpu::Instance::default();
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        let Ok((device, queue)) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())) else {
            eprintln!("no GPU device: skipped");
            return;
        };
        let (draws, layers) = sample();
        let (mut calls, mut ranges) = (Vec::new(), Vec::new());
        let plan = plan(&draws, layers, &mut calls, &mut ranges);
        let shared = embeddium::quad_index::indices(1024);
        // What the per-chunk loop draws: the index elements `first..first + count` of the shared buffer, `first` = base / 4 * 6.
        let expected: Vec<u32> = draws.iter().flat_map(|d| shared[(d.base / 4 * 6) as usize..][..d.count as usize].to_vec()).collect();
        let storage = |contents: &[u8], usage| device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents, usage: wgpu::BufferUsages::STORAGE | usage });
        let range_buffer = storage(bytemuck::cast_slice(&ranges), wgpu::BufferUsages::empty());
        let out = storage(&vec![0u8; plan.total as usize * 4], wgpu::BufferUsages::COPY_SRC);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(include_str!("flatten.wgsl").into()) });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: None, layout: None, module: &module, entry_point: Some("flatten"), compilation_options: Default::default(), cache: None });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: range_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: out.as_entire_binding() },
            ],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(ranges.len() as u32, 1, 1);
        }
        let read = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: plan.total * 4, usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        encoder.copy_buffer_to_buffer(&out, 0, &read, 0, plan.total * 4);
        queue.submit(Some(encoder.finish()));
        read.slice(..).map_async(wgpu::MapMode::Read, |r| r.unwrap());
        device.poll(wgpu::PollType::Wait).unwrap();
        let got: Vec<u32> = bytemuck::cast_slice(&read.slice(..).get_mapped_range()).to_vec();
        assert_eq!(got, expected);
    }
}
