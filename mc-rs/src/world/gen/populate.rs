//! `ChunkProviderGenerate.populate` and the `WorldGen*` classes it calls: lakes, dungeons, clay,
//! dirt/gravel/ore veins, trees (oak, birch, big, both taigas), flowers, tall grass, dead bush,
//! mushrooms, reeds, pumpkins, cactus, water/lava springs and snow.
//!
//! populate(cx, cz) works on the 2x2 chunks (cx..=cx+1, cz..=cz+1), see [`Region`]. Everything
//! that draws from the Random does so in the Java's order, so a feature that fails still burns
//! the same draws. Verified against the real classes by `populate_matches_java` (tools/golden/).
//!
//! UNVERIFIED / simplified on purpose (none of these change a Random draw, only a placed block):
//! - Light is the column model of `Chunk.func_1024_c` (15 minus the opacities from the top
//!   down), so no lateral spread and no block light. Used by flowers, mushrooms and lake grass.
//! - Only the dungeon chests are tile entities (`Region::take_loot`); the spawner mob is drawn from the Random and dropped. No block
//!   ticks: springs are placed but do not flow, sand/gravel do not fall.
//!
//! Block metadata is written like the Java (`setBlockAndMetadata`): birch and taiga leaf/log species, the
//! tall-grass type, the pumpkin facing; every other `setBlock` resets the cell's metadata to 0. Checked by
//! the `META` lines of the golden file.

use crate::world::biome::Biome;
use crate::world::chest::{Chest, Pos};
use crate::world::chunk::{idx, Nibbles};
use crate::world::gen::chunk_manager::WorldChunkManager;
use crate::world::gen::noise::{ifloor, mh_cos, mh_sin, JavaRandom};
use crate::world::items::ItemStack;
use crate::world::gen::overworld::{block::*, OverworldGenerator};
use std::f32::consts::PI;

/// The 2x2 chunks populate may touch, standing in for the Java `World`. Reads outside read as air
/// and writes outside are dropped, like the Java's `emptyChunk` (populate is only run when the +X/+Z
/// neighbours exist, and nothing reaches past them).
pub struct Region {
    cx: i32,
    cz: i32,
    /// Chunk (cx + i, cz + j) at `i * 2 + j`.
    b: [Vec<u8>; 4],
    /// Block metadata of the same chunks (`Chunk.data`).
    d: [Nibbles; 4],
    /// The tile entities populate filled: dungeon chests and their loot (`WorldGenDungeons`).
    loot: Vec<(Pos, Chest)>,
}

impl Region {
    pub fn new(cx: i32, cz: i32, b: [Vec<u8>; 4], d: [Nibbles; 4]) -> Self {
        Self { cx, cz, b, d, loot: Vec::new() }
    }

    /// The chests populate filled since the last call.
    pub fn take_loot(&mut self) -> Vec<(Pos, Chest)> {
        std::mem::take(&mut self.loot)
    }

    pub fn into_parts(self) -> ([Vec<u8>; 4], [Nibbles; 4]) {
        (self.b, self.d)
    }

    fn at(&self, x: i32, y: i32, z: i32) -> Option<(usize, usize)> {
        let (i, j) = ((x >> 4) - self.cx, (z >> 4) - self.cz);
        if !(0..128).contains(&y) || !(0..2).contains(&i) || !(0..2).contains(&j) {
            return None;
        }
        Some(((i * 2 + j) as usize, idx((x & 15) as usize, y as usize, (z & 15) as usize)))
    }

    pub fn get(&self, x: i32, y: i32, z: i32) -> u8 {
        self.at(x, y, z).map_or(0, |(c, i)| self.b[c][i])
    }

    /// `World.setBlock` (`Chunk.setBlockID`): a different id resets the cell's metadata to 0, the same id changes nothing.
    fn set(&mut self, x: i32, y: i32, z: i32, id: u8) {
        if self.get(x, y, z) != id {
            self.set_meta(x, y, z, id, 0);
        }
    }

    /// `World.setBlockAndMetadata`: id and metadata (low 4 bits) are both written.
    fn set_meta(&mut self, x: i32, y: i32, z: i32, id: u8, meta: u8) {
        if let Some((c, i)) = self.at(x, y, z) {
            self.b[c][i] = id;
            self.d[c].set((x & 15) as usize, y as usize, (z & 15) as usize, meta);
        }
    }

    fn air(&self, x: i32, y: i32, z: i32) -> bool {
        self.get(x, y, z) == AIR
    }

    /// Chunk.heightMap: y of the first cell above the topmost block with light opacity, 0 off the region.
    fn height(&self, x: i32, z: i32) -> i32 {
        if self.at(x, 0, z).is_none() {
            return 0;
        }
        let mut y = 127;
        while y > 0 && opacity(self.get(x, y - 1, z)) == 0 {
            y -= 1;
        }
        y
    }

    /// Sky light of the column model (see the module docs).
    fn sky(&self, x: i32, y: i32, z: i32) -> i32 {
        if y < 0 {
            return 0;
        }
        let mut light = 15;
        for yy in (y.min(127)..128).rev() {
            light -= opacity(self.get(x, yy, z)) as i32;
            if light <= 0 {
                return 0;
            }
        }
        light
    }

    /// World.findTopSolidBlock: one above the highest solid or liquid cell, -1 if none.
    fn top_solid(&self, x: i32, z: i32) -> i32 {
        (1..128).rev().find(|&y| matches!(self.get(x, y, z), b if is_solid(b) || is_liquid(b))).map_or(-1, |y| y + 1)
    }
}

// Block tables for the ids the generator can place (checked against the real `Block` statics by
// `block_tables_match_java`).
fn is_liquid(b: u8) -> bool {
    matches!(b, WATER_MOVING..=LAVA)
}

/// Material.isSolid(): false for air, liquids, plants, snow layer.
fn is_solid(b: u8) -> bool {
    !matches!(b, AIR | WATER_MOVING..=LAVA | TALL_GRASS | DEAD_BUSH | FLOWER_Y..=MUSH_RED | SNOW | REED)
}

/// Block.opaqueCubeLookup. Leaves are in it (the Java fills the lookup in the constructor, before
/// `graphicsLevel` is set), which is why trees do not replace leaves with leaves.
fn opaque(b: u8) -> bool {
    matches!(b, STONE..=COBBLE | BEDROCK | SAND..=LOG | LEAVES | ORE_LAPIS | SANDSTONE | MOSSY | ORE_DIAMOND | ORE_REDSTONE | CLAY | PUMPKIN | CHEST | 5)
}

/// Block.lightOpacity.
fn opacity(b: u8) -> u8 {
    match b {
        WATER_MOVING | WATER | ICE => 3,
        LAVA_MOVING | LAVA => 255,
        LEAVES => 1,
        _ if opaque(b) => 255,
        _ => 0,
    }
}

fn is_water(b: u8) -> bool {
    b == WATER_MOVING || b == WATER
}

/// BlockFlower.canBlockStay and its subclasses (mushroom, dead bush).
fn can_stay(w: &Region, id: u8, x: i32, y: i32, z: i32) -> bool {
    let below = w.get(x, y - 1, z);
    match id {
        MUSH_BROWN | MUSH_RED => (0..128).contains(&y) && w.sky(x, y, z) < 13 && opaque(below),
        _ => {
            let ground = if id == DEAD_BUSH { below == SAND } else { below == GRASS || below == DIRT || below == 60 };
            (w.sky(x, y, z) >= 8 || y >= w.height(x, z)) && ground
        }
    }
}

impl OverworldGenerator {
    /// ChunkProviderGenerate.populate for chunk (cx, cz); `w` must start at (cx, cz).
    pub fn populate(&mut self, w: &mut Region, cx: i32, cz: i32, cm: &mut WorldChunkManager) {
        let (bx, bz) = (cx * 16, cz * 16);
        let biome = cm.load_block_generator_data(bx + 16, bz + 16, 1, 1)[0];
        let mut r = JavaRandom::new(self.seed);
        let a = r.next_long() / 2 * 2 + 1;
        let b = r.next_long() / 2 * 2 + 1;
        r.set_seed((cx as i64).wrapping_mul(a).wrapping_add((cz as i64).wrapping_mul(b)) ^ self.seed);

        if r.next_int_bound(4) == 0 {
            let (x, y, z) = (bx + r.next_int_bound(16) + 8, r.next_int_bound(128), bz + r.next_int_bound(16) + 8);
            lake(w, &mut r, WATER, x, y, z);
        }
        if r.next_int_bound(8) == 0 {
            let x = bx + r.next_int_bound(16) + 8;
            let h = r.next_int_bound(120) + 8;
            let y = r.next_int_bound(h);
            let z = bz + r.next_int_bound(16) + 8;
            if y < 64 || r.next_int_bound(10) == 0 {
                lake(w, &mut r, LAVA, x, y, z);
            }
        }
        for _ in 0..8 {
            let (x, y, z) = (bx + r.next_int_bound(16) + 8, r.next_int_bound(128), bz + r.next_int_bound(16) + 8);
            dungeon(w, &mut r, x, y, z);
        }
        // Veins: (count, size, replaces stone with, y range). Lapis is the odd one (two draws summed).
        for _ in 0..10 {
            let (x, y, z) = (bx + r.next_int_bound(16), r.next_int_bound(128), bz + r.next_int_bound(16));
            if is_water(w.get(x, y, z)) {
                vein(w, &mut r, x, y, z, 32, SAND, CLAY);
            }
        }
        for &(n, size, id, ymax) in &[(20, 32, DIRT, 128), (10, 32, GRAVEL, 128), (20, 16, ORE_COAL, 128),
                                      (20, 8, ORE_IRON, 64), (2, 8, ORE_GOLD, 32), (8, 7, ORE_REDSTONE, 16),
                                      (1, 7, ORE_DIAMOND, 16)] {
            for _ in 0..n {
                let (x, y, z) = (bx + r.next_int_bound(16), r.next_int_bound(ymax), bz + r.next_int_bound(16));
                vein(w, &mut r, x, y, z, size, STONE, id);
            }
        }
        let x = bx + r.next_int_bound(16);
        let y = r.next_int_bound(16) + r.next_int_bound(16);
        let z = bz + r.next_int_bound(16);
        vein(w, &mut r, x, y, z, 6, STONE, ORE_LAPIS);

        let noise = self.noise_spawner.sample(bx as f64 * 0.5, bz as f64 * 0.5);
        let n = ((noise / 8.0 + r.next_double() * 4.0 + 4.0) / 3.0) as i32;
        let mut trees = i32::from(r.next_int_bound(10) == 0);
        trees += match biome {
            Biome::Forest | Biome::Rainforest | Biome::Taiga => n + 5,
            Biome::SeasonalForest => n + 2,
            Biome::Desert | Biome::Tundra | Biome::Plains => -20,
            _ => 0,
        };
        for _ in 0..trees {
            let (x, z) = (bx + r.next_int_bound(16) + 8, bz + r.next_int_bound(16) + 8);
            let kind = Tree::pick(biome, &mut r);
            let y = w.height(x, z);
            kind.generate(w, &mut r, x, y, z);
        }

        let n = match biome {
            Biome::Forest | Biome::Taiga => 2,
            Biome::SeasonalForest => 4,
            Biome::Plains => 3,
            _ => 0,
        };
        for _ in 0..n {
            let (x, y, z) = (bx + r.next_int_bound(16) + 8, r.next_int_bound(128), bz + r.next_int_bound(16) + 8);
            plants(w, &mut r, FLOWER_Y, x, y, z);
        }
        let n = match biome {
            Biome::Rainforest | Biome::Plains => 10,
            Biome::Forest | Biome::SeasonalForest => 2,
            Biome::Taiga => 1,
            _ => 0,
        };
        for _ in 0..n {
            // The grass type is drawn first: a fern (2) in two tries of three in a rainforest, else tall grass (1).
            let kind = if biome == Biome::Rainforest && r.next_int_bound(3) != 0 { 2 } else { 1 };
            let (x, y, z) = (bx + r.next_int_bound(16) + 8, r.next_int_bound(128), bz + r.next_int_bound(16) + 8);
            ground_plants(w, &mut r, TALL_GRASS, 128, kind, x, y, z);
        }
        for _ in 0..(if biome == Biome::Desert { 2 } else { 0 }) {
            let (x, y, z) = (bx + r.next_int_bound(16) + 8, r.next_int_bound(128), bz + r.next_int_bound(16) + 8);
            ground_plants(w, &mut r, DEAD_BUSH, 4, 0, x, y, z);
        }
        for &(odds, id) in &[(2, FLOWER_R), (4, MUSH_BROWN), (8, MUSH_RED)] {
            if r.next_int_bound(odds) == 0 {
                let (x, y, z) = (bx + r.next_int_bound(16) + 8, r.next_int_bound(128), bz + r.next_int_bound(16) + 8);
                plants(w, &mut r, id, x, y, z);
            }
        }
        for _ in 0..10 {
            let (x, y, z) = (bx + r.next_int_bound(16) + 8, r.next_int_bound(128), bz + r.next_int_bound(16) + 8);
            reeds(w, &mut r, x, y, z);
        }
        if r.next_int_bound(32) == 0 {
            let (x, y, z) = (bx + r.next_int_bound(16) + 8, r.next_int_bound(128), bz + r.next_int_bound(16) + 8);
            pumpkins(w, &mut r, x, y, z);
        }
        for _ in 0..(if biome == Biome::Desert { 10 } else { 0 }) {
            let (x, y, z) = (bx + r.next_int_bound(16) + 8, r.next_int_bound(128), bz + r.next_int_bound(16) + 8);
            cactus(w, &mut r, x, y, z);
        }
        for _ in 0..50 {
            let x = bx + r.next_int_bound(16) + 8;
            let h = r.next_int_bound(120) + 8;
            let y = r.next_int_bound(h);
            let z = bz + r.next_int_bound(16) + 8;
            spring(w, x, y, z, WATER_MOVING);
        }
        for _ in 0..20 {
            let x = bx + r.next_int_bound(16) + 8;
            let h = r.next_int_bound(112) + 8;
            let h = r.next_int_bound(h) + 8;
            let y = r.next_int_bound(h);
            let z = bz + r.next_int_bound(16) + 8;
            spring(w, x, y, z, LAVA_MOVING);
        }

        // Snow on cold ground. getTemperatures(x + 8, z + 8, 16, 16) is the same climate maths as
        // loadBlockGeneratorData, so reuse it.
        cm.load_block_generator_data(bx + 8, bz + 8, 16, 16);
        for x in bx + 8..bx + 24 {
            for z in bz + 8..bz + 24 {
                let y = w.top_solid(x, z);
                let t = cm.temperature[((x - (bx + 8)) * 16 + (z - (bz + 8))) as usize] - (y - 64) as f64 / 64.0 * 0.3;
                if t < 0.5 && y > 0 && y < 128 && w.air(x, y, z) {
                    let below = w.get(x, y - 1, z);
                    if is_solid(below) && below != ICE {
                        w.set(x, y, z, SNOW);
                    }
                }
            }
        }
    }
}

/// WorldGenMinable and WorldGenClay (same maths): an ellipsoid chain that turns `from` into `to`.
#[allow(clippy::too_many_arguments)]
fn vein(w: &mut Region, r: &mut JavaRandom, x: i32, y: i32, z: i32, n: i32, from: u8, to: u8) {
    let angle = r.next_float() * PI;
    let nf = n as f32;
    let x0 = ((x + 8) as f32 + mh_sin(angle) * nf / 8.0) as f64;
    let x1 = ((x + 8) as f32 - mh_sin(angle) * nf / 8.0) as f64;
    let z0 = ((z + 8) as f32 + mh_cos(angle) * nf / 8.0) as f64;
    let z1 = ((z + 8) as f32 - mh_cos(angle) * nf / 8.0) as f64;
    let y0 = (y + r.next_int_bound(3) + 2) as f64;
    let y1 = (y + r.next_int_bound(3) + 2) as f64;
    for i in 0..=n {
        let (cx, cy, cz) = (x0 + (x1 - x0) * i as f64 / n as f64, y0 + (y1 - y0) * i as f64 / n as f64, z0 + (z1 - z0) * i as f64 / n as f64);
        let size = r.next_double() * n as f64 / 16.0;
        let s = (mh_sin(i as f32 * PI / nf) + 1.0) as f64 * size + 1.0;
        for bx in ifloor(cx - s / 2.0)..=ifloor(cx + s / 2.0) {
            let dx = ((bx as f64) + 0.5 - cx) / (s / 2.0);
            if dx * dx >= 1.0 {
                continue;
            }
            for by in ifloor(cy - s / 2.0)..=ifloor(cy + s / 2.0) {
                let dy = ((by as f64) + 0.5 - cy) / (s / 2.0);
                if dx * dx + dy * dy >= 1.0 {
                    continue;
                }
                for bz in ifloor(cz - s / 2.0)..=ifloor(cz + s / 2.0) {
                    let dz = ((bz as f64) + 0.5 - cz) / (s / 2.0);
                    if dx * dx + dy * dy + dz * dz < 1.0 && w.get(bx, by, bz) == from {
                        w.set(bx, by, bz, to);
                    }
                }
            }
        }
    }
}

/// WorldGenLakes (`liquid` is still water 9 or still lava 11).
fn lake(w: &mut Region, r: &mut JavaRandom, liquid: u8, x: i32, y: i32, z: i32) {
    let (x, z) = (x - 8, z - 8);
    let mut y = y;
    while y > 0 && w.air(x, y, z) {
        y -= 1;
    }
    y -= 4;
    let mut m = [false; 2048];
    for _ in 0..r.next_int_bound(4) + 4 {
        let d1 = r.next_double() * 6.0 + 3.0;
        let d2 = r.next_double() * 4.0 + 2.0;
        let d3 = r.next_double() * 6.0 + 3.0;
        let c1 = r.next_double() * (16.0 - d1 - 2.0) + 1.0 + d1 / 2.0;
        let c2 = r.next_double() * (8.0 - d2 - 4.0) + 2.0 + d2 / 2.0;
        let c3 = r.next_double() * (16.0 - d3 - 2.0) + 1.0 + d3 / 2.0;
        for i in 1..15usize {
            for j in 1..15usize {
                for k in 1..7usize {
                    let dx = (i as f64 - c1) / (d1 / 2.0);
                    let dy = (k as f64 - c2) / (d2 / 2.0);
                    let dz = (j as f64 - c3) / (d3 / 2.0);
                    if dx * dx + dy * dy + dz * dz < 1.0 {
                        m[(i * 16 + j) * 8 + k] = true;
                    }
                }
            }
        }
    }
    let at = |i: usize, j: usize, k: usize| m[(i * 16 + j) * 8 + k];
    // Cells next to the lake but not in it (the shell).
    let shell = |i: usize, j: usize, k: usize| {
        !at(i, j, k)
            && (i < 15 && at(i + 1, j, k) || i > 0 && at(i - 1, j, k) || j < 15 && at(i, j + 1, k)
                || j > 0 && at(i, j - 1, k) || k < 7 && at(i, j, k + 1) || k > 0 && at(i, j, k - 1))
    };
    for i in 0..16usize {
        for j in 0..16usize {
            for k in 0..8usize {
                if shell(i, j, k) {
                    let b = w.get(x + i as i32, y + k as i32, z + j as i32);
                    if k >= 4 && is_liquid(b) {
                        return;
                    }
                    if k < 4 && !is_solid(b) && b != liquid {
                        return;
                    }
                }
            }
        }
    }
    for i in 0..16usize {
        for j in 0..16usize {
            for k in 0..8usize {
                if at(i, j, k) {
                    w.set(x + i as i32, y + k as i32, z + j as i32, if k >= 4 { AIR } else { liquid });
                }
            }
        }
    }
    for i in 0..16usize {
        for j in 0..16usize {
            for k in 4..8usize {
                let (bx, by, bz) = (x + i as i32, y + k as i32, z + j as i32);
                // Lit grass rule: sky light > 0 in the column model.
                if at(i, j, k) && w.get(bx, by - 1, bz) == DIRT && w.sky(bx, by, bz) > 0 {
                    w.set(bx, by - 1, bz, GRASS);
                }
            }
        }
    }
    if liquid == LAVA {
        for i in 0..16usize {
            for j in 0..16usize {
                for k in 0..8usize {
                    let (bx, by, bz) = (x + i as i32, y + k as i32, z + j as i32);
                    if shell(i, j, k) && (k < 4 || r.next_int_bound(2) != 0) && is_solid(w.get(bx, by, bz)) {
                        w.set(bx, by, bz, STONE);
                    }
                }
            }
        }
    }
}

/// WorldGenDungeons. The chest loot goes into the chest (`Region::take_loot`); the spawner mob is drawn (to keep the Random
/// in step) and dropped.
fn dungeon(w: &mut Region, r: &mut JavaRandom, x: i32, y: i32, z: i32) {
    let h = 3;
    let sx = r.next_int_bound(2) + 2;
    let sz = r.next_int_bound(2) + 2;
    let mut openings = 0;
    for xx in x - sx - 1..=x + sx + 1 {
        for yy in y - 1..=y + h + 1 {
            for zz in z - sz - 1..=z + sz + 1 {
                let solid = is_solid(w.get(xx, yy, zz));
                if (yy == y - 1 || yy == y + h + 1) && !solid {
                    return;
                }
                let wall = xx == x - sx - 1 || xx == x + sx + 1 || zz == z - sz - 1 || zz == z + sz + 1;
                if wall && yy == y && w.air(xx, yy, zz) && w.air(xx, yy + 1, zz) {
                    openings += 1;
                }
            }
        }
    }
    if !(1..=5).contains(&openings) {
        return;
    }
    for xx in x - sx - 1..=x + sx + 1 {
        for yy in (y - 1..=y + h).rev() {
            for zz in z - sz - 1..=z + sz + 1 {
                let inside = xx != x - sx - 1 && yy != y - 1 && zz != z - sz - 1 && xx != x + sx + 1 && yy != y + h + 1 && zz != z + sz + 1;
                if inside || (yy >= 0 && !is_solid(w.get(xx, yy - 1, zz))) {
                    w.set(xx, yy, zz, AIR);
                } else if is_solid(w.get(xx, yy, zz)) {
                    let mossy = yy == y - 1 && r.next_int_bound(4) != 0;
                    w.set(xx, yy, zz, if mossy { MOSSY } else { COBBLE });
                }
            }
        }
    }
    for _ in 0..2 {
        for _ in 0..3 {
            let cx = x + r.next_int_bound(sx * 2 + 1) - sx;
            let cz = z + r.next_int_bound(sz * 2 + 1) - sz;
            if !w.air(cx, y, cz) {
                continue;
            }
            let walls = [(cx - 1, cz), (cx + 1, cz), (cx, cz - 1), (cx, cz + 1)].iter().filter(|&&(a, b)| is_solid(w.get(a, y, b))).count();
            if walls == 1 {
                w.set(cx, y, cz, CHEST);
                let mut chest = Chest::default();
                for _ in 0..8 {
                    if let Some(st) = loot(r) {
                        chest[r.next_int_bound(27) as usize] = Some(st); // setInventorySlotContents: a later pick overwrites
                    }
                }
                w.loot.push(((cx, y, cz), chest));
                break;
            }
        }
    }
    w.set(x, y, z, SPAWNER);
    r.next_int_bound(4); // mob
}

/// WorldGenDungeons.pickCheckLootItem: the stack picked, if any (same draws, in the same order, as the Java).
fn loot(r: &mut JavaRandom) -> Option<ItemStack> {
    let n = |r: &mut JavaRandom| r.next_int_bound(4) as u8 + 1;
    let (id, count, damage) = match r.next_int_bound(11) {
        0 => (329, 1, 0),              // saddle
        1 => (265, n(r), 0),           // iron ingot
        2 => (297, 1, 0),              // bread
        3 => (296, n(r), 0),           // wheat
        4 => (289, n(r), 0),           // gunpowder
        5 => (287, n(r), 0),           // string
        6 => (325, 1, 0),              // bucket
        7 if r.next_int_bound(100) == 0 => (322, 1, 0), // golden apple
        8 if r.next_int_bound(2) == 0 => (331, n(r), 0), // redstone
        9 if r.next_int_bound(10) == 0 => (2256 + r.next_int_bound(2) as u16, 1, 0), // record 13 / cat
        10 => (351, 1, 3),             // cocoa beans
        _ => return None,
    };
    Some(ItemStack { id, count, damage })
}

/// WorldGenFlowers: yellow/red flowers and both mushrooms.
fn plants(w: &mut Region, r: &mut JavaRandom, id: u8, x: i32, y: i32, z: i32) {
    for _ in 0..64 {
        let (a, b) = (r.next_int_bound(8), r.next_int_bound(8));
        let x2 = x + a - b;
        let (a, b) = (r.next_int_bound(4), r.next_int_bound(4));
        let y2 = y + a - b;
        let (a, b) = (r.next_int_bound(8), r.next_int_bound(8));
        let z2 = z + a - b;
        if w.air(x2, y2, z2) && can_stay(w, id, x2, y2, z2) {
            w.set(x2, y2, z2, id);
        }
    }
}

/// WorldGenTallGrass (`tries` 128, `meta` its grass type) and WorldGenDeadBush (4, 0): sink to the ground first.
fn ground_plants(w: &mut Region, r: &mut JavaRandom, id: u8, tries: i32, meta: u8, x: i32, mut y: i32, z: i32) {
    while y > 0 && matches!(w.get(x, y, z), AIR | LEAVES) {
        y -= 1;
    }
    for _ in 0..tries {
        let (a, b) = (r.next_int_bound(8), r.next_int_bound(8));
        let x2 = x + a - b;
        let (a, b) = (r.next_int_bound(4), r.next_int_bound(4));
        let y2 = y + a - b;
        let (a, b) = (r.next_int_bound(8), r.next_int_bound(8));
        let z2 = z + a - b;
        if w.air(x2, y2, z2) && can_stay(w, id, x2, y2, z2) {
            w.set_meta(x2, y2, z2, id, meta);
        }
    }
}

/// BlockReed.canBlockStay.
fn reed_stays(w: &Region, x: i32, y: i32, z: i32) -> bool {
    match w.get(x, y - 1, z) {
        REED => true,
        GRASS | DIRT => [(x - 1, z), (x + 1, z), (x, z - 1), (x, z + 1)].iter().any(|&(a, b)| is_water(w.get(a, y - 1, b))),
        _ => false,
    }
}

fn reeds(w: &mut Region, r: &mut JavaRandom, x: i32, y: i32, z: i32) {
    for _ in 0..20 {
        let (a, b) = (r.next_int_bound(4), r.next_int_bound(4));
        let x2 = x + a - b;
        let (a, b) = (r.next_int_bound(4), r.next_int_bound(4));
        let z2 = z + a - b;
        let wet = [(x2 - 1, z2), (x2 + 1, z2), (x2, z2 - 1), (x2, z2 + 1)].iter().any(|&(a, b)| is_water(w.get(a, y - 1, b)));
        if w.air(x2, y, z2) && wet {
            let h = r.next_int_bound(3) + 1;
            for i in 0..2 + r.next_int_bound(h) {
                if reed_stays(w, x2, y + i, z2) {
                    w.set(x2, y + i, z2, REED);
                }
            }
        }
    }
}

fn pumpkins(w: &mut Region, r: &mut JavaRandom, x: i32, y: i32, z: i32) {
    for _ in 0..64 {
        let (a, b) = (r.next_int_bound(8), r.next_int_bound(8));
        let x2 = x + a - b;
        let (a, b) = (r.next_int_bound(4), r.next_int_bound(4));
        let y2 = y + a - b;
        let (a, b) = (r.next_int_bound(8), r.next_int_bound(8));
        let z2 = z + a - b;
        // canPlaceBlockAt: air above a normal cube; grass is one, so the grass test covers it.
        if w.air(x2, y2, z2) && w.get(x2, y2 - 1, z2) == GRASS {
            let facing = r.next_int_bound(4) as u8;
            w.set_meta(x2, y2, z2, PUMPKIN, facing);
        }
    }
}

fn cactus(w: &mut Region, r: &mut JavaRandom, x: i32, y: i32, z: i32) {
    for _ in 0..10 {
        let (a, b) = (r.next_int_bound(8), r.next_int_bound(8));
        let x2 = x + a - b;
        let (a, b) = (r.next_int_bound(4), r.next_int_bound(4));
        let y2 = y + a - b;
        let (a, b) = (r.next_int_bound(8), r.next_int_bound(8));
        let z2 = z + a - b;
        if w.air(x2, y2, z2) {
            let h = r.next_int_bound(3) + 1;
            for i in 0..1 + r.next_int_bound(h) {
                let y3 = y2 + i;
                let free = [(x2 - 1, z2), (x2 + 1, z2), (x2, z2 - 1), (x2, z2 + 1)].iter().all(|&(a, b)| !is_solid(w.get(a, y3, b)));
                if free && matches!(w.get(x2, y3 - 1, z2), CACTUS | SAND) {
                    w.set(x2, y3, z2, CACTUS);
                }
            }
        }
    }
}

/// WorldGenLiquids: a spring in a stone wall with exactly one open side. The Java then ticks the
/// block so it starts to flow; there are no block ticks yet, so it is only placed.
fn spring(w: &mut Region, x: i32, y: i32, z: i32, id: u8) {
    let here = w.get(x, y, z);
    if w.get(x, y + 1, z) != STONE || w.get(x, y - 1, z) != STONE || (here != AIR && here != STONE) {
        return;
    }
    let side = [(x - 1, z), (x + 1, z), (x, z - 1), (x, z + 1)];
    let stone = side.iter().filter(|&&(a, b)| w.get(a, y, b) == STONE).count();
    let air = side.iter().filter(|&&(a, b)| w.air(a, y, b)).count();
    if stone == 3 && air == 1 {
        w.set(x, y, z, id);
    }
}

/// The tree a biome's `getRandomWorldGenForTrees` picks.
#[derive(Clone, Copy)]
enum Tree {
    Oak,
    Birch,
    Big,
    Taiga1,
    Taiga2,
}

impl Tree {
    fn pick(biome: Biome, r: &mut JavaRandom) -> Tree {
        match biome {
            Biome::Forest => {
                if r.next_int_bound(5) == 0 {
                    Tree::Birch
                } else if r.next_int_bound(3) == 0 {
                    Tree::Big
                } else {
                    Tree::Oak
                }
            }
            Biome::Rainforest => if r.next_int_bound(3) == 0 { Tree::Big } else { Tree::Oak },
            Biome::Taiga => if r.next_int_bound(3) == 0 { Tree::Taiga1 } else { Tree::Taiga2 },
            _ => if r.next_int_bound(10) == 0 { Tree::Big } else { Tree::Oak },
        }
    }

    fn generate(self, w: &mut Region, r: &mut JavaRandom, x: i32, y: i32, z: i32) -> bool {
        match self {
            Tree::Oak => round_tree(w, r, x, y, z, 4, 0),
            Tree::Birch => round_tree(w, r, x, y, z, 5, 2),
            Tree::Big => big_tree(w, r, x, y, z),
            Tree::Taiga1 => taiga1(w, r, x, y, z),
            Tree::Taiga2 => taiga2(w, r, x, y, z),
        }
    }
}

/// True when every cell of the box is air or leaves (the trees' "room to grow" test).
fn room(w: &Region, x0: i32, x1: i32, y: i32, z0: i32, z1: i32) -> bool {
    (x0..=x1).all(|x| (z0..=z1).all(|z| (0..128).contains(&y) && matches!(w.get(x, y, z), AIR | LEAVES)))
}

fn grows_on(w: &Region, x: i32, y: i32, z: i32) -> bool {
    matches!(w.get(x, y - 1, z), GRASS | DIRT)
}

/// A leaf layer of radius `rad` around (x, z), skipping `skip_corner` cells; opaque cells are kept. `meta` is the
/// leaf kind: 0 oak, 1 spruce, 2 birch.
fn leaves(w: &mut Region, x: i32, y: i32, z: i32, rad: i32, meta: u8, mut skip_corner: impl FnMut(i32, i32) -> bool) {
    for xx in x - rad..=x + rad {
        for zz in z - rad..=z + rad {
            if !skip_corner(xx - x, zz - z) && !opaque(w.get(xx, y, zz)) {
                w.set_meta(xx, y, zz, LEAVES, meta);
            }
        }
    }
}

/// A trunk of `len` logs of species `meta` (0 oak, 1 spruce, 2 birch), only into air and leaves.
fn trunk(w: &mut Region, x: i32, y: i32, z: i32, len: i32, meta: u8) {
    for i in 0..len {
        if matches!(w.get(x, y + i, z), AIR | LEAVES) {
            w.set_meta(x, y + i, z, LOG, meta);
        }
    }
}

/// `BlockSapling.growTree`: clears the sapling cell, grows the species' tree (oak, one in ten big; spruce; birch) and puts
/// the sapling back when it does not fit.
pub fn grow_sapling(w: &mut Region, r: &mut JavaRandom, x: i32, y: i32, z: i32, species: u8) -> bool {
    w.set(x, y, z, 0);
    let ok = match species & 3 {
        1 => taiga2(w, r, x, y, z),
        2 => round_tree(w, r, x, y, z, 5, 2),
        _ if r.next_int_bound(10) == 0 => big_tree(w, r, x, y, z),
        _ => round_tree(w, r, x, y, z, 4, 0),
    };
    if !ok {
        w.set_meta(x, y, z, 6, species);
    }
    ok
}

/// WorldGenTrees (min height 4, species 0) and WorldGenForest (5, birch = 2).
fn round_tree(w: &mut Region, r: &mut JavaRandom, x: i32, y: i32, z: i32, min_h: i32, meta: u8) -> bool {
    let h = r.next_int_bound(3) + min_h;
    if y < 1 || y + h + 1 > 128 {
        return false;
    }
    for yy in y..=y + 1 + h {
        let rad = if yy >= y + 1 + h - 2 { 2 } else if yy == y { 0 } else { 1 };
        if !room(w, x - rad, x + rad, yy, z - rad, z + rad) {
            return false;
        }
    }
    if !grows_on(w, x, y, z) || y >= 128 - h - 1 {
        return false;
    }
    w.set(x, y - 1, z, DIRT);
    for yy in y - 3 + h..=y + h {
        let dy = yy - (y + h);
        let rad = 1 - dy / 2;
        // A corner is dropped half the time (and always on the top layer): one draw per corner.
        leaves(w, x, yy, z, rad, meta, |dx, dz| dx.abs() == rad && dz.abs() == rad && !(r.next_int_bound(2) != 0 && dy != 0));
    }
    trunk(w, x, y, z, h, meta);
    true
}

fn taiga1(w: &mut Region, r: &mut JavaRandom, x: i32, y: i32, z: i32) -> bool {
    let h = r.next_int_bound(5) + 7;
    let bare = h - r.next_int_bound(2) - 3;
    let span = h - bare;
    let max_rad = 1 + r.next_int_bound(span + 1);
    if y < 1 || y + h + 1 > 128 {
        return false;
    }
    for yy in y..=y + 1 + h {
        let rad = if yy - y < bare { 0 } else { max_rad };
        if !room(w, x - rad, x + rad, yy, z - rad, z + rad) {
            return false;
        }
    }
    if !grows_on(w, x, y, z) || y >= 128 - h - 1 {
        return false;
    }
    w.set(x, y - 1, z, DIRT);
    let mut rad = 0;
    for yy in (y + bare..=y + h).rev() {
        leaves(w, x, yy, z, rad, 1, |dx, dz| dx.abs() == rad && dz.abs() == rad && rad > 0);
        if rad >= 1 && yy == y + bare + 1 {
            rad -= 1;
        } else if rad < max_rad {
            rad += 1;
        }
    }
    trunk(w, x, y, z, h - 1, 1);
    true
}

fn taiga2(w: &mut Region, r: &mut JavaRandom, x: i32, y: i32, z: i32) -> bool {
    let h = r.next_int_bound(4) + 6;
    let bare = 1 + r.next_int_bound(2);
    let span = h - bare;
    let max_rad = 2 + r.next_int_bound(2);
    if y < 1 || y + h + 1 > 128 {
        return false;
    }
    for yy in y..=y + 1 + h {
        let rad = if yy - y < bare { 0 } else { max_rad };
        if !room(w, x - rad, x + rad, yy, z - rad, z + rad) {
            return false;
        }
    }
    if !grows_on(w, x, y, z) || y >= 128 - h - 1 {
        return false;
    }
    w.set(x, y - 1, z, DIRT);
    let mut rad = r.next_int_bound(2);
    let (mut next, mut after) = (1, 0);
    for i in 0..=span {
        leaves(w, x, y + h - i, z, rad, 1, |dx, dz| dx.abs() == rad && dz.abs() == rad && rad > 0);
        if rad >= next {
            rad = after;
            after = 1;
            next = (next + 1).min(max_rad);
        } else {
            rad += 1;
        }
    }
    let cut = r.next_int_bound(3);
    trunk(w, x, y, z, h - cut, 1);
    true
}

/// WorldGenBigTree, called as `func_517_a(1, 1, 1)` by populate: limit 12, leaf layers 5, scales 1.
struct Big<'a> {
    w: &'a mut Region,
    rnd: JavaRandom,
    base: [i32; 3],
    /// field_878_e: total height.
    total: i32,
    /// Trunk height.
    height: i32,
    /// Leaf clusters: (x, y, z, branch start y).
    nodes: Vec<[i32; 4]>,
}

const SPAN: i32 = 5; // field_869_n: leaf layers per cluster
const AXES: [usize; 6] = [2, 0, 0, 1, 2, 1]; // field_882_a

fn big_tree(w: &mut Region, r: &mut JavaRandom, x: i32, y: i32, z: i32) -> bool {
    let rnd = JavaRandom::new(r.next_long());
    let mut t = Big { w, rnd, base: [x, y, z], total: 0, height: 0, nodes: Vec::new() };
    t.total = 5 + t.rnd.next_int_bound(12);
    if !t.fits() {
        return false;
    }
    t.find_nodes();
    for i in 0..t.nodes.len() {
        let [nx, ny, nz, _] = t.nodes[i];
        for yy in ny..ny + SPAN {
            let rad = if (0..SPAN).contains(&(yy - ny)) { if yy - ny != 0 && yy - ny != SPAN - 1 { 3.0 } else { 2.0 } } else { -1.0 };
            t.disc(nx, yy, nz, rad, LEAVES);
        }
    }
    let [bx, by, bz] = t.base;
    t.line([bx, by, bz], [bx, by + t.height, bz], LOG);
    let mut from = t.base;
    for i in 0..t.nodes.len() {
        let n = t.nodes[i];
        from[1] = n[3];
        if (from[1] - t.base[1]) as f64 >= t.total as f64 * 0.2 {
            t.line(from, [n[0], n[1], n[2]], LOG);
        }
    }
    true
}

impl Big<'_> {
    /// func_528_a: leaf-cluster radius at height `i` above the base (negative: no cluster).
    fn radius_at(&self, i: i32) -> f32 {
        if (i as f64) < (self.total as f32) as f64 * 0.3 {
            return -1.618;
        }
        let half = self.total as f32 / 2.0;
        let d = self.total as f32 / 2.0 - i as f32;
        let v = if d == 0.0 {
            half
        } else if d.abs() >= half {
            0.0
        } else {
            ((half.abs() as f64).powi(2) - (d.abs() as f64).powi(2)).sqrt() as f32
        };
        v * 0.5
    }

    /// func_524_a: walk from a to b; -1 if the way is clear (air/leaves), else how far it got.
    fn clear(&self, a: [i32; 3], b: [i32; 3]) -> i32 {
        let mut d = [0; 3];
        let mut m = 0;
        for i in 0..3 {
            d[i] = b[i] - a[i];
            if d[i].abs() > d[m].abs() {
                m = i;
            }
        }
        if d[m] == 0 {
            return -1;
        }
        let (u, v) = (AXES[m], AXES[m + 3]);
        let s = if d[m] > 0 { 1 } else { -1 };
        let (du, dv) = (d[u] as f64 / d[m] as f64, d[v] as f64 / d[m] as f64);
        let (mut i, end) = (0, d[m] + s);
        while i != end {
            let mut p = [0; 3];
            p[m] = a[m] + i;
            p[u] = ifloor(a[u] as f64 + i as f64 * du);
            p[v] = ifloor(a[v] as f64 + i as f64 * dv);
            if !matches!(self.w.get(p[0], p[1], p[2]), AIR | LEAVES) {
                break;
            }
            i += s;
        }
        if i == end { -1 } else { i.abs() }
    }

    /// func_519_e.
    fn fits(&mut self) -> bool {
        let [x, y, z] = self.base;
        if !matches!(self.w.get(x, y - 1, z), GRASS | DIRT) {
            return false;
        }
        let n = self.clear([x, y, z], [x, y + self.total - 1, z]);
        if n == -1 {
            true
        } else if n < 6 {
            false
        } else {
            self.total = n;
            true
        }
    }

    /// func_521_a: trunk height and the leaf clusters with where their branch leaves the trunk.
    fn find_nodes(&mut self) {
        self.height = (self.total as f64 * 0.618) as i32;
        if self.height >= self.total {
            self.height = self.total - 1;
        }
        let per = ((1.382 + (self.total as f64 / 13.0).powi(2)) as i32).max(1);
        let [bx, by, bz] = self.base;
        let mut y = by + self.total - SPAN;
        let top = by + self.height;
        let mut level = y - by;
        self.nodes.push([bx, y, bz, top]);
        y -= 1;
        while level >= 0 {
            let rad = self.radius_at(level);
            if rad >= 0.0 {
                for _ in 0..per {
                    let dist = rad as f64 * (self.rnd.next_float() as f64 + 0.328);
                    let ang = self.rnd.next_float() as f64 * 2.0 * 3.14159;
                    let nx = ifloor(dist * ang.sin() + bx as f64 + 0.5);
                    let nz = ifloor(dist * ang.cos() + bz as f64 + 0.5);
                    if self.clear([nx, y, nz], [nx, y + SPAN, nz]) == -1 {
                        let off = ((bx - nx).abs() as f64).powi(2) + ((bz - nz).abs() as f64).powi(2);
                        let drop = off.sqrt() * 0.381;
                        let start = if y as f64 - drop > top as f64 { top } else { (y as f64 - drop) as i32 };
                        if self.clear([bx, start, bz], [nx, y, nz]) == -1 {
                            self.nodes.push([nx, y, nz, start]);
                        }
                    }
                }
            }
            y -= 1;
            level -= 1;
        }
    }

    /// func_523_a with axis 1: a horizontal disc of `id` at height y (only over air/leaves).
    fn disc(&mut self, x: i32, y: i32, z: i32, radius: f32, id: u8) {
        let n = (radius as f64 + 0.618) as i32;
        for a in -n..=n {
            for b in -n..=n {
                let d = (((a.abs() as f64) + 0.5).powi(2) + ((b.abs() as f64) + 0.5).powi(2)).sqrt();
                if d <= radius as f64 && matches!(self.w.get(x + a, y, z + b), AIR | LEAVES) {
                    self.w.set(x + a, y, z + b, id);
                }
            }
        }
    }

    /// func_522_a: a straight line of `id` from a to b, both ends included.
    fn line(&mut self, a: [i32; 3], b: [i32; 3], id: u8) {
        let mut d = [0; 3];
        let mut m = 0;
        for i in 0..3 {
            d[i] = b[i] - a[i];
            if d[i].abs() > d[m].abs() {
                m = i;
            }
        }
        if d[m] == 0 {
            return;
        }
        let (u, v) = (AXES[m], AXES[m + 3]);
        let s = if d[m] > 0 { 1 } else { -1 };
        let (du, dv) = (d[u] as f64 / d[m] as f64, d[v] as f64 / d[m] as f64);
        let (mut i, end) = (0, d[m] + s);
        while i != end {
            let mut p = [0; 3];
            p[m] = ifloor((a[m] + i) as f64 + 0.5);
            p[u] = ifloor(a[u] as f64 + i as f64 * du + 0.5);
            p[v] = ifloor(a[v] as f64 + i as f64 * dv + 0.5);
            self.w.set(p[0], p[1], p[2], id);
            i += s;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference numbers from the real classes (tools/golden/G.java -> golden.txt).
    const GOLDEN: &str = include_str!("../../../../tools/golden/golden.txt");

    fn fnv(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0xcbf29ce484222325, |h, &b| (h ^ b as u64).wrapping_mul(0x100000001b3))
    }

    fn lines<'a>(tag: &'a str) -> impl Iterator<Item = Vec<&'static str>> + 'a {
        GOLDEN.lines().filter(move |l| l.starts_with(tag)).map(|l| l.split(' ').skip(1).collect())
    }

    #[test]
    fn block_tables_match_java() {
        let mut n = 0;
        for f in lines("TAB") {
            let v: Vec<i32> = f.iter().map(|x| x.parse().unwrap()).collect();
            let id = v[0] as u8;
            if id == 60 {
                continue; // farmland: only read by flowers as a ground type, never generated
            }
            assert_eq!((opaque(id), opacity(id) as i32, is_solid(id), is_liquid(id)), (v[1] == 1, v[2], v[3] == 1, v[4] == 1), "block {id}");
            n += 1;
        }
        assert!(n > 30);
    }

    /// Raw chunks (terrain + surface + caves) and populate() of the 2x2 area, both bit for bit.
    #[test]
    fn populate_matches_java() {
        let seeds = [0xCAFEBABE_i64, 12345, -4172144997902289642];
        let mut cases = 0;
        for ((raw, pop), meta) in lines("RAW").zip(lines("POP")).zip(lines("META")) {
            let seed: i64 = raw[0].parse().unwrap();
            let (cx, cz): (i32, i32) = (raw[1].parse().unwrap(), raw[2].parse().unwrap());
            assert!(seeds.contains(&seed));
            let mut cm = WorldChunkManager::new(seed);
            let mut g = OverworldGenerator::new(seed);
            let at = [(cx, cz), (cx, cz + 1), (cx + 1, cz), (cx + 1, cz + 1)];
            let blocks = at.map(|(x, z)| g.generate(x, z, &mut cm));
            for i in 0..4 {
                assert_eq!(format!("{:x}", fnv(&blocks[i])), raw[3 + i], "raw chunk {:?}, seed {seed}", at[i]);
            }
            assert_eq!(meta[..3], raw[..3], "META line out of step with RAW");
            let mut w = Region::new(cx, cz, blocks, std::array::from_fn(|_| Nibbles::new()));
            g.populate(&mut w, cx, cz, &mut cm);
            let (blocks, data) = w.into_parts();
            for i in 0..4 {
                assert_eq!(format!("{:x}", fnv(&blocks[i])), pop[3 + i], "populated chunk {:?}, seed {seed}", at[i]);
                assert_eq!(format!("{:x}", fnv(data[i].bytes())), meta[3 + i], "metadata of chunk {:?}, seed {seed}", at[i]);
            }
            cases += 1;
        }
        assert!(cases >= 10);
    }
}
