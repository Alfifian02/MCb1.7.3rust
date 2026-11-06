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
//! - SITE (2026-11-06, rewrites the 2026-11-05 one): a structure stands on flat, dry ground and its trees keep away, so nothing stands
//!   on a platform, a pillar or a wall of dirt and no tree is cut in two.
//!   1. The layout is assembled ONCE, on flat ground (`BASE`); the site search only moves it. `settle` tries the nominal start and then
//!      rings of 16, 32 and 48 blocks (`shifts`, nearest first, 41 places) and keeps the first whose rigid pieces pass `judge` on the
//!      coarse terrain (`Ground`: per 4 x 4 block cell of `generate_far` the lowest and highest ground and whether any column is
//!      water, ice or lava): under them and `PAD` round them within `TIERS` .0 of the start's height, out to `AREA` (`PAD + MARGIN`)
//!      within .1 of it and no cell spreading more than .2, and no water within `SAFE` more blocks (a river may run near, not touch).
//!      A second, looser tier is tried before the structure is dropped (none = no structure there: the weighted pick tries the next).
//!      The biome at the new place must still be one of the structure's; it must fit y 1 .. top - 2; and it keeps `2 * AREA` of
//!      ground and 6 blocks of pieces from every start of lower key (set, x, z) within `NEAR` chunks (the priority keeps the answer
//!      a pure function of the seed). Structures that do not stand on the ground (`Data::fixed`: the water ones, the buried ones) keep
//!      their chunk, shape nothing and only keep trees off what shows.
//!   2. The levelling (`flatten`, per cell, before any piece) is a pure function of the start and of the `Region`'s own column: the
//!      ground is the start's target height under the rigid pieces and `PAD` blocks round them and eases into the column's natural
//!      height over `MARGIN` blocks with a smoothstep (`blend`). A column is cut or filled with its own surface block and filler
//!      (snow kept). Under the pad everything above the ground is cleared; in the margin only loose plants and snow, so the canopy of
//!      a tree outside stays whole. A water column is left alone. Where two starts cover a column the first in `near()` owns it.
//!   3. Trees, lakes and dungeons of `populate` ask the starts first (`Start::allows_tree` etc.): no trunk within `AREA` of a rigid
//!      piece (the whole levelled ground) or `CROWN` of any piece that shows, no lake centre within `AREA + LAKE` or 10, no dungeon
//!      within 8. So no tree grows through a wall or is sliced by the levelling, whichever cell is populated first.
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
/// The ground level the layout is assembled at; the finished structure is moved to its real level afterwards.
const BASE: i32 = 64;
/// Full flat ground round the rigid pieces, then the ease into the natural ground over `MARGIN` blocks.
const PAD: i32 = 2;
const MARGIN: i32 = 16;
/// How far from a rigid piece the ground is touched.
const AREA: i32 = PAD + MARGIN;
/// Extra blocks beyond `AREA` that must have no water, ice or lava.
const SAFE: i32 = 4;
/// What a site may ask of the land, strict first: the largest |ground - target| under the pad, the same in the margin, and the
/// largest spread (highest - lowest) of a 4 x 4 block cell in the margin (no wall).
const TIERS: [(i32, i32, i32); 2] = [(2, 6, 4), (3, 8, 5)];
/// The farthest the site search moves a structure from its start chunk (blocks).
const SHIFT: i32 = 48;
/// The widest reach of a tree (a big tree's branches): no trunk this close to a piece that shows.
const CROWN: i32 = 10;
/// A lake is up to 16 wide: no lake centre this close to the levelled ground.
const LAKE: i32 = 8;
/// What a start asks of the land around it reaches this far from its pieces (blocks).
const ZONE: i32 = 40;
/// Chunks within which two starts can touch each other: the priority rule looks this far.
const NEAR: i32 = 20;
/// Chunks a start can reach a cell from: `max_distance_from_center` 80 + `SHIFT` 48 + `ZONE`, rounded up.
const REACH: i32 = 12;
/// Chunks of coarse terrain kept between two searches (96 bytes each).
const CELLS: usize = 1536;
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

    /// Structures that keep their chunk and leave the ground alone: the water ones and the ones sunk into the land (`bury`, or a start
    /// height of -6 or less: the swamp ruin and the buried one). `structory.bin` keeps no names; this is read from the baked numbers.
    fn fixed(&self, st: usize) -> bool {
        let s = &self.structs[st];
        self.aquatic(st) || s.adapt == 3 || s.y <= -6
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
/// the first free height there. Twin of `structory_proto.assemble`.
fn assemble(d: &Data, st: usize, cx: i32, cz: i32, rnd: &mut JavaRandom, ground: &mut dyn FnMut(i32, i32) -> i32) -> Vec<Piece> {
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
    let (bx, bz) = (cx * 16, cz * 16);
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
    let area: Bx = [g.0 - s.dist, 0, g.1 - s.dist, g.0 + s.dist, crate::world::chunk::H as i32 - 1, g.1 + s.dist];
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

/// A structure that starts in one chunk: its pieces (already at their place) and the site that place gave it.
pub struct Start {
    st: usize,
    pieces: Vec<Piece>,
    site: Site,
}

impl Start {
    /// Whether a tree may be grown with its trunk at (x, z): not on the levelled ground, not within a crown of a piece that shows.
    pub fn allows_tree(&self, x: i32, z: i32) -> bool {
        self.site.tree_ok(x, z)
    }

    /// Whether a lake may be dug round (x, z): not within reach of the levelled ground or a piece.
    pub fn allows_lake(&self, x: i32, z: i32) -> bool {
        self.site.lake_ok(x, z)
    }

    /// Whether a dungeon room may be made round (x, z): not into a piece.
    pub fn allows_dungeon(&self, x: i32, z: i32) -> bool {
        self.site.dungeon_ok(x, z)
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

/// One 4 x 4 block cell of the natural terrain (`generate_far`: no caves, no trees): the lowest and the highest first free height of
/// its 16 columns, and whether the top block of any of them is water, lava or ice (sea, river, lake).
#[derive(Clone, Copy)]
struct Cell {
    lo: u16,
    hi: u16,
    wet: bool,
}

/// The coarse natural terrain, kept per chunk (16 cells of 6 bytes), generated only where a search looks.
struct Ground<'a> {
    g: &'a mut OverworldGenerator,
    cm: &'a mut WorldChunkManager,
    h: &'a mut HashMap<(i32, i32), [Cell; 16]>,
}

impl Ground<'_> {
    /// Cell (i, j): the blocks 4i ..= 4i + 3 by 4j ..= 4j + 3.
    fn cell(&mut self, i: i32, j: i32) -> Cell {
        let key = (i >> 2, j >> 2);
        if !self.h.contains_key(&key) {
            if self.h.len() > CELLS * 2 {
                self.h.clear(); // backstop only: `Structory::settle` clears at `CELLS`, between two searches
            }
            let b = self.g.generate_far(key.0, key.1, self.cm);
            let mut cells = [Cell { lo: u16::MAX, hi: 0, wet: false }; 16];
            for x in 0..16usize {
                for z in 0..16usize {
                    let base = idx(x, 0, z);
                    let (h, wet) = match (0..crate::world::chunk::H).rev().find(|&y| b[base + y] != AIR) {
                        Some(y) => (y as u16 + 1, is_liquid(b[base + y]) || b[base + y] == ICE),
                        None => (0, false),
                    };
                    let c = &mut cells[(x / 4) * 4 + z / 4];
                    c.lo = c.lo.min(h);
                    c.hi = c.hi.max(h);
                    c.wet |= wet;
                }
            }
            self.h.insert(key, cells);
        }
        self.h[&key][((i & 3) * 4 + (j & 3)) as usize]
    }

    /// The first free height at (x, z), to within the spread of its cell.
    fn at(&mut self, x: i32, z: i32) -> i32 {
        let c = self.cell(x >> 2, z >> 2);
        (i32::from(c.lo) + i32::from(c.hi) + 1) / 2
    }
}

// ---------------------------------------------------------------- the site

/// An x / z box `[x0, z0, x1, z1]`, both ends inclusive.
type R = [i32; 4];

/// Squared distance from (x, z) to a box, 0 inside it.
fn d2(r: &R, x: i32, z: i32) -> i32 {
    let dx = (r[0] - x).max(0).max(x - r[2]);
    let dz = (r[1] - z).max(0).max(z - r[3]);
    dx * dx + dz * dz
}

/// Squared distance between two boxes, 0 where they meet.
fn rect_d2(a: &R, b: &R) -> i32 {
    let dx = (a[0] - b[2]).max(b[0] - a[2]).max(0);
    let dz = (a[1] - b[3]).max(b[1] - a[3]).max(0);
    dx * dx + dz * dz
}

/// Whether (x, z) is within `r` blocks of a box.
fn within(rs: &[R], x: i32, z: i32, r: i32) -> bool {
    rs.iter().any(|a| d2(a, x, z) <= r * r)
}

fn shift(rs: &[R], dx: i32, dz: i32) -> Vec<R> {
    rs.iter().map(|r| [r[0] + dx, r[1] + dz, r[2] + dx, r[3] + dz]).collect()
}

/// Where a start stands and what it asks of the land round it.
struct Site {
    /// Whether the ground is levelled (false for the structures that are not on the ground, `Data::fixed`).
    flat: bool,
    /// The ground height under the start piece (the median of 9 samples) that every column of the pad is brought to.
    target: i32,
    /// Every rigid piece that stands on the ground: what is levelled.
    rigid: Vec<R>,
    /// Every piece that shows above the ground (paths too): what trees keep away from.
    high: Vec<R>,
    /// Every piece: what lakes and dungeon rooms keep away from.
    deep: Vec<R>,
    /// The box a cell must meet for this start to matter to it: every piece and `ZONE` round it.
    zone: R,
}

impl Site {
    fn new(flat: bool, target: i32, rigid: Vec<R>, high: Vec<R>, deep: Vec<R>) -> Self {
        let mut zone = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
        for a in &deep {
            zone = [zone[0].min(a[0] - ZONE), zone[1].min(a[1] - ZONE), zone[2].max(a[2] + ZONE), zone[3].max(a[3] + ZONE)];
        }
        Self { flat, target, rigid, high, deep, zone }
    }

    /// Squared distance from (x, z) to the nearest rigid piece (`i32::MAX` for none).
    fn dist2(&self, x: i32, z: i32) -> i32 {
        self.rigid.iter().map(|r| d2(r, x, z)).min().unwrap_or(i32::MAX)
    }

    /// The levelled height of a column that is `q` (squared) from the rigid pieces over the natural height `n`, and whether it is under
    /// the pad (the structure itself). Past `AREA` it is `n`.
    fn height(&self, q: i32, n: i32) -> (i32, bool) {
        let k = (f64::from(q).sqrt() - f64::from(PAD)).max(0.0);
        (blend(self.target, n, k), k == 0.0)
    }

    fn tree_ok(&self, x: i32, z: i32) -> bool {
        !within(&self.rigid, x, z, AREA) && !within(&self.high, x, z, CROWN)
    }

    fn lake_ok(&self, x: i32, z: i32) -> bool {
        !within(&self.rigid, x, z, AREA + LAKE) && !within(&self.deep, x, z, 10)
    }

    fn dungeon_ok(&self, x: i32, z: i32) -> bool {
        !within(&self.deep, x, z, 8)
    }
}

/// `target` eased to the natural height `n` by the smoothstep of `k` blocks of `MARGIN` (integer maths after the one `f64` sqrt, so every
/// cell and every thread gets the same height).
fn blend(target: i32, n: i32, k: f64) -> i32 {
    let t = ((k / f64::from(MARGIN)) * 1024.0).clamp(0.0, 1024.0) as i32;
    let s = (t * t * (3072 - 2 * t)) >> 20; // 0 ..= 1024
    target + ((n - target) * s + 512).div_euclid(1024)
}

/// Whether the rigid boxes `rigid` (already moved) stand on ground that `tier` accepts, on the coarse terrain `cell`: every 4 x 4 cell
/// under the pad within `.0` of `target`, in the margin within `.1` and spreading no more than `.2`, and none within `AREA + SAFE`
/// holding water. `buf` is scratch. The pad is checked first: it is what fails most, and it asks the fewest chunks.
fn judge(rigid: &[R], target: i32, tier: (i32, i32, i32), cell: &mut dyn FnMut(i32, i32) -> Cell, buf: &mut Vec<u8>) -> bool {
    let wide = AREA + SAFE;
    let (mut x0, mut z0, mut x1, mut z1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for r in rigid {
        x0 = x0.min(r[0] - wide);
        z0 = z0.min(r[1] - wide);
        x1 = x1.max(r[2] + wide);
        z1 = z1.max(r[3] + wide);
    }
    let (i0, j0, i1, j1) = (x0 >> 2, z0 >> 2, x1 >> 2, z1 >> 2);
    let nj = (j1 - j0 + 1) as usize;
    buf.clear();
    buf.resize((i1 - i0 + 1) as usize * nj, 0);
    // class of every cell near a rigid box: 1 = pad, 2 = margin, 3 = the water margin
    for r in rigid {
        for i in (r[0] - wide) >> 2..=(r[2] + wide) >> 2 {
            for j in (r[1] - wide) >> 2..=(r[3] + wide) >> 2 {
                let q = rect_d2(&[i * 4, j * 4, i * 4 + 3, j * 4 + 3], r);
                let class = if q <= PAD * PAD {
                    1
                } else if q <= AREA * AREA {
                    2
                } else if q <= wide * wide {
                    3
                } else {
                    continue;
                };
                let k = (i - i0) as usize * nj + (j - j0) as usize;
                if buf[k] == 0 || class < buf[k] {
                    buf[k] = class;
                }
            }
        }
    }
    for class in 1..=3u8 {
        for i in i0..=i1 {
            for j in j0..=j1 {
                if buf[(i - i0) as usize * nj + (j - j0) as usize] != class {
                    continue;
                }
                let c = cell(i, j);
                let (lo, hi) = (i32::from(c.lo), i32::from(c.hi));
                let ok = !c.wet
                    && match class {
                        1 => lo >= target - tier.0 && hi <= target + tier.0,
                        2 => lo >= target - tier.1 && hi <= target + tier.1 && hi - lo <= tier.2,
                        _ => true,
                    };
                if !ok {
                    return false;
                }
            }
        }
    }
    true
}

/// cos and sin of 22.5 degree steps, times 100 (integers: every platform tries the same places).
const COS: [i32; 16] = [100, 92, 71, 38, 0, -38, -71, -92, -100, -92, -71, -38, 0, 38, 71, 92];
const SIN: [i32; 16] = [0, 38, 71, 92, 100, 92, 71, 38, 0, -38, -71, -92, -100, -92, -71, -38];

/// The places the site search tries, nearest first: the start chunk, 8 places 16 blocks away, 16 at 32 and 16 at `SHIFT`.
fn shifts() -> Vec<(i32, i32)> {
    let mut v = vec![(0, 0)];
    for (r, step) in [(16, 2usize), (32, 1), (SHIFT, 1)] {
        for k in (0..16).step_by(step) {
            v.push((r * COS[k] / 100, r * SIN[k] / 100));
        }
    }
    v
}

/// What the site search asks of the world.
trait Land {
    fn cell(&mut self, i: i32, j: i32) -> Cell;
    /// The ground level under the start piece when the layout is moved by (dx, dz): the median of 9 samples.
    fn level(&mut self, dx: i32, dz: i32) -> i32;
    /// Whether a place that passed the ground test may be used: its biome, the height of the world, the starts of lower key.
    fn allow(&mut self, dx: i32, dz: i32, level: i32) -> bool;
}

/// The first place, nearest to where the start chunk put it, where `rigid` (the boxes of the layout) stands on ground `judge` accepts,
/// strict tier first, and `land` allows: (dx, dz, level). `flat` false: only the start chunk, no ground test.
fn search(rigid: &[R], flat: bool, land: &mut dyn Land) -> Option<(i32, i32, i32)> {
    let offs = if flat { shifts() } else { vec![(0, 0)] };
    let tiers = if flat { &TIERS[..] } else { &TIERS[..1] };
    let mut buf = Vec::new();
    for &tier in tiers {
        for &(dx, dz) in &offs {
            let level = land.level(dx, dz);
            if flat && !judge(&shift(rigid, dx, dz), level, tier, &mut |i, j| land.cell(i, j), &mut buf) {
                continue;
            }
            if land.allow(dx, dz, level) {
                return Some((dx, dz, level));
            }
        }
    }
    None
}

/// `search`'s view of the world while one start is settled.
struct Siting<'a, 'b> {
    gr: Ground<'a>,
    /// The start piece's box in the layout (before it is moved).
    p0: Bx,
    pieces: &'b [Piece],
    mask: u32,
    rigid: &'b [R],
    high: &'b [R],
    flat: bool,
    others: &'b [Arc<Start>],
}

impl Land for Siting<'_, '_> {
    fn cell(&mut self, i: i32, j: i32) -> Cell {
        self.gr.cell(i, j)
    }

    fn level(&mut self, dx: i32, dz: i32) -> i32 {
        let (cx, cz) = centre(&self.p0);
        let mut hs = [0i32; 9];
        let mut n = 0;
        for &x in &[self.p0[0], cx, self.p0[3]] {
            for &z in &[self.p0[2], cz, self.p0[5]] {
                hs[n] = self.gr.at(x + dx, z + dz);
                n += 1;
            }
        }
        hs.sort_unstable();
        hs[4]
    }

    fn allow(&mut self, dx: i32, dz: i32, level: i32) -> bool {
        let dy = level - BASE;
        if self.pieces.iter().any(|p| p.bx[1] + dy < 1 || p.bx[4] + dy > crate::world::chunk::H as i32 - 2) {
            return false; // out of the world
        }
        if self.flat {
            let (x, z) = centre(&self.p0);
            let biome = self.gr.cm.load_block_generator_data(x + dx, z + dz, 1, 1)[0];
            if self.mask >> (biome as u8) & 1 == 0 {
                return false; // moved out of the biomes it can stand in
            }
        }
        // a start of lower key keeps its place: no ground in common, and the pieces 6 blocks apart
        let (rigid, high) = (shift(self.rigid, dx, dz), shift(self.high, dx, dz));
        let far = (2 * AREA) * (2 * AREA);
        self.others.iter().all(|o| {
            rigid.iter().all(|a| o.site.rigid.iter().all(|b| rect_d2(a, b) > far)) && high.iter().all(|a| o.site.high.iter().all(|b| rect_d2(a, b) > 36))
        })
    }
}

/// Things that grow on the ground and are no part of it: a margin column that changes height loses these.
fn loose(id: u8) -> bool {
    matches!(id, TALL_GRASS | DEAD_BUSH | FLOWER_Y..=MUSH_RED | SNOW | REED | CACTUS | PUMPKIN | 6)
}

/// The natural ground of a column of the region: one above the top block that is not air, a tree or something loose.
fn natural_ground(w: &Region, x: i32, z: i32) -> i32 {
    (1..w.top()).rev().find(|&y| !matches!(w.get(x, y, z), AIR | LEAVES | LOG) && !loose(w.get(x, y, z))).map_or(1, |y| y + 1)
}

/// Bring column (x, z), whose natural ground is `cur`, to `h` (one above its top block): cut or fill, with the column's own surface block
/// and filler, snow kept. Under the pad (`pad`) everything above is cleared; in the margin only loose plants and snow go, so the canopy
/// of a tree outside it stays whole. A column of water or ice is left alone.
fn rebuild(w: &mut Region, x: i32, z: i32, cur: i32, h: i32, pad: bool) {
    if cur < 2 || h < 2 || (cur == h && !pad) {
        return;
    }
    let top = w.get(x, cur - 1, z);
    if is_liquid(top) || top == ICE {
        return;
    }
    let fill = match w.get(x, cur - 2, z) {
        AIR | WATER_MOVING..=LAVA => DIRT,
        f => f,
    };
    let snow = w.get(x, cur, z) == SNOW;
    for y in h..cur {
        w.set(x, y, z, AIR);
    }
    let from = h.max(cur);
    if pad {
        for y in from..w.top() {
            if w.get(x, y, z) != AIR {
                w.set(x, y, z, AIR);
            }
        }
    } else {
        let mut y = from;
        while y < w.top() && loose(w.get(x, y, z)) {
            w.set(x, y, z, AIR);
            y += 1;
        }
    }
    if h == cur {
        return;
    }
    let from = if h > cur { cur - 1 } else { h - 3 };
    for y in from.max(1)..h - 1 {
        w.set(x, y, z, fill);
    }
    w.set(x, h - 1, z, top);
    if snow && h < w.top() && is_solid(top) {
        w.set(x, h, z, SNOW);
    }
}

/// Level the columns of `cell` that `s` owns: the pad and its margin. A column already taken by an earlier start of this cell is left.
fn flatten(s: &Start, w: &mut Region, cell: [i32; 4], claimed: &mut [[bool; 16]; 16]) {
    let z0 = s.site.zone;
    if !s.site.flat || z0[0] > cell[2] || z0[2] < cell[0] || z0[1] > cell[3] || z0[3] < cell[1] {
        return;
    }
    for x in cell[0]..=cell[2] {
        for z in cell[1]..=cell[3] {
            let c = &mut claimed[(x - cell[0]) as usize][(z - cell[1]) as usize];
            let q = s.site.dist2(x, z);
            if *c || q > AREA * AREA {
                continue;
            }
            *c = true;
            let cur = natural_ground(w, x, z);
            let (h, pad) = s.site.height(q, cur);
            rebuild(w, x, z, cur, h, pad);
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

/// The per-generator state of the structures: the starts already settled and the coarse terrain their searches read.
pub struct Structory {
    seed: i64,
    starts: HashMap<(usize, i32, i32), Option<Arc<Start>>>,
    heights: HashMap<(i32, i32), [Cell; 16]>,
}

impl Structory {
    /// `None` when `structory.bin` is stale or broken (the Blooming world then simply has no structures).
    pub fn new(seed: i64) -> Option<Self> {
        data().map(|_| Self { seed, starts: HashMap::new(), heights: HashMap::new() })
    }

    /// The structure that starts in chunk (sx, sz) of set `si`, if there is one (`ChunkGenerator.tryGenerateStructures`). A pure function
    /// of (seed, si, sx, sz): the same from every worker and every side.
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
            let mut lower: Option<Vec<Arc<Start>>> = None; // the starts of lower key near this one, asked for once
            // the weighted pick; a structure that cannot start here (its biome, no piece, no place) is dropped and the next one tried
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
                // the layout does not depend on the ground: assembled once on flat ground, then moved to its place
                let layout = assemble(d, st, sx, sz, &mut r, &mut |_, _| BASE);
                if layout.is_empty() {
                    continue;
                }
                if lower.is_none() {
                    lower = Some(self.neighbours(d, g, cm, si, sx, sz));
                }
                if let Some(start) = self.settle(d, g, cm, st, layout, lower.as_deref().unwrap_or(&[])) {
                    made = Some(Arc::new(start));
                    break;
                }
            }
        }
        self.starts.insert((si, sx, sz), made.clone());
        made
    }

    /// The starts that rank before (si, sx, sz) (lower set, then chunk x, then chunk z) within `NEAR` chunks. Only they are asked about,
    /// so the recursion ends and every start gets the same answer whichever is looked at first.
    fn neighbours(&mut self, d: &'static Data, g: &mut OverworldGenerator, cm: &mut WorldChunkManager, si: usize, sx: i32, sz: i32) -> Vec<Arc<Start>> {
        let mut out = Vec::new();
        for (s2, set) in d.sets.iter().enumerate() {
            let (rx0, rx1) = ((sx - NEAR).div_euclid(set.spacing), (sx + NEAR).div_euclid(set.spacing));
            let (rz0, rz1) = ((sz - NEAR).div_euclid(set.spacing), (sz + NEAR).div_euclid(set.spacing));
            for rx in rx0..=rx1 {
                for rz in rz0..=rz1 {
                    let (x2, z2) = spread(self.seed, rx, rz, set);
                    if (s2, x2, z2) >= (si, sx, sz) || (x2 - sx).abs() > NEAR || (z2 - sz).abs() > NEAR {
                        continue;
                    }
                    if let Some(o) = self.start(d, g, cm, s2, x2, z2) {
                        out.push(o);
                    }
                }
            }
        }
        out
    }

    /// Give the layout `pieces` (assembled on flat ground at its start chunk) its place (`search`) and move it there. None when the land
    /// has no place for it or a start of lower key has it.
    fn settle(&mut self, d: &'static Data, g: &mut OverworldGenerator, cm: &mut WorldChunkManager, st: usize, mut pieces: Vec<Piece>, lower: &[Arc<Start>]) -> Option<Start> {
        if self.heights.len() > CELLS {
            self.heights.clear(); // between two searches only, never in the middle of one
        }
        let rect = |p: &Piece| [p.bx[0], p.bx[2], p.bx[3], p.bx[5]];
        let shows = |p: &&Piece| p.bx[4] >= BASE - 3; // the layout stands on ground level `BASE`
        let rigid: Vec<R> = if d.fixed(st) { Vec::new() } else { pieces.iter().filter(shows).filter(|p| p.rigid).map(rect).collect() };
        let high: Vec<R> = pieces.iter().filter(shows).map(rect).collect();
        let deep: Vec<R> = pieces.iter().map(rect).collect();
        let flat = !rigid.is_empty();
        let (dx, dz, target) = {
            let gr = Ground { g: &mut *g, cm: &mut *cm, h: &mut self.heights };
            let mut sit = Siting { gr, p0: pieces[0].bx, pieces: &pieces, mask: d.structs[st].mask, rigid: &rigid, high: &high, flat, others: lower };
            search(&rigid, flat, &mut sit)?
        };
        let dy = target - BASE;
        for p in &mut pieces {
            p.pos = [p.pos[0] + dx, p.pos[1] + dy, p.pos[2] + dz];
            p.bx = [p.bx[0] + dx, p.bx[1] + dy, p.bx[2] + dz, p.bx[3] + dx, p.bx[4] + dy, p.bx[5] + dz];
        }
        let site = Site::new(flat, target, shift(&rigid, dx, dz), shift(&high, dx, dz), shift(&deep, dx, dz));
        Some(Start { st, pieces, site })
    }

    /// The structures whose levelled ground, veto or pieces can touch the cell of chunk (cx, cz), in a fixed order (set, region x, region z).
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
                    if (sx - cx).abs() > REACH + 1 || (sz - cz).abs() > REACH + 1 {
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
    pub fn populate(&mut self, w: &mut Region, cx: i32, cz: i32, near: &[Arc<Start>]) {
        let Some(d) = data() else { return };
        let cell = [cx * 16 + 8, cz * 16 + 8, cx * 16 + 23, cz * 16 + 23];
        if near.iter().any(|s| s.site.flat) {
            let mut claimed = [[false; 16]; 16];
            for s in near {
                flatten(s, w, cell, &mut claimed);
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
    (1..w.top()).rev().find(|&y| !matches!(w.get(x, y, z), AIR | LEAVES | LOG | TALL_GRASS | DEAD_BUSH | FLOWER_Y..=MUSH_RED | SNOW | REED)).map_or(1, |y| y + 1)
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
                if !(1..w.top()).contains(&wy) {
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
                let ps = assemble(d, st, 100 + seed as i32, -40 + seed as i32, &mut r, &mut |_, _| 64);
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
                let ps = assemble(d, st, 50 + seed as i32, 20, &mut r, &mut |_, _| 64);
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

    /// A flat made-up land: every cell between `lo` and `hi`, dry.
    fn dry(lo: u16, hi: u16) -> impl FnMut(i32, i32) -> Cell {
        move |_, _| Cell { lo, hi, wet: false }
    }

    /// The land `h(x)` (the same along z) seen as cells.
    fn ramp(h: impl Fn(i32) -> i32) -> impl FnMut(i32, i32) -> Cell {
        move |i, _| {
            let hs: Vec<i32> = (0..4).map(|k| h(i * 4 + k)).collect();
            Cell { lo: *hs.iter().min().unwrap() as u16, hi: *hs.iter().max().unwrap() as u16, wet: false }
        }
    }

    const STRICT: (i32, i32, i32) = TIERS[0];

    fn accepts(rigid: &[R], mut land: impl FnMut(i32, i32) -> Cell) -> bool {
        judge(rigid, 64, STRICT, &mut land, &mut Vec::new())
    }

    #[test]
    fn the_blend_meets_the_terrain_and_never_jumps() {
        assert_eq!(blend(64, 70, 0.0), 64);
        assert_eq!(blend(64, 70, f64::from(MARGIN)), 70);
        assert_eq!(blend(64, 58, f64::from(MARGIN)), 58);
        for n in [-6, 6, 8] {
            let hs: Vec<i32> = (0..=MARGIN).map(|k| blend(64, 64 + n, f64::from(k))).collect();
            assert!(hs.windows(2).all(|w| (w[1] - w[0]).abs() <= 1 && (w[1] - 64) * n.signum() >= (w[0] - 64) * n.signum()), "{hs:?}");
        }
    }

    #[test]
    fn a_level_dry_place_is_accepted_and_slopes_cliffs_and_water_refuse_one() {
        let b = [[0, 0, 15, 15]];
        assert!(accepts(&b, dry(64, 64)));
        assert!(accepts(&b, dry(63, 65))); // a little unevenness is within the pad's 2
        assert!(accepts(&b, ramp(|x| 64 + (x - 20).clamp(0, 6)))); // a gentle rise of 6 in the margin
        assert!(!accepts(&b, ramp(|x| 64 + x.div_euclid(4)))); // a slope under the building
        assert!(!accepts(&b, ramp(|x| 64 + 3 * i32::from((9..=10).contains(&x))))); // a wall 3 high under the pad
        assert!(!accepts(&b, ramp(|x| 64 + 5 * i32::from(x > 24)))); // a wall of 5 in the margin
        assert!(!accepts(&b, ramp(|x| 64 + (x - 17).clamp(0, 8)))); // a long hill 8 high beside it
        let river = |i: i32| move |ci: i32, _: i32| Cell { lo: 64, hi: 64, wet: ci == i };
        assert!(!accepts(&b, river((15 + AREA + SAFE - 1) >> 2))); // water at the far end of the water margin
        assert!(accepts(&b, river((15 + AREA + SAFE + 4) >> 2))); // one cell beyond: it does not matter
        assert!(!judge(&b, 64, STRICT, &mut |_, _| Cell { lo: 64, hi: 64, wet: true }, &mut Vec::new())); // a lake under it
    }

    #[test]
    fn the_looser_tier_takes_what_the_strict_one_refuses() {
        let b = [[0, 0, 15, 15]];
        let mut land = ramp(|x| 64 + (x - 17).clamp(0, 7));
        assert!(!judge(&b, 64, TIERS[0], &mut land, &mut Vec::new()));
        assert!(judge(&b, 64, TIERS[1], &mut land, &mut Vec::new()));
    }

    #[test]
    fn the_places_tried_are_nearest_first_and_centred_on_the_start() {
        let n = shifts();
        assert_eq!((n[0], n.len()), ((0, 0), 41));
        let r2 = |o: &(i32, i32)| o.0 * o.0 + o.1 * o.1;
        let rings = [&n[1..9], &n[9..25], &n[25..41]];
        let mut last = 0;
        for ring in rings {
            assert!(ring.iter().map(r2).min().unwrap() > last, "a ring starts inside the one before");
            last = ring.iter().map(r2).max().unwrap();
        }
        assert!(n.iter().all(|o| o.0.abs() <= SHIFT && o.1.abs() <= SHIFT));
    }

    /// `search` on a made-up land: `cell` as cells, level 64 everywhere, `ok` the extra permission.
    struct Mock<F: FnMut(i32, i32) -> Cell, A: FnMut(i32, i32) -> bool>(F, A);

    impl<F: FnMut(i32, i32) -> Cell, A: FnMut(i32, i32) -> bool> Land for Mock<F, A> {
        fn cell(&mut self, i: i32, j: i32) -> Cell {
            (self.0)(i, j)
        }
        fn level(&mut self, _: i32, _: i32) -> i32 {
            64
        }
        fn allow(&mut self, dx: i32, dz: i32, _: i32) -> bool {
            (self.1)(dx, dz)
        }
    }

    #[test]
    fn the_search_steps_over_a_hump_and_over_a_refusal_and_gives_up_on_broken_land() {
        let b = [[0, 0, 15, 15]];
        // a hump 16 wide along z (heights of cells whole: it starts and ends on a cell edge): the start chunk and the near rings are refused
        let mut m = Mock(ramp(|x| if (0..16).contains(&x) { 72 } else { 64 }), |_, _| true);
        let (dx, dz, level) = search(&b, true, &mut m).expect("flat ground lies beside the hump");
        assert!(dx.abs() >= 32 && level == 64, "({dx}, {dz})");
        // a place that is flat but not allowed (its biome, another structure) is skipped
        let mut m = Mock(dry(64, 64), |dx, dz| (dx, dz) != (0, 0));
        let (dx, dz, _) = search(&b, true, &mut m).unwrap();
        assert_eq!((dx * dx + dz * dz > 0, dx.abs().max(dz.abs()) <= 16), (true, true));
        // land that is broken everywhere has no place: no structure
        let mut m = Mock(|i, j| Cell { lo: 60, hi: 75, wet: (i + j) % 7 == 0 }, |_, _| true);
        assert_eq!(search(&b, true, &mut m), None);
        // a structure that is not on the ground is never moved nor judged
        let mut m = Mock(|_, _| Cell { lo: 0, hi: 300, wet: true }, |_, _| true);
        assert_eq!(search(&b, false, &mut m), Some((0, 0, 64)));
    }

    #[test]
    fn trees_lakes_and_dungeons_keep_off_the_levelled_ground_and_the_pieces() {
        let site = Site::new(true, 64, vec![[0, 0, 15, 15]], vec![[0, 0, 15, 15], [16, 4, 24, 10]], vec![[0, 0, 15, 15], [16, 4, 24, 10]]);
        assert!(!site.tree_ok(8, 8) && !site.tree_ok(15 + AREA, 8), "no trunk on the building nor in its levelled ground");
        assert!(site.tree_ok(15 + AREA + 1, 8 + 30), "past the area");
        assert!(!site.tree_ok(24 + CROWN, 7) && site.tree_ok(24 + CROWN + 1, 7 + 30), "a crown's reach from the path");
        assert!(!site.lake_ok(15 + AREA + LAKE, 8) && site.lake_ok(15 + AREA + LAKE + 1, 8 + 30));
        assert!(!site.dungeon_ok(15 + 8, 8) && site.dungeon_ok(24 + 9, 7));
        // what is not on the ground levels nothing and vetoes only what shows
        let hidden = Site::new(false, 64, Vec::new(), Vec::new(), vec![[0, 0, 15, 15]]);
        assert!(hidden.tree_ok(8, 8) && hidden.lake_ok(30, 8) && !hidden.dungeon_ok(8, 8) && hidden.dist2(8, 8) == i32::MAX);
    }

    fn column(w: &mut Region, x: i32, z: i32, ground: i32) {
        for y in 1..ground - 4 {
            w.set(x, y, z, STONE);
        }
        for y in ground - 4..ground - 1 {
            w.set(x, y, z, DIRT);
        }
        w.set(x, ground - 1, z, GRASS);
        w.set(x, ground, z, TALL_GRASS);
    }

    fn region() -> Region {
        use crate::world::chunk::Nibbles;
        Region::new(0, 0, std::array::from_fn(|_| vec![AIR; crate::world::chunk::VOLUME]), std::array::from_fn(|_| Nibbles::new()))
    }

    #[test]
    fn a_column_under_the_pad_is_cut_or_filled_and_cleared_above() {
        let mut w = region();
        column(&mut w, 3, 3, 70);
        for y in 71..78 {
            w.set(3, y, 3, LEAVES);
        }
        assert_eq!(natural_ground(&w, 3, 3), 70, "leaves and grass are not the ground");
        rebuild(&mut w, 3, 3, 70, 66, true); // cut 4 down
        assert_eq!((w.get(3, 65, 3), w.get(3, 64, 3), w.get(3, 63, 3), w.get(3, 66, 3), w.get(3, 77, 3)), (GRASS, DIRT, DIRT, AIR, AIR));
        column(&mut w, 4, 3, 60);
        rebuild(&mut w, 4, 3, 60, 63, true); // fill 3 up
        assert_eq!((w.get(4, 62, 3), w.get(4, 61, 3), w.get(4, 60, 3), w.get(4, 63, 3)), (GRASS, DIRT, DIRT, AIR));
        column(&mut w, 5, 3, 64);
        w.set(5, 65, 3, LEAVES);
        rebuild(&mut w, 5, 3, 64, 64, false); // right height in the margin: left alone
        assert_eq!(w.get(5, 65, 3), LEAVES);
        rebuild(&mut w, 5, 3, 64, 64, true); // under the pad: cleared
        assert_eq!((w.get(5, 65, 3), w.get(5, 63, 3)), (AIR, GRASS));
    }

    #[test]
    fn a_margin_column_keeps_the_canopy_of_a_tree_outside_and_loses_its_loose_plants() {
        let mut w = region();
        column(&mut w, 3, 3, 70);
        for y in 73..78 {
            w.set(3, y, 3, LEAVES); // a canopy that hangs over the margin
        }
        w.set(3, 80, 3, LOG);
        rebuild(&mut w, 3, 3, 70, 68, false); // cut 2 down
        assert_eq!((w.get(3, 67, 3), w.get(3, 68, 3), w.get(3, 70, 3), w.get(3, 73, 3), w.get(3, 77, 3), w.get(3, 80, 3)), (GRASS, AIR, AIR, LEAVES, LEAVES, LOG));
        column(&mut w, 4, 3, 70);
        w.set(4, 73, 3, LEAVES);
        rebuild(&mut w, 4, 3, 70, 72, false); // fill 2 up
        assert_eq!((w.get(4, 71, 3), w.get(4, 72, 3), w.get(4, 73, 3)), (GRASS, AIR, LEAVES));
        // snow stays on top of what it covered
        column(&mut w, 5, 3, 70);
        w.set(5, 70, 3, SNOW);
        rebuild(&mut w, 5, 3, 70, 68, false);
        assert_eq!((w.get(5, 67, 3), w.get(5, 68, 3), w.get(5, 70, 3)), (GRASS, SNOW, AIR));
    }

    #[test]
    fn water_is_not_ours_to_move() {
        let mut w = region();
        for y in 1..62 {
            w.set(3, y, 3, STONE);
        }
        for y in 62..64 {
            w.set(3, y, 3, WATER);
        }
        rebuild(&mut w, 3, 3, 64, 60, true);
        assert_eq!((w.get(3, 63, 3), w.get(3, 62, 3)), (WATER, WATER));
    }

    #[test]
    fn flatten_levels_the_pad_and_eases_the_margin_without_a_step() {
        let mut w = region();
        for x in 8..24 {
            for z in 8..24 {
                column(&mut w, x, z, 66);
            }
        }
        // one rigid box [8, 8, 12, 12] levelled to 64: a pad 2 round it, then 16 blocks of margin
        let s = Start { st: 0, pieces: Vec::new(), site: Site::new(true, 64, vec![[8, 8, 12, 12]], vec![[8, 8, 12, 12]], vec![[8, 8, 12, 12]]) };
        let mut claimed = [[false; 16]; 16];
        flatten(&s, &mut w, [8, 8, 23, 23], &mut claimed);
        let ground = |w: &Region, x: i32| natural_ground(w, x, 10);
        let hs: Vec<i32> = (12..24).map(|x| ground(&w, x)).collect();
        assert_eq!(hs[0], 64, "under the pad");
        assert!(hs.windows(2).all(|p| (p[1] - p[0]).abs() <= 1 && p[1] >= p[0]), "a wall or a pit in {hs:?}");
        assert!(*hs.last().unwrap() <= 66);
        assert!(claimed[0][0] && claimed[15][15] == (s.site.dist2(23, 23) <= AREA * AREA));
    }

    #[test]
    fn only_what_is_not_on_the_ground_is_fixed() {
        let d = data().unwrap();
        let fixed: Vec<usize> = (0..d.structs.len()).filter(|&st| d.fixed(st)).collect();
        let wet: Vec<usize> = (0..d.structs.len()).filter(|&st| d.aquatic(st)).collect();
        assert_eq!(wet.len(), 2, "the boats and the swamp ruin: {wet:?}");
        assert!(wet.iter().all(|st| fixed.contains(st)));
        assert!(fixed.iter().all(|&st| d.aquatic(st) || d.structs[st].adapt == 3 || d.structs[st].y <= -6));
        assert!(fixed.len() < d.structs.len() / 2, "most structures stand on the ground: {fixed:?}");
    }
}
