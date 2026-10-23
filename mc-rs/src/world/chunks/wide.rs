//! The occlusion walk out of the render loop. Sodium keeps a "wide" tree (`CullType.WIDE`: the walk without the frustum) and rebuilds it
//! only when the camera enters another section or a section's occlusion data changes; each frame only tests that tree against the
//! frustum. It rebuilds on a worker (`AsyncCameraTimingControl`), and until the new tree is there the old one keeps being drawn from; only a
//! camera that jumps (a teleport, a new render distance) makes the frame walk itself.
//!
//! So turning the camera, the common case, costs a frustum test per visible column instead of a breadth-first walk over thousands of
//! sections. At 16 chunks that walk was the largest piece of the render thread's CPU time.

use std::collections::HashMap;
use std::sync::{mpsc, Arc};

use embeddium::cull::{Culler, Graph};
use glam::Vec3;

use super::{in_range, Key, KeyHasher, Shape, SECTIONS};

/// Per loaded chunk, the `vis` of each section (a chunk without a mesh lets the view through: `vis::FULL`).
pub(super) type VisMap = HashMap<Key, [u64; SECTIONS], std::hash::BuildHasherDefault<KeyHasher>>;

/// `VisMap` as the render thread edits it and the worker reads it: the worker holds an `Arc` while it walks, so an edit during a walk
/// copies the map once (about 70 KB at 16 chunks) instead of waiting for it. `gen` changes with every edit.
#[derive(Default)]
pub(super) struct Snapshot {
    map: Arc<VisMap>,
    pub gen: u32,
}

impl Snapshot {
    /// `Some(v)`: chunk `key` is loaded with these section `vis`; `None`: it is gone.
    pub fn set(&mut self, key: Key, v: Option<[u64; SECTIONS]>) {
        let map = Arc::make_mut(&mut self.map);
        match v {
            Some(v) => map.insert(key, v),
            None => map.remove(&key),
        };
        self.gen = self.gen.wrapping_add(1);
    }
}

struct Sections<'a>(&'a VisMap);

impl Graph for Sections<'_> {
    fn vis(&self, [x, y, z]: [i32; 3]) -> Option<u64> {
        if !(0..SECTIONS as i32).contains(&y) {
            return None;
        }
        self.0.get(&(x, z)).map(|v| v[y as usize])
    }
}

/// What a walk depends on besides the loaded chunks.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) struct WideKey {
    /// The camera's section (clamped to the world's height), and whether the eye really is inside that height.
    pub origin: [i32; 3],
    pub inside: bool,
    pub shape: Shape,
    pub rh: f32,
    pub rv: f32,
}

impl WideKey {
    /// A walk made for `self` can stand in for `o` for a frame or two: same ranges and shape, camera at most a section away.
    fn serves(&self, o: &WideKey) -> bool {
        self.inside == o.inside && self.shape == o.shape && self.rh == o.rh && self.rv == o.rv && (0..3).all(|i| (self.origin[i] - o.origin[i]).abs() <= 1)
    }
}

/// One finished walk: per chunk column, the sections (bit `y`) reached, in the order the walk found the columns (near to far).
pub(super) struct Wide {
    pub key: WideKey,
    pub gen: u32,
    pub cols: Vec<(Key, u8)>,
}

/// The walk from the camera's section, in range but not tested against the frustum. The eye is taken as the middle of its section and
/// the ranges grow by the most that can move it (half a section's diagonal), so the result holds every section any eye position inside
/// that section would reach; the per-frame test with the real eye (`ChunkManager::update_visible`) cuts it down again.
/// With the camera in a chunk that is not loaded there is nowhere to start: every loaded section in range is returned.
fn walk(map: &VisMap, culler: &mut Culler, k: &WideKey) -> Vec<(Key, u8)> {
    let [ox, oy, oz] = k.origin;
    let eye = Vec3::new(ox as f32 * 16.0 + 8.0, oy as f32 * 16.0 + 8.0, oz as f32 * 16.0 + 8.0);
    let (rh, rv) = (k.rh + 14.0, k.rv + 8.0);
    let ok = |[x, y, z]: [i32; 3]| {
        let min = Vec3::new(x as f32 * 16.0, y as f32 * 16.0 - 1.0, z as f32 * 16.0);
        in_range(k.shape, eye, min, min + Vec3::new(16.0, 18.0, 16.0), rh, rv)
    };
    let mut cols: Vec<(Key, u8)> = Vec::new();
    let mut at: HashMap<Key, usize, std::hash::BuildHasherDefault<KeyHasher>> = HashMap::default();
    let mut add = |x: i32, y: i32, z: i32| {
        let i = *at.entry((x, z)).or_insert_with(|| {
            cols.push(((x, z), 0));
            cols.len() - 1
        });
        cols[i].1 |= 1u8 << y;
    };
    match culler.find_visible(&Sections(map), k.origin, k.inside, &ok) {
        Some(found) => found.iter().for_each(|&[x, y, z]| add(x, y, z)),
        None => {
            for &(x, z) in map.keys() {
                for y in 0..SECTIONS as i32 {
                    if ok([x, y, z]) {
                        add(x, y, z);
                    }
                }
            }
        }
    }
    cols
}

struct Job {
    map: Arc<VisMap>,
    key: WideKey,
    gen: u32,
}

/// The current `Wide` and the worker that refreshes it.
pub(super) struct WideCull {
    jobs: mpsc::Sender<Job>,
    done: mpsc::Receiver<Wide>,
    /// A job is out (one at a time: the answer to an older one is only ever worth less than the next).
    busy: bool,
    cur: Option<Wide>,
    /// For the walks the frame makes itself.
    culler: Culler,
}

impl WideCull {
    /// ponytail: the worker keeps the default priority (the generation workers are niced): the frame waits for its answer, a frame or
    /// two at most, and it only runs when the camera or the loaded chunks changed.
    pub fn spawn() -> Self {
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (done_tx, done) = mpsc::channel();
        std::thread::Builder::new()
            .name("chunk-cull".into())
            .spawn(move || {
                let mut culler = Culler::default();
                // Ends when the manager is dropped (the job channel closes).
                while let Ok(j) = job_rx.recv() {
                    let cols = walk(&j.map, &mut culler, &j.key);
                    if done_tx.send(Wide { key: j.key, gen: j.gen, cols }).is_err() {
                        break;
                    }
                }
            })
            .expect("spawn chunk-cull thread");
        Self { jobs, done, busy: false, cur: None, culler: Culler::default() }
    }

    /// The walk to test against the frustum this frame. Takes a finished background walk; starts one when the loaded chunks or the camera's
    /// section changed since the one in use; walks right here when there is none yet or the camera left the one in use by more than a
    /// section.
    pub fn get(&mut self, graph: &Snapshot, key: WideKey) -> &Wide {
        let mut dead = false;
        loop {
            match self.done.try_recv() {
                Ok(w) => {
                    self.busy = false;
                    if w.key.serves(&key) {
                        self.cur = Some(w); // an answer for a camera that has moved on since is dropped
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    dead = true; // the worker died: walk here rather than draw a frozen view
                    break;
                }
            }
        }
        let fresh = |c: &Option<Wide>| c.as_ref().is_some_and(|w| w.key == key && w.gen == graph.gen);
        let usable = self.cur.as_ref().is_some_and(|w| w.key.serves(&key));
        let mut here = !usable || (dead && !fresh(&self.cur));
        if !here && !fresh(&self.cur) && !self.busy {
            self.busy = true;
            here = self.jobs.send(Job { map: Arc::clone(&graph.map), key, gen: graph.gen }).is_err();
        }
        if here {
            self.cur = Some(Wide { key, gen: graph.gen, cols: walk(&graph.map, &mut self.culler, &key) });
        }
        self.cur.as_ref().expect("set above or usable")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use embeddium::vis;

    fn key(origin: [i32; 3]) -> WideKey {
        WideKey { origin, inside: true, shape: Shape::Cylinder, rh: 1000.0, rv: 1000.0 }
    }

    /// Chunk columns (x, 0) for x in `xs`, every section open.
    fn strip(xs: std::ops::RangeInclusive<i32>) -> Snapshot {
        let mut s = Snapshot::default();
        xs.for_each(|x| s.set((x, 0), Some([vis::FULL; SECTIONS])));
        s
    }

    #[test]
    fn walk_reaches_every_open_section() {
        let s = strip(0..=3);
        let cols = walk(&s.map, &mut Culler::default(), &key([0, 0, 0]));
        let all = (0..SECTIONS).fold(0u8, |m, y| m | 1 << y);
        assert_eq!(cols.len(), 4);
        assert!(cols.iter().all(|c| c.1 == all));
    }

    /// A column of solid sections is reached but hides what is behind it (nothing passes in the `z` strip's single row).
    #[test]
    fn walk_stops_at_a_wall() {
        let mut s = strip(0..=3);
        s.set((2, 0), Some([0; SECTIONS]));
        let cols = walk(&s.map, &mut Culler::default(), &key([0, 0, 0]));
        assert!(cols.iter().any(|c| c.0 == (2, 0)));
        assert!(!cols.iter().any(|c| c.0 == (3, 0)));
    }

    #[test]
    fn walk_without_a_start_tests_every_section_in_range() {
        let s = strip(5..=6); // the camera's chunk (0, 0) is not loaded
        let near = WideKey { rh: 100.0, ..key([0, 0, 0]) }; // (5, 0) is 72 blocks from the eye, (6, 0) 88: both inside 100 + 14
        assert_eq!(walk(&s.map, &mut Culler::default(), &near).len(), 2);
        let short = WideKey { rh: 50.0, ..near };
        assert!(walk(&s.map, &mut Culler::default(), &short).is_empty());
    }

    #[test]
    fn first_walk_is_made_in_the_frame_and_a_change_is_refreshed_behind_it() {
        let mut w = WideCull::spawn();
        let mut s = strip(0..=1);
        assert_eq!(w.get(&s, key([0, 0, 0])).cols.len(), 2);
        s.set((2, 0), Some([vis::FULL; SECTIONS])); // same camera section, one more chunk
        let t = std::time::Instant::now();
        loop {
            let got = w.get(&s, key([0, 0, 0]));
            if got.gen == s.gen {
                assert_eq!(got.cols.len(), 3);
                break;
            }
            assert_eq!(got.cols.len(), 2, "the old walk stands until the worker answers");
            assert!(t.elapsed().as_secs() < 5, "the worker never answered");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    #[test]
    fn a_camera_that_jumps_is_walked_at_once() {
        let mut w = WideCull::spawn();
        let s = strip(0..=3);
        w.get(&s, key([0, 0, 0]));
        let got = w.get(&s, key([3, 0, 0]));
        assert_eq!(got.key.origin, [3, 0, 0]);
        assert_eq!(got.gen, s.gen);
    }
}
