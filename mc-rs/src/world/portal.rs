//! Nether portals: `BlockPortal.tryToCreatePortal` / `onNeighborBlockChange` and the `Teleporter` (find the nearest portal, or make
//! one), on a small `Grid` trait (implemented for `ChunkManager`, and for a toy map in the tests, as `rail::Rails`).
//!
//! ponytail: `find` looks at the portals in the loaded chunks only (vanilla scans 257 x 257 columns and generates what is missing);
//! the arrival chunks are preloaded, so a portal built near the target is found. The player stands on the frame's floor (the Java's
//! `posY` includes the eye height, which would sink the feet 1.12 below the portal).

use crate::world::chunk::collision;
use crate::world::chunks::ChunkManager;
use crate::world::gen::noise::JavaRandom;

pub const OBSIDIAN: u8 = 49;
pub const PORTAL: u8 = 90;
const FIRE: u8 = 51;
type P = (i32, i32, i32);

pub trait Grid {
    fn id(&self, p: P) -> u8;
    /// A write that tells nobody (`editingBlocks`); `notify` pings the cell afterwards.
    fn put(&mut self, p: P, id: u8);
    fn notify(&mut self, p: P);
}

impl Grid for ChunkManager {
    fn id(&self, p: P) -> u8 {
        self.block(p.0, p.1, p.2).unwrap_or(0)
    }
    fn put(&mut self, p: P, id: u8) {
        self.set_quiet(p.0, p.1, p.2, id, 0);
    }
    fn notify(&mut self, p: P) {
        self.ping(p.0, p.1, p.2);
    }
}

fn air(g: &impl Grid, p: P) -> bool {
    g.id(p) == 0
}

fn solid(g: &impl Grid, p: P) -> bool {
    collision(g.id(p)).is_some()
}

/// `BlockPortal.tryToCreatePortal`: a fire in a 4 x 5 obsidian frame (corners free) becomes a 2 x 3 portal. `(x, y, z)` is the fire.
pub fn try_create(g: &mut impl Grid, (x, y, z): P) -> bool {
    let a = (g.id((x - 1, y, z)) == OBSIDIAN || g.id((x + 1, y, z)) == OBSIDIAN) as i32;
    let b = (g.id((x, y, z - 1)) == OBSIDIAN || g.id((x, y, z + 1)) == OBSIDIAN) as i32;
    if a == b {
        return false;
    }
    let (mut x, mut z) = (x, z);
    if g.id((x - a, y, z - b)) == 0 {
        x -= a;
        z -= b;
    }
    for i in -1..=2 {
        for j in -1..=3 {
            let edge = i == -1 || i == 2 || j == -1 || j == 3;
            if (i != -1 && i != 2) || (j != -1 && j != 3) {
                let id = g.id((x + a * i, y + j, z + b * i));
                if edge && id != OBSIDIAN || !edge && id != 0 && id != FIRE {
                    return false;
                }
            }
        }
    }
    let cells: Vec<P> = (0..2).flat_map(|i| (0..3).map(move |j| (x + a * i, y + j, z + b * i))).collect();
    cells.iter().for_each(|&c| g.put(c, PORTAL));
    cells.iter().for_each(|&c| g.notify(c));
    true
}

/// `BlockPortal.onNeighborBlockChange`: true when the portal cell must go (frame broken, or not a 2 x 3 pane).
pub fn broken(g: &impl Grid, (x, y, z): P) -> bool {
    let (a, b) = if g.id((x - 1, y, z)) == PORTAL || g.id((x + 1, y, z)) == PORTAL { (1, 0) } else { (0, 1) };
    let mut y0 = y;
    while g.id((x, y0 - 1, z)) == PORTAL {
        y0 -= 1;
    }
    if g.id((x, y0 - 1, z)) != OBSIDIAN {
        return true;
    }
    let mut n = 1;
    while n < 4 && g.id((x, y0 + n, z)) == PORTAL {
        n += 1;
    }
    if n != 3 || g.id((x, y0 + n, z)) != OBSIDIAN {
        return true;
    }
    let h = g.id((x - 1, y, z)) == PORTAL || g.id((x + 1, y, z)) == PORTAL;
    let v = g.id((x, y, z - 1)) == PORTAL || g.id((x, y, z + 1)) == PORTAL;
    if h && v {
        return true;
    }
    (g.id((x + a, y, z + b)) != OBSIDIAN || g.id((x - a, y, z - b)) != PORTAL) && (g.id((x - a, y, z - b)) != OBSIDIAN || g.id((x + a, y, z + b)) != PORTAL)
}

/// `Teleporter.func_4106_b`: the nearest portal floor within 128 blocks of `pos` among `cells` (every loaded portal cell), as
/// the point to arrive at: the middle of the pane's floor cell, pushed to the pane's plane. `y` is the floor cell's bottom.
pub fn find(g: &impl Grid, cells: &[P], pos: (f64, f64, f64)) -> Option<(f64, f64, f64)> {
    let mut best: Option<(f64, P)> = None;
    // The Java walks x, then z, then y downwards, and keeps the first of equal distances.
    let mut floors: Vec<P> = cells.iter().copied().filter(|&(x, y, z)| g.id((x, y - 1, z)) != PORTAL && (x as f64 + 0.5 - pos.0).abs() <= 128.0 && (z as f64 + 0.5 - pos.2).abs() <= 128.0).collect();
    floors.sort_by_key(|&(x, y, z)| (x, z, -y));
    for (x, y, z) in floors {
        let (dx, dy, dz) = (x as f64 + 0.5 - pos.0, y as f64 + 0.5 - pos.1, z as f64 + 0.5 - pos.2);
        let d = dx * dx + dy * dy + dz * dz;
        if best.map_or(true, |(b, _)| d < b) {
            best = Some((d, (x, y, z)));
        }
    }
    best.map(|(_, (x, y, z))| {
        let (mut fx, mut fz) = (x as f64 + 0.5, z as f64 + 0.5);
        if g.id((x - 1, y, z)) == PORTAL {
            fx -= 0.5;
        }
        if g.id((x + 1, y, z)) == PORTAL {
            fx += 0.5;
        }
        if g.id((x, y, z - 1)) == PORTAL {
            fz -= 0.5;
        }
        if g.id((x, y, z + 1)) == PORTAL {
            fz += 0.5;
        }
        (fx, y as f64, fz)
    })
}

/// `Teleporter.func_4108_c`: build a portal near `pos`: a 4 x 5 frame in the nearest spot (within 16) with solid ground and air
/// above, else (no spot) a platform at y 70..118. Cells are written quietly and pinged after, as `editingBlocks` does.
pub fn make(g: &mut impl Grid, pos: (f64, f64, f64), rng: &mut JavaRandom) {
    let (x0, y0, z0) = (pos.0.floor() as i32, pos.1.floor() as i32, pos.2.floor() as i32);
    let (mut bx, mut by, mut bz, mut dir) = (x0, y0, z0, 0);
    let mut best = -1.0f64;
    let r1 = rng.next_int_bound(4);
    let dist = |x: i32, y: i32, z: i32| ((x - x0) as f64 + 0.5).powi(2) + ((y - y0) as f64 + 0.5).powi(2) + ((z - z0) as f64 + 0.5).powi(2);
    for x in x0 - 16..=x0 + 16 {
        for z in z0 - 16..=z0 + 16 {
            let mut y = 127;
            while y >= 0 {
                if air(g, (x, y, z)) {
                    while y > 0 && air(g, (x, y - 1, z)) {
                        y -= 1;
                    }
                    'k: for k in r1..r1 + 4 {
                        let (mut a, mut b) = (k % 2, 1 - k % 2);
                        if k % 4 >= 2 {
                            (a, b) = (-a, -b);
                        }
                        for i in 0..3 {
                            for j in 0..4 {
                                for l in -1..4 {
                                    let c = (x + (j - 1) * a + i * b, y + l, z + (j - 1) * b - i * a);
                                    if l < 0 && !solid(g, c) || l >= 0 && !air(g, c) {
                                        break 'k;
                                    }
                                }
                            }
                        }
                        let d = dist(x, y, z);
                        if best < 0.0 || d < best {
                            (best, bx, by, bz, dir) = (d, x, y, z, k % 4);
                        }
                    }
                }
                y -= 1;
            }
        }
    }
    if best < 0.0 {
        for x in x0 - 16..=x0 + 16 {
            for z in z0 - 16..=z0 + 16 {
                let mut y = 127;
                while y >= 0 {
                    if air(g, (x, y, z)) {
                        while y > 0 && air(g, (x, y - 1, z)) {
                            y -= 1;
                        }
                        'k2: for k in r1..r1 + 2 {
                            let (a, b) = (k % 2, 1 - k % 2);
                            for i in 0..4 {
                                for l in -1..4 {
                                    let c = (x + (i - 1) * a, y + l, z + (i - 1) * b);
                                    if l < 0 && !solid(g, c) || l >= 0 && !air(g, c) {
                                        break 'k2;
                                    }
                                }
                            }
                            let d = dist(x, y, z);
                            if best < 0.0 || d < best {
                                (best, bx, by, bz, dir) = (d, x, y, z, k % 2);
                            }
                        }
                    }
                    y -= 1;
                }
            }
        }
    }
    let (mut a, mut b) = (dir % 2, 1 - dir % 2);
    if dir % 4 >= 2 {
        (a, b) = (-a, -b);
    }
    if best < 0.0 {
        by = by.clamp(70, 118);
        for i in -1..=1 {
            for j in 1..3 {
                for l in -1..3 {
                    g.put((bx + (j - 1) * a + i * b, by + l, bz + (j - 1) * b - i * a), if l < 0 { OBSIDIAN } else { 0 });
                }
            }
        }
    }
    let mut cells = Vec::new();
    for i in 0..4 {
        for l in -1..4 {
            let c = (bx + (i - 1) * a, by + l, bz + (i - 1) * b);
            g.put(c, if i == 0 || i == 3 || l == -1 || l == 3 { OBSIDIAN } else { PORTAL });
            cells.push(c);
        }
    }
    cells.into_iter().for_each(|c| g.notify(c));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Toy(HashMap<P, u8>, Vec<P>);
    impl Grid for Toy {
        fn id(&self, p: P) -> u8 {
            *self.0.get(&p).unwrap_or(&if p.1 < 60 { 1 } else { 0 }) // stone below y 60, air above
        }
        fn put(&mut self, p: P, id: u8) {
            self.0.insert(p, id);
        }
        fn notify(&mut self, p: P) {
            self.1.push(p);
        }
    }

    fn frame(g: &mut Toy, (x, y, z): P) {
        for i in 0..4 {
            for l in 0..5 {
                if i == 0 || i == 3 || l == 0 || l == 4 {
                    g.put((x + i, y + l, z), OBSIDIAN);
                }
            }
        }
    }

    #[test]
    fn fire_in_a_frame_lights_a_portal_and_a_broken_frame_unlights_it() {
        let mut g = Toy::default();
        frame(&mut g, (10, 70, 5));
        g.put((11, 71, 5), FIRE);
        assert!(try_create(&mut g, (11, 71, 5)));
        for (i, l) in [(1, 1), (2, 1), (1, 2), (2, 2), (1, 3), (2, 3)] {
            assert_eq!(g.id((10 + i, 70 + l, 5)), PORTAL);
        }
        assert!(!broken(&g, (11, 72, 5)));
        g.put((10, 72, 5), 0); // a side of the frame
        assert!(broken(&g, (11, 72, 5)));
    }

    #[test]
    fn an_open_frame_stays_dark() {
        let mut g = Toy::default();
        frame(&mut g, (0, 70, 0));
        g.put((1, 74, 0), 0); // a hole in the top
        g.put((1, 71, 0), FIRE);
        assert!(!try_create(&mut g, (1, 71, 0)));
    }

    #[test]
    fn find_takes_the_nearest_portal_floor_and_make_builds_a_lit_one() {
        let mut g = Toy::default();
        let mut r = JavaRandom::new(5);
        make(&mut g, (30.0, 64.0, 30.0), &mut r);
        let cells: Vec<P> = g.0.iter().filter(|&(_, &v)| v == PORTAL).map(|(&p, _)| p).collect();
        assert_eq!(cells.len(), 6, "a 2 x 3 pane");
        assert!(cells.iter().all(|&c| !broken(&g, c)), "the new portal is stable");
        let (x, y, z) = find(&g, &cells, (30.0, 64.0, 30.0)).expect("found");
        assert!((x - 30.0).abs() < 20.0 && (z - 30.0).abs() < 20.0 && y >= 60.0);
        assert!(find(&g, &cells, (500.0, 64.0, 500.0)).is_none(), "beyond 128 blocks");
    }
}
