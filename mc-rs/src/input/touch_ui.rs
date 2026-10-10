// Touch UX state machine, landscape layout. See mod.rs for an overview.
//
// Scheme (Minecraft-PE-like, two thumbs):
//   - Left half of the screen: floating move stick. It anchors where the
//     finger lands; drag distance from the anchor is the analog (fwd, side).
//   - Right half: drag anywhere to look. A short tap there places a block, a press held still
//     digs (hold to keep digging, the dig can follow the look), like Minecraft PE.
//   - Jump button: bottom-right circle (held = keep jumping).
//   - Hotbar: bottom centre, tap to select. Pause: top-right, inventory button just left of it.
//   - Paused: only the pause button and the resume button respond.
//   - A container screen is open (inventory / workbench): every press except the two buttons is a tap, reported with
//     `take_taps` for the screen to hit-test; move, look, jump and hotbar are off.
//
// SOURCE-OF-TRUTH NOTE: b1.7.3 Java has no touch UX, so every rectangle and
// radius here is UNVERIFIED (no reference to diff against). Sizes are
// fractions of the short screen side `u`, so they hold on any landscape
// aspect ratio. The hotbar slot count (9) and the {-1..1} forward/strafe
// semantics come from GuiIngame / MovementInputFromOptions.
//
// All positions are surface pixels, (0, 0) top-left (MotionEvent convention).

use android_activity::input::{Axis, MotionAction, MotionEvent};
use std::collections::HashMap;

/// Hotbar slot count.
pub const HOTBAR_SLOTS: usize = 9;
/// Fraction of the stick radius below which the stick reads as zero.
const DEADZONE: f32 = 0.15;
// UNVERIFIED: tap/hold timings and slop (b1.7.3 has no touch input). Picked to feel like PE.
/// A press shorter than this, that did not move, is a tap (place).
const TAP_SECS: f32 = 0.25;
/// A press held still this long starts digging, which lasts until the finger lifts.
const HOLD_SECS: f32 = 0.4;
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
    /// Press landed on the pause button (or the resume button while paused).
    /// Toggles pause on release.
    Pause,
    /// Press landed on the temporary reset-world button (pause menu only). Arms on the first release, fires on the second.
    Reset,
    /// Press landed on the inventory button. Opens / closes the screen on release.
    Inventory,
    /// Any other press while a screen is open. Reported as a tap on release.
    Tap,
    /// Press landed on hotbar slot N. Selects on release.
    Hotbar(usize),
    /// Press landed on the jump button. Held = jumping.
    Jump,
    /// Floating move stick; `anchor` is where the finger first landed.
    Move { anchor: (f32, f32) },
    /// Look drag; deltas accumulate in `TouchUi::look_delta`.
    Look,
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
    pub width: f32,
    pub pause: Rect,
    pub inventory: Rect,
    pub resume: Rect,
    /// TEMPORARY: reset-world button, under `resume`.
    pub reset: Rect,
    pub hotbar: [Rect; HOTBAR_SLOTS],
    pub jump_center: (f32, f32),
    pub jump_radius: f32,
    /// Where the idle stick hint is drawn.
    pub stick_home: (f32, f32),
    /// Max drag distance (stick full deflection).
    pub stick_radius: f32,
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

        // Square cells; capped so nine of them always fit the width.
        let cell = (0.105 * u).min(w / 9.5);
        let hb_x0 = (w - cell * HOTBAR_SLOTS as f32) * 0.5;
        let hb_y = h - cell - pad;
        let mut hotbar = [(0.0, 0.0, 0.0, 0.0); HOTBAR_SLOTS];
        for (i, slot) in hotbar.iter_mut().enumerate() {
            *slot = (hb_x0 + cell * i as f32, hb_y, cell, cell);
        }

        let bw = (0.8 * u).min(w * 0.6);
        let bh = 0.16 * u;
        let resume = ((w - bw) * 0.5, (h - bh) * 0.5, bw, bh);
        let reset = (resume.0, resume.1 + bh * 1.25, bw, bh * 0.8);

        Self {
            width: w,
            pause,
            inventory,
            resume,
            reset,
            hotbar,
            jump_center: (w - 0.17 * u, h - 0.30 * u),
            jump_radius: 0.09 * u,
            stick_home: (0.22 * u + pad, h - 0.30 * u),
            stick_radius: 0.14 * u,
        }
    }

    /// Hit-test a press. `None` means the press is ignored. While paused only
    /// the pause and resume buttons respond; with a `screen` open everything but the two buttons is a tap.
    pub fn hit_test(&self, x: f32, y: f32, paused: bool, screen: bool) -> Option<PointerRole> {
        if in_rect(self.pause, x, y) || (paused && in_rect(self.resume, x, y)) {
            return Some(PointerRole::Pause);
        }
        if paused {
            return in_rect(self.reset, x, y).then_some(PointerRole::Reset);
        }
        if in_rect(self.inventory, x, y) {
            return Some(PointerRole::Inventory);
        }
        if screen {
            return Some(PointerRole::Tap);
        }
        let (jx, jy) = self.jump_center;
        if (x - jx).powi(2) + (y - jy).powi(2) <= self.jump_radius.powi(2) {
            return Some(PointerRole::Jump);
        }
        if let Some(i) = self.hotbar.iter().position(|&r| in_rect(r, x, y)) {
            return Some(PointerRole::Hotbar(i));
        }
        if x < self.width * 0.5 {
            Some(PointerRole::Move { anchor: (x, y) })
        } else {
            Some(PointerRole::Look)
        }
    }
}

/// Finger offset from the stick anchor -> analog (fwd, side), length <= 1.
/// Screen-up is forward.
pub fn stick_vector(dx: f32, dy: f32, radius: f32) -> (f32, f32) {
    let (mut side, mut fwd) = (dx / radius, -dy / radius);
    let len = (side * side + fwd * fwd).sqrt();
    if len < DEADZONE {
        return (0.0, 0.0);
    }
    if len > 1.0 {
        side /= len;
        fwd /= len;
    }
    (fwd, side)
}

/// Whole touch UX state. Owned by `App`.
pub struct TouchUi {
    pub paused: bool,
    pub hotbar_slot: usize,
    /// Analog movement in the camera frame: (fwd, side), each in [-1, 1].
    pub move_input: (f32, f32),
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
    /// TEMPORARY reset-world button: armed by one tap (drawn bright), confirmed by the next; leaving the pause menu disarms it.
    pub reset_armed: bool,
    reset: bool,
    /// Finger events on an open screen since the last `take_screen_events`.
    events: Vec<ScreenEv>,
    /// Where the finger last was on the open screen: the picked-up stack floats here.
    pub cursor_pos: (f32, f32),
}

impl TouchUi {
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            paused: false,
            hotbar_slot: 0,
            move_input: (0.0, 0.0),
            pointers: HashMap::new(),
            surface_w: w,
            surface_h: h,
            layout: LayoutRects::for_surface(w, h),
            look_delta: (0.0, 0.0),
            place: false,
            screen: false,
            inventory: false,
            reset_armed: false,
            reset: false,
            events: Vec::new(),
            cursor_pos: (w as f32 * 0.5, h as f32 * 0.5),
        }
    }

    /// The reset-world button was confirmed (tapped twice) since the last call.
    pub fn take_reset(&mut self) -> bool {
        std::mem::take(&mut self.reset)
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
        self.clear();
    }

    /// A tap (place) was requested since the last call.
    pub fn take_place(&mut self) -> bool {
        std::mem::take(&mut self.place)
    }

    /// True while a look finger that was held still is down: the player is digging.
    pub fn digging(&self) -> bool {
        self.pointers.values().any(|p| p.role == PointerRole::Look && p.broke)
    }

    /// Advance the hold timers by one frame: a look finger held still turns into digging after `HOLD_SECS`.
    pub fn tick(&mut self, dt: f32) {
        let slop = self.surface_w.min(self.surface_h) as f32 * SLOP;
        for p in self.pointers.values_mut().filter(|p| p.role == PointerRole::Look && !p.broke && p.travel < slop) {
            p.held += dt;
            p.broke = p.held >= HOLD_SECS;
        }
    }

    /// Read and clear the accumulated look drag (pixels).
    pub fn take_look(&mut self) -> (f32, f32) {
        std::mem::take(&mut self.look_delta)
    }

    /// True while a finger holds the jump button.
    pub fn jumping(&self) -> bool {
        self.pointers.values().any(|p| p.role == PointerRole::Jump)
    }

    /// (anchor, finger) of the active move stick, for drawing.
    pub fn stick_state(&self) -> Option<((f32, f32), (f32, f32))> {
        self.pointers.values().find_map(|p| match p.role {
            PointerRole::Move { anchor } => Some((anchor, p.last)),
            _ => None,
        })
    }

    /// Open the pause menu (the death screen reuses it: its resume button is the respawn button).
    pub fn pause(&mut self) {
        self.paused = true;
        self.clear();
    }

    /// Drop every finger and zero all input (pause, cancel, resize).
    fn clear(&mut self) {
        self.pointers.clear();
        self.move_input = (0.0, 0.0);
        self.look_delta = (0.0, 0.0);
        self.place = false;
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

    pub fn handle_motion(&mut self, ev: &MotionEvent) {
        match ev.action() {
            MotionAction::Down | MotionAction::PointerDown => {
                let p = ev.pointer_at_index(ev.pointer_index());
                let (x, y) = (p.axis_value(Axis::X), p.axis_value(Axis::Y));
                if let Some(role) = self.layout.hit_test(x, y, self.paused, self.screen) {
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
        // One move finger and one look finger at a time; extras are ignored.
        if matches!(role, PointerRole::Move { .. } | PointerRole::Look | PointerRole::Tap)
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
            PointerRole::Move { anchor } => {
                self.move_input = stick_vector(x - anchor.0, y - anchor.1, self.layout.stick_radius);
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
            PointerRole::Reset => {
                if self.reset_armed {
                    (self.reset, self.reset_armed, self.paused) = (true, false, false);
                    self.clear();
                } else {
                    self.reset_armed = true;
                }
            }
            PointerRole::Pause => {
                self.paused = !self.paused;
                self.reset_armed = false;
                self.clear();
                log::info!("touch: pause -> {}", self.paused);
            }
            PointerRole::Inventory => self.inventory = true,
            PointerRole::Tap => {
                self.events.push(ScreenEv::Up(ptr.last.0, ptr.last.1));
                self.cursor_pos = ptr.last;
            }
            PointerRole::Hotbar(slot) => self.hotbar_slot = slot,
            PointerRole::Move { .. } => self.move_input = (0.0, 0.0),
            PointerRole::Look => {
                let slop = self.surface_w.min(self.surface_h) as f32 * SLOP;
                if !ptr.broke && ptr.held < TAP_SECS && ptr.travel < slop {
                    self.place = true;
                }
            }
            PointerRole::Jump => {}
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
        assert_eq!(l.hit_test(l.jump_center.0, l.jump_center.1, false, false), Some(PointerRole::Jump));
        for i in 0..HOTBAR_SLOTS {
            let (hx, hy, hw, hh) = l.hotbar[i];
            assert_eq!(l.hit_test(hx + hw * 0.5, hy + hh * 0.5, false, false), Some(PointerRole::Hotbar(i)));
        }
        assert!(matches!(l.hit_test(600.0, 400.0, false, false), Some(PointerRole::Move { .. })));
        assert_eq!(l.hit_test(1800.0, 400.0, false, false), Some(PointerRole::Look));
    }

    #[test]
    fn buttons_do_not_overlap_in_common_landscape_ratios() {
        // 16:9, 20:9 and 4:3: jump button must not touch the hotbar row.
        for (w, h) in [(1920, 1080), (2400, 1080), (1440, 1080)] {
            let l = LayoutRects::for_surface(w, h);
            let jump_bottom = l.jump_center.1 + l.jump_radius;
            assert!(jump_bottom < l.hotbar[0].1, "{w}x{h}");
        }
    }

    #[test]
    fn paused_only_pause_and_resume_respond() {
        let l = LayoutRects::for_surface(W, H);
        assert_eq!(l.hit_test(1800.0, 400.0, true, false), None);
        assert_eq!(l.hit_test(l.jump_center.0, l.jump_center.1, true, false), None);
        let (rx, ry, rw, rh) = l.resume;
        assert_eq!(l.hit_test(rx + rw * 0.5, ry + rh * 0.5, true, false), Some(PointerRole::Pause));
    }

    /// The reset button answers only while paused, and needs two taps (the first arms it, leaving the menu disarms it).
    #[test]
    fn reset_needs_two_taps() {
        let mut ui = TouchUi::new(W, H);
        let (x, y) = (ui.layout.reset.0 + 5.0, ui.layout.reset.1 + 5.0);
        assert_eq!(ui.layout.hit_test(x, y, false, false), None);
        ui.paused = true;
        tap(&mut ui, 1, x, y);
        assert!(ui.reset_armed && !ui.take_reset() && ui.paused);
        let (rx, ry, rw, rh) = ui.layout.resume;
        tap(&mut ui, 2, rx + rw * 0.5, ry + rh * 0.5); // resume: disarms
        assert!(!ui.reset_armed && !ui.paused);
        ui.paused = true;
        tap(&mut ui, 3, x, y);
        tap(&mut ui, 4, x, y);
        assert!(ui.take_reset() && !ui.take_reset() && !ui.paused && !ui.reset_armed);
    }

    #[test]
    fn stick_vector_deadzone_direction_and_clamp() {
        assert_eq!(stick_vector(1.0, 1.0, 100.0), (0.0, 0.0));
        let (f, s) = stick_vector(0.0, -100.0, 100.0);
        approx(f, 1.0);
        approx(s, 0.0);
        let (f, s) = stick_vector(100.0, 0.0, 100.0);
        approx(f, 0.0);
        approx(s, 1.0);
        let (f, s) = stick_vector(500.0, -500.0, 100.0);
        approx((f * f + s * s).sqrt(), 1.0);
    }

    #[test]
    fn move_stick_drag_and_release() {
        let mut ui = TouchUi::new(W, H);
        ui.on_press(1, 400.0, 700.0, PointerRole::Move { anchor: (400.0, 700.0) });
        ui.on_move(1, 400.0, 700.0 - ui.layout.stick_radius);
        approx(ui.move_input.0, 1.0);
        approx(ui.move_input.1, 0.0);
        ui.on_release(1);
        assert_eq!(ui.move_input, (0.0, 0.0));
    }

    #[test]
    fn second_move_finger_is_ignored() {
        let mut ui = TouchUi::new(W, H);
        ui.on_press(1, 300.0, 600.0, PointerRole::Move { anchor: (300.0, 600.0) });
        ui.on_press(2, 500.0, 600.0, PointerRole::Move { anchor: (500.0, 600.0) });
        assert_eq!(ui.pointers.len(), 1);
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
        ui.on_press(1, 400.0, 700.0, PointerRole::Move { anchor: (400.0, 700.0) });
        ui.on_move(1, 400.0, 600.0);
        assert_ne!(ui.move_input, (0.0, 0.0));
        let (px, py, pw, ph) = ui.layout.pause;
        tap(&mut ui, 2, px + pw * 0.5, py + ph * 0.5);
        assert_eq!(ui.move_input, (0.0, 0.0));
        assert!(ui.pointers.is_empty());
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
        ui.tick(0.15);
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
        // Held still for 0.3 s: too long for a tap, too short to dig.
        ui.on_press(1, 1800.0, 400.0, PointerRole::Look);
        ui.tick(0.3);
        ui.on_release(1);
        assert!(!ui.take_place() && !ui.digging());
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
        ui.on_press(2, 400.0, 700.0, PointerRole::Move { anchor: (400.0, 700.0) });
        ui.set_screen(true);
        assert!(ui.pointers.is_empty());
        let (jx, jy) = ui.layout.jump_center;
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
