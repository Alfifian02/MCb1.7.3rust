//! Aquifers and lava: `Aquifer.NoiseBasedAquifer` of 1.18+ (the game's water and lava inside the ground), on the Cascades bundle.
//!
//! What it does. Every block of the chunk whose final density is not above 0 is asked what fills it: air, water, lava, or (where an
//! aquifer's barrier presses on it) stone. The game places 16 x 12 x 16 jittered aquifer centres (`positional random at (cx, cy, cz)`),
//! finds the three nearest to the block, gives each a fluid level (`computeFluid`: the sea above the land, a noise-picked level
//! `j * 40 + 20 + quantize(spread * 10, 3)` below it, none where `floodedness` says dry, lava where the level is deep and the lava noise is
//! high) and lets the pressure between the nearest ones (`calculatePressure`, the barrier noise) turn the cells on a border into stone.
//! The result: caves under the land are mostly dry air with lakes of water in some of them, flooded caves near rivers and the coast,
//! lava lakes at the bottom, and the open sea and its floor still full of water.
//!
//! Frames. The functions and noises are the pack's, in the pack's frame (sea at y = 63). A block's pack y is `index + min_y`. The world
//! of Cascades starts at pack y = -1 (`Config::cascades`): the pack's lowest 63 blocks, where the game puts its lava (`y < -54` globally,
//! aquifer levels `<= -10`), do not exist. So below the sea the aquifer works on a squeezed y: the 64 blocks from the floor to the sea
//! stand for the game's 127 from -64 to 63 (`Ctx::game_y`, a linear map that is the identity at and above the sea; the surfaces it
//! compares with go through the same map, so a block keeps its place relative to the ground). The game's floor of lava and its lava lakes
//! (levels <= -10) end up in the lowest ~26 blocks, its lakes of water between them and the sea. Above the sea nothing changes. A world
//! whose floor is the pack's own (-64) is not squeezed. UNVERIFIED: a choice of this port, not of the game.
//!
//! UNVERIFIED (written from memory of the 1.21 sources, no vanilla jar was at hand): the whole of `NoiseBasedAquifer` (the 16 x 12 x 16
//! grid, `similarity`, `calculatePressure`, the surface sampling offsets, the `-10` lava threshold, the global lava below -54), the noise
//! parameters of the four aquifer noises (`tools/cascades.py`), `Mth.getSeed`, the aquifer's `fromHashOf("minecraft:aquifer")` fork.
//! Not ported: `shouldScheduleFluidUpdate` (the game wakes the fluid at some borders; here the water is simply where it is placed),
//! the ore veins.

use super::{Grid, Memo, Tectonic, Xoro};
use std::collections::HashMap;

/// What a block is after the aquifer.
pub const STONE: u8 = 0;
pub const AIR: u8 = 1;
pub const WATER: u8 = 2;
pub const LAVA: u8 = 3;
/// Ore veins (`Veminer`): the game's copper and iron veins. Copper has no block in Beta: the caller picks one. (Granite / tuff, the veins'
/// filler, is plain `STONE` here: Beta has neither.)
pub const COPPER_ORE: u8 = 4;
pub const IRON_ORE: u8 = 5;
pub const RAW_COPPER: u8 = 6;
pub const RAW_IRON: u8 = 7;

/// The functions of the bundle's `aq.fns`, in the order of `Ids::f`.
pub(crate) const FNS: [&str; 11] = [
    "surface_density", "surface_upper", "barrier", "lava", "fluid_level_floodedness", "fluid_level_spread", "exclusion", "vein_toggle", "vein_a", "vein_b", "vein_gap",
];
const SURF: usize = 0;
const UPPER: usize = 1;
const BARRIER: usize = 2;
const LAVA_NOISE: usize = 3;
const FLOOD: usize = 4;
const SPREAD: usize = 5;
const EXCL: usize = 6;
const VEIN_TOGGLE: usize = 7;
const VEIN_A: usize = 8;
const VEIN_B: usize = 9;
const VEIN_GAP: usize = 10;

/// The pack's sea level, and the level of its global lava (`NoiseBasedChunkGenerator.createFluidPicker`: lava below `min(-54, sea)`).
const SEA: i32 = 63;
const GLOBAL_LAVA: i32 = -54;
/// `DimensionType.WAY_BELOW_MIN_Y`: a fluid level that never holds anything.
const WAY_BELOW: i32 = -32512;
/// `SURFACE_SAMPLING_OFFSETS_IN_CHUNKS`: the columns, in chunks from the aquifer centre, whose surface decides its fluid.
const OFFS: [(i32, i32); 5] = [(0, 0), (-2, -1), (-1, -1), (0, -1), (1, -1)];

/// The bundle's aquifer data, resolved to node ids (after `simplify`).
pub(crate) struct Ids {
    /// The positional factory of `fromHashOf("minecraft:aquifer").forkPositional()`.
    pub fac: (u64, u64),
    /// The positional factory of `fromHashOf("minecraft:ore").forkPositional()`.
    pub ore_fac: (u64, u64),
    pub f: [u32; FNS.len()],
    /// `find_top_surface`'s lower bound and cell height.
    pub lower: i32,
    pub cell: i32,
}

/// `Aquifer.FluidStatus`: the fluid fills every y below `level`.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Status {
    level: i32,
    lava: bool,
}

impl Status {
    fn at(self, y: i32) -> u8 {
        if y < self.level {
            if self.lava { LAVA } else { WATER }
        } else {
            AIR
        }
    }
}

/// The global fluid picker: lava deep down, the sea otherwise.
fn global(y: i32) -> Status {
    if y < GLOBAL_LAVA { Status { level: GLOBAL_LAVA, lava: true } } else { Status { level: SEA, lava: false } }
}

fn floor_div(a: i32, b: i32) -> i32 {
    a.div_euclid(b)
}

/// `Mth.getSeed(x, y, z)`.
fn get_seed(x: i32, y: i32, z: i32) -> i64 {
    let l = (x.wrapping_mul(3129871) as i64) ^ (z as i64).wrapping_mul(116129781) ^ (y as i64);
    l.wrapping_mul(l).wrapping_mul(42317861).wrapping_add(l.wrapping_mul(11)) >> 16
}

/// `similarity`: 1 for equal squared distances, falling to 0 at 25 apart.
fn similarity(a: i32, b: i32) -> f64 {
    1.0 - (b - a) as f64 / 25.0
}

struct Ctx<'a> {
    t: &'a Tectonic,
    ids: &'a Ids,
    g: Grid,
    m: Memo,
    /// How much a block below the sea is stretched for the aquifer: `127 / (sea - floor)` (1 when the floor is the pack's -64).
    squeeze: f64,
    /// The preliminary surface by quart column, in game y.
    surf: HashMap<(i32, i32), i32>,
    /// Aquifer centre (position, status once asked) by its grid index.
    lat: HashMap<(i32, i32, i32), ([i32; 3], Option<Status>)>,
}

impl Ctx<'_> {
    /// A function of the bundle at a point: both memo classes are fresh.
    fn ev(&mut self, f: usize, x: i32, y: i32, z: i32) -> f64 {
        self.m.column();
        self.m.point();
        self.t.ev(self.ids.f[f], x, y, z, &mut self.m, &self.g)
    }

    /// The game's y of a block of pack y `y`: below the sea the distance to the sea is stretched (see the module doc).
    fn game_y(&self, y: i32) -> i32 {
        if y >= SEA || self.squeeze == 1.0 { y } else { SEA - ((SEA - y) as f64 * self.squeeze).round() as i32 }
    }

    /// `NoiseChunk.preliminarySurfaceLevel` at a quart column: `find_top_surface` of the pack's `surface_level` (the density without
    /// caves, scanned down from its upper bound in `cell` steps), floored, in game y.
    fn prelim(&mut self, qx: i32, qz: i32) -> i32 {
        if let Some(&v) = self.surf.get(&(qx, qz)) {
            return v;
        }
        let (x, z) = (qx << 2, qz << 2);
        self.m.column();
        self.m.point();
        let up = self.t.ev(self.ids.f[UPPER], x, 0, z, &mut self.m, &self.g);
        let (cell, lower) = (self.ids.cell, self.ids.lower);
        let top = (up / cell as f64).floor() as i32 * cell;
        let mut r = lower;
        if top > lower {
            let mut y = top;
            while y >= lower {
                self.m.point();
                if self.t.ev(self.ids.f[SURF], x, y, z, &mut self.m, &self.g) > 0.0 {
                    r = y;
                    break;
                }
                y -= cell;
            }
        }
        let r = self.game_y(r);
        self.surf.insert((qx, qz), r);
        r
    }

    fn surface(&mut self, x: i32, z: i32) -> i32 {
        self.prelim(x >> 2, z >> 2)
    }

    /// `NoiseBasedAquifer.computeFluid`: the fluid of the aquifer centre (x, y, z), game coordinates.
    fn compute_fluid(&mut self, x: i32, y: i32, z: i32) -> Status {
        let glob = global(y);
        let (max_y, min_y) = (y + 12, y - 12);
        let mut min_surface = i32::MAX;
        let mut surface_below = false;
        for (i, &(dx, dz)) in OFFS.iter().enumerate() {
            let (sx, sz) = (x + dx * 16, z + dz * 16);
            let plus = self.surface(sx, sz) + 8;
            let first = i == 0;
            if first && min_y > plus {
                return glob;
            }
            let above = max_y > plus;
            if above || first {
                let g = global(plus);
                if g.at(plus) != AIR {
                    if first {
                        surface_below = true;
                    }
                    if above {
                        return g;
                    }
                }
            }
            min_surface = min_surface.min(plus - 8);
        }
        let level = self.surface_level(x, y, z, glob, min_surface, surface_below);
        Status { level, lava: self.is_lava(x, y, z, glob, level) }
    }

    /// `computeSurfaceLevel`: the level of the fluid at an aquifer centre (`WAY_BELOW` for none).
    fn surface_level(&mut self, x: i32, y: i32, z: i32, glob: Status, min_surface: i32, surface_below: bool) -> i32 {
        // The deep dark's rule of the game (`erosion < -0.225 && depth > 0.9`) is the pack's `exclusion` above 0: no fluid there.
        let (d, e);
        if self.ev(EXCL, x, y, z) > 0.0 {
            d = -1.0;
            e = -1.0;
        } else {
            let i = (min_surface + 8 - y) as f64;
            let f = if surface_below { 1.0 - (i / 64.0).clamp(0.0, 1.0) } else { 0.0 };
            let g = self.ev(FLOOD, x, y, z).clamp(-1.0, 1.0);
            // Mth.map(f, 1, 0, a, b) = a + (1 - f) * (b - a)
            let h = -0.3 + (1.0 - f) * 1.1;
            let k = -0.8 + (1.0 - f) * 1.2;
            d = g - k;
            e = g - h;
        }
        if e > 0.0 {
            glob.level
        } else if d > 0.0 {
            self.randomized_level(x, y, z, min_surface)
        } else {
            WAY_BELOW
        }
    }

    /// `computeRandomizedFluidSurfaceLevel`: the level of a 16 x 40 x 16 block, a noise step of 3 around its middle, under the land.
    fn randomized_level(&mut self, x: i32, y: i32, z: i32, min_surface: i32) -> i32 {
        let (i, j, k) = (floor_div(x, 16), floor_div(y, 40), floor_div(z, 16));
        let mid = j * 40 + 20;
        // The spread noise is read at the block's grid cell, not at the block (as the game does).
        let d = self.ev(SPREAD, i, j, k) * 10.0;
        let m = ((d / 3.0).floor() * 3.0) as i32;
        min_surface.min(mid + m)
    }

    /// `computeFluidType`: a deep level (<= -10) gets lava where the lava noise of its 64 x 40 x 64 block is strong.
    fn is_lava(&mut self, x: i32, y: i32, z: i32, glob: Status, level: i32) -> bool {
        if glob.lava {
            return true;
        }
        if level <= -10 && level != WAY_BELOW {
            let (i, j, k) = (floor_div(x, 64), floor_div(y, 40), floor_div(z, 64));
            return self.ev(LAVA_NOISE, i, j, k).abs() > 0.3;
        }
        false
    }

    /// The aquifer centre of grid cell (cx, cy, cz) and its fluid, cached.
    fn centre(&mut self, cx: i32, cy: i32, cz: i32) -> ([i32; 3], Status) {
        let key = (cx, cy, cz);
        let (pos, st) = match self.lat.get(&key) {
            Some(&v) => v,
            None => {
                let mut r = Xoro::new((get_seed(cx, cy, cz) as u64) ^ self.ids.fac.0, self.ids.fac.1);
                let x = cx * 16 + r.next_int(10) as i32;
                let y = cy * 12 + r.next_int(9) as i32;
                let z = cz * 16 + r.next_int(10) as i32;
                ([x, y, z], None)
            }
        };
        if let Some(s) = st {
            return (pos, s);
        }
        let s = self.compute_fluid(pos[0], pos[1], pos[2]);
        self.lat.insert(key, (pos, Some(s)));
        (pos, s)
    }

    /// `calculatePressure`.
    fn pressure(&mut self, x: i32, y: i32, z: i32, barrier: &mut Option<f64>, a: Status, b: Status) -> f64 {
        let (fa, fb) = (a.at(y), b.at(y));
        if (fa == LAVA && fb == WATER) || (fa == WATER && fb == LAVA) {
            return 2.0;
        }
        let diff = (a.level - b.level).abs();
        if diff == 0 {
            return 0.0;
        }
        let mid = 0.5 * (a.level + b.level) as f64;
        let d = y as f64 + 0.5 - mid;
        let half = diff as f64 / 2.0;
        let q = half - d.abs();
        let c = if d > 0.0 {
            if q > 0.0 { q / 1.5 } else { q / 2.5 }
        } else {
            let e = 3.0 + q;
            if e > 0.0 { e / 3.0 } else { e / 10.0 }
        };
        let mut g = 0.0;
        if (-2.0..=2.0).contains(&c) {
            g = match *barrier {
                Some(v) => v,
                None => {
                    let v = self.ev(BARRIER, x, y, z);
                    *barrier = Some(v);
                    v
                }
            };
        }
        2.0 * (g + c)
    }

    /// `OreVeinifier`: the ore (or raw ore block) of a vein at (x, y, z) in game y, `STONE` for none or for the filler. UNVERIFIED, from
    /// memory; the toggle and ridged noises are read at the block, not interpolated over 4 x 8 cells as the game does.
    fn vein(&mut self, x: i32, y: i32, z: i32) -> u8 {
        if !(-60..=50).contains(&y) {
            return STONE;
        }
        let d = self.ev(VEIN_TOGGLE, x, y, z);
        let copper = d > 0.0;
        let (lo, hi) = if copper { (0, 50) } else { (-60, -8) };
        let e = d.abs();
        let (j, k) = (hi - y, y - lo);
        if j < 0 || k < 0 {
            return STONE;
        }
        let f = -0.2 + (j.min(k) as f64 / 20.0).clamp(0.0, 1.0) * 0.2;
        if e + f < 0.4f32 as f64 {
            return STONE;
        }
        let mut r = Xoro::new((get_seed(x, y, z) as u64) ^ self.ids.ore_fac.0, self.ids.ore_fac.1);
        let mut next_float = move || (r.next_long() >> 40) as f32 * 5.9604645e-8;
        if next_float() > 0.7 {
            return STONE;
        }
        let ridged = -0.08 + self.ev(VEIN_A, x, y, z).abs().max(self.ev(VEIN_B, x, y, z).abs());
        if ridged >= 0.0 {
            return STONE;
        }
        // Mth.clampedMap(e, 0.4, 0.6, 0.1, 0.3)
        let g = 0.1 + ((e - 0.4) / 0.2).clamp(0.0, 1.0) * 0.2;
        if (next_float() as f64) < g && self.ev(VEIN_GAP, x, y, z) > -0.3 {
            let raw = next_float() < 0.02;
            return match (copper, raw) {
                (true, false) => COPPER_ORE,
                (true, true) => RAW_COPPER,
                (false, false) => IRON_ORE,
                (false, true) => RAW_IRON,
            };
        }
        STONE
    }

    /// `computeSubstance`, with the game's `shouldScheduleFluidUpdate` in the second place: a fluid block near the border of two aquifers
    /// (the game schedules its fluid tick at post-processing, so it flows into what lies beside it).
    fn substance(&mut self, x: i32, y: i32, z: i32, density: f64) -> (u8, bool) {
        let (v, wake) = self.substance0(x, y, z, density);
        (v, wake && (v == WATER || v == LAVA))
    }

    fn substance0(&mut self, x: i32, y: i32, z: i32, density: f64) -> (u8, bool) {
        if density > 0.0 {
            return (STONE, false);
        }
        let glob = global(y);
        if glob.at(y) == LAVA {
            return (LAVA, false);
        }
        let (gx, gy, gz) = (floor_div(x - 5, 16), floor_div(y + 1, 12), floor_div(z - 5, 16));
        // The three nearest aquifer centres (the game keeps a fourth to order them; it is never read).
        let mut best = [(i32::MAX, (0, 0, 0)); 3];
        for dx in 0..=1 {
            for dy in -1..=1 {
                for dz in 0..=1 {
                    let key = (gx + dx, gy + dy, gz + dz);
                    let (p, _) = self.centre(key.0, key.1, key.2);
                    let dist = (p[0] - x).pow(2) + (p[1] - y).pow(2) + (p[2] - z).pow(2);
                    if best[0].0 >= dist {
                        best[2] = best[1];
                        best[1] = best[0];
                        best[0] = (dist, key);
                    } else if best[1].0 >= dist {
                        best[2] = best[1];
                        best[1] = (dist, key);
                    } else if best[2].0 >= dist {
                        best[2] = (dist, key);
                    }
                }
            }
        }
        let st = |c: &mut Self, k: (i32, i32, i32)| c.centre(k.0, k.1, k.2).1;
        let (d1, d2, d3) = (best[0].0, best[1].0, best[2].0);
        let s1 = st(self, best[0].1);
        let sim12 = similarity(d1, d2);
        let fluid = s1.at(y);
        if sim12 <= 0.0 {
            // FLOWING_UPDATE_SIMULARITY = similarity(100, 144)
            return (fluid, sim12 >= similarity(100, 144));
        }
        if fluid == WATER && global(y - 1).at(y - 1) == LAVA {
            return (fluid, true);
        }
        let mut barrier = None;
        let s2 = st(self, best[1].1);
        let p12 = sim12 * self.pressure(x, y, z, &mut barrier, s1, s2);
        if density + p12 > 0.0 {
            return (STONE, false);
        }
        let s3 = st(self, best[2].1);
        let sim13 = similarity(d1, d3);
        if sim13 > 0.0 {
            let p13 = sim12 * sim13 * self.pressure(x, y, z, &mut barrier, s1, s3);
            if density + p13 > 0.0 {
                return (STONE, false);
            }
        }
        let sim23 = similarity(d2, d3);
        if sim23 > 0.0 {
            let p23 = sim12 * sim23 * self.pressure(x, y, z, &mut barrier, s2, s3);
            if density + p23 > 0.0 {
                return (STONE, false);
            }
        }
        (fluid, true)
    }
}

impl Tectonic {
    /// Does this bundle have aquifers (Cascades'), so that `fluids` has an answer?
    pub fn has_aquifers(&self) -> bool {
        self.aq.is_some()
    }

    /// What fills every block of chunk (cx, cz), given its `chunk` densities: `STONE` where the density is above 0 or an aquifer's barrier
    /// presses, else `AIR`, `WATER` or `LAVA`. Same index as `chunk`. `None` for a bundle without aquifers (Tectonic's), whose caller
    /// fills everything under the sea with water.
    pub fn fluids(&self, cx: i32, cz: i32, density: &[f32]) -> Option<Vec<u8>> {
        self.fluids_wake(cx, cz, density).map(|(v, _)| v)
    }

    /// `fluids`, and the indices (as in `chunk`) of the water and lava blocks the game would schedule a fluid tick for
    /// (`shouldScheduleFluidUpdate`).
    pub fn fluids_wake(&self, cx: i32, cz: i32, density: &[f32]) -> Option<(Vec<u8>, Vec<u32>)> {
        let ids = self.aq.as_ref()?;
        let h = self.height;
        assert_eq!(density.len(), 256 * h, "fluids: the densities of a chunk");
        let mut c = Ctx {
            t: self,
            ids,
            g: Grid { on: false, x0: 0, z0: 0, min_y: self.min_y, ny: 2, tabs: Vec::new() },
            m: Memo::new(self.nodes.len()),
            squeeze: if self.min_y <= -64 { 1.0 } else { (SEA + 64) as f64 / (SEA - self.min_y) as f64 },
            surf: HashMap::new(),
            lat: HashMap::new(),
        };
        let (x0, z0) = (cx * 16, cz * 16);
        // The aquifer centres of this chunk lie in x0 - 16 .. x0 + 25, z0 - 16 .. z0 + 25 and read the surface 2 chunks west and one north
        // of them: the highest of those columns says from where up every centre sees the open sky and the block is the sea's (air).
        let mut top = i32::MIN;
        for qx in ((x0 - 48) >> 2)..=((x0 + 41) >> 2) {
            for qz in ((z0 - 32) >> 2)..=((z0 + 25) >> 2) {
                top = top.max(c.prelim(qx, qz));
            }
        }
        // A centre sits at most 23 blocks below the block it serves and looks 12 down and 8 above the surface.
        let sky = top.saturating_add(44);
        let mut out = vec![STONE; 256 * h];
        let mut wake = Vec::new();
        for x in 0..16usize {
            for z in 0..16usize {
                for y in 0..h {
                    let i = (x * 16 + z) * h + y;
                    let d = density[i] as f64;
                    let gy = c.game_y(y as i32 + self.min_y);
                    let (wx, wz) = (x0 + x as i32, z0 + z as i32);
                    if d > 0.0 {
                        out[i] = c.vein(wx, gy, wz);
                        continue;
                    }
                    let (v, w) = if gy > sky { (global(gy).at(gy), false) } else { c.substance(wx, gy, wz, d) };
                    out[i] = if v == STONE { c.vein(wx, gy, wz) } else { v };
                    if w {
                        wake.push(i as u32);
                    }
                }
            }
        }
        Some((out, wake))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Mth.getSeed` for a few points, worked by hand from the Java (`(long)(x * 3129871) ^ (long)z * 116129781L ^ (long)y`, then
    /// `l * l * 42317861L + l * 11L`, `>> 16`): the origin is 0 and a point with only y set squares it.
    #[test]
    fn the_position_seed_follows_the_java() {
        assert_eq!(get_seed(0, 0, 0), 0);
        // y = 1: l = 1 -> 42317861 + 11 = 42317872 >> 16 = 645
        assert_eq!(get_seed(0, 1, 0), 645);
        // negatives: x = -1 -> (long)(-3129871); the result is an arithmetic shift of a negative or positive long, so only check it is stable
        assert_eq!(get_seed(-1, 2, -3), get_seed(-1, 2, -3));
        assert_ne!(get_seed(1, 0, 0), get_seed(0, 0, 1));
    }

    #[test]
    fn similarity_is_one_at_equal_and_zero_at_25() {
        assert_eq!(similarity(10, 10), 1.0);
        assert_eq!(similarity(10, 35), 0.0);
        assert!(similarity(10, 40) < 0.0);
    }

    #[test]
    fn a_status_fills_below_its_level() {
        let w = Status { level: 63, lava: false };
        assert_eq!((w.at(62), w.at(63)), (WATER, AIR));
        let l = Status { level: -54, lava: true };
        assert_eq!((l.at(-55), l.at(-54)), (LAVA, AIR));
        assert_eq!(global(-55).at(-55), LAVA);
        assert_eq!(global(10).at(10), WATER);
        assert_eq!(global(63).at(63), AIR);
    }

    /// One chunk, twice: the same answer, solid where the density is, and the open sky above the land is air.
    #[test]
    fn a_chunk_is_deterministic_and_keeps_the_solids() {
        let t = Tectonic::cascades(1);
        assert!(t.has_aquifers());
        let d = t.chunk(5, -3);
        let a = t.fluids(5, -3, &d).unwrap();
        let b = t.fluids(5, -3, &d).unwrap();
        assert_eq!(a, b);
        for (i, &v) in d.iter().enumerate() {
            if v > 0.0 {
                assert_eq!(a[i], STONE);
            }
        }
        let h = t.height();
        for col in 0..256 {
            assert_eq!(a[col * h + h - 1], AIR, "the top of the world is open air");
        }
    }

    /// Tectonic's own bundle has none.
    #[test]
    fn the_tectonic_bundle_has_no_aquifers() {
        assert!(!Tectonic::new(1, super::super::Config::compact(128, 64)).has_aquifers());
    }

    /// The numbers of the world: over a spread of chunks of 3 seeds, caves under the land keep air below the sea (they are not all
    /// flooded), some hold water, lava appears only in the lowest rows, the sea stays full of water, and the fraction of air under the
    /// sea inside the ground is printed (`cargo test -p tectonic aquifer_census -- --nocapture`).
    #[test]
    fn aquifer_census() {
        let (mut dry, mut wet, mut lava, mut sea_air, mut sea_cells) = (0u64, 0u64, 0u64, 0u64, 0u64);
        let (mut lava_lo, mut lava_hi) = (i32::MAX, i32::MIN);
        let mut barrier_stone = 0u64;
        for seed in [1i64, 7, 424242] {
            let t = Tectonic::cascades(seed);
            let h = t.height();
            for i in 0..8 {
                for j in 0..8 {
                    let (cx, cz) = (i * 11 - 44, j * 11 - 44);
                    let d = t.chunk(cx, cz);
                    let f = t.fluids(cx, cz, &d).unwrap();
                    for col in 0..256 {
                        // The top solid block of the column (by density) separates the sea above it from the ground below.
                        let top = (0..h).rev().find(|&y| d[col * h + y] > 0.0);
                        for y in 0..h.min(64) {
                            let v = f[col * h + y];
                            let underground = top.is_some_and(|tp| y < tp);
                            match v {
                                LAVA => {
                                    lava += 1;
                                    lava_lo = lava_lo.min(y as i32);
                                    lava_hi = lava_hi.max(y as i32);
                                }
                                AIR if underground => dry += 1,
                                WATER if underground => wet += 1,
                                STONE if d[col * h + y] <= 0.0 => barrier_stone += 1,
                                _ => {}
                            }
                            if !underground {
                                sea_cells += 1;
                                if v == AIR {
                                    sea_air += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        println!("under the sea level, inside the ground: dry air {dry}, water {wet}, lava {lava} (y {lava_lo}..{lava_hi}), barrier stone {barrier_stone}; open sea column cells {sea_cells}, air in them {sea_air}");
        assert!(dry > 0, "some caves stay dry");
        assert!(wet > 0, "some caves hold water");
        if lava > 0 {
            assert!(lava_hi < 60, "lava stays in the lowest rows, not at the surface: {lava_hi}");
        }
    }

    /// Veins and wake-ups exist, in sane amounts: ore only where the game has it (game y -60 .. 50), wake-ups only on water / lava.
    #[test]
    fn veins_and_wakeups_exist() {
        let t = Tectonic::cascades(1);
        let h = t.height();
        let (mut ores, mut stone, mut wakes, mut fluid) = ([0usize; 8], 0usize, 0usize, 0usize);
        for i in 0..6 {
            for j in 0..6 {
                let (cx, cz) = (i * 7 - 21, j * 7 - 21);
                let d = t.chunk(cx, cz);
                let (f, w) = t.fluids_wake(cx, cz, &d).unwrap();
                for &v in &f {
                    ores[v as usize] += 1;
                    if v == STONE {
                        stone += 1;
                    }
                    if v == WATER || v == LAVA {
                        fluid += 1;
                    }
                }
                for &k in &w {
                    assert!(f[k as usize] == WATER || f[k as usize] == LAVA);
                }
                wakes += w.len();
                let _ = h;
            }
        }
        println!("VEINS copper {} iron {} raw copper {} raw iron {} stone {stone}; fluid {fluid}, woken {wakes}", ores[4], ores[5], ores[6], ores[7]);
        assert!(ores[4] + ores[5] > 0, "some veins");
        assert!(wakes < fluid, "not every fluid block is woken");
    }
}
