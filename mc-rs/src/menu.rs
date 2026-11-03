//! M14: the screens before a world is loaded, ported from b1.7.3: `GuiMainMenu` (dirt, `mclogo.png`, the yellow splash), `GuiSelectWorld`
//! (+ `GuiSlot`, `GuiWorldSlot`), `GuiCreateWorld`, `GuiRenameWorld`, `GuiYesNo` (delete) and the "Loading level" screen. Everything is laid out in GUI units
//! (`ScaledResolution`: the biggest whole scale that leaves 320 x 240) and drawn with the HUD pipeline.
//!
//! UNVERIFIED (b1.7.3 has no such thing): the world-type button of the create screen (`GuiCreateWorld` has none until 1.1) and the phone's
//! own soft keyboard (`lib.rs` shows it while `Menu::typing`, and feeds the keys to `type_char` / `backspace` / `end_typing`). Not done: Multiplayer and Mods and Texture Packs
//! (drawn greyed out like a disabled `GuiButton`; Options is `options.rs`), the splash's calendar days, the local time zone (dates are UTC).

use crate::gen_ui::GenUi;
use crate::input::touch_ui::ScreenEv;
use crate::options::{OptEv, OptionsUi, Settings};
use crate::ui;
use crate::render::hud::{sprites as sp, HudPipeline, HudVertex, NO_UV};
use crate::render::player;
use crate::skin::{self, Look};
use crate::world::gen::natural::NaturalSettings;
use crate::world::gen::overworld::WorldType;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const SPLASHES: &str = include_str!("../assets/splashes.txt");
/// `GuiTextField`'s `maxStringLength` for the world name.
const MAX_NAME: usize = 32;
/// The seed the `world-cafebabe` save (before M14) was made with.
const LEGACY_SEED: i64 = 0xCAFEBABE;

/// A world on disk: `<root>/saves/<folder>/info` (name, seed, type, pack: one per line) next to its `level`, chunk and decor files, and
/// its `natural` file (the World-Gen Settings, only when they are not the defaults).
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub folder: String,
    pub name: String,
    pub seed: i64,
    pub ty: WorldType,
    /// The Immersive Pack rule (`world::combat::rules`): Cortes Combat, the healing and the QoL. Off for worlds made before it (their
    /// `info` has no fourth line); the Create World screen's switch sets it.
    pub pack: bool,
    /// The World-Gen Settings (`gen_ui`): the Natural Temperature climate, off by default; fixed when the world is made.
    pub nat: NaturalSettings,
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
        let text = format!("{}\n{}\n{}\n{}\n", self.name, self.seed, self.ty.name(), u8::from(self.pack));
        if let Err(e) = std::fs::write(d.join("info"), text) {
            log::warn!("world info {}: {e}", self.folder);
        }
        let nat = d.join("natural");
        if self.nat == NaturalSettings::default() {
            let _ = std::fs::remove_file(&nat);
        } else if let Err(e) = std::fs::write(&nat, self.nat.to_text()) {
            log::warn!("world natural {}: {e}", self.folder);
        }
    }

    fn read(root: &Path, folder: &str) -> Option<Self> {
        let d = saves(root).join(folder);
        let text = std::fs::read_to_string(d.join("info")).ok()?;
        let mut l = text.lines();
        let (name, seed, ty) = (l.next()?, l.next()?.trim().parse().ok()?, l.next()?);
        let pack = l.next().is_some_and(|p| p.trim() == "1");
        let nat = std::fs::read_to_string(d.join("natural")).map(|t| NaturalSettings::from_text(&t)).unwrap_or_default();
        let secs = |p: PathBuf| std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |t| t.as_secs());
        let bytes = std::fs::read_dir(&d).map_or(0, |r| r.flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum());
        let modified = secs(d.join("level")).max(secs(d.join("info")));
        Some(Self { folder: folder.into(), name: name.into(), seed, ty: WorldType::parse(ty), pack, nat, modified, bytes })
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
            Info { folder: old.into(), name: "Old world".into(), seed: LEGACY_SEED, ty: WorldType::Vanilla, pack: false, nat: Default::default(), modified: 0, bytes: 0 }.write(root);
        }
    }
    let mut v: Vec<Info> = std::fs::read_dir(saves(root)).map_or_else(|_| Vec::new(), |r| r.flatten().filter_map(|e| Info::read(root, e.file_name().to_str()?)).collect());
    v.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.folder.cmp(&b.folder)));
    v
}

/// `GuiRenameWorld` -> `ISaveFormat.func_22170_a`: only the name changes, the folder keeps its name (as in b1.7.3). The `info` file is
/// written again and its time put back, so the list (sorted by time played) keeps its order.
fn rename_world(root: &Path, w: &Info, name: &str) {
    Info { name: name.chars().take(MAX_NAME).collect(), ..w.clone() }.write(root);
    let t = UNIX_EPOCH + std::time::Duration::from_secs(w.modified);
    let _ = std::fs::File::options().write(true).open(w.dir(root).join("info")).and_then(|f| f.set_modified(t));
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
    /// The World-Gen Settings of the world being created (`gen_ui`), opened from Create World's top right.
    GenSettings,
    Main,
    Select,
    Create,
    Delete,
    /// `GuiRenameWorld`, from the list's "Rename" (the typed name is `Menu::name`).
    Rename,
    /// `GuiOptions`, from the title screen's "Options...".
    Options,
    /// NOT b1.7.3: the skin customizer, from the title screen's model button (`skin::Look`: tone, hair, shirt, trousers).
    Skin,
    /// NOT b1.7.3: the list of skin files `skin::found` saw, from the skin screen's "Import Custom Skin" (`Menu::picks`).
    Pick,
}

/// Height of a row of the skin picker (GUI units).
const PICK_ROW: f32 = 20.0;

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
    ty: WorldType,
    /// The create screen's immersive pack switch (see `Info::pack`).
    pack: bool,
    /// The World-Gen Settings being edited for the world being created, and the open screen (`Scr::GenSettings`).
    nat: NaturalSettings,
    gen: Option<GenUi>,
    /// The text field being typed in (0 name, 1 seed): the keyboard is up.
    typing: Option<u8>,
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
    /// The player's look (`<root>/skin.txt`), the title screen's model wears it; `skin_dirty` says the GPU does not have it yet (`take_skin`).
    look: Look,
    skin_dirty: bool,
    /// The skin files of the picker (`Scr::Pick`): name and path, newest first; `sel` and `scroll` are the list's.
    picks: Vec<(String, PathBuf)>,
    /// An imported skin (`skin::import`, kept as `<root>/custom_skin.png`): it replaces the palette look until Reset. `note` is the line the
    /// skin screen shows under the buttons (what the last import did).
    custom: Option<Vec<u8>>,
    note: String,
    /// The skin screen's model: degrees a drag has turned it, and the x (px) the finger was last at.
    spin: f32,
    drag_x: f32,
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
            ty: WorldType::Vanilla,
            pack: true,
            nat: NaturalSettings::default(),
            gen: None,
            typing: None,
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
            look: Look::load(root),
            skin_dirty: false,
            picks: Vec::new(),
            custom: skin::load_custom(root),
            note: String::new(),
            spin: 0.0,
            drag_x: 0.0,
        }
    }

    /// The title screen's player stands left of the button stack: the room there (GUI units) and the middle of it.
    fn side(hw: f32) -> (f32, f32) {
        let space = hw - 112.0; // the stack's panel starts at hw - 100 - 12
        (space, space / 2.0)
    }

    /// The new skin (64 x 32 RGBA) when the look changed since the last call: the app writes it into the atlas and the HUD sheet.
    pub fn take_skin(&mut self) -> Option<Vec<u8>> {
        std::mem::take(&mut self.skin_dirty).then(|| self.custom.clone().unwrap_or_else(|| self.look.apply()))
    }

    fn set_look(&mut self, l: Look) {
        if l != self.look {
            self.look = l;
            self.look.save(&self.root);
            self.skin_dirty = true;
        }
    }

    /// The player as the title and skin screens show him: feet at (`cx`, `feet`) GUI units, `mult` times the inventory's size, turned `yaw`.
    fn model(v: &mut Vec<HudVertex>, s: f32, (cx, feet): (f32, f32), mult: f32, yaw: f32, ms: f32) {
        let pose = player::Pose { age: ms / 50.0, ..Default::default() };
        HudPipeline::push_player(v, player::title_quads(cx * s, feet * s, s * mult, &pose, yaw));
    }

    // ---- layout ----

    fn buttons(&self, gw: f32, gh: f32) -> Vec<Btn> {
        let hw = (gw / 2.0).floor();
        let b = |id, x, y, w, l: &str, on| Btn(id, [x, y, w, 20.0], l.to_string(), on);
        match self.scr {
            Scr::Main => {
                let y = (gh / 4.0).floor() + 48.0;
                // The model's button sits under his feet (the stack's bottom, y + 104), as wide as the room left of the stack allows.
                let (space, cx) = Self::side(hw);
                let bw = (space - 8.0).clamp(40.0, 96.0);
                vec![
                    b(1, hw - 100.0, y, 200.0, "Singleplayer", true),
                    b(2, hw - 100.0, y + 24.0, 200.0, "Multiplayer", false),
                    b(3, hw - 100.0, y + 48.0, 200.0, "Mods and Texture Packs", false),
                    b(0, hw - 100.0, y + 84.0, 98.0, "Options...", true),
                    b(4, hw + 2.0, y + 84.0, 98.0, "Quit Game", true),
                    b(5, (cx - bw / 2.0).max(2.0), y + 110.0, bw, if bw >= 90.0 { "Customize Skin" } else { "Skin..." }, true),
                ]
            }
            Scr::Select => vec![
                b(1, hw - 154.0, gh - 52.0, 150.0, "Play Selected World", self.sel.is_some()),
                b(6, hw - 154.0, gh - 28.0, 70.0, "Rename", self.sel.is_some()),
                b(2, hw - 74.0, gh - 28.0, 70.0, "Delete", self.sel.is_some()),
                b(3, hw + 4.0, gh - 52.0, 150.0, "Create New World", true),
                b(0, hw + 4.0, gh - 28.0, 150.0, "Cancel", true),
            ],
            Scr::Create => {
                // Below the last hint (y 140..148), with the gap between buttons growing from 2 to 6 GUI units as the screen allows.
                let (y0, pitch) = (152.0, ((gh - 4.0 - 152.0) / 4.0).clamp(22.0, 26.0));
                vec![
                    b(2, hw - 100.0, y0, 200.0, match self.ty { WorldType::Vanilla => "World Type: Vanilla", WorldType::Tectonic => "World Type: Tectonic", WorldType::Blooming => "World Type: Blooming" }, true),
                    b(3, hw - 100.0, y0 + pitch, 200.0, if self.pack { "Immersive Pack: ON" } else { "Immersive Pack: OFF" }, true),
                    b(0, hw - 100.0, y0 + 2.0 * pitch, 200.0, "Create New World", !self.name.is_empty()),
                    b(1, hw - 100.0, y0 + 3.0 * pitch, 200.0, "Cancel", true),
                    // Top right; under the title's band instead when the screen is too narrow to keep clear of its text.
                    b(4, gw - tw("World-Gen Settings") - 14.0, if gw - tw("World-Gen Settings") - 14.0 < hw + 56.0 { 24.0 } else { 2.0 }, tw("World-Gen Settings") + 12.0, "World-Gen Settings", true),
                ]
            }
            Scr::Delete => {
                let y = (gh / 6.0).floor() + 96.0;
                vec![b(0, hw - 155.0, y, 150.0, "Delete", true), b(1, hw + 5.0, y, 150.0, "Cancel", true)]
            }
            Scr::Rename => {
                let y = (gh / 4.0).floor() + 96.0 + 12.0; // `GuiRenameWorld.initGui`
                vec![b(0, hw - 100.0, y, 200.0, "Rename", !self.name.trim().is_empty()), b(1, hw - 100.0, y + 24.0, 200.0, "Cancel", true)]
            }
            Scr::Options | Scr::GenSettings => Vec::new(),
            Scr::Pick => vec![
                b(1, hw - 154.0, gh - 28.0, 150.0, "Use Selected Skin", self.sel.is_some()),
                b(0, hw + 4.0, gh - 28.0, 150.0, "Cancel", true),
            ],
            Scr::Skin => {
                // The right half: the four palettes, Reset and Done under the title's band; the model has the left half.
                let (x, w) = (hw + 8.0, (hw - 16.0).min(150.0));
                // An imported skin is not repainted: the palette buttons wait (dim) until Reset.
                let own = self.custom.is_none();
                let l = |p: usize, t: &str| format!("{t}: {}", if own { self.look.name(p) } else { "Custom" });
                let c = |id, y, label: &str, on| Btn(id, [x, y, w, 20.0], label.to_string(), on);
                vec![
                    c(1, 34.0, &l(skin::SKIN, "Skin Tone"), own),
                    c(2, 58.0, &l(skin::HAIR, "Hair"), own),
                    c(3, 82.0, &l(skin::SHIRT, "Shirt"), own),
                    c(4, 106.0, &l(skin::PANTS, "Pants"), own),
                    c(6, 134.0, "Import Custom Skin", true),
                    c(5, 158.0, "Reset", true),
                    c(0, (gh - 28.0).max(210.0), "Done", true),
                ]
            }
        }
    }

    /// `GuiSlot`'s box: top and bottom y (the list of `GuiSelectWorld`: 32 .. height - 64), slot height 36.
    fn list_box(gh: f32) -> (f32, f32) {
        (32.0, gh - 64.0)
    }

    fn max_scroll(&self, gh: f32) -> f32 {
        let (top, bottom) = Self::list_box(gh);
        let (n, row) = if self.scr == Scr::Pick { (self.picks.len(), PICK_ROW) } else { (self.worlds.len(), 36.0) };
        (n as f32 * row - (bottom - top - 4.0)).max(0.0)
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

    /// The options screen is up: the pad's bumpers step its tabs.
    pub fn in_options(&self) -> bool {
        self.scr == Scr::Options
    }

    pub fn step_tab(&mut self, d: i32, st: &mut Settings) {
        self.opt.step_tab(d, st);
    }

    // ---- input ----

    /// A text field has the focus: the app shows the soft keyboard.
    pub fn typing(&self) -> bool {
        self.typing.is_some()
    }

    /// A character from the soft keyboard (printable ASCII only: that is what the HUD font has).
    pub fn type_char(&mut self, c: char) {
        if let Some(f) = self.field_mut().filter(|f| f.len() < MAX_NAME) {
            if c.is_ascii_graphic() || c == ' ' {
                f.push(c);
            }
        }
        self.folder = folder_for(&self.root, &self.name);
    }

    pub fn backspace(&mut self) {
        if let Some(f) = self.field_mut() {
            f.pop();
        }
        self.folder = folder_for(&self.root, &self.name);
    }

    /// Enter / Back / a tap elsewhere: the keyboard goes down.
    pub fn end_typing(&mut self) {
        self.typing = None;
    }

    fn field_mut(&mut self) -> Option<&mut String> {
        match self.typing? {
            0 => Some(&mut self.name),
            _ => Some(&mut self.seed),
        }
    }

    pub fn event(&mut self, ev: ScreenEv, w: f32, h: f32, st: &mut Settings) {
        self.gs = st.gui_scale;
        if self.scr == Scr::Options {
            if self.opt.event(ev, w, h, st) == Some(OptEv::Close) {
                self.scr = Scr::Main;
            }
            return;
        }
        if self.scr == Scr::GenSettings {
            if self.gen.as_mut().is_some_and(|g| g.event(ev, w, h, st.gui_scale)) {
                if let Some(g) = self.gen.take() {
                    self.nat = g.s;
                }
                self.scr = Scr::Create;
            }
            return;
        }
        let (s, _, gh) = scaled_gs(w, h, self.gs);
        match ev {
            ScreenEv::Down(x, y) => {
                self.press = Some((x / s, y / s));
                (self.press_scroll, self.dragging, self.drag_x) = (self.scroll, false, x);
            }
            ScreenEv::Move(x, y) => {
                if let Some((px, py)) = self.press {
                    // On the skin screen a sideways drag turns the model (and is not a tap on the button it ends over).
                    if self.scr == Scr::Skin {
                        if (x / s - px).abs() > 4.0 {
                            self.dragging = true;
                        }
                        if self.dragging {
                            self.spin += (x - self.drag_x) / s * 1.2;
                        }
                        self.drag_x = x;
                    }
                    if (y / s - py).abs() > 4.0 {
                        self.dragging = true;
                    }
                    if self.dragging && matches!(self.scr, Scr::Select | Scr::Pick) {
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
        if self.typing.is_some() {
            self.typing = None; // a tap anywhere closes the soft keyboard (the field keeps its text)
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
            Scr::Pick => {
                let (top, bottom) = Self::list_box(gh);
                let hw = (gw / 2.0).floor();
                if gy >= top && gy < bottom && (gx - hw).abs() < 110.0 {
                    let i = ((gy - top + self.scroll - 4.0) / PICK_ROW).floor();
                    if i >= 0.0 && (i as usize) < self.picks.len() {
                        let i = i as usize;
                        let twice = self.last_click.is_some_and(|(j, t)| j == i && t.elapsed().as_millis() < 500);
                        (self.sel, self.last_click) = (Some(i), Some((i, Instant::now())));
                        if twice {
                            self.pressed(1); // a double tap is "Use Selected Skin"
                        }
                    }
                }
            }
            Scr::Create => {
                if let Some(i) = Self::fields(gw).iter().position(|&r| hit(r)) {
                    self.typing = Some(i as u8);
                }
            }
            Scr::Rename => {
                if hit(Self::fields(gw)[0]) {
                    self.typing = Some(0); // the same name field as the create screen's, at y 60
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
            (Scr::Main, 5) => (self.scr, self.spin) = (Scr::Skin, 0.0),
            (Scr::Skin, 0) => self.scr = Scr::Main,
            (Scr::Skin, 5) => {
                if self.custom.take().is_some() {
                    skin::clear_custom(&self.root);
                    self.skin_dirty = true;
                }
                self.note.clear();
                self.set_look(Look::default());
            }
            (Scr::Skin, 6) => {
                let found = skin::found(&self.root);
                if found.is_empty() {
                    // Nothing seen: maybe the storage permission is missing, so the main loop asks (once granted, tap again).
                    skin::ASK_STORAGE.store(true, std::sync::atomic::Ordering::Relaxed);
                    self.note = "No skin .png found".to_string();
                } else {
                    (self.picks, self.sel, self.scroll, self.scr, self.note) = (found, None, 0.0, Scr::Pick, String::new());
                }
            }
            (Scr::Pick, 0) => self.scr = Scr::Skin,
            (Scr::Pick, 1) => {
                let Some((name, path)) = self.sel.and_then(|i| self.picks.get(i)).cloned() else { return };
                self.scr = Scr::Skin;
                match skin::import_file(&path, &self.root) {
                    Ok(px) => (self.custom, self.skin_dirty, self.note) = (Some(px), true, format!("Imported {name}")),
                    Err(e) => self.note = e.to_string(),
                }
            }
            (Scr::Skin, id @ 1..=4) => {
                let mut l = self.look;
                l.cycle(id as usize - 1);
                self.set_look(l);
            }
            (Scr::Select, 0) => self.scr = Scr::Main,
            (Scr::Select, 1) => self.loading = self.sel.and_then(|i| self.worlds.get(i)).cloned(),
            (Scr::Select, 2) => self.scr = Scr::Delete,
            (Scr::Select, 6) => {
                // `GuiRenameWorld`: the field starts with the world's name.
                if let Some(n) = self.sel.and_then(|i| self.worlds.get(i)).map(|w| w.name.clone()) {
                    (self.name, self.typing, self.scr) = (n, None, Scr::Rename);
                }
            }
            (Scr::Select, 3) => {
                (self.name, self.seed, self.ty, self.typing, self.scr) = (String::from("New World"), String::new(), WorldType::Vanilla, None, Scr::Create);
                self.pack = true;
                (self.nat, self.gen) = (NaturalSettings::default(), None);
                self.folder = folder_for(&self.root, &self.name);
            }
            (Scr::Create, 0) => {
                let info = Info { folder: folder_for(&self.root, &self.name), name: self.name.clone(), seed: parse_seed(&self.seed, now_nanos() as i64), ty: self.ty, pack: self.pack, nat: self.nat.clone(), modified: 0, bytes: 0 };
                info.write(&self.root);
                self.loading = Some(info);
            }
            (Scr::Create, 1) => self.scr = Scr::Select,
            (Scr::Create, 2) => self.ty = self.ty.next(),
            (Scr::Create, 3) => self.pack = !self.pack,
            (Scr::Create, 4) => (self.gen, self.scr) = (Some(GenUi::new(self.nat.clone(), true)), Scr::GenSettings),
            (Scr::Delete, 0) => {
                if let Some(w) = self.sel.and_then(|i| self.worlds.get(i)) {
                    if let Err(e) = std::fs::remove_dir_all(w.dir(&self.root)) {
                        log::warn!("delete {}: {e}", w.folder);
                    }
                }
                self.open_list();
            }
            (Scr::Delete, 1) => self.scr = Scr::Select,
            (Scr::Rename, 0) => {
                let name = self.name.trim().to_string();
                if let Some(w) = self.sel.and_then(|i| self.worlds.get(i)).cloned() {
                    if !name.is_empty() {
                        rename_world(&self.root, &w, &name);
                    }
                    self.open_list();
                    self.sel = self.worlds.iter().position(|x| x.folder == w.folder); // the list keeps its order and the choice
                }
            }
            (Scr::Rename, 1) => self.scr = Scr::Select,
            _ => {}
        }
    }

    fn open_list(&mut self) {
        (self.worlds, self.sel, self.scroll, self.scr) = (list(&self.root), None, 0.0, Scr::Select);
    }

    /// The title screen and the skin screen show the panorama (`render_menu` draws it under the HUD); every other screen keeps its dirt.
    pub fn panorama(&self) -> bool {
        matches!(self.scr, Scr::Main | Scr::Skin) && !self.shown_loading
    }

    /// What the screen fills that the pad's button hints keep off (px): the buttons, the text boxes, the footer and header texts, the list.
    pub fn avoid(&self, w: f32, h: f32, st: &Settings) -> Vec<[f32; 4]> {
        if self.scr == Scr::Options {
            return self.opt.avoid(w, h, st);
        }
        if self.scr == Scr::GenSettings {
            return self.gen.as_ref().map_or_else(Vec::new, |g| g.avoid(w, h, st.gui_scale));
        }
        let (s, gw, gh) = scaled_gs(w, h, st.gui_scale);
        let hw = (gw / 2.0).floor();
        let mut r: Vec<[f32; 4]> = self.buttons(gw, gh).into_iter().map(|b| b.1).collect();
        r.push([hw - 60.0, 0.0, 120.0, 24.0]); // a screen's title
        if self.loading.is_some() {
            r.push([hw - 80.0, gh / 2.0 - 20.0, 160.0, 40.0]);
        }
        if self.typing.is_some() {
            r.push([0.0, 0.0, gw, gh]); // the typing overlay: no room to spare
        }
        match self.scr {
            Scr::Main => {
                let c = "Copyright Mojang AB. Do not distribute.";
                r.push([gw - tw(c) - 4.0, gh - 11.0, tw(c) + 4.0, 11.0]);
                r.push([0.0, 0.0, tw(&crate::version::label()).max(tw("Minecraft Beta 1.7.3")) + 4.0, 22.0]);
                r.push([hw - 137.0, 4.0, 320.0, 76.0]); // the logo and the splash
            }
            Scr::Select => {
                let (top, bottom) = Self::list_box(gh);
                r.push([0.0, top, gw, bottom - top]);
            }
            Scr::Pick => {
                let (top, bottom) = Self::list_box(gh);
                r.push([0.0, top, gw, bottom - top]);
            }
            Scr::Create | Scr::Rename => {
                r.extend(Self::fields(gw));
                r.push([hw - 100.0, 40.0, 200.0, 112.0]); // the labels over and under the boxes
            }
            Scr::Delete => r.push([hw - 170.0, 62.0, 340.0, 44.0]),
            Scr::Skin => {
                r.push([0.0, 24.0, hw, gh - 24.0]); // the player and "Drag to turn"
                r.push([hw + 2.0, 30.0, hw - 4.0, (gh - 28.0).max(210.0) - 4.0]);
            }
            Scr::Options | Scr::GenSettings => {}
        }
        r.into_iter().map(|r| r.map(|u| u * s)).collect()
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
        if self.scr == Scr::GenSettings {
            if let Some(g) = &self.gen {
                g.draw(v, w, h, st.gui_scale, false);
            }
            return;
        }
        if let Some(t) = self.typing {
            self.draw_typing(v, s, gw, t);
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
                // a panel behind the stack of buttons, so the panorama does not fight the labels
                let (x0, y0, x1, y1) = self.buttons(gw, gh).iter().filter(|b| b.0 != 5).fold((f32::MAX, f32::MAX, 0.0f32, 0.0f32), |a, b| (a.0.min(b.1[0]), a.1.min(b.1[1]), a.2.max(b.1[0] + b.1[2]), a.3.max(b.1[1] + b.1[3])));
                ui::panel(v, s, [x0 - 12.0, y0 - 12.0, x1 - x0 + 24.0, y1 - y0 + 24.0], 0.4);
                // The player, left of the stack with his feet on its bottom line, swaying a quarter turn each way (his face stays in view).
                let (space, cx) = Self::side(hw);
                Self::model(v, s, (cx, y1), (space / 44.0).clamp(0.8, 1.6), 25f32.to_radians() * (ms / 1200.0).sin(), ms);
                txt(v, 2.0, 2.0, "Minecraft Beta 1.7.3", 0xFFFFFF, false); // was 0x505050, grey on dirt: unreadable over the panorama
                txt(v, 2.0, 12.0, &crate::version::label(), 0xA0A0A0, false); // the CI build and commit (`version.rs`), `dev` for a local build
                let c = "Copyright Mojang AB. Do not distribute.";
                txt(v, gw - tw(c) - 2.0, gh - 10.0, c, 0xFFFFFF, false);
            }
            Scr::Select => self.draw_list(v, s, gw, gh),
            Scr::Pick => self.draw_picks(v, s, gw, gh),
            Scr::Create => {
                ui::title(v, s, gw, 0.0, "Create New World", true);
                txt(v, hw - 100.0, 47.0, "World Name", 0xA0A0A0, false);
                txt(v, hw - 100.0, 85.0, &format!("Will be saved in: {}", self.folder), 0xA0A0A0, false);
                txt(v, hw - 100.0, 104.0, "Seed for the World Generator", 0xA0A0A0, false);
                txt(v, hw - 100.0, 140.0, "Leave blank for a random seed", 0xA0A0A0, false);
                for (r, t) in Self::fields(gw).iter().zip([&self.name, &self.seed]) {
                    text_box(v, s, *r, t, false);
                }
            }
            Scr::Options | Scr::GenSettings => {}
            Scr::Skin => {
                ui::title(v, s, gw, 0.0, "Customize Skin", true);
                // The player, big, on the left half, turning slowly (a drag turns him by hand).
                let mult = ((gh - 80.0) / 60.0).clamp(1.0, 3.0);
                let feet = (gh + 24.0) / 2.0 + 56.25 * mult / 2.0;
                Self::model(v, s, (hw / 2.0, feet), mult, (ms * 0.03 + self.spin).to_radians(), ms);
                txt(v, hw / 2.0, gh - 12.0, "Drag to turn", 0xA0A0A0, true);
                let (x, w) = (hw + 8.0, (hw - 16.0).min(150.0));
                ui::panel(v, s, [x - 6.0, 30.0, w + 12.0, (gh - 28.0).max(210.0) + 26.0 - 30.0], 0.4);
                // What the last import did, and where the file goes when none was found: two short lines between Reset and Done.
                let path = if self.note.starts_with("No skin") { "in Download or Pictures" } else { "" };
                for (i, line) in [self.note.as_str(), path].into_iter().enumerate() {
                    txt(v, x + w / 2.0, 184.0 + 10.0 * i as f32, &ui::fit(line, w + 8.0), 0xA0A0A0, true);
                }
            }
            Scr::Rename => {
                ui::title(v, s, gw, 0.0, "Rename World", true);
                txt(v, hw - 100.0, 47.0, "World Name", 0xA0A0A0, false);
                text_box(v, s, Self::fields(gw)[0], &self.name, false);
            }
            Scr::Delete => {
                let name = ui::fit(self.sel.and_then(|i| self.worlds.get(i)).map_or("", |w| w.name.as_str()), 140.0);
                ui::title(v, s, gw, 0.0, "Delete World", true);
                ui::panel(v, s, [hw - 170.0, 62.0, 340.0, 44.0], 0.5);
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
        if self.worlds.is_empty() {
            for (i, t) in ["No worlds yet.", "Tap Create New World to start one."].into_iter().enumerate() {
                HudPipeline::push_text(v, (hw - tw(t) / 2.0) * s, (top + 16.0 + 12.0 * i as f32) * s, s, t, rgb(0x808080));
            }
        }
        for (i, wd) in self.worlds.iter().enumerate() {
            let y = top + 4.0 - self.scroll + i as f32 * 36.0;
            if y + 36.0 < top || y > bottom {
                continue;
            }
            if self.sel == Some(i) {
                rect(v, s, [hw - 110.0, y - 2.0, 220.0, 36.0], 0x808080);
                rect(v, s, [hw - 109.0, y - 1.0, 218.0, 34.0], 0x000000);
            }
            // the kind is a coloured tag on the right of the name
            let (kind, kc) = match wd.ty { WorldType::Vanilla => ("Vanilla", 0xAAAAAA), WorldType::Tectonic => ("Tectonic", 0x55FFFF), WorldType::Blooming => ("Blooming", 0xFF55FF) };
            let line = format!("{} ({}, {:?} MB)", wd.folder, date(wd.modified), (wd.bytes / 1024 * 100 / 1024) as f32 / 100.0);
            HudPipeline::push_text(v, (hw - 108.0) * s, (y + 1.0) * s, s, &ui::fit(&wd.name, 216.0 - tw(kind) - 8.0), rgb(0xFFFFFF));
            HudPipeline::push_text(v, (hw + 108.0 - tw(kind)) * s, (y + 1.0) * s, s, kind, rgb(kc));
            HudPipeline::push_text(v, (hw - 108.0) * s, (y + 12.0) * s, s, &ui::fit(&line, 216.0), rgb(0x808080));
        }
        // The dirt over the list's ends, and their 4 px fade into the list.
        dirt(v, s, [0.0, 0.0, gw, top], 0.0, 0x404040);
        dirt(v, s, [0.0, bottom, gw, gh], 0.0, 0x404040);
        let (clear, black) = ([0.0; 4], [0.0, 0.0, 0.0, 1.0]);
        grad(v, s, [0.0, top, gw, 4.0], black, clear);
        grad(v, s, [0.0, bottom - 4.0, gw, 4.0], clear, black);
        ui::scrollbar(v, s, hw + 124.0, top, bottom - top, self.worlds.len() as f32 * 36.0, self.scroll);
        ui::title(v, s, gw, 4.0, "Select World", false);
    }

    /// The skin picker: one row per file, its name and the folder it lies in, over the same dirt, fades and scroll bar as the world list.
    fn draw_picks(&self, v: &mut Vec<HudVertex>, s: f32, gw: f32, gh: f32) {
        let hw = (gw / 2.0).floor();
        let (top, bottom) = Self::list_box(gh);
        dirt(v, s, [0.0, top, gw, bottom], self.scroll, 0x202020);
        for (i, (name, path)) in self.picks.iter().enumerate() {
            let y = top + 4.0 - self.scroll + i as f32 * PICK_ROW;
            if y + PICK_ROW < top || y > bottom {
                continue;
            }
            if self.sel == Some(i) {
                rect(v, s, [hw - 110.0, y - 2.0, 220.0, PICK_ROW], 0x808080);
                rect(v, s, [hw - 109.0, y - 1.0, 218.0, PICK_ROW - 2.0], 0x000000);
            }
            let dir = path.parent().and_then(|d| d.file_name()).map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            let dir = ui::fit(&dir, 80.0);
            HudPipeline::push_text(v, (hw - 108.0) * s, (y + 5.0) * s, s, &ui::fit(name, 216.0 - tw(&dir) - 8.0), rgb(0xFFFFFF));
            HudPipeline::push_text(v, (hw + 108.0 - tw(&dir)) * s, (y + 5.0) * s, s, &dir, rgb(0x808080));
        }
        dirt(v, s, [0.0, 0.0, gw, top], 0.0, 0x404040);
        dirt(v, s, [0.0, bottom, gw, gh], 0.0, 0x404040);
        let (clear, black) = ([0.0; 4], [0.0, 0.0, 0.0, 1.0]);
        grad(v, s, [0.0, top, gw, 4.0], black, clear);
        grad(v, s, [0.0, bottom - 4.0, gw, 4.0], clear, black);
        ui::scrollbar(v, s, hw + 124.0, top, bottom - top, self.picks.len() as f32 * PICK_ROW, self.scroll);
        ui::title(v, s, gw, 4.0, "Select Skin", false);
    }

    /// The text field being typed in, alone at the top (the phone's soft keyboard covers the rest of the screen).
    fn draw_typing(&self, v: &mut Vec<HudVertex>, s: f32, gw: f32, field: u8) {
        let t = if field == 0 { &self.name } else { &self.seed };
        let title = if field == 0 { "World Name" } else { "Seed for the World Generator" };
        let hint = "Tap anywhere to finish";
        HudPipeline::push_text(v, (gw - tw(title)) / 2.0 * s, 14.0 * s, s, title, rgb(0xA0A0A0));
        text_box(v, s, [(gw - 200.0) / 2.0, 40.0, 200.0, 20.0], t, true);
        HudPipeline::push_text(v, (gw - tw(hint)) / 2.0 * s, 68.0 * s, s, hint, rgb(0xA0A0A0));
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

pub(crate) fn rect(v: &mut Vec<HudVertex>, s: f32, r: [f32; 4], c: u32) {
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

    /// Main -> the model's button -> the skin screen: each palette button steps its colour and hands the app a new skin once, the choice is kept
    /// in `skin.txt`, a sideways drag turns the model and presses nothing, Reset and Done; both screens draw the 42 quads of the model.
    #[test]
    fn the_skin_screen_changes_the_look_and_hands_over_the_skin_once() {
        let root = tmp("skin");
        let (sz, mut st) = ((2400.0, 1080.0), Settings::default());
        let mut m = Menu::new(&root);
        assert!(m.take_skin().is_none(), "nothing changed yet");
        let mut v = Vec::new();
        m.draw(&mut v, sz.0, sz.1, &st);
        assert!(v.len() > 42 * 6, "the title screen draws the player ({} vertices)", v.len());
        assert!(v.iter().all(|p| p.pos[0].is_finite() && p.pos[1].is_finite()));
        tap(&mut m, "Customize Skin", sz);
        assert_eq!(m.scr, Scr::Skin);
        assert!(m.panorama(), "the skin screen keeps the title screen's backdrop");
        v.clear();
        m.draw(&mut v, sz.0, sz.1, &st);
        assert!(v.len() > 42 * 6, "the skin screen draws the player");
        tap(&mut m, "Hair: Brown", sz);
        assert_eq!(m.look, Look([0, 1, 0, 0]));
        let px = m.take_skin().expect("a new skin");
        assert_eq!(px.len(), skin::BYTES);
        assert!(m.take_skin().is_none(), "handed over once");
        tap(&mut m, "Skin Tone: Steve", sz);
        tap(&mut m, "Shirt: Cyan", sz);
        tap(&mut m, "Pants: Blue", sz);
        assert_eq!(m.look, Look([1, 1, 1, 1]));
        assert_eq!(Look::load(&root), m.look, "kept on disk");
        assert_eq!(Menu::new(&root).look, m.look, "and found again by the next title screen");
        let before = m.spin;
        for ev in [ScreenEv::Down(300.0, 500.0), ScreenEv::Move(380.0, 500.0), ScreenEv::Up(380.0, 500.0)] {
            m.event(ev, sz.0, sz.1, &mut st);
        }
        assert!(m.spin > before && m.look == Look([1, 1, 1, 1]) && m.scr == Scr::Skin, "a drag turns the model and nothing else");
        tap(&mut m, "Reset", sz);
        assert_eq!(m.look, Look::default());
        assert!(m.take_skin().is_some() && m.take_skin().is_none());
        tap(&mut m, "Done", sz);
        assert_eq!(m.scr, Scr::Main);
        // A 4:3 screen has the least room left of the stack: the button shrinks to "Skin..." and still opens the screen.
        let small = (640.0, 480.0);
        tap(&mut m, "Skin...", small);
        assert_eq!(m.scr, Scr::Skin);
    }

    /// Import Custom Skin: with no .png it says so; with one (a 1.8 style 64 x 64) the model wears it, the palette buttons go dim, the copy
    /// is kept and found again by the next title screen, and Reset gives the palette look back and forgets the copy.
    #[test]
    fn importing_a_png_replaces_the_palette_look_until_reset() {
        let root = tmp("import");
        let sz = (2400.0, 1080.0);
        let mut m = Menu::new(&root);
        tap(&mut m, "Customize Skin", sz);
        tap(&mut m, "Import Custom Skin", sz);
        assert_eq!(m.note, "No skin .png found");
        assert!(skin::ASK_STORAGE.swap(false, std::sync::atomic::Ordering::Relaxed), "nothing found: the storage permission is asked for");
        assert!(m.custom.is_none() && m.take_skin().is_none());
        let rgba: Vec<u8> = (0..64 * 64).flat_map(|i| [200, i as u8, 30, 255]).collect();
        std::fs::write(root.join("dreamybull.png"), skin::tests::png_of(64, 64, png::ColorType::Rgba, &rgba)).unwrap();
        tap(&mut m, "Import Custom Skin", sz);
        assert_eq!((m.scr, m.picks.len()), (Scr::Pick, 1));
        let (s, gw, gh) = scaled(sz.0, sz.1);
        assert!(!m.buttons(gw, gh).iter().any(|b| b.2 == "Use Selected Skin" && b.3), "nothing chosen yet");
        let (top, _) = Menu::list_box(gh);
        let (x, y) = ((gw / 2.0).floor() * s, (top + 10.0) * s);
        m.event(ScreenEv::Down(x, y), sz.0, sz.1, &mut Settings::default());
        m.event(ScreenEv::Up(x, y), sz.0, sz.1, &mut Settings::default());
        assert_eq!(m.sel, Some(0));
        let mut v = Vec::new();
        m.draw(&mut v, sz.0, sz.1, &Settings::default());
        assert!(v.iter().all(|p| p.pos[0].is_finite() && p.pos[1].is_finite()));
        tap(&mut m, "Use Selected Skin", sz);
        assert_eq!((m.scr, m.note.as_str()), (Scr::Skin, "Imported dreamybull.png"));
        let px = m.take_skin().expect("the imported skin is handed over");
        assert_eq!(px, rgba[..skin::BYTES]);
        assert!(m.take_skin().is_none(), "once");
        let (_, gw, gh) = scaled(sz.0, sz.1);
        let dim = |m: &Menu| m.buttons(gw, gh).iter().filter(|b| !b.3).count();
        assert_eq!(dim(&m), 4, "tone, hair, shirt and pants wait while a custom skin is on");
        assert!(m.buttons(gw, gh).iter().any(|b| b.2 == "Hair: Custom"));
        assert!(Menu::new(&root).custom.is_some(), "found again by the next title screen");
        tap(&mut m, "Reset", sz);
        assert!(m.custom.is_none() && dim(&m) == 0 && !root.join("custom_skin.png").exists());
        assert_eq!(m.take_skin().expect("the palette skin is back"), Look::default().apply());
        let mut v = Vec::new();
        m.draw(&mut v, sz.0, sz.1, &Settings::default());
        assert!(v.iter().all(|p| p.pos[0].is_finite() && p.pos[1].is_finite()));
    }

    /// Main -> Select -> Create (a name, the Blooming type: two taps of the type button) -> the world is on disk, listed, loading; then delete it.
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
        assert_eq!(m.ty, WorldType::Tectonic);
        tap(&mut m, "World Type: Tectonic", sz);
        assert_eq!(m.ty, WorldType::Blooming);
        assert!(m.pack, "the immersive pack is on by default");
        tap(&mut m, "Immersive Pack: ON", sz);
        assert!(!m.pack);
        tap(&mut m, "Immersive Pack: OFF", sz);
        tap(&mut m, "Immersive Pack: ON", sz); // off for this world
        tap(&mut m, "Create New World", sz);
        let info = m.loading.clone().expect("create starts loading");
        assert_eq!((info.name.as_str(), info.folder.as_str(), info.ty, info.pack), ("New World", "New World", WorldType::Blooming, false));
        let mut v = Vec::new();
        m.draw(&mut v, sz.0, sz.1, &Settings::default());
        assert!(m.shown_loading && !m.panorama() && !v.is_empty());

        let mut m = Menu::new(&root);
        tap(&mut m, "Singleplayer", sz);
        assert_eq!(m.worlds.len(), 1);
        assert_eq!((m.worlds[0].seed, m.worlds[0].ty, m.worlds[0].pack), (info.seed, WorldType::Blooming, false), "the switch is saved with the world");
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

    /// Create -> World-Gen Settings (the button at the top right) -> the switch on -> Done -> Create: the settings are part of the world on
    /// disk (`natural`) and come back with it; a world left at the defaults writes no such file.
    #[test]
    fn world_gen_settings_are_saved_with_the_world() {
        let root = tmp("gen");
        let sz = (2400.0, 1080.0);
        let mut m = Menu::new(&root);
        tap(&mut m, "Singleplayer", sz);
        tap(&mut m, "Create New World", sz);
        tap(&mut m, "World-Gen Settings", sz);
        assert_eq!(m.scr, Scr::GenSettings);
        m.gen.as_mut().expect("the screen is open").s.enabled = true;
        let mut v = Vec::new();
        m.draw(&mut v, sz.0, sz.1, &Settings::default());
        assert!(!v.is_empty() && v.iter().all(|p| p.pos[0].is_finite() && p.pos[1].is_finite()));
        let (s, gw, gh) = scaled(sz.0, sz.1);
        let (x, y) = ((gw / 2.0).floor() * s, (gh - 18.0) * s); // the middle of Done
        m.event(ScreenEv::Down(x, y), sz.0, sz.1, &mut Settings::default());
        m.event(ScreenEv::Up(x, y), sz.0, sz.1, &mut Settings::default());
        assert!(m.scr == Scr::Create && m.nat.enabled, "Done keeps the edits and goes back to Create World");
        tap(&mut m, "Create New World", sz);
        let info = m.loading.clone().expect("create starts loading");
        assert!(info.nat.enabled && info.dir(&root).join("natural").is_file());
        let mut again = Menu::new(&root);
        tap(&mut again, "Singleplayer", sz);
        assert_eq!(again.worlds[0].nat, info.nat, "read back from the `natural` file");
        Info { nat: Default::default(), ..info.clone() }.write(&root);
        assert!(!info.dir(&root).join("natural").exists(), "the defaults write no file");
    }

    /// Select -> Rename: the field starts with the name, a blank one disables the button, the new name is saved, the folder, the other
    /// settings, the order of the list and the choice stay.
    #[test]
    fn rename_changes_the_name_only() {
        let root = tmp("rename");
        let sz = (2400.0, 1080.0);
        let info = |f: &str, n: &str| Info { folder: f.into(), name: n.into(), seed: 5, ty: WorldType::Tectonic, pack: true, nat: Default::default(), modified: 0, bytes: 0 };
        info("a", "Alpha").write(&root);
        info("b", "Beta").write(&root);
        for (f, s) in [("a", 1000), ("b", 2000)] {
            let file = std::fs::File::options().write(true).open(saves(&root).join(f).join("info")).unwrap();
            file.set_modified(UNIX_EPOCH + std::time::Duration::from_secs(s)).unwrap();
        }
        let mut m = Menu::new(&root);
        tap(&mut m, "Singleplayer", sz);
        let folders = |m: &Menu| m.worlds.iter().map(|w| w.folder.clone()).collect::<Vec<_>>();
        assert_eq!(folders(&m), ["b", "a"], "the last played first");
        let (_, gw, gh) = scaled(sz.0, sz.1);
        assert!(m.buttons(gw, gh).iter().any(|b| b.2 == "Rename" && !b.3), "nothing chosen: Rename is greyed out");
        m.sel = Some(1);
        tap(&mut m, "Rename", sz);
        assert_eq!((m.scr, m.name.as_str()), (Scr::Rename, "Alpha"));
        m.name = "  ".into();
        assert!(m.buttons(gw, gh).iter().any(|b| b.2 == "Rename" && !b.3), "a blank name cannot be saved");
        let mut v = Vec::new();
        m.draw(&mut v, sz.0, sz.1, &Settings::default());
        assert!(!v.is_empty());
        tap(&mut m, "Cancel", sz);
        assert_eq!((m.scr, m.worlds[1].name.as_str()), (Scr::Select, "Alpha"));
        tap(&mut m, "Rename", sz);
        m.name = "Gamma ".into();
        tap(&mut m, "Rename", sz);
        assert_eq!((m.scr, m.sel, folders(&m)), (Scr::Select, Some(1), vec!["b".to_string(), "a".to_string()]));
        let a = &m.worlds[1];
        assert_eq!((a.name.as_str(), a.seed, a.ty, a.pack), ("Gamma", 5, WorldType::Tectonic, true));
        assert!(saves(&root).join("a").is_dir() && !saves(&root).join("Gamma").exists(), "the folder keeps its name");
    }

    #[test]
    fn old_saves_move_into_the_list() {
        let root = tmp("legacy");
        std::fs::create_dir_all(root.join("world-cafebabe")).unwrap();
        std::fs::create_dir_all(root.join("world-cafebabe-rtg")).unwrap();
        let l = list(&root);
        assert_eq!((l.len(), l[0].ty, l[0].seed, l[0].pack), (1, WorldType::Vanilla, LEGACY_SEED, false), "the Realistic save is left alone, old worlds keep the old panel");
        assert!(root.join("world-cafebabe-rtg").is_dir());
    }
}
