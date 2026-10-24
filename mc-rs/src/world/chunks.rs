//! Chunk manager: chunks stored by (cx, cz), one small mesh per chunk, a render-distance ring
//! that loads and unloads as the player walks, terrain generated on worker threads.
//!
//! Per frame: `stream` (cheap, no GPU) collects finished chunks, unloads far ones, asks the
//! workers for missing ones nearest-first, sends `populate` jobs to the workers and puts the finished
//! ones back (`populate_dispatch`, `populate_land`); `mesh_pending` uploads the meshes
//! the workers finished (within the frame's upload budget) and queues jobs for the chunks that need one (the
//! workers copy the chunk and its four neighbours, build the vertices and send them back).
//!
//! Three states per chunk: raw (terrain + caves, from a worker), populated (trees, ores, ...) and
//! final. populate(P) needs raw P, P+x, P+z, P+x+z and writes into all four, so a chunk C is final
//! once populate has run on C, C-x, C-z and C-x-z, and is meshed only then (and with its four
//! neighbours loaded, so border faces cull correctly). That is why the raw ring is two chunks wider
//! than the render ring. A populate that changes an already meshed chunk (or its neighbour's
//! border) just marks it for a rebuild.
//!
//! M13 frustum culling: each chunk's bounds are known from its key
//! ((cx*16, 0, cz*16) .. +(16, 128, 16)), so it is one `filter` on `meshes()` in the draw loop.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};

use embeddium::{arena::Arena, quad_index, upload_budget::{self, UploadBudget, UploadDurationEstimator}, vertex::{self, Packed}, vis};
use glam::Vec3;

use crate::gpu::gfx::{Buf, Gfx};
use crate::gpu::pipeline::{DrawOrigins, MAX_ORIGINS};
use crate::render::mesh;
use crate::world::chunk::{height_map, idx, meta_matters, normal_cube, opaque, physics_id, Nibbles, H, VOLUME};
use crate::world::gen::chunk_manager::WorldChunkManager;
use crate::world::gen::hydro;
use crate::world::gen::overworld::{OverworldGenerator, WorldType};
use crate::world::gen::noise::JavaRandom;
use crate::world::chest::{Chest, Pos};
use crate::world::gen::populate::Region;
use crate::world::save;

mod light;
mod wide;

type Key = (i32, i32);

/// Multiply-rotate hasher for the chunk map. The default SipHash was the main cost of the light engine
/// (about 10 map lookups per cell evaluated); keys here are small ints, not attacker input.
#[derive(Default)]
struct KeyHasher(u64);

impl KeyHasher {
    fn add(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

impl std::hash::Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        bytes.iter().for_each(|&b| self.add(b as u64));
    }
    fn write_i32(&mut self, i: i32) {
        self.add(i as u32 as u64);
    }
}

type Chunks = HashMap<Key, Entry, std::hash::BuildHasherDefault<KeyHasher>>;

/// The most finished meshes uploaded in a frame, whatever the budget says (a guard against a bad estimate; empty meshes count too).
/// What normally stops a frame's uploads is the budget (`UploadBudget`, Sodium's `UploadResourceBudget`): the meshing itself runs on
/// the workers (`Job::Mesh`), the upload is the only part that needs the device.
const MESH_PER_FRAME: usize = 8;

/// Bytes of mesh data a frame may upload (Sodium: the staging buffer's `getUploadSizeLimit`; there is no staging buffer here, so
/// it is a number). The first mesh of a frame always goes up whatever its size, so a finished mesh never starves; the next ones
/// wait while they do not fit this and the time share (`upload_budget::frame_duration_budget`: 30% of the average frame, at least
/// 2 ms). ponytail: 1 MiB is a guess (a surface chunk is some 100-500 KB); lever if the ring fills too slowly or a frame still hitches.
const UPLOAD_SIZE_LIMIT: u64 = 1 << 20;

/// The frame length assumed before the first two `update` calls give a measured one (60 Hz).
const FIRST_FRAME_NS: f64 = 16_666_667.0;

/// A frame whose chunk work (stream, populate, light, meshes) took this long or more is logged with its stages.
const SLOW_CHUNK_WORK: std::time::Duration = std::time::Duration::from_millis(16);

/// Saved chunks read per frame (the rest wait for the next frame, like generation does).
/// ponytail: the read + decode runs on the render thread (about 32 KB of work per chunk); move it to the workers if it shows.
const LOADS_PER_FRAME: usize = 8;

/// The grass and foliage colour of every column of chunk (cx, cz) (`BlockGrass.colorMultiplier` and friends read the climate of the
/// column itself, no blending). ponytail: the climate noise is sampled again for every (re)mesh; cache it per chunk if remeshing shows up in a profile.
pub(crate) fn biome_tints(cm: &mut WorldChunkManager, cx: i32, cz: i32) -> mesh::Tints {
    let biomes = cm.load_block_generator_data(cx * 16, cz * 16, 16, 16);
    let mut t = mesh::Tints::white();
    for i in 0..256 {
        let (temp, humid) = (cm.temperature[i], cm.humidity[i]);
        // The Blooming biomes have their own colours (the datapack's); every other biome reads the climate maps.
        let (grass, foliage) = biomes[i].colors().unwrap_or_else(|| (crate::world::climate::grass(temp, humid), crate::world::climate::foliage(temp, humid)));
        t.grass[i] = grass as f32;
        t.foliage[i] = foliage as f32;
    }
    t
}

/// The far terrain's columns of a loaded chunk.
fn cols_of(cm: &mut WorldChunkManager, k: Key, e: &Entry) -> crate::world::lod::Cols {
    crate::world::lod::columns(&e.blocks, &e.data, &biome_tints(cm, k.0, k.1))
}

/// Chunk coordinate of a world-space coordinate (floor division by 16).
pub fn chunk_coord(v: f32) -> i32 {
    (v.floor() as i32) >> 4
}

/// A chunk's mesh as a run of one `Page` (Sodium's region arena): its vertices at `v..` (12 bytes each, `embeddium::vertex`), sorted by
/// layer and section, four to a quad. The GPU buffers belong to the page and the index buffer is the one shared by every page
/// (`embeddium::quad_index`), so a chunk costs no buffer and no index upload of its own and the draw loop binds a page, not a chunk.
pub struct Mesh {
    page: u32,
    /// First vertex in the page: a multiple of 4, so its quads are the shared index buffer's quads `v / 4 ..`.
    v: u32,
    /// Slot of the chunk's origin in `DrawOrigins` (group 1; wgpu), and the origin itself (the GL draw loop sets it as a uniform).
    slot: u32,
    origin: [f32; 3],
    /// Index elements of the solid and cut-out layers: what `mesh_gen` compares to tell whether the geometry changed.
    pub index_count: u32,
    /// The quads are sorted by 16-block section: section `s` of the solid layer is `starts[s]..starts[s + 1]` (`pack`), the cut-out
    /// layer follows (`cut`). All of them, and `water`, are index offsets into the shared index buffer: the range that draws those quads.
    starts: [u32; SECTIONS + 1],
    cut: [u32; SECTIONS + 1],
    /// The water quads (blended, drawn after everything else) follow in the same run, sorted by section the same way.
    water: Option<[u32; SECTIONS + 1]>,
    /// Per section, which faces see each other through it (`embeddium::vis`): what the occlusion walk reads.
    vis: [u64; SECTIONS],
}

/// Vertices per page (`PAGE_VERTS * 12` bytes). A page is one vertex buffer carved up by an `embeddium::arena::Arena`; a few pages hold
/// the whole ring. A mesh that is bigger than a page is not drawn. Lever: a bigger page = fewer buffer switches in the draw loop,
/// but memory is taken a page at a time. The shared index buffer is sized for one page (`PAGE_VERTS / 4` quads, 6 MiB) and serves all.
const PAGE_VERTS: u32 = 1 << 20;
const VERT_BYTES: u64 = embeddium::vertex::STRIDE as u64;

struct Page {
    id: u32,
    vbuf: Buf,
    verts: Arena,
}

impl Page {
    fn new(gfx: Gfx, id: u32) -> Self {
        let vbuf = gfx.writable(PAGE_VERTS as u64 * VERT_BYTES, false);
        Self { id, vbuf, verts: Arena::new(PAGE_VERTS, PAGE_VERTS) }
    }
}

/// Every page of chunk meshes. ponytail: pages never grow or move (`Arena::grow` is unused): a mesh is written once, at its place, and the
/// shared index buffer names vertices by their absolute number in the page, so `draw_indexed` needs no `base_vertex` (absent on a GLES 3.0
/// fallback). The price is no compaction: a page fragments until its chunks unload, and an empty one is dropped. Every length is a multiple
/// of 4 (whole quads), so every offset is too. Upgrade = `base_vertex` where `DownlevelFlags::BASE_VERTEX` is set, one growing arena.
#[derive(Default)]
struct Pages {
    list: Vec<Page>,
    next: u32,
    /// `SharedQuadIndexBuffer`: `quad_index::indices(PAGE_VERTS / 4)`, made with the first page and kept.
    quads: Option<Buf>,
    /// Free `DrawOrigins` slots, and the first never used one.
    free_slots: Vec<u32>,
    next_slot: u32,
}

impl Pages {
    fn get(&self, id: u32) -> Option<&Page> {
        self.list.iter().find(|p| p.id == id)
    }

    fn slot(&mut self) -> Option<u32> {
        self.free_slots.pop().or_else(|| {
            (self.next_slot < MAX_ORIGINS).then(|| {
                self.next_slot += 1;
                self.next_slot - 1
            })
        })
    }

    /// Copies a mesh in: `verts` are whole quads, sorted by the caller. Writes the chunk's origin to a slot. Returns the page, the first
    /// vertex and the slot; `None` when the mesh is bigger than a page or every slot is taken.
    fn put(&mut self, gfx: Gfx, origins: Option<&DrawOrigins>, verts: &[Packed], key: Key) -> Option<(u32, u32, u32)> {
        let nv = verts.len() as u32;
        assert_eq!(nv % quad_index::VERTICES_PER_QUAD, 0, "whole quads only");
        if nv > PAGE_VERTS {
            return None;
        }
        let Some(slot) = self.slot() else {
            log::error!("chunk {key:?}: no free origin slot ({MAX_ORIGINS} meshes alive)");
            return None;
        };
        if self.quads.is_none() {
            self.quads = Some(gfx.index(bytemuck::cast_slice(&quad_index::indices(PAGE_VERTS / quad_index::VERTICES_PER_QUAD))));
        }
        let at = self.list.iter_mut().find_map(|p| p.verts.alloc(nv).map(|v| (p.id, v)));
        let (id, v) = match at {
            Some(a) => a,
            None => {
                let mut p = Page::new(gfx, self.next);
                self.next += 1;
                let v = p.verts.alloc(nv).expect("a mesh that fits a page fits an empty one");
                let id = p.id;
                self.list.push(p);
                (id, v)
            }
        };
        let p = self.list.iter().find(|p| p.id == id).expect("page just used");
        gfx.write(&p.vbuf, v as u64 * VERT_BYTES, bytemuck::cast_slice(verts));
        if let (Some(o), Gfx::Wgpu(_, q)) = (origins, gfx) {
            o.write(q, slot, [key.0 as f32 * 16.0, 0.0, key.1 as f32 * 16.0]);
        }
        Some((id, v, slot))
    }

    /// Gives a mesh's run and slot back; a page left empty is dropped unless it is the last one (no buffer churn for one chunk's rebuild).
    fn free(&mut self, m: &Mesh) {
        let Some(n) = self.list.iter().position(|p| p.id == m.page) else { return };
        self.list[n].verts.free(m.v);
        self.free_slots.push(m.slot);
        if self.list[n].verts.is_empty() && self.list.len() > 1 {
            self.list.remove(n);
        }
    }
}

/// One draw of a chunk run: the page's vertex buffer, the shared quad index buffer, the chunk (`slot` for wgpu, `origin` for GL) and the
/// index range.
pub struct Run<'a> {
    pub vbuf: &'a Buf,
    pub quads: &'a Buf,
    pub page: u32,
    pub slot: u32,
    pub origin: [f32; 3],
    pub first: u32,
    pub end: u32,
}

/// The render layers of a chunk mesh (Sodium's passes): opaque faces, alpha-tested faces, translucent water.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Layer {
    Solid,
    Cutout,
    Water,
}

/// Sections (16 blocks tall) of a chunk column: the unit the draw loop culls.
pub const SECTIONS: usize = H / 16;
const _: () = assert!(SECTIONS <= 8); // `ChunkManager::visible` keeps one bit per section in a u8

/// Radius (blocks, 3D distance from the eye) of the REAL translucent water. Real water blends over what is under it, so it is only safe where
/// the real floor is certainly drawn; the floor's 16-block sections reach less far than the water's when the eye is high, and the two earlier
/// fixes that cut by section (24 blocks, then the opacity fade) still left real water over bare sky and over LoD plates, skirts and trunks.
/// So the split is per PIXEL at exactly this distance: `fs_water` discards real water farther out, `fs_main` discards the LoD's water
/// sheet (`lod::SHEET_LIGHT`, translucent, drawn by `fs_water` too) closer in; `draw_ranges` only skips the sections that have none of it. Lever: raise it if the near water
/// looks too small, lower it if a pale plate shows near the edge. Only with the LoD ON: with it off there is no sheet to hand over to, so `lib.rs`
/// passes `1e9` instead (`update_visible`'s `wr`, `Uniforms::fogp.y`) and the real water is everything `visible` is.
pub const WATER_RANGE: f32 = 12.0;

/// Which blocks around the eye are drawn. `Cylinder`: a circle of the horizontal radius (circular horizontal distance).
/// `Sphere`: 3D distance <= the horizontal radius. Both are also capped by the vertical radius. All distances run from the eye
/// to the nearest point of the section's box, so a section is drawn as soon as any part of it is in range.
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Shape {
    Cylinder,
    Sphere,
}

/// The sections of the mask `mask` of chunk (x, z) that are within `rh` / `rv` of the eye: the same box (one block taller at both
/// ends) and the same `in_range` as `ChunkManager::update_visible`.
fn near_sections(x: i32, z: i32, mask: u8, eye: Vec3, shape: Shape, rh: f32, rv: f32) -> u8 {
    (0..SECTIONS as i32).filter(|&y| mask >> y & 1 != 0).fold(0, |m, y| {
        let min = Vec3::new(x as f32 * 16.0, y as f32 * 16.0 - 1.0, z as f32 * 16.0);
        if in_range(shape, eye, min, min + Vec3::new(16.0, 18.0, 16.0), rh, rv) { m | 1u8 << y } else { m }
    })
}

pub fn in_range(shape: Shape, eye: Vec3, min: Vec3, max: Vec3, rh: f32, rv: f32) -> bool {
    let d = (min - eye).max(eye - max).max(Vec3::ZERO);
    let dh2 = d.x * d.x + d.z * d.z;
    d.y <= rv && dh2 + if shape == Shape::Sphere { d.y * d.y } else { 0.0 } <= rh * rh
}

/// One quad of the mesher's output (`mesh::build_split_tinted`): its first vertex and how its index list drew it. The mesher draws a quad
/// as `b, b+1, b+2, b, b+2, b+3`; a smooth face may take the other diagonal (`b+1, b+2, b+3, b+1, b+3, b`) and a plant, rail or torch is
/// seen from both sides (that list, then the same triangles reversed: 12 indices).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Q {
    v: u32,
    flip: bool,
    both: bool,
}

/// The quads an index list draws, in order.
fn quads_of(idxs: &[u32]) -> Vec<Q> {
    let mut out = Vec::with_capacity(idxs.len() / 6);
    let mut p = 0;
    while p < idxs.len() {
        let f = idxs[p];
        let (v, flip) = (f & !3, f & 3 == 1);
        let both = !flip && idxs.get(p + 6) == Some(&v) && idxs.get(p + 7) == Some(&(v + 2)) && idxs.get(p + 8) == Some(&(v + 1));
        let want: &[u32] = match (flip, both) {
            (true, _) => &[v + 1, v + 2, v + 3, v + 1, v + 3, v],
            (false, false) => &[v, v + 1, v + 2, v, v + 2, v + 3],
            (false, true) => &[v, v + 1, v + 2, v, v + 2, v + 3, v, v + 2, v + 1, v, v + 3, v + 2],
        };
        assert_eq!(idxs.get(p..p + want.len()), Some(want), "the mesher drew a primitive that is not a quad");
        p += want.len();
        out.push(Q { v, flip, both });
    }
    out
}

/// A vertex of a quad on its way to the 12-byte format (`embeddium::vertex`): the mesher's `VF` floats, with the atlas coordinate turned
/// into a texel position inside the quad's tile.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PV {
    /// Chunk-local (the mesher is run with origin 0).
    pos: [f32; 3],
    /// Texels inside the tile: (0, 0) is its top left corner; a merged run of faces runs past 16.
    loc: [f32; 2],
    /// `mesh::SMOOTH_LM + sky quarters * 32 + block halves` (smooth), or `sky * 16 + block` (flat).
    lm: f32,
    /// A smooth vertex's face shade x ambient occlusion; a flat one gets its shade from the way its quad faces.
    lit: f32,
    /// Packed sRGB colour (`mesh::WHITE` = none).
    tint: f32,
}

/// A quad of four vertices (counter-clockwise from the front) and the atlas tile all four show a part of.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PQ {
    v: [PV; 4],
    tile: u16,
    /// The tile shows a see-through texel somewhere (`atlas::is_cutout`): the alpha-tested layer.
    cut: bool,
    /// The quad tiles its texture (a merged run, see `merged`): the shader takes the fraction of the tile coordinate.
    repeat: bool,
}

/// The tile (row * 16 + column) the point `c` (atlas coordinates, 0..1) lies in.
fn tile_of(c: [f32; 2]) -> u16 {
    let (x, y) = (c[0] * crate::render::atlas::ATLAS_W as f32, c[1] * crate::render::atlas::ATLAS_H as f32);
    let w = crate::render::atlas::TILES_W;
    ((y.max(0.0) as usize / 16) * w + (x.max(0.0) as usize / 16).min(w - 1)) as u16
}

/// The mesher's quad whose first vertex is `v`, as a `PQ`: the tile is the one at the centre of its four atlas coordinates (the corners of
/// a face lie on the tile's edge, so a single corner could be one tile off).
fn quad_at(verts: &[f32], v: u32) -> PQ {
    let vf = mesh::VF;
    let f = |k: usize| &verts[(v as usize + k) * vf..][..vf];
    let centre = [(0..4).map(|k| f(k)[3]).sum::<f32>() / 4.0, (0..4).map(|k| f(k)[4]).sum::<f32>() / 4.0];
    let tile = tile_of(centre);
    let w = crate::render::atlas::TILES_W;
    let (ox, oy) = ((tile as usize % w) as f32 * 16.0, (tile as usize / w) as f32 * 16.0);
    let pv = |k: usize| {
        let r = f(k);
        PV { pos: [r[0], r[1], r[2]], loc: [r[3] * crate::render::atlas::ATLAS_W as f32 - ox, r[4] * crate::render::atlas::ATLAS_H as f32 - oy], lm: r[6], lit: r[5], tint: r[7] }
    };
    PQ { v: [pv(0), pv(1), pv(2), pv(3)], tile, cut: crate::render::atlas::is_cutout(centre), repeat: false }
}

/// The 16-block section a quad is sorted into: the highest vertex minus half a block picks the block, so top, side and plant faces land in
/// their own block's section; a face on a section border may land in either, which the draw loop's one-block margin covers.
fn section_of(q: &PQ) -> usize {
    let top = q.v.iter().map(|v| v.pos[1]).fold(f32::MIN, f32::max);
    (((top - 0.5).floor().max(0.0) as usize) >> 4).min(SECTIONS - 1)
}

/// A quad that is one block face on the block grid showing a whole terrain tile, the same light, shade and colour on all four corners:
/// the only kind `merged` can join with its neighbours. `an` is the axis it is flat on, `au` / `av` the axes its corner 1 and corner 3 lie
/// along (the template's u and v), `su` / `sv` which way the corners go along them and `du` / `dv` which way the texture runs.
#[derive(Clone, Copy, Debug)]
struct Unit {
    an: usize,
    plane: i32,
    au: usize,
    av: usize,
    su: i32,
    sv: i32,
    du: i32,
    dv: i32,
    /// The cell: the lower of its two coordinates along `au` and `av`.
    cu: i32,
    cv: i32,
}

fn unit_of(q: &PQ) -> Option<Unit> {
    if q.tile >= 256 || (1..4).any(|k| q.v[k].lm.to_bits() != q.v[0].lm.to_bits() || q.v[k].lit.to_bits() != q.v[0].lit.to_bits() || q.v[k].tint.to_bits() != q.v[0].tint.to_bits()) {
        return None;
    }
    let p = |k: usize| q.v[k].pos;
    let same = |a: usize| (1..4).all(|k| p(k)[a] == p(0)[a]);
    let mut an = None;
    for a in 0..3 {
        if same(a) {
            if an.is_some() {
                return None; // a point or a line
            }
            an = Some(a);
        }
    }
    let an = an?;
    if p(0)[an].fract() != 0.0 {
        return None; // a face inside the cell (slab, torch, fence...)
    }
    let axes = match an { 0 => [1, 2], 1 => [0, 2], _ => [0, 1] };
    for a in axes {
        let (lo, hi) = (0..4).fold((f32::MAX, f32::MIN), |(lo, hi), k| (lo.min(p(k)[a]), hi.max(p(k)[a])));
        if hi - lo != 1.0 || lo.fract() != 0.0 {
            return None; // not exactly one block wide on the grid (a fluid's corner, a cactus, a stair step)
        }
    }
    let along = |k: usize| axes.into_iter().find(|&a| p(k)[a] != p(0)[a]);
    let (au, av) = (along(1)?, along(3)?);
    if au == av || p(1)[av] != p(0)[av] || p(3)[au] != p(0)[au] || p(2)[au] != p(1)[au] || p(2)[av] != p(3)[av] {
        return None;
    }
    // The texture: the whole tile (`atlas::terrain_uv` insets it by 0.02 texel), u along corner 0 -> 1 and v along corner 0 -> 3.
    let l = |k: usize| q.v[k].loc;
    let near = |a: f32, b: f32| (a - b).abs() < 0.1;
    let (dx, dy) = (l(1)[0] - l(0)[0], l(3)[1] - l(0)[1]);
    let inside = (0..4).all(|k| l(k).iter().all(|&c| c > -0.1 && c < 16.1));
    if !(inside && near(l(3)[0], l(0)[0]) && near(l(2)[0], l(1)[0]) && near(l(1)[1], l(0)[1]) && near(l(2)[1], l(3)[1]) && near(dx.abs(), 16.0) && near(dy.abs(), 16.0)) {
        return None;
    }
    let sign = |d: f32| if d > 0.0 { 1 } else { -1 };
    Some(Unit {
        an,
        plane: p(0)[an] as i32,
        au,
        av,
        su: sign(p(1)[au] - p(0)[au]),
        sv: sign(p(3)[av] - p(0)[av]),
        du: sign(dx),
        dv: sign(dy),
        cu: p(0)[au].min(p(1)[au]) as i32,
        cv: p(0)[av].min(p(3)[av]) as i32,
    })
}

/// What two `Unit`s must share to be one quad: the layer, the section (so a merged quad stays in the section it is drawn with), the plane,
/// the way they face and their texture runs, the tile, and the light, shade and colour (all four corners of each agree, so the run has the
/// same values everywhere and needs no per-cell interpolation).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Join {
    cut: bool,
    section: usize,
    an: usize,
    plane: i32,
    au: usize,
    av: usize,
    su: i32,
    sv: i32,
    du: i32,
    dv: i32,
    tile: u16,
    lm: u32,
    lit: u32,
    tint: u32,
}

/// The cells of one `Join`, and the first of its quads to build the merged ones from (`rcu`, `rcv`: its cell).
struct Group {
    rep: PQ,
    rcu: i32,
    rcv: i32,
    cells: Vec<(i32, i32)>,
}

/// Longest side of a merged quad, in blocks: a chunk is 16 wide, a section 16 high, and 16 tiles is the texture coordinate's range
/// (`embeddium::vertex`: 15 tiles either way from the one the quad starts in).
const MERGE_MAX: i32 = 16;

/// Greedy meshing of one plane: the rectangles (cell u, cell v, width, height) that cover `cells` once, taking the next free cell in
/// (v, u) order, widening as far as the row runs and then heightening while whole rows follow.
fn greedy(cells: &mut Vec<(i32, i32)>) -> Vec<(i32, i32, i32, i32)> {
    cells.sort_unstable_by_key(|&(u, v)| (v, u));
    let mut free: HashSet<(i32, i32)> = cells.iter().copied().collect();
    let mut out = Vec::new();
    for &(u0, v0) in cells.iter() {
        if !free.contains(&(u0, v0)) {
            continue;
        }
        let mut w = 1;
        while w < MERGE_MAX && free.contains(&(u0 + w, v0)) {
            w += 1;
        }
        let mut h = 1;
        'grow: while h < MERGE_MAX {
            for du in 0..w {
                if !free.contains(&(u0 + du, v0 + h)) {
                    break 'grow;
                }
            }
            h += 1;
        }
        for dv in 0..h {
            for du in 0..w {
                free.remove(&(u0 + du, v0 + dv));
            }
        }
        out.push((u0, v0, w, h));
    }
    out
}

/// The quad of the rectangle (`u0`, `v0`, `w`, `h`) of a run: the corners of `rep` (a quad of the run, in cell `rcu`, `rcv`) moved to the
/// rectangle, the far ones stretched, and the texture coordinate stretched with them, so the tile repeats once per block.
fn merged(rep: &PQ, run: &Join, rcu: i32, rcv: i32, u0: i32, v0: i32, w: i32, h: i32) -> PQ {
    let mut q = *rep;
    for (k, v) in q.v.iter_mut().enumerate() {
        let hi_u = v.pos[run.au] == (rcu + 1) as f32;
        let hi_v = v.pos[run.av] == (rcv + 1) as f32;
        v.pos[run.au] += (u0 - rcu) as f32 + if hi_u { (w - 1) as f32 } else { 0.0 };
        v.pos[run.av] += (v0 - rcv) as f32 + if hi_v { (h - 1) as f32 } else { 0.0 };
        if k == 1 || k == 2 {
            v.loc[0] += run.du as f32 * 16.0 * (w - 1) as f32;
        }
        if k == 2 || k == 3 {
            v.loc[1] += run.dv as f32 * 16.0 * (h - 1) as f32;
        }
    }
    q.repeat = w > 1 || h > 1;
    q
}

/// b1.7.3 `RenderBlocks` face shade of a quad (top 1.0, bottom 0.5, Z sides 0.8, X sides 0.6) as a `vertex::FACE_SHADE` index, from the quad's
/// first three corners (counter-clockwise seen from the front, so the cross product points out of the face): the axis the normal is longest
/// on, and its sign for Y. This is what the chunk shader's `flat_shade` derived per pixel from screen derivatives, with the same pick for a
/// face that is not axis aligned (a wall torch) and the same 1.0 for a degenerate one.
fn face_code(a: &[f32; 3], b: &[f32; 3], c: &[f32; 3]) -> u8 {
    let (u, v) = (std::array::from_fn::<f32, 3, _>(|i| b[i] - a[i]), std::array::from_fn::<f32, 3, _>(|i| c[i] - a[i]));
    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let (x, y, z) = (n[0].abs(), n[1].abs(), n[2].abs());
    if n.iter().map(|k| k * k).sum::<f32>() < 1e-20 {
        0
    } else if x >= y && x >= z {
        2 // 0.6
    } else if y >= z {
        if n[1] > 0.0 { 0 } else { 3 }
    } else {
        1 // 0.8
    }
}

/// A vertex as the 12-byte chunk vertex. A smooth vertex (`lm >= mesh::SMOOTH_LM`) carries its own sky quarters, block halves and its shade x
/// ambient occlusion as a shade code; a flat one is the same with `sky * 4`, `block * 2` and its quad's face shade (`face`); water keeps shade 1
/// (`fs_water` derives the shade of the side the eye sees).
fn encode_vertex(v: &PV, tile: u16, repeat: bool, face: u8, water: bool) -> Packed {
    let smooth = v.lm >= mesh::SMOOTH_LM;
    let (sky, block) = if smooth {
        let l = (v.lm - mesh::SMOOTH_LM) as u32;
        ((l >> 5) as u8, (l & 31) as u8)
    } else {
        let l = v.lm as u32;
        ((l >> 4) as u8 * 4, (l & 15) as u8 * 2)
    };
    let tint = v.tint as u32;
    vertex::encode(&vertex::Vertex {
        pos: v.pos,
        tile,
        uv: v.loc,
        repeat,
        sky,
        block,
        shade: if water { 0 } else if smooth { vertex::shade_code(v.lit) } else { face },
        color: [(tint >> 16) as u8, (tint >> 8) as u8, tint as u8],
    })
}

/// A chunk's quads as the GPU takes them: 12-byte vertices, four to a quad, the solid layer, then the cut-out layer, then the water, each
/// sorted by 16-block section. `starts`, `cut` and `wstarts` are QUAD numbers from the first vertex of the mesh; layer `x` section `s` is
/// `x[s]..x[s + 1]`.
#[derive(Debug, PartialEq)]
struct Quads {
    verts: Vec<Packed>,
    starts: [u32; SECTIONS + 1],
    cut: [u32; SECTIONS + 1],
    wstarts: [u32; SECTIONS + 1],
}

/// Sorts the mesher's quads into layers (a quad that shows a see-through tile is cut-out: the tile at the centre of its four uvs) and
/// the 16-block section they sit in, joins the block faces that can be one quad (`unit_of`, `greedy`, `merged`: a flat floor of 256
/// faces is one quad, not 256) and encodes them. The other diagonal becomes a rotation of the corners and the second side a second quad
/// with the corners reversed, so the one shared index pattern (`embeddium::quad_index`) draws exactly the triangles the mesher's own
/// index list did.
fn pack(verts: &[f32], solid: &[u32], water: &[u32]) -> Quads {
    let mut layers: [Vec<(usize, PQ, bool, bool)>; 3] = Default::default();
    let mut groups: BTreeMap<Join, Group> = BTreeMap::new();
    for q in quads_of(solid) {
        let pq = quad_at(verts, q.v);
        let section = section_of(&pq);
        match (!q.flip && !q.both).then(|| unit_of(&pq)).flatten() {
            Some(u) => {
                let run = Join {
                    cut: pq.cut,
                    section,
                    an: u.an,
                    plane: u.plane,
                    au: u.au,
                    av: u.av,
                    su: u.su,
                    sv: u.sv,
                    du: u.du,
                    dv: u.dv,
                    tile: pq.tile,
                    lm: pq.v[0].lm.to_bits(),
                    lit: pq.v[0].lit.to_bits(),
                    tint: pq.v[0].tint.to_bits(),
                };
                groups.entry(run).or_insert_with(|| Group { rep: pq, rcu: u.cu, rcv: u.cv, cells: Vec::new() }).cells.push((u.cu, u.cv));
            }
            None => layers[pq.cut as usize].push((section, pq, q.flip, q.both)),
        }
    }
    for q in quads_of(water) {
        let pq = quad_at(verts, q.v);
        layers[2].push((section_of(&pq), pq, q.flip, q.both));
    }
    for (run, mut g) in groups {
        for (u0, v0, w, h) in greedy(&mut g.cells) {
            layers[run.cut as usize].push((run.section, merged(&g.rep, &run, g.rcu, g.rcv, u0, v0, w, h), false, false));
        }
    }
    let mut out = Quads { verts: Vec::with_capacity((solid.len() + water.len()) / 6 * 4), starts: [0; SECTIONS + 1], cut: [0; SECTIONS + 1], wstarts: [0; SECTIONS + 1] };
    for (l, list) in layers.iter_mut().enumerate() {
        list.sort_by_key(|e| e.0);
        let mut starts = [0u32; SECTIONS + 1];
        let mut it = list.iter().peekable();
        for s in 0..SECTIONS {
            starts[s] = out.verts.len() as u32 / 4;
            while let Some((_, q, flip, both)) = it.next_if(|e| e.0 == s) {
                let mut put = |order: [usize; 4]| {
                    let face = face_code(&q.v[order[0]].pos, &q.v[order[1]].pos, &q.v[order[2]].pos);
                    order.iter().for_each(|&k| out.verts.push(encode_vertex(&q.v[k], q.tile, q.repeat, face, l == 2)));
                };
                if *flip {
                    put([1, 2, 3, 0]);
                } else {
                    put([0, 1, 2, 3]);
                    if *both {
                        put([0, 3, 2, 1]);
                    }
                }
            }
        }
        starts[SECTIONS] = out.verts.len() as u32 / 4;
        match l {
            0 => out.starts = starts,
            1 => out.cut = starts,
            _ => out.wstarts = starts,
        }
    }
    out
}

/// Blocks, metadata and light of one chunk for a mesh job: the two big arrays are shared (`Arc`), not copied; a write while the job
/// holds them (`Arc::make_mut`) copies that chunk's array once, so the job still sees the chunk as it was.
struct Snap {
    blocks: Arc<Vec<u8>>,
    data: Nibbles,
    light: Arc<Vec<u8>>,
}

fn snap(e: &Entry) -> Snap {
    Snap { blocks: Arc::clone(&e.blocks), data: e.data.clone(), light: Arc::clone(&e.light) }
}

/// Everything the mesher reads, owned, so a worker can build a chunk's mesh while the manager goes on editing the live chunks: the
/// chunk, then its +X, -X, +Z, -Z neighbours (`mesh::build_split_tinted`'s argument order).
/// ponytail: `data` (metadata) is still cloned per job (lazy `Nibbles`: free while all 0, 16 KB otherwise); the upgrade is an `Arc` for it too.
struct MeshJob {
    key: Key,
    sky_sub: u8,
    cells: [Snap; 5],
}

/// What a worker is asked: generate a chunk (`OverworldGenerator::generate`) or build a mesh.
enum Job {
    Gen(Key),
    Mesh(Box<MeshJob>),
    Populate(Box<PopJob>),
}

/// `populate` for the chunk (x, z) on a copy of its 2x2 block (`Region::new` takes the arrays by value), so the chunks stay
/// readable here while it runs.
struct PopJob {
    x: i32,
    z: i32,
    region: Region,
}

/// A finished mesh, short of its GPU buffers: the packed quads (`pack`) and the section ranges of its three layers.
struct Built {
    verts: Vec<Packed>,
    starts: [u32; SECTIONS + 1],
    cut: [u32; SECTIONS + 1],
    wstarts: [u32; SECTIONS + 1],
    vis: [u64; SECTIONS],
}

/// The CPU half of meshing a chunk, run by a worker with its own climate noise (`cm`) for the biome tints. The mesher runs with origin 0,
/// so the vertices are chunk-local (and exact far from the world's origin); the chunk's origin is `DrawOrigins`' business.
fn build_job(job: MeshJob, cm: &mut WorldChunkManager) -> Built {
    let (x, z) = job.key;
    let [me, px, nx, pz, nz] = &job.cells;
    let (verts, idxs, widxs) = mesh::build_split_tinted(
        crate::LEGACY,
        &me.blocks[..],
        &me.data,
        [&px.data, &nx.data, &pz.data, &nz.data],
        &me.light[..],
        [&px.blocks[..], &nx.blocks[..], &pz.blocks[..], &nz.blocks[..]],
        [&px.light[..], &nx.light[..], &pz.light[..], &nz.light[..]],
        job.sky_sub,
        0,
        0,
        &biome_tints(cm, x, z),
    );
    // Sodium's layers: the faces that show a see-through tile are drawn with the alpha test, the rest without it. All three layers live in
    // one run of the page, solid first.
    let Quads { verts, starts, cut, wstarts } = pack(&verts, &idxs, &widxs);
    // Only full cubes that stop light hide what is behind them: leaves, glass, ice and water are seen through, slabs and stairs are not cubes.
    let vis = std::array::from_fn(|s| vis::compute(|x, y, z| {
        let b = me.blocks[idx(x, s * 16 + y, z)];
        opaque(b) && normal_cube(b)
    }));
    Built { verts, starts, cut, wstarts, vis }
}

struct Entry {
    /// Shared with the light thread's copy (`LChunk.blocks`) and with running mesh jobs until the first write to either side.
    blocks: Arc<Vec<u8>>,
    /// Block metadata (`Chunk.data`), 4 bits per cell; worldgen only writes it in `populate`.
    data: Nibbles,
    /// Per cell, sky light in the low nibble and block light in the high nibble (same index as `blocks`).
    light: Arc<Vec<u8>>,
    /// `Chunk.heightMap`, indexed `z << 4 | x`; kept in step with `blocks` (insert, populate, edits).
    height: [u8; 256],
    /// Initial light was computed (the light thread's `init_chunk`, reported back); meshing waits for it. `light` and `lit` are the
    /// render thread's read-only copy: only the light thread's reports write them.
    lit: bool,
    /// populate() has run on this chunk (it also wrote into its +X/+Z/+X+Z neighbours).
    populated: bool,
    /// A stamp from `ChunkManager::stamp` taken when the entry was made and at every edit or `put_region`: a populate job that was
    /// snapshotted at another stamp was computed from an older chunk and is dropped (`populate_land`).
    rev: u32,
    /// Differs from what the seed generates (edited, or written into by a populate) and is not on disk yet.
    dirty: bool,
    /// The mesh matches the chunk as it is: built, or on its way from a worker (the mesh may still be `None`: a chunk of pure air has
    /// nothing to draw). Every edit clears it, which queues a new job once the running one (if any) has landed.
    meshed: bool,
    mesh: Option<Mesh>,
}

/// Offsets of the raw ring (radius + 2) round a chunk, nearest first.
fn ring_for(radius: i32) -> Vec<Key> {
    let r = radius + 2;
    let mut ring: Vec<Key> = (-r..=r).flat_map(|dx| (-r..=r).map(move |dz| (dx, dz))).filter(|&(dx, dz)| dx * dx + dz * dz <= r * r).collect();
    ring.sort_by_key(|&(dx, dz)| dx * dx + dz * dz);
    ring
}

pub struct ChunkManager {
    chunks: Chunks,
    /// Requested from a worker, not back yet.
    pending: HashSet<Key>,
    /// Offsets of the raw ring (radius + 2), nearest first.
    ring: Vec<Key>,
    /// Render radius in chunks.
    radius: i32,
    /// Player chunk at the last unload pass.
    center: Option<Key>,
    max_in_flight: usize,
    /// `preload` (the spawn area, before the first frame) and `populate_one` run populate here; during play `update` sends it to the
    /// workers (`populate_dispatch`), each of which has a generator of its own.
    gen: OverworldGenerator,
    cm: WorldChunkManager,
    /// The last `biomes` answer and the area it was for.
    biome_area: Option<((i32, i32, usize, usize), Vec<crate::world::biome::Biome>)>,
    jobs: mpsc::Sender<Job>,
    done: mpsc::Receiver<(Key, Vec<u8>)>,
    /// Meshes the workers finished, waiting for their upload (`mesh_pending`).
    built: mpsc::Receiver<(Key, Built)>,
    /// Chunks with a mesh job queued or running (one at a time per chunk, so results never arrive out of order), and the most at once.
    meshing: HashSet<Key>,
    max_meshing: usize,
    /// Chunks an edit made stale (`mark_dirty`): their next mesh job does not wait for the light engine to settle (see `mesh_pending`).
    urgent: HashSet<Key>,
    /// Finished populate jobs, waiting to be put back (`populate_land`).
    populated_rx: mpsc::Receiver<(Key, Region)>,
    /// Populate jobs out on the workers: the chunk each populates -> the `rev` of the four chunks of its block when it was copied.
    populating: HashMap<Key, [u32; 4]>,
    /// The chunks inside those blocks: a chunk is in at most one job, so two results never overwrite each other's chunks.
    busy: HashSet<Key>,
    max_populating: usize,
    /// Source of `Entry.rev`.
    stamp: u32,
    /// The light thread (`light.rs`): it holds the light engine and its own copy of the blocks; this side tells it what changes and
    /// takes its reports.
    light: light::LightLink,
    /// `World.skylightSubtracted` (0 day .. 11 night), applied by the mesher.
    sky_sub: u8,
    /// Bumped whenever the drawn geometry changes (a mesh with another size, an unload): the cached shadow map is stale then.
    pub mesh_gen: u32,
    /// Save folder (M7); `None` = nothing is read or written (the tests). `saved` = chunks that have a file there.
    dir: Option<PathBuf>,
    saved: HashSet<Key>,
    /// The save I/O thread, there while `dir` is (`with_dir`): chunk files are written there, so the render thread only encodes.
    writer: Option<Writer>,
    /// Cells written through `set_block_meta` since the last `take_changes`: (x, y, z, old id, new id). `world::ticks`
    /// turns them into `onBlockAdded` / `onBlockRemoval` / `onNeighborBlockChange` (the `...WithNotify` setters).
    changes: Vec<(i32, i32, i32, u8, u8)>,
    /// Still-water cells at the edge of a surface (air beside them) found when a chunk was populated; `Ticks::wake_water` takes them.
    fresh_water: Vec<(i32, i32, i32)>,
    /// Dungeon chests filled by populate that the game has not taken over yet (`take_loot`).
    loot: Vec<(Pos, Chest)>,
    /// Dungeon spawners populate placed that the game has not taken over yet (`take_spawners`).
    spawners: Vec<(Pos, u8)>,
    /// The occlusion walk without the frustum, refreshed on a worker (`wide`): `update_visible` only tests it against the frustum.
    wide: wide::WideCull,
    /// What that walk reads: the `vis` of every loaded chunk's sections (a chunk without a mesh lets the view through). Every change goes
    /// through `graph.set` (a chunk loads or unloads, a mesh arrives with other occlusion data), which is what makes the walk redo itself.
    graph: wide::Snapshot,
    /// Per chunk column, the sections (bit `s`) `update_visible` found drawable this frame.
    visible: HashMap<Key, u8>,
    /// The part of `visible` within the water radius `wr` (`WATER_RANGE`, or all of it with the LoD off): the sections whose real water is drawn (and whose LoD water sheet is cut).
    wet: HashMap<Key, u8>,
    /// The visible chunks near to far (`update_visible`): the order `draw_ranges` walks, once a frame instead of once a layer.
    order: Vec<Key>,
    /// Chunks whose blocks are final, as columns for the far terrain (`world::lod`, Voxy's ingest), until `take_ingest`.
    ingest: Vec<(Key, crate::world::lod::Cols)>,
    /// The chunks handed over while loaded: one that unloads is handed over again (it may have been edited), any other is not (its
    /// neighbours are gone by then, it cannot tell whether it was final).
    ingested: HashSet<Key>,
    /// The GPU buffers of every chunk mesh (`Mesh`), created on the first upload.
    pages: Pages,
    /// A finished mesh the upload budget held back: the next `mesh_pending` takes it first. Its chunk stays in `meshing` meanwhile.
    held: Option<(Key, Built)>,
    /// What an upload of n bytes costs (a linear fit to the measured `Pages::put`), and the average frame length (ns) between
    /// `update` calls: the two numbers the frame's `UploadBudget` is made of.
    upload: UploadDurationEstimator,
    frame_ns: Option<f64>,
    last_update: Option<std::time::Instant>,
}

impl ChunkManager {
    /// `radius` is the render distance in chunks. Dropping the manager closes the job channel,
    /// which ends the workers.
    pub fn new(seed: i64, radius: i32) -> Self {
        Self::new_typed(seed, radius, WorldType::Vanilla)
    }

    /// `new` with the world type (`WorldType::generator`) in every worker and for `preload`.
    pub fn new_typed(seed: i64, radius: i32, ty: WorldType) -> Self {
        let make: fn(i64) -> OverworldGenerator = match ty {
            WorldType::Vanilla => OverworldGenerator::world,
            WorldType::Tectonic => OverworldGenerator::world_tectonic,
            WorldType::Blooming => OverworldGenerator::world_blooming,
        };
        hydro::watch(Some(hydro::Hydro::of(seed, ty == WorldType::Tectonic))); // the currents of this world (Blooming's are Vanilla's)
        Self::new_with(seed, radius, make)
    }

    /// The Nether: the same manager with `gen::nether` in every worker (no sky light comes out of it by itself: the ceiling is solid
    /// bedrock, so every column's height map is 128).
    pub fn new_nether(seed: i64, radius: i32) -> Self {
        hydro::watch(None);
        Self::new_with(seed, radius, OverworldGenerator::new_nether)
    }

    fn new_with(seed: i64, radius: i32, make: fn(i64) -> OverworldGenerator) -> Self {
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (done_tx, done) = mpsc::channel();
        let (built_tx, built) = mpsc::channel();
        let (pop_tx, populated_rx) = mpsc::channel();
        let job_rx = Arc::new(Mutex::new(job_rx));
        // Leave a core for the render thread; two workers are plenty for walking speed.
        let workers = std::thread::available_parallelism().map_or(2, |n| n.get()).saturating_sub(1).clamp(1, 2);
        for i in 0..workers {
            let (rx, tx, bt, pt) = (job_rx.clone(), done_tx.clone(), built_tx.clone(), pop_tx.clone());
            std::thread::Builder::new()
                .name(format!("chunk-gen-{i}"))
                .spawn(move || {
                    crate::world::nice(5); // generation, populate and meshing stay behind the render thread when cores run short
                    // Each worker owns its generator: generate() reseeds per chunk, so the
                    // result does not depend on which worker gets which chunk.
                    let mut g = make(seed);
                    let mut cm = g.manager();
                    loop {
                        // The guard is a temporary of this statement: the lock is held while
                        // waiting for a job, not while generating.
                        let job = rx.lock().unwrap_or_else(|e| e.into_inner()).recv();
                        let Ok(job) = job else { break };
                        let sent = match job {
                            Job::Gen((cx, cz)) => tx.send(((cx, cz), g.generate(cx, cz, &mut cm))).is_ok(),
                            Job::Mesh(j) => {
                                let key = j.key;
                                bt.send((key, build_job(*j, &mut cm))).is_ok()
                            }
                            Job::Populate(j) => {
                                // populate() draws from a Random it seeds from the chunk, so the result does not depend on the worker.
                                let PopJob { x, z, mut region } = *j;
                                g.populate(&mut region, x, z, &mut cm);
                                pt.send(((x, z), region)).is_ok()
                            }
                        };
                        if !sent {
                            break;
                        }
                    }
                })
                .expect("spawn chunk worker");
        }

        let ring = ring_for(radius);

        let gen = make(seed);
        Self { chunks: Chunks::default(), pending: HashSet::new(), ring, radius, center: None, max_in_flight: workers * 2, built, meshing: HashSet::new(), max_meshing: workers * 2, urgent: HashSet::new(),
               populated_rx, populating: HashMap::new(), busy: HashSet::new(), max_populating: workers, stamp: 0,
               cm: gen.manager(), biome_area: None, gen, jobs, done, light: light::LightLink::spawn(), sky_sub: 0, mesh_gen: 0, dir: None, saved: HashSet::new(), writer: None, changes: Vec::new(), fresh_water: Vec::new(), loot: Vec::new(), spawners: Vec::new(), wide: wide::WideCull::spawn(), graph: wide::Snapshot::default(), visible: HashMap::new(), wet: HashMap::new(), order: Vec::new(), ingest: Vec::new(), ingested: HashSet::new(), pages: Pages::default(), held: None, upload: UploadDurationEstimator::new(), frame_ns: None, last_update: None }
    }

    /// The render distance in chunks.
    pub fn radius(&self) -> i32 {
        self.radius
    }

    /// A new render distance (the Video Settings option): the ring that is generated and kept follows at the next `update`.
    pub fn set_radius(&mut self, radius: i32) {
        (self.radius, self.ring) = (radius, ring_for(radius));
    }

    /// Keep this world in `dir`: chunks found there are loaded instead of generated, and edited ones are written back
    /// (`flush`, and when they unload).
    pub fn with_dir(mut self, dir: PathBuf) -> Self {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::warn!("save: cannot create {}: {e}", dir.display());
        }
        self.saved = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|f| {
                let name = f.file_name().into_string().ok()?;
                let (x, z) = name.strip_prefix("c.")?.split_once('.')?;
                Some((x.parse().ok()?, z.parse().ok()?)) // "c.1.2.tmp" and other strays do not parse
            })
            .collect();
        self.dir = Some(dir);
        self.writer = Some(Writer::spawn());
        self
    }

    pub(crate) fn insert(&mut self, key: Key, blocks: Vec<u8>) {
        let height = height_map(&blocks);
        self.stamp = self.stamp.wrapping_add(1);
        let blocks = Arc::new(blocks);
        let copy = Arc::clone(&blocks); // for the light thread: one array until either side writes
        let old = self.chunks.insert(key, Entry { blocks, data: Nibbles::new(), light: Arc::new(vec![0; VOLUME]), height, lit: false, populated: false, rev: self.stamp, dirty: false, meshed: false, mesh: None });
        if let Some(old) = old {
            old.mesh.iter().for_each(|m| self.pages.free(m));
            self.light_remove(key); // a fresh, unlit entry replaces it, light thread's copy included
        }
        self.graph.set(key, Some([vis::FULL; SECTIONS])); // no mesh yet: the view passes through
        self.light_chunk(key, copy);
    }

    /// Load a saved chunk. A file that does not decode is forgotten, so the chunk is generated instead.
    fn load(&mut self, key: Key) {
        // A write still queued is newer than the file: the chunk may have been unloaded and walked back to before it landed.
        let queued = self.writer.as_ref().and_then(|w| w.inflight.get(&key)).map(|(_, b)| b.to_vec());
        let bytes = queued.or_else(|| self.dir.as_ref().and_then(|d| std::fs::read(chunk_path(d, key)).ok()));
        match bytes.as_deref().and_then(save::decode_chunk) {
            Some((blocks, data, populated)) => {
                self.insert(key, blocks);
                if let Some(e) = self.chunks.get_mut(&key) {
                    e.data = data;
                    e.populated = populated;
                }
                self.settle_around(key);
            }
            None => {
                log::warn!("save: chunk {key:?} unreadable, generating it again");
                self.saved.remove(&key);
            }
        }
    }

    /// The chunk from disk when there is a save of it, else freshly generated (synchronously).
    fn fresh(&mut self, key: Key) {
        if self.saved.contains(&key) {
            self.load(key);
        }
        if !self.chunks.contains_key(&key) {
            let blocks = self.gen.generate(key.0, key.1, &mut self.cm);
            self.insert(key, blocks);
        }
    }

    /// Queue up to `budget` unsaved chunks for the I/O thread (`usize::MAX` = all, for pause and exit: that one also waits until
    /// every queued write has landed, so the process may end right after). A write that fails leaves its chunk dirty again for the
    /// next call (`reap_writes`).
    pub fn flush(&mut self, mut budget: usize) {
        let all = budget == usize::MAX;
        self.reap_writes(false);
        let (Some(dir), Some(w)) = (&self.dir, &mut self.writer) else { return };
        for (&k, e) in self.chunks.iter_mut().filter(|(_, e)| e.dirty) {
            if budget == 0 {
                break;
            }
            budget -= 1;
            w.queue(dir, k, save::encode_chunk(&e.blocks, &e.data, e.populated));
            e.dirty = false;
            self.saved.insert(k);
        }
        if all {
            self.reap_writes(true);
        }
    }

    /// Take the answers of the I/O thread (`wait`: until every queued write has been answered). A chunk whose write failed is
    /// dirty again if it is still loaded (the next `flush` retries it); one already unloaded is lost, as it was when the write ran
    /// here. One log line per call, so a full disk does not flood the log.
    fn reap_writes(&mut self, wait: bool) {
        let failed = self.writer.as_mut().map_or_else(Vec::new, |w| w.reap(wait));
        let Some((k, err)) = failed.first() else { return };
        log::warn!("save: {} chunk write(s) failed, the first {k:?}: {err}", failed.len());
        for (k, _) in &failed {
            if let Some(e) = self.chunks.get_mut(k) {
                e.dirty = true;
            }
        }
    }

    /// Generate (or load) and populate the 4x4 chunks `cx-1..=cx+2`, `cz-1..=cz+2` synchronously, for the spawn area:
    /// init needs real terrain before the first frame. Only chunks whose 2x2 is inside get populated, which finishes
    /// the 2x2 chunks `cx..=cx+1`, `cz..=cz+1`.
    pub fn preload(&mut self, cx: i32, cz: i32) {
        for z in cz - 1..=cz + 2 {
            for x in cx - 1..=cx + 2 {
                self.fresh((x, z));
            }
        }
        for z in cz - 1..cz + 2 {
            for x in cx - 1..cx + 2 {
                self.populate_one(x, z);
            }
        }
    }

    /// Block at world coordinates. `None` above/below the world (like the old bounds check);
    /// a chunk that is not loaded reads as solid, so the player is walled in at the edge of the
    /// loaded area instead of walking or falling into void.
    pub fn block(&self, x: i32, y: i32, z: i32) -> Option<u8> {
        (0..H as i32).contains(&y).then(|| self.block_loaded(x, y, z).unwrap_or(1))
    }

    /// Block at world coordinates, `None` when out of the world or the chunk is not loaded (picking
    /// must not hit the invisible wall `block` builds at the edge).
    pub fn block_loaded(&self, x: i32, y: i32, z: i32) -> Option<u8> {
        if !(0..H as i32).contains(&y) {
            return None;
        }
        self.chunks.get(&(x >> 4, z >> 4)).map(|e| e.blocks[idx((x & 15) as usize, y as usize, (z & 15) as usize)])
    }

    /// Raw (sky, block) light of a cell, what the mesher's `lm` takes: entities are lit by the cell they are in
    /// (`Entity.getBrightnessForRender`). Open sky above the world and unloaded chunks read as full sky light.
    pub fn light_levels(&self, x: i32, y: i32, z: i32) -> (u8, u8) {
        if !(0..H as i32).contains(&y) {
            return (if y >= H as i32 { 15 } else { 0 }, 0);
        }
        self.chunks.get(&(x >> 4, z >> 4)).map_or((15, 0), |e| {
            let l = e.light[idx((x & 15) as usize, y as usize, (z & 15) as usize)];
            (l & 15, l >> 4)
        })
    }

    /// `World.getBlockLightValue`: the brighter of block light and sky light less `skylightSubtracted`, 0..=15; the open sky
    /// above the world and unloaded chunks read as full daylight.
    pub fn light(&self, x: i32, y: i32, z: i32) -> u8 {
        let day = 15u8.saturating_sub(self.sky_sub);
        if !(0..H as i32).contains(&y) {
            return if y >= H as i32 { day } else { 0 };
        }
        self.chunks.get(&(x >> 4, z >> 4)).map_or(day, |e| {
            let l = e.light[idx((x & 15) as usize, y as usize, (z & 15) as usize)];
            (l & 15).saturating_sub(self.sky_sub).max(l >> 4)
        })
    }

    /// Block metadata at world coordinates (`World.getBlockMetadata`); 0 above/below the world and in an
    /// unloaded chunk. A caller that breaks a block reads this first: `set_block` clears it.
    pub fn meta(&self, x: i32, y: i32, z: i32) -> u8 {
        if !(0..H as i32).contains(&y) {
            return 0;
        }
        self.chunks.get(&(x >> 4, z >> 4)).map_or(0, |e| e.data.get((x & 15) as usize, y as usize, (z & 15) as usize))
    }

    /// `physics_id(b, meta)` of the cell holding block `b`; the metadata is only read where physics uses it (Radium
    /// `util.chunk_access` idea: fewer chunk lookups per cell of a collision query).
    pub fn physics_at(&self, x: i32, y: i32, z: i32, b: u8) -> u8 {
        physics_id(b, if meta_matters(b) { self.meta(x, y, z) } else { 0 })
    }

    /// Break or place a block with metadata 0 (`Chunk.setBlockID`): nothing when the cell already holds `id`,
    /// else the id is written and the metadata cleared. False if nothing changed.
    pub fn set_block(&mut self, x: i32, y: i32, z: i32, id: u8) -> bool {
        self.block_loaded(x, y, z) != Some(id) && self.set_block_meta(x, y, z, id, 0)
    }

    /// Place a block with metadata (`Chunk.setBlockIDWithMetadata`, `meta` is 0..=15): writes the cell, keeps
    /// the height map and light in step and marks the affected meshes for a rebuild. False if nothing changed.
    pub fn set_block_meta(&mut self, x: i32, y: i32, z: i32, id: u8, meta: u8) -> bool {
        let old = self.block_loaded(x, y, z);
        let done = self.set_quiet(x, y, z, id, meta);
        if done {
            self.changes.push((x, y, z, old.unwrap_or(0), id));
        }
        done
    }

    /// `World.notifyBlocksOfNeighborChange(x, y, z)`: the six neighbours of the cell hear of a change, nothing is written. The log entry
    /// carries the sentinel id 255 (no block), so `ticks` reads it as "something that may provide power changed here".
    pub fn ping(&mut self, x: i32, y: i32, z: i32) {
        self.changes.push((x, y, z, 255, 255));
    }

    /// `World.setBlockAndMetadata` / `setBlockMetadata`: the same write without telling the neighbours.
    pub fn set_quiet(&mut self, x: i32, y: i32, z: i32, id: u8, meta: u8) -> bool {
        if !(0..H as i32).contains(&y) {
            return false;
        }
        let Some(e) = self.chunks.get_mut(&(x >> 4, z >> 4)) else { return false };
        let (lx, lz) = ((x & 15) as usize, (z & 15) as usize);
        let i = idx(lx, y as usize, lz);
        let meta = meta & 15;
        if e.blocks[i] == id && e.data.get(lx, y as usize, lz) == meta {
            return false;
        }
        Arc::make_mut(&mut e.blocks)[i] = id;
        e.data.set(lx, y as usize, lz, meta);
        e.dirty = true;
        self.stamp = self.stamp.wrapping_add(1);
        e.rev = self.stamp;
        let h = e.height[lz << 4 | lx] as i32;
        // The column's height is kept here (readers such as `sees_sky` need it at once); the light thread does the same on its copy
        // and relights.
        if let Some(top) = light::height_after_set(&e.blocks, lx, lz, id, y, h) {
            e.height[lz << 4 | lx] = top as u8;
        }
        self.mark_dirty(x, z);
        self.light_edit(x, y, z, id);
        true
    }

    /// Every loaded cell holding `id` (the portal search of `Teleporter.func_4106_b`, which scans the loaded chunks only here).
    pub fn find(&self, id: u8) -> Vec<(i32, i32, i32)> {
        let mut out = Vec::new();
        for (&(cx, cz), e) in &self.chunks {
            for (i, _) in e.blocks.iter().enumerate().filter(|&(_, &b)| b == id) {
                out.push((cx * 16 + (i >> 11) as i32, (i & 127) as i32, cz * 16 + ((i >> 7) & 15) as i32));
            }
        }
        out
    }

    pub fn take_changes(&mut self) -> Vec<(i32, i32, i32, u8, u8)> {
        std::mem::take(&mut self.changes)
    }

    /// The edge water queued by `queue_edge_water` since the last call.
    pub fn take_fresh_water(&mut self) -> Vec<(i32, i32, i32)> {
        std::mem::take(&mut self.fresh_water)
    }

    /// Streams Reflowing's `scheduleTick` on the water at the edge of a carve: the still-water sources of chunk `k` that lie on a
    /// surface (air above) with air beside them. Left alone they end in a wall (a river's surface a block lower than its neighbour's,
    /// a channel against a cut bank); woken, the fluid sim pours them down and spreads them into levels, which is the slope. A
    /// neighbour that is not loaded is not an edge yet: the chunk beside it rescans the cell when it is populated.
    fn edge_water(&self, k: Key) -> Vec<(i32, i32, i32)> {
        let Some(e) = self.chunks.get(&k) else { return Vec::new() };
        let mut out = Vec::new();
        for lx in 0..16usize {
            for lz in 0..16usize {
                let col = idx(lx, 0, lz);
                for y in 1..H - 1 {
                    if e.blocks[col + y] != 9 || e.blocks[col + y + 1] != 0 || e.data.get(lx, y, lz) != 0 {
                        continue;
                    }
                    let (wx, wz) = (k.0 * 16 + lx as i32, k.1 * 16 + lz as i32);
                    if [(-1, 0), (1, 0), (0, -1), (0, 1)].iter().any(|&(dx, dz)| self.block_loaded(wx + dx, y as i32, wz + dz) == Some(0)) {
                        out.push((wx, y as i32, wz));
                    }
                }
            }
        }
        out
    }

    /// `edge_water` of the four chunks a populate of (x, z) wrote into, queued for the tick system (capped: nothing drains it in a tool).
    fn queue_edge_water(&mut self, x: i32, z: i32) {
        for k in [(x, z), (x + 1, z), (x, z + 1), (x + 1, z + 1)] {
            if self.fresh_water.len() < 50_000 {
                let found = self.edge_water(k);
                self.fresh_water.extend(found);
            }
        }
    }

    /// `World.getBlockLightValue` (`sub` = `skylightSubtracted`, 0 for `getFullBlockLightValue`): the brighter of sky and block light.
    pub fn light_at(&self, x: i32, y: i32, z: i32, sub: u8) -> u8 {
        if y < 0 {
            return 0;
        }
        let y = y.min(H as i32 - 1) as usize;
        self.chunks.get(&(x >> 4, z >> 4)).map_or(0, |e| {
            let l = e.light[idx((x & 15) as usize, y, (z & 15) as usize)];
            (l & 15).saturating_sub(sub).max(l >> 4)
        })
    }

    /// Light with the current day/night subtraction (`getBlockLightValue`).
    pub fn light_level(&self, x: i32, y: i32, z: i32) -> u8 {
        self.light_at(x, y, z, self.sky_sub)
    }

    /// `World.tick`'s random block ticks: 80 random cells in every lit, final chunk within `r` chunks of `center`, with the Java's
    /// own LCG. Returns the cells whose block ticks at random (`Block.tickOnLoad`).
    pub fn random_ticks(&self, center: Key, r: i32, lcg: &mut i32, rng: &mut JavaRandom) -> (Vec<(i32, i32, i32, u8)>, Vec<(i32, i32)>) {
        let (mut out, mut columns) = (Vec::new(), Vec::new());
        for (&(cx, cz), e) in &self.chunks {
            if (cx - center.0).abs() > r || (cz - center.1).abs() > r || !e.lit || !self.is_final(cx, cz) {
                continue;
            }
            // The snow / ice column of `World.tick`: one chunk in sixteen per tick, a column drawn from the same LCG.
            if rng.next_int_bound(16) == 0 {
                *lcg = lcg.wrapping_mul(3).wrapping_add(1013904223);
                let v = *lcg >> 2;
                columns.push((cx * 16 + (v & 15), cz * 16 + (v >> 8 & 15)));
            }
            for _ in 0..80 {
                *lcg = lcg.wrapping_mul(3).wrapping_add(1013904223);
                let v = *lcg >> 2;
                let (x, z, y) = ((v & 15) as usize, (v >> 8 & 15) as usize, (v >> 16 & 127) as usize);
                let id = e.blocks[idx(x, y, z)];
                if crate::world::ticks::ticks_at_random(id) {
                    out.push((cx * 16 + x as i32, y as i32, cz * 16 + z as i32, id));
                }
            }
        }
        (out, columns)
    }

    /// `World.getSavedLightValue(EnumSkyBlock.Block, ..)`.
    pub fn block_light(&self, x: i32, y: i32, z: i32) -> u8 {
        if y < 0 || y >= H as i32 {
            return 0;
        }
        self.chunks.get(&(x >> 4, z >> 4)).map_or(0, |e| e.light[idx((x & 15) as usize, y as usize, (z & 15) as usize)] >> 4)
    }

    /// `BlockSapling.growTree` in a 2x2 chunk region around (x, z) (a tree reaches at most 8 cells out); false when one of the
    /// four chunks is missing or the tree does not fit. ponytail: the four chunks are lit again as after a populate, one
    /// light pass per grown tree; a per-cell light update would be cheaper.
    pub fn grow_tree(&mut self, x: i32, y: i32, z: i32, species: u8, rng: &mut JavaRandom) -> bool {
        let (cx, cz) = ((x - 8) >> 4, (z - 8) >> 4);
        let Some(mut region) = self.take_region(cx, cz) else { return false };
        let ok = crate::world::gen::populate::grow_sapling(&mut region, rng, x, y, z, species);
        self.put_region(cx, cz, region);
        ok
    }

    /// The four chunks populate / tree growth write into, moved out into a `Region`.
    fn take_region(&mut self, x: i32, z: i32) -> Option<Region> {
        let keys = [(x, z), (x, z + 1), (x + 1, z), (x + 1, z + 1)];
        if !keys.iter().all(|k| self.chunks.contains_key(k)) {
            return None;
        }
        let blocks = keys.map(|k| Arc::unwrap_or_clone(std::mem::take(&mut self.chunks.get_mut(&k).unwrap().blocks)));
        let data = keys.map(|k| std::mem::take(&mut self.chunks.get_mut(&k).unwrap().data));
        Some(Region::new(x, z, blocks, data))
    }

    /// The chests populate filled since the last call; the game puts them into its tile entities.
    pub fn take_loot(&mut self) -> Vec<(Pos, Chest)> {
        std::mem::take(&mut self.loot)
    }

    /// The spawners populate placed since the last call; the game puts them into its tile entities.
    pub fn take_spawners(&mut self) -> Vec<(Pos, u8)> {
        std::mem::take(&mut self.spawners)
    }

    fn put_region(&mut self, x: i32, z: i32, mut region: Region) {
        self.loot.extend(region.take_loot());
        self.spawners.extend(region.take_spawners());
        let keys = [(x, z), (x, z + 1), (x + 1, z), (x + 1, z + 1)];
        let (blocks, data) = region.into_parts();
        for ((k, b), d) in keys.iter().zip(blocks).zip(data) {
            let e = self.chunks.get_mut(k).unwrap();
            e.height = height_map(&b);
            e.blocks = Arc::new(b);
            e.data = d;
            e.dirty = true; // a populate writes into all four (trees, ores, ... spill over the borders)
            self.stamp = self.stamp.wrapping_add(1);
            e.rev = self.stamp; // a populate job copied from before this is stale
            e.lit = false; // blocks changed under the light: it is computed again once final
            let copy = Arc::clone(&e.blocks);
            self.light_chunk(*k, copy); // the light thread gets the new blocks (and forgets the chunk was lit)
            // The chunk and its four neighbours (border faces) need a new mesh if they had one.
            for n in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                if let Some(e) = self.chunks.get_mut(&(k.0 + n.0, k.1 + n.1)) {
                    e.meshed = false;
                }
            }
        }
    }

    /// The chunk holding world column (x, z) needs a new mesh, and so does the neighbour whose border
    /// faces look at it when the column is on the chunk edge.
    fn mark_dirty(&mut self, x: i32, z: i32) {
        let (cx, cz, lx, lz) = (x >> 4, z >> 4, x & 15, z & 15);
        // At most one neighbour per axis, so three keys at most; an array, because this runs for every cell whose light changes.
        let mut keys = [(cx, cz); 3];
        let mut n = 1;
        for (hit, k) in [(lx == 0, (cx - 1, cz)), (lx == 15, (cx + 1, cz)), (lz == 0, (cx, cz - 1)), (lz == 15, (cx, cz + 1))] {
            if hit {
                keys[n] = k;
                n += 1;
            }
        }
        for k in &keys[..n] {
            if let Some(e) = self.chunks.get_mut(k) {
                e.meshed = false;
                self.urgent.insert(*k);
            }
        }
    }

    /// Chunk C is final when populate has run on C, C-x, C-z and C-x-z.
    fn is_final(&self, x: i32, z: i32) -> bool {
        [(x, z), (x - 1, z), (x, z - 1), (x - 1, z - 1)].iter().all(|k| self.chunks.get(k).is_some_and(|e| e.populated))
    }

    /// Time of day changed the light. In the legacy look the meshes stay: no vertex its shaders read depends on it (`lm` carries
    /// the raw levels, the sky-light the day takes away is the `amb.x` uniform), so a rebuild would only repeat the same vertices.
    /// It used to restart every mesh (`updateAllRenderers`): the whole ring, a couple per frame, at every sky-light step.
    /// Only the Photon water shader (`render/water.rs`, wave amplitude) still reads the baked light, so without `LEGACY` they restart.
    pub fn set_sky_sub(&mut self, v: u8) {
        let stale = !crate::LEGACY && v != self.sky_sub;
        self.sky_sub = v;
        if stale {
            self.chunks.values_mut().for_each(|e| e.meshed = false);
        }
    }

    /// The biome and climate (`temperature`, `humidity`, both 0..1) of a block column, for the pack's weather and fog.
    pub fn climate_at(&mut self, x: i32, z: i32) -> (crate::world::biome::Biome, f64, f64) {
        let biome = self.cm.load_block_generator_data(x, z, 1, 1)[0];
        (biome, self.cm.temperature[0], self.cm.humidity[0])
    }

    /// `WorldChunkManager.func_4069_a`: the biomes of a `w` x `d` area from (x, z), x outer, z inner (what `renderRainSnow` walks).
    pub fn biomes(&mut self, x: i32, z: i32, w: usize, d: usize) -> Vec<crate::world::biome::Biome> {
        // The rain / snow pass asks for the same area every frame until the player steps to the next block, and the climate noise behind
        // it is the costly part. The biomes are a pure function of the seed and the area, so the last answer is reused.
        if let Some((k, v)) = &self.biome_area {
            if *k == (x, z, w, d) {
                return v.clone();
            }
        }
        let v = self.cm.load_block_generator_data(x, z, w, d);
        self.biome_area = Some(((x, z, w, d), v.clone()));
        v
    }

    /// `World.findTopSolidBlock`: one above the highest solid or liquid cell of the loaded column, -1 if none.
    pub fn top_solid(&self, x: i32, z: i32) -> i32 {
        use crate::world::gen::populate::{is_liquid, is_solid};
        // One chunk lookup, then the column itself: `y` is the fastest index, so a column is `H` contiguous bytes (it used to be one hash
        // lookup per block, 441 columns a frame while it rains).
        let Some(e) = self.chunks.get(&(x >> 4, z >> 4)) else { return -1 };
        let base = idx((x & 15) as usize, 0, (z & 15) as usize);
        e.blocks[base + 1..base + H].iter().rposition(|&b| is_solid(b) || is_liquid(b)).map_or(-1, |i| i as i32 + 2)
    }

    /// Everything that has a mesh, for the draw loop.
    pub fn meshes(&self) -> impl Iterator<Item = &Mesh> {
        self.chunks.values().filter_map(|e| e.mesh.as_ref())
    }

    /// The meshes whose chunk box (16 x `H` x 16 at its key) passes `visible(min, max)`: the draw loop's frustum test.
    pub fn meshes_where<'a>(&'a self, visible: impl Fn(Vec3, Vec3) -> bool + 'a) -> impl Iterator<Item = &'a Mesh> {
        self.chunks.iter().filter_map(move |(&(x, z), e)| {
            let min = Vec3::new(x as f32 * 16.0, 0.0, z as f32 * 16.0);
            e.mesh.as_ref().filter(|_| visible(min, min + Vec3::new(16.0, H as f32, 16.0)))
        })
    }

    /// Call once per frame before `draw_ranges`: finds the sections to draw from the eye (`wr` = radius of the real water, `WATER_RANGE` or `1e9`).
    /// The sections come from a breadth-first walk out of the eye's section (`embeddium::cull`) that stays inside the `shape` (radii `rh`,
    /// `rv` in blocks) and does not continue through terrain that walls the view off, so caves and hills hide what is behind them. That walk
    /// does not look at the frustum or where in its section the eye is, so it is redone only when the eye changes section or a chunk's
    /// occlusion data changes, on a worker (`wide`); this frame's part is testing it against the frustum (`in_frustum(min, max)`) and the real
    /// eye. Each section's box is one block taller at both ends. With the eye in a chunk that is not loaded there is nowhere to start: every
    /// section is tested instead.
    /// ponytail: a hole in the loaded ring (a chunk still generating) cuts the walk there; it fills in as the chunk arrives. After a change
    /// the walk in use is a frame or two old: a chunk whose mesh just arrived is drawn from the next walk on.
    pub fn update_visible(&mut self, eye: Vec3, shape: Shape, rh: f32, rv: f32, wr: f32, in_frustum: impl Fn(Vec3, Vec3) -> bool) {
        let ey = (eye.y.floor() as i32) >> 4;
        let inside = (0..SECTIONS as i32).contains(&ey); // above or below the world the eye's section says nothing about the view
        let origin = [chunk_coord(eye.x), ey.clamp(0, SECTIONS as i32 - 1), chunk_coord(eye.z)];
        let wide = self.wide.get(&self.graph, wide::WideKey { origin, inside, shape, rh, rv });
        self.visible.clear();
        for &((x, z), mask) in &wide.cols {
            // The column's box holds those of its sections: most columns of a wide walk are behind the camera or out of the side.
            let min = Vec3::new(x as f32 * 16.0, -1.0, z as f32 * 16.0);
            let max = min + Vec3::new(16.0, H as f32 + 2.0, 16.0);
            if !in_range(shape, eye, min, max, rh, rv) || !in_frustum(min, max) {
                continue;
            }
            let seen = (0..SECTIONS as i32).filter(|&y| mask >> y & 1 != 0).fold(0u8, |m, y| {
                let min = Vec3::new(x as f32 * 16.0, y as f32 * 16.0 - 1.0, z as f32 * 16.0);
                let max = min + Vec3::new(16.0, 18.0, 16.0);
                if in_range(shape, eye, min, max, rh, rv) && in_frustum(min, max) { m | 1u8 << y } else { m }
            });
            if seen != 0 {
                self.visible.insert((x, z), seen);
            }
        }
        self.wet = self.visible.iter().filter_map(|(&(x, z), &m)| Some(((x, z), near_sections(x, z, m, eye, shape, wr.min(rh), rv))).filter(|e| e.1 != 0)).collect();
        let d2 = |k: &Key| ((k.0 * 16 + 8) as f32 - eye.x).powi(2) + ((k.1 * 16 + 8) as f32 - eye.z).powi(2);
        self.order.clear();
        self.order.extend(self.visible.keys().copied());
        self.order.sort_unstable_by(|a, b| d2(a).total_cmp(&d2(b)));
    }

    /// Whether an entity's box (`min`..`max`, world coordinates) touches a section `update_visible` found this frame: Sodium's
    /// `isBoxVisible` (`RenderSectionManager`, used by its entity and block-entity culling), so a mob in a cave behind a hill is not built.
    /// The walk lists every section it enters, air ones too, so a box in an empty section of a reachable room counts as visible. A box that
    /// reaches above or below the world, or lies in a chunk that is not loaded, or a frame before the first `update_visible`, is visible:
    /// there is nothing to judge it by.
    /// ponytail: a hole in the loaded ring cuts the walk (see `update_visible`), so an entity behind the hole is hidden until the chunk loads.
    pub fn box_visible(&self, min: Vec3, max: Vec3) -> bool {
        if self.visible.is_empty() {
            return true;
        }
        let (sy0, sy1) = ((min.y.floor() as i32) >> 4, (max.y.floor() as i32) >> 4);
        if sy0 < 0 || sy1 >= SECTIONS as i32 {
            return true;
        }
        let bits = (sy0..=sy1).fold(0u8, |m, s| m | 1u8 << s);
        for cx in chunk_coord(min.x)..=chunk_coord(max.x) {
            for cz in chunk_coord(min.z)..=chunk_coord(max.z) {
                match self.visible.get(&(cx, cz)) {
                    Some(&m) if m & bits != 0 => return true,
                    None if !self.chunks.contains_key(&(cx, cz)) => return true,
                    _ => {}
                }
            }
        }
        false
    }

    /// Changes whenever what the far terrain is cut by changes: the set of chunks that have a mesh (`drawn`). It used to add the set of
    /// sections drawn (`vis_gen`), which changes with every turn of the camera and recut every near tile (a lookup and a range test
    /// per cell, thousands of cells a tile) each time; `lod::covered` has not read the sections since the water sheets stopped being cut.
    pub fn cut_gen(&self) -> u32 {
        self.mesh_gen
    }

    /// The real WATER of section `sy` of chunk `k` has something within `WATER_RANGE` this frame (`update_visible`) and its chunk has a mesh.
    pub fn shown(&self, k: Key, sy: i32) -> bool {
        self.drawn(k) && self.wet.get(&k).is_some_and(|m| (0..SECTIONS as i32).contains(&sy) && m >> sy & 1 != 0)
    }

    /// The main pass's draw loop for the wgpu renderer: `for_each_run`, binding the shared index buffer once, a page's vertex buffer once per
    /// page, and a chunk's origin (`origins`, group 1) once per chunk.
    pub fn draw_ranges(&self, layer: Layer, rp: &mut wgpu::RenderPass<'_>, origins: &DrawOrigins) {
        let (mut page, mut slot, mut quads) = (None, None, false);
        self.for_each_run(layer, |r| {
            if !quads {
                rp.set_index_buffer(r.quads.wgpu().slice(..), wgpu::IndexFormat::Uint32);
                quads = true;
            }
            if page != Some(r.page) {
                rp.set_vertex_buffer(0, r.vbuf.wgpu().slice(..));
                page = Some(r.page);
            }
            if slot != Some(r.slot) {
                rp.set_bind_group(1, &origins.group, &[DrawOrigins::offset(r.slot)]);
                slot = Some(r.slot);
            }
            rp.draw_indexed(r.first..r.end, 0, 0..1); // the shared indices name the page's vertices by their absolute number
        });
    }

    /// Every run of neighbouring sections `update_visible` found (empty sections are skipped, touching ones are one range, so a chunk is
    /// mostly one run), for one render `Layer`, in draw order. Both renderers walk this. Opaque terrain goes near to far inside each page, so
    /// what is drawn first hides the rest from the depth test (early-Z / the tiler's hidden-surface removal); translucent water blends over
    /// what is behind it, so it goes far to near across pages (with the LoD on it only reaches `WATER_RANGE`, a few chunks).
    /// ponytail: whole chunks only, the faces inside one chunk keep map order; upgrade = sort the sections / faces too.
    pub fn for_each_run(&self, layer: Layer, mut f: impl FnMut(Run)) {
        let water = layer == Layer::Water;
        let masks = if water { &self.wet } else { &self.visible }; // real water only within `wr` (`update_visible`)
        let Some(quads) = self.pages.quads.as_ref() else { return };
        // One chunk: its runs of sections in `layer`, only if it lives in page `only` (None = any).
        let mut visit = |k: &Key, only: Option<u32>| {
            let (Some(&mask), Some(e)) = (masks.get(k), self.chunks.get(k)) else { return };
            let Some(m) = e.mesh.as_ref().filter(|m| only.map_or(true, |p| p == m.page)) else { return };
            let starts = match (layer, &m.water) {
                (Layer::Water, Some(w)) => w,
                (Layer::Water, None) => return,
                (Layer::Solid, _) => &m.starts,
                (Layer::Cutout, _) => &m.cut,
            };
            let Some(p) = self.pages.get(m.page) else { return };
            let mut run: Option<(u32, u32)> = None;
            for s in 0..=SECTIONS {
                let on = s < SECTIONS && starts[s + 1] > starts[s] && mask >> s & 1 != 0;
                match (on, run) {
                    (true, None) => run = Some((starts[s], starts[s + 1])),
                    (true, Some((a, _))) => run = Some((a, starts[s + 1])),
                    (false, Some((a, b))) => {
                        f(Run { vbuf: &p.vbuf, quads, page: m.page, slot: m.slot, origin: m.origin, first: a, end: b });
                        run = None;
                    }
                    _ => {}
                }
            }
        };
        if water {
            for k in self.order.iter().rev() {
                visit(k, None);
            }
        } else {
            for p in &self.pages.list {
                for k in &self.order {
                    visit(k, Some(p.id));
                }
            }
        }
    }

    /// Chunks that currently have something to draw.
    pub fn meshed(&self) -> usize {
        self.meshes().count()
    }

    pub fn loaded(&self) -> usize {
        self.chunks.len()
    }

    /// Call once per frame with the player's chunk.
    pub fn update(&mut self, gfx: Gfx, origins: Option<&DrawOrigins>, cx: i32, cz: i32) {
        let t0 = std::time::Instant::now();
        if let Some(last) = self.last_update.replace(t0) {
            self.frame_ns = Some(upload_budget::next_average_frame(self.frame_ns, t0.duration_since(last).as_nanos() as f64));
        }
        self.stream(cx, cz);
        self.populate_land();
        self.populate_dispatch(cx, cz);
        let populated = t0.elapsed();
        self.light_pending(cx, cz);
        let lit = t0.elapsed();
        // A new chunk waits for the light thread to settle, so it is meshed once, lit; an edited one does not (`mesh_pending`).
        let built = self.mesh_pending(gfx, origins, cx, cz, self.light_settled());
        let total = t0.elapsed();
        if total >= SLOW_CHUNK_WORK {
            log::warn!("slow chunk work {total:?}: stream + populate {populated:?}, light done at {lit:?}, {built} mesh(es)");
        }
    }

    /// The columns of the chunks that became final (or were unloaded after an edit) since the last call: what the far terrain shows
    /// instead of a guess.
    pub fn take_ingest(&mut self) -> Vec<(Key, crate::world::lod::Cols)> {
        std::mem::take(&mut self.ingest)
    }

    /// Chunk `k` has a mesh to draw: the far terrain (`world::lod`) is cut out only under chunks that do, so a chunk that is in
    /// range but still generating or meshing leaves the far terrain in place instead of a hole.
    pub fn drawn(&self, k: Key) -> bool {
        self.chunks.get(&k).is_some_and(|e| e.mesh.is_some())
    }

    /// Chunk `k` has the four populates that write into it behind it (its own and the three before it in -x / -z): its blocks are final.
    fn settled(&self, k: Key) -> bool {
        [(0, 0), (-1, 0), (0, -1), (-1, -1)].iter().all(|d| self.chunks.get(&(k.0 + d.0, k.1 + d.1)).is_some_and(|e| e.populated))
    }

    /// Hand chunk `k` to the far terrain once, when it is settled. Called for the chunks a populate or a load may have settled.
    fn settle_around(&mut self, k: Key) {
        for d in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let c = (k.0 + d.0, k.1 + d.1);
            if self.ingested.contains(&c) || !self.settled(c) {
                continue;
            }
            let cols = cols_of(&mut self.cm, c, &self.chunks[&c]);
            self.ingest.push((c, cols));
            self.ingested.insert(c);
        }
    }

    fn stream(&mut self, cx: i32, cz: i32) {
        // Keep radius: raw ring + 1, so walking along the edge does not thrash.
        let keep = (self.radius + 3) * (self.radius + 3);
        let d2 = |(x, z): Key| (x - cx) * (x - cx) + (z - cz) * (z - cz);

        // 1. Collect finished chunks; ones the player already walked away from are dropped.
        while let Ok((key, blocks)) = self.done.try_recv() {
            self.pending.remove(&key);
            if d2(key) <= keep {
                self.insert(key, blocks);
            }
        }

        // 2. Unload, only when the player crossed a chunk border. Dropping an entry frees its buffers; one that is
        // not on disk yet is encoded and queued for the I/O thread first (`load` finds it there if the player walks back).
        // ponytail: the RLE encode (~48 KB in, ~5 KB out) still runs here; the upgrade is to send the arrays and encode on the thread.
        self.reap_writes(false);
        if self.center != Some((cx, cz)) {
            self.center = Some((cx, cz));
            let gone: Vec<Key> = self.chunks.keys().copied().filter(|&k| d2(k) > keep).collect();
            self.mesh_gen = self.mesh_gen.wrapping_add(1);
            for k in gone {
                let Some(e) = self.chunks.remove(&k) else { continue };
                self.graph.set(k, None);
                e.mesh.iter().for_each(|m| self.pages.free(m));
                if self.ingested.remove(&k) {
                    let cols = cols_of(&mut self.cm, k, &e);
                    self.ingest.push((k, cols));
                }
                self.light_remove(k);
                if let (true, Some(dir), Some(w)) = (e.dirty, &self.dir, &mut self.writer) {
                    w.queue(dir, k, save::encode_chunk(&e.blocks, &e.data, e.populated));
                    self.saved.insert(k);
                }
            }
        }

        // 3. Missing chunks nearest-first: a saved one is read from disk (a few per frame), any other is requested
        // from a worker. Only a few jobs are in flight, and the list is rebuilt from the current position every
        // frame, so a worker never queues stale requests.
        let mut loads = LOADS_PER_FRAME;
        for i in 0..self.ring.len() {
            let key = (cx + self.ring[i].0, cz + self.ring[i].1);
            if self.chunks.contains_key(&key) {
                continue;
            }
            if self.saved.contains(&key) {
                if loads > 0 {
                    loads -= 1;
                    self.load(key);
                }
            } else if self.pending.len() < self.max_in_flight && self.pending.insert(key) {
                let _ = self.jobs.send(Job::Gen(key));
            }
        }
    }

    /// Send populate to the workers for chunks nearest to (cx, cz) first whose 2x2 block is loaded and free (not inside a job that is
    /// out), while fewer than `max_populating` jobs are out. The job gets a copy of the four chunks; they stay readable meanwhile.
    fn populate_dispatch(&mut self, cx: i32, cz: i32) {
        for i in 0..self.ring.len() {
            if self.populating.len() >= self.max_populating {
                break;
            }
            let (x, z) = (cx + self.ring[i].0, cz + self.ring[i].1);
            let keys = [(x, z), (x, z + 1), (x + 1, z), (x + 1, z + 1)];
            if self.chunks.get(&keys[0]).map_or(true, |e| e.populated) || keys.iter().any(|k| self.busy.contains(k) || !self.chunks.contains_key(k)) {
                continue;
            }
            let blocks = keys.map(|k| self.chunks[&k].blocks.to_vec());
            let data = keys.map(|k| self.chunks[&k].data.clone());
            self.populating.insert((x, z), keys.map(|k| self.chunks[&k].rev));
            keys.iter().for_each(|&k| {
                self.busy.insert(k);
            });
            let _ = self.jobs.send(Job::Populate(Box::new(PopJob { x, z, region: Region::new(x, z, blocks, data) })));
        }
    }

    /// Put back the populate results that are in. One whose chunks were edited, grown into, or unloaded since it was copied is
    /// dropped: it was computed from an older chunk, and the chunk, still unpopulated, is sent out again by `populate_dispatch`.
    fn populate_land(&mut self) {
        while let Ok((key, region)) = self.populated_rx.try_recv() {
            let Some(revs) = self.populating.remove(&key) else { continue };
            let keys = [(key.0, key.1), (key.0, key.1 + 1), (key.0 + 1, key.1), (key.0 + 1, key.1 + 1)];
            keys.iter().for_each(|k| {
                self.busy.remove(k);
            });
            if keys.iter().zip(revs).all(|(k, r)| self.chunks.get(k).is_some_and(|e| e.rev == r)) {
                self.put_region(key.0, key.1, region);
                self.chunks.get_mut(&key).unwrap().populated = true;
                self.queue_edge_water(key.0, key.1);
                self.settle_around(key);
            }
        }
    }

    /// populate chunk (x, z) here, now, if it is not yet and its 2x2 block is loaded and not out on a worker. True if it ran.
    fn populate_one(&mut self, x: i32, z: i32) -> bool {
        let keys = [(x, z), (x, z + 1), (x + 1, z), (x + 1, z + 1)];
        if self.chunks.get(&keys[0]).map_or(true, |e| e.populated) || !keys.iter().all(|k| self.chunks.contains_key(k)) || keys.iter().any(|k| self.busy.contains(k)) {
            return false;
        }
        let mut region = self.take_region(x, z).unwrap();
        self.gen.populate(&mut region, x, z, &mut self.cm);
        self.put_region(x, z, region);
        self.chunks.get_mut(&keys[0]).unwrap().populated = true;
        self.queue_edge_water(x, z);
        self.settle_around((x, z));
        true
    }

    /// A finished mesh for `key` arrived: false when the chunk was unloaded meanwhile. One edited since its job was queued is kept all
    /// the same: its `meshed` is clear, so a new job follows, and until that lands the older picture still beats the one before it
    /// (a chunk that keeps changing, flowing water, would otherwise never be redrawn while its jobs outlast the gaps between edits).
    fn accept(&mut self, key: Key) -> bool {
        self.meshing.remove(&key);
        self.chunks.contains_key(&key)
    }

    /// Uploads finished meshes while the frame's `UploadBudget` (bytes and time, Sodium's `UploadResourceBudget`) lasts, at most
    /// `MESH_PER_FRAME`: the first always, a later one only if its size and its estimated cost (`UploadDurationEstimator`) still
    /// fit; one that does not is kept for the next frame (`held`). Then queues jobs for the chunks that need one, nearest first,
    /// while fewer than `max_meshing` are out; returns how many it uploaded.
    ///
    /// `settled` = the light engine has nothing left. A new chunk or a light-stale one waits for it (meshed once, lit), but an edited
    /// chunk (`urgent`) does not: a broken block was visible for as long as the engine was busy (first light of the chunks streaming
    /// in keeps it busy for seconds), then a frame for the report, then the job. Its mesh takes the light as it is, and the engine's
    /// `stale` report queues the second, lit one. ponytail: two meshes per edit while the engine is busy, one when it is not.
    fn mesh_pending(&mut self, gfx: Gfx, origins: Option<&DrawOrigins>, cx: i32, cz: i32, settled: bool) -> usize {
        self.urgent.retain(|k| self.chunks.contains_key(k));
        let mut budget = UploadBudget::limited(upload_budget::frame_duration_budget(self.frame_ns.unwrap_or(FIRST_FRAME_NS)), UPLOAD_SIZE_LIMIT);
        let mut uploaded = 0;
        // What this frame really wrote (meshes that went into an existing page only: a new page's buffer creation is not an upload).
        let (mut writes, mut written, mut spent_ns) = (0u64, 0u64, 0u64);
        while uploaded < MESH_PER_FRAME {
            let Some((key, b)) = self.held.take().or_else(|| self.built.try_recv().ok()) else { break };
            if !self.chunks.contains_key(&key) {
                self.meshing.remove(&key); // unloaded meanwhile (what `accept` says), and it costs no budget
                continue;
            }
            let bytes = b.verts.len() as u64 * VERT_BYTES;
            if uploaded > 0 && bytes > 0 && !budget.fits(self.upload.estimate(bytes), bytes) {
                self.held = Some((key, b));
                break;
            }
            self.accept(key);
            uploaded += 1;
            let Built { verts, starts, cut, wstarts, vis } = b;
            let pages_before = self.pages.list.len();
            let t = std::time::Instant::now();
            let put = if verts.is_empty() { None } else { Some(self.pages.put(gfx, origins, &verts, key)) };
            let spent = t.elapsed().as_nanos() as u64;
            budget.consume(spent, bytes);
            if matches!(put, Some(Some(_))) && self.pages.list.len() == pages_before {
                (writes, written, spent_ns) = (writes + 1, written + bytes, spent_ns + spent);
            }
            // One run of the page holds the three layers; the offsets are index ranges of the shared index buffer.
            let mesh = match put {
                None => None,
                Some(Some((page, v, slot))) => {
                    let at = |q: u32| quad_index::first_index(v) + q * quad_index::ELEMENTS_PER_QUAD;
                    Some(Mesh {
                        page,
                        v,
                        slot,
                        origin: [key.0 as f32 * 16.0, 0.0, key.1 as f32 * 16.0],
                        index_count: cut[SECTIONS] * quad_index::ELEMENTS_PER_QUAD,
                        starts: starts.map(|q| at(q)),
                        cut: cut.map(|q| at(q)),
                        water: (wstarts[SECTIONS] > wstarts[0]).then(|| wstarts.map(|q| at(q))),
                        vis,
                    })
                }
                Some(None) => {
                    // ponytail: a mesh bigger than a page (> 1M vertices) is not drawn; `meshed` stays set, so it is not rebuilt in a loop.
                    log::error!("chunk {key:?}: mesh of {} vertices does not fit a page", verts.len());
                    None
                }
            };
            if let Some(e) = self.chunks.get_mut(&key) {
                // ponytail: a rebuild with the same index count (the time-of-day relight) is taken as the same geometry, so the
                // shadow map is not redrawn for it; an edit that keeps the count is picked up by the next sun / movement refresh.
                if e.mesh.as_ref().map_or(0, |m| m.index_count) != mesh.as_ref().map_or(0, |m| m.index_count) {
                    self.mesh_gen = self.mesh_gen.wrapping_add(1);
                }
                // Only other occlusion data makes the walk redo itself: the time-of-day relight rebuilds a mesh with the same `vis`.
                let (was, now) = (e.mesh.as_ref().map_or([vis::FULL; SECTIONS], |m| m.vis), mesh.as_ref().map_or([vis::FULL; SECTIONS], |m| m.vis));
                if was != now {
                    self.graph.set(key, Some(now));
                }
                // The new runs are in before the old ones go, so a page holding only this chunk is not dropped and made again.
                if let Some(old) = std::mem::replace(&mut e.mesh, mesh) {
                    self.pages.free(&old);
                }
            } else if let Some(m) = &mesh {
                self.pages.free(m); // (cannot happen: `accept` checked) keep the arena honest
            }
        }

        // Sodium feeds the estimator one sample per frame: the average size and the average duration of that frame's uploads
        // (the time of one mesh is not separable from the others', so the mean is what it learns from).
        if writes > 0 {
            self.upload.add_data(written / writes, spent_ns / writes);
            self.upload.update_models();
        }

        let r2 = self.radius * self.radius;
        for &(dx, dz) in &self.ring {
            if self.meshing.len() >= self.max_meshing || dx * dx + dz * dz > r2 {
                break; // the ring is sorted by distance, so everything after is outside too
            }
            let (x, z) = (cx + dx, cz + dz);
            if self.meshing.contains(&(x, z)) {
                continue;
            }
            let c = &self.chunks;
            let job = match (c.get(&(x, z)), c.get(&(x + 1, z)), c.get(&(x - 1, z)), c.get(&(x, z + 1)), c.get(&(x, z - 1))) {
                (Some(me), Some(px), Some(nx), Some(pz), Some(nz)) if !me.meshed && me.lit && (settled || self.urgent.contains(&(x, z))) && self.is_final(x, z) => {
                    Some(MeshJob { key: (x, z), sky_sub: self.sky_sub, cells: [snap(me), snap(px), snap(nx), snap(pz), snap(nz)] })
                }
                _ => None,
            };
            let Some(job) = job else { continue };
            if let Some(e) = self.chunks.get_mut(&(x, z)) {
                e.meshed = true; // "on its way": an edit before the result lands clears it again, and a new job follows
            }
            self.meshing.insert((x, z));
            self.urgent.remove(&(x, z));
            let _ = self.jobs.send(Job::Mesh(Box::new(job)));
        }
        uploaded
    }
}

fn chunk_path(dir: &Path, (x, z): Key) -> PathBuf {
    dir.join(format!("c.{x}.{z}"))
}

/// The save I/O thread (C2ME `rewrites-chunkio`): chunk files are written there, one after the other in the order they were
/// queued, each through `save::write` (atomic). The manager keeps the newest queued bytes of every chunk until its write has been
/// answered, because the file on disk can still be the older one.
struct Writer {
    tx: mpsc::Sender<(Key, u64, PathBuf, Arc<Vec<u8>>)>,
    answers: mpsc::Receiver<(Key, u64, std::io::Result<()>)>,
    /// Newest queued bytes per chunk and the sequence number of that write, until it lands.
    inflight: HashMap<Key, (u64, Arc<Vec<u8>>)>,
    seq: u64,
    /// Writes sent and not answered yet: what `reap(true)` waits for (not `inflight`, which keeps an entry only for the newest write).
    pending: usize,
}

impl Writer {
    fn spawn() -> Self {
        let (tx, jobs) = mpsc::channel::<(Key, u64, PathBuf, Arc<Vec<u8>>)>();
        let (done, answers) = mpsc::channel();
        std::thread::Builder::new()
            .name("chunk-io".into())
            .spawn(move || {
                crate::world::nice(10);
                // Ends when the manager is dropped (the channel closes after the last queued write).
                while let Ok((key, seq, path, bytes)) = jobs.recv() {
                    if done.send((key, seq, save::write(&path, &bytes))).is_err() {
                        break;
                    }
                }
            })
            .expect("spawn chunk io thread");
        Self { tx, answers, inflight: HashMap::new(), seq: 0, pending: 0 }
    }

    fn queue(&mut self, dir: &Path, key: Key, bytes: Vec<u8>) {
        self.seq += 1;
        let bytes = Arc::new(bytes);
        self.inflight.insert(key, (self.seq, bytes.clone()));
        if self.tx.send((key, self.seq, chunk_path(dir, key), bytes)).is_ok() {
            self.pending += 1;
        }
    }

    /// Take the answers that are in (`wait`: until every sent write has been answered, or the thread is gone); the chunks whose
    /// write failed.
    fn reap(&mut self, wait: bool) -> Vec<(Key, std::io::Error)> {
        let mut failed = Vec::new();
        loop {
            let msg = if wait && self.pending > 0 { self.answers.recv().ok() } else { self.answers.try_recv().ok() };
            let Some((key, seq, res)) = msg else { break };
            self.pending = self.pending.saturating_sub(1);
            // An answer for an older write of the chunk leaves the newer one's bytes in place.
            if self.inflight.get(&key).is_some_and(|(s, _)| *s == seq) {
                self.inflight.remove(&key);
            }
            if let Err(e) = res {
                failed.push((key, e));
            }
        }
        failed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::chunk::VOLUME;
    use std::time::{Duration, Instant};

    /// Real water is only drawn near the eye: a high eye over the sea, or a chunk 40 blocks off, keeps none of it.
    #[test]
    fn water_is_drawn_only_within_its_range() {
        let eye = Vec3::new(8.0, 70.0, 8.0);
        let near = |x| near_sections(x, 0, 0xFF, eye, Shape::Sphere, WATER_RANGE, 48.0);
        // Section boxes are one block taller at both ends: y 3 (47..65) is 5 below the eye, y 4 holds it, y 5 (79..97) is 9 above;
        // y 2 (31..49) is 21 below and y 6 (95..113) 25 above: out.
        assert_eq!(near(0), 0b0011_1000);
        assert_eq!(near(1), 0b0001_1000, "a chunk 8 blocks off loses the section 9 above");
        assert_eq!(near(3), 0, "a chunk 40 blocks off is out");
        assert_eq!(near_sections(0, 0, 0b0000_1000, Vec3::new(8.0, 200.0, 8.0), Shape::Sphere, WATER_RANGE, 48.0), 0, "an eye far above keeps none");
        // LoD off: `update_visible` passes `1e9.min(rh)`, so the chunk 40 blocks off keeps its water too.
        assert_ne!(near_sections(3, 0, 0xFF, eye, Shape::Sphere, 1e9f32.min(64.0), 48.0), 0);
    }

    /// `box_visible`: a box is seen when one of the sections it touches was found by `update_visible`; a box in a loaded chunk the walk
    /// did not reach is hidden, one in an unloaded chunk or above the world is not, and before the first walk nothing is hidden.
    #[test]
    fn entities_hide_in_sections_the_walk_did_not_reach() {
        let mut m = ChunkManager::new(1, 1);
        m.insert((0, 0), vec![0u8; VOLUME]);
        m.insert((1, 0), vec![0u8; VOLUME]);
        let v = |a: [f32; 3], b: [f32; 3]| m.box_visible(Vec3::from(a), Vec3::from(b));
        assert!(v([23.0, 66.0, 7.0], [25.0, 68.0, 9.0]), "no walk yet: nothing is hidden");
        m.visible.insert((0, 0), 0b0001_0000); // section 4 (y 64..79) of chunk (0, 0) only
        let v = |a: [f32; 3], b: [f32; 3]| m.box_visible(Vec3::from(a), Vec3::from(b));
        assert!(v([7.0, 66.0, 7.0], [9.0, 68.0, 9.0]), "inside a found section");
        assert!(!v([7.0, 82.0, 7.0], [9.0, 84.0, 9.0]), "section 5 was not found");
        assert!(!v([23.0, 66.0, 7.0], [25.0, 68.0, 9.0]), "chunk (1, 0) is loaded and not found");
        assert!(!v([14.0, 82.0, 7.0], [18.0, 84.0, 9.0]), "straddles two chunks, neither found at section 5");
        assert!(v([14.0, 66.0, 7.0], [18.0, 84.0, 9.0]), "tall box: touches section 4 of chunk (0, 0)");
        assert!(v([-40.0, 66.0, 7.0], [-38.0, 68.0, 9.0]), "chunk (-3, 0) is not loaded");
        assert!(v([7.0, 130.0, 7.0], [9.0, 134.0, 9.0]), "above the world");
        assert!(v([7.0, -3.0, 7.0], [9.0, -1.0, 9.0]), "below the world");
    }

    /// Edge water is a still source on a surface with air beside it: not the middle of a pool, not water with water above it.
    #[test]
    fn edge_water_is_a_surface_source_with_air_beside_it() {
        let mut m = ChunkManager::new(1, 1);
        let mut b = vec![0u8; VOLUME];
        for x in 4..9 {
            for z in 4..9 {
                b[idx(x, 40, z)] = 9;
            }
        }
        b[idx(6, 41, 6)] = 9; // a cell of water above the middle
        b[idx(12, 40, 12)] = 8; // flowing: not still
        m.insert((0, 0), b);
        let e = m.edge_water((0, 0));
        assert!(e.contains(&(4, 40, 4)) && e.contains(&(6, 40, 4)), "the rim of the pool");
        assert!(e.contains(&(6, 41, 6)), "the lone cell on top");
        assert!(!e.contains(&(6, 40, 6)) && !e.contains(&(5, 40, 5)), "water above, or water all round: not an edge");
        assert!(!e.contains(&(12, 40, 12)));
    }

    /// World coordinates (negative too) map to the right chunk cell; unloaded reads as solid.
    #[test]
    fn block_lookup_across_chunks() {
        let mut m = ChunkManager::new(1, 1);
        let mut b = vec![0u8; VOLUME];
        b[idx(15, 5, 0)] = 7; // local (15, 5, 0) of chunk (-1, 0) = world (-1, 5, 0)
        m.insert((-1, 0), b);
        assert_eq!(m.block(-1, 5, 0), Some(7));
        assert_eq!(m.block(-2, 5, 0), Some(0));
        assert_eq!(m.block(0, 5, 0), Some(1)); // chunk (0, 0) is not loaded
        assert_eq!(m.block(-1, 200, 0), None);
    }

    /// Metadata follows the Java setters: `set_block_meta` writes id and metadata, `set_block` does nothing on the
    /// same id and clears the metadata when the id changes, and an unloaded chunk reads 0.
    #[test]
    fn metadata_follows_chunk_setters() {
        let mut m = ChunkManager::new(1, 1);
        m.insert((0, 0), vec![0u8; VOLUME]);
        assert!(m.set_block_meta(3, 40, 5, 35, 14)); // red wool
        assert_eq!((m.block(3, 40, 5), m.meta(3, 40, 5)), (Some(35), 14));
        assert!(!m.set_block_meta(3, 40, 5, 35, 14)); // nothing changed
        assert!(m.set_block_meta(3, 40, 5, 35, 3 | 16)); // same id, new metadata (only 4 bits)
        assert_eq!(m.meta(3, 40, 5), 3);
        assert!(!m.set_block(3, 40, 5, 35)); // same id: untouched
        assert_eq!(m.meta(3, 40, 5), 3);
        assert!(m.set_block(3, 40, 5, 0)); // break: the metadata is cleared
        assert_eq!((m.block(3, 40, 5), m.meta(3, 40, 5)), (Some(0), 0));
        assert_eq!(m.meta(100, 40, 5), 0); // unloaded
        assert!(!m.set_block_meta(3, 128, 5, 35, 1)); // above the world
    }

    /// The chunk arrays are shared (`Arc`) with mesh jobs and the light thread: an edit copies on write, so a snapshot taken before it
    /// still shows the old block, and the chunk shows the new one.
    #[test]
    fn an_edit_leaves_a_shared_snapshot_alone() {
        let mut m = ChunkManager::new(1, 1);
        m.insert((0, 0), vec![0u8; VOLUME]);
        let before = snap(&m.chunks[&(0, 0)]);
        assert!(Arc::ptr_eq(&before.blocks, &m.chunks[&(0, 0)].blocks), "a snapshot shares the array, it does not copy it");
        assert!(m.set_block(3, 40, 5, 1));
        assert_eq!(before.blocks[idx(3, 40, 5)], 0);
        assert_eq!(m.block(3, 40, 5), Some(1));
    }

    /// preload populates every chunk whose 2x2 is loaded, which finishes the inner ones.
    #[test]
    fn preload_populates_and_finishes_inner_chunks() {
        let mut m = ChunkManager::new(1, 1);
        m.preload(0, 0);
        for z in -1..=1 {
            for x in -1..=1 {
                assert!(m.chunks[&(x, z)].populated, "({x}, {z})");
            }
        }
        assert!(!m.chunks[&(2, 2)].populated);
        assert!(m.is_final(0, 0) && m.is_final(1, 1) && !m.is_final(2, 2));
    }

    /// populate run on a worker (`populate_dispatch` + `populate_land`) gives the blocks and metadata that `populate_one` gives in
    /// place, and the chests and spawners with them.
    #[test]
    fn populate_on_a_worker_gives_the_in_place_result() {
        let (mut a, mut b) = (ChunkManager::new(1, 1), ChunkManager::new(1, 1));
        for z in 0..=1 {
            for x in 0..=1 {
                a.fresh((x, z));
                b.fresh((x, z));
            }
        }
        assert!(a.populate_one(0, 0));
        b.populate_dispatch(0, 0);
        assert!(b.populating.contains_key(&(0, 0)) && b.busy.len() == 4, "only chunk (0, 0) has its 2x2 block loaded");
        for _ in 0..4000 {
            b.populate_land();
            if b.chunks[&(0, 0)].populated {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(b.chunks[&(0, 0)].populated && b.populating.is_empty() && b.busy.is_empty());
        for k in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            assert!(a.chunks[&k].blocks == b.chunks[&k].blocks && a.chunks[&k].data == b.chunks[&k].data, "chunk {k:?}");
        }
        assert_eq!(a.loot.len(), b.loot.len());
        assert_eq!(a.spawners.len(), b.spawners.len());
    }

    /// A populate result is dropped when a chunk of its block was edited after the copy was made; the chunk, still unpopulated, is
    /// sent out again, and the job is out of `busy` meanwhile.
    #[test]
    fn a_stale_populate_result_is_dropped_and_sent_again() {
        let mut m = ChunkManager::new(1, 1);
        for z in 0..=1 {
            for x in 0..=1 {
                m.fresh((x, z));
            }
        }
        m.populate_dispatch(0, 0);
        assert!(m.populating.contains_key(&(0, 0)));
        assert!(m.set_block_meta(3, 40, 5, 35, 0), "an edit after the copy"); // red wool where the terrain has none
        for _ in 0..4000 {
            m.populate_land();
            if m.populating.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(m.populating.is_empty() && m.busy.is_empty());
        assert!(!m.chunks[&(0, 0)].populated, "the stale result was dropped");
        m.populate_dispatch(0, 0);
        assert!(m.populating.contains_key(&(0, 0)), "and the chunk was sent out again");
        for _ in 0..4000 {
            m.populate_land();
            if m.chunks[&(0, 0)].populated {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(m.chunks[&(0, 0)].populated);
    }

    /// An edit survives a restart: flushed, then read back by a fresh manager that loads (does not generate) the chunk.
    /// A chunk nobody touched is not written, and a corrupt file is forgotten instead of loaded.
    #[test]
    fn edits_survive_a_restart() {
        let dir = std::env::temp_dir().join(format!("mc-rs-save-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut a = ChunkManager::new(1, 1).with_dir(dir.clone());
        a.insert((0, 0), vec![0u8; VOLUME]);
        a.insert((1, 0), vec![0u8; VOLUME]);
        assert!(a.set_block_meta(3, 40, 5, 35, 14)); // red wool in chunk (0, 0)
        a.chunks.get_mut(&(0, 0)).unwrap().populated = true;
        a.flush(usize::MAX);
        assert!(!a.chunks[&(0, 0)].dirty && a.saved.contains(&(0, 0)) && !a.saved.contains(&(1, 0)));

        let mut b = ChunkManager::new(1, 1).with_dir(dir.clone());
        assert!(b.saved.contains(&(0, 0)) && b.saved.len() == 1);
        b.load((0, 0));
        assert_eq!((b.block(3, 40, 5), b.meta(3, 40, 5)), (Some(35), 14));
        assert!(b.chunks[&(0, 0)].populated && !b.chunks[&(0, 0)].dirty);

        std::fs::write(dir.join("c.0.0"), [1u8, 2, 3]).unwrap();
        let mut c = ChunkManager::new(1, 1).with_dir(dir.clone());
        c.load((0, 0));
        assert!(!c.saved.contains(&(0, 0)) && !c.chunks.contains_key(&(0, 0)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `load` reads the newest write still queued, not the older file on disk (a chunk unloaded and walked back to before its write
    /// landed must keep its edits), and `reap` retires the entry once its own write is answered but not for an older one.
    #[test]
    fn load_prefers_the_write_still_queued() {
        let dir = std::env::temp_dir().join(format!("mc-rs-queued-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut m = ChunkManager::new(1, 1).with_dir(dir.clone());
        std::fs::write(dir.join("c.0.0"), save::encode_chunk(&vec![0u8; VOLUME], &Nibbles::new(), true)).unwrap(); // the older file: air
        m.saved.insert((0, 0));
        let mut blocks = vec![0u8; VOLUME];
        blocks[idx(3, 40, 5)] = 35;
        m.writer.as_mut().unwrap().inflight.insert((0, 0), (7, Arc::new(save::encode_chunk(&blocks, &Nibbles::new(), true))));
        m.load((0, 0));
        assert_eq!(m.block(3, 40, 5), Some(35), "the queued bytes, not the file");
        m.chunks.remove(&(0, 0));
        m.flush(usize::MAX); // waits for nothing (no write was sent), so it must not hang on the entry above, which is not the writer's to retire
        assert!(m.writer.as_ref().unwrap().inflight.contains_key(&(0, 0)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The ring around the player fills in from the workers; after a long walk the old area is gone.
    #[test]
    fn ring_loads_then_unloads_when_walking() {
        let mut m = ChunkManager::new(1, 1);
        let settle = |m: &mut ChunkManager, c: Key| {
            let t = Instant::now();
            while !m.ring.iter().all(|&(dx, dz)| m.chunks.contains_key(&(c.0 + dx, c.1 + dz))) {
                assert!(t.elapsed() < Duration::from_secs(180), "ring did not load");
                m.stream(c.0, c.1);
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        settle(&mut m, (0, 0));
        settle(&mut m, (10, 0));
        assert!(!m.chunks.contains_key(&(0, 0)));
    }

    /// Sphere is stricter than cylinder, both stop at the vertical radius; sections come out in order with the right offsets.
    #[test]
    fn render_shape_and_sections() {
        let (eye, lo, hi) = (Vec3::new(8.0, 64.0, 8.0), Vec3::new(64.0, 48.0, 0.0), Vec3::new(80.0, 66.0, 16.0));
        assert!(in_range(Shape::Cylinder, eye, lo, hi, 64.0, 48.0) && in_range(Shape::Sphere, eye, lo, hi, 64.0, 48.0));
        let (lo, hi) = (Vec3::new(64.0, 0.0, 0.0), Vec3::new(80.0, 16.0, 16.0)); // 48 below, 56 across: 74 away in 3D
        assert!(in_range(Shape::Cylinder, eye, lo, hi, 64.0, 48.0) && !in_range(Shape::Sphere, eye, lo, hi, 64.0, 48.0));
        assert!(!in_range(Shape::Cylinder, eye, lo, hi, 64.0, 40.0), "vertical radius");
        // Two quads at y = 5 and y = 40 (given out of order) are sorted into sections 0 and 2, as quad numbers.
        let vf = crate::render::mesh::VF;
        let mut v = vec![0.0f32; 8 * vf];
        (0..8).for_each(|i| {
            v[i * vf + 1] = if i < 4 { 5.0 } else { 40.0 };
            (v[i * vf + 3], v[i * vf + 4], v[i * vf + 6], v[i * vf + 7]) = (0.07, 0.01, 255.0, mesh::WHITE); // tile 1, stone: not cut-out
        });
        let q = pack(&v, &[4, 5, 6, 4, 6, 7, 0, 1, 2, 0, 2, 3], &[]);
        assert_eq!((q.verts.len(), q.starts[1], q.starts[2], q.starts[3], q.starts[SECTIONS]), (8, 1, 1, 2, 2));
        assert_eq!(vertex::decode(&q.verts[0]).pos[1], 5.0);
        assert_eq!(vertex::decode(&q.verts[4]).pos[1], 40.0);
        assert!(q.cut.iter().all(|&c| c == 2) && q.wstarts.iter().all(|&c| c == 2), "no cut-out, no water: empty layers after the solid ones");
    }

    /// The CPU half of meshing run from a snapshot (what a worker does) gives exactly what the mesher gives on the live arrays.
    #[test]
    fn worker_mesh_is_the_in_place_mesh() {
        let snap_of = |id: u8| {
            let mut blocks = vec![0u8; VOLUME];
            (0..16).for_each(|x| (0..16).for_each(|z| blocks[idx(x, 60, z)] = id));
            Snap { blocks: Arc::new(blocks), data: Nibbles::new(), light: Arc::new(vec![15; VOLUME]) }
        };
        let job = MeshJob { key: (2, -3), sky_sub: 0, cells: [snap_of(1), snap_of(1), snap_of(1), snap_of(1), snap_of(1)] };
        let want = {
            let [me, px, nx, pz, nz] = &job.cells;
            let tints = biome_tints(&mut WorldChunkManager::new(1), 2, -3);
            let (v, i, w) = mesh::build_split_tinted(
                crate::LEGACY, &me.blocks[..], &me.data, [&px.data, &nx.data, &pz.data, &nz.data], &me.light[..],
                [&px.blocks[..], &nx.blocks[..], &pz.blocks[..], &nz.blocks[..]],
                [&px.light[..], &nx.light[..], &pz.light[..], &nz.light[..]], 0, 0, 0, &tints,
            );
            pack(&v, &i, &w)
        };
        let b = build_job(job, &mut WorldChunkManager::new(1));
        assert!(!b.verts.is_empty(), "a stone floor has a top face");
        assert_eq!((b.verts, b.starts, b.cut, b.wstarts), (want.verts, want.starts, want.cut, want.wstarts));
    }

    /// A result lands whether or not its chunk changed while the job ran (an edit clears `meshed`, so the next job is queued), is
    /// dropped when the chunk was unloaded, and always frees the chunk for its next job.
    #[test]
    fn mesh_results_land_and_an_edit_queues_the_next_job() {
        let mut m = ChunkManager::new(1, 1);
        m.insert((0, 0), vec![0u8; VOLUME]);
        m.chunks.get_mut(&(0, 0)).unwrap().meshed = true; // a job is out
        m.meshing.insert((0, 0));
        assert!(m.accept((0, 0)) && m.meshing.is_empty() && m.chunks[&(0, 0)].meshed, "untouched: kept, still current");
        m.meshing.insert((0, 0));
        m.mark_dirty(3, 3); // an edit while the job ran
        assert!(m.urgent.contains(&(0, 0)), "an edit is meshed without waiting for the light engine");
        assert!(m.accept((0, 0)) && m.meshing.is_empty() && !m.chunks[&(0, 0)].meshed, "edited: kept, and due a new job");
        m.meshing.insert((0, 0));
        m.chunks.remove(&(0, 0));
        assert!(!m.accept((0, 0)) && m.meshing.is_empty(), "unloaded: dropped");
    }

    /// A new sky-light level changes nothing the legacy shaders read from a vertex, so no mesh goes stale (the Photon look restarts them).
    /// `top_solid` reads the column as a slice: same answer as the per-block walk it replaced, on every kind of column and cell edge.
    #[test]
    fn top_solid_scans_the_column_like_the_block_walk() {
        use crate::world::gen::populate::{is_liquid, is_solid};
        let mut m = ChunkManager::new(1, 1);
        let mut blocks = vec![0u8; VOLUME];
        for (x, z) in [(0usize, 0usize), (15, 15), (3, 9)] {
            let b = idx(x, 0, z);
            blocks[b] = 7; // bedrock at y = 0 is never the answer
            (1..=60).for_each(|y| blocks[b + y] = 1);
            blocks[b + 61] = 31; // tall grass: not solid
            blocks[b + 62] = 78; // snow layer: not solid
        }
        let b = idx(5, 0, 5);
        (1..=63).for_each(|y| blocks[b + y] = 9); // water to the surface
        blocks[b + 64] = 31;
        blocks[b + 127] = 1; // a block in the topmost cell
        m.insert((0, 0), blocks);
        let walk = |x: i32, z: i32| (1..H as i32).rev().find(|&y| m.block_loaded(x, y, z).is_some_and(|b| is_solid(b) || is_liquid(b))).map_or(-1, |y| y + 1);
        for (x, z) in [(0, 0), (15, 15), (3, 9), (5, 5), (8, 8), (-1, 0), (16, 0), (0, -17)] {
            assert_eq!(m.top_solid(x, z), walk(x, z), "column ({x}, {z})");
        }
        assert_eq!((m.top_solid(0, 0), m.top_solid(5, 5), m.top_solid(8, 8), m.top_solid(16, 0)), (61, 128, -1, -1));
    }

    /// The biome grid of the rain pass is memoised: the same area twice is one noise run, a new area is a new answer.
    #[test]
    fn biomes_are_remembered_per_area() {
        let mut m = ChunkManager::new(1, 1);
        let a = m.biomes(100, -40, 21, 21);
        assert_eq!(m.biome_area.as_ref().map(|c| c.0), Some((100, -40, 21, 21)));
        assert_eq!(m.biomes(100, -40, 21, 21), a);
        let fresh = ChunkManager::new(1, 1).biomes(5000, 5000, 21, 21);
        assert_eq!(m.biomes(5000, 5000, 21, 21), fresh, "another area is computed, not served from the old one");
        assert_eq!(m.biomes(100, -40, 21, 21), a);
    }

    #[test]
    fn sky_light_step_keeps_the_meshes() {
        let mut m = ChunkManager::new(1, 1);
        m.insert((0, 0), vec![0u8; VOLUME]);
        m.chunks.get_mut(&(0, 0)).unwrap().meshed = true;
        m.set_sky_sub(7);
        assert_eq!(m.chunks[&(0, 0)].meshed, crate::LEGACY);
        assert_eq!(m.sky_sub, 7);
    }

    /// The quads `pack` emits, drawn with the shared index pattern, are exactly the triangles the mesher's own index lists drew: a plain
    /// quad, one on the other diagonal (smooth) and one seen from both sides, in the same positions and the same winding.
    #[test]
    fn packed_quads_draw_the_mesher_triangles() {
        let vf = crate::render::mesh::VF;
        let mut v: Vec<f32> = Vec::new();
        let mut quad = |p: [[f32; 3]; 4], lm: f32| {
            for c in p {
                v.extend_from_slice(&[c[0], c[1], c[2], 0.07, 0.01, 0.6, lm, mesh::WHITE]);
            }
        };
        quad([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]], 255.0);
        quad([[2.0, 0.0, 0.0], [3.0, 0.0, 0.0], [3.0, 0.0, 1.0], [2.0, 0.0, 1.0]], mesh::SMOOTH_LM + 33.0);
        quad([[4.0, 0.0, 0.0], [5.0, 0.0, 0.0], [5.0, 1.0, 0.0], [4.0, 1.0, 0.0]], 255.0);
        let idxs = [0u32, 1, 2, 0, 2, 3, 5, 6, 7, 5, 7, 4, 8, 9, 10, 8, 10, 11, 8, 10, 9, 8, 11, 10];
        let pos = |i: usize| {
            let p = [v[i * vf], v[i * vf + 1], v[i * vf + 2]];
            p.map(|c| (c * 16.0) as i32)
        };
        // A triangle as its three positions, rotated so the smallest comes first (the winding is kept).
        let norm = |t: [[i32; 3]; 3]| {
            let m = (0..3).min_by_key(|&k| t[k]).unwrap();
            [t[m], t[(m + 1) % 3], t[(m + 2) % 3]]
        };
        let mut want: Vec<_> = idxs.chunks_exact(3).map(|t| norm([pos(t[0] as usize), pos(t[1] as usize), pos(t[2] as usize)])).collect();
        let q = pack(&v, &idxs, &[]);
        assert_eq!(q.verts.len(), 4 + 4 + 8, "the double sided quad is two quads");
        let shared = quad_index::indices(q.verts.len() as u32 / 4);
        let ppos = |i: u32| vertex::decode(&q.verts[i as usize]).pos.map(|c| (c * 16.0) as i32);
        let mut got: Vec<_> = shared.chunks_exact(3).map(|t| norm([ppos(t[0]), ppos(t[1]), ppos(t[2])])).collect();
        want.sort();
        got.sort();
        assert_eq!(got, want);
        // The smooth quad keeps its light payload and its own shade; a flat one takes the shade of the way it faces (this first quad is wound
        // to look down: 0.5 = code 3; the third looks along +Z: 0.8 = code 1) and its `sky * 16 + block` as quarter and half levels.
        let d = |i: usize| vertex::decode(&q.verts[i]);
        assert_eq!((d(0).sky, d(0).block, d(0).shade), (60, 30, 3));
        assert_eq!(d(8).shade, 1);
        assert!(q.verts.iter().any(|b| { let d = vertex::decode(b); (d.sky, d.block) == (1, 1) && (vertex::shade_value(d.shade) - 0.6).abs() < 1e-6 }), "smooth: 33 = 1 * 32 + 1, shade 0.6");
    }

    /// The baked face shade is what `flat_shade` derived: top 1.0, bottom 0.5, Z sides 0.8, X sides 0.6, the sign of Y from the winding
    /// (counter-clockwise from the front), 1.0 for a degenerate quad; the water layer keeps 1.0.
    #[test]
    fn flat_quads_carry_their_face_shade() {
        let vf = crate::render::mesh::VF;
        let quads: [([[f32; 3]; 4], u8); 6] = [ // (corners, `FACE_SHADE` index)
            ([[0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]], 0), // top (the LoD's winding for a face looking up)
            ([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]], 3), // bottom
            ([[1.0, 0.0, 1.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 1.0, 1.0]], 2), // +X (`mesh::FACES[0]`)
            ([[0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 1.0], [0.0, 1.0, 0.0]], 2), // -X
            ([[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]], 1), // +Z
            ([[0.0; 3]; 4], 0),                                                          // degenerate
        ];
        let mut v = vec![0.0f32; 4 * quads.len() * vf];
        let mut idxs = Vec::new();
        for (n, (corners, _)) in quads.iter().enumerate() {
            for (k, c) in corners.iter().enumerate() {
                let o = (n * 4 + k) * vf;
                v[o..o + 3].copy_from_slice(c);
                (v[o + 3], v[o + 4], v[o + 6], v[o + 7]) = (0.07, 0.01, 255.0, mesh::WHITE); // tile 1, stone: solid layer
            }
            let b = n as u32 * 4;
            idxs.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
        }
        let q = pack(&v, &idxs, &[]);
        assert_eq!(q.verts.len(), 4 * quads.len());
        // Quads come out sorted by section, then in order; they all sit in section 0 here.
        for (n, (_, want)) in quads.iter().enumerate() {
            assert!((0..4).all(|k| vertex::decode(&q.verts[n * 4 + k]).shade == *want), "quad {n}: want {want}");
        }
        // The same top quad as water: 255 whatever it faces.
        let w = pack(&v, &[], &idxs[6..12]);
        assert!(w.verts.iter().all(|b| vertex::decode(b).shade == 0));
    }

    /// A mesher quad for the top face of cell (x, z) at height `y` (`mesh::FACES[2]`), showing tile 1 whole, with the given light.
    fn top_face(v: &mut Vec<f32>, x: f32, y: f32, z: f32, lm: f32, lit: f32) -> u32 {
        let base = v.len() as u32 / mesh::VF as u32;
        let (w, h) = (crate::render::atlas::ATLAS_W as f32, crate::render::atlas::ATLAS_H as f32);
        for (c, (lx, ly)) in [[0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]].iter().zip([(0.02, 0.02), (15.98, 0.02), (15.98, 15.98), (0.02, 15.98)]) {
            v.extend_from_slice(&[x + c[0], y - 1.0 + c[1], z + c[2], (16.0 + lx) / w, ly / h, lit, lm, mesh::WHITE]);
        }
        base
    }

    /// A flat floor of 4 x 3 faces with the same light is one quad that tiles its texture 4 x 3 times; the corners keep the mesher's winding.
    #[test]
    fn a_flat_floor_is_one_quad() {
        let mut v = Vec::new();
        let mut idxs = Vec::new();
        for z in 0..3 {
            for x in 0..4 {
                let b = top_face(&mut v, x as f32, 5.0, z as f32, 255.0, 1.0);
                idxs.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
            }
        }
        let q = pack(&v, &idxs, &[]);
        assert_eq!(q.verts.len(), 4, "12 faces, one quad");
        let d: Vec<_> = q.verts.iter().map(vertex::decode).collect();
        assert!(d.iter().all(|v| v.repeat && v.tile == 1 && v.pos[1] == 5.0 && (v.sky, v.block, v.shade) == (60, 30, 0)));
        let xz: Vec<[f32; 2]> = d.iter().map(|v| [v.pos[0], v.pos[2]]).collect();
        assert_eq!(xz, [[0.0, 3.0], [4.0, 3.0], [4.0, 0.0], [0.0, 0.0]], "the template's corner order, stretched");
        let uv: Vec<[f32; 2]> = d.iter().map(|v| v.uv).collect();
        assert!((uv[0][0] - 0.0).abs() < 0.13 && (uv[1][0] - 63.98).abs() < 0.13 && (uv[0][1] - 0.0).abs() < 0.13 && (uv[3][1] - 47.98).abs() < 0.13, "{uv:?}");
    }

    /// Faces that differ in light, or that are not a whole block on the grid, are not joined: the row stays 3 quads (2 + 1 with the dark one
    /// between) and a slab-high face stays itself.
    #[test]
    fn faces_that_differ_are_not_merged() {
        let mut v = Vec::new();
        let mut idxs = Vec::new();
        for (x, lm) in [(0.0, 255.0), (1.0, 17.0), (2.0, 255.0), (3.0, 255.0)] {
            let b = top_face(&mut v, x, 5.0, 0.0, lm, 1.0);
            idxs.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
        }
        let b = top_face(&mut v, 0.0, 6.5, 2.0, 255.0, 1.0); // half a block up: not on the grid
        idxs.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
        let q = pack(&v, &idxs, &[]);
        assert_eq!(q.verts.len(), 4 * 4, "(0), (1), (2, 3) and the slab-high one");
        assert_eq!(q.verts.iter().filter(|b| vertex::decode(b).repeat).count(), 4, "only the pair of faces repeats");
    }

    /// A merged run never grows past 16 blocks (the chunk's width, the texture coordinate's range).
    #[test]
    fn a_run_stops_at_sixteen() {
        let mut cells: Vec<(i32, i32)> = (0..16).flat_map(|v| (0..16).map(move |u| (u, v))).collect();
        assert_eq!(greedy(&mut cells), vec![(0, 0, 16, 16)]);
        let mut row: Vec<(i32, i32)> = (0..20).map(|u| (u, 0)).collect();
        assert_eq!(greedy(&mut row), vec![(0, 0, 16, 1), (16, 0, 4, 1)]);
        let mut l: Vec<(i32, i32)> = vec![(0, 0), (1, 0), (0, 1)];
        assert_eq!(greedy(&mut l), vec![(0, 0, 2, 1), (0, 1, 1, 1)], "an L is two rectangles that cover it once");
    }

    /// Meshes share a page: a freed run is handed out again (best fit), the page is not duplicated, and a mesh bigger than a page is refused.
    /// Needs an adapter (lavapipe is enough); skipped without one. Reads nothing back: the offsets are the check.
    #[test]
    fn pages_reuse_a_freed_run() {
        let instance = wgpu::Instance::default();
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
            eprintln!("no adapter: skipped");
            return;
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        let origins = crate::gpu::pipeline::ChunkPipeline::new(&device).origins;
        let mut pages = Pages::default();
        let quad = vec![[0u8; vertex::STRIDE]; 4];
        let put = |p: &mut Pages| p.put(Gfx::Wgpu(&device, &queue), Some(&origins), &quad, (1, 2)).unwrap();
        let (pa, va, sa) = put(&mut pages);
        let (pb, vb, sb) = put(&mut pages);
        assert_eq!((pa, pb, pages.list.len()), (0, 0, 1));
        assert!(va != vb && sa != sb && va % 4 == 0 && vb % 4 == 0, "whole quads, one slot each");
        assert!(pages.quads.is_some(), "the shared index buffer exists once a mesh is in");
        let at = |page, v, slot| Mesh { page, v, slot, origin: [0.0; 3], index_count: 6, starts: [0; SECTIONS + 1], cut: [0; SECTIONS + 1], water: None, vis: [0; SECTIONS] };
        pages.free(&at(pa, va, sa));
        assert_eq!(put(&mut pages), (pa, va, sa), "the run and the slot come back");
        assert!(pages.put(Gfx::Wgpu(&device, &queue), Some(&origins), &vec![[0u8; vertex::STRIDE]; PAGE_VERTS as usize + 4], (0, 0)).is_none());
    }
}
