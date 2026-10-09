//! Survival digging: `Block.blockStrength` and the `PlayerControllerSP` damage counter. A block breaks once the
//! damage added per game tick (20 per second) sums to 1. Whether the held item may harvest the block
//! (`InventoryPlayer.canHarvestBlock`) sets the dig speed here and decides in `lib.rs` whether it drops anything
//! (`world::items` spawns the drops; stone without a pickaxe breaks and drops nothing, like the original).

use crate::world::craft;
use crate::world::items::ItemStack;

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

/// `Material.getIsHarvestable`: false for rock, iron, snow, snow block and web (the materials built with
/// `setNoHarvest`). Those need the right tool to drop anything and dig 3.3x slower without it.
pub fn harvestable_by_hand(id: u8) -> bool {
    !matches!(id, 1 | 4 | 7 | 14..=16 | 21..=24 | 30 | 41 | 42 | 43..=45 | 48 | 49 | 52 | 56 | 57 | 61 | 62 | 67 | 70 | 71 | 73 | 74 | 78 | 80 | 87 | 89)
}

/// Material rock or iron: the no-harvest set minus snow (78), snow block (80) and web (30). A pickaxe harvests these.
pub fn rock_or_iron(id: u8) -> bool {
    !harvestable_by_hand(id) && !matches!(id, 30 | 78 | 80)
}

/// `InventoryPlayer.canHarvestBlock`: the block's material allows it, or the held item does.
pub fn can_harvest(id: u8, held: Option<ItemStack>) -> bool {
    harvestable_by_hand(id) || held.is_some_and(|s| craft::tool_can_harvest(s.id, id))
}

/// `Block.blockStrength` per tick with `held` in hand; a fall (not on the ground) or a head under water
/// divides the speed by 5 each (`EntityPlayer.getCurrentPlayerStrVsBlock`). Infinite for hardness 0.
pub fn strength(id: u8, held: Option<ItemStack>, on_ground: bool, in_water: bool) -> f32 {
    let h = hardness(id);
    if h < 0.0 {
        return 0.0;
    }
    if !can_harvest(id, held) {
        return 1.0 / h / 100.0;
    }
    let mut speed = held.map_or(1.0, |s| craft::str_vs_block(s.id, id));
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
    pub fn tick(&mut self, pos: (i32, i32, i32), id: u8, held: Option<ItemStack>, on_ground: bool, in_water: bool) -> bool {
        if self.wait > 0 {
            self.wait -= 1;
            return false;
        }
        if id == 0 {
            return false;
        }
        let s = strength(id, held, on_ground, in_water);
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
        ticks_with(id, None, max)
    }

    fn ticks_with(id: u8, held: Option<ItemStack>, max: u32) -> Option<u32> {
        let mut d = Dig::default();
        (1..=max).find(|_| d.tick((0, 0, 0), id, held, true, false))
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
        assert!((strength(3, None, false, false) * 5.0 - strength(3, None, true, false)).abs() < 1e-6);
        assert!((strength(3, None, false, true) * 25.0 - strength(3, None, true, false)).abs() < 1e-5);
    }

    /// `canHarvestBlock` gate and tool speed: stone drops nothing by hand, a pickaxe digs it 2x (wood) to 6x (iron)
    /// faster than the 1/100 hand rate, a shovel on dirt is 2x, a pickaxe on dirt is no faster than a hand.
    #[test]
    fn tools_gate_harvest_and_speed_up_digging() {
        let held = |id| Some(ItemStack { id, count: 1, damage: 0 });
        assert!(!can_harvest(1, None) && can_harvest(1, held(270)) && !can_harvest(1, held(271)));
        assert!(can_harvest(3, None) && can_harvest(17, None), "dirt and logs by hand");
        assert!(!can_harvest(56, held(274)) && can_harvest(56, held(257)), "diamond ore: stone no, iron yes");
        assert!(!can_harvest(30, held(270)) && can_harvest(30, held(268)) && can_harvest(30, held(359)), "web: sword or shears");
        // stone, hardness 1.5: hand 1/1.5/100 = 150 ticks; wooden pickaxe 2/1.5/30 -> 23; iron 6/1.5/30 -> 8.
        assert!(matches!(ticks_with(1, held(270), 400), Some(23..=25)));
        assert!(matches!(ticks_with(1, held(257), 400), Some(8..=10)));
        // dirt, hardness 0.5: hand 15 ticks; iron shovel 6x -> 3 ticks; pickaxe on dirt = hand.
        assert!(matches!(ticks_with(3, held(256), 100), Some(3..=5)));
        assert!(matches!(ticks_with(3, held(257), 100), Some(16..=17)));
        // Stone with the wrong tool (an axe) is still the slow hand rate.
        assert!(matches!(ticks_with(1, held(271), 400), Some(151..=152)));
    }

    #[test]
    fn retargeting_resets_and_a_break_waits_five_ticks() {
        let mut d = Dig::default();
        for _ in 0..8 {
            d.tick((0, 0, 0), 3, None, true, false);
        }
        assert!(d.damage > 0.2);
        d.tick((1, 0, 0), 3, None, true, false); // new block: back to zero
        assert_eq!(d.damage, 0.0);
        // After a break the next 5 ticks do nothing, even on an instant block.
        assert!(d.tick((2, 0, 0), 37, None, true, false));
        assert!((0..5).all(|_| !d.tick((3, 0, 0), 37, None, true, false)));
        assert!(d.tick((3, 0, 0), 37, None, true, false));
    }
}
