//! Dropped items: a 0.25 cube per `ItemEntity` in the flat colour of its item, hovering and bobbing like
//! `RenderItem` (`sin(age / 10) * 0.1 + 0.1`), drawn with the chunk pipeline from one dynamic buffer pair.
//! ponytail: no spin (needs rotated boxes), flat full brightness, no 2nd/3rd copy for big stacks; M14 draws
//! real item sprites.

use crate::render::atlas;
use crate::render::mesh::{lm, push_box, push_face, VF, WHITE};
use crate::world::boat::Boat;
use crate::world::minecart::Cart;
use crate::world::decor::{self, Painting, Text};
use crate::world::chunk::brightness;
use crate::world::effects::{Effects, Fx, MAX_BOLTS, MAX_PARTICLES};
use glam::{Mat4, Vec3};

use crate::world::items::{stack_tile, Drops, ItemStack, MAX_ITEMS};
use crate::world::mobs::{Kind::*, Mob, Mobs, MAX_ARROWS, MAX_MOBS};
use crate::world::ticks::{Falling, MAX_FALLING};

/// 24 vertices x 7 floats (`mesh::VF`) per cube, 36 indices per cube.
/// A mob is at most 12 boxes (spider 11), an arrow one, a falling block one.
/// Boats drawn at once (5 boxes each).
const MAX_BOATS: usize = 16;
/// Carts drawn at once (floor, four walls and the furnace: 6 boxes each).
const MAX_CARTS: usize = 16;
const BOXES: usize = MAX_ITEMS * 2 + MAX_MOBS * 12 + MAX_ARROWS + MAX_FALLING + MAX_PARTICLES + MAX_BOLTS + MAX_BOATS * 5 + MAX_CARTS * 6 + 1;
/// Paintings (16 px pieces, one face each), signs (a board and a post of 6 two-sided faces) and the sign text (one quad per run
/// of lit font pixels), on top of the boxes.
const MAX_SIGNS: usize = 32;
const PIECES: usize = 512;
const TEXT_QUADS: usize = 4096;
// The player: 42 quads of 4 vertices, both windings (12 indices) each, and an item of at most 2 boxes.
const EXTRA_V: usize = PIECES * 4 + MAX_SIGNS * 2 * 24 + TEXT_QUADS * 4 + 256;
const EXTRA_I: usize = PIECES * 6 + MAX_SIGNS * 2 * 72 + TEXT_QUADS * 6 + 640;
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
    pub vbuf: wgpu::Buffer,
    pub ibuf: wgpu::Buffer,
}

impl ItemMesh {
    pub fn new(device: &wgpu::Device) -> Self {
        let buf = |label, size, usage| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        Self { vbuf: buf("items_vbuf", VERTEX_BYTES, wgpu::BufferUsages::VERTEX), ibuf: buf("items_ibuf", INDEX_BYTES, wgpu::BufferUsages::INDEX) }
    }

    /// Upload this frame's items; returns the index count to draw.
    /// `lv` = raw (sky, block) light of a cell: every entity is lit by its own cell.
    pub fn update(&self, queue: &wgpu::Queue, drops: &Drops, mobs: &Mobs, falling: &[Falling], fx: &Effects, lv: &dyn Fn(i32, i32, i32) -> (u8, u8), hook: Option<Vec3>, paintings: &[Painting], signs: &[SignView], boats: (&[Boat], f32), carts: (&[Cart], f32), player: Option<&crate::render::player::View>, eye: Vec3, seen: &dyn Fn(Vec3, f32) -> bool) -> u32 {
        let (verts, idxs) = geometry(drops, mobs, falling, fx, lv, hook, paintings, signs, boats, carts, player, eye, seen);
        if idxs.is_empty() {
            return 0;
        }
        queue.write_buffer(&self.vbuf, 0, bytemuck::cast_slice(&verts));
        queue.write_buffer(&self.ibuf, 0, bytemuck::cast_slice(&idxs));
        idxs.len() as u32
    }
}

/// Light the boxes pushed since vertex float `from` like `Entity.getBrightnessForRender`: the levels of the cell `at`, in the
/// mesher's `lm` (the day's share is the `amb.x` uniform, as for terrain). ponytail: `light` is the brighter of the two levels.
fn lit(verts: &mut [f32], from: usize, lv: &dyn Fn(i32, i32, i32) -> (u8, u8), at: Vec3) {
    let (sky, block) = lv(at.x.floor() as i32, at.y.floor() as i32, at.z.floor() as i32);
    let b = brightness(sky.max(block));
    for v in verts[from..].chunks_exact_mut(VF) {
        v[5] = b;
        v[6] = lm(sky, block);
    }
}

fn geometry(drops: &Drops, mobs: &Mobs, falling: &[Falling], fx: &Effects, lv: &dyn Fn(i32, i32, i32) -> (u8, u8), hook: Option<Vec3>, paintings: &[Painting], signs: &[SignView], boats: (&[Boat], f32), carts: (&[Cart], f32), player: Option<&crate::render::player::View>, eye: Vec3, seen: &dyn Fn(Vec3, f32) -> bool) -> (Vec<f32>, Vec<u32>) {
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
        crate::render::player::push(&mut verts, &mut idxs, &p.root, &p.pose, p.held);
        lit(&mut verts, n0, lv, p.at);
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
    for s in signs.iter().take(MAX_SIGNS) {
        let c = Vec3::new(s.pos.0 as f32 + 0.5, s.pos.1 as f32 + 0.5, s.pos.2 as f32 + 0.5);
        let d = c.distance_squared(eye);
        if d < 64.0 * 64.0 {
            let n0 = verts.len();
            push_sign(&mut verts, &mut idxs, &mut quads, s, d < 16.0 * 16.0);
            lit(&mut verts, n0, lv, c);
        }
    }
    (verts, idxs)
}

/// `TileEntitySignRenderer.renderTileEntitySignAt`: the board (and the post of a standing sign) from `item/sign.png`, 2/3 scale,
/// turned by the metadata (post: 22.5 degrees a step; wall: the side), then the four lines in black from the HUD font's glyph
/// runs, 1/60 of that scale per font pixel, centred on the board. ponytail: the model is drawn two-sided (no culling choice),
/// text is flat black quads (no `FontRenderer` texture) and only within 16 blocks.
fn push_sign(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, quads: &mut usize, s: &SignView, text: bool) {
    let sc = 2.0 / 3.0;
    let wall = s.id != decor::POST;
    let turn = if wall { -match s.meta { 2 => 180.0f32, 4 => 90.0, 5 => -90.0, _ => 0.0 } } else { -(s.meta as f32) * 22.5 };
    let mut root = Mat4::from_translation(Vec3::new(s.pos.0 as f32 + 0.5, s.pos.1 as f32 + 12.0 / 16.0 * sc, s.pos.2 as f32 + 0.5)) * Mat4::from_rotation_y(turn.to_radians());
    if wall {
        root *= Mat4::from_translation(Vec3::new(0.0, -5.0 / 16.0, -7.0 / 16.0));
    }
    let model = root * Mat4::from_scale(Vec3::new(sc, -sc, -sc) / 16.0);
    push_model_box(verts, idxs, &model, [-12.0, -14.0, -1.0], [24.0, 12.0, 2.0], (0.0, 0.0));
    if !wall {
        push_model_box(verts, idxs, &model, [-1.0, -2.0, -1.0], [2.0, 14.0, 2.0], (0.0, 14.0));
    }
    if !text {
        return;
    }
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
                    if *quads >= TEXT_QUADS {
                        return;
                    }
                    *quads += 1;
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

/// One model box: (min, size, rotation point, rotation x/y/z, swing phase + amplitude, wool colour or `ME`), in model pixels.
type Part = ([f32; 3], [f32; 3], [f32; 3], [f32; 3], [f32; 2], u8);
const ME: u8 = 255;
use std::f32::consts::{FRAC_PI_2 as Q, FRAC_PI_4 as E, PI};

fn pt(min: [f32; 3], size: [f32; 3], rp: [f32; 3], rot: [f32; 3], sw: [f32; 2], col: u8) -> Part {
    (min, size, rp, rot, sw, col)
}

/// `ModelQuadruped`: head, the body lying down, four legs (`cos(limb x 0.6662 + phase) x 1.4 x amount`); one leg length `l`.
fn quad(head: ([f32; 3], [f32; 3], [f32; 3]), body: ([f32; 3], [f32; 3], [f32; 3]), l: f32, x: f32, back: f32, front: f32, col: [u8; 3]) -> Vec<Part> {
    let leg = |rx: f32, z: f32, ph: f32| pt([-2.0, 0.0, -2.0], [4.0, l, 4.0], [rx, 24.0 - l, z], [0.0; 3], [ph, 1.4], col[2]);
    vec![
        pt(head.0, head.1, head.2, [0.0; 3], [0.0; 2], col[0]),
        pt(body.0, body.1, body.2, [Q, 0.0, 0.0], [0.0; 2], col[1]),
        leg(-x, back, 0.0), leg(x, back, PI), leg(-x, front, PI), leg(x, front, 0.0),
    ]
}

/// `ModelBiped`-shaped (zombie, skeleton, giant): arms `w` wide, raised when `raised` (`ModelZombie`).
fn biped(w: f32, raised: bool) -> Vec<Part> {
    let (o, a) = (if w == 4.0 { 0.0 } else { 1.0 }, if raised { -Q } else { 0.0 });
    let amp = if raised { 0.0 } else { 1.0 };
    vec![
        pt([-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], [0.0; 3], [0.0; 3], [0.0; 2], ME),
        pt([-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], [0.0; 3], [0.0; 3], [0.0; 2], ME),
        pt([-3.0 + 2.0 * o, -2.0, -2.0 + o], [w, 12.0, w], [-5.0, 2.0, 0.0], [a, 0.0, 0.0], [PI, amp], ME),
        pt([-1.0, -2.0, -2.0 + o], [w, 12.0, w], [5.0, 2.0, 0.0], [a, 0.0, 0.0], [0.0, amp], ME),
        pt([-2.0 + o, 0.0, -2.0 + o], [w, 12.0, w], [-2.0, 12.0, 0.0], [0.0; 3], [0.0, 1.4], ME),
        pt([-2.0 + o, 0.0, -2.0 + o], [w, 12.0, w], [2.0, 12.0, 0.0], [0.0; 3], [PI, 1.4], ME),
    ]
}

fn parts(m: &Mob) -> Vec<Part> {
    match m.kind {
        Pig => quad(([-4.0, -4.0, -8.0], [8.0, 8.0, 8.0], [0.0, 12.0, -6.0]), ([-5.0, -10.0, -7.0], [10.0, 16.0, 8.0], [0.0, 11.0, 2.0]), 6.0, 3.0, 7.0, -5.0, [ME; 3]),
        Cow => quad(([-4.0, -4.0, -6.0], [8.0, 8.0, 6.0], [0.0, 4.0, -8.0]), ([-6.0, -10.0, -7.0], [12.0, 18.0, 10.0], [0.0, 5.0, 2.0]), 12.0, 4.0, 7.0, -6.0, [ME; 3]),
        // Sheep: the body is fleece, head and legs the grey skin.
        Sheep => quad(([-3.0, -4.0, -6.0], [6.0, 6.0, 8.0], [0.0, 6.0, -8.0]), ([-4.0, -10.0, -7.0], [8.0, 16.0, 6.0], [0.0, 5.0, 2.0]), 12.0, 3.0, 7.0, -5.0, [8, 255, 8]),
        Wolf => quad(([-3.0, -3.0, -4.0], [6.0, 6.0, 4.0], [0.0, 13.5, -7.0]), ([-3.0, -3.0, -3.0], [6.0, 9.0, 6.0], [0.0, 14.0, 2.0]), 8.0, 2.0, 7.0, -4.0, [ME; 3]),
        Chicken => vec![
            pt([-2.0, -6.0, -2.0], [4.0, 6.0, 3.0], [0.0, 15.0, -4.0], [0.0; 3], [0.0; 2], ME),
            pt([-2.0, -4.0, -4.0], [4.0, 2.0, 2.0], [0.0, 15.0, -4.0], [0.0; 3], [0.0; 2], 4),
            pt([-3.0, -4.0, -3.0], [6.0, 8.0, 6.0], [0.0, 16.0, 0.0], [Q, 0.0, 0.0], [0.0; 2], ME),
            pt([-1.0, 0.0, -3.0], [3.0, 5.0, 3.0], [-2.0, 19.0, 1.0], [0.0; 3], [0.0, 1.4], 4),
            pt([-1.0, 0.0, -3.0], [3.0, 5.0, 3.0], [1.0, 19.0, 1.0], [0.0; 3], [PI, 1.4], 4),
            pt([0.0, 0.0, -3.0], [1.0, 4.0, 6.0], [-4.0, 13.0, 0.0], [0.0; 3], [0.0; 2], ME),
            pt([0.0, 0.0, -3.0], [1.0, 4.0, 6.0], [3.0, 13.0, 0.0], [0.0; 3], [0.0; 2], ME),
        ],
        Squid => {
            let mut v = vec![pt([-6.0, -8.0, -6.0], [12.0, 16.0, 12.0], [0.0, 8.0, 0.0], [0.0; 3], [0.0; 2], ME)];
            v.extend((0..8).map(|i| {
                let a = i as f32 * E;
                pt([-1.0, 0.0, -1.0], [2.0, 18.0, 2.0], [a.cos() * 5.0, 15.0, a.sin() * 5.0], [0.0; 3], [0.0; 2], ME)
            }));
            v
        }
        Zombie | PigZombie | Giant => biped(4.0, true),
        Skeleton => biped(2.0, false),
        Creeper => {
            let leg = |x: f32, z: f32, ph: f32| pt([-2.0, 0.0, -2.0], [4.0, 6.0, 4.0], [x, 18.0, z], [0.0; 3], [ph, 1.4], ME);
            vec![
                pt([-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], [0.0, 4.0, 0.0], [0.0; 3], [0.0; 2], ME),
                pt([-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], [0.0, 4.0, 0.0], [0.0; 3], [0.0; 2], ME),
                leg(-2.0, 4.0, 0.0), leg(2.0, 4.0, PI), leg(-2.0, -4.0, PI), leg(2.0, -4.0, 0.0),
            ]
        }
        Spider => {
            let mut v = vec![
                pt([-4.0, -4.0, -8.0], [8.0, 8.0, 8.0], [0.0, 15.0, -3.0], [0.0; 3], [0.0; 2], ME),
                pt([-3.0, -3.0, -3.0], [6.0, 6.0, 6.0], [0.0, 15.0, 0.0], [0.0; 3], [0.0; 2], ME),
                pt([-5.0, -4.0, -6.0], [10.0, 8.0, 12.0], [0.0, 15.0, 9.0], [0.0; 3], [0.0; 2], ME),
            ];
            let (ry, rz) = ([E, -E, E / 2.0, -E / 2.0, -E / 2.0, E / 2.0, -E, E], [-E, E, -0.58, 0.58, -0.58, 0.58, -E, E]);
            v.extend((0..8).map(|i| {
                let left = i % 2 == 1;
                pt([if left { -1.0 } else { -15.0 }, -1.0, -1.0], [16.0, 2.0, 2.0], [if left { 4.0 } else { -4.0 }, 15.0, 2.0 - (i / 2) as f32], [0.0, ry[i], rz[i]], [0.0; 2], ME)
            }));
            v
        }
        Slime => vec![pt([-5.0, 14.0, -5.0], [10.0, 10.0, 10.0], [0.0; 3], [0.0; 3], [0.0; 2], ME)],
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

/// Mobs drawn like `RenderLiving`: turned to face their yaw, scaled 1/16 (times `size`) and flipped upside down (model y
/// points down), 24 px above the ground; parts turn about their rotation point, x then y then z, legs and arms swinging
/// with `limb`. UNVERIFIED stand-ins: one flat wool colour per kind (`mobs::spec`), red (wool 14) while hurt, white while a
/// creeper's fuse burns; the real textures come with M14.
fn push_mob(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, m: &Mob) {
    let feet = m.body.pos - Vec3::new(0.0, m.half.y, 0.0);
    let root = Mat4::from_translation(feet)
        * Mat4::from_rotation_y(PI - m.yaw.to_radians())
        * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0) * m.size / 16.0)
        * Mat4::from_translation(Vec3::new(0.0, -23.875, 0.0));
    let own = match m.kind {
        Sheep if m.sheared => 8, // bare skin
        Sheep => m.color,
        k => crate::world::mobs::colour(k),
    };
    let tint = if m.hurt > 0 { Some(14) } else if m.fuse > 0 && m.fuse % 6 < 3 { Some(0) } else { None };
    for (min, size, rp, rot, sw, col) in parts(m) {
        let tile = stack_tile(ItemStack { id: 35, count: 1, damage: tint.unwrap_or(if col == ME { own } else { col }) as u16 });
        let rx = rot[0] + (m.limb * 0.6662 + sw[0]).cos() * sw[1] * m.limb_amt;
        let mat = root * Mat4::from_translation(Vec3::from(rp)) * Mat4::from_rotation_z(rot[2]) * Mat4::from_rotation_y(rot[1]) * Mat4::from_rotation_x(rx);
        let n0 = verts.len();
        push_box(verts, idxs, min, [min[0] + size[0], min[1] + size[1], min[2] + size[2]], tile, 1.0);
        for v in verts[n0..].chunks_exact_mut(VF) {
            let w = mat.transform_point3(Vec3::new(v[0], v[1], v[2]));
            v[..3].copy_from_slice(&w.to_array());
        }
    }
}
