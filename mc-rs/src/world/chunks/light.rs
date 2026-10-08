//! M5 light engine: a port of the b1.7.3 lighting in `World` (`scheduleLightingUpdate`,
//! `neighborLightPropagationChanged`, get/setLightValue), `MetadataChunkBlock.func_4127_a` (the region
//! relaxation) and `Chunk` (`generateSkylightMap`, `func_1003_g` relight, `func_996_c` lateral
//! scheduling). Two light kinds per cell: sky (15 straight down from open sky) and block (emitters,
//! `Block.lightValue`). Every update recomputes a box of cells from their six neighbours
//! (`max(neighbours) - max(opacity, 1)`, or the cell's own source level) and queues the neighbours of
//! each cell that changed, until nothing changes. Newest region first, like the Java.
//!
//! Differences from vanilla, none of which change the settled result:
//! - the queue is drained by a per-frame budget (`run_light`) instead of `updatingLighting`'s 500 regions;
//! - a freshly final chunk is lit in one go (`init_chunk`) because worldgen writes raw blocks instead of
//!   calling `setBlockWithNotify`, so no per-block updates were ever queued for them;
//! - no day/night: `skylightSubtracted` is 0 (see `render::mesh`).

use super::{ChunkManager, Key};
use crate::world::chunk::{idx, light_opacity, light_value, H};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Kind {
    Sky,
    Block,
}

/// `MetadataChunkBlock`: a box of cells (inclusive) whose light must be recomputed.
#[derive(Clone, Copy, Debug)]
pub(super) struct Region {
    kind: Kind,
    lo: [i32; 3],
    hi: [i32; 3],
}

impl Region {
    fn volume(&self) -> i32 {
        (0..3).map(|a| self.hi[a] - self.lo[a] + 1).product()
    }

    /// `MetadataChunkBlock.func_866_a`: grow this region to also cover `lo..=hi` if that adds
    /// at most 2 cells of volume. True if it now covers it.
    fn merge(&mut self, lo: [i32; 3], hi: [i32; 3]) -> bool {
        if (0..3).all(|a| lo[a] >= self.lo[a] && hi[a] <= self.hi[a]) {
            return true;
        }
        if !(0..3).all(|a| lo[a] >= self.lo[a] - 1 && hi[a] <= self.hi[a] + 1) {
            return false;
        }
        let old: i32 = (0..3).map(|a| self.hi[a] - self.lo[a]).product();
        let (nlo, nhi): (Vec<i32>, Vec<i32>) = (0..3).map(|a| (lo[a].min(self.lo[a]), hi[a].max(self.hi[a]))).unzip();
        let new: i32 = (0..3).map(|a| nhi[a] - nlo[a]).product();
        if new - old <= 2 {
            self.lo = [nlo[0], nlo[1], nlo[2]];
            self.hi = [nhi[0], nhi[1], nhi[2]];
            return true;
        }
        false
    }
}

/// Regions per frame (`World.updatingLighting`) and a cell cap on top: a region is one hash lookup
/// per neighbour per cell. ponytail: per-chunk lookup cache if this shows in a profile.
const REGIONS_PER_FRAME: usize = 500;
const CELLS_PER_FRAME: i32 = 16_384;

impl ChunkManager {
    fn chunk_exists(&self, x: i32, z: i32) -> bool {
        self.chunks.contains_key(&(x >> 4, z >> 4))
    }

    /// Block id for light maths: air outside the world and in unloaded chunks (`World.getBlockId`).
    fn raw_block(&self, x: i32, y: i32, z: i32) -> u8 {
        self.block_loaded(x, y, z).unwrap_or(0)
    }

    /// `World.getSavedLightValue`: y is clamped into the world, an unloaded chunk is dark.
    pub(super) fn saved_light(&self, kind: Kind, x: i32, y: i32, z: i32) -> u8 {
        let y = y.clamp(0, H as i32 - 1) as usize;
        self.chunks.get(&(x >> 4, z >> 4)).map_or(0, |e| {
            let v = e.light[idx((x & 15) as usize, y, (z & 15) as usize)];
            if kind == Kind::Sky { v & 15 } else { v >> 4 }
        })
    }

    /// Write a nibble without telling the mesher.
    fn put_light(&mut self, kind: Kind, x: i32, y: i32, z: i32, v: u8) {
        let Some(e) = self.chunks.get_mut(&(x >> 4, z >> 4)) else { return };
        let i = idx((x & 15) as usize, y as usize, (z & 15) as usize);
        e.light[i] = if kind == Kind::Sky { e.light[i] & 0xF0 | v } else { e.light[i] & 0x0F | v << 4 };
    }

    /// `World.setLightValue`: write, and the mesh that shows this cell is stale.
    fn set_saved_light(&mut self, kind: Kind, x: i32, y: i32, z: i32, v: u8) {
        if (0..H as i32).contains(&y) && self.chunk_exists(x, z) {
            self.put_light(kind, x, y, z, v);
            self.mark_dirty(x, z);
        }
    }

    /// `World.canExistingBlockSeeTheSky`.
    fn sees_sky(&self, x: i32, y: i32, z: i32) -> bool {
        if y < 0 {
            return false;
        }
        if y >= H as i32 {
            return true;
        }
        self.chunks.get(&(x >> 4, z >> 4)).is_some_and(|e| y >= e.height[((z & 15) << 4 | (x & 15)) as usize] as i32)
    }

    /// `World.getHeightValue`: 0 for an unloaded chunk.
    fn height_at(&self, x: i32, z: i32) -> i32 {
        self.chunks.get(&(x >> 4, z >> 4)).map_or(0, |e| e.height[((z & 15) << 4 | (x & 15)) as usize] as i32)
    }

    /// `World.scheduleLightingUpdate`: queue a region unless the chunk under its middle is missing,
    /// folding it into one of the last 5 queued regions of the same kind when that is nearly free.
    fn schedule_light(&mut self, kind: Kind, lo: [i32; 3], hi: [i32; 3]) {
        if !self.chunk_exists((hi[0] + lo[0]) / 2, (hi[2] + lo[2]) / 2) {
            return;
        }
        if self.light_queue.iter_mut().rev().take(5).any(|r| r.kind == kind && r.merge(lo, hi)) {
            return;
        }
        self.light_queue.push(Region { kind, lo, hi });
        if self.light_queue.len() > 1_000_000 {
            log::warn!("light: more than 1000000 updates, dropping the queue");
            self.light_queue.clear();
        }
    }

    /// `World.neighborLightPropagationChanged`: cell (x, y, z) should now be at least `v` (the changed
    /// neighbour's level minus 1); queue it if its stored level differs from what it should be.
    fn neighbor_changed(&mut self, kind: Kind, x: i32, y: i32, z: i32, mut v: i32) {
        if !(0..H as i32).contains(&y) || !self.chunk_exists(x, z) {
            return;
        }
        match kind {
            Kind::Sky => {
                if self.sees_sky(x, y, z) {
                    v = 15;
                }
            }
            Kind::Block => v = v.max(light_value(self.raw_block(x, y, z)) as i32),
        }
        if self.saved_light(kind, x, y, z) as i32 != v {
            self.schedule_light(kind, [x, y, z], [x, y, z]);
        }
    }

    /// `MetadataChunkBlock.func_4127_a`: recompute every cell of the region, queueing the neighbours of
    /// each cell whose level changed.
    fn update_region(&mut self, mut r: Region) {
        if r.volume() > 32768 {
            log::warn!("light: region too large, skipping");
            return;
        }
        let kind = r.kind;
        let mut near: Option<(Key, bool)> = None; // chunk -> its 3x3 surroundings are loaded
        for x in r.lo[0]..=r.hi[0] {
            for z in r.lo[2]..=r.hi[2] {
                let key = (x >> 4, z >> 4);
                let ok = match near {
                    Some((k, ok)) if k == key => ok,
                    _ => {
                        // World.doChunksNearChunkExist(x, 0, z, 1): chunks of x-1..=x+1, z-1..=z+1.
                        let ok = ((x - 1) >> 4..=(x + 1) >> 4).all(|cx| ((z - 1) >> 4..=(z + 1) >> 4).all(|cz| self.chunks.contains_key(&(cx, cz))));
                        near = Some((key, ok));
                        ok
                    }
                };
                if !ok {
                    continue;
                }
                r.lo[1] = r.lo[1].max(0);
                r.hi[1] = r.hi[1].min(H as i32 - 1);
                for y in r.lo[1]..=r.hi[1] {
                    let old = self.saved_light(kind, x, y, z) as i32;
                    let id = self.raw_block(x, y, z);
                    let opacity = (light_opacity(id) as i32).max(1);
                    let source = match kind {
                        Kind::Sky => if self.sees_sky(x, y, z) { 15 } else { 0 },
                        Kind::Block => light_value(id) as i32,
                    };
                    let new = if opacity >= 15 && source == 0 {
                        0
                    } else {
                        let n = [(-1, 0, 0), (1, 0, 0), (0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1)]
                            .iter()
                            .map(|&(dx, dy, dz)| self.saved_light(kind, x + dx, y + dy, z + dz) as i32)
                            .max()
                            .unwrap_or(0);
                        (n - opacity).max(0).max(source)
                    };
                    if old != new {
                        self.set_saved_light(kind, x, y, z, new as u8);
                        let p = (new - 1).max(0);
                        self.neighbor_changed(kind, x - 1, y, z, p);
                        self.neighbor_changed(kind, x, y - 1, z, p);
                        self.neighbor_changed(kind, x, y, z - 1, p);
                        // The +side neighbours are inside the region (and recomputed by this loop) unless
                        // this is the last cell on that axis.
                        if x + 1 >= r.hi[0] { self.neighbor_changed(kind, x + 1, y, z, p) }
                        if y + 1 >= r.hi[1] { self.neighbor_changed(kind, x, y + 1, z, p) }
                        if z + 1 >= r.hi[2] { self.neighbor_changed(kind, x, y, z + 1, p) }
                    }
                }
            }
        }
    }

    /// Process queued regions, newest first, until the frame budget is spent.
    pub(super) fn run_light(&mut self, max_regions: usize, max_cells: i32) {
        let (mut regions, mut cells) = (0, 0);
        while regions < max_regions && cells < max_cells {
            let Some(r) = self.light_queue.pop() else { break };
            regions += 1;
            cells += r.volume();
            self.update_region(r);
        }
    }

    /// `Chunk.func_996_c`: the column's neighbours may need light from it (or it from them) between the
    /// two heights.
    fn schedule_lateral(&mut self, x: i32, z: i32) {
        let h = self.height_at(x, z);
        for (nx, nz) in [(x - 1, z), (x + 1, z), (x, z - 1), (x, z + 1)] {
            let nh = self.height_at(nx, nz);
            if nh > h {
                self.schedule_light(Kind::Sky, [nx, h, nz], [nx, nh, nz]);
            } else if nh < h {
                self.schedule_light(Kind::Sky, [nx, nh, nz], [nx, h, nz]);
            }
        }
    }

    /// Light bookkeeping of `Chunk.setBlockID` after the cell (x, y, z) became `id`; `h` is the
    /// column's height from before the change.
    pub(super) fn light_after_set(&mut self, x: i32, y: i32, z: i32, id: u8, h: i32) {
        if light_opacity(id) != 0 {
            if y >= h {
                self.relight_column(x, y + 1, z);
            }
        } else if y == h - 1 {
            self.relight_column(x, y, z);
        }
        self.schedule_light(Kind::Sky, [x, y, z], [x, y, z]);
        self.schedule_light(Kind::Block, [x, y, z], [x, y, z]);
        self.schedule_lateral(x, z);
    }

    /// `Chunk.func_1003_g`: the column's height map changed because of an edit at or above `y`; fix
    /// the height, the straight-down skylight, and queue the sideways fallout.
    fn relight_column(&mut self, x: i32, y: i32, z: i32) {
        let Some(e) = self.chunks.get(&(x >> 4, z >> 4)) else { return };
        let (lx, lz) = ((x & 15) as usize, (z & 15) as usize);
        let old = e.height[lz << 4 | lx] as i32;
        let mut h = old.max(y);
        while h > 0 && light_opacity(e.blocks[idx(lx, h as usize - 1, lz)]) == 0 {
            h -= 1;
        }
        if h == old {
            return;
        }
        self.chunks.get_mut(&(x >> 4, z >> 4)).unwrap().height[lz << 4 | lx] = h as u8;
        if h < old {
            (h..old).for_each(|yy| self.put_light(Kind::Sky, x, yy, z, 15));
        } else {
            self.schedule_light(Kind::Sky, [x, old, z], [x, h, z]);
            (old..h).for_each(|yy| self.put_light(Kind::Sky, x, yy, z, 0));
        }
        // Skylight under the new top, losing `opacity` (at least 1) per cell.
        let (mut yy, mut level) = (h, 15);
        while yy > 0 && level > 0 {
            yy -= 1;
            level = (level - (light_opacity(self.raw_block(x, yy, z)) as i32).max(1)).max(0);
            self.put_light(Kind::Sky, x, yy, z, level as u8);
        }
        while yy > 0 && light_opacity(self.raw_block(x, yy - 1, z)) == 0 {
            yy -= 1;
        }
        if yy != h {
            self.schedule_light(Kind::Sky, [x - 1, yy, z - 1], [x + 1, h, z + 1]);
        }
    }

    /// First light of a final chunk: `Chunk.generateSkylightMap` (column skylight from the height
    /// map), the lateral scheduling for every column, every emitter, and the seams with chunks that
    /// are already lit (their light is recomputed across the border both ways).
    fn init_chunk(&mut self, key: Key) {
        let (ox, oz) = (key.0 * 16, key.1 * 16);
        let mut emitters = Vec::new();
        {
            let e = self.chunks.get_mut(&key).unwrap();
            e.light.fill(0);
            for x in 0..16 {
                for z in 0..16 {
                    let base = idx(x, 0, z);
                    let (mut level, mut y) = (15i32, H - 1);
                    loop {
                        level -= light_opacity(e.blocks[base + y]) as i32;
                        if level > 0 {
                            e.light[base + y] |= level as u8;
                        }
                        y -= 1;
                        if !(y > 0 && level > 0) {
                            break;
                        }
                    }
                }
            }
            for (i, &b) in e.blocks.iter().enumerate() {
                if light_value(b) > 0 {
                    emitters.push([ox + (i >> 11) as i32, (i & 127) as i32, oz + ((i >> 7) & 15) as i32]);
                }
            }
            e.lit = true;
        }
        for x in 0..16 {
            for z in 0..16 {
                self.schedule_lateral(ox + x, oz + z);
            }
        }
        for p in emitters {
            self.schedule_light(Kind::Block, p, p);
        }
        // Seams: the border column on each side of every border with a lit neighbour, full height.
        for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            if !self.chunks.get(&(key.0 + dx, key.1 + dz)).is_some_and(|n| n.lit) {
                continue;
            }
            // (x range, z range) of our border column and of the neighbour's facing one.
            let strip = |cx: i32, cz: i32, d: (i32, i32)| {
                let (x0, z0) = (cx * 16, cz * 16);
                let (xs, zs) = match d {
                    (-1, 0) => ((x0, x0), (z0, z0 + 15)),
                    (1, 0) => ((x0 + 15, x0 + 15), (z0, z0 + 15)),
                    (0, -1) => ((x0, x0 + 15), (z0, z0)),
                    _ => ((x0, x0 + 15), (z0 + 15, z0 + 15)),
                };
                ([xs.0, 0, zs.0], [xs.1, H as i32 - 1, zs.1])
            };
            for (k, d) in [(key, (dx, dz)), ((key.0 + dx, key.1 + dz), (-dx, -dz))] {
                let (lo, hi) = strip(k.0, k.1, d);
                self.schedule_light(Kind::Sky, lo, hi);
                self.schedule_light(Kind::Block, lo, hi);
            }
        }
    }

    /// Per frame: when the queue is empty, light the nearest final chunk of the render circle that has
    /// all 8 neighbours loaded, then work the queue for this frame's budget.
    pub(super) fn light_pending(&mut self, cx: i32, cz: i32) {
        if self.light_queue.is_empty() {
            let r2 = self.radius * self.radius;
            let next = self.ring.iter().take_while(|&&(dx, dz)| dx * dx + dz * dz <= r2).map(|&(dx, dz)| (cx + dx, cz + dz)).find(|&(x, z)| {
                self.chunks.get(&(x, z)).is_some_and(|e| !e.lit)
                    && self.is_final(x, z)
                    && (-1..=1).all(|dx| (-1..=1).all(|dz| self.chunks.contains_key(&(x + dx, z + dz))))
            });
            if let Some(k) = next {
                self.init_chunk(k);
            }
        }
        self.run_light(REGIONS_PER_FRAME, CELLS_PER_FRAME);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::chunk::VOLUME;

    /// 3x3 chunks of flat stone up to y = 9, all lit with `init_chunk`, queue drained.
    fn flat() -> ChunkManager {
        let mut m = ChunkManager::new(1, 1);
        for cz in -1..=1 {
            for cx in -1..=1 {
                let mut b = vec![0u8; VOLUME];
                for x in 0..16 {
                    for z in 0..16 {
                        for y in 0..10 {
                            b[idx(x, y, z)] = 1;
                        }
                    }
                }
                m.insert((cx, cz), b);
            }
        }
        for cz in -1..=1 {
            for cx in -1..=1 {
                m.init_chunk((cx, cz));
            }
        }
        settle(&mut m);
        m.chunks.values_mut().for_each(|e| e.meshed = true); // so the test can see what an edit invalidates
        m
    }

    fn settle(m: &mut ChunkManager) {
        while !m.light_queue.is_empty() {
            m.run_light(usize::MAX, i32::MAX);
        }
    }

    /// Sky light: 15 over open ground, 0 inside stone, a roof dims the cell under it by one step of
    /// lateral spread (14), and removing the roof restores 15. Block light: a glowstone lights 15
    /// at the source, falls 1 per cell, crosses a chunk border, and is gone again when the source is removed.
    #[test]
    fn light_settles_after_edits() {
        let mut m = flat();
        let sky = |m: &ChunkManager, x, y, z| m.saved_light(Kind::Sky, x, y, z);
        let blk = |m: &ChunkManager, x, y, z| m.saved_light(Kind::Block, x, y, z);
        assert_eq!((sky(&m, 5, 20, 5), sky(&m, 5, 10, 5), sky(&m, 5, 5, 5)), (15, 15, 0));

        m.set_block(5, 12, 5, 1); // roof one block wide
        settle(&mut m);
        assert_eq!((sky(&m, 5, 12, 5), sky(&m, 5, 11, 5), sky(&m, 5, 13, 5)), (0, 14, 15));
        m.set_block(5, 12, 5, 0);
        settle(&mut m);
        assert_eq!(sky(&m, 5, 11, 5), 15);

        m.set_block(14, 11, 8, 89); // glowstone, 2 cells from the +X chunk border (x = 16)
        settle(&mut m);
        assert_eq!((blk(&m, 14, 11, 8), blk(&m, 15, 11, 8), blk(&m, 17, 11, 8), blk(&m, 24, 11, 8)), (15, 14, 12, 5));
        m.set_block(14, 11, 8, 0);
        settle(&mut m);
        assert_eq!((blk(&m, 14, 11, 8), blk(&m, 15, 11, 8), blk(&m, 17, 11, 8)), (0, 0, 0));
        // Meshes of both touched chunks were marked stale.
        assert!(!m.chunks[&(0, 0)].meshed && !m.chunks[&(1, 0)].meshed);
    }
}
