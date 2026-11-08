// OpenGL ES 3.0 renderer: one opaque + one water mesh per chunk, distance fog, a tiny HUD pass.
#pragma once
#include <cstdint>
#include <unordered_map>
#include "../core/mesher.hpp"
#include "../core/player.hpp"
#include "../core/vec.hpp"

namespace mc {

struct HudState {
    // Virtual controls in pixels (origin top-left). Radius 0 = hidden.
    float stick_cx = 0, stick_cy = 0, stick_r = 0;  // base ring
    float knob_x = 0, knob_y = 0;                   // current thumb position
    float jump_cx = 0, jump_cy = 0, jump_r = 0;
    bool jump_down = false;
};

class Renderer {
    struct GpuMesh {
        uint32_t vao = 0, vbo = 0, ibo = 0;
        int index_count = 0;
    };
    struct ChunkGpu {
        GpuMesh opaque, water;
    };
    std::unordered_map<int64_t, ChunkGpu> chunks_;
    uint32_t world_prog_ = 0, hud_prog_ = 0, hud_vao_ = 0, hud_vbo_ = 0;
    int u_mvp_ = -1, u_origin_ = -1, u_eye_ = -1, u_fog_ = -1, u_sky_ = -1, u_hud_color_ = -1, u_hud_res_ = -1;
    int width_ = 1, height_ = 1;
    float fog_far_ = 160.0f;

    static void upload(GpuMesh& m, const std::vector<Vertex>& v, const std::vector<uint32_t>& i);
    static void release(GpuMesh& m);

public:
    bool init();  // needs a current GLES 3 context
    void shutdown();
    void resize(int w, int h);
    void set_view_distance_chunks(int chunks) { fog_far_ = chunks * 16.0f; }

    void add_mesh(int cx, int cz, const MeshData& m);
    void drop_mesh(int cx, int cz);
    size_t mesh_count() const { return chunks_.size(); }

    void draw(const Player& p, const HudState& hud);
};

}  // namespace mc
