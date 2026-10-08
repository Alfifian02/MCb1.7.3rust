//! Chunk mesher.
//!
//! One mesh per 16x128x16 chunk. Faces are emitted only where the neighbouring cell is air;
//! at the chunk edge the neighbour is read from the adjacent chunk, so no faces are wasted on
//! the seams between loaded chunks. Vertex positions are world coordinates.
//!
//! Vertex format: position(3) + uv(2) + light(1) = 6 floats = 24 bytes (`gpu::pipeline::Vertex`).

use crate::render::atlas;
use crate::world::chunk::{idx, is_plant, H, VOLUME};

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

/// Build vertex + index buffers for one chunk.
/// `nb` are the blocks of the neighbouring chunks, in the order +X, -X, +Z, -Z; they are only
/// read for the one-cell border. `(ox, oz)` is the chunk's world origin (chunk * 16).
/// Vertices: 6 floats each (px, py, pz, u, v, light). Indices: 6 per face (two triangles).
pub fn build(blocks: &[u8], nb: [&[u8]; 4], ox: i32, oz: i32) -> (Vec<f32>, Vec<u32>) {
    assert_eq!(blocks.len(), VOLUME);
    // Cell lookup for x, z in -1..=16 (one cell outside the chunk); above/below the world is air.
    let get = |x: i32, y: i32, z: i32| -> u8 {
        if y < 0 || y >= H as i32 { return 0; }
        let b = if x < 0 { nb[1] } else if x >= 16 { nb[0] } else if z < 0 { nb[3] } else if z >= 16 { nb[2] } else { blocks };
        let id = b[idx((x & 15) as usize, y as usize, (z & 15) as usize)];
        if is_plant(id) { 0 } else { id } // see `is_plant`: neither drawn nor occluding
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
                    let light = match face_i {
                        2 => 1.00,  // +Y top
                        3 => 0.55,  // -Y bottom
                        _ => 0.80,  // +X, -X, +Z, -Z sides
                    };
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
        let (verts, _) = build(&c, [&a[..], &a[..], &a[..], &a[..]], 0, 0);
        assert_eq!(face_count(&verts), 6);
    }

    /// A block on the chunk edge hides the face it shares with the neighbour chunk's block,
    /// for every one of the four sides.
    #[test]
    fn edge_face_is_culled_against_neighbour() {
        let a = air();
        let mk = |x: usize, z: usize| { let mut c = air(); c[idx(x, 20, z)] = 1; c };
        let faces = |me: &[u8], nb: [&[u8]; 4]| face_count(&build(me, nb, 0, 0).0);
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
