//! Far terrain, Voxy style: the view reaches `RADIUS` blocks with the SAME blocks, textures, tints and face shades as the real
//! chunks, not a smooth painted heightfield.
//!
//! Voxy keeps the real 3D block data of far sections, mips it 2x2x2 at a time (`Mipper.mip`) and meshes every level as voxels:
//! a face is drawn wherever a solid voxel touches a see-through one (`RenderDataFactory`, `shouldMeshNonOpaqueBlockFace`). This
//! file does the same for a 128 high world. A worker thread makes the real columns of a 64 x 64 tile (`OverworldGenerator::generate`,
//! plus `populate` for the trees when the tile is near; the same generator, so the same terrain and biome tints as the chunk
//! ring) and keeps each column as a few vertical runs of equal blocks (`Span`: tree crown, trunk, grass, dirt ...; below the
//! last run it is stone). To mesh a tile at a step of 1, 2, 4, 8 or 16 blocks it fills a dense voxel grid of the tile plus one cell
//! of margin (the neighbours' columns), mips it `log2(step)` times (`mip`) and emits a face for every solid / see-through
//! boundary (`build`). So a tree is a real mass of leaf voxels on a trunk (not a pillar from the crown to the lawn), a cliff
//! is a stack of voxels whose height is a multiple of the step (not an arbitrary wall), and an overhang or a crown over a gap
//! stays what it is.
//!
//! The mip differs from Voxy's in two places, both chosen against the bugs seen in the screenshots:
//! - Voxy turns a cell solid when ANY of its eight children is, which bloats terrain by up to a step (peaks become pillars). Here
//!   a cell is solid when at least half of its children are (rounds the surface to the nearest step, no bias), except leaves / logs,
//!   which count from one child (a forest keeps its canopy) - UNVERIFIED against how it looks, picked on paper.
//! - Voxy picks the child of highest opacity, so leaves over dirt turn into dirt. Here the upper four children are looked at first
//!   (what you see from above), then opacity, then Voxy's own tie-break (+y, +z, +x).
//!
//! Blending with the real chunks:
//! - same atlas tiles, same grass / foliage tint, same face shade (top 1.0, Z sides 0.8, X sides 0.6), same light (sky 15), same fog;
//! - the LoD is cut out only where real sections are drawn AND have a mesh (`Lod::update`'s `drawn`, `ChunkManager::drawn`): the old
//!   cut followed the ring's range alone, so a chunk still generating or meshing left a hole to the sky. The part that stays under
//!   the real chunks is `EPS` lower (walls `EPS` inside their voxel) so the real surface always wins the depth test.
//! - a face on a tile's edge hangs a skirt into the ground (`build`): the neighbour tile may be at another step, and the two surfaces
//!   then differ by up to a step.
//!
//! Real chunks first (Voxy's `VoxelIngestService`): every chunk the chunk manager has finished (all four populates that write into
//! it done, `ChunkManager::take_ingest`) and every one it unloads after an edit is turned into columns (`columns`) and handed to
//! `Lod::ingest`; they replace the guessed columns of their tile, now and in every tile made later (`Store::real`), and the tile
//! is meshed again a moment after the last chunk of a burst (`REMESH_WAIT`, Voxy's `ChunkUpdateTracker` throttle).
//!
//! Missing tiles are asked for before tiles that only lack their trees (Voxy shows a coarse section where a fine one is not ready;
//! a tile that is not there at all is a hole to the sky), and a tile whose trees are not grown yet is made bare, fast.
//!
//! Pre-generation (Voxy World Gen's `DistanceGraph.findWork`; the pause menu's `Pregen` button, `Lod::toggle_pregen`): while it is
//! on, the worker makes the tiles around the player nearest first, each with its trees, and keeps them on disk (`<world>/lod/`).
//!
//! ponytail: a column keeps the runs down to `DEPTH` blocks under its ground and is stone below (caves and ravines under the surface
//! are not in the LoD, only the openings you could see into from outside are lost); plants (flowers, tall grass) are skipped; water
//! is one sheet at the top of the water (no sides, coloured by the depth over its floor); faces are one quad per voxel face (no
//! greedy merge: a step-1 tile of a forest is the most expensive); trees beyond `TREES` blocks are not grown (a forest there is a
//! lawn); one worker thread. UNVERIFIED: written without a toolchain (not compiled, not run), nothing measured on a device; the
//! distances (`step_for`, `TREES`), the slack values and the mip thresholds were picked on paper.

use crate::gpu::pipeline::{create_index_buffer, create_vertex_buffer, Vertex};
use crate::render::atlas;
use crate::render::mesh::{lm, tint_of, Tints};
use crate::world::chunk::{brightness, idx, is_plant, opaque, Nibbles};
use crate::world::chunks::{biome_tints, chunk_coord, in_range, Shape};
use crate::world::gen::chunk_manager::WorldChunkManager;
use crate::world::gen::overworld::OverworldGenerator;
use crate::world::gen::populate::Region;
use glam::Vec3;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering::Relaxed};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Arc;
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
/// How far inside the real chunks' range a land voxel is cut out (the eye is quantised to 2 blocks: 1.74 at most). Water is a
/// translucent sheet in the real pass, so it is cut closer: a LoD sheet under it would show through as a second layer of water.
const LAND_SLACK: f32 = 8.0;
const WATER_SLACK: f32 = 2.5;
/// Tiles asked of the worker at once.
const MAX_PENDING: usize = 2;
/// The worker keeps this many tiles of columns (~100 KB each), the farthest are dropped past it. The view needs about 510.
const KEEP_TILES: usize = 600;
/// A tile that got real chunks is meshed again this long after the first of them (a burst of chunks costs one rebuild).
const REMESH_WAIT: Duration = Duration::from_secs(2);
/// Blocks a column keeps under its ground (the ground block and the three under it); stone (sandstone under sand) below.
const DEPTH: usize = 4;
/// The world's height in blocks.
const HEIGHT: usize = 128;

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

const PLAIN: [u8; 3] = [255, 255, 255];

/// A run of equal blocks in a column: y in `y0..y1`, `mesh::tint_of` of them in this column (grass / foliage colour, white
/// otherwise). Air, plants and (below the ground) everything under `DEPTH` are not kept.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Span {
    y0: u8,
    y1: u8,
    id: u8,
    meta: u8,
    tint: [u8; 3],
}

/// One block column as the LoD keeps it: its runs, top first. Empty = nothing (never in a real column: bedrock is at the bottom).
pub type Col = Vec<Span>;

fn rgb(t: u32) -> [u8; 3] {
    [(t >> 16) as u8, (t >> 8) as u8, t as u8]
}

/// The column (x, z) of a chunk's blocks (`(x << 11) | (z << 7) | y`): every non-air, non-plant block from the top down to
/// `DEPTH` blocks under the ground (the first opaque block that is not a log or leaves, so a tree's trunk is read through to the
/// grass under it), as runs. Both kinds of water are one id (9).
fn column(blocks: &[u8], meta: &Nibbles, tints: &Tints, x: usize, z: usize) -> Col {
    let base = idx(x, 0, z);
    let mut out: Col = Vec::new();
    let mut ground: Option<usize> = None;
    for y in (0..HEIGHT).rev() {
        if ground.is_some_and(|g| y + DEPTH <= g) {
            break;
        }
        let mut b = blocks[base + y];
        if b == 0 || is_plant(b) {
            continue;
        }
        if b == 8 {
            b = 9;
        }
        let m = meta.get(x, y, z) & 15;
        let t = if matches!(b, 2 | 18) { rgb(tint_of(tints, b, m, x * 16 + z) as u32) } else { PLAIN };
        match out.last_mut() {
            Some(s) if s.y0 as usize == y + 1 && s.id == b && s.meta == m && s.tint == t => s.y0 = y as u8,
            _ => out.push(Span { y0: y as u8, y1: y as u8 + 1, id: b, meta: m, tint: t }),
        }
        if ground.is_none() && opaque(b) && !matches!(b, 17 | 18) {
            ground = Some(y);
        }
    }
    out
}

/// The 256 columns of a real chunk (index `x * 16 + z`), for `Lod::ingest`.
pub fn columns(blocks: &[u8], meta: &Nibbles, tints: &Tints) -> Vec<Col> {
    (0..256).map(|i| column(blocks, meta, tints, i >> 4, i & 15)).collect()
}

/// sRGB bytes of the water over its floor, seen from above: the floor (`block_color`) lit by what is left of the sky light under
/// `depth` blocks of water (3 a block, `Block.lightOpacity`), mixed with the water tile's average colour (47, 67, 244) at its alpha
/// (138 / 255), in linear light. The real water is a translucent sheet over the real floor in the same way.
fn water_rgb(floor_id: u8, depth: i32) -> [u8; 3] {
    let br = brightness((15 - 3 * depth.max(1)).max(0) as u8);
    let floor = atlas::block_color(floor_id);
    let water = [47.0f32, 67.0, 244.0];
    let mix = |f: u8, w: f32| {
        let l = (f as f32 / 255.0).powf(2.2) * br;
        let o = l + ((w / 255.0).powf(2.2) - l) * (138.0 / 255.0);
        (o.powf(1.0 / 2.2) * 255.0).round().clamp(0.0, 255.0) as u8
    };
    [mix(floor[0], water[0]), mix(floor[1], water[1]), mix(floor[2], water[2])]
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

/// The 64 x 64 columns of one tile (`start[x * 64 + z]..start[x * 64 + z + 1]` indexes `spans`), and whether its trees were grown.
struct Tile0 {
    start: Vec<u32>,
    spans: Vec<Span>,
    trees: bool,
}

impl Tile0 {
    fn from_cols(cols: &[Col], trees: bool) -> Self {
        let mut start = Vec::with_capacity(64 * 64 + 1);
        let mut spans = Vec::new();
        for c in cols {
            start.push(spans.len() as u32);
            spans.extend_from_slice(c);
        }
        start.push(spans.len() as u32);
        Tile0 { start, spans, trees }
    }

    fn col(&self, q: usize) -> &[Span] {
        &self.spans[self.start[q] as usize..self.start[q + 1] as usize]
    }

    /// Lay the columns of real chunk `k` over its place in this tile.
    fn put(&mut self, k: (i32, i32), cols: &[Col]) {
        let (ox, oz) = (k.0.rem_euclid(4) as usize * 16, k.1.rem_euclid(4) as usize * 16);
        let mut all: Vec<Col> = Vec::with_capacity(64 * 64);
        for q in 0..64 * 64 {
            let (x, z) = (q / 64, q % 64);
            if (ox..ox + 16).contains(&x) && (oz..oz + 16).contains(&z) {
                all.push(cols[(x - ox) * 16 + (z - oz)].clone());
            } else {
                all.push(self.col(q).to_vec());
            }
        }
        *self = Tile0::from_cols(&all, self.trees);
    }
}

/// What the worker owns: its own generator (the same world as the chunk ring's) and the columns it has made.
struct Store {
    g: OverworldGenerator,
    cm: WorldChunkManager,
    tiles: HashMap<(i32, i32), Tile0>,
    /// The columns of the real chunks `Lod::ingest` handed over, by chunk: laid over a tile whenever it is made (so they outlive it).
    real: HashMap<(i32, i32), Vec<Col>>,
    /// `<world>/lod`: tiles made with their trees are kept here (`flush`), None = nothing is read or written (the tests).
    dir: Option<PathBuf>,
    tectonic: bool,
    /// Tiles in memory that differ from their file.
    dirty: HashSet<(i32, i32)>,
    /// Tiles that are real (with trees) now or were loaded from a file: the pre-generation's progress.
    done: HashSet<(i32, i32)>,
}

const MAGIC: [u8; 2] = *b"L3";

fn tile_path(d: &Path, k: (i32, i32)) -> PathBuf {
    d.join(format!("{}_{}.lod", k.0, k.1))
}

/// A tile file: magic, world type, trees, then per column a run count (u8) and 7 bytes a run (y0, y1, id, meta, tint).
fn encode(t: &Tile0, tectonic: bool) -> Vec<u8> {
    let mut o = vec![MAGIC[0], MAGIC[1], tectonic as u8, t.trees as u8];
    for q in 0..64 * 64 {
        let c = t.col(q);
        o.push(c.len() as u8);
        for s in c {
            o.extend_from_slice(&[s.y0, s.y1, s.id, s.meta, s.tint[0], s.tint[1], s.tint[2]]);
        }
    }
    o
}

/// The tile in `b`, None when it is not a tile file of this format and world type or is cut short.
fn decode(b: &[u8], tectonic: bool) -> Option<Tile0> {
    if b.len() < 4 || b[..2] != MAGIC[..] || (b[2] != 0) != tectonic {
        return None;
    }
    let mut cols: Vec<Col> = Vec::with_capacity(64 * 64);
    let mut p = 4;
    for _ in 0..64 * 64 {
        let n = *b.get(p)? as usize;
        let raw = b.get(p + 1..p + 1 + n * 7)?;
        cols.push(raw.chunks_exact(7).map(|c| Span { y0: c[0], y1: c[1], id: c[2], meta: c[3], tint: [c[4], c[5], c[6]] }).collect());
        p += 1 + n * 7;
    }
    (p == b.len()).then(|| Tile0::from_cols(&cols, b[3] != 0))
}

/// What the render thread and the worker share: the pre-generation switch and its count, and the chunk the player is in.
#[derive(Default)]
struct Shared {
    on: AtomicBool,
    chunks: AtomicU32,
    cx: AtomicI32,
    cz: AtomicI32,
}

impl Store {
    fn new(g: OverworldGenerator, cm: WorldChunkManager, dir: Option<PathBuf>, tectonic: bool) -> Self {
        Store { g, cm, tiles: HashMap::new(), real: HashMap::new(), dir, tectonic, dirty: HashSet::new(), done: HashSet::new() }
    }

    fn load(&self, k: (i32, i32)) -> Option<Tile0> {
        decode(&std::fs::read(tile_path(self.dir.as_ref()?, k)).ok()?, self.tectonic)
    }

    /// Write the tiles that changed (to a temporary file first, then moved over the old one).
    fn flush(&mut self) {
        let Some(d) = self.dir.clone() else { return };
        for k in std::mem::take(&mut self.dirty) {
            let Some(t) = self.tiles.get(&k) else { continue };
            let p = tile_path(&d, k);
            if let Err(e) = std::fs::write(p.with_extension("tmp"), encode(t, self.tectonic)).and_then(|_| std::fs::rename(p.with_extension("tmp"), &p)) {
                log::warn!("lod: {p:?}: {e}");
            }
        }
    }

    /// Lay the real chunks that are known over tile `k`; true if there were any.
    fn overlay(&self, t: &mut Tile0, k: (i32, i32)) -> bool {
        let mut any = false;
        for ci in 0..4 {
            for cj in 0..4 {
                let c = (k.0 * 4 + ci, k.1 * 4 + cj);
                if let Some(r) = self.real.get(&c) {
                    t.put(c, r);
                    any = true;
                }
            }
        }
        any
    }

    /// A real chunk replaces what the generator guessed for it: in its tile if that is made, and in the tile when it is made.
    fn ingest(&mut self, k: (i32, i32), cols: Vec<Col>) {
        let tk = (k.0.div_euclid(4), k.1.div_euclid(4));
        if let Some(t) = self.tiles.get_mut(&tk) {
            t.put(k, &cols);
            if t.trees {
                self.dirty.insert(tk);
            }
        }
        self.real.insert(k, cols);
    }

    /// The columns of tile `k`, made now if missing (or if `trees` are wanted and it has none).
    fn tile(&mut self, k: (i32, i32), trees: bool) -> &Tile0 {
        if self.tiles.get(&k).map_or(true, |t| trees && !t.trees) {
            let loaded = self.load(k).filter(|t| t.trees || !trees);
            let fresh = loaded.is_none();
            let mut t = match loaded {
                Some(t) => t,
                None => self.make(k, trees),
            };
            let real = self.overlay(&mut t, k);
            if t.trees {
                self.done.insert(k);
                if fresh || real {
                    self.dirty.insert(k);
                }
            }
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
        let mut cols: Vec<Col> = vec![Vec::new(); 64 * 64];
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
        Tile0::from_cols(&cols, trees)
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

// ---- the voxel grid: a voxel is `id | meta << 8 | tint palette index << 16` (id 0 = air) ----

fn vid(v: u32) -> u8 {
    v as u8
}

/// The tints of one build, as an index in the voxel (0 = white, at most 255 distinct: more share the last).
struct Pal {
    v: Vec<[u8; 3]>,
    m: HashMap<[u8; 3], u32>,
}

impl Pal {
    fn new() -> Self {
        Pal { v: vec![PLAIN], m: HashMap::new() }
    }

    fn get(&mut self, t: [u8; 3]) -> u32 {
        if t == PLAIN {
            return 0;
        }
        if let Some(&i) = self.m.get(&t) {
            return i;
        }
        if self.v.len() >= 255 {
            return self.v.len() as u32 - 1;
        }
        self.v.push(t);
        self.m.insert(t, self.v.len() as u32 - 1);
        self.v.len() as u32 - 1
    }
}

/// The dense level-0 grid of tile `key` plus `s` blocks of margin on its four sides, index `(y * w + z) * w + x`, `w = 64 + 2 s`.
/// Under a column's last run it is stone (sandstone under sand). The 3 x 3 tiles around `key` must exist.
fn fill(store: &Store, key: (i32, i32), s: usize, pal: &mut Pal) -> Vec<u32> {
    let w = TILE as usize + 2 * s;
    let mut g = vec![0u32; w * HEIGHT * w];
    for rx in 0..w {
        let wx = key.0 * TILE - s as i32 + rx as i32;
        let (tx, lx) = (wx.div_euclid(TILE), wx.rem_euclid(TILE) as usize);
        for rz in 0..w {
            let wz = key.1 * TILE - s as i32 + rz as i32;
            let (tz, lz) = (wz.div_euclid(TILE), wz.rem_euclid(TILE) as usize);
            let col = store.tiles[&(tx, tz)].col(lx * 64 + lz);
            let Some(last) = col.last() else { continue };
            let under: u32 = if last.id == 12 { 24 } else { 1 };
            for y in 0..last.y0 as usize {
                g[(y * w + rz) * w + rx] = under;
            }
            for sp in col {
                let v = sp.id as u32 | (sp.meta as u32 & 15) << 8 | pal.get(sp.tint) << 16;
                for y in sp.y0 as usize..sp.y1 as usize {
                    g[(y * w + rz) * w + rx] = v;
                }
            }
        }
    }
    g
}

/// What a voxel wins with among the children of a mip: solid blocks over leaves over water.
fn prio(id: u8) -> usize {
    match id {
        9 => 1,
        18 => 2,
        _ => 3,
    }
}

/// `Mipper.mip` for eight children, index `dy << 2 | dz << 1 | dx`: the upper four are looked at first (what you see from above),
/// then `prio`, then Voxy's own tie-break (the highest index). The cell is air unless enough children are not: leaves and logs
/// are kept from one child, everything else from half of them (see the module doc).
fn pick(k: &[u32; 8]) -> u32 {
    let (mut n, mut best, mut key) = (0, 0u32, 0usize);
    for (q, &v) in k.iter().enumerate() {
        let id = vid(v);
        if id == 0 {
            continue;
        }
        n += 1;
        let kq = (q >> 2) * 1000 + prio(id) * 8 + q;
        if kq >= key {
            (best, key) = (v, kq);
        }
    }
    let need = if matches!(vid(best), 17 | 18) { 1 } else { 4 };
    if n >= need && n > 0 {
        best
    } else {
        0
    }
}

/// One mip level: a grid `w` x `h` x `w` becomes `w / 2` x `h / 2` x `w / 2`.
fn mip(src: &[u32], w: usize, h: usize) -> Vec<u32> {
    let (w2, h2) = (w / 2, h / 2);
    let mut out = vec![0u32; w2 * h2 * w2];
    for y in 0..h2 {
        for z in 0..w2 {
            for x in 0..w2 {
                let mut k = [0u32; 8];
                for (q, c) in k.iter_mut().enumerate() {
                    let (dx, dz, dy) = (q & 1, (q >> 1) & 1, q >> 2);
                    *c = src[((y * 2 + dy) * w + z * 2 + dz) * w + x * 2 + dx];
                }
                out[(y * w2 + z) * w2 + x] = pick(&k);
            }
        }
    }
    out
}

/// One voxel's place in the view: where it is (its section decides if the real chunks cover it) and its slice of the indices.
struct Cell {
    x: i32,
    y: i32,
    z: i32,
    water: bool,
    i0: u32,
    i1: u32,
}

/// What the worker hands back: a tile's vertices, indices (voxel after voxel) and cells.
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

    /// A face looking up (`top`) or down at height `y`, over the square at (x0, z0) of side `sf`, with the block's tile.
    #[allow(clippy::too_many_arguments)]
    fn flat(&mut self, top: bool, x0: f32, z0: f32, sf: f32, y: f32, id: u8, meta: u8, tint: f32) {
        let (x1, z1) = (x0 + sf, z0 + sf);
        let uv = |u: f32, v: f32| face_uv(id, meta, if top { 1 } else { 0 }, u, v);
        let uvs = [uv(0.0, 0.0), uv(1.0, 0.0), uv(1.0, 1.0), uv(0.0, 1.0)];
        if top {
            self.quad([[x0, y, z1], [x1, y, z1], [x1, y, z0], [x0, y, z0]], uvs, 1.0, tint);
        } else {
            self.quad([[x0, y, z0], [x1, y, z0], [x1, y, z1], [x0, y, z1]], uvs, 0.5, tint);
        }
    }

    /// A wall of the voxel at (x0, z0) facing `side`, from `yb` to `yt`, `EPS` inside the voxel; `v0` is where the tile starts (a
    /// 1/8 high snow layer shows the top eighth).
    #[allow(clippy::too_many_arguments)]
    fn wall(&mut self, side: Side, x0: f32, z0: f32, sf: f32, yt: f32, yb: f32, v0: f32, id: u8, meta: u8, tint: f32) {
        let ((di, dj), tile, shade, corners) = side;
        let p = corners.map(|cn| [x0 + cn[0] * sf - EPS * di as f32, if cn[1] > 0.5 { yt } else { yb }, z0 + cn[2] * sf - EPS * dj as f32]);
        let uv = [face_uv(id, meta, tile, 0.0, v0), face_uv(id, meta, tile, 1.0, v0), face_uv(id, meta, tile, 1.0, 1.0), face_uv(id, meta, tile, 0.0, 1.0)];
        self.quad(p, uv, shade, tint);
    }
}

/// The four sides, in the order of `render::mesh::FACES`' +X, -X, +Z, -Z: the neighbour's offset, the side index of `terrain_tile`, the
/// face shade (X 0.6, Z 0.8), the face's unit corners (bottom-left, bottom-right, top-right, top-left seen from outside).
type Side = ((i32, i32), u8, f32, [[f32; 3]; 4]);
const SIDES: [Side; 4] = [
    ((1, 0), 5, 0.6, [[1.0, 0.0, 1.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 1.0, 1.0]]),
    ((-1, 0), 4, 0.6, [[0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 1.0], [0.0, 1.0, 0.0]]),
    ((0, 1), 3, 0.8, [[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]]),
    ((0, -1), 2, 0.8, [[1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]]),
];

/// Does a voxel hide the faces behind it? Opaque blocks do (leaves too: the real mesher draws them as cubes); a snow layer is a
/// full block from step 2 on.
fn opq(id: u8, s: usize) -> bool {
    opaque(id) || (s > 1 && id == 78)
}

/// The mesh of tile `key` with a voxel every `step` blocks.
fn build(store: &mut Store, key: (i32, i32), step: i32, trees: bool) -> Built {
    let s = step as usize;
    let n = TILE as usize / s;
    let trees = store.tile(key, trees).trees;
    for dx in -1..=1 {
        for dz in -1..=1 {
            if (dx, dz) != (0, 0) {
                store.tile((key.0 + dx, key.1 + dz), false); // bare is enough for the margin
            }
        }
    }
    let mut pal = Pal::new();
    let mut g = fill(store, key, s, &mut pal);
    let (mut w, mut h) = (TILE as usize + 2 * s, HEIGHT);
    let mut lv = 1;
    while lv < s {
        g = mip(&g, w, h);
        (w, h, lv) = (w / 2, h / 2, lv * 2);
    }
    // Now `w == n + 2` (a voxel of margin on each side) and `h == HEIGHT / s`; the tile's voxels are 1..=n.
    let at = |i: usize, k: i32, j: usize| -> u32 {
        if k < 0 {
            1 // below the world: solid, no faces down there
        } else if k as usize >= h {
            0
        } else {
            g[(k as usize * w + j) * w + i]
        }
    };
    let sf = s as f32;
    // A wall on a tile's edge hangs deeper: the neighbouring tile may be coarser, and its surface then differs from the one this
    // side measured.
    let skirt = (2 * step).clamp(2, 32) as f32;
    let mut q = Quads { verts: Vec::new(), idx: Vec::new() };
    let mut cells: Vec<Cell> = Vec::new();
    for gi in 1..=n {
        for gj in 1..=n {
            let (x0, z0) = ((key.0 * TILE) as f32 + ((gi - 1) * s) as f32, (key.1 * TILE) as f32 + ((gj - 1) * s) as f32);
            for k in 0..h {
                let v = g[(k * w + gj) * w + gi];
                let id = vid(v);
                if id == 0 {
                    continue;
                }
                let ki = k as i32;
                let y0 = (k * s) as f32;
                let i0 = q.idx.len() as u32;
                if id == 9 {
                    // Water: one sheet on the top voxel, coloured by what is under it.
                    let up = vid(at(gi, ki + 1, gj));
                    if up != 9 && !opq(up, s) {
                        let (mut d, mut kk) = (1, ki - 1);
                        while kk >= 0 && vid(at(gi, kk, gj)) == 9 {
                            (d, kk) = (d + 1, kk - 1);
                        }
                        let floor = if kk >= 0 { vid(at(gi, kk, gj)) } else { 1 };
                        let y = y0 + sf - 1.0 / 9.0 - EPS;
                        let uv = atlas::atlas_uv(80, 0.0, 0.0); // the snow block's flat texel: white, so the tint is the colour
                        let (x1, z1) = (x0 + sf, z0 + sf);
                        q.quad([[x0, y, z1], [x1, y, z1], [x1, y, z0], [x0, y, z0]], [[uv.0, uv.1]; 4], 1.0, pack(water_rgb(floor, d * s as i32)));
                    }
                } else {
                    let meta = ((v >> 8) & 15) as u8;
                    let tint = pack(pal.v[(v >> 16) as usize]);
                    let hgt = if id == 78 && s == 1 { 0.125 } else { sf };
                    let nb = |di: i32, dk: i32, dj: i32| vid(at((gi as i32 + di) as usize, ki + dk, (gj as i32 + dj) as usize));
                    // A face is hidden by an opaque neighbour, by water (its sheet is drawn instead) and by the same block (glass, ice).
                    let hide = |o: u8| o != 0 && (o == 9 || opq(o, s) || o == id);
                    if !hide(nb(0, 1, 0)) {
                        q.flat(true, x0, z0, sf, y0 + hgt - EPS, id, meta, tint);
                    }
                    if !hide(nb(0, -1, 0)) {
                        q.flat(false, x0, z0, sf, y0 + EPS, id, meta, if id == 2 { pack(PLAIN) } else { tint });
                    }
                    for side in SIDES {
                        let ((di, dj), ..) = side;
                        if hide(nb(di, 0, dj)) {
                            continue;
                        }
                        q.wall(side, x0, z0, sf, y0 + hgt - EPS, y0 + EPS, 1.0 - hgt / sf, id, meta, tint);
                        let edge = (di == -1 && gi == 1) || (di == 1 && gi == n) || (dj == -1 && gj == 1) || (dj == 1 && gj == n);
                        // The skirt hangs from the lowest exposed voxel of the face, through the solid ground under it.
                        if edge && ki > 0 && opq(nb(0, -1, 0), s) && opq(nb(di, -1, dj), s) {
                            q.wall(side, x0, z0, sf, y0 + EPS, y0 - skirt, 0.0, 1, 0, pack(PLAIN));
                        }
                    }
                }
                let i1 = q.idx.len() as u32;
                if i1 > i0 {
                    let (cx, cy, cz, wet) = (x0 as i32, (k * s) as i32, z0 as i32, id == 9);
                    match cells.last_mut() {
                        Some(c) if (c.x, c.z, c.y >> 4, c.water) == (cx, cz, cy >> 4, wet) && c.i1 == i0 => c.i1 = i1,
                        _ => cells.push(Cell { x: cx, y: cy, z: cz, water: wet, i0, i1 }),
                    }
                }
            }
        }
    }
    Built { key, step, trees, verts: q.verts, idx: q.idx, cells }
}

/// Which cells the real chunks cover around `eye` = (position, range, vertical range): the same `in_range` test as
/// `ChunkManager::update_visible` on the section of the cell, made smaller by the slack, and only where that chunk has a mesh
/// (`drawn`): a chunk that is in range but still generating or meshing leaves the LoD in place instead of a hole.
fn covered(cells: &[Cell], eye: Option<(Vec3, f32, f32)>, drawn: &dyn Fn(i32, i32) -> bool) -> Vec<bool> {
    let Some((e, rh, rv)) = eye else { return vec![false; cells.len()] };
    cells
        .iter()
        .map(|c| {
            let slack = if c.water { WATER_SLACK } else { LAND_SLACK };
            let min = Vec3::new((c.x >> 4) as f32 * 16.0, (c.y >> 4) as f32 * 16.0 - 1.0, (c.z >> 4) as f32 * 16.0);
            in_range(Shape::Sphere, e, min, min + Vec3::new(16.0, 18.0, 16.0), rh - slack, rv - 2.0) && drawn(c.x >> 4, c.z >> 4)
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
    /// `ChunkManager::mesh_gen` the cut was last worked out for: a chunk mesh landing or leaving changes what is covered.
    gen: u32,
    /// The cells the index buffer leaves out (`covered`): it is made again only when this changes, not at every step of the eye.
    cov: Vec<bool>,
    /// Not cut yet.
    stale: bool,
}

impl Tile {
    fn cut(&mut self, device: &wgpu::Device, eye: Option<((i32, i32, i32), f32, f32)>, gen: u32, drawn: &dyn Fn(i32, i32) -> bool) {
        let cov = covered(&self.cells, eye.map(|(q, rh, rv)| (quant_eye(q), rh, rv)), drawn);
        if self.stale || cov != self.cov {
            let idx = gather(&self.cells, &self.all, &cov);
            self.index_count = idx.len() as u32;
            self.ibuf = (!idx.is_empty()).then(|| create_index_buffer(device, &idx));
            self.cov = cov;
        }
        self.hole = eye.map(|e| e.0);
        self.gen = gen;
        self.stale = false;
    }
}

/// What the worker is told: mesh a tile (x, z, step, trees), or take a real chunk's columns.
enum Msg {
    Build(i32, i32, i32, bool),
    Ingest((i32, i32), Vec<Col>),
    /// Look at the pre-generation switch again.
    Wake,
}

/// Voxy's `DistanceGraph.findWork`: the nearest tile (a 4 x 4 batch of chunks) within `RADIUS` of the player that is not real yet, made
/// now with its trees (or read from its file). None = every tile in range is done. The count is chunks (16 a tile).
fn pregen_step(st: &mut Store, sh: &Shared) -> Option<(i32, i32)> {
    let (tx, tz) = (sh.cx.load(Relaxed).div_euclid(4), sh.cz.load(Relaxed).div_euclid(4));
    let n = (RADIUS / TILE as f32) as i32 + 1;
    let mut best: Option<(i32, (i32, i32))> = None;
    for x in tx - n..=tx + n {
        for z in tz - n..=tz + n {
            let d = (x - tx).pow(2) + (z - tz).pow(2);
            if d <= n * n && !st.done.contains(&(x, z)) && best.map_or(true, |b| d < b.0) {
                best = Some((d, (x, z)));
            }
        }
    }
    let (_, k) = best?;
    st.tile(k, true);
    sh.chunks.store(st.done.len() as u32 * 16, Relaxed);
    Some(k)
}

pub struct Lod {
    tiles: HashMap<(i32, i32), Tile>,
    pending: HashSet<(i32, i32)>,
    /// Tiles that got real chunks since they were meshed: since when (`REMESH_WAIT`).
    dirty: HashMap<(i32, i32), Instant>,
    job: Sender<Msg>,
    rx: Receiver<Built>,
    /// Tiles the pre-generation made, to be meshed again (they may have been drawn as a guess).
    made: Receiver<(i32, i32)>,
    sh: Arc<Shared>,
    /// The tiles `update` found in the frustum.
    visible: Vec<(i32, i32)>,
}

impl Lod {
    /// A LoD for the world of `seed` (Tectonic or vanilla terrain, like the chunk ring), generated on its own thread.
    pub fn new(seed: i64, tectonic: bool, dir: Option<PathBuf>) -> Self {
        let (job, jobs) = channel::<Msg>();
        let (done, rx) = channel::<Built>();
        let (made_tx, made) = channel::<(i32, i32)>();
        let sh = Arc::new(Shared::default());
        let shared = sh.clone();
        std::thread::Builder::new()
            .name("lod".into())
            .spawn(move || {
                let mut g = OverworldGenerator::new(seed);
                if tectonic {
                    g = g.tectonic();
                }
                if let Some(d) = &dir {
                    let _ = std::fs::create_dir_all(d);
                }
                let mut store = Store::new(g, WorldChunkManager::new(seed), dir, tectonic);
                // Ends when the `Lod` (and so `job`) is dropped. While the pre-generation is on, a view build or an ingest that is
                // waiting goes first and a tile is made whenever there is none.
                loop {
                    let m = if shared.on.load(Relaxed) {
                        match jobs.try_recv() {
                            Ok(m) => Some(m),
                            Err(TryRecvError::Empty) => None,
                            Err(TryRecvError::Disconnected) => break,
                        }
                    } else {
                        match jobs.recv() {
                            Ok(m) => Some(m),
                            Err(_) => break,
                        }
                    };
                    match m {
                        Some(Msg::Wake) => {}
                        Some(Msg::Ingest(k, cols)) => store.ingest(k, cols),
                        Some(Msg::Build(tx, tz, step, trees)) => {
                            let b = build(&mut store, (tx, tz), step, trees);
                            store.flush();
                            store.prune((tx, tz));
                            if done.send(b).is_err() {
                                break;
                            }
                        }
                        None => match pregen_step(&mut store, &shared) {
                            Some(k) => {
                                store.flush();
                                store.prune((shared.cx.load(Relaxed).div_euclid(4), shared.cz.load(Relaxed).div_euclid(4)));
                                let _ = made_tx.send(k);
                            }
                            None => shared.on.store(false, Relaxed),
                        },
                    }
                }
                store.flush();
            })
            .expect("lod thread");
        Lod { tiles: HashMap::new(), pending: HashSet::new(), dirty: HashMap::new(), job, rx, made, sh, visible: Vec::new() }
    }

    /// The pause menu's button: start or stop pre-generating the tiles around the player.
    pub fn toggle_pregen(&mut self) {
        self.sh.on.store(!self.sh.on.load(Relaxed), Relaxed);
        let _ = self.job.send(Msg::Wake);
    }

    /// (is it on, chunks generated so far).
    pub fn pregen(&self) -> (bool, u32) {
        (self.sh.on.load(Relaxed), self.sh.chunks.load(Relaxed))
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
    #[allow(clippy::too_many_arguments)]
    pub fn update(&mut self, device: &wgpu::Device, eye: Vec3, rh: f32, rv: f32, gen: u32, drawn: impl Fn(i32, i32) -> bool, in_frustum: impl Fn(Vec3, Vec3) -> bool) {
        while let Ok(b) = self.rx.try_recv() {
            self.pending.remove(&b.key);
            let vbuf = create_vertex_buffer(device, &b.verts);
            let t = Tile { step: b.step, trees: b.trees, vbuf, ibuf: None, index_count: 0, cells: b.cells, all: b.idx, hole: None, gen: 0, cov: Vec::new(), stale: true };
            self.tiles.insert(b.key, t);
        }
        self.sh.cx.store(chunk_coord(eye.x), Relaxed);
        self.sh.cz.store(chunk_coord(eye.z), Relaxed);
        while let Ok(k) = self.made.try_recv() {
            // A tile drawn from the guess is meshed again with its real columns.
            if self.tiles.get(&k).map_or(self.pending.contains(&k), |t| !t.trees) {
                self.dirty.entry(k).or_insert_with(Instant::now);
            }
        }
        let ts = TILE as f32;
        let r = RADIUS + ts * 0.75;
        let centre = |k: (i32, i32)| ((k.0 as f32 + 0.5) * ts, (k.1 as f32 + 0.5) * ts);
        let dist = |k: (i32, i32)| {
            let (cx, cz) = centre(k);
            ((cx - eye.x).powi(2) + (cz - eye.z).powi(2)).sqrt()
        };
        let mut want: Vec<(bool, f32, (i32, i32), i32, bool)> = Vec::new();
        let (x0, x1) = (((eye.x - r) / ts).floor() as i32, ((eye.x + r) / ts).floor() as i32);
        let (z0, z1) = (((eye.z - r) / ts).floor() as i32, ((eye.z + r) / ts).floor() as i32);
        for tx in x0..=x1 {
            for tz in z0..=z1 {
                let k = (tx, tz);
                let d = dist(k);
                if d > r || self.pending.contains(&k) {
                    continue;
                }
                // A tile that is not there yet is made bare (fast); its trees come when nothing is missing.
                let (step, trees) = (step_for(d), d < TREES && self.tiles.contains_key(&k));
                // Finer when it should be; coarser (to give the memory back) only when it is 4x too fine, so a tile at a boundary
                // does not flip.
                let fresh = self.dirty.get(&k).is_some_and(|t| t.elapsed() >= REMESH_WAIT);
                if self.tiles.get(&k).map_or(true, |t| fresh || t.step > step || t.step * 2 < step || (trees && !t.trees)) {
                    want.push((self.tiles.contains_key(&k), d, k, step, trees));
                }
            }
        }
        // A tile that is not there at all is a hole to the sky: those first, nearest first, then the ones that only change.
        want.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        for (_, _, k, step, trees) in want {
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
            if t.stale || t.hole != want || (near && t.gen != gen) {
                t.cut(device, want.map(|q| (q, rh, rv)), gen, &drawn);
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

    fn sp(y0: u8, y1: u8, id: u8) -> Span {
        Span { y0, y1, id, meta: 0, tint: if id == 2 || id == 18 { [100, 200, 80] } else { PLAIN } }
    }

    fn tile_of(f: impl Fn(usize, usize) -> Col) -> Tile0 {
        let cols: Vec<Col> = (0..64 * 64).map(|q| f(q / 64, q % 64)).collect();
        Tile0::from_cols(&cols, false)
    }

    /// Grass on dirt, the grass block at y = `top`.
    fn land(top: u8) -> Col {
        vec![sp(top, top + 1, 2), sp(top - 3, top, 3)]
    }

    fn store() -> Store {
        Store::new(OverworldGenerator::new(0xCAFEBABE), WorldChunkManager::new(0xCAFEBABE), None, false)
    }

    /// A store whose 3 x 3 tiles around (0, 0) are `f`.
    fn store_of(f: impl Fn(usize, usize) -> Col) -> Store {
        let mut st = store();
        for dx in -1..=1 {
            for dz in -1..=1 {
                st.tiles.insert((dx, dz), tile_of(&f));
            }
        }
        st
    }

    fn quads(b: &Built) -> usize {
        b.idx.len() / 6
    }

    fn top(b: &Built) -> f32 {
        b.verts.iter().map(|v| v.pos[1]).fold(f32::MIN, f32::max)
    }

    /// A column keeps the runs down to `DEPTH` under the ground, reads a trunk through to the grass, skips plants and merges water.
    #[test]
    fn a_column_is_runs_down_to_the_ground() {
        let (mut b, d, t) = (vec![0u8; VOLUME], Nibbles::new(), Tints::white());
        let base = idx(3, 0, 4);
        (0..=60).for_each(|y| b[base + y] = 1);
        (61..=62).for_each(|y| b[base + y] = 8);
        b[base + 63] = 9;
        b[base + 64] = 31; // tall grass: not a surface
        let c = column(&b, &d, &t, 3, 4);
        let ids: Vec<_> = c.iter().map(|s| (s.id, s.y0, s.y1)).collect();
        assert_eq!(ids, vec![(9, 61, 64), (1, 57, 61)], "water is one run, then the floor down to DEPTH blocks");
        assert!(column(&b, &d, &t, 0, 0).is_empty());
        let base = idx(2, 0, 9);
        (0..=63).for_each(|y| b[base + y] = if y == 63 { 2 } else { 3 });
        (64..=65).for_each(|y| b[base + y] = 17);
        (68..=70).for_each(|y| b[base + y] = 18); // a gap of air between trunk and crown
        let c = column(&b, &d, &t, 2, 9);
        let ids: Vec<_> = c.iter().map(|s| (s.id, s.y0, s.y1)).collect();
        assert_eq!(ids, vec![(18, 68, 71), (17, 64, 66), (2, 63, 64), (3, 60, 63)], "crown, trunk and the grass under it");
    }

    /// `Mipper.mip`: the upper children win, a lone block of terrain in the air is dropped, half of them keep it, a lone leaf stays.
    #[test]
    fn a_mip_rounds_terrain_and_keeps_leaves() {
        let (stone, leaf, water) = (1u32, 18u32, 9u32);
        assert_eq!(pick(&[0; 8]), 0);
        assert_eq!(pick(&[stone, 0, 0, 0, 0, 0, 0, 0]), 0, "one block of eight is noise");
        assert_eq!(pick(&[stone, stone, stone, stone, 0, 0, 0, 0]), stone, "the lower half solid is solid");
        assert_eq!(pick(&[0, 0, 0, 0, stone, stone, stone, stone]), stone);
        assert_eq!(pick(&[stone, stone, stone, stone, leaf, 0, 0, 0]), leaf, "a crown over the ground wins");
        assert_eq!(pick(&[0, 0, 0, 0, leaf, 0, 0, 0]), leaf, "a lone leaf stays: a forest keeps its canopy");
        assert_eq!(pick(&[stone, stone, stone, stone, water, water, water, water]), water, "water over its floor");
    }

    /// Flat ground meshes to its tops only: no phantom walls, no faces inside the ground, nothing at the tile's edge.
    #[test]
    fn flat_ground_is_tops_only() {
        let mut st = store_of(|_, _| land(70));
        let b = build(&mut st, (0, 0), 1, false);
        assert_eq!(quads(&b), 64 * 64);
        assert!((top(&b) - (71.0 - EPS)).abs() < 1e-4);
        assert!(b.idx.iter().all(|&i| (i as usize) < b.verts.len()));
        // At step 2 the surface (top of block 70) rounds to the step: half of the voxel [70, 72) is solid, so it is solid.
        let b = build(&mut st, (0, 0), 2, false);
        assert_eq!(quads(&b), 32 * 32);
        assert!((top(&b) - (72.0 - EPS)).abs() < 1e-4, "got {}", top(&b));
    }

    /// A tree on flat ground is a trunk and a crown of real voxels, not a tower from the crown down to the lawn: exactly the faces
    /// a real chunk would draw, and nothing rises above the crown.
    #[test]
    fn a_tree_is_a_trunk_and_a_crown() {
        let mut st = store_of(|x, z| if (x, z) == (10, 10) { vec![sp(74, 77, 18), sp(71, 74, 17), sp(70, 71, 2), sp(67, 70, 3)] } else { land(70) });
        let b = build(&mut st, (0, 0), 1, false);
        // the lawn minus the cell under the trunk, 12 trunk sides, 12 crown sides, the crown's top (trunk top / crown bottom are hidden)
        assert_eq!(quads(&b), 4095 + 12 + 12 + 1);
        assert!((top(&b) - (77.0 - EPS)).abs() < 1e-4);
        assert!(b.cells.iter().any(|c| (c.x, c.z) == (10, 10) && c.y >> 4 == 4), "the crown (y 74..77) is in section 4");
    }

    /// The cut takes only what the chunk ring covers AND has a mesh for, so a chunk still being built leaves the LoD in place.
    #[test]
    fn the_cut_waits_for_the_real_mesh() {
        let mut st = store_of(|_, _| land(70));
        let b = build(&mut st, (0, 0), 1, false);
        let eye = Some((Vec3::new(32.0, 80.0, 32.0), 64.0, 48.0));
        let all = |_: i32, _: i32| true;
        let none = |_: i32, _: i32| false;
        let cut = gather(&b.cells, &b.idx, &covered(&b.cells, eye, &all));
        assert!(cut.len() < b.idx.len(), "cells under the ring are dropped");
        assert_eq!(gather(&b.cells, &b.idx, &covered(&b.cells, eye, &none)).len(), b.idx.len(), "no mesh, no hole");
        let only = |cx: i32, cz: i32| (cx, cz) == (2, 2);
        let some = gather(&b.cells, &b.idx, &covered(&b.cells, eye, &only)).len();
        assert!(some > cut.len() && some < b.idx.len(), "one meshed chunk is one hole");
        let far = Some((Vec3::new(1000.0, 80.0, 1000.0), 64.0, 48.0));
        assert_eq!(gather(&b.cells, &b.idx, &covered(&b.cells, far, &all)).len(), b.idx.len(), "an eye far away drops nothing");
    }

    /// Deeper water is darker and bluer-greyer than a shallow sheet over the same sand.
    #[test]
    fn water_is_darker_the_deeper_it_is() {
        let (shallow, deep) = (water_rgb(12, 2), water_rgb(12, 30));
        assert!(deep[0] < shallow[0] && deep[1] < shallow[1], "{shallow:?} vs {deep:?}");
        assert!(shallow[2] > shallow[0], "water is blue");
    }

    /// A real tile at step 8: every index in range, the surface inside the world, cells in index order.
    #[test]
    fn a_generated_tile_meshes() {
        let mut st = store();
        let b = build(&mut st, (0, 0), 8, false);
        assert!(!b.cells.is_empty() && b.verts.len() % 4 == 0);
        assert!(b.idx.iter().all(|&i| (i as usize) < b.verts.len()));
        assert!(b.verts.iter().all(|v| v.pos[1] > -40.0 && v.pos[1] <= 129.0 && v.tint >= 0.0 && v.tint < 16_777_216.0));
        assert!(b.cells.windows(2).all(|w| w[0].i1 <= w[1].i0) && b.cells.last().unwrap().i1 as usize == b.idx.len());
    }

    /// A real chunk replaces the guess: in a tile made after it arrived, and in one that already exists.
    #[test]
    fn an_ingested_chunk_replaces_the_guess() {
        let mut st = store();
        st.ingest((1, 2), vec![land(99); 256]);
        assert_eq!(st.tile((0, 0), false).col((16 + 5) * 64 + 32 + 7)[0].y1, 100);
        st.ingest((3, 3), vec![land(98); 256]);
        assert_eq!(st.tiles[&(0, 0)].col((48 + 1) * 64 + 48 + 1)[0].y1, 99, "a tile that exists is patched in place");
        assert_ne!(st.tiles[&(0, 0)].col(16 * 64 + 7)[0].y1, 100, "chunk (1, 0) was not touched");
    }

    /// A tile file reads back the tile it was made from, and nothing else (another world type, a cut file).
    #[test]
    fn a_tile_file_round_trips() {
        let t = tile_of(|x, z| if x < 5 { vec![sp(74, 77, 18), sp((60 + z % 7) as u8, 70, 3)] } else if z < 9 { vec![sp(40, 64, 9)] } else { land(70) });
        let b = encode(&t, true);
        let u = decode(&b, true).unwrap();
        assert!(u.spans == t.spans && u.start == t.start && !u.trees);
        assert!(decode(&b, false).is_none(), "a vanilla world does not read a Tectonic tile");
        assert!(decode(&b[..b.len() - 3], true).is_none() && decode(&[], true).is_none());
    }

    /// The pre-generation makes the tile nearest the player first, one at a time, counts chunks, and ends when the range is done.
    #[test]
    fn pregen_goes_nearest_first_and_counts_chunks() {
        let (mut st, sh) = (store(), Shared::default());
        sh.cx.store(70, Relaxed); // chunk 70 = tile 17
        let k = pregen_step(&mut st, &sh).unwrap();
        assert_eq!(k, (17, 0));
        assert!(st.tiles[&k].trees && sh.chunks.load(Relaxed) == 16);
        let k2 = pregen_step(&mut st, &sh).unwrap();
        assert_ne!(k2, k);
        assert_eq!(sh.chunks.load(Relaxed), 32);
    }
}
