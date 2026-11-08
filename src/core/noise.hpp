// b1.7.3 noise: NoiseGeneratorPerlin/Octaves (3D, terrain) and NoiseGenerator2/Octaves2 (2D simplex, climate).
// Port of mc-rs world/gen/noise.rs. All stacks draw from ONE shared Random in construction order.
// Array layout is x-major: (ix * sz + iz) * sy + iy.
#pragma once
#include <array>
#include <cstdint>
#include <vector>
#include "random.hpp"

namespace mc {

int32_t ifloor(double v);  // MathHelper.floor_double
float mh_sin(float x);     // MathHelper.sin: the 65536-entry float table (coarser than std::sin)
float mh_cos(float x);     // MathHelper.cos

class PerlinNoise {
    int32_t perm_[512];
    double xc_, yc_, zc_;
    int32_t p(int32_t i) const { return perm_[i & 511]; }

public:
    explicit PerlinNoise(JavaRandom& rng);
    // Add this octave into `out` (func_805_a). Samples are divided by `amp`.
    void add(double* out, double x, double y, double z, int sx, int sy, int sz, double dx, double dy, double dz,
             double amp) const;
};

class OctaveNoise {
    std::vector<PerlinNoise> gens_;

public:
    OctaveNoise(JavaRandom& rng, int octaves);
    void generate_into(double* out, double x, double y, double z, int sx, int sy, int sz, double dx, double dy,
                       double dz) const;
    std::vector<double> generate(double x, double y, double z, int sx, int sy, int sz, double dx, double dy,
                                 double dz) const;
    // x/z-only variant (sy = 1, y fixed at 10).
    void generate_2d_into(double* out, int x, int z, int sx, int sz, double dx, double dz) const {
        generate_into(out, x, 10.0, z, sx, 1, sz, dx, 1.0, dz);
    }
};

class SimplexNoise {
    int32_t perm_[512];
    double xo_, yo_;

public:
    explicit SimplexNoise(JavaRandom& rng);
    void add(double* out, double x, double z, int sx, int sz, double dx, double dz, double amp) const;
};

class SimplexOctaves {
    std::vector<SimplexNoise> gens_;

public:
    SimplexOctaves(JavaRandom& rng, int octaves);
    std::vector<double> generate(double x, double z, int sx, int sz, double dx, double dz, double lacunarity) const;
};

}  // namespace mc
