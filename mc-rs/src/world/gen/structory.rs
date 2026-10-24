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
//! - `terrain_adaptation`: `beard_thin` / `beard_box` = a dirt foundation under a rigid piece (up to 12 blocks), no terrain is cut away;
//!   `bury` and none leave the terrain alone. Terrain-matching pieces (paths) follow the ground column by column like `GravityProcessor`.
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
    pos[1] = s.y + ground(g.0, g.1);
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
}

/// `RandomSpreadStructurePlacement.getPotentialStructureChunk` of the region (rx, rz).
fn spread(seed: i64, rx: i32, rz: i32, s: &Set) -> (i32, i32) {
    let mut r = JavaRandom::new(large_feature(seed, rx, rz, s.salt));
    let k = s.spacing - s.sep;
    let mut one = |r: &mut JavaRandom| if s.tri { (r.next_int_bound(k) + r.next_int_bound(k)) / 2 } else { r.next_int_bound(k) };
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

/// The ground under a column of the terrain (`generate_far`, so no caves), first free height, kept per chunk.
struct Ground<'a> {
    g: &'a mut OverworldGenerator,
    cm: &'a mut WorldChunkManager,
    h: &'a mut HashMap<(i32, i32), Vec<u8>>,
}

impl Ground<'_> {
    fn at(&mut self, x: i32, z: i32) -> i32 {
        let key = (x >> 4, z >> 4);
        if !self.h.contains_key(&key) {
            if self.h.len() > 96 {
                self.h.clear(); // ponytail: dropped whole past 96 chunks (about 25 KB); upgrade is an LRU
            }
            let b = self.g.generate_far(key.0, key.1, self.cm);
            let col = (0..256usize)
                .map(|i| {
                    let base = idx(i / 16, 0, i % 16);
                    (0..128).rev().find(|&y| b[base + y] != AIR).map_or(0, |y| y + 1) as u8
                })
                .collect();
            self.h.insert(key, col);
        }
        i32::from(self.h[&key][((x & 15) * 16 + (z & 15)) as usize])
    }
}

/// The per-generator state of the structures: the starts already assembled and the heights they needed.
pub struct Structory {
    seed: i64,
    starts: HashMap<(usize, i32, i32), Option<Arc<Start>>>,
    heights: HashMap<(i32, i32), Vec<u8>>,
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
                let pieces = {
                    let mut gr = Ground { g: &mut *g, cm: &mut *cm, h: &mut self.heights };
                    assemble(d, st, sx, sz, &mut r, &mut |x, z| gr.at(x, z))
                };
                if !pieces.is_empty() {
                    made = Some(Arc::new(Start { st, pieces }));
                    break;
                }
            }
        }
        self.starts.insert((si, sx, sz), made.clone());
        made
    }

    /// Write what the structures starting near chunk (cx, cz) put in its cell. Called at the end of `populate`, after the features.
    pub fn populate(&mut self, g: &mut OverworldGenerator, w: &mut Region, cx: i32, cz: i32, cm: &mut WorldChunkManager) {
        let Some(d) = data() else { return };
        let cell = [cx * 16 + 8, cz * 16 + 8, cx * 16 + 23, cz * 16 + 23];
        const REACH: i32 = 6; // chunks: max_distance_from_center (80) + the cell
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
                    let s = &d.structs[start.st];
                    for p in &start.pieces {
                        if p.bx[0] <= cell[2] && p.bx[3] >= cell[0] && p.bx[2] <= cell[3] && p.bx[5] >= cell[1] {
                            place_piece(d, s, p, w, cell, self.seed);
                        }
                    }
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
    let mut low = [[i32::MAX; 16]; 16]; // the lowest solid block of the piece in each column of the cell
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
                w.set_meta(wx, wy, wz, id, rotate_meta(id, meta, p.rot));
                if is_solid(id) {
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
    // beard_thin / beard_box: the terrain climbs up to a rigid piece
    if p.rigid && (s.adapt == 1 || s.adapt == 2) {
        for (i, col) in low.iter().enumerate() {
            for (j, &y0) in col.iter().enumerate() {
                if y0 == i32::MAX {
                    continue;
                }
                let (wx, wz) = (cell[0] + i as i32, cell[1] + j as i32);
                let mut y = y0 - 1;
                while y > 0 && y0 - y <= 12 && !is_solid(w.get(wx, y, wz)) {
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
}
