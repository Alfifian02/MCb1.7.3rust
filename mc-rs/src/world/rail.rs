//! M22 rails: `RailLogic` (the shape a rail takes when it is placed and when a neighbour rail is joined to it) and the
//! `BlockRail.onNeighborBlockChange` rules (no ground drops it; a powered rail takes the power of the line it is part of).
//! Shapes (metadata bits 0-2, bit 3 = powered for rail 27 and activated for detector 28): 0 north-south, 1 east-west, 2..5 ascending
//! east / west / north / south, 6..9 curves (plain rail only: south-east, south-west, north-west, north-east).
//! Written against the small `Rails` trait so the logic runs on a toy grid in the tests and on the `ChunkManager` in the game.
//! ponytail: the detector rail never switches itself on yet (no minecarts); `Logic` keeps `RailLogic.connectedTracks` as plain cells.

use crate::world::chunk::normal_cube;
use crate::world::chunks::ChunkManager;
use crate::world::redstone;

pub type P = (i32, i32, i32);

pub trait Rails {
    fn id(&self, p: P) -> u8;
    fn meta(&self, p: P) -> u8;
    /// `setBlockMetadataWithNotify`.
    fn set_meta(&mut self, p: P, m: u8);
    /// `isBlockIndirectlyGettingPowered`.
    fn powered(&self, p: P) -> bool;
    /// `isBlockNormalCube`.
    fn normal(&self, p: P) -> bool;
    /// `notifyBlocksOfNeighborChange` at a cell.
    fn ping(&mut self, p: P);
}

impl Rails for ChunkManager {
    fn id(&self, p: P) -> u8 {
        self.block(p.0, p.1, p.2).unwrap_or(0)
    }
    fn meta(&self, p: P) -> u8 {
        ChunkManager::meta(self, p.0, p.1, p.2)
    }
    fn set_meta(&mut self, p: P, m: u8) {
        let id = self.id(p);
        self.set_block_meta(p.0, p.1, p.2, id, m);
    }
    fn powered(&self, p: P) -> bool {
        redstone::indirectly_powered(self, p.0, p.1, p.2, true)
    }
    fn normal(&self, p: P) -> bool {
        normal_cube(self.id(p))
    }
    fn ping(&mut self, p: P) {
        ChunkManager::ping(self, p.0, p.1, p.2);
    }
}

pub fn is_rail(id: u8) -> bool {
    matches!(id, 27 | 28 | 66)
}

fn rail_at<G: Rails>(g: &G, p: P) -> bool {
    is_rail(g.id(p))
}

/// `RailLogic`: one rail and the two cells it joins.
struct Logic {
    p: P,
    /// Rail 27 and detector 28 (`BlockRail.isPowered`): straight or ascending only.
    powered_rail: bool,
    conn: Vec<P>,
}

impl Logic {
    fn new<G: Rails>(g: &G, p: P) -> Self {
        let powered_rail = matches!(g.id(p), 27 | 28);
        let meta = if powered_rail { g.meta(p) & !8 } else { g.meta(p) };
        let mut l = Self { p, powered_rail, conn: Vec::new() };
        l.set_connections(meta);
        l
    }

    /// `getMinecartTrackLogic`: the rail at the cell, else the one above, else the one below.
    fn at<G: Rails>(g: &G, (x, y, z): P) -> Option<Self> {
        [y, y + 1, y - 1].into_iter().find(|&y| rail_at(g, (x, y, z))).map(|y| Self::new(g, (x, y, z)))
    }

    fn set_connections(&mut self, m: u8) {
        let (x, y, z) = self.p;
        self.conn = match m {
            0 => vec![(x, y, z - 1), (x, y, z + 1)],
            1 => vec![(x - 1, y, z), (x + 1, y, z)],
            2 => vec![(x - 1, y, z), (x + 1, y + 1, z)],
            3 => vec![(x - 1, y + 1, z), (x + 1, y, z)],
            4 => vec![(x, y + 1, z - 1), (x, y, z + 1)],
            5 => vec![(x, y, z - 1), (x, y + 1, z + 1)],
            6 => vec![(x + 1, y, z), (x, y, z + 1)],
            7 => vec![(x - 1, y, z), (x, y, z + 1)],
            8 => vec![(x - 1, y, z), (x, y, z - 1)],
            9 => vec![(x + 1, y, z), (x, y, z - 1)],
            _ => Vec::new(),
        };
    }

    /// `func_785_b`: keep only the connections whose rail points back here (by x and z), at the height it really is.
    fn refresh<G: Rails>(&mut self, g: &G) {
        let me = self.p;
        let kept: Vec<P> = self.conn.iter().filter_map(|&c| Self::at(g, c).filter(|l| l.is_connected_to(me)).map(|l| l.p)).collect();
        self.conn = kept;
    }

    fn is_connected_to(&self, o: P) -> bool {
        self.conn.iter().any(|c| c.0 == o.0 && c.2 == o.2)
    }

    fn in_track(&self, x: i32, z: i32) -> bool {
        self.conn.iter().any(|c| c.0 == x && c.2 == z)
    }

    /// `isMinecartTrack`: a rail in the cell, above it or below it.
    fn track_near<G: Rails>(g: &G, (x, y, z): P) -> bool {
        rail_at(g, (x, y, z)) || rail_at(g, (x, y + 1, z)) || rail_at(g, (x, y - 1, z))
    }

    fn adjacent<G: Rails>(&self, g: &G) -> usize {
        let (x, y, z) = self.p;
        [(x, y, z - 1), (x, y, z + 1), (x - 1, y, z), (x + 1, y, z)].into_iter().filter(|&c| Self::track_near(g, c)).count()
    }

    /// `handleKeyPress`: can `o` be joined to this rail (it is already, or there is room: fewer than two joins).
    fn can_connect(&self, o: P) -> bool {
        self.is_connected_to(o) || self.conn.len() != 2
    }

    /// `func_786_c`: the rail at the cell, refreshed, accepts a join from here.
    fn try_connect<G: Rails>(&self, g: &G, c: P) -> bool {
        Self::at(g, c).is_some_and(|mut l| {
            l.refresh(g);
            l.can_connect(self.p)
        })
    }

    fn slopes<G: Rails>(&self, g: &G, v: i32) -> i32 {
        let (x, y, z) = self.p;
        let mut v = v;
        if v == 0 {
            if rail_at(g, (x, y + 1, z - 1)) {
                v = 4;
            }
            if rail_at(g, (x, y + 1, z + 1)) {
                v = 5;
            }
        }
        if v == 1 {
            if rail_at(g, (x + 1, y + 1, z)) {
                v = 2;
            }
            if rail_at(g, (x - 1, y + 1, z)) {
                v = 3;
            }
        }
        v.max(0)
    }

    fn write<G: Rails>(&self, g: &mut G, v: i32) -> u8 {
        let m = if self.powered_rail { g.meta(self.p) & 8 | v as u8 } else { v as u8 };
        g.set_meta(self.p, m);
        m
    }

    /// `func_788_d`: a neighbour rail joined to this one; the shape follows the joins it now has.
    fn connect_to<G: Rails>(&mut self, g: &mut G, o: P) {
        self.conn.push(o);
        let (x, z) = (self.p.0, self.p.2);
        let (n, s, w, e) = (self.in_track(x, z - 1), self.in_track(x, z + 1), self.in_track(x - 1, z), self.in_track(x + 1, z));
        let mut v = -1;
        if n || s {
            v = 0;
        }
        if w || e {
            v = 1;
        }
        if !self.powered_rail {
            if s && e && !n && !w {
                v = 6;
            }
            if s && w && !n && !e {
                v = 7;
            }
            if n && w && !s && !e {
                v = 8;
            }
            if n && e && !s && !w {
                v = 9;
            }
        }
        let v = self.slopes(g, v);
        self.write(g, v);
    }

    /// `func_792_a`: a rail was placed (`force`) or a power source changed next to a rail with three neighbours.
    fn place<G: Rails>(&mut self, g: &mut G, powered: bool, force: bool) {
        let (x, y, z) = self.p;
        let (n, s, w, e) = (self.try_connect(g, (x, y, z - 1)), self.try_connect(g, (x, y, z + 1)), self.try_connect(g, (x - 1, y, z)), self.try_connect(g, (x + 1, y, z)));
        let mut v = -1;
        if (n || s) && !w && !e {
            v = 0;
        }
        if (w || e) && !n && !s {
            v = 1;
        }
        if !self.powered_rail {
            if s && e && !n && !w {
                v = 6;
            }
            if s && w && !n && !e {
                v = 7;
            }
            if n && w && !s && !e {
                v = 8;
            }
            if n && e && !s && !w {
                v = 9;
            }
        }
        if v == -1 {
            if n || s {
                v = 0;
            }
            if w || e {
                v = 1;
            }
            if !self.powered_rail {
                if powered {
                    if s && e {
                        v = 6;
                    }
                    if w && s {
                        v = 7;
                    }
                    if e && n {
                        v = 9;
                    }
                    if n && w {
                        v = 8;
                    }
                } else {
                    if n && w {
                        v = 8;
                    }
                    if e && n {
                        v = 9;
                    }
                    if w && s {
                        v = 7;
                    }
                    if s && e {
                        v = 6;
                    }
                }
            }
        }
        let v = self.slopes(g, v);
        self.set_connections(v as u8);
        let m = if self.powered_rail { g.meta(self.p) & 8 | v as u8 } else { v as u8 };
        if force || g.meta(self.p) != m {
            g.set_meta(self.p, m);
            for c in self.conn.clone() {
                if let Some(mut l) = Self::at(g, c) {
                    l.refresh(g);
                    if l.can_connect(self.p) {
                        l.connect_to(g, self.p);
                    }
                }
            }
        }
    }
}

/// `BlockRail.onBlockAdded`: the new rail picks its shape and bends its neighbours' (`powered` = `isBlockIndirectlyGettingPowered`).
pub fn added<G: Rails>(g: &mut G, p: P) {
    let powered = g.powered(p);
    Logic::new(g, p).place(g, powered, true);
}

/// `BlockRail.onNeighborBlockChange`. True when the rail has lost its ground (or the block its slope leans on) and must drop and
/// go; otherwise a powered rail follows the power of its line and a plain rail with three neighbours is re-laid when a power
/// source changed (`power`: the block that changed can provide power, or it was a ping).
pub fn on_neighbour<G: Rails>(g: &mut G, p: P, power: bool) -> bool {
    let (x, y, z) = p;
    let id = g.id(p);
    let meta = g.meta(p);
    let powered_type = matches!(id, 27 | 28);
    let m7 = if powered_type { meta & 7 } else { meta };
    let lost = !g.normal((x, y - 1, z))
        || (m7 == 2 && !g.normal((x + 1, y, z)))
        || (m7 == 3 && !g.normal((x - 1, y, z)))
        || (m7 == 4 && !g.normal((x, y, z - 1)))
        || (m7 == 5 && !g.normal((x, y, z + 1)));
    if lost {
        return true;
    }
    if id == 27 {
        let on = g.powered(p) || g.powered((x, y + 1, z)) || chain(g, p, meta, true, 0) || chain(g, p, meta, false, 0);
        let changed = if on && meta & 8 == 0 {
            g.set_meta(p, m7 | 8);
            true
        } else if !on && meta & 8 != 0 {
            g.set_meta(p, m7);
            true
        } else {
            false
        };
        if changed {
            g.ping((x, y - 1, z));
            if (2..=5).contains(&m7) {
                g.ping((x, y + 1, z));
            }
        }
    } else if id == 66 && power && Logic::new(g, p).adjacent(g) == 3 {
        let powered = g.powered(p);
        Logic::new(g, p).place(g, powered, false);
    }
    false
}

/// `BlockRail.func_27044_a`: does a powered rail further along the line (up to 8 rails away, one direction) carry power.
fn chain<G: Rails>(g: &G, (mut x, mut y, mut z): P, meta: u8, forward: bool, depth: u32) -> bool {
    if depth >= 8 {
        return false;
    }
    let mut m = (meta & 7) as i32;
    let mut below = true;
    match m {
        0 => z += if forward { 1 } else { -1 },
        1 => x += if forward { -1 } else { 1 },
        2 => {
            if forward {
                x -= 1;
            } else {
                x += 1;
                y += 1;
                below = false;
            }
            m = 1;
        }
        3 => {
            if forward {
                x -= 1;
                y += 1;
                below = false;
            } else {
                x += 1;
            }
            m = 1;
        }
        4 => {
            if forward {
                z += 1;
            } else {
                z -= 1;
                y += 1;
                below = false;
            }
            m = 0;
        }
        5 => {
            if forward {
                z += 1;
                y += 1;
                below = false;
            } else {
                z -= 1;
            }
            m = 0;
        }
        _ => {}
    }
    chain_at(g, (x, y, z), forward, depth, m) || (below && chain_at(g, (x, y - 1, z), forward, depth, m))
}

/// `BlockRail.func_27043_a`: the cell holds a powered rail on this axis that is on and has power of its own or further along.
fn chain_at<G: Rails>(g: &G, p: P, forward: bool, depth: u32, axis: i32) -> bool {
    if g.id(p) != 27 {
        return false;
    }
    let meta = g.meta(p);
    let shape = (meta & 7) as i32;
    if (axis == 1 && matches!(shape, 0 | 4 | 5)) || (axis == 0 && matches!(shape, 1 | 2 | 3)) {
        return false;
    }
    if meta & 8 == 0 {
        return false;
    }
    g.powered(p) || g.powered((p.0, p.1 + 1, p.2)) || chain(g, p, meta, forward, depth + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    #[derive(Default)]
    struct Grid {
        b: HashMap<P, (u8, u8)>,
        src: HashSet<P>,
        pinged: Vec<P>,
    }

    impl Rails for Grid {
        fn id(&self, p: P) -> u8 {
            self.b.get(&p).map_or(0, |b| b.0)
        }
        fn meta(&self, p: P) -> u8 {
            self.b.get(&p).map_or(0, |b| b.1)
        }
        fn set_meta(&mut self, p: P, m: u8) {
            if let Some(b) = self.b.get_mut(&p) {
                b.1 = m;
            }
        }
        fn powered(&self, p: P) -> bool {
            self.src.contains(&p)
        }
        fn normal(&self, p: P) -> bool {
            self.id(p) == 1
        }
        fn ping(&mut self, p: P) {
            self.pinged.push(p);
        }
    }

    /// A floor of stone at y = 9 and a block of stone at (7, 10, 5) is not placed; rails go where the test puts them.
    fn grid() -> Grid {
        let mut g = Grid::default();
        for x in 0..16 {
            for z in 0..16 {
                g.b.insert((x, 9, z), (1, 0));
            }
        }
        g
    }

    fn lay(g: &mut Grid, p: P, id: u8) {
        g.b.insert(p, (id, 0));
        added(g, p);
    }

    #[test]
    fn a_lone_rail_runs_north_south_and_one_beside_it_turns_both() {
        let mut g = grid();
        lay(&mut g, (5, 10, 5), 66);
        assert_eq!(g.meta((5, 10, 5)), 0, "alone: north-south");
        lay(&mut g, (5, 10, 6), 66);
        assert_eq!((g.meta((5, 10, 5)), g.meta((5, 10, 6))), (0, 0), "a line");
        lay(&mut g, (6, 10, 6), 66);
        assert_eq!(g.meta((6, 10, 6)), 1, "the new one lies east-west");
        assert_eq!(g.meta((5, 10, 6)), 9, "the end of the line bends north-east to meet it");
    }

    #[test]
    fn a_rail_beside_a_higher_one_ascends_and_a_powered_rail_never_bends() {
        let mut g = grid();
        g.b.insert((6, 10, 5), (1, 0)); // a step of stone, with a rail on top
        lay(&mut g, (6, 11, 5), 66);
        lay(&mut g, (5, 10, 5), 66);
        assert_eq!(g.meta((5, 10, 5)), 2, "ascending east");
        let mut g = grid();
        lay(&mut g, (5, 10, 4), 66);
        lay(&mut g, (6, 10, 5), 66);
        lay(&mut g, (5, 10, 5), 27);
        assert_eq!(g.meta((5, 10, 5)) & 7, 1, "a powered rail takes a straight shape even with a rail north and one east");
    }

    #[test]
    fn a_rail_without_ground_or_slope_support_drops() {
        let mut g = grid();
        lay(&mut g, (5, 10, 5), 66);
        assert!(!on_neighbour(&mut g, (5, 10, 5), false));
        g.b.insert((5, 9, 5), (0, 0));
        assert!(on_neighbour(&mut g, (5, 10, 5), false), "no ground");
        let mut g = grid();
        g.b.insert((5, 10, 5), (66, 2)); // ascending east, nothing at x = 6
        assert!(on_neighbour(&mut g, (5, 10, 5), false), "the slope leans on nothing");
        g.b.insert((6, 10, 5), (1, 0));
        assert!(!on_neighbour(&mut g, (5, 10, 5), false));
    }

    #[test]
    fn power_runs_down_a_line_of_powered_rails_and_stops_with_its_source() {
        let mut g = grid();
        for x in 5..=7 {
            g.b.insert((x, 10, 5), (27, 1));
        }
        g.src.insert((7, 10, 5));
        for x in [7, 6, 5] {
            assert!(!on_neighbour(&mut g, (x, 10, 5), true));
        }
        assert_eq!([5, 6, 7].map(|x| g.meta((x, 10, 5))), [9, 9, 9], "all three are on");
        assert!(g.pinged.contains(&(7, 9, 5)), "the block under a changed rail is told");
        g.src.clear();
        for x in [7, 6, 5] {
            on_neighbour(&mut g, (x, 10, 5), true);
        }
        assert_eq!([5, 6, 7].map(|x| g.meta((x, 10, 5))), [1, 1, 1], "all three are off again");
    }
}
