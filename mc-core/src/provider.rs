//! Sumber chunk (padanan IChunkProvider.provideChunk). Generator dunia (Fase 5) dan loader disk (Fase 6)
//! mengimplementasikan trait ini. Tahap `populate` (pohon, bijih, danau) ditambahkan bersama generator.

use crate::chunk::Chunk;

pub trait ChunkSource {
    /// Buat atau muat chunk (cx, cz). Skylight awal dihitung oleh `ChunkMap::add_generated_chunk`.
    fn provide(&mut self, cx: i32, cz: i32) -> Chunk;
}

/// Sumber sederhana: dunia kosong (udara). Berguna untuk tes dan untuk dunia "void".
pub struct EmptySource;

impl ChunkSource for EmptySource {
    fn provide(&mut self, cx: i32, cz: i32) -> Chunk {
        Chunk::new(cx, cz, vec![0u8; crate::chunk::VOLUME])
    }
}
