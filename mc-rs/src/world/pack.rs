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
//! The screen IS the cloth, in the world: the camera glides to a pose looking down at it while it spreads (`Mat::rig`, `Mat::glide`),
//! `Mat::at` puts a cloth unit in the world, `Mat::touch` takes a finger back to one (so `slot_at` is still asked in units), and
//! `scene` is everything that lies on it, flat patches and item stacks, for `render::items`. Text and numbers stay HUD (`lib.rs`
//! projects them from `Mat::at`).
//! ponytail: the cloth is axis-aligned (spread along x or z, whichever the player faces more) and the crafting grid stays 2 x 2 (a workbench still opens its own 3 x 3).

use glam::{Mat4, Vec3};
use std::f32::consts::{PI, TAU};

use crate::world::craft::{self, Kind, SlotId};
use crate::world::gear;
use crate::world::items::{self, Inventory, ItemStack, MAIN};

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

fn inside(r: [f32; 4], x: f32, y: f32) -> bool {
    x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]
}

/// Which slot a touch at `(x, y)` cloth units is on (`Mat::touch` gives them): a cell, or the empty cloth of a zone (then the first
/// free main slot, so a stack held on the cursor is put down where it lies and shows in its own zone from the next frame).
pub fn slot_at(inv: &Inventory, x: f32, y: f32) -> Option<SlotId> {
    if let Some(c) = layout(inv).cells.iter().find(|c| inside([c.r[0] - PAD, c.r[1] - PAD, c.r[2] + 2.0 * PAD, c.r[3] + 2.0 * PAD], x, y)) {
        return Some(c.id);
    }
    ZONES.iter().any(|(_, r)| inside(*r, x, y)).then(|| (9..MAIN).find(|&i| inv.slots[i].is_none()).map(SlotId::Inv)).flatten()
}

/// The two touch toggles (0 shift-click, 1 place one), under the armor.
pub fn toggle(which: usize) -> [f32; 4] {
    [12.0 + 22.0 * which as f32, 164.0, 18.0, 18.0]
}

pub fn on_toggle(which: usize, x: f32, y: f32) -> bool {
    inside(toggle(which), x, y)
}

/// A touch on the cloth at all (outside it a held stack is thrown).
pub fn in_mat(x: f32, y: f32) -> bool {
    inside([0.0, 0.0, W, H], x, y)
}

// ---- The cloth in the world ----

/// Width of the cloth across, and its length along the way it is spread, in blocks. It has the layout's proportions (`W` x `H`), so a
/// cloth unit is a square `UNIT` blocks on a side (0.005: 2.0 x 1.16 blocks).
pub const WID: f32 = 2.0;
pub const UNIT: f32 = WID / W;
pub const LEN: f32 = H * UNIT;
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

// ---- The cloth seen from above ----

/// Heights above the ground, in blocks: the cloth's own top is 0.04, the flat patches go on it (`scene`), a stack rests on `REST`.
pub const REST: f32 = 0.052;
/// The plane a finger is taken to touch: between the flat sprites (`REST`) and the tops of the little cubes, so a tap on what is
/// seen lands within about 2 units of it from the framing pose.
pub const HIT_H: f32 = 0.09;
/// How high a stack held on the cursor floats (blocks), and how far above the finger (units) it hangs so the finger does not hide it.
pub const LIFT: f32 = 0.12;
pub const FLOAT: f32 = 24.0;
/// How much of its cell a block (a cube) and anything else (a flat sprite) fills.
const CUBE_FILL: f32 = 0.78;
const SPRITE_FILL: f32 = 0.95;
/// The stacks come up one by one once the cloth is spread: `STAGGER` seconds apart, each taking `POP` to grow.
pub const STAGGER: f32 = 0.025;
pub const POP: f32 = 0.2;

/// The view of the open cloth: from 78 degrees down, 60 degrees tall (narrower than the game's 70, so the near edge is less than a third
/// wider than the far one), the cloth filling `FIT` of the frame (the flat screen left 4% too).
pub const PITCH: f32 = 1.361_357;
pub const FOV: f32 = 1.047_198;
const FIT: f32 = 0.96;

/// A camera pose: what `FirstPersonCamera` holds of one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rig {
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub fov: f32,
}

/// `FirstPersonCamera::forward`.
fn forward(yaw: f32, pitch: f32) -> Vec3 {
    let ((sy, cy), (sp, cp)) = (yaw.sin_cos(), pitch.sin_cos());
    Vec3::new(sy * cp, -sp, -cy * cp)
}

/// The cloth's own frame for the framing: its centre at the origin, far = -Z, the camera at yaw 0, looking at the point `s` ahead of
/// the centre from `d` away.
fn local_vp(aspect: f32, s: f32, d: f32) -> Mat4 {
    let tgt = Vec3::new(0.0, 0.0, -s);
    Mat4::perspective_rh(FOV, aspect, 0.05, 64.0) * Mat4::look_at_rh(tgt - forward(0.0, PITCH) * d, tgt, Vec3::Y)
}

/// The four corners of the cloth in that frame, on its top.
fn corners() -> [Vec3; 4] {
    [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)].map(|(x, z)| Vec3::new(x * WID * 0.5, 0.0, z * LEN * 0.5))
}

/// A point on screen in -1..1, `None` behind the camera.
fn ndc(vp: &Mat4, c: Vec3) -> Option<(f32, f32)> {
    let p = *vp * c.extend(1.0);
    (p.w > 0.0).then(|| (p.x / p.w, p.y / p.w))
}

/// The nearest the camera can be, looking at the point `s` ahead of the centre, for every corner of the cloth to be within `FIT`.
fn fit_dist(aspect: f32, s: f32) -> f32 {
    let (mut lo, mut hi) = (0.1_f32, 20.0_f32);
    for _ in 0..20 {
        let mid = 0.5 * (lo + hi);
        let vp = local_vp(aspect, s, mid);
        if corners().iter().all(|&c| ndc(&vp, c).is_some_and(|(x, y)| x.abs() <= FIT && y.abs() <= FIT)) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi
}

impl Mat {
    /// `u`: the way the cloth is spread (away from the player), `r`: the player's right of it (`u` x up). On the screen `u` is up and
    /// `r` is right, so the layout's `y = 0` is the far edge and its quick-select row is the one nearest the player.
    pub fn axes(&self) -> (Vec3, Vec3) {
        let u = if self.along_x { Vec3::new(self.sign, 0.0, 0.0) } else { Vec3::new(0.0, 0.0, self.sign) };
        (u, u.cross(Vec3::Y))
    }

    /// Cloth unit `(x, y)`, `h` blocks above the ground the cloth lies on, as an offset from the cloth's centre (exact however far the
    /// world is from the origin).
    pub fn off(&self, x: f32, y: f32, h: f32) -> Vec3 {
        let (u, r) = self.axes();
        r * ((x / W - 0.5) * WID) + u * ((0.5 - y / H) * LEN) + Vec3::Y * h
    }

    /// The world point of cloth unit `(x, y)`, `h` blocks above the ground the cloth lies on.
    pub fn at(&self, x: f32, y: f32, h: f32) -> Vec3 {
        self.c + self.off(x, y, h)
    }

    /// The cloth unit of a point `d` from the cloth's centre (the inverse of `at`, whatever the height).
    fn unit_of(&self, d: Vec3) -> (f32, f32) {
        let (u, r) = self.axes();
        ((d.dot(r) / WID + 0.5) * W, (0.5 - d.dot(u) / LEN) * H)
    }

    /// The camera pose that frames the cloth: looking down at it from behind (the player's side), as near as keeps all of it within
    /// `FIT` of a frame of this `aspect`, and vertically centred (the look-at point slides along the cloth until the top and bottom
    /// margins match; a few rounds are plenty).
    pub fn rig(&self, aspect: f32) -> Rig {
        let mut s = 0.0_f32;
        for _ in 0..6 {
            let (d, ys) = {
                let d = fit_dist(aspect, s);
                let vp = local_vp(aspect, s, d);
                (d, corners().map(|c| ndc(&vp, c).map_or(0.0, |p| p.1)))
            };
            let mid = 0.5 * (ys.iter().copied().fold(f32::MIN, f32::max) + ys.iter().copied().fold(f32::MAX, f32::min));
            s += mid * d * (FOV * 0.5).tan() / PITCH.sin(); // the image slides about this much of a frame per block
        }
        let (u, _) = self.axes();
        let yaw = u.x.atan2(-u.z);
        let tgt = self.c + Vec3::Y * REST + u * s;
        Rig { pos: tgt - forward(yaw, PITCH) * fit_dist(aspect, s), yaw, pitch: PITCH, fov: FOV }
    }

    /// The camera while the cloth spreads or rolls up: from `from` (where the eye is) toward the framing pose, as far as the cloth is
    /// spread, so it arrives as the screen opens and goes back as the cloth rolls up.
    pub fn glide(&self, from: Rig, aspect: f32) -> Rig {
        let (to, p) = (self.rig(aspect), ease(self.t));
        let turn = (to.yaw - from.yaw + PI).rem_euclid(TAU) - PI;
        Rig { pos: from.pos.lerp(to.pos, p), yaw: from.yaw + turn * p, pitch: from.pitch + (to.pitch - from.pitch) * p, fov: from.fov + (to.fov - from.fov) * p }
    }

    /// The ray through a finger: `(x, y)` px of a `size` surface seen from `rig` (a pose of this `aspect`), as its origin relative to the
    /// cloth's centre and its direction.
    pub fn ray(&self, rig: &Rig, aspect: f32, size: (f32, f32), (x, y): (f32, f32)) -> (Vec3, Vec3) {
        let f = forward(rig.yaw, rig.pitch);
        let r = f.cross(Vec3::Y).normalize();
        let t = (rig.fov * 0.5).tan();
        let (nx, ny) = (2.0 * x / size.0 - 1.0, 1.0 - 2.0 * y / size.1);
        (rig.pos - self.c, (f + r * (nx * t * aspect) + r.cross(f) * (ny * t)).normalize())
    }

    /// The cloth unit a ray (`ray`) meets on the plane `h` blocks above the ground, `None` if it never does.
    fn plane(&self, (o, d): (Vec3, Vec3), h: f32) -> Option<(f32, f32)> {
        let k = (h - o.y) / d.y;
        (d.y < -1e-4 && k > 0.0).then(|| self.unit_of(o + d * k))
    }

    /// The cloth unit under a finger, on the plane `h` blocks up (for where a held stack hangs, the toggles and the edge of the cloth).
    pub fn touch(&self, rig: &Rig, aspect: f32, size: (f32, f32), at: (f32, f32), h: f32) -> Option<(f32, f32)> {
        self.plane(self.ray(rig, aspect, size, at), h)
    }
}

/// Where the ray `o + t d` first enters the box `lo..hi`, `t >= 0`.
fn hit_box(o: Vec3, d: Vec3, lo: Vec3, hi: Vec3) -> Option<f32> {
    let (a, b) = ((lo - o) / d, (hi - o) / d);
    let (t0, t1) = (a.min(b).max_element().max(0.0), a.max(b).min_element());
    (t1 >= t0).then_some(t0)
}

/// The slot a finger is over. The flat things (the zones, the places with a frame, a sprite lying on the cloth) are the plane `REST`:
/// the ray's cloth unit goes to `slot_at`. A cube stands up from it, and a plane at one height is off by the cube's height times the
/// cotangent of the ray's elevation (44 degrees at the far edge of the framing pose): some 15 units for a cube top, more than the gap
/// between two stacks; so a ray that enters a cube first (`lays`: what was drawn) is that cube's slot, whatever the plane says.
pub fn pick(inv: &Inventory, lays: &[Lay], m: &Mat, rig: &Rig, aspect: f32, size: (f32, f32), at: (f32, f32)) -> Option<SlotId> {
    let ray = m.ray(rig, aspect, size, at);
    let cube = lays.iter().filter_map(|l| {
        let id = l.id.filter(|_| l.cube())?;
        let ((cx, cy), s) = (l.centre(), l.side() * 0.5);
        let (a, b) = (m.off(cx - s, cy - s, l.base()), m.off(cx + s, cy + s, l.top()));
        hit_box(ray.0, ray.1, a.min(b), a.max(b)).map(|t| (t, id))
    });
    if let Some((_, id)) = cube.min_by(|a, b| a.0.total_cmp(&b.0)) {
        return Some(id);
    }
    let (x, y) = m.plane(ray, REST)?;
    slot_at(inv, x, y)
}

/// The blocks the inventory draws as their flat tile and not a cube: plants, torches, rails, doors, the bed ...
pub fn flat_icon(id: u8) -> bool {
    crate::world::chunk::cross_shape(id).is_some() || matches!(id, 26 | 27 | 28 | 50 | 51 | 55 | 63 | 64 | 65 | 66 | 68 | 69 | 71 | 75 | 76 | 77 | 90 | 92 | 93 | 94 | 96)
}

/// A stack that lies on the cloth as a small cube (a block with a cube's shape); anything else is a flat sprite.
pub fn is_cube(id: u16) -> bool {
    id < 256 && !flat_icon(id as u8)
}

/// One flat patch on the cloth: the rectangle `r` (units) from `lo` to `hi` blocks above the ground, in the flat colour of block
/// `tile.0` (for wool, `tile.1` is the dye).
#[derive(Clone, Copy, Debug)]
pub struct Flat {
    pub r: [f32; 4],
    pub lo: f32,
    pub hi: f32,
    pub tile: (u8, u8),
}

/// One stack on the cloth: in the cell `r` (units), `lift` blocks above where it rests, grown to `grow` (0..1) of its size.
#[derive(Clone, Copy, Debug)]
pub struct Lay {
    /// The slot it is from (`None`: the stack held on the cursor).
    pub id: Option<SlotId>,
    pub stack: ItemStack,
    pub r: [f32; 4],
    pub lift: f32,
    pub grow: f32,
}

impl Lay {
    pub fn cube(&self) -> bool {
        is_cube(self.stack.id)
    }

    pub fn centre(&self) -> (f32, f32) {
        (self.r[0] + self.r[2] * 0.5, self.r[1] + self.r[3] * 0.5)
    }

    /// Side of its footprint, units.
    pub fn side(&self) -> f32 {
        self.r[2] * if self.cube() { CUBE_FILL } else { SPRITE_FILL } * self.grow
    }

    /// Height of the cube (a slab is half of it), blocks; a sprite has none.
    pub fn height(&self) -> f32 {
        if self.cube() { self.side() * UNIT * if self.stack.id == 44 { 0.5 } else { 1.0 } } else { 0.0 }
    }

    /// Where its underside is: a hair over `REST` (so its shadow lies under it), lifted, and dropping in while it grows.
    pub fn base(&self) -> f32 {
        REST + 0.004 + self.lift + (1.0 - self.grow) * 0.04
    }

    /// Its highest point.
    pub fn top(&self) -> f32 {
        self.base() + self.height()
    }
}

/// Everything that lies on the open cloth, for `render::items` and the text `lib.rs` lays over it.
pub struct Scene {
    pub flats: Vec<Flat>,
    pub lays: Vec<Lay>,
}

/// The most of each there can be (4 grid + result + 4 worn + 9 quick + 27 main + the held stack; the patches, frames, toggles, the
/// selection and the arrow), and the boxes `render::items` keeps for the cloth: its two, the flats, and a stack with its shadow as two.
pub const MAX_FLATS: usize = 30;
pub const MAX_LAYS: usize = 46;
pub const BOXES: usize = 2 + MAX_FLATS + 2 * MAX_LAYS;

/// The patches and the stacks. `get` = what a slot holds (`Screen::get`), `hot` the selected quick slot, `(one, shift)` the two toggles,
/// `age` the seconds since the cloth finished spreading, `held` = the cursor stack, the finger's place (units) and how far it is lifted
/// (0..1).
pub fn scene(inv: &Inventory, get: &dyn Fn(SlotId) -> Option<ItemStack>, hot: usize, (one, shift): (bool, bool), age: f32, held: Option<(ItemStack, (f32, f32), f32)>) -> Scene {
    const PATCH: (u8, u8) = (24, 0); // sandstone: the zones, lighter than the cloth
    const FRAME: (u8, u8) = (35, 7); // dark grey wool: where the pack has a fixed place
    let layout = layout(inv);
    let mut flats = Vec::with_capacity(MAX_FLATS);
    for r in [CRAFT, WEAR, QUICK].into_iter().chain(ZONES.iter().map(|z| z.1)) {
        flats.push(Flat { r, lo: 0.04, hi: 0.046, tile: PATCH });
    }
    // The arrow from the 2 x 2 grid to its result.
    for r in [[64.0, 58.0, 9.0, 3.0], [71.0, 55.0, 3.0, 9.0]] {
        flats.push(Flat { r, lo: 0.046, hi: 0.049, tile: (35, 0) });
    }
    let fixed = |id: SlotId| matches!(id, SlotId::Grid(_) | SlotId::Result) || matches!(id, SlotId::Inv(i) if i < 9 || i >= MAIN);
    for c in &layout.cells {
        if c.id == SlotId::Inv(hot) {
            flats.push(Flat { r: [c.r[0] - 1.5, c.r[1] - 1.5, c.r[2] + 3.0, c.r[3] + 3.0], lo: 0.046, hi: 0.05, tile: (35, 4) }); // yellow ring
        }
        if fixed(c.id) {
            flats.push(Flat { r: c.r, lo: 0.046, hi: 0.052, tile: FRAME });
        }
    }
    // The two touch toggles under the armor: shift-click (green, 2) and place one (orange, 1), blue when off.
    for (which, on) in [(0_usize, shift), (1, one)] {
        let tile = match (on, which) {
            (false, _) => (35, 11),
            (true, 0) => (35, 5),
            (true, _) => (35, 1),
        };
        flats.push(Flat { r: toggle(which), lo: 0.046, hi: 0.052, tile });
    }
    let mut lays = Vec::with_capacity(MAX_LAYS);
    let mut n = 0.0;
    for c in &layout.cells {
        let Some(stack) = get(c.id) else { continue };
        let grow = ease((age - n * STAGGER) / POP);
        n += 1.0;
        if grow > 0.0 {
            lays.push(Lay { id: Some(c.id), stack, r: c.r, lift: 0.0, grow });
        }
    }
    if let Some((stack, (x, y), up)) = held {
        let side = 26.0;
        lays.push(Lay { id: None, stack, r: [x - side * 0.5, y - FLOAT - side * 0.5, side, side], lift: LIFT * ease(up), grow: 1.0 });
    }
    Scene { flats, lays }
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
        let mut inv = Inventory::default();
        inv.slots[12] = st(4);
        let mid = |r: [f32; 4]| (r[0] + r[2] * 0.5, r[1] + r[3] * 0.5);
        let cell = |id| layout(&inv).cells.iter().find(|c| c.id == id).unwrap().r;
        let (x, y) = mid(cell(SlotId::Inv(12)));
        assert_eq!(slot_at(&inv, x, y), Some(SlotId::Inv(12)));
        let (x, y) = mid(cell(SlotId::Inv(3)));
        assert_eq!(slot_at(&inv, x, y), Some(SlotId::Inv(3)), "quick select");
        let (x, y) = mid(cell(SlotId::Result));
        assert_eq!(slot_at(&inv, x, y), Some(SlotId::Result));
        let (x, y) = mid(ZONES[1].1); // an empty Food zone: the first free place
        assert_eq!(slot_at(&inv, x, y), Some(SlotId::Inv(9)));
        assert_eq!(slot_at(&inv, -2.0, -2.0), None);
        assert!(!in_mat(-2.0, -2.0) && in_mat(W / 2.0, H / 2.0));
    }

    // ---- the cloth in the world ----

    /// The cloth in all four ways it can be spread, far from the origin (a float's worst case for the maths below).
    fn mats() -> Vec<Mat> {
        [(false, 1.0), (false, -1.0), (true, 1.0), (true, -1.0)].map(|(along_x, sign)| Mat { c: Vec3::new(310.5, 64.0, -220.5), along_x, sign, t: 1.0, dir: 0 }).to_vec()
    }

    /// A frame's view-projection for a pose, as the renderer builds it (`FirstPersonCamera::build_view_proj`).
    fn view_proj(r: &Rig, aspect: f32) -> Mat4 {
        Mat4::perspective_rh(r.fov, aspect, 0.05, 64.0) * Mat4::look_at_rh(r.pos, r.pos + forward(r.yaw, r.pitch), Vec3::Y)
    }

    fn to_px(v: &Mat4, p: Vec3, (w, h): (f32, f32)) -> (f32, f32) {
        let n = v.project_point3(p);
        ((n.x + 1.0) * 0.5 * w, (1.0 - n.y) * 0.5 * h)
    }

    #[test]
    fn the_cloth_is_the_layouts_shape_and_lies_with_the_player_at_its_bottom() {
        assert!((LEN / WID - H / W).abs() < 1e-6 && (LEN - 1.16).abs() < 1e-5 && (UNIT - 0.005).abs() < 1e-9);
        let m = mats()[0]; // spread along +z, away from the player, whose right is -x
        assert!((m.at(0.0, 0.0, 0.0) - (m.c + Vec3::new(WID * 0.5, 0.0, LEN * 0.5))).length() < 1e-4, "far left");
        assert!((m.at(W, H, 0.1) - (m.c + Vec3::new(-WID * 0.5, 0.1, -LEN * 0.5))).length() < 1e-4, "near right");
        for m in mats() {
            let (x, y) = m.unit_of(m.off(123.0, 45.0, 0.07));
            assert!((x - 123.0).abs() < 1e-3 && (y - 45.0).abs() < 1e-3, "{m:?}");
        }
        // The same forward as the camera's, so a rig is a camera.
        let mut cam = crate::render::camera::FirstPersonCamera::spawn_at(0.0, 0.0, 0.0);
        (cam.yaw, cam.pitch) = (2.1, 0.7);
        assert!((cam.forward() - forward(2.1, 0.7)).length() < 1e-6);
    }

    #[test]
    fn the_camera_frames_the_cloth_from_its_players_side_and_centred() {
        for m in mats() {
            let (u, _) = m.axes();
            for aspect in [2.22_f32, 1.78, 1.33] {
                let rig = m.rig(aspect);
                let f = forward(rig.yaw, rig.pitch);
                assert!((Vec3::new(f.x, 0.0, f.z).normalize() - u).length() < 1e-4, "looks along the cloth, away from the player: {m:?}");
                assert!((rig.pos - m.c).dot(u) < 0.0 && rig.pos.y > m.c.y + 0.5, "behind and above it: {rig:?}");
                let v = view_proj(&rig, aspect);
                let p: Vec<(f32, f32)> = [(0.0, 0.0), (W, 0.0), (0.0, H), (W, H)].map(|(x, y)| { let n = v.project_point3(m.at(x, y, REST)); (n.x, n.y) }).to_vec();
                let (wide, tall) = (p.iter().fold(0.0_f32, |a, q| a.max(q.0.abs())), p.iter().fold(0.0_f32, |a, q| a.max(q.1.abs())));
                assert!(wide <= 0.97 && tall <= 0.97, "all of it in the frame: {wide} {tall} at {aspect}");
                assert!(wide >= 0.93 || tall >= 0.93, "and nearly filling it: {wide} {tall} at {aspect}");
                assert!((p[0].1 + p[3].1).abs() < 0.03 && p[0].0 < 0.0 && p[0].1 > 0.0 && p[3].0 > 0.0 && p[3].1 < 0.0, "far left is top left, centred: {p:?}");
            }
        }
    }

    #[test]
    fn the_camera_glides_with_the_cloth() {
        let mut m = mats()[0];
        let from = Rig { pos: m.c + Vec3::new(0.0, 1.62, -2.0), yaw: 7.0, pitch: 0.2, fov: 1.2 }; // yaw 7.0 is 0.717 of a turn past 2 pi
        let to = m.rig(2.0);
        m.t = 0.0;
        assert_eq!(m.glide(from, 2.0), from);
        m.t = 1.0;
        let end = m.glide(from, 2.0);
        assert!((end.pos - to.pos).length() < 1e-5 && (end.pitch - to.pitch).abs() < 1e-6 && (end.fov - to.fov).abs() < 1e-6);
        assert!(((end.yaw - to.yaw + PI).rem_euclid(TAU) - PI).abs() < 1e-4, "the same way, by the short turn");
        m.t = 0.5;
        let mid = m.glide(from, 2.0);
        assert!(mid.pos.y < from.pos.y && mid.pos.y > to.pos.y && mid.pitch > from.pitch && mid.pitch < to.pitch);
    }

    #[test]
    fn a_finger_lands_on_the_unit_it_is_over() {
        let (size, aspect) = ((2400.0_f32, 1080.0_f32), 2400.0 / 1080.0);
        for m in mats() {
            let rig = m.rig(aspect);
            let v = view_proj(&rig, aspect);
            for (x, y) in [(5.0, 5.0), (200.0, 116.0), (395.0, 227.0), (64.0, 189.0), (300.0, 30.0)] {
                for h in [0.0, REST, HIT_H, 0.15] {
                    let (tx, ty) = m.touch(&rig, aspect, size, to_px(&v, m.at(x, y, h), size), h).expect("the plane is in front");
                    assert!((tx - x).abs() < 0.2 && (ty - y).abs() < 0.2, "({x}, {y}) at {h} came back ({tx}, {ty}) for {m:?}");
                }
            }
            assert!(m.touch(&rig, aspect, size, (1200.0, -4000.0), HIT_H).is_none(), "a ray above the horizon meets no plane");
        }
    }

    /// The cubes stand up from the cloth, so the flat plane puts a tap on the far part of one a whole stack away; `pick` finds the cube.
    #[test]
    fn a_tap_on_a_cube_is_that_cube_wherever_on_the_cloth() {
        let (size, aspect) = ((2400.0_f32, 1080.0_f32), 2400.0 / 1080.0);
        let mut inv = Inventory::default();
        for (n, id) in [4_u16, 1, 3, 12, 13, 5, 17, 24].into_iter().enumerate() {
            inv.slots[9 + n] = Some(ItemStack { id, count: 3, damage: 0 }); // all blocks: Materials
        }
        inv.slots[17] = st(270); // and a pickaxe (a sprite)
        let get = |id: SlotId| match id {
            SlotId::Inv(i) => inv.slots[i],
            _ => None,
        };
        let sc = scene(&inv, &get, 0, (false, false), 10.0, None);
        for m in mats() {
            let rig = m.rig(aspect);
            let v = view_proj(&rig, aspect);
            let mut cubes = 0;
            for l in sc.lays.iter().filter(|l| l.id.is_some()) {
                let (cx, cy) = l.centre();
                let s = l.side() * 0.5;
                let mut spots = vec![(cx, cy, l.top())]; // the middle of the top, as seen
                if l.cube() {
                    spots.push((cx, cy + s * 0.98, l.base() + l.height() * 0.9)); // high on the face toward the player
                    cubes += 1;
                }
                for (x, y, h) in spots {
                    let p = to_px(&v, m.at(x, y, h), size);
                    assert_eq!(pick(&inv, &sc.lays, &m, &rig, aspect, size, p), l.id, "{:?} at ({x}, {y}, {h})", l.stack);
                }
            }
            assert_eq!(cubes, 8);
            // Off every zone, nothing.
            let p = to_px(&v, m.at(W - 2.0, H - 2.0, REST), size);
            assert_eq!(pick(&inv, &sc.lays, &m, &rig, aspect, size, p), None);
        }
    }

    #[test]
    fn the_scene_fits_its_budget_and_the_stacks_come_up_one_by_one() {
        let mut inv = Inventory::default();
        for i in 0..MAIN + 4 {
            inv.slots[i] = st(4 + (i as u16 % 7));
        }
        let full = |id: SlotId| match id {
            SlotId::Inv(i) => inv.slots[i],
            _ => st(1),
        };
        let held = Some((ItemStack { id: 268, count: 1, damage: 0 }, (200.0, 100.0), 1.0));
        let sc = scene(&inv, &full, 3, (true, true), 100.0, held);
        assert!(sc.flats.len() <= MAX_FLATS && sc.lays.len() <= MAX_LAYS, "{} flats, {} stacks", sc.flats.len(), sc.lays.len());
        assert_eq!((sc.flats.len(), sc.lays.len()), (MAX_FLATS, MAX_LAYS), "the fullest scene is the budget");
        assert!(sc.lays.iter().all(|l| l.grow == 1.0) && sc.lays.last().is_some_and(|l| l.id.is_none() && l.lift > 0.0), "all grown; the held one floats");
        assert!(sc.flats.iter().all(|f| f.hi > f.lo && f.lo >= 0.04), "above the cloth's top");
        let top = |sc: &Scene| sc.lays.iter().filter(|l| l.id.is_some()).map(|l| l.top()).fold(0.0_f32, f32::max);
        assert!(top(&sc) < 0.2, "a cube is a little thing");
        // Just spread: nothing yet; then one more every STAGGER; the first ones fully grown while the last has not begun.
        let n = |age: f32| scene(&inv, &full, 3, (false, false), age, None).lays.len();
        assert_eq!((n(0.0), n(STAGGER * 0.5), n(STAGGER * 1.5)), (0, 1, 2));
        assert_eq!(n(STAGGER * 44.0 + POP), 45);
        let sc = scene(&inv, &full, 3, (false, false), POP, None);
        assert!(sc.lays[0].grow == 1.0 && sc.lays[1].grow < 1.0 && sc.lays[1].base() > sc.lays[0].base(), "the next is still dropping in");
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
