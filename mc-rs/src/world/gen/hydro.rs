//! Rivers, streams and lakes for every world type: a port of the Fabric mod "Streams Reflowing" 2.15.2 (nicejohn).
//!
//! The mod was read from its class files (no source was available); every number marked `mod:` below was read from the bytecode and
//! the plate lattice and hashing were checked bit for bit against the mod's own classes. The values are the ones the game really runs
//! with (`StreamsSettings` defaults, `QualityPreset.MEDIUM`, `WatershedSize.DEFAULT`, `StreamsWorldgen.engine`), not the core's
//! `StreamsConfig.defaults()`. How it works, as in the mod:
//!
//! * **Plates** (`core.plate`): the world is cut into jittered Voronoi cells of 768 blocks (`PlateLattice`, jitter 0.45). Each plate
//!   gets one river network, built once and cached.
//! * **Grid** (`core.grid.LowResGrid`): the terrain is read every 16 blocks over the plate and a margin of `HALO` cells. The generator
//!   reads it itself (`OverworldGenerator::probe`: the vanilla density column or `Tectonic::surface`), so Vanilla and Tectonic worlds
//!   use this same code.
//! * **Routing** (`NetworkTracer.build`): priority flood from the sea (depressions are filled, `+0.001` a step), D8 receivers on the
//!   filled surface, flow accumulation (0.8 per cell), a cell is a stream from `max(1, 6 * 0.75)` = 4.5, dry headwaters are dropped
//!   with the odds `clamp((wetness - 0.2) / 0.5, 0.1, 1)` and everything downstream of a kept headwater stays, two stream systems that
//!   touch yield to the bigger one (separation), basins of at least 4 cells and 1.5 blocks deep become lakes.
//! * **Every river reaches the sea.** A river that leaves the window is traced on over the real terrain (best-first, lowest pass first)
//!   until it finds the sea; one that finds none is culled with everything upstream (the mod's "4b5 CULL: drainage reaches no water").
//!   A lake is not an end: it spills over its lowest rim and the stream goes on from there.
//! * **Shape** (`NetworkTracer`): chains run from a headwater or confluence to the next confluence or the sea; the grade never rises
//!   downstream; Chaikin corner cutting x3, then the mod's meander (three sines, phases from the hash, amplitude `16 * 1.2`, tapered at
//!   both ends, damped on steep ground).
//! * **Width** (`NetworkTracer.a(.., HydraulicGeometry, ..)`, every slider 0.5 by default, so on): the half width is
//!   `min(16, 0.4 + sqrt(acc * 256 / 2500))`, then follows the slope (steeper: narrower), the climate, a slow random variation, braiding
//!   on flat loose ground and delta mouths over the last 64 blocks into the sea, kept inside the base width's inset class.
//! * **Column plan** (`RiverEngine.plan`): a column is in the channel when its distance to the centre line is under the half width,
//!   in the bank up to `max(5, 2 * width)` further; water surface `max(sea - 1, floor(grade) - inset)`, depth `max(1, round(2 * width * 0.24))`.
//! * **Current** (`FlowSpeed`): `clamp((slope / (1/7))^(1/3) * clamp(base width / width, 0.6, 1.8), 0.25, 2)`, times a depth factor.
//!
//! UNVERIFIED (not in the bytecode I traced, or chosen here): the carve's block rules and bank profile (the mod's `RiverCarverFeature`
//! is 6700 instructions of `place` plus bank templates; only its plan, heights and materials by biome are followed), the lake level
//! (`spill - inset`), the mod's `cohesion` (taken as `0.4 + 0.6 * wetness`), how a plate's margin meets its neighbour (the owner's
//! network wins and a neighbour's fills the gaps), the plate seed, the push strength of the current, and how any of it plays on a device.
//! NOT PORTED: the stage that drops chains shorter than `minStreamLength` 75 and `connectNetwork` (plate borders), lakes' 3-block masks,
//! waterfalls, the bed's channel bars and bend asymmetry in the carve (the width side of the hydraulics is ported), bank templates and
//! features, plants, the fluid block and its flow rendering, drifting items, particles, sound (mc-rs has no audio), disk cache of
//! networks, village bridges and structure promises (b1.7.3 has neither). The Python reference is `tools/hydro_proto.py` (without the
//! hydraulic widths).

use crate::world::biome::Biome;
use crate::world::gen::overworld::{block, SEA_LEVEL};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::sync::{Arc, Mutex, OnceLock};

/// Blocks between grid nodes (mod: `QualityPreset.MEDIUM`, the default, `gridSpacing` 16). A node is at world (16 gx, 16 gz); the generator's `probe` reads it there.
pub const CELL: i32 = 16;
/// mod: `WatershedSize.DEFAULT` = BLOCKS_768, `plateJitter` 0.45.
const PLATE: f64 = 768.0;
const JITTER: f64 = 0.45;
/// Cells of margin round a plate's own cell that are routed too, so a river runs on into the next plate for a while.
const HALO: i32 = 8;
/// mod, as `StreamsWorldgen.engine` sets them: `minRiverAccumulation` 6, `wetnessMultiplier` 1 x 0.8, `growDensity` 0.75, `baseWidth` 0.4, `maxWidth` 16, `widthScale` 1, `streamDepth` 0.24, `bankWidth` 2,
/// `ditchInset` 3, `aRiver` 1, `smoothingPasses` 3, `meanderStrength` 1.2, wetness cutoff / reference 0.2 / 0.7 and floor 0.1 (`StreamsSettings`).
/// ponytail: the mod's `minRiverAccumulation` is 6; here 20, a stream needs 3.3x the catchment, so there are fewer small streams and the
/// rivers lie about 1.8x farther apart (spacing goes with the square root of the catchment). Back to 6.0 for the mod's density.
const MIN_ACC: f64 = 20.0;
const WETNESS_MULT: f64 = 0.8;
const GROW_DENSITY: f64 = 0.75;
const BASE_W: f64 = 0.4;
const MAX_W: f64 = 16.0;
const WIDTH_SCALE: f64 = 1.0;
const STREAM_DEPTH: f64 = 0.24;
const BANK_WIDTH: f64 = 2.0;
/// Farthest a bank is cut from the channel's edge (blocks): the slope has to reach the real terrain, or a trench is left with a wall at
/// the end of the band (a 5 block band beside 20 blocks of hill was a 20 block cliff), and the slope's steepness (blocks up per block out).
const VALLEY: f64 = 48.0;
const BANK_SLOPE: f64 = 0.5;
const DITCH_INSET: f64 = 3.0;
const A_RIVER: f64 = 1.0;
const SMOOTHING: usize = 3;
const MEANDER: f64 = 1.2;
const WET_CUT: f64 = 0.2;
const WET_REF: f64 = 0.7;
const WET_FLOOR: f64 = 0.1;
/// mod: `StreamsSettings` default 0.5 for every hydraulic-geometry slider (width follows slope / climate / variation, channel bars,
/// delta mouths); `HydraulicGeometry.OFF` is only the core's own default, not the game's.
const HG_SLOPE: f64 = 0.5;
const HG_CLIMATE: f64 = 0.5;
const HG_VARIATION: f64 = 0.5;
const HG_DELTA: f64 = 0.5;
/// mod: `lakeMinArea` 1024 blocks^2 = 4 cells of 256; a basin's mean depth must reach 1.5 (its "film" gate).
const LAKE_MIN_CELLS: usize = 4;
const LAKE_MIN_DEPTH: f64 = 1.5;
/// Cells a river may be traced over the real terrain, past the window, to find the sea.
const TAIL_CAP: usize = 6000;
/// Blocks a second per unit of the mod's flow speed (0.25..2): UNVERIFIED, the mod pushes by its settings.
const PUSH: f64 = 1.5;
const BUCKET: i32 = 16;
const MAX_PLATES: usize = 12;
const NONE: usize = usize::MAX;
const NB: [(i32, i32); 8] = [(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1)];

// ---- hashing and plates: bit for bit the mod's `core.math.Hashing` and `core.plate.PlateLattice` --------------------------------

const K1: u64 = 0x9E37_79B9_7F4A_7C15;
const K2: u64 = 0xD1B5_4A32_D192_ED03;
const K3: u64 = 0xA076_1D64_78BD_642F;
const K4: u64 = 0xE703_7ED1_A0B4_28DB;

fn mix(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

fn hash3(a: i64, b: i64, c: i64) -> u64 {
    let mut h = (a as u64).wrapping_mul(K1);
    h = mix(h ^ (b as u64).wrapping_mul(K2));
    h = mix(h ^ (c as u64).wrapping_mul(K3));
    mix(h)
}

fn hash4(a: i64, b: i64, c: i64, d: i64) -> u64 {
    mix(hash3(a, b, c) ^ (d as u64).wrapping_mul(K4))
}

fn to_unit(h: u64) -> f64 {
    (h >> 11) as f64 * 1.110_223_024_625_156_5e-16
}

fn unit3(a: i64, b: i64, c: i64) -> f64 {
    to_unit(hash3(a, b, c))
}

fn unit4(a: i64, b: i64, c: i64, d: i64) -> f64 {
    to_unit(hash4(a, b, c, d))
}

/// The seed point of lattice cell (i, j) (`PlateLattice.seedX` / `seedZ`).
fn plate_seed_point(seed: i64, i: i32, j: i32) -> (f64, f64) {
    let c = JITTER.max(0.0).min(0.49);
    let (i, j) = (i as i64, j as i64);
    (
        ((i as f64 + 0.5) + (unit4(seed, i, j, 1) - 0.5) * 2.0 * c) * PLATE,
        ((j as f64 + 0.5) + (unit4(seed, i, j, 2) - 0.5) * 2.0 * c) * PLATE,
    )
}

/// The plate owning a point: the nearest seed of the 3 x 3 lattice cells round it (`PlateLattice.plateAt`).
pub fn plate_at(seed: i64, x: f64, z: f64) -> (i32, i32) {
    let (ci, cj) = ((x / PLATE).floor() as i32, (z / PLATE).floor() as i32);
    let mut best = f64::MAX;
    let mut key = (ci, cj);
    for dj in -1..=1 {
        for di in -1..=1 {
            let (i, j) = (ci + di, cj + dj);
            let (sx, sz) = plate_seed_point(seed, i, j);
            let d = (sx - x) * (sx - x) + (sz - z) * (sz - z);
            if d < best || (d == best && (i, j) < key) {
                best = d;
                key = (i, j);
            }
        }
    }
    key
}

// ---- the mod's small formulas ---------------------------------------------------------------------------------------------------

/// Half width of a stream draining `acc` cells (`NetworkTracer.b(acc, cfg)`).
fn width_of(acc: f64) -> f64 {
    let area = acc * (CELL * CELL) as f64;
    MAX_W.min(BASE_W + WIDTH_SCALE * (area / 2500.0).sqrt())
}

/// How far the water surface lies under the grade (`RiverEngine.insetFor`).
fn inset_for(w: f64) -> i32 {
    (DITCH_INSET * (0.5 + 0.5 * (w / 3.0).min(1.0)) + 0.5).floor() as i32
}

/// `FlowSpeed.forReach`: slope (blocks per block, positive downhill) and the widths at the two ends of the reach.
fn flow_for_reach(slope: f64, a1: f64, a2: f64) -> f64 {
    let s = (slope.max(0.0001) / (1.0 / 7.0)).powf(1.0 / 3.0);
    let r = if a2 > 1e-6 && a1 > 1e-6 { (a2 / a1).max(0.6).min(1.8) } else { 1.0 };
    (s * r).max(0.25).min(2.0)
}

/// `FlowSpeed.at`: the reach speed times a factor for the depth.
fn flow_at_depth(speed: f64, depth: f64) -> f64 {
    let f = ((depth.max(0.5) / 2.0).powf(1.0 / 3.0)).max(0.65).min(1.35);
    (speed * f).max(0.25).min(2.0)
}

/// `HydraulicGeometry.braidFloor` / `expectedBarShare`.
fn braid_floor(depth: f64) -> f64 {
    2.2f64.min(depth.max(0.0) * 0.7)
}

fn expected_bar_share(s: f64) -> f64 {
    0.22 * s.max(0.0).min(1.0).powf(1.8) * 0.75
}

/// `NetworkTracer.a(double, double, cfg)`: keep a reach's width within the inset class of its base width, so that widening by the
/// hydraulic geometry never moves the water surface.
fn adjust_width(w: f64, base: f64) -> f64 {
    let ib = inset_for(base);
    if inset_for(w) == ib {
        return w;
    }
    if w < base {
        let (mut lo, mut hi) = (0.0, base);
        for _ in 0..24 {
            let mid = (lo + hi) * 0.5;
            if inset_for(mid) < ib {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        return hi;
    }
    let (mut lo, mut hi) = (base, base.max(MAX_W));
    for _ in 0..24 {
        let mid = (lo + hi) * 0.5;
        if inset_for(mid) <= ib {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

/// Half width and flow speed of every point of a chain: `NetworkTracer.a(List, cfg, HydraulicGeometry, WetnessSampler, long, int, boolean)`.
/// Width follows the slope over +-8 blocks (steeper: narrower), the climate (wetter: wider), a slow random variation, and, on flat,
/// loose ground, braiding (shallower and wider, to carry the same flow) and delta mouths over the last 64 blocks into the sea.
/// `wet_at` is the humidity at a block; the mod's `cohesion` has no counterpart here and is taken as `0.4 + 0.6 * wetness`
/// (UNVERIFIED: dry ground is loose and braids).
fn hydraulic_widths(pts: &[Pt], seed: i64, cid: i64, to_sea: bool, wet_at: &dyn Fn(f64, f64) -> f64) -> (Vec<f64>, Vec<f64>) {
    let n = pts.len();
    let mut cum = vec![0.0; n];
    for i in 1..n {
        cum[i] = cum[i - 1] + (pts[i].x - pts[i - 1].x).hypot(pts[i].z - pts[i - 1].z);
    }
    let total = cum[n - 1];
    let tau = std::f64::consts::PI * 2.0;
    let (p1, p2) = (unit3(seed, cid, 21) * tau, unit3(seed, cid, 22) * tau);
    // Wetness and cohesion, read every 32 blocks along the chain.
    let (mut wet, mut coh) = (vec![0.5; n], vec![1.0; n]);
    let (mut w_now, mut c_now, mut next) = (0.5, 1.0, 0.0);
    for i in 0..n {
        if cum[i] >= next {
            w_now = wet_at(pts[i].x.round(), pts[i].z.round()).max(0.0).min(1.0);
            c_now = (0.4 + 0.6 * w_now).max(0.0).min(1.0);
            next = cum[i] + 32.0;
        }
        wet[i] = w_now;
        coh[i] = c_now;
    }
    let (mut width, mut speed) = (vec![0.0; n], vec![0.0; n]);
    let (mut lo, mut hi, mut flat_run) = (0usize, 0usize, 0.0);
    for i in 0..n {
        while lo < i && cum[i] - cum[lo] > 8.0 {
            lo += 1;
        }
        while hi < n - 1 && cum[hi + 1] - cum[i] <= 8.0 {
            hi += 1;
        }
        let span = cum[hi] - cum[lo];
        let slope = if span > 1e-6 { (pts[lo].y - pts[hi].y).abs() / span } else { 0.0 };
        let steep = (slope / 0.12).min(1.0);
        let mut f = 1.0;
        f *= 1.0 + HG_SLOPE * 0.3 * (1.0 - 2.0 * steep);
        f *= 1.0 + HG_SLOPE * 0.15 * (1.0 - 2.0 * coh[i]);
        f *= 1.0 + HG_CLIMATE * 0.25 * (2.0 * wet[i] - 1.0);
        f *= 1.0 + HG_VARIATION * 0.18 * (0.6 * (cum[i] * 0.045 + p1).sin() + 0.4 * (cum[i] * 0.011 + p2).sin());
        flat_run = if slope <= 0.03 { flat_run + if i > 0 { cum[i] - cum[i - 1] } else { 0.0 } } else { 0.0 };
        let loose = ((0.6 - coh[i]) / 0.6).min(1.0).max(0.0);
        let base = width_of(pts[i].acc);
        let mut bar = 0.0;
        let (mut flat_f, mut run_f) = ((1.0 - slope / 0.04).max(0.0), (flat_run / 64.0).min(1.0));
        if pts[i].y <= (SEA_LEVEL + 1) as f64 {
            flat_f = 1.0;
            run_f = 1.0;
        }
        let braid = loose * flat_f * run_f * HG_SLOPE.sqrt();
        let share = braid.sqrt();
        let depth = 2.0 * base * STREAM_DEPTH;
        let bed = depth * (1.0 - share) + braid_floor(depth) * share;
        let section = 2.0 / 3.0 + (1.0 / 3.0) * share;
        if bed > 1e-6 {
            let open = (0.2f64).max(1.0 - expected_bar_share(braid));
            bar = (depth * (2.0 / 3.0) / (bed * section * open) - 1.0).min(2.5).max(0.0);
        }
        if to_sea {
            let d = total - cum[i];
            if d < 64.0 {
                let t = 1.0 - d / 64.0;
                let smooth = t * t * (3.0 - 2.0 * t);
                bar = bar.max(HG_DELTA * 2.0 * smooth * (flat_run / 32.0).min(1.0) * loose);
            }
        }
        f *= 1.0 + bar;
        width[i] = adjust_width(BASE_W.max(MAX_W.min(base * f)), base);
        speed[i] = flow_for_reach(slope, width[i], base);
    }
    (width, speed)
}

#[derive(Clone, Copy)]
struct Pt {
    x: f64,
    z: f64,
    y: f64,
    acc: f64,
}

fn lerp_pt(a: Pt, b: Pt, t: f64) -> Pt {
    Pt { x: a.x + (b.x - a.x) * t, z: a.z + (b.z - a.z) * t, y: a.y + (b.y - a.y) * t, acc: a.acc + (b.acc - a.acc) * t }
}

/// `NetworkTracer.a(List, int)`: corner cutting, each point becomes the points at 1/4 and 3/4 of its segments.
fn chaikin(mut pts: Vec<Pt>, passes: usize) -> Vec<Pt> {
    for _ in 0..passes {
        if pts.len() < 3 {
            break;
        }
        let mut out = Vec::with_capacity(pts.len() * 2);
        out.push(pts[0]);
        for w in pts.windows(2) {
            out.push(lerp_pt(w[0], w[1], 0.25));
            out.push(lerp_pt(w[0], w[1], 0.75));
        }
        out.push(pts[pts.len() - 1]);
        pts = out;
    }
    pts
}

/// `NetworkTracer.a(List, long, int, cfg)`: the meander. `seed` is the plate's, `cid` the chain's number.
fn meander(pts: Vec<Pt>, seed: i64, cid: i64) -> Vec<Pt> {
    let n = pts.len();
    if n < 3 {
        return pts;
    }
    let amp = CELL as f64 * MEANDER;
    let tau = std::f64::consts::PI * 2.0;
    let (p1, p2, p3) = (unit3(seed, cid, 11) * tau, unit3(seed, cid, 12) * tau, unit3(seed, cid, 13) * tau);
    let mut cum = vec![0.0; n];
    for i in 1..n {
        cum[i] = cum[i - 1] + (pts[i].x - pts[i - 1].x).hypot(pts[i].z - pts[i - 1].z);
    }
    let mut out = Vec::with_capacity(n);
    out.push(pts[0]);
    for i in 1..n - 1 {
        let (a, c, b) = (pts[i - 1], pts[i], pts[i + 1]);
        let (dx, dz) = (b.x - a.x, b.z - a.z);
        let len = dx.hypot(dz);
        if len < 1e-6 {
            out.push(c);
            continue;
        }
        let (nx, nz) = (-dz / len, dx / len);
        let taper = (std::f64::consts::PI * i as f64 / (n - 1) as f64).sin();
        let flat = (1.0 - (a.y - b.y).abs() / len / 1.0).max(0.0);
        let off = amp * taper * flat * (0.5 * (cum[i] * 0.03 + p1).sin() + 0.3 * (cum[i] * 0.07 + p2).sin() + 0.2 * (cum[i] * 0.017 + p3).sin());
        out.push(Pt { x: c.x + nx * off, z: c.z + nz * off, y: c.y, acc: c.acc });
    }
    out.push(pts[n - 1]);
    out
}

/// `NetworkTracer.b(List)`: drop points closer than 0.25 to the one before (the last one replaces it).
fn dedupe(pts: Vec<Pt>) -> Vec<Pt> {
    if pts.len() < 3 {
        return pts;
    }
    let mut out: Vec<Pt> = Vec::with_capacity(pts.len());
    out.push(pts[0]);
    let last = pts.len() - 1;
    for (i, p) in pts.iter().enumerate().skip(1) {
        let q = out[out.len() - 1];
        if (p.x - q.x).hypot(p.z - q.z) < 0.25 {
            if i == last && out.len() >= 2 {
                let l = out.len() - 1;
                out[l] = *p;
            } else if i == last {
                out.push(*p);
            }
        } else {
            out.push(*p);
        }
    }
    out
}

// ---- the network of one plate ------------------------------------------------------------------------------------------------

/// A straight piece of a river, `1` upstream to `2` downstream: position, grade (the water surface plus its inset), half width,
/// flow speed and the distance still to run to the sea at each end.
#[derive(Clone, Copy)]
struct Seg {
    x1: f64,
    z1: f64,
    y1: f64,
    w1: f64,
    x2: f64,
    z2: f64,
    y2: f64,
    w2: f64,
    sp: f64,
    o1: f64,
    o2: f64,
}

/// What the carve needs to know about a column (the mod's `ColumnPlan`, the fields this port uses).
#[derive(Clone, Copy, Debug)]
pub struct Plan {
    /// Within the half width of the centre line.
    pub in_channel: bool,
    /// Top water block's y, and the bed's y.
    pub water_y: i32,
    pub bed_y: i32,
    /// 0 at the channel's edge, 1 at the end of the bank.
    pub blend: f64,
    pub distance: f64,
    pub width: f64,
    pub grade: f64,
    /// Unit direction of the flow.
    pub dir: [f64; 2],
    /// `FlowSpeed.at`, 0.25..2.
    pub speed: f64,
    /// Distance still to run to the outlet.
    pub to_outlet: f64,
}

impl Plan {
    /// Blocks from the channel's edge: negative inside the channel, the bank's distance outside it.
    fn clearance(&self) -> f64 {
        self.distance - self.width
    }
}

/// The rivers and lakes of one plate (and the margin round it).
pub struct Network {
    segs: Vec<Seg>,
    index: HashMap<(i32, i32), Vec<u32>>,
    /// Lake of each grid cell of the window (`u32::MAX`: none), the window's origin cell and size, and the lakes' top water block.
    lake_of: Vec<u32>,
    lake_top: Vec<i32>,
    g0: (i32, i32),
    n: (usize, usize),
}

/// The grid cell (node index) a block belongs to: a node owns the 12 x 12 blocks round it.
fn cell_of(v: f64) -> i32 {
    ((v + (CELL / 2) as f64) / CELL as f64).floor() as i32
}

/// How far from the channel's edge a river still shapes the ground.
fn reach(w: f64) -> f64 {
    VALLEY.max(w * BANK_WIDTH)
}

/// Distance from (x, z) to the segment and the parameter of the closest point.
fn seg_distance(s: &Seg, x: f64, z: f64) -> (f64, f64) {
    let (dx, dz) = (s.x2 - s.x1, s.z2 - s.z1);
    let len2 = dx * dx + dz * dz;
    let t = if len2 <= 1e-9 { 0.0 } else { (((x - s.x1) * dx + (z - s.z1) * dz) / len2).max(0.0).min(1.0) };
    ((x - (s.x1 + dx * t)).hypot(z - (s.z1 + dz * t)), t)
}

impl Network {
    /// The segment a point belongs to: the one with the least clearance (distance to the centre line minus the half width), so a point
    /// inside a wide channel is never taken for the bank of a narrow river that merely ends nearer to it. The mod takes the plain
    /// nearest centre line; its carver copes with the confluence, this one does not, and used to wall off the main branch.
    fn nearest(&self, x: f64, z: f64) -> Option<(usize, f64, f64)> {
        let b = ((x / BUCKET as f64).floor() as i32, (z / BUCKET as f64).floor() as i32);
        let mut best: Option<(usize, f64, f64, f64)> = None; // segment, clearance, distance, t
        for &i in self.index.get(&b)? {
            let s = &self.segs[i as usize];
            let (d, t) = seg_distance(s, x, z);
            let clear = d - (s.w1 + (s.w2 - s.w1) * t);
            if best.map_or(true, |(bi, bc, _, _)| clear < bc || (clear == bc && (i as usize) < bi)) {
                best = Some((i as usize, clear, d, t));
            }
        }
        best.map(|(i, _, d, t)| (i, t, d))
    }

    /// `RiverEngine.plan` at a point, `None` away from every river.
    pub fn plan(&self, x: f64, z: f64) -> Option<Plan> {
        let (si, t, d) = self.nearest(x, z)?;
        let s = &self.segs[si];
        let w = s.w1 + (s.w2 - s.w1) * t;
        let band = 5.0f64.max(w * BANK_WIDTH);
        if d > w + reach(w) {
            return None;
        }
        let grade = s.y1 + (s.y2 - s.y1) * t;
        let water_y = (SEA_LEVEL - 1).max(grade.floor() as i32 - inset_for(w));
        let depth = ((2.0 * w * STREAM_DEPTH + 0.5).floor() as i32).max(1);
        let len = (s.x2 - s.x1).hypot(s.z2 - s.z1).max(1e-6);
        Some(Plan {
            in_channel: d <= w,
            water_y,
            bed_y: water_y - depth,
            blend: ((d - w) / band).max(0.0).min(1.0).powf(A_RIVER),
            distance: d,
            width: w,
            grade,
            dir: [(s.x2 - s.x1) / len, (s.z2 - s.z1) / len],
            speed: flow_at_depth(s.sp, depth as f64),
            to_outlet: s.o1 + (s.o2 - s.o1) * t,
        })
    }

    /// Top water block's y of the lake covering the block (x, z): the lake's cells and 6 blocks round them.
    fn lake_top_at(&self, x: i32, z: i32) -> Option<i32> {
        let (ci, cj) = (cell_of(x as f64 + 0.5), cell_of(z as f64 + 0.5));
        let mut best: Option<i32> = None;
        for di in -1..=1 {
            for dj in -1..=1 {
                let (i, j) = (ci + di - self.g0.0, cj + dj - self.g0.1);
                if i < 0 || j < 0 || i as usize >= self.n.0 || j as usize >= self.n.1 {
                    continue;
                }
                let id = self.lake_of[i as usize * self.n.1 + j as usize];
                if id == u32::MAX {
                    continue;
                }
                // The cell owns [12 g - 6, 12 g + 6).
                let (gx, gz) = ((ci + di) * CELL - CELL / 2, (cj + dj) * CELL - CELL / 2);
                let ex = (gx - x).max(0).max(x - (gx + CELL - 1));
                let ez = (gz - z).max(0).max(z - (gz + CELL - 1));
                if ex.max(ez) <= 6 {
                    let t = self.lake_top[id as usize];
                    best = Some(best.map_or(t, |b| b.max(t)));
                }
            }
        }
        best
    }

    /// The current at a point in blocks per second, `None` outside a channel.
    pub fn flow_at(&self, x: f32, z: f32) -> Option<[f32; 2]> {
        let p = self.plan(x as f64, z as f64)?;
        if p.distance > p.width + 0.5 {
            return None;
        }
        let v = p.speed * PUSH;
        Some([(p.dir[0] * v) as f32, (p.dir[1] * v) as f32])
    }
}

/// Best-first walk over the real terrain from node (gi, gj) to the first sea node, lowest pass first: the nodes (and their heights).
fn trace_tail(height: &mut dyn FnMut(i32, i32) -> f64, gi: i32, gj: i32, sea_h: f64) -> Option<Vec<(i32, i32, f64)>> {
    let y0 = height(gi, gj);
    let mut heap = BinaryHeap::new();
    let mut parent: HashMap<(i32, i32), Option<(i32, i32)>> = HashMap::new();
    let mut level: HashMap<(i32, i32), f64> = HashMap::new();
    parent.insert((gi, gj), None);
    level.insert((gi, gj), y0);
    heap.push(Reverse(((y0 * 4096.0) as i64, gi, gj)));
    let mut steps = 0;
    while let Some(Reverse((_, a, b))) = heap.pop() {
        steps += 1;
        if steps > TAIL_CAP {
            break;
        }
        let lv = level[&(a, b)];
        let ha = height(a, b);
        if ha < sea_h {
            let mut path = vec![(a, b, ha)];
            let mut c = (a, b);
            while let Some(Some(p)) = parent.get(&c) {
                c = *p;
                path.push((c.0, c.1, height(c.0, c.1)));
            }
            path.reverse();
            return Some(path);
        }
        for (da, db) in NB {
            let c = (a + da, b + db);
            if parent.contains_key(&c) {
                continue;
            }
            parent.insert(c, Some((a, b)));
            let l = height(c.0, c.1).max(lv);
            level.insert(c, l);
            heap.push(Reverse(((l * 4096.0) as i64, c.0, c.1)));
        }
    }
    None
}

/// Route one plate. `probe(gx, gz)` is the (surface height, humidity 0..1) of the node at world (12 gx, 12 gz).
fn build(seed: i64, key: (i32, i32), probe: &mut dyn FnMut(i32, i32) -> (f64, f64)) -> Network {
    let (cx, cz) = plate_seed_point(seed, key.0, key.1);
    let c = CELL as f64;
    let g0 = (((cx - PLATE) / c).floor() as i32 - HALO, ((cz - PLATE) / c).floor() as i32 - HALO);
    let g1 = (((cx + PLATE) / c).ceil() as i32 + HALO, ((cz + PLATE) / c).ceil() as i32 + HALO);
    let (nx, nz) = ((g1.0 - g0.0 + 1) as usize, (g1.1 - g0.1 + 1) as usize);
    let n = nx * nz;
    let at = |i: usize, j: usize| i * nz + j;
    let node = |k: usize| (g0.0 + (k / nz) as i32, g0.1 + (k % nz) as i32);

    // The plate's own cell, grown by the margin (separable dilation).
    let hl = HALO as usize;
    let mut own = vec![false; n];
    for i in 0..nx {
        for j in 0..nz {
            own[at(i, j)] = plate_at(seed, ((g0.0 + i as i32) * CELL) as f64, ((g0.1 + j as i32) * CELL) as f64) == key;
        }
    }
    let mut tmp = vec![false; n];
    for j in 0..nz {
        for i in 0..nx {
            if own[at(i, j)] {
                for a in i.saturating_sub(hl)..=(i + hl).min(nx - 1) {
                    tmp[at(a, j)] = true;
                }
            }
        }
    }
    let mut dom = vec![false; n];
    for i in 0..nx {
        for j in 0..nz {
            if tmp[at(i, j)] {
                for b in j.saturating_sub(hl)..=(j + hl).min(nz - 1) {
                    dom[at(i, b)] = true;
                }
            }
        }
    }
    let in_dom = |i: i32, j: i32| i >= 0 && j >= 0 && (i as usize) < nx && (j as usize) < nz && dom[at(i as usize, j as usize)];

    let mut h = vec![0.0f64; n];
    let mut wet = vec![0.0f64; n];
    for k in 0..n {
        if dom[k] {
            let (gx, gz) = node(k);
            let (a, b) = probe(gx, gz);
            h[k] = a;
            wet[k] = b;
        }
    }
    let sea_h = SEA_LEVEL as f64 - 1.0;

    // Priority flood from the sea and the window's edge: `fill` is the terrain with its depressions filled to their spill height.
    let mut fill = vec![f64::INFINITY; n];
    let mut done = vec![false; n];
    let mut order: Vec<usize> = Vec::with_capacity(n);
    let mut heap = BinaryHeap::new();
    for i in 0..nx {
        for j in 0..nz {
            let k = at(i, j);
            if !dom[k] {
                continue;
            }
            let edge = NB.iter().any(|&(a, b)| !in_dom(i as i32 + a, j as i32 + b));
            if h[k] < sea_h || edge {
                fill[k] = h[k];
                heap.push(Reverse(((h[k] * 4096.0) as i64, k)));
            }
        }
    }
    while let Some(Reverse((_, k))) = heap.pop() {
        if done[k] {
            continue;
        }
        done[k] = true;
        order.push(k);
        let (i, j) = ((k / nz) as i32, (k % nz) as i32);
        for (a, b) in NB {
            if !in_dom(i + a, j + b) {
                continue;
            }
            let m = at((i + a) as usize, (j + b) as usize);
            if fill[m].is_finite() {
                continue;
            }
            fill[m] = h[m].max(fill[k] + 0.001);
            heap.push(Reverse(((fill[m] * 4096.0) as i64, m)));
        }
    }

    // D8 receivers on the filled surface (steepest descent, a diagonal is sqrt 2 long). The sea has none.
    let mut recv = vec![NONE; n];
    for &k in &order {
        if h[k] < sea_h {
            continue;
        }
        let (i, j) = ((k / nz) as i32, (k % nz) as i32);
        let mut best = 0.0;
        for (a, b) in NB {
            if !in_dom(i + a, j + b) {
                continue;
            }
            let m = at((i + a) as usize, (j + b) as usize);
            if !fill[m].is_finite() {
                continue;
            }
            let s = (fill[k] - fill[m]) / if a != 0 && b != 0 { std::f64::consts::SQRT_2 } else { 1.0 };
            if s > best {
                best = s;
                recv[k] = m;
            }
        }
    }

    // Flow accumulation, high to low.
    let mut acc: Vec<f64> = dom.iter().map(|&d| if d { WETNESS_MULT } else { 0.0 }).collect();
    for &k in order.iter().rev() {
        if recv[k] != NONE {
            let a = acc[k];
            acc[recv[k]] += a;
        }
    }
    let thr = (MIN_ACC * GROW_DENSITY).max(1.0) * (16.0 / c) * (16.0 / c);
    let mut stream: Vec<bool> = (0..n).map(|k| dom[k] && fill[k] > SEA_LEVEL as f64 - 0.5 && h[k] >= sea_h && acc[k] >= thr).collect();

    // Dry headwaters are dropped; whatever lies downstream of a kept one stays.
    let plate_seed = hash3(seed, key.0 as i64, key.1 as i64) as i64;
    let mut has_up = vec![false; n];
    for &k in &order {
        if stream[k] && recv[k] != NONE && stream[recv[k]] {
            has_up[recv[k]] = true;
        }
    }
    let mut keep = vec![false; n];
    for &k in &order {
        if !stream[k] || has_up[k] {
            continue;
        }
        let p = ((wet[k] - WET_CUT) / (WET_REF - WET_CUT)).min(1.0).max(WET_FLOOR);
        keep[k] = unit3(plate_seed, k as i64, 71) < p;
    }
    for &k in order.iter().rev() {
        if keep[k] && recv[k] != NONE && stream[recv[k]] {
            keep[recv[k]] = true;
        }
    }
    for k in 0..n {
        stream[k] = stream[k] && keep[k];
    }

    // Separation: two stream systems (trees draining to different mouths) must not touch; the smaller yields its contact cell unless
    // one of the two is a mouth, and what that cuts off from the sea goes with it (the alive pass below).
    let mut system = vec![NONE; n];
    for &k in &order {
        if stream[k] {
            let r = recv[k];
            system[k] = if r != NONE && stream[r] { system[r] } else { k };
        }
    }
    let mut size: HashMap<usize, usize> = HashMap::new();
    for k in 0..n {
        if stream[k] {
            *size.entry(system[k]).or_insert(0) += 1;
        }
    }
    let mouth = |k: usize, stream: &[bool]| recv[k] == NONE || !stream[recv[k]];
    let mut drop = vec![false; n];
    for k in 0..n {
        if !stream[k] {
            continue;
        }
        let (i, j) = ((k / nz) as i32, (k % nz) as i32);
        for (a, b) in NB {
            if !in_dom(i + a, j + b) {
                continue;
            }
            let m = at((i + a) as usize, (j + b) as usize);
            if !stream[m] || system[m] == system[k] || mouth(k, &stream) || mouth(m, &stream) {
                continue;
            }
            if (size[&system[k]], system[k]) < (size[&system[m]], system[m]) {
                drop[k] = true;
            }
        }
    }
    for k in 0..n {
        stream[k] = stream[k] && !drop[k];
    }

    // Every river must end in the sea. One that leaves the window is traced on over the real terrain; one that finds none is culled.
    let mut memo: HashMap<(i32, i32), f64> = HashMap::new();
    let mut height = |gx: i32, gz: i32| -> f64 { *memo.entry((gx, gz)).or_insert_with(|| probe(gx, gz).0) };
    let mut tails: HashMap<usize, Vec<(i32, i32, f64)>> = HashMap::new();
    for k in 0..n {
        if stream[k] && recv[k] == NONE {
            let (gx, gz) = node(k);
            if let Some(t) = trace_tail(&mut height, gx, gz, sea_h) {
                tails.insert(k, t);
            }
        }
    }
    let mut alive = vec![false; n];
    for &k in &order {
        if !stream[k] {
            continue;
        }
        let r = recv[k];
        alive[k] = if r == NONE {
            tails.contains_key(&k)
        } else if stream[r] {
            alive[r]
        } else {
            h[r] < sea_h
        };
    }
    for k in 0..n {
        stream[k] = stream[k] && alive[k];
    }

    // Lakes: basins of the filled terrain, at least 4 cells and 1.5 blocks deep on average, above the sea. They spill over their rim.
    let mut lake_of = vec![u32::MAX; n];
    let mut lake_top: Vec<i32> = Vec::new();
    let basin = |k: usize| dom[k] && h[k] >= sea_h && fill[k] - h[k] >= 0.5;
    let mut seen = vec![false; n];
    for s in 0..n {
        if seen[s] || !basin(s) || !own[s] {
            continue;
        }
        let mut comp = vec![s];
        seen[s] = true;
        let mut at_ = 0;
        while at_ < comp.len() {
            let k = comp[at_];
            at_ += 1;
            let (i, j) = ((k / nz) as i32, (k % nz) as i32);
            for (a, b) in NB {
                if !in_dom(i + a, j + b) {
                    continue;
                }
                let m = at((i + a) as usize, (j + b) as usize);
                if !seen[m] && basin(m) && (fill[m] - fill[s]).abs() <= 0.5 {
                    seen[m] = true;
                    comp.push(m);
                }
            }
        }
        let depth = comp.iter().map(|&k| fill[k] - h[k]).sum::<f64>() / comp.len() as f64;
        let level = comp.iter().map(|&k| fill[k]).fold(f64::INFINITY, f64::min);
        let top = level.floor() as i32 - 2;
        if comp.len() >= LAKE_MIN_CELLS && depth >= LAKE_MIN_DEPTH && top >= SEA_LEVEL - 1 {
            let id = lake_top.len() as u32;
            lake_top.push(top);
            for &k in &comp {
                lake_of[k] = id;
            }
        }
    }

    // Distance still to run to the outlet, per stream cell, low to high.
    let mut to_out = vec![0.0f64; n];
    for &k in &order {
        if !stream[k] {
            continue;
        }
        let r = recv[k];
        to_out[k] = if r == NONE {
            tails.get(&k).map_or(0.0, |t| (t.len() - 1) as f64 * c)
        } else if stream[r] {
            let (i, j) = (k / nz, k % nz);
            let (p, q) = (r / nz, r % nz);
            to_out[r] + if i != p && j != q { c * std::f64::consts::SQRT_2 } else { c }
        } else {
            0.0
        };
    }

    // Chains: from a headwater or a confluence to the next confluence, the sea or a lake. Stream cells inside a lake are not drawn.
    let drawn: Vec<bool> = (0..n).map(|k| stream[k] && lake_of[k] == u32::MAX).collect();
    let mut inflow = vec![0u8; n];
    for &k in &order {
        if drawn[k] && recv[k] != NONE && drawn[recv[k]] {
            inflow[recv[k]] = inflow[recv[k]].saturating_add(1);
        }
    }
    // Humidity at a block, read off the grid (0.5 where the window has none, as the mod's default).
    let wet_near = |x: f64, z: f64| -> f64 {
        let (i, j) = (cell_of(x) - g0.0, cell_of(z) - g0.1);
        if i < 0 || j < 0 || i as usize >= nx || j as usize >= nz || !dom[at(i as usize, j as usize)] {
            return 0.5;
        }
        wet[at(i as usize, j as usize)]
    };
    let mut segs: Vec<Seg> = Vec::new();
    let mut cid = 0i64;
    for &start in order.iter().rev() {
        if !drawn[start] || inflow[start] == 1 {
            continue;
        }
        let mut path = vec![start];
        loop {
            let r = recv[*path.last().unwrap()];
            if r == NONE {
                break;
            }
            path.push(r);
            if !drawn[r] || inflow[r] != 1 {
                break;
            }
        }
        let last = *path.last().unwrap();
        let mut pts: Vec<Pt> = Vec::with_capacity(path.len() + 8);
        let mut prev_acc = acc[start];
        for &k in &path {
            let (gx, gz) = node(k);
            let (y, a) = if drawn[k] { (fill[k], acc[k]) } else { (fill[k].max(sea_h), prev_acc) };
            prev_acc = a;
            pts.push(Pt { x: (gx * CELL) as f64, z: (gz * CELL) as f64, y, acc: a });
        }
        let mut out_base = if drawn[last] { to_out[last] } else { 0.0 };
        if let Some(t) = tails.get(&last) {
            // The tail runs on over the real terrain; the grade follows the lowest ground so far, never rising, never under the sea.
            let mut ly = pts[pts.len() - 1].y;
            for &(gx, gz, ty) in t.iter().skip(1) {
                ly = ly.min(ty.max(sea_h));
                pts.push(Pt { x: (gx * CELL) as f64, z: (gz * CELL) as f64, y: ly, acc: prev_acc });
            }
            out_base = 0.0;
        }
        // The grade never rises downstream.
        for t in 1..pts.len() {
            if pts[t].y > pts[t - 1].y {
                pts[t].y = pts[t - 1].y;
            }
        }
        let pts = dedupe(meander(chaikin(pts, SMOOTHING), plate_seed, cid));
        cid += 1;
        // Arc length to the chain's end, for the distance to the outlet; width and speed per point from the hydraulic geometry.
        let m = pts.len();
        let mut arc = vec![0.0; m];
        for i in (0..m.saturating_sub(1)).rev() {
            arc[i] = arc[i + 1] + (pts[i + 1].x - pts[i].x).hypot(pts[i + 1].z - pts[i].z);
        }
        let to_sea = tails.contains_key(&last) || (!drawn[last] && h[last] < sea_h);
        let (width, speed) = hydraulic_widths(&pts, plate_seed, cid - 1, to_sea, &wet_near);
        for i in 0..m.saturating_sub(1) {
            let (p, q) = (pts[i], pts[i + 1]);
            segs.push(Seg {
                x1: p.x,
                z1: p.z,
                y1: p.y,
                w1: width[i],
                x2: q.x,
                z2: q.z,
                y2: q.y,
                w2: width[i + 1],
                sp: (speed[i] + speed[i + 1]) * 0.5,
                o1: out_base + arc[i],
                o2: out_base + arc[i + 1],
            });
        }
    }

    let index = index_of(&segs);
    Network { segs, index, lake_of, lake_top, g0, n: (nx, nz) }
}

/// Every bucket a segment's channel and bank can reach, per bucket the segments to test.
fn index_of(segs: &[Seg]) -> HashMap<(i32, i32), Vec<u32>> {
    let mut index: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
    for (si, s) in segs.iter().enumerate() {
        let wmax = s.w1.max(s.w2);
        let r = wmax + reach(wmax);
        let b = BUCKET as f64;
        let (x0, x1) = (((s.x1.min(s.x2) - r) / b).floor() as i32, ((s.x1.max(s.x2) + r) / b).floor() as i32);
        let (z0, z1) = (((s.z1.min(s.z2) - r) / b).floor() as i32, ((s.z1.max(s.z2) + r) / b).floor() as i32);
        for bx in x0..=x1 {
            for bz in z0..=z1 {
                index.entry((bx, bz)).or_default().push(si as u32);
            }
        }
    }
    index
}

// ---- the carve ---------------------------------------------------------------------------------------------------------------------

/// Top solid block of a column (not water, not ice), or -1.
fn surface(col: &[u8]) -> i32 {
    (0..128).rev().find(|&y| !matches!(col[y], block::AIR | block::WATER | block::WATER_MOVING | block::ICE)).map_or(-1, |y| y as i32)
}

/// Blocks of a bank, after the mod's `bank_style` files by biome (river / plains: clay and gravel, desert and beach: sand, mountains:
/// gravel at the line; the mod's mud is dirt here, b1.7.3 has none).
struct Style {
    bed: &'static [u8],
    line: &'static [u8],
    top: &'static [u8],
    fill: u8,
}

fn style(b: Biome) -> Style {
    use block::*;
    match b {
        Biome::Desert | Biome::Beach | Biome::Ocean => Style { bed: &[SAND], line: &[SAND], top: &[SAND], fill: SAND },
        Biome::Savanna | Biome::Shrubland => Style { bed: &[SAND, GRAVEL, SAND], line: &[SAND, DIRT], top: &[GRASS], fill: DIRT },
        Biome::Mountains => Style { bed: &[GRAVEL, STONE], line: &[GRAVEL], top: &[STONE], fill: STONE },
        Biome::Swamp => Style { bed: &[DIRT, CLAY], line: &[DIRT, GRASS], top: &[GRASS], fill: DIRT },
        _ => Style { bed: &[CLAY, GRAVEL], line: &[GRAVEL], top: &[GRASS], fill: DIRT },
    }
}

/// One of `list`, by `h` in 0..1 (patches of the same block come from `h` being the same over a few blocks).
fn pick(list: &[u8], h: f64) -> u8 {
    list[((h * list.len() as f64) as usize).min(list.len() - 1)]
}

/// The networks that can reach one chunk (its plate's first), made on demand.
pub struct Region {
    nets: Vec<Arc<Network>>,
}

impl Region {
    /// The plan of a column over every network that reaches the chunk: the one with the least clearance, as `Network::nearest` does inside
    /// one network (a channel, clearance <= 0, always beats a bank). It used to be the first network with any plan, so the bank of a river
    /// of the plate's own network (often a lower one) cut or skipped the channel of a higher river from a neighbouring plate: the upper
    /// water was left standing on a cliff, or the river was gone. Ties go to the earlier network (the plate's own).
    fn plan_at(&self, x: f64, z: f64) -> Option<Plan> {
        self.nets.iter().filter_map(|n| n.plan(x, z)).reduce(|a, b| if b.clearance() < a.clearance() { b } else { a })
    }

    /// Cut this region's rivers and fill its lakes into chunk (cx, cz), `(x << 11) | (z << 7) | y`, after terrain and surface.
    /// `biomes` and `temps` are the chunk's, `x * 16 + z`; ice caps the water where it is cold.
    pub fn carve(&self, cx: i32, cz: i32, blocks: &mut [u8], biomes: &[Biome], temps: &[f64]) {
        for x in 0..16usize {
            for z in 0..16usize {
                let (wx, wz) = (cx * 16 + x as i32, cz * 16 + z as i32);
                let col = (x << 11) | (z << 7);
                let (st, cold) = (style(biomes[x * 16 + z]), temps[x * 16 + z] < 0.5);
                let pat = unit3(0x5EED, wx.div_euclid(3) as i64, wz.div_euclid(3) as i64);
                let s = surface(&blocks[col..col + 128]);
                let (px, pz) = (wx as f64 + 0.5, wz as f64 + 0.5);
                let plan = self.plan_at(px, pz);
                if !plan.is_some_and(|p| p.in_channel) {
                    if let Some(top) = self.nets.iter().find_map(|n| n.lake_top_at(wx, wz)) {
                        // Water fills what lies under the lake's surface; the shore is the terrain's own.
                        if (40..top).contains(&s) {
                            for y in s + 1..=top {
                                blocks[col + y as usize] = if cold && y == top { block::ICE } else { block::WATER };
                            }
                            blocks[col + s as usize] = pick(st.bed, pat);
                        }
                        continue;
                    }
                }
                let Some(p) = plan else { continue };
                let (ws, bed) = (p.water_y, p.bed_y);
                if s < 5 || bed < 5 || ws > 120 {
                    continue;
                }
                // Water already stands at the river's level (the sea, a lake, a bigger river): a mouth opens into it. The bank's berm used
                // to treat such a column as low ground and fill it with dirt and gravel, a dam across every mouth.
                if matches!(blocks[col + ws as usize], block::WATER | block::WATER_MOVING | block::ICE) {
                    continue;
                }
                if p.in_channel {
                    // Channel: water up to the surface, whatever stood above it removed, a bed under it (built up where the ground is lower).
                    if s + 6 < bed {
                        continue;
                    }
                    for y in bed + 1..=s.max(ws).min(126) {
                        blocks[col + y as usize] = if y <= ws {
                            if cold && y == ws { block::ICE } else { block::WATER }
                        } else {
                            block::AIR
                        };
                    }
                    let bm = pick(st.bed, pat);
                    for y in (if s < bed { s + 1 } else { bed - 1 }).max(1)..=bed {
                        blocks[col + y as usize] = bm;
                    }
                } else {
                    // Bank: a slope up from the waterline cut into higher ground, a berm on lower ground so the water is held.
                    let k = p.distance - p.width;
                    let band = 5.0f64.max(p.width * BANK_WIDTH);
                    // The slope rises BANK_SLOPE a block from the waterline (the first block level with the water cell, its top 1/9 over the
                    // water) and, over the outer half of the reach, eases into the terrain's own height, so a hill beside the river is a
                    // valley side and never a wall at the end of the band.
                    let t = ws as f64 + k * BANK_SLOPE;
                    let u = ((k / reach(p.width) - 0.5) * 2.0).clamp(0.0, 1.0);
                    let target = ((t + (s as f64 - t) * u * u * (3.0 - 2.0 * u)).floor() as i32).min(120);
                    let top = pick(if k < 1.5 { st.line } else { st.top }, pat);
                    if s > ws {
                        if s > target {
                            for y in target + 1..=s {
                                blocks[col + y as usize] = block::AIR;
                            }
                            blocks[col + target as usize] = top;
                            for y in (target - 2).max(1)..target {
                                if blocks[col + y as usize] == block::STONE {
                                    blocks[col + y as usize] = st.fill;
                                }
                            }
                        }
                    } else if k <= 2.5 && ws - s <= 5 {
                        // A levee on low ground so the water is held: level with the water at the channel's edge, a block lower farther out,
                        // never the extra block over the water it used to be.
                        let btop = ws - (k * 0.75) as i32;
                        if btop > s {
                            for y in s + 1..btop {
                                blocks[col + y as usize] = st.fill;
                            }
                            blocks[col + btop as usize] = top;
                        }
                    }
                    // Flowers along the water, thinning out with the distance.
                    let ns = surface(&blocks[col..col + 128]);
                    let r = unit3(0x71, wx as i64, wz as i64);
                    let f = 1.0 - k / band;
                    if ns > 0 && ns < 126 && blocks[col + ns as usize] == block::GRASS && blocks[col + ns as usize + 1] == block::AIR && r < 0.12 * f {
                        blocks[col + ns as usize + 1] = if r < 0.06 * f { block::FLOWER_Y } else { block::FLOWER_R };
                    }
                }
            }
        }
    }
}

// ---- the world's networks ----------------------------------------------------------------------------------------------------------

/// The plates of one world, made on demand and shared by every thread that generates for it.
#[derive(Default)]
pub struct Hydro {
    seed: i64,
    plates: Mutex<Vec<((i32, i32), Arc<OnceLock<Arc<Network>>>)>>,
}

impl Hydro {
    /// The (one) `Hydro` of the world with this seed and type.
    pub fn of(seed: i64, tectonic: bool) -> Arc<Hydro> {
        static WORLDS: OnceLock<Mutex<HashMap<(i64, bool), Arc<Hydro>>>> = OnceLock::new();
        let mut m = WORLDS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
        m.entry((seed, tectonic)).or_insert_with(|| Arc::new(Hydro { seed, plates: Mutex::default() })).clone()
    }

    /// The network of a plate, routed with `probe` if nobody has yet. A second thread that needs the same plate waits for the first.
    /// ponytail: past `MAX_PLATES` the oldest plate is dropped and made again if it is needed.
    fn network(&self, key: (i32, i32), probe: &mut dyn FnMut(i32, i32) -> (f64, f64)) -> Arc<Network> {
        let slot = {
            let mut p = self.plates.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((_, s)) = p.iter().find(|(k, _)| *k == key) {
                s.clone()
            } else {
                if p.len() >= MAX_PLATES {
                    p.remove(0);
                }
                let s = Arc::new(OnceLock::new());
                p.push((key, s.clone()));
                s
            }
        };
        slot.get_or_init(|| Arc::new(build(self.seed, key, probe))).clone()
    }

    /// The networks that reach chunk (cx, cz): its plate's and the neighbours' whose margin covers it.
    pub fn region(&self, cx: i32, cz: i32, probe: &mut dyn FnMut(i32, i32) -> (f64, f64)) -> Arc<Region> {
        let (mx, mz) = (cx as f64 * 16.0 + 8.0, cz as f64 * 16.0 + 8.0);
        let r = (HALO * CELL) as f64 + 8.0;
        let mut keys = vec![plate_at(self.seed, mx, mz)];
        for dx in [-r, 0.0, r] {
            for dz in [-r, 0.0, r] {
                let k = plate_at(self.seed, mx + dx, mz + dz);
                if !keys.contains(&k) {
                    keys.push(k);
                }
            }
        }
        let nets = keys.into_iter().map(|k| self.network(k, probe)).collect();
        Arc::new(Region { nets })
    }

    /// The network at a block, if it has been made.
    fn peek(&self, x: f32, z: f32) -> Option<Arc<Network>> {
        let key = plate_at(self.seed, x as f64, z as f64);
        let p = self.plates.lock().ok()?;
        p.iter().find(|(k, _)| *k == key)?.1.get().cloned()
    }
}

/// The world whose currents `current` reads (the one being played).
static WATCHED: Mutex<Option<Arc<Hydro>>> = Mutex::new(None);

/// Play this world's currents (`None`: none, as in the Nether). A no-op in tests, so a test's world does not push another's entities.
pub fn watch(h: Option<Arc<Hydro>>) {
    #[cfg(not(test))]
    {
        *WATCHED.lock().unwrap_or_else(|e| e.into_inner()) = h;
    }
    #[cfg(test)]
    {
        let _ = h;
    }
}

/// The current at a point in blocks per second, `None` away from a river or on a plate not made yet.
pub fn current(x: f32, z: f32) -> Option<[f32; 2]> {
    let h = WATCHED.lock().ok()?.clone()?;
    h.peek(x, z)?.flow_at(x, z)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEED: i64 = 0xCAFE_BABE;

    /// Land rising east and north from a sea far in the west (x < -1700, past the plate's window, so its rivers are traced on to it), a little rough: (surface, humidity) of node (gx, gz).
    fn land(gx: i32, gz: i32) -> (f64, f64) {
        (66.0 + 0.0667 * (gx + 60) as f64 + 0.01 * gz.abs() as f64 + 2.0 * ((gx as f64) * 0.31).sin() * ((gz as f64) * 0.27).cos(), 0.9)
    }

    /// Hash, seed points and plates are the mod's own, checked against its `Hashing` and `PlateLattice` classes (run on a JVM).
    #[test]
    fn lattice_matches_the_mod() {
        assert_eq!(hash3(1, 2, 3) as i64, -7970969774787746530);
        assert_eq!(hash4(12345, -7, 99, 1) as i64, -9084196385740661577);
        assert_eq!(unit4(SEED, -3, 5, 2), 0.5797157391788732);
        let (x, z) = plate_seed_point(SEED, 0, -1);
        assert!((x - 349.3443395521448).abs() < 1e-9 && (z + 407.4152264968453).abs() < 1e-9, "{x} {z}");
        assert!((plate_seed_point(SEED, -2, 3).0 + 1115.6033249305112).abs() < 1e-9);
        for (p, k) in [((0.0, 0.0), (0, -1)), ((1500.0, -900.0), (2, -1)), ((-3000.0, 2500.0), (-5, 3)), ((100.0, 2047.0), (-1, 2))] {
            assert_eq!(plate_at(SEED, p.0, p.1), k);
        }
    }

    /// Width, inset and speed follow the mod's formulas.
    #[test]
    fn formulas_match_the_mod() {
        assert!((width_of(4.0) - (0.4 + (4.0f64 * (CELL * CELL) as f64 / 2500.0).sqrt())).abs() < 1e-12);
        assert_eq!(width_of(1.0e9), 16.0);
        assert_eq!((inset_for(0.9), inset_for(3.0), inset_for(9.0)), (2, 3, 3));
        assert!((width_of(0.0) - BASE_W).abs() < 1e-12);
        // The hydraulic width stays in the base width's inset class and between the mod's bounds; a delta mouth widens.
        let line: Vec<Pt> = (0..60).map(|i| Pt { x: i as f64 * 4.0, z: 0.0, y: 63.5, acc: 80.0 }).collect();
        let (w, sp) = hydraulic_widths(&line, SEED, 1, true, &|_, _| 0.1);
        assert!(w.iter().all(|&v| v >= BASE_W && v <= MAX_W) && sp.iter().all(|&v| (0.25..=2.0).contains(&v)));
        assert!(w[58] > w[0], "a delta mouth widens the last 64 blocks: {} vs {}", w[58], w[0]);
        let s = flow_for_reach(1.0 / 7.0, 2.0, 2.0);
        assert!((s - 1.0).abs() < 1e-12, "{s}");
        assert_eq!(flow_for_reach(0.0, 1.0, 1.0), 0.25);
        assert_eq!(flow_for_reach(5.0, 1.0, 1.0), 2.0);
    }

    /// The meander keeps both ends and the number of points; a flat chain leaves the line, a steep one does not.
    #[test]
    fn meander_keeps_the_ends() {
        let line = |dy: f64| (0..40).map(|i| Pt { x: i as f64 * 12.0, z: 0.0, y: 80.0 - dy * i as f64, acc: 10.0 }).collect::<Vec<_>>();
        let flat = meander(line(0.0), SEED, 3);
        assert_eq!(flat.len(), 40);
        assert_eq!((flat[0].z, flat[39].z), (0.0, 0.0));
        assert!(flat.iter().any(|p| p.z.abs() > 1.0));
        assert!(meander(line(20.0), SEED, 3).iter().all(|p| p.z.abs() < 1e-9));
    }

    /// Every river ends in the sea or runs into another river, none stops on land, and the grade never rises downstream.
    #[test]
    fn every_river_drains_to_the_sea() {
        let key = plate_at(SEED, 0.0, 0.0);
        let net = build(SEED, key, &mut land);
        assert!(net.segs.len() > 40, "{} segments", net.segs.len());
        let starts: Vec<(i64, i64)> = net.segs.iter().map(|s| ((s.x1 * 1000.0).round() as i64, (s.z1 * 1000.0).round() as i64)).collect();
        let sea_h = SEA_LEVEL as f64 - 1.0;
        let mut mouths = 0;
        for s in &net.segs {
            assert!(s.y2 <= s.y1 + 1e-9, "water rises: {} -> {}", s.y1, s.y2);
            assert!(s.w1 >= BASE_W && s.w2 <= MAX_W + 1e-9);
            let end = ((s.x2 * 1000.0).round() as i64, (s.z2 * 1000.0).round() as i64);
            if !starts.contains(&end) {
                // A mouth: its node is under the sea, or in a lake (which spills on in a river of its own).
                mouths += 1;
                let (gx, gz) = ((s.x2 / CELL as f64).round() as i32, (s.z2 / CELL as f64).round() as i32);
                // A mouth past the grid's edge is in no lake (the index would be negative).
                let (ix, iz) = (gx - net.g0.0, gz - net.g0.1);
                let on_grid = ix >= 0 && iz >= 0 && (ix as usize) < net.n.0 && (iz as usize) < net.n.1;
                let in_lake = on_grid && net.lake_of[ix as usize * net.n.1 + iz as usize] != u32::MAX;
                let h = land(gx, gz).0;
                assert!(in_lake || h < sea_h + 0.5, "river ends on land at ({}, {}), ground {h}", s.x2, s.z2);
            }
        }
        assert!(mouths > 0);
    }

    /// The carve puts water in a chunk with a river, holds it up, and leaves most chunks alone.
    #[test]
    fn carved_water_rests_on_ground() {
        let h = Hydro { seed: SEED, plates: Mutex::default() };
        let (mut water, mut touched, mut chunks) = (0, 0, 0);
        let (sx, sz) = plate_seed_point(SEED, plate_at(SEED, 0.0, 0.0).0, plate_at(SEED, 0.0, 0.0).1);
        for cx in (sx / 16.0) as i32 - 12..(sx / 16.0) as i32 + 12 {
            for cz in (sz / 16.0) as i32 - 12..(sz / 16.0) as i32 + 12 {
                let reg = h.region(cx, cz, &mut land);
                let mut b = vec![block::AIR; 16 * 16 * 128];
                for i in 0..256 {
                    let top = land((cx * 16 + (i / 16) as i32).div_euclid(CELL), (cz * 16 + (i % 16) as i32).div_euclid(CELL)).0 as usize;
                    for y in 0..top.min(120) {
                        b[(i / 16) << 11 | (i % 16) << 7 | y] = block::STONE;
                    }
                }
                let before = b.clone();
                reg.carve(cx, cz, &mut b, &vec![Biome::Plains; 256], &vec![0.8; 256]);
                chunks += 1;
                if b != before {
                    touched += 1;
                }
                for i in 0..256usize {
                    for y in 1..127usize {
                        let at = (i / 16) << 11 | (i % 16) << 7 | y;
                        if b[at] == block::WATER {
                            water += 1;
                            assert!(b[at - 1] != block::AIR, "water over air at chunk ({cx}, {cz}), y {y}");
                        }
                    }
                }
            }
        }
        assert!(water > 100 && touched > 0 && touched < chunks, "{water} water blocks, {touched} of {chunks} chunks touched");
    }

    /// A hand-made network: segments (x1, z1, x2, z2, half width) at grade 64.5.
    fn made(list: &[(f64, f64, f64, f64, f64)]) -> Network {
        made_at(64.5, list)
    }

    /// `made` at another grade.
    fn made_at(grade: f64, list: &[(f64, f64, f64, f64, f64)]) -> Network {
        let segs: Vec<Seg> = list
            .iter()
            .map(|&(x1, z1, x2, z2, w)| Seg { x1, z1, y1: grade, w1: w, x2, z2, y2: grade, w2: w, sp: 1.0, o1: 0.0, o2: 0.0 })
            .collect();
        Network { index: index_of(&segs), segs, lake_of: Vec::new(), lake_top: Vec::new(), g0: (0, 0), n: (0, 0) }
    }

    /// A river on a terrace is not overwritten by a lower river whose bank reaches it: the lower network comes first, as the plate's own
    /// does. Its bank used to take the upper channel's columns (cut or skipped), leaving no river or water standing on a cliff.
    #[test]
    fn a_lower_river_does_not_overwrite_a_higher_one() {
        let lower = Arc::new(made_at(66.5, &[(-40.0, 0.0, 60.0, 0.0, 4.0)]));
        let upper = Arc::new(made_at(75.5, &[(-40.0, 10.0, 60.0, 10.0, 1.5)]));
        let reg = Region { nets: vec![lower, upper] };
        let mut b = vec![block::AIR; 16 * 16 * 128];
        for x in 0..16usize {
            for z in 0..16usize {
                for y in 0..70 {
                    b[(x << 11) | (z << 7) | y] = block::STONE;
                }
            }
        }
        reg.carve(0, 0, &mut b, &vec![Biome::Plains; 256], &vec![0.8; 256]);
        let at = |x: usize, z: usize, y: usize| b[(x << 11) | (z << 7) | y];
        // The upper river's surface: grade 75.5, half width 1.5 -> inset 2 -> water block 73, on channel columns z 8..=11.
        for x in 1..15usize {
            for z in 8..=11usize {
                assert_eq!(at(x, z, 73), block::WATER, "the upper river is gone at ({x}, {z})");
            }
        }
        // And it is held: no water of that level has air beside it.
        for x in 1..15usize {
            for z in 1..15usize {
                if at(x, z, 73) == block::WATER {
                    for (nx, nz) in [(x - 1, z), (x + 1, z), (x, z - 1), (x, z + 1)] {
                        assert_ne!(at(nx, nz, 73), block::AIR, "water at ({x}, {z}) stands beside air at ({nx}, {nz})");
                    }
                }
            }
        }
    }

    /// A river through high ground has a valley side, not a wall: over a plateau 36 blocks above the water, the surface never steps by
    /// more than 3 blocks between neighbours across the river (the old 5 block band left a cliff of ~30 at its end), the water is held,
    /// and the plateau itself is untouched past the reach.
    #[test]
    fn a_river_in_high_ground_has_sloped_banks_not_walls() {
        let reg = Region { nets: vec![Arc::new(made(&[(-200.0, 0.0, 200.0, 0.0, 2.0)]))] };
        let (x0, z0, nx, nz) = (-16i32, -80i32, 2usize, 10usize); // chunks cx -1..1, cz -5..5
        let mut h = vec![0i32; (nx * 16) * (nz * 16)];
        for cx in 0..nx {
            for cz in 0..nz {
                let mut b = vec![block::AIR; 16 * 16 * 128];
                for i in 0..256usize {
                    for y in 0..100 {
                        b[((i / 16) << 11) | ((i % 16) << 7) | y] = block::STONE;
                    }
                }
                reg.carve(x0 / 16 + cx as i32, z0 / 16 + cz as i32, &mut b, &vec![Biome::Plains; 256], &vec![0.8; 256]);
                for x in 0..16usize {
                    for z in 0..16usize {
                        let col = (x << 11) | (z << 7);
                        let top = (0..128usize).rev().find(|&y| b[col + y] != block::AIR).unwrap() as i32;
                        h[(cx * 16 + x) * (nz * 16) + cz * 16 + z] = top;
                    }
                }
            }
        }
        let at = |x: usize, z: usize| h[x * (nz * 16) + z];
        let mut worst = 0;
        for x in 0..nx * 16 {
            for z in 1..nz * 16 {
                worst = worst.max((at(x, z) - at(x, z - 1)).abs());
            }
            // Water at the river (z 0), plateau far away (z -80 and 79 are 78+ blocks out).
            assert!(at(x, 80) <= 66, "no river at x {x}: {}", at(x, 80));
            assert_eq!(at(x, 0), 99, "the plateau is cut at the far edge");
            assert_eq!(at(x, nz * 16 - 1), 99, "the plateau is cut at the far edge");
        }
        assert!(worst <= 3, "a wall of {worst} blocks beside the river");
    }

    /// A river opens into the sea: water already there is left alone (the bank's berm used to dam the mouth with dirt and gravel), and
    /// the river itself is cut on the land.
    #[test]
    fn the_mouth_opens_into_the_sea() {
        let reg = Region { nets: vec![Arc::new(made(&[(-30.0, 8.0, 12.0, 8.0, 1.5)]))] };
        let mut b = vec![block::AIR; 16 * 16 * 128];
        for x in 0..16usize {
            for z in 0..16usize {
                let top = if x < 8 { 66 } else { 60 };
                for y in 0..top {
                    b[(x << 11) | (z << 7) | y] = block::STONE;
                }
                if x >= 8 {
                    for y in top..64 {
                        b[(x << 11) | (z << 7) | y] = block::WATER;
                    }
                }
            }
        }
        let before = b.clone();
        reg.carve(0, 0, &mut b, &vec![Biome::Plains; 256], &vec![0.8; 256]);
        for x in 8..16usize {
            for z in 0..16usize {
                for y in 0..128usize {
                    let i = (x << 11) | (z << 7) | y;
                    assert_eq!(b[i], before[i], "the sea changed at ({x}, {z}, {y})");
                }
            }
        }
        assert_eq!(b[(3 << 11) | (8 << 7) | 63], block::WATER, "the river is cut on the land");
    }

    /// A point inside a wide channel belongs to it even where a narrow tributary's end lies nearer.
    #[test]
    fn a_wide_channel_wins_at_a_confluence() {
        let net = made(&[(-50.0, 0.0, 50.0, 0.0, 6.0), (10.0, -30.0, 10.0, 0.0, 1.5)]);
        let p = net.plan(12.0, -4.0).expect("a river here");
        assert!(p.in_channel && (p.width - 6.0).abs() < 1e-9, "{p:?}");
    }

    /// A river's current points downstream (west here) at a speed in the mod's range.
    #[test]
    fn the_current_follows_the_river() {
        let net = build(SEED, plate_at(SEED, 0.0, 0.0), &mut land);
        let s = net.segs.iter().find(|s| s.x2 - s.x1 < -4.0 && s.w1 > 1.0).copied().expect("a river going west");
        let c = net.flow_at(((s.x1 + s.x2) / 2.0) as f32, ((s.z1 + s.z2) / 2.0) as f32).expect("water at the middle of a piece");
        assert!(c[0] < 0.0 && (0.3..=3.1).contains(&c[0].hypot(c[1])), "{c:?}");
    }
}
