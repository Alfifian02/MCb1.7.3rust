//! Wall decorations: paintings (`EntityPainting`, `EnumArt`, `ItemPainting`) and signs (`BlockSign`, `ItemSign`,
//! `TileEntitySign`, `GuiEditSign`). Pure rules and data; `lib.rs` wires them, `render/items.rs` draws them.
//! ponytail: the sign editor is an on-screen keyboard (no hardware keys on a phone), not `Keyboard.getEventCharacter`;
//! a painting is only its front (the 1/16 thick sides and the back are against the wall and not drawn).

use glam::Vec3;

use crate::world::chunk::{collisions, physics_id};
use crate::world::gen::noise::JavaRandom;
use crate::world::ticks::non_solid;

pub type Pos = (i32, i32, i32);
/// `Item.painting` / `Item.sign`.
pub const PAINTING: u16 = 321;
pub const SIGN: u16 = 323;
/// Block ids of the free-standing sign and the one on a wall.
pub const POST: u8 = 63;
pub const WALL: u8 = 68;
pub const MAX_PAINTINGS: usize = 64;

/// `EnumArt` in the enum's order: (title, sizeX, sizeY, offsetX, offsetY) in pixels of `art/kz.png`.
pub const ART: [(&str, u16, u16, u16, u16); 25] = [
    ("Kebab", 16, 16, 0, 0), ("Aztec", 16, 16, 16, 0), ("Alban", 16, 16, 32, 0), ("Aztec2", 16, 16, 48, 0),
    ("Bomb", 16, 16, 64, 0), ("Plant", 16, 16, 80, 0), ("Wasteland", 16, 16, 96, 0),
    ("Pool", 32, 16, 0, 32), ("Courbet", 32, 16, 32, 32), ("Sea", 32, 16, 64, 32), ("Sunset", 32, 16, 96, 32),
    ("Creebet", 32, 16, 128, 32), ("Wanderer", 16, 32, 0, 64), ("Graham", 16, 32, 16, 64),
    ("Match", 32, 32, 0, 128), ("Bust", 32, 32, 32, 128), ("Stage", 32, 32, 64, 128), ("Void", 32, 32, 96, 128),
    ("SkullAndRoses", 32, 32, 128, 128), ("Fighters", 64, 32, 0, 96), ("Pointer", 64, 64, 0, 192),
    ("Pigscene", 64, 64, 64, 192), ("BurningSkull", 64, 64, 128, 192), ("Skeleton", 64, 48, 192, 64),
    ("DonkeyKong", 64, 48, 192, 112),
];

/// `EntityPainting`: the block it hangs on, `direction` (0 faces -Z, 1 -X, 2 +Z, 3 +X) and the `ART` index.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Painting {
    pub pos: Pos,
    pub dir: u8,
    pub art: u8,
    /// `field_695_c`: counts up to 100, then it checks it still fits.
    pub age: u8,
}

/// The face of `ItemPainting.onItemUse` (Java side 2..=5) as a direction, `None` for the top and bottom.
pub fn dir_of_face(face: u8) -> Option<u8> {
    match face {
        2 => Some(0),
        4 => Some(1),
        3 => Some(2),
        5 => Some(3),
        _ => None,
    }
}

/// Outward normal and the viewer's right-hand direction of each `direction`.
const NORMAL: [(f32, f32); 4] = [(0.0, -1.0), (-1.0, 0.0), (0.0, 1.0), (1.0, 0.0)];
const RIGHT: [(f32, f32); 4] = [(-1.0, 0.0), (0.0, 1.0), (1.0, 0.0), (0.0, -1.0)];

impl Painting {
    fn size(&self) -> (f32, f32) {
        let a = ART[self.art as usize];
        (a.1 as f32, a.2 as f32)
    }

    /// `func_412_b`: the centre of the plate and its half extents (a 1/32 thick slab, `sizeX / 32` wide along the wall).
    pub fn frame(&self) -> (Vec3, Vec3) {
        let (sx, sy) = self.size();
        let (mut hx, hy, mut hz) = (sx, sy, sx);
        if self.dir != 0 && self.dir != 2 { hx = 0.5 } else { hz = 0.5 }
        let off = |n: f32| if n == 32.0 || n == 64.0 { 0.5 } else { 0.0 };
        let (x, y, z) = self.pos;
        let (mut cx, mut cz) = (x as f32 + 0.5, z as f32 + 0.5);
        let d = 9.0 / 16.0;
        match self.dir {
            0 => { cz -= d; cx -= off(sx) }
            1 => { cx -= d; cz += off(sx) }
            2 => { cz += d; cx += off(sx) }
            _ => { cx += d; cz -= off(sx) }
        }
        (Vec3::new(cx, y as f32 + 0.5 + off(sy), cz), Vec3::new(hx / 32.0, hy / 32.0, hz / 32.0))
    }

    /// The bounding box (`setBounds`, pulled in by 0.1/16 on every side).
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let (c, h) = self.frame();
        (c - h + Vec3::splat(0.1 / 16.0), c + h - Vec3::splat(0.1 / 16.0))
    }

    /// `EntityPainting.func_410_i`: no block collision box in the plate's box, a solid wall behind every 16 px piece, and no
    /// other painting in the way. `get` is a cell's (block id, metadata); `None` = not loaded, which answers `None`.
    pub fn fits(&self, get: &dyn Fn(i32, i32, i32) -> Option<(u8, u8)>, others: &[Painting]) -> Option<bool> {
        let (lo, hi) = self.bounds();
        for x in lo.x.floor() as i32..=hi.x.floor() as i32 {
            for y in lo.y.floor() as i32..=hi.y.floor() as i32 {
                for z in lo.z.floor() as i32..=hi.z.floor() as i32 {
                    let (id, meta) = get(x, y, z)?;
                    let (boxes, n) = collisions(physics_id(id, meta));
                    let hit = |b: &[f32; 6]| lo.x < x as f32 + b[3] && hi.x > x as f32 + b[0] && lo.y < y as f32 + b[4] && hi.y > y as f32 + b[1] && lo.z < z as f32 + b[5] && hi.z > z as f32 + b[2];
                    if boxes[..n].iter().any(hit) {
                        return Some(false);
                    }
                }
            }
        }
        let (c, _) = self.frame();
        let (sx, sy) = self.size();
        let along_x = self.dir == 0 || self.dir == 2;
        let (x0, z0) = if along_x { ((c.x - sx / 32.0).floor() as i32, self.pos.2) } else { (self.pos.0, (c.z - sx / 32.0).floor() as i32) };
        let y0 = (c.y - sy / 32.0).floor() as i32;
        for i in 0..(sx as i32 / 16) {
            for j in 0..(sy as i32 / 16) {
                let (x, z) = if along_x { (x0 + i, z0) } else { (x0, z0 + i) };
                if non_solid(get(x, y0 + j, z)?.0) {
                    return Some(false);
                }
            }
        }
        Some(!others.iter().any(|o| {
            let (a, b) = o.bounds();
            hi.x > a.x && lo.x < b.x && hi.y > a.y && lo.y < b.y && hi.z > a.z && lo.z < b.z
        }))
    }

    /// The 16 px pieces the plate is drawn as: (`ART_TILE0` offset = png row * 16 + col, box min, box max). The outward
    /// face of each box (`face`) is the picture; `RenderPainting.func_159_a` draws the same grid.
    pub fn pieces(&self) -> Vec<(u16, [f32; 3], [f32; 3])> {
        let (c, _) = self.frame();
        let a = ART[self.art as usize];
        let (w, h) = ((a.1 / 16) as i32, (a.2 / 16) as i32);
        let (r, hv) = (RIGHT[self.dir as usize], 1.0 / 32.0);
        let half = Vec3::new(if r.0 != 0.0 { 0.5 } else { hv }, 0.5, if r.1 != 0.0 { 0.5 } else { hv });
        let mut out = Vec::new();
        for ky in 0..h {
            for kx in 0..w {
                let (or, up) = (kx as f32 + 0.5 - w as f32 / 2.0, h as f32 / 2.0 - ky as f32 - 0.5);
                let p = c + Vec3::new(r.0 * or, up, r.1 * or);
                let tile = (a.4 / 16 + ky as u16) * 16 + a.3 / 16 + kx as u16;
                out.push((tile, (p - half).to_array(), (p + half).to_array()));
            }
        }
        out
    }

    /// The face of `mesh::push_face` that points away from the wall (+X, -X, +Y, -Y, +Z, -Z).
    pub fn face(&self) -> usize {
        [5, 1, 4, 0][self.dir as usize]
    }

    /// The cell in front of the plate, whose light the painting is lit by.
    pub fn light_cell(&self) -> Pos {
        let (c, _) = self.frame();
        let n = NORMAL[self.dir as usize];
        ((c.x + n.0 * 0.5).floor() as i32, c.y.floor() as i32, (c.z + n.1 * 0.5).floor() as i32)
    }
}

/// `new EntityPainting(world, x, y, z, direction)`: every motive that fits goes in a list, one is drawn with `nextInt`.
/// `None` when none fits (`ItemPainting` then places nothing).
pub fn hang(pos: Pos, dir: u8, others: &[Painting], get: &dyn Fn(i32, i32, i32) -> Option<(u8, u8)>, rng: &mut JavaRandom) -> Option<Painting> {
    let at = |art| Painting { pos, dir, art, age: 0 };
    let fit: Vec<u8> = (0..ART.len() as u8).filter(|&a| at(a).fits(get, others) == Some(true)).collect();
    (!fit.is_empty()).then(|| at(fit[rng.next_int_bound(fit.len() as i32) as usize]))
}

/// `EntityPainting.onUpdate` for one 20 Hz tick: every 101st tick a painting that no longer fits is removed (the caller drops
/// the item). A painting whose wall is not loaded stays.
pub fn step(list: &mut Vec<Painting>, get: &dyn Fn(i32, i32, i32) -> Option<(u8, u8)>) -> Vec<Painting> {
    let mut gone = Vec::new();
    let mut i = 0;
    while i < list.len() {
        if list[i].age < 100 {
            list[i].age += 1;
        } else {
            list[i].age = 0;
            let others: Vec<Painting> = list.iter().enumerate().filter(|&(j, _)| j != i).map(|(_, o)| *o).collect();
            if list[i].fits(get, &others) == Some(false) {
                gone.push(list.remove(i));
                continue;
            }
        }
        i += 1;
    }
    gone
}

/// The painting under the crosshair within `reach`: the ray against the box grown by `getCollisionBorderSize` (0.1).
pub fn pick(list: &[Painting], eye: Vec3, dir: Vec3, reach: f32) -> Option<(usize, f32)> {
    list.iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let (lo, hi) = p.bounds();
            crate::world::mobs::ray_box(eye, dir, lo - Vec3::splat(0.1), hi + Vec3::splat(0.1)).filter(|&t| t <= reach).map(|t| (i, t))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

// ---- signs ----

/// `TileEntitySign.signText`: four lines of at most 15 characters.
pub type Text = [String; 4];
pub const MAX_LEN: usize = 15;

/// `ItemSign.onItemUse`: where the sign goes, its block id and metadata, or `None` when refused: not on a bottom face, the
/// block aimed at must be solid, the cell beside the face must be replaceable (`Block.canPlaceBlockAt`). A top face makes a
/// post turned to `floor((rotationYaw + 180) * 16 / 360 + 0.5) & 15`, a side face a wall sign with the face as metadata.
/// `get` is a block id (unloaded = air), `yaw` the Java `rotationYaw` in degrees.
pub fn sign_place(get: &dyn Fn(i32, i32, i32) -> u8, (x, y, z): Pos, face: u8, yaw: f32) -> Option<(Pos, u8, u8)> {
    if face == 0 || face > 5 || non_solid(get(x, y, z)) {
        return None;
    }
    let (dx, dy, dz) = [(0, 0, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)][face as usize];
    let at = (x + dx, y + dy, z + dz);
    if at.1 > 127 || !crate::world::pick::replaceable(get(at.0, at.1, at.2)) {
        return None;
    }
    Some(if face == 1 { (at, POST, (((yaw + 180.0) * 16.0 / 360.0 + 0.5).floor() as i32 & 15) as u8) } else { (at, WALL, face) })
}

/// A key of the sign editor's on-screen keyboard (`GuiEditSign.keyTyped`: characters, backspace, enter / down = next line).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Ch(u8),
    Shift,
    Space,
    Back,
    Enter,
    Done,
    /// Tap on a line of the board: edit that line.
    Line(u8),
}

/// The character rows, plain and shifted (all in `ChatAllowedCharacters`).
const KEYS: [[&str; 2]; 4] = [["1234567890", "!@#$%^&+()"], ["qwertyuiop", "QWERTYUIOP"], ["asdfghjkl-", "ASDFGHJKL_"], ["zxcvbnm,.'", "ZXCVBNM;!="]];

/// The line being edited and the shift state; the text itself lives with the sign.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Edit {
    pub pos: Pos,
    pub line: u8,
    pub shift: bool,
}

impl Edit {
    pub fn new(pos: Pos) -> Self {
        Self { pos, line: 0, shift: false }
    }

    /// A tapped key: returns true when the editor is done.
    pub fn press(&mut self, k: Key, text: &mut Text) -> bool {
        let line = &mut text[self.line as usize];
        match k {
            Key::Ch(c) if line.len() < MAX_LEN => line.push(c as char),
            Key::Space if line.len() < MAX_LEN => line.push(' '),
            Key::Back => {
                line.pop();
            }
            Key::Enter => self.line = (self.line + 1) & 3,
            Key::Line(l) => self.line = l,
            Key::Shift => self.shift = !self.shift,
            Key::Done => return true,
            _ => {}
        }
        false
    }
}

/// The editor screen for a `w` x `h` px surface in GUI units `u = h / 270`: (u, every key's rectangle x, y, w, h in px).
/// Lines 0..4 are the board's, then 4 rows of characters and the control row.
pub fn layout(w: f32, h: f32, shift: bool) -> (f32, Vec<(Key, [f32; 4])>) {
    let u = h / 270.0;
    let gw = w / u;
    let mut v = Vec::new();
    let (bw, lh) = (130.0, 14.0);
    for i in 0..4u8 {
        v.push((Key::Line(i), [(gw - bw) / 2.0 + 4.0, 34.0 + i as f32 * lh, bw - 8.0, lh]));
    }
    let pitch = ((gw - 24.0) / 10.0).min(48.0);
    let x0 = (gw - 10.0 * pitch) / 2.0;
    for (r, rows) in KEYS.iter().enumerate() {
        for (c, ch) in rows[shift as usize].bytes().enumerate() {
            v.push((Key::Ch(ch), [x0 + c as f32 * pitch, 104.0 + r as f32 * 33.0, pitch - 4.0, 30.0]));
        }
    }
    let mut x = x0;
    for (k, n) in [(Key::Shift, 1.5), (Key::Space, 5.0), (Key::Back, 1.5), (Key::Enter, 1.0), (Key::Done, 1.0)] {
        v.push((k, [x, 104.0 + 4.0 * 33.0, n * pitch - 4.0, 30.0]));
        x += n * pitch;
    }
    for (_, r) in v.iter_mut() {
        r.iter_mut().for_each(|c| *c *= u);
    }
    (u, v)
}

/// The key under (`x`, `y`) px.
pub fn key_at(w: f32, h: f32, shift: bool, x: f32, y: f32) -> Option<Key> {
    layout(w, h, shift).1.into_iter().find(|(_, r)| x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]).map(|(k, _)| k)
}

pub fn label(k: Key) -> String {
    match k {
        Key::Ch(c) => (c as char).to_string(),
        Key::Shift => "Shift".into(),
        Key::Back => "Del".into(),
        Key::Enter => "Next".into(),
        Key::Done => "Done".into(),
        Key::Space | Key::Line(_) => String::new(),
    }
}

// ---- save ----

const MAGIC: &[u8; 4] = b"MCD1";

/// The `decor` file next to `level`: paintings (x y z dir art) then signs (x y z, 4 lines of length + bytes). Its own file so
/// `Level`'s optional tails stay as they are.
pub fn encode(paintings: &[Painting], signs: &[(Pos, Text)]) -> Vec<u8> {
    let mut w = MAGIC.to_vec();
    w.extend((paintings.len() as u16).to_le_bytes());
    for p in paintings {
        [p.pos.0, p.pos.1, p.pos.2].iter().for_each(|c| w.extend(c.to_le_bytes()));
        w.extend([p.dir, p.art]);
    }
    w.extend((signs.len() as u16).to_le_bytes());
    for (p, t) in signs {
        [p.0, p.1, p.2].iter().for_each(|c| w.extend(c.to_le_bytes()));
        for l in t {
            w.push(l.len() as u8);
            w.extend(l.bytes());
        }
    }
    w
}

/// `None` for anything off (a cut file, a bad direction or motive, a character the font does not have, a line over 15).
pub fn decode(b: &[u8]) -> Option<(Vec<Painting>, Vec<(Pos, Text)>)> {
    let mut r = b.strip_prefix(MAGIC)?;
    fn take<'a>(r: &mut &'a [u8], n: usize) -> Option<&'a [u8]> {
        if r.len() < n {
            return None;
        }
        let (a, rest) = r.split_at(n);
        *r = rest;
        Some(a)
    }
    let i32_ = |r: &mut &[u8]| Some(i32::from_le_bytes(take(r, 4)?.try_into().ok()?));
    let u16_ = |r: &mut &[u8]| Some(u16::from_le_bytes(take(r, 2)?.try_into().ok()?));
    let mut paintings = Vec::new();
    for _ in 0..u16_(&mut r)? {
        let pos = (i32_(&mut r)?, i32_(&mut r)?, i32_(&mut r)?);
        let da = take(&mut r, 2)?;
        if da[0] > 3 || da[1] as usize >= ART.len() {
            return None;
        }
        paintings.push(Painting { pos, dir: da[0], art: da[1], age: 0 });
    }
    let mut signs = Vec::new();
    for _ in 0..u16_(&mut r)? {
        let pos = (i32_(&mut r)?, i32_(&mut r)?, i32_(&mut r)?);
        let mut t: Text = Default::default();
        for l in t.iter_mut() {
            let n = take(&mut r, 1)?[0] as usize;
            let s = take(&mut r, n)?;
            if n > MAX_LEN || !s.iter().all(|c| (32..127).contains(c)) {
                return None;
            }
            *l = String::from_utf8(s.to_vec()).ok()?;
        }
        signs.push((pos, t));
    }
    r.is_empty().then_some((paintings, signs))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stone wall at z = 10 (cells z >= 10 solid), air in front.
    fn wall(x: i32, y: i32, z: i32) -> Option<(u8, u8)> {
        Some((if z >= 10 && y >= 60 { 1 } else { 0 }, 0))
    }

    /// Painting geometry follows `func_412_b`, the placement rules `func_410_i`, and the pieces tile the plate.
    #[test]
    fn paintings_hang_on_walls_like_java() {
        // Kebab on the north face (dir 0) of the block (5, 64, 10): centre 9/16 out, plate 1/32 thick.
        let p = Painting { pos: (5, 64, 10), dir: 0, art: 0, age: 0 };
        let (c, h) = p.frame();
        assert_eq!((c.x, c.y, c.z), (5.5, 64.5, 10.5 - 9.0 / 16.0));
        assert_eq!((h.x, h.y, h.z), (0.5, 0.5, 1.0 / 64.0));
        assert_eq!(p.fits(&wall, &[]), Some(true));
        // A 32 wide motive is shifted half a block to -X and needs the wall behind both cells.
        let wide = Painting { art: 7, ..p };
        assert_eq!(wide.frame().0.x, 5.0);
        assert_eq!(wide.fits(&wall, &[]), Some(true));
        let hole = |x: i32, y: i32, z: i32| if (x, y, z) == (4, 64, 10) { Some((0, 0)) } else { wall(x, y, z) };
        assert_eq!(wide.fits(&hole, &[]), Some(false), "air behind one piece");
        assert_eq!(p.fits(&hole, &[]), Some(true), "the 16 px motive only needs its own cell");
        // A solid block in front, another painting, and an unloaded cell.
        let blocked = |x: i32, y: i32, z: i32| if (x, y, z) == (5, 64, 9) { Some((1, 0)) } else { wall(x, y, z) };
        assert_eq!(p.fits(&blocked, &[]), Some(false));
        assert_eq!(p.fits(&wall, &[p]), Some(false));
        assert_eq!(p.fits(&|_, _, _| None, &[]), None);
        // Every 16 px piece is one tile of the motive: Pool is 2 x 1 pieces at png (0, 32) = tiles 32 and 33 (row 2).
        let pool = Painting { art: 7, ..p };
        assert_eq!(pool.pieces().iter().map(|q| q.0).collect::<Vec<_>>(), vec![2 * 16, 2 * 16 + 1]);
        // Seen from the north the first piece is on the viewer's left = +X side.
        assert!(pool.pieces()[0].1[0] > pool.pieces()[1].1[0]);
        // The motive is drawn on the face away from the wall.
        assert_eq!((p.face(), Painting { dir: 3, ..p }.face()), (5, 0));
        // hang: only motives that fit; a 1 x 1 hole leaves the 16 px ones.
        let mut rng = JavaRandom::new(1);
        let one = |x: i32, y: i32, z: i32| if (4..=5).contains(&x) && y == 64 && z == 10 { wall(x, y, z) } else if z == 10 { Some((0, 0)) } else { wall(x, y, z) };
        let got = hang((5, 64, 10), 0, &[], &one, &mut rng).unwrap();
        assert!(ART[got.art as usize].2 == 16, "only motives one block high fit a one block high wall: {got:?}");
        assert!(hang((5, 64, 10), 0, &[], &|_, _, _| Some((0, 0)), &mut rng).is_none(), "no wall: no painting");
    }

    /// A painting that loses its wall drops on the 101st tick; the ray finds the plate.
    #[test]
    fn paintings_fall_off_and_are_picked() {
        let mut list = vec![Painting { pos: (5, 64, 10), dir: 0, art: 0, age: 0 }];
        for _ in 0..100 {
            assert!(step(&mut list, &|_, _, _| Some((0, 0))).is_empty());
        }
        assert_eq!(step(&mut list, &|_, _, _| Some((0, 0))).len(), 1, "no wall behind it");
        assert!(list.is_empty());
        let on = [Painting { pos: (5, 64, 10), dir: 0, art: 0, age: 0 }];
        let hit = pick(&on, Vec3::new(5.5, 64.5, 6.0), Vec3::Z, 4.0);
        assert!(hit.is_some_and(|(i, t)| i == 0 && (t - 3.4).abs() < 0.2), "{hit:?}");
        assert!(pick(&on, Vec3::new(8.5, 64.5, 6.0), Vec3::Z, 4.0).is_none());
        assert!(pick(&on, Vec3::new(5.5, 64.5, 6.0), Vec3::Z, 3.0).is_none(), "out of reach");
    }

    /// Sign placement (`ItemSign`), the editor keys (`GuiEditSign`) and the save codec.
    #[test]
    fn signs_place_edit_and_save() {
        let floor = |_: i32, y: i32, _: i32| if y < 64 { 1 } else { 0 };
        // On top of a block: a post turned toward the player (rotationYaw 0 = facing +Z, so the front looks back: 8).
        assert_eq!(sign_place(&floor, (3, 63, 3), 1, 0.0), Some(((3, 64, 3), POST, 8)));
        assert_eq!(sign_place(&floor, (3, 63, 3), 1, 90.0), Some(((3, 64, 3), POST, 12)));
        assert_eq!(sign_place(&floor, (3, 63, 3), 0, 0.0), None, "never on a bottom face");
        assert_eq!(sign_place(&floor, (3, 64, 3), 1, 0.0), None, "air is not solid");
        let side = |x: i32, y: i32, _: i32| if x <= 3 && y < 70 { 1 } else { 0 };
        assert_eq!(sign_place(&side, (3, 65, 3), 5, 0.0), Some(((4, 65, 3), WALL, 5)));
        assert_eq!(sign_place(&|_, _, _| 1, (3, 65, 3), 5, 0.0), None, "the cell must be free");
        let mut t: Text = Default::default();
        let mut e = Edit::new((3, 64, 3));
        for c in b"hello world 12345" {
            e.press(Key::Ch(*c), &mut t);
        }
        assert_eq!(t[0], "hello world 123", "15 characters at most");
        e.press(Key::Back, &mut t);
        assert_eq!(t[0], "hello world 12");
        e.press(Key::Enter, &mut t);
        e.press(Key::Space, &mut t);
        e.press(Key::Line(3), &mut t);
        assert_eq!((t[1].as_str(), e.line), (" ", 3));
        e.press(Key::Enter, &mut t);
        assert_eq!(e.line, 0, "the line wraps like `& 3`");
        assert!(e.press(Key::Done, &mut t) && !e.press(Key::Shift, &mut t) && e.shift);
        // Keyboard: every key is on the screen, none overlap, and a letter sits where its rectangle is.
        let (w, h) = (2400.0, 1080.0);
        let (_, keys) = layout(w, h, false);
        assert_eq!(keys.len(), 4 + 40 + 5);
        assert!(keys.iter().all(|(_, r)| r[0] >= 0.0 && r[0] + r[2] <= w && r[1] + r[3] <= h));
        let q = keys.iter().find(|(k, _)| *k == Key::Ch(b'q')).unwrap().1;
        assert_eq!(key_at(w, h, false, q[0] + 1.0, q[1] + 1.0), Some(Key::Ch(b'q')));
        assert_eq!(key_at(w, h, true, q[0] + 1.0, q[1] + 1.0), Some(Key::Ch(b'Q')));
        // Save round trip; every corrupt form is refused.
        let ps = vec![Painting { pos: (1, 2, -3), dir: 3, art: 24, age: 0 }];
        let ss = vec![((4, 70, 5), ["hi".to_string(), String::new(), "a b c".into(), "x".into()])];
        let enc = encode(&ps, &ss);
        assert_eq!(decode(&enc), Some((ps, ss)));
        assert!(decode(&enc[..enc.len() - 1]).is_none() && decode(b"nope").is_none());
        assert!(decode(&[&enc[..], &[0]].concat()).is_none());
        let mut bad = encode(&[Painting { pos: (0, 0, 0), dir: 4, art: 0, age: 0 }], &[]);
        assert!(decode(&bad).is_none());
        bad = encode(&[], &[((0, 0, 0), ["\u{7f}".into(), String::new(), String::new(), String::new()])]);
        assert!(decode(&bad).is_none());
    }
}
