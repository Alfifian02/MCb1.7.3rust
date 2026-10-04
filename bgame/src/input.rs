//! Merges touch, gamepad and (desktop-only) keyboard into one `Controls` per frame.
use crate::gamepad::{Axes, Gamepad, PadButton};
use crate::ui::Layout;

#[derive(Clone, Copy, Default, Debug)]
pub struct Controls {
    pub move_x: f32,
    pub move_y: f32,    // +1 forward
    pub look_yaw: f32,  // radians this frame, +right
    pub look_pitch: f32, // radians this frame, +up
    pub jump: bool,
    pub sneak: bool,
    pub sprint: bool,
    pub brk: bool,
    pub place: bool,
    pub hotbar_delta: i32,
    pub hotbar_set: Option<usize>,
    pub fly_toggle: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase { Down, Move, Up }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key { W, A, S, D, Space, Shift, Left, Right, Up, Down, Fly, Break, Place, Sprint, Num(u8) }

#[derive(Default)]
pub struct Input {
    pub pad: Gamepad,
    size: (f32, f32),
    // keyboard (desktop dev)
    kw: bool, ka: bool, ks: bool, kd: bool, kspace: bool, kshift: bool,
    kleft: bool, kright: bool, kup: bool, kdown: bool, kbreak: bool, kplace: bool, ksprint: bool,
    // touch
    stick: Option<(u64, (f32, f32), (f32, f32))>,
    look: Option<(u64, (f32, f32))>,
    btn: [Option<u64>; 3], // jump, place, break
    tlook: (f32, f32),
    pending_set: Option<usize>,
    pending_fly: bool,
}

impl Input {
    pub fn new() -> Self { Self::default() }

    pub fn set_size(&mut self, w: u32, h: u32) { self.size = (w as f32, h as f32); }

    pub fn show_touch_ui(&self) -> bool { !self.pad.active }

    pub fn stick_vis(&self) -> Option<((f32, f32), (f32, f32))> { self.stick.map(|s| (s.1, s.2)) }

    pub fn pad_button(&mut self, b: PadButton, down: bool) { self.pad.button(b, down); }

    pub fn pad_axes(&mut self, a: Axes) { self.pad.set_axes(a); }

    pub fn key(&mut self, k: Key, down: bool) {
        match k {
            Key::W => self.kw = down,
            Key::A => self.ka = down,
            Key::S => self.ks = down,
            Key::D => self.kd = down,
            Key::Space => self.kspace = down,
            Key::Shift => self.kshift = down,
            Key::Left => self.kleft = down,
            Key::Right => self.kright = down,
            Key::Up => self.kup = down,
            Key::Down => self.kdown = down,
            Key::Break => self.kbreak = down,
            Key::Place => self.kplace = down,
            Key::Sprint => self.ksprint = down,
            Key::Fly => if down { self.pending_fly = true },
            Key::Num(n) => if down && (1..=9).contains(&n) { self.pending_set = Some(n as usize - 1) },
        }
    }

    pub fn touch_cancel_all(&mut self) {
        self.stick = None;
        self.look = None;
        self.btn = [None; 3];
    }

    pub fn touch(&mut self, id: u64, phase: Phase, x: f32, y: f32) {
        let l = Layout::new(self.size.0, self.size.1);
        match phase {
            Phase::Down => {
                self.pad.active = false; // touching the screen brings the overlay back
                if let Some(i) = l.hotbar.iter().position(|r| r.contains(x, y)) {
                    self.pending_set = Some(i);
                    return;
                }
                for (i, r) in [l.jump, l.place, l.brk].iter().enumerate() {
                    if self.btn[i].is_none() && r.contains(x, y) {
                        self.btn[i] = Some(id);
                        return;
                    }
                }
                if l.fly.contains(x, y) {
                    self.pending_fly = true;
                    return;
                }
                if x < self.size.0 * 0.5 {
                    if self.stick.is_none() { self.stick = Some((id, (x, y), (x, y))); }
                } else if self.look.is_none() {
                    self.look = Some((id, (x, y)));
                }
            }
            Phase::Move => {
                if let Some(s) = self.stick.as_mut().filter(|s| s.0 == id) {
                    s.2 = (x, y);
                }
                if let Some(lk) = self.look.as_mut().filter(|k| k.0 == id) {
                    self.tlook.0 += (x - lk.1 .0) * 0.005;
                    self.tlook.1 -= (y - lk.1 .1) * 0.005;
                    lk.1 = (x, y);
                }
            }
            Phase::Up => {
                if self.stick.map_or(false, |s| s.0 == id) { self.stick = None; }
                if self.look.map_or(false, |k| k.0 == id) { self.look = None; }
                for b in self.btn.iter_mut() {
                    if *b == Some(id) { *b = None; }
                }
            }
        }
    }

    pub fn controls(&mut self, dt: f32) -> Controls {
        let pc = self.pad.sample(dt);
        let axis = |p: bool, n: bool| (p as i32 - n as i32) as f32;
        let (sx, sy) = match self.stick {
            Some((_, a, b)) => (((b.0 - a.0) / 120.0).clamp(-1.0, 1.0), ((a.1 - b.1) / 120.0).clamp(-1.0, 1.0)),
            None => (0.0, 0.0),
        };
        let c = Controls {
            move_x: (axis(self.kd, self.ka) + sx + pc.move_x).clamp(-1.0, 1.0),
            move_y: (axis(self.kw, self.ks) + sy + pc.move_y).clamp(-1.0, 1.0),
            look_yaw: self.tlook.0 + pc.look_x + axis(self.kright, self.kleft) * 2.0 * dt,
            look_pitch: self.tlook.1 + pc.look_y + axis(self.kup, self.kdown) * 2.0 * dt,
            jump: self.kspace || self.btn[0].is_some() || pc.jump,
            sneak: self.kshift || pc.sneak,
            sprint: self.ksprint || pc.sprint || sy > 0.98,
            brk: self.kbreak || self.btn[2].is_some() || pc.brk,
            place: self.kplace || self.btn[1].is_some() || pc.place,
            hotbar_delta: pc.hotbar_delta,
            hotbar_set: self.pending_set.take(),
            fly_toggle: self.pending_fly || pc.fly_toggle,
        };
        self.tlook = (0.0, 0.0);
        self.pending_fly = false;
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> Input {
        let mut i = Input::new();
        i.set_size(1600, 720);
        i
    }

    #[test]
    fn gamepad_and_touch_merge_and_overlay_hides() {
        let mut i = input();
        assert!(i.show_touch_ui());
        i.pad_axes(Axes { ly: -1.0, ..Default::default() });
        assert!(!i.show_touch_ui());
        assert!(i.controls(0.016).move_y > 0.99);
        i.touch(1, Phase::Down, 100.0, 100.0);
        assert!(i.show_touch_ui());
    }

    #[test]
    fn touch_buttons_hold_until_release() {
        let mut i = input();
        let l = Layout::new(1600.0, 720.0);
        let (cx, cy) = (l.brk.x + 5.0, l.brk.y + 5.0);
        i.touch(7, Phase::Down, cx, cy);
        assert!(i.controls(0.016).brk);
        assert!(i.controls(0.016).brk);
        i.touch(7, Phase::Up, cx, cy);
        assert!(!i.controls(0.016).brk);
    }

    #[test]
    fn hotbar_tap_selects_slot_once() {
        let mut i = input();
        let l = Layout::new(1600.0, 720.0);
        let r = l.hotbar[4];
        i.touch(3, Phase::Down, r.x + 2.0, r.y + 2.0);
        assert_eq!(i.controls(0.016).hotbar_set, Some(4));
        assert_eq!(i.controls(0.016).hotbar_set, None);
    }

    #[test]
    fn right_drag_looks() {
        let mut i = input();
        i.touch(2, Phase::Down, 1000.0, 300.0);
        i.touch(2, Phase::Move, 1100.0, 300.0);
        let c = i.controls(0.016);
        assert!(c.look_yaw > 0.4);
        assert_eq!(i.controls(0.016).look_yaw, 0.0);
    }
}
