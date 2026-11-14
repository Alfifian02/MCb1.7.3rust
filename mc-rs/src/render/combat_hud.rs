//! Cortes Combat's HUD (C9): the crosshair and what it says, the cooldown brackets and bars, the charge bar, the guard orbs above the
//! crosshair, and the damage vignette / flash. **NOT b1.7.3**, only with the Immersive Pack ON (`world::combat::rules::on`).
//! UNVERIFIED: written without a Rust toolchain, not compiled, not run, not seen on a device.
//!
//! Ported from the mod's `CrosshairFeedback`, `CrosshairSprites`, `CombatCrosshairLayer`, `CooldownBracketLayer`, `ConeGauge`,
//! `ChargeBar`, `GuardOrbsLayer`, `DamageOverlayLayer` and `Fade`. Everything that is arithmetic is pure (no GPU, no world, no clock: the caller
//! passes `dt` and the health), so `cargo test --lib combat_hud` runs on a plain host. Sizes are in GUI pixels (`gu` px on screen; the
//! crosshair sprite is 15 of them), the way the mod measures them, so the numbers read the same as the Java's.
//!
//! Not ported: `hotbar_attack_indicator_full.png` (nothing in the mod draws it), `HealthFlashBuffer` and `DamageFlash` (those tint an
//! entity's model, not the HUD), the purple special-charge bar (it is the shovel's orb, dropped: the arcabuz's reload and fuse use the plain bar, C8), `BracketMask` (it cut the
//! bracket sprite into runs because `GuiGraphics.blit` ignores a tint; here the sprite is drawn tinted, so the mask has no use).
//! ponytail: the cone's span uses the 70 degree base view, not the sprint's or the zoom's live one (a bracket that breathes with the
//! sprint would read as the weapon changing); the vignette is a 16 x 10 grid of gradient cells, not `damage_overlay.png` (its texels are
//! fainter than the 0.5 alpha `fs_main` keeps), fitted to the picture's alpha: 11 / 255 in the middle, 175 on the edges, 220 in the corners.

use super::hud::{
    sprites::{cc, Sprite},
    HudPipeline, HudVertex,
};

// ---- the crosshair's reaction (`CrosshairFeedback`, `CombatFeedback`) ----

/// What just happened to the player, as far as the crosshair is concerned (`CombatFeedback`). No "missed": a swing at nothing already costs
/// the cooldown and shows in the brackets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Feedback {
    /// A landed hit: the crosshair grows, shakes and goes dark red.
    Hit,
    /// A landed hit that killed: the same, longer, a louder red.
    Kill,
    /// A hit blocked by the guard: blue grey, no growth or shake (nothing was thrown at you).
    Guarded,
}

impl Feedback {
    /// Ticks the colour stays (`CombatFeedback.tintTicks`).
    pub fn ticks(self) -> i32 {
        match self {
            Feedback::Hit => 6,
            Feedback::Kill => 12,
            Feedback::Guarded => 8,
        }
    }

    fn rgb(self) -> u32 {
        match self {
            Feedback::Hit => 0x8C1A14,
            Feedback::Kill => 0xE11B1B,
            Feedback::Guarded => 0x8296AD,
        }
    }
}

/// Extra scale while a target is in reach, and on the tick a hit lands (decaying). The aim one is small on purpose: a hint that is loud all
/// the time says nothing on the one frame that matters.
const AIM_GROWTH: f32 = 0.14;
const HIT_GROWTH: f32 = 0.38;
/// How fast the aim growth catches up, per tick (a target on the edge of the cone flickers; a crosshair that strobes is worse than a late one).
const AIM_APPROACH: f32 = 0.35;
/// The shake path, in GUI pixels, one step a tick: a ring that starts and ends at the origin, so the crosshair rests exactly centred.
const SHAKE_CYCLE: [(i32, i32); 8] = [(0, 0), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1)];
/// The crosshair's size in GUI pixels (`CrosshairSprites.SIZE`) and its weight (the world shows through; `ALPHA`).
pub const SIZE: f32 = 15.0;
const CROSS_ALPHA: f32 = 0.85;

/// The crosshair's state, ticked at 20 Hz (`CrosshairFeedback`). The shake is seeded, not random: the same swing always shakes the same way.
#[derive(Default)]
pub struct Crosshair {
    kind: Option<Feedback>,
    /// Ticks since the event landed.
    age: i32,
    /// Bumped per event: the only "randomness", and it is not random.
    seed: u32,
    /// Eased 0..1 approach to "there is something to hit".
    aim: f32,
    /// Whether the held weapon has something in its cone right now (picks the aim sprite).
    aiming: bool,
}

impl Crosshair {
    pub fn event(&mut self, kind: Feedback) {
        self.kind = Some(kind);
        self.age = 0;
        self.seed = self.seed.wrapping_add(1);
    }

    /// One 20 Hz tick; `aiming` = the held weapon has a target in its cone.
    pub fn tick(&mut self, aiming: bool) {
        if let Some(k) = self.kind {
            self.age += 1;
            if self.age >= k.ticks() {
                self.kind = None;
            }
        }
        self.aiming = aiming;
        let target = if aiming { 1.0 } else { 0.0 };
        self.aim += (target - self.aim) * AIM_APPROACH;
        if self.aim < 0.001 {
            self.aim = 0.0;
        }
    }

    /// 0 at the end of the flash, 1 on the frame it starts: square rooted, so it holds its strength for the first third and a short flash
    /// reads as a flash and not as a tint that never quite arrived.
    fn envelope(&self) -> f32 {
        self.kind.map_or(0.0, |k| (1.0 - self.age as f32 / k.ticks() as f32).max(0.0).sqrt())
    }

    /// Whether a hit (not a block) flash runs: what picks the hit sprite, asked as a question rather than guessed from the size.
    pub fn hit_flashing(&self) -> bool {
        matches!(self.kind, Some(k) if k != Feedback::Guarded)
    }

    /// Scale of the 15 x 15 sprite: the hit growth jumps and decays (an ease-in would read as a slow camera move), the aim one eases.
    pub fn growth(&self) -> f32 {
        1.0 + AIM_GROWTH * self.aim + if self.hit_flashing() { HIT_GROWTH * self.envelope() } else { 0.0 }
    }

    /// The shake, in GUI pixels: 2 while the flash is young, 1 while it dies. Always (0, 0) outside a landed hit.
    pub fn shake(&self) -> (i32, i32) {
        let Some(k) = self.kind.filter(|&k| k != Feedback::Guarded) else { return (0, 0) };
        let (x, y) = SHAKE_CYCLE[(self.age as u32).wrapping_add(self.seed) as usize % SHAKE_CYCLE.len()];
        let amp = if self.age < k.ticks() / 2 { 2 } else { 1 };
        (x * amp, y * amp)
    }

    /// The tint of the crosshair and the brackets: white slid towards the event's colour (not faded with alpha: a hard-edged sprite that
    /// dissolves looks wrong, one whose flash is absorbed looks right).
    pub fn rgb(&self) -> [f32; 3] {
        let (from, to) = (0xFFFFFF_u32, self.kind.map_or(0xFFFFFF_u32, Feedback::rgb));
        let t = self.envelope();
        let ch = |shift: u32| {
            let (a, b) = (((from >> shift) & 0xFF) as f32, ((to >> shift) & 0xFF) as f32);
            ((a + (b - a) * t).round()) / 255.0
        };
        [ch(16), ch(8), ch(0)]
    }

    /// The sprite of the state: hit outranks aim.
    pub fn sprite(&self) -> Sprite {
        if self.hit_flashing() {
            cc::CROSS_HIT
        } else if self.aiming {
            cc::CROSS_AIM
        } else {
            cc::CROSS
        }
    }

    /// How far the crosshair reaches from its centre now (GUI px): where the bars start and the charge bar's clearance is measured from.
    pub fn half(&self) -> f32 {
        SIZE * self.growth() * 0.5
    }
}

// ---- the cone gauge: brackets at the cone's edge, a bar from the crosshair out to each (`ConeGauge`) ----

/// The bar stops this many GUI px short of the bracket, so the two never touch.
pub const BAR_INSET: f32 = 5.0;
const MIN_HALF_SPAN: f32 = 42.0;
/// The widest the brackets get, as a fraction of the screen's width (a sabre's 75 degrees would put them off the screen).
const MAX_SCREEN_FRACTION: f32 = 0.42;
/// How hard the tangent is bent so a wide cone does not run off to infinity: `t / (1 + t / SATURATION)`.
const SATURATION: f32 = 2.0;
const ALPHA_NEAR_BRACKET: f32 = 175.0 / 255.0;
const WIDGET_ALPHA: f32 = 0.55;
/// The brackets' weight (`ConeGauge.BRACKET_ALPHA`; the art's own alpha is 38 / 255 and `cc` draws it at 1).
pub const BRACKET_ALPHA: f32 = 0.30;
/// The bars are 2 GUI px thick (3 read as a slab laid over the middle of the screen, 1 can fall between two display pixels).
const BAR_THICKNESS: f32 = 2.0;
/// The bar's gradient is drawn in cells this many columns wide, each linear: the smoothstep is gentle enough that 4 px steps cannot be seen.
const BAR_CELL: i32 = 4;

/// Half the cone's width on screen, in GUI px, from its horizontal half-angle: the pixels one radian covers at this view times the bent
/// tangent (`ConeGauge.halfSpan`). Size comes from the angle, not the reach: the widget answers "how wide", and a sabre must look wider
/// than a ropera.
pub fn half_span(angle: f32, gui_w: f32, gui_h: f32, fov_deg: f32) -> f32 {
    if gui_h <= 0.0 || fov_deg <= 0.0 || fov_deg >= 180.0 {
        return MIN_HALF_SPAN;
    }
    let per_radian = (gui_h / 2.0) / (fov_deg.to_radians() / 2.0).tan();
    let t = angle.clamp(0.0, 89.0).to_radians().tan();
    let span = per_radian * t / (1.0 + t / SATURATION);
    if !span.is_finite() || span <= 0.0 {
        return MIN_HALF_SPAN;
    }
    let span = if gui_w > 0.0 { span.round().min((gui_w * MAX_SCREEN_FRACTION).round()) } else { span.round() };
    span.max(MIN_HALF_SPAN)
}

/// One geometry for the bars and the brackets, so they cannot disagree by a pixel.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Geometry {
    pub half_span: f32,
    /// Where the bar starts (the crosshair's edge) and the columns it has, out to `BAR_INSET` short of the bracket.
    pub outer: f32,
    pub length: i32,
}

pub fn geometry(angle: f32, gui_w: f32, gui_h: f32, fov_deg: f32, reach: f32) -> Geometry {
    let half_span = half_span(angle, gui_w, gui_h, fov_deg);
    Geometry { half_span, outer: reach, length: ((half_span - BAR_INSET) - reach).round().max(0.0) as i32 }
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The bar's alpha at column `i` of `length`: 0 by the crosshair (the moving edge stays out of the middle of the screen) up to the
/// bracket's end (`ConeGauge.barAlpha`, before the fill cuts it).
pub fn bar_alpha(i: i32, length: i32) -> f32 {
    if length <= 0 || i < 0 {
        return 0.0;
    }
    let t = if length <= 1 { 1.0 } else { i as f32 / (length - 1) as f32 };
    smoothstep(t) * ALPHA_NEAR_BRACKET * WIDGET_ALPHA
}

/// A run of the bar, `x` from the centre (GUI px, its left edge), alpha at its left and right edges.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Cell {
    pub x: f32,
    pub w: f32,
    pub a_left: f32,
    pub a_right: f32,
}

/// One side's bar filled to `ratio` (0 = the swing is not ready at all, 1 = ready), from the crosshair outwards: a single continuous line
/// whose alpha folds the gradient and the fill (two passes, a socket and a fill, left a visible step at the moving edge). `side` is -1 for the
/// left bar, 1 for the right.
pub fn bar_cells(g: &Geometry, ratio: f32, side: i32) -> Vec<Cell> {
    let filled = (g.length as f32 * ratio.clamp(0.0, 1.0)).round() as i32;
    let side_f = side as f32;
    let (mut out, mut i) = (Vec::new(), 0);
    while i < filled {
        let j = (i + BAR_CELL).min(filled);
        let (near, far) = (bar_alpha(i, g.length), bar_alpha(j - 1, g.length));
        let o = side_f * g.outer; // the column next to the crosshair; columns run outwards from here
        out.push(if side > 0 {
            Cell { x: o + i as f32, w: (j - i) as f32, a_left: near, a_right: far }
        } else {
            Cell { x: o - j as f32, w: (j - i) as f32, a_left: far, a_right: near }
        });
        i = j;
    }
    out
}

/// How ready the swing is, 0..1: the cooldown's share that has run (1 with nothing running, or after the item changed).
pub fn cooldown_ratio(swing_cd: u32, interval: u32, same_item: bool) -> f32 {
    if swing_cd == 0 || interval == 0 || !same_item { 1.0 } else { 1.0 - (swing_cd as f32 / interval as f32).min(1.0) }
}

// ---- drawing ----

fn rgba([r, g, b]: [f32; 3], a: f32) -> [f32; 4] {
    [r, g, b, a]
}

/// The crosshair, grown from its centre (the mod grows it from the corner, which slides the centre down and to the right a pixel at a time).
pub fn draw_crosshair(out: &mut Vec<HudVertex>, gu: f32, (cx, cy): (f32, f32), xh: &Crosshair) {
    let size = SIZE * xh.growth() * gu;
    let (sx, sy) = xh.shake();
    let (x, y) = (cx - size * 0.5 + sx as f32 * gu, cy - size * 0.5 + sy as f32 * gu);
    HudPipeline::push_gui_tinted(out, x, y, size, size, xh.sprite(), rgba(xh.rgb(), cc::ALPHA_CROSS * CROSS_ALPHA));
}

/// The two bars and the two brackets (`CooldownBracketLayer`). `angle` is the held weapon's horizontal half-angle in degrees, `(w, h)` the
/// screen in px, `fov_deg` the view the cone is projected through.
pub fn draw_gauge(out: &mut Vec<HudVertex>, gu: f32, (cx, cy): (f32, f32), xh: &Crosshair, angle: f32, (w, h): (f32, f32), fov_deg: f32, ratio: f32) {
    let g = geometry(angle, w / gu, h / gu, fov_deg, xh.half());
    let tint = xh.rgb();
    for side in [-1, 1] {
        for c in bar_cells(&g, ratio, side) {
            let (l, r) = (rgba(tint, c.a_left), rgba(tint, c.a_right));
            HudPipeline::push_quad_grad(out, cx + c.x * gu, cy - BAR_THICKNESS * 0.5 * gu, c.w * gu, BAR_THICKNESS * gu, [l, r, r, l]);
        }
    }
    // Each bracket's ink (4 columns of the 23 x 15 art) is centred on the cone's edge, so the pair is symmetric about the crosshair.
    let top = cy - 7.5 * gu;
    let t = rgba(tint, BRACKET_ALPHA);
    HudPipeline::push_gui_tinted(out, cx - (g.half_span + 2.0) * gu, top, 4.0 * gu, 15.0 * gu, cc::BRACKET_L, t);
    HudPipeline::push_gui_tinted(out, cx + (g.half_span - 2.0) * gu, top, 4.0 * gu, 15.0 * gu, cc::BRACKET_R, t);
}

/// The charge bar under the crosshair (`ChargeBar`): the empty socket, then the full art cropped to the charge (the splash at the end of the
/// stroke shows only once the charge is full). The gap is the crosshair's own reach plus a margin, so the two never touch however far a hit
/// makes the crosshair jump. Nothing is drawn at 0: an empty bar on its socket would appear before the player pressed anything.
pub fn draw_charge(out: &mut Vec<HudVertex>, gu: f32, (cx, cy): (f32, f32), xh: &Crosshair, ratio: f32) {
    if ratio <= 0.0 {
        return;
    }
    const W: f32 = 30.0;
    const H: f32 = 15.0;
    const GAP: f32 = 9.0;
    let (x, y) = (cx - W * 0.5 * gu, cy + (xh.half() + GAP) * gu);
    HudPipeline::push_gui_tinted(out, x, y, W * gu, H * gu, cc::CHARGE_EMPTY, [1.0, 1.0, 1.0, cc::ALPHA_CHARGE_EMPTY]);
    let filled = (W * ratio.min(1.0)).round().max(1.0);
    let (sx, sy, _, sh) = cc::CHARGE;
    HudPipeline::push_gui_tinted(out, x, y, filled * gu, H * gu, (sx, sy, filled, sh), [1.0, 1.0, 1.0, cc::ALPHA_CHARGE]);
}

/// The guard's points above the crosshair (`GuardOrbsLayer`): a full orb a point you can still block, a spent one grey. White with a varying
/// alpha: they carry no colour of their own, so a blocked hit does not paint three things red. `up` = the guard is raised or the item's
/// stagger runs (it stays bright while the orbs refill: dimming them then says the opposite of what the player is watching).
pub fn draw_orbs(out: &mut Vec<HudVertex>, gu: f32, (cx, cy): (f32, f32), points: u32, max: u32, up: bool) {
    if max == 0 {
        return;
    }
    const ORB: f32 = 9.0;
    const SPACING: f32 = 3.0;
    let total = max as f32 * ORB + (max - 1) as f32 * SPACING;
    let dim = if up { 1.0 } else { 110.0 / 255.0 };
    for i in 0..max {
        let (spr, a) = if i < points { (cc::SHIELD, cc::ALPHA_SHIELD) } else { (cc::SHIELD_OFF, cc::ALPHA_SHIELD_OFF) };
        let x = cx + (-total * 0.5 + i as f32 * (ORB + SPACING)) * gu;
        HudPipeline::push_gui_tinted(out, x, cy - 14.0 * gu, ORB * gu, ORB * gu, spr, [1.0, 1.0, 1.0, a * dim]);
    }
}

// ---- the gauges (NOT b1.7.3, UNVERIFIED) ----

/// What the body is doing that the hearts do not show: heat or cold on it (`biomelife::air`), a bandage working, the hearth ticking.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    Hot,
    Cold,
    Bandage,
    Hearth,
}

/// Below this `hot` or `cold` the weather is not worth a mark.
const MARK_MIN: f32 = 0.05;

/// The marks to show, in a fixed order (so one does not jump when another comes and goes).
pub fn marks(hot: f32, cold: f32, bandaged: bool, hearth: bool) -> Vec<Mark> {
    [(hot > MARK_MIN, Mark::Hot), (cold > MARK_MIN, Mark::Cold), (bandaged, Mark::Bandage), (hearth, Mark::Hearth)].into_iter().filter(|m| m.0).map(|m| m.1).collect()
}

/// Draw `marks` in a row from `(x, y)`, each in a square of `s` pixels (a 5 x 5 pixel glyph of flat quads, 1.25 `s` apart).
pub fn draw_marks(out: &mut Vec<HudVertex>, (x, y): (f32, f32), s: f32, marks: &[Mark]) {
    let u = s / 5.0;
    for (i, m) in marks.iter().enumerate() {
        let x0 = x + i as f32 * s * 1.25;
        // (x, y, w, h) in glyph pixels.
        let (col, cells): ([f32; 4], &[(f32, f32, f32, f32)]) = match m {
            Mark::Hot => ([1.0, 0.65, 0.2, 0.95], &[(1.0, 1.0, 3.0, 3.0), (2.0, 0.0, 1.0, 1.0), (2.0, 4.0, 1.0, 1.0), (0.0, 2.0, 1.0, 1.0), (4.0, 2.0, 1.0, 1.0)]),
            Mark::Cold => ([0.7, 0.88, 1.0, 0.95], &[(0.0, 2.0, 5.0, 1.0), (2.0, 0.0, 1.0, 5.0), (0.0, 0.0, 1.0, 1.0), (4.0, 0.0, 1.0, 1.0), (0.0, 4.0, 1.0, 1.0), (4.0, 4.0, 1.0, 1.0)]),
            Mark::Bandage => ([0.45, 0.9, 0.45, 0.95], &[(0.0, 1.5, 5.0, 2.0), (1.5, 0.0, 2.0, 5.0)]),
            Mark::Hearth => ([1.0, 0.45, 0.15, 0.95], &[(1.0, 3.0, 3.0, 2.0), (1.5, 1.5, 2.0, 1.5), (2.0, 0.0, 1.0, 1.5)]),
        };
        for &(cx, cy, w, h) in cells {
            HudPipeline::push_quad(out, x0 + cx * u, y + cy * u, w * u, h * u, col);
        }
    }
}

// ---- the damage vignette and flash (`DamageOverlayLayer`, `Fade`) ----

/// Above this health the vignette is gone, at `FADE_END` it is full (the mod's 20-point scale is b1.7.3's too).
const FADE_START: f32 = 14.0;
const FADE_END: f32 = 4.0;
const FADE_RATE: f32 = 6.0;
const FLASH_RATE: f32 = 5.0;
/// A new hit may re-arm the flash only once it has died below this (or when it is stronger), so a burst of small hits is not a strobe.
const FLASH_REARM: f32 = 0.35;
const OVERLAY_OPACITY: f32 = 0.75;
const FLASH_OPACITY: f32 = 0.55;
const SHOWN: f32 = 0.001;
/// The picture's colour (its red runs 86..206 with green and blue at 7; the mean).
const VIGNETTE_RGB: [f32; 3] = [0.45, 0.03, 0.03];
const FLASH_RGB: [f32; 3] = [0.8, 0.1, 0.1];

/// `Fade.toward`: an exponential ease, framerate independent.
fn toward(from: f32, to: f32, rate: f32, dt: f32) -> f32 {
    if dt <= 0.0 || !dt.is_finite() { from } else { from + (to - from) * (1.0 - (-rate * dt).exp()) }
}

/// `Fade.decay`.
fn decay(value: f32, rate: f32, dt: f32) -> f32 {
    if value <= 0.0 { 0.0 } else if dt <= 0.0 || !dt.is_finite() { value } else { value * (-rate * dt).exp() }
}

/// The two screen effects of getting hurt: a red vignette that grows as the health falls (14 down to 4) and a flash that a hit sets in
/// proportion to its damage (an eighth of the screen's worth a point) and that fades fast.
#[derive(Default)]
pub struct DamageFx {
    vignette: f32,
    flash: f32,
    /// The health of the last frame; `None` before the first (and after a reset), so entering a world never flashes.
    last: Option<f32>,
}

impl DamageFx {
    /// One frame: `health` in b1.7.3's points (20 = full), `dt` the real frame time in seconds.
    pub fn frame(&mut self, health: f32, dt: f32) {
        if let Some(last) = self.last.filter(|&l| health < l + 0.499) {
            let incoming = ((last - health) * 0.125).clamp(0.0, 1.0);
            if incoming > self.flash || self.flash < FLASH_REARM {
                self.flash = self.flash.max(incoming);
            }
        }
        self.last = Some(health);
        let target = ((health - FADE_START) / (FADE_END - FADE_START)).clamp(0.0, 1.0);
        let eased = -(target * std::f32::consts::PI).cos() * 0.5 + 0.5;
        self.vignette = toward(self.vignette, eased, FADE_RATE, dt);
        self.flash = decay(self.flash, FLASH_RATE, dt);
    }

    pub fn visible(&self) -> bool {
        self.vignette > SHOWN || self.flash > SHOWN
    }

    /// Below everything else in the HUD: the vignette (a grid of gradient cells, 160 quads), then the flash (one quad).
    pub fn draw(&self, out: &mut Vec<HudVertex>, (w, h): (f32, f32)) {
        if self.vignette > SHOWN {
            vignette(out, (w, h), self.vignette * OVERLAY_OPACITY);
        }
        if self.flash > SHOWN {
            HudPipeline::push_quad(out, 0.0, 0.0, w, h, rgba(FLASH_RGB, self.flash * FLASH_OPACITY));
        }
    }
}

/// The picture's alpha at a point of the screen, `nx` / `ny` in -1..1: 11 / 255 in the middle up to 220 / 255 in the corners, by the
/// distance from the middle (the picture reads 175 at the middle of an edge, the fit 185).
fn vignette_alpha(nx: f32, ny: f32) -> f32 {
    let d = ((nx * nx + ny * ny).sqrt() / 1.2).min(1.0);
    (11.0 + (220.0 - 11.0) * d) / 255.0
}

const VIG_COLS: usize = 16;
const VIG_ROWS: usize = 10;

fn vignette(out: &mut Vec<HudVertex>, (w, h): (f32, f32), scale: f32) {
    let at = |c: usize, r: usize| {
        let (nx, ny) = (c as f32 / VIG_COLS as f32 * 2.0 - 1.0, r as f32 / VIG_ROWS as f32 * 2.0 - 1.0);
        rgba(VIGNETTE_RGB, vignette_alpha(nx, ny) * scale)
    };
    let (cw, ch) = (w / VIG_COLS as f32, h / VIG_ROWS as f32);
    for r in 0..VIG_ROWS {
        for c in 0..VIG_COLS {
            HudPipeline::push_quad_grad(out, c as f32 * cw, r as f32 * ch, cw, ch, [at(c, r), at(c + 1, r), at(c + 1, r + 1), at(c, r + 1)]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticks(xh: &mut Crosshair, n: usize, aiming: bool) {
        for _ in 0..n {
            xh.tick(aiming);
        }
    }

    // Gauges: a mark for each thing that is on, in a fixed order, and a glyph stays inside its square.
    #[test]
    fn the_marks_come_in_a_fixed_order_and_stay_in_their_squares() {
        assert!(marks(0.0, 0.04, false, false).is_empty());
        assert_eq!(marks(0.5, 0.2, true, true), [Mark::Hot, Mark::Cold, Mark::Bandage, Mark::Hearth]);
        assert_eq!(marks(0.0, 0.0, false, true), [Mark::Hearth]);
        let mut v = Vec::new();
        draw_marks(&mut v, (10.0, 20.0), 10.0, &[Mark::Cold, Mark::Hearth]);
        assert!(!v.is_empty() && v.len() % 6 == 0);
        assert!(v.iter().all(|p| p.pos[0] >= 10.0 && p.pos[0] <= 10.0 + 10.0 * 2.25 && p.pos[1] >= 20.0 && p.pos[1] <= 30.0));
    }

    #[test]
    fn a_resting_crosshair_is_the_plain_sprite_at_scale_one() {
        let xh = Crosshair::default();
        assert_eq!(xh.growth(), 1.0);
        assert_eq!(xh.shake(), (0, 0));
        assert_eq!(xh.rgb(), [1.0; 3]);
        assert_eq!(xh.sprite(), cc::CROSS);
        assert_eq!(xh.half(), 7.5);
    }

    #[test]
    fn aiming_eases_to_a_slightly_bigger_aim_sprite_and_back() {
        let mut xh = Crosshair::default();
        ticks(&mut xh, 1, true);
        assert!(xh.growth() > 1.0 && xh.growth() < 1.0 + AIM_GROWTH, "eased, not snapped");
        ticks(&mut xh, 30, true);
        assert!((xh.growth() - 1.14).abs() < 0.001);
        assert_eq!(xh.sprite(), cc::CROSS_AIM);
        ticks(&mut xh, 60, false);
        assert_eq!(xh.growth(), 1.0);
        assert_eq!(xh.sprite(), cc::CROSS);
    }

    #[test]
    fn a_hit_jumps_shakes_reddens_and_is_gone_in_six_ticks() {
        let mut xh = Crosshair::default();
        xh.event(Feedback::Hit);
        assert!((xh.growth() - 1.38).abs() < 0.001, "full growth on the first frame");
        assert_eq!(xh.sprite(), cc::CROSS_HIT, "the hit sprite outranks the aim one");
        assert!(xh.rgb()[0] < 0.6 && xh.rgb()[1] < 0.2, "dark red at the start");
        let g = xh.growth();
        ticks(&mut xh, 3, false);
        assert!(xh.growth() < g, "decays");
        ticks(&mut xh, 3, false);
        assert_eq!((xh.growth(), xh.shake(), xh.rgb()), (1.0, (0, 0), [1.0; 3]), "the sixth tick ends it, centred and white");
    }

    #[test]
    fn the_shake_never_leaves_two_pixels_and_is_one_pixel_in_its_second_half() {
        let mut xh = Crosshair::default();
        xh.event(Feedback::Kill);
        for age in 0..Feedback::Kill.ticks() {
            let (x, y) = xh.shake();
            assert!(x.abs() <= 2 && y.abs() <= 2);
            if age >= Feedback::Kill.ticks() / 2 {
                assert!(x.abs() <= 1 && y.abs() <= 1, "age {age}");
            }
            xh.tick(false);
        }
    }

    #[test]
    fn a_block_tints_but_neither_grows_nor_shakes() {
        let mut xh = Crosshair::default();
        xh.event(Feedback::Guarded);
        assert_eq!((xh.growth(), xh.shake(), xh.sprite()), (1.0, (0, 0), cc::CROSS));
        assert!(xh.rgb()[0] < 0.6, "blue grey");
        ticks(&mut xh, 8, false);
        assert_eq!(xh.rgb(), [1.0; 3]);
    }

    #[test]
    fn a_kill_lasts_twice_a_hit_and_two_hits_do_not_shake_alike() {
        assert_eq!(Feedback::Kill.ticks(), 2 * Feedback::Hit.ticks());
        let (mut a, mut b) = (Crosshair::default(), Crosshair::default());
        a.event(Feedback::Hit);
        b.event(Feedback::Hit);
        b.event(Feedback::Hit);
        ticks(&mut a, 1, false);
        ticks(&mut b, 1, false);
        assert_ne!(a.shake(), b.shake());
    }

    #[test]
    fn the_cone_is_wider_for_a_wider_angle_and_never_leaves_the_screen_or_the_floor() {
        let (w, h, fov) = (667.0, 300.0, 70.0);
        let (ropera, sabre) = (half_span(12.0, w, h, fov), half_span(75.0, w, h, fov));
        assert!(ropera >= MIN_HALF_SPAN && sabre > ropera);
        assert!(sabre <= (w * MAX_SCREEN_FRACTION).round(), "{sabre}");
        assert_eq!(half_span(35.0, w, h, 0.0), MIN_HALF_SPAN);
        assert_eq!(half_span(35.0, w, 0.0, fov), MIN_HALF_SPAN);
        assert_eq!(half_span(0.0, w, h, fov), MIN_HALF_SPAN);
        assert_eq!(half_span(35.0, w, h, 180.0), MIN_HALF_SPAN);
    }

    #[test]
    fn the_bar_runs_from_the_crosshair_to_five_short_of_the_bracket() {
        let g = geometry(35.0, 667.0, 300.0, 70.0, 7.5);
        assert_eq!(g.outer, 7.5);
        assert_eq!(g.length as f32, (g.half_span - BAR_INSET - 7.5).round());
        let grown = geometry(35.0, 667.0, 300.0, 70.0, 7.5 * 1.52);
        assert!(grown.length < g.length, "a bigger crosshair leaves a shorter bar, never one that overlaps it");
        let tiny = geometry(0.0, 667.0, 300.0, 70.0, 200.0);
        assert_eq!(tiny.length, 0);
    }

    #[test]
    fn the_bar_is_clear_by_the_crosshair_and_brightest_by_the_bracket() {
        assert_eq!(bar_alpha(0, 50), 0.0);
        assert!((bar_alpha(49, 50) - 175.0 / 255.0 * 0.55).abs() < 1e-5);
        assert!(bar_alpha(10, 50) < bar_alpha(30, 50));
        assert_eq!(bar_alpha(0, 0), 0.0);
        assert_eq!(bar_alpha(0, 1), 175.0 / 255.0 * 0.55);
    }

    #[test]
    fn the_bars_are_one_contiguous_line_each_side_mirrored_and_cut_at_the_fill() {
        let g = Geometry { half_span: 60.0, outer: 8.0, length: 47 };
        assert!(bar_cells(&g, 0.0, 1).is_empty());
        for side in [-1, 1] {
            let cells = bar_cells(&g, 1.0, side);
            assert_eq!(cells.iter().map(|c| c.w).sum::<f32>(), 47.0);
            let mut sorted = cells.clone();
            sorted.sort_by(|a, b| a.x.total_cmp(&b.x));
            assert!(sorted.windows(2).all(|p| (p[0].x + p[0].w - p[1].x).abs() < 1e-4), "no gaps");
            let (lo, hi) = (sorted[0].x, sorted.last().map(|c| c.x + c.w).unwrap());
            if side > 0 {
                assert_eq!((lo, hi), (8.0, 55.0), "starts at the crosshair's edge");
            } else {
                assert_eq!((lo, hi), (-55.0, -8.0));
            }
        }
        let half = bar_cells(&g, 0.5, 1);
        assert_eq!(half.iter().map(|c| c.w).sum::<f32>(), 24.0, "round(47 / 2)");
        // the alpha rises away from the crosshair on both sides (the left bar's near end is its right edge)
        let (l, r) = (bar_cells(&g, 1.0, -1), bar_cells(&g, 1.0, 1));
        assert!(r[0].a_left < r.last().unwrap().a_right && l[0].a_right < l.last().unwrap().a_left);
        assert_eq!(r[0].a_left, 0.0, "clear by the crosshair");
    }

    #[test]
    fn the_cooldown_ratio_is_the_share_that_has_run() {
        assert_eq!(cooldown_ratio(0, 12, true), 1.0);
        assert_eq!(cooldown_ratio(12, 12, true), 0.0);
        assert!((cooldown_ratio(3, 12, true) - 0.75).abs() < 1e-6);
        assert_eq!(cooldown_ratio(5, 12, false), 1.0, "another item: the cooldown is not its");
        assert_eq!(cooldown_ratio(5, 0, true), 1.0);
    }

    #[test]
    fn nothing_is_drawn_for_an_empty_charge_and_the_full_bar_is_cropped_to_the_ratio() {
        let (mut v, xh) = (Vec::new(), Crosshair::default());
        draw_charge(&mut v, 3.0, (500.0, 300.0), &xh, 0.0);
        assert!(v.is_empty());
        draw_charge(&mut v, 3.0, (500.0, 300.0), &xh, 0.5);
        assert_eq!(v.len(), 12, "the socket and the fill");
        let w = |q: &[HudVertex]| q.iter().map(|p| p.pos[0]).fold(f32::MIN, f32::max) - q.iter().map(|p| p.pos[0]).fold(f32::MAX, f32::min);
        assert!((w(&v[..6]) - 90.0).abs() < 1e-3 && (w(&v[6..]) - 45.0).abs() < 1e-3, "30 and 15 GUI px at 3 px each");
        let top = v[0].pos[1];
        assert!(top > 300.0 + (7.5 + 9.0 - 0.01) * 3.0, "clear of the crosshair by its reach plus the gap");
    }

    #[test]
    fn the_charge_bar_keeps_clear_of_a_crosshair_that_grew() {
        let (mut a, mut b) = (Vec::new(), Vec::new());
        let mut xh = Crosshair::default();
        draw_charge(&mut a, 3.0, (500.0, 300.0), &xh, 1.0);
        xh.event(Feedback::Hit);
        draw_charge(&mut b, 3.0, (500.0, 300.0), &xh, 1.0);
        assert!(b[0].pos[1] > a[0].pos[1]);
    }

    #[test]
    fn the_orbs_are_centred_dim_when_down_and_spent_ones_use_the_grey_art() {
        let mut v = Vec::new();
        draw_orbs(&mut v, 2.0, (400.0, 200.0), 2, 3, true);
        assert_eq!(v.len(), 18);
        let (lo, hi) = (v.iter().map(|p| p.pos[0]).fold(f32::MAX, f32::min), v.iter().map(|p| p.pos[0]).fold(f32::MIN, f32::max));
        assert!(((lo + hi) * 0.5 - 400.0).abs() < 1e-3, "centred on the crosshair");
        assert!((hi - lo - (3.0 * 9.0 + 2.0 * 3.0) * 2.0).abs() < 1e-3);
        let last = &v[12..];
        assert!(last[0].color[3] < v[0].color[3], "a spent orb is fainter");
        let mut d = Vec::new();
        draw_orbs(&mut d, 2.0, (400.0, 200.0), 2, 3, false);
        assert!(d[0].color[3] < v[0].color[3], "a lowered guard dims");
        assert!(d[0].color[..3] == [1.0; 3], "white: the orbs carry no hit colour");
        let mut none = Vec::new();
        draw_orbs(&mut none, 2.0, (400.0, 200.0), 0, 0, true);
        assert!(none.is_empty());
    }

    #[test]
    fn the_gauge_draws_two_brackets_symmetric_about_the_centre() {
        let (mut v, xh) = (Vec::new(), Crosshair::default());
        draw_gauge(&mut v, 3.0, (1000.0, 540.0), &xh, 35.0, (2000.0, 900.0), 70.0, 0.0);
        assert_eq!(v.len(), 12, "a ratio of 0 draws no bar, only the two brackets");
        let (l, r) = (&v[..6], &v[6..]);
        let mid = |q: &[HudVertex]| q.iter().map(|p| p.pos[0]).sum::<f32>() / 6.0;
        assert!(((mid(l) - 1000.0) + (mid(r) - 1000.0)).abs() < 1e-2, "mirror images");
        assert!(l[0].color[3] == BRACKET_ALPHA && r[0].color[3] == BRACKET_ALPHA);
    }

    #[test]
    fn health_above_fourteen_shows_nothing_and_below_four_the_vignette_is_full() {
        let mut fx = DamageFx::default();
        for _ in 0..60 {
            fx.frame(20.0, 0.05);
        }
        assert!(!fx.visible());
        for _ in 0..200 {
            fx.frame(4.0, 0.05);
        }
        assert!((fx.vignette - 1.0).abs() < 0.01);
        for _ in 0..200 {
            fx.frame(9.0, 0.05);
        }
        assert!(fx.vignette > 0.3 && fx.vignette < 0.7, "halfway through the range, eased");
        for _ in 0..200 {
            fx.frame(20.0, 0.05);
        }
        assert!(!fx.visible(), "it fades out again");
    }

    #[test]
    fn a_hit_sets_the_flash_by_its_damage_and_it_dies_away_and_a_heal_never_flashes() {
        let mut fx = DamageFx::default();
        fx.frame(20.0, 0.0); // the first frame only learns the health
        assert_eq!(fx.flash, 0.0);
        fx.frame(16.0, 0.0);
        assert!((fx.flash - 0.5).abs() < 1e-6, "four points are half the flash");
        fx.frame(16.0, 0.5);
        assert!(fx.flash < 0.05, "gone in half a second");
        let mut h = DamageFx::default();
        h.frame(10.0, 0.0);
        h.frame(14.0, 0.0);
        assert_eq!(h.flash, 0.0, "healing is not damage");
        let mut big = DamageFx::default();
        big.frame(20.0, 0.0);
        big.frame(5.0, 0.0);
        assert_eq!(big.flash, 1.0, "capped");
    }

    #[test]
    fn a_small_hit_does_not_rearm_a_flash_that_is_still_bright() {
        let mut fx = DamageFx::default();
        fx.frame(20.0, 0.0);
        fx.frame(14.0, 0.0);
        let before = fx.flash;
        assert_eq!(before, 0.75);
        fx.frame(13.0, 0.0);
        assert_eq!(fx.flash, before, "one more point: weaker than what is showing, and not yet under the rearm line");
        fx.frame(1.0, 0.0);
        assert!(fx.flash > before, "a bigger one overrides");
    }

    #[test]
    fn the_vignette_is_dark_at_the_edges_and_clear_in_the_middle() {
        assert!(vignette_alpha(0.0, 0.0) < 0.05);
        assert!((vignette_alpha(1.0, 1.0) - 220.0 / 255.0).abs() < 1e-5);
        let edge = vignette_alpha(1.0, 0.0);
        assert!((edge - 175.0 / 255.0).abs() < 0.06, "within the picture's range at an edge: {edge}");
        let mut v = Vec::new();
        vignette(&mut v, (800.0, 400.0), 1.0);
        assert_eq!(v.len(), VIG_COLS * VIG_ROWS * 6);
        assert!(v.iter().all(|p| p.uv == crate::render::hud::NO_UV && p.color[3] <= 220.0 / 255.0 + 1e-5));
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for p in &v {
            lo = lo.min(p.color[3]);
            hi = hi.max(p.color[3]);
        }
        assert!(lo < 0.06 && hi > 0.8);
    }

    #[test]
    fn draw_is_empty_until_something_shows_and_the_flash_is_one_quad_over_the_screen() {
        let mut fx = DamageFx::default();
        let mut v = Vec::new();
        fx.draw(&mut v, (800.0, 400.0));
        assert!(v.is_empty());
        fx.frame(20.0, 0.0);
        fx.frame(12.0, 0.0);
        fx.draw(&mut v, (800.0, 400.0));
        assert_eq!(v.len(), 6, "health 12 is above the vignette's start (14): the flash alone");
        assert!(v.iter().all(|p| p.pos[0] >= 0.0 && p.pos[0] <= 800.0 && p.pos[1] >= 0.0 && p.pos[1] <= 400.0));
    }
}
