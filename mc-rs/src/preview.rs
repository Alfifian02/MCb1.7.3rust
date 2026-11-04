//! World Preview: a pannable, zoomable map of the world the Create World screen is about to make. Port of the idea of World Preview Prime
//! (`PreviewDisplay`, `WorkManager`, `PreviewSection`, `RenderSettings`), re-done for a phone and for this generator. UNVERIFIED: not compiled, not run.
//!
//! What it keeps from the mod: samples are made in the background (`WorkManager`) in square tiles (`PreviewSection`) that are cached and drawn
//! nearest-first; the map shows biome colours, a height map and the biome list with a highlight (`BiomesList`); a tap reads out a point.
//! What differs (and why):
//! - The mod makes real chunks (`FullChunkWorkUnit`) and reads their heightmap. Here the height is the zero crossing of the density column
//!   (`OverworldGenerator::height_at`, Tectonic's own `surface`), so a sample costs a few noise reads instead of a chunk: a screenful is
//!   ready in well under a second on a phone. Caves, rivers, structures and populate are not part of it (`OverworldGenerator::preview`).
//! - Zoom is continuous and every zoom level has its own tile set (a sample every 4 .. 512 blocks); while a tile of the wanted level is
//!   missing the coarser tile that covers it is drawn, so the map never goes blank.
//! - Two extra maps, Temperature and Humidity, show what the Natural Temperature settings do to the climate (the mod has none).
//! - Touch: one finger pans, two fingers pinch-zoom (`TouchUi::take_pinch`), a double tap zooms in, a tap reads a point out, big
//!   buttons, a legend that scrolls by dragging. No hover, no keyboard.
//! - The HUD pass holds only `gles::HUD_QUADS` quads, so the map is run-length merged and the cell size is chosen to keep it within ~4300
//!   quads; colours are quantized so neighbours merge.
//!
//! Second pass (2026-11-04): rivers and lakes from the game's own `Hydro` (`Region::water_at`), the Structory starts of a Blooming world
//! (`structory::starts`, with a legend filter), a seed that can be typed or re-rolled here, fling and eased zoom, contour lines, a chunk
//! grid, a live read-out of the middle of the map and, from the game menu, the same map of the world being played with the player and
//! the explored area (`Explored`, `<world>/explored`).
//!
//! Everything the Create World screen can change is read: the seed (the random one is fixed when the screen opens, `Menu::rand_seed`),
//! the world type, and every field of `NaturalSettings` (the generator and the chunk manager are made from them, after `natural::set`).
//! Rivers follow the river settings too (`hydro::Settings`: `riverDrainageSize` sets the plates, outlet biomes end rivers; the Create World map uses the
//! defaults a new world gets, the in-game map the world's own); the preview's `Hydro` is keyed by them (`Hydro::of_preview`) and does not prefetch plates.
//! The world type can also be cycled here, and `Settings` opens the World-Gen Settings screen and comes back here with the map rebuilt.

use crate::input::touch_ui::ScreenEv;
use crate::menu::{rgb, scaled_gs, tw};
use crate::options::button;
use crate::render::hud::{HudPipeline, HudVertex};
use crate::ui;
use crate::world::biome::Biome;
use crate::world::gen::hydro::{self, Region};
use crate::world::gen::natural::{self, NaturalSettings};
use crate::world::gen::structory;
use crate::world::gen::overworld::{OverworldGenerator, WorldType, SEA_LEVEL};
use std::collections::{HashMap, HashSet};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

type R = [f32; 4];
/// (level, tile x, tile z): level L samples every `4 << L` blocks.
type Key = (u8, i32, i32);

/// Samples per tile side.
const TS: i32 = 32;
const MAX_LVL: u8 = 7;
const WORKERS: usize = 2;
/// Tiles asked for and not yet delivered, at most.
const IN_FLIGHT: usize = WORKERS * 3;
const MAX_TILES: usize = 700;
/// Quads the map may use (the HUD pass holds 6144 for everything); less while the contour lines need room.
const MAP_QUADS: f32 = 4300.0;
const MAP_QUADS_LINES: f32 = 3200.0;
const BPG_MIN: f32 = 0.5;
const BPG_MAX: f32 = 160.0;
/// Height of the two bars (two rows of buttons on top, one below): the map lies between them.
const TOP: f32 = 46.0;
const BOT: f32 = 24.0;
/// Structure starts are asked for in squares of this many blocks, and only while zoomed in at most this far (blocks per GUI unit).
const MARK_SPAN: i32 = 1024;
const MARK_ZOOM: f32 = 48.0;
/// Contour lines every this many blocks (so one lies on the sea level), and the most quads they may use.
const CONTOUR: i32 = 8;
const CONTOUR_QUADS: usize = 1100;
const LEG_ROW: f32 = 12.0;

const ALL: [Biome; 19] = [
    Biome::Rainforest,
    Biome::Swamp,
    Biome::SeasonalForest,
    Biome::Forest,
    Biome::Savanna,
    Biome::Shrubland,
    Biome::Taiga,
    Biome::Desert,
    Biome::Plains,
    Biome::Tundra,
    Biome::Ocean,
    Biome::Beach,
    Biome::Mountains,
    Biome::AutumnalForest,
    Biome::Chaparral,
    Biome::Marsh,
    Biome::OakWoodland,
    Biome::WetWoodland,
    Biome::SnowyBirchGrove,
];
/// The first `BASE` of `ALL` exist in every world type; the rest only in Blooming.
const BASE: usize = 13;

/// Name and map colour (`r << 16 | g << 8 | b`) of a biome.
fn info(b: Biome) -> (&'static str, u32) {
    match b {
        Biome::Rainforest => ("Rainforest", 0x0B6A1D),
        Biome::Swamp => ("Swamp", 0x4C6B3A),
        Biome::SeasonalForest => ("Seasonal Forest", 0xB06F2A),
        Biome::Forest => ("Forest", 0x2F8A2F),
        Biome::Savanna => ("Savanna", 0xBDB25F),
        Biome::Shrubland => ("Shrubland", 0x8A9B5A),
        Biome::Taiga => ("Taiga", 0x3D6B57),
        Biome::Desert => ("Desert", 0xE3C971),
        Biome::Plains => ("Plains", 0x8BC34A),
        Biome::Tundra => ("Tundra", 0xD5E8EA),
        Biome::Ocean => ("Ocean", 0x2457C7),
        Biome::Beach => ("Beach", 0xF1E3A1),
        Biome::Mountains => ("Mountains", 0x8D8D8D),
        Biome::AutumnalForest => ("Autumnal Forest", 0xC2561F),
        Biome::Chaparral => ("Chaparral", 0xA8864A),
        Biome::Marsh => ("Marsh", 0x3F8F7A),
        Biome::OakWoodland => ("Oak Woodland", 0x5FA83A),
        Biome::WetWoodland => ("Wet Woodland", 0x0F8F5C),
        Biome::SnowyBirchGrove => ("Snowy Birch Grove", 0xE6C9D6),
    }
}

fn idx(b: Biome) -> u8 {
    ALL.iter().position(|&x| x == b).unwrap_or(0) as u8
}

fn ty_name(t: WorldType) -> &'static str {
    match t {
        WorldType::Vanilla => "Vanilla",
        WorldType::Tectonic => "Tectonic",
        WorldType::Blooming => "Blooming",
    }
}

// ---- colours ----

fn mix(a: u32, b: u32, t: f32) -> u32 {
    let ch = |s: u32| {
        let (x, y) = ((a >> s & 255) as f32, (b >> s & 255) as f32);
        (x + (y - x) * t).round().clamp(0.0, 255.0) as u32
    };
    ch(16) << 16 | ch(8) << 8 | ch(0)
}

fn lit(c: u32, k: f32) -> u32 {
    let ch = |s: u32| ((c >> s & 255) as f32 * k).round().clamp(0.0, 255.0) as u32;
    ch(16) << 16 | ch(8) << 8 | ch(0)
}

/// Even steps along `stops`, `t` in 0..1.
fn ramp(stops: &[u32], t: f32) -> u32 {
    let n = stops.len() - 1;
    let f = t.clamp(0.0, 1.0) * n as f32;
    let i = (f as usize).min(n - 1);
    mix(stops[i], stops[i + 1], f - i as f32)
}

const TEMP: [u32; 6] = [0x2A46D6, 0x3FB7E8, 0x45C46A, 0xE6D84A, 0xE8892A, 0xD02A1F];
const HUMID: [u32; 5] = [0xD8B86A, 0xA8C46A, 0x4CAA5A, 0x2A8A9A, 0x2A4FB0];
/// Height map: (y, colour); the sea is at `SEA_LEVEL`.
const HYPSO: [(i32, u32); 9] =
    [(0, 0x081A50), (40, 0x1A47A0), (63, 0x3F80E0), (64, 0xE6D890), (67, 0x58AA3C), (84, 0x9AA83C), (100, 0x8A6A45), (114, 0xA8A8A8), (127, 0xFFFFFF)];

fn hypso(y: i32) -> u32 {
    let y = y.clamp(0, 127);
    let i = HYPSO.iter().rposition(|&(h, _)| h <= y).unwrap_or(0).min(HYPSO.len() - 2);
    let ((y0, c0), (y1, c1)) = (HYPSO[i], HYPSO[i + 1]);
    mix(c0, c1, (y - y0) as f32 / (y1 - y0) as f32)
}

// ---- tiles and the workers ----

/// `TS` x `TS` samples, x outer and z inner: biome index, surface height, temperature and humidity (0..255), water (0 none, 1 river, 2 lake).
struct Tile {
    b: Vec<u8>,
    h: Vec<i16>,
    t: Vec<u8>,
    w: Vec<u8>,
    r: Vec<u8>,
}

/// A structure start: (x, z, structure index).
type Mark = (i32, i32, u8);

enum Job {
    Tile(Key),
    /// Structure starts of the square `MARK_SPAN` blocks wide with this index.
    Marks(i32, i32),
}

enum Res {
    Tile(Key, Tile),
    Marks((i32, i32), Vec<Mark>),
}

type Cache = HashMap<(i32, i32), Arc<Region>>;

fn make(g: &mut OverworldGenerator, cm: &crate::world::gen::chunk_manager::WorldChunkManager, cache: &mut Cache, (lvl, tx, tz): Key) -> Tile {
    let step = 4i32 << lvl;
    let n = (TS * TS) as usize;
    let mut t = Tile { b: Vec::with_capacity(n), h: Vec::with_capacity(n), t: Vec::with_capacity(n), w: Vec::with_capacity(n), r: Vec::with_capacity(n) };
    for i in 0..TS {
        for j in 0..TS {
            // The centre of the cell, so a sample is what lies in the middle of the square it paints.
            let (bx, bz) = ((tx * TS + i) * step + step / 2, (tz * TS + j) * step + step / 2);
            let (b, temp, hum) = cm.sample_at(bx, bz);
            let y = g.height_at(bx, bz, cm);
            let mut water = g.water_at(bx, bz, cm, cache, y);
            if water == 0 && lvl >= 2 {
                // A cell this big would miss a river that runs beside its middle: look at the 8 points round it too.
                let q = step / 3;
                for (ox, oz) in [(-q, -q), (0, -q), (q, -q), (-q, 0), (q, 0), (-q, q), (0, q), (q, q)] {
                    water = g.water_at(bx + ox, bz + oz, cm, cache, y);
                    if water != 0 {
                        break;
                    }
                }
            }
            t.b.push(idx(b));
            t.h.push(y.clamp(0, 255) as i16);
            t.t.push((temp.clamp(0.0, 1.0) * 255.0) as u8);
            t.w.push((hum.clamp(0.0, 1.0) * 255.0) as u8);
            t.r.push(water);
        }
    }
    t
}

/// One worker: builds its own generator and chunk manager (the chunk manager reads `natural::current` when made, so `natural::set` came
/// first), then does the jobs it is given until the sender is dropped.
fn spawn(seed: i64, ty: WorldType, rx: Arc<Mutex<mpsc::Receiver<Job>>>, tx: mpsc::Sender<Res>) {
    let _ = std::thread::Builder::new().name("world-preview".into()).spawn(move || {
        let mut g = OverworldGenerator::preview(ty, seed);
        let cm = g.manager();
        let mut cache = Cache::new();
        loop {
            let job = match rx.lock() {
                Ok(r) => match r.recv() {
                    Ok(j) => j,
                    Err(_) => break,
                },
                Err(_) => break,
            };
            let res = match job {
                Job::Tile(k) => Res::Tile(k, make(&mut g, &cm, &mut cache, k)),
                Job::Marks(rx, rz) => {
                    let (x0, z0) = (rx * MARK_SPAN, rz * MARK_SPAN);
                    Res::Marks((rx, rz), structory::starts(seed, x0, z0, x0 + MARK_SPAN - 1, z0 + MARK_SPAN - 1, &mut |x, z| cm.sample_at(x, z).0))
                }
            };
            if tx.send(res).is_err() {
                break;
            }
        }
    });
}

/// The chunks the player has been near (`<world>/explored`): level `k` holds them in squares of `2^k` chunks, so a coarse map cell is one
/// look-up. Only level 0 is saved.
#[derive(Clone, Default)]
pub struct Explored {
    lv: Vec<HashSet<(i32, i32)>>,
    last: Option<(i32, i32)>,
    dirty: bool,
}

impl Explored {
    fn ensure(&mut self) {
        if self.lv.len() <= usize::from(MAX_LVL) {
            self.lv.resize_with(usize::from(MAX_LVL) + 1, HashSet::new);
        }
    }

    pub fn mark(&mut self, cx: i32, cz: i32) {
        self.ensure();
        for k in 0..=MAX_LVL {
            let new = self.lv[usize::from(k)].insert((cx >> k, cz >> k));
            self.dirty |= new && k == 0;
        }
    }

    /// The player is in chunk (cx, cz): everything within `r` chunks counts as seen. Cheap while the chunk does not change.
    pub fn visit(&mut self, cx: i32, cz: i32, r: i32) {
        if self.last == Some((cx, cz)) {
            return;
        }
        self.last = Some((cx, cz));
        for dx in -r..=r {
            for dz in -r..=r {
                if dx * dx + dz * dz <= r * r {
                    self.mark(cx + dx, cz + dz);
                }
            }
        }
    }

    /// Whether block (bx, bz)'s square of `2^k` chunks has been seen.
    pub fn seen(&self, bx: i32, bz: i32, k: u8) -> bool {
        self.lv.get(usize::from(k)).is_some_and(|s| s.contains(&((bx >> 4) >> k, (bz >> 4) >> k)))
    }

    /// True once after a chunk was added: the caller writes the file then.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    pub fn encode(&self) -> Vec<u8> {
        self.lv.first().map_or_else(Vec::new, |s| s.iter().flat_map(|&(x, z)| x.to_le_bytes().into_iter().chain(z.to_le_bytes())).collect())
    }

    pub fn decode(b: &[u8]) -> Self {
        let mut e = Self::default();
        for c in b.chunks_exact(8) {
            e.mark(i32::from_le_bytes([c[0], c[1], c[2], c[3]]), i32::from_le_bytes([c[4], c[5], c[6], c[7]]));
        }
        e.dirty = false;
        e
    }

    pub fn load(path: &std::path::Path) -> Self {
        std::fs::read(path).map(|b| Self::decode(&b)).unwrap_or_default()
    }
}

/// What the game menu hands the preview: where the player is (overworld block coordinates) and what the player has explored.
pub struct InGame {
    pub player: (f64, f64),
    pub explored: Explored,
}

// ---- the screen ----

/// What the view keeps between visits (the Create screen hands it back when the preview is opened again).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub cx: f64,
    pub cz: f64,
    pub bpg: f32,
    pub mode: u8,
}

impl Default for View {
    fn default() -> Self {
        Self { cx: 0.0, cz: 0.0, bpg: 3.0, mode: 0 }
    }
}

const MODES: [&str; 5] = ["Biomes", "Relief", "Height", "Temperature", "Humidity"];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PvOut {
    None,
    /// Back to Create World (or to the game).
    Close,
    /// The World-Gen Settings screen (the menu comes back here when it is done).
    Settings,
    /// The seed button was tapped: the menu opens the keyboard on the seed field.
    EditSeed,
    /// The dice: the menu draws a new random seed.
    Random,
}

#[derive(Clone, Copy, PartialEq)]
enum Grab {
    Map,
    Legend,
    Bar,
}

/// A row of the legend.
#[derive(Clone, Copy, PartialEq)]
enum LegRow {
    Biome(Biome),
    /// The "Structures" heading: a tap shows or hides the markers.
    Head,
    Struct(u8),
}

struct Lay {
    s: f32,
    gw: f32,
    gh: f32,
    top: f32,
    bottom: f32,
    done: R,
    mode: R,
    ty: R,
    set: R,
    seed: R,
    rand: R,
    fog: R,
    cont: R,
    grid: R,
    live: R,
    leg: R,
    home: R,
    zout: R,
    zin: R,
    panel: R,
}

fn hit(r: R, x: f32, y: f32) -> bool {
    x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]
}

const ST_COLORS: [u32; 16] =
    [0xFF5555, 0xFFAA00, 0xFFFF55, 0x55FF55, 0x55FFFF, 0x5555FF, 0xFF55FF, 0xFFFFFF, 0xAA0000, 0x00AAAA, 0xAA00AA, 0xAAAAAA, 0x00AA00, 0xFFCC99, 0x99CCFF, 0xCC99FF];

/// The name of a structure: its number and the first biomes it can start in (`structory.bin` keeps no names).
fn st_label(st: u8) -> String {
    let m = structory::mask(usize::from(st));
    let names: Vec<&str> = ALL.iter().enumerate().filter(|(i, _)| m >> i & 1 != 0).map(|(_, b)| info(*b).0).collect();
    match names.len() {
        0 => format!("S{st}"),
        1..=2 => format!("S{st} {}", names.join("/")),
        n => format!("S{st} {}/{} +{}", names[0], names[1], n - 2),
    }
}

pub struct Preview {
    pub seed: i64,
    pub ty: WorldType,
    nat: NaturalSettings,
    cx: f64,
    cz: f64,
    /// Blocks per GUI unit: the zoom.
    bpg: f32,
    mode: u8,
    tiles: HashMap<Key, Arc<Tile>>,
    pending: HashSet<Key>,
    marks: HashMap<(i32, i32), Vec<Mark>>,
    pending_marks: HashSet<(i32, i32)>,
    req: Option<mpsc::Sender<Job>>,
    res: mpsc::Receiver<Res>,
    legend: bool,
    scroll: f32,
    hl: Option<u8>,
    /// Structure markers shown, and the one structure the legend picked (the others hide).
    show_struct: bool,
    sh: Option<u8>,
    /// Layers: contour lines, chunk grid, the live read-out of the map's middle.
    contours: bool,
    grid: bool,
    live: bool,
    /// Hide what the player has not explored (in the game only).
    fog: bool,
    ingame: Option<InGame>,
    /// The point a tap (or, live, the middle) reads out (block coordinates).
    probe: Option<(f64, f64)>,
    press: Option<(f32, f32)>,
    press_c: (f64, f64),
    press_scroll: f32,
    grab: Grab,
    moved: bool,
    pinched: bool,
    last_tap: Option<(Instant, (f32, f32))>,
    finger: Option<(f32, f32)>,
    /// The finger's recent track (for the fling), the fling's speed (GUI units per second), the zoom being eased to (blocks per GUI unit,
    /// and the GUI point it stays on), and the time of the last frame.
    track: Vec<((f32, f32), Instant)>,
    vel: (f32, f32),
    anim: Option<(f32, f32, f32)>,
    frame: Instant,
    /// The level drawn last frame (for the read-out).
    lvl: u8,
}

impl Preview {
    pub fn new(seed: i64, ty: WorldType, nat: &NaturalSettings, v: View) -> Self {
        Self::build(seed, ty, nat, v, None)
    }

    /// The preview of the world being played: read only (seed and type stay), with the player and the explored area.
    pub fn new_ingame(seed: i64, ty: WorldType, nat: &NaturalSettings, v: View, ig: InGame) -> Self {
        Self::build(seed, ty, nat, v, Some(ig))
    }

    fn build(seed: i64, ty: WorldType, nat: &NaturalSettings, v: View, ingame: Option<InGame>) -> Self {
        let (_, res) = mpsc::channel();
        let mut p = Self {
            seed,
            ty,
            nat: nat.clone(),
            cx: v.cx,
            cz: v.cz,
            bpg: v.bpg.clamp(BPG_MIN, BPG_MAX),
            mode: v.mode % MODES.len() as u8,
            tiles: HashMap::new(),
            pending: HashSet::new(),
            marks: HashMap::new(),
            pending_marks: HashSet::new(),
            req: None,
            res,
            legend: false,
            scroll: 0.0,
            hl: None,
            show_struct: true,
            sh: None,
            contours: false,
            grid: false,
            live: false,
            fog: ingame.is_some(),
            ingame,
            probe: None,
            press: None,
            press_c: (0.0, 0.0),
            press_scroll: 0.0,
            grab: Grab::Map,
            moved: false,
            pinched: false,
            last_tap: None,
            finger: None,
            track: Vec::new(),
            vel: (0.0, 0.0),
            anim: None,
            frame: Instant::now(),
            lvl: 0,
        };
        p.reset();
        p
    }

    pub fn view(&self) -> View {
        View { cx: self.cx, cz: self.cz, bpg: self.bpg, mode: self.mode }
    }

    /// A new seed (typed or re-rolled on the Create screen): the map is made again. The same seed changes nothing.
    pub fn set_seed(&mut self, seed: i64) {
        if seed != self.seed {
            self.seed = seed;
            self.reset();
        }
    }

    /// Throws every tile away and starts workers for the current seed, world type and Natural Temperature settings. The settings are
    /// handed to `natural` first: the workers' chunk managers take them when they are made (switch off, `natural` holds nothing). In the
    /// game they are already there (the world's own) and stay.
    fn reset(&mut self) {
        if self.ingame.is_none() {
            natural::set(Some(&self.nat));
            // A world made from the Create World screen has no `hydro.cfg` yet, so it gets the river defaults (plate size, outlet biomes): the map
            // shows those, not what the world played last left in `hydro`. In the game they are the world's own and stay.
            hydro::configure(hydro::Settings::default());
        }
        self.tiles.clear();
        self.pending.clear();
        self.marks.clear();
        self.pending_marks.clear();
        let (tx, rx) = mpsc::channel();
        let (qtx, qrx) = mpsc::channel();
        let qrx = Arc::new(Mutex::new(qrx));
        for _ in 0..WORKERS {
            spawn(self.seed, self.ty, qrx.clone(), tx.clone());
        }
        self.req = Some(qtx); // the old sender drops here: the old workers stop
        self.res = rx;
    }

    /// Whether this world has structures to mark (Blooming, with `structory.bin`).
    fn has_structs(&self) -> bool {
        self.ty == WorldType::Blooming && structory::available()
    }

    /// The legend's rows: the biomes this world type can show, then the structures.
    fn rows(&self) -> Vec<LegRow> {
        let mut v: Vec<LegRow> = if self.ty == WorldType::Blooming { &ALL[..] } else { &ALL[..BASE] }.iter().map(|&b| LegRow::Biome(b)).collect();
        if self.has_structs() {
            v.push(LegRow::Head);
            v.extend((0..structory::count().min(ST_COLORS.len())).map(|n| LegRow::Struct(n as u8)));
        }
        v
    }

    fn lay(&self, w: f32, h: f32, gs: u8) -> Lay {
        let (s, gw, gh) = scaled_gs(w, h, gs);
        let (top, bottom) = (TOP, gh - BOT);
        let ph = (self.rows().len() as f32 * LEG_ROW + 4.0).min(bottom - top - 16.0);
        let none = [0.0; 4];
        // The second row: in Create World the seed and the dice, in the game the way back to the player; then the layers.
        let ing = self.ingame.is_some();
        let (seed, rand, fog, cont, grid, live) = if ing {
            ([204.0, 25.0, (gw - 206.0).max(0.0), 20.0], [2.0, 25.0, 34.0, 20.0], [38.0, 25.0, 44.0, 20.0], [84.0, 25.0, 40.0, 20.0], [126.0, 25.0, 36.0, 20.0], [164.0, 25.0, 36.0, 20.0])
        } else {
            ([2.0, 25.0, 126.0, 20.0], [130.0, 25.0, 36.0, 20.0], none, [168.0, 25.0, 40.0, 20.0], [210.0, 25.0, 36.0, 20.0], [248.0, 25.0, (gw - 250.0).clamp(30.0, 70.0), 20.0])
        };
        Lay {
            s,
            gw,
            gh,
            top,
            bottom,
            done: [2.0, 2.0, 46.0, 20.0],
            mode: [50.0, 2.0, 96.0, 20.0],
            ty: if ing { none } else { [148.0, 2.0, 96.0, 20.0] },
            set: if ing { none } else { [246.0, 2.0, (gw - 248.0).clamp(40.0, 80.0), 20.0] },
            seed,
            rand,
            fog,
            cont,
            grid,
            live,
            leg: [2.0, gh - 22.0, 58.0, 20.0],
            home: [62.0, gh - 22.0, 50.0, 20.0],
            zout: [gw - 58.0, gh - 22.0, 28.0, 20.0],
            zin: [gw - 28.0, gh - 22.0, 26.0, 20.0],
            panel: [gw - 112.0, top + 2.0, 110.0, ph],
        }
    }

    /// What the pad's button hints keep off (px).
    pub fn avoid(&self, w: f32, h: f32, gs: u8) -> Vec<R> {
        let l = self.lay(w, h, gs);
        [[0.0, 0.0, l.gw, l.top], [0.0, l.bottom, l.gw, BOT]].into_iter().map(|r| r.map(|u| u * l.s)).collect()
    }

    fn max_scroll(&self, l: &Lay) -> f32 {
        (self.rows().len() as f32 * LEG_ROW + 4.0 - l.panel[3]).max(0.0)
    }

    /// Blocks per sample at level `lvl`.
    fn step(lvl: u8) -> f64 {
        f64::from(4i32 << lvl)
    }

    /// The level whose samples paint cells of `cs` GUI units or more at this zoom.
    fn level(&self, cs: f32) -> u8 {
        ((self.bpg * cs / 4.0).max(1.0).log2().ceil() as i32).clamp(0, i32::from(MAX_LVL)) as u8
    }

    /// Size of a painted cell in GUI units: the smallest that keeps the map within its quads (fewer while the contour lines are on).
    fn cell(&self, l: &Lay) -> f32 {
        ((l.gw * (l.bottom - l.top)) / if self.contours { MAP_QUADS_LINES } else { MAP_QUADS }).sqrt().max(3.0)
    }

    fn zoom_about(&mut self, l: &Lay, k: f32, gx: f32, gy: f32) {
        let (hx, hy) = (l.gw / 2.0, (l.top + l.bottom) / 2.0);
        let (dx, dy) = (f64::from(gx - hx), f64::from(gy - hy));
        let (wx, wz) = (self.cx + dx * f64::from(self.bpg), self.cz + dy * f64::from(self.bpg));
        self.bpg = (self.bpg / k).clamp(BPG_MIN, BPG_MAX);
        self.cx = wx - dx * f64::from(self.bpg);
        self.cz = wz - dy * f64::from(self.bpg);
    }

    /// Zoom by `k` about a GUI point, eased over a few frames (`draw` follows `anim`). Taps on the buttons add up.
    fn zoom_to(&mut self, k: f32, gx: f32, gy: f32) {
        let base = self.anim.map_or(self.bpg, |a| a.0);
        self.anim = Some(((base / k).clamp(BPG_MIN, BPG_MAX), gx, gy));
    }

    /// Two fingers moved: `k` is the ratio of their distance now to before, (mx, my) px their midpoint (`TouchUi::take_pinch`).
    pub fn pinch(&mut self, k: f32, mx: f32, my: f32, w: f32, h: f32, gs: u8) {
        let l = self.lay(w, h, gs);
        (self.pinched, self.press, self.moved, self.anim, self.vel) = (true, None, false, None, (0.0, 0.0));
        if k.is_finite() && k > 0.0 {
            self.zoom_about(&l, k, mx / l.s, my / l.s);
        }
    }

    // ---- input ----

    pub fn event(&mut self, ev: ScreenEv, w: f32, h: f32, gs: u8) -> PvOut {
        let l = self.lay(w, h, gs);
        match ev {
            ScreenEv::Down(x, y) => {
                let p = (x / l.s, y / l.s);
                self.finger = Some(p);
                self.press = Some(p);
                (self.press_c, self.press_scroll, self.moved, self.pinched) = ((self.cx, self.cz), self.scroll, false, false);
                (self.vel, self.anim) = ((0.0, 0.0), None);
                self.track.clear();
                self.grab = if p.1 < l.top || p.1 >= l.bottom {
                    Grab::Bar
                } else if self.legend && hit(l.panel, p.0, p.1) {
                    Grab::Legend
                } else {
                    Grab::Map
                };
            }
            ScreenEv::Move(x, y) => {
                let p = (x / l.s, y / l.s);
                self.finger = Some(p);
                let Some(d) = self.press else { return PvOut::None };
                if self.pinched {
                    return PvOut::None;
                }
                if (p.0 - d.0).hypot(p.1 - d.1) > 5.0 {
                    self.moved = true;
                }
                if self.moved {
                    match self.grab {
                        Grab::Map => {
                            self.cx = self.press_c.0 - f64::from(p.0 - d.0) * f64::from(self.bpg);
                            self.cz = self.press_c.1 - f64::from(p.1 - d.1) * f64::from(self.bpg);
                            self.track.push((p, Instant::now()));
                            if self.track.len() > 8 {
                                self.track.remove(0);
                            }
                        }
                        Grab::Legend => self.scroll = (self.press_scroll - (p.1 - d.1)).clamp(0.0, self.max_scroll(&l)),
                        Grab::Bar => {}
                    }
                }
            }
            ScreenEv::Up(x, y) => {
                self.finger = None;
                let down = self.press.take();
                let moved = std::mem::take(&mut self.moved);
                if moved && !self.pinched && self.grab == Grab::Map {
                    self.fling();
                }
                if self.pinched || moved || down.is_none() {
                    return PvOut::None;
                }
                return self.tap((x / l.s, y / l.s), &l);
            }
        }
        PvOut::None
    }

    /// The finger lifted while moving: the map keeps going at the speed the finger had over the last 100 ms (if it was still moving).
    fn fling(&mut self) {
        let now = Instant::now();
        let recent: Vec<((f32, f32), Instant)> = self.track.iter().copied().filter(|(_, t)| now.duration_since(*t) < Duration::from_millis(100)).collect();
        if let (Some(&(a, ta)), Some(&(b, tb))) = (recent.first(), recent.last()) {
            let dt = tb.duration_since(ta).as_secs_f32();
            if dt > 0.01 {
                let v = ((b.0 - a.0) / dt, (b.1 - a.1) / dt);
                let sp = v.0.hypot(v.1);
                if sp > 60.0 {
                    let k = sp.min(1200.0) / sp;
                    self.vel = (v.0 * k, v.1 * k);
                }
            }
        }
        self.track.clear();
    }

    fn tap(&mut self, p: (f32, f32), l: &Lay) -> PvOut {
        let (hx, hy) = (l.gw / 2.0, (l.top + l.bottom) / 2.0);
        let ing = self.ingame.is_some();
        if hit(l.done, p.0, p.1) {
            return PvOut::Close;
        } else if hit(l.set, p.0, p.1) {
            return PvOut::Settings;
        } else if hit(l.mode, p.0, p.1) {
            self.mode = (self.mode + 1) % MODES.len() as u8;
        } else if hit(l.ty, p.0, p.1) {
            self.ty = self.ty.next();
            self.hl = None;
            (self.sh, self.scroll) = (None, 0.0);
            self.reset();
        } else if !ing && hit(l.seed, p.0, p.1) {
            return PvOut::EditSeed;
        } else if hit(l.rand, p.0, p.1) {
            if let Some(ig) = &self.ingame {
                (self.cx, self.cz) = ig.player; // "Me"
            } else {
                return PvOut::Random;
            }
        } else if hit(l.fog, p.0, p.1) {
            self.fog = !self.fog;
        } else if hit(l.cont, p.0, p.1) {
            self.contours = !self.contours;
        } else if hit(l.grid, p.0, p.1) {
            self.grid = !self.grid;
        } else if hit(l.live, p.0, p.1) {
            self.live = !self.live;
            if !self.live {
                self.probe = None;
            }
        } else if hit(l.leg, p.0, p.1) {
            self.legend = !self.legend;
        } else if hit(l.home, p.0, p.1) {
            (self.cx, self.cz) = (0.0, 0.0);
        } else if hit(l.zout, p.0, p.1) {
            self.zoom_to(0.5, hx, hy);
        } else if hit(l.zin, p.0, p.1) {
            self.zoom_to(2.0, hx, hy);
        } else if self.legend && hit(l.panel, p.0, p.1) {
            let row = ((p.1 - l.panel[1] - 2.0 + self.scroll) / LEG_ROW).floor();
            let rows = self.rows();
            if row >= 0.0 && (row as usize) < rows.len() {
                match rows[row as usize] {
                    LegRow::Biome(b) => {
                        let b = idx(b);
                        self.hl = if self.hl == Some(b) { None } else { Some(b) };
                    }
                    LegRow::Head => self.show_struct = !self.show_struct,
                    LegRow::Struct(n) => {
                        self.show_struct = true;
                        self.sh = if self.sh == Some(n) { None } else { Some(n) };
                    }
                }
            }
        } else if p.1 >= l.top && p.1 < l.bottom && !self.live {
            let twice = self.last_tap.is_some_and(|(t, q)| t.elapsed() < Duration::from_millis(350) && (q.0 - p.0).hypot(q.1 - p.1) < 14.0);
            self.last_tap = Some((Instant::now(), p));
            self.probe = Some((self.cx + f64::from(p.0 - hx) * f64::from(self.bpg), self.cz + f64::from(p.1 - hy) * f64::from(self.bpg)));
            if twice {
                self.zoom_to(2.0, p.0, p.1);
                self.last_tap = None;
            }
        } else if p.1 >= l.top && p.1 < l.bottom {
            // Live read-out follows the middle: a double tap still zooms.
            let twice = self.last_tap.is_some_and(|(t, q)| t.elapsed() < Duration::from_millis(350) && (q.0 - p.0).hypot(q.1 - p.1) < 14.0);
            self.last_tap = Some((Instant::now(), p));
            if twice {
                self.zoom_to(2.0, p.0, p.1);
                self.last_tap = None;
            }
        }
        PvOut::None
    }

    // ---- tiles ----

    /// The sample (i, j) of level `lvl`, or the one of the nearest coarser level that has it.
    fn sample(&self, lvl: u8, i: i32, j: i32) -> Option<(&Tile, usize)> {
        for l in lvl..=MAX_LVL {
            let sh = u32::from(l - lvl);
            let (i, j) = (i >> sh, j >> sh);
            if let Some(t) = self.tiles.get(&(l, i.div_euclid(TS), j.div_euclid(TS))) {
                return Some((t, (i.rem_euclid(TS) * TS + j.rem_euclid(TS)) as usize));
            }
        }
        None
    }

    /// Takes in what the workers made, drops the tiles furthest away when there are too many, and asks for the missing ones of the view:
    /// the coarse level first (cheap, so the map is never empty), then the wanted one, each nearest the middle first; then the structure
    /// starts of the squares in view.
    fn update(&mut self, l: &Lay, lvl: u8) {
        while let Ok(r) = self.res.try_recv() {
            match r {
                Res::Tile(k, t) => {
                    self.pending.remove(&k);
                    self.tiles.insert(k, Arc::new(t));
                }
                Res::Marks(k, v) => {
                    self.pending_marks.remove(&k);
                    self.marks.insert(k, v);
                }
            }
        }
        if self.tiles.len() > MAX_TILES {
            let (cx, cz) = (self.cx, self.cz);
            let dist = |k: &Key| {
                let span = f64::from(TS) * Self::step(k.0);
                ((f64::from(k.1) + 0.5) * span - cx).hypot((f64::from(k.2) + 0.5) * span - cz) / span
            };
            let mut keys: Vec<Key> = self.tiles.keys().copied().collect();
            keys.sort_by(|a, b| dist(b).total_cmp(&dist(a)));
            for k in keys.into_iter().take(150) {
                self.tiles.remove(&k);
            }
        }
        let Some(req) = self.req.as_ref() else { return };
        let (hw, hh) = (f64::from(l.gw / 2.0 * self.bpg), f64::from((l.bottom - l.top) / 2.0 * self.bpg));
        let mut levels = vec![(lvl + 2).min(MAX_LVL), lvl];
        levels.dedup();
        'tiles: for lv in levels {
            let span = f64::from(TS) * Self::step(lv);
            let (x0, x1) = (((self.cx - hw) / span).floor() as i32, ((self.cx + hw) / span).floor() as i32);
            let (z0, z1) = (((self.cz - hh) / span).floor() as i32, ((self.cz + hh) / span).floor() as i32);
            let mut want: Vec<(f64, Key)> = Vec::new();
            for tx in x0..=x1 {
                for tz in z0..=z1 {
                    let k = (lv, tx, tz);
                    if !self.tiles.contains_key(&k) && !self.pending.contains(&k) {
                        let d = ((f64::from(tx) + 0.5) * span - self.cx).hypot((f64::from(tz) + 0.5) * span - self.cz);
                        want.push((d, k));
                    }
                }
            }
            want.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (_, k) in want {
                if self.pending.len() >= IN_FLIGHT {
                    break 'tiles;
                }
                if req.send(Job::Tile(k)).is_ok() {
                    self.pending.insert(k);
                }
            }
        }
        if self.has_structs() && self.show_struct && self.bpg <= MARK_ZOOM {
            let span = f64::from(MARK_SPAN);
            let (x0, x1) = (((self.cx - hw) / span).floor() as i32, ((self.cx + hw) / span).floor() as i32);
            let (z0, z1) = (((self.cz - hh) / span).floor() as i32, ((self.cz + hh) / span).floor() as i32);
            let mut want: Vec<(f64, (i32, i32))> = Vec::new();
            for rx in x0..=x1 {
                for rz in z0..=z1 {
                    if !self.marks.contains_key(&(rx, rz)) && !self.pending_marks.contains(&(rx, rz)) {
                        want.push((((f64::from(rx) + 0.5) * span - self.cx).hypot((f64::from(rz) + 0.5) * span - self.cz), (rx, rz)));
                    }
                }
            }
            want.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (_, k) in want {
                if self.pending_marks.len() >= 8 {
                    break;
                }
                if req.send(Job::Marks(k.0, k.1)).is_ok() {
                    self.pending_marks.insert(k);
                }
            }
        }
    }

    // ---- colours of a sample ----

    fn hgt(&self, lvl: u8, i: i32, j: i32) -> Option<i32> {
        self.sample(lvl, i, j).map(|(t, k)| i32::from(t.h[k]))
    }

    /// Light on a land cell from the north-west (the slope to its neighbours, in 7 steps) or the dark of the water above a deep bed.
    fn relief(&self, lvl: u8, i: i32, j: i32, y: i32) -> f32 {
        if y < SEA_LEVEL {
            return 1.0 - (((SEA_LEVEL - y) as f32 / 90.0).min(0.4) * 20.0).round() / 20.0;
        }
        let (Some(a), Some(b)) = (self.hgt(lvl, i - 1, j - 1), self.hgt(lvl, i + 1, j + 1)) else { return 1.0 };
        let d = (a - b) as f32 / Self::step(lvl) as f32 * 6.0;
        1.0 + 0.28 * ((d.clamp(-1.0, 1.0) * 3.0).round() / 3.0)
    }

    fn color(&self, lvl: u8, i: i32, j: i32) -> u32 {
        let Some((t, k)) = self.sample(lvl, i, j) else { return if (i + j) & 1 == 0 { 0x262626 } else { 0x2C2C2C } };
        let (b, y) = (t.b[k], i32::from(t.h[k]));
        let biome = info(ALL[b as usize % ALL.len()]).1;
        let mut c = match self.mode {
            0 => biome,
            1 => lit(biome, self.relief(lvl, i, j, y)),
            2 => lit(hypso(y & !1), if y < SEA_LEVEL { 1.0 } else { self.relief(lvl, i, j, y) }),
            3 => ramp(&TEMP, f32::from(t.t[k] >> 3) / 31.0),
            _ => ramp(&HUMID, f32::from(t.w[k] >> 3) / 31.0),
        };
        if self.mode < 3 {
            match t.r[k] {
                1 => c = mix(c, 0x3F7FE8, 0.85),
                2 => c = mix(c, 0x2F6FD0, 0.85),
                _ => {}
            }
            if self.hl.is_some_and(|h| h != b) {
                c = mix(c, 0x1A1A1A, 0.72);
            }
        }
        if let (true, Some(ig)) = (self.fog, &self.ingame) {
            let st = 4i32 << lvl;
            if !ig.explored.seen(i * st + st / 2, j * st + st / 2, lvl.saturating_sub(2)) {
                c = mix(c, 0x0A0A0A, 0.68);
            }
        }
        c
    }

    // ---- drawing ----

    pub fn draw(&mut self, v: &mut Vec<HudVertex>, w: f32, h: f32, gs: u8) {
        let l = self.lay(w, h, gs);
        let s = l.s;
        let now = Instant::now();
        let dt = now.duration_since(self.frame).as_secs_f32().min(0.1);
        self.frame = now;
        // A fling slows down; a zoom eases to its target (about the point it was asked at).
        if self.press.is_none() && self.vel != (0.0, 0.0) {
            self.cx -= f64::from(self.vel.0 * self.bpg * dt);
            self.cz -= f64::from(self.vel.1 * self.bpg * dt);
            let k = (-3.5 * dt).exp();
            self.vel = (self.vel.0 * k, self.vel.1 * k);
            if self.vel.0.hypot(self.vel.1) < 8.0 {
                self.vel = (0.0, 0.0);
            }
        }
        if let Some((t, gx, gy)) = self.anim {
            let gap = t.ln() - self.bpg.ln();
            let nb = if gap.abs() < 0.01 { t } else { (self.bpg.ln() + gap * (1.0 - (-12.0 * dt).exp())).exp() };
            self.zoom_about(&l, self.bpg / nb, gx, gy);
            if nb == t {
                self.anim = None;
            }
        }
        if self.live {
            self.probe = Some((self.cx, self.cz));
        }
        let lvl = self.level(self.cell(&l));
        self.lvl = lvl;
        self.update(&l, lvl);
        HudPipeline::push_quad(v, 0.0, 0.0, l.gw * s, l.gh * s, rgb(0x161616));
        self.draw_map(v, &l, lvl);
        if self.grid {
            self.draw_grid(v, &l);
        }
        self.draw_marks(v, &l);
        // The bars, over the map's edges.
        ui::panel(v, s, [0.0, 0.0, l.gw, l.top], 0.85);
        ui::panel(v, s, [0.0, l.bottom, l.gw, BOT], 0.85);
        let over = |r: R| self.finger.is_some_and(|(x, y)| hit(r, x, y));
        let ing = self.ingame.is_some();
        button(v, s, l.done, "Done", true, over(l.done), None);
        button(v, s, l.mode, &format!("View: {}", MODES[self.mode as usize]), true, over(l.mode), None);
        if ing {
            let t = format!("World: {}", ty_name(self.ty));
            HudPipeline::push_text(v, 150.0 * s, 8.0 * s, s, &ui::fit(&t, l.gw - 152.0), rgb(0xA0A0A0));
            button(v, s, l.rand, "Me", true, over(l.rand), None);
            button(v, s, l.fog, "Fog", true, over(l.fog) || self.fog, None);
            HudPipeline::push_text(v, (l.seed[0] + 2.0) * s, (l.seed[1] + 6.0) * s, s, &ui::fit(&format!("Seed {}", self.seed), l.seed[2] - 2.0), rgb(0xA0A0A0));
        } else {
            button(v, s, l.ty, &format!("Type: {}", ty_name(self.ty)), true, over(l.ty), None);
            button(v, s, l.set, "Settings", true, over(l.set), None);
            button(v, s, l.seed, &ui::fit(&format!("Seed: {}", self.seed), l.seed[2] - 8.0), true, over(l.seed), None);
            button(v, s, l.rand, "Rand", true, over(l.rand), None);
        }
        button(v, s, l.cont, "Cont", true, over(l.cont) || self.contours, None);
        button(v, s, l.grid, "Grid", true, over(l.grid) || self.grid, None);
        button(v, s, l.live, "Live", true, over(l.live) || self.live, None);
        button(v, s, l.leg, if self.legend { "Legend*" } else { "Legend" }, true, over(l.leg), None);
        button(v, s, l.home, "Spawn", true, over(l.home), None);
        button(v, s, l.zout, "-", true, over(l.zout), None);
        button(v, s, l.zin, "+", true, over(l.zin), None);
        let step = Self::step(lvl) as i64;
        let status = if self.pending.is_empty() {
            format!("X {}  Z {}   1 cell = {step}", self.cx.round() as i64, self.cz.round() as i64)
        } else {
            format!("Sampling... {}", self.pending.len())
        };
        let room = l.zout[0] - l.home[0] - l.home[2] - 8.0;
        let text = ui::fit(&status, room);
        HudPipeline::push_text(v, (l.home[0] + l.home[2] + 6.0 + (room - tw(&text)) / 2.0) * s, (l.bottom + 8.0) * s, s, &text, rgb(0xE0E0E0));
        if self.legend {
            self.draw_legend(v, &l);
        }
        if let Some(t) = self.readout(lvl) {
            let r = [2.0, l.bottom - 14.0, l.gw - 4.0, 12.0];
            HudPipeline::push_quad(v, r[0] * s, r[1] * s, r[2] * s, r[3] * s, [0.0, 0.0, 0.0, 0.6]);
            HudPipeline::push_text(v, (r[0] + 3.0) * s, (r[1] + 2.0) * s, s, &ui::fit(&t, r[2] - 6.0), rgb(0xFFFFFF));
        }
    }

    /// The painted cells: one quad per run of equal colour along a row, edges snapped to whole pixels so neighbours share them; with
    /// the contour lines on, a thin line on the edge between two cells that lie on different `CONTOUR` bands.
    fn draw_map(&self, v: &mut Vec<HudVertex>, l: &Lay, lvl: u8) {
        let s = f64::from(l.s);
        let (step, bpg) = (Self::step(lvl), f64::from(self.bpg));
        let (hx, hy) = (f64::from(l.gw) / 2.0, f64::from(l.top + l.bottom) / 2.0);
        let to_x = |i: i64| (hx + (i as f64 * step - self.cx) / bpg) * s;
        let to_y = |j: i64| (hy + (j as f64 * step - self.cz) / bpg) * s;
        let (pl, pr, pt, pb) = (0.0, (l.gw * l.s).round(), (l.top * l.s).round(), (l.bottom * l.s).round());
        let i0 = ((self.cx - hx * bpg) / step).floor() as i64;
        let i1 = ((self.cx + hx * bpg) / step).floor() as i64;
        let j0 = ((self.cz - (hy - f64::from(l.top)) * bpg) / step).floor() as i64;
        let j1 = ((self.cz + (f64::from(l.bottom) - hy) * bpg) / step).floor() as i64;
        let quad = |v: &mut Vec<HudVertex>, x0: f32, x1: f32, y0: f32, y1: f32, c: u32| {
            let (x0, x1) = (x0.max(pl), x1.min(pr));
            if x1 > x0 {
                HudPipeline::push_quad(v, x0, y0, x1 - x0, y1 - y0, rgb(c));
            }
        };
        for j in j0..=j1 {
            let (y0, y1) = ((to_y(j).round() as f32).max(pt), (to_y(j + 1).round() as f32).min(pb));
            if y1 <= y0 {
                continue;
            }
            let mut run: Option<(f32, u32)> = None;
            for i in i0..=i1 {
                let c = self.color(lvl, i as i32, j as i32);
                let x = to_x(i).round() as f32;
                match run {
                    Some((_, rc)) if rc == c => {}
                    Some((xs, rc)) => {
                        quad(v, xs, x, y0, y1, rc);
                        run = Some((x, c));
                    }
                    None => run = Some((x, c)),
                }
            }
            if let Some((xs, rc)) = run {
                quad(v, xs, to_x(i1 + 1).round() as f32, y0, y1, rc);
            }
        }
        if !self.contours {
            return;
        }
        let (mut left, line, w) = (CONTOUR_QUADS, [0.0, 0.0, 0.0, 0.4], (l.s * 0.7).max(1.0));
        let band = |y: i32| y.div_euclid(CONTOUR);
        'rows: for j in j0..=j1 {
            let (y0, y1) = ((to_y(j).round() as f32).max(pt), (to_y(j + 1).round() as f32).min(pb));
            if y1 <= y0 {
                continue;
            }
            for i in i0..=i1 {
                let Some(a) = self.hgt(lvl, i as i32, j as i32) else { continue };
                let x = to_x(i + 1).round() as f32;
                if x > pl && x < pr && self.hgt(lvl, i as i32 + 1, j as i32).is_some_and(|b| band(a) != band(b)) {
                    HudPipeline::push_quad(v, x - w / 2.0, y0, w, y1 - y0, line);
                    left -= 1;
                }
                if y1 < pb && self.hgt(lvl, i as i32, j as i32 + 1).is_some_and(|b| band(a) != band(b)) {
                    let (x0, x1) = ((to_x(i).round() as f32).max(pl), (to_x(i + 1).round() as f32).min(pr));
                    if x1 > x0 {
                        HudPipeline::push_quad(v, x0, y1 - w / 2.0, x1 - x0, w, line);
                        left -= 1;
                    }
                }
                if left < 2 {
                    break 'rows;
                }
            }
        }
    }

    /// Chunk lines: every 16 blocks when that is 9 GUI units or more apart, else every 64, 256 or 1024.
    fn draw_grid(&self, v: &mut Vec<HudVertex>, l: &Lay) {
        let s = l.s;
        let Some(g) = [16i64, 64, 256, 1024].into_iter().find(|&g| g as f32 / self.bpg >= 9.0) else { return };
        let (hx, hy, bpg) = (f64::from(l.gw) / 2.0, f64::from(l.top + l.bottom) / 2.0, f64::from(self.bpg));
        let w = s.max(1.0);
        let line = |major: bool| if major { [1.0, 1.0, 1.0, 0.28] } else { [1.0, 1.0, 1.0, 0.14] };
        let (x0, x1) = (((self.cx - hx * bpg) / g as f64).floor() as i64, ((self.cx + hx * bpg) / g as f64).ceil() as i64);
        for k in x0..=x1 {
            let x = ((hx + (k as f64 * g as f64 - self.cx) / bpg) as f32 * s).round();
            if x >= 0.0 && x < l.gw * s {
                HudPipeline::push_quad(v, x, l.top * s, w, (l.bottom - l.top) * s, line(k % 8 == 0));
            }
        }
        let (z0, z1) = (((self.cz - (hy - f64::from(l.top)) * bpg) / g as f64).floor() as i64, ((self.cz + (f64::from(l.bottom) - hy) * bpg) / g as f64).ceil() as i64);
        for k in z0..=z1 {
            let y = ((hy + (k as f64 * g as f64 - self.cz) / bpg) as f32 * s).round();
            if y >= l.top * s && y < l.bottom * s {
                HudPipeline::push_quad(v, 0.0, y, l.gw * s, w, line(k % 8 == 0));
            }
        }
    }

    fn to_gui(&self, l: &Lay, wx: f64, wz: f64) -> (f32, f32) {
        let bpg = f64::from(self.bpg);
        (l.gw / 2.0 + ((wx - self.cx) / bpg) as f32, (l.top + l.bottom) / 2.0 + ((wz - self.cz) / bpg) as f32)
    }

    /// Structure starts, the world's axes, the spawn, the player and the read-out point.
    fn draw_marks(&self, v: &mut Vec<HudVertex>, l: &Lay) {
        let s = l.s;
        let inside = |x: f32, y: f32, pad: f32| x > -pad && x < l.gw + pad && y > l.top - pad && y < l.bottom + pad;
        let (ox, oz) = self.to_gui(l, 0.0, 0.0);
        let line = [1.0, 1.0, 1.0, 0.3];
        if ox >= 0.0 && ox < l.gw {
            HudPipeline::push_quad(v, (ox * s).round(), l.top * s, s.max(1.0), (l.bottom - l.top) * s, line);
        }
        if oz >= l.top && oz < l.bottom {
            HudPipeline::push_quad(v, 0.0, (oz * s).round(), l.gw * s, s.max(1.0), line);
        }
        if inside(ox, oz, 4.0) {
            HudPipeline::push_quad(v, (ox - 2.0) * s, (oz - 2.0) * s, 5.0 * s, 5.0 * s, rgb(0xFF3030));
        }
        if self.show_struct && self.has_structs() {
            for &(x, z, st) in self.marks.values().flatten() {
                if self.sh.is_some_and(|h| h != st) {
                    continue;
                }
                let (gx, gy) = self.to_gui(l, f64::from(x), f64::from(z));
                if inside(gx, gy, -3.0) {
                    HudPipeline::push_quad(v, (gx - 4.0) * s, (gy - 4.0) * s, 8.0 * s, 8.0 * s, rgb(0x000000));
                    HudPipeline::push_quad(v, (gx - 3.0) * s, (gy - 3.0) * s, 6.0 * s, 6.0 * s, rgb(ST_COLORS[usize::from(st) % ST_COLORS.len()]));
                }
            }
        }
        if let Some(ig) = &self.ingame {
            let (gx, gy) = self.to_gui(l, ig.player.0, ig.player.1);
            if inside(gx, gy, -3.0) {
                HudPipeline::push_quad(v, (gx - 4.0) * s, (gy - 4.0) * s, 9.0 * s, 9.0 * s, rgb(0x000000));
                HudPipeline::push_quad(v, (gx - 3.0) * s, (gy - 3.0) * s, 7.0 * s, 7.0 * s, rgb(0xFFFF55));
            }
        }
        if let Some((px, pz)) = self.probe {
            let (x, y) = self.to_gui(l, px, pz);
            if y > l.top && y < l.bottom && x > 0.0 && x < l.gw {
                for r in [[x - 6.0, y - 0.5, 4.0, 1.0], [x + 2.0, y - 0.5, 4.0, 1.0], [x - 0.5, y - 6.0, 1.0, 4.0], [x - 0.5, y + 2.0, 1.0, 4.0]] {
                    HudPipeline::push_quad(v, (r[0] - 0.5) * s, (r[1] - 0.5) * s, (r[2] + 1.0) * s, (r[3] + 1.0) * s, rgb(0x000000));
                    HudPipeline::push_quad(v, r[0] * s, r[1] * s, r[2] * s, r[3] * s, rgb(0xFFFFFF));
                }
            }
        }
    }

    fn draw_legend(&self, v: &mut Vec<HudVertex>, l: &Lay) {
        let (s, p) = (l.s, l.panel);
        ui::panel(v, s, p, 0.8);
        let from = v.len();
        for (n, row) in self.rows().iter().enumerate() {
            let y = p[1] + 2.0 + n as f32 * LEG_ROW - self.scroll;
            if y + LEG_ROW < p[1] || y > p[1] + p[3] {
                continue;
            }
            let (name, c, on) = match *row {
                LegRow::Biome(b) => (info(b).0.to_string(), info(b).1, self.hl == Some(idx(b))),
                LegRow::Head => (format!("Structures {}", if self.show_struct { "(on)" } else { "(off)" }), 0xFFAA00, false),
                LegRow::Struct(n) => (st_label(n), ST_COLORS[usize::from(n) % ST_COLORS.len()], self.sh == Some(n)),
            };
            if on {
                HudPipeline::push_quad(v, (p[0] + 1.0) * s, y * s, (p[2] - 2.0) * s, LEG_ROW * s, [1.0, 1.0, 1.0, 0.25]);
            }
            if *row != LegRow::Head {
                HudPipeline::push_quad(v, (p[0] + 4.0) * s, (y + 2.0) * s, 8.0 * s, 8.0 * s, rgb(0x000000));
                HudPipeline::push_quad(v, (p[0] + 5.0) * s, (y + 3.0) * s, 6.0 * s, 6.0 * s, rgb(c));
            }
            let (x, col) = if *row == LegRow::Head { (4.0, 0xFFAA00) } else { (16.0, if on { 0xFFFF55 } else { 0xE0E0E0 }) };
            HudPipeline::push_text(v, (p[0] + x) * s, (y + 2.0) * s, s, &ui::fit(&name, p[2] - x - 4.0), rgb(col));
        }
        ui::clip(v, from, [p[0] * s, p[1] * s, p[2] * s, p[3] * s]);
        let max = self.max_scroll(l);
        if max > 0.0 {
            ui::scrollbar(v, s, p[0] + p[2] - 3.0, p[1], p[3], p[3] + max, self.scroll);
        }
    }

    /// The line under the map for the point a tap chose (or the middle, live): the structure start next to it if there is one, else biome,
    /// water, height, temperature and humidity (0..1) and the block.
    fn readout(&self, lvl: u8) -> Option<String> {
        let (px, pz) = self.probe?;
        if self.show_struct && self.has_structs() {
            let reach = f64::from(self.bpg) * 7.0;
            let near = self.marks.values().flatten().filter(|m| self.sh.map_or(true, |h| h == m.2)).find(|m| (f64::from(m.0) - px).hypot(f64::from(m.1) - pz) <= reach);
            if let Some(&(x, z, st)) = near {
                return Some(format!("{}  ({x}, {z})", st_label(st)));
            }
        }
        let step = Self::step(lvl);
        let (i, j) = ((px / step).floor() as i32, (pz / step).floor() as i32);
        let (t, k) = self.sample(lvl, i, j)?;
        let name = info(ALL[t.b[k] as usize % ALL.len()]).0;
        let water = ["", " (river)", " (lake)"][usize::from(t.r[k]).min(2)];
        Some(format!("{name}{water}  Y {}  T {:.2}  H {:.2}  ({}, {})", t.h[k], f32::from(t.t[k]) / 255.0, f32::from(t.w[k]) / 255.0, px.floor() as i64, pz.floor() as i64))
    }
}

impl Drop for Preview {
    /// No world is open in the menu, so nothing may keep the settings the preview handed to `natural` (in the game they are the world's).
    fn drop(&mut self) {
        if self.ingame.is_none() {
            natural::set(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_helpers_keep_their_ends() {
        assert_eq!(ramp(&TEMP, 0.0), TEMP[0]);
        assert_eq!(ramp(&TEMP, 1.0), TEMP[5]);
        assert_eq!(mix(0x000000, 0xFFFFFF, 0.5), 0x808080);
        assert_eq!(hypso(127), 0xFFFFFF);
        assert_eq!(hypso(0), 0x081A50);
    }

    #[test]
    fn explored_squares_nest_and_survive_a_save() {
        let mut e = Explored::default();
        e.visit(3, -2, 1);
        assert!(e.seen(3 * 16 + 5, -2 * 16 + 5, 0));
        assert!(!e.seen(40 * 16, 0, 0));
        assert!(e.seen(3 * 16, -2 * 16, 2)); // the square of 4 chunks that holds it
        let back = Explored::decode(&e.encode());
        assert!(back.seen(3 * 16, -2 * 16, 0) && back.seen(4 * 16, -2 * 16, 0));
        assert!(!back.seen(0, 40 * 16, 0));
    }

    #[test]
    fn every_biome_has_a_legend_entry() {
        for (n, b) in ALL.iter().enumerate() {
            assert_eq!(idx(*b) as usize, n);
            assert!(!info(*b).0.is_empty());
        }
    }
}
