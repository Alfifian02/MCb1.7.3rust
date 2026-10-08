//! mc-rs: Minecraft b1.7.3 in Rust for Android.
//!
//! M12 - Landscape touch UX (move stick, look drag, jump button, hotbar, pause menu) on
//! top of M2 (camera + physics) and M3 (overworld generation), with the world streamed by
//! `world::chunks::ChunkManager` (render-distance ring, generation on worker threads).

mod render;
mod world;
mod gpu;
mod input;
mod immersive;

use crate::input::touch_ui::{PointerRole, TouchUi};
use crate::render::hud::{HudPipeline, HudVertex};

use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};
use android_activity::input::{Axis, InputEvent as IEv};
use core::ffi::c_void;
use std::time::{Duration, Instant};

use crate::gpu::context::Gpu;
use crate::gpu::pipeline::ChunkPipeline;
use crate::render::camera::FirstPersonCamera;
use crate::world::chunks::{chunk_coord, generate, ChunkManager};
use crate::world::gen::chunk_manager::WorldChunkManager;
use crate::world::gen::overworld::OverworldGenerator;
use crate::world::physics::{self, Player};

/// Render distance in chunks (a circle of this radius is meshed and drawn, one more is generated).
const RENDER_DIST: i32 = 4;

const LOOK_SENS: f32 = 0.004;
/// World units per second when the d-pad is fully pressed.
const MOVE_SPEED: f32 = 4.3;
/// Eye offset from `player.pos`, which is the CENTRE of the 1.8-tall box. Steve's eyes are
/// 1.62 above his feet (EntityPlayer.yOffset), i.e. 1.62 - 0.9 above the centre.
const EYE_HEIGHT: f32 = 1.62 - physics::HALF.y;

/// Hotbar swatch colors (RGBA). Cycle through the same block ids the mesher
/// knows about, so the player can tell at a glance which slot is selected.
fn hotbar_color(slot: usize) -> [f32; 4] {
    // Match the 1-byte ids from world::gen::overworld::block.
    let id = match slot {
        0 => 1,  // stone
        1 => 2,  // grass
        2 => 3,  // dirt
        3 => 12, // sand
        4 => 4,  // cobblestone
        5 => 5,  // planks
        6 => 14, // gold ore
        7 => 56, // diamond
        _ => 16, // coal
    };
    // Approximate Beta-1.7 colors (same numbers as render::atlas::block_color).
    let px = match id {
        1 => [125, 125, 125],
        2 => [110, 170, 70],
        3 => [134, 96, 67],
        4 => [200, 200, 200],
        5 => [220, 200, 100],
        12 => [225, 215, 160],
        14 => [240, 220, 60],
        16 => [60, 60, 60],
        56 => [180, 240, 240],
        _ => [180, 30, 200],
    };
    [px[0] as f32 / 255.0, px[1] as f32 / 255.0, px[2] as f32 / 255.0, 1.0]
}

struct App {
    gpu: Gpu,
    pipe: ChunkPipeline,
    chunks: ChunkManager,
    camera: FirstPersonCamera,
    player: Player,
    last_frame: Instant,
    frames: u64,
    fps_last: Instant,
    /// FPS counter: average over the last second, and over the whole run (from `started`).
    fps_recent: u32,
    fps_overall: u32,
    total_frames: u64,
    started: Instant,
    // M12: touch UI state + HUD renderer.
    touch: TouchUi,
    hud: HudPipeline,
    /// Scratch vertex buffer for the HUD; cleared each frame, then emitted.
    hud_verts: Vec<HudVertex>,
}

impl App {
    async fn init(native_ptr: *mut c_void, width: u32, height: u32) -> Result<Self, String> {
        let gpu = Gpu::from_android_window(native_ptr, width, height).await?;
        let surface_format = gpu.surface_format();
        let pipe = ChunkPipeline::new(&gpu.device, surface_format);
        pipe.upload_atlas(&gpu.queue);

        // The spawn search needs real terrain before the first frame, so the 3x3 chunks around
        // the origin are generated here (init runs on its own thread, not the render loop) and
        // handed to the manager; everything further out streams in on the worker threads.
        const SEED: i64 = 0xCAFEBABE;
        let mut chunks = ChunkManager::new(SEED, RENDER_DIST);
        let mut generator = OverworldGenerator::new(SEED);
        let mut climate = WorldChunkManager::new(SEED);
        // Spawn on dry land closest to the centre of chunk (0, 0). A fixed spawn was under the
        // sea (top block y=60 < sea level 64), which put the camera inside water blocks.
        let mut best: Option<(i32, i32, i32, i32)> = None; // x, z, top, dist^2
        let mut highest = (8, 8, i32::MIN);
        for cz in -1..=1 {
            for cx in -1..=1 {
                let blocks = generate(&mut generator, &mut climate, cx, cz);
                for lz in 0..16usize {
                    for lx in 0..16usize {
                        let t = OverworldGenerator::top_block(&blocks, lx, lz, 16);
                        let (x, z) = (cx * 16 + lx as i32, cz * 16 + lz as i32);
                        if t > highest.2 { highest = (x, z, t); }
                        if t >= 64 {
                            let d2 = (x - 8).pow(2) + (z - 8).pow(2);
                            if best.map_or(true, |b| d2 < b.3) { best = Some((x, z, t, d2)); }
                        }
                    }
                }
                chunks.insert((cx, cz), blocks);
            }
        }
        let (spawn_x, spawn_z, top) = match best {
            Some((x, z, t, _)) => (x, z, t),
            None => highest,
        };
        let spawn_feet_y = (top as f32) + 1.0 + 0.9;
        log::info!("chunks: render distance {}, spawn at ({}, {}, {}), top block y={}", RENDER_DIST, spawn_x, spawn_feet_y, spawn_z, top);

        let mut camera = FirstPersonCamera::spawn_at(spawn_x as f32 + 0.5, spawn_feet_y + EYE_HEIGHT, spawn_z as f32 + 0.5);
        // spawn_at defaults to aspect 1.0 and resize() only runs when the size changes, so
        // without this the 3D view is squashed horizontally onto the real screen shape.
        camera.aspect = width as f32 / height as f32;
        let player = Player {
            pos: glam::Vec3::new(spawn_x as f32 + 0.5, spawn_feet_y, spawn_z as f32 + 0.5),
            vel: glam::Vec3::ZERO,
            on_ground: false,
        };
        camera.pos = player.pos + glam::Vec3::new(0.0, EYE_HEIGHT, 0.0);

        // M12: HUD pipeline + touch state machine. Surface dimensions match
        // the window we just initialised against.
        let touch = TouchUi::new(width, height);
        let hud = HudPipeline::new(&gpu.device, &gpu.queue, surface_format, width, height);

        Ok(Self {
            gpu, pipe,
            chunks, camera,
            player,
            last_frame: Instant::now(),
            frames: 0,
            fps_last: Instant::now(),
            fps_recent: 0,
            fps_overall: 0,
            total_frames: 0,
            started: Instant::now(),
            touch,
            hud,
            hud_verts: Vec::with_capacity(256),
        })
    }

    fn resize(&mut self, w: u32, h: u32) {
        self.gpu.resize(w, h);
        self.camera.aspect = w as f32 / h as f32;
        self.touch.ensure_layout(w, h);
        self.hud.resize(&self.gpu.queue, w, h);
    }

    fn step_frame(&mut self) {
        let now = Instant::now();
        let mut dt = now.duration_since(self.last_frame).as_secs_f32();
        if dt > 1.0 / 30.0 { dt = 1.0 / 30.0; }
        if dt < 0.0 { dt = 0.0; }
        self.last_frame = now;
        let chunks = &self.chunks;
        // Look drag accumulated by the touch UI since the last frame. Drained
        // even when paused so a drag started before pausing doesn't replay.
        let (look_dx, look_dy) = self.touch.take_look();
        // Pause: freeze the world but keep the timer warm so un-pause doesn't
        // produce a giant dt step (which would either teleport the player or
        // kill their fall).
        if self.touch.paused {
            // Player physics is still ticked so they don't fall through the
            // world if pause was opened mid-air, but gravity is zeroed.
            self.player.vel = glam::Vec3::ZERO;
            self.camera.pos = self.player.pos + glam::Vec3::new(0.0, EYE_HEIGHT, 0.0);
            return;
        }
        self.camera.add_yaw(look_dx * LOOK_SENS);
        self.camera.add_pitch(look_dy * LOOK_SENS);
        // Analog move stick: rotate the camera-frame (fwd, side) by yaw into
        // world-space velocity. Set every frame (zero when the stick is idle),
        // otherwise the player keeps sliding after the finger lifts.
        let (fwd, side) = self.touch.move_input;
        let yaw = self.camera.yaw;
        // forward = -Z when yaw = 0; rotate by yaw around +Y.
        self.player.vel.x = (fwd * yaw.sin() + side * yaw.cos()) * MOVE_SPEED;
        self.player.vel.z = (-fwd * yaw.cos() + side * yaw.sin()) * MOVE_SPEED;
        // Held jump button; 8.4 m/s matches the b1.7.3 jump velocity.
        if self.touch.jumping() && self.player.on_ground {
            self.player.vel.y = 8.4;
        }
        let get = |x: i32, y: i32, z: i32| chunks.block(x, y, z);
        physics::step(&mut self.player, dt, &get);
        self.camera.pos = self.player.pos + glam::Vec3::new(0.0, EYE_HEIGHT, 0.0);
    }

    fn render(&mut self) {
        self.step_frame();
        let (pcx, pcz) = (chunk_coord(self.player.pos.x), chunk_coord(self.player.pos.z));
        self.chunks.update(&self.gpu.device, pcx, pcz);
        let (view, proj) = self.camera.build_view_proj();
        self.pipe.upload_uniforms(&self.gpu.queue, view, proj);

        // Sky-blue clear color (the red/green debug cycling is no longer needed).
        let clear = wgpu::Color { r: 0.6, g: 0.8, b: 1.0, a: 1.0 };
        if self.frames == 0 {
            log::info!("render: first frame, surface_format={:?}, {} chunks loaded", self.gpu.surface_format(), self.chunks.loaded());
        }

        let frame = match self.gpu.surface.get_current_texture() {
            Ok(f) => f,
            Err(e) => {
                log::warn!("surface.get_current_texture: {e:?}");
                if matches!(e, wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) {
                    self.gpu.surface.configure(&self.gpu.device, &self.gpu.config);
                }
                return;
            }
        };
        // Reconcile surface/config size. Native-activity doesn't always send
        // MainEvent::WindowResized before the first frame, so the cached
        // config.width/height can be wrong (1x1 from from_android_window).
        // We drop the stale one (wgpu invalidates it after configure), resize, return.
        let actual_w = frame.texture.width();
        let actual_h = frame.texture.height();
        if actual_w != self.gpu.config.width || actual_h != self.gpu.config.height {
            log::info!("M3 render: frame is {actual_w}x{actual_h}, reconfiguring (was {}x{})", self.gpu.config.width, self.gpu.config.height);
            self.resize(actual_w, actual_h);
            return;
        }
        let view_tex = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut enc = self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });

        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("chunk_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view_tex,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.gpu.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rp.set_pipeline(&self.pipe.pipeline);
            rp.set_bind_group(0, &self.pipe.bind_group, &[]);
            // One draw per chunk mesh. M13: frustum-test each chunk's bounds here (see chunks.rs).
            for m in self.chunks.meshes() {
                rp.set_vertex_buffer(0, m.vbuf.slice(..));
                rp.set_index_buffer(m.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(0..m.index_count, 0, 0..1);
            }
        }
        // M12: HUD overlay pass. Built into the same encoder so the HUD
        // never gets lost if the GPU drops a frame.
        self.build_hud();
        let hud_vertex_count = self.hud_verts.len();
        if hud_vertex_count > 0 {
            self.gpu.queue.write_buffer(
                &self.hud.vbuf,
                0,
                bytemuck::cast_slice(&self.hud_verts),
            );
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hud_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view_tex,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        // Preserve the chunk render; just draw on top.
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rp.set_pipeline(&self.hud.pipeline);
            rp.set_bind_group(0, &self.hud.bind_group, &[]);
            rp.set_vertex_buffer(0, self.hud.vbuf.slice(..));
            rp.draw(0..hud_vertex_count as u32, 0..1);
        }
        self.gpu.queue.submit(std::iter::once(enc.finish()));
        frame.present();

        self.frames += 1;
        self.total_frames += 1;
        let now = Instant::now();
        if now.duration_since(self.fps_last).as_secs_f32() >= 1.0 {
            let fps = self.frames as f32 / now.duration_since(self.fps_last).as_secs_f32();
            self.fps_recent = fps.round() as u32;
            self.fps_overall = (self.total_frames as f32 / now.duration_since(self.started).as_secs_f32()).round() as u32;
            log::info!("FPS: {fps:.1} (average since start {}), {} chunks loaded", self.fps_overall, self.chunks.loaded());
            self.frames = 0;
            self.fps_last = now;
        }
    }

    fn on_motion(&mut self, ev: &android_activity::input::MotionEvent) {
        self.touch.handle_motion(ev);
    }

    /// Build the HUD vertex buffer for this frame. Called between the chunk
    /// and HUD render passes inside `render`.
    fn build_hud(&mut self) {
        let v = &mut self.hud_verts;
        v.clear();
        let layout = &self.touch.layout;
        let paused = self.touch.paused;
        let pressed = |role: PointerRole| self.touch.pointers.values().any(|p| p.role == role);

        // Pause button: top-right, two bars.
        let (px, py, pw, ph) = layout.pause;
        let pause_fill = if pressed(PointerRole::Pause) { [0.30, 0.55, 0.85, 0.95] } else { [0.20, 0.40, 0.70, 0.75] };
        HudPipeline::push_outlined_quad(v, px, py, pw, ph, pause_fill, [0.95, 0.95, 0.95, 0.9], 3.0);
        let bar_w = pw * 0.18;
        let bar_h = ph * 0.50;
        let bar_y = py + (ph - bar_h) * 0.5;
        let bx0 = px + (pw - bar_w * 3.0) * 0.5;
        HudPipeline::push_quad(v, bx0, bar_y, bar_w, bar_h, [1.0; 4]);
        HudPipeline::push_quad(v, bx0 + bar_w * 2.0, bar_y, bar_w, bar_h, [1.0; 4]);

        // Hotbar: 9 cells, selected slot highlighted.
        for (i, &(hx, hy, hw, hh)) in layout.hotbar.iter().enumerate() {
            let border = if i == self.touch.hotbar_slot { [1.0, 1.0, 1.0, 1.0] } else { [0.85, 0.85, 0.85, 0.85] };
            HudPipeline::push_outlined_quad(v, hx, hy, hw, hh, [0.10, 0.10, 0.10, 0.55], border, 3.0);
            let pad = (hw.min(hh) * 0.18).max(2.0);
            HudPipeline::push_quad(v, hx + pad, hy + pad, hw - pad * 2.0, hh - pad * 2.0, hotbar_color(i));
        }

        // Jump button: bottom-right disc with a ring.
        let (jx, jy) = layout.jump_center;
        let jr = layout.jump_radius;
        let jump_fill = if self.touch.jumping() { [0.45, 0.65, 0.95, 0.80] } else { [0.18, 0.20, 0.26, 0.50] };
        HudPipeline::push_disc(v, jx, jy, jr, jump_fill);
        HudPipeline::push_ring(v, jx, jy, jr, jr * 0.92, [0.95, 0.95, 0.95, 0.7]);

        // Move stick: base at the finger's anchor while held, a faint hint at
        // the home position otherwise.
        let sr = layout.stick_radius;
        match self.touch.stick_state() {
            Some(((ax, ay), (fx, fy))) => {
                let (dx, dy) = (fx - ax, fy - ay);
                let len = (dx * dx + dy * dy).sqrt().max(1.0);
                let k = (len.min(sr)) / len;
                HudPipeline::push_ring(v, ax, ay, sr, sr * 0.94, [0.95, 0.95, 0.95, 0.5]);
                HudPipeline::push_disc(v, ax, ay, sr, [0.18, 0.20, 0.26, 0.35]);
                HudPipeline::push_disc(v, ax + dx * k, ay + dy * k, sr * 0.45, [0.45, 0.65, 0.95, 0.85]);
            }
            None => {
                let (hx, hy) = layout.stick_home;
                HudPipeline::push_ring(v, hx, hy, sr, sr * 0.94, [0.95, 0.95, 0.95, 0.22]);
                HudPipeline::push_disc(v, hx, hy, sr * 0.45, [0.95, 0.95, 0.95, 0.15]);
            }
        }

        // FPS counter, top-left. White = average over the last second, yellow = average since
        // the app started (includes the chunk loading at the start).
        {
            let dh = (self.gpu.config.height as f32 * 0.05).max(14.0);
            let (fx, fy) = (self.gpu.config.width as f32 * 0.07, dh * 0.6);
            HudPipeline::push_quad(v, fx - dh * 0.3, fy - dh * 0.3, dh * 3.2, dh * 2.7, [0.0, 0.0, 0.0, 0.5]);
            HudPipeline::push_number(v, fx, fy, dh, self.fps_recent, [1.0, 1.0, 1.0, 1.0]);
            HudPipeline::push_number(v, fx, fy + dh * 1.3, dh, self.fps_overall, [1.0, 0.85, 0.2, 1.0]);
        }

        // Pause menu: dim the frame and show the resume button.
        if paused {
            let w = self.gpu.config.width as f32;
            let h = self.gpu.config.height as f32;
            HudPipeline::push_quad(v, 0.0, 0.0, w, h, [0.0, 0.0, 0.0, 0.55]);
            let (bx, by, bw, bh) = layout.resume;
            HudPipeline::push_outlined_quad(v, bx, by, bw, bh, [0.20, 0.30, 0.50, 0.95], [0.95, 0.95, 0.95, 0.95], 4.0);
            // Play triangle, approximated by two stacked quads.
            let tx = bx + bw * 0.46;
            let ty = by + bh * 0.25;
            let tw = bw * 0.08;
            let th = bh * 0.50;
            HudPipeline::push_quad(v, tx, ty, tw, th * 0.5, [1.0; 4]);
            HudPipeline::push_quad(v, tx, ty + th * 0.5, tw, th * 0.5, [1.0; 4]);
        }

        // Cap to the buffer capacity; extra quads are silently dropped.
        let max = self.hud.quad_capacity * 6;
        if v.len() > max {
            v.truncate(max);
        }
    }
}

/// Match the swapchain to the real window size. Hiding the nav bar or rotating changes the
/// window size; a swapchain left at the old size gets scaled by Android to fit = stretched.
fn sync_size(app: &AndroidApp, state: &mut Option<App>) {
    let (Some(a), Some(win)) = (state.as_mut(), app.native_window()) else { return };
    let (w, h) = (win.width().max(1) as u32, win.height().max(1) as u32);
    if (w, h) != (a.gpu.config.width, a.gpu.config.height) {
        a.resize(w, h);
    }
}

#[no_mangle]
fn android_main(app: AndroidApp) {
    #[cfg(target_os = "android")]
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("mc-rs"),
    );
    #[cfg(not(target_os = "android"))]
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();

    std::panic::set_hook(Box::new(|info| {
        log::error!("PANIC: {info}");
    }));

    let mut self_main_frames: u64 = 0;
    let mut app_state: Option<App> = None;
    let mut init_handle: Option<std::thread::JoinHandle<Result<App, String>>> = None;
    let mut running = true;

    while running {
        app.poll_events(Some(Duration::from_millis(8)), |event| {
            if let PollEvent::Main(main_event) = event {
                match main_event {
                    MainEvent::InitWindow { .. } => {
                        immersive::hide_system_bars(&app);
                        struct NativePtr(usize);
                        unsafe impl Send for NativePtr {}
                        let Some(window) = app.native_window() else { return };
                        let ptr = window.ptr().as_ptr() as usize;
                        let init_w = window.width().max(1) as u32;
                        let init_h = window.height().max(1) as u32;
                        log::info!("M3: InitWindow, native {}x{}, starting GPU init thread", init_w, init_h);
                        let h = std::thread::Builder::new()
                            .stack_size(8 * 1024 * 1024)
                            .spawn(move || {
                                let r = std::panic::catch_unwind(|| {
                                    pollster::block_on(App::init(ptr as *mut c_void, init_w, init_h))
                                });
                                match r {
                                    Ok(Ok(a)) => Ok(a),
                                    Ok(Err(e)) => {
                                        log::error!("M3 init error: {e}");
                                        Err(e)
                                    }
                                    Err(p) => {
                                        let msg = if let Some(s) = p.downcast_ref::<&str>() {
                                            s.to_string()
                                        } else if let Some(s) = p.downcast_ref::<String>() {
                                            s.clone()
                                        } else {
                                            "<unknown panic>".to_string()
                                        };
                                        log::error!("M3 init panic: {msg}");
                                        Err(format!("panic: {msg}"))
                                    }
                                }
                            })
                            .expect("spawn");
                        init_handle = Some(h);
                    }
                    MainEvent::InputAvailable => {
                        if let Ok(mut iter) = app.input_events_iter() {
                            // `next` handles one queued event per call; loop so
                            // multi-touch moves and Up events are never dropped.
                            while iter.next(|event| {
                                if let IEv::MotionEvent(m) = event {
                                    if let Some(a) = app_state.as_mut() {
                                        a.on_motion(&m);
                                    }
                                }
                                InputStatus::Unhandled
                            }) {}
                        }
                    }
                    MainEvent::TerminateWindow { .. } => {
                        // Window is going away (home button, rotate, etc).
                        // Drop the surface before the ANativeWindow dies.
                        app_state = None;
                    }
                    MainEvent::GainedFocus => {
                        immersive::hide_system_bars(&app);
                        sync_size(&app, &mut app_state);
                    }
                    MainEvent::WindowResized { .. }
                    | MainEvent::ContentRectChanged { .. }
                    | MainEvent::InsetsChanged { .. }
                    | MainEvent::RedrawNeeded { .. } => sync_size(&app, &mut app_state),
                    MainEvent::Destroy { .. } => running = false,
                    _ => {}
                }
            }
        });

        if init_handle.as_ref().map_or(false, |h| h.is_finished()) {
            if let Some(h) = init_handle.take() {
                match h.join() {
                    Ok(Ok(a)) => {
                        if app.native_window().is_some() {
                            app_state = Some(a);
                            sync_size(&app, &mut app_state);
                            self_main_frames = 0;
                        } else {
                            log::warn!("init finished but window is gone; discarding");
                        }
                    }
                    Ok(Err(e)) => log::error!("init failed: {e}"),
                    Err(_) => log::error!("init panicked"),
                }
            }
        }

        if let Some(a) = app_state.as_mut() {
            a.render();
            self_main_frames += 1;
            if self_main_frames == 1 || self_main_frames == 30 || self_main_frames % 300 == 0 {
                log::info!("M3 main frame {self_main_frames}");
            }
        }
        if self_main_frames % 30 == 0 {
            sync_size(&app, &mut app_state);
        }
    }
}
