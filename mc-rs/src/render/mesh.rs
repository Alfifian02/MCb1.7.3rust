//! Chunk mesher.
//!
//! One mesh per 16x128x16 chunk. Faces are emitted only where the neighbouring cell is air;
//! at the chunk edge the neighbour is read from the adjacent chunk, so no faces are wasted on
//! the seams between loaded chunks. Vertex positions are world coordinates plus the `ox`, `oz` it is given (`world::chunks::build_job` passes 0: the
//! chunk-local positions the 12-byte chunk vertex wants; `chunks::pack` turns this output into it).
//!
//! Vertex format: position(3) + uv(2) + light(1) + lm(1) + tint(1) = `VF` (8) floats = 32 bytes (`gpu::pipeline::Vertex`).
//! `light` = face shade (RenderBlocks: top 1.0, bottom 0.5, Z sides 0.8, X sides 0.6) times the
//! brightness of the cell the face looks into (`Block.getBlockBrightness` -> `lightBrightnessTable`).

use crate::render::atlas;
use crate::world::chest;
use crate::world::chunk::{box_bounds, brightness, cross_shape, stair_boxes, idx, is_fluid, is_plant, opaque, Nibbles, CACTUS_INSET, H, VOLUME};
use crate::world::climate;
use crate::world::ticks::non_solid;

/// Water vertices pack `light * WATER_LIGHT + column depth` (see `build_split`); `fs_water` in `gpu/pipeline.rs` unpacks it.
pub const WATER_LIGHT: f32 = 0.9;

/// Floats per vertex: position(3) + uv(2) + light(1) + lm(1) + tint(1).
pub const VF: usize = 8;
/// `tint` packs an sRGB colour as `r << 16 | g << 8 | b` (exact in an f32): the texel is multiplied by it in the vertex shader\n/// (`ColorizerGrass` / `ColorizerFoliage`, per block column). No tint is white.
pub const WHITE: f32 = 16_777_215.0;

/// Grass and foliage colour (packed like `WHITE`) of the 16 x 16 columns of a chunk, index `x * 16 + z`.
pub struct Tints {
    pub grass: [f32; 256],
    pub foliage: [f32; 256],
}

impl Tints {
    pub fn white() -> Self {
        Self { grass: [WHITE; 256], foliage: [WHITE; 256] }
    }
}

/// `Block.colorMultiplier` of the blocks that have one: grass top (the side overlay uses it too), tall grass (not the dead\n/// shrub, metadata 0), reeds, leaves (spruce / birch by metadata, else the foliage colour).
pub(crate) fn tint_of(t: &Tints, id: u8, meta: u8, col: usize) -> f32 {
    match id {
        2 | 83 => t.grass[col],
        31 if meta != 0 => t.grass[col],
        18 => match meta & 3 { 1 => climate::PINE as f32, 2 => climate::BIRCH as f32, _ => t.foliage[col] },
        _ => WHITE,
    }
}
/// Packed light levels of a vertex, `sky * 16 + block` (each 0..=15, exact in f32 and f16): the two numbers photon's lighting
/// reads separately as `light_levels` (`lmcoord`). `sky` is the RAW sky light, not minus `skylight_subtracted`: day and night
/// come from the light colours, so a mesh no longer has to be relit when the sun moves. `light` (shade x brightness) stays for the
/// water depth packing and the old flat consumers.
pub fn lm(sky: u8, block: u8) -> f32 {
    (sky.min(15) * 16 + block.min(15)) as f32
}

/// Smooth vertices carry `SMOOTH_LM + sky_q * 32 + block_h` in `lm` (flat ones keep `sky * 16 + block`, always below this):
/// `sky_q` = four cells' sky levels summed (quarter levels, 0..=60), `block_h` = their block levels averaged in half levels
/// (0..=30), so `sky_q * 32 + block_h <= 1950` stays exact in the G-buffer's f16. `light` then holds `face shade * ambient occlusion`.
/// `vs_main` in `gpu/pipeline.rs` unpacks both kinds.
pub const SMOOTH_LM: f32 = 256.0;

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
/// Vertices: `VF` floats each (px, py, pz, u, v, light, lm). Indices: 6 per face (two triangles).
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
    build_split_with(false, blocks, data, nb_data, light, nb, nb_light, sky_sub, ox, oz)
}

/// `build_split`; `smooth` (Console-Edition mode) gives every full face per-corner light and ambient occlusion, see `SMOOTH_LM`.
pub fn build_split_with(smooth: bool, blocks: &[u8], data: &Nibbles, nb_data: [&Nibbles; 4], light: &[u8], nb: [&[u8]; 4], nb_light: [&[u8]; 4], sky_sub: u8, ox: i32, oz: i32) -> (Vec<f32>, Vec<u32>, Vec<u32>) {
    build_split_tinted(smooth, blocks, data, nb_data, light, nb, nb_light, sky_sub, ox, oz, &Tints::white())
}

/// `build_split_with` with the biome colours of the chunk's columns (`Tints`).
pub fn build_split_tinted(smooth: bool, blocks: &[u8], data: &Nibbles, nb_data: [&Nibbles; 4], light: &[u8], nb: [&[u8]; 4], nb_light: [&[u8]; 4], sky_sub: u8, ox: i32, oz: i32, tints: &Tints) -> (Vec<f32>, Vec<u32>, Vec<u32>) {
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
    // Xenon's `LeafCulling.shouldCullSide`: a leaf face against a leaf is dropped when the next `leaf_depth - 1` cells that way are leaves
    // too. Depth 2 (Xenon's HOLLOW) keeps the faces within 2 cells of the air, so a see-through texel still shows the next leaf. When no
    // leaf tile has a see-through texel (the shipped `terrain.rgba`: tiles 53, 133, 255) a leaf is an opaque cube, a face between two
    // leaves can never be seen, and depth 1 drops every one of them (Xenon's SOLID / SOLID_AGGRESSIVE end up there too: both remove the
    // faces of leaves enclosed by leaves). Such a leaf face is also in the `solid` layer already (`atlas::is_cutout`: no `discard`).
    // ponytail: a cell the snapshot does not hold (x or z outside -1..=16) is "not a leaf", so at depth 2 a face on the chunk border stays.
    let leaf_depth = leaf_depth();
    let leaf = |x: i32, y: i32, z: i32| (-1..=16).contains(&x) && (-1..=16).contains(&z) && rid(x, y, z) == 18;
    // Metadata of a cell for x, z in -1..=16, from this chunk or a side neighbour (fluid levels across a seam).
    let meta_at = |x: i32, y: i32, z: i32| -> u8 {
        if y < 0 || y >= H as i32 { return 0; }
        let d = match slot(x, z) { 0 => data, s => nb_data[s - 1] };
        d.get((x & 15) as usize, y as usize, (z & 15) as usize)
    };
    // Only air, plants and leaves above a cell: the sky is open. Leaves cast their shadow through the shadow map, so they must
    // not also dim the sky light (vanilla takes 1 level per leaf: a hard-edged dark square under every tree).
    let open = |x: i32, y: i32, z: i32| (y + 1..H as i32).all(|yy| matches!(rid(x, yy, z), 0 | 18) || is_plant(rid(x, yy, z)));
    // Brightness of a cell (`World.getBlockLightValue`): open sky above the world, dark below it.
    let table: [f32; 16] = std::array::from_fn(|i| brightness(i as u8));
    // (raw sky, block) light of a cell; the open-sky rule above lifts a covered-by-leaves cell to 15.
    let lv = |x: i32, y: i32, z: i32| -> (u8, u8) {
        if y < 0 { return (0, 0); }
        if y >= H as i32 { return (15, 0); }
        let l = match slot(x, z) { 0 => light, s => nb_light[s - 1] };
        let v = l[idx((x & 15) as usize, y as usize, (z & 15) as usize)];
        (if (v & 15) < 15 && open(x, y, z) { 15 } else { v & 15 }, v >> 4)
    };
    let br = |(s, b): (u8, u8)| table[s.saturating_sub(sky_sub).max(b) as usize];
    // `RenderBlocks.renderStandardBlockWithAmbientOcclusion`: the light of a face corner is the mean of the four cells around it (the
    // face's own cell, the two beside it along the face, and the diagonal); an opaque cell counts as 0.2 for the occlusion and, as in
    // later versions, takes the face cell's light instead of its own 0. A diagonal behind two opaque sides is that side. A cell
    // beyond the chunk's corner (x and z both outside) is not loaded here: it counts as open. -> (sky quarters, block halves, ao)
    let corner_light = |cell: [i32; 3], d: [i32; 3], corner: &[f32; 3]| -> (f32, f32, f32) {
        let axes: Vec<usize> = (0..3).filter(|&a| d[a] == 0).collect();
        let base = [cell[0] + d[0], cell[1] + d[1], cell[2] + d[2]];
        let at = |o1: i32, o2: i32| {
            let mut c = base;
            c[axes[0]] += o1 * if corner[axes[0]] > 0.5 { 1 } else { -1 };
            c[axes[1]] += o2 * if corner[axes[1]] > 0.5 { 1 } else { -1 };
            c
        };
        let centre = lv(base[0], base[1], base[2]);
        let get = |c: [i32; 3]| -> Option<((u8, u8), f32)> {
            if (c[0] < 0 || c[0] >= 16) && (c[2] < 0 || c[2] >= 16) { return None; }
            let solid = opaque(rid(c[0], c[1], c[2]));
            Some((if solid { centre } else { lv(c[0], c[1], c[2]) }, if solid { 0.2 } else { 1.0 }))
        };
        let open_cell = (centre, 1.0);
        let (s1, s2) = (get(at(1, 0)).unwrap_or(open_cell), get(at(0, 1)).unwrap_or(open_cell));
        let diag = if s1.1 < 1.0 && s2.1 < 1.0 { s1 } else { get(at(1, 1)).unwrap_or(open_cell) };
        let cells = [(centre, 1.0), s1, s2, diag];
        let sky: u32 = cells.iter().map(|c| c.0 .0 as u32).sum();
        let blk: u32 = cells.iter().map(|c| c.0 .1 as u32).sum();
        (sky as f32, (blk as f32 / 2.0).round(), cells.iter().map(|c| c.1).sum::<f32>() / 4.0)
    };
    let mut verts: Vec<f32> = Vec::new();
    let mut idxs: Vec<u32> = Vec::new();
    let mut widxs: Vec<u32> = Vec::new();

    for y in 0..H as i32 {
        for z in 0..16 {
            for x in 0..16 {
                // Plants: two crossed quads, visible from both sides, lit by their own cell.
                let raw = blocks[idx(x as usize, y as usize, z as usize)];
                if matches!(raw, 93 | 94) {
                    // `RenderBlocks.renderBlockRedstoneRepeater`: the fixed torch in front, the one that shows the delay behind it.
                    let meta = data.get(x as usize, y as usize, z as usize);
                    let a = [-0.0625, 0.0625, 0.1875, 0.3125][((meta & 12) >> 2) as usize];
                    let (front, delay) = match meta & 3 { 0 => ((0.0, -0.3125), (0.0, a)), 1 => ((0.3125, 0.0), (-a, 0.0)), 2 => ((0.0, 0.3125), (0.0, -a)), _ => ((-0.3125, 0.0), (a, 0.0)) };
                    for (dx, dz) in [front, delay] {
                        push_torch(&mut verts, &mut idxs, [(ox + x) as f32 + dx, y as f32 - 0.2, (oz + z) as f32 + dz], 5, if raw == 94 { 99 } else { 115 });
                    }
                }
                if matches!(raw, 50 | 75 | 76) {
                    push_torch(&mut verts, &mut idxs, [(ox + x) as f32, y as f32, (oz + z) as f32], data.get(x as usize, y as usize, z as usize), match raw { 50 => 80, 76 => 99, _ => 115 });
                    continue;
                }
                if raw == 59 {
                    { let l = lv(x, y, z); push_crops(&mut verts, &mut idxs, [(ox + x) as f32, y as f32, (oz + z) as f32], data.get(x as usize, y as usize, z as usize), br(l), lm(l.0, l.1)); }
                    continue;
                }
                if raw == 51 {
                    push_fire(&mut verts, &mut idxs, [(ox + x) as f32, y as f32, (oz + z) as f32]);
                    continue;
                }
                if matches!(raw, 27 | 28 | 55 | 65 | 66) {
                    let l = lv(x, y, z);
                    let meta = data.get(x as usize, y as usize, z as usize);
                    let n0 = verts.len();
                    push_flat(&mut verts, &mut idxs, raw, meta, [(ox + x) as f32, y as f32, (oz + z) as f32], br(l), lm(l.0, l.1));
                    if raw == 55 {
                        // `BlockRedstoneWire.colorMultiplier` for the level: dull red off, bright red on.
                        let f = meta as f32 / 15.0;
                        let (r, g, b) = (if meta == 0 { 0.3 } else { f * 0.6 + 0.4 }, (f * f * 0.7 - 0.5).max(0.0), (f * f * 0.6 - 0.7).max(0.0));
                        let tint = ((r * 255.0) as u32) << 16 | ((g * 255.0) as u32) << 8 | (b * 255.0) as u32;
                        verts[n0..].chunks_mut(VF).for_each(|v| v[7] = tint as f32);
                    }
                    continue;
                }
                if cross_shape(raw).is_some() {
                    let l = lv(x, y, z);
                    // Full shade (`renderCrossedSquares` sets the plain brightness, no face factor), sent as a smooth vertex so `fs_main` takes
                    // the shade from the vertex. As a flat one it derived the face from the screen derivatives, and a quad on the diagonal
                    // has no axis face: x and z tie, the pick flipped per pixel (0.6 against 0.8) and the plant wore light and dark patches.
                    let (lit, lmv) = (1.0, SMOOTH_LM + l.0.min(15) as f32 * 4.0 * 32.0 + l.1.min(15) as f32 * 2.0);
                    let meta = data.get(x as usize, y as usize, z as usize);
                    let (cx, cz) = ((ox + x) as f32 + 0.5, (oz + z) as f32 + 0.5);
                    // `RenderBlocks.renderCrossedSquares`: full-height quads 0.9 wide, the texture's cut-out does the shaping.
                    let half = 0.45;
                    for (a, b) in [((-half, -half), (half, half)), ((-half, half), (half, -half))] {
                        let base = verts.len() as u32 / VF as u32;
                        for (k, (px, pz, py)) in [(a.0, a.1, 0.0), (b.0, b.1, 0.0), (b.0, b.1, 1.0), (a.0, a.1, 1.0)].into_iter().enumerate() {
                            let (au, av) = atlas::face_uv(raw, meta, 2, (k == 1 || k == 2) as u8 as f32, (k >= 2) as u8 as f32);
                            verts.extend_from_slice(&[cx + px, y as f32 + py, cz + pz, au, av, lit, lmv, tint_of(tints, raw, meta, (x * 16 + z) as usize)]);
                        }
                        // Both windings: the pipeline culls back faces.
                        idxs.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3, base, base + 2, base + 1, base, base + 3, base + 2]);
                    }
                    continue;
                }
                // Sign posts and wall signs (`getRenderType` -1): no block shape, `render::items` draws the model.
                if raw == 0 || matches!(raw, 36 | 63 | 68) { // 36: the moving piston block is drawn by `Falling` pieces
                    continue;
                }
                let blk = raw;
                let meta = data.get(x as usize, y as usize, z as usize);
                // Non-full blocks: a box (`box_bounds`) or a fluid whose top corners have their own height.
                let (boxes, n_boxes) = shapes(blk, meta, &|dx, dz| rid(x + dx, y, z + dz) == blk);
                for &bounds in &boxes[..n_boxes] {
                let heights = is_fluid(blk).then(|| fluid_heights(&rid, &meta_at, x, y, z));
                let flow = if heights.is_some() { flow_angle(&rid, &meta_at, x, y, z) } else { None };
                for (face_i, face) in FACES.iter().enumerate() {
                    let (dx, dy, dz) = DIRS[face_i];
                    let n = rid(x + dx, y + dy, z + dz);
                    // `Block.shouldSideBeRendered`: a face inside the cell is always drawn, one on its edge only against a see-through
                    // neighbour. A fluid hides against the same fluid and ice, and draws its top whatever lies above.
                    let reach = [bounds[3] >= 1.0, bounds[0] <= 0.0, bounds[4] >= 1.0, bounds[1] <= 0.0, bounds[5] >= 1.0, bounds[2] <= 0.0][face_i];
                    let hidden = if heights.is_some() {
                        (is_fluid(n) && n >> 1 == blk >> 1) || n == 79 || (face_i != 2 && opaque(n))
                    } else {
                        // `BlockLeavesBase.shouldSideBeRendered` (fancy graphics): leaves do not hide against leaves, or a hollow crown shows
                        // the sky through its edge rows as pale dashes (only while a leaf tile has see-through texels, see `leaf_depth`). Only the faces deeper than `leaf_depth` leaves are dropped (Xenon).
                        reach && opaque(n) && (blk != 18 || n != 18 || (2..=leaf_depth).all(|k| leaf(x + dx * k, y + dy * k, z + dz * k)))
                    };
                    if hidden {
                        continue;
                    }
                    let base = verts.len() as u32 / VF as u32;
                    // A chest's tile depends on its neighbours (`BlockChest.getBlockTexture`), `face_i` as the Java's side.
                    let side_of_cell = face_i != 2 && face_i != 3;
                    // `BlockGrass.getBlockTexture`: snow on top turns the side into tile 68 (and drops the overlay).
                    let snowy = blk == 2 && side_of_cell && matches!(rid(x, y + 1, z), 78 | 80);
                    let chest_tile = if blk == chest::ID { Some(chest::tile(&rid, (x, y, z), [5, 4, 1, 0, 3, 2][face_i])) } else if snowy { Some(68) } else { None };
                    // Grass: its top is tinted; a side carries the grass colour for the overlay `fs_main` draws over tile 3 (the shader tints
                    // only that, not the base fringe), the bottom and a snowy side (tile 68) stay white; every other tinted block all its faces.
                    let tint = if blk == 2 && (face_i == 3 || snowy) { WHITE } else { tint_of(tints, blk, meta, (x * 16 + z) as usize) };
                    // Doors draw some faces mirrored (negative tile in the Java).
                    let flip = matches!(blk, 64 | 71) && atlas::door_face(blk, meta, [5, 4, 1, 0, 3, 2][face_i]).1;
                    let shade = match face_i {
                        2 => 1.0, // +Y top
                        3 => 0.5, // -Y bottom
                        4 | 5 => 0.8, // +Z, -Z
                        _ => 0.6, // +X, -X
                    };
                    // A face the box does not reach the edge with, and a fluid's top, take the light of their own cell.
                    let own_cell = !reach || (heights.is_some() && face_i == 2);
                    let lvl = if own_cell { lv(x, y, z) } else { lv(x + dx, y + dy, z + dz) };
                    let lmv = lm(lvl.0, lvl.1);
                    let mut light = shade * br(lvl);
                    // Water carries the depth of its column (water cells from here down, <= 15) in the integer part of `light`:
                    // `fs_water` unpacks `floor` / `fract` and makes deep water opaque and dark. ponytail: whole cells only, and
                    // the column is counted straight down, not along the view ray.
                    if matches!(blk, 8 | 9) {
                        let depth = (0..=y).take_while(|&d| d < 15 && matches!(rid(x, y - d, z), 8 | 9)).count();
                        light = light * WATER_LIGHT + depth as f32;
                    }
                    let smooth_face = smooth && !own_cell && heights.is_none();
                    let mut vls = [0.0f32; 4]; // shade * ao per corner, to pick the quad's diagonal
                    let mut k = 0;
                    for corner in face.corners.iter() {
                        // Corner inside the cell: the box's, a fluid's top corners at their height.
                        let mut p: [f32; 3] = std::array::from_fn(|a| bounds[a] + corner[a] * (bounds[a + 3] - bounds[a]));
                        if let Some(h) = &heights {
                            p[1] = if corner[1] > 0.5 { h[corner[2] as usize][corner[0] as usize] } else { 0.0 };
                        }
                        // The tile window of the box's face (`renderEastFace`...), the fluid's v is its height.
                        let (u, v) = window(face, p);
                        let u = if flip { 1.0 - u } else { u };
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
                        let (vl, vlm) = if smooth_face {
                            let (sq, bh, ao) = corner_light([x, y, z], [dx, dy, dz], corner);
                            (shade * ao, SMOOTH_LM + sq * 32.0 + bh)
                        } else {
                            (light, lmv)
                        };
                        vls[k] = vl;
                        k += 1;
                        verts.extend_from_slice(&[p[0] + (ox + x) as f32, p[1] + y as f32, p[2] + (oz + z) as f32, au, av, vl, vlm, tint]);
                    }
                    let list = if matches!(blk, 8 | 9) { &mut widxs } else { &mut idxs };
                    // Smooth faces split along the diagonal that joins the brighter pair, so a dark corner does not smear across the quad.
                    if smooth_face && vls[0] + vls[2] < vls[1] + vls[3] {
                        list.extend_from_slice(&[base + 1, base + 2, base + 3, base + 1, base + 3, base]);
                    } else {
                        list.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
                    }
                    // The grass side's tile 38 overlay (`RenderBlocks`, x grass colour) is not a second quad any more: two quads on one plane could
                    // only be told apart by the depth test, which flickered on a phone at a grazing angle. `fs_main` composites it on tile 3.
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
    (0..6).for_each(|face| push_face(verts, idxs, face, min, max, tile, light));
}

/// One face of `push_box` (index as in `FACES`: +X, -X, +Y, -Y, +Z, -Z): a painting piece shows only its front.
/// A tile from `ART_TILE0` up is a piece of `art/kz.png`, from `ITEM_TILE0` an items.png sprite.
pub fn push_face(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, face: usize, min: [f32; 3], max: [f32; 3], tile: u16, light: f32) {
    // Photon lights by levels, not by `light`: items and mobs (light 1) read as open sky, the outline (0.15, near black) as a dark cell.
    let lmv = if light < 0.2 { lm(0, 0) } else { lm(15, 0) };
    let face = &FACES[face];
    let base = verts.len() as u32 / VF as u32;
    for (c, uv) in face.corners.iter().zip(face.uv.iter()) {
        let (au, av) = if tile >= atlas::ART_TILE0 {
            atlas::art_uv((tile - atlas::ART_TILE0) as u8, uv[0], uv[1])
        } else if tile >= atlas::ITEM_TILE0 {
            atlas::item_uv((tile - atlas::ITEM_TILE0) as u8, uv[0], uv[1])
        } else {
            atlas::atlas_uv(tile, uv[0], uv[1])
        };
        verts.extend_from_slice(&[min[0] + c[0] * (max[0] - min[0]), min[1] + c[1] * (max[1] - min[1]), min[2] + c[2] * (max[2] - min[2]), au, av, light, lmv, WHITE]);
    }
    idxs.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// The boxes a block is drawn as (`setBlockBounds` + `renderStandardBlock` per box in `RenderBlocks`): the stairs' two steps
/// (`renderBlockStairs`, metadata & 3), the fence's post + bars (`renderBlockFence`; `same(dx, dz)` = a fence in that neighbour,
/// a bar runs to the cell edge there, else it is a 2/16 stub), everything else one box. Up to 5 boxes.
fn shapes(id: u8, meta: u8, same: &dyn Fn(i32, i32) -> bool) -> ([[f32; 6]; 5], usize) {
    let mut o = [[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]; 5];
    match id {
        53 | 67 => {
            o[..2].copy_from_slice(&stair_boxes(meta));
            (o, 2)
        }
        90 => {
            // `BlockPortal.setBlockBoundsBasedOnState`: a 4/16 thick plane along the axis of its neighbours.
            let along_x = same(-1, 0) || same(1, 0);
            o[0] = if along_x { [0.0, 0.0, 6.0 / 16.0, 1.0, 1.0, 10.0 / 16.0] } else { [6.0 / 16.0, 0.0, 0.0, 10.0 / 16.0, 1.0, 1.0] };
            (o, 1)
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
pub fn push_crops(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, cell: [f32; 3], meta: u8, light: f32, lmv: f32) {
    let [x, y, z] = cell;
    let y = y - 1.0 / 16.0;
    // (x0, z0, x1, z1) of each quad's base line
    for (x0, z0, x1, z1) in [(0.25, 0.0, 0.25, 1.0), (0.75, 0.0, 0.75, 1.0), (0.0, 0.25, 1.0, 0.25), (0.0, 0.75, 1.0, 0.75)] {
        let base = verts.len() as u32 / VF as u32;
        for (px, pz, py, u, v) in [(x0, z0, 1.0, 0.0, 0.0), (x0, z0, 0.0, 0.0, 1.0), (x1, z1, 0.0, 1.0, 1.0), (x1, z1, 1.0, 1.0, 0.0)] {
            let (au, av) = atlas::face_uv(59, meta, 2, u, v);
            verts.extend_from_slice(&[x + px, y + py, z + pz, au, av, light, lmv, WHITE]);
        }
        idxs.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3, base, base + 2, base + 1, base, base + 3, base + 2]);
    }
}

/// `RenderBlocks.renderBlockTorch` / `renderTorchAtAngle` for the torch in `cell` (world coordinates of its corner): a 2/16
/// wide stick of four side quads (the tile's cut-out shapes them) and a 2 x 2 texel head at 0.625; a wall torch (metadata
/// 1..=4: wall at -X, +X, -Z, +Z) sits 0.1 off its cell centre, 0.2 up, and leans 0.4 away from the wall. Full bright like
/// any block that emits light, visible from both sides (the pipeline culls back faces).
/// `tile`: 80 torch, 99 lit redstone torch, 115 one that is out.
pub fn push_torch(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, cell: [f32; 3], meta: u8, tile: u8) {
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
        let base = verts.len() as u32 / VF as u32;
        for p in q {
            let (u, v) = atlas::terrain_uv(tile, p[3], p[4]);
            verts.extend_from_slice(&[p[0], p[1], p[2], u, v, 1.0, lm(15, 15), WHITE]);
        }
        idxs.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3, base, base + 2, base + 1, base, base + 3, base + 2]);
    }
}

/// `RenderBlocks.renderBlockMinecartTrack` (rails 66, powered 27, detector 28) and `renderBlockLadder` (65): one cut-out quad, seen from\n/// both sides. A rail lies 1/16 up, ascending shapes (metadata 2..=5) lift one edge a block; curves (6..=9) use the tile above.\n/// A ladder sits 0.05 inside the wall it is on (metadata 2..=5 = wall on +Z, -Z, +X, -X).
pub fn push_flat(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, id: u8, meta: u8, [x, y, z]: [f32; 3], light: f32, lmv: f32) {
    // (x, y, z, u, v) with u, v in the tile, v up.
    let q: [[f32; 5]; 4] = if id == 65 {
        match meta {
            5 => [[0.05, 1.0, 1.0, 0.0, 1.0], [0.05, 0.0, 1.0, 0.0, 0.0], [0.05, 0.0, 0.0, 1.0, 0.0], [0.05, 1.0, 0.0, 1.0, 1.0]],
            4 => [[0.95, 0.0, 1.0, 1.0, 0.0], [0.95, 1.0, 1.0, 1.0, 1.0], [0.95, 1.0, 0.0, 0.0, 1.0], [0.95, 0.0, 0.0, 0.0, 0.0]],
            3 => [[1.0, 0.0, 0.05, 1.0, 0.0], [1.0, 1.0, 0.05, 1.0, 1.0], [0.0, 1.0, 0.05, 0.0, 1.0], [0.0, 0.0, 0.05, 0.0, 0.0]],
            2 => [[1.0, 1.0, 0.95, 0.0, 1.0], [1.0, 0.0, 0.95, 0.0, 0.0], [0.0, 0.0, 0.95, 1.0, 0.0], [0.0, 1.0, 0.95, 1.0, 1.0]],
            _ => return, // a ladder with no wall is not drawn (the Java draws nothing either)
        }
    } else {
        let m = if id == 66 { meta } else if id == 55 { 0 } else { meta & 7 }; // a powered / detector rail keeps its shape in the low 3 bits; dust is flat
        // corners 20..23 of the Java as (x, z), then the height lifts
        let (xs, zs) = match m {
            1 | 2 | 3 | 7 => ([1.0, 0.0, 0.0, 1.0], [1.0, 1.0, 0.0, 0.0]),
            8 => ([0.0, 0.0, 1.0, 1.0], [1.0, 0.0, 0.0, 1.0]),
            9 => ([0.0, 1.0, 1.0, 0.0], [0.0, 0.0, 1.0, 1.0]),
            _ => ([1.0, 1.0, 0.0, 0.0], [0.0, 1.0, 1.0, 0.0]),
        };
        let mut h = [1.0 / 16.0; 4];
        match m {
            2 | 4 => { h[0] += 1.0; h[3] += 1.0; }
            3 | 5 => { h[1] += 1.0; h[2] += 1.0; }
            _ => {}
        }
        let uv = [[1.0, 1.0], [1.0, 0.0], [0.0, 0.0], [0.0, 1.0]];
        std::array::from_fn(|i| [xs[i], h[i], zs[i], uv[i][0], uv[i][1]])
    };
    let tile = match (id, meta) {
        (65, _) => 83,
        (66, 6..) => 112,
        (66, _) => 128,
        (27, m) => if m & 8 == 0 { 163 } else { 179 },
        (55, _) => 164, // redstone dust (ponytail: always the dot / cross tile, not the line tile 165 along a straight run)
        _ => 195,
    };
    let base = verts.len() as u32 / VF as u32;
    for p in q {
        let (u, v) = atlas::terrain_uv(tile, p[3], p[4]);
        verts.extend_from_slice(&[x + p[0], y + p[1], z + p[2], u, v, light, lmv, WHITE]);
    }
    idxs.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3, base, base + 2, base + 1, base, base + 3, base + 2]);
}

/// Fire (`BlockFire`, render type 3), simplified: the two flame tiles (31, 47: animated, `render::anim`) as crossed quads, one
/// pair a little narrower than the other. Full bright (fire emits light), cut out by the tile's alpha. ponytail: vanilla also
/// draws the side flames on a wall a flammable block stands against and the top flames on a flammable roof; this is the
/// standing-fire look on every cell.
pub fn push_fire(verts: &mut Vec<f32>, idxs: &mut Vec<u32>, [x, y, z]: [f32; 3]) {
    for (tile, half) in [(31u8, 0.45f32), (47, 0.35)] {
        for ((x0, z0), (x1, z1)) in [((-half, -half), (half, half)), ((-half, half), (half, -half))] {
            let base = verts.len() as u32 / VF as u32;
            for (px, pz, py, u, v) in [(x0, z0, 0.0, 0.0, 0.0), (x1, z1, 0.0, 1.0, 0.0), (x1, z1, 1.0, 1.0, 1.0), (x0, z0, 1.0, 0.0, 1.0)] {
                let (au, av) = atlas::terrain_uv(tile, u, v);
                verts.extend_from_slice(&[x + 0.5 + px, y + py, z + 0.5 + pz, au, av, 1.0, lm(15, 15), WHITE]);
            }
            idxs.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3, base, base + 2, base + 1, base, base + 3, base + 2]);
        }
    }
}

/// Simple count of generated faces (verts.len() / (4 * VF), since 4 verts per face).
/// How many cells of leaves must lie behind a leaf-against-leaf face for the mesher to drop it: 1 (every such face) while the three leaf
/// tiles (oak, spruce, birch) are opaque, 2 (Xenon's HOLLOW) as soon as one has a see-through texel.
fn leaf_depth() -> i32 {
    if (0..3).any(|m| atlas::terrain_tile(18, m, 2).map_or(false, |t| atlas::tile_is_cutout(t as u16))) { 2 } else { 1 }
}

pub fn face_count(verts: &[f32]) -> usize {
    verts.len() / (4 * VF)
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
            build(&c, &Nibbles::new(), &a, [&a[..], &a[..], &a[..], &a[..]], [&a[..], &a[..], &a[..], &a[..]], 0, 0, 0).0[face * 4 * VF + 3]
        };
        assert_eq!(u_of(&[idx(5, 20, 5)], 4), atlas::terrain_uv(27, 0.0, 0.0).0);
        // Block 5 emits -X, +Y, -Y, +Z, -Z (+X touches the other chest): its +Z is face 3; block 6's is face 5 + 3.
        let two = [idx(5, 20, 5), idx(6, 20, 5)];
        assert_eq!((u_of(&two, 3), u_of(&two, 8)), (atlas::terrain_uv(41, 0.0, 0.0).0, atlas::terrain_uv(42, 0.0, 0.0).0));
    }

    /// The three leaf tiles of the shipped atlas have no see-through texel, so a leaf is an opaque cube and `leaf_depth` is 1. (If this
    /// fails, a new atlas has fancy leaves: the mesher falls back to Xenon's HOLLOW depth 2 by itself, and the two counts below change.)
    #[test]
    fn the_shipped_leaf_tiles_are_opaque() {
        assert_eq!(leaf_depth(), 1);
        for m in 0..3 { assert!(!atlas::tile_is_cutout(atlas::terrain_tile(18, m, 2).unwrap() as u16)); }
        assert!(atlas::tile_is_cutout(atlas::terrain_tile(31, 1, 2).unwrap() as u16)); // tall grass does have see-through texels
    }

    /// Opaque leaves hide against each other like stone (`BlockLeavesBase.shouldSideBeRendered` in fast graphics): two touching cells
    /// are one 2 x 1 box (10 quads, as for stone), a 3 x 3 x 3 crown is only its 6 x 9 outside quads, not the 6 x 27 of every cell.
    #[test]
    fn leaves_hide_their_faces_against_each_other() {
        let a = air();
        let faces = |id: u8| {
            let mut c = air();
            c[idx(5, 20, 5)] = id;
            c[idx(6, 20, 5)] = id;
            face_count(&build(&c, &Nibbles::new(), &a, [&a[..]; 4], [&a[..]; 4], 0, 0, 0).0)
        };
        assert_eq!((faces(18), faces(1)), (10, 10));
        let mut c = air();
        for x in 5..8 { for y in 20..23 { for z in 5..8 { c[idx(x, y, z)] = 18; } } }
        assert_eq!(face_count(&build(&c, &Nibbles::new(), &a, [&a[..]; 4], [&a[..]; 4], 0, 0, 0).0), 54);
    }

    /// A row of 6 leaves is a 6 x 1 box (4 x 6 sides + 2 ends = 26 quads); at the chunk's +X border the face against a leaf of the next
    /// chunk is dropped too (3 cells: 4 x 3 + 1 end = 13), since depth 1 needs only the neighbour cell, which the snapshot holds.
    #[test]
    fn a_row_of_leaves_is_a_box() {
        let a = air();
        let quads = |cells: &[usize], nbx: &[u8]| {
            let mut c = air();
            for &x in cells { c[idx(x, 20, 5)] = 18; }
            face_count(&build(&c, &Nibbles::new(), &a, [nbx, &a[..], &a[..], &a[..]], [&a[..]; 4], 0, 0, 0).0)
        };
        assert_eq!(quads(&[4, 5, 6, 7, 8, 9], &a), 26);
        let mut nbx = air();
        nbx[idx(0, 20, 5)] = 18;
        assert_eq!(quads(&[13, 14, 15], &nbx), 13);
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

    /// Console-Edition smooth light: the corner of a floor face that touches a wall is darker (ambient occlusion), the far one is not,
    /// and smooth vertices carry the packed light (`SMOOTH_LM`).
    #[test]
    fn smooth_corner_next_to_a_wall_is_darker() {
        let (a, e, mut c) = (air(), Nibbles::new(), air());
        for x in 3..8 { for z in 3..8 { c[idx(x, 19, z)] = 1; } }
        c[idx(5, 20, 5)] = 1;
        let v = build_split_with(true, &c, &e, [&e; 4], &a, [&a[..]; 4], [&a[..]; 4], 0, 0, 0).0;
        let min = |q: &[f32], o: usize| (0..4).map(|k| q[k * VF + o]).fold(f32::MAX, f32::min);
        // the top face of the floor cell (4, 19, 5): y = 20, x in 4..5, z in 5..6
        let q = v.chunks(4 * VF).find(|q| min(q, 1) == 20.0 && (0..4).all(|k| q[k * VF + 1] == 20.0) && min(q, 0) == 4.0 && min(q, 2) == 5.0).unwrap();
        let at_x = |x: f32| (0..4).filter(|&k| q[k * VF] == x).map(|k| q[k * VF + 5]).fold(f32::MAX, f32::min);
        assert!(at_x(5.0) < at_x(4.0), "wall corner {} not darker than open corner {}", at_x(5.0), at_x(4.0));
        assert!((0..4).all(|k| q[k * VF + 6] >= SMOOTH_LM));
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
        let v = build(&c, &Nibbles::new(), &a, [&a[..], &a[..], &a[..], &a[..]], [&a[..], &a[..], &a[..], &a[..]], 0, 0, 0).0;
        assert_eq!(face_count(&v), 2);
        assert!(v.chunks(VF).all(|q| q[5] == 1.0 && q[6] >= SMOOTH_LM), "a diagonal quad has no axis face: shade 1 from the vertex");
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
        assert!(w.iter().all(|&i| (5.0..=6.0).contains(&v[i as usize * VF])));
        // One water cell = depth 1: the integer part of its light; a 3-deep column reads 3 on its top face.
        assert!(w.iter().all(|&i| v[i as usize * VF + 5].floor() == 1.0));
        c[idx(5, 19, 5)] = 9;
        c[idx(5, 18, 5)] = 9;
        let (v, _, w) = build_split(&c, &Nibbles::new(), [&Nibbles::new(); 4], &a, [&a[..], &a[..], &a[..], &a[..]], [&a[..], &a[..], &a[..], &a[..]], 0, 0, 0);
        assert!(w.iter().any(|&i| v[i as usize * VF + 5].floor() == 3.0));
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

    /// Grass is one quad per face (the side's overlay is composited in the shader): its top and its 4 sides carry the column's grass
    /// colour (the shader tints only the overlay), its bottom does not; a rail is one flat quad drawn from both sides.
    #[test]
    fn grass_overlay_tint_and_rail_quad() {
        let (mut c, z, a) = (vec![0u8; VOLUME], vec![0u8; VOLUME], vec![0xFFu8; VOLUME]);
        c[idx(5, 64, 5)] = 2;
        c[idx(9, 64, 9)] = 66;
        let mut t = Tints::white();
        t.grass[5 * 16 + 5] = 0x112233 as f32;
        let e = Nibbles::new();
        let (v, i, _) = build_split_tinted(false, &c, &e, [&e; 4], &a, [&z[..]; 4], [&a[..]; 4], 0, 0, 0, &t);
        assert_eq!(v.len() / (4 * VF), 6 + 1);
        assert_eq!(i.len(), 6 * 6 + 12);
        assert_eq!(v.chunks(VF).filter(|q| q[7] == 0x112233 as f32).count(), 4 + 16); // top face + 4 sides, not the bottom
    }
}
