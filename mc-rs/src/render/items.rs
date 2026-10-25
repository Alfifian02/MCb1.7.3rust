//! Dropped items: a 0.25 cube per `ItemEntity` in the flat colour of its item, hovering and bobbing like
//! `RenderItem` (`sin(age / 10) * 0.1 + 0.1`), drawn with the chunk pipeline from one dynamic buffer pair.
//! ponytail: no spin (needs rotated boxes), flat full brightness, no 2nd/3rd copy for big stacks; M14 draws
//! real item sprites.

use crate::gpu::gfx::{Buf, Gfx};
use crate::render::atlas;
use crate::render::mesh::{lm, push_box, push_face, VF, WHITE};
use crate::world::boat::Boat;
use crate::world::minecart::Cart;
use crate::world::decor::{self, Painting, Text};
use crate::world::chunk::brightness;
use crate::world::effects::{Effects, Fx, MAX_BOLTS, MAX_PARTICLES};
use glam::{Mat4, Vec3};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::world::items::{placed_meta, stack_tile, Drops, ItemStack, MAX_ITEMS};
use crate::world::pack::{self, Lay, Mat, Scene};
use crate::world::mobs::{Kind, Kind::*, Mob, Mobs, MAX_ARROWS, MAX_MOBS};
use crate::world::ticks::{Falling, MAX_FALLING};

/// 24 vertices x 7 floats (`mesh::VF`) per cube, 36 indices per cube.
/// A mob is at most 12 boxes (spider 11), an arrow one, a falling block one.
/// Boats drawn at once (5 boxes each).
const MAX_BOATS: usize = 16;
/// Carts drawn at once (floor, four walls and the furnace: 6 boxes each); the pack's cloth and all that lies on it are `pack::BOXES` more.
const MAX_CARTS: usize = 16;
const BOXES: usize = MAX_ITEMS * 2 + MAX_MOBS * 12 + MAX_ARROWS + MAX_FALLING + MAX_PARTICLES + MAX_BOLTS + MAX_BOATS * 5 + MAX_CARTS * 6 + pack::BOXES + 1;
/// Paintings (16 px pieces, one face each), signs (a board and a post of 6 two-sided faces) and the sign text (one quad per run
/// of lit font pixels), on top of the boxes.
const MAX_SIGNS: usize = 32;
const PIECES: usize = 512;
const TEXT_QUADS: usize = 4096;
// The player: 42 quads of 4 vertices, both windings (12 indices) each, and an item of at most 2 boxes; or the first-person hand, an
// item sprite of 66 quads (two faces and 64 edge strips) in both windings.
const EXTRA_V: usize = PIECES * 4 + MAX_SIGNS * 2 * 24 + TEXT_QUADS * 4 + 320;
const EXTRA_I: usize = PIECES * 6 + MAX_SIGNS * 2 * 72 + TEXT_QUADS * 6 + 800;
const VERTEX_BYTES: u64 = ((BOXES * 24 + EXTRA_V) * crate::render::mesh::VF * 4) as u64;
const INDEX_BYTES: u64 = ((BOXES * 36 + EXTRA_I) * 4) as u64;

/// A sign to draw: its cell, block id (63 post, 68 wall), metadata and text.
pub struct SignView<'a> {
    pub pos: (i32, i32, i32),
    pub id: u8,
    pub meta: u8,
    pub text: &'a Text,
}

pub struct ItemMesh {
    pub vbuf: Buf,
    pub ibuf: Buf,
    /// The sign text kept between frames (`SignCache`); a `Mutex` only so `update` keeps `&self`.
    signs: Mutex<SignCache>,
}

impl ItemMesh {
    pub fn new(gfx: Gfx) -> Self {
        Self { vbuf: gfx.writable(VERTEX_BYTES, false), ibuf: gfx.writable(INDEX_BYTES, true), signs: Mutex::default() }
    }

    /// Signs whose text is kept (the `FPS:` log line).
    pub fn cached_signs(&self) -> usize {
        self.signs.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Upload this frame's items; returns the index count to draw.
    /// `lv` = raw (sky, block) light of a cell: every entity is lit by its own cell.
    pub fn update(&self, gfx: Gfx, drops: &Drops, mobs: &Mobs, falling: &[Falling], fx: &Effects, lv: &dyn Fn(i32, i32, i32) -> (u8, u8), hook: Option<Vec3>, paintings: &[Painting], signs: &[SignView], boats: (&[Boat], f32), carts: (&[Cart], f32), player: Option<&crate::render::player::View>, hand: Option<&crate::render::player::Hand>, pack: Option<PackDraw>, eye: Vec3, seen: &dyn Fn(Vec3, f32) -> bool) -> u32 {
        let (verts, idxs) = geometry(drops, mobs, falling, fx, lv, hook, paintings, signs, boats, carts, player, hand, pack, eye, seen, &mut self.signs.lock().unwrap_or_else(|e| e.into_inner()));
        if idxs.is_empty() {
            return 0;
        }
        gfx.write(&self.vbuf, 0, bytemuck::cast_slice(&verts));
        gfx.write(&self.ibuf, 0, bytemuck::cast_slice(&idxs));
        idxs.len() as u32
    }
}

/// Light the boxes pushed since vertex float `from` like `Entity.getBrightnessForRender`: the levels of the cell `at`, in the
/// mesher's `lm` (the day's share is the `amb.x` uniform, as for terrain). ponytail: `light` is the brighter of the two levels.
fn lit(verts: &mut [f32], from: usize, lv: &dyn Fn(i32, i32, i32) -> (u8, u8), at: Vec3) {
    lit_by(verts, from, lv, at, 0);
}

/// `lit` with both levels `dim` lower (the pack's contact shadows: the same cell's light, darker).
fn lit_by(verts: &mut [f32], from: usize, lv: &dyn Fn(i32, i32, i32) -> (u8, u8), at: Vec3, dim: u8) {
    let (sky, block) = lv(at.x.floor() as i32, at.y.floor() as i32, at.z.floor() as i32);
    let (sky, block) = (sky.saturating_sub(dim), block.saturating_sub(dim));
    let b = brightness(sky.max(block));
    for v in verts[from..].chunks_exact_mut(VF) {
        v[5] = b;
        v[6] = lm(sky, block);
    }
}

fn geometry(drops: &Drops, mobs: &Mobs, falling: &[Falling], fx: &Effects, lv: &dyn Fn(i32, i32, i32) -> (u8, u8), hook: Option<Vec3>, paintings: &[Painting], signs: &[SignView], boats: (&[Boat], f32), carts: (&[Cart], f32), player: Option<&crate::render::player::View>, hand: Option<&crate::render::player::Hand>, pack: Option<PackDraw>, eye: Vec3, seen: &dyn Fn(Vec3, f32) -> bool, sign_cache: &mut SignCache) -> (Vec<f32>, Vec<u32>) {
    let (mut verts, mut idxs) = (Vec::new(), Vec::new());
    let a = drops.alpha();
    for e in &drops.items {
        let n0 = verts.len();
        let mut c = e.prev.lerp(e.pos, a);
        c.y += ((e.age as f32 + a) / 10.0).sin() * 0.1 + 0.1;
        if !seen(c, 1.0) {
            continue;
        }
        let h = 0.125;
        // An item (not a block) is its `gui/items.png` sprite, two thin crossed slabs so it shows from every side; the alpha cuts it out.
        match Some(e.stack.id).filter(|&i| i >= 256).and_then(atlas::item_icon) {
            Some(icon) => {
                let (t, w, d) = (atlas::ITEM_TILE0 + icon as u16, 0.18, 0.015);
                push_box(&mut verts, &mut idxs, [c.x - w, c.y - w, c.z - d], [c.x + w, c.y + w, c.z + d], t, 1.0);
                push_box(&mut verts, &mut idxs, [c.x - d, c.y - w, c.z - w], [c.x + d, c.y + w, c.z + w], t, 1.0);
            }
            None => push_box(&mut verts, &mut idxs, [c.x - h, c.y - h, c.z - h], [c.x + h, c.y + h, c.z + h], stack_tile(e.stack), 1.0),
        }
        lit(&mut verts, n0, lv, c);
    }
    for m in mobs.list.iter().filter(|m| seen(m.body.pos, 4.0)) {
        let n0 = verts.len();
        push_mob(&mut verts, &mut idxs, m);
        lit(&mut verts, n0, lv, m.body.pos);
    }
    // The local player (`RenderPlayer`), only in the third-person view.
    if let Some(p) = player {
        let n0 = verts.len();
        crate::render::player::push(&mut verts, &mut idxs, &p.root, &p.pose, p.held, &p.worn);
        lit(&mut verts, n0, lv, p.at);
    }
    // The first-person hand (`ItemRenderer`), in the first-person view only.
    if let Some(h) = hand {
        let n0 = verts.len();
        crate::render::player::push_hand(&mut verts, &mut idxs, h);
        lit(&mut verts, n0, lv, h.at);
    }
    for c in carts.0.iter().filter(|c| seen(c.pos, 2.0)).take(MAX_CARTS) {
        let n0 = verts.len();
        push_cart(&mut verts, &mut idxs, c, carts.1);
        lit(&mut verts, n0, lv, c.pos);
    }
    for b in boats.0.iter().filter(|b| seen(b.pos, 3.0)).take(MAX_BOATS) {
        let n0 = verts.len();
        push_boat(&mut verts, &mut idxs, b, boats.1);
        lit(&mut verts, n0, lv, b.pos);
    }
    // The pack's cloth spread on the ground (`world::pack`): a flat box of wool (`wool` = its colour: brown, or the carried pack's
    // preview, lime where it fits and red where it does not) and the roll it comes off; with its screen open, the flat patches and the
    // stacks on it, all lit by the cell above the cloth, then their contact shadows (dimmer by `SHADOW_DIM` levels).
    if let Some(p) = pack.filter(|p| seen(p.mat.c, 3.0)) {
        let (m, n0, at) = (p.mat, verts.len(), p.mat.c + Vec3::Y * 0.5);
        for (lo, hi) in m.boxes() {
            push_box(&mut verts, &mut idxs, lo, hi, atlas::tile_of(35, p.wool), 1.0);
        }
        if let Some(sc) = p.scene {
            sc.flats.iter().for_each(|f| push_flat(&mut verts, &mut idxs, m, f));
            sc.lays.iter().for_each(|l| push_lay(&mut verts, &mut idxs, m, l, false));
        }
        lit(&mut verts, n0, lv, at);
        if let Some(sc) = p.scene {
            let s0 = verts.len();
            sc.lays.iter().for_each(|l| push_lay(&mut verts, &mut idxs, m, l, true));
            lit_by(&mut verts, s0, lv, at, SHADOW_DIM);
        }
    }
    for a in mobs.arrows.iter().filter(|a| seen(a.pos, 1.0)) {
        let (p, h) = (a.pos, if a.kind == 0 { 0.06 } else { 0.125 });
        let n0 = verts.len();
        let tile = if a.kind == 0 { ItemStack { id: 35, count: 1, damage: 12 } } else { ItemStack { id: a.kind, count: 1, damage: 0 } };
        push_box(&mut verts, &mut idxs, [p.x - h, p.y - h, p.z - h], [p.x + h, p.y + h, p.z + h], stack_tile(tile), 1.0);
        lit(&mut verts, n0, lv, p);
    }
    // The fishing bobber: a small red box.
    if let Some(p) = hook {
        let (h, n0) = (0.125, verts.len());
        push_box(&mut verts, &mut idxs, [p.x - h, p.y, p.z - h], [p.x + h, p.y + 2.0 * h, p.z + h], stack_tile(ItemStack { id: 35, count: 1, damage: 14 }), 1.0);
        lit(&mut verts, n0, lv, p);
    }
    // A falling block is a 0.98 box in its block's flat colour (`EntityFallingSand`).
    for f in falling {
        let c = f.prev.lerp(f.pos, a);
        if !seen(c, 1.5) {
            continue;
        }
        let n0 = verts.len();
        if f.mv.is_some() {
            // A moving piston piece: the block's own bounds (head plate, trimmed base) in its flat colour, `meta` as the damage.
            let b = crate::world::chunk::box_bounds(f.id, f.meta).unwrap_or([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
            let o = c - Vec3::splat(0.5);
            push_box(&mut verts, &mut idxs, [o.x + b[0], o.y + b[1], o.z + b[2]], [o.x + b[3], o.y + b[4], o.z + b[5]], stack_tile(ItemStack { id: f.id as u16, count: 1, damage: f.meta as u16 }), 1.0);
            lit(&mut verts, n0, lv, c);
            continue;
        }
        let h = 0.49;
        // `RenderTNTPrimed`: every other 5 ticks of the fuse the block flashes white (ponytail: a flat white box, not the additive overlay).
        let flash = f.id == 46 && f.fuse >= 0 && (f.fuse / 5) % 2 == 0;
        let tile = if flash { stack_tile(ItemStack { id: 35, count: 1, damage: 0 }) } else { stack_tile(ItemStack { id: f.id as u16, count: 1, damage: 0 }) };
        push_box(&mut verts, &mut idxs, [c.x - h, c.y - h, c.z - h], [c.x + h, c.y + h, c.z + h], tile, 1.0);
        lit(&mut verts, n0, lv, c);
    }
    // Water and spawner particles: small full-bright boxes in a flat colour (wool: white, light blue, orange, grey). ponytail: no
    // textured quads facing the camera (`EntityFX.renderParticle`), no sprite from `particles.png`.
    for p in fx.particles.iter().filter(|p| seen(p.pos, 1.0)) {
        let (c, h) = (p.prev.lerp(p.pos, a), p.half * 0.5);
        let (tile, light) = match p.kind {
            Fx::Dig(t) => (t, 0.6), // `EntityDiggingFX`: colour x 0.6
            k => (stack_tile(ItemStack { id: 35, count: 1, damage: match k { Fx::Bubble => 0, Fx::Splash => 3, Fx::Flame => 1, Fx::Heart => 14, Fx::Spray => 0, Fx::Leaf => 13, Fx::Twig => 12, _ => 7 } }), 1.0),
        };
        push_box(&mut verts, &mut idxs, [c.x - h, c.y - h, c.z - h], [c.x + h, c.y + h, c.z + h], tile, light);
    }
    // A lightning bolt: one straight white column from the ground up (vanilla's is jagged, seeded by `field_27029_a`).
    for b in fx.bolts.iter().filter(|b| b.visible()) {
        let (p, w) = (b.pos, 0.15);
        push_box(&mut verts, &mut idxs, [p.x - w, p.y, p.z - w], [p.x + w, p.y + 96.0, p.z + w], stack_tile(ItemStack { id: 35, count: 1, damage: 0 }), 1.0);
    }
    // Paintings: each 16 px piece shows its tile of `art/kz.png` on the face away from the wall, lit by the cell in front.
    let mut pieces = 0;
    for p in paintings.iter().filter(|p| p.frame().0.distance_squared(eye) < 48.0 * 48.0) {
        let ps = p.pieces();
        if pieces + ps.len() > PIECES {
            break;
        }
        pieces += ps.len();
        let n0 = verts.len();
        for (tile, min, max) in ps {
            push_face(&mut verts, &mut idxs, p.face(), min, max, atlas::ART_TILE0 + tile, 1.0);
        }
        let (x, y, z) = p.light_cell();
        lit(&mut verts, n0, lv, Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5));
    }
    // Signs (`TileEntitySignRenderer`): the model within 64 blocks, the text within 16.
    let mut quads = 0;
    let now = Instant::now();
    for s in signs.iter().take(MAX_SIGNS) {
        let c = Vec3::new(s.pos.0 as f32 + 0.5, s.pos.1 as f32 + 0.5, s.pos.2 as f32 + 0.5);
        let d = c.distance_squared(eye);
        if d < 64.0 * 64.0 {
            let n0 = verts.len();
            push_sign(&mut verts, &mut idxs, &mut quads, s, d < 16.0 * 16.0, sign_cache, now);
            lit(&mut verts, n0, lv, c);
        }
    }
    sign_cache.sweep(now);
    (verts, idxs)
}

/// The pack's cloth for `ItemMesh::update`: the cloth, its wool colour (brown, or the carried pack's preview: lime / red), and, with the
/// screen open, what lies on it.
pub struct PackDraw<'a> {
    pub mat: &'a Mat,
    pub wool: u8,
    pub scene: Option<&'a Scene>,
}

/// Levels a contact shadow is darker than the cell's light.
const SHADOW_DIM: u8 = 5;

/// A flat patch of the cloth (`pack::Flat`): a thin box, so it has an edge to catch the light.
fn push_flat(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, m: &Mat, f: &pack::Flat) {
    let (a, b) = (m.at(f.r[0], f.r[1], f.lo), m.at(f.r[0] + f.r[2], f.r[1] + f.r[3], f.hi));
    push_box(verts, idxs, a.min(b).to_array(), a.max(b).to_array(), atlas::tile_of(f.tile.0, f.tile.1), 1.0);
}

/// A flat quad in both windings: a sprite lying on the cloth is seen from above, one lifted on the finger from any side. Corners near
/// left, near right, far right, far left; `uv(u, v)` is the atlas place of a corner (u across, v up).
fn push_quad2(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, p: [Vec3; 4], tint: f32, uv: &dyn Fn(f32, f32) -> (f32, f32)) {
    let base = verts.len() as u32 / VF as u32;
    for (c, (u, v)) in p.iter().zip([(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]) {
        let (au, av) = uv(u, v);
        verts.extend_from_slice(&[c.x, c.y, c.z, au, av, 1.0, lm(15, 0), tint]);
    }
    idxs.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3, base, base + 2, base + 1, base, base + 3, base + 2]);
}

/// `ColorizerGrass` / `ColorizerFoliage` for a block face, as the mesher packs it (`mesh::WHITE` = none).
fn block_tint(id: u8, meta: u8, face: usize) -> f32 {
    use crate::world::climate;
    (match id {
        2 if face == 2 => climate::grass(0.8, 0.4),
        18 => match meta & 3 {
            1 => climate::PINE,
            2 => climate::BIRCH,
            _ => climate::foliage(0.8, 0.4),
        },
        _ => return WHITE,
    }) as f32
}

/// One stack lying on the cloth (`pack::Lay`), or its shadow. A block with a cube's shape is a small cube in its terrain tiles (the
/// bottom is never seen, so it is not built); everything else is one flat two-sided quad: the `items.png` sprite, the block's own
/// tile for a plant or a torch, the flat colour where there is neither. No 16-layer extrusion like the hand's: ~45 stacks on a
/// low-end phone. The shadow is a quad on the cloth a little toward the player and to the right: a sprite's own image (dimmed by
/// `lit_by`, so a tool throws its own outline), a cube's footprint in dark wool; it moves out as the stack lifts.
fn push_lay(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, m: &Mat, l: &Lay, shadow: bool) {
    let side = l.side();
    if side <= 0.0 {
        return;
    }
    let (cx, cy) = l.centre();
    let (s, meta, half) = (l.stack, placed_meta(l.stack), side * pack::UNIT * 0.5);
    let (u, r) = m.axes();
    let (h, off) = if shadow { (pack::REST + 0.001, (r * 0.5 - u) * (0.012 + l.lift * 0.5)) } else { (l.base(), Vec3::ZERO) };
    let c = m.at(cx, cy, h) + off;
    let quad = |k: f32| [c - r * half * k - u * half * k, c + r * half * k - u * half * k, c + r * half * k + u * half * k, c - r * half * k + u * half * k];
    if l.cube() {
        if shadow {
            let dark = atlas::tile_of(35, 7);
            push_quad2(verts, idxs, quad(1.05), WHITE, &|u, v| atlas::atlas_uv(dark, u, v));
            return;
        }
        let (a, b) = (m.at(cx - side * 0.5, cy - side * 0.5, h), m.at(cx + side * 0.5, cy + side * 0.5, h + l.height()));
        for face in [0, 1, 2, 4, 5] {
            let n0 = verts.len();
            push_face(verts, idxs, face, a.min(b).to_array(), a.max(b).to_array(), 0, 1.0);
            for (k, v) in verts[n0..].chunks_exact_mut(VF).enumerate() {
                let (au, av) = atlas::face_uv(s.id as u8, meta, face, [0.0, 1.0, 1.0, 0.0][k], [0.0, 0.0, 1.0, 1.0][k]);
                (v[3], v[4], v[7]) = (au, av, block_tint(s.id as u8, meta, face));
            }
        }
        return;
    }
    let uv = move |u: f32, v: f32| -> (f32, f32) {
        if s.id < 256 {
            atlas::face_uv(s.id as u8, meta, 5, u, v)
        } else if let Some(icon) = atlas::item_icon(s.id) {
            atlas::item_uv(icon, u, v)
        } else {
            atlas::atlas_uv(stack_tile(s), u, v)
        }
    };
    let tint = if s.id == 31 && meta != 0 { block_tint(2, 0, 2) } else { WHITE };
    push_quad2(verts, idxs, quad(1.0), tint, &uv);
}

/// A sign's root transform (`TileEntitySignRenderer.renderTileEntitySignAt`: the board's centre at 2/3 scale, turned by the
/// metadata: a post 22.5 degrees a step, a wall sign the side it hangs on) and whether it is a wall sign.
fn sign_root(s: &SignView) -> (Mat4, bool) {
    let sc = SIGN_SCALE;
    let wall = s.id != decor::POST;
    let turn = if wall { -match s.meta { 2 => 180.0f32, 4 => 90.0, 5 => -90.0, _ => 0.0 } } else { -(s.meta as f32) * 22.5 };
    let mut root = Mat4::from_translation(Vec3::new(s.pos.0 as f32 + 0.5, s.pos.1 as f32 + 12.0 / 16.0 * sc, s.pos.2 as f32 + 0.5)) * Mat4::from_rotation_y(turn.to_radians());
    if wall {
        root *= Mat4::from_translation(Vec3::new(0.0, -5.0 / 16.0, -7.0 / 16.0));
    }
    (root, wall)
}

const SIGN_SCALE: f32 = 2.0 / 3.0;

/// `TileEntitySignRenderer.renderTileEntitySignAt`: the board (and the post of a standing sign) from `item/sign.png`, 2/3 scale,
/// turned by the metadata (post: 22.5 degrees a step; wall: the side), then the four lines in black from the HUD font's glyph
/// runs (`cache`: built once per text, see `SignCache`), 1/60 of that scale per font pixel, centred on the board. ponytail: the
/// model is drawn two-sided (no culling choice), text is flat black quads (no `FontRenderer` texture) and only within 16 blocks.
fn push_sign(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, quads: &mut usize, s: &SignView, text: bool, cache: &mut SignCache, now: Instant) {
    let sc = SIGN_SCALE;
    let (root, wall) = sign_root(s);
    let model = root * Mat4::from_scale(Vec3::new(sc, -sc, -sc) / 16.0);
    push_model_box(verts, idxs, &model, [-12.0, -14.0, -1.0], [24.0, 12.0, 2.0], (0.0, 0.0));
    if !wall {
        push_model_box(verts, idxs, &model, [-1.0, -2.0, -1.0], [2.0, 14.0, 2.0], (0.0, 14.0));
    }
    if !text {
        return;
    }
    // As many quads as fit under `TEXT_QUADS`, from the first: what drawing them one by one until the cap gave.
    let t = cache.get(s, now);
    let n = t.quads.min(TEXT_QUADS - *quads);
    let base = verts.len() as u32 / VF as u32;
    verts.extend_from_slice(&t.verts[..n * 4 * VF]);
    idxs.extend(t.idxs[..n * 6].iter().map(|i| i + base));
    *quads += n;
}

/// How long a sign's text stays built after the last frame it was drawn in (ImmediatelyFast's sign cache: `expireAfterAccess` 5 s).
const SIGN_TTL: Duration = Duration::from_secs(5);

/// The text of one sign as it was last built: `verts` (`mesh::VF` floats a vertex, world space, not lit yet) and `idxs` (from 0) of
/// `quads` quads in reading order, and what they were built from.
struct SignText {
    id: u8,
    meta: u8,
    text: Text,
    verts: Vec<f32>,
    idxs: Vec<u32>,
    quads: usize,
    used: Instant,
}

/// The text quads of the signs in view, kept between frames (ImmediatelyFast's `sign_text_buffering`: text is rebuilt only when it
/// changes, not every frame; there the text is drawn into an atlas and a sign is one quad, here the quads themselves are kept, so the
/// result is vertex for vertex what the per-frame build gave). One entry per sign cell; it is rebuilt when the text, the block or the
/// metadata of that cell differ, and dropped after `SIGN_TTL` unused (a sign that was broken, left behind or is out of the 16 block range
/// for text; another world's signs go the same way).
#[derive(Default)]
pub struct SignCache {
    map: std::collections::HashMap<(i32, i32, i32), SignText>,
}

impl SignCache {
    fn get(&mut self, s: &SignView, now: Instant) -> &SignText {
        if self.map.get(&s.pos).map_or(true, |t| t.id != s.id || t.meta != s.meta || t.text != *s.text) {
            let (verts, idxs, quads) = text_quads(s);
            self.map.insert(s.pos, SignText { id: s.id, meta: s.meta, text: s.text.clone(), verts, idxs, quads, used: now });
        }
        let t = self.map.get_mut(&s.pos).expect("inserted above");
        t.used = now;
        t
    }

    /// Drop what has not been drawn for `SIGN_TTL`.
    fn sweep(&mut self, now: Instant) {
        self.map.retain(|_, t| now.duration_since(t.used) < SIGN_TTL);
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }
}

/// The four lines of `s` as black quads: the font's runs of lit pixels (`font::GLYPHS`), 1/60 of the sign's scale a pixel, each line
/// centred on the board; vertices in world space, indices from 0, and the quad count.
fn text_quads(s: &SignView) -> (Vec<f32>, Vec<u32>, usize) {
    let sc = SIGN_SCALE;
    let (root, _) = sign_root(s);
    let (mut verts, mut idxs, mut quads) = (Vec::new(), Vec::new(), 0usize);
    let tm = root * Mat4::from_translation(Vec3::new(0.0, 0.5 * sc, 0.07 * sc)) * Mat4::from_scale(Vec3::new(sc / 60.0, -sc / 60.0, sc / 60.0));
    let (au, av) = atlas::atlas_uv(stack_tile(ItemStack { id: 35, count: 1, damage: 15 }), 0.0, 0.0);
    for (i, line) in s.text.iter().enumerate() {
        let g = |b: u8| if (32..127).contains(&b) { (b - 32) as usize } else { 31 };
        let width: i32 = line.bytes().map(|b| crate::render::font::WIDTH[g(b)] as i32).sum();
        let (mut cx, y0) = (-(width / 2) as f32, (i as i32 * 10 - 20) as f32);
        for b in line.bytes() {
            let gi = g(b);
            for (row, bits) in crate::render::font::GLYPHS[gi].iter().enumerate() {
                let mut px = 0;
                while px < 8 {
                    if bits >> px & 1 == 0 {
                        px += 1;
                        continue;
                    }
                    let run = (px..8).take_while(|p| bits >> p & 1 == 1).count();
                    quads += 1;
                    let (x0, x1, ya, yb) = (cx + px as f32, cx + (px + run) as f32, y0 + row as f32, y0 + row as f32 + 1.0);
                    let base = verts.len() as u32 / VF as u32;
                    for (x, y) in [(x0, yb), (x1, yb), (x1, ya), (x0, ya)] {
                        let p = tm.transform_point3(Vec3::new(x, y, 0.0));
                        verts.extend_from_slice(&[p.x, p.y, p.z, au, av, 1.0, lm(15, 0), WHITE]);
                    }
                    idxs.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
                    px += run;
                }
            }
            cx += crate::render::font::WIDTH[gi] as f32;
        }
    }
    (verts, idxs, quads)
}

/// `ModelRenderer.addBox` + `TexturedQuad` for `item/sign.png` (64 x 32): the box at `min` with `size` in model pixels through `m`,
/// its six faces unwrapped from texture offset `o` (right, left, top, bottom, front, back), both windings.
fn push_model_box(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, m: &Mat4, min: [f32; 3], size: [f32; 3], o: (f32, f32)) {
    let (x1, y1, z1) = (min[0], min[1], min[2]);
    let (x2, y2, z2) = (x1 + size[0], y1 + size[1], z1 + size[2]);
    let (dx, dy, dz, ox, oy) = (size[0], size[1], size[2], o.0, o.1);
    let c = [[x1, y1, z1], [x2, y1, z1], [x2, y2, z1], [x1, y2, z1], [x1, y1, z2], [x2, y1, z2], [x2, y2, z2], [x1, y2, z2]];
    let faces: [([usize; 4], [f32; 4]); 6] = [
        ([5, 1, 2, 6], [ox + dz + dx, oy + dz, ox + dz + dx + dz, oy + dz + dy]),
        ([0, 4, 7, 3], [ox, oy + dz, ox + dz, oy + dz + dy]),
        ([5, 4, 0, 1], [ox + dz, oy, ox + dz + dx, oy + dz]),
        ([2, 3, 7, 6], [ox + dz + dx, oy, ox + dz + dx + dx, oy + dz]),
        ([1, 0, 3, 2], [ox + dz, oy + dz, ox + dz + dx, oy + dz + dy]),
        ([4, 5, 6, 7], [ox + dz + dx + dz, oy + dz, ox + dz + dx + dz + dx, oy + dz + dy]),
    ];
    for (vi, [u0, v0, u1, v1]) in faces {
        let base = verts.len() as u32 / VF as u32;
        // `TexturedQuad`'s corner order and its 0.1 px inset.
        let uv = [(u1 - 0.1, v0 + 0.1), (u0 + 0.1, v0 + 0.1), (u0 + 0.1, v1 - 0.1), (u1 - 0.1, v1 - 0.1)];
        for (k, &i) in vi.iter().enumerate() {
            let p = m.transform_point3(Vec3::from(c[i]));
            let (au, av) = atlas::sign_px(uv[k].0, uv[k].1);
            verts.extend_from_slice(&[p.x, p.y, p.z, au, av, 1.0, lm(15, 0), WHITE]);
        }
        idxs.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3, base, base + 2, base + 1, base, base + 3, base + 2]);
    }
}

/// One model box: (min, size, rotation point, rotation x/y/z, swing phase + amplitude, texture), in model pixels.
type Part = ([f32; 3], [f32; 3], [f32; 3], [f32; 3], [f32; 2], Tx);
/// `ModelRenderer.setTextureOffset`, `addBox`'s inflation and `mirror` of one box.
#[derive(Clone, Copy)]
struct Tx {
    o: (f32, f32),
    grow: f32,
    mirror: bool,
}
use std::f32::consts::{FRAC_PI_2 as Q, FRAC_PI_4 as E, PI};

fn pt(min: [f32; 3], size: [f32; 3], rp: [f32; 3], rot: [f32; 3], sw: [f32; 2], o: (f32, f32)) -> Part {
    (min, size, rp, rot, sw, Tx { o, grow: 0.0, mirror: false })
}

/// `addBox`'s inflation `g` on a box.
fn grown(mut p: Part, g: f32) -> Part {
    p.5.grow = g;
    p
}

/// A mirrored box (`ModelRenderer.mirror`, the left limbs of `ModelBiped`).
fn mirrored(mut p: Part) -> Part {
    p.5.mirror = true;
    p
}

/// `ModelQuadruped`: head, the body lying down, four legs (`cos(limb x 0.6662 + phase) x 1.4 x amount`); one leg length `l`. `tex` is the
/// texture offset of the head and of the body; the legs are at (0, 16).
fn quad(head: ([f32; 3], [f32; 3], [f32; 3]), body: ([f32; 3], [f32; 3], [f32; 3]), l: f32, x: f32, back: f32, front: f32, tex: [(f32, f32); 2]) -> Vec<Part> {
    let leg = |rx: f32, z: f32, ph: f32| pt([-2.0, 0.0, -2.0], [4.0, l, 4.0], [rx, 24.0 - l, z], [0.0; 3], [ph, 1.4], (0.0, 16.0));
    vec![
        pt(head.0, head.1, head.2, [0.0; 3], [0.0; 2], tex[0]),
        pt(body.0, body.1, body.2, [Q, 0.0, 0.0], [0.0; 2], tex[1]),
        leg(-x, back, 0.0), leg(x, back, PI), leg(-x, front, PI), leg(x, front, 0.0),
    ]
}

/// `ModelBiped`-shaped (zombie, skeleton, giant): arms `w` wide, raised when `raised` (`ModelZombie`). Head (0, 0), body (16, 16),
/// arms (40, 16), legs (0, 16); the left arm and leg are mirrored.
fn biped(w: f32, raised: bool) -> Vec<Part> {
    let (o, a) = (if w == 4.0 { 0.0 } else { 1.0 }, if raised { -Q } else { 0.0 });
    let amp = if raised { 0.0 } else { 1.0 };
    vec![
        pt([-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], [0.0; 3], [0.0; 3], [0.0; 2], (0.0, 0.0)),
        pt([-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], [0.0; 3], [0.0; 3], [0.0; 2], (16.0, 16.0)),
        pt([-3.0 + 2.0 * o, -2.0, -2.0 + o], [w, 12.0, w], [-5.0, 2.0, 0.0], [a, 0.0, 0.0], [PI, amp], (40.0, 16.0)),
        mirrored(pt([-1.0, -2.0, -2.0 + o], [w, 12.0, w], [5.0, 2.0, 0.0], [a, 0.0, 0.0], [0.0, amp], (40.0, 16.0))),
        pt([-2.0 + o, 0.0, -2.0 + o], [w, 12.0, w], [-2.0, 12.0, 0.0], [0.0; 3], [0.0, 1.4], (0.0, 16.0)),
        mirrored(pt([-2.0 + o, 0.0, -2.0 + o], [w, 12.0, w], [2.0, 12.0, 0.0], [0.0; 3], [PI, 1.4], (0.0, 16.0))),
    ]
}

/// How many of the first parts hang from the head's pivot and follow its dip (`head_dip`): the cow's horns, the hen's bill and chin.
fn head_n(kind: Kind) -> usize {
    match kind {
        Cow | Chicken => 3,
        Pig | Sheep => 1,
        _ => 0,
    }
}

/// `ModelWolf`, standing or sitting (`setLivingAnimations`); the tail angle is `EntityWolf.setTailRotation`.
fn wolf(m: &Mob) -> Vec<Part> {
    let tail = if m.anger > 0 { PI * 0.49 } else if m.tame { (0.55 - (20 - m.health).clamp(0, 20) as f32 * 0.02) * PI } else { PI * 0.2 };
    let head = |min: [f32; 3], size: [f32; 3], rp: [f32; 3], o: (f32, f32)| pt(min, size, rp, [0.0; 3], [0.0; 2], o);
    let leg = |x: f32, y: f32, z: f32, rx: f32, ph: f32, amp: f32| pt([-1.0, 0.0, -1.0], [2.0, 8.0, 2.0], [x, y, z], [rx, 0.0, 0.0], [ph, amp], (0.0, 18.0));
    let h = [-1.0, 13.5, -7.0];
    let mut v = vec![
        head([-3.0, -3.0, -2.0], [6.0, 6.0, 4.0], h, (0.0, 0.0)),
        head([-3.0, -5.0, 0.0], [2.0, 2.0, 1.0], h, (16.0, 14.0)),
        head([1.0, -5.0, 0.0], [2.0, 2.0, 1.0], h, (16.0, 14.0)),
        head([-2.0, 0.0, -5.0], [3.0, 3.0, 4.0], [-0.5, 13.5, -7.0], (0.0, 10.0)),
    ];
    if m.sitting {
        v.extend([
            pt([-4.0, -2.0, -3.0], [6.0, 9.0, 6.0], [0.0, 18.0, 0.0], [PI * 0.25, 0.0, 0.0], [0.0; 2], (18.0, 14.0)),
            pt([-4.0, -3.0, -3.0], [8.0, 6.0, 7.0], [-1.0, 16.0, -3.0], [PI * 0.4, 0.0, 0.0], [0.0; 2], (21.0, 0.0)),
            leg(-2.5, 22.0, 2.0, PI * 1.5, 0.0, 0.0),
            leg(0.5, 22.0, 2.0, PI * 1.5, 0.0, 0.0),
            leg(-2.49, 17.0, -4.0, PI * 1.85, 0.0, 0.0),
            leg(0.51, 17.0, -4.0, PI * 1.85, 0.0, 0.0),
            pt([-1.0, 0.0, -1.0], [2.0, 8.0, 2.0], [-1.0, 21.0, 6.0], [tail, 0.0, 0.0], [0.0; 2], (9.0, 18.0)),
        ]);
    } else {
        v.extend([
            pt([-4.0, -2.0, -3.0], [6.0, 9.0, 6.0], [0.0, 14.0, 2.0], [Q, 0.0, 0.0], [0.0; 2], (18.0, 14.0)),
            pt([-4.0, -3.0, -3.0], [8.0, 6.0, 7.0], [-1.0, 14.0, -3.0], [Q, 0.0, 0.0], [0.0; 2], (21.0, 0.0)),
            leg(-2.5, 16.0, 7.0, 0.0, 0.0, 1.4),
            leg(0.5, 16.0, 7.0, 0.0, PI, 1.4),
            leg(-2.5, 16.0, -4.0, 0.0, PI, 1.4),
            leg(0.5, 16.0, -4.0, 0.0, 0.0, 1.4),
            pt([-1.0, 0.0, -1.0], [2.0, 8.0, 2.0], [-1.0, 12.0, 8.0], [tail, 0.0, 0.0], [0.0; 2], (9.0, 18.0)),
        ]);
    }
    v
}

fn parts(m: &Mob) -> Vec<Part> {
    match m.kind {
        Pig => quad(([-4.0, -4.0, -8.0], [8.0, 8.0, 8.0], [0.0, 12.0, -6.0]), ([-5.0, -10.0, -7.0], [10.0, 16.0, 8.0], [0.0, 11.0, 2.0]), 6.0, 3.0, 7.0, -5.0, [(0.0, 0.0), (28.0, 8.0)]),
        // `ModelCow`: the horns (22, 0) follow the head, the udder (52, 0) hangs under the back of the body.
        Cow => {
            let mut v = quad(([-4.0, -4.0, -6.0], [8.0, 8.0, 6.0], [0.0, 4.0, -8.0]), ([-6.0, -10.0, -7.0], [12.0, 18.0, 10.0], [0.0, 5.0, 2.0]), 12.0, 4.0, 7.0, -6.0, [(0.0, 0.0), (18.0, 4.0)]);
            v.insert(1, pt([-4.0, -5.0, -4.0], [1.0, 3.0, 1.0], [0.0, 3.0, -7.0], [0.0; 3], [0.0; 2], (22.0, 0.0)));
            v.insert(2, pt([3.0, -5.0, -4.0], [1.0, 3.0, 1.0], [0.0, 3.0, -7.0], [0.0; 3], [0.0; 2], (22.0, 0.0)));
            v.push(pt([-2.0, -3.0, 0.0], [4.0, 6.0, 2.0], [0.0, 14.0, 6.0], [Q, 0.0, 0.0], [0.0; 2], (52.0, 0.0)));
            v
        }
        // `ModelSheep2`: the skin (head, body, legs) under the fleece of `fur`.
        Sheep => quad(([-3.0, -4.0, -6.0], [6.0, 6.0, 8.0], [0.0, 6.0, -8.0]), ([-4.0, -10.0, -7.0], [8.0, 16.0, 6.0], [0.0, 5.0, 2.0]), 12.0, 3.0, 7.0, -5.0, [(0.0, 0.0), (28.0, 8.0)]),
        Wolf => wolf(m),
        // `ModelChicken`: head, bill and chin hang from one pivot.
        Chicken => vec![
            pt([-2.0, -6.0, -2.0], [4.0, 6.0, 3.0], [0.0, 15.0, -4.0], [0.0; 3], [0.0; 2], (0.0, 0.0)),
            pt([-2.0, -4.0, -4.0], [4.0, 2.0, 2.0], [0.0, 15.0, -4.0], [0.0; 3], [0.0; 2], (14.0, 0.0)),
            pt([-1.0, -2.0, -3.0], [2.0, 2.0, 2.0], [0.0, 15.0, -4.0], [0.0; 3], [0.0; 2], (14.0, 4.0)),
            pt([-3.0, -4.0, -3.0], [6.0, 8.0, 6.0], [0.0, 16.0, 0.0], [Q, 0.0, 0.0], [0.0; 2], (0.0, 9.0)),
            pt([-1.0, 0.0, -3.0], [3.0, 5.0, 3.0], [-2.0, 19.0, 1.0], [0.0; 3], [0.0, 1.4], (26.0, 0.0)),
            pt([-1.0, 0.0, -3.0], [3.0, 5.0, 3.0], [1.0, 19.0, 1.0], [0.0; 3], [PI, 1.4], (26.0, 0.0)),
            pt([0.0, 0.0, -3.0], [1.0, 4.0, 6.0], [-4.0, 13.0, 0.0], [0.0; 3], [0.0; 2], (24.0, 13.0)),
            pt([-1.0, 0.0, -3.0], [1.0, 4.0, 6.0], [4.0, 13.0, 0.0], [0.0; 3], [0.0; 2], (24.0, 13.0)),
        ],
        Squid => {
            let mut v = vec![pt([-6.0, -8.0, -6.0], [12.0, 16.0, 12.0], [0.0, 8.0, 0.0], [0.0; 3], [0.0; 2], (0.0, 0.0))];
            v.extend((0..8).map(|i| {
                let a = i as f32 * E;
                pt([-1.0, 0.0, -1.0], [2.0, 18.0, 2.0], [a.cos() * 5.0, 15.0, a.sin() * 5.0], [0.0, -a + Q, 0.0], [0.0; 2], (48.0, 0.0))
            }));
            v
        }
        Zombie | PigZombie | Giant => biped(4.0, true),
        Skeleton => biped(2.0, false),
        // `ModelCreeper` (b1.7.3's `var2` is 4: the legs end 2 px above the model's ground line).
        Creeper => {
            let leg = |x: f32, z: f32, ph: f32| pt([-2.0, 0.0, -2.0], [4.0, 6.0, 4.0], [x, 16.0, z], [0.0; 3], [ph, 1.4], (0.0, 16.0));
            vec![
                pt([-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], [0.0, 4.0, 0.0], [0.0; 3], [0.0; 2], (0.0, 0.0)),
                pt([-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], [0.0, 4.0, 0.0], [0.0; 3], [0.0; 2], (16.0, 16.0)),
                leg(-2.0, 4.0, 0.0), leg(2.0, 4.0, PI), leg(-2.0, -4.0, PI), leg(2.0, -4.0, 0.0),
            ]
        }
        Spider => {
            let mut v = vec![
                pt([-4.0, -4.0, -8.0], [8.0, 8.0, 8.0], [0.0, 15.0, -3.0], [0.0; 3], [0.0; 2], (32.0, 4.0)),
                pt([-3.0, -3.0, -3.0], [6.0, 6.0, 6.0], [0.0, 15.0, 0.0], [0.0; 3], [0.0; 2], (0.0, 0.0)),
                pt([-5.0, -4.0, -6.0], [10.0, 8.0, 12.0], [0.0, 15.0, 9.0], [0.0; 3], [0.0; 2], (0.0, 12.0)),
            ];
            let (ry, rz) = ([E, -E, E / 2.0, -E / 2.0, -E / 2.0, E / 2.0, -E, E], [-E, E, -0.58, 0.58, -0.58, 0.58, -E, E]);
            v.extend((0..8).map(|i| {
                let left = i % 2 == 1;
                pt([if left { -1.0 } else { -15.0 }, -1.0, -1.0], [16.0, 2.0, 2.0], [if left { 4.0 } else { -4.0 }, 15.0, 2.0 - (i / 2) as f32], [0.0, ry[i], rz[i]], [0.0; 2], (18.0, 0.0))
            }));
            v
        }
        // `ModelSlime(16)`: the core, the eyes and the mouth. ponytail: the outer jelly layer (`ModelSlime(0)`, a translucent pass) is not
        // drawn: the entity pass has no blending, and at 78% alpha it would hide the face.
        Slime => vec![
            pt([-3.0, 17.0, -3.0], [6.0, 6.0, 6.0], [0.0; 3], [0.0; 3], [0.0; 2], (0.0, 16.0)),
            pt([-3.25, 18.0, -3.5], [2.0, 2.0, 2.0], [0.0; 3], [0.0; 3], [0.0; 2], (32.0, 0.0)),
            pt([1.25, 18.0, -3.5], [2.0, 2.0, 2.0], [0.0; 3], [0.0; 3], [0.0; 2], (32.0, 4.0)),
            pt([0.0, 21.0, -3.5], [1.0, 1.0, 1.0], [0.0; 3], [0.0; 3], [0.0; 2], (32.0, 8.0)),
        ],
    }
}

/// `ModelSheep1`, the fleece pass (`RenderSheep.setWoolColorAndRender`): the same pose on inflated head, body and upper legs, from
/// `mob/sheep_fur.png`. Empty for a shorn sheep.
fn fur(m: &Mob) -> Vec<Part> {
    if m.kind != Sheep || m.sheared {
        return Vec::new();
    }
    let leg = |x: f32, z: f32, ph: f32| grown(pt([-2.0, 0.0, -2.0], [4.0, 6.0, 4.0], [x, 12.0, z], [0.0; 3], [ph, 1.4], (0.0, 16.0)), 0.5);
    vec![
        grown(pt([-3.0, -4.0, -4.0], [6.0, 6.0, 6.0], [0.0, 6.0, -8.0], [0.0; 3], [0.0; 2], (0.0, 0.0)), 0.6),
        grown(pt([-4.0, -10.0, -7.0], [8.0, 16.0, 6.0], [0.0, 5.0, 2.0], [Q, 0.0, 0.0], [0.0; 2], (28.0, 8.0)), 1.75),
        leg(-3.0, 7.0, 0.0), leg(3.0, 7.0, PI), leg(-3.0, -5.0, PI), leg(3.0, -5.0, 0.0),
    ]
}

/// The skin texture slot of `m` (`Entity.texture`; the wolf's changes with `EntityWolf.getEntityTexture`).
fn skin_of(m: &Mob) -> u8 {
    use crate::render::atlas::skin;
    match m.kind {
        Pig => skin::PIG,
        Cow => skin::COW,
        Chicken => skin::CHICKEN,
        Sheep => skin::SHEEP,
        Wolf if m.tame => skin::WOLF_TAME,
        Wolf if m.anger > 0 => skin::WOLF_ANGRY,
        Wolf => skin::WOLF,
        Squid => skin::SQUID,
        Zombie | Giant => skin::ZOMBIE,
        PigZombie => skin::PIGZOMBIE,
        Skeleton => skin::SKELETON,
        Creeper => skin::CREEPER,
        Spider => skin::SPIDER,
        Slime => skin::SLIME,
    }
}

/// `EntitySheep.fleeceColorTable` as a vertex tint.
fn fleece(color: u8) -> f32 {
    const T: [[f32; 3]; 16] = [
        [1.0, 1.0, 1.0], [0.95, 0.7, 0.2], [0.9, 0.5, 0.85], [0.6, 0.7, 0.95], [0.9, 0.9, 0.2], [0.5, 0.8, 0.1], [0.95, 0.7, 0.8], [0.3, 0.3, 0.3],
        [0.6, 0.6, 0.6], [0.3, 0.6, 0.7], [0.7, 0.4, 0.9], [0.2, 0.4, 0.8], [0.5, 0.4, 0.3], [0.4, 0.5, 0.2], [0.8, 0.3, 0.3], [0.1, 0.1, 0.1],
    ];
    let c = T[color as usize & 15];
    tint(c[0], c[1], c[2])
}

/// A vertex tint (`mesh::WHITE`'s packing) from linear 0..1 channels.
fn tint(r: f32, g: f32, b: f32) -> f32 {
    let q = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u32;
    ((q(r) << 16) | (q(g) << 8) | q(b)) as f32
}

/// `ModelRenderer.addBox` + `TexturedQuad` for a 64 x 32 skin in atlas slot `slot` (`push_model_box`'s unwrap, plus the inflation and
/// the mirror of `ModelRenderer`): the box at `min` with `size` in model pixels through `m`, both windings, multiplied by `tint`.
fn push_skin_box(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, m: &Mat4, min: [f32; 3], size: [f32; 3], t: Tx, slot: u8, tint: f32) {
    let g = t.grow;
    let (mut x1, y1, z1) = (min[0] - g, min[1] - g, min[2] - g);
    let (mut x2, y2, z2) = (min[0] + size[0] + g, min[1] + size[1] + g, min[2] + size[2] + g);
    if t.mirror {
        std::mem::swap(&mut x1, &mut x2);
    }
    let (dx, dy, dz, ox, oy) = (size[0], size[1], size[2], t.o.0, t.o.1);
    let c = [[x1, y1, z1], [x2, y1, z1], [x2, y2, z1], [x1, y2, z1], [x1, y1, z2], [x2, y1, z2], [x2, y2, z2], [x1, y2, z2]];
    let faces: [([usize; 4], [f32; 4]); 6] = [
        ([5, 1, 2, 6], [ox + dz + dx, oy + dz, ox + dz + dx + dz, oy + dz + dy]),
        ([0, 4, 7, 3], [ox, oy + dz, ox + dz, oy + dz + dy]),
        ([5, 4, 0, 1], [ox + dz, oy, ox + dz + dx, oy + dz]),
        ([2, 3, 7, 6], [ox + dz + dx, oy, ox + dz + dx + dx, oy + dz]),
        ([1, 0, 3, 2], [ox + dz, oy + dz, ox + dz + dx, oy + dz + dy]),
        ([4, 5, 6, 7], [ox + dz + dx + dz, oy + dz, ox + dz + dx + dz + dx, oy + dz + dy]),
    ];
    for (vi, [u0, v0, u1, v1]) in faces {
        let base = verts.len() as u32 / VF as u32;
        let mut q: [(Vec3, (f32, f32)); 4] = [(Vec3::ZERO, (0.0, 0.0)); 4];
        let uv = [(u1 - 0.1, v0 + 0.1), (u0 + 0.1, v0 + 0.1), (u0 + 0.1, v1 - 0.1), (u1 - 0.1, v1 - 0.1)];
        for k in 0..4 {
            q[k] = (m.transform_point3(Vec3::from(c[vi[k]])), uv[k]);
        }
        if t.mirror {
            q.reverse(); // `flipFace`
        }
        for (p, uv) in q {
            let (au, av) = atlas::skin_px(slot, uv.0, uv.1);
            verts.extend_from_slice(&[p.x, p.y, p.z, au, av, 1.0, lm(15, 0), tint]);
        }
        idxs.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3, base, base + 2, base + 1, base, base + 3, base + 2]);
    }
}

/// `RenderMinecart` / `ModelMinecart`: a floor and four walls in iron-block tiles, turned to `180 - yaw` and rocking after a hit, and
/// for the furnace cart a furnace block 0.75 wide on the floor (`renderBlockAsItem` at 0.75, 0.3125 up). UNVERIFIED stand-in for
/// `item/cart.png`; no pitch on slopes.
fn push_cart(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, c: &Cart, a: f32) {
    let t = c.hit_time as f32 - a;
    let rock = if t > 0.0 { (t.sin() * t * (c.damage as f32 - a).max(0.0) / 10.0 * c.rock as f32).to_radians() } else { 0.0 };
    let root = Mat4::from_translation(c.prev.lerp(c.pos, a)) * Mat4::from_rotation_y(PI - c.yaw.to_radians()) * Mat4::from_rotation_x(rock);
    let iron = stack_tile(ItemStack { id: 42, count: 1, damage: 0 });
    let mut parts = vec![
        ([-0.625, -0.3, -0.5], [1.25, 0.125, 1.0], iron),
        ([-0.625, -0.175, -0.5], [0.125, 0.5, 1.0], iron),
        ([0.5, -0.175, -0.5], [0.125, 0.5, 1.0], iron),
        ([-0.5, -0.175, -0.5], [1.0, 0.5, 0.125], iron),
        ([-0.5, -0.175, 0.375], [1.0, 0.5, 0.125], iron),
    ];
    if c.kind == 2 {
        parts.push(([-0.375, -0.175, -0.375], [0.75, 0.75, 0.75], stack_tile(ItemStack { id: 61, count: 1, damage: 0 })));
    }
    if c.kind == 1 {
        parts.push(([-0.375, -0.175, -0.375], [0.75, 0.75, 0.75], stack_tile(ItemStack { id: 54, count: 1, damage: 0 })));
    }
    for (min, size, tile) in parts {
        let n0 = verts.len();
        push_box(verts, idxs, min, [min[0] + size[0], min[1] + size[1], min[2] + size[2]], tile, 1.0);
        for v in verts[n0..].chunks_exact_mut(VF) {
            let w = root.transform_point3(Vec3::new(v[0], v[1], v[2]));
            v[..3].copy_from_slice(&w.to_array());
        }
    }
}

/// `RenderBoat` / `ModelBoat`: a floor and four low walls, turned to `180 - yaw` and rocking about x for a while after a hit
/// (`sin(t) * t * damage / 10 * direction` degrees). UNVERIFIED stand-in: planks instead of `item/boat.png`.
fn push_boat(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, b: &Boat, a: f32) {
    let t = b.hit_time as f32 - a;
    let rock = if t > 0.0 { (t.sin() * t * (b.damage as f32 - a).max(0.0) / 10.0 * b.rock as f32).to_radians() } else { 0.0 };
    let root = Mat4::from_translation(b.prev.lerp(b.pos, a)) * Mat4::from_rotation_y(PI - b.yaw.to_radians()) * Mat4::from_rotation_x(rock);
    let tile = stack_tile(ItemStack { id: 5, count: 1, damage: 0 });
    for (min, size) in [
        ([-0.75, -0.375, -0.5], [1.5, 0.25, 1.0]),
        ([-0.75, -0.1875, -0.625], [0.125, 0.375, 1.25]),
        ([0.625, -0.1875, -0.625], [0.125, 0.375, 1.25]),
        ([-0.625, -0.1875, -0.625], [1.25, 0.375, 0.125]),
        ([-0.625, -0.1875, 0.5], [1.25, 0.375, 0.125]),
    ] {
        let n0 = verts.len();
        push_box(verts, idxs, min, [min[0] + size[0], min[1] + size[1], min[2] + size[2]], tile, 1.0);
        for v in verts[n0..].chunks_exact_mut(VF) {
            let w = root.transform_point3(Vec3::new(v[0], v[1], v[2]));
            v[..3].copy_from_slice(&w.to_array());
        }
    }
}

/// The parts of `parts(m)` that are the head: those on its pivot (`head_n`: a cow's horns, a hen's bill and chin), else the first one; a squid and a slime have none.
fn is_head(k: crate::world::mobs::Kind, i: usize) -> bool {
    i < match k {
        Squid | Slime => 0,
        Cow | Chicken => head_n(k),
        _ => 1,
    }
}

/// Model y of the ground under a mob: the root translation of `push_mob` (the legs end at 24, 0.125 px under it).
const GROUND: f32 = 23.875;

/// NOT b1.7.3 (smart animals graze, root and peck, `Mob::head`): how far the head box `head` (parts[0]) is dipped, as (pitch in radians, shift
/// down in model pixels), so that its lowest front corner touches the ground at `m.head` = 1. The pitch is the base angle (cow, sheep 1.1, pig 0.6,
/// hen 0.9) x `m.head`, with a slow chew (+-0.08 rad) on the cow, sheep and pig and quick dips on the hen, both by `m.munch`. `None` for a head that
/// is up and for the kinds that do not eat from the ground. UNVERIFIED by eye: the angles and the bob.
fn head_dip(m: &Mob, head: &Part) -> Option<(f32, f32)> {
    let base = match m.kind {
        Cow | Sheep => 1.1,
        Pig => 0.6,
        Chicken => 0.9,
        _ => return None,
    };
    if m.head < 0.01 {
        return None;
    }
    let s = m.munch.sin();
    let a = m.head * if m.kind == Chicken { base * (0.35 + 0.65 * (0.5 + 0.5 * s)) } else { base + 0.08 * s };
    let (min, size, rp, ..) = *head;
    let low = (min[1] + size[1]) * a.cos() - min[2] * a.sin();
    Some((a, (GROUND - rp[1] - low).max(0.0) * m.head))
}

/// Mobs drawn like `RenderLiving`: turned to face their body (`body_yaw`), scaled 1/16 (times `size`) and flipped upside down (model y
/// points down), 24 px above the ground; parts turn about their rotation point, x then y then z, legs and arms swinging
/// with `limb`; each from the mob's own skin (`skin_of`), a sheep's fleece as a second pass (`fur`) tinted by its colour. UNVERIFIED
/// stand-ins: a hurt mob is tinted red (the Java blends a red overlay), a creeper's burning fuse flashes flat white.
fn push_mob(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, m: &Mob) {
    let feet = m.body.pos - Vec3::new(0.0, m.half.y, 0.0);
    let root = Mat4::from_translation(feet)
        * Mat4::from_rotation_y(PI - m.body_yaw.to_radians())
        * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0) * m.size / 16.0)
        * Mat4::from_translation(Vec3::new(0.0, -23.875, 0.0));
    let flash = m.fuse > 0 && m.fuse % 6 < 3;
    let red = if m.hurt > 0 { tint(1.0, 0.4, 0.4) } else { WHITE };
    let model = parts(m);
    let dip = model.first().and_then(|h| head_dip(m, h));
    draw_parts(verts, idxs, m, &root, &model, skin_of(m), red, flash, dip);
    let fleece_pass = fur(m);
    if !fleece_pass.is_empty() {
        let c = if m.hurt > 0 { tint(0.8, 0.3, 0.3) } else { fleece(m.color) };
        draw_parts(verts, idxs, m, &root, &fleece_pass, atlas::skin::SHEEP_FUR, c, false, dip);
    }
}

/// One pass of `push_mob`: `model` posed for `m` and drawn from skin `slot` (flat white for a `flash`).
fn draw_parts(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, m: &Mob, root: &Mat4, model: &[Part], slot: u8, tint: f32, flash: bool, dip: Option<(f32, f32)>) {
    // `ModelBase.setRotationAngles` for the head: turned by how far the head (`yaw`) is from the body (`body_yaw`, at most 75 degrees) and
    // tipped by `head_pitch`; a head that grazes keeps only the dip. In this model space (y and x mirrored) a positive turn is the head's lead.
    let lead = crate::world::mobs::lead(m.yaw, m.body_yaw).to_radians();
    let tip = m.head_pitch * (1.0 - m.head);
    for (i, &(min, size, mut rp, rot, sw, tx)) in model.iter().enumerate() {
        let mut rx = rot[0] + (m.limb * 0.6662 + sw[0]).cos() * sw[1] * m.limb_amt;
        let mut ry = rot[1];
        // The head and what hangs from its pivot (a cow's horns, a hen's bill and chin) lead the body, tip, and go down when it grazes.
        if is_head(m.kind, i) {
            (ry, rx) = (ry + lead, rx + tip);
            if let Some((a, shift)) = dip {
                rx += a;
                rp[1] += shift;
            }
        }
        let mat = *root * Mat4::from_translation(Vec3::from(rp)) * Mat4::from_rotation_z(rot[2]) * Mat4::from_rotation_y(ry) * Mat4::from_rotation_x(rx);
        if flash {
            let white = stack_tile(ItemStack { id: 35, count: 1, damage: 0 });
            let n0 = verts.len();
            push_box(verts, idxs, min, [min[0] + size[0], min[1] + size[1], min[2] + size[2]], white, 1.0);
            for v in verts[n0..].chunks_exact_mut(VF) {
                let w = mat.transform_point3(Vec3::new(v[0], v[1], v[2]));
                v[..3].copy_from_slice(&w.to_array());
            }
        } else {
            push_skin_box(verts, idxs, &mat, min, size, tx, slot, tint);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NOT b1.7.3: a head that is up is not dipped; with it down the nose rests on the ground (model y `GROUND`), never through it.
    #[test]
    fn a_grazing_head_dips_to_the_ground_not_through_it() {
        let mut rng = crate::world::gen::noise::JavaRandom::new(1);
        for kind in [Cow, Sheep, Pig, Chicken] {
            let mut m = Mob::new(kind, 0.0, 0.0, 0.0, 0.0, &mut rng);
            assert!(head_dip(&m, &parts(&m)[0]).is_none(), "{kind:?} dipped with its head up");
            m.head = 1.0;
            let ps = parts(&m);
            let (a, shift) = head_dip(&m, &ps[0]).unwrap();
            assert!(a > 0.3 && shift > 0.0, "{kind:?}: pitch {a}, shift {shift}");
            let (min, size, rp, ..) = ps[0];
            let low = rp[1] + shift + (min[1] + size[1]) * a.cos() - min[2] * a.sin();
            assert!((low - GROUND).abs() < 0.01, "{kind:?}: the nose is at {low}, the ground at {GROUND}");
        }
        let wolf = Mob::new(Wolf, 0.0, 0.0, 0.0, 0.0, &mut rng);
        assert!(head_dip(&wolf, &parts(&wolf)[0]).is_none());
    }

    /// Every box of every mob unwraps inside its 64 x 32 skin, and what `push_mob` draws reads that skin's atlas slot (not the flat strip,
    /// not another skin); a sheep wears fleece until it is shorn; a wolf's skin follows its state.
    #[test]
    fn mob_boxes_unwrap_inside_their_skin() {
        let mut rng = crate::world::gen::noise::JavaRandom::new(1);
        for kind in [Pig, Cow, Sheep, Chicken, Wolf, Squid, Zombie, PigZombie, Giant, Skeleton, Creeper, Spider, Slime] {
            let mut m = Mob::new(kind, 0.0, 0.0, 0.0, 0.0, &mut rng);
            let mut boxes = parts(&m);
            boxes.extend(fur(&m));
            for (min, size, _, _, _, tx) in &boxes {
                let (w, h) = (2.0 * (size[0] + size[2]), size[1] + size[2]);
                assert!(tx.o.0 + w <= 64.0 && tx.o.1 + h <= 32.0, "{kind:?}: box {min:?} {size:?} at {:?} leaves the skin", tx.o);
            }
            let (mut v, mut i) = (Vec::new(), Vec::new());
            push_mob(&mut v, &mut i, &m);
            assert_eq!(v.len(), boxes.len() * 24 * VF, "{kind:?}: 24 vertices a box");
            let slots: Vec<u8> = (0..26).collect();
            for vert in v.chunks_exact(VF) {
                let (x, y) = (vert[3] * atlas::ATLAS_W as f32, vert[4] * atlas::ATLAS_H as f32 - atlas::CHAR_Y as f32);
                assert!(y >= 0.0, "{kind:?}: a vertex reads above the skins");
                let slot = slots.iter().copied().find(|&s| x >= (s % 4) as f32 * 64.0 && x <= (s % 4) as f32 * 64.0 + 64.0 && y >= (s / 4) as f32 * 32.0 && y <= (s / 4) as f32 * 32.0 + 32.0).unwrap();
                assert!(slot == skin_of(&m) || (kind == Sheep && slot == atlas::skin::SHEEP_FUR), "{kind:?}: read slot {slot}");
            }
            m.sheared = true;
            assert!(fur(&m).is_empty());
        }
        let mut wolf = Mob::new(Wolf, 0.0, 0.0, 0.0, 0.0, &mut rng);
        assert_eq!(skin_of(&wolf), atlas::skin::WOLF);
        wolf.anger = 10;
        assert_eq!(skin_of(&wolf), atlas::skin::WOLF_ANGRY);
        wolf.tame = true;
        assert_eq!(skin_of(&wolf), atlas::skin::WOLF_TAME, "tamed beats angry, as `EntityWolf.getEntityTexture`");
        assert_eq!(parts(&wolf).len(), 11, "head, ears, snout, body, mane, four legs, tail");
        wolf.sitting = true;
        let sat = parts(&wolf);
        assert!(sat[4].2[1] > 16.0 && sat[8].3[0] > 5.0, "the body sinks to 18 and the hind legs fold forward");
        // Cow: head, two horns, body, four legs, udder; the horns follow the head's dip.
        let cow = Mob::new(Cow, 0.0, 0.0, 0.0, 0.0, &mut rng);
        assert_eq!((parts(&cow).len(), head_n(Cow)), (9, 3));
    }

    /// `tint` packs like `WHITE`; the fleece table and the red of a hurt mob go through it.
    #[test]
    fn tints_pack_like_white() {
        assert_eq!(tint(1.0, 1.0, 1.0), WHITE);
        assert_eq!(fleece(0), WHITE);
        assert_eq!(fleece(15), tint(0.1, 0.1, 0.1));
        assert_eq!(tint(1.0, 0.0, 0.0), 16_711_680.0);
        assert_eq!(fleece(16), fleece(0), "the colour is a nibble");
    }

    fn text() -> Text {
        ["Hello".into(), "world".into(), String::new(), "!".into()]
    }

    /// What `push_sign` adds for `s` with the cache `c`, starting from `used` text quads: vertices, indices, the quad count after.
    fn drawn(s: &SignView, c: &mut SignCache, now: Instant, used: usize, with_text: bool) -> (Vec<f32>, Vec<u32>, usize) {
        let (mut v, mut i, mut q) = (Vec::new(), Vec::new(), used);
        push_sign(&mut v, &mut i, &mut q, s, with_text, c, now);
        (v, i, q)
    }

    /// A text kept from an earlier frame is, vertex for vertex, the one built fresh, and the indices follow what is already in the
    /// buffer.
    #[test]
    fn a_kept_sign_text_is_the_one_built_fresh() {
        let t = text();
        let s = SignView { pos: (3, 64, -7), id: decor::POST, meta: 5, text: &t };
        let (now, mut cache) = (Instant::now(), SignCache::default());
        let first = drawn(&s, &mut cache, now, 0, true);
        assert!(first.2 > 20, "the lines have lit pixels");
        assert_eq!(cache.len(), 1);
        assert_eq!(drawn(&s, &mut cache, now, 0, true), first, "a hit");
        assert_eq!(drawn(&s, &mut SignCache::default(), now, 0, true), first, "a fresh build");
        // After the model's own vertices the text indices start at the first text vertex.
        let model = drawn(&s, &mut cache, now, 0, false);
        assert_eq!(first.0[..model.0.len()], model.0[..]);
        let base = (model.0.len() / VF) as u32;
        assert_eq!(first.1[model.1.len()..].iter().min(), Some(&base));
        assert_eq!(first.0.len() / VF - model.0.len() / VF, first.2 * 4);
    }

    /// Edited text, another block or another side rebuild the entry of that cell; the cache keeps one entry a cell.
    #[test]
    fn a_changed_sign_is_rebuilt() {
        let (t, mut t2) = (text(), text());
        t2[2] = "more".into();
        let (now, mut cache) = (Instant::now(), SignCache::default());
        let a = drawn(&SignView { pos: (0, 70, 0), id: decor::POST, meta: 0, text: &t }, &mut cache, now, 0, true);
        let b = drawn(&SignView { pos: (0, 70, 0), id: decor::POST, meta: 0, text: &t2 }, &mut cache, now, 0, true);
        assert!(b.2 > a.2, "the third line is drawn now");
        let c = drawn(&SignView { pos: (0, 70, 0), id: decor::POST, meta: 4, text: &t2 }, &mut cache, now, 0, true);
        assert_ne!(b.0, c.0, "turned");
        assert_eq!(b.2, c.2);
        assert_eq!(cache.len(), 1);
    }

    /// Under the cap the text is cut off after its first quads, as drawing it quad by quad until the cap did.
    #[test]
    fn the_text_quad_cap_keeps_the_first_quads() {
        let t = text();
        let s = SignView { pos: (1, 64, 1), id: decor::POST, meta: 0, text: &t };
        let (now, mut cache) = (Instant::now(), SignCache::default());
        let all = drawn(&s, &mut cache, now, 0, true);
        let model = drawn(&s, &mut cache, now, 0, false);
        let cut = drawn(&s, &mut cache, now, TEXT_QUADS - 3, true);
        assert_eq!(cut.2, TEXT_QUADS);
        assert_eq!(cut.0.len() - model.0.len(), 3 * 4 * VF);
        assert_eq!(cut.0[model.0.len()..], all.0[model.0.len()..model.0.len() + 3 * 4 * VF]);
        assert_eq!(drawn(&s, &mut cache, now, TEXT_QUADS, true).0.len(), model.0.len(), "no room: the model only");
    }

    /// An entry nobody drew for five seconds goes; one drawn since stays.
    #[test]
    fn unused_sign_text_expires() {
        let t = text();
        let now = Instant::now();
        let mut cache = SignCache::default();
        drawn(&SignView { pos: (0, 64, 0), id: decor::POST, meta: 0, text: &t }, &mut cache, now, 0, true);
        drawn(&SignView { pos: (9, 64, 0), id: decor::POST, meta: 0, text: &t }, &mut cache, now + Duration::from_secs(4), 0, true);
        cache.sweep(now + Duration::from_secs(4));
        assert_eq!(cache.len(), 2);
        cache.sweep(now + Duration::from_secs(6));
        assert_eq!(cache.len(), 1, "only the one drawn at 4 s is left");
        cache.sweep(now + Duration::from_secs(10));
        assert_eq!(cache.len(), 0);
    }
}
