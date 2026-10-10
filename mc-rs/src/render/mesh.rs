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
use crate::world::chest;
use crate::world::chunk::{box_bounds, brightness, cross_shape, idx, is_fluid, is_plant, opaque, Nibbles, CACTUS_INSET, H, VOLUME};
use crate::world::ticks::non_solid;

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

/// (u, v) in 0..1 of the in-cell point `p` on `face`, the way the full-cell template maps its corners (u runs from corner 0 to
/// corner 1, v from corner 0 to corner 3). For a box that is its window of the tile (`renderEastFace` and friends crop the tile
/// by the bounds the same way); for a full cell it gives the template's own uv.
fn window(face: &Face, p: [f32; 3]) -> (f32, f32) {
    let along = |to: usize| {
        let a = (0..3).find(|&a| face.corners[to][a] != face.corners[0][a]).unwrap();
        if face.corners[to][a] > face.corners[0][a] { p[a] } else { 1.0 - p[a] }
    };
    (along(1), along(3))
}

/// `BlockFluid.getPercentAir`: the share of a cell that is air above a fluid of flow level `m` (a source, 0, is 1/9).
fn percent_air(m: u8) -> f32 {
    ((if m >= 8 { 0 } else { m }) + 1) as f32 / 9.0
}

/// The four top corner heights of the fluid in cell (x, y, z), indexed `[dz][dx]`: `RenderBlocks.func_1224_a` at (x, z), (x + 1, z),
/// (x, z + 1), (x + 1, z + 1). A corner averages the 4 cells around it: a fluid cell weighs its `percent_air` (a source or falling
/// cell counts x10), a cell that is neither the fluid nor solid counts as air, and fluid above it makes the corner 1. A pool is
/// 8/9 high, the corner of one block beside air is lower.
// `meta` reads the 4 side neighbours' metadata too. ponytail: the cell across the chunk corner (no diagonal neighbour is passed) is
// still skipped, so a fluid exactly on a 4-chunk corner is a touch off; upgrade = pass the 4 diagonal chunks.
fn fluid_heights(rid: &dyn Fn(i32, i32, i32) -> u8, meta: &dyn Fn(i32, i32, i32) -> u8, x: i32, y: i32, z: i32) -> [[f32; 2]; 2] {
    let kind = rid(x, y, z) >> 1;
    let same = |id: u8| is_fluid(id) && id >> 1 == kind;
    let corner = |cx: i32, cz: i32| -> f32 {
        let (mut sum, mut n) = (0.0f32, 0.0f32);
        for i in 0..4 {
            let (px, pz) = (cx - (i & 1), cz - (i >> 1 & 1));
            let (in_x, in_z) = ((0..16).contains(&px), (0..16).contains(&pz));
            if !in_x && !in_z {
                continue;
            }
            if same(rid(px, y + 1, pz)) {
                return 1.0;
            }
            let id = rid(px, y, pz);
            if same(id) {
                let m = meta(px, y, pz);
                if m >= 8 || m == 0 {
                    sum += percent_air(m) * 10.0;
                    n += 10.0;
                }
                sum += percent_air(m);
                n += 1.0;
            } else if non_solid(id) {
                sum += 1.0;
                n += 1.0;
            }
        }
        1.0 - sum / n
    };
    [[corner(x, z), corner(x + 1, z)], [corner(x, z + 1), corner(x + 1, z + 1)]]
}

/// `BlockFluid.getFlowVector` + `func_293_a`: the angle the fluid in cell (x, y, z) flows at (`atan2(z, x) - pi/2`), `None` when it
/// does not flow sideways. The vector adds up, over the 4 side neighbours, the difference of their flow level to ours (a neighbour
/// that is not fluid and not solid counts the level of the cell below it, offset by 8). The falling-cell correction of the Java only
/// rescales x and z, so it does not change the angle.
fn flow_angle(rid: &dyn Fn(i32, i32, i32) -> u8, meta: &dyn Fn(i32, i32, i32) -> u8, x: i32, y: i32, z: i32) -> Option<f32> {
    let kind = rid(x, y, z) >> 1;
    let same = |id: u8| is_fluid(id) && id >> 1 == kind;
    // `getEffectiveFlowDecay`: -1 for anything but this fluid, a falling cell (>= 8) counts as 0.
    let decay = |cx: i32, cy: i32, cz: i32| -> i32 {
        if !same(rid(cx, cy, cz)) {
            return -1;
        }
        let m = meta(cx, cy, cz);
        (if m >= 8 { 0 } else { m }) as i32
    };
    let own = decay(x, y, z);
    let (mut vx, mut vz) = (0, 0);
    for (dx, dz) in [(-1, 0), (0, -1), (1, 0), (0, 1)] {
        let mut d = decay(x + dx, y, z + dz);
        let w = if d < 0 {
            d = if non_solid(rid(x + dx, y, z + dz)) { decay(x + dx, y - 1, z + dz) } else { -1 };
            if d < 0 { continue; }
            d - (own - 8)
        } else {
            d - own
        };
        vx += dx * w;
        vz += dz * w;
    }
    (vx != 0 || vz != 0).then(|| (vz as f32).atan2(vx as f32) - std::f32::consts::FRAC_PI_2)
}

/// UV of a corner of a flowing fluid's top (`renderBlockFluids`): the flowing tile turned by `angle`, a square of 16 px
/// around the tile's middle. Vanilla centres it on the corner of 4 flow tiles; this atlas has only one (tile 255 is the birch
/// leaf), so the square is shrunk to stay inside the tile, whatever the angle.
fn flow_uv(blk: u8, angle: f32, corner: &[f32; 3]) -> (f32, f32) {
    let t = atlas::terrain_tile(blk, 0, 2).unwrap_or(0);
    let k = 0.98 / (angle.sin().abs() + angle.cos().abs());
    let (s, c) = (angle.sin() * 8.0 * k, angle.cos() * 8.0 * k);
    let (du, dv) = match (corner[0] as u8, corner[2] as u8) {
        (0, 0) => (-c - s, -c + s),
        (0, 1) => (-c + s, c + s),
        (1, 1) => (c + s, c - s),
        _ => (c - s, -c - s),
    };
    atlas::terrain_px(t, 8.0 + du, 8.0 + dv)
}

/// Build vertex + index buffers for one chunk.
/// `data` is this chunk's block metadata: it picks the atlas tile of a cell (wool colour, wood species); the
/// neighbours' metadata is never needed, a neighbour only has to be see-through or not.
/// `nb` / `nb_light` are the blocks and light of the neighbouring chunks, in the order +X, -X, +Z, -Z;
/// they are only read for the one-cell border. `light` holds sky light in the low nibble and block
/// light in the high nibble. `sky_sub` is `World.skylightSubtracted` (0 day .. 11 night, `world::sky`).
/// `(ox, oz)` is the chunk's world origin (chunk * 16).
/// Vertices: 6 floats each (px, py, pz, u, v, light). Indices: 6 per face (two triangles).
/// Shadows are not baked here: the sun's shadow map darkens the frame in `render::vl`. Leaves do not dim the sky light either
/// (see `open` below), or a tree would be shaded twice.
pub fn build(blocks: &[u8], data: &Nibbles, light: &[u8], nb: [&[u8]; 4], nb_light: [&[u8]; 4], sky_sub: u8, ox: i32, oz: i32) -> (Vec<f32>, Vec<u32>) {
    let e = Nibbles::new();
    let (verts, idxs, _) = build_split(blocks, data, [&e; 4], light, nb, nb_light, sky_sub, ox, oz);
    (verts, idxs)
}

/// `build`, with the water faces (blocks 8, 9: `getRenderBlockPass` 1, drawn blended after everything else) as a second index list
/// into the same vertices: `(vertices, opaque indices, water indices)`. `build` leaves them out of its indices.
pub fn build_split(blocks: &[u8], data: &Nibbles, nb_data: [&Nibbles; 4], light: &[u8], nb: [&[u8]; 4], nb_light: [&[u8]; 4], sky_sub: u8, ox: i32, oz: i32) -> (Vec<f32>, Vec<u32>, Vec<u32>) {
    assert_eq!(blocks.len(), VOLUME);
    assert_eq!(light.len(), VOLUME);
    // Which chunk's arrays a cell with x, z in -1..=16 lives in: 0 = this one, 1.. = +X, -X, +Z, -Z.
    let slot = |x: i32, z: i32| if x < 0 { 2 } else if x >= 16 { 1 } else if z < 0 { 4 } else if z >= 16 { 3 } else { 0 };
    // Raw id of a cell for x, z in -1..=16 (one cell outside the chunk); above/below the world is air.
    let rid = |x: i32, y: i32, z: i32| -> u8 {
        if y < 0 || y >= H as i32 { return 0; }
        let b = match slot(x, z) { 0 => blocks, s => nb[s - 1] };
        b[idx((x & 15) as usize, y as usize, (z & 15) as usize)]
    };
    // Metadata of a cell for x, z in -1..=16, from this chunk or a side neighbour (fluid levels across a seam).
    let meta = |x: i32, y: i32, z: i32| -> u8 {
        if y < 0 || y >= H as i32 { return 0; }
        let d = match slot(x, z) { 0 => data, s => nb_data[s - 1] };
        d.get((x & 15) as usize, y as usize, (z & 15) as usize)
    };
    // Only air, plants and leaves above a cell: the sky is open. Leaves cast their shadow through the shadow map, so they must
    // not also dim the sky light (vanilla takes 1 level per leaf: a hard-edged dark square under every tree).
    let open = |x: i32, y: i32, z: i32| (y + 1..H as i32).all(|yy| matches!(rid(x, yy, z), 0 | 18) || is_plant(rid(x, yy, z)));
    // Brightness of a cell (`World.getBlockLightValue`): open sky above the world, dark below it.
    let table: [f32; 16] = std::array::from_fn(|i| brightness(i as u8));
    let bright = |x: i32, y: i32, z: i32| -> f32 {
        if y < 0 { return table[0]; }
        if y >= H as i32 { return table[15usize.saturating_sub(sky_sub as usize)]; }
        let l = match slot(x, z) { 0 => light, s => nb_light[s - 1] };
        let v = l[idx((x & 15) as usize, y as usize, (z & 15) as usize)];
        let sky: u8 = if (v & 15) < 15 && open(x, y, z) { 15 } else { v & 15 };
        table[sky.saturating_sub(sky_sub).max(v >> 4) as usize]
    };
    let mut verts: Vec<f32> = Vec::new();
    let mut idxs: Vec<u32> = Vec::new();
    let mut widxs: Vec<u32> = Vec::new();

    for y in 0..H as i32 {
        for z in 0..16 {
            for x in 0..16 {
                // Plants: two crossed quads, visible from both sides, lit by their own cell.
                let raw = blocks[idx(x as usize, y as usize, z as usize)];
                if raw == 50 {
                    push_torch(&mut verts, &mut idxs, [(ox + x) as f32, y as f32, (oz + z) as f32], data.get(x as usize, y as usize, z as usize));
                    continue;
                }
                if raw == 59 {
                    push_crops(&mut verts, &mut idxs, [(ox + x) as f32, y as f32, (oz + z) as f32], data.get(x as usize, y as usize, z as usize), bright(x, y, z));
                    continue;
                }
                if cross_shape(raw).is_some() {
                    let lit = bright(x, y, z);
                    let meta = data.get(x as usize, y as usize, z as usize);
                    let (cx, cz) = ((ox + x) as f32 + 0.5, (oz + z) as f32 + 0.5);
                    // `RenderBlocks.renderCrossedSquares`: full-height quads 0.9 wide, the texture's cut-out does the shaping.
                    let half = 0.45;
                    for (a, b) in [((-half, -half), (half, half)), ((-half, half), (half, -half))] {
                        let base = verts.len() as u32 / 6;
                        for (k, (px, pz, py)) in [(a.0, a.1, 0.0), (b.0, b.1, 0.0), (b.0, b.1, 1.0), (a.0, a.1, 1.0)].into_iter().enumerate() {
                            let (au, av) = atlas::face_uv(raw, meta, 2, (k == 1 || k == 2) as u8 as f32, (k >= 2) as u8 as f32);
                            verts.extend_from_slice(&[cx + px, y as f32 + py, cz + pz, au, av, lit]);
                        }
                        // Both windings: the pipeline culls back faces.
                        idxs.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3, base, base + 2, base + 1, base, base + 3, base + 2]);
                    }
                    continue;
                }
                if raw == 0 {
                    continue;
                }
                let blk = raw;
                let meta = data.get(x as usize, y as usize, z as usize);
                // Non-full blocks: a box (`box_bounds`) or a fluid whose top corners have their own height.
                let (boxes, n_boxes) = shapes(blk, meta, &|dx, dz| rid(x + dx, y, z + dz) == blk);
                for &bounds in &boxes[..n_boxes] {
                let heights = is_fluid(blk).then(|| fluid_heights(&rid, &meta, x, y, z));
                let flow = if heights.is_some() { flow_angle(&rid, &meta, x, y, z) } else { None };
                for (face_i, face) in FACES.iter().enumerate() {
                    let (dx, dy, dz) = DIRS[face_i];
                    let n = rid(x + dx, y + dy, z + dz);
                    // `Block.shouldSideBeRendered`: a face inside the cell is always drawn, one on its edge only against a see-through
                    // neighbour. A fluid hides against the same fluid and ice, and draws its top whatever lies above.
                    let reach = [bounds[3] >= 1.0, bounds[0] <= 0.0, bounds[4] >= 1.0, bounds[1] <= 0.0, bounds[5] >= 1.0, bounds[2] <= 0.0][face_i];
                    let hidden = if heights.is_some() {
                        (is_fluid(n) && n >> 1 == blk >> 1) || n == 79 || (face_i != 2 && opaque(n))
                    } else {
                        reach && opaque(n)
                    };
                    if hidden {
                        continue;
                    }
                    let base = verts.len() as u32 / 6;
                    // A chest's tile depends on its neighbours (`BlockChest.getBlockTexture`), `face_i` as the Java's side.
                    let chest_tile = (blk == chest::ID).then(|| chest::tile(&rid, (x, y, z), [5, 4, 1, 0, 3, 2][face_i]));
                    let shade = match face_i {
                        2 => 1.0, // +Y top
                        3 => 0.5, // -Y bottom
                        4 | 5 => 0.8, // +Z, -Z
                        _ => 0.6, // +X, -X
                    };
                    // A face the box does not reach the edge with, and a fluid's top, take the light of their own cell.
                    let own_cell = !reach || (heights.is_some() && face_i == 2);
                    let light = shade * if own_cell { bright(x, y, z) } else { bright(x + dx, y + dy, z + dz) };
                    for corner in face.corners.iter() {
                        // Corner inside the cell: the box's, a fluid's top corners at their height.
                        let mut p: [f32; 3] = std::array::from_fn(|a| bounds[a] + corner[a] * (bounds[a + 3] - bounds[a]));
                        if let Some(h) = &heights {
                            p[1] = if corner[1] > 0.5 { h[corner[2] as usize][corner[0] as usize] } else { 0.0 };
                        }
                        // The tile window of the box's face (`renderEastFace`...), the fluid's v is its height.
                        let (u, v) = window(face, p);
                        let (au, av) = match flow {
                            Some(a) if face_i == 2 => flow_uv(blk, a, corner),
                            _ => match chest_tile {
                                Some(t) => atlas::terrain_uv(t, u, v),
                                None => atlas::face_uv(blk, meta, face_i, u, v),
                            },
                        };
                        // `renderBlockCactus` pulls the four sides in; the tile stays whole.
                        if blk == 81 && dy == 0 {
                            p[0] -= dx as f32 * CACTUS_INSET;
                            p[2] -= dz as f32 * CACTUS_INSET;
                        }
                        // ponytail: world-space f32 vertices lose precision far from the origin
                        // (about 1 cm at 100k blocks); upgrade path is a per-chunk offset uniform.
                        verts.extend_from_slice(&[p[0] + (ox + x) as f32, p[1] + y as f32, p[2] + (oz + z) as f32, au, av, light]);
                    }
                    let list = if matches!(blk, 8 | 9) { &mut widxs } else { &mut idxs };
                    list.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
                }
                }
            }
        }
    }
    (verts, idxs, widxs)
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

/// The boxes a block is drawn as (`setBlockBounds` + `renderStandardBlock` per box in `RenderBlocks`): the stairs' two steps
/// (`renderBlockStairs`, metadata & 3), the fence's post + bars (`renderBlockFence`; `same(dx, dz)` = a fence in that neighbour,
/// a bar runs to the cell edge there, else it is a 2/16 stub), everything else one box. Up to 5 boxes.
fn shapes(id: u8, meta: u8, same: &dyn Fn(i32, i32) -> bool) -> ([[f32; 6]; 5], usize) {
    let mut o = [[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]; 5];
    match id {
        53 | 67 => {
            o[..2].copy_from_slice(&match meta & 3 {
                0 => [[0.0, 0.0, 0.0, 0.5, 0.5, 1.0], [0.5, 0.0, 0.0, 1.0, 1.0, 1.0]],
                1 => [[0.0, 0.0, 0.0, 0.5, 1.0, 1.0], [0.5, 0.0, 0.0, 1.0, 0.5, 1.0]],
                2 => [[0.0, 0.0, 0.0, 1.0, 0.5, 0.5], [0.0, 0.0, 0.5, 1.0, 1.0, 1.0]],
                _ => [[0.0, 0.0, 0.0, 1.0, 1.0, 0.5], [0.0, 0.0, 0.5, 1.0, 0.5, 1.0]],
            });
            (o, 2)
        }
        85 => {
            let (nx, px, nz, pz) = (same(-1, 0), same(1, 0), same(0, -1), same(0, 1));
            let along_x = nx || px || !(nz || pz); // no neighbour at all: the Java draws the x stub
            let (a, b) = (7.0 / 16.0, 9.0 / 16.0);
            let (x0, x1, z0, z1) = (if nx { 0.0 } else { a }, if px { 1.0 } else { b }, if nz { 0.0 } else { a }, if pz { 1.0 } else { b });
            o[0] = [6.0 / 16.0, 0.0, 6.0 / 16.0, 10.0 / 16.0, 1.0, 10.0 / 16.0];
            let mut n = 1;
            for (lo, hi) in [(12.0 / 16.0, 15.0 / 16.0), (6.0 / 16.0, 9.0 / 16.0)] {
                if along_x { o[n] = [x0, lo, a, x1, hi, b]; n += 1; }
                if nz || pz { o[n] = [a, lo, z0, b, hi, z1]; n += 1; }
            }
            (o, n)
        }
        _ => {
            o[0] = box_bounds(id, meta).unwrap_or(o[0]);
            (o, 1)
        }
    }
}

/// `RenderBlocks.func_1245_b` (crops): two pairs of crossing quads at 1/4 and 3/4 of the cell, one sixteenth sunk into the farmland,
/// the growth stage (metadata) picks the tile. Visible from both sides.
pub fn push_crops(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, cell: [f32; 3], meta: u8, light: f32) {
    let [x, y, z] = cell;
    let y = y - 1.0 / 16.0;
    // (x0, z0, x1, z1) of each quad's base line
    for (x0, z0, x1, z1) in [(0.25, 0.0, 0.25, 1.0), (0.75, 0.0, 0.75, 1.0), (0.0, 0.25, 1.0, 0.25), (0.0, 0.75, 1.0, 0.75)] {
        let base = verts.len() as u32 / 6;
        for (px, pz, py, u, v) in [(x0, z0, 1.0, 0.0, 0.0), (x0, z0, 0.0, 0.0, 1.0), (x1, z1, 0.0, 1.0, 1.0), (x1, z1, 1.0, 1.0, 0.0)] {
            let (au, av) = atlas::face_uv(59, meta, 2, u, v);
            verts.extend_from_slice(&[x + px, y + py, z + pz, au, av, light]);
        }
        idxs.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3, base, base + 2, base + 1, base, base + 3, base + 2]);
    }
}

/// `RenderBlocks.renderBlockTorch` / `renderTorchAtAngle` for the torch in `cell` (world coordinates of its corner): a 2/16
/// wide stick of four side quads (the tile's cut-out shapes them) and a 2 x 2 texel head at 0.625; a wall torch (metadata
/// 1..=4: wall at -X, +X, -Z, +Z) sits 0.1 off its cell centre, 0.2 up, and leans 0.4 away from the wall. Full bright like
/// any block that emits light, visible from both sides (the pipeline culls back faces).
pub fn push_torch(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, cell: [f32; 3], meta: u8) {
    let ([ox, oy, oz], [tx, tz]) = match meta {
        1 => ([-0.1, 0.2, 0.0], [-0.4, 0.0]),
        2 => ([0.1, 0.2, 0.0], [0.4, 0.0]),
        3 => ([0.0, 0.2, -0.1], [0.0, -0.4]),
        4 => ([0.0, 0.2, 0.1], [0.0, 0.4]),
        _ => ([0.0; 3], [0.0; 2]),
    };
    let (x, y, z) = (cell[0] + ox + 0.5, cell[1] + oy, cell[2] + oz + 0.5);
    let (w, h) = (1.0 / 16.0, 0.625);
    let (hx, hz) = (x + tx * (1.0 - h), z + tz * (1.0 - h));
    // (x, y, z, u, v) with u, v in the tile, v up; the head is texels 7..9 across and 6..8 down.
    let quads: [[[f32; 5]; 4]; 5] = [
        [[hx - w, y + h, hz - w, 0.4375, 0.625], [hx - w, y + h, hz + w, 0.4375, 0.5], [hx + w, y + h, hz + w, 0.5625, 0.5], [hx + w, y + h, hz - w, 0.5625, 0.625]],
        [[x - w, y + 1.0, z - 0.5, 0.0, 1.0], [x - w + tx, y, z - 0.5 + tz, 0.0, 0.0], [x - w + tx, y, z + 0.5 + tz, 1.0, 0.0], [x - w, y + 1.0, z + 0.5, 1.0, 1.0]],
        [[x + w, y + 1.0, z + 0.5, 0.0, 1.0], [x + w + tx, y, z + 0.5 + tz, 0.0, 0.0], [x + w + tx, y, z - 0.5 + tz, 1.0, 0.0], [x + w, y + 1.0, z - 0.5, 1.0, 1.0]],
        [[x - 0.5, y + 1.0, z + w, 0.0, 1.0], [x - 0.5 + tx, y, z + w + tz, 0.0, 0.0], [x + 0.5 + tx, y, z + w + tz, 1.0, 0.0], [x + 0.5, y + 1.0, z + w, 1.0, 1.0]],
        [[x + 0.5, y + 1.0, z - w, 0.0, 1.0], [x + 0.5 + tx, y, z - w + tz, 0.0, 0.0], [x - 0.5 + tx, y, z - w + tz, 1.0, 0.0], [x - 0.5, y + 1.0, z - w, 1.0, 1.0]],
    ];
    for q in quads {
        let base = verts.len() as u32 / 6;
        for p in q {
            let (u, v) = atlas::terrain_uv(80, p[3], p[4]);
            verts.extend_from_slice(&[p[0], p[1], p[2], u, v, 1.0]);
        }
        idxs.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3, base, base + 2, base + 1, base, base + 3, base + 2]);
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

    /// Chests take their tile from the neighbours: a lone chest has its front (27) on +Z, two side by side show the two
    /// halves of the large-chest front (41 | 42) instead. Faces are emitted +X, -X, +Y, -Y, +Z, -Z per block (hidden ones
    /// skipped); the first corner of a face is (0, 0) of its tile.
    #[test]
    fn chests_pick_their_tile_from_the_neighbours() {
        let a = air();
        let u_of = |cells: &[usize], face: usize| {
            let mut c = air();
            cells.iter().for_each(|&i| c[i] = chest::ID);
            build(&c, &Nibbles::new(), &a, [&a[..], &a[..], &a[..], &a[..]], [&a[..], &a[..], &a[..], &a[..]], 0, 0, 0).0[face * 4 * 6 + 3]
        };
        assert_eq!(u_of(&[idx(5, 20, 5)], 4), atlas::terrain_uv(27, 0.0, 0.0).0);
        // Block 5 emits -X, +Y, -Y, +Z, -Z (+X touches the other chest): its +Z is face 3; block 6's is face 5 + 3.
        let two = [idx(5, 20, 5), idx(6, 20, 5)];
        assert_eq!((u_of(&two, 3), u_of(&two, 8)), (atlas::terrain_uv(41, 0.0, 0.0).0, atlas::terrain_uv(42, 0.0, 0.0).0));
    }

    /// One block in the middle of a chunk is exactly one cube.
    #[test]
    fn single_block_is_one_cube() {
        let mut c = air();
        c[idx(5, 20, 5)] = 1;
        let a = air();
        let (verts, _) = build(&c, &Nibbles::new(), &a, [&a[..], &a[..], &a[..], &a[..]], [&a[..], &a[..], &a[..], &a[..]], 0, 0, 0);
        assert_eq!(face_count(&verts), 6);
    }

    /// Leaves above a cell do not dim its sky light (the shadow map darkens it, `render::vl`); a stone roof still does.
    #[test]
    fn leaves_do_not_shade_like_a_roof() {
        let (l, e) = (vec![12u8; VOLUME], air()); // sky light 12 everywhere, empty neighbours
        let top = |roof: u8| {
            let mut c = air();
            c[idx(5, 0, 5)] = 1;
            c[idx(5, 5, 5)] = roof;
            let v = build(&c, &Nibbles::new(), &l, [&e[..], &e[..], &e[..], &e[..]], [&l[..], &l[..], &l[..], &l[..]], 0, 0, 0).0;
            // The top face has the largest face shade (1.0), so it is the brightest vertex at y = 1.
            v.chunks(6).filter(|q| q[1] == 1.0).map(|q| q[5]).fold(0.0, f32::max)
        };
        assert_eq!(top(18), brightness(15));
        assert_eq!(top(1), brightness(12));
    }

    /// The cell's metadata picks the atlas tile of its faces: red wool is not white wool.
    #[test]
    fn metadata_picks_the_face_tile() {
        let a = air();
        let mut c = air();
        c[idx(5, 20, 5)] = 35;
        let uv = |d: &Nibbles| {
            let v = build(&c, d, &a, [&a[..], &a[..], &a[..], &a[..]], [&a[..], &a[..], &a[..], &a[..]], 0, 0, 0).0;
            (v[3], v[4])
        };
        let mut d = Nibbles::new();
        let white = uv(&d);
        d.set(5, 20, 5, 14);
        assert_ne!(uv(&d), white);
        assert_eq!(uv(&d), atlas::terrain_uv(129, 0.0, 0.0)); // red wool: the first corner is the corner of tile 129
    }

    /// A plant is two crossed quads, and does not hide the top of the block under it.
    #[test]
    fn plant_is_two_crossed_quads() {
        let a = air();
        let mut c = air();
        c[idx(5, 20, 5)] = 37;
        assert_eq!(face_count(&build(&c, &Nibbles::new(), &a, [&a[..], &a[..], &a[..], &a[..]], [&a[..], &a[..], &a[..], &a[..]], 0, 0, 0).0), 2);
        c[idx(5, 19, 5)] = 3; // dirt under the flower: 6 faces + 2 quads
        assert_eq!(face_count(&build(&c, &Nibbles::new(), &a, [&a[..], &a[..], &a[..], &a[..]], [&a[..], &a[..], &a[..], &a[..]], 0, 0, 0).0), 8);
    }

    /// A block on the chunk edge hides the face it shares with the neighbour chunk's block,
    /// for every one of the four sides.
    #[test]
    fn edge_face_is_culled_against_neighbour() {
        let a = air();
        let mk = |x: usize, z: usize| { let mut c = air(); c[idx(x, 20, z)] = 1; c };
        let faces = |me: &[u8], nb: [&[u8]; 4]| face_count(&build(me, &Nibbles::new(), &a, nb, [&a[..], &a[..], &a[..], &a[..]], 0, 0, 0).0);
        let (px, nx, pz, nz) = (mk(0, 5), mk(15, 5), mk(5, 0), mk(5, 15));
        let (a_px, a_nx, a_pz, a_nz) = (mk(15, 5), mk(0, 5), mk(5, 15), mk(5, 0));
        assert_eq!(faces(&a_px[..], [&px[..], &a[..], &a[..], &a[..]]), 5);
        assert_eq!(faces(&a_nx[..], [&a[..], &nx[..], &a[..], &a[..]]), 5);
        assert_eq!(faces(&a_pz[..], [&a[..], &a[..], &pz[..], &a[..]]), 5);
        assert_eq!(faces(&a_nz[..], [&a[..], &a[..], &a[..], &nz[..]]), 5);
        // Without the neighbour the same block shows all six.
        assert_eq!(faces(&a_px[..], [&a[..], &a[..], &a[..], &a[..]]), 6);
    }

    fn mesh(c: &[u8], d: &Nibbles) -> Vec<f32> {
        let a = air();
        build(c, d, &a, [&a[..], &a[..], &a[..], &a[..]], [&a[..], &a[..], &a[..], &a[..]], 0, 0, 0).0
    }

    /// A sapling is crossed quads like a flower (it used to be a solid cube), and its tile follows the species.
    #[test]
    fn sapling_is_two_crossed_quads() {
        let mut c = air();
        c[idx(5, 20, 5)] = 6;
        assert_eq!(face_count(&mesh(&c, &Nibbles::new())), 2);
    }

    /// Still water in a 4x4 pool: the surface is 8/9 high where 4 water cells meet, full height under more water, and the
    /// pool's side faces next to its own water are not drawn.
    #[test]
    fn pool_surface_is_eight_ninths_high() {
        let mut c = air();
        for x in 4..8 { for z in 4..8 { c[idx(x, 10, z)] = 9; } }
        let v = mesh(&c, &Nibbles::new());
        let top = |x: f32, z: f32| v.chunks(6).find(|q| q[0] == x && q[2] == z && q[1] > 10.0 && q[1] < 11.0).map(|q| q[1]);
        assert!((top(6.0, 6.0).unwrap() - (10.0 + 8.0 / 9.0)).abs() < 1e-5);
        // 16 tops + 16 bottoms + the 16 outer sides; the 24 inner sides are the same fluid.
        assert_eq!(face_count(&v), 16 + 16 + 16);
        c[idx(6, 11, 6)] = 9; // water on top of a cell at the corner (6, 6): no surface is left below 11 there
        let v = mesh(&c, &Nibbles::new());
        assert!(v.chunks(6).all(|q| !(q[0] == 6.0 && q[2] == 6.0 && q[1] > 10.0 && q[1] < 11.0)));
    }

    /// A single slab is a box half a block high; the face on top is drawn although the neighbour above is air, and a block
    /// above it does not hide it (the top is inside the cell no more than the half).
    #[test]
    fn slab_is_half_a_block() {
        let mut c = air();
        c[idx(5, 20, 5)] = 44;
        let v = mesh(&c, &Nibbles::new());
        assert_eq!(face_count(&v), 6);
        assert!(v.chunks(6).all(|q| q[1] >= 20.0 && q[1] <= 20.5));
        c[idx(5, 21, 5)] = 1; // stone above the slab: the slab's top (at 20.5) is still drawn, the stone's bottom too
        assert_eq!(face_count(&mesh(&c, &Nibbles::new())), 6 + 6);
    }

    /// The cactus keeps full-size tiles on side faces 1/16 inside the cell (`renderBlockCactus`), and its top stays at the edge.
    #[test]
    fn cactus_sides_are_pulled_in() {
        let mut c = air();
        c[idx(5, 20, 5)] = 81;
        let v = mesh(&c, &Nibbles::new());
        assert_eq!(face_count(&v), 6);
        let (lo, hi) = (5.0 + 1.0 / 16.0, 6.0 - 1.0 / 16.0);
        assert!(v.chunks(6).any(|q| q[0] == hi) && v.chunks(6).any(|q| q[0] == lo) && v.chunks(6).any(|q| q[2] == hi));
        assert!(v.chunks(6).any(|q| q[0] == 5.0 && q[1] == 21.0)); // the top face reaches the cell edge
    }

    /// A torch is a 0.2 wide box; its sides show the tile's middle columns (the stick), not the whole tile squeezed in.
    #[test]
    fn torch_shows_the_middle_of_its_tile() {
        let mut c = air();
        c[idx(5, 20, 5)] = 50;
        let v = mesh(&c, &Nibbles::new());
        let us: Vec<f32> = v.chunks(6).map(|q| q[3]).collect();
        let (lo, hi) = (atlas::terrain_uv(80, 0.4, 0.0).0, atlas::terrain_uv(80, 0.6, 0.0).0);
        assert!(us.iter().all(|&u| u >= lo - 1e-6 && u <= hi + 1e-6), "u outside the stick columns");
    }

    /// A source next to a level-1 cell flows toward it (+X): the angle is -pi/2, the same for the cell it flows into; a pool of
    /// sources and a falling column do not flow; every corner of the turned square stays inside the flowing tile.
    #[test]
    fn flow_follows_the_levels() {
        let mut d = Nibbles::new();
        d.set(6, 10, 5, 1);
        let rid = |x: i32, y: i32, z: i32| -> u8 { match (x, y, z) { (5, 10, 5) => 9, (6, 10, 5) => 8, _ => 0 } };
        let m = |x: i32, y: i32, z: i32| d.get(x as usize, y as usize, z as usize);
        let (a, b) = (flow_angle(&rid, &m, 5, 10, 5).unwrap(), flow_angle(&rid, &m, 6, 10, 5).unwrap());
        assert!((a + std::f32::consts::FRAC_PI_2).abs() < 1e-6 && (b - a).abs() < 1e-6, "{a} {b}");
        let pool = |x: i32, y: i32, z: i32| -> u8 { if y == 10 && (4..8).contains(&x) && (4..8).contains(&z) { 9 } else { 0 } };
        assert_eq!(flow_angle(&pool, &|_, _, _| 0, 5, 10, 5), None);
        let (lo, hi) = (atlas::terrain_px(206, 0.0, 0.0), atlas::terrain_px(206, 16.0, 16.0));
        for i in 0..64 {
            let angle = i as f32 * std::f32::consts::TAU / 64.0;
            for corner in [[0.0, 1.0, 0.0], [0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0]] {
                let (u, v) = flow_uv(8, angle, &corner);
                assert!(u >= lo.0 && u <= hi.0 && v >= lo.1 && v <= hi.1, "angle {angle}");
            }
        }
    }

    /// Water faces are their own index list (blended after everything else); lava and stone stay in the opaque one.
    #[test]
    fn water_has_its_own_indices() {
        let a = air();
        let mut c = air();
        c[idx(5, 20, 5)] = 9;
        c[idx(8, 20, 5)] = 11;
        c[idx(11, 20, 5)] = 1;
        let (v, o, w) = build_split(&c, &Nibbles::new(), [&Nibbles::new(); 4], &a, [&a[..], &a[..], &a[..], &a[..]], [&a[..], &a[..], &a[..], &a[..]], 0, 0, 0);
        assert_eq!((w.len(), o.len()), (6 * 6, 12 * 6)); // 6 faces of 6 indices for the water; lava and stone 12 faces
        assert!(w.iter().all(|&i| (5.0..=6.0).contains(&v[i as usize * 6])));
    }

    /// A level-1 cell in the +X neighbour is read as level 1, not as a source: the source beside it flows toward +X across the seam.
    #[test]
    fn flow_reads_the_neighbour_chunk_levels() {
        let mut nb = Nibbles::new();
        nb.set(0, 10, 5, 1);
        let rid = |x: i32, y: i32, z: i32| -> u8 { if y == 10 && z == 5 && (x == 15 || x == 16) { 9 } else { 0 } };
        let m = |x: i32, y: i32, z: i32| if x >= 16 { nb.get((x & 15) as usize, y as usize, z as usize) } else { 0 };
        let a = flow_angle(&rid, &m, 15, 10, 5).unwrap();
        assert!((a + std::f32::consts::FRAC_PI_2).abs() < 1e-6, "{a}");
    }

    /// A torch is a stick (4 quads) + a head quad; a wall torch leans away from its wall (the foot sits nearer the wall than the
    /// top) and an upright one stays inside its cell.
    #[test]
    fn torch_is_a_stick_that_leans_off_the_wall() {
        let mk = |meta: u8| {
            let mut c = air();
            c[idx(5, 20, 5)] = 50;
            let mut d = Nibbles::new();
            d.set(5, 20, 5, meta);
            mesh(&c, &d)
        };
        let (floor, wall) = (mk(5), mk(1));
        assert_eq!((face_count(&floor), face_count(&wall)), (5, 5));
        let xs = |v: &[f32], top: bool| v.chunks(6).filter(|p| (p[1] > 20.5) == top).map(|p| p[0]).fold(f32::MAX, f32::min);
        assert!(floor.chunks(6).all(|p| (5.0..=6.0).contains(&p[0]) && p[5] == 1.0));
        assert!((xs(&floor, false) - xs(&floor, true)).abs() < 1e-5, "upright: foot under the top");
        assert!(xs(&wall, false) < xs(&wall, true) - 0.3, "wall torch on the -X wall: foot toward -X");
    }

    /// Stairs are two boxes (the step and the riser), a lone fence a post and a stub, a fence beside a fence a bar to the edge,
    /// crops four quads; none hides the stone beside it.
    #[test]
    fn stairs_fence_and_crops_have_their_shapes() {
        let one = |id: u8, meta: u8| { let mut c = air(); c[idx(5, 20, 5)] = id; mesh(&c, &{ let mut d = Nibbles::new(); d.set(5, 20, 5, meta); d }) };
        assert_eq!(face_count(&one(53, 0)), 12, "two boxes, 6 faces each");
        assert_eq!(face_count(&one(59, 3)), 4);
        let lone = one(85, 0);
        assert_eq!(face_count(&lone), 3 * 6, "post + the two stubs along x (top and low bar)");
        assert!(!lone.chunks(6).any(|p| (p[0] - 6.0).abs() < 1e-5));
        let mut c = air();
        c[idx(5, 20, 5)] = 85;
        c[idx(6, 20, 5)] = 85;
        let v = mesh(&c, &Nibbles::new());
        assert!(v.chunks(6).any(|p| (p[0] - 6.0).abs() < 1e-5), "the bar of the first fence reaches the second");
        let (s, b) = ([0.0, 0.0, 0.0, 1.0, 1.0, 1.0f32], shapes(53, 1, &|_, _| false));
        assert!(b.1 == 2 && b.0[0] != s && !opaque(53) && !opaque(85) && !opaque(59));
    }
}
