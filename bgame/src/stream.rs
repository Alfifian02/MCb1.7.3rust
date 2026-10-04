//! Chunk streaming: load/unload around the player, generation and meshing on worker threads.
//! The render thread only receives finished meshes (at most a few per frame) and uploads them.
use crate::world::{SecKey, World};
use bcore::chunk::{Chunk, H};
use bcore::mesh::{self, Mesh};
use bcore::worldgen;
use std::collections::{HashMap, HashSet};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

pub const RADII: [i32; 5] = [3, 4, 5, 6, 8];
const DEFAULT_RADIUS_IDX: usize = 1;
const FRAME_BUDGET: u32 = 8; // gen = 1 point, mesh = 3 points

type Cell = (i32, i32);
pub type Sections = Vec<(usize, Mesh)>;

enum Job {
    Gen { seed: u32, cx: i32, cz: i32 },
    Mesh { cx: i32, cz: i32, ver: u32, chunk: Arc<Chunk>, nb: [Arc<Chunk>; 4] }, // -x, +x, -z, +z
}

enum Done {
    Gen { cx: i32, cz: i32, chunk: Chunk },
    Mesh { cx: i32, cz: i32, ver: u32, sections: Sections },
}

struct Entry {
    ver: u32,
    meshed_ver: Option<u32>,
    in_flight: bool,
    edited: bool,
}

#[derive(Default)]
pub struct Output {
    pub meshed: Vec<(i32, i32, Sections)>,
    pub unloaded: Vec<Cell>,
}

pub struct Streamer {
    seed: u32,
    radius_idx: usize,
    jobs: mpsc::Sender<Job>,
    done: mpsc::Receiver<Done>,
    entries: HashMap<Cell, Entry>,
    gen_pending: HashSet<Cell>,
    persist: HashMap<Cell, Arc<Chunk>>, // edited chunks that scrolled out of range (until saving exists)
    inflight: usize,
    cap: usize,
    ver_counter: u32,
}

fn mesh_chunk(chunk: &Chunk, nb: &[Arc<Chunk>; 4]) -> Sections {
    let look = |nx: i32, ny: i32, nz: i32| -> u8 {
        if nx < 0 { nb[0].get(15, ny, nz) }
        else if nx >= 16 { nb[1].get(0, ny, nz) }
        else if nz < 0 { nb[2].get(nx, ny, 15) }
        else { nb[3].get(nx, ny, 0) }
    };
    (0..H / mesh::SECTION)
        .filter_map(|sy| {
            let m = mesh::build_section(chunk, sy, &look);
            if m.indices.is_empty() { None } else { Some((sy, m)) }
        })
        .collect()
}

fn d2(a: Cell, b: Cell) -> i32 {
    let (dx, dz) = (a.0 - b.0, a.1 - b.1);
    dx * dx + dz * dz
}

impl Streamer {
    pub fn new(seed: u32, workers: usize) -> Self {
        let (jobs, jrx) = mpsc::channel::<Job>();
        let jrx = Arc::new(Mutex::new(jrx));
        let (dtx, done) = mpsc::channel::<Done>();
        for i in 0..workers.max(1) {
            let (jrx, dtx) = (jrx.clone(), dtx.clone());
            let _ = thread::Builder::new().name(format!("bgame-worker-{i}")).spawn(move || loop {
                let job = { jrx.lock().unwrap().recv() };
                let res = match job {
                    Ok(Job::Gen { seed, cx, cz }) => Done::Gen { cx, cz, chunk: worldgen::generate(seed, cx, cz) },
                    Ok(Job::Mesh { cx, cz, ver, chunk, nb }) => Done::Mesh { cx, cz, ver, sections: mesh_chunk(&chunk, &nb) },
                    Err(_) => break,
                };
                if dtx.send(res).is_err() {
                    break;
                }
            });
        }
        Self {
            seed, radius_idx: DEFAULT_RADIUS_IDX, jobs, done,
            entries: HashMap::new(), gen_pending: HashSet::new(), persist: HashMap::new(),
            inflight: 0, cap: workers.max(1) * 2, ver_counter: 0,
        }
    }

    pub fn default_workers() -> usize {
        thread::available_parallelism().map_or(2, |n| (n.get() / 2).clamp(1, 2))
    }

    fn next_ver(&mut self) -> u32 {
        self.ver_counter = self.ver_counter.wrapping_add(1);
        self.ver_counter
    }

    /// Registers chunks that already exist in the world (the synchronous start-up area).
    pub fn adopt(&mut self, world: &World) {
        let keys: Vec<Cell> = world.chunks.keys().copied().collect();
        for k in keys {
            let ver = self.next_ver();
            self.entries.insert(k, Entry { ver, meshed_ver: None, in_flight: false, edited: false });
        }
    }

    pub fn radius(&self) -> i32 { RADII[self.radius_idx] }

    pub fn cycle_radius(&mut self) { self.radius_idx = (self.radius_idx + 1) % RADII.len(); }

    /// (start, end) distances in blocks for linear fog; hides the unloaded boundary.
    pub fn fog(&self) -> (f32, f32) {
        let end = (((self.radius() - 1) * 16) as f32).max(24.0);
        (end * 0.6, end)
    }

    /// GL resources were lost (Android pause): every chunk needs a fresh mesh.
    pub fn invalidate_meshes(&mut self) {
        for e in self.entries.values_mut() {
            e.meshed_ver = None;
        }
    }

    /// Call after the player edits blocks, before re-meshing the touched sections.
    pub fn on_edit(&mut self, dirty: &[SecKey]) {
        let cells: HashSet<Cell> = dirty.iter().map(|k| (k.0, k.1)).collect();
        for c in cells {
            let ver = self.next_ver();
            if let Some(e) = self.entries.get_mut(&c) {
                let was_current = e.meshed_ver == Some(e.ver);
                e.ver = ver;
                e.edited = true;
                // Edited sections are re-meshed synchronously by the caller, so the chunk stays "current"
                // unless a stale async job is in flight (its result will be discarded and redone).
                if was_current && !e.in_flight {
                    e.meshed_ver = Some(ver);
                }
            }
        }
    }

    pub fn update(&mut self, world: &mut World, center: Cell, accept_meshes: bool) -> Output {
        let r = self.radius();
        let mut out = Output::default();

        // 1. collect finished work (bounded so a frame never stalls on uploads)
        let mut budget = 0;
        while budget < FRAME_BUDGET {
            match self.done.try_recv() {
                Ok(Done::Gen { cx, cz, chunk }) => {
                    budget += 1;
                    self.inflight = self.inflight.saturating_sub(1);
                    self.gen_pending.remove(&(cx, cz));
                    if d2((cx, cz), center) <= (r + 2) * (r + 2) {
                        world.chunks.insert((cx, cz), Arc::new(chunk));
                        let ver = self.next_ver();
                        self.entries.insert((cx, cz), Entry { ver, meshed_ver: None, in_flight: false, edited: false });
                    }
                }
                Ok(Done::Mesh { cx, cz, ver, sections }) => {
                    budget += 3;
                    self.inflight = self.inflight.saturating_sub(1);
                    if let Some(e) = self.entries.get_mut(&(cx, cz)) {
                        e.in_flight = false;
                        if accept_meshes && e.ver == ver {
                            e.meshed_ver = Some(ver);
                            out.meshed.push((cx, cz, sections));
                        }
                    }
                }
                Err(_) => break,
            }
        }

        // 2. unload far chunks (edited ones are parked, not lost)
        let lim = (r + 2) * (r + 2);
        let far: Vec<Cell> = self.entries.keys().copied().filter(|&k| d2(k, center) > lim).collect();
        for k in far {
            let e = self.entries.remove(&k).unwrap();
            if let Some(ch) = world.chunks.remove(&k) {
                if e.edited {
                    self.persist.insert(k, ch);
                }
            }
            out.unloaded.push(k);
        }

        // 3. request new work, nearest first
        if self.inflight < self.cap {
            let g = r + 1;
            let mut reqs: Vec<(i32, Cell, bool)> = Vec::new();
            for dz in -g..=g {
                for dx in -g..=g {
                    let dd = dx * dx + dz * dz;
                    if dd > g * g {
                        continue;
                    }
                    let k = (center.0 + dx, center.1 + dz);
                    if !world.chunks.contains_key(&k) {
                        if !self.gen_pending.contains(&k) {
                            reqs.push((dd, k, false));
                        }
                    } else if dd <= r * r {
                        if let Some(e) = self.entries.get(&k) {
                            if !e.in_flight && e.meshed_ver != Some(e.ver) && Self::neighbors_ready(world, k) {
                                reqs.push((dd, k, true));
                            }
                        }
                    }
                }
            }
            reqs.sort_unstable_by_key(|q| q.0);
            for (_, k, is_mesh) in reqs {
                if is_mesh {
                    if self.inflight >= self.cap {
                        break;
                    }
                    let chunk = world.chunks[&k].clone();
                    let get = |dx: i32, dz: i32| world.chunks[&(k.0 + dx, k.1 + dz)].clone();
                    let nb = [get(-1, 0), get(1, 0), get(0, -1), get(0, 1)];
                    let ver = self.entries[&k].ver;
                    if self.jobs.send(Job::Mesh { cx: k.0, cz: k.1, ver, chunk, nb }).is_ok() {
                        self.entries.get_mut(&k).unwrap().in_flight = true;
                        self.inflight += 1;
                    }
                } else if let Some(ch) = self.persist.remove(&k) {
                    world.chunks.insert(k, ch);
                    let ver = self.next_ver();
                    self.entries.insert(k, Entry { ver, meshed_ver: None, in_flight: false, edited: true });
                } else {
                    if self.inflight >= self.cap {
                        continue;
                    }
                    if self.jobs.send(Job::Gen { seed: self.seed, cx: k.0, cz: k.1 }).is_ok() {
                        self.gen_pending.insert(k);
                        self.inflight += 1;
                    }
                }
            }
        }
        out
    }

    fn neighbors_ready(world: &World, k: Cell) -> bool {
        [(-1, 0), (1, 0), (0, -1), (0, 1)].iter().all(|d| world.chunks.contains_key(&(k.0 + d.0, k.1 + d.1)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bcore::block::STONE;
    use std::time::{Duration, Instant};

    fn within(center: Cell, r: i32) -> HashSet<Cell> {
        let mut v = HashSet::new();
        for dx in -r..=r {
            for dz in -r..=r {
                if dx * dx + dz * dz <= r * r {
                    v.insert((center.0 + dx, center.1 + dz));
                }
            }
        }
        v
    }

    /// Pumps the streamer until every chunk within the view radius has been meshed.
    fn load_around(s: &mut Streamer, w: &mut World, c: Cell) {
        let want = within(c, s.radius());
        let mut meshed = HashSet::new();
        let t = Instant::now();
        while !want.is_subset(&meshed) {
            assert!(t.elapsed() < Duration::from_secs(20), "timeout loading");
            for m in s.update(w, c, true).meshed {
                meshed.insert((m.0, m.1));
            }
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn move_to(s: &mut Streamer, w: &mut World, c: Cell, until: impl Fn(&World) -> bool) {
        let t = Instant::now();
        while !until(w) {
            assert!(t.elapsed() < Duration::from_secs(20), "timeout moving");
            s.update(w, c, true);
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn loads_meshes_then_unloads() {
        let mut w = World::empty(7);
        let mut s = Streamer::new(7, 2);
        load_around(&mut s, &mut w, (0, 0));
        assert!(w.chunks.len() >= within((0, 0), s.radius()).len());
        move_to(&mut s, &mut w, (40, 0), |w| !w.chunks.contains_key(&(0, 0)));
    }

    #[test]
    fn edits_survive_unload_and_reload() {
        let mut w = World::empty(7);
        let mut s = Streamer::new(7, 2);
        load_around(&mut s, &mut w, (0, 0));
        let d = w.set_block(3, 120, 3, STONE);
        s.on_edit(&d);
        move_to(&mut s, &mut w, (40, 0), |w| !w.chunks.contains_key(&(0, 0)));
        move_to(&mut s, &mut w, (0, 0), |w| w.chunks.contains_key(&(0, 0)));
        assert_eq!(w.block(3, 120, 3), STONE);
    }

    #[test]
    fn adopted_start_area_gets_meshed_without_regeneration() {
        let mut w = World::new(7, 2);
        let mut s = Streamer::new(7, 2);
        s.adopt(&w);
        let before = w.chunks[&(0, 0)].clone();
        load_around(&mut s, &mut w, (0, 0));
        assert!(Arc::ptr_eq(&before, &w.chunks[&(0, 0)]));
    }

    #[test]
    fn fog_scales_with_radius() {
        let mut s = Streamer::new(1, 1);
        let a = s.fog().1;
        s.cycle_radius();
        assert!(s.fog().1 > a);
    }
}
