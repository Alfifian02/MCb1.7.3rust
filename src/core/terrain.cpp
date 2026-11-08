#include "terrain.hpp"
#include "caves.hpp"
#include <algorithm>

namespace mc {

namespace {
constexpr int SY = 17;

uint64_t col_key(int gx, int gz) {
    return (static_cast<uint64_t>(static_cast<uint32_t>(gx)) << 32) | static_cast<uint32_t>(gz);
}

// The 17 densities of one grid column (func_4061_a's inner loop). `h` is humidity x temperature there.
// Source arrays are `double[17]`.
struct ColView {
    double main, height;
    const double *base, *lim, *low;
};

void column_density(const ColView& c, double h, double* out) {
    double v25 = 1.0 - h;
    v25 *= v25;
    v25 *= v25;
    v25 = 1.0 - v25;
    double v27 = (c.main + 256.0) / 512.0;
    v27 *= v25;
    if (v27 > 1.0) v27 = 1.0;
    double v29 = c.height / 8000.0;
    if (v29 < 0.0) v29 = -v29 * 0.3;
    v29 = v29 * 3.0 - 2.0;
    if (v29 < 0.0) {
        v29 /= 2.0;
        if (v29 < -1.0) v29 = -1.0;
        v29 /= 1.4;
        v29 /= 2.0;
        v27 = 0.0;
    } else {
        if (v29 > 1.0) v29 = 1.0;
        v29 /= 8.0;
    }
    if (v27 < 0.0) v27 = 0.0;
    v27 += 0.5;
    v29 = v29 * SY / 16.0;
    const double v31 = SY / 2.0 + v29 * 4.0;
    for (int iy = 0; iy < SY; iy++) {
        double v36 = (iy - v31) * 12.0 / v27;
        if (v36 < 0.0) v36 *= 4.0;
        double lim = c.lim[iy] / 512.0;
        double low = c.low[iy] / 512.0;
        double mix = (c.base[iy] / 10.0 + 1.0) / 2.0;
        double v34 = mix < 0.0 ? lim : (mix > 1.0 ? low : lim + (low - lim) * mix);
        v34 -= v36;
        if (iy > SY - 4) {
            // The Java divides in float here.
            double v44 = static_cast<double>(static_cast<float>(iy - (SY - 4)) / 3.0f);
            v34 = v34 * (1.0 - v44) + -10.0 * v44;
        }
        out[iy] = v34;
    }
}
}  // namespace

TerrainGenerator::TerrainGenerator(int64_t seed)
    : seed_(seed),
      rand_(seed),
      // Order matters: every stack draws from the same Random (members are initialised in declaration order).
      noise_lim_(rand_, 16),
      noise_low_(rand_, 16),
      noise_base_(rand_, 8),
      noise_sand_(rand_, 4),
      noise_stone_(rand_, 4),
      noise_main_(rand_, 10),
      noise_height_(rand_, 16) {}

const TerrainGenerator::Col& TerrainGenerator::column(int gx, int gz) {
    auto it = cols_.find(col_key(gx, gz));
    if (it != cols_.end()) return it->second;
    const double xf = gx, zf = gz;
    Col c{};
    noise_main_.generate_2d_into(&c.main, gx, gz, 1, 1, 1.121, 1.121);
    noise_height_.generate_2d_into(&c.height, gx, gz, 1, 1, 200.0, 200.0);
    noise_base_.generate_into(c.base, xf, 0.0, zf, 1, SY, 1, 684.412 / 80.0, 684.412 / 160.0, 684.412 / 80.0);
    noise_lim_.generate_into(c.lim, xf, 0.0, zf, 1, SY, 1, 684.412, 684.412, 684.412);
    noise_low_.generate_into(c.low, xf, 0.0, zf, 1, SY, 1, 684.412, 684.412, 684.412);
    return cols_.emplace(col_key(gx, gz), c).first->second;
}

void TerrainGenerator::density(int x, int z, const std::vector<double>& temps, const std::vector<double>& hums,
                               double* out) {
    constexpr int SX = 5, SZ = 5;
    if (cols_.size() + SX * SZ > 4096) cols_.clear();  // dropped whole, never mid-fill
    int i = 0;
    for (int ix = 0; ix < SX; ix++) {
        const int bx = ix * (16 / SX) + (16 / SX) / 2;
        for (int iz = 0; iz < SZ; iz++) {
            const int bz = iz * (16 / SX) + (16 / SX) / 2;
            const double t = temps[bx * 16 + bz];
            const double h = hums[bx * 16 + bz] * t;
            const Col& c = column(x + ix, z + iz);
            ColView v{c.main, c.height, c.base, c.lim, c.low};
            column_density(v, h, out + i);
            i += SY;
        }
    }
}

void TerrainGenerator::seed_chunk(int cx, int cz) {
    rand_.set_seed(static_cast<int64_t>(static_cast<uint64_t>(static_cast<int64_t>(cx)) * 341873128712ULL +
                                        static_cast<uint64_t>(static_cast<int64_t>(cz)) * 132897987541ULL));
}

// generateTerrain: interpolate the 5x17x5 density grid into stone / water / ice. The incremental
// `+=` interpolation is kept as in the Java so the sign of the density matches bit for bit.
void TerrainGenerator::generate_terrain(int cx, int cz, uint8_t* blocks, const std::vector<double>& temps,
                                        const std::vector<double>& hums) {
    double d[5 * 17 * 5];
    density(cx * 4, cz * 4, temps, hums, d);
    auto at = [&](int i, int j, int k) { return d[(i * 5 + j) * 17 + k]; };
    for (int i = 0; i < 4; i++)
        for (int j = 0; j < 4; j++)
            for (int k = 0; k < 16; k++) {
                double v16 = at(i, j, k), v18 = at(i, j + 1, k), v20 = at(i + 1, j, k), v22 = at(i + 1, j + 1, k);
                const double v24 = (at(i, j, k + 1) - v16) * 0.125;
                const double v26 = (at(i, j + 1, k + 1) - v18) * 0.125;
                const double v28 = (at(i + 1, j, k + 1) - v20) * 0.125;
                const double v30 = (at(i + 1, j + 1, k + 1) - v22) * 0.125;
                for (int l = 0; l < 8; l++) {
                    const int y = k * 8 + l;
                    double v35 = v16, v37 = v18;
                    const double v39 = (v20 - v16) * 0.25;
                    const double v41 = (v22 - v18) * 0.25;
                    for (int m = 0; m < 4; m++) {
                        int pos = cell(m + i * 4, y, j * 4);
                        double v48 = v35;
                        const double v50 = (v37 - v35) * 0.25;
                        for (int n = 0; n < 4; n++) {
                            const double t = temps[(i * 4 + m) * 16 + j * 4 + n];
                            uint8_t b = block::AIR;
                            if (y < SEA_LEVEL) b = (t < 0.5 && y >= SEA_LEVEL - 1) ? block::ICE : block::WATER;
                            if (v48 > 0.0) b = block::STONE;
                            blocks[pos] = b;
                            pos += CHUNK_H;
                            v48 += v50;
                        }
                        v35 += v39;
                        v37 += v41;
                    }
                    v16 += v24;
                    v18 += v26;
                    v20 += v28;
                    v22 += v30;
                }
            }
}

// replaceBlocksForBiome: grass / dirt / sand / gravel skin and the bedrock floor.
void TerrainGenerator::replace_blocks_for_biome(int cx, int cz, uint8_t* blocks, const std::vector<Biome>& biomes) {
    const double s = 1.0 / 32.0;
    const double bx = cx * 16, bz = cz * 16;
    double sand_n[256], gravel_n[256], stone_n[256];
    // Sand and gravel both use noise_sand; gravel samples an x/z plane, sand an x/y plane (z fixed at 0). Java quirk.
    noise_sand_.generate_into(sand_n, bx, bz, 0.0, 16, 16, 1, s, s, 1.0);
    noise_sand_.generate_into(gravel_n, bx, 109.0134, bz, 16, 1, 16, s, 1.0, s);
    noise_stone_.generate_into(stone_n, bx, bz, 0.0, 16, 16, 1, s * 2.0, s * 2.0, s * 2.0);
    const int sea = SEA_LEVEL;
    for (int a = 0; a < 16; a++) {
        for (int b = 0; b < 16; b++) {
            const Biome biome = biomes[a + b * 16];
            const bool sand = sand_n[a + b * 16] + rand_.next_double() * 0.2 > 0.0;
            const bool gravel = gravel_n[a + b * 16] + rand_.next_double() * 0.2 > 3.0;
            const int stone = static_cast<int>(stone_n[a + b * 16] / 3.0 + 3.0 + rand_.next_double() * 0.25);
            int depth = -1;
            uint8_t top = biome_top(biome), filler = biome_filler(biome);
            uint8_t* col = blocks + (b * 16 + a) * CHUNK_H;
            for (int y = CHUNK_H - 1; y >= 0; y--) {
                // The draw is made for every y, as in the Java: it advances the Random.
                if (y <= rand_.next_int(5)) {
                    col[y] = block::BEDROCK;
                    continue;
                }
                const uint8_t cur = col[y];
                if (cur == block::AIR) {
                    depth = -1;
                } else if (cur == block::STONE) {
                    if (depth == -1) {
                        if (stone <= 0) {
                            top = block::AIR;
                            filler = block::STONE;
                        } else if (y >= sea - 4 && y <= sea + 1) {
                            top = biome_top(biome);
                            filler = biome_filler(biome);
                            if (gravel) top = block::AIR;
                            if (gravel) filler = block::GRAVEL;
                            if (sand) top = block::SAND;
                            if (sand) filler = block::SAND;
                        }
                        if (y < sea && top == block::AIR) top = block::WATER;
                        depth = stone;
                        col[y] = y >= sea - 1 ? top : filler;
                    } else if (depth > 0) {
                        depth--;
                        col[y] = filler;
                        if (depth == 0 && filler == block::SAND) {
                            depth = rand_.next_int(4);
                            filler = block::SANDSTONE;
                        }
                    }
                }
            }
        }
    }
}

std::vector<uint8_t> TerrainGenerator::generate(int cx, int cz, ClimateManager& cm) {
    seed_chunk(cx, cz);
    auto biomes = cm.load_block_generator_data(cx * 16, cz * 16, 16, 16);
    std::vector<uint8_t> blocks(CHUNK_VOLUME, block::AIR);
    generate_terrain(cx, cz, blocks.data(), cm.temperature, cm.humidity);
    replace_blocks_for_biome(cx, cz, blocks.data(), biomes);
    carve_caves(seed_, cx, cz, blocks.data());
    return blocks;
}

}  // namespace mc
