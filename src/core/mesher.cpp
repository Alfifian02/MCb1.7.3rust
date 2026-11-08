#include "mesher.hpp"
#include <cstring>

namespace mc {

void block_color(uint8_t id, int face, uint8_t o[3]) {
    auto set = [&](int r, int g, int b) { o[0] = r; o[1] = g; o[2] = b; };
    switch (id) {
        case block::STONE: set(125, 125, 125); break;
        case block::GRASS:
            if (face == 2) set(98, 162, 56);        // top (plains tint)
            else if (face == 3) set(121, 85, 58);   // bottom = dirt
            else set(112, 118, 54);                 // side: dirt with grass fringe, flattened
            break;
        case block::DIRT: set(121, 85, 58); break;
        case block::COBBLE: set(110, 110, 110); break;
        case block::BEDROCK: set(52, 52, 52); break;
        case block::SAND: set(219, 207, 142); break;
        case block::GRAVEL: set(136, 126, 126); break;
        case block::SANDSTONE: set(216, 203, 155); break;
        case block::ICE: set(145, 183, 253); break;
        case block::WATER: case block::WATER_MOVING: set(48, 92, 210); break;
        case block::LAVA: case block::LAVA_MOVING: set(230, 100, 20); break;
        default: set(200, 0, 200); break;  // unknown: magenta
    }
}

namespace {

struct View {
    const Neighborhood& n;
    // x, z in [-16, 32) relative to the centre chunk.
    uint8_t at(int x, int y, int z) const {
        if (y < 0) return block::STONE;
        if (y >= CHUNK_H) return block::AIR;
        int cx = (x + 16) >> 4, cz = (z + 16) >> 4;
        const ChunkPtr& c = n[cz * 3 + cx];
        if (!c) return block::AIR;
        return c->blocks[cell(x & 15, y, z & 15)];
    }
    bool solid(int x, int y, int z) const { return is_solid(at(x, y, z)); }
};

// Deterministic +-4% brightness noise per block, a cheap stand-in for texture grain.
inline float grain(int x, int y, int z) {
    uint32_t h = static_cast<uint32_t>(x) * 73856093u ^ static_cast<uint32_t>(y) * 19349663u ^
                 static_cast<uint32_t>(z) * 83492791u;
    h ^= h >> 13; h *= 0x5bd1e995u; h ^= h >> 15;
    return 0.96f + 0.08f * static_cast<float>(h & 255) / 255.0f;
}

const float FACE_SHADE[6] = {0.6f, 0.6f, 1.0f, 0.5f, 0.8f, 0.8f};
const float AO_LEVEL[4] = {0.55f, 0.72f, 0.86f, 1.0f};

inline uint32_t pack(float r, float g, float b, float a) {
    auto c = [](float v) { return static_cast<uint32_t>(v < 0 ? 0 : (v > 255 ? 255 : v)); };
    return c(r) | (c(g) << 8) | (c(b) << 16) | (c(a * 255.0f) << 24);
}

}  // namespace

MeshData mesh_chunk(const Neighborhood& nb) {
    MeshData out;
    const View v{nb};
    const ChunkPtr& me = nb[4];
    if (!me) return out;
    out.opaque_v.reserve(8192);
    out.opaque_i.reserve(12288);

    for (int x = 0; x < 16; x++)
        for (int z = 0; z < 16; z++)
            for (int y = 0; y < CHUNK_H; y++) {
                const uint8_t id = me->blocks[cell(x, y, z)];
                if (id == block::AIR) continue;
                const bool water = id == block::WATER || id == block::WATER_MOVING;
                const bool lava = id == block::LAVA || id == block::LAVA_MOVING;
                const bool top_exposed = water && !is_fluid(v.at(x, y + 1, z));

                for (int face = 0; face < 6; face++) {
                    const int axis = face >> 1;
                    const int sign = (face & 1) ? -1 : 1;
                    int d[3] = {0, 0, 0};
                    d[axis] = sign;
                    const uint8_t nid = v.at(x + d[0], y + d[1], z + d[2]);
                    if (water) { if (nid != block::AIR) continue; }
                    else if (lava) { if (nid != block::AIR && !(nid == block::WATER)) continue; }
                    else if (is_solid(nid)) continue;

                    // Corner frame: u x v = normal for a positive face; swap for a negative one.
                    int ua = (axis + 1) % 3, va = (axis + 2) % 3;
                    if (sign < 0) std::swap(ua, va);
                    int ud[3] = {0, 0, 0}, vd[3] = {0, 0, 0};
                    ud[ua] = 1;
                    vd[va] = 1;
                    const int base[3] = {x + (axis == 0 && sign > 0), y + (axis == 1 && sign > 0),
                                         z + (axis == 2 && sign > 0)};
                    const int cu[4] = {0, 1, 1, 0}, cv[4] = {0, 0, 1, 1};

                    uint8_t col[3];
                    block_color(id, face, col);
                    const float shade = FACE_SHADE[face] * (water || lava ? 1.0f : grain(x, y, z));
                    const float alpha = water ? 0.72f : 1.0f;

                    std::vector<Vertex>& vs = water ? out.water_v : out.opaque_v;
                    std::vector<uint32_t>& is = water ? out.water_i : out.opaque_i;
                    const uint32_t first = static_cast<uint32_t>(vs.size());
                    int ao[4];
                    for (int k = 0; k < 4; k++) {
                        float p[3];
                        for (int a = 0; a < 3; a++) p[a] = static_cast<float>(base[a] + ud[a] * cu[k] + vd[a] * cv[k]);
                        if (water && top_exposed && p[1] > y + 0.5f) p[1] -= 0.1111f;  // water surface sits 1/9 low
                        // Ambient occlusion from the layer in front of the face.
                        int s1 = 0, s2 = 0, cn = 0;
                        if (!water && !lava) {
                            const int su = cu[k] ? 1 : -1, sv = cv[k] ? 1 : -1;
                            int o[3] = {x + d[0], y + d[1], z + d[2]};
                            int a1[3] = {o[0] + ud[0] * su, o[1] + ud[1] * su, o[2] + ud[2] * su};
                            int a2[3] = {o[0] + vd[0] * sv, o[1] + vd[1] * sv, o[2] + vd[2] * sv};
                            int a3[3] = {a1[0] + vd[0] * sv, a1[1] + vd[1] * sv, a1[2] + vd[2] * sv};
                            s1 = v.solid(a1[0], a1[1], a1[2]);
                            s2 = v.solid(a2[0], a2[1], a2[2]);
                            cn = v.solid(a3[0], a3[1], a3[2]);
                        }
                        ao[k] = (s1 && s2) ? 0 : 3 - (s1 + s2 + cn);
                        const float br = shade * AO_LEVEL[ao[k]];
                        vs.push_back({p[0], p[1], p[2], pack(col[0] * br, col[1] * br, col[2] * br, alpha)});
                    }
                    if (ao[0] + ao[2] > ao[1] + ao[3]) {
                        const uint32_t q[6] = {0, 1, 2, 2, 3, 0};
                        for (uint32_t i : q) is.push_back(first + i);
                    } else {
                        const uint32_t q[6] = {1, 2, 3, 3, 0, 1};
                        for (uint32_t i : q) is.push_back(first + i);
                    }
                }
            }
    return out;
}

}  // namespace mc
