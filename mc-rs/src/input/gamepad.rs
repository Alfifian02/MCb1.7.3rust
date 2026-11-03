//! Controllify (isXander's Fabric mod, LGPL-3.0-or-later), ported to mc-rs: game-pad support on top of `TouchUi`, the one sink every input ends in.
//!
//! Ported from the Java: the default bindings (`default_bind/default.json`), the stick deadzone (`ControllerUtils.deadzone`), the look
//! curves (`InputCurves`, eased on the stick's length so aiming stays circular), 10 degrees per tick at 100% look sensitivity
//! (`InGameInputHandler.handleRegularLook`), the activation threshold of analogue triggers, toggle-sneak, the virtual mouse that drives
//! every GUI (`VirtualMouseHandler`), the radial hotbar (`HOTBAR_SLOT_SELECT`) and rumble on damage / block break / explosions.
//! Each binding is rebindable in Options > Controls > Controller (`options.rs`), saved in `options.txt` as `pad_*`.
//!
//! Button hints (`Pad::hints`): while a pad is in use a corner list names what the bound buttons do, drawn with the pictures of the "Controller
//! Tooltip Selector" pack (Xbox, PlayStation or Switch, `tools/gen_pad_buttons.py`) or as plain text; Controls > Controller picks the style, the
//! sizes (`Icon Size` for the pictures and `Text Size` for the labels, multipliers on the GUI scale, so 100% matches it but they stay separate),
//! the backdrop (`Hint BG`: black at that opacity behind each box) and the layout (a row along the bottom in menus,
//! a list in the corner while playing, or one of them always). The screen hands in the rects it draws (`HintView::avoid`); the hints slide along the
//! bottom (then the top), the pad's left-half buttons (LB, LT, L3, Back, d-pad) from the left corner and the rest from the right, shrink down to half their size, and are left out when nothing is clear: they never cover a button or a footer text.
//!
//! Not ported (nothing in mc-rs to hang them on): gyro, DualSense LEDs / adaptive triggers / touchpad, on-screen keyboard, splitscreen, joystick remapping, the server vibration API, chat, swap hands
//! (b1.7.3 has no sprint / off hand, and mc-rs has no chat yet). Drop throws one item of the selected slot (`Out::drop`). UNVERIFIED on real hardware: the Android axis / key mapping
//! follows the documented layout (X/Y left stick, Z/Rz right stick, Ltrigger/Rtrigger, hat = d-pad).

use super::touch_ui::{HOLD_SECS, TAP_SECS};
use crate::render::hud::{sprites, HudPipeline, HudVertex};
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

    /// The picture of this button in icon `style` (`sprites::pad`: 0 Xbox, 1 PlayStation, 2 Switch); None in the Text style. The face buttons are
    /// Android's, which name the PLACE: A is the bottom one, B right, X left, Y top; the Switch pack has Nintendo's letters in those places.
    fn glyph(self, style: u8) -> Option<sprites::Sprite> {
        (style < 3).then(|| sprites::pad::glyph(style as usize, self as usize))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    Jump,
    Sneak,
    /// Hold digs, a short press hits (the touch screen's look finger, same timings).
    Attack,
    /// Places / uses; held, it repeats.
    Use,
    Inventory,
    Pause,
    NextSlot,
    PrevSlot,
    /// Hold, flick the right stick at a slot, let go.
    Wheel,
    Zoom,
    GuiSelect,
    GuiBack,
    /// Held: sprint (was L3, fixed).
    Sprint,
    /// Cycles first person, third person, over the shoulder (the view button).
    View,
    /// Toggles the info (debug) screen.
    Info,
    /// Throws one of the selected hotbar slot's items (the Drop key).
    Drop,
}

pub const ACTS: [Act; 16] = [Act::Jump, Act::Sneak, Act::Attack, Act::Use, Act::Inventory, Act::Pause, Act::NextSlot, Act::PrevSlot, Act::Wheel, Act::Zoom, Act::GuiSelect, Act::GuiBack, Act::Sprint, Act::View, Act::Info, Act::Drop];

impl Act {
    pub fn name(self) -> &'static str {
        ["Jump", "Sneak", "Attack", "Use", "Inventory", "Pause", "Next slot", "Prev slot", "Hotbar wheel", "Zoom", "GUI select", "GUI back", "Sprint", "View", "Info", "Drop"][self as usize]
    }

    fn id(self) -> &'static str {
        ["jump", "sneak", "attack", "use", "inventory", "pause", "next_slot", "prev_slot", "wheel", "zoom", "gui_select", "gui_back", "sprint", "view", "info", "drop"][self as usize]
    }

    /// `default_bind/default.json` (zoom: its `change_perspective` button; the view bind is the d-pad's right, info its left, drop its down).
    fn default(self) -> Btn {
        [Btn::A, Btn::R3, Btn::Rt, Btn::Lt, Btn::Y, Btn::Start, Btn::Rb, Btn::Lb, Btn::Up, Btn::Back, Btn::A, Btn::B, Btn::L3, Btn::Right, Btn::Left, Btn::Down][self as usize]
    }
}

const KNOBS: [&str; 9] = ["Look X", "Look Y", "Deadzone", "Cursor", "Trigger", "Rumble", "Icon Size", "Text Size", "Hint BG"];
const KNOB_IDS: [&str; 9] = ["lookH", "lookV", "deadzone", "cursor", "threshold", "rumble", "hintSize", "textSize", "hintBack"];
/// Each knob's value at slider 0 and 1.
const KNOB_RANGE: [(f32, f32); 9] = [(0.0, 2.0), (0.0, 2.0), (0.0, 0.5), (0.2, 2.0), (0.1, 0.9), (0.0, 1.0), (0.5, 2.0), (0.5, 2.0), (0.0, 1.0)];
const TOGGLES: [&str; 7] = ["Curve", "Invert X", "Invert Y", "Toggle Sneak", "Button Hints", "Button Icons", "Hint Layout"];
const TOGGLE_IDS: [&str; 7] = ["curve", "invertX", "invertY", "toggleSneak", "hints", "iconStyle", "hintLayout"];
/// The button picture styles (`PadCfg::style`).
const STYLES: [&str; 4] = ["Xbox", "PlayStation", "Switch", "Text"];
/// Where the hints go (`PadCfg::layout`): Auto is a row in menus and a list while playing.
const LAYOUTS: [&str; 3] = ["Auto", "Horizontal", "Vertical"];
/// `InputCurves`.
const CURVES: [&str; 6] = ["Linear", "Gentle", "Standard", "Strong", "Precision", "S-Curve"];

/// Controllify's `InputSettings` (the sensitivity, deadzone, threshold and generic parts) and the bindings; lives in `Settings`.
#[derive(Clone, Debug, PartialEq)]
pub struct PadCfg {
    pub binds: [Btn; ACTS.len()],
    pub look_h: f32,
    pub look_v: f32,
    pub deadzone: f32,
    /// Virtual mouse speed.
    pub cursor: f32,
    /// Trigger value that counts as pressed.
    pub threshold: f32,
    pub rumble: f32,
    pub curve: u8,
    pub invert_x: bool,
    pub invert_y: bool,
    pub toggle_sneak: bool,
    /// The corner list of what the buttons do (`Pad::hints`).
    pub hints: bool,
    /// Index into `STYLES`: 0 Xbox, 1 PlayStation, 2 Switch, 3 Text.
    pub style: u8,
    /// Size of the hints' button pictures as a multiple of the GUI scale (0.5 .. 2.0; 1.0 = the same size as the GUI).
    pub hint_size: f32,
    /// Size of the hints' label text, same scale as `hint_size`, set apart from it.
    pub text_size: f32,
    /// Opacity of the black box behind each group of hints (0 = none .. 1).
    pub hint_back: f32,
    /// Index into `LAYOUTS`: 0 Auto, 1 Horizontal, 2 Vertical.
    pub layout: u8,
}

impl Default for PadCfg {
    /// `default_config.json`: 100% / 90% look, 0.12 deadzone, standard curve, 0.5 threshold, toggle sneak.
    fn default() -> Self {
        Self {
            binds: ACTS.map(Act::default),
            look_h: 1.0,
            look_v: 0.9,
            deadzone: 0.12,
            cursor: 1.0,
            threshold: 0.5,
            rumble: 1.0,
            curve: 2,
            invert_x: false,
            invert_y: false,
            toggle_sneak: true,
            hints: true,
            style: 0,
            hint_size: 1.0,
            text_size: 1.0,
            hint_back: 0.5,
            layout: 0,
        }
    }
}

impl PadCfg {
    fn bind(&self, a: Act) -> u16 {
        self.binds[a as usize].bit()
    }

    /// No GUI action sits on `b`: X and RT are then a container screen's place-one / quick-move (`Out::alt`), and a hint may name X.
    fn free(&self, b: Btn) -> bool {
        ![Act::GuiSelect, Act::GuiBack, Act::Pause, Act::Inventory].iter().any(|&a| self.binds[a as usize] == b)
    }

    fn knob_value(&self, i: usize) -> f32 {
        [self.look_h, self.look_v, self.deadzone, self.cursor, self.threshold, self.rumble, self.hint_size, self.text_size, self.hint_back][i]
    }

    /// The slider position (0..1) of knob `i`.
    pub fn knob(&self, i: usize) -> f32 {
        let (lo, hi) = KNOB_RANGE[i];
        ((self.knob_value(i) - lo) / (hi - lo)).clamp(0.0, 1.0)
    }

    pub fn set_knob(&mut self, i: usize, t: f32) {
        let (lo, hi) = KNOB_RANGE[i];
        let v = lo + t.clamp(0.0, 1.0) * (hi - lo);
        match i {
            0 => self.look_h = v,
            1 => self.look_v = v,
            2 => self.deadzone = v,
            3 => self.cursor = v,
            4 => self.threshold = v,
            5 => self.rumble = v,
            6 => self.hint_size = v,
            7 => self.text_size = v,
            _ => self.hint_back = v,
        }
    }

    pub fn knob_text(&self, i: usize) -> String {
        format!("{}: {}%", KNOBS[i], (self.knob_value(i) * 100.0).round() as i32)
    }

    pub fn knob_count() -> usize {
        KNOBS.len()
    }

    pub fn toggle(&mut self, i: usize) {
        match i {
            0 => self.curve = (self.curve + 1) % CURVES.len() as u8,
            1 => self.invert_x = !self.invert_x,
            2 => self.invert_y = !self.invert_y,
            3 => self.toggle_sneak = !self.toggle_sneak,
            4 => self.hints = !self.hints,
            5 => self.style = (self.style + 1) % STYLES.len() as u8,
            _ => self.layout = (self.layout + 1) % LAYOUTS.len() as u8,
        }
    }

    pub fn toggle_text(&self, i: usize) -> String {
        let onoff = |b: bool| if b { "ON" } else { "OFF" };
        let v = match i {
            0 => CURVES[self.curve as usize % CURVES.len()],
            1 => onoff(self.invert_x),
            2 => onoff(self.invert_y),
            3 => onoff(self.toggle_sneak),
            4 => onoff(self.hints),
            5 => STYLES[self.style as usize % STYLES.len()],
            _ => LAYOUTS[self.layout as usize % LAYOUTS.len()],
        };
        format!("{}: {v}", TOGGLES[i])
    }

    pub fn toggle_count() -> usize {
        TOGGLES.len()
    }

    /// `options.txt` line: `pad_<id>:<value>`. A bad value is ignored.
    pub fn load(&mut self, k: &str, v: &str) {
        let Some(k) = k.strip_prefix("pad_") else { return };
        if let (Some(i), Ok(n)) = (ACTS.iter().position(|a| a.id() == k), v.parse::<usize>()) {
            self.binds[i] = Btn::ALL[n.min(15)];
        } else if let (Some(i), Ok(f)) = (KNOB_IDS.iter().position(|id| *id == k), v.parse::<f32>()) {
            let (lo, hi) = KNOB_RANGE[i];
            self.set_knob(i, (f.clamp(lo, hi) - lo) / (hi - lo));
        } else if let (Some(i), Ok(n)) = (TOGGLE_IDS.iter().position(|id| *id == k), v.parse::<u8>()) {
            match i {
                0 => self.curve = n.min(CURVES.len() as u8 - 1),
                1 => self.invert_x = n != 0,
                2 => self.invert_y = n != 0,
                3 => self.toggle_sneak = n != 0,
                4 => self.hints = n != 0,
                5 => self.style = n.min(STYLES.len() as u8 - 1),
                _ => self.layout = n.min(LAYOUTS.len() as u8 - 1),
            }
        }
    }

    pub fn save_into(&self, t: &mut String) {
        for (a, b) in ACTS.iter().zip(self.binds) {
            t.push_str(&format!("pad_{}:{}\n", a.id(), b as usize));
        }
        for (i, id) in KNOB_IDS.iter().enumerate() {
            t.push_str(&format!("pad_{id}:{}\n", self.knob_value(i)));
        }
        let b = |x: bool| x as u8;
        t.push_str(&format!("pad_curve:{}\npad_invertX:{}\npad_invertY:{}\npad_toggleSneak:{}\npad_hints:{}\npad_iconStyle:{}\npad_hintLayout:{}\n", self.curve, b(self.invert_x), b(self.invert_y), b(self.toggle_sneak), b(self.hints), self.style, self.layout));
    }
}

/// A button a contextual hint names by what it does in a GUI: the bound select / back, or X (`Out::alt`) while no bind has taken it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Select,
    Alt,
    Back,
}

/// What the hints need to know of the screen they are drawn over: its size and GUI scale (px), whether it is a menu / container, and the rects
/// (px) the screen draws that a hint must keep off: buttons, footer text, panels, the hotbar. Built by the caller, one list per screen.
pub struct HintView<'a> {
    pub w: f32,
    pub h: f32,
    /// Pixels per GUI unit (`menu::scaled_gs`).
    pub s: f32,
    pub ui: bool,
    pub avoid: &'a [[f32; 4]],
}

/// The size of the hints' box and, for each row, [button x, y, button width, label x] inside it. `btns` / `texts` are the widths of each row's
/// button and label, `row` its height. A list lines the buttons up in one column; a row sets each button by its label.
fn hint_boxes(btns: &[f32], texts: &[f32], row: f32, gap: f32, horiz: bool) -> ((f32, f32), Vec<[f32; 4]>) {
    if horiz {
        let mut x = 0.0;
        let at = btns.iter().zip(texts).map(|(&b, &t)| {
            let r = [x, 0.0, b, x + b + gap];
            x += b + gap + t + 4.0 * gap;
            r
        });
        let at: Vec<_> = at.collect();
        ((x - 4.0 * gap, row), at)
    } else {
        let (col, text) = (btns.iter().cloned().fold(0.0, f32::max), texts.iter().cloned().fold(0.0, f32::max));
        let at = btns.iter().enumerate().map(|(i, &b)| [(col - b) * 0.5, i as f32 * (row + gap), b, col + gap]).collect();
        ((col + gap + text, btns.len() as f32 * row + (btns.len() as f32 - 1.0) * gap), at)
    }
}

/// The top-left of a `(bw, bh)` box on a `w` x `h` screen that is `margin` from the edges and at least `clear` from every rect of `avoid`:
/// along the bottom edge from the right corner to the left (from the left corner to the right when `left`), then along the top edge. None when no place is clear.
fn hint_spot(w: f32, h: f32, (bw, bh): (f32, f32), margin: f32, clear: f32, avoid: &[[f32; 4]], left: bool) -> Option<(f32, f32)> {
    let free = |x: f32, y: f32| avoid.iter().all(|a| x >= a[0] + a[2] + clear || x + bw <= a[0] - clear || y >= a[1] + a[3] + clear || y + bh <= a[1] - clear);
    let step = (margin * 0.5).max(1.0);
    for y in [h - margin - bh, margin] {
        let mut x = if left { margin } else { w - margin - bw };
        while x >= margin - 0.01 && x <= w - margin - bw + 0.01 {
            if free(x, y) {
                return Some((x, y));
            }
            x += if left { step } else { -step };
        }
    }
    None
}

/// `ControllerUtils.deadzone`.
fn deadzone(v: f32, dz: f32) -> f32 {
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
/// A held d-pad direction steps again after `NAV_FIRST` s, then every `NAV_REPEAT` s.
const NAV_FIRST: f32 = 0.4;
const NAV_REPEAT: f32 = 0.12;

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
    /// The sprint bind is held (Retro Sprint's key; L3 by default).
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
    /// The drop bind was pressed (playing only).
    pub drop: bool,
    pub pause: bool,
    pub back: bool,
    pub slot_delta: i32,
    pub slot_to: Option<usize>,
    /// Virtual mouse move in px, and the press / release of the click.
    pub cursor: (f32, f32),
    pub click: Option<bool>,
    /// With `Pad::snap`: one step of the d-pad as (x, y), y down; the first on the press, then repeating while held.
    pub nav: Option<(i32, i32)>,
    /// In a GUI: LB / RB turn a page (-1 / +1; the options tabs), and the right stick scrolls (px, down is positive) like a finger dragging a list.
    pub page: i32,
    pub scroll: f32,
    /// In a GUI: X was pressed (`Some(false)`: the slot under the cursor is clicked for one, the "One" button's click without the toggle)
    /// or RT (`Some(true)`: it is quick-moved, the "Shift" button's click). Not while that button is bound to a GUI action.
    pub alt: Option<bool>,
}

/// Live state of the pad(s): every controller feeds the same state, like Controllify's single active input mode.
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
    prev: u16,
    /// Buttons held when the mode changed: ignored until released, so leaving a GUI with A held does not jump.
    suppress: u16,
    ui: bool,
    moving: bool,
    attack_t: f32,
    /// `dig_now` was called for the press now down: it digs from here to the release, which is no hit.
    attack_dug: bool,
    use_t: f32,
    /// The Controls screen is waiting for a button; the next press lands in `captured` and does nothing else.
    pub capture: bool,
    pub captured: Option<Btn>,
    wheel_open: bool,
    wheel: Option<usize>,
    /// The open screen jumps between its targets (`Out::nav`) instead of the d-pad nudging the free cursor, which the stick still pushes (set by `lib.rs`: any container screen).
    pub snap: bool,
    nav_dir: (i32, i32),
    nav_t: f32,
    hp: i32,
    rumble: Option<(u32, u8)>,
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

    /// Buttons held now: keys, the hat as a d-pad, triggers past the threshold.
    fn digital(&self, cfg: &PadCfg) -> u16 {
        let mut m = self.keys;
        for (on, b) in [(self.hat.0 < -0.5, Btn::Left), (self.hat.0 > 0.5, Btn::Right), (self.hat.1 < -0.5, Btn::Up), (self.hat.1 > 0.5, Btn::Down), (self.lt > cfg.threshold, Btn::Lt), (self.rt > cfg.threshold, Btn::Rt)] {
            if on {
                m |= b.bit();
            }
        }
        m
    }

    /// Queue a rumble (`ms`, `amp` 1..255); the strongest of a frame wins.
    pub fn rumble(&mut self, ms: u32, amp: u8) {
        if self.rumble.map_or(true, |(_, a)| amp >= a) {
            self.rumble = Some((ms, amp));
        }
    }

    /// Health went down: `RumbleSource.PLAYER` on damage. Call every frame with the current health.
    pub fn watch_health(&mut self, hp: i32) {
        if hp < self.hp {
            self.rumble(250, 200);
        }
        self.hp = hp;
    }

    /// (device, ms, amplitude) to send to the controller, scaled by the Rumble setting.
    pub fn take_rumble(&mut self, cfg: &PadCfg) -> Option<(i32, u32, u8)> {
        let (ms, a) = self.rumble.take()?;
        let a = (a as f32 * cfg.rumble) as u8;
        (self.connected && a > 0).then_some((self.device, ms, a))
    }

    /// One frame. `ui`: a screen, the pause menu or a title menu is up (the sticks drive the cursor, not the player). `side`: the short
    /// side of the surface in px.
    /// The attack button has been down for `secs` (the Dig Delay: the arm swings, the dig may start: `dig_now`).
    pub fn resting(&self, secs: f32) -> bool {
        self.attack_t >= secs
    }

    /// The caller saw a block, not a mob, under the crosshair: the press now down digs from this frame, and its release is no hit.
    pub fn dig_now(&mut self, secs: f32) {
        self.attack_dug = self.resting(secs);
    }

    /// The d-pad as one of four directions: a step on the press, again after `NAV_FIRST` and then every `NAV_REPEAT` while it is held, so
    /// a long list is one hold, not a press per row.
    fn nav_step(&mut self, dt: f32, held: u16) -> Option<(i32, i32)> {
        let on = |b: Btn| i32::from(held & b.bit() != 0);
        let dp = (on(Btn::Right) - on(Btn::Left), on(Btn::Down) - on(Btn::Up));
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

    pub fn update(&mut self, dt: f32, cfg: &PadCfg, ui: bool, side: f32) -> Out {
        let mut o = Out::default();
        if !self.connected {
            return o;
        }
        let raw = self.digital(cfg);
        if ui != self.ui {
            self.suppress |= raw;
            (self.ui, self.wheel_open, self.wheel, self.attack_t, self.attack_dug, self.use_t) = (ui, false, None, 0.0, false, 0.0);
            self.nav_dir = (0, 0);
        }
        self.suppress &= raw;
        let held = raw & !self.suppress;
        let (pressed, released) = (held & !self.prev, self.prev & !held);
        self.prev = held;
        let stick = [self.lx, self.ly, self.rx, self.ry].iter().any(|v| v.abs() > cfg.deadzone);
        self.idle = if raw != 0 || stick { 0.0 } else { self.idle + dt };
        if self.capture {
            self.captured = Btn::ALL.into_iter().find(|b| pressed & b.bit() != 0).or(self.captured);
            return o;
        }
        let (down, just, up) = (|a: Act| held & cfg.bind(a) != 0, |a: Act| pressed & cfg.bind(a) != 0, |a: Act| released & cfg.bind(a) != 0);
        let dz = |v: f32| deadzone(v, cfg.deadzone.min(0.9));
        let (lx, ly, rx, ry) = (dz(self.lx), dz(self.ly), dz(self.rx), dz(self.ry));
        if ui {
            // `VirtualMouseHandler`: the left stick (eased) and the d-pad move the cursor.
            // The stick always pushes the free cursor; on a snapping screen the d-pad jumps between targets instead of nudging it.
            let (sx, sy) = ease(lx, ly, 2);
            let (hx, hy) = if self.snap { (0.0, 0.0) } else { (self.hat.0.round() * 0.4, self.hat.1.round() * 0.4) };
            let k = side * CURSOR_SPEED * cfg.cursor * dt;
            o.cursor = ((sx + hx) * k, (sy + hy) * k);
            o.page = i32::from(pressed & Btn::Rb.bit() != 0) - i32::from(pressed & Btn::Lb.bit() != 0);
            o.scroll = ry * k;
            let free = |b: Btn| cfg.free(b);
            let on = |b: Btn| pressed & b.bit() != 0 && free(b);
            o.alt = if on(Btn::X) { Some(false) } else if on(Btn::Rt) { Some(true) } else { None };
            if self.snap {
                o.nav = self.nav_step(dt, held);
            }
            o.click = if just(Act::GuiSelect) { Some(true) } else if up(Act::GuiSelect) { Some(false) } else { None };
            (o.back, o.pause, o.inventory) = (just(Act::GuiBack), just(Act::Pause), just(Act::Inventory));
            return o;
        }
        // Walking: the left stick, up is forward.
        let (side_v, fwd) = (lx, -ly);
        let l = side_v.hypot(fwd).max(1.0);
        let active = side_v != 0.0 || fwd != 0.0;
        if active || self.moving {
            o.mv = Some((fwd / l, side_v / l));
        }
        self.moving = active;
        // The hotbar wheel takes the right stick while it is held.
        if down(Act::Wheel) {
            self.wheel_open = true;
            if rx.hypot(ry) > 0.5 {
                let a = ry.atan2(rx) + std::f32::consts::FRAC_PI_2; // 0 = up, clockwise
                let s = (a / (std::f32::consts::TAU / HOTBAR as f32)).round().rem_euclid(HOTBAR as f32) as usize;
                self.wheel = Some(s);
            }
        } else if self.wheel_open {
            self.wheel_open = false;
            o.slot_to = self.wheel.take();
        } else {
            let (x, y) = ease(if cfg.invert_x { -rx } else { rx }, if cfg.invert_y { -ry } else { ry }, cfg.curve);
            o.look = (x * cfg.look_h * LOOK_RAD_PER_SEC * dt, y * cfg.look_v * LOOK_RAD_PER_SEC * dt);
        }
        o.jump = down(Act::Jump);
        o.zoom = down(Act::Zoom);
        (o.sprint, o.view, o.info) = (down(Act::Sprint), just(Act::View), just(Act::Info));
        (o.sneak_down, o.sneak_up) = (just(Act::Sneak), up(Act::Sneak));
        // Attack: the touch screen's look finger. Held past HOLD_SECS digs, a short press hits.
        o.attack = down(Act::Attack);
        if o.attack {
            self.attack_t += dt;
            o.dig = self.attack_dug || self.attack_t >= HOLD_SECS;
        } else if up(Act::Attack) {
            o.strike = !self.attack_dug && self.attack_t < HOLD_SECS; // the complement of `dig`: no press is neither
            o.release = !self.attack_dug && self.attack_t >= HOLD_SECS;
            (self.attack_t, self.attack_dug) = (0.0, false);
        }
        // Use: on the press, then 4 a second while held.
        if just(Act::Use) {
            (o.place, self.use_t) = (true, 0.0);
        } else if down(Act::Use) {
            self.use_t += dt;
            if self.use_t >= TAP_SECS {
                (o.place, self.use_t) = (true, self.use_t - TAP_SECS);
            }
        }
        (o.use_down, o.use_just) = (down(Act::Use), just(Act::Use));
        o.place |= o.strike;
        (o.inventory, o.pause, o.drop) = (just(Act::Inventory), just(Act::Pause), just(Act::Drop));
        o.slot_delta = just(Act::NextSlot) as i32 - just(Act::PrevSlot) as i32;
        o
    }

    /// The radial hotbar and, in a GUI, the cursor drawn over `cursor` (px). The cursor's cross, shadow included, is 14 pixels of `k` wide;
    /// `cap` (px, `f32::INFINITY` for none) is the most it may take, so over a slot the cursor stays smaller than the slot (whole pixels).
    pub fn draw(&self, v: &mut Vec<HudVertex>, w: f32, h: f32, slot: usize, cursor: Option<(f32, f32)>, cap: f32) {
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
            let k = (h.min(w) * 0.006).max(2.0);
            let k = if cap.is_finite() { k.min((cap / 14.0).floor().max(1.0)) } else { k };
            HudPipeline::push_quad(v, x - k * 6.0 - k, y - k, k * 12.0 + 2.0 * k, k * 3.0, [0.0, 0.0, 0.0, 0.8]);
            HudPipeline::push_quad(v, x - k, y - k * 6.0 - k, k * 3.0, k * 12.0 + 2.0 * k, [0.0, 0.0, 0.0, 0.8]);
            HudPipeline::push_quad(v, x - k * 6.0, y, k * 12.0, k, [1.0; 4]);
            HudPipeline::push_quad(v, x, y - k * 6.0, k, k * 12.0, [1.0; 4]);
        }
    }

    /// Button hints: while a pad is in use, what the bound buttons do (playing: attack, place, jump, sneak, inventory, drop; in a GUI: select,
    /// back). Each button is its picture from `sprites::pad` (`PadCfg::style`), or in the Text style a dark pill with its name. One font pixel
    /// `k` is a whole screen pixel (the GUI scale times `Icon Size` for the pictures, `Text Size` for the labels) and a picture is 13 of them, so both stay crisp.
    /// The left-hand buttons of the pad get a box at the left of the screen, the right-hand ones a box at the right, each on a black backdrop of `Hint BG` opacity.
    /// The box goes where `hint_spot` finds room clear of `view.avoid`; when none does it shrinks (down to half) and tries the other layout;
    /// when still nothing is clear it is not drawn at all.
    pub fn hints(&self, v: &mut Vec<HudVertex>, cfg: &PadCfg, view: &HintView) {
        self.hints_with(v, cfg, view, &[]);
    }

    /// `hints` with the rows a screen chose for what is under the cursor (`rows`, in order; X is left out while a bind has it). Empty `rows`
    /// give the standard ones: select and back in a GUI, the playing buttons otherwise.
    pub fn hints_with(&self, v: &mut Vec<HudVertex>, cfg: &PadCfg, view: &HintView, rows: &[(Role, &str)]) {
        if !cfg.hints || !self.active() {
            return;
        }
        let btn = |a: Act| cfg.binds[a as usize];
        let rows: Vec<(Btn, &str)> = if !rows.is_empty() {
            rows.iter()
                .filter_map(|&(r, t)| match r {
                    Role::Select => Some(btn(Act::GuiSelect)),
                    Role::Back => Some(btn(Act::GuiBack)),
                    Role::Alt => cfg.free(Btn::X).then_some(Btn::X),
                }.map(|b| (b, t)))
                .collect()
        } else if view.ui {
            vec![(btn(Act::GuiSelect), "Select"), (btn(Act::GuiBack), "Back")]
        } else {
            [(Act::Attack, "Attack"), (Act::Use, "Place"), (Act::Jump, "Jump"), (Act::Sneak, "Sneak"), (Act::Inventory, "Inventory"), (Act::Drop, "Drop")].iter().map(|&(a, t)| (btn(a), t)).collect()
        };
        if rows.is_empty() {
            return;
        }
        let (ki0, kt0) = ((view.s * cfg.hint_size).round().max(1.0), (view.s * cfg.text_size).round().max(1.0));
        let (kimin, ktmin) = ((ki0 / 2.0).ceil().max(1.0), (kt0 / 2.0).ceil().max(1.0));
        // Auto: a row in a menu (the bottom of a screen is mostly empty), a list in the corner while playing; the other one is the fallback.
        let horiz = match cfg.layout {
            1 => vec![true],
            2 => vec![false],
            _ => vec![view.ui, !view.ui],
        };
        let clear = 4.0 * view.s;
        // The buttons on the pad's left half go to the left of the screen, the rest to the right: one box each.
        let groups: Vec<(bool, Vec<(Btn, &str)>)> = [true, false]
            .into_iter()
            .map(|l| (l, rows.iter().filter(|r| r.0.left() == l).copied().collect::<Vec<_>>()))
            .filter(|g| !g.1.is_empty())
            .collect();
        let a = cfg.hint_back.clamp(0.0, 1.0);
        let (mut ki, mut kt) = (ki0, kt0);
        loop {
            let (row, gap, m) = ((13.0 * ki).max(8.0 * kt), 2.0 * ki.max(kt), ki.max(kt));
            let pad = if a > 0.0 { 2.0 * m } else { 0.0 };
            let width = |b: Btn| if b.glyph(cfg.style).is_some() { 13.0 * ki } else { (HudPipeline::text_width(b.name()) as f32 + 4.0) * ki };
            let mut avoid = view.avoid.to_vec(); // the first box is one more thing the second keeps off
            let mut put = Vec::new();
            for (left, g) in &groups {
                let btns: Vec<f32> = g.iter().map(|&(b, _)| width(b)).collect();
                let texts: Vec<f32> = g.iter().map(|&(_, t)| HudPipeline::text_width(t) as f32 * kt).collect();
                for &h in &horiz {
                    let ((bw, bh), at) = hint_boxes(&btns, &texts, row, gap, h);
                    let (bw, bh) = (bw + kt + 2.0 * pad, bh + kt + 2.0 * pad); // + kt: the text's shadow
                    if let Some((x0, y0)) = hint_spot(view.w, view.h, (bw, bh), 6.0 * m, clear, &avoid, *left) {
                        avoid.push([x0, y0, bw, bh]);
                        put.push((g, at, [x0, y0, bw, bh]));
                        break;
                    }
                }
            }
            if put.len() == groups.len() || (ki <= kimin && kt <= ktmin) {
                // ponytail: a box may be left out at the smallest size, it may not cover a button; a smaller one still would be unreadable
                for (g, at, [x0, y0, bw, bh]) in put {
                    if a > 0.0 {
                        HudPipeline::push_quad(v, x0, y0, bw, bh, [0.0, 0.0, 0.0, a]);
                    }
                    for (i, &(b, t)) in g.iter().enumerate() {
                        let [gx, gy, gw, tx] = at[i];
                        let (gx, y) = (x0 + pad + gx, y0 + pad + gy);
                        let iy = y + (row - 13.0 * ki) * 0.5; // the picture and the 8 px font are each centred on the row
                        if let Some(spr) = b.glyph(cfg.style) {
                            HudPipeline::push_gui(v, gx, iy, 13.0 * ki, 13.0 * ki, spr);
                        } else {
                            HudPipeline::push_quad(v, gx, iy, gw, 13.0 * ki, [0.1, 0.1, 0.1, 0.85]);
                            HudPipeline::push_text(v, gx + 2.0 * ki, iy + 2.5 * ki, ki, b.name(), [1.0; 4]);
                        }
                        HudPipeline::push_text(v, x0 + pad + tx, y + (row - 8.0 * kt) * 0.5, kt, t, [1.0; 4]);
                    }
                }
                return;
            }
            (ki, kt) = ((ki - 1.0).max(kimin), (kt - 1.0).max(ktmin));
        }
    }

    /// The touch buttons (everything of the HUD but the hotbar) are not drawn: a pad, keyboard or mouse is in hand. They still answer a finger.
    pub fn hud_hidden(&self) -> bool {
        self.active() || self.kbm
    }

    pub fn shows_cursor(&self) -> bool {
        self.active()
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

    /// `pad()` with Place and Drop moved to right-hand buttons, so all the playing hints are one box on the right.
    fn pad_right() -> (Pad, PadCfg) {
        let (p, mut c) = pad();
        c.binds[Act::Use as usize] = Btn::X;
        c.binds[Act::Drop as usize] = Btn::Rb;
        (p, c)
    }

    #[test]
    fn deadzone_and_curves() {
        assert_eq!(deadzone(0.1, 0.12), 0.0);
        assert!((deadzone(1.0, 0.12) - 1.0).abs() < 1e-6 && deadzone(-1.0, 0.12) < 0.0);
        let (x, y) = ease(0.6, 0.8, 2); // length 1 keeps its direction
        assert!((x - 0.6).abs() < 1e-5 && (y - 0.8).abs() < 1e-5);
        let (x, _) = ease(0.5, 0.0, 2);
        assert!((x - 0.25).abs() < 1e-5);
    }

    #[test]
    fn l3_is_the_sprint_key() {
        let (mut p, c) = pad();
        assert!(!p.update(DT, &c, false, 1000.0).sprint);
        p.key(Btn::L3, true);
        assert!(p.update(DT, &c, false, 1000.0).sprint);
        p.key(Btn::L3, false);
        assert!(!p.update(DT, &c, false, 1000.0).sprint);
    }

    #[test]
    fn walks_looks_and_jumps() {
        let (mut p, c) = pad();
        p.axes(0.0, -1.0, 1.0, 0.0, 0.0, 0.0, (0.0, 0.0));
        p.key(Btn::A, true);
        let o = p.update(DT, &c, false, 1000.0);
        assert_eq!(o.mv, Some((1.0, 0.0)), "up is forward");
        assert!(o.look.0 > 0.0 && o.look.1 == 0.0 && o.jump);
        p.axes(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, (0.0, 0.0));
        assert_eq!(p.update(DT, &c, false, 1000.0).mv, Some((0.0, 0.0)), "one zero when the stick returns");
        assert_eq!(p.update(DT, &c, false, 1000.0).mv, None);
    }

    #[test]
    fn attack_taps_hit_and_holds_dig() {
        let (mut p, c) = pad();
        p.axes(0.0, 0.0, 0.0, 0.0, 0.0, 1.0, (0.0, 0.0)); // right trigger
        let o = p.update(DT, &c, false, 1000.0);
        assert!(!o.dig && o.attack, "the arm may swing from the first frame, the dig waits");
        p.axes(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, (0.0, 0.0));
        let o = p.update(DT, &c, false, 1000.0);
        assert!(o.strike && o.place, "a short press is a hit");
        p.axes(0.0, 0.0, 0.0, 0.0, 0.0, 1.0, (0.0, 0.0));
        let mut dug = false;
        for _ in 0..40 {
            dug |= p.update(DT, &c, false, 1000.0).dig;
        }
        assert!(dug);
        p.axes(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, (0.0, 0.0));
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
        p.axes(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, (0.0, 0.0));
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
        p.axes(1.0, 0.0, 0.0, 0.0, 0.0, 0.0, (0.0, 0.0));
        p.key(Btn::A, true);
        let o = p.update(DT, &c, true, 1000.0);
        assert!(o.cursor.0 > 0.0 && o.click == Some(true));
        p.key(Btn::A, false);
        assert_eq!(p.update(DT, &c, true, 1000.0).click, Some(false));
        // wheel: hold D-Up, push the stick right (a quarter turn: slot index 2), release the button.
        p.axes(0.0, 0.0, 1.0, 0.0, 0.0, 0.0, (0.0, 0.0));
        p.key(Btn::Up, true);
        p.update(DT, &c, false, 1000.0);
        p.key(Btn::Up, false);
        assert_eq!(p.update(DT, &c, false, 1000.0).slot_to, Some(2));
    }

    #[test]
    fn snap_steps_once_on_a_press_then_repeats_while_held() {
        let (mut p, c) = pad();
        p.snap = true;
        p.update(DT, &c, true, 1000.0); // the mode change: what is held now is ignored
        p.key(Btn::Down, true);
        let o = p.update(DT, &c, true, 1000.0);
        assert_eq!((o.nav, o.cursor), (Some((0, 1)), (0.0, 0.0)), "a step, and the cursor is not pushed");
        let mut steps = 0;
        for _ in 0..60 {
            steps += i32::from(p.update(DT, &c, true, 1000.0).nav.is_some());
        }
        assert!((5..=7).contains(&steps), "after the first delay a held d-pad steps every {NAV_REPEAT} s: {steps} in 1 s");
        p.key(Btn::Down, false);
        assert_eq!(p.update(DT, &c, true, 1000.0).nav, None);
        // The stick never steps: it pushes the free cursor, snap or not; the d-pad does not nudge it while snapping.
        p.axes(1.0, 0.0, 0.0, 0.0, 0.0, 0.0, (0.0, 0.0));
        let o = p.update(DT, &c, true, 1000.0);
        assert!(o.nav.is_none() && o.cursor.0 > 0.0);
        p.axes(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, (1.0, 0.0));
        let o = p.update(DT, &c, true, 1000.0);
        assert_eq!((o.nav, o.cursor), (Some((1, 0)), (0.0, 0.0)));
        // Without `snap` the d-pad nudges the cursor and never steps.
        p.snap = false;
        let o = p.update(DT, &c, true, 1000.0);
        assert!(o.nav.is_none() && o.cursor.0 > 0.0);
    }

    #[test]
    fn bumpers_page_and_the_right_stick_scrolls_in_a_gui() {
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
        assert!(o.scroll > 0.0 && o.cursor == (0.0, 0.0), "the right stick scrolls, it does not move the cursor");
    }

    #[test]
    fn x_and_rt_are_a_one_click_and_a_quick_move_in_a_gui() {
        let (mut p, mut c) = pad();
        p.update(DT, &c, true, 1000.0);
        p.key(Btn::X, true);
        assert_eq!(p.update(DT, &c, true, 1000.0).alt, Some(false));
        assert_eq!(p.update(DT, &c, true, 1000.0).alt, None, "a held button acts once");
        p.key(Btn::X, false);
        p.axes(0.0, 0.0, 0.0, 0.0, 0.0, 1.0, (0.0, 0.0));
        assert_eq!(p.update(DT, &c, true, 1000.0).alt, Some(true));
        p.axes(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, (0.0, 0.0));
        p.update(DT, &c, true, 1000.0);
        c.binds[ACTS.iter().position(|a| *a == Act::GuiBack).unwrap()] = Btn::X;
        p.key(Btn::X, true);
        let o = p.update(DT, &c, true, 1000.0);
        assert!(o.alt.is_none() && o.back, "X rebound to GUI back is back, not a click");
    }

    #[test]
    fn sprint_view_and_info_follow_their_binds() {
        let (mut p, mut c) = pad();
        p.key(Btn::L3, true);
        assert!(p.update(DT, &c, false, 1000.0).sprint);
        p.key(Btn::L3, false);
        c.binds[ACTS.iter().position(|a| *a == Act::Sprint).unwrap()] = Btn::X;
        p.key(Btn::L3, true);
        assert!(!p.update(DT, &c, false, 1000.0).sprint, "L3 no longer sprints once rebound");
        p.key(Btn::L3, false);
        p.key(Btn::X, true);
        assert!(p.update(DT, &c, false, 1000.0).sprint);
        p.key(Btn::X, false);
        p.key(Btn::Right, true);
        let o = p.update(DT, &c, false, 1000.0);
        assert!(o.view && !o.info);
        assert!(!p.update(DT, &c, false, 1000.0).view, "one press, one switch");
        p.key(Btn::Left, true);
        assert!(p.update(DT, &c, false, 1000.0).info);
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
    fn capture_swallows_the_press() {
        let (mut p, c) = pad();
        p.capture = true;
        p.key(Btn::Start, true);
        let o = p.update(DT, &c, false, 1000.0);
        assert!(!o.pause && p.captured == Some(Btn::Start));
    }

    #[test]
    fn hints_follow_the_binds_and_the_style() {
        let (mut p, mut c) = pad();
        let quads = |p: &Pad, c: &PadCfg, ui| {
            let mut v = Vec::new();
            p.hints(&mut v, c, &HintView { w: 1280.0, h: 720.0, s: 3.0, ui, avoid: &[] });
            v.len()
        };
        assert!(quads(&p, &c, false) > 0 && quads(&p, &c, false) > quads(&p, &c, true), "playing lists more than a GUI");
        c.hints = false;
        assert_eq!(quads(&p, &c, false), 0, "off draws nothing");
        c.hints = true;
        c.style = 3;
        assert!(Btn::A.glyph(3).is_none() && Btn::A.glyph(2).is_some(), "Text has no pictures");
        assert!(quads(&p, &c, false) > 0, "Text draws pills");
        p.idle = 9.0;
        assert_eq!(quads(&p, &c, false), 0, "no pad in use: no hints");
        // every button has its own picture, in the style's rows of the sheet
        let all: Vec<_> = Btn::ALL.iter().map(|b| b.glyph(1).unwrap()).collect();
        assert!(all.iter().enumerate().all(|(i, a)| all[..i].iter().all(|b| a != b)));
        assert_eq!(Btn::A.glyph(1).unwrap().1 - Btn::A.glyph(0).unwrap().1, 52.0);
    }

    /// The bounding box of what `hints` drew (None when it drew nothing).
    fn hint_box(p: &Pad, c: &PadCfg, view: &HintView) -> Option<[f32; 4]> {
        hint_box_with(p, c, view, &[])
    }

    fn hint_box_with(p: &Pad, c: &PadCfg, view: &HintView, rows: &[(Role, &str)]) -> Option<[f32; 4]> {
        let mut v = Vec::new();
        p.hints_with(&mut v, c, view, rows);
        v.iter().fold(None, |b: Option<[f32; 4]>, q| {
            let [x, y] = q.pos;
            Some(b.map_or([x, y, x, y], |b| [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)]))
        })
    }

    fn view(ui: bool, avoid: &[[f32; 4]]) -> HintView<'_> {
        HintView { w: 1280.0, h: 720.0, s: 3.0, ui, avoid }
    }

    /// The box `b` (x0, y0, x1, y1) is at least `clear` px from the rect `a` (x, y, w, h).
    fn apart(b: [f32; 4], a: [f32; 4], clear: f32) -> bool {
        b[0] >= a[0] + a[2] + clear || b[2] <= a[0] - clear || b[1] >= a[1] + a[3] + clear || b[3] <= a[1] - clear
    }

    #[test]
    fn hints_never_cover_a_button_or_a_footer() {
        let (p, mut c) = pad_right();
        // the options screen of the screenshots: Done bottom right (80 x 20 units at 3 px), a list over the rest, a footer text
        let (done, list, footer) = ([1000.0, 636.0, 240.0, 60.0], [60.0, 90.0, 1000.0, 520.0], [700.0, 690.0, 330.0, 24.0]);
        let none = hint_box(&p, &c, &view(true, &[])).unwrap();
        assert!(none[2] > 1200.0 && none[3] > 650.0, "nothing in the way: the bottom right corner, as it was");
        let cases: [Vec<[f32; 4]>; 3] = [vec![done], vec![done, list], vec![done, list, footer]];
        for avoid in &cases {
            let b = hint_box(&p, &c, &view(true, avoid)).expect("there is room along the bottom");
            assert!(avoid.iter().all(|a| apart(b, *a, 12.0)), "{avoid:?} vs {b:?}");
            assert!(b[0] >= 0.0 && b[2] <= 1280.0 && b[1] >= 0.0 && b[3] <= 720.0);
        }
        // a row in a menu, a list while playing, and the setting forces either
        let tall = |b: [f32; 4]| b[3] - b[1] > b[2] - b[0];
        assert!(!tall(hint_box(&p, &c, &view(true, &[])).unwrap()), "Auto: a row in a menu");
        assert!(tall(hint_box(&p, &c, &view(false, &[])).unwrap()), "Auto: a list while playing");
        c.layout = 1;
        assert!(!tall(hint_box(&p, &c, &view(false, &[])).unwrap()), "Horizontal");
        c.layout = 2;
        assert!(tall(hint_box(&p, &c, &view(false, &[])).unwrap()), "Vertical");
        // the whole screen is covered: nothing is drawn, not even a smaller one
        assert!(hint_box(&p, &c, &view(true, &[[0.0, 0.0, 1280.0, 720.0]])).is_none());
    }

    #[test]
    fn hints_shrink_or_turn_before_they_give_up_and_follow_the_size() {
        let (p, mut c) = pad_right();
        let high = |c: &PadCfg, avoid: &[[f32; 4]]| hint_box(&p, c, &view(false, avoid)).map(|b| b[3] - b[1]);
        let normal = high(&c, &[]).unwrap();
        (c.hint_size, c.text_size) = (2.0, 2.0);
        assert!(high(&c, &[]).unwrap() > normal * 1.9, "200% doubles the hints");
        (c.hint_size, c.text_size) = (0.5, 0.5);
        assert!(high(&c, &[]).unwrap() < normal * 0.75, "50% shrinks them (whole pixels: 3 -> 2)");
        // the screen is taken but for a strip along the bottom: the list of 6 does not fit, a row does
        (c.hint_size, c.text_size) = (1.0, 1.0);
        let strip = high(&c, &[[0.0, 0.0, 1280.0, 560.0]]).unwrap();
        assert!(strip < normal / 3.0, "a row where the list cannot go: {strip} < {normal}");
        // a Vertical list that cannot fit at full size shrinks (to half at most) instead of being drawn over something
        c.layout = 2;
        let list = high(&c, &[[0.0, 0.0, 1280.0, 450.0]]).unwrap();
        assert!(list < normal && list >= normal / 2.0 - 1.0, "shrunk to fit: {list} of {normal}");
        assert!(high(&c, &[[0.0, 0.0, 1280.0, 540.0]]).is_none(), "not even half size fits: left out");
    }

    /// Left-hand buttons (Place on LT, Drop on D-Down) go to the left of the screen, the rest to the right; a menu's A / B stay right; the two sizes
    /// are separate; the backdrop is one black quad per box at the opacity set.
    #[test]
    fn hints_split_left_right_with_separate_sizes_and_a_backdrop() {
        let (p, mut c) = pad();
        let quads = |c: &PadCfg, ui| {
            let mut v = Vec::new();
            p.hints(&mut v, c, &view(ui, &[]));
            v
        };
        let v = quads(&c, false);
        assert!(v.iter().any(|q| q.pos[0] < 640.0) && v.iter().any(|q| q.pos[0] > 640.0), "a box on each side");
        assert!(quads(&c, true).iter().all(|q| q.pos[0] > 640.0), "A / B are right-hand buttons");
        let height = |c: &PadCfg| {
            let v = quads(c, false);
            let ys = v.iter().map(|q| q.pos[1]);
            ys.clone().fold(0.0, f32::max) - ys.fold(f32::MAX, f32::min)
        };
        let normal = height(&c);
        c.hint_size = 2.0;
        assert!(height(&c) > normal, "Icon Size alone makes the rows taller");
        c.hint_size = 1.0;
        c.text_size = 2.0;
        assert!(height(&c) > normal, "Text Size alone makes the rows taller");
        c.text_size = 1.0;
        c.hint_back = 0.0;
        let bare = quads(&c, false).len();
        c.hint_back = 0.5;
        assert_eq!(quads(&c, false).len(), bare + 2 * 6, "one backdrop quad (6 vertices) per box, two boxes");
        assert!(quads(&c, false).iter().any(|q| q.color == [0.0, 0.0, 0.0, 0.5]));
    }

    #[test]
    fn hint_options_round_trip() {
        let mut c = PadCfg::default();
        assert_eq!((c.hint_size, c.layout), (1.0, 0));
        c.set_knob(6, 1.0);
        c.toggle(6);
        assert_eq!(c.toggle_text(6), "Hint Layout: Horizontal");
        assert_eq!(c.knob_text(6), "Icon Size: 200%");
        (c.text_size, c.hint_back) = (1.5, 0.25);
        let mut t = String::new();
        c.save_into(&mut t);
        let mut back = PadCfg::default();
        t.lines().for_each(|l| back.load(l.split_once(':').unwrap().0, l.split_once(':').unwrap().1));
        assert_eq!((back.hint_size, back.layout, back.text_size, back.hint_back), (2.0, 1, 1.5, 0.25));
        back.load("pad_hintSize", "9");
        back.load("pad_hintLayout", "9");
        assert_eq!((back.hint_size, back.layout), (2.0, 2), "out-of-range values are clamped");
    }

    #[test]
    fn drop_is_one_press_while_playing_and_follows_its_bind() {
        let (mut p, mut c) = pad();
        p.key(Btn::Down, true);
        assert!(p.update(DT, &c, false, 1000.0).drop, "D-Down drops by default");
        assert!(!p.update(DT, &c, false, 1000.0).drop, "one press, one drop");
        p.key(Btn::Down, false);
        c.binds[ACTS.iter().position(|a| *a == Act::Drop).unwrap()] = Btn::X;
        p.key(Btn::X, true);
        assert!(p.update(DT, &c, false, 1000.0).drop);
        p.key(Btn::X, false);
        p.update(DT, &c, true, 1000.0);
        p.key(Btn::X, true);
        p.update(DT, &c, true, 1000.0);
        assert!(!p.update(DT, &c, true, 1000.0).drop, "not in a GUI");
    }

    #[test]
    fn config_round_trips() {
        let mut c = PadCfg::default();
        (c.hints, c.style) = (false, 2);
        let mut t = String::new();
        c.save_into(&mut t);
        let mut back = PadCfg::default();
        t.lines().for_each(|l| back.load(l.split_once(':').unwrap().0, l.split_once(':').unwrap().1));
        assert_eq!((back.hints, back.style), (false, 2));
        let mut c = PadCfg::default();
        (c.binds[0], c.curve, c.invert_y, c.toggle_sneak) = (Btn::X, 4, true, false);
        c.set_knob(0, 0.25);
        let mut t = String::new();
        c.save_into(&mut t);
        let mut back = PadCfg::default();
        for l in t.lines() {
            let (k, v) = l.split_once(':').unwrap();
            back.load(k, v);
        }
        assert_eq!((back.binds[0], back.curve, back.invert_y, back.toggle_sneak), (Btn::X, 4, true, false));
        assert!((back.look_h - 0.5).abs() < 1e-4);
    }

    /// In a container screen the hints keep clear of the centred panel; X is named only while no bind has it; the cursor stays under 60% of a slot.
    #[test]
    fn hints_keep_off_the_panel_and_the_cursor_stays_small() {
        let (p, mut c) = pad();
        let (w, h) = (1604.0, 720.0);
        let avoid = vec![[w * 0.3, h * 0.2, w * 0.4, h * 0.6]];
        let view = HintView { w, h, s: 3.0, ui: true, avoid: &avoid };
        let rows = [(Role::Select, "Take"), (Role::Back, "Close")];
        let b = hint_box_with(&p, &c, &view, &rows).expect("there is room beside the panel");
        assert!(avoid.iter().all(|a| apart(b, *a, 12.0)), "{avoid:?} vs {b:?}");
        let quads = |c: &PadCfg, rows: &[(Role, &str)]| {
            let mut v = Vec::new();
            p.hints_with(&mut v, c, &view, rows);
            v.len()
        };
        let plain = quads(&c, &[(Role::Select, "Take"), (Role::Back, "Close")]);
        assert!(quads(&c, &[(Role::Select, "Take"), (Role::Alt, "Take half"), (Role::Back, "Close")]) > plain, "X is a row of its own");
        c.binds[ACTS.iter().position(|a| *a == Act::GuiBack).unwrap()] = Btn::X;
        assert_eq!(quads(&c, &[(Role::Select, "Take"), (Role::Alt, "Take half"), (Role::Back, "Close")]), plain, "X is bound: not named as place-one");
        // the cursor: a whole-pixel cross of 14 k, k shrunk until it fits the cap; no cap keeps the old size
        let cross = |cap: f32| {
            let mut v = Vec::new();
            p.draw(&mut v, w, h, 0, Some((500.0, 300.0)), cap);
            v.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, q| [b[0].min(q.pos[0]), b[1].min(q.pos[1]), b[2].max(q.pos[0]), b[3].max(q.pos[1])])
        };
        let slot_px = 40.0;
        let small = cross(0.6 * slot_px);
        assert!(small[2] - small[0] <= 0.6 * slot_px + 0.01 && small[3] - small[1] <= 0.6 * slot_px + 0.01, "{small:?} in a slot of {slot_px}");
        let free = cross(f32::INFINITY);
        assert!(free[2] - free[0] > small[2] - small[0] || (free[2] - free[0] - (small[2] - small[0])).abs() < 0.01, "the cap never makes it bigger");
    }
}
