//! The geometry of the REI overlay, with no game types: `ScreenOverlayImpl` / `EntryListWidget` / `PaginatedEntryListWidget` of REI 26.2 on a touch screen.
//! All rects are `[x, y, w, h]` in GUI units (`menu::scaled_gs`), so this file is testable on its own (see the tests at the bottom).
//!
//! REI keeps the overlay in the space the open screen leaves: the entry list right of it, the favorites left of it. When that space is too narrow the
//! screen is pushed sideways (`shift_target`) and `ease_to` moves it there over a few frames.

/// One entry cell (REI's `entrySize` 18, 20 here so a fingertip hits one).
pub const CELL: f32 = 20.0;
/// A button's side: REI's 16 / 20, 20 here.
pub const BTN: f32 = 20.0;
/// The most columns / rows of one page.
pub const MAX_COLS: usize = 14;
pub const MAX_ROWS: usize = 12;
/// The entry list wants this much width (7 columns and the margins) before the screen is pushed aside.
pub const WANT_RIGHT: f32 = 7.0 * CELL + 6.0;
/// A left strip narrower than this is not drawn: the favorites move to a tab of the entry list.
pub const MIN_LEFT: f32 = 2.0 * CELL + 6.0;
/// The bottom row of the right strip: the search field and the filter / config buttons (REI `BOTTOM_SIDE`).
pub const BOTTOM_ROW: f32 = BTN + 4.0;
/// What the bottom left keeps free for the container's own toggles and the view's buttons.
pub const BOTTOM_LEFT: f32 = 36.0;

pub fn inside(r: [f32; 4], x: f32, y: f32) -> bool {
    x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]
}

pub fn overlaps(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
}

/// How far (GUI units, negative = left) to push a panel `[x, y, w, h]` so the strip on its right is `want` wide, never past `edge` from the screen's left.
pub fn shift_target(gw: f32, panel: [f32; 4], want: f32, edge: f32) -> f32 {
    let room = gw - 2.0 - (panel[0] + panel[2] + 2.0);
    let need = want - room;
    if need <= 0.0 {
        return 0.0;
    }
    -need.min((panel[0] - edge).max(0.0))
}

/// `cur` moved towards `target` over `dt` seconds, exponentially (about 95% there after 0.2 s), snapped when within a quarter pixel.
pub fn ease_to(cur: f32, target: f32, dt: f32) -> f32 {
    let d = target - cur;
    if d.abs() < 0.25 {
        return target;
    }
    cur + d * (1.0 - (-dt.clamp(0.0, 0.1) * 15.0).exp())
}

/// 0 to 1 over `secs`, decelerating (cubic).
pub fn ease_out(elapsed: f32, secs: f32) -> f32 {
    let t = (elapsed / secs).clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t)
}

/// One column of the overlay: a page row on top (arrows and the label), the entry grid under it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strip {
    pub area: [f32; 4],
    pub prev: [f32; 4],
    pub next: [f32; 4],
    pub label: [f32; 4],
    pub gx: f32,
    pub gy: f32,
    pub cols: usize,
    pub rows: usize,
}

impl Strip {
    pub fn per_page(&self) -> usize {
        self.cols * self.rows
    }
    /// The cell under (x, y), as an index into the page (`None` outside the grid).
    pub fn cell_at(&self, x: f32, y: f32) -> Option<usize> {
        let (c, r) = (((x - self.gx) / CELL).floor(), ((y - self.gy) / CELL).floor());
        if c < 0.0 || r < 0.0 || c >= self.cols as f32 || r >= self.rows as f32 {
            return None;
        }
        Some(r as usize * self.cols + c as usize)
    }
    /// The rect of the page's cell `i`.
    pub fn cell_rect(&self, i: usize) -> [f32; 4] {
        [self.gx + (i % self.cols) as f32 * CELL, self.gy + (i / self.cols) as f32 * CELL, CELL, CELL]
    }
    /// The strip slid `dx` sideways (the overlay's slide-in).
    pub fn moved(mut self, dx: f32) -> Strip {
        for r in [&mut self.area, &mut self.prev, &mut self.next, &mut self.label] {
            r[0] += dx;
        }
        self.gx += dx;
        self
    }
    /// The grid's rect.
    pub fn grid(&self) -> [f32; 4] {
        [self.gx, self.gy, self.cols as f32 * CELL, self.rows as f32 * CELL]
    }
}

/// The strip in `area`; `hud` is the touch HUD's pause / inventory buttons (the page row stops short of them, the grid starts under them); `bottom` keeps the
/// bottom of the area free. `None` when not even one column fits.
pub fn strip(area: [f32; 4], hud: Option<[f32; 4]>, bottom: f32) -> Option<Strip> {
    let (x0, x1) = (area[0], area[0] + area[2]);
    let cols = (((x1 - x0 - 2.0) / CELL).floor() as usize).min(MAX_COLS);
    if cols == 0 {
        return None;
    }
    let row = [x0, 5.0, x1 - x0, BTN];
    let mut limit = x1;
    let mut gy = 5.0 + BTN + 3.0;
    if let Some(h) = hud.filter(|h| overlaps(*h, area)) {
        if overlaps(h, row) {
            limit = (h[0] - 2.0).max(x0 + 2.0 * BTN + 12.0);
        }
        gy = gy.max(h[1] + h[3] + 2.0);
    }
    let bottom_y = area[1] + area[3] - bottom;
    let rows = (((bottom_y - gy) / CELL).floor().max(1.0) as usize).min(MAX_ROWS);
    let gx = x0 + ((x1 - x0 - cols as f32 * CELL) / 2.0).floor();
    Some(Strip {
        area,
        prev: [x0, 5.0, BTN, BTN],
        next: [limit - BTN, 5.0, BTN, BTN],
        label: [x0 + BTN + 2.0, 5.0, limit - x0 - 2.0 * BTN - 4.0, BTN],
        gx,
        gy,
        cols,
        rows,
    })
}

/// The overlay around a panel (GUI units, already pushed): the entry list on the right and the favorites on the left (`None` if it does not fit).
pub fn around(gw: f32, gh: f32, panel: [f32; 4], hud: Option<[f32; 4]>) -> (Option<Strip>, Option<Strip>) {
    let rx = panel[0] + panel[2] + 2.0;
    let right = strip([rx, 0.0, gw - 2.0 - rx, gh], hud, BOTTOM_ROW + 4.0);
    let lw = panel[0] - 4.0;
    let left = if lw >= MIN_LEFT { strip([2.0, 0.0, lw, gh], hud, BOTTOM_LEFT) } else { None };
    (right, left)
}

/// The bottom row of the right strip: (search field, craftable filter button, config button).
pub fn bottom_row(area: [f32; 4], gh: f32) -> ([f32; 4], [f32; 4], [f32; 4]) {
    let (x0, x1, y) = (area[0], area[0] + area[2], gh - BTN - 4.0);
    let config = [x1 - BTN, y, BTN, BTN];
    let filter = [config[0] - BTN - 2.0, y, BTN, BTN];
    let search = [x0 + 1.0, y, (filter[0] - 3.0 - (x0 + 1.0)).max(BTN), BTN];
    (search, filter, config)
}

/// How many pages `n` entries take at `per` a page (at least one).
pub fn pages(n: usize, per: usize) -> usize {
    n.div_ceil(per.max(1)).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_screen_needs_no_push() {
        // 600 x 270 (a phone at scale 4): a 256 wide panel in the middle leaves 170 each side.
        assert_eq!(shift_target(600.0, [172.0, 6.0, 256.0, 258.0], WANT_RIGHT, 2.0), 0.0);
    }

    #[test]
    fn a_narrow_screen_pushes_left_just_enough() {
        // 480 wide, panel 256 centred: 112 left, 112 right; the list wants 146.
        let p = [112.0, 6.0, 256.0, 258.0];
        let s = shift_target(480.0, p, WANT_RIGHT, 2.0);
        assert!((s + 38.0).abs() < 0.01 + 2.0, "pushed by what is missing: {s}");
        let pushed = [p[0] + s, p[1], p[2], p[3]];
        let (right, left) = around(480.0, 270.0, pushed, None);
        assert!(right.unwrap().cols >= 7, "the list got its 7 columns");
        assert!(left.is_some(), "and the favorites still fit on the left");
    }

    #[test]
    fn the_push_stops_at_the_screen_edge() {
        let s = shift_target(300.0, [20.0, 0.0, 256.0, 200.0], WANT_RIGHT, 2.0);
        assert_eq!(s, -18.0);
        assert_eq!(shift_target(300.0, [0.0, 0.0, 256.0, 200.0], WANT_RIGHT, 2.0), 0.0, "a panel at the edge cannot move");
    }

    #[test]
    fn easing_arrives_and_never_overshoots() {
        let mut x = 0.0;
        for _ in 0..60 {
            x = ease_to(x, -40.0, 1.0 / 60.0);
            assert!(x >= -40.0 && x <= 0.0);
        }
        assert_eq!(x, -40.0, "one second at 60 fps is plenty");
        assert_eq!(ease_to(5.0, 5.1, 0.016), 5.1, "a tiny gap snaps");
        let once = ease_to(0.0, 10.0, 5.0);
        assert!(once < 10.0, "a long frame is clamped, no overshoot: {once}");
        assert_eq!((ease_out(0.0, 0.2), ease_out(1.0, 0.2)), (0.0, 1.0));
        assert!(ease_out(0.05, 0.2) > 0.25 * (0.05 / 0.2), "decelerating: fast first");
    }

    #[test]
    fn the_grid_starts_under_the_hud_buttons() {
        let hud = [520.0, 10.0, 70.0, 30.0];
        let s = strip([430.0, 0.0, 168.0, 270.0], Some(hud), 28.0).unwrap();
        assert!(s.gy >= 42.0, "under the pause button: {}", s.gy);
        assert!(s.next[0] + s.next[2] <= hud[0], "the page row stops before the buttons");
        assert!(s.label[2] > 20.0);
        let free = strip([430.0, 0.0, 168.0, 270.0], None, 28.0).unwrap();
        assert!(free.gy < s.gy && free.rows >= s.rows);
        assert_eq!(free.cols, 8);
    }

    #[test]
    fn cells_map_both_ways() {
        let s = strip([100.0, 0.0, 100.0, 200.0], None, 28.0).unwrap();
        for i in 0..s.per_page() {
            let r = s.cell_rect(i);
            assert_eq!(s.cell_at(r[0] + 1.0, r[1] + 1.0), Some(i));
            assert_eq!(s.cell_at(r[0] + CELL - 0.1, r[1] + CELL - 0.1), Some(i));
        }
        assert_eq!(s.cell_at(s.gx - 1.0, s.gy), None);
        assert_eq!(s.cell_at(s.gx, s.gy + s.rows as f32 * CELL), None);
        assert_eq!(strip([0.0, 0.0, 22.0, 200.0], None, 0.0).map(|s| s.cols), Some(1), "one column needs the cell and a margin of 2");
        assert!(strip([0.0, 0.0, 21.0, 200.0], None, 0.0).is_none());
    }

    #[test]
    fn the_bottom_row_fits_its_three_parts() {
        let (search, filter, config) = bottom_row([430.0, 0.0, 168.0, 270.0], 270.0);
        assert!(!overlaps(search, filter) && !overlaps(filter, config));
        assert!(config[0] + config[2] <= 598.0 + 0.01 && search[2] >= BTN);
        assert_eq!(pages(0, 80), 1);
        assert_eq!(pages(81, 80), 2);
    }

    #[test]
    fn a_narrow_left_strip_is_dropped() {
        let (right, left) = around(480.0, 270.0, [40.0, 0.0, 256.0, 270.0], None);
        assert!(right.is_some() && left.is_none());
    }
}
