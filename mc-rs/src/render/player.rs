//! The player model: `ModelBiped` (head, hat, body, two arms, two legs, `mob/char.png`) with `setRotationAngles`, drawn like
//! `RenderPlayer` / `RenderLiving` (third-person view) and like `GuiInventory` (the preview in the inventory panel).
//! The geometry is one function (`quads`) that both targets use: the world pushes it into the entity mesh, the HUD projects it.
//! Anim state is `EntityLiving.onUpdate` (body yaw follows the walk, limb swing) + `EntityPlayer` arm swing.
//! ponytail: no armor layers (`modelArmor*`, 10 more textures), ears, cloak, hurt flash or sleeping pose; the third-person held item is a flat
//! sprite or a cube, not the extruded `ItemRenderer` model (the first-person hand has it); the inventory preview looks straight ahead (no mouse on touch).
//! UNVERIFIED: written without a Rust toolchain: not compiled, not run.

use crate::render::atlas::{self, ITEM_TILE0};
use crate::render::mesh::{lm, push_box, VF, WHITE};
use crate::world::craft::{self, Kind};
use crate::world::items::{stack_tile, ItemStack};
use glam::{Mat4, Vec3};
use std::f32::consts::PI;

/// What `ModelBiped.setRotationAngles` reads, in the Java's units (angles in degrees, `limb` = `field_703_S`-style swing phase).
#[derive(Clone, Copy, Default)]
pub struct Pose {
    /// `rotationYaw - renderYawOffset`: the head's turn against the body.
    pub head_yaw: f32,
    pub pitch: f32,
    pub limb: f32,
    pub limb_amt: f32,
    /// `ticksExisted + partial`: the idle sway of the arms.
    pub age: f32,
    /// `onGround` of the model = swing progress 0..1.
    pub swing: f32,
    pub sneak: bool,
    pub riding: bool,
    /// `field_1278_i`: an item in hand lowers the right arm's swing.
    pub holding: bool,
}

/// The local player as the entity mesh draws him in the third-person view.
pub struct View {
    pub root: Mat4,
    pub pose: Pose,
    pub held: Option<ItemStack>,
    /// Where he stands (his light).
    pub at: Vec3,
}

/// One `ModelRenderer`: texture offset, box (model pixels, y down), inflation, mirror, rotation point and angles.
struct Limb {
    tex: (f32, f32),
    min: [f32; 3],
    size: [f32; 3],
    grow: f32,
    mirror: bool,
    rp: [f32; 3],
    rot: [f32; 3],
}

const HEAD: usize = 0;
const BODY: usize = 2;
const R_ARM: usize = 3;

fn limb(tex: (f32, f32), min: [f32; 3], size: [f32; 3], grow: f32, mirror: bool, rp: [f32; 3]) -> Limb {
    Limb { tex, min, size, grow, mirror, rp, rot: [0.0; 3] }
}

/// The seven parts of `new ModelBiped(0.0F)` with `setRotationAngles` applied: head, headwear, body, right arm, left arm, right
/// leg, left leg.
fn limbs(p: &Pose) -> [Limb; 7] {
    let mut l = [
        limb((0.0, 0.0), [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], 0.0, false, [0.0, 0.0, 0.0]),
        limb((32.0, 0.0), [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], 0.5, false, [0.0, 0.0, 0.0]),
        limb((16.0, 16.0), [-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], 0.0, false, [0.0, 0.0, 0.0]),
        limb((40.0, 16.0), [-3.0, -2.0, -2.0], [4.0, 12.0, 4.0], 0.0, false, [-5.0, 2.0, 0.0]),
        limb((40.0, 16.0), [-1.0, -2.0, -2.0], [4.0, 12.0, 4.0], 0.0, true, [5.0, 2.0, 0.0]),
        limb((0.0, 16.0), [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], 0.0, false, [-2.0, 12.0, 0.0]),
        limb((0.0, 16.0), [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], 0.0, true, [2.0, 12.0, 0.0]),
    ];
    let (hy, hx) = (p.head_yaw.to_radians(), p.pitch.to_radians());
    let (a, m) = (p.limb * 0.6662, p.limb_amt);
    let (mut rax, mut lax) = ((a + PI).cos() * 2.0 * m * 0.5, a.cos() * 2.0 * m * 0.5);
    let (mut rlx, mut llx) = (a.cos() * 1.4 * m, (a + PI).cos() * 1.4 * m);
    let (mut rly, mut lly) = (0.0, 0.0);
    if p.riding {
        rax += PI * -0.2;
        lax += PI * -0.2;
        (rlx, llx, rly, lly) = (PI * -0.4, PI * -0.4, PI * 0.1, PI * -0.1);
    }
    if p.holding {
        rax = rax * 0.5 - PI * 0.1;
    }
    let (mut ray, mut lay, mut raz, mut laz) = (0.0, 0.0, 0.0, 0.0);
    // The swing (`onGround > -9990` is always true for a living entity: with 0 it leaves the arms where they were).
    let by = (p.swing.sqrt() * PI * 2.0).sin() * 0.2;
    l[R_ARM].rp[2] = by.sin() * 5.0;
    l[R_ARM].rp[0] = -by.cos() * 5.0;
    l[R_ARM + 1].rp[2] = -by.sin() * 5.0;
    l[R_ARM + 1].rp[0] = by.cos() * 5.0;
    ray += by;
    lay += by;
    lax += by;
    let s = 1.0 - p.swing;
    let s = 1.0 - s * s * s * s;
    let f = (p.swing * PI).sin() * -(hx - 0.7) * (12.0 / 16.0);
    rax -= (s * PI).sin() * 1.2 + f;
    ray += by * 2.0;
    raz = (p.swing * PI).sin() * -0.4 + raz;
    let mut bx = 0.0;
    if p.sneak {
        bx = 0.5;
        rax += 0.4;
        lax += 0.4;
        l[5].rp[2] = 4.0;
        l[6].rp[2] = 4.0;
        l[5].rp[1] = 9.0;
        l[6].rp[1] = 9.0;
        l[HEAD].rp[1] = 1.0; // the hat keeps its rotation point (`bipedHeadwear` is not moved in the Java)
    }
    raz += (p.age * 0.09).cos() * 0.05 + 0.05;
    laz -= (p.age * 0.09).cos() * 0.05 + 0.05;
    rax += (p.age * 0.067).sin() * 0.05;
    lax -= (p.age * 0.067).sin() * 0.05;
    l[HEAD].rot = [hx, hy, 0.0];
    l[1].rot = [hx, hy, 0.0];
    l[BODY].rot = [bx, by, 0.0];
    l[R_ARM].rot = [rax, ray, raz];
    l[R_ARM + 1].rot = [lax, lay, laz];
    l[5].rot = [rlx, rly, 0.0];
    l[6].rot = [llx, lly, 0.0];
    l
}

/// `ModelRenderer.render`: translate to the rotation point, then z, y, x.
fn matrix(l: &Limb) -> Mat4 {
    Mat4::from_translation(Vec3::from(l.rp)) * Mat4::from_rotation_z(l.rot[2]) * Mat4::from_rotation_y(l.rot[1]) * Mat4::from_rotation_x(l.rot[0])
}

/// A model quad: four corners and their skin pixels (`mob/char.png`, 64 x 32, y down).
pub struct Quad {
    pub p: [Vec3; 4],
    pub uv: [[f32; 2]; 4],
}

/// `ModelRenderer.addBox` + `TexturedQuad` (0.1 px inset, `flipFace` when mirrored) for every part, through `root`.
pub fn quads(root: &Mat4, pose: &Pose) -> Vec<Quad> {
    let mut out = Vec::with_capacity(42);
    for l in limbs(pose) {
        let m = *root * matrix(&l);
        let ([dx, dy, dz], (ox, oy), g) = (l.size, l.tex, l.grow);
        let (mut x1, y1, z1) = (l.min[0] - g, l.min[1] - g, l.min[2] - g);
        let (mut x2, y2, z2) = (l.min[0] + dx + g, l.min[1] + dy + g, l.min[2] + dz + g);
        if l.mirror {
            std::mem::swap(&mut x1, &mut x2);
        }
        let c = [[x1, y1, z1], [x2, y1, z1], [x2, y2, z1], [x1, y2, z1], [x1, y1, z2], [x2, y1, z2], [x2, y2, z2], [x1, y2, z2]];
        // `ModelRenderer.faces`: corner indices and the texture rectangle (u0, v0, u1, v1) of each.
        let faces: [([usize; 4], [f32; 4]); 6] = [
            ([5, 1, 2, 6], [ox + dz + dx, oy + dz, ox + dz + dx + dz, oy + dz + dy]),
            ([0, 4, 7, 3], [ox, oy + dz, ox + dz, oy + dz + dy]),
            ([5, 4, 0, 1], [ox + dz, oy, ox + dz + dx, oy + dz]),
            ([2, 3, 7, 6], [ox + dz + dx, oy, ox + dz + dx + dx, oy + dz]),
            ([1, 0, 3, 2], [ox + dz, oy + dz, ox + dz + dx, oy + dz + dy]),
            ([4, 5, 6, 7], [ox + dz + dx + dz, oy + dz, ox + dz + dx + dz + dx, oy + dz + dy]),
        ];
        for (vi, [u0, v0, u1, v1]) in faces {
            let uv = [[u1 - 0.1, v0 + 0.1], [u0 + 0.1, v0 + 0.1], [u0 + 0.1, v1 - 0.1], [u1 - 0.1, v1 - 0.1]];
            let mut q = Quad { p: std::array::from_fn(|k| m.transform_point3(Vec3::from(c[vi[k]]))), uv };
            if l.mirror {
                q.p.reverse();
                q.uv.reverse();
            }
            out.push(q);
        }
    }
    out
}

/// `RenderLiving.doRenderLiving` as a matrix from model pixels to the world: feet at `feet`, turned `180 - body_yaw` degrees, flipped
/// (model y is down), 1/16 and `RenderPlayer.preRenderCallback`'s 15/16, lifted 24 px less 1/128.
pub fn root(feet: Vec3, body_yaw: f32) -> Mat4 {
    Mat4::from_translation(feet) * Mat4::from_rotation_y((180.0 - body_yaw).to_radians()) * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0) * (15.0 / 16.0 / 16.0)) * Mat4::from_translation(Vec3::new(0.0, -23.875, 0.0))
}

/// The player at the given pose into the entity mesh (both windings: the Java draws with culling off), then his item.
/// Vertices are `[x, y, z, u, v, light, lm, tint]`; the caller lights them (`items::lit`).
pub fn push(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, root: &Mat4, pose: &Pose, held: Option<ItemStack>) {
    push_quads(verts, idxs, &quads(root, pose));
    if let Some(s) = held {
        let l = limbs(pose);
        let hand = *root * matrix(&l[R_ARM]) * Mat4::from_translation(Vec3::new(-1.0, 7.0, 1.0)); // `postRender` + (-1/16, 7/16, 1/16)
        push_held(verts, idxs, &hand, s);
    }
}

/// Skin quads into the entity mesh, both windings.
fn push_quads(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, qs: &[Quad]) {
    for q in qs {
        let base = verts.len() as u32 / VF as u32;
        for (p, uv) in q.p.iter().zip(q.uv) {
            let (u, v) = atlas::char_px(uv[0], uv[1]);
            verts.extend_from_slice(&[p.x, p.y, p.z, u, v, 1.0, lm(15, 0), WHITE]);
        }
        idxs.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3, base, base + 2, base + 1, base, base + 3, base + 2]);
    }
}

/// Move the vertices (`[x, y, z, ..]` of `VF` floats) pushed since `verts` was sliced by `m`.
fn place(verts: &mut [f32], m: &Mat4) {
    for v in verts.chunks_exact_mut(VF) {
        let w = m.transform_point3(Vec3::new(v[0], v[1], v[2]));
        v[..3].copy_from_slice(&w.to_array());
    }
}

/// `RenderPlayer.renderSpecials`' three transforms for the item in the right hand (a block, a full-size tool, anything else). The
/// result is a cube (block) or a flat items.png sprite slab, moved by `hand` (model pixels, so the item's block units are x16).
fn push_held(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, hand: &Mat4, s: ItemStack) {
    let (m, n0) = (*hand, verts.len());
    let rx = |d: f32| Mat4::from_rotation_x(d.to_radians());
    let (m, tile, min, max) = if s.id < 256 && crate::world::chunk::normal_cube(s.id as u8) {
        let k = 0.5 * 12.0 / 16.0;
        (m * Mat4::from_translation(Vec3::new(0.0, 3.0, -5.0)) * rx(20.0) * Mat4::from_rotation_y(45.0_f32.to_radians()) * Mat4::from_scale(Vec3::new(k, -k, k)), stack_tile(s), [-0.5; 3], [0.5; 3])
    } else {
        let Some(icon) = atlas::item_icon(s.id) else { return };
        let t = ITEM_TILE0 + icon as u16;
        let full3d = craft::tool(s.id).is_some() || matches!(s.id, 280 | 352 | 353) || s.id == crate::world::gear::ROD;
        let m = if full3d {
            let k = 10.0 / 16.0;
            let spin = if matches!(craft::tool(s.id), Some((Kind::Sword, _))) || s.id == crate::world::gear::ROD { Mat4::from_rotation_z(PI) * Mat4::from_translation(Vec3::new(0.0, -2.0, 0.0)) } else { Mat4::IDENTITY };
            m * spin * Mat4::from_translation(Vec3::new(0.0, 3.0, 0.0)) * Mat4::from_scale(Vec3::new(k, -k, k)) * rx(-100.0) * Mat4::from_rotation_y(45.0_f32.to_radians())
        } else {
            m * Mat4::from_translation(Vec3::new(4.0, 3.0, -3.0)) * Mat4::from_scale(Vec3::splat(6.0 / 16.0)) * Mat4::from_rotation_z(60.0_f32.to_radians()) * rx(-90.0) * Mat4::from_rotation_z(20.0_f32.to_radians())
        };
        (m, t, [0.0, 0.0, -1.0 / 32.0], [1.0, 1.0, 1.0 / 32.0])
    };
    let m = m * Mat4::from_scale(Vec3::splat(16.0)); // item units are blocks, the arm is in pixels
    push_box(verts, idxs, min, max, tile, 1.0);
    place(&mut verts[n0..], &m);
}

// ---- the first-person hand ----

/// Hand-space squash toward the eye. The Java clears the depth buffer before the hand; the mesh shares the scene's depth, so the hand
/// is drawn at 0.3 of its distance instead (the same pixels), where no wall the player can stand at reaches it. ponytail: the item's
/// nearest corner (0.19 blocks) would pass the near plane (0.05) below 0.27; a pipeline with `depth_compare: Always` drops this.
const SQUASH: f32 = 0.3;

/// Eye space (as the view bob leaves it) to the world for the hand's geometry; `view` is the camera's, bob included.
pub fn hand_to_world(view: &Mat4, bob: &Mat4) -> Mat4 {
    view.inverse() * Mat4::from_scale(Vec3::splat(SQUASH)) * *bob
}

/// What `ItemRenderer.renderItemInFirstPerson` draws: the bare arm, or the item in hand (not the map).
pub struct Hand {
    pub to_world: Mat4,
    /// `equippedProgress` (1 = raised) and the swing progress, both for this frame.
    pub equip: f32,
    pub swing: f32,
    /// The item shown (it lags the hotbar while the hand is lowered).
    pub held: Option<ItemStack>,
    /// Where he stands (his light).
    pub at: Vec3,
}

/// A quad of the item mesh in both windings (the Java draws with culling off), corners `p` and atlas UVs, moved by `m`.
fn quad2(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, m: &Mat4, p: [[f32; 3]; 4], uv: [(f32, f32); 4]) {
    let base = verts.len() as u32 / VF as u32;
    for (c, (u, v)) in p.iter().zip(uv) {
        let w = m.transform_point3(Vec3::from(*c));
        verts.extend_from_slice(&[w.x, w.y, w.z, u, v, 1.0, lm(15, 0), WHITE]);
    }
    idxs.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3, base, base + 2, base + 1, base, base + 3, base + 2]);
}

/// `ItemRenderer.renderItem`'s sprite: both faces (mirrored like the Java's) and, for each texel column and row, a strip along the
/// four edges, 1/16 thick (z -1/16..0); the strip at step `i` shows texel `15 - i`.
fn sprite(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, m: &Mat4, icon: u8) {
    let (t, uv) = (1.0 / 16.0, |u: f32, v: f32| atlas::item_uv(icon, u, v));
    // `uv(u, v)`: u across the texture, v up (the Java's v runs down).
    quad2(verts, idxs, m, [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]], [uv(1.0, 0.0), uv(0.0, 0.0), uv(0.0, 1.0), uv(1.0, 1.0)]);
    quad2(verts, idxs, m, [[0.0, 1.0, -t], [1.0, 1.0, -t], [1.0, 0.0, -t], [0.0, 0.0, -t]], [uv(1.0, 1.0), uv(0.0, 1.0), uv(0.0, 0.0), uv(1.0, 0.0)]);
    for i in 0..16 {
        let (f, tex) = (i as f32 / 16.0, (15.5 - i as f32) / 16.0); // the column / row `15 - i`, at its centre
        let (x, row) = (f + t, (i as f32 + 0.5) / 16.0); // `row`: the same texel row, v up
        quad2(verts, idxs, m, [[f, 0.0, -t], [f, 0.0, 0.0], [f, 1.0, 0.0], [f, 1.0, -t]], [uv(tex, 0.0), uv(tex, 0.0), uv(tex, 1.0), uv(tex, 1.0)]);
        quad2(verts, idxs, m, [[x, 1.0, -t], [x, 1.0, 0.0], [x, 0.0, 0.0], [x, 0.0, -t]], [uv(tex, 1.0), uv(tex, 1.0), uv(tex, 0.0), uv(tex, 0.0)]);
        quad2(verts, idxs, m, [[0.0, x, 0.0], [1.0, x, 0.0], [1.0, x, -t], [0.0, x, -t]], [uv(1.0, row), uv(0.0, row), uv(0.0, row), uv(1.0, row)]);
        quad2(verts, idxs, m, [[1.0, f, 0.0], [0.0, f, 0.0], [0.0, f, -t], [1.0, f, -t]], [uv(0.0, row), uv(1.0, row), uv(1.0, row), uv(0.0, row)]);
    }
}

/// `renderItemInFirstPerson`: swing, raise and the fixed pose of the hand, in eye space after the view bob.
pub fn push_hand(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, h: &Hand) {
    let (s, up) = (h.swing, (1.0 - h.equip) * 0.6);
    let (a, b, c, d) = ((s * PI).sin(), (s.sqrt() * PI).sin(), (s.sqrt() * PI * 2.0).sin(), (s * s * PI).sin());
    let (t, rx, ry, rz) = (|x, y, z| Mat4::from_translation(Vec3::new(x, y, z)), |d: f32| Mat4::from_rotation_x(d.to_radians()), |d: f32| Mat4::from_rotation_y(d.to_radians()), |d: f32| Mat4::from_rotation_z(d.to_radians()));
    let Some(st) = h.held else {
        // The arm (`drawFirstPersonHand`: `setRotationAngles(0, ..)` = the default pose, the right arm alone, 1/16 a pixel).
        let m = h.to_world * t(-b * 0.3, c * 0.4, -a * 0.4) * t(0.64, -0.6 - up, -0.72) * ry(45.0) * ry(b * 70.0) * rz(-d * 20.0) * t(-1.0, 3.6, 3.5) * rz(120.0) * rx(200.0) * ry(-135.0) * t(5.6, 0.0, 0.0) * Mat4::from_scale(Vec3::splat(1.0 / 16.0));
        push_quads(verts, idxs, &quads(&m, &Pose::default())[R_ARM * 6..R_ARM * 6 + 6]);
        return;
    };
    let rod = if st.id == crate::world::gear::ROD { ry(180.0) } else { Mat4::IDENTITY }; // `shouldRotateAroundWhenRendering`
    let m = h.to_world * t(-b * 0.4, c * 0.2, -a * 0.2) * t(0.56, -0.52 - up, -0.72) * ry(45.0) * ry(-d * 20.0) * rz(-b * 20.0) * rx(-b * 80.0) * Mat4::from_scale(Vec3::splat(0.4)) * rod;
    if st.id < 256 && crate::world::chunk::normal_cube(st.id as u8) {
        let n0 = verts.len();
        push_box(verts, idxs, [-0.5; 3], [0.5; 3], stack_tile(st), 1.0);
        place(&mut verts[n0..], &m);
    } else if let Some(icon) = atlas::item_icon(st.id) {
        sprite(verts, idxs, &(m * t(0.0, -0.3, 0.0) * Mat4::from_scale(Vec3::splat(1.5)) * ry(50.0) * rz(335.0) * t(-15.0 / 16.0, -1.0 / 16.0, 0.0)), icon);
    }
}

/// `GuiInventory.drawGuiContainerBackgroundLayer`: the player at `(x, y)` (his feet; the panel's (51, 75)) with `k` px a GUI unit,
/// scale 30, front toward the viewer. `z` (larger = nearer) only orders the quads.
pub fn inventory_quads(x: f32, y: f32, k: f32, pose: &Pose) -> Vec<Quad> {
    let m = Mat4::from_translation(Vec3::new(x, y, 0.0)) * Mat4::from_scale(Vec3::new(30.0, 30.0, -30.0) * k * (15.0 / 16.0 / 16.0)) * Mat4::from_translation(Vec3::new(0.0, -23.875, 0.0));
    quads(&m, pose)
}

// ---- animation state ----

fn wrap(a: f32) -> f32 {
    (a + 180.0).rem_euclid(360.0) - 180.0
}

/// Interpolate angles in degrees the short way (`prevRotationYaw` is moved within 180 of `rotationYaw` each tick in the Java).
fn lerp_angle(prev: f32, cur: f32, t: f32) -> f32 {
    prev + wrap(cur - prev) * t
}

/// `EntityLiving.onUpdate` (body yaw, limb swing) and `EntityPlayer.onLivingUpdate` (arm swing) of the local player, at 20 Hz.
#[derive(Default)]
pub struct Anim {
    off: f32,
    prev_off: f32,
    rot: f32,
    prev_rot: f32,
    amt: f32,
    prev_amt: f32,
    phase: f32,
    swing_int: i32,
    swinging: bool,
    swing: f32,
    prev_swing: f32,
    last: Option<(f32, f32)>,
    age: u32,
    /// `distanceWalkedModified`, `field_774_f` (the walking amount) and `field_9328_R` (the air tilt, degrees), each with last tick's.
    walked: f32,
    prev_walked: f32,
    bob: f32,
    prev_bob: f32,
    tilt: f32,
    prev_tilt: f32,
    /// `ItemRenderer`: `equippedProgress`, the item it shows and the hotbar slot that was.
    eq: f32,
    prev_eq: f32,
    shown: Option<ItemStack>,
    slot: usize,
}

impl Anim {
    /// `EntityPlayer.swingItem`: the arm swings (a tap that places, hits or breaks, every dig tick). ponytail: the Java restarts at
    /// once, which would hold a dig's arm at progress 0 here; this waits for the swing in progress to end, so holding keeps swinging.
    /// The Java starts at -1, whose first tick is progress 0 (the arm has not moved yet); this starts at 0, so the arm moves on the
    /// first tick (50 ms sooner, 7 ticks of motion instead of 8).
    pub fn swing(&mut self) {
        if !self.swinging {
            self.swing_int = 0;
            self.swinging = true;
        }
    }

    /// One tick: the player's position `(x, z)`, `rotationYaw` in degrees, standing on something (not riding), his vertical speed in
    /// blocks a tick, and whether his steps count (`Entity.moveEntity`: not riding, not sneaking on the ground).
    pub fn tick(&mut self, x: f32, z: f32, yaw: f32, ground: bool, vy: f32, steps: bool) {
        (self.prev_swing, self.prev_off, self.prev_rot, self.prev_amt) = (self.swing, self.off, self.rot, self.amt);
        (self.prev_walked, self.prev_bob, self.prev_tilt) = (self.walked, self.bob, self.tilt);
        self.age += 1;
        if self.swinging {
            self.swing_int += 1;
            if self.swing_int >= 8 {
                self.swing_int = 0;
                self.swinging = false;
            }
        } else {
            self.swing_int = 0;
        }
        self.swing = self.swing_int as f32 / 8.0;
        self.rot = yaw;
        let (dx, dz) = self.last.map_or((0.0, 0.0), |(lx, lz)| (x - lx, z - lz));
        self.last = Some((x, z));
        let d = (dx * dx + dz * dz).sqrt();
        let mut target = self.off;
        if d > 0.05 {
            target = dz.atan2(dx).to_degrees() - 90.0;
        }
        if self.swing > 0.0 {
            target = self.rot;
        }
        self.off += wrap(target - self.off) * 0.3;
        let turn = wrap(self.rot - self.off).clamp(-75.0, 75.0);
        self.off = self.rot - turn;
        if turn * turn > 2500.0 {
            self.off += turn * 0.2;
        }
        self.amt += ((d * 4.0).min(1.0) - self.amt) * 0.4;
        self.phase += self.amt;
        // `EntityPlayer.onLivingUpdate`: the Java reads the motion left after the tick's friction (0.546 on the ground), here the step.
        if steps {
            self.walked += d * 0.6;
        }
        self.bob += (if ground { (d * 0.546).min(0.1) } else { 0.0 } - self.bob) * 0.4;
        self.tilt += (if ground { 0.0 } else { (-vy * 0.2).atan() * 15.0 } - self.tilt) * 0.8;
    }

    /// `ItemRenderer.updateEquippedItem`: the hand lowers when the hotbar slot or the item changes, and shows the new item once it
    /// is down (< 0.1). ponytail: `Minecraft.clickMouse` resets the progress when a place or use changes the stack size; here any
    /// drop of the count of the item in hand does (so does throwing one away).
    pub fn equip_tick(&mut self, slot: usize, held: Option<ItemStack>) {
        self.prev_eq = self.eq;
        // A tool's wear is the same stack (Java compares the object, and `damageItem` mutates it): only the id counts for a tool,
        // or every block broken and every mob hit lowers the hand and hides the swing. ponytail: two tools of one kind swapped
        // in a slot do not lower the hand.
        let key = |s: Option<ItemStack>| s.map(|s| (s.id, if craft::max_damage(s.id).is_some() { 0 } else { s.damage }));
        let same = slot == self.slot && key(held) == key(self.shown);
        if same {
            if held.zip(self.shown).is_some_and(|(h, s)| h.count < s.count) {
                self.eq = 0.0;
            }
            self.shown = held;
        }
        self.eq += ((if same { 1.0 } else { 0.0 }) - self.eq).clamp(-0.4, 0.4);
        if self.eq < 0.1 {
            (self.shown, self.slot) = (held, slot);
        }
    }

    /// `equippedProgress` and the item the hand shows, `partial` of the way to the next tick.
    pub fn equipped(&self, partial: f32) -> (f32, Option<ItemStack>) {
        (self.prev_eq + (self.eq - self.prev_eq) * partial, self.shown)
    }

    /// `getSwingProgress`: the arm swing 0..1 (the model's `onGround`).
    pub fn swing_at(&self, partial: f32) -> f32 {
        let mut s = self.swing - self.prev_swing;
        if s < 0.0 {
            s += 1.0;
        }
        self.prev_swing + s * partial
    }

    /// `EntityRenderer.setupViewBobbing`: the eye sways with the step. The Java extrapolates `distanceWalkedModified` from its current
    /// value (`dwm + (dwm - prev) * partial`) instead of interpolating; kept.
    pub fn view_bob(&self, partial: f32) -> Mat4 {
        let w = -(self.walked + (self.walked - self.prev_walked) * partial) * PI;
        let (a, tilt) = (self.prev_bob + (self.bob - self.prev_bob) * partial, self.prev_tilt + (self.tilt - self.prev_tilt) * partial);
        Mat4::from_translation(Vec3::new(w.sin() * a * 0.5, -(w.cos() * a).abs(), 0.0))
            * Mat4::from_rotation_z((w.sin() * a * 3.0).to_radians())
            * Mat4::from_rotation_x((((w - 0.2).cos() * a).abs() * 5.0).to_radians())
            * Mat4::from_rotation_x(tilt.to_radians())
    }

    /// The pose `partial` of the way to the next tick (`RenderLiving.doRenderLiving`), and the body's yaw for `root`.
    pub fn pose(&self, partial: f32, pitch: f32, sneak: bool, riding: bool, holding: bool) -> (Pose, f32) {
        let body = lerp_angle(self.prev_off, self.off, partial);
        let pose = Pose {
            head_yaw: lerp_angle(self.prev_rot, self.rot, partial) - body,
            pitch,
            limb: self.phase - self.amt * (1.0 - partial),
            limb_amt: (self.prev_amt + (self.amt - self.prev_amt) * partial).min(1.0),
            age: self.age as f32 + partial,
            swing: self.swing_at(partial),
            sneak,
            riding,
            holding,
        };
        (pose, body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_skin_is_in_the_atlas_and_the_model_is_42_quads() {
        let q = quads(&Mat4::IDENTITY, &Pose::default());
        assert_eq!(q.len(), 42);
        assert!(q.iter().all(|q| q.uv.iter().all(|t| (0.0..=64.0).contains(&t[0]) && (0.0..=32.0).contains(&t[1]))));
        // The face of the head (face 4 of part 0) is the 8 x 8 square at (8, 8) of char.png.
        assert!((q[4].uv[1][0] - 8.1).abs() < 1e-4 && (q[4].uv[1][1] - 8.1).abs() < 1e-4);
        // char.png is in the atlas rows under the sign: the head's skin pixel is opaque.
        let (u, v) = atlas::char_px(12.0, 12.0);
        let px = ((v * atlas::ATLAS_H as f32) as usize * atlas::ATLAS_W + (u * atlas::ATLAS_W as f32) as usize) * 4;
        assert_eq!(atlas::atlas_rgba()[px + 3], 255, "skin pixel at the head's face");
    }

    #[test]
    fn walking_swings_legs_opposite_and_sneaking_leans() {
        let walk = Pose { limb: 1.0, limb_amt: 1.0, ..Pose::default() };
        let l = limbs(&walk);
        assert!((l[5].rot[0] + l[6].rot[0]).abs() < 1e-5 && l[5].rot[0].abs() > 0.1, "legs swing against each other");
        let l = limbs(&Pose { sneak: true, ..Pose::default() });
        assert_eq!((l[BODY].rot[0], l[5].rp[1], l[HEAD].rp[1], l[1].rp[1]), (0.5, 9.0, 1.0, 0.0));
        // A swing at 0.5 lifts the right arm forward (x rotation well below the idle one).
        assert!(limbs(&Pose { swing: 0.5, ..Pose::default() })[R_ARM].rot[0] < -1.0);
    }

    #[test]
    fn the_arm_swing_runs_8_ticks_and_the_body_follows_the_walk() {
        let mut a = Anim::default();
        a.swing();
        let mut seen = vec![];
        for _ in 0..9 {
            a.tick(0.0, 0.0, 0.0, true, 0.0, true);
            seen.push(a.swing);
        }
        assert_eq!(seen[0], 1.0 / 8.0, "the arm has moved on the first tick");
        assert!(seen[..7].windows(2).all(|w| w[1] > w[0]) && seen[7] == 0.0 && seen[8] == 0.0, "{seen:?}");
        // Walking along +x (Java yaw -90): the body turns toward the walk but never more than 75 degrees from the head.
        let mut a = Anim::default();
        for i in 0..40 {
            a.tick(i as f32 * 0.2, 0.0, 0.0, true, 0.0, true);
        }
        assert!(a.amt > 0.5 && (a.rot - a.off).abs() <= 75.0 + 0.01 && a.off < -20.0, "{} {}", a.amt, a.off);
    }

    #[test]
    fn walking_bobs_the_view_and_the_hand_sits_low_right_in_front() {
        let mut a = Anim::default();
        a.tick(0.0, 0.0, 0.0, true, 0.0, true);
        assert!(a.view_bob(0.5).abs_diff_eq(Mat4::IDENTITY, 1e-6), "standing still does not sway");
        for i in 1..30 {
            a.tick(i as f32 * 0.2, 0.0, 0.0, true, 0.0, true);
        }
        let m = a.view_bob(0.5);
        assert!(a.bob > 0.09 && !m.abs_diff_eq(Mat4::IDENTITY, 1e-4) && m.w_axis.y <= 0.0, "walking sways and never lifts the eye");
        a.tick(6.0, 0.0, 0.0, false, -0.5, true);
        assert!(a.tilt > 0.0, "falling tilts the view");
        // The bare arm and a held sword, in eye space: ahead (-z) and below the eye, to the right.
        for held in [None, Some(ItemStack { id: 267, count: 1, damage: 0 })] {
            let (mut v, mut i) = (vec![], vec![]);
            push_hand(&mut v, &mut i, &Hand { to_world: Mat4::IDENTITY, equip: 1.0, swing: 0.0, held, at: Vec3::ZERO });
            assert!(!i.is_empty());
            for p in v.chunks_exact(VF) {
                assert!(p[2] < -0.1 && p[2] > -1.6 && p[0] > -0.2 && p[0] < 1.0 && p[1] < 0.2, "{held:?}: {p:?}", held = held.is_some());
            }
        }
    }

    #[test]
    fn tool_wear_does_not_lower_the_hand() {
        let mut a = Anim::default();
        for _ in 0..10 {
            a.equip_tick(0, Some(ItemStack { id: 267, count: 1, damage: 0 }));
        }
        a.equip_tick(0, Some(ItemStack { id: 267, count: 1, damage: 1 }));
        assert_eq!(a.equipped(1.0).0, 1.0, "one use of a sword is not a new item");
    }

    #[test]
    fn the_hand_lowers_before_it_shows_another_item() {
        let (sword, stone) = (Some(ItemStack { id: 267, count: 1, damage: 0 }), Some(ItemStack { id: 1, count: 1, damage: 0 }));
        let mut a = Anim::default();
        for _ in 0..10 {
            a.equip_tick(0, sword);
        }
        assert_eq!((a.equipped(1.0).0, a.shown.map(|s| s.id)), (1.0, Some(267)));
        a.equip_tick(1, stone);
        assert!(a.eq < 1.0 && a.shown.map(|s| s.id) == Some(267), "still the sword while it goes down");
        for _ in 0..4 {
            a.equip_tick(1, stone);
        }
        assert_eq!(a.shown.map(|s| s.id), Some(1));
    }
}
