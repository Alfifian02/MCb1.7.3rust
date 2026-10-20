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
//   - Paused: only the pause button and the resume button respond.
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
use std::collections::HashMap;

/// Hotbar slot count.
pub const HOTBAR_SLOTS: usize = 9;
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
    /// Press landed on the pre-generate button (pause menu only, top-left). Fires on release.
    Pregen,
    /// Press landed on the TEMPORARY fly button (pause menu only, beside Pregen). Toggles `TouchUi.flying` on release.
    Fly,
    /// The pause menu's LoD debug button (cycles the colouring).
    LodDebug,
    /// Press landed on the save-and-quit-to-title button (pause menu only). Fires on release.
    Quit,
    /// Press landed on the zoom button. Zoomed while held (`TouchUi::zooming`).
    Zoom,
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
    /// Hold to zoom (Zoomify), left of `inventory`.
    pub zoom: Rect,
    pub resume: Rect,
    /// TEMPORARY: reset-world button, under `resume`.
    pub reset: Rect,
    /// `GuiIngameMenu`'s "Save and quit to title", under `reset`.
    pub quit: Rect,
    /// The pre-generation switch, top-left (the pause menu only).
    pub pregen: Rect,
    /// TEMPORARY fly toggle, right of `pregen` (the pause menu only).
    pub fly: Rect,
    /// LoD debug colouring, under `pregen` (the pause menu only).
    pub dbg: Rect,
    pub hotbar: [Rect; HOTBAR_SLOTS],
    pub jump: Rect,
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
        let resume = ((w - bw) * 0.5, (h - bh) * 0.5 - bh * 0.45, bw, bh);
        let reset = (resume.0, resume.1 + bh * 1.25, bw, bh * 0.8);
        let quit = (resume.0, resume.1 + bh * 2.15, bw, bh * 0.8);

        // The d-pad: a 3 x 3 block of square cells, its bottom edge half a pad above the hotbar row, so it clears the hotbar at any ratio.
        let dpad_gap = 0.012 * u;
        let step = 0.12 * u + dpad_gap;
        let (cx, cy) = (pad * 1.5 + step * 1.5, hb_y - pad * 0.5 - step * 1.5);
        let cell = |col: f32, row: f32| (cx + (col - 0.5) * step, cy + (row - 0.5) * step, step, step);
        // (column, row) offsets from the centre, in `DIRS` order.
        let dpad = [(0.0, -1.0), (0.0, 1.0), (-1.0, 0.0), (1.0, 0.0), (-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)].map(|(c, r)| cell(c, r));
        let jw = 0.16 * u;
        let jump = (w - pad * 1.5 - jw, cy - jw * 0.5, jw, jw);

        let pregen = (pad, pad, (0.6 * u).min(w * 0.4), ps * 0.8);
        let fly = (pregen.0 + pregen.2 + pad * 0.5, pad, 0.3 * u, ps * 0.8);
        let dbg = (pregen.0, pregen.1 + pregen.3 + pad * 0.5, pregen.2, ps * 0.8);

        Self { width: w, pause, inventory, zoom, resume, reset, quit, pregen, fly, dbg, hotbar, jump, sneak: cell(0.0, 0.0), dpad, dpad_gap }
    }

    /// Hit-test a press. `None` means the press is ignored. While paused only
    /// the pause and resume buttons respond; with a `screen` open everything but the two buttons is a tap.
    pub fn hit_test(&self, x: f32, y: f32, paused: bool, screen: bool) -> Option<PointerRole> {
        if in_rect(self.pause, x, y) || (paused && in_rect(self.resume, x, y)) {
            return Some(PointerRole::Pause);
        }
        if paused {
            return if in_rect(self.reset, x, y) {
                Some(PointerRole::Reset)
            } else if in_rect(self.pregen, x, y) {
                Some(PointerRole::Pregen)
            } else if in_rect(self.fly, x, y) {
                Some(PointerRole::Fly)
            } else if in_rect(self.dbg, x, y) {
                Some(PointerRole::LodDebug)
            } else {
                in_rect(self.quit, x, y).then_some(PointerRole::Quit)
            };
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
    quit: bool,
    pregen: bool,
    lod_debug: bool,
    /// A menu screen (`menu::Menu`) is up: every press is a tap, the pause and inventory buttons included.
    pub menu: bool,
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
            dir: None,
            shown: [false; 4],
            sneak: false,
            flying: false,
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
            quit: false,
            pregen: false,
            lod_debug: false,
            menu: false,
            events: Vec::new(),
            cursor_pos: (w as f32 * 0.5, h as f32 * 0.5),
        }
    }

    /// The reset-world button was confirmed (tapped twice) since the last call.
    pub fn take_reset(&mut self) -> bool {
        std::mem::take(&mut self.reset)
    }

    /// The pre-generate button was tapped since the last call.
    pub fn take_pregen(&mut self) -> bool {
        std::mem::take(&mut self.pregen)
    }

    /// The LoD debug button was tapped since the last call.
    pub fn take_lod_debug(&mut self) -> bool {
        std::mem::take(&mut self.lod_debug)
    }

    /// The quit-to-title button was tapped since the last call.
    pub fn take_quit(&mut self) -> bool {
        std::mem::take(&mut self.quit)
    }

    /// A menu screen opened or closed: like `set_screen`, but nothing on the glass is a button of the game.
    pub fn set_menu(&mut self, open: bool) {
        self.menu = open;
        self.paused = false;
        self.set_screen(open);
    }

    /// A finger is on the zoom button.
    pub fn zooming(&self) -> bool {
        self.pointers.values().any(|p| p.role == PointerRole::Zoom)
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
        if self.menu { Some(PointerRole::Tap) } else { self.layout.hit_test(x, y, self.paused, self.screen) }
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
            PointerRole::Reset => {
                if self.reset_armed {
                    (self.reset, self.reset_armed, self.paused) = (true, false, false);
                    self.clear();
                } else {
                    self.reset_armed = true;
                }
            }
            PointerRole::Quit => self.quit = true,
            PointerRole::Pregen => self.pregen = true,
            PointerRole::Fly => self.flying = !self.flying,
            PointerRole::LodDebug => self.lod_debug = true,
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
            PointerRole::Dir => {
                self.set_dir(None);
                self.shown = [false; 4];
            }
            PointerRole::Sneak => self.sneak = !self.sneak,
            PointerRole::Look => {
                let slop = self.surface_w.min(self.surface_h) as f32 * SLOP;
                if !ptr.broke && ptr.held < TAP_SECS && ptr.travel < slop {
                    self.place = true;
                }
            }
            PointerRole::Jump | PointerRole::Zoom => {}
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

    #[test]
    fn paused_only_pause_and_resume_respond() {
        let l = LayoutRects::for_surface(W, H);
        assert_eq!(l.hit_test(1800.0, 400.0, true, false), None);
        let (jx, jy) = mid(l.jump);
        assert_eq!(l.hit_test(jx, jy, true, false), None);
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

    /// The quit button answers only while paused, once per tap; a menu screen turns every press (the pause button's too) into a tap.
    #[test]
    fn quit_button_fires_once_and_a_menu_takes_every_press() {
        let mut ui = TouchUi::new(W, H);
        let (x, y) = (ui.layout.quit.0 + 5.0, ui.layout.quit.1 + 5.0);
        assert_eq!(ui.layout.hit_test(x, y, false, false), None);
        ui.paused = true;
        tap(&mut ui, 1, x, y);
        assert!(ui.take_quit() && !ui.take_quit() && ui.paused && !ui.take_reset());
        ui.set_menu(true);
        assert!(!ui.paused);
        let (px, py) = mid(ui.layout.pause);
        assert_eq!(ui.role_at(px, py), Some(PointerRole::Tap));
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

    /// The pause menu's Fly button sits beside Pregen, toggles on release and stays on after resuming.
    #[test]
    fn fly_button_toggles_and_survives_resume() {
        let mut ui = TouchUi::new(W, H);
        let (px, py, pw, ph) = ui.layout.pause;
        tap(&mut ui, 1, px + pw * 0.5, py + ph * 0.5);
        let (x, y) = mid(ui.layout.fly);
        assert_eq!(ui.layout.hit_test(x, y, true, false), Some(PointerRole::Fly));
        assert_eq!(ui.layout.hit_test(x, y, false, false), Some(PointerRole::Look));
        tap(&mut ui, 2, x, y);
        tap(&mut ui, 3, px + pw * 0.5, py + ph * 0.5);
        assert!(!ui.paused && ui.flying);
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
