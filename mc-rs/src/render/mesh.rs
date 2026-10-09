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
use crate::world::chunk::{brightness, cross_shape, idx, is_plant, Nibbles, H, VOLUME};

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
/// `data` is this chunk's block metadata: it picks the atlas tile of a cell (wool colour, wood species); the
/// neighbours' metadata is never needed, a neighbour only has to be see-through or not.
/// `nb` / `nb_light` are the blocks and light of the neighbouring chunks, in the order +X, -X, +Z, -Z;
/// they are only read for the one-cell border. `light` holds sky light in the low nibble and block
/// light in the high nibble. `sky_sub` is `World.skylightSubtracted` (0 day .. 11 night, `world::sky`).
/// `(ox, oz)` is the chunk's world origin (chunk * 16).
/// Vertices: 6 floats each (px, py, pz, u, v, light). Indices: 6 per face (two triangles).
/// `sun` = `sky::sun_key`: sun tan(angle) x 2, or `NO_SUN`. ponytail: a face is shadowed when a block lies on the ray
/// toward the sun (8 steps, the sun moves along Z like `render::sky` draws it, rays cross into the +Z/-Z chunks; a shadow
/// cast across an X border is approximated); upgrade = a per-column shadow map shared by chunks.
pub const NO_SUN: i32 = i32::MAX;

pub fn build(blocks: &[u8], data: &Nibbles, light: &[u8], nb: [&[u8]; 4], nb_light: [&[u8]; 4], sky_sub: u8, sun: i32, ox: i32, oz: i32) -> (Vec<f32>, Vec<u32>) {
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
        if y >= H as i32 { return table[15usize.saturating_sub(sky_sub as usize)]; }
        let l = match slot(x, z) { 0 => light, s => nb_light[s - 1] };
        let v = l[idx((x & 15) as usize, y as usize, (z & 15) as usize)];
        table[((v & 15).saturating_sub(sky_sub)).max(v >> 4) as usize]
    };
    // Sky light nibble of a cell (open sky above the world, none below it).
    let skyl = |x: i32, y: i32, z: i32| -> u8 {
        if y < 0 { return 0; }
        if y >= H as i32 { return 15; }
        let l = match slot(x, z) { 0 => light, s => nb_light[s - 1] };
        l[idx((x & 15) as usize, y as usize, (z & 15) as usize)] & 15
    };
    // Solid cell for the shadow ray: the sun moves along Z, so only the +Z / -Z neighbours (whole chunks) are read, x is
    // clamped into the chunk (ponytail: a shadow cast across an X border is approximated by the cell at the edge).
    let solid = |x: i32, y: i32, z: i32| -> bool {
        if y < 0 || y >= H as i32 { return false; }
        let b = if z < 0 { nb[3] } else if z >= 16 { nb[2] } else { blocks };
        let id = b[idx(x.clamp(0, 15) as usize, y as usize, (z & 15) as usize)];
        id != 0 && !is_plant(id)
    };
    // Shade of an air cell, 0..1: 1 when a ray toward the sun hits a block, scaled by the cell's sky light so it fades
    // out where vanilla sky light already darkens (no step at the edge of a tree's own shade).
    let occ = |x: i32, y: i32, z: i32| -> f32 {
        if sun == NO_SUN || !(-1..=16).contains(&x) || !(-1..=16).contains(&z) { return 0.0; }
        let (cz, cy) = (z as f32 + 0.5, y as f32 + 0.5);
        let hit = (1..=8).map(|n| ((cz + sx * n as f32).floor() as i32, (cy + sy * n as f32).floor() as i32))
            .take_while(|&(rz, _)| (-16..32).contains(&rz)).any(|(rz, ry)| solid(x, ry, rz));
        if hit { skyl(x, y, z) as f32 / 15.0 } else { 0.0 }
    };
    let mut verts: Vec<f32> = Vec::new();
    let mut idxs: Vec<u32> = Vec::new();

    for y in 0..H as i32 {
        for z in 0..16 {
            for x in 0..16 {
                // Plants: two crossed quads, visible from both sides, lit by their own cell.
                let raw = blocks[idx(x as usize, y as usize, z as usize)];
                if let Some((half, h)) = cross_shape(raw) {
                    let lit = bright(x, y, z);
                    let (cx, cz) = ((ox + x) as f32 + 0.5, (oz + z) as f32 + 0.5);
                    let (au, av) = atlas::atlas_uv(raw as u16, 0.0, 0.0);
                    for (a, b) in [((-half, -half), (half, half)), ((-half, half), (half, -half))] {
                        let base = verts.len() as u32 / 6;
                        for (px, pz, py) in [(a.0, a.1, 0.0), (b.0, b.1, 0.0), (b.0, b.1, h), (a.0, a.1, h)] {
                            verts.extend_from_slice(&[cx + px, y as f32 + py, cz + pz, au, av, lit]);
                        }
                        // Both windings: the pipeline culls back faces.
                        idxs.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3, base, base + 2, base + 1, base, base + 3, base + 2]);
                    }
                    continue;
                }
                let blk = get(x, y, z);
                if blk == 0 {
                    continue;
                }
                let tile = atlas::tile_of(blk, data.get(x as usize, y as usize, z as usize));
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
                    let own = occ(x + dx, y + dy, z + dz);
                    let mut ls = [light; 4];
                    for (ci, (corner, uv)) in face.corners.iter().zip(face.uv.iter()).enumerate() {
                        let (au, av) = atlas::atlas_uv(tile, uv[0], uv[1]);
                        // ponytail: world-space f32 vertices lose precision far from the origin
                        // (about 1 cm at 100k blocks); upgrade path is a per-chunk offset uniform.
                        verts.push(corner[0] + (ox + x) as f32);
                        verts.push(corner[1] + y as f32);
                        verts.push(corner[2] + (oz + z) as f32);
                        verts.push(au);
                        verts.push(av);
                        // Soft edge: the shadow of the 4 air cells touching this corner in the face plane, averaged, so the
                        // GPU fades it across a block instead of cutting it at the block edge.
                        let t = if dx != 0 { [1, 2] } else if dy != 0 { [0, 2] } else { [0, 1] };
                        let hit: f32 = if sun == NO_SUN { 0.0 } else { (0..4usize).map(|i| {
                            let mut c = [x + dx, y + dy, z + dz];
                            c[t[0]] += corner[t[0]] as i32 - 1 + (i & 1) as i32;
                            c[t[1]] += corner[t[1]] as i32 - 1 + (i >> 1) as i32;
                            if get(c[0], c[1], c[2]) != 0 { own } else { occ(c[0], c[1], c[2]) }
                        }).sum() };
                        ls[ci] = light * (1.0 - 0.4 * hit / 4.0);
                        verts.push(ls[ci]);
                    }
                    // Split along the diagonal whose ends match best, or the gradient shows as dark triangles.
                    idxs.extend_from_slice(&if (ls[0] - ls[2]).abs() <= (ls[1] - ls[3]).abs() {
                        [base, base + 1, base + 2, base, base + 2, base + 3]
                    } else {
                        [base + 1, base + 2, base + 3, base + 1, base + 3, base]
                    });
                }
            }
        }
    }
    (verts, idxs)
}

/// Append an axis-aligned box `min..max` (world coordinates) of atlas tile `tile` with a flat `light`; the
/// outline of the picked block reuses the face templates above, so the winding is the mesher's.
pub fn push_box(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, min: [f32; 3], max: [f32; 3], tile: u16, light: f32) {
    for face in &FACES {
        let base = verts.len() as u32 / 6;
        for (c, uv) in face.corners.iter().zip(face.uv.iter()) {
            let (au, av) = atlas::atlas_uv(tile, uv[0], uv[1]);
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
        let (verts, _) = build(&c, &Nibbles::new(), &a, [&a[..], &a[..], &a[..], &a[..]], [&a[..], &a[..], &a[..], &a[..]], 0, NO_SUN, 0, 0);
        assert_eq!(face_count(&verts), 6);
    }

    /// A pillar at z=9 shades the ground at z=7 (sun at +Z, 45 degrees) but not the ground at z=2.
    #[test]
    fn pillar_casts_a_shadow_away_from_the_sun() {
        let mut c = air();
        for z in 0..16 { c[idx(5, 0, z)] = 1; }
        for y in 1..4 { c[idx(5, y, 9)] = 1; }
        let (a, e) = (vec![15u8; VOLUME], air()); // full sky light, empty neighbours
        let v = build(&c, &Nibbles::new(), &a, [&e[..], &e[..], &e[..], &e[..]], [&a[..], &a[..], &a[..], &a[..]], 0, 2, 0, 0).0;
        let top = |z0: f32| v.chunks(6).find(|q| q[1] == 1.0 && q[2] >= z0 && q[2] < z0 + 1.0 && q[0] == 5.0).unwrap()[5];
        assert!(top(7.0) < top(2.0));
    }

    /// The cell's metadata picks the atlas tile of its faces: red wool is not white wool.
    #[test]
    fn metadata_picks_the_face_tile() {
        let a = air();
        let mut c = air();
        c[idx(5, 20, 5)] = 35;
        let uv = |d: &Nibbles| {
            let v = build(&c, d, &a, [&a[..], &a[..], &a[..], &a[..]], [&a[..], &a[..], &a[..], &a[..]], 0, NO_SUN, 0, 0).0;
            (v[3], v[4])
        };
        let mut d = Nibbles::new();
        assert_eq!(uv(&d), atlas::atlas_uv(35, 0.0, 0.0));
        d.set(5, 20, 5, 14);
        assert_eq!(uv(&d), atlas::atlas_uv(atlas::tile_of(35, 14), 0.0, 0.0));
        assert_ne!(uv(&d), atlas::atlas_uv(35, 0.0, 0.0));
    }

    /// A plant is two crossed quads, and does not hide the top of the block under it.
    #[test]
    fn plant_is_two_crossed_quads() {
        let a = air();
        let mut c = air();
        c[idx(5, 20, 5)] = 37;
        assert_eq!(face_count(&build(&c, &Nibbles::new(), &a, [&a[..], &a[..], &a[..], &a[..]], [&a[..], &a[..], &a[..], &a[..]], 0, NO_SUN, 0, 0).0), 2);
        c[idx(5, 19, 5)] = 3; // dirt under the flower: 6 faces + 2 quads
        assert_eq!(face_count(&build(&c, &Nibbles::new(), &a, [&a[..], &a[..], &a[..], &a[..]], [&a[..], &a[..], &a[..], &a[..]], 0, NO_SUN, 0, 0).0), 8);
    }

    /// A block on the chunk edge hides the face it shares with the neighbour chunk's block,
    /// for every one of the four sides.
    #[test]
    fn edge_face_is_culled_against_neighbour() {
        let a = air();
        let mk = |x: usize, z: usize| { let mut c = air(); c[idx(x, 20, z)] = 1; c };
        let faces = |me: &[u8], nb: [&[u8]; 4]| face_count(&build(me, &Nibbles::new(), &a, nb, [&a[..], &a[..], &a[..], &a[..]], 0, NO_SUN, 0, 0).0);
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
