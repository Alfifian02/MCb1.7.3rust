//! Dropped items: a 0.25 cube per `ItemEntity` in the flat colour of its item, hovering and bobbing like
//! `RenderItem` (`sin(age / 10) * 0.1 + 0.1`), drawn with the chunk pipeline from one dynamic buffer pair.
//! ponytail: no spin (needs rotated boxes), flat full brightness, no 2nd/3rd copy for big stacks; M14 draws
//! real item sprites.

use crate::render::mesh::push_box;
use crate::world::items::{tile, Drops, MAX_ITEMS};

/// 24 vertices x 6 floats per cube, 36 indices per cube.
const VERTEX_BYTES: u64 = (MAX_ITEMS * 24 * 6 * 4) as u64;
const INDEX_BYTES: u64 = (MAX_ITEMS * 36 * 4) as u64;

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
    pub fn update(&self, queue: &wgpu::Queue, drops: &Drops) -> u32 {
        let (verts, idxs) = geometry(drops);
        if idxs.is_empty() {
            return 0;
        }
        queue.write_buffer(&self.vbuf, 0, bytemuck::cast_slice(&verts));
        queue.write_buffer(&self.ibuf, 0, bytemuck::cast_slice(&idxs));
        idxs.len() as u32
    }
}

fn geometry(drops: &Drops) -> (Vec<f32>, Vec<u32>) {
    let (mut verts, mut idxs) = (Vec::new(), Vec::new());
    let a = drops.alpha();
    for e in &drops.items {
        let mut c = e.prev.lerp(e.pos, a);
        c.y += ((e.age as f32 + a) / 10.0).sin() * 0.1 + 0.1;
        let h = 0.125;
        push_box(&mut verts, &mut idxs, [c.x - h, c.y - h, c.z - h], [c.x + h, c.y + h, c.z + h], tile(e.stack.id), 1.0);
    }
    (verts, idxs)
}
