#pragma once
#include <cstdint>

namespace mc {
namespace block {
constexpr uint8_t AIR = 0, STONE = 1, GRASS = 2, DIRT = 3, COBBLE = 4, BEDROCK = 7, WATER_MOVING = 8, WATER = 9,
                  LAVA_MOVING = 10, LAVA = 11, SAND = 12, GRAVEL = 13, SANDSTONE = 24, ICE = 79;
}
constexpr int CHUNK_W = 16, CHUNK_D = 16, CHUNK_H = 128;  // b1.7.3 chunk height
constexpr int SEA_LEVEL = 64;
constexpr int CHUNK_VOLUME = CHUNK_W * CHUNK_H * CHUNK_D;

// Cell order of b1.7.3 (and of the Java golden hashes): x << 11 | z << 7 | y.
inline constexpr int cell(int x, int y, int z) { return (x * CHUNK_D + z) * CHUNK_H + y; }

inline bool is_fluid(uint8_t b) { return b >= block::WATER_MOVING && b <= block::LAVA; }
inline bool is_solid(uint8_t b) { return b != block::AIR && !is_fluid(b); }
}  // namespace mc
