//! Section mesher (16x16x16). Face-culled, 8-byte vertices, u16 indices.
use crate::block::*;
use crate::chunk::{Chunk, W};

pub const SECTION: usize = 16;

/// 8 bytes per vertex: pos = [x, y, z, face] (chunk-local), uv = [u, v, tile, shade].
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vertex {
    pub pos: [u8; 4],
    pub uv: [u8; 4],
}

#[derive(Default)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u16>,
}

// Corner offsets per face, counter-clockwise seen from outside.
const CORNERS: [[[u8; 3]; 4]; 6] = [
    [[1, 0, 1], [1, 0, 0], [1, 1, 0], [1, 1, 1]], // +X
    [[0, 0, 0], [0, 0, 1], [0, 1, 1], [0, 1, 0]], // -X
    [[0, 1, 1], [1, 1, 1], [1, 1, 0], [0, 1, 0]], // +Y
    [[0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1]], // -Y
    [[0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]], // +Z
    [[1, 0, 0], [0, 0, 0], [0, 1, 0], [1, 1, 0]], // -Z
];
const NORMALS: [[i32; 3]; 6] = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]];
const SHADE: [u8; 6] = [200, 200, 255, 128, 170, 170];
const UVS: [[u8; 2]; 4] = [[0, 1], [1, 1], [1, 0], [0, 0]];

/// `neighbor(x, y, z)` is queried for coordinates outside the chunk's x/z range
/// (use it to look into adjacent chunks; return AIR if not loaded yet).
pub fn build_section(chunk: &Chunk, sy: usize, neighbor: &dyn Fn(i32, i32, i32) -> u8) -> Mesh {
    let mut mesh = Mesh::default();
    let y0 = (sy * SECTION) as i32;
    for x in 0..W as i32 {
        for z in 0..W as i32 {
            for y in y0..y0 + SECTION as i32 {
                let id = chunk.get(x, y, z);
                if id == AIR {
                    continue;
                }
                for (fi, face) in FACES.iter().enumerate() {
                    let n = NORMALS[fi];
                    let (nx, ny, nz) = (x + n[0], y + n[1], z + n[2]);
                    let nb = if nx < 0 || nx >= W as i32 || nz < 0 || nz >= W as i32 {
                        neighbor(nx, ny, nz)
                    } else if ny < 0 {
                        BEDROCK // below the world: never visible
                    } else {
                        chunk.get(nx, ny, nz)
                    };
                    if is_opaque(nb) || nb == id {
                        continue;
                    }
                    let base = mesh.vertices.len() as u16;
                    let tile = tile(id, *face);
                    for (ci, c) in CORNERS[fi].iter().enumerate() {
                        mesh.vertices.push(Vertex {
                            pos: [x as u8 + c[0], y as u8 + c[1], z as u8 + c[2], fi as u8],
                            uv: [UVS[ci][0], UVS[ci][1], tile, SHADE[fi]],
                        });
                    }
                    mesh.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
                }
            }
        }
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    fn air(_: i32, _: i32, _: i32) -> u8 { AIR }

    #[test]
    fn single_block_has_six_faces() {
        let mut c = Chunk::new();
        c.set(5, 5, 5, STONE);
        let m = build_section(&c, 0, &air);
        assert_eq!(m.vertices.len(), 24);
        assert_eq!(m.indices.len(), 36);
    }

    #[test]
    fn adjacent_blocks_share_hidden_faces() {
        let mut c = Chunk::new();
        c.set(5, 5, 5, STONE);
        c.set(6, 5, 5, STONE);
        assert_eq!(build_section(&c, 0, &air).vertices.len(), 10 * 4);
    }

    #[test]
    fn buried_section_is_empty() {
        let mut c = Chunk::new();
        for x in 0..16 { for y in 0..16 { for z in 0..16 { c.set(x, y, z, STONE); } } }
        // neighbours solid, section above/below handled by chunk data (y 16 is air => top visible)
        let m = build_section(&c, 0, &|_, _, _| STONE);
        assert_eq!(m.vertices.len(), 16 * 16 * 4); // only the top layer
    }

    #[test]
    fn winding_matches_normals() {
        let mut c = Chunk::new();
        c.set(3, 3, 3, STONE);
        let m = build_section(&c, 0, &air);
        for q in m.vertices.chunks(4) {
            let f = q[0].pos[3] as usize;
            let p = |i: usize| [q[i].pos[0] as i32, q[i].pos[1] as i32, q[i].pos[2] as i32];
            let (a, b, d) = (p(0), p(1), p(2));
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [d[0] - a[0], d[1] - a[1], d[2] - a[2]];
            let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            assert_eq!(n, NORMALS[f], "face {f} winding");
        }
    }

    #[test]
    fn indices_fit_u16_worst_case() {
        let mut c = Chunk::new();
        for x in 0..16 { for y in 0..16 { for z in 0..16 {
            if (x + y + z) % 2 == 0 { c.set(x, y, z, STONE); }
        } } }
        let m = build_section(&c, 0, &air);
        assert!(m.vertices.len() <= u16::MAX as usize);
    }
}
