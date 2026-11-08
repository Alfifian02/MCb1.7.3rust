#include "player.hpp"
#include <algorithm>
#include <cmath>

namespace mc {

namespace {
constexpr float STEP = 0.5f;  // step-up height when blocked while grounded (as in mc-rs)

bool cell_solid(const World& w, int x, int y, int z) {
    uint8_t b = w.block_at(x, y, z);
    return b == UNLOADED || is_solid(b);  // unloaded ground is a wall: never fall through the world
}

// Does the box (centre p) strictly overlap any solid cell?
bool hits(const World& w, const Vec3& p) {
    const int x0 = static_cast<int>(std::floor(p.x - HALF_W)), x1 = static_cast<int>(std::ceil(p.x + HALF_W)) - 1;
    const int y0 = static_cast<int>(std::floor(p.y - HALF_H)), y1 = static_cast<int>(std::ceil(p.y + HALF_H)) - 1;
    const int z0 = static_cast<int>(std::floor(p.z - HALF_W)), z1 = static_cast<int>(std::ceil(p.z + HALF_W)) - 1;
    for (int y = y0; y <= y1; y++)
        for (int z = z0; z <= z1; z++)
            for (int x = x0; x <= x1; x++)
                if (cell_solid(w, x, y, z)) return true;
    return false;
}

bool touches_fluid(const World& w, const Vec3& p, uint8_t lo_id, uint8_t hi_id) {
    const float ylo = p.y - HALF_H + 0.401f, yhi = p.y + HALF_H - 0.401f;
    for (int cy = static_cast<int>(std::floor(ylo)); cy <= static_cast<int>(std::floor(yhi)); cy++) {
        if (static_cast<float>(cy) + 8.0f / 9.0f < ylo) continue;  // a fluid cell only reaches 8/9 of its height
        for (int cz = static_cast<int>(std::floor(p.z - HALF_W + 0.001f)); cz <= static_cast<int>(std::floor(p.z + HALF_W - 0.001f)); cz++)
            for (int cx = static_cast<int>(std::floor(p.x - HALF_W + 0.001f)); cx <= static_cast<int>(std::floor(p.x + HALF_W - 0.001f)); cx++) {
                uint8_t b = w.block_at(cx, cy, cz);
                if (b >= lo_id && b <= hi_id && b != UNLOADED) return true;
            }
    }
    return false;
}

// Binary search for the last position along one axis that does not collide, then snap to it.
template <class F>
float snap(float start, float delta, F&& collides) {
    if (delta == 0.0f) return start;
    float lo = start, hi = start + delta;
    for (int i = 0; i < 12; i++) {
        float m = 0.5f * (lo + hi);
        if (collides(m)) hi = m; else lo = m;
    }
    return lo;
}

Vec3 sweep(const World& w, Vec3 p, const Vec3& delta, Vec3& vel, bool grounded) {
    for (int axis : {0, 2}) {
        Vec3 t = p;
        t[axis] += delta[axis];
        if (hits(w, t)) {
            Vec3 up = t;
            up.y += STEP;
            if (grounded && !hits(w, up)) {
                // Raise by STEP, then settle on whatever is under the new spot (never below where we were).
                const float top = t.y + STEP;
                t.y = snap(top, -STEP, [&](float y) { return hits(w, {t.x, y, t.z}); });
            } else {
                t[axis] = snap(p[axis], delta[axis], [&](float v) { Vec3 q = p; q[axis] = v; return hits(w, q); });
                vel[axis] = 0.0f;
            }
        }
        p = t;
    }
    Vec3 t = p;
    t.y += delta.y;
    if (hits(w, t)) {
        t.y = snap(p.y, delta.y, [&](float y) { return hits(w, {p.x, y, p.z}); });
        vel.y = 0.0f;
    }
    return t;
}
}  // namespace

bool step_player(Player& p, const World& w, float dt, float strafe, float fwd, bool jump) {
    // Horizontal velocity is set from the input each frame (like mc-rs), yaw-rotated.
    const float len = std::sqrt(strafe * strafe + fwd * fwd);
    if (len > 1.0f) { strafe /= len; fwd /= len; }
    const float sy = std::sin(p.yaw), cy = std::cos(p.yaw);
    // forward = (-sin, -cos); right = (cos, -sin)
    p.vel.x = (-sy * fwd + cy * strafe) * WALK_SPEED;
    p.vel.z = (-cy * fwd - sy * strafe) * WALK_SPEED;

    const bool water = touches_fluid(w, p.pos, block::WATER_MOVING, block::WATER);
    const bool lava = !water && touches_fluid(w, p.pos, block::LAVA_MOVING, block::LAVA);
    const float ticks = dt * 20.0f;
    if (water || lava) {
        const float speed = (water ? 2.0f : 0.8f) / WALK_SPEED;
        p.vel.x *= speed;
        p.vel.z *= speed;
        if (jump) p.vel.y += 0.8f * ticks;
    } else {
        p.vel.y -= 23.0f * dt;
        if (p.vel.y < -55.0f) p.vel.y = -55.0f;
        if (jump && p.on_ground) p.vel.y = 8.4f;
    }

    const Vec3 delta = p.vel * dt;
    const bool grounded = p.on_ground && !(water || lava);
    const float vx = p.vel.x, vz = p.vel.z;
    p.pos = sweep(w, p.pos, delta, p.vel, grounded);
    if (water || lava) {
        p.vel.y = p.vel.y * std::pow(water ? 0.8f : 0.5f, ticks) - 0.4f * ticks;
        const bool blocked = (vx != 0.0f && p.vel.x == 0.0f) || (vz != 0.0f && p.vel.z == 0.0f);
        // Climb out of the water onto a ledge when pushing into it.
        if (blocked && !hits(w, {p.pos.x + vx * dt, p.pos.y + 0.6f, p.pos.z + vz * dt})) p.vel.y = 6.0f;
    }
    p.on_ground = hits(w, {p.pos.x, p.pos.y - 0.002f, p.pos.z});
    return water;
}

}  // namespace mc
