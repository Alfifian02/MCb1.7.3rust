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
use android_activity::input::InputEvent as IEv;
use core::ffi::c_void;
use std::time::{Duration, Instant};

use crate::gpu::context::Gpu;
use crate::gpu::pipeline::ChunkPipeline;
use crate::render::atlas;
use crate::render::items::ItemMesh;
use crate::render::camera::FirstPersonCamera;
use crate::render::outline::Outline;
use crate::world::pick::{self, Hit};
use crate::world::dig::{self, Dig};
use crate::world::craft::{self, Screen};
use crate::world::items::{self, Drops, Inventory, ItemStack};
use crate::world::sky;
use crate::world::chunk::{cross_shape, is_plant};
use crate::world::chunks::{chunk_coord, ChunkManager};
use crate::world::physics::{self, Player};

/// Render distance in chunks (a circle of this radius is meshed and drawn, two more are generated).
const RENDER_DIST: i32 = 4;

const LOOK_SENS: f32 = 0.004;
/// World units per second when the d-pad is fully pressed.
const MOVE_SPEED: f32 = 4.3;
/// Eye offset from `player.pos`, which is the CENTRE of the 1.8-tall box. Steve's eyes are
/// 1.62 above his feet (EntityPlayer.yOffset), i.e. 1.62 - 0.9 above the centre.
const EYE_HEIGHT: f32 = 1.62 - physics::HALF.y;

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
    // M5: block under the crosshair (within reach) and its outline buffers.
    target: Option<Hit>,
    outline: Outline,
    /// World time in ticks (20 per second); a new world starts at 0, sunrise.
    world_ticks: f64,
    /// Survival digging state and the leftover time towards the next 20 Hz dig tick.
    dig: Dig,
    dig_acc: f32,
    /// Hotbar stacks (starts empty), dropped items on the ground and their draw buffers.
    inv: Inventory,
    drops: Drops,
    item_mesh: ItemMesh,
    /// Open container screen (the 2x2 inventory or a workbench's 3x3) and its "place one" click toggle.
    screen: Option<Screen>,
    one_mode: bool,
}

impl App {
    async fn init(native_ptr: *mut c_void, width: u32, height: u32) -> Result<Self, String> {
        let gpu = Gpu::from_android_window(native_ptr, width, height).await?;
        let surface_format = gpu.surface_format();
        let pipe = ChunkPipeline::new(&gpu.device, surface_format);
        pipe.upload_atlas(&gpu.queue);

        // The spawn search needs real terrain before the first frame, so the chunks around the
        // origin are generated and populated here (init runs on its own thread, not the render loop);
        // everything further out streams in on the worker threads. Raw -1..=2 populates -1..=1,
        // which finishes chunks 0..=1, the area searched.
        const SEED: i64 = 0xCAFEBABE;
        let mut chunks = ChunkManager::new(SEED, RENDER_DIST);
        chunks.preload(-1, 2);
        // Spawn on dry ground closest to the centre of chunk (0, 0): not under water (a fixed spawn
        // at y=60 put the camera inside the sea), and not on top of a tree.
        let top_of = |x: i32, z: i32| {
            (0..128).rev().find(|&y| !matches!(chunks.block(x, y, z), Some(0 | 8 | 9 | 17 | 18)) && !chunks.block(x, y, z).is_some_and(is_plant)).unwrap_or(-1)
        };
        let mut best: Option<(i32, i32, i32, i32)> = None; // x, z, top, dist^2
        let mut highest = (8, 8, i32::MIN);
        for z in 0..32 {
            for x in 0..32 {
                let t = top_of(x, z);
                if t > highest.2 { highest = (x, z, t); }
                if t >= 64 {
                    let d2 = (x - 8).pow(2) + (z - 8).pow(2);
                    if best.map_or(true, |b| d2 < b.3) { best = Some((x, z, t, d2)); }
                }
            }
        }
        let (spawn_x, spawn_z, top) = match best {
            Some((x, z, t, _)) => (x, z, t),
            None => highest,
        };
        let spawn_feet_y = (top as f32) + 1.0 + 0.9;
        log::info!("chunks: render distance {}, spawn at ({}, {}, {}), top block y={}", RENDER_DIST, spawn_x, spawn_feet_y, spawn_z, top);

        // Light and mesh the area around the spawn now: init runs off the render thread, so the first
        // frame already shows terrain instead of filling in over the next few seconds.
        let warm = Instant::now();
        while chunks.meshed() < 24 && warm.elapsed() < Duration::from_secs(8) {
            chunks.update(&gpu.device, spawn_x >> 4, spawn_z >> 4);
            std::thread::sleep(Duration::from_millis(1));
        }
        log::info!("chunks: {} meshed after {:?}", chunks.meshed(), warm.elapsed());

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
        let outline = Outline::new(&gpu.device);
        let item_mesh = ItemMesh::new(&gpu.device);
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as i64);

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
            target: None,
            outline,
            world_ticks: 0.0,
            dig: Dig::default(),
            dig_acc: 0.0,
            inv: Inventory::default(),
            drops: Drops::new(seed),
            item_mesh,
            screen: None,
            one_mode: false,
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
        // Day/night: time only runs while playing; a new sky-light level restarts the meshes.
        self.world_ticks += dt as f64 * 20.0;
        let sub = sky::skylight_subtracted(sky::celestial_angle(self.world_ticks as u64, 1.0));
        self.chunks.set_sky_sub(sub);
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
        // Plants are drawn but not solid yet (collision shapes come with M14), so physics reads them as air.
        let get = |x: i32, y: i32, z: i32| self.chunks.block(x, y, z).map(|b| if is_plant(b) { 0 } else { b });
        physics::step(&mut self.player, dt, &get);
        self.camera.pos = self.player.pos + glam::Vec3::new(0.0, EYE_HEIGHT, 0.0);
        // Dropped items fall, settle and get picked up (before the target check below, which can return early).
        self.drops.tick(dt, &|x: i32, y: i32, z: i32| self.chunks.block(x, y, z), self.player.pos, &mut self.inv);

        // M5: pick the block under the crosshair, dig it while the dig input is held, place on a tap.
        self.touch.tick(dt);
        let place = self.touch.take_place();
        // The inventory button, and taps on an open screen (its presses are not look / move / place).
        if self.touch.take_inventory() {
            if self.screen.is_some() {
                self.close_screen();
            } else {
                self.open_screen(2);
            }
        }
        for (x, y) in self.touch.take_taps() {
            self.on_screen_tap(x, y);
        }
        let eye = self.camera.pos.as_dvec3();
        let end = eye + self.camera.forward().as_dvec3() * pick::REACH;
        // Liquids are not pickable (vanilla's rayTraceBlocks skips them), nor is the snow layer, which is not drawn yet.
        // ponytail: a plant is picked as a whole cell, not by its (smaller) bounds.
        let solid = |x: i32, y: i32, z: i32| matches!(self.chunks.block_loaded(x, y, z), Some(b) if b != 0 && (!is_plant(b) || cross_shape(b).is_some()) && !(8..=11).contains(&b));
        self.target = pick::ray_trace(&solid, eye, end);

        // Digging runs on the 20 Hz game tick of the Java (hardness-based time, see world::dig).
        // A broken block drops its items (world::items) only if the held item may harvest it (stone needs a pickaxe),
        // and wears the tool by one use.
        match self.target {
            Some(hit) if self.touch.digging() => {
                self.dig_acc += dt;
                while self.dig_acc >= dig::TICK {
                    self.dig_acc -= dig::TICK;
                    let (x, y, z) = hit.pos;
                    let in_water = matches!(self.chunks.block_loaded(eye.x.floor() as i32, eye.y.floor() as i32, eye.z.floor() as i32), Some(8 | 9));
                    if let Some(id) = self.chunks.block_loaded(x, y, z) {
                        let slot = self.touch.hotbar_slot;
                        let held = self.inv.slots[slot];
                        if self.dig.tick(hit.pos, id, held, self.player.on_ground, in_water) {
                            self.chunks.set_block(x, y, z, 0);
                            // PlayerControllerSP.sendBlockRemoved: `canHarvestBlock` is read before the tool wears, so
                            // a tool that breaks on this block still harvests it; no suitable tool, no drop.
                            let harvest = dig::can_harvest(id, held);
                            self.inv.damage(slot, 1);
                            if harvest {
                                self.drops.spawn_block(id, hit.pos);
                            }
                        }
                    }
                }
            }
            _ => {
                self.dig.reset();
                self.dig_acc = 0.0;
            }
        }
        let Some(hit) = self.target else { return };
        if place {
            // Using a workbench opens its 3x3 grid instead of placing a block against it.
            if self.chunks.block_loaded(hit.pos.0, hit.pos.1, hit.pos.2) == Some(58) {
                self.open_screen(3);
                return;
            }
            let (x, y, z) = pick::place_pos(&hit);
            // Items below 256 are blocks; anything else cannot be placed. ItemBlock refuses a solid block at y = 127
            // (ponytail: plants are refused there too).
            let slot = self.touch.hotbar_slot;
            if let Some(s) = self.inv.slots[slot].filter(|s| s.id < 256) {
                if y < 127 && self.chunks.block_loaded(x, y, z).is_some_and(pick::replaceable) && !pick::overlaps_player((x, y, z), self.player.pos) && self.chunks.set_block(x, y, z, s.id as u8) {
                    self.inv.consume(slot);
                }
            }
        }
    }

    /// Open the 2x2 inventory (`gw` 2) or a workbench's 3x3 screen (`gw` 3).
    fn open_screen(&mut self, gw: usize) {
        self.screen = Some(Screen::new(gw));
        self.one_mode = false;
        self.touch.set_screen(true);
    }

    /// `onCraftGuiClosed`: the cursor stack and whatever is left in the grid are thrown out in front of the player.
    fn close_screen(&mut self) {
        if let Some(mut s) = self.screen.take() {
            for st in s.close() {
                self.drops.throw(st, self.camera.pos, self.camera.forward());
            }
        }
        self.touch.set_screen(false);
    }

    /// A tap on the open screen: the "place one" toggle, a slot, or outside the panel (throws the cursor stack).
    fn on_screen_tap(&mut self, x: f32, y: f32) {
        let (w, h) = (self.gpu.config.width as f32, self.gpu.config.height as f32);
        let Some(s) = self.screen.as_mut() else { return };
        if craft::on_mode_button(w, h, x, y) {
            self.one_mode = !self.one_mode;
        } else if let Some(id) = craft::slot_at(s.gw, w, h, x, y) {
            s.click(id, self.one_mode, &mut self.inv);
        } else if !craft::in_panel(w, h, x, y) {
            // Click outside the window (slot -999): throw the cursor stack, or one item of it.
            if let Some(c) = s.cursor {
                let n = if self.one_mode { 1 } else { c.count };
                s.cursor = (c.count > n).then(|| ItemStack { count: c.count - n, ..c });
                self.drops.throw(ItemStack { count: n, ..c }, self.camera.pos, self.camera.forward());
            }
        }
    }

    fn render(&mut self) {
        self.step_frame();
        let (pcx, pcz) = (chunk_coord(self.player.pos.x), chunk_coord(self.player.pos.z));
        self.chunks.update(&self.gpu.device, pcx, pcz);
        let (view, proj) = self.camera.build_view_proj();
        self.pipe.upload_uniforms(&self.gpu.queue, view, proj);
        let outline_indices = self.target.map_or(0, |h| self.outline.update(&self.gpu.queue, h.pos));
        let item_indices = self.item_mesh.update(&self.gpu.queue, &self.drops);

        // Sky colour from the sun angle and the climate under the player.
        let (px, pz) = (self.camera.pos.x.floor() as i32, self.camera.pos.z.floor() as i32);
        let temp = self.chunks.temperature_at(px, pz) as f32;
        let angle = sky::celestial_angle(self.world_ticks as u64, self.world_ticks.fract() as f32);
        let [r, g, b] = sky::sky_color(angle, temp);
        let clear = wgpu::Color { r: r as f64, g: g as f64, b: b as f64, a: 1.0 };
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
            if item_indices > 0 {
                rp.set_vertex_buffer(0, self.item_mesh.vbuf.slice(..));
                rp.set_index_buffer(self.item_mesh.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(0..item_indices, 0, 0..1);
            }
            if outline_indices > 0 {
                rp.set_vertex_buffer(0, self.outline.vbuf.slice(..));
                rp.set_index_buffer(self.outline.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(0..outline_indices, 0, 0..1);
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
        // The in-world controls are hidden while a container screen is open.
        let play = self.screen.is_none();

        // Crosshair: the aim point of the pick ray (first, so the quad cap never drops it).
        if play {
            let (w, h) = (self.gpu.config.width as f32, self.gpu.config.height as f32);
            let (arm, th) = (0.025 * w.min(h), (0.004 * w.min(h)).max(2.0));
            HudPipeline::push_quad(v, w * 0.5 - arm, h * 0.5 - th * 0.5, arm * 2.0, th, [1.0, 1.0, 1.0, 0.85]);
            HudPipeline::push_quad(v, w * 0.5 - th * 0.5, h * 0.5 - arm, th, arm * 2.0, [1.0, 1.0, 1.0, 0.85]);
        }

        // Dig progress under the crosshair.
        if play && self.dig.damage > 0.0 {
            let (w, h) = (self.gpu.config.width as f32, self.gpu.config.height as f32);
            let (bw, bh) = (0.12 * w.min(h), (0.012 * w.min(h)).max(3.0));
            let (bx, by) = (w * 0.5 - bw * 0.5, h * 0.5 + 0.05 * w.min(h));
            HudPipeline::push_quad(v, bx, by, bw, bh, [0.0, 0.0, 0.0, 0.6]);
            HudPipeline::push_quad(v, bx, by, bw * self.dig.damage.min(1.0), bh, [1.0, 1.0, 1.0, 0.9]);
        }

        // Container screen: the vanilla GUI (176 x 166 units) over a dimmed frame, slots with their stacks.
        if let Some(s) = &self.screen {
            let (w, h) = (self.gpu.config.width as f32, self.gpu.config.height as f32);
            let (ox, oy, k) = craft::panel(w, h);
            HudPipeline::push_quad(v, 0.0, 0.0, w, h, [0.0, 0.0, 0.0, 0.55]);
            HudPipeline::push_outlined_quad(v, ox, oy, craft::PANEL.0 * k, craft::PANEL.1 * k, [0.78, 0.78, 0.78, 0.97], [0.15, 0.15, 0.15, 1.0], (2.0 * k).max(2.0));
            let slot = |v: &mut Vec<HudVertex>, c: (f32, f32, f32), st: Option<ItemStack>, edge: [f32; 4]| {
                let inner = (c.0 + k, c.1 + k, c.2 - 2.0 * k, c.2 - 2.0 * k);
                HudPipeline::push_quad(v, c.0, c.1, c.2, c.2, edge);
                HudPipeline::push_quad(v, inner.0, inner.1, inner.2, inner.3, [0.55, 0.55, 0.55, 1.0]);
                if let Some(st) = st {
                    push_stack(v, inner, st);
                }
            };
            for (id, ux, uy) in craft::layout(s.gw) {
                slot(v, craft::cell(w, h, (ux, uy)), s.get(&self.inv, id), [0.3, 0.3, 0.3, 1.0]);
            }
            // The stack on the cursor (yellow edge), and the "place one" toggle under it: blue = whole stacks,
            // orange with a 1 = one at a time (what the right mouse button does on a desktop).
            slot(v, craft::cell(w, h, craft::HELD), s.cursor, [0.95, 0.8, 0.2, 1.0]);
            let (mx, my, ms) = craft::cell(w, h, craft::MODE);
            let fill = if self.one_mode { [0.85, 0.5, 0.1, 1.0] } else { [0.2, 0.4, 0.7, 1.0] };
            HudPipeline::push_outlined_quad(v, mx, my, ms, ms, fill, [0.15, 0.15, 0.15, 1.0], k.max(2.0));
            if self.one_mode {
                HudPipeline::push_number(v, mx + ms * 0.32, my + ms * 0.2, ms * 0.6, 1, [1.0; 4]);
            }
        }

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

        // Inventory button, left of pause: a 2x2 grid icon (the crafting grid of the screen it opens).
        let (ix, iy, iw, ih) = layout.inventory;
        let inv_fill = if pressed(PointerRole::Inventory) || !play { [0.30, 0.55, 0.85, 0.95] } else { [0.20, 0.40, 0.70, 0.75] };
        HudPipeline::push_outlined_quad(v, ix, iy, iw, ih, inv_fill, [0.95, 0.95, 0.95, 0.9], 3.0);
        let (q, g) = (iw * 0.22, iw * 0.08);
        let (qx, qy) = (ix + (iw - q * 2.0 - g) * 0.5, iy + (ih - q * 2.0 - g) * 0.5);
        for (dx, dy) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
            HudPipeline::push_quad(v, qx + dx * (q + g), qy + dy * (q + g), q, q, [1.0; 4]);
        }

        // Hotbar: 9 cells, selected slot highlighted.
        for (i, &(hx, hy, hw, hh)) in layout.hotbar.iter().enumerate().filter(|_| play) {
            let border = if i == self.touch.hotbar_slot { [1.0, 1.0, 1.0, 1.0] } else { [0.85, 0.85, 0.85, 0.85] };
            HudPipeline::push_outlined_quad(v, hx, hy, hw, hh, [0.10, 0.10, 0.10, 0.55], border, 3.0);
            if let Some(s) = self.inv.slots[i] {
                push_stack(v, (hx, hy, hw, hh), s);
            }
        }

        if play {
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

/// One item stack drawn into the cell `(x, y, w, h)`: the flat colour of its tile (a tool is a stick-coloured handle
/// with a head in the material colour, shaped per kind, so pickaxe, axe and shovel can be told apart), a wear bar
/// under a damaged tool, and the count bottom right with a shadow like `RenderItem.renderItemOverlayIntoGUI`
/// (shown above 1 only). Real item sprites are M14.
// UNVERIFIED: the head shapes and the wear-bar colours are stand-ins for the item sprites and `renderItemOverlay`.
fn push_stack(v: &mut Vec<HudVertex>, (hx, hy, hw, hh): (f32, f32, f32, f32), s: ItemStack) {
    let pad = (hw.min(hh) * 0.18).max(2.0);
    let rgb = |tile: u8| {
        let c = atlas::block_color(tile);
        [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, 1.0]
    };
    let (ix, iy, iw, ih) = (hx + pad, hy + pad, hw - pad * 2.0, hh - pad * 2.0);
    match craft::tool(s.id) {
        None => HudPipeline::push_quad(v, ix, iy, iw, ih, rgb(items::tile(s.id))),
        Some((kind, _)) => {
            HudPipeline::push_quad(v, ix + iw * 0.42, iy, iw * 0.16, ih, rgb(17));
            let head = rgb(items::tile(s.id));
            match kind {
                craft::Kind::Pickaxe => HudPipeline::push_quad(v, ix, iy, iw, ih * 0.22, head),
                craft::Kind::Axe => HudPipeline::push_quad(v, ix, iy, iw * 0.58, ih * 0.38, head),
                craft::Kind::Shovel => HudPipeline::push_quad(v, ix + iw * 0.28, iy, iw * 0.44, ih * 0.32, head),
            }
        }
    }
    if let Some(max) = craft::max_damage(s.id).filter(|_| s.damage > 0) {
        let left = (1.0 - s.damage as f32 / max as f32).clamp(0.0, 1.0);
        let (by, bh) = (hy + hh - pad * 0.5 - (hh * 0.06).max(2.0), (hh * 0.06).max(2.0));
        HudPipeline::push_quad(v, ix, by, iw, bh, [0.0, 0.0, 0.0, 0.8]);
        HudPipeline::push_quad(v, ix, by, iw * left, bh, [1.0 - left, left, 0.0, 1.0]);
    }
    if s.count > 1 {
        let dh = hh * 0.3;
        let digits = if s.count > 9 { 2.0 } else { 1.0 };
        let (nx, ny) = (hx + hw - pad * 0.5 - (digits * 0.71 - 0.21) * dh, hy + hh - pad * 0.5 - dh);
        HudPipeline::push_number(v, nx + 1.5, ny + 1.5, dh, s.count as u32, [0.0, 0.0, 0.0, 0.9]);
        HudPipeline::push_number(v, nx, ny, dh, s.count as u32, [1.0; 4]);
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
