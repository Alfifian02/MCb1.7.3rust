//! M1 chunk mesher.
//!
//! Goal: emit only the *exterior* faces of a 16x16x128 chunk filled with stone.
//! No neighbor culling across chunks yet (this chunk is air on all 6 sides).
//!
//! Vertex format: position(3) + uv(2) + normal_or_ao(1) = 6 floats = 24 bytes.

use crate::world::chunk::{Chunk, H};
use crate::render::atlas;

#[inline]
fn idx_ext(x: usize, y: usize, z: usize, _w: usize, _d: usize) -> usize {
    (x << 11) | (z << 7) | y
}


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

/// Decide which of the 6 faces a single cell exposes by checking each of the 6
/// neighbors. Out-of-bounds counts as air (so a stone chunk emits its side faces).
/// Air cells emit no faces. Interior cells emit no faces -> mesh is much smaller.
fn exposed_faces(chunk: &Chunk, x: usize, y: usize, z: usize, w: usize, d: usize) -> u8 {
    use crate::world::chunk::H;
    let here = chunk.blocks[idx_ext(x, y, z, w, d)];
    if here == 0 { return 0; }
    let mut mask = 0u8;
    // Face order in FACES: 0=+X 1=-X 2=+Y 3=-Y 4=+Z 5=-Z
    let nx_p = if x + 1 < w { chunk.blocks[idx_ext(x+1, y, z, w, d)] } else { 0 };
    let nx_m = if x     > 0 { chunk.blocks[idx_ext(x-1, y, z, w, d)] } else { 0 };
    let ny_p = if y + 1 < H { chunk.blocks[idx_ext(x, y+1, z, w, d)] } else { 0 };
    let ny_m = if y     > 0 { chunk.blocks[idx_ext(x, y-1, z, w, d)] } else { 0 };
    let nz_p = if z + 1 < d { chunk.blocks[idx_ext(x, y, z+1, w, d)] } else { 0 };
    let nz_m = if z     > 0 { chunk.blocks[idx_ext(x, y, z-1, w, d)] } else { 0 };
    if nx_p == 0 { mask |= 1 << 0; }
    if nx_m == 0 { mask |= 1 << 1; }
    if ny_p == 0 { mask |= 1 << 2; }
    if ny_m == 0 { mask |= 1 << 3; }
    if nz_p == 0 { mask |= 1 << 4; }
    if nz_m == 0 { mask |= 1 << 5; }
    mask
}

/// Build vertex + index buffers for the chunk.
/// Vertices: 6 floats each (px, py, pz, u, v, light).
/// Indices: 6 per face (two triangles).
///
/// M3d: this is now generic over the X/Z extent. For the legacy 16x16 chunk
/// pass `(W, D)` from `world::chunk`. For the 48x48 super-chunk pass `(48, 48)`.
pub fn build_ext(chunk: &Chunk, w: usize, d: usize) -> (Vec<f32>, Vec<u32>) {
    assert_eq!(chunk.blocks.len(), w * H * d);
    let mut verts: Vec<f32> = Vec::with_capacity(64 * 1024);
    let mut idxs: Vec<u32> = Vec::with_capacity(96 * 1024);

    for y in 0..H {
        for z in 0..d {
            for x in 0..w {
                if chunk.blocks[idx_ext(x, y, z, w, d)] == 0 {
                    continue; // M2+: only render exposed surfaces; M1 emits all 6
                }
                let mask = exposed_faces(chunk, x, y, z, w, d);
                if mask == 0 {
                    continue;
                }
                for (face_i, face) in FACES.iter().enumerate() {
                    if mask & (1 << face_i) == 0 {
                        continue;
                    }
                    let base = verts.len() as u32 / 6;
                    for corner in face.corners {
                        verts.push(corner[0] + x as f32);
                        verts.push(corner[1] + y as f32);
                        verts.push(corner[2] + z as f32);
                    }
                    let light = match face_i {
                        2 => 1.00,  // +Y top
                        3 => 0.55,  // -Y bottom
                        _ => 0.80,  // +X, -X, +Z, -Z sides
                    };
                    let blk = chunk.blocks[idx_ext(x, y, z, w, d)];
                    for uv in face.uv {
                        let (au, av) = atlas::atlas_uv(blk, uv[0], uv[1]);
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

/// Backwards-compat wrapper for the original 16x16 chunk shape.
pub fn build(chunk: &Chunk) -> (Vec<f32>, Vec<u32>) {
    use crate::world::chunk::{W, D};
    build_ext(chunk, W, D)
}
