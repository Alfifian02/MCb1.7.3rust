// Touch UX state machine, landscape layout. See mod.rs for an overview.
//
// Scheme (Minecraft PE's classic d-pad, two thumbs):
//   - Bottom left: the d-pad. Four arrows round a centre button; pressing an arrow shows the two corner buttons beside it
//     (up shows up-left and up-right, left shows up-left and down-left, ...), and sliding the finger onto one walks that
//     diagonal. The centre button is sneak (a toggle, like PE's). Sprites: the Stone D-pad pack.
//   - Anywhere else that is not a button: drag to look. A short tap places a block, a press held still
//     digs (hold to keep digging, the dig can follow the look), like Minecraft PE.
//   - Jump button: bottom right (held = keep jumping).
//   - Hotbar: bottom centre, tap to select. Pause: top-right, inventory button just left of it.
//   - Paused: the pause button toggles it; every other press is a tap on the game menu (`options::Pause`).
//   - A container screen is open (inventory / workbench): every press except the two buttons is a tap, reported with
//     `take_taps` for the screen to hit-test; move, look, jump and hotbar are off.
//
// SOURCE-OF-TRUTH NOTE: b1.7.3 Java has no touch UX, so every rectangle and
// radius here is UNVERIFIED (no reference to diff against; the d-pad is drawn after PE's, from the pack's screenshot). Sizes are
// fractions of the short screen side `u`, so they hold on any landscape
// aspect ratio. The hotbar slot count (9) and the {-1..1} forward/strafe
// semantics come from GuiIngame / MovementInputFromOptions.
//
// All positions are surface pixels, (0, 0) top-left (MotionEvent convention).

use android_activity::input::{Axis, MotionAction, MotionEvent};
use super::gamepad::{Pad, PadCfg};
use std::collections::HashMap;

/// Hotbar slot count.
pub const HOTBAR_SLOTS: usize = 9;
// UNVERIFIED: tap/hold timings and slop (b1.7.3 has no touch input). Picked to feel like PE.
/// The interval of the pad's use-repeat (`gamepad.rs`).
pub const TAP_SECS: f32 = 0.25;
/// A press held still this long starts digging, which lasts until the finger lifts; a shorter press that did not move is a tap
/// (place), so every press is a tap or a dig, whatever this is. 0.2 s felt too eager, 0.4 s is the first value.
pub const HOLD_SECS: f32 = 0.4;
/// The default of the Dig Delay (`TouchUi::swing_secs`): a look finger resting still this long on a block starts the arm swing
/// (`TouchUi::pressing`) and the dig, well before `HOLD_SECS`. A finger that moves more than the slop is a look drag and never
/// swings. UNVERIFIED on a device: 0.15 s is a guess, the slider in Controls is the answer.
pub const SWING_SECS: f32 = 0.15;
/// Finger travel (fraction of the short screen side) above which a press is a look drag.
const SLOP: f32 = 0.02;

/// (x, y, w, h) in pixels.
pub type Rect = (f32, f32, f32, f32);

/// What the finger on an open screen did, in order; the screen decides what a tap or a drag means.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScreenEv {
    Down(f32, f32),
    Move(f32, f32),
    Up(f32, f32),
}

/// What a single pointer is currently doing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointerRole {
    /// Press landed on the pause button. Toggles pause on release.
    Pause,
    /// Press landed on the zoom button. Zoomed while held (`TouchUi::zooming`).
    Zoom,
    /// Press landed on the view button (left of zoom). Cycles first person -> third -> shoulder surf (`TouchUi.third` / `shoulder`) on release.
    View,
    /// Press landed on the inventory button. Opens / closes the screen on release.
    Inventory,
    /// Any other press while a screen is open. Reported as a tap on release.
    Tap,
    /// Press landed on hotbar slot N. Selects on release.
    Hotbar(usize),
    /// Press landed on the jump button. Held = jumping.
    Jump,
    /// Press landed on the sneak button (the d-pad's centre). Toggles sneaking on release.
    Sneak,
    /// Finger on the d-pad; `TouchUi::dir` is the button under it.
    Dir,
    /// Look drag; deltas accumulate in `TouchUi::look_delta`.
    Look,
    /// Press landed on the guard button (Cortes Combat C4, only drawn and hit while `TouchUi::guard_shown`). Guarding while held.
    Guard,
    /// Press landed on the special button (Cortes Combat C5, same place as the guard button, only drawn and hit while `TouchUi::special_shown`). A press is one use.
    Special,
}

/// Live state of one finger on the glass.
#[derive(Clone, Copy, Debug)]
pub struct Pointer {
    pub role: PointerRole,
    pub last: (f32, f32),
    /// Seconds held (frame time) and pixels travelled since the press.
    pub held: f32,
    pub travel: f32,
    /// The hold turned into digging, so releasing is not a tap.
    pub broke: bool,
}

/// Screen-space layout (pixels, top-left origin).
#[derive(Clone, Debug)]
pub struct LayoutRects {
    pub pause: Rect,
    pub inventory: Rect,
    /// Hold to zoom (Zoomify), left of `inventory`.
    pub zoom: Rect,
    /// Tap to toggle the third-person view, left of `zoom`.
    pub view: Rect,
    pub hotbar: [Rect; HOTBAR_SLOTS],
    pub jump: Rect,
    /// The guard button, above `jump` (C4; hit only while `TouchUi::guard_shown`).
    pub guard: Rect,
    /// The special button (C5): where the guard button is, since no weapon has both (only the sword guards, and it has no special).
    pub special: Rect,
    /// The sneak button, the d-pad's centre cell.
    pub sneak: Rect,
    /// The d-pad's 8 cells (`DIRS` order: up, down, left, right, up_left, up_right, down_left, down_right). They tile without a gap, so
    /// a finger sliding between two buttons never falls through; a button is drawn `dpad_gap` smaller than its cell.
    pub dpad: [Rect; 8],
    pub dpad_gap: f32,
}

fn in_rect(r: Rect, x: f32, y: f32) -> bool {
    x >= r.0 && x <= r.0 + r.2 && y >= r.1 && y <= r.1 + r.3
}

impl LayoutRects {
    /// Layout for a `w` x `h` surface, tuned for landscape (works in portrait
    /// too, just cramped). Everything scales with the short side.
    pub fn for_surface(w: u32, h: u32) -> Self {
        let w = w as f32;
        let h = h as f32;
        let u = w.min(h);
        let pad = 0.04 * u;

        let ps = 0.11 * u;
        let pause = (w - ps - pad, pad, ps, ps);
        // UNVERIFIED: b1.7.3 opens the inventory with a key; this button's place is invented for touch.
        let inventory = (w - ps * 2.0 - pad * 1.5, pad, ps, ps);
        let zoom = (w - ps * 3.0 - pad * 2.0, pad, ps, ps);
        // UNVERIFIED: b1.7.3 toggles the view with F5; this button's place is invented for touch.
        let view = (w - ps * 4.0 - pad * 2.5, pad, ps, ps);

        // Square cells; capped so nine of them always fit the width.
        let cell = (0.105 * u).min(w / 9.5);
        let hb_x0 = (w - cell * HOTBAR_SLOTS as f32) * 0.5;
        let hb_y = h - cell - pad;
        let mut hotbar = [(0.0, 0.0, 0.0, 0.0); HOTBAR_SLOTS];
        for (i, slot) in hotbar.iter_mut().enumerate() {
            *slot = (hb_x0 + cell * i as f32, hb_y, cell, cell);
        }

        // The d-pad: a 3 x 3 block of square cells, its bottom edge half a pad above the hotbar row, so it clears the hotbar at any ratio.
        let dpad_gap = 0.012 * u;
        let step = 0.12 * u + dpad_gap;
        let (cx, cy) = (pad * 1.5 + step * 1.5, hb_y - pad * 0.5 - step * 1.5);
        let cell = |col: f32, row: f32| (cx + (col - 0.5) * step, cy + (row - 0.5) * step, step, step);
        // (column, row) offsets from the centre, in `DIRS` order.
        let dpad = [(0.0, -1.0), (0.0, 1.0), (-1.0, 0.0), (1.0, 0.0), (-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)].map(|(c, r)| cell(c, r));
        let jw = 0.16 * u;
        let jump = (w - pad * 1.5 - jw, cy - jw * 0.5, jw, jw);

        let gs = 0.13 * u;
        let guard = (jump.0 + (jw - gs) * 0.5, jump.1 - gs - pad, gs, gs);

        Self { pause, inventory, zoom, view, hotbar, jump, guard, special: guard, sneak: cell(0.0, 0.0), dpad, dpad_gap }
    }

    /// Hit-test a press. `None` means the press is ignored. The pause button always answers; while paused everything else is a tap on
    /// the game menu (`options::Pause`); with a `screen` open everything but the pause and inventory buttons is a tap.
    pub fn hit_test(&self, x: f32, y: f32, paused: bool, screen: bool) -> Option<PointerRole> {
        if in_rect(self.pause, x, y) {
            return Some(PointerRole::Pause);
        }
        if paused {
            return Some(PointerRole::Tap);
        }
        if in_rect(self.inventory, x, y) {
            return Some(PointerRole::Inventory);
        }
        if screen {
            return Some(PointerRole::Tap);
        }
        if in_rect(self.zoom, x, y) {
            return Some(PointerRole::Zoom);
        }
        if in_rect(self.view, x, y) {
            return Some(PointerRole::View);
        }
        if in_rect(self.jump, x, y) {
            return Some(PointerRole::Jump);
        }
        if in_rect(self.sneak, x, y) {
            return Some(PointerRole::Sneak);
        }
        // Only the arrows start a press; a corner is reached by sliding from an arrow (it is not even drawn before).
        if self.dpad[..4].iter().any(|&r| in_rect(r, x, y)) {
            return Some(PointerRole::Dir);
        }
        if let Some(i) = self.hotbar.iter().position(|&r| in_rect(r, x, y)) {
            return Some(PointerRole::Hotbar(i));
        }
        Some(PointerRole::Look)
    }
}

const H: f32 = std::f32::consts::FRAC_1_SQRT_2;
/// What each d-pad button walks, as (forward, side): up, down, left, right, then the corners up_left, up_right, down_left, down_right.
/// A corner is normalised like `Entity.moveFlying` does (it divides by the length when that is above 1): a diagonal is no faster.
pub const DIRS: [(f32, f32); 8] = [(1.0, 0.0), (-1.0, 0.0), (0.0, -1.0), (0.0, 1.0), (H, -H), (H, H), (-H, -H), (-H, H)];
/// The two corners (`DIRS` indices) beside each arrow: the ones that appear while it is held.
const NEXT: [[usize; 2]; 4] = [[4, 5], [6, 7], [4, 6], [5, 7]];

/// Whole touch UX state. Owned by `App`.
pub struct TouchUi {
    pub paused: bool,
    pub hotbar_slot: usize,
    /// Movement in the camera frame: (fwd, side), a `DIRS` entry or zero.
    pub move_input: (f32, f32),
    /// The d-pad button under the finger (`DIRS` index), `None` when no finger is on the pad or it left every button.
    pub dir: Option<usize>,
    /// Which corner buttons (indices 4..8, as `[i - 4]`) are showing: the two beside the arrow the finger last rested on.
    shown: [bool; 4],
    /// Sneaking: the centre button toggles it. Kept across pause and screens (only the fingers are dropped).
    pub sneak: bool,
    /// TEMPORARY fly mode (the pause menu's Fly button); kept over pause like `sneak`.
    pub flying: bool,
    /// Third-person view (`GameSettings.thirdPersonView`), toggled by the view button, kept over pause.
    pub third: bool,
    /// With `third`: the cinematic over-the-shoulder camera (`render::shoulder`) instead of the 4-blocks-behind one. The view button's second tap.
    pub shoulder: bool,
    /// The view button was tapped in the shoulder view: swap shoulders (`take_swap`). A hold there goes on to the next view instead.
    swap: bool,
    pub pointers: HashMap<i32, Pointer>,
    pub surface_w: u32,
    pub surface_h: u32,
    pub layout: LayoutRects,
    /// Look drag pixels since the last `take_look`.
    look_delta: (f32, f32),
    /// A tap fired since the last `take_place`.
    place: bool,
    /// A container screen is open (set by `set_screen`); presses become taps.
    pub screen: bool,
    /// The inventory button was released since the last `take_inventory`.
    inventory: bool,
    /// A menu screen (`menu::Menu`) is up: every press is a tap, the pause and inventory buttons included.
    pub menu: bool,
    /// The pack's table is open (`world::pack`): its buttons are 3D ones at the edges of the view, so the four HUD buttons are hidden
    /// and every press is a tap.
    pub pack_open: bool,
    /// Finger events on an open screen since the last `take_screen_events`.
    events: Vec<ScreenEv>,
    /// Where the finger last was on the open screen: the picked-up stack floats here.
    pub cursor_pos: (f32, f32),
    /// The game pad (`gamepad.rs`): its outputs below are OR-ed with the fingers'.
    pub pad: Pad,
    pad_jump: bool,
    /// The Dig Delay in seconds (`Settings::dig_secs`, set by `lib.rs` each frame).
    pub swing_secs: f32,
    /// The held item can guard (`lib.rs` sets it each frame, C4): the guard button is drawn and answers presses.
    pub guard_shown: bool,
    /// The held item has a right-button special (`lib.rs` sets it each frame, C5): the special button is drawn and answers presses.
    pub special_shown: bool,
    /// A press on the special button since the last `take_special`.
    special_hit: bool,
    pad_dig: bool,
    pad_attack: bool,
    pad_zoom: bool,
    /// The pad's L3 is held (`gamepad::Out::sprint`): the sprint key.
    pub pad_sprint: bool,
    /// Look from the right stick since the last `take_pad_look`, radians (yaw right, pitch down).
    pad_look: (f32, f32),
    /// The pad's attack tap: hit what is under the crosshair but never place a block.
    strike: bool,
    /// C3: a still look finger (or the pad's attack) lifted after a hold, `HOLD_SECS` or more: not a tap, the end of a charge.
    release: bool,
    /// A d-pad step on a screen that jumps between its targets (`Pad::snap`), until `take_nav`.
    nav: Option<(i32, i32)>,
    /// B in a container screen, LB / RB pages (-1 / +1) and the right stick's scroll (px), until taken: `lib.rs` decides what they mean.
    back: bool,
    pad_info: bool,
    page: i32,
    scroll: f32,
    /// X (`Some(false)`, click for one) or RT (`Some(true)`, quick-move) in a container screen, until `take_alt`.
    alt: Option<bool>,
}

impl TouchUi {
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            paused: false,
            hotbar_slot: 0,
            move_input: (0.0, 0.0),
            dir: None,
            shown: [false; 4],
            sneak: false,
            flying: false,
            third: false,
            shoulder: false,
            swap: false,
            pointers: HashMap::new(),
            surface_w: w,
            surface_h: h,
            layout: LayoutRects::for_surface(w, h),
            look_delta: (0.0, 0.0),
            place: false,
            screen: false,
            inventory: false,
            menu: false,
            pack_open: false,
            events: Vec::new(),
            cursor_pos: (w as f32 * 0.5, h as f32 * 0.5),
            pad: Pad::default(),
            pad_jump: false,
            swing_secs: SWING_SECS,
            guard_shown: false,
            special_shown: false,
            special_hit: false,
            pad_dig: false,
            pad_attack: false,
            pad_zoom: false,
            pad_sprint: false,
            pad_look: (0.0, 0.0),
            strike: false,
            release: false,
            nav: None,
            back: false,
            pad_info: false,
            page: 0,
            scroll: 0.0,
            alt: None,
        }
    }

    /// One frame of the game pad: drives the same state the fingers do (move, look, jump, dig, place, hotbar, inventory, pause), or in a
    /// GUI (pause menu, container, title menu) a virtual mouse that sends the `ScreenEv`s a finger would.
    pub fn pad_update(&mut self, dt: f32, cfg: &PadCfg) {
        let ui = self.paused || self.screen || self.menu;
        let o = self.pad.update(dt, cfg, ui, self.surface_w.min(self.surface_h) as f32);
        if ui {
            let (w, h) = (self.surface_w as f32, self.surface_h as f32);
            let p = (self.cursor_pos.0 + o.cursor.0, self.cursor_pos.1 + o.cursor.1);
            let p = (p.0.clamp(0.0, w - 1.0), p.1.clamp(0.0, h - 1.0));
            if p != self.cursor_pos {
                self.cursor_pos = p;
                self.events.push(ScreenEv::Move(p.0, p.1));
            }
            self.nav = o.nav.or(self.nav);
            match o.click {
                Some(true) => self.events.push(ScreenEv::Down(p.0, p.1)),
                Some(false) => self.events.push(ScreenEv::Up(p.0, p.1)),
                None => {}
            }
            (self.page, self.scroll) = (self.page + o.page, self.scroll + o.scroll);
            if self.screen && !self.paused && !self.menu {
                self.alt = o.alt.or(self.alt);
            }
            if o.back && !self.paused && self.screen && !self.menu {
                self.back = true; // the screen steps back one level first (`take_back`); closing is `lib.rs`'s call
            } else if o.pause || o.back {
                if self.paused {
                    self.resume();
                } else if self.screen && !self.menu {
                    self.inventory = true;
                }
            } else if o.inventory && self.screen && !self.menu {
                self.inventory = true;
            }
            self.pad_jump = false;
            self.pad_dig = false;
            self.pad_attack = false;
            self.pad_zoom = false;
            self.pad_sprint = false;
            return;
        }
        if let Some(m) = o.mv {
            self.move_input = m;
        }
        self.pad_look.0 += o.look.0;
        self.pad_look.1 += o.look.1;
        (self.pad_jump, self.pad_dig, self.pad_attack, self.pad_zoom) = (o.jump, o.dig, o.attack, o.zoom);
        self.pad_sprint = o.sprint;
        if o.view {
            self.cycle_view();
        }
        self.pad_info |= o.info;
        if cfg.toggle_sneak {
            self.sneak ^= o.sneak_down;
        } else if o.sneak_down {
            self.sneak = true;
        } else if o.sneak_up {
            self.sneak = false;
        }
        self.place |= o.place;
        self.strike |= o.strike;
        self.release |= o.release;
        self.inventory |= o.inventory;
        if o.pause {
            self.pause();
        }
        let n = crate::input::touch_ui::HOTBAR_SLOTS as i32;
        self.hotbar_slot = o.slot_to.unwrap_or((self.hotbar_slot as i32 + o.slot_delta).rem_euclid(n) as usize);
    }

    /// The right stick's look since the last call, radians.
    pub fn take_pad_look(&mut self) -> (f32, f32) {
        std::mem::take(&mut self.pad_look)
    }

    /// First person -> third -> over the shoulder -> first (the view button's tap, the pad's view bind).
    fn cycle_view(&mut self) {
        (self.third, self.shoulder) = match (self.third, self.shoulder) {
            (false, _) => (true, false),
            (true, false) => (true, true),
            (true, true) => (false, false),
        };
    }

    /// The pad's info bind was pressed since the last call.
    pub fn take_pad_info(&mut self) -> bool {
        std::mem::take(&mut self.pad_info)
    }

    /// Close the open screen as the inventory button does.
    pub fn press_inventory(&mut self) {
        self.inventory = true;
    }

    /// B pressed in a container screen, the pages turned and the pixels scrolled since the last call.
    pub fn take_back(&mut self) -> bool {
        std::mem::take(&mut self.back)
    }

    pub fn take_page(&mut self) -> i32 {
        std::mem::take(&mut self.page)
    }

    pub fn take_scroll(&mut self) -> f32 {
        std::mem::take(&mut self.scroll)
    }

    /// The pad's X / RT in a container screen since the last call: `Some(false)` clicks the slot under the cursor for one, `Some(true)` quick-moves it.
    pub fn take_alt(&mut self) -> Option<bool> {
        self.alt.take()
    }

    /// The pad's step since the last call (only while `pad.snap`): `lib.rs` moves the cursor to the neighbouring target.
    pub fn take_nav(&mut self) -> Option<(i32, i32)> {
        self.nav.take()
    }

    /// Put the cursor at `p` as a finger sliding there would (a held press spreads its stack over what the cursor crosses).
    pub fn warp_cursor(&mut self, p: (f32, f32)) {
        if p != self.cursor_pos {
            self.cursor_pos = p;
            self.events.push(ScreenEv::Move(p.0, p.1));
        }
    }

    /// A still hold ended (finger up, or the pad's attack released) since the last call: Cortes Combat's charged blow (C3).
    pub fn take_release(&mut self) -> bool {
        std::mem::take(&mut self.release)
    }

    /// The pad's attack tap since the last call.
    pub fn take_strike(&mut self) -> bool {
        std::mem::take(&mut self.strike)
    }

    /// Leave the pause menu ("Back to game", "Respawn").
    pub fn resume(&mut self) {
        self.paused = false;
        self.clear();
    }

    /// A menu screen opened or closed: like `set_screen`, but nothing on the glass is a button of the game.
    pub fn set_menu(&mut self, open: bool) {
        self.menu = open;
        self.paused = false;
        self.set_screen(open);
    }

    /// A press on the special button since the last call (C5).
    pub fn take_special(&mut self) -> bool {
        std::mem::take(&mut self.special_hit)
    }

    /// A finger is on the guard button (C4).
    pub fn guarding(&self) -> bool {
        self.pointers.values().any(|p| p.role == PointerRole::Guard)
    }

    /// A finger is on the zoom button.
    pub fn zooming(&self) -> bool {
        self.pad_zoom || self.pointers.values().any(|p| p.role == PointerRole::Zoom)
    }

    /// The inventory button was pressed and released since the last call.
    pub fn take_inventory(&mut self) -> bool {
        std::mem::take(&mut self.inventory)
    }

    /// Finger events on an open screen since the last call, in surface pixels.
    pub fn take_screen_events(&mut self) -> Vec<ScreenEv> {
        std::mem::take(&mut self.events)
    }

    /// A screen opened or closed: drop every finger so none stays stuck as move, look or dig.
    pub fn set_screen(&mut self, open: bool) {
        self.screen = open;
        self.cursor_pos = (self.surface_w as f32 * 0.5, self.surface_h as f32 * 0.5);
        self.alt = None;
        self.clear();
    }

    /// A tap (place) was requested since the last call.
    pub fn take_place(&mut self) -> bool {
        std::mem::take(&mut self.place)
    }

    /// The arm may swing: the pad's attack button is down, or a look finger has rested still for `SWING_SECS` (a drag has not).
    pub fn pressing(&self) -> bool {
        self.pad_attack || self.resting()
    }

    /// A look finger has rested still for `SWING_SECS` (a drag has not), or the pad's attack has been down that long.
    pub fn resting(&self) -> bool {
        self.pad.resting(self.swing_secs) || self.pointers.values().any(|p| p.role == PointerRole::Look && p.held >= self.swing_secs && p.travel < self.surface_w.min(self.surface_h) as f32 * SLOP)
    }

    /// The resting look finger(s) start digging now, without waiting for `HOLD_SECS` (the caller saw a block, not a mob, under the
    /// crosshair); the release is no tap, as for a hold.
    pub fn dig_now(&mut self) {
        self.pad.dig_now(self.swing_secs);
        self.pad_dig |= self.pad.resting(self.swing_secs);
        let slop = self.surface_w.min(self.surface_h) as f32 * SLOP;
        self.pointers.values_mut().filter(|p| p.role == PointerRole::Look && p.held >= self.swing_secs && p.travel < slop).for_each(|p| p.broke = true);
    }

    /// True while a look finger that was held still is down: the player is digging.
    pub fn digging(&self) -> bool {
        self.pad_dig || self.pointers.values().any(|p| p.role == PointerRole::Look && p.broke)
    }

    /// Advance the hold timers by one frame: a look finger held still turns into digging after `HOLD_SECS`.
    pub fn tick(&mut self, dt: f32) {
        let slop = self.surface_w.min(self.surface_h) as f32 * SLOP;
        for p in self.pointers.values_mut().filter(|p| matches!(p.role, PointerRole::Look | PointerRole::View) && !p.broke && p.travel < slop) {
            p.held += dt;
            p.broke = p.held >= HOLD_SECS;
        }
    }

    /// True once per tap of the view button in the shoulder view.
    pub fn take_swap(&mut self) -> bool {
        std::mem::take(&mut self.swap)
    }

    /// Read and clear the accumulated look drag (pixels).
    pub fn take_look(&mut self) -> (f32, f32) {
        std::mem::take(&mut self.look_delta)
    }

    /// True while a finger holds the jump button.
    pub fn jumping(&self) -> bool {
        self.pad_jump || self.pointers.values().any(|p| p.role == PointerRole::Jump)
    }

    /// Is d-pad button `i` drawn and pressable? The arrows always are; a corner only while an arrow beside it is held.
    pub fn dpad_shown(&self, i: usize) -> bool {
        i < 4 || self.shown[i - 4]
    }

    /// The visible d-pad button under (x, y).
    fn dir_at(&self, x: f32, y: f32) -> Option<usize> {
        (0..8).find(|&i| self.dpad_shown(i) && in_rect(self.layout.dpad[i], x, y))
    }

    /// The finger is on button `d` (`None`: it left the pad). Landing on an arrow shows its two corners; a corner leaves them as they are,
    /// so the finger can slide arrow -> corner -> next arrow.
    fn set_dir(&mut self, d: Option<usize>) {
        self.dir = d;
        self.move_input = d.map_or((0.0, 0.0), |d| DIRS[d]);
        if let Some(d) = d.filter(|&d| d < 4) {
            self.shown = [false; 4];
            for c in NEXT[d] {
                self.shown[c - 4] = true;
            }
        }
    }

    /// Open the pause menu (the death screen reuses it: its resume button is the respawn button).
    pub fn pause(&mut self) {
        self.paused = true;
        self.clear();
    }

    /// Drop every finger and zero all input (pause, cancel, resize).
    fn clear(&mut self) {
        self.pointers.clear();
        self.dir = None;
        self.shown = [false; 4];
        self.move_input = (0.0, 0.0);
        self.look_delta = (0.0, 0.0);
        self.place = false;
        self.release = false;
        self.events.clear();
    }

    /// Rebuild the layout if the surface size changed. True if it did.
    pub fn ensure_layout(&mut self, w: u32, h: u32) -> bool {
        if self.surface_w == w && self.surface_h == h {
            return false;
        }
        self.surface_w = w;
        self.surface_h = h;
        self.layout = LayoutRects::for_surface(w, h);
        self.clear();
        true
    }

    /// What a press at (x, y) starts: in a menu everything is a tap.
    fn role_at(&self, x: f32, y: f32) -> Option<PointerRole> {
        if self.menu || (self.screen && self.pack_open) {
            return Some(PointerRole::Tap);
        }
        let role = self.layout.hit_test(x, y, self.paused, self.screen);
        if self.guard_shown && role == Some(PointerRole::Look) && in_rect(self.layout.guard, x, y) {
            Some(PointerRole::Guard)
        } else if self.special_shown && role == Some(PointerRole::Look) && in_rect(self.layout.special, x, y) {
            Some(PointerRole::Special)
        } else {
            role
        }
    }

    pub fn handle_motion(&mut self, ev: &MotionEvent) {
        match ev.action() {
            MotionAction::Down | MotionAction::PointerDown => {
                let p = ev.pointer_at_index(ev.pointer_index());
                let (x, y) = (p.axis_value(Axis::X), p.axis_value(Axis::Y));
                if let Some(role) = self.role_at(x, y) {
                    self.on_press(p.pointer_id(), x, y, role);
                }
            }
            MotionAction::Move => {
                for p in ev.pointers() {
                    self.on_move(p.pointer_id(), p.axis_value(Axis::X), p.axis_value(Axis::Y));
                }
            }
            MotionAction::Up | MotionAction::PointerUp => {
                let p = ev.pointer_at_index(ev.pointer_index());
                self.on_release(p.pointer_id());
            }
            MotionAction::Cancel => self.clear(),
            _ => {}
        }
    }

    fn on_press(&mut self, pid: i32, x: f32, y: f32, role: PointerRole) {
        // One d-pad finger and one look finger at a time; extras are ignored.
        if matches!(role, PointerRole::Dir | PointerRole::Look | PointerRole::Tap)
            && self
                .pointers
                .values()
                .any(|p| std::mem::discriminant(&p.role) == std::mem::discriminant(&role))
        {
            return;
        }
        if role == PointerRole::Tap {
            self.events.push(ScreenEv::Down(x, y));
            self.cursor_pos = (x, y);
        }
        if role == PointerRole::Dir {
            self.set_dir(self.dir_at(x, y));
        }
        self.special_hit |= role == PointerRole::Special;
        self.pointers.insert(pid, Pointer { role, last: (x, y), held: 0.0, travel: 0.0, broke: false });
    }

    fn on_move(&mut self, pid: i32, x: f32, y: f32) {
        let Some(ptr) = self.pointers.get_mut(&pid) else { return };
        let (lx, ly) = ptr.last;
        ptr.last = (x, y);
        match ptr.role {
            PointerRole::Look => {
                self.look_delta.0 += x - lx;
                self.look_delta.1 += y - ly;
                ptr.travel += (x - lx).hypot(y - ly);
            }
            PointerRole::Dir => {
                let d = self.dir_at(x, y);
                if d != self.dir {
                    self.set_dir(d);
                }
            }
            PointerRole::Tap => {
                self.events.push(ScreenEv::Move(x, y));
                self.cursor_pos = (x, y);
            }
            _ => {}
        }
    }

    fn on_release(&mut self, pid: i32) {
        let Some(ptr) = self.pointers.remove(&pid) else { return };
        match ptr.role {
            PointerRole::Pause => {
                self.paused = !self.paused;
                self.clear();
                log::info!("touch: pause -> {}", self.paused);
            }
            PointerRole::Inventory => self.inventory = true,
            PointerRole::View if self.third && self.shoulder && !ptr.broke => self.swap = true,
            PointerRole::View => self.cycle_view(),
            PointerRole::Tap => {
                self.events.push(ScreenEv::Up(ptr.last.0, ptr.last.1));
                self.cursor_pos = ptr.last;
            }
            PointerRole::Hotbar(slot) => self.hotbar_slot = slot,
            PointerRole::Dir => {
                self.set_dir(None);
                self.shown = [false; 4];
            }
            PointerRole::Sneak => self.sneak = !self.sneak,
            PointerRole::Look => {
                let slop = self.surface_w.min(self.surface_h) as f32 * SLOP;
                if !ptr.broke && ptr.travel < slop {
                    self.place = true;
                } else if ptr.broke && ptr.travel < slop {
                    self.release = true; // a hold that stayed put: `lib.rs` turns it into a charged blow if one was building
                }
            }
            PointerRole::Jump | PointerRole::Zoom | PointerRole::Guard | PointerRole::Special => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: u32 = 2400;
    const H: u32 = 1080;

    fn approx(a: f32, b: f32) {
        assert!((a - b).abs() < 0.001, "{a} != {b}");
    }

    fn mid(r: Rect) -> (f32, f32) {
        (r.0 + r.2 * 0.5, r.1 + r.3 * 0.5)
    }

    fn tap(ui: &mut TouchUi, pid: i32, x: f32, y: f32) {
        let role = ui.layout.hit_test(x, y, ui.paused, ui.screen).expect("hit");
        ui.on_press(pid, x, y, role);
        ui.on_release(pid);
    }

    #[test]
    fn regions_hit_where_expected() {
        let l = LayoutRects::for_surface(W, H);
        let (px, py, pw, ph) = l.pause;
        assert_eq!(l.hit_test(px + pw * 0.5, py + ph * 0.5, false, false), Some(PointerRole::Pause));
        let (jx, jy) = mid(l.jump);
        assert_eq!(l.hit_test(jx, jy, false, false), Some(PointerRole::Jump));
        let (sx, sy) = mid(l.sneak);
        assert_eq!(l.hit_test(sx, sy, false, false), Some(PointerRole::Sneak));
        for i in 0..4 {
            let (dx, dy) = mid(l.dpad[i]);
            assert_eq!(l.hit_test(dx, dy, false, false), Some(PointerRole::Dir), "arrow {i}");
        }
        // A corner cell is no button until an arrow is held: a press there is a look.
        let (cx, cy) = mid(l.dpad[4]);
        assert_eq!(l.hit_test(cx, cy, false, false), Some(PointerRole::Look));
        for i in 0..HOTBAR_SLOTS {
            let (hx, hy, hw, hh) = l.hotbar[i];
            assert_eq!(l.hit_test(hx + hw * 0.5, hy + hh * 0.5, false, false), Some(PointerRole::Hotbar(i)));
        }
        assert_eq!(l.hit_test(1000.0, 300.0, false, false), Some(PointerRole::Look));
        assert_eq!(l.hit_test(1800.0, 400.0, false, false), Some(PointerRole::Look));
    }

    #[test]
    fn buttons_do_not_overlap_in_common_landscape_ratios() {
        // 16:9, 20:9 and 4:3: the jump button and the whole d-pad must stay above the hotbar row and inside the screen.
        for (w, h) in [(1920, 1080), (2400, 1080), (1440, 1080)] {
            let l = LayoutRects::for_surface(w, h);
            assert!(l.jump.1 + l.jump.3 < l.hotbar[0].1, "{w}x{h}");
            assert!(l.dpad.iter().chain([&l.sneak]).all(|r| r.0 >= 0.0 && r.1 >= 0.0 && r.1 + r.3 < l.hotbar[0].1), "{w}x{h}");
            assert!(l.jump.0 + l.jump.2 <= w as f32);
        }
    }

    /// Paused, the pause button still answers and everything else is a tap on the game menu; `resume` closes it.
    #[test]
    fn paused_presses_are_taps_on_the_game_menu() {
        let l = LayoutRects::for_surface(W, H);
        let (px, py) = mid(l.pause);
        assert_eq!(l.hit_test(px, py, true, false), Some(PointerRole::Pause));
        assert_eq!(l.hit_test(1800.0, 400.0, true, false), Some(PointerRole::Tap));
        let (jx, jy) = mid(l.jump);
        assert_eq!(l.hit_test(jx, jy, true, false), Some(PointerRole::Tap));
        let mut ui = TouchUi::new(W, H);
        ui.paused = true;
        tap(&mut ui, 1, 1800.0, 400.0);
        assert_eq!(ui.take_screen_events(), vec![ScreenEv::Down(1800.0, 400.0), ScreenEv::Up(1800.0, 400.0)]);
        ui.resume();
        assert!(!ui.paused);
        ui.set_menu(true);
        let (px, py) = mid(ui.layout.pause);
        assert_eq!(ui.role_at(px, py), Some(PointerRole::Tap), "a menu screen takes every press");
    }

    /// An arrow shows the two corners beside it, a corner walks the diagonal (normalised), sliding to the next arrow swaps the corners,
    /// and lifting the finger stops and hides them all. Every arrow / corner pairing of the eight-way pad is checked.
    #[test]
    fn dpad_arrow_shows_its_two_corners_and_walks_diagonally() {
        let mut ui = TouchUi::new(W, H);
        let at = |ui: &TouchUi, i: usize| mid(ui.layout.dpad[i]);
        assert!((4..8).all(|i| !ui.dpad_shown(i)) && (0..4).all(|i| ui.dpad_shown(i)));
        // Each arrow brings up exactly its two neighbours (up: up_left + up_right, left: up_left + down_left, ...).
        for (arrow, corners) in [(0, [4, 5]), (1, [6, 7]), (2, [4, 6]), (3, [5, 7])] {
            let (x, y) = at(&ui, arrow);
            ui.on_press(1, x, y, PointerRole::Dir);
            assert_eq!(ui.dir, Some(arrow));
            assert_eq!(ui.move_input, DIRS[arrow]);
            assert!((4..8).all(|i| ui.dpad_shown(i) == corners.contains(&i)), "arrow {arrow}");
            ui.on_release(1);
            assert!(ui.dir.is_none() && ui.move_input == (0.0, 0.0) && (4..8).all(|i| !ui.dpad_shown(i)));
        }
        // Up, slide to up_right: forward + right at 1 / sqrt 2 each; both corners stay; then on to the right arrow.
        let (x, y) = at(&ui, 0);
        ui.on_press(1, x, y, PointerRole::Dir);
        let (x, y) = at(&ui, 5);
        ui.on_move(1, x, y);
        assert_eq!(ui.dir, Some(5));
        approx(ui.move_input.0, super::H);
        approx(ui.move_input.1, super::H);
        approx((ui.move_input.0.powi(2) + ui.move_input.1.powi(2)).sqrt(), 1.0);
        assert!(ui.dpad_shown(4) && ui.dpad_shown(5));
        let (x, y) = at(&ui, 3);
        ui.on_move(1, x, y);
        assert_eq!(ui.move_input, (0.0, 1.0));
        assert!(ui.dpad_shown(5) && ui.dpad_shown(7) && !ui.dpad_shown(4));
        // Off the pad: stop, but the corners stay so the finger can come back.
        ui.on_move(1, 1500.0, 100.0);
        assert!(ui.dir.is_none() && ui.move_input == (0.0, 0.0) && ui.dpad_shown(7));
        ui.on_release(1);
        assert!((4..8).all(|i| !ui.dpad_shown(i)));
    }

    /// The centre button toggles sneaking on release; it survives pause and the fingers being dropped.
    #[test]
    fn sneak_button_toggles_and_survives_pause() {
        let mut ui = TouchUi::new(W, H);
        let (x, y) = mid(ui.layout.sneak);
        tap(&mut ui, 1, x, y);
        assert!(ui.sneak);
        let (px, py, pw, ph) = ui.layout.pause;
        tap(&mut ui, 2, px + pw * 0.5, py + ph * 0.5);
        assert!(ui.paused && ui.sneak);
        tap(&mut ui, 3, px + pw * 0.5, py + ph * 0.5);
        tap(&mut ui, 4, x, y);
        assert!(!ui.sneak);
    }

    #[test]
    fn second_dpad_finger_is_ignored() {
        let mut ui = TouchUi::new(W, H);
        let (x, y) = mid(ui.layout.dpad[0]);
        ui.on_press(1, x, y, PointerRole::Dir);
        let (x, y) = mid(ui.layout.dpad[3]);
        ui.on_press(2, x, y, PointerRole::Dir);
        assert_eq!(ui.pointers.len(), 1);
        assert_eq!(ui.dir, Some(0));
    }

    #[test]
    fn look_drag_accumulates_and_clears() {
        let mut ui = TouchUi::new(W, H);
        ui.on_press(1, 1800.0, 400.0, PointerRole::Look);
        ui.on_move(1, 1810.0, 395.0);
        ui.on_move(1, 1830.0, 395.0);
        assert_eq!(ui.take_look(), (30.0, -5.0));
        assert_eq!(ui.take_look(), (0.0, 0.0));
    }

    #[test]
    fn jump_is_held_not_latched() {
        let mut ui = TouchUi::new(W, H);
        assert!(!ui.jumping());
        ui.on_press(1, 2200.0, 750.0, PointerRole::Jump);
        assert!(ui.jumping());
        ui.on_release(1);
        assert!(!ui.jumping());
    }

    #[test]
    fn single_finger_taps_work() {
        let mut ui = TouchUi::new(W, H);
        let (hx, hy, hw, hh) = ui.layout.hotbar[3];
        tap(&mut ui, 1, hx + hw * 0.5, hy + hh * 0.5);
        assert_eq!(ui.hotbar_slot, 3);
        let (px, py, pw, ph) = ui.layout.pause;
        tap(&mut ui, 2, px + pw * 0.5, py + ph * 0.5);
        assert!(ui.paused);
        tap(&mut ui, 3, px + pw * 0.5, py + ph * 0.5);
        assert!(!ui.paused);
    }

    #[test]
    fn pausing_zeroes_stuck_input() {
        let mut ui = TouchUi::new(W, H);
        let (x, y) = mid(ui.layout.dpad[0]);
        ui.on_press(1, x, y, PointerRole::Dir);
        assert_ne!(ui.move_input, (0.0, 0.0));
        let (px, py, pw, ph) = ui.layout.pause;
        tap(&mut ui, 2, px + pw * 0.5, py + ph * 0.5);
        assert_eq!(ui.move_input, (0.0, 0.0));
        assert!(ui.pointers.is_empty() && ui.dir.is_none() && !ui.dpad_shown(4));
    }

    #[test]
    fn a_resting_finger_swings_the_arm_before_it_digs_a_drag_does_not() {
        let mut ui = TouchUi::new(W, H);
        ui.on_press(1, 1800.0, 400.0, PointerRole::Look);
        ui.tick(SWING_SECS - 0.01);
        assert!(!ui.pressing());
        ui.tick(0.02);
        assert!(ui.pressing() && !ui.digging(), "swinging from SWING_SECS, digging only from HOLD_SECS");
        ui.on_release(1);
        ui.on_press(1, 1800.0, 400.0, PointerRole::Look);
        ui.on_move(1, 1900.0, 400.0);
        ui.tick(0.3);
        assert!(!ui.pressing(), "a drag is a look");
        ui.on_release(1);
    }

    #[test]
    fn dig_now_digs_a_resting_finger_at_once_and_is_no_tap() {
        let mut ui = TouchUi::new(W, H);
        ui.on_press(1, 1800.0, 400.0, PointerRole::Look);
        ui.tick(SWING_SECS - 0.01);
        ui.dig_now();
        assert!(!ui.digging(), "a finger that has not rested yet does not dig");
        ui.tick(0.02);
        assert!(ui.resting());
        ui.dig_now();
        assert!(ui.digging());
        ui.on_release(1);
        assert!(!ui.digging() && !ui.take_place());
    }

    #[test]
    fn tap_places_hold_digs_drag_does_neither() {
        let mut ui = TouchUi::new(W, H);
        // Quick tap -> place.
        ui.on_press(1, 1800.0, 400.0, PointerRole::Look);
        ui.tick(0.1);
        ui.on_release(1);
        assert!(ui.take_place() && !ui.digging());
        // Hold still: digging starts at 0.4 s, lasts while the finger is down (even if it then moves),
        // and the release is not a tap.
        ui.on_press(1, 1800.0, 400.0, PointerRole::Look);
        ui.tick(0.3);
        assert!(!ui.digging());
        ui.tick(0.1);
        assert!(ui.digging());
        ui.on_move(1, 1900.0, 400.0);
        ui.tick(0.1);
        assert!(ui.digging());
        ui.on_release(1);
        assert!(!ui.digging() && !ui.take_place());
        // A drag is a look: neither a tap nor digging.
        ui.on_press(1, 1800.0, 400.0, PointerRole::Look);
        ui.on_move(1, 1900.0, 400.0);
        ui.tick(0.5);
        assert!(!ui.digging());
        ui.on_release(1);
        assert!(!ui.take_place());
        // No dead zone: a press just under HOLD_SECS is a tap, one at HOLD_SECS a dig, never nothing.
        ui.on_press(1, 1800.0, 400.0, PointerRole::Look);
        ui.tick(HOLD_SECS - 0.01);
        ui.on_release(1);
        assert!(ui.take_place() && !ui.digging());
        ui.on_press(1, 1800.0, 400.0, PointerRole::Look);
        ui.tick(HOLD_SECS);
        assert!(ui.digging());
        ui.on_release(1);
        assert!(!ui.take_place() && !ui.digging());
    }

    #[test]
    fn view_button_cycles_first_third_shoulder() {
        let mut ui = TouchUi::new(W, H);
        let (x, y) = mid(ui.layout.view);
        assert!(ui.layout.view.0 + ui.layout.view.2 < ui.layout.zoom.0, "view sits left of zoom");
        assert_eq!(ui.layout.hit_test(x, y, false, false), Some(PointerRole::View));
        tap(&mut ui, 1, x, y);
        assert!(ui.third && !ui.shoulder);
        tap(&mut ui, 1, x, y);
        assert!(ui.third && ui.shoulder);
        tap(&mut ui, 1, x, y);
        assert!(ui.third && ui.shoulder && ui.take_swap() && !ui.take_swap(), "a tap in the shoulder view swaps shoulders");
        ui.on_press(1, x, y, PointerRole::View);
        ui.tick(HOLD_SECS);
        ui.on_release(1);
        assert!(!ui.third && !ui.shoulder && !ui.take_swap(), "a hold leaves it");
    }

    /// The zoom button is held, not tapped: `zooming` is true while the finger is down, only in play, and it clears on release.
    #[test]
    fn zoom_button_is_a_hold() {
        let mut ui = TouchUi::new(W, H);
        let (x, y) = mid(ui.layout.zoom);
        assert!(ui.layout.zoom.0 + ui.layout.zoom.2 < ui.layout.inventory.0, "zoom sits left of inventory");
        assert_eq!(ui.layout.hit_test(x, y, true, false), None);
        assert_eq!(ui.layout.hit_test(x, y, false, true), Some(PointerRole::Tap));
        assert_eq!(ui.layout.hit_test(x, y, false, false), Some(PointerRole::Zoom));
        ui.on_press(1, x, y, PointerRole::Zoom);
        assert!(ui.zooming());
        ui.on_release(1);
        assert!(!ui.zooming());
    }

    #[test]
    fn open_screen_turns_presses_into_taps() {
        let mut ui = TouchUi::new(W, H);
        let (ix, iy, iw, ih) = ui.layout.inventory;
        let (px, py, pw, _) = ui.layout.pause;
        assert!(ix + iw < px, "inventory button sits left of pause");
        assert_eq!(ui.layout.hit_test(ix + iw * 0.5, iy + ih * 0.5, false, false), Some(PointerRole::Inventory));
        tap(&mut ui, 1, ix + iw * 0.5, iy + ih * 0.5);
        assert!(ui.take_inventory() && !ui.take_inventory());
        // A held move finger is dropped when the screen opens; afterwards the jump button and the world are taps.
        let (x, y) = mid(ui.layout.dpad[0]);
        ui.on_press(2, x, y, PointerRole::Dir);
        ui.set_screen(true);
        assert!(ui.pointers.is_empty());
        let (jx, jy) = mid(ui.layout.jump);
        tap(&mut ui, 3, jx, jy);
        tap(&mut ui, 4, 1800.0, 400.0);
        assert_eq!(
            ui.take_screen_events(),
            vec![ScreenEv::Down(jx, jy), ScreenEv::Up(jx, jy), ScreenEv::Down(1800.0, 400.0), ScreenEv::Up(1800.0, 400.0)]
        );
        assert!(!ui.jumping() && !ui.take_place() && ui.take_screen_events().is_empty());
        // A drag reports every move and floats the cursor with the finger.
        ui.on_press(6, 1000.0, 500.0, PointerRole::Tap);
        ui.on_move(6, 1100.0, 520.0);
        assert_eq!(ui.cursor_pos, (1100.0, 520.0));
        ui.on_release(6);
        assert_eq!(ui.take_screen_events(), vec![ScreenEv::Down(1000.0, 500.0), ScreenEv::Move(1100.0, 520.0), ScreenEv::Up(1100.0, 520.0)]);
        // The pause button still works with a screen open.
        tap(&mut ui, 5, px + pw * 0.5, py + 1.0);
        assert!(ui.paused);
    }
}
