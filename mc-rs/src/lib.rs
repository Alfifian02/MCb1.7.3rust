//! mc-rs: Minecraft b1.7.3 in Rust for Android.
//!
//! M1 - First chunk visible. 16x16x128 stone half-block, fixed camera, no movement.
//! Renderer: wgpu (Vulkan primary, GLES fallback).

mod render;
mod world;
mod gpu;

use android_activity::{AndroidApp, MainEvent, PollEvent};
use core::ffi::c_void;
use std::time::{Duration, Instant};

use crate::gpu::context::Gpu;
use crate::gpu::pipeline::{ChunkPipeline, Vertex, create_index_buffer, create_vertex_buffer};
use crate::render::camera::Camera;
use crate::render::mesh;
use crate::world::chunk::Chunk;

/// Top-level game state. Lives for the duration of the surface.
struct App {
    gpu: Gpu,
    pipe: ChunkPipeline,
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    index_count: u32,
    camera: Camera,
    last_frame: Instant,
    frames: u64,
    fps_last: Instant,
}

impl App {
    async fn init(native_ptr: *mut c_void) -> Result<Self, String> {
        let gpu = Gpu::from_android_window(native_ptr).await?;
        let surface_format = gpu.surface_format();
        let pipe = ChunkPipeline::new(&gpu.device, surface_format);
        pipe.upload_atlas(&gpu.queue);

        // Build a chunk: stone in the bottom half, air above.
        let chunk = Chunk::stone_pillar();
        let (raw_verts, raw_idxs) = mesh::build(&chunk);

        // Re-pack into Vertex structs.
        let mut verts: Vec<Vertex> = Vec::with_capacity(raw_verts.len() / 6);
        for chunk_v in raw_verts.chunks(6) {
            verts.push(Vertex {
                pos: [chunk_v[0], chunk_v[1], chunk_v[2]],
                uv: [chunk_v[3], chunk_v[4]],
                light: chunk_v[5],
            });
        }
        log::info!("M1: built {} verts, {} idx", verts.len(), raw_idxs.len());

        let vbuf = create_vertex_buffer(&gpu.device, &verts);
        let ibuf = create_index_buffer(&gpu.device, &raw_idxs);
        let index_count = raw_idxs.len() as u32;

        let camera = Camera::default_orbit(1.0);

        Ok(Self {
            gpu, pipe, vbuf, ibuf, index_count,
            camera,
            last_frame: Instant::now(),
            frames: 0,
            fps_last: Instant::now(),
        })
    }

    fn resize(&mut self, w: u32, h: u32) {
        self.gpu.resize(w, h);
        self.camera.aspect = w as f32 / h as f32;
    }

    fn render(&mut self) {
        // Compute view/proj with current aspect.
        let (view, proj) = self.camera.build_view_proj();
        self.pipe.upload_uniforms(&self.gpu.queue, view, proj);

        let frame = match self.gpu.surface.get_current_texture() {
            Ok(f) => f,
            Err(e) => {
                log::warn!("surface.get_current_texture: {e:?}");
                return;
            }
        };
        let view_tex = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut enc = self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame"),
        });

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
                depth_stencil_attachment: None,
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

        // FPS counter
        self.frames += 1;
        let now = Instant::now();
        if now.duration_since(self.fps_last).as_secs_f32() >= 1.0 {
            let fps = self.frames as f32 / now.duration_since(self.fps_last).as_secs_f32();
            log::info!("FPS: {fps:.1}");
            self.frames = 0;
            self.fps_last = now;
        }
    }
}

#[no_mangle]
fn android_main(app: AndroidApp) {
    // Logger - pipe Rust log messages to Android logcat
    #[cfg(target_os = "android")]
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("mc-rs"),
    );
    #[cfg(not(target_os = "android"))]
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();

    // We need to bridge from the android-activity sync event model to async GPU init.
    // Strategy: spawn the init on a separate thread, communicate via a small oneshot.
    // For M1 simplicity: poll events synchronously, then init once we have a window,
    // then enter the render loop on the same thread.

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
                            .spawn(move || {
                                pollster::block_on(App::init(ptr as *mut c_void))
                            })
                            .expect("spawn");
                        init_handle = Some(h);
                    }
                    MainEvent::WindowResized { .. } => {
                        redraw = true;
                    }
                    MainEvent::RedrawNeeded { .. } => {
                        redraw = true;
                    }
                    MainEvent::Destroy { .. } => running = false,
                    _ => {}
                }
            }
        });

        if let Some(h) = init_handle.take() {
            match h.join() {
                Ok(Ok(a)) => {
                    app_state = Some(a);
                    redraw = true;
                }
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
