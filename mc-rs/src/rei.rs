//! Item and recipe viewer after Roughly Enough Items (REI 26.2, shedaniel; its `ScreenOverlayImpl` entry list, `DefaultDisplayViewingScreen`,
//! `DefaultCraftingCategory`, `DefaultFuelCategory`), in GUI units like `stats_ui.rs`, drawn with the "Immersive Interfaces REI support" pack
//! (`tools/packs/immersive_rei`, `tools/gen_rei.py`, light or dark by `Settings::rei_theme`). UNVERIFIED: not compiled, not run.
//!
//! Touch, not mouse (REI: hover + `R` / `U` keys, click, page keys): the entry list is paged like REI's (arrows or a horizontal swipe), a TAP on an
//! entry shows its recipes (REI's `R`), a LONG press its uses (`U`), the held name is the tooltip; the mode button flips tap / long press to
//! REI's cheat mode (tap gives 1, long press a stack). In the recipe view the category tabs, the page row and the workstation follow REI; a tap on a
//! slot of a recipe opens that item's recipes, a long press its uses. Back returns through the history, then to the list.
//! Categories ported: Crafting, Smelting, Fuel (what `craft` has). Not ported: favorites, tags, plugins, auto crafting (`+`), `@mod` search.

use crate::input::touch_ui::ScreenEv;
use crate::menu::{rgb, scaled_gs, tw};
use crate::options::Settings;
use crate::render::hud::{sprites::rei as sp, HudPipeline, HudVertex};
use crate::world::craft;
use crate::world::items::{self, ItemStack};
use std::sync::OnceLock;
use std::time::Instant;

const LONG_SECS: f32 = 0.45;
const CELL: f32 = 20.0;
const WHITE: u32 = 0xFFFFFF;
const GREY: u32 = 0xA0A0A0;

/// An item as REI lists it: the id and the damage that tells variants apart (wool and dye colour, charcoal, slab kind).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Key {
    pub id: u16,
    pub dmg: u16,
}

impl Key {
    fn stack(self, count: u8) -> ItemStack {
        ItemStack { id: self.id, count, damage: self.dmg }
    }
    fn name(self) -> String {
        items::name(self.stack(1)).map_or_else(|| format!("#{}", self.id), str::to_string)
    }
}

/// Blocks without an item form (liquids, the bed / door / sign / crops / cake blocks, lit variants, fire, portal ...).
const SKIP: &[u16] = &[8, 9, 10, 11, 26, 34, 36, 43, 51, 52, 55, 59, 60, 62, 63, 64, 68, 71, 74, 75, 83, 90, 92, 93, 94, 95];

/// `EntryRegistry`: every named block and item, the wool and dye colours, the slab kinds, charcoal and the Cortes weapons.
fn catalog() -> &'static [Key] {
    static C: OnceLock<Vec<Key>> = OnceLock::new();
    C.get_or_init(|| {
        let mut v: Vec<Key> = Vec::new();
        for &(id, _) in crate::world::names::NAMES {
            if !SKIP.contains(&id) {
                v.push(Key { id, dmg: 0 });
            }
        }
        v.extend((0..16).map(|dmg| Key { id: 35, dmg }));
        v.extend((0..16).map(|dmg| Key { id: 351, dmg }));
        v.extend((0..3).map(|dmg| Key { id: 44, dmg }));
        v.push(Key { id: 263, dmg: 1 });
        v.extend((400..=430).filter(|&id| items::name(ItemStack { id, count: 1, damage: 0 }).is_some()).map(|id| Key { id, dmg: 0 }));
        v.sort_by_key(|k| (k.id, k.dmg));
        v.dedup();
        v
    })
}

/// A recipe the viewer shows: `DisplaySpec`.
#[derive(Clone, Debug, PartialEq)]
pub enum Disp {
    Craft { w: usize, cells: [u16; 9], out: ItemStack },
    Smelt { inp: Key, out: ItemStack },
    Fuel { item: Key, ticks: u16 },
}

/// A `craft::code`: the id, and the colour above bit 9 for wool and dye.
fn decode(c: u16) -> Key {
    let id = c & 511;
    Key { id, dmg: if matches!(id, 35 | 351) { c >> 9 } else { 0 } }
}

fn same(a: Key, b: Key) -> bool {
    a.id == b.id && (!matches!(a.id, 35 | 351 | 263) || a.dmg == b.dmg)
}

/// `ViewSearchBuilder.addRecipesFor` (`uses` false) / `addUsagesFor` (true), per category: crafting, smelting, fuel.
pub fn displays(k: Key, uses: bool) -> [Vec<Disp>; 3] {
    let mut d: [Vec<Disp>; 3] = Default::default();
    for r in craft::recipes() {
        let (w, _, cells, out) = r.view();
        let hit = if uses { cells.iter().any(|&c| c != 0 && same(decode(c), k)) } else { same(Key { id: out.id, dmg: out.damage }, k) };
        if hit {
            d[0].push(Disp::Craft { w, cells, out });
        }
    }
    for id in 1..512u16 {
        if let Some(out) = craft::smelting(id) {
            let inp = Key { id, dmg: 0 };
            if if uses { same(inp, k) } else { same(Key { id: out.id, dmg: out.damage }, k) } {
                d[1].push(Disp::Smelt { inp, out });
            }
        }
    }
    let t = craft::fuel_ticks(k.id);
    if uses && t > 0 {
        d[2].push(Disp::Fuel { item: k, ticks: t });
    }
    d
}

/// The workstation of a category (`CategoryRegistry.getWorkstations`), and the tab icon.
const WORK: [u16; 3] = [58, 61, 61];
const TAB: [u16; 3] = [58, 61, 263];
const CAT: [&str; 3] = ["Crafting", "Smelting", "Fuel"];

#[derive(Clone, Debug)]
struct View {
    target: Key,
    uses: bool,
    cat: usize,
    page: usize,
    disp: [Vec<Disp>; 3],
}

impl View {
    fn new(target: Key, uses: bool) -> Self {
        let disp = displays(target, uses);
        let cat = (0..3).find(|&c| !disp[c].is_empty()).unwrap_or(0);
        Self { target, uses, cat, page: 0, disp }
    }
    fn cats(&self) -> Vec<usize> {
        (0..3).filter(|&c| !self.disp[c].is_empty()).collect()
    }
}

/// What the viewer asks of the game.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Out {
    Close,
    Give(ItemStack),
}

struct Press {
    at: (f32, f32),
    t: Instant,
    drag: bool,
}

pub struct ReiUi {
    /// REI's cheat mode: a tap gives the item (a long press a stack) instead of opening its recipes (long press: uses).
    cheat: bool,
    query: String,
    typing: bool,
    list: Vec<Key>,
    page: usize,
    view: Option<View>,
    hist: Vec<View>,
    press: Option<Press>,
    finger: Option<(f32, f32)>,
}

fn inside(r: [f32; 4], x: f32, y: f32) -> bool {
    x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]
}

fn quad(v: &mut Vec<HudVertex>, s: f32, r: [f32; 4], c: [f32; 4]) {
    HudPipeline::push_quad(v, r[0] * s, r[1] * s, r[2] * s, r[3] * s, c);
}

fn text(v: &mut Vec<HudVertex>, s: f32, x: f32, y: f32, t: &str, c: u32) {
    HudPipeline::push_text(v, x * s, y * s, s, t, rgb(c));
}

fn centred(v: &mut Vec<HudVertex>, s: f32, r: [f32; 4], t: &str, c: u32) {
    text(v, s, r[0] + (r[2] - tw(t)) / 2.0, r[1] + (r[3] - 8.0) / 2.0, t, c);
}

/// A nine-slice panel of the pack (border 4) over `r` in GUI units.
fn nine(v: &mut Vec<HudVertex>, s: f32, theme: usize, kind: usize, r: [f32; 4]) {
    let (sx, sy, ..) = sp::panel(theme, kind);
    let (x, y, w, h) = (r[0], r[1], r[2].max(8.0), r[3].max(8.0));
    let cols = [(0.0, x, 4.0), (4.0, x + 4.0, w - 8.0), (8.0, x + w - 4.0, 4.0)];
    let rows = [(0.0, y, 4.0), (4.0, y + 4.0, h - 8.0), (8.0, y + h - 4.0, 4.0)];
    for &(cy, dy, dh) in &rows {
        for &(cx, dx, dw) in &cols {
            HudPipeline::push_gui(v, dx * s, dy * s, dw * s, dh * s, (sx + cx, sy + cy, 4.0, 4.0));
        }
    }
}

fn button(v: &mut Vec<HudVertex>, s: f32, theme: usize, r: [f32; 4], label: &str, lit: bool, enabled: bool) {
    nine(v, s, theme, if lit { sp::LIGHTER } else { sp::DEFAULT }, r);
    centred(v, s, r, label, if enabled { WHITE } else { GREY });
}

fn icon(v: &mut Vec<HudVertex>, s: f32, x: f32, y: f32, st: ItemStack) {
    crate::push_stack(v, (x * s, y * s, 16.0 * s, 16.0 * s), 1.0, st);
}

/// The panel's layout, GUI units.
struct Lay {
    close: [f32; 4],
    search: [f32; 4],
    mode: [f32; 4],
    prev: [f32; 4],
    next: [f32; 4],
    label: [f32; 4],
    gx: f32,
    gy: f32,
    cols: usize,
    rows: usize,
}

fn lay(gw: f32, gh: f32) -> Lay {
    let cols = (((gw - 24.0) / CELL).floor() as usize).max(1);
    let rows = (((gh - 76.0) / CELL).floor() as usize).max(1);
    let gx = ((gw - cols as f32 * CELL) / 2.0).floor();
    Lay {
        close: [10.0, 10.0, 40.0, 20.0],
        search: [54.0, 10.0, gw - 54.0 - 100.0, 20.0],
        mode: [gw - 92.0, 10.0, 82.0, 20.0],
        prev: [10.0, gh - 30.0, 40.0, 20.0],
        next: [gw - 50.0, gh - 30.0, 40.0, 20.0],
        label: [54.0, gh - 30.0, gw - 108.0, 20.0],
        gx,
        gy: 36.0,
        cols,
        rows,
    }
}

impl ReiUi {
    /// `cheat`: opened from Tools (gives) or from the inventory (views).
    pub fn new(cheat: bool) -> Self {
        Self { cheat, query: String::new(), typing: false, list: catalog().to_vec(), page: 0, view: None, hist: Vec::new(), press: None, finger: None }
    }

    // ---- the search field (the phone's soft keyboard, see `rei_key` in lib.rs) ----
    pub fn typing(&self) -> bool {
        self.typing
    }
    pub fn end_typing(&mut self) {
        self.typing = false;
    }
    pub fn type_char(&mut self, c: char) {
        if !c.is_control() && self.query.chars().count() < 24 {
            self.query.push(c);
            self.refilter();
        }
    }
    pub fn backspace(&mut self) {
        self.query.pop();
        self.refilter();
    }

    /// `EntryListSearchManager`: the entries whose name holds the query, any case.
    fn refilter(&mut self) {
        let q = self.query.to_lowercase();
        self.list = catalog().iter().copied().filter(|k| q.is_empty() || k.name().to_lowercase().contains(&q)).collect();
        self.page = 0;
    }

    fn per_page(l: &Lay) -> usize {
        l.cols * l.rows
    }

    fn pages(&self, l: &Lay) -> usize {
        self.list.len().div_ceil(Self::per_page(l)).max(1)
    }

    fn entry_at(&self, l: &Lay, x: f32, y: f32) -> Option<Key> {
        let (c, r) = (((x - l.gx) / CELL).floor(), ((y - l.gy) / CELL).floor());
        if c < 0.0 || r < 0.0 || c >= l.cols as f32 || r >= l.rows as f32 {
            return None;
        }
        self.list.get(self.page * Self::per_page(l) + r as usize * l.cols + c as usize).copied()
    }

    // ---- the recipe view's geometry ----
    /// (panel, workstation, tabs, previous page, next page, page label, first display top, recipes per page).
    fn vlay(gw: f32, gh: f32, tabs: usize) -> ([f32; 4], [f32; 4], Vec<[f32; 4]>, [f32; 4], [f32; 4], [f32; 4], f32, usize) {
        let cx = (gw / 2.0).floor();
        let panel = [cx - 95.0, 34.0, 190.0, gh - 42.0];
        let tab = (0..tabs).map(|i| [panel[0] + 6.0 + 26.0 * i as f32, panel[1] + 4.0, 24.0, 24.0]).collect();
        let ny = panel[1] + 32.0;
        let (prev, next) = ([panel[0] + 5.0, ny, 28.0, 18.0], [panel[0] + 190.0 - 33.0, ny, 28.0, 18.0]);
        let label = [panel[0] + 35.0, ny, 120.0, 18.0];
        let top = ny + 22.0;
        let per = (((panel[1] + panel[3] - 6.0 - top) / 70.0).floor() as usize).max(1);
        (panel, [panel[0] - 28.0, panel[1] + 16.0, 24.0, 24.0], tab, prev, next, label, top, per)
    }

    fn header(gw: f32) -> ([f32; 4], [f32; 4], [f32; 4]) {
        ([10.0, 8.0, 40.0, 20.0], [gw - 92.0, 8.0, 40.0, 20.0], [gw - 50.0, 8.0, 40.0, 20.0])
    }

    /// The slots of a display drawn with its top-left at (x, y) of a 150 x 66 base: (rect, item, drawn as the result).
    fn slots(d: &Disp, x: f32, y: f32) -> Vec<([f32; 4], Key, bool)> {
        let mut v = Vec::new();
        match d {
            Disp::Craft { w, cells, out } => {
                let (sx, sy) = (x + 17.0, y + 6.0); // `DefaultCraftingCategory`: centre - 58, centre - 27
                let w = if *w == 0 { 3 } else { *w };
                for (i, &c) in cells.iter().enumerate() {
                    if c != 0 {
                        v.push(([sx + 1.0 + (i % w) as f32 * 18.0, sy + 1.0 + (i / w) as f32 * 18.0, 18.0, 18.0], decode(c), false));
                    }
                }
                v.push(([sx + 95.0, sy + 19.0, 26.0, 26.0], Key { id: out.id, dmg: out.damage }, true));
            }
            Disp::Smelt { inp, out } => {
                v.push(([x + 23.0, y + 24.0, 18.0, 18.0], *inp, false));
                v.push(([x + 101.0, y + 20.0, 26.0, 26.0], Key { id: out.id, dmg: out.damage }, true));
            }
            Disp::Fuel { item, .. } => v.push(([x + 25.0, y + 24.0, 18.0, 18.0], *item, false)),
        }
        v
    }

    fn open(&mut self, k: Key, uses: bool) {
        if let Some(cur) = self.view.take() {
            self.hist.push(cur);
        }
        self.view = Some(View::new(k, uses));
    }

    /// A finger event in px. `Some(Out)` asks the game to close or to give an item.
    pub fn event(&mut self, ev: ScreenEv, w: f32, h: f32, st: &Settings) -> Option<Out> {
        let (s, gw, gh) = scaled_gs(w, h, st.gui_scale);
        match ev {
            ScreenEv::Down(x, y) => {
                self.press = Some(Press { at: (x / s, y / s), t: Instant::now(), drag: false });
                self.finger = Some((x / s, y / s));
                None
            }
            ScreenEv::Move(x, y) => {
                let p = (x / s, y / s);
                self.finger = Some(p);
                if let Some(pr) = self.press.as_mut() {
                    pr.drag |= (p.0 - pr.at.0).abs().max((p.1 - pr.at.1).abs()) > 6.0;
                }
                None
            }
            ScreenEv::Up(x, y) => {
                self.finger = None;
                let pr = self.press.take()?;
                let (gx, gy) = (x / s, y / s);
                if pr.drag {
                    let d = gx - pr.at.0;
                    if d.abs() > 40.0 && d.abs() > (gy - pr.at.1).abs() {
                        self.turn(if d < 0.0 { 1 } else { -1 }, gw, gh);
                    }
                    return None;
                }
                self.tap(gx, gy, pr.t.elapsed().as_secs_f32() >= LONG_SECS, gw, gh)
            }
        }
    }

    /// Page `d` steps (wrapping, like REI's arrows).
    fn turn(&mut self, d: i32, gw: f32, gh: f32) {
        let l = lay(gw, gh);
        match self.view.as_mut() {
            None => {
                let n = self.pages(&l) as i32;
                self.page = (self.page as i32 + d).rem_euclid(n) as usize;
            }
            Some(v) => {
                let per = Self::vlay(gw, gh, 0).7;
                let n = v.disp[v.cat].len().div_ceil(per).max(1) as i32;
                v.page = (v.page as i32 + d).rem_euclid(n) as usize;
            }
        }
    }

    fn tap(&mut self, x: f32, y: f32, long: bool, gw: f32, gh: f32) -> Option<Out> {
        if self.view.is_some() {
            return self.tap_view(x, y, long, gw, gh);
        }
        let l = lay(gw, gh);
        if inside(l.search, x, y) {
            self.typing = true;
            return None;
        }
        self.typing = false;
        if inside(l.close, x, y) {
            return Some(Out::Close);
        }
        if inside(l.mode, x, y) {
            self.cheat = !self.cheat;
        } else if inside(l.prev, x, y) {
            self.turn(-1, gw, gh);
        } else if inside(l.next, x, y) {
            self.turn(1, gw, gh);
        } else if let Some(k) = self.entry_at(&l, x, y) {
            if self.cheat {
                return Some(Out::Give(k.stack(if long { items::max_stack(k.id) } else { 1 })));
            }
            self.open(k, long);
        }
        None
    }

    fn tap_view(&mut self, x: f32, y: f32, long: bool, gw: f32, gh: f32) -> Option<Out> {
        let cheat = self.cheat;
        let (back, one, stack) = Self::header(gw);
        let v = self.view.as_mut()?;
        if inside(back, x, y) {
            self.view = self.hist.pop();
            return None;
        }
        if cheat && inside(one, x, y) {
            return Some(Out::Give(v.target.stack(1)));
        }
        if cheat && inside(stack, x, y) {
            return Some(Out::Give(v.target.stack(items::max_stack(v.target.id))));
        }
        let cats = v.cats();
        let (_, _, tabs, prev, next, _, top, per) = Self::vlay(gw, gh, cats.len());
        if let Some(i) = tabs.iter().position(|&r| inside(r, x, y)) {
            (v.cat, v.page) = (cats[i], 0);
        } else if inside(prev, x, y) {
            self.turn(-1, gw, gh);
        } else if inside(next, x, y) {
            self.turn(1, gw, gh);
        } else {
            let cx = (gw / 2.0).floor();
            let first = v.page * per;
            for (i, d) in v.disp[v.cat].iter().skip(first).take(per).enumerate() {
                for (r, k, _) in Self::slots(d, cx - 75.0, top + i as f32 * 70.0) {
                    if inside(r, x, y) {
                        let k = k;
                        self.open(k, long);
                        return None;
                    }
                }
            }
        }
        None
    }

    /// What the screen fills, in px, for the pad's button hints to keep off.
    pub fn avoid(&self, w: f32, h: f32, st: &Settings) -> Vec<[f32; 4]> {
        let (s, gw, gh) = scaled_gs(w, h, st.gui_scale);
        vec![[0.0, 0.0, gw * s, gh * s]]
    }

    pub fn draw(&self, v: &mut Vec<HudVertex>, w: f32, h: f32, st: &Settings) {
        let (s, gw, gh) = scaled_gs(w, h, st.gui_scale);
        let th = (st.rei_theme as usize).min(1);
        quad(v, 1.0, [0.0, 0.0, w, h], [0.0, 0.0, 0.0, 0.7]);
        match &self.view {
            None => self.draw_list(v, s, gw, gh, th),
            Some(view) => self.draw_view(v, s, gw, gh, th, view),
        }
    }

    fn tooltip(&self, v: &mut Vec<HudVertex>, s: f32, gw: f32, k: Key) {
        let Some((fx, fy)) = self.finger else { return };
        let t = k.name();
        let (bw, bh) = (tw(&t) + 8.0, 14.0);
        let (x, y) = ((fx - bw / 2.0).clamp(2.0, gw - bw - 2.0), (fy - 44.0).max(2.0));
        quad(v, s, [x, y, bw, bh], [0.06, 0.0, 0.1, 0.92]);
        quad(v, s, [x, y, bw, 1.0], [0.3, 0.1, 0.6, 1.0]);
        text(v, s, x + 4.0, y + 3.0, &t, WHITE);
    }

    fn draw_list(&self, v: &mut Vec<HudVertex>, s: f32, gw: f32, gh: f32, th: usize) {
        let l = lay(gw, gh);
        nine(v, s, th, sp::DEFAULT, [4.0, 4.0, gw - 8.0, gh - 8.0]);
        button(v, s, th, l.close, "Close", false, true);
        nine(v, s, th, sp::SLOT, l.search);
        let q = if self.query.is_empty() && !self.typing { "Search...".to_string() } else { format!("{}{}", self.query, if self.typing { "_" } else { "" }) };
        let fit = crate::ui::fit(&q, l.search[2] - 10.0);
        text(v, s, l.search[0] + 5.0, l.search[1] + 6.0, &fit, if self.query.is_empty() && !self.typing { GREY } else { WHITE });
        button(v, s, th, l.mode, if self.cheat { "Mode: Give" } else { "Mode: View" }, self.cheat, true);
        let pages = self.pages(&l);
        button(v, s, th, l.prev, "<", false, pages > 1);
        button(v, s, th, l.next, ">", false, pages > 1);
        centred(v, s, l.label, &format!("{}/{}  ({} items)", self.page + 1, pages, self.list.len()), WHITE);
        let per = Self::per_page(&l);
        let mut under = None;
        for i in 0..per {
            let Some(&k) = self.list.get(self.page * per + i) else { break };
            let (x, y) = (l.gx + (i % l.cols) as f32 * CELL, l.gy + (i / l.cols) as f32 * CELL);
            nine(v, s, th, sp::SLOT, [x + 1.0, y + 1.0, 18.0, 18.0]);
            icon(v, s, x + 2.0, y + 2.0, k.stack(1));
            if self.finger.is_some_and(|(fx, fy)| inside([x, y, CELL, CELL], fx, fy)) {
                quad(v, s, [x + 2.0, y + 2.0, 16.0, 16.0], [1.0, 1.0, 1.0, 0.35]);
                under = Some(k);
            }
        }
        if self.list.is_empty() {
            centred(v, s, [0.0, gh / 2.0 - 4.0, gw, 8.0], "No items match", GREY);
        }
        if let Some(k) = under {
            self.tooltip(v, s, gw, k);
        }
    }

    fn draw_view(&self, v: &mut Vec<HudVertex>, s: f32, gw: f32, gh: f32, th: usize, view: &View) {
        let cx = (gw / 2.0).floor();
        let (back, one, stack) = Self::header(gw);
        button(v, s, th, back, "Back", false, true);
        let title = format!("{}: {}", if view.uses { "Uses" } else { "Recipes" }, view.target.name());
        icon(v, s, cx - (tw(&title) + 20.0) / 2.0, 10.0, view.target.stack(1));
        text(v, s, cx - (tw(&title) + 20.0) / 2.0 + 20.0, 12.0, &title, WHITE);
        if self.cheat {
            button(v, s, th, one, "+1", false, true);
            button(v, s, th, stack, "Stack", false, true);
        }
        let cats = view.cats();
        let (panel, work, tabs, prev, next, label, top, per) = Self::vlay(gw, gh, cats.len());
        nine(v, s, th, sp::DEFAULT, panel);
        if cats.is_empty() {
            centred(v, s, [panel[0], panel[1] + 40.0, panel[2], 8.0], "No recipes", GREY);
            return;
        }
        for (r, &c) in tabs.iter().zip(&cats) {
            nine(v, s, th, if c == view.cat { sp::LIGHTER } else { sp::DEFAULT }, *r);
            icon(v, s, r[0] + 4.0, r[1] + 4.0, ItemStack { id: TAB[c], count: 1, damage: 0 });
        }
        text(v, s, panel[0] + 6.0 + 26.0 * cats.len() as f32 + 4.0, panel[1] + 12.0, CAT[view.cat], WHITE);
        // `categoryBack` / `recipeBack`: the page row, and the workstation panel at the left.
        let n = view.disp[view.cat].len();
        let pages = n.div_ceil(per).max(1);
        button(v, s, th, prev, "<", false, pages > 1);
        button(v, s, th, next, ">", false, pages > 1);
        centred(v, s, label, &format!("{}/{}", view.page + 1, pages), WHITE);
        nine(v, s, th, sp::DEFAULT, work);
        icon(v, s, work[0] + 4.0, work[1] + 4.0, ItemStack { id: WORK[view.cat], count: 1, damage: 0 });
        let mut under = None;
        for (i, d) in view.disp[view.cat].iter().skip(view.page * per).take(per).enumerate() {
            let (x, y) = (cx - 75.0, top + i as f32 * 70.0);
            nine(v, s, th, sp::LIGHTER, [x, y, 150.0, 66.0]);
            if matches!(d, Disp::Craft { .. } | Disp::Smelt { .. }) {
                let (ax, ay) = if matches!(d, Disp::Craft { .. }) { (x + 17.0 + 60.0, y + 6.0 + 18.0) } else { (x + 63.0, y + 25.0) };
                HudPipeline::push_gui(v, ax * s, ay * s, 22.0 * s, 16.0 * s, crate::render::hud::sprites::rei::arrow(th));
            }
            if let Disp::Fuel { ticks, .. } = d {
                text(v, s, x + 50.0, y + 29.0, &format!("Burns {} items", *ticks as f32 / 200.0), WHITE);
            }
            for (r, k, big) in Self::slots(d, x, y) {
                nine(v, s, th, sp::SLOT, r);
                let count = if let (true, Disp::Craft { out, .. } | Disp::Smelt { out, .. }) = (big, d) { out.count } else { 1 };
                icon(v, s, r[0] + (r[2] - 16.0) / 2.0, r[1] + (r[3] - 16.0) / 2.0, k.stack(count));
                if self.finger.is_some_and(|(fx, fy)| inside(r, fx, fy)) {
                    quad(v, s, [r[0] + 1.0, r[1] + 1.0, r[2] - 2.0, r[3] - 2.0], [1.0, 1.0, 1.0, 0.3]);
                    under = Some(k);
                }
            }
        }
        if let Some(k) = under {
            self.tooltip(v, s, gw, k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_has_no_liquids_and_every_wool() {
        let c = catalog();
        assert!(c.iter().all(|k| !SKIP.contains(&k.id)));
        assert_eq!(c.iter().filter(|k| k.id == 35).count(), 16);
        assert!(c.contains(&Key { id: 58, dmg: 0 }));
    }

    #[test]
    fn a_workbench_has_a_recipe_and_planks_are_used() {
        let r = displays(Key { id: 58, dmg: 0 }, false);
        assert!(!r[0].is_empty(), "crafting table is crafted");
        let u = displays(Key { id: 5, dmg: 0 }, true);
        assert!(!u[0].is_empty(), "planks are an ingredient");
        assert!(!displays(Key { id: 263, dmg: 0 }, true)[2].is_empty(), "coal is a fuel");
        assert!(!displays(Key { id: 265, dmg: 0 }, false)[1].is_empty(), "iron ingots are smelted");
    }

    #[test]
    fn search_filters_and_pages_wrap() {
        let mut ui = ReiUi::new(false);
        let all = ui.list.len();
        "stone".chars().for_each(|c| ui.type_char(c));
        assert!(ui.list.len() < all && !ui.list.is_empty());
        let l = lay(427.0, 240.0);
        assert!(ui.pages(&l) >= 1);
    }
}
