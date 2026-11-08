// Loaded chunks, owned by the main thread. Workers only ever see immutable shared_ptr<const Chunk>.
#pragma once
#include <cstdint>
#include <memory>
#include <unordered_map>
#include <vector>
#include "blocks.hpp"

namespace mc {

struct Chunk {
    int cx, cz;
    std::vector<uint8_t> blocks;  // cell order x << 11 | z << 7 | y
};
using ChunkPtr = std::shared_ptr<const Chunk>;

constexpr uint8_t UNLOADED = 0xFF;

inline int64_t chunk_key(int cx, int cz) {
    return static_cast<int64_t>((static_cast<uint64_t>(static_cast<uint32_t>(cx)) << 32) | static_cast<uint32_t>(cz));
}
inline int chunk_key_x(int64_t k) { return static_cast<int32_t>(static_cast<uint32_t>(static_cast<uint64_t>(k) >> 32)); }
inline int chunk_key_z(int64_t k) { return static_cast<int32_t>(static_cast<uint32_t>(static_cast<uint64_t>(k))); }
inline int floor_div16(int v) { return v >> 4; }  // arithmetic shift = floor for negatives

class World {
public:
    std::unordered_map<int64_t, ChunkPtr> chunks;

    ChunkPtr get_chunk(int cx, int cz) const {
        auto it = chunks.find(chunk_key(cx, cz));
        return it == chunks.end() ? nullptr : it->second;
    }

    // Block id at world coordinates; UNLOADED if the chunk is not here, AIR above the world, STONE below it.
    uint8_t block_at(int x, int y, int z) const {
        if (y < 0) return block::STONE;
        if (y >= CHUNK_H) return block::AIR;
        auto c = get_chunk(floor_div16(x), floor_div16(z));
        if (!c) return UNLOADED;
        return c->blocks[cell(x & 15, y, z & 15)];
    }

    // Highest non-air, non-fluid block of a column, or -1 if the chunk is not loaded / the column is empty.
    int top_solid(int x, int z) const {
        auto c = get_chunk(floor_div16(x), floor_div16(z));
        if (!c) return -1;
        for (int y = CHUNK_H - 1; y >= 0; y--)
            if (is_solid(c->blocks[cell(x & 15, y, z & 15)])) return y;
        return -1;
    }
};

}  // namespace mc
