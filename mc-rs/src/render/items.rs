//! Dropped items: a 0.25 cube per `ItemEntity` in the flat colour of its item, hovering and bobbing like
//! `RenderItem` (`sin(age / 10) * 0.1 + 0.1`), drawn with the chunk pipeline from one dynamic buffer pair.
//! ponytail: no spin (needs rotated boxes), flat full brightness, no 2nd/3rd copy for big stacks; M14 draws
//! real item sprites.

use crate::render::mesh::push_box;
use glam::{Mat4, Vec3};

use crate::world::items::{stack_tile, Drops, ItemStack, MAX_ITEMS};
use crate::world::mobs::{Mobs, Pig, MAX_PIGS};

/// 24 vertices x 6 floats per cube, 36 indices per cube.
/// A pig is 6 boxes.
const BOXES: usize = MAX_ITEMS + MAX_PIGS * 6;
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
    pub fn update(&self, queue: &wgpu::Queue, drops: &Drops, mobs: &Mobs) -> u32 {
        let (verts, idxs) = geometry(drops, mobs);
        if idxs.is_empty() {
            return 0;
        }
        queue.write_buffer(&self.vbuf, 0, bytemuck::cast_slice(&verts));
        queue.write_buffer(&self.ibuf, 0, bytemuck::cast_slice(&idxs));
        idxs.len() as u32
    }
}

fn geometry(drops: &Drops, mobs: &Mobs) -> (Vec<f32>, Vec<u32>) {
    let (mut verts, mut idxs) = (Vec::new(), Vec::new());
    let a = drops.alpha();
    for e in &drops.items {
        let mut c = e.prev.lerp(e.pos, a);
        c.y += ((e.age as f32 + a) / 10.0).sin() * 0.1 + 0.1;
        let h = 0.125;
        push_box(&mut verts, &mut idxs, [c.x - h, c.y - h, c.z - h], [c.x + h, c.y + h, c.z + h], stack_tile(e.stack), 1.0);
    }
    for p in &mobs.pigs {
        push_pig(&mut verts, &mut idxs, p);
    }
    (verts, idxs)
}

/// `ModelPig` = `ModelQuadruped(6)` drawn like `RenderLiving`: turned to face its yaw, scaled 1/16 and flipped upside down
/// (model y points down), 24 px above the ground. Parts are (box min, size, rotation point, rotation x) in model pixels;
/// the body lies down (x turned 90 degrees) and legs swing with `limb` (`cos(limb x 0.6662) x 1.4 x amount`).
/// UNVERIFIED stand-in: flat pink (wool 6), flashing red (wool 14) while hurt; the real `pig.png` comes with M14.
fn push_pig(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, p: &Pig) {
    let feet = p.body.pos - Vec3::new(0.0, 0.45, 0.0);
    let root = Mat4::from_translation(feet)
        * Mat4::from_rotation_y(std::f32::consts::PI - p.yaw.to_radians())
        * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0) / 16.0)
        * Mat4::from_translation(Vec3::new(0.0, -23.875, 0.0));
    let tile = stack_tile(ItemStack { id: 35, count: 1, damage: if p.hurt > 0 { 14 } else { 6 } });
    let swing = |phase: f32| (p.limb * 0.6662 + phase).cos() * 1.4 * p.limb_amt;
    let parts: [([f32; 3], [f32; 3], [f32; 3], f32); 6] = [
        ([-4.0, -4.0, -8.0], [8.0, 8.0, 8.0], [0.0, 12.0, -6.0], 0.0),                              // head
        ([-5.0, -10.0, -7.0], [10.0, 16.0, 8.0], [0.0, 11.0, 2.0], std::f32::consts::FRAC_PI_2),     // body
        ([-2.0, 0.0, -2.0], [4.0, 6.0, 4.0], [-3.0, 18.0, 7.0], swing(0.0)),                         // leg1
        ([-2.0, 0.0, -2.0], [4.0, 6.0, 4.0], [3.0, 18.0, 7.0], swing(std::f32::consts::PI)),         // leg2
        ([-2.0, 0.0, -2.0], [4.0, 6.0, 4.0], [-3.0, 18.0, -5.0], swing(std::f32::consts::PI)),       // leg3
        ([-2.0, 0.0, -2.0], [4.0, 6.0, 4.0], [3.0, 18.0, -5.0], swing(0.0)),                         // leg4
    ];
    for (min, size, rp, rx) in parts {
        let m = root * Mat4::from_translation(Vec3::from(rp)) * Mat4::from_rotation_x(rx);
        let n0 = verts.len();
        push_box(verts, idxs, min, [min[0] + size[0], min[1] + size[1], min[2] + size[2]], tile, 1.0);
        for v in verts[n0..].chunks_exact_mut(6) {
            let w = m.transform_point3(Vec3::new(v[0], v[1], v[2]));
            v[..3].copy_from_slice(&w.to_array());
        }
    }
}
