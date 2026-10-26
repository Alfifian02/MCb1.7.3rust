//! The satchel: the immersive pack's flat replacement (`world::pack`'s cloth stays behind `packCloth:true`). The camera never moves; the
//! inventory button lifts a leather bag in front of the view with a closed recipe book lying beside it; a tap on the book opens it.
//!
//! - One layout: the bag is a 9 x 3 grid with the quick belt and the armor row (big cells for a thumb), as leather with stitching, rivets,
//!   a pocket band behind each row and a strap round the belt; a slot's rim is tinted by its item's category (`pack::category`).
//! - The bag and the closed book lie side by side, centred together (the book: a leather cover, a clasp, `Tap to open`). A tap swings the
//!   cover away, both pages grow from the spine, and the bag glides to the left while the spine glides to its place, a `Close` tab above the spine shuts it. Open: the left page is the categories, then the recipes of one as a list; the right page is the crafting grid
//!   (2 x 2, or 3 x 3 at a workbench) with the card of the picked recipe under it (`world::book`).
//! - Everything here is geometry (`Frame`) and drawing (`draw`, `draw_pin`); the screen's state is `Satchel`, the rules are `world::book`
//!   and `world::craft::Screen`, and `lib.rs` only routes the finger.
//!
//! - Look (2026-10-26, UI overhaul): pixel art from `assets/satchel.rgba` (`tools/gen_satchel_ui.py`, `hud::sprites::sat`), 9-slices and tiles at one
//!   pixel size (`u` = the text's pixel): a grey bevelled stone panel with corner marks and a red stitched seam, a pocket / belt band behind each block of slots, vanilla inset slots that touch (the category rim shows only under a finger),
//!   a plain brown book with a red-stitched spine, aged parchment with a red-stitched binding, grey bevelled plates for buttons and tags. Text is flat (no shadow), whole-pixel sized and fitted to its box (`fitted`, `text_fit`, `text_in`, `paragraph`).
//!
//! UNVERIFIED: written without a compiler or a device. The proportions are in fractions of the surface height (`h`), the text unit is
//! `craft::panel(0, w, h).2`; both are guesses to tune on the phone.
//! ponytail: the world behind is dimmed, not blurred (a blur is a post pass); a page is ~200 tiled quads (a wrapping sampler would be one); an unknown recipe is a dark box with a `?`, not a true
//! silhouette of the sprite (`push_stack` has no tint); partly visible list rows are not drawn instead of being clipped.

use crate::render::hud::{HudPipeline, HudVertex};
use crate::world::book::{self, Src, CATS};
use crate::world::craft::{self, Screen, SlotId};
use crate::world::items::{self, Inventory, ItemStack, MAIN};
use crate::world::pack;

/// Seconds the bag takes to come up, the book to open, a list to slide in; a press on the result this long crafts all.
pub const RISE_SECS: f32 = 0.25;
pub const BOOK_SECS: f32 = 0.3;
pub const PAGE_SECS: f32 = 0.2;
pub const HOLD_SECS: f32 = 0.5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Btn {
    Close,
    Book,
    Sort,
    Shift,
    One,
}

/// What a finger is on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pick {
    Slot(SlotId),
    Btn(Btn),
    /// A category button of the left page (index into `book::CATS`), the back arrow over a list.
    Cat(usize),
    Back,
    /// A recipe of the list (index into `book::entries()`), the empty part of the list (a drag there scrolls).
    Row(usize),
    List,
    /// The pin button of the right page's header and the discovery toggle.
    Pin,
    Hide,
    /// The recipe card under the grid: a drag there scrolls its ingredients.
    Card,
    /// Leather or paper with nothing on it: a tap does nothing, and does not throw the held stack.
    Panel,
    /// Past every panel: a held stack is thrown.
    Outside,
}

pub struct Satchel {
    pub age: f32,
    /// The book is (being) opened / closed, and how far it is, 0 to 1.
    pub book: bool,
    pub book_t: f32,
    /// The category page of the left page (`book::CATS` index), the pixels the list is scrolled, the slide-in of a page, the picked recipe.
    pub cat: Option<usize>,
    pub scroll: f32,
    pub page_t: f32,
    pub sel: Option<usize>,
    /// The pixels the recipe card's ingredient rows are scrolled (drawn snapped to a whole row).
    pub card_scroll: f32,
}

impl Satchel {
    pub fn new(book: bool) -> Self {
        Self { age: 0.0, book, book_t: 0.0, cat: None, scroll: 0.0, page_t: 1.0, sel: None, card_scroll: 0.0 }
    }

    pub fn tick(&mut self, dt: f32) {
        self.age += dt;
        let to = if self.book { 1.0 } else { 0.0 };
        self.book_t += (to - self.book_t).clamp(-dt / BOOK_SECS, dt / BOOK_SECS);
        self.page_t = (self.page_t + dt / PAGE_SECS).min(1.0);
    }

    /// A page of the left page turns: the list starts at its top and slides in.
    pub fn turn(&mut self, cat: Option<usize>) {
        (self.cat, self.scroll, self.page_t) = (cat, 0.0, 0.0);
    }

    /// The book is fully open: its contents answer the finger.
    pub fn open(&self) -> bool {
        self.book && self.book_t >= 0.98
    }
}

/// Where the open book spans, in fractions of the width; its spine is the middle.
const BOOK_L: f32 = 0.485;
const BOOK_R: f32 = 0.985;

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
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

/// Flat text (no shadow) with its left edge at `x` and its top at `y` (for size `k`), fitted into `maxw`; a shrunk text stays centred on
/// the line it would have had.
fn text_fit(v: &mut Vec<HudVertex>, at: (f32, f32), k: f32, maxw: f32, s: &str, col: [f32; 4]) {
    text_fit_o(v, at, k, maxw, s, col, 0.0);
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

/// The whole book, as rects in px.
#[derive(Default)]
pub struct BookFrame {
    /// The cover while closed, the pages while open (both grow from the spine).
    pub rect: [f32; 4],
    pub open: bool,
    /// The `Close` tab above the spine (hit while the book is open).
    pub tab: [f32; 4],
    pub left: [f32; 4],
    pub right: [f32; 4],
    pub cats: Vec<(usize, [f32; 4])>,
    pub back: Option<[f32; 4]>,
    pub list: Option<[f32; 4]>,
    pub rows: Vec<(usize, [f32; 4])>,
    pub row_h: f32,
    pub max_scroll: f32,
    pub grid: Vec<(SlotId, [f32; 4])>,
    pub arrow: [f32; 4],
    pub card: [f32; 4],
    pub pin: [f32; 4],
    pub hide: [f32; 4],
    /// How far the card's ingredient rows can scroll (px).
    pub card_max: f32,
    /// The clearance of a page's content from its rim, and the height of a page's header (set once the book is open).
    pub p: f32,
    pub hh: f32,
}

pub struct Frame {
    /// The text unit (px per GUI unit).
    pub k: f32,
    pub bag: [f32; 4],
    pub cells: Vec<(SlotId, [f32; 4])>,
    /// The pocket band behind each row of cells (the armor row, three of the bag, the belt), and whether it is the belt's.
    pub rows: Vec<([f32; 4], bool)>,
    pub btns: Vec<(Btn, [f32; 4])>,
    pub book: Option<BookFrame>,
}

impl Frame {
    /// Every rect of the screen for `w` x `h` px, this state of the satchel and a crafting grid `gw` wide (the layout is fixed, every slot
    /// has its cell: the bag's contents only order the book's list).
    pub fn new(sat: &Satchel, inv: &Inventory, gw: usize, w: f32, h: f32) -> Self {
        Self::with_grid(sat, inv, &[None; 9], gw, w, h)
    }

    /// `new` for a screen whose crafting grid holds `grid`: the book's list puts the recipes the bag and the grid can make first, so the
    /// rows (and what a finger finds on them) depend on both.
    pub fn with_grid(sat: &Satchel, inv: &Inventory, grid: &[Option<ItemStack>; 9], gw: usize, w: f32, h: f32) -> Self {
        let k = craft::panel(0, w, h).2;
        let tk = units(k).0;
        let u = tk; // one sprite pixel
        // The panel's border is 8 sprite pixels and its seam and corner marks lie inside 7 of them: the slots start clear of both.
        let pad = (0.03 * h).max(11.0 * u).round();
        let rise = (1.0 - ease(sat.age / RISE_SECS)) * 0.1 * h;
        let (top, bh) = (0.17 * h + rise, 0.74 * h);
        let mut f = Frame { k, bag: [0.0; 4], cells: Vec::new(), rows: Vec::new(), btns: Vec::new(), book: None };

        // The bag: 9 columns, the armor row on top, the belt at the foot. With the book closed the bag and the cover lie side by side, centred
        // together; as the book opens the bag glides to the left 45% of the screen and the book's spine to the middle of the right 55%.
        // Slots touch, like Minecraft's: a cell is one slot sprite and the next starts where its border ends. A pocket band lies behind each
        // block of them (the armor row, the bag's three rows, the belt), two sprite pixels wider than it, clear of the seam.
        let gv = 12.0 * u; // the gap between those blocks: the armor, the bag and the belt read as three separate things
        let cell = ((0.455 * w - 2.0 * pad) / 9.0).min((bh - 2.0 * pad - 2.0 * gv) / 5.0).floor().max(1.0);
        let (pw, ph) = (9.0 * cell + 2.0 * pad, 5.0 * cell + 2.0 * gv + 2.0 * pad);
        let (open_x, gap, half) = (0.012 * w, 0.02 * w, (BOOK_R - BOOK_L) * 0.5 * w); // half = a page of the open book
        let closed_x = ((w - (pw + gap + half)) * 0.5).max(open_x);
        let e = ease(sat.book_t);
        let x = lerp(closed_x, open_x, e).round();
        let spine = lerp(closed_x + pw + gap + half * 0.5, (BOOK_L + BOOK_R) * 0.5 * w, e);
        let top = top.round();
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

        // The buttons above the bag, and the close button at the screen's corner.
        let (by, bh2) = (0.035 * h + rise, 0.095 * h);
        let mut x = f.bag[0];
        for (b, label) in [(Btn::Sort, "Sort"), (Btn::Shift, "Shift"), (Btn::One, "One")] {
            let bw = HudPipeline::text_width(label) as f32 * tk + 6.0 * tk;
            f.btns.push((b, [x, by, bw, bh2]));
            x += bw + 0.012 * h;
        }
        let cw = HudPipeline::text_width("X") as f32 * tk + 8.0 * tk;
        f.btns.push((Btn::Close, [w - 0.02 * w - cw, by, cw, bh2]));

        f.book = Some(book_frame(sat, (gw, inv, grid), (w, h), (top, bh), spine));
        f
    }

    /// What is under `(x, y)`.
    pub fn hit(&self, sat: &Satchel, x: f32, y: f32) -> Pick {
        if let Some((b, _)) = self.btns.iter().find(|(_, r)| inside(*r, x, y)) {
            return Pick::Btn(*b);
        }
        if let Some(b) = &self.book {
            // A closed (or closing) book opens at a tap; an open one closes at its tab.
            if (!sat.book && inside(b.rect, x, y)) || (sat.open() && inside(b.tab, x, y)) {
                return Pick::Btn(Btn::Book);
            }
            if b.open && sat.open() {
                let hit = b
                    .cats
                    .iter()
                    .find(|(_, r)| inside(*r, x, y))
                    .map(|&(i, _)| Pick::Cat(i))
                    .or_else(|| b.back.filter(|r| inside(*r, x, y)).map(|_| Pick::Back))
                    .or_else(|| b.rows.iter().find(|(_, r)| inside(*r, x, y)).map(|&(i, _)| Pick::Row(i)))
                    .or_else(|| (sat.sel.is_some() && inside(b.pin, x, y)).then_some(Pick::Pin))
                    .or_else(|| inside(b.hide, x, y).then_some(Pick::Hide))
                    .or_else(|| b.grid.iter().find(|(_, r)| inside(*r, x, y)).map(|&(id, _)| Pick::Slot(id)))
                    .or_else(|| b.list.filter(|r| inside(*r, x, y)).map(|_| Pick::List))
                    .or_else(|| inside(b.card, x, y).then_some(Pick::Card));
                if let Some(h) = hit {
                    return h;
                }
            }
            if inside(b.rect, x, y) {
                return Pick::Panel;
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
}

impl Frame {
    /// Every rect a pad can rest on, the ones `hit` answers with something other than `Panel` / `Outside`: the buttons, the closed book
    /// (a tap opens it) or an open one's tab, categories, back arrow, recipes, pin, toggle and grid, and every slot of the bag.
    pub fn targets(&self, sat: &Satchel) -> Vec<[f32; 4]> {
        let mut t: Vec<[f32; 4]> = self.btns.iter().map(|&(_, r)| r).collect();
        if let Some(b) = &self.book {
            if !sat.book {
                t.push(b.rect);
            }
            if sat.open() && b.open {
                t.push(b.tab);
                t.extend(b.cats.iter().map(|&(_, r)| r));
                t.extend(b.back);
                t.extend(b.rows.iter().map(|&(_, r)| r));
                if sat.sel.is_some() {
                    t.push(b.pin);
                }
                t.push(b.hide);
                t.extend(b.grid.iter().map(|&(_, r)| r));
            }
        }
        t.extend(self.cells.iter().map(|&(_, r)| r));
        t
    }
}

/// The target the cursor `at` rests on.
pub fn under(targets: &[[f32; 4]], at: (f32, f32)) -> Option<[f32; 4]> {
    targets.iter().copied().find(|r| inside(*r, at.0, at.1))
}

/// The pad's step: the rect to jump to from the one under `at` when the d-pad says `d` (x, y; y down). Off every rect (the cursor
/// was left where a page turned or the book glided), the nearest one. Among the rects that lie that way, within 63 degrees of it, the
/// one with the least `along + 2 * across` (centre to centre) wins: touching slots step one by one, a button row above the bag is a
/// step up from its columns, the book is a step right from the bag's last column. `None` when nothing lies that way.
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

/// The book's rects: closed, a cover one page wide; opening, both pages grow from the spine; the contents exist once it is open.
fn book_frame(sat: &Satchel, (gw, inv, grid): (usize, &Inventory, &[Option<ItemStack>; 9]), (w, h): (f32, f32), (top, bh): (f32, f32), spine: f32) -> BookFrame {
    let (bx0, bx1) = (BOOK_L * w, BOOK_R * w);
    let (spine_open, half) = ((bx0 + bx1) * 0.5, (bx1 - bx0) * 0.5); // `spine` is where it is now, on its way there
    let e = ease(sat.book_t);
    // Closed it is one page wide, centred on the spine; opening, it grows to both pages (and the spine slides to its place).
    let tab = [spine - 0.05 * w, top - 0.07 * h, 0.1 * w, 0.07 * h];
    let mut b = BookFrame { rect: [spine - half * (1.0 + e) * 0.5, top, half * (1.0 + e), bh], open: sat.book_t >= 0.98, tab, ..Default::default() };
    let gap = 0.003 * w;
    b.left = [bx0, top, half - gap, bh];
    b.right = [spine_open + gap, top, half - gap, bh];
    if !b.open {
        return b;
    }
    let (u, sk) = units(craft::panel(0, w, h).2);
    // `p`: clear of the page's rim (6 sprite pixels thick) by 4 more; the header's content lies in `p ..= hh - 6u`, above its rule.
    let (p, hh) = ((0.04 * h).max(10.0 * u).round(), 0.11 * h);
    let zh = hh - p - 6.0 * u;
    (b.p, b.hh) = (p, hh);
    let slide = (1.0 - ease(sat.page_t)) * 0.05 * w;
    let l = b.left;
    match sat.cat {
        None => {
            let cg = 0.012 * h;
            let ch = (l[3] - hh - 1.5 * p - 5.0 * cg) / 6.0;
            for i in 0..CATS.len() {
                b.cats.push((i, [l[0] + p + slide, l[1] + hh + 0.5 * p + i as f32 * (ch + cg), l[2] - 2.0 * p, ch]));
            }
        }
        Some(ci) => {
            b.back = Some([l[0] + p, l[1] + p, 0.14 * l[2], zh]);
            let list = [l[0] + p + slide, l[1] + hh + 0.5 * p, l[2] - 2.0 * p, l[3] - hh - 1.5 * p];
            b.list = Some(list);
            b.row_h = 0.1 * h;
            let ids = book::in_cat_ready(CATS[ci.min(CATS.len() - 1)], gw, inv, grid); // what can be made now first
            b.max_scroll = (ids.len() as f32 * b.row_h - list[3]).max(0.0);
            for (n, &ei) in ids.iter().enumerate() {
                let y = list[1] + n as f32 * b.row_h - sat.scroll;
                if y >= list[1] - 0.5 && y + b.row_h <= list[1] + list[3] + 0.5 {
                    b.rows.push((ei, [list[0], y, list[2], b.row_h - 0.008 * h]));
                }
            }
        }
    }
    // Right page: the grid (2 x 2 or 3 x 3), the arrow and the result; the card of the picked recipe under them.
    let (r, gw) = (b.right, gw.clamp(2, 3));
    // The cells touch (no gap). A cell is the smaller of the height's share and what the page's width leaves for the grid, the arrow and
    // the result (1.2 cells): a narrow screen shrinks the grid instead of pushing the result off the page.
    let (gx, gy) = ((r[0] + p + 0.01 * w).round(), (r[1] + hh + 3.0 * u).round());
    let (ga, aw) = (0.004 * w, 0.03 * w);
    let room = r[0] + r[2] - p - gx - 2.0 * ga - aw;
    let c = (if gw == 2 { 0.11 * h } else { 0.075 * h }).min(room / (gw as f32 + 1.2)).floor().max(1.0);
    for i in 0..gw * gw {
        b.grid.push((SlotId::Grid(i), [gx + (i % gw) as f32 * c, gy + (i / gw) as f32 * c, c, c]));
    }
    let gh = gw as f32 * c;
    let ax = gx + gh + ga;
    b.arrow = [ax, gy + gh * 0.5 - 0.02 * h, aw, 0.04 * h];
    let rc = (c * 1.2).floor();
    b.grid.push((SlotId::Result, [(ax + aw + ga).floor(), (gy + (gh - rc) * 0.5).round(), rc, rc]));
    let cy = gy + gh + 0.02 * h;
    b.card = [r[0] + p, cy, r[2] - 2.0 * p, r[1] + r[3] - 0.18 * h - cy];
    b.card_max = sat.sel.and_then(|i| book::entries().get(i)).map_or(0.0, |e| card_m(b.card, (sk, u), e.need.len()).max);
    // The pin's hit box is a square down to the rule; its picture (10 sprite pixels) hangs from the top of it.
    b.pin = [r[0] + r[2] - p - (hh - p), r[1] + p, hh - p, hh - p];
    b.hide = [r[0] + p, r[1] + r[3] - 0.17 * h, r[2] - 2.0 * p, 0.07 * h]; // the hint under it ends above the page's rim
    b
}

// ---- Drawing ----
//
// Every panel is pixel art from `assets/satchel.rgba` (`tools/gen_satchel_ui.py`, `hud::sprites::sat`), drawn as 9-slices and tiles at one
// pixel size, `u` = 0.75 GUI units = the text's own pixel (`tk`), so a glyph, a stitch and a rivet are the same size. Slots, plates and
// buttons are the same sprites at every size. The only quads left are the dimmed world and the scroll bar.

use crate::render::hud::sprites::sat as art;

const WHITE: [f32; 4] = [1.0; 4];
const INK: [f32; 4] = [0.22, 0.15, 0.08, 1.0];
const CREAM: [f32; 4] = [0.96, 0.9, 0.72, 1.0];
const GOOD: [f32; 4] = [0.25, 0.6, 0.2, 1.0];
const BAD: [f32; 4] = [0.75, 0.2, 0.15, 1.0];
const WARN: [f32; 4] = [0.85, 0.5, 0.1, 1.0];
const MUTE: [f32; 4] = [0.55, 0.47, 0.34, 1.0];
const GOLD: [f32; 4] = [0.9, 0.76, 0.38, 1.0];
const SELECTED: [f32; 4] = [0.95, 0.85, 0.1, 1.0];
/// Ink on the brown cover.
const COVER_INK: [f32; 4] = [0.91, 0.85, 0.64, 1.0];

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

/// `spr` as a 9-slice (`b` sprite pixels of corner) over the rect `r`.
fn nine(v: &mut Vec<HudVertex>, r: [f32; 4], spr: crate::render::hud::sprites::Sprite, b: f32, u: f32) {
    HudPipeline::push_nine(v, (r[0], r[1], r[2], r[3]), spr, b, u, WHITE, true);
}

/// A slot cell: the inset slot, a rim in `rim` when it has one, the stack in it.
fn slot(v: &mut Vec<HudVertex>, r: [f32; 4], st: Option<ItemStack>, rim: Option<[f32; 4]>, u: f32) {
    nine(v, r, art::SLOT, 2.0, u);
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

/// A leather plate with its label: `state` 0 disabled, 1 normal, 2 on.
fn button(v: &mut Vec<HudVertex>, r: [f32; 4], (tk, u): (f32, f32), text: &str, state: u8) {
    let c = if state == 0 { [0.6, 0.6, 0.6, 1.0] } else { WHITE };
    HudPipeline::push_nine(v, (r[0], r[1], r[2], r[3]), if state == 2 { art::BTN_ON } else { art::BTN }, 4.0, u, c, true);
    text_in(v, r[0] + r[2] * 0.5, r[1] + (r[3] - 8.0 * tk) * 0.5, tk, (r[0] + 2.0 * u, r[0] + r[2] - 2.0 * u), text, [0.96, 0.96, 0.96, 1.0]);
}

/// A title's rule under a page's header: an ink line across the page.
fn rule(v: &mut Vec<HudVertex>, page: [f32; 4], hh: f32, p: f32, u: f32) {
    HudPipeline::push_tiled(v, (page[0] + p, page[1] + hh - 4.3 * u, page[2] - 2.0 * p, 4.0 * u), art::RULE, u, WHITE);
}

/// The whole screen: the dimmed world, the bag, the book, the stack on the finger and the name of what it touches.
pub fn draw(v: &mut Vec<HudVertex>, (w, h): (f32, f32), sat: &Satchel, c: &Ctx) {
    let f = Frame::with_grid(sat, c.inv, &c.screen.grid, c.screen.gw, w, h);
    let k = f.k;
    let (tk, sk) = units(k);
    let u = tk;
    HudPipeline::push_quad(v, 0.0, 0.0, w, h, [0.0, 0.0, 0.0, 0.5]);

    // The bag: leather with its seam and rivets; a pocket behind each row (the belt's is a strap with a buckle), the label tag sewn over
    // the armor pocket's lip, then every cell.
    nine(v, f.bag, art::BAG, 8.0, u);
    for &(r, belt) in &f.rows {
        nine(v, r, if belt { art::BELT } else { art::POCKET }, 4.0, u);
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

    // Buttons: the toggles show their number while on.
    for &(b, r) in &f.btns {
        let (text, state) = match b {
            Btn::Close => ("X", 1),
            Btn::Book => ("Book", if sat.book { 2 } else { 1 }),
            Btn::Sort => ("Sort", 1),
            Btn::Shift => (if c.shift { "2" } else { "Shift" }, if c.shift { 2 } else { 1 }),
            Btn::One => (if c.one { "1" } else { "One" }, if c.one { 2 } else { 1 }),
        };
        button(v, r, (tk, u), text, state);
    }

    if let Some(b) = &f.book {
        draw_book(v, (w, h), (sat, b), c, (tk, sk, u));
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
        let under = || match f.hit(sat, fx, fy) {
            Pick::Slot(id) => c.screen.get(c.inv, None, id),
            Pick::Row(i) => book::entries().get(i).map(|e| e.out).filter(|_| !c.discovery || c.notes.known(&book::entries()[i])),
            _ => None,
        };
        if let Some(name) = c.screen.cursor.or_else(under).and_then(items::name) {
            let bw = (HudPipeline::text_width(name) as f32 + 6.0) * tk;
            let ty = if fy > 36.0 * tk { fy - 34.0 * tk } else { fy + 14.0 * tk };
            HudPipeline::push_tooltip(v, (fx - bw * 0.5).clamp(0.0, (w - bw).max(0.0)), ty, tk, name);
        }
    }
}

fn draw_book(v: &mut Vec<HudVertex>, (w, h): (f32, f32), (sat, b): (&Satchel, &BookFrame), c: &Ctx, (tk, sk, u): (f32, f32, f32)) {
    // Under the cover that swings away: the back cover, thick leather round the pages (it grows with the opening, so it never shows
    // wider than the closed cover), the paper (one sheet while it opens, aged at its rim), and the red-stitched binding at the spine.
    if sat.book_t > 0.0 {
        let cm = 0.013 * w * ease(sat.book_t * 2.0);
        nine(v, [b.rect[0] - cm, b.rect[1], b.rect[2] + 2.0 * cm, b.rect[3]], art::COVER, 8.0, u); // wider than the pages, never taller
        let r = (b.rect[0], b.rect[1], b.rect[2], b.rect[3]);
        HudPipeline::push_tiled(v, r, art::PARCH, u, WHITE);
        HudPipeline::push_nine(v, r, art::PAGE_EDGE, 6.0, u, WHITE, false);
        if sat.book_t > 0.3 {
            HudPipeline::push_tiled(v, (r.0 + r.2 * 0.5 - 4.0 * u, r.1 + 6.0 * u, 8.0 * u, r.3 - 12.0 * u), art::BINDING, u, WHITE);
        }
    }
    if sat.book_t < 0.5 {
        draw_cover(v, [b.rect[0], b.rect[1], b.rect[2] * (1.0 - 2.0 * sat.book_t), b.rect[3]], sat, (tk, sk, u));
    }
    if !b.open {
        return;
    }
    if sat.book {
        button(v, b.tab, (tk, u), "Close", 1);
    }
    // The corner of the right page, turned up.
    HudPipeline::push_gui(v, b.right[0] + b.right[2] - 14.0 * u, b.right[1] + b.right[3] - 14.0 * u, 12.0 * u, 12.0 * u, art::CURL);
    let (p, hh) = (b.p, b.hh);
    let entries = book::entries();
    let gw = c.screen.gw;
    let title_y = |r: [f32; 4]| r[1] + p + (hh - p - 6.0 * u - 8.0 * tk) * 0.5; // centred between the page's margin and the rule
    rule(v, b.left, hh, p, u);
    rule(v, b.right, hh, p, u);

    // Left page: the categories, or the recipes of one.
    match sat.cat {
        None => {
            text_fit(v, (b.left[0] + p, title_y(b.left)), tk, b.left[2] - 2.0 * p, "Recipes", INK);
            for &(i, r) in &b.cats {
                let ids = book::in_cat(CATS[i]);
                nine(v, r, art::ENTRY, 3.0, u);
                if let Some(e) = ids.first().map(|&e| &entries[e]) {
                    crate::push_stack(v, (r[0] + 3.0 * u, r[1] + (r[3] - r[3] * 0.8) * 0.5, r[3] * 0.8, r[3] * 0.8), 0.875, ItemStack { count: 1, ..e.out });
                }
                // `made/all`: how many of the page the bag can make now (just the total while none).
                let made = ids.iter().filter(|&&e| book::ready(&entries[e], c.screen.gw, c.inv, &c.screen.grid)).count();
                let n = if made > 0 { format!("{made}/{}", ids.len()) } else { ids.len().to_string() };
                let nw = HudPipeline::text_width(&n) as f32 * tk;
                let (lx, ty) = (r[0] + r[3] + 4.0 * u, r[1] + (r[3] - 8.0 * tk) * 0.5);
                text_fit(v, (lx, ty), tk, r[0] + r[2] - 8.0 * u - nw - lx, CATS[i].label(), INK);
                HudPipeline::push_text_flat(v, r[0] + r[2] - nw - 5.0 * u, ty, tk, &n, INK);
            }
        }
        Some(ci) => {
            if let Some(r) = b.back {
                nine(v, r, art::ENTRY, 3.0, u);
                let (aw, ah) = (12.0 * u * 0.9, 9.0 * u * 0.9);
                HudPipeline::push_gui_tinted(v, r[0] + (r[2] - aw) * 0.5, r[1] + (r[3] - ah) * 0.5, aw, ah, art::ARROW, INK);
            }
            let tx = b.left[0] + p + 0.14 * b.left[2] + 8.0 * u;
            text_fit(v, (tx, title_y(b.left)), tk, b.left[0] + b.left[2] - p - tx, CATS[ci.min(CATS.len() - 1)].label(), INK);
            for &(ei, r) in &b.rows {
                let e = &entries[ei];
                let known = !c.discovery || c.notes.known(e);
                let ok = known && book::craftable(e, gw, c.inv, &c.screen.grid);
                let lit = known && book::ready(e, gw, c.inv, &c.screen.grid); // the list puts these first
                let sel = sat.sel == Some(ei);
                nine(v, r, if sel { art::ENTRY_SEL } else { art::ENTRY }, 3.0, u);
                let isz = r[3] * 0.82;
                let ib = (r[0] + 3.0 * u, r[1] + (r[3] - isz) * 0.5, isz, isz);
                if known {
                    crate::push_stack(v, ib, 0.875, ItemStack { count: 1, ..e.out });
                } else {
                    HudPipeline::push_nine(v, (ib.0, ib.1, ib.2, ib.3), art::SLOT, 2.0, u, WHITE, true);
                    text_in(v, ib.0 + ib.2 * 0.5, ib.1 + (ib.3 - 8.0 * tk) * 0.5, tk, (ib.0, ib.0 + ib.2), "?", [0.85, 0.85, 0.9, 1.0]);
                }
                let name = if known { items::name(e.out).unwrap_or("?") } else { "???" };
                let nx = ib.0 + isz + 6.0 * u;
                text_fit(v, (nx, r[1] + (r[3] - 8.0 * tk) * 0.5), tk, r[0] + r[2] - r[3] * 0.34 - 8.0 * u - nx, name, INK); // up to the gem
                // The gem at the right: green = the bag has it all and the grid fits; orange = a furnace's; red = something is short.
                let dot = if !known { MUTE } else if e.grid == 0 { WARN } else if ok { GOOD } else { BAD };
                let ds = r[3] * 0.34;
                HudPipeline::push_gui_tinted(v, r[0] + r[2] - ds - 5.0 * u, r[1] + (r[3] - ds) * 0.5, ds, ds, art::GEM, dot);
                if !lit {
                    HudPipeline::push_quad(v, r[0], r[1], r[2], r[3], [0.0, 0.0, 0.0, 0.55]); // not now: the row is dimmed, icon and name with it
                }
            }
            if b.max_scroll > 0.0 {
                // A thin scroll bar along the list's right edge.
                if let Some(l) = b.list {
                    let (bar, at) = (l[3] * l[3] / (l[3] + b.max_scroll), sat.scroll / b.max_scroll);
                    HudPipeline::push_quad(v, l[0] + l[2] - 0.6 * u, l[1] + (l[3] - bar) * at, 0.6 * u, bar, [0.3, 0.2, 0.1, 0.6]);
                }
            }
        }
    }

    // Right page: title, pin, the grid with its arrow and result, the card of the picked recipe.
    text_fit(v, (b.right[0] + p, title_y(b.right)), tk, b.pin[0] - 2.0 * u - b.right[0] - p, if gw == 3 { "Workbench" } else { "Crafting" }, INK);
    // The pin: a picture that turns red when this recipe is pinned (dim while none is picked).
    let pinned = sat.sel.is_some_and(|s| c.notes.pinned == Some(s));
    let a = if sat.sel.is_some() { 1.0 } else { 0.4 };
    HudPipeline::push_gui_tinted(v, b.pin[0] + (b.pin[2] - 10.0 * u) * 0.5, b.pin[1], 10.0 * u, 10.0 * u, if pinned { art::PIN_ON } else { art::PIN }, [1.0, 1.0, 1.0, a]);
    for &(id, r) in &b.grid {
        let st = c.screen.get(c.inv, None, id);
        let rim = (id == SlotId::Result && st.is_some()).then_some(GOOD);
        slot(v, r, st, rim, u);
    }
    text_in(v, b.arrow[0] + b.arrow[2] * 0.5, b.arrow[1] + (b.arrow[3] - 8.0 * tk) * 0.5, tk, (b.arrow[0], b.arrow[0] + b.arrow[2]), "->", INK);
    if c.hold {
        if let Some((_, r)) = b.grid.iter().find(|(id, _)| *id == SlotId::Result) {
            text_in(v, r[0] + r[2] * 0.5, r[1] - 10.0 * sk, sk, (b.right[0] + p, b.right[0] + b.right[2] - p), "Release: craft all", GOOD);
        }
    }
    draw_card(v, sat, b, c, (tk, sk, u));

    // The discovery toggle and the hint under it.
    nine(v, b.hide, art::ENTRY, 3.0, u);
    let label = if c.discovery { "Unknown recipes: hidden" } else { "Unknown recipes: shown" };
    text_in(v, b.hide[0] + b.hide[2] * 0.5, b.hide[1] + (b.hide[3] - 8.0 * sk) * 0.5, sk, (b.hide[0] + 2.0 * u, b.hide[0] + b.hide[2] - 2.0 * u), label, INK);
    text_in(v, b.hide[0] + b.hide[2] * 0.5, b.hide[1] + b.hide[3] + 3.0 * sk, sk, (b.right[0] + p, b.right[0] + b.right[2] - p), "Hold the result to craft all", [0.4, 0.3, 0.2, 1.0]);
}

/// The closed book: a plain brown cover with a red-stitched spine (the sprite's left border) and a sliver of page edge, the book icon and
/// its title; the writing only while the cover is flat (it is foreshortened, narrower, while it swings). No clasp, no strap.
fn draw_cover(v: &mut Vec<HudVertex>, r: [f32; 4], sat: &Satchel, (tk, sk, u): (f32, f32, f32)) {
    if r[2] < 8.0 * u {
        return;
    }
    nine(v, r, art::COVER, 8.0, u);
    if sat.book_t < 0.1 {
        let (lo, hi) = (r[0] + 10.0 * u, r[0] + r[2] - 4.0 * u); // right of the spine
        let (cx, isz) = ((lo + hi) * 0.5, ((hi - lo) * 0.4).min(r[3] * 0.25));
        crate::push_stack(v, (cx - isz * 0.5, r[1] + r[3] * 0.3, isz, isz), 0.875, ItemStack { id: 340, count: 1, damage: 0 });
        text_in(v, cx, r[1] + r[3] * 0.3 + isz + 6.0 * u, tk, (lo, hi), "Recipes", COVER_INK);
        if !sat.book {
            text_in(v, cx, r[1] + r[3] * 0.86, sk, (lo, hi), "Tap to open", [0.74, 0.66, 0.48, 1.0]);
        }
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

/// Brighter than `GOOD` / `BAD` / `WARN`: these sit on a black outline, not on bare parchment.
const GOOD_T: [f32; 4] = [0.45, 0.95, 0.35, 1.0];
const BAD_T: [f32; 4] = [1.0, 0.4, 0.35, 1.0];
const WARN_T: [f32; 4] = [1.0, 0.72, 0.2, 1.0];

/// The picked recipe, on a card sunk into the page: result, a line on where it is made, the ingredients with what the bag has of each
/// (all outlined text; the ingredient rows scroll).
fn draw_card(v: &mut Vec<HudVertex>, sat: &Satchel, b: &BookFrame, c: &Ctx, (tk, sk, u): (f32, f32, f32)) {
    let r = b.card;
    nine(v, r, art::CARD, 3.0, u);
    let o = (u * 0.34).round().max(1.0);
    let Some(e) = sat.sel.and_then(|i| book::entries().get(i)) else {
        paragraph(v, r, (sk, u), "Pick a recipe on the left page: its items go into the grid.", WHITE, o);
        return;
    };
    if c.discovery && !c.notes.known(e) {
        paragraph(v, r, (sk, u), "??? Find one of its items to learn this recipe.", WHITE, o);
        return;
    }
    let m = card_m(r, (sk, u), e.need.len());
    crate::push_stack(v, (r[0] + 3.0 * u, r[1] + 3.0 * u, m.isz, m.isz), 0.875, ItemStack { count: 1, ..e.out });
    let name = items::name(e.out).unwrap_or("?");
    let head = if e.out.count > 1 { format!("{name} x{}", e.out.count) } else { name.to_string() };
    let (tx, gw) = (r[0] + 6.0 * u + m.isz, c.screen.gw);
    let mw = r[0] + r[2] - 3.0 * u - tx;
    text_fit_o(v, (tx, r[1] + 2.0 * u), tk, mw, &head, WHITE, o);
    let short = book::missing(e, c.inv, &c.screen.grid);
    let (note, col) = match (e.src, e.grid) {
        (Src::Smelt(_), _) => ("Smelt it in a furnace", WARN_T),
        (_, g) if g > gw => ("Needs a workbench (3x3)", WARN_T),
        _ if short.is_empty() => ("Tap again to add a set", GOOD_T),
        _ => ("Missing ingredients", BAD_T),
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
        text_in_o(v, x + m.chip * 0.5, y + m.chip + u, sk, (lo, lo + m.cw), &t, if have >= u32::from(need) { GOOD_T } else { BAD_T }, o);
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

    fn frame(sat: &Satchel, inv: &Inventory, gw: usize) -> Frame {
        Frame::new(sat, inv, gw, SIZE.0, SIZE.1)
    }

    fn open_book() -> Satchel {
        let mut s = Satchel::new(true);
        s.tick(10.0);
        s
    }

    fn centre(r: [f32; 4]) -> (f32, f32) {
        (r[0] + r[2] * 0.5, r[1] + r[3] * 0.5)
    }

    #[test]
    fn every_slot_has_one_cell_that_a_finger_finds_in_both_layouts() {
        let mut inv = Inventory::default();
        inv.slots[12] = Some(ItemStack { id: 17, count: 5, damage: 0 });
        for sat in [{ let mut s = Satchel::new(false); s.tick(10.0); s }, open_book()] {
            let f = frame(&sat, &inv, 2);
            for i in (0..9).chain(MAIN..MAIN + 4) {
                let c = f.cells.iter().filter(|(id, _)| *id == SlotId::Inv(i)).count();
                assert_eq!(c, 1, "slot {i}");
            }
            for &(id, r) in &f.cells {
                let (x, y) = centre(r);
                assert_eq!(f.hit(&sat, x, y), Pick::Slot(id));
                assert!(r[0] >= 0.0 && r[1] >= 0.0 && r[0] + r[2] <= SIZE.0 && r[1] + r[3] <= SIZE.1, "on the screen: {r:?}");
            }
        }
        // Both layouts are the same grid: all 27 main slots have a cell even when the bag is empty.
        for sat in [Satchel::new(false), open_book()] {
            let f = frame(&sat, &Inventory::default(), 2);
            assert_eq!(f.cells.iter().filter(|(id, _)| matches!(id, SlotId::Inv(i) if (9..MAIN).contains(i))).count(), 27);
        }
    }

    #[test]
    fn opening_the_book_glides_the_bag_left_and_never_into_the_cover() {
        let inv = Inventory::default();
        let at = |t: f32| {
            let mut s = Satchel::new(true);
            (s.age, s.book_t) = (10.0, t);
            frame(&s, &inv, 2)
        };
        let (c, m, o) = (at(0.0), at(0.5), at(1.0));
        assert_eq!((c.bag[2], c.bag[3]), (o.bag[2], o.bag[3]), "the bag keeps its size");
        assert!(c.bag[0] > m.bag[0] && m.bag[0] > o.bag[0], "it glides left");
        let dx = o.bag[0] - c.bag[0];
        for ((i, r), (j, q)) in c.cells.iter().zip(&o.cells) {
            assert!(i == j && (r[2], r[3]) == (q[2], q[3]) && (q[0] - r[0] - dx).abs() < 0.01 && q[1] == r[1], "cell {i:?} only moves sideways");
        }
        for f in [&c, &m, &o] {
            let r = f.book.as_ref().expect("book").rect;
            assert!(r[0] >= f.bag[0] + f.bag[2] && r[0] + r[2] <= SIZE.0, "the book stays right of the bag and on the screen: {r:?}");
        }
    }

    #[test]
    fn a_closed_book_lies_beside_the_bag_and_a_tap_opens_it() {
        let inv = Inventory::default();
        let mut closed = Satchel::new(false);
        closed.tick(10.0);
        let f = frame(&closed, &inv, 2);
        let r = f.book.as_ref().expect("the book is there from the start").rect;
        assert!(r[0] > f.bag[0] + f.bag[2] && r[0] + r[2] <= SIZE.0 && r[1] >= 0.0 && r[1] + r[3] <= SIZE.1, "on the screen, right of the bag");
        let (x, y) = centre(r);
        assert_eq!(f.hit(&closed, x, y), Pick::Btn(Btn::Book));
        // Open, the cover's place is a page (a tap there does nothing) and the tab above the spine shuts it.
        let (open, f) = (open_book(), frame(&open_book(), &inv, 2));
        let (x, y) = centre(f.book.as_ref().expect("book").tab);
        assert_eq!(f.hit(&open, x, y), Pick::Btn(Btn::Book));
        assert_ne!(f.hit(&open, f.book.as_ref().expect("book").right[0] + 2.0, f.book.as_ref().expect("book").right[1] + 2.0), Pick::Btn(Btn::Book));
    }

    /// The slots touch each other (rows and columns), start clear of the panel's 8-pixel border and its seam, and the pocket bands stay
    /// inside the border: the old cells covered the seam on the left.
    #[test]
    fn slots_touch_and_sit_clear_of_the_border() {
        for (w, h) in [(1604.0, 720.0), (1280.0, 720.0), (2400.0, 1080.0), (1920.0, 1080.0)] {
            let f = Frame::new(&open_book(), &Inventory::default(), 2, w, h);
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
            assert!(f.bag[1] + f.bag[3] <= h, "the bag fits the height at {w}x{h}");
        }
    }

    /// The grid, the arrow and the result stay on the right page on every screen shape (the old sizes came from the height only).
    #[test]
    fn the_result_stays_on_the_page_on_every_screen() {
        for (w, h) in [(1604.0, 720.0), (1280.0, 720.0), (2400.0, 1080.0), (1920.0, 1080.0), (2340.0, 1080.0), (1560.0, 720.0)] {
            for gw in [2, 3] {
                let f = Frame::new(&open_book(), &Inventory::default(), gw, w, h);
                let b = f.book.as_ref().expect("book");
                let edge = b.right[0] + b.right[2] - b.p;
                for &(_, r) in &b.grid {
                    assert!(r[0] >= b.right[0] + b.p - 0.01 && r[0] + r[2] <= edge + 0.01, "{gw}x{gw} cell {r:?} at {w}x{h} (edge {edge})");
                }
                assert!(b.arrow[0] + b.arrow[2] <= b.grid.last().expect("result").1[0] + 0.01, "the arrow is left of the result");
            }
        }
    }

    /// The book is as tall closed as open (the back cover only grows sideways), and the cover is one page wide.
    #[test]
    fn the_book_is_as_tall_closed_as_open() {
        let inv = Inventory::default();
        let mut closed = Satchel::new(false);
        closed.tick(10.0);
        let (c, o) = (frame(&closed, &inv, 2), frame(&open_book(), &inv, 2));
        let (rc, ro) = (c.book.expect("closed").rect, o.book.expect("open").rect);
        assert_eq!((rc[1], rc[3]), (ro[1], ro[3]));
        assert!(rc[2] < ro[2]);
    }

    /// A text is stepped down whole pixels and then cut until it fits; a paragraph wraps within its width.
    #[test]
    fn text_is_fitted_and_wrapped_within_its_box() {
        for (s, k, maxw) in [("Redstone & rails", 3.0, 150.0), ("Redstone & rails", 3.0, 40.0), ("Hold the result to craft all", 2.0, 60.0), ("X", 3.0, 100.0)] {
            let (kk, t) = fitted(s, k, maxw);
            assert!(kk >= 1.0 && kk <= k && kk.fract() == 0.0 && HudPipeline::text_width(&t) as f32 * kk <= maxw.max(8.0), "{s}: {kk} {t}");
        }
        assert_eq!(fitted("X", 3.0, 100.0), (3.0, "X".to_string()), "a text that fits keeps its size");
        for l in lines("Pick a recipe on the left page: its items go into the grid.", 2.0, 150.0) {
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
    fn cells_are_big_enough_for_a_thumb_beside_the_book() {
        let f = frame(&open_book(), &Inventory::default(), 2);
        let min = f.cells.iter().map(|(_, r)| r[2]).fold(f32::MAX, f32::min);
        assert!(min >= 0.08 * SIZE.1, "a slot is {min} px");
        let b = f.book.as_ref().expect("book");
        assert!(f.bag[0] + f.bag[2] < b.left[0], "the bag and the book do not overlap");
    }

    #[test]
    fn the_book_turns_pages_lists_recipes_and_scrolls() {
        let inv = Inventory::default();
        let mut sat = open_book();
        let f = frame(&sat, &inv, 2);
        let b = f.book.as_ref().expect("book");
        assert_eq!(b.cats.len(), CATS.len());
        let (x, y) = centre(b.cats[0].1);
        assert_eq!(f.hit(&sat, x, y), Pick::Cat(0));
        sat.turn(Some(0));
        sat.tick(1.0);
        let f = frame(&sat, &inv, 2);
        let b = f.book.as_ref().expect("book");
        assert!(b.back.is_some() && !b.rows.is_empty());
        let (x, y) = centre(b.rows[0].1);
        assert_eq!(f.hit(&sat, x, y), Pick::Row(b.rows[0].0));
        let (x, y) = centre(b.back.expect("back"));
        assert_eq!(f.hit(&sat, x, y), Pick::Back);
        // Scrolled down, other rows show and the first is gone.
        let first = b.rows[0].0;
        sat.scroll = b.max_scroll.min(2.5 * b.row_h);
        let f = frame(&sat, &inv, 2);
        let b = f.book.as_ref().expect("book");
        assert!(b.max_scroll > 0.0 && b.rows.iter().all(|(i, _)| *i != first) == (sat.scroll >= b.row_h));
    }

    #[test]
    fn the_right_page_has_the_grid_the_screen_asks_for() {
        let inv = Inventory::default();
        let sat = open_book();
        for (gw, cells) in [(2, 4), (3, 9)] {
            let f = frame(&sat, &inv, gw);
            let b = f.book.as_ref().expect("book");
            assert_eq!(b.grid.iter().filter(|(id, _)| matches!(id, SlotId::Grid(_))).count(), cells);
            let r = b.grid.iter().find(|(id, _)| *id == SlotId::Result).expect("result").1;
            assert!(r[0] + r[2] <= b.right[0] + b.right[2], "the result stays on the page ({gw}x{gw})");
            assert!(b.card[3] > 0.1 * SIZE.1, "the card has room ({gw}x{gw})");
            let (x, y) = centre(r);
            assert_eq!(f.hit(&sat, x, y), Pick::Slot(SlotId::Result));
        }
    }

    #[test]
    fn a_finger_past_everything_throws_and_the_papers_do_not() {
        let inv = Inventory::default();
        let sat = open_book();
        let f = frame(&sat, &inv, 2);
        assert_eq!(f.hit(&sat, 2.0, SIZE.1 - 2.0), Pick::Outside);
        let b = f.book.as_ref().expect("book");
        assert_eq!(f.hit(&sat, b.right[0] + 2.0, b.right[1] + b.right[3] - 2.0), Pick::Panel);
        let (x, y) = centre(f.btns.iter().find(|(b, _)| *b == Btn::Close).expect("close").1);
        assert_eq!(f.hit(&sat, x, y), Pick::Btn(Btn::Close));
    }

    /// A pad steps from slot to slot, from the bag's edge to the book, up to the buttons, and never off a screen with nothing that way.
    #[test]
    fn a_pad_steps_between_targets() {
        let inv = Inventory::default();
        let sat = open_book();
        let f = frame(&sat, &inv, 2);
        let t = f.targets(&sat);
        let cell = |id: SlotId| f.cells.iter().find(|(i, _)| *i == id).expect("cell").1;
        let on = |r: [f32; 4]| centre(r);
        // Along a row and down a column the neighbour is the touching cell.
        let (a, b) = (cell(SlotId::Inv(9)), cell(SlotId::Inv(10)));
        assert_eq!(step(&t, on(a), (1, 0)), Some(b));
        assert_eq!(step(&t, on(b), (-1, 0)), Some(a));
        assert_eq!(step(&t, on(a), (0, 1)), Some(cell(SlotId::Inv(18))));
        // Up from the bag's first row: the armor row (a block over it), up from that: the button row.
        let armor_up = step(&t, on(a), (0, -1)).expect("the armor row is above");
        assert!(f.cells.iter().any(|&(id, r)| r == armor_up && matches!(id, SlotId::Inv(i) if i >= MAIN)));
        let btn = step(&t, on(armor_up), (0, -1)).expect("a button is above the armor");
        assert!(f.btns.iter().any(|&(_, r)| r == btn));
        // Nothing lies below the belt; right of the bag's last column is the book (the page's first thing), not another slot.
        assert_eq!(step(&t, on(cell(SlotId::Inv(0))), (0, 1)), None);
        let last = cell(SlotId::Inv(26));
        let right = step(&t, on(last), (1, 0)).expect("the book is right of the bag");
        assert!(right[0] > f.bag[0] + f.bag[2], "{right:?}");
        // Off every target (a page turned under the cursor): the nearest one, in any direction.
        let hole = (f.bag[0] + f.bag[2] + 1.0, 1.0);
        assert!(t.iter().all(|r| !inside(*r, hole.0, hole.1)));
        assert!(step(&t, hole, (0, 1)).is_some());
        // Every target is on the screen and answers a finger with something (never Panel / Outside).
        for r in &t {
            let (x, y) = centre(*r);
            assert!(!matches!(f.hit(&sat, x, y), Pick::Panel | Pick::Outside | Pick::List | Pick::Card), "{r:?}");
        }
    }

    /// The recipes the bag can make come first on a page of the list and keep that order where a finger and the pad find them.
    #[test]
    fn the_list_puts_what_can_be_made_first() {
        let mut inv = Inventory::default();
        inv.slots[10] = Some(ItemStack { id: 17, count: 3, damage: 0 });
        let blocks = CATS.iter().position(|&c| c == book::Cat::Blocks).expect("blocks");
        let mut sat = open_book();
        sat.turn(Some(blocks));
        sat.tick(1.0);
        let planks = book::entries().iter().position(|e| e.out.id == 5 && e.grid == 2 && matches!(e.src, Src::Craft(_))).expect("planks");
        let f = Frame::with_grid(&sat, &inv, &[None; 9], 2, SIZE.0, SIZE.1);
        let rows = &f.book.as_ref().expect("book").rows;
        assert_eq!(rows[0].0, planks, "logs in the bag: planks are the first row");
        let (x, y) = centre(rows[0].1);
        assert_eq!(f.hit(&sat, x, y), Pick::Row(planks));
        // With an empty bag the page is in its own order again.
        let empty = Frame::with_grid(&sat, &Inventory::default(), &[None; 9], 2, SIZE.0, SIZE.1);
        assert_eq!(empty.book.as_ref().expect("book").rows[0].0, book::in_cat(book::Cat::Blocks)[0]);
    }

    /// A count lies inside its cell, inside it (the vanilla count's box ended a sixteenth past it), on every screen shape.
    #[test]
    fn a_count_stays_inside_its_cell() {
        for (w, h) in [(1604.0, 720.0), (1280.0, 720.0), (2400.0, 1080.0), (1920.0, 1080.0)] {
            let f = Frame::new(&open_book(), &Inventory::default(), 2, w, h);
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

    /// Armor, bag and belt are three blocks with a clear gap between them.
    #[test]
    fn the_blocks_are_apart() {
        let f = frame(&open_book(), &Inventory::default(), 2);
        let u = units(f.k).0;
        let row = |id: SlotId| f.cells.iter().find(|(i, _)| *i == id).expect("cell").1;
        let (armor, top, last, belt) = (row(SlotId::Inv(MAIN)), row(SlotId::Inv(9)), row(SlotId::Inv(35)), row(SlotId::Inv(0)));
        assert!(top[1] - (armor[1] + armor[3]) >= 11.0 * u && belt[1] - (last[1] + last[3]) >= 11.0 * u, "gaps between the blocks");
        assert!(f.bag[1] + f.bag[3] <= SIZE.1);
    }

    /// The page's content keeps clear of its rim (6 sprite pixels), the pin is a picture whose box lies in the header, and the card
    /// shows at least one row of ingredients below its header and scrolls only when there are more rows than fit.
    #[test]
    fn the_page_has_margins_a_pin_box_and_a_scrolling_card() {
        let inv = Inventory::default();
        for gw in [2, 3] {
            let mut sat = open_book();
            sat.sel = Some(0);
            let f = frame(&sat, &inv, gw);
            let b = f.book.as_ref().expect("book");
            let (u, sk) = units(f.k);
            assert!(b.p >= 10.0 * u, "content is 4 sprite pixels clear of the rim");
            assert!(b.pin[1] >= b.right[1] + b.p && b.pin[1] + b.pin[3] <= b.right[1] + b.hh + 0.01 && b.pin[0] + b.pin[2] <= b.right[0] + b.right[2] - b.p + 0.01, "pin box in the header");
            let (x, y) = centre(b.pin);
            assert_eq!(f.hit(&sat, x, y), Pick::Pin);
            assert!(b.pin[1] + b.pin[3] <= b.grid[0].1[1], "the pin does not reach the grid");
            let one = card_m(b.card, (sk, u), 1);
            assert!(b.card[3] - one.top >= one.chip + 8.0 * sk + 2.0 * u, "a row of ingredients fits the card ({gw}x{gw})");
            assert_eq!(one.max, 0.0, "one row never scrolls");
            assert_eq!(card_m(b.card, (sk, u), 5).max, 0.0, "five fit in one row");
            assert!(card_m(b.card, (sk, u), 11).max > 0.0, "three rows scroll");
            let (x, y) = centre(b.card);
            assert_eq!(f.hit(&sat, x, y), Pick::Card);
            assert!(b.card[1] + b.card[3] < b.hide[1], "the card ends above the toggle");
        }
    }
}
