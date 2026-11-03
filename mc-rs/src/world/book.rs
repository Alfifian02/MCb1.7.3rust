//! The recipe book of the satchel (`satchel.rs`). NOT b1.7.3: there the crafting grid is all there is, and the player has to know the
//! recipes. Everything here is pure (no GPU, no `lib.rs` state) and reads the one list of recipes the crafting grid uses
//! (`craft::recipes()`), so a recipe added there shows up in the book with no line more.
//!
//! - `entries()`: every recipe of `CraftingManager` that this port has, plus every smelting (`FurnaceRecipes`), sorted into six
//!   categories (`Cat`); the left page of the book lists them.
//! - `craftable` / `missing` / `need`: what the bag holds against what a recipe wants (green / red on the card).
//! - `fill`: a tap on a recipe puts one set of its ingredients into the grid (what lay in the grid goes back to the bag first); a tap
//!   on the recipe the grid already holds adds one more set to it instead, until the bag runs dry or a cell is a full stack;
//!   `craft_all` is a long press on the result: fill, take, fill, take, until the bag runs dry or full.
//! - `Notes`: the items the player has ever held (so unknown recipes can show as a silhouette) and the one recipe pinned to the HUD;
//!   a 69 byte `book` file in the world folder.
//! ponytail: the pin is an index into `entries()`, so a release that adds recipes can shift it; the file is checked against the list's
//! length and a stale pin is dropped, not repaired.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::world::chest::{Chest, Pos};
use crate::world::craft::{self, Screen, SlotId};
use crate::world::items::{max_stack, Inventory, ItemStack, MAIN};
use crate::world::pack;

/// The six pages of the left side of the book.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cat {
    Tools,
    Food,
    Blocks,
    Redstone,
    Other,
    Furnace,
}

pub const CATS: [Cat; 6] = [Cat::Tools, Cat::Food, Cat::Blocks, Cat::Redstone, Cat::Other, Cat::Furnace];

impl Cat {
    pub fn label(self) -> &'static str {
        match self {
            Cat::Tools => "Tools & gear",
            Cat::Food => "Food",
            Cat::Blocks => "Blocks",
            Cat::Redstone => "Redstone & rails",
            Cat::Other => "Other",
            Cat::Furnace => "Furnace",
        }
    }
}

/// Where an entry comes from: the `i`-th recipe of `craft::recipes()`, or the smelting of an item (its id).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Src {
    Craft(usize),
    Smelt(u16),
}

pub struct Entry {
    pub src: Src,
    pub out: ItemStack,
    /// The ingredient codes (`craft::code`) with how many of each, sorted by code.
    pub need: Vec<(u16, u8)>,
    pub cat: Cat,
    /// The smallest grid it fits: 2 (the inventory's), 3 (a workbench's), 0 for a smelting (no grid, a furnace).
    pub grid: usize,
}

/// What a block of redstone and transport is made of: lever, torch, repeater, button, plates, jukebox, note block, dispenser, the two
/// pistons, the three rails and the three carts.
const REDSTONE: [u16; 17] = [23, 25, 27, 28, 29, 33, 66, 69, 70, 72, 76, 77, 84, 328, 342, 343, 356];

fn category(out: ItemStack) -> Cat {
    if REDSTONE.contains(&out.id) {
        return Cat::Redstone;
    }
    match pack::category(out.id) {
        pack::Cat::Tools => Cat::Tools,
        pack::Cat::Food => Cat::Food,
        _ if out.id < 256 => Cat::Blocks,
        _ => Cat::Other,
    }
}

/// Every recipe and smelting, category by category, each by result id and damage.
pub fn entries() -> &'static [Entry] {
    static E: OnceLock<Vec<Entry>> = OnceLock::new();
    E.get_or_init(|| {
        let mut v: Vec<Entry> = craft::recipes()
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let mut need: Vec<(u16, u8)> = Vec::new();
                for &c in r.pattern().iter().filter(|&&c| c != 0) {
                    match need.iter_mut().find(|n| n.0 == c) {
                        Some(n) => n.1 += 1,
                        None => need.push((c, 1)),
                    }
                }
                need.sort_unstable();
                let grid = match r.shape() {
                    Some((w, h)) if w <= 2 && h <= 2 => 2,
                    None if r.pattern().len() <= 4 => 2,
                    _ => 3,
                };
                Entry { src: Src::Craft(i), out: r.out(), need, cat: category(r.out()), grid }
            })
            .collect();
        v.extend((1..512u16).filter_map(|id| craft::smelting(id).map(|out| Entry { src: Src::Smelt(id), out, need: vec![(id, 1)], cat: Cat::Furnace, grid: 0 })));
        v.sort_by_key(|e| (CATS.iter().position(|&c| c == e.cat), e.out.id, e.out.damage));
        v
    })
}

/// The entries of one category, as indices into `entries()`.
pub fn in_cat(cat: Cat) -> Vec<usize> {
    // The recipes of the 20 Cortes weapons are listed only in a world with the Immersive Pack ON (`combat::rules`, NOT b1.7.3).
    let combat = super::combat::rules::on();
    entries().iter().enumerate().filter(|(_, e)| e.cat == cat && (combat || !super::combat::rules::is_weapon(e.out.id))).map(|(i, _)| i).collect()
}

/// How many of the ingredient `code` the bag (main inventory and quick belt), the grid and the chests near the workbench (`Inventory::near`) hold together.
pub fn have(inv: &Inventory, grid: &[Option<ItemStack>; 9], code: u16) -> u32 {
    let near: u32 = inv.near.iter().filter(|n| n.0 == code).map(|n| n.1).sum();
    near + inv.slots[..MAIN].iter().chain(grid.iter()).flatten().filter(|s| craft::code(s.id, s.damage) == code).map(|s| u32::from(s.count)).sum::<u32>()
}

// ---- Craft from a nearby chest (NOT b1.7.3, Immersive Pack worlds, UNVERIFIED) ----

/// A chest feeds the book when it is within this many blocks of the workbench on each axis.
pub const NEAR: i32 = 3;

/// The chests of `chests` near `bench`, in a fixed order (so a pull always starts from the same one).
fn near_chests(bench: Pos, chests: &HashMap<Pos, Chest>) -> Vec<Pos> {
    let mut v: Vec<Pos> = chests.keys().copied().filter(|p| (p.0 - bench.0).abs() <= NEAR && (p.1 - bench.1).abs() <= NEAR && (p.2 - bench.2).abs() <= NEAR).collect();
    v.sort_unstable();
    v
}

/// What the chests near `bench` hold: (ingredient code, count) for each code, for `Inventory::near`.
pub fn near_stock(bench: Pos, chests: &HashMap<Pos, Chest>) -> Vec<(u16, u32)> {
    let mut stock: Vec<(u16, u32)> = Vec::new();
    for s in near_chests(bench, chests).iter().filter_map(|p| chests.get(p)).flat_map(|c| c.iter().flatten()) {
        let code = craft::code(s.id, s.damage);
        match stock.iter_mut().find(|n| n.0 == code) {
            Some(n) => n.1 += u32::from(s.count),
            None => stock.push((code, u32::from(s.count))),
        }
    }
    stock
}

/// Before a tap on a recipe fills the grid: bring what the bag lacks of each ingredient out of the chests near `bench` into the bag, so `fill`
/// finds it there. The grid is not counted (`fill` stashes it into the bag first), so this may bring a little more than a set; it is all in the
/// bag, nothing is lost. A bag with no room leaves the rest in the chest.
pub fn pull(e: &Entry, inv: &mut Inventory, chests: &mut HashMap<Pos, Chest>, bench: Pos) {
    let order = near_chests(bench, chests);
    for &(code, n) in &e.need {
        let in_bag = |inv: &Inventory| inv.slots[..MAIN].iter().flatten().filter(|s| craft::code(s.id, s.damage) == code).map(|s| u32::from(s.count)).sum::<u32>();
        let mut short = u32::from(n).saturating_sub(in_bag(&*inv));
        for p in &order {
            let Some(chest) = chests.get_mut(p) else { continue };
            for slot in chest.iter_mut() {
                let Some(s) = slot.as_mut().filter(|s| short > 0 && craft::code(s.id, s.damage) == code) else { continue };
                let mut moved = ItemStack { count: short.min(u32::from(s.count)) as u8, ..*s };
                let asked = moved.count;
                inv.add(&mut moved); // what does not fit stays in `moved`
                let took = asked - moved.count;
                s.count -= took;
                short -= u32::from(took);
                if s.count == 0 {
                    *slot = None;
                }
                if moved.count > 0 {
                    return; // the bag is full
                }
            }
        }
    }
}

/// What `e` still lacks: (ingredient code, how many short), nothing short = empty.
pub fn missing(e: &Entry, inv: &Inventory, grid: &[Option<ItemStack>; 9]) -> Vec<(u16, u32)> {
    e.need.iter().filter_map(|&(c, n)| u32::from(n).checked_sub(have(inv, grid, c)).filter(|&s| s > 0).map(|s| (c, s))).collect()
}

/// A grid of width `gw` could make it now: it fits the grid and nothing is short. A smelting is never made in the grid.
pub fn craftable(e: &Entry, gw: usize, inv: &Inventory, grid: &[Option<ItemStack>; 9]) -> bool {
    e.grid != 0 && e.grid <= gw && missing(e, inv, grid).is_empty()
}

/// Something the player can make right now, which the list shows first and lit: a recipe that fits the grid with nothing short, or a
/// smelting whose ingredient the bag holds (the furnace itself is somewhere else).
pub fn ready(e: &Entry, gw: usize, inv: &Inventory, grid: &[Option<ItemStack>; 9]) -> bool {
    if e.grid == 0 { missing(e, inv, grid).is_empty() } else { craftable(e, gw, inv, grid) }
}

/// `in_cat` with the `ready` ones first; each group keeps its order (a stable sort), so a row only moves when the bag changes.
pub fn in_cat_ready(cat: Cat, gw: usize, inv: &Inventory, grid: &[Option<ItemStack>; 9]) -> Vec<usize> {
    let mut v = in_cat(cat);
    v.sort_by_key(|&i| !ready(&entries()[i], gw, inv, grid));
    v
}

/// Put what lies in the grid back into the bag; whatever has no room stays. True when the grid is empty after.
pub fn stash(screen: &mut Screen, inv: &mut Inventory) -> bool {
    for cell in screen.grid.iter_mut() {
        if let Some(mut s) = cell.take() {
            inv.add(&mut s);
            *cell = (s.count > 0).then_some(s);
        }
    }
    screen.grid.iter().all(Option::is_none)
}

/// Where each ingredient code of recipe `ri` lies in a grid `gw` wide (`0` = an empty cell); a shaped recipe keeps its rows, a shapeless
/// one lies in the first cells.
fn pattern_cells(ri: usize, gw: usize) -> [u16; 9] {
    let r = &craft::recipes()[ri];
    let mut cells = [0u16; 9];
    match r.shape() {
        Some((w, h)) => {
            for y in 0..h {
                for x in 0..w {
                    cells[y * gw + x] = r.pattern()[y * w + x];
                }
            }
        }
        None => cells[..r.pattern().len()].copy_from_slice(r.pattern()),
    }
    cells
}

/// The ingredient codes of `e` in the cells of a preview `pg` wide (`0` = empty): a recipe in its places (`pg` at least as wide as the
/// recipe), a smelting's one input in the first cell (`pg` = 1). What the satchel shows as ghosts, red where the bag is short.
pub fn preview(e: &Entry, pg: usize) -> [u16; 9] {
    match e.src {
        Src::Craft(ri) => pattern_cells(ri, pg.clamp(1, 3)),
        Src::Smelt(_) => {
            let mut cells = [0u16; 9];
            cells[0] = e.need.first().map_or(0, |n| n.0);
            cells
        }
    }
}

/// The grid holds exactly this recipe's ingredients in its places, whatever the counts (an empty grid holds none).
fn holds(cells: &[u16; 9], grid: &[Option<ItemStack>; 9]) -> bool {
    cells.iter().any(|&c| c != 0) && cells.iter().zip(grid.iter()).all(|(&c, g)| g.as_ref().map_or(0, |s| craft::code(s.id, s.damage)) == c)
}

/// One more set for a grid that holds `cells` already: each of its cells grows by one, taken from the bag (the main slots first, the
/// quick belt last). Nothing changes, and false, when the bag has run dry of an ingredient or a cell is a full stack.
fn add_set(cells: &[u16; 9], screen: &mut Screen, inv: &mut Inventory) -> bool {
    let (g0, i0) = (screen.grid, inv.slots);
    for (i, &c) in cells.iter().enumerate().filter(|&(_, &c)| c != 0) {
        let slot = (9..MAIN).chain(0..9).find(|&k| inv.slots[k].is_some_and(|s| craft::code(s.id, s.damage) == c));
        let ok = match (slot, screen.grid[i].as_mut()) {
            (Some(k), Some(g)) if g.count < max_stack(g.id) => {
                inv.slots[k] = inv.slots[k].and_then(|s| (s.count > 1).then(|| ItemStack { count: s.count - 1, ..s }));
                g.count += 1;
                true
            }
            _ => false,
        };
        if !ok {
            (screen.grid, inv.slots) = (g0, i0);
            return false;
        }
    }
    true
}

/// A tap on a recipe. When the grid already holds that recipe (a second tap, a third, ...), one more set joins it from the bag (`add_set`).
/// Otherwise the grid is cleared into the bag and one set of ingredients goes from the bag into the recipe's own cells (the main slots
/// first, the quick belt last): another recipe always starts over from one set. Nothing changes, and false, when the entry is a smelting,
/// does not fit the screen's grid, something is short (or the bag has no more of a set to add), or the grid cannot be emptied.
pub fn fill(e: &Entry, screen: &mut Screen, inv: &mut Inventory) -> bool {
    let (gw, Src::Craft(ri)) = (screen.gw, e.src) else { return false };
    if !(gw == 2 || gw == 3) || e.grid > gw {
        return false;
    }
    let cells = pattern_cells(ri, gw);
    if holds(&cells, &screen.grid) {
        return add_set(&cells, screen, inv);
    }
    let (g0, i0) = (screen.grid, inv.slots);
    let undo = |screen: &mut Screen, inv: &mut Inventory| {
        screen.grid = g0;
        inv.slots = i0;
        false
    };
    if !stash(screen, inv) || !missing(e, inv, &screen.grid).is_empty() {
        return undo(screen, inv);
    }
    for (i, &c) in cells.iter().enumerate().filter(|&(_, &c)| c != 0) {
        let Some(slot) = (9..MAIN).chain(0..9).find(|&k| inv.slots[k].is_some_and(|s| craft::code(s.id, s.damage) == c)) else { return undo(screen, inv) };
        let Some(s) = inv.slots[slot].as_mut() else { return undo(screen, inv) };
        let one = ItemStack { count: 1, ..*s };
        s.count -= 1;
        if s.count == 0 {
            inv.slots[slot] = None;
        }
        screen.grid[i] = Some(one);
    }
    if screen.result().is_none() {
        return undo(screen, inv); // the recipe did not match its own pattern: a bug, but never a lost item
    }
    true
}

/// A long press on the result: craft what the grid makes, refill it from the bag with the same recipe and craft again, until the bag
/// has no more ingredients or no room for the result. Returns how many rounds took a result; the grid ends empty when it can be.
pub fn craft_all(screen: &mut Screen, inv: &mut Inventory) -> u32 {
    let Some(first) = screen.result().filter(|_| screen.cursor.is_none()) else { return 0 };
    let mut rounds = 0;
    for _ in 0..256 {
        if screen.result().is_none() {
            let gw = screen.gw;
            let again = entries().iter().find(|e| matches!(e.src, Src::Craft(_)) && e.out.id == first.id && e.out.damage == first.damage && craftable(e, gw, inv, &screen.grid));
            if !again.is_some_and(|e| fill(e, screen, inv)) {
                break;
            }
        }
        let g = screen.grid;
        screen.shift_click(SlotId::Result, inv, None);
        if screen.grid == g {
            break; // no room: nothing was taken
        }
        rounds += 1;
    }
    stash(screen, inv);
    rounds
}

/// Legacy's A on a recipe: one set (or, with `all`, every set the bag can make) goes straight into the bag; the grid is only the machinery
/// and ends empty. Nothing changes, and false, when a stack is on the cursor, the entry is a smelting or does not fit the screen's grid,
/// something is short, or the bag has no room for the result.
pub fn make(e: &Entry, screen: &mut Screen, inv: &mut Inventory, all: bool) -> bool {
    if screen.cursor.is_some() || !stash(screen, inv) || !fill(e, screen, inv) {
        return false;
    }
    if all {
        return craft_all(screen, inv) > 0;
    }
    let before = screen.grid;
    screen.shift_click(SlotId::Result, inv, None);
    let made = screen.grid != before; // no room: nothing was taken, the grid is as it was
    stash(screen, inv);
    made
}

// ---- What the player has seen, and the pinned recipe ----

/// The items the player has ever held (one bit per id below 512) and the recipe pinned to the HUD.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notes {
    seen: [u8; 64],
    pub pinned: Option<usize>,
}

const MAGIC: &[u8; 3] = b"BK1";

impl Default for Notes {
    fn default() -> Self {
        Self { seen: [0; 64], pinned: None }
    }
}

impl Notes {
    pub fn seen(&self, id: u16) -> bool {
        id < 512 && self.seen[usize::from(id) / 8] >> (id % 8) & 1 == 1
    }

    /// Note an item; true when it was new.
    pub fn see(&mut self, id: u16) -> bool {
        let new = id < 512 && !self.seen(id);
        if new {
            self.seen[usize::from(id) / 8] |= 1 << (id % 8);
        }
        new
    }

    /// Note every item in the bag and the grid (once per 20 Hz tick); true when any was new, so the caller saves.
    pub fn observe(&mut self, inv: &Inventory, grid: &[Option<ItemStack>; 9]) -> bool {
        inv.slots.iter().chain(grid.iter()).flatten().fold(false, |any, s| self.see(s.id) | any)
    }

    /// A recipe is known once its result or any of its ingredients has been held; the rest can show as a silhouette.
    pub fn known(&self, e: &Entry) -> bool {
        self.seen(e.out.id) || e.need.iter().any(|&(c, _)| self.seen(craft::unpack(c).0))
    }

    pub fn encode(&self) -> Vec<u8> {
        let pin = self.pinned.map_or(u16::MAX, |p| p as u16);
        let mut b = MAGIC.to_vec();
        b.extend(pin.to_le_bytes());
        b.extend(self.seen);
        b
    }

    /// `None` for anything that is not an `encode` (wrong size or magic). A pin past the end of the list is dropped.
    pub fn decode(b: &[u8]) -> Option<Self> {
        if b.len() != 69 || !b.starts_with(MAGIC) {
            return None;
        }
        let pin = usize::from(u16::from_le_bytes([b[3], b[4]]));
        Some(Self { seen: std::array::from_fn(|i| b[5 + i]), pinned: Some(pin).filter(|&p| p < entries().len()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(id: u16, count: u8) -> ItemStack {
        ItemStack { id, count, damage: 0 }
    }

    fn entry(out: u16, damage: u16, grid: usize) -> &'static Entry {
        entries().iter().find(|e| e.out.id == out && e.out.damage == damage && e.grid == grid && matches!(e.src, Src::Craft(_))).expect("entry")
    }

    fn count(inv: &Inventory, id: u16) -> u32 {
        inv.slots.iter().flatten().filter(|s| s.id == id).map(|s| u32::from(s.count)).sum()
    }

    // Craft from a nearby chest: only a chest within `NEAR` blocks of the bench is counted, a pull brings what the bag lacks (and no more of the far chest),
    // and a bag with no room takes nothing and loses nothing.
    #[test]
    fn a_chest_near_the_bench_lends_its_ingredients_to_the_book() {
        let planks = ItemStack { id: 5, count: 7, damage: 0 };
        let (mut near, mut far): (Chest, Chest) = (Default::default(), Default::default());
        (near[0], far[0]) = (Some(planks), Some(ItemStack { count: 50, ..planks }));
        let mut chests: HashMap<Pos, Chest> = HashMap::from([((1, 0, 1), near), ((9, 0, 0), far)]);
        let bench = (0, 0, 0);
        let stock = near_stock(bench, &chests);
        assert_eq!(stock, vec![(craft::code(5, 0), 7)], "the chest 9 blocks off is not counted");
        let table = entry(58, 0, 2); // a workbench: 4 planks
        let mut inv = Inventory { near: stock, ..Default::default() };
        assert!(craftable(table, 2, &inv, &[None; 9]), "the bag is empty and the chest has the planks");
        inv.slots[0] = Some(ItemStack { count: 1, ..planks });
        pull(table, &mut inv, &mut chests, bench);
        assert_eq!(count(&inv, 5), 4, "1 in the bag and 3 from the chest");
        assert_eq!(chests[&(1, 0, 1)][0].map(|s| s.count), Some(4));
        assert_eq!(chests[&(9, 0, 0)][0].map(|s| s.count), Some(50));
        let mut full = Inventory::default();
        full.slots[..MAIN].iter_mut().for_each(|s| *s = Some(ItemStack { id: 4, count: 64, damage: 0 }));
        pull(table, &mut full, &mut chests, bench);
        assert_eq!(chests[&(1, 0, 1)][0].map(|s| s.count), Some(4), "no room: nothing leaves the chest");
    }

    #[test]
    fn every_recipe_and_smelting_is_listed_in_a_category() {
        let e = entries();
        assert_eq!(e.iter().filter(|e| matches!(e.src, Src::Craft(_))).count(), craft::recipes().len());
        assert!(e.iter().any(|e| e.src == Src::Smelt(15) && e.out.id == 265 && e.cat == Cat::Furnace), "iron ore -> iron ingot");
        assert!(CATS.iter().all(|&c| !in_cat(c).is_empty()), "no empty page");
        assert!(e.windows(2).all(|w| CATS.iter().position(|&c| c == w[0].cat) <= CATS.iter().position(|&c| c == w[1].cat)), "sorted by page");
        assert_eq!((entry(5, 0, 2).cat, entry(270, 0, 3).cat, entry(297, 0, 3).cat, entry(33, 0, 3).cat), (Cat::Blocks, Cat::Tools, Cat::Food, Cat::Redstone));
        assert_eq!(entry(58, 0, 2).need, vec![(5, 4)], "a workbench is four planks and fits the 2x2");
        assert_eq!(entry(270, 0, 3).need, vec![(5, 3), (280, 2)], "a wooden pickaxe wants the 3x3");
    }

    #[test]
    fn a_tap_fills_the_grid_and_the_result_shows() {
        let (mut inv, mut s) = (Inventory::default(), Screen::new(2));
        inv.slots[10] = Some(stack(17, 3));
        assert!(craftable(entry(5, 0, 2), 2, &inv, &s.grid));
        assert!(fill(entry(5, 0, 2), &mut s, &mut inv));
        assert_eq!((s.grid[0].map(|g| g.id), count(&inv, 17)), (Some(17), 2));
        assert_eq!(s.result().map(|r| (r.id, r.count)), Some((5, 4)), "one log makes four planks");
        // A second recipe takes the first one's items back first.
        inv.slots[11] = Some(stack(5, 2));
        assert!(!fill(entry(58, 0, 2), &mut s, &mut inv), "two planks are not a workbench");
        assert_eq!(s.grid[0].map(|g| g.id), Some(17), "a refused fill leaves the grid as it was");
        inv.slots[11] = Some(stack(5, 4));
        assert!(fill(entry(58, 0, 2), &mut s, &mut inv));
        assert_eq!((count(&inv, 17), count(&inv, 5), s.result().map(|r| r.id)), (3, 0, Some(58)));
    }

    #[test]
    fn tapping_the_same_recipe_again_adds_a_set_and_another_recipe_starts_over() {
        let (mut inv, mut s) = (Inventory::default(), Screen::new(2));
        inv.slots[10] = Some(stack(17, 3));
        let planks = entry(5, 0, 2);
        assert!(fill(planks, &mut s, &mut inv));
        assert!(fill(planks, &mut s, &mut inv) && fill(planks, &mut s, &mut inv), "taps two and three each add a set");
        assert_eq!((s.grid[0].map(|g| g.count), count(&inv, 17)), (Some(3), 0));
        assert_eq!(s.result().map(|r| (r.id, r.count)), Some((5, 4)), "the result is still one craft; taking it uses one set");
        assert!(!fill(planks, &mut s, &mut inv), "the bag is dry: a fourth tap does nothing");
        assert_eq!((s.grid[0].map(|g| g.count), count(&inv, 17)), (Some(3), 0));
        // Another recipe: the whole stack goes back to the bag and it starts from one set.
        inv.slots[11] = Some(stack(5, 4));
        assert!(fill(entry(58, 0, 2), &mut s, &mut inv));
        assert_eq!((count(&inv, 17), s.result().map(|r| r.id)), (3, Some(58)));
        assert!(s.grid.iter().flatten().all(|g| g.count == 1));
        // A cell that is a full stack takes no more.
        let mut inv = Inventory::default();
        let mut s = Screen::new(2);
        inv.slots[10] = Some(stack(17, 64));
        assert!(fill(planks, &mut s, &mut inv));
        s.grid[0] = Some(stack(17, 64));
        assert!(!fill(planks, &mut s, &mut inv) && s.grid[0].map(|g| g.count) == Some(64) && count(&inv, 17) == 63);
    }

    #[test]
    fn a_workbench_recipe_needs_the_3x3_grid() {
        let mut inv = Inventory::default();
        inv.slots[9] = Some(stack(5, 3));
        inv.slots[10] = Some(stack(280, 2));
        let pick = entry(270, 0, 3);
        assert!(craftable(pick, 3, &inv, &[None; 9]) && !craftable(pick, 2, &inv, &[None; 9]));
        let mut small = Screen::new(2);
        assert!(!fill(pick, &mut small, &mut inv) && count(&inv, 5) == 3, "the 2x2 grid refuses it and nothing moves");
        let mut bench = Screen::new(3);
        assert!(fill(pick, &mut bench, &mut inv));
        assert_eq!((bench.result().map(|r| r.id), count(&inv, 5), count(&inv, 280)), (Some(270), 0, 0));
    }

    #[test]
    fn what_is_short_is_named_and_dyes_keep_their_colour() {
        let mut inv = Inventory::default();
        inv.slots[9] = Some(stack(5, 1));
        assert_eq!(missing(entry(58, 0, 2), &inv, &[None; 9]), vec![(5, 3)]);
        // Red dye (351 damage 1) + white wool = red wool (35 damage 14): shapeless, and the colour decides.
        inv.slots[10] = Some(ItemStack { id: 351, count: 1, damage: 1 });
        inv.slots[11] = Some(stack(35, 1));
        let red = entry(35, 14, 2);
        let mut s = Screen::new(2);
        assert!(fill(red, &mut s, &mut inv));
        assert_eq!(s.result().map(|r| (r.id, r.damage)), Some((35, 14)));
        let mut green = Inventory::default();
        green.slots[9] = Some(ItemStack { id: 351, count: 1, damage: 2 });
        green.slots[10] = Some(stack(35, 1));
        assert!(!craftable(red, 2, &green, &[None; 9]), "green dye is not red dye");
    }

    #[test]
    fn a_long_press_crafts_until_the_bag_runs_dry() {
        let (mut inv, mut s) = (Inventory::default(), Screen::new(2));
        inv.slots[9] = Some(stack(17, 3));
        assert!(fill(entry(5, 0, 2), &mut s, &mut inv));
        assert_eq!(craft_all(&mut s, &mut inv), 3);
        assert_eq!((count(&inv, 17), count(&inv, 5)), (0, 12), "three logs, twelve planks");
        assert!(s.grid.iter().all(Option::is_none));
        // No room for the result (a second log keeps its slot taken): nothing is lost, it stops, the log goes back to the bag.
        let mut full = Inventory::default();
        full.slots[..MAIN].iter_mut().for_each(|c| *c = Some(stack(3, 64)));
        full.slots[9] = Some(stack(17, 2));
        let mut t = Screen::new(2);
        assert!(fill(entry(5, 0, 2), &mut t, &mut full));
        assert_eq!(craft_all(&mut t, &mut full), 0);
        assert_eq!((count(&full, 17), count(&full, 5)), (2, 0));
    }

    #[test]
    fn what_can_be_made_comes_first_and_keeps_its_order() {
        let mut inv = Inventory::default();
        let blocks = in_cat(Cat::Blocks);
        let none = in_cat_ready(Cat::Blocks, 2, &inv, &[None; 9]);
        assert_eq!(none, blocks, "nothing in the bag: the page keeps its own order");
        inv.slots[10] = Some(stack(17, 3));
        let planks = entries().iter().position(|e| e.out.id == 5 && e.grid == 2 && matches!(e.src, Src::Craft(_))).expect("planks");
        let now = in_cat_ready(Cat::Blocks, 2, &inv, &[None; 9]);
        assert_eq!((now.len(), now[0]), (blocks.len(), planks), "logs: planks lead");
        assert!(now[1..].iter().all(|&i| !ready(&entries()[i], 2, &inv, &[None; 9])) && now[1..].iter().copied().eq(blocks.iter().copied().filter(|&i| i != planks)));
        // A tap that moves the logs into the grid does not make the recipe drop: the grid counts too.
        let mut s = Screen::new(2);
        assert!(fill(&entries()[planks], &mut s, &mut inv));
        assert_eq!(in_cat_ready(Cat::Blocks, 2, &inv, &s.grid)[0], planks);
        // A 3x3 recipe is not ready at the 2x2 grid, a smelting is ready with its ore in the bag.
        let mut rich = Inventory::default();
        rich.slots[9] = Some(stack(5, 3));
        rich.slots[10] = Some(stack(280, 2));
        let pick = entries().iter().position(|e| e.out.id == 270 && e.grid == 3).expect("wooden pickaxe");
        assert!(ready(&entries()[pick], 3, &rich, &[None; 9]) && !ready(&entries()[pick], 2, &rich, &[None; 9]));
        rich.slots[11] = Some(stack(15, 1));
        let smelt = entries().iter().position(|e| e.src == Src::Smelt(15)).expect("iron ore");
        assert_eq!(in_cat_ready(Cat::Furnace, 2, &rich, &[None; 9])[0], smelt);
    }

    /// `make` crafts into the bag without a grid to tend: one set, then all that is left, and the grid is empty after each.
    #[test]
    fn make_crafts_straight_into_the_bag_and_leaves_the_grid_empty() {
        let planks = entry(5, 0, 2);
        let mut inv = Inventory::default();
        inv.slots[10] = Some(stack(17, 3));
        let mut s = Screen::new(2);
        assert!(make(planks, &mut s, &mut inv, false));
        assert!(s.grid.iter().all(Option::is_none), "the grid is machinery: empty after");
        assert_eq!((count(&inv, 17), count(&inv, 5)), (2, u32::from(planks.out.count)));
        assert!(make(planks, &mut s, &mut inv, true));
        assert_eq!((count(&inv, 17), count(&inv, 5)), (0, 3 * u32::from(planks.out.count)));
        assert!(!make(planks, &mut s, &mut inv, false), "nothing left to make it from");
        // A stack on the cursor, or a smelting: nothing changes.
        inv.slots[11] = Some(stack(17, 1));
        s.cursor = Some(stack(3, 1));
        assert!(!make(planks, &mut s, &mut inv, false) && count(&inv, 17) == 1);
        assert_eq!(preview(planks, 2)[0], craft::code(17, 0), "a log in the first cell");
    }

    #[test]
    fn notes_remember_what_was_held_and_round_trip() {
        let mut n = Notes::default();
        assert!(!n.known(entry(5, 0, 2)), "nothing seen: a silhouette");
        let mut inv = Inventory::default();
        inv.slots[0] = Some(stack(17, 1));
        assert!(n.observe(&inv, &[None; 9]) && !n.observe(&inv, &[None; 9]));
        assert!(n.known(entry(5, 0, 2)), "a log is an ingredient of planks: known");
        assert!(!n.known(entry(61, 0, 3)), "a furnace wants cobblestone, not seen");
        n.pinned = Some(3);
        let b = n.encode();
        assert_eq!((b.len(), Notes::decode(&b)), (69, Some(n.clone())));
        assert_eq!(Notes::decode(&b[..68]), None);
        let mut far = n.clone();
        far.pinned = Some(entries().len() + 5);
        assert_eq!(Notes::decode(&far.encode()).map(|d| d.pinned), Some(None), "a pin past the list is dropped");
    }
}
