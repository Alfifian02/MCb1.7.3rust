//! The map item: `ItemMap` (`Item.mapItem`, id 102 + 256 = 358), `MapData`, `MapCoord`, `MapStorage` and the colour decode of
//! `MapItemRenderer`, ported from the decompiled b1.7.3 sources. Everything here is pure (no GPU, no `lib.rs` state): the app feeds it
//! the player, the dimension and a way to read a block column, and draws the result (`render::hud`, `HudPipeline::push_map`).
//!
//! - Recipe 358 (`CraftingManager`): 8 paper round a compass, in `craft::recipes()`. Like the Java, the crafted stack is made with damage 0
//!   and `ItemMap.onCreated` then gives it a new map id (its damage) and a `MapData` centred on where the player stands, scale 3
//!   (1 pixel = 8 blocks, 1024 blocks across). `craft.rs` has no world, so a crafted map leaves it with the damage `FRESH` and
//!   `Maps::settle` does the `onCreated` on the next game tick (see `App::map_tick`).
//! - `ItemMap.onUpdate` (every tick, for each map in the main inventory): `MapData.func_28169_a` sets the player's marker and, for the
//!   map in hand, `ItemMap.func_28011_a` repaints one of 16 interleaved strips of the view (`MapData::scan`).
//! - `MapStorage`: one file per map (`<world>/data/map_<id>`, `MapData::encode`) and the id counter (`<world>/data/idcounts`, the last id
//!   handed out; the first map is 0). NOT NBT, like the rest of `world::save`.
//!
//! ponytail: the Java asks the world for a chunk at every pixel and so generates the whole 1024 block square; here only chunks that are
//! loaded are read, an unloaded pixel keeps what it had (blank until the player has been near). Upgrade: ask the chunk manager to
//! generate (or the LoD columns) for the missing chunks, a few a tick.
//! UNVERIFIED: the `map_N` label `MapItemRenderer` draws at the top left is not drawn (b1.7.3 shows none that anyone remembers).

use std::collections::HashMap;
use std::path::PathBuf;

use crate::world::items::ItemStack;
use crate::world::mapcolors::{COLOR, LIQUID, OPACITY};
use crate::world::save;

/// `Item.mapItem.shiftedIndex`.
pub const MAP: u16 = 358;
/// The damage of a map that was just crafted and has no id yet (a real id is a `short`, so this never clashes).
pub const FRESH: u16 = u16::MAX;
/// `MapColor.mapColorArray[i].colorValue`.
const MAP_COLORS: [u32; 14] = [0, 8368696, 16247203, 10987431, 16711680, 10526975, 10987431, 31744, 16777215, 10791096, 12020271, 7368816, 4210943, 6837042];
/// `MapColor.waterColor.colorIndex`.
const WATER: u8 = 12;
/// Pixels on a side (`MapData.field_28176_f` is 128 x 128 bytes).
pub const SIDE: usize = 128;

/// A crafted map has not been through `ItemMap.onCreated` yet.
pub fn fresh(s: &mut ItemStack) {
    if s.id == MAP {
        s.damage = FRESH;
    }
}

/// `MapCoord`: a marker on the map. `x` and `z` are half pixels from the centre, `rot` sixteenths of a turn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Marker {
    /// The cell of `mapicons.png` (4 x 4 cells of 8 x 8): 0 is the player's arrow.
    pub icon: u8,
    pub x: i8,
    pub z: i8,
    pub rot: i8,
}

/// `MapData`.
pub struct MapData {
    /// `field_28180_b` / `field_28179_c`: the block at the middle of the map.
    pub x_center: i32,
    pub z_center: i32,
    /// `field_28178_d`: `WorldProvider.worldType`, 0 the overworld, -1 the Nether.
    pub dimension: i8,
    /// `field_28177_e`: 0..=4, one pixel is `1 << scale` blocks.
    pub scale: u8,
    /// `field_28176_f`: per pixel `colour index * 4 + shade` (shade 0 dark, 1 normal, 2 light), 0 = nothing seen.
    pub colors: Vec<u8>,
    /// `field_28175_g`: how often `scan` ran (picks the strip, spins the Nether's marker).
    pub tick: i32,
    /// `field_28173_i`: the markers of this tick.
    pub markers: Vec<Marker>,
    /// `MapDataBase.dirty`: not saved yet.
    pub dirty: bool,
    /// Bumped whenever a pixel changes: the renderer uploads the texture again when it differs from the one it has.
    pub version: u32,
}

impl MapData {
    fn new() -> Self {
        Self { x_center: 0, z_center: 0, dimension: 0, scale: 3, colors: vec![0; SIDE * SIDE], tick: 0, markers: Vec::new(), dirty: false, version: 1 }
    }

    /// `MapData.writeToNBT` in this port's own bytes: `MAP1`, dimension, centre x and z, scale, the 16384 colours.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Vec::with_capacity(14 + SIDE * SIDE);
        w.extend_from_slice(b"MAP1");
        w.push(self.dimension as u8);
        w.extend_from_slice(&self.x_center.to_le_bytes());
        w.extend_from_slice(&self.z_center.to_le_bytes());
        w.push(self.scale);
        w.extend_from_slice(&self.colors);
        w
    }

    /// `MapData.readFromNBT`: the scale is clamped to 0..=4. `None` for a file that is not this format or the wrong size.
    pub fn decode(b: &[u8]) -> Option<Self> {
        let rest = b.strip_prefix(b"MAP1")?;
        if rest.len() != 10 + SIDE * SIDE {
            return None;
        }
        let (x, z) = (i32::from_le_bytes(rest[1..5].try_into().ok()?), i32::from_le_bytes(rest[5..9].try_into().ok()?));
        Some(Self { x_center: x, z_center: z, dimension: rest[0] as i8, scale: rest[9].min(4), colors: rest[10..].to_vec(), ..Self::new() })
    }

    /// `MapData.func_28169_a`, for the one local player: the arrow on the map (none when the player is dead, outside the map or in
    /// another dimension). `yaw` is `EntityPlayer.rotationYaw` in degrees.
    pub fn track(&mut self, alive: bool, player_dim: i8, (px, pz): (f64, f64), yaw: f32) {
        self.markers.clear();
        if !alive {
            return;
        }
        let k = (1u32 << self.scale) as f32;
        let (fx, fz) = ((px - self.x_center as f64) as f32 / k, (pz - self.z_center as f64) as f32 / k);
        if !(-64.0..=64.0).contains(&fx) || !(-64.0..=64.0).contains(&fz) {
            return;
        }
        let (x, z) = ((fx * 2.0) as f64 + 0.5, (fz * 2.0) as f64 + 0.5);
        let mut rot = ((yaw * 16.0 / 360.0) as f64 + 0.5) as i32 as i8;
        if self.dimension < 0 {
            // A map of the Nether has no north: the arrow spins (`34187121` is the Java's hash constant, the maths wraps like `int`).
            let t = self.tick / 10;
            rot = (t.wrapping_mul(t).wrapping_mul(34187121).wrapping_add(t.wrapping_mul(121)) >> 15 & 15) as i8;
        }
        if player_dim == self.dimension {
            self.markers.push(Marker { icon: 0, x: x as i32 as i8, z: z as i32 as i8, rot });
        }
    }

    /// `ItemMap.func_28011_a`: repaint the strip of pixels whose column `& 15` is this tick's, in a disc of `128 / 2^scale` pixels round the
    /// player (half of it in the Nether, which has no sky). `col(x, z)` is the 128 blocks of the column at block `x`, `z` (index = y), or
    /// `None` when its chunk is not loaded (that pixel is left alone, see the module doc).
    pub fn scan<'a>(&mut self, world_dim: i8, no_sky: bool, (px, pz): (f64, f64), col: impl Fn(i32, i32) -> Option<&'a [u8]>) {
        if world_dim != self.dimension {
            return;
        }
        let side = SIDE as i32;
        let k = 1i32 << self.scale;
        let (cx, cz) = (self.x_center, self.z_center);
        let x0 = (px - cx as f64).floor() as i32 / k + side / 2;
        let z0 = (pz - cz as f64).floor() as i32 / k + side / 2;
        let mut r = 128 / k;
        if no_sky {
            r /= 2;
        }
        self.tick = self.tick.wrapping_add(1);
        for x in x0 - r + 1..x0 + r {
            if x & 15 != self.tick & 15 {
                continue;
            }
            let (mut lo, mut hi) = (255i32, 0i32);
            let mut prev: Option<f64> = None; // `var15`: the height of the pixel above
            for z in z0 - r - 1..z0 + r {
                if !(x >= 0 && z >= -1 && x < side && z < side) {
                    continue;
                }
                let (dx, dz) = (x - x0, z - z0);
                let edge = dx * dx + dz * dz > (r - 2) * (r - 2);
                let (bx, bz) = ((cx / k + x - side / 2) * k, (cz / k + z - side / 2) * k);
                let mut count = [0i32; 256];
                let (mut depth, mut height) = (0i32, 0.0f64);
                if no_sky {
                    let mut h = bx.wrapping_add(bz.wrapping_mul(231871));
                    h = h.wrapping_mul(h).wrapping_mul(31287121).wrapping_add(h.wrapping_mul(11));
                    count[if h >> 20 & 1 == 0 { 3 } else { 1 }] += 10; // dirt, stone
                    height = 100.0;
                } else {
                    if col(bx, bz).is_none() {
                        prev = None;
                        continue;
                    }
                    for sx in 0..k {
                        for sz in 0..k {
                            let Some(c) = col(bx + sx, bz + sz) else { continue };
                            let (top, id) = surface(c, &mut depth);
                            height += top as f64 / (k * k) as f64;
                            count[id as usize] += 1;
                        }
                    }
                }
                depth /= k * k;
                // The commonest block of the pixel (the first of equals, lowest id).
                let (mut best, mut id) = (0, 0usize);
                for (i, &n) in count.iter().enumerate() {
                    if n > best {
                        (best, id) = (n, i);
                    }
                }
                let parity = ((x + z) & 1) as f64;
                let mut slope = (height - prev.unwrap_or(height)) * 4.0 / (k + 4) as f64 + (parity - 0.5) * 0.4;
                let mut shade = if slope > 0.6 { 2 } else if slope < -0.6 { 0 } else { 1 };
                let mut color = 0u8;
                if id > 0 {
                    color = COLOR[id];
                    if color == WATER {
                        slope = depth as f64 * 0.1 + parity * 0.2;
                        shade = if slope < 0.5 { 2 } else if slope > 0.9 { 0 } else { 1 };
                    }
                }
                prev = Some(height);
                if z >= 0 && dx * dx + dz * dz < r * r && (!edge || (x + z) & 1 != 0) {
                    let (i, v) = (x as usize + z as usize * SIDE, color * 4 + shade);
                    if self.colors[i] != v {
                        (lo, hi) = (lo.min(z), hi.max(z));
                        self.colors[i] = v;
                    }
                }
            }
            if lo <= hi {
                self.dirty = true; // `func_28170_a`
                self.version = self.version.wrapping_add(1);
            }
        }
    }

    /// `MapItemRenderer.func_28157_a`'s colour loop: the map as 128 x 128 RGBA bytes. A pixel nobody has seen is a faint dark checker
    /// (alpha 16 / 24) so the paper shows through. The anaglyph grey is not ported (the option is stored only).
    pub fn rgba(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(SIDE * SIDE * 4);
        for (i, &c) in self.colors.iter().enumerate() {
            let (idx, shade) = ((c / 4) as usize, c & 3);
            if idx == 0 || idx >= MAP_COLORS.len() {
                out.extend_from_slice(&[0, 0, 0, (((i + i / SIDE) & 1) * 8 + 16) as u8]);
                continue;
            }
            let v = MAP_COLORS[idx];
            let m = match shade {
                2 => 255,
                0 => 180,
                _ => 220,
            };
            out.extend_from_slice(&[((v >> 16 & 255) * m / 255) as u8, ((v >> 8 & 255) * m / 255) as u8, ((v & 255) * m / 255) as u8, 255]);
        }
        out
    }
}

/// One column of one pixel in `ItemMap.func_28011_a`: the height of the first block that is not clear of colour, going down from the
/// light-blocking top (`Chunk.getHeightValue` + 1), and its id; `depth` gets one for every block of the water under a surface.
fn surface(c: &[u8], depth: &mut i32) -> (i32, u8) {
    let get = |y: i32| if (0..c.len() as i32).contains(&y) { c[y as usize] } else { 0 };
    // `Chunk.generateHeightMap`.
    let mut h = 127i32;
    while h > 0 && OPACITY[get(h - 1) as usize] == 0 {
        h -= 1;
    }
    let mut y = h + 1;
    let mut id = 0u8;
    if y > 1 {
        loop {
            id = get(y - 1);
            // A block that is air, or a material with the air colour (glass, a plant's circuits, fire...), is looked through.
            if id == 0 || COLOR[id as usize] == 0 {
                y -= 1;
                id = get(y - 1);
                if y <= 0 {
                    break; // not in the Java (it reads the column next door): the bottom of the world ends the walk
                }
                continue;
            }
            if LIQUID[id as usize] == 0 {
                break;
            }
            // Water: how deep (every liquid block under the surface counts one).
            let mut yy = y - 1;
            loop {
                let b = get(yy);
                yy -= 1;
                *depth += 1;
                if yy <= 0 || b == 0 || LIQUID[b as usize] == 0 {
                    break;
                }
            }
            break;
        }
    }
    (y, id)
}

/// `MapStorage` for the maps of one world (the overworld's folder: both dimensions share the id counter, like the Java's one save handler).
#[derive(Default)]
pub struct Maps {
    dir: Option<PathBuf>,
    maps: HashMap<u16, MapData>,
    /// `idCounts["map"]`: the last id handed out, `None` before the first.
    last: Option<u16>,
}

impl Maps {
    /// The maps in `<world>/data`; `None` (a test) keeps them in memory only.
    pub fn open(dir: Option<PathBuf>) -> Self {
        let last = dir.as_ref().and_then(|d| std::fs::read(d.join("idcounts")).ok()).and_then(|b| Some(u16::from_le_bytes(b.get(..2)?.try_into().ok()?)));
        Self { dir, maps: HashMap::new(), last }
    }

    /// `MapStorage.getUniqueDataId("map")`: 0 the first time, then one more each, written to `idcounts` at once.
    pub fn unique_id(&mut self) -> u16 {
        let id = self.last.map_or(0, |l| l.wrapping_add(1));
        self.last = Some(id);
        if let Some(d) = &self.dir {
            let _ = std::fs::create_dir_all(d);
            if let Err(e) = save::write(&d.join("idcounts"), &id.to_le_bytes()) {
                log::warn!("map: idcounts: {e}");
            }
        }
        id
    }

    /// `MapStorage.loadData`: in memory, else read from its file.
    fn has(&mut self, id: u16) -> bool {
        if self.maps.contains_key(&id) {
            return true;
        }
        let loaded = self.dir.as_ref().and_then(|d| std::fs::read(d.join(format!("map_{id}"))).ok()).and_then(|b| MapData::decode(&b));
        loaded.map(|m| self.maps.insert(id, m)).is_some()
    }

    /// The data of map `id` if it exists (memory or disk).
    pub fn get(&mut self, id: u16) -> Option<&mut MapData> {
        self.has(id).then(|| self.maps.get_mut(&id)).flatten()
    }

    /// `ItemMap.onCreated`: a new id for `s` and its data, centred on the player's block, scale 3. Does nothing unless `s` is `fresh`.
    pub fn settle(&mut self, s: &mut ItemStack, (x, z): (f64, f64), dim: i8) -> bool {
        if s.id != MAP || s.damage != FRESH {
            return false;
        }
        let id = self.unique_id();
        s.damage = id;
        let mut m = MapData::new();
        (m.x_center, m.z_center, m.dimension, m.dirty) = (x.floor() as i32, z.floor() as i32, dim, true);
        self.maps.insert(id, m);
        true
    }

    /// `ItemMap.func_28012_a`: the data of the map in `s`; a map whose data is missing (made by a command, or its file is gone) gets
    /// a new id and a fresh one centred on the world spawn.
    pub fn data_for(&mut self, s: &mut ItemStack, spawn: (i32, i32), dim: i8) -> &mut MapData {
        if !self.has(s.damage) {
            s.damage = self.unique_id();
            let mut m = MapData::new();
            (m.x_center, m.z_center, m.dimension, m.dirty) = (spawn.0, spawn.1, dim, true);
            self.maps.insert(s.damage, m);
        }
        self.maps.entry(s.damage).or_insert_with(MapData::new)
    }

    /// `MapStorage.saveAllData`: every changed map to its file.
    pub fn save_all(&mut self) {
        let Some(d) = &self.dir else { return };
        for (id, m) in self.maps.iter_mut().filter(|(_, m)| m.dirty) {
            let _ = std::fs::create_dir_all(d);
            match save::write(&d.join(format!("map_{id}")), &m.encode()) {
                Ok(()) => m.dirty = false,
                Err(e) => log::warn!("map: map_{id}: {e}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flat world: stone to y 61, grass at 62, air above (`col` of every block).
    fn flat() -> Vec<u8> {
        (0..128).map(|y| if y < 62 { 1 } else if y == 62 { 2 } else { 0 }).collect()
    }

    fn stack() -> ItemStack {
        let mut s = ItemStack { id: MAP, count: 1, damage: 0 };
        fresh(&mut s);
        s
    }

    #[test]
    fn colours_decode_like_the_item_renderer() {
        let mut m = MapData::new();
        m.colors[1] = 1 * 4 + 2; // grass, light
        m.colors[2] = 1 * 4 + 1; // grass, normal
        m.colors[3] = 1 * 4; // grass, dark
        let p = m.rgba();
        assert_eq!(p.len(), SIDE * SIDE * 4);
        assert_eq!(&p[0..4], &[0, 0, 0, 16], "unseen, even cell of the checker");
        assert_eq!(&p[4..8], &[0x7F, 0xB2, 0x38, 255], "grass 8368696 = 0x7FB238 at 255 / 255");
        assert_eq!(&p[8..12], &[0x7F * 220 / 255, 0xB2 * 220 / 255, 0x38 * 220 / 255, 255]);
        assert_eq!(&p[12..16], &[0x7F * 180 / 255, 0xB2 * 180 / 255, 0x38 * 180 / 255, 255]);
        assert_eq!(p[(SIDE + 1) * 4 + 3], 16, "row 1, column 1: (i + i / 128) is even again");
        assert_eq!(p[SIDE * 4 + 3], 24, "row 1, column 0: odd");
    }

    #[test]
    fn a_map_survives_a_file() {
        let mut m = MapData::new();
        (m.x_center, m.z_center, m.dimension, m.scale) = (-1234, 99, -1, 2);
        m.colors[77] = 50;
        let b = m.encode();
        let r = MapData::decode(&b).expect("decodes");
        assert_eq!((r.x_center, r.z_center, r.dimension, r.scale, r.colors[77]), (-1234, 99, -1, 2, 50));
        assert!(MapData::decode(&b[..b.len() - 1]).is_none() && MapData::decode(b"nope").is_none());
        let mut big = b.clone();
        big[13] = 9; // the scale byte (4 + 1 + 4 + 4)
        assert_eq!(MapData::decode(&big).map(|m| m.scale), Some(4), "readFromNBT clamps the scale");
    }

    #[test]
    fn ids_count_up_from_zero_and_come_back_from_disk() {
        let dir = std::env::temp_dir().join(format!("mcrs_map_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut maps = Maps::open(Some(dir.clone()));
        let (mut a, mut b) = (stack(), stack());
        assert!(maps.settle(&mut a, (10.7, -3.2), 0) && maps.settle(&mut b, (0.0, 0.0), 0));
        assert_eq!((a.damage, b.damage), (0, 1), "the first map is 0");
        assert!(!maps.settle(&mut a, (0.0, 0.0), 0), "a settled map is left alone");
        let d = maps.get(0).expect("data");
        assert_eq!((d.x_center, d.z_center, d.scale, d.dimension), (10, -4, 3, 0), "the block under the player, floored");
        d.colors[5] = 9;
        maps.save_all();
        let mut again = Maps::open(Some(dir.clone()));
        assert_eq!(again.get(0).map(|m| m.colors[5]), Some(9));
        assert_eq!(again.unique_id(), 2, "idcounts is read back");
        assert!(again.get(7).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_map_without_data_is_made_at_the_spawn() {
        let mut maps = Maps::open(None);
        let mut s = ItemStack { id: MAP, count: 1, damage: 40 };
        let m = maps.data_for(&mut s, (100, 200), 0);
        assert_eq!((m.x_center, m.z_center, m.scale), (100, 200, 3));
        assert_eq!(s.damage, 0, "func_28012_a gives it the next id");
    }

    #[test]
    fn the_scan_paints_grass_in_a_disc_and_the_arrow_sits_at_the_middle() {
        let c = flat();
        let mut maps = Maps::open(None);
        let mut s = stack();
        maps.settle(&mut s, (0.0, 0.0), 0);
        let m = maps.get(s.damage).expect("data");
        for _ in 0..16 {
            m.scan(0, false, (0.5, 0.5), |_, _| Some(&c[..])); // every strip once
        }
        let grass = |i: usize| (m.colors[i] / 4, m.colors[i] & 3);
        assert_eq!(grass(64 + 64 * SIDE), (1, 1), "level ground is grass, normal shade");
        assert_eq!(m.colors[0], 0, "far outside the disc nothing is drawn");
        assert_eq!(m.colors[64 + 120 * SIDE], 0, "64 pixels is the radius");
        let seen = m.colors.iter().filter(|&&c| c != 0).count();
        assert_eq!(seen, 697, "a disc of radius 16 pixels (128 / 2^3 / ... = `var11`), its two outer rings drawn in a checker");
        assert!(m.dirty && m.version > 1);
        m.track(true, 0, (0.5, 0.5), 90.0);
        assert_eq!(m.markers, vec![Marker { icon: 0, x: 0, z: 0, rot: 4 }], "half a block off the middle is under one half pixel; 90 degrees is 4 sixteenths");
        m.track(true, 0, (80.0, -40.0), 0.0); // 80 / 8 = 10 pixels east, 5 north: 20 and -10 half pixels (+ 0.5, truncated)
        assert_eq!(m.markers, vec![Marker { icon: 0, x: 20, z: -9, rot: 0 }]);
        m.track(true, -1, (0.5, 0.5), 0.0);
        assert!(m.markers.is_empty(), "another dimension has no arrow");
        m.track(false, 0, (0.5, 0.5), 0.0);
        assert!(m.markers.is_empty(), "nor does a dead player");
        m.track(true, 0, (5000.0, 0.0), 0.0);
        assert!(m.markers.is_empty(), "nor one off the map");
    }

    #[test]
    fn water_is_blue_and_deeper_water_is_darker() {
        let shallow: Vec<u8> = (0..128).map(|y| if y < 60 { 1 } else if y < 63 { 9 } else { 0 }).collect();
        let deep: Vec<u8> = (0..128).map(|y| if y < 20 { 1 } else if y < 63 { 9 } else { 0 }).collect();
        let mut d1 = 0;
        let mut d2 = 0;
        assert_eq!(surface(&shallow, &mut d1), (63, 9));
        assert_eq!(surface(&deep, &mut d2), (63, 9));
        assert!(d2 > d1 && d1 > 0, "{d1} {d2}");
        assert_eq!(COLOR[9], WATER);
    }

    #[test]
    fn glass_and_plants_are_looked_through_and_an_unloaded_chunk_is_left_alone() {
        let mut c = flat();
        c[63] = 20; // glass: the air colour, and it does not block light, so the height map ignores it
        c[64] = 31; // tall grass: opacity 0 as well, so it sits above where the walk starts and a map never shows plants
        let mut d = 0;
        let (y, id) = surface(&c, &mut d);
        assert_eq!((y, id), (63, 2), "starts above the grass block (y 62: height 63, walk from 64), looks through the glass, stops on the grass");
        let mut m = MapData::new();
        for _ in 0..16 {
            m.scan(0, false, (0.5, 0.5), |_, _| None);
        }
        assert!(m.colors.iter().all(|&c| c == 0) && !m.dirty);
    }

    #[test]
    fn the_nether_map_is_a_dirt_and_stone_hash_and_its_arrow_spins() {
        let mut m = MapData::new();
        m.dimension = -1;
        for _ in 0..16 {
            m.scan(-1, true, (0.0, 0.0), |_, _| None);
        }
        let seen: Vec<u8> = m.colors.iter().filter(|&&c| c != 0).map(|c| c / 4).collect();
        assert!(!seen.is_empty() && seen.iter().all(|&c| c == 10 || c == 11), "dirt or stone");
        m.track(true, -1, (0.0, 0.0), 0.0);
        let r1 = m.markers[0].rot;
        m.tick += 10;
        m.track(true, -1, (0.0, 0.0), 0.0);
        assert!((0..16).contains(&r1) && (0..16).contains(&m.markers[0].rot));
        m.scan(0, false, (0.0, 0.0), |_, _| None); // the wrong dimension: nothing happens (tick unchanged)
        assert_eq!(m.tick, 26, "16 scans + 10");
    }
}
