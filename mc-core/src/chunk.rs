//! Port Chunk (data + pencahayaan lokal). Tidak menyimpan referensi ke World:
//! operasi yang di Java memanggil World mengembalikan `ChunkEffect` yang diterapkan `ChunkMap`
//! dengan urutan yang sama seperti Java.
//! Entity dan tile entity ditambahkan di fase berikutnya.

use crate::blocks::LIGHT_OPACITY;
use crate::nibble::NibbleArray;

pub const HEIGHT: i32 = 128;
pub const VOLUME: usize = 16 * 16 * 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkyBlock {
    Sky,
    Block,
}

impl SkyBlock {
    /// Java: EnumSkyBlock.field_1722_c (nilai di luar dunia/vertikal)
    #[inline]
    pub fn default_light(self) -> i32 {
        match self {
            SkyBlock::Sky => 15,
            SkyBlock::Block => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChunkEffect {
    ScheduleLight { kind: SkyBlock, x1: i32, y1: i32, z1: i32, x2: i32, y2: i32, z2: i32 },
    /// Java memanggil markBlocksDirtyVertical dengan x,z LOKAL chunk; nilai dipertahankan apa adanya
    /// (hanya memengaruhi render, bukan logika).
    DirtyVertical { x: i32, z: i32, y_lo: i32, y_hi: i32 },
}

/// Konteks pencarian tinggi kolom di luar chunk ini (Java: world.getHeightValue).
pub struct HeightCtx<'a> {
    /// true jika chunk ini sudah terdaftar di dunia (kolom di dalam chunk dibaca dari heightmap sendiri);
    /// false saat chunk baru dibuat (Java: chunk belum ada di peta, jadi dibaca 0).
    pub self_loaded: bool,
    pub outside: &'a dyn Fn(i32, i32) -> i32,
}

/// Status di antara dua tahap `set_block` (hook onBlockRemoval dipanggil di antaranya).
#[derive(Clone, Copy, Debug)]
pub struct PendingSet {
    pub old_id: u8,
    old_height: u8,
}

#[inline]
fn idx(x: usize, y: usize, z: usize) -> usize {
    (x << 11) | (z << 7) | y
}

#[inline]
fn opacity(id: u8) -> i32 {
    LIGHT_OPACITY[id as usize] as i32
}

pub struct Chunk {
    pub x: i32,
    pub z: i32,
    pub blocks: Vec<u8>,
    pub data: NibbleArray,
    pub skylight: NibbleArray,
    pub blocklight: NibbleArray,
    /// Indeks (z << 4) | x. Nilai = Y blok pertama di atas permukaan tak-transparan.
    pub height_map: [u8; 256],
    pub lowest_block_height: i32,
    pub is_terrain_populated: bool,
    pub is_modified: bool,
    pub never_save: bool,
    /// Java: EmptyChunk (func_21167_h)
    pub empty: bool,
}

impl Chunk {
    pub fn new(x: i32, z: i32, blocks: Vec<u8>) -> Self {
        assert_eq!(blocks.len(), VOLUME);
        Self {
            x,
            z,
            data: NibbleArray::new(VOLUME),
            skylight: NibbleArray::new(VOLUME),
            blocklight: NibbleArray::new(VOLUME),
            blocks,
            height_map: [0; 256],
            lowest_block_height: 0,
            is_terrain_populated: false,
            is_modified: false,
            never_save: false,
            empty: false,
        }
    }

    #[inline]
    pub fn height_value(&self, x: usize, z: usize) -> i32 {
        self.height_map[(z << 4) | x] as i32
    }

    #[inline]
    pub fn block_id(&self, x: usize, y: usize, z: usize) -> u8 {
        self.blocks[idx(x, y, z)]
    }

    #[inline]
    pub fn block_metadata(&self, x: usize, y: usize, z: usize) -> u8 {
        self.data.get(x, y, z)
    }

    pub fn set_block_metadata(&mut self, x: usize, y: usize, z: usize, meta: u8) {
        self.is_modified = true;
        self.data.set(x, y, z, meta);
    }

    #[inline]
    pub fn can_block_see_sky(&self, x: usize, y: i32, z: usize) -> bool {
        y >= self.height_map[(z << 4) | x] as i32
    }

    pub fn saved_light(&self, kind: SkyBlock, x: usize, y: usize, z: usize) -> i32 {
        match kind {
            SkyBlock::Sky => self.skylight.get(x, y, z) as i32,
            SkyBlock::Block => self.blocklight.get(x, y, z) as i32,
        }
    }

    pub fn set_light(&mut self, kind: SkyBlock, x: usize, y: usize, z: usize, v: i32) {
        self.is_modified = true;
        match kind {
            SkyBlock::Sky => self.skylight.set(x, y, z, v as u8),
            SkyBlock::Block => self.blocklight.set(x, y, z, v as u8),
        }
    }

    /// Java: getBlockLightValue (tanpa flag statis `isLit`, itu hanya hint render).
    pub fn block_light_value(&self, x: usize, y: usize, z: usize, sky_subtracted: i32) -> i32 {
        let mut v = self.skylight.get(x, y, z) as i32 - sky_subtracted;
        let b = self.blocklight.get(x, y, z) as i32;
        if b > v {
            v = b;
        }
        v
    }

    /// Java: generateHeightMap
    pub fn generate_height_map(&mut self) {
        let mut lowest = 127;
        for x in 0..16usize {
            for z in 0..16usize {
                let base = (x << 11) | (z << 7);
                let mut y = 127usize;
                while y > 0 && opacity(self.blocks[base + y - 1]) == 0 {
                    y -= 1;
                }
                self.height_map[(z << 4) | x] = y as u8;
                if (y as i32) < lowest {
                    lowest = y as i32;
                }
            }
        }
        self.lowest_block_height = lowest;
        self.is_modified = true;
    }

    /// Java: func_1024_c. Heightmap + skylight awal + jadwal pencahayaan di tepi chunk.
    pub fn init_skylight(&mut self, has_no_sky: bool, ctx: &HeightCtx, fx: &mut Vec<ChunkEffect>) {
        let mut lowest = 127;
        for x in 0..16usize {
            for z in 0..16usize {
                let base = (x << 11) | (z << 7);
                let mut y = 127usize;
                while y > 0 && opacity(self.blocks[base + y - 1]) == 0 {
                    y -= 1;
                }
                self.height_map[(z << 4) | x] = y as u8;
                if (y as i32) < lowest {
                    lowest = y as i32;
                }
                if !has_no_sky {
                    let mut light = 15i32;
                    let mut yy = 127usize;
                    loop {
                        light -= opacity(self.blocks[base + yy]);
                        if light > 0 {
                            self.skylight.set(x, yy, z, light as u8);
                        }
                        yy -= 1; // aman: loop berhenti saat yy == 0 sebelum mengurangi lagi
                        if !(yy > 0 && light > 0) {
                            break;
                        }
                    }
                }
            }
        }
        self.lowest_block_height = lowest;
        for x in 0..16usize {
            for z in 0..16usize {
                self.schedule_neighbor_columns(x, z, ctx, fx);
            }
        }
        self.is_modified = true;
    }

    fn neighbor_height(&self, wx: i32, wz: i32, ctx: &HeightCtx) -> i32 {
        if (wx >> 4) == self.x && (wz >> 4) == self.z {
            if ctx.self_loaded {
                self.height_map[(((wz & 15) as usize) << 4) | (wx & 15) as usize] as i32
            } else {
                0
            }
        } else {
            (ctx.outside)(wx, wz)
        }
    }

    /// Java: func_996_c + func_1020_f
    fn schedule_neighbor_columns(&mut self, x: usize, z: usize, ctx: &HeightCtx, fx: &mut Vec<ChunkEffect>) {
        let h = self.height_value(x, z);
        let wx = self.x * 16 + x as i32;
        let wz = self.z * 16 + z as i32;
        for (nx, nz) in [(wx - 1, wz), (wx + 1, wz), (wx, wz - 1), (wx, wz + 1)] {
            let nh = self.neighbor_height(nx, nz, ctx);
            if nh > h {
                fx.push(ChunkEffect::ScheduleLight { kind: SkyBlock::Sky, x1: nx, y1: h, z1: nz, x2: nx, y2: nh, z2: nz });
                self.is_modified = true;
            } else if nh < h {
                fx.push(ChunkEffect::ScheduleLight { kind: SkyBlock::Sky, x1: nx, y1: nh, z1: nz, x2: nx, y2: h, z2: nz });
                self.is_modified = true;
            }
        }
    }

    /// Java: func_1003_g. Hitung ulang tinggi kolom dan skylight kolom itu.
    fn relight_column(&mut self, x: usize, y: i32, z: usize, fx: &mut Vec<ChunkEffect>) {
        let old = self.height_map[(z << 4) | x] as i32;
        let mut new = old;
        if y > old {
            new = y;
        }
        let base = (x << 11) | (z << 7);
        while new > 0 && opacity(self.blocks[base + new as usize - 1]) == 0 {
            new -= 1;
        }
        if new == old {
            return;
        }
        fx.push(ChunkEffect::DirtyVertical { x: x as i32, z: z as i32, y_lo: new, y_hi: old });
        self.height_map[(z << 4) | x] = new as u8;
        if new < self.lowest_block_height {
            self.lowest_block_height = new;
        } else {
            let mut m = 127;
            for h in self.height_map.iter() {
                if (*h as i32) < m {
                    m = *h as i32;
                }
            }
            self.lowest_block_height = m;
        }
        let wx = self.x * 16 + x as i32;
        let wz = self.z * 16 + z as i32;
        if new < old {
            for yy in new..old {
                self.skylight.set(x, yy as usize, z, 15);
            }
        } else {
            fx.push(ChunkEffect::ScheduleLight { kind: SkyBlock::Sky, x1: wx, y1: old, z1: wz, x2: wx, y2: new, z2: wz });
            for yy in old..new {
                self.skylight.set(x, yy as usize, z, 0);
            }
        }
        let top = new;
        let mut yy = new;
        let mut light = 15i32;
        while yy > 0 && light > 0 {
            yy -= 1;
            let mut op = opacity(self.blocks[base + yy as usize]);
            if op == 0 {
                op = 1;
            }
            light -= op;
            if light < 0 {
                light = 0;
            }
            self.skylight.set(x, yy as usize, z, light as u8);
        }
        while yy > 0 && opacity(self.blocks[base + yy as usize - 1]) == 0 {
            yy -= 1;
        }
        if yy != top {
            fx.push(ChunkEffect::ScheduleLight {
                kind: SkyBlock::Sky, x1: wx - 1, y1: yy, z1: wz - 1, x2: wx + 1, y2: top, z2: wz + 1,
            });
        }
        self.is_modified = true;
    }

    /// Tahap 1 setBlock: tulis ID blok. None jika tidak ada perubahan.
    /// `meta = Some(m)` = setBlockIDWithMetadata; `None` = setBlockID.
    pub fn set_block_begin(&mut self, x: usize, y: usize, z: usize, id: u8, meta: Option<u8>) -> Option<PendingSet> {
        let i = idx(x, y, z);
        let old_id = self.blocks[i];
        let old_height = self.height_map[(z << 4) | x];
        match meta {
            Some(m) => {
                if old_id == id && self.data.get(x, y, z) == m {
                    return None;
                }
            }
            None => {
                if old_id == id {
                    return None;
                }
            }
        }
        self.blocks[i] = id;
        Some(PendingSet { old_id, old_height })
    }

    /// Tahap 2 setBlock (setelah hook onBlockRemoval). Hook onBlockAdded dipanggil pemanggil sesudahnya.
    #[allow(clippy::too_many_arguments)]
    pub fn set_block_finish(
        &mut self, x: usize, y: usize, z: usize, id: u8, meta: Option<u8>, p: PendingSet,
        has_no_sky: bool, ctx: &HeightCtx, fx: &mut Vec<ChunkEffect>,
    ) {
        let wx = self.x * 16 + x as i32;
        let wz = self.z * 16 + z as i32;
        let yi = y as i32;
        let old_h = p.old_height as i32;
        self.data.set(x, y, z, meta.unwrap_or(0));

        // setBlockIDWithMetadata hanya memeriksa has_no_sky; setBlockID tidak (perilaku Java dipertahankan;
        // penjadwalan Sky untuk dunia tanpa langit tetap disaring di ChunkMap::schedule_lighting_update).
        let guard = meta.is_some() && has_no_sky;
        if !guard {
            if opacity(id) != 0 {
                if yi >= old_h {
                    self.relight_column(x, yi + 1, z, fx);
                }
            } else if yi == old_h - 1 {
                self.relight_column(x, yi, z, fx);
            }
            fx.push(ChunkEffect::ScheduleLight { kind: SkyBlock::Sky, x1: wx, y1: yi, z1: wz, x2: wx, y2: yi, z2: wz });
        }
        fx.push(ChunkEffect::ScheduleLight { kind: SkyBlock::Block, x1: wx, y1: yi, z1: wz, x2: wx, y2: yi, z2: wz });
        self.schedule_neighbor_columns(x, z, ctx, fx);
        if let Some(m) = meta {
            self.data.set(x, y, z, m);
        }
        self.is_modified = true;
    }
}
