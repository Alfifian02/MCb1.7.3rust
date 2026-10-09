//! M6 crafting + tools. Ports of `EnumToolMaterial`, `ItemTool` / `ItemPickaxe` / `ItemSpade` / `ItemSword` /
//! `ItemHoe` / `ItemShears` (`getStrVsBlock`,
//! `canHarvestBlock`), `CraftingManager` + `ShapedRecipes.matches`, and the left-click rules of
//! `Container.func_27280_a` for `ContainerPlayer` (2x2 grid), `ContainerWorkbench` (3x3 grid) and `ContainerFurnace`,
//! plus `TileEntityFurnace` and `FurnaceRecipes` (smelting).
//! Item ids follow `world::items`: below 256 = the block, from 256 up = `Item.shiftedIndex` (256 + n).

use std::sync::OnceLock;

use crate::world::items::{max_stack, Inventory, ItemStack};

// ---- Tools ----

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Shovel,
    Pickaxe,
    Axe,
    Sword,
    Hoe,
}

/// `EnumToolMaterial` in the order wood, stone, iron, diamond ("EMERALD"), gold: (harvestLevel, maxUses, efficiency).
const MATERIALS: [(u8, u16, f32); 5] = [(0, 59, 2.0), (1, 131, 4.0), (2, 250, 6.0), (3, 1561, 8.0), (0, 32, 12.0)];
/// `Item.shiftedIndex` of (shovel, pickaxe, axe, sword, hoe) per material, same order as `MATERIALS` and `KINDS`.
const TOOL_IDS: [[u16; 5]; 5] = [
    [269, 270, 271, 268, 290],
    [273, 274, 275, 272, 291],
    [256, 257, 258, 267, 292],
    [277, 278, 279, 276, 293],
    [284, 285, 286, 283, 294],
];
const KINDS: [Kind; 5] = [Kind::Shovel, Kind::Pickaxe, Kind::Axe, Kind::Sword, Kind::Hoe];
/// `Item.shears` (103): no material, 238 uses.
pub const SHEARS: u16 = 359;

/// (kind, material index) of a tool item, `None` for anything else.
pub fn tool(id: u16) -> Option<(Kind, usize)> {
    TOOL_IDS.iter().enumerate().find_map(|(m, ids)| ids.iter().position(|&i| i == id).map(|k| (KINDS[k], m)))
}

/// `Item.getMaxDamage`: durability of a tool, `None` = not damageable.
pub fn max_damage(id: u16) -> Option<u16> {
    if id == SHEARS {
        return Some(238);
    }
    tool(id).map(|(_, m)| MATERIALS[m].1)
}

/// `Item.onBlockDestroyed` wear for breaking block `b` with `held`: `ItemTool` 1, `ItemSword` 2, shears 1 on leaves
/// and web only, a hoe none (it wears when it tills instead).
pub fn wear_on_break(held: u16, b: u8) -> u16 {
    if held == SHEARS {
        return matches!(b, 18 | 30) as u16;
    }
    match tool(held) {
        Some((Kind::Sword, _)) => 2,
        Some((Kind::Hoe, _)) | None => 0,
        Some(_) => 1,
    }
}

pub fn is_hoe(id: u16) -> bool {
    matches!(tool(id), Some((Kind::Hoe, _)))
}

/// `ItemHoe.onItemUse`: dirt always tills; grass only from the side or top with air above it.
pub fn can_till(block: u8, above: u8, face: u8) -> bool {
    block == 3 || (block == 2 && above == 0 && face != 0)
}

/// `blocksEffectiveAgainst` of `ItemPickaxe` / `ItemAxe` / `ItemSpade`, as block ids.
fn effective(kind: Kind, b: u8) -> bool {
    match kind {
        Kind::Pickaxe => matches!(b, 1 | 4 | 14..=16 | 21 | 22 | 24 | 41..=44 | 48 | 56 | 57 | 79 | 87),
        Kind::Axe => matches!(b, 5 | 17 | 47 | 54),
        Kind::Shovel => matches!(b, 2 | 3 | 12 | 13 | 60 | 78 | 80 | 82),
        Kind::Sword | Kind::Hoe => false,
    }
}

/// `ItemStack.getStrVsBlock` of the held item `held`: the material's efficiency on its blocks, else 1.
pub fn str_vs_block(held: u16, b: u8) -> f32 {
    if held == SHEARS {
        return match b {
            18 | 30 => 15.0,
            35 => 5.0,
            _ => 1.0,
        };
    }
    match tool(held) {
        Some((Kind::Sword, _)) => if b == 30 { 15.0 } else { 1.5 },
        Some((k, m)) if effective(k, b) => MATERIALS[m].2,
        _ => 1.0,
    }
}

/// `Item.canHarvestBlock` of the held item (the item half of `InventoryPlayer.canHarvestBlock`; the material half is
/// `dig::harvestable_by_hand`). `ItemPickaxe` gates by harvest level, `ItemSpade` takes snow, sword and shears take web, every other item says no.
pub fn tool_can_harvest(held: u16, b: u8) -> bool {
    if held == SHEARS {
        return b == 30;
    }
    let Some((kind, m)) = tool(held) else { return false };
    let level = MATERIALS[m].0;
    match kind {
        Kind::Shovel => matches!(b, 78 | 80),
        Kind::Sword => b == 30,
        Kind::Axe | Kind::Hoe => false,
        Kind::Pickaxe => match b {
            49 => level == 3,                         // obsidian
            14 | 41 | 56 | 57 | 73 | 74 => level >= 2, // gold, diamond, redstone
            15 | 21 | 22 | 42 => level >= 1,          // iron, lapis
            _ => crate::world::dig::rock_or_iron(b),
        },
    }
}

// ---- Recipes ----

/// A `ShapedRecipes`: `w` x `h` pattern, row-major, 0 = empty cell. Ingredients ignore damage (every Java ingredient
/// used here is an `Item`/`Block` with damage -1 or 0, and nothing in this port has a second subtype yet).
pub struct Recipe {
    w: usize,
    h: usize,
    cells: [u16; 9],
    out: ItemStack,
}

impl Recipe {
    /// `ShapedRecipes.matches` on a `gw` x `gw` grid of item ids: the pattern may sit anywhere in the 3x3 area
    /// (cells outside a 2x2 grid are empty), mirrored or not, and every other cell must be empty.
    fn matches(&self, grid: &[u16; 9], gw: usize) -> bool {
        let at = |x: usize, y: usize| if x < gw && y < gw { grid[y * gw + x] } else { 0 };
        let want = |x: usize, y: usize, ox: usize, oy: usize, mirror: bool| {
            let (rx, ry) = (x.wrapping_sub(ox), y.wrapping_sub(oy));
            if rx < self.w && ry < self.h {
                self.cells[ry * self.w + if mirror { self.w - 1 - rx } else { rx }]
            } else {
                0
            }
        };
        (0..=3 - self.w).any(|ox| {
            (0..=3 - self.h).any(|oy| [true, false].into_iter().any(|m| (0..3).all(|y| (0..3).all(|x| at(x, y) == want(x, y, ox, oy, m)))))
        })
    }
}

/// `CraftingManager.addRecipe` with a pattern: `key` maps pattern characters to item ids, anything else is empty.
fn shaped(rows: &[&str], key: &[(char, u16)], out: u16, n: u8) -> Recipe {
    let (h, w) = (rows.len(), rows[0].len());
    let mut cells = [0; 9];
    for (y, row) in rows.iter().enumerate() {
        for (x, c) in row.chars().enumerate() {
            cells[y * w + x] = key.iter().find(|k| k.0 == c).map_or(0, |k| k.1);
        }
    }
    Recipe { w, h, cells, out: ItemStack { id: out, count: n, damage: 0 } }
}

/// Every recipe of `RecipesTools` (pickaxe, shovel, axe, hoe x 5 materials), `RecipesWeapons` (swords), the shears
/// and the `CraftingManager` / `RecipesCrafting` ones whose result exists in this port (a block that can be placed, or
/// a tool ingredient). Not ported: bow, arrow, armor, food, rails, doors, stairs, dyes and the rest, none of which has anything to do yet. One line each to add.
// ponytail: vanilla sorts the list (`RecipeSorter`, bigger first); no two patterns here can match one grid, so the
// first match is the only match and the sort is skipped.
pub fn recipes() -> &'static [Recipe] {
    static R: OnceLock<Vec<Recipe>> = OnceLock::new();
    R.get_or_init(|| {
        const PLANKS: u16 = 5;
        const STICK: u16 = 280;
        // shovel, pickaxe, axe (RecipesTools), sword (RecipesWeapons), hoe (RecipesTools)
        const PATTERNS: [&[&str]; 5] = [&["X", "#", "#"], &["XXX", " # ", " # "], &["XX", "X#", " #"], &["X", "X", "#"], &["XX", " #", " #"]];
        let mut r = vec![
            shaped(&["#"], &[('#', 17)], PLANKS, 4),
            shaped(&["#", "#"], &[('#', PLANKS)], STICK, 4),
            shaped(&["##", "##"], &[('#', PLANKS)], 58, 1),  // workbench
            shaped(&["###", "# #", "###"], &[('#', PLANKS)], 54, 1), // chest
            shaped(&["###", "# #", "###"], &[('#', 4)], 61, 1),      // furnace
            shaped(&["X", "#"], &[('X', 263), ('#', STICK)], 50, 4),  // torch (coal)
            shaped(&["##", "##"], &[('#', 12)], 24, 1),   // sand -> sandstone
            shaped(&["##", "##"], &[('#', 332)], 80, 1),  // snowballs -> snow block
            shaped(&["##", "##"], &[('#', 337)], 82, 1),  // clay -> clay block
            shaped(&["##", "##"], &[('#', 348)], 89, 1),  // glowstone dust -> glowstone
            shaped(&["##", "##"], &[('#', 287)], 35, 1),  // string -> wool
            shaped(&["# #", " # "], &[('#', PLANKS)], 281, 4), // bowl
            shaped(&["Y", "X", "#"], &[('X', 39), ('Y', 40), ('#', 281)], 282, 1), // mushroom stew, either order
            shaped(&["Y", "X", "#"], &[('X', 40), ('Y', 39), ('#', 281)], 282, 1),
            shaped(&[" #", "# "], &[('#', 265)], SHEARS, 1), // shears
        ];
        // RecipesTools: head material per tier = planks, cobblestone, iron ingot, diamond, gold ingot.
        for (m, head) in [PLANKS, 4, 265, 264, 266].into_iter().enumerate() {
            for (k, rows) in PATTERNS.iter().enumerate() {
                r.push(shaped(rows, &[('#', STICK), ('X', head)], TOOL_IDS[m][k], 1));
            }
        }
        r
    })
}

/// `CraftingManager.findMatchingRecipe` for a `gw` x `gw` grid.
pub fn find(grid: &[Option<ItemStack>; 9], gw: usize) -> Option<ItemStack> {
    let ids = grid.map(|s| s.map_or(0, |s| s.id));
    recipes().iter().find(|r| r.matches(&ids, gw)).map(|r| r.out)
}

// ---- Furnace ----

/// `FurnaceRecipes`: what smelting `id` gives. Not ported: raw fish (no fish yet).
fn smelting(id: u16) -> Option<ItemStack> {
    let (out, damage) = match id {
        15 => (265, 0),  // iron ore -> iron ingot
        14 => (266, 0),  // gold ore -> gold ingot
        56 => (264, 0),  // diamond ore -> diamond
        12 => (20, 0),   // sand -> glass
        4 => (1, 0),     // cobblestone -> stone
        337 => (336, 0), // clay -> brick
        81 => (351, 2),  // cactus -> green dye
        17 => (263, 1),  // log -> charcoal
        319 => (320, 0), // raw -> cooked porkchop
        _ => return None,
    };
    Some(ItemStack { id: out, count: 1, damage })
}

/// `TileEntityFurnace.getItemBurnTime` in ticks: wooden blocks 300, stick and sapling 100, coal 1600.
/// Not ported: the lava bucket (20000), there are no buckets.
fn burn_time(id: u16) -> u16 {
    match id {
        5 | 17 | 25 | 47 | 53 | 54 | 58 | 63 | 64 | 68 | 72 | 84 | 85 | 95 | 96 => 300,
        280 | 6 => 100,
        263 => 1600,
        _ => 0,
    }
}

/// A `TileEntityFurnace`: 0 input, 1 fuel, 2 output. 20 Hz `tick`s, 200 ticks per item.
#[derive(Default, Clone, PartialEq, Debug)]
pub struct Furnace {
    pub slots: [Option<ItemStack>; 3],
    /// `furnaceBurnTime`, `currentItemBurnTime`, `furnaceCookTime`.
    pub burn: u16,
    pub item_burn: u16,
    pub cook: u16,
}

impl Furnace {
    fn can_smelt(&self) -> bool {
        let Some(r) = self.slots[0].and_then(|s| smelting(s.id)) else { return false };
        self.slots[2].map_or(true, |o| o.id == r.id && o.damage == r.damage && o.count < max_stack(r.id))
    }

    /// One `updateEntity`; true when the fire went on or off (the block must swap between unlit 61 and lit 62).
    pub fn tick(&mut self) -> bool {
        let was = self.burn > 0;
        self.burn = self.burn.saturating_sub(1);
        if self.burn == 0 && self.can_smelt() {
            self.item_burn = self.slots[1].map_or(0, |s| burn_time(s.id));
            self.burn = self.item_burn;
            if self.burn > 0 {
                if let Some(f) = &mut self.slots[1] {
                    f.count -= 1;
                    if f.count == 0 {
                        self.slots[1] = None;
                    }
                }
            }
        }
        if self.burn > 0 && self.can_smelt() {
            self.cook += 1;
            if self.cook == 200 {
                self.cook = 0;
                let r = smelting(self.slots[0].map_or(0, |s| s.id)).unwrap();
                match &mut self.slots[2] {
                    Some(o) => o.count += 1,
                    none => *none = Some(r),
                }
                self.slots[0] = rest(self.slots[0].unwrap(), 1);
            }
        } else {
            self.cook = 0;
        }
        was != (self.burn > 0)
    }
}

// ---- Container screen ----

/// A slot of the open screen. `Inv(i)` is `InventoryPlayer.mainInventory[i]`: 0..9 hotbar, 9..36 the rest.
/// `Furn(i)` is a furnace slot (0 input, 1 fuel, 2 output).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotId {
    Result,
    Grid(usize),
    Furn(usize),
    Inv(usize),
}

/// `ContainerPlayer` (`gw` 2) or `ContainerWorkbench` (`gw` 3): the crafting grid and the stack on the cursor.
/// The result slot is not stored, it is whatever `find` says about the grid.
// ponytail: no armor slots (no armor items), no shift-click (`getStackInSlot` quick-move), no right-button except as
// the `one` flag of `click`. Upgrade: add them when the touch UI has a gesture for them.
pub struct Screen {
    /// Grid width: 2 inventory, 3 workbench, 0 furnace (then `furnace` is the block it belongs to).
    pub gw: usize,
    pub furnace: Option<(i32, i32, i32)>,
    pub grid: [Option<ItemStack>; 9],
    pub cursor: Option<ItemStack>,
}

fn rest(s: ItemStack, n: u8) -> Option<ItemStack> {
    (s.count > n).then(|| ItemStack { count: s.count - n, ..s })
}

impl Screen {
    pub fn new(gw: usize) -> Self {
        Self { gw, furnace: None, grid: [None; 9], cursor: None }
    }

    /// `ContainerFurnace` for the furnace at `pos`; the caller passes that furnace to `get` and `click`.
    pub fn at_furnace(pos: (i32, i32, i32)) -> Self {
        Self { furnace: Some(pos), ..Self::new(0) }
    }

    pub fn result(&self) -> Option<ItemStack> {
        find(&self.grid, self.gw)
    }

    pub fn get(&self, inv: &Inventory, furn: Option<&Furnace>, id: SlotId) -> Option<ItemStack> {
        match id {
            SlotId::Result => self.result(),
            SlotId::Furn(i) => furn.and_then(|f| f.slots[i]),
            SlotId::Grid(i) => self.grid[i],
            SlotId::Inv(i) => inv.slots[i],
        }
    }

    fn set(&mut self, inv: &mut Inventory, furn: Option<&mut Furnace>, id: SlotId, s: Option<ItemStack>) {
        match id {
            SlotId::Result => {}
            SlotId::Furn(i) => {
                if let Some(f) = furn {
                    f.slots[i] = s;
                }
            }
            SlotId::Grid(i) => self.grid[i] = s,
            SlotId::Inv(i) => inv.slots[i] = s,
        }
    }

    /// `SlotCrafting.onPickupFromSlot`: one of every ingredient is used up (no container items in this port).
    fn take_result(&mut self) {
        for s in self.grid.iter_mut() {
            if let Some(t) = s {
                t.count -= 1;
                if t.count == 0 {
                    *s = None;
                }
            }
        }
    }

    /// Left click (`one` = false) or right click (`one` = true) on `id`, `Container.func_27280_a` branch by branch:
    /// empty slot takes the cursor (all, or one); empty cursor takes the slot (all, or the larger half); two
    /// different items swap; the same item tops the slot up. The result slot accepts nothing, and only gives
    /// its stack to an empty cursor or to a cursor already holding the same item (when it fits).
    ///
    /// The furnace output (`SlotFurnace`) is an output slot like the craft result (accepts nothing), except that
    /// taking from it uses nothing up and a right click takes half. Pass the furnace for a furnace screen.
    pub fn click(&mut self, id: SlotId, one: bool, inv: &mut Inventory, mut furn: Option<&mut Furnace>) {
        let is_result = id == SlotId::Result;
        let is_out = is_result || id == SlotId::Furn(2);
        if matches!(id, SlotId::Furn(_)) && furn.is_none() {
            return;
        }
        match (self.get(inv, furn.as_deref(), id), self.cursor) {
            (None, Some(c)) if !is_out => {
                let n = if one { 1 } else { c.count };
                self.set(inv, furn, id, Some(ItemStack { count: n, ..c }));
                self.cursor = rest(c, n);
            }
            (Some(h), None) => {
                let n = if one && !is_result { (h.count + 1) / 2 } else { h.count };
                self.cursor = Some(ItemStack { count: n, ..h });
                if is_result {
                    self.take_result();
                } else {
                    self.set(inv, furn, id, rest(h, n));
                }
            }
            (Some(h), Some(c)) => {
                let same = h.id == c.id && h.damage == c.damage;
                if is_out {
                    if same && max_stack(c.id) > 1 && h.count + c.count <= max_stack(c.id) {
                        self.cursor = Some(ItemStack { count: h.count + c.count, ..c });
                        if is_result {
                            self.take_result();
                        } else {
                            self.set(inv, furn, id, None);
                        }
                    }
                } else if !same {
                    self.set(inv, furn, id, Some(c));
                    self.cursor = Some(h);
                } else {
                    let n = (if one { 1 } else { c.count }).min(max_stack(c.id).saturating_sub(h.count));
                    self.set(inv, furn, id, Some(ItemStack { count: h.count + n, ..h }));
                    self.cursor = rest(c, n);
                }
            }
            _ => {}
        }
    }

    /// Spread (touch drag): put one item of the cursor stack into `id` when that slot is empty or holds the same
    /// item with room. Unlike a right click it never swaps, and never fills an output slot. True if it was valid.
    pub fn drop_one(&mut self, id: SlotId, inv: &mut Inventory, mut furn: Option<&mut Furnace>) -> bool {
        let Some(c) = self.cursor else { return false };
        if matches!(id, SlotId::Result | SlotId::Furn(2)) {
            return false;
        }
        let fits = self.get(inv, furn.as_deref(), id).map_or(true, |h| h.id == c.id && h.damage == c.damage && h.count < max_stack(c.id));
        if fits {
            self.click(id, true, inv, furn);
        }
        fits
    }

    /// `onCraftGuiClosed`: the cursor stack and everything left in the grid goes back into the world.
    pub fn close(&mut self) -> Vec<ItemStack> {
        self.cursor.take().into_iter().chain(self.grid.iter_mut().filter_map(Option::take)).collect()
    }
}

// ---- Geometry: the vanilla 176 x 166 GUI, scaled to the surface ----

pub const PANEL: (f32, f32) = (176.0, 166.0);
/// The "place one" toggle (this port has no right mouse button), in the column the vanilla armor slots would use.
// UNVERIFIED: position and the toggle itself are invented for touch (the original uses the mouse cursor and buttons).
pub const MODE: (f32, f32) = (8.0, 53.0);

/// Vanilla `(slot, x, y)` of every slot's top-left pixel (`ContainerPlayer` / `ContainerWorkbench` constructors).
pub fn layout(gw: usize) -> Vec<(SlotId, f32, f32)> {
    let ((rx, ry), (gx, gy)) = if gw == 2 { ((144, 36), (88, 26)) } else { ((124, 35), (30, 17)) };
    let mut v = if gw == 0 {
        vec![(SlotId::Furn(0), 56.0, 17.0), (SlotId::Furn(1), 56.0, 53.0), (SlotId::Furn(2), 116.0, 35.0)]
    } else {
        vec![(SlotId::Result, rx as f32, ry as f32)]
    };
    v.extend((0..gw * gw).map(|i| (SlotId::Grid(i), (gx + 18 * (i % gw)) as f32, (gy + 18 * (i / gw)) as f32)));
    v.extend((9..36).map(|i| (SlotId::Inv(i), (8 + 18 * (i % 9)) as f32, (84 + 18 * (i / 9 - 1)) as f32)));
    v.extend((0..9).map(|i| (SlotId::Inv(i), (8 + 18 * i) as f32, 142.0)));
    v
}

/// Panel origin (px) and scale (px per GUI unit) for a `w` x `h` surface.
pub fn panel(w: f32, h: f32) -> (f32, f32, f32) {
    let k = (h * 0.92 / PANEL.1).min(w * 0.92 / PANEL.0);
    ((w - PANEL.0 * k) * 0.5, (h - PANEL.1 * k) * 0.5, k)
}

/// The 18 x 18 unit cell around the 16 x 16 slot at GUI position `(ux, uy)`, in px: (x, y, size).
pub fn cell(w: f32, h: f32, (ux, uy): (f32, f32)) -> (f32, f32, f32) {
    let (ox, oy, k) = panel(w, h);
    (ox + (ux - 1.0) * k, oy + (uy - 1.0) * k, 18.0 * k)
}

fn inside((x, y, s): (f32, f32, f32), px: f32, py: f32) -> bool {
    px >= x && px < x + s && py >= y && py < y + s
}

/// Which slot a touch at `(px, py)` is on.
pub fn slot_at(gw: usize, w: f32, h: f32, px: f32, py: f32) -> Option<SlotId> {
    layout(gw).into_iter().find(|&(_, ux, uy)| inside(cell(w, h, (ux, uy)), px, py)).map(|s| s.0)
}

pub fn on_mode_button(w: f32, h: f32, px: f32, py: f32) -> bool {
    inside(cell(w, h, MODE), px, py)
}

pub fn in_panel(w: f32, h: f32, px: f32, py: f32) -> bool {
    let (ox, oy, k) = panel(w, h);
    px >= ox && px < ox + PANEL.0 * k && py >= oy && py < oy + PANEL.1 * k
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(id: u16, count: u8) -> ItemStack {
        ItemStack { id, count, damage: 0 }
    }

    /// Recipe matching: anywhere in the grid, mirrored, nothing else in the grid, 3x3 only where Java needs it.
    #[test]
    fn recipes_match_like_java() {
        let out = |cells: &[(usize, u16)], gw| {
            let mut grid = [None; 9];
            for &(i, id) in cells {
                grid[i] = Some(stack(id, 1));
            }
            find(&grid, gw).map(|s| (s.id, s.count))
        };
        assert_eq!(out(&[(0, 17)], 2), Some((5, 4)), "log -> 4 planks");
        assert_eq!(out(&[(1, 5), (3, 5)], 2), Some((280, 4)), "sticks, right column of a 2x2");
        assert_eq!(out(&[(0, 5), (1, 5), (2, 5), (3, 5)], 2), Some((58, 1)), "workbench");
        let pick = [(0, 5), (1, 5), (2, 5), (4, 280), (7, 280)];
        assert_eq!(out(&pick, 3), Some((270, 1)), "wooden pickaxe");
        assert_eq!(out(&pick, 2), None, "pickaxe does not fit a 2x2");
        assert_eq!(out(&[(0, 4), (1, 4), (3, 4), (4, 280), (7, 280)], 3), Some((275, 1)), "stone axe");
        assert_eq!(out(&[(0, 4), (1, 4), (4, 4), (3, 280), (6, 280)], 3), Some((275, 1)), "stone axe, mirrored");
        assert_eq!(out(&[(3, 264), (4, 280), (5, 5), (6, 280), (7, 5)], 3), None, "scrambled");
        assert_eq!(out(&[(0, 5), (1, 5), (2, 5), (4, 280), (7, 280), (8, 3)], 3), None, "a stray item breaks it");
        assert_eq!(out(&[(3, 264), (4, 264), (5, 264), (7, 280), (1, 280)], 3), None, "diamonds in the wrong place");
        assert_eq!(out(&[(0, 264), (3, 280), (6, 280)], 3), Some((277, 1)), "diamond shovel");
        assert_eq!(out(&[(0, 5), (2, 5), (4, 5)], 3), Some((281, 4)), "bowls");
        assert_eq!(out(&[(1, 40), (4, 39), (7, 281)], 3), Some((282, 1)), "stew");
        assert_eq!(out(&[(1, 39), (4, 40), (7, 281)], 3), Some((282, 1)), "stew, other order");
        assert_eq!(out(&[(1, 39), (4, 39), (7, 281)], 3), None, "two brown mushrooms");
        assert_eq!(out(&[(0, 4), (3, 4), (6, 280)], 3), Some((272, 1)), "stone sword");
        assert_eq!(out(&[(0, 5), (1, 5), (4, 280), (7, 280)], 3), Some((290, 1)), "wooden hoe");
        assert_eq!(out(&[(0, 5), (1, 5), (3, 280), (6, 280)], 3), Some((290, 1)), "wooden hoe, mirrored");
        assert_eq!(out(&[(1, 265), (2, 265)], 2), Some((359, 1)), "shears in a 2x2");
        assert_eq!(out(&[(0, 265), (3, 265)], 2), Some((359, 1)), "shears, mirrored");
        assert_eq!(out(&[(1, 265), (3, 265)], 3), Some((359, 1)), "shears in a 3x3");
    }

    /// The `Container.func_27280_a` rules: pick up, put one, take the result (uses up the grid), swap, top up, close.
    #[test]
    fn clicks_follow_container_rules() {
        let mut inv = Inventory::default();
        inv.slots[0] = Some(stack(17, 3));
        let mut s = Screen::new(2);
        s.click(SlotId::Inv(0), false, &mut inv, None); // all 3 logs to the cursor
        assert_eq!((inv.slots[0], s.cursor), (None, Some(stack(17, 3))));
        s.click(SlotId::Grid(0), true, &mut inv, None); // one log in the grid
        assert_eq!((s.grid[0], s.cursor), (Some(stack(17, 1)), Some(stack(17, 2))));
        assert_eq!(s.result(), Some(stack(5, 4)));
        s.click(SlotId::Result, false, &mut inv, None); // the cursor holds logs, the result is planks: refused
        assert_eq!((s.grid[0], s.cursor), (Some(stack(17, 1)), Some(stack(17, 2))));
        s.click(SlotId::Inv(0), false, &mut inv, None); // logs back into the empty slot
        assert_eq!((inv.slots[0], s.cursor), (Some(stack(17, 2)), None));
        s.click(SlotId::Result, false, &mut inv, None); // empty cursor takes 4 planks, the log is used up
        assert_eq!((s.cursor, s.grid[0], s.result()), (Some(stack(5, 4)), None, None));
        s.click(SlotId::Inv(1), false, &mut inv, None);
        s.click(SlotId::Inv(0), false, &mut inv, None); // swap: planks out of slot 1, 2 logs on the cursor
        s.click(SlotId::Inv(1), false, &mut inv, None);
        assert_eq!((inv.slots[1], s.cursor), (Some(stack(17, 2)), Some(stack(5, 4))));
        s.click(SlotId::Inv(0), false, &mut inv, None); // the empty slot takes the planks
        assert_eq!(inv.slots[0], Some(stack(5, 4)));
        assert_eq!(s.cursor, None);
        // Half a stack, then close: the cursor and the grid come back as drops.
        s.click(SlotId::Inv(1), true, &mut inv, None);
        assert_eq!((s.cursor, inv.slots[1]), (Some(stack(17, 1)), Some(stack(17, 1))));
        s.click(SlotId::Grid(3), false, &mut inv, None);
        s.click(SlotId::Inv(1), false, &mut inv, None);
        assert_eq!(s.close(), vec![stack(17, 1), stack(17, 1)]);
        assert!(s.cursor.is_none() && s.grid.iter().all(Option::is_none));
    }

    /// `TileEntityFurnace`: fuel is used up when the fire starts, 200 ticks per item, output stacks, and the
    /// lit state flips on and off. Plus the output slot rules of `SlotFurnace`.
    #[test]
    fn furnace_smelts_like_java() {
        let mut f = Furnace::default();
        f.slots[0] = Some(stack(15, 1)); // iron ore
        f.slots[1] = Some(stack(280, 2)); // 2 sticks: 100 ticks of fire each, so exactly one item
        assert!(f.tick(), "fire starts");
        assert_eq!((f.burn, f.slots[1]), (100, Some(stack(280, 1))));
        for _ in 0..199 {
            assert!(!f.tick(), "the second stick takes over without the fire going out");
        }
        assert_eq!((f.slots[0], f.slots[1], f.slots[2]), (None, None, Some(stack(265, 1))), "ingot after 200 ticks");
        assert_eq!(f.burn, 1);
        assert!(f.tick() && f.burn == 0, "fire out");
        f.slots[0] = Some(stack(3, 1)); // dirt does not smelt
        f.slots[1] = Some(stack(263, 1));
        assert!(!f.tick() && f.burn == 0 && f.slots[1].is_some());
        // Output slot: takes nothing, gives half on a right click.
        let mut s = Screen::at_furnace((0, 0, 0));
        let mut inv = Inventory::default();
        f.slots[2] = Some(stack(265, 5));
        s.cursor = Some(stack(3, 1));
        s.click(SlotId::Furn(2), false, &mut inv, Some(&mut f));
        assert_eq!((s.cursor, f.slots[2]), (Some(stack(3, 1)), Some(stack(265, 5))));
        s.cursor = None;
        s.click(SlotId::Furn(2), true, &mut inv, Some(&mut f));
        assert_eq!((s.cursor, f.slots[2]), (Some(stack(265, 3)), Some(stack(265, 2))));
        s.click(SlotId::Furn(0), false, &mut inv, Some(&mut f)); // the input slot takes anything
        assert_eq!(f.slots[0], Some(stack(265, 3)));
    }

    /// Touch spread: one item per slot, never a swap, never into an output slot.
    #[test]
    fn drop_one_spreads_the_cursor_stack() {
        let mut inv = Inventory::default();
        let mut s = Screen::new(2);
        s.cursor = Some(stack(5, 3));
        inv.slots[0] = Some(stack(3, 1));
        assert!(!s.drop_one(SlotId::Result, &mut inv, None), "output slots refuse");
        assert!(s.drop_one(SlotId::Grid(0), &mut inv, None) && s.drop_one(SlotId::Grid(2), &mut inv, None));
        assert!(!s.drop_one(SlotId::Inv(0), &mut inv, None), "a different item is not swapped");
        assert_eq!((inv.slots[0], s.cursor), (Some(stack(3, 1)), Some(stack(5, 1))));
        assert!(s.drop_one(SlotId::Grid(0), &mut inv, None), "same item stacks up");
        assert_eq!((s.grid[0], s.grid[2], s.cursor), (Some(stack(5, 2)), Some(stack(5, 1)), None));
    }

    /// Tools: ids, durability, speed on their blocks and the harvest gate.
    #[test]
    fn tool_tables() {
        assert_eq!(tool(270), Some((Kind::Pickaxe, 0)));
        assert_eq!(tool(279), Some((Kind::Axe, 3)));
        assert_eq!(tool(280), None);
        assert_eq!((max_damage(257), max_damage(278), max_damage(285)), (Some(250), Some(1561), Some(32)));
        assert_eq!((str_vs_block(274, 1), str_vs_block(274, 3), str_vs_block(285, 17), str_vs_block(286, 5)), (4.0, 1.0, 1.0, 12.0));
        assert!(!tool_can_harvest(270, 15) && tool_can_harvest(274, 15), "iron ore needs stone");
        assert!(!tool_can_harvest(274, 56) && tool_can_harvest(257, 56), "diamond ore needs iron");
        assert!(!tool_can_harvest(257, 49) && tool_can_harvest(278, 49), "obsidian needs diamond");
        assert!(tool_can_harvest(270, 1) && tool_can_harvest(285, 4), "any pickaxe takes stone");
        assert!(!tool_can_harvest(271, 1) && !tool_can_harvest(5, 1), "an axe or a block does not");
        assert!(tool_can_harvest(269, 78) && !tool_can_harvest(270, 78), "snow takes a shovel");
        // Sword, hoe, shears.
        assert_eq!((tool(267), tool(294), tool(359)), (Some((Kind::Sword, 2)), Some((Kind::Hoe, 4)), None));
        assert_eq!((max_damage(276), max_damage(SHEARS), max_damage(292)), (Some(1561), Some(238), Some(250)));
        let speeds = [str_vs_block(268, 1), str_vs_block(268, 30), str_vs_block(SHEARS, 18), str_vs_block(SHEARS, 35), str_vs_block(SHEARS, 1)];
        assert_eq!(speeds, [1.5, 15.0, 15.0, 5.0, 1.0]);
        assert!(tool_can_harvest(268, 30) && tool_can_harvest(SHEARS, 30) && !tool_can_harvest(290, 30), "web: sword and shears");
        assert!(!tool_can_harvest(SHEARS, 1) && !tool_can_harvest(268, 1), "a sword or shears is no pickaxe");
        let wear = [wear_on_break(270, 1), wear_on_break(268, 3), wear_on_break(290, 3), wear_on_break(SHEARS, 18), wear_on_break(SHEARS, 3), wear_on_break(5, 1)];
        assert_eq!(wear, [1, 2, 0, 1, 0, 0]);
        assert!(can_till(3, 1, 0) && can_till(2, 0, 1) && !can_till(2, 0, 0) && !can_till(2, 1, 1) && !can_till(1, 0, 1), "hoe rules");
        assert!(is_hoe(291) && !is_hoe(274));
    }
}
