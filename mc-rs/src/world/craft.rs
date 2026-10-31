//! M6 crafting + tools. Ports of `EnumToolMaterial`, `ItemTool` / `ItemPickaxe` / `ItemSpade` / `ItemSword` /
//! `ItemHoe` / `ItemShears` (`getStrVsBlock`,
//! `canHarvestBlock`), `CraftingManager` + `ShapedRecipes.matches`, and the left-click rules of
//! `Container.func_27280_a` for `ContainerPlayer` (2x2 grid), `ContainerWorkbench` (3x3 grid) and `ContainerFurnace`,
//! plus `TileEntityFurnace` and `FurnaceRecipes` (smelting).
//! Item ids follow `world::items`: below 256 = the block, from 256 up = `Item.shiftedIndex` (256 + n).

use std::sync::OnceLock;

use crate::world::combat;
use crate::world::items::{max_stack, Inventory, ItemStack, MAIN};
use crate::world::map;

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
/// `Item.flintAndSteel` (3 + 256): 64 uses, `ItemFlintAndSteel.onItemUse` lights the cell next to the face aimed at.
pub const FLINT_AND_STEEL: u16 = 259;

/// (kind, material index) of a tool item, `None` for anything else.
pub fn tool(id: u16) -> Option<(Kind, usize)> {
    TOOL_IDS.iter().enumerate().find_map(|(m, ids)| ids.iter().position(|&i| i == id).map(|k| (KINDS[k], m)))
}

/// `Item.getMaxDamage`: durability of a tool, `None` = not damageable.
pub fn max_damage(id: u16) -> Option<u16> {
    if id == SHEARS {
        return Some(238);
    }
    if id == FLINT_AND_STEEL {
        return Some(64);
    }
    if id == crate::world::gear::ROD {
        return Some(64);
    }
    if let Some((t, m)) = armor(id) {
        return Some([11, 16, 15, 13][t] * 3 << [0, 1, 2, 3, 1][m]); // ItemArmor: maxDamageArray[type] * 3 << armorLevel
    }
    // Cortes Combat weapons (NOT b1.7.3): the same uses as a tool of their material.
    if let Some((_, m)) = combat::catalog::get(id) {
        return Some(MATERIALS[m].1);
    }
    tool(id).map(|(_, m)| MATERIALS[m].1)
}

/// (armor type 0 helmet..3 boots, material 0 leather, chain, iron, diamond, gold) of an `ItemArmor` (298..=317).
pub fn armor(id: u16) -> Option<(usize, usize)> {
    (298..318).contains(&id).then(|| ((id - 298) as usize % 4, (id - 298) as usize / 4))
}

/// `SlotArmor.isItemValid` for inventory slot `MAIN + k` (`armorInventory[k]`, type `3 - k`): matching armor, or a pumpkin on the head.
fn armor_fits(k: usize, id: u16) -> bool {
    armor(id).map_or(id == 86 && k == 3, |a| a.0 == 3 - k)
}

/// `InventoryPlayer.getTotalArmorValue`: `slots` is the armor part of the inventory.
pub fn armor_value(slots: &[Option<ItemStack>]) -> i32 {
    let (mut red, mut left, mut max) = (0, 0, 0);
    for s in slots.iter().flatten().filter(|s| armor(s.id).is_some()) {
        let m = max_damage(s.id).unwrap() as i32;
        (red, left, max) = (red + [3, 8, 6, 3][armor(s.id).unwrap().0], left + m - s.damage as i32, max + m);
    }
    if max == 0 { 0 } else { (red - 1) * left / max + 1 }
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

/// A `ShapedRecipes`: `w` x `h` pattern, row-major, 0 = empty cell (`w` 0 = `ShapelessRecipes`, `cells` the ingredients).
/// Ingredients are item codes (`code`): the id, plus the damage for wool and dye, the only ones that need it (the Java
/// ingredients are damage -1 = any, or an exact wool / dye colour).
pub struct Recipe {
    w: usize,
    h: usize,
    cells: [u16; 9],
    out: ItemStack,
}

impl Recipe {
    /// What the recipe makes (the recipe book, `world::book`).
    pub fn out(&self) -> ItemStack {
        self.out
    }

    /// The pattern's width and height, `None` for a shapeless recipe.
    pub fn shape(&self) -> Option<(usize, usize)> {
        (self.w > 0).then_some((self.w, self.h))
    }

    /// The ingredient codes: a shaped recipe's pattern row by row (0 = an empty cell), a shapeless one's list.
    pub fn pattern(&self) -> &[u16] {
        if self.w == 0 {
            &self.cells[..self.cells.iter().position(|&c| c == 0).unwrap_or(9)]
        } else {
            &self.cells[..self.w * self.h]
        }
    }

    /// `ShapedRecipes.matches` on a `gw` x `gw` grid of item ids: the pattern may sit anywhere in the 3x3 area
    /// (cells outside a 2x2 grid are empty), mirrored or not, and every other cell must be empty.
    fn matches(&self, grid: &[u16; 9], gw: usize) -> bool {
        if self.w == 0 {
            // `ShapelessRecipes.matches`: the same ingredients, in any cells (`cells` is the list).
            let sorted = |c: &[u16]| { let mut v: Vec<u16> = c.iter().copied().filter(|&c| c != 0).collect(); v.sort_unstable(); v };
            return sorted(grid) == sorted(&self.cells);
        }
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

/// Ingredient / result code: wool and dye keep their colour in the bits above the id (ids stay below 512).
pub fn code(id: u16, damage: u16) -> u16 {
    if matches!(id, 35 | 351) { id | (damage & 15) << 9 } else { id }
}

/// `code` the other way: the item id and the damage (the colour of a wool or a dye, else 0).
pub fn unpack(code: u16) -> (u16, u16) {
    (code & 511, code >> 9)
}

fn shapeless(ings: &[u16], out: u16, n: u8) -> Recipe {
    let mut cells = [0; 9];
    cells[..ings.len()].copy_from_slice(ings);
    Recipe { w: 0, h: 0, cells, out: ItemStack { id: out & 511, count: n, damage: out >> 9 } }
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
    Recipe { w, h, cells, out: ItemStack { id: out & 511, count: n, damage: out >> 9 } }
}

/// Every recipe of `RecipesTools` (pickaxe, shovel, axe, hoe x 5 materials), `RecipesWeapons` (swords), the shears
/// and the `CraftingManager` / `RecipesCrafting` ones whose result exists in this port (a block that can be placed, or
/// a tool ingredient), `RecipesFood` (cookie), `RecipesDyes`, `RecipesIngots` and `RecipesArmor`. Not ported: rails, doors, stairs and the rest of
/// `CraftingManager`. One line each to add.
// ponytail: vanilla sorts the list (`RecipeSorter`, bigger first); no two patterns here can match one grid, so the
// first match is the only match and the sort is skipped.
pub fn recipes() -> &'static [Recipe] {
    static R: OnceLock<Vec<Recipe>> = OnceLock::new();
    R.get_or_init(|| {
        const PLANKS: u16 = 5;
        const STICK: u16 = 280;
        let dy = |d: u16| code(351, d);
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
            shaped(&["A ", " B"], &[('A', 265), ('B', 318)], FLINT_AND_STEEL, 1), // CraftingManager: iron ingot + flint
            shaped(&["###", "XXX"], &[('#', 35), ('X', 5)], 355, 1), // bed: wool + planks
            shaped(&["###", "###"], &[('#', PLANKS)], 96, 2), // trapdoor (CraftingManager)
            shaped(&["AAA", "BEB", "CCC"], &[('A', 335), ('B', 353), ('C', 296), ('E', 344)], 354, 1), // cake: milk, sugar, wheat, egg
            shaped(&["##", "##", "##"], &[('#', 5)], 324, 1),   // wooden door
            shaped(&["##", "##", "##"], &[('#', 265)], 330, 1), // iron door
            shaped(&["#  ", "## ", "###"], &[('#', 5)], 53, 4), // wooden stairs
            shaped(&["#  ", "## ", "###"], &[('#', 4)], 67, 4), // cobblestone stairs
            shaped(&["# #", " # "], &[('#', 265)], 325, 1), // bucket
            shaped(&["#X#"], &[('X', dy(3)), ('#', 296)], 357, 8), // cookie: wheat, cocoa, wheat
            shapeless(&[37], dy(11), 2), // dandelion -> yellow dye
            shapeless(&[38], dy(1), 2),  // rose -> red dye
            shapeless(&[352], dy(15), 3), // bone -> bone meal
            shaped(&[" #X", "# X", " #X"], &[('#', STICK), ('X', 287)], 261, 1), // bow (RecipesWeapons)
            shaped(&["X", "#", "Y"], &[('X', 318), ('#', STICK), ('Y', 288)], 262, 4), // arrow: flint, stick, feather
            shaped(&["  #", " #X", "# X"], &[('#', STICK), ('X', 287)], 346, 1), // fishing rod
            shaped(&["###"], &[('#', 296)], 297, 1), // bread
            shaped(&["###", "#X#", "###"], &[('#', 41), ('X', 260)], 322, 1), // golden apple: gold block + apple
            // CraftingManager: the plain ones whose result already works (blocks that place, items that stack).
            shaped(&["###"], &[('#', 338)], 339, 3),                         // paper: 3 sugar cane
            shaped(&["#", "#", "#"], &[('#', 339)], 340, 1),                 // book: 3 paper
            shaped(&["#"], &[('#', 338)], 353, 1),                           // sugar: 1 sugar cane
            shaped(&["###", "###"], &[('#', STICK)], 85, 2),                 // fence
            shaped(&["# #", "###", "# #"], &[('#', STICK)], 65, 2),          // ladder
            shaped(&["##", "##"], &[('#', 336)], 45, 1),                     // brick block: 4 bricks
            shaped(&["###", "XXX", "###"], &[('#', PLANKS), ('X', 340)], 47, 1), // bookshelf
            shaped(&["A", "B"], &[('A', 86), ('B', 50)], 91, 1),             // jack-o'-lantern: pumpkin + torch
            shaped(&["###", "###", " X "], &[('#', PLANKS), ('X', STICK)], 323, 1),   // sign
            shaped(&["###", "#X#", "###"], &[('#', STICK), ('X', 35)], 321, 1),        // painting: 8 sticks round a wool
            shaped(&["X#X", "#X#", "X#X"], &[('X', 289), ('#', 12)], 46, 1),  // TNT: gunpowder + sand
            shaped(&["# #", "###"], &[('#', PLANKS)], 333, 1), // boat
            shaped(&["# #", "###"], &[('#', 265)], 328, 1), // minecart: 5 iron ingots
            shaped(&["A", "B"], &[('A', 61), ('B', 328)], 343, 1), // minecart with furnace (CraftingManager): furnace over minecart
            shaped(&["A", "B"], &[('A', 54), ('B', 328)], 342, 1), // minecart with chest (CraftingManager): chest over minecart
            shaped(&[" # ", "#X#", " # "], &[('#', 265), ('X', 331)], 345, 1), // compass: 4 iron ingots round redstone
            shaped(&[" # ", "#X#", " # "], &[('#', 266), ('X', 331)], 347, 1), // clock (`pocketSundial`): 4 gold ingots round redstone
            shaped(&["###", "#X#", "###"], &[('#', 339), ('X', 345)], map::MAP, 1), // map (`Item.mapItem`): 8 paper round a compass; `map::fresh` marks the result as it is taken
            shaped(&["X X", "X#X", "X X"], &[('X', 265), ('#', STICK)], 66, 16), // rails: 6 iron + a stick
            shaped(&["X X", "X#X", "XRX"], &[('X', 266), ('#', STICK), ('R', 331)], 27, 6), // powered rails: 6 gold + stick + redstone
            shaped(&["X X", "X#X", "XRX"], &[('X', 265), ('#', 70), ('R', 331)], 28, 6), // detector rails: 6 iron + stone plate + redstone
            // The redstone recipes of `CraftingManager` (M21): lever, torch, repeater, button, the two plates, jukebox, note block,
            // dispenser, the two pistons.
            shaped(&["X", "#"], &[('X', STICK), ('#', 4)], 69, 1),
            shaped(&["X", "#"], &[('X', 331), ('#', STICK)], 76, 1),
            shaped(&["#X#", "III"], &[('#', 76), ('X', 331), ('I', 1)], 356, 1),
            shaped(&["#", "#"], &[('#', 1)], 77, 1),
            shaped(&["##"], &[('#', 1)], 70, 1),
            shaped(&["##"], &[('#', PLANKS)], 72, 1),
            shaped(&["###", "#X#", "###"], &[('#', PLANKS), ('X', 264)], 84, 1),
            shaped(&["###", "#X#", "###"], &[('#', PLANKS), ('X', 331)], 25, 1),
            shaped(&["###", "#X#", "#R#"], &[('#', 4), ('X', 261), ('R', 331)], 23, 1),
            shaped(&["TTT", "#X#", "#R#"], &[('T', PLANKS), ('#', 4), ('X', 265), ('R', 331)], 33, 1),
            shaped(&["S", "P"], &[('S', 341), ('P', 33)], 29, 1),
            shaped(&["###"], &[('#', 4)], 44 | 3 << 9, 3),                   // slabs x3: cobblestone, stone, sandstone, planks
            shaped(&["###"], &[('#', 1)], 44, 3),
            shaped(&["###"], &[('#', 24)], 44 | 1 << 9, 3),
            shaped(&["###"], &[('#', PLANKS)], 44 | 2 << 9, 3),
        ];
        // RecipesDyes: dye + white wool -> the dye's wool (`BlockCloth.func_21035_d` = `~d & 15`), then the dye mixes.
        r.extend((0..16).map(|d| shapeless(&[dy(d), 35], 35 | (!d & 15) << 9, 1)));
        let mixes: [(&[u16], u16, u8); 12] = [(&[1, 15][..], 9, 2), (&[1, 11][..], 14, 2), (&[2, 15][..], 10, 2), (&[0, 15][..], 8, 2), (&[8, 15][..], 7, 2), (&[0, 15, 15][..], 7, 3), (&[4, 15][..], 12, 2), (&[4, 2][..], 6, 2), (&[4, 1][..], 5, 2), (&[5, 9][..], 13, 2), (&[4, 1, 9][..], 13, 3), (&[4, 1, 1, 15][..], 13, 4)];
        r.extend(mixes.iter().map(|&(src, out, n)| shapeless(&src.iter().map(|&d| dy(d)).collect::<Vec<_>>(), dy(out), n)));
        // RecipesIngots: 9 of the item <-> the block (gold, iron, diamond, lapis).
        for (block, item) in [(41, 266), (42, 265), (57, 264), (22, dy(4))] {
            r.push(shaped(&["###", "###", "###"], &[('#', item)], block, 1));
            r.push(shaped(&["#"], &[('#', block)], item, 9));
        }
        // RecipesArmor: leather, fire (chain: no item makes it), iron, diamond, gold; helmet, chestplate, leggings, boots.
        const ARMOR: [&[&str]; 4] = [&["XXX", "X X"], &["X X", "XXX", "XXX"], &["XXX", "X X", "X X"], &["X X", "X X"]];
        for (m, x) in [334, 51, 265, 264, 266].into_iter().enumerate() {
            r.extend(ARMOR.iter().enumerate().map(|(t, rows)| shaped(rows, &[('X', x)], 298 + (m * 4 + t) as u16, 1)));
        }
        // RecipesTools: head material per tier = planks, cobblestone, iron ingot, diamond, gold ingot.
        for (m, head) in [PLANKS, 4, 265, 264, 266].into_iter().enumerate() {
            for (k, rows) in PATTERNS.iter().enumerate() {
                r.push(shaped(rows, &[('#', STICK), ('X', head)], TOOL_IDS[m][k], 1));
            }
        }
        // Cortes Combat (NOT b1.7.3): 20 melee weapons, one line of data each in `combat::catalog`.
        r.extend(combat::catalog::all().filter_map(|(f, t)| combat::catalog::recipe(f, t)).map(|(rows, key, out)| shaped(rows, &key, out, 1)));
        r
    })
}

/// `CraftingManager.findMatchingRecipe` for a `gw` x `gw` grid.
pub fn find(grid: &[Option<ItemStack>; 9], gw: usize) -> Option<ItemStack> {
    let ids = grid.map(|s| s.map_or(0, |s| code(s.id, s.damage)));
    // The 20 Cortes weapons are made only in a world with the Immersive Pack ON (`combat::rules`, NOT b1.7.3).
    let combat = super::combat::rules::on();
    recipes().iter().find(|r| r.matches(&ids, gw) && (combat || !super::combat::rules::is_weapon(r.out.id))).map(|r| r.out)
}

// ---- Furnace ----

/// `FurnaceRecipes`: what smelting `id` gives.
pub fn smelting(id: u16) -> Option<ItemStack> {
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
        349 => (350, 0), // raw -> cooked fish
        _ => return None,
    };
    Some(ItemStack { id: out, count: 1, damage })
}

/// `TileEntityFurnace.getItemBurnTime` in ticks: wooden blocks 300, stick and sapling 100, coal 1600.
/// and the lava bucket 20000 (it leaves an empty bucket).
fn burn_time(id: u16) -> u16 {
    match id {
        5 | 17 | 25 | 47 | 53 | 54 | 58 | 63 | 64 | 68 | 72 | 84 | 85 | 95 | 96 => 300,
        280 | 6 => 100,
        263 => 1600,
        327 => 20000,
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
                        // `getContainerItem`: a lava bucket leaves an empty one.
                        self.slots[1] = (f.id == 327).then_some(ItemStack { id: 325, count: 1, damage: 0 });
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
/// `Furn(i)` is a furnace slot (0 input, 1 fuel, 2 output), `Chest(i)` a slot of the open chest (a large chest counts on: 27..54).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotId {
    Result,
    Grid(usize),
    Furn(usize),
    Chest(usize),
    Inv(usize),
}

/// `ContainerPlayer` (`gw` 2) or `ContainerWorkbench` (`gw` 3): the crafting grid and the stack on the cursor.
/// The result slot is not stored, it is whatever `find` says about the grid.
// ponytail: no shift-click (`getStackInSlot` quick-move), no right-button except as
// the `one` flag of `click`. Upgrade: add them when the touch UI has a gesture for them.
pub struct Screen {
    /// Grid width: 2 inventory, 3 workbench, 0 furnace (then `furnace` is the block it belongs to), or the number of chest
    /// slots (27 per chest, then `chests` are the blocks, upper half first): there is no grid in that case.
    pub gw: usize,
    pub furnace: Option<(i32, i32, i32)>,
    pub chests: Vec<(i32, i32, i32)>,
    pub grid: [Option<ItemStack>; 9],
    pub cursor: Option<ItemStack>,
}

/// Put `s` into `slots`: top up matching stacks first, then fill empty slots. `s.count` is what is left.
fn merge(slots: &mut [Option<ItemStack>], s: &mut ItemStack) {
    for pass in 0..2 {
        for o in slots.iter_mut() {
            if s.count == 0 {
                return;
            }
            if let Some(t) = o.as_mut() {
                if pass == 0 && t.id == s.id && t.damage == s.damage {
                    let n = s.count.min(max_stack(t.id).saturating_sub(t.count));
                    t.count += n;
                    s.count -= n;
                }
            } else if pass == 1 {
                let n = s.count.min(max_stack(s.id));
                *o = Some(ItemStack { count: n, ..*s });
                s.count -= n;
            }
        }
    }
}

fn rest(s: ItemStack, n: u8) -> Option<ItemStack> {
    (s.count > n).then(|| ItemStack { count: s.count - n, ..s })
}

impl Screen {
    pub fn new(gw: usize) -> Self {
        Self { gw, furnace: None, chests: Vec::new(), grid: [None; 9], cursor: None }
    }

    /// `ContainerFurnace` for the furnace at `pos`; the caller passes that furnace to `get` and `click`.
    pub fn at_furnace(pos: (i32, i32, i32)) -> Self {
        Self { furnace: Some(pos), ..Self::new(0) }
    }

    /// `ContainerDispenser` for the dispenser at `pos` (`gw` 9: a chest-like screen of 9 slots in a 3 x 3 block).
    pub fn at_dispenser(pos: (i32, i32, i32)) -> Self {
        Self { gw: 9, chests: vec![pos], ..Self::new(0) }
    }

    /// `ContainerChest` for the chests in `group` (`chest::open`); the caller passes their slots, in that order, to `get` and `click`.
    pub fn at_chest(group: Vec<(i32, i32, i32)>) -> Self {
        Self { gw: group.len() * 27, chests: group, ..Self::new(0) }
    }

    pub fn result(&self) -> Option<ItemStack> {
        find(&self.grid, self.gw)
    }

    pub fn get(&self, inv: &Inventory, ext: Option<&[Option<ItemStack>]>, id: SlotId) -> Option<ItemStack> {
        match id {
            SlotId::Result => self.result(),
            SlotId::Furn(i) | SlotId::Chest(i) => ext.and_then(|f| f[i]),
            SlotId::Grid(i) => self.grid[i],
            SlotId::Inv(i) => inv.slots[i],
        }
    }

    fn set(&mut self, inv: &mut Inventory, ext: Option<&mut [Option<ItemStack>]>, id: SlotId, s: Option<ItemStack>) {
        match id {
            SlotId::Result => {}
            SlotId::Furn(i) | SlotId::Chest(i) => {
                if let Some(f) = ext {
                    f[i] = s;
                }
            }
            SlotId::Grid(i) => self.grid[i] = s,
            SlotId::Inv(i) => inv.slots[i] = s,
        }
    }

    /// `SlotCrafting.onPickupFromSlot`: one of every ingredient is used up; a bucket with something in it (water, lava, milk)
    /// leaves an empty one (`getContainerItem`).
    fn take_result(&mut self) {
        for s in self.grid.iter_mut() {
            if let Some(t) = s {
                t.count -= 1;
                if t.count == 0 {
                    *s = matches!(t.id, 326 | 327 | 335).then_some(ItemStack { id: 325, count: 1, damage: 0 });
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
    /// taking from it uses nothing up and a right click takes half. Pass the open container's slots (`ext`: a furnace's 3, the chests' 27 each) for a furnace or chest screen.
    pub fn click(&mut self, id: SlotId, one: bool, inv: &mut Inventory, ext: Option<&mut [Option<ItemStack>]>) {
        if !self.armor_ok(id) {
            return;
        }
        let is_result = id == SlotId::Result;
        let is_out = is_result || id == SlotId::Furn(2);
        if matches!(id, SlotId::Furn(_) | SlotId::Chest(_)) && ext.is_none() {
            return;
        }
        match (self.get(inv, ext.as_deref(), id), self.cursor) {
            (None, Some(c)) if !is_out => {
                let n = if one { 1 } else { c.count };
                self.set(inv, ext, id, Some(ItemStack { count: n, ..c }));
                self.cursor = rest(c, n);
            }
            (Some(h), None) => {
                let n = if one && !is_result { (h.count + 1) / 2 } else { h.count };
                self.cursor = Some(ItemStack { count: n, ..h });
                if is_result {
                    self.take_result();
                    self.cursor.iter_mut().for_each(map::fresh); // `ItemMap.onCreated` runs on the stack the player took (`App::map_tick`)
                } else {
                    self.set(inv, ext, id, rest(h, n));
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
                            self.set(inv, ext, id, None);
                        }
                    }
                } else if !same {
                    self.set(inv, ext, id, Some(c));
                    self.cursor = Some(h);
                } else {
                    let n = (if one { 1 } else { c.count }).min(max_stack(c.id).saturating_sub(h.count));
                    self.set(inv, ext, id, Some(ItemStack { count: h.count + n, ..h }));
                    self.cursor = rest(c, n);
                }
            }
            _ => {}
        }
    }

    /// `SlotArmor`: only the right armor goes in, one at a time. Any other slot takes anything.
    fn armor_ok(&self, id: SlotId) -> bool {
        match (id, self.cursor) {
            (SlotId::Inv(i @ MAIN..), Some(c)) => armor_fits(i - MAIN, c.id) && c.count == 1,
            _ => true,
        }
    }

    /// Spread (touch drag): put one item of the cursor stack into `id` when that slot is empty or holds the same
    /// item with room. Unlike a right click it never swaps, and never fills an output slot. True if it was valid.
    pub fn drop_one(&mut self, id: SlotId, inv: &mut Inventory, ext: Option<&mut [Option<ItemStack>]>) -> bool {
        let Some(c) = self.cursor else { return false };
        if matches!(id, SlotId::Result | SlotId::Furn(2)) {
            return false;
        }
        let fits = self.armor_ok(id) && self.get(inv, ext.as_deref(), id).map_or(true, |h| h.id == c.id && h.damage == c.damage && h.count < max_stack(c.id));
        if fits {
            self.click(id, true, inv, ext);
        }
        fits
    }

    /// Shift-click (quick move). UNVERIFIED: b1.7.3 has none; this follows the later vanilla rules. A result is crafted
    /// until the grid or the room runs out; grid, furnace, chest and armor slots go to the inventory; an inventory
    /// stack goes to its armor slot, the furnace (input or fuel), the chest, else swaps between hotbar and main.
    pub fn shift_click(&mut self, id: SlotId, inv: &mut Inventory, mut ext: Option<&mut [Option<ItemStack>]>) {
        if self.cursor.is_some() || (matches!(id, SlotId::Furn(_) | SlotId::Chest(_)) && ext.is_none()) {
            return;
        }
        let Some(s) = self.get(inv, ext.as_deref(), id) else { return };
        if id == SlotId::Result {
            while let Some(mut r) = self.result() {
                map::fresh(&mut r);
                let before = inv.slots;
                inv.add(&mut r);
                if r.count > 0 {
                    inv.slots = before;
                    break;
                }
                self.take_result();
            }
            return;
        }
        let mut t = s;
        match id {
            SlotId::Inv(i) if i < MAIN => {
                let k = (0..4).find(|&k| armor_fits(k, t.id) && inv.slots[MAIN + k].is_none() && (armor(t.id).is_some() || t.id == 86));
                if let Some(k) = k {
                    inv.slots[MAIN + k] = Some(ItemStack { count: 1, ..t });
                    t.count -= 1;
                } else if let (Some(f), true) = (ext.as_deref_mut().filter(|_| self.furnace.is_some()), true) {
                    let slot = if smelting(t.id).is_some() { 0 } else if burn_time(t.id) > 0 { 1 } else { 9 };
                    if slot < 9 {
                        merge(&mut f[slot..=slot], &mut t);
                    }
                } else if let Some(c) = ext.as_deref_mut().filter(|_| !self.chests.is_empty()) {
                    merge(c, &mut t);
                } else if t.count > 0 {
                    let (a, b) = if i < 9 { (9, MAIN) } else { (0, 9) };
                    merge(&mut inv.slots[a..b], &mut t);
                }
            }
            _ => {
                inv.add(&mut t);
            }
        }
        self.set(inv, ext, id, (t.count > 0).then_some(t));
    }

    /// `onCraftGuiClosed`: the cursor stack and everything left in the grid goes back into the world.
    pub fn close(&mut self) -> Vec<ItemStack> {
        self.cursor.take().into_iter().chain(self.grid.iter_mut().filter_map(Option::take)).collect()
    }
}

// ---- Geometry: the vanilla GUI (176 x 166 units, a chest's taller), scaled to the surface ----

/// `GuiContainer.xSize` x `ySize`: the vanilla 176 x 166 GUI, or a chest's `114 + 18 * rows` (`GuiChest`).
pub fn panel_size(gw: usize) -> (f32, f32) {
    (176.0, if gw > 3 && gw != 9 { 114.0 + 18.0 * (gw / 9) as f32 } else { 166.0 })
}

/// Units the inventory screen's crafting block (2x2 grid, arrow, result) sits left of where the pack's panel has it. The panel picture
/// is shifted by the same amount in `assets/containers.rgba` (`tools/gen_gui.py`); `slots_sit_on_the_panels_frames` fails if they part.
pub const CRAFT_SHIFT: usize = 6;

/// Vanilla `(slot, x, y)` of every slot's top-left pixel (`ContainerPlayer` / `ContainerWorkbench` / `ContainerFurnace` /
/// `ContainerChest` constructors). A chest screen has `gw` chest slots (rows of 9) and the player's 27 + 9 below them.
pub fn layout(gw: usize) -> Vec<(SlotId, f32, f32)> {
    let (disp, chest) = (gw == 9, gw > 3 && gw != 9);
    let d = if chest { (gw / 9) as i32 * 18 - 72 } else { 0 }; // `ContainerChest`: `(rows - 4) * 18`
    let ((rx, ry), (gx, gy)) = if gw == 2 { ((154 - CRAFT_SHIFT, 28), (98 - CRAFT_SHIFT, 18)) } else { ((124, 35), (30, 17)) };
    let mut v: Vec<_> = if disp {
        (0..9).map(|i| (SlotId::Chest(i), (62 + 18 * (i % 3)) as f32, (17 + 18 * (i / 3)) as f32)).collect() // `ContainerDispenser`
    } else if chest {
        (0..gw).map(|i| (SlotId::Chest(i), (8 + 18 * (i % 9)) as f32, (18 + 18 * (i / 9)) as f32)).collect()
    } else if gw == 0 {
        vec![(SlotId::Furn(0), 56.0, 17.0), (SlotId::Furn(1), 56.0, 53.0), (SlotId::Furn(2), 116.0, 35.0)]
    } else {
        vec![(SlotId::Result, rx as f32, ry as f32)]
    };
    if !chest && !disp {
        v.extend((0..gw * gw).map(|i| (SlotId::Grid(i), (gx + 18 * (i % gw)) as f32, (gy + 18 * (i / gw)) as f32)));
    }
    if gw == 2 {
        v.extend((0..4).map(|k| (SlotId::Inv(MAIN + k), 8.0, 8.0 + 18.0 * (3 - k) as f32))); // ContainerPlayer: SlotArmor at (8, 8 + 18 * type)
    }
    let (rows_y, hot_y) = if chest { (103 + d, 161 + d) } else { (84, 142) };
    v.extend((9..36).map(|i| (SlotId::Inv(i), (8 + 18 * (i % 9)) as f32, (rows_y + 18 * (i as i32 / 9 - 1)) as f32)));
    v.extend((0..9).map(|i| (SlotId::Inv(i), (8 + 18 * i) as f32, hot_y as f32)));
    v
}

/// Panel origin (px) and scale (px per GUI unit) for a `w` x `h` surface and a screen with `gw` (see `Screen::gw`).
pub fn panel(gw: usize, w: f32, h: f32) -> (f32, f32, f32) {
    let (pw, ph) = panel_size(gw);
    let k = (h * 0.92 / ph).min(w * 0.92 / pw);
    ((w - pw * k) * 0.5, (h - ph * k) * 0.5, k)
}

/// The 18 x 18 unit cell around the 16 x 16 slot at GUI position `(ux, uy)`, in px: (x, y, size).
pub fn cell(gw: usize, w: f32, h: f32, (ux, uy): (f32, f32)) -> (f32, f32, f32) {
    let (ox, oy, k) = panel(gw, w, h);
    (ox + (ux - 1.0) * k, oy + (uy - 1.0) * k, 18.0 * k)
}

/// The two touch toggles, in the screen's bottom-left corner (outside every panel): `which` 0 = shift-click, 1 = "place one" (this
/// port has no right mouse button or shift key). Same 18-unit size as a slot cell. UNVERIFIED: position and toggles are invented for touch.
pub fn toggle(gw: usize, w: f32, h: f32, which: usize) -> (f32, f32, f32) {
    let (_, _, k) = panel(gw, w, h);
    let s = 18.0 * k;
    (s * 0.4 + which as f32 * s * 1.2, h - s * 1.4, s)
}

fn inside((x, y, s): (f32, f32, f32), px: f32, py: f32) -> bool {
    px >= x && px < x + s && py >= y && py < y + s
}

/// Which slot a touch at `(px, py)` is on.
pub fn slot_at(gw: usize, w: f32, h: f32, px: f32, py: f32) -> Option<SlotId> {
    layout(gw).into_iter().find(|&(_, ux, uy)| inside(cell(gw, w, h, (ux, uy)), px, py)).map(|s| s.0)
}

pub fn on_mode_button(gw: usize, w: f32, h: f32, px: f32, py: f32) -> bool {
    inside(toggle(gw, w, h, 1), px, py)
}

pub fn on_shift_button(gw: usize, w: f32, h: f32, px: f32, py: f32) -> bool {
    inside(toggle(gw, w, h, 0), px, py)
}

pub fn in_panel(gw: usize, w: f32, h: f32, px: f32, py: f32) -> bool {
    let (ox, oy, k) = panel(gw, w, h);
    let (pw, ph) = panel_size(gw);
    px >= ox && px < ox + pw * k && py >= oy && py < oy + ph * k
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every slot of `layout` sits on a frame of the pack's panel (`assets/containers.rgba`, 256 rows per panel): its left edge, one texel
    /// out, is the frame's dark line. The 2x2 screen once used b1.7.3's positions on the modern panel, and taps missed the drawn slots.
    #[test]
    fn slots_sit_on_the_panels_frames() {
        let px = |x: f32, y: f32, row: usize| crate::render::hud::CONTAINERS[((row + y as usize) * 256 + x as usize) * 4];
        for (gw, row) in [(2, 0), (3, 256), (0, 512)] {
            for (id, x, y) in layout(gw) {
                let big = matches!(id, SlotId::Furn(2)) || (gw == 3 && id == SlotId::Result); // the 26 x 26 output frames sit 4 units out
                let edge = if big { px(x - 5.0, y + 8.0, row) } else { px(x - 1.0, y + 8.0, row) };
                assert_eq!(edge, 55, "gw {gw} slot {id:?} at ({x}, {y})");
            }
        }
    }

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
        let ring = |ingot| [(1, ingot), (3, ingot), (5, ingot), (7, ingot), (4, 331)];
        assert_eq!(out(&ring(265), 3), Some((345, 1)), "compass");
        assert_eq!(out(&ring(266), 3), Some((347, 1)), "clock");
        let paper_ring = |x: u16| [(0, 339), (1, 339), (2, 339), (3, 339), (4, x), (5, 339), (6, 339), (7, 339), (8, 339)];
        assert_eq!(out(&paper_ring(345), 3), Some((358, 1)), "map: 8 paper round a compass");
        assert_eq!(out(&paper_ring(331), 3), None, "redstone in the middle is no map");
        assert_eq!(out(&paper_ring(345)[1..], 3), None, "a gap in the ring");
        assert_eq!(out(&[(0, 54), (2, 328)], 2), Some((342, 1)), "minecart with chest, in a 2x2");
        assert_eq!(out(&[(0, 328), (2, 54)], 2), None, "chest under the cart is nothing");
        assert_eq!(out(&[(0, 61), (2, 328)], 2), Some((343, 1)), "minecart with furnace, furnace on top");
        assert_eq!(out(&[(0, 328), (2, 61)], 2), None, "a furnace under the cart is nothing (it was shapeless before)");
        assert_eq!(out(&[(0, 61), (1, 328)], 2), None, "side by side is nothing");
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
        s.click(SlotId::Furn(2), false, &mut inv, Some(&mut f.slots));
        assert_eq!((s.cursor, f.slots[2]), (Some(stack(3, 1)), Some(stack(265, 5))));
        s.cursor = None;
        s.click(SlotId::Furn(2), true, &mut inv, Some(&mut f.slots));
        assert_eq!((s.cursor, f.slots[2]), (Some(stack(265, 3)), Some(stack(265, 2))));
        s.click(SlotId::Furn(0), false, &mut inv, Some(&mut f.slots)); // the input slot takes anything
        assert_eq!(f.slots[0], Some(stack(265, 3)));
    }

    /// `ContainerChest`: clicks reach the chest's slots (the lower half of a large chest too), the player's slots sit below
    /// them as in the Java constructor, and the panel is `114 + 18 * rows` tall with every slot findable at its own centre.
    #[test]
    fn chest_screen_clicks_and_layout() {
        let mut inv = Inventory::default();
        let mut s = Screen::at_chest(vec![(0, 64, 0), (1, 64, 0)]);
        let mut ext = vec![None; 54];
        ext[30] = Some(stack(5, 7));
        inv.slots[0] = Some(stack(5, 60));
        s.click(SlotId::Chest(30), false, &mut inv, Some(&mut ext));
        assert_eq!((s.gw, s.cursor, ext[30]), (54, Some(stack(5, 7)), None));
        s.click(SlotId::Inv(0), false, &mut inv, Some(&mut ext)); // tops up to 64, 3 stay on the cursor
        assert_eq!((inv.slots[0], s.cursor), (Some(stack(5, 64)), Some(stack(5, 3))));
        s.click(SlotId::Chest(2), true, &mut inv, Some(&mut ext)); // right click puts one
        assert_eq!((ext[2], s.cursor), (Some(stack(5, 1)), Some(stack(5, 2))));
        s.click(SlotId::Chest(0), false, &mut inv, None); // no chest behind the screen: nothing moves
        assert_eq!(s.cursor, Some(stack(5, 2)));

        let l = layout(54);
        assert_eq!(l.len(), 54 + 27 + 9);
        assert_eq!((panel_size(27).1, panel_size(54).1, panel_size(0).1), (168.0, 222.0, 166.0));
        let y = |gw, id| layout(gw).into_iter().find(|e| e.0 == id).map(|e| e.2);
        assert_eq!((y(27, SlotId::Inv(0)), y(54, SlotId::Inv(0)), y(54, SlotId::Inv(9)), y(54, SlotId::Chest(53))), (Some(143.0), Some(197.0), Some(139.0), Some(108.0)));
        let d = layout(9);
        assert_eq!((d.len(), d[0], d[8]), (45, (SlotId::Chest(0), 62.0, 17.0), (SlotId::Chest(8), 98.0, 53.0)), "dispenser: 3 x 3 at (62, 17), the inventory as in the workbench");
        for gw in [27, 54, 81] {
            for (id, ux, uy) in layout(gw) {
                let (x, y, sz) = cell(gw, 800.0, 400.0, (ux, uy));
                assert_eq!(slot_at(gw, 800.0, 400.0, x + sz / 2.0, y + sz / 2.0), Some(id));
                assert!(in_panel(gw, 800.0, 400.0, x + sz / 2.0, y + sz / 2.0));
            }
        }
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

    /// RecipesDyes / Ingots / Armor / Food, armor slots and the armor value.
    #[test]
    fn table_fills() {
        let st = |id, damage| ItemStack { id, count: 1, damage };
        let find_in = |cells: &[(usize, ItemStack)], gw| {
            let mut g = [None; 9];
            cells.iter().for_each(|&(i, s)| g[i] = Some(s));
            find(&g, gw).map(|s| (s.id, s.count, s.damage))
        };
        assert_eq!(find_in(&[(0, st(351, 1)), (3, st(35, 0))], 2), Some((35, 1, 14)), "red dye + white wool = red wool");
        assert_eq!(find_in(&[(0, st(351, 1)), (1, st(35, 5))], 2), None, "coloured wool is not white wool");
        assert_eq!(find_in(&[(2, st(351, 4)), (1, st(351, 1))], 2), Some((351, 2, 5)), "lapis + red = purple, any cells");
        assert_eq!(find_in(&[(0, st(352, 0))], 2), Some((351, 3, 15)), "bone meal");
        assert_eq!(find_in(&[(0, st(22, 0))], 2), Some((351, 9, 4)), "lapis block -> 9 lapis");
        assert_eq!(find_in(&(0..9).map(|i| (i, st(351, 4))).collect::<Vec<_>>(), 3), Some((22, 1, 0)));
        assert_eq!(find_in(&[(0, st(296, 0)), (1, st(351, 3)), (2, st(296, 0))], 3), Some((357, 8, 0)), "cookie");
        assert_eq!(find_in(&[(0, st(296, 0)), (1, st(351, 4)), (2, st(296, 0))], 3), None, "cookie needs cocoa");
        assert_eq!(find_in(&[(0, st(265, 0)), (2, st(265, 0)), (3, st(265, 0)), (5, st(265, 0))], 3), Some((309, 1, 0)), "iron boots");
        assert_eq!(find_in(&(0..9).filter(|i| *i != 4).map(|i| (i, st(266, 0))).collect::<Vec<_>>(), 3), None, "no gold ring recipe");
        // ItemArmor: durability, SlotArmor, getTotalArmorValue.
        assert_eq!((max_damage(298), max_damage(311), max_damage(314)), (Some(33), Some(384), Some(66)));
        let mut inv = Inventory::default();
        let mut s = Screen::new(2);
        s.cursor = Some(st(298, 0)); // leather cap: only on the head slot (armorInventory[3])
        s.click(SlotId::Inv(MAIN), false, &mut inv, None);
        assert!(inv.slots[MAIN].is_none() && s.cursor.is_some());
        s.click(SlotId::Inv(MAIN + 3), false, &mut inv, None);
        assert!(inv.slots[MAIN + 3].is_some() && s.cursor.is_none());
        assert_eq!(armor_value(&inv.slots[MAIN..]), 3); // one new cap: (3 - 1) * 33 / 33 + 1
        inv.slots[MAIN + 2] = Some(st(311, 0)); // diamond chestplate
        assert_eq!(armor_value(&inv.slots[MAIN..]), 11); // (3 + 8 - 1) * 417 / 417 + 1
        inv.damage(MAIN + 3, 40); // past 33 uses: the cap breaks
        assert!(inv.slots[MAIN + 3].is_none() && inv.slots[MAIN + 2].is_some());
        let mut v = crate::world::vitals::Vitals::default();
        v.armor = 5;
        assert!(v.hurt(10) && v.health == 12 && v.wear == 10, "10 * 20 / 25 = 8, remainder 0");
    }

    /// The three rail recipes: 16 rails from 6 iron and a stick, 6 powered from 6 gold, a stick and redstone, 6 detector rails.
    #[test]
    fn rail_recipes_match_java() {
        let st = |id| Some(ItemStack { id, count: 1, damage: 0 });
        let grid = |ring: u16, mid: u16, bottom: u16| {
            let mut g = [None; 9];
            for i in [0, 2, 3, 5, 6, 8] {
                g[i] = st(ring);
            }
            g[4] = st(mid);
            if bottom != 0 {
                g[7] = st(bottom);
                g[6] = st(ring);
            }
            find(&g, 3).map(|s| (s.id, s.count))
        };
        assert_eq!(grid(265, 280, 0), Some((66, 16)), "rails");
        assert_eq!(grid(266, 280, 331), Some((27, 6)), "powered rails");
        assert_eq!(grid(265, 70, 331), Some((28, 6)), "detector rails");
    }

    /// The plain `CraftingManager` recipes added for blocks and items that already work.
    #[test]
    fn plain_recipes_match_java() {
        let st = |id, damage| ItemStack { id, count: 1, damage };
        let row = |id| (0..3).map(|i| (i, st(id, 0))).collect::<Vec<_>>();
        let find_in = |cells: &[(usize, ItemStack)], gw| {
            let mut g = [None; 9];
            cells.iter().for_each(|&(i, s)| g[i] = Some(s));
            find(&g, gw).map(|s| (s.id, s.count, s.damage))
        };
        assert_eq!(find_in(&row(338), 3), Some((339, 3, 0)), "paper");
        assert_eq!(find_in(&[(1, st(338, 0))], 3), Some((353, 1, 0)), "sugar");
        assert_eq!(find_in(&[(0, st(339, 0)), (3, st(339, 0)), (6, st(339, 0))], 3), Some((340, 1, 0)), "book");
        assert_eq!(find_in(&row(4), 3), Some((44, 3, 3)), "cobblestone slab");
        assert_eq!(find_in(&row(1), 3), Some((44, 3, 0)), "stone slab");
        assert_eq!(find_in(&row(24), 3), Some((44, 3, 1)), "sandstone slab");
        assert_eq!(find_in(&row(5), 3), Some((44, 3, 2)), "wooden slab");
        let shelf: Vec<_> = (0..9).map(|i| (i, st(if (3..6).contains(&i) { 340 } else { 5 }, 0))).collect();
        assert_eq!(find_in(&shelf, 3), Some((47, 1, 0)), "bookshelf");
        assert_eq!(find_in(&[(0, st(86, 0)), (3, st(50, 0))], 3), Some((91, 1, 0)), "jack-o'-lantern");
        let tnt: Vec<_> = (0..9).map(|i| (i, st(if i % 2 == 0 { 289 } else { 12 }, 0))).collect();
        assert_eq!(find_in(&tnt, 3), Some((46, 1, 0)), "TNT: gunpowder on the corners and the centre, sand between");
        let sign: Vec<_> = (0..9).map(|i| (i, st(if i < 6 { 5 } else if i == 7 { 280 } else { 0 }, 0))).filter(|c| c.1.id != 0).collect();
        assert_eq!(find_in(&sign, 3), Some((323, 1, 0)), "sign: 6 planks over a stick");
        let frame: Vec<_> = (0..9).map(|i| (i, st(if i == 4 { 35 } else { 280 }, 0))).collect();
        assert_eq!(find_in(&frame, 3), Some((321, 1, 0)), "painting: 8 sticks round a wool");
        assert_eq!(find_in(&[(0, st(280, 0)), (3, st(4, 0))], 3), Some((69, 1, 0)), "lever: stick over cobblestone");
        let repeater: Vec<_> = [(0, 76), (1, 331), (2, 76), (3, 1), (4, 1), (5, 1)].iter().map(|&(i, id)| (i, st(id, 0))).collect();
        assert_eq!(find_in(&repeater, 3), Some((356, 1, 0)), "repeater: two torches and dust over three stone");
        let piston: Vec<_> = [(0, 5), (1, 5), (2, 5), (3, 4), (4, 265), (5, 4), (6, 4), (7, 331), (8, 4)].iter().map(|&(i, id)| (i, st(id, 0))).collect();
        assert_eq!(find_in(&piston, 3), Some((33, 1, 0)), "piston: planks, cobblestone round an iron ingot and redstone");
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

    #[test]
    fn shift_click_and_new_recipes() {
        let st = |id: u16, count: u8| ItemStack { id, count, damage: 0 };
        let (mut inv, mut s) = (Inventory::default(), Screen::new(2));
        inv.slots[0] = Some(st(298, 1)); // leather cap in the hotbar -> head slot
        s.shift_click(SlotId::Inv(0), &mut inv, None);
        assert!(inv.slots[0].is_none() && inv.slots[MAIN + 3].is_some_and(|a| a.id == 298));
        inv.slots[1] = Some(st(4, 10)); // hotbar -> main
        s.shift_click(SlotId::Inv(1), &mut inv, None);
        assert!(inv.slots[1].is_none() && inv.slots[9].is_some_and(|a| a.count == 10));
        s.shift_click(SlotId::Inv(9), &mut inv, None); // and back
        assert!(inv.slots[0].is_some_and(|a| a.count == 10));
        s.grid[0] = Some(st(17, 3)); // logs -> planks x4 per log, three times
        s.shift_click(SlotId::Result, &mut inv, None);
        assert!(s.grid[0].is_none() && inv.slots.iter().flatten().filter(|a| a.id == 5).map(|a| a.count as u32).sum::<u32>() == 12);
        // furnace: raw fish to the input, coal to the fuel
        let mut f = [Some(st(349, 1)), None, None];
        let mut fs = Screen::at_furnace((0, 0, 0));
        inv.slots[2] = Some(st(263, 5));
        fs.shift_click(SlotId::Inv(2), &mut inv, Some(&mut f));
        assert_eq!(f[1].map(|a| a.id), Some(263));
        // recipes
        let find = |rows: &[(usize, u16)], gw| { let mut g = [None; 9]; for &(i, id) in rows { g[i] = Some(st(id, 1)); } find(&g, gw).map(|r| (r.id, r.count)) };
        assert_eq!(find(&[(0, 296), (1, 296), (2, 296)], 3), Some((297, 1)), "bread");
        assert_eq!(find(&[(0, 318), (3, 280), (6, 288)], 3), Some((262, 4)), "arrow");
        assert_eq!(find(&[(1, 280), (2, 287), (3, 280), (5, 287), (7, 280), (8, 287)], 3), Some((261, 1)), "bow");
        assert_eq!(find(&[(2, 280), (4, 280), (5, 287), (6, 280), (8, 287)], 3), Some((346, 1)), "rod");
    }

    #[test]
    fn cake_takes_milk_sugar_wheat_egg_and_gives_the_buckets_back() {
        let st = |id: u16| ItemStack { id, count: 1, damage: 0 };
        let (mut inv, mut s) = (Inventory::default(), Screen::new(3));
        for (i, id) in [335, 335, 335, 353, 344, 353, 296, 296, 296].into_iter().enumerate() {
            s.grid[i] = Some(st(id));
        }
        s.click(SlotId::Result, false, &mut inv, None);
        assert_eq!(s.cursor.map(|c| (c.id, c.count)), Some((354, 1)));
        assert!(s.grid[..3].iter().all(|g| g.is_some_and(|b| b.id == 325)), "three empty buckets");
        assert!(s.grid[3..].iter().all(Option::is_none));
        assert_eq!(crate::world::items::max_stack(354), 1);
    }
}
