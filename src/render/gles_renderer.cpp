#include "gles_renderer.hpp"
#include <GLES3/gl3.h>
#include <cmath>
#include <cstddef>
#include <cstdio>
#include <vector>

namespace mc {

namespace {
const float SKY[3] = {0.62f, 0.76f, 1.0f};

const char* WORLD_VS = R"(#version 300 es
uniform mat4 u_mvp;
uniform vec3 u_origin;   // chunk origin in world space
uniform vec3 u_eye;
layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec4 a_color;  // normalised ubyte
out vec4 v_color;
out float v_dist;
void main() {
    vec3 world = a_pos + u_origin;
    v_color = a_color;
    v_dist = length(world - u_eye);
    gl_Position = u_mvp * vec4(world, 1.0);
}
)";

const char* WORLD_FS = R"(#version 300 es
precision mediump float;
uniform vec3 u_sky;
uniform vec2 u_fog;  // start, end
in vec4 v_color;
in float v_dist;
out vec4 o_color;
void main() {
    float f = clamp((v_dist - u_fog.x) / (u_fog.y - u_fog.x), 0.0, 1.0);
    o_color = vec4(mix(v_color.rgb, u_sky, f), v_color.a);
}
)";

const char* HUD_VS = R"(#version 300 es
uniform vec2 u_res;
layout(location = 0) in vec2 a_pos;  // pixels, origin top-left
void main() {
    gl_Position = vec4(a_pos.x / u_res.x * 2.0 - 1.0, 1.0 - a_pos.y / u_res.y * 2.0, 0.0, 1.0);
}
)";

const char* HUD_FS = R"(#version 300 es
precision mediump float;
uniform vec4 u_color;
out vec4 o_color;
void main() { o_color = u_color; }
)";

GLuint compile(GLenum type, const char* src) {
    GLuint s = glCreateShader(type);
    glShaderSource(s, 1, &src, nullptr);
    glCompileShader(s);
    GLint ok = 0;
    glGetShaderiv(s, GL_COMPILE_STATUS, &ok);
    if (!ok) {
        char log[1024];
        glGetShaderInfoLog(s, sizeof log, nullptr, log);
        std::fprintf(stderr, "shader compile failed: %s\n", log);
        glDeleteShader(s);
        return 0;
    }
    return s;
}

GLuint link(const char* vs, const char* fs) {
    GLuint v = compile(GL_VERTEX_SHADER, vs), f = compile(GL_FRAGMENT_SHADER, fs);
    if (!v || !f) return 0;
    GLuint p = glCreateProgram();
    glAttachShader(p, v);
    glAttachShader(p, f);
    glLinkProgram(p);
    glDeleteShader(v);
    glDeleteShader(f);
    GLint ok = 0;
    glGetProgramiv(p, GL_LINK_STATUS, &ok);
    if (!ok) {
        char log[1024];
        glGetProgramInfoLog(p, sizeof log, nullptr, log);
        std::fprintf(stderr, "program link failed: %s\n", log);
        glDeleteProgram(p);
        return 0;
    }
    return p;
}
}  // namespace

bool Renderer::init() {
    world_prog_ = link(WORLD_VS, WORLD_FS);
    hud_prog_ = link(HUD_VS, HUD_FS);
    if (!world_prog_ || !hud_prog_) return false;
    u_mvp_ = glGetUniformLocation(world_prog_, "u_mvp");
    u_origin_ = glGetUniformLocation(world_prog_, "u_origin");
    u_eye_ = glGetUniformLocation(world_prog_, "u_eye");
    u_fog_ = glGetUniformLocation(world_prog_, "u_fog");
    u_sky_ = glGetUniformLocation(world_prog_, "u_sky");
    u_hud_color_ = glGetUniformLocation(hud_prog_, "u_color");
    u_hud_res_ = glGetUniformLocation(hud_prog_, "u_res");

    glGenVertexArrays(1, &hud_vao_);
    glGenBuffers(1, &hud_vbo_);
    glBindVertexArray(hud_vao_);
    glBindBuffer(GL_ARRAY_BUFFER, hud_vbo_);
    glEnableVertexAttribArray(0);
    glVertexAttribPointer(0, 2, GL_FLOAT, GL_FALSE, 8, nullptr);
    glBindVertexArray(0);

    glEnable(GL_DEPTH_TEST);
    glEnable(GL_CULL_FACE);
    glCullFace(GL_BACK);
    glFrontFace(GL_CCW);
    return true;
}

void Renderer::shutdown() {
    for (auto& kv : chunks_) { release(kv.second.opaque); release(kv.second.water); }
    chunks_.clear();
    if (hud_vbo_) glDeleteBuffers(1, &hud_vbo_);
    if (hud_vao_) glDeleteVertexArrays(1, &hud_vao_);
    if (world_prog_) glDeleteProgram(world_prog_);
    if (hud_prog_) glDeleteProgram(hud_prog_);
    hud_vbo_ = hud_vao_ = world_prog_ = hud_prog_ = 0;
}

void Renderer::resize(int w, int h) {
    width_ = w > 0 ? w : 1;
    height_ = h > 0 ? h : 1;
    glViewport(0, 0, width_, height_);
}

void Renderer::upload(GpuMesh& m, const std::vector<Vertex>& v, const std::vector<uint32_t>& i) {
    if (i.empty()) return;
    glGenVertexArrays(1, &m.vao);
    glGenBuffers(1, &m.vbo);
    glGenBuffers(1, &m.ibo);
    glBindVertexArray(m.vao);
    glBindBuffer(GL_ARRAY_BUFFER, m.vbo);
    glBufferData(GL_ARRAY_BUFFER, static_cast<GLsizeiptr>(v.size() * sizeof(Vertex)), v.data(), GL_STATIC_DRAW);
    glBindBuffer(GL_ELEMENT_ARRAY_BUFFER, m.ibo);
    glBufferData(GL_ELEMENT_ARRAY_BUFFER, static_cast<GLsizeiptr>(i.size() * sizeof(uint32_t)), i.data(), GL_STATIC_DRAW);
    glEnableVertexAttribArray(0);
    glVertexAttribPointer(0, 3, GL_FLOAT, GL_FALSE, sizeof(Vertex), reinterpret_cast<void*>(offsetof(Vertex, x)));
    glEnableVertexAttribArray(1);
    glVertexAttribPointer(1, 4, GL_UNSIGNED_BYTE, GL_TRUE, sizeof(Vertex), reinterpret_cast<void*>(offsetof(Vertex, rgba)));
    glBindVertexArray(0);
    m.index_count = static_cast<int>(i.size());
}

void Renderer::release(GpuMesh& m) {
    if (m.vbo) glDeleteBuffers(1, &m.vbo);
    if (m.ibo) glDeleteBuffers(1, &m.ibo);
    if (m.vao) glDeleteVertexArrays(1, &m.vao);
    m = {};
}

void Renderer::add_mesh(int cx, int cz, const MeshData& m) {
    drop_mesh(cx, cz);
    ChunkGpu g;
    upload(g.opaque, m.opaque_v, m.opaque_i);
    upload(g.water, m.water_v, m.water_i);
    if (g.opaque.index_count || g.water.index_count) chunks_[chunk_key(cx, cz)] = g;
}

void Renderer::drop_mesh(int cx, int cz) {
    auto it = chunks_.find(chunk_key(cx, cz));
    if (it == chunks_.end()) return;
    release(it->second.opaque);
    release(it->second.water);
    chunks_.erase(it);
}

void Renderer::draw(const Player& p, const HudState& hud) {
    glClearColor(SKY[0], SKY[1], SKY[2], 1.0f);
    glClear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT);

    const Vec3 eye = p.eye();
    const Vec3 fwd = p.forward();
    const float aspect = static_cast<float>(width_) / static_cast<float>(height_);
    const Mat4 mvp = Mat4::perspective(70.0f * 3.14159265f / 180.0f, aspect, 0.05f, fog_far_ * 1.6f) *
                     Mat4::look(eye, fwd, Vec3(0, 1, 0));

    glUseProgram(world_prog_);
    glUniformMatrix4fv(u_mvp_, 1, GL_FALSE, mvp.m);
    glUniform3f(u_eye_, eye.x, eye.y, eye.z);
    glUniform3f(u_sky_, SKY[0], SKY[1], SKY[2]);
    glUniform2f(u_fog_, fog_far_ * 0.55f, fog_far_);

    // Chunks in front of the camera only (a sphere test around each chunk's centre).
    const float radius = 16.0f * 1.5f * 4.5f;  // half diagonal of a 16x128x16 box is ~65
    auto visible = [&](int cx, int cz) {
        const Vec3 c(cx * 16.0f + 8.0f, 64.0f, cz * 16.0f + 8.0f);
        const Vec3 d = c - eye;
        const float dist2 = dot(d, d);
        if (dist2 < 70.0f * 70.0f) return true;  // right around the player: always draw
        return dot(d, fwd) > -radius * 0.9f && dist2 < (fog_far_ + 70.0f) * (fog_far_ + 70.0f);
    };

    glDisable(GL_BLEND);
    glDepthMask(GL_TRUE);
    for (auto& kv : chunks_) {
        const int cx = chunk_key_x(kv.first), cz = chunk_key_z(kv.first);
        if (!kv.second.opaque.index_count || !visible(cx, cz)) continue;
        glUniform3f(u_origin_, cx * 16.0f, 0.0f, cz * 16.0f);
        glBindVertexArray(kv.second.opaque.vao);
        glDrawElements(GL_TRIANGLES, kv.second.opaque.index_count, GL_UNSIGNED_INT, nullptr);
    }
    // Water after all opaque geometry, blended, not written to depth. (Chunk order is unsorted; mc-rs sorts back to front.)
    glEnable(GL_BLEND);
    glBlendFunc(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA);
    glDepthMask(GL_FALSE);
    glDisable(GL_CULL_FACE);
    for (auto& kv : chunks_) {
        const int cx = chunk_key_x(kv.first), cz = chunk_key_z(kv.first);
        if (!kv.second.water.index_count || !visible(cx, cz)) continue;
        glUniform3f(u_origin_, cx * 16.0f, 0.0f, cz * 16.0f);
        glBindVertexArray(kv.second.water.vao);
        glDrawElements(GL_TRIANGLES, kv.second.water.index_count, GL_UNSIGNED_INT, nullptr);
    }
    glEnable(GL_CULL_FACE);
    glDepthMask(GL_TRUE);

    // HUD: crosshair + virtual stick + jump button, as plain translucent quads / discs.
    glDisable(GL_DEPTH_TEST);
    glUseProgram(hud_prog_);
    glUniform2f(u_hud_res_, static_cast<float>(width_), static_cast<float>(height_));
    glBindVertexArray(hud_vao_);
    glBindBuffer(GL_ARRAY_BUFFER, hud_vbo_);
    glDisable(GL_CULL_FACE);
    auto quad = [&](float x0, float y0, float x1, float y1, float r, float g, float b, float a) {
        const float v[12] = {x0, y0, x1, y0, x1, y1, x0, y0, x1, y1, x0, y1};
        glBufferData(GL_ARRAY_BUFFER, sizeof v, v, GL_STREAM_DRAW);
        glUniform4f(u_hud_color_, r, g, b, a);
        glDrawArrays(GL_TRIANGLES, 0, 6);
    };
    auto disc = [&](float cx, float cy, float rad, float r, float g, float b, float a) {
        std::vector<float> v;
        const int N = 24;
        for (int k = 0; k < N; k++) {
            const float a0 = 6.2831853f * k / N, a1 = 6.2831853f * (k + 1) / N;
            v.insert(v.end(), {cx, cy, cx + rad * std::cos(a0), cy + rad * std::sin(a0), cx + rad * std::cos(a1),
                               cy + rad * std::sin(a1)});
        }
        glBufferData(GL_ARRAY_BUFFER, static_cast<GLsizeiptr>(v.size() * sizeof(float)), v.data(), GL_STREAM_DRAW);
        glUniform4f(u_hud_color_, r, g, b, a);
        glDrawArrays(GL_TRIANGLES, 0, N * 3);
    };
    const float mx = width_ * 0.5f, my = height_ * 0.5f;
    quad(mx - 10, my - 1, mx + 10, my + 1, 1, 1, 1, 0.8f);
    quad(mx - 1, my - 10, mx + 1, my + 10, 1, 1, 1, 0.8f);
    if (hud.stick_r > 0) {
        disc(hud.stick_cx, hud.stick_cy, hud.stick_r, 1, 1, 1, 0.12f);
        disc(hud.knob_x, hud.knob_y, hud.stick_r * 0.45f, 1, 1, 1, 0.35f);
    }
    if (hud.jump_r > 0) disc(hud.jump_cx, hud.jump_cy, hud.jump_r, 1, 1, 1, hud.jump_down ? 0.45f : 0.2f);
    glEnable(GL_CULL_FACE);
    glEnable(GL_DEPTH_TEST);
    glDisable(GL_BLEND);
    glBindVertexArray(0);
}

}  // namespace mc
