//! M5 block picking: the voxel ray trace and the place/break rules.
//!
//! `ray_trace` is `World.func_28105_a` (rayTraceBlocks) with `Block.collisionRayTrace` for a unit
//! cube, in f64 like the Java. Face numbers are `MovingObjectPosition.sideHit`:
//! 0 = -Y, 1 = +Y, 2 = -Z, 3 = +Z, 4 = -X, 5 = +X.

use glam::{DVec3, Vec3};

use crate::world::chunk::is_plant;
use crate::world::physics::HALF;

/// `PlayerControllerSP.getBlockReachDistance`.
pub const REACH: f64 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    /// The block that was hit.
    pub pos: (i32, i32, i32),
    pub face: u8,
    pub point: DVec3,
}

/// `Block.collisionRayTrace` for bounds (0,0,0)-(1,1,1): nearest face plane the segment crosses
/// inside the face, tested in the Java order (-X, +X, -Y, +Y, -Z, +Z) with a strict `<` tie-break.
fn cube_hit(cell: (i32, i32, i32), start: DVec3, end: DVec3) -> Option<Hit> {
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
    let inside = |p: DVec3, a: usize, b: usize| (0.0..=1.0).contains(&p[a]) && (0.0..=1.0).contains(&p[b]);
    // (axis, plane, the two other axes, face)
    let planes = [(0, 0.0, 1, 2, 4u8), (0, 1.0, 1, 2, 5), (1, 0.0, 0, 2, 0), (1, 1.0, 0, 2, 1), (2, 0.0, 0, 1, 2), (2, 1.0, 0, 1, 3)];
    let mut best: Option<(DVec3, u8)> = None;
    for (axis, v, a, b, face) in planes {
        let Some(p) = at(axis, v).filter(|&p| inside(p, a, b)) else { continue };
        if best.map_or(true, |(q, _)| s.distance(p) < s.distance(q)) {
            best = Some((p, face));
        }
    }
    best.map(|(p, face)| Hit { pos: cell, face, point: p + c })
}

/// First block along `start -> end` for which `solid` is true, or `None`. The cell walk is the
/// Java loop, including its 200 step cap.
pub fn ray_trace(solid: &dyn Fn(i32, i32, i32) -> bool, start: DVec3, end: DVec3) -> Option<Hit> {
    if !start.is_finite() || !end.is_finite() {
        return None;
    }
    let fl = |v: f64| v.floor() as i32;
    let (ex, ey, ez) = (fl(end.x), fl(end.y), fl(end.z));
    let (mut bx, mut by, mut bz) = (fl(start.x), fl(start.y), fl(start.z));
    let mut cur = start;
    if solid(bx, by, bz) {
        if let Some(h) = cube_hit((bx, by, bz), cur, end) {
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
        if solid(bx, by, bz) {
            if let Some(h) = cube_hit((bx, by, bz), cur, end) {
                return Some(h);
            }
        }
    }
    None
}

/// Cell a block placed against `hit` goes into (`ItemBlock.onItemUse` offsets by the face).
/// The snow-layer rule (place into the snow itself) is not needed: plants are not pickable yet.
pub fn place_pos(hit: &Hit) -> (i32, i32, i32) {
    let (dx, dy, dz) = [(0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)][hit.face as usize];
    (hit.pos.0 + dx, hit.pos.1 + dy, hit.pos.2 + dz)
}

/// Cells a block can be placed into (`World.canBlockBePlacedAt`: air, water, lava, fire, snow).
/// Plants count too: they are drawn as nothing yet, so refusing to place into one would look like a bug.
pub fn replaceable(id: u8) -> bool {
    matches!(id, 0 | 8..=11 | 51 | 78) || is_plant(id)
}

/// `World.checkIfAABBIsClear` against the player: does the cube at `cell` strictly overlap the
/// player box (`pos` is the box centre, see `world::physics`)?
pub fn overlaps_player(cell: (i32, i32, i32), pos: Vec3) -> bool {
    let (c, lo, hi) = (Vec3::new(cell.0 as f32, cell.1 as f32, cell.2 as f32), pos - HALF, pos + HALF);
    hi.x > c.x && lo.x < c.x + 1.0 && hi.y > c.y && lo.y < c.y + 1.0 && hi.z > c.z && lo.z < c.z + 1.0
}

/// Bedrock is the only unbreakable block worldgen makes (`setHardness(-1.0F)`).
pub fn breakable(id: u8) -> bool {
    id != 7
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world(x: i32, y: i32, z: i32) -> bool {
        (x, y, z) == (5, 10, 5)
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
        assert!(overlaps_player((5, 11, 5), p) && overlaps_player((5, 12, 5), p));
        assert!(!overlaps_player((5, 10, 5), p) && !overlaps_player((5, 13, 5), p) && !overlaps_player((6, 11, 5), p));
        assert!(breakable(1) && !breakable(7));
    }
}
