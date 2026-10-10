//! Chest storage: `TileEntityChest` (27 slots), `BlockChest` (what may be placed, what opens, which tile a face shows) and
//! `InventoryLargeChest` (chests side by side are one inventory, upper half first).
//!
//! The Java keeps no facing: `BlockChest.getBlockTexture` derives the front and the large-chest halves from the
//! neighbours every time it draws, so `tile` does too (the mesher calls it) and nothing is stored in metadata.

use glam::Vec3;

use crate::world::items::ItemStack;

pub type Pos = (i32, i32, i32);
/// `Block.chest`.
pub const ID: u8 = 54;
/// `TileEntityChest.getSizeInventory` (the Java array is 36 long, the last 9 are never read, written or saved).
pub const SIZE: usize = 27;
pub type Chest = [Option<ItemStack>; SIZE];
type Get<'a> = &'a dyn Fn(i32, i32, i32) -> u8;

/// The horizontal neighbours in the order the Java reads them: -X, +X, -Z, +Z (even index = lines up before, odd = after).
const SIDE: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

/// `Block.opaqueCubeLookup`: every id keeps `Block.isOpaqueCube() == true` except the ones the Java overrides to false
/// (plants, fluids, rails, doors, torches, stairs, slabs, glass, ice, leaves with fancy graphics, ...).
// UNVERIFIED: `World.isBlockNormalCube` also asks the material; it is taken as the same table here (the cube-like blocks agree).
pub const fn opaque(id: u8) -> bool {
    !matches!(id, 0 | 6 | 8..=11 | 18 | 20 | 26..=34 | 36..=40 | 44 | 50..=53 | 55 | 59 | 60 | 63..=72 | 75..=79 | 81 | 83 | 85 | 90 | 92..=94 | 96)
}

/// `BlockChest.canPlaceBlockAt` for the empty cell `p`: at most one chest next to it, and that one has no chest beside it yet
/// (a chest is never part of a row of three, nor of a 2x2).
pub fn can_place(get: Get, p: Pos) -> bool {
    let chests = |(x, y, z): Pos| SIDE.iter().filter(|d| get(x + d.0, y, z + d.1) == ID).count();
    chests(p) <= 1 && SIDE.iter().all(|d| {
        let n = (p.0 + d.0, p.1, p.2 + d.1);
        get(n.0, n.1, n.2) != ID || chests(n) == 0
    })
}

/// `BlockChest.blockActivated`: the chests that make up the inventory opened by tapping `p`, upper half first
/// (`InventoryLargeChest(upper, lower)`: a chest at -X / -Z goes in front of `p`, one at +X / +Z behind it). `None` when a
/// normal cube sits on `p` or on its other half: nothing opens.
pub fn open(get: Get, p: Pos) -> Option<Vec<Pos>> {
    let covered = |(x, y, z): Pos| opaque(get(x, y + 1, z));
    if covered(p) {
        return None;
    }
    let mut group = vec![p];
    for (i, d) in SIDE.iter().enumerate() {
        let n = (p.0 + d.0, p.1, p.2 + d.1);
        if get(n.0, n.1, n.2) != ID {
            continue;
        }
        if covered(n) {
            return None;
        }
        if i % 2 == 0 { group.insert(0, n) } else { group.push(n) }
    }
    Some(group)
}

/// `canInteractWith` of every half (`InventoryLargeChest` needs both): still chests, and the eye within 8 blocks of each centre.
/// `EntityPlayer.onUpdate` closes the screen the tick this turns false.
pub fn usable(get: Get, eye: Vec3, group: &[Pos]) -> bool {
    group.iter().all(|&(x, y, z)| get(x, y, z) == ID && (eye - Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5)).length_squared() <= 64.0)
}

/// `BlockChest.getBlockTexture(world, x, y, z, side)`: the `terrain.png` tile of face `side` (0 bottom, 1 top, 2 -Z, 3 +Z,
/// 4 -X, 5 +X). Single chests show their front (27) toward the open side, away from walls; large chests use 41 / 42 for the
/// two halves of the front and 57 / 58 for the back, with 26 on the short ends.
// ponytail: the cells diagonal to `p` come from the mesher's one-cell border, which has no diagonal chunks, so at a chunk
// corner they read the neighbour's wrong column; it only decides the front of a large chest touching a wall there.
pub fn tile(get: Get, (x, y, z): Pos, side: u8) -> u8 {
    const BASE: i32 = 26;
    if side <= 1 {
        return (BASE - 1) as u8;
    }
    let (n, s, w, e) = (get(x, y, z - 1), get(x, y, z + 1), get(x - 1, y, z), get(x + 1, y, z));
    // A cell seen by the Java as opaque and its two neighbours' opposites, as in `(o(a) || o(b)) && !o(c) && !o(d)`.
    let wall = |a: bool, b: bool, c: bool, d: bool| (a || b) && !c && !d;
    let side = side as i32;
    let t = if n != ID && s != ID {
        if w != ID && e != ID {
            // Single chest: the front is +Z unless a wall is on that side only.
            let mut front = 3;
            if wall(opaque(s), false, opaque(n), false) { front = 2 }
            if wall(opaque(w), false, opaque(e), false) { front = 5 }
            if wall(opaque(e), false, opaque(w), false) { front = 4 }
            if side == front { BASE + 1 } else { BASE }
        } else if side != 4 && side != 5 {
            // Large chest along X: this half is left (-1) or right (0) as seen from +Z, mirrored as seen from -Z.
            let nx = if w == ID { x - 1 } else { x + 1 };
            let (pn, ps) = (opaque(get(nx, y, z - 1)), opaque(get(nx, y, z + 1)));
            let mut off = if w == ID { -1 } else { 0 };
            if side == 3 { off = -1 - off }
            let mut front = 3;
            if wall(opaque(s), ps, opaque(n), pn) { front = 2 }
            (if side == front { BASE + 16 } else { BASE + 32 }) + off
        } else {
            BASE
        }
    } else if side != 2 && side != 3 {
        // Large chest along Z.
        let nz = if n == ID { z - 1 } else { z + 1 };
        let (pw, pe) = (opaque(get(x - 1, y, nz)), opaque(get(x + 1, y, nz)));
        let mut off = if n == ID { -1 } else { 0 };
        if side == 4 { off = -1 - off }
        let mut front = 5;
        if wall(opaque(e), pe, opaque(w), pw) { front = 4 }
        (if side == front { BASE + 16 } else { BASE + 32 }) + off
    } else {
        BASE
    };
    t as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn world(cells: &[(Pos, u8)]) -> impl Fn(i32, i32, i32) -> u8 {
        let m: HashMap<Pos, u8> = cells.iter().copied().collect();
        move |x, y, z| m.get(&(x, y, z)).copied().unwrap_or(0)
    }

    /// Placement (`canPlaceBlockAt`), which chests open together and in which order, the block on top, and the tiles
    /// `getBlockTexture` hands out for single and large chests.
    #[test]
    fn chests_follow_the_java() {
        // One chest at x = 0: a second may go beside it, but not on the far side of a pair.
        let one = world(&[((0, 64, 0), ID)]);
        assert!(can_place(&one, (1, 64, 0)) && can_place(&one, (0, 64, 1)) && can_place(&one, (5, 64, 5)));
        let two = world(&[((0, 64, 0), ID), ((1, 64, 0), ID)]);
        assert!(!can_place(&two, (2, 64, 0)) && !can_place(&two, (-1, 64, 0)));
        assert!(!can_place(&two, (1, 64, 1)), "a third chest beside either half");
        let diag = world(&[((0, 64, 0), ID), ((2, 64, 0), ID)]);
        assert!(!can_place(&diag, (1, 64, 0)), "two neighbours");

        // The group is upper half first: -X / -Z before the tapped chest, +X / +Z after it.
        assert_eq!(open(&two, (0, 64, 0)), Some(vec![(0, 64, 0), (1, 64, 0)]));
        assert_eq!(open(&two, (1, 64, 0)), Some(vec![(0, 64, 0), (1, 64, 0)]));
        let z = world(&[((0, 64, 0), ID), ((0, 64, 1), ID)]);
        assert_eq!(open(&z, (0, 64, 1)), Some(vec![(0, 64, 0), (0, 64, 1)]));
        assert_eq!(open(&one, (0, 64, 0)), Some(vec![(0, 64, 0)]));
        // A normal cube above either half blocks it; glass does not.
        assert_eq!(open(&world(&[((0, 64, 0), ID), ((0, 65, 0), 1)]), (0, 64, 0)), None);
        assert_eq!(open(&world(&[((0, 64, 0), ID), ((1, 64, 0), ID), ((1, 65, 0), 4)]), (0, 64, 0)), None);
        assert!(open(&world(&[((0, 64, 0), ID), ((0, 65, 0), 20)]), (0, 64, 0)).is_some());
        // Reach: 8 blocks from the centre of every half, and the block must still be a chest.
        let eye = Vec3::new(0.5, 65.5, 0.5);
        assert!(usable(&one, eye + Vec3::new(7.9, 0.0, 0.0), &[(0, 64, 0)]) && !usable(&one, eye + Vec3::new(8.1, 0.0, 0.0), &[(0, 64, 0)]));
        assert!(!usable(&world(&[]), eye, &[(0, 64, 0)]));

        // Single chest: top/bottom 25, front 27 on +Z in the open, on -Z with a wall behind it on +Z, sides 26.
        assert_eq!([0, 1, 2, 3, 4, 5].map(|s| tile(&one, (0, 64, 0), s)), [25, 25, 26, 27, 26, 26]);
        let wall = world(&[((0, 64, 0), ID), ((0, 64, 1), 1)]);
        assert_eq!([2, 3].map(|s| tile(&wall, (0, 64, 0), s)), [27, 26]);
        let wall_w = world(&[((0, 64, 0), ID), ((-1, 64, 0), 1)]);
        assert_eq!(tile(&wall_w, (0, 64, 0), 5), 27, "wall on -X: the front looks +X");
        // Large chest along X, seen from +Z: left half 41, right half 42; from -Z the back 58 | 57; the short ends are 26.
        assert_eq!([(0, 3), (1, 3), (0, 2), (1, 2), (0, 4), (1, 5)].map(|(cx, s)| tile(&two, (cx, 64, 0), s)), [41, 42, 58, 57, 26, 26]);
        // Along Z: (z 0, side 5) 42, (z 1, side 5) 41, (z 0, side 4) 57, (z 1, side 4) 58; the short ends are 26.
        assert_eq!([(0, 5), (1, 5), (0, 4), (1, 4), (0, 2), (1, 3)].map(|(cz, s)| tile(&z, (0, 64, cz), s)), [42, 41, 57, 58, 26, 26]);
        // Every tile stays inside terrain.png.
        for s in 0..6 {
            assert!(tile(&two, (0, 64, 0), s) < 64 && tile(&z, (0, 64, 0), s) < 64);
        }
    }
}
