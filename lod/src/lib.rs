//! Distant terrain for a short world (b1.7.3 is 128 blocks tall), std only, no `unsafe`, no threads.
//!
//! Far away only the surface matters, so a far tile is a **height field**, not a voxel volume: one [`Cell`] (4 bytes: top block
//! height, its block id, water level) per column. A level-0 tile is one chunk (16 x 16 columns, 1 KiB); a level-`l` tile covers
//! `2^l x 2^l` chunks with the same 16 x 16 cells, each `2^l` blocks wide, so every level costs the same to store, mesh and draw.
//!
//! - [`Store`]: level-0 tiles in, the coarser levels are built from them on the way ([`Store::set`]), one file ([`Store::save`]).
//! - [`Store::select`]: a quadtree walk that picks, per distance, the coarsest tile whose cells are still small on screen.
//! - [`Store::mesh`]: a tile as axis-aligned [`Quad`]s: merged tops, and walls only where a neighbour is lower.
//!
//! Costs: meshing is O(256) per tile whatever the level; memory is 1 KiB per chunk (+ 1/3 for the coarser levels); a frame draws
//! about 150 tiles. What it cannot show: caves, overhangs, anything under the top block.
//!
//! ponytail: a coarse cell keeps the mean height and the block of its highest column (a ridge sinks, a lone tree fades with
//! distance); the upgrade is a per-cell max height next to the mean. Seams between two levels hang a skirt instead of
//! stitching the vertices.

use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Cells per tile side.
pub const N: usize = 16;
pub const CELLS: usize = N * N;
/// Coarsest level: a tile of `2^3 x 2^3` chunks, 8-block cells.
pub const MAX_LEVEL: u8 = 3;
/// A tile of level `l` is split into its four children while the camera is closer than `DETAIL * 2^l` blocks to it.
pub const DETAIL: f32 = 48.0;
/// Block id drawn for a water cell.
pub const WATER: u8 = 9;
/// Tops and walls sit this far below the block's real top, so they hide under real chunks drawn at the same place.
pub const BIAS: f32 = 0.5;
/// A wall facing a cell nobody knows hangs this many blocks (it hides the crack until the data arrives).
pub const SKIRT: i32 = 16;

/// (level, x, z): a tile. Level 0 coordinates are chunk coordinates; level `l` ones are `chunk >> l`.
pub type Key = (u8, i32, i32);
/// 16 x 16 cells, index `z * N + x`.
pub type Tile = Box<[Cell; CELLS]>;

pub fn empty_tile() -> Tile {
    Box::new([Cell::NONE; CELLS])
}

/// One column: `known` bit 24, water level bits 16..24 (0 = none), block id bits 8..16, ground height bits 0..8.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell(u32);

impl Cell {
    /// Nothing known here.
    pub const NONE: Cell = Cell(0);

    /// `ground`: y of the top solid block, `id` its block, `water`: y of the top water block above it (0 = dry).
    pub fn new(ground: u8, id: u8, water: u8) -> Cell {
        Cell(1 << 24 | (water as u32) << 16 | (id as u32) << 8 | ground as u32)
    }
    pub fn known(self) -> bool {
        self.0 != 0
    }
    pub fn ground(self) -> u8 {
        self.0 as u8
    }
    pub fn id(self) -> u8 {
        (self.0 >> 8) as u8
    }
    pub fn water(self) -> u8 {
        (self.0 >> 16) as u8
    }
    /// y of the highest block that is drawn: the water's when there is water.
    pub fn top(self) -> i32 {
        if self.water() > 0 { self.water() as i32 } else { self.ground() as i32 }
    }
    pub fn top_id(self) -> u8 {
        if self.water() > 0 { WATER } else { self.id() }
    }
    pub fn raw(self) -> u32 {
        self.0
    }
    pub fn from_raw(r: u32) -> Cell {
        Cell(r)
    }
}

/// Four cells (a 2 x 2 patch of the finer level) as one: the mean ground height, the block (and water) of the highest one.
fn reduce(cells: [Cell; 4]) -> Cell {
    let (mut n, mut sum, mut best) = (0u32, 0u32, Cell::NONE);
    for c in cells.into_iter().filter(|c| c.known()) {
        n += 1;
        sum += c.ground() as u32;
        if !best.known() || c.top() > best.top() {
            best = c;
        }
    }
    if n == 0 {
        return Cell::NONE;
    }
    let ground = ((sum + n / 2) / n) as u8;
    Cell::new(ground, best.id(), if best.water() > ground { best.water() } else { 0 })
}

/// A flat rectangle of one block id. `face` is the index of `render::mesh`'s face templates (0 +X, 1 -X, 2 +Y, 4 +Z, 5 -Z):
/// the two corners are equal on the face's own axis. World coordinates.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Quad {
    pub face: u8,
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub id: u8,
}

#[derive(Default)]
pub struct Store {
    tiles: HashMap<Key, Tile>,
    /// Tiles (and the same-level neighbours whose border walls read them) whose mesh is stale: `take_dirty`.
    dirty: HashSet<Key>,
    changed: bool,
}

impl Store {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    /// Is chunk (x, z) known?
    pub fn has(&self, x: i32, z: i32) -> bool {
        self.tiles.contains_key(&(0, x, z))
    }

    pub fn get(&self, k: Key) -> Option<&Tile> {
        self.tiles.get(&k)
    }

    fn touch(&mut self, (l, x, z): Key) {
        for (dx, dz) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
            self.dirty.insert((l, x + dx, z + dz));
        }
    }

    /// Put chunk (x, z)'s tile in and rebuild its ancestors (a missing sibling is unknown cells until it arrives).
    pub fn set(&mut self, x: i32, z: i32, tile: Tile) {
        self.tiles.insert((0, x, z), tile);
        self.touch((0, x, z));
        self.changed = true;
        for l in 1..=MAX_LEVEL {
            let (px, pz) = (x >> l, z >> l);
            let mut up = empty_tile();
            for qz in 0..2i32 {
                for qx in 0..2i32 {
                    let Some(ch) = self.tiles.get(&(l - 1, 2 * px + qx, 2 * pz + qz)) else { continue };
                    for cz in 0..N / 2 {
                        for cx in 0..N / 2 {
                            let at = |dx: usize, dz: usize| ch[(2 * cz + dz) * N + 2 * cx + dx];
                            up[(qz as usize * N / 2 + cz) * N + qx as usize * N / 2 + cx] = reduce([at(0, 0), at(1, 0), at(0, 1), at(1, 1)]);
                        }
                    }
                }
            }
            self.tiles.insert((l, px, pz), up);
            self.touch((l, px, pz));
        }
    }

    pub fn take_dirty(&mut self) -> Vec<Key> {
        self.dirty.drain().collect()
    }

    /// The finest known cell over block column (wx, wz) (`NONE` when no tile covers it).
    pub fn sample(&self, wx: i32, wz: i32) -> Cell {
        for l in 0..=MAX_LEVEL {
            if let Some(t) = self.tiles.get(&(l, wx >> (4 + l), wz >> (4 + l))) {
                let c = t[((wz >> l) & 15) as usize * N + ((wx >> l) & 15) as usize];
                if c.known() {
                    return c;
                }
            }
        }
        Cell::NONE
    }

    /// The tiles to draw from the camera `cam` (x, z in blocks): between `near` blocks (what real chunks already draw; a tile
    /// entirely inside is skipped) and `far`. A tile is replaced by its four children while the camera is closer than
    /// `DETAIL * 2^level`, but only when all four exist, so a half-generated area never has holes.
    pub fn select(&self, cam: (f32, f32), near: f32, far: f32, out: &mut Vec<Key>) {
        out.clear();
        let span = (N as i32) << MAX_LEVEL;
        let r = |v: f32| (v.floor() as i32).div_euclid(span);
        for z in r(cam.1 - far)..=r(cam.1 + far) {
            for x in r(cam.0 - far)..=r(cam.0 + far) {
                self.walk((MAX_LEVEL, x, z), cam, near, far, out);
            }
        }
    }

    fn walk(&self, (l, x, z): Key, cam: (f32, f32), near: f32, far: f32, out: &mut Vec<Key>) {
        let size = ((N as i32) << l) as f32;
        let (bx, bz) = (x as f32 * size, z as f32 * size);
        let (nx, nz) = (cam.0.clamp(bx, bx + size) - cam.0, cam.1.clamp(bz, bz + size) - cam.1);
        let (fx, fz) = ((cam.0 - bx).abs().max((cam.0 - bx - size).abs()), (cam.1 - bz).abs().max((cam.1 - bz - size).abs()));
        let dmin = (nx * nx + nz * nz).sqrt();
        if dmin > far || fx * fx + fz * fz <= near * near {
            return;
        }
        if l > 0 && dmin < DETAIL * (1u32 << l) as f32 {
            let kids = [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(dx, dz)| (l - 1, 2 * x + dx, 2 * z + dz));
            if kids.iter().all(|k| self.tiles.contains_key(k)) {
                kids.into_iter().for_each(|k| self.walk(k, cam, near, far, out));
                return;
            }
        }
        if self.tiles.contains_key(&(l, x, z)) {
            out.push((l, x, z));
        }
    }

    /// A tile as quads: its tops merged into rectangles of one block and height, and a wall wherever the neighbouring column is
    /// lower (down to it) or unknown (a skirt). Walls take the block of the column they belong to.
    pub fn mesh(&self, key: Key) -> Vec<Quad> {
        let Some(tile) = self.tiles.get(&key) else { return Vec::new() };
        let (l, tx, tz) = key;
        let s = 1i32 << l;
        let (x0, z0) = (tx << (4 + l), tz << (4 + l));
        let y_of = |y: i32| y as f32 - BIAS;
        let mut out = Vec::new();

        let mut used = [false; CELLS];
        for cz in 0..N {
            for cx in 0..N {
                let c = tile[cz * N + cx];
                if used[cz * N + cx] || !c.known() {
                    continue;
                }
                let same = |o: Cell| o.known() && o.top() == c.top() && o.top_id() == c.top_id();
                let mut w = 1;
                while cx + w < N && !used[cz * N + cx + w] && same(tile[cz * N + cx + w]) {
                    w += 1;
                }
                let mut d = 1;
                'grow: while cz + d < N {
                    for i in 0..w {
                        let j = (cz + d) * N + cx + i;
                        if used[j] || !same(tile[j]) {
                            break 'grow;
                        }
                    }
                    d += 1;
                }
                for dz in 0..d {
                    for dx in 0..w {
                        used[(cz + dz) * N + cx + dx] = true;
                    }
                }
                let y = y_of(c.top() + 1);
                let (ax, az) = ((x0 + cx as i32 * s) as f32, (z0 + cz as i32 * s) as f32);
                out.push(Quad { face: 2, min: [ax, y, az], max: [ax + (w as i32 * s) as f32, y, az + (d as i32 * s) as f32], id: c.top_id() });
            }
        }

        // Walls: (face, step to the neighbour). Per line of cells along the wall, equal neighbours merge into one quad.
        for (face, dx, dz) in [(0u8, 1i32, 0i32), (1, -1, 0), (4, 0, 1), (5, 0, -1)] {
            for a in 0..N {
                let mut run: Option<(usize, i32, i32, u8)> = None; // (first cell along the line, lo, hi, id)
                for b in 0..=N {
                    let (cx, cz) = if dx != 0 { (a, b) } else { (b, a) };
                    let wall = (b < N).then(|| tile[cz * N + cx]).filter(|c| c.known()).and_then(|c| {
                        let (nx, nz) = (cx as i32 + dx, cz as i32 + dz);
                        let n = if (0..N as i32).contains(&nx) && (0..N as i32).contains(&nz) {
                            tile[nz as usize * N + nx as usize]
                        } else {
                            self.sample(x0 + nx * s, z0 + nz * s)
                        };
                        let hi = c.top() + 1;
                        let lo = if n.known() { n.top() + 1 } else { (hi - SKIRT).max(0) };
                        (lo < hi).then_some((lo, hi, c.top_id()))
                    });
                    if let Some((a0, lo, hi, id)) = run {
                        if wall != Some((lo, hi, id)) {
                            // the wall's plane (the cell's far edge on a + side), and its extent along the line
                            let (across, along) = if dx != 0 { (x0, z0) } else { (z0, x0) };
                            let p = (across + (a as i32 + (dx > 0 || dz > 0) as i32) * s) as f32;
                            let (from, to) = ((along + a0 as i32 * s) as f32, (along + b as i32 * s) as f32);
                            let (min, max) = if dx != 0 { ([p, y_of(lo), from], [p, y_of(hi), to]) } else { ([from, y_of(lo), p], [to, y_of(hi), p]) };
                            out.push(Quad { face, min, max, id });
                            run = None;
                        }
                    }
                    if run.is_none() {
                        run = wall.map(|(lo, hi, id)| (b, lo, hi, id));
                    }
                }
            }
        }
        out
    }

    /// Write the level-0 tiles (the rest is derived) to `path` (via a temp file); nothing when nothing changed since the last save.
    pub fn save(&mut self, path: &Path) -> std::io::Result<()> {
        if !self.changed {
            return Ok(());
        }
        let mut level0: Vec<_> = self.tiles.iter().filter(|(k, _)| k.0 == 0).collect();
        level0.sort_by_key(|(k, _)| (k.1, k.2));
        let mut b = Vec::with_capacity(8 + level0.len() * (8 + CELLS * 4));
        b.extend_from_slice(b"FAR1");
        b.extend_from_slice(&(level0.len() as u32).to_le_bytes());
        for (k, t) in level0 {
            b.extend_from_slice(&k.1.to_le_bytes());
            b.extend_from_slice(&k.2.to_le_bytes());
            t.iter().for_each(|c| b.extend_from_slice(&c.raw().to_le_bytes()));
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, b)?;
        std::fs::rename(&tmp, path)?;
        self.changed = false;
        Ok(())
    }

    /// Read what `save` wrote; a missing or damaged file is an empty store (the tiles are regenerated).
    pub fn load(path: &Path) -> Store {
        let mut s = Store::new();
        let Ok(b) = std::fs::read(path) else { return s };
        let n = b.get(4..8).map_or(0, |c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]) as usize);
        const REC: usize = 8 + CELLS * 4;
        if !b.starts_with(b"FAR1") || n.checked_mul(REC).and_then(|v| v.checked_add(8)) != Some(b.len()) {
            return s;
        }
        let i32_at = |o: usize| i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        for r in 0..n {
            let o = 8 + r * REC;
            let mut t = empty_tile();
            for (i, c) in t.iter_mut().enumerate() {
                *c = Cell::from_raw(i32_at(o + 8 + i * 4) as u32);
            }
            s.set(i32_at(o), i32_at(o + 4), t);
        }
        s.dirty.clear();
        s.changed = false;
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(h: u8, id: u8) -> Tile {
        Box::new([Cell::new(h, id, 0); CELLS])
    }

    /// Four chunks make one level-1 tile whose cells are the 2 x 2 reductions: the mean height, the highest column's block,
    /// water kept only while it is above the mean ground.
    #[test]
    fn ancestors_are_built_from_the_children() {
        let mut s = Store::new();
        s.set(0, 0, flat(60, 2));
        let mut t = flat(60, 2);
        t[0] = Cell::new(70, 1, 0); // one tall stone column in chunk (1, 0)
        s.set(1, 0, t);
        let l1 = s.get((1, 0, 0)).unwrap();
        assert!(l1[0].known() && l1[0].ground() == 60 && l1[0].id() == 2, "chunk (0, 0)'s quadrant");
        assert!(l1[8].ground() == 63 && l1[8].id() == 1, "mean of 70, 60, 60, 60 = 62.5 rounds to 63; the stone is the highest");
        assert!(!l1[8 * N].known(), "the missing chunks stay unknown");
        let sea = reduce([Cell::new(40, 12, 63), Cell::new(40, 12, 63), Cell::new(40, 12, 63), Cell::new(70, 2, 0)]);
        assert_eq!((sea.ground(), sea.id(), sea.water()), (48, 2, 0), "land beats the water beside it");
        let deep = reduce([Cell::new(40, 12, 63); 4]);
        assert_eq!((deep.water(), deep.top_id()), (63, WATER));
    }

    /// A flat tile with no neighbours: one merged top and one skirt per side.
    #[test]
    fn flat_tile_is_five_quads() {
        let mut s = Store::new();
        s.set(0, 0, flat(64, 2));
        let q = s.mesh((0, 0, 0));
        assert_eq!(q.len(), 5, "{q:?}");
        let top = q.iter().find(|q| q.face == 2).unwrap();
        assert_eq!((top.min, top.max), ([0.0, 64.5, 0.0], [16.0, 64.5, 16.0]));
        assert!(q.iter().filter(|q| q.face != 2).all(|q| q.max[1] - q.min[1] == SKIRT as f32));
    }

    /// A step between two columns inside a tile gets exactly one wall, from the lower top up to the higher, on the high side.
    #[test]
    fn a_step_gets_a_wall_on_its_high_side() {
        let mut s = Store::new();
        let mut t = flat(64, 3);
        for z in 0..N {
            t[z * N + 5] = Cell::new(68, 1, 0); // a ridge along z at x = 5
        }
        s.set(0, 0, t);
        let q = s.mesh((0, 0, 0));
        let walls: Vec<_> = q.iter().filter(|q| q.face == 0 && q.min[0] == 6.0).collect(); // +X wall of the ridge: x = 6
        assert_eq!(walls.len(), 1, "one merged wall along the whole ridge: {q:?}");
        assert_eq!((walls[0].min[1], walls[0].max[1], walls[0].id), (65.0 - BIAS, 69.0 - BIAS, 1));
        assert_eq!((walls[0].min[2], walls[0].max[2]), (0.0, 16.0));
        // tops: left of the ridge, the ridge, right of it
        assert_eq!(q.iter().filter(|q| q.face == 2).count(), 3);
    }

    /// Tiles never overlap, nothing lies wholly inside `near`, and every chunk of the ring is covered once it is known.
    #[test]
    fn selection_covers_the_ring_once() {
        let mut s = Store::new();
        for z in -20..20 {
            for x in -20..20 {
                s.set(x, z, flat(64, 2));
            }
        }
        let mut out = Vec::new();
        s.select((8.0, 8.0), 64.0, 224.0, &mut out);
        assert!(out.len() > 20 && out.len() < 400, "{} tiles", out.len());
        let mut seen = HashSet::new();
        for &(l, x, z) in &out {
            for cz in (z << l)..((z + 1) << l) {
                for cx in (x << l)..((x + 1) << l) {
                    assert!(seen.insert((cx, cz)), "chunk ({cx}, {cz}) drawn twice");
                }
            }
        }
        for cz in -13..14i32 {
            for cx in -13..14i32 {
                let (dx, dz) = ((cx * 16 + 8) as f32 - 8.0, (cz * 16 + 8) as f32 - 8.0);
                let d = (dx * dx + dz * dz).sqrt();
                if d > 80.0 && d < 200.0 {
                    assert!(seen.contains(&(cx, cz)), "hole at chunk ({cx}, {cz}), {d} blocks away");
                }
            }
        }
        // a tile of level l > 0 is only picked once the camera is too far to split it
        for &(l, x, z) in out.iter().filter(|k| k.0 > 0) {
            let size = (16 << l) as f32;
            let (nx, nz) = (8.0f32.clamp(x as f32 * size, (x + 1) as f32 * size) - 8.0, 8.0f32.clamp(z as f32 * size, (z + 1) as f32 * size) - 8.0);
            assert!((nx * nx + nz * nz).sqrt() >= DETAIL * (1 << l) as f32 - 0.01, "level {l} tile ({x}, {z}) is too near");
        }
        // a store missing the children draws the coarse tile instead of leaving a hole
        let mut p = Store::new();
        p.set(40, 40, flat(64, 2));
        p.select((40.0 * 16.0 + 8.0, 40.0 * 16.0 + 8.0), 0.0, 400.0, &mut out);
        assert_eq!(out, vec![(MAX_LEVEL, 5, 5)]);
    }

    #[test]
    fn save_and_load_round_trip() {
        let path = std::env::temp_dir().join(format!("lod-test-{}.far", std::process::id()));
        let mut s = Store::new();
        let mut t = flat(70, 12);
        t[17] = Cell::new(40, 12, 63);
        s.set(-3, 5, t);
        s.set(2, -1, flat(64, 2));
        s.save(&path).unwrap();
        let l = Store::load(&path);
        assert_eq!(l.get((0, -3, 5)).unwrap()[17], Cell::new(40, 12, 63));
        assert!(l.get((1, 1, -1)).is_some() && l.get((1, 1, -1)) == s.get((1, 1, -1)), "ancestors rebuilt");
        assert_eq!(l.len(), s.len());
        std::fs::write(&path, b"FAR1\x05\0\0\0junk").unwrap();
        assert!(Store::load(&path).is_empty(), "a damaged file is an empty store");
        let _ = std::fs::remove_file(&path);
    }
}
