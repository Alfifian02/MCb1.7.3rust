//! The "World-Gen Settings" screen: the switch and the knobs of the Natural Temperature climate (`world::gen::natural`), one scrolling
//! list of rows in GUI units like `options.rs`. NOT b1.7.3's (it has no such screen). UNVERIFIED: not compiled, not run.
//!
//! Editable on the Create World screen (`menu::Scr::GenSettings`, from the button at its top right); read only from the game menu
//! (the settings are part of the world and cannot change after it was made: the terrain already built would not match).
//! Touch: a slider is dragged, a toggle and the mode button are tapped, the `A` button of a zone or a transition point switches it
//! to the mod's 8.0 (random / automatic), the list scrolls by dragging anywhere else.
//! Only the rows the mode reads are listed (the mod's own screen lists them all and tags each with its modes).

use crate::input::touch_ui::ScreenEv;
use crate::menu::{dirt, grad, rgb, scaled_gs, tw};
use crate::options::{background, button, button_c};
use crate::render::hud::{HudPipeline, HudVertex};
use crate::ui;
use crate::world::gen::natural::{NaturalSettings, AUTO};

const ROW_H: f32 = 20.0;
const ROW_PITCH: f32 = 22.0;
const HEAD_H: f32 = 16.0;
/// The control (slider or button) of a row, and the `A` button beside a zone's.
const CTL: f32 = 112.0;
const AUTO_W: f32 = 18.0;
const TOP: f32 = 26.0;

const MODES: [&str; 13] = [
    "Default", "Simplified", "Linear", "Circular: custom", "Circular", "Banded: custom", "Banded", "Circular6: custom", "Circular6",
    "Square6: custom", "Square6", "Discrete: custom", "Discrete",
];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Id {
    On,
    Mode,
    Eq,
    Off,
    WMag,
    WFreq,
    WAmp,
    Loop,
    Infl,
    Dist,
    Modi,
    Mit,
    TZone(usize),
    TTrans(usize),
    TVar(usize),
    HBands,
    HBw,
    HOff,
    HZone(usize),
    HTrans(usize),
    HVar(usize),
    Extra,
    Cov(usize),
}

enum Row {
    Head(&'static str),
    Item(Id, String),
}

impl Id {
    fn is_toggle(self) -> bool {
        matches!(self, Id::On | Id::Loop | Id::HBands | Id::Extra)
    }

    /// (min, max, step, has the `A` button) of a slider.
    fn spec(self) -> (f64, f64, f64, bool) {
        match self {
            Id::Eq => (100.0, 40000.0, 100.0, false),
            Id::Off => (-40000.0, 40000.0, 100.0, false),
            Id::WMag => (0.0, 10.0, 0.1, false),
            Id::WFreq | Id::WAmp => (0.0, 5.0, 0.1, false),
            Id::Dist => (0.0, 5.0, 1.0, false),
            Id::Infl | Id::Mit => (0.0, 100.0, 1.0, false),
            Id::Modi => (-100.0, 100.0, 1.0, false),
            Id::TZone(_) | Id::HZone(_) | Id::TTrans(_) | Id::HTrans(_) => (-1.0, 1.0, 0.05, true),
            Id::TVar(_) | Id::HVar(_) => (0.0, 1.0, 0.05, false),
            Id::HBw => (100.0, 10000.0, 50.0, false),
            Id::HOff => (-5000.0, 5000.0, 50.0, false),
            Id::Cov(_) => (0.0, 30.0, 0.5, false),
            _ => (0.0, 1.0, 1.0, false),
        }
    }

    fn get(self, s: &NaturalSettings) -> f64 {
        match self {
            Id::On => f64::from(u8::from(s.enabled)),
            Id::Mode => f64::from(s.mode),
            Id::Eq => s.equatorial_distance,
            Id::Off => s.equator_offset,
            Id::WMag => s.wave_magnification,
            Id::WFreq => s.wave_frequency,
            Id::WAmp => s.wave_amplitude,
            Id::Loop => f64::from(u8::from(s.looping_world)),
            Id::Infl => s.influence,
            Id::Dist => f64::from(s.distortion),
            Id::Modi => s.modifier,
            Id::Mit => s.mitigation,
            Id::TZone(i) => s.temp_zone[i],
            Id::TTrans(i) => s.temp_transition[i],
            Id::TVar(i) => s.temp_variability[i],
            Id::HBands => f64::from(u8::from(s.humidity_bands)),
            Id::HBw => s.humidity_bands_width,
            Id::HOff => s.humidity_offset,
            Id::HZone(i) => s.humidity_zone[i],
            Id::HTrans(i) => s.humidity_transition[i],
            Id::HVar(i) => s.humidity_variability[i],
            Id::Extra => f64::from(u8::from(s.extra_bands)),
            Id::Cov(i) => s.coverage[i],
        }
    }

    fn set(self, s: &mut NaturalSettings, v: f64) {
        match self {
            Id::Eq => s.equatorial_distance = v,
            Id::Off => s.equator_offset = v,
            Id::WMag => s.wave_magnification = v,
            Id::WFreq => s.wave_frequency = v,
            Id::WAmp => s.wave_amplitude = v,
            Id::Infl => s.influence = v,
            Id::Dist => s.distortion = v as i32,
            Id::Modi => s.modifier = v,
            Id::Mit => s.mitigation = v,
            Id::TZone(i) => s.temp_zone[i] = v,
            Id::TTrans(i) => s.temp_transition[i] = v,
            Id::TVar(i) => s.temp_variability[i] = v,
            Id::HBw => s.humidity_bands_width = v,
            Id::HOff => s.humidity_offset = v,
            Id::HZone(i) => s.humidity_zone[i] = v,
            Id::HTrans(i) => s.humidity_transition[i] = v,
            Id::HVar(i) => s.humidity_variability[i] = v,
            Id::Cov(i) => s.coverage[i] = v,
            Id::On | Id::Mode | Id::Loop | Id::HBands | Id::Extra => {}
        }
    }

    /// A tap on a toggle or on the mode button.
    fn tap(self, s: &mut NaturalSettings) {
        match self {
            Id::On => s.enabled = !s.enabled,
            Id::Loop => s.looping_world = !s.looping_world,
            Id::HBands => s.humidity_bands = !s.humidity_bands,
            Id::Extra => s.extra_bands = !s.extra_bands,
            Id::Mode => s.mode = (s.mode + 1) % 13,
            _ => {}
        }
    }

    fn is_transition(self) -> bool {
        matches!(self, Id::TTrans(_) | Id::HTrans(_))
    }

    /// The text on the control.
    fn text(self, s: &NaturalSettings) -> String {
        let v = self.get(s);
        match self {
            Id::Mode => MODES[s.mode.clamp(0, 12) as usize].to_string(),
            _ if self.is_toggle() => (if v != 0.0 { "ON" } else { "OFF" }).to_string(),
            _ if v == AUTO => (if self.is_transition() { "Auto" } else { "Random" }).to_string(),
            _ => {
                let step = self.spec().2;
                if step >= 1.0 { format!("{v:.0}") } else if step >= 0.1 { format!("{v:.1}") } else { format!("{v:.2}") }
            }
        }
    }
}

/// How many zones, transition points and coverage rows a mode reads (the custom modes read their own zones).
fn zones(m: i32) -> usize {
    match m {
        3 => 4,
        5 | 7 | 9 => 6,
        11 => 12,
        _ => 0,
    }
}

fn trans(m: i32) -> usize {
    match m {
        3 => 3,
        5 | 7 | 9 => 5,
        _ => 0,
    }
}

fn covers(m: i32) -> usize {
    match m {
        3 => 4,
        5 | 6 | 7 | 9 => 6,
        11 | 12 => 12,
        _ => 0,
    }
}

fn section(r: &mut Vec<Row>, head: &'static str, items: Vec<(Id, String)>) {
    if !items.is_empty() {
        r.push(Row::Head(head));
        r.extend(items.into_iter().map(|(i, l)| Row::Item(i, l)));
    }
}

/// The rows to show for these settings: what the mode reads.
fn rows(s: &NaturalSettings) -> Vec<Row> {
    let m = s.mode;
    let it = |i: Id, l: &str| (i, l.to_string());
    let mut r = Vec::new();
    let mut general = vec![it(Id::On, "Natural Temperature"), it(Id::Mode, "Generation Mode"), it(Id::Eq, "Equatorial Distance"), it(Id::Off, "Equator Offset")];
    general.extend([it(Id::WMag, "Wave Magnification"), it(Id::WFreq, "Wave Frequency"), it(Id::WAmp, "Wave Amplitude"), it(Id::Dist, "Distortion")]);
    if !matches!(m, 2 | 3 | 4 | 7 | 8 | 9 | 10) {
        general.push(it(Id::Loop, "Looping World"));
    }
    general.push(it(Id::Infl, "Influence %"));
    section(&mut r, "General", general);
    let mut t = Vec::new();
    if matches!(m, 0 | 1 | 2 | 5 | 6) {
        t.extend([it(Id::Modi, "Temp Modifier %"), it(Id::Mit, "Temp Mitigation %")]);
    }
    t.extend((0..zones(m)).map(|i| (Id::TZone(i), format!("Temp Zone {}", i + 1))));
    t.extend((0..trans(m)).map(|i| (Id::TTrans(i), format!("Temp Transition {}", i + 1))));
    if m == 11 {
        t.extend((0..12).map(|i| (Id::TVar(i), format!("Temp Variability {}", i + 1))));
    }
    section(&mut r, "Temperature", t);
    section(&mut r, "Zone Coverage", (0..covers(m)).map(|i| (Id::Cov(i), format!("Coverage Zone {} %", i + 1))).collect());
    let mut h = Vec::new();
    if m == 0 {
        h.push(it(Id::Extra, "Extra Bands"));
    } else {
        h.extend([it(Id::HBands, "Humidity Bands"), it(Id::HBw, "Humidity Bands Width"), it(Id::HOff, "Humidity Offset")]);
    }
    h.extend((0..zones(m)).map(|i| (Id::HZone(i), format!("Humidity Zone {}", i + 1))));
    h.extend((0..trans(m)).map(|i| (Id::HTrans(i), format!("Humidity Transition {}", i + 1))));
    if m == 11 {
        h.extend((0..12).map(|i| (Id::HVar(i), format!("Humidity Variability {}", i + 1))));
    }
    section(&mut r, "Humidity", h);
    r
}

fn inside(r: [f32; 4], x: f32, y: f32) -> bool {
    x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]
}

/// Where everything is (GUI units, the list at scroll 0).
struct Lay {
    s: f32,
    gw: f32,
    gh: f32,
    /// The list's top and bottom (the view).
    top: f32,
    bottom: f32,
    done: [f32; 4],
    rows: Vec<(Row, [f32; 4])>,
    total: f32,
}

impl Lay {
    fn max_scroll(&self) -> f32 {
        (self.total - (self.bottom - self.top - 4.0)).max(0.0)
    }
}

/// The track of a row's slider and its `A` button, if it has one.
fn parts(r: [f32; 4], id: Id) -> ([f32; 4], Option<[f32; 4]>) {
    let x1 = r[0] + r[2];
    if id.spec().3 {
        ([x1 - AUTO_W - 2.0 - CTL, r[1], CTL, ROW_H], Some([x1 - AUTO_W, r[1], AUTO_W, ROW_H]))
    } else {
        ([x1 - CTL, r[1], CTL, ROW_H], None)
    }
}

struct Press {
    at: (f32, f32),
    from: f32,
    drag: bool,
    /// The slider the finger went down on (its row id and track).
    grab: Option<(Id, [f32; 4])>,
}

pub struct GenUi {
    pub s: NaturalSettings,
    edit: bool,
    scroll: f32,
    press: Option<Press>,
    finger: Option<(f32, f32)>,
}

impl GenUi {
    pub fn new(s: NaturalSettings, edit: bool) -> Self {
        Self { s, edit, scroll: 0.0, press: None, finger: None }
    }

    fn lay(&self, w: f32, h: f32, gs: u8) -> Lay {
        let (s, gw, gh) = scaled_gs(w, h, gs);
        let hw = (gw / 2.0).floor();
        let rw = (gw - 16.0).min(300.0);
        let x = (gw - rw) / 2.0;
        let (top, bottom) = (TOP, gh - 34.0);
        let mut y = top + 4.0;
        let rows = rows(&self.s)
            .into_iter()
            .map(|r| {
                let rect = match r {
                    Row::Head(_) => [x, y, rw, HEAD_H],
                    Row::Item(..) => [x, y, rw, ROW_H],
                };
                y += if matches!(r, Row::Head(_)) { HEAD_H + 2.0 } else { ROW_PITCH };
                (r, rect)
            })
            .collect();
        Lay { s, gw, gh, top, bottom, done: [hw - 100.0, gh - 28.0, 200.0, 20.0], rows, total: y - (top + 4.0) }
    }

    /// What the screen fills that the pad's button hints keep off (px).
    pub fn avoid(&self, w: f32, h: f32, gs: u8) -> Vec<[f32; 4]> {
        let l = self.lay(w, h, gs);
        [l.done, [0.0, l.top, l.gw, l.bottom - l.top]].into_iter().map(|r| r.map(|u| u * l.s)).collect()
    }

    /// A finger event in px. True when `Done` is tapped (the caller closes the screen).
    pub fn event(&mut self, ev: ScreenEv, w: f32, h: f32, gs: u8) -> bool {
        let l = self.lay(w, h, gs);
        match ev {
            ScreenEv::Down(x, y) => {
                let p = (x / l.s, y / l.s);
                self.finger = Some(p);
                let mut grab = None;
                if self.edit && p.1 >= l.top && p.1 < l.bottom {
                    for (r, rect) in &l.rows {
                        let Row::Item(id, _) = r else { continue };
                        let rect = [rect[0], rect[1] - self.scroll, rect[2], rect[3]];
                        let (track, _) = parts(rect, *id);
                        if !id.is_toggle() && *id != Id::Mode && inside(track, p.0, p.1) {
                            grab = Some((*id, track));
                        }
                    }
                }
                self.press = Some(Press { at: p, from: self.scroll, drag: false, grab });
                if let Some((id, track)) = grab {
                    self.slide(id, track, p.0);
                }
            }
            ScreenEv::Move(x, y) => {
                let p = (x / l.s, y / l.s);
                self.finger = Some(p);
                let grab = self.press.as_ref().and_then(|pr| pr.grab);
                if let Some((id, track)) = grab {
                    self.slide(id, track, p.0);
                } else if let Some(pr) = self.press.as_mut() {
                    pr.drag |= (p.1 - pr.at.1).abs() > 4.0;
                    if pr.drag {
                        self.scroll = (pr.from - (p.1 - pr.at.1)).clamp(0.0, l.max_scroll());
                    }
                }
            }
            ScreenEv::Up(x, y) => {
                self.finger = None;
                let Some(pr) = self.press.take() else { return false };
                if pr.grab.is_some() || pr.drag {
                    return false;
                }
                let p = (x / l.s, y / l.s);
                if inside(l.done, p.0, p.1) {
                    return true;
                }
                if !self.edit || p.1 < l.top || p.1 >= l.bottom {
                    return false;
                }
                for (r, rect) in &l.rows {
                    let Row::Item(id, _) = r else { continue };
                    let rect = [rect[0], rect[1] - self.scroll, rect[2], rect[3]];
                    if !inside(rect, p.0, p.1) {
                        continue;
                    }
                    let (_, auto) = parts(rect, *id);
                    if auto.is_some_and(|a| inside(a, p.0, p.1)) {
                        let v = if id.get(&self.s) == AUTO { 0.0 } else { AUTO };
                        id.set(&mut self.s, v);
                    } else if id.is_toggle() || *id == Id::Mode {
                        id.tap(&mut self.s);
                        self.scroll = self.scroll.min(self.lay(w, h, gs).max_scroll());
                    }
                    break;
                }
            }
        }
        false
    }

    /// The slider of `id` (its `track` already scrolled) set from a finger at GUI x `x`.
    fn slide(&mut self, id: Id, track: [f32; 4], x: f32) {
        let (min, max, step, _) = id.spec();
        let f = f64::from(((x - track[0] - 4.0) / (track[2] - 8.0)).clamp(0.0, 1.0));
        let v = (min + f * (max - min)).clamp(min, max);
        id.set(&mut self.s, (((v - min) / step).round() * step + min).clamp(min, max));
    }

    pub fn draw(&self, v: &mut Vec<HudVertex>, w: f32, h: f32, gs: u8, world: bool) {
        let l = self.lay(w, h, gs);
        let (s, gw, gh) = (l.s, l.gw, l.gh);
        background(v, s, gw, gh, world);
        let over = |r: [f32; 4]| self.finger.is_some_and(|(x, y)| inside(r, x, y));
        for (r, rect) in &l.rows {
            let rect = [rect[0], rect[1] - self.scroll, rect[2], rect[3]];
            if rect[1] + rect[3] < l.top || rect[1] > l.bottom {
                continue;
            }
            match r {
                Row::Head(t) => {
                    grad(v, s, rect, [0.0, 0.0, 0.0, 0.65], [0.0, 0.0, 0.0, 0.25]);
                    HudPipeline::push_quad(v, rect[0] * s, (rect[1] + rect[3] - 1.0) * s, rect[2] * s, s, rgb(0x808080));
                    HudPipeline::push_quad(v, (rect[0] + 3.0) * s, (rect[1] + 6.0) * s, 4.0 * s, 4.0 * s, rgb(0xFFAA00));
                    HudPipeline::push_text(v, (rect[0] + 12.0) * s, (rect[1] + 5.0) * s, s, t, rgb(0xFFFF55));
                }
                Row::Item(id, label) => {
                    HudPipeline::push_quad(v, rect[0] * s, rect[1] * s, rect[2] * s, rect[3] * s, [0.0, 0.0, 0.0, 0.25]);
                    let (track, auto) = parts(rect, *id);
                    let room = track[0] - rect[0] - 10.0;
                    HudPipeline::push_text(v, (rect[0] + 6.0) * s, (rect[1] + 6.0) * s, s, &ui::fit(label, room), rgb(if self.edit { 0xE0E0E0 } else { 0xA0A0A0 }));
                    let text = id.text(&self.s);
                    let val = id.get(&self.s);
                    let (lit, on) = (over(track) && self.edit, self.edit);
                    if id.is_toggle() || *id == Id::Mode {
                        button(v, s, track, &text, on, lit, None);
                    } else {
                        let (min, max, ..) = id.spec();
                        let knob = if val == AUTO { 0.5 } else { ((val - min) / (max - min)).clamp(0.0, 1.0) as f32 };
                        button_c(v, s, track, &text, if on { if val == AUTO { 0xFFFF55 } else { 0xE0E0E0 } } else { 0xA0A0A0 }, 0, Some(knob));
                    }
                    if let Some(a) = auto {
                        button_c(v, s, a, "A", if val == AUTO { 0xFFFF55 } else if on { 0xE0E0E0 } else { 0xA0A0A0 }, if on { if over(a) { 2 } else { 1 } } else { 0 }, None);
                    }
                }
            }
        }
        // The dirt bars hide what the rows spill over (`GuiSlot.overlayBackground`), with the shadows under them.
        let (clear, black) = ([0.0; 4], [0.0, 0.0, 0.0, 1.0]);
        dirt(v, s, [0.0, 0.0, gw, l.top], 0.0, 0x404040);
        dirt(v, s, [0.0, l.bottom, gw, gh - l.bottom], 0.0, 0x404040);
        grad(v, s, [0.0, l.top, gw, 4.0], black, clear);
        grad(v, s, [0.0, l.bottom - 4.0, gw, 4.0], clear, black);
        let title = if self.edit { "World-Gen Settings" } else { "World-Gen Settings (read only)" };
        HudPipeline::push_text(v, ((gw - tw(title)) / 2.0).floor() * s, 9.0 * s, s, title, rgb(0xFFFFFF));
        button(v, s, l.done, "Done", true, over(l.done), None);
        if l.max_scroll() > 0.0 {
            ui::scrollbar(v, s, gw - 6.0, l.top, l.bottom - l.top, l.total + 4.0, self.scroll);
        }
    }
}
