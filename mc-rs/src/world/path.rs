//! `Pathfinder` (A*), `PathPoint`, `Path`, `PathEntity`: the way `EntityCreature` walks to a player around walls, over one-block
//! steps and down drops of up to 3, never through water or lava. A point is a cell; the walk is 4-connected, one cell at a time,
//! with one step up when the cell above the current one is free (`findPathOptions`). A mob wider or taller than a cell checks the
//! whole box of cells it needs (`getVerticalOffset` over `size`).
//! ponytail: the open list is a binary heap with lazy deletion (`Path.changeDistance` re-sifts in place), so equal-cost ties may
//! break differently; the search stops after 3000 expansions; doors are whatever the block query says (open = air).

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use crate::world::physics::BlockQuery;

pub type Pt = (i32, i32, i32);

fn dist(a: Pt, b: Pt) -> f32 {
    let (x, y, z) = ((a.0 - b.0) as f32, (a.1 - b.1) as f32, (a.2 - b.2) as f32);
    (x * x + y * y + z * z).sqrt()
}

/// `Pathfinder.getVerticalOffset` over the box `size` of cells at `p`: 1 free, 0 blocked, -1 water, -2 lava.
fn offset(get: BlockQuery<'_>, p: Pt, size: Pt) -> i32 {
    for x in p.0..p.0 + size.0 {
        for y in p.1..p.1 + size.1 {
            for z in p.2..p.2 + size.2 {
                match get(x, y, z) {
                    Some(8 | 9) => return -1,
                    Some(10 | 11) => return -2,
                    Some(b) if b > 0 => return 0,
                    _ => {}
                }
            }
        }
    }
    1
}

/// `Pathfinder.getSafePoint`: the cell at `(x, y, z)`, or one up by `up`, then dropped to the floor (a fall of 4 or more, or onto
/// lava, is no point).
fn safe(get: BlockQuery<'_>, (x, mut y, z): Pt, size: Pt, up: i32) -> Option<Pt> {
    let mut r = None;
    if offset(get, (x, y, z), size) == 1 {
        r = Some((x, y, z));
    }
    if r.is_none() && up > 0 && offset(get, (x, y + up, z), size) == 1 {
        r = Some((x, y + up, z));
        y += up;
    }
    if r.is_some() {
        let (mut fell, mut below) = (0, 0);
        while y > 0 {
            below = offset(get, (x, y - 1, z), size);
            if below != 1 {
                break;
            }
            fell += 1;
            if fell >= 4 {
                return None;
            }
            y -= 1;
            if y > 0 {
                r = Some((x, y, z));
            }
        }
        if below == -2 {
            return None;
        }
    }
    r
}

#[derive(Clone, Copy)]
struct Node {
    g: f32,
    prev: Option<Pt>,
    assigned: bool,
    closed: bool,
}

/// `Pathfinder.createEntityPathTo`: the cells from `start` to `target` (both included), or, when the target cannot be reached, to
/// the cell nearest to it that could (`None` when not even one step gets closer). Only cells within `max` of the target are used.
pub fn find(get: BlockQuery<'_>, start: Pt, target: Pt, size: Pt, max: f32) -> Option<Vec<Pt>> {
    let mut nodes: HashMap<Pt, Node> = HashMap::new();
    nodes.insert(start, Node { g: 0.0, prev: None, assigned: true, closed: false });
    let mut open = BinaryHeap::new();
    let key = |f: f32| Reverse((f * 1024.0) as i64);
    open.push((key(dist(start, target)), start));
    let mut best = start;
    let mut budget = 3000;
    while let Some((Reverse(_), p)) = open.pop() {
        if nodes[&p].closed {
            continue; // a stale entry: the point was queued again with a smaller cost
        }
        if p == target {
            return Some(build(&nodes, start, p));
        }
        if dist(p, target) < dist(best, target) {
            best = p;
        }
        nodes.get_mut(&p).unwrap().closed = true;
        budget -= 1;
        if budget == 0 {
            break;
        }
        // `findPathOptions`: one up if the cell above is free, then the four sides (+z, -x, +x, -z).
        let up = i32::from(offset(get, (p.0, p.1 + 1, p.2), size) == 1);
        for (dx, dz) in [(0, 1), (-1, 0), (1, 0), (0, -1)] {
            let Some(q) = safe(get, (p.0 + dx, p.1, p.2 + dz), size, up) else { continue };
            if nodes.get(&q).is_some_and(|n| n.closed) || dist(q, target) >= max {
                continue;
            }
            let g = nodes[&p].g + dist(p, q);
            let n = nodes.entry(q).or_insert(Node { g: f32::MAX, prev: None, assigned: false, closed: false });
            if !n.assigned || g < n.g {
                *n = Node { g, prev: Some(p), assigned: true, closed: false };
                open.push((key(g + dist(q, target)), q));
            }
        }
    }
    (best != start).then(|| build(&nodes, start, best))
}

/// `createEntityPath`: follow `previous` back from `end`.
fn build(nodes: &HashMap<Pt, Node>, start: Pt, end: Pt) -> Vec<Pt> {
    let mut v = vec![end];
    let mut p = end;
    while p != start {
        p = nodes[&p].prev.expect("every point but the start has a previous");
        v.push(p);
    }
    v.reverse();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: Pt = (1, 2, 1);

    /// Floor at y = 9 (stone), air above, a wall of stone 3 high at x = 10 for z in 0..=14 (open at z = 15), 32 wide.
    fn world(x: i32, y: i32, z: i32) -> Option<u8> {
        if !(0..32).contains(&x) || !(0..32).contains(&z) || y < 0 { return None }
        Some(if y <= 9 { 1 } else if x == 10 && z <= 14 && y <= 12 { 1 } else { 0 })
    }

    /// It goes round a wall, every step is one cell, and the path ends on the target.
    #[test]
    fn walks_round_a_wall() {
        let p = find(&world, (5, 10, 5), (15, 10, 5), ONE, 40.0).expect("a way round");
        assert_eq!((p[0], *p.last().unwrap()), ((5, 10, 5), (15, 10, 5)));
        assert!(p.iter().any(|c| c.2 >= 15), "through the gap at z = 15");
        assert!(p.windows(2).all(|w| (w[0].0 - w[1].0).abs() + (w[0].2 - w[1].2).abs() == 1 && (w[0].1 - w[1].1).abs() <= 3));
        assert!(p.iter().all(|&(x, y, z)| world(x, y, z) == Some(0)), "never inside a block");
    }

    /// A one-block step up is climbed, a drop of 3 is taken, a drop of 4 is not, and lava is never stepped on.
    #[test]
    fn steps_up_and_down_but_not_off_cliffs_or_into_lava() {
        let hill = |x: i32, y: i32, z: i32| world(x, y, z).map(|b| if x >= 8 && y == 10 { 1 } else { b });
        let p = find(&hill, (5, 10, 5), (12, 11, 5), ONE, 40.0).unwrap();
        assert_eq!(*p.last().unwrap(), (12, 11, 5), "up onto the one-block rise");
        let pit = |x: i32, y: i32, z: i32| world(x, y, z).map(|b| if x >= 8 && (5..=9).contains(&y) { 0 } else { b });
        assert!(find(&pit, (5, 10, 5), (12, 5, 5), ONE, 40.0).map_or(true, |p| *p.last().unwrap() != (12, 5, 5)), "a 5 block drop is no step");
        let ledge = |x: i32, y: i32, z: i32| world(x, y, z).map(|b| if x >= 8 && (7..=9).contains(&y) { 0 } else { b });
        assert_eq!(*find(&ledge, (5, 10, 5), (12, 7, 5), ONE, 40.0).unwrap().last().unwrap(), (12, 7, 5), "a 3 block drop is");
        let lava = |x: i32, y: i32, z: i32| world(x, y, z).map(|b| if y == 9 && x == 7 { 10 } else { b });
        let p = find(&lava, (5, 10, 5), (9, 10, 5), ONE, 40.0).unwrap_or_default();
        assert!(p.iter().all(|&(x, y, _)| !(x == 7 && y == 10)), "no walking over lava: {p:?}");
    }

    /// Walled in, it gets as near as it can; a target already under it is a one-cell path; a 2 wide mob needs a 2 wide gap.
    #[test]
    fn gets_as_near_as_it_can_and_respects_width() {
        let cage = |x: i32, y: i32, z: i32| world(x, y, z).map(|b| if (x, z) != (5, 5) && (4..=6).contains(&x) && (4..=6).contains(&z) && y >= 10 && y <= 12 { 1 } else { b });
        let p = find(&cage, (5, 10, 5), (15, 10, 5), ONE, 40.0);
        assert!(p.is_none(), "caged: no step brings it closer");
        assert_eq!(find(&world, (3, 10, 3), (3, 10, 3), ONE, 40.0).unwrap(), vec![(3, 10, 3)]);
        let slit = |x: i32, y: i32, z: i32| world(x, y, z).map(|b| if x == 10 && z >= 16 && (10..=12).contains(&y) { 1 } else { b });
        assert_eq!(find(&slit, (5, 10, 5), (15, 10, 5), ONE, 40.0).unwrap().last(), Some(&(15, 10, 5)), "one cell wide fits the gap at z = 15");
        let wide = find(&slit, (5, 10, 5), (15, 10, 5), (2, 2, 2), 40.0).unwrap_or_default();
        assert!(wide.last() != Some(&(15, 10, 5)), "no way through for a mob that is two cells wide");
    }
}
