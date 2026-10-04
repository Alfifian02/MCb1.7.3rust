//! Peta chunk + mesin pencahayaan (port World: getSavedLightValue, setLightValue,
//! neighborLightPropagationChanged, scheduleLightingUpdate, updatingLighting, dan MetadataChunkBlock).
//! Provider (generate/load otomatis) belum ada: chunk yang tidak ada dianggap kosong (ID 0).
//! Penjaga reentrancy Java (lightingUpdatesCounter/Scheduled) tidak dibawa: tidak bisa tercapai di satu thread.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

use crate::blocks::{LIGHT_OPACITY, LIGHT_VALUE};
use crate::chunk::{Chunk, ChunkEffect, HeightCtx, PendingSet, SkyBlock, HEIGHT};

#[derive(Default)]
pub struct FastHasher(u64);

impl Hasher for FastHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }
    fn write_u64(&mut self, i: u64) {
        self.0 = (self.0.rotate_left(5) ^ i).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

type Map = HashMap<u64, Box<Chunk>, BuildHasherDefault<FastHasher>>;

#[inline]
fn key(cx: i32, cz: i32) -> u64 {
    ((cx as u32 as u64) << 32) | (cz as u32 as u64)
}

#[inline]
fn in_range(x: i32, z: i32) -> bool {
    x >= -32_000_000 && z >= -32_000_000 && x < 32_000_000 && z <= 32_000_000
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookEvent {
    /// onBlockRemoval (hanya jika bukan dunia multiplayer)
    Removed { id: u8, x: i32, y: i32, z: i32 },
    /// onBlockAdded
    Added { id: u8, x: i32, y: i32, z: i32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderDirty {
    Block { x: i32, y: i32, z: i32 },
    Vertical { x: i32, z: i32, y_lo: i32, y_hi: i32 },
}

/// Padanan MetadataChunkBlock: kotak area yang perlu dihitung ulang cahayanya.
#[derive(Clone, Copy, Debug)]
struct LightTask {
    kind: SkyBlock,
    x1: i32, y1: i32, z1: i32,
    x2: i32, y2: i32, z2: i32,
}

impl LightTask {
    /// Java: func_866_a (gabung kotak jika hampir sama)
    fn try_merge(&mut self, mut x1: i32, mut y1: i32, mut z1: i32, mut x2: i32, mut y2: i32, mut z2: i32) -> bool {
        if x1 >= self.x1 && y1 >= self.y1 && z1 >= self.z1 && x2 <= self.x2 && y2 <= self.y2 && z2 <= self.z2 {
            return true;
        }
        let t = 1;
        if x1 >= self.x1 - t && y1 >= self.y1 - t && z1 >= self.z1 - t
            && x2 <= self.x2 + t && y2 <= self.y2 + t && z2 <= self.z2 + t
        {
            let (a, b, c) = (self.x2 - self.x1, self.y2 - self.y1, self.z2 - self.z1);
            x1 = x1.min(self.x1);
            y1 = y1.min(self.y1);
            z1 = z1.min(self.z1);
            x2 = x2.max(self.x2);
            y2 = y2.max(self.y2);
            z2 = z2.max(self.z2);
            let (d, e, f) = (x2 - x1, y2 - y1, z2 - z1);
            let old_vol = a.wrapping_mul(b).wrapping_mul(c);
            let new_vol = d.wrapping_mul(e).wrapping_mul(f);
            if new_vol.wrapping_sub(old_vol) <= 2 {
                self.x1 = x1; self.y1 = y1; self.z1 = z1;
                self.x2 = x2; self.y2 = y2; self.z2 = z2;
                return true;
            }
        }
        false
    }

    /// Java: func_4127_a
    fn run(&mut self, w: &mut ChunkMap) {
        let dx = self.x2 - self.x1 + 1;
        let dy = self.y2 - self.y1 + 1;
        let dz = self.z2 - self.z1 + 1;
        if dx.wrapping_mul(dy).wrapping_mul(dz) > 32768 {
            return; // "Light too large, skipping!"
        }
        let kind = self.kind;
        // Cache per kolom-chunk: keberadaan chunk tidak berubah selama satu tugas (aman).
        let mut cache: Option<(i32, i32, bool)> = None;
        for x in self.x1..=self.x2 {
            for z in self.z1..=self.z2 {
                let (cx, cz) = (x >> 4, z >> 4);
                let ok = match cache {
                    Some((a, b, v)) if a == cx && b == cz => v,
                    _ => {
                        let mut v = w.do_chunks_near_chunk_exist(x, 0, z, 1);
                        if v {
                            if let Some(c) = w.chunk(cx, cz) {
                                if c.empty {
                                    v = false;
                                }
                            }
                        }
                        cache = Some((cx, cz, v));
                        v
                    }
                };
                if !ok {
                    continue;
                }
                if self.y1 < 0 {
                    self.y1 = 0;
                }
                if self.y2 >= HEIGHT {
                    self.y2 = HEIGHT - 1;
                }
                for y in self.y1..=self.y2 {
                    let old = w.saved_light_value(kind, x, y, z);
                    let id = w.block_id(x, y, z);
                    let mut op = LIGHT_OPACITY[id as usize] as i32;
                    if op == 0 {
                        op = 1;
                    }
                    let mut emit = 0;
                    match kind {
                        SkyBlock::Sky => {
                            if w.can_existing_block_see_sky(x, y, z) {
                                emit = 15;
                            }
                        }
                        SkyBlock::Block => emit = LIGHT_VALUE[id as usize] as i32,
                    }
                    let new = if op >= 15 && emit == 0 {
                        0
                    } else {
                        let m = w.saved_light_value(kind, x - 1, y, z)
                            .max(w.saved_light_value(kind, x + 1, y, z))
                            .max(w.saved_light_value(kind, x, y - 1, z))
                            .max(w.saved_light_value(kind, x, y + 1, z))
                            .max(w.saved_light_value(kind, x, y, z - 1))
                            .max(w.saved_light_value(kind, x, y, z + 1));
                        let mut v = m - op;
                        if v < 0 {
                            v = 0;
                        }
                        if emit > v {
                            v = emit;
                        }
                        v
                    };
                    if old != new {
                        w.set_light_value(kind, x, y, z, new);
                        let n = (new - 1).max(0);
                        w.neighbor_light_propagation_changed(kind, x - 1, y, z, n);
                        w.neighbor_light_propagation_changed(kind, x, y - 1, z, n);
                        w.neighbor_light_propagation_changed(kind, x, y, z - 1, n);
                        if x + 1 >= self.x2 {
                            w.neighbor_light_propagation_changed(kind, x + 1, y, z, n);
                        }
                        if y + 1 >= self.y2 {
                            w.neighbor_light_propagation_changed(kind, x, y + 1, z, n);
                        }
                        if z + 1 >= self.z2 {
                            w.neighbor_light_propagation_changed(kind, x, y, z + 1, n);
                        }
                    }
                }
            }
        }
    }
}

pub struct ChunkMap {
    chunks: Map,
    /// Java: worldProvider.hasNoSky (Nether = true)
    pub has_no_sky: bool,
    /// Java: world.multiplayerWorld
    pub multiplayer: bool,
    queue: Vec<LightTask>,
    pub track_dirty: bool,
    dirty: Vec<RenderDirty>,
}

impl ChunkMap {
    pub fn new(has_no_sky: bool) -> Self {
        Self {
            chunks: Map::default(),
            has_no_sky,
            multiplayer: false,
            queue: Vec::new(),
            track_dirty: false,
            dirty: Vec::new(),
        }
    }

    /// Catat blok yang perlu digambar ulang (hanya jika pelacakan dinyalakan oleh renderer).
    pub fn mark_dirty(&mut self, d: RenderDirty) {
        if self.track_dirty {
            self.dirty.push(d);
        }
    }

    pub fn take_dirty(&mut self) -> Vec<RenderDirty> {
        std::mem::take(&mut self.dirty)
    }

    pub fn pending_light_tasks(&self) -> usize {
        self.queue.len()
    }

    #[inline]
    pub fn chunk(&self, cx: i32, cz: i32) -> Option<&Chunk> {
        self.chunks.get(&key(cx, cz)).map(|b| &**b)
    }

    #[inline]
    pub fn chunk_exists(&self, cx: i32, cz: i32) -> bool {
        self.chunks.contains_key(&key(cx, cz))
    }

    /// Masukkan chunk yang BARU dibuat generator: skylight awal dihitung SEBELUM chunk terdaftar,
    /// meniru urutan Java (provideChunk memanggil func_1024_c sebelum chunkMap.put).
    pub fn add_generated_chunk(&mut self, mut c: Chunk) {
        let mut fx = Vec::new();
        {
            let me = &*self;
            let outside = |x: i32, z: i32| me.height_value(x, z);
            let ctx = HeightCtx { self_loaded: false, outside: &outside };
            c.init_skylight(self.has_no_sky, &ctx, &mut fx);
        }
        self.apply_effects(fx);
        self.chunks.insert(key(c.x, c.z), Box::new(c));
    }

    /// Masukkan chunk apa adanya (hasil load dari disk).
    pub fn insert_chunk(&mut self, c: Chunk) {
        self.chunks.insert(key(c.x, c.z), Box::new(c));
    }

    pub fn chunk_mut(&mut self, cx: i32, cz: i32) -> Option<&mut Chunk> {
        self.chunks.get_mut(&key(cx, cz)).map(|b| &mut **b)
    }

    pub fn remove_chunk(&mut self, cx: i32, cz: i32) -> Option<Box<Chunk>> {
        self.chunks.remove(&key(cx, cz))
    }

    // ---------- query dasar (Java: World) ----------

    pub fn block_id(&self, x: i32, y: i32, z: i32) -> u8 {
        if !in_range(x, z) || y < 0 || y >= HEIGHT {
            return 0;
        }
        match self.chunk(x >> 4, z >> 4) {
            Some(c) => c.block_id((x & 15) as usize, y as usize, (z & 15) as usize),
            None => 0,
        }
    }

    pub fn block_metadata(&self, x: i32, y: i32, z: i32) -> u8 {
        if !in_range(x, z) || y < 0 || y >= HEIGHT {
            return 0;
        }
        match self.chunk(x >> 4, z >> 4) {
            Some(c) => c.block_metadata((x & 15) as usize, y as usize, (z & 15) as usize),
            None => 0,
        }
    }

    pub fn block_exists(&self, x: i32, y: i32, z: i32) -> bool {
        y >= 0 && y < HEIGHT && self.chunk_exists(x >> 4, z >> 4)
    }

    pub fn check_chunks_exist(&self, x1: i32, y1: i32, z1: i32, x2: i32, y2: i32, z2: i32) -> bool {
        if y2 >= 0 && y1 < HEIGHT {
            for cx in (x1 >> 4)..=(x2 >> 4) {
                for cz in (z1 >> 4)..=(z2 >> 4) {
                    if !self.chunk_exists(cx, cz) {
                        return false;
                    }
                }
            }
            true
        } else {
            false
        }
    }

    pub fn do_chunks_near_chunk_exist(&self, x: i32, y: i32, z: i32, r: i32) -> bool {
        self.check_chunks_exist(x - r, y - r, z - r, x + r, y + r, z + r)
    }

    pub fn height_value(&self, x: i32, z: i32) -> i32 {
        if !in_range(x, z) {
            return 0;
        }
        match self.chunk(x >> 4, z >> 4) {
            Some(c) => c.height_value((x & 15) as usize, (z & 15) as usize),
            None => 0,
        }
    }

    pub fn can_existing_block_see_sky(&self, x: i32, y: i32, z: i32) -> bool {
        if !in_range(x, z) {
            return false;
        }
        if y < 0 {
            return false;
        }
        if y >= HEIGHT {
            return true;
        }
        match self.chunk(x >> 4, z >> 4) {
            Some(c) => c.can_block_see_sky((x & 15) as usize, y, (z & 15) as usize),
            None => false,
        }
    }

    pub fn saved_light_value(&self, kind: SkyBlock, x: i32, y: i32, z: i32) -> i32 {
        let y = y.clamp(0, HEIGHT - 1);
        if in_range(x, z) {
            match self.chunk(x >> 4, z >> 4) {
                Some(c) => c.saved_light(kind, (x & 15) as usize, y as usize, (z & 15) as usize),
                None => 0,
            }
        } else {
            kind.default_light()
        }
    }

    pub fn set_light_value(&mut self, kind: SkyBlock, x: i32, y: i32, z: i32, v: i32) {
        if in_range(x, z) && y >= 0 && y < HEIGHT {
            let track = self.track_dirty;
            if let Some(c) = self.chunks.get_mut(&key(x >> 4, z >> 4)) {
                c.set_light(kind, (x & 15) as usize, y as usize, (z & 15) as usize, v);
                if track {
                    self.dirty.push(RenderDirty::Block { x, y, z });
                }
            }
        }
    }

    /// Java: getBlockLightValue_do tanpa cabang tangga/slab (itu bagian World, fase 4b).
    pub fn raw_light_value(&self, x: i32, y: i32, z: i32, sky_subtracted: i32) -> i32 {
        if !in_range(x, z) || y < 0 || y >= HEIGHT {
            return 0;
        }
        match self.chunk(x >> 4, z >> 4) {
            Some(c) => c.block_light_value((x & 15) as usize, y as usize, (z & 15) as usize, sky_subtracted),
            None => 0,
        }
    }

    // ---------- antrean cahaya ----------

    pub fn neighbor_light_propagation_changed(&mut self, kind: SkyBlock, x: i32, y: i32, z: i32, mut v: i32) {
        if self.has_no_sky && kind == SkyBlock::Sky {
            return;
        }
        if !self.block_exists(x, y, z) {
            return;
        }
        match kind {
            SkyBlock::Sky => {
                if self.can_existing_block_see_sky(x, y, z) {
                    v = 15;
                }
            }
            SkyBlock::Block => {
                let lv = LIGHT_VALUE[self.block_id(x, y, z) as usize] as i32;
                if lv > v {
                    v = lv;
                }
            }
        }
        if self.saved_light_value(kind, x, y, z) != v {
            self.schedule_lighting_update(kind, x, y, z, x, y, z);
        }
    }

    pub fn schedule_lighting_update(&mut self, kind: SkyBlock, x1: i32, y1: i32, z1: i32, x2: i32, y2: i32, z2: i32) {
        self.schedule_lighting_update_do(kind, x1, y1, z1, x2, y2, z2, true);
    }

    pub fn schedule_lighting_update_do(
        &mut self, kind: SkyBlock, x1: i32, y1: i32, z1: i32, x2: i32, y2: i32, z2: i32, merge: bool,
    ) {
        if self.has_no_sky && kind == SkyBlock::Sky {
            return;
        }
        // Java: pembagian int menuju nol
        let mx = (x2 + x1) / 2;
        let mz = (z2 + z1) / 2;
        if !self.block_exists(mx, 64, mz) {
            return;
        }
        if let Some(c) = self.chunk(mx >> 4, mz >> 4) {
            if c.empty {
                return;
            }
        }
        if merge {
            let n = self.queue.len().min(5);
            for i in 0..n {
                let li = self.queue.len() - i - 1;
                let t = &mut self.queue[li];
                if t.kind == kind && t.try_merge(x1, y1, z1, x2, y2, z2) {
                    return;
                }
            }
        }
        self.queue.push(LightTask { kind, x1, y1, z1, x2, y2, z2 });
        if self.queue.len() > 1_000_000 {
            self.queue.clear(); // Java: "More than 1000000 updates, aborting lighting updates"
        }
    }

    /// Java: updatingLighting. Proses sampai 499 tugas per panggilan (LIFO). true = masih ada sisa.
    pub fn update_lighting(&mut self) -> bool {
        let mut budget = 500;
        while !self.queue.is_empty() {
            budget -= 1;
            if budget <= 0 {
                return true;
            }
            let mut t = self.queue.pop().unwrap();
            t.run(self);
        }
        false
    }

    /// Jalankan sampai antrean habis (untuk tes dan pembuatan dunia).
    pub fn flush_lighting(&mut self) {
        while self.update_lighting() {}
    }

    fn apply_effects(&mut self, fx: Vec<ChunkEffect>) {
        for e in fx {
            match e {
                ChunkEffect::ScheduleLight { kind, x1, y1, z1, x2, y2, z2 } => {
                    self.schedule_lighting_update(kind, x1, y1, z1, x2, y2, z2)
                }
                ChunkEffect::DirtyVertical { x, z, y_lo, y_hi } => {
                    if self.track_dirty {
                        self.dirty.push(RenderDirty::Vertical { x, z, y_lo, y_hi });
                    }
                }
            }
        }
    }

    // ---------- ubah blok ----------

    fn with_chunk<R>(&mut self, cx: i32, cz: i32, f: impl FnOnce(&mut Chunk, &HeightCtx) -> R) -> Option<R> {
        let k = key(cx, cz);
        let mut chunk = self.chunks.remove(&k)?;
        let r = {
            let me = &*self;
            let outside = |x: i32, z: i32| me.height_value(x, z);
            let ctx = HeightCtx { self_loaded: true, outside: &outside };
            f(&mut chunk, &ctx)
        };
        self.chunks.insert(k, chunk);
        Some(r)
    }

    /// Java: World.setBlockAndMetadata -> Chunk.setBlockIDWithMetadata (tanpa notifikasi tetangga;
    /// itu ditambahkan di World fase 4b). `hooks` menerima onBlockRemoval/onBlockAdded dengan urutan Java.
    pub fn set_block_and_meta(
        &mut self, x: i32, y: i32, z: i32, id: u8, meta: u8,
        hooks: &mut dyn FnMut(&mut ChunkMap, HookEvent),
    ) -> bool {
        self.set_block_inner(x, y, z, id, Some(meta), hooks)
    }

    /// Java: World.setBlock -> Chunk.setBlockID (metadata dijadikan 0).
    pub fn set_block(
        &mut self, x: i32, y: i32, z: i32, id: u8, hooks: &mut dyn FnMut(&mut ChunkMap, HookEvent),
    ) -> bool {
        self.set_block_inner(x, y, z, id, None, hooks)
    }

    /// Tahap 1 setBlock: tulis ID blok ke chunk. None = tidak ada perubahan / di luar dunia / chunk tidak ada.
    pub fn begin_set(&mut self, x: i32, y: i32, z: i32, id: u8, meta: Option<u8>) -> Option<PendingSet> {
        if !in_range(x, z) || y < 0 || y >= HEIGHT {
            return None;
        }
        let (lx, lz, ly) = ((x & 15) as usize, (z & 15) as usize, y as usize);
        self.with_chunk(x >> 4, z >> 4, |c, _| c.set_block_begin(lx, ly, lz, id, meta)).flatten()
    }

    /// Apakah hook onBlockRemoval dipanggil untuk blok lama (aturan Java berbeda antara dua varian).
    pub fn removal_hook_enabled(&self, meta: Option<u8>) -> bool {
        if meta.is_some() { !self.multiplayer } else { true }
    }

    /// Apakah hook onBlockAdded dipanggil untuk blok baru.
    pub fn added_hook_enabled(&self, meta: Option<u8>) -> bool {
        if meta.is_some() { true } else { !self.multiplayer }
    }

    /// Tahap 2 setBlock (setelah hook onBlockRemoval): heightmap, data, jadwal cahaya.
    pub fn finish_set(&mut self, x: i32, y: i32, z: i32, id: u8, meta: Option<u8>, p: PendingSet) {
        let (lx, lz, ly) = ((x & 15) as usize, (z & 15) as usize, y as usize);
        let has_no_sky = self.has_no_sky;
        let mut fx = Vec::new();
        self.with_chunk(x >> 4, z >> 4, |c, ctx| c.set_block_finish(lx, ly, lz, id, meta, p, has_no_sky, ctx, &mut fx));
        self.apply_effects(fx);
    }

    fn set_block_inner(
        &mut self, x: i32, y: i32, z: i32, id: u8, meta: Option<u8>,
        hooks: &mut dyn FnMut(&mut ChunkMap, HookEvent),
    ) -> bool {
        let pending = match self.begin_set(x, y, z, id, meta) {
            Some(p) => p,
            None => return false,
        };
        if pending.old_id != 0 && self.removal_hook_enabled(meta) {
            hooks(self, HookEvent::Removed { id: pending.old_id, x, y, z });
        }
        self.finish_set(x, y, z, id, meta, pending);
        if id != 0 && self.added_hook_enabled(meta) {
            hooks(self, HookEvent::Added { id, x, y, z });
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::VOLUME;

    fn no_hooks() -> impl FnMut(&mut ChunkMap, HookEvent) {
        |_, _| {}
    }

    /// Dunia datar 3x3 chunk: batu (ID 1) untuk y < 60.
    fn flat_world() -> ChunkMap {
        let mut w = ChunkMap::new(false);
        for cx in -1..=1 {
            for cz in -1..=1 {
                let mut blocks = vec![0u8; VOLUME];
                for x in 0..16usize {
                    for z in 0..16usize {
                        for y in 0..60usize {
                            blocks[(x << 11) | (z << 7) | y] = 1;
                        }
                    }
                }
                w.add_generated_chunk(Chunk::new(cx, cz, blocks));
            }
        }
        w.flush_lighting();
        w
    }

    #[test]
    fn skylight_dan_heightmap_dasar() {
        let w = flat_world();
        assert_eq!(w.height_value(8, 8), 60);
        assert_eq!(w.saved_light_value(SkyBlock::Sky, 8, 100, 8), 15);
        assert_eq!(w.saved_light_value(SkyBlock::Sky, 8, 60, 8), 15);
        assert_eq!(w.saved_light_value(SkyBlock::Sky, 8, 59, 8), 0);
    }

    #[test]
    fn obor_menyala_dan_padam() {
        let mut w = flat_world();
        let mut h = no_hooks();
        assert!(w.set_block_and_meta(8, 61, 8, 50, 0, &mut h)); // obor, cahaya 14
        w.flush_lighting();
        assert_eq!(w.saved_light_value(SkyBlock::Block, 8, 61, 8), 14);
        assert_eq!(w.saved_light_value(SkyBlock::Block, 9, 61, 8), 13);
        assert_eq!(w.saved_light_value(SkyBlock::Block, 8, 61, 11), 11);
        assert!(w.set_block_and_meta(8, 61, 8, 0, 0, &mut h));
        w.flush_lighting();
        assert_eq!(w.saved_light_value(SkyBlock::Block, 8, 61, 8), 0);
        assert_eq!(w.saved_light_value(SkyBlock::Block, 9, 61, 8), 0);
    }

    #[test]
    fn atap_menurunkan_skylight() {
        let mut w = flat_world();
        let mut h = no_hooks();
        assert!(w.set_block_and_meta(8, 70, 8, 1, 0, &mut h));
        w.flush_lighting();
        assert_eq!(w.height_value(8, 8), 71);
        // Di bawah atap, cahaya dari samping: 15 - 1
        assert_eq!(w.saved_light_value(SkyBlock::Sky, 8, 69, 8), 14);
        // Menghapus atap mengembalikan 15
        assert!(w.set_block_and_meta(8, 70, 8, 0, 0, &mut h));
        w.flush_lighting();
        assert_eq!(w.height_value(8, 8), 60);
        assert_eq!(w.saved_light_value(SkyBlock::Sky, 8, 69, 8), 15);
    }

    #[test]
    fn hook_urutan_dan_tanpa_perubahan() {
        let mut w = flat_world();
        let mut log: Vec<HookEvent> = Vec::new();
        let mut h = |_: &mut ChunkMap, e: HookEvent| log.push(e);
        assert!(w.set_block_and_meta(0, 61, 0, 1, 0, &mut h));
        assert!(!w.set_block_and_meta(0, 61, 0, 1, 0, &mut h)); // sama persis: tidak ada perubahan
        assert!(w.set_block_and_meta(0, 61, 0, 3, 0, &mut h));
        assert_eq!(log, vec![
            HookEvent::Added { id: 1, x: 0, y: 61, z: 0 },
            HookEvent::Removed { id: 1, x: 0, y: 61, z: 0 },
            HookEvent::Added { id: 3, x: 0, y: 61, z: 0 },
        ]);
    }

    #[test]
    fn gabung_tugas_cahaya() {
        let mut t = LightTask { kind: SkyBlock::Block, x1: 0, y1: 0, z1: 0, x2: 0, y2: 0, z2: 0 };
        assert!(t.try_merge(0, 0, 0, 0, 0, 0)); // sudah tercakup
        assert!(t.try_merge(1, 0, 0, 1, 0, 0)); // bersebelahan, volume tambahan <= 2
        assert_eq!(t.x2, 1);
    }
}
