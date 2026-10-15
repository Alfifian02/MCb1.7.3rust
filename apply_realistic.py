#!/usr/bin/env python3
"""Run from the repo root: python3 apply_realistic.py
Adds a second world generator ("Realistic", RTG-style terrain) to mc-rs and a pause-menu button that switches the world type and
resets the world. Every edit is found by its text (not line numbers); nothing is written unless every anchor is found exactly
once. Safe to re-run (applied edits are skipped)."""
import os, sys

ED = {}
def e(path, anchor, new, marker):
    ED.setdefault(path, []).append((anchor, new, marker))

G = "mc-rs/src/world/gen/"
T = "mc-rs/src/input/touch_ui.rs"
L = "mc-rs/src/lib.rs"
C = "mc-rs/src/world/chunks.rs"

# ---- generator plumbing
e(G + "mod.rs", "pub mod populate;", "pub mod populate;\npub mod realistic;", "pub mod realistic;")
O = G + "overworld.rs"
e(O, "    cols: HashMap<(i32, i32), Col>,\n    max_cols: usize,\n}",
  "    cols: HashMap<(i32, i32), Col>,\n    max_cols: usize,\n    /// The second world type (`realistic.rs`): its terrain replaces the density pass; surface, caves and populate are unchanged.\n    realistic: Option<crate::world::gen::realistic::Realistic>,\n}", "realistic: Option<crate")
e(O, "cols: HashMap::new(), max_cols: MAX_COLS }", "cols: HashMap::new(), max_cols: MAX_COLS, realistic: None }", "max_cols: MAX_COLS, realistic")
e(O, "    /// Highest solid block at column (x, z) of a grid",
  "    /// This generator on the Realistic world type.\n    pub fn realistic(mut self) -> Self {\n        self.realistic = Some(crate::world::gen::realistic::Realistic::new(self.seed));\n        self\n    }\n\n    /// Highest solid block at column (x, z) of a grid", "pub fn realistic(mut self)")
e(O, "        self.generate_terrain(chunk_x, chunk_z, &mut blocks, &cm.temperature, &cm.humidity);\n",
  """        if let Some(r) = self.realistic.as_ref() {
            use crate::world::gen::realistic::GRID;
            let grid = cm.load_block_generator_data(chunk_x * 16 - 16, chunk_z * 16 - 16, GRID, GRID);
            cm.load_block_generator_data(chunk_x * 16, chunk_z * 16, 16, 16); // leaves the chunk's own climate in `cm`, as the vanilla path does
            r.fill(chunk_x, chunk_z, &mut blocks, &grid, &cm.temperature);
        } else {
            self.generate_terrain(chunk_x, chunk_z, &mut blocks, &cm.temperature, &cm.humidity);
        }
""", "r.fill(chunk_x")

# ---- chunk manager: `new` stays, `new_typed` takes the world type
e(C, "    pub fn new(seed: i64, radius: i32) -> Self {\n",
  "    pub fn new(seed: i64, radius: i32) -> Self {\n        Self::new_typed(seed, radius, false)\n    }\n\n    /// `new` with the world type: `realistic` = the second generator (`gen::realistic`) in every worker and for `preload`.\n    pub fn new_typed(seed: i64, radius: i32, realistic: bool) -> Self {\n", "pub fn new_typed")
e(C, "let mut g = OverworldGenerator::new(seed);\n", "let mut g = OverworldGenerator::new(seed);\n                    if realistic {\n                        g = g.realistic();\n                    }\n", "g = g.realistic();")
e(C, "gen: OverworldGenerator::new(seed),", "gen: if realistic { OverworldGenerator::new(seed).realistic() } else { OverworldGenerator::new(seed) },", "gen: if realistic")

# ---- touch UI: a second two-tap button under the renderer button (the menu moves up so four buttons fit the screen)
e(T, "let resume = ((w - bw) * 0.5, (h - bh) * 0.5, bw, bh);", "let resume = ((w - bw) * 0.5, (h - bh) * 0.5 - bh * 0.45, bw, bh);", "- bh * 0.45")
e(T, "    Reset,\n", "    Reset,\n    /// Press landed on the world-type button (pause menu only). Arms on the first release, switches the type and resets on the second.\n    WorldType,\n", "    WorldType,\n")
e(T, "    pub backend: Rect,\n", "    pub backend: Rect,\n    /// World-type toggle, under `backend`.\n    pub world: Rect,\n", "pub world: Rect")
e(T, "        let backend = (resume.0, resume.1 + bh * 2.15, bw, bh * 0.8);\n",
  "        let backend = (resume.0, resume.1 + bh * 2.15, bw, bh * 0.8);\n        let world = (resume.0, resume.1 + bh * 3.05, bw, bh * 0.8);\n", "let world = (resume.0")
e(T, "resume, reset, backend, hotbar", "resume, reset, backend, world, hotbar", "backend, world, hotbar")
e(T, "else { in_rect(self.backend, x, y).then_some(PointerRole::Backend) };",
  "else if in_rect(self.backend, x, y) { Some(PointerRole::Backend) } else { in_rect(self.world, x, y).then_some(PointerRole::WorldType) };", "PointerRole::WorldType) };")
e(T, "    reset: bool,\n", "    reset: bool,\n    /// World-type button: armed by one tap, confirmed by the next (like the reset button).\n    pub wt_armed: bool,\n    wt: bool,\n", "pub wt_armed")
e(T, "            reset: false,\n", "            reset: false,\n            wt_armed: false,\n            wt: false,\n", "wt_armed: false")
e(T, "    /// The reset-world button was confirmed (tapped twice) since the last call.\n",
  "    /// The world-type button was confirmed (tapped twice) since the last call.\n    pub fn take_worldtype(&mut self) -> bool {\n        std::mem::take(&mut self.wt)\n    }\n\n    /// The reset-world button was confirmed (tapped twice) since the last call.\n", "pub fn take_worldtype")
e(T, "            PointerRole::Reset => {\n",
  "            PointerRole::WorldType => {\n                self.reset_armed = false;\n                if self.wt_armed {\n                    (self.wt, self.wt_armed, self.paused) = (true, false, false);\n                    self.clear();\n                } else {\n                    self.wt_armed = true;\n                }\n            }\n            PointerRole::Reset => {\n", "PointerRole::WorldType => {\n")
e(T, "                    self.reset_armed = true;\n", "                    self.reset_armed = true;\n                    self.wt_armed = false;\n", "self.wt_armed = false;\n                }")
e(T, "                self.paused = !self.paused;\n                self.reset_armed = false;\n", "                self.paused = !self.paused;\n                self.reset_armed = false;\n                self.wt_armed = false;\n", "self.reset_armed = false;\n                self.wt_armed = false;")
e(T, "    #[test]\n    fn open_screen_turns_presses_into_taps() {", """    /// The world-type button answers only while paused and needs two taps; arming it disarms the reset button and the other way round.
    #[test]
    fn world_type_needs_two_taps() {
        let mut ui = TouchUi::new(W, H);
        let (x, y) = (ui.layout.world.0 + 5.0, ui.layout.world.1 + 5.0);
        assert!(ui.layout.world.1 + ui.layout.world.3 <= H as f32);
        assert_eq!(ui.layout.hit_test(x, y, false, false), Some(PointerRole::Look));
        ui.paused = true;
        tap(&mut ui, 1, x, y);
        assert!(ui.wt_armed && !ui.take_worldtype() && ui.paused);
        let (rx, ry) = (ui.layout.reset.0 + 5.0, ui.layout.reset.1 + 5.0);
        tap(&mut ui, 2, rx, ry);
        assert!(ui.reset_armed && !ui.wt_armed);
        tap(&mut ui, 3, x, y);
        assert!(ui.wt_armed && !ui.reset_armed);
        tap(&mut ui, 4, x, y);
        assert!(ui.take_worldtype() && !ui.take_worldtype() && !ui.take_reset() && !ui.paused && !ui.wt_armed);
    }

    #[test]
    fn open_screen_turns_presses_into_taps() {""", "fn world_type_needs_two_taps")

# ---- lib.rs: the App remembers the type; the button switches it and resets
e(L, "struct App {\n    gpu: Gpu,\n", "struct App {\n    gpu: Gpu,\n    /// World type: false = vanilla b1.7.3 terrain, true = Realistic (`world::gen::realistic`). Saved in `<private storage>/worldtype`.\n    realistic: bool,\n", "    realistic: bool,\n")
e(L, "        let dir = dir.join(format!(\"world-{SEED:x}\"));\n",
  "        let realistic = std::fs::read_to_string(dir.join(\"worldtype\")).is_ok_and(|s| s.trim() == \"realistic\");\n        let dir = dir.join(world_dir(realistic));\n", "let realistic = std::fs::read_to_string")
e(L, "let mut chunks = ChunkManager::new(SEED, RENDER_DIST).with_dir(dir.clone());", "let mut chunks = ChunkManager::new_typed(SEED, RENDER_DIST, realistic).with_dir(dir.clone());", "new_typed(SEED, RENDER_DIST, realistic)")
e(L, "let mut app = Self {\n            gpu, switch: false, pipe,", "let mut app = Self {\n            gpu, realistic, switch: false, pipe,", "gpu, realistic, switch: false")
e(L, "self.chunks = ChunkManager::new(SEED, RENDER_DIST).with_dir(self.dir.clone());", "self.chunks = ChunkManager::new_typed(SEED, RENDER_DIST, self.realistic).with_dir(self.dir.clone());", "new_typed(SEED, RENDER_DIST, self.realistic)")
e(L, "    /// TEMPORARY (pause-menu reset button): delete the save folder", """    /// The pause menu's world-type button: save this world (it stays in its own folder), flip the type, remember it, and start a
    /// fresh world of the other type (its folder is wiped, as a reset does).
    fn switch_world_type(&mut self) {
        self.save(usize::MAX);
        self.realistic = !self.realistic;
        let root = self.dir.parent().map_or_else(std::env::temp_dir, |p| p.to_path_buf());
        let _ = std::fs::write(root.join("worldtype"), if self.realistic { "realistic" } else { "vanilla" });
        self.dir = root.join(world_dir(self.realistic));
        self.reset_world();
    }

    /// TEMPORARY (pause-menu reset button): delete the save folder""", "fn switch_world_type")
e(L, "/// A new world's spawn point (feet position)", "/// Save folder of a world type: the vanilla one keeps the name older saves have.\nfn world_dir(realistic: bool) -> String {\n    if realistic { format!(\"world-{SEED:x}-rtg\") } else { format!(\"world-{SEED:x}\") }\n}\n\n/// A new world's spawn point (feet position)", "fn world_dir(")
e(L, "        if self.touch.take_reset() {\n            self.reset_world();\n            return;\n        }\n", "        if self.touch.take_reset() {\n            self.reset_world();\n            return;\n        }\n        if self.touch.take_worldtype() {\n            self.switch_world_type();\n            return;\n        }\n", "self.touch.take_worldtype()")
e(L, "            label(v, layout.reset, if self.touch.reset_armed { \"Tap again to confirm\" } else { \"Reset world\" }, self.touch.reset_armed);\n",
  "            label(v, layout.reset, if self.touch.reset_armed { \"Tap again to confirm\" } else { \"Reset world\" }, self.touch.reset_armed);\n            label(v, layout.world, if self.touch.wt_armed { \"Tap again to confirm\" } else if self.realistic { \"World: Realistic\" } else { \"World: Vanilla\" }, self.touch.wt_armed);\n", "layout.world,")

NEW = {G + "realistic.rs": r'''//! Second world generator, "Realistic": terrain shaped the way Realistic Terrain Generation (Team RTG, GPL-3.0) shapes it,
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
'''}

out, bad = {}, []
for path, edits in ED.items():
    try:
        s = open(path).read()
    except OSError as ex:
        bad.append(f"{path}: {ex}"); continue
    for anchor, new, marker in edits:
        if marker in s:
            continue
        n = s.count(anchor)
        if n != 1:
            bad.append(f"{path}: anchor found {n}x (need 1): {anchor.strip()[:70]!r}")
        else:
            s = s.replace(anchor, new, 1)
    out[path] = s
if bad:
    print("NOTHING WRITTEN. Problems:\n  " + "\n  ".join(bad)); sys.exit(1)
for path, s in out.items():
    open(path, "w").write(s)
for path, body in NEW.items():
    if not os.path.exists(path):
        open(path, "w").write(body)

try:
    r = open("ROADMAP.md").read()
    if "realistic_world_is_deterministic" not in r:
        open("ROADMAP.md", "a").write("\n- `done` Second world type \"Realistic\" (compiled: no, ran: no, written without a toolchain; +2 tests `gen::realistic::tests::realistic_world_is_deterministic_hilly_and_wet`, `touch_ui::tests::world_type_needs_two_taps`, not yet to be tested). `world/gen/realistic.rs`: terrain after Realistic Terrain Generation (Team RTG, GPL-3.0): per-biome base height + hill strength blended over a 17 x 17 box, RTG's `blendedHillHeight` / `hills`, mountain mask with ridges, ocean basins, river valleys. It replaces only the density pass in `OverworldGenerator::generate` (`OverworldGenerator::realistic()`); surface, caves and populate are the vanilla ones. `ChunkManager::new_typed(.., realistic)`. Pause menu: `World: Vanilla / Realistic` (two taps) saves the current world, flips the type, writes `<private storage>/worldtype`, and resets into a fresh world of the other type; each type has its own save folder (`world-<seed>` and `world-<seed>-rtg`).\n    - ponytail: RTG's OpenSimplex, 1.12 biomes, `ChunkLandscape` cache and its 256-high numbers are not ported (the b1.7.3 simplex and Beta biomes stand in, scaled to sea level 64 of 128); river bends are a second added noise. UNVERIFIED: how the terrain looks and whether the spawn search finds dry land at the origin; the numbers in `params` and the 0.045 river width are eyeballed, not measured.\n")
except OSError:
    pass
try:
    rd = open("README.md").read()
    row = "| Realistic world type | written without a toolchain: not compiled, not run (+2 tests) | `world/gen/realistic.rs`: RTG-style terrain (blended biome heights, hills, mountains, oceans, rivers) as a second generator; the pause menu's `World: ...` button (two taps) saves, switches the type and resets. See ROADMAP changelog. |\n"
    if "Realistic world type" not in rd and "| M9..M14" in rd:
        open("README.md", "w").write(rd.replace("| M9..M14", row + "| M9..M14", 1))
except OSError:
    pass
print("ok: Realistic world type added;", len(out), "files edited,", len(NEW), "created")
