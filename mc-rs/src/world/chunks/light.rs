//! M5 light engine: a port of the b1.7.3 lighting in `World` (`scheduleLightingUpdate`,
//! `neighborLightPropagationChanged`, get/setLightValue), `MetadataChunkBlock.func_4127_a` (the region
//! relaxation) and `Chunk` (`generateSkylightMap`, `func_1003_g` relight, `func_996_c` lateral
//! scheduling). Two light kinds per cell: sky (15 straight down from open sky) and block (emitters,
//! `Block.lightValue`). Every update recomputes a box of cells from their six neighbours
//! (`max(neighbours) - max(opacity, 1)`, or the cell's own source level) and queues the neighbours of
//! each cell that changed, until nothing changes. Newest region first, like the Java.
//!
//! The engine runs on its own thread (`chunk-light`, C2ME's `threading-lighting` idea): `LightWorld` keeps its own copy of every
//! chunk's blocks and height map and is the only writer of the light; the render thread tells it what happens (`Msg`: a chunk's
//! blocks, an edit, an unload, the first light of a chunk) and keeps a read-only copy of the light that the thread's `Report`s
//! replace. So a region never waits for the frame. The chunk arrays are `Arc`s shared with the render thread while nobody writes
//! (`Arc::make_mut` copies a chunk's array on its first write from either side), so an untouched chunk costs one copy of blocks and light.
//!
//! Differences from vanilla, none of which change the settled result:
//! - the queue is drained in time slices on the light thread (`run_light`) instead of `updatingLighting`'s 500 regions;
//! - a freshly final chunk is lit in one go (`init_chunk`) because worldgen writes raw blocks instead of
//!   calling `setBlockWithNotify`, so no per-block updates were ever queued for them;
//! - no day/night: `skylightSubtracted` is 0 (see `render::mesh`).

use super::{ChunkManager, Key, KeyHasher};
use crate::world::chunk::{height_map, idx, light_opacity, light_value, H, VOLUME};
use std::collections::{HashMap, HashSet};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Kind {
    Sky,
    Block,
}

/// The `kind` half of a raw light byte (sky in the low nibble, block light in the high one).
fn nib(kind: Kind, v: u8) -> u8 {
    if kind == Kind::Sky { v & 15 } else { v >> 4 }
}

/// `MetadataChunkBlock`: a box of cells (inclusive) whose light must be recomputed.
#[derive(Clone, Copy, Debug)]
pub(super) struct Region {
    kind: Kind,
    lo: [i32; 3],
    hi: [i32; 3],
}

impl Region {
    fn volume(&self) -> i32 {
        (0..3).map(|a| self.hi[a] - self.lo[a] + 1).product()
    }

    /// `MetadataChunkBlock.func_866_a`: grow this region to also cover `lo..=hi` if that adds
    /// at most 2 cells of volume. True if it now covers it.
    fn merge(&mut self, lo: [i32; 3], hi: [i32; 3]) -> bool {
        if (0..3).all(|a| lo[a] >= self.lo[a] && hi[a] <= self.hi[a]) {
            return true;
        }
        if !(0..3).all(|a| lo[a] >= self.lo[a] - 1 && hi[a] <= self.hi[a] + 1) {
            return false;
        }
        let old: i32 = (0..3).map(|a| self.hi[a] - self.lo[a]).product();
        let (nlo, nhi): (Vec<i32>, Vec<i32>) = (0..3).map(|a| (lo[a].min(self.lo[a]), hi[a].max(self.hi[a]))).unzip();
        let new: i32 = (0..3).map(|a| nhi[a] - nlo[a]).product();
        if new - old <= 2 {
            self.lo = [nlo[0], nlo[1], nlo[2]];
            self.hi = [nhi[0], nhi[1], nhi[2]];
            return true;
        }
        false
    }
}

/// How long the light thread works on its queue before it reports what it has (the render thread sees nothing before that).
const LIGHT_SLICE: Duration = Duration::from_millis(5);

/// What the engine knows of one chunk: its own copy of the blocks and of the height map (kept in step by `Msg::Chunk` and
/// `Msg::Edit`), and the light, which only the engine writes.
struct LChunk {
    /// Shared with the render thread's `Entry.blocks` until either side writes (`Arc::make_mut` copies then).
    blocks: Arc<Vec<u8>>,
    /// Per cell, sky light in the low nibble and block light in the high nibble (same index as `blocks`). A report hands the render
    /// thread a second `Arc` of it; the next write of the engine copies the array once, so it is shared only while it is at rest.
    light: Arc<Vec<u8>>,
    height: [u8; 256],
    /// `init_chunk` ran and the blocks have not been replaced since.
    lit: bool,
    /// The light array or the lit flag changed since the last report.
    touched: bool,
    /// A light value changed in this chunk or beside it (the `mark_dirty` rule): its mesh is stale.
    stale: bool,
}

type LChunks = HashMap<Key, LChunk, std::hash::BuildHasherDefault<KeyHasher>>;

/// The engine's state: what `ChunkManager` used to hold for light, on the light thread.
#[derive(Default)]
struct LightWorld {
    chunks: LChunks,
    /// Pending light updates, newest last (`World.lightingToUpdate`).
    queue: Vec<Region>,
}

/// What the render thread tells the engine, in the order it happened.
pub(super) enum Msg {
    /// The blocks of a chunk, new or rewritten by populate: the height map is rebuilt, and the chunk is unlit until `Init`.
    Chunk(Key, Arc<Vec<u8>>),
    /// The cell (x, y, z) became `id`.
    Edit { x: i32, y: i32, z: i32, id: u8 },
    /// The chunk was unloaded.
    Remove(Key),
    /// First light of the chunk (`init_chunk`).
    Init(Key),
}

/// What the engine tells the render thread after each time slice.
struct Report {
    /// Messages processed since the previous report.
    done: usize,
    /// The queue is empty: everything the render thread sent is lit.
    idle: bool,
    /// Chunks written to since the previous report: key, the whole light array, the lit flag.
    chunks: Vec<(Key, Arc<Vec<u8>>, bool)>,
    /// Chunks whose mesh went stale because their light (or a neighbour's across the edge) changed.
    stale: Vec<Key>,
}

/// The render thread's end of the light thread.
pub(super) struct LightLink {
    tx: mpsc::Sender<Msg>,
    rx: mpsc::Receiver<Report>,
    /// Messages sent that no report has counted yet.
    outstanding: usize,
    /// The engine's queue was empty at its last report.
    idle: bool,
    /// Chunks an `Init` was sent for whose lit flag has not come back, so `light_pending` does not send it twice.
    lighting: HashSet<Key>,
}

impl LightLink {
    pub(super) fn spawn() -> Self {
        let (tx, jobs) = mpsc::channel::<Msg>();
        let (done, rx) = mpsc::channel::<Report>();
        std::thread::Builder::new().name("chunk-light".into()).spawn(move || light_thread(jobs, done)).expect("spawn light thread");
        Self { tx, rx, outstanding: 0, idle: true, lighting: HashSet::new() }
    }
}

/// The light thread: take every message that is in, run the queue for a time slice, report; sleep when there is nothing to do.
/// Ends when the manager is dropped (the message channel closes).
fn light_thread(jobs: mpsc::Receiver<Msg>, done: mpsc::Sender<Report>) {
    let mut w = LightWorld::default();
    let mut n = 0usize;
    loop {
        if w.queue.is_empty() {
            match jobs.recv() {
                Ok(m) => {
                    w.apply(m);
                    n += 1;
                }
                Err(_) => return,
            }
        }
        while let Ok(m) = jobs.try_recv() {
            w.apply(m);
            n += 1;
        }
        w.run_light(Some(Instant::now() + LIGHT_SLICE));
        if done.send(w.report(std::mem::take(&mut n))).is_err() {
            return;
        }
    }
}

/// The height half of `Chunk.setBlockID` + `func_1003_g`: the column's new height after the cell at `y` became `id` (`h` = the
/// height before, `blocks` already hold `id`), `None` when it does not change. The render thread keeps its own height map with
/// this (readers like `sees_sky` need it at once); the engine does the same on its copy in `relight_column`.
pub(super) fn height_after_set(blocks: &[u8], lx: usize, lz: usize, id: u8, y: i32, h: i32) -> Option<i32> {
    let from = if light_opacity(id) != 0 {
        if y >= h { y + 1 } else { return None }
    } else if y == h - 1 {
        y
    } else {
        return None;
    };
    let top = column_top(blocks, lx, lz, h, from);
    (top != h).then_some(top)
}

/// `Chunk.func_1003_g`'s height search: from `max(old, y)` down to the first opaque cell.
fn column_top(blocks: &[u8], lx: usize, lz: usize, old: i32, y: i32) -> i32 {
    let mut h = old.max(y);
    while h > 0 && light_opacity(blocks[idx(lx, h as usize - 1, lz)]) == 0 {
        h -= 1;
    }
    h
}

impl LightWorld {
    /// One message from the render thread.
    fn apply(&mut self, m: Msg) {
        match m {
            Msg::Chunk(k, blocks) => {
                let height = height_map(&blocks);
                match self.chunks.get_mut(&k) {
                    // Rewritten by populate: the light stays until `Init` computes it again, as it did.
                    Some(e) => {
                        e.blocks = blocks;
                        e.height = height;
                        e.lit = false;
                        e.touched = true;
                    }
                    None => {
                        self.chunks.insert(k, LChunk { blocks, light: Arc::new(vec![0; VOLUME]), height, lit: false, touched: false, stale: false });
                    }
                }
            }
            Msg::Edit { x, y, z, id } => {
                if !(0..H as i32).contains(&y) {
                    return;
                }
                let Some(e) = self.chunks.get_mut(&(x >> 4, z >> 4)) else { return };
                let (lx, lz) = ((x & 15) as usize, (z & 15) as usize);
                let h = e.height[lz << 4 | lx] as i32;
                Arc::make_mut(&mut e.blocks)[idx(lx, y as usize, lz)] = id;
                self.light_after_set(x, y, z, id, h);
            }
            Msg::Remove(k) => {
                self.chunks.remove(&k);
            }
            Msg::Init(k) => {
                if self.chunks.contains_key(&k) {
                    self.init_chunk(k);
                }
            }
        }
    }

    /// What changed since the last report, and clear it.
    fn report(&mut self, done: usize) -> Report {
        let (mut chunks, mut stale) = (Vec::new(), Vec::new());
        for (&k, e) in self.chunks.iter_mut() {
            if e.touched {
                e.touched = false;
                chunks.push((k, Arc::clone(&e.light), e.lit));
            }
            if e.stale {
                e.stale = false;
                stale.push(k);
            }
        }
        Report { done, idle: self.queue.is_empty(), chunks, stale }
    }

    /// A light value at column (x, z) changed: the mesh of its chunk is stale, and so is the neighbour's across a chunk edge (its
    /// border faces look at it).
    fn mark_dirty(&mut self, x: i32, z: i32) {
        let (cx, cz, lx, lz) = (x >> 4, z >> 4, x & 15, z & 15);
        // At most one neighbour per axis, so three keys at most.
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
                e.stale = true;
            }
        }
    }

    fn chunk_exists(&self, x: i32, z: i32) -> bool {
        self.chunks.contains_key(&(x >> 4, z >> 4))
    }

    /// Block id for light maths: air outside the world and in unloaded chunks (`World.getBlockId`).
    fn raw_block(&self, x: i32, y: i32, z: i32) -> u8 {
        if !(0..H as i32).contains(&y) {
            return 0;
        }
        self.chunks.get(&(x >> 4, z >> 4)).map_or(0, |e| e.blocks[idx((x & 15) as usize, y as usize, (z & 15) as usize)])
    }

    /// `World.getSavedLightValue`: y is clamped into the world, an unloaded chunk is dark.
    pub(super) fn saved_light(&self, kind: Kind, x: i32, y: i32, z: i32) -> u8 {
        let y = y.clamp(0, H as i32 - 1) as usize;
        self.chunks.get(&(x >> 4, z >> 4)).map_or(0, |e| nib(kind, e.light[idx((x & 15) as usize, y, (z & 15) as usize)]))
    }

    /// Write a nibble without telling the mesher.
    fn put_light(&mut self, kind: Kind, x: i32, y: i32, z: i32, v: u8) {
        let Some(e) = self.chunks.get_mut(&(x >> 4, z >> 4)) else { return };
        let i = idx((x & 15) as usize, y as usize, (z & 15) as usize);
        e.touched = true;
        let light = Arc::make_mut(&mut e.light);
        light[i] = if kind == Kind::Sky { light[i] & 0xF0 | v } else { light[i] & 0x0F | v << 4 };
    }

    /// `World.setLightValue`: write, and the mesh that shows this cell is stale.
    fn set_saved_light(&mut self, kind: Kind, x: i32, y: i32, z: i32, v: u8) {
        // (`update_region`, the only caller, has checked that the cell's chunk is loaded)
        if (0..H as i32).contains(&y) {
            self.put_light(kind, x, y, z, v);
            self.mark_dirty(x, z);
        }
    }

    /// `World.getHeightValue`: 0 for an unloaded chunk.
    fn height_at(&self, x: i32, z: i32) -> i32 {
        self.chunks.get(&(x >> 4, z >> 4)).map_or(0, |e| e.height[((z & 15) << 4 | (x & 15)) as usize] as i32)
    }

    /// `World.scheduleLightingUpdate`: queue a region unless the chunk under its middle is missing,
    /// folding it into one of the last 5 queued regions of the same kind when that is nearly free.
    fn schedule_light(&mut self, kind: Kind, lo: [i32; 3], hi: [i32; 3]) {
        if !self.chunk_exists((hi[0] + lo[0]) / 2, (hi[2] + lo[2]) / 2) {
            return;
        }
        if self.queue.iter_mut().rev().take(5).any(|r| r.kind == kind && r.merge(lo, hi)) {
            return;
        }
        self.queue.push(Region { kind, lo, hi });
        if self.queue.len() > 1_000_000 {
            log::warn!("light: more than 1000000 updates, dropping the queue");
            self.queue.clear();
        }
    }

    /// `World.neighborLightPropagationChanged`: cell (x, y, z) should now be at least `v` (the changed
    /// neighbour's level minus 1); queue it if its stored level differs from what it should be.
    fn neighbor_changed(&mut self, kind: Kind, x: i32, y: i32, z: i32, v: i32) {
        if !(0..H as i32).contains(&y) {
            return;
        }
        // One lookup of the cell's chunk gives the open-sky test (`sees_sky`), the block (`raw_block`) and the stored level.
        let Some(e) = self.chunks.get(&(x >> 4, z >> 4)) else { return };
        let (lx, lz) = ((x & 15) as usize, (z & 15) as usize);
        let v = match kind {
            Kind::Sky if y >= e.height[lz << 4 | lx] as i32 => 15,
            Kind::Sky => v,
            Kind::Block => v.max(light_value(e.blocks[idx(lx, y as usize, lz)]) as i32),
        };
        if nib(kind, e.light[idx(lx, y as usize, lz)]) as i32 != v {
            self.schedule_light(kind, [x, y, z], [x, y, z]);
        }
    }

    /// A cell's stored level and the level it should have: `max(neighbours) - max(opacity, 1)`, or its own source level
    /// (`MetadataChunkBlock.func_4127_a`). The cell's chunk is looked up once, and its neighbours' only where they lie across a
    /// chunk border (it used to be ~9 lookups). `y` is inside the world. `None`: the chunk is not loaded.
    fn relit(&self, kind: Kind, x: i32, y: i32, z: i32) -> Option<(i32, i32)> {
        let e = self.chunks.get(&(x >> 4, z >> 4))?;
        let (lx, lz) = ((x & 15) as usize, (z & 15) as usize);
        // Stored level of a cell beside this one (`saved_light`: y is clamped into the world).
        let at = |nx: i32, ny: i32, nz: i32| -> i32 {
            if nx >> 4 == x >> 4 && nz >> 4 == z >> 4 {
                nib(kind, e.light[idx((nx & 15) as usize, ny.clamp(0, H as i32 - 1) as usize, (nz & 15) as usize)]) as i32
            } else {
                self.saved_light(kind, nx, ny, nz) as i32
            }
        };
        let id = e.blocks[idx(lx, y as usize, lz)];
        let opacity = (light_opacity(id) as i32).max(1);
        let source = match kind {
            Kind::Sky => if y >= e.height[lz << 4 | lx] as i32 { 15 } else { 0 },
            Kind::Block => light_value(id) as i32,
        };
        let old = at(x, y, z);
        let new = if opacity >= 15 && source == 0 {
            0
        } else {
            let n = at(x - 1, y, z).max(at(x + 1, y, z)).max(at(x, y - 1, z)).max(at(x, y + 1, z)).max(at(x, y, z - 1)).max(at(x, y, z + 1));
            (n - opacity).max(0).max(source)
        };
        Some((old, new))
    }

    /// `MetadataChunkBlock.func_4127_a`: recompute every cell of the region, queueing the neighbours of
    /// each cell whose level changed.
    fn update_region(&mut self, mut r: Region) {
        if r.volume() > 32768 {
            log::warn!("light: region too large, skipping");
            return;
        }
        let kind = r.kind;
        let mut near: Option<(Key, bool)> = None; // chunk -> its 3x3 surroundings are loaded
        for x in r.lo[0]..=r.hi[0] {
            for z in r.lo[2]..=r.hi[2] {
                let key = (x >> 4, z >> 4);
                let ok = match near {
                    Some((k, ok)) if k == key => ok,
                    _ => {
                        // World.doChunksNearChunkExist(x, 0, z, 1): chunks of x-1..=x+1, z-1..=z+1.
                        let ok = ((x - 1) >> 4..=(x + 1) >> 4).all(|cx| ((z - 1) >> 4..=(z + 1) >> 4).all(|cz| self.chunks.contains_key(&(cx, cz))));
                        near = Some((key, ok));
                        ok
                    }
                };
                if !ok {
                    continue;
                }
                r.lo[1] = r.lo[1].max(0);
                r.hi[1] = r.hi[1].min(H as i32 - 1);
                for y in r.lo[1]..=r.hi[1] {
                    let Some((old, new)) = self.relit(kind, x, y, z) else { continue };
                    if old != new {
                        self.set_saved_light(kind, x, y, z, new as u8);
                        let p = (new - 1).max(0);
                        self.neighbor_changed(kind, x - 1, y, z, p);
                        self.neighbor_changed(kind, x, y - 1, z, p);
                        self.neighbor_changed(kind, x, y, z - 1, p);
                        // The +side neighbours are inside the region (and recomputed by this loop) unless
                        // this is the last cell on that axis.
                        if x + 1 >= r.hi[0] { self.neighbor_changed(kind, x + 1, y, z, p) }
                        if y + 1 >= r.hi[1] { self.neighbor_changed(kind, x, y + 1, z, p) }
                        if z + 1 >= r.hi[2] { self.neighbor_changed(kind, x, y, z + 1, p) }
                    }
                }
            }
        }
    }

    /// Process queued regions, newest first, until the queue is empty or `deadline` passes.
    pub(super) fn run_light(&mut self, deadline: Option<Instant>) {
        let mut n = 0u32;
        while let Some(r) = self.queue.pop() {
            let big = r.volume() > 256;
            self.update_region(r);
            n += 1;
            // The clock is read after a big region or every 32nd one: most regions are a single cell, which costs less than
            // `Instant::now` does (so the budget can overshoot by a few dozen cells, microseconds).
            if (big || n % 32 == 0) && deadline.is_some_and(|d| Instant::now() >= d) {
                break;
            }
        }
    }

    /// `Chunk.func_996_c`: the column's neighbours may need light from it (or it from them) between the
    /// two heights.
    fn schedule_lateral(&mut self, x: i32, z: i32) {
        let h = self.height_at(x, z);
        for (nx, nz) in [(x - 1, z), (x + 1, z), (x, z - 1), (x, z + 1)] {
            let nh = self.height_at(nx, nz);
            if nh > h {
                self.schedule_light(Kind::Sky, [nx, h, nz], [nx, nh, nz]);
            } else if nh < h {
                self.schedule_light(Kind::Sky, [nx, nh, nz], [nx, h, nz]);
            }
        }
    }

    /// Light bookkeeping of `Chunk.setBlockID` after the cell (x, y, z) became `id`; `h` is the
    /// column's height from before the change.
    fn light_after_set(&mut self, x: i32, y: i32, z: i32, id: u8, h: i32) {
        if light_opacity(id) != 0 {
            if y >= h {
                self.relight_column(x, y + 1, z);
            }
        } else if y == h - 1 {
            self.relight_column(x, y, z);
        }
        self.schedule_light(Kind::Sky, [x, y, z], [x, y, z]);
        self.schedule_light(Kind::Block, [x, y, z], [x, y, z]);
        self.schedule_lateral(x, z);
    }

    /// `Chunk.func_1003_g`: the column's height map changed because of an edit at or above `y`; fix
    /// the height, the straight-down skylight, and queue the sideways fallout.
    fn relight_column(&mut self, x: i32, y: i32, z: i32) {
        let Some(e) = self.chunks.get(&(x >> 4, z >> 4)) else { return };
        let (lx, lz) = ((x & 15) as usize, (z & 15) as usize);
        let old = e.height[lz << 4 | lx] as i32;
        let h = column_top(&e.blocks, lx, lz, old, y);
        if h == old {
            return;
        }
        self.chunks.get_mut(&(x >> 4, z >> 4)).unwrap().height[lz << 4 | lx] = h as u8;
        if h < old {
            (h..old).for_each(|yy| self.put_light(Kind::Sky, x, yy, z, 15));
        } else {
            self.schedule_light(Kind::Sky, [x, old, z], [x, h, z]);
            (old..h).for_each(|yy| self.put_light(Kind::Sky, x, yy, z, 0));
        }
        // Skylight under the new top, losing `opacity` (at least 1) per cell.
        let (mut yy, mut level) = (h, 15);
        while yy > 0 && level > 0 {
            yy -= 1;
            level = (level - (light_opacity(self.raw_block(x, yy, z)) as i32).max(1)).max(0);
            self.put_light(Kind::Sky, x, yy, z, level as u8);
        }
        while yy > 0 && light_opacity(self.raw_block(x, yy - 1, z)) == 0 {
            yy -= 1;
        }
        if yy != h {
            self.schedule_light(Kind::Sky, [x - 1, yy, z - 1], [x + 1, h, z + 1]);
        }
    }

    /// First light of a final chunk: `Chunk.generateSkylightMap` (column skylight from the height
    /// map), the lateral scheduling for every column, every emitter, and the seams with chunks that
    /// are already lit (their light is recomputed across the border both ways).
    fn init_chunk(&mut self, key: Key) {
        let (ox, oz) = (key.0 * 16, key.1 * 16);
        let mut emitters = Vec::new();
        {
            let e = self.chunks.get_mut(&key).unwrap();
            let light = Arc::make_mut(&mut e.light);
            light.fill(0);
            for x in 0..16 {
                for z in 0..16 {
                    let base = idx(x, 0, z);
                    let (mut level, mut y) = (15i32, H - 1);
                    loop {
                        level -= light_opacity(e.blocks[base + y]) as i32;
                        if level > 0 {
                            light[base + y] |= level as u8;
                        }
                        y -= 1;
                        if !(y > 0 && level > 0) {
                            break;
                        }
                    }
                }
            }
            for (i, &b) in e.blocks.iter().enumerate() {
                if light_value(b) > 0 {
                    emitters.push([ox + (i >> 11) as i32, (i & 127) as i32, oz + ((i >> 7) & 15) as i32]);
                }
            }
            e.lit = true;
            e.touched = true;
        }
        for x in 0..16 {
            for z in 0..16 {
                self.schedule_lateral(ox + x, oz + z);
            }
        }
        for p in emitters {
            self.schedule_light(Kind::Block, p, p);
        }
        // Seams: the border column on each side of every border with a lit neighbour. Sky light only needs
        // recomputing up to the tallest column of the two chunks (above that both sides are open sky);
        // block light only if either chunk holds any.
        for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            let nk = (key.0 + dx, key.1 + dz);
            let (top, glow) = match (self.chunks.get(&key), self.chunks.get(&nk)) {
                (Some(me), Some(n)) if n.lit => (
                    (*me.height.iter().chain(n.height.iter()).max().unwrap() as i32 + 1).min(H as i32 - 1),
                    me.light.iter().chain(n.light.iter()).any(|&v| v >> 4 != 0),
                ),
                _ => continue,
            };
            // Our border column and the neighbour's facing one: (x range, z range) of the strip.
            let strip = |cx: i32, cz: i32, d: (i32, i32), y1: i32| {
                let (x0, z0) = (cx * 16, cz * 16);
                let (xs, zs) = match d {
                    (-1, 0) => ((x0, x0), (z0, z0 + 15)),
                    (1, 0) => ((x0 + 15, x0 + 15), (z0, z0 + 15)),
                    (0, -1) => ((x0, x0 + 15), (z0, z0)),
                    _ => ((x0, x0 + 15), (z0 + 15, z0 + 15)),
                };
                ([xs.0, 0, zs.0], [xs.1, y1, zs.1])
            };
            for (k, d) in [(key, (dx, dz)), (nk, (-dx, -dz))] {
                let (lo, hi) = strip(k.0, k.1, d, top);
                self.schedule_light(Kind::Sky, lo, hi);
                if glow {
                    let (lo, hi) = strip(k.0, k.1, d, H as i32 - 1);
                    self.schedule_light(Kind::Block, lo, hi);
                }
            }
        }
    }
}

impl ChunkManager {
    /// `World.canExistingBlockSeeTheSky`.
    pub fn sees_sky(&self, x: i32, y: i32, z: i32) -> bool {
        if y < 0 {
            return false;
        }
        if y >= H as i32 {
            return true;
        }
        self.chunks.get(&(x >> 4, z >> 4)).is_some_and(|e| y >= e.height[((z & 15) << 4 | (x & 15)) as usize] as i32)
    }

    /// The stored level of the render thread's copy (tests).
    #[cfg(test)]
    pub(super) fn saved_light(&self, kind: Kind, x: i32, y: i32, z: i32) -> u8 {
        let y = y.clamp(0, H as i32 - 1) as usize;
        self.chunks.get(&(x >> 4, z >> 4)).map_or(0, |e| nib(kind, e.light[idx((x & 15) as usize, y, (z & 15) as usize)]))
    }

    fn light_send(&mut self, m: Msg) {
        if self.light.tx.send(m).is_ok() {
            self.light.outstanding += 1;
            self.light.idle = false;
        }
    }

    /// The engine gets the blocks of a chunk (new, or rewritten by populate); it is unlit until `light_init`.
    pub(super) fn light_chunk(&mut self, key: Key, blocks: Arc<Vec<u8>>) {
        self.light.lighting.remove(&key);
        self.light_send(Msg::Chunk(key, blocks));
    }

    /// The cell (x, y, z) became `id` (its height map is already updated here).
    pub(super) fn light_edit(&mut self, x: i32, y: i32, z: i32, id: u8) {
        self.light_send(Msg::Edit { x, y, z, id });
    }

    pub(super) fn light_remove(&mut self, key: Key) {
        self.light.lighting.remove(&key);
        self.light_send(Msg::Remove(key));
    }

    /// Ask for the first light of a chunk (`init_chunk`).
    pub(super) fn light_init(&mut self, key: Key) {
        self.light.lighting.insert(key);
        self.light_send(Msg::Init(key));
    }

    /// Every message sent is lit and the engine's queue is empty: meshes (which copy the light) wait for this, so an edit or a
    /// new chunk is meshed once, lit.
    pub(super) fn light_settled(&self) -> bool {
        self.light.outstanding == 0 && self.light.idle
    }

    fn apply_report(&mut self, r: Report) {
        self.light.outstanding = self.light.outstanding.saturating_sub(r.done);
        self.light.idle = r.idle;
        for (k, light, lit) in r.chunks {
            if let Some(e) = self.chunks.get_mut(&k) {
                e.light = light;
                e.lit = lit;
                if lit {
                    self.light.lighting.remove(&k);
                }
            }
        }
        for k in r.stale {
            if let Some(e) = self.chunks.get_mut(&k) {
                e.meshed = false;
            }
        }
    }

    /// Take the reports that are in. A dead light thread (it panicked) leaves the light as it is, and the gate open.
    pub(super) fn pump_light(&mut self) {
        loop {
            match self.light.rx.try_recv() {
                Ok(r) => self.apply_report(r),
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    if self.light.outstanding > 0 || !self.light.idle {
                        log::error!("light: the light thread is gone, light is no longer updated");
                    }
                    self.light.outstanding = 0;
                    self.light.idle = true;
                    return;
                }
            }
        }
    }

    /// Wait until the engine has lit everything sent so far (tests).
    #[cfg(test)]
    pub(super) fn settle_light(&mut self) {
        let t = Instant::now();
        loop {
            self.pump_light();
            if self.light_settled() {
                return;
            }
            assert!(t.elapsed() < Duration::from_secs(60), "the light thread did not settle");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Per frame: take the reports, and when the engine has nothing left to do send it the first light of up to 4 final chunks of
    /// the render circle that have all 8 neighbours loaded (nearest first). Meshing waits for the engine to settle again.
    pub(super) fn light_pending(&mut self, cx: i32, cz: i32) {
        self.pump_light();
        if !self.light_settled() {
            return;
        }
        let r2 = self.radius * self.radius;
        let ready: Vec<Key> = self
            .ring
            .iter()
            .take_while(|&&(dx, dz)| dx * dx + dz * dz <= r2)
            .map(|&(dx, dz)| (cx + dx, cz + dz))
            .filter(|&(x, z)| {
                self.chunks.get(&(x, z)).is_some_and(|e| !e.lit)
                    && !self.light.lighting.contains(&(x, z))
                    && self.is_final(x, z)
                    && (-1..=1).all(|dx| (-1..=1).all(|dz| self.chunks.contains_key(&(x + dx, z + dz))))
            })
            .take(4)
            .collect();
        for k in ready {
            self.light_init(k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::chunk::VOLUME;

    /// 3x3 chunks of flat stone up to y = 9, all lit by the light thread, queue drained.
    fn flat() -> ChunkManager {
        let mut m = ChunkManager::new(1, 1);
        for cz in -1..=1 {
            for cx in -1..=1 {
                let mut b = vec![0u8; VOLUME];
                for x in 0..16 {
                    for z in 0..16 {
                        for y in 0..10 {
                            b[idx(x, y, z)] = 1;
                        }
                    }
                }
                m.insert((cx, cz), b);
            }
        }
        for cz in -1..=1 {
            for cx in -1..=1 {
                m.light_init((cx, cz));
            }
        }
        settle(&mut m);
        m.chunks.values_mut().for_each(|e| e.meshed = true); // so the test can see what an edit invalidates
        m
    }

    fn settle(m: &mut ChunkManager) {
        m.settle_light();
    }

    /// The same 3x3 flat world in an engine run directly (no thread, no messages across): `apply` is what the thread does with
    /// each message, `run_light(None)` drains the queue.
    fn flat_world() -> LightWorld {
        let mut w = LightWorld::default();
        for cz in -1..=1 {
            for cx in -1..=1 {
                let mut b = vec![0u8; VOLUME];
                for x in 0..16 {
                    for z in 0..16 {
                        for y in 0..10 {
                            b[idx(x, y, z)] = 1;
                        }
                    }
                }
                w.apply(Msg::Chunk((cx, cz), std::sync::Arc::new(b)));
            }
        }
        for cz in -1..=1 {
            for cx in -1..=1 {
                w.apply(Msg::Init((cx, cz)));
            }
        }
        w.run_light(None);
        w
    }

    /// `relit` (one chunk lookup per cell) gives what the plain formula of `update_region` gives (`saved_light` / `raw_block` /
    /// open-sky test for the cell and its six neighbours): mid-chunk, on borders and corners, at the outer edge of the loaded area and
    /// at the bottom and top of the world, with sky and block light in a settled state and in a stale one.
    #[test]
    fn relit_matches_the_plain_formula() {
        let slow = |w: &LightWorld, kind: Kind, x: i32, y: i32, z: i32| {
            let old = w.saved_light(kind, x, y, z) as i32;
            let id = w.raw_block(x, y, z);
            let opacity = (light_opacity(id) as i32).max(1);
            let sees_sky = y >= H as i32
                || (y >= 0 && w.chunks.get(&(x >> 4, z >> 4)).is_some_and(|e| y >= e.height[((z & 15) << 4 | (x & 15)) as usize] as i32));
            let source = match kind {
                Kind::Sky => if sees_sky { 15 } else { 0 },
                Kind::Block => light_value(id) as i32,
            };
            let new = if opacity >= 15 && source == 0 {
                0
            } else {
                let n = [(-1, 0, 0), (1, 0, 0), (0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1)]
                    .iter()
                    .map(|&(dx, dy, dz)| w.saved_light(kind, x + dx, y + dy, z + dz) as i32)
                    .max()
                    .unwrap_or(0);
                (n - opacity).max(0).max(source)
            };
            (old, new)
        };
        let mut w = flat_world();
        w.apply(Msg::Edit { x: 14, y: 11, z: 8, id: 89 }); // glowstone near the +X border
        w.apply(Msg::Edit { x: 5, y: 12, z: 5, id: 1 }); // a roof
        w.run_light(None);
        w.apply(Msg::Edit { x: 0, y: 11, z: 0, id: 89 }); // not run: the stored light is stale around it, so old != new somewhere
        w.apply(Msg::Edit { x: 16, y: 10, z: 15, id: 1 });
        let mut differs = false;
        for kind in [Kind::Sky, Kind::Block] {
            for (x, z) in [(5, 5), (0, 0), (15, 15), (16, 0), (-1, -1), (15, 8), (8, 0), (14, 8), (16, 15), (31, 31), (-16, -16), (0, 15)] {
                for y in [0, 1, 9, 10, 11, 12, 60, 126, 127] {
                    let want = slow(&w, kind, x, y, z);
                    differs |= want.0 != want.1;
                    assert_eq!(w.relit(kind, x, y, z), Some(want), "{kind:?} ({x}, {y}, {z})");
                }
            }
        }
        assert!(differs, "the stale state must exercise old != new");
        assert_eq!(w.relit(Kind::Sky, 100, 5, 0), None, "chunk not loaded");
    }

    /// The light thread (messages in, reports out, applied to the render thread's copy) lights a world exactly as the engine does
    /// when it is run directly, edits included: the same light arrays, height maps and blocks in every chunk after each step.
    #[test]
    fn light_thread_matches_the_engine() {
        let mut m = flat();
        let mut w = flat_world();
        for (x, y, z, id) in [(5, 12, 5, 1u8), (14, 11, 8, 89), (0, 11, 0, 89), (16, 10, 15, 1), (5, 12, 5, 0), (14, 11, 8, 0), (20, 30, 3, 1)] {
            m.set_block(x, y, z, id);
            w.apply(Msg::Edit { x, y, z, id });
            w.run_light(None);
            m.settle_light();
            for (k, e) in &m.chunks {
                let c = &w.chunks[k];
                assert!(e.blocks == c.blocks && e.height == c.height, "blocks or height of chunk {k:?} after ({x}, {y}, {z}) = {id}");
                assert!(e.light == c.light, "light of chunk {k:?} after ({x}, {y}, {z}) = {id}");
                assert!(e.lit, "chunk {k:?} lit");
            }
        }
    }

    /// Sky light: 15 over open ground, 0 inside stone, a roof dims the cell under it by one step of
    /// lateral spread (14), and removing the roof restores 15. Block light: a glowstone lights 15
    /// at the source, falls 1 per cell, crosses a chunk border, and is gone again when the source is removed.
    #[test]
    fn light_settles_after_edits() {
        let mut m = flat();
        let sky = |m: &ChunkManager, x, y, z| m.saved_light(Kind::Sky, x, y, z);
        let blk = |m: &ChunkManager, x, y, z| m.saved_light(Kind::Block, x, y, z);
        assert_eq!((sky(&m, 5, 20, 5), sky(&m, 5, 10, 5), sky(&m, 5, 5, 5)), (15, 15, 0));

        m.set_block(5, 12, 5, 1); // roof one block wide
        settle(&mut m);
        assert_eq!((sky(&m, 5, 12, 5), sky(&m, 5, 11, 5), sky(&m, 5, 13, 5)), (0, 14, 15));
        m.set_block(5, 12, 5, 0);
        settle(&mut m);
        assert_eq!(sky(&m, 5, 11, 5), 15);

        m.set_block(14, 11, 8, 89); // glowstone, 2 cells from the +X chunk border (x = 16)
        settle(&mut m);
        assert_eq!((blk(&m, 14, 11, 8), blk(&m, 15, 11, 8), blk(&m, 17, 11, 8), blk(&m, 24, 11, 8)), (15, 14, 12, 5));
        m.set_block(14, 11, 8, 0);
        settle(&mut m);
        assert_eq!((blk(&m, 14, 11, 8), blk(&m, 15, 11, 8), blk(&m, 17, 11, 8)), (0, 0, 0));
        // Meshes of both touched chunks were marked stale.
        assert!(!m.chunks[&(0, 0)].meshed && !m.chunks[&(1, 0)].meshed);
    }
}
