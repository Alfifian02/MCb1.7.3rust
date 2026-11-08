// b1.7.3 WorldChunkManager: temperature / humidity simplex noise + the 2D biome table
// (BiomeGenBase.getBiome). mc-rs replaced this with a multi-noise placement; the original is
// kept here because it is bit-exact against the real Java (golden test).
#pragma once
#include <cstdint>
#include <vector>
#include "blocks.hpp"
#include "noise.hpp"

namespace mc {

enum class Biome : uint8_t {
    Rainforest = 0, Swamp = 1, SeasonalForest = 2, Forest = 3, Savanna = 4,
    Shrubland = 5, Taiga = 6, Desert = 7, Plains = 8, Tundra = 10,
};

inline uint8_t biome_top(Biome b) { return b == Biome::Desert ? block::SAND : block::GRASS; }
inline uint8_t biome_filler(Biome b) { return b == Biome::Desert ? block::SAND : block::DIRT; }

// BiomeGenBase.getBiome(t, h) as built into the table (float maths, like the Java).
inline Biome biome_lookup(float t, float h) {
    h = h * t;
    if (t < 0.1f) return Biome::Tundra;
    if (h < 0.2f) {
        if (t < 0.5f) return Biome::Tundra;
        if (t < 0.95f) return Biome::Savanna;
        return Biome::Desert;
    }
    if (h > 0.5f && t < 0.7f) return Biome::Swamp;
    if (t < 0.5f) return Biome::Taiga;
    if (t < 0.97f) return h < 0.35f ? Biome::Shrubland : Biome::Forest;
    if (h < 0.45f) return Biome::Plains;
    if (h < 0.9f) return Biome::SeasonalForest;
    return Biome::Rainforest;
}

inline Biome biome_from_climate(double temperature, double humidity) {
    int t = static_cast<int>(temperature * 63.0);  // the Java table is a 64 x 64 grid
    int h = static_cast<int>(humidity * 63.0);
    return biome_lookup(static_cast<float>(t) / 63.0f, static_cast<float>(h) / 63.0f);
}

class ClimateManager {
    SimplexOctaves temp_noise_, humid_noise_, extra_noise_;

    static SimplexOctaves make(int64_t seed, int64_t mul, int octaves) {
        JavaRandom r(static_cast<int64_t>(static_cast<uint64_t>(seed) * static_cast<uint64_t>(mul)));
        return SimplexOctaves(r, octaves);
    }

public:
    // x-major (x * 16 + z), filled by load_block_generator_data.
    std::vector<double> temperature, humidity;

    explicit ClimateManager(int64_t seed)
        : temp_noise_(make(seed, 9871, 4)), humid_noise_(make(seed, 39811, 4)), extra_noise_(make(seed, 543321, 2)) {}

    // loadBlockGeneratorData: w == d is the only meaningful case, as in the Java.
    std::vector<Biome> load_block_generator_data(int x, int z, int w, int d) {
        // The Java passes float literals widened to double; these are NOT 0.025 / 0.05 as doubles.
        const double ts = static_cast<double>(0.025f), hs = static_cast<double>(0.05f);
        const double xf = x, zf = z;
        auto temp = temp_noise_.generate(xf, zf, w, w, ts, ts, 0.25);
        auto humid = humid_noise_.generate(xf, zf, w, w, hs, hs, 1.0 / 3.0);
        auto extra = extra_noise_.generate(xf, zf, w, w, 0.25, 0.25, 0.5882352941176471);
        std::vector<Biome> biomes;
        biomes.reserve(static_cast<size_t>(w) * d);
        size_t i = 0;
        for (int a = 0; a < w; a++) {
            for (int b = 0; b < d; b++, i++) {
                double e = extra[i] * 1.1 + 0.5;
                double t = (temp[i] * 0.15 + 0.7) * (1.0 - 0.01) + e * 0.01;
                double h = (humid[i] * 0.15 + 0.5) * (1.0 - 0.002) + e * 0.002;
                t = 1.0 - (1.0 - t) * (1.0 - t);
                t = t < 0.0 ? 0.0 : (t > 1.0 ? 1.0 : t);
                h = h < 0.0 ? 0.0 : (h > 1.0 ? 1.0 : h);
                temp[i] = t;
                humid[i] = h;
                biomes.push_back(biome_from_climate(t, h));
            }
        }
        temperature = std::move(temp);
        humidity = std::move(humid);
        return biomes;
    }
};

}  // namespace mc
