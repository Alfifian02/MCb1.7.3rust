//! Immersive inventory, after The Forest: the pack is a cloth that has to be spread on flat ground before it can be opened.
//! NOT b1.7.3 (there the inventory is a panel that opens anywhere). A world switch, `pack` in the world's `info` file, turns it off
//! (the old panel) and the Create World screen sets it.
//!
//! Two halves, both pure (no GPU, no `lib.rs` state):
//! - `Mat`: the cloth in the world. `lay` checks the ground (a full block under every sample of the footprint, air above it),
//!   `tick` unrolls it (0.9 s) or rolls it up (0.7 s), `boxes` is what the entity pass draws, `valid` says it was left behind.
//! - The cloth as a screen: every item lies on it in the zone of its category (tools and gear, food, materials, other), each item at
//!   its own size, packed in rows and shrunk together when a zone is crowded. The crafting mat, the worn armor and the quick-select
//!   row (the hotbar) have their places. The slots stay `InventoryPlayer`'s: a zone is a sorted VIEW of main slots 9..36, so
//!   `Screen::click` and everything else of `craft.rs` works on the same `SlotId`s as the panel.
//! ponytail: the cloth is axis-aligned (spread along x or z, whichever the player faces more), the screen is the HUD's flat top-down
//! view of it and not the world seen in perspective, and the crafting grid stays 2 x 2 (a workbench still opens its own 3 x 3).

use glam::Vec3;

use crate::world::craft::{self, Kind, SlotId};
use crate::world::gear;
use crate::world::items::{self, Inventory, MAIN};

// ---- Categories and sizes ----

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cat {
    Tools,
    Food,
    Materials,
    Misc,
}

impl Cat {
    pub fn label(self) -> &'static str {
        match self {
            Cat::Tools => "Tools & gear",
            Cat::Food => "Food",
            Cat::Materials => "Materials",
            Cat::Misc => "Other",
        }
    }
}

/// The zone an item lies in: tools, weapons, armor, bow and arrows, shears, rod; anything that heals (and cake); blocks and raw
/// materials; and the rest (buckets, doors, sign, boat, carts, bed, records, eggs ...).
pub fn category(id: u16) -> Cat {
    if craft::tool(id).is_some() || craft::armor(id).is_some() || matches!(id, craft::SHEARS | craft::FLINT_AND_STEEL | gear::ROD | 261 | 262) {
        Cat::Tools
    } else if items::heal_amount(id).is_some() || id == 354 {
        Cat::Food
    } else if matches!(id, 321 | 323..=328 | 330 | 332 | 333 | 335 | 342..=345 | 347 | 355 | 356 | 2256 | 2257) {
        Cat::Misc
    } else {
        Cat::Materials
    }
}

/// Side of an item's cell in cloth units: a pickaxe is longer than a stick, a boat bigger than a coal, a chestplate than boots.
pub fn size(id: u16) -> f32 {
    if let Some((t, _)) = craft::armor(id) {
        return [28.0, 32.0, 30.0, 26.0][t]; // helmet, chestplate, leggings, boots
    }
    if let Some((k, _)) = craft::tool(id) {
        return match k {
            Kind::Sword | Kind::Pickaxe | Kind::Axe => 32.0,
            Kind::Shovel => 31.0,
            Kind::Hoe => 29.0,
        };
    }
    match id {
        261 | gear::ROD => 32.0,
        craft::SHEARS => 26.0,
        craft::FLINT_AND_STEEL => 24.0,
        262 => 18.0,
        328 | 333 | 342 | 343 | 355 => 34.0,
        321 | 323 | 324 | 330 => 28.0,
        325..=327 | 335 => 26.0,
        _ => match category(id) {
            Cat::Tools => 28.0,
            Cat::Food | Cat::Misc => 24.0,
            Cat::Materials if id < 256 => 22.0,
            Cat::Materials => 17.0,
        },
    }
}

// ---- The cloth as a screen (units: the cloth is `W` x `H`) ----

pub const W: f32 = 400.0;
pub const H: f32 = 232.0;
const GAP: f32 = 3.0;
/// Touch slop around a cell, in units (less than half the smallest gap, so neighbours never overlap).
const PAD: f32 = 0.4;

/// The crafting mat (2 x 2 grid, arrow, result) and the worn armor, left; the zones of the categories, right; the quick-select row below.
pub const CRAFT: [f32; 4] = [8.0, 24.0, 104.0, 86.0];
pub const WEAR: [f32; 4] = [8.0, 114.0, 104.0, 44.0];
pub const QUICK: [f32; 4] = [64.0, 189.0, 272.0, 32.0];
pub const ZONES: [(Cat, [f32; 4]); 4] = [
    (Cat::Tools, [118.0, 24.0, 140.0, 80.0]),
    (Cat::Food, [262.0, 24.0, 130.0, 80.0]),
    (Cat::Materials, [118.0, 108.0, 176.0, 78.0]),
    (Cat::Misc, [298.0, 108.0, 94.0, 78.0]),
];

/// The room items get in a zone (a label's height above, a margin around): x, y, w, h.
pub fn inner(r: [f32; 4]) -> [f32; 4] {
    [r[0] + 4.0, r[1] + 12.0, r[2] - 8.0, r[3] - 16.0]
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub id: SlotId,
    /// x, y, w, h in cloth units.
    pub r: [f32; 4],
}

pub struct Layout {
    pub cells: Vec<Cell>,
    /// How far each zone's items were shrunk (1 = full size), in `ZONES` order.
    pub scale: [f32; 4],
}

/// Shelf-pack `items` (slot, size) into `inner` in rows; if they do not fit, everything shrinks by a twentieth until it does (a
/// crowded zone shows smaller items, like a pile). Returns each item's top-left and the scale used.
fn flow(inner: [f32; 4], items: &[(usize, f32)]) -> (Vec<(f32, f32)>, f32) {
    let mut s = 1.0_f32;
    loop {
        let (mut x, mut y, mut row) = (0.0_f32, 0.0_f32, 0.0_f32);
        let mut pos = Vec::with_capacity(items.len());
        for &(_, sz) in items {
            let sz = sz * s;
            if x > 0.0 && x + sz > inner[2] {
                x = 0.0;
                y += row + GAP * s;
                row = 0.0;
            }
            pos.push((inner[0] + x, inner[1] + y));
            x += sz + GAP * s;
            row = row.max(sz);
        }
        if y + row <= inner[3] || s <= 0.3 {
            return (pos, s);
        }
        s -= 0.05;
    }
}

/// Every cell of the cloth for this inventory: the fixed ones (crafting grid and result, armor, quick select) and one per filled
/// main slot, laid in the zone of its category in slot order.
pub fn layout(inv: &Inventory) -> Layout {
    let mut cells = Vec::new();
    for (i, (x, y)) in [(14.0, 34.0), (40.0, 34.0), (14.0, 60.0), (40.0, 60.0)].into_iter().enumerate() {
        cells.push(Cell { id: SlotId::Grid(i), r: [x, y, 24.0, 24.0] });
    }
    cells.push(Cell { id: SlotId::Result, r: [78.0, 46.0, 28.0, 28.0] });
    for i in 0..4 {
        cells.push(Cell { id: SlotId::Inv(MAIN + 3 - i), r: [12.0 + 24.0 * i as f32, 124.0, 22.0, 22.0] }); // helmet first
    }
    for i in 0..9 {
        cells.push(Cell { id: SlotId::Inv(i), r: [QUICK[0] + 3.0 + 30.0 * i as f32, QUICK[1] + 3.0, 26.0, 26.0] });
    }
    let mut scale = [1.0; 4];
    for (z, (cat, r)) in ZONES.iter().enumerate() {
        let list: Vec<(usize, f32)> = (9..MAIN).filter_map(|i| inv.slots[i].filter(|s| category(s.id) == *cat).map(|s| (i, size(s.id)))).collect();
        let (pos, s) = flow(inner(*r), &list);
        scale[z] = s;
        for (&(i, sz), (x, y)) in list.iter().zip(pos) {
            cells.push(Cell { id: SlotId::Inv(i), r: [x, y, sz * s, sz * s] });
        }
    }
    Layout { cells, scale }
}

/// Cloth origin (px) and px per unit for a `w` x `h` surface.
pub fn panel(w: f32, h: f32) -> (f32, f32, f32) {
    let k = (w * 0.96 / W).min(h * 0.96 / H);
    ((w - W * k) * 0.5, (h - H * k) * 0.5, k)
}

/// A rectangle of the cloth in px.
pub fn px(w: f32, h: f32, r: [f32; 4]) -> [f32; 4] {
    let (ox, oy, k) = panel(w, h);
    [ox + r[0] * k, oy + r[1] * k, r[2] * k, r[3] * k]
}

fn inside(r: [f32; 4], x: f32, y: f32) -> bool {
    x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]
}

/// Which slot a touch at `(x, y)` px is on: a cell, or the empty cloth of a zone (then the first free main slot, so a stack held
/// on the cursor is put down where it lies and shows in its own zone from the next frame).
pub fn slot_at(inv: &Inventory, w: f32, h: f32, x: f32, y: f32) -> Option<SlotId> {
    let (ox, oy, k) = panel(w, h);
    let (x, y) = ((x - ox) / k, (y - oy) / k);
    if let Some(c) = layout(inv).cells.iter().find(|c| inside([c.r[0] - PAD, c.r[1] - PAD, c.r[2] + 2.0 * PAD, c.r[3] + 2.0 * PAD], x, y)) {
        return Some(c.id);
    }
    ZONES.iter().any(|(_, r)| inside(*r, x, y)).then(|| (9..MAIN).find(|&i| inv.slots[i].is_none()).map(SlotId::Inv)).flatten()
}

/// The two touch toggles (0 shift-click, 1 place one), under the armor.
pub fn toggle(which: usize) -> [f32; 4] {
    [12.0 + 22.0 * which as f32, 164.0, 18.0, 18.0]
}

pub fn on_toggle(which: usize, w: f32, h: f32, x: f32, y: f32) -> bool {
    inside(px(w, h, toggle(which)), x, y)
}

/// A touch on the cloth at all (outside it a held stack is thrown).
pub fn in_mat(w: f32, h: f32, x: f32, y: f32) -> bool {
    inside(px(w, h, [0.0, 0.0, W, H]), x, y)
}

// ---- The cloth in the world ----

/// Length of the cloth along the way it is spread, and its width across, in blocks.
pub const LEN: f32 = 1.6;
pub const WID: f32 = 2.0;
pub const SPREAD_SECS: f32 = 0.9;
pub const ROLL_SECS: f32 = 0.7;
/// Walk farther than this from it and it rolls itself up (blocks, from the eye).
pub const AWAY: f32 = 6.5;
/// Seconds the pack takes to come up into both hands.
pub const CARRY_SECS: f32 = 0.35;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Idle,
    /// Fully spread: the screen may open.
    Open,
    /// Fully rolled up: remove it.
    Gone,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat {
    /// Centre of the cloth on the ground (y = the top of the block it lies on).
    pub c: Vec3,
    /// Spread along x (else z), away from the player: `sign` is the direction on that axis.
    pub along_x: bool,
    pub sign: f32,
    /// 0 rolled .. 1 spread; `dir` 1 spreading, -1 rolling up, 0 spread and still.
    pub t: f32,
    pub dir: i8,
}

pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Mat {
    /// Nine block columns under the footprint (corners, edge middles, centre).
    fn samples(c: Vec3, along_x: bool) -> [(i32, i32); 9] {
        let (hx, hz) = if along_x { (LEN * 0.5, WID * 0.5) } else { (WID * 0.5, LEN * 0.5) };
        let mut v = [(0, 0); 9];
        for (n, (i, j)) in [(-1.0, -1.0), (0.0, -1.0), (1.0, -1.0), (-1.0, 0.0), (0.0, 0.0), (1.0, 0.0), (-1.0, 1.0), (0.0, 1.0), (1.0, 1.0)].into_iter().enumerate() {
            v[n] = ((c.x + i * hx * 0.98).floor() as i32, (c.z + j * hz * 0.98).floor() as i32);
        }
        v
    }

    /// The cloth as it would be spread on the top of the block hit at `point` (`top` = the y of the air cell above it), away from `eye`,
    /// not yet checked against the ground (`fits`).
    pub fn plan(point: Vec3, eye: Vec3, top: i32) -> Mat {
        let (dx, dz) = (point.x - eye.x, point.z - eye.z);
        let along_x = dx.abs() > dz.abs();
        let sign = if (if along_x { dx } else { dz }) < 0.0 { -1.0 } else { 1.0 };
        Mat { c: Vec3::new(point.x, top as f32, point.z), along_x, sign, t: 0.0, dir: 1 }
    }

    /// Flat ground: under all nine samples a full block (`floor`) with nothing (`air`) in the cell on it.
    pub fn fits(&self, air: &dyn Fn(i32, i32, i32) -> bool, floor: &dyn Fn(i32, i32, i32) -> bool) -> bool {
        let top = self.c.y as i32;
        Self::samples(self.c, self.along_x).iter().all(|&(x, z)| floor(x, top - 1, z) && air(x, top, z))
    }

    pub fn lay(point: Vec3, eye: Vec3, top: i32, air: &dyn Fn(i32, i32, i32) -> bool, floor: &dyn Fn(i32, i32, i32) -> bool) -> Option<Mat> {
        let m = Self::plan(point, eye, top);
        m.fits(air, floor).then_some(m)
    }

    /// The same cloth fully spread and still: the preview shown while the pack is carried.
    pub fn shown(self) -> Mat {
        Mat { t: 1.0, dir: 0, ..self }
    }

    /// Still worth keeping: near enough, and the ground under it is still there.
    pub fn valid(&self, eye: Vec3, floor: &dyn Fn(i32, i32, i32) -> bool) -> bool {
        let y = self.c.y as i32 - 1;
        (self.c - eye).length() < AWAY && Self::samples(self.c, self.along_x).iter().all(|&(x, z)| floor(x, y, z))
    }

    pub fn tick(&mut self, dt: f32) -> Step {
        match self.dir {
            1 => {
                self.t += dt / SPREAD_SECS;
                if self.t >= 1.0 {
                    (self.t, self.dir) = (1.0, 0);
                    return Step::Open;
                }
            }
            -1 => {
                self.t -= dt / ROLL_SECS;
                if self.t <= 0.0 {
                    return Step::Gone;
                }
            }
            _ => {}
        }
        Step::Idle
    }

    /// Boxes (min, max in world coordinates) to draw: the cloth from the near edge to the roll, and the roll that shrinks as it
    /// travels away from the player (gone when spread).
    pub fn boxes(&self) -> Vec<([f32; 3], [f32; 3])> {
        let p = ease(self.t);
        let (ux, uz) = if self.along_x { (self.sign, 0.0) } else { (0.0, self.sign) };
        let (wx, wz) = if self.along_x { (0.0, WID * 0.5) } else { (WID * 0.5, 0.0) };
        let y = self.c.y;
        let near = (self.c.x - ux * LEN * 0.5, self.c.z - uz * LEN * 0.5);
        let lead = (near.0 + ux * LEN * p, near.1 + uz * LEN * p);
        let mut v = Vec::new();
        if p > 0.01 {
            let (a, b) = ([near.0 - wx, near.1 - wz], [lead.0 + wx, lead.1 + wz]);
            v.push(([a[0].min(b[0]), y + 0.01, a[1].min(b[1])], [a[0].max(b[0]), y + 0.04, a[1].max(b[1])]));
        }
        if p < 0.97 {
            let r = 0.14 * (1.0 - p) + 0.02;
            v.push(([lead.0 - r * ux.abs() - wx, y + 0.01, lead.1 - r * uz.abs() - wz], [lead.0 + r * ux.abs() + wx, y + 0.01 + 2.0 * r, lead.1 + r * uz.abs() + wz]));
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::items::ItemStack;

    fn st(id: u16) -> Option<ItemStack> {
        Some(ItemStack { id, count: 1, damage: 0 })
    }

    #[test]
    fn items_have_a_zone_and_their_own_size() {
        assert_eq!([268, 270, 261, 262, 298].map(category), [Cat::Tools; 5]); // sword, pickaxe, bow, arrow, helmet
        assert_eq!([297, 320, 354].map(category), [Cat::Food; 3]);
        assert_eq!([325, 333, 355].map(category), [Cat::Misc; 3]);
        assert_eq!([4, 263, 280, 265].map(category), [Cat::Materials; 4]);
        assert!(size(268) > size(262) && size(333) > size(325) && size(4) > size(263) && size(299) > size(301), "sword > arrow, boat > bucket, cobble > coal, chestplate > leggings... boots");
    }

    /// Every filled main slot is on the cloth exactly once, inside its zone, and no two cells overlap, even with 27 stacks of one zone.
    #[test]
    fn the_cloth_never_overflows_or_overlaps() {
        let sets: [Vec<u16>; 4] = [(1..=27).collect(), (0..27).map(|i| [325, 333, 355, 323][i % 4]).collect(), (0..27).map(|i| [268, 298, 299, 261][i % 4]).collect(), (0..27).map(|i| [4, 297, 325, 270, 263][i % 5]).collect()];
        for ids in sets {
            let mut inv = Inventory::default();
            for (n, id) in ids.iter().enumerate() {
                inv.slots[9 + n] = st(*id);
            }
            inv.slots[0] = st(4);
            let lay = layout(&inv);
            for i in 0..MAIN {
                let n = lay.cells.iter().filter(|c| c.id == SlotId::Inv(i)).count();
                assert_eq!(n, usize::from(inv.slots[i].is_some() || i < 9), "slot {i} of {ids:?}");
            }
            for c in &lay.cells {
                assert!(c.r[0] >= 0.0 && c.r[1] >= 0.0 && c.r[0] + c.r[2] <= W && c.r[1] + c.r[3] <= H, "{c:?} off the cloth");
                match c.id {
                    SlotId::Inv(i) if (9..MAIN).contains(&i) => {
                        let (_, z) = ZONES.iter().find(|(cat, _)| *cat == category(inv.slots[i].unwrap().id)).unwrap();
                        let n = inner(*z);
                        assert!(c.r[0] >= n[0] - 0.01 && c.r[1] >= n[1] - 0.01 && c.r[0] + c.r[2] <= n[0] + n[2] + 0.01 && c.r[1] + c.r[3] <= n[1] + n[3] + 0.01, "{c:?} out of its zone");
                    }
                    _ => {}
                }
            }
            for (a, c) in lay.cells.iter().enumerate() {
                for d in &lay.cells[a + 1..] {
                    let hit = c.r[0] < d.r[0] + d.r[2] - 0.01 && d.r[0] < c.r[0] + c.r[2] - 0.01 && c.r[1] < d.r[1] + d.r[3] - 0.01 && d.r[1] < c.r[1] + c.r[3] - 0.01;
                    assert!(!hit, "{c:?} overlaps {d:?}");
                }
            }
        }
        // A few items keep their full size.
        let mut inv = Inventory::default();
        inv.slots[10] = st(270);
        assert_eq!(layout(&inv).scale[0], 1.0);
    }

    #[test]
    fn a_touch_finds_its_slot_or_the_empty_cloth() {
        let (w, h) = (2400.0, 1080.0);
        let mut inv = Inventory::default();
        inv.slots[12] = st(4);
        let mid = |r: [f32; 4]| {
            let p = px(w, h, r);
            (p[0] + p[2] * 0.5, p[1] + p[3] * 0.5)
        };
        let cell = |id| layout(&inv).cells.iter().find(|c| c.id == id).unwrap().r;
        let (x, y) = mid(cell(SlotId::Inv(12)));
        assert_eq!(slot_at(&inv, w, h, x, y), Some(SlotId::Inv(12)));
        let (x, y) = mid(cell(SlotId::Inv(3)));
        assert_eq!(slot_at(&inv, w, h, x, y), Some(SlotId::Inv(3)), "quick select");
        let (x, y) = mid(cell(SlotId::Result));
        assert_eq!(slot_at(&inv, w, h, x, y), Some(SlotId::Result));
        let (x, y) = mid(ZONES[1].1); // an empty Food zone: the first free place
        assert_eq!(slot_at(&inv, w, h, x, y), Some(SlotId::Inv(9)));
        assert_eq!(slot_at(&inv, w, h, 2.0, 2.0), None);
        assert!(!in_mat(w, h, 2.0, 2.0) && in_mat(w, h, w / 2.0, h / 2.0));
    }

    #[test]
    fn a_planned_cloth_fits_exactly_where_lay_accepts_it() {
        let floor = |_x: i32, y: i32, _z: i32| y == 63;
        let air = |x: i32, y: i32, z: i32| y >= 64 && (x, y, z) != (5, 64, 5);
        let eye = Vec3::new(3.5, 65.6, 0.5);
        for (p, ok) in [(Vec3::new(3.5, 64.0, 3.5), true), (Vec3::new(5.2, 64.0, 5.2), false)] {
            let m = Mat::plan(p, eye, 64);
            assert_eq!((m.fits(&air, &floor), Mat::lay(p, eye, 64, &air, &floor) == Some(m)), (ok, ok));
        }
        let shown = Mat::plan(Vec3::new(3.5, 64.0, 3.5), eye, 64).shown();
        assert_eq!((shown.boxes().len(), shown.dir), (1, 0), "the preview is the spread cloth alone");
    }

    #[test]
    fn the_cloth_needs_flat_clear_ground_unrolls_and_rolls_up() {
        let floor = |_x: i32, y: i32, _z: i32| y == 63;
        let air = |x: i32, y: i32, z: i32| y >= 64 && (x, y, z) != (5, 64, 5);
        let eye = Vec3::new(3.5, 65.6, 0.5);
        let mut m = Mat::lay(Vec3::new(3.5, 64.0, 3.5), eye, 64, &air, &floor).expect("flat and clear");
        assert!(!m.along_x && m.sign == 1.0, "spread away from the player, along z");
        assert!(Mat::lay(Vec3::new(5.2, 64.0, 5.2), eye, 64, &air, &floor).is_none(), "a block on the footprint");
        assert!(Mat::lay(Vec3::new(3.5, 64.0, 3.5), eye, 64, &air, &|_, _, _| false).is_none(), "no floor");
        assert!(m.valid(eye, &floor) && !m.valid(eye + Vec3::new(0.0, 0.0, 20.0), &floor) && !m.valid(eye, &|_, _, _| false));
        assert_eq!(m.boxes().len(), 1, "t = 0: only the roll");
        let mut n = 0;
        while m.tick(0.05) != Step::Open {
            n += 1;
            assert!(n < 100);
        }
        assert_eq!((m.t, m.dir, m.boxes().len()), (1.0, 0, 1), "spread: only the cloth");
        let cloth = m.boxes()[0];
        assert!((cloth.1[2] - cloth.0[2] - LEN).abs() < 1e-4 && (cloth.1[0] - cloth.0[0] - WID).abs() < 1e-4);
        m.dir = -1;
        while m.tick(0.05) != Step::Gone {
            n += 1;
            assert!(n < 200);
        }
        assert_eq!((ease(0.0), ease(1.0)), (0.0, 1.0));
    }
}
