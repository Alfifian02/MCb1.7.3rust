//! Chunk manager: chunks stored by (cx, cz), one small mesh per chunk, a render-distance ring
//! that loads and unloads as the player walks, terrain generated on worker threads.
//!
//! Per frame: `stream` (cheap, no GPU) collects finished chunks, unloads far ones and asks the
//! workers for missing ones nearest-first; `mesh_pending` meshes a couple of chunks and uploads
//! them. A chunk is meshed only once its four neighbours are loaded, so border faces are culled
//! correctly. That is why the generate ring is one chunk wider than the render ring.
//!
//! M13 frustum culling: each chunk's bounds are known from its key
//! ((cx*16, 0, cz*16) .. +(16, 128, 16)), so it is one `filter` on `meshes()` in the draw loop.

use std::collections::{HashMap, HashSet};
use std::sync::{mpsc, Arc, Mutex};

use crate::gpu::pipeline::{create_index_buffer, create_vertex_buffer};
use crate::render::mesh;
use crate::world::chunk::{idx, H};
use crate::world::gen::chunk_manager::WorldChunkManager;
use crate::world::gen::overworld::OverworldGenerator;

type Key = (i32, i32);

/// Chunks meshed + uploaded per frame, so walking into fresh terrain never stalls a frame.
/// ponytail: meshing runs on the render thread; if one chunk mesh is slow on a weak phone,
/// move it to the workers (they would need the neighbours' blocks behind an `Arc`).
const MESH_PER_FRAME: usize = 2;

/// Chunk coordinate of a world-space coordinate (floor division by 16).
pub fn chunk_coord(v: f32) -> i32 {
    (v.floor() as i32) >> 4
}

/// Terrain for one chunk: provideChunk passes, then the (still unfaithful) M3 ore placer.
pub fn generate(g: &mut OverworldGenerator, cm: &mut WorldChunkManager, cx: i32, cz: i32) -> Vec<u8> {
    let mut blocks = g.generate(cx, cz, cm);
    g.populate_ores(&mut blocks, (cx, cz)); // M4d replaces this with the real populate()
    blocks
}

pub struct Mesh {
    pub vbuf: wgpu::Buffer,
    pub ibuf: wgpu::Buffer,
    pub index_count: u32,
}

struct Entry {
    blocks: Vec<u8>,
    /// Meshing was done (the mesh may still be `None`: a chunk of pure air has nothing to draw).
    meshed: bool,
    mesh: Option<Mesh>,
}

pub struct ChunkManager {
    chunks: HashMap<Key, Entry>,
    /// Requested from a worker, not back yet.
    pending: HashSet<Key>,
    /// Offsets of the generate ring (radius + 1), nearest first.
    ring: Vec<Key>,
    /// Render radius in chunks.
    radius: i32,
    /// Player chunk at the last unload pass.
    center: Option<Key>,
    max_in_flight: usize,
    jobs: mpsc::Sender<Key>,
    done: mpsc::Receiver<(Key, Vec<u8>)>,
}

impl ChunkManager {
    /// `radius` is the render distance in chunks. Dropping the manager closes the job channel,
    /// which ends the workers.
    pub fn new(seed: i64, radius: i32) -> Self {
        let (jobs, job_rx) = mpsc::channel::<Key>();
        let (done_tx, done) = mpsc::channel();
        let job_rx = Arc::new(Mutex::new(job_rx));
        // Leave a core for the render thread; two workers are plenty for walking speed.
        let workers = std::thread::available_parallelism().map_or(2, |n| n.get()).saturating_sub(1).clamp(1, 2);
        for i in 0..workers {
            let (rx, tx) = (job_rx.clone(), done_tx.clone());
            std::thread::Builder::new()
                .name(format!("chunk-gen-{i}"))
                .spawn(move || {
                    // Each worker owns its generator: generate() reseeds per chunk, so the
                    // result does not depend on which worker gets which chunk.
                    let mut g = OverworldGenerator::new(seed);
                    let mut cm = WorldChunkManager::new(seed);
                    loop {
                        // The guard is a temporary of this statement: the lock is held while
                        // waiting for a job, not while generating.
                        let job = rx.lock().unwrap_or_else(|e| e.into_inner()).recv();
                        let Ok((cx, cz)) = job else { break };
                        if tx.send(((cx, cz), generate(&mut g, &mut cm, cx, cz))).is_err() {
                            break;
                        }
                    }
                })
                .expect("spawn chunk worker");
        }

        let r = radius + 1;
        let mut ring: Vec<Key> = (-r..=r)
            .flat_map(|dx| (-r..=r).map(move |dz| (dx, dz)))
            .filter(|&(dx, dz)| dx * dx + dz * dz <= r * r)
            .collect();
        ring.sort_by_key(|&(dx, dz)| dx * dx + dz * dz);

        Self { chunks: HashMap::new(), pending: HashSet::new(), ring, radius, center: None, max_in_flight: workers * 2, jobs, done }
    }

    /// Add an already generated chunk (the spawn area, made synchronously during init).
    pub fn insert(&mut self, key: Key, blocks: Vec<u8>) {
        self.chunks.insert(key, Entry { blocks, meshed: false, mesh: None });
    }

    /// Block at world coordinates. `None` above/below the world (like the old bounds check);
    /// a chunk that is not loaded reads as solid, so the player is walled in at the edge of the
    /// loaded area instead of walking or falling into void.
    pub fn block(&self, x: i32, y: i32, z: i32) -> Option<u8> {
        if !(0..H as i32).contains(&y) {
            return None;
        }
        Some(match self.chunks.get(&(x >> 4, z >> 4)) {
            Some(e) => e.blocks[idx((x & 15) as usize, y as usize, (z & 15) as usize)],
            None => 1,
        })
    }

    /// Everything that has a mesh, for the draw loop.
    pub fn meshes(&self) -> impl Iterator<Item = &Mesh> {
        self.chunks.values().filter_map(|e| e.mesh.as_ref())
    }

    pub fn loaded(&self) -> usize {
        self.chunks.len()
    }

    /// Call once per frame with the player's chunk.
    pub fn update(&mut self, device: &wgpu::Device, cx: i32, cz: i32) {
        self.stream(cx, cz);
        self.mesh_pending(device, cx, cz);
    }

    fn stream(&mut self, cx: i32, cz: i32) {
        // Keep radius: generate ring + 1, so walking along the edge does not thrash.
        let keep = (self.radius + 2) * (self.radius + 2);
        let d2 = |(x, z): Key| (x - cx) * (x - cx) + (z - cz) * (z - cz);

        // 1. Collect finished chunks; ones the player already walked away from are dropped.
        while let Ok((key, blocks)) = self.done.try_recv() {
            self.pending.remove(&key);
            if d2(key) <= keep {
                self.insert(key, blocks);
            }
        }

        // 2. Unload, only when the player crossed a chunk border. Dropping an entry frees its buffers.
        if self.center != Some((cx, cz)) {
            self.center = Some((cx, cz));
            self.chunks.retain(|&k, _| d2(k) <= keep);
        }

        // 3. Request missing chunks nearest-first. Only a few jobs are in flight, and the list is
        // rebuilt from the current position every frame, so a worker never queues stale requests.
        for &(dx, dz) in &self.ring {
            if self.pending.len() >= self.max_in_flight {
                break;
            }
            let key = (cx + dx, cz + dz);
            if !self.chunks.contains_key(&key) && self.pending.insert(key) {
                let _ = self.jobs.send(key);
            }
        }
    }

    fn mesh_pending(&mut self, device: &wgpu::Device, cx: i32, cz: i32) {
        let r2 = self.radius * self.radius;
        let mut budget = MESH_PER_FRAME;
        for &(dx, dz) in &self.ring {
            if budget == 0 || dx * dx + dz * dz > r2 {
                break; // the ring is sorted by distance, so everything after is outside too
            }
            let (x, z) = (cx + dx, cz + dz);
            let c = &self.chunks;
            let built = match (c.get(&(x, z)), c.get(&(x + 1, z)), c.get(&(x - 1, z)), c.get(&(x, z + 1)), c.get(&(x, z - 1))) {
                (Some(me), Some(px), Some(nx), Some(pz), Some(nz)) if !me.meshed => Some(mesh::build(
                    &me.blocks,
                    [px.blocks.as_slice(), nx.blocks.as_slice(), pz.blocks.as_slice(), nz.blocks.as_slice()],
                    x * 16,
                    z * 16,
                )),
                _ => None,
            };
            let Some((verts, idxs)) = built else { continue };
            budget -= 1;
            let mesh = (!idxs.is_empty()).then(|| Mesh {
                vbuf: create_vertex_buffer(device, bytemuck::cast_slice(&verts)),
                ibuf: create_index_buffer(device, &idxs),
                index_count: idxs.len() as u32,
            });
            if let Some(e) = self.chunks.get_mut(&(x, z)) {
                e.mesh = mesh;
                e.meshed = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::chunk::VOLUME;
    use std::time::{Duration, Instant};

    /// World coordinates (negative too) map to the right chunk cell; unloaded reads as solid.
    #[test]
    fn block_lookup_across_chunks() {
        let mut m = ChunkManager::new(1, 1);
        let mut b = vec![0u8; VOLUME];
        b[idx(15, 5, 0)] = 7; // local (15, 5, 0) of chunk (-1, 0) = world (-1, 5, 0)
        m.insert((-1, 0), b);
        assert_eq!(m.block(-1, 5, 0), Some(7));
        assert_eq!(m.block(-2, 5, 0), Some(0));
        assert_eq!(m.block(0, 5, 0), Some(1)); // chunk (0, 0) is not loaded
        assert_eq!(m.block(-1, 200, 0), None);
    }

    /// The ring around the player fills in from the workers; after a long walk the old area is gone.
    #[test]
    fn ring_loads_then_unloads_when_walking() {
        let mut m = ChunkManager::new(1, 1);
        let settle = |m: &mut ChunkManager, c: Key| {
            let t = Instant::now();
            while !m.ring.iter().all(|&(dx, dz)| m.chunks.contains_key(&(c.0 + dx, c.1 + dz))) {
                assert!(t.elapsed() < Duration::from_secs(180), "ring did not load");
                m.stream(c.0, c.1);
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        settle(&mut m, (0, 0));
        settle(&mut m, (10, 0));
        assert!(!m.chunks.contains_key(&(0, 0)));
    }
}
