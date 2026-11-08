// b1.7.3 ChunkProviderGenerate: generateTerrain + replaceBlocksForBiome (no caves / populate yet).
// Port of mc-rs world/gen/overworld.rs (Beta terrain path).
#pragma once
#include <cstdint>
#include <memory>
#include <unordered_map>
#include <vector>
#include "blocks.hpp"
#include "climate.hpp"
#include "noise.hpp"

namespace mc {

class TerrainGenerator {
    int64_t seed_;
    JavaRandom rand_;
    OctaveNoise noise_lim_, noise_low_, noise_base_, noise_sand_, noise_stone_, noise_main_, noise_height_;
    struct Col {
        double main, height;
        double base[17], lim[17], low[17];
    };
    std::unordered_map<uint64_t, Col> cols_;  // noise columns by absolute grid (x, z), shared by neighbours

    static OctaveNoise first(JavaRandom& r, int n) { return OctaveNoise(r, n); }
    const Col& column(int gx, int gz);
    void density(int x, int z, const std::vector<double>& temps, const std::vector<double>& hums, double* out);

public:
    explicit TerrainGenerator(int64_t seed);

    // The two passes, exposed separately so the golden test can hash between them.
    // Chunk RNG seed (provideChunk).
    void seed_chunk(int cx, int cz);
    void generate_terrain(int cx, int cz, uint8_t* blocks, const std::vector<double>& temps,
                          const std::vector<double>& hums);
    void replace_blocks_for_biome(int cx, int cz, uint8_t* blocks, const std::vector<Biome>& biomes);

    // provideChunk: one 16x128x16 chunk in cell order (x << 11 | z << 7 | y).
    std::vector<uint8_t> generate(int cx, int cz, ClimateManager& cm);
};

}  // namespace mc
