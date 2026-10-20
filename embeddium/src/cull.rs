//! `OcclusionCuller`: a breadth-first walk outward from the camera's section. A section is only entered through a
//! neighbour when the section before it connects the face it came in by to the face it leaves by (`vis`), so caves and
//! hills hide what is behind them. Frustum and distance are the host's call (`visible`), like Embeddium's viewport.
//!
//! One walk is O(visible sections); the scratch buffers live in the `Culler`, so a steady-state frame allocates nothing.
//! Sections are `[x, y, z]` in section units (16 blocks).

use crate::vis::{self, ALL_DIRS, OPPOSITE, STEP};
use std::collections::{HashMap, VecDeque};

/// The host's section store.
pub trait Graph {
    /// The connectivity (`vis`) of a loaded section, `None` when there is none there (not loaded, or outside the world's
    /// height): the walk does not go through it.
    fn vis(&self, section: [i32; 3]) -> Option<u64>;
}

#[derive(Default)]
pub struct Culler {
    incoming: HashMap<[i32; 3], u8>,
    queue: VecDeque<[i32; 3]>,
    out: Vec<[i32; 3]>,
}

impl Culler {
    /// The sections to draw, nearest first (breadth-first order: reverse it for back-to-front blending).
    /// `occlude: false` walks through everything the frustum and distance allow (camera outside the world's height, where
    /// `origin`'s connectivity says nothing about the view). `None`: the origin section is not loaded, so there is no
    /// place to start; the host then falls back to testing every section.
    ///
    /// ponytail: Embeddium seeds a whole layer when the camera is above or below the world (a diamond spiral); this starts
    /// from the one clamped section, which reaches the same sections while the frustum is convex.
    pub fn find_visible(&mut self, graph: &impl Graph, origin: [i32; 3], occlude: bool, visible: impl Fn([i32; 3]) -> bool) -> Option<&[[i32; 3]]> {
        self.incoming.clear();
        self.queue.clear();
        self.out.clear();

        let first = graph.vis(origin)?;
        self.incoming.insert(origin, 0);
        self.out.push(origin); // the camera's own section is always drawn
        let outgoing = if occlude { vis::connections_any(first) } else { ALL_DIRS };
        self.enter_neighbors(graph, origin, outgoing);

        while let Some(s) = self.queue.pop_front() {
            if !visible(s) {
                continue;
            }
            self.out.push(s);
            let mut outgoing = if occlude { vis::connections(graph.vis(s).unwrap_or(vis::FULL), self.incoming[&s]) } else { ALL_DIRS };
            // Only ever walk away from the camera: no path doubles back, so every section is reached from the previous layer only.
            outgoing &= outward(origin, s);
            self.enter_neighbors(graph, s, outgoing);
        }
        Some(self.out.as_slice())
    }

    fn enter_neighbors(&mut self, graph: &impl Graph, from: [i32; 3], outgoing: u8) {
        for d in 0..6 {
            if outgoing >> d & 1 == 0 {
                continue;
            }
            let n = [from[0] + STEP[d][0], from[1] + STEP[d][1], from[2] + STEP[d][2]];
            if graph.vis(n).is_none() {
                continue;
            }
            let new = !self.incoming.contains_key(&n);
            *self.incoming.entry(n).or_insert(0) |= 1 << OPPOSITE[d];
            if new {
                self.queue.push_back(n);
            }
        }
    }
}

/// The directions that lead away from `origin` (`getOutwardDirections`).
fn outward(origin: [i32; 3], s: [i32; 3]) -> u8 {
    let mut m = 0u8;
    if s[0] <= origin[0] { m |= 1 << vis::WEST }
    if s[0] >= origin[0] { m |= 1 << vis::EAST }
    if s[1] <= origin[1] { m |= 1 << vis::DOWN }
    if s[1] >= origin[1] { m |= 1 << vis::UP }
    if s[2] <= origin[2] { m |= 1 << vis::NORTH }
    if s[2] >= origin[2] { m |= 1 << vis::SOUTH }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    struct World(HashMap<[i32; 3], u64>);
    impl Graph for World {
        fn vis(&self, s: [i32; 3]) -> Option<u64> {
            self.0.get(&s).copied()
        }
    }

    fn line(vis_at: &[(i32, u64)]) -> World {
        let mut w: HashMap<[i32; 3], u64> = (0..5).map(|x| ([x, 0, 0], vis::FULL)).collect();
        for &(x, v) in vis_at {
            w.insert([x, 0, 0], v);
        }
        World(w)
    }

    fn walk(w: &World, occlude: bool, visible: impl Fn([i32; 3]) -> bool) -> Vec<[i32; 3]> {
        Culler::default().find_visible(w, [0, 0, 0], occlude, visible).unwrap().to_vec()
    }

    #[test]
    fn open_line_is_all_visible_nearest_first() {
        let got = walk(&line(&[]), true, |_| true);
        assert_eq!(got, (0..5).map(|x| [x, 0, 0]).collect::<Vec<_>>());
    }

    /// A solid section is drawn (its face is what you see) but nothing behind it; `occlude: false` ignores that.
    #[test]
    fn solid_section_hides_what_is_behind() {
        let w = line(&[(2, 0)]);
        assert_eq!(walk(&w, true, |_| true).len(), 3);
        assert_eq!(walk(&w, false, |_| true).len(), 5);
    }

    #[test]
    fn frustum_stops_the_walk() {
        assert_eq!(walk(&line(&[]), true, |s| s[0] != 3).len(), 3);
    }

    /// A west-east tunnel passes the view along x but not up into the section above it.
    #[test]
    fn tunnel_does_not_leak_sideways() {
        let mut w = line(&[(1, vis::bit(vis::WEST, vis::EAST) | vis::bit(vis::EAST, vis::WEST) | vis::bit(vis::WEST, vis::WEST) | vis::bit(vis::EAST, vis::EAST))]);
        w.0.insert([1, 1, 0], vis::FULL);
        let got = walk(&w, true, |_| true);
        assert!(got.contains(&[2, 0, 0]));
        assert!(!got.contains(&[1, 1, 0]));
    }

    #[test]
    fn missing_origin_is_none() {
        assert!(Culler::default().find_visible(&World(HashMap::new()), [0, 0, 0], true, |_| true).is_none());
    }
}
