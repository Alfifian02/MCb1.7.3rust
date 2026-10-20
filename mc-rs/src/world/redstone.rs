//! M21 power queries: `World.isBlockProvidingPowerTo` / `isBlockGettingPowered` / `isBlockIndirectlyProvidingPowerTo` /
//! `isBlockIndirectlyGettingPowered` and the `isPoweringTo` (weak) / `isIndirectlyPoweringTo` (strong) of wire (55), lever (69),
//! stone / wooden plate (70 / 72), redstone torch (75 off, 76 on) and button (77). Pure reads of the world; the blocks that act
//! on them live in `world::ticks`. Sides are the Java ones: 0 -y, 1 +y, 2 -z, 3 +z, 4 -x, 5 +x.
//! `wires` is `BlockRedstoneWire.wiresProvidePower`: false while a wire asks what powers it, so wires do not feed themselves.

use crate::world::chunk::normal_cube;
use crate::world::chunks::ChunkManager;

pub const SIDE: [(i32, i32, i32); 6] = [(0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)];

fn get(w: &ChunkManager, x: i32, y: i32, z: i32) -> u8 {
    w.block_loaded(x, y, z).unwrap_or(0)
}

fn nc(w: &ChunkManager, x: i32, y: i32, z: i32) -> bool {
    normal_cube(get(w, x, y, z))
}

/// `Block.canProvidePower`.
pub fn can_provide_power(id: u8) -> bool {
    matches!(id, 28 | 55 | 69 | 70 | 72 | 75 | 76 | 77)
}

/// `BlockRedstoneTorch.isPoweringTo` of a lit torch: everywhere but toward the block it hangs on (metadata 1 -x wall, 2 +x, 3 -z, 4 +z, 5 floor).
fn torch_powers(m: u8, side: u8) -> bool {
    !matches!((m, side), (5, 1) | (3, 3) | (4, 2) | (1, 5) | (2, 4))
}

/// `Block.isPoweringTo`: the power a block gives its neighbours that are components (wire, torch, repeater...).
pub fn weak(w: &ChunkManager, x: i32, y: i32, z: i32, side: u8, wires: bool) -> bool {
    let m = w.meta(x, y, z);
    match get(w, x, y, z) {
        55 => wires && m > 0 && wire_powers(w, x, y, z, side),
        28 | 69 | 77 => m & 8 != 0, // `BlockDetectorRail.isPoweringTo`: on while a cart is over it
        70 | 72 => m > 0,
        76 => torch_powers(m, side),
        94 => repeater_powers(m, side),
        _ => false,
    }
}

/// `BlockRedstoneRepeater.isPoweringTo` of a repeater that is on: out of its front only.
fn repeater_powers(m: u8, side: u8) -> bool {
    matches!((m & 3, side), (0, 3) | (1, 4) | (2, 2) | (3, 5))
}

/// `Block.isIndirectlyPoweringTo`: the power a block gives the (solid) block it points at.
pub fn strong(w: &ChunkManager, x: i32, y: i32, z: i32, side: u8, wires: bool) -> bool {
    let m = w.meta(x, y, z);
    match get(w, x, y, z) {
        55 => weak(w, x, y, z, side, wires),
        69 => m & 8 != 0 && matches!((m & 7, side), (5 | 6, 1) | (4, 2) | (3, 3) | (2, 4) | (1, 5)),
        77 => m & 8 != 0 && matches!((m & 7, side), (5, 1) | (4, 2) | (3, 3) | (2, 4) | (1, 5)),
        70 | 72 => m > 0 && side == 1,
        28 => m & 8 != 0 && side == 1, // `isIndirectlyPoweringTo`: the block above
        76 => side == 0 && torch_powers(m, 0),
        94 => repeater_powers(m, side),
        _ => false,
    }
}

/// `BlockRedstoneWire.isPowerProviderOrWire`: `dir` is the Java direction the wire looks along (0 +z, 1 -x, 2 -z, 3 +x) or -1 for a
/// neighbour one step up or down, where only another wire counts; a repeater counts along its own axis.
fn provider_or_wire(w: &ChunkManager, x: i32, y: i32, z: i32, dir: i8) -> bool {
    match get(w, x, y, z) {
        0 => false,
        55 => true,
        93 | 94 => {
            let f = (w.meta(x, y, z) & 3) as usize;
            dir == f as i8 || dir == [2, 3, 0, 1][f]
        }
        id => can_provide_power(id) && dir != -1,
    }
}

/// `BlockRedstoneWire.isPoweringTo` for a wire with a level: it points along the lines it connects to (all four sides when it connects
/// to nothing), always up.
fn wire_powers(w: &ChunkManager, x: i32, y: i32, z: i32, side: u8) -> bool {
    if side == 1 {
        return true;
    }
    let p = |a, b, c, d| provider_or_wire(w, a, b, c, d);
    // (west, east, north, south) = the neighbours at -x, +x, -z, +z, looked at along Java directions 1, 3, 2, 0
    let around = [(-1, 0, 1), (1, 0, 3), (0, -1, 2), (0, 1, 0)];
    let mut c = around.map(|(dx, dz, d)| p(x + dx, y, z + dz, d) || (!nc(w, x + dx, y, z + dz) && p(x + dx, y - 1, z + dz, -1)));
    if !nc(w, x, y + 1, z) {
        for (i, (dx, dz, _)) in around.into_iter().enumerate() {
            c[i] |= nc(w, x + dx, y, z + dz) && p(x + dx, y + 1, z + dz, -1);
        }
    }
    let [west, east, north, south] = c;
    let none = !west && !east && !north && !south;
    (none && (2..=5).contains(&side)) || (side == 2 && north && !west && !east) || (side == 3 && south && !west && !east) || (side == 4 && west && !north && !south) || (side == 5 && east && !north && !south)
}

/// `World.isBlockGettingPowered`: some neighbour strongly powers this cell.
pub fn getting_powered(w: &ChunkManager, x: i32, y: i32, z: i32, wires: bool) -> bool {
    SIDE.iter().enumerate().any(|(s, (dx, dy, dz))| strong(w, x + dx, y + dy, z + dz, s as u8, wires))
}

/// `World.isBlockIndirectlyProvidingPowerTo`: a solid block passes on what powers it, anything else gives its own weak power.
pub fn indirect(w: &ChunkManager, x: i32, y: i32, z: i32, side: u8, wires: bool) -> bool {
    if nc(w, x, y, z) { getting_powered(w, x, y, z, wires) } else { weak(w, x, y, z, side, wires) }
}

/// `World.isBlockIndirectlyGettingPowered`.
pub fn indirectly_powered(w: &ChunkManager, x: i32, y: i32, z: i32, wires: bool) -> bool {
    SIDE.iter().enumerate().any(|(s, (dx, dy, dz))| indirect(w, x + dx, y + dy, z + dz, s as u8, wires))
}

/// The level (0..=15) a wire at the cell settles at (`BlockRedstoneWire.func_21030_a`): 15 when something other than a wire powers it,
/// else the strongest wire beside it (one step up over a solid neighbour, one down past an open one) minus one.
pub fn wire_level(w: &ChunkManager, x: i32, y: i32, z: i32) -> u8 {
    if indirectly_powered(w, x, y, z, false) {
        return 15;
    }
    let lv = |a, b, c| if get(w, a, b, c) == 55 { w.meta(a, b, c) } else { 0 };
    let mut best = 0;
    for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
        let (a, c) = (x + dx, z + dz);
        best = best.max(lv(a, y, c));
        if nc(w, a, y, c) {
            if !nc(w, x, y + 1, z) {
                best = best.max(lv(a, y + 1, c));
            }
        } else {
            best = best.max(lv(a, y - 1, c));
        }
    }
    best.saturating_sub(1)
}

/// What a repeater reads (`BlockRedstoneRepeater.func_22022_g`): the cell behind it is a power source, or a live wire.
pub fn repeater_input(w: &ChunkManager, x: i32, y: i32, z: i32) -> bool {
    let (dx, dz, side) = [(0, 1, 3), (-1, 0, 4), (0, -1, 2), (1, 0, 5)][(w.meta(x, y, z) & 3) as usize];
    let (a, c) = (x + dx, z + dz);
    indirect(w, a, y, c, side, true) || (get(w, a, y, c) == 55 && w.meta(a, y, c) > 0)
}

/// What a redstone torch reads (`BlockRedstoneTorch.func_30002_h`): the block it hangs on is powered from another side.
pub fn torch_input(w: &ChunkManager, x: i32, y: i32, z: i32) -> bool {
    match w.meta(x, y, z) {
        5 => indirect(w, x, y - 1, z, 0, true),
        3 => indirect(w, x, y, z - 1, 2, true),
        4 => indirect(w, x, y, z + 1, 3, true),
        1 => indirect(w, x - 1, y, z, 4, true),
        2 => indirect(w, x + 1, y, z, 5, true),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> ChunkManager {
        let mut m = ChunkManager::new(1, 1);
        m.insert((0, 0), vec![0u8; crate::world::chunk::VOLUME]);
        (0..16).for_each(|x| (0..16).for_each(|z| { m.set_block(x, 39, z, 1); }));
        m
    }

    /// A lever on the floor powers the wire next to it with 15, the wire line loses one a block, a torch inverts a powered block.
    #[test]
    fn lever_wire_and_torch_follow_java() {
        let mut m = world();
        m.set_block_meta(3, 40, 5, 69, 5 | 8); // lever on the floor, on
        assert!(weak(&m, 3, 40, 5, 0, true) && !strong(&m, 3, 40, 5, 0, true) && strong(&m, 3, 40, 5, 1, true));
        for x in 4..8 {
            m.set_block(x, 40, 5, 55);
        }
        assert_eq!(wire_level(&m, 4, 40, 5), 15, "a lever beside a wire gives it 15");
        m.set_block_meta(4, 40, 5, 55, 15);
        assert_eq!([5, 6, 7].map(|x| wire_level(&m, x, 40, 5)), [14, 0, 0], "each wire reads only the one before it so far");
        // a wire pointing at a solid block powers it (weakly), a torch on the far side of that block reads it
        m.set_block(8, 40, 5, 1);
        m.set_block_meta(7, 40, 5, 55, 3);
        assert!(indirect(&m, 8, 40, 5, 4, true) || getting_powered(&m, 8, 40, 5, true), "the wire's end powers the block it points at");
        m.set_block_meta(9, 40, 5, 76, 1); // torch on the -x wall of x = 9, hanging on the block at 8
        assert!(torch_input(&m, 9, 40, 5));
        // torches power everything but the block they hang on
        assert!(!weak(&m, 9, 40, 5, 5, true) && weak(&m, 9, 40, 5, 3, true));
    }
}
