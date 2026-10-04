//! Port bagian World: set-block dengan notifikasi, tick terjadwal, kecerahan.
//! Perilaku blok (updateTick, onNeighborBlockChange, onBlockAdded/Removal) didaftarkan di `BlockBehaviors`
//! per ID; sampai fase blok masing-masing ditulis, semuanya no-op.
//! Fungsi perilaku memakai `world.rand` langsung (Java mengoper world.rand sebagai argumen: objek yang sama).
//! Catatan: Java memanggil getChunkFromChunkCoords yang membuat chunk jika belum ada. Provider belum
//! ada di fase ini, jadi chunk yang tidak ada dianggap kosong dan operasi tulis diabaikan.

use std::collections::{BTreeSet, HashSet};

use crate::chunk::HEIGHT;
use crate::collision::CollisionBehaviors;
use crate::dimension::Dimension;
use crate::provider::ChunkSource;
use crate::chunk_map::{ChunkMap, HookEvent, RenderDirty};
use crate::jrandom::JRandom;
use crate::math;

pub type PosFn = fn(&mut World, i32, i32, i32);
/// (world, x, y, z, id blok yang berubah)
pub type NeighborFn = fn(&mut World, i32, i32, i32, i32);

pub struct BlockBehaviors {
    pub update_tick: [Option<PosFn>; 256],
    pub on_neighbor_change: [Option<NeighborFn>; 256],
    pub on_added: [Option<PosFn>; 256],
    pub on_removal: [Option<PosFn>; 256],
}

impl Default for BlockBehaviors {
    fn default() -> Self {
        Self {
            update_tick: [None; 256],
            on_neighbor_change: [None; 256],
            on_added: [None; 256],
            on_removal: [None; 256],
        }
    }
}

/// Java: NextTickListEntry. Identitas (untuk dedup) = x,y,z,id; urutan = waktu lalu nomor urut pembuatan.
#[derive(Clone, Copy, Debug)]
struct TickEntry {
    x: i32,
    y: i32,
    z: i32,
    id: i32,
    time: i64,
    seq: u64,
}

impl PartialEq for TickEntry {
    fn eq(&self, o: &Self) -> bool {
        self.seq == o.seq && self.time == o.time
    }
}
impl Eq for TickEntry {}
impl PartialOrd for TickEntry {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for TickEntry {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.time.cmp(&o.time).then(self.seq.cmp(&o.seq))
    }
}

const STAIR_SINGLE: i32 = 44;
const TILLED_FIELD: i32 = 60;
const STAIR_COBBLE: i32 = 67;
const STAIR_PLANKS: i32 = 53;

pub struct World {
    pub chunks: ChunkMap,
    pub rand: JRandom,
    pub behaviors: BlockBehaviors,
    pub collision: CollisionBehaviors,
    pub dimension: Dimension,
    /// Java: worldProvider.lightBrightnessTable
    pub light_brightness: [f32; 16],
    pub chunk_source: Option<Box<dyn ChunkSource>>,
    pub world_time: i64,
    /// Java: skylightSubtracted (0..11)
    pub skylight_subtracted: i32,
    /// Java: editingBlocks (mematikan notifikasi tetangga selama edit massal, mis. generator)
    pub editing_blocks: bool,
    /// Java: scheduledUpdatesAreImmediate
    pub scheduled_updates_are_immediate: bool,
    tick_tree: BTreeSet<TickEntry>,
    tick_set: HashSet<(i32, i32, i32, i32)>,
    next_seq: u64,
}

impl World {
    pub fn new(has_no_sky: bool, seed: i64) -> Self {
        Self::with_dimension(if has_no_sky { Dimension::Hell } else { Dimension::Surface }, seed)
    }

    pub fn with_dimension(dimension: Dimension, seed: i64) -> Self {
        Self {
            chunks: ChunkMap::new(dimension.has_no_sky()),
            rand: JRandom::new(seed),
            behaviors: BlockBehaviors::default(),
            collision: CollisionBehaviors::default(),
            dimension,
            light_brightness: dimension.light_brightness_table(),
            chunk_source: None,
            world_time: 0,
            skylight_subtracted: 0,
            editing_blocks: false,
            scheduled_updates_are_immediate: false,
            tick_tree: BTreeSet::new(),
            tick_set: HashSet::new(),
            next_seq: 0,
        }
    }

    #[inline]
    pub fn block_id(&self, x: i32, y: i32, z: i32) -> u8 {
        self.chunks.block_id(x, y, z)
    }

    #[inline]
    pub fn block_metadata(&self, x: i32, y: i32, z: i32) -> u8 {
        self.chunks.block_metadata(x, y, z)
    }

    pub fn is_air(&self, x: i32, y: i32, z: i32) -> bool {
        self.block_id(x, y, z) == 0
    }

    // ---------- hook perilaku ----------

    fn dispatch_hook(&mut self, e: HookEvent) {
        match e {
            HookEvent::Removed { id, x, y, z } => {
                if let Some(f) = self.behaviors.on_removal[id as usize] {
                    f(self, x, y, z);
                }
            }
            HookEvent::Added { id, x, y, z } => {
                if let Some(f) = self.behaviors.on_added[id as usize] {
                    f(self, x, y, z);
                }
            }
        }
    }

    /// Java: getChunkFromChunkCoords membuat chunk bila belum ada. Di sini eksplisit lewat `chunk_source`.
    /// Mengembalikan true bila chunk ada setelah panggilan.
    pub fn ensure_chunk(&mut self, cx: i32, cz: i32) -> bool {
        if self.chunks.chunk_exists(cx, cz) {
            return true;
        }
        match self.chunk_source.take() {
            Some(mut src) => {
                let c = src.provide(cx, cz);
                self.chunks.add_generated_chunk(c);
                self.chunk_source = Some(src);
                true
            }
            None => false,
        }
    }

    /// Java: getCelestialAngle
    pub fn celestial_angle(&self, partial: f32) -> f32 {
        self.dimension.celestial_angle(self.world_time, partial)
    }

    /// Java: getLightBrightness (0..1) di posisi blok
    pub fn light_brightness_at(&self, x: i32, y: i32, z: i32) -> f32 {
        self.light_brightness[self.block_light_value(x, y, z).clamp(0, 15) as usize]
    }

    /// Java: getBrightness(x, y, z, minLight)
    pub fn brightness_at(&self, x: i32, y: i32, z: i32, min_light: i32) -> f32 {
        let v = self.block_light_value(x, y, z).max(min_light);
        self.light_brightness[v.clamp(0, 15) as usize]
    }

    fn set_block_inner(&mut self, x: i32, y: i32, z: i32, id: u8, meta: Option<u8>) -> bool {
        if y >= 0 && y < HEIGHT {
            self.ensure_chunk(x >> 4, z >> 4);
        }
        let p = match self.chunks.begin_set(x, y, z, id, meta) {
            Some(p) => p,
            None => return false,
        };
        if p.old_id != 0 && self.chunks.removal_hook_enabled(meta) {
            self.dispatch_hook(HookEvent::Removed { id: p.old_id, x, y, z });
        }
        self.chunks.finish_set(x, y, z, id, meta, p);
        if id != 0 && self.chunks.added_hook_enabled(meta) {
            self.dispatch_hook(HookEvent::Added { id, x, y, z });
        }
        true
    }

    /// Java: setBlock (metadata jadi 0), tanpa notifikasi.
    pub fn set_block(&mut self, x: i32, y: i32, z: i32, id: u8) -> bool {
        self.set_block_inner(x, y, z, id, None)
    }

    /// Java: setBlockAndMetadata, tanpa notifikasi.
    pub fn set_block_and_metadata(&mut self, x: i32, y: i32, z: i32, id: u8, meta: u8) -> bool {
        self.set_block_inner(x, y, z, id, Some(meta))
    }

    pub fn set_block_with_notify(&mut self, x: i32, y: i32, z: i32, id: u8) -> bool {
        if self.set_block(x, y, z, id) {
            self.notify_block_change(x, y, z, id as i32);
            true
        } else {
            false
        }
    }

    pub fn set_block_and_metadata_with_notify(&mut self, x: i32, y: i32, z: i32, id: u8, meta: u8) -> bool {
        if self.set_block_and_metadata(x, y, z, id, meta) {
            self.notify_block_change(x, y, z, id as i32);
            true
        } else {
            false
        }
    }

    /// Java: setBlockMetadata (tanpa notifikasi).
    pub fn set_block_metadata(&mut self, x: i32, y: i32, z: i32, meta: u8) -> bool {
        if !(x >= -32_000_000 && z >= -32_000_000 && x < 32_000_000 && z <= 32_000_000) {
            return false;
        }
        if y < 0 || y >= HEIGHT {
            return false;
        }
        self.ensure_chunk(x >> 4, z >> 4);
        match self.chunks.chunk_mut(x >> 4, z >> 4) {
            Some(c) => {
                c.set_block_metadata((x & 15) as usize, y as usize, (z & 15) as usize, meta);
                true
            }
            None => false,
        }
    }

    /// Java: setBlockMetadataWithNotify. Blok dengan field_28032_t tidak memberi tahu tetangga.
    pub fn set_block_metadata_with_notify(&mut self, x: i32, y: i32, z: i32, meta: u8) {
        if self.set_block_metadata(x, y, z, meta) {
            let id = self.block_id(x, y, z);
            let skip_neighbors = crate::blocks::block(id).map_or(false, |d| d.no_neighbor_notify_on_meta);
            if skip_neighbors {
                self.notify_block_change(x, y, z, id as i32);
            } else {
                self.notify_blocks_of_neighbor_change(x, y, z, id as i32);
            }
        }
    }

    // ---------- notifikasi ----------

    /// Java: notifyBlockChange = markBlockNeedsUpdate + notifyBlocksOfNeighborChange.
    pub fn notify_block_change(&mut self, x: i32, y: i32, z: i32, id: i32) {
        self.mark_block_needs_update(x, y, z);
        self.notify_blocks_of_neighbor_change(x, y, z, id);
    }

    pub fn mark_block_needs_update(&mut self, x: i32, y: i32, z: i32) {
        self.chunks.mark_dirty(RenderDirty::Block { x, y, z });
    }

    pub fn notify_blocks_of_neighbor_change(&mut self, x: i32, y: i32, z: i32, id: i32) {
        self.notify_block_of_neighbor_change(x - 1, y, z, id);
        self.notify_block_of_neighbor_change(x + 1, y, z, id);
        self.notify_block_of_neighbor_change(x, y - 1, z, id);
        self.notify_block_of_neighbor_change(x, y + 1, z, id);
        self.notify_block_of_neighbor_change(x, y, z - 1, id);
        self.notify_block_of_neighbor_change(x, y, z + 1, id);
    }

    fn notify_block_of_neighbor_change(&mut self, x: i32, y: i32, z: i32, changed: i32) {
        if !self.editing_blocks && !self.chunks.multiplayer {
            let id = self.block_id(x, y, z);
            if let Some(f) = self.behaviors.on_neighbor_change[id as usize] {
                f(self, x, y, z, changed);
            }
        }
    }

    // ---------- tick terjadwal ----------

    /// Java: scheduleBlockUpdate(x, y, z, id, delay). Perhatikan: waktu hanya diisi jika id > 0 (apa adanya di Java).
    pub fn schedule_block_update(&mut self, x: i32, y: i32, z: i32, id: i32, delay: i32) {
        let seq = self.next_seq;
        self.next_seq += 1;
        let mut e = TickEntry { x, y, z, id, time: 0, seq };
        if self.scheduled_updates_are_immediate {
            if self.chunks.check_chunks_exist(x - 8, y - 8, z - 8, x + 8, y + 8, z + 8) {
                let cur = self.block_id(x, y, z) as i32;
                if cur == id && cur > 0 {
                    self.run_update_tick(cur as u8, x, y, z);
                }
            }
        } else if self.chunks.check_chunks_exist(x - 8, y - 8, z - 8, x + 8, y + 8, z + 8) {
            if id > 0 {
                e.time = delay as i64 + self.world_time;
            }
            if self.tick_set.insert((x, y, z, id)) {
                self.tick_tree.insert(e);
            }
        }
    }

    fn run_update_tick(&mut self, id: u8, x: i32, y: i32, z: i32) {
        if let Some(f) = self.behaviors.update_tick[id as usize] {
            f(self, x, y, z);
        }
    }

    /// Java: TickUpdates(force). Maksimum 1000 entri per panggilan. true = masih ada antrean.
    pub fn tick_updates(&mut self, force: bool) -> bool {
        let n = self.tick_tree.len().min(1000);
        for _ in 0..n {
            let e = match self.tick_tree.iter().next().copied() {
                Some(e) => e,
                None => break,
            };
            if !force && e.time > self.world_time {
                break;
            }
            self.tick_tree.remove(&e);
            self.tick_set.remove(&(e.x, e.y, e.z, e.id));
            if self.chunks.check_chunks_exist(e.x - 8, e.y - 8, e.z - 8, e.x + 8, e.y + 8, e.z + 8) {
                let cur = self.block_id(e.x, e.y, e.z) as i32;
                if cur == e.id && cur > 0 {
                    self.run_update_tick(cur as u8, e.x, e.y, e.z);
                }
            }
        }
        !self.tick_tree.is_empty()
    }

    pub fn pending_ticks(&self) -> usize {
        self.tick_tree.len()
    }

    // ---------- kecerahan ----------

    /// Java: getFullBlockLightValue (tanpa pengurangan langit).
    pub fn full_block_light_value(&self, x: i32, y: i32, z: i32) -> i32 {
        if y < 0 {
            return 0;
        }
        self.chunks.raw_light_value(x, y.min(HEIGHT - 1), z, 0)
    }

    pub fn block_light_value(&self, x: i32, y: i32, z: i32) -> i32 {
        self.block_light_value_do(x, y, z, true)
    }

    /// Java: getBlockLightValue_do. Tangga/slab/tanah bajak memakai cahaya terbesar di sekitarnya.
    pub fn block_light_value_do(&self, x: i32, y: i32, z: i32, check_special: bool) -> i32 {
        if !(x >= -32_000_000 && z >= -32_000_000 && x < 32_000_000 && z <= 32_000_000) {
            return 15;
        }
        if check_special {
            let id = self.block_id(x, y, z) as i32;
            if id == STAIR_SINGLE || id == TILLED_FIELD || id == STAIR_COBBLE || id == STAIR_PLANKS {
                let up = self.block_light_value_do(x, y + 1, z, false);
                let e = self.block_light_value_do(x + 1, y, z, false);
                let w = self.block_light_value_do(x - 1, y, z, false);
                let s = self.block_light_value_do(x, y, z + 1, false);
                let n = self.block_light_value_do(x, y, z - 1, false);
                return up.max(e).max(w).max(s).max(n);
            }
        }
        if y < 0 {
            return 0;
        }
        self.chunks.raw_light_value(x, y.min(HEIGHT - 1), z, self.skylight_subtracted)
    }

    /// Java: calculateSkylightSubtracted, dengan sudut langit, kekuatan hujan, dan petir sebagai masukan.
    pub fn calculate_skylight_subtracted(celestial_angle: f32, rain: f32, thunder: f32) -> i32 {
        let mut v = 1.0f32 - (math::cos(celestial_angle * std::f32::consts::PI * 2.0f32) * 2.0f32 + 0.5f32);
        v = v.clamp(0.0, 1.0);
        v = 1.0f32 - v;
        v = ((v as f64) * (1.0f64 - ((rain * 5.0f32) as f64) / 16.0f64)) as f32;
        v = ((v as f64) * (1.0f64 - ((thunder * 5.0f32) as f64) / 16.0f64)) as f32;
        v = 1.0f32 - v;
        (v * 11.0f32) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::{Chunk, VOLUME};

    fn flat(has_no_sky: bool) -> World {
        let mut w = World::new(has_no_sky, 1);
        for cx in -2..=2 {
            for cz in -2..=2 {
                let mut b = vec![0u8; VOLUME];
                for x in 0..16usize {
                    for z in 0..16usize {
                        for y in 0..60usize {
                            b[(x << 11) | (z << 7) | y] = 1;
                        }
                    }
                }
                w.chunks.add_generated_chunk(Chunk::new(cx, cz, b));
            }
        }
        w.chunks.flush_lighting();
        w
    }

    fn bump_meta(w: &mut World, x: i32, y: i32, z: i32, _changed: i32) {
        let m = w.block_metadata(x, y, z);
        w.set_block_metadata(x, y, z, m + 1);
    }

    #[test]
    fn notifikasi_tetangga() {
        let mut w = flat(false);
        // pasir (ID 12) di enam sisi (8,70,8) menghitung notifikasi lewat metadata
        w.behaviors.on_neighbor_change[12] = Some(bump_meta);
        for (x, y, z) in [(7, 70, 8), (9, 70, 8), (8, 69, 8), (8, 71, 8), (8, 70, 7), (8, 70, 9)] {
            w.set_block(x, y, z, 12);
        }
        assert!(w.set_block_with_notify(8, 70, 8, 1));
        for (x, y, z) in [(7, 70, 8), (9, 70, 8), (8, 69, 8), (8, 71, 8), (8, 70, 7), (8, 70, 9)] {
            assert_eq!(w.block_metadata(x, y, z), 1);
        }
        // tanpa perubahan -> tidak ada notifikasi
        assert!(!w.set_block_with_notify(8, 70, 8, 1));
        assert_eq!(w.block_metadata(7, 70, 8), 1);
        // editing_blocks mematikan notifikasi
        w.editing_blocks = true;
        w.set_block_with_notify(8, 70, 8, 3);
        assert_eq!(w.block_metadata(7, 70, 8), 1);
    }

    fn mark_tick(w: &mut World, x: i32, y: i32, z: i32) {
        w.set_block_metadata(x, y, z, 9);
    }

    #[test]
    fn tick_terjadwal_urutan_dan_dedup() {
        let mut w = flat(false);
        w.behaviors.update_tick[12] = Some(mark_tick);
        w.set_block(0, 70, 0, 12);
        w.set_block(1, 70, 0, 12);
        w.schedule_block_update(0, 70, 0, 12, 5);
        w.schedule_block_update(0, 70, 0, 12, 5); // duplikat identik: diabaikan
        w.schedule_block_update(1, 70, 0, 12, 2);
        assert_eq!(w.pending_ticks(), 2);
        w.world_time = 1;
        assert!(w.tick_updates(false)); // belum ada yang jatuh tempo
        assert_eq!(w.block_metadata(1, 70, 0), 0);
        w.world_time = 2;
        assert!(w.tick_updates(false)); // hanya (1,70,0) jatuh tempo
        assert_eq!(w.block_metadata(1, 70, 0), 9);
        assert_eq!(w.block_metadata(0, 70, 0), 0);
        w.world_time = 5;
        assert!(!w.tick_updates(false));
        assert_eq!(w.block_metadata(0, 70, 0), 9);
    }

    #[test]
    fn tick_dibatalkan_jika_blok_berganti() {
        let mut w = flat(false);
        w.behaviors.update_tick[12] = Some(mark_tick);
        w.set_block(0, 70, 0, 12);
        w.schedule_block_update(0, 70, 0, 12, 0);
        w.set_block(0, 70, 0, 13); // kerikil: ID tidak cocok lagi
        w.tick_updates(true);
        assert_eq!(w.block_metadata(0, 70, 0), 0);
    }

    #[test]
    fn cahaya_langit_dan_tangga() {
        let mut w = flat(false);
        assert_eq!(w.block_light_value(8, 61, 8), 15);
        w.skylight_subtracted = 4;
        assert_eq!(w.block_light_value(8, 61, 8), 11);
        // tanah bajak (ID 60) memakai nilai terbesar di sekelilingnya
        w.set_block(8, 61, 8, 60);
        w.chunks.flush_lighting();
        assert_eq!(w.block_light_value(8, 61, 8), 11);
    }

    #[test]
    fn skylight_subtracted_siang_malam() {
        // sudut 0.0 = siang: tidak ada pengurangan; sudut 0.5 = tengah malam: maksimum 11 (bila cos(pi)= -1)
        assert_eq!(World::calculate_skylight_subtracted(0.0, 0.0, 0.0), 0);
        assert_eq!(World::calculate_skylight_subtracted(0.5, 0.0, 0.0), 11);
    }
}
