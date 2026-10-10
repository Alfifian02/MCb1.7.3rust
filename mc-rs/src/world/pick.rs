//! M5 block picking: the voxel ray trace and the place/break rules.
//!
//! `ray_trace` is `World.func_28105_a` (rayTraceBlocks) with `Block.collisionRayTrace` for the block's
//! bounds (`chunk::pick_bounds`: a sapling's small box, a slab's half cell, ...), in f64 like the Java. Face numbers are `MovingObjectPosition.sideHit`:
//! 0 = -Y, 1 = +Y, 2 = -Z, 3 = +Z, 4 = -X, 5 = +X.

use glam::{DVec3, Vec3};

use crate::world::chunk::{collision, is_plant, normal_cube};
use crate::world::physics::HALF;

/// A block's bounds inside its cell: [min x, y, z, max x, y, z].
pub type Bounds = [f32; 6];

/// `PlayerControllerSP.getBlockReachDistance`.
pub const REACH: f64 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    /// The block that was hit.
    pub pos: (i32, i32, i32),
    pub face: u8,
    pub point: DVec3,
}

/// `Block.collisionRayTrace` for `bounds`: nearest face plane the segment crosses
/// inside the face, tested in the Java order (-X, +X, -Y, +Y, -Z, +Z) with a strict `<` tie-break.
fn box_hit(cell: (i32, i32, i32), bounds: Bounds, start: DVec3, end: DVec3) -> Option<Hit> {
    let (lo, hi) = (DVec3::from_array([0, 1, 2].map(|a| bounds[a] as f64)), DVec3::from_array([0, 1, 2].map(|a| bounds[a + 3] as f64)));
    let c = DVec3::new(cell.0 as f64, cell.1 as f64, cell.2 as f64);
    let (s, e) = (start - c, end - c);
    // Vec3D.getIntermediateWith{X,Y,Z}Value, including the (double)1.0E-7F parallel cut-off.
    let at = |axis: usize, v: f64| -> Option<DVec3> {
        let d = e - s;
        let da = d[axis];
        if da * da < 1.0e-7_f32 as f64 {
            return None;
        }
        let t = (v - s[axis]) / da;
        (0.0..=1.0).contains(&t).then(|| s + d * t)
    };
    let inside = |p: DVec3, a: usize, b: usize| (lo[a]..=hi[a]).contains(&p[a]) && (lo[b]..=hi[b]).contains(&p[b]);
    // (axis, plane, the two other axes, face)
    let planes = [(0, lo.x, 1, 2, 4u8), (0, hi.x, 1, 2, 5), (1, lo.y, 0, 2, 0), (1, hi.y, 0, 2, 1), (2, lo.z, 0, 1, 2), (2, hi.z, 0, 1, 3)];
    let mut best: Option<(DVec3, u8)> = None;
    for (axis, v, a, b, face) in planes {
        let Some(p) = at(axis, v).filter(|&p| inside(p, a, b)) else { continue };
        if best.map_or(true, |(q, _)| s.distance(p) < s.distance(q)) {
            best = Some((p, face));
        }
    }
    best.map(|(p, face)| Hit { pos: cell, face, point: p + c })
}

/// First block along `start -> end` whose `bounds` the segment crosses, or `None` (`bounds` is `None` for air and
/// whatever the ray passes through). The cell walk is the Java loop, including its 200 step cap.
pub fn ray_trace(bounds: &dyn Fn(i32, i32, i32) -> Option<Bounds>, start: DVec3, end: DVec3) -> Option<Hit> {
    if !start.is_finite() || !end.is_finite() {
        return None;
    }
    let fl = |v: f64| v.floor() as i32;
    let (ex, ey, ez) = (fl(end.x), fl(end.y), fl(end.z));
    let (mut bx, mut by, mut bz) = (fl(start.x), fl(start.y), fl(start.z));
    let mut cur = start;
    if let Some(b) = bounds(bx, by, bz) {
        if let Some(h) = box_hit((bx, by, bz), b, cur, end) {
            return Some(h);
        }
    }
    for _ in 0..=200 {
        if (bx, by, bz) == (ex, ey, ez) {
            return None;
        }
        // Next cell boundary on each axis that still has to be crossed.
        let (mut nx, mut ny, mut nz) = (999.0, 999.0, 999.0);
        let (mut fx, mut fy, mut fz) = (true, true, true);
        if ex > bx { nx = bx as f64 + 1.0 } else if ex < bx { nx = bx as f64 } else { fx = false }
        if ey > by { ny = by as f64 + 1.0 } else if ey < by { ny = by as f64 } else { fy = false }
        if ez > bz { nz = bz as f64 + 1.0 } else if ez < bz { nz = bz as f64 } else { fz = false }
        let (dx, dy, dz) = (end.x - cur.x, end.y - cur.y, end.z - cur.z);
        let tx = if fx { (nx - cur.x) / dx } else { 999.0 };
        let ty = if fy { (ny - cur.y) / dy } else { 999.0 };
        let tz = if fz { (nz - cur.z) / dz } else { 999.0 };
        let face;
        if tx < ty && tx < tz {
            face = if ex > bx { 4 } else { 5 };
            cur = DVec3::new(nx, cur.y + dy * tx, cur.z + dz * tx);
        } else if ty < tz {
            face = if ey > by { 0 } else { 1 };
            cur = DVec3::new(cur.x + dx * ty, ny, cur.z + dz * ty);
        } else {
            face = if ez > bz { 2 } else { 3 };
            cur = DVec3::new(cur.x + dx * tz, cur.y + dy * tz, nz);
        }
        bx = fl(cur.x);
        if face == 5 { bx -= 1 }
        by = fl(cur.y);
        if face == 1 { by -= 1 }
        bz = fl(cur.z);
        if face == 3 { bz -= 1 }
        if let Some(b) = bounds(bx, by, bz) {
            if let Some(h) = box_hit((bx, by, bz), b, cur, end) {
                return Some(h);
            }
        }
    }
    None
}

/// Cell a block placed against `hit` goes into (`ItemBlock.onItemUse` offsets by the face).
/// ponytail: the snow-layer rule (place into the snow itself) is missing: a block placed on a snow layer goes above it.
pub fn place_pos(hit: &Hit) -> (i32, i32, i32) {
    let (dx, dy, dz) = [(0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)][hit.face as usize];
    (hit.pos.0 + dx, hit.pos.1 + dy, hit.pos.2 + dz)
}

/// Cells a block can be placed into (`World.canBlockBePlacedAt`: air, water, lava, fire, snow).
/// Plants count too: they are drawn as nothing yet, so refusing to place into one would look like a bug.
pub fn replaceable(id: u8) -> bool {
    matches!(id, 0 | 8..=11 | 51 | 78) || is_plant(id)
}

/// `BlockTorch.func_31032_h`: a torch can stand on a normal cube or a fence.
fn torch_floor(get: &dyn Fn(i32, i32, i32) -> u8, x: i32, y: i32, z: i32) -> bool {
    let b = get(x, y - 1, z);
    normal_cube(b) || b == 85
}

/// Which of a torch's supports exist at `(x, y, z)`: walls -X, +X, -Z, +Z (metadata 1..=4) and the floor (5).
fn torch_supports(get: &dyn Fn(i32, i32, i32) -> u8, x: i32, y: i32, z: i32) -> [bool; 5] {
    let n = |dx, dz| normal_cube(get(x + dx, y, z + dz));
    [n(-1, 0), n(1, 0), n(0, -1), n(0, 1), torch_floor(get, x, y, z)]
}

/// `BlockTorch.canPlaceBlockAt` + `onBlockAdded` + `onBlockPlaced`: the metadata of a torch placed at `cell` against the face
/// `face` of the block aimed at (`Hit`'s numbering), or `None` when nothing can hold it. The side aimed at wins when it can hold
/// the torch, else the first support in the order -X, +X, -Z, +Z, floor.
pub fn torch_meta(get: &dyn Fn(i32, i32, i32) -> u8, (x, y, z): (i32, i32, i32), face: u8) -> Option<u8> {
    let s = torch_supports(get, x, y, z);
    let aimed = match face { 1 => 5, 2 => 4, 3 => 3, 4 => 2, 5 => 1, _ => 0 };
    if aimed > 0 && s[aimed as usize - 1] {
        return Some(aimed);
    }
    s.iter().position(|&h| h).map(|i| i as u8 + 1)
}

/// `BlockTorch.onNeighborBlockChange`: false when the torch with metadata `meta` has lost what it hangs on.
pub fn torch_stays(get: &dyn Fn(i32, i32, i32) -> u8, (x, y, z): (i32, i32, i32), meta: u8) -> bool {
    let s = torch_supports(get, x, y, z);
    s.iter().any(|&h| h) && (!(1..=5).contains(&meta) || s[meta as usize - 1])
}

/// `World.checkIfAABBIsClear` against the player: does the collision box of block `id` in `cell` strictly overlap the
/// player box (`pos` is the box centre, see `world::physics`)? A block with no collision box (torch, sapling) never does.
pub fn overlaps_player(cell: (i32, i32, i32), id: u8, pos: Vec3) -> bool {
    let Some(b) = collision(id) else { return false };
    let c = Vec3::new(cell.0 as f32, cell.1 as f32, cell.2 as f32);
    let (lo, hi, bl, bh) = (pos - HALF, pos + HALF, c + Vec3::from_slice(&b[..3]), c + Vec3::from_slice(&b[3..]));
    hi.x > bl.x && lo.x < bh.x && hi.y > bl.y && lo.y < bh.y && hi.z > bl.z && lo.z < bh.z
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world(x: i32, y: i32, z: i32) -> Option<Bounds> {
        ((x, y, z) == (5, 10, 5)).then_some([0.0, 0.0, 0.0, 1.0, 1.0, 1.0])
    }
    fn v(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    /// Hits from each side report the face that was entered, the block, and the exact point.
    #[test]
    fn ray_hits_the_right_face_and_point() {
        let h = ray_trace(&world, v(2.5, 10.5, 5.5), v(7.5, 10.5, 5.5)).unwrap();
        assert_eq!((h.pos, h.face, h.point.x), ((5, 10, 5), 4, 5.0)); // from -X: the -X face
        let h = ray_trace(&world, v(8.5, 10.5, 5.5), v(3.5, 10.5, 5.5)).unwrap();
        assert_eq!((h.face, h.point.x), (5, 6.0)); // from +X: the +X face
        let h = ray_trace(&world, v(5.5, 14.5, 5.5), v(5.5, 9.5, 5.5)).unwrap();
        assert_eq!((h.face, h.point.y), (1, 11.0)); // from above: the top
        let h = ray_trace(&world, v(5.5, 7.5, 5.5), v(5.5, 12.5, 5.5)).unwrap();
        assert_eq!((h.face, h.point.y), (0, 10.0)); // from below
        let h = ray_trace(&world, v(5.5, 10.5, 2.5), v(5.5, 10.5, 7.5)).unwrap();
        assert_eq!((h.face, h.point.z), (2, 5.0));
        let h = ray_trace(&world, v(5.5, 10.5, 8.5), v(5.5, 10.5, 3.5)).unwrap();
        assert_eq!((h.face, h.point.z), (3, 6.0));
        // A slanted ray that crosses the block's corner column.
        assert!(ray_trace(&world, v(1.2, 12.9, 1.3), v(8.2, 8.1, 8.3)).is_some());
    }

    /// A segment that stops short, or passes beside the block, hits nothing; a start inside the block hits it.
    #[test]
    fn ray_misses_and_starts_inside() {
        assert_eq!(ray_trace(&world, v(0.5, 10.5, 5.5), v(4.5, 10.5, 5.5)), None); // reach ends before it
        assert_eq!(ray_trace(&world, v(0.5, 10.5, 6.5), v(9.5, 10.5, 6.5)), None); // one row beside
        assert_eq!(ray_trace(&world, v(f64::NAN, 0.0, 0.0), v(1.0, 1.0, 1.0)), None);
        let h = ray_trace(&world, v(5.5, 10.5, 5.5), v(5.5, 10.5, 9.5)).unwrap();
        assert_eq!((h.pos, h.face), ((5, 10, 5), 3)); // eye inside: the exit face, like the Java
    }

    #[test]
    fn placement_rules() {
        let hit = Hit { pos: (5, 10, 5), face: 1, point: DVec3::ZERO };
        assert_eq!(place_pos(&hit), (5, 11, 5));
        assert_eq!(place_pos(&Hit { face: 4, ..hit }), (4, 10, 5));
        assert!(replaceable(0) && replaceable(9) && replaceable(31) && !replaceable(1));
        // Player centre (5.5, 11.9, 5.5) stands on top of (5, 10, 5): feet at 11.0, box 11.0..12.8.
        let p = Vec3::new(5.5, 11.9, 5.5);
        assert!(overlaps_player((5, 11, 5), 1, p) && overlaps_player((5, 12, 5), 1, p));
        assert!(!overlaps_player((5, 10, 5), 1, p) && !overlaps_player((5, 13, 5), 1, p) && !overlaps_player((6, 11, 5), 1, p));
        assert!(!overlaps_player((5, 11, 5), 50, p) && !overlaps_player((5, 11, 5), 6, p)); // torch, sapling: no collision box
        assert!(!overlaps_player((5, 12, 5), 44, Vec3::new(5.5, 13.5, 5.5)) && overlaps_player((5, 12, 5), 44, Vec3::new(5.5, 13.2, 5.5))); // a slab reaches y 12.5
    }

    /// A small box is hit only inside its own bounds: a torch (0.4..0.6 wide, 0.6 high) is missed 0.3 beside its axis and over its
    /// top, hit through its middle, and the face reported is the box's.
    #[test]
    fn ray_hits_the_bounds_not_the_cell() {
        let torch = |x: i32, y: i32, z: i32| ((x, y, z) == (5, 10, 5)).then_some([0.4, 0.0, 0.4, 0.6, 0.6, 0.6]);
        let h = ray_trace(&torch, v(2.5, 10.3, 5.5), v(7.5, 10.3, 5.5)).unwrap();
        assert_eq!((h.pos, h.face), ((5, 10, 5), 4));
        assert!((h.point.x - 5.4).abs() < 1e-6, "x = {}", h.point.x);
        assert_eq!(ray_trace(&torch, v(2.5, 10.3, 5.2), v(7.5, 10.3, 5.2)), None); // beside it, inside the cell
        assert_eq!(ray_trace(&torch, v(2.5, 10.8, 5.5), v(7.5, 10.8, 5.5)), None); // above its top (0.6), inside the cell
    }

    /// Torch: the face aimed at decides the facing when a block holds it there, else the first support in the Java order;
    /// nothing to hold it = no placement; losing the wall it hangs on drops it.
    #[test]
    fn torch_facing_and_support() {
        let w = |x: i32, y: i32, z: i32| if (x, y, z) == (5, 10, 5) { 1u8 } else { 0 };
        assert_eq!(torch_meta(&w, (6, 10, 5), 5), Some(1), "east face of the stone: leans on the -X wall");
        assert_eq!(torch_meta(&w, (5, 11, 5), 1), Some(5), "top: stands on the floor");
        assert_eq!(torch_meta(&w, (4, 10, 5), 4), Some(2), "west face: hangs on the +X wall");
        assert_eq!(torch_meta(&w, (6, 10, 5), 1), Some(1), "a face that cannot hold it: the first support");
        assert_eq!(torch_meta(&w, (6, 12, 5), 1), None, "nothing next to it or under it");
        let f = |x: i32, y: i32, z: i32| if (x, y, z) == (5, 10, 5) { 85u8 } else { 0 };
        assert_eq!(torch_meta(&f, (5, 11, 5), 1), Some(5), "a fence holds it from below");
        let g = |x: i32, y: i32, z: i32| if (x, y, z) == (5, 10, 5) { 20u8 } else { 0 };
        assert_eq!(torch_meta(&g, (5, 11, 5), 1), None, "glass does not");
        assert!(torch_stays(&w, (6, 10, 5), 1) && !torch_stays(&w, (6, 10, 5), 4) && !torch_stays(&w, (6, 12, 5), 5));
    }
}
