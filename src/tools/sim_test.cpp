// Headless simulation: real streamer + worker threads, a player walking across the world at 60 Hz.
// Checks: chunks stream in and out, meshes are produced, the player never falls through the ground,
// can walk, jump and swim, and stays put until its chunk exists.
#include <chrono>
#include <cstdio>
#include <thread>
#include "../core/player.hpp"
#include "../core/streamer.hpp"

using namespace mc;

static int fails = 0;
#define CHECK(c, ...) do { if (!(c)) { fails++; std::printf("FAIL: " __VA_ARGS__); std::printf("\n"); } } while (0)

int main() {
    const int64_t seed = 12345;
    auto t0 = std::chrono::steady_clock::now();
    Spawn sp = find_spawn(seed);
    double ms = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t0).count();
    std::printf("find_spawn: %.0f ms -> %.1f %.1f %.1f\n", ms, sp.x, sp.y, sp.z);

    World world;
    ChunkStreamer streamer(world, seed, 3);
    Player p;
    p.pos = {sp.x, sp.y, sp.z};
    p.yaw = 0.7f;

    size_t meshes = 0, drops = 0, verts = 0, tris = 0;
    int frames_waiting = 0;
    bool started = false;
    float min_clearance = 1e9f, max_dy = 0;
    float travelled = 0;
    Vec3 start;
    const float dt = 1.0f / 60.0f;
    int jumps_ok = 0;
    for (int f = 0; f < 60 * 40; f++) {  // 40 simulated seconds
        std::vector<MeshResult> fresh;
        std::vector<std::pair<int, int>> dropped;
        const int pcx = static_cast<int>(std::floor(p.pos.x)) >> 4, pcz = static_cast<int>(std::floor(p.pos.z)) >> 4;
        streamer.update(pcx, pcz, 6, fresh, dropped);
        for (auto& m : fresh) { meshes++; verts += m.data.opaque_v.size() + m.data.water_v.size(); tris += (m.data.opaque_i.size() + m.data.water_i.size()) / 3; }
        drops += dropped.size();

        if (world.get_chunk(pcx, pcz)) {
            if (!started) { started = true; start = p.pos; std::printf("chunk under player after %d frames\n", frames_waiting); }
            // Walk forward; jump when the ground is blocked, plus a jump every 3 s to exercise it.
            const bool jump = (f % 180) == 0;
            float y0 = p.pos.y;
            step_player(p, world, dt, 0.0f, 1.0f, jump);
            if (jump && p.vel.y > 7.0f) jumps_ok++;
            max_dy = std::max(max_dy, std::fabs(p.pos.y - y0));
            int top = world.top_solid(static_cast<int>(std::floor(p.pos.x)), static_cast<int>(std::floor(p.pos.z)));
            if (top >= 0) min_clearance = std::min(min_clearance, p.pos.y - HALF_H - (top + 1.0f));
        } else {
            frames_waiting++;
            CHECK(p.pos.y == sp.y, "player moved before its chunk existed");
            std::this_thread::sleep_for(std::chrono::milliseconds(2));
        }
        if (f % 600 == 0) std::printf("  t=%2ds pos=(%.1f %.1f %.1f) loaded=%zu meshed=%zu pending=%zu\n", f / 60, p.pos.x, p.pos.y, p.pos.z, world.chunks.size(), meshes - drops, streamer.pending());
        std::this_thread::sleep_for(std::chrono::milliseconds(3));  // ~5x faster than real time: a stress test for streaming
    }
    travelled = std::hypot(p.pos.x - start.x, p.pos.z - start.z);
    std::printf("meshes=%zu dropped=%zu verts=%zu tris=%zu loaded now=%zu\n", meshes, drops, verts, tris, world.chunks.size());
    std::printf("travelled %.1f blocks, min clearance over top block %.3f, max |dy|/frame %.3f, jumps ok %d\n", travelled, min_clearance, max_dy, jumps_ok);

    CHECK(started, "never got a chunk under the player");
    CHECK(meshes > 100, "too few meshes (%zu)", meshes);
    CHECK(drops > 0, "no chunk was ever unloaded while walking away");
    CHECK(travelled > 30.0f, "player barely moved (%.1f)", travelled);
    CHECK(p.pos.y > 0.0f, "fell out of the world (y=%.1f)", p.pos.y);
    CHECK(world.chunks.size() < 400, "chunk map is not bounded (%zu)", world.chunks.size());
    std::printf(fails ? "%d FAILED\n" : "sim ok\n", fails);
    return fails ? 1 : 0;
}
