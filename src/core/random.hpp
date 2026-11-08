// java.util.Random (48-bit LCG), identical to the JDK. Port of mc-rs world/gen/noise.rs.
#pragma once
#include <cstdint>
#include <cmath>

namespace mc {

class JavaRandom {
    static constexpr uint64_t MULT = 0x5DEECE66DULL;
    static constexpr uint64_t MASK = (1ULL << 48) - 1;
    uint64_t seed_;
    bool have_spare_ = false;
    double spare_ = 0.0;

    int32_t next(int bits) {
        seed_ = (seed_ * MULT + 0xB) & MASK;
        return static_cast<int32_t>(static_cast<uint32_t>(seed_ >> (48 - bits)));
    }

public:
    explicit JavaRandom(int64_t seed = 0) { set_seed(seed); }

    void set_seed(int64_t seed) {
        seed_ = (static_cast<uint64_t>(seed) ^ MULT) & MASK;
        have_spare_ = false;  // setSeed clears haveNextNextGaussian
    }

    int32_t next_int() { return next(32); }

    // nextInt(bound): uniform in [0, bound)
    int32_t next_int(int32_t bound) {
        if ((bound & -bound) == bound) {
            return static_cast<int32_t>((static_cast<int64_t>(bound) * static_cast<int64_t>(next(31))) >> 31);
        }
        for (;;) {
            int32_t bits = next(31);
            int32_t val = bits % bound;
            int32_t t = static_cast<int32_t>(static_cast<uint32_t>(bits) - static_cast<uint32_t>(val) +
                                             static_cast<uint32_t>(bound - 1));  // Java int overflow wraps
            if (t >= 0) return val;
        }
    }

    int64_t next_long() {
        int64_t hi = next(32);
        int64_t lo = next(32);
        return static_cast<int64_t>((static_cast<uint64_t>(hi) << 32) + static_cast<uint64_t>(lo));
    }

    double next_double() {
        int64_t hi = next(26);
        int64_t lo = next(27);
        return static_cast<double>((hi << 27) + lo) / static_cast<double>(1ULL << 53);
    }

    float next_float() { return static_cast<float>(next(24)) / static_cast<float>(1u << 24); }

    bool next_boolean() { return next(1) != 0; }

    double next_gaussian() {
        if (have_spare_) { have_spare_ = false; return spare_; }
        for (;;) {
            double v1 = 2.0 * next_double() - 1.0;
            double v2 = 2.0 * next_double() - 1.0;
            double s = v1 * v1 + v2 * v2;
            if (s < 1.0 && s != 0.0) {
                double m = std::sqrt(-2.0 * std::log(s) / s);
                spare_ = v2 * m;
                have_spare_ = true;
                return v1 * m;
            }
        }
    }
};

}  // namespace mc
