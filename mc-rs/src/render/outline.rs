//! Outline of the block the pick ray points at: 12 thin dark boxes along the edges of the block,
//! grown by 0.002 (`RenderGlobal.drawSelectionBox` grows the box by the same amount against
//! z-fighting). Drawn with the chunk pipeline from one small dynamic buffer pair.

use crate::render::mesh::push_box;

const GROW: f32 = 0.002;
/// Edge thickness in blocks. // UNVERIFIED: vanilla draws 2 px GL lines, which have no block size.
const THICK: f32 = 0.02;
/// 12 edges x 6 faces x 4 vertices x 6 floats, and x 6 indices per face.
const VERTEX_BYTES: u64 = 12 * 6 * 4 * 6 * 4;
const INDEX_BYTES: u64 = 12 * 6 * 6 * 4;

pub struct Outline {
    pub vbuf: wgpu::Buffer,
    pub ibuf: wgpu::Buffer,
}

impl Outline {
    pub fn new(device: &wgpu::Device) -> Self {
        let buf = |label, size, usage| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        Self { vbuf: buf("outline_vbuf", VERTEX_BYTES, wgpu::BufferUsages::VERTEX), ibuf: buf("outline_ibuf", INDEX_BYTES, wgpu::BufferUsages::INDEX) }
    }

    /// Upload the outline of the box `bounds` (min xyz, max xyz) inside block `pos`; returns the index count to draw.
    pub fn update(&self, queue: &wgpu::Queue, pos: (i32, i32, i32), bounds: [f32; 6]) -> u32 {
        let (verts, idxs) = geometry(pos, bounds);
        queue.write_buffer(&self.vbuf, 0, bytemuck::cast_slice(&verts));
        queue.write_buffer(&self.ibuf, 0, bytemuck::cast_slice(&idxs));
        idxs.len() as u32
    }
}

fn geometry(pos: (i32, i32, i32), bounds: [f32; 6]) -> (Vec<f32>, Vec<u32>) {
    let p = [pos.0 as f32, pos.1 as f32, pos.2 as f32];
    let (a, b): ([f32; 3], [f32; 3]) = (std::array::from_fn(|i| p[i] + bounds[i] - GROW), std::array::from_fn(|i| p[i] + bounds[i + 3] + GROW));
    let (mut verts, mut idxs) = (Vec::new(), Vec::new());
    for axis in 0..3 {
        // The 4 edges along `axis`: each of the other two axes pinned to its low or high side.
        for k in 0..4 {
            let (mut lo, mut hi) = (a, b);
            for (j, other) in [(axis + 1) % 3, (axis + 2) % 3].into_iter().enumerate() {
                if k >> j & 1 == 0 { hi[other] = a[other] + THICK } else { lo[other] = b[other] - THICK }
            }
            push_box(&mut verts, &mut idxs, lo, hi, 7, 0.15); // bedrock grey x 0.15 = near black
        }
    }
    (verts, idxs)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The CPU geometry fills exactly the buffers `new` allocates, and stays around the block.
    #[test]
    fn geometry_fits_the_buffers_and_hugs_the_block() {
        let (v, i) = geometry((-3, 70, 8), [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
        assert_eq!((v.len() * 4) as u64, VERTEX_BYTES);
        assert_eq!((i.len() * 4) as u64, INDEX_BYTES);
        for p in v.chunks(6) {
            assert!(p[0] >= -3.0 - GROW - 1e-4 && p[0] <= -2.0 + GROW + 1e-4);
            assert!(p[1] >= 70.0 - GROW - 1e-4 && p[1] <= 71.0 + GROW + 1e-4);
            assert!(p[2] >= 8.0 - GROW - 1e-4 && p[2] <= 9.0 + GROW + 1e-4);
        }
    }

    /// A slab's outline stops at half a block.
    #[test]
    fn outline_follows_the_bounds() {
        let (v, _) = geometry((0, 10, 0), [0.0, 0.0, 0.0, 1.0, 0.5, 1.0]);
        assert!(v.chunks(6).all(|q| q[1] <= 10.5 + GROW + 1e-4));
        assert!(v.chunks(6).any(|q| q[1] > 10.5));
    }
}
