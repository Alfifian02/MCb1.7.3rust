//! Controllify (isXander's Fabric mod, LGPL-3.0-or-later), ported to mc-rs: game-pad support on top of `TouchUi`, the one sink every input ends in.
//!
//! Ported from the Java: the default bindings (`default_bind/default.json`), the stick deadzone (`ControllerUtils.deadzone`), the look
//! curves (`InputCurves`, eased on the stick's length so aiming stays circular), 10 degrees per tick at 100% look sensitivity
//! (`InGameInputHandler.handleRegularLook`), the activation threshold of analogue triggers, toggle-sneak, the virtual mouse that drives
//! every GUI (`VirtualMouseHandler`), the radial hotbar (`HOTBAR_SLOT_SELECT`) and rumble on damage / block break / explosions.
//! Each binding is rebindable in Options > Controls > Controller (`options.rs`), saved in `options.txt` as `pad_*`.
//!
//! Not ported (nothing in mc-rs to hang them on): gyro, DualSense LEDs / adaptive triggers / touchpad, on-screen keyboard, button-guide
//! glyph packs, splitscreen, joystick remapping, the server vibration API, chat, swap hands, drop item
//! (b1.7.3 has no sprint / off hand, and mc-rs has no chat or drop yet). UNVERIFIED on real hardware: the Android axis / key mapping
//! follows the documented layout (X/Y left stick, Z/Rz right stick, Ltrigger/Rtrigger, hat = d-pad).

use super::touch_ui::{HOLD_SECS, TAP_SECS};
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
}

/// What a button can be bound to (Controllify's `ControlifyBindings`, the ones b1.7.3 has). Movement and look stay on the sticks.
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
}

pub const ACTS: [Act; 15] = [Act::Jump, Act::Sneak, Act::Attack, Act::Use, Act::Inventory, Act::Pause, Act::NextSlot, Act::PrevSlot, Act::Wheel, Act::Zoom, Act::GuiSelect, Act::GuiBack, Act::Sprint, Act::View, Act::Info];

impl Act {
    pub fn name(self) -> &'static str {
        ["Jump", "Sneak", "Attack", "Use", "Inventory", "Pause", "Next slot", "Prev slot", "Hotbar wheel", "Zoom", "GUI select", "GUI back", "Sprint", "View", "Info"][self as usize]
    }

    fn id(self) -> &'static str {
        ["jump", "sneak", "attack", "use", "inventory", "pause", "next_slot", "prev_slot", "wheel", "zoom", "gui_select", "gui_back", "sprint", "view", "info"][self as usize]
    }

    /// `default_bind/default.json` (zoom: its `change_perspective` button; the view bind is the d-pad's right, info its left).
    fn default(self) -> Btn {
        [Btn::A, Btn::R3, Btn::Rt, Btn::Lt, Btn::Y, Btn::Start, Btn::Rb, Btn::Lb, Btn::Up, Btn::Back, Btn::A, Btn::B, Btn::L3, Btn::Right, Btn::Left][self as usize]
    }
}

const KNOBS: [&str; 6] = ["Look X", "Look Y", "Deadzone", "Cursor", "Trigger", "Rumble"];
const KNOB_IDS: [&str; 6] = ["lookH", "lookV", "deadzone", "cursor", "threshold", "rumble"];
/// Each knob's value at slider 0 and 1.
const KNOB_RANGE: [(f32, f32); 6] = [(0.0, 2.0), (0.0, 2.0), (0.0, 0.5), (0.2, 2.0), (0.1, 0.9), (0.0, 1.0)];
const TOGGLES: [&str; 4] = ["Curve", "Invert X", "Invert Y", "Toggle Sneak"];
const TOGGLE_IDS: [&str; 4] = ["curve", "invertX", "invertY", "toggleSneak"];
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
        }
    }
}

impl PadCfg {
    fn bind(&self, a: Act) -> u16 {
        self.binds[a as usize].bit()
    }

    fn knob_value(&self, i: usize) -> f32 {
        [self.look_h, self.look_v, self.deadzone, self.cursor, self.threshold, self.rumble][i]
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
            _ => self.rumble = v,
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
            _ => self.toggle_sneak = !self.toggle_sneak,
        }
    }

    pub fn toggle_text(&self, i: usize) -> String {
        let onoff = |b: bool| if b { "ON" } else { "OFF" };
        let v = match i {
            0 => CURVES[self.curve as usize % CURVES.len()],
            1 => onoff(self.invert_x),
            2 => onoff(self.invert_y),
            _ => onoff(self.toggle_sneak),
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
                _ => self.toggle_sneak = n != 0,
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
        t.push_str(&format!("pad_curve:{}\npad_invertX:{}\npad_invertY:{}\npad_toggleSneak:{}\n", self.curve, b(self.invert_x), b(self.invert_y), b(self.toggle_sneak)));
    }
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
    pub strike: bool,
    /// C3: the attack button went up after a hold that dug nothing (`attack_dug` false, `HOLD_SECS` passed): the end of a charge.
    pub release: bool,
    pub inventory: bool,
    pub pause: bool,
    pub back: bool,
    pub slot_delta: i32,
    pub slot_to: Option<usize>,
    /// Virtual mouse move in px, and the press / release of the click.
    pub cursor: (f32, f32),
    pub click: Option<bool>,
    /// With `Pad::snap`: one step of the d-pad as (x, y), y down; the first on the press, then repeating while held.
    pub nav: Option<(i32, i32)>,
    /// In a GUI: LB / RB turn the book's page (-1 / +1), and the right stick scrolls (px, down is positive) like a finger dragging a list.
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
    /// The open screen jumps between its targets (`Out::nav`) instead of the d-pad nudging the free cursor, which the stick still pushes (set by `lib.rs`: the satchel).
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
            let free = |b: Btn| ![Act::GuiSelect, Act::GuiBack, Act::Pause, Act::Inventory].iter().any(|&a| cfg.binds[a as usize] == b);
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
        o.place |= o.strike;
        (o.inventory, o.pause) = (just(Act::Inventory), just(Act::Pause));
        o.slot_delta = just(Act::NextSlot) as i32 - just(Act::PrevSlot) as i32;
        o
    }

    /// The radial hotbar and, in a GUI, the cursor drawn over `cursor` (px).
    pub fn draw(&self, v: &mut Vec<HudVertex>, w: f32, h: f32, slot: usize, cursor: Option<(f32, f32)>) {
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
            HudPipeline::push_quad(v, x - k * 6.0 - k, y - k, k * 12.0 + 2.0 * k, k * 3.0, [0.0, 0.0, 0.0, 0.8]);
            HudPipeline::push_quad(v, x - k, y - k * 6.0 - k, k * 3.0, k * 12.0 + 2.0 * k, [0.0, 0.0, 0.0, 0.8]);
            HudPipeline::push_quad(v, x - k * 6.0, y, k * 12.0, k, [1.0; 4]);
            HudPipeline::push_quad(v, x, y - k * 6.0, k, k * 12.0, [1.0; 4]);
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
    fn config_round_trips() {
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
}
