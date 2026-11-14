//! `GuiAchievements`, `GuiStats` (+ `GuiSlotStats*`) and `GuiAchievement` (the toast), in GUI units like `options.rs`. UNVERIFIED: not compiled, not run.
//!
//! Touch, not mouse: the achievement map is dragged with a finger (b1.7.3 drags with the mouse, and its arrow keys are gone) and a TAP on an
//! achievement shows its tooltip (hover does not exist), staying until the next tap; the lists scroll by dragging; the three tabs and the
//! column headers (tap to sort: b1.7.3's `SorterStats*` behind its header icons) and `Done` are 24 to 20 units tall. The map fills the screen
//! instead of the 224x155 window, and rows carry the item NAME beside the icon (b1.7.3 shows it on hover).
//! Not b1.7.3's: the achievement frames (`achievement/bg.png`, not in the HUD sheet) are plain quads, and the toast panel too.

use crate::input::touch_ui::ScreenEv;
use crate::menu::{dirt, grad, rgb, scaled_gs, tw};
use crate::options::{background, button, Settings};
use crate::render::atlas;
use crate::render::hud::{HudPipeline, HudVertex};
use crate::world::gen::noise::JavaRandom;
use crate::world::items::{self, ItemStack};
use crate::world::stats::*;
use std::time::Instant;

fn inside(r: [f32; 4], x: f32, y: f32) -> bool {
    x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]
}

fn quad(v: &mut Vec<HudVertex>, s: f32, r: [f32; 4], c: [f32; 4]) {
    HudPipeline::push_quad(v, r[0] * s, r[1] * s, r[2] * s, r[3] * s, c);
}

fn text(v: &mut Vec<HudVertex>, s: f32, x: f32, y: f32, t: &str, c: u32) {
    HudPipeline::push_text(v, x * s, y * s, s, t, rgb(c));
}

fn stack(v: &mut Vec<HudVertex>, s: f32, x: f32, y: f32, id: u16) {
    crate::push_stack(v, (x * s, y * s, 16.0 * s, 16.0 * s), 1.0, ItemStack { id, count: 1, damage: 0 });
}

/// `FontRenderer.func_27278_a`: words wrapped to `w` units.
fn wrap(t: &str, w: f32) -> Vec<String> {
    let (mut out, mut cur) = (Vec::new(), String::new());
    for word in t.split(' ') {
        let n = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if tw(&n) > w && !cur.is_empty() {
            out.push(std::mem::replace(&mut cur, word.to_string()));
        } else {
            cur = n;
        }
    }
    out.push(cur);
    out
}

fn item_name(id: u16) -> String {
    items::name(ItemStack { id, count: 1, damage: 0 }).map_or_else(|| format!("#{id}"), str::to_string)
}

/// The scroll range of the map (`field_27126_s` .. `field_27123_v`), per axis: centred when the whole map fits.
fn span(lo: i32, hi: i32, view: f32) -> (f32, f32) {
    let (a, b) = (lo as f32 * 24.0 - 24.0, hi as f32 * 24.0 + 50.0 - view);
    if b < a { ((a + b) / 2.0, (a + b) / 2.0) } else { (a, b) }
}

fn clamp_pan(p: (f32, f32), vw: f32, vh: f32) -> (f32, f32) {
    let ext = |f: fn(&Ach) -> i32| (ACHS.iter().map(f).min().unwrap_or(0), ACHS.iter().map(f).max().unwrap_or(0));
    let ((c0, c1), (r0, r1)) = (ext(|a| a.col), ext(|a| a.row));
    let (x, y) = (span(c0, c1, vw), span(r0, r1, vh));
    (p.0.clamp(x.0, x.1), p.1.clamp(y.0, y.1))
}

const PAGES: [&str; 3] = ["General", "Blocks", "Items"];
/// Right edges of the three value columns, from the centre line.
const COLS: [f32; 3] = [40.0, 95.0, 150.0];

struct Press {
    at: (f32, f32),
    from: (f32, f32),
    drag: bool,
}

/// 0 = Achievements, 1..=3 = Statistics General / Blocks / Items.
pub struct StatsUi {
    page: usize,
    pan: Option<(f32, f32)>,
    scroll: f32,
    /// (column, direction): -1 largest first, 1 smallest first, 0 by id.
    sort: (usize, i32),
    sel: Option<usize>,
    press: Option<Press>,
    finger: Option<(f32, f32)>,
    born: Instant,
}

impl StatsUi {
    pub fn new(ach: bool) -> Self {
        Self { page: if ach { 0 } else { 1 }, pan: None, scroll: 0.0, sort: (0, 0), sel: None, press: None, finger: None, born: Instant::now() }
    }

    /// (Done, the three tabs), GUI units.
    fn lay(gw: f32, gh: f32) -> ([f32; 4], [[f32; 4]; 3]) {
        let hw = (gw / 2.0).floor();
        ([hw - 100.0, gh - 28.0, 200.0, 24.0], [0, 1, 2].map(|i| [hw - 152.0 + 102.0 * i as f32, gh - 56.0, 100.0, 24.0]))
    }

    /// What the screen fills that the pad's button hints keep off (px): the map or the list, Done and the tabs.
    pub fn avoid(&self, w: f32, h: f32, st: &Settings) -> Vec<[f32; 4]> {
        let (s, gw, gh) = scaled_gs(w, h, st.gui_scale);
        let (done, tabs) = Self::lay(gw, gh);
        let (top, vh) = Self::view(gh);
        let mut r = vec![done, [0.0, top, gw, vh]];
        r.extend(tabs);
        r.into_iter().map(|r| r.map(|u| u * s)).collect()
    }

    /// The map's viewport: (top, height) with the full width.
    fn view(gh: f32) -> (f32, f32) {
        (24.0, gh - 24.0 - 32.0)
    }

    /// The map's scroll, centred on the first achievement until the finger moves it (`GuiAchievements.initGui`).
    fn pan(&self, vw: f32, vh: f32) -> (f32, f32) {
        clamp_pan(self.pan.unwrap_or((13.0 - vw / 2.0, 13.0 - vh / 2.0)), vw, vh)
    }

    /// Row height, header height, the rows shown, the list's top and bottom.
    fn rows(&self, gh: f32, stats: &Stats) -> (f32, f32, Vec<u16>, f32, f32) {
        let (rh, hdr) = if self.page == 1 { (14.0, 0.0) } else { (22.0, 20.0) };
        let mut ids = if self.page == 1 { Vec::new() } else { stats.listed(self.page == 2) };
        let (col, dir) = self.sort;
        if dir != 0 {
            let k = self.kinds()[col];
            ids.sort_by_key(|&id| (dir as i64 * stats.count(k, id) as i64, id));
        }
        (rh, hdr, ids, 32.0, gh - 62.0)
    }

    fn kinds(&self) -> [Kind; 3] {
        if self.page == 2 { [Kind::Crafted, Kind::Used, Kind::Mined] } else { [Kind::Depleted, Kind::Crafted, Kind::Used] }
    }

    fn max_scroll(&self, gh: f32, stats: &Stats) -> f32 {
        let (rh, hdr, ids, top, bot) = self.rows(gh, stats);
        let n = if self.page == 1 { GENERAL.len() } else { ids.len() };
        (n as f32 * rh - (bot - top - hdr)).max(0.0)
    }

    /// A finger event in px. True when `Done` is tapped (back to the game menu).
    pub fn event(&mut self, ev: ScreenEv, w: f32, h: f32, st: &Settings, stats: &Stats) -> bool {
        let (s, gw, gh) = scaled_gs(w, h, st.gui_scale);
        let (vy, vh) = Self::view(gh);
        match ev {
            ScreenEv::Down(x, y) => {
                let p = (x / s, y / s);
                self.finger = Some(p);
                let from = if self.page == 0 { self.pan(gw, vh) } else { (0.0, self.scroll) };
                self.press = Some(Press { at: p, from, drag: false });
            }
            ScreenEv::Move(x, y) => {
                let p = (x / s, y / s);
                self.finger = Some(p);
                if let Some(pr) = self.press.as_mut() {
                    let d = (p.0 - pr.at.0, p.1 - pr.at.1);
                    pr.drag |= d.0.abs().max(d.1.abs()) > 4.0;
                    if pr.drag {
                        let from = pr.from;
                        if self.page == 0 {
                            self.pan = Some(clamp_pan((from.0 - d.0, from.1 - d.1), gw, vh));
                        } else {
                            self.scroll = (from.1 - d.1).clamp(0.0, self.max_scroll(gh, stats));
                        }
                    }
                }
            }
            ScreenEv::Up(x, y) => {
                self.finger = None;
                if self.press.take().is_some_and(|p| p.drag) {
                    return false;
                }
                let (gx, gy) = (x / s, y / s);
                let (done, tabs) = Self::lay(gw, gh);
                if inside(done, gx, gy) {
                    return true;
                }
                if self.page > 0 {
                    if let Some(i) = tabs.iter().position(|&r| inside(r, gx, gy)) {
                        if self.page != i + 1 {
                            (self.page, self.scroll, self.sort) = (i + 1, 0.0, (0, 0));
                        }
                        return false;
                    }
                }
                if self.page >= 2 {
                    let hw = (gw / 2.0).floor();
                    if let Some(c) = COLS.iter().position(|&e| inside([hw + e - 54.0, 32.0, 56.0, 20.0], gx, gy)) {
                        // `func_27266_c`: a new column sorts largest first, again smallest first, again by id.
                        self.sort = match self.sort {
                            (k, -1) if k == c => (c, 1),
                            (k, 1) if k == c => (0, 0),
                            _ => (c, -1),
                        };
                    }
                } else if self.page == 0 {
                    let (px, py) = self.pan(gw, vh);
                    self.sel = ACHS.iter().position(|a| inside([a.col as f32 * 24.0 - px - 5.0, vy + a.row as f32 * 24.0 - py - 5.0, 32.0, 32.0], gx, gy) && gy >= vy && gy < vy + vh);
                }
            }
        }
        false
    }

    pub fn draw(&self, v: &mut Vec<HudVertex>, w: f32, h: f32, st: &Settings, stats: &Stats) {
        let (s, gw, gh) = scaled_gs(w, h, st.gui_scale);
        let hw = (gw / 2.0).floor();
        background(v, s, gw, gh, true);
        let bars = if self.page == 0 { self.draw_map(v, s, gw, gh, stats) } else { self.draw_list(v, s, gw, gh, stats) };
        // The dirt bars hide what the map and the rows spill over, `GuiSlot.overlayBackground`.
        for r in &bars {
            dirt(v, s, *r, 0.0, 0x404040);
        }
        if self.page > 0 {
            // The shadows `GuiSlot` draws under the bars: black to clear, 4 units.
            let (clear, black, a, b) = ([0.0; 4], [0.0, 0.0, 0.0, 1.0], bars[0][3], bars[1][1]);
            grad(v, s, [0.0, a, gw, 4.0], black, clear);
            grad(v, s, [0.0, b - 4.0, gw, 4.0], clear, black);
        }
        let title = if self.page == 0 { "Achievements" } else { "Statistics" };
        text(v, s, hw - tw(title) / 2.0, 10.0, title, 0xFFFFFF);
        let (done, tabs) = Self::lay(gw, gh);
        let over = |r: [f32; 4]| self.finger.is_some_and(|(x, y)| inside(r, x, y));
        button(v, s, done, "Done", true, over(done), None);
        if self.page > 0 {
            for (i, r) in tabs.into_iter().enumerate() {
                button(v, s, r, PAGES[i], true, over(r) || self.page == i + 1, None);
            }
        }
        if self.page == 0 {
            self.draw_tip(v, s, gw, gh, stats);
        } else if self.page > 1 {
            let names = if self.page == 2 { ["Crafted", "Used", "Mined"] } else { ["Depleted", "Crafted", "Used"] };
            for (i, n) in names.iter().enumerate() {
                let mark = if self.sort.1 != 0 && self.sort.0 == i { if self.sort.1 < 0 { " v" } else { " ^" } } else { "" };
                let t = format!("{n}{mark}");
                text(v, s, hw + COLS[i] - tw(&t), 38.0, &t, if self.sort.1 != 0 && self.sort.0 == i { 0xFFFF80 } else { 0xE0E0E0 });
            }
        }
    }

    /// `GuiAchievements.drawScreen`: the terrain pattern, the links, the frames. Returns the bars to cover the edges.
    fn draw_map(&self, v: &mut Vec<HudVertex>, s: f32, gw: f32, gh: f32, stats: &Stats) -> Vec<[f32; 4]> {
        let (vy, vh) = Self::view(gh);
        let (px, py) = self.pan(gw, vh);
        let (bx, by) = (px.floor() + 288.0, py.floor() + 288.0);
        let (tx, ty, ox, oy) = ((bx as i32).div_euclid(16), (by as i32).div_euclid(16), bx.rem_euclid(16.0), by.rem_euclid(16.0));
        let mut rnd = JavaRandom::new(0);
        let mut r = 0;
        while r as f32 * 16.0 - oy < vh {
            let row = ty + r;
            let g = 0.6 - row as f32 / 25.0 * 0.3;
            let mut c = 0;
            while c as f32 * 16.0 - ox < gw {
                rnd.set_seed(1234 + (tx + c) as i64);
                rnd.next_int();
                let n = rnd.next_int_bound((1 + row).max(1)) + row / 2;
                let id: u8 = if n <= 37 && row != 35 {
                    if n == 22 { if rnd.next_int_bound(2) == 0 { 56 } else { 73 } } else if n == 10 { 15 } else if n == 8 { 16 } else if n > 4 { 1 } else if n > 0 { 3 } else { 12 }
                } else {
                    7
                };
                HudPipeline::push_tile(v, (c as f32 * 16.0 - ox) * s, (vy + r as f32 * 16.0 - oy) * s, 16.0 * s, 16.0 * s, atlas::tile_of(id, 0), [g; 3]);
                c += 1;
            }
            r += 1;
        }
        let pulse = self.born.elapsed().as_millis() % 600;
        let sin = (pulse as f32 / 600.0 * std::f32::consts::TAU).sin();
        let (cx, cy) = (|c: i32| c as f32 * 24.0 - px + 11.0, |r: i32| vy + r as f32 * 24.0 - py + 11.0);
        for (i, a) in ACHS.iter().enumerate() {
            let Some(p) = a.parent else { continue };
            let col = if stats.has(i) { rgb(0x707070) } else if stats.open(i) { [0.0, 1.0, 0.0, if sin > 0.6 { 1.0 } else { 0.51 }] } else { [0.0, 0.0, 0.0, 1.0] };
            let (x0, x1) = (cx(a.col).min(cx(ACHS[p].col)), cx(a.col).max(cx(ACHS[p].col)));
            quad(v, s, [x0, cy(a.row), x1 - x0 + 1.0, 1.0], col);
            let (y0, y1) = (cy(a.row).min(cy(ACHS[p].row)), cy(a.row).max(cy(ACHS[p].row)));
            quad(v, s, [cx(ACHS[p].col), y0, 1.0, y1 - y0 + 1.0], col);
        }
        for (i, a) in ACHS.iter().enumerate() {
            let (x, y) = (cx(a.col) - 11.0, cy(a.row) - 11.0);
            // UNVERIFIED stand-in for the 26x26 `bg.png` frames; the shades are `GuiAchievements`' (1.0 / 0.6..0.8 / 0.3).
            let b = if stats.has(i) { 1.0 } else if stats.open(i) { if sin < 0.6 { 0.6 } else { 0.8 } } else { 0.3 };
            let edge = if self.sel == Some(i) { [1.0; 3] } else if a.special { [0.88, 0.75, 0.25] } else { [0.63; 3] };
            quad(v, s, [x - 2.0, y - 2.0, 26.0, 26.0], [edge[0] * b, edge[1] * b, edge[2] * b, 1.0]);
            quad(v, s, [x - 1.0, y - 1.0, 24.0, 24.0], [0.16 * b, 0.16 * b, 0.16 * b, 1.0]);
            stack(v, s, x + 3.0, y + 3.0, a.icon);
            if !stats.open(i) {
                quad(v, s, [x + 3.0, y + 3.0, 16.0, 16.0], [0.0, 0.0, 0.0, 0.9]);
            }
        }
        vec![[0.0, 0.0, gw, vy], [0.0, vy + vh, gw, gh - vy - vh]]
    }

    /// The tooltip of the tapped achievement (`GuiAchievements`' hover text, kept on screen until the next tap).
    fn draw_tip(&self, v: &mut Vec<HudVertex>, s: f32, gw: f32, gh: f32, stats: &Stats) {
        let Some(i) = self.sel else { return };
        let (a, (_, vh)) = (&ACHS[i], Self::view(gh));
        let (px, py) = self.pan(gw, vh);
        let (nx, ny) = (a.col as f32 * 24.0 - px, Self::view(gh).0 + a.row as f32 * 24.0 - py);
        let open = stats.open(i);
        let w = tw(a.name).max(120.0);
        let body = match (open, a.parent) {
            (false, Some(p)) => format!("Requires '{}'", ACHS[p].name),
            _ => a.desc.to_string(),
        };
        let lines = wrap(&body, w);
        let hgt = lines.len() as f32 * 10.0 + if stats.has(i) { 12.0 } else { 0.0 };
        let x = if nx + 30.0 + w + 3.0 > gw { nx - 6.0 - w } else { nx + 30.0 };
        let y = ny.clamp(Self::view(gh).0 + 4.0, gh - 36.0 - hgt - 14.0);
        quad(v, s, [x - 3.0, y - 3.0, w + 6.0, hgt + 18.0], [0.0, 0.0, 0.0, 0.75]);
        let c = if !open { if a.special { 0x808040 } else { 0x808080 } } else if a.special { 0xFFFF80 } else { 0xFFFFFF };
        text(v, s, x, y, a.name, c);
        for (k, l) in lines.iter().enumerate() {
            text(v, s, x, y + 12.0 + k as f32 * 10.0, l, if open { 0xA0A0A0 } else { 0x705050 });
        }
        if stats.has(i) {
            text(v, s, x, y + 14.0 + lines.len() as f32 * 10.0, "Taken!", 0x9090FF);
        }
    }

    /// `GuiStats` and its three `GuiSlotStats*`: rows clipped by the bars returned.
    fn draw_list(&self, v: &mut Vec<HudVertex>, s: f32, gw: f32, gh: f32, stats: &Stats) -> Vec<[f32; 4]> {
        let (rh, hdr, ids, top, bot) = self.rows(gh, stats);
        let hw = (gw / 2.0).floor();
        let n = if self.page == 1 { GENERAL.len() } else { ids.len() };
        if n == 0 {
            text(v, s, hw - tw("Nothing yet") / 2.0, top + hdr + 20.0, "Nothing yet", 0x909090);
        }
        for i in 0..n {
            let y = top + hdr - self.scroll + i as f32 * rh;
            if y + rh < top + hdr || y > bot {
                continue;
            }
            if i % 2 == 0 {
                quad(v, s, [hw - 156.0, y, 312.0, rh], [1.0, 1.0, 1.0, 0.05]);
            }
            if self.page == 1 {
                let (id, name, f) = GENERAL[i];
                let (c, val) = (if i % 2 == 0 { 0xFFFFFF } else { 0x909090 }, fmt(f, stats.get(id)));
                text(v, s, hw - 146.0, y + 3.0, name, c);
                text(v, s, hw + 146.0 - tw(&val), y + 3.0, &val, c);
            } else {
                let id = ids[i];
                quad(v, s, [hw - 152.0, y + 2.0, 18.0, 18.0], rgb(0x303030));
                stack(v, s, hw - 151.0, y + 3.0, id);
                text(v, s, hw - 130.0, y + 7.0, &item_name(id), 0xFFFFFF);
                for (c, k) in self.kinds().into_iter().enumerate() {
                    let val = if matches!(k, Kind::Mined) && !mineable(id) { "-".to_string() } else { fmt(Fmt::Plain, stats.count(k, id)) };
                    text(v, s, hw + COLS[c] - tw(&val), y + 7.0, &val, 0xE0E0E0);
                }
            }
        }
        vec![[0.0, 0.0, gw, top + hdr], [0.0, bot, gw, gh - bot]]
    }
}

/// `GuiAchievement`: the box sliding in at the top right for 3 s, "Achievement get!" or (a hint) just the description, held until stale.
#[derive(Default)]
pub struct Toast {
    cur: Option<(usize, bool, Instant)>,
}

impl Toast {
    pub fn taken(&mut self, a: usize) {
        self.cur = Some((a, false, Instant::now()));
    }

    /// `queueAchievementInformation`: call every tick while the hint applies; it vanishes half a second after the calls stop.
    pub fn hint(&mut self, a: usize) {
        if !self.cur.is_some_and(|c| !c.1) {
            self.cur = Some((a, true, Instant::now()));
        }
    }

    pub fn draw(&self, v: &mut Vec<HudVertex>, w: f32, h: f32, st: &Settings) {
        let Some((a, info, t)) = self.cur else { return };
        let e = t.elapsed().as_secs_f32();
        let up = if info {
            if e > 0.5 {
                return;
            }
            0.0
        } else if e > 3.0 {
            return;
        } else {
            let mut k = e / 3.0 * 2.0;
            if k > 1.0 {
                k = 2.0 - k;
            }
            let k = (1.0 - k * 4.0).max(0.0);
            k * k * k * k
        };
        let (s, gw, _) = scaled_gs(w, h, st.gui_scale);
        let (x, y) = (gw - 160.0, -up * 36.0);
        quad(v, s, [x, y, 160.0, 32.0], rgb(0x808080));
        quad(v, s, [x + 1.0, y + 1.0, 158.0, 30.0], rgb(0x1A1A1A));
        stack(v, s, x + 8.0, y + 8.0, ACHS[a].icon);
        if info {
            for (k, l) in wrap(ACHS[a].desc, 120.0).iter().enumerate() {
                text(v, s, x + 30.0, y + 7.0 + k as f32 * 10.0, l, 0xFFFFFF);
            }
        } else {
            text(v, s, x + 30.0, y + 7.0, "Achievement get!", 0xFFFF00);
            text(v, s, x + 30.0, y + 18.0, ACHS[a].name, 0xFFFFFF);
        }
    }
}
