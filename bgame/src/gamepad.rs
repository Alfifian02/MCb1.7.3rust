//! Platform-independent gamepad model. Platforms feed buttons and axes in; the game reads `sample()`.
//!
//! Default layout (Bedrock-style):
//!   Left stick = move          Right stick = look
//!   RT / R2    = break (hold)  LT / L2 / X = place (hold)
//!   A = jump / fly up          B = sneak / fly down
//!   Y or D-pad Up = toggle fly L3 = sprint (latches until you stop)
//!   LB/RB or D-pad Left/Right = previous / next hotbar slot
//!   D-pad Down = cycle view distance (pips shown top-left)
//!   R3, Start, Select, Mode = unassigned (see `sample`, one line each to remap)

const LOOK_SPEED: f32 = 3.2; // rad/s at full deflection
const MOVE_DEADZONE: f32 = 0.2;
const LOOK_DEADZONE: f32 = 0.15;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum PadButton { A, B, X, Y, L1, R1, L2, R2, L3, R3, Start, Select, Mode, DUp, DDown, DLeft, DRight }

#[derive(Clone, Copy, Default, Debug)]
pub struct Axes {
    pub lx: f32, pub ly: f32,
    pub rx: f32, pub ry: f32,
    pub lt: f32, pub rt: f32,
    pub hat_x: f32, pub hat_y: f32,
}

#[derive(Clone, Copy, Default, Debug)]
pub struct PadControls {
    pub move_x: f32,
    pub move_y: f32, // +1 = forward
    pub look_x: f32, // radians this frame, +right
    pub look_y: f32, // radians this frame, +up
    pub jump: bool,
    pub sneak: bool,
    pub sprint: bool,
    pub brk: bool,
    pub place: bool,
    pub hotbar_delta: i32,
    pub fly_toggle: bool,
    pub view_cycle: bool,
}

#[derive(Default)]
pub struct Gamepad {
    pub axes: Axes,
    /// True once any gamepad event arrived (used to hide the touch overlay).
    pub active: bool,
    key_bits: u32,
    hat_bits: u32,
    now: u32,
    pressed: u32,
    sprint_latch: bool,
}

fn bit(b: PadButton) -> u32 { 1 << (b as u32) }

pub fn radial_deadzone(x: f32, y: f32, dz: f32) -> (f32, f32) {
    let (x, y) = (x.clamp(-1.0, 1.0), y.clamp(-1.0, 1.0));
    let m = (x * x + y * y).sqrt();
    if m < dz {
        return (0.0, 0.0);
    }
    let s = ((m - dz) / (1.0 - dz)).min(1.0) / m;
    (x * s, y * s)
}

impl Gamepad {
    fn refresh(&mut self) {
        let now = self.key_bits | self.hat_bits;
        self.pressed |= now & !self.now;
        self.now = now;
    }

    pub fn button(&mut self, b: PadButton, down: bool) {
        self.active = true;
        if down { self.key_bits |= bit(b) } else { self.key_bits &= !bit(b) }
        self.refresh();
    }

    pub fn set_axes(&mut self, a: Axes) {
        self.active = true;
        self.axes = a;
        let mut hat = 0;
        if a.hat_x < -0.5 { hat |= bit(PadButton::DLeft) }
        if a.hat_x > 0.5 { hat |= bit(PadButton::DRight) }
        if a.hat_y < -0.5 { hat |= bit(PadButton::DUp) }
        if a.hat_y > 0.5 { hat |= bit(PadButton::DDown) }
        self.hat_bits = hat;
        self.refresh();
    }

    fn held(&self, b: PadButton) -> bool { self.now & bit(b) != 0 }

    fn take(&mut self, b: PadButton) -> bool {
        let p = self.pressed & bit(b) != 0;
        self.pressed &= !bit(b);
        p
    }

    pub fn sample(&mut self, dt: f32) -> PadControls {
        let a = self.axes;
        let (mx, my) = radial_deadzone(a.lx, a.ly, MOVE_DEADZONE);
        let (rx, ry) = radial_deadzone(a.rx, a.ry, LOOK_DEADZONE);
        let rm = (rx * rx + ry * ry).sqrt(); // quadratic response: fine aim near centre
        let moving = mx != 0.0 || my != 0.0;

        if self.take(PadButton::L3) { self.sprint_latch = !self.sprint_latch; }
        if !moving { self.sprint_latch = false; }

        let mut hd = 0;
        for (b, d) in [(PadButton::L1, -1), (PadButton::DLeft, -1), (PadButton::R1, 1), (PadButton::DRight, 1)] {
            if self.take(b) { hd += d; }
        }
        let y = self.take(PadButton::Y);
        let up = self.take(PadButton::DUp);
        let view = self.take(PadButton::DDown);

        PadControls {
            move_x: mx,
            move_y: -my, // Android reports stick-up as negative Y
            look_x: rx * rm * LOOK_SPEED * dt,
            look_y: -ry * rm * LOOK_SPEED * dt,
            jump: self.held(PadButton::A),
            sneak: self.held(PadButton::B),
            sprint: self.sprint_latch,
            brk: self.held(PadButton::R2) || a.rt > 0.5,
            place: self.held(PadButton::L2) || self.held(PadButton::X) || a.lt > 0.5,
            hotbar_delta: hd,
            fly_toggle: y || up,
            view_cycle: view,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadzone_kills_drift_and_reaches_full() {
        assert_eq!(radial_deadzone(0.1, 0.1, 0.2), (0.0, 0.0));
        let (x, y) = radial_deadzone(1.0, 0.0, 0.2);
        assert!((x - 1.0).abs() < 1e-6 && y == 0.0);
    }

    #[test]
    fn stick_up_is_forward() {
        let mut g = Gamepad::default();
        g.set_axes(Axes { ly: -1.0, ..Default::default() });
        assert!(g.sample(0.016).move_y > 0.99);
    }

    #[test]
    fn triggers_work_as_axes_or_buttons() {
        let mut g = Gamepad::default();
        g.set_axes(Axes { rt: 0.9, ..Default::default() });
        assert!(g.sample(0.016).brk);
        g.set_axes(Axes::default());
        assert!(!g.sample(0.016).brk);
        g.button(PadButton::L2, true);
        assert!(g.sample(0.016).place);
    }

    #[test]
    fn hat_and_dpad_cycle_hotbar_once_per_press() {
        let mut g = Gamepad::default();
        g.set_axes(Axes { hat_x: 1.0, ..Default::default() });
        assert_eq!(g.sample(0.016).hotbar_delta, 1);
        assert_eq!(g.sample(0.016).hotbar_delta, 0); // held, no repeat
        g.set_axes(Axes::default());
        g.button(PadButton::L1, true);
        g.button(PadButton::L1, false);
        assert_eq!(g.sample(0.016).hotbar_delta, -1);
    }

    #[test]
    fn sprint_latches_until_stopped() {
        let mut g = Gamepad::default();
        g.set_axes(Axes { ly: -1.0, ..Default::default() });
        g.button(PadButton::L3, true);
        g.button(PadButton::L3, false);
        assert!(g.sample(0.016).sprint);
        assert!(g.sample(0.016).sprint);
        g.set_axes(Axes::default());
        assert!(!g.sample(0.016).sprint);
    }

    #[test]
    fn dpad_down_cycles_view_once() {
        let mut g = Gamepad::default();
        g.button(PadButton::DDown, true);
        assert!(g.sample(0.016).view_cycle);
        assert!(!g.sample(0.016).view_cycle);
    }

    #[test]
    fn look_has_quadratic_response() {
        let mut g = Gamepad::default();
        g.set_axes(Axes { rx: 0.5, ..Default::default() });
        let half = g.sample(1.0).look_x;
        g.set_axes(Axes { rx: 1.0, ..Default::default() });
        let full = g.sample(1.0).look_x;
        assert!(half < full * 0.3, "half {half} full {full}");
    }
}
