// Face-culled chunk mesher with per-vertex ambient occlusion and flat block colours.
// (mc-rs textures the faces from terrain.png and runs a light engine; both are later milestones here.)
#pragma once
#include <array>
#include <cstdint>
#include <vector>
#include "world.hpp"

namespace mc {

struct Vertex {
    float x, y, z;
    uint32_t rgba;  // r | g << 8 | b << 16 | a << 24
};

struct MeshData {
    std::vector<Vertex> opaque_v, water_v;
    std::vector<uint32_t> opaque_i, water_i;
    bool empty() const { return opaque_i.empty() && water_i.empty(); }
};

// 3x3 neighbourhood, row-major by (dz + 1) * 3 + (dx + 1); index 4 is the chunk to mesh.
using Neighborhood = std::array<ChunkPtr, 9>;

MeshData mesh_chunk(const Neighborhood& n);

// Flat colour of a block face (r, g, b), face 0..5 = +x -x +y -y +z -z.
void block_color(uint8_t id, int face, uint8_t out[3]);

}  // namespace mc
