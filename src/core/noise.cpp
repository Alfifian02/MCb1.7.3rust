#include "noise.hpp"
#include <cmath>
#include <algorithm>

namespace mc {

int32_t ifloor(double v) {
    int32_t i = static_cast<int32_t>(v);
    return v < static_cast<double>(i) ? i - 1 : i;
}

namespace {
const float* sin_table() {
    static const std::vector<float> t = [] {
        std::vector<float> v(65536);
        for (int i = 0; i < 65536; i++)
            v[i] = static_cast<float>(std::sin(static_cast<double>(i) * 3.14159265358979323846 * 2.0 / 65536.0));
        return v;
    }();
    return t.data();
}
}  // namespace

float mh_sin(float x) { return sin_table()[static_cast<int32_t>(x * 10430.378f) & 0xffff]; }
float mh_cos(float x) { return sin_table()[static_cast<int32_t>(x * 10430.378f + 16384.0f) & 0xffff]; }

namespace {
inline double fade(double t) { return t * t * t * (t * (t * 6.0 - 15.0) + 10.0); }
inline double lerp(double t, double a, double b) { return a + t * (b - a); }

double grad(int32_t h, double x, double y, double z) {
    h &= 15;
    double u = h < 8 ? x : y;
    double v = h < 4 ? y : (h != 12 && h != 14 ? z : x);
    return ((h & 1) == 0 ? u : -u) + ((h & 2) == 0 ? v : -v);
}

// NoiseGeneratorPerlin.func_4110_a: the x/z-only gradient the 2D path uses.
double grad_xz(int32_t h, double x, double z) {
    h &= 15;
    double u = static_cast<double>(1 - ((h & 8) >> 3)) * x;
    double v = h < 4 ? 0.0 : (h != 12 && h != 14 ? z : x);
    return ((h & 1) == 0 ? u : -u) + ((h & 2) == 0 ? v : -v);
}

void shuffle_perm(int32_t* perm, JavaRandom& rng) {
    for (int i = 0; i < 256; i++) perm[i] = i;
    for (int i = 0; i < 256; i++) {
        int j = rng.next_int(256 - i) + i;
        std::swap(perm[i], perm[j]);
        perm[i + 256] = perm[i];
    }
}
}  // namespace

PerlinNoise::PerlinNoise(JavaRandom& rng) {
    xc_ = rng.next_double() * 256.0;
    yc_ = rng.next_double() * 256.0;
    zc_ = rng.next_double() * 256.0;
    shuffle_perm(perm_, rng);
}

void PerlinNoise::add(double* out, double x, double y, double z, int sx, int sy, int sz, double dx, double dy,
                      double dz, double amp) const {
    const double inv = 1.0 / amp;
    int idx = 0;
    if (sy == 1) {
        for (int ix = 0; ix < sx; ix++) {
            double xf = (x + ix) * dx + xc_;
            int32_t xi = ifloor(xf);
            int32_t xb = xi & 255;
            xf -= xi;
            double u = fade(xf);
            for (int iz = 0; iz < sz; iz++) {
                double zf = (z + iz) * dz + zc_;
                int32_t zi = ifloor(zf);
                int32_t zb = zi & 255;
                zf -= zi;
                double w = fade(zf);
                int32_t a = p(xb);
                int32_t aa = p(a) + zb;
                int32_t b = p(xb + 1);
                int32_t ba = p(b) + zb;
                double l1 = lerp(u, grad_xz(p(aa), xf, zf), grad(p(ba), xf - 1.0, 0.0, zf));
                double l2 = lerp(u, grad(p(aa + 1), xf, 0.0, zf - 1.0), grad(p(ba + 1), xf - 1.0, 0.0, zf - 1.0));
                out[idx++] += lerp(w, l1, l2) * inv;
            }
        }
        return;
    }
    int32_t last_y = -1;
    double d1 = 0, d2 = 0, d3 = 0, d4 = 0;
    for (int ix = 0; ix < sx; ix++) {
        double xf = (x + ix) * dx + xc_;
        int32_t xi = ifloor(xf);
        int32_t xb = xi & 255;
        xf -= xi;
        double u = fade(xf);
        for (int iz = 0; iz < sz; iz++) {
            double zf = (z + iz) * dz + zc_;
            int32_t zi = ifloor(zf);
            int32_t zb = zi & 255;
            zf -= zi;
            double w = fade(zf);
            for (int iy = 0; iy < sy; iy++) {
                double yf = (y + iy) * dy + yc_;
                int32_t yi = ifloor(yf);
                int32_t yb = yi & 255;
                yf -= yi;
                double v = fade(yf);
                if (iy == 0 || yb != last_y) {
                    last_y = yb;
                    int32_t a = p(xb) + yb;
                    int32_t aa = p(a) + zb;
                    int32_t ab = p(a + 1) + zb;
                    int32_t b = p(xb + 1) + yb;
                    int32_t ba = p(b) + zb;
                    int32_t bb = p(b + 1) + zb;
                    d1 = lerp(u, grad(p(aa), xf, yf, zf), grad(p(ba), xf - 1.0, yf, zf));
                    d2 = lerp(u, grad(p(ab), xf, yf - 1.0, zf), grad(p(bb), xf - 1.0, yf - 1.0, zf));
                    d3 = lerp(u, grad(p(aa + 1), xf, yf, zf - 1.0), grad(p(ba + 1), xf - 1.0, yf, zf - 1.0));
                    d4 = lerp(u, grad(p(ab + 1), xf, yf - 1.0, zf - 1.0),
                              grad(p(bb + 1), xf - 1.0, yf - 1.0, zf - 1.0));
                }
                double l1 = lerp(v, d1, d2);
                double l2 = lerp(v, d3, d4);
                out[idx++] += lerp(w, l1, l2) * inv;
            }
        }
    }
}

OctaveNoise::OctaveNoise(JavaRandom& rng, int octaves) {
    gens_.reserve(octaves);
    for (int i = 0; i < octaves; i++) gens_.emplace_back(rng);
}

void OctaveNoise::generate_into(double* out, double x, double y, double z, int sx, int sy, int sz, double dx,
                                double dy, double dz) const {
    std::fill(out, out + static_cast<size_t>(sx) * sy * sz, 0.0);
    double amp = 1.0;
    for (const auto& g : gens_) {
        g.add(out, x, y, z, sx, sy, sz, dx * amp, dy * amp, dz * amp, amp);
        amp /= 2.0;
    }
}

std::vector<double> OctaveNoise::generate(double x, double y, double z, int sx, int sy, int sz, double dx, double dy,
                                          double dz) const {
    std::vector<double> out(static_cast<size_t>(sx) * sy * sz);
    generate_into(out.data(), x, y, z, sx, sy, sz, dx, dy, dz);
    return out;
}

// ---- simplex ----
namespace {
const int GRAD3[12][3] = {{1, 1, 0}, {-1, 1, 0}, {1, -1, 0}, {-1, -1, 0}, {1, 0, 1}, {-1, 0, 1},
                          {1, 0, -1}, {-1, 0, -1}, {0, 1, 1}, {0, -1, 1}, {0, 1, -1}, {0, -1, -1}};

int32_t wrap(double v) { return v > 0.0 ? static_cast<int32_t>(v) : static_cast<int32_t>(v) - 1; }

double corner(int gi, double x, double y) {
    double t = 0.5 - x * x - y * y;
    if (t < 0.0) return 0.0;
    t = t * t;
    return t * t * (GRAD3[gi][0] * x + GRAD3[gi][1] * y);
}
}  // namespace

SimplexNoise::SimplexNoise(JavaRandom& rng) {
    xo_ = rng.next_double() * 256.0;
    yo_ = rng.next_double() * 256.0;
    (void)rng.next_double();  // drawn, never used
    shuffle_perm(perm_, rng);
}

void SimplexNoise::add(double* out, double x, double z, int sx, int sz, double dx, double dz, double amp) const {
    const double f2 = 0.5 * (std::sqrt(3.0) - 1.0);
    const double g2 = (3.0 - std::sqrt(3.0)) / 6.0;
    auto p = [&](int32_t i) { return perm_[i]; };
    int idx = 0;
    for (int i = 0; i < sx; i++) {
        double xin = (x + i) * dx + xo_;
        for (int j = 0; j < sz; j++) {
            double yin = (z + j) * dz + yo_;
            double s = (xin + yin) * f2;
            int32_t ii = wrap(xin + s);
            int32_t jj = wrap(yin + s);
            double t = static_cast<double>(ii + jj) * g2;
            double x0 = xin - (ii - t);
            double y0 = yin - (jj - t);
            int i1, j1;
            if (x0 > y0) { i1 = 1; j1 = 0; } else { i1 = 0; j1 = 1; }
            double x1 = x0 - i1 + g2;
            double y1 = y0 - j1 + g2;
            double x2 = x0 - 1.0 + 2.0 * g2;
            double y2 = y0 - 1.0 + 2.0 * g2;
            int32_t ib = ii & 255;
            int32_t jb = jj & 255;
            int gi0 = p(ib + p(jb)) % 12;
            int gi1 = p(ib + i1 + p(jb + j1)) % 12;
            int gi2 = p(ib + 1 + p(jb + 1)) % 12;
            out[idx++] += 70.0 * (corner(gi0, x0, y0) + corner(gi1, x1, y1) + corner(gi2, x2, y2)) * amp;
        }
    }
}

SimplexOctaves::SimplexOctaves(JavaRandom& rng, int octaves) {
    gens_.reserve(octaves);
    for (int i = 0; i < octaves; i++) gens_.emplace_back(rng);
}

std::vector<double> SimplexOctaves::generate(double x, double z, int sx, int sz, double dx, double dz,
                                             double lacunarity) const {
    dx /= 1.5;
    dz /= 1.5;
    std::vector<double> out(static_cast<size_t>(sx) * sz, 0.0);
    double freq = 1.0, amp = 1.0;
    for (const auto& g : gens_) {
        g.add(out.data(), x, z, sx, sz, dx * freq, dz * freq, 0.55 / amp);
        freq *= lacunarity;
        amp *= 0.5;
    }
    return out;
}

}  // namespace mc
