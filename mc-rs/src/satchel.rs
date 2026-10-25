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
//!   pixel size (`u` = the text's pixel): stitched leather with rivets, a pocket / belt / buckle per row, an inset slot with a category rim,
//!   a burgundy cover with a strap and clasp, aged parchment with a red-stitched binding, leather plates for buttons and tags.
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
}

impl Satchel {
    pub fn new(book: bool) -> Self {
        Self { age: 0.0, book, book_t: 0.0, cat: None, scroll: 0.0, page_t: 1.0, sel: None }
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
}

pub struct Frame {
    /// The text unit (px per GUI unit).
    pub k: f32,
    pub bag: [f32; 4],
    pub cells: Vec<(SlotId, [f32; 4])>,
    /// The pocket band behind each row of cells (the armor row, three of the bag, the belt), and whether it is the belt's.
    pub rows: Vec<([f32; 4], bool)>,
    pub labels: Vec<(&'static str, f32, f32)>,
    pub btns: Vec<(Btn, [f32; 4])>,
    pub book: Option<BookFrame>,
}

impl Frame {
    /// Every rect of the screen for `w` x `h` px, this state of the satchel and a crafting grid `gw` wide (`_inv` is not read: the layout
    /// is fixed, every slot has its cell).
    pub fn new(sat: &Satchel, _inv: &Inventory, gw: usize, w: f32, h: f32) -> Self {
        let k = craft::panel(0, w, h).2;
        let pad = 0.03 * h; // room for the leather's border and its seam
        let rise = (1.0 - ease(sat.age / RISE_SECS)) * 0.1 * h;
        let (top, bh) = (0.17 * h + rise, 0.74 * h);
        let lh = 8.0 * 0.6 * k + 4.0;
        let mut f = Frame { k, bag: [0.0; 4], cells: Vec::new(), rows: Vec::new(), labels: Vec::new(), btns: Vec::new(), book: None };

        // The bag: 9 columns, the armor row on top, the belt at the foot. With the book closed the bag and the cover lie side by side, centred
        // together; as the book opens the bag glides to the left 45% of the screen and the book's spine to the middle of the right 55%.
        let pitch = ((0.455 * w - 2.0 * pad) / 9.0).min((bh - 2.0 * pad - lh) / 5.55);
        let (pw, ph) = (9.0 * pitch + 2.0 * pad, 5.55 * pitch + 2.0 * pad + lh);
        let (open_x, gap, half) = (0.012 * w, 0.02 * w, (BOOK_R - BOOK_L) * 0.5 * w); // half = a page of the open book
        let closed_x = ((w - (pw + gap + half)) * 0.5).max(open_x);
        let e = ease(sat.book_t);
        let x = lerp(closed_x, open_x, e);
        let spine = lerp(closed_x + pw + gap + half * 0.5, (BOOK_L + BOOK_R) * 0.5 * w, e);
        f.bag = [x, top, pw, ph];
        let (bx, by, cell) = (x + pad, top + pad + lh, pitch * 0.92);
        f.labels.push(("Armor", bx, top + pad));
        let band = |y: f32, n: f32| [bx - 0.4 * pad, y - 0.12 * pitch, n * pitch + 0.4 * pad, cell + 0.18 * pitch]; // a lip above the cells, a hem under them
        f.rows.push((band(by, 4.0), false));
        for i in 0..4 {
            f.cells.push((SlotId::Inv(MAIN + 3 - i), [bx + i as f32 * pitch, by, cell, cell])); // helmet first
        }
        for r in 0..3 {
            let y = by + (1.15 + r as f32) * pitch;
            f.rows.push((band(y, 9.0), false));
            for c in 0..9 {
                f.cells.push((SlotId::Inv(9 + r * 9 + c), [bx + c as f32 * pitch, y, cell, cell]));
            }
        }
        f.rows.push((band(by + 4.55 * pitch, 9.0), true));
        for c in 0..9 {
            f.cells.push((SlotId::Inv(c), [bx + c as f32 * pitch, by + 4.55 * pitch, cell, cell]));
        }

        // The buttons above the bag, and the close button at the screen's corner.
        let (by, bh2, tk) = (0.035 * h + rise, 0.095 * h, 0.75 * k);
        let mut x = f.bag[0];
        for (b, label) in [(Btn::Sort, "Sort"), (Btn::Shift, "Shift"), (Btn::One, "One")] {
            let bw = HudPipeline::text_width(label) as f32 * tk + 6.0 * tk;
            f.btns.push((b, [x, by, bw, bh2]));
            x += bw + 0.012 * h;
        }
        let cw = HudPipeline::text_width("X") as f32 * tk + 8.0 * tk;
        f.btns.push((Btn::Close, [w - 0.02 * w - cw, by, cw, bh2]));

        f.book = Some(book_frame(sat, gw, (w, h), (top, bh), spine));
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
                    .or_else(|| b.list.filter(|r| inside(*r, x, y)).map(|_| Pick::List));
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

/// The book's rects: closed, a cover one page wide; opening, both pages grow from the spine; the contents exist once it is open.
fn book_frame(sat: &Satchel, gw: usize, (w, h): (f32, f32), (top, bh): (f32, f32), spine: f32) -> BookFrame {
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
    let (p, hh) = (0.028 * h, 0.085 * h); // `p`: clear of the page's rim
    let slide = (1.0 - ease(sat.page_t)) * 0.05 * w;
    let l = b.left;
    match sat.cat {
        None => {
            let cg = 0.012 * h;
            let ch = (l[3] - hh - p - 5.0 * cg) / 6.0;
            for i in 0..CATS.len() {
                b.cats.push((i, [l[0] + p + slide, l[1] + hh + i as f32 * (ch + cg), l[2] - 2.0 * p, ch]));
            }
        }
        Some(ci) => {
            b.back = Some([l[0] + p, l[1] + p, 0.14 * l[2], hh - p]);
            let list = [l[0] + p + slide, l[1] + hh + p, l[2] - 2.0 * p, l[3] - hh - 2.0 * p];
            b.list = Some(list);
            b.row_h = 0.1 * h;
            let ids = book::in_cat(CATS[ci.min(CATS.len() - 1)]);
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
    let (c, g) = (if gw == 2 { 0.11 * h } else { 0.085 * h }, 0.012 * h);
    let (gx, gy) = (r[0] + p + 0.01 * w, r[1] + hh);
    for i in 0..gw * gw {
        b.grid.push((SlotId::Grid(i), [gx + (i % gw) as f32 * (c + g), gy + (i / gw) as f32 * (c + g), c, c]));
    }
    let gh = gw as f32 * (c + g) - g;
    let (ax, aw) = (gx + gw as f32 * (c + g) + 0.004 * w, 0.03 * w);
    b.arrow = [ax, gy + gh * 0.5 - 0.02 * h, aw, 0.04 * h];
    let rc = c * 1.2;
    b.grid.push((SlotId::Result, [ax + aw + 0.004 * w, gy + (gh - rc) * 0.5, rc, rc]));
    let cy = gy + gh + 0.03 * h;
    b.card = [r[0] + p, cy, r[2] - 2.0 * p, r[1] + r[3] - 0.185 * h - cy];
    b.pin = [r[0] + r[2] - p - 0.2 * r[2], r[1] + p, 0.2 * r[2], hh - p];
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
        crate::push_stack(v, (r[0], r[1], r[2], r[3]), 0.875, st);
    }
}

/// A leather plate with its label: `state` 0 disabled, 1 normal, 2 on.
fn button(v: &mut Vec<HudVertex>, r: [f32; 4], (tk, u): (f32, f32), text: &str, state: u8) {
    let c = if state == 0 { [0.6, 0.6, 0.6, 1.0] } else { WHITE };
    HudPipeline::push_nine(v, (r[0], r[1], r[2], r[3]), if state == 2 { art::BTN_ON } else { art::BTN }, 4.0, u, c, true);
    HudPipeline::push_text_centered(v, r[0] + r[2] * 0.5, r[1] + (r[3] - 8.0 * tk) * 0.5, tk, text, [1.0, 0.97, 0.85, 1.0]);
}

/// A title's rule under a page's header: an ink line across the page.
fn rule(v: &mut Vec<HudVertex>, page: [f32; 4], hh: f32, p: f32, u: f32) {
    HudPipeline::push_tiled(v, (page[0] + p, page[1] + hh - 4.3 * u, page[2] - 2.0 * p, 4.0 * u), art::RULE, u, WHITE);
}

/// The whole screen: the dimmed world, the bag, the book, the stack on the finger and the name of what it touches.
pub fn draw(v: &mut Vec<HudVertex>, (w, h): (f32, f32), sat: &Satchel, c: &Ctx) {
    let f = Frame::new(sat, c.inv, c.screen.gw, w, h);
    let (k, tk, sk) = (f.k, 0.75 * f.k, 0.6 * f.k);
    let u = tk;
    HudPipeline::push_quad(v, 0.0, 0.0, w, h, [0.0, 0.0, 0.0, 0.5]);

    // The bag: leather with its seam and rivets; a pocket behind each row (the belt's is a strap with a buckle), the label tag sewn over
    // the armor pocket's lip, then every cell.
    nine(v, f.bag, art::BAG, 8.0, u);
    for &(r, belt) in &f.rows {
        if belt {
            nine(v, r, art::BELT, 4.0, u);
            let (bh, bw) = (r[3] * 0.5, r[3] * 0.5 * 12.0 / 16.0);
            HudPipeline::push_gui(v, r[0] + r[2] - 10.0, r[1] + (r[3] - bh) * 0.5, bw, bh, art::BUCKLE);
        } else {
            nine(v, r, art::POCKET, 4.0, u);
        }
    }
    for &(text, x, y) in &f.labels {
        let tw = HudPipeline::text_width(text) as f32 * sk;
        HudPipeline::push_nine(v, (x - 3.0 * u, y - u, tw + 6.0 * u, 8.0 * sk + 2.0 * u), art::TAB, 3.0, u, WHITE, true);
        HudPipeline::push_text(v, x, y, sk, text, INK);
    }
    for &(id, r) in &f.cells {
        let st = c.screen.get(c.inv, None, id);
        let rim = if id == SlotId::Inv(c.hotbar) { Some(SELECTED) } else { st.map(|s| tint(s.id)) };
        slot(v, r, st, rim, u);
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

    // The stack on the finger floats above it; a finger down names what it holds or rests on.
    let (fx, fy) = c.finger;
    if let Some(st) = c.screen.cursor {
        let sz = 16.0 * k;
        crate::push_stack(v, (fx - sz * 0.5, fy - sz * 1.4, sz, sz), 0.875, st);
    }
    if c.down {
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
        nine(v, [b.rect[0] - cm, b.rect[1] - cm, b.rect[2] + 2.0 * cm, b.rect[3] + 2.0 * cm], art::COVER, 8.0, u);
        let r = (b.rect[0], b.rect[1], b.rect[2], b.rect[3]);
        HudPipeline::push_tiled(v, r, art::PARCH, u, WHITE);
        HudPipeline::push_nine(v, r, art::PAGE_EDGE, 4.0, u, WHITE, false);
        if sat.book_t > 0.3 {
            HudPipeline::push_tiled(v, (r.0 + r.2 * 0.5 - 4.0 * u, r.1 + u, 8.0 * u, r.3 - 2.0 * u), art::BINDING, u, WHITE);
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
    let (p, hh) = (0.028 * h, 0.085 * h);
    let entries = book::entries();
    let gw = c.screen.gw;
    let title_y = |r: [f32; 4]| r[1] + (hh - 8.0 * tk) * 0.5;
    rule(v, b.left, hh, p, u);
    rule(v, b.right, hh, p, u);

    // Left page: the categories, or the recipes of one.
    match sat.cat {
        None => {
            HudPipeline::push_text(v, b.left[0] + p, title_y(b.left), tk, "Recipes", INK);
            for &(i, r) in &b.cats {
                let ids = book::in_cat(CATS[i]);
                nine(v, r, art::ENTRY, 3.0, u);
                if let Some(e) = ids.first().map(|&e| &entries[e]) {
                    crate::push_stack(v, (r[0] + 3.0 * u, r[1] + (r[3] - r[3] * 0.8) * 0.5, r[3] * 0.8, r[3] * 0.8), 0.875, ItemStack { count: 1, ..e.out });
                }
                HudPipeline::push_text(v, r[0] + r[3] + 4.0 * u, r[1] + (r[3] - 8.0 * tk) * 0.5, tk, CATS[i].label(), INK);
                let n = ids.len().to_string();
                HudPipeline::push_text(v, r[0] + r[2] - HudPipeline::text_width(&n) as f32 * tk - 5.0 * u, r[1] + (r[3] - 8.0 * tk) * 0.5, tk, &n, INK);
            }
        }
        Some(ci) => {
            if let Some(r) = b.back {
                nine(v, r, art::ENTRY, 3.0, u);
                let (aw, ah) = (12.0 * u * 0.9, 9.0 * u * 0.9);
                HudPipeline::push_gui_tinted(v, r[0] + (r[2] - aw) * 0.5, r[1] + (r[3] - ah) * 0.5, aw, ah, art::ARROW, INK);
            }
            HudPipeline::push_text(v, b.left[0] + p + 0.14 * b.left[2] + 8.0 * u, title_y(b.left), tk, CATS[ci.min(CATS.len() - 1)].label(), INK);
            for &(ei, r) in &b.rows {
                let e = &entries[ei];
                let known = !c.discovery || c.notes.known(e);
                let ok = known && book::craftable(e, gw, c.inv, &c.screen.grid);
                let sel = sat.sel == Some(ei);
                nine(v, r, if sel { art::ENTRY_SEL } else { art::ENTRY }, 3.0, u);
                let isz = r[3] * 0.82;
                let ib = (r[0] + 3.0 * u, r[1] + (r[3] - isz) * 0.5, isz, isz);
                if known {
                    crate::push_stack(v, ib, 0.875, ItemStack { count: 1, ..e.out });
                } else {
                    HudPipeline::push_nine(v, (ib.0, ib.1, ib.2, ib.3), art::SLOT, 2.0, u, WHITE, true);
                    HudPipeline::push_text_centered(v, ib.0 + ib.2 * 0.5, ib.1 + (ib.3 - 8.0 * tk) * 0.5, tk, "?", [0.7, 0.7, 0.75, 1.0]);
                }
                let name = if known { items::name(e.out).unwrap_or("?") } else { "???" };
                HudPipeline::push_text(v, ib.0 + isz + 6.0 * u, r[1] + (r[3] - 8.0 * tk) * 0.5, tk, name, INK);
                // The gem at the right: green = the bag has it all and the grid fits; orange = a furnace's; red = something is short.
                let dot = if !known { MUTE } else if e.grid == 0 { WARN } else if ok { GOOD } else { BAD };
                let ds = r[3] * 0.34;
                HudPipeline::push_gui_tinted(v, r[0] + r[2] - ds - 5.0 * u, r[1] + (r[3] - ds) * 0.5, ds, ds, art::GEM, dot);
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
    HudPipeline::push_text(v, b.right[0] + p, title_y(b.right), tk, if gw == 3 { "Workbench" } else { "Crafting" }, INK);
    if let Some(sel) = sat.sel {
        let pinned = c.notes.pinned == Some(sel);
        button(v, b.pin, (sk, u), if pinned { "Unpin" } else { "Pin" }, if pinned { 2 } else { 1 });
    }
    for &(id, r) in &b.grid {
        let st = c.screen.get(c.inv, None, id);
        let rim = (id == SlotId::Result && st.is_some()).then_some(GOOD);
        slot(v, r, st, rim, u);
    }
    HudPipeline::push_text_centered(v, b.arrow[0] + b.arrow[2] * 0.5, b.arrow[1] + (b.arrow[3] - 8.0 * tk) * 0.5, tk, "->", INK);
    if c.hold {
        if let Some((_, r)) = b.grid.iter().find(|(id, _)| *id == SlotId::Result) {
            HudPipeline::push_text_centered(v, r[0] + r[2] * 0.5, r[1] - 10.0 * sk, sk, "Release: craft all", GOOD);
        }
    }
    draw_card(v, sat, b, c, (tk, sk, u));

    // The discovery toggle and the hint under it.
    nine(v, b.hide, art::ENTRY, 3.0, u);
    let label = if c.discovery { "Unknown recipes: hidden" } else { "Unknown recipes: shown" };
    HudPipeline::push_text_centered(v, b.hide[0] + b.hide[2] * 0.5, b.hide[1] + (b.hide[3] - 8.0 * sk) * 0.5, sk, label, INK);
    HudPipeline::push_text_centered(v, b.hide[0] + b.hide[2] * 0.5, b.hide[1] + b.hide[3] + 3.0 * sk, sk, "Hold the result to craft all", [0.4, 0.3, 0.2, 1.0]);
}

/// The closed book: a burgundy leather cover (seam, tooled line, gold studs), a strap with a clasp, the book icon and its title; the
/// writing only while the cover is flat (it is foreshortened, narrower, while it swings).
fn draw_cover(v: &mut Vec<HudVertex>, r: [f32; 4], sat: &Satchel, (tk, sk, u): (f32, f32, f32)) {
    if r[2] < 8.0 * u {
        return;
    }
    nine(v, r, art::COVER, 8.0, u);
    let sx = r[0] + r[2] * 0.78; // the strap, down the cover, and its clasp
    HudPipeline::push_tiled(v, (sx, r[1] + u, 8.0 * u, r[3] - 2.0 * u), art::STRAP, u, WHITE);
    HudPipeline::push_gui(v, sx - 2.0 * u, r[1] + r[3] * 0.5 - 10.0 * u, 12.0 * u, 20.0 * u, art::CLASP);
    if sat.book_t < 0.1 {
        let (cx, isz) = (r[0] + r[2] * 0.39, (r[2] * 0.3).min(r[3] * 0.25));
        crate::push_stack(v, (cx - isz * 0.5, r[1] + r[3] * 0.3, isz, isz), 0.875, ItemStack { id: 340, count: 1, damage: 0 });
        HudPipeline::push_text_centered(v, cx, r[1] + r[3] * 0.3 + isz + 6.0 * u, tk, "Recipes", GOLD);
        if !sat.book {
            HudPipeline::push_text_centered(v, cx, r[1] + r[3] * 0.86, sk, "Tap to open", [0.85, 0.7, 0.55, 1.0]);
        }
    }
}

/// The picked recipe, on a card sunk into the page: result, a line on where it is made, the ingredients with what the bag has of each.
fn draw_card(v: &mut Vec<HudVertex>, sat: &Satchel, b: &BookFrame, c: &Ctx, (tk, sk, u): (f32, f32, f32)) {
    let r = b.card;
    nine(v, r, art::CARD, 3.0, u);
    let line = u * 0.5;
    let Some(e) = sat.sel.and_then(|i| book::entries().get(i)) else {
        HudPipeline::push_text(v, r[0] + 4.0 * u, r[1] + 4.0 * u, sk, "Pick a recipe on the left page:", INK);
        HudPipeline::push_text(v, r[0] + 4.0 * u, r[1] + 4.0 * u + 12.0 * sk, sk, "its items go into the grid.", INK);
        return;
    };
    if c.discovery && !c.notes.known(e) {
        HudPipeline::push_text(v, r[0] + 4.0 * u, r[1] + 4.0 * u, sk, "???  Find one of its items", INK);
        HudPipeline::push_text(v, r[0] + 4.0 * u, r[1] + 4.0 * u + 12.0 * sk, sk, "to learn this recipe.", INK);
        return;
    }
    let isz = (r[3] * 0.3).min(r[2] * 0.2);
    crate::push_stack(v, (r[0] + 3.0 * u, r[1] + 3.0 * u, isz, isz), 0.875, ItemStack { count: 1, ..e.out });
    let name = items::name(e.out).unwrap_or("?");
    let head = if e.out.count > 1 { format!("{name} x{}", e.out.count) } else { name.to_string() };
    HudPipeline::push_text(v, r[0] + isz + 8.0 * u, r[1] + 3.0 * u + (isz - 8.0 * tk) * 0.5, tk * 0.85, &head, INK);
    let gw = c.screen.gw;
    let short = book::missing(e, c.inv, &c.screen.grid);
    let (note, col) = match (e.src, e.grid) {
        (Src::Smelt(_), _) => ("Smelt it in a furnace", WARN),
        (_, g) if g > gw => ("Needs a workbench (3x3)", WARN),
        _ if short.is_empty() => ("Tap the recipe again to refill", GOOD),
        _ => ("Missing ingredients", BAD),
    };
    HudPipeline::push_text(v, r[0] + 4.0 * u, r[1] + isz + 8.0 * u, sk, note, col);
    // Ingredients, five to a row: the item, then what the bag has over what the recipe wants.
    let (cw, rowy) = (r[2] / 5.0, r[1] + isz + 8.0 * u + 12.0 * sk);
    let chip = ((r[3] - (rowy - r[1])).min(cw) * 0.7).max(1.0);
    for (n, &(code, need)) in e.need.iter().enumerate() {
        let (x, y) = (r[0] + (n % 5) as f32 * cw + (cw - chip) * 0.5, rowy + (n / 5) as f32 * (chip + 12.0 * sk + 4.0 * line));
        if y + chip > r[1] + r[3] {
            break;
        }
        crate::push_stack(v, (x, y, chip, chip), 0.875, item(code));
        let have = book::have(c.inv, &c.screen.grid, code);
        let t = format!("{have}/{need}");
        HudPipeline::push_text_centered(v, x + chip * 0.5, y + chip + 2.0 * line, sk, &t, if have >= u32::from(need) { GOOD } else { BAD });
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
}
