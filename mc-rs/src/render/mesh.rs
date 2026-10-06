//! M1 chunk mesher.
//!
//! Goal: emit only the *exterior* faces of a 16x16x128 chunk filled with stone.
//! No neighbor culling across chunks yet (this chunk is air on all 6 sides).
//!
//! Vertex format: position(3) + uv(2) + normal_or_ao(1) = 6 floats = 24 bytes.

use crate::world::chunk::{Chunk, W, H, D, VOLUME};

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

/// Decide which of the 6 faces a single cell exposes. For M1, every cell is stone
/// and we treat everything outside the chunk as air, so every cell emits all 6 faces.
/// This is wasteful but correct; neighbor culling comes in M13.
fn exposed_faces(_chunk: &Chunk, _x: usize, _y: usize, _z: usize) -> u8 {
    0b0011_1111
}

/// Build vertex + index buffers for the chunk.
/// Vertices: 6 floats each (px, py, pz, u, v, light).
/// Indices: 6 per face (two triangles).
pub fn build(chunk: &Chunk) -> (Vec<f32>, Vec<u16>) {
    assert_eq!(chunk.blocks.len(), VOLUME);
    let mut verts: Vec<f32> = Vec::with_capacity(64 * 1024);
    let mut idxs: Vec<u16> = Vec::with_capacity(96 * 1024);

    for y in 0..H {
        for z in 0..D {
            for x in 0..W {
                if chunk.blocks[(x << 11) | (z << 7) | y] == 0 {
                    continue; // M2+: only render exposed surfaces; M1 emits all 6
                }
                let mask = exposed_faces(chunk, x, y, z);
                if mask == 0 {
                    continue;
                }
                for (face_i, face) in FACES.iter().enumerate() {
                    if mask & (1 << face_i) == 0 {
                        continue;
                    }
                    let base = verts.len() as u16 / 6;
                    for corner in face.corners {
                        verts.push(corner[0] + x as f32);
                        verts.push(corner[1] + y as f32);
                        verts.push(corner[2] + z as f32);
                    }
                    for uv in face.uv {
                        verts.push(uv[0]);
                        verts.push(uv[1]);
                        verts.push(15.0 / 16.0); // full sky light for M1
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
