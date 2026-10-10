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

use crate::input::touch_ui::{PointerRole, ScreenEv, TouchUi};
use crate::render::hud::{HudPipeline, HudVertex};

use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};
use android_activity::input::InputEvent as IEv;
use core::ffi::c_void;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::gpu::context::Gpu;
use crate::gpu::pipeline::ChunkPipeline;
use crate::render::atlas;
use crate::render::items::ItemMesh;
use crate::render::camera::{FirstPersonCamera, Frustum, ShadowCache};
use crate::render::outline::Outline;
use crate::render::sky::{SkyFrame, SkyRenderer};
use crate::render::vl::{self, LightShafts};
use crate::world::pick::{self, Hit};
use crate::world::dig::{self, Dig};
use crate::world::chest::{self, Chest};
use crate::world::craft::{self, Furnace, Screen};
use crate::world::items::{self, Drops, Inventory, ItemStack};
use crate::world::mobs::{self, Ctx, Ev, Mobs};
use crate::world::ticks::Ticks;
use crate::world::save::{self, Level};
use crate::world::sky;
use crate::world::chunk::{is_plant, outline_bounds, physics_id, pick_bounds};
use crate::world::chunks::{chunk_coord, ChunkManager, Shape};
use crate::world::physics::{self, Player};
use crate::world::vitals::{self, Env, Vitals};

/// Render distance in chunks (a circle of this radius is meshed and drawn, two more are generated).
const RENDER_DIST: i32 = 4;
/// The world seed (its save folder is `world-<seed>`).
const SEED: i64 = 0xCAFEBABE;
/// Vertical render distance in chunks (16 blocks): sections further above or below the eye are not drawn.
const RENDER_VERT: i32 = 3;
/// `Sphere`: what is drawn is within `RENDER_DIST` blocks-of-16 in 3D (matches the fog, which is by 3D distance);
/// `Cylinder`: a circle of that radius at any height within `RENDER_VERT`.
const RENDER_SHAPE: Shape = Shape::Sphere;

/// Seconds of play between autosaves (the level file and a few chunks; pause and exit save everything).
const AUTOSAVE_SECS: f32 = 5.0;
/// Chunks written per autosave, so the first ones after generating a lot of terrain do not hitch a frame.
const AUTOSAVE_CHUNKS: usize = 24;

const LOOK_SENS: f32 = 0.004;
/// World units per second when the d-pad is fully pressed.
const MOVE_SPEED: f32 = physics::WALK_SPEED;
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
    /// Sun, moon and stars drawn behind the terrain, and the rain/thunder timers (not saved: a loaded world starts clear).
    /// `weather_ticks` counts the whole ticks the weather has run, so it catches up to `world_ticks` one tick at a time.
    sky: SkyRenderer,
    /// Volumetric light (`render::vl`), drawn after the terrain.
    shafts: LightShafts,
    /// Where and along what the shadow map was last drawn (it is reused between redraws).
    shadow: ShadowCache,
    /// The entity shadow map holds something (or was never written: a fresh texture reads as depth 0 = all shadow) and needs a clear.
    ent_shadow_dirty: bool,
    weather: sky::Weather,
    weather_ticks: u64,
    /// Hotbar slot last seen and seconds its item name still shows (`GuiIngame` shows it ~2 s after a switch).
    tip: (usize, f32),
    /// Survival digging state and the leftover time towards the next 20 Hz dig tick.
    dig: Dig,
    dig_acc: f32,
    /// Hotbar stacks (starts empty), dropped items on the ground and their draw buffers.
    inv: Inventory,
    drops: Drops,
    mobs: Mobs,
    /// Block updates (scheduled + random ticks, notifications, falling blocks), stepped on the 20 Hz game tick.
    ticks: Ticks,
    item_mesh: ItemMesh,
    /// Open container screen (the 2x2 inventory or a workbench's 3x3) and its "place one" click toggle.
    screen: Option<Screen>,
    one_mode: bool,
    /// Furnace tile entities by block position (created on first use), and the time left over from the last 20 Hz tick.
    furnaces: HashMap<(i32, i32, i32), Furnace>,
    furn_acc: f32,
    /// Chest tile entities by block position (created on first open, or by a dungeon's populate).
    chests: HashMap<chest::Pos, Chest>,
    /// The finger gesture on the open screen, to tell a tap from a spread.
    gesture: Option<Gesture>,
    /// Health, air and fire; and where a dead player comes back (the first spawn: no beds yet).
    vitals: Vitals,
    spawn: glam::Vec3,
    /// Where this world is saved (M7) and the play time since the last autosave.
    dir: PathBuf,
    save_acc: f32,
}

/// A press on an open screen: where it began, whether it became a spread (a drag over slots with a stack on the
/// cursor, one item into each slot it enters) and which slots it already filled.
struct Gesture {
    start: Option<craft::SlotId>,
    spread: bool,
    visited: Vec<craft::SlotId>,
}

impl App {
    async fn init(native_ptr: *mut c_void, width: u32, height: u32, dir: PathBuf) -> Result<Self, String> {
        let gpu = Gpu::from_android_window(native_ptr, width, height).await?;
        let surface_format = gpu.surface_format();
        let pipe = ChunkPipeline::new(&gpu.device, surface_format);
        pipe.upload_atlas(&gpu.queue);

        // The terrain around the player must be real before the first frame, so those chunks are loaded (or
        // generated and populated) here (init runs on its own thread, not the render loop); everything further out
        // streams in on the worker threads. A saved world resumes where the player stood, a new one at the origin.
        // One folder per seed: chunks saved for another seed would not fit this terrain.
        let dir = dir.join(format!("world-{SEED:x}"));
        let level = std::fs::read(dir.join("level")).ok().and_then(|b| Level::decode(&b));
        let mut chunks = ChunkManager::new(SEED, RENDER_DIST).with_dir(dir.clone());
        let (pcx, pcz) = level.as_ref().map_or((0, 0), |l| (chunk_coord(l.pos.x), chunk_coord(l.pos.z)));
        chunks.preload(pcx, pcz);
        let spawn = level.as_ref().map_or_else(|| find_spawn(&chunks), |l| l.spawn);
        let start = level.as_ref().map_or(spawn, |l| l.pos);
        log::info!("save: {} in {}", if level.is_some() { "resuming" } else { "new world" }, dir.display());

        // Light and mesh the area around the player now: init runs off the render thread, so the first
        // frame already shows terrain instead of filling in over the next few seconds.
        let warm = Instant::now();
        while chunks.meshed() < 24 && warm.elapsed() < Duration::from_secs(8) {
            chunks.update(&gpu.device, chunk_coord(start.x), chunk_coord(start.z));
            std::thread::sleep(Duration::from_millis(1));
        }
        log::info!("chunks: {} meshed after {:?}", chunks.meshed(), warm.elapsed());

        let mut camera = FirstPersonCamera::spawn_at(start.x, start.y + EYE_HEIGHT, start.z);
        // spawn_at defaults to aspect 1.0 and resize() only runs when the size changes, so
        // without this the 3D view is squashed horizontally onto the real screen shape.
        camera.aspect = width as f32 / height as f32;
        let player = Player { pos: start, vel: glam::Vec3::ZERO, on_ground: false };

        // M12: HUD pipeline + touch state machine. Surface dimensions match
        // the window we just initialised against.
        let touch = TouchUi::new(width, height);
        let hud = HudPipeline::new(&gpu.device, &gpu.queue, surface_format, width, height);
        let outline = Outline::new(&gpu.device);
        let item_mesh = ItemMesh::new(&gpu.device);
        let sky_renderer = SkyRenderer::new(&gpu.device, &gpu.queue, surface_format);
        let shafts = LightShafts::new(&gpu.device, surface_format, &pipe.shadow_layout);
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as i64);

        let mut app = Self {
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
            sky: sky_renderer,
            shafts,
            shadow: ShadowCache::new(),
            ent_shadow_dirty: true,
            weather: sky::Weather::new(seed),
            weather_ticks: 0,
            tip: (0, 0.0),
            dig: Dig::default(),
            dig_acc: 0.0,
            inv: Inventory::default(),
            drops: Drops::new(seed),
            mobs: Mobs::new(seed),
            ticks: Ticks::new(seed),
            item_mesh,
            screen: None,
            one_mode: false,
            furnaces: HashMap::new(),
            chests: HashMap::new(),
            furn_acc: 0.0,
            gesture: None,
            vitals: Vitals::default(),
            spawn,
            dir,
            save_acc: 0.0,
        };
        if let Some(l) = level {
            app.restore(l);
        }
        Ok(app)
    }

    /// TEMPORARY (pause-menu reset button): delete the save folder and put everything back to a new world of `SEED`: terrain
    /// without edits, spawn point, hotbar and inventory, health, time, weather, mobs, drops, furnaces, block updates. An open
    /// screen's stacks are discarded, not dropped. ponytail: `preload` runs on the render thread (a second or two of freeze).
    fn reset_world(&mut self) {
        if self.screen.take().is_some() {
            self.touch.set_screen(false);
        }
        let _ = std::fs::remove_dir_all(&self.dir);
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as i64);
        self.chunks = ChunkManager::new(SEED, RENDER_DIST).with_dir(self.dir.clone()); // the old one's workers stop on drop
        self.chunks.preload(0, 0);
        self.spawn = find_spawn(&self.chunks);
        let s = self.spawn;
        self.player = Player { pos: s, vel: glam::Vec3::ZERO, on_ground: false };
        let aspect = self.camera.aspect;
        self.camera = FirstPersonCamera::spawn_at(s.x, s.y + EYE_HEIGHT, s.z);
        self.camera.aspect = aspect;
        (self.world_ticks, self.weather_ticks, self.weather) = (0.0, 0, sky::Weather::new(seed));
        (self.tip, self.dig, self.dig_acc, self.target, self.shadow) = ((0, 0.0), Dig::default(), 0.0, None, ShadowCache::new());
        (self.inv, self.drops, self.mobs, self.ticks) = (Inventory::default(), Drops::new(seed), Mobs::new(seed), Ticks::new(seed));
        (self.furnaces, self.furn_acc, self.vitals, self.save_acc) = (HashMap::new(), 0.0, Vitals::default(), 0.0);
        self.touch.hotbar_slot = 0;
        log::info!("reset: new world of seed {SEED:x}, spawn {s:?}");
    }

    /// Put a loaded `Level` back: time, look direction, hotbar slot, health, inventory, furnaces and dropped items
    /// (the position and spawn were used when the app was built).
    fn restore(&mut self, l: Level) {
        self.world_ticks = l.ticks;
        self.weather_ticks = l.ticks as u64;
        self.camera.yaw = l.yaw;
        self.camera.pitch = l.pitch;
        self.touch.hotbar_slot = l.slot as usize;
        self.vitals.health = l.vitals[0];
        self.vitals.air = l.vitals[1];
        self.vitals.fire = l.vitals[2];
        self.inv.slots = l.inv;
        self.furnaces = l.furnaces.into_iter().collect();
        self.chests = l.chests.into_iter().collect();
        self.drops.items = l.drops.into_iter().map(|(p, s, age)| items::ItemEntity::resting(p, s, age)).collect();
    }

    /// Write the world to disk: the level file, then up to `chunks` unsaved chunks (`usize::MAX` = all). An open
    /// screen is closed first, so the cursor stack and the grid become dropped items, which are saved.
    // ponytail: runs on the render thread; the level file is ~1 KB and the chunk writes are capped per autosave.
    fn save(&mut self, chunks: usize) {
        // `close_screen` also calls `touch.set_screen(false)`, which resets the touch state. Autosave runs while
        // playing (no screen open), so calling it unconditionally cut every held action (move, dig, look) every 5 s.
        if self.screen.is_some() {
            self.close_screen();
        }
        let level = Level {
            ticks: self.world_ticks,
            pos: self.player.pos,
            spawn: self.spawn,
            yaw: self.camera.yaw,
            pitch: self.camera.pitch,
            slot: self.touch.hotbar_slot as u8,
            vitals: [self.vitals.health, self.vitals.air, self.vitals.fire],
            inv: self.inv.slots,
            furnaces: self.furnaces.iter().map(|(&p, f)| (p, f.clone())).collect(),
            chests: self.chests.iter().map(|(&p, &c)| (p, c)).collect(),
            drops: self.drops.items.iter().map(|e| (e.pos, e.stack, e.age)).collect(),
        };
        if let Err(e) = save::write(&self.dir.join("level"), &level.encode()) {
            log::warn!("save: level file: {e}");
        }
        self.chunks.flush(chunks);
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
        // TEMPORARY pause-menu button: wipe the save and start the seed's world again.
        if self.touch.take_reset() {
            self.reset_world();
            return;
        }
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
        // The death screen's resume button was pressed: come back at the spawn point with full health.
        if self.vitals.dead() {
            self.vitals = Vitals::default();
            self.player.pos = self.spawn;
            self.player.vel = glam::Vec3::ZERO;
        }
        // Autosave while playing (not with a screen open: its cursor stack is not in the inventory yet).
        self.save_acc += dt;
        if self.save_acc >= AUTOSAVE_SECS && self.screen.is_none() {
            self.save_acc = 0.0;
            self.save(AUTOSAVE_CHUNKS);
        }
        let slot = self.touch.hotbar_slot;
        self.tip = if slot != self.tip.0 { (slot, 2.0) } else { (slot, (self.tip.1 - dt).max(0.0)) };
        // Day/night: time only runs while playing; a new sky-light level restarts the meshes.
        self.world_ticks += dt as f64 * 20.0;
        while self.weather_ticks < self.world_ticks as u64 {
            self.weather.tick();
            self.weather_ticks += 1;
        }
        let angle = sky::celestial_angle(self.world_ticks as u64, 1.0);
        let sub = sky::skylight_subtracted(angle, self.weather.rain(1.0), self.weather.thunder(1.0));
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
        // Plants are drawn but not solid yet (collision shapes come with M14), so physics reads them as air.
        let get = |x: i32, y: i32, z: i32| self.chunks.block(x, y, z).map(|b| if is_plant(b) { 0 } else { physics_id(b, self.chunks.meta(x, y, z)) });
        // The held jump button jumps (8.4 m/s, the b1.7.3 velocity) or swims up; fluids (ids 8..=11) are not solid.
        let y0 = self.player.pos.y;
        let (water, lava) = physics::step(&mut self.player, dt, self.touch.jumping(), &get);
        self.camera.pos = self.player.pos + glam::Vec3::new(0.0, EYE_HEIGHT, 0.0);
        let e = self.camera.pos;
        let eye_in_water = matches!(self.chunks.block_loaded(e.x.floor() as i32, e.y.floor() as i32, e.z.floor() as i32), Some(8 | 9));
        self.vitals.armor = craft::armor_value(&self.inv.slots[items::MAIN..]);
        self.vitals.update(dt, Env { dy: self.player.pos.y - y0, on_ground: self.player.on_ground, water, lava, eye_in_water, eye_y: e.y });
        // `InventoryPlayer.damageArmor`: every worn piece takes the damage that was dealt (before armor).
        let wear = std::mem::take(&mut self.vitals.wear) as u16;
        (items::MAIN..items::SLOTS).filter(|_| wear > 0).for_each(|i| self.inv.damage(i, wear));
        if self.vitals.dead() {
            self.die();
            return;
        }
        // Dropped items fall, settle and get picked up (before the target check below, which can return early).
        self.drops.tick(dt, &|x: i32, y: i32, z: i32| self.chunks.block(x, y, z).map(|b| physics_id(b, self.chunks.meta(x, y, z))), self.player.pos, &mut self.inv);
        let light = |x: i32, y: i32, z: i32| self.chunks.light(x, y, z);
        let ctx = Ctx { get: &get, light: &light, day: sub < 4, player: self.player.pos, eye: self.camera.pos };
        for e in self.mobs.update(dt, &ctx, &mut self.drops) {
            match e {
                Ev::Hurt(d) => { self.vitals.hurt(d); }
                Ev::Boom(p) => self.explode(p, 3.0),
                _ => {}
            }
        }
        // Furnaces burn on the 20 Hz tick, open or not, but only in loaded chunks (vanilla ticks loaded tile entities).
        // The block swaps between unlit 61 and lit 62 when the fire goes on or off.
        self.furn_acc += dt;
        while self.furn_acc >= dig::TICK {
            self.furn_acc -= dig::TICK;
            let centre = (chunk_coord(self.player.pos.x), chunk_coord(self.player.pos.z));
            self.ticks.step(&mut self.chunks, &mut self.drops, centre);
            let mut flips = Vec::new();
            for (&pos, f) in self.furnaces.iter_mut() {
                if matches!(self.chunks.block_loaded(pos.0, pos.1, pos.2), Some(61 | 62)) && f.tick() {
                    flips.push((pos, f.burn > 0));
                }
            }
            for ((x, y, z), lit) in flips {
                self.chunks.set_block(x, y, z, if lit { 62 } else { 61 });
            }
            // Chests: take over what a dungeon's populate put in them; one whose block is gone (dug, blown up, washed away)
            // spills its contents (`BlockChest.onBlockRemoval`; ponytail: seen on the next 20 Hz tick, not at the removal).
            for (p, c) in self.chunks.take_loot() {
                self.chests.entry(p).or_insert(c);
            }
            let gone: Vec<_> = self.chests.keys().copied().filter(|&(x, y, z)| self.chunks.block_loaded(x, y, z).is_some_and(|b| b != chest::ID)).collect();
            for p in gone {
                for st in self.chests.remove(&p).into_iter().flatten().flatten() {
                    self.drops.scatter(st, p);
                }
            }
        }

        // `EntityPlayer.onUpdate`: a container that is no longer usable (the chest is gone, or the eye is 8+ blocks away) closes.
        if self.screen.as_ref().is_some_and(|s| !s.chests.is_empty() && !chest::usable(&|x, y, z| self.chunks.block(x, y, z).unwrap_or(0), self.camera.pos, &s.chests)) {
            self.close_screen();
        }

        // M5: pick the block under the crosshair, dig it while the dig input is held, place on a tap.
        self.touch.tick(dt);
        let place = self.touch.take_place();
        // The inventory button, and taps on an open screen (its presses are not look / move / place).
        if self.touch.take_inventory() {
            if self.screen.is_some() {
                self.close_screen();
            } else {
                self.open_screen(Screen::new(2));
            }
        }
        for ev in self.touch.take_screen_events() {
            self.on_screen_event(ev);
        }
        let eye = self.camera.pos.as_dvec3();
        let end = eye + self.camera.forward().as_dvec3() * pick::REACH;
        // The crosshair hits a block's bounds (`Block.collisionRayTrace`: a sapling's small box, half a slab); liquids are not pickable
        // (vanilla's rayTraceBlocks skips them).
        let bounds = |x: i32, y: i32, z: i32| self.chunks.block_loaded(x, y, z).and_then(|b| pick_bounds(b, self.chunks.meta(x, y, z)));
        self.target = pick::ray_trace(&bounds, eye, end);

        // Digging runs on the 20 Hz game tick of the Java (hardness-based time, see world::dig).
        // A broken block drops its items (world::items) only if the held item may harvest it (stone needs a pickaxe),
        // and wears the tool (`Item.onBlockDestroyed`: 1 use, a sword 2, a hoe none).
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
                            let meta = self.chunks.meta(x, y, z); // before the cell is cleared: the drop depends on it
                            self.chunks.set_block(x, y, z, 0);
                            if let (61 | 62, Some(f)) = (id, self.furnaces.remove(&hit.pos)) {
                                for st in f.slots.into_iter().flatten() {
                                    self.drops.scatter(st, hit.pos);
                                }
                            }
                            // PlayerControllerSP.sendBlockRemoved: `canHarvestBlock` is read before the tool wears, so
                            // a tool that breaks on this block still harvests it; no suitable tool, no drop.
                            let harvest = dig::can_harvest(id, held);
                            if let Some(h) = held {
                                self.inv.damage(slot, craft::wear_on_break(h.id, id));
                            }
                            if harvest {
                                // BlockLeaves.harvestBlock: shears drop the leaves themselves, not a sapling.
                                if id == 18 && held.is_some_and(|h| h.id == craft::SHEARS) {
                                    self.drops.spawn_stack(ItemStack { id: 18, count: 1, damage: (meta & 3) as u16 }, hit.pos);
                                } else {
                                    self.drops.spawn_block(id, meta, hit.pos);
                                }
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
        // A tap that has a mob under the crosshair, nearer than the block behind it, hits it (`Minecraft.clickMouse`'s attack;
        // touch has no left button, and placing a block against a pig is not worth a gesture).
        if place {
            let (eye, fwd, slot) = (self.camera.pos, self.camera.forward(), self.touch.hotbar_slot);
            let wall = self.target.map_or(f32::MAX, |h| h.point.distance(eye.as_dvec3()) as f32);
            if let Some((i, _)) = self.mobs.pick(eye, fwd, pick::REACH as f32).filter(|&(_, t)| t < wall) {
                let held = self.inv.slots[slot].map(|s| s.id);
                let (dmg, wear) = mobs::attack(held);
                self.mobs.hit(i, dmg, self.player.pos);
                self.inv.damage(slot, wear);
                return;
            }
        }
        let Some(hit) = self.target else {
            if place {
                self.eat();
            }
            return;
        };
        if place {
            // Using a workbench or a furnace opens its screen instead of placing a block against it.
            match self.chunks.block_loaded(hit.pos.0, hit.pos.1, hit.pos.2) {
                Some(58) => {
                    self.open_screen(Screen::new(3));
                    return;
                }
                Some(61 | 62) => {
                    self.furnaces.entry(hit.pos).or_default();
                    self.open_screen(Screen::at_furnace(hit.pos));
                    return;
                }
                Some(chest::ID) => {
                    // `BlockChest.blockActivated`: a large chest opens as one; under a normal cube nothing opens. Never places.
                    if let Some(group) = chest::open(&|x, y, z| self.chunks.block(x, y, z).unwrap_or(0), hit.pos) {
                        for p in &group {
                            self.chests.entry(*p).or_default();
                        }
                        self.open_screen(Screen::at_chest(group));
                    }
                    return;
                }
                _ => {}
            }
            // ItemHoe.onItemUse: till dirt (or grass with air above) into farmland, one use of wear. A hoe never
            // places anything. ponytail: farmland stays farmland (no `BlockFarmland.updateTick` reverting it to dirt),
            // it is drawn as a full cube (the original is 15/16 high) and nothing grows on it yet.
            let slot = self.touch.hotbar_slot;
            if self.inv.slots[slot].is_some_and(|s| craft::is_hoe(s.id)) {
                let (x, y, z) = hit.pos;
                let (b, above) = (self.chunks.block_loaded(x, y, z), self.chunks.block_loaded(x, y + 1, z));
                if let (Some(b), Some(above)) = (b, above) {
                    if craft::can_till(b, above, hit.face) {
                        self.chunks.set_block(x, y, z, 60);
                        self.inv.damage(slot, 1);
                    }
                }
                return;
            }
            let (x, y, z) = pick::place_pos(&hit);
            // Items below 256 are blocks; anything else cannot be placed. ItemBlock refuses a solid block at y = 127
            // (ponytail: plants are refused there too).
            if let Some(s) = self.inv.slots[slot].filter(|s| s.id < 256) {
                // A torch's facing comes from the face aimed at and what can hold it (None = nothing to hang on: no placement).
                let meta = if s.id == 50 { pick::torch_meta(&|a, b, c| self.chunks.block_loaded(a, b, c).unwrap_or(0), (x, y, z), hit.face) } else { Some(items::placed_meta(s)) };
                if let Some(meta) = meta.filter(|_| y < 127 && self.chunks.block_loaded(x, y, z).is_some_and(pick::replaceable) && !pick::overlaps_player((x, y, z), s.id as u8, self.player.pos) && (s.id != chest::ID as u16 || chest::can_place(&|a, b, c| self.chunks.block(a, b, c).unwrap_or(0), (x, y, z)))) {
                    if self.chunks.set_block_meta(x, y, z, s.id as u8, meta) {
                        self.inv.consume(slot);
                    }
                }
            } else {
                self.eat();
            }
        }
    }

    /// `Minecraft.clickMouse` -> `sendUseItem`: a tap that neither placed a block nor opened a screen uses the held item;
    /// food is eaten, aimed at a block or not.
    /// `Explosion` at `at`: the rays pick the cells (`Mobs::explode`), a broken block drops with chance 0.3 (vanilla: per item),
    /// the player takes `(impact^2 + impact) / 2 x 8 x power + 1` (no exposure test, no knockback).
    fn explode(&mut self, at: glam::Vec3, power: f32) {
        let cells = self.mobs.explode(&|x: i32, y: i32, z: i32| self.chunks.block(x, y, z), at, power);
        for (x, y, z) in cells {
            let Some(id) = self.chunks.block_loaded(x, y, z).filter(|&b| b > 0) else { continue };
            let meta = self.chunks.meta(x, y, z);
            if self.mobs.rng.next_float() < 0.3 {
                self.drops.spawn_block(id, meta, (x, y, z));
            }
            self.chunks.set_block(x, y, z, 0);
        }
        let impact = 1.0 - (self.player.pos - at).length() / (power * 2.0);
        if impact > 0.0 {
            self.vitals.hurt(((impact * impact + impact) / 2.0 * 8.0 * power + 1.0) as i32);
        }
    }

    fn eat(&mut self) {
        if let Some(n) = self.inv.eat(self.touch.hotbar_slot) {
            self.vitals.heal(n);
        }
    }

    /// `EntityPlayer.onDeath`: the open screen's cursor and grid, then the whole inventory, fall to the ground where he
    /// died (ponytail: `scatter` drops one entity per stack, vanilla throws each with a random spread). The pause
    /// menu is the death screen: its resume button respawns (top of `step_frame`).
    fn die(&mut self) {
        self.close_screen();
        let at = (self.player.pos.x.floor() as i32, self.player.pos.y.floor() as i32, self.player.pos.z.floor() as i32);
        for slot in self.inv.slots.iter_mut() {
            if let Some(st) = slot.take() {
                self.drops.scatter(st, at);
            }
        }
        self.touch.pause();
    }

    /// Open a container screen: the 2x2 inventory, a workbench's 3x3, a furnace or chests.
    fn open_screen(&mut self, screen: Screen) {
        self.screen = Some(screen);
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

    /// The slots behind the open screen: a furnace's 3, or the chests' 27 each (upper half first); `None` for the 2x2 and the workbench.
    fn ext(&self, s: &Screen) -> Option<Vec<Option<ItemStack>>> {
        if let Some(f) = s.furnace.and_then(|p| self.furnaces.get(&p)) {
            return Some(f.slots.to_vec());
        }
        (!s.chests.is_empty()).then(|| s.chests.iter().flat_map(|p| self.chests.get(p).copied().unwrap_or_default()).collect())
    }

    /// Run `f` on the open screen with the slots behind it (`ext`): they are copied out and written back, so a large chest's
    /// two halves are one slice, as `InventoryLargeChest` makes them. `None` when no screen is open.
    fn with_ext<R>(&mut self, f: impl FnOnce(&mut Screen, &mut Inventory, Option<&mut [Option<ItemStack>]>) -> R) -> Option<R> {
        let mut s = self.screen.take()?;
        let mut ext = self.ext(&s);
        let r = f(&mut s, &mut self.inv, ext.as_deref_mut());
        if let Some(e) = ext {
            if let Some(furn) = s.furnace.and_then(|p| self.furnaces.get_mut(&p)) {
                furn.slots.copy_from_slice(&e);
            }
            for (p, part) in s.chests.iter().zip(e.chunks(chest::SIZE)) {
                if let Some(c) = self.chests.get_mut(p) {
                    c.copy_from_slice(part);
                }
            }
        }
        self.screen = Some(s);
        Some(r)
    }

    /// A finger event on the open screen: a press that lifts where it began is a tap; one that drags over other
    /// slots with a stack on the cursor spreads it, one item per slot (the start slot included).
    fn on_screen_event(&mut self, ev: ScreenEv) {
        let (w, h) = (self.gpu.config.width as f32, self.gpu.config.height as f32);
        let Some(gw) = self.screen.as_ref().map(|s| s.gw) else { return };
        let mut fill = Vec::new();
        match ev {
            ScreenEv::Down(x, y) => {
                self.gesture = Some(Gesture { start: craft::slot_at(gw, w, h, x, y), spread: false, visited: Vec::new() });
            }
            ScreenEv::Move(x, y) => {
                let (Some(g), Some(cur)) = (self.gesture.as_mut(), craft::slot_at(gw, w, h, x, y)) else { return };
                if self.screen.as_ref().map_or(true, |s| s.cursor.is_none()) || (!g.spread && g.start == Some(cur)) {
                    return;
                }
                if !g.spread {
                    g.spread = true;
                    fill.extend(g.start);
                }
                fill.push(cur);
                fill.retain(|id| !g.visited.contains(id));
                fill.dedup();
                g.visited.extend(fill.iter().copied());
            }
            ScreenEv::Up(x, y) => {
                if !self.gesture.take().is_some_and(|g| g.spread) {
                    self.on_screen_tap(x, y);
                }
            }
        }
        for id in fill {
            self.with_ext(|s, inv, ext| s.drop_one(id, inv, ext));
        }
    }

    /// A tap on the open screen: the "place one" toggle, a slot, or outside the panel (throws the cursor stack).
    fn on_screen_tap(&mut self, x: f32, y: f32) {
        let (w, h) = (self.gpu.config.width as f32, self.gpu.config.height as f32);
        let Some(gw) = self.screen.as_ref().map(|s| s.gw) else { return };
        let one = self.one_mode;
        if craft::on_mode_button(gw, w, h, x, y) {
            self.one_mode = !one;
        } else if let Some(id) = craft::slot_at(gw, w, h, x, y) {
            self.with_ext(|s, inv, ext| s.click(id, one, inv, ext));
        } else if !craft::in_panel(gw, w, h, x, y) {
            let Some(s) = self.screen.as_mut() else { return };
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
        let outline_indices = self.target.map_or(0, |h| {
            let (x, y, z) = h.pos;
            let b = self.chunks.block_loaded(x, y, z).and_then(|b| outline_bounds(b, self.chunks.meta(x, y, z)));
            b.map_or(0, |b| self.outline.update(&self.gpu.queue, h.pos, b))
        });
        let item_indices = self.item_mesh.update(&self.gpu.queue, &self.drops, &self.mobs, &self.ticks.falling);

        // Sky and fog colour from the sun angle, the weather and the climate under the player; the frame is cleared to
        // the fog colour and the sky pass is drawn over it (vanilla order).
        let (px, pz) = (self.camera.pos.x.floor() as i32, self.camera.pos.z.floor() as i32);
        let temp = self.chunks.temperature_at(px, pz) as f32;
        let partial = self.world_ticks.fract() as f32;
        let angle = sky::celestial_angle(self.world_ticks as u64, partial);
        let (rain, thunder) = (self.weather.rain(partial), self.weather.thunder(partial));
        let sky_rgb = sky::sky_color(angle, temp, rain, thunder);
        // Light shafts march through the shadow map, so the shadow pass also runs for them (sunrise, sunset, rain, the moon);
        // the terrain still takes its shadow only from `sun_strength`. Not under water (the pack has its own shafts there).
        let e = self.camera.pos;
        let eye_in_water = matches!(self.chunks.block_loaded(e.x.floor() as i32, e.y.floor() as i32, e.z.floor() as i32), Some(8 | 9));
        let mut vlp = vl::params(self.world_ticks, angle, rain, sky_rgb, e.y);
        vlp.active &= !eye_in_water;
        let sun_strength = sky::shadow_strength(angle, rain);
        vlp.shadow = sun_strength; // surface shadows come from the volumetric pass (`render::vl`), not the chunk shader
        // The shadow map is only redrawn when the eye, the light or the geometry moved enough (`ShadowCache`); the terrain and
        // the shafts read it with the matrix and light it was drawn with.
        let draw_shadow = self.shadow.refresh(sun_strength > 0.0 || vlp.active, e, vlp.light, self.chunks.mesh_gen);
        vlp.light = self.shadow.light;
        let shadow_vp = self.pipe.upload_uniforms(&self.gpu.queue, view, proj, self.shadow.light, sun_strength, self.shadow.center);
        let (frustum, shadow_frustum) = (Frustum::from_view_proj(proj * view), Frustum::from_view_proj(shadow_vp));
        let fog = sky::fog_color(angle, sky_rgb, rain, thunder);
        // Terrain fog (AstraLex `NormalFog`): fades to the horizon colour at the render distance; off under water.
        self.pipe.set_fog(&self.gpu.queue, fog, if eye_in_water { 0.0 } else { (RENDER_DIST * 16) as f32 }, rain);
        let clear = wgpu::Color { r: fog[0] as f64, g: fog[1] as f64, b: fog[2] as f64, a: 1.0 };
        self.sky.update(&self.gpu.queue, &self.camera, &SkyFrame { angle, rain, sky: sky_rgb, fog });
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

        if draw_shadow {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow_pass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.pipe.shadow_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rp.set_pipeline(&self.pipe.shadow_pipeline);
            rp.set_bind_group(0, &self.pipe.bind_group, &[]);
            for m in self.chunks.meshes_where(|min, max| shadow_frustum.intersects_aabb(min, max)) {
                rp.set_vertex_buffer(0, m.vbuf.slice(..));
                rp.set_index_buffer(m.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(0..m.index_count, 0, 0..1);
            }
        }
        // Moving things cast shadows through a second map, redrawn every frame (the terrain map is cached, a walking mob would leave a
        // trail in it). It is cleared once more after the last entity is gone. ponytail: one draw of the whole item buffer, no culling.
        let draw_ent = (sun_strength > 0.0 || vlp.active) && (item_indices > 0 || self.ent_shadow_dirty);
        if draw_ent {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("entity_shadow_pass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.pipe.ent_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if item_indices > 0 {
                rp.set_pipeline(&self.pipe.shadow_pipeline);
                rp.set_bind_group(0, &self.pipe.bind_group, &[]);
                rp.set_vertex_buffer(0, self.item_mesh.vbuf.slice(..));
                rp.set_index_buffer(self.item_mesh.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(0..item_indices, 0, 0..1);
            }
            self.ent_shadow_dirty = item_indices > 0;
        }
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
                    // Only the light-shaft march reads the depth back; otherwise a tile GPU never writes it to memory.
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: if vlp.active { wgpu::StoreOp::Store } else { wgpu::StoreOp::Discard },
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.sky.draw(&mut rp);
            rp.set_pipeline(&self.pipe.pipeline);
            rp.set_bind_group(0, &self.pipe.bind_group, &[]);
            rp.set_bind_group(1, &self.pipe.shadow_bind, &[]);
            // One draw per run of chunk sections that is in the render shape and the view frustum.
            let (rh, rv) = ((RENDER_DIST * 16) as f32, (RENDER_VERT * 16) as f32);
            self.chunks.draw_ranges(self.camera.pos, RENDER_SHAPE, rh, rv, false, |min, max| frustum.intersects_aabb(min, max), |m, ib, r| {
                rp.set_vertex_buffer(0, m.vbuf.slice(..));
                rp.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(r, 0, 0..1);
            });
            if item_indices > 0 {
                rp.set_vertex_buffer(0, self.item_mesh.vbuf.slice(..));
                rp.set_index_buffer(self.item_mesh.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(0..item_indices, 0, 0..1);
            }
            // Water last: blended over the terrain and the entities, no depth write (`water_pipeline`; same groups as the terrain).
            rp.set_pipeline(&self.pipe.water_pipeline);
            self.chunks.draw_ranges(self.camera.pos, RENDER_SHAPE, rh, rv, true, |min, max| frustum.intersects_aabb(min, max), |m, ib, r| {
                rp.set_vertex_buffer(0, m.vbuf.slice(..));
                rp.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(r, 0, 0..1);
            });
            rp.set_pipeline(&self.pipe.pipeline);
            if outline_indices > 0 {
                rp.set_vertex_buffer(0, self.outline.vbuf.slice(..));
                rp.set_index_buffer(self.outline.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(0..outline_indices, 0, 0..1);
            }
        }
        if vlp.active {
            self.shafts.march(&self.gpu, &mut enc, &self.pipe.shadow_bind, &self.camera, shadow_vp, &vlp);
        }
        // M12: HUD overlay pass. Built into the same encoder so the HUD
        // never gets lost if the GPU drops a frame.
        self.build_hud();
        let hud_vertex_count = self.hud_verts.len();
        if hud_vertex_count > 0 || vlp.active {
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
            // The light shafts are blended here, under the HUD, so the frame is loaded once for both.
            if vlp.active {
                self.shafts.composite(&mut rp);
            }
            if hud_vertex_count > 0 {
                rp.set_pipeline(&self.hud.pipeline);
                rp.set_bind_group(0, &self.hud.bind_group, &[]);
                rp.set_vertex_buffer(0, self.hud.vbuf.slice(..));
                rp.draw(0..hud_vertex_count as u32, 0..1);
            }
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
        // `ext` reads all of `self`, so it is taken before `hud_verts` is borrowed mutably.
        let ext = self.screen.as_ref().and_then(|s| self.ext(s));
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
            let (ox, oy, k) = craft::panel(s.gw, w, h);
            let (pw, ph) = craft::panel_size(s.gw);
            HudPipeline::push_quad(v, 0.0, 0.0, w, h, [0.0, 0.0, 0.0, 0.55]);
            HudPipeline::push_outlined_quad(v, ox, oy, pw * k, ph * k, [0.78, 0.78, 0.78, 0.97], [0.15, 0.15, 0.15, 1.0], (2.0 * k).max(2.0));
            let slot = |v: &mut Vec<HudVertex>, c: (f32, f32, f32), st: Option<ItemStack>, edge: [f32; 4]| {
                let inner = (c.0 + k, c.1 + k, c.2 - 2.0 * k, c.2 - 2.0 * k);
                HudPipeline::push_quad(v, c.0, c.1, c.2, c.2, edge);
                HudPipeline::push_quad(v, inner.0, inner.1, inner.2, inner.3, [0.55, 0.55, 0.55, 1.0]);
                if let Some(st) = st {
                    push_stack(v, inner, st);
                }
            };
            let furn = s.furnace.and_then(|p| self.furnaces.get(&p));
            for (id, ux, uy) in craft::layout(s.gw) {
                slot(v, craft::cell(s.gw, w, h, (ux, uy)), s.get(&self.inv, ext.as_deref(), id), [0.3, 0.3, 0.3, 1.0]);
            }
            if !s.chests.is_empty() {
                // `GuiChest.drawGuiContainerForegroundLayer`: the chest's name at (8, 6), "Inventory" at (8, ySize - 96 + 2), colour 0x404040.
                let (title, ink) = (if s.chests.len() > 1 { "Large chest" } else { "Chest" }, [0.25, 0.25, 0.25, 1.0]);
                HudPipeline::push_text(v, ox + 8.0 * k, oy + 6.0 * k, k, title, ink);
                HudPipeline::push_text(v, ox + 8.0 * k, oy + (ph - 94.0) * k, k, "Inventory", ink);
            }
            if let Some(f) = furn {
                // Cook arrow (fills left to right over 200 ticks) and flame (drains from the top), vanilla positions.
                let u = |ux: f32, uy: f32, uw: f32, uh: f32| (ox + ux * k, oy + uy * k, uw * k, uh * k);
                let (ax, ay, aw, ah) = u(79.0, 34.0, 24.0, 17.0);
                HudPipeline::push_quad(v, ax, ay, aw, ah, [0.45, 0.45, 0.45, 1.0]);
                HudPipeline::push_quad(v, ax, ay, aw * f.cook as f32 / 200.0, ah, [1.0; 4]);
                let (fx, fy, fw, fh) = u(56.0, 36.0, 14.0, 14.0);
                let left = f.burn as f32 / f.item_burn.max(1) as f32;
                HudPipeline::push_quad(v, fx, fy, fw, fh, [0.45, 0.45, 0.45, 1.0]);
                HudPipeline::push_quad(v, fx, fy + fh * (1.0 - left), fw, fh * left, [1.0, 0.6, 0.1, 1.0]);
            }
            // The "place one" toggle: blue = whole stacks, orange with a 1 = one at a time (what the right mouse
            // button does on a desktop). Dragging the stack over slots spreads it without the toggle.
            let (mx, my, ms) = craft::cell(s.gw, w, h, craft::MODE);
            let fill = if self.one_mode { [0.85, 0.5, 0.1, 1.0] } else { [0.2, 0.4, 0.7, 1.0] };
            HudPipeline::push_outlined_quad(v, mx, my, ms, ms, fill, [0.15, 0.15, 0.15, 1.0], k.max(2.0));
            if self.one_mode {
                HudPipeline::push_number(v, mx + ms * 0.32, my + ms * 0.2, ms * 0.6, 1, [1.0; 4]);
            }
            // The picked-up stack floats just above the finger.
            if let Some(c) = s.cursor {
                let (cx, cy) = self.touch.cursor_pos;
                let sz = 16.0 * k;
                push_stack(v, (cx - sz * 0.5, cy - sz * 1.4, sz, sz), c);
            }
            // Item name while a finger is down (hover has no touch twin): the picked-up stack, else the stack under the
            // finger. `GuiContainer.drawScreen` names only the hovered slot and only with an empty cursor; the held
            // stack is named too because a tap picks the slot's stack up and the finger then covers it.
            if self.gesture.is_some() {
                let (cx, cy) = self.touch.cursor_pos;
                let under = || craft::slot_at(s.gw, w, h, cx, cy).and_then(|id| s.get(&self.inv, ext.as_deref(), id));
                if let Some(name) = s.cursor.or_else(under).and_then(items::name) {
                    let bw = (HudPipeline::text_width(name) as f32 + 6.0) * k;
                    let ty = if cy > 36.0 * k { cy - 34.0 * k } else { cy + 14.0 * k };
                    HudPipeline::push_tooltip(v, (cx - bw * 0.5).clamp(0.0, (w - bw).max(0.0)), ty, k, name);
                }
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

        // Name of the item just switched to, centred above the hotbar and the hearts.
        // UNVERIFIED: not in b1.7.3 (Beta 1.8 added it); the 2 s, the 0.75 scale and the spot are invented.
        if let (true, Some(name)) = (play && self.tip.1 > 0.0, self.inv.slots[self.touch.hotbar_slot].and_then(items::name)) {
            let (w, h) = (self.gpu.config.width as f32, self.gpu.config.height as f32);
            let k = craft::panel(0, w, h).2;
            let (hx, hy, hw, _) = layout.hotbar[4];
            let bw = (HudPipeline::text_width(name) as f32 + 6.0) * k * 0.75;
            HudPipeline::push_tooltip(v, hx + hw * 0.5 - bw * 0.5, hy - 34.0 * k, k * 0.75, name);
        }

        if play {
            // Hearts (GuiIngame: 10, 8 units apart on the 20 unit hotbar cells, left half, above the hotbar): a full
            // heart is 2 health. Air bubbles sit over the right half while the eye is under water: whole ones
            // `ceil((air - 2) * 10 / 300)`, then the one that is about to pop, faded. Flat shapes until M14 sprites.
            let (hx, hy, hw, _) = layout.hotbar[0];
            let (sz, y) = (hw * 0.45, hy - hw * 0.6);
            for i in 0..10 {
                let x = hx + i as f32 * hw * 0.4;
                let at = 2 * i + 1;
                HudPipeline::push_quad(v, x, y, sz, sz, [0.25, 0.0, 0.0, 0.8]);
                if at < self.vitals.health {
                    HudPipeline::push_quad(v, x, y, sz, sz, [0.9, 0.1, 0.1, 1.0]);
                } else if at == self.vitals.health {
                    HudPipeline::push_quad(v, x, y, sz * 0.5, sz, [0.9, 0.1, 0.1, 1.0]);
                }
            }
            if self.vitals.air < vitals::MAX_AIR {
                let air = self.vitals.air as f32;
                let whole = ((air - 2.0) * 10.0 / 300.0).ceil().max(0.0) as i32;
                let popping = (air * 10.0 / 300.0).ceil().max(0.0) as i32 - whole;
                let (rx, _, rw, _) = layout.hotbar[8];
                for i in 0..whole + popping {
                    let alpha = if i < whole { 0.9 } else { 0.4 };
                    HudPipeline::push_disc(v, rx + rw - sz * 0.5 - i as f32 * hw * 0.4, y - hw * 0.5, sz * 0.4, [0.55, 0.8, 1.0, alpha]);
                }
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
            // Dead: the same menu, tinted red; the resume button respawns.
            let tint = if self.vitals.dead() { [0.55, 0.0, 0.0, 0.6] } else { [0.0, 0.0, 0.0, 0.55] };
            HudPipeline::push_quad(v, 0.0, 0.0, w, h, tint);
            let (bx, by, bw, bh) = layout.resume;
            HudPipeline::push_outlined_quad(v, bx, by, bw, bh, [0.20, 0.30, 0.50, 0.95], [0.95, 0.95, 0.95, 0.95], 4.0);
            // Play triangle, approximated by two stacked quads.
            let tx = bx + bw * 0.46;
            let ty = by + bh * 0.25;
            let tw = bw * 0.08;
            let th = bh * 0.50;
            HudPipeline::push_quad(v, tx, ty, tw, th * 0.5, [1.0; 4]);
            HudPipeline::push_quad(v, tx, ty + th * 0.5, tw, th * 0.5, [1.0; 4]);
            // TEMPORARY reset-world button: dark red; after the first tap bright red (tap again to confirm). Its symbol is a
            // hollow square (there is no text in the HUD yet).
            let (rx, ry, rw, rh) = layout.reset;
            let fill = if self.touch.reset_armed { [0.90, 0.10, 0.10, 0.95] } else { [0.45, 0.10, 0.10, 0.90] };
            HudPipeline::push_outlined_quad(v, rx, ry, rw, rh, fill, [0.95, 0.95, 0.95, 0.95], 4.0);
            let (s, t) = (rh * 0.4, rh * 0.07);
            let (qx, qy) = (rx + (rw - s) * 0.5, ry + (rh - s) * 0.5);
            for (x, y, w, h) in [(qx, qy, s, t), (qx, qy + s - t, s, t), (qx, qy, t, s), (qx + s - t, qy, t, s)] {
                HudPipeline::push_quad(v, x, y, w, h, [1.0; 4]);
            }
        }

        // Cap to the buffer capacity; extra quads are silently dropped.
        let max = self.hud.quad_capacity * 6;
        if v.len() > max {
            v.truncate(max);
        }
    }
}

/// A new world's spawn point (feet position): dry ground closest to the centre of chunk (0, 0), not under water (a
/// fixed spawn at y=60 put the camera inside the sea), and not on top of a tree. Needs chunks 0..=1 final.
fn find_spawn(chunks: &ChunkManager) -> glam::Vec3 {
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
    glam::Vec3::new(spawn_x as f32 + 0.5, spawn_feet_y, spawn_z as f32 + 0.5)
}

/// One item stack drawn into the cell `(x, y, w, h)`: the flat colour of its tile (a tool is a stick-coloured handle
/// with a head in the material colour, shaped per kind, so pickaxe, axe, shovel, sword and hoe can be told apart), a wear bar
/// under a damaged tool, and the count bottom right with a shadow like `RenderItem.renderItemOverlayIntoGUI`
/// (shown above 1 only). Real item sprites are M14.
// UNVERIFIED: the head shapes and the wear-bar colours are stand-ins for the item sprites and `renderItemOverlay`.
fn push_stack(v: &mut Vec<HudVertex>, (hx, hy, hw, hh): (f32, f32, f32, f32), s: ItemStack) {
    let pad = (hw.min(hh) * 0.18).max(2.0);
    let rgb = |tile: u16| {
        let c = atlas::tile_color(tile);
        [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, 1.0]
    };
    let (ix, iy, iw, ih) = (hx + pad, hy + pad, hw - pad * 2.0, hh - pad * 2.0);
    match craft::tool(s.id) {
        None if s.id == craft::SHEARS => {
            // Two blades over a grip.
            let c = rgb(items::tile(s.id).into());
            HudPipeline::push_quad(v, ix + iw * 0.15, iy, iw * 0.2, ih * 0.6, c);
            HudPipeline::push_quad(v, ix + iw * 0.65, iy, iw * 0.2, ih * 0.6, c);
            HudPipeline::push_quad(v, ix + iw * 0.25, iy + ih * 0.6, iw * 0.5, ih * 0.4, rgb(17));
        }
        None => HudPipeline::push_quad(v, ix, iy, iw, ih, rgb(items::stack_tile(s))),
        Some((kind, _)) => {
            HudPipeline::push_quad(v, ix + iw * 0.42, iy, iw * 0.16, ih, rgb(17));
            let head = rgb(items::tile(s.id).into());
            match kind {
                craft::Kind::Sword => {
                    HudPipeline::push_quad(v, ix + iw * 0.4, iy, iw * 0.2, ih * 0.7, head);
                    HudPipeline::push_quad(v, ix + iw * 0.2, iy + ih * 0.68, iw * 0.6, ih * 0.1, head);
                }
                craft::Kind::Hoe => HudPipeline::push_quad(v, ix + iw * 0.42, iy, iw * 0.5, ih * 0.18, head),
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
                        // App-private storage, where the world is saved.
                        let dir = app.internal_data_path().unwrap_or_else(std::env::temp_dir);
                        log::info!("M3: InitWindow, native {}x{}, starting GPU init thread", init_w, init_h);
                        let h = std::thread::Builder::new()
                            .stack_size(8 * 1024 * 1024)
                            .spawn(move || {
                                let r = std::panic::catch_unwind(|| {
                                    pollster::block_on(App::init(ptr as *mut c_void, init_w, init_h, dir))
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
                        // Window is going away (home button, rotate, etc): the world is rebuilt from the save on
                        // return, so save everything first. Then drop the surface before the ANativeWindow dies.
                        if let Some(a) = app_state.as_mut() {
                            a.save(usize::MAX);
                        }
                        app_state = None;
                    }
                    // The process may be killed after this without another event.
                    MainEvent::Pause => {
                        if let Some(a) = app_state.as_mut() {
                            a.save(usize::MAX);
                        }
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
