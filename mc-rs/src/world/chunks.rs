//! Chunk manager: chunks stored by (cx, cz), one small mesh per chunk, a render-distance ring
//! that loads and unloads as the player walks, terrain generated on worker threads.
//!
//! Per frame: `stream` (cheap, no GPU) collects finished chunks, unloads far ones, asks the
//! workers for missing ones nearest-first and populates one chunk; `mesh_pending` uploads the meshes
//! the workers finished (a couple a frame) and queues jobs for the chunks that need one (the
//! workers copy the chunk and its four neighbours, build the vertices and send them back).
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
use crate::world::gen::noise::JavaRandom;
use crate::world::chest::{Chest, Pos};
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

/// Finished meshes uploaded (buffers created) per frame, so walking into fresh terrain never stalls a frame. The meshing itself runs
/// on the workers (`Job::Mesh`), the upload is the only part that needs the device.
const MESH_PER_FRAME: usize = 2;

/// A second upload in a frame starts only while the frame's chunk work is still under this (the first one always runs, so a finished
/// mesh never starves).
const CHUNK_WORK_BUDGET: std::time::Duration = std::time::Duration::from_millis(6);

/// A frame whose chunk work (stream, populate, light, meshes) took this long or more is logged with its stages.
const SLOW_CHUNK_WORK: std::time::Duration = std::time::Duration::from_millis(16);

/// Saved chunks read per frame (the rest wait for the next frame, like generation does).
/// ponytail: the read + decode runs on the render thread (about 32 KB of work per chunk); move it to the workers if it shows.
const LOADS_PER_FRAME: usize = 8;

/// Chunk coordinate of a world-space coordinate (floor division by 16).
/// The grass and foliage colour of every column of chunk (cx, cz) (`BlockGrass.colorMultiplier` and friends read the climate of the
/// column itself, no blending). ponytail: the climate noise is sampled again for every (re)mesh; cache it per chunk if remeshing shows up in a profile.
fn biome_tints(cm: &mut WorldChunkManager, cx: i32, cz: i32) -> mesh::Tints {
    cm.load_block_generator_data(cx * 16, cz * 16, 16, 16);
    let mut t = mesh::Tints::white();
    for i in 0..256 {
        let (temp, humid) = (cm.temperature[i], cm.humidity[i]);
        t.grass[i] = crate::world::climate::grass(temp, humid) as f32;
        t.foliage[i] = crate::world::climate::foliage(temp, humid) as f32;
    }
    t
}

pub fn chunk_coord(v: f32) -> i32 {
    (v.floor() as i32) >> 4
}

pub struct Mesh {
    pub vbuf: wgpu::Buffer,
    pub ibuf: wgpu::Buffer,
    pub index_count: u32,
    /// The indices are sorted by 16-block section: section `s` is `starts[s]..starts[s + 1]` (`by_section`).
    starts: [u32; SECTIONS + 1],
    /// The water faces (blended, drawn after everything else) share `vbuf`; their indices sorted by section the same way.
    water: Option<(wgpu::Buffer, [u32; SECTIONS + 1])>,
}

/// Sections (16 blocks tall) of a chunk column: the unit the draw loop culls.
pub const SECTIONS: usize = H / 16;

/// Which blocks around the eye are drawn. `Cylinder`: a circle of the horizontal radius (circular horizontal distance).
/// `Sphere`: 3D distance <= the horizontal radius. Both are also capped by the vertical radius. All distances run from the eye
/// to the nearest point of the section's box, so a section is drawn as soon as any part of it is in range.
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Shape {
    Cylinder,
    Sphere,
}

pub fn in_range(shape: Shape, eye: Vec3, min: Vec3, max: Vec3, rh: f32, rv: f32) -> bool {
    let d = (min - eye).max(eye - max).max(Vec3::ZERO);
    let dh2 = d.x * d.x + d.z * d.z;
    d.y <= rv && dh2 + if shape == Shape::Sphere { d.y * d.y } else { 0.0 } <= rh * rh
}

/// Reorders a mesh's triangles by the 16-block section they sit in (the highest vertex minus half a block picks the block, so
/// top, side and plant faces land in their own block's section; a face on a section border may land in either, which the
/// draw loop's one-block margin covers). Returns the new indices and the section offsets. Vertices are `mesh::VF` floats, y second.
fn by_section(verts: &[f32], idxs: &[u32]) -> (Vec<u32>, [u32; SECTIONS + 1]) {
    let sec = |t: &[u32]| {
        let top = t.iter().map(|&i| verts[i as usize * crate::render::mesh::VF + 1]).fold(f32::MIN, f32::max);
        (((top - 0.5).floor().max(0.0) as usize) >> 4).min(SECTIONS - 1)
    };
    let mut starts = [0u32; SECTIONS + 1];
    for t in idxs.chunks_exact(3) {
        starts[sec(t) + 1] += 3;
    }
    for s in 0..SECTIONS {
        starts[s + 1] += starts[s];
    }
    let (mut at, mut out) = (starts, vec![0u32; idxs.len()]);
    for t in idxs.chunks_exact(3) {
        let s = sec(t);
        out[at[s] as usize..at[s] as usize + 3].copy_from_slice(t);
        at[s] += 3;
    }
    (out, starts)
}

/// Blocks, metadata and light of one chunk, copied for a mesh job.
struct Snap {
    blocks: Vec<u8>,
    data: Nibbles,
    light: Vec<u8>,
}

fn snap(e: &Entry) -> Snap {
    Snap { blocks: e.blocks.clone(), data: e.data.clone(), light: e.light.clone() }
}

/// Everything the mesher reads, owned, so a worker can build a chunk's mesh while the manager goes on editing the live chunks: the
/// chunk, then its +X, -X, +Z, -Z neighbours (`mesh::build_split_tinted`'s argument order).
/// ponytail: the copy is ~0.4 MB per job (5 x 80 KB), memcpy speed, on the render thread; the upgrade is `Arc`-held chunk arrays with
/// `Arc::make_mut` on every write, which would copy nothing but touches every writer of `blocks` / `data` / `light`.
struct MeshJob {
    key: Key,
    sky_sub: u8,
    cells: [Snap; 5],
}

/// What a worker is asked: generate a chunk (`OverworldGenerator::generate`) or build a mesh.
enum Job {
    Gen(Key),
    Mesh(Box<MeshJob>),
}

/// A finished mesh, short of its GPU buffers: the vertices, and the opaque and water indices sorted by section (`by_section`).
struct Built {
    verts: Vec<f32>,
    idxs: Vec<u32>,
    starts: [u32; SECTIONS + 1],
    widxs: Vec<u32>,
    wstarts: [u32; SECTIONS + 1],
}

/// The CPU half of meshing a chunk, run by a worker with its own climate noise (`cm`) for the biome tints.
fn build_job(job: MeshJob, cm: &mut WorldChunkManager) -> Built {
    let (x, z) = job.key;
    let [me, px, nx, pz, nz] = &job.cells;
    let (verts, idxs, widxs) = mesh::build_split_tinted(
        crate::LEGACY,
        &me.blocks,
        &me.data,
        [&px.data, &nx.data, &pz.data, &nz.data],
        &me.light,
        [px.blocks.as_slice(), nx.blocks.as_slice(), pz.blocks.as_slice(), nz.blocks.as_slice()],
        [px.light.as_slice(), nx.light.as_slice(), pz.light.as_slice(), nz.light.as_slice()],
        job.sky_sub,
        x * 16,
        z * 16,
        &biome_tints(cm, x, z),
    );
    let (idxs, starts) = by_section(&verts, &idxs);
    let (widxs, wstarts) = by_section(&verts, &widxs);
    Built { verts, idxs, starts, widxs, wstarts }
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
    /// The mesh matches the chunk as it is: built, or on its way from a worker (the mesh may still be `None`: a chunk of pure air has
    /// nothing to draw). Every edit clears it, which queues a new job once the running one (if any) has landed.
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
    jobs: mpsc::Sender<Job>,
    done: mpsc::Receiver<(Key, Vec<u8>)>,
    /// Meshes the workers finished, waiting for their upload (`mesh_pending`).
    built: mpsc::Receiver<(Key, Built)>,
    /// Chunks with a mesh job queued or running (one at a time per chunk, so results never arrive out of order), and the most at once.
    meshing: HashSet<Key>,
    max_meshing: usize,
    /// Pending light updates, newest last (`World.lightingToUpdate`).
    light_queue: Vec<light::Region>,
    /// `World.skylightSubtracted` (0 day .. 11 night), applied by the mesher.
    sky_sub: u8,
    /// Bumped whenever the drawn geometry changes (a mesh with another size, an unload): the cached shadow map is stale then.
    pub mesh_gen: u32,
    /// Save folder (M7); `None` = nothing is read or written (the tests). `saved` = chunks that have a file there.
    dir: Option<PathBuf>,
    saved: HashSet<Key>,
    /// Cells written through `set_block_meta` since the last `take_changes`: (x, y, z, old id, new id). `world::ticks`
    /// turns them into `onBlockAdded` / `onBlockRemoval` / `onNeighborBlockChange` (the `...WithNotify` setters).
    changes: Vec<(i32, i32, i32, u8, u8)>,
    /// Dungeon chests filled by populate that the game has not taken over yet (`take_loot`).
    loot: Vec<(Pos, Chest)>,
    /// Dungeon spawners populate placed that the game has not taken over yet (`take_spawners`).
    spawners: Vec<(Pos, u8)>,
}

impl ChunkManager {
    /// `radius` is the render distance in chunks. Dropping the manager closes the job channel,
    /// which ends the workers.
    pub fn new(seed: i64, radius: i32) -> Self {
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (done_tx, done) = mpsc::channel();
        let (built_tx, built) = mpsc::channel();
        let job_rx = Arc::new(Mutex::new(job_rx));
        // Leave a core for the render thread; two workers are plenty for walking speed.
        let workers = std::thread::available_parallelism().map_or(2, |n| n.get()).saturating_sub(1).clamp(1, 2);
        for i in 0..workers {
            let (rx, tx, bt) = (job_rx.clone(), done_tx.clone(), built_tx.clone());
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
                        let Ok(job) = job else { break };
                        let sent = match job {
                            Job::Gen((cx, cz)) => tx.send(((cx, cz), g.generate(cx, cz, &mut cm))).is_ok(),
                            Job::Mesh(j) => {
                                let key = j.key;
                                bt.send((key, build_job(*j, &mut cm))).is_ok()
                            }
                        };
                        if !sent {
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

        Self { chunks: Chunks::default(), pending: HashSet::new(), ring, radius, center: None, max_in_flight: workers * 2, built, meshing: HashSet::new(), max_meshing: workers * 2,
               gen: OverworldGenerator::new(seed), cm: WorldChunkManager::new(seed), jobs, done, light_queue: Vec::new(), sky_sub: 0, mesh_gen: 0, dir: None, saved: HashSet::new(), changes: Vec::new(), loot: Vec::new(), spawners: Vec::new() }
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

    pub(crate) fn insert(&mut self, key: Key, blocks: Vec<u8>) {
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

    /// Raw (sky, block) light of a cell, what the mesher's `lm` takes: entities are lit by the cell they are in
    /// (`Entity.getBrightnessForRender`). Open sky above the world and unloaded chunks read as full sky light.
    pub fn light_levels(&self, x: i32, y: i32, z: i32) -> (u8, u8) {
        if !(0..H as i32).contains(&y) {
            return (if y >= H as i32 { 15 } else { 0 }, 0);
        }
        self.chunks.get(&(x >> 4, z >> 4)).map_or((15, 0), |e| {
            let l = e.light[idx((x & 15) as usize, y as usize, (z & 15) as usize)];
            (l & 15, l >> 4)
        })
    }

    /// `World.getBlockLightValue`: the brighter of block light and sky light less `skylightSubtracted`, 0..=15; the open sky
    /// above the world and unloaded chunks read as full daylight.
    pub fn light(&self, x: i32, y: i32, z: i32) -> u8 {
        let day = 15u8.saturating_sub(self.sky_sub);
        if !(0..H as i32).contains(&y) {
            return if y >= H as i32 { day } else { 0 };
        }
        self.chunks.get(&(x >> 4, z >> 4)).map_or(day, |e| {
            let l = e.light[idx((x & 15) as usize, y as usize, (z & 15) as usize)];
            (l & 15).saturating_sub(self.sky_sub).max(l >> 4)
        })
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
        let old = self.block_loaded(x, y, z);
        let done = self.set_quiet(x, y, z, id, meta);
        if done {
            self.changes.push((x, y, z, old.unwrap_or(0), id));
        }
        done
    }

    /// `World.notifyBlocksOfNeighborChange(x, y, z)`: the six neighbours of the cell hear of a change, nothing is written. The log entry
    /// carries the sentinel id 255 (no block), so `ticks` reads it as "something that may provide power changed here".
    pub fn ping(&mut self, x: i32, y: i32, z: i32) {
        self.changes.push((x, y, z, 255, 255));
    }

    /// `World.setBlockAndMetadata` / `setBlockMetadata`: the same write without telling the neighbours.
    pub fn set_quiet(&mut self, x: i32, y: i32, z: i32, id: u8, meta: u8) -> bool {
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

    pub fn take_changes(&mut self) -> Vec<(i32, i32, i32, u8, u8)> {
        std::mem::take(&mut self.changes)
    }

    /// `World.getBlockLightValue` (`sub` = `skylightSubtracted`, 0 for `getFullBlockLightValue`): the brighter of sky and block light.
    pub fn light_at(&self, x: i32, y: i32, z: i32, sub: u8) -> u8 {
        if y < 0 {
            return 0;
        }
        let y = y.min(H as i32 - 1) as usize;
        self.chunks.get(&(x >> 4, z >> 4)).map_or(0, |e| {
            let l = e.light[idx((x & 15) as usize, y, (z & 15) as usize)];
            (l & 15).saturating_sub(sub).max(l >> 4)
        })
    }

    /// Light with the current day/night subtraction (`getBlockLightValue`).
    pub fn light_level(&self, x: i32, y: i32, z: i32) -> u8 {
        self.light_at(x, y, z, self.sky_sub)
    }

    /// `World.tick`'s random block ticks: 80 random cells in every lit, final chunk within `r` chunks of `center`, with the Java's
    /// own LCG. Returns the cells whose block ticks at random (`Block.tickOnLoad`).
    pub fn random_ticks(&self, center: Key, r: i32, lcg: &mut i32, rng: &mut JavaRandom) -> (Vec<(i32, i32, i32, u8)>, Vec<(i32, i32)>) {
        let (mut out, mut columns) = (Vec::new(), Vec::new());
        for (&(cx, cz), e) in &self.chunks {
            if (cx - center.0).abs() > r || (cz - center.1).abs() > r || !e.lit || !self.is_final(cx, cz) {
                continue;
            }
            // The snow / ice column of `World.tick`: one chunk in sixteen per tick, a column drawn from the same LCG.
            if rng.next_int_bound(16) == 0 {
                *lcg = lcg.wrapping_mul(3).wrapping_add(1013904223);
                let v = *lcg >> 2;
                columns.push((cx * 16 + (v & 15), cz * 16 + (v >> 8 & 15)));
            }
            for _ in 0..80 {
                *lcg = lcg.wrapping_mul(3).wrapping_add(1013904223);
                let v = *lcg >> 2;
                let (x, z, y) = ((v & 15) as usize, (v >> 8 & 15) as usize, (v >> 16 & 127) as usize);
                let id = e.blocks[idx(x, y, z)];
                if crate::world::ticks::ticks_at_random(id) {
                    out.push((cx * 16 + x as i32, y as i32, cz * 16 + z as i32, id));
                }
            }
        }
        (out, columns)
    }

    /// `World.getSavedLightValue(EnumSkyBlock.Block, ..)`.
    pub fn block_light(&self, x: i32, y: i32, z: i32) -> u8 {
        if y < 0 || y >= H as i32 {
            return 0;
        }
        self.chunks.get(&(x >> 4, z >> 4)).map_or(0, |e| e.light[idx((x & 15) as usize, y as usize, (z & 15) as usize)] >> 4)
    }

    /// `BlockSapling.growTree` in a 2x2 chunk region around (x, z) (a tree reaches at most 8 cells out); false when one of the
    /// four chunks is missing or the tree does not fit. ponytail: the four chunks are lit again as after a populate, one
    /// light pass per grown tree; a per-cell light update would be cheaper.
    pub fn grow_tree(&mut self, x: i32, y: i32, z: i32, species: u8, rng: &mut JavaRandom) -> bool {
        let (cx, cz) = ((x - 8) >> 4, (z - 8) >> 4);
        let Some(mut region) = self.take_region(cx, cz) else { return false };
        let ok = crate::world::gen::populate::grow_sapling(&mut region, rng, x, y, z, species);
        self.put_region(cx, cz, region);
        ok
    }

    /// The four chunks populate / tree growth write into, moved out into a `Region`.
    fn take_region(&mut self, x: i32, z: i32) -> Option<Region> {
        let keys = [(x, z), (x, z + 1), (x + 1, z), (x + 1, z + 1)];
        if !keys.iter().all(|k| self.chunks.contains_key(k)) {
            return None;
        }
        let blocks = keys.map(|k| std::mem::take(&mut self.chunks.get_mut(&k).unwrap().blocks));
        let data = keys.map(|k| std::mem::take(&mut self.chunks.get_mut(&k).unwrap().data));
        Some(Region::new(x, z, blocks, data))
    }

    /// The chests populate filled since the last call; the game puts them into its tile entities.
    pub fn take_loot(&mut self) -> Vec<(Pos, Chest)> {
        std::mem::take(&mut self.loot)
    }

    /// The spawners populate placed since the last call; the game puts them into its tile entities.
    pub fn take_spawners(&mut self) -> Vec<(Pos, u8)> {
        std::mem::take(&mut self.spawners)
    }

    fn put_region(&mut self, x: i32, z: i32, mut region: Region) {
        self.loot.extend(region.take_loot());
        self.spawners.extend(region.take_spawners());
        let keys = [(x, z), (x, z + 1), (x + 1, z), (x + 1, z + 1)];
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
    }

    /// The chunk holding world column (x, z) needs a new mesh, and so does the neighbour whose border
    /// faces look at it when the column is on the chunk edge.
    fn mark_dirty(&mut self, x: i32, z: i32) {
        let (cx, cz, lx, lz) = (x >> 4, z >> 4, x & 15, z & 15);
        // At most one neighbour per axis, so three keys at most; an array, because this runs for every cell whose light changes.
        let mut keys = [(cx, cz); 3];
        let mut n = 1;
        for (hit, k) in [(lx == 0, (cx - 1, cz)), (lx == 15, (cx + 1, cz)), (lz == 0, (cx, cz - 1)), (lz == 15, (cx, cz + 1))] {
            if hit {
                keys[n] = k;
                n += 1;
            }
        }
        for k in &keys[..n] {
            if let Some(e) = self.chunks.get_mut(k) {
                e.meshed = false;
            }
        }
    }

    /// Chunk C is final when populate has run on C, C-x, C-z and C-x-z.
    fn is_final(&self, x: i32, z: i32) -> bool {
        [(x, z), (x - 1, z), (x, z - 1), (x - 1, z - 1)].iter().all(|k| self.chunks.get(k).is_some_and(|e| e.populated))
    }

    /// Time of day changed the light. In the legacy look the meshes stay: no vertex its shaders read depends on it (`lm` carries
    /// the raw levels, the sky-light the day takes away is the `amb.x` uniform), so a rebuild would only repeat the same vertices.
    /// It used to restart every mesh (`updateAllRenderers`): the whole ring, a couple per frame, at every sky-light step.
    /// Only the Photon water shader (`render/water.rs`, wave amplitude) still reads the baked light, so without `LEGACY` they restart.
    pub fn set_sky_sub(&mut self, v: u8) {
        let stale = !crate::LEGACY && v != self.sky_sub;
        self.sky_sub = v;
        if stale {
            self.chunks.values_mut().for_each(|e| e.meshed = false);
        }
    }

    /// `WorldChunkManager.getTemperature` at a block column (for the sky colour).
    pub fn temperature_at(&mut self, x: i32, z: i32) -> f64 {
        self.cm.load_block_generator_data(x, z, 1, 1);
        self.cm.temperature[0]
    }

    /// The biome and climate (`temperature`, `humidity`, both 0..1) of a block column, for the pack's weather and fog.
    pub fn climate_at(&mut self, x: i32, z: i32) -> (crate::world::biome::Biome, f64, f64) {
        let biome = self.cm.load_block_generator_data(x, z, 1, 1)[0];
        (biome, self.cm.temperature[0], self.cm.humidity[0])
    }

    /// `WorldChunkManager.func_4069_a`: the biomes of a `w` x `d` area from (x, z), x outer, z inner (what `renderRainSnow` walks).
    pub fn biomes(&mut self, x: i32, z: i32, w: usize, d: usize) -> Vec<crate::world::biome::Biome> {
        self.cm.load_block_generator_data(x, z, w, d)
    }

    /// `World.findTopSolidBlock`: one above the highest solid or liquid cell of the loaded column, -1 if none.
    pub fn top_solid(&self, x: i32, z: i32) -> i32 {
        use crate::world::gen::populate::{is_liquid, is_solid};
        (1..H as i32).rev().find(|&y| self.block_loaded(x, y, z).is_some_and(|b| is_solid(b) || is_liquid(b))).map_or(-1, |y| y + 1)
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

    /// The main pass's draw loop: calls `draw(mesh, index range)` for every run of neighbouring sections that is inside the
    /// `shape` around `eye` (radii `rh`, `rv` in blocks) and passes `visible(min, max)` (the frustum). Each section's box is
    /// one block taller at both ends. Empty sections are skipped, touching ones are one range, so a chunk is mostly one draw.
    /// `water` picks the water faces (index buffer passed to `draw`) instead of the opaque ones.
    pub fn draw_ranges(&self, eye: Vec3, shape: Shape, rh: f32, rv: f32, water: bool, visible: impl Fn(Vec3, Vec3) -> bool, mut draw: impl FnMut(&Mesh, &wgpu::Buffer, std::ops::Range<u32>)) {
        // Translucent water blends over what is behind it, so its chunks go far to near (`WorldRenderer` sorts chunks too).
        // ponytail: whole chunks only, the faces inside one chunk keep map order; upgrade = sort the sections / faces too.
        let mut order: Vec<_> = self.chunks.iter().collect();
        if water {
            let d2 = |e: &(&Key, &_)| ((e.0 .0 * 16 + 8) as f32 - eye.x).powi(2) + ((e.0 .1 * 16 + 8) as f32 - eye.z).powi(2);
            order.sort_by(|a, b| d2(b).total_cmp(&d2(a)));
        }
        for (&(x, z), e) in order {
            let Some(m) = e.mesh.as_ref() else { continue };
            let (ibuf, starts) = match (&m.water, water) {
                (Some((b, s)), true) => (b, s),
                (None, true) => continue,
                _ => (&m.ibuf, &m.starts),
            };
            let mut run: Option<(u32, u32)> = None;
            for s in 0..=SECTIONS {
                let on = s < SECTIONS && starts[s + 1] > starts[s] && {
                    let min = Vec3::new(x as f32 * 16.0, s as f32 * 16.0 - 1.0, z as f32 * 16.0);
                    let max = min + Vec3::new(16.0, 18.0, 16.0);
                    in_range(shape, eye, min, max, rh, rv) && visible(min, max)
                };
                match (on, run) {
                    (true, None) => run = Some((starts[s], starts[s + 1])),
                    (true, Some((a, _))) => run = Some((a, starts[s + 1])),
                    (false, Some((a, b))) => {
                        draw(m, ibuf, a..b);
                        run = None;
                    }
                    _ => {}
                }
            }
        }
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
        let t0 = std::time::Instant::now();
        self.stream(cx, cz);
        self.populate_pending(cx, cz, 1);
        let populated = t0.elapsed();
        self.light_pending(cx, cz);
        let lit = t0.elapsed();
        // Meshes wait for the light queue to drain, so an edit or a new chunk is meshed once, lit.
        let mut built = 0;
        if self.light_queue.is_empty() {
            built = self.mesh_pending(device, cx, cz, t0);
        }
        let total = t0.elapsed();
        if total >= SLOW_CHUNK_WORK {
            log::warn!("slow chunk work {total:?}: stream + populate {populated:?}, light done at {lit:?}, {built} mesh(es)");
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
            self.mesh_gen = self.mesh_gen.wrapping_add(1);
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
                let _ = self.jobs.send(Job::Gen(key));
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
        let mut region = self.take_region(x, z).unwrap();
        self.gen.populate(&mut region, x, z, &mut self.cm);
        self.put_region(x, z, region);
        self.chunks.get_mut(&keys[0]).unwrap().populated = true;
        true
    }

    /// A finished mesh for `key` arrived: false when the chunk was unloaded meanwhile. One edited since its job was queued is kept all
    /// the same: its `meshed` is clear, so a new job follows, and until that lands the older picture still beats the one before it
    /// (a chunk that keeps changing, flowing water, would otherwise never be redrawn while its jobs outlast the gaps between edits).
    fn accept(&mut self, key: Key) -> bool {
        self.meshing.remove(&key);
        self.chunks.contains_key(&key)
    }

    /// Uploads up to `MESH_PER_FRAME` finished meshes (`t0` = when this frame's chunk work began), then queues jobs for the chunks
    /// that need one, nearest first, while fewer than `max_meshing` are out; returns how many it uploaded.
    fn mesh_pending(&mut self, device: &wgpu::Device, cx: i32, cz: i32, t0: std::time::Instant) -> usize {
        let mut uploaded = 0;
        while uploaded == 0 || (uploaded < MESH_PER_FRAME && t0.elapsed() <= CHUNK_WORK_BUDGET) {
            let Ok((key, b)) = self.built.try_recv() else { break };
            if !self.accept(key) {
                continue;
            }
            uploaded += 1;
            let Built { verts, idxs, starts, widxs, wstarts } = b;
            let mesh = (!idxs.is_empty() || !widxs.is_empty()).then(|| Mesh {
                vbuf: create_vertex_buffer(device, bytemuck::cast_slice(&verts)),
                // (a chunk of only water has no opaque faces: a dummy index keeps the buffer non-empty, `index_count` is 0)
                ibuf: create_index_buffer(device, if idxs.is_empty() { &[0u32][..] } else { &idxs[..] }),
                index_count: idxs.len() as u32,
                starts,
                water: (!widxs.is_empty()).then(|| (create_index_buffer(device, &widxs), wstarts)),
            });
            if let Some(e) = self.chunks.get_mut(&key) {
                // ponytail: a rebuild with the same index count (the time-of-day relight) is taken as the same geometry, so the
                // shadow map is not redrawn for it; an edit that keeps the count is picked up by the next sun / movement refresh.
                if e.mesh.as_ref().map_or(0, |m| m.index_count) != mesh.as_ref().map_or(0, |m| m.index_count) {
                    self.mesh_gen = self.mesh_gen.wrapping_add(1);
                }
                e.mesh = mesh;
            }
        }

        let r2 = self.radius * self.radius;
        for &(dx, dz) in &self.ring {
            if self.meshing.len() >= self.max_meshing || dx * dx + dz * dz > r2 {
                break; // the ring is sorted by distance, so everything after is outside too
            }
            let (x, z) = (cx + dx, cz + dz);
            if self.meshing.contains(&(x, z)) {
                continue;
            }
            let c = &self.chunks;
            let job = match (c.get(&(x, z)), c.get(&(x + 1, z)), c.get(&(x - 1, z)), c.get(&(x, z + 1)), c.get(&(x, z - 1))) {
                (Some(me), Some(px), Some(nx), Some(pz), Some(nz)) if !me.meshed && me.lit && self.is_final(x, z) => {
                    Some(MeshJob { key: (x, z), sky_sub: self.sky_sub, cells: [snap(me), snap(px), snap(nx), snap(pz), snap(nz)] })
                }
                _ => None,
            };
            let Some(job) = job else { continue };
            if let Some(e) = self.chunks.get_mut(&(x, z)) {
                e.meshed = true; // "on its way": an edit before the result lands clears it again, and a new job follows
            }
            self.meshing.insert((x, z));
            let _ = self.jobs.send(Job::Mesh(Box::new(job)));
        }
        uploaded
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

    /// Sphere is stricter than cylinder, both stop at the vertical radius; sections come out in order with the right offsets.
    #[test]
    fn render_shape_and_sections() {
        let (eye, lo, hi) = (Vec3::new(8.0, 64.0, 8.0), Vec3::new(64.0, 48.0, 0.0), Vec3::new(80.0, 66.0, 16.0));
        assert!(in_range(Shape::Cylinder, eye, lo, hi, 64.0, 48.0) && in_range(Shape::Sphere, eye, lo, hi, 64.0, 48.0));
        let (lo, hi) = (Vec3::new(64.0, 0.0, 0.0), Vec3::new(80.0, 16.0, 16.0)); // 48 below, 56 across: 74 away in 3D
        assert!(in_range(Shape::Cylinder, eye, lo, hi, 64.0, 48.0) && !in_range(Shape::Sphere, eye, lo, hi, 64.0, 48.0));
        assert!(!in_range(Shape::Cylinder, eye, lo, hi, 64.0, 40.0), "vertical radius");
        // Two triangles at y = 5 and y = 40 (given out of order) are sorted into sections 0 and 2.
        let mut v = vec![0.0f32; 6 * crate::render::mesh::VF];
        (0..3).for_each(|i| v[i * crate::render::mesh::VF + 1] = 5.0);
        (3..6).for_each(|i| v[i * crate::render::mesh::VF + 1] = 40.0);
        let (idx, st) = by_section(&v, &[3, 4, 5, 0, 1, 2]);
        assert_eq!((idx, st[1], st[2], st[3], st[SECTIONS]), (vec![0, 1, 2, 3, 4, 5], 3, 3, 6, 6));
    }

    /// The CPU half of meshing run from a snapshot (what a worker does) gives exactly what the mesher gives on the live arrays.
    #[test]
    fn worker_mesh_is_the_in_place_mesh() {
        let snap_of = |id: u8| {
            let mut blocks = vec![0u8; VOLUME];
            (0..16).for_each(|x| (0..16).for_each(|z| blocks[idx(x, 60, z)] = id));
            Snap { blocks, data: Nibbles::new(), light: vec![15; VOLUME] }
        };
        let job = MeshJob { key: (2, -3), sky_sub: 0, cells: [snap_of(1), snap_of(1), snap_of(1), snap_of(1), snap_of(1)] };
        let want = {
            let [me, px, nx, pz, nz] = &job.cells;
            let tints = biome_tints(&mut WorldChunkManager::new(1), 2, -3);
            let (v, i, w) = mesh::build_split_tinted(
                crate::LEGACY, &me.blocks, &me.data, [&px.data, &nx.data, &pz.data, &nz.data], &me.light,
                [px.blocks.as_slice(), nx.blocks.as_slice(), pz.blocks.as_slice(), nz.blocks.as_slice()],
                [px.light.as_slice(), nx.light.as_slice(), pz.light.as_slice(), nz.light.as_slice()], 0, 32, -48, &tints,
            );
            let (i, si) = by_section(&v, &i);
            let (w, sw) = by_section(&v, &w);
            (v, i, si, w, sw)
        };
        let b = build_job(job, &mut WorldChunkManager::new(1));
        assert!(!b.idxs.is_empty(), "a stone floor has a top face");
        assert_eq!((b.verts, b.idxs, b.starts, b.widxs, b.wstarts), want);
    }

    /// A result lands whether or not its chunk changed while the job ran (an edit clears `meshed`, so the next job is queued), is
    /// dropped when the chunk was unloaded, and always frees the chunk for its next job.
    #[test]
    fn mesh_results_land_and_an_edit_queues_the_next_job() {
        let mut m = ChunkManager::new(1, 1);
        m.insert((0, 0), vec![0u8; VOLUME]);
        m.chunks.get_mut(&(0, 0)).unwrap().meshed = true; // a job is out
        m.meshing.insert((0, 0));
        assert!(m.accept((0, 0)) && m.meshing.is_empty() && m.chunks[&(0, 0)].meshed, "untouched: kept, still current");
        m.meshing.insert((0, 0));
        m.mark_dirty(3, 3); // an edit while the job ran
        assert!(m.accept((0, 0)) && m.meshing.is_empty() && !m.chunks[&(0, 0)].meshed, "edited: kept, and due a new job");
        m.meshing.insert((0, 0));
        m.chunks.remove(&(0, 0));
        assert!(!m.accept((0, 0)) && m.meshing.is_empty(), "unloaded: dropped");
    }

    /// A new sky-light level changes nothing the legacy shaders read from a vertex, so no mesh goes stale (the Photon look restarts them).
    #[test]
    fn sky_light_step_keeps_the_meshes() {
        let mut m = ChunkManager::new(1, 1);
        m.insert((0, 0), vec![0u8; VOLUME]);
        m.chunks.get_mut(&(0, 0)).unwrap().meshed = true;
        m.set_sky_sub(7);
        assert_eq!(m.chunks[&(0, 0)].meshed, crate::LEGACY);
        assert_eq!(m.sky_sub, 7);
    }
}
