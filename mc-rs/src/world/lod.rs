//! Far terrain, Voxy style: the view reaches `RADIUS` blocks with the SAME blocks, textures, tints and face shades as the real
//! chunks, not a smooth painted heightfield.
//!
//! Voxy keeps the real 3D block data of far sections, mips it 2x2x2 at a time (`Mipper.mip`) and meshes every level as voxels:
//! a face is drawn wherever a solid voxel touches a see-through one (`RenderDataFactory`, `shouldMeshNonOpaqueBlockFace`). This
//! file does the same for a 128 high world. A worker thread makes the real columns of a 64 x 64 tile (`OverworldGenerator::generate`,
//! plus `populate` for the trees when the tile is near; the same generator, so the same terrain and biome tints as the chunk
//! ring) and keeps each column as a few vertical runs of equal blocks (`Span`: tree crown, trunk, grass, dirt ...; below the
//! last run it is stone). To mesh a tile at a step of 1, 2, 4, 8 or 16 blocks it fills a dense voxel grid of the tile plus one cell
//! of margin (the neighbours' columns), reduces it to cells (`reduce`) and emits a face for every solid / see-through
//! boundary (`build`). So a tree is a real mass of leaf voxels on a trunk (not a pillar from the crown to the lawn), a cliff
//! is a stack of voxels whose height is a multiple of the step (not an arbitrary wall), and an overhang or a crown over a gap
//! stays what it is.
//!
//! Where this differs from Voxy (each against a bug seen in the screenshots):
//! - A cell is `step` x `step` blocks wide but ONE block high (`reduce`), not `step` high. Voxy's 2x2x2 mip is fine at ~2 px a voxel;
//!   here a voxel is 4-8 px, and a hill became a stack of `step` high plates (contour stripes), rivers floated up to `step - 1`
//!   blocks, snow lines were wrong. Now the surface is the median height of the footprint, exact to the block.
//! - The cell looks straight at its footprint (no level-by-level rounding), leaves and logs from a third of it, and a trunk
//!   survives under its crown (else every crown floats).
//! - From step 4 (`CARPET_FROM`, beyond ~320 blocks) there are no tree cells at all: a forest is a leaf carpet 1 to `CARPET_MAX` blocks thick on
//!   its ground (`reduce`), a lone tree is gone. A 4 or 8 block crown was a floating slab or, filled down, a pillar: the fake trees.
//! - Water is the REAL water everywhere: beyond `chunks::WATER_RANGE` a translucent sheet (`Built::sheet`, `Lod::draw_water`, `fs_water`) over a floor whose top
//!   faces are never cut (`Cell::plate`), so there is no opaque colour plate, no pale plate over bare sky, and the sea floor shows through far water as it does near.
//! - The far fog starts at `FOG_START` of the view, not at b1.7.3's quarter, so a far sea and forest keep their colours (the pale haze).
//! - Every tile gets its trees (`TREES` is the whole view) and the pre-generation threads run from the start (`lib.rs` `new_lod`).
//! - Each tile reads its neighbour's columns at its own step, so two tiles at different steps do not agree on the edge and a slit
//!   opened along it, showing the sky (the long pale strips). Tile edges hang a skirt (`skirt_len`) under the lower surface.
//!
//! Blending with the real chunks:
//! - same atlas tiles, same grass / foliage tint, same face shade (top 1.0, Z sides 0.8, X sides 0.6), same light (sky 15), same fog;
//! - the LoD is cut out only where real sections are drawn AND have a mesh (`Lod::update`'s `drawn`, `ChunkManager::drawn`): the old
//!   cut followed the ring's range alone, so a chunk still generating or meshing left a hole to the sky. The part that stays under
//!   the real chunks is `EPS` lower (walls `EPS` inside their voxel) so the real surface always wins the depth test.
//! - a tile's edge hangs a skirt into the ground (`build`): the neighbour tile may be at another step.
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
//! on (it is from the start, `lib.rs` `new_lod`), 2 to 4 `lod-pregen` threads (apart from the worker, so view builds never starve them) make the tiles
//! around the player nearest first, each with its trees, and hand them to the worker, which keeps them on disk (`<world>/lod/`).
//!
//! ponytail: a column keeps the runs down to `DEPTH` blocks under its ground and is stone below (caves and ravines under the surface
//! are not in the LoD, only the openings you could see into from outside are lost); plants (flowers, tall grass) are skipped; water
//! is one translucent sheet at the top of the water (no sides; the real water's tile, drawn by the water pass over the floor, which is drawn under it at every step, darker the deeper); faces are one quad per voxel face (no
//! greedy merge: a step-1 tile of a forest is the most expensive); one meshing worker thread (pre-generation has its own, see above); each pregen tile generates 36 chunks and populates 25 for
//! its 16 (the margin is made again by the neighbour tile: a chunk cache would halve it, at ~50 KB a chunk). UNVERIFIED: nothing measured on a device; the
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
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU8, Ordering::Relaxed};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// How far the far terrain reaches, and the distance the fog completes at (it starts at a quarter of it).
pub const RADIUS: f32 = 768.0;
/// Where the terrain fog starts, as a fraction of `RADIUS` (b1.7.3's is a quarter: it turned a far sea and forest into a pale haze).
pub const FOG_START: f32 = 0.88;
/// The camera's far plane while the LoD is on: past the last tile's far corner.
pub const ZFAR: f32 = 1100.0;
/// Tile edge in blocks (4 chunks).
const TILE: i32 = 64;
/// Tiles nearer than this get their trees (`populate`), the rest are bare ground. The whole view: a bare far forest was an empty lawn.
const TREES: f32 = RADIUS + 64.0;
/// The LoD under the real chunks sits this much lower (and its walls this much inside), so the real surface always wins.
const EPS: f32 = 0.04;
/// `Vertex::light` of a water sheet: `1000 +` the water tile (205), like any far quad (`light >= 1000` carries its tile). `fs_water` draws a
/// sheet only beyond `chunks::WATER_RANGE` (the real water is drawn inside it), tiled once per block, translucent like the real water.
pub const SHEET_LIGHT: f32 = 1000.0 + 205.0;
/// How far inside the real chunks' range a land voxel is cut out (the eye is quantised to 2 blocks: 1.74 at most; a chunk that is in
/// range but not meshed yet is `ChunkManager::drawn`'s business, no longer the slack's). Water sheets are not cut by
/// range but by `ChunkManager::shown`, floor plates are never cut, see `covered`.
const LAND_SLACK: f32 = 3.0;
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
    // Trees are real voxels (1 block cells) up to 200: at step 2 a tree is a 2 wide trunk under 2 x 2 cells of leaves, twice its size.
    // From step 4 `reduce` has no trees at all but a leaf carpet over dense forest (see `CARPET_FROM`).
    // These used to be 280 / 420 / 580. Measured on seed 12345 (a quadrant of the ring, every tile, no culling): steps 1 and 2 held
    // 82% of the ~2 M far-view triangles (a step 1 tile 16k, step 2 9k, step 4 1.9k, step 8 0.5k), which a phone GPU has to bin every frame.
    if dist < 200.0 {
        1
    } else if dist < 320.0 {
        2
    } else if dist < 480.0 {
        4
    } else {
        8
    }
}

/// From this step on a forest is a low leaf carpet over its ground (`reduce`), not crown cells on stilts: a crown of 4 x 4 or 8 x 8
/// blocks over a gap was a floating slab, and filled down to the lawn it was a pillar (both: the fake trees).
const CARPET_FROM: usize = 4;
/// The carpet is this many blocks thick at most (a forest edge is a short leaf wall, never a tower).
const CARPET_MAX: i32 = 3;

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
    /// The pre-generation's progress, shared with its threads (`Shared::pg`).
    sh: Arc<Shared>,
}

const MAGIC: [u8; 2] = *b"L6"; // L3 tiles were made with the b1.7.3 climate, L4 before the terrain followed it, L5 before the rivers: another terrain, so they are made again

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

/// What the render thread, the worker and the pre-generation threads share: the switch, its count, and the chunk the player is in.
#[derive(Default)]
struct Shared {
    on: AtomicBool,
    /// The `Lod` is gone: the pre-generation threads end.
    quit: AtomicBool,
    chunks: AtomicU32,
    cx: AtomicI32,
    cz: AtomicI32,
    /// Debug colouring of the meshes (the pause menu's button, `Lod::cycle_debug`): 0 off, 1 by step, 2 by kind of face, 3 by tile.
    dbg: AtomicU8,
    pg: Mutex<Progress>,
}

/// The pre-generation's tiles: finished (real, with trees; kept by the worker) and taken by a thread that is making them.
#[derive(Default)]
struct Progress {
    done: HashSet<(i32, i32)>,
    busy: HashSet<(i32, i32)>,
}

impl Shared {
    fn pg(&self) -> MutexGuard<'_, Progress> {
        self.pg.lock().unwrap_or_else(|e| e.into_inner()) // a panicked thread leaves plain sets behind: still usable
    }
}

impl Store {
    fn new(g: OverworldGenerator, cm: WorldChunkManager, dir: Option<PathBuf>, tectonic: bool, sh: Arc<Shared>) -> Self {
        Store { g, cm, tiles: HashMap::new(), real: HashMap::new(), dir, tectonic, dirty: HashSet::new(), sh }
    }

    /// Tile `k` is real (with trees) now or was loaded from its file: counted by the pre-generation, never asked of it again.
    fn mark_done(&self, k: (i32, i32)) {
        let mut p = self.sh.pg();
        p.done.insert(k);
        p.busy.remove(&k);
        self.sh.chunks.store(p.done.len() as u32 * 16, Relaxed);
    }

    /// A tile a pre-generation thread made (or read from its file, `fresh` = false): kept unless a view build made it first.
    fn accept(&mut self, k: (i32, i32), mut t: Tile0, fresh: bool) {
        self.mark_done(k);
        if self.tiles.get(&k).is_some_and(|o| o.trees) {
            return;
        }
        let real = self.overlay(&mut t, k);
        if fresh || real {
            self.dirty.insert(k);
        }
        self.tiles.insert(k, t);
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
                None => make_tile(&mut self.g, &mut self.cm, k, trees),
            };
            let real = self.overlay(&mut t, k);
            if t.trees {
                self.mark_done(k);
                if fresh || real {
                    self.dirty.insert(k);
                }
            }
            self.tiles.insert(k, t);
        }
        &self.tiles[&k]
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

/// Generate the tile's 4 x 4 chunks. With trees the grid starts one chunk to the -x / -z side (a chunk's populate reaches into
/// +x / +z, so the tile's first row and column get their trees from there) and every chunk but the last row is populated
/// on a 2 x 2 block, in raster order, as the chunk manager does it.
fn make_tile(g: &mut OverworldGenerator, cm: &mut WorldChunkManager, k: (i32, i32), trees: bool) -> Tile0 {
    let m = trees as usize;
    let gn = 4 + 2 * m;
    let (lx, lz) = (k.0 * 4 - m as i32, k.1 * 4 - m as i32);
    let mut b: Vec<Vec<u8>> = Vec::with_capacity(gn * gn);
    for i in 0..gn {
        for j in 0..gn {
            b.push(g.generate(lx + i as i32, lz + j as i32, cm));
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
                g.populate(&mut r, x, z, cm);
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
            let tints = biome_tints(cm, k.0 * 4 + ci as i32, k.1 * 4 + cj as i32);
            for x in 0..16 {
                for z in 0..16 {
                    cols[(ci * 16 + x) * 64 + cj * 16 + z] = column(&b[q], &d[q], &tints, x, z);
                }
            }
        }
    }
    Tile0::from_cols(&cols, trees)
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
            // From step 2 a snow layer is no voxel (as a full block it was a white cube on every crown and hill): the ground under it
            // is the snow block instead, leaves stay green.
            let snow = s > 1 && col[0].id == 78;
            for (n, sp) in col.iter().enumerate() {
                if snow && n == 0 {
                    continue;
                }
                let v = sp.id as u32 | (sp.meta as u32 & 15) << 8 | pal.get(sp.tint) << 16;
                for y in sp.y0 as usize..sp.y1 as usize {
                    g[(y * w + rz) * w + rx] = v;
                }
                if snow && n == 1 && !matches!(sp.id, 9 | 17 | 18) {
                    g[((sp.y1 as usize - 1) * w + rz) * w + rx] = 80;
                }
            }
        }
    }
    g
}

/// The voxels of the columns under one cell, at one height, by kind (`reduce`).
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Land,
    Water,
    Leaf,
    Wood,
}

fn kind(id: u8) -> Kind {
    match id {
        9 => Kind::Water,
        18 => Kind::Leaf,
        17 => Kind::Wood,
        _ => Kind::Land,
    }
}

/// The distinct voxels of a footprint's ground at one height (at most 6, enough for a few block types and tints), with counts.
struct Tally {
    v: [u32; 6],
    n: [u16; 6],
    len: usize,
}

impl Tally {
    fn new() -> Self {
        Tally { v: [0; 6], n: [0; 6], len: 0 }
    }

    fn add(&mut self, v: u32) {
        for q in 0..self.len {
            if self.v[q] == v {
                self.n[q] += 1;
                return;
            }
        }
        if self.len < 6 {
            (self.v[self.len], self.n[self.len]) = (v, 1);
            self.len += 1;
        }
    }

    /// The most frequent voxel (the first of equals), 0 when empty.
    fn best(&self) -> u32 {
        let mut b = 0;
        for q in 1..self.len {
            if self.n[q] > self.n[b] {
                b = q;
            }
        }
        if self.len == 0 {
            0
        } else {
            self.v[b]
        }
    }
}

/// The level-0 grid `g0` (`w0` x `HEIGHT` x `w0`, index `(y * w0 + z) * w0 + x`) as cells of `s` x `s` columns that stay ONE block
/// high: `w0 / s` x `HEIGHT` x `w0 / s`, same index layout. `Mipper.mip` halves y as well as x and z, so a far hill came out as a stack
/// of `s` high plates (the contour stripes in the screenshots), a river sheet floated up to `s - 1` blocks, and each mip level rounded
/// the level before it. Here every cell looks straight at its `s * s` columns, one height at a time (a heightmap's median, no
/// compounding):
/// - land / water: the cell is solid when at least half of its columns are, of the kind that has more of them (ties: land), and of
///   the most frequent block among them; so the surface sits where half of the footprint is below it, within a block of the truth;
/// - leaves and logs: kept from half of the footprint like land (from a third every crown grew by half a cell on each side: the oversized trees);
/// - mixed heights (a tree on a hill, a shore): the biggest of the kinds, when together they fill half of it;
/// - a trunk is one column, so no cell gets half of it: at step 2 a cell whose footprint holds a log, directly under a crown or trunk
///   cell of its own column, keeps one (2 wide); from step 4 there is none (a 4 to 8 wide trunk was worse than a crown a few blocks up).
/// - from `CARPET_FROM` on no leaf or log is a cell: a footprint where 3 of 5 columns hold leaves gets a carpet of its leaves on its ground,
///   as thick as the crowns are tall over it (`CARPET_MAX` at most); a sparse tree is gone, as it is at 2 px a tree in Voxy.
/// `s == 1` is the grid as it is.
fn reduce(g0: Vec<u32>, w0: usize, s: usize) -> (Vec<u32>, usize) {
    if s == 1 {
        return (g0, w0);
    }
    let w = w0 / s;
    let t = (s * s) as u32;
    let carpet = s >= CARPET_FROM;
    let mut out = vec![0u32; w * HEIGHT * w];
    let (mut col, mut wood_at) = ([0u32; HEIGHT], [0u32; HEIGHT]);
    for j in 0..w {
        for i in 0..w {
            // Carpet: the columns of this footprint that hold leaves, the sum of their crown tops and one leaf voxel (tint) to paint with.
            let (mut leaf_cols, mut crown_sum, mut crown_leaf) = (0i32, 0i32, 0u32);
            if carpet {
                for dz in 0..s {
                    for dx in 0..s {
                        let at = |y: usize| (y * w0 + j * s + dz) * w0 + i * s + dx;
                        if let Some(y) = (0..HEIGHT).rev().find(|&y| vid(g0[at(y)]) == 18) {
                            leaf_cols += 1;
                            crown_sum += y as i32;
                            if crown_leaf == 0 {
                                crown_leaf = g0[at(y)];
                            }
                        }
                    }
                }
            }
            for y in 0..HEIGHT {
                let (mut land, mut wat, mut leaf, mut wood) = (0u32, 0u32, 0u32, 0u32);
                let (mut tl, mut rep_water, mut rep_leaf, mut rep_wood) = (Tally::new(), 0u32, 0u32, 0u32);
                for dz in 0..s {
                    for dx in 0..s {
                        let v = g0[(y * w0 + j * s + dz) * w0 + i * s + dx];
                        let id = vid(v);
                        if id == 0 {
                            continue;
                        }
                        match kind(id) {
                            Kind::Land => {
                                land += 1;
                                tl.add(v);
                            }
                            Kind::Water => {
                                wat += 1;
                                rep_water = v;
                            }
                            Kind::Leaf | Kind::Wood if carpet => {} // painted on the ground below, not meshed
                            Kind::Leaf => {
                                leaf += 1;
                                if rep_leaf == 0 {
                                    rep_leaf = v;
                                }
                            }
                            Kind::Wood => {
                                wood += 1;
                                if rep_wood == 0 {
                                    rep_wood = v;
                                }
                            }
                        }
                    }
                }
                let (solid, all) = (land + wat, land + wat + leaf + wood);
                col[y] = if solid > 0 && solid * 2 >= t {
                    if land >= wat {
                        tl.best()
                    } else {
                        rep_water
                    }
                } else if leaf + wood > 0 && (leaf + wood) * 2 >= t {
                    if wood > leaf {
                        rep_wood
                    } else {
                        rep_leaf
                    }
                } else if all > 0 && all * 2 >= t {
                    let m = land.max(wat).max(leaf).max(wood);
                    if land == m {
                        tl.best()
                    } else if wat == m {
                        rep_water
                    } else if leaf == m {
                        rep_leaf
                    } else {
                        rep_wood
                    }
                } else {
                    0
                };
                wood_at[y] = rep_wood;
            }
            // Dense forest (3 of 5 columns hold leaves): a leaf carpet over the highest ground, as thick as the crowns are tall over it.
            if carpet && leaf_cols * 5 >= t as i32 * 3 {
                if let Some(gt) = (0..HEIGHT).rev().find(|&y| col[y] != 0) {
                    if kind(vid(col[gt])) == Kind::Land {
                        let h = (crown_sum / leaf_cols - gt as i32).clamp(1, CARPET_MAX) as usize;
                        for y in gt + 1..(gt + 1 + h).min(HEIGHT) {
                            col[y] = crown_leaf;
                        }
                    }
                }
            }
            let mut hang = false; // a crown or trunk voxel is right above
            for y in (0..HEIGHT).rev() {
                if s <= 2 && col[y] == 0 && hang && wood_at[y] != 0 {
                    col[y] = wood_at[y];
                }
                hang = matches!(vid(col[y]), 17 | 18);
            }
            for y in 0..HEIGHT {
                out[(y * w + j) * w + i] = col[y];
            }
        }
    }
    (out, w)
}

/// One voxel's place in the view: where it is (its section decides if the real chunks cover it) and its slice of the indices.
struct Cell {
    x: i32,
    y: i32,
    z: i32,
    /// Cut by what the real chunks draw (`shown`) rather than by range: a water sheet (`y` = its water voxel).
    water: bool,
    /// The floor plate under a sheet (`y` = the floor's voxel): never cut, see `covered`.
    plate: bool,
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
    /// The water sheets' indices (same vertices): drawn by the water pass, never cut (`Lod::draw_water`).
    sheet: Vec<u32>,
    cells: Vec<Cell>,
    /// The debug mode it was made in; a tile made in another is dropped.
    dbg: u8,
}

/// What a quad is, for the debug colours.
const K_TOP: u8 = 0;
const K_BOTTOM: u8 = 1;
const K_WALL: u8 = 2;
const K_SKIRT: u8 = 3;
const K_WATER: u8 = 4;
const K_FLOOR: u8 = 5;

struct Quads {
    verts: Vec<Vertex>,
    idx: Vec<u32>,
    /// The indices of the quads of kind `K_WATER` (`Built::sheet`).
    sheet: Vec<u32>,
    /// Debug mode (0 off), the tile's step and checker parity, and the kind of the quad being emitted.
    dbg: u8,
    step: i32,
    par: bool,
    kind: u8,
}

/// The colour of a quad in debug mode `dbg`: 1 by step (green 1, yellow 2, orange 4, red 8), 2 by kind (top white, bottom grey, wall blue,
/// skirt pink, water cyan), 3 tiles alternate red / blue (skirts pink, water cyan) so every tile edge shows.
fn debug_rgb(dbg: u8, step: i32, par: bool, kind: u8) -> [u8; 3] {
    match (dbg, kind) {
        (_, K_SKIRT) => [255, 40, 220],
        (_, K_WATER) => [0, 230, 230],
        (_, K_FLOOR) => [255, 170, 0],
        (1, _) => match step {
            1 => [40, 220, 60],
            2 => [240, 220, 40],
            4 => [250, 140, 30],
            _ => [230, 40, 40],
        },
        (2, K_TOP) => [235, 235, 235],
        (2, K_BOTTOM) => [110, 110, 110],
        (2, _) => [60, 110, 255],
        _ => {
            if par {
                [255, 110, 110]
            } else {
                [110, 150, 255]
            }
        }
    }
}

impl Quads {
    /// One quad, corners counter-clockwise seen from the front (the pipeline culls back faces).
    fn quad(&mut self, p: [[f32; 3]; 4], uv: [[f32; 2]; 4], light: f32, tint: f32) {
        // Debug: every face is the flat white texel, tinted by what it is (the shader multiplies, as for the water sheet).
        let (uv, light, tint) = if self.dbg == 0 {
            (uv, light, tint)
        } else {
            let t = atlas::atlas_uv(80, 0.0, 0.0);
            // A debug sheet is the opaque snow tile (1066) tinted cyan: `fs_water` multiplies a sheet's tint.
            ([[t.0, t.1]; 4], if self.kind == K_WATER { 1066.0 } else { 1.0 }, pack(debug_rgb(self.dbg, self.step, self.par, self.kind)))
        };
        let base = self.verts.len() as u32;
        for k in 0..4 {
            self.verts.push(Vertex { pos: p[k], uv: uv[k], light, lm: lm(15, 0), tint });
        }
        let ix = [base, base + 1, base + 2, base, base + 2, base + 3];
        if self.kind == K_WATER {
            self.sheet.extend_from_slice(&ix);
        } else {
            self.idx.extend_from_slice(&ix);
        }
    }

    /// A water sheet over the square at (x0, z0), side `sf`, at height `y`: the water tile, tiled once per block (`fs_water` wraps the uv).
    fn sheet_quad(&mut self, x0: f32, z0: f32, sf: f32, y: f32) {
        let (x1, z1) = (x0 + sf, z0 + sf);
        self.quad([[x0, y, z1], [x1, y, z1], [x1, y, z0], [x0, y, z0]], [[0.0, 0.0], [sf, 0.0], [sf, sf], [0.0, sf]], SHEET_LIGHT, pack(PLAIN));
    }

    /// A quad tiled once per BLOCK: `uv` is in blocks (v up) and `light` carries the terrain tile (`fs_main` wraps it, light >= 1000).
    #[allow(clippy::too_many_arguments)]
    fn quad_t(&mut self, p: [[f32; 3]; 4], t: u8, ur: f32, vb: f32, vt: f32, tint: f32) {
        self.quad(p, [[0.0, vb], [ur, vb], [ur, vt], [0.0, vt]], 1000.0 + t as f32, tint);
    }

    /// A face looking up (`top`) or down at height `y`, over the square at (x0, z0) of side `sf`, with the block's tile.
    #[allow(clippy::too_many_arguments)]
    fn flat(&mut self, top: bool, x0: f32, z0: f32, sf: f32, y: f32, id: u8, meta: u8, tint: f32) {
        self.kind = if top { K_TOP } else { K_BOTTOM };
        let (x1, z1) = (x0 + sf, z0 + sf);
        let p = if top { [[x0, y, z1], [x1, y, z1], [x1, y, z0], [x0, y, z0]] } else { [[x0, y, z0], [x1, y, z0], [x1, y, z1], [x0, y, z1]] };
        if let Some(t) = atlas::terrain_tile(id, meta, if top { 1 } else { 0 }) {
            return self.quad_t(p, t, sf, 0.0, sf, tint);
        }
        let uv = |u: f32, v: f32| face_uv(id, meta, if top { 1 } else { 0 }, u, v);
        self.quad(p, [uv(0.0, 0.0), uv(1.0, 0.0), uv(1.0, 1.0), uv(0.0, 1.0)], if top { 1.0 } else { 0.5 }, tint);
    }

    /// A wall of the voxel at (x0, z0) facing `side`, from `yb` to `yt`, exactly on the voxel's boundary (an inset leaves a slit
    /// between the wall's foot and the lower neighbour's top face, which starts on the boundary: the gap lines on every step; the tie with
    /// the real chunk's coplanar wall is broken by the LoD pipeline's depth bias instead); `v0` is where the tile starts (a
    /// 1/8 high snow layer shows the top eighth). The tile repeats every block, across and up.
    #[allow(clippy::too_many_arguments)]
    fn wall(&mut self, side: Side, x0: f32, z0: f32, sf: f32, yt: f32, yb: f32, v0: f32, id: u8, meta: u8, tint: f32) {
        let (_, tile, shade, corners) = side;
        if self.kind != K_SKIRT {
            self.kind = K_WALL;
        }
        let p = corners.map(|cn| [x0 + cn[0] * sf, if cn[1] > 0.5 { yt } else { yb }, z0 + cn[2] * sf]);
        if let Some(t) = atlas::terrain_tile(id, meta, tile) {
            return self.quad_t(p, t, sf, v0, v0 + (yt - yb), tint);
        }
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

/// How far (blocks) a tile's edge hangs a skirt under the lower of its own surface and its neighbour's, see `build`.
fn skirt_len(step: i32) -> i32 {
    (4 * step).clamp(8, 32)
}

/// The mesh of tile `key` with a cell every `step` blocks (`step` x `step` blocks wide, one block high: see `reduce`).
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
    let g0 = fill(store, key, s, &mut pal);
    let (g, w) = reduce(g0, TILE as usize + 2 * s, s);
    // Now `w == n + 2` (a cell of margin on each side); the tile's cells are 1..=n.
    let h = HEIGHT;
    let at = |i: usize, k: i32, j: usize| -> u32 {
        if k < 0 {
            1 // below the world: solid, no faces down there
        } else if k as usize >= h {
            0
        } else {
            g[(k as usize * w + j) * w + i]
        }
    };
    // The highest GROUND block of each column, not air, water, log or leaf (-1: none): the skirts hang from it (from a crown they hung
    // stone-grey curtains under every tree on a tile edge).
    let tops: Vec<i32> = (0..w * w)
        .map(|c| {
            let (i, j) = (c % w, c / w);
            (0..h as i32).rev().find(|&k| !matches!(vid(g[(k as usize * w + j) * w + i]), 0 | 9 | 17 | 18)).unwrap_or(-1)
        })
        .collect();
    let sf = s as f32;
    let skirt = skirt_len(step);
    let dbg = store.sh.dbg.load(Relaxed);
    let mut q = Quads { verts: Vec::new(), idx: Vec::new(), sheet: Vec::new(), dbg, step, par: (key.0 + key.1) & 1 != 0, kind: K_TOP };
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
                let y0 = k as f32;
                let i0 = q.idx.len() as u32;
                let mut plate_end: Option<u32> = None;
                if id == 9 {
                    // Water: one translucent sheet on the top voxel (the water pass draws it, over the floor that is drawn under it).
                    let up = vid(at(gi, ki + 1, gj));
                    if up != 9 && !opq(up, s) {
                        q.kind = K_WATER;
                        q.sheet_quad(x0, z0, sf, y0 + 1.0 - 1.0 / 9.0); // exactly the real water's top: `fs_water` splits the two per pixel, so any height gap is a ring
                    }
                } else {
                    let meta = ((v >> 8) & 15) as u8;
                    let tint = pack(pal.v[(v >> 16) as usize]);
                    let hgt = if id == 78 && s == 1 { 0.125 } else { 1.0 };
                    let nb = |di: i32, dk: i32, dj: i32| vid(at((gi as i32 + di) as usize, ki + dk, (gj as i32 + dj) as usize));
                    // A face is hidden by an opaque neighbour, by water (its sheet is drawn instead) and by the same block (glass, ice).
                    let hide = |o: u8| o != 0 && (o == 9 || opq(o, s) || o == id);
                    // Water above does not hide a top face any more: the sheet is translucent and the floor shows through it, at every step.
                    let upo = nb(0, 1, 0);
                    let (up_hidden, down_hidden) = (upo != 9 && hide(upo), hide(nb(0, -1, 0)));
                    if !up_hidden {
                        // A floor under water is lit by what is left of the sky light under its water (3 a block, `Block.lightOpacity`).
                        let mut tint_up = tint;
                        if upo == 9 {
                            let mut d = 1;
                            while vid(at(gi, ki + 1 + d, gj)) == 9 {
                                d += 1;
                            }
                            let f = brightness((15 - 3 * d).max(0) as u8).powf(1.0 / 2.2);
                            tint_up = pack(pal.v[(v >> 16) as usize].map(|b| (b as f32 * f).round() as u8));
                            plate_end = Some(q.idx.len() as u32 + 6); // the top face is the 6 indices that follow
                        }
                        q.flat(true, x0, z0, sf, y0 + hgt - EPS, id, meta, tint_up);
                    }
                    if !down_hidden {
                        q.flat(false, x0, z0, sf, y0 + EPS, id, meta, if id == 2 { pack(PLAIN) } else { tint });
                    }
                    // A wall is only shortened by EPS where a top / bottom face is: between two stacked voxels the walls meet, otherwise
                    // a 2 * EPS slit shows the sky through every step of a cliff (white lines, blocks that look cut apart).
                    let wt = y0 + hgt - if up_hidden { 0.0 } else { EPS };
                    for side in SIDES {
                        let ((di, dj), ..) = side;
                        let o = nb(di, 0, dj);
                        // Water beside a land voxel hides its wall only under the water's surface: where the water's top voxel is, the
                        // land stands 1/9 above the sheet, and culling that strip left a white line along every shore.
                        let up = nb(di, 1, dj);
                        let hidden = if o == 9 { up == 9 || opq(up, s) } else { hide(o) };
                        let edge = (di == -1 && gi == 1) || (di == 1 && gi == n) || (dj == -1 && gj == 1) || (dj == 1 && gj == n);
                        if hidden {
                            // Tile edge skirt. Each tile reads its neighbour's columns at ITS OWN step, so where the neighbour is
                            // drawn at another step the two surfaces differ and neither tile has a wall there: a slit along the whole
                            // edge, through which the sky shows (the long pale strips of the screenshots; measured on a profile
                            // of hills: 4 -> 8 left every exposed block of the edge open). One quad hangs from just under the lower
                            // of the two surfaces, on the tile's edge plane, where it is inside the ground (hidden) unless a slit opens.
                            let (kt, mt) = (tops[gj * w + gi], tops[(gj as i32 + dj) as usize * w + (gi as i32 + di) as usize]);
                            // Under water too: the floor is drawn now (the sheet is translucent), so a slit between two steps of the sea floor shows.
                            if edge && kt >= 0 && mt >= 0 && ki == kt.min(mt) {
                                // A snow layer is 1/8 high: the skirt hangs from ITS top (else a grey wall stood 7/8 above the snow) and shows snow.
                                // Only ground gets one: under a log or leaves it was a grey curtain of stone beside every tree on a tile edge.
                                let (sid, smeta, stint) = match id {
                                    78 => (80, 0, pack(PLAIN)),
                                    _ if opaque(id) && !matches!(id, 17 | 18) => (id, meta, tint),
                                    _ => continue,
                                };
                                let top = y0 + hgt;
                                q.kind = K_SKIRT;
                                q.wall(side, x0, z0, sf, top - EPS, (top - skirt as f32).max(0.0), 0.0, sid, smeta, stint);
                                q.kind = K_TOP;
                            }
                            continue;
                        }
                        // The foot of the wall meets the top face of the lower neighbour (EPS under its voxel's top), or the water's sheet
                        // (1/9 and EPS under it), or the bottom face of this voxel; where the wall goes on into the voxel under it, it is flush.
                        let below = nb(di, -1, dj);
                        let wb = if !down_hidden {
                            y0 + EPS
                        } else if below == 9 {
                            y0 - 1.0 / 9.0 - EPS
                        } else if opq(below, s) {
                            y0 - EPS
                        } else {
                            y0
                        };
                        q.wall(side, x0, z0, sf, wt, wb, 1.0 - hgt, id, meta, tint);
                    }
                }
                let i1 = q.idx.len() as u32;
                let (cx, cy, cz, wet) = (x0 as i32, k as i32, z0 as i32, id == 9);
                // The top face of a floor under water is its own cell, never cut (`covered`): where the real floor is drawn it hides it
                // (EPS lower), where it is not it IS the floor, and no real water is left over bare sky.
                let mut start = i0;
                if let Some(pe) = plate_end {
                    cells.push(Cell { x: cx, y: cy, z: cz, water: false, plate: true, i0, i1: pe });
                    start = pe;
                }
                if i1 > start {
                    match cells.last_mut() {
                        Some(c) if !c.plate && (c.x, c.z, c.y >> 4, c.water) == (cx, cz, cy >> 4, wet) && c.i1 == start => c.i1 = i1,
                        _ => cells.push(Cell { x: cx, y: cy, z: cz, water: wet, plate: false, i0: start, i1 }),
                    }
                }
            }
        }
    }
    Built { key, step, trees, verts: q.verts, idx: q.idx, sheet: q.sheet, cells, dbg }
}

/// Which cells the real chunks cover around `eye` = (position, range, vertical range): the same `in_range` test as
/// `ChunkManager::update_visible` on the section of the cell, made smaller by the slack, and only where that chunk has a mesh
/// (`drawn`): a chunk that is in range but still generating or meshing leaves the LoD in place instead of a hole.
fn covered(cells: &[Cell], eye: Option<(Vec3, f32, f32)>, drawn: &dyn Fn(i32, i32) -> bool, _shown: &dyn Fn(i32, i32, i32) -> bool) -> Vec<bool> {
    let Some((e, rh, rv)) = eye else { return vec![false; cells.len()] };
    cells
        .iter()
        .map(|c| {
            // A floor plate is never cut: it sits EPS under the floor's top, so where the real floor is drawn it loses the depth test
            // (EPS 0.04 against ~0.005 of depth resolution at the 64 blocks of the ring), and where it is not it IS the floor. Cutting it
            // by `shown` / range was the pale plate (real water over bare sky) that came back after every fix: any frame where the real
            // floor was missing for a reason `shown` did not know left a hole. ponytail: drawn under the ring too (early-z drops it).
            if c.plate {
                return false;
            }
            if !drawn(c.x >> 4, c.z >> 4) {
                return false;
            }
            if c.water {
                // A water sheet is never cut: where the real water is drawn the shader drops the sheet per pixel (`SHEET_LIGHT`), at the
                // same distance at which it drops the real water, so the two are exact complements whatever the sections do.
                return false;
            }
            let min = Vec3::new((c.x >> 4) as f32 * 16.0, (c.y >> 4) as f32 * 16.0 - 1.0, (c.z >> 4) as f32 * 16.0);
            in_range(Shape::Sphere, e, min, min + Vec3::new(16.0, 18.0, 16.0), rh - LAND_SLACK, rv - 2.0)
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
    /// The water sheets' indices (never cut), drawn by `Lod::draw_water`.
    sheet: Option<wgpu::Buffer>,
    sheet_count: u32,
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
    fn cut(&mut self, device: &wgpu::Device, eye: Option<((i32, i32, i32), f32, f32)>, gen: u32, drawn: &dyn Fn(i32, i32) -> bool, shown: &dyn Fn(i32, i32, i32) -> bool) {
        let cov = covered(&self.cells, eye.map(|(q, rh, rv)| (quant_eye(q), rh, rv)), drawn, shown);
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
    /// A tile a pre-generation thread made with its trees: (tile, columns, new = not read from its file).
    Made((i32, i32), Box<Tile0>, bool),
}

/// Voxy's `DistanceGraph.findWork`: the nearest tile (a 4 x 4 batch of chunks) within `RADIUS` of the player that is not real yet and
/// that no other thread has taken; it is marked taken. None = nothing is left to take.
fn claim(sh: &Shared) -> Option<(i32, i32)> {
    let (tx, tz) = (sh.cx.load(Relaxed).div_euclid(4), sh.cz.load(Relaxed).div_euclid(4));
    let n = (RADIUS / TILE as f32) as i32 + 1;
    let mut p = sh.pg();
    let mut best: Option<(i32, (i32, i32))> = None;
    for x in tx - n..=tx + n {
        for z in tz - n..=tz + n {
            let d = (x - tx).pow(2) + (z - tz).pow(2);
            if d <= n * n && !p.done.contains(&(x, z)) && !p.busy.contains(&(x, z)) && best.map_or(true, |b| d < b.0) {
                best = Some((d, (x, z)));
            }
        }
    }
    let (_, k) = best?;
    p.busy.insert(k);
    Some(k)
}

fn generator(seed: i64, tectonic: bool) -> OverworldGenerator {
    if tectonic {
        OverworldGenerator::world_tectonic(seed)
    } else {
        OverworldGenerator::world(seed)
    }
}

/// One pre-generation thread (Voxy World Gen's `maxActiveTasks`: several at once, apart from the thread that meshes the view, so a
/// long queue of view builds never starves it). While the switch is on it takes the nearest free tile, reads its file or makes it with
/// its trees, and hands it to the worker; when nothing is left it waits for the player to move.
/// Its own generator is made at its first tile, so a world that never pre-generates pays nothing.
fn pregen_thread(sh: Arc<Shared>, job: Sender<Msg>, seed: i64, tectonic: bool, dir: Option<PathBuf>) {
    crate::world::nice(19); // it runs all the time: only on what the render thread and the chunk workers leave free
    let mut gen: Option<(OverworldGenerator, WorldChunkManager)> = None;
    while !sh.quit.load(Relaxed) {
        let k = if sh.on.load(Relaxed) { claim(&sh) } else { None };
        let Some(k) = k else {
            // Nothing is left: wait, the player moves on and the next ring of tiles is free to take (it used to switch itself off here).
            std::thread::sleep(Duration::from_millis(100));
            continue;
        };
        let file = dir.as_ref().and_then(|d| decode(&std::fs::read(tile_path(d, k)).ok()?, tectonic)).filter(|t| t.trees);
        let fresh = file.is_none();
        let t = file.unwrap_or_else(|| {
            let (g, cm) = gen.get_or_insert_with(|| {
                let g = generator(seed, tectonic);
                let cm = g.manager();
                (g, cm)
            });
            make_tile(g, cm, k, true)
        });
        if job.send(Msg::Made(k, Box::new(t), fresh)).is_err() {
            break;
        }
    }
}

/// Threads for the pre-generation: a quarter of the cores, 1 to 2. It runs all the time, and 2 to 4 busy threads on top of the chunk
/// workers, the light thread and the mesher kept every core of a phone at 100% (heat, then the clock drops and the frame with it).
/// The far view is only as complete as it is quick: fewer threads fill it more slowly, never at the cost of the frame.
fn pregen_threads() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get() / 4).clamp(1, 2)
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
        for i in 0..pregen_threads() {
            let (sh, job, dir) = (sh.clone(), job.clone(), dir.clone());
            std::thread::Builder::new().name(format!("lod-pregen{i}")).spawn(move || pregen_thread(sh, job, seed, tectonic, dir)).expect("lod pregen thread");
        }
        std::thread::Builder::new()
            .name("lod".into())
            .spawn(move || {
                crate::world::nice(10); // meshing the far view must never take a core from the frame
                if let Some(d) = &dir {
                    let _ = std::fs::create_dir_all(d);
                }
                let g = generator(seed, tectonic);
                let cm = g.manager();
                let mut store = Store::new(g, cm, dir, tectonic, shared);
                // Ends when the `Lod` (and so `job`) is dropped.
                while let Ok(m) = jobs.recv() {
                    match m {
                        Msg::Ingest(k, cols) => store.ingest(k, cols),
                        Msg::Build(tx, tz, step, trees) => {
                            let b = build(&mut store, (tx, tz), step, trees);
                            store.flush();
                            store.prune((tx, tz));
                            if done.send(b).is_err() {
                                break;
                            }
                        }
                        Msg::Made(k, t, fresh) => {
                            store.accept(k, *t, fresh);
                            store.flush();
                            store.prune((store.sh.cx.load(Relaxed).div_euclid(4), store.sh.cz.load(Relaxed).div_euclid(4)));
                            let _ = made_tx.send(k);
                        }
                    }
                }
                store.flush();
            })
            .expect("lod thread");
        Lod { tiles: HashMap::new(), pending: HashSet::new(), dirty: HashMap::new(), job, rx, made, sh, visible: Vec::new() }
    }

    /// The pause menu's button: start or stop pre-generating the tiles around the player.
    pub fn toggle_pregen(&mut self) {
        self.sh.on.fetch_xor(true, Relaxed);
    }

    /// The pause menu's debug button: the next colouring (off, step, kind of face, tile), every tile is meshed again.
    pub fn cycle_debug(&mut self) {
        let m = (self.sh.dbg.load(Relaxed) + 1) % 5;
        self.sh.dbg.store(m, Relaxed);
        self.tiles.clear();
        self.dirty.clear();
        self.visible.clear();
    }

    /// The debug mode: 0 off, 1 step, 2 kind, 3 tile, 4 the LoD is not drawn at all (tells a LoD artifact from a real-chunk one).
    pub fn debug(&self) -> u8 {
        self.sh.dbg.load(Relaxed)
    }

    /// The line shown at the top of the screen while a debug mode is on (empty when off): which mode, and what its colours mean.
    pub fn debug_title(&self) -> &'static str {
        match self.sh.dbg.load(Relaxed) {
            1 => "LoD debug 1 STEP: g=1 y=2 o=4 r=8",
            2 => "LoD debug 2 FACE: top wht wall blu skirt pnk",
            3 => "LoD debug 3 TILE: red/blue, skirt pnk",
            4 => "LoD debug 4: LoD HIDDEN",
            _ => "",
        }
    }

    /// (is it on, chunks generated so far: 16 for every finished tile).
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
    pub fn update(&mut self, device: &wgpu::Device, eye: Vec3, rh: f32, rv: f32, gen: u32, drawn: impl Fn(i32, i32) -> bool, shown: impl Fn(i32, i32, i32) -> bool, in_frustum: impl Fn(Vec3, Vec3) -> bool) {
        while let Ok(b) = self.rx.try_recv() {
            self.pending.remove(&b.key);
            if b.dbg != self.sh.dbg.load(Relaxed) {
                continue; // made before the debug mode changed: asked for again
            }
            let vbuf = create_vertex_buffer(device, &b.verts);
            let sheet = (!b.sheet.is_empty()).then(|| create_index_buffer(device, &b.sheet));
            let t = Tile { step: b.step, trees: b.trees, vbuf, ibuf: None, index_count: 0, sheet, sheet_count: b.sheet.len() as u32, cells: b.cells, all: b.idx, hole: None, gen: 0, cov: Vec::new(), stale: true };
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
        // A tile that is not there at all is a hole to the sky, so it goes before one that only changes; but not at any price: a tree
        // upgrade closer than `UPGRADE_LAG` blocks goes before a missing tile that far (else no tree shows until the whole view is made).
        const UPGRADE_LAG: f32 = 200.0;
        let key = |w: &(bool, f32, (i32, i32), i32, bool)| w.1 + if w.0 { UPGRADE_LAG } else { 0.0 };
        want.sort_by(|a, b| key(a).total_cmp(&key(b)));
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
                t.cut(device, want.map(|q| (q, rh, rv)), gen, &drawn, &shown);
            }
        }
        self.visible.clear();
        for &k in self.tiles.keys() {
            let (min, max) = (Vec3::new(k.0 as f32 * ts, 0.0, k.1 as f32 * ts), Vec3::new((k.0 + 1) as f32 * ts, 128.0, (k.1 + 1) as f32 * ts));
            if in_frustum(min, max) {
                self.visible.push(k);
            }
        }
        self.visible.sort_by(|&a, &b| dist(a).total_cmp(&dist(b))); // near to far, like the chunks: the depth test rejects what is behind early
    }

    /// Into the opaque pass, after the chunks (`ChunkPipeline`'s pipeline and bind groups are already set).
    pub fn draw(&self, rp: &mut wgpu::RenderPass<'_>) {
        if self.sh.dbg.load(Relaxed) == 4 {
            return;
        }
        for k in &self.visible {
            let Some(t) = self.tiles.get(k) else { continue };
            let Some(ib) = t.ibuf.as_ref() else { continue };
            rp.set_vertex_buffer(0, t.vbuf.slice(..));
            rp.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
            rp.draw_indexed(0..t.index_count, 0, 0..1);
        }
    }
}

impl Lod {
    /// Into the water pass (`water_pipeline`'s pipeline and groups are set): the far water, translucent over the floor drawn under it.
    /// Before the real water, which only exists inside `chunks::WATER_RANGE` where `fs_water` drops these sheets.
    pub fn draw_water(&self, rp: &mut wgpu::RenderPass<'_>) {
        if self.sh.dbg.load(Relaxed) == 4 {
            return;
        }
        for k in &self.visible {
            let Some(t) = self.tiles.get(k) else { continue };
            let Some(ib) = t.sheet.as_ref() else { continue };
            rp.set_vertex_buffer(0, t.vbuf.slice(..));
            rp.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
            rp.draw_indexed(0..t.sheet_count, 0, 0..1);
        }
    }
}

impl Drop for Lod {
    fn drop(&mut self) {
        self.sh.quit.store(true, Relaxed);
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
        Store::new(OverworldGenerator::new(0xCAFEBABE), WorldChunkManager::new(0xCAFEBABE), None, false, Arc::default())
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

    /// A `w0` x `w0` level-0 grid whose voxel at (x, y, z) is `f(x, y, z)`.
    fn grid_of(w0: usize, f: impl Fn(usize, usize, usize) -> u32) -> Vec<u32> {
        let mut g = vec![0u32; w0 * HEIGHT * w0];
        for y in 0..HEIGHT {
            for z in 0..w0 {
                for x in 0..w0 {
                    g[(y * w0 + z) * w0 + x] = f(x, y, z);
                }
            }
        }
        g
    }

    /// `reduce`: a cell is as high as the median of its columns (not the highest, not pushed to a multiple of the step), a lone column
    /// is noise, a crown needs half of the footprint, a trunk survives under a crown at step 2, water over its floor.
    #[test]
    fn a_reduce_takes_the_median_height_and_keeps_crowns() {
        let (stone, leaf, log, water) = (1u32, 18u32, 17u32, 9u32);
        // 2 x 2 columns (a 4 x 4 grid is 2 x 2 cells): the first cell's columns are stone up to 10, 10, 10 and 14.
        let one_tall = reduce(grid_of(4, |x, y, z| { let top = if (x, z) == (0, 0) { 14 } else { 10 }; if y < top { stone } else { 0 } }), 4, 2).0;
        let at = |g: &[u32], y: usize| g[(y * 2) * 2];
        assert_eq!(at(&one_tall, 9), stone);
        assert_eq!(at(&one_tall, 10), 0, "one column of four is a spike, not a plateau");
        // two of the four reach 14: half of the footprint, so the cell reaches it (ties go up)
        let two_tall = reduce(grid_of(4, |x, y, z| { let top = if (x + z) % 2 == 0 { 14 } else { 10 }; if y < top { stone } else { 0 } }), 4, 2).0;
        assert_eq!((at(&two_tall, 13), at(&two_tall, 14)), (stone, 0), "height is exact to the block, not a multiple of 2");
        // a crown: 3 of 4 columns hold leaves at y 20..23, one trunk column (logs) 12..20, grass at 11
        let tree = |x: usize, y: usize, z: usize| -> u32 {
            if y <= 11 {
                stone
            } else if (x, z) == (0, 0) && (12..20).contains(&y) {
                log
            } else if (x, z) != (1, 1) && (20..23).contains(&y) {
                leaf
            } else {
                0
            }
        };
        let g = reduce(grid_of(4, tree), 4, 2).0;
        assert_eq!(vid(at(&g, 21)), 18, "3 of 4 columns is a crown");
        assert_eq!(vid(at(&g, 15)), 17, "the trunk keeps its cell under the crown");
        assert_eq!(vid(at(&g, 12)), 17, "down to the ground");
        assert_eq!(at(&g, 24), 0);
        // from step 4 a trunk would be 4 wide and a crown a slab on stilts: a forest (16 of 16 columns hold leaves) is a leaf carpet
        // `CARPET_MAX` thick on its ground, no trunk, nothing at the crown's height (8 x 8 grid = 2 x 2 cells, the first one is the forest)
        let big = reduce(grid_of(8, |x, y, z| if y <= 11 { stone } else if (x, z) == (0, 0) && (12..20).contains(&y) { log } else if x < 6 && z < 6 && (20..23).contains(&y) { leaf } else { 0 }), 8, 4).0;
        let cell = |y: usize| big[(y * 2) * 2];
        assert_eq!((cell(11), vid(cell(12)), vid(cell(14)), cell(15), cell(21)), (stone, 18, 18, 0, 0), "a carpet on the ground, no crown, no trunk");
        // one tree among 64 columns (25 hold leaves) is not a forest: bare ground
        let lone_tree = reduce(grid_of(8, |x, y, z| if y <= 11 { stone } else if x < 5 && z < 5 && (20..23).contains(&y) { leaf } else { 0 }), 8, 8).0;
        assert!(lone_tree[12..].iter().all(|&v| v == 0), "a lone tree is gone at step 8");
        // a crown cell of one column, no trunk: noise
        let lone = reduce(grid_of(4, |x, y, z| if (x, z) == (0, 0) && y == 30 { leaf } else { 0 }), 4, 2).0;
        assert!(lone.iter().all(|&v| v == 0));
        // a log with no crown over it is no trunk
        let post = reduce(grid_of(4, |x, y, z| if (x, z) == (0, 0) && y < 20 { log } else { 0 }), 4, 2).0;
        assert!(post.iter().all(|&v| v == 0));
        let sea = reduce(grid_of(4, |_, y, _| if y < 40 { stone } else if y < 64 { water } else { 0 }), 4, 2).0;
        assert_eq!((vid(at(&sea, 63)), vid(at(&sea, 64))), (9, 0));
        // step 1 is the grid itself
        let g1 = grid_of(2, |x, y, z| if y < 5 + x + z { stone } else { 0 });
        assert!(reduce(g1.clone(), 2, 1).0 == g1);
    }

    /// Flat ground meshes to its tops, the tile's edge skirts and nothing else: no phantom walls, no faces inside the ground. The
    /// surface is exact to the block at every step.
    #[test]
    fn flat_ground_is_tops_and_skirts_only() {
        let mut st = store_of(|_, _| land(70));
        let b = build(&mut st, (0, 0), 1, false);
        assert_eq!(quads(&b), 64 * 64 + 4 * 64, "the tops, and a skirt for every edge column");
        assert!((top(&b) - (71.0 - EPS)).abs() < 1e-4);
        assert!(b.idx.iter().all(|&i| (i as usize) < b.verts.len()));
        // At step 2 the cells are 2 x 2 columns but still one block high: the surface does not round up to 72.
        let b = build(&mut st, (0, 0), 2, false);
        assert_eq!(quads(&b), 32 * 32 + 4 * 32);
        assert!((top(&b) - (71.0 - EPS)).abs() < 1e-4, "got {}", top(&b));
    }

    /// Debug colouring: every face is the flat texel (no tiled `light`) and the skirts are their own colour, so a device screenshot says
    /// which quad is which.
    #[test]
    fn debug_mode_colours_skirts_and_walls() {
        let mut st = store_of(|_, _| land(70));
        st.sh.dbg.store(2, Relaxed);
        let b = build(&mut st, (0, 0), 1, false);
        assert_eq!(b.dbg, 2);
        assert!(b.verts.iter().all(|v| v.light < 1000.0));
        let (skirt, top) = (pack(debug_rgb(2, 1, false, K_SKIRT)), pack(debug_rgb(2, 1, false, K_TOP)));
        assert_eq!(b.verts.iter().filter(|v| v.tint == skirt).count(), 4 * 64 * 4);
        assert_eq!(b.verts.iter().filter(|v| v.tint == top).count(), 64 * 64 * 4);
    }

    /// The slit along the edge between two tiles at different steps: a tile hangs a skirt under the LOWER of its own surface and its
    /// neighbour's. Here the +x neighbour is 4 blocks lower: the wall covers 67..71, the skirt hangs from 67 for `skirt_len` blocks.
    #[test]
    fn the_edge_skirt_hangs_under_the_lower_surface() {
        let mut st = store_of(|_, _| land(70));
        st.tiles.insert((1, 0), tile_of(|_, _| land(66)));
        let b = build(&mut st, (0, 0), 4, false);
        let low = b.verts.iter().map(|v| v.pos[1]).fold(f32::MAX, f32::min);
        assert!((low - (67.0 - skirt_len(4) as f32)).abs() < 1e-4, "the skirt hangs {} blocks from the neighbour's top, got {low}", skirt_len(4));
        // a skirt is on the edge plane x = 64, between the tile and its neighbour
        assert!(b.verts.iter().any(|v| v.pos[0] == 64.0 && v.pos[1] < 66.0));
    }

    /// A tree on flat ground is a trunk and a crown of real voxels, not a tower from the crown down to the lawn: exactly the faces
    /// a real chunk would draw, and nothing rises above the crown.
    #[test]
    fn a_tree_is_a_trunk_and_a_crown() {
        let mut st = store_of(|x, z| if (x, z) == (10, 10) { vec![sp(74, 77, 18), sp(71, 74, 17), sp(70, 71, 2), sp(67, 70, 3)] } else { land(70) });
        let b = build(&mut st, (0, 0), 1, false);
        // the lawn minus the cell under the trunk, 12 trunk sides, 12 crown sides, the crown's top (trunk top / crown bottom are hidden)
        assert_eq!(quads(&b), 4095 + 12 + 12 + 1 + 4 * 64, "the lawn minus the trunk's cell, the sides, the crown top, the edge skirts");
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
        let cut = gather(&b.cells, &b.idx, &covered(&b.cells, eye, &all, &|_, _, _| true));
        assert!(cut.len() < b.idx.len(), "cells under the ring are dropped");
        assert_eq!(gather(&b.cells, &b.idx, &covered(&b.cells, eye, &none, &|_, _, _| true)).len(), b.idx.len(), "no mesh, no hole");
        let only = |cx: i32, cz: i32| (cx, cz) == (2, 2);
        let some = gather(&b.cells, &b.idx, &covered(&b.cells, eye, &only, &|_, _, _| true)).len();
        assert!(some > cut.len() && some < b.idx.len(), "one meshed chunk is one hole");
        let far = Some((Vec3::new(1000.0, 80.0, 1000.0), 64.0, 48.0));
        assert_eq!(gather(&b.cells, &b.idx, &covered(&b.cells, far, &all, &|_, _, _| true)).len(), b.idx.len(), "an eye far away drops nothing");
    }

    /// A water sheet goes with the water's section; its floor plate is never cut, whatever the real chunks draw (it hides under a real floor).
    #[test]
    fn a_floor_plate_stays_under_water_over_an_undrawn_floor() {
        let cell = |y| Cell { x: 32, y, z: 32, water: y == 63, plate: y != 63, i0: 0, i1: 6 };
        let eye = Some((Vec3::new(32.0, 100.0, 32.0), 64.0, 48.0));
        let all = |_: i32, _: i32| true;
        assert_eq!(covered(&[cell(63), cell(40)], eye, &all, &|_, _, _| true), [false, false], "neither the sheet nor the plate is cut: the shader splits the water");
    }

    /// A sea: the sheets are their own indices (one per column, in the water pass), the floor under them is drawn (top + edge skirts)
    /// and is darker the deeper the water, the floor tops are never cut.
    #[test]
    fn a_sea_is_a_translucent_sheet_over_a_drawn_floor() {
        let sea = |d: u8| vec![sp(60, 60 + d, 9), sp(56, 60, 12)];
        for (d, s) in [(1u8, 1i32), (4, 1), (4, 2)] {
            let mut st = store_of(|_, _| sea(d));
            let b = build(&mut st, (0, 0), s, false);
            let n = (64 / s as usize).pow(2);
            assert_eq!(b.sheet.len(), n * 6, "one sheet a column");
            assert_eq!(quads(&b), n + 4 * (64 / s as usize), "the floor's tops and the edge skirts, no sheet among them");
            assert_eq!(b.verts.iter().filter(|v| v.light == SHEET_LIGHT).count(), n * 4);
            assert!(b.cells.iter().filter(|c| c.plate).count() == n, "every floor top is a plate");
        }
        let dark = |d: u8| {
            let mut st = store_of(|_, _| sea(d));
            let b = build(&mut st, (0, 0), 1, false);
            b.verts.iter().find(|v| v.light == 1018.0).map(|v| v.tint).unwrap() // sand's tile
        };
        assert!(dark(1) > dark(4), "a floor under 4 blocks of water is darker than under 1");
        assert!(dark(1) < pack(PLAIN), "and under 1 it is already dimmer than in the open");
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

    /// Threads take the tile nearest the player first, never the same one twice, never a finished one.
    #[test]
    fn pregen_claims_nearest_first_and_never_twice() {
        let sh = Shared::default();
        sh.cx.store(70, Relaxed); // chunk 70 = tile 17
        let a = claim(&sh).unwrap();
        assert_eq!(a, (17, 0));
        let b = claim(&sh).unwrap();
        assert_ne!(a, b, "a tile that is being made is not taken again");
        sh.pg().done.insert(b);
        let c = claim(&sh).unwrap();
        assert!(c != a && c != b);
    }

    /// A made tile is kept with its trees, counted 16 chunks, written to disk; a view build that got there first wins.
    #[test]
    fn a_made_tile_is_kept_and_counted() {
        let mut st = store();
        let k = (3, -2);
        let t = make_tile(&mut st.g, &mut st.cm, k, true);
        st.accept(k, t, true);
        assert!(st.tiles[&k].trees && st.dirty.contains(&k));
        assert_eq!(st.sh.chunks.load(Relaxed), 16);
        let again = make_tile(&mut st.g, &mut st.cm, k, true);
        st.accept(k, again, false);
        assert_eq!(st.sh.chunks.load(Relaxed), 16, "the same tile counts once");
    }

    /// The bug: with the view asking for builds all the time (`update` refills `MAX_PENDING` every frame), the pre-generation never
    /// ran (the worker took it only when idle). Now its own threads make tiles while builds keep coming.
    #[test]
    fn pregen_runs_while_the_view_is_busy() {
        let mut lod = Lod::new(0xCAFEBABE, false, None);
        lod.toggle_pregen();
        let (t0, mut next) = (Instant::now(), 0);
        let mut ask = |lod: &Lod| {
            next += 1;
            lod.job.send(Msg::Build(200 + 5 * next, 0, 1, false)).unwrap();
        };
        (0..MAX_PENDING).for_each(|_| ask(&lod));
        while lod.pregen().1 == 0 {
            assert!(t0.elapsed() < Duration::from_secs(120), "no tile was made while the view kept building");
            if lod.rx.recv_timeout(Duration::from_millis(50)).is_ok() {
                ask(&lod);
            }
        }
        lod.toggle_pregen();
    }

    /// From step 2 a snow layer is no voxel: it made a white cube of the crown (or hill) it lay on. Ground under it turns white, leaves
    /// stay green. At step 1 it is the thin layer.
    #[test]
    fn snow_is_a_thin_layer_near_and_not_a_cube_far() {
        let in_snow = |v: &Vertex| v.light == 1066.0; // the quad's tile rides in `light` (`Quads::quad_t`)
        // A crown (leaves 77..81) with snow on top (81): the cell [80, 84) held snow over leaves and turned white.
        let mut st = store_of(|_, _| vec![sp(81, 82, 78), sp(77, 81, 18), sp(60, 77, 3)]);
        let near = build(&mut st, (0, 0), 1, false);
        assert!((top(&near) - (81.0 + 0.125 - EPS)).abs() < 1e-4, "got {}", top(&near));
        let far = build(&mut st, (0, 0), 4, false);
        assert!(!far.verts.iter().any(|v| in_snow(v)), "the crown stays leaves");
        // Snow on grass: the ground shows the snow tile instead.
        let mut st = store_of(|_, _| vec![sp(71, 72, 78), sp(70, 71, 2), sp(66, 70, 3)]);
        let far = build(&mut st, (0, 0), 2, false);
        assert!(far.verts.iter().any(|v| in_snow(v) && v.pos[1] > 69.0), "the ground shows the snow tile");
    }

    /// Cells get coarser with distance, never finer, and the far hills are at most 8 blocks.
    #[test]
    fn steps_grow_with_distance() {
        let s: Vec<i32> = (0..100).map(|i| step_for(i as f32 * 10.0)).collect();
        assert!(s.windows(2).all(|w| w[0] <= w[1]) && s[0] == 1 && *s.last().unwrap() == 8);
    }
}
