//! Controllify (isXander's Fabric mod, LGPL-3.0-or-later), ported to mc-rs: game-pad support on top of `TouchUi`, the one sink every input ends in.
//!
//! Ported from the Java: every binding with its context and default (`bindings.rs`, `bind_defs.rs`), the stick deadzone of each stick (`ControllerUtils.deadzone`,
//! `DeadzoneGroup`), the look curves (`InputCurves`, eased on the stick's length so aiming stays circular), 10 degrees per tick at 100% look sensitivity
//! (`InGameInputHandler.handleRegularLook`), its LCE mode (a quadratic curve per axis, 7.5 degrees a tick) and the lower look speed while aiming, flick stick
//! (`handleFlickStick`), the hold-repeat of the hotbar bumpers (10 / 4 ticks) and of drop (20 / 1), drop item and drop stack, the activation threshold of
//! analogue triggers, toggle sneak and sprint, the virtual mouse that drives a container screen (`VirtualMouseHandler`, with its per-screen toggle on the screens that
//! are focus screens by default), the radial hotbar (`HOTBAR_SLOT_SELECT`) and vibration (`rumble_fx.rs`: Controlify's effects per source, 20 Hz; `rumble.rs` sends them to the pad).
//! Each binding is rebindable in Options > Controls > Controller (`options.rs`), saved in `options.txt` as `pad_*` (only what differs from the default).
//!
//! Button guide (`Pad::draw_guide`, `guide.rs`): Controlify's contextual guide, ported from its rule engine: the rules are its own JSON
//! (`tools/gen_guide.py`), the glyphs its controller fonts (`tools/gen_glyphs.py`, `glyph_map.rs`). While a pad is in use two columns in the top
//! (or bottom) corners name what each bound button does right now, from the crosshair, the held item and the player; a container screen gets
//! its own (take, place, swap, quick move...). Controls > Controller picks the controller type of the glyphs, the guide's detail (full /
//! reduced / minimal), its position, its size and the two switches (in-game, screens).
//!
//! Not ported (nothing in mc-rs to hang them on): gyro, DualSense LEDs / adaptive triggers / touchpad, on-screen keyboard, splitscreen, joystick remapping, the server vibration API, chat, swap hands
//! pick block, game modes (b1.7.3 has no off hand and mc-rs no chat yet: those binds are in the registry but hidden). Not yet (later phases): the radial menu, the keyboard. UNVERIFIED on real hardware: the Android axis / key mapping
//! follows the documented layout (X/Y left stick, Z/Rz right stick, Ltrigger/Rtrigger, hat = d-pad).

use super::bind_defs::B;
use super::bindings::{ctx, Ax, Binds, Input, N_INPUTS};
use super::touch_ui::{HOLD_SECS, TAP_SECS};
use super::glyph_map::TYPES;
use super::rumble_fx::{fx, Cmd, Effect, Handle, Manager, Source as RSource, Strengths, Wire, TICK_SECS};
use super::guide::{Domain, Guide, Verbosity};
use super::vmouse::{Behaviour, HoldRepeat, SnapPoint, VIn, VMouse};
use crate::render::hud::{HudPipeline, HudVertex};
use android_activity::input::{Axis, KeyAction, KeyEvent, Keycode, MotionEvent, Source};

/// A physical button, in `Btn::bit` order. Triggers are buttons too (analogue value over the activation threshold).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Btn {
    A,
    B,
    X,
    Y,
    Lb,
    Rb,
    Lt,
    Rt,
    L3,
    R3,
    Start,
    Back,
    Up,
    Down,
    Left,
    Right,
}

impl Btn {
    pub const ALL: [Btn; 16] = [Btn::A, Btn::B, Btn::X, Btn::Y, Btn::Lb, Btn::Rb, Btn::Lt, Btn::Rt, Btn::L3, Btn::R3, Btn::Start, Btn::Back, Btn::Up, Btn::Down, Btn::Left, Btn::Right];

    fn bit(self) -> u16 {
        1 << self as u16
    }

    pub fn name(self) -> &'static str {
        ["A", "B", "X", "Y", "LB", "RB", "LT", "RT", "L3", "R3", "Start", "Back", "D-Up", "D-Down", "D-Left", "D-Right"][self as usize]
    }

    /// On the left half of a pad (the left bumper / trigger / stick, Back and the d-pad): its hint goes on the left of the screen, the others on the right.
    fn left(self) -> bool {
        matches!(self, Btn::Lb | Btn::Lt | Btn::L3 | Btn::Back | Btn::Up | Btn::Down | Btn::Left | Btn::Right)
    }

    /// The Controlify input this button is (`controlify:button/south`...), which says its glyph in `glyph_map`. The face buttons are Android's, which
    /// name the PLACE: A is the bottom one, B right, X left, Y top; the controller's own font draws the letter that place has on it (Nintendo's on a Switch).
    pub fn input(self) -> &'static str {
        Self::INPUT_NAMES[self as usize]
    }

    const INPUT_NAMES: [&'static str; 16] = [
        "button/south", "button/east", "button/west", "button/north", "button/left_shoulder", "button/right_shoulder", "axis/left_trigger", "axis/right_trigger",
        "button/left_stick", "button/right_stick", "button/start", "button/back", "button/dpad_up", "button/dpad_down", "button/dpad_left", "button/dpad_right",
    ];
}

/// `ControllerBindings`' sliders: each one's name, its `options.txt` id (`pad_<id>`), its range at slider 0 and 1 (Controlify's own where it has one) and its step.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Knob {
    LookH,
    LookV,
    DzLeft,
    DzRight,
    Cursor,
    Threshold,
    Rumble,
    GuideSize,
    RumblePlayer,
    RumbleWorld,
    RumbleInteraction,
    RumbleGui,
}

impl Knob {
    pub const ALL: [Knob; 12] = [Knob::LookH, Knob::LookV, Knob::DzLeft, Knob::DzRight, Knob::Cursor, Knob::Threshold, Knob::Rumble, Knob::GuideSize, Knob::RumblePlayer, Knob::RumbleWorld, Knob::RumbleInteraction, Knob::RumbleGui];
    const NAMES: [&'static str; 12] = ["Look X", "Look Y", "Left Deadzone", "Right Deadzone", "Cursor", "Button Threshold", "Vibration", "Guide Size", "Vib Player", "Vib World", "Vib Interaction", "Vib GUI"];
    /// `rumble` is the master vibration (`RumbleSource.MASTER`; the old `pad_rumble` id is kept), the last four are the other sources.
    const IDS: [&'static str; 12] = ["lookH", "lookV", "deadzoneLeft", "deadzoneRight", "cursor", "threshold", "rumble", "hintSize", "rumblePlayer", "rumbleWorld", "rumbleInteraction", "rumbleGui"];
    /// Controlify: the look and virtual mouse sliders are 10% to 200% in 5% steps, a deadzone 0 to 100% in 2% steps, the activation threshold 1% to 100%.
    const RANGE: [(f32, f32); 12] = [(0.1, 2.0), (0.1, 2.0), (0.0, 1.0), (0.0, 1.0), (0.1, 2.0), (0.01, 1.0), (0.0, 2.0), (0.5, 2.0), (0.0, 2.0), (0.0, 2.0), (0.0, 2.0), (0.0, 2.0)];
    const STEP: [f32; 12] = [0.05, 0.05, 0.02, 0.02, 0.05, 0.01, 0.05, 0.05, 0.05, 0.05, 0.05, 0.05];
}

/// The switches and cyclers, in `options.txt` order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tog {
    Curve,
    InvertX,
    InvertY,
    ToggleSneak,
    ToggleSprint,
    ReduceAim,
    Lce,
    Flick,
    Hints,
    IconStyle,
    HintLayout,
    GuideDetail,
    ScreenGuides,
    AllowVibration,
    TestVibration,
    ReachAround,
}

impl Tog {
    pub const ALL: [Tog; 16] = [
        Tog::Curve,
        Tog::InvertX,
        Tog::InvertY,
        Tog::ToggleSneak,
        Tog::ToggleSprint,
        Tog::ReduceAim,
        Tog::Lce,
        Tog::Flick,
        Tog::Hints,
        Tog::IconStyle,
        Tog::HintLayout,
        Tog::GuideDetail,
        Tog::ScreenGuides,
        Tog::AllowVibration,
        Tog::TestVibration,
        Tog::ReachAround,
    ];
    const NAMES: [&'static str; 16] = [
        "Look Curve",
        "Invert Look X",
        "Invert Look Y",
        "Toggle Sneak",
        "Toggle Sprint",
        "Reduce Aiming Sensitivity",
        "LCE Style Controls",
        "Flick Stick",
        "Button Guide",
        "Controller Icons",
        "Guide Position",
        "Guide Detail",
        "Screen Guides",
        "Allow Vibration",
        "Test Vibration",
        "Reach Around",
    ];
    const IDS: [&'static str; 16] = ["curve", "invertX", "invertY", "toggleSneak", "toggleSprint", "reduceAim", "lce", "flick", "hints", "iconStyle", "hintLayout", "guideDetail", "screenGuides", "vibration", "testVibration", "reachAround"];
}

/// A line of the controller page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Row {
    Knob(Knob),
    Tog(Tog),
}

/// The groups of the controller page, as Controlify's config screen has them (`makeSensitivityGroup`, `makeControlsGroup`, `makeAccessibilityGroup`,
/// `makeDeadzoneGroup`): its name and its lines. Flick stick is Controlify's gyro group; with no gyro here it is a plain switch of the sensitivity group.
pub const SECTIONS: [(&str, &[Row]); 5] = [
    (
        "Sensitivity",
        &[
            Row::Knob(Knob::LookH),
            Row::Knob(Knob::LookV),
            Row::Tog(Tog::InvertX),
            Row::Tog(Tog::InvertY),
            Row::Knob(Knob::Cursor),
            Row::Tog(Tog::ReduceAim),
            Row::Tog(Tog::Curve),
            Row::Tog(Tog::Lce),
            Row::Tog(Tog::Flick),
        ],
    ),
    ("Controls", &[Row::Tog(Tog::ToggleSprint), Row::Tog(Tog::ToggleSneak), Row::Tog(Tog::ReachAround)]),
    (
        "Accessibility",
        &[Row::Tog(Tog::Hints), Row::Tog(Tog::HintLayout), Row::Tog(Tog::GuideDetail), Row::Tog(Tog::ScreenGuides), Row::Knob(Knob::GuideSize), Row::Tog(Tog::IconStyle)],
    ),
    ("Deadzones", &[Row::Knob(Knob::DzLeft), Row::Knob(Knob::DzRight), Row::Knob(Knob::Threshold)]),
    (
        "Haptics",
        &[
            Row::Tog(Tog::AllowVibration),
            Row::Knob(Knob::Rumble),
            Row::Knob(Knob::RumblePlayer),
            Row::Knob(Knob::RumbleWorld),
            Row::Knob(Knob::RumbleInteraction),
            Row::Knob(Knob::RumbleGui),
            Row::Tog(Tog::TestVibration),
        ],
    ),
];

/// Where the in-game guide goes (`PadCfg::layout`, Controlify's `ingame_guide_bottom`).
const LAYOUTS: [&str; 2] = ["Top", "Bottom"];
/// `InputCurves`.
const CURVES: [&str; 6] = ["Linear", "Gentle", "Standard", "Strong", "Precision", "S-Curve"];

/// The screens whose virtual mouse the player can switch (`VMOUSE_TOGGLE` / `virtualMouseScreens`): the ones that are focus screens by default.
/// A container screen always has the cursor (`CURSOR_SCROLL`: the toggle says "unavailable").
pub const VM_TITLE: u8 = 1;
pub const VM_DELETE: u8 = 2;
pub const VM_PAUSE: u8 = 4;

/// Controlify's `InputSettings` (the sensitivity, deadzone, threshold and generic parts) and the bindings; lives in `Settings`.
#[derive(Clone, Debug, PartialEq)]
pub struct PadCfg {
    pub binds: Binds,
    pub look_h: f32,
    pub look_v: f32,
    /// The deadzone of each stick (`DeadzoneGroup` left_stick / right_stick): applied per axis (`ControllerUtils.deadzone`).
    pub dz_left: f32,
    pub dz_right: f32,
    /// Virtual mouse speed.
    pub cursor: f32,
    /// `buttonActivationThreshold`: the value over which an axis bind counts as pressed.
    pub threshold: f32,
    /// `RumbleSettings`: vibration on or off and the strength of each source (`rumble_fx::Strengths`; the master's is the old `pad_rumble`).
    pub vib: Strengths,
    pub curve: u8,
    pub invert_x: bool,
    pub invert_y: bool,
    pub toggle_sneak: bool,
    /// `toggle_sprint`: the sprint bind switches sprinting on and off instead of holding it.
    pub toggle_sprint: bool,
    /// `reduce_aiming`: less look speed while a bow-like hold is on (`Pad::aiming`).
    pub reduce_aim: bool,
    /// `is_lce`: LCE-style look (`x * |x| * 0.75` per axis, 7.5 degrees a tick), virtual mouse speed and aiming reduction.
    pub lce: bool,
    /// `flick_stick`: pushing the look stick over the threshold turns the view to where it points.
    pub flick: bool,
    /// `show_ingame_guide`: the two columns of what the buttons do while playing (`Pad::draw_guide`).
    pub hints: bool,
    /// Index into `glyph_map::TYPES`: the controller whose glyphs the guide draws (0 Xbox, 1 PlayStation 5, 2 PlayStation 4, 3 PlayStation 3, 4 Switch, 5 Steam, 6 Generic).
    /// Controlify detects it from the device; Android names the device less reliably, so it is a choice here.
    pub style: u8,
    /// The guide's scale as a multiple of the GUI scale (0.5 .. 2.0; 1.0 = the same size as the GUI): Controlify's `ingame_gui_scale`.
    pub hint_size: f32,
    /// Index into `LAYOUTS`: 0 top, 1 bottom (`ingame_guide_bottom`).
    pub layout: u8,
    /// `GuideVerbosity` as an index: 0 full, 1 reduced, 2 minimal.
    pub verbosity: u8,
    /// `show_screen_guides`: the container screen's guide.
    pub screen_guides: bool,
    /// `GlobalSettings.reachAround` (`input/reach.rs`): off by default like Controlify's. One switch for its four modes: mc-rs has no multiplayer.
    pub reach_around: bool,
    /// The Test Vibration row was tapped: `lib.rs` takes it, plays `fx::test` and clears it. Not saved.
    pub test_vibration: bool,
    /// `virtualMouseScreens`: the `VM_*` bits of the focus screens the player gave the cursor.
    pub vm_screens: u8,
}

impl Default for PadCfg {
    /// `default_config.json`: 100% / 90% look, 0.12 deadzones, standard curve, 0.5 threshold, toggle sneak and sprint, reduced aiming.
    fn default() -> Self {
        Self {
            binds: Binds::default(),
            look_h: 1.0,
            look_v: 0.9,
            dz_left: 0.12,
            dz_right: 0.12,
            cursor: 1.0,
            threshold: 0.5,
            vib: Strengths::default(),
            curve: 2,
            invert_x: false,
            invert_y: false,
            toggle_sneak: true,
            toggle_sprint: true,
            reduce_aim: true,
            lce: false,
            flick: false,
            hints: true,
            style: 0,
            hint_size: 1.0,
            layout: 0,
            verbosity: 0,
            screen_guides: true,
            reach_around: false,
            test_vibration: false,
            vm_screens: 0,
        }
    }
}

impl PadCfg {
    /// The controller input a Controlify binding id is on (`jump` -> `button/south`), none where the bind is unbound or does nothing in this port (swap hands, pick block).
    /// `radial_menu` is the hotbar wheel until the radial menu itself comes (phase 4).
    pub fn input_of(&self, id: &str) -> Option<&'static str> {
        let id = if id == "radial_menu" { "hotbar_item_select_radial" } else { id };
        let b = B::from_id(id).filter(|b| b.def().live)?;
        self.binds.get(b).glyph()
    }

    fn knob_value(&self, k: Knob) -> f32 {
        match k {
            Knob::LookH => self.look_h,
            Knob::LookV => self.look_v,
            Knob::DzLeft => self.dz_left,
            Knob::DzRight => self.dz_right,
            Knob::Cursor => self.cursor,
            Knob::Threshold => self.threshold,
            Knob::Rumble => self.vib.by_source[RSource::Master.index()],
            Knob::GuideSize => self.hint_size,
            Knob::RumblePlayer => self.vib.by_source[RSource::Player.index()],
            Knob::RumbleWorld => self.vib.by_source[RSource::World.index()],
            Knob::RumbleInteraction => self.vib.by_source[RSource::Interaction.index()],
            Knob::RumbleGui => self.vib.by_source[RSource::Gui.index()],
        }
    }

    fn set_knob_value(&mut self, k: Knob, v: f32) {
        match k {
            Knob::LookH => self.look_h = v,
            Knob::LookV => self.look_v = v,
            Knob::DzLeft => self.dz_left = v,
            Knob::DzRight => self.dz_right = v,
            Knob::Cursor => self.cursor = v,
            Knob::Threshold => self.threshold = v,
            Knob::Rumble => self.vib.by_source[RSource::Master.index()] = v,
            Knob::GuideSize => self.hint_size = v,
            Knob::RumblePlayer => self.vib.by_source[RSource::Player.index()] = v,
            Knob::RumbleWorld => self.vib.by_source[RSource::World.index()] = v,
            Knob::RumbleInteraction => self.vib.by_source[RSource::Interaction.index()] = v,
            Knob::RumbleGui => self.vib.by_source[RSource::Gui.index()] = v,
        }
    }

    /// The slider position (0..1) of knob `i` (`Knob::ALL` order).
    pub fn knob(&self, i: usize) -> f32 {
        let (lo, hi) = Knob::RANGE[i];
        ((self.knob_value(Knob::ALL[i]) - lo) / (hi - lo)).clamp(0.0, 1.0)
    }

    /// Slider position `t` (0..1) to a value on the slider's step (`FloatSliderControllerBuilder.step`).
    pub fn set_knob(&mut self, i: usize, t: f32) {
        let (lo, hi) = Knob::RANGE[i];
        let step = Knob::STEP[i];
        let v = lo + t.clamp(0.0, 1.0) * (hi - lo);
        let v = (lo + ((v - lo) / step).round() * step).clamp(lo, hi);
        self.set_knob_value(Knob::ALL[i], v);
    }

    pub fn knob_text(&self, i: usize) -> String {
        format!("{}: {}%", Knob::NAMES[i], (self.knob_value(Knob::ALL[i]) * 100.0).round() as i32)
    }

    pub fn knob_count() -> usize {
        Knob::ALL.len()
    }

    pub fn toggle(&mut self, i: usize) {
        match Tog::ALL[i] {
            Tog::Curve => self.curve = (self.curve + 1) % CURVES.len() as u8,
            Tog::InvertX => self.invert_x = !self.invert_x,
            Tog::InvertY => self.invert_y = !self.invert_y,
            Tog::ToggleSneak => self.toggle_sneak = !self.toggle_sneak,
            Tog::ToggleSprint => self.toggle_sprint = !self.toggle_sprint,
            Tog::ReduceAim => self.reduce_aim = !self.reduce_aim,
            Tog::Lce => self.lce = !self.lce,
            Tog::Flick => self.flick = !self.flick,
            Tog::Hints => self.hints = !self.hints,
            Tog::IconStyle => self.style = (self.style + 1) % TYPES.len() as u8,
            Tog::HintLayout => self.layout = (self.layout + 1) % LAYOUTS.len() as u8,
            Tog::GuideDetail => self.verbosity = (self.verbosity + 1) % Verbosity::NAMES.len() as u8,
            Tog::ScreenGuides => self.screen_guides = !self.screen_guides,
            Tog::AllowVibration => self.vib.enabled = !self.vib.enabled,
            Tog::TestVibration => self.test_vibration = true,
            Tog::ReachAround => self.reach_around = !self.reach_around,
        }
    }

    pub fn toggle_text(&self, i: usize) -> String {
        let onoff = |b: bool| if b { "ON" } else { "OFF" };
        let t = Tog::ALL[i];
        let v = match t {
            Tog::Curve => CURVES[self.curve as usize % CURVES.len()],
            Tog::InvertX => onoff(self.invert_x),
            Tog::InvertY => onoff(self.invert_y),
            Tog::ToggleSneak => onoff(self.toggle_sneak),
            Tog::ToggleSprint => onoff(self.toggle_sprint),
            Tog::ReduceAim => onoff(self.reduce_aim),
            Tog::Lce => onoff(self.lce),
            Tog::Flick => onoff(self.flick),
            Tog::Hints => onoff(self.hints),
            Tog::IconStyle => TYPES[self.style as usize % TYPES.len()],
            Tog::HintLayout => LAYOUTS[self.layout as usize % LAYOUTS.len()],
            Tog::GuideDetail => Verbosity::NAMES[self.verbosity as usize % Verbosity::NAMES.len()],
            Tog::ScreenGuides => onoff(self.screen_guides),
            Tog::AllowVibration => onoff(self.vib.enabled),
            Tog::TestVibration => "Tap",
            Tog::ReachAround => onoff(self.reach_around),
        };
        format!("{}: {v}", Tog::NAMES[i])
    }

    pub fn toggle_count() -> usize {
        Tog::ALL.len()
    }

    /// The Look Curve row is greyed out under LCE (`lookInputCurveOpt.setAvailable(!isLCE)`).
    pub fn curve_available(&self) -> bool {
        !self.lce
    }

    /// `options.txt` line: `pad_<id>:<value>`. A bad value is ignored.
    pub fn load(&mut self, k: &str, v: &str) {
        let Some(k) = k.strip_prefix("pad_") else { return };
        if let Some(i) = Knob::IDS.iter().position(|id| *id == k) {
            if let Ok(f) = v.parse::<f32>() {
                let (lo, hi) = Knob::RANGE[i];
                self.set_knob_value(Knob::ALL[i], f.clamp(lo, hi)); // as written: the file holds a value that is already on its step
            }
        } else if k == "deadzone" {
            // the one deadzone of the builds before the groups
            if let Ok(f) = v.parse::<f32>() {
                self.dz_left = f.clamp(0.0, 1.0);
                self.dz_right = f.clamp(0.0, 1.0);
            }
        } else if let Some(i) = Tog::IDS.iter().position(|id| *id == k) {
            if let Ok(n) = v.parse::<u8>() {
                match Tog::ALL[i] {
                    Tog::Curve => self.curve = n.min(CURVES.len() as u8 - 1),
                    Tog::InvertX => self.invert_x = n != 0,
                    Tog::InvertY => self.invert_y = n != 0,
                    Tog::ToggleSneak => self.toggle_sneak = n != 0,
                    Tog::ToggleSprint => self.toggle_sprint = n != 0,
                    Tog::ReduceAim => self.reduce_aim = n != 0,
                    Tog::Lce => self.lce = n != 0,
                    Tog::Flick => self.flick = n != 0,
                    Tog::Hints => self.hints = n != 0,
                    Tog::IconStyle => self.style = n.min(TYPES.len() as u8 - 1),
                    Tog::HintLayout => self.layout = n.min(LAYOUTS.len() as u8 - 1),
                    Tog::GuideDetail => self.verbosity = n.min(Verbosity::NAMES.len() as u8 - 1),
                    Tog::ScreenGuides => self.screen_guides = n != 0,
                    Tog::AllowVibration => self.vib.enabled = n != 0,
                    Tog::TestVibration => {} // not a setting
                    Tog::ReachAround => self.reach_around = n != 0,
                }
            }
        } else if k == "vmouseScreens" {
            if let Ok(n) = v.parse::<u8>() {
                self.vm_screens = n & (VM_TITLE | VM_DELETE | VM_PAUSE);
            }
        } else {
            self.binds.load(k, v);
        }
    }

    pub fn save_into(&self, t: &mut String) {
        self.binds.save_into(t);
        for (i, id) in Knob::IDS.iter().enumerate() {
            t.push_str(&format!("pad_{id}:{}\n", self.knob_value(Knob::ALL[i])));
        }
        let b = |x: bool| x as u8;
        let vals = [
            self.curve,
            b(self.invert_x),
            b(self.invert_y),
            b(self.toggle_sneak),
            b(self.toggle_sprint),
            b(self.reduce_aim),
            b(self.lce),
            b(self.flick),
            b(self.hints),
            self.style,
            self.layout,
            self.verbosity,
            b(self.screen_guides),
            b(self.vib.enabled),
            0,
            b(self.reach_around),
        ];
        for (i, (id, v)) in Tog::IDS.iter().zip(vals).enumerate() {
            if Tog::ALL[i] == Tog::TestVibration {
                continue; // a button, not a setting
            }
            t.push_str(&format!("pad_{id}:{v}\n"));
        }
        t.push_str(&format!("pad_vmouseScreens:{}\n", self.vm_screens));
    }
}

/// `ControllerUtils.deadzone`.
fn deadzone(v: f32, dz: f32) -> f32 {
    let dz = dz.clamp(0.0, 0.95); // a slider at 100% would divide by nothing
    (v - v.signum() * v.abs().min(dz)) / (1.0 - dz)
}


/// `InputCurves`, applied to a stick's length (`applyEasingToLength`) so a diagonal keeps its direction.
fn ease(x: f32, y: f32, curve: u8) -> (f32, f32) {
    let l = x.hypot(y).min(1.0);
    if l == 0.0 {
        return (0.0, 0.0);
    }
    let e = match curve {
        0 => l,
        1 => l.powf(1.5),
        2 => l.powi(2),
        3 => l.powf(2.5),
        4 => 0.5 * l + 0.5 * l * l * l,
        _ => l * l * (3.0 - 2.0 * l),
    };
    (x / x.hypot(y) * e, y / x.hypot(y) * e)
}

/// 10 degrees per tick at 100% (`handleRegularLook`), 20 ticks a second, in radians.
const LOOK_RAD_PER_SEC: f32 = 10.0 * 20.0 * std::f32::consts::PI / 180.0;
/// The virtual cursor at full deflection crosses this many short sides a second (`virtual_mouse` 1.0).
const CURSOR_SPEED: f32 = 0.9;
const HOTBAR: usize = 9;
/// A held d-pad direction steps again after `NAV_FIRST` s, then every `NAV_REPEAT` s (`ScreenProcessor`'s `HoldRepeatHelper(10, 3)`, in 20 Hz ticks).
const NAV_FIRST: f32 = 0.5;
const NAV_REPEAT: f32 = 0.15;
/// `InGameInputHandler`: the hotbar bumpers repeat after 10 ticks, then every 4; a held drop after 20, then every tick.
const HOTBAR_REPEAT: (u32, u32) = (10, 4);
const DROP_REPEAT: (u32, u32) = (20, 1);
/// `handleFlickStick`: the turn is an 8 tick animation eased out (`EASE_OUT_EXPO`).
const FLICK_SECS: f32 = 8.0 * 0.05;
/// `handleRegularLook`: the look speed while aiming a bow-like use (`reduceAimingSensitivity`), and in LCE.
const AIM_REDUCE: f32 = 0.6;
const AIM_REDUCE_LCE: f32 = 0.15;
/// LCE's turn speed: 7.5 degrees a tick at 100%, so 0.75 of the 10 of the regular look.
const LCE_LOOK: f32 = 0.75;

/// What one frame of the pad asks of the game (`TouchUi::pad_update` applies it).
#[derive(Default, Debug)]
pub struct Out {
    /// (forward, side), set while the stick is out and once more when it returns to centre.
    pub mv: Option<(f32, f32)>,
    /// Radians (yaw right, pitch down) this frame.
    pub look: (f32, f32),
    pub jump: bool,
    pub dig: bool,
    /// The attack button is down (from the first frame: the arm swings at once, the dig waits for `HOLD_SECS`).
    pub attack: bool,
    pub zoom: bool,
    /// Sprinting: the sprint bind is held, or switched on with Toggle Sprint (Retro Sprint's key).
    pub sprint: bool,
    /// The view / info binds were pressed.
    pub view: bool,
    pub info: bool,
    pub sneak_down: bool,
    pub sneak_up: bool,
    pub place: bool,
    /// The Use bind is held, and went down this frame (C4 / C5: a weapon's right button is this bind, as the guard and special buttons are on the touch screen).
    pub use_down: bool,
    pub use_just: bool,
    pub strike: bool,
    /// C3: the attack button went up after a hold that dug nothing (`attack_dug` false, `HOLD_SECS` passed): the end of a charge.
    pub release: bool,
    pub inventory: bool,
    /// How many single items the drop bind throws this frame (playing only): one on the press, then 20 a second after a second held (`HoldRepeatHelper(20, 1)`).
    pub drop: u32,
    /// The drop-stack bind was pressed: the whole selected stack.
    pub drop_stack: bool,
    pub pause: bool,
    pub back: bool,
    /// The virtual mouse toggle was pressed on a screen (`lib.rs` flips the screen's `PadCfg::vm_screens` bit if it can).
    pub vm_toggle: bool,
    pub slot_delta: i32,
    pub slot_to: Option<usize>,
    /// Where the virtual mouse (`vmouse.rs`) put the cursor this frame (px), none if it stayed: the stick's chase, a snap, a d-pad step.
    pub cursor_to: Option<(f32, f32)>,
    pub click: Option<bool>,
    /// With `Pad::focus`: one step of the d-pad or the left stick as (x, y), y down; the first on the press, then repeating while held
    /// (`ScreenProcessor.handleComponentNavigation`).
    pub nav: Option<(i32, i32)>,
    /// In a GUI: the tab binds turn a page (-1 / +1; the options tabs), and the right stick scrolls (px, down is positive) like a finger dragging a list.
    pub page: i32,
    pub scroll: f32,
    /// In a GUI: `Some(false)` is the right click (the slot under the cursor is clicked for one: `inv_take_half`, `vmouse_rclick`, `gui_abstract_action_1`),
    /// `Some(true)` the shift click (quick move, or `drop_inventory`; `vmouse_shift_click`; `gui_abstract_action_2`).
    pub alt: Option<bool>,
}

/// The hold-repeat timers of the hotbar bumpers and the drop (`InGameInputHandler`'s constructor).
#[derive(Clone, Copy, Debug)]
struct Reps {
    next: HoldRepeat,
    prev: HoldRepeat,
    drop: HoldRepeat,
}

impl Default for Reps {
    fn default() -> Self {
        Self { next: HoldRepeat::new(HOTBAR_REPEAT.0, HOTBAR_REPEAT.1), prev: HoldRepeat::new(HOTBAR_REPEAT.0, HOTBAR_REPEAT.1), drop: HoldRepeat::new(DROP_REPEAT.0, DROP_REPEAT.1) }
    }
}

/// A turn that is still going (`flickAnimation`): `total` radians, `given` of them handed out, `t` seconds in.
#[derive(Clone, Copy, Debug)]
struct Flick {
    total: f32,
    given: f32,
    t: f32,
}

/// Live state of the pad(s): every controller feeds the same state, like Controlify's single active input mode.
#[derive(Default)]
pub struct Pad {
    keys: u16,
    hat: (f32, f32),
    lx: f32,
    ly: f32,
    rx: f32,
    ry: f32,
    lt: f32,
    rt: f32,
    /// A pad event has been seen: until then nothing here acts.
    connected: bool,
    pub device: i32,
    /// A keyboard or mouse was the last thing used (`lib.rs` sets it on their events and clears it on a finger).
    pub kbm: bool,
    /// Seconds since the last pad input (the cursor and the touch buttons follow it).
    idle: f32,
    prev: u32,
    /// Inputs held when the mode changed: ignored until released, so leaving a GUI with A held does not jump.
    suppress: u32,
    ui: bool,
    moving: bool,
    attack_t: f32,
    /// `dig_now` was called for the press now down: it digs from here to the release, which is no hit.
    attack_dug: bool,
    use_t: f32,
    /// The Controls screen is waiting for an input; the next press lands in `captured` and does nothing else.
    pub capture: bool,
    pub captured: Option<Input>,
    wheel_open: bool,
    wheel: Option<usize>,
    /// The open screen is a focus screen (Controlify's default for everything but a container: the virtual mouse is off): the d-pad and the
    /// left stick step the focus (`Out::nav`), there is no cursor. Set by `lib.rs` for the game menu's top page and the title menu.
    pub focus: bool,
    /// `VirtualMouseBehaviour` of the open screen (`lib.rs`).
    pub behaviour: Behaviour,
    /// The virtual mouse (`vmouse.rs`), used while a screen is open and `focus` is off.
    vm: VMouse,
    /// The open screen's snap points (`SnapPoint::widget` / `SnapPoint::slot`), px, rebuilt by `lib.rs` every frame.
    pub points: Vec<SnapPoint>,
    /// Where the cursor is now (`TouchUi::cursor_pos`), the surface size and the GUI scale (px per unit): `lib.rs` sets them before `update`.
    pub at: (f32, f32),
    pub bounds: (f32, f32),
    pub gs: f32,
    nav_dir: (i32, i32),
    nav_t: f32,
    hp: i32,
    /// Vibration (`rumble_fx.rs`): the effect queue, its wire to the phone's vibrators, the command `lib.rs` has yet to send, the 20 Hz
    /// accumulator and its clock in ms, and the effects that live as long as something goes on (`Holds`).
    fx: Manager,
    wire: Wire,
    cmd: Option<Cmd>,
    fx_acc: f32,
    fx_ms: u64,
    fx_key: u16,
    holds: Holds,
    /// A bow-like use is on (`player.isUsingItem()` with the bow, crossbow or spear animation): `lib.rs` sets it while a weapon's charge builds.
    pub aiming: bool,
    /// Toggle Sprint's state.
    sprint_on: bool,
    /// `dropRepeating`, and the three `HoldRepeatHelper`s of `InGameInputHandler`.
    dropping: bool,
    reps: Reps,
    flick: Option<Flick>,
}

/// The four directions of a stick as (up, down, left, right) in `Ax` order: a stick's axes, y down, split into the values 0..1 of its four inputs.
fn split(x: f32, y: f32) -> [f32; 4] {
    [(-y).max(0.0), y.max(0.0), (-x).max(0.0), x.max(0.0)]
}

/// The effects that run for as long as a state lasts, one slot each (Controlify keeps them in a field of the mixin that plays them).
#[derive(Default)]
struct Holds {
    eat: Option<Handle>,
    guard: Option<Handle>,
    bite: Option<Handle>,
    slow: Option<Handle>,
    /// The block being dug and the effect for it (`MultiPlayerGameModeMixin.blockBreakRumble`).
    dig: Option<((i32, i32, i32), Handle)>,
}

impl Pad {
    pub fn key(&mut self, b: Btn, down: bool) {
        self.connected = true;
        self.idle = 0.0;
        if down {
            self.keys |= b.bit();
        } else {
            self.keys &= !b.bit();
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn axes(&mut self, lx: f32, ly: f32, rx: f32, ry: f32, lt: f32, rt: f32, hat: (f32, f32)) {
        self.connected = true;
        (self.lx, self.ly, self.rx, self.ry, self.lt, self.rt, self.hat) = (lx, ly, rx, ry, lt, rt, hat);
    }

    /// A pad is in use: the touch buttons hide and the GUI cursor shows.
    pub fn active(&self) -> bool {
        self.connected && self.idle < 5.0
    }

    /// The eight stick directions after each stick's deadzone (`DeadzoneControllerStateView`: per axis, by the stick's group), `Ax` order.
    fn sticks(&self, cfg: &PadCfg) -> [f32; 8] {
        let (l, r) = (
            split(deadzone(self.lx, cfg.dz_left), deadzone(self.ly, cfg.dz_left)),
            split(deadzone(self.rx, cfg.dz_right), deadzone(self.ry, cfg.dz_right)),
        );
        [l[0], l[1], l[2], l[3], r[0], r[1], r[2], r[3]]
    }

    /// Inputs held now: keys, the hat as a d-pad, triggers and stick directions over the activation threshold (strictly: `StateAccess.digital`).
    fn digital(&self, cfg: &PadCfg, st: &[f32; 8]) -> u32 {
        let mut m = u32::from(self.keys);
        for (on, b) in [(self.hat.0 < -0.5, Btn::Left), (self.hat.0 > 0.5, Btn::Right), (self.hat.1 < -0.5, Btn::Up), (self.hat.1 > 0.5, Btn::Down), (self.lt > cfg.threshold, Btn::Lt), (self.rt > cfg.threshold, Btn::Rt)] {
            if on {
                m |= u32::from(b.bit());
            }
        }
        for a in Ax::ALL {
            if st[a as usize] > cfg.threshold {
                m |= Input::Stick(a).bit();
            }
        }
        m
    }

    /// The value 0..1 of an input (`Input.state`): a stick direction's, a trigger's pull, or 1 for a button held.
    fn analog_of(&self, i: Input, held: u32, st: &[f32; 8]) -> f32 {
        match i {
            Input::None => 0.0,
            Input::Stick(a) => st[a as usize],
            Input::Btn(b @ (Btn::Lt | Btn::Rt)) => {
                if self.suppress & i.bit() != 0 {
                    0.0
                } else if self.keys & b.bit() != 0 {
                    1.0
                } else if b == Btn::Lt {
                    self.lt
                } else {
                    self.rt
                }
            }
            Input::Btn(_) => f32::from(held & i.bit() != 0),
        }
    }

    /// The contexts of this frame (`BindContext.isApplicable`): see `bindings::ctx`.
    fn contexts(&self, ui: bool) -> u8 {
        if !ui {
            // the radial menu is a screen: while the hotbar wheel is open only its own binds act
            return if self.wheel_open { ctx::RADIAL } else { ctx::IN_GAME };
        }
        let mut c = ctx::SCREEN;
        if self.behaviour == Behaviour::CursorScroll {
            c |= ctx::CONTAINER;
        }
        if !self.focus && self.behaviour.has_cursor() {
            c |= ctx::VM_CURSOR;
            if self.behaviour == Behaviour::Enabled {
                c |= ctx::VM_COMPAT;
            }
        } else {
            c |= ctx::REGULAR;
        }
        c
    }

    /// `ControlifyApi.playRumbleEffect`.
    pub fn rumble_play(&mut self, (src, e): (RSource, Effect)) -> Handle {
        self.fx.play(src, e)
    }

    /// `RumbleSource.PLAYER` on damage (`damage/ClientPacketListenerMixin`). Call every frame with the current health.
    pub fn watch_health(&mut self, hp: i32) {
        if hp < self.hp {
            self.rumble_play(fx::damage_taken());
        }
        self.hp = hp;
    }

    /// The Test Vibration button: it ends when `rumble_screen` sees another screen.
    pub fn rumble_test(&mut self) {
        let e = self.fx.epoch();
        self.rumble_play(fx::test(e));
    }

    /// `finishOnScreenChange`: `key` says which screen is open (any number that differs between screens); another one ends the effects made
    /// with `until_epoch_changes` (the Test Vibration).
    pub fn rumble_screen(&mut self, key: u16) {
        if key != self.fx_key {
            self.fx_key = key;
            self.fx.set_epoch(self.fx.epoch().wrapping_add(1));
        }
    }

    /// `MinecraftMixin.clearRumbleEffects` (`Minecraft.disconnect`): leaving a world.
    pub fn rumble_clear(&mut self) {
        self.fx.clear_effects();
        self.holds = Holds::default();
    }

    /// An effect that lasts while `on`: started with `make` when it turns on, stopped when it turns off, and made again if it ended by itself.
    fn hold(fx: &mut Manager, slot: &mut Option<Handle>, on: bool, make: fn() -> (RSource, Effect)) {
        match (on, *slot) {
            (true, Some(h)) if !fx.is_finished(h) => {}
            (true, _) => {
                let (src, e) = make();
                *slot = Some(fx.play(src, e));
            }
            (false, Some(h)) => {
                fx.stop(h);
                *slot = None;
            }
            (false, None) => {}
        }
    }

    /// A meal in the mouth (`useitem` `EAT, DRINK`).
    pub fn rumble_eating(&mut self, on: bool) {
        Self::hold(&mut self.fx, &mut self.holds.eat, on, fx::eating);
    }

    /// A raised guard (`useitem` `BLOCK`).
    pub fn rumble_guard(&mut self, on: bool) {
        Self::hold(&mut self.fx, &mut self.holds.guard, on, fx::guard);
    }

    /// A fish on the line (`fishing/FishingHookMixin`).
    pub fn rumble_bite(&mut self, on: bool) {
        Self::hold(&mut self.fx, &mut self.holds.bite, on, fx::bite);
    }

    /// Walking through something that slows (`slowblock/LocalPlayerMixin`): `Some(length of the move vector)` while it does, `None` otherwise.
    /// Called every frame: it is the effect's heartbeat.
    pub fn rumble_slow(&mut self, moving: Option<f32>) {
        match (moving, self.holds.slow) {
            (Some(amount), Some(h)) if !self.fx.is_finished(h) => {
                self.fx.heartbeat(h);
                self.fx.set_param(h, amount);
            }
            (Some(amount), _) => {
                let (src, e) = fx::slow_block();
                let h = self.fx.play(src, e);
                self.fx.set_param(h, amount);
                self.holds.slow = Some(h);
            }
            (None, Some(h)) => {
                if !self.fx.is_finished(h) {
                    self.fx.stop(h);
                }
                self.holds.slow = None;
            }
            (None, None) => {}
        }
    }

    /// The block being dug this tick and how hard it is (`Block.defaultDestroyTime`), or `None` when nothing is dug: a new block starts the
    /// effect (`startRumble`: the old one stops first), `None` stops it (`stopRumble`). A block that breaks calls `rumble_dig(None)` right after.
    pub fn rumble_dig(&mut self, at: Option<((i32, i32, i32), f32)>) {
        match (at, self.holds.dig) {
            (Some((pos, _)), Some((cur, _))) if pos == cur => {}
            (Some((pos, hardness)), old) => {
                if let Some((_, h)) = old {
                    self.fx.stop(h);
                }
                let h = self.rumble_play(fx::block_break(hardness));
                self.holds.dig = Some((pos, h));
            }
            (None, Some((_, h))) => {
                self.fx.stop(h);
                self.holds.dig = None;
            }
            (None, None) => {}
        }
    }

    /// 20 Hz of vibration, from the wall-clock frame time (at most two ticks a frame). `silent`: nothing may buzz now (the game is paused outside the
    /// settings, or the keyboard and mouse are in use: `Controlify.tickActiveController`'s `setSilent`); the effects run on regardless.
    pub fn rumble_frame(&mut self, dt: f32, cfg: &PadCfg, silent: bool) {
        self.fx_acc = (self.fx_acc + dt).min(2.0 * TICK_SECS);
        while self.fx_acc >= TICK_SECS {
            self.fx_acc -= TICK_SECS;
            self.fx_ms += 50;
            let out = self.fx.tick(&cfg.vib, silent || !self.connected || self.kbm);
            if let Some(c) = self.wire.feed(out, self.fx_ms) {
                self.cmd = Some(c);
            }
        }
    }

    /// The command (`device`, `Cmd`) for the phone's vibrators, if this frame made one.
    pub fn take_haptics(&mut self) -> Option<(i32, Cmd)> {
        let c = self.cmd.take()?;
        self.connected.then_some((self.device, c))
    }

    /// The attack button has been down for `secs` (the Dig Delay: the arm swings, the dig may start: `dig_now`).
    pub fn resting(&self, secs: f32) -> bool {
        self.attack_t >= secs
    }

    /// The caller saw a block, not a mob, under the crosshair: the press now down digs from this frame, and its release is no hit.
    pub fn dig_now(&mut self, secs: f32) {
        self.attack_dug = self.resting(secs);
    }

    /// The d-pad / navigation binds as one of four directions: a step on the press, again after `NAV_FIRST` and then every `NAV_REPEAT` while it is held, so
    /// a long list is one hold, not a press per row. `d` is (left, right, up, down).
    fn nav_step(&mut self, dt: f32, d: (bool, bool, bool, bool)) -> Option<(i32, i32)> {
        let dp = (i32::from(d.1) - i32::from(d.0), i32::from(d.3) - i32::from(d.2));
        let dir = if dp.0 != 0 { (dp.0, 0) } else { (0, dp.1) };
        if dir != self.nav_dir {
            (self.nav_dir, self.nav_t) = (dir, NAV_FIRST);
            return (dir != (0, 0)).then_some(dir);
        }
        if dir == (0, 0) {
            return None;
        }
        self.nav_t -= dt;
        (self.nav_t <= 0.0).then(|| {
            self.nav_t += NAV_REPEAT;
            dir
        })
    }

    /// `handleRegularLook` / `handleFlickStick`: the look of the frame in radians (yaw right, pitch down) from the stick's look binds.
    fn look(&mut self, dt: f32, cfg: &PadCfg, impulse: (f32, f32), flicked: bool) -> (f32, f32) {
        let mut yaw = 0.0;
        if cfg.flick {
            // `handleFlickStick`: the angle of the stick, 0 up, clockwise, handed out over 8 ticks; a new flick finishes the one that is playing
            if flicked {
                if let Some(f) = self.flick.take() {
                    yaw += f.total - f.given;
                }
                let a = impulse.1.atan2(impulse.0) + std::f32::consts::FRAC_PI_2;
                let a = a.sin().atan2(a.cos()); // wrapDegrees: (-pi, pi]
                self.flick = Some(Flick { total: a, given: 0.0, t: 0.0 });
            }
            if let Some(mut f) = self.flick {
                f.t += dt;
                let p = if f.t >= FLICK_SECS { 1.0 } else { 1.0 - 2f32.powf(-10.0 * f.t / FLICK_SECS) };
                yaw += f.total * p - f.given;
                f.given = f.total * p;
                self.flick = (f.t < FLICK_SECS).then_some(f);
            }
            return (yaw, 0.0);
        }
        self.flick = None;
        let (mut x, mut y) = impulse;
        if cfg.invert_x {
            x = -x;
        }
        if cfg.invert_y {
            y = -y;
        }
        if cfg.lce {
            // LCE: a quadratic curve on each axis alone
            (x, y) = (x * x.abs() * LCE_LOOK, y * y.abs() * LCE_LOOK);
        } else {
            (x, y) = ease(x, y, cfg.curve);
        }
        if cfg.reduce_aim && self.aiming {
            let k = if cfg.lce { AIM_REDUCE_LCE } else { AIM_REDUCE };
            (x, y) = (x * k, y * k);
        }
        (x * cfg.look_h * LOOK_RAD_PER_SEC * dt, y * cfg.look_v * LOOK_RAD_PER_SEC * dt)
    }

    /// One frame. `ui`: a screen, the pause menu or a title menu is up (the sticks drive the cursor, not the player). `side`: the short
    /// side of the surface in px.
    pub fn update(&mut self, dt: f32, cfg: &PadCfg, ui: bool, side: f32) -> Out {
        let mut o = Out::default();
        if !self.connected {
            return o;
        }
        let st = self.sticks(cfg);
        let raw = self.digital(cfg, &st);
        if ui != self.ui {
            self.suppress |= raw;
            (self.ui, self.wheel_open, self.wheel, self.attack_t, self.attack_dug, self.use_t) = (ui, false, None, 0.0, false, 0.0);
            (self.nav_dir, self.dropping, self.flick) = ((0, 0), false, None);
            self.vm.reset();
        }
        self.suppress &= raw;
        let held = raw & !self.suppress;
        let was = self.prev;
        let (pressed, released) = (held & !was, was & !held);
        self.prev = held;
        let stick = [self.lx, self.ly, self.rx, self.ry].iter().any(|v| v.abs() > cfg.dz_left.min(cfg.dz_right));
        self.idle = if raw != 0 || stick { 0.0 } else { self.idle + dt };
        if self.capture {
            self.captured = Input::all().find(|i| pressed & i.bit() != 0).or(self.captured);
            return o;
        }
        let cx = self.contexts(ui);
        // A bind acts in the contexts it allows (`InputBindingImpl.suppressed`); an unbound one never does.
        let bit = |b: B| if b.def().ctx & cx != 0 { cfg.binds.bit(b) } else { 0 };
        let down = |b: B| held & bit(b) != 0;
        let just = |b: B| pressed & bit(b) != 0;
        let up = |b: B| released & bit(b) != 0;
        let was_down = |b: B| was & bit(b) != 0;
        // the value of every input this frame, taken up front so the closures below do not borrow the pad
        let ana: [f32; N_INPUTS] = std::array::from_fn(|k| self.analog_of(Input::from_code(k as u8), held, &st));
        let av = |b: B| match cfg.binds.get(b) {
            Input::None => 0.0,
            i if b.def().ctx & cx != 0 => ana[i.code() as usize],
            _ => 0.0,
        };
        if ui {
            let (bounds, gs) = (if self.bounds.0 > 0.0 { self.bounds } else { (side, side) }, if self.gs > 0.0 { self.gs } else { 1.0 });
            o.page = i32::from(just(B::GuiNextTab)) - i32::from(just(B::GuiPrevTab));
            // A container screen (`AbstractContainerScreenProcessor`): `inv_take_half` is the right click, `inv_quick_move` the shift click, or `drop_inventory` while the
            // cursor carries a stack (the caller tells them apart). A focus screen has two abstract actions (the title's and the game menu's Options, and the Quit
            // button's focus); a cursor screen has the virtual mouse's right and shift clicks (`handleCompatibilityBinds`).
            let container = self.behaviour == Behaviour::CursorScroll;
            o.alt = if container {
                if just(B::InvTakeHalf) {
                    Some(false)
                } else if just(B::InvQuickMove) || just(B::DropInventory) {
                    Some(true)
                } else {
                    None
                }
            } else if self.focus {
                if just(B::GuiAbstractAction1) {
                    Some(false)
                } else if just(B::GuiAbstractAction2) {
                    Some(true)
                } else {
                    None
                }
            } else if just(B::VmouseRclick) {
                Some(false)
            } else if just(B::VmouseShiftClick) {
                Some(true)
            } else {
                None
            };
            if self.focus {
                // `ScreenProcessor.handleComponentNavigation`: GUI_NAVI_* (the left stick over the activation threshold) and the d-pad, which is not rebindable.
                let pad = |b: Btn| held & u32::from(b.bit()) != 0;
                let nav = (down(B::GuiNaviLeft) || pad(Btn::Left), down(B::GuiNaviRight) || pad(Btn::Right), down(B::GuiNaviUp) || pad(Btn::Up), down(B::GuiNaviDown) || pad(Btn::Down));
                o.nav = self.nav_step(dt, nav);
                // UNVERIFIED stopgap, not in Controlify (a focus screen scrolls by moving the focus): the right stick drags the list
                o.scroll = (st[Ax::RightDown as usize] - st[Ax::RightUp as usize]) * side * CURSOR_SPEED * cfg.cursor * dt;
            } else if self.behaviour.has_cursor() {
                // `VirtualMouseHandler`: the move binds (cubed on their length) move the cursor, the snap binds jump, the scroll binds scroll.
                self.vm.sync(self.at);
                // With no snap point on the screen (the pages that have no focus map yet) the d-pad nudges the cursor, as this port did before:
                // UNVERIFIED, not in Controlify, which has nothing to snap to there either.
                let nudge = self.points.is_empty();
                let dirs = if nudge { [false; 4] } else { [down(B::VmouseSnapUp), down(B::VmouseSnapDown), down(B::VmouseSnapLeft), down(B::VmouseSnapRight)] };
                let stick = (av(B::VmouseMoveRight) - av(B::VmouseMoveLeft), av(B::VmouseMoveDown) - av(B::VmouseMoveUp));
                let scroll = av(B::VmouseScrollDown) - av(B::VmouseScrollUp);
                let vin = VIn { stick, scroll, dirs, sens: cfg.cursor, lce: cfg.lce, bounds, gs, scrolls: self.behaviour.is_default_or(Behaviour::Enabled) || container };
                let vo = self.vm.step(dt, &vin, &self.points);
                o.cursor_to = vo.to;
                o.scroll = vo.scroll_px;
                if nudge && (self.hat.0 != 0.0 || self.hat.1 != 0.0) {
                    let k = side * CURSOR_SPEED * cfg.cursor * dt * 0.4;
                    let (x, y) = (self.at.0 + self.hat.0.round() * k, self.at.1 + self.hat.1.round() * k);
                    o.cursor_to = Some((x.clamp(0.0, bounds.0 - 1.0), y.clamp(0.0, bounds.1 - 1.0)));
                }
            }
            // The click: `inv_select` in a container, `vmouse_lclick` on a cursor screen, `gui_press` on a focus screen.
            let sel = if container {
                B::InvSelect
            } else if self.focus {
                B::GuiPress
            } else {
                B::VmouseLclick
            };
            o.click = if just(sel) {
                Some(true)
            } else if up(sel) {
                Some(false)
            } else {
                None
            };
            o.back = just(B::GuiBack);
            // `pause` is `REGULAR_SCREEN` only: it is a focus screen's. The cursor pages that stand in for focus screens keep it as well (UNVERIFIED stopgap);
            // a container does not have it, B closes it.
            o.pause = !container && pressed & cfg.binds.bit(B::Pause) != 0;
            o.vm_toggle = just(B::VmouseToggle);
            return o;
        }
        // The hotbar wheel (`HOTBAR_SLOT_SELECT`'s radial menu): held, the radial axes pick a slot, let go selects it.
        let wheel_bind = down(B::HotbarItemSelectRadial);
        let (rax, ray) = (av(B::RadialAxisRight) - av(B::RadialAxisLeft), av(B::RadialAxisDown) - av(B::RadialAxisUp));
        if wheel_bind {
            self.wheel_open = true;
            if rax.hypot(ray) > cfg.threshold.max(0.05) {
                let a = ray.atan2(rax) + std::f32::consts::FRAC_PI_2; // 0 = up, clockwise
                let s = (a / (std::f32::consts::TAU / HOTBAR as f32)).round().rem_euclid(HOTBAR as f32) as usize;
                self.wheel = Some(s);
            }
        } else if self.wheel_open {
            self.wheel_open = false;
            o.slot_to = self.wheel.take();
        }
        if self.wheel_open {
            // a screen is open: the player stands (`ControllerPlayerMovement.tick`), and no other bind acts (`contexts`)
            if self.moving {
                o.mv = Some((0.0, 0.0));
            }
            self.moving = false;
            return o;
        }
        // Walking: the walk binds (`ControllerPlayerMovement`); the vector is limited to length 1, not normalised, so a half push is a slow walk.
        let (side_v, fwd) = (av(B::StrafeRight) - av(B::StrafeLeft), av(B::WalkForward) - av(B::WalkBackward));
        let l = side_v.hypot(fwd).max(1.0);
        let active = side_v != 0.0 || fwd != 0.0;
        if active || self.moving {
            o.mv = Some((fwd / l, side_v / l));
        }
        self.moving = active;
        let impulse = (av(B::LookRight) - av(B::LookLeft), av(B::LookDown) - av(B::LookUp));
        let flicked = just(B::LookUp) || just(B::LookDown) || just(B::LookLeft) || just(B::LookRight);
        o.look = self.look(dt, cfg, impulse, flicked);
        o.jump = down(B::Jump);
        o.zoom = down(B::Zoom);
        if cfg.toggle_sprint {
            self.sprint_on ^= just(B::Sprint);
        } else {
            self.sprint_on = down(B::Sprint);
        }
        (o.sprint, o.view, o.info) = (self.sprint_on, just(B::ChangePerspective), just(B::ToggleDebugMenu));
        (o.sneak_down, o.sneak_up) = (just(B::Sneak), up(B::Sneak));
        // Attack: the touch screen's look finger. Held past HOLD_SECS digs, a short press hits.
        o.attack = down(B::Attack);
        if o.attack {
            self.attack_t += dt;
            o.dig = self.attack_dug || self.attack_t >= HOLD_SECS;
        } else if up(B::Attack) {
            o.strike = !self.attack_dug && self.attack_t < HOLD_SECS; // the complement of `dig`: no press is neither
            o.release = !self.attack_dug && self.attack_t >= HOLD_SECS;
            (self.attack_t, self.attack_dug) = (0.0, false);
        }
        // Use: on the press, then 4 a second while held.
        if just(B::Use) {
            (o.place, self.use_t) = (true, 0.0);
        } else if down(B::Use) {
            self.use_t += dt;
            if self.use_t >= TAP_SECS {
                (o.place, self.use_t) = (true, self.use_t - TAP_SECS);
            }
        }
        (o.use_down, o.use_just) = (down(B::Use), just(B::Use));
        o.place |= o.strike;
        (o.inventory, o.pause) = (just(B::Inventory), just(B::Pause));
        // The hotbar: the bumpers repeat while held (`HoldRepeatHelper(10, 4)` each), the slot binds select.
        if self.reps.next.should_action(dt, down(B::NextSlot), was_down(B::NextSlot)) {
            self.reps.next.on_navigate();
            o.slot_delta += 1;
        }
        if self.reps.prev.should_action(dt, down(B::PrevSlot), was_down(B::PrevSlot)) {
            self.reps.prev.on_navigate();
            o.slot_delta -= 1;
        }
        for (i, b) in [B::HotbarSlot1, B::HotbarSlot2, B::HotbarSlot3, B::HotbarSlot4, B::HotbarSlot5, B::HotbarSlot6, B::HotbarSlot7, B::HotbarSlot8, B::HotbarSlot9].into_iter().enumerate() {
            if just(b) {
                o.slot_to = Some(i);
            }
        }
        // Drop: the stack on its own bind, else one item on the press and, held a second, 20 a second (`HoldRepeatHelper(20, 1)`).
        if just(B::DropStack) {
            o.drop_stack = true;
        } else {
            if just(B::Drop) {
                self.dropping = true;
            } else if up(B::Drop) {
                self.dropping = false;
            }
            if self.dropping && self.reps.drop.should_action(dt, down(B::Drop), was_down(B::Drop)) {
                self.reps.drop.on_navigate();
                o.drop += 1;
            }
        }
        o
    }

    /// The radial hotbar and, in a GUI, Controlify's cursor (`vmouse::draw_cursor`) drawn over `cursor` (px). `_cap` is kept for the callers:
    /// the old cursor shrank over a slot, Controlify's does not.
    pub fn draw(&self, v: &mut Vec<HudVertex>, w: f32, h: f32, slot: usize, cursor: Option<(f32, f32)>, _cap: f32) {
        if self.wheel_open {
            let (cx, cy, r) = (w / 2.0, h / 2.0, h.min(w) * 0.28);
            let b = h.min(w) * 0.07;
            HudPipeline::push_quad(v, cx - r - b * 1.5, cy - r - b * 1.5, (r + b * 1.5) * 2.0, (r + b * 1.5) * 2.0, [0.0, 0.0, 0.0, 0.35]);
            for i in 0..HOTBAR {
                let a = i as f32 * std::f32::consts::TAU / HOTBAR as f32 - std::f32::consts::FRAC_PI_2;
                let (x, y) = (cx + r * a.cos() - b / 2.0, cy + r * a.sin() - b / 2.0);
                let col = if self.wheel == Some(i) { [1.0, 0.9, 0.3, 0.95] } else if slot == i { [0.3, 0.5, 0.9, 0.8] } else { [0.2, 0.2, 0.2, 0.8] };
                HudPipeline::push_quad(v, x, y, b, b, col);
                HudPipeline::push_number(v, x + b * 0.25, y + b * 0.2, b * 0.6, i as u32 + 1, [1.0; 4]);
            }
        }
        if let Some((x, y)) = cursor {
            // `renderVirtualMouse`: Controlify's own cursor texture, 16 GUI units wide (no cap: it is what the Java draws over a slot too).
            let s = if self.gs > 0.0 { self.gs } else { (h.min(w) * 0.006).max(2.0) * 2.0 };
            super::vmouse::draw_cursor(v, x, y, s);
        }
    }

    /// The contextual button guide (`guide.rs`, Controlify's `GuideRenderer`) for `dom`: the in-game one (two columns in the top corners or
    /// the bottom ones, with a contrast box behind the text) or the container's (bottom, text with its shadow). `s` is px per GUI unit; the
    /// guide's own scale is `s * Guide Size`, in whole pixels so the font stays crisp. Nothing while no pad is in use or the guide is off.
    pub fn draw_guide(&self, v: &mut Vec<HudVertex>, cfg: &PadCfg, w: f32, h: f32, s: f32, dom: Domain) {
        let container = matches!(dom, Domain::Container(_));
        if !self.active() || (container && !cfg.screen_guides) || (!container && !cfg.hints) {
            return;
        }
        let ty = cfg.style as usize % TYPES.len();
        let input = |id: &str| cfg.input_of(id);
        // worked out afresh every frame (about 90 rules): the screen draws with `&self`, so nothing is kept between frames
        let mut g = if container { Guide::container() } else { Guide::in_game() };
        g.update(dom, Verbosity::from_index(cfg.verbosity), ty, &input);
        let gs = (s * cfg.hint_size).round().max(1.0);
        g.draw(v, w, h, gs, ty, container || cfg.layout == 1, !container);
    }

    /// The touch buttons (everything of the HUD but the hotbar) are not drawn: a pad, keyboard or mouse is in hand. They still answer a finger.
    pub fn hud_hidden(&self) -> bool {
        self.active() || self.kbm
    }

    /// The cursor is drawn: a pad is in use and the screen is not a focus screen (`focus`) nor one that asks for none (`Behaviour::Disabled`).
    pub fn shows_cursor(&self) -> bool {
        self.active() && !self.focus && self.behaviour.has_cursor()
    }

    // ---- Android ----

    /// A key event from a pad. True when it was one (the caller reports it handled, so Android does not turn B into Back).
    pub fn on_key(&mut self, k: &KeyEvent) -> bool {
        let Some(b) = btn_of(k.key_code()) else { return false };
        match k.action() {
            KeyAction::Down => self.key(b, true),
            KeyAction::Up => self.key(b, false),
            _ => {}
        }
        self.device = k.device_id();
        true
    }

    pub fn on_motion(&mut self, m: &MotionEvent) {
        let p = m.pointer_at_index(0);
        let a = |x: Axis| p.axis_value(x);
        self.axes(a(Axis::X), a(Axis::Y), a(Axis::Z), a(Axis::Rz), a(Axis::Ltrigger).max(a(Axis::Brake)), a(Axis::Rtrigger).max(a(Axis::Gas)), (a(Axis::HatX), a(Axis::HatY)));
        self.device = m.device_id();
    }
}

/// A joystick or game-pad motion event (everything else is a finger).
pub fn is_pad_motion(m: &MotionEvent) -> bool {
    matches!(m.source(), Source::Joystick | Source::Gamepad)
}

fn btn_of(k: Keycode) -> Option<Btn> {
    Some(match k {
        Keycode::ButtonA => Btn::A,
        Keycode::ButtonB => Btn::B,
        Keycode::ButtonX => Btn::X,
        Keycode::ButtonY => Btn::Y,
        Keycode::ButtonL1 => Btn::Lb,
        Keycode::ButtonR1 => Btn::Rb,
        Keycode::ButtonL2 => Btn::Lt,
        Keycode::ButtonR2 => Btn::Rt,
        Keycode::ButtonThumbl => Btn::L3,
        Keycode::ButtonThumbr => Btn::R3,
        Keycode::ButtonStart => Btn::Start,
        Keycode::ButtonSelect => Btn::Back,
        Keycode::DpadUp => Btn::Up,
        Keycode::DpadDown => Btn::Down,
        Keycode::DpadLeft => Btn::Left,
        Keycode::DpadRight => Btn::Right,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::touch_ui::SWING_SECS;

    const DT: f32 = 1.0 / 60.0;

    fn pad() -> (Pad, PadCfg) {
        let mut p = Pad::default();
        p.key(Btn::Start, false); // a pad is connected
        (p, PadCfg::default())
    }

    fn idle(p: &mut Pad) {
        p.axes(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, (0.0, 0.0));
    }

    #[test]
    fn deadzone_and_curves() {
        assert_eq!(deadzone(0.1, 0.12), 0.0);
        assert!((deadzone(1.0, 0.12) - 1.0).abs() < 1e-6 && deadzone(-1.0, 0.12) < 0.0);
        let (x, y) = ease(0.6, 0.8, 2); // length 1 keeps its direction
        assert!((x - 0.6).abs() < 1e-5 && (y - 0.8).abs() < 1e-5);
        let (x, _) = ease(0.5, 0.0, 2);
        assert!((x - 0.25).abs() < 1e-5);
        assert!(deadzone(0.5, 1.0).is_finite(), "a slider at 100% divides by nothing");
    }

    #[test]
    fn each_stick_has_its_own_deadzone() {
        let (mut p, mut c) = pad();
        c.dz_left = 0.5;
        c.dz_right = 0.05;
        p.axes(0.4, 0.0, 0.4, 0.0, 0.0, 0.0, (0.0, 0.0));
        let o = p.update(DT, &c, false, 1000.0);
        assert!(o.mv.is_none(), "0.4 is inside the left stick's 50%");
        assert!(o.look.0 > 0.0, "and outside the right stick's 5%");
    }

    #[test]
    fn sprint_is_toggled_by_default_and_held_when_asked() {
        let (mut p, mut c) = pad();
        p.key(Btn::L3, true);
        assert!(p.update(DT, &c, false, 1000.0).sprint);
        p.key(Btn::L3, false);
        assert!(p.update(DT, &c, false, 1000.0).sprint, "Toggle Sprint: it stays on");
        p.key(Btn::L3, true);
        assert!(!p.update(DT, &c, false, 1000.0).sprint, "and the next press ends it");
        p.key(Btn::L3, false);
        c.toggle_sprint = false;
        p.update(DT, &c, false, 1000.0);
        p.key(Btn::L3, true);
        assert!(p.update(DT, &c, false, 1000.0).sprint);
        p.key(Btn::L3, false);
        assert!(!p.update(DT, &c, false, 1000.0).sprint, "held: it ends with the button");
    }

    #[test]
    fn walks_looks_and_jumps() {
        let (mut p, c) = pad();
        p.axes(0.0, -1.0, 1.0, 0.0, 0.0, 0.0, (0.0, 0.0));
        p.key(Btn::A, true);
        let o = p.update(DT, &c, false, 1000.0);
        assert_eq!(o.mv, Some((1.0, 0.0)), "up is forward");
        assert!(o.look.0 > 0.0 && o.look.1 == 0.0 && o.jump);
        idle(&mut p);
        assert_eq!(p.update(DT, &c, false, 1000.0).mv, Some((0.0, 0.0)), "one zero when the stick returns");
        assert_eq!(p.update(DT, &c, false, 1000.0).mv, None);
    }

    #[test]
    fn a_bind_can_be_an_axis_or_a_button_either_way() {
        let (mut p, mut c) = pad();
        // forward on a face button: a full push forward
        c.binds.set(B::WalkForward, Input::Btn(Btn::X));
        p.key(Btn::X, true);
        assert_eq!(p.update(DT, &c, false, 1000.0).mv, Some((1.0, 0.0)));
        p.key(Btn::X, false);
        p.update(DT, &c, false, 1000.0);
        // jump on the right stick pushed up: a digital bind on an axis (over the threshold)
        c.binds.set(B::Jump, Input::Stick(Ax::RightUp));
        p.axes(0.0, 0.0, 0.0, -0.3, 0.0, 0.0, (0.0, 0.0));
        assert!(!p.update(DT, &c, false, 1000.0).jump, "0.3 is under the threshold");
        p.axes(0.0, 0.0, 0.0, -1.0, 0.0, 0.0, (0.0, 0.0));
        assert!(p.update(DT, &c, false, 1000.0).jump);
        // an unbound bind does nothing
        c.binds.set(B::Jump, Input::None);
        p.key(Btn::A, true);
        assert!(!p.update(DT, &c, false, 1000.0).jump);
    }

    #[test]
    fn lce_look_is_quadratic_per_axis_and_slower() {
        let (mut p, mut c) = pad();
        p.axes(0.0, 0.0, 1.0, 0.0, 0.0, 0.0, (0.0, 0.0));
        let regular = p.update(DT, &c, false, 1000.0).look.0;
        c.lce = true;
        let lce = p.update(DT, &c, false, 1000.0).look.0;
        assert!((lce / regular - 0.75).abs() < 1e-4, "7.5 degrees a tick against 10: {}", lce / regular);
        // half a push on one axis: x * |x| * 0.75, with no curve on the length
        p.axes(0.0, 0.0, 0.5 * (1.0 - 0.12) + 0.12, 0.0, 0.0, 0.0, (0.0, 0.0));
        let half = p.update(DT, &c, false, 1000.0).look.0;
        assert!((half / lce - 0.25).abs() < 1e-3, "{}", half / lce);
    }

    #[test]
    fn aiming_slows_the_look_when_asked() {
        let (mut p, mut c) = pad();
        p.axes(0.0, 0.0, 1.0, 0.0, 0.0, 0.0, (0.0, 0.0));
        let free = p.update(DT, &c, false, 1000.0).look.0;
        p.aiming = true;
        let aimed = p.update(DT, &c, false, 1000.0).look.0;
        assert!((aimed / free - 0.6).abs() < 1e-4);
        c.lce = true;
        let lce_aimed = p.update(DT, &c, false, 1000.0).look.0;
        assert!((lce_aimed / free - 0.75 * 0.15).abs() < 1e-4);
        c.lce = false;
        c.reduce_aim = false;
        assert!((p.update(DT, &c, false, 1000.0).look.0 / free - 1.0).abs() < 1e-4);
    }

    #[test]
    fn flick_stick_turns_to_where_the_stick_points_over_eight_ticks() {
        let (mut p, mut c) = pad();
        c.flick = true;
        p.axes(0.0, 0.0, 1.0, 0.0, 0.0, 0.0, (0.0, 0.0)); // right: a quarter turn
        let mut yaw = p.update(DT, &c, false, 1000.0).look.0;
        assert!(yaw > 0.0 && yaw < std::f32::consts::FRAC_PI_2, "it starts, it does not jump");
        for _ in 0..40 {
            yaw += p.update(DT, &c, false, 1000.0).look.0;
        }
        assert!((yaw - std::f32::consts::FRAC_PI_2).abs() < 1e-3, "a quarter turn right: {yaw}");
        assert_eq!(p.update(DT, &c, false, 1000.0).look, (0.0, 0.0), "holding the stick turns nothing more");
        idle(&mut p);
        p.update(DT, &c, false, 1000.0);
        p.axes(0.0, 0.0, 0.0, 1.0, 0.0, 0.0, (0.0, 0.0)); // down: about face
        let mut yaw = 0.0;
        for _ in 0..40 {
            yaw += p.update(DT, &c, false, 1000.0).look.0;
        }
        assert!((yaw.abs() - std::f32::consts::PI).abs() < 1e-3, "{yaw}");
    }

    #[test]
    fn attack_taps_hit_and_holds_dig() {
        let (mut p, c) = pad();
        p.axes(0.0, 0.0, 0.0, 0.0, 0.0, 1.0, (0.0, 0.0)); // right trigger
        let o = p.update(DT, &c, false, 1000.0);
        assert!(!o.dig && o.attack, "the arm may swing from the first frame, the dig waits");
        idle(&mut p);
        let o = p.update(DT, &c, false, 1000.0);
        assert!(o.strike && o.place, "a short press is a hit");
        p.axes(0.0, 0.0, 0.0, 0.0, 0.0, 1.0, (0.0, 0.0));
        let mut dug = false;
        for _ in 0..40 {
            dug |= p.update(DT, &c, false, 1000.0).dig;
        }
        assert!(dug);
        idle(&mut p);
        assert!(!p.update(DT, &c, false, 1000.0).strike, "a hold is not a hit");
    }

    #[test]
    fn dig_now_digs_a_held_attack_early_and_its_release_is_no_hit() {
        let (mut p, c) = pad();
        p.axes(0.0, 0.0, 0.0, 0.0, 0.0, 1.0, (0.0, 0.0));
        p.update(DT, &c, false, 1000.0);
        p.dig_now(SWING_SECS);
        assert!(!p.update(DT, &c, false, 1000.0).dig, "held under SWING_SECS: not resting, no early dig");
        while !p.resting(SWING_SECS) {
            p.update(DT, &c, false, 1000.0);
        }
        assert!(!p.update(DT, &c, false, 1000.0).dig, "resting alone does not dig");
        p.dig_now(SWING_SECS);
        assert!(p.update(DT, &c, false, 1000.0).dig && SWING_SECS < HOLD_SECS);
        idle(&mut p);
        assert!(!p.update(DT, &c, false, 1000.0).strike, "an early dig is not a hit");
    }

    #[test]
    fn button_held_across_a_gui_does_not_jump() {
        let (mut p, c) = pad();
        p.key(Btn::A, true);
        p.update(DT, &c, true, 1000.0);
        assert!(!p.update(DT, &c, false, 1000.0).jump, "A is still down from the GUI click");
        p.key(Btn::A, false);
        p.update(DT, &c, false, 1000.0);
        p.key(Btn::A, true);
        assert!(p.update(DT, &c, false, 1000.0).jump);
    }

    #[test]
    fn gui_cursor_click_and_wheel() {
        let (mut p, c) = pad();
        p.update(DT, &c, true, 1000.0); // the mode change: what is held now is ignored
        p.axes(1.0, 0.0, 0.0, 0.0, 0.0, 0.0, (0.0, 0.0));
        p.key(Btn::A, true);
        let o = p.update(DT, &c, true, 1000.0);
        assert!(o.cursor_to.is_some_and(|c| c.0 > 0.0) && o.click == Some(true), "vmouse_lclick on a cursor screen");
        p.key(Btn::A, false);
        assert_eq!(p.update(DT, &c, true, 1000.0).click, Some(false));
        // wheel: hold D-Up, push the stick right (a quarter turn: slot index 2), release the button.
        idle(&mut p);
        p.update(DT, &c, false, 1000.0); // back to the world: the mode change
        p.axes(0.0, 0.0, 1.0, 0.0, 0.0, 0.0, (0.0, 0.0));
        p.key(Btn::Up, true);
        p.update(DT, &c, false, 1000.0); // opens the wheel
        p.update(DT, &c, false, 1000.0); // its axes act from here on
        p.key(Btn::Up, false);
        assert_eq!(p.update(DT, &c, false, 1000.0).slot_to, Some(2));
    }

    #[test]
    fn while_the_wheel_is_open_only_it_acts_and_the_player_stands() {
        let (mut p, c) = pad();
        p.axes(0.0, -1.0, 0.0, 0.0, 0.0, 0.0, (0.0, 0.0));
        assert!(p.update(DT, &c, false, 1000.0).mv.is_some());
        p.key(Btn::Up, true);
        let o = p.update(DT, &c, false, 1000.0);
        assert_eq!(o.mv, Some((0.0, 0.0)), "stops walking");
        p.key(Btn::A, true);
        assert!(!p.update(DT, &c, false, 1000.0).jump, "the in-game binds are off while the radial is open");
    }

    #[test]
    fn hotbar_slot_binds_select_and_the_bumpers_repeat() {
        let (mut p, mut c) = pad();
        c.binds.set(B::HotbarSlot4, Input::Btn(Btn::X));
        p.key(Btn::X, true);
        assert_eq!(p.update(DT, &c, false, 1000.0).slot_to, Some(3));
        p.key(Btn::X, false);
        p.key(Btn::Rb, true);
        assert_eq!(p.update(DT, &c, false, 1000.0).slot_delta, 1, "a step on the press");
        let mut steps = 0;
        for _ in 0..60 {
            steps += p.update(DT, &c, false, 1000.0).slot_delta;
        }
        // after 10 ticks (0.5 s) one step every 4 ticks (0.2 s): about 3 in the next second
        assert!((2..=4).contains(&steps), "{steps}");
        p.key(Btn::Rb, false);
        assert_eq!(p.update(DT, &c, false, 1000.0).slot_delta, 0);
        p.key(Btn::Lb, true);
        assert_eq!(p.update(DT, &c, false, 1000.0).slot_delta, -1);
    }

    #[test]
    fn focus_steps_once_on_a_press_then_repeats_while_held() {
        let (mut p, c) = pad();
        p.focus = true;
        p.update(DT, &c, true, 1000.0); // the mode change: what is held now is ignored
        p.key(Btn::Down, true);
        let o = p.update(DT, &c, true, 1000.0);
        assert_eq!((o.nav, o.cursor_to), (Some((0, 1)), None), "a step, and there is no cursor to push");
        let mut steps = 0;
        for _ in 0..60 {
            steps += i32::from(p.update(DT, &c, true, 1000.0).nav.is_some());
        }
        assert!((4..=6).contains(&steps), "after the first delay a held d-pad steps every {NAV_REPEAT} s: {steps} in 1 s");
        p.key(Btn::Down, false);
        assert_eq!(p.update(DT, &c, true, 1000.0).nav, None);
        // The left stick steps as well (GUI_NAVI_*, over the activation threshold); a half push does not.
        p.axes(0.4, 0.0, 0.0, 0.0, 0.0, 0.0, (0.0, 0.0));
        assert!(p.update(DT, &c, true, 1000.0).nav.is_none(), "under the threshold");
        p.axes(1.0, 0.0, 0.0, 0.0, 0.0, 0.0, (0.0, 0.0));
        let o = p.update(DT, &c, true, 1000.0);
        assert_eq!((o.nav, o.cursor_to), (Some((1, 0)), None));
        assert!(!p.shows_cursor(), "a focus screen draws no cursor");
    }

    #[test]
    fn a_focus_screens_a_press_is_gui_press_and_its_actions_are_the_abstract_ones() {
        let (mut p, mut c) = pad();
        p.focus = true;
        p.update(DT, &c, true, 1000.0);
        p.key(Btn::A, true);
        assert_eq!(p.update(DT, &c, true, 1000.0).click, Some(true));
        p.key(Btn::A, false);
        p.update(DT, &c, true, 1000.0);
        p.key(Btn::X, true);
        assert_eq!(p.update(DT, &c, true, 1000.0).alt, Some(false));
        p.key(Btn::X, false);
        p.key(Btn::Y, true);
        assert_eq!(p.update(DT, &c, true, 1000.0).alt, Some(true));
        p.key(Btn::Y, false);
        // gui_press moved off A: A does nothing there any more
        c.binds.set(B::GuiPress, Input::Btn(Btn::B));
        p.key(Btn::A, true);
        assert_eq!(p.update(DT, &c, true, 1000.0).click, None);
    }

    #[test]
    fn the_d_pad_snaps_the_virtual_mouse_between_points_and_the_stick_moves_it() {
        let (mut p, c) = pad();
        p.bounds = (1000.0, 600.0);
        p.gs = 3.0;
        p.at = (100.0, 100.0);
        p.points = vec![SnapPoint { x: 100.0, y: 100.0, range: 40.0 }, SnapPoint { x: 200.0, y: 100.0, range: 40.0 }];
        p.behaviour = Behaviour::CursorScroll;
        p.update(DT, &c, true, 1000.0); // the mode change
        p.key(Btn::Right, true);
        let o = p.update(DT, &c, true, 1000.0);
        assert_eq!(o.cursor_to, Some((200.0, 100.0)), "a press snaps to the neighbour");
        assert!(o.nav.is_none(), "snapping is not a focus step");
        p.key(Btn::Right, false);
        p.at = (200.0, 100.0);
        p.axes(1.0, 0.0, 0.0, 0.0, 0.0, 0.0, (0.0, 0.0));
        let o = p.update(DT, &c, true, 1000.0);
        assert!(o.cursor_to.is_some_and(|c| c.0 > 200.0), "the stick pushes the cursor");
        assert!(p.shows_cursor() || !p.active());
    }

    #[test]
    fn without_snap_points_the_d_pad_still_nudges_the_cursor() {
        let (mut p, c) = pad();
        p.bounds = (1000.0, 600.0);
        p.at = (500.0, 300.0);
        p.update(DT, &c, true, 1000.0);
        p.axes(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, (1.0, 0.0));
        let o = p.update(DT, &c, true, 1000.0);
        assert!(o.nav.is_none() && o.cursor_to.is_some_and(|c| c.0 > 500.0));
    }

    #[test]
    fn the_tab_binds_page_and_the_right_stick_scrolls_in_a_gui() {
        let (mut p, c) = pad();
        p.update(DT, &c, true, 1000.0);
        p.key(Btn::Rb, true);
        assert_eq!(p.update(DT, &c, true, 1000.0).page, 1);
        assert_eq!(p.update(DT, &c, true, 1000.0).page, 0, "a held bumper turns one page");
        p.key(Btn::Rb, false);
        p.key(Btn::Lb, true);
        assert_eq!(p.update(DT, &c, true, 1000.0).page, -1);
        p.axes(0.0, 0.0, 0.0, 1.0, 0.0, 0.0, (0.0, 0.0));
        let o = p.update(DT, &c, true, 1000.0);
        assert!(o.scroll > 0.0 && o.cursor_to.is_none(), "the right stick scrolls, it does not move the cursor");
    }

    #[test]
    fn x_is_a_one_click_and_y_a_quick_move_in_a_container_screen() {
        let (mut p, mut c) = pad();
        p.behaviour = Behaviour::CursorScroll;
        p.update(DT, &c, true, 1000.0);
        p.key(Btn::X, true);
        assert_eq!(p.update(DT, &c, true, 1000.0).alt, Some(false), "inv_take_half");
        assert_eq!(p.update(DT, &c, true, 1000.0).alt, None, "a held button acts once");
        p.key(Btn::X, false);
        p.key(Btn::Y, true);
        let o = p.update(DT, &c, true, 1000.0);
        assert_eq!(o.alt, Some(true), "inv_quick_move / drop_inventory");
        assert!(!o.inventory, "the inventory bind (Y) does not close a container screen");
        p.key(Btn::Y, false);
        p.axes(0.0, 0.0, 0.0, 0.0, 0.0, 1.0, (0.0, 0.0));
        assert_eq!(p.update(DT, &c, true, 1000.0).alt, None, "there is no trigger action");
        idle(&mut p);
        p.update(DT, &c, true, 1000.0);
        // rebound: take half on B, and B is back too (both act: Controlify lets a shared input do both)
        c.binds.set(B::InvTakeHalf, Input::Btn(Btn::B));
        p.key(Btn::B, true);
        let o = p.update(DT, &c, true, 1000.0);
        assert!(o.alt == Some(false) && o.back);
        // Start does not close a container (`pause` is a regular screen's); B does
        p.key(Btn::B, false);
        p.key(Btn::Start, true);
        assert!(!p.update(DT, &c, true, 1000.0).pause);
    }

    #[test]
    fn the_virtual_mouse_toggle_is_reported_on_screens() {
        let (mut p, c) = pad();
        p.update(DT, &c, true, 1000.0);
        p.key(Btn::Back, true);
        assert!(p.update(DT, &c, true, 1000.0).vm_toggle);
        p.key(Btn::Back, false);
        p.update(DT, &c, false, 1000.0);
        p.key(Btn::Back, true);
        let o = p.update(DT, &c, false, 1000.0);
        assert!(!o.vm_toggle && o.view, "in the world Back is the perspective");
    }

    #[test]
    fn view_and_info_follow_their_binds() {
        let (mut p, mut c) = pad();
        p.key(Btn::Back, true);
        let o = p.update(DT, &c, false, 1000.0);
        assert!(o.view && !o.info);
        assert!(!p.update(DT, &c, false, 1000.0).view, "one press, one switch");
        p.key(Btn::Back, false);
        p.key(Btn::Left, true);
        assert!(p.update(DT, &c, false, 1000.0).info);
        p.key(Btn::Left, false);
        c.binds.set(B::ChangePerspective, Input::Btn(Btn::X));
        p.key(Btn::X, true);
        assert!(p.update(DT, &c, false, 1000.0).view);
    }

    #[test]
    fn keyboard_or_mouse_hides_the_touch_buttons() {
        let (mut p, _) = pad();
        assert!(!p.hud_hidden());
        p.kbm = true;
        assert!(p.hud_hidden());
        p.kbm = false;
        p.key(Btn::A, true);
        assert!(p.hud_hidden(), "a pad hides them too");
    }

    #[test]
    fn capture_swallows_the_press_and_takes_a_stick_direction_too() {
        let (mut p, c) = pad();
        p.capture = true;
        p.key(Btn::Start, true);
        let o = p.update(DT, &c, false, 1000.0);
        assert!(!o.pause && p.captured == Some(Input::Btn(Btn::Start)));
        p.captured = None;
        p.key(Btn::Start, false);
        p.axes(0.0, 0.0, 0.0, -1.0, 0.0, 0.0, (0.0, 0.0));
        p.update(DT, &c, false, 1000.0);
        assert_eq!(p.captured, Some(Input::Stick(Ax::RightUp)));
    }

    #[test]
    fn every_input_has_a_glyph_in_every_controller_type() {
        use super::super::glyph_map::{CELL, INPUTS};
        for i in Input::all() {
            let g = i.glyph().unwrap();
            let k = INPUTS.iter().position(|n| *n == g).unwrap_or_else(|| panic!("{g} is not a Controlify input"));
            for (ty, row) in CELL.iter().enumerate() {
                assert!(row[k] >= 0, "{} has no glyph in type {ty}", i.name());
            }
        }
        let inputs: Vec<_> = Input::all().filter_map(Input::glyph).collect();
        assert!(inputs.iter().enumerate().all(|(i, a)| inputs[..i].iter().all(|b| a != b)), "one glyph an input");
    }

    #[test]
    fn bindings_resolve_to_the_inputs_they_are_on() {
        let (_, mut c) = pad();
        assert_eq!(c.input_of("jump"), Some(Btn::A.input()));
        c.binds.set(B::Jump, Input::Btn(Btn::X));
        assert_eq!(c.input_of("jump"), Some(Btn::X.input()), "follows a rebind");
        c.binds.set(B::Sneak, Input::Stick(Ax::LeftDown));
        assert_eq!(c.input_of("sneak"), Some("axis/left_stick_down"));
        assert_eq!(c.input_of("inv_quick_move"), Some(Btn::Y.input()));
        assert_eq!(c.input_of("inv_take_half"), Some(Btn::X.input()));
        assert_eq!(c.input_of("radial_menu"), Some(Btn::Up.input()), "the hotbar wheel stands in for it until the radial menu");
        assert_eq!(c.input_of("swap_hands"), None, "not live: b1.7.3 has no off hand");
        assert_eq!(c.input_of("pick_block"), None);
        assert_eq!(c.input_of("drop_stack"), None, "unbound by default");
    }

    #[test]
    fn the_guide_draws_only_while_a_pad_is_in_use_and_follows_its_settings() {
        use super::super::guide::{Domain, InGame, Me};
        let (mut p, mut c) = pad();
        let g = InGame { me: Me { on_ground: true, ..Me::default() }, ..InGame::default() };
        let n = |p: &Pad, c: &PadCfg| {
            let mut v = Vec::new();
            p.draw_guide(&mut v, c, 1280.0, 720.0, 3.0, Domain::InGame(&g));
            v.len()
        };
        assert!(n(&p, &c) > 0);
        let full = n(&p, &c);
        c.verbosity = 2;
        assert!(n(&p, &c) < full, "Minimal says less");
        c.verbosity = 0;
        c.hints = false;
        assert_eq!(n(&p, &c), 0, "off draws nothing");
        c.hints = true;
        p.idle = 9.0;
        assert_eq!(n(&p, &c), 0, "no pad in use: no guide");
    }

    #[test]
    fn drop_is_a_press_then_twenty_a_second_after_a_second_held() {
        let (mut p, mut c) = pad();
        p.key(Btn::Down, true);
        assert_eq!(p.update(DT, &c, false, 1000.0).drop, 1, "D-Down drops by default");
        let mut early = 0;
        for _ in 0..50 {
            early += p.update(DT, &c, false, 1000.0).drop;
        }
        assert_eq!(early, 0, "nothing for the first second");
        let mut later = 0;
        for _ in 0..60 {
            later += p.update(DT, &c, false, 1000.0).drop;
        }
        assert!((15..=25).contains(&later), "then about 20 a second: {later}");
        p.key(Btn::Down, false);
        assert_eq!(p.update(DT, &c, false, 1000.0).drop, 0);
        // the stack has its own bind
        c.binds.set(B::DropStack, Input::Btn(Btn::X));
        p.key(Btn::X, true);
        let o = p.update(DT, &c, false, 1000.0);
        assert!(o.drop_stack && o.drop == 0);
        p.key(Btn::X, false);
        p.update(DT, &c, true, 1000.0);
        p.key(Btn::Down, true);
        p.update(DT, &c, true, 1000.0);
        assert_eq!(p.update(DT, &c, true, 1000.0).drop, 0, "not in a GUI");
    }

    #[test]
    fn config_round_trips() {
        let mut c = PadCfg::default();
        (c.hints, c.style) = (false, 2);
        c.binds.set(B::Jump, Input::Btn(Btn::X));
        c.binds.set(B::Sprint, Input::Stick(Ax::LeftDown));
        (c.curve, c.invert_y, c.toggle_sneak, c.toggle_sprint, c.lce, c.flick, c.reduce_aim) = (4, true, false, false, true, true, false);
        c.vm_screens = VM_TITLE | VM_PAUSE;
        c.set_knob(Knob::LookH as usize, 0.26);
        c.set_knob(Knob::DzRight as usize, 0.3);
        let mut t = String::new();
        c.save_into(&mut t);
        let mut back = PadCfg::default();
        for l in t.lines() {
            let (k, v) = l.split_once(':').unwrap();
            back.load(k, v);
        }
        assert_eq!(back, c);
        assert!((back.look_h - 0.6).abs() < 1e-4, "{}", back.look_h);
    }

    #[test]
    fn knobs_snap_to_controlifys_steps_and_ranges() {
        let mut c = PadCfg::default();
        c.set_knob(Knob::LookH as usize, 0.0);
        assert!((c.look_h - 0.1).abs() < 1e-6, "10% is the lowest look speed");
        c.set_knob(Knob::LookH as usize, 1.0);
        assert!((c.look_h - 2.0).abs() < 1e-6);
        c.set_knob(Knob::DzLeft as usize, 0.1234);
        assert!(((c.dz_left / 0.02).round() * 0.02 - c.dz_left).abs() < 1e-6, "2% steps: {}", c.dz_left);
        assert!(c.knob_text(Knob::Threshold as usize).starts_with("Button Threshold: 50"));
    }

    #[test]
    fn the_old_single_deadzone_and_the_guide_options_still_load() {
        let mut c = PadCfg::default();
        c.load("pad_deadzone", "0.2");
        assert!((c.dz_left - 0.2).abs() < 1e-6 && (c.dz_right - 0.2).abs() < 1e-6);
        c.style = 4;
        c.layout = 1;
        c.verbosity = 2;
        c.screen_guides = false;
        c.hint_size = 1.5;
        let mut t = String::new();
        c.save_into(&mut t);
        let mut back = PadCfg::default();
        for l in t.lines() {
            if let Some((k, v)) = l.split_once(':') {
                back.load(k, v);
            }
        }
        assert_eq!((back.style, back.layout, back.verbosity, back.screen_guides), (4, 1, 2, false));
        assert!((back.hint_size - 1.5).abs() < 1e-3);
        let text = |t: Tog| c.toggle_text(t as usize);
        assert!(text(Tog::HintLayout).ends_with("Bottom") && text(Tog::GuideDetail).ends_with("Minimal") && text(Tog::IconStyle).ends_with("Switch"));
    }

    #[test]
    fn every_row_of_the_page_exists_once() {
        let mut knobs = Vec::new();
        let mut togs = Vec::new();
        for (_, rows) in SECTIONS {
            for r in rows {
                match r {
                    Row::Knob(k) => knobs.push(*k as usize),
                    Row::Tog(t) => togs.push(*t as usize),
                }
            }
        }
        knobs.sort();
        togs.sort();
        assert_eq!(knobs, (0..PadCfg::knob_count()).collect::<Vec<_>>());
        assert_eq!(togs, (0..PadCfg::toggle_count()).collect::<Vec<_>>());
    }

    #[test]
    fn vibration_and_reach_around_settings_round_trip() {
        let mut c = PadCfg::default();
        assert!(c.vib.enabled && !c.reach_around, "vibration on, reach-around off like Controlify");
        assert_eq!(c.vib.by_source, [1.0; 5]);
        c.vib.enabled = false;
        c.reach_around = true;
        c.set_knob(Knob::Rumble as usize, 0.25); // master: 0..200%
        c.set_knob(Knob::RumbleWorld as usize, 1.0); // world
        let mut t = String::new();
        c.save_into(&mut t);
        assert!(!t.contains("testVibration"), "the test button is not a setting");
        let mut back = PadCfg::default();
        for l in t.lines() {
            let (k, v) = l.split_once(':').unwrap();
            back.load(k, v);
        }
        assert!(!back.vib.enabled && back.reach_around);
        assert!((back.vib.by_source[RSource::Master.index()] - 0.5).abs() < 1e-4);
        assert!((back.vib.by_source[RSource::World.index()] - 2.0).abs() < 1e-4);
        assert_eq!(back.vib.by_source[RSource::Player.index()], 1.0);
        assert_eq!(PadCfg::knob_count(), 12);
        assert_eq!(c.knob_text(Knob::Rumble as usize), "Vibration: 50%");
        assert_eq!(c.toggle_text(Tog::AllowVibration as usize), "Allow Vibration: OFF");
        assert_eq!(c.toggle_text(Tog::ReachAround as usize), "Reach Around: ON");
    }

    #[test]
    fn the_old_single_rumble_line_is_the_master_strength() {
        let mut c = PadCfg::default();
        c.load("pad_rumble", "0.5");
        assert_eq!(c.vib.by_source[RSource::Master.index()], 0.5);
    }

    #[test]
    fn the_test_vibration_row_only_raises_a_flag() {
        let mut c = PadCfg::default();
        assert!(!c.test_vibration);
        c.toggle(Tog::TestVibration as usize);
        assert!(c.test_vibration);
        assert_eq!(c.toggle_text(Tog::TestVibration as usize), "Test Vibration: Tap");
    }

    /// Run `n` 20 Hz ticks of vibration and return the last command made.
    fn buzz(p: &mut Pad, c: &PadCfg, n: usize) -> Option<(i32, Cmd)> {
        let mut last = None;
        for _ in 0..n {
            p.rumble_frame(TICK_SECS, c, false);
            if let Some(x) = p.take_haptics() {
                last = Some(x);
            }
        }
        last
    }

    #[test]
    fn damage_buzzes_the_pad_and_then_stops_it() {
        let (mut p, c) = pad();
        p.device = 7;
        p.watch_health(20);
        buzz(&mut p, &c, 1);
        p.watch_health(17);
        let got = buzz(&mut p, &c, 1);
        assert_eq!(got, Some((7, Cmd::Vibrate { strong: 204, weak: 128, ms: 5000 })), "(0.8, 0.5) of 255");
        assert_eq!(buzz(&mut p, &c, 4), None, "the same level is not sent again");
        assert_eq!(buzz(&mut p, &c, 1), Some((7, Cmd::Cancel)), "5 ticks, then the motors stop");
    }

    #[test]
    fn nothing_buzzes_with_the_keyboard_in_use_or_vibration_off() {
        let (mut p, mut c) = pad();
        p.watch_health(20);
        p.watch_health(10);
        p.kbm = true;
        assert_eq!(buzz(&mut p, &c, 6), None, "keyboard and mouse: silent");
        p.kbm = false;
        c.vib.enabled = false;
        p.watch_health(5);
        assert_eq!(buzz(&mut p, &c, 3), None, "vibration off: nothing is sent");
    }

    #[test]
    fn a_dig_buzzes_until_the_block_is_gone_and_a_new_block_starts_over() {
        let (mut p, c) = pad();
        p.rumble_dig(Some(((1, 2, 3), 2.0)));
        assert!(matches!(buzz(&mut p, &c, 1), Some((_, Cmd::Vibrate { .. }))));
        p.rumble_dig(Some(((1, 2, 3), 2.0)));
        assert_eq!(buzz(&mut p, &c, 3), None, "the same block, the same effect");
        p.rumble_dig(None);
        assert_eq!(buzz(&mut p, &c, 2), Some((p.device, Cmd::Cancel)));
    }

    #[test]
    fn a_held_effect_follows_its_state() {
        let (mut p, c) = pad();
        p.rumble_guard(true);
        assert!(matches!(buzz(&mut p, &c, 1), Some((_, Cmd::Vibrate { .. }))));
        p.rumble_guard(false);
        assert_eq!(buzz(&mut p, &c, 2), Some((p.device, Cmd::Cancel)));
        p.rumble_slow(Some(1.0));
        assert!(matches!(buzz(&mut p, &c, 1), Some((_, Cmd::Vibrate { .. }))));
        p.rumble_slow(None);
        buzz(&mut p, &c, 2);
        assert!(!p.fx.is_playing());
    }

    #[test]
    fn leaving_a_world_clears_the_queue() {
        let (mut p, c) = pad();
        p.rumble_eating(true);
        buzz(&mut p, &c, 1);
        p.rumble_clear();
        assert_eq!(buzz(&mut p, &c, 1), Some((p.device, Cmd::Cancel)));
        assert!(!p.fx.is_playing());
    }
}
