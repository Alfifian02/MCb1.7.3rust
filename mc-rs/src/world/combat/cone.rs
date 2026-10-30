//! Reach as a cone (`ConeMath`, `Reach.inCone`, `ConeTargets.nearestInCone`). **NOT b1.7.3.**
//!
//! A target is hit when its closest point to the eye is within `dist` and inside the horizontal and vertical
//! half-angles of the look direction. A thin "leash" keeps a point blank target from falling out of a narrow cone
//! just because its centre is off the axis: the allowed width is `max(along * tan(angle), leash)`.

use glam::DVec3;

use super::profile::Reach;

const EPS: f64 = 1e-6;
/// `ConeMath.LEASH_FRACTION` / `LEASH_CAP`: 65% of the target's half extent, at most 0.8 blocks.
const LEASH_FRACTION: f64 = 0.65;
const LEASH_CAP: f64 = 0.8;

/// `ConeMath.leash`.
pub fn leash(half_extent: f64) -> f64 {
    if half_extent <= 0.0 || half_extent.is_nan() {
        0.0
    } else {
        (half_extent * LEASH_FRACTION).min(LEASH_CAP)
    }
}

/// `ConeMath.tangent`: tan of the angle, the angle capped at 89 degrees; 0 for an angle that is not positive.
pub fn tangent(deg: f64) -> f64 {
    if deg.is_nan() || deg <= 0.0 {
        0.0
    } else {
        deg.min(89.0).to_radians().tan()
    }
}

/// `ConeMath.inCone`: is `target` inside the cone at `origin` looking along `look`?
/// `leash_lat` / `leash_vert` are the minimum half widths (0 for a point target).
pub fn in_cone(origin: DVec3, look: DVec3, target: DVec3, r: Reach, leash_lat: f64, leash_vert: f64) -> bool {
    let d = target - origin;
    let dist = d.length();
    if dist > r.dist as f64 {
        return false;
    }
    if dist < EPS {
        return true;
    }
    if look.length() < EPS {
        return false;
    }
    let f = look.normalize();
    // `ConeMath.frame`: right = horizontal perpendicular of the look (x axis when looking straight up or down),
    // up = right x forward.
    let h = (f.x * f.x + f.z * f.z).sqrt();
    let right = if h < EPS { DVec3::X } else { DVec3::new(-f.z / h, 0.0, f.x / h) };
    let up = right.cross(f);
    let along = d.dot(f);
    if along <= EPS {
        return false;
    }
    let (lateral, vertical) = (d.dot(right), d.dot(up));
    lateral.abs() <= (along * tangent(r.h as f64)).max(leash_lat) && vertical.abs() <= (along * tangent(r.v as f64)).max(leash_vert)
}

/// `Reach.inCone(origin, look, box, reach)`: the point of the box nearest the eye is tested, with the leash taken
/// from the box's own size (half of its wider horizontal side, half of its height).
pub fn box_in_cone(origin: DVec3, look: DVec3, lo: DVec3, hi: DVec3, r: Reach) -> bool {
    let p = origin.clamp(lo, hi); // `closestOnAxis` on each axis
    let size = hi - lo;
    in_cone(origin, look, p, r, leash(size.x.max(size.z) * 0.5), leash(size.y * 0.5))
}

/// `ConeTargets.nearestInCone`: the indexes of the boxes in the cone, nearest first (by squared distance from the
/// eye to the box centre, like `distanceToSqr` on the entity), at most `limit` of them (0 = all).
pub fn nearest_in_cone(origin: DVec3, look: DVec3, boxes: &[(DVec3, DVec3)], r: Reach, limit: usize) -> Vec<usize> {
    let mut hit: Vec<(usize, f64)> = boxes
        .iter()
        .enumerate()
        .filter(|(_, (lo, hi))| box_in_cone(origin, look, *lo, *hi, r))
        .map(|(i, (lo, hi))| (i, origin.distance_squared((*lo + *hi) * 0.5)))
        .collect();
    hit.sort_by(|a, b| a.1.total_cmp(&b.1));
    let n = if limit > 0 { limit.min(hit.len()) } else { hit.len() };
    hit[..n].iter().map(|h| h.0).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SWORD: Reach = Reach { dist: 3.6, h: 35.0, v: 30.0 };
    const ROPERA: Reach = Reach { dist: 3.6, h: 13.0, v: 16.0 };

    fn at(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    // One check for the geometry: the cases the mod's ConeMathTest / ConeLeashTest exist for.
    #[test]
    fn cone_follows_distance_angle_and_leash() {
        let (o, look) = (at(0.0, 0.0, 0.0), at(0.0, 0.0, 1.0));
        assert!(in_cone(o, look, at(0.0, 0.0, 3.0), SWORD, 0.0, 0.0)); // dead ahead
        assert!(!in_cone(o, look, at(0.0, 0.0, 3.7), SWORD, 0.0, 0.0)); // beyond the reach
        assert!(!in_cone(o, look, at(0.0, 0.0, -1.0), SWORD, 0.0, 0.0)); // behind
        assert!(in_cone(o, look, o, SWORD, 0.0, 0.0)); // on the eye
        // 2 blocks ahead, 1 aside: atan(0.5) = 26.6 degrees, inside the sword's 35 but outside the ropera's 13.
        assert!(in_cone(o, look, at(1.0, 0.0, 2.0), SWORD, 0.0, 0.0));
        assert!(!in_cone(o, look, at(1.0, 0.0, 2.0), ROPERA, 0.0, 0.0));
        // Point blank, 0.5 aside at 0.5 ahead is 45 degrees: out for the ropera, rescued by a 0.6 leash.
        assert!(!in_cone(o, look, at(0.5, 0.0, 0.5), ROPERA, 0.0, 0.0));
        assert!(in_cone(o, look, at(0.5, 0.0, 0.5), ROPERA, 0.6, 0.0));
        // Looking straight up must not divide by zero: right falls back to +x.
        assert!(in_cone(o, at(0.0, 1.0, 0.0), at(0.0, 2.0, 0.0), SWORD, 0.0, 0.0));
        assert_eq!(leash(1.0), 0.65); // 65% of the half extent...
        assert_eq!(leash(5.0), 0.8); // ...capped at 0.8
        assert_eq!(leash(-1.0), 0.0);
    }

    #[test]
    fn nearest_in_cone_sorts_and_limits() {
        let (o, look) = (at(0.0, 1.6, 0.0), at(0.0, 0.0, 1.0));
        let b = |z: f64| (at(-0.3, 0.0, z), at(0.3, 1.8, z + 0.6));
        let boxes = [b(3.0), b(1.0), b(2.0), (at(10.0, 0.0, 1.0), at(10.6, 1.8, 1.6))];
        assert_eq!(nearest_in_cone(o, look, &boxes, SWORD, 0), vec![1, 2, 0]); // the far-aside one is out
        assert_eq!(nearest_in_cone(o, look, &boxes, SWORD, 2), vec![1, 2]);
    }
}
