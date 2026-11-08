// Android NativeActivity entry point: EGL / GLES 3 setup, touch controls, and the frame loop.
//   - left 40% of the screen: floating move stick (moves under the first finger that lands there)
//   - right side: drag anywhere to look
//   - round button bottom-right: jump
#include <EGL/egl.h>
#include <android/input.h>
#include <android/log.h>
#include <android/native_window.h>
#include <android_native_app_glue.h>

#include <algorithm>
#include <chrono>
#include <cmath>
#include <memory>
#include <thread>
#include <vector>

#include "../core/player.hpp"
#include "../core/streamer.hpp"
#include "../core/world.hpp"
#include "../render/gles_renderer.hpp"

#define LOG(...) __android_log_print(ANDROID_LOG_INFO, "mc-cpp", __VA_ARGS__)

namespace {

constexpr int64_t SEED = 12345;
constexpr int VIEW_RADIUS = 6;  // chunks

struct Touch {
    int32_t id = -1;
    float x = 0, y = 0;        // current
    float sx = 0, sy = 0;      // where the finger landed
    float lx = 0, ly = 0;      // last frame (look delta)
};

struct Game {
    // EGL
    EGLDisplay display = EGL_NO_DISPLAY;
    EGLSurface surface = EGL_NO_SURFACE;
    EGLContext context = EGL_NO_CONTEXT;
    int width = 0, height = 0;

    // World (rebuilt whenever the GL context is lost)
    mc::World world;
    std::unique_ptr<mc::ChunkStreamer> streamer;
    mc::Renderer renderer;
    mc::Player player;
    bool have_player = false;
    bool gl_ready = false;

    // Input
    Touch stick, look, jump;
    mc::HudState hud;

    std::chrono::steady_clock::time_point last_frame;
};

bool init_egl(Game& g, ANativeWindow* win) {
    g.display = eglGetDisplay(EGL_DEFAULT_DISPLAY);
    if (g.display == EGL_NO_DISPLAY || !eglInitialize(g.display, nullptr, nullptr)) return false;
    const EGLint cfg_attr[] = {EGL_RENDERABLE_TYPE, EGL_OPENGL_ES3_BIT, EGL_SURFACE_TYPE, EGL_WINDOW_BIT,
                               EGL_RED_SIZE, 8, EGL_GREEN_SIZE, 8, EGL_BLUE_SIZE, 8, EGL_DEPTH_SIZE, 24, EGL_NONE};
    EGLConfig config;
    EGLint n = 0;
    if (!eglChooseConfig(g.display, cfg_attr, &config, 1, &n) || n < 1) return false;
    EGLint fmt = 0;
    eglGetConfigAttrib(g.display, config, EGL_NATIVE_VISUAL_ID, &fmt);
    ANativeWindow_setBuffersGeometry(win, 0, 0, fmt);
    g.surface = eglCreateWindowSurface(g.display, config, win, nullptr);
    const EGLint ctx_attr[] = {EGL_CONTEXT_CLIENT_VERSION, 3, EGL_NONE};
    g.context = eglCreateContext(g.display, config, EGL_NO_CONTEXT, ctx_attr);
    if (g.surface == EGL_NO_SURFACE || g.context == EGL_NO_CONTEXT) return false;
    if (!eglMakeCurrent(g.display, g.surface, g.surface, g.context)) return false;
    eglQuerySurface(g.display, g.surface, EGL_WIDTH, &g.width);
    eglQuerySurface(g.display, g.surface, EGL_HEIGHT, &g.height);
    eglSwapInterval(g.display, 1);
    return true;
}

void term_egl(Game& g) {
    if (g.display != EGL_NO_DISPLAY) {
        eglMakeCurrent(g.display, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT);
        if (g.context != EGL_NO_CONTEXT) eglDestroyContext(g.display, g.context);
        if (g.surface != EGL_NO_SURFACE) eglDestroySurface(g.display, g.surface);
        eglTerminate(g.display);
    }
    g.display = EGL_NO_DISPLAY;
    g.surface = EGL_NO_SURFACE;
    g.context = EGL_NO_CONTEXT;
}

void start_world(Game& g) {
    g.world.chunks.clear();
    const int workers = std::clamp(static_cast<int>(std::thread::hardware_concurrency()) - 1, 1, 3);
    g.streamer = std::make_unique<mc::ChunkStreamer>(g.world, SEED, workers);
    if (!g.have_player) {
        mc::Spawn sp = mc::find_spawn(SEED);
        g.player = mc::Player();
        g.player.pos = {sp.x, sp.y, sp.z};
        g.have_player = true;
        LOG("spawn %.1f %.1f %.1f", sp.x, sp.y, sp.z);
    }
}

void layout_hud(Game& g) {
    const float u = static_cast<float>(std::min(g.width, g.height));
    g.hud.jump_r = u * 0.11f;
    g.hud.jump_cx = g.width - u * 0.2f;
    g.hud.jump_cy = g.height - u * 0.22f;
    g.hud.jump_down = g.jump.id >= 0;
    g.hud.stick_r = 0;
    if (g.stick.id >= 0) {
        g.hud.stick_r = u * 0.16f;
        g.hud.stick_cx = g.stick.sx;
        g.hud.stick_cy = g.stick.sy;
        float dx = g.stick.x - g.stick.sx, dy = g.stick.y - g.stick.sy;
        const float len = std::sqrt(dx * dx + dy * dy);
        if (len > g.hud.stick_r) { dx *= g.hud.stick_r / len; dy *= g.hud.stick_r / len; }
        g.hud.knob_x = g.stick.sx + dx;
        g.hud.knob_y = g.stick.sy + dy;
    }
}

int32_t on_input(android_app* app, AInputEvent* ev) {
    auto* g = static_cast<Game*>(app->userData);
    if (AInputEvent_getType(ev) != AINPUT_EVENT_TYPE_MOTION) return 0;
    const int32_t action = AMotionEvent_getAction(ev);
    const int32_t kind = action & AMOTION_EVENT_ACTION_MASK;
    const size_t idx = (action & AMOTION_EVENT_ACTION_POINTER_INDEX_MASK) >> AMOTION_EVENT_ACTION_POINTER_INDEX_SHIFT;
    const float u = static_cast<float>(std::min(g->width, g->height));

    auto release = [&](int32_t id) {
        if (g->stick.id == id) g->stick.id = -1;
        if (g->look.id == id) g->look.id = -1;
        if (g->jump.id == id) g->jump.id = -1;
    };

    if (kind == AMOTION_EVENT_ACTION_DOWN || kind == AMOTION_EVENT_ACTION_POINTER_DOWN) {
        const int32_t id = AMotionEvent_getPointerId(ev, idx);
        const float x = AMotionEvent_getX(ev, idx), y = AMotionEvent_getY(ev, idx);
        const float jx = x - g->hud.jump_cx, jy = y - g->hud.jump_cy;
        if (g->jump.id < 0 && jx * jx + jy * jy <= g->hud.jump_r * g->hud.jump_r * 1.6f) {
            g->jump = {id, x, y, x, y, x, y};
        } else if (g->stick.id < 0 && x < g->width * 0.4f) {
            g->stick = {id, x, y, x, y, x, y};
        } else if (g->look.id < 0) {
            g->look = {id, x, y, x, y, x, y};
        }
    } else if (kind == AMOTION_EVENT_ACTION_UP || kind == AMOTION_EVENT_ACTION_POINTER_UP) {
        release(AMotionEvent_getPointerId(ev, idx));
    } else if (kind == AMOTION_EVENT_ACTION_CANCEL) {
        g->stick.id = g->look.id = g->jump.id = -1;
    } else if (kind == AMOTION_EVENT_ACTION_MOVE) {
        const size_t count = AMotionEvent_getPointerCount(ev);
        for (size_t i = 0; i < count; i++) {
            const int32_t id = AMotionEvent_getPointerId(ev, i);
            const float x = AMotionEvent_getX(ev, i), y = AMotionEvent_getY(ev, i);
            if (id == g->stick.id) { g->stick.x = x; g->stick.y = y; }
            if (id == g->look.id) { g->look.x = x; g->look.y = y; }
            if (id == g->jump.id) { g->jump.x = x; g->jump.y = y; }
        }
    }
    (void)u;
    return 1;
}

void on_cmd(android_app* app, int32_t cmd) {
    auto* g = static_cast<Game*>(app->userData);
    switch (cmd) {
        case APP_CMD_INIT_WINDOW:
            if (app->window && init_egl(*g, app->window)) {
                g->gl_ready = g->renderer.init();
                g->renderer.resize(g->width, g->height);
                g->renderer.set_view_distance_chunks(VIEW_RADIUS);
                start_world(*g);
                g->last_frame = std::chrono::steady_clock::now();
                LOG("GL ready %dx%d", g->width, g->height);
            } else {
                LOG("EGL init failed");
            }
            break;
        case APP_CMD_TERM_WINDOW:
            // The GL context goes with the window: drop the GPU side and the worker threads, keep the player.
            g->streamer.reset();
            g->world.chunks.clear();
            if (g->gl_ready) g->renderer.shutdown();
            g->gl_ready = false;
            term_egl(*g);
            g->stick.id = g->look.id = g->jump.id = -1;
            break;
        case APP_CMD_WINDOW_RESIZED:
        case APP_CMD_CONFIG_CHANGED:
            if (g->gl_ready) {
                eglQuerySurface(g->display, g->surface, EGL_WIDTH, &g->width);
                eglQuerySurface(g->display, g->surface, EGL_HEIGHT, &g->height);
                g->renderer.resize(g->width, g->height);
            }
            break;
        default: break;
    }
}

void frame(Game& g) {
    const auto now = std::chrono::steady_clock::now();
    float dt = std::chrono::duration<float>(now - g.last_frame).count();
    g.last_frame = now;
    dt = std::min(dt, 0.05f);

    // Look: finger delta since the last frame.
    if (g.look.id >= 0) {
        const float sens = 0.0042f;  // radians per pixel
        g.player.yaw -= (g.look.x - g.look.lx) * sens;
        g.player.pitch -= (g.look.y - g.look.ly) * sens;
        g.player.pitch = std::clamp(g.player.pitch, -1.55f, 1.55f);
        g.look.lx = g.look.x;
        g.look.ly = g.look.y;
    }
    // Move: stick offset, full speed at one radius.
    float strafe = 0, fwd = 0;
    layout_hud(g);
    if (g.stick.id >= 0 && g.hud.stick_r > 0) {
        strafe = (g.hud.knob_x - g.hud.stick_cx) / g.hud.stick_r;
        fwd = -(g.hud.knob_y - g.hud.stick_cy) / g.hud.stick_r;
    }

    // Streaming: meshes up to the GPU, dropped chunks off it.
    std::vector<mc::MeshResult> fresh;
    std::vector<std::pair<int, int>> dropped;
    const int pcx = static_cast<int>(std::floor(g.player.pos.x)) >> 4;
    const int pcz = static_cast<int>(std::floor(g.player.pos.z)) >> 4;
    g.streamer->update(pcx, pcz, VIEW_RADIUS, fresh, dropped);
    int uploaded = 0;
    for (auto& m : fresh) {
        g.renderer.add_mesh(m.cx, m.cz, m.data);
        if (++uploaded >= 64) break;  // (all of them; the cap only guards a pathological frame)
    }
    for (auto& d : dropped) g.renderer.drop_mesh(d.first, d.second);

    // Physics only once the chunk under the player exists, so nobody drops through unloaded ground.
    if (g.world.get_chunk(pcx, pcz)) {
        mc::step_player(g.player, g.world, dt, strafe, fwd, g.jump.id >= 0);
        if (g.player.pos.y < -20.0f) {  // fell out of the world: back to the surface
            const int top = g.world.top_solid(static_cast<int>(std::floor(g.player.pos.x)),
                                              static_cast<int>(std::floor(g.player.pos.z)));
            g.player.pos.y = static_cast<float>(std::max(top, mc::SEA_LEVEL) + 2) + mc::HALF_H;
            g.player.vel = {};
        }
    }

    g.renderer.draw(g.player, g.hud);
    eglSwapBuffers(g.display, g.surface);
}

}  // namespace

void android_main(android_app* app) {
    static Game game;  // survives window destruction; the activity process outlives the surface
    app->userData = &game;
    app->onAppCmd = on_cmd;
    app->onInputEvent = on_input;

    while (!app->destroyRequested) {
        int events;
        android_poll_source* source;
        // Block while there is nothing to draw, spin when there is.
        while (ALooper_pollOnce(game.gl_ready ? 0 : -1, nullptr, &events, reinterpret_cast<void**>(&source)) >= 0) {
            if (source) source->process(app, source);
            if (app->destroyRequested) break;
        }
        if (game.gl_ready && game.display != EGL_NO_DISPLAY) frame(game);
    }
    game.streamer.reset();
    if (game.gl_ready) game.renderer.shutdown();
    term_egl(game);
}
