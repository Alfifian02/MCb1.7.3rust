// M12 - Touch UX state machine. See mod.rs for an overview.
//
// SOURCE-OF-TRUTH NOTE (per GPT-5.5 porting guide, rule #1, #2, #8):
//   - b1.7.3 Java does NOT contain any touch UX. The d-pad, look stick
//     and pause-button rectangles are Pocket Edition-style inventions.
//     Layout constants (cell sizes, anchor offsets) are UNVERIFIED: I
//     have no reference implementation to diff against. A reviewer
//     should compare against a Pocket Edition 0.x build before signing M12.
//   - The hotbar slot count (9) and the MovementInput.moveStrafe /
//     MovementInput.moveForward semantics are derived from b1.7.3
//     sources (GuiIngame.java: 9 cells; MovementInputFromOptions.java:
//     moveStrafe in {-1,0,1}, moveForward in {-1,0,1}). Those bits are
//     verified.
//   - The Android target build was interrupted before completion and was
//     not tested on a real device.
//
// All angles are radians, all positions are in surface pixels with (0, 0) at
// the top-left (the Android MotionEvent convention).

use android_activity::input::{Axis, MotionAction, MotionEvent};
use std::collections::HashMap;

/// Hotbar slot count.
pub const HOTBAR_SLOTS: usize = 9;

/// What a single pointer is currently doing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointerRole {
    /// Press landed on the pause button. Tap on release to toggle the menu.
    Pause,
    /// Press landed on hotbar slot N (0..HOTBAR_SLOTS). Tap on release.
    Hotbar(usize),
    /// Press landed in the look-stick hit area. We use the absolute press
    /// position as the anchor; move deltas drive the camera.
    LookStick { anchor: (f32, f32) },
    /// Press landed on a d-pad arm. `fwd` and `side` are -1/0/+1 in the
    /// camera-relative frame; App::step_input rotates them by yaw.
    Dpad { fwd: f32, side: f32 },
}

/// Live state of one finger on the glass.
#[derive(Clone, Copy, Debug)]
pub struct Pointer {
    pub role: PointerRole,
    pub last: (f32, f32),
}

/// Screen-space layout (pixels, top-left origin).
#[derive(Clone, Debug)]
pub struct LayoutRects {
    pub pause: (f32, f32, f32, f32),
    pub hotbar: [(f32, f32, f32, f32); HOTBAR_SLOTS],
    pub look_center: (f32, f32),
    pub look_radius: f32,
    pub dpad_center: (f32, f32),
    pub dpad_button_radius: f32,
    pub dpad_arm_len: f32,
}

impl LayoutRects {
    /// Layout tuned for phones in portrait. `w`, `h` are surface dimensions
    /// in pixels.
    pub fn for_surface(w: u32, h: u32) -> Self {
        let w = w as f32;
        let h = h as f32;

        // Pause: top-right corner.
        let pause_size = 64.0_f32.min(w * 0.12).min(h * 0.08);
        let pause_pad = 16.0_f32;
        let pause = (w - pause_size - pause_pad, pause_pad, pause_size, pause_size);

        // Hotbar: b1.7.3 lays out 9 cells of 20 px each (background 182x22),
        // centred horizontally, 22 px above the bottom edge. The cell width
        // here is the *touch hit-target*, scaled to the surface so a thumb
        // can reliably hit each slot on a phone. We multiply the original
        // 20 px by `touch_scale = w / 320` (b1.7.3 default scaled width).
        // UNVERIFIED: Pocket Edition uses a larger hit-target than b1.7.3
        // PC; the exact size is not derivable from the Java sources.
        let touch_scale = (w / 320.0).max(1.0);
        let cell = 20.0 * touch_scale;
        let hb_h = 22.0 * touch_scale;
        let hb_y = h - hb_h - 16.0 * touch_scale;
        let hb_x0 = (w - cell * HOTBAR_SLOTS as f32) * 0.5;
        let mut hotbar = [(0.0, 0.0, 0.0, 0.0); HOTBAR_SLOTS];
        for i in 0..HOTBAR_SLOTS {
            hotbar[i] = (hb_x0 + cell * i as f32, hb_y, cell, hb_h);
        }

        // Look stick and d-pad: anchored above the hotbar strip. Sizes and
        // anchor offsets are UNVERIFIED — see header.
        let touch_scale = (w / 320.0).max(1.0);
        let look_radius = (w.min(h) * 0.18).max(96.0 * touch_scale).min(180.0 * touch_scale);
        let look_center = (
            look_radius * 0.7,
            h - hb_h - 16.0 * touch_scale - look_radius * 0.9,
        );

        let dpad_button_radius = (w.min(h) * 0.08).max(44.0 * touch_scale).min(72.0 * touch_scale);
        let dpad_arm_len = dpad_button_radius * 1.6;
        let dpad_center = (
            w - dpad_button_radius * 1.4,
            h - hb_h - 16.0 * touch_scale - dpad_arm_len * 1.2,
        );

        Self {
            pause,
            hotbar,
            look_center,
            look_radius,
            dpad_center,
            dpad_button_radius,
            dpad_arm_len,
        }
    }

    /// Hit-test a touch position. Returns `None` if it lands outside every
    /// named region (the press is then ignored).
    pub fn hit_test(&self, x: f32, y: f32) -> Option<PointerRole> {
        let (px, py, pw, ph) = self.pause;
        if x >= px && x <= px + pw && y >= py && y <= py + ph {
            return Some(PointerRole::Pause);
        }
        for i in 0..HOTBAR_SLOTS {
            let (hx, hy, hw, hh) = self.hotbar[i];
            if x >= hx && x <= hx + hw && y >= hy && y <= hy + hh {
                return Some(PointerRole::Hotbar(i));
            }
        }
        // Look stick: anything inside the lower-left circle.
        let (cx, cy) = self.look_center;
        let dx = x - cx;
        let dy = y - cy;
        if dx * dx + dy * dy <= self.look_radius * self.look_radius {
            return Some(PointerRole::LookStick { anchor: (x, y) });
        }
        // D-pad: 4 buttons at N/S/E/W of the centre.
        let (cx, cy) = self.dpad_center;
        let arm = self.dpad_arm_len;
        let br = self.dpad_button_radius;
        if x >= cx - arm - br && x <= cx + arm + br && y >= cy - arm - br && y <= cy + arm + br {
            let rx = x - cx;
            let ry = y - cy;
            if rx.abs() >= ry.abs() {
                if rx >= 0.0 {
                    return Some(PointerRole::Dpad { fwd: 0.0, side: 1.0 });
                } else {
                    return Some(PointerRole::Dpad { fwd: 0.0, side: -1.0 });
                }
            } else if ry >= 0.0 {
                // y grows downward; positive ry is below the centre, which is
                // "backward" from the player's frame.
                return Some(PointerRole::Dpad { fwd: -1.0, side: 0.0 });
            } else {
                return Some(PointerRole::Dpad { fwd: 1.0, side: 0.0 });
            }
        }
        None
    }
}

/// Whole touch UX state. Owned by `App`.
pub struct TouchUi {
    pub paused: bool,
    pub hotbar_slot: usize,
    /// Movement vector from the d-pad in the camera frame: (fwd, side),
    /// each in {-1, 0, +1}.
    pub move_input: (f32, f32),
    pub jump_pending: bool,
    /// Latched jump request. M2 behaviour: tapping anywhere on the screen
    /// (i.e. a touch-down that lands outside any named region) sets this
    /// flag. App::step_input reads and clears it each frame. The PE-style
    /// replacement is a dedicated jump button, but this is a smaller diff
    /// from the M2 baseline and stays compatible with desktop tests.
    pub pointers: HashMap<i32, Pointer>,
    pub surface_w: u32,
    pub surface_h: u32,
    pub layout: LayoutRects,
}

impl TouchUi {
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            paused: false,
            hotbar_slot: 0,
            move_input: (0.0, 0.0),
            jump_pending: false,
            pointers: HashMap::new(),
            surface_w: w,
            surface_h: h,
            layout: LayoutRects::for_surface(w, h),
        }
    }

    /// Atomically read and clear the jump latch.
    pub fn take_jump(&mut self) -> bool {
        let j = self.jump_pending;
        self.jump_pending = false;
        j
    }

    /// Rebuild the layout if the surface size has changed. Returns true if
    /// the layout changed.
    pub fn ensure_layout(&mut self, w: u32, h: u32) -> bool {
        if self.surface_w == w && self.surface_h == h {
            return false;
        }
        self.surface_w = w;
        self.surface_h = h;
        self.layout = LayoutRects::for_surface(w, h);
        self.pointers.clear();
        self.move_input = (0.0, 0.0);
        true
    }

    pub fn handle_motion(&mut self, ev: &MotionEvent) {
        match ev.action() {
            MotionAction::Down | MotionAction::PointerDown => {
                let idx = ev.pointer_index();
                let p = ev.pointer_at_index(idx);
                let pid = p.pointer_id();
                let x = p.axis_value(Axis::X);
                let y = p.axis_value(Axis::Y);
                if let Some(role) = self.layout.hit_test(x, y) {
                    self.on_press(pid, x, y, role);
                } else {
                    // Touch-down on empty space: legacy M2 "tap-to-jump".
                    // Only the first finger to land triggers the latch;
                    // subsequent fingers on empty space are ignored so a
                    // second thumb doesn't cause a double-jump.
                    if self.pointers.is_empty() {
                        self.jump_pending = true;
                    }
                }
            }
            MotionAction::Move => {
                for p in ev.pointers() {
                    let pid = p.pointer_id();
                    let x = p.axis_value(Axis::X);
                    let y = p.axis_value(Axis::Y);
                    self.on_move(pid, x, y);
                }
            }
            MotionAction::Up | MotionAction::Cancel => {
                let idx = ev.pointer_index();
                let p = ev.pointer_at_index(idx);
                self.on_release(p.pointer_id(), true);
            }
            MotionAction::PointerUp => {
                let idx = ev.pointer_index();
                let p = ev.pointer_at_index(idx);
                self.on_release(p.pointer_id(), false);
            }
            _ => {}
        }
    }

    fn on_press(&mut self, pid: i32, x: f32, y: f32, role: PointerRole) {
        match role {
            PointerRole::Dpad { .. } => {
                self.recompute_dpad(pid, role);
                if let Some(p) = self.pointers.get_mut(&pid) {
                    p.last = (x, y);
                }
            }
            PointerRole::LookStick { anchor } => {
                self.pointers.insert(
                    pid,
                    Pointer {
                        role: PointerRole::LookStick { anchor },
                        last: (x, y),
                    },
                );
            }
            PointerRole::Pause => {
                self.pointers.insert(pid, Pointer { role, last: (x, y) });
            }
            PointerRole::Hotbar(_) => {
                self.pointers.insert(pid, Pointer { role, last: (x, y) });
            }
        }
    }

    fn recompute_dpad(&mut self, pid: i32, role: PointerRole) {
        self.pointers.insert(pid, Pointer { role, last: (0.0, 0.0) });
        let mut f = 0.0_f32;
        let mut s = 0.0_f32;
        for p in self.pointers.values() {
            if let PointerRole::Dpad { fwd, side } = p.role {
                if fwd.abs() > f.abs() { f = fwd; }
                if side.abs() > s.abs() { s = side; }
            }
        }
        self.move_input = (f, s);
    }

    fn on_move(&mut self, pid: i32, x: f32, y: f32) {
        if let Some(ptr) = self.pointers.get_mut(&pid) {
            ptr.last = (x, y);
        }
    }

    fn on_release(&mut self, pid: i32, is_cancel: bool) {
        let Some(ptr) = self.pointers.remove(&pid) else { return };
        if is_cancel {
            return;
        }
        match ptr.role {
            PointerRole::Pause => {
                self.paused = !self.paused;
                log::info!("M12 touch: pause toggled -> {}", self.paused);
            }
            PointerRole::Hotbar(slot) => {
                self.hotbar_slot = slot;
                log::info!("M12 touch: hotbar slot -> {slot}");
            }
            PointerRole::Dpad { fwd: _, side: _ } => {
                let mut f = 0.0_f32;
                let mut s = 0.0_f32;
                for p in self.pointers.values() {
                    if let PointerRole::Dpad { fwd, side } = p.role {
                        if fwd.abs() > f.abs() { f = fwd; }
                        if side.abs() > s.abs() { s = side; }
                    }
                }
                self.move_input = (f, s);
            }
            PointerRole::LookStick { anchor: _ } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) {
        assert!((a - b).abs() < 0.001, "{a} != {b}");
    }

    #[test]
    fn pause_button_in_corner() {
        let layout = LayoutRects::for_surface(1080, 1920);
        let (px, py, pw, ph) = layout.pause;
        let role = layout.hit_test(px + 1.0, py + 1.0).expect("pause hit");
        assert_eq!(role, PointerRole::Pause);
        let _ = (pw, ph);
    }

    #[test]
    fn hotbar_slots_centered() {
        let layout = LayoutRects::for_surface(1080, 1920);
        for i in 0..HOTBAR_SLOTS {
            let (hx, hy, hw, hh) = layout.hotbar[i];
            let cx = hx + hw * 0.5;
            let cy = hy + hh * 0.5;
            let role = layout.hit_test(cx, cy).expect("hotbar hit");
            assert_eq!(role, PointerRole::Hotbar(i));
        }
    }

    #[test]
    fn dpad_cardinal_directions() {
        let layout = LayoutRects::for_surface(1080, 1920);
        let (cx, cy) = layout.dpad_center;
        let fwd_role = layout.hit_test(cx, cy - layout.dpad_arm_len).expect("fwd");
        assert_eq!(fwd_role, PointerRole::Dpad { fwd: 1.0, side: 0.0 });
        let back_role = layout.hit_test(cx, cy + layout.dpad_arm_len).expect("back");
        assert_eq!(back_role, PointerRole::Dpad { fwd: -1.0, side: 0.0 });
        let left_role = layout.hit_test(cx - layout.dpad_arm_len, cy).expect("left");
        assert_eq!(left_role, PointerRole::Dpad { fwd: 0.0, side: -1.0 });
        let right_role = layout.hit_test(cx + layout.dpad_arm_len, cy).expect("right");
        assert_eq!(right_role, PointerRole::Dpad { fwd: 0.0, side: 1.0 });
        let _ = approx;
    }

    #[test]
    fn look_stick_centre_is_in() {
        let layout = LayoutRects::for_surface(1080, 1920);
        let (cx, cy) = layout.look_center;
        let role = layout.hit_test(cx, cy).expect("look centre");
        match role {
            PointerRole::LookStick { .. } => {}
            other => panic!("expected LookStick, got {other:?}"),
        }
    }

    #[test]
    fn empty_outside_dpad_returns_none() {
        let layout = LayoutRects::for_surface(1080, 1920);
        // Hit the middle of the screen -- should land on no region (or the
        // look stick if the centre happens to overlap; on a 1080x1920 the
        // centre is well outside both stick regions).
        let role = layout.hit_test(540.0, 960.0);
        assert!(role.is_none(), "centre should not hit anything: {role:?}");
    }

    #[test]
    fn dpad_compose_fwd_plus_right() {
        let mut ui = TouchUi::new(1080, 1920);
        let (cx, cy) = ui.layout.dpad_center;
        // Press forward, then right while forward is still held.
        let fwd_role = ui.layout.hit_test(cx, cy - ui.layout.dpad_arm_len).unwrap();
        ui.on_press(1, cx, cy - ui.layout.dpad_arm_len, fwd_role);
        let rt_role = ui.layout.hit_test(cx + ui.layout.dpad_arm_len, cy).unwrap();
        ui.on_press(2, cx + ui.layout.dpad_arm_len, cy, rt_role);
        // The max-absolute rule keeps both axes.
        approx(ui.move_input.0, 1.0);
        approx(ui.move_input.1, 1.0);
    }

    #[test]
    fn dpad_release_collapses_axis() {
        let mut ui = TouchUi::new(1080, 1920);
        let (cx, cy) = ui.layout.dpad_center;
        let fwd_role = ui.layout.hit_test(cx, cy - ui.layout.dpad_arm_len).unwrap();
        ui.on_press(1, cx, cy - ui.layout.dpad_arm_len, fwd_role);
        let rt_role = ui.layout.hit_test(cx + ui.layout.dpad_arm_len, cy).unwrap();
        ui.on_press(2, cx + ui.layout.dpad_arm_len, cy, rt_role);
        ui.on_release(1, false);
        // Right is still held, so side stays +1; fwd collapses to 0.
        approx(ui.move_input.0, 0.0);
        approx(ui.move_input.1, 1.0);
    }

    #[test]
    fn pause_tap_toggles() {
        let mut ui = TouchUi::new(1080, 1920);
        let (px, py, pw, ph) = ui.layout.pause;
        let role = ui.layout.hit_test(px + pw * 0.5, py + ph * 0.5).unwrap();
        ui.on_press(1, px + pw * 0.5, py + ph * 0.5, role);
        ui.on_release(1, false);
        assert!(ui.paused);
        ui.on_press(2, px + pw * 0.5, py + ph * 0.5, role);
        ui.on_release(2, false);
        assert!(!ui.paused);
    }

    #[test]
    fn hotbar_tap_selects_slot() {
        let mut ui = TouchUi::new(1080, 1920);
        let (hx, hy, hw, hh) = ui.layout.hotbar[3];
        let role = ui.layout.hit_test(hx + hw * 0.5, hy + hh * 0.5).unwrap();
        ui.on_press(1, hx + hw * 0.5, hy + hh * 0.5, role);
        ui.on_release(1, false);
        assert_eq!(ui.hotbar_slot, 3);
    }
}
