//! Far terrain for the Tectonic world type: a smooth, coloured heightfield out to `RADIUS` blocks, drawn with the chunk pipeline
//! behind the real chunks, so the view reaches the horizon (a 4-chunk render distance alone ends in fog 64 blocks out).
//!
//! The ground comes from the same density the chunks are made of (`tectonic::Tectonic::heights`), so a LoD cell and the block
//! terrain it hands over to agree; they differ only in being smooth and flat-coloured. A worker thread builds 128 x 128 block
//! tiles (columns every 4 / 8 / 16 blocks, finer the nearer the eye was when the tile was asked for), nearest first, and the render
//! thread uploads them. Where real chunks are drawn the LoD cells are left out of the index buffer (a hole that follows the eye),
//! so nothing z-fights and the real terrain's edge continues into the LoD ground. Fog (`RADIUS` is its far distance) hides the
//! outer edge.
//!
//! Colour is the flat strip tile of the snow block (white) times a per-vertex tint, which the vertex shader unpacks and the
//! rasteriser blends: shores, snow lines and rock all fade into each other. Light is the slope's shade (the pipeline's `light`).
//!
//! ponytail: no trees, ore, rivers or biome tint (the colours follow height and slope only), no LoD for the vanilla world type;
//! tiles are not shared with a save, they are rebuilt each run (a tile is ~100 columns x 5 density evaluations).
//! UNVERIFIED: on a device (written without a toolchain); the radius, step distances and colours were picked by eye on paper.

use crate::gpu::pipeline::{create_index_buffer, create_vertex_buffer, Vertex};
use crate::render::atlas;
use crate::render::mesh::lm;
use crate::world::gen::overworld::{H, SEA_LEVEL};
use glam::Vec3;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};

/// How far the far terrain reaches, and the distance the fog completes at (it starts at 0.6 of it).
pub const RADIUS: f32 = 768.0;
/// The camera's far plane while the LoD is on: past the last tile's far corner.
pub const ZFAR: f32 = 1100.0;
/// Tile edge in blocks.
const TILE: i32 = 128;
/// LoD cells nearer than this (3D) to the eye are left out: the real chunks (64 blocks) are drawn there.
const HOLE: f32 = 56.0;
/// ... and only within this vertical distance (`RENDER_VERT` is 3 sections of 16).
const HOLE_DY: f32 = 44.0;
/// Depth of the skirts that hang from a tile's border and hide the cracks between tiles of different steps.
const SKIRT: f32 = 14.0;
/// Tiles asked of the worker at once.
const MAX_PENDING: usize = 2;

/// Column spacing of a tile whose centre is `dist` blocks from the eye.
fn step_for(dist: f32) -> i32 {
    if dist < 176.0 {
        4
    } else if dist < 336.0 {
        8
    } else {
        16
    }
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// 0..1, a fixed function of the column: the grain that keeps big flat areas from looking printed.
fn hash(x: i32, z: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9E3779B1) ^ (z as u32).wrapping_mul(0x85EBCA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B3C6D);
    h ^= h >> 12;
    (h & 0xFFFF) as f32 / 65535.0
}

/// sRGB bytes of the ground at height `h` (blocks) and `slope` (rise per block): the flat colours the atlas already has for grass,
/// stone, sand, snow and water, blended by height and steepness.
fn colour(h: f32, slope: f32, grain: f32) -> [f32; 3] {
    let sea = SEA_LEVEL as f32;
    let rgb = |id: u8| {
        let p = atlas::block_color(id);
        [p[0] as f32, p[1] as f32, p[2] as f32]
    };
    let k = 0.92 + 0.16 * grain;
    let c = if h < sea - 0.3 {
        mix(rgb(9), [18.0, 40.0, 110.0], ((sea - h) / 40.0).clamp(0.0, 1.0))
    } else {
        let snow = smooth(94.0, 104.0, h);
        let rock = smooth(0.55, 1.0, slope).max(0.8 * smooth(84.0, 96.0, h));
        let sand = 1.0 - smooth(sea + 0.5, sea + 3.0, h);
        let mut c = mix(rgb(2), rgb(1), rock);
        c = mix(c, rgb(12), sand * (1.0 - rock));
        mix(c, rgb(80), snow)
    };
    [(c[0] * k).min(255.0), (c[1] * k).min(255.0), (c[2] * k).min(255.0)]
}

/// One quad of the terrain grid: its centre (for the hole) and its six indices.
struct Cell {
    x: f32,
    y: f32,
    z: f32,
    idx: [u32; 6],
}

/// What the worker hands back: a tile's vertices (grid, then skirt vertices), terrain cells and skirt indices.
struct Built {
    key: (i32, i32),
    step: i32,
    verts: Vec<Vertex>,
    cells: Vec<Cell>,
    skirt: Vec<u32>,
}

/// A hanging flap under the edge a-b, both sides drawn (the pipeline culls back faces and the flap has no right side).
fn flap(verts: &mut Vec<Vertex>, skirt: &mut Vec<u32>, a: u32, b: u32) {
    let (pa, pb) = (verts[a as usize], verts[b as usize]);
    let base = verts.len() as u32;
    verts.push(Vertex { pos: [pa.pos[0], pa.pos[1] - SKIRT, pa.pos[2]], ..pa });
    verts.push(Vertex { pos: [pb.pos[0], pb.pos[1] - SKIRT, pb.pos[2]], ..pb });
    skirt.extend_from_slice(&[a, b, base + 1, a, base + 1, base, a, base + 1, b, a, base, base + 1]);
}

/// The mesh of tile `key` with a column every `step` blocks. Heights are asked for one column more on each side so the slope (and
/// so the shade and colour) at the tile's border is the same as the neighbour's.
fn build(t: &tectonic::Tectonic, key: (i32, i32), step: i32) -> Built {
    let n = (TILE / step) as usize + 1;
    let m = n + 2;
    let (x0, z0) = (key.0 * TILE, key.1 * TILE);
    let hs = t.heights(x0 - step, z0 - step, m, m, step);
    let h = |i: usize, j: usize| hs[i * m + j];
    let sea = SEA_LEVEL as f32;
    let (u, v) = atlas::atlas_uv(80, 0.0, 0.0); // the snow block's flat texel: white, so the tint is the colour
    let mut verts = Vec::with_capacity(n * n + 8 * (n - 1));
    for i in 0..n {
        for j in 0..n {
            let c = h(i + 1, j + 1);
            let dx = (h(i + 2, j + 1) - h(i, j + 1)) / (2.0 * step as f32);
            let dz = (h(i + 1, j + 2) - h(i + 1, j)) / (2.0 * step as f32);
            let slope = dx.hypot(dz);
            let (x, z) = (x0 + i as i32 * step, z0 + j as i32 * step);
            let water = c < sea - 0.3;
            // Water is one flat sheet just under the real water's surface; the shore is where the ground crosses it.
            let y = if water { sea - 0.4 } else { c };
            let light = if water { 0.9 } else { 0.55 + 0.45 / (1.0 + slope * slope).sqrt() };
            let rgb = colour(c, slope, hash(x, z));
            verts.push(Vertex {
                pos: [x as f32, y, z as f32],
                uv: [u, v],
                light,
                lm: lm(15, 0),
                tint: ((rgb[0] as u32) << 16 | (rgb[1] as u32) << 8 | rgb[2] as u32) as f32,
            });
        }
    }
    let vi = |i: usize, j: usize| (i * n + j) as u32;
    let mut cells = Vec::with_capacity((n - 1) * (n - 1));
    for i in 0..n - 1 {
        for j in 0..n - 1 {
            // The top face template (`render::mesh::FACES`): corners (x, z+1) (x+1, z+1) (x+1, z) (x, z), indices 0 1 2 0 2 3.
            let (a, b, c, d) = (vi(i, j + 1), vi(i + 1, j + 1), vi(i + 1, j), vi(i, j));
            let y = (verts[a as usize].pos[1] + verts[b as usize].pos[1] + verts[c as usize].pos[1] + verts[d as usize].pos[1]) / 4.0;
            cells.push(Cell {
                x: x0 as f32 + (i as f32 + 0.5) * step as f32,
                y,
                z: z0 as f32 + (j as f32 + 0.5) * step as f32,
                idx: [a, b, c, a, c, d],
            });
        }
    }
    let mut skirt = Vec::new();
    for k in 0..n - 1 {
        flap(&mut verts, &mut skirt, vi(k, 0), vi(k + 1, 0));
        flap(&mut verts, &mut skirt, vi(k, n - 1), vi(k + 1, n - 1));
        flap(&mut verts, &mut skirt, vi(0, k), vi(0, k + 1));
        flap(&mut verts, &mut skirt, vi(n - 1, k), vi(n - 1, k + 1));
    }
    Built { key, step, verts, cells, skirt }
}

/// The indices to draw: every cell, except those inside the hole around `eye` (if the tile is near it); the skirts only when there
/// is no hole (inside it they would hang through the real water).
fn indices(cells: &[Cell], skirt: &[u32], eye: Option<Vec3>) -> Vec<u32> {
    let mut v = Vec::with_capacity(cells.len() * 6 + skirt.len());
    for c in cells {
        if let Some(e) = eye {
            let (dx, dy, dz) = (c.x - e.x, c.y - e.y, c.z - e.z);
            if dx * dx + dy * dy + dz * dz < HOLE * HOLE && dy.abs() < HOLE_DY {
                continue;
            }
        }
        v.extend_from_slice(&c.idx);
    }
    if eye.is_none() {
        v.extend_from_slice(skirt);
    }
    v
}

/// The eye on a grid of 8 blocks: the hole is recut only when this changes.
fn quant(e: Vec3) -> (i32, i32, i32) {
    ((e.x / 8.0).floor() as i32, (e.y / 8.0).floor() as i32, (e.z / 8.0).floor() as i32)
}

struct Tile {
    step: i32,
    vbuf: wgpu::Buffer,
    ibuf: Option<wgpu::Buffer>,
    index_count: u32,
    cells: Vec<Cell>,
    skirt: Vec<u32>,
    /// `quant(eye)` the index buffer was cut for, None = no hole.
    hole: Option<(i32, i32, i32)>,
    /// Not cut yet.
    stale: bool,
}

impl Tile {
    fn cut(&mut self, device: &wgpu::Device, eye: Option<Vec3>) {
        let idx = indices(&self.cells, &self.skirt, eye);
        self.index_count = idx.len() as u32;
        self.ibuf = (!idx.is_empty()).then(|| create_index_buffer(device, &idx));
        self.hole = eye.map(quant);
        self.stale = false;
    }
}

pub struct Lod {
    tiles: HashMap<(i32, i32), Tile>,
    pending: HashSet<(i32, i32)>,
    job: Sender<(i32, i32, i32)>,
    rx: Receiver<Built>,
    /// The tiles `update` found in the frustum.
    visible: Vec<(i32, i32)>,
}

impl Lod {
    /// A LoD for the Tectonic world of `seed` (its own `Tectonic`, on its own thread, built like the generator's).
    pub fn new(seed: i64) -> Self {
        let (job, jobs) = channel::<(i32, i32, i32)>();
        let (done, rx) = channel::<Built>();
        std::thread::Builder::new()
            .name("lod".into())
            .spawn(move || {
                let t = tectonic::Tectonic::new(seed, tectonic::Config::compact(H as i32, SEA_LEVEL));
                // Ends when the `Lod` (and so `job`) is dropped.
                while let Ok((tx, tz, step)) = jobs.recv() {
                    if done.send(build(&t, (tx, tz), step)).is_err() {
                        break;
                    }
                }
            })
            .expect("lod thread");
        Lod { tiles: HashMap::new(), pending: HashSet::new(), job, rx, visible: Vec::new() }
    }

    /// Once per frame: take the finished tiles, ask for the missing or too coarse ones nearest the eye first, drop the far ones,
    /// recut the holes that moved, and list the tiles `in_frustum` (a test of a world-space box) lets through.
    pub fn update(&mut self, device: &wgpu::Device, eye: Vec3, in_frustum: impl Fn(Vec3, Vec3) -> bool) {
        while let Ok(b) = self.rx.try_recv() {
            self.pending.remove(&b.key);
            let vbuf = create_vertex_buffer(device, &b.verts);
            let t = Tile { step: b.step, vbuf, ibuf: None, index_count: 0, cells: b.cells, skirt: b.skirt, hole: None, stale: true };
            self.tiles.insert(b.key, t);
        }
        let ts = TILE as f32;
        let r = RADIUS + ts * 0.75;
        let centre = |k: (i32, i32)| ((k.0 as f32 + 0.5) * ts, (k.1 as f32 + 0.5) * ts);
        let dist = |k: (i32, i32)| {
            let (cx, cz) = centre(k);
            ((cx - eye.x).powi(2) + (cz - eye.z).powi(2)).sqrt()
        };
        let mut want: Vec<(f32, (i32, i32), i32)> = Vec::new();
        let (x0, x1) = (((eye.x - r) / ts).floor() as i32, ((eye.x + r) / ts).floor() as i32);
        let (z0, z1) = (((eye.z - r) / ts).floor() as i32, ((eye.z + r) / ts).floor() as i32);
        for tx in x0..=x1 {
            for tz in z0..=z1 {
                let k = (tx, tz);
                let d = dist(k);
                if d > r || self.pending.contains(&k) {
                    continue;
                }
                let step = step_for(d);
                if self.tiles.get(&k).map_or(true, |t| t.step > step) {
                    want.push((d, k, step));
                }
            }
        }
        want.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, k, step) in want {
            if self.pending.len() >= MAX_PENDING {
                break;
            }
            if self.job.send((k.0, k.1, step)).is_ok() {
                self.pending.insert(k);
            }
        }
        self.tiles.retain(|&k, _| dist(k) <= r + ts);
        // The hole follows the eye; a tile far from it has none.
        let q = quant(eye);
        for (&k, t) in self.tiles.iter_mut() {
            let near = dist(k) < HOLE + ts * 0.75;
            let want = if near { Some(q) } else { None };
            if t.stale || t.hole != want {
                t.cut(device, near.then_some(eye));
            }
        }
        self.visible.clear();
        for &k in self.tiles.keys() {
            let (min, max) = (Vec3::new(k.0 as f32 * ts, 0.0, k.1 as f32 * ts), Vec3::new((k.0 + 1) as f32 * ts, H as f32, (k.1 + 1) as f32 * ts));
            if in_frustum(min, max) {
                self.visible.push(k);
            }
        }
    }

    /// Into the opaque pass, after the chunks (`ChunkPipeline`'s pipeline and bind groups are already set).
    pub fn draw(&self, rp: &mut wgpu::RenderPass<'_>) {
        for k in &self.visible {
            let Some(t) = self.tiles.get(k) else { continue };
            let Some(ib) = t.ibuf.as_ref() else { continue };
            rp.set_vertex_buffer(0, t.vbuf.slice(..));
            rp.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
            rp.draw_indexed(0..t.index_count, 0, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tile is a closed grid with a skirt, every index in range, no vertex outside the world, and the hole takes out cells and
    /// the skirts only.
    #[test]
    fn a_tile_is_a_grid_with_skirts_and_a_hole() {
        let t = tectonic::Tectonic::new(0xCAFEBABE, tectonic::Config::compact(H as i32, SEA_LEVEL));
        let b = build(&t, (0, 0), 16);
        let n = 9usize;
        assert_eq!(b.cells.len(), (n - 1) * (n - 1));
        assert_eq!(b.verts.len(), n * n + 8 * (n - 1));
        assert!(b.cells.iter().all(|c| c.idx.iter().all(|&i| (i as usize) < n * n)));
        assert!(b.skirt.iter().all(|&i| (i as usize) < b.verts.len()));
        assert!(b.verts.iter().all(|v| v.pos[1] > -20.0 && v.pos[1] <= H as f32), "a vertex outside the world");
        assert!(b.verts.iter().all(|v| v.tint >= 0.0 && v.tint < 16_777_216.0));
        let all = indices(&b.cells, &b.skirt, None);
        assert_eq!(all.len(), b.cells.len() * 6 + b.skirt.len());
        let eye = Vec3::new(64.0, b.cells[(n - 1) * (n - 1) / 2].y, 64.0);
        let holed = indices(&b.cells, &b.skirt, Some(eye));
        assert!(!holed.is_empty() && holed.len() < b.cells.len() * 6, "the hole removes some cells, not all, and no skirt");
    }

    /// Water is a flat sheet, high ground is rock and snow, a beach is sand.
    #[test]
    fn colours_follow_height_and_slope() {
        let sea = SEA_LEVEL as f32;
        let deep = colour(sea - 30.0, 0.0, 0.5);
        let shallow = colour(sea - 1.0, 0.0, 0.5);
        assert!(deep[2] < shallow[2], "deeper water is darker");
        assert!(colour(110.0, 0.2, 0.5)[0] > 200.0, "a high flat is snow");
        assert!(colour(sea + 12.0, 0.2, 0.5)[1] > colour(sea + 12.0, 0.2, 0.5)[0], "lowland is green");
        let beach = colour(sea + 1.0, 0.1, 0.5);
        assert!(beach[0] > 180.0 && beach[2] > 100.0, "a beach is sand");
    }
}
