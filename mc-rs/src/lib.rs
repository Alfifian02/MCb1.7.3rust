//! mc-rs: Minecraft b1.7.3 in Rust for Android.
//!
//! M12 - Landscape touch UX (move stick, look drag, jump button, hotbar, pause menu) on
//! top of M2 (camera + physics) and M3 (overworld generation), with the world streamed by
//! `world::chunks::ChunkManager` (render-distance ring, generation on worker threads).

mod audio;
mod render;
mod gles;
mod backend;
mod world;
mod gpu;
mod input;
mod immersive;
#[cfg(feature = "pad-rumble")]
mod rumble;
mod menu;
mod options;
mod satchel;

use crate::input::touch_ui::{PointerRole, ScreenEv, TouchUi};
use crate::menu::Menu;
use crate::options::{Pause, PauseEv, PauseInfo, Settings};
use crate::render::hud::{sprites as sp, HudPipeline, HudVertex};

use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};
use android_activity::input::InputEvent as IEv;
use core::ffi::c_void;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::backend::Backend;
use crate::gpu::context::{Drs, FrameTimes};
use crate::gpu::gfx::{Drawn, WorldDraw};
use crate::render::atlas;
use crate::render::debug::Row;
use crate::render::items::{ItemMesh, SignView};
use crate::render::camera::{FirstPersonCamera, Frustum};
use crate::render::look::{self, GlowMesh, Look};
use crate::render::outline::Outline;
use crate::render::color;
use crate::world::gen::overworld::WorldType;
use crate::world::lod::{self, Lod};
use crate::world::pick::{self, Hit};
use crate::world::dig::{self, Dig};
use crate::world::chest::{self, Chest};
use crate::world::decor::{self, Edit, Key, Painting, Text};
use crate::world::craft::{self, Furnace, Screen};
use crate::world::items::{self, Drops, Inventory, ItemStack};
use crate::world::gear::{self, Hook, Used};
use crate::world::boat;
use crate::world::pack;
use crate::world::book;
use crate::world::minecart;
use crate::world::portal;
use crate::world::mobs::{self, Ctx, Ev, Mobs, Strike};
use crate::world::ticks::{self, Ticks};
use crate::world::save::{self, Level};
use crate::world::sky;
use crate::world::sprint::{self, Sprint};
use crate::world::chunk::{is_plant, normal_cube, outline_bounds, physics_id, pick_bounds};
use crate::world::chunks::{chunk_coord, ChunkManager, Shape, WATER_RANGE};
use crate::world::physics::{self, Player};
use crate::world::redstone;
use crate::world::vitals::{self, Env, Vitals};
use crate::world::effects::{self, Effects, Spawner};
use crate::render::weather;

/// Render distance in chunks (a circle of this radius is meshed and drawn, two more are generated).
const RENDER_DIST: i32 = 4;
/// The world seed (its save folder is `world-<seed>`).
/// The world the player is in (its folder): when Android takes the window away the `App` is dropped and built again, and that world
/// is entered again instead of the title screen.
static LAST_WORLD: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
/// Vertical render distance in chunks (16 blocks): sections further above or below the eye are not drawn.
const RENDER_VERT: i32 = 3;
/// `Sphere`: what is drawn is within `RENDER_DIST` blocks-of-16 in 3D (matches the fog, which is by 3D distance);
/// `Cylinder`: a circle of that radius at any height within `RENDER_VERT`.
const RENDER_SHAPE: Shape = Shape::Sphere;

/// Seconds of play between autosaves (the level file and a few chunks; pause and exit save everything).
const AUTOSAVE_SECS: f32 = 5.0;
/// Chunks written per autosave, so the first ones after generating a lot of terrain do not hitch a frame.
const AUTOSAVE_CHUNKS: usize = 24;

/// Console-Edition look: the vanilla light table + face shades. It is the only look now (the Photon passes were removed); the mesher and
/// the sky still read this switch for their legacy branches.
pub(crate) const LEGACY: bool = true;

const LOOK_SENS: f32 = 0.004;
/// Zoom button magnification (Zoomify's default).
const ZOOM: f32 = 4.0;
/// World units per second when a d-pad direction is held.
const MOVE_SPEED: f32 = physics::WALK_SPEED;
/// Eye offset from `player.pos`, which is the CENTRE of the 1.8-tall box. Steve's eyes are
/// 1.62 above his feet (EntityPlayer.yOffset), i.e. 1.62 - 0.9 above the centre.
const EYE_HEIGHT: f32 = 1.62 - physics::HALF.y;

/// Cortes Combat C4 (NOT b1.7.3, UNVERIFIED): a meal in the mouth. Food takes `EAT_TICKS` (the mod's 1.21 use time) and any damage taken ends it
/// unless the guard is up (`EatInterruptHandler`); it also ends when the hotbar slot changes or the item is gone.
#[derive(Clone, Copy)]
struct Eating {
    left: u32,
    slot: usize,
    health: i32,
}
const EAT_TICKS: u32 = 32;

struct App {
    /// The renderer: wgpu (Vulkan) or OpenGL ES 3.2 (`backend.rs`).
    be: Backend,
    /// The world's type (chosen on the create screen, saved in its `info`): vanilla b1.7.3 terrain, Tectonic (the `tectonic` crate) or Blooming (Vanilla + the Blooming Biosphere biomes).
    world_type: WorldType,
    /// Far terrain behind the chunks (Voxy-style voxel LoD, `world::lod`): the view reaches the horizon with the same blocks; none in
    /// the Nether (it would be overworld terrain).
    lod: Option<Lod>,
    seed: i64,
    /// The app's private storage: `saves/<folder>` holds the worlds.
    root: PathBuf,
    /// The title screen, world list and world creation; `Some` while no world is loaded.
    menu: Option<Menu>,
    /// The far terrain was drawn this frame (`opts.lod` and a dimension that has it): the draw passes read it.
    lod_on: bool,
    /// `GameSettings` (`<root>/options.txt`): the options screens change it, the frame reads it.
    opts: Settings,
    /// The game menu (`GuiIngameMenu`, or the death screen) while `touch.paused`.
    pause: Option<Pause>,
    chunks: ChunkManager,
    camera: FirstPersonCamera,
    player: Player,
    last_frame: Instant,
    frames: u64,
    fps_last: Instant,
    /// FPS counter: average over the last second, and a moving average over the last ~10 s of play (`fps_avg`; 0 = not started).
    fps_recent: u32,
    fps_avg: f32,
    /// Shown under the FPS: CPU ms per frame and the longest frame (ms) of the last second; `frame_prev` / `worst_acc` feed the latter.
    cpu_ms: u32,
    /// Scene-pass GPU time (`gpu::timing`): this second's sum, count, and last second's mean in tenths of a ms (0 = not measured); `gpu_buf` is the scratch for `Backend::gpu_ms`.
    gpu_acc: f32,
    gpu_n: u32,
    gpu_tenths: u32,
    gpu_buf: Vec<f32>,
    worst_ms: u32,
    worst_acc: f32,
    /// The 99th percentile of the frame gaps of the last second (ms), `FrameTimes` feeding it, and the panel's refresh rate (60 until the display says).
    p99_ms: u32,
    times: FrameTimes,
    refresh_hz: f32,
    /// The previous frame was a paused one (its idle wait is not a spike).
    was_paused: bool,
    frame_prev: Instant,
    /// `render::look`: the emitters and flowers round the player, the drifting motes, and the sprite buffers of the halos and motes.
    scan: look::Scan,
    motes: look::Motes,
    glow: GlowMesh,
    // M12: touch UI state + HUD renderer.
    touch: TouchUi,
    /// Scratch vertex buffer for the HUD; cleared each frame, then emitted.
    hud_verts: Vec<HudVertex>,
    // M5: block under the crosshair (within reach) and its outline buffers.
    target: Option<Hit>,
    outline: Outline,
    /// World time in ticks (20 per second); a new world starts at 0, sunrise.
    world_ticks: f64,
    /// Sun, moon and stars drawn behind the terrain, and the rain/thunder timers (not saved: a loaded world starts clear).
    /// `weather_ticks` counts the whole ticks the weather has run, so it catches up to `world_ticks` one tick at a time.
    weather: sky::Weather,
    weather_ticks: u64,
    /// The biome's sky, fog, haze, clouds and stars, eased in real time (`world::atmos`).
    atmos: world::atmos::Atmos,
    /// Water particles, lightning and the dungeon spawners (`world::effects`).
    fx: Effects,
    /// The player's box touched water last frame (`Entity.inWater`): a splash on entry.
    was_in_water: bool,
    /// `Entity.ySize` of the player, as far as the camera goes: how far the eye hangs below standing height (0.08 sneaking, then it decays).
    sneak_y: f32,
    /// The horizontal push an explosion gave the player (m/s, `Explosion.doExplosionA`'s `motion +=`): added to the walk each frame and dying away (`frame`).
    kick: glam::Vec3,
    /// Sprinting (`world::sprint`, Retro Sprint): ticked at 20 Hz, its push and field of view are read every frame.
    sprint: Sprint,
    /// A wall stopped the player's move on x or z since the last sprint tick (`horizontalCollision`).
    sprint_blocked: bool,
    /// Hotbar slot last seen and seconds its item name still shows (`GuiIngame` shows it ~2 s after a switch).
    tip: (usize, f32),
    /// The local player's model state (`render::player`): body yaw, limb and arm swing, ticked at 20 Hz.
    body: render::player::Anim,
    /// The eye's place while the frame is drawn from behind (`third_pos`), put back after the frame.
    third_saved: Option<(glam::Vec3, f32)>,
    /// The shoulder-surf camera (`render::shoulder`), reset on every frame it is not the view in use.
    shoulder: render::shoulder::Rig,
    /// Survival digging state and the leftover time towards the next 20 Hz dig tick.
    dig: Dig,
    dig_acc: f32,
    /// Hotbar stacks (starts empty), dropped items on the ground and their draw buffers.
    inv: Inventory,
    drops: Drops,
    mobs: Mobs,
    boats: boat::Boats,
    carts: minecart::Carts,
    /// Detector rails switched on by a cart, to be switched off when it has gone (`scheduleBlockUpdate` of `BlockDetectorRail`).
    detectors: std::collections::HashSet<(i32, i32, i32)>,
    /// In the Nether (its folder is `<world>/nether`; `<world>/dim` says which one a restart resumes in).
    nether: bool,
    /// Time inside a portal, 0..1 (`timeInPortal`: +0.0125 a tick inside, -0.05 outside; at 1 the player travels) and the 10 tick
    /// cooldown after arriving (`timeUntilPortal`), in seconds.
    portal_t: f32,
    portal_cd: f32,
    /// Block updates (scheduled + random ticks, notifications, falling blocks), stepped on the 20 Hz game tick.
    ticks: Ticks,
    /// Sounds (`SoundManager`): effects, music and records, `audio/`.
    audio: audio::Audio,
    /// Cave sounds and the water, fire and portal hum around the player (`World` 20 Hz).
    ambient: audio::Ambient,
    item_mesh: ItemMesh,
    /// Open container screen (the 2x2 inventory or a workbench's 3x3) and its "place one" click toggle.
    screen: Option<Screen>,
    one_mode: bool,
    /// Shift-click toggle (second button beside `one_mode`): a tap on a slot quick-moves its stack.
    shift_mode: bool,
    /// The immersive pack (`world::pack`): on for this world (`menu::Info::pack`), the cloth spread on the ground (if any), whether the
    /// open inventory screen is that cloth (the vanilla panel otherwise), and the seconds the "find flat ground" message has left.
    pack_rule: bool,
    /// Cortes Combat (C2, only with `pack_rule`; NOT b1.7.3, UNVERIFIED): ticks until the held item may swing again, and the item
    /// (`None` = the bare hand) that cooldown belongs to: changing item starts it over (`resetOnItemChange`).
    swing_cd: u32,
    swing_held: Option<u16>,
    /// C3: ticks the attack has been held on a mob in reach of a weapon that can charge (`Profile::charge`), and the damage multiplier of the
    /// blow this frame's release delivers (1 = a plain one). NOT b1.7.3, UNVERIFIED.
    charge: u32,
    blow: f32,
    /// C3 fix: a still finger on a mob in reach of a charging weapon while its cooldown runs; the hand rises with the cooldown (`charge_phase`).
    cd_rise: bool,
    /// C5 (NOT b1.7.3, UNVERIFIED): the right button's state: the shotel's parry, the ropera's lunge, the martillo's streak.
    sp: world::combat::special::State,
    eating: Option<Eating>,
    mat: Option<pack::Mat>,
    mat_ui: bool,
    hint: f32,
    /// What `hint` shows: the pack's message (`put_pack`) or the bed's (`use_bed`).
    hint_msg: &'static str,
    /// The pack carried in both hands before it is spread (`Some` = how far it is raised, 0 to 1), and the cloth as it would lie under
    /// the crosshair with whether the ground fits (`pack_tick`): the preview, green or red.
    carry: Option<f32>,
    ghost: Option<(pack::Mat, bool)>,
    /// The immersive screen in the world (`pack::scene`): the camera as it was before the cloth moved it for this frame (put back after),
    /// what lies on the cloth this frame (the touch and the text read it too), the seconds since the cloth finished spreading (the stacks
    /// come up one by one) and how far the cursor stack is lifted (0..1).
    pack_saved: Option<(glam::Vec3, f32, f32, f32)>,
    pack_scene: Option<pack::Scene>,
    pack_age: f32,
    pack_lift: f32,
    /// Where the camera is over the table (a cloth unit: the centre of the view), and where it is gliding to (a tab was tapped).
    pack_focus: (f32, f32),
    pack_goal: Option<(f32, f32)>,
    /// The satchel (`satchel.rs`, the flat bag and the recipe book) while its screen is open, and what the player has seen / pinned in the
    /// book (`world::book::Notes`, the `book` file of the world).
    sat: Option<satchel::Satchel>,
    notes: book::Notes,
    /// The fishing bobber in the water (`EntityPlayer.fishEntity`).
    hook: Option<Hook>,
    /// Furnace tile entities by block position (created on first use), and the time left over from the last 20 Hz tick.
    furnaces: HashMap<(i32, i32, i32), Furnace>,
    furn_acc: f32,
    /// Leftover wall-clock time towards the next tick of the player's model state (`body`).
    body_acc: f32,
    /// Animated water / lava / fire tiles (`render::anim`) and the time left over from their last 20 Hz tick.
    anim: crate::render::anim::Anim,
    anim_acc: f32,
    /// CPU seconds spent in `render` (without the swap-chain wait) since the last FPS log line.
    cpu_acc: f32,
    /// Dynamic resolution (`gpu::context::Drs`), fed by `render_frame`; off with `dynamicRes:false`.
    drs: Drs,
    /// Chest tile entities by block position (created on first open, or by a dungeon's populate).
    chests: HashMap<chest::Pos, Chest>,
    /// Paintings on walls, sign texts by block position (`TileEntitySign`) and the sign being edited (`GuiEditSign`); saved in `decor`.
    paintings: Vec<Painting>,
    signs: HashMap<decor::Pos, Text>,
    sign_edit: Option<Edit>,
    /// The finger gesture on the open screen, to tell a tap from a spread.
    gesture: Option<Gesture>,
    /// Health, air and fire; and where a dead player comes back (the first spawn: no beds yet).
    vitals: Vitals,
    spawn: glam::Vec3,
    /// Asleep: the bed's cell (the half with bit 8) and the ticks slept (`EntityPlayer.sleepTimer`, 100 = fully asleep).
    sleep: Option<(i32, i32, i32, u32)>,
    /// The bed a respawn goes to while it still stands (`playerSpawnCoordinate`).
    bed_spawn: Option<(i32, i32, i32)>,
    /// Where this world is saved (M7) and the play time since the last autosave.
    dir: PathBuf,
    save_acc: f32,
    /// The world's maps (`world::map`, `<world>/data`), the (map id, version) whose pixels are in the HUD texture, and the markers of the
    /// map in hand (`None` = no map in hand: nothing is drawn).
    maps: world::map::Maps,
    map_shown: (u16, u32),
    map_held: Option<Vec<world::map::Marker>>,
}

/// A press on an open screen: where it began, whether it became a spread (a drag over slots with a stack on the
/// cursor, one item into each slot it enters) and which slots it already filled.
struct Gesture {
    start: Option<craft::SlotId>,
    spread: bool,
    visited: Vec<craft::SlotId>,
    /// On the pack's cloth: the press began on a 3D button (a tap, nothing else), or on empty cloth or ground (`grab` = the cloth unit
    /// under the finger: a drag pans the camera so it stays under it; `moved` = it was dragged, so lifting is not a tap).
    btn: bool,
    grab: Option<(f32, f32)>,
    moved: bool,
    down: (f32, f32),
    /// Seconds the finger has been down (a press on the satchel's result this long crafts all), and on the book's list (or the recipe card: `true`) the scroll it began at.
    age: f32,
    list: Option<(bool, f32)>,
}

/// Pixels a finger must travel before a press on empty cloth is a pan and not a tap.
const PAN_SLOP: f32 = 12.0;

impl App {
    async fn init(native_ptr: *mut c_void, width: u32, height: u32, dir: PathBuf) -> Result<Self, String> {
        // The options come first: the MSAA count is baked into every pipeline `Gpu` and what follows build.
        let opts = Settings::load(&dir);
        crate::gpu::context::set_error_trace(opts.gpu_error_trace);
        crate::gpu::context::set_frame_latency(u32::from(opts.frame_latency));
        crate::render::present::set_msaa_want(u32::from(opts.msaa));
        crate::render::present::set_post_want(u32::from(opts.post));
        crate::render::present::set_fxaa_want(u32::from(opts.fxaa));
        crate::gles::set_want(opts.renderer);
        let be = Backend::new(opts.renderer == 2, native_ptr, width, height).await?;
        log::info!("renderer: {}", be.gpu_info());

        // No world yet: the title screen is up (`enter_world` loads one, and does it again for `LAST_WORLD` below).
        let start = glam::Vec3::ZERO;
        let chunks = ChunkManager::new_typed(0, RENDER_DIST, WorldType::Vanilla);
        let mut camera = FirstPersonCamera::spawn_at(0.0, 80.0, 0.0);
        // spawn_at defaults to aspect 1.0 and resize() only runs when the size changes, so
        // without this the 3D view is squashed horizontally onto the real screen shape.
        camera.aspect = width as f32 / height as f32;
        let player = Player { pos: start, vel: glam::Vec3::ZERO, on_ground: false };

        // M12: HUD pipeline + touch state machine. Surface dimensions match
        // the window we just initialised against.
        let touch = TouchUi::new(width, height);
        let outline = Outline::new(be.gfx());
        let item_mesh = ItemMesh::new(be.gfx());
        let glow = GlowMesh::new(be.gfx());
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as i64);

        let mut app = Self {
            be, world_type: WorldType::Vanilla, lod: None, seed: 0, root: dir.clone(), opts, pause: None, lod_on: false, menu: Some(Menu::new(&dir)),
            chunks, camera,
            player,
            last_frame: Instant::now(),
            frames: 0,
            fps_last: Instant::now(),
            fps_recent: 0,
            fps_avg: 0.0,
            cpu_ms: 0,
            gpu_acc: 0.0,
            gpu_n: 0,
            gpu_tenths: 0,
            gpu_buf: Vec::new(),
            worst_ms: 0,
            worst_acc: 0.0,
            p99_ms: 0,
            times: FrameTimes::default(),
            refresh_hz: 60.0,
            was_paused: false,
            frame_prev: Instant::now(),
            scan: Default::default(),
            motes: Default::default(),
            glow,
            touch,
            hud_verts: Vec::with_capacity(256),
            target: None,
            outline,
            world_ticks: 0.0,
            weather: sky::Weather::new(seed),
            weather_ticks: 0,
            atmos: world::atmos::Atmos::new(),
            fx: Effects::new(seed),
            was_in_water: false,
            sneak_y: 0.0,
            kick: glam::Vec3::ZERO,
            sprint: Sprint::default(),
            sprint_blocked: false,
            tip: (0, 0.0),
            body: render::player::Anim::default(),
            third_saved: None,
            shoulder: render::shoulder::Rig::new(),
            dig: Dig::default(),
            dig_acc: 0.0,
            inv: Inventory::default(),
            drops: Drops::new(seed),
            mobs: Mobs::new(seed),
            boats: boat::Boats::default(),
            carts: minecart::Carts::new(seed),
            detectors: std::collections::HashSet::new(),
            nether: false,
            portal_t: 0.0,
            portal_cd: 0.0,
            ticks: Ticks::new(seed),
            audio: audio::Audio::new(),
            ambient: audio::Ambient::new(seed),
            item_mesh,
            screen: None,
            one_mode: false,
            shift_mode: false,
            pack_rule: false,
            swing_cd: 0,
            swing_held: None,
            charge: 0,
            blow: 1.0,
            eating: None,
            cd_rise: false,
            sp: Default::default(),
            mat: None,
            mat_ui: false,
            pack_saved: None,
            pack_scene: None,
            pack_age: 0.0,
            pack_lift: 0.0,
            pack_focus: pack::tab_focus(0),
            pack_goal: None,
            sat: None,
            notes: book::Notes::default(),
            hint: 0.0,
            hint_msg: "",
            carry: None,
            ghost: None,
            hook: None,
            furnaces: HashMap::new(),
            chests: HashMap::new(),
            paintings: Vec::new(),
            signs: HashMap::new(),
            sign_edit: None,
            furn_acc: 0.0,
            maps: world::map::Maps::default(),
            map_shown: (u16::MAX, 0),
            map_held: None,
            body_acc: 0.0,
            anim: crate::render::anim::Anim::new(),
            anim_acc: 0.0,
            cpu_acc: 0.0,
            drs: Drs::new(crate::gpu::context::START_PCT),
            gesture: None,
            vitals: Vitals::default(),
            spawn: start,
            sleep: None,
            bed_spawn: None,
            dir,
            save_acc: 0.0,
        };
        app.touch.set_menu(true);
        let last = LAST_WORLD.lock().ok().and_then(|l| l.clone());
        if let Some(info) = last.and_then(|f| menu::list(&app.root).into_iter().find(|i| i.folder == f)) {
            app.enter_world(&info);
        }
        app.be.release(); // the GL context goes to the render thread (a no-op for wgpu)
        Ok(app)
    }

    /// The distant terrain of the dimension in `self.dir`: the overworld has it, the Nether does not.
    fn new_lod(&self) -> Option<Lod> {
        // Pre-generation is on from the start: the view's own worker alone left the far tiles empty for minutes (`Lod::toggle_pregen` stops it).
        (!self.nether).then(|| {
            let mut l = Lod::new(self.seed, self.world_type, Some(self.dir.join("lod")));
            l.toggle_pregen();
            l
        })
    }

    /// The overworld's folder, whichever dimension this is.
    fn overworld_dir(&self) -> PathBuf {
        if self.nether { self.dir.parent().map_or_else(|| self.dir.clone(), |p| p.to_path_buf()) } else { self.dir.clone() }
    }

    /// Back to the overworld's folder without moving anyone (a reset builds the world anew).
    fn leave_nether(&mut self) {
        if self.nether {
            self.dir = self.overworld_dir();
            self.nether = false;
            self.mobs.nether = false;
            let _ = std::fs::write(self.dir.join("dim"), "overworld");
        }
    }

    /// Portal travel (`Minecraft.usePortal`, `Teleporter`): save this dimension, load the other one (a new `ChunkManager` on its own
    /// folder; `<root>/dim` remembers it), arrive at the nearest portal to the player's spot x 8 or / 8 (one is built when none
    /// is loaded there). `respawn`: no portal, the spawn point (death in the Nether). What belongs to a dimension (mobs, items on
    /// the ground, block updates, vehicles, furnaces, chests, spawners, signs, paintings) is swapped for what that dimension's own
    /// `level` and `decor` files held; the player, inventory, health and bed stay. ponytail: `preload` runs on the render thread.
    fn enter(&mut self, nether: bool, respawn: bool) {
        if self.nether == nether {
            return;
        }
        self.save(usize::MAX);
        let ow = self.overworld_dir();
        let _ = std::fs::write(ow.join("dim"), if nether { "nether" } else { "overworld" });
        let (old, k) = (self.player.pos, if nether { 0.125 } else { 8.0 });
        (self.nether, self.dir) = (nether, if nether { ow.join("nether") } else { ow });
        let to = if respawn { self.spawn } else { glam::Vec3::new(old.x * k, old.y, old.z * k) };
        self.chunks = chunk_manager(self.seed, self.world_type, nether).with_dir(self.dir.clone()); // the old one's workers stop on drop
        self.lod = self.new_lod();
        self.chunks.preload(chunk_coord(to.x), chunk_coord(to.z));
        self.close_sign();
        (self.carry, self.mat) = (None, None); // the cloth stays in the dimension it was spread in; `pack_tick` closes its screen
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as i64);
        (self.mobs, self.drops, self.ticks) = (Mobs::new(seed), Drops::new(seed), Ticks::new(seed));
        self.mobs.nether = nether;
        self.boats = boat::Boats::default();
        self.carts = minecart::Carts::default();
        (self.detectors, self.paintings, self.signs) = (Default::default(), Vec::new(), HashMap::new());
        (self.furnaces, self.chests, self.fx.spawners, self.furn_acc) = (HashMap::new(), HashMap::new(), HashMap::new(), 0.0);
        (self.target, self.dig, self.dig_acc) = (None, Dig::default(), 0.0);
        if let Some(l) = std::fs::read(self.dir.join("level")).ok().and_then(|b| Level::decode(&b)) {
            self.furnaces = l.furnaces.into_iter().collect();
            self.chests = l.chests.into_iter().collect();
            self.fx.spawners = l.spawners.into_iter().map(|(p, kind, delay)| (p, Spawner::new(kind, delay))).collect();
            self.drops.items = l.drops.into_iter().map(|(p, s, age)| items::ItemEntity::resting(p, s, age)).collect();
        }
        if let Some((p, s)) = std::fs::read(self.dir.join("decor")).ok().and_then(|b| decor::decode(&b)) {
            (self.paintings, self.signs) = (p, s.into_iter().collect());
        }
        let mut at = to;
        if !respawn {
            let want = (to.x as f64, to.y as f64, to.z as f64);
            let mut found = portal::find(&self.chunks, &self.chunks.find(portal::PORTAL), want);
            if found.is_none() {
                portal::make(&mut self.chunks, want, &mut crate::world::gen::noise::JavaRandom::new(seed));
                found = portal::find(&self.chunks, &self.chunks.find(portal::PORTAL), want);
            }
            if let Some((x, y, z)) = found {
                at = glam::Vec3::new(x as f32, y as f32 + physics::HALF.y + 0.01, z as f32);
            }
        }
        self.player = Player { pos: at, vel: glam::Vec3::ZERO, on_ground: false };
        self.camera.pos = at + glam::Vec3::new(0.0, EYE_HEIGHT, 0.0);
        (self.portal_t, self.portal_cd) = (0.0, 0.5);
        log::info!("portal: now in the {} at {at:?}", if nether { "Nether" } else { "overworld" });
    }

    /// Load a world of the list (or just made): from its `level` and chunk files where it has them, else a new one of its seed.
    fn enter_world(&mut self, info: &menu::Info) {
        self.dir = info.dir(&self.root);
        (self.seed, self.world_type) = (info.seed, info.ty);
        self.pack_rule = info.pack;
        world::combat::rules::set(info.pack); // Cortes Combat lives only in an Immersive Pack world
        (self.swing_cd, self.charge, self.eating, self.cd_rise) = (0, 0, None, false);
        self.sp = Default::default();
        // The world resumes in the dimension it was left in (`<world>/dim`).
        self.nether = std::fs::read_to_string(self.dir.join("dim")).is_ok_and(|s| s.trim() == "nether");
        if self.nether {
            self.dir = self.dir.join("nether");
        }
        self.reset_world(false);
        self.menu = None;
        self.touch.set_menu(false);
        (self.last_frame, self.frame_prev, self.fps_last) = (Instant::now(), Instant::now(), Instant::now()); // the load and the menu before it are not a frame
        (self.frames, self.cpu_acc, self.fps_avg) = (0, 0.0, 0.0); // a new world starts the FPS average again
        if let Ok(mut l) = LAST_WORLD.lock() {
            *l = Some(info.folder.clone());
        }
    }

    /// The pause menu's "Save and quit to title" (`GuiIngameMenu`).
    fn quit_to_title(&mut self) {
        self.close_sign();
        self.save(usize::MAX);
        self.chunks = ChunkManager::new_typed(0, RENDER_DIST, WorldType::Vanilla); // the world's workers stop on drop
        self.lod = None;
        self.menu = Some(Menu::new(&self.root));
        self.pause = None;
        self.touch.set_menu(true);
        if let Ok(mut l) = LAST_WORLD.lock() {
            *l = None;
        }
    }

    /// Put everything back for the world in `self.dir` (`self.seed`, `self.world_type`): `wipe` (the TEMPORARY pause-menu reset button)
    /// deletes the save folder first and makes a new world: terrain without edits, spawn point, hotbar and inventory, health, time,
    /// weather, mobs, drops, furnaces, block updates; otherwise it resumes from the `level`, chunk and `decor` files, a new world
    /// where there are none. An open screen's stacks are discarded, not dropped.
    /// ponytail: `preload` runs on the render thread (a second or two of freeze after the "Loading level" frame).
    fn reset_world(&mut self, wipe: bool) {
        if wipe {
            self.leave_nether();
        }
        if self.screen.take().is_some() {
            self.touch.set_screen(false);
        }
        (self.mat, self.mat_ui, self.hint, self.carry, self.ghost) = (None, false, 0.0, None, None);
        (self.pack_scene, self.pack_age, self.pack_lift) = (None, 0.0, 0.0);
        (self.pack_focus, self.pack_goal) = (pack::tab_focus(0), None);
        self.sat = None;
        self.close_sign();
        (self.paintings, self.signs) = (Vec::new(), HashMap::new());
        self.boats = boat::Boats::default();
        self.carts = minecart::Carts::default();
        self.detectors.clear();
        if wipe {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as i64);
        let level = if wipe { None } else { std::fs::read(self.dir.join("level")).ok().and_then(|b| Level::decode(&b)) };
        self.chunks = chunk_manager(self.seed, self.world_type, self.nether).with_dir(self.dir.clone()); // the old one's workers stop on drop
        self.lod = self.new_lod();
        // The terrain around the player must be real before the first frame, so those chunks are loaded (or generated and
        // populated) here; everything further out streams in on the worker threads.
        let (pcx, pcz) = level.as_ref().map_or((0, 0), |l| (chunk_coord(l.pos.x), chunk_coord(l.pos.z)));
        self.chunks.preload(pcx, pcz);
        self.spawn = level.as_ref().map_or_else(|| find_spawn(&self.chunks), |l| l.spawn);
        let s = level.as_ref().map_or(self.spawn, |l| l.pos);
        log::info!("save: {} in {}", if level.is_some() { "resuming" } else { "new world" }, self.dir.display());
        // Light and mesh the area around the player now, so the first frame already shows terrain.
        let warm = Instant::now();
        while self.chunks.meshed() < 24 && warm.elapsed() < Duration::from_secs(8) {
            self.chunks.update(self.be.gfx(), self.be.origins(), chunk_coord(s.x), chunk_coord(s.z));
            std::thread::sleep(Duration::from_millis(1));
        }
        log::info!("chunks: {} meshed after {:?}", self.chunks.meshed(), warm.elapsed());
        (self.sleep, self.bed_spawn) = (None, None);
        self.player = Player { pos: s, vel: glam::Vec3::ZERO, on_ground: false };
        let aspect = self.camera.aspect;
        self.camera = FirstPersonCamera::spawn_at(s.x, s.y + EYE_HEIGHT, s.z);
        self.camera.aspect = aspect;
        (self.world_ticks, self.weather_ticks, self.weather) = (0.0, 0, sky::Weather::new(seed));
        self.atmos.reset();
        self.fx = Effects::new(seed);
        self.sprint = Sprint::default();
        (self.tip, self.dig, self.dig_acc, self.target) = ((0, 0.0), Dig::default(), 0.0, None);
        (self.inv, self.drops, self.mobs, self.ticks) = (Inventory::default(), Drops::new(seed), Mobs::new(seed), Ticks::new(seed));
        (self.furnaces, self.furn_acc, self.vitals, self.save_acc) = (HashMap::new(), 0.0, Vitals::default(), 0.0);
        self.chests.clear();
        self.mobs.nether = self.nether;
        self.touch.hotbar_slot = 0;
        if let Some(l) = level {
            self.restore(l);
        }
        if let Some((p, s)) = std::fs::read(self.dir.join("decor")).ok().and_then(|b| decor::decode(&b)).filter(|_| !wipe) {
            (self.paintings, self.signs) = (p, s.into_iter().collect());
        }
        // The recipe book's notes (what was held, the pinned recipe): a new world starts with none.
        self.notes = std::fs::read(self.dir.join("book")).ok().and_then(|b| book::Notes::decode(&b)).filter(|_| !wipe).unwrap_or_default();
        self.maps = world::map::Maps::open(Some(self.overworld_dir().join("data")));
        (self.map_shown, self.map_held) = ((u16::MAX, 0), None);
        log::info!("world of seed {:x} ({}), spawn {:?}", self.seed, self.world_type.name(), self.spawn);
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
        self.fx.spawners = l.spawners.into_iter().map(|(p, kind, delay)| (p, Spawner::new(kind, delay))).collect();
        self.bed_spawn = l.bed;
        self.drops.items = l.drops.into_iter().map(|(p, s, age)| items::ItemEntity::resting(p, s, age)).collect();
    }

    /// Write the world to disk: the level file, then up to `chunks` unsaved chunks (`usize::MAX` = all). An open
    /// screen is closed first, so the cursor stack and the grid become dropped items, which are saved.
    // ponytail: runs on the render thread; the level file is ~1 KB and the chunk writes are capped per autosave.
    fn save(&mut self, chunks: usize) {
        if self.menu.is_some() {
            return; // no world is loaded
        }
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
            chests: self.chests.iter().filter(|(p, _)| p.0 != i32::MIN).map(|(&p, &c)| (p, c)).collect(),
            spawners: self.fx.spawners.iter().map(|(&p, s)| (p, s.kind, s.delay)).collect(),
            bed: self.bed_spawn,
            drops: self.drops.items.iter().map(|e| (e.pos, e.stack, e.age)).collect(),
        };
        if let Err(e) = save::write(&self.dir.join("level"), &level.encode()) {
            log::warn!("save: level file: {e}");
        }
        let signs: Vec<_> = self.signs.iter().map(|(&p, t)| (p, t.clone())).collect();
        if let Err(e) = save::write(&self.dir.join("decor"), &decor::encode(&self.paintings, &signs)) {
            log::warn!("save: decor file: {e}");
        }
        if let Err(e) = save::write(&self.dir.join("book"), &self.notes.encode()) {
            log::warn!("save: book file: {e}");
        }
        self.settle_maps();
        self.maps.save_all();
        self.chunks.flush(chunks);
    }

    /// `ItemMap.onCreated` for the maps just crafted, then `ItemMap.onUpdate` for every map in the main inventory (`InventoryPlayer.decrementAnimations`):
    /// the arrow on it, and for the one in hand the repaint of this tick's strip (`MapData::scan`) and the upload of its pixels when they changed.
    fn map_tick(&mut self) {
        use world::map::MAP;
        self.settle_maps();
        let dim: i8 = if self.nether { -1 } else { 0 };
        let at = (self.player.pos.x as f64, self.player.pos.z as f64);
        let (slot, alive, yaw) = (self.touch.hotbar_slot, !self.vitals.dead(), self.camera.yaw.to_degrees() + 180.0);
        let spawn = (self.spawn.x.floor() as i32, self.spawn.z.floor() as i32);
        let (chunks, no_sky) = (&self.chunks, self.nether);
        let mut held = None;
        for i in 0..items::MAIN {
            let Some(mut st) = self.inv.slots[i].filter(|s| s.id == MAP) else { continue };
            let d = self.maps.data_for(&mut st, spawn, dim);
            d.track(alive, dim, at, yaw);
            if i == slot {
                d.scan(dim, no_sky, at, |x, z| chunks.column(x, z));
                held = Some(d.markers.clone());
                if self.map_shown != (st.damage, d.version) {
                    self.be.upload_map(&d.rgba());
                    self.map_shown = (st.damage, d.version);
                }
            }
            self.inv.slots[i] = Some(st);
        }
        self.map_held = held;
    }

    /// `ItemMap.onCreated` for the maps taken from a crafting result since the last call (`map::fresh`): in the bag or on the cursor.
    fn settle_maps(&mut self) {
        let dim: i8 = if self.nether { -1 } else { 0 };
        let at = (self.player.pos.x as f64, self.player.pos.z as f64);
        for s in self.inv.slots.iter_mut().flatten().chain(self.screen.iter_mut().filter_map(|s| s.cursor.as_mut())) {
            self.maps.settle(s, at, dim);
        }
    }

    fn resize(&mut self, w: u32, h: u32) {
        self.be.resize(w, h);
        self.camera.aspect = w as f32 / h as f32;
        self.touch.ensure_layout(w, h);
    }

    fn step_frame(&mut self) {
        let now = Instant::now();
        let mut dt = now.duration_since(self.last_frame).as_secs_f32();
        // The wall-clock frame time, for what a person times with a finger or an eye (hold to dig, dig ticks, the arm swing): `dt` below
        // is cut at 1/30 s, which is slow motion under 30 fps (at 10 fps a 0.2 s hold took 0.6 s and the swing 1.2 s).
        let real_dt = dt.min(0.25);
        if dt > 1.0 / 30.0 { dt = 1.0 / 30.0; }
        if dt < 0.0 { dt = 0.0; }
        self.last_frame = now;
        self.pad_sync(dt);
        self.touch.pad.watch_health(self.vitals.health);
        // Zoomify: hold the zoom button for 4x (the vertical tangent / 4), eased; the look drag slows with the field of view.
        // Retro Sprint: the field of view x 1.15 while sprinting (eased), and the zoom starts from it.
        self.sprint.fov_step(dt);
        let base = 70_f32.to_radians() * self.sprint.fov;
        let target = if self.touch.zooming() { 2.0 * ((base * 0.5).tan() / ZOOM).atan() } else { base };
        self.camera.fov_y += (target - self.camera.fov_y) * (1.0 - (-14.0 * dt).exp());
        // Look drag accumulated by the touch UI since the last frame. Drained
        // even when paused so a drag started before pausing doesn't replay.
        let (look_dx, look_dy) = self.touch.take_look();
        let (pad_yaw, pad_pitch) = self.touch.take_pad_look();
        // Pause: freeze the world but keep the timer warm so un-pause doesn't
        // produce a giant dt step (which would either teleport the player or
        // kill their fall).
        if self.touch.paused {
            if self.pause.is_none() {
                self.pause = Some(Pause::new());
            } else if self.pause.as_ref().is_some_and(|p| !p.saved) {
                // `GuiIngameMenu` shows "Saving level..": the world is written on the menu's second frame, so the first one (the tap that
                // opened it) is on screen before the encode and the file writes, not after them.
                if let Some(p) = self.pause.as_mut() {
                    p.saved = true;
                }
                if self.screen.is_none() {
                    self.save(AUTOSAVE_CHUNKS);
                }
            }
            let (w, h) = { let (w, h) = self.be.size(); (w as f32, h as f32) };
            let info = self.pause_info();
            let mut asked = Vec::new();
            for ev in self.touch.take_screen_events() {
                if let Some(p) = self.pause.as_mut() {
                    asked.extend(p.event(ev, w, h, &mut self.opts, &info));
                }
            }
            for a in asked {
                match a {
                    PauseEv::Resume => self.touch.resume(),
                    PauseEv::Quit => {
                        self.quit_to_title();
                        return;
                    }
                    PauseEv::Pregen => {
                        if let Some(l) = self.lod.as_mut() {
                            l.toggle_pregen();
                        }
                    }
                    PauseEv::Fly => self.touch.flying = !self.touch.flying,
                    PauseEv::Info => {
                        self.opts.debug_screen = !self.opts.debug_screen;
                        self.opts.save();
                    }
                    PauseEv::LodDebug => {
                        if let Some(l) = self.lod.as_mut() {
                            l.cycle_debug();
                        }
                    }
                }
            }
        }
        if self.touch.paused {
            // Player physics is still ticked so they don't fall through the
            // world if pause was opened mid-air, but gravity is zeroed.
            self.player.vel = glam::Vec3::ZERO;
            self.camera.pos = self.player.pos + glam::Vec3::new(0.0, EYE_HEIGHT - self.sneak_y, 0.0);
            return;
        }
        self.pause = None;
        // Four seconds in a portal: the other dimension (`EntityPlayerSP.onLivingUpdate`).
        if self.portal_t >= 1.0 {
            let p = self.audio.rnd() * 0.4 + 0.8;
            self.audio.play_fx("portal.travel", 1.0, p);
            self.enter(!self.nether, false);
            return;
        }
        // The death screen's resume button was pressed: come back at the spawn point with full health.
        if self.vitals.dead() {
            self.vitals = Vitals::default();
            self.sprint.stop();
            self.enter(false, true); // `Minecraft.respawn`: always in the overworld
            self.player.pos = self.respawn_pos();
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
        // Day/night: time only runs while playing; the shader reads the sky-light level from a uniform, so the meshes are not rebuilt.
        self.world_ticks += dt as f64 * 20.0;
        while self.weather_ticks < self.world_ticks as u64 {
            self.weather.tick();
            self.weather_ticks += 1;
        }
        let angle = sky::celestial_angle(self.world_ticks as u64, 1.0);
        let sub = sky::skylight_subtracted(angle, self.weather.rain(1.0), self.weather.thunder(1.0));
        self.chunks.set_sky_sub(sub);
        // NOT b1.7.3 (`biomeRules`): the oak foliage of the two forests follows the season; a change restarts the meshes (once an hour).
        let season = if self.opts.biome_rules && !self.nether { world::biomelife::season(self.world_ticks as u64) } else { world::biomelife::Season::Summer };
        if world::biomelife::set_season(season) {
            self.chunks.restart_meshes();
        }
        // Asleep, the jump button is "leave bed" (`GuiSleepMP`); the view and the body stay put.
        if self.sleep.is_some() && self.touch.jumping() {
            self.wake(true);
        }
        // On a saddled pig the jump button is "get off" (PE dismounts with sneak; b1.7.3 has no sneak-dismount, so the jump button keeps it) and the body goes
        // wherever the pig goes (`updateRidden`: no motion of its own).
        if self.touch.jumping() {
            self.mobs.list.iter_mut().for_each(|m| m.ridden = false);
            self.boats.list.iter_mut().for_each(|b| b.ridden = false);
            self.carts.list.iter_mut().for_each(|c| c.ridden = false);
        }
        let riding = self.riding();
        let asleep = self.sleep.is_some();
        if !asleep {
            let k = LOOK_SENS * (self.camera.fov_y / self.sprint.fov * 0.5).tan() / (70_f32.to_radians() * 0.5).tan() * self.opts.look_scale(); // the sprint's wider view does not speed the look up
            self.camera.add_yaw(look_dx * k);
            self.camera.add_pitch(look_dy * k * if self.opts.invert_mouse { -1.0 } else { 1.0 });
            // The right stick has its own sensitivity (Controls > Controller); only the zoom narrows it.
            let z = (self.camera.fov_y / self.sprint.fov * 0.5).tan() / (70_f32.to_radians() * 0.5).tan();
            self.camera.add_yaw(pad_yaw * z);
            self.camera.add_pitch(pad_pitch * z);
        }
        // Analog move stick: rotate the camera-frame (fwd, side) by yaw into
        // world-space velocity. Set every frame (zero when the stick is idle),
        // otherwise the player keeps sliding after the finger lifts.
        let (mut fwd, mut side) = if asleep || riding { (0.0, 0.0) } else { self.touch.move_input };
        // `MovementInputFromOptions`: sneaking scales both axes by 0.3. `EntityPlayerSP.isSneaking` is false in bed.
        let sneaking = self.touch.sneak && !asleep;
        if sneaking {
            fwd *= 0.3;
            side *= 0.3;
        }
        let yaw = self.camera.yaw;
        // forward = -Z when yaw = 0; rotate by yaw around +Y.
        self.player.vel.x = (fwd * yaw.sin() + side * yaw.cos()) * MOVE_SPEED;
        self.player.vel.z = (-fwd * yaw.cos() + side * yaw.sin()) * MOVE_SPEED;
        // Retro Sprint (`LivingEntityMixin`): the sprint pushes along the facing on top of the walk, and a jump on the ground adds the jump push.
        if !asleep && !riding {
            if self.touch.jumping() && self.player.on_ground && !self.was_in_water {
                self.sprint.jump();
            }
            let push = self.sprint.add();
            self.player.vel.x += yaw.sin() * push;
            self.player.vel.z -= yaw.cos() * push;
        }
        // Stamina's slowdown: x0.9 while locked, x0.85 more while sprinting on little (it multiplies the walk, the sprint's push and the jump's).
        let slow = self.vitals.stamina.speed(self.sprint.on()) * self.sp.slow(); // C5: the parry's blade out slows the walk (x0.4)
        self.player.vel.x *= slow;
        self.player.vel.z *= slow;
        // An explosion's push rides on top of the walk and the sprint (and is neither slowed by stamina nor kept in bed, on a ride or in death).
        if asleep || riding || self.vitals.dead() {
            self.kick = glam::Vec3::ZERO;
        }
        self.player.vel.x += self.kick.x;
        self.player.vel.z += self.kick.z;
        // C5: a lunge drives the body along its line over the walk, the sprint and any push (`Lunge`: the step replaces the player's own).
        if let Some(l) = &self.sp.lunge {
            (self.player.vel.x, self.player.vel.z) = (l.dir.x * world::combat::special::LUNGE_SPEED, l.dir.z * world::combat::special::LUNGE_SPEED);
        }
        let (vx0, vz0) = (self.player.vel.x, self.player.vel.z);
        // Plants are drawn but not solid yet (collision shapes come with M14), so physics reads them as air.
        let get = |x: i32, y: i32, z: i32| self.chunks.block(x, y, z).map(|b| if is_plant(b) { 0 } else { self.chunks.physics_at(x, y, z, b) });
        // The held jump button jumps (8.4 m/s, the b1.7.3 velocity) or swims up; fluids (ids 8..=11) are not solid.
        let (y0, grounded) = (self.player.pos.y, self.player.on_ground);
        if sneaking && !riding {
            physics::sneak(&mut self.player, physics::HALF, dt, &get); // no walking off an edge, a hold on ladders
        }
        // TEMPORARY fly mode (pause menu button): jump climbs, sneak descends, otherwise hover; the frame's gravity is added back so `step`
        // leaves the speed as set. ponytail: in water the drag still sags it a little; no fall damage while on.
        let flying = self.touch.flying && !asleep && !riding;
        if flying {
            self.player.vel.y = (if self.touch.jumping() { 8.0 } else if sneaking { -8.0 } else { 0.0 }) + 23.0 * dt;
        }
        let (water, lava) = if asleep || riding { (false, false) } else { physics::step(&mut self.player, dt, self.touch.jumping() && !flying, &get) };
        // The push dies like the Java's motion (x0.546 a tick on the ground, x0.91 in the air, x0.8 in water, x0.5 in lava), and a wall that stopped the
        // frame's move stopped it.
        if self.kick != glam::Vec3::ZERO {
            let drag = if lava { 0.5 } else if water { 0.8 } else if self.player.on_ground { 0.546 } else { 0.91f32 };
            self.kick *= drag.powf(dt * 20.0);
            if self.player.vel.x == 0.0 {
                self.kick.x = 0.0;
            }
            if self.player.vel.z == 0.0 {
                self.kick.z = 0.0;
            }
            if self.kick.length_squared() < 0.01 {
                self.kick = glam::Vec3::ZERO;
            }
        }
        // `LivingJumpEvent` (Stamina): a jump that left the ground costs 10; one a ceiling stopped zeroed `vel.y`, so it is free.
        if grounded && !water && !flying && !asleep && !riding && self.touch.jumping() && self.player.vel.y > 0.0 {
            self.vitals.stamina.jump();
        }
        // `horizontalCollision`: `step` zeroes the velocity of the axis a wall stopped (a stepped-up slab keeps it).
        self.sprint_blocked |= !asleep && !riding && ((vx0 != 0.0 && self.player.vel.x == 0.0) || (vz0 != 0.0 && self.player.vel.z == 0.0));
        // `EntityPlayerSP.onLivingUpdate` sets `ySize` 0.2 while sneaking and `moveEntity` multiplies it by 0.4 every tick: the eye hangs 0.08
        // low (1.54 up), and it comes back by the same x0.4 a tick once the button is released.
        self.sneak_y = if sneaking { 0.08 } else { self.sneak_y * 0.4f32.powf(dt * 20.0) };
        self.camera.pos = self.player.pos + glam::Vec3::new(0.0, EYE_HEIGHT - self.sneak_y, 0.0);
        let e = self.camera.pos;
        let eye_in_water = matches!(self.chunks.block_loaded(e.x.floor() as i32, e.y.floor() as i32, e.z.floor() as i32), Some(8 | 9));
        self.vitals.armor = craft::armor_value(&self.inv.slots[items::MAIN..]);
        // `Entity.isEntityInsideOpaqueBlock` at the head (`EntityPlayer.getEyeHeight` is 0.12 over the eye): 1 damage a tick in a wall.
        let in_wall = effects::inside_opaque(&|x, y, z| self.chunks.block(x, y, z), self.player.pos.x, self.player.pos.y + 0.84, self.player.pos.z, 0.6);
        let ladder = !asleep && physics::on_ladder(self.player.pos, physics::HALF, &get);
        // `BlockCactus.onEntityCollidedWithBlock`: 1 damage every frame the box touches a cactus cell (the 10 tick damage window paces it).
        if !asleep && physics::touching(self.player.pos, physics::HALF, &get, 81) {
            self.vitals.hurt(1);
        }
        // `BlockPortal.onEntityCollidedWithBlock` -> `Entity.setInPortal`: inside a portal the timer fills (a quarter a second, drains
        // at 1 a second outside); the cooldown after an arrival is held while the player still stands in the pane.
        if !asleep && !riding && physics::touching(self.player.pos, physics::HALF, &get, 90) {
            if self.portal_cd > 0.0 {
                self.portal_cd = 0.5;
            } else {
                if self.portal_t == 0.0 {
                    let p = self.audio.rnd() * 0.4 + 0.8;
                    self.audio.play_fx("portal.trigger", 1.0, p); // `EntityPlayerSP`: the timer starts
                }
                self.portal_t += 0.25 * dt;
            }
        } else {
            self.portal_t = (self.portal_t - dt).max(0.0);
            self.portal_cd = (self.portal_cd - dt).max(0.0);
        }
        self.vitals.update(dt, Env { dy: self.player.pos.y - y0, on_ground: self.player.on_ground || flying, water, lava, ladder, eye_in_water, eye_y: e.y, in_wall });
        // Entering water splashes (`Entity.onEntityUpdate`); running out of air pops 8 bubbles round the eye (`EntityLiving.onEntityUpdate`).
        let motion = self.player.vel / 20.0; // blocks per tick
        if water && !self.was_in_water {
            // `Entity.onEntityUpdate`: `random.splash`, as loud as the fall in is fast (`sqrt(vx² * 0.2 + vy² + vz² * 0.2) * 0.2`, at most 1).
            let v = (((motion.x * motion.x + motion.z * motion.z) * 0.2 + motion.y * motion.y).sqrt() * 0.2).min(1.0);
            let p = 1.0 + (self.audio.rnd() - self.audio.rnd()) * 0.4;
            self.audio.play_here("random.splash", v, p);
            self.fx.splash(glam::Vec3::new(self.player.pos.x, (self.player.pos.y - 0.9).floor() + 1.0, self.player.pos.z), 0.6, motion);
        }
        self.was_in_water = water;
        if std::mem::take(&mut self.vitals.drowned) {
            self.fx.breath(e, motion);
        }
        // `random.hurt` for a fresh hit (and the death), then a hard landing's step sound at half volume and 3/4 pitch (`EntityLiving.fall`).
        if std::mem::take(&mut self.vitals.cry) {
            let p = 1.0 + (self.audio.rnd() - self.audio.rnd()) * 0.2;
            self.audio.play_here("random.hurt", 1.0, p);
        }
        // `Entity.moveEntity`: a burning body in water: "random.fizz" at 0.7, pitch `1.6 + (r - r) * 0.4`.
        if std::mem::take(&mut self.vitals.fizz) {
            let p = 1.6 + (self.audio.rnd() - self.audio.rnd()) * 0.4;
            self.audio.play_here("random.fizz", 0.7, p);
        }
        if std::mem::take(&mut self.vitals.thud) {
            let below = self.chunks.block_loaded(self.player.pos.x.floor() as i32, (self.player.pos.y - 1.1).floor() as i32, self.player.pos.z.floor() as i32).unwrap_or(0);
            let (key, _, pitch) = audio::STEP[audio::step_of(below)];
            if below > 0 {
                self.audio.play_here(key, 0.5, pitch * 0.75);
            }
        }
        // `InventoryPlayer.damageArmor`: every worn piece takes the damage that was dealt (before armor).
        let wear = std::mem::take(&mut self.vitals.wear) as u16;
        (items::MAIN..items::SLOTS).filter(|_| wear > 0).for_each(|i| self.inv.damage(i, wear));
        if self.vitals.dead() {
            self.die();
            return;
        }
        // Dropped items fall, settle and get picked up (before the target check below, which can return early).
        self.drops.tick(dt, &|x: i32, y: i32, z: i32| self.chunks.block(x, y, z).map(|b| self.chunks.physics_at(x, y, z, b)), self.player.pos, &mut self.inv);
        for at in std::mem::take(&mut self.drops.pops) {
            // `EntityItem.onCollideWithPlayer` (`EntityArrow`'s is the same call): at the item, volume 0.2, pitch `((r - r) * 0.7 + 1) * 2`.
            let pitch = ((self.audio.rnd() - self.audio.rnd()) * 0.7 + 1.0) * 2.0;
            self.audio.play("random.pop", at, 0.2, pitch);
        }
        let light = |x: i32, y: i32, z: i32| self.chunks.light(x, y, z);
        let biome = |x: i32, z: i32| self.chunks.biome_at(x, z);
        let sight = if self.opts.biome_rules && !self.nether { world::biomelife::sight(self.atmos.density()) } else { 1.0 };
        let ctx = Ctx { get: &get, light: &light, day: sub < 4, player: self.player.pos, eye: self.camera.pos, biome: &biome, sight };
        self.mobs.difficulty = self.opts.difficulty;
        self.mobs.smart = self.opts.animal_ai;
        // What a fireball can hit besides mobs, the player and other fireballs (`canBeCollidedWith`): boats, carts, paintings, a lit TNT (0.98 box on its middle).
        self.mobs.targets.clear();
        self.mobs.targets.extend(self.boats.list.iter().enumerate().map(|(i, b)| (b.pos - boat::HALF, b.pos + boat::HALF, Strike::Boat(i))));
        self.mobs.targets.extend(self.carts.list.iter().enumerate().map(|(i, c)| (c.pos - minecart::HALF, c.pos + minecart::HALF, Strike::Cart(i))));
        self.mobs.targets.extend(self.paintings.iter().enumerate().map(|(i, p)| { let (lo, hi) = p.bounds(); (lo, hi, Strike::Painting(i)) }));
        self.mobs.targets.extend(self.ticks.falling.iter().filter(|f| f.id == 46 && f.mv.is_none()).map(|f| (f.pos - glam::Vec3::splat(0.49), f.pos + glam::Vec3::splat(0.49), Strike::Tnt)));
        let evs = self.mobs.update(dt, &ctx, &mut self.drops);
        self.struck(&evs);
        for e in evs {
            match e {
                Ev::Strike(_) => {} // done by `struck`, ahead of the rest (the blasts below shift the lists the indices point into)
                // `EntityPlayer.attackEntityFrom`: Peaceful 0, Easy d / 3 + 1, Hard d * 3 / 2.
                Ev::Hurt(d, from) => {
                    let d = match self.opts.difficulty { 0 => 0, 1 => d / 3 + 1, 3 => d * 3 / 2, _ => d };
                    // C5: a lunge cannot be hurt (`LungeImmunity`), a parry turns the hit back (`ParryHandler`), then the guard.
                    if d > 0 && self.sp.lunge.is_none() && !self.parried(d, from) && !self.guarded(from) {
                        self.vitals.hurt(d);
                    }
                }
                Ev::Boom(p) => self.explode(p, 3.0, false),
                // `EntityFireball.onUpdate`: `newExplosion(null, x, y, z, 1.0F, true)`, the fire flag on.
                Ev::Burst(p) => self.explode(p, 1.0, true),
                Ev::Smoke(p) => self.fx.puff(p),
                Ev::Snd(name, at, vol, mul, jit) => self.audio.cry(name, at, vol, mul, jit),
                // `EntitySkeleton`'s shot is "random.bow" at 1.0, pitch `1 / (r * 0.4 + 0.8)`; `EntityArrow`'s hit is "random.bowhit" at 1.0, pitch
                // `1.2 / (r * 0.2 + 0.9)` (both NOT in the b1.7.3 index, see `assets/sound/extra`).
                Ev::Bow(at, hit) => {
                    let pitch = if hit { 1.2 / (self.audio.rnd() * 0.2 + 0.9) } else { 1.0 / (self.audio.rnd() * 0.4 + 0.8) };
                    self.audio.play(if hit { "random.bowhit" } else { "random.bow" }, at, 1.0, pitch);
                }
                // NOT b1.7.3 (`animalAi`): a sheep ate; the grass spread covers the dirt again.
                Ev::Graze((x, y, z)) => match (self.chunks.block(x, y, z), self.chunks.block(x, y - 1, z)) {
                    (Some(31), _) => drop(self.chunks.set_block(x, y, z, 0)),
                    (_, Some(2)) => drop(self.chunks.set_block(x, y - 1, z, 3)),
                    _ => {}
                },
                _ => {}
            }
        }
        // Boats (`EntityBoat.onUpdate`, 20 Hz ticks): the rider's own walking paddles the boat (`motion += rider.motion * 0.2`).
        let paddle = match self.boats.ridden() {
            Some(_) => {
                let ((f, s), yaw) = (self.touch.move_input, self.camera.yaw);
                glam::Vec3::new(f * yaw.sin() + s * yaw.cos(), 0.0, -f * yaw.cos() + s * yaw.sin()) * MOVE_SPEED / 20.0
            }
            None => glam::Vec3::ZERO,
        };
        let boat_events = {
            let bget = |x: i32, y: i32, z: i32| self.chunks.block(x, y, z).map(|b| if is_plant(b) { 0 } else { physics_id(b, self.chunks.meta(x, y, z)) });
            self.boats.update(dt, &bget, paddle)
        };
        for e in boat_events {
            match e {
                boat::Event::Drop(s, p) => self.drops.spawn_stack(s, (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32)),
                boat::Event::Snow(c) => {
                    self.chunks.set_block(c.0, c.1, c.2, 0); // `Block.snow` is wiped by the hull
                }
            }
        }
        // Minecarts (`EntityMinecart.onUpdate`): on the rails the cart is its own master; a rider only slows it.
        let cart_events = {
            let raw = |x: i32, y: i32, z: i32| match self.chunks.block(x, y, z) {
                Some(b) => (b, self.chunks.meta(x, y, z)),
                None => (0, 0),
            };
            let cget = |x: i32, y: i32, z: i32| self.chunks.block(x, y, z).map(|b| if is_plant(b) { 0 } else { physics_id(b, self.chunks.meta(x, y, z)) });
            self.carts.update(dt, &minecart::Env { get: &cget, raw: &raw })
        };
        for e in cart_events {
            match e {
                minecart::Event::Drop(s, p) => self.drops.spawn_stack(s, (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32)),
                minecart::Event::Smoke(p) => self.fx.puff(p),
            }
        }
        // `BlockDetectorRail.setStateIfMinecartInteractsWithRail`: on while a cart is over it, off when it has gone.
        for c in self.carts.cells() {
            if self.chunks.block(c.0, c.1, c.2) == Some(28) && self.carts.over_detector(c) {
                self.detectors.insert(c);
                let m = self.chunks.meta(c.0, c.1, c.2);
                if m & 8 == 0 {
                    self.chunks.set_block_meta(c.0, c.1, c.2, 28, m | 8);
                    self.chunks.ping(c.0, c.1 - 1, c.2);
                }
            }
        }
        for c in self.detectors.iter().copied().collect::<Vec<_>>() {
            if self.chunks.block(c.0, c.1, c.2) != Some(28) {
                self.detectors.remove(&c);
            } else if !self.carts.over_detector(c) {
                self.detectors.remove(&c);
                let m = self.chunks.meta(c.0, c.1, c.2);
                if m & 8 != 0 {
                    self.chunks.set_block_meta(c.0, c.1, c.2, 28, m & 7);
                    self.chunks.ping(c.0, c.1 - 1, c.2);
                }
            }
        }
        // `Entity.updateRiderPosition` for a cart: on the cart's centre line, feet 0.8 under the box's centre.
        if let Some(c) = self.carts.ridden() {
            self.player.pos = c.rider_pos(physics::HALF.y);
            self.player.vel = glam::Vec3::ZERO;
            self.player.on_ground = true;
            self.camera.pos = self.player.pos + glam::Vec3::new(0.0, EYE_HEIGHT - self.sneak_y, 0.0);
        }
        // `Entity.updateRiderPosition` for a boat: 0.4 behind the middle, feet 0.8 under the box's centre.
        if let Some(b) = self.boats.ridden() {
            self.player.pos = b.rider_pos(physics::HALF.y);
            self.player.vel = glam::Vec3::ZERO;
            self.player.on_ground = true;
            self.camera.pos = self.player.pos + glam::Vec3::new(0.0, EYE_HEIGHT - self.sneak_y, 0.0);
        }
        // `Entity.updateRiderPosition`: the rider's feet are 0.175 over the pig's (`getMountedYOffset` 0.675, `getYOffset` -0.5).
        if let Some(m) = self.mobs.list.iter().find(|m| m.ridden) {
            let feet = m.body.pos.y - m.half.y;
            self.player.pos = glam::Vec3::new(m.body.pos.x, feet + 0.175 + physics::HALF.y, m.body.pos.z);
            self.player.vel = glam::Vec3::ZERO;
            self.player.on_ground = true; // no fall damage from a pig's hop
            self.camera.pos = self.player.pos + glam::Vec3::new(0.0, EYE_HEIGHT, 0.0);
        }
        // Furnaces burn on the 20 Hz tick, open or not, but only in loaded chunks (vanilla ticks loaded tile entities).
        // The block swaps between unlit 61 and lit 62 when the fire goes on or off.
        // `TextureFX.onTick` every 20 Hz tick (at most 2 per frame: a stall does not queue a burst of texture uploads).
        self.anim_acc = (self.anim_acc + dt).min(2.0 * dig::TICK);
        while self.anim_acc >= dig::TICK {
            self.anim_acc -= dig::TICK;
            self.be.animate(&self.anim.tick());
            // The compass points at the world spawn and the clock shows the sky (`TextureCompassFX`, `TextureWatchFX`); the sprite
            // is rewritten in the atlas (held items, drops) and in the HUD's copy of items.png (hotbar, inventory).
            let g = self.anim.gauges(
                self.camera.yaw.to_degrees() + 180.0,
                (self.player.pos.x as f64, self.player.pos.z as f64),
                (self.spawn.x as f64, self.spawn.z as f64),
                sky::celestial_angle(self.world_ticks as u64, 1.0),
                self.nether,
            );
            self.be.animate_items(&g);
        }
        // The player's model state (arm swing, walk, the hand's raise) ticks at 20 Hz of the wall clock, at most 2 a frame.
        self.body_acc = (self.body_acc + real_dt).min(2.0 * dig::TICK);
        while self.body_acc >= dig::TICK {
            self.body_acc -= dig::TICK;
            let (riding, slot) = (self.riding(), self.touch.hotbar_slot);
            let sneaking = self.touch.sneak && self.sleep.is_none();
            let (ground, vy) = (self.player.on_ground && !riding, self.player.vel.y / 20.0);
            self.body.tick(self.player.pos.x, self.player.pos.z, self.camera.yaw.to_degrees() + 180.0, ground, vy, !riding && !(ground && sneaking));
            self.body.equip_tick(slot, self.inv.slots[slot]);
        }
        self.furn_acc += dt;
        while self.furn_acc >= dig::TICK {
            self.furn_acc -= dig::TICK;
            // The bobber: gone when the rod is not in hand, 32 blocks away, or after 1200 ticks on the ground.
            if let Some(h) = &mut self.hook {
                let rod = self.inv.slots[self.touch.hotbar_slot].is_some_and(|s| s.id == gear::ROD);
                let get = |x: i32, y: i32, z: i32| self.chunks.block(x, y, z);
                let rain = self.weather.rain(1.0) > 0.5;
                let alive = rod && h.pos.distance_squared(self.player.pos) <= 1024.0 && h.tick(&get, &mut self.mobs.rng, rain);
                let splash = std::mem::take(&mut h.splash);
                if splash > 0.0 {
                    // `EntityFish`: "random.splash" at the bobber, pitch `1 + (r - r) * 0.4`.
                    let pitch = 1.0 + (self.audio.rnd() - self.audio.rnd()) * 0.4;
                    self.audio.play("random.splash", h.pos, splash, pitch);
                }
                if !alive {
                    self.hook = None;
                }
            }
            let centre = (chunk_coord(self.player.pos.x), chunk_coord(self.player.pos.z));
            self.sleep_tick(sub);
            self.sprint_tick();
            self.map_tick();
            // `BlockPressurePlate`: the boxes of what may stand on a plate (the player, the mobs).
            let p = self.player.pos;
            self.ticks.plate_ents.clear();
            self.ticks.plate_ents.push((p - physics::HALF, p + physics::HALF, true));
            self.ticks.plate_ents.extend(self.mobs.list.iter().map(|m| { let (lo, hi) = m.aabb(); (lo, hi, true) }));
            for (x, y, z) in self.chunks.take_fresh_water() {
                self.ticks.wake_water(&mut self.chunks, x, y, z); // a river's edge water, Streams Reflowing's scheduleTick
            }
            self.ticks.step(&mut self.chunks, &mut self.drops, centre, self.weather.rain(1.0));
            for s in std::mem::take(&mut self.ticks.sounds) {
                self.audio.play(s.name, s.at, s.vol, s.pitch);
            }
            {
                let ch = &self.chunks;
                let ears = self.camera.pos;
                if let Some(s) = self.ambient.cave(ears, &|x, y, z| (ch.block_loaded(x, y, z).unwrap_or(1), ch.light_at(x, y, z, 0), ch.light_levels(x, y, z).0)) {
                    self.audio.play(s.name, s.at, s.vol, s.pitch);
                }
                let mut heard = Vec::new();
                self.ambient.display((ears.x.floor() as i32, ears.y.floor() as i32, ears.z.floor() as i32), &|x, y, z| ch.block_loaded(x, y, z).unwrap_or(0), &|x, y, z| ch.meta(x, y, z), &mut heard);
                for s in heard {
                    self.audio.play(s.name, s.at, s.vol, s.pitch);
                }
            }
            for ((x, y, z), n, inst) in std::mem::take(&mut self.ticks.note_events) {
                self.audio.note(glam::Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5), n, inst); // `BlockNote.playBlock`
            }
            // `TileEntityPiston`: whatever stands in a sliding piece is carried half a block along per tick (ponytail: the player and
            // the mobs only, and no collision check on the way).
            for f in &self.ticks.falling {
                let Some(mv) = f.mv else { continue };
                let d = glam::Vec3::new((mv.to.0 - mv.from.0) as f32, (mv.to.1 - mv.from.1) as f32, (mv.to.2 - mv.from.2) as f32) * 0.5;
                let (lo, hi) = (f.pos - glam::Vec3::splat(0.5), f.pos + glam::Vec3::splat(0.5));
                let inside = |a: glam::Vec3, b: glam::Vec3| b.cmpgt(lo).all() && a.cmplt(hi).all();
                let p = self.player.pos;
                if inside(p - physics::HALF, p + physics::HALF) {
                    self.player.pos += d;
                }
                for m in self.mobs.list.iter_mut() {
                    let (a, b) = m.aabb();
                    if inside(a, b) {
                        m.body.pos += d;
                    }
                }
            }
            for p in std::mem::take(&mut self.ticks.dispenses) {
                self.dispense(p);
            }
            for at in std::mem::take(&mut self.ticks.booms) {
                self.explode(at, 4.0, false); // `EntityTNTPrimed.explode`
            }
            if self.sleep.is_none() {
                let p = self.player.pos; // the box centre is 0.9 over the feet
                if let Some((id, snow)) = self.ticks.walk(&mut self.chunks, p.x, p.y - 0.9, p.z) {
                    // `Entity.moveEntity`: the block under the feet steps at 0.15 x its volume (snow on top: the snow's own sound); liquids are silent.
                    if !matches!(id, 8..=11) {
                        let (_, key, pitch) = audio::STEP[if snow { 7 } else { audio::step_of(id) }];
                        self.audio.play_here(key, 0.15, pitch);
                    }
                }
            }
            self.audio.listener(self.player.pos + glam::Vec3::new(0.0, EYE_HEIGHT, 0.0), self.camera.yaw.to_degrees() + 180.0);
            self.audio.set_volumes(self.opts.sound, self.opts.music);
            self.audio.tick(); // `playRandomMusicIfReady`
            self.tick_effects(centre);
            let mut flips = Vec::new();
            for (&pos, f) in self.furnaces.iter_mut() {
                if matches!(self.chunks.block_loaded(pos.0, pos.1, pos.2), Some(61 | 62)) && f.tick() {
                    flips.push((pos, f.burn > 0));
                }
            }
            for ((x, y, z), lit) in flips {
                // `updateFurnaceBlockState`: the facing survives the swap.
                let m = self.chunks.meta(x, y, z);
                self.chunks.set_block_meta(x, y, z, if lit { 62 } else { 61 }, m);
            }
            // Chests: take over what a dungeon's populate put in them; one whose block is gone (dug, blown up, washed away)
            // spills its contents (`BlockChest.onBlockRemoval`; ponytail: seen on the next 20 Hz tick, not at the removal).
            for (p, c) in self.chunks.take_loot() {
                self.chests.entry(p).or_insert(c);
            }
            // Dungeon spawners (`TileEntityMobSpawner`): taken over from populate; one whose block is gone is dropped.
            for (p, kind) in self.chunks.take_spawners() {
                self.fx.spawners.entry(p).or_insert(Spawner::new(kind, 20));
            }
            // Paintings check their wall every 101 ticks and drop as an item when it is gone (`EntityPainting.onUpdate`); a sign's
            // text goes with its block, and closes the editor.
            let chunks = &self.chunks;
            let cell = |x: i32, y: i32, z: i32| chunks.block_loaded(x, y, z).map(|b| (b, chunks.meta(x, y, z)));
            for p in decor::step(&mut self.paintings, &cell) {
                let c = p.frame().0;
                self.drops.scatter(ItemStack { id: decor::PAINTING, count: 1, damage: 0 }, (c.x.floor() as i32, c.y.floor() as i32, c.z.floor() as i32));
            }
            self.signs.retain(|&(x, y, z), _| self.chunks.block_loaded(x, y, z).map_or(true, |b| matches!(b, decor::POST | decor::WALL)));
            if self.sign_edit.is_some_and(|e| !self.signs.contains_key(&e.pos)) {
                self.close_sign();
            }
            self.fx.spawners.retain(|&(x, y, z), _| self.chunks.block_loaded(x, y, z).map_or(true, |b| b == effects::SPAWNER_ID));
            let gone: Vec<_> = self.chests.keys().copied().filter(|&(x, y, z)| self.chunks.block_loaded(x, y, z).is_some_and(|b| b != chest::ID && b != chest::DISPENSER)).collect();
            for p in gone {
                for st in self.chests.remove(&p).into_iter().flatten().flatten() {
                    self.drops.scatter(st, p);
                }
            }
        }

        // `EntityPlayer.onUpdate`: a container that is no longer usable (the chest is gone, or the eye is 8+ blocks away) closes.
        let reach = |p: &chest::Pos| match p.0 {
            i32::MIN => self.carts.list.iter().any(|c| c.key() == *p && (self.player.pos - c.pos).length_squared() <= 64.0),
            _ => chest::usable(&|x, y, z| self.chunks.block(x, y, z).unwrap_or(0), self.camera.pos, std::slice::from_ref(p)),
        };
        if self.screen.as_ref().is_some_and(|s| !s.chests.iter().all(reach)) {
            self.close_screen();
        }

        // M5: pick the block under the crosshair, dig it while the dig input is held, place on a tap.
        self.touch.swing_secs = self.opts.dig_secs();
        self.touch.guard_shown = self.held_guard().points > 0 && self.screen.is_none(); // C4: the guard button, only for an item that can guard
        self.touch.special_shown = matches!(self.held_special(), world::combat::profile::Special::Lunge | world::combat::profile::Special::MultiHit | world::combat::profile::Special::Parry) && self.screen.is_none(); // C5
        self.touch.tick(real_dt);
        let place = self.touch.take_place();
        let place = if self.carry.is_some() { if place { self.put_pack() } false } else { place }; // a carried pack takes the tap
        let strike = self.touch.take_strike();
        // Cortes Combat C3 (NOT b1.7.3, UNVERIFIED): lifting a finger that held still on a mob ends the charge as one blow, `place` for the code below.
        let release = self.touch.take_release();
        let charged = self.charge_release(release);
        self.blow = charged.unwrap_or(1.0);
        let place = place || charged.is_some();
        if place && !self.swing_swallowed() {
            self.body.swing(); // a tap places, uses or hits: `swingItem`
        }
        // The inventory button, and taps on an open screen (its presses are not look / move / place).
        if self.touch.take_inventory() {
            if self.sign_edit.is_some() {
                self.close_sign();
            } else if self.screen.is_some() {
                self.close_screen();
            } else if self.pack_rule {
                if self.opts.pack_cloth {
                    self.pack_button(); // the old pack is a cloth: take it in both hands, then a tap spreads it on flat ground and the screen opens
                } else if !self.riding() && self.sleep.is_none() && !self.vitals.dead() {
                    self.open_satchel(Screen::new(2)); // the satchel: the flat bag, and a button for the recipe book
                }
            } else {
                self.open_screen(Screen::new(2));
            }
        }
        for ev in self.touch.take_screen_events() {
            self.on_screen_event(ev);
        }
        self.pack_tick(real_dt);
        let eye = self.camera.pos.as_dvec3();
        let end = eye + self.camera.forward().as_dvec3() * pick::REACH;
        // The crosshair hits a block's bounds (`Block.collisionRayTrace`: a sapling's small box, half a slab); liquids are not pickable
        // (vanilla's rayTraceBlocks skips them).
        let bounds = |x: i32, y: i32, z: i32| self.chunks.block_loaded(x, y, z).and_then(|b| pick_bounds(b, self.chunks.meta(x, y, z)));
        self.target = pick::ray_trace(&bounds, eye, end);
        // The arm starts to swing while the pad's attack is down or a finger rests on a block, before the hold turns into digging
        // (`HOLD_SECS`). ponytail: it swings without digging for that moment; a finger that moves is a look drag and does not swing.
        if self.touch.pressing() && self.target.is_some() {
            self.body.swing();
        }
        // A finger that rests on a block (no mob, boat or cart nearer) is digging: it starts now instead of at `HOLD_SECS`, so the dig
        // does not wait for the swing that began with the rest; the pad's attack held that long digs too (its release is no hit then).
        // ponytail: a tap longer than `SWING_SECS` on a block digs instead of placing.
        if let Some(hit) = self.target.filter(|_| self.touch.resting() && self.carry.is_none()) {
            let (eye, fwd) = (self.camera.pos, self.camera.forward());
            let wall = hit.point.distance(eye.as_dvec3()) as f32;
            let near = |o: Option<(usize, f32)>| o.is_some_and(|(_, t)| t < wall);
            if !(near(self.mobs.pick(eye, fwd, pick::REACH as f32)) || near(self.boats.pick(eye, fwd, pick::REACH as f32)) || near(self.carts.pick(eye, fwd, pick::REACH as f32))) {
                self.touch.dig_now();
            }
        }

        // Digging runs on the 20 Hz game tick of the Java (hardness-based time, see world::dig).
        // A broken block drops its items (world::items) only if the held item may harvest it (stone needs a pickaxe),
        // and wears the tool (`Item.onBlockDestroyed`: 1 use, a sword 2, a hoe none).
        match self.target {
            Some(hit) if self.touch.digging() && self.charge == 0 && !self.cd_rise => {
                self.dig_acc += real_dt;
                while self.dig_acc >= dig::TICK {
                    self.dig_acc -= dig::TICK;
                    let (x, y, z) = hit.pos;
                    self.body.swing();
                    let in_water = matches!(self.chunks.block_loaded(eye.x.floor() as i32, eye.y.floor() as i32, eye.z.floor() as i32), Some(8 | 9));
                    if let Some(id) = self.chunks.block_loaded(x, y, z) {
                        let slot = self.touch.hotbar_slot;
                        let held = self.inv.slots[slot];
                        let broke = self.dig.tick(hit.pos, id, held, self.player.on_ground, in_water);
                        if self.dig.hit {
                            // `PlayerControllerSP.sendBlockRemoving`: the step sound at (volume + 1) / 8 and half the pitch.
                            let (key, _, pitch) = audio::STEP[audio::step_of(id)];
                            self.audio.play(key, glam::Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5), 0.25, pitch * 0.5);
                        }
                        if broke {
                            let meta = self.chunks.meta(x, y, z); // before the cell is cleared: the drop depends on it
                            self.chunks.set_block(x, y, z, 0);
                            // `RenderGlobal` break effect: `stepSoundDir()` at ((volume + 1) / 2, pitch x 0.8).
                            let (brk, pitch) = (audio::STEP[audio::step_of(id)].1, audio::STEP[audio::step_of(id)].2 * 0.8);
                            self.audio.play(brk, glam::Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5), 1.0, pitch);
                            self.touch.pad.rumble(60, 140); // `RumbleSource.INTERACTION`: a block breaks
                            self.fx.dig(hit.pos, crate::render::atlas::tile_of(id, meta)); // `addBlockDestroyEffects`
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
                            if id == 84 && meta & 15 != 0 {
                                self.audio.play_record(None, glam::Vec3::ZERO);
                                self.drops.spawn_stack(ItemStack { id: 2255 + (meta & 15) as u16, count: 1, damage: 0 }, hit.pos); // the record comes out
                            }
                            if id == 46 {
                                // `BlockTNT.onBlockDestroyedByPlayer` (unconditional): metadata 1, or flint and steel in hand (`onBlockClicked`), lights it.
                                if meta & 1 == 1 || held.is_some_and(|h| h.id == craft::FLINT_AND_STEEL) {
                                    self.ticks.prime(hit.pos, false);
                                } else {
                                    self.drops.spawn_stack(ItemStack { id: 46, count: 1, damage: 0 }, hit.pos);
                                }
                            } else if harvest {
                                // BlockLeaves.harvestBlock: shears drop the leaves themselves, not a sapling.
                                if id == 18 && held.is_some_and(|h| h.id == craft::SHEARS) {
                                    self.drops.spawn_stack(ItemStack { id: 18, count: 1, damage: (meta & 3) as u16 }, hit.pos);
                                } else if id == 18 && self.opts.biome_rules {
                                    // NOT b1.7.3: saplings and apples by the biome and the season (`biomelife::leaf_odds`).
                                    let odds = world::biomelife::leaf_odds(self.chunks.biome_at(hit.pos.0, hit.pos.2), world::biomelife::season(self.world_ticks as u64));
                                    self.drops.spawn_leaf(meta, hit.pos, odds);
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
            // A boat the crosshair is on (and nothing nearer): an empty hand gets in (`Entity.interact`), a held item hits it
            // (`attackEntityFrom`: damage x 10, over 40 breaks it into 3 planks and 2 sticks).
            let mob_t = self.mobs.pick(eye, fwd, pick::REACH as f32).filter(|&(_, t)| t < wall).map(|(_, t)| t);
            let boat_t = self.boats.pick(eye, fwd, pick::REACH as f32).filter(|&(_, t)| t < wall && mob_t.map_or(true, |m| t < m));
            // A ghast's fireball the crosshair is on and nothing nearer (`EntityFireball.attackEntityFrom`): it flies back along the look.
            if let Some((fi, _)) = self.mobs.pick_fireball(eye, fwd, pick::REACH as f32).filter(|&(_, t)| t < wall && mob_t.map_or(true, |m| t < m) && boat_t.map_or(true, |b| t < b.1)) {
                self.mobs.deflect(fi, fwd);
                return;
            }
            // A cart the same way (`EntityMinecart.interact`): the empty one takes a rider, the furnace one is pushed away from you
            // (coal is burnt first); a held item otherwise hits it, over 40 damage breaks it into the cart (and the furnace).
            if let Some((ci, _)) = self.carts.pick(eye, fwd, pick::REACH as f32).filter(|&(_, t)| t < wall && mob_t.map_or(true, |m| t < m) && boat_t.map_or(true, |b| t < b.1)) {
                let held = self.inv.slots[slot].map(|s| s.id);
                let furnace = self.carts.list[ci].kind == 2;
                match held {
                    None if furnace => {
                        self.carts.list[ci].push_from(self.player.pos, false);
                    }
                    None if self.carts.list[ci].kind == 1 => {
                        let k = self.carts.list[ci].key();
                        self.chests.entry(k).or_default();
                        self.open_screen(Screen::at_chest(vec![k]));
                    }
                    None => {
                        if self.carts.ridden().is_none() {
                            self.carts.list[ci].ridden = true;
                        }
                    }
                    Some(minecart::COAL) if furnace => {
                        if self.carts.list[ci].push_from(self.player.pos, true) {
                            self.inv.consume(slot);
                        }
                    }
                    held => {
                        let (dmg, wear) = mobs::attack(held);
                        if self.carts.list[ci].hurt(dmg as i32) {
                            self.smash_cart(ci);
                        }
                        if wear > 0 {
                            self.inv.damage(slot, wear);
                        }
                    }
                }
                return;
            }
            if let Some((bi, _)) = boat_t {
                match self.inv.slots[slot].map(|s| s.id) {
                    None => {
                        if self.boats.ridden().is_none() {
                            self.boats.list[bi].ridden = true;
                        }
                    }
                    held => {
                        let (dmg, wear) = mobs::attack(held);
                        if self.boats.list[bi].hurt(dmg as i32) {
                            self.smash_boat(bi);
                        }
                        if wear > 0 {
                            self.inv.damage(slot, wear);
                        }
                    }
                }
                return;
            }
            // Cortes Combat (C2, **NOT b1.7.3**, UNVERIFIED; only with the world's Immersive Pack ON): the held weapon or tool swings through
            // its cone, not along the crosshair's ray; the bare hand still takes the mob under the crosshair first, the cone being its
            // fallback. Either way an `Entity.interact` (a saddle, a tame wolf, shears, a bone) needs the mob under the crosshair (`touch`).
            let held_id = self.inv.slots[slot].map(|s| s.id);
            let swing = world::combat::rules::swing(held_id);
            if self.swing_held != held_id {
                (self.swing_held, self.swing_cd) = (held_id, 0);
            }
            let (target, ray) = self.melee_target(held_id, eye, fwd, wall);
            if let Some(i) = target {
                let touch = ray == Some(i); // an interaction only with the mob under the crosshair
                // A bone or pork on a wolf, a tap on a tame one, a saddle on a pig, a tap on a saddled one (`Entity.interact`).
                let held = self.inv.slots[slot];
                if let Some(b) = if touch { gear::befriend(&mut self.mobs.list[i], held, &mut self.mobs.rng) } else { None } {
                    let m = &self.mobs.list[i];
                    let (feet, w, h) = (m.body.pos - glam::Vec3::new(0.0, m.half.y, 0.0), m.half.x * 2.0, m.half.y * 2.0);
                    match b {
                        gear::Befriend::Tamed | gear::Befriend::Failed => {
                            self.inv.consume(slot);
                            self.fx.hearts(feet, w, h, b == gear::Befriend::Tamed);
                        }
                        gear::Befriend::Healed | gear::Befriend::Saddled => self.inv.consume(slot),
                        gear::Befriend::Sat | gear::Befriend::Stood | gear::Befriend::Mounted => {}
                    }
                    return;
                }
                if let Some(h) = self.inv.slots[slot].filter(|_| touch) {
                    // Shears / dye on a sheep, a bucket on a cow: `Entity.interact` before the attack.
                    let used = gear::use_on_mob(&mut self.mobs.list[i], h, &mut self.mobs.rng);
                    let at = self.mobs.list[i].body.pos;
                    let cell = (at.x.floor() as i32, at.y.floor() as i32, at.z.floor() as i32);
                    match used {
                        Some(Used::Sheared(wool)) => {
                            wool.into_iter().for_each(|w| self.drops.spawn_stack(w, cell));
                            self.inv.damage(slot, 1);
                            return;
                        }
                        Some(Used::Dyed) => {
                            self.inv.consume(slot);
                            return;
                        }
                        Some(Used::Milked) => {
                            self.inv.slots[slot] = Some(ItemStack { id: gear::MILK_BUCKET, count: 1, damage: 0 });
                            return;
                        }
                        None => {}
                    }
                }
                if swing.is_some() && self.swing_blocked(held_id) {
                    return; // still inside the last swing's cooldown (or behind a raised or staggered guard): nothing happens, and no part of the blow is paid early
                }
                let (dmg, wear) = mobs::attack(held_id);
                // C3: the charged blow. C5: a live hammer streak adds 0.15 a hit to the multiplier (`DamageResolver`), counted before this blow.
                let hammer = swing.is_some_and(|s| s.profile == world::combat::profile::Profile::Warhammer);
                let dmg = dmg * (std::mem::replace(&mut self.blow, 1.0) + if hammer { self.sp.streak_bonus() } else { 0.0 });
                // Retro Sprint (`PlayerEntityMixin.onAttack`): a sprinting hit shoves the target the way the player looks, before the hit's own
                // knockback, and ends the sprint (the walk is set from the input every frame, so the mod's x0.6 on the player's motion has no use here).
                if self.sprint.attack() {
                    let f = self.camera.forward();
                    let d = glam::Vec3::new(f.x, 0.0, f.z).normalize_or_zero();
                    self.mobs.shove(i, d * sprint::KNOCK + glam::Vec3::Y * sprint::KNOCK_UP);
                }
                if let Some(s) = swing {
                    self.swing_cd = s.interval;
                    self.mobs.strike(i, dmg, self.player.pos); // the weapon's own 3-tick damage window
                } else {
                    self.mobs.hit(i, dmg, self.player.pos);
                }
                if hammer {
                    self.sp.streak_hit(); // ponytail: every blow that connects counts, one the mob's damage window refused too (a martillo is 19 ticks, the window 3)
                }
                self.vitals.stamina.hit(); // Stamina: a hit dealt puts the player in combat for 15 s (ponytail: also one the mob's damage window ignored)
                self.inv.damage(slot, wear);
                return;
            }
        }
        // A tap on a painting nearer than the block behind it breaks it (`EntityPainting.attackEntityFrom`: it drops as an item).
        if place {
            let (eye, fwd) = (self.camera.pos, self.camera.forward());
            let wall = self.target.map_or(f32::MAX, |h| h.point.distance(eye.as_dvec3()) as f32);
            if let Some((i, _)) = decor::pick(&self.paintings, eye, fwd, pick::REACH as f32).filter(|&(_, t)| t < wall) {
                self.break_painting(i);
                return;
            }
        }
        // The pad's attack tap only hits: it never places a block.
        if strike {
            return;
        }
        let Some(hit) = self.target else {
            if place {
                self.use_held();
            }
            return;
        };
        if place {
            // Using a workbench or a furnace opens its screen instead of placing a block against it.
            match self.chunks.block_loaded(hit.pos.0, hit.pos.1, hit.pos.2) {
                Some(58) => {
                    if self.pack_rule && !self.opts.pack_cloth {
                        self.open_satchel(Screen::new(3)); // the bag, and the book open on its 3x3 grid
                    } else {
                        self.open_screen(Screen::new(3));
                    }
                    return;
                }
                Some(61 | 62) => {
                    self.furnaces.entry(hit.pos).or_default();
                    self.open_screen(Screen::at_furnace(hit.pos));
                    return;
                }
                // BlockDoor.blockActivated: a wooden door swings, an iron one ignores the hand.
                Some(64) => {
                    self.toggle_door(hit.pos);
                    return;
                }
                Some(71) => return,
                // BlockCake.blockActivated: a slice heals 3 while below full health; the sixth slice removes the cake.
                // ponytail: a punch (`onBlockClicked`) does not eat; holding it digs the cake away like any block.
                Some(92) => {
                    let (x, y, z) = hit.pos;
                    if self.vitals.health < crate::world::vitals::MAX_HEALTH {
                        self.vitals.heal(3);
                        let m = self.chunks.meta(x, y, z) + 1;
                        if m >= 6 {
                            self.chunks.set_block(x, y, z, 0);
                        } else {
                            self.chunks.set_block_meta(x, y, z, 92, m);
                        }
                    }
                    return;
                }
                // BlockTrapDoor.blockActivated: the open bit flips.
                Some(96) => {
                    let (x, y, z) = hit.pos;
                    self.chunks.set_block_meta(x, y, z, 96, self.chunks.meta(x, y, z) ^ 4);
                    return;
                }
                // BlockLever.blockActivated: the on bit flips. ponytail: a punch (`onBlockClicked`) does not flip it.
                Some(69) => {
                    let (x, y, z) = hit.pos;
                    let on = self.chunks.meta(x, y, z) & 8 == 0; // `BlockLever`: pitch 0.6 turning on, 0.5 off
                    self.chunks.set_block_meta(x, y, z, 69, self.chunks.meta(x, y, z) ^ 8);
                    self.audio.play("random.click", glam::Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5), 0.3, if on { 0.6 } else { 0.5 });
                    return;
                }
                // BlockButton.blockActivated: pressed for 20 ticks.
                Some(77) => {
                    let (x, y, z) = hit.pos;
                    let m = self.chunks.meta(x, y, z);
                    if m & 8 == 0 && self.chunks.set_block_meta(x, y, z, 77, m | 8) {
                        self.ticks.schedule(x, y, z, 77, 20);
                        self.audio.play("random.click", glam::Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5), 0.3, 0.6);
                    }
                    return;
                }
                // BlockNote.blockActivated.
                Some(25) => {
                    let (x, y, z) = hit.pos;
                    self.ticks.note_use(&self.chunks, x, y, z);
                    return;
                }
                // BlockJukeBox.blockActivated + ItemRecord.onItemUse: a record goes in (the metadata says which: 1 = 13, 2 = cat;
                // ponytail: the Java keeps it in a tile entity), clicking again takes it out.
                Some(84) => {
                    let (x, y, z) = hit.pos;
                    let m = self.chunks.meta(x, y, z);
                    if m != 0 {
                        self.chunks.set_block_meta(x, y, z, 84, 0);
                        self.audio.play_record(None, glam::Vec3::ZERO); // `playRecord(null)`
                        self.drops.spawn_stack(ItemStack { id: 2255 + m as u16, count: 1, damage: 0 }, (x, y + 1, z));
                        return;
                    } else if let Some(s) = self.inv.slots[self.touch.hotbar_slot].filter(|s| matches!(s.id, 2256 | 2257)) {
                        self.chunks.set_block_meta(x, y, z, 84, (s.id - 2255) as u8);
                        self.audio.play_record(Some(if s.id == 2256 { "13" } else { "cat" }), glam::Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5));
                        self.inv.consume(self.touch.hotbar_slot);
                        return;
                    }
                }
                // BlockRedstoneRepeater.blockActivated: the delay goes 1 -> 2 -> 3 -> 4 -> 1.
                Some(93 | 94) => {
                    let (x, y, z) = hit.pos;
                    let (m, id) = (self.chunks.meta(x, y, z), self.chunks.block_loaded(x, y, z).unwrap_or(93));
                    self.chunks.set_block_meta(x, y, z, id, ((m & 12) + 4 & 12) | m & 3);
                    return;
                }
                // BlockRedstoneOre.blockActivated: it lights up, then the block in hand is still placed against it.
                Some(73) => {
                    self.chunks.set_block(hit.pos.0, hit.pos.1, hit.pos.2, 74);
                }
                Some(26) => {
                    self.use_bed(hit.pos);
                    return;
                }
                // BlockDispenser.blockActivated: its 9 slots open as a screen.
                Some(chest::DISPENSER) => {
                    self.chests.entry(hit.pos).or_default();
                    self.open_screen(Screen::at_dispenser(hit.pos));
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
            // ItemPainting.onItemUse: a side face; the motive is drawn among those that fit (`decor::hang`). Never places a block.
            if self.inv.slots[slot].is_some_and(|s| s.id == decor::PAINTING) {
                if let (Some(dir), true) = (decor::dir_of_face(hit.face), self.paintings.len() < decor::MAX_PAINTINGS) {
                    let chunks = &self.chunks;
                    let cell = |x: i32, y: i32, z: i32| chunks.block_loaded(x, y, z).map(|b| (b, chunks.meta(x, y, z)));
                    if let Some(p) = decor::hang(hit.pos, dir, &self.paintings, &cell, &mut self.mobs.rng) {
                        self.paintings.push(p);
                        self.inv.consume(slot);
                    }
                }
                return;
            }
            // ItemSign.onItemUse: a post on a top face, a wall sign on a side face; the editor opens on the new sign.
            if self.inv.slots[slot].is_some_and(|s| s.id == decor::SIGN) {
                let yaw = self.camera.yaw.to_degrees() + 180.0;
                let placed = decor::sign_place(&|a, b, c| self.chunks.block_loaded(a, b, c).unwrap_or(0), hit.pos, hit.face, yaw);
                if let Some(((x, y, z), id, meta)) = placed {
                    if self.chunks.set_block_meta(x, y, z, id, meta) {
                        self.inv.consume(slot);
                        self.signs.insert((x, y, z), Default::default());
                        self.sign_edit = Some(Edit::new((x, y, z)));
                        self.touch.set_screen(true);
                    }
                }
                return;
            }
            // ItemBed.onItemUse: only against a top face, both halves on air over solid cubes.
            if self.inv.slots[slot].is_some_and(|s| s.id == 355) {
                let (x, y, z) = pick::place_pos(&hit);
                let yaw = self.camera.yaw.to_degrees() + 180.0;
                let n = pick::bed_meta(&|a, b, c| self.chunks.block_loaded(a, b, c).unwrap_or(0), (x, y, z), yaw);
                if let Some(n) = n.filter(|_| hit.face == 1) {
                    let (dx, dz) = pick::BED_DIR[n as usize];
                    self.chunks.set_block_meta(x, y, z, 26, n);
                    self.chunks.set_block_meta(x + dx, y, z + dz, 26, n + 8);
                    self.inv.consume(slot);
                }
                return;
            }
            // ItemDoor.onItemUse: only against a top face; both halves are written before the neighbours are told.
            if let Some(s) = self.inv.slots[slot].filter(|s| matches!(s.id, 324 | 330)) {
                let (x, y, z) = pick::place_pos(&hit);
                let bid = if s.id == 324 { 64 } else { 71 };
                let yaw = self.camera.yaw.to_degrees() + 180.0;
                let meta = pick::door_meta(&|a, b, c| self.chunks.block_loaded(a, b, c).unwrap_or(0), (x, y, z), yaw, bid);
                if let Some(m) = meta.filter(|_| hit.face == 1) {
                    self.chunks.set_block_meta(x, y, z, bid, m);
                    self.chunks.set_block_meta(x, y + 1, z, bid, m + 8);
                    self.inv.consume(slot);
                }
                return;
            }
            // ItemFlintAndSteel.onItemUse: fire in the empty cell next to the face aimed at; one use of wear either way.
            if self.inv.slots[slot].is_some_and(|s| s.id == craft::FLINT_AND_STEEL) {
                let (x, y, z) = pick::place_pos(&hit);
                if self.chunks.block_loaded(x, y, z) == Some(0) {
                    let p = self.audio.rnd() * 0.4 + 0.8;
                    self.audio.play("fire.ignite", glam::Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5), 1.0, p);
                    self.chunks.set_block(x, y, z, 51);
                }
                self.inv.damage(slot, 1);
                return;
            }
            if self.inv.slots[slot].is_some_and(|s| craft::is_hoe(s.id)) {
                let (x, y, z) = hit.pos;
                let (b, above) = (self.chunks.block_loaded(x, y, z), self.chunks.block_loaded(x, y + 1, z));
                if let (Some(b), Some(above)) = (b, above) {
                    if craft::can_till(b, above, hit.face) {
                        self.chunks.set_block(x, y, z, 60);
                        let (key, _, pitch) = audio::STEP[audio::step_of(60)]; // `ItemHoe`: the tilled field's step sound
                        self.audio.play(key, glam::Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5), 1.0, pitch * 0.8);
                        self.inv.damage(slot, 1);
                    }
                }
                return;
            }
            // ItemMinecart.onItemUse: aimed at a rail, the cart is set in the rail's cell (its centre, 0.5 up).
            if let Some(kind) = self.inv.slots[slot].and_then(|s| match s.id {
                minecart::EMPTY => Some(0),
                minecart::WITH_CHEST => Some(1),
                minecart::WITH_FURNACE => Some(2),
                _ => None,
            }) {
                let (x, y, z) = hit.pos;
                if matches!(self.chunks.block_loaded(x, y, z), Some(27 | 28 | 66)) {
                    self.carts.list.push(minecart::Cart::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5, kind));
                    self.inv.consume(slot);
                }
                return;
            }
            // ItemSeeds.onItemUse: only the top face of farmland with air above; one seed becomes a crops block (meta 0).
            if self.inv.slots[slot].is_some_and(|s| s.id == 295) {
                let (x, y, z) = hit.pos;
                if hit.face == 1 && self.chunks.block_loaded(x, y, z) == Some(60) && self.chunks.block_loaded(x, y + 1, z) == Some(0) {
                    self.chunks.set_block(x, y + 1, z, 59);
                    self.inv.consume(slot);
                }
                return;
            }
            // ItemReed.onItemUse (sugar cane, cake): the cell beside the face aimed at, or the snow layer itself; the block's
            // `canPlaceBlockAt` decides (reeds: `reed_ok`; cake: solid ground, and the player's box must not be in the way).
            if self.inv.slots[slot].is_some_and(|s| matches!(s.id, 338 | 354)) {
                let cake = self.inv.slots[slot].is_some_and(|s| s.id == 354);
                let at = if self.chunks.block_loaded(hit.pos.0, hit.pos.1, hit.pos.2) == Some(78) { hit.pos } else { pick::place_pos(&hit) };
                let get = |a, b, c| self.chunks.block_loaded(a, b, c).unwrap_or(0);
                let ok = at.1 < 127 && self.chunks.block_loaded(at.0, at.1, at.2).is_some_and(pick::replaceable)
                    && if cake { !crate::world::ticks::non_solid(get(at.0, at.1 - 1, at.2)) && !pick::overlaps_player(at, physics_id(92, 0), self.player.pos) } else { pick::reed_ok(&get, at) };
                if ok && self.chunks.set_block_meta(at.0, at.1, at.2, if cake { 92 } else { 83 }, 0) {
                    self.inv.consume(slot);
                    self.placed(if cake { 92 } else { 83 }, at);
                }
                return;
            }
            // ItemRedstone.onItemUse: dust on the cell beside the face aimed at (`BlockRedstoneWire.canPlaceBlockAt`: solid below).
            // ItemReed for the repeater (356 -> block 93, facing from the yaw, `onBlockPlacedBy`: scheduled at once when powered).
            if let Some(b) = self.inv.slots[slot].and_then(|s| match s.id { 331 => Some(55u8), 356 => Some(93), _ => None }) {
                let at = pick::place_pos(&hit);
                let meta = pick::facing_meta(b as u16, self.camera.yaw.to_degrees() + 180.0).unwrap_or(0);
                if at.1 < 127 && self.chunks.block_loaded(at.0, at.1, at.2).is_some_and(pick::replaceable) && ticks::can_place(&self.chunks, at.0, at.1, at.2, b) && self.chunks.set_block_meta(at.0, at.1, at.2, b, meta) {
                    self.inv.consume(slot);
                    if b == 93 {
                        self.placed(b, at); // an `ItemReed`; the dust (`ItemRedstone`) is silent
                    }
                    if b == 93 && redstone::repeater_input(&self.chunks, at.0, at.1, at.2) {
                        self.ticks.schedule(at.0, at.1, at.2, 93, 1);
                    }
                }
                return;
            }
            let (x, y, z) = pick::place_pos(&hit);
            // Items below 256 are blocks; anything else cannot be placed. ItemBlock refuses a solid block at y = 127
            // (ponytail: plants are refused there too).
            if let Some(s) = self.inv.slots[slot].filter(|s| s.id < 256) {
                // A torch's facing comes from the face aimed at and what can hold it (None = nothing to hang on: no placement).
                let floor = 5 + self.mobs.rng.next_int_bound(2) as u8; // a lever on the floor: 5 or 6
                let meta = if matches!(s.id, 50 | 75 | 76) { pick::torch_meta(&|a, b, c| self.chunks.block_loaded(a, b, c).unwrap_or(0), (x, y, z), hit.face) } else if matches!(s.id, 33 | 29) { Some(pick::piston_facing(self.player.pos, (x, y, z), self.camera.yaw.to_degrees() + 180.0)) } else if matches!(s.id, 69 | 77) { pick::switch_meta(&|a, b, c| self.chunks.block_loaded(a, b, c).unwrap_or(0), (x, y, z), hit.face, s.id == 69, floor) } else if s.id == 96 { pick::trap_meta(hit.face, self.chunks.block_loaded(hit.pos.0, hit.pos.1, hit.pos.2).is_some_and(normal_cube)) } else { pick::facing_meta(s.id, self.camera.yaw.to_degrees() + 180.0).or(Some(items::placed_meta(s))) };
                if let Some(meta) = meta.filter(|_| y < 127 && self.chunks.block_loaded(x, y, z).is_some_and(pick::replaceable) && !pick::overlaps_player((x, y, z), s.id as u8, self.player.pos) && ticks::can_place(&self.chunks, x, y, z, s.id as u8) && (s.id != chest::ID as u16 || chest::can_place(&|a, b, c| self.chunks.block(a, b, c).unwrap_or(0), (x, y, z)))) {
                    if self.chunks.set_block_meta(x, y, z, s.id as u8, meta) {
                        let (key, _, pitch) = audio::STEP[audio::step_of(s.id as u8)]; // `ItemBlock`: step sound, volume (1 + 1) / 2, pitch * 0.8
                        self.audio.play(key, glam::Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5), 1.0, pitch * 0.8);
                        self.inv.consume(slot);
                    }
                }
            } else {
                self.use_held();
            }
        }
    }

    /// NOT b1.7.3 (`biomeRules`, `biomelife::air`): the heat or cold of the biome on the player: open daylight on the head, a fire at hand
    /// (block light of a torch, furnace or lava), a meal, and the armor worn (leather is fur, the rest is metal).
    fn air(&self) -> world::biomelife::Air {
        if !self.opts.biome_rules || self.nether {
            return Default::default();
        }
        let (p, angle) = (self.player.pos, sky::celestial_angle(self.world_ticks as u64, 1.0));
        let sub = sky::skylight_subtracted(angle, self.weather.rain(1.0), self.weather.thunder(1.0));
        let (x, z) = (p.x.floor() as i32, p.z.floor() as i32);
        let (head, feet) = ((p.y + 0.7).floor() as i32, (p.y - 0.5).floor() as i32);
        let sun = ((self.chunks.light_levels(x, head, z).0.saturating_sub(sub) as f32 - 8.0) / 7.0).clamp(0.0, 1.0);
        let fire = self.chunks.light_levels(x, head, z).1.max(self.chunks.light_levels(x, feet, z).1) >= world::biomelife::FIRE_LIGHT;
        let (mut metal, mut fur) = (0u8, 0u8);
        for a in self.inv.slots[items::MAIN..].iter().flatten().filter_map(|s| craft::armor(s.id)) {
            if a.1 == 0 { fur += 1 } else { metal += 1 }
        }
        world::biomelife::air(self.opts.atmos.here, world::biomelife::Body { sun, fire, fed: self.vitals.fed > 0, metal, fur })
    }

    /// The mob a tap with `held_id` hits (first) and the one under the crosshair's ray (second). With the Immersive Pack ON (Cortes Combat,
    /// C2, NOT b1.7.3, UNVERIFIED) a weapon or tool takes the nearest mob in its cone, the bare hand the ray's with the cone as fallback.
    fn melee_target(&self, held_id: Option<u16>, eye: glam::Vec3, fwd: glam::Vec3, wall: f32) -> (Option<usize>, Option<usize>) {
        let swing = world::combat::rules::swing(held_id);
        let ray = self.mobs.pick(eye, fwd, swing.map_or(pick::REACH as f32, |s| s.reach.dist)).filter(|&(_, t)| t < wall).map(|(i, _)| i);
        let cone = swing.and_then(|s| self.mobs.cone(eye, fwd, s.reach, wall));
        (if swing.is_some() && held_id.is_some() { cone } else { ray.or(cone) }, ray)
    }

    /// Would a tap now be swallowed by the held item's cooldown (a target in reach, `swing_cd` still running)? Then the arm does not swing,
    /// so a blocked tap looks like what it is. Cortes Combat C2, NOT b1.7.3, UNVERIFIED. ponytail: ignores a boat or cart nearer than the mob.
    fn swing_swallowed(&self) -> bool {
        let (eye, fwd, slot) = (self.camera.pos, self.camera.forward(), self.touch.hotbar_slot);
        let held_id = self.inv.slots[slot].map(|s| s.id);
        if world::combat::rules::swing(held_id).is_none() || !(self.swing_held == held_id && self.swing_cd > 0 || self.guard_blocks(held_id)) {
            return false;
        }
        let wall = self.target.map_or(f32::MAX, |h| h.point.distance(eye.as_dvec3()) as f32);
        self.melee_target(held_id, eye, fwd, wall).0.is_some()
    }

    /// C4: what guard the held item is worth (`guard::profile_of`; nothing with the Immersive Pack OFF).
    fn held_guard(&self) -> world::combat::guard::GuardProfile {
        let held = self.inv.slots[self.touch.hotbar_slot].map(|s| s.id);
        world::combat::rules::swing(held).map_or(world::combat::guard::NONE, |s| world::combat::guard::profile_of(s.profile))
    }

    /// C4: a raised guard, or the stagger of the one in hand, stops the swing (`AttackCooldown.beginCharge`: covering and swinging are exclusive).
    fn guard_blocks(&self, held_id: Option<u16>) -> bool {
        let g = &self.vitals.guard;
        g.active() || (g.stun > 0 && world::combat::rules::swing(held_id).is_some_and(|s| world::combat::guard::profile_of(s.profile).points > 0))
    }

    /// C4: the held item cannot swing now: its cooldown, or `guard_blocks`.
    fn swing_blocked(&self, held_id: Option<u16>) -> bool {
        self.swing_cd > 0 || self.guard_blocks(held_id)
    }

    /// C4 (`GuardHandler.onShieldBlock`): a hit from `from` into the raised guard is absorbed, a point spent and a combat mark made; the last
    /// point breaks the guard and the charge. True if blocked. ponytail: the arc is measured on the ground plane, the mod's is 3D with the pitch.
    fn guarded(&mut self, from: glam::Vec3) -> bool {
        let (gp, me, f) = (self.held_guard(), self.player.pos, self.camera.forward());
        let Some(broke) = self.vitals.guard.block(gp, glam::Vec3::new(f.x, 0.0, f.z), me, glam::Vec3::new(from.x, me.y, from.z)) else { return false };
        self.vitals.stamina.hit();
        if broke {
            self.charge = 0;
        }
        true
    }

    /// C4, 20 Hz (only with the Immersive Pack ON): the guard follows its button and regenerates, the food bar pays for healing, a meal runs.
    fn survival_tick(&mut self) {
        if !world::combat::rules::on() {
            return;
        }
        let (gp, down) = (self.held_guard(), self.touch.guarding() && self.sleep.is_none() && !self.vitals.dead());
        self.vitals.guard.sync(gp);
        if self.vitals.guard.input(down, self.eating.is_some()) {
            self.charge = 0; // raising the guard ends a charge (`endCharge`)
        }
        self.vitals.guard.tick(gp);
        let hurt = !self.vitals.dead() && self.vitals.health < vitals::MAX_HEALTH;
        if self.vitals.food.tick(hurt) > 0 {
            self.vitals.heal(1);
        }
        self.vitals.stamina.hungry = self.vitals.food.hungry();
        self.eat_tick();
    }

    /// C4, 20 Hz: the meal in the mouth runs down; damage (unless guarding), another slot, no more food or a closed game cut it short.
    fn eat_tick(&mut self) {
        let Some(mut e) = self.eating else { return };
        let food = self.touch.hotbar_slot == e.slot && self.inv.slots[e.slot].is_some_and(|s| items::heal_amount(s.id).is_some());
        let hit = self.vitals.health < e.health && !self.vitals.guard.active();
        if !food || hit || self.vitals.dead() || self.screen.is_some() {
            self.eating = None;
            return;
        }
        e.health = self.vitals.health; // healing raises the line a hit is measured from
        e.left -= 1;
        if e.left > 0 {
            self.eating = Some(e);
        } else {
            self.eating = None;
            if let Some(n) = self.inv.eat(e.slot) {
                self.vitals.food.eat(n);
                self.vitals.fed = world::biomelife::FED_TICKS; // NOT b1.7.3 (`biomeRules`): food for the cold
            }
        }
    }

    /// C5: what the held item's right button does (`Special::None` with the Immersive Pack OFF).
    fn held_special(&self) -> world::combat::profile::Special {
        let held = self.inv.slots[self.touch.hotbar_slot].map(|s| s.id);
        world::combat::rules::swing(held).map_or(world::combat::profile::Special::None, |s| s.profile.special())
    }

    /// C5, 20 Hz: the right button's clocks, and a press on it. A press is taken even with the rule off, so it never waits for a later world.
    fn special_tick(&mut self) {
        let pressed = self.touch.take_special();
        if !world::combat::rules::on() || self.vitals.dead() {
            self.sp = Default::default();
            return;
        }
        self.sp.tick();
        if pressed {
            self.special_press();
        }
        self.lunge_tick();
    }

    /// C5 (`SpecialHandler.resolve`): the sweep and the lunge spend the attack's own cooldown (4 and 8 swings) and are refused behind it or a
    /// raised guard; the parry has a lockout of its own and does not care about the swing. Not ported: wear, Fusee (C8), sounds (C7).
    fn special_press(&mut self) {
        use world::combat::{profile::Special, special};
        let held_id = self.inv.slots[self.touch.hotbar_slot].map(|s| s.id);
        let Some(s) = world::combat::rules::swing(held_id) else { return };
        if self.screen.is_some() || self.sleep.is_some() || self.riding() {
            return;
        }
        match s.profile.special() {
            Special::Parry => {
                if self.sp.parry_press() {
                    self.body.swing();
                }
            }
            Special::MultiHit if !self.swing_blocked(held_id) => {
                let (eye, fwd) = (self.camera.pos, self.camera.forward());
                let wall = self.target.map_or(f32::MAX, |h| h.point.distance(eye.as_dvec3()) as f32);
                (self.swing_cd, self.charge) = (special::sweep_cost(s.interval), 0); // spent whether or not it connects
                self.body.swing();
                // Everything in the wider arc, the weapon's flat damage, no charge. ponytail: the knockback is `strike`'s own, not the sweep's 0.8 share.
                let hit = self.mobs.cone_n(eye, fwd, special::sweep_reach(s.reach), wall, 0);
                let (dmg, _) = mobs::attack(held_id);
                for &i in &hit {
                    self.mobs.strike(i, dmg, self.player.pos);
                }
                if !hit.is_empty() {
                    self.vitals.stamina.hit();
                }
            }
            Special::Lunge if !self.swing_blocked(held_id) && self.sp.lunge.is_none() => {
                let f = self.camera.forward();
                let dir = glam::Vec3::new(f.x, 0.0, f.z).normalize_or_zero();
                if dir == glam::Vec3::ZERO {
                    return;
                }
                let tier = held_id.and_then(world::combat::catalog::get).map_or(0, |(_, t)| t);
                (self.swing_cd, self.charge) = (special::lunge_cost(s.interval), 0);
                self.body.swing();
                let p = self.player.pos;
                self.sp.lunge = Some(special::Lunge { dir, start: p, last: p, dist: special::lunge_distance(tier), left: special::LUNGE_WINDOW, done: Vec::new(), ticks: 0 });
            }
            _ => {}
        }
    }

    /// C5, 20 Hz (`Lunge.tick`): the body is driven along the line in `frame`; here it ends (20 ticks, the distance of its tier, a wall or
    /// anything that stops it, another item, a screen) and strikes what its cube meets on the way, once each, the nearest first.
    fn lunge_tick(&mut self) {
        use world::combat::{profile::Special, special};
        let Some(mut l) = self.sp.lunge.take() else { return };
        let held_id = self.inv.slots[self.touch.hotbar_slot].map(|s| s.id);
        let Some(s) = world::combat::rules::swing(held_id).filter(|s| s.profile.special() == Special::Lunge) else { return };
        let p = self.player.pos;
        let travelled = (p.x - l.start.x).hypot(p.z - l.start.z);
        let step = (p.x - l.last.x).hypot(p.z - l.last.z);
        if l.left == 0 || travelled >= l.dist || (l.ticks > 0 && step < special::STILL) || self.screen.is_some() {
            return;
        }
        let (lo, hi) = special::lunge_cube(p, s.reach.dist);
        let fresh: Vec<usize> = self.mobs.in_box(lo, hi, p).into_iter().filter(|i| !l.done.contains(i)).collect();
        let (dmg, _) = mobs::attack(held_id);
        for (k, &i) in fresh.iter().enumerate() {
            self.mobs.strike(i, special::lunge_damage(dmg, k), p);
            l.done.push(i);
        }
        if !fresh.is_empty() {
            self.vitals.stamina.hit();
        }
        (l.last, l.left, l.ticks) = (p, l.left - 1, l.ticks + 1);
        self.sp.lunge = Some(l);
    }

    /// C5 (`ParryHandler`): inside the shotel's window a hit does nothing and goes back to its dealer at the same strength. True when it parried.
    /// ponytail: the dealer is the mob standing at the hit's source; an arrow is parried too (nobody to answer it).
    fn parried(&mut self, d: i32, from: glam::Vec3) -> bool {
        if !self.sp.parry_take() {
            return false;
        }
        self.mobs.reflect(from, d as f32, self.player.pos);
        self.vitals.stamina.hit();
        true
    }

    /// C3 / C4 request: while a charge builds, the hand repeats the hotbar switch (the weapon rises slowly from low), one rise per cooldown of
    /// the weapon; at a full charge it stays raised. During the cooldown that comes first (the charge cannot build yet) the same rise follows
    /// the cooldown itself, so a hold never looks idle. `None` when nothing is charging.
    fn charge_phase(&self, partial: f32) -> Option<f32> {
        let s = world::combat::rules::swing(self.inv.slots[self.touch.hotbar_slot].map(|s| s.id))?;
        let full = s.profile.charge().ticks;
        if full == 0 || s.interval == 0 {
            return None;
        }
        let i = s.interval as f32;
        if self.cd_rise && self.swing_cd > 0 {
            return Some(1.0 - ((self.swing_cd as f32 - partial).max(0.0) / i).min(1.0));
        }
        if self.charge == 0 {
            return None;
        }
        if self.charge >= full {
            return Some(1.0);
        }
        Some(((self.charge as f32 - 1.0 + partial).max(0.0) % i) / i)
    }

    /// C3, 20 Hz: a finger (or the pad's attack) held still on a mob in the cone of a weapon that can charge builds `charge` up to the profile's
    /// full hold, once the cooldown is over. Changing item starts everything over, as in the swing code (`resetOnItemChange`).
    fn charge_tick(&mut self) {
        let (eye, fwd, slot) = (self.camera.pos, self.camera.forward(), self.touch.hotbar_slot);
        let held_id = self.inv.slots[slot].map(|s| s.id);
        if self.swing_held != held_id {
            (self.swing_held, self.swing_cd, self.charge) = (held_id, 0, 0);
        }
        let full = world::combat::rules::swing(held_id).map_or(0, |s| s.profile.charge().ticks);
        let cooling = self.swing_cd > 0 && !self.guard_blocks(held_id); // only the cooldown holds it back, not a guard
        self.cd_rise = false;
        if full == 0 || (self.swing_blocked(held_id) && !cooling) || self.screen.is_some() || !self.touch.resting() {
            return; // `charge_release` clears it once the finger is up or dragging
        }
        let wall = self.target.map_or(f32::MAX, |h| h.point.distance(eye.as_dvec3()) as f32);
        if self.melee_target(held_id, eye, fwd, wall).0.is_some() {
            if cooling {
                self.cd_rise = true; // the charge starts when the cooldown ends; until then the hand rises with it
            } else {
                self.charge = (self.charge + 1).min(full);
            }
        }
    }

    /// C3: the multiplier of the blow a release now delivers (`rules::charged`), and the end of the charge once the finger is not holding
    /// still any more (up, or dragging: a drag cancels).
    fn charge_release(&mut self, release: bool) -> Option<f32> {
        let held = self.charge;
        if !self.touch.resting() {
            self.charge = 0;
        }
        let (eye, fwd) = (self.camera.pos, self.camera.forward());
        let held_id = self.inv.slots[self.touch.hotbar_slot].map(|s| s.id);
        let wall = self.target.map_or(f32::MAX, |h| h.point.distance(eye.as_dvec3()) as f32);
        // A release with nothing in reach is no blow: it must not fall through to placing a block or using the item.
        let hit = release && !self.swing_blocked(held_id) && self.melee_target(held_id, eye, fwd, wall).0.is_some();
        world::combat::rules::swing(held_id).filter(|_| hit).and_then(|s| world::combat::rules::charged(s.profile, held))
    }

    /// One 20 Hz tick of sprinting (`ClientPlayerEntityMixin.tickSprintingMovement`) and its dust (`EntityMixin.addParticles`: a speck of the
    /// block below the feet while sprinting out of the water). The forward input is the d-pad's / the stick's, sneaking already x0.3.
    fn sprint_tick(&mut self) {
        self.swing_cd = self.swing_cd.saturating_sub(1); // Cortes Combat's attack cooldown (C2), 20 Hz
        self.charge_tick();
        self.survival_tick();
        self.special_tick();
        let (asleep, riding) = (self.sleep.is_some(), self.riding());
        let fwd = if asleep || riding { 0.0 } else { self.touch.move_input.0 * if self.touch.sneak { 0.3 } else { 1.0 } };
        self.sprint.tick(sprint::In {
            fwd,
            key: self.touch.pad_sprint,
            ground: self.player.on_ground,
            blocked: std::mem::take(&mut self.sprint_blocked),
            water: self.was_in_water,
            can: !asleep && !riding && !self.vitals.dead() && self.vitals.stamina.can_sprint(), // an empty bar locks the sprint (Stamina)
        });
        // Stamina (`StaminaFeature.onPlayerTick`): a sprint consumes, anything else regenerates. The debug Fly button stands in for creative (no cost).
        let air = self.air();
        self.vitals.fed = self.vitals.fed.saturating_sub(1);
        self.vitals.stamina.tick_in(self.sprint.on() && !self.touch.flying, self.vitals.health, self.vitals.armor, air);
        if self.sprint.on() && !self.was_in_water {
            let (p, v) = (self.player.pos, self.player.vel / 20.0);
            let (x, y, z) = (p.x.floor() as i32, (p.y - 0.9 - 0.2).floor() as i32, p.z.floor() as i32); // 0.2 under the feet
            if let Some(id) = self.chunks.block(x, y, z).filter(|&b| b > 0 && !matches!(b, 8..=11)) {
                let tile = atlas::tile_of(id, self.chunks.meta(x, y, z));
                self.fx.sprint_dust(glam::Vec3::new(p.x, p.y - 0.9, p.z), v, tile);
            }
        }
    }

    /// `Minecraft.clickMouse` -> `sendUseItem`: a tap that neither placed a block nor opened a screen uses the held item;
    /// food is eaten, aimed at a block or not.
    /// `Explosion` at `at`: the rays pick the cells (`Mobs::explode`), a broken block drops with chance 0.3 (vanilla: per item),
    /// the player takes `(impact^2 + impact) / 2 x 8 x power + 1` (no exposure test, no knockback).
    /// One 20 Hz tick of `world::effects`: thunderstorm rolls and what a bolt hits, particles, spawners.
    fn tick_effects(&mut self, centre: (i32, i32)) {
        // `World.tick`: in a thunderstorm (`func_27161_C` rain > 0.2 and `func_27160_B` thunder > 0.9) each chunk near the player
        // rolls 1 in 100000 for a bolt on a column that is rained on (`canBlockBeRainedOn`: open to the sky, in a biome with rain).
        if self.weather.rain(1.0) > 0.2 && self.weather.thunder(1.0) > 0.9 {
            for (x, z) in self.fx.storm(centre) {
                let y = self.chunks.top_solid(x, z);
                if y >= 0 && self.chunks.biomes(x, z, 1, 1)[0].rains() {
                    let at = glam::Vec3::new(x as f32, y as f32, z as f32);
                    self.fx.strike(at);
                    self.fx.clap(at, self.camera.pos);
                }
            }
        }
        let get = |x: i32, y: i32, z: i32| self.chunks.block(x, y, z);
        self.fx.river_life(self.player.pos, &get);
        for b in self.fx.tick(&get) {
            // `Entity.onStruckByLightning` for everything in the bolt's box.
            if effects::struck(b, self.player.pos, glam::Vec3::new(0.3, 0.9, 0.3)) {
                self.vitals.lightning();
            }
            for m in self.mobs.list.iter_mut().filter(|m| effects::struck(b, m.body.pos, m.half)) {
                m.damage(5.0, b, &mut self.mobs.rng); // ponytail: through `damage`, so it also knocks back; no burning
            }
        }
        // The rain you hear: a patter clip every few ticks where it rains (not snow, not in the Nether), dull under a roof, none deep down.
        let (px, pz) = (self.player.pos.x.floor() as i32, self.player.pos.z.floor() as i32);
        let roof = self.chunks.top_solid(px, pz) as f32 - self.camera.pos.y;
        if !self.nether && self.weather.rain(1.0) > 0.2 {
            if let Some((vol, pitch)) = self.fx.patter(self.weather.rain(1.0), roof) {
                if self.chunks.biomes(px, pz, 1, 1)[0].rains() {
                    self.audio.play_here("ambient.weather.rain", vol, pitch);
                }
            }
        }
        // `EntityLightningBolt`: thunder (pitch 0.8 + 0.2 r) and, for a near bolt, a crack (`random.explode`, 0.5 + 0.2 r). Vanilla plays the
        // thunder at volume 10000, the same loudness everywhere; 12 dies out over about 190 blocks, so a far storm is a quiet rumble.
        // Through a roof it is quieter and lower (`effects::muffle`).
        let m = effects::muffle(roof);
        for c in self.fx.heard() {
            let r = self.mobs.rng.next_float();
            self.audio.play_gain("ambient.weather.thunder", c.at, 12.0, (0.8 + r * 0.2) * (0.7 + 0.3 * m), m);
            if c.dist < 40.0 {
                self.audio.play_gain("random.explode", c.at, 2.0, (0.5 + r * 0.2) * (0.7 + 0.3 * m), m);
                self.touch.pad.rumble(250, (80.0 + 175.0 * (1.0 - c.dist / 40.0)) as u8);
            }
        }
        // Spawners see the same world as the mobs do (plants read as air).
        let get = |x: i32, y: i32, z: i32| self.chunks.block(x, y, z).map(|b| if is_plant(b) { 0 } else { self.chunks.physics_at(x, y, z, b) });
        let light = |x: i32, y: i32, z: i32| self.chunks.light(x, y, z);
        let biome = |x: i32, z: i32| self.chunks.biome_at(x, z);
        let ctx = Ctx { get: &get, light: &light, day: false, player: self.player.pos, eye: self.camera.pos, biome: &biome, sight: 1.0 };
        let chunks = &self.chunks;
        self.fx.tick_spawners(&mut self.mobs, &ctx, &|(x, _, z)| chunks.block_loaded(x, 0, z).is_some());
    }

    /// A minecart that took its last hit: removed, its drops and what its chest held on the ground.
    fn smash_cart(&mut self, ci: usize) {
        let c = self.carts.list.remove(ci);
        let cell = (c.pos.x.floor() as i32, c.pos.y.floor() as i32, c.pos.z.floor() as i32);
        c.drops().into_iter().for_each(|s| self.drops.spawn_stack(s, cell));
        self.chests.remove(&c.key()).into_iter().flatten().flatten().for_each(|s| self.drops.scatter(s, cell));
    }

    /// A boat that took its last hit: removed, 3 planks and 2 sticks dropped.
    fn smash_boat(&mut self, bi: usize) {
        let b = self.boats.list.remove(bi);
        let cell = (b.pos.x.floor() as i32, b.pos.y.floor() as i32, b.pos.z.floor() as i32);
        self.drops.spawn_stack(ItemStack { id: boat::PLANKS, count: 3, damage: 0 }, cell);
        self.drops.spawn_stack(ItemStack { id: boat::STICK, count: 2, damage: 0 }, cell);
    }

    /// `Explosion`; `flame` is `isFlaming` (a ghast's fireball): afterwards, in each destroyed cell that is now air over an opaque block, fire 1 time in 3.
    fn explode(&mut self, at: glam::Vec3, power: f32, flame: bool) {
        self.touch.pad.rumble(400, 255);
        self.audio.cry("random.explode", at, 4.0, 0.7, true); // `Explosion`: volume 4, pitch `(1 + (r - r) * 0.2) * 0.7`
        // The fireball's (`flame`) is the one with no exploder entity: no `knockBack`, only the push.
        let cells = self.mobs.explode(&|x: i32, y: i32, z: i32| self.chunks.block(x, y, z), at, power, !flame);
        // What the blast does to the boats, the carts and the player is worked out here, with the blocks still standing: `Explosion.doExplosionA` reads
        // the exposure through them before `doExplosionB` takes the cells away. (Boats and carts: `vel` is blocks a tick, and `hurt` takes 10 a point.)
        let get = |x: i32, y: i32, z: i32| self.chunks.block(x, y, z);
        let hit = |pos: glam::Vec3, half: glam::Vec3| mobs::blast(&get, at, power, pos, pos - half, pos + half);
        let boats: Vec<_> = self.boats.list.iter().map(|b| hit(b.pos, boat::HALF)).collect();
        let carts: Vec<_> = self.carts.list.iter().map(|c| hit(c.pos, minecart::HALF)).collect();
        // The player's `posY` is its eye (`yOffset` 1.62), which is what the distance and the push run from; the box is what the exposure looks at.
        let me = mobs::blast(&get, at, power, self.player.pos + glam::Vec3::Y * EYE_HEIGHT, self.player.pos - physics::HALF, self.player.pos + physics::HALF);
        let cells_fire = if flame { cells.clone() } else { Vec::new() };
        for (x, y, z) in cells {
            let Some(id) = self.chunks.block_loaded(x, y, z).filter(|&b| b > 0) else { continue };
            let meta = self.chunks.meta(x, y, z);
            if id == 46 {
                self.ticks.prime((x, y, z), true); // `BlockTNT.onBlockDestroyedByExplosion`: a chain, no drop
            } else if self.mobs.rng.next_float() < 0.3 {
                self.drops.spawn_block(id, meta, (x, y, z));
            }
            self.chunks.set_block(x, y, z, 0);
        }
        if flame {
            for &(x, y, z) in cells_fire.iter().rev() {
                let below = self.chunks.block_loaded(x, y - 1, z).unwrap_or(0);
                if self.chunks.block_loaded(x, y, z) == Some(0) && world::chunk::opaque(below) && self.mobs.rng.next_int_bound(3) == 0 {
                    self.chunks.set_block(x, y, z, 51);
                }
            }
        }
        for (i, h) in boats.into_iter().enumerate().rev() {
            if let Some((dmg, push)) = h {
                self.boats.list[i].vel += push / 20.0;
                if self.boats.list[i].hurt(dmg) {
                    self.smash_boat(i);
                }
            }
        }
        for (i, h) in carts.into_iter().enumerate().rev() {
            if let Some((dmg, push)) = h {
                self.carts.list[i].vel += push / 20.0;
                if self.carts.list[i].hurt(dmg) {
                    self.smash_cart(i);
                }
            }
        }
        // `Explosion.doExplosionA`: `attackEntityFrom` with the damage, and `motion += direction x impact`. The walk overwrites the horizontal velocity
        // every frame, so the horizontal push is carried in `kick` (see `frame`); the vertical one is the velocity's own.
        if let Some((dmg, push)) = me {
            self.vitals.hurt(dmg);
            self.kick += glam::Vec3::new(push.x, 0.0, push.z);
            self.player.vel.y += push.y;
        }
    }

    /// `EntityFireball.onUpdate`'s `attackEntityFrom(owner, 0)` on what it ran into (`Ev::Strike`): a boat or a cart rocks (`hurt(0)`: its own wobble,
    /// apart from the blast's that follows, and no damage), a painting comes off the wall. The indices are of the lists as they were when `Mobs::targets`
    /// was filled, so this runs before any blast changes them.
    fn struck(&mut self, evs: &[Ev]) {
        let mut paintings = Vec::new();
        for e in evs {
            match *e {
                Ev::Strike(Strike::Boat(i)) => drop(self.boats.list[i].hurt(0)), // damage + 0 cannot cross the 40 that breaks it
                Ev::Strike(Strike::Cart(i)) => drop(self.carts.list[i].hurt(0)),
                Ev::Strike(Strike::Painting(i)) => paintings.push(i),
                _ => {} // a lit TNT: `Entity.attackEntityFrom` only marks it hit
            }
        }
        paintings.sort_unstable();
        paintings.dedup();
        for i in paintings.into_iter().rev() {
            self.break_painting(i);
        }
    }

    /// `EntityPainting.attackEntityFrom`: it comes off the wall and drops as an item.
    fn break_painting(&mut self, i: usize) {
        let c = self.paintings.remove(i).frame().0;
        self.drops.scatter(ItemStack { id: decor::PAINTING, count: 1, damage: 0 }, (c.x.floor() as i32, c.y.floor() as i32, c.z.floor() as i32));
    }

    /// `World.isDaytime`: the sky-light subtraction is under 4.
    fn daytime(&self) -> bool {
        let angle = sky::celestial_angle(self.world_ticks as u64, 1.0);
        sky::skylight_subtracted(angle, self.weather.rain(1.0), self.weather.thunder(1.0)) < 4
    }

    /// `EntityPlayer.func_25060_a`: the bed's free cell, else the world spawn (feet 0.1 above the cell; `pos` is the box centre).
    /// `Minecraft.respawn`: a bed that is gone, or has no free cell beside it, says `tile.bed.notValid` and is forgotten (the new player
    /// has no spawn coordinate). UNVERIFIED: the text is from memory of b1.7.3's `en_US.lang`, which is not in the decompiled tree.
    /// ponytail: a bed in a chunk that is not loaded says nothing and stays remembered (the Java `prepareChunk`s it); it respawns at the world spawn.
    fn respawn_pos(&mut self) -> glam::Vec3 {
        let get = |a, b, c| self.chunks.block_loaded(a, b, c).unwrap_or(0);
        let cell = self.bed_spawn.filter(|&(x, y, z)| get(x, y, z) == 26).and_then(|b| pick::wake_cell(&get, b, self.chunks.meta(b.0, b.1, b.2), 0));
        if cell.is_none() && self.bed_spawn.is_some_and(|(x, _, z)| self.chunks.block_loaded(x, 0, z).is_some()) {
            (self.hint, self.hint_msg, self.bed_spawn) = (2.5, "Your home bed was missing or obstructed", None);
        }
        cell.map_or(self.spawn, |(x, y, z)| glam::Vec3::new(x as f32 + 0.5, y as f32 + 1.0, z as f32 + 0.5))
    }

    /// `BlockBed.blockActivated` + `EntityPlayer.sleepInBedAt` (the OK status: at night, within 3 / 2 / 3 blocks; by day NOT_POSSIBLE_NOW says so).
    /// b1.7.3 has no other message: `TOO_FAR_AWAY` returns silently (`BlockBed` only answers `NOT_POSSIBLE_NOW`), `tile.bed.occupied` needs a second
    /// player and there is no "monsters nearby" check (Beta 1.8 added it); monsters come at the end of the sleep instead (`sleep_spawn`).
    fn use_bed(&mut self, (mut x, y, mut z): (i32, i32, i32)) {
        let mut m = self.chunks.meta(x, y, z);
        if m & 8 == 0 {
            let (dx, dz) = pick::BED_DIR[(m & 3) as usize];
            (x, z) = (x + dx, z + dz);
            if self.chunks.block_loaded(x, y, z) != Some(26) {
                return;
            }
            m = self.chunks.meta(x, y, z);
        }
        let eye = self.player.pos + glam::Vec3::new(0.0, EYE_HEIGHT, 0.0);
        let near = (eye.x - x as f32).abs() <= 3.0 && (eye.y - y as f32).abs() <= 2.0 && (eye.z - z as f32).abs() <= 3.0;
        if self.sleep.is_none() && !self.vitals.dead() && self.daytime() {
            (self.hint, self.hint_msg) = (2.5, "You can only sleep at night");
        }
        if self.sleep.is_some() || self.vitals.dead() || self.daytime() || !near {
            return;
        }
        let d = m & 3;
        let (px, pz) = (if d == 1 { 0.1 } else if d == 3 { 0.9 } else { 0.5 }, if d == 0 { 0.9 } else if d == 2 { 0.1 } else { 0.5 });
        self.player.pos = glam::Vec3::new(x as f32 + px, y as f32 + 15.0 / 16.0 - EYE_HEIGHT, z as f32 + pz);
        self.player.vel = glam::Vec3::ZERO;
        // getBedOrientationInDegrees, as `rotationYaw`; our yaw is that minus 180 degrees. UNVERIFIED on a device.
        self.camera.yaw = ([90.0f32, 0.0, 270.0, 180.0][d as usize] - 180.0).to_radians();
        self.camera.pitch = 0.0;
        self.chunks.set_block_meta(x, y, z, 26, m | 4);
        self.sleep = Some((x, y, z, 0));
    }

    /// `EntityPlayer.wakeUpPlayer`: the bed is free again and the player stands beside it (`set_spawn` = the bed becomes the
    /// respawn point).
    fn wake(&mut self, set_spawn: bool) {
        let Some((x, y, z, _)) = self.sleep.take() else { return };
        let get = |a, b, c| self.chunks.block_loaded(a, b, c).unwrap_or(0);
        let m = self.chunks.meta(x, y, z);
        let cell = pick::wake_cell(&get, (x, y, z), m, 0).unwrap_or((x, y + 1, z));
        if get(x, y, z) == 26 {
            self.chunks.set_block_meta(x, y, z, 26, m & !4);
        }
        self.player.pos = glam::Vec3::new(cell.0 as f32 + 0.5, cell.1 as f32 + 1.0, cell.2 as f32 + 0.5);
        if set_spawn {
            self.bed_spawn = Some((x, y, z));
        }
    }

    /// `World.tick` -> `SpawnerAnimals.performSleepSpawning` for the sleeper at the bed (`Mobs::sleep_spawn`); the monster's cry is played.
    fn sleep_spawn(&mut self) -> bool {
        let Some((x, y, z, _)) = self.sleep else { return false };
        // Spawners see the same world as the mobs do (plants read as air).
        let get = |x: i32, y: i32, z: i32| self.chunks.block(x, y, z).map(|b| if is_plant(b) { 0 } else { self.chunks.physics_at(x, y, z, b) });
        let light = |x: i32, y: i32, z: i32| self.chunks.light(x, y, z);
        let biome = |x: i32, z: i32| self.chunks.biome_at(x, z);
        let raw = |a: i32, b: i32, c: i32| self.chunks.block_loaded(a, b, c).unwrap_or(0);
        let eye = self.player.pos + glam::Vec3::new(0.0, EYE_HEIGHT, 0.0); // asleep: `posY` is the bed's surface
        let ctx = Ctx { get: &get, light: &light, day: false, player: self.player.pos, eye, biome: &biome, sight: 1.0 };
        let beside = pick::wake_cell(&raw, (x, y, z), self.chunks.meta(x, y, z), 1);
        let mut evs = Vec::new();
        let came = self.mobs.sleep_spawn(&ctx, eye, beside, &mut evs);
        for e in evs {
            if let Ev::Snd(name, at, vol, mul, jit) = e {
                self.audio.cry(name, at, vol, mul, jit);
            }
        }
        came
    }

    /// One 20 Hz tick of sleeping: fully asleep (100 ticks) a monster may come first (`World.tick`: `performSleepSpawning`; it wakes the player
    /// without making the bed their spawn and the night stays); else everyone wakes at the next morning (no weather runs during the skip);
    /// a bed that is gone or day wakes the player earlier (`EntityPlayer.onUpdate`).
    fn sleep_tick(&mut self, sky_sub: u8) {
        let Some((x, y, z, t)) = self.sleep else { return };
        if t >= 100 {
            if self.sleep_spawn() {
                self.wake(false);
            } else {
                let next = self.world_ticks as u64 + 24000;
                self.world_ticks = (next - next % 24000) as f64;
                self.weather_ticks = self.world_ticks as u64;
                self.wake(true);
            }
        } else if self.chunks.block_loaded(x, y, z) != Some(26) {
            self.wake(false);
        } else if sky_sub < 4 {
            self.wake(true);
        } else {
            self.sleep = Some((x, y, z, t + 1));
        }
    }

    /// `BlockDoor.blockActivated` for a wooden door: the lower half's open bit flips, and the upper half follows.
    fn toggle_door(&mut self, (x, y, z): (i32, i32, i32)) {
        let y = if self.chunks.meta(x, y, z) & 8 != 0 { y - 1 } else { y };
        if self.chunks.block_loaded(x, y, z) == Some(64) {
            let m = self.chunks.meta(x, y, z);
            if self.chunks.block_loaded(x, y + 1, z) == Some(64) {
                self.chunks.set_block_meta(x, y + 1, z, 64, (m ^ 4) + 8);
            }
            self.chunks.set_block_meta(x, y, z, 64, m ^ 4);
            self.audio.door(glam::Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5)); // aux effect 1003 (silent: no such asset in b1.7.3)
        }
    }

    /// `ItemStack.useItemRightClick` for what is held and not placed: bow, rod, bucket, else food.
    fn use_held(&mut self) {
        let slot = self.touch.hotbar_slot;
        let Some(held) = self.inv.slots[slot] else { return };
        let (eye, fwd) = (self.camera.pos, self.camera.forward());
        match held.id {
            // `ItemBow`: uses one arrow from the inventory and shoots it at once (no draw time in b1.7.3).
            gear::BOW => {
                if self.inv.take_one(gear::ARROW) {
                    let v = gear::arrow_velocity(fwd, &mut self.mobs.rng);
                    self.mobs.shoot(eye + fwd * 0.5, v, 0);
                    // `ItemBow`: "random.bow" at 1.0, pitch `1 / (r * 0.4 + 0.8)` (NOT in the b1.7.3 index, see `assets/sound/extra`).
                    let pitch = 1.0 / (self.audio.rnd() * 0.4 + 0.8);
                    self.audio.play_here("random.bow", 1.0, pitch);
                }
            }
            // `ItemSnowball` / `ItemEgg`: one is used up and thrown along the look direction (same heading as an arrow).
            gear::SNOWBALL | gear::EGG => {
                self.inv.consume(slot);
                let v = gear::arrow_velocity(fwd, &mut self.mobs.rng);
                self.mobs.shoot(eye - glam::Vec3::Y * 0.1, v, held.id);
            }
            // `ItemFishingRod`: reel in what is out (a bite gives a raw fish), else cast.
            gear::ROD => match self.hook.take() {
                Some(h) => {
                    let (fish, wear) = h.catch(self.player.pos);
                    if let Some((s, v)) = fish {
                        self.drops.throw_from(s, h.pos, v);
                    }
                    self.inv.damage(slot, wear);
                }
                None => {
                    self.hook = Some(Hook::cast(eye, fwd, &mut self.mobs.rng));
                    // `ItemFishingRod`: "random.bow" (NOT in the b1.7.3 index, see `assets/sound/extra`) at 0.5, pitch `0.4 / (r * 0.4 + 0.8)`.
                    let pitch = 0.4 / (self.audio.rnd() * 0.4 + 0.8);
                    self.audio.play_here("random.bow", 0.5, pitch);
                }
            },
            gear::BUCKET | gear::WATER_BUCKET | gear::LAVA_BUCKET => self.use_bucket(slot, held.id),
            gear::BOAT => self.use_boat(slot),
            _ => self.eat(),
        }
    }

    /// `ItemBucket.onItemRightClick`: a 5 block ray; an empty bucket also hits liquids and takes a source, a full one
    /// pours into the cell beside the face it hit (air or a non-solid block).
    /// `ItemBoat.onItemUse`: the ray reaches 5 blocks and stops at water; the boat is set on top of the cell it hit.
    fn use_boat(&mut self, slot: usize) {
        let eye = self.camera.pos.as_dvec3();
        let end = eye + self.camera.forward().as_dvec3() * 5.0;
        let bounds = |x: i32, y: i32, z: i32| {
            let b = self.chunks.block_loaded(x, y, z)?;
            if (8..=11).contains(&b) { Some([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]) } else { pick_bounds(b, self.chunks.meta(x, y, z)) }
        };
        let Some(hit) = pick::ray_trace(&bounds, eye, end) else { return };
        let (x, y, z) = hit.pos;
        self.boats.list.push(boat::Boat::new(x as f32 + 0.5, y as f32 + 1.0, z as f32 + 0.5));
        self.inv.consume(slot);
    }

    /// `ItemReed.onItemUse` (as `ItemBlock`'s): the placed block's step sound at its cell, volume (1 + 1) / 2, pitch * 0.8.
    fn placed(&mut self, id: u8, (x, y, z): (i32, i32, i32)) {
        let (key, _, pitch) = audio::STEP[audio::step_of(id)];
        self.audio.play(key, glam::Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5), 1.0, pitch * 0.8);
    }

    fn use_bucket(&mut self, slot: usize, held: u16) {
        let eye = self.camera.pos.as_dvec3();
        let end = eye + self.camera.forward().as_dvec3() * 5.0;
        let empty = held == gear::BUCKET;
        let bounds = |x: i32, y: i32, z: i32| {
            let b = self.chunks.block_loaded(x, y, z)?;
            if empty && (8..=11).contains(&b) { Some([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]) } else { pick_bounds(b, self.chunks.meta(x, y, z)) }
        };
        let Some(hit) = pick::ray_trace(&bounds, eye, end) else { return };
        let (x, y, z) = hit.pos;
        let filled = self.chunks.block_loaded(x, y, z).and_then(|b| gear::bucket_fill(b, self.chunks.meta(x, y, z)));
        if let (true, Some(full)) = (empty, filled) {
            self.chunks.set_block(x, y, z, 0);
            self.inv.slots[slot] = Some(ItemStack { id: full, count: 1, damage: 0 });
        } else if let Some(fluid) = gear::bucket_pour(held) {
            let (x, y, z) = pick::place_pos(&hit);
            if self.chunks.block_loaded(x, y, z).is_some_and(pick::replaceable) && y < 128 {
                if self.nether && fluid == 8 {
                    // `ItemBucket`: water does not stay in the Nether; it fizzes away (ponytail: the 8 smoke puffs are not drawn).
                    let p = 2.6 + (self.audio.rnd() - self.audio.rnd()) * 0.8;
                    self.audio.play("random.fizz", glam::Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5), 0.5, p);
                } else {
                    self.chunks.set_block_meta(x, y, z, fluid, 0);
                }
                self.inv.slots[slot] = Some(ItemStack { id: gear::BUCKET, count: 1, damage: 0 });
            }
        }
    }

    fn eat(&mut self) {
        // Cortes Combat C4 (NOT b1.7.3, only with the Immersive Pack ON): food is a meal that takes `EAT_TICKS` and feeds the healing bar
        // (`eat_tick`), not a heal on the tap. ponytail: a tap starts it and it runs by itself; the mod holds the button.
        if world::combat::rules::on() {
            let slot = self.touch.hotbar_slot;
            if self.eating.is_none() && self.inv.slots[slot].is_some_and(|s| items::heal_amount(s.id).is_some()) {
                self.eating = Some(Eating { left: EAT_TICKS, slot, health: self.vitals.health });
            }
            return;
        }
        if let Some(n) = self.inv.eat(self.touch.hotbar_slot) {
            self.vitals.heal(n);
            self.vitals.fed = world::biomelife::FED_TICKS; // NOT b1.7.3 (`biomeRules`): food for the cold
        }
    }

    /// `EntityPlayer.onDeath`: the open screen's cursor and grid, then the whole inventory, fall to the ground where he
    /// died (ponytail: `scatter` drops one entity per stack, vanilla throws each with a random spread). The pause
    /// menu is the death screen: its resume button respawns (top of `step_frame`).
    fn die(&mut self) {
        self.close_screen();
        self.close_sign();
        let at = (self.player.pos.x.floor() as i32, self.player.pos.y.floor() as i32, self.player.pos.z.floor() as i32);
        for slot in self.inv.slots.iter_mut() {
            if let Some(st) = slot.take() {
                self.drops.scatter(st, at);
            }
        }
        self.touch.pause();
    }

    /// The sign editor is done: the text stays on the sign (it is edited in place).
    fn close_sign(&mut self) {
        if self.sign_edit.take().is_some() {
            self.touch.set_screen(false);
        }
    }

    /// A tap on the sign editor: a key of the on-screen keyboard or a line of the board (`GuiEditSign.keyTyped`).
    fn on_sign_tap(&mut self, x: f32, y: f32) {
        let (w, h) = { let (w, h) = self.be.size(); (w as f32, h as f32) };
        let Some(mut e) = self.sign_edit else { return };
        let Some(k) = decor::key_at(w, h, e.shift, x, y) else { return };
        let done = self.signs.get_mut(&e.pos).map_or(k == Key::Done, |t| e.press(k, t));
        self.sign_edit = Some(e);
        if done {
            self.close_sign();
        }
    }

    /// Open a container screen: the 2x2 inventory, a workbench's 3x3, a furnace or chests.
    fn open_screen(&mut self, screen: Screen) {
        self.screen = Some(screen);
        (self.one_mode, self.shift_mode) = (false, false);
        self.touch.set_screen(true);
    }

    /// `onCraftGuiClosed`: the cursor stack and whatever is left in the grid are thrown out in front of the player.
    fn close_screen(&mut self) {
        let was_sat = self.sat.take().is_some();
        if let Some(mut s) = self.screen.take() {
            if was_sat {
                // The satchel puts the grid and the held stack back in the bag; only what does not fit is thrown.
                if let Some(mut c) = s.cursor.take() {
                    self.inv.add(&mut c);
                    s.cursor = (c.count > 0).then_some(c);
                }
                book::stash(&mut s, &mut self.inv);
            }
            for st in s.close() {
                self.drops.throw(st, self.camera.pos, self.camera.forward());
            }
        }
        self.touch.set_screen(false);
        if std::mem::take(&mut self.mat_ui) {
            // The pack's cloth rolls itself up (it is removed when `pack_tick` sees it done).
            if let Some(m) = self.mat.as_mut() {
                m.dir = -1;
            }
            self.audio.play_here("step.cloth", 0.6, 0.8);
        }
    }

    /// The inventory button with the immersive pack on and nothing open: take the pack in both hands (the same button puts it away
    /// again), or, with the cloth already out and still unrolling, roll it up again.
    fn pack_button(&mut self) {
        if let Some(m) = self.mat.as_mut() {
            if m.dir > 0 {
                m.dir = -1;
            }
            return;
        }
        self.carry = match self.carry {
            Some(_) => None,
            None if self.riding() || self.sleep.is_some() || self.vitals.dead() => None,
            None => Some(0.0),
        };
    }

    /// The place tap while the pack is carried: spread the cloth where the preview is green; anywhere else only the message shows.
    fn put_pack(&mut self) {
        match self.ghost.take() {
            Some((m, true)) => {
                (self.mat, self.carry) = (Some(m), None);
                (self.pack_focus, self.pack_goal) = (pack::tab_focus(0), None);
                self.audio.play_here("step.cloth", 0.7, 1.0);
            }
            _ => (self.hint, self.hint_msg) = (2.5, "Look at flat, clear ground to spread your pack"),
        }
    }

    /// The cloth's life, once a frame: it unrolls and the screen opens on it; it rolls up and goes; it rolls itself up when the player
    /// walks off or the ground under it goes (the screen closes first, its stacks are thrown as always).
    fn pack_tick(&mut self, dt: f32) {
        self.hint = (self.hint - dt).max(0.0);
        // The satchel's clocks, the finger's (a long press on the result), and the book's notes of what the bag has held.
        if let Some(g) = self.gesture.as_mut() {
            g.age += dt;
        }
        if let Some(s) = self.sat.as_mut() {
            s.tick(dt);
        }
        if self.sat.is_some() && (self.vitals.dead() || self.sleep.is_some()) {
            self.close_screen();
        }
        let grid = self.screen.as_ref().map_or([None; 9], |s| s.grid);
        self.notes.observe(&self.inv, &grid);
        // The open screen's two clocks: since it opened, and the cursor stack's lift (up in 0.12 s).
        let held = self.mat_ui && self.screen.as_ref().is_some_and(|s| s.cursor.is_some());
        self.pack_age = if self.mat_ui { self.pack_age + dt } else { 0.0 };
        self.pack_lift += (f32::from(u8::from(held)) - self.pack_lift).clamp(-dt / 0.12, dt / 0.12);
        self.touch.pack_open = self.mat_ui || self.sat.is_some(); // the four HUD buttons are the table's own (or the satchel's) while it is open
        if let Some(g) = self.pack_goal {
            // A tab glides the camera: it closes 12 / s of the way to it, and lands when it is half a unit off.
            let k = 1.0 - (-dt * 12.0).exp();
            let (f, d) = (self.pack_focus, (g.0 - self.pack_focus.0, g.1 - self.pack_focus.1));
            (self.pack_focus, self.pack_goal) = if d.0.hypot(d.1) < 0.5 { (g, None) } else { ((f.0 + d.0 * k, f.1 + d.1 * k), Some(g)) };
        }
        if self.riding() || self.sleep.is_some() || self.vitals.dead() || self.screen.is_some() {
            self.carry = None; // a screen opens only by a tap, which a carried pack swallows; a ride, a bed or death put it away
        }
        self.ghost = None;
        if let Some(t) = self.carry.as_mut() {
            *t = (*t + dt / pack::CARRY_SECS).min(1.0);
            let chunks = &self.chunks;
            let air = |x: i32, y: i32, z: i32| chunks.block_loaded(x, y, z).is_some_and(|b| b == 0 || (is_plant(b) && b != 51)); // grass and flowers do not stop a cloth (they poke through it), fire does
            let floor = |x: i32, y: i32, z: i32| chunks.block_loaded(x, y, z).is_some_and(normal_cube);
            // ponytail: the target is the last frame's (`pack_tick` runs before the pick), a one-frame lag on the preview.
            self.ghost = self.target.filter(|h| h.face == 1).map(|h| {
                let m = pack::Mat::plan(h.point.as_vec3(), self.camera.pos, h.pos.1 + 1);
                (m, m.fits(&air, &floor))
            });
        }
        let Some(mut m) = self.mat.take() else {
            if self.mat_ui {
                self.close_screen(); // the cloth is gone (another dimension): the screen has nothing to lie on
            }
            return;
        };
        let chunks = &self.chunks;
        let floor = |x: i32, y: i32, z: i32| chunks.block_loaded(x, y, z).is_some_and(normal_cube);
        if m.dir >= 0 && !m.valid(self.camera.pos, &floor) {
            m.dir = -1;
            self.close_screen(); // `self.mat` is empty just now, so this only closes the screen
        }
        if m.dir == 0 && !self.mat_ui {
            m.dir = -1;
        }
        match m.tick(dt) {
            pack::Step::Idle => self.mat = Some(m),
            pack::Step::Gone => {}
            pack::Step::Open => {
                if self.screen.is_none() && self.sign_edit.is_none() && !self.vitals.dead() {
                    self.open_screen(Screen::new(2));
                    self.mat_ui = true;
                } else {
                    m.dir = -1;
                }
                self.mat = Some(m);
            }
        }
    }

    /// Open the satchel (`satchel.rs`) on `screen`: the 2x2 inventory (the book closed) or a workbench's 3x3 (the book open).
    fn open_satchel(&mut self, screen: Screen) {
        let (bench, gw) = (screen.gw == 3, screen.gw);
        self.open_screen(screen);
        self.sat = Some(satchel::Satchel::new(bench));
        // A pad's cursor starts on the selected quick-belt slot, where it will lie once the bag has risen, not in the middle of the screen.
        let mut settled = satchel::Satchel::new(bench);
        settled.tick(10.0);
        let (w, h) = self.be.size();
        let f = satchel::Frame::new(&settled, &self.inv, gw, w as f32, h as f32);
        if let Some((_, r)) = f.cells.iter().find(|(id, _)| *id == craft::SlotId::Inv(self.touch.hotbar_slot)) {
            self.touch.cursor_pos = (r[0] + r[2] * 0.5, r[1] + r[3] * 0.5);
        }
        self.audio.play_here("step.cloth", 0.6, 1.0);
    }

    /// A d-pad step in the open satchel: the cursor jumps to the neighbouring target (`satchel::step`), so there is no stick to push
    /// from a slot to a button. At the foot or head of the recipe list nothing lies that way: the list scrolls a row and the cursor steps on.
    fn sat_nav(&mut self, d: (i32, i32)) {
        let (w, h) = self.be.size();
        let at = self.touch.cursor_pos;
        if self.sat.is_none() {
            // Chest, furnace, bench, the 2x2 screen (no satchel) and the pause menu: the slots and the two toggles, or the buttons.
            let t: Vec<[f32; 4]> = match self.screen.as_ref() {
                Some(s) => {
                    let (w, h) = (w as f32, h as f32);
                    let cell = |(ux, uy)| craft::cell(s.gw, w, h, (ux, uy));
                    let slots = craft::layout(s.gw).into_iter().map(|(_, ux, uy)| cell((ux, uy)));
                    let togs = (0..2).map(|i| craft::toggle(s.gw, w, h, i));
                    slots.chain(togs).map(|(x, y, k)| [x, y, k, k]).collect()
                }
                None => match self.pause.as_ref() {
                    Some(p) => p.targets(w as f32, h as f32, &self.opts, &self.pause_info()),
                    None => Vec::new(),
                },
            };
            if let Some(r) = satchel::step(&t, at, d) {
                self.touch.warp_cursor((r[0] + r[2] * 0.5, r[1] + r[3] * 0.5));
            }
            return;
        }
        let targets = |me: &Self| -> Vec<[f32; 4]> {
            match (me.sat.as_ref(), me.screen.as_ref()) {
                (Some(sat), Some(s)) => satchel::Frame::with_grid(sat, &me.inv, &s.grid, s.gw, w as f32, h as f32).targets(sat),
                _ => Vec::new(),
            }
        };
        let mut to = satchel::step(&targets(&*self), at, d);
        if to.is_none() && d.0 == 0 && matches!(self.sat_hit(at.0, at.1), Some(satchel::Pick::Row(_))) {
            let max = self.sat_max_scroll(false);
            let row_h = match (self.sat.as_ref(), self.screen.as_ref()) {
                (Some(sat), Some(s)) => satchel::Frame::with_grid(sat, &self.inv, &s.grid, s.gw, w as f32, h as f32).book.map_or(0.0, |b| b.row_h),
                _ => 0.0,
            };
            if let Some(sat) = self.sat.as_mut() {
                sat.scroll = (sat.scroll + d.1 as f32 * row_h).clamp(0.0, max);
            }
            let t = targets(&*self);
            to = satchel::step(&t, at, d).or_else(|| satchel::under(&t, at)); // the row it is on, when the list could not move
        }
        if let Some(r) = to {
            self.touch.warp_cursor((r[0] + r[2] * 0.5, r[1] + r[3] * 0.5));
        }
    }

    /// The pad's other keys in a container screen: B steps back (a category to the category list) before it closes, LB / RB turn the
    /// book's page through the categories, the right stick scrolls the recipe card's ingredients under the cursor, else the list.
    fn sat_pad_keys(&mut self) {
        let (back, page, px) = (self.touch.take_back(), self.touch.take_page(), self.touch.take_scroll());
        let at = self.touch.cursor_pos;
        let pick = self.sat_hit(at.0, at.1);
        let (card, max) = (matches!(pick, Some(satchel::Pick::Card)), |me: &Self, c| me.sat_max_scroll(c));
        let (mc, ml) = (max(self, true), max(self, false));
        if let Some(s) = self.sat.as_mut().filter(|s| s.open()) {
            if page != 0 {
                let n = book::CATS.len() as i32;
                let to = s.cat.map_or(if page > 0 { 0 } else { n - 1 }, |i| (i as i32 + page).rem_euclid(n));
                s.turn(Some(to as usize));
            }
            if px != 0.0 {
                if card { s.card_scroll = (s.card_scroll + px).clamp(0.0, mc) } else { s.scroll = (s.scroll + px).clamp(0.0, ml) }
            }
        }
        if back {
            match self.sat.as_mut().filter(|s| s.open() && s.cat.is_some()) {
                Some(s) => s.turn(None),
                None => self.touch.press_inventory(),
            }
        }
    }

    /// The pad's X (`shift` false) or RT (`shift` true) in a container screen: the slot under the cursor is clicked for one (place one, or
    /// take the larger half) or quick-moved, as the "One" and "Shift" buttons do, but for this press only and without moving the cursor to
    /// them. Nothing off a slot, or while a press is down (a held A may be spreading a stack, or counting toward craft-all).
    fn pad_alt(&mut self, shift: bool) {
        let at = self.touch.cursor_pos;
        let Some(id) = self.slot_under(at.0, at.1).filter(|_| self.gesture.is_none()) else { return };
        self.with_ext(|s, inv, ext| if shift { s.shift_click(id, inv, ext) } else { s.click(id, true, inv, ext) });
    }

    /// What the finger is on in the open satchel (`satchel::Frame::hit`); `None` when it is not open.
    fn sat_hit(&self, x: f32, y: f32) -> Option<satchel::Pick> {
        let (sat, s) = (self.sat.as_ref()?, self.screen.as_ref()?);
        let (w, h) = self.be.size();
        Some(satchel::Frame::with_grid(sat, &self.inv, &s.grid, s.gw, w as f32, h as f32).hit(sat, x, y))
    }

    /// How far the book's list (or the recipe card's ingredients) can scroll (px).
    fn sat_max_scroll(&self, card: bool) -> f32 {
        let (Some(sat), Some(s)) = (self.sat.as_ref(), self.screen.as_ref()) else { return 0.0 };
        let (w, h) = self.be.size();
        satchel::Frame::with_grid(sat, &self.inv, &s.grid, s.gw, w as f32, h as f32).book.map_or(0.0, |b| if card { b.card_max } else { b.max_scroll })
    }

    /// A tap on the open satchel: a button, a page of the book, a recipe (it fills the grid), a slot, or past it all (a held stack is thrown).
    fn on_sat_tap(&mut self, x: f32, y: f32) {
        use satchel::{Btn, Pick};
        let Some(p) = self.sat_hit(x, y) else { return };
        match p {
            Pick::Btn(Btn::Close) => self.close_screen(),
            Pick::Btn(Btn::Book) => {
                // Closing the book puts what is in its grid back in the bag.
                let closing = self.sat.as_mut().map_or(false, |s| {
                    s.book = !s.book;
                    !s.book
                });
                if let (true, Some(s)) = (closing, self.screen.as_mut()) {
                    book::stash(s, &mut self.inv);
                }
            }
            Pick::Btn(Btn::Sort) => {
                pack::sort(&mut self.inv);
                self.audio.play_here("step.cloth", 0.5, 1.2);
            }
            Pick::Btn(Btn::Shift) => self.shift_mode = !self.shift_mode,
            Pick::Btn(Btn::One) => self.one_mode = !self.one_mode,
            Pick::Cat(i) => {
                if let Some(s) = self.sat.as_mut() {
                    s.turn(Some(i));
                }
            }
            Pick::Back => {
                if let Some(s) = self.sat.as_mut() {
                    s.turn(None);
                }
            }
            Pick::Row(i) => self.sat_pick(i),
            Pick::Pin => {
                if let Some(sel) = self.sat.as_ref().and_then(|s| s.sel) {
                    self.notes.pinned = if self.notes.pinned == Some(sel) { None } else { Some(sel) };
                }
            }
            Pick::Hide => {
                self.opts.book_discovery = !self.opts.book_discovery;
                self.opts.save();
            }
            Pick::Slot(id) => {
                let (one, shift) = (self.one_mode, self.shift_mode);
                self.with_ext(|s, inv, ext| if shift { s.shift_click(id, inv, ext) } else { s.click(id, one, inv, ext) });
            }
            Pick::Outside => {
                let Some(s) = self.screen.as_mut() else { return };
                // Click outside the window (slot -999): throw the cursor stack, or one item of it.
                if let Some(c) = s.cursor {
                    let n = if self.one_mode { 1 } else { c.count };
                    s.cursor = (c.count > n).then(|| ItemStack { count: c.count - n, ..c });
                    self.drops.throw(ItemStack { count: n, ..c }, self.camera.pos, self.camera.forward());
                }
            }
            Pick::List | Pick::Panel | Pick::Card => {}
        }
    }

    /// A tap on a recipe of the book's list: it is picked, and when the bag has it all and the grid fits, its ingredients go into the grid
    /// (what lay there goes back to the bag). A recipe the player has not met yet (Unknown recipes: hidden) is picked but not filled.
    fn sat_pick(&mut self, i: usize) {
        let Some(e) = book::entries().get(i) else { return };
        if let Some(s) = self.sat.as_mut() {
            s.sel = Some(i);
            s.card_scroll = 0.0;
        }
        if self.opts.book_discovery && !self.notes.known(e) {
            return;
        }
        let filled = match self.screen.as_mut() {
            Some(s) if s.cursor.is_none() => book::fill(e, s, &mut self.inv),
            _ => false,
        };
        if filled {
            self.audio.play_here("step.cloth", 0.4, 1.4);
        }
    }

    /// The slot under a finger of the open screen: a cell of the pack's cloth, or a slot of the vanilla panel.
    fn slot_under(&self, x: f32, y: f32) -> Option<craft::SlotId> {
        let (w, h) = { let (w, h) = self.be.size(); (w as f32, h as f32) };
        let gw = self.screen.as_ref()?.gw;
        if self.sat.is_some() {
            return match self.sat_hit(x, y)? {
                satchel::Pick::Slot(id) => Some(id),
                _ => None,
            };
        }
        if self.mat_ui {
            // The finger is a ray at the cloth: the cubes are tested as boxes, everything flat as the cloth unit it lands on (`pack::pick`).
            let (m, lays) = (self.mat.as_ref()?, self.pack_scene.as_ref().map_or(&[][..], |s| &s.lays[..]));
            pack::pick(&self.inv, lays, m, &m.rig(w / h, self.pack_focus), w / h, (w, h), (x, y))
        } else {
            craft::slot_at(gw, w, h, x, y)
        }
    }

    /// What lies on the open cloth this frame: the patches, the stacks (coming up one by one), and the cursor stack hanging over the finger.
    fn pack_scene_now(&self) -> Option<pack::Scene> {
        let s = self.screen.as_ref().filter(|_| self.mat_ui && self.mat.is_some())?;
        let held = s.cursor.zip(self.cloth_at(self.touch.cursor_pos, pack::REST)).map(|(st, at)| (st, at, self.pack_lift));
        Some(pack::scene(&self.inv, &|id| s.get(&self.inv, None, id), self.touch.hotbar_slot, (self.one_mode, self.shift_mode), self.pack_age, held, self.pack_focus))
    }

    /// The cloth unit under a finger of the open pack screen, on the plane `h` blocks up (`pack::Mat::touch`).
    fn cloth_at(&self, at: (f32, f32), h: f32) -> Option<(f32, f32)> {
        let (w, hh) = { let (w, hh) = self.be.size(); (w as f32, hh as f32) };
        let m = self.mat.as_ref()?;
        m.touch(&m.rig(w / hh, self.pack_focus), w / hh, (w, hh), at, h)
    }

    /// The 3D button of the open pack under a finger (`pack::btn_at`), if any.
    fn pack_btn(&self, x: f32, y: f32) -> Option<pack::Btn> {
        let (a, b) = self.cloth_at((x, y), pack::BTN_H)?;
        pack::btn_at(self.pack_focus, a, b)
    }

    /// A tap on a 3D button: close the pack, the two toggles, sort the main slots, or glide the camera to a part of the table.
    fn pack_press(&mut self, b: pack::Btn) {
        match b {
            pack::Btn::Close => self.close_screen(),
            pack::Btn::Shift => self.shift_mode = !self.shift_mode,
            pack::Btn::One => self.one_mode = !self.one_mode,
            pack::Btn::Sort => {
                pack::sort(&mut self.inv);
                self.audio.play_here("step.cloth", 0.6, 1.2);
            }
            pack::Btn::Tab(i) => self.pack_goal = Some(pack::tab_focus(i)),
        }
    }

    /// `BlockDispenser.dispenseItem`: one item of a random filled slot (`getRandomStackFromInventory`: each filled slot replaces the
    /// pick with chance 1 / n) leaves through the front: an arrow, egg or snowball flies (speed 1.1, spread 6), anything else drops.
    fn dispense(&mut self, (x, y, z): (i32, i32, i32)) {
        let (dx, dz) = match self.chunks.meta(x, y, z) { 3 => (0.0, 1.0), 2 => (0.0, -1.0), 5 => (1.0, 0.0), _ => (-1.0, 0.0) };
        let Some(c) = self.chests.get_mut(&(x, y, z)) else { return };
        let at = glam::Vec3::new(x as f32, y as f32, z as f32); // the sounds are at the block's corner, as the Java passes the ints
        let (mut pick, mut n) = (None, 1);
        for (i, s) in c[..9].iter().enumerate() {
            if s.is_some() {
                if self.mobs.rng.next_int_bound(n) == 0 {
                    pick = Some(i);
                }
                n += 1;
            }
        }
        let Some(i) = pick else {
            self.audio.play("random.click", at, 1.0, 1.2); // an empty dispenser only clicks
            return;
        };
        let Some(s) = c[i].as_mut() else { return };
        let (id, damage) = (s.id, s.damage);
        s.count -= 1;
        if s.count == 0 {
            c[i] = None;
        }
        let pos = glam::Vec3::new(x as f32 + dx * 0.6 + 0.5, y as f32 + 0.5, z as f32 + dz * 0.6 + 0.5);
        let mut g = |sd: f32| self.mobs.rng.next_gaussian() as f32 * 0.0075 * sd;
        if matches!(id, gear::ARROW | gear::SNOWBALL | gear::EGG) {
            let v = glam::Vec3::new(dx, 0.1, dz).normalize() * 1.1 + glam::Vec3::new(g(6.0), g(6.0), g(6.0));
            self.mobs.shoot(pos, v, if id == gear::ARROW { 0 } else { id });
            self.audio.play("random.bow", at, 1.0, 1.2); // NOT in the b1.7.3 index, see `assets/sound/extra`
        } else {
            let s = self.mobs.rng.next_double() as f32 * 0.1 + 0.2;
            let mut g = |sd: f32| self.mobs.rng.next_gaussian() as f32 * 0.0075 * sd;
            let v = glam::Vec3::new(dx * s + g(6.0), 0.2 + g(6.0), dz * s + g(6.0));
            self.drops.throw_from(ItemStack { id, count: 1, damage }, pos - glam::Vec3::Y * 0.3, v);
            self.audio.play("random.click", at, 1.0, 1.0);
        }
    }

    /// The slots behind the open screen: a furnace's 3, or the chests' 27 each (upper half first); `None` for the 2x2 and the workbench.
    fn ext(&self, s: &Screen) -> Option<Vec<Option<ItemStack>>> {
        if let Some(f) = s.furnace.and_then(|p| self.furnaces.get(&p)) {
            return Some(f.slots.to_vec());
        }
        (!s.chests.is_empty()).then(|| s.chests.iter().flat_map(|p| self.chests.get(p).copied().unwrap_or_default().into_iter().take(if s.gw == 9 { 9 } else { chest::SIZE })).collect())
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
            for (p, part) in s.chests.iter().zip(e.chunks(if s.gw == 9 { 9 } else { chest::SIZE })) {
                if let Some(c) = self.chests.get_mut(p) {
                    c[..part.len()].copy_from_slice(part);
                }
            }
        }
        self.screen = Some(s);
        Some(r)
    }

    /// A finger event on the open screen: a press that lifts where it began is a tap; one that drags over other
    /// slots with a stack on the cursor spreads it, one item per slot (the start slot included).
    fn on_screen_event(&mut self, ev: ScreenEv) {
        if self.sign_edit.is_some() {
            if let ScreenEv::Up(x, y) = ev {
                self.on_sign_tap(x, y);
            }
            return;
        }
        if self.screen.is_none() {
            return;
        }
        let mut fill = Vec::new();
        match ev {
            ScreenEv::Down(x, y) => {
                // On the pack's cloth a press on a 3D button is only a tap; one on empty cloth or ground may become a pan of the camera.
                // In the satchel anything but a slot is a tap (a button, a page); a press on the book's list may become a scroll.
                let pick = self.sat_hit(x, y);
                let btn = (self.mat_ui && self.pack_btn(x, y).is_some()) || pick.is_some_and(|p| !matches!(p, satchel::Pick::Slot(_)));
                let list = match (self.sat.as_ref(), pick) {
                    (Some(s), Some(satchel::Pick::Row(_) | satchel::Pick::List)) => Some((false, s.scroll)),
                    (Some(s), Some(satchel::Pick::Card)) => Some((true, s.card_scroll)),
                    _ => None,
                };
                let start = if btn { None } else { self.slot_under(x, y) };
                let on_cell = start.is_some_and(|id| pack::has_cell(&self.inv, id));
                let grab = if self.mat_ui && !btn && !on_cell { self.cloth_at((x, y), pack::REST) } else { None };
                self.gesture = Some(Gesture { start, spread: false, visited: Vec::new(), btn, grab, moved: false, down: (x, y), age: 0.0, list });
            }
            ScreenEv::Move(x, y) => {
                // A drag that began on the book's list scrolls it; lifting after one is not a tap.
                if let Some(((card, s0), down, moved)) = self.gesture.as_ref().and_then(|g| g.list.map(|l| (l, g.down, g.moved))) {
                    let moved = moved || (x - down.0).hypot(y - down.1) > PAN_SLOP;
                    if moved {
                        let max = self.sat_max_scroll(card);
                        if let Some(s) = self.sat.as_mut() {
                            let to = (s0 - (y - down.1)).clamp(0.0, max);
                            if card { s.card_scroll = to } else { s.scroll = to }
                        }
                    }
                    if let Some(g) = self.gesture.as_mut() {
                        g.moved = moved;
                    }
                    return;
                }
                if let Some((btn, grab, moved, down)) = self.gesture.as_ref().filter(|g| g.btn || g.grab.is_some()).map(|g| (g.btn, g.grab, g.moved, g.down)) {
                    // A drag that began on empty cloth or ground slides the camera over the table so the spot stays under the finger.
                    if let (false, Some(grab)) = (btn, grab) {
                        let moved = moved || (x - down.0).hypot(y - down.1) > PAN_SLOP;
                        if let (true, Some(now)) = (moved, self.cloth_at((x, y), pack::REST)) {
                            let f = self.pack_focus;
                            (self.pack_focus, self.pack_goal) = (pack::clamp_focus((f.0 + grab.0 - now.0, f.1 + grab.1 - now.1)), None);
                        }
                        if let Some(g) = self.gesture.as_mut() {
                            g.moved = moved;
                        }
                    }
                    return;
                }
                let under = self.slot_under(x, y);
                let (Some(g), Some(cur)) = (self.gesture.as_mut(), under) else { return };
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
                let g = self.gesture.take();
                // The satchel's result held for a while crafts all (`book::craft_all`); anything else lifts as a tap.
                let hold = self.sat.is_some() && g.as_ref().is_some_and(|g| g.start == Some(craft::SlotId::Result) && g.age >= satchel::HOLD_SECS && (x - g.down.0).hypot(y - g.down.1) < PAN_SLOP);
                if hold {
                    let n = self.screen.as_mut().map_or(0, |s| book::craft_all(s, &mut self.inv));
                    if n > 0 {
                        self.audio.play_here("step.cloth", 0.5, 1.3);
                    }
                } else if !g.is_some_and(|g| g.spread || g.moved) {
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
        if self.sat.is_some() {
            self.on_sat_tap(x, y);
            return;
        }
        let (w, h) = { let (w, h) = self.be.size(); (w as f32, h as f32) };
        let Some(gw) = self.screen.as_ref().map(|s| s.gw) else { return };
        let one = self.one_mode;
        if let Some(b) = self.pack_btn(x, y).filter(|_| self.mat_ui) {
            self.pack_press(b);
            return;
        }
        let (on_one, on_shift, inside) = if self.mat_ui {
            let at = self.cloth_at((x, y), pack::REST);
            (false, false, at.is_some_and(|(a, b)| pack::in_mat(a, b)))
        } else {
            (craft::on_mode_button(gw, w, h, x, y), craft::on_shift_button(gw, w, h, x, y), craft::in_panel(gw, w, h, x, y))
        };
        if on_one {
            self.one_mode = !one;
        } else if on_shift {
            self.shift_mode = !self.shift_mode;
        } else if let Some(id) = self.slot_under(x, y) {
            let shift = self.shift_mode;
            self.with_ext(|s, inv, ext| if shift { s.shift_click(id, inv, ext) } else { s.click(id, one, inv, ext) });
        } else if !inside {
            let Some(s) = self.screen.as_mut() else { return };
            // Click outside the window (slot -999): throw the cursor stack, or one item of it.
            if let Some(c) = s.cursor {
                let n = if self.one_mode { 1 } else { c.count };
                s.cursor = (c.count > n).then(|| ItemStack { count: c.count - n, ..c });
                self.drops.throw(ItemStack { count: n, ..c }, self.camera.pos, self.camera.forward());
            }
        }
    }

    /// On a pig, in a boat or in a cart.
    /// The armor the player wears for `render::player` (`RenderPlayer.setArmorModel`): `armorInventory[3 - slot]`, the slots after `MAIN`.
    fn worn(&self) -> render::player::Worn {
        std::array::from_fn(|slot| {
            let s = self.inv.slots[world::items::MAIN + 3 - slot]?;
            world::craft::armor(s.id).filter(|a| a.0 == slot).map(|a| a.1 as u8)
        })
    }

    fn riding(&self) -> bool {
        self.mobs.list.iter().any(|m| m.ridden) || self.boats.ridden().is_some() || self.carts.ridden().is_some()
    }

    /// One frame; in the third-person view the camera sits behind the eye for it and is put back after (input reads the eye).
    fn render(&mut self) {
        self.render_frame();
        if let Some((p, yaw, pitch, fov)) = self.pack_saved.take() {
            (self.camera.pos, self.camera.yaw, self.camera.pitch, self.camera.fov_y) = (p, yaw, pitch, fov);
        }
        if let Some((p, fov)) = self.third_saved.take() {
            (self.camera.pos, self.camera.fov_y) = (p, fov);
        }
    }

    /// The shoulder-surf shot for this frame (`render::shoulder`): where the camera sits, the field-of-view scale and the view-space turn
    /// that keeps the crosshair on what the eye's pick ray hits.
    fn shoulder_shot(&mut self) -> render::shoulder::Shot {
        let (eye, chunks) = (self.camera.pos, &self.chunks);
        let solid = |p: glam::Vec3| chunks.block_loaded(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32).is_some_and(|b| crate::world::chunk::collision(b).is_some()); // leaves, glass, slabs, fences too, not only full opaque cubes
        let aim = self.target.map(|h| (h.point.as_vec3() - eye).length());
        let dt = Instant::now().duration_since(self.frame_prev).as_secs_f32().min(0.1);
        if self.touch.take_swap() {
            self.shoulder.swap();
        }
        self.shoulder.update(&render::shoulder::Frame {
            dt,
            eye,
            yaw: self.camera.yaw,
            pitch: self.camera.pitch,
            vel: self.player.vel,
            on_ground: self.player.on_ground,
            sneak: self.touch.sneak && self.sleep.is_none(),
            sprint: self.sprint.on(),
            zoom: self.touch.zooming(),
            health: self.vitals.health,
            aim,
            solid: &solid,
        })
    }

    /// `EntityRenderer.orientCamera` for `thirdPersonView`: 4 blocks behind the eye, pulled in before terrain.
    /// ponytail: one ray in 0.1 block steps (the Java casts 8 around it).
    fn third_pos(&self) -> glam::Vec3 {
        let (eye, back) = (self.camera.pos, -self.camera.forward());
        let mut t = 0.1;
        while t <= 4.0 {
            let p = eye + back * t;
            if self.chunks.block_loaded(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32).is_some_and(normal_cube) {
                return eye + back * (t - 0.2).max(0.0);
            }
            t += 0.1;
        }
        eye + back * 4.0
    }

    /// One frame: the scene pass (sky, terrain, far terrain, entities, water, rain; 4x MSAA resolved into `Present`'s 8-bit scene) and then
    /// its one full-screen pass (CAS + upscale + vignette + the HUD) onto the swap chain. Two render passes in all; the old chain had a
    /// shadow map, a G-buffer, a deferred pass, the light shafts and an HDR post chain of ~8 passes.
    fn render_frame(&mut self) {
        if self.menu.is_some() {
            self.render_menu();
            return;
        }
        let t0 = Instant::now();
        self.step_frame();
        if self.menu.is_some() {
            return; // "Save and quit to title" just ran
        }
        let (mut rig_view, mut close) = (glam::Mat4::IDENTITY, false); // the shoulder-surf shot's roll / convergence / kicks, composed into `camera.bob` below
        if self.touch.third {
            self.third_saved = Some((self.camera.pos, self.camera.fov_y));
            if self.touch.shoulder {
                let shot = self.shoulder_shot();
                (self.camera.pos, rig_view, close) = (shot.pos, shot.view, shot.close);
                self.camera.fov_y *= shot.fov_scale;
            } else {
                self.camera.pos = self.third_pos();
                self.shoulder.reset();
            }
        } else {
            self.shoulder.reset();
        }
        // The player's own model, drawn only from behind (`RenderPlayer`; the first-person view has the hand instead).
        let tick_partial = (self.body_acc / dig::TICK).clamp(0.0, 1.0);
        let player_view = (self.touch.third && !close).then(|| {
            let riding = self.riding();
            let held = self.inv.slots[self.touch.hotbar_slot];
            let (pose, yaw) = self.body.pose(tick_partial, self.camera.pitch.to_degrees(), self.touch.sneak && self.sleep.is_none(), riding, held.is_some());
            let feet = self.player.pos - glam::Vec3::new(0.0, physics::HALF.y, 0.0);
            render::player::View { root: render::player::root(feet, yaw), pose, held, worn: self.worn(), at: self.player.pos }
        });
        // Options: the render distance (Video Settings) and the far terrain switch (Options).
        let rd = self.opts.chunks();
        if self.chunks.radius() != rd {
            self.chunks.set_radius(rd);
        }
        let lod_on = self.opts.lod && self.lod.is_some();
        self.lod_on = lod_on;
        // Real water reaches `WATER_RANGE` with the LoD on (its water sheet takes over beyond); with it off there is no sheet, so the real water is drawn everywhere the player sees.
        let wr = if lod_on { WATER_RANGE } else { 1e9 };
        let (pcx, pcz) = (chunk_coord(self.player.pos.x), chunk_coord(self.player.pos.z));
        self.chunks.update(self.be.gfx(), self.be.origins(), pcx, pcz);
        self.camera.zfar = if lod_on { lod::ZFAR } else { 256.0 }; // the far terrain needs the depth range
        self.camera.bob = rig_view * if self.opts.view_bobbing { self.body.view_bob(tick_partial) } else { glam::Mat4::IDENTITY };
        // The cloth takes the camera while it spreads and while it is open (it glides back as the cloth rolls up), put back after the frame.
        if let Some(m) = self.mat.filter(|m| m.t > 0.0) {
            let from = pack::Rig { pos: self.camera.pos, yaw: self.camera.yaw, pitch: self.camera.pitch, fov: self.camera.fov_y };
            self.pack_saved = Some((from.pos, from.yaw, from.pitch, from.fov));
            let r = m.glide(from, self.camera.aspect, self.pack_focus);
            (self.camera.pos, self.camera.yaw, self.camera.pitch, self.camera.fov_y, self.camera.bob) = (r.pos, r.yaw, r.pitch, r.fov, glam::Mat4::IDENTITY);
        }
        let (view, proj) = self.camera.build_view_proj();
        // The first-person hand (`EntityRenderer.renderHand`): not in the third-person view, not in bed.
        let hand = ((!self.touch.third || close) && self.sleep.is_none() && self.mat.is_none()).then(|| {
            let (mut equip, held) = self.body.equipped(tick_partial);
            if let Some(p) = self.charge_phase(tick_partial) {
                equip = p; // C3: charging repeats the switch-slot rise
            }
            render::player::Hand { to_world: render::player::hand_to_world(&view, &self.camera.bob), equip, swing: self.body.swing_at(tick_partial), held: held.filter(|s| s.id != world::map::MAP), at: self.player.pos, pack: self.carry.map_or(0.0, |t| t.max(0.01)) }
        });
        let outline_indices = self.target.map_or(0, |h| {
            let (x, y, z) = h.pos;
            let b = self.chunks.block_loaded(x, y, z).and_then(|b| outline_bounds(b, self.chunks.meta(x, y, z)));
            b.map_or(0, |b| self.outline.update(self.be.gfx(), h.pos, b))
        });
        let eye = self.camera.pos;
        let mut sign_views: Vec<SignView> = self
            .signs
            .iter()
            .filter_map(|(&pos, text)| {
                let id = self.chunks.block_loaded(pos.0, pos.1, pos.2).filter(|b| matches!(*b, decor::POST | decor::WALL))?;
                Some(SignView { pos, id, meta: self.chunks.meta(pos.0, pos.1, pos.2), text })
            })
            .collect();
        let near = |s: &SignView| (glam::Vec3::new(s.pos.0 as f32 + 0.5, s.pos.1 as f32 + 0.5, s.pos.2 as f32 + 0.5) - eye).length_squared();
        sign_views.sort_by(|a, b| near(a).total_cmp(&near(b)));
        let frustum = Frustum::from_view_proj(proj * view);
        // Which terrain sections the camera sees (frustum, render shape, and occlusion by terrain); every layer draws from it, and so does
        // the entity test below (it needs this frame's sections, so the walk runs before the entities are built).
        self.chunks.update_visible(self.camera.pos, RENDER_SHAPE, (rd * 16) as f32, (RENDER_VERT * 16) as f32, wr, |min, max| frustum.intersects_aabb(min, max));
        // Entities are only built when they are within the render range, in the camera's frustum and in a section the occlusion walk reached
        // (Sodium's `isBoxVisible`: a mob in a cave behind a hill is skipped); `radius` = a bound on their size.
        let reach2 = ((rd * 16 + 16) as f32).powi(2);
        let terrain = &self.chunks;
        let seen = |p: glam::Vec3, r: f32| p.distance_squared(eye) < reach2 && frustum.intersects_aabb(p - r, p + r) && terrain.box_visible(p - r, p + r);
        self.pack_scene = self.pack_scene_now();
        let item_indices = {
            let chunks = &self.chunks;
            let shown = self.ghost.map(|(m, ok)| (m.shown(), ok));
            let cloth = self.mat.as_ref().map(|m| render::items::PackDraw { mat: m, wool: 12, scene: self.pack_scene.as_ref() }).or(shown.as_ref().map(|(m, ok)| render::items::PackDraw { mat: m, wool: if *ok { 5 } else { 14 }, scene: None })); // brown, or the preview's lime / red wool
            self.item_mesh.update(self.be.gfx(), &self.drops, &self.mobs, &self.ticks.falling, &self.fx, &|x, y, z| chunks.light_levels(x, y, z), self.hook.as_ref().map(|h| h.pos), &self.paintings, &sign_views, (&self.boats.list, self.boats.alpha()), (&self.carts.list, self.carts.alpha()), player_view.as_ref(), hand.as_ref(), cloth, eye, &seen)
        };

        // Sky and fog colour from the sun angle, the weather and the climate under the player; the frame is cleared to the fog colour and
        // the sky pass is drawn over it (vanilla order).
        let (px, pz) = (self.camera.pos.x.floor() as i32, self.camera.pos.z.floor() as i32);
        let partial = self.world_ticks.fract() as f32;
        let angle = sky::celestial_angle(self.world_ticks as u64, partial);
        let (rain, thunder) = if self.nether { (0.0, 0.0) } else { (self.weather.rain(partial), self.weather.thunder(partial)) };
        let hell = [0.2, 0.03, 0.03]; // `WorldProviderHell.func_4096_a`
        // The biome's look (`world::atmos`): 9 columns around the player, eased over seconds; in the Nether the next overworld frame jumps.
        let atm = if self.nether {
            self.atmos.reset();
            world::atmos::Frame::OFF
        } else {
            let (chunks, dt) = (&mut self.chunks, Instant::now().duration_since(self.frame_prev).as_secs_f32().min(0.25));
            if !self.touch.paused {
                self.opts.atmos.view = None; // the editor's preview lives only while its screen is up
            }
            self.opts.atmos.here = self.atmos.update(dt, (px, pz), &self.opts.atmos, |x, z| chunks.climate_at(x, z).0);
            self.atmos.frame(angle, rain, thunder)
        };
        let sky_rgb = if self.nether { hell } else { atm.sky };
        let e = self.camera.pos;
        let eye_in_water = matches!(self.chunks.block_loaded(e.x.floor() as i32, e.y.floor() as i32, e.z.floor() as i32), Some(8 | 9));
        let fresh = self.chunks.take_ingest(); // drained in the Nether too (no LoD there)
        // ponytail: with the LoD switched off the fresh chunks are dropped, not kept for it (it re-reads the saved tiles when it is on again).
        if let Some(l) = self.lod.as_mut().filter(|_| lod_on) {
            fresh.into_iter().for_each(|(k, cols)| l.ingest(k, cols));
            // The LoD is cut out only where a real chunk mesh exists (`drawn`), and again whenever the set of meshes changes (`mesh_gen`).
            let chunks = &self.chunks;
            l.update(self.be.gfx(), e, (rd * 16) as f32, (RENDER_VERT * 16) as f32, chunks.cut_gen(), |cx, cz| chunks.drawn((cx, cz)), |cx, sy, cz| chunks.shown((cx, cz), sy), |min, max| frustum.intersects_aabb(min, max));
        }
        let fog = color::legacy_lin(if self.nether { hell } else { atm.fog }); // vanilla values -> the frame's linear space
        // Terrain fog (b1.7.3 `setupFog`): linear to the render distance, off the far terrain's own start; under water GL_EXP, density 0.1,
        // colour (0.02, 0.02, 0.2); a negative far plane is how the shader learns the density.
        let (fog_c, far) = if eye_in_water { (color::legacy_lin([0.02, 0.02, 0.2]), -0.1) } else { (fog, if lod_on { lod::RADIUS } else { (rd * 16) as f32 }) };
        let start = if lod_on && !eye_in_water { lod::FOG_START } else { 0.0 };
        // Rain and snow (`EntityRenderer.renderRainSnow`), drawn in the scene pass after the water; the depth test hides the streaks behind
        // the terrain. ponytail: not behind water (its translucent layer is not wired), no splashes on the ground.
        let precip = (rain > 0.0).then(|| {
            let r = weather::radius();
            let n = (2 * r + 1) as usize;
            let biomes = self.chunks.biomes(px - r, pz - r, n, n);
            let chunks = &self.chunks;
            let top = |x: i32, z: i32| chunks.top_solid(x, z);
            let bright = |x: i32, y: i32, z: i32| crate::world::chunk::brightness(chunks.light(x, y, z));
            let (snow, rain_quads) = weather::build(&weather::Input { eye: e, ticks: self.world_ticks as u32, partial, rain, biomes: &biomes, top_solid: &top, brightness: &bright });
            (snow, rain_quads, sky::sun_dir(angle).y)
        });
        if self.frames == 0 {
            log::info!("render: first frame, {}, {} chunks loaded", self.be.gpu_info(), self.chunks.loaded());
        }
        self.build_hud();
        // The look (`render::look`): the grade, the light's tones, the water, the sway; then the halos and motes it allows, capped.
        let sun_v = sky::sun_dir(angle);
        let look = Look::new(&self.opts.look, &look::Env { sun: sun_v, rain, seconds: self.world_ticks as f32 / 20.0, nether: self.nether, underwater: eye_in_water, bolt: self.fx.light(partial) });
        let glow_indices = {
            let dt = Instant::now().duration_since(self.frame_prev).as_secs_f32().min(0.25);
            let mut quads = Vec::new();
            if look.glow || look.motes > 0 {
                self.scan.update(&self.chunks, (pcx, pcz));
                let chunks = &self.chunks;
                let mut water = |x: i32, y: i32, z: i32| matches!(chunks.block_loaded(x, y, z), Some(8 | 9)) && chunks.block_loaded(x, y + 1, z) == Some(0);
                self.motes.update(dt, e, look.motes, if sun_v.y < 0.0 { 1.0 } else { 0.0 }, &self.scan, &mut water);
                if look.glow {
                    look::halos(&self.scan, e, &mut quads);
                    quads.extend(look::sun_halo(&look, sun_v, e, self.camera.zfar, rain));
                }
                self.motes.quads(&mut quads);
            }
            let (right, up) = look::axes(&view);
            self.glow.update(self.be.gfx(), &look::vertices(&quads, right, up))
        };
        let d = WorldDraw {
            chunks: &self.chunks,
            lod: self.lod.as_ref().filter(|_| lod_on),
            items: &self.item_mesh,
            item_indices,
            outline: &self.outline,
            outline_indices,
            glow: &self.glow,
            glow_indices,
            look,
            hud: &self.hud_verts,
            camera: &self.camera,
            view,
            proj,
            eye: e,
            sky_sub: sky::skylight_subtracted(angle, rain, thunder) as f32 / 15.0,
            fog: fog_c,
            far,
            start,
            wr,
            nether: self.nether,
            angle,
            rain,
            seconds: self.world_ticks as f32 / 20.0,
            zenith: color::legacy_lin(sky::lightning_flash(sky_rgb, self.fx.flash, partial)),
            horizon: fog,
            atmos: atm,
            precip: precip.as_ref().map(|(s, r, y)| (&s[..], &r[..], *y)),
        };
        let acq = match self.be.draw_world(&d) {
            Drawn::Done(acq) => acq,
            Drawn::Resize(w, h) => {
                log::info!("render: frame is {w}x{h}, reconfiguring");
                self.resize(w, h);
                return;
            }
            Drawn::Skipped => return,
        };
        let spent = t0.elapsed();
        let cpu = spent.saturating_sub(acq);
        self.cpu_acc += cpu.as_secs_f32();
        // Scene-pass GPU time of the frames whose stamps arrived (a few frames late): one mean per frame for `Drs`, all of them for the debug screen.
        self.gpu_buf.clear();
        self.be.gpu_ms(&mut self.gpu_buf);
        self.gpu_acc += self.gpu_buf.iter().sum::<f32>();
        self.gpu_n += self.gpu_buf.len() as u32;
        let gpu = (!self.gpu_buf.is_empty()).then(|| self.gpu_buf.iter().sum::<f32>() / self.gpu_buf.len() as f32 / 1000.0);
        if self.opts.dynamic_res {
            if let Some(p) = self.drs.tick(spent.as_secs_f32(), cpu.as_secs_f32(), gpu) {
                crate::gpu::context::set_scale_pct(p);
                self.be.rescale();
                log::info!("dynamic resolution: scene at {p}%");
            }
        } else if crate::gpu::context::scale_pct() != crate::gpu::context::START_PCT {
            // Switched off in Video Settings: pin the scene at `START_PCT` again and start `Drs` from there for the next switch-on.
            crate::gpu::context::set_scale_pct(crate::gpu::context::START_PCT);
            self.be.rescale();
            self.drs = Drs::at(crate::gpu::context::START_PCT, self.refresh_hz);
        }

        self.frames += 1;
        let now = Instant::now();
        // A paused frame, and the first one after it, waited on purpose (the idle poll): not a spike.
        if !(self.touch.paused || self.was_paused) {
            let gap = now.duration_since(self.frame_prev).as_secs_f32();
            self.worst_acc = self.worst_acc.max(gap);
            self.times.push(gap);
        }
        self.was_paused = self.touch.paused;
        self.frame_prev = now;
        if now.duration_since(self.fps_last).as_secs_f32() >= 1.0 {
            self.cpu_ms = (self.cpu_acc / self.frames as f32 * 1000.0).round() as u32;
            self.worst_ms = (self.worst_acc * 1000.0).round() as u32;
            self.worst_acc = 0.0;
            self.p99_ms = (self.times.take_p99() * 1000.0).round() as u32;
            self.gpu_tenths = if self.gpu_n > 0 { (self.gpu_acc / self.gpu_n as f32 * 10.0).round() as u32 } else { 0 };
            (self.gpu_acc, self.gpu_n) = (0.0, 0);
            let fps = self.frames as f32 / now.duration_since(self.fps_last).as_secs_f32();
            self.fps_recent = fps.round() as u32;
            // Moving average, 0.1 a second (~10 s); not fed while paused (the menu idles on purpose) and seeded by its first second.
            if !self.touch.paused {
                self.fps_avg = if self.fps_avg == 0.0 { fps } else { self.fps_avg * 0.9 + fps * 0.1 };
            }
            log::info!("FPS: {fps:.1} (average of the last ~10 s {:.0}), {} chunks loaded, CPU {:.1} ms/frame (CPU ~ frame time: CPU-bound; far below: GPU-bound), {} HUD vertices, {} sign texts kept", self.fps_avg, self.chunks.loaded(), self.cpu_acc / self.frames as f32 * 1000.0, self.hud_verts.len(), self.item_mesh.cached_signs());
            // The pacing line: the slowest 1% of frames (not the average), the worst one, the panel, the scene pass's GPU time (`gpu::timing`; n/a
            // without timestamp queries), and how long new chunks took to become drawable.
            let wait = self.chunks.take_mesh_wait().map_or(String::new(), |(avg, max, n)| format!(", first mesh of {n} chunks: avg {avg:.0} ms, max {max:.0} ms"));
            let gpu = if self.gpu_tenths > 0 { format!("{:.1} ms", self.gpu_tenths as f32 / 10.0) } else { "n/a".to_string() };
            log::info!("pacing: p99 {} ms, worst {} ms, panel {:.0} Hz, scene pass GPU {gpu}{wait}", self.p99_ms, self.worst_ms, self.refresh_hz);
            self.cpu_acc = 0.0;
            self.frames = 0;
            self.fps_last = now;
        }
    }

    /// A frame of `menu::Menu` (the title screen, world list, creation, "Loading level"): the HUD pass over black, no world. The frame
    /// that shows "Loading level" is presented before the world is loaded, which freezes the render thread for a moment.
    fn render_menu(&mut self) {
        let (w, h) = { let (w, h) = self.be.size(); (w as f32, h as f32) };
        if self.menu.is_none() {
            return;
        }
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        self.pad_sync(dt);
        let Some(m) = self.menu.as_mut() else { return };
        for ev in self.touch.take_screen_events() {
            m.event(ev, w, h, &mut self.opts);
        }
        if m.quit {
            std::process::exit(0); // Quit Game: nothing is loaded, so nothing to save
        }
        self.hud_verts.clear();
        m.draw(&mut self.hud_verts, w, h, &self.opts);
        let cur = self.touch.pad.shows_cursor().then_some(self.touch.cursor_pos);
        self.touch.pad.draw(&mut self.hud_verts, w, h, self.touch.hotbar_slot, cur);
        let pano = m.panorama();
        self.hud_verts.truncate(crate::gles::HUD_QUADS * 6);
        match self.be.draw_menu(pano, &self.hud_verts) {
            Drawn::Resize(w, h) => {
                self.resize(w, h);
                return;
            }
            Drawn::Skipped => return,
            Drawn::Done(_) => {}
        }
        let load = self.menu.as_mut().filter(|m| m.shown_loading).and_then(|m| m.loading.take());
        if let Some(info) = load {
            self.enter_world(&info);
        }
    }

    /// What the game menu shows of the game.
    /// The pad's frame: moves / looks / clicks through `TouchUi`, and hands a button the Controls screen waits for to it.
    fn pad_sync(&mut self, dt: f32) {
        self.touch.pad.capture = self.pause.as_ref().is_some_and(Pause::pad_waiting) || self.menu.as_ref().is_some_and(Menu::pad_waiting);
        // The d-pad jumps between targets in the satchel, a chest / furnace / bench screen and the pause menu's top page; the stick is the free cursor.
        self.touch.pad.snap = self.screen.is_some() || (self.touch.paused && self.pause.as_ref().is_some_and(Pause::top));
        self.touch.pad_update(dt, &self.opts.pad);
        if let Some(d) = self.touch.take_nav() {
            self.sat_nav(d);
        }
        if let Some(shift) = self.touch.take_alt() {
            self.pad_alt(shift);
        }
        // LB / RB step the options tabs (the satchel's book takes them in `sat_pad_keys` otherwise).
        if self.pause.as_ref().is_some_and(Pause::in_options) || self.menu.as_ref().is_some_and(Menu::in_options) {
            let d = self.touch.take_page();
            if d != 0 {
                if let Some(p) = self.pause.as_mut().filter(|p| p.in_options()) {
                    p.step_tab(d, &mut self.opts);
                } else if let Some(m) = self.menu.as_mut() {
                    m.step_tab(d, &mut self.opts);
                }
            }
        }
        self.sat_pad_keys();
        if self.touch.take_pad_info() {
            self.opts.debug_screen = !self.opts.debug_screen;
            self.opts.save();
        }
        if let Some(b) = self.touch.pad.captured.take() {
            if let Some(p) = self.pause.as_mut() {
                p.pad_button(b, &mut self.opts);
            }
            if let Some(m) = self.menu.as_mut() {
                m.pad_button(b, &mut self.opts);
            }
        }
    }

    fn pause_info(&self) -> PauseInfo {
        PauseInfo { dead: self.vitals.dead(), pregen: self.lod.as_ref().map(|l| l.pregen()), debug: self.lod.as_ref().map_or(0, |l| l.debug()), flying: self.touch.flying, info: self.opts.debug_screen }
    }

    /// The display's refresh rate (`immersive::refresh_hz`): the dynamic resolution aims at it, 60 to 90 Hz.
    fn set_refresh_hz(&mut self, hz: f32) {
        if (hz - self.refresh_hz).abs() > 0.5 {
            log::info!("display refresh rate {hz:.1} Hz");
        }
        self.refresh_hz = hz;
        self.drs.set_refresh_hz(hz);
    }

    fn on_motion(&mut self, ev: &android_activity::input::MotionEvent) {
        self.touch.handle_motion(ev);
    }

    /// The rows of the debug screen (`render::debug`): left = the player and the world, right = the engine. Only built while it is on
    /// and the game is not paused. ponytail: `meshed()` walks the chunk map once a frame (a few hundred entries) and the biome comes
    /// from the cached one-column lookup the weather uses; neither is worth a counter of its own.
    fn debug_rows(&mut self) -> (Vec<Row>, Vec<Row>) {
        use crate::options::Opt;
        use crate::render::debug::{bearing, clock, compass, fps_color, words, CYAN, GREEN, GREY, WHITE, YELLOW};
        use crate::render::present::{fast_post, fxaa, scene_samples};
        let p = self.player.pos;
        let feet = p.y - physics::HALF.y;
        let (bx, by, bz) = (p.x.floor() as i32, feet.floor() as i32, p.z.floor() as i32);
        let (sky, blk) = self.chunks.light_levels(bx, (p.y + EYE_HEIGHT).floor() as i32, bz);
        let b = bearing(self.camera.yaw);
        let weather = if self.weather.thunder(1.0) > 0.5 { "  thunder" } else if self.weather.rain(1.0) > 0.5 { "  rain" } else { "" };
        let mut left = vec![
            Row::Head("Minecraft b1.7.3 (mc-rs)".to_string()),
            Row::Gap,
            Row::Kv("XYZ", format!("{:.2} / {:.2} / {:.2}", p.x, feet, p.z), WHITE),
            Row::Kv("Block", format!("{bx} {by} {bz}"), WHITE),
            Row::Kv("Chunk", format!("{} {}  (in {} {})", bx >> 4, bz >> 4, bx & 15, bz & 15), WHITE),
            // Pitch is shown up-positive (the camera's is down-positive).
            Row::Kv("Facing", format!("{} ({b})  pitch {:+}", compass(b), (-self.camera.pitch.to_degrees()).round() as i32), WHITE),
        ];
        if !self.nether {
            let biome = self.chunks.biomes(bx, bz, 1, 1).first().map_or("?".to_string(), |b| words(&format!("{b:?}")));
            left.push(Row::Kv("Biome", biome, GREEN));
        }
        left.push(Row::Kv("Light", format!("sky {sky}  block {blk}"), WHITE));
        left.push(Row::Kv("Time", format!("{}{weather}", clock(self.world_ticks as u64)), WHITE));
        let st = &self.vitals.stamina; // `StaminaFeatureClient`'s debug entry
        left.push(Row::Kv("Stamina", format!("{:.1} / {:.1}  locked {}", st.cur, st.max, st.locked), if st.locked { YELLOW } else { WHITE }));
        left.push(match self.target {
            Some(h) => {
                let (x, y, z) = h.pos;
                let id = self.chunks.block(x, y, z).unwrap_or(0);
                let name = crate::world::names::NAMES.binary_search_by_key(&(id as u16), |e| e.0).map_or_else(|_| format!("id {id}"), |i| crate::world::names::NAMES[i].1.to_string());
                Row::Kv("Looking at", format!("{name} ({x} {y} {z})"), CYAN)
            }
            None => Row::Kv("Looking at", "nothing in reach".to_string(), GREY),
        });

        let (w, h) = self.be.size();
        let fps = self.fps_recent;
        // A setting as its button shows it (`Settings::text`, "Name: value"), so the two never disagree; "(restart)" comes along.
        let set = |o: Opt| self.opts.text(o).split_once(": ").map_or(String::new(), |t| t.1.to_lowercase());
        let (sc, pct, ms) = (crate::gpu::context::scene_size(w, h), crate::gpu::context::scale_pct(), scene_samples());
        let mut right = vec![
            Row::Kv("FPS", format!("{fps}  (avg {:.0})", self.fps_avg), fps_color(fps)),
            Row::Kv("Frame", if self.gpu_tenths > 0 { format!("cpu {} ms  gpu {:.1} ms  p99 {} ms  worst {} ms", self.cpu_ms, self.gpu_tenths as f32 / 10.0, self.p99_ms, self.worst_ms) } else { format!("cpu {} ms  p99 {} ms  worst {} ms", self.cpu_ms, self.p99_ms, self.worst_ms) }, WHITE),
            Row::Kv("Screen", format!("{w}x{h}"), WHITE),
            Row::Kv("Scene", format!("{}x{}  {pct}%", sc.0, sc.1), WHITE),
            Row::Kv("Dyn. res", set(Opt::DynamicRes), WHITE),
            Row::Kv("MSAA", format!("{}  [{}]", if ms > 1 { format!("{ms}x") } else { "off".into() }, set(Opt::Msaa)), WHITE),
            Row::Kv("Post", format!("{}  [{}]", if fast_post() { "fast" } else { "full" }, set(Opt::Post)), WHITE),
            Row::Kv("FXAA", format!("{}  [{}]", if fxaa() && !fast_post() { "on" } else { "off" }, set(Opt::Fxaa)), WHITE),
            Row::Kv("Renderer", self.be.gpu_info().chars().take(30).collect::<String>(), WHITE),
            Row::Kv("Graphics", format!("{}, smooth lighting {}", set(Opt::Graphics), set(Opt::AmbientOcclusion)), WHITE),
            Row::Kv("Perf", set(Opt::FramerateLimit), WHITE),
            Row::Kv("Distance", format!("{} chunks", self.opts.chunks()), WHITE),
            Row::Kv("Chunks", format!("{} loaded  {} meshed", self.chunks.loaded(), self.chunks.meshed()), WHITE),
            Row::Kv("LoD", if self.lod_on { "on" } else { "off" }.to_string(), YELLOW),
        ];
        // The LoD debug mode that is on (the standalone title at the top centre is not drawn while this screen is).
        if let Some((mode, title)) = self.lod.as_ref().map(|l| (l.debug(), l.debug_title())).filter(|t| t.0 != 0) {
            right.push(Row::Kv("LoD debug", format!("{mode}: {}", title.split_once(": ").map_or(title, |t| t.1)), YELLOW));
        }
        right.push(Row::Kv("World", if self.nether { "Nether" } else { "Overworld" }.to_string(), WHITE));
        (left, right)
    }

    /// Build the HUD vertex buffer for this frame. Called between the chunk
    /// and HUD render passes inside `render`.
    fn build_hud(&mut self) {
        // `ext` reads all of `self`, so it is taken before `hud_verts` is borrowed mutably.
        let ext = self.screen.as_ref().and_then(|s| self.ext(s));
        let pause_info = self.pause_info();
        // The pack screen's text is projected from the cloth through this frame's camera (the cloth's), and the finger's unit names the stack.
        let pack_hud = self.mat.filter(|_| self.mat_ui).map(|m| {
            let (view, proj) = self.camera.build_view_proj();
            (m, proj * view, self.cloth_at(self.touch.cursor_pos, pack::REST), self.slot_under(self.touch.cursor_pos.0, self.touch.cursor_pos.1))
        });
        // The debug screen replaces the FPS counter and the LoD debug title; its panels are not drawn under the pause menu.
        let info_on = self.opts.debug_screen;
        let dbg = if info_on && !self.touch.paused { Some(self.debug_rows()) } else { None };
        let worn = self.worn();
        let v = &mut self.hud_verts;
        v.clear();
        let layout = &self.touch.layout;
        let paused = self.touch.paused;
        let pressed = |role: PointerRole| self.touch.pointers.values().any(|p| p.role == role);
        // The in-world controls are hidden while a container screen is open.
        let play = self.screen.is_none() && self.sign_edit.is_none();

        // Crosshair: the aim point of the pick ray (first, so the quad cap never drops it).
        if play {
            let (w, h) = { let (w, h) = self.be.size(); (w as f32, h as f32) };
            // `icons.png`'s cross (the Java draws it inverting the pixels behind it; here plain white), the size the two bars had.
            let s = 0.05 * w.min(h);
            HudPipeline::push_gui(v, w * 0.5 - s * 0.5, h * 0.5 - s * 0.5, s, s, sp::CROSSHAIR);
        }

        // The map in hand (`ItemRenderer`'s map branch, here a flat sheet: UNVERIFIED place and size), right of the crosshair.
        if let (true, Some(m)) = (play, &self.map_held) {
            let (w, h) = { let (w, h) = self.be.size(); (w as f32, h as f32) };
            let size = (0.62 * h).min(0.42 * w);
            HudPipeline::push_map(v, (((w - size) * 0.5 + 0.2 * w).min(w - size - 0.02 * w), (h - size) * 0.35, size), m);
        }

        // Dig progress under the crosshair.
        if play && self.dig.damage > 0.0 {
            let (w, h) = { let (w, h) = self.be.size(); (w as f32, h as f32) };
            let (bw, bh) = (0.12 * w.min(h), (0.012 * w.min(h)).max(3.0));
            let (bx, by) = (w * 0.5 - bw * 0.5, h * 0.5 + 0.05 * w.min(h));
            HudPipeline::push_quad(v, bx, by, bw, bh, [0.0, 0.0, 0.0, 0.6]);
            HudPipeline::push_quad(v, bx, by, bw * self.dig.damage.min(1.0), bh, [1.0, 1.0, 1.0, 0.9]);
        }

        // Cortes Combat C3 (NOT b1.7.3, UNVERIFIED): the charge bar under the crosshair (white, gold when full; a tick at the minimum hold, below which a
        // release is a plain tap) and the cooldown mark, the attack indicator: a thin bar that fills until the next swing.
        if play && world::combat::rules::on() {
            let (w, h) = { let (w, h) = self.be.size(); (w as f32, h as f32) };
            let (bw, bh) = (0.12 * w.min(h), (0.012 * w.min(h)).max(3.0));
            let (bx, by) = (w * 0.5 - bw * 0.5, h * 0.5 + 0.05 * w.min(h) + bh * 2.0);
            let swing = world::combat::rules::swing(self.inv.slots[self.touch.hotbar_slot].map(|s| s.id));
            let full = swing.map_or(0, |s| s.profile.charge().ticks);
            // C4: the meal's progress (brown) takes the bar's place while it is eaten.
            if let Some(e) = self.eating {
                HudPipeline::push_quad(v, bx, by, bw, bh, [0.0, 0.0, 0.0, 0.6]);
                HudPipeline::push_quad(v, bx, by, bw * (1.0 - e.left as f32 / EAT_TICKS as f32), bh, [0.85, 0.55, 0.25, 0.95]);
            }
            // C4: the guard button over the jump button (a shield; lit while the guard is up) with one orb a point, grey while it is staggered.
            if self.touch.guard_shown && !self.touch.pad.hud_hidden() {
                let (gx, gy, gw, gh) = self.touch.layout.guard;
                let g = self.vitals.guard;
                HudPipeline::push_button(v, gx, gy, gw, gh, if self.touch.guarding() || g.active() { 2 } else { 1 });
                let shield = if g.stun > 0 { [0.6, 0.6, 0.6, 0.8] } else { [1.0; 4] };
                HudPipeline::push_quad(v, gx + gw * 0.27, gy + gh * 0.2, gw * 0.46, gh * 0.26, shield);
                HudPipeline::push_quad(v, gx + gw * 0.33, gy + gh * 0.46, gw * 0.34, gh * 0.18, shield);
                HudPipeline::push_quad(v, gx + gw * 0.42, gy + gh * 0.64, gw * 0.16, gh * 0.14, shield);
                for i in 0..g.max {
                    let ox = gx + gw * 0.5 + (i as f32 - (g.max as f32 - 1.0) * 0.5) * gw * 0.22 - gw * 0.07;
                    HudPipeline::push_quad(v, ox, gy - gw * 0.2, gw * 0.14, gw * 0.14, if i < g.points { [0.4, 0.8, 1.0, 0.95] } else { [0.0, 0.0, 0.0, 0.5] });
                }
            }
            // C5: the special button, where the guard's would be (no weapon has both): lit while the blade is out or the lunge runs, an icon
            // by special (an arrow, a fan, a cross) and a dark veil that sinks as its price (cooldown) is paid.
            if self.touch.special_shown && !self.touch.pad.hud_hidden() {
                use world::combat::{profile::Special, special};
                let (sx, sy, sw, sh) = self.touch.layout.special;
                let kind = self.held_special();
                HudPipeline::push_button(v, sx, sy, sw, sh, if self.sp.parrying() || self.sp.lunge.is_some() { 2 } else { 1 });
                let c = [1.0; 4];
                match kind {
                    Special::Lunge => {
                        HudPipeline::push_quad(v, sx + sw * 0.2, sy + sh * 0.46, sw * 0.5, sh * 0.08, c);
                        HudPipeline::push_quad(v, sx + sw * 0.62, sy + sh * 0.36, sw * 0.16, sh * 0.28, c);
                    }
                    Special::MultiHit => {
                        for k in 0..3 {
                            HudPipeline::push_quad(v, sx + sw * (0.2 + 0.06 * k as f32), sy + sh * (0.28 + 0.16 * k as f32), sw * (0.6 - 0.12 * k as f32), sh * 0.08, c);
                        }
                    }
                    _ => {
                        HudPipeline::push_quad(v, sx + sw * 0.46, sy + sh * 0.22, sw * 0.08, sh * 0.56, c);
                        HudPipeline::push_quad(v, sx + sw * 0.3, sy + sh * 0.36, sw * 0.4, sh * 0.08, c);
                    }
                }
                let left = match (kind, swing) {
                    (Special::Parry, _) => self.sp.parry_cooldown().0 as f32 / self.sp.parry_cooldown().1 as f32,
                    (Special::Lunge, Some(s)) => self.swing_cd as f32 / special::lunge_cost(s.interval) as f32,
                    (Special::MultiHit, Some(s)) => self.swing_cd as f32 / special::sweep_cost(s.interval) as f32,
                    _ => 0.0,
                }
                .min(1.0);
                if left > 0.0 {
                    HudPipeline::push_quad(v, sx, sy + sh * (1.0 - left), sw, sh * left, [0.0, 0.0, 0.0, 0.55]);
                }
            }
            // C5: the martillo's streak, one gold orb a hit (up to the nine of a full stack) where the guard orbs would be.
            if self.sp.streak() > 0 && swing.is_some_and(|s| s.profile == world::combat::profile::Profile::Warhammer) && !self.touch.pad.hud_hidden() {
                let (gx, gy, gw, _) = self.touch.layout.guard;
                for i in 0..self.sp.streak().min(9) {
                    HudPipeline::push_quad(v, gx + gw * 0.05 + i as f32 * gw * 0.1, gy - gw * 0.2, gw * 0.08, gw * 0.08, [1.0, 0.8, 0.2, 0.95]);
                }
            }
            if self.charge > 0 && full > 0 {
                let f = (self.charge as f32 / full as f32).min(1.0);
                HudPipeline::push_quad(v, bx, by, bw, bh, [0.0, 0.0, 0.0, 0.6]);
                HudPipeline::push_quad(v, bx, by, bw * f, bh, if self.charge >= full { [1.0, 0.8, 0.2, 0.95] } else { [1.0, 1.0, 1.0, 0.9] });
                let m = world::combat::profile::MIN_CHARGE_HOLD as f32 / full as f32;
                HudPipeline::push_quad(v, bx + bw * m, by, (bw * 0.01).max(1.0), bh, [1.0, 0.3, 0.3, 0.9]);
            } else if let Some(s) = swing.filter(|s| self.swing_cd > 0 && self.swing_held == self.inv.slots[self.touch.hotbar_slot].map(|i| i.id) && s.interval > 0) {
                let f = 1.0 - (self.swing_cd as f32 / s.interval as f32).min(1.0);
                HudPipeline::push_quad(v, bx, by, bw, bh * 0.5, [0.0, 0.0, 0.0, 0.5]);
                HudPipeline::push_quad(v, bx, by, bw * f, bh * 0.5, [0.8, 0.8, 0.8, 0.8]);
            }
        }

        // `GuiEditSign`: the title, the board with its four lines (the one being edited is wrapped in `> <` and blinks), and the keys.
        if let Some(e) = self.sign_edit {
            let (w, h) = { let (w, h) = self.be.size(); (w as f32, h as f32) };
            let (u, keys) = decor::layout(w, h, e.shift);
            HudPipeline::push_quad(v, 0.0, 0.0, w, h, [0.0, 0.0, 0.0, 0.55]);
            let title = "Edit sign message:";
            HudPipeline::push_text(v, (w - HudPipeline::text_width(title) as f32 * u) / 2.0, 14.0 * u, u, title, [1.0; 4]);
            HudPipeline::push_outlined_quad(v, (w - 130.0 * u) / 2.0, 30.0 * u, 130.0 * u, 64.0 * u, [0.62, 0.49, 0.28, 1.0], [0.25, 0.18, 0.08, 1.0], (2.0 * u).max(2.0));
            let text = self.signs.get(&e.pos).cloned().unwrap_or_default();
            let blink = (self.world_ticks as u64 / 6) % 2 == 0;
            for (k, r) in &keys {
                let label = match *k {
                    Key::Line(i) if i == e.line && blink => format!("> {} <", text[i as usize]),
                    Key::Line(i) => text[i as usize].clone(),
                    k => decor::label(k),
                };
                let (fill, ink) = match *k {
                    Key::Line(_) => ([0.0; 4], [0.0, 0.0, 0.0, 1.0]),
                    Key::Shift if e.shift => ([0.2, 0.5, 0.8, 0.97], [1.0; 4]),
                    Key::Done => ([0.2, 0.55, 0.25, 0.97], [1.0; 4]),
                    _ => ([0.33, 0.33, 0.33, 0.97], [1.0; 4]),
                };
                if fill[3] > 0.0 {
                    HudPipeline::push_outlined_quad(v, r[0], r[1], r[2], r[3], fill, [0.1, 0.1, 0.1, 1.0], (u).max(1.0));
                }
                let tw = HudPipeline::text_width(&label) as f32 * u;
                HudPipeline::push_text(v, r[0] + (r[2] - tw) / 2.0, r[1] + (r[3] - 8.0 * u) / 2.0, u, &label, ink);
            }
        }

        // The immersive pack: the inventory as a cloth with every item lying in the zone of its category (`world::pack`).
        if let (Some(s), Some((m, vp, finger, picked)), Some(sc)) = (self.screen.as_ref().filter(|_| self.mat_ui), pack_hud, self.pack_scene.as_ref()) {
            let (w, h) = { let (w, h) = self.be.size(); (w as f32, h as f32) };
            draw_pack(v, (w, h), (&m, vp), &self.inv, s, sc, (self.one_mode, self.shift_mode), (self.touch.cursor_pos, finger, picked), self.gesture.is_some(), self.pack_focus);
        }

        // The satchel: the flat bag and the recipe book (`satchel.rs`).
        if let (Some(s), Some(sat)) = (self.screen.as_ref(), self.sat.as_ref()) {
            let (w, h) = { let (w, h) = self.be.size(); (w as f32, h as f32) };
            let hold = self.gesture.as_ref().is_some_and(|g| g.start == Some(craft::SlotId::Result) && g.age >= satchel::HOLD_SECS);
            let ctx = satchel::Ctx { inv: &self.inv, screen: s, notes: &self.notes, one: self.one_mode, shift: self.shift_mode, discovery: self.opts.book_discovery, hotbar: self.touch.hotbar_slot, finger: self.touch.cursor_pos, down: self.gesture.is_some(), hold, pad: self.touch.pad.shows_cursor() };
            satchel::draw(v, (w, h), sat, &ctx);
        }

        // Container screen: the vanilla GUI (176 x 166 units) over a dimmed frame, slots with their stacks.
        if let Some(s) = self.screen.as_ref().filter(|_| !self.mat_ui && self.sat.is_none()) {
            let (w, h) = { let (w, h) = self.be.size(); (w as f32, h as f32) };
            let (ox, oy, k) = craft::panel(s.gw, w, h);
            let (pw, ph) = craft::panel_size(s.gw);
            HudPipeline::push_quad(v, 0.0, 0.0, w, h, [0.0, 0.0, 0.0, 0.55]);
            // The panel is the pack's picture (`GuiInventory` / `GuiCrafting` / `GuiFurnace` / `GuiChest`: a chest is its rows' top, then the
            // 96 units of the player's inventory from y = 126 of the sheet); the slots are in it, so a slot is only its stack.
            if s.gw > 3 {
                let top = 17.0 + 18.0 * (s.gw / 9) as f32;
                HudPipeline::push_gui(v, ox, oy, pw * k, top * k, (0.0, sp::CHEST_Y, 176.0, top));
                HudPipeline::push_gui(v, ox, oy + top * k, pw * k, 96.0 * k, (0.0, sp::CHEST_Y + 126.0, 176.0, 96.0));
            } else {
                HudPipeline::push_gui(v, ox, oy, pw * k, ph * k, match s.gw { 2 => sp::INVENTORY, 3 => sp::CRAFTING, _ => sp::FURNACE });
            }
            // `GuiInventory`: the player at (51, 75) of the panel, straight ahead, an item in hand lowers the arm.
            if s.gw == 2 {
                let pose = render::player::Pose { age: self.world_ticks as f32, holding: self.inv.slots[self.touch.hotbar_slot].is_some(), ..Default::default() };
                HudPipeline::push_player(v, render::player::inventory_quads(ox + 51.0 * k, oy + 75.0 * k, k, &pose, &worn));
            }
            // Icons here are drawn as big in px as the hotbar's (`16 * hw / 20` box, 0.875 of it: 14 hotbar units), not as 16 panel units.
            let icon_fill = 14.0 * layout.hotbar[0].2 / 20.0 / (16.0 * k);
            let slot = |v: &mut Vec<HudVertex>, c: (f32, f32, f32), st: Option<ItemStack>| {
                if let Some(st) = st {
                    push_stack(v, (c.0 + k, c.1 + k, c.2 - 2.0 * k, c.2 - 2.0 * k), icon_fill, st);
                }
            };
            let furn = s.furnace.and_then(|p| self.furnaces.get(&p));
            for (id, ux, uy) in craft::layout(s.gw) {
                slot(v, craft::cell(s.gw, w, h, (ux, uy)), s.get(&self.inv, ext.as_deref(), id));
            }
            // The labels of `drawGuiContainerForegroundLayer`, colour 0x404040: the chest's name at (8, 6), "Inventory" at (8, ySize - 96 + 2);
            // "Crafting" at (28, 6) on the workbench, (86, 16) in the inventory; "Furnace" at (60, 6).
            // UNVERIFIED: the non-chest positions are recalled, not read from the Java.
            let ink = [0.25, 0.25, 0.25, 1.0];
            let labels: &[(&str, f32, f32)] = match s.gw {
                2 => &[("Crafting", 97.0 - craft::CRAFT_SHIFT as f32, 8.0)], // the pack's panel is the modern one: its grid is at (98, 18), moved left by CRAFT_SHIFT
                3 => &[("Crafting", 28.0, 6.0), ("Inventory", 8.0, ph - 94.0)],
                0 => &[("Furnace", 60.0, 6.0), ("Inventory", 8.0, ph - 94.0)],
                9 => &[("Dispenser", 60.0, 6.0), ("Inventory", 8.0, ph - 94.0)],
                _ => &[(if s.chests.len() > 1 { "Large chest" } else if s.chests.first().is_some_and(|p| p.0 == i32::MIN) { "Minecart" } else { "Chest" }, 8.0, 6.0), ("Inventory", 8.0, ph - 94.0)],
            };
            for &(text, ux, uy) in labels {
                // 0.75 x: the 8 px glyphs are wider than vanilla's proportional font and ran into the grid and the buttons.
                if text == "Crafting" {
                    HudPipeline::push_text_outlined(v, ox + ux * k, oy + uy * k, k * 0.75, (k * 0.3).max(1.0), text, [1.0; 4]); // white, thin black outline
                } else {
                    HudPipeline::push_text(v, ox + ux * k, oy + uy * k, k * 0.75, text, ink);
                }
            }
            if let Some(f) = furn {
                // Cook arrow (fills left to right over 200 ticks) and flame (drains from the top), vanilla positions.
                let u = |ux: f32, uy: f32, uw: f32, uh: f32| (ox + ux * k, oy + uy * k, uw * k, uh * k);
                // `GuiFurnace`: the arrow is `cook * 24 / 200 + 1` wide, the flame `burn * 12 / item_burn` + 2 tall from its bottom.
                let fy = sp::FURNACE.1;
                let cook = (f.cook as f32 * 24.0 / 200.0).floor() + 1.0;
                let (ax, ay, ..) = u(79.0, 34.0, 0.0, 0.0);
                HudPipeline::push_gui(v, ax, ay, cook * k, 16.0 * k, (176.0, fy + 14.0, cook, 16.0));
                if f.burn > 0 {
                    let b = (f.burn as f32 * 12.0 / f.item_burn.max(1) as f32).floor();
                    let (fx, fyy, ..) = u(56.0, 36.0 + 12.0 - b, 0.0, 0.0);
                    HudPipeline::push_gui(v, fx, fyy, 14.0 * k, (b + 2.0) * k, (176.0, fy + 12.0 - b, 14.0, b + 2.0));
                }
            }
            // The "place one" toggle: blue = whole stacks, orange with a 1 = one at a time (what the right mouse
            // button does on a desktop). Dragging the stack over slots spreads it without the toggle.
            let (mx, my, ms) = craft::toggle(s.gw, w, h, 1);
            let fill = if self.one_mode { [0.85, 0.5, 0.1, 1.0] } else { [0.2, 0.4, 0.7, 1.0] };
            HudPipeline::push_outlined_quad(v, mx, my, ms, ms, fill, [0.15, 0.15, 0.15, 1.0], k.max(2.0));
            if self.one_mode {
                HudPipeline::push_number(v, mx + ms * 0.32, my + ms * 0.2, ms * 0.6, 1, [1.0; 4]);
            }
            // The shift-click toggle: left of the other one, green when on (both sit bottom-left of the screen, `craft::toggle`).
            let (sx, sy, ss) = craft::toggle(s.gw, w, h, 0);
            let fill = if self.shift_mode { [0.2, 0.7, 0.3, 1.0] } else { [0.2, 0.4, 0.7, 1.0] };
            HudPipeline::push_outlined_quad(v, sx, sy, ss, ss, fill, [0.15, 0.15, 0.15, 1.0], k.max(2.0));
            if self.shift_mode {
                HudPipeline::push_number(v, sx + ss * 0.32, sy + ss * 0.2, ss * 0.6, 2, [1.0; 4]);
            }
            // The picked-up stack floats just above the finger.
            if let Some(c) = s.cursor {
                let (cx, cy) = self.touch.cursor_pos;
                let sz = 16.0 * k;
                push_stack(v, (cx - sz * 0.5, cy - sz * 1.4, sz, sz), icon_fill, c);
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

        // The four buttons top-right (pause, inventory, view, zoom) are not drawn while the pack's table is open: its edges carry the buttons.
        if !self.mat_ui && self.sat.is_none() && !self.touch.pad.hud_hidden() {
        // Pause button: top-right, two bars.
        let (px, py, pw, ph) = layout.pause;
        HudPipeline::push_button(v, px, py, pw, ph, if pressed(PointerRole::Pause) { 2 } else { 1 });
        let bar_w = pw * 0.18;
        let bar_h = ph * 0.50;
        let bar_y = py + (ph - bar_h) * 0.5;
        let bx0 = px + (pw - bar_w * 3.0) * 0.5;
        HudPipeline::push_quad(v, bx0, bar_y, bar_w, bar_h, [1.0; 4]);
        HudPipeline::push_quad(v, bx0 + bar_w * 2.0, bar_y, bar_w, bar_h, [1.0; 4]);

        // Inventory button, left of pause: a 2x2 grid icon (the crafting grid of the screen it opens).
        let (ix, iy, iw, ih) = layout.inventory;
        HudPipeline::push_button(v, ix, iy, iw, ih, if pressed(PointerRole::Inventory) || !play { 2 } else { 1 });
        let (q, g) = (iw * 0.22, iw * 0.08);
        let (qx, qy) = (ix + (iw - q * 2.0 - g) * 0.5, iy + (ih - q * 2.0 - g) * 0.5);
        for (dx, dy) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
            HudPipeline::push_quad(v, qx + dx * (q + g), qy + dy * (q + g), q, q, [1.0; 4]);
        }

        // View button, left of zoom: a person (head over body), lit in the third-person view.
        let (vx, vy, vw, vh) = layout.view;
        HudPipeline::push_button(v, vx, vy, vw, vh, if self.touch.third || pressed(PointerRole::View) || !play { 2 } else { 1 });
        HudPipeline::push_quad(v, vx + vw * 0.38, vy + vh * 0.2, vw * 0.24, vh * 0.24, [1.0; 4]);
        HudPipeline::push_quad(v, vx + vw * 0.32, vy + vh * 0.48, vw * 0.36, vh * 0.32, [1.0; 4]);

        // Zoom button, left of the inventory button: a Z (Zoomify), lit while held.
        let (zx, zy, zw, zh) = layout.zoom;
        HudPipeline::push_button(v, zx, zy, zw, zh, if pressed(PointerRole::Zoom) || !play { 2 } else { 1 });
        HudPipeline::push_text_centered(v, zx + zw * 0.5, zy + (zh - 8.0 * zh / 20.0) * 0.5, zh / 20.0, "Z", [1.0; 4]);
        }

        // Hotbar (`GuiIngame`): the 182 x 22 strip (20 units a slot, a unit = a ninth-cell / 20), then the 24 x 24 selector over the slot
        // in use, then the stacks. The touch cells are the slots, so the strip is drawn around them.
        if play {
            let (hx, hy, hw, _) = layout.hotbar[0];
            let u = hw / 20.0;
            HudPipeline::push_gui(v, hx - u, hy - u, 182.0 * u, 22.0 * u, sp::HOTBAR);
            let (sx, _, _, _) = layout.hotbar[self.touch.hotbar_slot];
            HudPipeline::push_gui(v, sx - 2.0 * u, hy - 2.0 * u, 24.0 * u, 24.0 * u, sp::SELECTOR);
        }
        for (i, &(hx, hy, hw, _)) in layout.hotbar.iter().enumerate().filter(|_| play) {
            if let Some(s) = self.inv.slots[i] {
                let u = hw / 20.0; // the slot is 20 units, its item 16 at (2, 2)
                push_stack(v, (hx + 2.0 * u, hy + 2.0 * u, 16.0 * u, 16.0 * u), 0.875, s); // icon 14 of the 16 units
            }
        }

        // The recipe pinned in the book: what it wants, and what the bag has of each.
        if let (true, Some(e)) = (play, self.notes.pinned.and_then(|i| book::entries().get(i))) {
            let (w, h) = { let (w, h) = self.be.size(); (w as f32, h as f32) };
            satchel::draw_pin(v, (w, h), e, &self.inv);
        }

        // The pack found no flat ground under the crosshair, or a bed was tapped by day.
        if play && self.hint > 0.0 {
            let (w, h) = { let (w, h) = self.be.size(); (w as f32, h as f32) };
            let k = craft::panel(0, w, h).2 * 0.75;
            let msg = self.hint_msg;
            let bw = (HudPipeline::text_width(msg) as f32 + 6.0) * k;
            HudPipeline::push_tooltip(v, (w - bw) * 0.5, h * 0.62, k, msg);
        }

        // Name of the item just switched to, centred above the hotbar and the hearts.
        // UNVERIFIED: not in b1.7.3 (Beta 1.8 added it); the 2 s, the 0.75 scale and the spot are invented.
        if let (true, Some(name)) = (play && self.tip.1 > 0.0, self.inv.slots[self.touch.hotbar_slot].and_then(items::name)) {
            let (w, h) = { let (w, h) = self.be.size(); (w as f32, h as f32) };
            let k = craft::panel(0, w, h).2;
            let (hx, hy, hw, _) = layout.hotbar[4];
            let bw = (HudPipeline::text_width(name) as f32 + 6.0) * k * 0.75;
            HudPipeline::push_tooltip(v, hx + hw * 0.5 - bw * 0.5, hy - 34.0 * k, k * 0.75, name);
        }

        if play {
            // Hearts (GuiIngame: 10, 8 units apart on the 20 unit hotbar cells, left half, above the hotbar): a full
            // heart is 2 health. Air bubbles sit over the right half while the eye is under water: whole ones
            // `ceil((air - 2) * 10 / 300)`, then the one that is about to pop. Armor sits on the row above, left (`GuiIngame`).
            let (hx, hy, hw, _) = layout.hotbar[0];
            let (sz, y) = (hw * 0.45, hy - hw * 0.6);
            for i in 0..10 {
                let x = hx + i as f32 * hw * 0.4;
                let at = 2 * i + 1;
                HudPipeline::push_gui(v, x, y, sz, sz, sp::HEART_BG);
                if at < self.vitals.health {
                    HudPipeline::push_gui(v, x, y, sz, sz, sp::HEART_FULL);
                } else if at == self.vitals.health {
                    HudPipeline::push_gui(v, x, y, sz, sz, sp::HEART_HALF);
                }
            }
            // Stamina (`StaminaFeatureClient.renderStamina`): the half hearts it no longer covers, from the right, get a grey half-heart overlay
            // (alpha 0x9c, 0xcc while locked), 4 of the heart's 9 px each. ponytail: the hearts do not shake at low health here, so neither does it.
            let (st, hp) = (&self.vitals.stamina, self.vitals.health.min(vitals::MAX_HEALTH));
            for h in st.halves(hp)..hp {
                let (u, spr) = (if h % 2 == 0 { 1.0 } else { 5.0 }, sp::STAMINA);
                HudPipeline::push_gui_tinted(v, hx + (h / 2) as f32 * hw * 0.4 + u * sz / 9.0, y, sz * 4.0 / 9.0, sz, (spr.0 + u, spr.1, 4.0, spr.3), [1.0, 1.0, 1.0, if st.locked { 0.8 } else { 0.612 }]);
            }
            // Armor bar (GuiIngame): 10 shields over the hearts, a full one is 2 armor points, a half one 1.
            if self.vitals.armor > 0 {
                for i in 0..10 {
                    let x = hx + i as f32 * hw * 0.4;
                    let at = 2 * i + 1;
                    let s = if at < self.vitals.armor { sp::ARMOR_FULL } else if at == self.vitals.armor { sp::ARMOR_HALF } else { sp::ARMOR_EMPTY };
                    HudPipeline::push_gui(v, x, y - sz * 1.2, sz, sz, s);
                }
            }
            // C4 (NOT b1.7.3): the healing food bar, 10 icons of 2 points from the hotbar's right edge, on the hearts' row; a plain square each
            // until the food sprites arrive (C6). Orange, red under the healing floor.
            if world::combat::rules::on() {
                let (rx, _, rw, _) = layout.hotbar[8];
                for i in 0..10 {
                    let x = rx + rw - sz - i as f32 * hw * 0.4;
                    let f = ((self.vitals.food.level - 2 * i) as f32 / 2.0).clamp(0.0, 1.0);
                    HudPipeline::push_quad(v, x, y, sz, sz, [0.0, 0.0, 0.0, 0.45]);
                    if f > 0.0 {
                        HudPipeline::push_quad(v, x, y, sz * f, sz, if self.vitals.food.hungry() { [0.85, 0.3, 0.2, 0.95] } else { [0.9, 0.6, 0.2, 0.95] });
                    }
                }
            }
            if self.vitals.air < vitals::MAX_AIR {
                let air = self.vitals.air as f32;
                let whole = ((air - 2.0) * 10.0 / 300.0).ceil().max(0.0) as i32;
                let popping = (air * 10.0 / 300.0).ceil().max(0.0) as i32 - whole;
                let (rx, _, rw, _) = layout.hotbar[8];
                for i in 0..whole + popping {
                    let s = if i < whole { sp::BUBBLE } else { sp::BUBBLE_POP };
                    HudPipeline::push_gui(v, rx + rw - sz - i as f32 * hw * 0.4, y - hw * 0.5, sz, sz, s);
                }
            }

            // A pad, keyboard or mouse in hand hides the touch buttons (Controllify's `InputMode`); they stay where they are and still answer a finger.
            // Touch d-pad (Stone D-pad sprites): the four arrows always, a corner only while an arrow beside it is held; the held button
            // is drawn pressed. The cell is the hit area, the button is drawn `dpad_gap` smaller inside it, a corner (18 of 22 px)
            // smaller still and centred.
            let t = &self.touch;
            for i in (0..8).filter(|&i| !t.pad.hud_hidden() && t.dpad_shown(i)) {
                let spr = if t.dir == Some(i) { sp::DPAD_PRESSED[i] } else { sp::DPAD[i] };
                let (cx, cy, cw, ch) = layout.dpad[i];
                let k = (cw - layout.dpad_gap) / 22.0 * spr.2; // the arrows are 22 px, a corner 18: same pixel size
                HudPipeline::push_gui(v, cx + (cw - k) * 0.5, cy + (ch - k) * 0.5, k, k, spr);
            }
            if !t.pad.hud_hidden() {
                let (sx, sy, sw, sh) = layout.sneak;
                let g = layout.dpad_gap * 0.5;
                HudPipeline::push_gui(v, sx + g, sy + g, sw - 2.0 * g, sh - 2.0 * g, if t.sneak { sp::SNEAK_ON } else { sp::SNEAK });
                let (jx, jy, jw, jh) = layout.jump;
                HudPipeline::push_gui(v, jx, jy, jw, jh, if t.jumping() { sp::JUMP_ON } else { sp::JUMP });
            }
        }

        // The LoD debug mode that is on, centred at the top (also while playing, not only in the pause menu).
        if let Some(title) = self.lod.as_ref().map(|l| l.debug_title()).filter(|t| !t.is_empty() && !info_on) {
            let (w, dh) = (self.be.size().0 as f32, (self.be.size().1 as f32 * 0.05).max(14.0));
            let tw = HudPipeline::text_width(title) as f32;
            let k = (dh / 10.0).min(w * 0.55 / tw);
            let (tx, ty) = (w * 0.5, dh * 0.45);
            HudPipeline::push_quad(v, tx - tw * k * 0.5 - k * 3.0, ty - k * 2.0, tw * k + k * 6.0, 8.0 * k + k * 4.0, [0.0, 0.0, 0.0, 0.55]);
            HudPipeline::push_text_centered(v, tx, ty, k, title, [1.0, 1.0, 0.4, 1.0]);
        }

        // The debug screen (`render::debug`): two panels in the GUI scale.
        if let Some((left, right)) = &dbg {
            let (w, h) = self.be.size();
            let (k, ..) = crate::menu::scaled_gs(w as f32, h as f32, self.opts.gui_scale);
            crate::render::debug::draw(v, w as f32, k, left, right);
        }

        // The game menu (`GuiIngameMenu`, the death screen when dead), over the dimmed frame.
        if paused {
            let (w, h) = { let (w, h) = self.be.size(); (w as f32, h as f32) };
            if let Some(p) = &self.pause {
                p.draw(v, w, h, &self.opts, &pause_info);
            }
        }

        // The pad's radial hotbar, and its cursor in any GUI (the pause menu, a container).
        {
            let (w, h) = { let (w, h) = self.be.size(); (w as f32, h as f32) };
            let t = &self.touch;
            let cur = (t.pad.shows_cursor() && (paused || t.screen) && self.sat.is_none()).then_some(t.cursor_pos); // the satchel frames its target instead
            t.pad.draw(v, w, h, t.hotbar_slot, cur);
        }

        // Cap to the buffer capacity; extra quads are silently dropped.
        let max = crate::gles::HUD_QUADS * 6;
        if v.len() > max {
            v.truncate(max);
        }
    }
}

/// The chunk manager of a dimension of the world with this seed: the overworld's (of the world's type) or the Nether's.
fn chunk_manager(seed: i64, ty: WorldType, nether: bool) -> ChunkManager {
    if nether { ChunkManager::new_nether(seed, RENDER_DIST) } else { ChunkManager::new_typed(seed, RENDER_DIST, ty) }
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

/// The biome colour of a block icon: the plains climate (temperature 0.8, humidity 0.4), as `rgb` floats.
fn icon_tint(c: u32) -> [f32; 3] {
    [(c >> 16 & 255) as f32 / 255.0, (c >> 8 & 255) as f32 / 255.0, (c & 255) as f32 / 255.0]
}

/// The text of the open cloth (`world::pack`), over the 3D scene `render::items` draws: each place is projected from its spot on the
/// cloth through `vp` (this frame's camera), so labels, counts, wear bars, the toggles' numbers and the name of the stack under a
/// finger sit on what they belong to. `finger` = (px, the cloth unit under it, the slot it is over); `down` = a finger is down (then the stack's name shows).
fn draw_pack(v: &mut Vec<HudVertex>, (w, h): (f32, f32), (m, vp): (&pack::Mat, glam::Mat4), inv: &Inventory, s: &Screen, sc: &pack::Scene, (one, shift): (bool, bool), (cursor_at, finger, picked): ((f32, f32), Option<(f32, f32)>, Option<craft::SlotId>), down: bool, focus: (f32, f32)) {
    // A cloth unit `h` blocks up, in px, and the px a unit measures there.
    let to_px = |x: f32, y: f32, hh: f32| {
        let c = vp * m.at(x, y, hh).extend(1.0);
        (c.w > 0.0).then(|| ((c.x / c.w * 0.5 + 0.5) * w, (0.5 - c.y / c.w * 0.5) * h))
    };
    let unit_px = |x: f32, y: f32, hh: f32| Some((to_px(x + 10.0, y, hh)?.0 - to_px(x, y, hh)?.0).abs() / 10.0);
    let ink = [0.96, 0.9, 0.72, 1.0];
    let label = |v: &mut Vec<HudVertex>, x: f32, y: f32, k: f32, text: &str| {
        if let (Some(p), Some(u)) = (to_px(x, y, 0.05), unit_px(x, y, 0.05)) {
            HudPipeline::push_text(v, p.0, p.1, u * k, text, ink);
        }
    };
    label(v, pack::CRAFT[0] + 4.0, pack::CRAFT[1] + 3.0, 0.6, "Crafting");
    label(v, pack::STAND[0] + 4.0, pack::STAND[1] + 3.0, 0.5, "Armor");
    for (cat, r) in pack::ZONES {
        label(v, r[0] + 3.0, r[1] + 2.5, 0.6, cat.label());
    }
    // The 3D buttons at the edges of the view: a word on each (the toggles show their number while on).
    let centred = |v: &mut Vec<HudVertex>, r: [f32; 4], k: f32, text: &str| {
        if let (Some(p), Some(u)) = (to_px(r[0] + r[2] * 0.5, r[1] + r[3] * 0.5, pack::BTN_H), unit_px(r[0], r[1], pack::BTN_H)) {
            let t = u * k;
            HudPipeline::push_text(v, p.0 - HudPipeline::text_width(text) as f32 * t * 0.5, p.1 - 4.0 * t, t, text, [1.0; 4]);
        }
    };
    for b in pack::BTNS {
        let text = match b {
            pack::Btn::Close => "X",
            pack::Btn::Shift => if shift { "2" } else { "Shift" },
            pack::Btn::One => if one { "1" } else { "One" },
            pack::Btn::Sort => "Sort",
            pack::Btn::Tab(i) => pack::TABS[i],
        };
        centred(v, pack::btn_rect(b, focus), if matches!(b, pack::Btn::Tab(_)) { 0.75 } else { 0.55 }, text);
    }
    // The count and wear bar of every stack that has come up, in its cell seen at its top.
    for l in sc.lays.iter().filter(|l| l.grow >= 1.0) {
        let t = l.top();
        if let (Some(a), Some(b)) = (to_px(l.r[0], l.r[1], t), to_px(l.r[0] + l.r[2], l.r[1] + l.r[3], t)) {
            push_overlay(v, (a.0.min(b.0), a.1.min(b.1), (b.0 - a.0).abs(), (b.1 - a.1).abs()), l.stack);
        }
    }
    // A finger down names the stack it holds or rests on.
    if down {
        let (cx, cy) = cursor_at;
        let under = || picked.and_then(|id| s.get(inv, None, id));
        if let (Some(name), Some(u)) = (s.cursor.or_else(under).and_then(items::name), unit_px(finger.map_or(200.0, |f| f.0), finger.map_or(116.0, |f| f.1), 0.05)) {
            let tk = u * 0.75;
            let bw = (HudPipeline::text_width(name) as f32 + 6.0) * tk;
            let ty = if cy > 40.0 * tk { cy - 36.0 * tk } else { cy + 16.0 * tk };
            HudPipeline::push_tooltip(v, (cx - bw * 0.5).clamp(0.0, (w - bw).max(0.0)), ty, tk, name);
        }
    }
}

/// A block stack as the inventory draws it (`RenderItem.drawItemIntoGui`): cubes (a slab half high) as an isometric cube with the
/// grass and leaf tints, everything with another shape (plants, torches, rails, the ladder, wire, doors, signs, the lever, the
/// button, the bed...) as its flat tile. `false`: the block has no tile (the caller draws the flat colour).
fn push_block_icon(v: &mut Vec<HudVertex>, (x, y, w, h): (f32, f32, f32, f32), s: ItemStack) -> bool {
    let (id, meta) = (s.id as u8, items::placed_meta(s));
    let tile = |side: u8| atlas::terrain_tile(id, meta, side).map(u16::from);
    let flat = pack::flat_icon(id);
    let grass = icon_tint(crate::world::climate::grass(0.8, 0.4));
    let white = [1.0; 3];
    if flat {
        let Some(t) = tile(2) else { return false };
        let rgb = if id == 31 && meta != 0 { grass } else { white };
        HudPipeline::push_tile(v, x, y, w, h, t, rgb);
        return true;
    }
    let (Some(top), Some(left), Some(right)) = (tile(1), tile(3), tile(5)) else { return false };
    let tint = match id {
        2 => [grass, white, white],
        18 => {
            let c = match meta & 3 { 1 => crate::world::climate::PINE, 2 => crate::world::climate::BIRCH, _ => crate::world::climate::foliage(0.8, 0.4) };
            [icon_tint(c); 3]
        }
        _ => [white; 3],
    };
    let overlay = (id == 2).then_some((38, grass));
    // The cactus tiles are 14 wide inside a transparent border (the world mesh pulls its faces in by 1/16): trim it so the cube stays solid.
    HudPipeline::push_cube(v, (x, y, w, h), [(top, tint[0]), (left, tint[1]), (right, tint[2])], overlay, if id == 44 { 0.5 } else { 1.0 }, if id == 81 { 1.0 } else { 0.0 });
    true
}

/// One item stack drawn into the 16 x 16 unit item box `(x, y, w, h)` (`hw / 16` px a unit): the flat colour of its tile (a tool is a stick-coloured handle
/// with a head in the material colour, shaped per kind, so pickaxe, axe, shovel, sword and hoe can be told apart), a wear bar
/// under a damaged tool, and the count bottom right in the font with a shadow like `RenderItem.renderItemOverlayIntoGUI`
/// (shown above 1 only).
// UNVERIFIED: the head shapes and the wear-bar colours are stand-ins for the item sprites and `renderItemOverlay`.
fn push_stack(v: &mut Vec<HudVertex>, (cx, cy, cw, ch): (f32, f32, f32, f32), fill: f32, s: ItemStack) {
    let u = cw / 16.0;
    // The count and the wear bar keep the 16 unit box; the icon alone is drawn `fill` of it, centred (the hotbar draws it a little smaller).
    let (hx, hy, hw, hh) = (cx + cw * (1.0 - fill) * 0.5, cy + ch * (1.0 - fill) * 0.5, cw * fill, ch * fill);
    let rgb = |tile: u16| {
        let c = atlas::tile_color(tile);
        [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, 1.0]
    };
    let (ix, iy, iw, ih) = (hx + u, hy + u, hw - u * 2.0, hh - u * 2.0);
    // Blocks are their isometric cube (plants, torches, rails... their flat tile), items their `gui/items.png` sprite (tools and shears
    // included); anything without either keeps the flat shapes.
    if !(s.id < 256 && push_block_icon(v, (hx, hy, hw, hh), s)) {
    match (s.id >= 256).then(|| atlas::item_icon(s.id)).flatten() {
    Some(icon) => HudPipeline::push_sprite(v, hx, hy, hw, hh, icon),
    None => match craft::tool(s.id) {
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
    }}}
    push_overlay(v, (cx, cy, cw, ch), s);
}

/// The wear bar under a damaged tool and the count of a stack, in the 16 unit box `(cx, cy, cw, ch)` (what `push_stack` ends with; the
/// pack's 3D stacks have only this on the HUD).
fn push_overlay(v: &mut Vec<HudVertex>, (cx, cy, cw, _): (f32, f32, f32, f32), s: ItemStack) {
    let u = cw / 16.0;
    if let Some(max) = craft::max_damage(s.id).filter(|_| s.damage > 0) {
        // `renderItemOverlayIntoGUI`: a 13 x 2 unit bar at (2, 13), black behind the green-to-red fill.
        let left = (1.0 - s.damage as f32 / max as f32).clamp(0.0, 1.0);
        HudPipeline::push_quad(v, cx + 2.0 * u, cy + 13.0 * u, 13.0 * u, 2.0 * u, [0.0, 0.0, 0.0, 1.0]);
        HudPipeline::push_quad(v, cx + 2.0 * u, cy + 13.0 * u, 13.0 * u * left, u, [1.0 - left, left, 0.0, 1.0]);
    }
    if s.count > 1 {
        HudPipeline::push_count(v, cx, cy, u, s.count as u32);
    }
}

/// Match the swapchain to the real window size. Hiding the nav bar or rotating changes the
/// window size; a swapchain left at the old size gets scaled by Android to fit = stretched.
fn sync_size(app: &AndroidApp, state: &mut Option<App>) {
    let (Some(a), Some(win)) = (state.as_mut(), app.native_window()) else { return };
    let (w, h) = (win.width().max(1) as u32, win.height().max(1) as u32);
    if (w, h) != a.be.size() {
        a.resize(w, h);
    }
}

/// Spawn the GPU init thread for the current window (Vulkan, or GLES where there is no Vulkan driver).
fn start_init(app: &AndroidApp) -> Option<std::thread::JoinHandle<Result<App, String>>> {
    struct NativePtr(usize);
    unsafe impl Send for NativePtr {}
    let window = app.native_window()?;
    let ptr = window.ptr().as_ptr() as usize;
    let init_w = window.width().max(1) as u32;
    let init_h = window.height().max(1) as u32;
    // App-private storage, where the world is saved.
    let dir = app.internal_data_path().unwrap_or_else(std::env::temp_dir);
    log::info!("M3: InitWindow, native {init_w}x{init_h}, starting GPU init thread");
    let h = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let r = std::panic::catch_unwind(|| pollster::block_on(App::init(ptr as *mut c_void, init_w, init_h, dir)));
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
    Some(h)
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
    let mut last_input = Instant::now();

    while running {
        // Once the world is up the loop must not wait for events: an empty poll sleeps its whole timeout, and that 8 ms was added to EVERY
        // frame without a touch event (CPU time + 8 ms > 16.7 ms drops a 60 fps frame to 30). Frames are paced by the swap chain (Fifo).
        // Before the window and the GPU exist there is nothing to draw, so the loop may sleep.
        // A menu left alone for a second (the world is frozen behind it) idles in the poll itself: 33 ms on the title screen, 90 ms behind the
        // game menu. Any touch or pad event ends the poll at once, so the menu answers a finger in one frame instead of up to 90 ms later.
        // Not while the atmosphere editor is up: the clouds drift behind it, so it runs at full rate.
        let wait = match app_state.as_ref() {
            None => Duration::from_millis(8),
            Some(a) if (a.touch.paused || a.menu.is_some()) && a.opts.atmos.view.is_none() && !a.touch.pad.active() && last_input.elapsed() > Duration::from_secs(1) => {
                Duration::from_millis(if a.menu.is_some() { 33 } else { 90 })
            }
            Some(_) => Duration::ZERO,
        };
        app.poll_events(Some(wait), |event| {
            if let PollEvent::Main(main_event) = event {
                match main_event {
                    MainEvent::InitWindow { .. } => {
                        immersive::hide_system_bars(&app);
                        init_handle = start_init(&app);
                    }
                    MainEvent::InputAvailable => {
                        last_input = Instant::now();
                        if let Ok(mut iter) = app.input_events_iter() {
                            // `next` handles one queued event per call; loop so
                            // multi-touch moves and Up events are never dropped.
                            while iter.next(|event| match event {
                                // Game-pad sticks / triggers / hat, and its buttons (reported handled: Android would turn B into Back).
                                IEv::MotionEvent(m) if input::gamepad::is_pad_motion(&m) => {
                                    if let Some(a) = app_state.as_mut() {
                                        a.touch.pad.on_motion(&m);
                                    }
                                    InputStatus::Handled
                                }
                                IEv::KeyEvent(k) => match app_state.as_mut() {
                                    // not a guard: a guard binds `a` immutably, `on_key` needs it mutable
                                    Some(a) => if a.touch.pad.on_key(&k) {
                                        InputStatus::Handled
                                    } else {
                                        a.touch.pad.kbm |= matches!(k.source(), android_activity::input::Source::Keyboard);
                                        InputStatus::Unhandled
                                    },
                                    None => InputStatus::Unhandled,
                                },
                                IEv::MotionEvent(m) => {
                                    if let Some(a) = app_state.as_mut() {
                                        match m.source() {
                                            android_activity::input::Source::Mouse => a.touch.pad.kbm = true,
                                            android_activity::input::Source::Touchscreen => a.touch.pad.kbm = false,
                                            _ => {}
                                        }
                                        a.on_motion(&m);
                                    }
                                    InputStatus::Unhandled
                                }
                                _ => InputStatus::Unhandled,
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
                        if let (Some(a), Some(hz)) = (app_state.as_mut(), immersive::refresh_hz(&app)) {
                            a.set_refresh_hz(hz); // the panel may have switched mode while we were away
                        }
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
                            if let (Some(a), Some(hz)) = (app_state.as_mut(), immersive::refresh_hz(&app)) {
                                a.set_refresh_hz(hz);
                            }
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
            let frame_start = Instant::now();
            a.render();
            if let Some(left) = a.opts.frame_cap().and_then(|cap| cap.checked_sub(frame_start.elapsed())) {
                std::thread::sleep(left); // `Performance`: Balanced 120 fps, Power saver 40
            }
            #[cfg(feature = "pad-rumble")]
            {
                if let Some((dev, ms, amp)) = a.touch.pad.take_rumble(&a.opts.pad) {
                    rumble::buzz(&app, dev, ms, amp);
                }
            }
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
