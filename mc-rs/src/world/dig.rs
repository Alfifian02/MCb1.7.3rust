//! Survival digging, bare-handed: `Block.blockStrength` and the `PlayerControllerSP` damage counter.
//! A block breaks once the damage added per game tick (20 per second) sums to 1. No tools yet, so
//! "harvestable" only depends on the block's material and only sets the dig speed here; what a broken block
//! drops is `world::items` (which, until tools exist, lets bare hands harvest everything).

/// `Block.blockHardness`; negative = unbreakable. Values are the `setHardness` calls in Block.java.
pub fn hardness(id: u8) -> f32 {
    match id {
        7 | 90 => -1.0,
        0 | 6 | 10 | 31 | 32 | 37..=40 | 46 | 50 | 51 | 55 | 59 | 75 | 76 | 93 | 94 | 95 => 0.0,
        78 => 0.1,
        18 | 26 | 80 => 0.2,
        20 | 89 => 0.3,
        65 | 81 | 87 => 0.4,
        3 | 12 | 69 | 70 | 72 | 77 | 79 | 88 | 92 => 0.5,
        2 | 13 | 19 | 60 | 82 => 0.6,
        27 | 28 | 66 => 0.7,
        24 | 25 => 0.8,
        63 | 68 | 86 | 91 => 1.0,
        1 | 47 => 1.5,
        4 | 5 | 17 | 43 | 44 | 45 | 48 | 84 | 85 => 2.0,
        54 | 58 => 2.5,
        14 | 15 | 16 | 21 | 22 | 41 | 56 | 64 | 73 | 74 | 96 => 3.0,
        23 | 61 | 62 => 3.5,
        30 => 4.0,
        42 | 52 | 57 | 71 => 5.0,
        49 => 10.0,
        8 | 9 | 11 => 100.0,
        _ => 2.0, // UNVERIFIED: ids with no entry (not generated, no Block.java hardness read for them)
    }
}

/// `Material.getIsHarvestable` without a tool: false for rock, iron, snow, snow block and web
/// (the materials built with `setNoHarvest`). Those dig 3.3x slower by hand and drop nothing.
pub fn harvestable_by_hand(id: u8) -> bool {
    !matches!(id, 1 | 4 | 7 | 14..=16 | 21..=24 | 30 | 41 | 42 | 43..=45 | 48 | 49 | 52 | 56 | 57 | 61 | 62 | 70 | 71 | 73 | 74 | 78 | 80 | 87 | 89)
}

/// `Block.blockStrength` per tick for a bare hand; a fall (not on the ground) or a head under water
/// divides the speed by 5 each (`EntityPlayer.getCurrentPlayerStrVsBlock`). Infinite for hardness 0.
pub fn strength(id: u8, on_ground: bool, in_water: bool) -> f32 {
    let h = hardness(id);
    if h < 0.0 {
        return 0.0;
    }
    if !harvestable_by_hand(id) {
        return 1.0 / h / 100.0;
    }
    let mut speed = 1.0;
    if in_water {
        speed /= 5.0;
    }
    if !on_ground {
        speed /= 5.0;
    }
    speed / h / 30.0
}

/// Seconds per game tick.
pub const TICK: f32 = 0.05;

/// `PlayerControllerSP` digging state.
#[derive(Default)]
pub struct Dig {
    target: Option<(i32, i32, i32)>,
    /// Progress 0..1 of the block being dug (`curBlockDamage`).
    pub damage: f32,
    /// Ticks to wait after a block broke (`blockHitWait`, 5).
    wait: u32,
}

impl Dig {
    /// `resetBlockRemoving`: the dig input was released.
    pub fn reset(&mut self) {
        self.damage = 0.0;
        self.wait = 0;
    }

    /// One game tick with the dig input held on block `id` at `pos`; true when it breaks.
    /// Aiming at a new block restarts the damage; a block that is instant (`strength >= 1`) breaks at once,
    /// like `clickBlock`.
    pub fn tick(&mut self, pos: (i32, i32, i32), id: u8, on_ground: bool, in_water: bool) -> bool {
        if self.wait > 0 {
            self.wait -= 1;
            return false;
        }
        if id == 0 {
            return false;
        }
        let s = strength(id, on_ground, in_water);
        if self.target != Some(pos) {
            self.target = Some(pos);
            self.damage = 0.0;
            if s < 1.0 {
                return false;
            }
        } else {
            self.damage += s;
        }
        if self.damage >= 1.0 || s >= 1.0 {
            self.damage = 0.0;
            self.wait = 5;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ticks until `id` breaks when held on one cell, or None if it never does within `max`.
    fn ticks_to_break(id: u8, max: u32) -> Option<u32> {
        let mut d = Dig::default();
        (1..=max).find(|_| d.tick((0, 0, 0), id, true, false))
    }

    #[test]
    fn dig_times_match_the_java_formulas() {
        // dirt: 0.5 hardness, 1/0.5/30 per tick -> 15 ticks (+1 to lock the target).
        assert!(matches!(ticks_to_break(3, 100), Some(16..=17)));
        // stone by hand: not harvestable, 1/1.5/100 per tick -> 150 ticks.
        assert!(matches!(ticks_to_break(1, 400), Some(151..=152)));
        // flowers (hardness 0) and bedrock (-1).
        assert_eq!(ticks_to_break(37, 10), Some(1));
        assert_eq!(ticks_to_break(7, 5000), None);
        // In the air or under water: 5x slower; both: 25x.
        assert!((strength(3, false, false) * 5.0 - strength(3, true, false)).abs() < 1e-6);
        assert!((strength(3, false, true) * 25.0 - strength(3, true, false)).abs() < 1e-5);
    }

    #[test]
    fn retargeting_resets_and_a_break_waits_five_ticks() {
        let mut d = Dig::default();
        for _ in 0..8 {
            d.tick((0, 0, 0), 3, true, false);
        }
        assert!(d.damage > 0.2);
        d.tick((1, 0, 0), 3, true, false); // new block: back to zero
        assert_eq!(d.damage, 0.0);
        // After a break the next 5 ticks do nothing, even on an instant block.
        assert!(d.tick((2, 0, 0), 37, true, false));
        assert!((0..5).all(|_| !d.tick((3, 0, 0), 37, true, false)));
        assert!(d.tick((3, 0, 0), 37, true, false));
    }
}
