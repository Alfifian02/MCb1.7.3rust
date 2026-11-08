// Headless check: generates the world around the spawn point and ray-casts one frame on the CPU into a PPM.
// Same terrain, block colours and shading as the GL renderer; it exists so the port can be looked at without a GPU.
#include <cstdio>
#include <cstdlib>
#include <cmath>
#include "../core/climate.hpp"
#include "../core/mesher.hpp"
#include "../core/streamer.hpp"
#include "../core/terrain.hpp"
#include "../core/vec.hpp"

using namespace mc;

int main(int argc, char** argv) {
    const int64_t seed = argc > 1 ? std::strtoll(argv[1], nullptr, 10) : 12345;
    const float yaw_deg = argc > 2 ? std::strtof(argv[2], nullptr) : 30.0f;
    const float pitch_deg = argc > 3 ? std::strtof(argv[3], nullptr) : -12.0f;
    const char* path = argc > 4 ? argv[4] : "shot.ppm";
    const int W = 800, H = 450, R = 10;

    Spawn sp = find_spawn(seed);
    std::printf("spawn %.1f %.1f %.1f\n", sp.x, sp.y, sp.z);
    World world;
    TerrainGenerator gen(seed);
    ClimateManager cm(seed);
    const int pcx = static_cast<int>(std::floor(sp.x)) >> 4, pcz = static_cast<int>(std::floor(sp.z)) >> 4;
    for (int cz = pcz - R; cz <= pcz + R; cz++)
        for (int cx = pcx - R; cx <= pcx + R; cx++) {
            auto c = std::make_shared<Chunk>();
            c->cx = cx; c->cz = cz;
            c->blocks = gen.generate(cx, cz, cm);
            world.chunks[chunk_key(cx, cz)] = c;
        }

    const float yaw = yaw_deg * 3.14159265f / 180.0f, pitch = pitch_deg * 3.14159265f / 180.0f;
    Vec3 eye(sp.x, sp.y + 0.72f, sp.z);
    float cp = std::cos(pitch);
    Vec3 f(-std::sin(yaw) * cp, std::sin(pitch), -std::cos(yaw) * cp);
    Vec3 right = normalize(cross(f, Vec3(0, 1, 0)));
    Vec3 up = cross(right, f);
    const float tanh = std::tan(70.0f * 3.14159265f / 180.0f * 0.5f);
    const float aspect = static_cast<float>(W) / H;
    const float maxd = R * 16.0f;
    const float sky[3] = {0.62f, 0.76f, 1.0f};

    FILE* fp = std::fopen(path, "wb");
    std::fprintf(fp, "P6\n%d %d\n255\n", W, H);
    for (int py = 0; py < H; py++)
        for (int px = 0; px < W; px++) {
            float u = ((px + 0.5f) / W * 2 - 1) * tanh * aspect, v = (1 - (py + 0.5f) / H * 2) * tanh;
            Vec3 d = normalize(f + right * u + up * v);
            // DDA voxel traversal.
            int ix = static_cast<int>(std::floor(eye.x)), iy = static_cast<int>(std::floor(eye.y)),
                iz = static_cast<int>(std::floor(eye.z));
            int step[3] = {d.x > 0 ? 1 : -1, d.y > 0 ? 1 : -1, d.z > 0 ? 1 : -1};
            float tmax[3], tdelta[3];
            const float o[3] = {eye.x, eye.y, eye.z}, dd[3] = {d.x, d.y, d.z};
            const int ic[3] = {ix, iy, iz};
            for (int a = 0; a < 3; a++) {
                if (dd[a] == 0) { tmax[a] = 1e30f; tdelta[a] = 1e30f; continue; }
                float nb = ic[a] + (step[a] > 0 ? 1 : 0);
                tmax[a] = (nb - o[a]) / dd[a];
                tdelta[a] = std::fabs(1.0f / dd[a]);
            }
            float r = sky[0], g = sky[1], b = sky[2];
            float acc[3] = {0, 0, 0}, trans = 1.0f;  // front-to-back accumulation through water
            bool hit = false;
            int cur[3] = {ix, iy, iz};
            float t = 0;
            int lastaxis = 1;
            while (t < maxd) {
                int a = tmax[0] < tmax[1] ? (tmax[0] < tmax[2] ? 0 : 2) : (tmax[1] < tmax[2] ? 1 : 2);
                t = tmax[a];
                tmax[a] += tdelta[a];
                cur[a] += step[a];
                lastaxis = a;
                uint8_t id = world.block_at(cur[0], cur[1], cur[2]);
                if (id == UNLOADED || id == block::AIR) continue;
                if (cur[1] < 0) break;
                int face = lastaxis * 2 + (step[lastaxis] > 0 ? 1 : 0);  // entering through the face opposite the step
                uint8_t col[3];
                block_color(id, face, col);
                const float shade[6] = {0.6f, 0.6f, 1.0f, 0.5f, 0.8f, 0.8f};
                float s = shade[face];
                float fog = std::fmin(1.0f, std::fmax(0.0f, (t - maxd * 0.55f) / (maxd * 0.45f)));
                float c3[3] = {col[0] / 255.f * s, col[1] / 255.f * s, col[2] / 255.f * s};
                for (int k = 0; k < 3; k++) c3[k] = c3[k] * (1 - fog) + sky[k] * fog;
                if (id == block::WATER || id == block::WATER_MOVING) {
                    for (int k = 0; k < 3; k++) acc[k] += trans * 0.72f * c3[k];
                    trans *= 0.28f;
                    continue;
                }
                for (int k = 0; k < 3; k++) acc[k] += trans * c3[k];
                trans = 0; hit = true;
                break;
            }
            if (!hit) for (int k = 0; k < 3; k++) acc[k] += trans * sky[k];
            r = acc[0]; g = acc[1]; b = acc[2];
            unsigned char px3[3] = {(unsigned char)(std::fmin(1.f, r) * 255), (unsigned char)(std::fmin(1.f, g) * 255),
                                    (unsigned char)(std::fmin(1.f, b) * 255)};
            std::fwrite(px3, 1, 3, fp);
        }
    std::fclose(fp);
    std::printf("wrote %s\n", path);
    return 0;
}
