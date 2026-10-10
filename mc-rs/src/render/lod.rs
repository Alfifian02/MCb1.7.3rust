//! Distant terrain (LOD): the ground out to `RADIUS` blocks, drawn beyond the real chunk ring. The idea of Distant Horizons
//! (a quadtree of tiles, finer near the eye, coarser far away), cut down to what a Beta 1.7.3 world on a phone needs:
//!
//! - **No stored LOD data.** DH generates real chunks, down-samples them into a column database (SQLite + compression) and
//!   re-reads it. Beta terrain is a pure function of the seed, so a tile is computed straight from the terrain's density
//!   grid (`OverworldGenerator::lod_columns`): no chunk, no caves, no populate, no light, nothing on disk. Evicted tiles are
//!   simply computed again (about 13 ms of noise on one background thread).
//! - **One height per column** (2.5D), not DH's stack of vertical slices: Beta has no floating islands worth the memory.
//! - **A smooth heightfield, not boxes**: one vertex per cell corner (shared by its four cells), normal from the neighbouring
//!   heights, colour = the block's mean tile colour, lit per pixel by the sun like the chunks. No vertical walls, except a
//!   skirt hanging from each tile's edge (covers the crack against a tile of another level). 20-byte vertices, `u16`
//!   indices, one draw call per tile, its own small shader (fog identical to the chunk shader's).
//! - **Quadtree** of 16 x 16 cell tiles: level `l` has cells of `4 << l` blocks (level 0 = the terrain's own 4-block grid).
//!   A node splits while the eye is closer than `SPLIT` x its width; a node whose children are not all ready is drawn itself,
//!   so loading never leaves a hole (coarse tiles are requested first).
//! - The real chunks win inside their ring: the fragment shader drops LOD pixels over a chunk the ring covers (DH's
//!   `uClipDistance`), so the tiles under the player need no special case and no rebuild when he walks. (A dither band
//!   as in DH's `uDitherDhRendering` was tried and dropped: it made the chunks' trees see-through, since the LOD has none,
//!   and discarding in the chunk shader costs early-z.)
//! - **Matches the chunks**: colours are the mean of the real `terrain.png` tile (`atlas::lod_color`, tint included), the sun
//!   term is the chunk shader's (`mix(SHADOW_BRIGHTNESS, 1, sqrt(N.L))`), plus a per-block brightness jitter that fades with
//!   distance (DH's noise texture, `noiseDropoff`), so flat ground is not one flat colour.
//!
//! ponytail: LOD tiles do not show edits (the player's blocks, trees, caves): they come from the seed. Upgrade = re-sample
//! the edited chunks into their tiles. Cracks between two levels are covered by skirts (the tile's outer side faces go down
//! to y = 0), not by DH's fade/dither. No shadows or light shafts on LOD (the shadow map is 64 blocks wide anyway).
//! UNVERIFIED: not compiled, not run on a device, no frame times; the 1024-block radius, `SPLIT` 2.0 and the single worker
//! are guesses to measure.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

use crate::render::atlas;
use crate::render::camera::Frustum;
use crate::world::gen::chunk_manager::WorldChunkManager;
use crate::world::gen::overworld::{LodCol, OverworldGenerator};

/// Cells along a tile edge (the tile mesh), and the blocks of a level-0 cell (the terrain's own density grid).
const CELLS: usize = 16;
const BASE: i32 = 4;
/// Quadtree levels 0..LEVELS; the roots are `LEVELS - 1`. A node of width `w` splits while the eye is closer than `SPLIT * w`,
/// so a cell is never wider than `dist / (SPLIT * 16)`: with 2.0 the level-2 cells (16 blocks) start at 512 blocks.
const LEVELS: u8 = 3;
const SPLIT: f32 = 2.0;
/// Blocks of LOD terrain around the eye; the fog is complete there (`set_fog` takes it too), the far plane sits just past it.
pub const RADIUS: f32 = 1024.0;
pub const FAR: f32 = RADIUS + 256.0;
/// Tiles asked of the worker at once (the list is rebuilt every frame, so stale requests stay few), and uploaded per frame.
const MAX_PENDING: usize = 4;
const UPLOADS_PER_FRAME: usize = 4;
/// A tile that was neither drawn nor wanted for this many frames is dropped.
const KEEP_FRAMES: u64 = 600;

/// (level, tile x, tile z); the tile covers blocks `t * width(level)` .. `+ width(level)`.
type Key = (u8, i32, i32);

fn width(level: u8) -> i32 {
    (CELLS as i32 * BASE) << level
}

/// Distance in blocks from (ex, ez) to the tile's square (0 inside it).
fn box_dist(k: Key, ex: f32, ez: f32) -> f32 {
    let w = width(k.0) as f32;
    let (x0, z0) = (k.1 as f32 * w, k.2 as f32 * w);
    let dx = (x0 - ex).max(ex - (x0 + w)).max(0.0);
    let dz = (z0 - ez).max(ez - (z0 + w)).max(0.0);
    dx.hypot(dz)
}

/// The tiles to draw for an eye at (ex, ez), and the ones to have ready (`want`, children before their parent).
/// `ready` says which tiles exist. Every point within `RADIUS` is in exactly one drawn tile, or in none while nothing that
/// covers it is ready yet; two drawn tiles never overlap.
fn select(ex: f32, ez: f32, ready: &dyn Fn(Key) -> bool) -> (Vec<Key>, Vec<Key>) {
    fn walk(k: Key, ex: f32, ez: f32, ready: &dyn Fn(Key) -> bool, draw: &mut Vec<Key>, want: &mut Vec<Key>) -> bool {
        let d = box_dist(k, ex, ez);
        if d >= RADIUS {
            return true; // nothing to draw here
        }
        if k.0 > 0 && d < SPLIT * width(k.0) as f32 {
            let mark = draw.len();
            let mut all = true;
            for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                all &= walk((k.0 - 1, k.1 * 2 + dx, k.2 * 2 + dz), ex, ez, ready, draw, want);
            }
            if all {
                return true;
            }
            draw.truncate(mark); // a child is missing: show this tile instead (and ask for it)
        }
        want.push(k);
        if ready(k) {
            draw.push(k);
            true
        } else {
            false
        }
    }
    let (top, w) = (LEVELS - 1, width(LEVELS - 1) as f32);
    let (r0, r1) = (((ex - RADIUS) / w).floor() as i32, ((ex + RADIUS) / w).floor() as i32);
    let (s0, s1) = (((ez - RADIUS) / w).floor() as i32, ((ez + RADIUS) / w).floor() as i32);
    let (mut draw, mut want) = (Vec::new(), Vec::new());
    for tx in r0..=r1 {
        for tz in s0..=s1 {
            walk((top, tx, tz), ex, ez, ready, &mut draw, &mut want);
        }
    }
    (draw, want)
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct LodVertex {
    pos: [f32; 3],
    /// sRGB values like the atlas; a is unused.
    col: [u8; 4],
    /// Unit normal, snorm8 (w unused).
    nrm: [i8; 4],
}

/// Edge of a skirt: how far it hangs below the tile's edge, in blocks.
const SKIRT: f32 = 24.0;

/// Mesh of one tile from its `n` x `n` columns (`n` = `CELLS + 3`; column (i, j) is the corner at block
/// `x0 + (i - 1) * cell`, `z0 + (j - 1) * cell`). Vertices are the `CELLS + 1` squared corners 1..=CELLS + 1 (the ring around
/// them is only read, for normals and so that neighbouring tiles agree on their shared edge), two triangles per cell, and a
/// skirt hanging `SKIRT` blocks from each of the four edges.
fn build(cols: &[LodCol], n: usize, x0: i32, z0: i32, cell: i32) -> (Vec<LodVertex>, Vec<u16>) {
    assert_eq!(cols.len(), n * n);
    let (m, c) = (CELLS + 1, cell as f32);
    let h = |i: usize, j: usize| cols[i * n + j].top as f32;
    let mut v = Vec::with_capacity(m * m + 4 * m);
    for i in 1..=m {
        for j in 1..=m {
            let (nx, nz) = (h(i - 1, j) - h(i + 1, j), h(i, j - 1) - h(i, j + 1));
            let len = (nx * nx + 4.0 * c * c + nz * nz).sqrt();
            let q = |f: f32| (f / len * 127.0).round() as i8;
            let k = atlas::lod_color(cols[i * n + j].block);
            v.push(LodVertex {
                pos: [(x0 + (i as i32 - 1) * cell) as f32, h(i, j), (z0 + (j as i32 - 1) * cell) as f32],
                col: [k[0], k[1], k[2], 255],
                nrm: [q(nx), q(2.0 * c), q(nz), 0],
            });
        }
    }
    let mut idx = Vec::with_capacity(6 * CELLS * CELLS + 24 * CELLS);
    for a in 0..CELLS {
        for b in 0..CELLS {
            let (p00, p10, p01, p11) = ((a * m + b) as u16, ((a + 1) * m + b) as u16, (a * m + b + 1) as u16, ((a + 1) * m + b + 1) as u16);
            idx.extend_from_slice(&[p01, p11, p10, p01, p10, p00]); // counter-clockwise from above
        }
    }
    // Skirts: copies of the edge vertices, lowered. (The pipeline does not cull, so their winding does not matter.)
    for edge in 0..4 {
        let at = |k: usize| match edge { 0 => k, 1 => CELLS * m + k, 2 => k * m, _ => k * m + CELLS };
        let base = v.len() as u16;
        for k in 0..m {
            let mut d = v[at(k)];
            d.pos[1] = (d.pos[1] - SKIRT).max(0.0);
            v.push(d);
        }
        for k in 0..CELLS {
            let (s0, s1, d0, d1) = (at(k) as u16, at(k + 1) as u16, base + k as u16, base + k as u16 + 1);
            idx.extend_from_slice(&[s0, s1, d1, s0, d1, d0]);
        }
    }
    (v, idx)
}

/// The worker: owns its own generator (a tile does not depend on which thread makes it).
fn work(seed: i64, jobs: mpsc::Receiver<Key>, done: mpsc::Sender<(Key, Vec<LodVertex>, Vec<u16>)>) {
    let (gen, mut cm) = (OverworldGenerator::new(seed), WorldChunkManager::new(seed));
    while let Ok(key) = jobs.recv() {
        let cell = BASE << key.0;
        let (x0, z0) = (key.1 * width(key.0), key.2 * width(key.0));
        let n = CELLS + 3;
        // Columns are sampled at their middle, so start half a cell before the corner of index 0 (corner -1).
        let cols = gen.lod_columns(&mut cm, x0 - cell - cell / 2, z0 - cell - cell / 2, n, cell);
        let (v, i) = build(&cols, n, x0, z0, cell);
        if done.send((key, v, i)).is_err() {
            break;
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct Uniforms {
    vp: [[f32; 4]; 4],
    eye: [f32; 4],
    /// rgb: fog colour, w: fog distance.
    fog: [f32; 4],
    /// x: daylight brightness (`chunk::brightness`), y: 1 when the surface is sRGB, zw: the chunk the player is in.
    p: [f32; 4],
    /// x: radius of the real chunk ring.
    q: [f32; 4],
    /// xyz: direction toward the light, w: shadow strength (0 = flat).
    sun: [f32; 4],
}

const SHADER: &str = r#"
struct U { vp: mat4x4<f32>, eye: vec4<f32>, fog: vec4<f32>, p: vec4<f32>, q: vec4<f32>, sun: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) col: vec3<f32>, @location(1) wp: vec3<f32>, @location(2) nrm: vec3<f32> };
@vertex
fn vs(@location(0) p: vec3<f32>, @location(1) c: vec4<f32>, @location(2) nr: vec4<f32>) -> VsOut {
    var o: VsOut;
    o.pos = u.vp * vec4<f32>(p, 1.0);
    o.col = c.rgb;
    o.wp = p;
    o.nrm = nr.xyz;
    return o;
}
@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    // The real chunks are drawn inside their ring (the same test as the mesher's ring), the LOD only outside it.
    let c = floor(in.wp.xz / 16.0) - u.p.zw;
    if dot(c, c) <= u.q.x * u.q.x { discard; }
    let n = normalize(in.nrm);
    let d = length(in.wp.xz - u.eye.xz);
    var rgb = in.col;
    if u.p.y > 0.5 { rgb = pow(rgb, vec3<f32>(2.2)); } // the atlas is sRGB and decoded on sampling; match it
    // Per-block jitter (integer hash of the block), fading out with distance where it would only shimmer.
    let b = bitcast<vec2<u32>>(vec2<i32>(floor(in.wp.xz + 0.001)));
    var h = (b.x * 73856093u) ^ (b.y * 83492791u);
    h = (h ^ (h >> 13u)) * 1274126177u;
    rgb = rgb * (1.0 + (f32((h >> 8u) & 255u) / 255.0 - 0.5) * 0.2 * (1.0 - smoothstep(48.0, 200.0, d)));
    // The chunk shader's sun term without the map (unshadowed): `mix(SHADOW_BRIGHTNESS, 1, sqrt(N.L))`, faded by strength.
    var k = 1.0;
    if u.sun.w > 0.0 { k = mix(1.0, mix(0.75, 1.0, sqrt(max(dot(n, u.sun.xyz), 0.0))), u.sun.w); }
    rgb = rgb * u.p.x * k * (0.6 + 0.4 * max(n.y, 0.0)); // slopes darken like the chunks' side faces (1.0 flat, 0.6 vertical)
    // The chunk shader's fog (`NormalFog`, density 2): 1 - (far - d) * 5 / (2 far), smoothstepped.
    let far = u.fog.w;
    let f = clamp(1.0 - (far - length(in.wp - u.eye.xyz)) * 5.0 / (2.0 * far), 0.0, 1.0);
    return vec4<f32>(mix(rgb, u.fog.rgb, f * f * (3.0 - 2.0 * f)), 1.0);
}
"#;

struct Tile {
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    index_count: u32,
    /// Frame this tile was last drawn or wanted.
    used: u64,
}

pub struct LodRenderer {
    pipeline: wgpu::RenderPipeline,
    ubuf: wgpu::Buffer,
    bind: wgpu::BindGroup,
    srgb: f32,
    tiles: HashMap<Key, Tile>,
    pending: HashSet<Key>,
    /// The tiles `update` chose for this frame.
    drawn: Vec<Key>,
    jobs: mpsc::Sender<Key>,
    done: mpsc::Receiver<(Key, Vec<LodVertex>, Vec<u16>)>,
    frame: u64,
}

impl LodRenderer {
    /// `seed` is the world's; the worker thread ends when this is dropped.
    pub fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat, seed: i64) -> Self {
        let (jobs, job_rx) = mpsc::channel();
        let (done_tx, done) = mpsc::channel();
        // One worker: the chunk workers and the render thread need the cores more.
        std::thread::Builder::new().name("lod-gen".into()).spawn(move || work(seed, job_rx, done_tx)).expect("spawn lod worker");

        let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lod_uniform"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lod_bind_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("lod_bind"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("lod_shader"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("lod_pipeline"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("lod_pipeline_layout"),
                bind_group_layouts: &[&layout],
                push_constant_ranges: &[],
            })),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<LodVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Unorm8x4, offset: 12, shader_location: 1 },
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Snorm8x4, offset: 16, shader_location: 2 },
                    ],
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState { format: surface_format, blend: Some(wgpu::BlendState::REPLACE), write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(), // no culling: the skirts are seen from either side
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        Self {
            pipeline, ubuf, bind, srgb: if surface_format.is_srgb() { 1.0 } else { 0.0 },
            tiles: HashMap::new(), pending: HashSet::new(), drawn: Vec::new(), jobs, done, frame: 0,
        }
    }

    /// Once per frame, before `prepare` / `draw`: take finished tiles, choose what to draw, ask for what is missing
    /// (coarse tiles first, nearest first), drop tiles nobody used for a while.
    pub fn update(&mut self, device: &wgpu::Device, eye: Vec3) {
        self.frame += 1;
        for _ in 0..UPLOADS_PER_FRAME {
            let Ok((key, v, i)) = self.done.try_recv() else { break };
            self.pending.remove(&key);
            let buf = |label, contents: &[u8], usage| device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents, usage });
            self.tiles.insert(key, Tile {
                vbuf: buf("lod_vbuf", bytemuck::cast_slice(&v), wgpu::BufferUsages::VERTEX),
                ibuf: buf("lod_ibuf", bytemuck::cast_slice(&i), wgpu::BufferUsages::INDEX),
                index_count: i.len() as u32,
                used: self.frame,
            });
        }
        let tiles = &self.tiles;
        let (draw, mut want) = select(eye.x, eye.z, &|k| tiles.contains_key(&k));
        for k in draw.iter().chain(&want) {
            if let Some(t) = self.tiles.get_mut(k) {
                t.used = self.frame;
            }
        }
        self.drawn = draw;
        want.sort_by(|a, b| b.0.cmp(&a.0).then(box_dist(*a, eye.x, eye.z).total_cmp(&box_dist(*b, eye.x, eye.z))));
        for k in want {
            if self.pending.len() >= MAX_PENDING {
                break;
            }
            if !self.tiles.contains_key(&k) && self.pending.insert(k) {
                let _ = self.jobs.send(k);
            }
        }
        if self.frame % 120 == 0 {
            let f = self.frame;
            self.tiles.retain(|_, t| t.used + KEEP_FRAMES > f);
        }
    }

    /// `day` = brightness of full sky light now (`chunk::brightness(15 - skylight_subtracted)`); `ring` = (chunk x, chunk z,
    /// radius in chunks) of the real terrain, which the LOD leaves alone; `sun`, `strength` = the chunk shader's light.
    pub fn prepare(&self, queue: &wgpu::Queue, view_proj: Mat4, eye: Vec3, fog: [f32; 3], day: f32, ring: (i32, i32, i32), sun: Vec3, strength: f32) {
        let u = Uniforms {
            vp: view_proj.to_cols_array_2d(),
            eye: eye.extend(0.0).to_array(),
            fog: [fog[0], fog[1], fog[2], RADIUS],
            p: [day, self.srgb, ring.0 as f32, ring.1 as f32],
            q: [ring.2 as f32, 0.0, 0.0, 0.0],
            sun: sun.extend(strength).to_array(),
        };
        queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&u));
    }

    /// Inside the chunk pass, after the real chunks.
    pub fn draw(&self, rp: &mut wgpu::RenderPass<'_>, frustum: &Frustum) {
        rp.set_pipeline(&self.pipeline);
        rp.set_bind_group(0, &self.bind, &[]);
        for k in &self.drawn {
            let Some(t) = self.tiles.get(k) else { continue };
            let w = width(k.0) as f32;
            let min = Vec3::new(k.1 as f32 * w, 0.0, k.2 as f32 * w);
            if !frustum.intersects_aabb(min, min + Vec3::new(w, 128.0, w)) {
                continue;
            }
            rp.set_vertex_buffer(0, t.vbuf.slice(..));
            rp.set_index_buffer(t.ibuf.slice(..), wgpu::IndexFormat::Uint16);
            rp.draw_indexed(0..t.index_count, 0, 0..1);
        }
    }

    pub fn loaded(&self) -> usize {
        self.tiles.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inside(k: Key, x: f32, z: f32) -> bool {
        let w = width(k.0) as f32;
        x >= k.1 as f32 * w && x < (k.1 + 1) as f32 * w && z >= k.2 as f32 * w && z < (k.2 + 1) as f32 * w
    }

    /// Whatever is ready, no point is covered twice; with everything ready every point near the eye is covered once.
    #[test]
    fn tiles_never_overlap_and_cover() {
        let halves = |k: Key| (k.0 as i32 * 31 + k.1 * 7 + k.2 * 13).rem_euclid(2) == 0;
        for (ex, ez) in [(0.0, 0.0), (37.5, -911.0), (-3000.25, 4096.0), (255.9, 256.1)] {
            for case in 0..3 {
                let ready = |k: Key| match case { 0 => true, 1 => k.0 == LEVELS - 1, _ => halves(k) };
                let (draw, want) = select(ex, ez, &ready);
                assert!(draw.iter().all(|k| ready(*k)) && draw.iter().all(|k| want.contains(k)));
                for a in -20..=20 {
                    for b in -20..=20 {
                        let (x, z) = (ex + a as f32 * 45.0, ez + b as f32 * 45.0);
                        let n = draw.iter().filter(|k| inside(**k, x, z)).count();
                        assert!(n <= 1, "{n} tiles over ({x}, {z}), case {case}");
                        if case < 2 && (x - ex).hypot(z - ez) < RADIUS * 0.9 {
                            assert_eq!(n, 1, "hole at ({x}, {z}), case {case}");
                        }
                    }
                }
            }
        }
        // Close to the eye the tiles are the finest ones.
        let (draw, _) = select(0.0, 0.0, &|_| true);
        assert!(draw.iter().any(|k| k.0 == 0 && inside(*k, 1.0, 1.0)) && draw.iter().any(|k| k.0 == LEVELS - 1));
    }

    /// The grass colour is the tinted tile's mean (green over red and blue), not the old flat one.
    #[test]
    fn colours_come_from_the_tiles() {
        let g = atlas::lod_color(2);
        assert!(g[1] > g[0] && g[1] > g[2], "grass {g:?}");
    }

    fn flat(n: usize, top: u8) -> Vec<LodCol> {
        vec![LodCol { top, block: 2 }; n * n]
    }

    /// Flat ground: the corner grid plus four skirts, normals straight up; a raised column tilts its neighbours' normals away from it.
    #[test]
    fn heightfield_mesh() {
        let n = CELLS + 3;
        let mut cols = flat(n, 70);
        let (v, i) = build(&cols, n, 0, 0, 4);
        let m = CELLS + 1;
        assert_eq!((v.len(), i.len()), (m * m + 4 * m, 6 * CELLS * CELLS + 24 * CELLS));
        assert!(v.iter().all(|p| p.pos[1] >= 46.0 && p.pos[1] <= 70.0) && v[0].nrm[1] == 127 && v[0].nrm[0] == 0);
        // Corner (i, j) = (1, 1) is the tile's own corner, block (0, 0); (CELLS + 1, CELLS + 1) is (64, 64).
        assert_eq!((v[0].pos[0], v[0].pos[2], v[m * m - 1].pos[0]), (0.0, 0.0, 64.0));
        // The top faces up: counter-clockwise seen from above, like the chunk mesher's.
        let t: Vec<Vec3> = i[0..3].iter().map(|&k| Vec3::from(v[k as usize].pos)).collect();
        assert!((t[1] - t[0]).cross(t[2] - t[1]).y > 0.0);
        cols[8 * n + 8].top = 75; // corner (7, 7) in vertex space
        let (v, _) = build(&cols, n, 0, 0, 4);
        assert!(v[6 * m + 7].nrm[0] < 0 && v[7 * m + 6].nrm[2] < 0 && v[7 * m + 7].pos[1] == 75.0, "{:?}", v[6 * m + 7].nrm);
    }

    /// The density-grid height is the real terrain's within a few blocks (cells are sampled at their middle, the chunk is
    /// built from the grid corners): land columns of nine chunks, mean error under 4 blocks.
    #[test]
    fn lod_follows_the_terrain() {
        let seed = 0xCAFEBABE_i64;
        let (mut g, gen, mut cm) = (OverworldGenerator::new(seed), OverworldGenerator::new(seed), WorldChunkManager::new(seed));
        let (mut sum, mut land) = (0.0, 0);
        for cx in 0..3 {
            for cz in 0..3 {
                let blocks = g.generate(cx, cz, &mut cm);
                let cols = gen.lod_columns(&mut cm, cx * 16, cz * 16, 4, 4);
                for (i, c) in cols.iter().enumerate() {
                    if matches!(c.block, 9 | 79) {
                        continue; // under water the real top block is the sea floor
                    }
                    let (x, z) = (i / 4 * 4 + 2, i % 4 * 4 + 2);
                    sum += ((c.top as i32 - 1) - OverworldGenerator::top_block(&blocks, x, z, 16)).abs() as f64;
                    land += 1;
                }
            }
        }
        assert!(land > 20, "only {land} land columns");
        assert!(sum / (land as f64) < 4.0, "mean error {} blocks", sum / land as f64);
    }
}
