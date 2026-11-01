//! The options and the pause menu, ported from b1.7.3: `GameSettings` (+ `EnumOptions`, `options.txt`), `GuiOptions`, `GuiVideoSettings`,
//! `GuiControls`, `GuiSlider`, `GuiSmallButton` and `GuiIngameMenu`. Laid out in GUI units like `menu.rs` (`ScaledResolution`, now with the
//! GUI Scale option) and drawn with the HUD pipeline; the strings are `lang/en_US.lang`'s.
//!
//! Layout (not b1.7.3's three nested screens): ONE options screen with a tab bar along the top, so any category is a single tap away:
//! General (sound, difficulty, GUI scale, bobbing) | Graphics (quality + LoD) | Perf. (frame cap, Dynamic Res, MSAA, FXAA, Post, Renderer) |
//! Effects (Vibrance + the look switches) | Controls (Keys / Controller sub-tabs, with the look and dig settings). `Done` always closes;
//! on a pad LB / RB step the tabs (`OptionsUi::step_tab`). The game menu keeps b1.7.3's buttons plus `Tools...` (Pregen, Fly, Info, LoD debug).
//!
//! Added on top of b1.7.3 (it has none of them): the `LoD: ON/OFF` button (now on the Graphics tab), and the pause menu's
//! `Pregen` / `Fly` / `LoD debug` buttons (top-left, kept from the old pause menu).
//! UNVERIFIED: the default render distance is Short (4 chunks, what mc-rs always drew), not Far as in `GameSettings`.
//! Stored and saved but with no effect yet: Music and Sound (M9: now wired, see `audio/`), 3D Anaglyph, Advanced OpenGL, Graphics (Fancy/Fast),
//! Smooth Lighting and the key bindings (a touch screen has no keys; `OptionsUi::key` takes one when a keyboard event exists).
//! Achievements and Statistics open `stats_ui` (`PauseEv::Stats`).

use crate::input::gamepad::{Btn, PadCfg, ACTS};
use crate::input::touch_ui::{ScreenEv, HOLD_SECS, SWING_SECS};
use crate::menu::{dirt, grad, rect, rgb, scaled_gs, tw};
use crate::ui;
use crate::render::hud::{HudPipeline, HudVertex};
use crate::render::look::LookCfg;
use crate::world::atmos::{self, AtmosCfg};
use crate::world::biome::Biome;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// `EnumOptions`, in its order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opt {
    Music,
    Sound,
    InvertMouse,
    Sensitivity,
    RenderDistance,
    ViewBobbing,
    Anaglyph,
    AdvancedOpengl,
    FramerateLimit,
    Difficulty,
    Graphics,
    AmbientOcclusion,
    GuiScale,
    /// Added: how long a finger or the attack button rests on a block before the arm swings and the dig starts.
    DigDelay,
    /// Added: samples per pixel of the scene pass (Auto, Off, 2x, 4x). Read once at start-up: the pipelines are built with it.
    Msaa,
    /// Added: the final pass, Auto / Full (CAS, and FXAA when it is on) / Fast (neither). Read once at start-up, like MSAA.
    Post,
    /// Added: FXAA in the final pass, Auto (only without MSAA) / On (also over MSAA, beats Fast) / Off. Read once at start-up, like MSAA.
    Fxaa,
    Renderer,
    /// Added: the scene scale follows the GPU load (`gpu::context::Drs`) or stays at `START_PCT`. Takes effect at once.
    DynamicRes,
    /// Added: `Window.setSustainedPerformanceMode` (clocks the chip can hold, less thermal throttling, a lower peak). Takes effect at once.
    Sustained,
    /// Added: the one dial of the colour grade (`render::look`), 0 = off. Takes effect at once.
    Vibrance,
    /// Added (Effects screen): warm directional light, water depth + glint, soft halos on emitters, motion (off / low / on), aerial haze.
    FxWarm,
    FxWater,
    FxGlow,
    FxMotion,
    FxHaze,
}

impl Opt {
    /// `getEnumFloat`: a slider.
    fn slider(self) -> bool {
        matches!(self, Opt::Music | Opt::Sound | Opt::Sensitivity | Opt::DigDelay | Opt::Vibrance)
    }

    /// `lang/en_US.lang`'s `options.*`.
    fn name(self) -> &'static str {
        match self {
            Opt::Music => "Music",
            Opt::Sound => "Sound",
            Opt::InvertMouse => "Invert Mouse",
            Opt::Sensitivity => "Sensitivity",
            Opt::RenderDistance => "Render Distance",
            Opt::ViewBobbing => "View Bobbing",
            Opt::Anaglyph => "3D Anaglyph",
            Opt::AdvancedOpengl => "Advanced OpenGL",
            Opt::FramerateLimit => "Performance",
            Opt::Difficulty => "Difficulty",
            Opt::Graphics => "Graphics",
            Opt::AmbientOcclusion => "Smooth Lighting",
            Opt::GuiScale => "GUI Scale",
            Opt::DigDelay => "Dig Delay",
            Opt::Msaa => "MSAA",
            Opt::Post => "Post",
            Opt::Fxaa => "FXAA",
            Opt::Renderer => "Renderer",
            Opt::DynamicRes => "Dynamic Res",
            Opt::Sustained => "Sustained Perf",
            Opt::Vibrance => "Vibrance",
            Opt::FxWarm => "Warm Light",
            Opt::FxWater => "Water Look",
            Opt::FxGlow => "Glow",
            Opt::FxMotion => "Motion",
            Opt::FxHaze => "Haze",
        }
    }
}

/// The Dig Delay slider's lower end, seconds.
const DIG_MIN: f32 = 0.10;

const KEY_NAMES: [&str; 10] = ["Forward", "Left", "Back", "Right", "Jump", "Sneak", "Drop", "Inventory", "Chat", "Toggle Fog"];
/// `GameSettings.keyBindings`' ids, for `options.txt` (`key_key.forward`, ...).
const KEY_IDS: [&str; 10] = ["forward", "left", "back", "right", "jump", "sneak", "drop", "inventory", "chat", "fog"];
/// The LWJGL key codes of `GameSettings`: W A S D, space, left shift, Q, E, T, F.
const KEY_DEFAULT: [u32; 10] = [17, 30, 31, 32, 57, 42, 16, 18, 20, 33];

/// `Keyboard.getKeyName` for the keys that matter here.
fn key_name(code: u32) -> String {
    let letter = |row: &str, first: u32| row.chars().nth((code - first) as usize).map(String::from);
    match code {
        57 => "SPACE".into(),
        42 => "LSHIFT".into(),
        54 => "RSHIFT".into(),
        29 => "LCONTROL".into(),
        56 => "LMENU".into(),
        28 => "RETURN".into(),
        15 => "TAB".into(),
        16..=25 => letter("QWERTYUIOP", 16).unwrap_or_default(),
        30..=38 => letter("ASDFGHJKL", 30).unwrap_or_default(),
        44..=50 => letter("ZXCVBNM", 44).unwrap_or_default(),
        _ => format!("KEY{code}"),
    }
}

/// `GameSettings` (+ the LoD switch).
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub music: f32,
    pub sound: f32,
    pub sensitivity: f32,
    /// The Dig Delay slider, 0..1 (`dig_secs`).
    pub dig_delay: f32,
    pub invert_mouse: bool,
    /// 0 Far, 1 Normal, 2 Short, 3 Tiny.
    pub render_distance: u8,
    pub view_bobbing: bool,
    pub anaglyph: bool,
    pub advanced_opengl: bool,
    /// 0 Max FPS, 1 Balanced, 2 Power saver.
    pub limit_framerate: u8,
    /// 0 Peaceful .. 3 Hard.
    pub difficulty: u8,
    pub fancy_graphics: bool,
    pub ambient_occlusion: bool,
    /// 0 Auto, 1 Small, 2 Normal, 3 Large.
    pub gui_scale: u8,
    pub keys: [u32; 10],
    /// Controllify's settings and button bindings (`input/gamepad.rs`).
    pub pad: PadCfg,
    /// The far terrain is drawn (the extra `LoD` button).
    pub lod: bool,
    /// The debug screen (`render::debug`) is drawn instead of the FPS counter (the pause menu's `Info` button, `debugScreen` in options.txt).
    pub debug_screen: bool,
    /// Log a backtrace with every wgpu error, at most one a second (`gpu::context::set_error_trace`); a debug switch with no button.
    pub gpu_error_trace: bool,
    /// See `Opt::Sustained`; on by default (`sustainedPerf` in options.txt).
    pub sustained: bool,
    /// Frames the swap chain may queue (`gpu::context::set_frame_latency`): 2 = as before, 1 = less latency, a frame that needs more than half the refresh period may then drop a step; no button. Next start.
    pub frame_latency: u8,
    /// Smart animals (`Mobs::smart`, not b1.7.3): path-walked wandering, herding, panic, and the grazing / rooting / pecking / resting pauses; a switch with no button, `animalAi:false` in options.txt turns all of it off.
    pub animal_ai: bool,
    /// NOT b1.7.3 (`world::biomelife`): fog is cover for the mobs, heat and cold weigh on stamina, the forests have seasons and leaves give
    /// apples; a switch with no button, `biomeRules:false` in options.txt turns all of it off.
    pub biome_rules: bool,
    /// The inventory button spreads the old cloth on the ground and pans a camera over it (`world::pack`, dizzying) instead of opening the
    /// satchel and its recipe book (`satchel.rs`); a switch with no button, `packCloth:true` in options.txt.
    pub pack_cloth: bool,
    /// The recipe book hides what the player has not met yet: a recipe none of whose items was ever held shows as a silhouette and `???`
    /// (`world::book::Notes::known`). The book's own toggle, `bookDiscovery` in options.txt.
    pub book_discovery: bool,
    /// The scene scale follows the GPU load (`gpu::context::Drs`); off (`dynamicRes:false`, the Perf. tab's button) it stays at `START_PCT`.
    pub dynamic_res: bool,
    /// MSAA samples for the next start: 0 Auto (`present::pick_samples` decides by GPU), 1, 2 or 4 (`msaa:` in options.txt).
    pub msaa: u8,
    /// The final pass for the next start: 0 Auto (`present::pick_final` decides by GPU), 1 Full, 2 Fast (`post:` in options.txt).
    pub post: u8,
    /// FXAA for the next start: 0 Auto (on only when the scene has one sample), 1 On, 2 Off (`fxaa:` in options.txt).
    pub fxaa: u8,
    /// The renderer for the next start: 0 Auto (Vulkan), 1 Vulkan, 2 OpenGL ES 3.2 (`renderer:` in options.txt).
    pub renderer: u8,
    /// The look of every biome's sky as the player tuned it (the sun button of the game menu, `world::atmos`), `atmos_<n>:` in options.txt.
    pub atmos: AtmosCfg,
    /// The Vibrance dial and the effect switches (`render::look`), `look_*` in options.txt.
    pub look: LookCfg,
    path: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            music: 1.0,
            sound: 1.0,
            sensitivity: 0.5,
            dig_delay: (SWING_SECS - DIG_MIN) / (HOLD_SECS - DIG_MIN),
            invert_mouse: false,
            render_distance: 2,
            view_bobbing: true,
            anaglyph: false,
            advanced_opengl: false,
            limit_framerate: 1,
            difficulty: 2,
            fancy_graphics: true,
            ambient_occlusion: true,
            gui_scale: 0,
            keys: KEY_DEFAULT,
            pad: PadCfg::default(),
            lod: true,
            debug_screen: false,
            gpu_error_trace: false,
            frame_latency: 2,
            animal_ai: true,
            sustained: true,
            biome_rules: true,
            pack_cloth: false,
            book_discovery: false,
            dynamic_res: true,
            msaa: 0,
            post: 0,
            fxaa: 0,
            renderer: 0,
            atmos: AtmosCfg::default(),
            look: LookCfg::default(),
            path: None,
        }
    }
}

impl Settings {
    /// `loadOptions`: `<root>/options.txt`, `name:value` per line; a bad line is skipped.
    pub fn load(root: &Path) -> Self {
        let mut s = Self { path: Some(root.join("options.txt")), ..Self::default() };
        let text = s.path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once(':') else { continue };
            let (f, b, n) = (v.parse::<f32>().ok(), v == "true", v.parse::<u8>().ok());
            match (k, f, n) {
                ("music", Some(f), _) => s.music = f.clamp(0.0, 1.0),
                ("sound", Some(f), _) => s.sound = f.clamp(0.0, 1.0),
                ("mouseSensitivity", Some(f), _) => s.sensitivity = f.clamp(0.0, 1.0),
                ("digDelay", Some(f), _) => s.dig_delay = f.clamp(0.0, 1.0),
                ("viewDistance", _, Some(n)) => s.render_distance = n.min(3),
                ("guiScale", _, Some(n)) => s.gui_scale = n.min(3),
                ("fpsLimit", _, Some(n)) => s.limit_framerate = n.min(2),
                ("difficulty", _, Some(n)) => s.difficulty = n.min(3),
                ("invertYMouse", ..) => s.invert_mouse = b,
                ("bobView", ..) => s.view_bobbing = b,
                ("anaglyph3d", ..) => s.anaglyph = b,
                ("advancedOpengl", ..) => s.advanced_opengl = b,
                ("fancyGraphics", ..) => s.fancy_graphics = b,
                ("ao", ..) => s.ambient_occlusion = b,
                ("lod", ..) => s.lod = b,
                ("debugScreen", ..) => s.debug_screen = b,
                ("gpuErrorTrace", ..) => s.gpu_error_trace = b,
                ("frameLatency", _, Some(n)) if (1..=3).contains(&n) => s.frame_latency = n,
                ("animalAi", ..) => s.animal_ai = b,
                ("biomeRules", ..) => s.biome_rules = b,
                ("packCloth", ..) => s.pack_cloth = b,
                ("bookDiscovery", ..) => s.book_discovery = b,
                ("dynamicRes", ..) => s.dynamic_res = b,
                ("sustainedPerf", ..) => s.sustained = b,
                ("msaa", _, Some(n)) if matches!(n, 0 | 1 | 2 | 4) => s.msaa = n,
                ("post", _, Some(n)) if n <= 2 => s.post = n,
                ("fxaa", _, Some(n)) if n <= 2 => s.fxaa = n,
                ("renderer", _, Some(n)) if n <= 2 => s.renderer = n,
                (k, ..) if k.starts_with("pad_") => s.pad.load(k, v),
                (k, ..) if k.starts_with("atmos_") => s.atmos.load(k, v),
                (k, ..) if k.starts_with("look_") => s.look.load(k, v),
                _ => {
                    if let (Some(i), Ok(code)) = (KEY_IDS.iter().position(|id| k.strip_prefix("key_key.") == Some(*id)), v.parse::<u32>()) {
                        s.keys[i] = code;
                    }
                }
            }
        }
        s
    }

    /// `saveOptions`.
    pub fn save(&self) {
        let Some(p) = &self.path else { return };
        let mut t = format!(
            "music:{}\nsound:{}\ninvertYMouse:{}\nmouseSensitivity:{}\nviewDistance:{}\nguiScale:{}\nbobView:{}\nanaglyph3d:{}\nadvancedOpengl:{}\nfpsLimit:{}\ndifficulty:{}\nfancyGraphics:{}\nao:{}\nlod:{}\ndigDelay:{}\ngpuErrorTrace:{}\ndynamicRes:{}\nmsaa:{}\npost:{}\nfxaa:{}\nrenderer:{}\ndebugScreen:{}\nanimalAi:{}\nbiomeRules:{}\npackCloth:{}\nbookDiscovery:{}\nframeLatency:{}\nsustainedPerf:{}\n",
            self.music, self.sound, self.invert_mouse, self.sensitivity, self.render_distance, self.gui_scale, self.view_bobbing, self.anaglyph, self.advanced_opengl, self.limit_framerate, self.difficulty, self.fancy_graphics, self.ambient_occlusion, self.lod, self.dig_delay, self.gpu_error_trace, self.dynamic_res, self.msaa, self.post, self.fxaa, self.renderer, self.debug_screen, self.animal_ai, self.biome_rules, self.pack_cloth, self.book_discovery, self.frame_latency, self.sustained
        );
        for (id, code) in KEY_IDS.iter().zip(self.keys) {
            t.push_str(&format!("key_key.{id}:{code}\n"));
        }
        self.pad.save_into(&mut t);
        self.atmos.save_into(&mut t);
        self.look.save_into(&mut t);
        if let Err(e) = std::fs::write(p, t) {
            log::warn!("options: {e}");
        }
    }

    /// The render distance in chunks: Far 256 blocks, Normal 128, Short 64, Tiny 32.
    pub fn chunks(&self) -> i32 {
        [16, 8, 4, 2][self.render_distance.min(3) as usize]
    }

    /// `EntityRenderer`'s mouse factor `(0.2 + 0.6 s)^3 * 8`: 1.0 at the default 50%, so the look drag keeps its feel.
    pub fn look_scale(&self) -> f32 {
        (self.sensitivity * 0.6 + 0.2).powi(3) * 8.0
    }

    /// Seconds a finger or the attack button rests on a block before the arm swings and the dig starts: `DIG_MIN` .. `HOLD_SECS` (at
    /// the top the dig waits for the plain hold, as it did before the early dig).
    pub fn dig_secs(&self) -> f32 {
        DIG_MIN + self.dig_delay * (HOLD_SECS - DIG_MIN)
    }

    /// The frame time the Performance option allows (`EntityRenderer`: 120 fps balanced, 40 power saver); none for Max FPS.
    pub fn frame_cap(&self) -> Option<Duration> {
        [None, Some(Duration::from_micros(8333)), Some(Duration::from_millis(25))][self.limit_framerate.min(2) as usize]
    }

    fn float(&self, o: Opt) -> f32 {
        match o {
            Opt::Music => self.music,
            Opt::Sound => self.sound,
            Opt::Sensitivity => self.sensitivity,
            Opt::DigDelay => self.dig_delay,
            Opt::Vibrance => self.look.vibrance,
            _ => 0.0,
        }
    }

    /// `setOptionFloatValue`.
    fn set_float(&mut self, o: Opt, v: f32) {
        match o {
            Opt::Music => self.music = v,
            Opt::Sound => self.sound = v,
            Opt::Sensitivity => self.sensitivity = v,
            Opt::DigDelay => self.dig_delay = v,
            Opt::Vibrance => self.look.vibrance = v,
            _ => {}
        }
    }

    /// `setOptionValue(o, 1)`: the next value, then saved.
    pub fn cycle(&mut self, o: Opt) {
        match o {
            Opt::InvertMouse => self.invert_mouse = !self.invert_mouse,
            Opt::RenderDistance => self.render_distance = (self.render_distance + 1) & 3,
            Opt::GuiScale => self.gui_scale = (self.gui_scale + 1) & 3,
            Opt::ViewBobbing => self.view_bobbing = !self.view_bobbing,
            Opt::AdvancedOpengl => self.advanced_opengl = !self.advanced_opengl,
            Opt::Anaglyph => self.anaglyph = !self.anaglyph,
            Opt::FramerateLimit => self.limit_framerate = (self.limit_framerate + 1) % 3,
            Opt::Difficulty => self.difficulty = (self.difficulty + 1) & 3,
            Opt::Graphics => self.fancy_graphics = !self.fancy_graphics,
            Opt::AmbientOcclusion => self.ambient_occlusion = !self.ambient_occlusion,
            Opt::Msaa => self.msaa = match self.msaa { 0 => 1, 1 => 2, 2 => 4, _ => 0 },
            Opt::Post => self.post = (self.post + 1) % 3,
            Opt::Fxaa => self.fxaa = (self.fxaa + 1) % 3,
            Opt::Renderer => self.renderer = (self.renderer + 1) % 3,
            Opt::DynamicRes => self.dynamic_res = !self.dynamic_res,
            Opt::Sustained => self.sustained = !self.sustained,
            Opt::FxWarm => self.look.warm = !self.look.warm,
            Opt::FxWater => self.look.water = !self.look.water,
            Opt::FxGlow => self.look.glow = !self.look.glow,
            Opt::FxMotion => self.look.motion = (self.look.motion + 1) % 3,
            Opt::FxHaze => self.look.haze = !self.look.haze,
            _ => {}
        }
        self.save();
    }

    /// `getKeyBinding`: the button text, "Name: value".
    pub fn text(&self, o: Opt) -> String {
        let onoff = |b: bool| if b { "ON" } else { "OFF" };
        let v: String = if o.slider() {
            let f = self.float(o);
            match o {
                Opt::Sensitivity if f == 0.0 => "*yawn*".to_string(),
                Opt::Sensitivity if f == 1.0 => "HYPERSPEED!!!".to_string(),
                Opt::Sensitivity => format!("{}%", (f * 200.0) as i32),
                Opt::DigDelay => format!("{} ms", (self.dig_secs() * 1000.0).round() as i32),
                _ if f == 0.0 => "OFF".to_string(),
                _ => format!("{}%", (f * 100.0) as i32),
            }
        } else {
            match o {
                Opt::InvertMouse => onoff(self.invert_mouse).into(),
                Opt::ViewBobbing => onoff(self.view_bobbing).into(),
                Opt::Anaglyph => onoff(self.anaglyph).into(),
                Opt::AdvancedOpengl => onoff(self.advanced_opengl).into(),
                Opt::AmbientOcclusion => onoff(self.ambient_occlusion).into(),
                Opt::DynamicRes => onoff(self.dynamic_res).into(),
                Opt::Sustained => onoff(self.sustained).into(),
                Opt::FxWarm => onoff(self.look.warm).into(),
                Opt::FxWater => onoff(self.look.water).into(),
                Opt::FxGlow => onoff(self.look.glow).into(),
                Opt::FxMotion => ["OFF", "Low", "On"][self.look.motion.min(2) as usize].into(),
                Opt::FxHaze => onoff(self.look.haze).into(),
                Opt::RenderDistance => ["Far", "Normal", "Short", "Tiny"][self.render_distance.min(3) as usize].into(),
                Opt::Difficulty => ["Peaceful", "Easy", "Normal", "Hard"][self.difficulty.min(3) as usize].into(),
                Opt::GuiScale => ["Auto", "Small", "Normal", "Large"][self.gui_scale.min(3) as usize].into(),
                Opt::FramerateLimit => ["Max FPS", "Balanced", "Power saver"][self.limit_framerate.min(2) as usize].into(),
                // The pipelines keep the count they were built with: a change shows only at the next start.
                Opt::Msaa => {
                    let v = match self.msaa { 0 => "Auto", 1 => "Off", 2 => "2x", _ => "4x" };
                    if u32::from(self.msaa) != crate::render::present::msaa_want() { format!("{v} (restart)") } else { v.into() }
                }
                Opt::Post => {
                    let v = ["Auto", "Full", "Fast"][self.post.min(2) as usize];
                    if u32::from(self.post) != crate::render::present::post_want() { format!("{v} (restart)") } else { v.into() }
                }
                Opt::Fxaa => {
                    let v = ["Auto", "On", "Off"][self.fxaa.min(2) as usize];
                    if u32::from(self.fxaa) != crate::render::present::fxaa_want() { format!("{v} (restart)") } else { v.into() }
                }
                Opt::Renderer => {
                    let v = ["Auto", "Vulkan", "OpenGL ES"][self.renderer.min(2) as usize];
                    if self.renderer != crate::gles::want() { format!("{v} (restart)") } else { v.into() }
                }
                _ => (if self.fancy_graphics { "Fancy" } else { "Fast" }).into(),
            }
        };
        format!("{}: {v}", o.name())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OptScr {
    /// Every setting in ONE scrolling list, with the page list beside it (`PAGES`): Sodium's layout in b1.7.3's skin.
    List,
    /// The live atmosphere editor (the sun button of the game menu): the world stays undimmed beside its panel. Only `OptionsUi::atmos` opens it.
    Atmos,
}

/// The number of sections of the list.
const N_PAGES: usize = 7;

/// The sections of the list, in order: the label in the page list and the heading over the section.
const PAGES: [(&str, &str); N_PAGES] = [
    ("General", "General"),
    ("Graphics", "Graphics"),
    ("Performance", "Performance"),
    ("Effects", "Effects"),
    ("Controls", "Controls"),
    ("Pad buttons", "Controller buttons"),
    ("Pad tuning", "Controller tuning"),
];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Act {
    Opt(Opt),
    Lod,
    /// A page of the page list (index into `PAGES`): the list jumps to its section.
    Tab(usize),
    Done,
    Key(usize),
    PadBind(usize),
    PadKnob(usize),
    PadToggle(usize),
    AtTab(usize),
    AtKnob(usize),
    AtStep(i32),
    AtList,
    AtPick(usize),
    AtHere,
    AtReset,
    AtResetAll,
}

/// Which biome and tab the atmosphere editor shows, and whether its biome list is open.
#[derive(Clone, Copy, Debug)]
struct AtmosEd {
    biome: Biome,
    tab: usize,
    list: bool,
}

/// One control: what it does, its rectangle (GUI units) and its text.
struct Wid {
    act: Act,
    r: [f32; 4],
    label: String,
}

/// What an options screen tells its parent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OptEv {
    /// Done on the first screen: back to the parent.
    Close,
}

fn inside(r: [f32; 4], x: f32, y: f32) -> bool {
    x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]
}

fn is_slider(a: Act) -> bool {
    matches!(a, Act::Opt(o) if o.slider()) || matches!(a, Act::PadKnob(_) | Act::AtKnob(_))
}

/// A setting of the list (not a page entry, Done or the editor's controls): it is clipped to the list.
fn is_row(a: Act) -> bool {
    matches!(a, Act::Opt(_) | Act::Lod | Act::Key(_) | Act::PadBind(_) | Act::PadKnob(_) | Act::PadToggle(_))
}

/// Saved, but nothing reads it yet (README, M14b): the key bindings (a touch screen has no keys), Graphics, Smooth Lighting, Advanced
/// OpenGL and 3D Anaglyph. Drawn dim, like Sodium's unavailable options.
fn unused(a: Act) -> bool {
    matches!(a, Act::Key(_) | Act::Opt(Opt::Graphics | Opt::AmbientOcclusion | Opt::AdvancedOpengl | Opt::Anaglyph))
}

fn set_slider(a: Act, st: &mut Settings, v: f32, ed: &AtmosEd) {
    match a {
        Act::Opt(o) => st.set_float(o, v),
        Act::PadKnob(i) => st.pad.set_knob(i, v),
        Act::AtKnob(j) => st.atmos.set(ed.biome, &atmos::TABS[ed.tab].1[j], v),
        _ => {}
    }
}

/// `GuiSlider`'s value for a finger at `x` on the control `r`: its 4 px ends are not part of the track.
fn slider_value(r: [f32; 4], x: f32) -> f32 {
    ((x - (r[0] + 4.0)) / (r[2] - 8.0)).clamp(0.0, 1.0)
}

// ---- the list: its items, where they sit, and the screen's frame ----

/// One line of the list: a section heading, a group heading, or a setting (what a tap does, its text "Name: value").
enum Item {
    Page(usize),
    Group(&'static str),
    Row(Act, String),
}

const PAGE_H: f32 = 18.0;
const GROUP_H: f32 = 13.0;
/// A row is 20 high (a `GuiButton`) and 22 apart.
const ROW_H: f32 = 20.0;
const ROW_PITCH: f32 = 22.0;
const SECTION_GAP: f32 = 6.0;

/// Where each item sits in the list (units from its top), the top of every section, and the list's height.
struct Flow {
    y: Vec<f32>,
    starts: [f32; N_PAGES],
    total: f32,
}

/// Lays the items out top to bottom. It does not depend on the screen's width, so a jump needs no size.
fn flow(items: &[Item]) -> Flow {
    let (mut y, mut at, mut starts) = (0.0f32, Vec::with_capacity(items.len()), [0.0f32; N_PAGES]);
    for it in items {
        match it {
            Item::Page(i) => {
                if *i > 0 {
                    y += SECTION_GAP;
                }
                starts[*i] = y;
                at.push(y);
                y += PAGE_H + 3.0;
            }
            Item::Group(_) => {
                y += 2.0;
                at.push(y);
                y += GROUP_H;
            }
            Item::Row(..) => {
                at.push(y);
                y += ROW_PITCH;
            }
        }
    }
    Flow { y: at, starts, total: y + 4.0 }
}

/// The section the page list lights while the list is scrolled to `scroll` (Sodium's `updateSectionFocus`): the last one that has begun a
/// few rows below the top of the view.
fn which(starts: &[f32; N_PAGES], scroll: f32, view: f32) -> usize {
    let probe = scroll + (3.0 * ROW_PITCH).min(view / 2.0);
    (0..N_PAGES).rev().find(|&i| starts[i] <= probe).unwrap_or(0)
}

/// Where the parts of the options screen sit (GUI units): the page list, the scrolling list, the description (a pane beside the list on a
/// wide screen, a strip under it on a narrow one), Done, and the width of a row's control.
struct Frame {
    side: [f32; 4],
    list: [f32; 4],
    tip: [f32; 4],
    wide: bool,
    done: [f32; 4],
    ctl: f32,
}

fn frame(gw: f32, gh: f32) -> Frame {
    let wide = gw >= 520.0;
    let (side_w, tip_w, gap) = (if wide { 100.0 } else { 80.0 }, if wide { 170.0 } else { 0.0 }, 6.0);
    let list_w = (gw - side_w - tip_w - gap * if wide { 4.0 } else { 3.0 }).min(300.0);
    let total = side_w + gap + list_w + if wide { gap + tip_w } else { 0.0 };
    let x0 = ((gw - total) / 2.0).floor();
    let (top, h) = (30.0, gh - 30.0 - 38.0);
    let list = [x0 + side_w + gap, top, list_w, h];
    let tip = if wide { [list[0] + list_w + gap, top, tip_w, h] } else { [x0, gh - 37.0, total - 86.0, 35.0] };
    Frame { side: [x0, top, side_w, h], list, tip, wide, done: [x0 + total - 80.0, gh - 28.0, 80.0, 20.0], ctl: (list_w * 0.42).floor().clamp(80.0, 104.0) }
}

/// The page list's entry `i`.
fn side_entry(f: &Frame, i: usize) -> [f32; 4] {
    [f.side[0], f.side[1] + i as f32 * 20.0, f.side[2], 18.0]
}

/// The part of a row a slider answers to (a toggle answers all over its row): the control on its right.
fn track(r: [f32; 4], ctl: f32) -> [f32; 4] {
    [r[0] + r[2] - ctl - 2.0, r[1] + 2.0, ctl, 16.0]
}

/// What a finger that went down is doing, decided as it moves: a tap, a slider it is on but has not yet moved along, a drag of the list,
/// a drag of that slider, or nothing (it slid sideways off a plain row: no tap on lift).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Grab {
    Tap,
    Slider(Act),
    Scroll,
    Drag(Act),
    Dead,
}

#[derive(Clone, Copy, Debug)]
struct Press {
    x: f32,
    y: f32,
    /// The list's scroll when the finger went down.
    scroll: f32,
    grab: Grab,
}

/// What a setting is for, by its name (the pad's knobs and switches have none of their own in `Opt`). The sentences are written to fit
/// three lines of the narrow screen's strip; `tools/optlist_proto.py` checks that.
const TIPS: [(&str, &str); 32] = [
    ("Music", "Volume of the background music."),
    ("Sound", "Volume of steps, mobs, blocks and ambience."),
    ("Difficulty", "Peaceful removes monsters. Easy hurts less, Hard hurts more."),
    ("GUI Scale", "Size of menus and the HUD. Auto picks the largest that fits."),
    ("View Bobbing", "The camera sways a little as you walk."),
    ("Render Distance", "Chunks kept around you: Far 16, Normal 8, Short 4, Tiny 2."),
    ("LoD", "Draws far terrain as simplified blocks, out to 768 blocks."),
    ("Performance", "Frame cap: Max FPS, Balanced (120 fps) or Power saver (40 fps)."),
    ("Sustained Perf", "Holds clocks the chip can keep up, so long sessions throttle less. May lower the peak speed."),
    ("Dynamic Res", "Raises and lowers the render resolution (50% to 100%) with the GPU load."),
    ("MSAA", "Samples for smoother edges. Auto picks by GPU. Applies at the next start."),
    ("FXAA", "Smooths edges in the final pass. Auto uses it only without MSAA. Next start."),
    ("Post", "The final pass: Full sharpens and smooths, Fast skips both. Next start."),
    ("Renderer", "Auto and Vulkan use wgpu, OpenGL ES the native one. Next start."),
    ("Vibrance", "Strength of the colour grade. Off turns it off."),
    ("Warm Light", "Golden light at a low sun and bluer shade."),
    ("Water Look", "A depth gradient and the sun's glint on water."),
    ("Glow", "Soft halos on lava, torches, glowstone and the sun."),
    ("Motion", "Leaf sway, motes near flowers and water, light rays at a low sun."),
    ("Haze", "Aerial perspective: far things fade into the sky."),
    ("Sensitivity", "How far a finger drag turns the view."),
    ("Invert Mouse", "Flips the vertical look."),
    ("Dig Delay", "How long a finger rests on a block before digging starts."),
    ("Look X", "Horizontal look speed of the right stick."),
    ("Look Y", "Vertical look speed of the right stick."),
    ("Deadzone", "How far a stick must move before it counts."),
    ("Cursor", "Speed of the pad's pointer in menus."),
    ("Trigger", "How far a trigger is pulled before it counts as pressed."),
    ("Rumble", "Strength of the vibration on damage, digging and blasts."),
    ("Curve", "How stick movement turns into look speed. Tap to change."),
    ("Invert X", "Flips the horizontal look of the right stick."),
    ("Invert Y", "Flips the vertical look of the right stick."),
    ("Toggle Sneak", "Sneak stays on until pressed again."),
];

/// The sentence under a setting's name in the description pane.
fn tip(act: Act, name: &str) -> &'static str {
    if unused(act) {
        "Saved in options.txt, but nothing reads this setting yet."
    } else if matches!(act, Act::PadBind(_)) {
        "Tap, then press the pad button to bind it. Tap another button to cancel."
    } else {
        TIPS.iter().find(|t| t.0 == name).map_or("", |t| t.1)
    }
}

/// The colour of a control's value: green on, red off, gold when it waits for a restart, yellow while it waits for a key.
fn value_color(val: &str) -> u32 {
    if val.eq_ignore_ascii_case("on") {
        0x55FF55
    } else if val.eq_ignore_ascii_case("off") {
        0xFF5555
    } else if val.contains("(restart)") {
        0xFFAA00
    } else if val.starts_with('>') {
        0xFFFF55
    } else {
        0xE0E0E0
    }
}

/// A `GuiButton` (a slider when `knob` has its value): `state` 0 dark, 1 normal, 2 highlighted; the label centred, in `color`.
pub(crate) fn button_c(v: &mut Vec<HudVertex>, s: f32, r: [f32; 4], label: &str, color: u32, state: u8, knob: Option<f32>) {
    HudPipeline::push_button(v, r[0] * s, r[1] * s, r[2] * s, r[3] * s, state);
    if let Some(f) = knob {
        // `GuiSlider.mouseDragged`: the two 4 px halves of the knob, `widgets.png` row 66.
        let x = r[0] + (f * (r[2] - 8.0)).floor();
        HudPipeline::push_gui(v, x * s, r[1] * s, 4.0 * s, r[3] * s, (0.0, 256.0 + 66.0, 4.0, 20.0));
        HudPipeline::push_gui(v, (x + 4.0) * s, r[1] * s, 4.0 * s, r[3] * s, (196.0, 256.0 + 66.0, 4.0, 20.0));
    }
    HudPipeline::push_text(v, (r[0] + (r[2] - tw(label)) / 2.0) * s, (r[1] + (r[3] - 8.0) / 2.0) * s, s, label, rgb(color));
}

/// A `GuiButton`: the label centred, white-ish, yellow under the finger, grey when disabled.
pub(crate) fn button(v: &mut Vec<HudVertex>, s: f32, r: [f32; 4], label: &str, on: bool, over: bool, knob: Option<f32>) {
    let state = if knob.is_some() || !on { 0 } else if over { 2 } else { 1 };
    button_c(v, s, r, label, if !on { 0xA0A0A0 } else if over { 0xFFFFA0 } else { 0xE0E0E0 }, state, knob);
}

/// A section heading of the list: a dark strip with a gold stud, the name in the splash's yellow, a rule under it.
fn heading(v: &mut Vec<HudVertex>, s: f32, r: [f32; 4], text: &str) {
    grad(v, s, r, [0.0, 0.0, 0.0, 0.65], [0.0, 0.0, 0.0, 0.25]);
    rect(v, s, [r[0], r[1] + r[3] - 1.0, r[2], 1.0], 0x808080);
    rect(v, s, [r[0] + 3.0, r[1] + 6.0, 4.0, 4.0], 0xFFAA00);
    HudPipeline::push_text(v, (r[0] + 12.0) * s, (r[1] + 5.0) * s, s, text, rgb(0xFFFF55));
}

/// A group heading inside a section: grey text and a thin rule running to the edge.
fn group(v: &mut Vec<HudVertex>, s: f32, r: [f32; 4], text: &str) {
    HudPipeline::push_text(v, (r[0] + 6.0) * s, (r[1] + 3.0) * s, s, text, rgb(0xAAAAAA));
    let x = r[0] + 6.0 + tw(text) + 6.0;
    rect(v, s, [x, r[1] + 7.0, (r[0] + r[2] - x).max(0.0), 1.0], 0x505050);
}

/// A setting: its name on the left, its control on the right (a stone button holding the value, or a slider), the chosen row framed like
/// the chosen world of the world list.
#[allow(clippy::too_many_arguments)]
fn row(v: &mut Vec<HudVertex>, s: f32, r: [f32; 4], act: Act, label: &str, st: &Settings, ctl: f32, over: bool, sel: bool) {
    let (name, val) = label.split_once(": ").unwrap_or((label, ""));
    let dim = unused(act);
    if sel {
        rect(v, s, r, 0x808080);
        rect(v, s, [r[0] + 1.0, r[1] + 1.0, r[2] - 2.0, r[3] - 2.0], 0x000000);
    } else {
        HudPipeline::push_quad(v, r[0] * s, r[1] * s, r[2] * s, r[3] * s, [0.0, 0.0, 0.0, 0.25]);
    }
    HudPipeline::push_text(v, (r[0] + 6.0) * s, (r[1] + 6.0) * s, s, &ui::fit(name, r[2] - ctl - 14.0), rgb(if dim { 0x808080 } else { 0xE0E0E0 }));
    let knob = match act {
        Act::Opt(o) if o.slider() => Some(st.float(o)),
        Act::PadKnob(i) => Some(st.pad.knob(i)),
        _ => None,
    };
    let c = if dim { 0x808080 } else if over { 0xFFFFA0 } else { value_color(val) };
    button_c(v, s, track(r, ctl), val, c, if knob.is_some() || dim { 0 } else if over { 2 } else { 1 }, knob);
}

/// The atmosphere editor's controls (GUI units, a panel 212 wide down the left edge): the biome (`<` name `>`), the tabs, the tab's
/// sliders, or the biome list, the two resets and Done.
fn atmos_widgets(ed: &AtmosEd, st: &Settings) -> Vec<Wid> {
    let x0 = 4.0;
    let w = |act, r: [f32; 4], label: String| Wid { act, r, label };
    let b = ed.biome;
    let mut v = vec![
        w(Act::AtStep(-1), [x0, 4.0, 20.0, 16.0], "<".into()),
        w(Act::AtList, [x0 + 22.0, 4.0, 168.0, 16.0], format!("{}{}", atmos::name(b), if st.atmos.edited(b) { " *" } else { "" })),
        w(Act::AtStep(1), [x0 + 192.0, 4.0, 20.0, 16.0], ">".into()),
    ];
    for (i, (name, _)) in atmos::TABS.iter().enumerate() {
        v.push(w(Act::AtTab(i), [x0 + (i as f32 * 42.4).floor(), 24.0, 41.0, 14.0], name.to_string()));
    }
    if ed.list {
        v.push(w(Act::AtHere, [x0, 42.0, 212.0, 14.0], format!("Under me: {}", atmos::name(st.atmos.here))));
        for (i, &o) in atmos::ALL.iter().enumerate() {
            v.push(w(Act::AtPick(i), [x0 + (i % 2) as f32 * 107.0, 58.0 + (i / 2) as f32 * 15.0, 105.0, 14.0], atmos::name(o).to_string()));
        }
    } else {
        let look = st.atmos.look(b);
        for (j, k) in atmos::TABS[ed.tab].1.iter().enumerate() {
            v.push(w(Act::AtKnob(j), [x0, 44.0 + j as f32 * 22.0, 150.0, 20.0], k.text(look)));
        }
        v.push(w(Act::AtReset, [x0, 170.0, 104.0, 18.0], "Reset biome".into()));
        v.push(w(Act::AtResetAll, [x0 + 108.0, 170.0, 104.0, 18.0], "Reset all".into()));
    }
    v.push(w(Act::Done, [x0, 214.0, 212.0, 18.0], "Done".into()));
    v
}

/// The sun of the editor's button: a 13 x 13 pixel sun in the style of b1.7.3's `sun.png` (a square yellow disc, edged, with rays), centred on `r`.
fn sun(v: &mut Vec<HudVertex>, s: f32, r: [f32; 4]) {
    let (ox, oy) = (r[0] + ((r[2] - 13.0) / 2.0).floor(), r[1] + ((r[3] - 13.0) / 2.0).floor());
    for y in 0..13i32 {
        for x in 0..13i32 {
            let (dx, dy) = (x - 6, y - 6);
            let core = dx.abs().max(dy.abs());
            let c = if core == 3 {
                0xE08A12
            } else if core < 3 {
                if matches!((dx, dy), (-2, -2) | (-1, -2) | (-2, -1)) { 0xFFF7B8 } else { 0xFFD93B }
            } else if (dx == 0 || dy == 0 || dx.abs() == dy.abs()) && core >= 5 {
                0xFFB62E
            } else {
                continue;
            };
            HudPipeline::push_quad(v, (ox + x as f32) * s, (oy + y as f32) * s, s, s, rgb(c));
        }
    }
}

/// `drawDefaultBackground`: the dimmed world when one is behind, else the dirt.
pub(crate) fn background(v: &mut Vec<HudVertex>, s: f32, gw: f32, gh: f32, world: bool) {
    if world {
        grad(v, s, [0.0, 0.0, gw, gh], [16.0 / 255.0, 16.0 / 255.0, 16.0 / 255.0, 0xC0 as f32 / 255.0], [16.0 / 255.0, 16.0 / 255.0, 16.0 / 255.0, 0xD0 as f32 / 255.0]);
    } else {
        dirt(v, s, [0.0, 0.0, gw, gh], 0.0, 0x404040);
    }
}

/// `GuiOptions` and the two screens under it, `GuiVideoSettings` and `GuiControls`: here one scrolling list (Sodium's layout).
pub struct OptionsUi {
    pub scr: OptScr,
    /// The slider the finger is dragging in the atmosphere editor (the list's sliders go through `press`).
    drag: Option<Act>,
    /// The finger that is down on the list, and what it turned out to be doing.
    press: Option<Press>,
    /// `GuiControls.buttonId`: the key button waiting for its key.
    wait: Option<usize>,
    /// The Controller button waiting for a pad press.
    wait_pad: Option<usize>,
    /// Where the finger is (GUI units), for the highlight.
    finger: Option<(f32, f32)>,
    /// How far the list is scrolled (units); clamped to its length wherever it is used.
    scroll: f32,
    /// The page the page list lights: set by a tap on it, or by the section under the top of the view as a finger scrolls.
    page: usize,
    /// The setting the description pane explains: the last one tapped.
    sel: Option<Act>,
    ed: AtmosEd,
}

impl Default for OptionsUi {
    fn default() -> Self {
        Self::new()
    }
}

impl OptionsUi {
    pub fn new() -> Self {
        Self { scr: OptScr::List, drag: None, press: None, wait: None, wait_pad: None, finger: None, scroll: 0.0, page: 0, sel: None, ed: AtmosEd { biome: Biome::Plains, tab: 0, list: false } }
    }

    /// The atmosphere editor (the game menu's sun), on the biome under the player; the world behind it is the live preview.
    pub fn atmos(st: &mut Settings) -> Self {
        let ed = AtmosEd { biome: st.atmos.here, tab: 0, list: false };
        st.atmos.view = Some(ed.biome);
        Self { scr: OptScr::Atmos, ed, ..Self::new() }
    }

    /// The list is showing (not the atmosphere editor).
    pub fn in_list(&self) -> bool {
        self.scr == OptScr::List
    }

    /// Every line of the list, in order. Each row's text is "Name: value", as `getKeyBinding` made it.
    fn items(&self, st: &Settings) -> Vec<Item> {
        use Item::{Group as G, Page as P, Row};
        let o = |o: Opt| Row(Act::Opt(o), st.text(o));
        let mut v = vec![
            P(0),
            o(Opt::Music),
            o(Opt::Sound),
            o(Opt::Difficulty),
            o(Opt::GuiScale),
            o(Opt::ViewBobbing),
            P(1),
            G("World"),
            o(Opt::Graphics),
            o(Opt::RenderDistance),
            Row(Act::Lod, format!("LoD: {}", if st.lod { "ON" } else { "OFF" })),
            G("Lighting"),
            o(Opt::AmbientOcclusion),
            o(Opt::AdvancedOpengl),
            o(Opt::Anaglyph),
            P(2),
            G("Frame rate"),
            o(Opt::FramerateLimit),
            o(Opt::DynamicRes),
            o(Opt::Sustained),
            G("Next start"),
            o(Opt::Msaa),
            o(Opt::Fxaa),
            o(Opt::Post),
            o(Opt::Renderer),
            P(3),
            o(Opt::Vibrance),
            G("Switches"),
            o(Opt::FxWarm),
            o(Opt::FxWater),
            o(Opt::FxGlow),
            o(Opt::FxMotion),
            o(Opt::FxHaze),
            P(4),
            G("Look and dig"),
            o(Opt::Sensitivity),
            o(Opt::InvertMouse),
            o(Opt::DigDelay),
            G("Keyboard"),
        ];
        v.extend((0..KEY_NAMES.len()).map(|i| {
            let k = key_name(st.keys[i]);
            Row(Act::Key(i), format!("{}: {}", KEY_NAMES[i], if self.wait == Some(i) { format!("> {k} <") } else { k }))
        }));
        v.push(P(5));
        v.extend(ACTS.iter().enumerate().map(|(i, a)| {
            let b = if self.wait_pad == Some(i) { "> <" } else { st.pad.binds[i].name() };
            Row(Act::PadBind(i), format!("{}: {b}", a.name()))
        }));
        v.push(P(6));
        v.extend((0..PadCfg::knob_count()).map(|i| Row(Act::PadKnob(i), st.pad.knob_text(i))));
        v.extend((0..PadCfg::toggle_count()).map(|i| Row(Act::PadToggle(i), st.pad.toggle_text(i))));
        v
    }

    /// The items, where they sit, and how far the list can scroll in `f`.
    fn extent(&self, st: &Settings, f: &Frame) -> (Vec<Item>, Flow, f32) {
        let items = self.items(st);
        let fl = flow(&items);
        let max = (fl.total - f.list[3]).max(0.0);
        (items, fl, max)
    }

    /// Scrolls the list to the top of section `i` and lights its page entry (a section near the end stops where the list does).
    fn jump(&mut self, i: usize, st: &Settings) {
        (self.scroll, self.page) = (flow(&self.items(st)).starts[i], i);
    }

    /// The page list, the settings in view (every one a finger could touch, partly cut ones too) and Done; the editor's own controls
    /// when that is open.
    fn widgets(&self, gw: f32, gh: f32, st: &Settings) -> Vec<Wid> {
        if self.scr == OptScr::Atmos {
            return atmos_widgets(&self.ed, st);
        }
        let f = frame(gw, gh);
        let (items, fl, max) = self.extent(st, &f);
        let scroll = self.scroll.clamp(0.0, max);
        let mut w: Vec<Wid> = items
            .into_iter()
            .zip(&fl.y)
            .filter_map(|(it, &y)| match it {
                Item::Row(act, label) => {
                    let r = [f.list[0] + 2.0, f.list[1] + 4.0 + y - scroll, f.list[2] - 11.0, ROW_H];
                    (r[1] < f.list[1] + f.list[3] && r[1] + r[3] > f.list[1]).then_some(Wid { act, r, label })
                }
                _ => None,
            })
            .collect();
        w.extend(PAGES.iter().enumerate().map(|(i, p)| Wid { act: Act::Tab(i), r: side_entry(&f, i), label: p.0.to_string() }));
        w.push(Wid { act: Act::Done, r: f.done, label: "Done".to_string() });
        w
    }

    /// A finger: down on a setting it selects it (a tap acts on lift); on a slider's track it grabs the slider; dragged up or down it
    /// scrolls the list; the page list jumps to a section; Done leaves. The editor's sliders follow the finger at once.
    pub fn event(&mut self, ev: ScreenEv, w: f32, h: f32, st: &mut Settings) -> Option<OptEv> {
        let (s, gw, gh) = scaled_gs(w, h, st.gui_scale);
        let f = frame(gw, gh);
        let (_, fl, max) = self.extent(st, &f);
        self.scroll = self.scroll.clamp(0.0, max);
        let wids = self.widgets(gw, gh, st);
        // The settings answer only inside the list: a row half under the list's edge is not touched through the footer.
        let at = |x: f32, y: f32| wids.iter().find(|b| inside(b.r, x / s, y / s) && (!is_row(b.act) || inside(f.list, x / s, y / s)));
        match ev {
            ScreenEv::Down(x, y) => {
                let (gx, gy) = (x / s, y / s);
                self.finger = Some((gx, gy));
                let mut grab = Grab::Tap;
                if let Some(Wid { act, r, .. }) = at(x, y).filter(|b| is_slider(b.act)) {
                    if self.scr == OptScr::Atmos {
                        self.drag = Some(*act);
                        set_slider(*act, st, slider_value(*r, gx), &self.ed);
                    } else if inside(track(*r, f.ctl), gx, gy) {
                        grab = Grab::Slider(*act);
                    }
                }
                self.press = Some(Press { x: gx, y: gy, scroll: self.scroll, grab });
                None
            }
            ScreenEv::Move(x, y) => {
                let (gx, gy) = (x / s, y / s);
                self.finger = Some((gx, gy));
                if let Some(a) = self.drag {
                    if let Some(b) = wids.iter().find(|b| b.act == a) {
                        set_slider(a, st, slider_value(b.r, gx), &self.ed);
                    }
                }
                let mut scrolled = false;
                if let Some(p) = self.press.as_mut() {
                    let (dx, dy) = (gx - p.x, gy - p.y);
                    p.grab = match p.grab {
                        Grab::Tap | Grab::Slider(_) if dy.abs() > 4.0 && dy.abs() > dx.abs() => Grab::Scroll,
                        Grab::Slider(a) if dx.abs() > 3.0 => Grab::Drag(a),
                        Grab::Tap if dx.abs() > 10.0 => Grab::Dead,
                        g => g,
                    };
                    match p.grab {
                        Grab::Scroll => {
                            self.scroll = (p.scroll - dy).clamp(0.0, max);
                            scrolled = true;
                        }
                        Grab::Drag(a) => {
                            if let Some(b) = wids.iter().find(|b| b.act == a) {
                                set_slider(a, st, slider_value(track(b.r, f.ctl), gx), &self.ed);
                            }
                        }
                        _ => {}
                    }
                }
                if scrolled {
                    self.page = which(&fl.starts, self.scroll, f.list[3]);
                }
                None
            }
            ScreenEv::Up(x, y) => {
                self.finger = None;
                if self.drag.take().is_some() {
                    self.press = None;
                    st.save(); // the editor's slider is let go
                    return None;
                }
                match self.press.take().map_or(Grab::Tap, |p| p.grab) {
                    Grab::Drag(_) => {
                        st.save();
                        return None;
                    }
                    Grab::Scroll | Grab::Dead => return None,
                    Grab::Slider(a) => {
                        // a tap on the track puts the slider there
                        if let Some(b) = wids.iter().find(|b| b.act == a) {
                            set_slider(a, st, slider_value(track(b.r, f.ctl), x / s), &self.ed);
                            st.save();
                            self.sel = Some(a);
                        }
                        return None;
                    }
                    Grab::Tap => {}
                }
                let act = at(x, y)?.act;
                if is_row(act) {
                    self.sel = Some(act);
                    if is_slider(act) {
                        return None; // on a slider's name: selected, nothing to flip
                    }
                }
                self.act(act, st)
            }
        }
    }

    fn act(&mut self, a: Act, st: &mut Settings) -> Option<OptEv> {
        self.wait = None; // `GuiControls`: any button ends the wait
        self.wait_pad = None;
        match a {
            Act::Opt(o) => st.cycle(o),
            Act::Lod => {
                st.lod = !st.lod;
                st.save();
            }
            Act::Tab(i) => {
                st.save();
                self.jump(i, st);
            }
            Act::Key(i) => self.wait = Some(i),
            Act::PadBind(i) => self.wait_pad = Some(i),
            Act::PadKnob(_) => {}
            Act::PadToggle(i) => {
                st.pad.toggle(i);
                st.save();
            }
            Act::AtTab(i) => (self.ed.tab, self.ed.list) = (i, false),
            Act::AtKnob(_) => {}
            Act::AtList => self.ed.list = !self.ed.list,
            Act::AtStep(d) => self.show(st, atmos::ALL[(atmos::idx(self.ed.biome) as i32 + d).rem_euclid(atmos::ALL.len() as i32) as usize]),
            Act::AtPick(i) => self.show(st, atmos::ALL[i]),
            Act::AtHere => {
                let b = st.atmos.here;
                self.show(st, b);
            }
            Act::AtReset => {
                st.atmos.reset(self.ed.biome);
                st.save();
            }
            Act::AtResetAll => {
                st.atmos.reset_all();
                st.save();
            }
            Act::Done => {
                st.save();
                if self.scr == OptScr::Atmos {
                    st.atmos.view = None; // the sky goes back to the biome under the player, easing over 2 s
                }
                return Some(OptEv::Close); // from any page: back to the game menu / the title screen
            }
        }
        None
    }

    /// The editor shows (and the game draws everywhere) the look of `b`.
    fn show(&mut self, st: &mut Settings, b: Biome) {
        self.ed = AtmosEd { biome: b, list: false, ..self.ed };
        st.atmos.view = Some(b);
    }

    /// The pad's LB (-1) / RB (+1): the next section of the list, wrapping round. Not while a Controller button waits for its press (the
    /// bumper is the binding).
    pub fn step_tab(&mut self, d: i32, st: &mut Settings) {
        if self.scr != OptScr::List || self.wait_pad.is_some() {
            return;
        }
        st.save();
        (self.drag, self.wait, self.press) = (None, None, None);
        self.jump((self.page as i32 + d).rem_euclid(N_PAGES as i32) as usize, st);
    }

    /// `GuiControls.keyTyped`: the key that follows a tap on a key button becomes its binding.
    #[allow(dead_code)] // ponytail: nothing sends key events yet (touch only)
    pub fn key(&mut self, code: u32, st: &mut Settings) {
        if let Some(i) = self.wait.take() {
            st.keys[i] = code;
            st.save();
        }
    }

    /// The Controller buttons wait for a pad button: `Pad` swallows the next press into `Pad::captured`.
    pub fn pad_waiting(&self) -> bool {
        self.wait_pad.is_some()
    }

    /// The pad button that follows a tap on a Controller binding becomes it.
    pub fn pad_button(&mut self, b: Btn, st: &mut Settings) {
        if let Some(i) = self.wait_pad.take() {
            st.pad.binds[i] = b;
            st.save();
        }
    }

    /// `world`: over the dimmed game (the game menu's options) instead of the dirt of the title screen.
    pub fn draw(&self, v: &mut Vec<HudVertex>, w: f32, h: f32, st: &Settings, world: bool) {
        let (s, gw, gh) = scaled_gs(w, h, st.gui_scale);
        if self.scr == OptScr::Atmos {
            self.draw_atmos(v, s, gw, gh, st);
        } else {
            self.draw_list(v, s, gw, gh, st, world);
        }
    }

    /// The atmosphere editor: no dimming (the world is the preview), only the panel's own shade down the left.
    fn draw_atmos(&self, v: &mut Vec<HudVertex>, s: f32, gw: f32, gh: f32, st: &Settings) {
        grad(v, s, [0.0, 0.0, 220.0, gh], [0.0, 0.0, 0.0, 0.62], [0.0, 0.0, 0.0, 0.76]);
        for b in self.widgets(gw, gh, st) {
            let over = self.finger.is_some_and(|(x, y)| inside(b.r, x, y));
            let knob = match b.act {
                Act::AtKnob(j) => Some(atmos::TABS[self.ed.tab].1[j].pos(st.atmos.look(self.ed.biome))),
                _ => None,
            };
            let on = !matches!(b.act, Act::AtTab(i) if i == self.ed.tab && !self.ed.list) && !matches!(b.act, Act::AtPick(i) if atmos::ALL[i] == self.ed.biome);
            button(v, s, b.r, &b.label, on, over, knob);
            if let Act::AtPick(i) = b.act {
                if st.atmos.edited(atmos::ALL[i]) {
                    HudPipeline::push_quad(v, (b.r[0] + b.r[2] - 5.0) * s, (b.r[1] + 2.0) * s, 3.0 * s, 3.0 * s, rgb(0xFFD93B));
                }
            }
        }
        if !self.ed.list {
            // The tab's colour (its first three sliders) as it is authored, and its hex code.
            let (look, k0) = (st.atmos.look(self.ed.biome), atmos::TABS[self.ed.tab].1[0].i);
            let c: [f32; 3] = [look[k0], look[k0 + 1], look[k0 + 2]];
            HudPipeline::push_quad(v, 159.0 * s, 43.0 * s, 54.0 * s, 46.0 * s, rgb(0xA0A0A0));
            HudPipeline::push_quad(v, 160.0 * s, 44.0 * s, 52.0 * s, 44.0 * s, [c[0], c[1], c[2], 1.0]);
            let hex = format!("#{:02X}{:02X}{:02X}", (c[0] * 255.0).round() as u32, (c[1] * 255.0).round() as u32, (c[2] * 255.0).round() as u32);
            HudPipeline::push_text(v, (186.0 - tw(&hex) / 2.0) * s, 93.0 * s, s, &hex, rgb(0xE0E0E0));
            HudPipeline::push_text(v, 4.0 * s, 158.0 * s, s, "Noon colours; night and rain follow", rgb(0xA0A0A0));
        }
    }

    /// The options list: title, page list, scrolling list, description, Done.
    fn draw_list(&self, v: &mut Vec<HudVertex>, s: f32, gw: f32, gh: f32, st: &Settings, world: bool) {
        background(v, s, gw, gh, world);
        ui::title(v, s, gw, 0.0, "Options", true);
        let f = frame(gw, gh);
        let (items, fl, max) = self.extent(st, &f);
        let scroll = self.scroll.clamp(0.0, max);
        // a finger that is scrolling or dragging a slider lights nothing
        let calm = !matches!(self.press.map(|p| p.grab), Some(Grab::Scroll | Grab::Dead | Grab::Drag(_)));
        let over = |r: [f32; 4]| calm && self.finger.is_some_and(|(x, y)| inside(r, x, y));
        // the page list: the section in view is the dark, pressed-in one, in yellow
        for (i, p) in PAGES.iter().enumerate() {
            let r = side_entry(&f, i);
            let (cur, hot) = (i == self.page, over(r));
            button_c(v, s, r, p.0, if cur { 0xFFFF55 } else if hot { 0xFFFFA0 } else { 0xE0E0E0 }, if cur { 0 } else if hot { 2 } else { 1 }, None);
        }
        // the list: the dirt scrolls with it (`GuiSlot`), what falls over its edges is cut off
        dirt(v, s, f.list, scroll, 0x202020);
        let from = v.len();
        let (x, w, top) = (f.list[0] + 2.0, f.list[2] - 11.0, f.list[1] + 4.0 - scroll);
        for (it, &dy) in items.iter().zip(&fl.y) {
            let y = top + dy;
            if y + ROW_PITCH < f.list[1] || y > f.list[1] + f.list[3] {
                continue;
            }
            match it {
                Item::Page(i) => heading(v, s, [x, y, w, PAGE_H], PAGES[*i].1),
                Item::Group(g) => group(v, s, [x, y, w, GROUP_H], g),
                Item::Row(act, label) => {
                    let r = [x, y, w, ROW_H];
                    row(v, s, r, *act, label, st, f.ctl, over(r), self.sel == Some(*act));
                }
            }
        }
        ui::clip(v, from, f.list.map(|u| u * s));
        // the 4 unit fades `GuiSlot` lays over the ends of a list, then its bar and a frame
        let (clear, black) = ([0.0; 4], [0.0, 0.0, 0.0, 1.0]);
        grad(v, s, [f.list[0], f.list[1], f.list[2], 4.0], black, clear);
        grad(v, s, [f.list[0], f.list[1] + f.list[3] - 4.0, f.list[2], 4.0], clear, black);
        ui::scrollbar(v, s, f.list[0] + f.list[2] - 8.0, f.list[1], f.list[3], fl.total, scroll);
        ui::bevel(v, s, f.list);
        let hot = over(f.done);
        button_c(v, s, f.done, "Done", if hot { 0xFFFFA0 } else { 0xE0E0E0 }, if hot { 2 } else { 1 }, None);
        self.draw_tip(v, s, &f, &items);
    }

    /// The description of the chosen setting: its name, on a wide screen its value, and a sentence on what it does.
    fn draw_tip(&self, v: &mut Vec<HudVertex>, s: f32, f: &Frame, items: &[Item]) {
        ui::panel(v, s, f.tip, 0.55);
        let (x, mut y, w) = (f.tip[0] + 6.0, f.tip[1] + 5.0, f.tip[2] - 12.0);
        let text = |v: &mut Vec<HudVertex>, y: f32, t: &str, c: u32| HudPipeline::push_text(v, x * s, y * s, s, t, rgb(c));
        let sel = self.sel.and_then(|a| {
            items.iter().find_map(|it| match it {
                Item::Row(r, l) if *r == a => Some((a, l.as_str())),
                _ => None,
            })
        });
        let Some((act, label)) = sel else {
            for l in ui::wrap("Tap a setting to read what it does. Drag the list to scroll.", w).into_iter().take(if f.wide { 6 } else { 3 }) {
                text(v, y, &l, 0xA0A0A0);
                y += 10.0;
            }
            return;
        };
        let (name, val) = label.split_once(": ").unwrap_or((label, ""));
        text(v, y, &ui::fit(name, w), 0xFFFF55);
        y += 10.0;
        let mut lines = 2;
        if f.wide {
            text(v, y, &ui::fit(val, w), value_color(val));
            y += 14.0;
            lines = 8;
        }
        for l in ui::wrap(tip(act, name), w).into_iter().take(lines) {
            text(v, y, &l, 0xA0A0A0);
            y += 10.0;
        }
    }
}

/// What the pause menu needs to know about the game.
pub struct PauseInfo {
    /// The death screen: "You died!", the resume button respawns.
    pub dead: bool,
    /// The far terrain's pre-generation: (running, chunks), or none in a dimension without it.
    pub pregen: Option<(bool, u32)>,
    /// `Lod::debug`.
    pub debug: u8,
    pub flying: bool,
    /// `Settings::debug_screen`.
    pub info: bool,
}

/// What a pause-menu button asks of the game.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PauseEv {
    Resume,
    Quit,
    Pregen,
    Fly,
    LodDebug,
    Info,
    /// Open `stats_ui` (true: Achievements, false: Statistics).
    Stats(bool),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PAct {
    Resume,
    Quit,
    Options,
    Achievements,
    Stats,
    Pregen,
    Fly,
    Debug,
    Info,
    /// The sun, bottom right: the live atmosphere editor.
    Atmos,
    /// Opens the page with Pregen / Fly / Info / LoD debug; its Done comes back.
    Tools,
    ToolsDone,
}

/// `GuiIngameMenu` (and the death screen, `GuiGameOver`, which shares it).
pub struct Pause {
    opt: Option<OptionsUi>,
    /// The Tools page is open (instead of the game menu's own buttons).
    tools: bool,
    finger: Option<(f32, f32)>,
    born: Instant,
    /// The world was written for this opening (done on the second frame, see `step_frame`).
    pub saved: bool,
}

impl Default for Pause {
    fn default() -> Self {
        Self::new()
    }
}

impl Pause {
    pub fn new() -> Self {
        Self { opt: None, tools: false, finger: None, born: Instant::now(), saved: false }
    }

    /// (action, rectangle, text, enabled), GUI units.
    fn buttons(gw: f32, gh: f32, info: &PauseInfo) -> Vec<(PAct, [f32; 4], String, bool)> {
        let (hw, q) = ((gw / 2.0).floor(), (gh / 4.0).floor());
        let mut v = if info.dead {
            vec![(PAct::Resume, [hw - 100.0, q + 72.0, 200.0, 20.0], "Respawn".to_string(), true), (PAct::Quit, [hw - 100.0, q + 96.0, 200.0, 20.0], "Save and quit to title".to_string(), true)]
        } else {
            // `GuiIngameMenu.initGui`, `byte var1 = -16`.
            vec![
                (PAct::Quit, [hw - 100.0, q + 120.0 - 16.0, 200.0, 20.0], "Save and quit to title".to_string(), true),
                (PAct::Resume, [hw - 100.0, q + 24.0 - 16.0, 200.0, 20.0], "Back to game".to_string(), true),
                (PAct::Options, [hw - 100.0, q + 96.0 - 16.0, 200.0, 20.0], "Options...".to_string(), true),
                (PAct::Achievements, [hw - 100.0, q + 48.0 - 16.0, 98.0, 20.0], "Achievements".to_string(), true),
                (PAct::Stats, [hw + 2.0, q + 48.0 - 16.0, 98.0, 20.0], "Statistics".to_string(), true),
                // b1.7.3 leaves the row between Statistics and Options empty.
                (PAct::Tools, [hw - 100.0, q + 72.0 - 16.0, 200.0, 20.0], "Tools...".to_string(), true),
            ]
        };
        // Bottom right: the sun (not on the death screen).
        if !info.dead {
            v.push((PAct::Atmos, [gw - 24.0, gh - 24.0, 20.0, 20.0], String::new(), true));
        }
        v
    }

    /// The Tools page (GUI units): what the old game menu kept in its top-left corner, now in a tidy column.
    fn tool_buttons(gw: f32, gh: f32, info: &PauseInfo) -> Vec<(PAct, [f32; 4], String, bool)> {
        let (hw, y) = ((gw / 2.0).floor(), (gh / 4.0).floor() + 8.0);
        let pregen = match info.pregen {
            None => "Pregen: overworld only".to_string(),
            Some((true, n)) => format!("Pregen: {n} chunks (tap to stop)"),
            Some((false, n)) => format!("Pregen: off, {n} chunks"),
        };
        let debug = match info.debug {
            1 => "LoD dbg 1: step g1 y2 o4 r8",
            2 => "LoD dbg 2: top wht wall blu skirt pnk",
            3 => "LoD dbg 3: tiles red/blue",
            4 => "LoD dbg 4: LoD hidden",
            _ => "LoD debug: off",
        };
        vec![
            (PAct::Pregen, [hw - 120.0, y, 240.0, 20.0], pregen, true),
            (PAct::Fly, [hw - 120.0, y + 24.0, 118.0, 20.0], if info.flying { "Fly: on" } else { "Fly: off" }.to_string(), true),
            (PAct::Info, [hw + 2.0, y + 24.0, 118.0, 20.0], if info.info { "Info: on" } else { "Info: off" }.to_string(), true),
            (PAct::Debug, [hw - 120.0, y + 48.0, 240.0, 20.0], debug.to_string(), true),
            (PAct::ToolsDone, [hw - 100.0, y + 96.0, 200.0, 20.0], "Done".to_string(), true),
        ]
    }

    /// The buttons of the page that is showing.
    fn page(&self, gw: f32, gh: f32, info: &PauseInfo) -> Vec<(PAct, [f32; 4], String, bool)> {
        if self.tools { Self::tool_buttons(gw, gh, info) } else { Self::buttons(gw, gh, info) }
    }

    /// A finger event in px; a button acts when the finger is lifted over it.
    pub fn event(&mut self, ev: ScreenEv, w: f32, h: f32, st: &mut Settings, info: &PauseInfo) -> Option<PauseEv> {
        if let Some(o) = self.opt.as_mut() {
            if o.event(ev, w, h, st).is_some() {
                self.opt = None;
            }
            return None;
        }
        let (s, gw, gh) = scaled_gs(w, h, st.gui_scale);
        match ev {
            ScreenEv::Down(x, y) | ScreenEv::Move(x, y) => self.finger = Some((x / s, y / s)),
            ScreenEv::Up(x, y) => {
                self.finger = None;
                let (gx, gy) = (x / s, y / s);
                let (act, ..) = self.page(gw, gh, info).into_iter().find(|b| b.3 && inside(b.1, gx, gy))?;
                return match act {
                    PAct::Resume => Some(PauseEv::Resume),
                    PAct::Quit => Some(PauseEv::Quit),
                    PAct::Pregen => Some(PauseEv::Pregen),
                    PAct::Fly => Some(PauseEv::Fly),
                    PAct::Debug => Some(PauseEv::LodDebug),
                    PAct::Info => Some(PauseEv::Info),
                    PAct::Options => {
                        self.opt = Some(OptionsUi::new());
                        None
                    }
                    PAct::Atmos => {
                        self.opt = Some(OptionsUi::atmos(st));
                        None
                    }
                    PAct::Tools | PAct::ToolsDone => {
                        self.tools = act == PAct::Tools;
                        None
                    }
                    PAct::Achievements | PAct::Stats => Some(PauseEv::Stats(act == PAct::Achievements)),
                };
            }
        }
        None
    }

    pub fn top(&self) -> bool {
        self.opt.is_none()
    }

    /// The enabled buttons of the top page in px (none while a sub-screen is open: the d-pad nudges its cursor), for the pad's jumps.
    pub fn targets(&self, w: f32, h: f32, st: &Settings, info: &PauseInfo) -> Vec<[f32; 4]> {
        if self.opt.is_some() {
            return Vec::new();
        }
        let (s, gw, gh) = scaled_gs(w, h, st.gui_scale);
        self.page(gw, gh, info).into_iter().filter(|b| b.3).map(|b| b.1.map(|v| v * s)).collect()
    }

    pub fn pad_waiting(&self) -> bool {
        self.opt.as_ref().is_some_and(OptionsUi::pad_waiting)
    }

    /// An options page is open: the pad's bumpers step its tabs (`step_tab`).
    pub fn in_options(&self) -> bool {
        self.opt.as_ref().is_some_and(OptionsUi::in_list)
    }

    pub fn step_tab(&mut self, d: i32, st: &mut Settings) {
        if let Some(o) = self.opt.as_mut() {
            o.step_tab(d, st);
        }
    }

    pub fn pad_button(&mut self, b: Btn, st: &mut Settings) {
        if let Some(o) = self.opt.as_mut() {
            o.pad_button(b, st);
        }
    }

    pub fn draw(&self, v: &mut Vec<HudVertex>, w: f32, h: f32, st: &Settings, info: &PauseInfo) {
        if let Some(o) = &self.opt {
            o.draw(v, w, h, st, true);
            return;
        }
        let (s, gw, gh) = scaled_gs(w, h, st.gui_scale);
        let hw = (gw / 2.0).floor();
        if info.dead {
            // `GuiGameOver`: 0x60500000 to 0xA0803030.
            grad(v, s, [0.0, 0.0, gw, gh], [0x50 as f32 / 255.0, 0.0, 0.0, 0x60 as f32 / 255.0], [0x80 as f32 / 255.0, 0x30 as f32 / 255.0, 0x30 as f32 / 255.0, 0xA0 as f32 / 255.0]);
            HudPipeline::push_text(v, (hw - tw("You died!")) * s, 30.0 * s, 2.0 * s, "You died!", rgb(0xFFFFFF));
        } else {
            background(v, s, gw, gh, true);
            ui::title(v, s, gw, 0.0, if self.tools { "Tools" } else { "Game menu" }, true);
            // a panel behind the stack of buttons (the sun, bottom right, is not part of it)
            let (x0, y0, x1, y1) = self.page(gw, gh, info).iter().filter(|b| b.0 != PAct::Atmos).fold((f32::MAX, f32::MAX, 0.0f32, 0.0f32), |a, b| (a.0.min(b.1[0]), a.1.min(b.1[1]), a.2.max(b.1[0] + b.1[2]), a.3.max(b.1[1] + b.1[3])));
            ui::panel(v, s, [x0 - 12.0, y0 - 12.0, x1 - x0 + 24.0, y1 - y0 + 24.0], 0.45);
            // "Saving level.." for the first 20 ticks, its grey pulsing every 10.
            let t = self.born.elapsed().as_secs_f32();
            if t < 1.0 && !self.tools {
                let g = ((((t * 20.0) % 10.0) / 10.0 * std::f32::consts::TAU).sin() * 0.2 + 0.8) * 255.0;
                let g = g as u32;
                HudPipeline::push_text(v, 8.0 * s, (gh - 16.0) * s, s, "Saving level..", rgb(g << 16 | g << 8 | g));
            }
        }
        for (act, r, label, on) in self.page(gw, gh, info) {
            let over = self.finger.is_some_and(|(x, y)| inside(r, x, y));
            let lit = (act == PAct::Pregen && info.pregen.is_some_and(|p| p.0)) || (act == PAct::Fly && info.flying) || (act == PAct::Debug && info.debug != 0) || (act == PAct::Info && info.info);
            button(v, s, r, &label, on, over || lit, None);
            if act == PAct::Atmos {
                sun(v, s, r);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SZ: (f32, f32) = (2400.0, 1080.0);

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mcrs-options-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn px(r: [f32; 4], st: &Settings) -> (f32, f32) {
        let (s, ..) = scaled_gs(SZ.0, SZ.1, st.gui_scale);
        ((r[0] + r[2] / 2.0) * s, (r[1] + r[3] / 2.0) * s)
    }

    /// Scrolls `o` so that the setting whose text is `label` is at the top of the list (nothing for a label that is no setting).
    fn show(o: &mut OptionsUi, st: &Settings, label: &str) {
        let items = o.items(st);
        if let Some(i) = items.iter().position(|it| matches!(it, Item::Row(_, l) if l == label)) {
            o.scroll = flow(&items).y[i];
        }
    }

    /// A tap on the first control of `o` whose text is `label` (a setting is scrolled into view first).
    fn tap(o: &mut OptionsUi, st: &mut Settings, label: &str) -> Option<OptEv> {
        let (_, gw, gh) = scaled_gs(SZ.0, SZ.1, st.gui_scale);
        show(o, st, label);
        let r = o.widgets(gw, gh, st).into_iter().find(|b| b.label == label).unwrap_or_else(|| panic!("no {label}")).r;
        let (x, y) = px(r, st);
        o.event(ScreenEv::Down(x, y), SZ.0, SZ.1, st);
        o.event(ScreenEv::Up(x, y), SZ.0, SZ.1, st)
    }

    /// The control of the slider row starting with `label`, in px: (x, y, width, px per unit).
    fn track_px(o: &mut OptionsUi, st: &Settings, label: &str) -> (f32, f32, f32, f32) {
        let (s, gw, gh) = scaled_gs(SZ.0, SZ.1, st.gui_scale);
        let items = o.items(st);
        if let Some(i) = items.iter().position(|it| matches!(it, Item::Row(_, l) if l.starts_with(label))) {
            o.scroll = flow(&items).y[i];
        }
        let r = o.widgets(gw, gh, st).into_iter().find(|b| b.label.starts_with(label)).unwrap_or_else(|| panic!("no {label}")).r;
        let t = track(r, frame(gw, gh).ctl);
        (t[0] * s, t[1] * s, t[2] * s, s)
    }

    #[test]
    fn texts_and_cycles_follow_game_settings() {
        let mut st = Settings::default();
        assert_eq!(st.text(Opt::Sensitivity), "Sensitivity: 100%");
        assert_eq!(st.text(Opt::Music), "Music: 100%");
        assert_eq!(st.text(Opt::RenderDistance), "Render Distance: Short");
        assert_eq!(st.text(Opt::FramerateLimit), "Performance: Balanced");
        st.set_float(Opt::Sensitivity, 1.0);
        st.set_float(Opt::Music, 0.0);
        assert_eq!((st.text(Opt::Sensitivity).as_str(), st.text(Opt::Music).as_str()), ("Sensitivity: HYPERSPEED!!!", "Music: OFF"));
        (st.render_distance, st.difficulty, st.limit_framerate, st.gui_scale) = (3, 3, 2, 3);
        for o in [Opt::RenderDistance, Opt::Difficulty, Opt::FramerateLimit, Opt::GuiScale] {
            st.cycle(o);
        }
        assert_eq!((st.render_distance, st.difficulty, st.limit_framerate, st.gui_scale), (0, 0, 0, 0));
        assert_eq!(Settings::default().chunks(), 4);
        assert!((Settings::default().look_scale() - 1.0).abs() < 1e-5);
        assert_eq!(key_name(17) + &key_name(57) + &key_name(33), "WSPACEF");
    }

    #[test]
    fn options_txt_round_trips() {
        let root = tmp("file");
        let mut st = Settings::load(&root);
        (st.sensitivity, st.invert_mouse, st.render_distance, st.lod, st.keys[4]) = (0.25, true, 0, false, 50);
        assert!(!st.gpu_error_trace, "a debug switch: off until options.txt says so");
        assert!(st.dynamic_res, "on until options.txt says otherwise");
        assert!(!st.debug_screen, "off until the Info button or options.txt says so");
        assert!(st.animal_ai && st.biome_rules, "on until options.txt says otherwise");
        assert_eq!(st.frame_latency, 2, "the wgpu default until options.txt says otherwise");
        st.frame_latency = 1;
        assert!(!st.pack_cloth && !st.book_discovery, "the satchel with every recipe shown, until options.txt says otherwise");
        (st.gpu_error_trace, st.dynamic_res, st.debug_screen, st.pack_cloth, st.book_discovery, st.biome_rules) = (true, false, true, true, true, false);
        st.save();
        let back = Settings::load(&root);
        assert_eq!((back.sensitivity, back.invert_mouse, back.render_distance, back.lod, back.keys[4]), (0.25, true, 0, false, 50));
        assert!(back.gpu_error_trace && !back.dynamic_res && back.debug_screen && back.pack_cloth && back.book_discovery && !back.biome_rules);
        assert_eq!(back.msaa, 0, "Auto until options.txt says otherwise");
        assert_eq!(back.frame_latency, 1);
        assert!(std::fs::read_to_string(root.join("options.txt")).unwrap().contains("key_key.jump:50"));
    }

    /// The MSAA button cycles Auto -> Off -> 2x -> 4x -> Auto, reads back from options.txt, and a bad value falls back to Auto.
    #[test]
    fn msaa_option_cycles_and_round_trips() {
        let root = tmp("msaa");
        let mut st = Settings::load(&root);
        let seen: Vec<u8> = (0..5).map(|_| { st.cycle(Opt::Msaa); st.msaa }).collect();
        assert_eq!(seen, [1, 2, 4, 0, 1]);
        st.msaa = 2;
        st.save();
        assert_eq!(Settings::load(&root).msaa, 2);
        std::fs::write(root.join("options.txt"), "msaa:3\n").unwrap();
        assert_eq!(Settings::load(&root).msaa, 0);
        st.msaa = 4;
        assert!(st.text(Opt::Msaa).starts_with("MSAA: 4x"));
    }

    /// The FXAA button cycles Auto -> On -> Off -> Auto, reads back from options.txt, and a bad value falls back to Auto.
    #[test]
    fn fxaa_option_cycles_and_round_trips() {
        let root = tmp("fxaa");
        let mut st = Settings::load(&root);
        let seen: Vec<u8> = (0..4).map(|_| { st.cycle(Opt::Fxaa); st.fxaa }).collect();
        assert_eq!(seen, [1, 2, 0, 1]);
        st.fxaa = 1;
        st.save();
        assert_eq!(Settings::load(&root).fxaa, 1);
        std::fs::write(root.join("options.txt"), "fxaa:3\n").unwrap();
        assert_eq!(Settings::load(&root).fxaa, 0);
        st.fxaa = 2;
        assert!(st.text(Opt::Fxaa).starts_with("FXAA: Off"));
    }

    /// The Vibrance dial and the switches share the Effects tab (conservative default, 0 = off), and everything reads back from options.txt.
    #[test]
    fn vibrance_and_effects_are_reachable_and_saved() {
        let root = tmp("look");
        let mut st = Settings::load(&root);
        assert_eq!((st.text(Opt::Vibrance).as_str(), st.look.motion), ("Vibrance: 30%", 1));
        let mut o = OptionsUi::new();
        tap(&mut o, &mut st, "Effects");
        assert_eq!(o.page, 3);
        tap(&mut o, &mut st, "Motion: Low");
        tap(&mut o, &mut st, "Glow: ON");
        tap(&mut o, &mut st, "Haze: ON");
        assert_eq!((st.look.motion, st.look.glow, st.look.haze), (2, false, false));
        tap(&mut o, &mut st, "Motion: On");
        assert_eq!(st.look.motion, 0, "off, low, on, off again");
        tap(&mut o, &mut st, "Graphics");
        assert_eq!(o.page, 1, "a page is one tap from any other");
        st.set_float(Opt::Vibrance, 0.0);
        assert_eq!(st.text(Opt::Vibrance), "Vibrance: OFF");
        st.save();
        let back = Settings::load(&root);
        assert_eq!(back.look, st.look);
        std::fs::write(root.join("options.txt"), "look_vibrance:5\nlook_motion:7\n").unwrap();
        let bad = Settings::load(&root);
        assert!(bad.look.vibrance == 1.0 && bad.look.motion == LookCfg::default().motion, "out of range: clamped / ignored");
    }

    /// The Post button cycles Auto -> Full -> Fast -> Auto, reads back from options.txt, and a bad value falls back to Auto.
    #[test]
    fn post_option_cycles_and_round_trips() {
        let root = tmp("post");
        let mut st = Settings::load(&root);
        let seen: Vec<u8> = (0..4).map(|_| { st.cycle(Opt::Post); st.post }).collect();
        assert_eq!(seen, [1, 2, 0, 1]);
        st.post = 2;
        st.save();
        assert_eq!(Settings::load(&root).post, 2);
        std::fs::write(root.join("options.txt"), "post:3\n").unwrap();
        assert_eq!(Settings::load(&root).post, 0);
    }

    /// General -> Graphics (LoD, Render Distance) -> Performance (Dynamic Res) -> Controls (wait for a key, invert) -> Done closes.
    #[test]
    fn options_screens_flow() {
        let mut st = Settings::default();
        let mut o = OptionsUi::new();
        assert_eq!((o.scr, o.page), (OptScr::List, 0));
        assert!(tap(&mut o, &mut st, "Graphics").is_none());
        assert_eq!(o.page, 1);
        assert!(tap(&mut o, &mut st, "LoD: ON").is_none() && !st.lod);
        tap(&mut o, &mut st, "Render Distance: Short");
        assert_eq!(st.render_distance, 3);
        tap(&mut o, &mut st, "Performance");
        assert_eq!(o.page, 2);
        tap(&mut o, &mut st, "Dynamic Res: ON");
        assert!(!st.dynamic_res && st.text(Opt::DynamicRes) == "Dynamic Res: OFF");
        tap(&mut o, &mut st, "Controls");
        assert_eq!(o.page, 4);
        tap(&mut o, &mut st, "Back: S");
        assert!(tap(&mut o, &mut st, "Back: > S <").is_none(), "tapping it again keeps waiting");
        o.key(44, &mut st);
        assert_eq!(st.keys[2], 44);
        tap(&mut o, &mut st, "Invert Mouse: OFF");
        assert!(st.invert_mouse);
        assert_eq!(tap(&mut o, &mut st, "Done"), Some(OptEv::Close));
        let mut v = Vec::new();
        o.draw(&mut v, SZ.0, SZ.1, &st, true);
        assert!(!v.is_empty());
    }

    /// A slider: a tap on its track puts it there, a finger moving along it follows the finger and ends at the track's ends, a tap on its
    /// name only selects it.
    #[test]
    fn slider_drags() {
        let mut st = Settings::default();
        let mut o = OptionsUi::new();
        let (x, y, w, s) = track_px(&mut o, &st, "Music");
        let y = y + 4.0 * s;
        o.event(ScreenEv::Down(x + 4.0 * s, y), SZ.0, SZ.1, &mut st);
        assert_eq!(st.music, 1.0, "going down alone changes nothing");
        o.event(ScreenEv::Move(x + 4.0 * s + (w - 8.0 * s) / 2.0, y), SZ.0, SZ.1, &mut st);
        assert!((st.music - 0.5).abs() < 0.01);
        o.event(ScreenEv::Move(x + w + 50.0 * s, y), SZ.0, SZ.1, &mut st);
        assert_eq!(st.music, 1.0);
        o.event(ScreenEv::Up(x + w, y), SZ.0, SZ.1, &mut st);
        assert!(o.press.is_none());
        // a tap at the left end
        o.event(ScreenEv::Down(x + 4.0 * s, y), SZ.0, SZ.1, &mut st);
        o.event(ScreenEv::Up(x + 4.0 * s, y), SZ.0, SZ.1, &mut st);
        assert_eq!(st.music, 0.0);
        // a tap on the name selects the row and moves nothing
        let (_, gw, gh) = scaled_gs(SZ.0, SZ.1, 0);
        let r = o.widgets(gw, gh, &st).into_iter().find(|b| b.label.starts_with("Music")).unwrap().r;
        let (nx, ny) = ((r[0] + 8.0) * s, (r[1] + 10.0) * s);
        o.event(ScreenEv::Down(nx, ny), SZ.0, SZ.1, &mut st);
        o.event(ScreenEv::Up(nx, ny), SZ.0, SZ.1, &mut st);
        assert_eq!((st.music, o.sel), (0.0, Some(Act::Opt(Opt::Music))));
    }

    /// Dragging up or down scrolls the list, the page list follows the section in view, a drag that starts on a slider and goes up still
    /// scrolls, one that goes along it does not, and a drag never acts as a tap.
    #[test]
    fn dragging_scrolls_the_list() {
        let mut st = Settings::default();
        let mut o = OptionsUi::new();
        let (s, gw, gh) = scaled_gs(SZ.0, SZ.1, 0);
        let f = frame(gw, gh);
        let (x, y) = ((f.list[0] + 20.0) * s, (f.list[1] + f.list[3] - 20.0) * s);
        // up by 100 units from a plain row: the list moves 100, nothing flips
        let before = st.clone();
        o.event(ScreenEv::Down(x, y), SZ.0, SZ.1, &mut st);
        o.event(ScreenEv::Move(x, y - 100.0 * s), SZ.0, SZ.1, &mut st);
        o.event(ScreenEv::Up(x, y - 100.0 * s), SZ.0, SZ.1, &mut st);
        assert!((o.scroll - 100.0).abs() < 0.01, "{}", o.scroll);
        assert_eq!(st, before, "a drag is not a tap");
        // a long drag stops at the end, and the last section is lit there
        o.event(ScreenEv::Down(x, y), SZ.0, SZ.1, &mut st);
        o.event(ScreenEv::Move(x, y - 5000.0 * s), SZ.0, SZ.1, &mut st);
        o.event(ScreenEv::Up(x, y - 5000.0 * s), SZ.0, SZ.1, &mut st);
        let (_, fl, max) = o.extent(&st, &f);
        assert!((o.scroll - max).abs() < 0.01 && max > 0.0 && fl.total > f.list[3]);
        assert_eq!(o.page, N_PAGES - 1);
        // and back to the top
        o.event(ScreenEv::Down(x, y - 200.0 * s), SZ.0, SZ.1, &mut st);
        o.event(ScreenEv::Move(x, y + 5000.0 * s), SZ.0, SZ.1, &mut st);
        o.event(ScreenEv::Up(x, y + 5000.0 * s), SZ.0, SZ.1, &mut st);
        assert_eq!((o.scroll, o.page), (0.0, 0));
        // from a slider's track: along it moves the slider, up scrolls
        let (tx, ty, tw_, _) = track_px(&mut o, &st, "Music");
        let (tx, ty) = (tx + tw_ / 2.0, ty + 4.0 * s);
        let at = o.scroll;
        o.event(ScreenEv::Down(tx, ty), SZ.0, SZ.1, &mut st);
        o.event(ScreenEv::Move(tx, ty - 30.0 * s), SZ.0, SZ.1, &mut st);
        o.event(ScreenEv::Up(tx, ty - 30.0 * s), SZ.0, SZ.1, &mut st);
        assert!(o.scroll > at && st.music == 1.0, "up from a slider scrolls and leaves it");
    }

    /// A tap on a page jumps to its section and lights it even where the list cannot scroll that far; scrolling lights the section in view.
    #[test]
    fn pages_jump_and_light() {
        let mut st = Settings::default();
        let mut o = OptionsUi::new();
        let (_, gw, gh) = scaled_gs(SZ.0, SZ.1, 0);
        let f = frame(gw, gh);
        let (items, fl, max) = o.extent(&st, &f);
        for (i, p) in PAGES.iter().enumerate() {
            tap(&mut o, &mut st, p.0);
            assert_eq!(o.page, i);
            assert!((o.scroll.clamp(0.0, max) - fl.starts[i].min(max)).abs() < 0.01, "{}", p.0);
        }
        assert!(fl.starts.windows(2).all(|w| w[0] < w[1]), "sections follow each other");
        assert_eq!(items.iter().filter(|it| matches!(it, Item::Page(_))).count(), N_PAGES);
        for i in 0..N_PAGES {
            assert_eq!(which(&fl.starts, fl.starts[i], f.list[3]), i, "a section at the top of the view is the lit one");
        }
        assert_eq!(which(&fl.starts, 0.0, f.list[3]), 0);
        assert_eq!(which(&fl.starts, max, f.list[3]), N_PAGES - 1);
    }

    /// Controller buttons and tuning: rebind a button from the pad, drag a knob, flip a switch, and it all reaches options.txt.
    #[test]
    fn controller_flow() {
        let root = tmp("pad");
        let mut st = Settings::load(&root);
        let mut o = OptionsUi::new();
        tap(&mut o, &mut st, "Pad buttons");
        assert_eq!(o.page, 5);
        tap(&mut o, &mut st, "Jump: A");
        assert!(o.pad_waiting());
        o.pad_button(Btn::X, &mut st);
        assert_eq!((st.pad.binds[0], o.pad_waiting()), (Btn::X, false));
        tap(&mut o, &mut st, "Pad tuning");
        assert_eq!(o.page, 6);
        tap(&mut o, &mut st, "Invert Y: OFF");
        assert!(st.pad.invert_y);
        let (x, y, _, s) = track_px(&mut o, &st, "Look X");
        let y = y + 4.0 * s;
        o.event(ScreenEv::Down(x + 4.0 * s, y), SZ.0, SZ.1, &mut st);
        o.event(ScreenEv::Up(x + 4.0 * s, y), SZ.0, SZ.1, &mut st);
        assert_eq!(st.pad.look_h, 0.0);
        let back = Settings::load(&root);
        assert_eq!((back.pad.binds[0], back.pad.invert_y, back.pad.look_h), (Btn::X, true, 0.0));
        let mut v = Vec::new();
        o.draw(&mut v, SZ.0, SZ.1, &st, true);
        assert!(!v.is_empty());
    }

    /// The Dig Delay slider (Controls): 100 ms .. `HOLD_SECS`, default `SWING_SECS`, kept in options.txt.
    #[test]
    fn dig_delay_slider() {
        let root = tmp("dig");
        let mut st = Settings::load(&root);
        assert!((st.dig_secs() - SWING_SECS).abs() < 1e-4);
        let mut o = OptionsUi::new();
        let (x, y, w, s) = track_px(&mut o, &st, "Dig Delay");
        let (px_, py_) = (x + w - 2.0 * s, y + 4.0 * s);
        o.event(ScreenEv::Down(px_, py_), SZ.0, SZ.1, &mut st);
        o.event(ScreenEv::Up(px_, py_), SZ.0, SZ.1, &mut st);
        assert_eq!((st.dig_delay, st.dig_secs()), (1.0, HOLD_SECS));
        assert_eq!(Settings::load(&root).dig_delay, 1.0);
    }

    /// At the smallest GUI (320 x 240 units) and a phone's: the page list, Done, the description and the list lie inside the screen
    /// without touching each other, every row is inside the list's width, and every setting of the old screens is a row.
    #[test]
    fn list_fits_and_keeps_every_setting() {
        let st = Settings::default();
        let o = OptionsUi::new();
        let mut labels = Vec::new();
        for (w, h) in [(320.0, 240.0), SZ] {
            let (_, gw, gh) = scaled_gs(w, h, 0);
            let f = frame(gw, gh);
            let boxes = [("side", [f.side[0], f.side[1], f.side[2], 7.0 * 20.0]), ("list", f.list), ("tip", f.tip), ("done", f.done)];
            for (i, (n, a)) in boxes.iter().enumerate() {
                assert!(a[0] >= 0.0 && a[0] + a[2] <= gw && a[1] >= 0.0 && a[1] + a[3] <= gh, "{n} {a:?}: off screen at {gw}x{gh}");
                for (m, b) in &boxes[i + 1..] {
                    let apart = a[0] + a[2] <= b[0] || b[0] + b[2] <= a[0] || a[1] + a[3] <= b[1] || b[1] + b[3] <= a[1];
                    assert!(apart, "{n} overlaps {m} at {gw}x{gh}");
                }
            }
            assert!(f.side[3] >= 7.0 * 20.0, "all seven pages fit the page list");
            let (items, fl, _) = o.extent(&st, &f);
            for (it, &y) in items.iter().zip(&fl.y) {
                if let Item::Row(_, l) = it {
                    let name = l.split_once(": ").map_or(l.as_str(), |p| p.0);
                    assert!(tw(name) <= f.list[2] - 11.0 - f.ctl - 14.0, "{name} does not fit beside its control at {gw}");
                    assert!(y >= 0.0 && y + ROW_H <= fl.total);
                    labels.push(l.clone());
                }
            }
        }
        for want in [
            "Music", "Sound", "Difficulty", "GUI Scale", "View Bobbing", "Graphics: ", "Render Distance", "LoD", "Smooth Lighting", "Advanced OpenGL", "3D Anaglyph",
            "Performance", "Dynamic Res", "Sustained Perf", "MSAA", "FXAA", "Post", "Renderer", "Vibrance", "Warm Light", "Water Look", "Glow", "Motion", "Haze", "Sensitivity", "Invert Mouse",
            "Dig Delay", "Jump: ", "Look X", "Invert Y", "Forward", "Toggle Fog",
        ] {
            assert!(labels.iter().any(|l| l.starts_with(want)), "{want} is not in the list");
        }
        // every control is reachable after scrolling: a row's rectangle is inside the list whenever it is the top row
        let (_, gw, gh) = scaled_gs(320.0, 240.0, 0);
        assert!(o.widgets(gw, gh, &st).iter().filter(|b| matches!(b.act, Act::Tab(_))).count() == N_PAGES);
    }

    /// Every sentence of the description pane fits the strip of the narrow screen (3 lines) and the pane of a wide one.
    #[test]
    fn tips_fit_their_panes() {
        let (_, gw, gh) = scaled_gs(320.0, 240.0, 0);
        let narrow = frame(gw, gh);
        let wide = frame(2400.0 / 4.0, 1080.0 / 4.0);
        for (name, text) in TIPS {
            assert!(ui::wrap(text, narrow.tip[2] - 12.0).len() <= 2, "{name}: more than 2 lines in the strip");
            assert!(ui::wrap(text, wide.tip[2] - 12.0).len() <= 8, "{name}: too long for the pane");
            assert!(tw(name) <= narrow.tip[2] - 12.0);
        }
        // every named setting of the list that is not dim has a sentence
        let st = Settings::default();
        let o = OptionsUi::new();
        for it in o.items(&st) {
            if let Item::Row(a, l) = it {
                let name = l.split_once(": ").map_or(l.as_str(), |p| p.0);
                assert!(!tip(a, name).is_empty(), "{name} has no description");
            }
        }
    }

    /// LB / RB step the sections, wrapping round; not on the editor, and not while a Controller button waits for its press.
    #[test]
    fn bumpers_step_the_pages() {
        let mut st = Settings::default();
        let mut o = OptionsUi::new();
        o.step_tab(1, &mut st);
        assert_eq!(o.page, 1);
        o.step_tab(-1, &mut st);
        o.step_tab(-1, &mut st);
        assert_eq!(o.page, N_PAGES - 1, "the first page wraps back to the last");
        o.step_tab(1, &mut st);
        assert_eq!(o.page, 0, "and forward again");
        tap(&mut o, &mut st, "Pad buttons");
        tap(&mut o, &mut st, "Jump: A");
        o.step_tab(1, &mut st);
        assert_eq!(o.page, 5, "the bumper is the binding while it waits");
        o.pad_button(Btn::Rb, &mut st);
        o.step_tab(1, &mut st);
        assert_eq!(o.page, 6);
        let mut e = OptionsUi::atmos(&mut st);
        e.step_tab(1, &mut st);
        assert_eq!(e.scr, OptScr::Atmos);
        assert_eq!(e.page, 0);
    }

    /// Both game menu pages fit at the smallest GUI and a phone's, with no two buttons on one another.
    #[test]
    fn game_menu_pages_fit() {
        let info = PauseInfo { dead: false, pregen: Some((true, 120)), debug: 2, flying: true, info: true };
        for (w, h) in [(320.0, 240.0), SZ] {
            let (_, gw, gh) = scaled_gs(w, h, 0);
            for tools in [false, true] {
                let p = Pause { tools, ..Pause::new() };
                let bs = p.page(gw, gh, &info);
                for (i, a) in bs.iter().enumerate() {
                    assert!(a.1[0] >= 0.0 && a.1[0] + a.1[2] <= gw && a.1[1] >= 0.0 && a.1[1] + a.1[3] <= gh, "{}: off screen", a.2);
                    for b in &bs[i + 1..] {
                        let apart = a.1[0] + a.1[2] <= b.1[0] || b.1[0] + b.1[2] <= a.1[0] || a.1[1] + a.1[3] <= b.1[1] || b.1[1] + b.1[3] <= a.1[1];
                        assert!(apart, "{} overlaps {}", a.2, b.2);
                    }
                }
            }
        }
    }

    /// The game menu's sun (bottom right, not on the death screen) > the editor: the biome under the player, a slider edits it live, the
    /// edit reaches options.txt, `<` and the list change the biome, Done goes back to the game menu and the preview ends.
    #[test]
    fn atmosphere_editor_flow() {
        let root = tmp("atmos");
        let mut st = Settings::load(&root);
        st.atmos.here = Biome::Desert;
        let (s, gw, gh) = scaled_gs(SZ.0, SZ.1, 0);
        let info = PauseInfo { dead: false, pregen: None, debug: 0, flying: false, info: false };
        let sun = Pause::buttons(gw, gh, &info).into_iter().find(|b| b.0 == PAct::Atmos).unwrap().1;
        assert!(sun[0] + sun[2] >= gw - 4.0 && sun[1] + sun[3] >= gh - 4.0, "bottom right: {sun:?}");
        assert!(Pause::buttons(gw, gh, &PauseInfo { dead: true, ..info }).iter().all(|b| b.0 != PAct::Atmos), "none on the death screen");
        let mut p = Pause::new();
        let (x, y) = px(sun, &st);
        p.event(ScreenEv::Up(x, y), SZ.0, SZ.1, &mut st, &info);
        assert_eq!(st.atmos.view, Some(Biome::Desert));
        let mut o = p.opt.take().expect("the editor is open");
        assert_eq!(o.scr, OptScr::Atmos);
        tap(&mut o, &mut st, "Fog");
        // the red slider all the way left
        let r = o.widgets(gw, gh, &st).into_iter().find(|b| b.label.starts_with("Red")).unwrap().r;
        let y = (r[1] + 10.0) * s;
        o.event(ScreenEv::Down((r[0] + 4.0) * s, y), SZ.0, SZ.1, &mut st);
        o.event(ScreenEv::Up((r[0] + 4.0) * s, y), SZ.0, SZ.1, &mut st);
        assert_eq!(st.atmos.look(Biome::Desert)[3], 0.0);
        assert!(st.atmos.edited(Biome::Desert) && !st.atmos.edited(Biome::Plains));
        assert_eq!(Settings::load(&root).atmos.look(Biome::Desert)[3], 0.0, "saved when the slider is let go");
        // the previous biome, then one from the list
        tap(&mut o, &mut st, "<");
        assert_eq!(st.atmos.view, Some(Biome::Taiga));
        tap(&mut o, &mut st, "Taiga"); // the biome button opens the list
        tap(&mut o, &mut st, "Swamp");
        assert_eq!((st.atmos.view, o.ed.list), (Some(Biome::Swamp), false));
        // Reset all brings the table back, Done ends the preview and closes the editor
        tap(&mut o, &mut st, "Reset all");
        assert!(!st.atmos.edited(Biome::Desert));
        let mut v = Vec::new();
        o.draw(&mut v, SZ.0, SZ.1, &st, true);
        assert!(!v.is_empty());
        assert_eq!(tap(&mut o, &mut st, "Done"), Some(OptEv::Close));
        assert_eq!(st.atmos.view, None);
    }

    #[test]
    fn pause_menu_buttons() {
        let mut st = Settings::default();
        let mut p = Pause::new();
        let info = PauseInfo { dead: false, pregen: Some((false, 3)), debug: 0, flying: false, info: false };
        let (_, gw, gh) = scaled_gs(SZ.0, SZ.1, 0);
        let press = |p: &mut Pause, st: &mut Settings, label: &str| {
            let b = p.page(gw, gh, &info).into_iter().find(|b| b.2 == label).unwrap_or_else(|| panic!("no {label}"));
            let (x, y) = px(b.1, st);
            p.event(ScreenEv::Down(x, y), SZ.0, SZ.1, st, &info);
            p.event(ScreenEv::Up(x, y), SZ.0, SZ.1, st, &info)
        };
        assert_eq!(press(&mut p, &mut st, "Back to game"), Some(PauseEv::Resume));
        assert_eq!(press(&mut p, &mut st, "Save and quit to title"), Some(PauseEv::Quit));
        assert_eq!(press(&mut p, &mut st, "Tools..."), None);
        assert!(p.tools && p.page(gw, gh, &info).iter().all(|b| b.2 != "Back to game"), "the Tools page replaces the game menu's buttons");
        assert_eq!(press(&mut p, &mut st, "Pregen: off, 3 chunks"), Some(PauseEv::Pregen));
        assert_eq!(press(&mut p, &mut st, "Fly: off"), Some(PauseEv::Fly));
        assert_eq!(press(&mut p, &mut st, "LoD debug: off"), Some(PauseEv::LodDebug));
        assert_eq!(press(&mut p, &mut st, "Info: off"), Some(PauseEv::Info));
        let mut v = Vec::new();
        p.draw(&mut v, SZ.0, SZ.1, &st, &info);
        assert_eq!(press(&mut p, &mut st, "Done"), None);
        assert!(!p.tools, "Done goes back to the game menu");
        assert_eq!(press(&mut p, &mut st, "Achievements"), None, "greyed out");
        assert_eq!(press(&mut p, &mut st, "Options..."), None);
        assert!(p.opt.is_some());
        let (_, gw2, gh2) = scaled_gs(SZ.0, SZ.1, 0);
        let done = p.opt.as_ref().unwrap().widgets(gw2, gh2, &st).into_iter().find(|b| b.label == "Done").unwrap().r;
        let (x, y) = px(done, &st);
        p.event(ScreenEv::Up(x, y), SZ.0, SZ.1, &mut st, &info);
        assert!(p.opt.is_none(), "Done goes back to the game menu");
        let mut v = Vec::new();
        p.draw(&mut v, SZ.0, SZ.1, &st, &info);
        p.draw(&mut v, SZ.0, SZ.1, &st, &PauseInfo { dead: true, ..info });
        assert!(!v.is_empty());
    }
}
