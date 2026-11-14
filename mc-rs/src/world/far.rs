//! Distant terrain: the `lod` crate's height-column tiles, generated, kept and drawn beyond the real chunks.
//!
//! - Data: a worker thread runs the world generator without caves (`OverworldGenerator::generate_far`) for the chunks around the
//!   player, nearest first, and keeps the top block of every column (`columns`). A chunk the game unloads hands over its real
//!   columns (`ChunkManager::take_unloaded`: trees, edits), which replace the generated ones. The tiles live in `<world>/far`.
//! - Drawing: `lod::Store::select` picks the tiles per frame (finer near, coarser far), each is meshed once into the terrain
//!   `Vertex` format with the flat-colour tile of its block, and drawn by the terrain pipeline (`draw`) in the same pass as the
//!   chunks, so fog, sky and lighting treat it as terrain. It is not drawn into the shadow map.
//!
//! ponytail: the worker builds the land only: no trees, no caves, no structures (they pop in as the real chunks load); one worker
//! thread, so a first walk fills the ring slowly and a later one reads it from the file; the tile meshes are uploaded 4 a frame.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use glam::Vec3;
use lod::{Cell, Key, Store, Tile};

use crate::gpu::pipeline::{create_index_buffer, create_vertex_buffer};
use crate::render::mesh;
use crate::world::chunk::{idx, is_fluid, opaque, H};
use crate::world::gen::chunk_manager::WorldChunkManager;
use crate::world::gen::overworld::OverworldGenerator;

/// How far the far terrain reaches, in chunks / blocks. Fog and the far plane follow it (`lib.rs`).
pub const FAR_CHUNKS: i32 = 14;
pub const FAR_BLOCKS: f32 = (FAR_CHUNKS * 16) as f32;
/// Chunks the worker has been asked for and has not returned.
const MAX_IN_FLIGHT: usize = 2;
/// Tile meshes built and uploaded per frame.
const MESHES_PER_FRAME: usize = 4;
/// Above this many cached tile meshes, those not drawn this frame are dropped.
const MESH_CAP: usize = 400;

/// The 16 x 16 columns of a chunk as far-terrain cells: the highest block that is solid (`opaque`: leaves, glass and ice count,
/// plants, torches and the like are looked through), lava included, and the top of the water above it.
pub fn columns(blocks: &[u8]) -> Tile {
    let mut t = lod::empty_tile();
    for z in 0..16 {
        for x in 0..16 {
            let base = idx(x, 0, z);
            let (mut water, mut ground, mut id) = (0u8, 0u8, 1u8); // an empty column is stone at the bottom
            for y in (0..H).rev() {
                let b = blocks[base + y];
                if b == 0 || (!opaque(b) && !is_fluid(b)) {
                    continue;
                }
                if matches!(b, 8 | 9) {
                    if water == 0 {
                        water = y as u8;
                    }
                    continue;
                }
                (ground, id) = (y as u8, b);
                break;
            }
            t[z * 16 + x] = Cell::new(ground, id, water);
        }
    }
    t
}

struct GpuMesh {
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    index_count: u32,
}

/// `RenderBlocks`' face shade for a `render::mesh` face index (+X, -X, +Y, -Y, +Z, -Z).
fn shade(face: u8) -> f32 {
    match face {
        2 => 1.0,
        3 => 0.5,
        4 | 5 => 0.8,
        _ => 0.6,
    }
}

/// `None`: the tile has nothing to draw.
fn build(device: &wgpu::Device, quads: &[lod::Quad]) -> Option<GpuMesh> {
    if quads.is_empty() {
        return None;
    }
    let (mut verts, mut idxs) = (Vec::new(), Vec::new());
    for q in quads {
        mesh::push_face(&mut verts, &mut idxs, q.face as usize, q.min, q.max, q.id as u16, shade(q.face));
    }
    Some(GpuMesh { vbuf: create_vertex_buffer(device, bytemuck::cast_slice(&verts)), ibuf: create_index_buffer(device, &idxs), index_count: idxs.len() as u32 })
}

pub struct Far {
    store: Store,
    path: PathBuf,
    jobs: mpsc::Sender<(i32, i32)>,
    done: mpsc::Receiver<((i32, i32), Tile)>,
    pending: HashSet<(i32, i32)>,
    /// Chunk offsets within `FAR_CHUNKS`, nearest first.
    ring: Vec<(i32, i32)>,
    meshes: HashMap<Key, Option<GpuMesh>>,
    /// Tiles whose cached mesh is out of date: still drawn until the new one is built.
    stale: HashSet<Key>,
    picked: Vec<Key>,
    scratch: Vec<Key>,
}

impl Far {
    /// The far terrain of the world in `dir` (what an earlier run saved there is loaded). The worker ends when this is dropped.
    pub fn new(seed: i64, tectonic: bool, dir: &Path) -> Self {
        let (jobs, job_rx) = mpsc::channel::<(i32, i32)>();
        let (done_tx, done) = mpsc::channel();
        std::thread::Builder::new()
            .name("far-gen".into())
            .spawn(move || {
                let mut g = OverworldGenerator::new(seed);
                if tectonic {
                    g = g.tectonic();
                }
                let mut cm = WorldChunkManager::new(seed);
                while let Ok((x, z)) = job_rx.recv() {
                    let t = columns(&g.generate_far(x, z, &mut cm));
                    if done_tx.send(((x, z), t)).is_err() {
                        break;
                    }
                }
            })
            .expect("spawn far worker");
        let mut ring: Vec<(i32, i32)> = (-FAR_CHUNKS..=FAR_CHUNKS)
            .flat_map(|dx| (-FAR_CHUNKS..=FAR_CHUNKS).map(move |dz| (dx, dz)))
            .filter(|&(dx, dz)| dx * dx + dz * dz <= FAR_CHUNKS * FAR_CHUNKS)
            .collect();
        ring.sort_by_key(|&(dx, dz)| dx * dx + dz * dz);
        let path = dir.join("far");
        let mut store = Store::load(&path);
        store.take_dirty(); // nothing is meshed yet
        Self { store, path, jobs, done, pending: HashSet::new(), ring, meshes: HashMap::new(), stale: HashSet::new(), picked: Vec::new(), scratch: Vec::new() }
    }

    /// Once a frame: take in what the worker finished and the real columns of unloaded chunks (`unloaded`), ask the worker for
    /// the nearest chunks it has not done, pick the tiles for `eye` (outside `near` blocks, in the frustum) and mesh a few.
    pub fn update(&mut self, device: &wgpu::Device, eye: Vec3, unloaded: Vec<((i32, i32), Tile)>, near: f32, in_frustum: impl Fn(Vec3, Vec3) -> bool) {
        for ((x, z), t) in unloaded {
            self.store.set(x, z, t); // the real chunk wins over a generated one
        }
        while let Ok(((x, z), t)) = self.done.try_recv() {
            self.pending.remove(&(x, z));
            if !self.store.has(x, z) {
                self.store.set(x, z, t);
            }
        }
        let (cx, cz) = ((eye.x.floor() as i32) >> 4, (eye.z.floor() as i32) >> 4);
        for &(dx, dz) in &self.ring {
            if self.pending.len() >= MAX_IN_FLIGHT {
                break;
            }
            let k = (cx + dx, cz + dz);
            if !self.store.has(k.0, k.1) && !self.pending.contains(&k) && self.jobs.send(k).is_ok() {
                self.pending.insert(k);
            }
        }
        self.stale.extend(self.store.take_dirty());

        self.store.select((eye.x, eye.z), near, FAR_BLOCKS, &mut self.scratch);
        self.picked.clear();
        let mut built = 0;
        for &k in &self.scratch {
            let (l, x, z) = k;
            let size = (16 << l) as f32;
            if !in_frustum(Vec3::new(x as f32 * size, 0.0, z as f32 * size), Vec3::new((x + 1) as f32 * size, H as f32, (z + 1) as f32 * size)) {
                continue;
            }
            if (!self.meshes.contains_key(&k) || self.stale.contains(&k)) && built < MESHES_PER_FRAME {
                built += 1;
                self.stale.remove(&k);
                self.meshes.insert(k, build(device, &self.store.mesh(k)));
            }
            self.picked.push(k);
        }
        if self.meshes.len() > MESH_CAP {
            let keep: HashSet<Key> = self.scratch.iter().copied().collect();
            self.meshes.retain(|k, _| keep.contains(k));
        }
    }

    /// Draw the tiles `update` picked; the terrain pipeline and its bind groups are already set.
    pub fn draw(&self, rp: &mut wgpu::RenderPass<'_>) {
        for k in &self.picked {
            if let Some(Some(m)) = self.meshes.get(k) {
                rp.set_vertex_buffer(0, m.vbuf.slice(..));
                rp.set_index_buffer(m.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(0..m.index_count, 0, 0..1);
            }
        }
    }

    /// Write the tiles to `<world>/far` (nothing if unchanged).
    pub fn save(&mut self) {
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = self.store.save(&self.path) {
            log::warn!("save: far terrain: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::chunk::VOLUME;

    /// The top solid block wins; plants are looked through; water is a level above its floor; lava is ground; air is bedrock.
    #[test]
    fn columns_read_the_top_block_and_the_water() {
        let mut b = vec![0u8; VOLUME];
        (0..=40).for_each(|y| b[idx(3, y, 5)] = 1);
        b[idx(3, 41, 5)] = 2; // grass
        b[idx(3, 42, 5)] = 31; // tall grass
        (0..=30).for_each(|y| b[idx(0, y, 0)] = 12);
        (31..=40).for_each(|y| b[idx(0, y, 0)] = 9);
        b[idx(2, 50, 2)] = 11; // lava on top
        let t = columns(&b);
        assert_eq!((t[5 * 16 + 3].ground(), t[5 * 16 + 3].id(), t[5 * 16 + 3].water()), (41, 2, 0));
        assert_eq!((t[0].ground(), t[0].id(), t[0].water()), (30, 12, 40));
        assert_eq!((t[2 * 16 + 2].ground(), t[2 * 16 + 2].id()), (50, 11));
        assert_eq!((t[16 + 1].ground(), t[16 + 1].id()), (0, 1));
    }
}
