// MapGenCaves. Caves only read the world seed and write inside the one chunk being generated: every chunk in a 17x17
// square around it may start tunnels, and the part that crosses into this chunk is carved.
//
// Fidelity notes (kept from the Rust port, which matches the Java bit for bit):
//  * float maths is 32-bit with the MathHelper sin/cos table, widened to double exactly where the Java widens;
//  * every expression that draws from a Random twice is split into statements, because C++ does not fix the order in
//    which operands are evaluated (Java and Rust do: left to right);
//  * the carve loop reads the block one cell above the cell its `y` refers to (idx starts at y_hi, iy at y_hi - 1).
#include "caves.hpp"
#include <algorithm>
#include "blocks.hpp"
#include "noise.hpp"
#include "random.hpp"

namespace mc {
namespace {

constexpr int RANGE = 8;  // MapGenBase.field_1306_a: how many chunks away a tunnel may start
constexpr float PI_F = 3.14159265358979323846f;

bool touches_water(const uint8_t* blocks, int x_lo, int x_hi, int y_lo, int y_hi, int z_lo, int z_hi) {
    for (int ix = x_lo; ix < x_hi; ix++) {
        for (int iz = z_lo; iz < z_hi; iz++) {
            int iy = y_hi + 1;
            while (iy >= y_lo - 1) {
                if (iy >= 0 && iy < CHUNK_H) {
                    uint8_t b = blocks[cell(ix, iy, iz)];
                    if (b == block::WATER_MOVING || b == block::WATER) return true;
                    // Only the shell matters: from the inside jump straight to the floor.
                    if (iy != y_lo - 1 && ix != x_lo && ix != x_hi - 1 && iz != z_lo && iz != z_hi - 1) iy = y_lo;
                }
                iy--;
            }
        }
    }
    return false;
}

void carve_ellipsoid(uint8_t* blocks, int cx, int cz, double x, double y, double z, double r1, double r2, int x_lo,
                     int x_hi, int y_lo, int y_hi, int z_lo, int z_hi) {
    for (int ix = x_lo; ix < x_hi; ix++) {
        double dx = (static_cast<double>(ix + cx * 16) + 0.5 - x) / r1;
        for (int iz = z_lo; iz < z_hi; iz++) {
            double dz = (static_cast<double>(iz + cz * 16) + 0.5 - z) / r1;
            if (dx * dx + dz * dz >= 1.0) continue;
            int i = cell(ix, y_hi, iz);  // Java quirk kept: starts one cell above `iy`
            bool grass = false;
            for (int iy = y_hi - 1; iy >= y_lo; iy--) {
                double dy = (static_cast<double>(iy) + 0.5 - y) / r2;
                if (dy > -0.7 && dx * dx + dy * dy + dz * dz < 1.0) {
                    uint8_t b = blocks[i];
                    if (b == block::GRASS) grass = true;
                    if (b == block::STONE || b == block::DIRT || b == block::GRASS) {
                        if (iy < 10) {
                            blocks[i] = block::LAVA_MOVING;
                        } else {
                            blocks[i] = block::AIR;
                            if (grass && blocks[i - 1] == block::DIRT) blocks[i - 1] = block::GRASS;
                        }
                    }
                }
                i--;
            }
        }
    }
}

// MapGenCaves.releaseEntitySkin: walk one tunnel (or room) and carve the part inside chunk (cx, cz). `step` / `steps`
// of 0 and -1 mean "pick them here", as in the Java. `main` only seeds this tunnel's own Random (and the branches').
void tunnel(JavaRandom& main, int cx, int cz, uint8_t* blocks, double x, double y, double z, float radius, float yaw,
            float pitch, int step, int steps, double vscale) {
    const double mid_x = static_cast<double>(cx * 16 + 8), mid_z = static_cast<double>(cz * 16 + 8);
    float yaw_rate = 0.0f, pitch_rate = 0.0f;
    JavaRandom rnd(main.next_long());
    if (steps <= 0) {
        int m = RANGE * 16 - 16;
        steps = m - rnd.next_int(m / 4);
    }
    bool room = false;
    if (step == -1) {
        step = steps / 2;
        room = true;
    }
    int split_at = rnd.next_int(steps / 2) + steps / 4;
    bool steep = rnd.next_int(6) == 0;
    while (step < steps) {
        double r1 = 1.5 + static_cast<double>(mh_sin(static_cast<float>(step) * PI_F / static_cast<float>(steps)) * radius * 1.0f);
        double r2 = r1 * vscale;
        float cp = mh_cos(pitch);
        float sp = mh_sin(pitch);
        x += static_cast<double>(mh_cos(yaw) * cp);
        y += static_cast<double>(sp);
        z += static_cast<double>(mh_sin(yaw) * cp);
        pitch *= steep ? 0.92f : 0.7f;
        pitch += pitch_rate * 0.1f;
        yaw += yaw_rate * 0.1f;
        pitch_rate *= 0.9f;
        yaw_rate *= 12.0f / 16.0f;
        {
            float a = rnd.next_float();
            float b = rnd.next_float();
            float c = rnd.next_float();
            pitch_rate += (a - b) * c * 2.0f;
        }
        {
            float a = rnd.next_float();
            float b = rnd.next_float();
            float c = rnd.next_float();
            yaw_rate += (a - b) * c * 4.0f;
        }
        if (!room && step == split_at && radius > 1.0f) {
            float r_a = rnd.next_float() * 0.5f + 0.5f;
            tunnel(main, cx, cz, blocks, x, y, z, r_a, yaw - PI_F * 0.5f, pitch / 3.0f, step, steps, 1.0);
            float r_b = rnd.next_float() * 0.5f + 0.5f;
            tunnel(main, cx, cz, blocks, x, y, z, r_b, yaw + PI_F * 0.5f, pitch / 3.0f, step, steps, 1.0);
            return;
        }
        if (room || rnd.next_int(4) != 0) {
            double dx = x - mid_x, dz = z - mid_z;
            double left = static_cast<double>(steps - step);
            double reach = static_cast<double>(radius + 2.0f + 16.0f);
            if (dx * dx + dz * dz - left * left > reach * reach) return;
            double lim = 16.0 + r1 * 2.0;
            if (x >= mid_x - lim && z >= mid_z - lim && x <= mid_x + lim && z <= mid_z + lim) {
                int x_lo = std::max(ifloor(x - r1) - cx * 16 - 1, 0);
                int x_hi = std::min(ifloor(x + r1) - cx * 16 + 1, 16);
                int y_lo = std::max(ifloor(y - r2) - 1, 1);
                int y_hi = std::min(ifloor(y + r2) + 1, 120);
                int z_lo = std::max(ifloor(z - r1) - cz * 16 - 1, 0);
                int z_hi = std::min(ifloor(z + r1) - cz * 16 + 1, 16);
                if (!touches_water(blocks, x_lo, x_hi, y_lo, y_hi, z_lo, z_hi)) {
                    carve_ellipsoid(blocks, cx, cz, x, y, z, r1, r2, x_lo, x_hi, y_lo, y_hi, z_lo, z_hi);
                    if (room) break;
                }
            }
        }
        step++;
    }
}

// MapGenCaves.func_868_a: the tunnels that start in chunk (sx, sz).
void starts(JavaRandom& r, int sx, int sz, int cx, int cz, uint8_t* blocks) {
    // nextInt(nextInt(nextInt(40) + 1) + 1), innermost first.
    int a = r.next_int(40) + 1;
    int b = r.next_int(a) + 1;
    int count = r.next_int(b);
    if (r.next_int(15) != 0) count = 0;
    for (int k = 0; k < count; k++) {
        double x = static_cast<double>(sx * 16 + r.next_int(16));
        int h = r.next_int(120) + 8;
        double y = static_cast<double>(r.next_int(h));
        double z = static_cast<double>(sz * 16 + r.next_int(16));
        int n = 1;
        if (r.next_int(4) == 0) {
            // A big round room (func_870_a), then 0-3 ordinary tunnels from the same point.
            float radius = 1.0f + r.next_float() * 6.0f;
            tunnel(r, cx, cz, blocks, x, y, z, radius, 0.0f, 0.0f, -1, -1, 0.5);
            n += r.next_int(4);
        }
        for (int j = 0; j < n; j++) {
            float yaw = r.next_float() * PI_F * 2.0f;
            float pitch = (r.next_float() - 0.5f) * 2.0f / 8.0f;
            float r_a = r.next_float() * 2.0f;
            float r_b = r.next_float();
            tunnel(r, cx, cz, blocks, x, y, z, r_a + r_b, yaw, pitch, 0, 0, 1.0);
        }
    }
}

}  // namespace

void carve_caves(int64_t seed, int cx, int cz, uint8_t* blocks) {
    JavaRandom rand(seed);
    int64_t a = rand.next_long() / 2 * 2 + 1;
    int64_t b = rand.next_long() / 2 * 2 + 1;
    for (int x = cx - RANGE; x <= cx + RANGE; x++) {
        for (int z = cz - RANGE; z <= cz + RANGE; z++) {
            // Java long arithmetic wraps: do it unsigned.
            uint64_t s = static_cast<uint64_t>(static_cast<int64_t>(x)) * static_cast<uint64_t>(a) +
                         static_cast<uint64_t>(static_cast<int64_t>(z)) * static_cast<uint64_t>(b);
            rand.set_seed(static_cast<int64_t>(s) ^ seed);
            starts(rand, x, z, cx, cz, blocks);
        }
    }
}

}  // namespace mc
