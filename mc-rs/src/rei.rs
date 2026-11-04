//! Item and recipe viewer after Roughly Enough Items (REI 26.2, shedaniel; its `ScreenOverlayImpl` entry list, `DefaultDisplayViewingScreen`,
//! `DefaultCraftingCategory`, `DefaultFuelCategory`, `FavoritesListWidget`, `TagArgumentType` / `ModArgumentType`, `TransferHandler`), in GUI units like
//! `stats_ui.rs`, drawn with the "Immersive Interfaces REI support" pack (`tools/packs/immersive_rei`, `tools/gen_rei.py`, light or dark by
//! `Settings::rei_theme`). UNVERIFIED: not compiled, not run.
//!
//! Touch, not mouse (REI: hover + `R` / `U` keys, click, page keys): the entry list is paged like REI's (arrows or a horizontal swipe), a TAP on an
//! entry shows its recipes (REI's `R`), a LONG press its uses (`U`), the held name is the tooltip; the mode button cycles tap / long press through
//! View, Give (REI's cheat mode: tap gives 1, long press a stack) and Fav (tap stars the item). In the recipe view the category tabs, the page row and the
//! workstation follow REI; a tap on a slot of a recipe opens that item's recipes, a long press its uses. Back returns through the history, then to the list.
//!
//! * Categories are a registry (`Category`, `register_category`): Crafting, Smelting, Fuel, Block Drops (what `items::block_drops` says) and Tag.
//! * Tags (`TAGS`, not b1.7.3: it has none) are predicates over the catalog, namespaced like REI's (`minecraft:ores`, `cortes:weapons`).
//! * Search: words match the name, `@mod` the namespace (`@minecraft`, `@cortes`), `#tag` the tags, a leading `-` excludes.
//! * Favorites (`FavoritesListWidget`): items (the star of the recipe view, or Fav mode) and recipes (the star of each display), shown by the Favs tab
//! (tap opens, long press removes), kept per world in `reiFavorites` of the world folder.
//! * Auto-crafting (`TransferHandler`): while the viewer is open over a crafting grid, the `+` of a crafting display lays the recipe out from the
//! inventory (long press: as many sets as the stock allows); the viewer then closes. `craft::Screen::auto_fill` does the moving.

use crate::input::touch_ui::ScreenEv;
use crate::menu::{rgb, scaled_gs, tw};
use crate::options::Settings;
use crate::render::hud::{sprites::rei as sp, HudPipeline, HudVertex};
use crate::world::craft;
use crate::world::items::{self, ItemStack};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

const LONG_SECS: f32 = 0.45;
const CELL: f32 = 20.0;
const WHITE: u32 = 0xFFFFFF;
const GREY: u32 = 0xA0A0A0;
const RED: [f32; 4] = [0.85, 0.1, 0.1, 0.45];
/// Favorites kept per world, and the tag members one display shows.
const FAV_MAX: usize = 64;
const TAG_SHOWN: usize = 14;
const FAV_FILE: &str = "reiFavorites";

/// An item as REI lists it: the id and the damage that tells variants apart (wool and dye colour, charcoal, slab kind).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Key {
    pub id: u16,
    pub dmg: u16,
}

impl Key {
    fn stack(self, count: u8) -> ItemStack {
        ItemStack { id: self.id, count, damage: self.dmg }
    }
    fn name(self) -> String {
        items::name(self.stack(1)).map_or_else(|| format!("#{}", self.id), str::to_string)
    }
    /// The mod an entry belongs to (REI's `@mod`): Cortes Combat's weapons and bandage (ids 400..=430, NOT b1.7.3), everything else vanilla.
    pub fn ns(self) -> &'static str {
        if (400..=430).contains(&self.id) { "cortes" } else { "minecraft" }
    }
}

/// Blocks without an item form (liquids, the bed / door / sign / crops / cake blocks, lit variants, fire, portal ...).
const SKIP: &[u16] = &[8, 9, 10, 11, 26, 34, 36, 43, 51, 52, 55, 59, 60, 62, 63, 64, 68, 71, 74, 75, 83, 90, 92, 93, 94, 95];

/// `EntryRegistry`: every named block and item, the wool and dye colours, the slab kinds, charcoal and the Cortes weapons.
fn catalog() -> &'static [Key] {
    static C: OnceLock<Vec<Key>> = OnceLock::new();
    C.get_or_init(|| {
        let mut v: Vec<Key> = Vec::new();
        for &(id, _) in crate::world::names::NAMES {
            if !SKIP.contains(&id) {
                v.push(Key { id, dmg: 0 });
            }
        }
        v.extend((0..16).map(|dmg| Key { id: 35, dmg }));
        v.extend((0..16).map(|dmg| Key { id: 351, dmg }));
        v.extend((0..3).map(|dmg| Key { id: 44, dmg }));
        v.push(Key { id: 263, dmg: 1 });
        v.extend((400..=430).filter(|&id| items::name(ItemStack { id, count: 1, damage: 0 }).is_some()).map(|id| Key { id, dmg: 0 }));
        v.sort_by_key(|k| (k.id, k.dmg));
        v.dedup();
        v
    })
}

// ---- Tags ----

/// A tag (`TagKey`): `has` says which catalog entries belong to it. b1.7.3 has no tags; these are this port's own, named like REI's.
pub struct Tag {
    pub ns: &'static str,
    pub name: &'static str,
    pub has: fn(Key) -> bool,
}

fn tool_is(k: Key, kind: craft::Kind) -> bool {
    craft::tool(k.id).is_some_and(|t| t.0 == kind)
}

static TAGS: &[Tag] = &[
    Tag { ns: "minecraft", name: "ores", has: |k| matches!(k.id, 14 | 15 | 16 | 21 | 56 | 73) },
    Tag { ns: "minecraft", name: "fuel", has: |k| craft::fuel_ticks(k.id) > 0 },
    Tag { ns: "minecraft", name: "wool", has: |k| k.id == 35 },
    Tag { ns: "minecraft", name: "dyes", has: |k| k.id == 351 },
    Tag { ns: "minecraft", name: "ingots", has: |k| matches!(k.id, 265 | 266) },
    Tag { ns: "minecraft", name: "slabs", has: |k| k.id == 44 },
    Tag { ns: "minecraft", name: "food", has: |k| items::heal_amount(k.id).is_some() },
    Tag { ns: "minecraft", name: "armor", has: |k| craft::armor(k.id).is_some() },
    Tag { ns: "minecraft", name: "tools", has: |k| craft::tool(k.id).is_some() || k.id == craft::SHEARS },
    Tag { ns: "minecraft", name: "swords", has: |k| tool_is(k, craft::Kind::Sword) },
    Tag { ns: "minecraft", name: "pickaxes", has: |k| tool_is(k, craft::Kind::Pickaxe) },
    Tag { ns: "minecraft", name: "axes", has: |k| tool_is(k, craft::Kind::Axe) },
    Tag { ns: "minecraft", name: "shovels", has: |k| tool_is(k, craft::Kind::Shovel) },
    Tag { ns: "minecraft", name: "hoes", has: |k| tool_is(k, craft::Kind::Hoe) },
    Tag { ns: "cortes", name: "weapons", has: |k| (400..=425).contains(&k.id) },
];

fn tag_id(t: &Tag) -> String {
    format!("{}:{}", t.ns, t.name)
}

fn tags_of(k: Key) -> impl Iterator<Item = &'static Tag> {
    TAGS.iter().filter(move |t| (t.has)(k))
}

fn members(t: &Tag) -> Vec<Key> {
    catalog().iter().copied().filter(|&k| (t.has)(k)).collect()
}

fn tag_by_id(id: &str) -> Option<&'static Tag> {
    TAGS.iter().find(|t| tag_id(t) == id)
}

// ---- Displays and categories ----

/// A recipe the viewer shows: `DisplaySpec`. `Info` is the general one a category (built in or registered) fills: inputs, outputs and a note.
#[derive(Clone, Debug, PartialEq)]
pub enum Disp {
    Craft { w: usize, cells: [u16; 9], out: ItemStack },
    Smelt { inp: Key, out: ItemStack },
    Fuel { item: Key, ticks: u16 },
    Info { cat: &'static str, ins: Vec<Key>, outs: Vec<Key>, note: String },
}

impl Disp {
    /// The `Category::id` this display belongs to.
    fn cat(&self) -> &'static str {
        match self {
            Disp::Craft { .. } => "crafting",
            Disp::Smelt { .. } => "smelting",
            Disp::Fuel { .. } => "fuel",
            Disp::Info { cat, .. } => *cat,
        }
    }
    /// The entry to open to see this display again, and whether as uses.
    fn target(&self) -> (Key, bool) {
        match self {
            Disp::Craft { out, .. } | Disp::Smelt { out, .. } => (Key { id: out.id, dmg: out.damage }, false),
            Disp::Fuel { item, .. } => (*item, true),
            Disp::Info { ins, outs, .. } => (outs.first().or(ins.first()).copied().unwrap_or(Key { id: 1, dmg: 0 }), false),
        }
    }
}

/// A `craft::code`: the id, and the colour above bit 9 for wool and dye.
fn decode(c: u16) -> Key {
    let id = c & 511;
    Key { id, dmg: if matches!(id, 35 | 351) { c >> 9 } else { 0 } }
}

fn same(a: Key, b: Key) -> bool {
    a.id == b.id && (!matches!(a.id, 35 | 351 | 263) || a.dmg == b.dmg)
}

/// A category (`DisplayCategory`): its tab icon, its workstations and what it finds for an entry (`uses` false: how it is made, true: what it is used for).
#[derive(Debug)]
pub struct Category {
    pub id: &'static str,
    pub name: &'static str,
    /// The item drawn on the tab.
    pub tab: u16,
    /// The workstations (`CategoryRegistry.getWorkstations`), the first one is drawn.
    pub work: &'static [u16],
    pub find: fn(Key, bool) -> Vec<Disp>,
}

static BUILTIN: &[Category] = &[
    Category { id: "crafting", name: "Crafting", tab: 58, work: &[58], find: find_crafting },
    Category { id: "smelting", name: "Smelting", tab: 61, work: &[61], find: find_smelting },
    Category { id: "fuel", name: "Fuel", tab: 263, work: &[61], find: find_fuel },
    Category { id: "drops", name: "Block Drops", tab: 270, work: &[270], find: find_drops },
    Category { id: "tag", name: "Tag", tab: 339, work: &[], find: find_tags },
];

static EXTRA: Mutex<Vec<&'static Category>> = Mutex::new(Vec::new());

/// `REIClientPlugin.registerCategories`: add a category of your own (an id nobody has). Its `find` returns `Disp::Info`s.
#[allow(dead_code)]
pub fn register_category(c: &'static Category) {
    if let Ok(mut e) = EXTRA.lock() {
        if !e.iter().any(|o| o.id == c.id) && !BUILTIN.iter().any(|o| o.id == c.id) {
            e.push(c);
        }
    }
}

fn categories() -> Vec<&'static Category> {
    let mut v: Vec<&'static Category> = BUILTIN.iter().collect();
    if let Ok(e) = EXTRA.lock() {
        v.extend(e.iter().copied());
    }
    v
}

fn category(id: &str) -> Option<&'static Category> {
    categories().into_iter().find(|c| c.id == id)
}

fn find_crafting(k: Key, uses: bool) -> Vec<Disp> {
    craft::recipes()
        .iter()
        .filter_map(|r| {
            let (w, _, cells, out) = r.view();
            let hit = if uses { cells.iter().any(|&c| c != 0 && same(decode(c), k)) } else { same(Key { id: out.id, dmg: out.damage }, k) };
            hit.then_some(Disp::Craft { w, cells, out })
        })
        .collect()
}

fn find_smelting(k: Key, uses: bool) -> Vec<Disp> {
    let mut d = Vec::new();
    for id in 1..512u16 {
        if let Some(out) = craft::smelting(id) {
            let inp = Key { id, dmg: 0 };
            if if uses { same(inp, k) } else { same(Key { id: out.id, dmg: out.damage }, k) } {
                d.push(Disp::Smelt { inp, out });
            }
        }
    }
    d
}

fn find_fuel(k: Key, uses: bool) -> Vec<Disp> {
    let t = craft::fuel_ticks(k.id);
    if uses && t > 0 { vec![Disp::Fuel { item: k, ticks: t }] } else { Vec::new() }
}

/// What mining a block gives: for a block its drops (uses), for an item the blocks that give it. A block that drops itself is no news.
fn find_drops(k: Key, uses: bool) -> Vec<Disp> {
    let mk = |block: Key, out: Key| Disp::Info { cat: "drops", ins: vec![block], outs: vec![out], note: String::new() };
    let mut v = Vec::new();
    if uses {
        if k.id < 256 && !SKIP.contains(&k.id) {
            for (id, dmg) in items::block_drops(k.id as u8) {
                if id != k.id {
                    v.push(mk(Key { id: k.id, dmg: 0 }, Key { id, dmg }));
                }
            }
        }
    } else {
        for b in 1..256u16 {
            if SKIP.contains(&b) {
                continue;
            }
            for (id, dmg) in items::block_drops(b as u8) {
                if id != b && same(Key { id, dmg }, k) {
                    v.push(mk(Key { id: b, dmg: 0 }, Key { id, dmg }));
                }
            }
        }
    }
    v
}

/// `DefaultTagCategory`: the tags an entry is in, with their members.
fn find_tags(k: Key, _uses: bool) -> Vec<Disp> {
    tags_of(k).map(|t| Disp::Info { cat: "tag", ins: Vec::new(), outs: members(t).into_iter().take(TAG_SHOWN).collect(), note: tag_id(t) }).collect()
}

/// `ViewSearchBuilder.addRecipesFor` (`uses` false) / `addUsagesFor` (true): every category that has something, in registry order.
pub fn displays(k: Key, uses: bool) -> Vec<(&'static Category, Vec<Disp>)> {
    categories().into_iter().map(|c| (c, (c.find)(k, uses))).filter(|t| !t.1.is_empty()).collect()
}

#[derive(Clone, Debug)]
struct View {
    target: Key,
    uses: bool,
    cat: usize,
    page: usize,
    tabs: Vec<(&'static Category, Vec<Disp>)>,
}

impl View {
    fn new(target: Key, uses: bool) -> Self {
        Self { target, uses, cat: 0, page: 0, tabs: displays(target, uses) }
    }
    fn disps(&self) -> &[Disp] {
        match self.tabs.get(self.cat) {
            Some(t) => t.1.as_slice(),
            None => &[],
        }
    }
}

// ---- Search ----

enum Term {
    Text(String),
    Mod(String),
    Tag(String),
}

/// `SearchFilter`: words are ANDed. `@x` the mod, `#x` the tag, a leading `-` excludes; the rest match the name. Any case.
fn parse_query(q: &str) -> Vec<(bool, Term)> {
    q.split_whitespace()
        .map(|w| {
            let (neg, w) = match w.strip_prefix('-') {
                Some(r) if !r.is_empty() => (true, r),
                _ => (false, w),
            };
            let t = if let Some(m) = w.strip_prefix('@') {
                Term::Mod(m.to_lowercase())
            } else if let Some(t) = w.strip_prefix('#') {
                Term::Tag(t.to_lowercase())
            } else {
                Term::Text(w.to_lowercase())
            };
            (neg, t)
        })
        .collect()
}

fn matches(k: Key, terms: &[(bool, Term)]) -> bool {
    if terms.is_empty() {
        return true;
    }
    let name = k.name().to_lowercase();
    terms.iter().all(|(neg, t)| {
        let hit = match t {
            Term::Text(x) => name.contains(x.as_str()),
            Term::Mod(x) => k.ns().contains(x.as_str()),
            Term::Tag(x) => tags_of(k).any(|t| tag_id(t).contains(x.as_str())),
        };
        hit != *neg
    })
}

// ---- Favorites ----

/// A favorite (`FavoriteEntry`): an item, or a whole recipe.
#[derive(Clone, Debug, PartialEq)]
pub enum Fav {
    Item(Key),
    Recipe(Disp),
}

fn fav_key(f: &Fav) -> Key {
    match f {
        Fav::Item(k) => *k,
        Fav::Recipe(d) => d.target().0,
    }
}

const MAX_ID: u16 = 4096;

fn enc_key(k: Key) -> String {
    format!("{}.{}", k.id, k.dmg)
}

fn dec_key(s: &str) -> Option<Key> {
    let (a, b) = s.split_once('.')?;
    let k = Key { id: a.parse().ok()?, dmg: b.parse().ok()? };
    (k.id < MAX_ID && k.dmg < MAX_ID).then_some(k)
}

fn enc_stack(s: ItemStack) -> String {
    format!("{}.{}.{}", s.id, s.damage, s.count)
}

fn dec_stack(s: &str) -> Option<ItemStack> {
    let mut p = s.split('.');
    let st = ItemStack { id: p.next()?.parse().ok()?, damage: p.next()?.parse().ok()?, count: p.next()?.parse().ok()? };
    (p.next().is_none() && st.id < MAX_ID && st.damage < MAX_ID && st.count > 0).then_some(st)
}

fn enc_keys(v: &[Key]) -> String {
    if v.is_empty() { "-".to_string() } else { v.iter().map(|k| enc_key(*k)).collect::<Vec<_>>().join(",") }
}

fn dec_keys(s: &str) -> Option<Vec<Key>> {
    if s == "-" {
        return Some(Vec::new());
    }
    let v = s.split(',').take(FAV_MAX + 1).map(dec_key).collect::<Option<Vec<_>>>()?;
    (v.len() <= FAV_MAX).then_some(v)
}

/// One line of `reiFavorites`: `i key`, `c w out cells`, `s in out`, `f item ticks`, `x category note ins outs` (keys are `id.damage`, stacks `id.damage.count`).
fn fav_encode(f: &Fav) -> String {
    match f {
        Fav::Item(k) => format!("i {}", enc_key(*k)),
        Fav::Recipe(Disp::Craft { w, cells, out }) => format!("c {} {} {}", w, enc_stack(*out), cells.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(",")),
        Fav::Recipe(Disp::Smelt { inp, out }) => format!("s {} {}", enc_key(*inp), enc_stack(*out)),
        Fav::Recipe(Disp::Fuel { item, ticks }) => format!("f {} {}", enc_key(*item), ticks),
        Fav::Recipe(Disp::Info { cat, ins, outs, note }) => {
            let note = if note.is_empty() { "-".to_string() } else { note.replace(char::is_whitespace, "_") };
            format!("x {} {} {} {}", cat, note, enc_keys(ins), enc_keys(outs))
        }
    }
}

/// The file is the player's to edit and to corrupt: a line that does not parse, or names an unknown category, is dropped.
fn fav_decode(line: &str) -> Option<Fav> {
    let mut p = line.split_whitespace();
    let f = match p.next()? {
        "i" => Fav::Item(dec_key(p.next()?)?),
        "c" => {
            let w: usize = p.next()?.parse().ok()?;
            let out = dec_stack(p.next()?)?;
            let list = p.next()?.split(',').map(|c| c.parse::<u16>().ok()).collect::<Option<Vec<u16>>>()?;
            if w > 3 || list.len() != 9 || list.iter().any(|&c| c & 511 >= MAX_ID) {
                return None;
            }
            let mut cells = [0u16; 9];
            cells.copy_from_slice(&list);
            Fav::Recipe(Disp::Craft { w, cells, out })
        }
        "s" => Fav::Recipe(Disp::Smelt { inp: dec_key(p.next()?)?, out: dec_stack(p.next()?)? }),
        "f" => {
            let ticks: u16 = p.next()?.parse().ok()?;
            Fav::Recipe(Disp::Fuel { item: dec_key(p.next()?)?, ticks })
        }
        "x" => {
            let cat = category(p.next()?)?.id;
            let note = p.next()?;
            let note = if note == "-" { String::new() } else { note.to_string() };
            Fav::Recipe(Disp::Info { cat, ins: dec_keys(p.next()?)?, outs: dec_keys(p.next()?)?, note })
        }
        _ => return None,
    };
    p.next().is_none().then_some(f)
}

fn parse_favs(text: &str) -> Vec<Fav> {
    let mut v: Vec<Fav> = Vec::new();
    for f in text.lines().filter_map(fav_decode) {
        if v.len() < FAV_MAX && !v.contains(&f) {
            v.push(f);
        }
    }
    v
}

fn dump_favs(f: &[Fav]) -> String {
    f.iter().map(|f| fav_encode(f) + "\n").collect()
}

fn load_favs(dir: &Path) -> Vec<Fav> {
    std::fs::read_to_string(dir.join(FAV_FILE)).map(|s| parse_favs(&s)).unwrap_or_default()
}

fn save_favs(dir: &Path, f: &[Fav]) {
    if let Err(e) = crate::world::save::write(&dir.join(FAV_FILE), dump_favs(f).as_bytes()) {
        log::warn!("rei: favorites not saved: {e}");
    }
}

// ---- The viewer ----

/// What the viewer asks of the game.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Out {
    Close,
    Give(ItemStack),
    /// Auto-crafting: lay this recipe (`w` x rows of `cells` codes, `w` 0 = shapeless) out on the open grid; `max` = as many sets as possible.
    Fill { w: usize, cells: [u16; 9], max: bool },
}

/// What the game knows when it opens the viewer.
#[derive(Default)]
pub struct Ctx {
    /// The open crafting grid's width (2 inventory, 3 table), 0 when there is none: then the `+` of a recipe is not offered.
    pub craft_gw: usize,
    /// What the player has, `craft::Screen::have`.
    pub have: Vec<(u16, u32)>,
    /// The world folder, where the favorites are kept; `None` keeps none.
    pub dir: Option<PathBuf>,
}

/// What a tap on an entry does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    /// Tap: recipes, long press: uses.
    View,
    /// REI's cheat mode: tap gives 1, long press a stack.
    Give,
    /// Tap (or long press) stars the item.
    Fav,
}

struct Press {
    at: (f32, f32),
    t: Instant,
    drag: bool,
}

pub struct ReiUi {
    mode: Mode,
    /// The Favs tab of the list instead of every entry.
    tab_favs: bool,
    craft_gw: usize,
    have: Vec<(u16, u32)>,
    dir: Option<PathBuf>,
    favs: Vec<Fav>,
    /// The favorites the query lets through (indices into `favs`).
    fav_idx: Vec<usize>,
    query: String,
    typing: bool,
    list: Vec<Key>,
    page: usize,
    view: Option<View>,
    hist: Vec<View>,
    press: Option<Press>,
    finger: Option<(f32, f32)>,
    notice: Option<(String, Instant)>,
}

fn inside(r: [f32; 4], x: f32, y: f32) -> bool {
    x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]
}

fn quad(v: &mut Vec<HudVertex>, s: f32, r: [f32; 4], c: [f32; 4]) {
    HudPipeline::push_quad(v, r[0] * s, r[1] * s, r[2] * s, r[3] * s, c);
}

fn text(v: &mut Vec<HudVertex>, s: f32, x: f32, y: f32, t: &str, c: u32) {
    HudPipeline::push_text(v, x * s, y * s, s, t, rgb(c));
}

fn centred(v: &mut Vec<HudVertex>, s: f32, r: [f32; 4], t: &str, c: u32) {
    text(v, s, r[0] + (r[2] - tw(t)) / 2.0, r[1] + (r[3] - 8.0) / 2.0, t, c);
}

/// A nine-slice panel of the pack (border 4) over `r` in GUI units.
fn nine(v: &mut Vec<HudVertex>, s: f32, theme: usize, kind: usize, r: [f32; 4]) {
    let (sx, sy, ..) = sp::panel(theme, kind);
    let (x, y, w, h) = (r[0], r[1], r[2].max(8.0), r[3].max(8.0));
    let cols = [(0.0, x, 4.0), (4.0, x + 4.0, w - 8.0), (8.0, x + w - 4.0, 4.0)];
    let rows = [(0.0, y, 4.0), (4.0, y + 4.0, h - 8.0), (8.0, y + h - 4.0, 4.0)];
    for &(cy, dy, dh) in &rows {
        for &(cx, dx, dw) in &cols {
            HudPipeline::push_gui(v, dx * s, dy * s, dw * s, dh * s, (sx + cx, sy + cy, 4.0, 4.0));
        }
    }
}

fn button(v: &mut Vec<HudVertex>, s: f32, theme: usize, r: [f32; 4], label: &str, lit: bool, enabled: bool) {
    nine(v, s, theme, if lit { sp::LIGHTER } else { sp::DEFAULT }, r);
    centred(v, s, r, label, if enabled { WHITE } else { GREY });
}

fn icon(v: &mut Vec<HudVertex>, s: f32, x: f32, y: f32, st: ItemStack) {
    crate::push_stack(v, (x * s, y * s, 16.0 * s, 16.0 * s), 1.0, st);
}

/// The favorite star (the sprite is 9 x 9) drawn `size` wide at (x, y).
fn star(v: &mut Vec<HudVertex>, s: f32, th: usize, x: f32, y: f32, size: f32, on: bool) {
    HudPipeline::push_gui(v, x * s, y * s, size * s, size * s, sp::star(th, on));
}

/// The panel's layout, GUI units.
struct Lay {
    close: [f32; 4],
    search: [f32; 4],
    mode: [f32; 4],
    prev: [f32; 4],
    next: [f32; 4],
    favs: [f32; 4],
    label: [f32; 4],
    gx: f32,
    gy: f32,
    cols: usize,
    rows: usize,
}

fn lay(gw: f32, gh: f32) -> Lay {
    let cols = (((gw - 24.0) / CELL).floor() as usize).max(1);
    let rows = (((gh - 76.0) / CELL).floor() as usize).max(1);
    let gx = ((gw - cols as f32 * CELL) / 2.0).floor();
    Lay {
        close: [10.0, 10.0, 40.0, 20.0],
        search: [54.0, 10.0, gw - 54.0 - 100.0, 20.0],
        mode: [gw - 92.0, 10.0, 82.0, 20.0],
        prev: [10.0, gh - 30.0, 40.0, 20.0],
        next: [gw - 50.0, gh - 30.0, 40.0, 20.0],
        favs: [gw - 110.0, gh - 30.0, 56.0, 20.0],
        label: [54.0, gh - 30.0, gw - 168.0, 20.0],
        gx,
        gy: 36.0,
        cols,
        rows,
    }
}

impl ReiUi {
    /// `cheat`: opened from Tools (gives) or from the inventory (views).
    pub fn new(cheat: bool, ctx: Ctx) -> Self {
        let favs = ctx.dir.as_deref().map(load_favs).unwrap_or_default();
        let mut ui = Self {
            mode: if cheat { Mode::Give } else { Mode::View },
            tab_favs: false,
            craft_gw: ctx.craft_gw,
            have: ctx.have,
            dir: ctx.dir,
            favs,
            fav_idx: Vec::new(),
            query: String::new(),
            typing: false,
            list: catalog().to_vec(),
            page: 0,
            view: None,
            hist: Vec::new(),
            press: None,
            finger: None,
            notice: None,
        };
        ui.refresh_favs();
        ui
    }

    /// A line for the player in red at the bottom (the game uses it for a refused auto-craft).
    pub fn notice(&mut self, s: &str) {
        self.notice = Some((s.to_string(), Instant::now()));
    }

    // ---- the search field (the phone's soft keyboard, see `rei_key` in lib.rs) ----
    pub fn typing(&self) -> bool {
        self.typing
    }
    pub fn end_typing(&mut self) {
        self.typing = false;
    }
    pub fn type_char(&mut self, c: char) {
        if !c.is_control() && self.query.chars().count() < 32 {
            self.query.push(c);
            self.refilter();
        }
    }
    pub fn backspace(&mut self) {
        self.query.pop();
        self.refilter();
    }

    /// `EntryListSearchManager`: the entries the query lets through (see `parse_query`), and the favorites with them.
    fn refilter(&mut self) {
        let terms = parse_query(&self.query);
        self.list = catalog().iter().copied().filter(|&k| matches(k, &terms)).collect();
        self.refresh_favs();
        self.page = 0;
    }

    fn refresh_favs(&mut self) {
        let terms = parse_query(&self.query);
        self.fav_idx = (0..self.favs.len()).filter(|&i| matches(fav_key(&self.favs[i]), &terms)).collect();
    }

    // ---- favorites ----
    fn is_fav(&self, f: &Fav) -> bool {
        self.favs.contains(f)
    }

    fn toggle(&mut self, f: Fav) {
        if let Some(i) = self.favs.iter().position(|x| *x == f) {
            self.favs.remove(i);
        } else if self.favs.len() < FAV_MAX {
            self.favs.push(f);
        } else {
            self.notice("Favorites are full");
            return;
        }
        if let Some(d) = &self.dir {
            save_favs(d, &self.favs);
        }
        self.refresh_favs();
    }

    // ---- the entry list ----
    fn len(&self) -> usize {
        if self.tab_favs { self.fav_idx.len() } else { self.list.len() }
    }

    /// The entry of cell `i` of the list, and its place in `favs` on the Favs tab.
    fn cell(&self, i: usize) -> Option<(Key, Option<usize>)> {
        if self.tab_favs {
            let f = *self.fav_idx.get(i)?;
            Some((fav_key(&self.favs[f]), Some(f)))
        } else {
            self.list.get(i).map(|&k| (k, None))
        }
    }

    fn per_page(l: &Lay) -> usize {
        l.cols * l.rows
    }

    fn pages(&self, l: &Lay) -> usize {
        self.len().div_ceil(Self::per_page(l)).max(1)
    }

    /// The page shown: a removed favorite can leave `page` past the end.
    fn cur(&self, l: &Lay) -> usize {
        self.page.min(self.pages(l) - 1)
    }

    fn cell_at(&self, l: &Lay, x: f32, y: f32) -> Option<(Key, Option<usize>)> {
        let (c, r) = (((x - l.gx) / CELL).floor(), ((y - l.gy) / CELL).floor());
        if c < 0.0 || r < 0.0 || c >= l.cols as f32 || r >= l.rows as f32 {
            return None;
        }
        self.cell(self.cur(l) * Self::per_page(l) + r as usize * l.cols + c as usize)
    }

    // ---- the recipe view's geometry ----
    /// (panel, workstation, tabs, previous page, next page, page label, first display top, recipes per page).
    fn vlay(gw: f32, gh: f32, tabs: usize) -> ([f32; 4], [f32; 4], Vec<[f32; 4]>, [f32; 4], [f32; 4], [f32; 4], f32, usize) {
        let cx = (gw / 2.0).floor();
        let panel = [cx - 95.0, 34.0, 190.0, gh - 42.0];
        let tab = (0..tabs).map(|i| [panel[0] + 6.0 + 26.0 * i as f32, panel[1] + 4.0, 24.0, 24.0]).collect();
        let ny = panel[1] + 32.0;
        let (prev, next) = ([panel[0] + 5.0, ny, 28.0, 18.0], [panel[0] + 190.0 - 33.0, ny, 28.0, 18.0]);
        let label = [panel[0] + 35.0, ny, 120.0, 18.0];
        let top = ny + 22.0;
        let per = (((panel[1] + panel[3] - 6.0 - top) / 70.0).floor() as usize).max(1);
        (panel, [panel[0] - 28.0, panel[1] + 16.0, 24.0, 24.0], tab, prev, next, label, top, per)
    }

    /// (back, the star of the target, "+1", "Stack").
    fn header(gw: f32) -> ([f32; 4], [f32; 4], [f32; 4], [f32; 4]) {
        ([10.0, 8.0, 40.0, 20.0], [54.0, 8.0, 20.0, 20.0], [gw - 92.0, 8.0, 40.0, 20.0], [gw - 50.0, 8.0, 40.0, 20.0])
    }

    /// The star of a display (a touch-sized hit area in the top right corner) and its `+` (under the result).
    fn star_at(x: f32, y: f32) -> [f32; 4] {
        [x + 132.0, y, 18.0, 16.0]
    }
    fn plus_at(x: f32, y: f32) -> [f32; 4] {
        [x + 120.0, y + 52.0, 26.0, 12.0]
    }

    /// The slots of a display drawn with its top-left at (x, y) of a 150 x 66 base: (rect, item, drawn as the result).
    fn slots(d: &Disp, x: f32, y: f32) -> Vec<([f32; 4], Key, bool)> {
        let mut v = Vec::new();
        match d {
            Disp::Craft { w, cells, out } => {
                let (sx, sy) = (x + 17.0, y + 6.0); // `DefaultCraftingCategory`: centre - 58, centre - 27
                let w = if *w == 0 { 3 } else { *w };
                for (i, &c) in cells.iter().enumerate() {
                    if c != 0 {
                        v.push(([sx + 1.0 + (i % w) as f32 * 18.0, sy + 1.0 + (i / w) as f32 * 18.0, 18.0, 18.0], decode(c), false));
                    }
                }
                v.push(([sx + 95.0, sy + 19.0, 26.0, 26.0], Key { id: out.id, dmg: out.damage }, true));
            }
            Disp::Smelt { inp, out } => {
                v.push(([x + 23.0, y + 24.0, 18.0, 18.0], *inp, false));
                v.push(([x + 101.0, y + 20.0, 26.0, 26.0], Key { id: out.id, dmg: out.damage }, true));
            }
            Disp::Fuel { item, .. } => v.push(([x + 25.0, y + 24.0, 18.0, 18.0], *item, false)),
            Disp::Info { cat, ins, outs, .. } => {
                if *cat == "tag" {
                    for (i, k) in outs.iter().take(TAG_SHOWN).enumerate() {
                        v.push(([x + 6.0 + (i % 7) as f32 * 18.0, y + 15.0 + (i / 7) as f32 * 18.0, 18.0, 18.0], *k, false));
                    }
                } else {
                    for (i, k) in ins.iter().take(2).enumerate() {
                        v.push(([x + 12.0 + i as f32 * 20.0, y + 24.0, 18.0, 18.0], *k, false));
                    }
                    if outs.len() == 1 {
                        v.push(([x + 101.0, y + 20.0, 26.0, 26.0], outs[0], true));
                    } else {
                        for (i, k) in outs.iter().take(3).enumerate() {
                            v.push(([x + 84.0 + i as f32 * 20.0, y + 24.0, 18.0, 18.0], *k, false));
                        }
                    }
                }
            }
        }
        v
    }

    fn open(&mut self, k: Key, uses: bool) {
        if let Some(cur) = self.view.take() {
            self.hist.push(cur);
        }
        self.view = Some(View::new(k, uses));
    }

    /// A favorite's view: its entry's recipes (or uses), on the tab and page of that display.
    fn open_fav(&mut self, f: &Fav, gw: f32, gh: f32) {
        let (k, uses) = match f {
            Fav::Item(k) => (*k, false),
            Fav::Recipe(d) => d.target(),
        };
        self.open(k, uses);
        let per = Self::vlay(gw, gh, 0).7;
        if let (Fav::Recipe(d), Some(v)) = (f, self.view.as_mut()) {
            if let Some(ci) = v.tabs.iter().position(|t| t.0.id == d.cat()) {
                v.cat = ci;
                v.page = v.tabs[ci].1.iter().position(|x| x == d).map_or(0, |di| di / per);
            }
        }
    }

    // ---- auto-crafting ----
    /// Why the recipe cannot be laid out now: no table for a 3x3 one, or an ingredient the player lacks. `None`: it can.
    fn cant_craft(&self, w: usize, cells: &[u16; 9]) -> Option<String> {
        let gw = self.craft_gw;
        let list: Vec<u16> = cells.iter().copied().filter(|&c| c != 0).collect();
        let rows = if w == 0 { 0 } else { cells.iter().rposition(|&c| c != 0).map_or(0, |l| l / w + 1) };
        if (w == 0 && list.len() > gw * gw) || w > gw || rows > gw {
            return Some(if gw == 2 { "Needs a crafting table" } else { "Does not fit the grid" }.to_string());
        }
        list.iter().find(|&&c| self.have.iter().find(|h| h.0 == c).map_or(0, |h| h.1) < list.iter().filter(|&&o| o == c).count() as u32).map(|&c| format!("Missing: {}", decode(c).name()))
    }

    /// An ingredient slot the player cannot fill (REI paints it red).
    fn short(&self, k: Key, d: &Disp) -> bool {
        let Disp::Craft { cells, .. } = d else { return false };
        let c = craft::code(k.id, k.dmg);
        let need = cells.iter().filter(|&&o| o == c).count() as u32;
        self.have.iter().find(|h| h.0 == c).map_or(0, |h| h.1) < need
    }

    fn plus(&mut self, d: &Disp, long: bool) -> Option<Out> {
        let Disp::Craft { w, cells, .. } = d else { return None };
        if let Some(why) = self.cant_craft(*w, cells) {
            self.notice(&why);
            return None;
        }
        Some(Out::Fill { w: *w, cells: *cells, max: long })
    }

    /// A finger event in px. `Some(Out)` asks the game to close, to give an item or to lay a recipe out.
    pub fn event(&mut self, ev: ScreenEv, w: f32, h: f32, st: &Settings) -> Option<Out> {
        let (s, gw, gh) = scaled_gs(w, h, st.gui_scale);
        match ev {
            ScreenEv::Down(x, y) => {
                self.press = Some(Press { at: (x / s, y / s), t: Instant::now(), drag: false });
                self.finger = Some((x / s, y / s));
                None
            }
            ScreenEv::Move(x, y) => {
                let p = (x / s, y / s);
                self.finger = Some(p);
                if let Some(pr) = self.press.as_mut() {
                    pr.drag |= (p.0 - pr.at.0).abs().max((p.1 - pr.at.1).abs()) > 6.0;
                }
                None
            }
            ScreenEv::Up(x, y) => {
                self.finger = None;
                let pr = self.press.take()?;
                let (gx, gy) = (x / s, y / s);
                if pr.drag {
                    let d = gx - pr.at.0;
                    if d.abs() > 40.0 && d.abs() > (gy - pr.at.1).abs() {
                        self.turn(if d < 0.0 { 1 } else { -1 }, gw, gh);
                    }
                    return None;
                }
                self.tap(gx, gy, pr.t.elapsed().as_secs_f32() >= LONG_SECS, gw, gh)
            }
        }
    }

    /// Page `d` steps (wrapping, like REI's arrows).
    fn turn(&mut self, d: i32, gw: f32, gh: f32) {
        let l = lay(gw, gh);
        match self.view.as_mut() {
            None => {
                let n = self.pages(&l) as i32;
                self.page = (self.cur(&l) as i32 + d).rem_euclid(n) as usize;
            }
            Some(v) => {
                let per = Self::vlay(gw, gh, 0).7;
                let n = v.disps().len().div_ceil(per).max(1) as i32;
                v.page = (v.page as i32 + d).rem_euclid(n) as usize;
            }
        }
    }

    fn tap(&mut self, x: f32, y: f32, long: bool, gw: f32, gh: f32) -> Option<Out> {
        if self.view.is_some() {
            return self.tap_view(x, y, long, gw, gh);
        }
        let l = lay(gw, gh);
        if inside(l.search, x, y) {
            self.typing = true;
            return None;
        }
        self.typing = false;
        if inside(l.close, x, y) {
            return Some(Out::Close);
        }
        if inside(l.mode, x, y) {
            self.mode = match self.mode {
                Mode::View => Mode::Give,
                Mode::Give => Mode::Fav,
                Mode::Fav => Mode::View,
            };
        } else if inside(l.favs, x, y) {
            self.tab_favs = !self.tab_favs;
            self.page = 0;
        } else if inside(l.prev, x, y) {
            self.turn(-1, gw, gh);
        } else if inside(l.next, x, y) {
            self.turn(1, gw, gh);
        } else if let Some((k, fav)) = self.cell_at(&l, x, y) {
            if let Some(fi) = fav {
                // The Favs tab: a long press removes, a tap opens (or gives, in Give mode).
                let f = self.favs[fi].clone();
                if long {
                    self.toggle(f);
                } else if let (Fav::Item(_), Mode::Give) = (&f, self.mode) {
                    return Some(Out::Give(k.stack(1)));
                } else {
                    self.open_fav(&f, gw, gh);
                }
                return None;
            }
            match self.mode {
                Mode::Give => return Some(Out::Give(k.stack(if long { items::max_stack(k.id) } else { 1 }))),
                Mode::Fav => self.toggle(Fav::Item(k)),
                Mode::View => self.open(k, long),
            }
        }
        None
    }

    fn tap_view(&mut self, x: f32, y: f32, long: bool, gw: f32, gh: f32) -> Option<Out> {
        let give = self.mode == Mode::Give;
        let (back, star_r, one, stack) = Self::header(gw);
        let (target, page, ntabs, disps) = {
            let v = self.view.as_ref()?;
            (v.target, v.page, v.tabs.len(), v.disps().to_vec())
        };
        if inside(back, x, y) {
            self.view = self.hist.pop();
            return None;
        }
        if inside(star_r, x, y) {
            self.toggle(Fav::Item(target));
            return None;
        }
        if give && inside(one, x, y) {
            return Some(Out::Give(target.stack(1)));
        }
        if give && inside(stack, x, y) {
            return Some(Out::Give(target.stack(items::max_stack(target.id))));
        }
        let (_, _, tabs, prev, next, _, top, per) = Self::vlay(gw, gh, ntabs);
        if let Some(i) = tabs.iter().position(|&r| inside(r, x, y)) {
            if let Some(v) = self.view.as_mut() {
                (v.cat, v.page) = (i, 0);
            }
        } else if inside(prev, x, y) {
            self.turn(-1, gw, gh);
        } else if inside(next, x, y) {
            self.turn(1, gw, gh);
        } else {
            let cx = (gw / 2.0).floor();
            for (i, d) in disps.iter().enumerate().skip(page * per).take(per) {
                let (dx, dy) = (cx - 75.0, top + (i - page * per) as f32 * 70.0);
                if inside(Self::star_at(dx, dy), x, y) {
                    self.toggle(Fav::Recipe(d.clone()));
                    return None;
                }
                if self.craft_gw > 0 && matches!(d, Disp::Craft { .. }) && inside(Self::plus_at(dx, dy), x, y) {
                    return self.plus(d, long);
                }
                for (r, k, _) in Self::slots(d, dx, dy) {
                    if inside(r, x, y) {
                        self.open(k, long);
                        return None;
                    }
                }
            }
        }
        None
    }

    /// What the screen fills, in px, for the pad's button hints to keep off.
    pub fn avoid(&self, w: f32, h: f32, st: &Settings) -> Vec<[f32; 4]> {
        let (s, gw, gh) = scaled_gs(w, h, st.gui_scale);
        vec![[0.0, 0.0, gw * s, gh * s]]
    }

    pub fn draw(&self, v: &mut Vec<HudVertex>, w: f32, h: f32, st: &Settings) {
        let (s, gw, gh) = scaled_gs(w, h, st.gui_scale);
        let th = (st.rei_theme as usize).min(1);
        quad(v, 1.0, [0.0, 0.0, w, h], [0.0, 0.0, 0.0, 0.7]);
        match &self.view {
            None => self.draw_list(v, s, gw, gh, th),
            Some(view) => self.draw_view(v, s, gw, gh, th, view),
        }
        if let Some((m, t)) = &self.notice {
            if t.elapsed().as_secs_f32() < 2.5 {
                let bw = tw(m) + 12.0;
                let r = [(gw - bw) / 2.0, gh - 52.0, bw, 16.0];
                quad(v, s, r, [0.5, 0.05, 0.05, 0.92]);
                centred(v, s, r, m, WHITE);
            }
        }
    }

    /// The held name, and under it what the entry is to the player: its mod, or the recipe a favorite stands for.
    fn tooltip(&self, v: &mut Vec<HudVertex>, s: f32, gw: f32, k: Key, extra: Option<String>) {
        let Some((fx, fy)) = self.finger else { return };
        let lines = [(k.name(), WHITE), (extra.unwrap_or_else(|| format!("@{}", k.ns())), GREY)];
        let bw = lines.iter().map(|l| tw(&l.0)).fold(0.0, f32::max) + 8.0;
        let bh = 6.0 + 10.0 * lines.len() as f32;
        let (x, y) = ((fx - bw / 2.0).clamp(2.0, gw - bw - 2.0), (fy - 44.0).max(2.0));
        quad(v, s, [x, y, bw, bh], [0.06, 0.0, 0.1, 0.92]);
        quad(v, s, [x, y, bw, 1.0], [0.3, 0.1, 0.6, 1.0]);
        for (i, (t, c)) in lines.iter().enumerate() {
            text(v, s, x + 4.0, y + 4.0 + 10.0 * i as f32, t, *c);
        }
    }

    fn draw_list(&self, v: &mut Vec<HudVertex>, s: f32, gw: f32, gh: f32, th: usize) {
        let l = lay(gw, gh);
        nine(v, s, th, sp::DEFAULT, [4.0, 4.0, gw - 8.0, gh - 8.0]);
        button(v, s, th, l.close, "Close", false, true);
        nine(v, s, th, sp::SLOT, l.search);
        let q = if self.query.is_empty() && !self.typing { "Search...  @mod  #tag".to_string() } else { format!("{}{}", self.query, if self.typing { "_" } else { "" }) };
        let fit = crate::ui::fit(&q, l.search[2] - 10.0);
        text(v, s, l.search[0] + 5.0, l.search[1] + 6.0, &fit, if self.query.is_empty() && !self.typing { GREY } else { WHITE });
        let mode = match self.mode {
            Mode::View => "Mode: View",
            Mode::Give => "Mode: Give",
            Mode::Fav => "Mode: Fav",
        };
        button(v, s, th, l.mode, mode, self.mode != Mode::View, true);
        let pages = self.pages(&l);
        let page = self.cur(&l);
        button(v, s, th, l.prev, "<", false, pages > 1);
        button(v, s, th, l.next, ">", false, pages > 1);
        nine(v, s, th, if self.tab_favs { sp::LIGHTER } else { sp::DEFAULT }, l.favs);
        star(v, s, th, l.favs[0] + 6.0, l.favs[1] + 5.5, 9.0, self.tab_favs);
        text(v, s, l.favs[0] + 19.0, l.favs[1] + 6.0, "Favs", WHITE);
        let unit = if self.tab_favs { "favorites" } else { "items" };
        centred(v, s, l.label, &format!("{}/{}  ({} {})", page + 1, pages, self.len(), unit), WHITE);
        let per = Self::per_page(&l);
        let mut under = None;
        for i in 0..per {
            let Some((k, fav)) = self.cell(page * per + i) else { break };
            let (x, y) = (l.gx + (i % l.cols) as f32 * CELL, l.gy + (i / l.cols) as f32 * CELL);
            nine(v, s, th, sp::SLOT, [x + 1.0, y + 1.0, 18.0, 18.0]);
            icon(v, s, x + 2.0, y + 2.0, k.stack(1));
            let mut extra = None;
            match fav.map(|f| &self.favs[f]) {
                // A favorite recipe wears its category's icon.
                Some(Fav::Recipe(d)) => {
                    if let Some(c) = category(d.cat()) {
                        crate::push_stack(v, ((x + 10.0) * s, (y + 10.0) * s, 8.0 * s, 8.0 * s), 1.0, ItemStack { id: c.tab, count: 1, damage: 0 });
                        extra = Some(format!("Recipe: {}", c.name));
                    }
                }
                Some(Fav::Item(_)) => {}
                None => {
                    if self.is_fav(&Fav::Item(k)) {
                        star(v, s, th, x + 12.0, y + 2.0, 6.0, true);
                    }
                }
            }
            if self.finger.is_some_and(|(fx, fy)| inside([x, y, CELL, CELL], fx, fy)) {
                quad(v, s, [x + 2.0, y + 2.0, 16.0, 16.0], [1.0, 1.0, 1.0, 0.35]);
                under = Some((k, extra));
            }
        }
        if self.len() == 0 {
            if self.tab_favs {
                centred(v, s, [0.0, gh / 2.0 - 10.0, gw, 8.0], "No favorites yet", GREY);
                centred(v, s, [0.0, gh / 2.0 + 2.0, gw, 8.0], "Mode: Fav, or the star in a recipe", GREY);
            } else {
                centred(v, s, [0.0, gh / 2.0 - 4.0, gw, 8.0], "No items match", GREY);
            }
        }
        if let Some((k, extra)) = under {
            self.tooltip(v, s, gw, k, extra);
        }
    }

    fn draw_view(&self, v: &mut Vec<HudVertex>, s: f32, gw: f32, gh: f32, th: usize, view: &View) {
        let cx = (gw / 2.0).floor();
        let (back, star_r, one, stack) = Self::header(gw);
        button(v, s, th, back, "Back", false, true);
        let on = self.is_fav(&Fav::Item(view.target));
        nine(v, s, th, if on { sp::LIGHTER } else { sp::DEFAULT }, star_r);
        star(v, s, th, star_r[0] + 5.5, star_r[1] + 5.5, 9.0, on);
        let title = format!("{}: {}", if view.uses { "Uses" } else { "Recipes" }, view.target.name());
        icon(v, s, cx - (tw(&title) + 20.0) / 2.0, 10.0, view.target.stack(1));
        text(v, s, cx - (tw(&title) + 20.0) / 2.0 + 20.0, 12.0, &title, WHITE);
        if self.mode == Mode::Give {
            button(v, s, th, one, "+1", false, true);
            button(v, s, th, stack, "Stack", false, true);
        }
        let (panel, work, tabs, prev, next, label, top, per) = Self::vlay(gw, gh, view.tabs.len());
        nine(v, s, th, sp::DEFAULT, panel);
        let Some((cat, disps)) = view.tabs.get(view.cat) else {
            centred(v, s, [panel[0], panel[1] + 40.0, panel[2], 8.0], "No recipes", GREY);
            return;
        };
        for (i, (r, t)) in tabs.iter().zip(&view.tabs).enumerate() {
            nine(v, s, th, if i == view.cat { sp::LIGHTER } else { sp::DEFAULT }, *r);
            icon(v, s, r[0] + 4.0, r[1] + 4.0, ItemStack { id: t.0.tab, count: 1, damage: 0 });
        }
        // `categoryBack` / `recipeBack`: the page row, and the workstation panel at the left.
        let pages = disps.len().div_ceil(per).max(1);
        button(v, s, th, prev, "<", false, pages > 1);
        button(v, s, th, next, ">", false, pages > 1);
        centred(v, s, label, &format!("{}  {}/{}", cat.name, view.page + 1, pages), WHITE);
        if let Some(&wk) = cat.work.first() {
            nine(v, s, th, sp::DEFAULT, work);
            icon(v, s, work[0] + 4.0, work[1] + 4.0, ItemStack { id: wk, count: 1, damage: 0 });
        }
        let mut under = None;
        for (i, d) in disps.iter().skip(view.page * per).take(per).enumerate() {
            let (x, y) = (cx - 75.0, top + i as f32 * 70.0);
            nine(v, s, th, sp::LIGHTER, [x, y, 150.0, 66.0]);
            let arrow = match d {
                Disp::Craft { .. } => Some((x + 17.0 + 60.0, y + 6.0 + 18.0)),
                Disp::Smelt { .. } => Some((x + 63.0, y + 25.0)),
                Disp::Info { cat, .. } if *cat != "tag" => Some((x + 62.0, y + 25.0)),
                _ => None,
            };
            if let Some((ax, ay)) = arrow {
                HudPipeline::push_gui(v, ax * s, ay * s, 22.0 * s, 16.0 * s, sp::arrow(th));
            }
            match d {
                Disp::Fuel { ticks, .. } => text(v, s, x + 50.0, y + 29.0, &format!("Burns {} items", *ticks as f32 / 200.0), WHITE),
                Disp::Info { cat, note, .. } if *cat == "tag" => {
                    text(v, s, x + 6.0, y + 4.0, note, WHITE);
                    if let Some(t) = tag_by_id(note) {
                        let total = members(t).len();
                        if total > TAG_SHOWN {
                            text(v, s, x + 100.0, y + 52.0, &format!("+{} more", total - TAG_SHOWN), GREY);
                        }
                    }
                }
                Disp::Info { note, .. } if !note.is_empty() => text(v, s, x + 6.0, y + 4.0, note, GREY),
                _ => {}
            }
            let fav = self.is_fav(&Fav::Recipe(d.clone()));
            star(v, s, th, x + 137.0, y + 4.0, 9.0, fav);
            if self.craft_gw > 0 {
                if let Disp::Craft { w, cells, .. } = d {
                    button(v, s, th, Self::plus_at(x, y), "+", false, self.cant_craft(*w, cells).is_none());
                }
            }
            for (r, k, big) in Self::slots(d, x, y) {
                nine(v, s, th, sp::SLOT, r);
                let count = if let (true, Disp::Craft { out, .. } | Disp::Smelt { out, .. }) = (big, d) { out.count } else { 1 };
                icon(v, s, r[0] + (r[2] - 16.0) / 2.0, r[1] + (r[3] - 16.0) / 2.0, k.stack(count));
                if self.craft_gw > 0 && !big && self.short(k, d) {
                    quad(v, s, [r[0] + 1.0, r[1] + 1.0, r[2] - 2.0, r[3] - 2.0], RED);
                }
                if self.finger.is_some_and(|(fx, fy)| inside(r, fx, fy)) {
                    quad(v, s, [r[0] + 1.0, r[1] + 1.0, r[2] - 2.0, r[3] - 2.0], [1.0, 1.0, 1.0, 0.3]);
                    under = Some(k);
                }
            }
        }
        if let Some(k) = under {
            self.tooltip(v, s, gw, k, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(d: &[(&'static Category, Vec<Disp>)], cat: &str) -> usize {
        d.iter().find(|t| t.0.id == cat).map_or(0, |t| t.1.len())
    }

    #[test]
    fn the_catalog_has_no_liquids_and_every_wool() {
        let c = catalog();
        assert!(c.iter().all(|k| !SKIP.contains(&k.id)));
        assert_eq!(c.iter().filter(|k| k.id == 35).count(), 16);
        assert!(c.contains(&Key { id: 58, dmg: 0 }));
    }

    #[test]
    fn a_workbench_has_a_recipe_and_planks_are_used() {
        assert!(ids(&displays(Key { id: 58, dmg: 0 }, false), "crafting") > 0, "crafting table is crafted");
        assert!(ids(&displays(Key { id: 5, dmg: 0 }, true), "crafting") > 0, "planks are an ingredient");
        assert!(ids(&displays(Key { id: 263, dmg: 0 }, true), "fuel") > 0, "coal is a fuel");
        assert!(ids(&displays(Key { id: 265, dmg: 0 }, false), "smelting") > 0, "iron ingots are smelted");
    }

    #[test]
    fn the_new_categories_find_drops_and_tags() {
        // Cobblestone is what stone drops, and stone drops it.
        let r = displays(Key { id: 4, dmg: 0 }, false);
        assert!(r.iter().any(|t| t.0.id == "drops" && t.1.iter().any(|d| matches!(d, Disp::Info { ins, .. } if ins == &vec![Key { id: 1, dmg: 0 }]))));
        assert!(ids(&displays(Key { id: 1, dmg: 0 }, true), "drops") > 0);
        // A stone sword is in the swords tag, which lists it.
        let t = displays(Key { id: 272, dmg: 0 }, false);
        assert!(t.iter().any(|t| t.0.id == "tag" && t.1.iter().any(|d| matches!(d, Disp::Info { note, outs, .. } if note == "minecraft:swords" && outs.contains(&Key { id: 272, dmg: 0 })))));
    }

    #[test]
    fn a_registered_category_shows_up() {
        fn find(k: Key, _: bool) -> Vec<Disp> {
            vec![Disp::Info { cat: "test_cat", ins: vec![k], outs: vec![k], note: "hi".into() }]
        }
        static C: Category = Category { id: "test_cat", name: "Test", tab: 1, work: &[], find };
        register_category(&C);
        register_category(&C); // twice is once
        assert_eq!(categories().iter().filter(|c| c.id == "test_cat").count(), 1);
        assert_eq!(ids(&displays(Key { id: 1, dmg: 0 }, false), "test_cat"), 1);
    }

    #[test]
    fn search_filters_and_pages_wrap() {
        let mut ui = ReiUi::new(false, Ctx::default());
        let all = ui.list.len();
        "stone".chars().for_each(|c| ui.type_char(c));
        assert!(ui.list.len() < all && !ui.list.is_empty());
        let l = lay(427.0, 240.0);
        assert!(ui.pages(&l) >= 1);
    }

    #[test]
    fn mod_tag_and_exclusion_search() {
        let q = |s: &str| catalog().iter().copied().filter(|&k| matches(k, &parse_query(s))).collect::<Vec<_>>();
        let cortes = q("@cortes");
        assert!(!cortes.is_empty() && cortes.iter().all(|k| (400..=430).contains(&k.id)));
        assert!(q("@minecraft").iter().all(|k| k.id < 400 || k.id > 430) && !q("@minecraft").is_empty());
        assert!(q("#ores").iter().all(|k| matches!(k.id, 14 | 15 | 16 | 21 | 56 | 73)));
        assert!(q("#minecraft:swords").contains(&Key { id: 268, dmg: 0 }));
        assert!(q("#swords @minecraft").len() == q("#swords").len(), "no sword of the pack is in the tag");
        assert_eq!(q("wool -white").len() + 1, q("wool").len(), "-white drops the one white wool");
        assert_eq!(q("@").len(), catalog().len(), "an empty term lets everything through");
    }

    #[test]
    fn favorites_round_trip_and_survive_garbage() {
        let favs = vec![
            Fav::Item(Key { id: 35, dmg: 3 }),
            Fav::Recipe(Disp::Craft { w: 2, cells: [5, 5, 5, 5, 0, 0, 0, 0, 0], out: ItemStack { id: 58, count: 1, damage: 0 } }),
            Fav::Recipe(Disp::Smelt { inp: Key { id: 15, dmg: 0 }, out: ItemStack { id: 265, count: 1, damage: 0 } }),
            Fav::Recipe(Disp::Fuel { item: Key { id: 263, dmg: 0 }, ticks: 1600 }),
            Fav::Recipe(Disp::Info { cat: "drops", ins: vec![Key { id: 1, dmg: 0 }], outs: vec![Key { id: 4, dmg: 0 }], note: String::new() }),
            Fav::Recipe(Disp::Info { cat: "tag", ins: Vec::new(), outs: vec![Key { id: 14, dmg: 0 }, Key { id: 15, dmg: 0 }], note: "minecraft:ores".into() }),
        ];
        assert_eq!(parse_favs(&dump_favs(&favs)), favs);
        let junk = "i 1.0\nzzz\nc 9 1.0.1 0\nx nosuch - - -\ni 99999999.0\ni 1.0 extra\ni 1.0\n\n";
        assert_eq!(parse_favs(junk), vec![Fav::Item(Key { id: 1, dmg: 0 })], "bad lines go, a duplicate is one");
        let many: String = (1..200).map(|i| format!("i {i}.0\n")).collect();
        assert_eq!(parse_favs(&many).len(), FAV_MAX);
    }

    #[test]
    fn favorites_are_kept_per_world_folder() {
        let dir = std::env::temp_dir().join(format!("rei_fav_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut ui = ReiUi::new(false, Ctx { dir: Some(dir.clone()), ..Ctx::default() });
        ui.toggle(Fav::Item(Key { id: 276, dmg: 0 }));
        ui.toggle(Fav::Item(Key { id: 1, dmg: 0 }));
        ui.toggle(Fav::Item(Key { id: 1, dmg: 0 }));
        let again = ReiUi::new(false, Ctx { dir: Some(dir.clone()), ..Ctx::default() });
        assert_eq!(again.favs, vec![Fav::Item(Key { id: 276, dmg: 0 })]);
        let other = ReiUi::new(false, Ctx::default());
        assert!(other.favs.is_empty(), "another world has none");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_favs_tab_follows_the_search() {
        let mut ui = ReiUi::new(false, Ctx::default());
        ui.toggle(Fav::Item(Key { id: 276, dmg: 0 }));
        ui.toggle(Fav::Item(Key { id: 4, dmg: 0 }));
        ui.tab_favs = true;
        assert_eq!(ui.len(), 2);
        "cobble".chars().for_each(|c| ui.type_char(c));
        assert_eq!(ui.len(), 1);
        assert_eq!(ui.cell(0).map(|c| c.0), Some(Key { id: 4, dmg: 0 }));
    }

    #[test]
    fn auto_crafting_needs_a_grid_and_the_stock() {
        let table = [5, 5, 5, 5, 0, 0, 0, 0, 0];
        let chest = [5, 5, 5, 5, 0, 5, 5, 5, 5];
        let none = ReiUi::new(false, Ctx::default());
        assert!(none.cant_craft(2, &table).is_some(), "no grid, no stock");
        let ui = ReiUi::new(false, Ctx { craft_gw: 2, have: vec![(5, 4)], ..Ctx::default() });
        assert_eq!(ui.cant_craft(2, &table), None);
        assert_eq!(ui.cant_craft(3, &chest).as_deref(), Some("Needs a crafting table"));
        let big = ReiUi::new(false, Ctx { craft_gw: 3, have: vec![(5, 4)], ..Ctx::default() });
        assert!(big.cant_craft(3, &chest).unwrap().starts_with("Missing"));
        assert!(ui.short(Key { id: 5, dmg: 0 }, &Disp::Craft { w: 3, cells: chest, out: ItemStack { id: 54, count: 1, damage: 0 } }));
    }
}
