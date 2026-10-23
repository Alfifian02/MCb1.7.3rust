//! Rust port of the core of ScalableLux 26.2.0 (Starlight, LGPL-3.0): the light engine that replaces the vanilla one.
//! std only, no `unsafe`; the only threads are the scoped ones of [`run`].
//!
//! What is ported (`StarLightEngine`, `BlockStarLightEngine`, `SkyStarLightEngine`, `StarLightInterface`):
//! - the propagator: one FIFO of increases and one of decreases per light kind, entries carry a direction bitset (the
//!   neighbour a level came from is never asked again) and the `WRITE` / `RECHECK` flags (`increase`, `decrease`);
//! - `checkBlock` / `calculateLightValue`: a changed block resets its cell and re-derives it from its neighbours;
//! - stateless sky sources: a column's sources are `y >= height`, fixed up by scanning the light of the column (`edits`),
//!   and the first light of a chunk queues only the source cells that touch a darker column (`light_chunk`);
//! - chunk edge checks when a chunk is lit beside lit neighbours (`check_edges`), and no light written into unlit chunks;
//! - ScalableLux's parallel light updates: a task owns a 3x3 window of chunks (light never travels further than 15 cells, a
//!   chunk is 16), tasks with disjoint windows run on scoped threads ([`run`]).
//!
//! Left out on purpose: `SWMRNibbleArray` (null / zero / full nibble states, the host keeps flat arrays), the vanilla light
//! interface, the save format (the host does not save light), conditionally transparent blocks (b1.7.3 has none) and
//! section emptiness maps (a chunk here is one 128-high array).
//!
//! The settled light is the same as the b1.7.3 relaxation's: `L(c) = max(source(c), max(L(n)) - max(opacity(c), 1))`, with
//! sky sources at `y >= height` and block sources at the cell's emission; `tests` check it against a brute-force fixpoint.
//!
//! Cell layout of the host (`mc-rs/src/world/chunk.rs`): index `x << 11 | z << 7 | y`, `H` = 128 high, one light byte per cell
//! with sky in the low nibble and block light in the high one, `height[z << 4 | x]` = first y with no opaque cell above.

use std::collections::HashMap;
use std::hash::BuildHasher;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Instant;

pub const H: usize = 128;
pub const VOLUME: usize = 16 * 16 * H;
pub type Key = (i32, i32);

/// What the engine needs to know about a block id (`getLightDampening`, `getLightEmission`).
pub trait Blocks {
    fn opacity(id: u8) -> u8;
    fn emission(id: u8) -> u8;
}

/// One chunk as the engine sees it. `blocks` and `height` belong to the host (the engine only reads them), `light` is
/// written by the engine alone; `touched` / `stale` say what the host must pick up (light array changed / mesh outdated).
pub struct Chunk {
    pub blocks: Arc<Vec<u8>>,
    pub light: Arc<Vec<u8>>,
    pub height: [u8; 256],
    /// `light_chunk` ran and the blocks have not been replaced since. Unlit chunks are invisible to the engine's neighbours.
    pub lit: bool,
    pub touched: bool,
    pub stale: bool,
}

/// Work queued for one chunk: its first light, and block changes (world coordinates) in it. Stateless like Starlight's
/// `ChunkTasks`: the engine reads the blocks as they are when the task runs, so edits can be applied to the host's chunk at
/// once and their light queued here, in any amount, and one pass handles them all.
#[derive(Default)]
pub struct Task {
    pub init: bool,
    pub edits: Vec<[i32; 3]>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Sky,
    Block,
}

impl Kind {
    fn shift(self) -> u32 {
        if self == Kind::Sky { 0 } else { 4 }
    }
}

// Directions: -x, +x, -z, +z, -y, +y; the opposite of `d` is `d ^ 1`.
const DIRS: [(i32, i32, i32); 6] = [(-1, 0, 0), (1, 0, 0), (0, 0, -1), (0, 0, 1), (0, -1, 0), (0, 1, 0)];
const DOWN: u8 = 4;
const UP: u8 = 5;
const ALL: u8 = 63;
const WRITE: u8 = 1; // the increase writes the level to its own cell first (re-propagated block sources)
const RECHECK: u8 = 2; // the increase only runs if its cell still holds the level it was queued with

fn but(d: usize) -> u8 {
    ALL & !(1 << (d ^ 1))
}

/// A queue entry, window coordinates (`x`, `z` in 0..48): `lvl` is the level the cell holds (increase) or the level to
/// take away from the neighbours (decrease); `dirs` the neighbours to visit.
#[derive(Clone, Copy)]
struct Q {
    x: u8,
    y: u8,
    z: u8,
    lvl: u8,
    dirs: u8,
    flags: u8,
}

fn q(x: i32, y: i32, z: i32, lvl: u8, dirs: u8, flags: u8) -> Q {
    Q { x: x as u8, y: y as u8, z: z as u8, lvl, dirs, flags }
}

/// The 3x3 chunks a task owns, moved out of the host's map for the length of the task (so tasks with disjoint windows can
/// run on different threads with no lock). Only the centre and lit neighbours are in it: the others read as absent.
struct Window {
    cx0: i32,
    cz0: i32,
    c: [Option<Chunk>; 9],
}

impl Window {
    fn take<S: BuildHasher>(map: &mut HashMap<Key, Chunk, S>, key: Key) -> Option<Self> {
        if !map.contains_key(&key) {
            return None;
        }
        let mut c: [Option<Chunk>; 9] = Default::default();
        for s in 0..9i32 {
            let k = (key.0 - 1 + s % 3, key.1 - 1 + s / 3);
            if s == 4 || map.get(&k).is_some_and(|e| e.lit) {
                c[s as usize] = map.remove(&k);
            }
        }
        Some(Self { cx0: key.0 - 1, cz0: key.1 - 1, c })
    }

    fn put_back<S: BuildHasher>(self, map: &mut HashMap<Key, Chunk, S>) {
        for (s, c) in self.c.into_iter().enumerate() {
            if let Some(c) = c {
                map.insert((self.cx0 + s as i32 % 3, self.cz0 + s as i32 / 3), c);
            }
        }
    }

    /// (chunk slot, cell index) of a window cell; `None` outside the world, the window or a present chunk.
    fn at(&self, x: i32, y: i32, z: i32) -> Option<(usize, usize)> {
        if !(0..48).contains(&x) || !(0..48).contains(&z) || !(0..H as i32).contains(&y) {
            return None;
        }
        let s = (x >> 4) as usize + 3 * (z >> 4) as usize;
        self.c[s].as_ref()?;
        Some((s, ((x & 15) as usize) << 11 | ((z & 15) as usize) << 7 | y as usize))
    }

    fn ch(&self, s: usize) -> &Chunk {
        self.c[s].as_ref().unwrap()
    }

    fn lvl(&self, k: Kind, s: usize, i: usize) -> u8 {
        self.ch(s).light[i] >> k.shift() & 15
    }

    fn id(&self, s: usize, i: usize) -> u8 {
        self.ch(s).blocks[i]
    }

    /// `Chunk.heightMap` of the column holding the cell `i`.
    fn height(&self, s: usize, i: usize) -> i32 {
        self.ch(s).height[(i >> 7 & 15) << 4 | i >> 11] as i32
    }

    fn height_at(&self, x: i32, z: i32) -> Option<i32> {
        self.at(x, 0, z).map(|(s, i)| self.height(s, i))
    }

    /// Write a nibble; a change marks the chunk (and the neighbour across a border the cell touches: its border faces look at
    /// this cell) as touched / stale.
    fn put(&mut self, k: Kind, s: usize, i: usize, v: u8) {
        let sh = k.shift();
        let c = self.c[s].as_mut().unwrap();
        let old = c.light[i];
        let new = old & !(15 << sh) | v << sh;
        if new == old {
            return;
        }
        // ponytail: `make_mut` is an atomic check per write (the arrays are shared with the host between reports); unshare once per chunk to remove it.
        Arc::make_mut(&mut c.light)[i] = new;
        c.touched = true;
        c.stale = true;
        let (lx, lz) = (i >> 11, i >> 7 & 15);
        for (hit, ok, o) in [(lx == 0, s % 3 > 0, -1i32), (lx == 15, s % 3 < 2, 1), (lz == 0, s / 3 > 0, -3), (lz == 15, s / 3 < 2, 3)] {
            if hit && ok {
                if let Some(n) = self.c[(s as i32 + o) as usize].as_mut() {
                    n.stale = true;
                }
            }
        }
    }
}

/// The propagator and the two task kinds, with its queues kept between calls (`StarLightEngine`'s increase / decrease queues).
struct Engine<B> {
    inc: Vec<Q>,
    dec: Vec<Q>,
    _b: PhantomData<fn() -> B>,
}

impl<B: Blocks> Engine<B> {
    fn new() -> Self {
        Self { inc: Vec::new(), dec: Vec::new(), _b: PhantomData }
    }

    /// `performLightIncrease`: raise the neighbours of every entry to `level - max(1, opacity)` where that is more than they
    /// hold, and queue them. The cell an entry came from is not visited again, and nothing below level 2 is queued.
    fn increase(&mut self, w: &mut Window, k: Kind) {
        let mut r = 0;
        while r < self.inc.len() {
            let e = self.inc[r];
            r += 1;
            let (x, y, z) = (e.x as i32, e.y as i32, e.z as i32);
            if e.flags & RECHECK != 0 {
                if w.at(x, y, z).map(|(s, i)| w.lvl(k, s, i)) != Some(e.lvl) {
                    continue; // something changed since it was queued
                }
            } else if e.flags & WRITE != 0 {
                if let Some((s, i)) = w.at(x, y, z) {
                    w.put(k, s, i, e.lvl);
                }
            }
            for d in 0..6 {
                if e.dirs >> d & 1 == 0 {
                    continue;
                }
                let (nx, ny, nz) = (x + DIRS[d].0, y + DIRS[d].1, z + DIRS[d].2);
                let Some((s, i)) = w.at(nx, ny, nz) else { continue };
                let cur = w.lvl(k, s, i) as i32;
                if cur >= e.lvl as i32 - 1 {
                    continue; // already as bright as this entry could make it, and no block read for it
                }
                let t = e.lvl as i32 - (B::opacity(w.id(s, i)) as i32).max(1);
                if t <= cur {
                    continue;
                }
                w.put(k, s, i, t as u8);
                if t > 1 {
                    self.inc.push(q(nx, ny, nz, t as u8, but(d), 0));
                }
            }
        }
        self.inc.clear();
    }

    /// `performLightDecrease`, then the increases it (and the caller) queued. A neighbour brighter than what this entry could
    /// have given it is lit by something else: queue it again (`RECHECK`); otherwise zero it (queueing its own emission
    /// again) and go on from it.
    fn decrease(&mut self, w: &mut Window, k: Kind) {
        let mut r = 0;
        while r < self.dec.len() {
            let e = self.dec[r];
            r += 1;
            let (x, y, z) = (e.x as i32, e.y as i32, e.z as i32);
            for d in 0..6 {
                if e.dirs >> d & 1 == 0 {
                    continue;
                }
                let (nx, ny, nz) = (x + DIRS[d].0, y + DIRS[d].1, z + DIRS[d].2);
                let Some((s, i)) = w.at(nx, ny, nz) else { continue };
                let l = w.lvl(k, s, i);
                if l == 0 {
                    continue;
                }
                let id = w.id(s, i);
                let t = (e.lvl as i32 - (B::opacity(id) as i32).max(1)).max(0);
                if l as i32 > t {
                    self.inc.push(q(nx, ny, nz, l, ALL, RECHECK));
                    continue;
                }
                if k == Kind::Block && B::emission(id) != 0 {
                    self.inc.push(q(nx, ny, nz, B::emission(id), ALL, WRITE));
                }
                w.put(k, s, i, 0);
                if t > 0 {
                    self.dec.push(q(nx, ny, nz, t as u8, but(d), 0));
                }
            }
        }
        self.dec.clear();
        self.increase(w, k);
    }

    /// `checkBlock`: the cell's block changed (opacity, emission): drop it to what the block gives by itself and let the
    /// decrease re-derive the neighbours (which re-derives this cell from them). A sky source keeps its 15.
    fn check_block(&mut self, w: &mut Window, k: Kind, x: i32, y: i32, z: i32) {
        let Some((s, i)) = w.at(x, y, z) else { return };
        let cur = w.lvl(k, s, i);
        if k == Kind::Block {
            let e = B::emission(w.id(s, i));
            w.put(k, s, i, e);
            if e != 0 {
                self.inc.push(q(x, y, z, e, ALL, 0));
            }
        } else if cur == 15 {
            self.inc.push(q(x, y, z, 15, ALL, 0));
        } else {
            w.put(k, s, i, 0);
        }
        self.dec.push(q(x, y, z, cur, ALL, 0));
    }

    /// `calculateLightValue`: the level a cell should hold. Stops early: more than `expect` means "at least this", equal to it
    /// means correct, less than it is the real value.
    fn calc(&self, w: &Window, k: Kind, x: i32, y: i32, z: i32, expect: i32) -> i32 {
        let Some((s, i)) = w.at(x, y, z) else { return 0 };
        let id = w.id(s, i);
        let mut level = match k {
            Kind::Block => B::emission(id) as i32,
            Kind::Sky => if y >= w.height(s, i) { 15 } else { 0 },
        };
        let op = (B::opacity(id) as i32).max(1);
        if level >= 14 || level > expect || op >= 15 {
            return level;
        }
        for d in 0..6 {
            let Some((s2, i2)) = w.at(x + DIRS[d].0, y + DIRS[d].1, z + DIRS[d].2) else { continue };
            let nl = w.lvl(k, s2, i2) as i32;
            if nl - 1 <= level {
                continue;
            }
            level = level.max(nl - op);
            if level > expect {
                return level;
            }
        }
        level
    }

    /// `checkChunkEdges`: both sides of every seam with a lit neighbour must agree with what their neighbours say. Wrong cells
    /// are collected first (`check_block` clobbers light that later `calc`s would read) and then checked.
    fn check_edges(&mut self, w: &mut Window) {
        for k in [Kind::Sky, Kind::Block] {
            let mut bad: Vec<[i32; 3]> = Vec::new();
            for d in 0..4 {
                let (dx, dz) = (DIRS[d].0, DIRS[d].2);
                if w.c[(4 + dx + 3 * dz) as usize].is_none() {
                    continue;
                }
                let seam = |t: i32| if dx != 0 { (if dx < 0 { 16 } else { 31 }, 16 + t) } else { (16 + t, if dz < 0 { 16 } else { 31 }) };
                // Above both columns' heights every cell on both sides is a source: only the block light has to look there.
                let top = if k == Kind::Sky {
                    (0..16).flat_map(|t| { let (x, z) = seam(t); [w.height_at(x, z), w.height_at(x + dx, z + dz)] }).flatten().max().unwrap_or(0).min(H as i32 - 1) + 1
                } else {
                    H as i32
                };
                for t in 0..16 {
                    let (x, z) = seam(t);
                    for y in 0..top {
                        for (cx, cz) in [(x, z), (x + dx, z + dz)] {
                            let (s, i) = w.at(cx, y, cz).unwrap();
                            let cur = w.lvl(k, s, i) as i32;
                            if self.calc(w, k, cx, y, cz, cur) != cur {
                                bad.push([cx, y, cz]);
                            }
                        }
                    }
                }
            }
            bad.sort_unstable();
            bad.dedup();
            for [x, y, z] in bad {
                self.check_block(w, k, x, y, z);
            }
            self.decrease(w, k);
        }
    }

    /// `lightChunk`: the first light of the centre chunk: sky sources at `y >= height`, block sources, propagation, edge checks.
    fn light_chunk(&mut self, w: &mut Window) {
        let c = w.c[4].as_mut().unwrap();
        c.light = Arc::new(vec![0; VOLUME]);
        (c.lit, c.touched, c.stale) = (true, true, true);
        {
            let Chunk { light, height, blocks, .. } = c;
            let light = Arc::get_mut(light).unwrap();
            for col in 0..256 {
                let base = (col & 15) << 11 | (col >> 4) << 7;
                light[base + height[col] as usize..base + H].fill(15);
            }
            // ponytail: reads all 32768 blocks; Starlight skips sections whose palette has no emitter (a per-chunk emitter count would do).
            for (i, &id) in blocks.iter().enumerate() {
                let e = B::emission(id);
                if e != 0 {
                    light[i] |= e << 4;
                    self.inc.push(q(16 + (i >> 11) as i32, (i & 127) as i32, 16 + (i >> 7 & 15) as i32, e, ALL, 0));
                }
            }
        }
        let block_sources = std::mem::take(&mut self.inc);
        // Sky: only the source cells beside a darker column propagate (the top block under a column too).
        for col in 0..256 {
            let (x, z) = (16 + (col & 15) as i32, 16 + (col >> 4) as i32);
            let h = w.ch(4).height[col] as i32;
            let nh = [(x - 1, z), (x + 1, z), (x, z - 1), (x, z + 1)].map(|(nx, nz)| w.height_at(nx, nz));
            let top = nh.iter().flatten().copied().max().unwrap_or(0).max(h + 1).min(H as i32);
            for y in h..top {
                let mut dirs = (0..4).filter(|&d| nh[d].is_some_and(|n| n > y)).fold(0u8, |m, d| m | 1 << d);
                if y == h && h > 0 {
                    dirs |= 1 << DOWN;
                }
                if dirs != 0 {
                    self.inc.push(q(x, y, z, 15, dirs, 0));
                }
            }
        }
        self.increase(w, Kind::Sky);
        self.inc = block_sources;
        self.increase(w, Kind::Block);
        self.check_edges(w);
    }

    /// `propagateBlockChanges` for both kinds. The host already holds the new blocks and heights. Sky: the column's source run
    /// is brought in line with its height (dark cells at and above it become sources, lit cells below it stop being), then
    /// every changed cell is checked; positions that are sources after the change need nothing.
    fn edits(&mut self, w: &mut Window, pos: &[[i32; 3]]) {
        let mut cols: Vec<(i32, i32)> = pos.iter().map(|p| (p[0], p[2])).collect();
        cols.sort_unstable();
        cols.dedup();
        for (x, z) in cols {
            let Some(h) = w.height_at(x, z) else { continue };
            for y in h..H as i32 {
                let (s, i) = w.at(x, y, z).unwrap();
                if w.lvl(Kind::Sky, s, i) == 15 {
                    break;
                }
                w.put(Kind::Sky, s, i, 15);
                self.inc.push(q(x, y, z, 15, if y > h { ALL & !(1 << UP) & !(1 << DOWN) } else { ALL & !(1 << UP) }, 0));
            }
            for y in (0..h).rev() {
                let (s, i) = w.at(x, y, z).unwrap();
                if w.lvl(Kind::Sky, s, i) != 15 {
                    break;
                }
                w.put(Kind::Sky, s, i, 0);
                self.dec.push(q(x, y, z, 15, ALL & !(1 << UP), 0));
            }
        }
        for &[x, y, z] in pos {
            if w.height_at(x, z).is_some_and(|h| y < h) {
                self.check_block(w, Kind::Sky, x, y, z);
            }
        }
        self.decrease(w, Kind::Sky);
        for &[x, y, z] in pos {
            self.check_block(w, Kind::Block, x, y, z);
        }
        self.decrease(w, Kind::Block);
    }
}

/// One task on its window. `key` is the centre chunk; positions come in world coordinates.
fn run_task<B: Blocks>(item: &mut (Key, Task, Window)) {
    let (key, task, w) = item;
    let mut e = Engine::<B>::new();
    if task.init {
        e.light_chunk(w);
    }
    let (ox, oz) = (w.cx0 * 16, w.cz0 * 16);
    let pos: Vec<[i32; 3]> = task.edits.iter().filter(|p| (p[0] >> 4, p[2] >> 4) == *key).map(|p| [p[0] - ox, p[1], p[2] - oz]).collect();
    if !pos.is_empty() {
        e.edits(w, &pos);
    }
}

/// ScalableLux's `Config.PARALLELISM` default: a third of the cores, at least 1.
pub fn default_workers() -> usize {
    std::thread::available_parallelism().map_or(1, |n| (n.get() / 3).max(1))
}

/// Run queued tasks until none are left or `deadline` passes (checked between waves). A wave is up to `workers` tasks whose
/// windows are disjoint (centres more than 2 chunks apart in x or z), run on scoped threads; one task runs inline. Edits of a
/// chunk that is not lit, and tasks of a chunk that is gone, are dropped: its first light computes from the blocks it has.
/// ponytail: a wave waits for its slowest task; ScalableLux's per-chunk lock tokens on a persistent pool have no barrier.
pub fn run<B: Blocks, S: BuildHasher>(chunks: &mut HashMap<Key, Chunk, S>, tasks: &mut HashMap<Key, Task>, workers: usize, deadline: Option<Instant>) {
    while !tasks.is_empty() && deadline.map_or(true, |d| Instant::now() < d) {
        let mut wave: Vec<(Key, Task, Window)> = Vec::new();
        let keys: Vec<Key> = tasks.keys().copied().collect();
        for k in keys {
            if wave.len() >= workers.max(1) {
                break;
            }
            if wave.iter().any(|(o, ..)| (o.0 - k.0).abs() <= 2 && (o.1 - k.1).abs() <= 2) {
                continue; // waits for the next wave
            }
            let t = tasks.remove(&k).unwrap();
            if !chunks.get(&k).is_some_and(|c| t.init || c.lit) {
                continue;
            }
            if let Some(w) = Window::take(chunks, k) {
                wave.push((k, t, w));
            }
        }
        if let [one] = wave.as_mut_slice() {
            run_task::<B>(one);
        } else {
            std::thread::scope(|s| {
                for item in wave.iter_mut() {
                    s.spawn(move || run_task::<B>(item));
                }
            });
        }
        for (.., w) in wave {
            w.put_back(chunks);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 0 air, 1 stone, 2 leaves (opacity 1), 3 water (opacity 3), 4 glowstone (15), 5 torch (14).
    struct T;
    impl Blocks for T {
        fn opacity(id: u8) -> u8 {
            match id { 0 | 4 | 5 => 0, 2 => 1, 3 => 3, _ => 255 }
        }
        fn emission(id: u8) -> u8 {
            match id { 4 => 15, 5 => 14, _ => 0 }
        }
    }

    struct Rng(u64);
    impl Rng {
        fn next(&mut self, n: u32) -> u32 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((self.0 >> 33) as u32) % n
        }
    }

    fn cell(x: usize, y: usize, z: usize) -> usize {
        x << 11 | z << 7 | y
    }

    fn heights(blocks: &[u8]) -> [u8; 256] {
        let mut h = [0u8; 256];
        for x in 0..16 {
            for z in 0..16 {
                let mut y = H;
                while y > 0 && T::opacity(blocks[cell(x, y - 1, z)]) == 0 {
                    y -= 1;
                }
                h[z << 4 | x] = y as u8;
            }
        }
        h
    }

    /// Random terrain: stone to a random height, a cave or two, leaves, water, glowstone and torches.
    fn terrain(r: &mut Rng) -> Vec<u8> {
        let mut b = vec![0u8; VOLUME];
        for x in 0..16 {
            for z in 0..16 {
                let top = 30 + r.next(30) as usize;
                for y in 0..top {
                    b[cell(x, y, z)] = 1;
                }
                if r.next(6) == 0 {
                    b[cell(x, top, z)] = 3;
                    b[cell(x, top + 1, z)] = 3;
                }
                if r.next(10) == 0 {
                    b[cell(x, top + 3, z)] = 2;
                }
            }
        }
        for _ in 0..6 {
            let (cx, cy, cz) = (r.next(16) as usize, 10 + r.next(30) as usize, r.next(16) as usize);
            for _ in 0..40 {
                let (x, y, z) = ((cx + r.next(5) as usize).min(15), cy + r.next(3) as usize, (cz + r.next(5) as usize).min(15));
                b[cell(x, y, z)] = 0;
            }
        }
        for _ in 0..8 {
            b[cell(r.next(16) as usize, 5 + r.next(60) as usize, r.next(16) as usize)] = [4, 5][r.next(2) as usize];
        }
        b
    }

    /// The test world: chunks (0..GX, 0..GZ).
    const GX: i32 = 7;
    const GZ: i32 = 2;

    fn slot(k: Key) -> Option<usize> {
        ((0..GX).contains(&k.0) && (0..GZ).contains(&k.1)).then(|| (k.0 * GZ + k.1) as usize)
    }

    /// The settled light of the lit chunks by brute force: start every cell at its own source level and flood outwards until no
    /// cell rises (chunks that are not lit are absent). No decreases, no flags: nothing in common with the engine but the rule.
    fn oracle(chunks: &HashMap<Key, Chunk>) -> HashMap<Key, Vec<u8>> {
        let mut out: Vec<Option<Vec<u8>>> = (0..GX * GZ).map(|_| None).collect();
        let mut blocks: Vec<&[u8]> = vec![&[]; (GX * GZ) as usize];
        let mut work: Vec<(usize, usize)> = Vec::new();
        for (k, c) in chunks.iter().filter(|(_, c)| c.lit) {
            let (s, mut l) = (slot(*k).unwrap(), vec![0u8; VOLUME]);
            for x in 0..16 {
                for z in 0..16 {
                    for y in 0..H {
                        let i = cell(x, y, z);
                        l[i] = if y as u8 >= c.height[z << 4 | x] { 15 } else { 0 } | T::emission(c.blocks[i]) << 4;
                        if l[i] != 0 {
                            work.push((s, i));
                        }
                    }
                }
            }
            (out[s], blocks[s]) = (Some(l), &c.blocks);
        }
        while let Some((s, i)) = work.pop() {
            let v = out[s].as_ref().unwrap()[i];
            let (x, y, z) = ((i >> 11) as i32, (i & 127) as i32, (i >> 7 & 15) as i32);
            for (dx, dy, dz) in DIRS {
                let (nx, ny, nz) = (x + dx, y + dy, z + dz);
                let k = ((s as i32) / GZ + (nx >> 4), (s as i32) % GZ + (nz >> 4));
                let Some(ns) = slot(k).filter(|&t| out[t].is_some() && (0..H as i32).contains(&ny)) else { continue };
                let ni = cell((nx & 15) as usize, ny as usize, (nz & 15) as usize);
                let op = (T::opacity(blocks[ns][ni]) as i32).max(1);
                let n = out[ns].as_mut().unwrap();
                let (sky, blk) = (((v & 15) as i32 - op).max((n[ni] & 15) as i32), ((v >> 4) as i32 - op).max((n[ni] >> 4) as i32));
                let new = sky as u8 | (blk as u8) << 4;
                if new != n[ni] {
                    n[ni] = new;
                    work.push((ns, ni));
                }
            }
        }
        out.into_iter().enumerate().filter_map(|(s, l)| l.map(|l| (((s as i32) / GZ, (s as i32) % GZ), l))).collect()
    }

    fn assert_settled(chunks: &HashMap<Key, Chunk>, what: &str) {
        for (k, want) in oracle(chunks) {
            let got = &chunks[&k].light;
            if let Some(i) = (0..VOLUME).find(|&i| got[i] != want[i]) {
                panic!("{what}: chunk {k:?} cell ({}, {}, {}): sky/block {}/{} want {}/{}", i >> 11, i & 127, i >> 7 & 15, got[i] & 15, got[i] >> 4, want[i] & 15, want[i] >> 4);
            }
        }
    }

    /// Replace the blocks of a chunk (unlit until its next `init`).
    fn fill(chunks: &mut HashMap<Key, Chunk>, k: Key, b: Vec<u8>) {
        let height = heights(&b);
        match chunks.get_mut(&k) {
            Some(c) => (c.blocks, c.height, c.lit) = (Arc::new(b), height, false),
            None => {
                chunks.insert(k, Chunk { blocks: Arc::new(b), light: Arc::new(vec![0; VOLUME]), height, lit: false, touched: false, stale: false });
            }
        }
    }

    /// The host's side of an edit: block and height at once, light queued in the task of a lit chunk.
    fn edit(chunks: &mut HashMap<Key, Chunk>, tasks: &mut HashMap<Key, Task>, x: i32, y: i32, z: i32, id: u8) {
        let k = (x >> 4, z >> 4);
        let c = chunks.get_mut(&k).unwrap();
        Arc::make_mut(&mut c.blocks)[cell((x & 15) as usize, y as usize, (z & 15) as usize)] = id;
        c.height = heights(&c.blocks);
        if c.lit {
            tasks.entry(k).or_default().edits.push([x, y, z]);
        }
    }

    /// Random edits, first lights and re-lights in random order, with 1 and 3 workers: after each batch the engine's light is the fixpoint.
    #[test]
    fn settled_light_is_the_fixpoint() {
        for (seed, workers) in [(1, 1), (2, 3), (3, 3)] {
            let mut r = Rng(seed);
            let mut chunks: HashMap<Key, Chunk> = HashMap::new();
            let mut tasks: HashMap<Key, Task> = HashMap::new();
            // 7 x 2 chunks: the two ends can be lit in the same wave.
            let keys: Vec<Key> = (0..GX).flat_map(|x| (0..GZ).map(move |z| (x, z))).collect();
            for &k in &keys {
                let b = terrain(&mut r);
                fill(&mut chunks, k, b);
            }
            let mut order = keys.clone();
            for i in (1..order.len()).rev() {
                order.swap(i, r.next(i as u32 + 1) as usize);
            }
            for (n, group) in order.chunks(5).enumerate() {
                for &k in group {
                    tasks.entry(k).or_default().init = true;
                }
                run::<T, _>(&mut chunks, &mut tasks, workers, None);
                assert_settled(&chunks, &format!("seed {seed} lit group {n}"));
                for _ in 0..12 {
                    let lit: Vec<Key> = keys.iter().copied().filter(|k| chunks[k].lit).collect();
                    let k = lit[r.next(lit.len() as u32) as usize];
                    let (x, y, z) = (k.0 * 16 + r.next(16) as i32, 20 + r.next(60) as i32, k.1 * 16 + r.next(16) as i32);
                    edit(&mut chunks, &mut tasks, x, y, z, [0, 1, 2, 3, 4, 5][r.next(6) as usize]);
                }
                run::<T, _>(&mut chunks, &mut tasks, workers, None);
                assert_settled(&chunks, &format!("seed {seed} edits after group {n}"));
            }
            // A lit chunk is rewritten (populate) and lit again beside lit neighbours.
            let k = (3, 1);
            let b = terrain(&mut r);
            fill(&mut chunks, k, b);
            tasks.remove(&k);
            tasks.entry(k).or_default().init = true;
            run::<T, _>(&mut chunks, &mut tasks, workers, None);
            assert_settled(&chunks, &format!("seed {seed} re-lit"));
        }
    }

    /// Edits of a chunk that is not lit are dropped, and its first light still sees the blocks as they are.
    #[test]
    fn unlit_chunk_ignores_edits_and_lights_from_its_blocks() {
        let mut r = Rng(9);
        let mut chunks: HashMap<Key, Chunk> = HashMap::new();
        let mut tasks: HashMap<Key, Task> = HashMap::new();
        for k in [(0, 0), (1, 0)] {
            let b = terrain(&mut r);
            fill(&mut chunks, k, b);
        }
        tasks.entry((0, 0)).or_default().init = true;
        run::<T, _>(&mut chunks, &mut tasks, 1, None);
        edit(&mut chunks, &mut tasks, 20, 40, 3, 4); // chunk (1, 0) is not lit: no task
        assert!(tasks.is_empty());
        tasks.entry((1, 0)).or_default().init = true;
        run::<T, _>(&mut chunks, &mut tasks, 1, None);
        assert_settled(&chunks, "late first light");
        assert_eq!(chunks[&(1, 0)].light[cell(4, 40, 3)] >> 4, 15, "the glowstone placed before the first light");
    }
}
