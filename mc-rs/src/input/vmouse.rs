//! Controlify's virtual mouse (isXander's Fabric mod, LGPL-3.0-or-later), ported line by line from `VirtualMouseHandler`, `SnapPoint`,
//! `HoldRepeatHelper`, `ControllerUtils.applyEasingToLength` and the snapping mixins (`ScreenMixin`, `AbstractContainerScreenMixin`).
//!
//! What is ported, with the Java it comes from:
//! * the stick's impulse is eased on its LENGTH with `x^3` (`handleControllerInput`), so a diagonal keeps its direction;
//! * the target moves `impulse * 20 * sensitivity * max(w, h) / 800` px a tick (the LCE mode: `10 * sensitivity * guiScale`);
//! * the shown cursor chases the target by `lerp(delta, current, target)` every frame (`updateMouse`);
//! * when the stick returns to centre the cursor snaps to the closest `SnapPoint` in range (`snapToClosestPoint`), never to the
//!   one it is already on while still inside its range (`lastSnappedPoint`);
//! * the d-pad snaps in a direction (`snapInDirection` / `findOrthogonalSnapPoint`: the point must lie that way, at least its range away,
//!   with a deviation under twice the distance; score `distance + 4 * deviation`; nothing that way wraps round from the far edge),
//!   repeating with `HoldRepeatHelper(10, 3)`;
//! * the right stick scrolls with the same carry-over (`scrollY`, a share of it per frame);
//! * the cursor is `virtual_mouse.png` at half size (`renderVirtualMouse`), see `vmouse_cursor.rs`.
//!
//! Controlify counts in 20 Hz ticks and mc-rs in frames: every per-tick step here is scaled by `dt / TICK`.
//! A snap point's position and range are in px (Controlify: GUI units): `SnapPoint::widget` / `SnapPoint::slot` do the conversion with the GUI scale.
//!
//! Not ported: touchpad fingers (the Java has them commented out), the `ISnapBehaviour` API for other mods, the per-screen toggle
//! (`VMOUSE_TOGGLE` + `virtualMouseScreens`, phase 3 with the settings), the debug overlay of the points (`DEBUG_SNAPPING`).
//! UNVERIFIED: `SCROLL_NOTCH` (a wheel notch in GUI units; Minecraft's lists use `itemHeight / 2`).

use super::vmouse_cursor::RUNS;
use crate::render::hud::{HudPipeline, HudVertex};

/// One game tick, in seconds.
pub const TICK: f32 = 0.05;
/// A wheel notch of the virtual scroll, in GUI units. UNVERIFIED: see the module doc.
pub const SCROLL_NOTCH: f32 = 9.0;

/// `VirtualMouseBehaviour`: what a screen asks of the virtual mouse.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Behaviour {
    /// The user's choice (Controlify: `virtualMouseScreens`, the container screens by default).
    #[default]
    Default,
    Enabled,
    Disabled,
    CursorOnly,
    /// A container screen: the cursor and the scroll, but its own clicks.
    CursorScroll,
}

impl Behaviour {
    pub fn has_cursor(self) -> bool {
        self != Behaviour::Disabled
    }

    pub fn is_default_or(self, b: Behaviour) -> bool {
        self == Behaviour::Default || self == b
    }
}

/// `SnapPoint`: where the cursor lands and how far from it still snaps there. px.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SnapPoint {
    pub x: f32,
    pub y: f32,
    pub range: f32,
}

impl SnapPoint {
    /// `ScreenMixin`: a widget's centre, `min(width, height) / 2 + 10` GUI units. `r` is the widget's rect in px, `s` px per GUI unit.
    pub fn widget(r: [f32; 4], s: f32) -> Self {
        Self { x: (r[0] + (r[2] / 2.0).floor()).floor(), y: (r[1] + (r[3] / 2.0).floor()).floor(), range: (r[2].min(r[3]) / 2.0).floor() + 10.0 * s }
    }

    /// `AbstractContainerScreenMixin`: a slot's icon centre (`leftPos + slot.x + 8`), 17 GUI units. `cell` is the slot's 18-unit cell in px.
    pub fn slot(cell: [f32; 4], s: f32) -> Self {
        Self { x: cell[0] + cell[2] / 2.0, y: cell[1] + cell[3] / 2.0, range: 17.0 * s }
    }

    fn dist2(&self, p: (f32, f32)) -> f32 {
        (self.x - p.0).powi(2) + (self.y - p.1).powi(2)
    }
}

/// A direction of the d-pad (`ScreenDirection`); y grows downward, so DOWN and RIGHT are the positive ones.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Dir {
    pub const ALL: [Dir; 4] = [Dir::Up, Dir::Down, Dir::Left, Dir::Right];

    fn horizontal(self) -> bool {
        matches!(self, Dir::Left | Dir::Right)
    }

    fn positive(self) -> bool {
        matches!(self, Dir::Down | Dir::Right)
    }
}

/// `HoldRepeatHelper`: acts on the press, again after `first` seconds and then every `repeat` seconds while held.
/// Controlify counts ticks (`10, 3`); `new` takes the same numbers.
#[derive(Clone, Copy, Debug)]
pub struct HoldRepeat {
    first: f32,
    repeat: f32,
    left: f32,
    reset_now: bool,
}

impl HoldRepeat {
    pub fn new(first_ticks: u32, repeat_ticks: u32) -> Self {
        Self { first: first_ticks as f32 * TICK, repeat: repeat_ticks as f32 * TICK, left: 0.0, reset_now: false }
    }

    pub fn reset(&mut self) {
        self.left = self.first;
        self.reset_now = true;
    }

    pub fn clear_delay(&mut self) {
        self.left = 0.0;
    }

    /// `onNavigate`: the action happened.
    pub fn on_navigate(&mut self) {
        if self.reset_now {
            self.reset_now = false;
        } else {
            self.left = self.repeat;
        }
    }

    /// `shouldAction(binding)`: `down` / `prev` are the binding now and a frame ago. Counts down only while held, like the Java's `&&`.
    pub fn should_action(&mut self, dt: f32, down: bool, prev: bool) -> bool {
        if !down {
            return false;
        }
        self.left -= dt;
        let act = self.left <= 1e-4 || !prev; // not `<= 0`: 10 subtractions of 0.05 f32 may leave a hair
        if act && !prev {
            self.reset();
        }
        act
    }
}

/// `ControllerUtils.applyEasingToLength`.
pub fn ease_length(x: f32, y: f32, f: impl Fn(f32) -> f32) -> (f32, f32) {
    let l = x.hypot(y);
    if l == 0.0 {
        return (0.0, 0.0);
    }
    let e = f(l);
    (x / l * e, y / l * e)
}

/// `findOrthogonalSnapPoint`.
fn find_orthogonal(from: Option<SnapPoint>, dir: Dir, target: (f32, f32), points: &[SnapPoint]) -> Option<SnapPoint> {
    let sign = if dir.positive() { 1.0 } else { -1.0 };
    points
        .iter()
        .filter(|p| Some(**p) != from)
        .filter_map(|p| {
            let (dx, dy) = (p.x - target.0, p.y - target.1);
            let (along, across) = if dir.horizontal() { (dx, dy) } else { (dy, dx) };
            if along * sign <= 0.0 {
                return None;
            }
            // the distance the right way, and the deviation the wrong way, punished four times over
            let (distance, deviation) = (along.abs(), across.abs());
            (distance >= p.range && deviation < distance * 2.0).then_some((*p, distance + deviation * 4.0))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(p, _)| p)
}

/// `snapInDirection`: the point the d-pad moves to from `target`; none that way takes it from the opposite edge (`bounds`).
pub fn snap_in_direction(from: Option<SnapPoint>, dir: Dir, target: (f32, f32), bounds: (f32, f32), points: &[SnapPoint]) -> Option<SnapPoint> {
    find_orthogonal(from, dir, target, points).or_else(|| {
        let mut wrap = target;
        match dir {
            Dir::Up => wrap.1 = bounds.1,
            Dir::Down => wrap.1 = 0.0,
            Dir::Left => wrap.0 = bounds.0,
            Dir::Right => wrap.0 = 0.0,
        }
        find_orthogonal(from, dir, wrap, points)
    })
}

/// `snapToClosestPoint`: the nearest point `target` is inside the range of, not the one it is already resting on. Returns it and the
/// `last` to remember (the old one is forgotten once the target has left its range).
pub fn snap_closest(last: Option<SnapPoint>, target: (f32, f32), points: &[SnapPoint]) -> (Option<SnapPoint>, Option<SnapPoint>) {
    let last = last.filter(|l| l.dist2(target) <= l.range * l.range);
    let best = points.iter().filter(|p| Some(**p) != last).filter(|p| p.dist2(target) <= p.range * p.range).min_by(|a, b| a.dist2(target).total_cmp(&b.dist2(target))).copied();
    (last, best)
}

/// One frame's input of the virtual mouse.
#[derive(Clone, Copy, Debug)]
pub struct VIn {
    /// The left stick after the deadzone, x right and y down (`VMOUSE_MOVE_*`).
    pub stick: (f32, f32),
    /// The scroll axis: positive scrolls the content up, i.e. the right stick pushed down (`VMOUSE_SCROLL_*`).
    pub scroll: f32,
    /// The snap binds (d-pad) held now: up, down, left, right.
    pub dirs: [bool; 4],
    /// `virtualMouseSensitivity` (1.0 = 100%).
    pub sens: f32,
    /// `isLCE`.
    pub lce: bool,
    /// The surface, px.
    pub bounds: (f32, f32),
    /// px per GUI unit.
    pub gs: f32,
    /// Whether the screen takes the scroll (`CURSOR_SCROLL` / `ENABLED` / default).
    pub scrolls: bool,
}

/// What a frame of the virtual mouse asks of the screen.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VOut {
    /// The cursor's new place (px); none while it did not move.
    pub to: Option<(f32, f32)>,
    /// Scroll to apply this frame in px, down positive.
    pub scroll_px: f32,
}

#[derive(Clone, Debug)]
pub struct VMouse {
    target: (f32, f32),
    cur: (f32, f32),
    /// Where the caller last saw the cursor: `update` reports a move when `cur` has left it (a snap included).
    reported: (f32, f32),
    last: Option<SnapPoint>,
    scroll: f32,
    prev_impulse: (f32, f32),
    prev_dirs: [bool; 4],
    repeat: HoldRepeat,
}

impl Default for VMouse {
    fn default() -> Self {
        Self { target: (0.0, 0.0), cur: (0.0, 0.0), reported: (0.0, 0.0), last: None, scroll: 0.0, prev_impulse: (0.0, 0.0), prev_dirs: [false; 4], repeat: HoldRepeat::new(10, 3) }
    }
}

impl VMouse {
    /// The cursor was put somewhere else (a finger, a warp, a new screen): both ends of the chase go there (`enableVirtualMouse`).
    pub fn sync(&mut self, at: (f32, f32)) {
        if (at.0 - self.cur.0).abs() > 0.5 || (at.1 - self.cur.1).abs() > 0.5 {
            self.cur = at;
            self.target = at;
            self.reported = at;
            self.last = None;
        }
    }

    /// A new screen: nothing carries over.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// `VirtualMouseHandler.handleControllerInput` + `updateMouse`, one frame of `dt` seconds.
    pub fn step(&mut self, dt: f32, i: &VIn, points: &[SnapPoint]) -> VOut {
        let k = dt / TICK; // ticks passed
        let impulse = ease_length(i.stick.0, i.stick.1, |x| x.powi(3));
        // if just released the stick, snap to the nearest point
        if impulse == (0.0, 0.0) && self.prev_impulse != (0.0, 0.0) {
            let (last, best) = snap_closest(self.last, self.target, points);
            self.last = last;
            if let Some(p) = best {
                self.snap_to(p);
            }
        }
        self.prev_impulse = impulse;
        let (mx, my) = if i.lce {
            let m = 10.0 * i.sens * i.gs; // screenWidth / guiScaledWidth
            (m, m)
        } else {
            let m = 20.0 * i.sens * i.bounds.0.max(i.bounds.1) / 800.0;
            (m, m)
        };
        self.target.0 = (self.target.0 + impulse.0 * mx * k).clamp(0.0, (i.bounds.0 - 1.0).max(0.0));
        self.target.1 = (self.target.1 + impulse.1 * my * k).clamp(0.0, (i.bounds.1 - 1.0).max(0.0));
        // the d-pad: one shared repeat, the first held direction in the order up, down, left, right
        for (n, d) in Dir::ALL.into_iter().enumerate() {
            if self.repeat.should_action(dt, i.dirs[n], self.prev_dirs[n]) {
                if let Some(p) = snap_in_direction(self.last, d, self.target, i.bounds, points) {
                    self.snap_to(p);
                }
                self.repeat.on_navigate();
                break;
            }
        }
        self.prev_dirs = i.dirs;
        if i.scrolls {
            self.scroll += i.scroll * k;
        }
        self.update(dt, i)
    }

    /// `updateMouse`: the cursor chases the target by `lerp(delta, current, target)`; the scroll hands out a share of what is owed.
    fn update(&mut self, dt: f32, i: &VIn) -> VOut {
        let delta = (dt / TICK).min(1.0);
        let differs = |a: f32, b: f32| ((a * 100.0).round() / 100.0) != ((b * 100.0).round() / 100.0);
        if differs(self.target.0, self.cur.0) || differs(self.target.1, self.cur.1) {
            self.cur.0 += (self.target.0 - self.cur.0) * delta;
            self.cur.1 += (self.target.1 - self.cur.1) * delta;
        } else {
            self.cur = self.target;
        }
        let mut o = VOut { to: (self.cur != self.reported).then_some(self.cur), scroll_px: 0.0 };
        self.reported = self.cur;
        if self.scroll.abs() >= 0.01 {
            let now = self.scroll * delta;
            self.scroll -= now;
            o.scroll_px = now * SCROLL_NOTCH * i.gs;
        } else {
            self.scroll = 0.0;
        }
        o
    }

    /// `snapToPoint`: the cursor is there at once, both ends.
    fn snap_to(&mut self, p: SnapPoint) {
        self.last = Some(p);
        self.cur = (p.x, p.y);
        self.target = (p.x, p.y);
    }

    /// `preventScrollingThisTick`.
    pub fn prevent_scrolling(&mut self) {
        self.scroll = 0.0;
    }

    /// Where the target is now (px).
    pub fn target(&self) -> (f32, f32) {
        self.target
    }
}

/// `renderVirtualMouse`: `virtual_mouse.png` at half size (a 32 texel image is 16 GUI units), centred on `(x, y)`. `s` is px per GUI unit.
pub fn draw_cursor(v: &mut Vec<HudVertex>, x: f32, y: f32, s: f32) {
    let u = s * 0.5; // px per texel
    for &(rx, ry, n, c) in RUNS.iter() {
        let col = [f32::from(c[0]) / 255.0, f32::from(c[1]) / 255.0, f32::from(c[2]) / 255.0, f32::from(c[3]) / 255.0];
        HudPipeline::push_quad(v, x + (f32::from(rx) - 16.0) * u, y + (f32::from(ry) - 16.0) * u, f32::from(n) * u, u, col);
    }
}

/// Minecraft's arrow navigation for a focus screen (the title, the game menu): from the target under `at` to the one that `d` (x, y; y down)
/// leads to. A target that overlaps the current one across the direction wins by the least gap (what `ContainerEventHandler` does with
/// `overlapsInAxis`); with none the old geometric step (`ui::step`) takes over, since this port's pages are not all one column.
/// Off every target, the nearest one. UNVERIFIED against the Java's exact tie-breaks.
pub fn focus_step(targets: &[[f32; 4]], at: (f32, f32), d: (i32, i32)) -> Option<[f32; 4]> {
    let Some(cur) = crate::ui::under(targets, at) else {
        let mid = |r: &[f32; 4]| (r[0] + r[2] * 0.5, r[1] + r[3] * 0.5);
        let far = |r: &[f32; 4]| (mid(r).0 - at.0).hypot(mid(r).1 - at.1);
        return targets.iter().copied().min_by(|a, b| far(a).total_cmp(&far(b)));
    };
    let horizontal = d.0 != 0;
    let sign = if horizontal { d.0 } else { d.1 } as f32;
    let overlap = |r: &[f32; 4]| if horizontal { r[1] < cur[1] + cur[3] && cur[1] < r[1] + r[3] } else { r[0] < cur[0] + cur[2] && cur[0] < r[0] + r[2] };
    let gap = |r: &[f32; 4]| {
        // the distance from our leading edge to its trailing one, along the direction
        if horizontal {
            if sign > 0.0 { r[0] - (cur[0] + cur[2]) } else { cur[0] - (r[0] + r[2]) }
        } else if sign > 0.0 {
            r[1] - (cur[1] + cur[3])
        } else {
            cur[1] - (r[1] + r[3])
        }
    };
    let best = targets.iter().copied().filter(|r| *r != cur && overlap(r) && gap(r) >= -0.5).min_by(|a, b| gap(a).total_cmp(&gap(b)));
    best.or_else(|| crate::ui::step(targets, at, d))
}

#[cfg(test)]
mod tests {
    use super::*;

    const B: (f32, f32) = (1000.0, 600.0);

    fn pts() -> Vec<SnapPoint> {
        // a 3 x 2 grid of slots, 100 px apart, range 40
        let mut v = Vec::new();
        for y in 0..2 {
            for x in 0..3 {
                v.push(SnapPoint { x: 100.0 + 100.0 * x as f32, y: 100.0 + 100.0 * y as f32, range: 40.0 });
            }
        }
        v
    }

    fn input(stick: (f32, f32), dirs: [bool; 4]) -> VIn {
        VIn { stick, scroll: 0.0, dirs, sens: 1.0, lce: false, bounds: B, gs: 3.0, scrolls: true }
    }

    #[test]
    fn easing_acts_on_the_length_and_keeps_the_direction() {
        let (x, y) = ease_length(0.6, 0.8, |l| l.powi(3));
        assert!((x - 0.6).abs() < 1e-5 && (y - 0.8).abs() < 1e-5, "length 1 stays 1: {x} {y}");
        let (x, y) = ease_length(0.3, 0.4, |l| l.powi(3)); // length 0.5 -> 0.125
        assert!((x - 0.075).abs() < 1e-5 && (y - 0.1).abs() < 1e-5, "{x} {y}");
        assert_eq!(ease_length(0.0, 0.0, |l| l), (0.0, 0.0));
        let (x, y) = ease_length(0.5, 0.5, |l| l.powi(3));
        assert!((x - y).abs() < 1e-6, "a diagonal stays a diagonal");
    }

    #[test]
    fn hold_repeat_acts_on_the_press_then_after_the_first_delay_then_every_repeat() {
        let mut h = HoldRepeat::new(10, 3);
        let mut acts = vec![];
        let mut prev = false;
        for f in 0..60 {
            let down = true;
            if h.should_action(TICK, down, prev) {
                h.on_navigate();
                acts.push(f);
            }
            prev = down;
        }
        assert_eq!(acts[0], 0, "the press");
        assert_eq!(acts[1], 10, "10 ticks later");
        assert_eq!(acts[2] - acts[1], 3, "then every 3");
        assert!(!h.should_action(TICK, false, true), "not held: nothing");
    }

    #[test]
    fn the_target_moves_by_the_java_step() {
        let mut m = VMouse::default();
        m.sync((500.0, 300.0));
        // full deflection right, one tick: 20 * 1.0 * max(1000, 600) / 800 = 25 px
        m.step(TICK, &input((1.0, 0.0), [false; 4]), &[]);
        assert!((m.target().0 - 525.0).abs() < 1e-3, "{:?}", m.target());
        // half deflection is an eighth of the speed (0.5^3)
        let mut m = VMouse::default();
        m.sync((500.0, 300.0));
        m.step(TICK, &input((0.5, 0.0), [false; 4]), &[]);
        assert!((m.target().0 - 500.0 - 25.0 * 0.125).abs() < 1e-3);
    }

    #[test]
    fn lce_speed_is_ten_a_tick_times_the_gui_scale() {
        let mut m = VMouse::default();
        m.sync((500.0, 300.0));
        let mut i = input((1.0, 0.0), [false; 4]);
        i.lce = true;
        m.step(TICK, &i, &[]);
        assert!((m.target().0 - 530.0).abs() < 1e-3, "10 * 1.0 * 3: {:?}", m.target());
    }

    #[test]
    fn the_cursor_chases_the_target_and_arrives() {
        let mut m = VMouse::default();
        m.sync((500.0, 300.0));
        // one 60 Hz frame of full deflection: the target moves a third of a tick's 25 px and the cursor a third of the way there
        let first = m.step(1.0 / 60.0, &input((1.0, 0.0), [false; 4]), &[]).to;
        assert!(first.is_some_and(|p| p.0 > 500.0 && p.0 < m.target().0), "behind its target: {first:?}");
        let mut last = first;
        for _ in 0..200 {
            last = m.step(1.0 / 60.0, &input((0.0, 0.0), [false; 4]), &[]).to.or(last);
        }
        assert!((last.unwrap().0 - m.target().0).abs() < 0.5, "and arrives");
    }

    #[test]
    fn releasing_the_stick_snaps_to_the_closest_point_in_range() {
        let mut m = VMouse::default();
        m.sync((180.0, 110.0)); // 20 px from (200, 100)
        m.step(TICK, &input((0.2, 0.0), [false; 4]), &pts()); // a nudge
        let o = m.step(TICK, &input((0.0, 0.0), [false; 4]), &pts());
        assert_eq!(o.to, Some((200.0, 100.0)), "released: onto the slot");
        // out of range of everything: stays
        let mut m = VMouse::default();
        m.sync((150.0, 150.0)); // 70.7 from the nearest: out of range 40
        m.step(TICK, &input((0.2, 0.0), [false; 4]), &pts());
        let before = m.target();
        m.step(TICK, &input((0.0, 0.0), [false; 4]), &pts());
        assert_eq!(m.target(), before);
    }

    #[test]
    fn it_does_not_snap_back_to_the_point_it_rests_on() {
        let p = pts();
        let (last, best) = snap_closest(Some(p[1]), (201.0, 101.0), &p);
        assert_eq!(last, Some(p[1]), "still inside the range: remembered");
        assert!(best.is_none(), "and not chosen again");
        let (last, best) = snap_closest(Some(p[1]), (262.0, 101.0), &p);
        assert!(last.is_none(), "left its range: forgotten");
        assert_eq!(best, Some(p[2]), "262 is inside the range of (300, 100)");
    }

    #[test]
    fn the_d_pad_snaps_to_the_neighbour_and_wraps_round() {
        let p = pts();
        let from = p[1]; // (200, 100)
        let got = snap_in_direction(Some(from), Dir::Right, (200.0, 100.0), B, &p);
        assert_eq!(got, Some(p[2]));
        let got = snap_in_direction(Some(from), Dir::Down, (200.0, 100.0), B, &p);
        assert_eq!(got, Some(p[4]), "straight down, not the diagonal ones");
        // nothing to the right of the last column: wraps to the first one in the row
        let got = snap_in_direction(Some(p[2]), Dir::Right, (300.0, 100.0), B, &p);
        assert_eq!(got, Some(p[0]));
        // a point that is too near (under its range) is not a neighbour
        let near = vec![SnapPoint { x: 100.0, y: 100.0, range: 40.0 }, SnapPoint { x: 120.0, y: 100.0, range: 40.0 }];
        assert_eq!(find_orthogonal(Some(near[0]), Dir::Right, (100.0, 100.0), &near), None);
    }

    #[test]
    fn a_held_d_pad_steps_then_repeats() {
        let p = pts();
        let mut m = VMouse::default();
        m.sync((100.0, 100.0));
        let right = [false, false, false, true];
        let mut xs = vec![];
        for _ in 0..40 {
            m.step(TICK, &input((0.0, 0.0), right), &p);
            xs.push(m.target().0 as i32);
        }
        assert_eq!(xs[0], 200, "the press steps at once");
        assert!(xs.iter().any(|&x| x == 300), "and again after the first delay");
        assert!(!xs.iter().take(8).any(|&x| x == 300), "not before it");
    }

    #[test]
    fn scroll_hands_out_a_share_each_frame_and_ends() {
        let mut m = VMouse::default();
        let mut i = input((0.0, 0.0), [false; 4]);
        i.scroll = 1.0;
        let first = m.step(TICK, &i, &[]).scroll_px;
        assert!(first > 0.0);
        i.scroll = 0.0;
        let mut total = first;
        for _ in 0..100 {
            total += m.step(TICK, &i, &[]).scroll_px;
        }
        assert!((total - 1.0 * SCROLL_NOTCH * 3.0).abs() < 0.3, "all of a notch comes out: {total}");
        i.scrolls = false;
        i.scroll = 1.0;
        assert_eq!(m.step(TICK, &i, &[]).scroll_px, 0.0, "a screen that does not scroll gets none");
    }

    #[test]
    fn widget_and_slot_points_follow_the_mixins() {
        let w = SnapPoint::widget([100.0, 200.0, 600.0, 60.0], 3.0);
        assert_eq!((w.x, w.y), (400.0, 230.0));
        assert_eq!(w.range, 30.0 + 30.0, "min(w, h) / 2 + 10 GUI units");
        let s = SnapPoint::slot([90.0, 90.0, 54.0, 54.0], 3.0);
        assert_eq!((s.x, s.y, s.range), (117.0, 117.0, 51.0));
    }

    #[test]
    fn focus_goes_to_the_button_below_and_stays_on_the_edge() {
        let col = [[100.0, 100.0, 200.0, 20.0], [100.0, 130.0, 200.0, 20.0], [100.0, 160.0, 200.0, 20.0]];
        assert_eq!(focus_step(&col, (150.0, 110.0), (0, 1)), Some(col[1]));
        assert_eq!(focus_step(&col, (150.0, 170.0), (0, -1)), Some(col[1]));
        assert_eq!(focus_step(&col, (150.0, 170.0), (0, 1)), None, "nothing below the last");
        let two = [[100.0, 100.0, 95.0, 20.0], [205.0, 100.0, 95.0, 20.0]];
        assert_eq!(focus_step(&two, (110.0, 110.0), (1, 0)), Some(two[1]));
    }

    #[test]
    fn the_cursor_is_drawn_from_runs_around_its_centre() {
        let mut v = Vec::new();
        draw_cursor(&mut v, 100.0, 100.0, 2.0);
        assert!(!v.is_empty() && v.len() % 6 == 0);
        assert!(v.iter().all(|p| (p.pos[0] - 100.0).abs() <= 16.0 && (p.pos[1] - 100.0).abs() <= 16.0), "32 texels at 1 px each: 16 px either way");
    }
}
