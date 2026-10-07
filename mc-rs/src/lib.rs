//! mc-rs: Minecraft b1.7.3 in Rust for Android.
//!
//! M2 - First-person camera + swept AABB physics. Touch-drag = look.

mod render;
mod world;
mod gpu;

use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};
use android_activity::input::{Axis, InputEvent as IEv, MotionAction};
use core::ffi::c_void;
use std::time::{Duration, Instant};

use crate::gpu::context::Gpu;
use crate::gpu::pipeline::{ChunkPipeline, Vertex, create_index_buffer, create_vertex_buffer};
use crate::render::camera::FirstPersonCamera;
use crate::render::mesh;
use crate::world::chunk::{Chunk, W, H, D};
use crate::world::biome::Biome;
use crate::world::gen::overworld::OverworldGenerator;
use crate::world::physics::{self, Player};

const LOOK_SENS: f32 = 0.004;
const EYE_HEIGHT: f32 = 1.62;

struct App {
    gpu: Gpu,
    pipe: ChunkPipeline,
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    index_count: u32,
    chunk: Chunk,
    generator: OverworldGenerator,
    camera: FirstPersonCamera,
    player: Player,
    last_frame: Instant,
    frames: u64,
    fps_last: Instant,
    look_pid: Option<i32>,
    look_last: Option<(f32, f32)>,
}

impl App {
    async fn init(native_ptr: *mut c_void, width: u32, height: u32) -> Result<Self, String> {
        let gpu = Gpu::from_android_window(native_ptr, width, height).await?;
        let surface_format = gpu.surface_format();
        let pipe = ChunkPipeline::new(&gpu.device, surface_format);
        pipe.upload_atlas(&gpu.queue);

        // M3d: build a 3x3 grid of chunks and stitch them into a 48x128x48
        // "super-chunk" so the player can walk off the original 16x16 boundary
        // and see neighboring terrain. The mesh + physics still use a single
        // 48-wide X stride, so no neighbor culling changes are needed.
        let mut generator = OverworldGenerator::new(0xCAFEBABEu64);
        let biomes = [Biome::Plains; 256];
        const SUPER_W: usize = 16 * 3; // 48
        const SUPER_H: usize = 128;
        const SUPER_D: usize = 16 * 3; // 48
        const SUPER_VOLUME: usize = SUPER_W * SUPER_H * SUPER_D;
        let mut super_blocks: Vec<u8> = vec![0; SUPER_VOLUME];
        // Order: chunks(cx, cz) for cx in -1..=1, cz in -1..=1
        for cz_off in -1..=1 {
            for cx_off in -1..=1 {
                let blocks = generator.generate(cx_off, cz_off, &biomes);
                let x0 = ((cx_off + 1) as usize) * 16;
                let z0 = ((cz_off + 1) as usize) * 16;
                for z in 0..16 {
                    for x in 0..16 {
                        for y in 0..128 {
                            let src = (x << 11) | (z << 7) | y;
                            // Super-chunk uses (x << 11) | (z << 7) | y too,
                            // but x/z are now global to the 48-wide grid.
                            let dst = ((x + x0) << 11) | ((z + z0) << 7) | y;
                            super_blocks[dst] = blocks[src];
                        }
                    }
                }
            }
        }
        // Treat the super-chunk as a single Chunk (same layout).
        // M3e-ores: place ore veins in each of the 9 chunks.
        for cz_off in -1..=1 {
            for cx_off in -1..=1 {
                generator.populate_ores(
                    &mut super_blocks,
                    (cx_off * 16, cz_off * 16),
                );
            }
        }

        let chunk = Chunk { blocks: super_blocks.clone() };
        

        // Spawn at the center of the super-chunk (x=24, z=24, the center of
        // chunk (0,0)).
        let spawn_x: usize = 24;
        let spawn_z: usize = 24;
        let top = OverworldGenerator::top_block(&super_blocks, spawn_x, spawn_z);
        let spawn_feet_y = (top as f32) + 1.0 + 0.9;
        log::info!("M3d: super-chunk 48x128x48, spawn at ({}, {}, {}), top block y={}", spawn_x, spawn_feet_y, spawn_z, top);

        let (raw_verts, raw_idxs) = mesh::build_ext(&chunk, 48, 48);
        let mut verts: Vec<Vertex> = Vec::with_capacity(raw_verts.len() / 6);
        for chunk_v in raw_verts.chunks(6) {
            verts.push(Vertex {
                pos: [chunk_v[0], chunk_v[1], chunk_v[2]],
                uv: [chunk_v[3], chunk_v[4]],
                light: chunk_v[5],
            });
        }
        log::info!("M3b: built {} verts, {} idx", verts.len(), raw_idxs.len());

        let vbuf = create_vertex_buffer(&gpu.device, &verts);
        let ibuf = create_index_buffer(&gpu.device, &raw_idxs);
        let index_count = raw_idxs.len() as u32;
        log::info!("M3 init: {} blocks, {} verts, {} idx", chunk.blocks.len(), verts.len(), index_count);

        let mut camera = FirstPersonCamera::spawn_at(spawn_x as f32 + 0.5, spawn_feet_y + EYE_HEIGHT, spawn_z as f32 + 0.5);
        let player = Player {
            pos: glam::Vec3::new(spawn_x as f32 + 0.5, spawn_feet_y, spawn_z as f32 + 0.5),
            vel: glam::Vec3::ZERO,
            on_ground: false,
        };
        camera.pos = player.pos + glam::Vec3::new(0.0, EYE_HEIGHT, 0.0);

        Ok(Self {
            gpu, pipe, vbuf, ibuf, index_count,
            chunk, generator, camera,
            player,
            last_frame: Instant::now(),
            frames: 0,
            fps_last: Instant::now(),
            look_pid: None,
            look_last: None,
        })
    }

    fn resize(&mut self, w: u32, h: u32) {
        self.gpu.resize(w, h);
        self.camera.aspect = w as f32 / h as f32;
    }

    fn step_frame(&mut self) {
        let now = Instant::now();
        let mut dt = now.duration_since(self.last_frame).as_secs_f32();
        if dt > 1.0 / 30.0 { dt = 1.0 / 30.0; }
        if dt < 0.0 { dt = 0.0; }
        self.last_frame = now;
        let blocks = &self.chunk.blocks;
        // M3d: super-chunk is 48x128x48. Layout is still (x<<11)|(z<<7)|y,
        // so the only change is the bound check.
        let get = |x: i32, y: i32, z: i32| -> Option<u8> {
            if x < 0 || y < 0 || z < 0 { return None; }
            let (x, y, z) = (x as usize, y as usize, z as usize);
            if x >= 48 || y >= 128 || z >= 48 { return None; }
            Some(blocks[(x << 11) | (z << 7) | y])
        };
        physics::step(&mut self.player, dt, &get);
        self.camera.pos = self.player.pos + glam::Vec3::new(0.0, EYE_HEIGHT, 0.0);
    }

    fn render(&mut self) {
        self.step_frame();
        let (view, proj) = self.camera.build_view_proj();
        self.pipe.upload_uniforms(&self.gpu.queue, view, proj);

        // M3e-atlas-fix debug: cycle clear color so a black screen is obvious.
        // phase 0 = sky, phase 1 = red, phase 2 = green.
        let phase = (self.frames / 30) % 3;
        let clear = match phase {
            0 => wgpu::Color { r: 0.6, g: 0.8, b: 1.0, a: 1.0 },
            1 => wgpu::Color { r: 1.0, g: 0.2, b: 0.2, a: 1.0 },
            _ => wgpu::Color { r: 0.2, g: 1.0, b: 0.2, a: 1.0 },
        };
        if self.frames == 0 {
            log::info!("M3 render: first frame, surface_format={:?}, index_count={}, drawing chunk", self.gpu.surface_format(), self.index_count);
        }

        let frame = match self.gpu.surface.get_current_texture() {
            Ok(f) => f,
            Err(e) => { log::warn!("surface.get_current_texture: {e:?}"); return; }
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
            rp.set_vertex_buffer(0, self.vbuf.slice(..));
            rp.set_index_buffer(self.ibuf.slice(..), wgpu::IndexFormat::Uint32);
            rp.draw_indexed(0..self.index_count, 0, 0..1);
        }
        self.gpu.queue.submit(std::iter::once(enc.finish()));
        frame.present();

        self.frames += 1;
        let now = Instant::now();
        if now.duration_since(self.fps_last).as_secs_f32() >= 1.0 {
            let fps = self.frames as f32 / now.duration_since(self.fps_last).as_secs_f32();
            log::info!("FPS: {fps:.1}");
            self.frames = 0;
            self.fps_last = now;
        }
    }

    fn on_motion(&mut self, ev: &android_activity::input::MotionEvent) {
        let find_pos = |pid: i32| -> Option<(f32, f32)> {
            for p in ev.pointers() {
                if p.pointer_id() == pid {
                    return Some((p.axis_value(Axis::X), p.axis_value(Axis::Y)));
                }
            }
            None
        };
        match ev.action() {
            MotionAction::Down => {
                let pid = ev.pointer_at_index(0).pointer_id();
                if let Some(pos) = find_pos(pid) {
                    self.look_pid = Some(pid);
                    self.look_last = Some(pos);
                    if self.player.on_ground {
                        self.player.vel.y = 8.4;
                    }
                }
            }
            MotionAction::PointerDown => {
                if self.look_pid.is_none() {
                    let pid = ev.pointer_at_index(0).pointer_id();
                    if let Some(pos) = find_pos(pid) {
                        self.look_pid = Some(pid);
                        self.look_last = Some(pos);
                    }
                }
            }
            MotionAction::Move => {
                if let Some(cur_pid) = self.look_pid {
                    if let (Some((nx, ny)), Some((lx0, ly0))) = (find_pos(cur_pid), self.look_last) {
                        let dx = nx - lx0;
                        let dy = ny - ly0;
                        log::debug!("M2 touch: move dx={:.1} dy={:.1} yaw={:.2} pitch={:.2}", dx, dy, self.camera.yaw, self.camera.pitch);
                        self.camera.add_yaw(dx * LOOK_SENS);
                        self.camera.add_pitch(dy * LOOK_SENS);
                        self.look_last = Some((nx, ny));
                    }
                }
            }
            MotionAction::Up | MotionAction::Cancel => {
                self.look_pid = None;
                self.look_last = None;
            }
            MotionAction::PointerUp => {
                if let Some(cur_pid) = self.look_pid {
                    if find_pos(cur_pid).is_none() {
                        self.look_pid = None;
                        self.look_last = None;
                    }
                }
            }
            _ => {}
        }
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
                            iter.next(|event| {
                                if let IEv::MotionEvent(m) = event {
                                    if let Some(a) = app_state.as_mut() {
                                        a.on_motion(&m);
                                    }
                                }
                                InputStatus::Unhandled
                            });
                        }
                    }
                    MainEvent::TerminateWindow { .. } => {
                        // Window is going away (home button, rotate, etc).
                        // Drop the surface before the ANativeWindow dies.
                        app_state = None;
                    }
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
    }
}
