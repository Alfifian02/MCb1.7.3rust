//! Chunk mesher.
//!
//! One mesh per 16x128x16 chunk. Faces are emitted only where the neighbouring cell is air;
//! at the chunk edge the neighbour is read from the adjacent chunk, so no faces are wasted on
//! the seams between loaded chunks. Vertex positions are world coordinates.
//!
//! Vertex format: position(3) + uv(2) + light(1) = 6 floats = 24 bytes (`gpu::pipeline::Vertex`).
//! `light` = face shade (RenderBlocks: top 1.0, bottom 0.5, Z sides 0.8, X sides 0.6) times the
//! brightness of the cell the face looks into (`Block.getBlockBrightness` -> `lightBrightnessTable`).

use crate::render::atlas;
use crate::world::chunk::{brightness, idx, is_plant, H, VOLUME};

/// One textured quad. Four corners in CCW order from the front.
/// `normal_index` selects the face normal (0..5) for debug-coloring later.
#[derive(Clone, Copy)]
struct Face {
    corners: [[f32; 3]; 4],
    uv: [[f32; 2]; 4],
    normal_index: u8,
}

/// 6 face templates. Each face is a unit quad on one of +/-X/Y/Z.
const FACES: [Face; 6] = [
    // +X (right)
    Face {
        corners: [[1.0, 0.0, 1.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 1.0, 1.0]],
        uv:       [[0.0, 0.0],     [1.0, 0.0],     [1.0, 1.0],     [0.0, 1.0]],
        normal_index: 0,
    },
    // -X (left)
    Face {
        corners: [[0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 1.0], [0.0, 1.0, 0.0]],
        uv:       [[0.0, 0.0],     [1.0, 0.0],     [1.0, 1.0],     [0.0, 1.0]],
        normal_index: 1,
    },
    // +Y (top)
    Face {
        corners: [[0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
        uv:       [[0.0, 0.0],     [1.0, 0.0],     [1.0, 1.0],     [0.0, 1.0]],
        normal_index: 2,
    },
    // -Y (bottom)
    Face {
        corners: [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]],
        uv:       [[0.0, 0.0],     [1.0, 0.0],     [1.0, 1.0],     [0.0, 1.0]],
        normal_index: 3,
    },
    // +Z (front)
    Face {
        corners: [[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]],
        uv:       [[0.0, 0.0],     [1.0, 0.0],     [1.0, 1.0],     [0.0, 1.0]],
        normal_index: 4,
    },
    // -Z (back)
    Face {
        corners: [[1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]],
        uv:       [[0.0, 0.0],     [1.0, 0.0],     [1.0, 1.0],     [0.0, 1.0]],
        normal_index: 5,
    },
];

/// Face order matches `FACES`: +X, -X, +Y, -Y, +Z, -Z.
const DIRS: [(i32, i32, i32); 6] = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)];

/// `World.skylightSubtracted`: 0 = full day. The day/night cycle is not ported yet.
const SKYLIGHT_SUBTRACTED: u8 = 0;

/// Build vertex + index buffers for one chunk.
/// `nb` / `nb_light` are the blocks and light of the neighbouring chunks, in the order +X, -X, +Z, -Z;
/// they are only read for the one-cell border. `light` holds sky light in the low nibble and block
/// light in the high nibble. `(ox, oz)` is the chunk's world origin (chunk * 16).
/// Vertices: 6 floats each (px, py, pz, u, v, light). Indices: 6 per face (two triangles).
pub fn build(blocks: &[u8], light: &[u8], nb: [&[u8]; 4], nb_light: [&[u8]; 4], ox: i32, oz: i32) -> (Vec<f32>, Vec<u32>) {
    assert_eq!(blocks.len(), VOLUME);
    assert_eq!(light.len(), VOLUME);
    // Which chunk's arrays a cell with x, z in -1..=16 lives in: 0 = this one, 1.. = +X, -X, +Z, -Z.
    let slot = |x: i32, z: i32| if x < 0 { 2 } else if x >= 16 { 1 } else if z < 0 { 4 } else if z >= 16 { 3 } else { 0 };
    // Cell lookup for x, z in -1..=16 (one cell outside the chunk); above/below the world is air.
    let get = |x: i32, y: i32, z: i32| -> u8 {
        if y < 0 || y >= H as i32 { return 0; }
        let b = match slot(x, z) { 0 => blocks, s => nb[s - 1] };
        let id = b[idx((x & 15) as usize, y as usize, (z & 15) as usize)];
        if is_plant(id) { 0 } else { id } // see `is_plant`: neither drawn nor occluding
    };
    // Brightness of a cell (`World.getBlockLightValue`): open sky above the world, dark below it.
    let table: [f32; 16] = std::array::from_fn(|i| brightness(i as u8));
    let bright = |x: i32, y: i32, z: i32| -> f32 {
        if y < 0 { return table[0]; }
        if y >= H as i32 { return table[15]; }
        let l = match slot(x, z) { 0 => light, s => nb_light[s - 1] };
        let v = l[idx((x & 15) as usize, y as usize, (z & 15) as usize)];
        table[((v & 15).saturating_sub(SKYLIGHT_SUBTRACTED)).max(v >> 4) as usize]
    };
    let mut verts: Vec<f32> = Vec::new();
    let mut idxs: Vec<u32> = Vec::new();

    for y in 0..H as i32 {
        for z in 0..16 {
            for x in 0..16 {
                let blk = get(x, y, z);
                if blk == 0 {
                    continue;
                }
                for (face_i, face) in FACES.iter().enumerate() {
                    let (dx, dy, dz) = DIRS[face_i];
                    if get(x + dx, y + dy, z + dz) != 0 {
                        continue;
                    }
                    let base = verts.len() as u32 / 6;
                    let shade = match face_i {
                        2 => 1.0, // +Y top
                        3 => 0.5, // -Y bottom
                        4 | 5 => 0.8, // +Z, -Z
                        _ => 0.6, // +X, -X
                    };
                    let light = shade * bright(x + dx, y + dy, z + dz);
                    for (corner, uv) in face.corners.iter().zip(face.uv.iter()) {
                        let (au, av) = atlas::atlas_uv(blk, uv[0], uv[1]);
                        // ponytail: world-space f32 vertices lose precision far from the origin
                        // (about 1 cm at 100k blocks); upgrade path is a per-chunk offset uniform.
                        verts.push(corner[0] + (ox + x) as f32);
                        verts.push(corner[1] + y as f32);
                        verts.push(corner[2] + (oz + z) as f32);
                        verts.push(au);
                        verts.push(av);
                        verts.push(light);
                    }
                    idxs.extend_from_slice(&[
                        base, base + 1, base + 2,
                        base, base + 2, base + 3,
                    ]);
                }
            }
        }
    }
    (verts, idxs)
}

/// Append an axis-aligned box `min..max` (world coordinates) of block `id` with a flat `light`; the
/// outline of the picked block reuses the face templates above, so the winding is the mesher's.
pub fn push_box(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, min: [f32; 3], max: [f32; 3], id: u8, light: f32) {
    for face in &FACES {
        let base = verts.len() as u32 / 6;
        for (c, uv) in face.corners.iter().zip(face.uv.iter()) {
            let (au, av) = atlas::atlas_uv(id, uv[0], uv[1]);
            verts.extend_from_slice(&[min[0] + c[0] * (max[0] - min[0]), min[1] + c[1] * (max[1] - min[1]), min[2] + c[2] * (max[2] - min[2]), au, av, light]);
        }
        idxs.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// Simple count of generated faces (verts.len()/24, since 4 verts * 6 floats).
pub fn face_count(verts: &[f32]) -> usize {
    verts.len() / 24
}

#[cfg(test)]
mod tests {
    use super::*;

    fn air() -> Vec<u8> { vec![0; VOLUME] }

    /// One block in the middle of a chunk is exactly one cube.
    #[test]
    fn single_block_is_one_cube() {
        let mut c = air();
        c[idx(5, 20, 5)] = 1;
        let a = air();
        let (verts, _) = build(&c, &a, [&a[..], &a[..], &a[..], &a[..]], [&a[..], &a[..], &a[..], &a[..]], 0, 0);
        assert_eq!(face_count(&verts), 6);
    }

    /// A block on the chunk edge hides the face it shares with the neighbour chunk's block,
    /// for every one of the four sides.
    #[test]
    fn edge_face_is_culled_against_neighbour() {
        let a = air();
        let mk = |x: usize, z: usize| { let mut c = air(); c[idx(x, 20, z)] = 1; c };
        let faces = |me: &[u8], nb: [&[u8]; 4]| face_count(&build(me, &a, nb, [&a[..], &a[..], &a[..], &a[..]], 0, 0).0);
        let (px, nx, pz, nz) = (mk(0, 5), mk(15, 5), mk(5, 0), mk(5, 15));
        let (a_px, a_nx, a_pz, a_nz) = (mk(15, 5), mk(0, 5), mk(5, 15), mk(5, 0));
        assert_eq!(faces(&a_px[..], [&px[..], &a[..], &a[..], &a[..]]), 5);
        assert_eq!(faces(&a_nx[..], [&a[..], &nx[..], &a[..], &a[..]]), 5);
        assert_eq!(faces(&a_pz[..], [&a[..], &a[..], &pz[..], &a[..]]), 5);
        assert_eq!(faces(&a_nz[..], [&a[..], &a[..], &a[..], &nz[..]]), 5);
        // Without the neighbour the same block shows all six.
        assert_eq!(faces(&a_px[..], [&a[..], &a[..], &a[..], &a[..]]), 6);
    }
}
