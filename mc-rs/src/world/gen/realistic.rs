//! Second world generator, "Realistic": terrain shaped the way Realistic Terrain Generation (Team RTG, GPL-3.0) shapes it,
//! on the 128-high b1.7.3 column. Ported from RTG: `TerrainBase.blendedHillHeight` and `hills` (the hill curve and the
//! two-scale rolling noise), the per-biome base height blended over its neighbours (`ChunkLandscape` weighting, here a
//! box mean), a mountain mask with ridges, ocean basins and river valleys carved toward the sea floor
//! (`TerrainBase.getRiverStrength`). Not RTG: its OpenSimplex and 1.12 biomes (the Beta climate biomes and the
//! b1.7.3 simplex stand in) and its 256-high numbers (scaled to sea level 64 of 128). The surface skin, caves and
//! populate are the vanilla passes of `overworld.rs` / `populate.rs`, run on this terrain.
//!
//! ponytail: the blend is an O(17 x 17) box mean per column (prefix sums if a profile ever asks), and the river bends are
//! a second noise added to the first, not RTG's jittered cellular noise.

use crate::world::biome::Biome;
use crate::world::gen::noise::{JavaRandom, SimplexOctaves};
use crate::world::gen::overworld::{block, H, SEA_LEVEL};

/// Biome grid the blend reads: the chunk plus 16 blocks each side, x-major (`x * GRID + z`).
pub const GRID: usize = 48;
/// Blend radius in blocks (a box of `2 * BLUR + 1` columns).
const BLUR: i32 = 8;

pub struct Realistic {
    hill: SimplexOctaves,
    bump: SimplexOctaves,
    cont: SimplexOctaves,
    mount: SimplexOctaves,
    ridge: SimplexOctaves,
    river: SimplexOctaves,
    bend: SimplexOctaves,
}

/// `TerrainBase.blendedHillHeight`: -1..1 in, 0..1 out, flat near 0 so hills do not leave dead valleys.
fn blended_hill_height(s: f64) -> f64 {
    let r = s.clamp(-1.0, 1.0) + 1.0;
    ((r * r * r + 10.0).powf(1.0 / 3.0) / 0.46631 - 4.62021).clamp(0.0, 1.0)
}

/// `blendedHillHeight(simplex, turnAt)`: the zero point moved to `turn_at`.
fn hill_turn(s: f64, turn_at: f64) -> f64 {
    blended_hill_height(1.0 - (1.0 - s) / (1.0 - turn_at))
}

fn smooth(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// (base height, hill strength) of a biome's ground. RTG's per-biome terrain classes, reduced to the ten Beta climates.
fn params(b: Biome) -> (f64, f64) {
    match b {
        Biome::Desert => (66.0, 7.0),
        Biome::Plains => (66.0, 6.0),
        Biome::Savanna => (67.0, 9.0),
        Biome::Shrubland => (67.0, 8.0),
        Biome::Forest => (68.0, 15.0),
        Biome::SeasonalForest => (69.0, 17.0),
        Biome::Rainforest => (68.0, 19.0),
        Biome::Swamp => (63.5, 2.0),
        Biome::Taiga => (70.0, 24.0),
        Biome::Tundra => (70.0, 13.0),
    }
}

/// One 16 x 16 field of a single simplex octave, x-major, in -1..1 with features about `scale` blocks wide.
/// `generate` halves the step by 1.5 and scales a lone octave by 0.55: both undone here.
fn layer(o: &SimplexOctaves, x0: i32, z0: i32, scale: f64) -> Vec<f64> {
    let d = 1.5 / scale;
    o.generate(x0 as f64, z0 as f64, 16, 16, d, d, 1.0).into_iter().map(|v| (v / 0.55).clamp(-1.0, 1.0)).collect()
}

impl Realistic {
    pub fn new(seed: i64) -> Self {
        let mut r = JavaRandom::new(seed ^ 0x5254_4721);
        let mut o = || SimplexOctaves::new(&mut r, 1);
        Self { hill: o(), bump: o(), cont: o(), mount: o(), ridge: o(), river: o(), bend: o() }
    }

    /// Stone up to the terrain height, water (ice when cold) up to sea level, air above: the same raw column the vanilla density
    /// pass leaves for `replace_blocks_for_biome`. `grid` is `GRID` x `GRID` biomes from (cx * 16 - 16, cz * 16 - 16),
    /// `temps` the chunk's own 16 x 16 temperatures (`x * 16 + z`).
    pub fn fill(&self, cx: i32, cz: i32, blocks: &mut [u8], grid: &[Biome], temps: &[f64]) {
        let (x0, z0) = (cx * 16, cz * 16);
        let hill = layer(&self.hill, x0, z0, 150.0);
        let bump = layer(&self.bump, x0, z0, 55.0);
        let cont = layer(&self.cont, x0, z0, 520.0);
        let mount = layer(&self.mount, x0, z0, 330.0);
        let ridge = layer(&self.ridge, x0, z0, 75.0);
        let river = layer(&self.river, x0, z0, 260.0);
        let bend = layer(&self.bend, x0, z0, 85.0);
        let p: Vec<(f64, f64)> = grid.iter().map(|&b| params(b)).collect();
        let sea = SEA_LEVEL as f64;
        for x in 0..16usize {
            for z in 0..16usize {
                let i = x * 16 + z;
                // The neighbourhood's mean ground: biome borders melt into each other instead of stepping.
                let (mut base, mut amp) = (0.0, 0.0);
                for dx in -BLUR..=BLUR {
                    for dz in -BLUR..=BLUR {
                        let (b, a) = p[((x as i32 + 16 + dx) as usize) * GRID + (z as i32 + 16 + dz) as usize];
                        base += b;
                        amp += a;
                    }
                }
                let n = ((2 * BLUR + 1) * (2 * BLUR + 1)) as f64;
                let (base, amp) = (base / n, amp / n);
                // TerrainBase.hills: a broad hill noise plus a finer one that only rises on top of the broad one.
                let m = hill_turn(hill[i], 0.2);
                let mut sm = hill_turn(bump[i], 0.2);
                sm *= sm * m;
                // Mountains: a wide mask, sharp ridges inside it (RTG's mountains with passes); the rolling hills fade under them
                // so the highest peak (taiga 70 + 13 + 36) stays below the 124 cap and nothing is clipped flat.
                let ocean = smooth((-0.15 - cont[i]) / 0.35);
                let mask = smooth((mount[i] - 0.3) / 0.45) * (1.0 - ocean);
                let mut h = base + amp * (m + sm / 3.0) * (1.0 - 0.6 * mask);
                h += mask * (10.0 + 26.0 * (1.0 - ridge[i].abs()).powi(2));
                // Ocean basins.
                h += (40.0 + 8.0 * bump[i] - h) * ocean;
                // Rivers: where the bent noise crosses zero the ground is pulled down below the sea.
                let rs = smooth(1.0 - (river[i] + 0.3 * bend[i]).abs() / 0.045) * (1.0 - ocean) * (1.0 - mask);
                if h > sea - 5.0 {
                    h += (sea - 5.0 - h) * rs;
                }
                let top = (h as i32).clamp(1, H as i32 - 4);
                let cold = temps[i] < 0.5;
                let col = &mut blocks[(x << 11) | (z << 7)..][..H];
                for (y, c) in col.iter_mut().enumerate() {
                    let y = y as i32;
                    *c = if y <= top {
                        block::STONE
                    } else if y < SEA_LEVEL {
                        if cold && y >= SEA_LEVEL - 1 { block::ICE } else { block::WATER }
                    } else {
                        block::AIR
                    };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::world::gen::chunk_manager::WorldChunkManager;
    use crate::world::gen::overworld::{block, OverworldGenerator};

    /// The realistic generator is deterministic, has real relief (not flat), has water, and `OverworldGenerator::generate` fills
    /// whole chunks with it.
    #[test]
    fn realistic_world_is_deterministic_hilly_and_wet() {
        let mut g = OverworldGenerator::new(7).realistic();
        let mut cm = WorldChunkManager::new(7);
        let (mut lo, mut hi, mut water) = (i32::MAX, i32::MIN, false);
        let mut first = Vec::new();
        for cz in -3..3 {
            for cx in -3..3 {
                let b = g.generate(cx, cz, &mut cm);
                assert_eq!(b.len(), 16 * 128 * 16);
                if (cx, cz) == (0, 0) {
                    first = b.clone();
                }
                water |= b.contains(&block::WATER);
                for x in 0..16 {
                    for z in 0..16 {
                        let t = OverworldGenerator::top_block(&b, x, z, 16);
                        lo = lo.min(t);
                        hi = hi.max(t);
                    }
                }
            }
        }
        assert!(hi - lo >= 12, "relief {lo}..{hi}");
        assert!(water, "no water in 36 chunks");
        let mut again = OverworldGenerator::new(7).realistic();
        assert_eq!(again.generate(0, 0, &mut WorldChunkManager::new(7)), first);
        // And it is a different world from the vanilla one.
        assert_ne!(OverworldGenerator::new(7).generate(0, 0, &mut WorldChunkManager::new(7)), first);
    }
}
