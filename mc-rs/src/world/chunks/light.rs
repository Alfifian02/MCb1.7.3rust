//! M5 light engine: ScalableLux's (Starlight's) light engine in the `starlight` crate, on top of the b1.7.3 rules: two light kinds per
//! cell (sky: 15 straight down from open sky, `y >= height`; block: emitters, `Block.lightValue`), a cell holds
//! `max(own source, max(neighbours) - max(opacity, 1))`. The b1.7.3 version (`MetadataChunkBlock.func_4127_a`: recompute a box of
//! cells from their six neighbours until nothing changes) is gone; the settled light is the same (the crate's tests check it against a
//! brute-force fixpoint, and `settled_light_is_the_plain_formula` below against the old formula with the real block tables).
//!
//! The engine runs on its own thread (`chunk-light`, C2ME's `threading-lighting` idea): `LightWorld` keeps its own copy of every
//! chunk's blocks and height map and is the only writer of the light; the render thread tells it what happens (`Msg`: a chunk's
//! blocks, an edit, an unload, the first light of a chunk) and keeps a read-only copy of the light that the thread's `Report`s
//! replace. So a region never waits for the frame. The chunk arrays are `Arc`s shared with the render thread while nobody writes
//! (`Arc::make_mut` copies a chunk's array on its first write from either side), so an untouched chunk costs one copy of blocks and light.
//!
//! Stateless like Starlight: a message changes the blocks and the height map at once and only queues a `Task` (the cell, or the
//! first light of the chunk); the light is computed when the task runs, from the blocks as they are then, and every edit of a chunk
//! that piled up since is one pass. Tasks whose 3x3 chunk windows are disjoint run in parallel (ScalableLux's parallel light
//! updates, `starlight::run`; `starlight::default_workers()` threads, a third of the cores: one task runs on the light thread itself).
//!
//! Differences from vanilla, none of which change the settled result:
//! - the queue is drained in time slices on the light thread (`run_light`) instead of `updatingLighting`'s 500 regions;
//! - a freshly final chunk is lit in one go (`Msg::Init`) because worldgen writes raw blocks instead of calling
//!   `setBlockWithNotify`, so no per-block updates were ever queued for them;
//! - a chunk that is not lit is not part of the light world (nothing is written into it, it is read as absent); its first light
//!   checks its seams with the lit chunks around it, both ways;
//! - no day/night: `skylightSubtracted` is 0 (see `render::mesh`).

use super::{ChunkManager, Key, KeyHasher};
use crate::world::chunk::{height_map, idx, light_opacity, light_value, H, VOLUME};
use starlight::{Blocks, Chunk as LChunk, Task};
#[cfg(test)]
use starlight::Kind;
use std::collections::{HashMap, HashSet};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

/// The block tables the engine reads (`getLightDampening`, `getLightEmission`).
struct Mc;

impl Blocks for Mc {
    fn opacity(id: u8) -> u8 {
        light_opacity(id)
    }
    fn emission(id: u8) -> u8 {
        light_value(id)
    }
}

// The crate has its own copy of the chunk layout; it must be this one (`idx` is checked by `chunk_layout_matches_the_engine`).
const _: () = assert!(starlight::H == H && starlight::VOLUME == VOLUME);

/// The `kind` half of a raw light byte (sky in the low nibble, block light in the high one).
#[cfg(test)]
fn nib(kind: Kind, v: u8) -> u8 {
    if kind == Kind::Sky { v & 15 } else { v >> 4 }
}

/// How long the light thread works on its queue before it reports what it has (the render thread sees nothing before that).
const LIGHT_SLICE: Duration = Duration::from_millis(5);

type LChunks = HashMap<Key, LChunk, std::hash::BuildHasherDefault<KeyHasher>>;

/// The engine's state: what `ChunkManager` used to hold for light, on the light thread.
#[derive(Default)]
struct LightWorld {
    /// What the engine knows of each chunk: its own copy of the blocks and of the height map (kept in step by `Msg::Chunk` and
    /// `Msg::Edit`), and the light, which only the engine writes.
    chunks: LChunks,
    /// Pending light work per chunk (`StarLightInterface`'s `ChunkTasks`).
    tasks: HashMap<Key, Task>,
    /// Threads for `starlight::run` (0 = 1).
    workers: usize,
}

/// What the render thread tells the engine, in the order it happened.
pub(super) enum Msg {
    /// The blocks of a chunk, new or rewritten by populate: the height map is rebuilt, and the chunk is unlit until `Init`.
    Chunk(Key, Arc<Vec<u8>>),
    /// The cell (x, y, z) became `id`.
    Edit { x: i32, y: i32, z: i32, id: u8 },
    /// The chunk was unloaded.
    Remove(Key),
    /// First light of the chunk (`light_chunk`).
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
    let mut w = LightWorld { workers: starlight::default_workers(), ..Default::default() };
    let mut n = 0usize;
    loop {
        if w.tasks.is_empty() {
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
                self.tasks.remove(&k);
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
                let k = (x >> 4, z >> 4);
                let Some(e) = self.chunks.get_mut(&k) else { return };
                let (lx, lz) = ((x & 15) as usize, (z & 15) as usize);
                let h = e.height[lz << 4 | lx] as i32;
                Arc::make_mut(&mut e.blocks)[idx(lx, y as usize, lz)] = id;
                if let Some(top) = height_after_set(&e.blocks, lx, lz, id, y, h) {
                    e.height[lz << 4 | lx] = top as u8;
                }
                // An unlit chunk lights from its blocks as they are when its first light runs.
                if e.lit {
                    self.tasks.entry(k).or_default().edits.push([x, y, z]);
                }
            }
            Msg::Remove(k) => {
                self.chunks.remove(&k);
                self.tasks.remove(&k);
            }
            Msg::Init(k) => {
                if self.chunks.contains_key(&k) {
                    self.tasks.entry(k).or_default().init = true;
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
        Report { done, idle: self.tasks.is_empty(), chunks, stale }
    }

    /// `World.getSavedLightValue`: y is clamped into the world, an unloaded chunk is dark (tests).
    #[cfg(test)]
    pub(super) fn saved_light(&self, kind: Kind, x: i32, y: i32, z: i32) -> u8 {
        let y = y.clamp(0, H as i32 - 1) as usize;
        self.chunks.get(&(x >> 4, z >> 4)).map_or(0, |e| nib(kind, e.light[idx((x & 15) as usize, y, (z & 15) as usize)]))
    }

    /// Run the queued tasks until none are left or `deadline` passes (checked between waves of parallel tasks).
    pub(super) fn run_light(&mut self, deadline: Option<Instant>) {
        starlight::run::<Mc, _>(&mut self.chunks, &mut self.tasks, self.workers, deadline);
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

    /// The settled light of the engine is what the plain b1.7.3 formula gives (`MetadataChunkBlock.func_4127_a`: `max(neighbours) -
    /// max(opacity, 1)` or the cell's own source level, with the real block tables), at mid-chunk cells, borders and corners, the outer
    /// edge of the loaded area and the bottom and top of the world, after edits (a roof, glowstone near a border, a removed one).
    #[test]
    fn settled_light_is_the_plain_formula() {
        let mut w = flat_world();
        for (x, y, z, id) in [(14, 11, 8, 89u8), (5, 12, 5, 1), (0, 11, 0, 89), (16, 10, 15, 1), (14, 11, 8, 0)] {
            w.apply(Msg::Edit { x, y, z, id });
            w.run_light(None);
        }
        for kind in [Kind::Sky, Kind::Block] {
            for (x, z) in [(5, 5), (0, 0), (15, 15), (16, 0), (-1, -1), (15, 8), (8, 0), (14, 8), (16, 15), (31, 31), (-16, -16), (0, 15)] {
                for y in [0, 1, 9, 10, 11, 12, 60, 126, 127] {
                    let Some(e) = w.chunks.get(&(x >> 4, z >> 4)) else { continue };
                    let id = e.blocks[idx((x & 15) as usize, y as usize, (z & 15) as usize)];
                    let source = match kind {
                        Kind::Sky => if y >= e.height[((z & 15) << 4 | (x & 15)) as usize] as i32 { 15 } else { 0 },
                        Kind::Block => light_value(id) as i32,
                    };
                    let n = [(-1, 0, 0), (1, 0, 0), (0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1)]
                        .iter()
                        .filter(|&&(_, dy, _)| (0..H as i32).contains(&(y + dy)))
                        .map(|&(dx, dy, dz)| w.saved_light(kind, x + dx, y + dy, z + dz) as i32)
                        .max()
                        .unwrap_or(0);
                    let want = (n - (light_opacity(id) as i32).max(1)).max(0).max(source);
                    assert_eq!(w.saved_light(kind, x, y, z) as i32, want, "{kind:?} ({x}, {y}, {z})");
                }
            }
        }
    }

    /// The crate's copy of the cell layout is the game's.
    #[test]
    fn chunk_layout_matches_the_engine() {
        assert_eq!(idx(3, 5, 7), 3 << 11 | 7 << 7 | 5);
        assert_eq!((starlight::H, starlight::VOLUME), (H, VOLUME));
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
