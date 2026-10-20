//! M14: the screens before a world is loaded, ported from b1.7.3: `GuiMainMenu` (dirt, `mclogo.png`, the yellow splash), `GuiSelectWorld`
//! (+ `GuiSlot`, `GuiWorldSlot`), `GuiCreateWorld`, `GuiYesNo` (delete) and the "Loading level" screen. Everything is laid out in GUI units
//! (`ScaledResolution`: the biggest whole scale that leaves 320 x 240) and drawn with the HUD pipeline.
//!
//! UNVERIFIED (b1.7.3 has no such thing): the world-type button of the create screen (`GuiCreateWorld` has none until 1.1) and the on-screen
//! keyboard (the sign editor's, `decor::layout`) standing in for the keyboard. Not done: Multiplayer, Mods and Texture Packs and
//! Rename (drawn greyed out like a disabled `GuiButton`; Options is `options.rs`), the splash's calendar days, the local time zone (dates are UTC).

use crate::input::touch_ui::ScreenEv;
use crate::options::{OptEv, OptionsUi, Settings};
use crate::render::hud::{sprites as sp, HudPipeline, HudVertex, NO_UV};
use crate::world::decor::{self, Key};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const SPLASHES: &str = include_str!("../assets/splashes.txt");
/// `GuiTextField`'s `maxStringLength` for the world name.
const MAX_NAME: usize = 32;
/// The seed the `world-cafebabe` save (before M14) was made with.
const LEGACY_SEED: i64 = 0xCAFEBABE;

/// A world on disk: `<root>/saves/<folder>/info` (name, seed, type: one per line) next to its `level`, chunk and decor files.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub folder: String,
    pub name: String,
    pub seed: i64,
    pub tectonic: bool,
    /// Seconds since the epoch of the last save (`SaveFormatComparator.lastTimePlayed`).
    pub modified: u64,
    pub bytes: u64,
}

pub fn saves(root: &Path) -> PathBuf {
    root.join("saves")
}

impl Info {
    pub fn dir(&self, root: &Path) -> PathBuf {
        saves(root).join(&self.folder)
    }

    pub fn write(&self, root: &Path) {
        let d = self.dir(root);
        let _ = std::fs::create_dir_all(&d);
        let text = format!("{}\n{}\n{}\n", self.name, self.seed, if self.tectonic { "tectonic" } else { "vanilla" });
        if let Err(e) = std::fs::write(d.join("info"), text) {
            log::warn!("world info {}: {e}", self.folder);
        }
    }

    fn read(root: &Path, folder: &str) -> Option<Self> {
        let d = saves(root).join(folder);
        let text = std::fs::read_to_string(d.join("info")).ok()?;
        let mut l = text.lines();
        let (name, seed, ty) = (l.next()?, l.next()?.trim().parse().ok()?, l.next()?);
        let secs = |p: PathBuf| std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |t| t.as_secs());
        let bytes = std::fs::read_dir(&d).map_or(0, |r| r.flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum());
        let modified = secs(d.join("level")).max(secs(d.join("info")));
        Some(Self { folder: folder.into(), name: name.into(), seed, tectonic: ty.trim() == "tectonic", modified, bytes })
    }
}

/// Every world, the last played first. The folder of the old single-seed save moves in (with an `info`) the first time.
pub fn list(root: &Path) -> Vec<Info> {
    // ponytail: only the vanilla folder moves in; `world-cafebabe-rtg` (the Realistic terrain, gone with the Tectonic port) stays where it is.
    let old = "world-cafebabe";
    let from = root.join(old);
    if from.is_dir() && !saves(root).join(old).exists() {
        let _ = std::fs::create_dir_all(saves(root));
        if std::fs::rename(&from, saves(root).join(old)).is_ok() {
            Info { folder: old.into(), name: "Old world".into(), seed: LEGACY_SEED, tectonic: false, modified: 0, bytes: 0 }.write(root);
        }
    }
    let mut v: Vec<Info> = std::fs::read_dir(saves(root)).map_or_else(|_| Vec::new(), |r| r.flatten().filter_map(|e| Info::read(root, e.file_name().to_str()?)).collect());
    v.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.folder.cmp(&b.folder)));
    v
}

/// `GuiCreateWorld.func_22129_j`: the name as a folder name (the characters a file name cannot have become `_`), `-` added until unused.
fn folder_for(root: &Path, name: &str) -> String {
    let mut f: String = name.trim().chars().map(|c| if "/\\:*?\"<>|`".contains(c) { '_' } else { c }).collect();
    if f.is_empty() {
        f = "World".into();
    }
    while saves(root).join(&f).exists() {
        f.push('-');
    }
    f
}

/// `GuiCreateWorld.actionPerformed`: blank or 0 is a random seed, a number is the seed, any other text its `String.hashCode`.
fn parse_seed(s: &str, random: i64) -> i64 {
    let s = s.trim();
    match s.parse::<i64>() {
        _ if s.is_empty() => random,
        Ok(0) => random,
        Ok(n) => n,
        Err(_) => s.chars().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32)) as i64,
    }
}

/// `SimpleDateFormat`'s default, "M/d/yy h:mm a", in UTC.
fn date(secs: u64) -> String {
    let (days, tod) = ((secs / 86400) as i64, secs % 86400);
    let z = days + 719468;
    let (era, doe) = (z.div_euclid(146097), z.rem_euclid(146097));
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    let h = tod / 3600;
    format!("{m}/{d}/{:02} {}:{:02} {}", y % 100, if h % 12 == 0 { 12 } else { h % 12 }, tod % 3600 / 60, if h < 12 { "AM" } else { "PM" })
}

/// The width of `t` in font pixels (GUI units at scale 1).
pub(crate) fn tw(t: &str) -> f32 {
    HudPipeline::text_width(t) as f32
}

fn now_nanos() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos())
}

pub(crate) fn rgb(c: u32) -> [f32; 4] {
    [(c >> 16 & 255) as f32 / 255.0, (c >> 8 & 255) as f32 / 255.0, (c & 255) as f32 / 255.0, 1.0]
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scr {
    Main,
    Select,
    Create,
    Delete,
    /// `GuiOptions`, from the title screen's "Options...".
    Options,
}

/// A `GuiButton`: id, rectangle in GUI units, label, enabled.
struct Btn(u8, [f32; 4], String, bool);

pub struct Menu {
    root: PathBuf,
    pub scr: Scr,
    worlds: Vec<Info>,
    sel: Option<usize>,
    scroll: f32,
    splash: String,
    // create screen
    name: String,
    seed: String,
    folder: String,
    tectonic: bool,
    /// The text field being typed in (0 name, 1 seed): the keyboard is up.
    typing: Option<u8>,
    shift: bool,
    // finger
    press: Option<(f32, f32)>,
    press_scroll: f32,
    dragging: bool,
    last_click: Option<(usize, Instant)>,
    born: Instant,
    /// Set by the Play / Create buttons; `shown_loading` says the "Loading level" frame went out, so the app may now load it.
    pub loading: Option<Info>,
    pub shown_loading: bool,
    pub quit: bool,
    opt: OptionsUi,
    /// `GameSettings.guiScale` of the last event / frame.
    gs: u8,
}

/// `ScaledResolution` with `guiScale` `gs` (0 auto, else at most that scale): (scale, width, height in GUI units).
pub(crate) fn scaled_gs(w: f32, h: f32, gs: u8) -> (f32, f32, f32) {
    let max = if gs == 0 { 1000.0 } else { gs as f32 };
    let mut s = 1.0;
    while s < max && (w / (s + 1.0)).floor() >= 320.0 && (h / (s + 1.0)).floor() >= 240.0 {
        s += 1.0;
    }
    (s, (w / s).ceil(), (h / s).ceil())
}

#[cfg(test)]
fn scaled(w: f32, h: f32) -> (f32, f32, f32) {
    scaled_gs(w, h, 0)
}

impl Menu {
    pub fn new(root: &Path) -> Self {
        let lines: Vec<&str> = SPLASHES.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        let splash = lines.get(now_nanos() as usize / 7 % lines.len().max(1)).copied().unwrap_or("missingno").to_string();
        Self {
            root: root.to_path_buf(),
            scr: Scr::Main,
            worlds: Vec::new(),
            sel: None,
            scroll: 0.0,
            splash,
            name: String::new(),
            seed: String::new(),
            folder: String::new(),
            tectonic: false,
            typing: None,
            shift: false,
            press: None,
            press_scroll: 0.0,
            dragging: false,
            last_click: None,
            born: Instant::now(),
            loading: None,
            shown_loading: false,
            quit: false,
            opt: OptionsUi::new(),
            gs: 0,
        }
    }

    // ---- layout ----

    fn buttons(&self, gw: f32, gh: f32) -> Vec<Btn> {
        let hw = (gw / 2.0).floor();
        let b = |id, x, y, w, l: &str, on| Btn(id, [x, y, w, 20.0], l.to_string(), on);
        match self.scr {
            Scr::Main => {
                let y = (gh / 4.0).floor() + 48.0;
                vec![
                    b(1, hw - 100.0, y, 200.0, "Singleplayer", true),
                    b(2, hw - 100.0, y + 24.0, 200.0, "Multiplayer", false),
                    b(3, hw - 100.0, y + 48.0, 200.0, "Mods and Texture Packs", false),
                    b(0, hw - 100.0, y + 84.0, 98.0, "Options...", true),
                    b(4, hw + 2.0, y + 84.0, 98.0, "Quit Game", true),
                ]
            }
            Scr::Select => vec![
                b(1, hw - 154.0, gh - 52.0, 150.0, "Play Selected World", self.sel.is_some()),
                b(6, hw - 154.0, gh - 28.0, 70.0, "Rename", false),
                b(2, hw - 74.0, gh - 28.0, 70.0, "Delete", self.sel.is_some()),
                b(3, hw + 4.0, gh - 52.0, 150.0, "Create New World", true),
                b(0, hw + 4.0, gh - 28.0, 150.0, "Cancel", true),
            ],
            Scr::Create => {
                let y = (gh / 4.0).floor() + 108.0;
                vec![
                    b(2, hw - 100.0, y - 22.0, 200.0, if self.tectonic { "World Type: Tectonic" } else { "World Type: Vanilla" }, true),
                    b(0, hw - 100.0, y, 200.0, "Create New World", !self.name.is_empty()),
                    b(1, hw - 100.0, y + 24.0, 200.0, "Cancel", true),
                ]
            }
            Scr::Delete => {
                let y = (gh / 6.0).floor() + 96.0;
                vec![b(0, hw - 155.0, y, 150.0, "Delete", true), b(1, hw + 5.0, y, 150.0, "Cancel", true)]
            }
            Scr::Options => Vec::new(),
        }
    }

    /// `GuiSlot`'s box: top and bottom y (the list of `GuiSelectWorld`: 32 .. height - 64), slot height 36.
    fn list_box(gh: f32) -> (f32, f32) {
        (32.0, gh - 64.0)
    }

    fn max_scroll(&self, gh: f32) -> f32 {
        let (top, bottom) = Self::list_box(gh);
        (self.worlds.len() as f32 * 36.0 - (bottom - top - 4.0)).max(0.0)
    }

    /// The two text fields of the create screen (GUI units), name then seed.
    fn fields(gw: f32) -> [[f32; 4]; 2] {
        let hw = (gw / 2.0).floor();
        [[hw - 100.0, 60.0, 200.0, 20.0], [hw - 100.0, 116.0, 200.0, 20.0]]
    }

    /// The Controller tab waits for a pad button (only on the options screen).
    pub fn pad_waiting(&self) -> bool {
        self.scr == Scr::Options && self.opt.pad_waiting()
    }

    pub fn pad_button(&mut self, b: crate::input::gamepad::Btn, st: &mut Settings) {
        self.opt.pad_button(b, st);
    }

    // ---- input ----

    pub fn event(&mut self, ev: ScreenEv, w: f32, h: f32, st: &mut Settings) {
        self.gs = st.gui_scale;
        if self.scr == Scr::Options {
            if self.opt.event(ev, w, h, st) == Some(OptEv::Close) {
                self.scr = Scr::Main;
            }
            return;
        }
        let (s, _, gh) = scaled_gs(w, h, self.gs);
        match ev {
            ScreenEv::Down(x, y) => {
                self.press = Some((x / s, y / s));
                (self.press_scroll, self.dragging) = (self.scroll, false);
            }
            ScreenEv::Move(_, y) => {
                if let Some((_, py)) = self.press {
                    if (y / s - py).abs() > 4.0 {
                        self.dragging = true;
                    }
                    if self.dragging && self.scr == Scr::Select {
                        self.scroll = (self.press_scroll - (y / s - py)).clamp(0.0, self.max_scroll(gh));
                    }
                }
            }
            ScreenEv::Up(x, y) => {
                let dragged = std::mem::take(&mut self.dragging);
                if self.press.take().is_some() && !dragged {
                    self.click(x, y, w, h);
                }
            }
        }
    }

    /// A tap at (x, y) px.
    fn click(&mut self, x: f32, y: f32, w: f32, h: f32) {
        if let Some(t) = self.typing {
            let field = if t == 0 { &mut self.name } else { &mut self.seed };
            match decor::key_at(w, h, self.shift, x, y) {
                Some(Key::Ch(c)) if field.len() < MAX_NAME => field.push(c as char),
                Some(Key::Space) if field.len() < MAX_NAME => field.push(' '),
                Some(Key::Back) => drop(field.pop()),
                Some(Key::Shift) => self.shift = !self.shift,
                Some(Key::Enter | Key::Done) => self.typing = None,
                _ => {}
            }
            self.folder = folder_for(&self.root, &self.name);
            return;
        }
        let (s, gw, gh) = scaled_gs(w, h, self.gs);
        let (gx, gy) = (x / s, y / s);
        let hit = |r: [f32; 4]| gx >= r[0] && gx < r[0] + r[2] && gy >= r[1] && gy < r[1] + r[3];
        if let Some(Btn(id, _, _, _)) = self.buttons(gw, gh).into_iter().find(|b| b.3 && hit(b.1)) {
            self.pressed(id);
            return;
        }
        match self.scr {
            Scr::Select => {
                let (top, bottom) = Self::list_box(gh);
                let hw = (gw / 2.0).floor();
                if gy >= top && gy < bottom && (gx - hw).abs() < 110.0 {
                    let i = ((gy - top + self.scroll - 4.0) / 36.0).floor();
                    if i >= 0.0 && (i as usize) < self.worlds.len() {
                        let i = i as usize;
                        let twice = self.last_click.is_some_and(|(j, t)| j == i && t.elapsed().as_millis() < 500);
                        (self.sel, self.last_click) = (Some(i), Some((i, Instant::now())));
                        if twice {
                            self.pressed(1); // `GuiSlot`: a double click is "Play Selected World"
                        }
                    }
                }
            }
            Scr::Create => {
                if let Some(i) = Self::fields(gw).iter().position(|&r| hit(r)) {
                    self.typing = Some(i as u8);
                }
            }
            _ => {}
        }
    }

    /// `actionPerformed` of the screen's button `id`.
    fn pressed(&mut self, id: u8) {
        match (self.scr, id) {
            (Scr::Main, 0) => (self.opt, self.scr) = (OptionsUi::new(), Scr::Options),
            (Scr::Main, 1) => self.open_list(),
            (Scr::Main, 4) => self.quit = true,
            (Scr::Select, 0) => self.scr = Scr::Main,
            (Scr::Select, 1) => self.loading = self.sel.and_then(|i| self.worlds.get(i)).cloned(),
            (Scr::Select, 2) => self.scr = Scr::Delete,
            (Scr::Select, 3) => {
                (self.name, self.seed, self.tectonic, self.typing, self.scr) = (String::from("New World"), String::new(), false, None, Scr::Create);
                self.folder = folder_for(&self.root, &self.name);
            }
            (Scr::Create, 0) => {
                let info = Info { folder: folder_for(&self.root, &self.name), name: self.name.clone(), seed: parse_seed(&self.seed, now_nanos() as i64), tectonic: self.tectonic, modified: 0, bytes: 0 };
                info.write(&self.root);
                self.loading = Some(info);
            }
            (Scr::Create, 1) => self.scr = Scr::Select,
            (Scr::Create, 2) => self.tectonic = !self.tectonic,
            (Scr::Delete, 0) => {
                if let Some(w) = self.sel.and_then(|i| self.worlds.get(i)) {
                    if let Err(e) = std::fs::remove_dir_all(w.dir(&self.root)) {
                        log::warn!("delete {}: {e}", w.folder);
                    }
                }
                self.open_list();
            }
            (Scr::Delete, 1) => self.scr = Scr::Select,
            _ => {}
        }
    }

    fn open_list(&mut self) {
        (self.worlds, self.sel, self.scroll, self.scr) = (list(&self.root), None, 0.0, Scr::Select);
    }

    /// The title screen itself shows the panorama (`render_menu` draws it under the HUD); every other screen keeps its dirt.
    pub fn panorama(&self) -> bool {
        self.scr == Scr::Main && !self.shown_loading
    }

    // ---- drawing ----

    /// The whole screen into `v` for a `w` x `h` px surface.
    pub fn draw(&mut self, v: &mut Vec<HudVertex>, w: f32, h: f32, st: &Settings) {
        self.gs = st.gui_scale;
        let (s, gw, gh) = scaled_gs(w, h, self.gs);
        let hw = (gw / 2.0).floor();
        self.shown_loading = self.loading.is_some();
        let ms = self.born.elapsed().as_millis() as f32;
        let txt = |v: &mut Vec<HudVertex>, x: f32, y: f32, t: &str, c: u32, center: bool| {
            let x = if center { x - tw(t) / 2.0 } else { x };
            HudPipeline::push_text(v, x * s, y * s, s, t, rgb(c));
        };
        if !self.panorama() {
            dirt(v, s, [0.0, 0.0, gw, gh], 0.0, 0x404040);
        }
        if self.loading.is_some() {
            txt(v, hw, gh / 2.0 - 20.0, "Loading level", 0xFFFFFF, true);
            txt(v, hw, gh / 2.0 + 4.0, "Building terrain", 0xFFFFFF, true);
            return;
        }
        if self.scr == Scr::Options {
            self.opt.draw(v, w, h, st, false);
            return;
        }
        if let Some(t) = self.typing {
            self.draw_keyboard(v, w, h, t);
            return;
        }
        match self.scr {
            Scr::Main => {
                let x = hw - 137.0; // `GuiMainMenu`: width / 2 - 137, y 30
                HudPipeline::push_gui(v, x * s, 30.0 * s, 155.0 * s, 44.0 * s, sp::LOGO[0]);
                HudPipeline::push_gui(v, (x + 155.0) * s, 30.0 * s, 155.0 * s, 44.0 * s, sp::LOGO[1]);
                // The splash: -20 degrees, 1.8 pulsing by 0.1 once a second, fitted to the width of the text.
                let sc = (1.8 - ((ms % 1000.0) / 1000.0 * std::f32::consts::TAU).sin().abs() * 0.1) * 100.0 / (tw(&self.splash) + 32.0);
                let n = v.len();
                let k = s * sc;
                HudPipeline::push_text(v, -tw(&self.splash) * k / 2.0, -8.0 * k, k, &self.splash, rgb(0xFFFF00));
                let (sn, cs) = (-20f32.to_radians()).sin_cos();
                let (ox, oy) = ((hw + 90.0) * s, 70.0 * s);
                for p in &mut v[n..] {
                    let [px, py] = p.pos;
                    p.pos = [ox + px * cs - py * sn, oy + px * sn + py * cs];
                }
                txt(v, 2.0, 2.0, "Minecraft Beta 1.7.3", 0xFFFFFF, false); // was 0x505050, grey on dirt: unreadable over the panorama
                let c = "Copyright Mojang AB. Do not distribute.";
                txt(v, gw - tw(c) - 2.0, gh - 10.0, c, 0xFFFFFF, false);
            }
            Scr::Select => self.draw_list(v, s, gw, gh),
            Scr::Create => {
                txt(v, hw, (gh / 4.0).floor() - 40.0, "Create New World", 0xFFFFFF, true);
                txt(v, hw - 100.0, 47.0, "World Name", 0xA0A0A0, false);
                txt(v, hw - 100.0, 85.0, &format!("Will be saved in: {}", self.folder), 0xA0A0A0, false);
                txt(v, hw - 100.0, 104.0, "Seed for the World Generator", 0xA0A0A0, false);
                txt(v, hw - 100.0, 140.0, "Leave blank for a random seed", 0xA0A0A0, false);
                for (r, t) in Self::fields(gw).iter().zip([&self.name, &self.seed]) {
                    text_box(v, s, *r, t, false);
                }
            }
            Scr::Options => {}
            Scr::Delete => {
                let name = self.sel.and_then(|i| self.worlds.get(i)).map_or("", |w| w.name.as_str());
                txt(v, hw, 70.0, "Are you sure you want to delete this world?", 0xFFFFFF, true);
                txt(v, hw, 90.0, &format!("'{name}' will be lost forever! (A long time!)"), 0xFFFFFF, true);
            }
        }
        let held = self.press.filter(|_| !self.dragging);
        for Btn(_, r, label, on) in self.buttons(gw, gh) {
            let over = held.is_some_and(|(x, y)| x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]);
            HudPipeline::push_button(v, r[0] * s, r[1] * s, r[2] * s, r[3] * s, if !on { 0 } else if over { 2 } else { 1 });
            let c = if !on { 0xA0A0A0 } else if over { 0xFFFFA0 } else { 0xE0E0E0 };
            txt(v, r[0] + r[2] / 2.0, r[1] + (r[3] - 8.0) / 2.0, &label, c, true);
        }
    }

    /// `GuiSlot.drawScreen` for `GuiSelectWorld`.
    fn draw_list(&self, v: &mut Vec<HudVertex>, s: f32, gw: f32, gh: f32) {
        let hw = (gw / 2.0).floor();
        let (top, bottom) = Self::list_box(gh);
        dirt(v, s, [0.0, top, gw, bottom], self.scroll, 0x202020);
        for (i, wd) in self.worlds.iter().enumerate() {
            let y = top + 4.0 - self.scroll + i as f32 * 36.0;
            if y + 36.0 < top || y > bottom {
                continue;
            }
            if self.sel == Some(i) {
                rect(v, s, [hw - 110.0, y - 2.0, 220.0, 36.0], 0x808080);
                rect(v, s, [hw - 109.0, y - 1.0, 218.0, 34.0], 0x000000);
            }
            let kind = if wd.tectonic { "Tectonic" } else { "Vanilla" };
            let line = format!("{} ({}, {:?} MB) {kind}", wd.folder, date(wd.modified), (wd.bytes / 1024 * 100 / 1024) as f32 / 100.0);
            HudPipeline::push_text(v, (hw - 108.0) * s, (y + 1.0) * s, s, &wd.name, rgb(0xFFFFFF));
            HudPipeline::push_text(v, (hw - 108.0) * s, (y + 12.0) * s, s, &line, rgb(0x808080));
        }
        // The dirt over the list's ends, and their 4 px fade into the list.
        dirt(v, s, [0.0, 0.0, gw, top], 0.0, 0x404040);
        dirt(v, s, [0.0, bottom, gw, gh], 0.0, 0x404040);
        let (clear, black) = ([0.0; 4], [0.0, 0.0, 0.0, 1.0]);
        grad(v, s, [0.0, top, gw, 4.0], black, clear);
        grad(v, s, [0.0, bottom - 4.0, gw, 4.0], clear, black);
        let max = self.max_scroll(gh);
        if max > 0.0 {
            let (x, span) = (hw + 124.0, bottom - top);
            let th = ((span * span / (self.worlds.len() as f32 * 36.0)).clamp(32.0, span - 8.0)).floor();
            let ty = (self.scroll * (span - th) / max + top).max(top);
            rect(v, s, [x, top, 6.0, span], 0x000000);
            rect(v, s, [x, ty, 6.0, th], 0x808080);
            rect(v, s, [x, ty, 5.0, th - 1.0], 0xC0C0C0);
        }
        HudPipeline::push_text(v, (hw - tw("Select World") / 2.0) * s, 20.0 * s, s, "Select World", rgb(0xFFFFFF));
    }

    /// The text field being typed in, over the sign editor's keyboard (`decor::layout`, which scales by `h / 270`).
    fn draw_keyboard(&self, v: &mut Vec<HudVertex>, w: f32, h: f32, field: u8) {
        let (u, keys) = decor::layout(w, h, self.shift);
        let gw = w / u;
        let t = if field == 0 { &self.name } else { &self.seed };
        let title = if field == 0 { "World Name" } else { "Seed for the World Generator" };
        HudPipeline::push_text(v, (gw - tw(title)) / 2.0 * u, 14.0 * u, u, title, rgb(0xA0A0A0));
        text_box(v, u, [(gw - 200.0) / 2.0, 40.0, 200.0, 20.0], t, true);
        for (k, r) in keys.into_iter().filter(|(k, _)| !matches!(k, Key::Line(_))) {
            HudPipeline::push_button(v, r[0], r[1], r[2], r[3], if k == Key::Shift && self.shift { 2 } else { 1 });
            let l = if k == Key::Space { "".to_string() } else { decor::label(k) };
            HudPipeline::push_text(v, r[0] + (r[2] - tw(&l) * u) / 2.0, r[1] + (r[3] - 8.0 * u) / 2.0, u, &l, rgb(0xE0E0E0));
        }
    }
}

/// `drawBackground`: `background.png` over `r` (GUI units), one texture per 32 x 32, tinted `tint`, shifted up by `scroll`. Cut at `r`'s edges.
pub(crate) fn dirt(v: &mut Vec<HudVertex>, s: f32, r: [f32; 4], scroll: f32, tint: u32) {
    let (x1, y1) = (r[0] + r[2], r[1] + r[3]);
    let mut y = r[1];
    while y < y1 {
        let ty = (y + scroll).rem_euclid(32.0); // where in its tile this row starts
        let hh = (32.0 - ty).min(y1 - y);
        let mut x = r[0];
        while x < x1 {
            let tx = x.rem_euclid(32.0);
            let ww = (32.0 - tx).min(x1 - x);
            let (a, b, _, _) = sp::DIRT;
            HudPipeline::push_gui_tinted(v, x * s, y * s, ww * s, hh * s, (a + tx / 2.0, b + ty / 2.0, ww / 2.0, hh / 2.0), rgb(tint));
            x += ww;
        }
        y += hh;
    }
}

fn rect(v: &mut Vec<HudVertex>, s: f32, r: [f32; 4], c: u32) {
    HudPipeline::push_quad(v, r[0] * s, r[1] * s, r[2] * s, r[3] * s, rgb(c));
}

/// `Gui.drawGradientRect` (top colour to bottom colour), GUI units.
pub(crate) fn grad(v: &mut Vec<HudVertex>, s: f32, r: [f32; 4], top: [f32; 4], bottom: [f32; 4]) {
    let p = |x: f32, y: f32, c: [f32; 4]| HudVertex { pos: [x * s, y * s], color: c, uv: NO_UV };
    let (tl, tr, br, bl) = (p(r[0], r[1], top), p(r[0] + r[2], r[1], top), p(r[0] + r[2], r[1] + r[3], bottom), p(r[0], r[1] + r[3], bottom));
    v.extend([tl, tr, br, tl, br, bl]);
}

/// `GuiTextField.drawTextBox`: a grey frame round black, the text in white (a `_` blinks after it while it has the focus).
fn text_box(v: &mut Vec<HudVertex>, s: f32, r: [f32; 4], t: &str, focus: bool) {
    rect(v, s, [r[0] - 1.0, r[1] - 1.0, r[2] + 2.0, r[3] + 2.0], 0xA0A0A0);
    rect(v, s, r, 0x000000);
    let blink = focus && now_nanos() / 300_000_000 % 2 == 0;
    let shown = if blink { format!("{t}_") } else { t.to_string() };
    HudPipeline::push_text(v, (r[0] + 4.0) * s, (r[1] + (r[3] - 8.0) / 2.0) * s, s, &shown, rgb(0xE0E0E0));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mcrs-menu-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A tap in the middle of the first enabled button called `label`.
    fn tap(m: &mut Menu, label: &str, (w, h): (f32, f32)) {
        let (s, gw, gh) = scaled(w, h);
        let r = m.buttons(gw, gh).into_iter().find(|b| b.2 == label && b.3).unwrap_or_else(|| panic!("no enabled {label} on {:?}", m.scr)).1;
        let (x, y) = ((r[0] + r[2] / 2.0) * s, (r[1] + r[3] / 2.0) * s);
        m.event(ScreenEv::Down(x, y), w, h, &mut Settings::default());
        m.event(ScreenEv::Up(x, y), w, h, &mut Settings::default());
    }

    #[test]
    fn gui_scale_is_the_biggest_that_leaves_320_by_240() {
        assert_eq!(scaled(2400.0, 1080.0), (4.0, 600.0, 270.0));
        assert_eq!(scaled(320.0, 240.0), (1.0, 320.0, 240.0));
        assert_eq!(scaled(1920.0, 1080.0).0, 4.0);
    }

    #[test]
    fn dates_seeds_and_folders() {
        assert_eq!(date(0), "1/1/70 12:00 AM");
        assert_eq!(date(1_760_547_720), "10/15/25 5:02 PM");
        assert_eq!((parse_seed("", 7), parse_seed("0", 7), parse_seed("-5", 7), parse_seed("hi", 7)), (7, 7, -5, 3329));
        let root = tmp("folder");
        assert_eq!(folder_for(&root, "a/b:c"), "a_b_c");
        std::fs::create_dir_all(saves(&root).join("x")).unwrap();
        assert_eq!(folder_for(&root, " x "), "x-");
    }

    /// Main -> Select -> Create (a name, the tectonic type) -> the world is on disk, listed, loading; then delete it.
    #[test]
    fn create_list_and_delete_a_world() {
        let root = tmp("flow");
        let sz = (2400.0, 1080.0);
        let mut m = Menu::new(&root);
        tap(&mut m, "Singleplayer", sz);
        assert_eq!(m.scr, Scr::Select);
        tap(&mut m, "Create New World", sz);
        assert_eq!((m.scr, m.name.as_str()), (Scr::Create, "New World"));
        tap(&mut m, "World Type: Vanilla", sz);
        assert!(m.tectonic);
        tap(&mut m, "Create New World", sz);
        let info = m.loading.clone().expect("create starts loading");
        assert_eq!((info.name.as_str(), info.folder.as_str(), info.tectonic), ("New World", "New World", true));
        let mut v = Vec::new();
        m.draw(&mut v, sz.0, sz.1, &Settings::default());
        assert!(m.shown_loading && !m.panorama() && !v.is_empty());

        let mut m = Menu::new(&root);
        tap(&mut m, "Singleplayer", sz);
        assert_eq!(m.worlds.len(), 1);
        assert_eq!((m.worlds[0].seed, m.worlds[0].tectonic), (info.seed, true));
        let (s, gw, gh) = scaled(sz.0, sz.1);
        let (x, y) = ((gw / 2.0).floor() * s, (Menu::list_box(gh).0 + 20.0) * s);
        m.event(ScreenEv::Down(x, y), sz.0, sz.1, &mut Settings::default());
        m.event(ScreenEv::Up(x, y), sz.0, sz.1, &mut Settings::default());
        assert_eq!(m.sel, Some(0));
        tap(&mut m, "Delete", sz);
        assert_eq!(m.scr, Scr::Delete);
        tap(&mut m, "Delete", sz);
        assert!(m.worlds.is_empty() && !saves(&root).join("New World").exists());
        m.draw(&mut v, sz.0, sz.1, &Settings::default());
    }

    #[test]
    fn old_saves_move_into_the_list() {
        let root = tmp("legacy");
        std::fs::create_dir_all(root.join("world-cafebabe")).unwrap();
        std::fs::create_dir_all(root.join("world-cafebabe-rtg")).unwrap();
        let l = list(&root);
        assert_eq!((l.len(), l[0].tectonic, l[0].seed), (1, false, LEGACY_SEED), "the Realistic save is left alone");
        assert!(root.join("world-cafebabe-rtg").is_dir());
    }
}
