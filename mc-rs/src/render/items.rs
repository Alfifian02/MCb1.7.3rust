//! Dropped items: a 0.25 cube per `ItemEntity` in the flat colour of its item, hovering and bobbing like
//! `RenderItem` (`sin(age / 10) * 0.1 + 0.1`), drawn with the chunk pipeline from one dynamic buffer pair.
//! ponytail: no spin (needs rotated boxes), flat full brightness, no 2nd/3rd copy for big stacks; M14 draws
//! real item sprites.

use crate::render::mesh::push_box;
use glam::{Mat4, Vec3};

use crate::world::items::{stack_tile, Drops, ItemStack, MAX_ITEMS};
use crate::world::mobs::{Kind::*, Mob, Mobs, MAX_ARROWS, MAX_MOBS};
use crate::world::ticks::{Falling, MAX_FALLING};

/// 24 vertices x 6 floats per cube, 36 indices per cube.
/// A mob is at most 12 boxes (spider 11), an arrow one, a falling block one.
const BOXES: usize = MAX_ITEMS + MAX_MOBS * 12 + MAX_ARROWS + MAX_FALLING;
const VERTEX_BYTES: u64 = (BOXES * 24 * 6 * 4) as u64;
const INDEX_BYTES: u64 = (BOXES * 36 * 4) as u64;

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
    pub fn update(&self, queue: &wgpu::Queue, drops: &Drops, mobs: &Mobs, falling: &[Falling]) -> u32 {
        let (verts, idxs) = geometry(drops, mobs, falling);
        if idxs.is_empty() {
            return 0;
        }
        queue.write_buffer(&self.vbuf, 0, bytemuck::cast_slice(&verts));
        queue.write_buffer(&self.ibuf, 0, bytemuck::cast_slice(&idxs));
        idxs.len() as u32
    }
}

fn geometry(drops: &Drops, mobs: &Mobs, falling: &[Falling]) -> (Vec<f32>, Vec<u32>) {
    let (mut verts, mut idxs) = (Vec::new(), Vec::new());
    let a = drops.alpha();
    for e in &drops.items {
        let mut c = e.prev.lerp(e.pos, a);
        c.y += ((e.age as f32 + a) / 10.0).sin() * 0.1 + 0.1;
        let h = 0.125;
        push_box(&mut verts, &mut idxs, [c.x - h, c.y - h, c.z - h], [c.x + h, c.y + h, c.z + h], stack_tile(e.stack), 1.0);
    }
    for m in &mobs.list {
        push_mob(&mut verts, &mut idxs, m);
    }
    for a in &mobs.arrows {
        let (p, h) = (a.pos, 0.06);
        push_box(&mut verts, &mut idxs, [p.x - h, p.y - h, p.z - h], [p.x + h, p.y + h, p.z + h], stack_tile(ItemStack { id: 35, count: 1, damage: 12 }), 1.0);
    }
    // A falling block is a 0.98 box in its block's flat colour (`EntityFallingSand`).
    for f in falling {
        let c = f.prev.lerp(f.pos, a);
        let h = 0.49;
        push_box(&mut verts, &mut idxs, [c.x - h, c.y - h, c.z - h], [c.x + h, c.y + h, c.z + h], stack_tile(ItemStack { id: f.id as u16, count: 1, damage: 0 }), 1.0);
    }
    (verts, idxs)
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
        for v in verts[n0..].chunks_exact_mut(6) {
            let w = mat.transform_point3(Vec3::new(v[0], v[1], v[2]));
            v[..3].copy_from_slice(&w.to_array());
        }
    }
}
