//! The renderer the app draws with: wgpu (Vulkan, or wgpu's own GL fallback) or the OpenGL ES 3.2 renderer (`gles`). `App` owns one
//! `Backend`; the CPU side of a frame is shared and arrives as a `gpu::gfx::WorldDraw`, so only the passes are here.

use std::time::{Duration, Instant};

use core::ffi::c_void;

use crate::gles::renderer::Renderer as Gles;
use crate::gpu::context::Gpu;
use crate::gpu::gfx::{Drawn, Gfx, WorldDraw};
use crate::gpu::pipeline::{ChunkPipeline, DrawOrigins};
use crate::render::hud::{HudPipeline, HudVertex};
use crate::render::panorama::Panorama;
use crate::render::present::{Present, SCENE_FORMAT};
use crate::render::sky::{SkyFrame, SkyRenderer};
use crate::render::weather;
use crate::render::anim::Tile;

/// What the wgpu renderer owns.
pub struct Wg {
    pub gpu: Gpu,
    pub pipe: ChunkPipeline,
    hud: HudPipeline,
    pano: Panorama,
    sky: SkyRenderer,
    present: Present,
    rain_fx: weather::Weather,
}

impl Wg {
    pub fn new(gpu: Gpu) -> Self {
        let (w, h) = (gpu.config.width, gpu.config.height);
        let fmt = gpu.surface_format();
        let pipe = ChunkPipeline::new(&gpu.device);
        pipe.upload_atlas(&gpu.queue);
        let hud = HudPipeline::new(&gpu.device, &gpu.queue, fmt, w, h);
        let pano = Panorama::new(&gpu.device, &gpu.queue, fmt);
        let rain_fx = weather::Weather::new(&gpu.device, &gpu.queue, SCENE_FORMAT);
        let sky = SkyRenderer::new(&gpu.device, &gpu.queue, SCENE_FORMAT);
        let present = Present::new(&gpu.device, &gpu.queue, gpu.scene(), (w, h), fmt);
        Self { gpu, pipe, hud, pano, sky, present, rain_fx }
    }

    fn sync_scene(&mut self) {
        let c = &self.gpu.config;
        self.present.resize(&self.gpu.device, &self.gpu.queue, self.gpu.scene(), (c.width, c.height));
    }

    /// Opaque layers, the far terrain, entities and the outline (the old `App::draw_opaque`).
    fn draw_opaque(&self, rp: &mut wgpu::RenderPass<'_>, d: &WorldDraw) {
        rp.set_bind_group(0, &self.pipe.bind_group, &[]);
        rp.set_pipeline(&self.pipe.solid);
        d.chunks.draw_ranges(crate::world::chunks::Layer::Solid, rp, &self.pipe.origins);
        if let Some(l) = d.lod {
            rp.set_pipeline(&self.pipe.lod);
            l.draw(rp);
        }
        rp.set_pipeline(&self.pipe.cutout);
        d.chunks.draw_ranges(crate::world::chunks::Layer::Cutout, rp, &self.pipe.origins);
        if d.item_indices > 0 || d.outline_indices > 0 {
            rp.set_pipeline(&self.pipe.entity); // the float vertex, no chunk origin
        }
        if d.item_indices > 0 {
            rp.set_vertex_buffer(0, d.items.vbuf.wgpu().slice(..));
            rp.set_index_buffer(d.items.ibuf.wgpu().slice(..), wgpu::IndexFormat::Uint32);
            rp.draw_indexed(0..d.item_indices, 0, 0..1);
        }
        if d.outline_indices > 0 {
            rp.set_vertex_buffer(0, d.outline.vbuf.wgpu().slice(..));
            rp.set_index_buffer(d.outline.ibuf.wgpu().slice(..), wgpu::IndexFormat::Uint32);
            rp.draw_indexed(0..d.outline_indices, 0, 0..1);
        }
    }

    /// The scene pass (sky, terrain, far terrain, entities, water, rain; 4x MSAA resolved into `Present`'s 8-bit scene) and then its one
    /// full-screen pass (CAS + upscale + vignette + the HUD) onto the swap chain. Two render passes in all.
    fn draw_world(&mut self, d: &WorldDraw) -> Drawn {
        self.pipe.frame(&self.gpu.queue, d.view, d.proj, d.eye, d.sky_sub, d.fog, d.far, d.start, d.wr, d.atmos.density);
        let clear = wgpu::Color { r: d.fog[0] as f64, g: d.fog[1] as f64, b: d.fog[2] as f64, a: 1.0 };
        self.sky.update(&self.gpu.queue, d.camera, &SkyFrame { angle: d.angle, rain: d.rain, seconds: d.seconds });
        self.sky.set_legacy(&self.gpu.queue, d.zenith, d.horizon, d.angle, d.rain, &d.atmos, d.eye);
        // Rain and snow, drawn in the scene pass after the water; the depth test hides the streaks behind the terrain.
        if let Some((snow, rain, sun_y)) = d.precip {
            self.rain_fx.update(&self.gpu.queue, d.proj * d.view, d.eye, sun_y, snow, rain);
        }
        let t_acq = Instant::now();
        let frame = match self.gpu.surface.get_current_texture() {
            Ok(f) => f,
            Err(e) => {
                log::warn!("surface.get_current_texture: {e:?}");
                if matches!(e, wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) {
                    self.gpu.surface.configure(&self.gpu.device, &self.gpu.config);
                }
                return Drawn::Skipped;
            }
        };
        let acq = t_acq.elapsed(); // the swap-chain wait (vsync / GPU back-pressure) is not CPU work
        // Reconcile surface/config size. Native-activity doesn't always send MainEvent::WindowResized before the first frame, so the cached
        // config.width/height can be wrong (1x1 from from_android_window). We drop the stale one (wgpu invalidates it after configure), resize, return.
        let (actual_w, actual_h) = (frame.texture.width(), frame.texture.height());
        if actual_w != self.gpu.config.width || actual_h != self.gpu.config.height {
            log::info!("M3 render: frame is {actual_w}x{actual_h}, reconfiguring (was {}x{})", self.gpu.config.width, self.gpu.config.height);
            return Drawn::Resize(actual_w, actual_h);
        }
        let view_tex = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let hud_vertex_count = d.hud.len();
        if hud_vertex_count > 0 {
            self.gpu.queue.write_buffer(&self.hud.vbuf, 0, bytemuck::cast_slice(d.hud));
        }
        let mut enc = self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(self.present.scene_attachment(clear))], // MSAA: resolved into the scene in tile memory, never stored
                // The depth is only this pass's: `Discard` keeps it in the tile memory and never writes it out.
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.gpu.depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.draw_opaque(&mut rp, d);
            if !d.nether {
                self.sky.draw(&mut rp); // depth-tested LessEqual: only the pixels the opaque draws left at 1
                self.sky.draw_extras(&mut rp); // sunrise fan, sun, moon, stars
            }
            // Water last: blended over everything, no depth write.
            rp.set_pipeline(&self.pipe.lod_water);
            rp.set_bind_group(0, &self.pipe.bind_group, &[]); // the sky pass set its own
            if let Some(l) = d.lod {
                l.draw_water(&mut rp); // the far water first (it is farther than the real water, which only reaches `WATER_RANGE`)
            }
            rp.set_pipeline(&self.pipe.water);
            d.chunks.draw_ranges(crate::world::chunks::Layer::Water, &mut rp, &self.pipe.origins);
            if d.precip.is_some() {
                self.rain_fx.draw(&mut rp);
            }
        }
        // CAS + upscale + vignette in one pass; the HUD goes over the finished frame in it (neither sharpened nor vignetted).
        self.present.finish(&mut enc, &view_tex, |rp| {
            if hud_vertex_count > 0 {
                rp.set_pipeline(&self.hud.pipeline);
                rp.set_bind_group(0, &self.hud.bind_group, &[]);
                rp.set_vertex_buffer(0, self.hud.vbuf.slice(..));
                rp.draw(0..hud_vertex_count as u32, 0..1);
            }
        });
        self.gpu.queue.submit(std::iter::once(enc.finish()));
        frame.present();
        Drawn::Done(acq)
    }

    /// A menu frame: the HUD pass over black, the panorama under it on the title screen.
    fn draw_menu(&mut self, pano: bool, verts: &[HudVertex]) -> Drawn {
        let (w, h) = (self.gpu.config.width as f32, self.gpu.config.height as f32);
        let frame = match self.gpu.surface.get_current_texture() {
            Ok(f) => f,
            Err(e) => {
                log::warn!("surface.get_current_texture: {e:?}");
                if matches!(e, wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) {
                    self.gpu.surface.configure(&self.gpu.device, &self.gpu.config);
                }
                return Drawn::Skipped;
            }
        };
        let (fw, fh) = (frame.texture.width(), frame.texture.height());
        if fw != self.gpu.config.width || fh != self.gpu.config.height {
            return Drawn::Resize(fw, fh);
        }
        let view_tex = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("menu") });
        self.gpu.queue.write_buffer(&self.hud.vbuf, 0, bytemuck::cast_slice(verts));
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("menu_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view_tex, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if pano {
                self.pano.draw(&self.gpu.queue, &mut rp, w, h);
            }
            rp.set_pipeline(&self.hud.pipeline);
            rp.set_bind_group(0, &self.hud.bind_group, &[]);
            rp.set_vertex_buffer(0, self.hud.vbuf.slice(..));
            rp.draw(0..verts.len() as u32, 0..1);
        }
        self.gpu.queue.submit(std::iter::once(enc.finish()));
        frame.present();
        Drawn::Done(Duration::ZERO)
    }
}

pub enum Backend {
    Wgpu(Box<Wg>),
    Gles(Box<Gles>),
}

impl Backend {
    /// The OpenGL ES renderer on `window`, if `use_gles`; else (or if it cannot start) wgpu, Vulkan first.
    pub async fn new(use_gles: bool, window: *mut c_void, w: u32, h: u32) -> Result<Self, String> {
        if use_gles {
            match Gles::new(window) {
                Ok(g) => return Ok(Backend::Gles(Box::new(g))),
                Err(e) => log::warn!("OpenGL ES renderer failed ({e}), starting wgpu"),
            }
        }
        Ok(Backend::Wgpu(Box::new(Wg::new(Gpu::from_android_window(window, w, h).await?))))
    }

    pub fn gfx(&self) -> Gfx<'_> {
        match self {
            Backend::Wgpu(w) => Gfx::Wgpu(&w.gpu.device, &w.gpu.queue),
            Backend::Gles(g) => g.gfx(),
        }
    }

    /// The wgpu chunk origins (the GL renderer sets the origin as a uniform).
    pub fn origins(&self) -> Option<&DrawOrigins> {
        match self {
            Backend::Wgpu(w) => Some(&w.pipe.origins),
            Backend::Gles(_) => None,
        }
    }

    pub fn size(&self) -> (u32, u32) {
        match self {
            Backend::Wgpu(w) => (w.gpu.config.width, w.gpu.config.height),
            Backend::Gles(g) => g.surface(),
        }
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        match self {
            Backend::Wgpu(g) => {
                g.gpu.resize(w, h);
                g.sync_scene();
                g.hud.resize(&g.gpu.queue, w, h);
            }
            Backend::Gles(g) => g.resize(w, h),
        }
    }

    /// The scene targets at the current dynamic-resolution scale.
    pub fn rescale(&mut self) {
        match self {
            Backend::Wgpu(g) => {
                g.gpu.rescale();
                g.sync_scene();
            }
            Backend::Gles(g) => g.rescale(),
        }
    }

    /// Animated water / lava / fire tiles of the block atlas.
    pub fn animate(&self, tiles: &[(u8, Tile)]) {
        match self {
            Backend::Wgpu(w) => w.pipe.animate(&w.gpu.queue, tiles),
            Backend::Gles(g) => g.animate(tiles, false),
        }
    }

    /// The compass and clock: the atlas copy and the HUD sheet's.
    pub fn animate_items(&self, tiles: &[(u8, Tile)]) {
        match self {
            Backend::Wgpu(w) => {
                w.pipe.animate_items(&w.gpu.queue, tiles);
                w.hud.animate_items(&w.gpu.queue, tiles);
            }
            Backend::Gles(g) => {
                g.animate(tiles, true);
                g.animate_hud(tiles);
            }
        }
    }

    pub fn draw_world(&mut self, d: &WorldDraw) -> Drawn {
        match self {
            Backend::Wgpu(w) => w.draw_world(d),
            Backend::Gles(g) => g.draw_world(d),
        }
    }

    pub fn draw_menu(&mut self, pano: bool, verts: &[HudVertex]) -> Drawn {
        match self {
            Backend::Wgpu(w) => w.draw_menu(pano, verts),
            Backend::Gles(g) => g.draw_menu(pano, verts),
        }
    }

    /// Lets the init thread's GL context go so the render thread can take it (a no-op for wgpu).
    pub fn release(&self) {
        if let Backend::Gles(g) = self {
            g.gl.release();
        }
    }

    pub fn gpu_info(&self) -> String {
        match self {
            Backend::Wgpu(w) => format!("{:?}", w.gpu.adapter.get_info().backend),
            Backend::Gles(g) => format!("OpenGL ES {}.{} ({})", g.gl.version.0, g.gl.version.1, g.gl.name),
        }
    }

}
