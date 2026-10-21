//! The options and the pause menu, ported from b1.7.3: `GameSettings` (+ `EnumOptions`, `options.txt`), `GuiOptions`, `GuiVideoSettings`,
//! `GuiControls`, `GuiSlider`, `GuiSmallButton` and `GuiIngameMenu`. Laid out in GUI units like `menu.rs` (`ScaledResolution`, now with the
//! GUI Scale option) and drawn with the HUD pipeline; the strings are `lang/en_US.lang`'s.
//!
//! Added on top of b1.7.3 (it has none of them): the `LoD: ON/OFF` button (the free slot of `GuiOptions`' grid), and the pause menu's
//! `Pregen` / `Fly` / `LoD debug` buttons (top-left, kept from the old pause menu).
//! UNVERIFIED: the default render distance is Short (4 chunks, what mc-rs always drew), not Far as in `GameSettings`.
//! Stored and saved but with no effect yet: Music and Sound (no audio, M9), 3D Anaglyph, Advanced OpenGL, Graphics (Fancy/Fast),
//! Smooth Lighting and the key bindings (a touch screen has no keys; `OptionsUi::key` takes one when a keyboard event exists).
//! Not done: Achievements and Statistics (drawn greyed out, there is no stat system).

use crate::input::gamepad::{Btn, PadCfg, ACTS};
use crate::input::touch_ui::{ScreenEv, HOLD_SECS, SWING_SECS};
use crate::menu::{dirt, grad, rgb, scaled_gs, tw};
use crate::render::hud::{HudPipeline, HudVertex};
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
}

impl Opt {
    /// `getEnumFloat`: a slider.
    fn slider(self) -> bool {
        matches!(self, Opt::Music | Opt::Sound | Opt::Sensitivity | Opt::DigDelay)
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
                (k, ..) if k.starts_with("pad_") => s.pad.load(k, v),
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
            "music:{}\nsound:{}\ninvertYMouse:{}\nmouseSensitivity:{}\nviewDistance:{}\nguiScale:{}\nbobView:{}\nanaglyph3d:{}\nadvancedOpengl:{}\nfpsLimit:{}\ndifficulty:{}\nfancyGraphics:{}\nao:{}\nlod:{}\ndigDelay:{}\n",
            self.music, self.sound, self.invert_mouse, self.sensitivity, self.render_distance, self.gui_scale, self.view_bobbing, self.anaglyph, self.advanced_opengl, self.limit_framerate, self.difficulty, self.fancy_graphics, self.ambient_occlusion, self.lod, self.dig_delay
        );
        for (id, code) in KEY_IDS.iter().zip(self.keys) {
            t.push_str(&format!("key_key.{id}:{code}\n"));
        }
        self.pad.save_into(&mut t);
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
                Opt::RenderDistance => ["Far", "Normal", "Short", "Tiny"][self.render_distance.min(3) as usize].into(),
                Opt::Difficulty => ["Peaceful", "Easy", "Normal", "Hard"][self.difficulty.min(3) as usize].into(),
                Opt::GuiScale => ["Auto", "Small", "Normal", "Large"][self.gui_scale.min(3) as usize].into(),
                Opt::FramerateLimit => ["Max FPS", "Balanced", "Power saver"][self.limit_framerate.min(2) as usize].into(),
                _ => (if self.fancy_graphics { "Fancy" } else { "Fast" }).into(),
            }
        };
        format!("{}: {v}", o.name())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OptScr {
    Main,
    Video,
    /// The keys tab of Controls.
    Controls,
    /// The Controller tab (Controllify): its button bindings, and its tuning page.
    Pad,
    PadTune,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Act {
    Opt(Opt),
    Lod,
    Video,
    Controls,
    Done,
    Key(usize),
    TabKeys,
    TabPad,
    PadPage,
    PadBind(usize),
    PadKnob(usize),
    PadToggle(usize),
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
    matches!(a, Act::Opt(o) if o.slider()) || matches!(a, Act::PadKnob(_))
}

fn set_slider(a: Act, st: &mut Settings, v: f32) {
    match a {
        Act::Opt(o) => st.set_float(o, v),
        Act::PadKnob(i) => st.pad.set_knob(i, v),
        _ => {}
    }
}

/// `GuiSlider`'s value for a finger at `x`: its 4 px ends are not part of the track.
fn slider_value(r: [f32; 4], x: f32) -> f32 {
    ((x - (r[0] + 4.0)) / (r[2] - 8.0)).clamp(0.0, 1.0)
}

/// A `GuiButton` (a slider when `knob` has its value): the label centred, white-ish, yellow under the finger, grey when disabled.
pub(crate) fn button(v: &mut Vec<HudVertex>, s: f32, r: [f32; 4], label: &str, on: bool, over: bool, knob: Option<f32>) {
    let state = if knob.is_some() || !on { 0 } else if over { 2 } else { 1 };
    HudPipeline::push_button(v, r[0] * s, r[1] * s, r[2] * s, r[3] * s, state);
    if let Some(f) = knob {
        // `GuiSlider.mouseDragged`: the two 4 px halves of the knob, `widgets.png` row 66.
        let x = r[0] + (f * (r[2] - 8.0)).floor();
        HudPipeline::push_gui(v, x * s, r[1] * s, 4.0 * s, r[3] * s, (0.0, 256.0 + 66.0, 4.0, 20.0));
        HudPipeline::push_gui(v, (x + 4.0) * s, r[1] * s, 4.0 * s, r[3] * s, (196.0, 256.0 + 66.0, 4.0, 20.0));
    }
    let c = if !on { 0xA0A0A0 } else if over { 0xFFFFA0 } else { 0xE0E0E0 };
    HudPipeline::push_text(v, (r[0] + (r[2] - tw(label)) / 2.0) * s, (r[1] + (r[3] - 8.0) / 2.0) * s, s, label, rgb(c));
}

/// `drawDefaultBackground`: the dimmed world when one is behind, else the dirt.
fn background(v: &mut Vec<HudVertex>, s: f32, gw: f32, gh: f32, world: bool) {
    if world {
        grad(v, s, [0.0, 0.0, gw, gh], [16.0 / 255.0, 16.0 / 255.0, 16.0 / 255.0, 0xC0 as f32 / 255.0], [16.0 / 255.0, 16.0 / 255.0, 16.0 / 255.0, 0xD0 as f32 / 255.0]);
    } else {
        dirt(v, s, [0.0, 0.0, gw, gh], 0.0, 0x404040);
    }
}

/// `GuiOptions` and the two screens under it, `GuiVideoSettings` and `GuiControls`.
pub struct OptionsUi {
    pub scr: OptScr,
    /// The slider the finger is dragging.
    drag: Option<Act>,
    /// `GuiControls.buttonId`: the key button waiting for its key.
    wait: Option<usize>,
    /// The Controller tab's button waiting for a pad press.
    wait_pad: Option<usize>,
    /// Where the finger is (GUI units), for the highlight.
    finger: Option<(f32, f32)>,
}

impl Default for OptionsUi {
    fn default() -> Self {
        Self::new()
    }
}

impl OptionsUi {
    pub fn new() -> Self {
        Self { scr: OptScr::Main, drag: None, wait: None, wait_pad: None, finger: None }
    }

    fn widgets(&self, gw: f32, gh: f32, st: &Settings) -> Vec<Wid> {
        let (hw, top) = ((gw / 2.0).floor(), (gh / 6.0).floor());
        let cell = |i: usize| (hw - 155.0 + (i % 2) as f32 * 160.0, top + 24.0 * (i / 2) as f32);
        let wide = |act, y: f32, label: &str| Wid { act, r: [hw - 100.0, y, 200.0, 20.0], label: label.to_string() };
        let opts = |list: &[Opt]| -> Vec<Wid> {
            list.iter().enumerate().map(|(i, &o)| Wid { act: Act::Opt(o), r: [cell(i).0, cell(i).1, 150.0, 20.0], label: st.text(o) }).collect()
        };
        let mut w = match self.scr {
            OptScr::Main => {
                let mut w = opts(&[Opt::Music, Opt::Sound, Opt::InvertMouse, Opt::Sensitivity, Opt::Difficulty]);
                // The sixth cell of the grid is free in b1.7.3: the LoD switch.
                w.push(Wid { act: Act::Lod, r: [cell(5).0, cell(5).1, 150.0, 20.0], label: format!("LoD: {}", if st.lod { "ON" } else { "OFF" }) });
                w.push(wide(Act::Video, top + 96.0 + 12.0, "Video Settings..."));
                w.push(wide(Act::Controls, top + 120.0 + 12.0, "Controls..."));
                w
            }
            OptScr::Video => opts(&[Opt::Graphics, Opt::RenderDistance, Opt::AmbientOcclusion, Opt::FramerateLimit, Opt::Anaglyph, Opt::ViewBobbing, Opt::GuiScale, Opt::AdvancedOpengl]),
            OptScr::Controls => (0..KEY_NAMES.len())
                .map(|i| {
                    let name = key_name(st.keys[i]);
                    Wid { act: Act::Key(i), r: [cell(i).0, cell(i).1, 70.0, 20.0], label: if self.wait == Some(i) { format!("> {name} <") } else { name } }
                })
                .collect(),
            OptScr::Pad => ACTS
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    let b = if self.wait_pad == Some(i) { "> <" } else { st.pad.binds[i].name() };
                    Wid { act: Act::PadBind(i), r: [cell(i).0, cell(i).1, 150.0, 20.0], label: format!("{}: {b}", a.name()) }
                })
                .collect(),
            OptScr::PadTune => (0..PadCfg::knob_count())
                .map(|i| Wid { act: Act::PadKnob(i), r: [cell(i).0, cell(i).1, 150.0, 20.0], label: st.pad.knob_text(i) })
                .chain((0..PadCfg::toggle_count()).map(|i| {
                    let c = cell(PadCfg::knob_count() + i);
                    Wid { act: Act::PadToggle(i), r: [c.0, c.1, 150.0, 20.0], label: st.pad.toggle_text(i) }
                }))
                .collect(),
        };
        // The Dig Delay slider sits under the keys and under the pad's tuning (one setting for finger and pad).
        if matches!(self.scr, OptScr::Controls | OptScr::PadTune) {
            w.push(wide(Act::Opt(Opt::DigDelay), top + 120.0, &st.text(Opt::DigDelay)));
        }
        // Controls has two tabs: the keys, and the Controller (Controllify) with its buttons page and its tuning page.
        if matches!(self.scr, OptScr::Controls | OptScr::Pad | OptScr::PadTune) {
            w.push(Wid { act: Act::TabKeys, r: [hw - 100.0, 20.0, 99.0, 14.0], label: "Keys".into() });
            w.push(Wid { act: Act::TabPad, r: [hw + 1.0, 20.0, 99.0, 14.0], label: "Controller".into() });
        }
        if matches!(self.scr, OptScr::Pad | OptScr::PadTune) {
            w.push(wide(Act::PadPage, top + 144.0, if self.scr == OptScr::Pad { "Tuning..." } else { "Buttons..." }));
        }
        w.push(wide(Act::Done, top + 168.0, "Done"));
        w
    }

    /// A finger event in px. Sliders follow the finger, every other button acts when the finger is lifted over it.
    pub fn event(&mut self, ev: ScreenEv, w: f32, h: f32, st: &mut Settings) -> Option<OptEv> {
        let (s, gw, gh) = scaled_gs(w, h, st.gui_scale);
        let wids = self.widgets(gw, gh, st);
        let at = |x: f32, y: f32| wids.iter().find(|b| inside(b.r, x / s, y / s));
        match ev {
            ScreenEv::Down(x, y) => {
                self.finger = Some((x / s, y / s));
                if let Some(Wid { act, r, .. }) = at(x, y).filter(|b| is_slider(b.act)) {
                    self.drag = Some(*act);
                    set_slider(*act, st, slider_value(*r, x / s));
                }
                None
            }
            ScreenEv::Move(x, y) => {
                self.finger = Some((x / s, y / s));
                if let Some(a) = self.drag {
                    if let Some(b) = wids.iter().find(|b| b.act == a) {
                        set_slider(a, st, slider_value(b.r, x / s));
                    }
                }
                None
            }
            ScreenEv::Up(x, y) => {
                self.finger = None;
                if self.drag.take().is_some() {
                    st.save(); // `GuiOptions` saves when the screen is left; here when the slider is let go
                    return None;
                }
                let act = at(x, y)?.act;
                self.act(act, st)
            }
        }
    }

    /// `actionPerformed`.
    fn act(&mut self, a: Act, st: &mut Settings) -> Option<OptEv> {
        self.wait = None; // `GuiControls`: any button ends the wait
        self.wait_pad = None;
        match a {
            Act::Opt(o) => st.cycle(o),
            Act::Lod => {
                st.lod = !st.lod;
                st.save();
            }
            Act::Video => {
                st.save();
                self.scr = OptScr::Video;
            }
            Act::Controls => {
                st.save();
                self.scr = OptScr::Controls;
            }
            Act::Key(i) => self.wait = Some(i),
            Act::TabKeys => self.scr = OptScr::Controls,
            Act::TabPad => self.scr = OptScr::Pad,
            Act::PadPage => self.scr = if self.scr == OptScr::Pad { OptScr::PadTune } else { OptScr::Pad },
            Act::PadBind(i) => self.wait_pad = Some(i),
            Act::PadKnob(_) => {}
            Act::PadToggle(i) => {
                st.pad.toggle(i);
                st.save();
            }
            Act::Done => {
                st.save();
                if self.scr == OptScr::Main {
                    return Some(OptEv::Close);
                }
                self.scr = OptScr::Main;
            }
        }
        None
    }

    /// `GuiControls.keyTyped`: the key that follows a tap on a key button becomes its binding.
    #[allow(dead_code)] // ponytail: nothing sends key events yet (touch only)
    pub fn key(&mut self, code: u32, st: &mut Settings) {
        if let Some(i) = self.wait.take() {
            st.keys[i] = code;
            st.save();
        }
    }

    /// The Controller tab waits for a pad button: `Pad` swallows the next press into `Pad::captured`.
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

    pub fn draw(&self, v: &mut Vec<HudVertex>, w: f32, h: f32, st: &Settings, world: bool) {
        let (s, gw, gh) = scaled_gs(w, h, st.gui_scale);
        let hw = (gw / 2.0).floor();
        background(v, s, gw, gh, world);
        let title = match self.scr {
            OptScr::Main => "Options",
            OptScr::Video => "Video Settings",
            OptScr::Controls | OptScr::Pad | OptScr::PadTune => "Controls",
        };
        let ty = if matches!(self.scr, OptScr::Controls | OptScr::Pad | OptScr::PadTune) { 8.0 } else { 20.0 }; // room for the tabs
        HudPipeline::push_text(v, (hw - tw(title) / 2.0) * s, ty * s, s, title, rgb(0xFFFFFF));
        for b in self.widgets(gw, gh, st) {
            let over = self.finger.is_some_and(|(x, y)| inside(b.r, x, y));
            let knob = match b.act {
                Act::Opt(o) if o.slider() => Some(st.float(o)),
                Act::PadKnob(i) => Some(st.pad.knob(i)),
                _ => None,
            };
            // The tab you are on is drawn greyed.
            let on = !matches!((self.scr, b.act), (OptScr::Controls, Act::TabKeys) | (OptScr::Pad | OptScr::PadTune, Act::TabPad));
            button(v, s, b.r, &b.label, on, over, knob);
            if let (OptScr::Controls, Act::Key(i)) = (self.scr, b.act) {
                HudPipeline::push_text(v, (b.r[0] + 70.0 + 6.0) * s, (b.r[1] + 7.0) * s, s, KEY_NAMES[i], rgb(0xFFFFFF));
            }
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
}

/// What a pause-menu button asks of the game.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PauseEv {
    Resume,
    Quit,
    Pregen,
    Fly,
    LodDebug,
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
}

/// `GuiIngameMenu` (and the death screen, `GuiGameOver`, which shares it).
pub struct Pause {
    opt: Option<OptionsUi>,
    finger: Option<(f32, f32)>,
    born: Instant,
}

impl Default for Pause {
    fn default() -> Self {
        Self::new()
    }
}

impl Pause {
    pub fn new() -> Self {
        Self { opt: None, finger: None, born: Instant::now() }
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
                (PAct::Achievements, [hw - 100.0, q + 48.0 - 16.0, 98.0, 20.0], "Achievements".to_string(), false),
                (PAct::Stats, [hw + 2.0, q + 48.0 - 16.0, 98.0, 20.0], "Statistics".to_string(), false),
            ]
        };
        // Kept from the old pause menu, top-left.
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
        v.push((PAct::Pregen, [2.0, 2.0, 230.0, 20.0], pregen, true));
        v.push((PAct::Fly, [236.0, 2.0, 70.0, 20.0], if info.flying { "Fly: on" } else { "Fly: off" }.to_string(), true));
        v.push((PAct::Debug, [2.0, 24.0, 230.0, 20.0], debug.to_string(), true));
        v
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
                let (act, ..) = Self::buttons(gw, gh, info).into_iter().find(|b| b.3 && inside(b.1, gx, gy))?;
                return match act {
                    PAct::Resume => Some(PauseEv::Resume),
                    PAct::Quit => Some(PauseEv::Quit),
                    PAct::Pregen => Some(PauseEv::Pregen),
                    PAct::Fly => Some(PauseEv::Fly),
                    PAct::Debug => Some(PauseEv::LodDebug),
                    PAct::Options => {
                        self.opt = Some(OptionsUi::new());
                        None
                    }
                    PAct::Achievements | PAct::Stats => None,
                };
            }
        }
        None
    }

    pub fn pad_waiting(&self) -> bool {
        self.opt.as_ref().is_some_and(OptionsUi::pad_waiting)
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
            HudPipeline::push_text(v, (hw - tw("Game menu") / 2.0) * s, 40.0 * s, s, "Game menu", rgb(0xFFFFFF));
            // "Saving level.." for the first 20 ticks, its grey pulsing every 10.
            let t = self.born.elapsed().as_secs_f32();
            if t < 1.0 {
                let g = ((((t * 20.0) % 10.0) / 10.0 * std::f32::consts::TAU).sin() * 0.2 + 0.8) * 255.0;
                let g = g as u32;
                HudPipeline::push_text(v, 8.0 * s, (gh - 16.0) * s, s, "Saving level..", rgb(g << 16 | g << 8 | g));
            }
        }
        for (act, r, label, on) in Self::buttons(gw, gh, info) {
            let over = self.finger.is_some_and(|(x, y)| inside(r, x, y));
            let lit = (act == PAct::Pregen && info.pregen.is_some_and(|p| p.0)) || (act == PAct::Fly && info.flying) || (act == PAct::Debug && info.debug != 0);
            button(v, s, r, &label, on, over || lit, None);
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

    /// A tap on the first control of `o` whose text is `label`.
    fn tap(o: &mut OptionsUi, st: &mut Settings, label: &str) -> Option<OptEv> {
        let (_, gw, gh) = scaled_gs(SZ.0, SZ.1, st.gui_scale);
        let r = o.widgets(gw, gh, st).into_iter().find(|b| b.label == label).unwrap_or_else(|| panic!("no {label}")).r;
        let (x, y) = px(r, st);
        o.event(ScreenEv::Down(x, y), SZ.0, SZ.1, st);
        o.event(ScreenEv::Up(x, y), SZ.0, SZ.1, st)
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
        st.save();
        let back = Settings::load(&root);
        assert_eq!((back.sensitivity, back.invert_mouse, back.render_distance, back.lod, back.keys[4]), (0.25, true, 0, false, 50));
        assert!(std::fs::read_to_string(root.join("options.txt")).unwrap().contains("key_key.jump:50"));
    }

    /// Options -> LoD button -> Video Settings (cycle Render Distance) -> Done -> Controls (wait for a key) -> Done -> Done closes.
    #[test]
    fn options_screens_flow() {
        let mut st = Settings::default();
        let mut o = OptionsUi::new();
        assert!(tap(&mut o, &mut st, "LoD: ON").is_none() && !st.lod);
        tap(&mut o, &mut st, "Video Settings...");
        assert_eq!(o.scr, OptScr::Video);
        tap(&mut o, &mut st, "Render Distance: Short");
        assert_eq!(st.render_distance, 3);
        tap(&mut o, &mut st, "Done");
        assert_eq!(o.scr, OptScr::Main);
        tap(&mut o, &mut st, "Controls...");
        tap(&mut o, &mut st, "S");
        assert!(tap(&mut o, &mut st, "> S <").is_none(), "tapping it again keeps waiting");
        o.key(44, &mut st);
        assert_eq!(st.keys[2], 44);
        tap(&mut o, &mut st, "Done");
        assert_eq!(tap(&mut o, &mut st, "Done"), Some(OptEv::Close));
        let mut v = Vec::new();
        o.draw(&mut v, SZ.0, SZ.1, &st, true);
        assert!(!v.is_empty());
    }

    /// A slider follows the finger and ends at the track's ends.
    #[test]
    fn slider_drags() {
        let mut st = Settings::default();
        let mut o = OptionsUi::new();
        let (_, gw, gh) = scaled_gs(SZ.0, SZ.1, 0);
        let r = o.widgets(gw, gh, &st).into_iter().find(|b| b.label.starts_with("Music")).unwrap().r;
        let (s, ..) = scaled_gs(SZ.0, SZ.1, 0);
        let y = (r[1] + 10.0) * s;
        o.event(ScreenEv::Down((r[0] + 4.0) * s, y), SZ.0, SZ.1, &mut st);
        assert_eq!(st.music, 0.0);
        o.event(ScreenEv::Move((r[0] + 4.0 + (r[2] - 8.0) / 2.0) * s, y), SZ.0, SZ.1, &mut st);
        assert!((st.music - 0.5).abs() < 0.01);
        o.event(ScreenEv::Move((r[0] + r[2] + 50.0) * s, y), SZ.0, SZ.1, &mut st);
        assert_eq!(st.music, 1.0);
        o.event(ScreenEv::Up((r[0] + r[2]) * s, y), SZ.0, SZ.1, &mut st);
        assert!(o.drag.is_none());
    }

    /// Controls > Controller tab: rebind a button from the pad, drag a tuning slider, flip a toggle, and it all reaches options.txt.
    #[test]
    fn controller_tab_flow() {
        let root = tmp("pad");
        let mut st = Settings::load(&root);
        let mut o = OptionsUi::new();
        tap(&mut o, &mut st, "Controls...");
        tap(&mut o, &mut st, "Controller");
        assert_eq!(o.scr, OptScr::Pad);
        tap(&mut o, &mut st, "Jump: A");
        assert!(o.pad_waiting());
        o.pad_button(Btn::X, &mut st);
        assert_eq!((st.pad.binds[0], o.pad_waiting()), (Btn::X, false));
        tap(&mut o, &mut st, "Tuning...");
        assert_eq!(o.scr, OptScr::PadTune);
        tap(&mut o, &mut st, "Invert Y: OFF");
        assert!(st.pad.invert_y);
        let (_, gw, gh) = scaled_gs(SZ.0, SZ.1, 0);
        let r = o.widgets(gw, gh, &st).into_iter().find(|b| b.label.starts_with("Look X")).unwrap().r;
        let (s, ..) = scaled_gs(SZ.0, SZ.1, 0);
        let y = (r[1] + 10.0) * s;
        o.event(ScreenEv::Down((r[0] + 4.0) * s, y), SZ.0, SZ.1, &mut st);
        o.event(ScreenEv::Up((r[0] + 4.0) * s, y), SZ.0, SZ.1, &mut st);
        assert_eq!(st.pad.look_h, 0.0);
        tap(&mut o, &mut st, "Keys");
        assert_eq!(o.scr, OptScr::Controls);
        let back = Settings::load(&root);
        assert_eq!((back.pad.binds[0], back.pad.invert_y, back.pad.look_h), (Btn::X, true, 0.0));
        let mut v = Vec::new();
        o.draw(&mut v, SZ.0, SZ.1, &st, true);
        assert!(!v.is_empty());
    }

    /// The Dig Delay slider: on the keys and the tuning pages, 100 ms .. `HOLD_SECS`, default `SWING_SECS`, kept in options.txt.
    #[test]
    fn dig_delay_slider() {
        let root = tmp("dig");
        let mut st = Settings::load(&root);
        assert!((st.dig_secs() - SWING_SECS).abs() < 1e-4);
        let mut o = OptionsUi::new();
        tap(&mut o, &mut st, "Controls...");
        let (s, gw, gh) = scaled_gs(SZ.0, SZ.1, 0);
        let r = o.widgets(gw, gh, &st).into_iter().find(|b| b.label.starts_with("Dig Delay")).unwrap().r;
        let y = (r[1] + 10.0) * s;
        o.event(ScreenEv::Down((r[0] + r[2] - 2.0) * s, y), SZ.0, SZ.1, &mut st);
        o.event(ScreenEv::Up((r[0] + r[2] - 2.0) * s, y), SZ.0, SZ.1, &mut st);
        assert_eq!((st.dig_delay, st.dig_secs()), (1.0, HOLD_SECS));
        assert_eq!(Settings::load(&root).dig_delay, 1.0);
        tap(&mut o, &mut st, "Controller");
        tap(&mut o, &mut st, "Tuning...");
        assert!(o.widgets(gw, gh, &st).iter().any(|b| b.label.starts_with("Dig Delay")));
    }

    #[test]
    fn pause_menu_buttons() {
        let mut st = Settings::default();
        let mut p = Pause::new();
        let info = PauseInfo { dead: false, pregen: Some((false, 3)), debug: 0, flying: false };
        let (_, gw, gh) = scaled_gs(SZ.0, SZ.1, 0);
        let press = |p: &mut Pause, st: &mut Settings, label: &str| {
            let b = Pause::buttons(gw, gh, &info).into_iter().find(|b| b.2 == label).unwrap_or_else(|| panic!("no {label}"));
            let (x, y) = px(b.1, st);
            p.event(ScreenEv::Down(x, y), SZ.0, SZ.1, st, &info);
            p.event(ScreenEv::Up(x, y), SZ.0, SZ.1, st, &info)
        };
        assert_eq!(press(&mut p, &mut st, "Back to game"), Some(PauseEv::Resume));
        assert_eq!(press(&mut p, &mut st, "Save and quit to title"), Some(PauseEv::Quit));
        assert_eq!(press(&mut p, &mut st, "Pregen: off, 3 chunks"), Some(PauseEv::Pregen));
        assert_eq!(press(&mut p, &mut st, "Fly: off"), Some(PauseEv::Fly));
        assert_eq!(press(&mut p, &mut st, "LoD debug: off"), Some(PauseEv::LodDebug));
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
