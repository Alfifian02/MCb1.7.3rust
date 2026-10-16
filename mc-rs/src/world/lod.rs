//! Far terrain, Voxy style: the view reaches `RADIUS` blocks with the SAME blocks, textures, tints and face shades as the real
//! chunks, not a smooth painted heightfield.
//!
//! Voxy keeps the real block data of far sections, mips it 2x2x2 at a time (`Mipper.mip`: of the eight children take the
//! top-most, most opaque one) and meshes every level with the block's own model. Here the world is 128 high, so a "section" is a
//! column grid: a worker thread makes the real columns of a 64 x 64 tile (`OverworldGenerator::generate`, plus `populate` for
//! the trees when the tile is near; the same generator, so the same terrain and the same biome tints as the chunk ring), keeps
//! them (`Store`), and meshes a tile at a step of 1, 2, 4, 8 or 16 blocks by distance. One cell = one column of `step` x `step`
//! blocks: its top block's tile on the top face (leaves for a tree top, snow, ice, sand ...), a side wall to every lower neighbour
//! built from the block's side tile (grass side overlay, dirt, stone below), water as one sheet coloured like the translucent water
//! over its floor. At step 1 it is the block terrain itself, so where the real chunks stop the LoD goes on with the same pixels.
//!
//! Blending with the real chunks (what was wrong before: a smooth flat-coloured heightfield, a 56 block hole inside a 64 block ring,
//! different shading on both sides):
//! - same atlas tiles, same grass / foliage tint, same face shade (top 1.0, Z sides 0.8, X sides 0.6), same light (sky 15), same fog;
//! - the LoD is cut out exactly where real sections are drawn (`world::chunks::in_range`, the test `update_visible` uses, 8 blocks
//!   inside it so the eye's 2 block quantisation and a chunk still being meshed never open a gap), and the part that stays under
//!   the real chunks is `EPS` lower (walls `EPS` inside their cell) so the real surface always wins the depth test: no z-fighting.
//!
//! Real chunks first (Voxy's `VoxelIngestService`): the worker's own generator only guesses what no chunk has shown yet. Every chunk
//! the chunk manager has finished (all four populates that write into it done, `ChunkManager::take_ingest`) and every one it unloads
//! after an edit is turned into columns (`columns`) and handed to `Lod::ingest`; they replace the guessed columns of their tile, now
//! and in every tile made later (`Store::real`), and the tile is meshed again a moment after the last chunk of a burst
//! (`REMESH_WAIT`, Voxy's `ChunkUpdateTracker` throttle). So the band between the drawn ring and the loaded chunks, the player's
//! edits and the exact trees are what the LoD shows, not a second opinion of the generator.
//!
//! A tree is a box of leaves over its own ground (`Col::leaf`), not a tower from the top leaf down to the lawn: the old top-block-only
//! column made every tree a pillar with a trunk-coloured wall. A forest cell (a quarter of it under leaves) at a coarse step keeps the
//! tree mass at the mean crown height, a lone tree on a lawn is dropped (`resolve`).
//!
//! ponytail: no persistence (the guessed tiles are generated again each run, nearest first; real chunks are ingested again as they are
//! visited); a cell is one ground block over its whole footprint plus one leaf box, no caves / overhangs / interiors (a wall shows the
//! blocks under the top one by a depth rule, `wall_block`); a trunk is drawn only at step 1; trees beyond `TREES` blocks of where the
//! tile was asked for are not grown (a forest there is a lawn); edits far from the player are only seen once their chunk has been
//! loaded; one worker thread. UNVERIFIED: written without a toolchain (not compiled, not run), nothing measured on a device; the
//! distances (`step_for`, `TREES`) and the slack values were picked on paper.

use crate::gpu::pipeline::{create_index_buffer, create_vertex_buffer, Vertex};
use crate::render::atlas;
use crate::render::mesh::{lm, tint_of, Tints, WHITE};
use crate::world::chunk::{brightness, idx, is_plant, Nibbles};
use crate::world::chunks::{biome_tints, in_range, Shape};
use crate::world::gen::chunk_manager::WorldChunkManager;
use crate::world::gen::overworld::{OverworldGenerator, SEA_LEVEL};
use crate::world::gen::populate::Region;
use glam::Vec3;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

/// How far the far terrain reaches, and the distance the fog completes at (it starts at a quarter of it).
pub const RADIUS: f32 = 768.0;
/// The camera's far plane while the LoD is on: past the last tile's far corner.
pub const ZFAR: f32 = 1100.0;
/// Tile edge in blocks (4 chunks).
const TILE: i32 = 64;
/// Tiles nearer than this get their trees (`populate`), the rest are bare ground.
const TREES: f32 = 520.0;
/// The LoD under the real chunks sits this much lower (and its walls this much inside), so the real surface always wins.
const EPS: f32 = 0.04;
/// How far inside the real chunks' range a land cell is cut out (the eye is quantised to 2 blocks: 1.74 at most; the rest is slack
/// for a chunk that is in range but not meshed yet). Water is a translucent sheet in the real pass, so it is cut closer: a LoD sheet
/// under it would show through as a second layer of water.
const LAND_SLACK: f32 = 8.0;
const WATER_SLACK: f32 = 2.5;
/// Tiles asked of the worker at once.
const MAX_PENDING: usize = 2;
/// The worker keeps this many tiles of columns (~60 KB each), the farthest are dropped past it.
const KEEP_TILES: usize = 700;
/// A tile that got real chunks is meshed again this long after the first of them (a burst of chunks costs one rebuild).
const REMESH_WAIT: Duration = Duration::from_secs(2);

/// Cell width (blocks) of a tile whose centre is `dist` blocks from the eye.
fn step_for(dist: f32) -> i32 {
    if dist < 120.0 {
        1
    } else if dist < 200.0 {
        2
    } else if dist < 330.0 {
        4
    } else if dist < 520.0 {
        8
    } else {
        16
    }
}

/// One block column as the LoD keeps it: the ground you see and the tree over it, if any. `id` 0 = nothing (never in a real column:
/// bedrock is at the bottom).
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Col {
    /// The top block that is not air, a plant, water or a tree (ice, snow layer and cactus count).
    id: u8,
    top: u8,
    meta: u8,
    /// y of the highest water block above `top`, 0 = no water over it.
    wtop: u8,
    /// `mesh::tint_of` of that block in this column (grass / foliage colour, white otherwise).
    tint: [u8; 3],
    /// A tree over the ground: y of the top face of its highest leaf (0 = no tree), y of its lowest leaf block, the leaves' metadata
    /// (species) and tint, and whether a trunk stands under them.
    leaf: u8,
    lb: u8,
    lmeta: u8,
    ltint: [u8; 3],
    log: bool,
}

fn rgb(t: u32) -> [u8; 3] {
    [(t >> 16) as u8, (t >> 8) as u8, t as u8]
}

/// y of the top face (a snow layer is 1/8 high).
fn land_h(c: Col) -> f32 {
    c.top as f32 + if c.id == 78 { 0.125 } else { 1.0 }
}

/// y of the surface you see: the water's (`BlockFluid.getPercentAir` of a source: 1/9 under the cell's top) or the land's.
fn surf_h(c: Col) -> f32 {
    if c.wtop > 0 {
        c.wtop as f32 + 1.0 - 1.0 / 9.0
    } else {
        land_h(c)
    }
}

/// The column (x, z) of a chunk's blocks (`(x << 11) | (z << 7) | y`). Leaves seen first make a tree: they are noted and the
/// trunk and air under them are read through to the ground; a log seen first is an ordinary block (a cabin wall, a stump).
fn column(blocks: &[u8], meta: &Nibbles, tints: &Tints, x: usize, z: usize) -> Col {
    let base = idx(x, 0, z);
    let mut c = Col::default();
    for y in (0..128usize).rev() {
        let b = blocks[base + y];
        if b == 0 || is_plant(b) {
            continue;
        }
        if b == 8 || b == 9 {
            if c.wtop == 0 {
                c.wtop = y as u8;
            }
            continue;
        }
        let m = meta.get(x, y, z);
        let t = rgb(tint_of(tints, b, m, x * 16 + z) as u32);
        if b == 18 {
            if c.leaf == 0 {
                (c.leaf, c.lmeta, c.ltint) = (y as u8 + 1, m, t);
            }
            c.lb = y as u8;
            continue;
        }
        if b == 17 && c.leaf > 0 {
            c.log = true;
            continue;
        }
        (c.id, c.top, c.meta, c.tint) = (b, y as u8, m, t);
        return c;
    }
    Col::default()
}

/// The 256 columns of a real chunk (index `x * 16 + z`), for `Lod::ingest`.
pub fn columns(blocks: &[u8], meta: &Nibbles, tints: &Tints) -> Vec<Col> {
    (0..256).map(|i| column(blocks, meta, tints, i >> 4, i & 15)).collect()
}

/// The cell of `s` x `s` columns at (x0, z0) of a tile (`Mipper.mip`): of the children take the one whose ground is the highest, so
/// a peak and a cliff edge stay in the far view. A shore cell that is mostly water and whose land is low stays water (otherwise one
/// beach block would make a 16 block wide island of every bay). The trees of the cell become one box of leaves at the mean height of
/// the crowns when a quarter of the cell is under leaves (a forest keeps its mass), else none (a lone tree is a lawn at this scale).
fn resolve(t: &Tile0, x0: usize, z0: usize, s: usize) -> Col {
    if s == 1 {
        return t.cols[x0 * 64 + z0];
    }
    let (mut best, mut best_h) = (Col::default(), -1.0f32);
    let (mut wet, mut water) = (Col::default(), 0usize);
    let (mut crown, mut leaves, mut lsum, mut bsum) = (Col::default(), 0u32, 0u32, 0u32);
    for x in x0..x0 + s {
        for z in z0..z0 + s {
            let c = t.cols[x * 64 + z];
            if c.id == 0 {
                continue;
            }
            if c.leaf > 0 {
                (leaves, lsum, bsum) = (leaves + 1, lsum + c.leaf as u32, bsum + c.lb as u32);
                if c.leaf > crown.leaf {
                    crown = c;
                }
            }
            if c.wtop > 0 {
                water += 1;
                if c.wtop > wet.wtop {
                    wet = c;
                }
            }
            let h = surf_h(c);
            if h > best_h {
                best_h = h;
                best = c;
            }
        }
    }
    let ground = |c: Col| Col { leaf: 0, lb: 0, lmeta: 0, ltint: [0; 3], log: false, ..c };
    if best.wtop == 0 && water * 2 > s * s && land_h(best) < SEA_LEVEL as f32 + 4.0 {
        return ground(wet);
    }
    let mut r = ground(best);
    if r.wtop == 0 && leaves * 4 >= (s * s) as u32 {
        let (lt, lb) = ((lsum / leaves) as u8, ((bsum / leaves) as u8).max(r.top + 1));
        if lt > lb {
            (r.leaf, r.lb, r.lmeta, r.ltint) = (lt, lb, crown.lmeta, crown.ltint);
        }
    }
    r
}

/// sRGB bytes of the water over its floor, seen from above: the floor (`block_color`) lit by what is left of the sky light under
/// `depth` blocks of water (3 a block, `Block.lightOpacity`), mixed with the water tile's average colour (47, 67, 244) at its alpha
/// (138 / 255), in linear light. The real water is a translucent sheet over the real floor in the same way.
fn water_rgb(c: Col) -> [u8; 3] {
    let depth = (c.wtop as i32 - c.top as i32).max(1);
    let br = brightness((15 - 3 * depth).max(0) as u8);
    let floor = atlas::block_color(c.id);
    let water = [47.0f32, 67.0, 244.0];
    let mix = |f: u8, w: f32| {
        let l = (f as f32 / 255.0).powf(2.2) * br;
        let o = l + ((w / 255.0).powf(2.2) - l) * (138.0 / 255.0);
        (o.powf(1.0 / 2.2) * 255.0).round().clamp(0.0, 255.0) as u8
    };
    [mix(floor[0], water[0]), mix(floor[1], water[1]), mix(floor[2], water[2])]
}

/// The block whose side tile a wall segment `depth` blocks under the cell's top shows: the top block itself first, then what
/// b1.7.3's surface pass puts under it (dirt under grass, sandstone under sand), stone below.
fn wall_block(c: Col, depth: f32) -> (u8, u8) {
    if depth < 0.5 {
        return (c.id, c.meta);
    }
    match c.id {
        2 | 3 if depth < 4.0 => (3, 0),
        12 => (if depth < 4.0 { 12 } else { 24 }, 0),
        13 => (13, 0),
        _ => (1, 0),
    }
}

/// Atlas uv of a block's face tile at (u, v) (v up), the flat colour of the block where it has no tile.
fn face_uv(id: u8, meta: u8, side: u8, u: f32, v: f32) -> [f32; 2] {
    let (a, b) = match atlas::terrain_tile(id, meta, side) {
        Some(t) => atlas::terrain_uv(t, u, v),
        None => atlas::atlas_uv(id as u16, 0.0, 0.0),
    };
    [a, b]
}

fn pack(rgb: [u8; 3]) -> f32 {
    ((rgb[0] as u32) << 16 | (rgb[1] as u32) << 8 | rgb[2] as u32) as f32
}

/// The 64 x 64 columns of one tile (index `x * 64 + z`), and whether its trees were grown.
struct Tile0 {
    cols: Vec<Col>,
    trees: bool,
}

/// What the worker owns: its own generator (the same world as the chunk ring's) and the columns it has made.
struct Store {
    g: OverworldGenerator,
    cm: WorldChunkManager,
    tiles: HashMap<(i32, i32), Tile0>,
    /// The columns of the real chunks `Lod::ingest` handed over, by chunk: laid over a tile whenever it is made (so they outlive it).
    real: HashMap<(i32, i32), Vec<Col>>,
}

/// Lay the columns of real chunk `k` over its place in tile `t`.
fn put(t: &mut Tile0, k: (i32, i32), cols: &[Col]) {
    let (ox, oz) = (k.0.rem_euclid(4) as usize * 16, k.1.rem_euclid(4) as usize * 16);
    for x in 0..16 {
        t.cols[(ox + x) * 64 + oz..(ox + x) * 64 + oz + 16].copy_from_slice(&cols[x * 16..x * 16 + 16]);
    }
}

impl Store {
    /// A real chunk replaces what the generator guessed for it: in its tile if that is made, and in the tile when it is made.
    fn ingest(&mut self, k: (i32, i32), cols: Vec<Col>) {
        if let Some(t) = self.tiles.get_mut(&(k.0.div_euclid(4), k.1.div_euclid(4))) {
            put(t, k, &cols);
        }
        self.real.insert(k, cols);
    }

    /// The columns of tile `k`, made now if missing (or if `trees` are wanted and it has none).
    fn tile(&mut self, k: (i32, i32), trees: bool) -> &Tile0 {
        if self.tiles.get(&k).map_or(true, |t| trees && !t.trees) {
            let t = self.make(k, trees);
            self.tiles.insert(k, t);
        }
        &self.tiles[&k]
    }

    /// Generate the tile's 4 x 4 chunks. With trees the grid starts one chunk to the -x / -z side (a chunk's populate reaches into
    /// +x / +z, so the tile's first row and column get their trees from there) and every chunk but the last row is populated
    /// on a 2 x 2 block, in raster order, as the chunk manager does it.
    fn make(&mut self, k: (i32, i32), trees: bool) -> Tile0 {
        let m = trees as usize;
        let gn = 4 + 2 * m;
        let (lx, lz) = (k.0 * 4 - m as i32, k.1 * 4 - m as i32);
        let mut b: Vec<Vec<u8>> = Vec::with_capacity(gn * gn);
        for i in 0..gn {
            for j in 0..gn {
                b.push(self.g.generate(lx + i as i32, lz + j as i32, &mut self.cm));
            }
        }
        let mut d: Vec<Nibbles> = (0..gn * gn).map(|_| Nibbles::new()).collect();
        if trees {
            for i in 0..gn - 1 {
                for j in 0..gn - 1 {
                    let at = [i * gn + j, i * gn + j + 1, (i + 1) * gn + j, (i + 1) * gn + j + 1];
                    let (x, z) = (lx + i as i32, lz + j as i32);
                    let bs: [Vec<u8>; 4] = at.map(|q| std::mem::take(&mut b[q]));
                    let ds: [Nibbles; 4] = at.map(|q| std::mem::take(&mut d[q]));
                    let mut r = Region::new(x, z, bs, ds);
                    self.g.populate(&mut r, x, z, &mut self.cm);
                    let (bs, ds) = r.into_parts();
                    for (n, (bq, dq)) in bs.into_iter().zip(ds).enumerate() {
                        b[at[n]] = bq;
                        d[at[n]] = dq;
                    }
                }
            }
        }
        let mut cols = vec![Col::default(); 64 * 64];
        for ci in 0..4usize {
            for cj in 0..4usize {
                let q = (ci + m) * gn + cj + m;
                let tints = biome_tints(&mut self.cm, k.0 * 4 + ci as i32, k.1 * 4 + cj as i32);
                for x in 0..16 {
                    for z in 0..16 {
                        cols[(ci * 16 + x) * 64 + cj * 16 + z] = column(&b[q], &d[q], &tints, x, z);
                    }
                }
            }
        }
        let mut t = Tile0 { cols, trees };
        for ci in 0..4 {
            for cj in 0..4 {
                let c = (k.0 * 4 + ci, k.1 * 4 + cj);
                if let Some(r) = self.real.get(&c) {
                    put(&mut t, c, r);
                }
            }
        }
        t
    }

    /// Drop the tiles farthest from `k` once there are more than `KEEP_TILES`.
    fn prune(&mut self, k: (i32, i32)) {
        // Real chunks stay while they could still be in a tile of the view (48 chunks = `RADIUS`).
        self.real.retain(|c, _| (c.0 - k.0 * 4).abs() <= 48 && (c.1 - k.1 * 4).abs() <= 48);
        if self.tiles.len() <= KEEP_TILES {
            return;
        }
        let d = |q: &(i32, i32)| (q.0 - k.0).pow(2) + (q.1 - k.1).pow(2);
        let mut keys: Vec<_> = self.tiles.keys().copied().collect();
        keys.sort_by_key(|q| std::cmp::Reverse(d(q)));
        for q in keys.into_iter().take(self.tiles.len() - (KEEP_TILES - 100)) {
            self.tiles.remove(&q);
        }
    }
}

/// The cell of step `s` at global cell (gi, gj), from whatever tile holds it (made bare if missing).
fn cell_at(store: &mut Store, gi: i32, gj: i32, s: usize) -> Col {
    let (x, z) = (gi * s as i32, gj * s as i32);
    let t = store.tile((x.div_euclid(TILE), z.div_euclid(TILE)), false);
    resolve(t, x.rem_euclid(TILE) as usize, z.rem_euclid(TILE) as usize, s)
}

/// One cell of a tile: where it is (its top block's section decides if the real chunks cover it) and its slice of the indices.
struct Cell {
    x: i32,
    y: i32,
    z: i32,
    water: bool,
    i0: u32,
    i1: u32,
}

/// What the worker hands back: a tile's vertices, indices (cell after cell) and cells.
struct Built {
    key: (i32, i32),
    step: i32,
    trees: bool,
    verts: Vec<Vertex>,
    idx: Vec<u32>,
    cells: Vec<Cell>,
}

struct Quads {
    verts: Vec<Vertex>,
    idx: Vec<u32>,
}

impl Quads {
    /// One quad, corners counter-clockwise seen from the front (the pipeline culls back faces).
    fn quad(&mut self, p: [[f32; 3]; 4], uv: [[f32; 2]; 4], light: f32, tint: f32) {
        let base = self.verts.len() as u32;
        for k in 0..4 {
            self.verts.push(Vertex { pos: p[k], uv: uv[k], light, lm: lm(15, 0), tint });
        }
        self.idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// The four sides, in the order of `render::mesh::FACES`' +X, -X, +Z, -Z: the neighbour cell, the side index of `terrain_tile`, the face
/// shade (X 0.6, Z 0.8), the face's unit corners (bottom-left, bottom-right, top-right, top-left seen from outside).
type Side = ((i32, i32), u8, f32, [[f32; 3]; 4]);
const SIDES: [Side; 4] = [
    ((1, 0), 5, 0.6, [[1.0, 0.0, 1.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 1.0, 1.0]]),
    ((-1, 0), 4, 0.6, [[0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 1.0], [0.0, 1.0, 0.0]]),
    ((0, 1), 3, 0.8, [[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]]),
    ((0, -1), 2, 0.8, [[1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]]),
];

/// A wall of the cell at (x0, z0) facing `side`, from `yt` down to `yb`, in segments one cell tall (a tile is mapped once per `step`
/// blocks, so a taller quad would run off its tile in the atlas); `block(depth)` = block, metadata and tint of the segment `depth`
/// under `yt`. The wall sits `EPS` inside its cell.
#[allow(clippy::too_many_arguments)]
fn wall(q: &mut Quads, side: Side, x0: f32, z0: f32, sf: f32, yt: f32, yb: f32, block: impl Fn(f32) -> (u8, u8, f32)) {
    let ((di, dj), tile, shade, corners) = side;
    let mut k = 0;
    loop {
        let (st, sb) = (yt - k as f32 * sf, (yt - (k + 1) as f32 * sf).max(yb));
        if st <= yb + 0.001 || k > 140 {
            break;
        }
        let (id, meta, tint) = block(k as f32 * sf);
        let v0 = 1.0 - (st - sb) / sf;
        let p = corners.map(|cn| [x0 + cn[0] * sf - EPS * di as f32, if cn[1] > 0.5 { st } else { sb }, z0 + cn[2] * sf - EPS * dj as f32]);
        let uv = [face_uv(id, meta, tile, 0.0, v0), face_uv(id, meta, tile, 1.0, v0), face_uv(id, meta, tile, 1.0, 1.0), face_uv(id, meta, tile, 0.0, 1.0)];
        q.quad(p, uv, shade, tint);
        k += 1;
    }
}

/// The mesh of tile `key` with a cell every `step` blocks.
fn build(store: &mut Store, key: (i32, i32), step: i32, trees: bool) -> Built {
    let s = step as usize;
    let n = TILE as usize / s;
    let trees = store.tile(key, trees).trees;
    let grid: Vec<Col> = {
        let t = &store.tiles[&key];
        (0..n * n).map(|q| resolve(t, q / n * s, q % n * s, s)).collect()
    };
    // The cells just outside the tile, for the walls that face it.
    let (gi0, gj0) = (key.0 * n as i32, key.1 * n as i32);
    let mut ring: [Vec<Col>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()]; // +X, -X, +Z, -Z
    for q in 0..n as i32 {
        ring[0].push(cell_at(store, gi0 + n as i32, gj0 + q, s));
        ring[1].push(cell_at(store, gi0 - 1, gj0 + q, s));
        ring[2].push(cell_at(store, gi0 + q, gj0 + n as i32, s));
        ring[3].push(cell_at(store, gi0 + q, gj0 - 1, s));
    }
    let nb = |i: i32, j: i32| -> (Col, bool) {
        let n = n as i32;
        if i >= n {
            (ring[0][j as usize], true)
        } else if i < 0 {
            (ring[1][j as usize], true)
        } else if j >= n {
            (ring[2][i as usize], true)
        } else if j < 0 {
            (ring[3][i as usize], true)
        } else {
            (grid[(i * n + j) as usize], false)
        }
    };
    let sf = s as f32;
    // A wall on a tile's edge hangs deeper: the neighbouring tile may be coarser, and its surface then differs from the one this
    // side measured (the old LoD's skirt, as part of the wall).
    let skirt = (2 * step).clamp(2, 32) as f32;
    let mut q = Quads { verts: Vec::new(), idx: Vec::new() };
    let mut cells = Vec::with_capacity(n * n);
    for i in 0..n {
        for j in 0..n {
            let c = grid[i * n + j];
            if c.id == 0 {
                continue;
            }
            let (x0, z0) = ((key.0 * TILE) as f32 + (i * s) as f32, (key.1 * TILE) as f32 + (j * s) as f32);
            let (x1, z1) = (x0 + sf, z0 + sf);
            let i0 = q.idx.len() as u32;
            let water = c.wtop > 0;
            if water {
                let y = surf_h(c) - EPS;
                let uv = atlas::atlas_uv(80, 0.0, 0.0); // the snow block's flat texel: white, so the tint is the colour
                q.quad([[x0, y, z1], [x1, y, z1], [x1, y, z0], [x0, y, z0]], [[uv.0, uv.1]; 4], 1.0, pack(water_rgb(c)));
            } else {
                let h = land_h(c);
                let y = h - EPS;
                let uv = |u: f32, v: f32| face_uv(c.id, c.meta, 1, u, v);
                q.quad([[x0, y, z1], [x1, y, z1], [x1, y, z0], [x0, y, z0]], [uv(0.0, 0.0), uv(1.0, 0.0), uv(1.0, 1.0), uv(0.0, 1.0)], 1.0, pack(c.tint));
                for side in SIDES {
                    let ((di, dj), ..) = side;
                    let (o, edge) = nb(i as i32 + di, j as i32 + dj);
                    let low = if o.id == 0 { h } else { surf_h(o) };
                    let bottom = if edge { low.min(h) - skirt } else { low };
                    if bottom >= h - 0.01 {
                        continue;
                    }
                    // Down the wall: the top block's side first, what is under it below.
                    wall(&mut q, side, x0, z0, sf, h - EPS, bottom - EPS - 0.3, |d| {
                        let (id, meta) = wall_block(c, d);
                        (id, meta, if id == c.id { pack(c.tint) } else { WHITE })
                    });
                }
                if c.leaf > c.lb {
                    // The tree over this ground: a box of leaves from its lowest leaf to its top face (the part of a side the neighbouring
                    // crown already hides is left out), on a trunk at step 1.
                    let (lt, lb) = (c.leaf as f32, c.lb as f32);
                    let lf = |u: f32, v: f32| face_uv(18, c.lmeta, 1, u, v);
                    let (y, yb) = (lt - EPS, lb + EPS);
                    q.quad([[x0, y, z1], [x1, y, z1], [x1, y, z0], [x0, y, z0]], [lf(0.0, 0.0), lf(1.0, 0.0), lf(1.0, 1.0), lf(0.0, 1.0)], 1.0, pack(c.ltint));
                    q.quad([[x0, yb, z0], [x1, yb, z0], [x1, yb, z1], [x0, yb, z1]], [lf(0.0, 0.0), lf(1.0, 0.0), lf(1.0, 1.0), lf(0.0, 1.0)], 0.5, pack(c.ltint));
                    for side in SIDES {
                        let ((di, dj), ..) = side;
                        let (o, _) = nb(i as i32 + di, j as i32 + dj);
                        let (ob, ot) = if o.leaf > o.lb { (o.lb as f32, o.leaf as f32) } else { (lt, lt) };
                        let leaf = |_: f32| (18, c.lmeta, pack(c.ltint));
                        wall(&mut q, side, x0, z0, sf, lt.min(ob) - EPS, yb, leaf);
                        wall(&mut q, side, x0, z0, sf, y, lb.max(ot) + EPS, leaf);
                        if step == 1 && c.log {
                            wall(&mut q, side, x0, z0, sf, lb, h - EPS, |_| (17, c.lmeta & 3, WHITE));
                        }
                    }
                }
            }
            cells.push(Cell { x: x0 as i32, y: c.top.max(c.wtop).max(c.leaf) as i32, z: z0 as i32, water, i0, i1: q.idx.len() as u32 });
        }
    }
    Built { key, step, trees, verts: q.verts, idx: q.idx, cells }
}

/// Which cells the real chunks cover around `eye` = (position, range, vertical range): the same `in_range` test as
/// `ChunkManager::update_visible`, on the section of the cell's top block, made smaller by the slack.
fn covered(cells: &[Cell], eye: Option<(Vec3, f32, f32)>) -> Vec<bool> {
    let Some((e, rh, rv)) = eye else { return vec![false; cells.len()] };
    cells
        .iter()
        .map(|c| {
            let slack = if c.water { WATER_SLACK } else { LAND_SLACK };
            let min = Vec3::new((c.x >> 4) as f32 * 16.0, (c.y >> 4) as f32 * 16.0 - 1.0, (c.z >> 4) as f32 * 16.0);
            in_range(Shape::Sphere, e, min, min + Vec3::new(16.0, 18.0, 16.0), rh - slack, rv - 2.0)
        })
        .collect()
}

/// The indices of the cells the real chunks do not cover.
fn gather(cells: &[Cell], all: &[u32], cov: &[bool]) -> Vec<u32> {
    let mut v = Vec::with_capacity(all.len());
    for (c, &hide) in cells.iter().zip(cov) {
        if !hide {
            v.extend_from_slice(&all[c.i0 as usize..c.i1 as usize]);
        }
    }
    v
}

/// The eye on a grid of 2 blocks: the cut is redone only when this changes, with the eye at the middle of its grid cell.
fn quant(e: Vec3) -> (i32, i32, i32) {
    ((e.x / 2.0).floor() as i32, (e.y / 2.0).floor() as i32, (e.z / 2.0).floor() as i32)
}

fn quant_eye(q: (i32, i32, i32)) -> Vec3 {
    Vec3::new((q.0 * 2 + 1) as f32, (q.1 * 2 + 1) as f32, (q.2 * 2 + 1) as f32)
}

struct Tile {
    step: i32,
    trees: bool,
    vbuf: wgpu::Buffer,
    ibuf: Option<wgpu::Buffer>,
    index_count: u32,
    cells: Vec<Cell>,
    all: Vec<u32>,
    /// `quant(eye)` the cut was last worked out for, None = no cut.
    hole: Option<(i32, i32, i32)>,
    /// The cells the index buffer leaves out (`covered`): it is made again only when this changes, not at every step of the eye.
    cov: Vec<bool>,
    /// Not cut yet.
    stale: bool,
}

impl Tile {
    fn cut(&mut self, device: &wgpu::Device, eye: Option<((i32, i32, i32), f32, f32)>) {
        let cov = covered(&self.cells, eye.map(|(q, rh, rv)| (quant_eye(q), rh, rv)));
        if self.stale || cov != self.cov {
            let idx = gather(&self.cells, &self.all, &cov);
            self.index_count = idx.len() as u32;
            self.ibuf = (!idx.is_empty()).then(|| create_index_buffer(device, &idx));
            self.cov = cov;
        }
        self.hole = eye.map(|e| e.0);
        self.stale = false;
    }
}

/// What the worker is told: mesh a tile (x, z, step, trees), or take a real chunk's columns.
enum Msg {
    Build(i32, i32, i32, bool),
    Ingest((i32, i32), Vec<Col>),
}

pub struct Lod {
    tiles: HashMap<(i32, i32), Tile>,
    pending: HashSet<(i32, i32)>,
    /// Tiles that got real chunks since they were meshed: since when (`REMESH_WAIT`).
    dirty: HashMap<(i32, i32), Instant>,
    job: Sender<Msg>,
    rx: Receiver<Built>,
    /// The tiles `update` found in the frustum.
    visible: Vec<(i32, i32)>,
}

impl Lod {
    /// A LoD for the world of `seed` (Tectonic or vanilla terrain, like the chunk ring), generated on its own thread.
    pub fn new(seed: i64, tectonic: bool) -> Self {
        let (job, jobs) = channel::<Msg>();
        let (done, rx) = channel::<Built>();
        std::thread::Builder::new()
            .name("lod".into())
            .spawn(move || {
                let mut g = OverworldGenerator::new(seed);
                if tectonic {
                    g = g.tectonic();
                }
                let mut store = Store { g, cm: WorldChunkManager::new(seed), tiles: HashMap::new(), real: HashMap::new() };
                // Ends when the `Lod` (and so `job`) is dropped.
                while let Ok(m) = jobs.recv() {
                    match m {
                        Msg::Ingest(k, cols) => store.ingest(k, cols),
                        Msg::Build(tx, tz, step, trees) => {
                            let b = build(&mut store, (tx, tz), step, trees);
                            store.prune((tx, tz));
                            if done.send(b).is_err() {
                                break;
                            }
                        }
                    }
                }
            })
            .expect("lod thread");
        Lod { tiles: HashMap::new(), pending: HashSet::new(), dirty: HashMap::new(), job, rx, visible: Vec::new() }
    }

    /// The columns of a real chunk (`columns`), from the chunk manager: they replace the guessed ones. Its tile, and the neighbour tiles
    /// whose edge walls read it, are meshed again `REMESH_WAIT` after the first chunk of a burst.
    pub fn ingest(&mut self, k: (i32, i32), cols: Vec<Col>) {
        let _ = self.job.send(Msg::Ingest(k, cols));
        let (t, e) = ((k.0.div_euclid(4), k.1.div_euclid(4)), (k.0.rem_euclid(4), k.1.rem_euclid(4)));
        for dx in -1..=1 {
            for dz in -1..=1 {
                if (dx == -1 && e.0 != 0) || (dx == 1 && e.0 != 3) || (dz == -1 && e.1 != 0) || (dz == 1 && e.1 != 3) {
                    continue; // that neighbour does not touch this chunk
                }
                let n = (t.0 + dx, t.1 + dz);
                if self.tiles.contains_key(&n) || self.pending.contains(&n) {
                    self.dirty.entry(n).or_insert_with(Instant::now);
                }
            }
        }
    }

    /// Once per frame: take the finished tiles, ask for the missing or wrongly detailed ones nearest the eye first, drop the far ones,
    /// recut where the real chunks (`rh` blocks wide, `rv` tall around the eye) are drawn, and list the tiles `in_frustum` (a test
    /// of a world-space box) lets through.
    pub fn update(&mut self, device: &wgpu::Device, eye: Vec3, rh: f32, rv: f32, in_frustum: impl Fn(Vec3, Vec3) -> bool) {
        while let Ok(b) = self.rx.try_recv() {
            self.pending.remove(&b.key);
            let vbuf = create_vertex_buffer(device, &b.verts);
            let t = Tile { step: b.step, trees: b.trees, vbuf, ibuf: None, index_count: 0, cells: b.cells, all: b.idx, hole: None, cov: Vec::new(), stale: true };
            self.tiles.insert(b.key, t);
        }
        let ts = TILE as f32;
        let r = RADIUS + ts * 0.75;
        let centre = |k: (i32, i32)| ((k.0 as f32 + 0.5) * ts, (k.1 as f32 + 0.5) * ts);
        let dist = |k: (i32, i32)| {
            let (cx, cz) = centre(k);
            ((cx - eye.x).powi(2) + (cz - eye.z).powi(2)).sqrt()
        };
        let mut want: Vec<(f32, (i32, i32), i32, bool)> = Vec::new();
        let (x0, x1) = (((eye.x - r) / ts).floor() as i32, ((eye.x + r) / ts).floor() as i32);
        let (z0, z1) = (((eye.z - r) / ts).floor() as i32, ((eye.z + r) / ts).floor() as i32);
        for tx in x0..=x1 {
            for tz in z0..=z1 {
                let k = (tx, tz);
                let d = dist(k);
                if d > r || self.pending.contains(&k) {
                    continue;
                }
                let (step, trees) = (step_for(d), d < TREES);
                // Finer when it should be; coarser (to give the memory back) only when it is 4x too fine, so a tile at a boundary
                // does not flip.
                let fresh = self.dirty.get(&k).is_some_and(|t| t.elapsed() >= REMESH_WAIT);
                if self.tiles.get(&k).map_or(true, |t| fresh || t.step > step || t.step * 2 < step || (trees && !t.trees)) {
                    want.push((d, k, step, trees));
                }
            }
        }
        want.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, k, step, trees) in want {
            if self.pending.len() >= MAX_PENDING {
                break;
            }
            if self.job.send(Msg::Build(k.0, k.1, step, trees)).is_ok() {
                self.pending.insert(k);
                self.dirty.remove(&k); // this build is made after every chunk ingested so far
            }
        }
        self.tiles.retain(|&k, _| dist(k) <= r + ts);
        let (tiles, pending) = (&self.tiles, &self.pending);
        self.dirty.retain(|k, _| tiles.contains_key(k) || pending.contains(k));
        // The cut follows the eye; a tile too far to meet the real chunks has none.
        let q = quant(eye);
        for (&k, t) in self.tiles.iter_mut() {
            let near = dist(k) < rh + ts;
            let want = near.then_some(q);
            if t.stale || t.hole != want {
                t.cut(device, want.map(|q| (q, rh, rv)));
            }
        }
        self.visible.clear();
        for &k in self.tiles.keys() {
            let (min, max) = (Vec3::new(k.0 as f32 * ts, 0.0, k.1 as f32 * ts), Vec3::new((k.0 + 1) as f32 * ts, 128.0, (k.1 + 1) as f32 * ts));
            if in_frustum(min, max) {
                self.visible.push(k);
            }
        }
    }

    /// Into the opaque pass, after the chunks (`ChunkPipeline`'s pipeline and bind groups are already set).
    pub fn draw(&self, rp: &mut wgpu::RenderPass<'_>) {
        for k in &self.visible {
            let Some(t) = self.tiles.get(k) else { continue };
            let Some(ib) = t.ibuf.as_ref() else { continue };
            rp.set_vertex_buffer(0, t.vbuf.slice(..));
            rp.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
            rp.draw_indexed(0..t.index_count, 0, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::chunk::VOLUME;

    fn tile_of(f: impl Fn(usize, usize) -> Col) -> Tile0 {
        Tile0 { cols: (0..64 * 64).map(|q| f(q / 64, q % 64)).collect(), trees: false }
    }

    fn land(top: u8) -> Col {
        Col { id: 2, top, tint: [100, 200, 80], ..Col::default() }
    }

    fn sea(floor: u8) -> Col {
        Col { id: 12, top: floor, wtop: 63, tint: [255, 255, 255], ..Col::default() }
    }

    fn tree(c: Col) -> Col {
        Col { leaf: c.top + 8, lb: c.top + 4, lmeta: 0, ltint: [40, 120, 40], log: true, ..c }
    }

    fn store() -> Store {
        Store { g: OverworldGenerator::new(0xCAFEBABE), cm: WorldChunkManager::new(0xCAFEBABE), tiles: HashMap::new(), real: HashMap::new() }
    }

    /// A column reads past plants to the top block, and the water over a floor is kept apart from it.
    #[test]
    fn a_column_is_its_top_block_and_the_water_over_it() {
        let (mut b, d, t) = (vec![0u8; VOLUME], Nibbles::new(), Tints::white());
        let base = idx(3, 0, 4);
        (0..=60).for_each(|y| b[base + y] = 1);
        (61..=63).for_each(|y| b[base + y] = 9);
        b[base + 64] = 31; // tall grass: not a surface
        let c = column(&b, &d, &t, 3, 4);
        assert_eq!((c.id, c.top, c.wtop), (1, 60, 63));
        assert_eq!(column(&b, &d, &t, 0, 0), Col::default());
    }

    /// Leaves seen first are a tree over its ground (the trunk is read through, the lowest leaf and the top face are kept); a log seen
    /// first is a plain block.
    #[test]
    fn a_tree_is_leaves_over_its_own_ground() {
        let (mut b, d, t) = (vec![0u8; VOLUME], Nibbles::new(), Tints::white());
        let base = idx(2, 0, 9);
        (0..=63).for_each(|y| b[base + y] = if y == 63 { 2 } else { 3 });
        (64..=65).for_each(|y| b[base + y] = 17);
        (66..=68).for_each(|y| b[base + y] = 18);
        let c = column(&b, &d, &t, 2, 9);
        assert_eq!((c.id, c.top, c.leaf, c.lb, c.log, c.wtop), (2, 63, 69, 66, true, 0));
        (66..=68).for_each(|y| b[base + y] = 0);
        let c = column(&b, &d, &t, 2, 9);
        assert_eq!((c.id, c.top, c.leaf, c.log), (17, 65, 0, false));
    }

    /// `Mipper.mip`: the highest child wins (a tree top over the lawn), and a bay with one beach block in it stays water.
    #[test]
    fn a_cell_keeps_the_highest_child_and_a_bay_stays_water() {
        let t = tile_of(|x, z| if (x, z) == (1, 1) { land(90) } else { land(70) });
        assert_eq!(resolve(&t, 0, 0, 4).top, 90);
        // A forest cell (half of it under leaves) keeps its tree mass; one tree on a lawn is not worth a 4 block wide box.
        let t = tile_of(|x, z| if x < 2 { tree(land(70)) } else { land(70) });
        let c = resolve(&t, 0, 0, 4);
        assert_eq!((c.top, c.leaf, c.lb), (70, 78, 74));
        let t = tile_of(|x, z| if (x, z) == (1, 1) { tree(land(70)) } else { land(70) });
        assert_eq!(resolve(&t, 0, 0, 4).leaf, 0);
        let t = tile_of(|x, z| if (x, z) == (1, 1) { land(64) } else { sea(40) });
        let c = resolve(&t, 0, 0, 4);
        assert!(c.wtop > 0, "one low beach block does not make an island of a 4 x 4 cell");
        let t = tile_of(|x, _| if x < 2 { sea(40) } else { land(64) });
        assert_eq!(resolve(&t, 0, 0, 4).wtop, 0, "half land is land");
    }

    /// Deeper water is darker and bluer-greyer than a shallow sheet over the same sand.
    #[test]
    fn water_is_darker_the_deeper_it_is() {
        let shallow = water_rgb(Col { id: 12, top: 61, wtop: 63, ..Col::default() });
        let deep = water_rgb(Col { id: 12, top: 30, wtop: 63, ..Col::default() });
        assert!(deep[0] < shallow[0] && deep[1] < shallow[1], "{shallow:?} vs {deep:?}");
        assert!(shallow[2] > shallow[0], "water is blue");
    }

    /// A real tile at step 8: every index in range, the surface inside the world, walls where the ground steps, and the cut takes the cells
    /// the chunk ring covers and nothing else.
    #[test]
    fn a_tile_meshes_to_cells_and_the_ring_cuts_a_hole() {
        let mut st = store();
        let b = build(&mut st, (0, 0), 8, false);
        assert_eq!(b.cells.len(), 64, "8 x 8 cells of 8 blocks");
        assert_eq!(b.verts.len() % 4, 0);
        assert!(b.idx.iter().all(|&i| (i as usize) < b.verts.len()));
        assert!(b.verts.iter().all(|v| v.pos[1] > -20.0 && v.pos[1] <= 129.0 && v.tint >= 0.0 && v.tint < 16_777_216.0));
        assert!(b.verts.len() > 64 * 4, "a rolling tile has walls as well as tops");
        assert!(b.cells.windows(2).all(|w| w[0].i1 == w[1].i0) && b.cells.last().unwrap().i1 as usize == b.idx.len());
        let eye = (Vec3::new(32.0, 80.0, 32.0), 64.0, 48.0);
        let cut = gather(&b.cells, &b.idx, &covered(&b.cells, Some(eye)));
        assert!(cut.len() < b.idx.len(), "cells under the ring are dropped");
        let far = (Vec3::new(1000.0, 80.0, 1000.0), 64.0, 48.0);
        assert_eq!(gather(&b.cells, &b.idx, &covered(&b.cells, Some(far))).len(), b.idx.len(), "an eye far away drops nothing");
    }

    /// A tree on flat ground is a floating box on a trunk, not a tower: nothing but the tree rises above the lawn.
    #[test]
    fn a_tree_meshes_as_a_box_on_a_trunk() {
        let mut st = store();
        st.tiles.insert((0, 0), tile_of(|x, z| if (x, z) == (10, 10) { tree(land(70)) } else { land(70) }));
        let b = build(&mut st, (0, 0), 1, false);
        assert!(b.idx.iter().all(|&i| (i as usize) < b.verts.len()));
        let cell = b.cells.iter().find(|c| (c.x, c.z) == (10, 10)).unwrap();
        assert_eq!(cell.y, 78);
        let high = b.verts.iter().filter(|v| v.pos[1] > 71.5).count();
        assert!(high > 0 && high < 200, "{high} vertices over the lawn");
        let top = b.verts.iter().map(|v| v.pos[1]).fold(f32::MIN, f32::max);
        assert!(top > 78.9 && top < 79.0, "the leaf top is at 79 less EPS, got {top}");
    }

    /// A real chunk replaces the guess: in a tile made after it arrived, and in one that already exists.
    #[test]
    fn an_ingested_chunk_replaces_the_guess() {
        let mut st = store();
        st.ingest((1, 2), vec![land(99); 256]);
        assert_eq!(st.tile((0, 0), false).cols[(16 + 5) * 64 + 32 + 7].top, 99);
        st.ingest((3, 3), vec![land(98); 256]);
        assert_eq!(st.tiles[&(0, 0)].cols[(48 + 1) * 64 + 48 + 1].top, 98, "a tile that exists is patched in place");
        assert_ne!(st.tiles[&(0, 0)].cols[16 * 64 + 7].top, 99, "chunk (1, 0) was not touched");
    }
}
