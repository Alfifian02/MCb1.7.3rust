//! Which faces of a 16x16x16 section can see each other through it (`VisibilityEncoding`, and Mojang's `VisGraph`
//! flood fill that Embeddium reads it from). A section is 36 booleans: `from` face x `to` face, packed in a `u64`
//! at bit `from * 8 + to`.

pub const DOWN: usize = 0;
pub const UP: usize = 1;
pub const NORTH: usize = 2;
pub const SOUTH: usize = 3;
pub const WEST: usize = 4;
pub const EAST: usize = 5;

/// A set of directions, bit `d` = direction `d`.
pub const ALL_DIRS: u8 = 0b11_1111;
pub const OPPOSITE: [usize; 6] = [UP, DOWN, SOUTH, NORTH, EAST, WEST];
/// Step to the neighbouring section in each direction.
pub const STEP: [[i32; 3]; 6] = [[0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]];

/// Every pair connected: an empty section, or one that is not built yet (never hides anything).
pub const FULL: u64 = 0x3F3F_3F3F_3F3F;

pub const fn bit(from: usize, to: usize) -> u64 {
    1 << (from * 8 + to)
}

/// Outgoing directions reachable from any of the `incoming` faces (`getConnections(data, incoming)`).
pub fn connections(vis: u64, incoming: u8) -> u8 {
    let mut mask = 0u64;
    for d in 0..6 {
        if incoming >> d & 1 != 0 {
            mask |= 0xFF << (d * 8);
        }
    }
    fold(vis & mask)
}

/// Outgoing directions reachable from anywhere (`getConnections(data)`): for the section the camera is in.
pub fn connections_any(vis: u64) -> u8 {
    fold(vis)
}

fn fold(mut v: u64) -> u8 {
    v |= v >> 32;
    v |= v >> 16;
    v |= v >> 8;
    (v & ALL_DIRS as u64) as u8
}

/// Flood-fills the non-solid cells of one section; every connected pocket links all the faces it touches.
/// `solid(x, y, z)` (each 0..16) must say whether a cell stops the view: only full, light-blocking cubes, or terrain
/// shows through where it should not. Few solid cells cannot wall off a face (that takes 256), so they short-cut to `FULL`.
pub fn compute(solid: impl Fn(usize, usize, usize) -> bool) -> u64 {
    let mut wall = [0u64; 64]; // 4096 bits, index x | y << 4 | z << 8
    let mut count = 0;
    for i in 0..4096usize {
        if solid(i & 15, i >> 4 & 15, i >> 8) {
            wall[i >> 6] |= 1 << (i & 63);
            count += 1;
        }
    }
    if count < 256 {
        return FULL;
    }
    let mut seen = wall; // solid cells count as visited
    let mut stack: Vec<u16> = Vec::new();
    let mut out = 0u64;
    for start in 0..4096usize {
        if seen[start >> 6] >> (start & 63) & 1 != 0 {
            continue;
        }
        seen[start >> 6] |= 1 << (start & 63);
        stack.push(start as u16);
        let mut faces = 0u8;
        while let Some(i) = stack.pop() {
            let i = i as usize;
            let (x, y, z) = (i & 15, i >> 4 & 15, i >> 8);
            faces |= match x { 0 => 1 << WEST, 15 => 1 << EAST, _ => 0 };
            faces |= match y { 0 => 1 << DOWN, 15 => 1 << UP, _ => 0 };
            faces |= match z { 0 => 1 << NORTH, 15 => 1 << SOUTH, _ => 0 };
            let mut go = |n: usize| {
                if seen[n >> 6] >> (n & 63) & 1 == 0 {
                    seen[n >> 6] |= 1 << (n & 63);
                    stack.push(n as u16);
                }
            };
            if x > 0 { go(i - 1) }
            if x < 15 { go(i + 1) }
            if y > 0 { go(i - 16) }
            if y < 15 { go(i + 16) }
            if z > 0 { go(i - 256) }
            if z < 15 { go(i + 256) }
        }
        for a in 0..6 {
            for b in 0..6 {
                if faces >> a & 1 != 0 && faces >> b & 1 != 0 {
                    out |= bit(a, b);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_solid() {
        assert_eq!(compute(|_, _, _| false), FULL);
        assert_eq!(compute(|_, _, _| true), 0);
    }

    /// A full wall at x = 8 splits west from east; a one-cell tunnel along x joins only west and east.
    #[test]
    fn wall_and_tunnel() {
        let wall = compute(|x, _, _| x == 8);
        assert_ne!(wall & bit(WEST, WEST), 0);
        assert_ne!(wall & bit(UP, DOWN), 0);
        assert_eq!(wall & bit(WEST, EAST), 0);
        assert_eq!(wall & bit(EAST, WEST), 0);

        let tunnel = compute(|_, y, z| !(y == 5 && z == 5));
        assert_eq!(connections(tunnel, 1 << WEST), 1 << WEST | 1 << EAST);
        assert_eq!(connections(tunnel, 1 << UP), 0);
        assert_eq!(connections_any(tunnel), 1 << WEST | 1 << EAST);
    }
}
