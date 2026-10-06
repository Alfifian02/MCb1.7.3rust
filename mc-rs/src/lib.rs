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
    async fn init(native_ptr: *mut c_void) -> Result<Self, String> {
        let gpu = Gpu::from_android_window(native_ptr).await?;
        let surface_format = gpu.surface_format();
        let pipe = ChunkPipeline::new(&gpu.device, surface_format);
        pipe.upload_atlas(&gpu.queue);

        // M3b: build a real overworld chunk and spawn the player on top of it.
        let generator = OverworldGenerator::new(0xCAFEBABEu64);
        let blocks = generator.generate(0, 0);
        // Convert the 16x128x16 blocks into a Chunk (we keep the same storage
        // layout; Chunk is essentially Vec<u8> of length 32768).
        let chunk = Chunk { blocks: blocks.clone() };

        // Find a safe spawn: scan the top surface at chunk center and snap feet
        // to the top solid block + 0.9 (player AABB half-height).
        let spawn_x: usize = 8;
        let spawn_z: usize = 8;
        let top = generator.top_block(&blocks, spawn_x, spawn_z);
        let spawn_feet_y = (top as f32) + 1.0 + 0.9;
        log::info!("M3b: spawn at ({}, {}, {}), top block y={}", spawn_x, spawn_feet_y, spawn_z, top);

        let (raw_verts, raw_idxs) = mesh::build(&chunk);
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
        let get = |x: i32, y: i32, z: i32| -> Option<u8> {
            if x < 0 || y < 0 || z < 0 { return None; }
            let (x, y, z) = (x as usize, y as usize, z as usize);
            if x >= W || y >= H || z >= D { return None; }
            Some(blocks[(x << 11) | (z << 7) | y])
        };
        physics::step(&mut self.player, dt, &get);
        self.camera.pos = self.player.pos + glam::Vec3::new(0.0, EYE_HEIGHT, 0.0);
    }

    fn render(&mut self) {
        self.step_frame();
        let (view, proj) = self.camera.build_view_proj();
        self.pipe.upload_uniforms(&self.gpu.queue, view, proj);

        let frame = match self.gpu.surface.get_current_texture() {
            Ok(f) => f,
            Err(e) => { log::warn!("surface.get_current_texture: {e:?}"); return; }
        };
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
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.6, g: 0.8, b: 1.0, a: 1.0 }),
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
            rp.set_index_buffer(self.ibuf.slice(..), wgpu::IndexFormat::Uint16);
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

    let mut app_state: Option<App> = None;
    let mut init_handle: Option<std::thread::JoinHandle<Result<App, String>>> = None;
    let mut running = true;
    let mut redraw = true;

    while running {
        app.poll_events(Some(Duration::from_millis(8)), |event| {
            if let PollEvent::Main(main_event) = event {
                match main_event {
                    MainEvent::InitWindow { .. } => {
                        struct NativePtr(usize);
                        unsafe impl Send for NativePtr {}
                        let Some(window) = app.native_window() else { return };
                        let ptr = window.ptr().as_ptr() as usize;
                        let h = std::thread::Builder::new()
                            .stack_size(8 * 1024 * 1024)
                            .spawn(move || pollster::block_on(App::init(ptr as *mut c_void)))
                            .expect("spawn");
                        init_handle = Some(h);
                    }
                    MainEvent::WindowResized { .. } => { redraw = true; }
                    MainEvent::RedrawNeeded { .. } => { redraw = true; }
                    MainEvent::InputAvailable => {
                        if let Ok(mut iter) = app.input_events_iter() {
                            iter.next(|event| {
                                if let IEv::MotionEvent(m) = event {
                                    if let Some(a) = app_state.as_mut() {
                                        a.on_motion(&m);
                                        redraw = true;
                                    }
                                }
                                InputStatus::Unhandled
                            });
                        }
                    }
                    MainEvent::Destroy { .. } => running = false,
                    _ => {}
                }
            }
        });

        if let Some(h) = init_handle.take() {
            match h.join() {
                Ok(Ok(a)) => { app_state = Some(a); redraw = true; }
                Ok(Err(e)) => log::error!("init failed: {e}"),
                Err(_) => log::error!("init panicked"),
            }
        }

        if let Some(a) = app_state.as_mut() {
            if redraw {
                a.render();
                redraw = false;
            }
        }
    }
}
