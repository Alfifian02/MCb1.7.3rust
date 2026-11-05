//! Structory (Stardust Labs, 1.20) in the Blooming world type: 15 jigsaw structures (ruins, chapels, camps, manors, boats, a taiga
//! dungeon, ...), assembled from 164 templates, placed 5 sets at a time by `random_spread`.
//!
//! The datapack is baked by `tools/gen_structory.py` into `assets/structory.bin` (modern block states already turned into the Beta
//! (id, metadata) they stand for, processor lists resolved per state, loot tables on Beta items); `tools/structory_proto.py` is the
//! Python twin of `assemble` and the one runnable check of it. Nothing here needs NBT or inflate code.
//!
//! How a structure gets into the world: a structure is a pure function of (seed, start chunk), so no start is saved. `populate(cx, cz)`
//! owns the 16 x 16 cell `[cx*16+8, cx*16+24)` (the part of the 2x2 `Region` every feature draws in); it asks which starts lie within
//! reach (`max_distance_from_center`, 80 blocks), assembles each (cached) and writes the part of every piece that lies in its cell.
//! A structure that crosses cells is therefore written by several populates and always the same.
//!
//! UNVERIFIED / simplified on purpose (no Random draw of the rest of populate is touched; this file draws from its own):
//! - `JigsawPlacement`, `RandomSpreadStructurePlacement`, the weighted pick of a set and the rule processors are written from memory of the
//!   1.20 Java, not read from it; nothing here is checked against a real world's structure positions.
//! - Heights come from `generate_far` (terrain without caves, rivers carved) instead of the 1.20 noise `getFirstFreeHeight`.
//! - SITE (2026-11-05, replaces the dirt foundations and the 2026-11-04 plinth rules): a structure is moved to flat, dry ground and
//!   the little slope that is left is levelled, so nothing stands on a platform, a pillar or a wall of dirt.
//!   1. `Structory::start` tries the nominal start and then shifts of 8 .. `SHIFT` blocks (nearest first) and keeps the first one whose
//!      rigid pieces (`Piece::rigid`) stand on ground within `FLAT` of the start's height, whose surroundings out to `PAD + MARGIN`
//!      are within `SIDE` of it with no step over `CLIFF`, and with no water, river, lake or ice within `SAFE` more blocks (`judge`).
//!      None fits: no structure there. Water structures (`Data::aquatic`: only Ocean / Swamp / Marsh in their biome mask: the boats
//!      and the swamp ruin) are not moved or levelled; they keep the natural ground.
//!   2. The levelling is `Site::height`, a pure function of (seed, start) and the `Ground` heights (`generate_far`, no caves, no trees),
//!      never of the `Region`: the ground is the start's target height under the rigid pieces and `PAD` blocks around them, and
//!      eases into the natural height over `MARGIN` blocks with a smoothstep. `flatten` rebuilds each column of a cell that way (cut or
//!      fill, the surface block and two filler layers of the column's own biome surface, snow kept, everything above cleared, so
//!      trees and plants of the area go). Several starts: the first in iteration order owns a column, the same from every cell.
//!   3. Trees, lakes and dungeons of `populate` are vetoed near a structure (`Start::blocks`), so a later populate cannot grow a tree
//!      through a wall (`TREE_VETO`); trees already there are cleared by the levelling.
//!   4. Foundations are gone. Only a gap of at most `GAP` (2) blocks under a load-bearing block of a rigid piece is filled. Terrain-
//!      matching pieces (paths) follow the levelled ground column by column like `GravityProcessor`.
//!   5. Baked ground (`base_run`, `base_depth`), the median-of-9 start height, air only under a roof (`roof_of`) and the load-bearing
//!      test (`bearing`) stay from the 2026-11-04 pass.
//! - No entities (villagers, illagers, item frames, armor stands), no sign text, no banners: Beta has none. Chests and barrels become
//!   chests with their loot table on Beta items; a spawner takes the template's mob (skeleton, zombie, spider).
//! - Where the Beta world has no such block the converter picked the nearest one (stone bricks = cobblestone, walls and iron bars =
//!   fences, carpets = nothing, ...), see `to_beta` in the tool.

use crate::world::biome::Biome;
use crate::world::chest::Chest;
use crate::world::chunk::idx;
use crate::world::gen::chunk_manager::WorldChunkManager;
use crate::world::gen::noise::JavaRandom;
use crate::world::gen::overworld::{block::*, OverworldGenerator};
use crate::world::gen::populate::{is_solid, Region};
use crate::world::items::ItemStack;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, OnceLock};

const SKIP: u16 = 0xFFFF;
const NONE16: u16 = 0xFFFF;
/// Deepest gap filled under a load-bearing block of a rigid piece (blocks): a 1 .. 2 block rest, never a foundation.
const GAP: i32 = 2;
/// Full flat ground around the rigid pieces, then the ease into the natural ground over `MARGIN` blocks (12 .. 20 in the plan).
const PAD: i32 = 2;
const MARGIN: i32 = 16;
/// Largest |natural ground - target| under (and `PAD` round) the rigid pieces, and in the margin.
const FLAT: i32 = 2;
const SIDE: i32 = 6;
/// Largest step between two neighbouring columns anywhere in the area (3 would be a wall).
const CLIFF: i32 = 2;
/// Extra blocks beyond the margin that must have no water, ice or lava.
const SAFE: i32 = 4;
/// The start may move this far from `spread`'s chunk (blocks), in steps of `SHIFT_STEP`.
const SHIFT: i32 = 48;
const SHIFT_STEP: i32 = 8;
/// A tree, lake or dungeon centre closer than this to a piece (blocks) is vetoed: `PAD` + the widest crown (7). `LAKE_VETO` covers a pit.
pub const TREE_VETO: i32 = 9;
pub const LAKE_VETO: i32 = 10;
/// Chunks a start can reach a cell from: `max_distance_from_center` 80 + `SHIFT` 48 + the area `PAD + MARGIN` 18 + the veto 10, rounded up.
const REACH: i32 = 10;
static BIN: &[u8] = include_bytes!("../../../assets/structory.bin");

// ---------------------------------------------------------------- the baked data

struct Jig {
    pos: [i32; 3],
    front: u8,
    top: u8,
    joint: u8,
    name: u16,
    target: u16,
    pool: u16,
}

struct Template {
    size: [i32; 3],
    /// One state per cell, `(y * sz + z) * sx + x`; `SKIP` where the template lists no block.
    cells: Vec<u16>,
    jig: Vec<Jig>,
    /// (x, y, z, kind, arg): kind 0 chest (arg = loot table), 1 spawner (arg = mob index of `pickMobSpawner`).
    ents: Vec<(i32, i32, i32, u8, u16)>,
}

#[derive(Clone, Copy)]
struct Rule {
    /// 16 bit chance of a `random_*` predicate, 65535 = no draw.
    chance: u16,
    /// The Beta id the world block must have, 255 = any.
    loc: u8,
    out: u16,
}

type Proc = HashMap<u16, Vec<Rule>>;

#[derive(Clone, Copy)]
struct Elem {
    w: u8,
    /// 0 empty, 1 single, 2 legacy (air is not placed).
    kind: u8,
    t: u16,
    proc: u8,
    /// 1 = terrain matching.
    proj: u8,
}

struct Pool {
    fb: u16,
    els: Vec<Elem>,
}

struct Struct {
    mask: u32,
    pool: u16,
    size: i32,
    adapt: u8,
    y: i32,
    dist: i32,
    hack: bool,
}

struct Set {
    salt: u32,
    spacing: i32,
    sep: i32,
    tri: bool,
    freq: u16,
    entries: Vec<(u8, u8)>,
}

/// (rolls min, rolls max, entries (weight, item id, damage, count min, count max, chance of 256)).
type LootPool = (u8, u8, Vec<(u8, u16, u16, u8, u8, u16)>);

pub struct Data {
    beta: Vec<(u8, u8)>,
    t: Vec<Template>,
    procs: Vec<Vec<Proc>>,
    pools: Vec<Pool>,
    structs: Vec<Struct>,
    sets: Vec<Set>,
    loot: Vec<Vec<LootPool>>,
}

struct Rd<'a> {
    b: &'a [u8],
    i: usize,
}

impl Rd<'_> {
    fn u8(&mut self) -> Option<u8> {
        let v = *self.b.get(self.i)?;
        self.i += 1;
        Some(v)
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes([self.u8()?, self.u8()?]))
    }
    fn i16(&mut self) -> Option<i16> {
        Some(self.u16()? as i16)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from(self.u16()?) | (u32::from(self.u16()?) << 16))
    }
}

impl Data {
    /// Structures that belong in water (boats, the swamp ruin): the ones that can only start in Ocean, Swamp or Marsh. `structory.bin`
    /// keeps no names, so this is read from the biome mask baked from each structure's `biomes` tag.
    fn aquatic(&self, st: usize) -> bool {
        let wet = (1u32 << Biome::Ocean as u8) | (1 << Biome::Swamp as u8) | (1 << Biome::Marsh as u8);
        self.structs[st].mask & !wet == 0
    }

    /// The layout `tools/gen_structory.py` writes (and `structory_proto.py` reads).
    pub fn parse(b: &[u8]) -> Option<Data> {
        if b.get(..4)? != b"STRY" || *b.get(4)? != 1 {
            return None;
        }
        let mut r = Rd { b, i: 5 };
        let mut beta = Vec::new();
        for _ in 0..r.u16()? {
            beta.push((r.u8()?, r.u8()?));
        }
        for _ in 0..r.u16()? {
            let n = usize::from(r.u8()?);
            r.i += n; // the jigsaw names: compared as ids, never read
        }
        let mut t = Vec::new();
        for _ in 0..r.u16()? {
            let size = [i32::from(r.u8()?), i32::from(r.u8()?), i32::from(r.u8()?)];
            let n = (size[0] * size[1] * size[2]) as usize;
            let mut cells = Vec::with_capacity(n);
            for _ in 0..n {
                cells.push(r.u16()?);
            }
            let mut jig = Vec::new();
            for _ in 0..r.u16()? {
                let pos = [i32::from(r.u8()?), i32::from(r.u8()?), i32::from(r.u8()?)];
                jig.push(Jig { pos, front: r.u8()?, top: r.u8()?, joint: r.u8()?, name: r.u16()?, target: r.u16()?, pool: r.u16()? });
            }
            let mut ents = Vec::new();
            for _ in 0..r.u16()? {
                ents.push((i32::from(r.u8()?), i32::from(r.u8()?), i32::from(r.u8()?), r.u8()?, r.u16()?));
            }
            t.push(Template { size, cells, jig, ents });
        }
        let mut procs = Vec::new();
        for _ in 0..r.u16()? {
            let mut list = Vec::new();
            for _ in 0..r.u8()? {
                let mut per = Proc::new();
                for _ in 0..r.u16()? {
                    let st = r.u16()?;
                    let mut rules = Vec::new();
                    for _ in 0..r.u8()? {
                        rules.push(Rule { chance: r.u16()?, loc: r.u8()?, out: r.u16()? });
                    }
                    per.insert(st, rules);
                }
                list.push(per);
            }
            procs.push(list);
        }
        let mut pools = Vec::new();
        for _ in 0..r.u16()? {
            let fb = r.u16()?;
            let mut els = Vec::new();
            for _ in 0..r.u8()? {
                els.push(Elem { w: r.u8()?, kind: r.u8()?, t: r.u16()?, proc: r.u8()?, proj: r.u8()? });
            }
            pools.push(Pool { fb, els });
        }
        let mut structs = Vec::new();
        for _ in 0..r.u8()? {
            structs.push(Struct {
                mask: r.u32()?,
                pool: r.u16()?,
                size: i32::from(r.u8()?),
                adapt: r.u8()?,
                y: i32::from(r.i16()?),
                dist: i32::from(r.u8()?),
                hack: r.u8()? != 0,
            });
        }
        let mut sets = Vec::new();
        for _ in 0..r.u8()? {
            let (salt, spacing, sep, tri, freq) = (r.u32()?, i32::from(r.u8()?), i32::from(r.u8()?), r.u8()? != 0, r.u16()?);
            let mut entries = Vec::new();
            for _ in 0..r.u8()? {
                entries.push((r.u8()?, r.u8()?));
            }
            sets.push(Set { salt, spacing, sep, tri, freq, entries });
        }
        let mut loot = Vec::new();
        for _ in 0..r.u8()? {
            let mut pl = Vec::new();
            for _ in 0..r.u8()? {
                let (lo, hi) = (r.u8()?, r.u8()?);
                let mut es = Vec::new();
                for _ in 0..r.u8()? {
                    es.push((r.u8()?, r.u16()?, r.u16()?, r.u8()?, r.u8()?, r.u16()?));
                }
                pl.push((lo, hi, es));
            }
            loot.push(pl);
        }
        // every reference the game follows must exist (a stale `structory.bin` next to new code fails here, not in a worker)
        let ok = t.iter().all(|t| t.cells.iter().all(|&c| c == SKIP || (c as usize) < beta.len()))
            && pools.iter().all(|p| (p.fb == NONE16 || (p.fb as usize) < pools.len()) && p.els.iter().all(|e| e.kind == 0 || (e.t as usize) < t.len()))
            && t.iter().all(|t| t.jig.iter().all(|j| j.pool == NONE16 || (j.pool as usize) < pools.len()))
            && structs.iter().all(|s| (s.pool as usize) < pools.len())
            && sets.iter().all(|s| s.entries.iter().all(|e| (e.0 as usize) < structs.len()));
        ok.then_some(Data { beta, t, procs, pools, structs, sets, loot })
    }
}

fn data() -> Option<&'static Data> {
    static D: OnceLock<Option<Data>> = OnceLock::new();
    D.get_or_init(|| Data::parse(BIN)).as_ref()
}

// ---------------------------------------------------------------- geometry

type Bx = [i32; 6];
const STEP: [[i32; 3]; 6] = [[0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]];
const OPP: [u8; 6] = [1, 0, 3, 2, 5, 4];

/// `Rotation.rotate` about the origin, `k` quarter turns clockwise: (x, z) -> (-z, x).
fn rot_xz(x: i32, z: i32, k: u8) -> (i32, i32) {
    let (mut x, mut z) = (x, z);
    for _ in 0..k {
        (x, z) = (-z, x);
    }
    (x, z)
}

/// A direction (0 down, 1 up, 2 north, 3 south, 4 west, 5 east) turned `k` quarter turns clockwise.
fn rot_dir(d: u8, k: u8) -> u8 {
    let mut d = d;
    for _ in 0..k {
        d = match d {
            2 => 5,
            5 => 3,
            3 => 4,
            4 => 2,
            _ => d,
        };
    }
    d
}

/// `StructureTemplate.getBoundingBox(pos, rotation)`: the rotated template's box, its origin at `pos`.
fn box_of(size: [i32; 3], rot: u8, pos: [i32; 3]) -> Bx {
    let (a, b) = (rot_xz(0, 0, rot), rot_xz(size[0] - 1, size[2] - 1, rot));
    [pos[0] + a.0.min(b.0), pos[1], pos[2] + a.1.min(b.1), pos[0] + a.0.max(b.0), pos[1] + size[1] - 1, pos[2] + a.1.max(b.1)]
}

fn inside(b: &Bx, p: [i32; 3]) -> bool {
    b[0] <= p[0] && p[0] <= b[3] && b[1] <= p[1] && p[1] <= b[4] && b[2] <= p[2] && p[2] <= b[5]
}

fn meets(a: &Bx, b: &Bx) -> bool {
    a[0] <= b[3] && a[3] >= b[0] && a[1] <= b[4] && a[4] >= b[1] && a[2] <= b[5] && a[5] >= b[2]
}

/// `BoundingBox.getCenter` on x / z.
fn centre(b: &Bx) -> (i32, i32) {
    (b[0] + (b[3] - b[0] + 1) / 2, b[2] + (b[5] - b[2] + 1) / 2)
}

/// The jigsaw blocks of template `t` turned by `rot` and moved to `pos`.
fn jigsaws(d: &Data, t: usize, rot: u8, pos: [i32; 3]) -> Vec<Jig> {
    d.t[t]
        .jig
        .iter()
        .map(|j| {
            let (rx, rz) = rot_xz(j.pos[0], j.pos[2], rot);
            Jig {
                pos: [pos[0] + rx, pos[1] + j.pos[1], pos[2] + rz],
                front: rot_dir(j.front, rot),
                top: rot_dir(j.top, rot),
                joint: j.joint,
                name: j.name,
                target: j.target,
                pool: j.pool,
            }
        })
        .collect()
}

/// `JigsawBlock.canAttach`: opposite fronts, the same top unless the joint is rollable, and the target names the other's name.
fn can_attach(a: &Jig, b: &Jig) -> bool {
    a.front == OPP[usize::from(b.front)] && (a.joint == 0 || a.top == b.top) && a.target == b.name
}

/// `Util.shuffle`.
fn shuffle<T>(l: &mut [T], r: &mut JavaRandom) {
    for i in (2..=l.len()).rev() {
        let j = r.next_int_bound(i as i32) as usize;
        l.swap(i - 1, j);
    }
}

// ---------------------------------------------------------------- natural terrain

/// Natural ground the templates bake into their bottom layers.
fn is_ground(id: u8) -> bool {
    matches!(id, GRASS | DIRT | SAND | GRAVEL)
}

/// Blocks that carry a building (walls, floors, roofs): a foundation is only built under these.
fn bearing(id: u8) -> bool {
    matches!(id, STONE | COBBLE | 5 | LOG | SANDSTONE | 35 | 43 | 44 | 45 | MOSSY | 49 | 53 | 67 | 82)
}

fn is_liquid(id: u8) -> bool {
    matches!(id, WATER_MOVING..=LAVA)
}

/// How many bottom layers of template `t` are baked ground (0..=3): the most common run of grass / dirt / sand / gravel from y = 0 up,
/// if at least 40% of the footprint has one. Such a template is a house on a grass plinth; the plinth is not written (see `place_piece`).
fn base_depth(d: &Data, t: usize) -> i32 {
    let tp = &d.t[t];
    let [sx, sy, sz] = tp.size;
    let mut count = [0usize; 4];
    for z in 0..sz {
        for x in 0..sx {
            let mut n = 0;
            while n < sy.min(3) {
                let c = tp.cells[((n * sz + z) * sx + x) as usize];
                if c == SKIP || !is_ground(d.beta[usize::from(c)].0) {
                    break;
                }
                n += 1;
            }
            count[n as usize] += 1;
        }
    }
    (1..4usize).max_by_key(|&n| count[n]).filter(|&n| count[n] * 5 >= (sx * sz) as usize * 2).map_or(0, |n| n as i32)
}

/// Per column of template `t` (`z * sx + x`): the height of its highest load-bearing block, -1 if there is none. Air below it is
/// under a roof (an interior); air at or above it is open sky and is not written.
fn roof_of(d: &Data, t: usize) -> Vec<i32> {
    let tp = &d.t[t];
    let [sx, sy, sz] = tp.size;
    let mut roof = vec![-1; (sx * sz) as usize];
    for y in 0..sy {
        for z in 0..sz {
            for x in 0..sx {
                let c = tp.cells[((y * sz + z) * sx + x) as usize];
                if c != SKIP && bearing(d.beta[usize::from(c)].0) {
                    roof[(z * sx + x) as usize] = y;
                }
            }
        }
    }
    roof
}

// ---------------------------------------------------------------- assembly

#[derive(Clone)]
struct Piece {
    t: usize,
    rot: u8,
    pos: [i32; 3],
    bx: Bx,
    rigid: bool,
    kind: u8,
    proc: u8,
}

/// A free space: the outer box and the boxes taken out of it (`Shapes.joinUnoptimized(.., ONLY_FIRST)`).
fn fits(outer: &Bx, holes: &[Bx], b: &Bx) -> bool {
    outer[0] <= b[0] && b[3] <= outer[3] && outer[1] <= b[1] && b[4] <= outer[4] && outer[2] <= b[2] && b[5] <= outer[5] && !holes.iter().any(|h| meets(b, h))
}

/// `JigsawPlacement.addPieces` + `Placer.tryPlacingChildren` of 1.20 for structure `st` starting in chunk (cx, cz). `ground(x, z)` is
/// the first free height there; `off` moves the whole structure from the chunk corner (the site search). Twin of `structory_proto.assemble`.
fn assemble(d: &Data, st: usize, cx: i32, cz: i32, off: (i32, i32), rnd: &mut JavaRandom, ground: &mut dyn FnMut(i32, i32) -> i32) -> Vec<Piece> {
    let s = &d.structs[st];
    let flat = |ix: u16| -> Vec<Elem> { d.pools[usize::from(ix)].els.iter().flat_map(|e| std::iter::repeat(*e).take(usize::from(e.w))).collect() };
    let pool_max = |ix: u16| -> i32 { d.pools[usize::from(ix)].els.iter().filter(|e| e.kind != 0).map(|e| d.t[usize::from(e.t)].size[1]).max().unwrap_or(0) };
    let first = flat(s.pool);
    if first.is_empty() {
        return Vec::new();
    }
    let start = first[rnd.next_int_bound(first.len() as i32) as usize];
    if start.kind == 0 {
        return Vec::new();
    }
    let rot = rnd.next_int_bound(4) as u8;
    let (bx, bz) = (cx * 16 + off.0, cz * 16 + off.1);
    let size = d.t[usize::from(start.t)].size;
    let b0 = box_of(size, rot, [bx, 0, bz]);
    let c0 = centre(&b0);
    let mut pos = [bx - (c0.0 - bx), 0, bz - (c0.1 - bz)]; // the box's centre goes to the chunk's corner
    let mut bxx = box_of(size, rot, pos);
    let g = centre(&bxx);
    // the median of 9 samples over the footprint (not the one under the centre: on a slope that left one side floating and the other
    // buried), less the baked ground of the template so that its walls, not its grass plinth, stand on the surface
    let mut hs = Vec::with_capacity(9);
    for &x in &[bxx[0], g.0, bxx[3]] {
        for &z in &[bxx[2], g.1, bxx[5]] {
            hs.push(ground(x, z));
        }
    }
    hs.sort_unstable();
    pos[1] = s.y + hs[4] - base_depth(d, usize::from(start.t));
    bxx = box_of(size, rot, pos);
    let p0 = Piece { t: usize::from(start.t), rot, pos, bx: bxx, rigid: start.proj == 0, kind: start.kind, proc: start.proc };
    let mut pieces = vec![p0.clone()];
    let area: Bx = [g.0 - s.dist, 0, g.1 - s.dist, g.0 + s.dist, 127, g.1 + s.dist];
    let mut free_holes: Vec<Bx> = vec![bxx];
    let mut queue: VecDeque<(Piece, i32)> = VecDeque::new();
    queue.push_back((p0, 0));
    while let Some((pc, depth)) = queue.pop_front() {
        let (pb, rigid) = (pc.bx, pc.rigid);
        let mut k: Option<i32> = None;
        let mut inner: Vec<Bx> = Vec::new();
        let mut jigs = jigsaws(d, pc.t, pc.rot, pc.pos);
        shuffle(&mut jigs, rnd);
        for jg in &jigs {
            if jg.pool == NONE16 {
                continue;
            }
            let f = STEP[usize::from(jg.front)];
            let tpos = [jg.pos[0] + f[0], jg.pos[1] + f[1], jg.pos[2] + f[2]];
            let j = jg.pos[1] - pb[1];
            let in_parent = inside(&pb, tpos);
            let fb = d.pools[usize::from(jg.pool)].fb;
            let mut cand: Vec<Elem> = Vec::new();
            if depth != s.size {
                let mut l = flat(jg.pool);
                shuffle(&mut l, rnd);
                cand.extend(l);
            }
            if fb != NONE16 {
                let mut l = flat(fb);
                shuffle(&mut l, rnd);
                cand.extend(l);
            }
            'cand: for c in cand {
                if c.kind == 0 {
                    break;
                }
                let mut rots = [0u8, 1, 2, 3];
                shuffle(&mut rots, rnd);
                for r2 in rots {
                    let tj = jigsaws(d, usize::from(c.t), r2, [0, 0, 0]);
                    let cs = d.t[usize::from(c.t)].size;
                    let cb = box_of(cs, r2, [0, 0, 0]);
                    let mut i1 = 0;
                    if s.hack && cb[4] - cb[1] + 1 <= 16 {
                        for ij in &tj {
                            let f = STEP[usize::from(ij.front)];
                            if ij.pool != NONE16 && inside(&cb, [ij.pos[0] + f[0], ij.pos[1] + f[1], ij.pos[2] + f[2]]) {
                                let ifb = d.pools[usize::from(ij.pool)].fb;
                                i1 = i1.max(pool_max(ij.pool)).max(if ifb != NONE16 { pool_max(ifb) } else { 0 });
                            }
                        }
                    }
                    for cj in &tj {
                        if !can_attach(jg, cj) {
                            continue;
                        }
                        let off = [tpos[0] - cj.pos[0], tpos[1] - cj.pos[1], tpos[2] - cj.pos[2]];
                        let cb2 = box_of(cs, r2, off);
                        let crigid = c.proj == 0;
                        let l1 = j - cj.pos[1] + f[1];
                        let i2 = if rigid && crigid {
                            pb[1] + l1
                        } else {
                            let kk = *k.get_or_insert_with(|| ground(jg.pos[0], jg.pos[2]));
                            kk - cj.pos[1]
                        };
                        let j2 = i2 - cb2[1];
                        let mut cb3 = [cb2[0], cb2[1] + j2, cb2[2], cb2[3], cb2[4] + j2, cb2[5]];
                        if i1 > 0 {
                            cb3[4] = cb3[4].max(cb3[1] + (i1 + 1).max(cb3[4] - cb3[1] + 1) - 1);
                        }
                        let ok = if in_parent { fits(&pb, &inner, &cb3) } else { fits(&area, &free_holes, &cb3) };
                        if ok {
                            if in_parent { inner.push(cb3) } else { free_holes.push(cb3) }
                            let np = Piece { t: usize::from(c.t), rot: r2, pos: [off[0], off[1] + j2, off[2]], bx: cb3, rigid: crigid, kind: c.kind, proc: c.proc };
                            pieces.push(np.clone());
                            if depth + 1 <= s.size {
                                queue.push_back((np, depth + 1));
                            }
                            break 'cand;
                        }
                    }
                }
            }
        }
    }
    pieces
}

// ---------------------------------------------------------------- starts

/// A structure that starts in one chunk: its pieces, assembled once.
pub struct Start {
    st: usize,
    pieces: Vec<Piece>,
    site: Site,
}

impl Start {
    /// Whether a tree, lake or dungeon centred at (x, z) would come within `r` blocks of a piece of this structure.
    pub fn blocks(&self, x: i32, z: i32, r: i32) -> bool {
        self.site.blocks(x, z, r)
    }
}

/// `RandomSpreadStructurePlacement.getPotentialStructureChunk` of the region (rx, rz).
fn spread(seed: i64, rx: i32, rz: i32, s: &Set) -> (i32, i32) {
    let mut r = JavaRandom::new(large_feature(seed, rx, rz, s.salt));
    let k = s.spacing - s.sep;
    let one = |r: &mut JavaRandom| if s.tri { (r.next_int_bound(k) + r.next_int_bound(k)) / 2 } else { r.next_int_bound(k) };
    let (a, b) = (one(&mut r), one(&mut r));
    (rx * s.spacing + a, rz * s.spacing + b)
}

/// `WorldgenRandom.setLargeFeatureWithSalt`.
fn large_feature(seed: i64, x: i32, z: i32, salt: u32) -> i64 {
    i64::from(x).wrapping_mul(341873128712).wrapping_add(i64::from(z).wrapping_mul(132897987541)).wrapping_add(seed).wrapping_add(i64::from(salt))
}

/// `Mth.getSeed(x, y, z)`: what a rule processor seeds its Random with.
fn pos_seed(x: i32, y: i32, z: i32) -> i64 {
    let l = i64::from(x.wrapping_mul(3129871)) ^ i64::from(z).wrapping_mul(116129781) ^ i64::from(y);
    l.wrapping_mul(l).wrapping_mul(42317861).wrapping_add(l.wrapping_mul(11)) >> 16
}

/// The ground under a column of the terrain (`generate_far`, so no caves and no trees), first free height, kept per chunk. A column
/// is `height | wet << 8`: wet = its top block is water, lava or ice (sea, river, lake).
struct Ground<'a> {
    g: &'a mut OverworldGenerator,
    cm: &'a mut WorldChunkManager,
    h: &'a mut HashMap<(i32, i32), Vec<u16>>,
}

impl Ground<'_> {
    fn col(&mut self, x: i32, z: i32) -> u16 {
        let key = (x >> 4, z >> 4);
        if !self.h.contains_key(&key) {
            if self.h.len() > 900 {
                self.h.clear(); // backstop only: `Structory::start` clears at 400 between starts (about 0.5 MB at 900)
            }
            let b = self.g.generate_far(key.0, key.1, self.cm);
            let col = (0..256usize)
                .map(|i| {
                    let base = idx(i / 16, 0, i % 16);
                    (0..128).rev().find(|&y| b[base + y] != AIR).map_or(0u16, |y| {
                        let t = b[base + y];
                        (y as u16 + 1) | (u16::from(is_liquid(t) || t == ICE) << 8)
                    })
                })
                .collect();
            self.h.insert(key, col);
        }
        self.h[&key][((x & 15) * 16 + (z & 15)) as usize]
    }
    fn at(&mut self, x: i32, z: i32) -> i32 {
        i32::from(self.col(x, z) & 0xFF)
    }
}

// ---------------------------------------------------------------- the site

/// The flat ground of a structure: what `judge` accepted and `flatten` builds.
pub struct Site {
    /// Whether the ground is levelled (false for the water structures).
    flat: bool,
    /// The ground height under the start piece (the median of 9 samples) that every column of the pad is brought to.
    target: i32,
    /// x0, z0, x1, z1 (inclusive) of every rigid piece; they are what is levelled.
    rigid: Vec<[i32; 4]>,
    /// The same for every piece, rigid or not: what trees, lakes and dungeons keep away from.
    all: Vec<[i32; 4]>,
    /// The box a cell must meet for this start to matter to it: every piece, `PAD + MARGIN` and the veto round it.
    zone: [i32; 4],
}

/// `target` eased to the natural height `n` by the smoothstep of `k` blocks of `MARGIN` (integer maths after the one `f64` sqrt, so every
/// cell and every thread gets the same height).
fn blend(target: i32, n: i32, k: f64) -> i32 {
    let t = ((k / f64::from(MARGIN)) * 1024.0).clamp(0.0, 1024.0) as i32;
    let s = (t * t * (3072 - 2 * t)) >> 20; // 0 ..= 1024
    target + ((n - target) * s + 512).div_euclid(1024)
}

fn rect_dist(r: &[i32; 4], x: i32, z: i32) -> f64 {
    let dx = (r[0] - x).max(0).max(x - r[2]);
    let dz = (r[1] - z).max(0).max(z - r[3]);
    f64::from(dx * dx + dz * dz).sqrt()
}

impl Site {
    /// The levelled height of column (x, z) over natural ground `n`, and whether the column is under the pad (the structure itself);
    /// `None` outside the area, where nothing is touched.
    fn height(&self, x: i32, z: i32, n: i32) -> Option<(i32, bool)> {
        let d = self.rigid.iter().map(|r| rect_dist(r, x, z)).fold(f64::MAX, f64::min);
        if d > f64::from(PAD + MARGIN) {
            return None;
        }
        let k = (d - f64::from(PAD)).max(0.0);
        Some((blend(self.target, n, k), k == 0.0))
    }

    /// Whether (x, z) is within `r` blocks of a piece.
    fn blocks(&self, x: i32, z: i32, r: i32) -> bool {
        self.all.iter().any(|a| a[0] - r <= x && x <= a[2] + r && a[1] - r <= z && z <= a[3] + r)
    }
}

/// The columns a..=b every 2 blocks, always with b.
fn every2(a: i32, b: i32) -> impl Iterator<Item = i32> {
    (a..=b).step_by(2).chain((b > a && (b - a) % 2 == 1).then_some(b))
}

/// Accepts or refuses the assembled `pieces` on the ground `at(x, z) = (height, wet)` for the target `target`: the rigid pieces on ground
/// within `FLAT` of it, their surroundings within `SIDE` out to `PAD + MARGIN`, no step over `CLIFF` between neighbours, and no water within
/// `SAFE` more. A pure function of the ground, so every cell and every worker comes to the same answer.
fn judge(pieces: &[Piece], target: i32, flat: bool, at: &mut dyn FnMut(i32, i32) -> (i32, bool)) -> Option<Site> {
    let rect = |p: &Piece| [p.bx[0], p.bx[2], p.bx[3], p.bx[5]];
    let all: Vec<[i32; 4]> = pieces.iter().map(rect).collect();
    let rigid: Vec<[i32; 4]> = pieces.iter().filter(|p| p.rigid).map(rect).collect();
    let mut zone = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
    for a in &all {
        let e = PAD + MARGIN + LAKE_VETO;
        zone = [zone[0].min(a[0] - e), zone[1].min(a[1] - e), zone[2].max(a[2] + e), zone[3].max(a[3] + e)];
    }
    if flat && !rigid.is_empty() {
        let reach = PAD + MARGIN;
        for r in &rigid {
            for x in every2(r[0] - reach - SAFE, r[2] + reach + SAFE) {
                for z in every2(r[1] - reach - SAFE, r[3] + reach + SAFE) {
                    let (n, wet) = at(x, z);
                    if wet {
                        return None;
                    }
                    let d = rect_dist(r, x, z);
                    let dev = (n - target).abs();
                    if (d <= f64::from(PAD) && dev > FLAT) || (d <= f64::from(reach) && dev > SIDE) {
                        return None;
                    }
                    if d <= f64::from(reach) && ((at(x + 1, z).0 - n).abs() > CLIFF || (at(x, z + 1).0 - n).abs() > CLIFF) {
                        return None;
                    }
                }
            }
        }
    }
    Some(Site { flat: flat && !rigid.is_empty(), target, rigid, all, zone })
}

/// Rebuild column (x, z) so its ground is at `h` (one above the top block): cut or fill, the column's own surface block and two filler
/// layers, snow kept, everything above cleared. `force` clears even when the height is right (the pad: no tree stays inside a wall).
fn rebuild(w: &mut Region, x: i32, z: i32, h: i32, force: bool) {
    let cur = region_ground(w, x, z);
    if cur == h && !force {
        return;
    }
    let (mut top, mut fill) = (if cur >= 2 { w.get(x, cur - 1, z) } else { GRASS }, if cur >= 3 { w.get(x, cur - 2, z) } else { DIRT });
    if is_liquid(top) || top == ICE || top == AIR {
        (top, fill) = (GRASS, DIRT);
    }
    if is_liquid(fill) || fill == AIR {
        fill = DIRT;
    }
    let snow = w.get(x, cur, z) == SNOW;
    for y in h.max(1)..128 {
        if w.get(x, y, z) != AIR {
            w.set(x, y, z, AIR);
        }
    }
    if h < 2 {
        return;
    }
    let from = if h > cur { cur - 1 } else { h - 3 };
    for y in from.max(1)..h - 1 {
        w.set(x, y, z, fill);
    }
    w.set(x, h - 1, z, top);
    if snow && h < 128 && is_solid(top) {
        w.set(x, h, z, SNOW);
    }
}

/// Level the columns of `cell` that `s` owns: the pad and its margin. A column already taken by an earlier start of this cell is left.
fn flatten(s: &Start, gr: &mut Ground, w: &mut Region, cell: [i32; 4], claimed: &mut [[bool; 16]; 16]) {
    let z0 = s.site.zone;
    if !s.site.flat || z0[0] > cell[2] || z0[2] < cell[0] || z0[1] > cell[3] || z0[3] < cell[1] {
        return;
    }
    for x in cell[0]..=cell[2] {
        for z in cell[1]..=cell[3] {
            let c = &mut claimed[(x - cell[0]) as usize][(z - cell[1]) as usize];
            if *c {
                continue;
            }
            let n = gr.at(x, z);
            if let Some((h, pad)) = s.site.height(x, z, n) {
                *c = true;
                rebuild(w, x, z, h, pad);
            }
        }
    }
}

/// Whether `structory.bin` loaded (the Blooming world has structures at all).
pub fn available() -> bool {
    data().is_some()
}

/// Number of structures, and the biomes (bit `Biome as u8`) structure `st` can start in.
pub fn count() -> usize {
    data().map_or(0, |d| d.structs.len())
}

pub fn mask(st: usize) -> u32 {
    data().and_then(|d| d.structs.get(st)).map_or(0, |s| s.mask)
}

/// Where structures start inside the block box (x0, z0) .. (x1, z1), for the World Preview: (x, z of the start chunk's middle,
/// structure index). The same `spread`, frequency roll and weighted pick as `Structory::start`, with the biome read through
/// `biome`; the pieces are not assembled, so a start that `assemble` would refuse (rare) still shows.
pub fn starts(seed: i64, x0: i32, z0: i32, x1: i32, z1: i32, biome: &mut dyn FnMut(i32, i32) -> Biome) -> Vec<(i32, i32, u8)> {
    let Some(d) = data() else { return Vec::new() };
    let (cx0, cx1, cz0, cz1) = (x0 >> 4, x1 >> 4, z0 >> 4, z1 >> 4);
    let mut out = Vec::new();
    for set in &d.sets {
        for rx in cx0.div_euclid(set.spacing)..=cx1.div_euclid(set.spacing) {
            for rz in cz0.div_euclid(set.spacing)..=cz1.div_euclid(set.spacing) {
                let (sx, sz) = spread(seed, rx, rz, set);
                if sx < cx0 || sx > cx1 || sz < cz0 || sz > cz1 {
                    continue;
                }
                if set.freq != 65535 && JavaRandom::new(large_feature(seed, sx, sz, set.salt)).next_float() >= f32::from(set.freq) / 65535.0 {
                    continue;
                }
                let mut r = JavaRandom::new(seed);
                let (a, b) = (r.next_long(), r.next_long());
                r.set_seed(i64::from(sx).wrapping_mul(a) ^ i64::from(sz).wrapping_mul(b) ^ seed);
                let mut list = set.entries.clone();
                let mut total: i32 = list.iter().map(|e| i32::from(e.1)).sum();
                let b = biome(sx * 16, sz * 16);
                while !list.is_empty() && total > 0 {
                    let mut k = r.next_int_bound(total);
                    let mut at = list.len() - 1;
                    for (i, e) in list.iter().enumerate() {
                        k -= i32::from(e.1);
                        if k < 0 {
                            at = i;
                            break;
                        }
                    }
                    let (ix, w) = list.remove(at);
                    total -= i32::from(w);
                    if d.structs[usize::from(ix)].mask >> (b as u8) & 1 != 0 {
                        out.push((sx * 16 + 8, sz * 16 + 8, ix));
                        break;
                    }
                }
            }
        }
    }
    out
}

/// The per-generator state of the structures: the starts already assembled and the heights they needed.
pub struct Structory {
    seed: i64,
    starts: HashMap<(usize, i32, i32), Option<Arc<Start>>>,
    heights: HashMap<(i32, i32), Vec<u16>>,
}

impl Structory {
    /// `None` when `structory.bin` is stale or broken (the Blooming world then simply has no structures).
    pub fn new(seed: i64) -> Option<Self> {
        data().map(|_| Self { seed, starts: HashMap::new(), heights: HashMap::new() })
    }

    /// The structure that starts in chunk (sx, sz) of set `si`, if there is one (`ChunkGenerator.tryGenerateStructures`).
    fn start(&mut self, d: &'static Data, g: &mut OverworldGenerator, cm: &mut WorldChunkManager, si: usize, sx: i32, sz: i32) -> Option<Arc<Start>> {
        if let Some(s) = self.starts.get(&(si, sx, sz)) {
            return s.clone();
        }
        let set = &d.sets[si];
        let mut made = None;
        // frequency reduction (`probabilityReducer`)
        let passes = set.freq == 65535 || JavaRandom::new(large_feature(self.seed, sx, sz, set.salt)).next_float() < f32::from(set.freq) / 65535.0;
        if passes {
            let mut r = JavaRandom::new(self.seed); // setLargeFeatureSeed
            let (a, b) = (r.next_long(), r.next_long());
            r.set_seed(i64::from(sx).wrapping_mul(a) ^ i64::from(sz).wrapping_mul(b) ^ self.seed);
            let mut list = set.entries.clone();
            let mut total: i32 = list.iter().map(|e| i32::from(e.1)).sum();
            // the weighted pick; a structure that cannot start here (its biome, no piece) is dropped and the next one tried
            while !list.is_empty() && total > 0 {
                let mut k = r.next_int_bound(total);
                let mut at = list.len() - 1;
                for (i, e) in list.iter().enumerate() {
                    k -= i32::from(e.1);
                    if k < 0 {
                        at = i;
                        break;
                    }
                }
                let (ix, w) = list.remove(at);
                total -= i32::from(w);
                let st = usize::from(ix);
                let biome = cm.load_block_generator_data(sx * 16, sz * 16, 1, 1)[0];
                if d.structs[st].mask >> (biome as u8) & 1 == 0 {
                    continue;
                }
                // the nominal start, then shifts nearest first; the first flat, dry site wins (`judge`); water structures stay put
                let offs = if d.aquatic(st) { vec![(0, 0)] } else { shifts() };
                if self.heights.len() > 400 {
                    self.heights.clear(); // between starts only, never in the middle of a search
                }
                let mut gr = Ground { g: &mut *g, cm: &mut *cm, h: &mut self.heights };
                for off in offs {
                    let mut rr = r.clone();
                    let pieces = assemble(d, st, sx, sz, off, &mut rr, &mut |x, z| gr.at(x, z));
                    let Some(p0) = pieces.first() else { break }; // the same draws whatever the shift: no start piece, no structure
                    let target = p0.pos[1] - d.structs[st].y + base_depth(d, p0.t);
                    let flat = !d.aquatic(st);
                    if let Some(site) = judge(&pieces, target, flat, &mut |x, z| {
                        let c = gr.col(x, z);
                        (i32::from(c & 0xFF), c >> 8 != 0)
                    }) {
                        made = Some(Arc::new(Start { st, pieces, site }));
                        break;
                    }
                }
                if made.is_some() {
                    break;
                }
            }
        }
        self.starts.insert((si, sx, sz), made.clone());
        made
    }

    /// The structures whose area, veto or pieces can touch the cell of chunk (cx, cz), in a fixed order (set, region x, region z).
    pub fn near(&mut self, g: &mut OverworldGenerator, cx: i32, cz: i32, cm: &mut WorldChunkManager) -> Vec<Arc<Start>> {
        let Some(d) = data() else { return Vec::new() };
        let cell = [cx * 16 + 8, cz * 16 + 8, cx * 16 + 23, cz * 16 + 23];
        let mut out = Vec::new();
        for (si, set) in d.sets.iter().enumerate() {
            let (rx0, rx1) = ((cx - REACH).div_euclid(set.spacing), (cx + 1 + REACH).div_euclid(set.spacing));
            let (rz0, rz1) = ((cz - REACH).div_euclid(set.spacing), (cz + 1 + REACH).div_euclid(set.spacing));
            for rx in rx0..=rx1 {
                for rz in rz0..=rz1 {
                    let (sx, sz) = spread(self.seed, rx, rz, set);
                    if (sx * 16 - cell[0]).abs() > 16 * REACH + 16 || (sz * 16 - cell[1]).abs() > 16 * REACH + 16 {
                        continue;
                    }
                    let Some(start) = self.start(d, g, cm, si, sx, sz) else { continue };
                    let z = start.site.zone;
                    if z[0] <= cell[2] && z[2] >= cell[0] && z[1] <= cell[3] && z[3] >= cell[1] {
                        out.push(start);
                    }
                }
            }
        }
        out
    }

    /// Write what the structures `near` put in the cell of chunk (cx, cz): first the levelled ground of all of them, then the pieces.
    /// Called at the end of `populate`, after the features.
    pub fn populate(&mut self, g: &mut OverworldGenerator, w: &mut Region, cx: i32, cz: i32, cm: &mut WorldChunkManager, near: &[Arc<Start>]) {
        let Some(d) = data() else { return };
        let cell = [cx * 16 + 8, cz * 16 + 8, cx * 16 + 23, cz * 16 + 23];
        if near.iter().any(|s| s.site.flat) {
            let mut claimed = [[false; 16]; 16];
            let mut gr = Ground { g: &mut *g, cm: &mut *cm, h: &mut self.heights };
            for s in near {
                flatten(s, &mut gr, w, cell, &mut claimed);
            }
        }
        for start in near {
            let s = &d.structs[start.st];
            for p in &start.pieces {
                if p.bx[0] <= cell[2] && p.bx[3] >= cell[0] && p.bx[2] <= cell[3] && p.bx[5] >= cell[1] {
                    place_piece(d, s, p, w, cell, self.seed);
                }
            }
        }
    }
}

/// The offsets the site search tries: (0, 0), then multiples of `SHIFT_STEP` up to `SHIFT`, the nearest first.
fn shifts() -> Vec<(i32, i32)> {
    let n = SHIFT / SHIFT_STEP;
    let mut v: Vec<(i32, i32)> = (-n..=n).flat_map(|i| (-n..=n).map(move |j| (i * SHIFT_STEP, j * SHIFT_STEP))).collect();
    v.sort_by_key(|&(x, z)| (x * x + z * z, x, z));
    v
}

// ---------------------------------------------------------------- writing a piece

/// A block's metadata turned `rot` quarter turns clockwise (the Beta metadata of what faces somewhere).
fn rotate_meta(id: u8, m: u8, rot: u8) -> u8 {
    let mut m = m;
    for _ in 0..rot {
        let facing = |v: u8| match v {
            2 => 5,
            5 => 3,
            3 => 4,
            4 => 2,
            _ => v,
        };
        m = match id {
            53 | 67 => (m & !3) | [2, 3, 1, 0][usize::from(m & 3)],
            23 | 54 | 61 | 62 | 65 | 68 => facing(m),
            33 | 29 => (m & 8) | facing(m & 7),
            50 | 75 | 76 | 77 | 69 => (m & 8) | match m & 7 { 1 => 3, 3 => 2, 2 => 4, 4 => 1, 5 if id == 69 => 6, 6 if id == 69 => 5, v => v },
            64 | 71 | 26 | 86 | 91 | 93 | 94 => (m & !3) | ((m + 1) & 3),
            96 => (m & !3) | [3, 2, 0, 1][usize::from(m & 3)],
            66 => [1, 0, 5, 4, 2, 3, 7, 8, 9, 6, 0, 0, 0, 0, 0, 0][usize::from(m & 15)],
            27 | 28 => (m & 8) | [1, 0, 5, 4, 2, 3, 6, 7][usize::from(m & 7)],
            63 => (m + 4) & 15,
            _ => m,
        };
    }
    m
}

/// WORLD_SURFACE_WG-like ground in the region: one above the top block that is not air, foliage or a plant.
fn region_ground(w: &Region, x: i32, z: i32) -> i32 {
    (1..128).rev().find(|&y| !matches!(w.get(x, y, z), AIR | LEAVES | LOG | TALL_GRASS | DEAD_BUSH | FLOWER_Y..=MUSH_RED | SNOW | REED)).map_or(1, |y| y + 1)
}

/// A chest's contents from a baked loot table (`LootTable`: rolls, weighted entries, count, chance).
fn fill(d: &Data, ix: usize, r: &mut JavaRandom) -> Chest {
    let mut c = Chest::default();
    for (lo, hi, es) in &d.loot[ix] {
        let rolls = i32::from(*lo) + r.next_int_bound(i32::from(*hi) - i32::from(*lo) + 1);
        let total: i32 = es.iter().map(|e| i32::from(e.0)).sum();
        for _ in 0..rolls {
            let mut k = r.next_int_bound(total.max(1));
            for e in es {
                k -= i32::from(e.0);
                if k < 0 {
                    if (r.next_int_bound(256) as u16) < e.5 {
                        let count = i32::from(e.3) + r.next_int_bound(i32::from(e.4) - i32::from(e.3) + 1);
                        c[r.next_int_bound(27) as usize] = Some(ItemStack { id: e.1, count: count as u8, damage: e.2 });
                    }
                    break;
                }
            }
        }
    }
    c
}

/// `StructureTemplate.placeInWorld` for the part of piece `p` in `cell` (x0, z0, x1, z1, inclusive): rotation, the processor list,
/// the ground for terrain-matching pieces, chests and spawners, then the foundation.
fn place_piece(d: &Data, s: &Struct, p: &Piece, w: &mut Region, cell: [i32; 4], seed: i64) {
    let t = &d.t[p.t];
    let [sx, sy, sz] = t.size;
    let procs: &[Proc] = if p.proc == 255 { &[] } else { &d.procs[usize::from(p.proc)] };
    let roof = roof_of(d, p.t);
    // the baked ground of a rigid piece (the run of grass / dirt from y = 0 up) is the terrain's job, not the template's
    let base_run = |x: i32, z: i32| -> i32 {
        let mut n = 0;
        while p.rigid && n < sy.min(3) {
            let c = t.cells[((n * sz + z) * sx + x) as usize];
            if c == SKIP || !is_ground(d.beta[usize::from(c)].0) {
                break;
            }
            n += 1;
        }
        n
    };
    let mut low = [[i32::MAX; 16]; 16]; // the lowest load-bearing block of the piece in each column of the cell
    for y in 0..sy {
        for z in 0..sz {
            for x in 0..sx {
                let s0 = t.cells[((y * sz + z) * sx + x) as usize];
                if s0 == SKIP || (p.kind == 2 && s0 == 0) {
                    continue; // not in the template / a legacy piece does not place air
                }
                let (rx, rz) = rot_xz(x, z, p.rot);
                let (wx, wz) = (p.pos[0] + rx, p.pos[2] + rz);
                if wx < cell[0] || wx > cell[2] || wz < cell[1] || wz > cell[3] {
                    continue;
                }
                if y < base_run(x, z) {
                    continue; // the plinth: the terrain stays as it is
                }
                let wy = if p.rigid { p.pos[1] + y } else { region_ground(w, wx, wz) - 1 + y }; // `GravityProcessor(WORLD_SURFACE_WG, -1)`
                if !(1..128).contains(&wy) {
                    continue; // never the bedrock layer, nor outside the world
                }
                let mut st = s0;
                for pr in procs {
                    if let Some(rules) = pr.get(&st) {
                        let mut r = JavaRandom::new(pos_seed(wx, wy, wz));
                        for rule in rules {
                            if rule.chance != 65535 && r.next_float() * 65536.0 >= f32::from(rule.chance) {
                                continue;
                            }
                            if rule.loc != 255 && w.get(wx, wy, wz) != rule.loc {
                                continue;
                            }
                            st = rule.out;
                            break;
                        }
                    }
                }
                let (id, meta) = d.beta[usize::from(st)];
                if id == AIR && roof[(z * sx + x) as usize] <= y {
                    continue; // open air: no box is cut out of a hill around the walls
                }
                w.set_meta(wx, wy, wz, id, rotate_meta(id, meta, p.rot));
                if bearing(id) {
                    let l = &mut low[(wx - cell[0]) as usize][(wz - cell[1]) as usize];
                    *l = (*l).min(wy);
                }
                if id == CHEST || id == SPAWNER {
                    let ent = t.ents.iter().find(|e| e.0 == x && e.1 == y && e.2 == z);
                    if id == CHEST {
                        let mut r = JavaRandom::new(seed ^ pos_seed(wx, wy, wz));
                        let chest = match ent {
                            Some(&(_, _, _, 0, tb)) if usize::from(tb) < d.loot.len() => fill(d, usize::from(tb), &mut r),
                            _ => Chest::default(),
                        };
                        w.loot.push(((wx, wy, wz), chest));
                    } else {
                        w.spawners.push(((wx, wy, wz), ent.map_or(1, |e| e.4 as u8)));
                    }
                }
            }
        }
    }
    // a rest of at most GAP blocks between the ground and a load-bearing block (a template that stands a block or two high): filled,
    // never through water. No foundation: the ground under the piece was levelled to it (`flatten`).
    if p.rigid && matches!(s.adapt, 1 | 2) {
        for (i, col) in low.iter().enumerate() {
            for (j, &y0) in col.iter().enumerate() {
                if y0 == i32::MAX {
                    continue;
                }
                let (wx, wz) = (cell[0] + i as i32, cell[1] + j as i32);
                let mut y = y0 - 1;
                while y > 0 && y0 - y <= GAP {
                    let b = w.get(wx, y, wz);
                    if is_solid(b) || is_liquid(b) {
                        break;
                    }
                    w.set(wx, y, wz, DIRT);
                    y -= 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::biome::Biome;

    #[test]
    fn the_baked_file_parses_and_biome_indices_match_the_tool() {
        let d = data().expect("structory.bin parses: run tools/gen_structory.py");
        assert_eq!((d.structs.len(), d.sets.len()), (15, 5));
        // `BIOMES` in gen_structory.py: Biome as u8
        assert_eq!((Biome::Rainforest as u8, Biome::Ocean as u8, Biome::Mountains as u8, Biome::AutumnalForest as u8, Biome::SnowyBirchGrove as u8), (0, 10, 12, 13, 18));
        assert!(d.structs.iter().any(|s| s.mask >> (Biome::OakWoodland as u8) & 1 == 1));
    }

    #[test]
    fn every_structure_assembles_inside_its_area_on_flat_ground() {
        let d = data().unwrap();
        for st in 0..d.structs.len() {
            let dist = d.structs[st].dist;
            for seed in 0..12 {
                let mut r = JavaRandom::new(seed * 7919 + st as i64);
                let ps = assemble(d, st, 100 + seed as i32, -40 + seed as i32, (0, 0), &mut r, &mut |_, _| 64);
                let c = centre(&ps.first().map_or([0; 6], |p| p.bx));
                for p in &ps {
                    assert!(p.bx[1] >= 0 && p.bx[4] < 128, "structure {st} piece out of the world: {:?}", p.bx);
                    assert!((p.bx[0] - c.0).abs() <= dist && (p.bx[3] - c.0).abs() <= dist + 1 && (p.bx[2] - c.1).abs() <= dist && (p.bx[5] - c.1).abs() <= dist + 1);
                }
            }
        }
    }

    #[test]
    fn baked_ground_is_found_and_the_start_is_lowered_by_it() {
        let d = data().unwrap();
        assert!((0..d.t.len()).any(|t| base_depth(d, t) > 0), "some template stands on a grass plinth");
        assert!((0..d.t.len()).all(|t| (0..=3).contains(&base_depth(d, t))));
        for st in 0..d.structs.len() {
            for seed in 0..6 {
                let mut r = JavaRandom::new(seed * 31 + st as i64);
                let ps = assemble(d, st, 50 + seed as i32, 20, (0, 0), &mut r, &mut |_, _| 64);
                if let Some(p0) = ps.first() {
                    assert_eq!(p0.pos[1], d.structs[st].y + 64 - base_depth(d, p0.t), "structure {st}");
                }
            }
        }
    }

    #[test]
    fn only_walls_and_roofs_bear_a_foundation() {
        assert!(bearing(COBBLE) && bearing(5) && bearing(LOG));
        assert!(!bearing(AIR) && !bearing(50) && !bearing(GRASS) && !bearing(DIRT) && !bearing(85));
        assert!(is_ground(GRASS) && !is_ground(COBBLE) && is_liquid(WATER) && !is_liquid(AIR));
    }

    #[test]
    fn a_rotated_block_comes_home_after_four_turns() {
        for &(id, m) in &[(53u8, 2u8), (67, 3), (65, 4), (54, 0), (61, 5), (50, 3), (69, 5), (64, 5), (64, 13), (96, 6), (26, 10), (86, 1), (66, 6), (27, 10), (63, 9), (33, 3)] {
            assert_eq!(rotate_meta(id, m, 4), m, "id {id} meta {m}");
            assert_eq!(rotate_meta(id, m, 1), rotate_meta(id, rotate_meta(id, m, 0), 1));
        }
        assert_eq!(rotate_meta(53, 0, 1), 2); // facing east, turned clockwise, faces south
        assert_eq!(rotate_meta(65, 2, 1), 5); // a ladder facing north faces east
    }

    #[test]
    fn spread_stays_in_its_region_and_the_start_is_stable() {
        let d = data().unwrap();
        for set in &d.sets {
            for (rx, rz) in [(0, 0), (-3, 7), (12, -9)] {
                let (x, z) = spread(12345, rx, rz, set);
                assert!((rx * set.spacing..(rx + 1) * set.spacing - set.sep).contains(&x) && (rz * set.spacing..(rz + 1) * set.spacing - set.sep).contains(&z));
                assert_eq!(spread(12345, rx, rz, set), (x, z));
            }
        }
    }

    /// A rigid piece box (x0, z0, x1, z1) at height 64, for `judge`.
    fn piece(b: [i32; 4], rigid: bool) -> Piece {
        Piece { t: 0, rot: 0, pos: [b[0], 64, b[1]], bx: [b[0], 64, b[1], b[2], 70, b[3]], rigid, kind: 1, proc: 255 }
    }

    #[test]
    fn the_blend_meets_the_terrain_and_never_jumps() {
        assert_eq!(blend(64, 70, 0.0), 64);
        assert_eq!(blend(64, 70, f64::from(MARGIN)), 70);
        assert_eq!(blend(64, 58, f64::from(MARGIN)), 58);
        for n in [-6, 6] {
            let hs: Vec<i32> = (0..=MARGIN).map(|k| blend(64, 64 + n, f64::from(k))).collect();
            assert!(hs.windows(2).all(|w| (w[1] - w[0]).abs() <= 1 && (w[1] - 64) * n.signum() >= (w[0] - 64) * n.signum()), "{hs:?}");
        }
    }

    #[test]
    fn a_flat_dry_site_is_accepted_and_levelled_to_its_pad() {
        let ps = [piece([0, 0, 15, 15], true), piece([16, 4, 24, 10], false)];
        let site = judge(&ps, 64, true, &mut |_, _| (64, false)).expect("flat ground");
        assert!(site.flat && site.rigid.len() == 1 && site.all.len() == 2);
        assert_eq!(site.height(8, 8, 64), Some((64, true)));
        assert_eq!(site.height(8, 8, 66), Some((64, true))); // under the pad: the target, whatever the ground was
        assert_eq!(site.height(8, 15 + PAD + MARGIN + 1, 70), None); // past the margin nothing is touched
        let (h, pad) = site.height(8, 15 + PAD + MARGIN, 70).unwrap();
        assert_eq!((h, pad), (70, false)); // the margin ends flush with the natural ground
        // a neighbour column never differs by more than the natural ground does, plus one
        let mut last = 64;
        for z in 16..=15 + PAD + MARGIN {
            let (h, _) = site.height(8, z, 70).unwrap();
            assert!((h - last).abs() <= 1, "z {z}: {last} -> {h}");
            last = h;
        }
        assert!(site.blocks(24 + 3, 7, 3) && !site.blocks(24 + 4, 7, 3)); // the veto follows every piece, rigid or not
        assert!(site.zone[0] <= -(PAD + MARGIN) && site.zone[2] >= 24 + PAD + MARGIN);
    }

    #[test]
    fn slopes_cliffs_and_water_refuse_a_site() {
        let ps = [piece([0, 0, 15, 15], true)];
        let ok = |f: &mut dyn FnMut(i32, i32) -> (i32, bool)| judge(&ps, 64, true, f).is_some();
        assert!(ok(&mut |_, _| (64, false)));
        assert!(ok(&mut |x, _| (64 + (x - 20).clamp(0, 6), false))); // a gentle rise of 6 in the margin is fine
        assert!(!ok(&mut |x, _| (64 + x.div_euclid(4), false))); // a slope under the building
        assert!(!ok(&mut |x, _| (64 + (x > 8 && x < 10) as i32 * 3, false))); // a 3 block wall under the pad
        assert!(!ok(&mut |x, _| (64 + (x > 24) as i32 * 3, false))); // a 3 block wall in the margin
        assert!(!ok(&mut |x, _| (64 + (x - 17).clamp(0, 8), false))); // a long gentle hill 8 high beside it: over SIDE
        assert!(!ok(&mut |x, z| (64, x == 15 + PAD + MARGIN + SAFE && z == 0))); // a river `SAFE` blocks past the margin
        assert!(ok(&mut |x, z| (64, x == 15 + PAD + MARGIN + SAFE + 3 && z == 0))); // further away it does not matter
        // a water structure is never levelled nor refused for its water
        let site = judge(&ps, 64, false, &mut |_, _| (40, true)).unwrap();
        assert!(!site.flat && site.blocks(0, 0, 0));
    }

    #[test]
    fn the_site_search_shifts_nearest_first_and_a_shift_only_translates_the_layout() {
        let n = shifts();
        assert_eq!((n[0], n.len()), ((0, 0), 169));
        assert!(n.windows(2).all(|w| w[0].0 * w[0].0 + w[0].1 * w[0].1 <= w[1].0 * w[1].0 + w[1].1 * w[1].1));
        // a hump 16 wide that runs along z: the nominal rectangle is refused, the first shift that clears it is accepted
        let hump = |x: i32| if (0..16).contains(&x) { 70 } else { 64 };
        let found = n.iter().find(|&&(dx, _)| judge(&[piece([dx, 0, dx + 15, 15], true)], 64, true, &mut |x, _| (hump(x), false)).is_some());
        assert!(found.is_some_and(|o| o.0.abs() >= 40), "{found:?}");
        // the same Random and ground give the same layout, only moved
        let d = data().unwrap();
        for st in (0..d.structs.len()).filter(|&st| !d.aquatic(st)) {
            let (mut r0, mut r1) = (JavaRandom::new(77 + st as i64), JavaRandom::new(77 + st as i64));
            let a = assemble(d, st, 12, 4, (0, 0), &mut r0, &mut |_, _| 64);
            let b = assemble(d, st, 12, 4, (8, -16), &mut r1, &mut |_, _| 64);
            assert_eq!(a.len(), b.len(), "structure {st}");
            for (p, q) in a.iter().zip(&b) {
                assert_eq!((q.bx[0] - p.bx[0], q.bx[2] - p.bx[2], q.bx[1] - p.bx[1], q.bx[5] - p.bx[5]), (8, -16, 0, -16), "structure {st}");
            }
        }
    }

    #[test]
    fn a_column_is_rebuilt_cut_or_filled_and_cleared_above() {
        use crate::world::chunk::Nibbles;
        let mut w = Region::new(0, 0, std::array::from_fn(|_| vec![AIR; 16 * 16 * 128]), std::array::from_fn(|_| Nibbles::new()));
        let col = |w: &mut Region, x: i32, z: i32, ground: i32| {
            for y in 1..ground - 4 {
                w.set(x, y, z, STONE);
            }
            for y in ground - 4..ground - 1 {
                w.set(x, y, z, DIRT);
            }
            w.set(x, ground - 1, z, GRASS);
            w.set(x, ground, z, TALL_GRASS);
        };
        col(&mut w, 3, 3, 70);
        for y in 71..78 {
            w.set(3, y, 3, LEAVES);
        }
        rebuild(&mut w, 3, 3, 66, false); // cut 4 down
        assert_eq!((w.get(3, 65, 3), w.get(3, 64, 3), w.get(3, 63, 3), w.get(3, 66, 3), w.get(3, 77, 3)), (GRASS, DIRT, DIRT, AIR, AIR));
        assert_eq!(region_ground(&w, 3, 3), 66);
        col(&mut w, 4, 3, 60);
        rebuild(&mut w, 4, 3, 63, false); // fill 3 up
        assert_eq!((w.get(4, 62, 3), w.get(4, 61, 3), w.get(4, 60, 3), w.get(4, 63, 3)), (GRASS, DIRT, DIRT, AIR));
        col(&mut w, 5, 3, 64);
        w.set(5, 65, 3, LEAVES);
        rebuild(&mut w, 5, 3, 64, false); // right height, not forced: left alone
        assert_eq!(w.get(5, 65, 3), LEAVES);
        rebuild(&mut w, 5, 3, 64, true); // under the pad: cleared
        assert_eq!((w.get(5, 65, 3), w.get(5, 63, 3)), (AIR, GRASS));
    }

    #[test]
    fn only_water_structures_are_aquatic() {
        let d = data().unwrap();
        let wet: Vec<usize> = (0..d.structs.len()).filter(|&st| d.aquatic(st)).collect();
        assert_eq!(wet.len(), 2, "the boats and the swamp ruin: {wet:?}");
        assert!(wet.iter().all(|&st| d.structs[st].mask != 0));
    }
}
