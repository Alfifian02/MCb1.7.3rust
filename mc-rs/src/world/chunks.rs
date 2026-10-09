//! Chunk manager: chunks stored by (cx, cz), one small mesh per chunk, a render-distance ring
//! that loads and unloads as the player walks, terrain generated on worker threads.
//!
//! Per frame: `stream` (cheap, no GPU) collects finished chunks, unloads far ones, asks the
//! workers for missing ones nearest-first and populates one chunk; `mesh_pending` meshes a couple
//! of chunks and uploads them.
//!
//! Three states per chunk: raw (terrain + caves, from a worker), populated (trees, ores, ...) and
//! final. populate(P) needs raw P, P+x, P+z, P+x+z and writes into all four, so a chunk C is final
//! once populate has run on C, C-x, C-z and C-x-z, and is meshed only then (and with its four
//! neighbours loaded, so border faces cull correctly). That is why the raw ring is two chunks wider
//! than the render ring. A populate that changes an already meshed chunk (or its neighbour's
//! border) just marks it for a rebuild.
//!
//! M13 frustum culling: each chunk's bounds are known from its key
//! ((cx*16, 0, cz*16) .. +(16, 128, 16)), so it is one `filter` on `meshes()` in the draw loop.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};

use glam::Vec3;

use crate::gpu::pipeline::{create_index_buffer, create_vertex_buffer};
use crate::render::mesh;
use crate::world::chunk::{height_map, idx, Nibbles, H, VOLUME};
use crate::world::gen::chunk_manager::WorldChunkManager;
use crate::world::gen::overworld::OverworldGenerator;
use crate::world::gen::populate::Region;
use crate::world::save;

mod light;

type Key = (i32, i32);

/// Multiply-rotate hasher for the chunk map. The default SipHash was the main cost of the light engine
/// (about 10 map lookups per cell evaluated); keys here are small ints, not attacker input.
#[derive(Default)]
struct KeyHasher(u64);

impl KeyHasher {
    fn add(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

impl std::hash::Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        bytes.iter().for_each(|&b| self.add(b as u64));
    }
    fn write_i32(&mut self, i: i32) {
        self.add(i as u32 as u64);
    }
}

type Chunks = HashMap<Key, Entry, std::hash::BuildHasherDefault<KeyHasher>>;

/// Chunks meshed + uploaded per frame, so walking into fresh terrain never stalls a frame.
/// ponytail: meshing runs on the render thread; if one chunk mesh is slow on a weak phone,
/// move it to the workers (they would need the neighbours' blocks behind an `Arc`).
const MESH_PER_FRAME: usize = 2;

/// Saved chunks read per frame (the rest wait for the next frame, like generation does).
/// ponytail: the read + decode runs on the render thread (about 32 KB of work per chunk); move it to the workers if it shows.
const LOADS_PER_FRAME: usize = 8;

/// Chunk coordinate of a world-space coordinate (floor division by 16).
pub fn chunk_coord(v: f32) -> i32 {
    (v.floor() as i32) >> 4
}

pub struct Mesh {
    pub vbuf: wgpu::Buffer,
    pub ibuf: wgpu::Buffer,
    pub index_count: u32,
}

struct Entry {
    blocks: Vec<u8>,
    /// Block metadata (`Chunk.data`), 4 bits per cell; worldgen only writes it in `populate`.
    data: Nibbles,
    /// Per cell, sky light in the low nibble and block light in the high nibble (same index as `blocks`).
    light: Vec<u8>,
    /// `Chunk.heightMap`, indexed `z << 4 | x`; kept in step with `blocks` (insert, populate, edits).
    height: [u8; 256],
    /// Initial light was computed (`light::init_chunk`); meshing waits for it.
    lit: bool,
    /// populate() has run on this chunk (it also wrote into its +X/+Z/+X+Z neighbours).
    populated: bool,
    /// Differs from what the seed generates (edited, or written into by a populate) and is not on disk yet.
    dirty: bool,
    /// Meshing was done (the mesh may still be `None`: a chunk of pure air has nothing to draw).
    meshed: bool,
    mesh: Option<Mesh>,
}

pub struct ChunkManager {
    chunks: Chunks,
    /// Requested from a worker, not back yet.
    pending: HashSet<Key>,
    /// Offsets of the raw ring (radius + 2), nearest first.
    ring: Vec<Key>,
    /// Render radius in chunks.
    radius: i32,
    /// Player chunk at the last unload pass.
    center: Option<Key>,
    max_in_flight: usize,
    /// populate() runs here, on the render thread, one chunk per frame.
    /// ponytail: move it to the workers if it shows up in a frame profile (it would need the 4 chunks behind a lock).
    gen: OverworldGenerator,
    cm: WorldChunkManager,
    jobs: mpsc::Sender<Key>,
    done: mpsc::Receiver<(Key, Vec<u8>)>,
    /// Pending light updates, newest last (`World.lightingToUpdate`).
    light_queue: Vec<light::Region>,
    /// `World.skylightSubtracted` (0 day .. 11 night), applied by the mesher.
    sky_sub: u8,
    sun: i32,
    /// Save folder (M7); `None` = nothing is read or written (the tests). `saved` = chunks that have a file there.
    dir: Option<PathBuf>,
    saved: HashSet<Key>,
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
                        if tx.send(((cx, cz), g.generate(cx, cz, &mut cm))).is_err() {
                            break;
                        }
                    }
                })
                .expect("spawn chunk worker");
        }

        let r = radius + 2;
        let mut ring: Vec<Key> = (-r..=r)
            .flat_map(|dx| (-r..=r).map(move |dz| (dx, dz)))
            .filter(|&(dx, dz)| dx * dx + dz * dz <= r * r)
            .collect();
        ring.sort_by_key(|&(dx, dz)| dx * dx + dz * dz);

        Self { chunks: Chunks::default(), pending: HashSet::new(), ring, radius, center: None, max_in_flight: workers * 2,
               gen: OverworldGenerator::new(seed), cm: WorldChunkManager::new(seed), jobs, done, light_queue: Vec::new(), sky_sub: 0, sun: crate::render::mesh::NO_SUN, dir: None, saved: HashSet::new() }
    }

    /// Keep this world in `dir`: chunks found there are loaded instead of generated, and edited ones are written back
    /// (`flush`, and when they unload).
    pub fn with_dir(mut self, dir: PathBuf) -> Self {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::warn!("save: cannot create {}: {e}", dir.display());
        }
        self.saved = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|f| {
                let name = f.file_name().into_string().ok()?;
                let (x, z) = name.strip_prefix("c.")?.split_once('.')?;
                Some((x.parse().ok()?, z.parse().ok()?)) // "c.1.2.tmp" and other strays do not parse
            })
            .collect();
        self.dir = Some(dir);
        self
    }

    fn insert(&mut self, key: Key, blocks: Vec<u8>) {
        let height = height_map(&blocks);
        self.chunks.insert(key, Entry { blocks, data: Nibbles::new(), light: vec![0; VOLUME], height, lit: false, populated: false, dirty: false, meshed: false, mesh: None });
    }

    /// Load a saved chunk. A file that does not decode is forgotten, so the chunk is generated instead.
    fn load(&mut self, key: Key) {
        let bytes = self.dir.as_ref().and_then(|d| std::fs::read(chunk_path(d, key)).ok());
        match bytes.as_deref().and_then(save::decode_chunk) {
            Some((blocks, data, populated)) => {
                self.insert(key, blocks);
                if let Some(e) = self.chunks.get_mut(&key) {
                    e.data = data;
                    e.populated = populated;
                }
            }
            None => {
                log::warn!("save: chunk {key:?} unreadable, generating it again");
                self.saved.remove(&key);
            }
        }
    }

    /// The chunk from disk when there is a save of it, else freshly generated (synchronously).
    fn fresh(&mut self, key: Key) {
        if self.saved.contains(&key) {
            self.load(key);
        }
        if !self.chunks.contains_key(&key) {
            let blocks = self.gen.generate(key.0, key.1, &mut self.cm);
            self.insert(key, blocks);
        }
    }

    /// Write up to `budget` unsaved chunks (`usize::MAX` = all, for pause and exit). A failed write leaves the chunk
    /// dirty for the next call and stops this one, so a full disk costs one log line per call, not one per chunk.
    pub fn flush(&mut self, mut budget: usize) {
        let Some(dir) = &self.dir else { return };
        for (&k, e) in self.chunks.iter_mut().filter(|(_, e)| e.dirty) {
            if budget == 0 {
                break;
            }
            budget -= 1;
            match write_entry(dir, k, e) {
                Ok(()) => {
                    e.dirty = false;
                    self.saved.insert(k);
                }
                Err(err) => {
                    log::warn!("save: chunk {k:?}: {err}");
                    break;
                }
            }
        }
    }

    /// Generate (or load) and populate the 4x4 chunks `cx-1..=cx+2`, `cz-1..=cz+2` synchronously, for the spawn area:
    /// init needs real terrain before the first frame. Only chunks whose 2x2 is inside get populated, which finishes
    /// the 2x2 chunks `cx..=cx+1`, `cz..=cz+1`.
    pub fn preload(&mut self, cx: i32, cz: i32) {
        for z in cz - 1..=cz + 2 {
            for x in cx - 1..=cx + 2 {
                self.fresh((x, z));
            }
        }
        for z in cz - 1..cz + 2 {
            for x in cx - 1..cx + 2 {
                self.populate_one(x, z);
            }
        }
    }

    /// Block at world coordinates. `None` above/below the world (like the old bounds check);
    /// a chunk that is not loaded reads as solid, so the player is walled in at the edge of the
    /// loaded area instead of walking or falling into void.
    pub fn block(&self, x: i32, y: i32, z: i32) -> Option<u8> {
        (0..H as i32).contains(&y).then(|| self.block_loaded(x, y, z).unwrap_or(1))
    }

    /// Block at world coordinates, `None` when out of the world or the chunk is not loaded (picking
    /// must not hit the invisible wall `block` builds at the edge).
    pub fn block_loaded(&self, x: i32, y: i32, z: i32) -> Option<u8> {
        if !(0..H as i32).contains(&y) {
            return None;
        }
        self.chunks.get(&(x >> 4, z >> 4)).map(|e| e.blocks[idx((x & 15) as usize, y as usize, (z & 15) as usize)])
    }

    /// Block metadata at world coordinates (`World.getBlockMetadata`); 0 above/below the world and in an
    /// unloaded chunk. A caller that breaks a block reads this first: `set_block` clears it.
    pub fn meta(&self, x: i32, y: i32, z: i32) -> u8 {
        if !(0..H as i32).contains(&y) {
            return 0;
        }
        self.chunks.get(&(x >> 4, z >> 4)).map_or(0, |e| e.data.get((x & 15) as usize, y as usize, (z & 15) as usize))
    }

    /// Break or place a block with metadata 0 (`Chunk.setBlockID`): nothing when the cell already holds `id`,
    /// else the id is written and the metadata cleared. False if nothing changed.
    pub fn set_block(&mut self, x: i32, y: i32, z: i32, id: u8) -> bool {
        self.block_loaded(x, y, z) != Some(id) && self.set_block_meta(x, y, z, id, 0)
    }

    /// Place a block with metadata (`Chunk.setBlockIDWithMetadata`, `meta` is 0..=15): writes the cell, keeps
    /// the height map and light in step and marks the affected meshes for a rebuild. False if nothing changed.
    pub fn set_block_meta(&mut self, x: i32, y: i32, z: i32, id: u8, meta: u8) -> bool {
        if !(0..H as i32).contains(&y) {
            return false;
        }
        let Some(e) = self.chunks.get_mut(&(x >> 4, z >> 4)) else { return false };
        let (lx, lz) = ((x & 15) as usize, (z & 15) as usize);
        let i = idx(lx, y as usize, lz);
        let meta = meta & 15;
        if e.blocks[i] == id && e.data.get(lx, y as usize, lz) == meta {
            return false;
        }
        e.blocks[i] = id;
        e.data.set(lx, y as usize, lz, meta);
        e.dirty = true;
        let h = e.height[lz << 4 | lx] as i32;
        self.mark_dirty(x, z);
        self.light_after_set(x, y, z, id, h);
        true
    }

    /// The chunk holding world column (x, z) needs a new mesh, and so does the neighbour whose border
    /// faces look at it when the column is on the chunk edge.
    fn mark_dirty(&mut self, x: i32, z: i32) {
        let (cx, cz, lx, lz) = (x >> 4, z >> 4, x & 15, z & 15);
        let mut keys = vec![(cx, cz)];
        if lx == 0 { keys.push((cx - 1, cz)) }
        if lx == 15 { keys.push((cx + 1, cz)) }
        if lz == 0 { keys.push((cx, cz - 1)) }
        if lz == 15 { keys.push((cx, cz + 1)) }
        for k in keys {
            if let Some(e) = self.chunks.get_mut(&k) {
                e.meshed = false;
            }
        }
    }

    /// Chunk C is final when populate has run on C, C-x, C-z and C-x-z.
    fn is_final(&self, x: i32, z: i32) -> bool {
        [(x, z), (x - 1, z), (x, z - 1), (x - 1, z - 1)].iter().all(|k| self.chunks.get(k).is_some_and(|e| e.populated))
    }

    /// Time of day changed the light: every mesh is stale (`updateAllRenderers`) and is rebuilt a
    /// couple per frame, nearest first; the old meshes keep drawing until their replacement is ready.
    pub fn set_sky_sub(&mut self, v: u8) {
        if v != self.sky_sub {
            self.sky_sub = v;
            self.chunks.values_mut().for_each(|e| e.meshed = false);
        }
    }

    /// Sun shadow direction changed (`sky::sun_key`): every mesh is stale, like `set_sky_sub`.
    pub fn set_sun(&mut self, v: i32) {
        if v != self.sun {
            self.sun = v;
            self.chunks.values_mut().for_each(|e| e.meshed = false);
        }
    }

    /// `WorldChunkManager.getTemperature` at a block column (for the sky colour).
    pub fn temperature_at(&mut self, x: i32, z: i32) -> f64 {
        self.cm.load_block_generator_data(x, z, 1, 1);
        self.cm.temperature[0]
    }

    /// Everything that has a mesh, for the draw loop.
    pub fn meshes(&self) -> impl Iterator<Item = &Mesh> {
        self.chunks.values().filter_map(|e| e.mesh.as_ref())
    }

    /// The meshes whose chunk box (16 x `H` x 16 at its key) passes `visible(min, max)`: the draw loop's frustum test.
    pub fn meshes_where<'a>(&'a self, visible: impl Fn(Vec3, Vec3) -> bool + 'a) -> impl Iterator<Item = &'a Mesh> {
        self.chunks.iter().filter_map(move |(&(x, z), e)| {
            let min = Vec3::new(x as f32 * 16.0, 0.0, z as f32 * 16.0);
            e.mesh.as_ref().filter(|_| visible(min, min + Vec3::new(16.0, H as f32, 16.0)))
        })
    }

    /// Chunks that currently have something to draw.
    pub fn meshed(&self) -> usize {
        self.meshes().count()
    }

    pub fn loaded(&self) -> usize {
        self.chunks.len()
    }

    /// Call once per frame with the player's chunk.
    pub fn update(&mut self, device: &wgpu::Device, cx: i32, cz: i32) {
        self.stream(cx, cz);
        self.populate_pending(cx, cz, 1);
        self.light_pending(cx, cz);
        // Meshes wait for the light queue to drain, so an edit or a new chunk is meshed once, lit.
        if self.light_queue.is_empty() {
            self.mesh_pending(device, cx, cz);
        }
    }

    fn stream(&mut self, cx: i32, cz: i32) {
        // Keep radius: raw ring + 1, so walking along the edge does not thrash.
        let keep = (self.radius + 3) * (self.radius + 3);
        let d2 = |(x, z): Key| (x - cx) * (x - cx) + (z - cz) * (z - cz);

        // 1. Collect finished chunks; ones the player already walked away from are dropped.
        while let Ok((key, blocks)) = self.done.try_recv() {
            self.pending.remove(&key);
            if d2(key) <= keep {
                self.insert(key, blocks);
            }
        }

        // 2. Unload, only when the player crossed a chunk border. Dropping an entry frees its buffers; one that is
        // not on disk yet is written first.
        // ponytail: the writes run on the render thread, a handful of ~5 KB files per border crossing.
        if self.center != Some((cx, cz)) {
            self.center = Some((cx, cz));
            let gone: Vec<Key> = self.chunks.keys().copied().filter(|&k| d2(k) > keep).collect();
            for k in gone {
                let Some(e) = self.chunks.remove(&k) else { continue };
                if let (true, Some(dir)) = (e.dirty, &self.dir) {
                    match write_entry(dir, k, &e) {
                        Ok(()) => {
                            self.saved.insert(k);
                        }
                        Err(err) => log::warn!("save: chunk {k:?} lost on unload: {err}"),
                    }
                }
            }
        }

        // 3. Missing chunks nearest-first: a saved one is read from disk (a few per frame), any other is requested
        // from a worker. Only a few jobs are in flight, and the list is rebuilt from the current position every
        // frame, so a worker never queues stale requests.
        let mut loads = LOADS_PER_FRAME;
        for i in 0..self.ring.len() {
            let key = (cx + self.ring[i].0, cz + self.ring[i].1);
            if self.chunks.contains_key(&key) {
                continue;
            }
            if self.saved.contains(&key) {
                if loads > 0 {
                    loads -= 1;
                    self.load(key);
                }
            } else if self.pending.len() < self.max_in_flight && self.pending.insert(key) {
                let _ = self.jobs.send(key);
            }
        }
    }

    /// Run populate on up to `budget` chunks, nearest to (cx, cz) first, whose 2x2 block is loaded.
    fn populate_pending(&mut self, cx: i32, cz: i32, mut budget: usize) {
        for i in 0..self.ring.len() {
            if budget == 0 {
                break;
            }
            if self.populate_one(cx + self.ring[i].0, cz + self.ring[i].1) {
                budget -= 1;
            }
        }
    }

    /// populate chunk (x, z) if it is not yet and its 2x2 block is loaded. True if it ran.
    fn populate_one(&mut self, x: i32, z: i32) -> bool {
        let keys = [(x, z), (x, z + 1), (x + 1, z), (x + 1, z + 1)];
        if self.chunks.get(&keys[0]).map_or(true, |e| e.populated) || !keys.iter().all(|k| self.chunks.contains_key(k)) {
            return false;
        }
        let blocks = keys.map(|k| std::mem::take(&mut self.chunks.get_mut(&k).unwrap().blocks));
        let data = keys.map(|k| std::mem::take(&mut self.chunks.get_mut(&k).unwrap().data));
        let mut region = Region::new(x, z, blocks, data);
        self.gen.populate(&mut region, x, z, &mut self.cm);
        let (blocks, data) = region.into_parts();
        for ((k, b), d) in keys.iter().zip(blocks).zip(data) {
            let e = self.chunks.get_mut(k).unwrap();
            e.height = height_map(&b);
            e.blocks = b;
            e.data = d;
            e.dirty = true; // a populate writes into all four (trees, ores, ... spill over the borders)
            e.lit = false; // blocks changed under the light: it is computed again once final
            // The chunk and its four neighbours (border faces) need a new mesh if they had one.
            for n in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                if let Some(e) = self.chunks.get_mut(&(k.0 + n.0, k.1 + n.1)) {
                    e.meshed = false;
                }
            }
        }
        self.chunks.get_mut(&keys[0]).unwrap().populated = true;
        true
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
                (Some(me), Some(px), Some(nx), Some(pz), Some(nz)) if !me.meshed && me.lit && self.is_final(x, z) => Some(mesh::build(
                    &me.blocks,
                    &me.data,
                    &me.light,
                    [px.blocks.as_slice(), nx.blocks.as_slice(), pz.blocks.as_slice(), nz.blocks.as_slice()],
                    [px.light.as_slice(), nx.light.as_slice(), pz.light.as_slice(), nz.light.as_slice()],
                    self.sky_sub,
                    self.sun,
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

fn chunk_path(dir: &Path, (x, z): Key) -> PathBuf {
    dir.join(format!("c.{x}.{z}"))
}

fn write_entry(dir: &Path, key: Key, e: &Entry) -> std::io::Result<()> {
    save::write(&chunk_path(dir, key), &save::encode_chunk(&e.blocks, &e.data, e.populated))
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

    /// Metadata follows the Java setters: `set_block_meta` writes id and metadata, `set_block` does nothing on the
    /// same id and clears the metadata when the id changes, and an unloaded chunk reads 0.
    #[test]
    fn metadata_follows_chunk_setters() {
        let mut m = ChunkManager::new(1, 1);
        m.insert((0, 0), vec![0u8; VOLUME]);
        assert!(m.set_block_meta(3, 40, 5, 35, 14)); // red wool
        assert_eq!((m.block(3, 40, 5), m.meta(3, 40, 5)), (Some(35), 14));
        assert!(!m.set_block_meta(3, 40, 5, 35, 14)); // nothing changed
        assert!(m.set_block_meta(3, 40, 5, 35, 3 | 16)); // same id, new metadata (only 4 bits)
        assert_eq!(m.meta(3, 40, 5), 3);
        assert!(!m.set_block(3, 40, 5, 35)); // same id: untouched
        assert_eq!(m.meta(3, 40, 5), 3);
        assert!(m.set_block(3, 40, 5, 0)); // break: the metadata is cleared
        assert_eq!((m.block(3, 40, 5), m.meta(3, 40, 5)), (Some(0), 0));
        assert_eq!(m.meta(100, 40, 5), 0); // unloaded
        assert!(!m.set_block_meta(3, 128, 5, 35, 1)); // above the world
    }

    /// preload populates every chunk whose 2x2 is loaded, which finishes the inner ones.
    #[test]
    fn preload_populates_and_finishes_inner_chunks() {
        let mut m = ChunkManager::new(1, 1);
        m.preload(0, 0);
        for z in -1..=1 {
            for x in -1..=1 {
                assert!(m.chunks[&(x, z)].populated, "({x}, {z})");
            }
        }
        assert!(!m.chunks[&(2, 2)].populated);
        assert!(m.is_final(0, 0) && m.is_final(1, 1) && !m.is_final(2, 2));
    }

    /// An edit survives a restart: flushed, then read back by a fresh manager that loads (does not generate) the chunk.
    /// A chunk nobody touched is not written, and a corrupt file is forgotten instead of loaded.
    #[test]
    fn edits_survive_a_restart() {
        let dir = std::env::temp_dir().join(format!("mc-rs-save-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut a = ChunkManager::new(1, 1).with_dir(dir.clone());
        a.insert((0, 0), vec![0u8; VOLUME]);
        a.insert((1, 0), vec![0u8; VOLUME]);
        assert!(a.set_block_meta(3, 40, 5, 35, 14)); // red wool in chunk (0, 0)
        a.chunks.get_mut(&(0, 0)).unwrap().populated = true;
        a.flush(usize::MAX);
        assert!(!a.chunks[&(0, 0)].dirty && a.saved.contains(&(0, 0)) && !a.saved.contains(&(1, 0)));

        let mut b = ChunkManager::new(1, 1).with_dir(dir.clone());
        assert!(b.saved.contains(&(0, 0)) && b.saved.len() == 1);
        b.load((0, 0));
        assert_eq!((b.block(3, 40, 5), b.meta(3, 40, 5)), (Some(35), 14));
        assert!(b.chunks[&(0, 0)].populated && !b.chunks[&(0, 0)].dirty);

        std::fs::write(dir.join("c.0.0"), [1u8, 2, 3]).unwrap();
        let mut c = ChunkManager::new(1, 1).with_dir(dir.clone());
        c.load((0, 0));
        assert!(!c.saved.contains(&(0, 0)) && !c.chunks.contains_key(&(0, 0)));
        let _ = std::fs::remove_dir_all(&dir);
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
