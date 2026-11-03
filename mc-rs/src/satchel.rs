//! The satchel: the immersive pack's flat replacement (`world::pack`'s cloth stays behind `packCloth:true`). The camera never moves; the
//! inventory button lifts one panel in front of the view, laid out like Legacy Minecraft's screens.
//!
//! - Two tabs. `Bag`: the armor row, the 9 x 3 bag and the quick belt, centred. `Craft`: a row of category tabs, a strip of recipes you
//!   slide sideways, and under it two sunk panels: the picked recipe (its ingredients as a ghost grid, red where the bag is short, the
//!   arrow and the result) and the bag (or, with `Info`, the ingredient list). There is no grid to fill by hand: the grid is machinery
//!   (`world::book::make`), a tap on a recipe picks it, a tap on the picked recipe (or on its result) crafts one into the bag, a hold on
//!   the result (or `Shift` on) crafts all. `Craftable` hides the recipes the bag cannot make now.
//! - Everything here is geometry (`Frame`) and drawing (`draw`, `draw_pin`); the screen's state is `Satchel`, the rules are `world::book`
//!   and `world::craft::Screen`, and `lib.rs` only routes the finger.
//!
//! - Look: Legacy Minecraft's: a light grey plate with a black outline, tabs on its edge that show an item each, sunk grey panels, slots
//!   that touch, a slot short of an ingredient red with a yellow "!", a row of hints under the panel (Exit, Info, Craftable) as buttons.
//!   Text is whole-pixel sized, dark on the grey, and fitted to its box (`fitted`, `text_in`, `paragraph`).
//! - Variants: the recipes that make the same item (the planks of each log, the wool of each dye) share one slot of the strip; the picked
//!   one's others show in a column left of the strip (up to three, arrows over and under it). An arrow, a drag on the column, or a pad's
//!   up / down on the slot steps through them (`variant`).
//!
//! UNVERIFIED: written without a compiler or a device. The proportions are in fractions of the surface height (`h`), the text unit is
//! `craft::panel(0, w, h).2`; both are guesses to tune on the phone.
//! ponytail: the world behind is dimmed, not blurred (a blur is a post pass); the strip scrolls a whole recipe at a time (no scissor, so
//! no half-visible cells); an unknown recipe is a `?` in a slot, not a true silhouette; the console's pixel font is not here (the HUD's is).

use crate::input::gamepad::Role;
use crate::render::hud::{HudPipeline, HudVertex};
use crate::world::book::{self, Src, CATS};
use crate::world::craft::{self, Screen, SlotId};
use crate::world::items::{self, Inventory, ItemStack, MAIN};
use crate::world::pack;

/// Seconds the panel takes to come up; a press on the result this long crafts all.
pub const RISE_SECS: f32 = 0.25;
pub const HOLD_SECS: f32 = 0.5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Btn {
    Close,
    /// The `Bag` and `Craft` tabs.
    Bag,
    Craft,
    Sort,
    Shift,
    One,
    /// The right panel shows the ingredients instead of the bag.
    Info,
    /// Only the recipes the bag can make now.
    Only,
}

/// What a finger is on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pick {
    Slot(SlotId),
    Btn(Btn),
    /// A category tab (index into `book::CATS`).
    Cat(usize),
    /// A recipe of the strip (index into `book::entries()`), the empty part of the strip (a drag there scrolls).
    Row(usize),
    List,
    /// The variants of the picked recipe's slot (the other recipes that make the same item): a cell of the column (index into
    /// `book::entries()`), the arrows over and under it, and the column between them (a drag there steps through them).
    Var(usize),
    VarUp,
    VarDown,
    VarList,
    /// The pin button of the recipe's header and the discovery toggle.
    Pin,
    Hide,
    /// The ingredient list of the right panel: a drag there scrolls it.
    Card,
    /// A panel with nothing on it: a tap does nothing, and does not throw the held stack.
    Panel,
    /// Past every panel: a held stack is thrown.
    Outside,
}

pub struct Satchel {
    pub age: f32,
    /// The Craft tab is up (else the Bag tab).
    pub craft: bool,
    /// The category tab (`book::CATS` index), the pixels the strip is scrolled (drawn snapped to a whole recipe), the picked recipe.
    pub cat: usize,
    pub scroll: f32,
    pub sel: Option<usize>,
    /// The pixels the ingredient rows are scrolled (drawn snapped to a whole row).
    pub card_scroll: f32,
    /// The right panel lists the ingredients; only the recipes that can be made now.
    pub info: bool,
    pub only: bool,
}

impl Satchel {
    pub fn new(craft: bool) -> Self {
        Self { age: 0.0, craft, cat: 0, scroll: 0.0, sel: None, card_scroll: 0.0, info: false, only: false }
    }

    pub fn tick(&mut self, dt: f32) {
        self.age += dt;
    }

    /// A category tab is picked: the Craft tab is up on it, its strip at the start.
    pub fn turn(&mut self, cat: usize) {
        (self.craft, self.cat, self.scroll) = (true, cat.min(CATS.len() - 1), 0.0);
    }
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn inside(r: [f32; 4], x: f32, y: f32) -> bool {
    x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]
}

/// The text size and the sprite pixel from the text unit `k`, in whole screen pixels: a glyph or a sprite pixel that is a fraction of a
/// screen pixel is smeared by the HUD's bilinear sampler, which is what made the old text look doubled. (title / button size, small size)
fn units(k: f32) -> (f32, f32) {
    ((0.75 * k).round().max(1.0), (0.6 * k).round().max(1.0))
}

/// `s` at size `k`, stepped down a whole pixel at a time (never under 1) until it is at most `maxw` wide, then cut with a dot if it still
/// is: (size, text).
fn fitted(s: &str, k: f32, maxw: f32) -> (f32, String) {
    let wide = |t: &str, k: f32| HudPipeline::text_width(t) as f32 * k;
    let mut k = k;
    while k > 1.0 && wide(s, k) > maxw {
        k = (k - 1.0).max(1.0);
    }
    let mut t = s.to_string();
    if wide(&t, k) > maxw {
        while !t.is_empty() && wide(&format!("{t}."), k) > maxw {
            t.pop();
        }
        t.push('.');
    }
    (k, t)
}

/// Flat text, or with a black outline `o` px wide when `o` > 0 (the recipe card's text: coloured ink alone is hard to read on parchment).
fn put(v: &mut Vec<HudVertex>, (x, y): (f32, f32), k: f32, s: &str, col: [f32; 4], o: f32) {
    if o > 0.0 {
        HudPipeline::push_text_outlined(v, x.round(), y.round(), k, o, s, col);
    } else {
        HudPipeline::push_text_flat(v, x, y, k, s, col);
    }
}

fn text_fit_o(v: &mut Vec<HudVertex>, (x, y): (f32, f32), k: f32, maxw: f32, s: &str, col: [f32; 4], o: f32) {
    let (kk, t) = fitted(s, k, maxw);
    put(v, (x, y + 4.0 * (k - kk)), kk, &t, col, o);
}

/// Flat text centred on `cx`, kept inside `[lo, hi]` (shrunk to fit it, pushed in when it would stick out).
fn text_in(v: &mut Vec<HudVertex>, cx: f32, y: f32, k: f32, range: (f32, f32), s: &str, col: [f32; 4]) {
    text_in_o(v, cx, y, k, range, s, col, 0.0);
}

fn text_in_o(v: &mut Vec<HudVertex>, cx: f32, y: f32, k: f32, (lo, hi): (f32, f32), s: &str, col: [f32; 4], o: f32) {
    let (kk, t) = fitted(s, k, hi - lo);
    let w = HudPipeline::text_width(&t) as f32 * kk;
    put(v, ((cx - w * 0.5).clamp(lo, (hi - w).max(lo)), y + 4.0 * (k - kk)), kk, &t, col, o);
}

/// The words of `s` in lines of at most `maxw` px at size `k` (a word wider than that stays alone on its line).
fn lines(s: &str, k: f32, maxw: f32) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in s.split_whitespace() {
        let joined = out.last().map(|l| format!("{l} {word}"));
        match joined {
            Some(j) if HudPipeline::text_width(&j) as f32 * k <= maxw => *out.last_mut().expect("a line") = j,
            _ => out.push(word.to_string()),
        }
    }
    out
}

/// A paragraph in the rect `r` (a 4-pixel margin of the sprite's), flat and wrapped, the last lines that do not fit left out.
fn paragraph(v: &mut Vec<HudVertex>, r: [f32; 4], (k, u): (f32, f32), s: &str, col: [f32; 4], o: f32) {
    let mut y = r[1] + 4.0 * u;
    for line in lines(s, k, r[2] - 8.0 * u) {
        if y + 8.0 * k > r[1] + r[3] - 2.0 * u {
            break;
        }
        put(v, (r[0] + 4.0 * u, y), k, &line, col, o);
        y += 10.0 * k;
    }
}


/// The Craft tab's rects, in px.
#[derive(Default)]
pub struct CraftFrame {
    /// The strip of recipes (exactly a whole number of cells wide), the cells in view with their entry, and one cell's width.
    pub strip: [f32; 4],
    /// The category's name, centred over the strip.
    pub title: [f32; 4],
    /// The variants' column, left of the strip, while the picked recipe shares its slot with others.
    pub var: Option<VarFrame>,
    pub rows: Vec<(usize, [f32; 4])>,
    pub row_w: f32,
    pub max_scroll: f32,
    /// The two sunk panels under the strip: the recipe (left) and the bag or the ingredients (right).
    pub left: [f32; 4],
    pub right: [f32; 4],
    /// The preview grid's cells (1 x 1 for a smelting, else 2 x 2 or 3 x 3), the arrow and the result.
    pub grid: Vec<[f32; 4]>,
    pub arrow: [f32; 4],
    pub result: [f32; 4],
    /// The ingredient list (`Info` on) and how far it can scroll (px).
    pub card: [f32; 4],
    pub card_max: f32,
    pub pin: [f32; 4],
    pub hide: [f32; 4],
    /// The clearance of a sunk panel's content from its rim.
    pub ip: f32,
}

/// The column of variants: up to three of them in view, the arrows over and under, and how far a finger drags for one step.
#[derive(Default)]
pub struct VarFrame {
    pub area: [f32; 4],
    pub up: [f32; 4],
    pub down: [f32; 4],
    pub cells: Vec<(usize, [f32; 4])>,
    /// The picked variant's place among `len`.
    pub pos: usize,
    pub len: usize,
    pub step: f32,
}

impl VarFrame {
    fn hit(&self, x: f32, y: f32) -> Option<Pick> {
        if inside(self.up, x, y) {
            Some(Pick::VarUp)
        } else if inside(self.down, x, y) {
            Some(Pick::VarDown)
        } else if let Some(&(i, _)) = self.cells.iter().find(|(_, r)| inside(*r, x, y)) {
            Some(Pick::Var(i))
        } else {
            inside(self.area, x, y).then_some(Pick::VarList)
        }
    }
}

pub struct Frame {
    /// The text unit (px per GUI unit).
    pub k: f32,
    /// The panel.
    pub bag: [f32; 4],
    pub cells: Vec<(SlotId, [f32; 4])>,
    /// The pocket band behind each row of cells (Bag tab: the armor row, three of the bag, the belt), and whether it is the belt's.
    pub rows: Vec<([f32; 4], bool)>,
    pub btns: Vec<(Btn, [f32; 4])>,
    /// The tabs on the panel's top edge: `Bag` and `Craft`, or `Bag` and the categories.
    pub tabs: Vec<(Pick, [f32; 4])>,
    pub craft: Option<CraftFrame>,
}

/// The side of the preview grid for `e` on a screen with grid `gw`: a smelting shows its one ingredient, a recipe the grid it needs (at
/// least the screen's).
fn preview_dim(e: &book::Entry, gw: usize) -> usize {
    if e.grid == 0 { 1 } else { gw.max(e.grid).clamp(2, 3) }
}

/// The recipes of the strip in order: the category's, what the bag can make first, and with `Craftable` only those.
fn strip_ids(sat: &Satchel, gw: usize, inv: &Inventory, grid: &[Option<ItemStack>; 9]) -> Vec<usize> {
    let mut ids = book::in_cat_ready(CATS[sat.cat.min(CATS.len() - 1)], gw, inv, grid);
    if sat.only {
        ids.retain(|&i| book::ready(&book::entries()[i], gw, inv, grid));
    }
    ids
}

/// The strip's slots: the recipes that make the same item (the planks of each log, the wool of each dye, the two torches) share one, in
/// the place of the first of them in `ids`; inside a slot they keep the book's order.
fn groups(ids: &[usize]) -> Vec<Vec<usize>> {
    let es = book::entries();
    let mut out: Vec<Vec<usize>> = Vec::new();
    for &i in ids {
        match out.iter_mut().find(|g| es[g[0]].out.id == es[i].out.id) {
            Some(g) => g.push(i),
            None => out.push(vec![i]),
        }
    }
    for g in &mut out {
        g.sort_unstable();
    }
    out
}

/// The recipe a slot shows: the picked one when it is in it, else the first the bag can make, else the first.
fn shown(g: &[usize], sel: Option<usize>, gw: usize, inv: &Inventory, grid: &[Option<ItemStack>; 9]) -> usize {
    sel.filter(|s| g.contains(s)).or_else(|| g.iter().copied().find(|&i| book::ready(&book::entries()[i], gw, inv, grid))).unwrap_or(g[0])
}

/// The recipe `d` places on in the picked one's slot (the other recipes that make the same item); `None` at either end, or when it
/// has no others. The arrows, a drag on the column and a pad's up / down on a slot step through it.
pub fn variant(sat: &Satchel, inv: &Inventory, grid: &[Option<ItemStack>; 9], gw: usize, d: i32) -> Option<usize> {
    let sel = sat.sel?;
    let g = groups(&strip_ids(sat, gw, inv, grid)).into_iter().find(|g| g.contains(&sel))?;
    let pos = i32::try_from(g.iter().position(|&i| i == sel)?).ok()?;
    g.get(usize::try_from(pos + d).ok()?).copied()
}

impl Frame {
    /// Every rect of the screen for `w` x `h` px, this state of the satchel and a crafting grid `gw` wide (the layout is fixed, every slot
    /// has its cell: the bag's contents only order the strip).
    pub fn new(sat: &Satchel, inv: &Inventory, gw: usize, w: f32, h: f32) -> Self {
        Self::with_grid(sat, inv, &[None; 9], gw, w, h)
    }

    /// `new` for a screen whose crafting grid holds `grid`: the strip puts the recipes the bag and the grid can make first, so the cells
    /// (and what a finger finds on them) depend on both.
    pub fn with_grid(sat: &Satchel, inv: &Inventory, grid: &[Option<ItemStack>; 9], gw: usize, w: f32, h: f32) -> Self {
        let k = craft::panel(0, w, h).2;
        let (tk, sk) = units(k);
        let u = tk; // one sprite pixel
        // The panel's border is 8 sprite pixels and its seam and corner marks lie inside 7 of them: the content starts clear of both.
        let pad = (0.03 * h).max(11.0 * u).round();
        let rise = (1.0 - ease(sat.age / RISE_SECS)) * 0.1 * h;
        let (top, bh) = ((0.17 * h + rise).round(), 0.74 * h);
        let mut f = Frame { k, bag: [0.0; 4], cells: Vec::new(), rows: Vec::new(), btns: Vec::new(), tabs: Vec::new(), craft: None };
        if sat.craft {
            let c = craft_layout(&mut f, sat, (gw, inv, grid), (w, h), (top, bh), ((0.02 * h).max(6.0 * u).round(), tk, sk));
            f.craft = Some(c);
        } else {
            bag_layout(&mut f, w, (top, bh), (pad, u));
        }

        // The tabs sit on the panel's top edge, one seventh of its width each.
        let (tw, tabh) = ((f.bag[2] / (1 + CATS.len()) as f32).floor(), (0.07 * h).floor());
        let picks: Vec<Pick> = if sat.craft {
            std::iter::once(Pick::Btn(Btn::Bag)).chain((0..CATS.len()).map(Pick::Cat)).collect()
        } else {
            vec![Pick::Btn(Btn::Bag), Pick::Btn(Btn::Craft)]
        };
        for (i, p) in picks.into_iter().enumerate() {
            f.tabs.push((p, [f.bag[0] + i as f32 * tw, top - tabh, tw, tabh]));
        }

        // The plates above the tabs: Sort, Shift, One.
        let (by, bh2) = (0.015 * h + rise, 0.075 * h);
        let mut x = f.bag[0];
        for (b, label) in [(Btn::Sort, "Sort"), (Btn::Shift, "Shift"), (Btn::One, "One")] {
            let bw = HudPipeline::text_width(label) as f32 * tk + 6.0 * tk;
            f.btns.push((b, [x, by, bw, bh2]));
            x += bw + 0.012 * h;
        }
        // Legacy's row of hints under the panel, as buttons a finger can press: Exit, and on the Craft tab Info and Craftable.
        let (cy, ch) = (f.bag[1] + f.bag[3] + 0.015 * h, 0.06 * h);
        let mut x = f.bag[0];
        let mut chips = vec![(Btn::Close, "Exit")];
        if sat.craft {
            chips.extend([(Btn::Info, "Info"), (Btn::Only, "Craftable")]);
        }
        for (b, label) in chips {
            let bw = HudPipeline::text_width(label) as f32 * tk + 10.0 * tk;
            f.btns.push((b, [x, cy, bw, ch]));
            x += bw + 0.012 * h;
        }
        f
    }

    /// What is under `(x, y)`.
    pub fn hit(&self, sat: &Satchel, x: f32, y: f32) -> Pick {
        if let Some((b, _)) = self.btns.iter().find(|(_, r)| inside(*r, x, y)) {
            return Pick::Btn(*b);
        }
        if let Some((p, _)) = self.tabs.iter().find(|(_, r)| inside(*r, x, y)) {
            return *p;
        }
        if let Some(b) = &self.craft {
            let hit = b
                .rows
                .iter()
                .find(|(_, r)| inside(*r, x, y))
                .map(|&(i, _)| Pick::Row(i))
                .or_else(|| b.var.as_ref().and_then(|v| v.hit(x, y)))
                .or_else(|| (sat.sel.is_some() && inside(b.pin, x, y)).then_some(Pick::Pin))
                .or_else(|| inside(b.hide, x, y).then_some(Pick::Hide))
                .or_else(|| inside(b.result, x, y).then_some(Pick::Slot(SlotId::Result)))
                .or_else(|| inside(b.strip, x, y).then_some(Pick::List))
                .or_else(|| (sat.info && inside(b.card, x, y)).then_some(Pick::Card));
            if let Some(h) = hit {
                return h;
            }
        }
        if let Some((id, _)) = self.cells.iter().find(|(_, r)| inside(*r, x, y)) {
            return Pick::Slot(*id);
        }
        if inside(self.bag, x, y) {
            return Pick::Panel;
        }
        Pick::Outside
    }

    /// Every rect a pad can rest on, the ones `hit` answers with something other than `Panel` / `Outside` / `List` / `Card`: the buttons,
    /// the tabs, the recipes in view, the pin, the toggle and the result, and every slot in view.
    pub fn targets(&self, sat: &Satchel) -> Vec<[f32; 4]> {
        let mut t: Vec<[f32; 4]> = self.btns.iter().map(|&(_, r)| r).collect();
        t.extend(self.tabs.iter().map(|&(_, r)| r));
        if let Some(b) = &self.craft {
            t.extend(b.rows.iter().map(|&(_, r)| r));
            if let Some(v) = &b.var {
                t.push(v.up);
                t.extend(v.cells.iter().map(|&(_, r)| r));
                t.push(v.down);
            }
            if sat.sel.is_some() {
                t.push(b.pin);
            }
            t.push(b.hide);
            t.push(b.result);
        }
        t.extend(self.cells.iter().map(|&(_, r)| r));
        t
    }

    /// The rects a pad's button hints keep off: every button (Close among them), the tabs and the panel.
    pub fn avoid(&self, _sat: &Satchel) -> Vec<[f32; 4]> {
        let mut a: Vec<[f32; 4]> = self.btns.iter().map(|&(_, r)| r).collect();
        a.extend(self.tabs.iter().map(|&(_, r)| r));
        a.push(self.bag);
        a
    }
}

/// What the pad's buttons do over `pick` (the hints' rows: A, X, B), `holding` a stack on the cursor. Only what the press really does:
/// `on_sat_tap` for A, `pad_alt` for X (a slot only), `sat_pad_keys` for B (it closes the screen).
pub fn pad_hints(sat: &Satchel, pick: Pick, holding: bool) -> Vec<(Role, &'static str)> {
    let a = match pick {
        Pick::Slot(SlotId::Result) => Some("Craft"),
        Pick::Slot(_) => Some(if holding { "Place" } else { "Take" }),
        Pick::Btn(Btn::Close) => Some("Close"),
        Pick::Btn(Btn::Bag) => Some("Bag"),
        Pick::Btn(Btn::Craft) => Some("Crafting"),
        Pick::Btn(Btn::Sort) => Some("Sort"),
        Pick::Btn(Btn::Shift | Btn::One | Btn::Info | Btn::Only) | Pick::Hide => Some("Toggle"),
        Pick::Cat(_) => Some("Open"),
        Pick::Row(i) => Some(if sat.sel == Some(i) { "Craft" } else { "Pick" }), // a second tap on the picked recipe crafts it
        Pick::Pin => Some("Pin"),
        Pick::Var(_) => Some("Pick"),
        Pick::VarUp | Pick::VarDown => Some("Variant"),
        Pick::Outside => holding.then_some("Throw"),
        Pick::List | Pick::VarList | Pick::Panel | Pick::Card => None,
    };
    let x = match pick {
        Pick::Slot(SlotId::Result) => None,
        Pick::Slot(_) => Some(if holding { "Place one" } else { "Take half" }),
        _ => None,
    };
    let mut rows: Vec<(Role, &'static str)> = a.map(|t| (Role::Select, t)).into_iter().collect();
    rows.extend(x.map(|t| (Role::Alt, t)));
    rows.push((Role::Back, "Close"));
    rows
}

/// The target the cursor `at` rests on.
pub fn under(targets: &[[f32; 4]], at: (f32, f32)) -> Option<[f32; 4]> {
    targets.iter().copied().find(|r| inside(*r, at.0, at.1))
}

/// The pad's step: the rect to jump to from the one under `at` when the d-pad says `d` (x, y; y down). Off every rect (the cursor
/// was left where a tab changed under it), the nearest one. Among the rects that lie that way, within 63 degrees of it, the
/// one with the least `along + 2 * across` (centre to centre) wins: touching slots step one by one, a button row above the bag is a
/// step up from its columns, a recipe is a step right of the one before it. `None` when nothing lies that way.
pub fn step(targets: &[[f32; 4]], at: (f32, f32), d: (i32, i32)) -> Option<[f32; 4]> {
    let mid = |r: &[f32; 4]| (r[0] + r[2] * 0.5, r[1] + r[3] * 0.5);
    let Some(cur) = under(targets, at) else {
        let far = |r: &[f32; 4]| (mid(r).0 - at.0).hypot(mid(r).1 - at.1);
        return targets.iter().copied().min_by(|a, b| far(a).total_cmp(&far(b)));
    };
    let from = mid(&cur);
    let score = |r: &[f32; 4]| {
        let (dx, dy) = (mid(r).0 - from.0, mid(r).1 - from.1);
        let (along, across) = if d.0 != 0 { (dx * d.0 as f32, dy.abs()) } else { (dy * d.1 as f32, dx.abs()) };
        (along > 1.0 && across <= 2.0 * along).then_some(along + 2.0 * across)
    };
    targets.iter().copied().filter(|r| *r != cur).filter_map(|r| score(&r).map(|sc| (sc, r))).min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, r)| r)
}


/// The Bag tab: the armor row, three rows of the bag and the quick belt, centred on the screen. Slots touch, like Minecraft's: a cell is
/// one slot sprite and the next starts where its border ends. A pocket band lies behind each block of them, two sprite pixels wider than
/// it, clear of the seam.
fn bag_layout(f: &mut Frame, w: f32, (top, bh): (f32, f32), (pad, u): (f32, f32)) {
    let gv = 12.0 * u; // the gap between the blocks: the armor, the bag and the belt read as three separate things
    let cell = ((0.9 * w - 2.0 * pad) / 9.0).min((bh - 2.0 * pad - 2.0 * gv) / 5.0).floor().max(1.0);
    let (pw, ph) = (9.0 * cell + 2.0 * pad, 5.0 * cell + 2.0 * gv + 2.0 * pad);
    let x = ((w - pw) * 0.5).round();
    f.bag = [x, top, pw, ph];
    let (bx, by) = (x + pad, top + pad);
    let band = |y: f32, n: f32, rows: f32| [bx - 2.0 * u, y - 2.0 * u, n * cell + 4.0 * u, rows * cell + 4.0 * u];
    f.rows.push((band(by, 4.0, 1.0), false));
    for i in 0..4 {
        f.cells.push((SlotId::Inv(MAIN + 3 - i), [bx + i as f32 * cell, by, cell, cell])); // helmet first
    }
    let my = by + cell + gv;
    f.rows.push((band(my, 9.0, 3.0), false));
    for r in 0..3 {
        for c in 0..9 {
            f.cells.push((SlotId::Inv(9 + r * 9 + c), [bx + c as f32 * cell, my + r as f32 * cell, cell, cell]));
        }
    }
    let belt_y = my + 3.0 * cell + gv;
    f.rows.push((band(belt_y, 9.0, 1.0), true));
    for c in 0..9 {
        f.cells.push((SlotId::Inv(c), [bx + c as f32 * cell, belt_y, cell, cell]));
    }
}

/// The Craft tab: the panel across the screen; on top the variants' column (while there are some), the category's title and the strip of
/// recipes; under it the recipe (left) and the bag or its ingredients (right).
fn craft_layout(f: &mut Frame, sat: &Satchel, (gw, inv, grid): (usize, &Inventory, &[Option<ItemStack>; 9]), (w, h): (f32, f32), (top, bh): (f32, f32), (pad, tk, sk): (f32, f32, f32)) -> CraftFrame {
    let u = tk;
    let px = (0.02 * w).round();
    f.bag = [px, top, w - 2.0 * px, bh];
    let p = f.bag;
    let mut c = CraftFrame::default();

    // The top band. The column is as tall as three variants and two arrows (it is always reserved: the strip does not jump when it shows).
    let (cu, vc, ah) = (2.0 * u, (0.055 * h).floor().max(1.0), (0.03 * h).floor().max(1.0));
    let (col_w, col_h) = (vc + 2.0 * cu, 2.0 * cu + 2.0 * ah + 3.0 * vc);
    let (ty0, tt, rc) = (p[1] + pad, 8.0 * tk + 4.0 * u, (0.11 * h).floor().max(1.0));
    let sx = p[0] + pad + col_w + (0.012 * w).round();
    let inner = p[0] + p[2] - pad - sx;
    let vis = (inner / rc).floor().max(1.0);
    c.strip = [sx + ((inner - vis * rc) * 0.5).floor(), ty0 + tt, vis * rc, rc];
    c.title = [c.strip[0], ty0, c.strip[2], tt];
    c.row_w = rc;

    // The strip: one slot per group of recipes, whole slots only, centred; it scrolls a whole slot at a time.
    let gs = groups(&strip_ids(sat, gw, inv, grid));
    c.max_scroll = (gs.len() as f32 - vis).max(0.0) * rc;
    let off = (sat.scroll.clamp(0.0, c.max_scroll) / rc).round() * rc;
    for (n, g) in gs.iter().enumerate() {
        let x = c.strip[0] + n as f32 * rc - off;
        if x >= c.strip[0] - 0.5 && x + rc <= c.strip[0] + c.strip[2] + 0.5 {
            c.rows.push((shown(g, sat.sel, gw, inv, grid), [x, c.strip[1], rc, rc]));
        }
    }
    // The column: the picked recipe's variants, three in view with the picked one in the middle where it can be.
    if let Some(g) = sat.sel.and_then(|s| gs.iter().find(|g| g.contains(&s))).filter(|g| g.len() > 1) {
        let pos = g.iter().position(|&i| Some(i) == sat.sel).unwrap_or(0);
        let first = pos.saturating_sub(1).min(g.len().saturating_sub(3));
        let area = [p[0] + pad, ty0, col_w, col_h];
        let cells: Vec<(usize, [f32; 4])> = g.iter().skip(first).take(3).enumerate().map(|(n, &i)| (i, [area[0] + cu, ty0 + cu + ah + n as f32 * vc, vc, vc])).collect();
        let (up, down) = ([area[0] + cu, ty0 + cu, vc, ah], [area[0] + cu, ty0 + cu + ah + 3.0 * vc, vc, ah]);
        c.var = Some(VarFrame { area, up, down, cells, pos, len: g.len(), step: vc });
    }

    // The two sunk panels.
    let (ip, gap) = (5.0 * u, (0.012 * w).round());
    let y1 = (ty0 + col_h + 0.008 * h).round();
    let bz = p[1] + p[3] - pad - y1;
    let hw = ((p[2] - 2.0 * pad - gap) * 0.5).floor();
    c.left = [p[0] + pad, y1, hw, bz];
    c.right = [p[0] + pad + hw + gap, y1, hw, bz];
    c.ip = ip;

    // Left: the name centred with a line under it, the pin in the corner, the toggle at the foot, the preview between them.
    let l = c.left;
    let th = ip + 8.0 * tk + 2.0 * u + 8.0 * sk + 3.0 * u;
    let (pin, hide_h) = (8.0 * tk + 4.0 * u, (0.055 * h).floor());
    c.pin = [l[0] + l[2] - ip - pin, l[1] + 0.6 * ip, pin, pin];
    c.hide = [l[0] + ip, l[1] + l[3] - ip - hide_h, l[2] - 2.0 * ip, hide_h];
    let pg = sat.sel.and_then(|i| book::entries().get(i)).map_or(gw.clamp(2, 3), |e| preview_dim(e, gw));
    let (ga, aw) = (0.004 * w, 0.03 * w);
    let avail = (c.hide[1] - ip - (l[1] + th)).max(1.0);
    // The cells are the smaller of the panel's width share (the grid, the arrow and the result, 1.2 cells) and its height share.
    let cg = ((l[2] - 2.0 * ip - 2.0 * ga - aw) / (pg as f32 + 1.2)).min(avail / pg as f32).min(0.13 * h).floor().max(1.0);
    let (gh, rcell) = (pg as f32 * cg, (cg * 1.2).floor());
    let gx = (l[0] + (l[2] - (gh + 2.0 * ga + aw + rcell)) * 0.5).floor();
    let gy = (l[1] + th + ((avail - gh) * 0.5).max(0.0)).floor();
    for i in 0..pg * pg {
        c.grid.push([gx + (i % pg) as f32 * cg, gy + (i / pg) as f32 * cg, cg, cg]);
    }
    let ax = gx + gh + ga;
    c.arrow = [ax, gy + gh * 0.5 - 0.02 * h, aw, 0.04 * h];
    c.result = [(ax + aw + ga).floor(), (gy + (gh - rcell) * 0.5).round(), rcell, rcell];

    // Right: a title, then the bag (the three rows and the belt, a small gap before it) or the ingredient list.
    let (r, th2) = (c.right, 8.0 * tk + 2.0 * u + ip);
    if sat.info {
        c.card = [r[0] + ip, r[1] + th2, r[2] - 2.0 * ip, r[3] - th2 - ip];
        c.card_max = sat.sel.and_then(|i| book::entries().get(i)).map_or(0.0, |e| card_m(c.card, (sk, u), e.need.len()).max);
    } else {
        let gv = 4.0 * u;
        let ci = ((r[2] - 2.0 * ip) / 9.0).min((r[3] - th2 - ip - gv) / 4.0).floor().max(1.0);
        let x0 = (r[0] + (r[2] - 9.0 * ci) * 0.5).floor();
        for row in 0..3 {
            for col in 0..9 {
                f.cells.push((SlotId::Inv(9 + row * 9 + col), [x0 + col as f32 * ci, r[1] + th2 + row as f32 * ci, ci, ci]));
            }
        }
        for col in 0..9 {
            f.cells.push((SlotId::Inv(col), [x0 + col as f32 * ci, r[1] + th2 + 3.0 * ci + gv, ci, ci]));
        }
    }
    c
}

// ---- Drawing ----
//
// The panels, tabs, slots and buttons are flat quads in Legacy Minecraft's greys (a black outline, a bright edge top-left, a shadow
// bottom-right), at one pixel size: `u` = the text's own pixel (`tk`). Icons are the game's items; from `assets/satchel.rgba`
// (`hud::sprites::sat`) only the slot rim, the pin and the armor shapes remain.

use crate::render::hud::sprites::sat as art;

const WHITE: [f32; 4] = [1.0; 4];
const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
const SELECTED: [f32; 4] = [0.95, 0.85, 0.1, 1.0];

// Legacy Minecraft's greys: the light plate, the sunk panels, the slots, the tabs; dark text on them, and the red and yellow of a missing
// ingredient (read off the console's screenshot).
const PLATE: [f32; 4] = [0.776, 0.776, 0.776, 1.0];
const PLATE_HI: [f32; 4] = [0.93, 0.93, 0.93, 1.0];
const PLATE_LO: [f32; 4] = [0.56, 0.56, 0.56, 1.0];
const WELL: [f32; 4] = [0.70, 0.70, 0.70, 1.0];
const WELL_LO: [f32; 4] = [0.36, 0.36, 0.36, 1.0];
const WELL_HI: [f32; 4] = [0.95, 0.95, 0.95, 1.0];
const TAB_OFF: [f32; 4] = [0.63, 0.63, 0.63, 1.0];
const CELL: [f32; 4] = [0.545, 0.545, 0.545, 1.0];
const CELL_LO: [f32; 4] = [0.22, 0.22, 0.22, 1.0];
const RED: [f32; 4] = [0.82, 0.25, 0.25, 1.0];
const YELLOW: [f32; 4] = [0.98, 0.82, 0.10, 1.0];
const PICKED: [f32; 4] = [0.40, 0.50, 0.40, 1.0];
const TEXT: [f32; 4] = [0.25, 0.25, 0.25, 1.0];
/// Notes on the grey panels: it can be made, it cannot, it is made elsewhere.
const OK_T: [f32; 4] = [0.08, 0.42, 0.08, 1.0];
const NO_T: [f32; 4] = [0.70, 0.12, 0.12, 1.0];
const WARN_I: [f32; 4] = [0.58, 0.36, 0.0, 1.0];
/// The item on each tab, in the order of `Pick::Cat`: a pickaxe, bread, planks, a lever, a painting, a furnace.
const CAT_ICONS: [u16; 6] = [257, 297, 5, 69, 321, 61];

/// The rim of a filled slot: its item's category, tinted lightly (tools blue, food green, materials brown, the rest grey).
fn tint(id: u16) -> [f32; 4] {
    match pack::category(id) {
        pack::Cat::Tools => [0.38, 0.52, 0.78, 1.0],
        pack::Cat::Food => [0.42, 0.68, 0.36, 1.0],
        pack::Cat::Materials => [0.66, 0.5, 0.32, 1.0],
        pack::Cat::Misc => [0.62, 0.62, 0.68, 1.0],
    }
}

/// What `draw` needs besides the satchel and its frame.
pub struct Ctx<'a> {
    pub inv: &'a Inventory,
    pub screen: &'a Screen,
    pub notes: &'a book::Notes,
    pub one: bool,
    pub shift: bool,
    /// The book hides what the player has not met (`Settings::book_discovery`).
    pub discovery: bool,
    pub hotbar: usize,
    /// The finger: where it is, whether it is down, and whether a press on the result has lasted `HOLD_SECS`.
    pub finger: (f32, f32),
    pub down: bool,
    pub hold: bool,
    /// A pad drives the cursor: the target under it gets a frame and its name shows without a press.
    pub pad: bool,
}

fn item(c: u16) -> ItemStack {
    let (id, damage) = craft::unpack(c);
    ItemStack { id, count: 1, damage }
}

fn quad(v: &mut Vec<HudVertex>, r: [f32; 4], col: [f32; 4]) {
    HudPipeline::push_quad(v, r[0], r[1], r[2], r[3], col);
}

/// Legacy's light plate: a black outline, a bright edge top-left and a shadow bottom-right.
fn plate(v: &mut Vec<HudVertex>, r: [f32; 4], u: f32) {
    let (b, e) = ((1.5 * u).round().max(1.0), u.max(1.0));
    quad(v, r, BLACK);
    let i = [r[0] + b, r[1] + b, r[2] - 2.0 * b, r[3] - 2.0 * b];
    quad(v, i, PLATE);
    quad(v, [i[0], i[1], i[2], e], PLATE_HI);
    quad(v, [i[0], i[1], e, i[3]], PLATE_HI);
    quad(v, [i[0], i[1] + i[3] - e, i[2], e], PLATE_LO);
    quad(v, [i[0] + i[2] - e, i[1], e, i[3]], PLATE_LO);
}

/// A sunk panel: a dark edge top-left, a light one bottom-right.
fn well(v: &mut Vec<HudVertex>, r: [f32; 4], u: f32) {
    let e = (1.5 * u).round().max(1.0);
    quad(v, r, WELL_HI);
    quad(v, [r[0], r[1], r[2] - e, r[3] - e], WELL_LO);
    quad(v, [r[0] + e, r[1] + e, r[2] - 2.0 * e, r[3] - 2.0 * e], WELL);
}

/// A slot's well: grey (or the red of a missing ingredient `fill`), dark top-left, white bottom-right. Neighbours touch: a dark line and
/// a white one lie between them, as in the console's slots.
fn cell(v: &mut Vec<HudVertex>, r: [f32; 4], u: f32, fill: [f32; 4]) {
    let e = (0.67 * u).round().max(1.0);
    quad(v, r, WHITE);
    quad(v, [r[0], r[1], r[2] - e, r[3] - e], CELL_LO);
    quad(v, [r[0] + e, r[1] + e, r[2] - 2.0 * e, r[3] - 2.0 * e], fill);
}

/// A slot cell: the well, a rim in `rim` when it has one, the stack in it.
fn slot(v: &mut Vec<HudVertex>, r: [f32; 4], st: Option<ItemStack>, rim: Option<[f32; 4]>, u: f32) {
    slot_in(v, r, st, rim, u, CELL);
}

fn slot_in(v: &mut Vec<HudVertex>, r: [f32; 4], st: Option<ItemStack>, rim: Option<[f32; 4]>, u: f32, fill: [f32; 4]) {
    cell(v, r, u, fill);
    if let Some(c) = rim {
        HudPipeline::push_nine(v, (r[0], r[1], r[2], r[3]), art::RIM, 2.0, u, c, false);
    }
    if let Some(st) = st {
        // The count is drawn here, not by `push_stack`: that one keeps the count in a 16 unit box the size of the whole cell, which
        // ends on the cell's border (the cell is 18 units, the item 16) and is as big as the vanilla one.
        crate::push_stack(v, (r[0], r[1], r[2], r[3]), 0.875, ItemStack { count: 1, ..st });
        if st.count > 1 {
            count(v, r, u, st.count as u32);
        }
    }
}

/// A stack's count in the lower right corner of the cell `r`, a whole-pixel size (about a third of the item's) and clear of the border.
fn count(v: &mut Vec<HudVertex>, r: [f32; 4], u: f32, n: u32) {
    let k = (r[2] / 32.0).round().max(1.0);
    let m = 2.0 * u + 2.0; // the border and a little air
    let s = n.to_string();
    let w = HudPipeline::text_width(&s) as f32 * k;
    HudPipeline::push_text(v, (r[0] + r[2] - m - k - w).round(), (r[1] + r[3] - m - 9.0 * k).round(), k, &s, WHITE); // 9 k: the glyph and its shadow
}

/// The border of `r`, `t` px thick, inside it.
fn frame_rect(v: &mut Vec<HudVertex>, r: [f32; 4], t: f32, col: [f32; 4]) {
    HudPipeline::push_quad(v, r[0], r[1], r[2], t, col);
    HudPipeline::push_quad(v, r[0], r[1] + r[3] - t, r[2], t, col);
    HudPipeline::push_quad(v, r[0], r[1] + t, t, r[3] - 2.0 * t, col);
    HudPipeline::push_quad(v, r[0] + r[2] - t, r[1] + t, t, r[3] - 2.0 * t, col);
}

/// The yellow "!" of a missing ingredient, in the corner of its red slot `r`.
fn badge(v: &mut Vec<HudVertex>, r: [f32; 4], u: f32) {
    let s = (r[2] * 0.34).floor().max(8.0);
    let (x, y) = (r[0] + u, r[1] + u);
    quad(v, [x, y, s, s], BLACK);
    quad(v, [x + u, y + u, s - 2.0 * u, s - 2.0 * u], YELLOW);
    let k = (s / 10.0).floor().max(1.0);
    text_in(v, x + s * 0.5, y + (s - 8.0 * k) * 0.5, k, (x, x + s), "!", BLACK);
}

/// A triangle in the box `r`, pointing up or down, drawn in rows.
fn tri(v: &mut Vec<HudVertex>, r: [f32; 4], up: bool, col: [f32; 4]) {
    let (w, h) = (r[2] * 0.5, r[3] * 0.5);
    let n = (h / 2.0).floor().max(2.0);
    let (x, y, rh) = (r[0] + r[2] * 0.5, r[1] + r[3] * 0.25, h / n);
    for i in 0..n as usize {
        let rw = w * (i as f32 + 1.0) / n;
        let k = if up { i as f32 } else { n - 1.0 - i as f32 };
        quad(v, [x - rw * 0.5, y + k * rh, rw, rh + 0.5], col);
    }
}

/// The arrow between the preview and the result: a shaft and a head pointing right.
fn arrow_right(v: &mut Vec<HudVertex>, r: [f32; 4], col: [f32; 4]) {
    quad(v, [r[0], r[1] + r[3] * 0.35, r[2] * 0.6, r[3] * 0.3], col);
    let n = 6usize;
    let cw = r[2] * 0.4 / n as f32;
    for i in 0..n {
        let hh = r[3] * (1.0 - i as f32 / n as f32);
        quad(v, [r[0] + r[2] * 0.6 + i as f32 * cw, r[1] + (r[3] - hh) * 0.5, cw + 0.5, hh], col);
    }
}

/// A tab on the panel's top edge: the picked one stands taller in the plate's colour and joins it, the others are darker. Its icon is an item.
fn tab(v: &mut Vec<HudVertex>, r: [f32; 4], u: f32, on: bool, icon: u16) {
    let b = (1.5 * u).round().max(1.0);
    let r = if on { [r[0], r[1] - 2.0 * u, r[2], r[3] + 2.0 * u] } else { r };
    quad(v, r, BLACK);
    let i = [r[0] + b, r[1] + b, r[2] - 2.0 * b, r[3] - b];
    quad(v, i, if on { PLATE } else { TAB_OFF });
    quad(v, [i[0], i[1], i[2], u.max(1.0)], PLATE_HI);
    if on {
        quad(v, [r[0] + b, r[1] + r[3] - b, r[2] - 2.0 * b, 2.0 * b], PLATE); // over the panel's outline: the tab and the panel are one
    }
    let s = (r[3] * 0.7).min(r[2] * 0.7).floor();
    crate::push_stack(v, (r[0] + (r[2] - s) * 0.5, r[1] + (r[3] - s) * 0.5, s, s), 0.875, ItemStack { id: icon, count: 1, damage: 0 });
}

/// A grey button with a black outline: `state` 0 disabled, 1 normal, 2 on; the label is white with a black edge.
fn btn(v: &mut Vec<HudVertex>, r: [f32; 4], (tk, u): (f32, f32), text: &str, state: u8) {
    let b = (1.5 * u).round().max(1.0);
    quad(v, r, BLACK);
    let i = [r[0] + b, r[1] + b, r[2] - 2.0 * b, r[3] - 2.0 * b];
    quad(v, i, match state { 2 => [0.38, 0.55, 0.38, 1.0], 0 => [0.45, 0.45, 0.45, 1.0], _ => [0.52, 0.52, 0.52, 1.0] });
    quad(v, [i[0], i[1], i[2], u.max(1.0)], PLATE_HI);
    text_in_o(v, r[0] + r[2] * 0.5, r[1] + (r[3] - 8.0 * tk) * 0.5, tk, (r[0] + 2.0 * u, r[0] + r[2] - 2.0 * u), text, WHITE, (u * 0.34).round().max(1.0));
}

/// An entry of the row of hints under the panel (Legacy's Exit / Info): white text on a dark veil, lit green while on.
fn chip(v: &mut Vec<HudVertex>, r: [f32; 4], (tk, u): (f32, f32), text: &str, on: bool) {
    quad(v, r, if on { [0.20, 0.42, 0.20, 0.75] } else { [0.0, 0.0, 0.0, 0.45] });
    text_in_o(v, r[0] + r[2] * 0.5, r[1] + (r[3] - 8.0 * tk) * 0.5, tk, (r[0] + 2.0 * u, r[0] + r[2] - 2.0 * u), text, WHITE, (u * 0.34).round().max(1.0));
}

/// The whole screen: the dimmed world, the panel with its tabs, the stack on the finger and the name of what it touches.
pub fn draw(v: &mut Vec<HudVertex>, (w, h): (f32, f32), sat: &Satchel, c: &Ctx) {
    let f = Frame::with_grid(sat, c.inv, &c.screen.grid, c.screen.gw, w, h);
    let k = f.k;
    let (tk, sk) = units(k);
    let u = tk;
    quad(v, [0.0, 0.0, w, h], [0.0, 0.0, 0.0, 0.5]);

    // The plate, the tabs on its edge, a sunk band behind each block of the bag, and on the Craft tab its strip and panels.
    plate(v, f.bag, u);
    for &(p, r) in &f.tabs {
        let (icon, on) = match p {
            Pick::Btn(Btn::Bag) => (54, !sat.craft),
            Pick::Btn(Btn::Craft) => (58, false),
            Pick::Cat(i) => (CAT_ICONS[i.min(CAT_ICONS.len() - 1)], sat.craft && sat.cat == i),
            _ => (0, false),
        };
        tab(v, r, u, on, icon);
    }
    for &(r, _) in &f.rows {
        well(v, r, u);
    }
    if let Some(b) = &f.craft {
        draw_craft(v, sat, b, c, (tk, sk, u));
    }
    for &(id, r) in &f.cells {
        let st = c.screen.get(c.inv, None, id);
        // The selected belt slot keeps its yellow rim; an item's category rim shows only under a finger.
        let touched = (c.down || c.pad) && inside(r, c.finger.0, c.finger.1);
        let rim = if id == SlotId::Inv(c.hotbar) { Some(SELECTED) } else if touched { st.map(|s| tint(s.id)) } else { None };
        slot(v, r, st, rim, u);
        // An empty armor slot shows the shape of what goes in it (no label needed), dark and faint like a hole in the stone.
        if let (None, SlotId::Inv(i)) = (st, id) {
            if i >= MAIN {
                // Fitted into 80% of the cell, proportions kept (the shapes differ in size: the head is small, the legs are tall).
                let spr = art::ARMOR[MAIN + 3 - i];
                let sc = (r[2] * 0.8 / spr.2).min(r[3] * 0.8 / spr.3);
                let (dw, dh) = (spr.2 * sc, spr.3 * sc);
                HudPipeline::push_gui_tinted(v, r[0] + (r[2] - dw) * 0.5, r[1] + (r[3] - dh) * 0.5, dw, dh, spr, [0.0, 0.0, 0.0, 0.45]);
            }
        }
    }
    // Buttons: the plates above the tabs (the toggles show their number while on), the hints under the panel.
    for &(b, r) in &f.btns {
        match b {
            Btn::Sort => btn(v, r, (tk, u), "Sort", 1),
            Btn::Shift => btn(v, r, (tk, u), if c.shift { "2" } else { "Shift" }, if c.shift { 2 } else { 1 }),
            Btn::One => btn(v, r, (tk, u), if c.one { "1" } else { "One" }, if c.one { 2 } else { 1 }),
            Btn::Close => chip(v, r, (tk, u), "Exit", false),
            Btn::Info => chip(v, r, (tk, u), "Info", sat.info),
            Btn::Only => chip(v, r, (tk, u), "Craftable", sat.only),
            Btn::Bag | Btn::Craft => {}
        }
    }

    // A pad's cursor rests on a target (it jumps from one to the next): a white frame round it, edged in black.
    let (fx, fy) = c.finger;
    if c.pad {
        if let Some(r) = under(&f.targets(sat), (fx, fy)) {
            let t = (1.5 * u).round().max(2.0);
            frame_rect(v, [r[0] - t, r[1] - t, r[2] + 2.0 * t, r[3] + 2.0 * t], t, [0.0, 0.0, 0.0, 0.9]);
            frame_rect(v, r, t, WHITE);
        }
    }
    // The stack on the finger floats above it; a finger down (or a pad's cursor) names what it holds or rests on.
    if let Some(st) = c.screen.cursor {
        let sz = 16.0 * k;
        crate::push_stack(v, (fx - sz * 0.5, fy - sz * 1.4, sz, sz), 0.875, st);
    }
    if c.down || c.pad {
        let known = |e: &&book::Entry| !c.discovery || c.notes.known(e);
        let under = || match f.hit(sat, fx, fy) {
            Pick::Slot(SlotId::Result) => sat.sel.and_then(|i| book::entries().get(i)).filter(known).map(|e| e.out),
            Pick::Slot(id) => c.screen.get(c.inv, None, id),
            Pick::Row(i) | Pick::Var(i) => book::entries().get(i).filter(known).map(|e| e.out),
            _ => None,
        };
        if let Some((st, name)) = c.screen.cursor.or_else(under).and_then(|st| items::name(st).map(|n| (st, n))) {
            let lines = crate::world::combat::tooltip::lines(st); // C6
            let (bw, bh) = HudPipeline::tooltip_size(name, &lines);
            let (bw, extra) = (bw * tk, (bh - 14.0) * tk);
            let ty = if fy > 36.0 * tk + extra { fy - 34.0 * tk - extra } else { fy + 14.0 * tk };
            HudPipeline::push_tooltip_lines(v, (fx - bw * 0.5).clamp(0.0, (w - bw).max(0.0)), ty, tk, name, &lines);
        }
    }
}

/// The Craft tab's contents: the column of variants, the category's title and the strip, the recipe's panel and the right panel (the bag's
/// slots are drawn by `draw`).
fn draw_craft(v: &mut Vec<HudVertex>, sat: &Satchel, b: &CraftFrame, c: &Ctx, (tk, sk, u): (f32, f32, f32)) {
    let entries = book::entries();
    let gw = c.screen.gw;
    let ip = b.ip;

    // The title, and the strip: the whole row of slots (the empty ones too), a recipe's item in each, dimmed when it cannot be made now,
    // the picked one in a green-grey frame (an unknown one is a `?`).
    let t = b.title;
    text_in(v, t[0] + t[2] * 0.5, t[1] + (t[3] - 8.0 * tk) * 0.5, tk, (t[0], t[0] + t[2]), CATS[sat.cat.min(CATS.len() - 1)].label(), TEXT);
    for n in 0..(b.strip[2] / b.row_w).round() as usize {
        cell(v, [b.strip[0] + n as f32 * b.row_w, b.strip[1], b.row_w, b.row_w], u, CELL);
    }
    for &(ei, r) in &b.rows {
        let e = &entries[ei];
        let known = !c.discovery || c.notes.known(e);
        if known {
            crate::push_stack(v, (r[0], r[1], r[2], r[3]), 0.875, ItemStack { count: 1, ..e.out });
        } else {
            text_in(v, r[0] + r[2] * 0.5, r[1] + (r[3] - 8.0 * tk) * 0.5, tk, (r[0], r[0] + r[2]), "?", TEXT);
        }
        if !(known && book::ready(e, gw, c.inv, &c.screen.grid)) {
            quad(v, r, [0.0, 0.0, 0.0, 0.45]); // not now: icon and slot dimmed
        }
        if sat.sel == Some(ei) {
            frame_rect(v, r, (2.0 * u).round(), PICKED);
        }
    }
    if b.max_scroll > 0.0 {
        // A thin scroll bar under the strip.
        let (sw, at) = (b.strip[2], sat.scroll.clamp(0.0, b.max_scroll) / b.max_scroll);
        let bar = sw * sw / (sw + b.max_scroll);
        quad(v, [b.strip[0] + (sw - bar) * at, b.strip[1] + b.strip[3] + 1.5 * u, bar, 0.8 * u], [0.3, 0.3, 0.3, 0.7]);
    }

    // The column of variants: a small plate left of the strip, the picked one framed; the arrows are dim at either end.
    if let Some(col) = &b.var {
        plate(v, col.area, u);
        for &(ei, r) in &col.cells {
            cell(v, r, u, CELL);
            crate::push_stack(v, (r[0], r[1], r[2], r[3]), 0.875, ItemStack { count: 1, ..entries[ei].out });
            if sat.sel == Some(ei) {
                frame_rect(v, r, (2.0 * u).round(), PICKED);
            }
        }
        tri(v, col.up, true, if col.pos > 0 { TEXT } else { PLATE_LO });
        tri(v, col.down, false, if col.pos + 1 < col.len { TEXT } else { PLATE_LO });
    }

    well(v, b.left, u);
    well(v, b.right, u);

    // Left panel: the name centred and a line under it (what is missing, what a tap does), the pin.
    let l = b.left;
    let sel = sat.sel.and_then(|i| entries.get(i));
    let known = sel.map_or(true, |e| !c.discovery || c.notes.known(e));
    let shown = sel.filter(|_| known);
    let (cx, m) = (l[0] + l[2] * 0.5, b.pin[2] + 2.0 * ip); // the text keeps clear of the pin on both sides, so it stays centred
    let (lo, hi, ny) = (l[0] + m, l[0] + l[2] - m, l[1] + ip);
    match (sel, shown) {
        (None, _) => text_in(v, cx, ny, tk, (lo, hi), "Pick a recipe above", TEXT),
        (Some(_), None) => {
            text_in(v, cx, ny, tk, (lo, hi), "???", TEXT);
            text_in(v, cx, ny + 8.0 * tk + 2.0 * u, sk, (lo, hi), "Find one of its items to learn it", WARN_I);
        }
        (Some(_), Some(e)) => {
            let name = items::name(e.out).unwrap_or("?");
            let head = if e.out.count > 1 { format!("{name} x{}", e.out.count) } else { name.to_string() };
            text_in(v, cx, ny, tk, (lo, hi), &head, TEXT);
            let short = book::missing(e, c.inv, &c.screen.grid);
            let (note, col) = match (e.src, e.grid) {
                (Src::Smelt(_), _) => ("Smelt it in a furnace", WARN_I),
                (_, g) if g > gw => ("Needs a workbench (3x3)", WARN_I),
                _ if short.is_empty() => ("Tap again to craft, hold for all", OK_T),
                _ => ("Missing ingredients", NO_T),
            };
            text_in(v, cx, ny + 8.0 * tk + 2.0 * u, sk, (lo, hi), note, col);
        }
    }
    // The pin: a picture that turns red when this recipe is pinned (dim while none is picked).
    let pinned = sat.sel.is_some_and(|s| c.notes.pinned == Some(s));
    let a = if sat.sel.is_some() { 1.0 } else { 0.4 };
    HudPipeline::push_gui_tinted(v, b.pin[0] + (b.pin[2] - 10.0 * u) * 0.5, b.pin[1] + (b.pin[3] - 10.0 * u) * 0.5, 10.0 * u, 10.0 * u, if pinned { art::PIN_ON } else { art::PIN }, [1.0, 1.0, 1.0, a]);

    // The preview: the recipe's own ingredients in their places, a slot that the bag cannot fill red with a yellow "!" (not a grid to fill:
    // it is only shown).
    let pg = (b.grid.len() as f32).sqrt().round() as usize;
    let cells = shown.map_or([0u16; 9], |e| book::preview(e, pg));
    let short = shown.map_or_else(Vec::new, |e| book::missing(e, c.inv, &c.screen.grid));
    for (i, r) in b.grid.iter().enumerate() {
        let lacking = cells[i] != 0 && short.iter().any(|s| s.0 == cells[i]);
        cell(v, *r, u, if lacking { RED } else { CELL });
        if cells[i] != 0 {
            crate::push_stack(v, (r[0], r[1], r[2], r[3]), 0.875, item(cells[i]));
        }
        if lacking {
            badge(v, *r, u);
        }
    }
    arrow_right(v, b.arrow, [0.62, 0.62, 0.62, 1.0]);
    let ready = shown.is_some_and(|e| book::ready(e, gw, c.inv, &c.screen.grid));
    let lacking = shown.is_some() && !ready;
    slot_in(v, b.result, shown.map(|e| e.out), None, u, if lacking { RED } else { CELL });
    if lacking {
        badge(v, b.result, u);
    }
    if c.hold {
        let r = b.result;
        text_in(v, r[0] + r[2] * 0.5, r[1] - 10.0 * sk, sk, (l[0] + ip, l[0] + l[2] - ip), "Release: craft all", OK_T);
    }

    // The discovery toggle at the foot.
    let label = if c.discovery { "Unknown recipes: hidden" } else { "Unknown recipes: shown" };
    btn(v, b.hide, (sk, u), label, 1);

    // Right panel: its title; the slots are drawn by `draw`, the ingredient list here.
    let r = b.right;
    let title = if sat.info { "Ingredients" } else { "Inventory" };
    text_in(v, r[0] + r[2] * 0.5, r[1] + ip, tk, (r[0] + ip, r[0] + r[2] - ip), title, TEXT);
    if sat.info {
        draw_card(v, sat, b, c, (tk, sk, u));
    }
}

/// The card's sizes from its rect `r`, the small text size and the sprite pixel, for `n` ingredients: the header (icon + name + note) is fixed,
/// the ingredients lie five to a row below it and scroll a whole row at a time (no scissor here, so a row half out of the card is not drawn).
struct CardM {
    isz: f32,
    top: f32,
    cw: f32,
    chip: f32,
    pitch: f32,
    max: f32,
}

fn card_m(r: [f32; 4], (sk, u): (f32, f32), n: usize) -> CardM {
    let isz = (r[2] * 0.12).floor();
    let top = isz + 6.0 * u;
    let cw = r[2] / 5.0;
    let chip = (cw * 0.6).floor();
    let pitch = chip + 12.0 * sk + 2.0 * u;
    let rows = ((n + 4) / 5) as f32;
    CardM { isz, top, cw, chip, pitch, max: (rows * pitch - (r[3] - top - u)).max(0.0) }
}

/// Brighter than `OK_T` / `NO_T` / `WARN_I`: these sit on a black outline (the pinned recipe on the play HUD), not on a grey panel.
const GOOD_T: [f32; 4] = [0.45, 0.95, 0.35, 1.0];
const BAD_T: [f32; 4] = [1.0, 0.4, 0.35, 1.0];
const WARN_T: [f32; 4] = [1.0, 0.72, 0.2, 1.0];

/// The picked recipe's ingredients in the right panel: result, a line on where it is made, the ingredients with what the bag has of each
/// (dark text on the grey; the ingredient rows scroll).
fn draw_card(v: &mut Vec<HudVertex>, sat: &Satchel, b: &CraftFrame, c: &Ctx, (tk, sk, u): (f32, f32, f32)) {
    let r = b.card;
    let o = 0.0;
    let Some(e) = sat.sel.and_then(|i| book::entries().get(i)) else {
        paragraph(v, r, (sk, u), "Pick a recipe in the strip above: tap it again to craft it.", TEXT, o);
        return;
    };
    if c.discovery && !c.notes.known(e) {
        paragraph(v, r, (sk, u), "??? Find one of its items to learn this recipe.", TEXT, o);
        return;
    }
    let m = card_m(r, (sk, u), e.need.len());
    crate::push_stack(v, (r[0] + 3.0 * u, r[1] + 3.0 * u, m.isz, m.isz), 0.875, ItemStack { count: 1, ..e.out });
    let name = items::name(e.out).unwrap_or("?");
    let head = if e.out.count > 1 { format!("{name} x{}", e.out.count) } else { name.to_string() };
    let (tx, gw) = (r[0] + 6.0 * u + m.isz, c.screen.gw);
    let mw = r[0] + r[2] - 3.0 * u - tx;
    text_fit_o(v, (tx, r[1] + 2.0 * u), tk, mw, &head, TEXT, o);
    let short = book::missing(e, c.inv, &c.screen.grid);
    let (note, col) = match (e.src, e.grid) {
        (Src::Smelt(_), _) => ("Smelt it in a furnace", WARN_I),
        (_, g) if g > gw => ("Needs a workbench (3x3)", WARN_I),
        _ if short.is_empty() => ("Tap again to craft", OK_T),
        _ => ("Missing ingredients", NO_T),
    };
    text_fit_o(v, (tx, r[1] + 2.0 * u + 8.0 * tk + u), sk, mw, note, col, o);
    // Ingredients, five to a row: the item, then what the bag has over what the recipe wants. ponytail: the scroll snaps to a row.
    let (y0, rh) = (r[1] + m.top, m.chip + 8.0 * sk + 2.0 * u);
    let off = (sat.card_scroll.clamp(0.0, m.max) / m.pitch).round() * m.pitch;
    for (n, &(code, need)) in e.need.iter().enumerate() {
        let (x, y) = (r[0] + (n % 5) as f32 * m.cw + (m.cw - m.chip) * 0.5, y0 + (n / 5) as f32 * m.pitch - off);
        if y < y0 - 0.5 || y + rh > r[1] + r[3] - 0.5 {
            continue;
        }
        crate::push_stack(v, (x, y, m.chip, m.chip), 0.875, item(code));
        let have = book::have(c.inv, &c.screen.grid, code);
        let t = format!("{have}/{need}");
        let lo = x - (m.cw - m.chip) * 0.5; // this ingredient's column
        text_in_o(v, x + m.chip * 0.5, y + m.chip + u, sk, (lo, lo + m.cw), &t, if have >= u32::from(need) { OK_T } else { NO_T }, o);
    }
    if m.max > 0.0 {
        // A thin scroll bar along the card's right edge.
        let (vh, at) = (r[3] - m.top, sat.card_scroll.clamp(0.0, m.max) / m.max);
        let bar = vh * vh / (vh + m.max);
        HudPipeline::push_quad(v, r[0] + r[2] - 1.5 * u, y0 + (vh - bar) * at, 0.8 * u, bar, [0.3, 0.2, 0.1, 0.6]);
    }
}

/// The pinned recipe on the play HUD, a small card at the left edge: the result, then every ingredient with what the bag holds of it.
pub fn draw_pin(v: &mut Vec<HudVertex>, (w, h): (f32, f32), e: &book::Entry, inv: &Inventory) {
    let k = craft::panel(0, w, h).2 * 0.5;
    let (x, y, row) = (0.012 * w, 0.28 * h, 18.0 * k);
    let n = e.need.len() as f32 + 1.0;
    HudPipeline::push_quad(v, x, y, 90.0 * k, row * n + 4.0 * k, [0.0, 0.0, 0.0, 0.45]);
    let name = items::name(e.out).unwrap_or("?");
    crate::push_stack(v, (x + 2.0 * k, y + 2.0 * k, 16.0 * k, 16.0 * k), 0.875, ItemStack { count: 1, ..e.out });
    HudPipeline::push_text(v, x + 20.0 * k, y + 2.0 * k + 4.0 * k, k, name, [1.0; 4]);
    for (i, &(code, need)) in e.need.iter().enumerate() {
        let ry = y + 2.0 * k + row * (i as f32 + 1.0);
        crate::push_stack(v, (x + 2.0 * k, ry, 16.0 * k, 16.0 * k), 0.875, item(code));
        let have = book::have(inv, &[None; 9], code);
        let t = format!("{have}/{need}");
        HudPipeline::push_text(v, x + 20.0 * k, ry + 4.0 * k, k, &t, if have >= u32::from(need) { [0.5, 1.0, 0.5, 1.0] } else { [1.0, 0.45, 0.4, 1.0] });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (f32, f32) = (1604.0, 720.0);
    const SIZES: [(f32, f32); 4] = [(1604.0, 720.0), (1280.0, 720.0), (2400.0, 1080.0), (1920.0, 1080.0)];

    fn tab(craft: bool) -> Satchel {
        let mut s = Satchel::new(craft);
        s.tick(10.0);
        s
    }

    fn frame(sat: &Satchel, inv: &Inventory, gw: usize) -> Frame {
        Frame::new(sat, inv, gw, SIZE.0, SIZE.1)
    }

    fn centre(r: [f32; 4]) -> (f32, f32) {
        (r[0] + r[2] * 0.5, r[1] + r[3] * 0.5)
    }

    fn on_screen(r: [f32; 4], (w, h): (f32, f32)) -> bool {
        r[0] >= 0.0 && r[1] >= 0.0 && r[0] + r[2] <= w && r[1] + r[3] <= h
    }

    #[test]
    fn every_slot_has_one_cell_that_a_finger_finds_on_both_tabs() {
        let mut inv = Inventory::default();
        inv.slots[12] = Some(ItemStack { id: 17, count: 5, damage: 0 });
        for sat in [tab(false), tab(true)] {
            let f = frame(&sat, &inv, 2);
            for i in 0..MAIN {
                assert_eq!(f.cells.iter().filter(|(id, _)| *id == SlotId::Inv(i)).count(), 1, "slot {i}");
            }
            for &(id, r) in &f.cells {
                let (x, y) = centre(r);
                assert_eq!(f.hit(&sat, x, y), Pick::Slot(id));
                assert!(on_screen(r, SIZE), "on the screen: {r:?}");
            }
        }
        // The armor slots are on the Bag tab only.
        let f = frame(&tab(false), &inv, 2);
        assert!((MAIN..MAIN + 4).all(|i| f.cells.iter().filter(|(id, _)| *id == SlotId::Inv(i)).count() == 1));
        assert!(frame(&tab(true), &inv, 2).cells.iter().all(|(id, _)| !matches!(id, SlotId::Inv(i) if *i >= MAIN)));
    }

    /// The slots touch each other (rows and columns), start clear of the panel's 8-pixel border and its seam, and the pocket bands stay
    /// inside the border.
    #[test]
    fn bag_slots_touch_and_sit_clear_of_the_border() {
        for (w, h) in SIZES {
            let f = Frame::new(&tab(false), &Inventory::default(), 2, w, h);
            let u = units(f.k).0;
            let (lo, hi) = (f.bag[0] + 10.0 * u, f.bag[0] + f.bag[2] - 10.0 * u);
            for &(_, r) in &f.cells {
                assert!(r[0] >= lo - 0.01 && r[0] + r[2] <= hi + 0.01, "a slot is clear of the border at {w}x{h}: {r:?}");
                assert_eq!(r[2], r[3], "slots are square");
            }
            for &(r, _) in &f.rows {
                assert!(r[0] >= f.bag[0] + 8.0 * u && r[0] + r[2] <= f.bag[0] + f.bag[2] - 8.0 * u, "a band stays inside the border: {r:?}");
            }
            let first = f.cells.iter().filter(|(id, _)| matches!(id, SlotId::Inv(i) if (9..MAIN).contains(i))).map(|&(_, r)| r).collect::<Vec<_>>();
            for pair in first.windows(2).filter(|p| p[0][1] == p[1][1]) {
                assert!((pair[1][0] - pair[0][0] - pair[0][2]).abs() < 0.01, "neighbours touch");
            }
            assert!(f.bag[1] + f.bag[3] <= h * 0.92, "the panel fits the height at {w}x{h}, with room for the pad's hints");
        }
    }

    /// Armor, bag and belt are three blocks with a clear gap between them.
    #[test]
    fn the_blocks_are_apart() {
        let f = frame(&tab(false), &Inventory::default(), 2);
        let u = units(f.k).0;
        let row = |id: SlotId| f.cells.iter().find(|(i, _)| *i == id).expect("cell").1;
        let (armor, top, last, belt) = (row(SlotId::Inv(MAIN)), row(SlotId::Inv(9)), row(SlotId::Inv(35)), row(SlotId::Inv(0)));
        assert!(top[1] - (armor[1] + armor[3]) >= 11.0 * u && belt[1] - (last[1] + last[3]) >= 11.0 * u, "gaps between the blocks");
    }

    /// The tabs sit on the panel's edge and a finger on one finds it: `Bag` and `Craft`, or `Bag` and every category.
    #[test]
    fn the_tabs_switch_between_the_bag_and_the_categories() {
        let inv = Inventory::default();
        let f = frame(&tab(false), &inv, 2);
        assert_eq!(f.tabs.iter().map(|t| t.0).collect::<Vec<_>>(), [Pick::Btn(Btn::Bag), Pick::Btn(Btn::Craft)]);
        let sat = tab(true);
        let f = frame(&sat, &inv, 2);
        assert_eq!(f.tabs.len(), 1 + CATS.len());
        for &(p, r) in &f.tabs {
            let (x, y) = centre(r);
            assert_eq!(f.hit(&sat, x, y), p);
            assert!(on_screen(r, SIZE) && r[1] + r[3] <= f.bag[1] + 0.01, "a tab lies on the panel's top edge: {r:?}");
        }
        assert_eq!(f.tabs[3].0, Pick::Cat(2));
        let mut s = Satchel::new(false);
        s.turn(99);
        assert!(s.craft && s.cat == CATS.len() - 1, "a category tab opens the Craft tab, clamped");
    }

    /// The strip, the recipe's panel and the bag stay on the panel on every screen shape, the result next to the arrow inside the left panel.
    #[test]
    fn the_craft_tab_fits_every_screen() {
        for (w, h) in SIZES.into_iter().chain([(800.0, 480.0), (2340.0, 1080.0)]) {
            for gw in [2, 3] {
                let mut sat = tab(true);
                sat.sel = Some(0);
                let f = Frame::new(&sat, &Inventory::default(), gw, w, h);
                let b = f.craft.as_ref().expect("craft");
                let inner = |r: [f32; 4], p: [f32; 4]| r[0] >= p[0] - 0.01 && r[1] >= p[1] - 0.01 && r[0] + r[2] <= p[0] + p[2] + 0.01 && r[1] + r[3] <= p[1] + p[3] + 0.01;
                assert!(on_screen(f.bag, (w, h)) && f.bag[1] + f.bag[3] <= h * 0.92, "the panel at {w}x{h}");
                assert!(inner(b.strip, f.bag) && inner(b.left, f.bag) && inner(b.right, f.bag), "{w}x{h}");
                for r in b.grid.iter().chain([&b.result, &b.pin, &b.hide, &b.arrow]) {
                    assert!(inner(*r, b.left), "{r:?} on the left panel at {w}x{h} ({gw})");
                }
                assert!(inner(b.title, f.bag) && b.title[1] + b.title[3] <= b.strip[1] + 0.01, "the title is over the strip at {w}x{h}");
                assert!(b.arrow[0] + b.arrow[2] <= b.result[0] + 0.01, "the arrow is left of the result");
                assert!(b.grid.last().is_some_and(|g| g[0] + g[2] <= b.arrow[0] + 0.01), "the grid is left of the arrow");
                for &(_, r) in &f.cells {
                    assert!(inner(r, b.right), "a slot is on the right panel at {w}x{h}: {r:?}");
                }
                for &(_, r) in &b.rows {
                    assert!(inner(r, b.strip), "{r:?}");
                }
                assert!(b.pin[1] + b.pin[3] <= b.grid[0][1] + 0.01, "the pin does not reach the preview");
            }
        }
    }

    /// The strip shows whole slots, a drag moves it a whole slot at a time, and it ends where the slots do.
    #[test]
    fn the_strip_scrolls_a_whole_slot_at_a_time() {
        let inv = Inventory::default();
        let mut sat = tab(true);
        let gs = groups(&strip_ids(&sat, 2, &inv, &[None; 9]));
        let f = frame(&sat, &inv, 2);
        let b = f.craft.as_ref().expect("craft");
        assert_eq!(b.rows.len() as f32, (b.strip[2] / b.row_w).round(), "the strip is full");
        assert!(b.max_scroll > 0.0 && (b.max_scroll / b.row_w).fract().abs() < 1e-3, "tools & gear do not fit one screen");
        let first = b.rows[0].0;
        sat.scroll = 2.4 * b.row_w;
        let f = frame(&sat, &inv, 2);
        let b = f.craft.as_ref().expect("craft");
        assert_eq!(b.rows[0].0, gs[2][0], "two whole slots over");
        assert_ne!(b.rows[0].0, first);
        sat.scroll = 1e9;
        let f = frame(&sat, &inv, 2);
        let b = f.craft.as_ref().expect("craft");
        assert_eq!(b.rows.last().map(|r| r.0), gs.last().map(|g| g[0]), "the last slot is in view at the end");
    }

    /// The recipes the bag can make come first and keep that order where a finger and the pad find them; `Craftable` leaves only those.
    #[test]
    fn the_strip_puts_what_can_be_made_first_and_can_hide_the_rest() {
        let mut inv = Inventory::default();
        inv.slots[10] = Some(ItemStack { id: 17, count: 3, damage: 0 });
        let blocks = CATS.iter().position(|&c| c == book::Cat::Blocks).expect("blocks");
        let mut sat = tab(true);
        sat.turn(blocks);
        let planks = book::entries().iter().position(|e| e.out.id == 5 && e.grid == 2 && matches!(e.src, Src::Craft(_))).expect("planks");
        let f = Frame::with_grid(&sat, &inv, &[None; 9], 2, SIZE.0, SIZE.1);
        let rows = &f.craft.as_ref().expect("craft").rows;
        assert_eq!(rows[0].0, planks, "logs in the bag: planks are the first slot, showing the kind the bag can make");
        let (x, y) = centre(rows[0].1);
        assert_eq!(f.hit(&sat, x, y), Pick::Row(planks));
        // With an empty bag the category is in its own order again.
        let empty = Frame::with_grid(&sat, &Inventory::default(), &[None; 9], 2, SIZE.0, SIZE.1);
        let (es, first) = (book::entries(), empty.craft.as_ref().expect("craft").rows[0].0);
        assert_eq!(es[first].out.id, es[book::in_cat(book::Cat::Blocks)[0]].out.id);
        // Craftable only: every slot left can be made now, and an empty bag leaves none.
        sat.only = true;
        let f = Frame::with_grid(&sat, &inv, &[None; 9], 2, SIZE.0, SIZE.1);
        let rows = &f.craft.as_ref().expect("craft").rows;
        assert!(!rows.is_empty() && rows.iter().all(|&(i, _)| book::ready(&book::entries()[i], 2, &inv, &[None; 9])));
        let none = Frame::with_grid(&sat, &Inventory::default(), &[None; 9], 2, SIZE.0, SIZE.1);
        assert!(none.craft.as_ref().expect("craft").rows.is_empty());
    }

    /// The preview is 1 x 1 for a smelting, 2 x 2 or 3 x 3 for a recipe by what it needs and the screen has; the result is a slot to press.
    #[test]
    fn the_preview_fits_the_recipe_and_the_result_is_pressable() {
        let inv = Inventory::default();
        let pick = book::entries().iter().position(|e| e.out.id == 270 && e.grid == 3).expect("wooden pickaxe");
        let smelt = book::entries().iter().position(|e| e.src == Src::Smelt(15)).expect("iron ore");
        let planks = book::entries().iter().position(|e| e.out.id == 5 && e.grid == 2 && matches!(e.src, Src::Craft(_))).expect("planks");
        for (gw, sel, side) in [(2, pick, 3), (3, planks, 3), (2, planks, 2), (2, smelt, 1)] {
            let mut sat = tab(true);
            sat.sel = Some(sel);
            let f = frame(&sat, &inv, gw);
            let b = f.craft.as_ref().expect("craft");
            assert_eq!(b.grid.len(), side * side, "gw {gw} entry {sel}");
            let (x, y) = centre(b.result);
            assert_eq!(f.hit(&sat, x, y), Pick::Slot(SlotId::Result));
            assert!(b.result[2] > b.grid[0][2], "the result is bigger than an ingredient");
        }
    }

    #[test]
    fn a_finger_past_everything_throws_and_the_panels_do_not() {
        let inv = Inventory::default();
        for sat in [tab(false), tab(true)] {
            let f = frame(&sat, &inv, 2);
            assert_eq!(f.hit(&sat, 2.0, SIZE.1 - 2.0), Pick::Outside);
            assert_eq!(f.hit(&sat, f.bag[0] + 1.0, f.bag[1] + f.bag[3] - 1.0), Pick::Panel);
            let (x, y) = centre(f.btns.iter().find(|(b, _)| *b == Btn::Close).expect("close").1);
            assert_eq!(f.hit(&sat, x, y), Pick::Btn(Btn::Close));
        }
        let sat = tab(true);
        let f = frame(&sat, &inv, 2);
        let b = f.craft.as_ref().expect("craft");
        assert_eq!(f.hit(&sat, b.left[0] + 1.0, b.left[1] + b.left[3] - 1.0), Pick::Panel);
    }

    /// A pad steps from slot to slot, down a column, from the bag's top row up over the armor row to the tabs, and never off a screen with
    /// nothing that way; every target answers a finger with something on both tabs.
    #[test]
    fn a_pad_steps_between_targets() {
        let inv = Inventory::default();
        let sat = tab(false);
        let f = frame(&sat, &inv, 2);
        let t = f.targets(&sat);
        let cell = |id: SlotId| f.cells.iter().find(|(i, _)| *i == id).expect("cell").1;
        let (a, b) = (cell(SlotId::Inv(9)), cell(SlotId::Inv(10)));
        assert_eq!(step(&t, centre(a), (1, 0)), Some(b));
        assert_eq!(step(&t, centre(b), (-1, 0)), Some(a));
        assert_eq!(step(&t, centre(a), (0, 1)), Some(cell(SlotId::Inv(18))));
        let armor_up = step(&t, centre(a), (0, -1)).expect("the armor row is above");
        assert!(f.cells.iter().any(|&(id, r)| r == armor_up && matches!(id, SlotId::Inv(i) if i >= MAIN)));
        let up = step(&t, centre(armor_up), (0, -1)).expect("a tab or a button is above the armor");
        assert!(f.tabs.iter().any(|&(_, r)| r == up) || f.btns.iter().any(|&(_, r)| r == up));
        let below = step(&t, centre(cell(SlotId::Inv(0))), (0, 1)).expect("the row of hints is under the belt");
        assert!(f.btns.iter().any(|&(b, r)| r == below && b == Btn::Close), "Exit");
        assert_eq!(step(&t, centre(below), (0, 1)), None, "nothing lies below the hints");
        // Off every target (a tab changed under the cursor): the nearest one, in any direction.
        let hole = (1.0, 1.0);
        assert!(t.iter().all(|r| !inside(*r, hole.0, hole.1)));
        assert!(step(&t, hole, (0, 1)).is_some());
        for (craft, info) in [(false, false), (true, false), (true, true)] {
            let mut sat = tab(craft);
            (sat.sel, sat.info) = (Some(0), info);
            let f = frame(&sat, &inv, 2);
            for r in &f.targets(&sat) {
                let (x, y) = centre(*r);
                assert!(on_screen(*r, SIZE) && !matches!(f.hit(&sat, x, y), Pick::Panel | Pick::Outside | Pick::List | Pick::Card), "{r:?} (craft {craft}, info {info})");
            }
        }
        // On the Craft tab a recipe is a step right of the previous one and the result is a target.
        let sat = tab(true);
        let f = frame(&sat, &inv, 2);
        let rows = &f.craft.as_ref().expect("craft").rows;
        assert_eq!(step(&f.targets(&sat), centre(rows[0].1), (1, 0)), Some(rows[1].1));
    }

    /// A count lies inside its cell on every screen shape.
    #[test]
    fn a_count_stays_inside_its_cell() {
        for (w, h) in SIZES {
            let f = Frame::new(&tab(false), &Inventory::default(), 2, w, h);
            let u = units(f.k).0;
            for &(_, r) in f.cells.iter().step_by(7) {
                for n in [2, 64] {
                    let mut v = Vec::new();
                    count(&mut v, r, u, n);
                    assert!(!v.is_empty());
                    assert!(v.iter().all(|q| q.pos[0] >= r[0] + u && q.pos[0] <= r[0] + r[2] - 0.5 * u && q.pos[1] >= r[1] + u && q.pos[1] <= r[1] + r[3] - 0.5 * u), "{n} in {r:?} at {w}x{h}");
                }
            }
        }
    }

    /// A text is stepped down whole pixels and then cut until it fits; a paragraph wraps within its width.
    #[test]
    fn text_is_fitted_and_wrapped_within_its_box() {
        for (s, k, maxw) in [("Redstone & rails", 3.0, 150.0), ("Redstone & rails", 3.0, 40.0), ("Tap again to craft, hold for all", 2.0, 60.0), ("X", 3.0, 100.0)] {
            let (kk, t) = fitted(s, k, maxw);
            assert!(kk >= 1.0 && kk <= k && kk.fract() == 0.0 && HudPipeline::text_width(&t) as f32 * kk <= maxw.max(8.0), "{s}: {kk} {t}");
        }
        assert_eq!(fitted("X", 3.0, 100.0), (3.0, "X".to_string()), "a text that fits keeps its size");
        for l in lines("Pick a recipe above: tap it again to craft it.", 2.0, 150.0) {
            assert!(HudPipeline::text_width(&l) as f32 * 2.0 <= 150.0 || !l.contains(' '), "{l}");
        }
        assert_eq!(units(3.99), (3.0, 2.0));
    }

    #[test]
    fn a_slot_rim_is_tinted_by_category() {
        assert_ne!(tint(270), tint(297), "a pickaxe and bread differ");
        assert_ne!(tint(297), tint(17), "bread and a log differ");
    }

    #[test]
    fn cells_are_big_enough_for_a_thumb_on_both_tabs() {
        for craft in [false, true] {
            let f = frame(&tab(craft), &Inventory::default(), 2);
            let min = f.cells.iter().map(|(_, r)| r[2]).fold(f32::MAX, f32::min);
            assert!(min >= 0.08 * SIZE.1, "a slot is {min} px (craft {craft})");
        }
    }

    /// The pin is a box in the recipe's header, the ingredient list shows at least one row and scrolls only when there are more rows than
    /// fit, and the toggle sits under the preview.
    #[test]
    fn the_recipe_panel_has_a_pin_a_toggle_and_a_scrolling_list() {
        let inv = Inventory::default();
        for gw in [2, 3] {
            let mut sat = tab(true);
            (sat.sel, sat.info) = (Some(0), true);
            let f = frame(&sat, &inv, gw);
            let b = f.craft.as_ref().expect("craft");
            let (u, sk) = units(f.k);
            let (x, y) = centre(b.pin);
            assert_eq!(f.hit(&sat, x, y), Pick::Pin);
            let (x, y) = centre(b.hide);
            assert_eq!(f.hit(&sat, x, y), Pick::Hide);
            assert!(b.grid.last().is_some_and(|g| g[1] + g[3] <= b.hide[1] + 0.01), "the toggle is under the preview");
            let one = card_m(b.card, (sk, u), 1);
            assert!(b.card[3] - one.top >= one.chip + 8.0 * sk + 2.0 * u, "a row of ingredients fits the list ({gw}x{gw})");
            assert_eq!(one.max, 0.0, "one row never scrolls");
            assert_eq!(card_m(b.card, (sk, u), 5).max, 0.0, "five fit in one row");
            assert!(card_m(b.card, (sk, u), 11).max > 0.0, "three rows scroll");
            let (x, y) = centre(b.card);
            assert_eq!(f.hit(&sat, x, y), Pick::Card);
            assert!(f.cells.iter().all(|(id, _)| !matches!(id, SlotId::Inv(_))), "the bag gives way to the list");
        }
    }

    /// Recipes that make the same item share a slot, a column left of the strip shows the picked one's others, and an arrow, a drag or a pad
    /// steps through them.
    #[test]
    fn recipes_that_make_the_same_item_share_a_slot_and_a_column_steps_through_them() {
        let inv = Inventory::default();
        let blocks = CATS.iter().position(|&c| c == book::Cat::Blocks).expect("blocks");
        let mut sat = tab(true);
        sat.turn(blocks);
        let es = book::entries();
        let planks: Vec<usize> = es.iter().enumerate().filter(|(_, e)| e.out.id == 5 && matches!(e.src, Src::Craft(_))).map(|(i, _)| i).collect();
        assert!(planks.len() >= 2, "planks of each kind of log");
        let f = frame(&sat, &inv, 2);
        let b = f.craft.as_ref().expect("craft");
        assert_eq!(b.rows.iter().filter(|&&(i, _)| es[i].out.id == 5).count(), 1, "one slot for all the planks");
        assert!(b.var.is_none(), "nothing picked: no column");
        sat.sel = Some(planks[0]);
        let f = frame(&sat, &inv, 2);
        let b = f.craft.as_ref().expect("craft");
        let col = b.var.as_ref().expect("the column");
        assert_eq!((col.len, col.pos, col.cells.len()), (planks.len(), 0, planks.len().min(3)));
        assert!(col.area[0] + col.area[2] <= b.strip[0] && col.area[1] + col.area[3] <= b.left[1] + 0.01, "left of the strip, over the panels");
        assert_eq!(f.hit(&sat, centre(col.up).0, centre(col.up).1), Pick::VarUp);
        assert_eq!(f.hit(&sat, centre(col.down).0, centre(col.down).1), Pick::VarDown);
        let (x, y) = centre(col.cells[0].1);
        assert_eq!(f.hit(&sat, x, y), Pick::Var(planks[0]));
        assert_eq!(f.hit(&sat, col.area[0] + 1.0, col.area[1] + 1.0), Pick::VarList, "the column between is a place to drag");
        assert!(f.targets(&sat).contains(&col.up) && f.targets(&sat).contains(&col.cells[0].1), "a pad reaches the column");
        // Stepping: on from the first, nowhere before it; the slot shows the picked one.
        assert_eq!(variant(&sat, &inv, &[None; 9], 2, 1), Some(planks[1]));
        assert_eq!(variant(&sat, &inv, &[None; 9], 2, -1), None);
        sat.sel = Some(planks[1]);
        assert_eq!(variant(&sat, &inv, &[None; 9], 2, -1), Some(planks[0]));
        let f = frame(&sat, &inv, 2);
        assert!(f.craft.as_ref().expect("craft").rows.iter().any(|&(i, _)| i == planks[1]), "the slot shows the picked kind");
        // A recipe with no others has no column.
        sat.sel = book::entries().iter().position(|e| e.out.id == 58 && matches!(e.src, Src::Craft(_)));
        assert!(frame(&sat, &inv, 2).craft.as_ref().expect("craft").var.is_none());
    }

    /// The Exit button under the panel is as high as a thumb is wide, stays on the screen on the shapes the game runs on, clear of the other
    /// buttons and the tabs, and a finger finds it across its whole width.
    #[test]
    fn the_exit_button_is_under_the_panel_and_easy_to_hit() {
        for (w, h) in [(1604.0, 720.0), (1280.0, 720.0), (800.0, 480.0)] {
            for craft in [false, true] {
                let sat = tab(craft);
                let f = Frame::new(&sat, &Inventory::default(), 2, w, h);
                let exit = f.btns.iter().find(|(b, _)| *b == Btn::Close).expect("close").1;
                assert!(on_screen(exit, (w, h)), "on the screen at {w} x {h}: {exit:?}");
                assert!(exit[1] >= f.bag[1] + f.bag[3] && exit[3] >= 0.05 * h, "under the panel, at least {} px high: {exit:?}", 0.05 * h);
                let apart = |a: [f32; 4], b: [f32; 4]| a[0] >= b[0] + b[2] || b[0] >= a[0] + a[2] || a[1] >= b[1] + b[3] || b[1] >= a[1] + a[3];
                assert!(f.btns.iter().filter(|(b, _)| *b != Btn::Close).all(|(_, r)| apart(*r, exit)), "no other button touches it");
                assert!(f.tabs.iter().all(|(_, r)| apart(*r, exit)), "no tab touches it");
                for x in [exit[0] + 1.0, exit[0] + exit[2] * 0.5, exit[0] + exit[2] - 1.0] {
                    assert_eq!(f.hit(&sat, x, exit[1] + exit[3] * 0.5), Pick::Btn(Btn::Close), "a tap at x {x}");
                }
            }
        }
    }

    /// The hints name only what the press does: take / place on a slot (X: half / one), the result crafts, a recipe is picked and then
    /// crafted, a tab opens; B always closes.
    #[test]
    fn the_pad_hints_follow_what_is_under_the_cursor() {
        let slot = Pick::Slot(SlotId::Inv(0));
        let mut sat = tab(true);
        assert_eq!(pad_hints(&sat, slot, false), [(Role::Select, "Take"), (Role::Alt, "Take half"), (Role::Back, "Close")]);
        assert_eq!(pad_hints(&sat, slot, true), [(Role::Select, "Place"), (Role::Alt, "Place one"), (Role::Back, "Close")]);
        assert_eq!(pad_hints(&sat, Pick::Slot(SlotId::Result), false), [(Role::Select, "Craft"), (Role::Back, "Close")]);
        assert_eq!(pad_hints(&sat, Pick::Btn(Btn::Close), false)[0], (Role::Select, "Close"));
        assert_eq!(pad_hints(&sat, Pick::Btn(Btn::Craft), false)[0], (Role::Select, "Crafting"));
        assert_eq!(pad_hints(&sat, Pick::Cat(1), false)[0], (Role::Select, "Open"));
        assert_eq!(pad_hints(&sat, Pick::Row(3), false)[0], (Role::Select, "Pick"));
        sat.sel = Some(3);
        assert_eq!(pad_hints(&sat, Pick::Row(3), false)[0], (Role::Select, "Craft"));
        assert_eq!(pad_hints(&sat, Pick::Row(4), false)[0], (Role::Select, "Pick"));
        assert_eq!(pad_hints(&sat, Pick::Var(2), false)[0], (Role::Select, "Pick"));
        assert_eq!(pad_hints(&sat, Pick::VarDown, false)[0], (Role::Select, "Variant"));
        assert_eq!(pad_hints(&sat, Pick::VarList, false), [(Role::Back, "Close")]);
        assert_eq!(pad_hints(&sat, Pick::Panel, false), [(Role::Back, "Close")], "nothing to press on a panel");
        assert_eq!(pad_hints(&sat, Pick::Outside, true), [(Role::Select, "Throw"), (Role::Back, "Close")]);
        assert_eq!(pad_hints(&sat, Pick::Outside, false), [(Role::Back, "Close")]);
    }

    /// What the hints keep off covers every button, every tab and the panel, and so the strip, the preview, the result and the lists on it.
    #[test]
    fn the_hints_keep_off_the_buttons_the_tabs_and_the_panel() {
        let contains = |a: [f32; 4], r: [f32; 4]| r[0] >= a[0] - 0.01 && r[1] >= a[1] - 0.01 && r[0] + r[2] <= a[0] + a[2] + 0.01 && r[1] + r[3] <= a[1] + a[3] + 0.01;
        for craft in [false, true] {
            let mut sat = tab(craft);
            (sat.sel, sat.info) = (Some(0), craft);
            let f = frame(&sat, &Inventory::default(), 2);
            let avoid = f.avoid(&sat);
            let covered = |r: [f32; 4]| avoid.iter().any(|a| contains(*a, r));
            assert!(f.btns.iter().all(|(_, r)| covered(*r)), "every button, Close among them");
            assert!(f.tabs.iter().all(|(_, r)| covered(*r)), "every tab");
            assert!(f.cells.iter().all(|(_, r)| covered(*r)), "every slot");
            if let Some(b) = &f.craft {
                assert!(covered(b.strip) && covered(b.result) && covered(b.card) && b.grid.iter().all(|r| covered(*r)), "the strip, the preview, the result and the list");
            }
        }
    }
}
