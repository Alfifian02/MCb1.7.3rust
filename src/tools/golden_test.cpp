// Checks the C++ port against hashes taken from the real b1.7.3 classes (tools/golden/G.java in the Rust repo).
#include <cstdio>
#include <cstring>
#include "../core/climate.hpp"
#include "../core/terrain.hpp"
#include "golden_raw.inc"

using namespace mc;

static uint64_t fnv(const uint8_t* p, size_t n) {
    uint64_t h = 0xcbf29ce484222325ULL;
    for (size_t i = 0; i < n; i++) { h ^= p[i]; h *= 0x100000001b3ULL; }
    return h;
}
static uint64_t bits(double d) { uint64_t u; std::memcpy(&u, &d, 8); return u; }

static int fails = 0;
#define CHECK(cond, ...) do { if (!(cond)) { fails++; std::printf("FAIL: " __VA_ARGS__); std::printf("\n"); } } while (0)

struct Case { int64_t seed; int cx, cz; uint64_t want[7]; };

int main() {
    // random_matches_java
    { JavaRandom r(-99);
      CHECK(r.next_int() == 1191266223, "next_int");
      CHECK(r.next_int(5) == 0, "bound 5");
      CHECK(r.next_int(16) == 1, "bound 16");
      CHECK(r.next_int(100) == 40, "bound 100");
      CHECK(r.next_long() == 998464440063232087LL, "next_long");
      CHECK(r.next_float() == 0.88211465f, "next_float");
      CHECK(!r.next_boolean(), "next_boolean");
      CHECK(r.next_double() == 0.026367317307998128, "next_double"); }
    // octaves_match_java
    { JavaRandom r(static_cast<int64_t>(0xCAFEBABEll));
      OctaveNoise n(r, 4);
      const double want[] = {-4.720342015445862, -4.649583033718352, -4.582807977351463, -4.952773590280636,
                             -4.894379140217165, -4.837783983592656, -4.7247700475690415, -4.661145753241822,
                             -4.60062249470231, -4.957584999336169, -4.909536679806759, -4.862025341300867};
      auto v = n.generate(10, 20, 30, 2, 3, 2, 0.05, 0.07, 0.09);
      for (int i = 0; i < 12; i++) CHECK(std::fabs(v[i] - want[i]) < 1e-9, "octaves3d[%d] %.15g", i, v[i]);
      CHECK(r.next_int(1000) == 822, "post-noise rng 1");
      CHECK(r.next_int(64) == 9, "post-noise rng 2");
      CHECK(r.next_long() == 2035750212453278413LL, "post-noise rng 3"); }
    // simplex_matches_java
    { JavaRandom r(777);
      SimplexOctaves s(r, 3);
      const double want[] = {0.865514441206283, 0.8893549615037963, 0.8918411455319262,
                             0.9154464514247099, 0.9194438467113024, 0.9427534071340529};
      auto v = s.generate(12.0, -30.0, 3, 2, 0.025, 0.05, 0.25);
      for (int i = 0; i < 6; i++) CHECK(std::fabs(v[i] - want[i]) < 1e-9, "simplex[%d]", i); }
    // matches_java_reference: biomes, climate bits, terrain and surface hashes
    const int64_t S = -4172144997902289642LL;
    const Case cases[] = {
        {S, 3, -5, {0x9c0e1f6aa8bc6325ULL, 0xb6745edd68f89a69ULL, 0xea5bfd643f4df4b0ULL, 0x3fefe077749c032cULL, 0x3fe8c2ee5a718d7cULL, 0x3feffe7c47ef27a0ULL, 0x3fe36f7c15bd5455ULL}},
        {S, 7, 7, {0x625fcbb3471847faULL, 0x1d4d17f848620803ULL, 0x7a6f2c451bdf45adULL, 0x3fef14348077e4ffULL, 0x3fc590dc5fded887ULL, 0x3fee272b89616484ULL, 0x3f805451b6c600b3ULL}},
        {S, -9, 12, {0xc8727fb8ed51d025ULL, 0x962d5e969105894dULL, 0x4357efb289b05be6ULL, 0x3fee9b2c199d1697ULL, 0x3fe364981c5a5245ULL, 0x3feecea5c3aaa326ULL, 0x3fe3bf38c95cf33dULL}},
        {S, 20, -20, {0x9c0e1f6aa8bc6325ULL, 0x6b173dd57b841225ULL, 0x65810d927c5461d7ULL, 0x3fefffc5557a61d0ULL, 0x3fe0501aa1b22356ULL, 0x3feff68079d6f263ULL, 0x3fe6455f9e848ce0ULL}},
    };
    for (const auto& c : cases) {
        ClimateManager cm(c.seed);
        TerrainGenerator g(c.seed);
        g.seed_chunk(c.cx, c.cz);
        auto biomes = cm.load_block_generator_data(c.cx * 16, c.cz * 16, 16, 16);
        std::vector<uint8_t> codes;
        for (auto b : biomes) codes.push_back(static_cast<uint8_t>(b));
        CHECK(fnv(codes.data(), codes.size()) == c.want[0], "biomes (%d,%d)", c.cx, c.cz);
        CHECK(bits(cm.temperature[0]) == c.want[3], "temperature[0] (%d,%d)", c.cx, c.cz);
        CHECK(bits(cm.humidity[0]) == c.want[4], "humidity[0] (%d,%d)", c.cx, c.cz);
        CHECK(bits(cm.temperature[255]) == c.want[5], "temperature[255] (%d,%d)", c.cx, c.cz);
        CHECK(bits(cm.humidity[255]) == c.want[6], "humidity[255] (%d,%d)", c.cx, c.cz);
        std::vector<uint8_t> blocks(CHUNK_VOLUME, block::AIR);
        g.generate_terrain(c.cx, c.cz, blocks.data(), cm.temperature, cm.humidity);
        CHECK(fnv(blocks.data(), blocks.size()) == c.want[1], "terrain (%d,%d)", c.cx, c.cz);
        g.replace_blocks_for_biome(c.cx, c.cz, blocks.data(), biomes);
        CHECK(fnv(blocks.data(), blocks.size()) == c.want[2], "surface (%d,%d)", c.cx, c.cz);
    }
    // raw_chunks_match_java: terrain + surface + caves of 11 cases x 4 chunks (the 2x2 area populate works on).
    for (const auto& c : RAW_CASES) {
        ClimateManager cm(c.seed);
        TerrainGenerator g(c.seed);
        const int at[4][2] = {{c.cx, c.cz}, {c.cx, c.cz + 1}, {c.cx + 1, c.cz}, {c.cx + 1, c.cz + 1}};
        for (int i = 0; i < 4; i++) {
            auto blocks = g.generate(at[i][0], at[i][1], cm);
            CHECK(fnv(blocks.data(), blocks.size()) == c.want[i], "raw+caves seed %lld chunk (%d,%d)",
                  static_cast<long long>(c.seed), at[i][0], at[i][1]);
        }
    }
    std::printf(fails ? "%d FAILED\n" : "all golden checks passed\n", fails);
    return fails ? 1 : 0;
}
