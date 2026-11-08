// b1.7.3 MapGenBase + MapGenCaves: the last step of ChunkProviderGenerate.provideChunk. Port of mc-rs world/gen/caves.rs.
#pragma once
#include <cstdint>

namespace mc {

// Carve caves into chunk (cx, cz); `blocks` is its 16x128x16 array in cell order. b1.7.3 has no ravines.
void carve_caves(int64_t seed, int cx, int cz, uint8_t* blocks);

}  // namespace mc
