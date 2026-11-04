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
use crate::rei_layout::{self, around, bottom_row, ease_out, pages, shift_target, Strip, BTN, CELL, WANT_RIGHT};
use crate::world::craft;
use crate::world::items::{self, ItemStack};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

const LONG_SECS: f32 = 0.45;
const STAND_X: f32 = 72.0;
const SLIDE_SECS: f32 = 0.25;
const SWIPE_SECS: f32 = 0.18;
const ITEM_H: f32 = 20.0;
const DIM: [f32; 4] = [0.0, 0.0, 0.0, 0.6];
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
    /// Leave the viewer (the standalone one, from Tools).
    Close,
    Give(ItemStack),
    /// Auto-crafting: lay this recipe (`w` x rows of `cells` codes, `w` 0 = shapeless) out on the open grid; `max` = as many sets as possible.
    Fill { w: usize, cells: [u16; 9], max: bool },
    /// The config menu's theme switch: 0 light, 1 dark (`Settings::rei_theme`).
    Theme(u8),
    /// The config menu's "Hide REI": the overlay goes until the R button brings it back.
    Hide,
}

/// What `ReiUi::event` tells the game: whether the viewer took the finger (the game's own screen handling then does not see it), and what it asks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reaction {
    pub taken: bool,
    pub out: Option<Out>,
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

/// Where a finger went down: it decides what a drag pages.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Region {
    None,
    Right,
    Left,
    View,
}

struct Press {
    at: (f32, f32),
    t: Instant,
    drag: bool,
    region: Region,
}

/// What a menu row does.
#[derive(Clone, Debug, PartialEq)]
enum Act {
    Cheat,
    Theme,
    Hide,
    ClearSearch,
    Recipes(Key),
    Uses(Key),
    Give(Key, bool),
    Toggle(Fav),
    OpenFav(Fav),
}

/// A popup (REI's context menu): the config menu above the gear, or what a long press on an entry offers.
struct Menu {
    rect: [f32; 4],
    items: Vec<(String, Act)>,
}

impl Menu {
    /// Centred on `ax`, above `ay` (below when there is no room), kept on the screen.
    fn new(ax: f32, ay: f32, items: Vec<(String, Act)>, gw: f32, gh: f32) -> Self {
        let w = items.iter().map(|i| tw(&i.0)).fold(0.0f32, f32::max).max(60.0) + 20.0;
        let h = items.len() as f32 * ITEM_H + 8.0;
        let y = if ay - h - 6.0 >= 2.0 { ay - h - 6.0 } else { ay + 22.0 };
        Menu { rect: [(ax - w / 2.0).clamp(2.0, (gw - w - 2.0).max(2.0)), y.clamp(2.0, (gh - h - 2.0).max(2.0)), w, h], items }
    }
    fn item_rect(&self, i: usize) -> [f32; 4] {
        [self.rect[0] + 4.0, self.rect[1] + 4.0 + i as f32 * ITEM_H, self.rect[2] - 8.0, ITEM_H]
    }
    fn at(&self, x: f32, y: f32) -> Option<usize> {
        (0..self.items.len()).find(|&i| inside(self.item_rect(i), x, y))
    }
}

/// The overlay's page of the world, as of this frame (GUI units).
struct Geo {
    s: f32,
    gw: f32,
    gh: f32,
    th: usize,
    /// How far the screen is pushed now (the recipe panel follows it).
    cur: f32,
    right: Option<Strip>,
    left: Option<Strip>,
    search: [f32; 4],
    filter: [f32; 4],
    config: [f32; 4],
}

/// The recipe view's rects.
struct VGeo {
    panel: [f32; 4],
    tabs: Vec<(usize, [f32; 4])>,
    tab_prev: Option<[f32; 4]>,
    tab_next: Option<[f32; 4]>,
    cat_prev: [f32; 4],
    cat_next: [f32; 4],
    cat_label: [f32; 4],
    rec_prev: [f32; 4],
    rec_next: [f32; 4],
    rec_label: [f32; 4],
    work: [f32; 4],
    top: f32,
    per: usize,
}

/// The bottom-left buttons: back one view, leave the views, leave the viewer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Bl {
    Back,
    CloseView,
    Exit,
}

pub struct ReiUi {
    /// REI's cheat mode: a tap gives, instead of showing recipes.
    cheat: bool,
    /// Opened from Tools: no container screen beside it.
    standalone: bool,
    /// Only what the player can craft from what they carry (`CraftableFilterButtonWidget`).
    craftable: bool,
    /// The favorites as a tab of the entry list (when the left strip has no room).
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
    fpage: usize,
    tab_page: usize,
    view: Option<View>,
    hist: Vec<View>,
    press: Option<Press>,
    finger: Option<(f32, f32)>,
    notice: Option<(String, Instant)>,
    menu: Option<Menu>,
    /// Whether the finger that is down was taken by the viewer.
    grab: bool,
    opened: Instant,
    /// The live offset of a page swipe, and the ease back / in after it.
    drag: (Region, f32),
    swipe: Option<(Region, f32, Instant)>,
    /// The touch HUD's pause and inventory buttons (px): the overlay keeps clear of them.
    hud: Option<[f32; 4]>,
}

fn inside(r: [f32; 4], x: f32, y: f32) -> bool {
    rei_layout::inside(r, x, y)
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

/// A nine-slice of the pack over `r` (GUI units): the sprite's top-left `(sx, sy)`, its border `b` (the sprite is `3 * b` square).
fn slice(v: &mut Vec<HudVertex>, s: f32, (sx, sy): (f32, f32), b: f32, r: [f32; 4]) {
    let (x, y, w, h) = (r[0], r[1], r[2].max(2.0 * b), r[3].max(2.0 * b));
    let cols = [(0.0, x, b), (b, x + b, w - 2.0 * b), (2.0 * b, x + w - b, b)];
    let rows = [(0.0, y, b), (b, y + b, h - 2.0 * b), (2.0 * b, y + h - b, b)];
    for &(cy, dy, dh) in &rows {
        for &(cx, dx, dw) in &cols {
            HudPipeline::push_gui(v, dx * s, dy * s, dw * s, dh * s, (sx + cx, sy + cy, b, b));
        }
    }
}

/// A panel of the pack (border 4).
fn nine(v: &mut Vec<HudVertex>, s: f32, theme: usize, kind: usize, r: [f32; 4]) {
    let (sx, sy, ..) = sp::panel(theme, kind);
    slice(v, s, (sx, sy), 4.0, r);
}

/// A button face (border 2): normal, lit or disabled.
fn face(v: &mut Vec<HudVertex>, s: f32, theme: usize, kind: usize, r: [f32; 4]) {
    let (sx, sy, ..) = sp::button(theme, kind);
    slice(v, s, (sx, sy), 2.0, r);
}

/// A button with a label; it lights while a finger is on it.
fn button(v: &mut Vec<HudVertex>, s: f32, theme: usize, r: [f32; 4], label: &str, lit: bool, enabled: bool) {
    face(v, s, theme, if !enabled { sp::BTN_OFF } else if lit { sp::BTN_LIT } else { sp::BTN }, r);
    centred(v, s, r, label, if enabled { WHITE } else { GREY });
}

/// A button with REI's left / right arrow.
fn arrow_button(v: &mut Vec<HudVertex>, s: f32, theme: usize, r: [f32; 4], right: bool, lit: bool, enabled: bool) {
    face(v, s, theme, if !enabled { sp::BTN_OFF } else if lit { sp::BTN_LIT } else { sp::BTN }, r);
    let a = if enabled { [1.0; 4] } else { [1.0, 1.0, 1.0, 0.4] };
    HudPipeline::push_gui_tinted(v, (r[0] + (r[2] - 8.0) / 2.0) * s, (r[1] + (r[3] - 8.0) / 2.0) * s, 8.0 * s, 8.0 * s, sp::arrow_lr(theme, right), a);
}

fn icon(v: &mut Vec<HudVertex>, s: f32, x: f32, y: f32, st: ItemStack) {
    crate::push_stack(v, (x * s, y * s, 16.0 * s, 16.0 * s), 1.0, st);
}

/// The favorite star (the sprite is 9 x 9) drawn `size` wide at (x, y).
fn star(v: &mut Vec<HudVertex>, s: f32, th: usize, x: f32, y: f32, size: f32, on: bool) {
    HudPipeline::push_gui(v, x * s, y * s, size * s, size * s, sp::star(th, on));
}

/// How many displays a view page holds (70 each: 66 and a gap) in a panel of the screen's height.
fn per_view(gh: f32) -> usize {
    (((gh - 44.0 - 42.0 - 4.0) / 70.0).floor() as usize).max(1)
}

impl ReiUi {
    /// The viewer on its own (from Tools): `cheat` makes a tap give the item.
    pub fn new(cheat: bool, ctx: Ctx) -> Self {
        Self::make(cheat, true, ctx)
    }

    /// The overlay beside an open container screen (REI's normal look).
    pub fn overlay(ctx: Ctx) -> Self {
        Self::make(false, false, ctx)
    }

    fn make(cheat: bool, standalone: bool, ctx: Ctx) -> Self {
        let favs = ctx.dir.as_deref().map(load_favs).unwrap_or_default();
        let mut ui = Self {
            cheat,
            standalone,
            craftable: false,
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
            fpage: 0,
            tab_page: 0,
            view: None,
            hist: Vec::new(),
            press: None,
            finger: None,
            notice: None,
            menu: None,
            grab: false,
            opened: Instant::now(),
            drag: (Region::None, 0.0),
            swipe: None,
            hud: None,
        };
        ui.refresh_favs();
        ui
    }

    /// A line for the player in red at the bottom (the game uses it for a refused auto-craft).
    pub fn notice(&mut self, s: &str) {
        self.notice = Some((s.to_string(), Instant::now()));
    }

    /// The recipe view is open: it takes the container screen's place (the game draws the screen only while this is false).
    pub fn covers(&self) -> bool {
        self.view.is_some()
    }

    /// Back to the container screen: the recipe views close.
    pub fn close_views(&mut self) {
        self.view = None;
        self.hist.clear();
        self.menu = None;
    }

    /// Where the touch HUD's pause and inventory buttons are, px (their joint box).
    pub fn set_hud(&mut self, r: Option<[f32; 4]>) {
        self.hud = r;
    }

    /// What the player has and the grid that is open now (the game calls this each frame: the stock changes while the container is used).
    pub fn sync(&mut self, craft_gw: usize, have: Vec<(u16, u32)>) {
        self.craft_gw = craft_gw;
        if have != self.have {
            self.have = have;
            if self.craftable {
                self.refilter();
            }
        }
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

    /// Whether the player carries what one crafting recipe of `k` needs.
    fn can_make(&self, k: Key) -> bool {
        find_crafting(k, false).iter().any(|d| {
            let Disp::Craft { cells, .. } = d else { return false };
            let list: Vec<u16> = cells.iter().copied().filter(|&c| c != 0).collect();
            list.iter().all(|&c| self.have.iter().find(|h| h.0 == c).map_or(0, |h| h.1) >= list.iter().filter(|&&o| o == c).count() as u32)
        })
    }

    /// `EntryListSearchManager`: the entries the query lets through (see `parse_query`; with the filter on, only the craftable ones), and the favorites with them.
    fn refilter(&mut self) {
        let terms = parse_query(&self.query);
        let only = self.craftable;
        let mine: Vec<Key> = catalog().iter().copied().filter(|&k| matches(k, &terms)).collect();
        self.list = if only { mine.into_iter().filter(|&k| self.can_make(k)).collect() } else { mine };
        self.refresh_favs();
        self.page = 0;
        self.fpage = 0;
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

    // ---- the entry lists ----
    fn count(&self, favs: bool) -> usize {
        if favs { self.fav_idx.len() } else { self.list.len() }
    }

    /// The entry at place `i` of a list, and its place in `favs` when it is a favorite.
    fn cell(&self, favs: bool, i: usize) -> Option<(Key, Option<usize>)> {
        if favs {
            let f = *self.fav_idx.get(i)?;
            Some((fav_key(&self.favs[f]), Some(f)))
        } else {
            self.list.get(i).map(|&k| (k, None))
        }
    }

    /// The page shown: a removed favorite can leave the page past the end.
    fn cur_page(&self, favs: bool, pages: usize) -> usize {
        (if favs { self.fpage } else { self.page }).min(pages - 1)
    }

    /// The right strip shows the favorites instead of the entries (the Favs tab, only when the left strip has no room).
    fn right_favs(&self, g: &Geo) -> bool {
        self.tab_favs && g.left.is_none()
    }

    // ---- geometry ----
    /// The main panel where it rests (shift 0): the recipe view's, the open container's (`panel`, px, where it is now), or a stand-in for none.
    fn base_panel(&self, gw: f32, gh: f32, s: f32, panel: Option<[f32; 4]>, cur: f32) -> [f32; 4] {
        if self.view.is_some() {
            [(gw / 2.0).floor() - 95.0, 36.0, 190.0, gh - 44.0]
        } else if let Some(p) = panel {
            [p[0] / s - cur, p[1] / s, p[2] / s, p[3] / s]
        } else {
            [STAND_X.min(gw / 3.0), 0.0, 0.0, gh]
        }
    }

    /// How far the game should push the container panel, px (negative = left), so the entry list gets its room. The game eases the push (`rei_layout::ease_to`).
    pub fn target_shift_px(&self, w: f32, h: f32, st: &Settings, panel: Option<[f32; 4]>) -> f32 {
        let (s, gw, gh) = scaled_gs(w, h, st.gui_scale);
        let base = self.base_panel(gw, gh, s, panel, craft::shift() / s);
        shift_target(gw, base, WANT_RIGHT, 2.0) * s
    }

    fn geo(&self, w: f32, h: f32, st: &Settings, panel: Option<[f32; 4]>) -> Geo {
        let (s, gw, gh) = scaled_gs(w, h, st.gui_scale);
        let cur = craft::shift() / s;
        let base = self.base_panel(gw, gh, s, panel, cur);
        let tgt = shift_target(gw, base, WANT_RIGHT, 2.0);
        let hud = self.hud.map(|r| [r[0] / s, r[1] / s, r[2] / s, r[3] / s]);
        let (right, left) = around(gw, gh, [base[0] + tgt, base[1], base[2], base[3]], hud);
        let slide = (1.0 - ease_out(self.opened.elapsed().as_secs_f32(), SLIDE_SECS)) * 48.0;
        let (right, left) = (right.map(|r| r.moved(slide)), left.map(|l| l.moved(-slide)));
        let (search, filter, config) = right.map_or(([0.0; 4], [0.0; 4], [0.0; 4]), |r| bottom_row(r.area, gh));
        Geo { s, gw, gh, th: (st.rei_theme as usize).min(1), cur, right, left, search, filter, config }
    }

    /// The search field: on the bottom row, or while typing on top (the soft keyboard covers the bottom).
    fn search_rect(&self, g: &Geo) -> [f32; 4] {
        match (&g.right, self.typing) {
            (Some(r), true) => [r.area[0] + 1.0, 5.0, r.next[0] + r.next[2] - r.area[0] - 1.0, BTN],
            _ => g.search,
        }
    }

    /// The Favs tab button (only when the favorites have no strip of their own).
    fn star_tab(&self, g: &Geo) -> Option<[f32; 4]> {
        let r = g.right.as_ref()?;
        g.left.is_none().then(|| [r.prev[0] + BTN + 2.0, 5.0, BTN, BTN])
    }

    fn bottom_left(&self, g: &Geo) -> Vec<(Bl, [f32; 4], &'static str)> {
        let (y, mut x) = (g.gh - BTN - 4.0, 2.0);
        let mut out = Vec::new();
        if self.view.is_some() {
            if !self.hist.is_empty() {
                out.push((Bl::Back, [x, y, 46.0, BTN], "Back"));
                x += 48.0;
            }
            out.push((Bl::CloseView, [x, y, 46.0, BTN], "Close"));
        } else if self.standalone {
            out.push((Bl::Exit, [x, y, 46.0, BTN], "Exit"));
        }
        out
    }

    fn vgeo(&self, g: &Geo, ntabs: usize) -> VGeo {
        let panel = [(g.gw / 2.0).floor() - 95.0 + g.cur, 36.0, 190.0, g.gh - 44.0];
        let (x, y) = (panel[0], panel[1]);
        let paged = ntabs > 6;
        let per_t = if paged { 5 } else { 6 };
        let first = if paged { self.tab_page.min((ntabs - 1) / per_t) * per_t } else { 0 };
        let x0 = x + if paged { 24.0 } else { 4.0 };
        let tabs = (first..ntabs.min(first + per_t)).enumerate().map(|(j, i)| (i, [x0 + 28.0 * j as f32, y - 28.0, 28.0, 28.0])).collect();
        let top = y + 42.0;
        VGeo {
            panel,
            tabs,
            tab_prev: paged.then_some([x, y - 26.0, 22.0, 24.0]),
            tab_next: paged.then_some([x + 168.0, y - 26.0, 22.0, 24.0]),
            cat_prev: [x + 4.0, y + 4.0, 22.0, 16.0],
            cat_next: [x + 164.0, y + 4.0, 22.0, 16.0],
            cat_label: [x + 28.0, y + 4.0, 134.0, 16.0],
            rec_prev: [x + 4.0, y + 22.0, 22.0, 16.0],
            rec_next: [x + 164.0, y + 22.0, 22.0, 16.0],
            rec_label: [x + 28.0, y + 22.0, 134.0, 16.0],
            work: [x - 23.0, y + 11.0, 31.0, 26.0],
            top,
            per: per_view(g.gh),
        }
    }

    /// The star of a display (a touch-sized hit area in the top right corner) and its `+` (under the result).
    fn star_at(x: f32, y: f32) -> [f32; 4] {
        [x + 132.0, y, 18.0, 16.0]
    }
    fn plus_at(x: f32, y: f32) -> [f32; 4] {
        [x + 118.0, y + 51.0, 28.0, 14.0]
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
    fn open_fav(&mut self, f: &Fav, gh: f32) {
        let (k, uses) = match f {
            Fav::Item(k) => (*k, false),
            Fav::Recipe(d) => d.target(),
        };
        self.open(k, uses);
        let per = per_view(gh);
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

    // ---- fingers ----
    /// The live offset of the page a finger is dragging, or the ease of the page that just changed.
    fn off(&self, r: Region) -> f32 {
        if self.drag.0 == r && r != Region::None {
            return self.drag.1;
        }
        match self.swipe {
            Some((sr, from, t)) if sr == r => from * (1.0 - ease_out(t.elapsed().as_secs_f32(), SWIPE_SECS)),
            _ => 0.0,
        }
    }

    fn region_at(&self, g: &Geo, x: f32, y: f32) -> Region {
        if g.right.as_ref().is_some_and(|r| inside(r.area, x, y)) {
            Region::Right
        } else if g.left.as_ref().is_some_and(|l| inside(l.area, x, y)) {
            Region::Left
        } else if self.view.is_some() {
            Region::View
        } else {
            Region::None
        }
    }

    /// A finger event in px. `panel` is the open container's panel rect in px (`craft::panel_rect`), `None` when there is none. The reaction says whether the
    /// viewer took the finger and what it asks of the game.
    pub fn event(&mut self, ev: ScreenEv, w: f32, h: f32, st: &Settings, panel: Option<[f32; 4]>) -> Reaction {
        let g = self.geo(w, h, st, panel);
        let s = g.s;
        match ev {
            ScreenEv::Down(x, y) => {
                let p = (x / s, y / s);
                let on_hud = self.hud.is_some_and(|r| inside(r, x, y)) && self.menu.is_none();
                let region = if on_hud { Region::None } else { self.region_at(&g, p.0, p.1) };
                self.grab = !on_hud && (self.menu.is_some() || region != Region::None || self.view.is_some() || self.standalone);
                self.press = Some(Press { at: p, t: Instant::now(), drag: false, region });
                self.finger = Some(p);
                Reaction { taken: self.grab, out: None }
            }
            ScreenEv::Move(x, y) => {
                let p = (x / s, y / s);
                self.finger = Some(p);
                if let Some(pr) = self.press.as_mut() {
                    pr.drag |= (p.0 - pr.at.0).abs().max((p.1 - pr.at.1).abs()) > 6.0;
                    let (dx, dy) = (p.0 - pr.at.0, p.1 - pr.at.1);
                    if pr.drag && pr.region != Region::None && dx.abs() > dy.abs() && self.menu.is_none() {
                        self.drag = (pr.region, dx.clamp(-30.0, 30.0));
                    }
                }
                Reaction { taken: self.grab, out: None }
            }
            ScreenEv::Up(x, y) => {
                self.finger = None;
                let taken = std::mem::take(&mut self.grab);
                let drag = std::mem::replace(&mut self.drag, (Region::None, 0.0));
                let Some(pr) = self.press.take() else { return Reaction { taken, out: None } };
                if !taken {
                    return Reaction { taken, out: None };
                }
                let (gx, gy) = (x / s, y / s);
                if pr.drag && self.menu.is_none() {
                    let d = gx - pr.at.0;
                    if d.abs() > 40.0 && d.abs() > (gy - pr.at.1).abs() {
                        self.turn(pr.region, if d < 0.0 { 1 } else { -1 }, &g);
                        self.swipe = Some((pr.region, if d < 0.0 { 18.0 } else { -18.0 }, Instant::now()));
                    } else if drag.0 != Region::None {
                        self.swipe = Some((drag.0, drag.1, Instant::now()));
                    }
                    return Reaction { taken, out: None };
                }
                let out = self.tap(&g, gx, gy, pr.t.elapsed().as_secs_f32() >= LONG_SECS);
                Reaction { taken, out }
            }
        }
    }

    /// Page `d` steps in a region (wrapping, like REI's arrows).
    fn turn(&mut self, region: Region, d: i32, g: &Geo) {
        match region {
            Region::View => {
                let per = per_view(g.gh);
                if let Some(v) = self.view.as_mut() {
                    let n = v.disps().len().div_ceil(per).max(1) as i32;
                    v.page = (v.page as i32 + d).rem_euclid(n) as usize;
                }
            }
            Region::Right | Region::Left => {
                let (strip, favs) = if region == Region::Right { (g.right.as_ref(), self.right_favs(g)) } else { (g.left.as_ref(), true) };
                let Some(strip) = strip else { return };
                let n = pages(self.count(favs), strip.per_page()) as i32;
                let cur = self.cur_page(favs, n as usize) as i32;
                let p = (cur + d).rem_euclid(n) as usize;
                if favs { self.fpage = p } else { self.page = p }
            }
            Region::None => {}
        }
    }

    fn tap(&mut self, g: &Geo, x: f32, y: f32, long: bool) -> Option<Out> {
        // A menu eats the tap: a row runs, anywhere else closes it.
        if let Some(m) = self.menu.take() {
            return m.at(x, y).and_then(|i| self.run(m.items[i].1.clone(), g));
        }
        for (b, r, _) in self.bottom_left(g) {
            if inside(r, x, y) {
                match b {
                    Bl::Back => self.view = self.hist.pop(),
                    Bl::CloseView => {
                        self.view = None;
                        self.hist.clear();
                    }
                    Bl::Exit => return Some(Out::Close),
                }
                return None;
            }
        }
        if let Some(r) = g.right.as_ref().filter(|r| inside(r.area, x, y)) {
            return self.tap_right(g, *r, x, y, long);
        }
        if let Some(l) = g.left.as_ref().filter(|l| inside(l.area, x, y)) {
            let n = pages(self.count(true), l.per_page());
            if inside(l.prev, x, y) && n > 1 {
                self.turn(Region::Left, -1, g);
            } else if inside(l.next, x, y) && n > 1 {
                self.turn(Region::Left, 1, g);
            } else if let Some(i) = l.cell_at(x, y) {
                let page = self.cur_page(true, n);
                if let Some((k, fav)) = self.cell(true, page * l.per_page() + i) {
                    return self.entry_tap(k, fav, long, (x, y), g);
                }
            }
            return None;
        }
        if self.view.is_some() {
            return self.tap_view(g, x, y, long);
        }
        None
    }

    fn tap_right(&mut self, g: &Geo, r: Strip, x: f32, y: f32, long: bool) -> Option<Out> {
        let favs = self.right_favs(g);
        if inside(self.search_rect(g), x, y) {
            self.typing = true;
            return None;
        }
        self.typing = false;
        if inside(g.filter, x, y) {
            self.craftable = !self.craftable;
            self.refilter();
            let msg = if self.craftable { "Craftable only" } else { "All items" };
            self.notice(msg);
            return None;
        }
        if inside(g.config, x, y) {
            let items = vec![
                (format!("Cheating: {}", if self.cheat { "On" } else { "Off" }), Act::Cheat),
                (format!("Dark theme: {}", if g.th == 1 { "On" } else { "Off" }), Act::Theme),
                ("Clear search".to_string(), Act::ClearSearch),
                ("Hide REI".to_string(), Act::Hide),
            ];
            self.menu = Some(Menu::new(g.config[0] + g.config[2] / 2.0, g.config[1], items, g.gw, g.gh));
            return None;
        }
        if self.star_tab(g).is_some_and(|t| inside(t, x, y)) {
            self.tab_favs = !self.tab_favs;
            return None;
        }
        let n = pages(self.count(favs), r.per_page());
        if inside(r.prev, x, y) && n > 1 {
            self.turn(Region::Right, -1, g);
        } else if inside(r.next, x, y) && n > 1 {
            self.turn(Region::Right, 1, g);
        } else if let Some(i) = r.cell_at(x, y) {
            let page = self.cur_page(favs, n);
            if let Some((k, fav)) = self.cell(favs, page * r.per_page() + i) {
                return self.entry_tap(k, fav, long, (x, y), g);
            }
        }
        None
    }

    /// A tap on an entry: recipes (or the item itself with cheating on); a long press opens its menu.
    fn entry_tap(&mut self, k: Key, fav: Option<usize>, long: bool, at: (f32, f32), g: &Geo) -> Option<Out> {
        if long {
            self.menu = Some(self.entry_menu(k, fav, at, g));
            return None;
        }
        if let Some(fi) = fav {
            let f = self.favs[fi].clone();
            if let (Fav::Item(_), true) = (&f, self.cheat) {
                return Some(Out::Give(k.stack(1)));
            }
            self.open_fav(&f, g.gh);
            return None;
        }
        if self.cheat {
            return Some(Out::Give(k.stack(1)));
        }
        self.open(k, false);
        None
    }

    fn entry_menu(&self, k: Key, fav: Option<usize>, at: (f32, f32), g: &Geo) -> Menu {
        let mut items = Vec::new();
        let own = fav.map(|fi| self.favs[fi].clone());
        if let Some(f) = &own {
            items.push(("Open".to_string(), Act::OpenFav(f.clone())));
        }
        items.push(("Recipes".to_string(), Act::Recipes(k)));
        items.push(("Uses".to_string(), Act::Uses(k)));
        if self.cheat {
            items.push(("Give 1".to_string(), Act::Give(k, false)));
            items.push(("Give a stack".to_string(), Act::Give(k, true)));
        }
        let f = own.unwrap_or(Fav::Item(k));
        items.push((if self.is_fav(&f) { "Unfavorite" } else { "Favorite" }.to_string(), Act::Toggle(f)));
        Menu::new(at.0, at.1 - 8.0, items, g.gw, g.gh)
    }

    fn run(&mut self, a: Act, g: &Geo) -> Option<Out> {
        match a {
            Act::Cheat => self.cheat = !self.cheat,
            Act::Theme => return Some(Out::Theme(1 - g.th as u8)),
            Act::Hide => return Some(Out::Hide),
            Act::ClearSearch => {
                self.query.clear();
                self.refilter();
            }
            Act::Recipes(k) => self.open(k, false),
            Act::Uses(k) => self.open(k, true),
            Act::Give(k, stack) => return Some(Out::Give(k.stack(if stack { items::max_stack(k.id) } else { 1 }))),
            Act::Toggle(f) => self.toggle(f),
            Act::OpenFav(f) => self.open_fav(&f, g.gh),
        }
        None
    }

    fn tap_view(&mut self, g: &Geo, x: f32, y: f32, long: bool) -> Option<Out> {
        let (ntabs, page, disps) = {
            let v = self.view.as_ref()?;
            (v.tabs.len(), v.page, v.disps().to_vec())
        };
        let vg = self.vgeo(g, ntabs);
        if let Some(&(i, _)) = vg.tabs.iter().find(|t| inside(t.1, x, y)) {
            if let Some(v) = self.view.as_mut() {
                (v.cat, v.page) = (i, 0);
            }
            return None;
        }
        let per_t = 5;
        if vg.tab_prev.is_some_and(|r| inside(r, x, y)) {
            let n = ntabs.div_ceil(per_t);
            self.tab_page = (self.tab_page.min(n - 1) + n - 1) % n;
            return None;
        }
        if vg.tab_next.is_some_and(|r| inside(r, x, y)) {
            let n = ntabs.div_ceil(per_t);
            self.tab_page = (self.tab_page.min(n - 1) + 1) % n;
            return None;
        }
        for (r, d) in [(vg.cat_prev, ntabs as i32 - 1), (vg.cat_next, 1)] {
            if inside(r, x, y) && ntabs > 1 {
                if let Some(v) = self.view.as_mut() {
                    v.cat = (v.cat as i32 + d).rem_euclid(ntabs as i32) as usize;
                    v.page = 0;
                }
                return None;
            }
        }
        let pages_n = disps.len().div_ceil(vg.per).max(1);
        if inside(vg.rec_prev, x, y) && pages_n > 1 {
            self.turn(Region::View, -1, g);
            return None;
        }
        if inside(vg.rec_next, x, y) && pages_n > 1 {
            self.turn(Region::View, 1, g);
            return None;
        }
        if inside(vg.work, x, y) {
            let wk = self.view.as_ref().and_then(|v| v.tabs.get(v.cat)).and_then(|t| t.0.work.first().copied());
            if let Some(wk) = wk {
                self.open(Key { id: wk, dmg: 0 }, false);
            }
            return None;
        }
        for (i, d) in disps.iter().enumerate().skip(page * vg.per).take(vg.per) {
            let (dx, dy) = (vg.panel[0] + 20.0, vg.top + (i - page * vg.per) as f32 * 70.0);
            if inside(Self::star_at(dx, dy), x, y) {
                self.toggle(Fav::Recipe(d.clone()));
                return None;
            }
            if self.craft_gw > 0 && matches!(d, Disp::Craft { .. }) && inside(Self::plus_at(dx, dy), x, y) {
                return self.plus(d, long);
            }
            for (r, k, _) in Self::slots(d, dx, dy) {
                if inside(r, x, y) {
                    return self.entry_tap(k, None, long, (x, y), g);
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
}

/// What a finger rests on: the entry, and what to say about it besides its mod.
type Under = Option<(Key, Option<String>)>;

impl ReiUi {
    /// Draws the viewer. `panel` is the open container's panel rect in px as `craft::panel_rect` gives it now (`None` without one).
    pub fn draw(&self, v: &mut Vec<HudVertex>, w: f32, h: f32, st: &Settings, panel: Option<[f32; 4]>) {
        let g = self.geo(w, h, st, panel);
        let s = g.s;
        if self.view.is_some() || self.standalone {
            quad(v, 1.0, [0.0, 0.0, w, h], DIM);
        }
        let mut under = self.draw_right(v, &g);
        under = self.draw_left(v, &g).or(under);
        if let Some(view) = &self.view {
            under = self.draw_view(v, &g, view).or(under);
        }
        for (_, r, label) in self.bottom_left(&g) {
            button(v, s, g.th, r, label, self.lit(r), true);
        }
        if let Some(m) = &self.menu {
            nine(v, s, g.th, sp::DEFAULT, m.rect);
            for (i, (label, _)) in m.items.iter().enumerate() {
                let r = m.item_rect(i);
                face(v, s, g.th, if self.lit(r) { sp::BTN_LIT } else { sp::BTN }, r);
                text(v, s, r[0] + 6.0, r[1] + 6.0, label, WHITE);
            }
        } else if let Some((k, extra)) = under {
            self.tooltip(v, s, g.gw, k, extra);
        }
        if let Some((m, t)) = &self.notice {
            if t.elapsed().as_secs_f32() < 2.5 {
                let bw = tw(m) + 12.0;
                let r = [(g.gw - bw) / 2.0, g.gh - 56.0, bw, 16.0];
                quad(v, s, r, [0.5, 0.05, 0.05, 0.92]);
                centred(v, s, r, m, WHITE);
            }
        }
    }

    fn lit(&self, r: [f32; 4]) -> bool {
        self.finger.is_some_and(|(x, y)| inside(r, x, y))
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

    /// The page of a strip: items on the grid, no cell backgrounds (REI draws the entries bare); the one under a finger lights.
    fn draw_grid(&self, v: &mut Vec<HudVertex>, g: &Geo, r: &Strip, favs: bool, page: usize, region: Region) -> Under {
        let (s, th) = (g.s, g.th);
        let off = self.off(region);
        let per = r.per_page();
        let mut under = None;
        for i in 0..per {
            let Some((k, fav)) = self.cell(favs, page * per + i) else { break };
            let c = r.cell_rect(i);
            let (x, y) = (c[0] + off, c[1]);
            icon(v, s, x + 2.0, y + 2.0, k.stack(1));
            let mut extra = None;
            match fav.map(|f| &self.favs[f]) {
                // A favorite recipe wears its category's icon.
                Some(Fav::Recipe(d)) => {
                    if let Some(cat) = category(d.cat()) {
                        crate::push_stack(v, ((x + 10.0) * s, (y + 10.0) * s, 8.0 * s, 8.0 * s), 1.0, ItemStack { id: cat.tab, count: 1, damage: 0 });
                        extra = Some(format!("Recipe: {}", cat.name));
                    }
                }
                Some(Fav::Item(_)) => {}
                None => {
                    if self.is_fav(&Fav::Item(k)) {
                        star(v, s, th, x + 12.0, y + 2.0, 6.0, true);
                    }
                }
            }
            if self.lit([x, y, CELL, CELL]) && self.menu.is_none() {
                quad(v, s, [x + 1.0, y + 1.0, CELL - 2.0, CELL - 2.0], [1.0, 1.0, 1.0, 0.35]);
                under = Some((k, extra));
            }
        }
        if self.count(favs) == 0 {
            let gr = r.grid();
            let lines: &[&str] = if favs { &["No favorites", "Hold an entry"] } else if self.craftable { &["Nothing", "craftable"] } else { &["No items", "match"] };
            for (i, l) in lines.iter().enumerate() {
                let t = crate::ui::fit(l, gr[2]);
                centred(v, s, [gr[0], gr[1] + 6.0 + 11.0 * i as f32, gr[2], 8.0], &t, GREY);
            }
        }
        under
    }

    /// The entry list: page row on top, grid, and the bottom row (search, craftable filter, config) of REI's `BOTTOM_SIDE`.
    fn draw_right(&self, v: &mut Vec<HudVertex>, g: &Geo) -> Under {
        let r = g.right.as_ref()?;
        let (s, th) = (g.s, g.th);
        let favs = self.right_favs(g);
        let n = pages(self.count(favs), r.per_page());
        let page = self.cur_page(favs, n);
        if !self.typing {
            arrow_button(v, s, th, r.prev, false, self.lit(r.prev), n > 1);
            arrow_button(v, s, th, r.next, true, self.lit(r.next), n > 1);
            let mut label = r.label;
            if let Some(t) = self.star_tab(g) {
                face(v, s, th, if self.tab_favs || self.lit(t) { sp::BTN_LIT } else { sp::BTN }, t);
                star(v, s, th, t[0] + 5.5, t[1] + 5.5, 9.0, self.tab_favs);
                label[0] += BTN + 2.0;
                label[2] -= BTN + 2.0;
            }
            centred(v, s, label, &format!("{}/{}", page + 1, n), WHITE);
        }
        let under = self.draw_grid(v, g, r, favs, page, Region::Right);
        let sr = self.search_rect(g);
        nine(v, s, th, sp::SLOT, sr);
        let empty = self.query.is_empty() && !self.typing;
        let q = if empty { "Search...".to_string() } else { format!("{}{}", self.query, if self.typing { "_" } else { "" }) };
        // The tail of a long query is what the player is typing.
        let fit = if tw(&q) > sr[2] - 10.0 && !empty { crate::ui::fit(&q.chars().rev().collect::<String>(), sr[2] - 10.0).chars().rev().collect() } else { q };
        text(v, s, sr[0] + 5.0, sr[1] + 6.0, &fit, if empty { GREY } else { WHITE });
        face(v, s, th, if self.craftable || self.lit(g.filter) { sp::BTN_LIT } else { sp::BTN }, g.filter);
        icon(v, s, g.filter[0] + 2.0, g.filter[1] + 2.0, ItemStack { id: 58, count: 1, damage: 0 });
        if !self.craftable {
            quad(v, s, [g.filter[0] + 2.0, g.filter[1] + 2.0, 16.0, 16.0], [0.0, 0.0, 0.0, 0.35]);
        }
        face(v, s, th, if self.lit(g.config) { sp::BTN_LIT } else { sp::BTN }, g.config);
        HudPipeline::push_gui(v, (g.config[0] + 3.0) * s, (g.config[1] + 3.0) * s, 14.0 * s, 14.0 * s, sp::gear(th));
        under
    }

    /// The favorites strip left of the panel.
    fn draw_left(&self, v: &mut Vec<HudVertex>, g: &Geo) -> Under {
        let l = g.left.as_ref()?;
        let (s, th) = (g.s, g.th);
        let n = pages(self.count(true), l.per_page());
        let page = self.cur_page(true, n);
        arrow_button(v, s, th, l.prev, false, self.lit(l.prev), n > 1);
        arrow_button(v, s, th, l.next, true, self.lit(l.next), n > 1);
        centred(v, s, l.label, &format!("{}/{}", page + 1, n), WHITE);
        self.draw_grid(v, g, l, true, page, Region::Left)
    }

    /// REI's `DefaultDisplayViewingScreen`: the tabs above the panel, category and page rows inside it, the displays under them, the workstation at its left.
    fn draw_view(&self, v: &mut Vec<HudVertex>, g: &Geo, view: &View) -> Under {
        let (s, th) = (g.s, g.th);
        let vg = self.vgeo(g, view.tabs.len());
        let Some((cat, disps)) = view.tabs.get(view.cat) else {
            nine(v, s, th, sp::DEFAULT, vg.panel);
            centred(v, s, [vg.panel[0], vg.panel[1] + 40.0, vg.panel[2], 8.0], "No recipes", GREY);
            return None;
        };
        // The workstation panel sits behind the main one.
        if let Some(&wk) = cat.work.first() {
            nine(v, s, th, sp::DEFAULT, vg.work);
            nine(v, s, th, sp::SLOT, [vg.work[0] + 4.0, vg.work[1] + 4.0, 18.0, 18.0]);
            icon(v, s, vg.work[0] + 5.0, vg.work[1] + 5.0, ItemStack { id: wk, count: 1, damage: 0 });
        }
        nine(v, s, th, sp::DEFAULT, vg.panel);
        for (sel, r) in vg.tabs.iter().map(|(i, r)| (*i == view.cat, *r)) {
            HudPipeline::push_gui(v, r[0] * s, (r[1] + 2.0) * s, 28.0 * s, if sel { 30.0 } else { 26.0 } * s, sp::tab(th, sel));
        }
        for &(i, r) in &vg.tabs {
            icon(v, s, r[0] + 6.0, r[1] + 8.0, ItemStack { id: view.tabs[i].0.tab, count: 1, damage: 0 });
        }
        if let Some(r) = vg.tab_prev {
            arrow_button(v, s, th, r, false, self.lit(r), true);
        }
        if let Some(r) = vg.tab_next {
            arrow_button(v, s, th, r, true, self.lit(r), true);
        }
        let per = vg.per;
        let pages_n = disps.len().div_ceil(per).max(1);
        let multi = view.tabs.len() > 1;
        arrow_button(v, s, th, vg.cat_prev, false, self.lit(vg.cat_prev), multi);
        arrow_button(v, s, th, vg.cat_next, true, self.lit(vg.cat_next), multi);
        nine(v, s, th, sp::SLOT, [vg.cat_label[0], vg.cat_label[1], vg.cat_label[2], vg.cat_label[3]]);
        centred(v, s, vg.cat_label, cat.name, WHITE);
        arrow_button(v, s, th, vg.rec_prev, false, self.lit(vg.rec_prev), pages_n > 1);
        arrow_button(v, s, th, vg.rec_next, true, self.lit(vg.rec_next), pages_n > 1);
        nine(v, s, th, sp::SLOT, vg.rec_label);
        centred(v, s, vg.rec_label, &format!("{}/{}", view.page.min(pages_n - 1) + 1, pages_n), WHITE);
        let off = self.off(Region::View);
        let mut under = None;
        for (i, d) in disps.iter().skip(view.page * per).take(per).enumerate() {
            let (x, y) = (vg.panel[0] + 20.0 + off, vg.top + i as f32 * 70.0);
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
            if let Disp::Craft { w, .. } = d {
                // `DefaultCraftingCategory` draws all nine slots, empty ones too, and marks a shapeless recipe.
                for c in 0..9 {
                    nine(v, s, th, sp::SLOT, [x + 18.0 + (c % 3) as f32 * 18.0, y + 7.0 + (c / 3) as f32 * 18.0, 18.0, 18.0]);
                }
                if *w == 0 {
                    HudPipeline::push_gui(v, (x + 5.0) * s, (y + 5.0) * s, 8.0 * s, 8.0 * s, sp::shapeless(th));
                }
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
                    let p = Self::plus_at(x, y);
                    button(v, s, th, p, "+", self.lit(p), self.cant_craft(*w, cells).is_none());
                }
            }
            for (r, k, big) in Self::slots(d, x, y) {
                if big || !matches!(d, Disp::Craft { .. }) {
                    nine(v, s, th, sp::SLOT, r);
                }
                let count = if let (true, Disp::Craft { out, .. } | Disp::Smelt { out, .. }) = (big, d) { out.count } else { 1 };
                icon(v, s, r[0] + (r[2] - 16.0) / 2.0, r[1] + (r[3] - 16.0) / 2.0, k.stack(count));
                if self.craft_gw > 0 && !big && self.short(k, d) {
                    quad(v, s, [r[0] + 1.0, r[1] + 1.0, r[2] - 2.0, r[3] - 2.0], RED);
                }
                if self.lit(r) && self.menu.is_none() {
                    quad(v, s, [r[0] + 1.0, r[1] + 1.0, r[2] - 2.0, r[3] - 2.0], [1.0, 1.0, 1.0, 0.3]);
                    under = Some((k, None));
                }
            }
        }
        under
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
        assert!(pages(ui.count(false), 80) >= 1);
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
        assert_eq!(ui.count(true), 2);
        "cobble".chars().for_each(|c| ui.type_char(c));
        assert_eq!(ui.count(true), 1);
        assert_eq!(ui.cell(true, 0).map(|c| c.0), Some(Key { id: 4, dmg: 0 }));
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

    // ---- the overlay: fingers, menus, the push ----
    const W: f32 = 2400.0;
    const H: f32 = 1080.0;

    /// A viewer past its slide-in, over a panel in the middle of a 2400 x 1080 screen.
    fn overlay() -> (ReiUi, Settings, [f32; 4]) {
        let mut ui = ReiUi::overlay(Ctx::default());
        ui.opened = Instant::now() - std::time::Duration::from_secs(2);
        (ui, Settings::default(), [687.0, 30.0, 1026.0, 1020.0])
    }

    fn tap(ui: &mut ReiUi, st: &Settings, panel: Option<[f32; 4]>, x: f32, y: f32) -> Reaction {
        ui.event(ScreenEv::Down(x, y), W, H, st, panel);
        ui.event(ScreenEv::Up(x, y), W, H, st, panel)
    }

    /// The centre of the first entry cell of the right strip, in px.
    fn first_cell(ui: &ReiUi, st: &Settings, panel: [f32; 4]) -> (f32, f32) {
        let g = ui.geo(W, H, st, Some(panel));
        let r = g.right.unwrap().cell_rect(0);
        ((r[0] + 10.0) * g.s, (r[1] + 10.0) * g.s)
    }

    #[test]
    fn a_tap_on_an_entry_opens_its_recipes_and_the_viewer_takes_it() {
        let (mut ui, st, panel) = overlay();
        let (x, y) = first_cell(&ui, &st, panel);
        let r = tap(&mut ui, &st, Some(panel), x, y);
        assert!(r.taken && r.out.is_none());
        let v = ui.view.as_ref().expect("the recipes of the first entry");
        assert_eq!(v.target, catalog()[0]);
        assert!(ui.covers(), "the container screen steps aside");
    }

    #[test]
    fn a_tap_on_the_container_panel_is_the_games() {
        let (mut ui, st, panel) = overlay();
        let r = tap(&mut ui, &st, Some(panel), panel[0] + 500.0, panel[1] + 500.0);
        assert!(!r.taken && r.out.is_none() && ui.view.is_none());
        // and so is a tap on the touch HUD's buttons, though they lie over the strip
        ui.set_hud(Some([2000.0, 40.0, 340.0, 120.0]));
        assert!(!tap(&mut ui, &st, Some(panel), 2100.0, 100.0).taken);
    }

    #[test]
    fn cheating_gives_and_the_long_press_menu_offers_both() {
        let (mut ui, st, panel) = overlay();
        ui.cheat = true;
        let (x, y) = first_cell(&ui, &st, panel);
        let r = tap(&mut ui, &st, Some(panel), x, y);
        assert_eq!(r.out, Some(Out::Give(catalog()[0].stack(1))));
        let g = ui.geo(W, H, &st, Some(panel));
        assert!(ui.entry_tap(catalog()[0], None, true, (x / g.s, y / g.s), &g).is_none());
        let labels: Vec<&str> = ui.menu.as_ref().unwrap().items.iter().map(|i| i.0.as_str()).collect();
        assert_eq!(labels, ["Recipes", "Uses", "Give 1", "Give a stack", "Favorite"]);
        // a tap on a row runs it; a tap elsewhere only closes
        let m = ui.menu.as_ref().unwrap().item_rect(3);
        let out = tap(&mut ui, &st, Some(panel), (m[0] + 5.0) * g.s, (m[1] + 5.0) * g.s).out;
        assert_eq!(out, Some(Out::Give(catalog()[0].stack(items::max_stack(catalog()[0].id)))));
        assert!(ui.menu.is_none());
    }

    #[test]
    fn the_gear_opens_the_config_menu() {
        let (mut ui, st, panel) = overlay();
        let g = ui.geo(W, H, &st, Some(panel));
        let c = g.config;
        assert!(tap(&mut ui, &st, Some(panel), (c[0] + 5.0) * g.s, (c[1] + 5.0) * g.s).taken);
        let m = ui.menu.as_ref().expect("the menu is open");
        assert_eq!(m.items[1].1, Act::Theme);
        let r = m.item_rect(1);
        let out = tap(&mut ui, &st, Some(panel), (r[0] + 5.0) * g.s, (r[1] + 5.0) * g.s).out;
        assert_eq!(out, Some(Out::Theme(1)), "light becomes dark");
        // Cheating flips in place
        let c = g.config;
        tap(&mut ui, &st, Some(panel), (c[0] + 5.0) * g.s, (c[1] + 5.0) * g.s);
        let r = ui.menu.as_ref().unwrap().item_rect(0);
        tap(&mut ui, &st, Some(panel), (r[0] + 5.0) * g.s, (r[1] + 5.0) * g.s);
        assert!(ui.cheat);
    }

    #[test]
    fn a_swipe_turns_the_page_and_a_short_drag_does_not() {
        let (mut ui, st, panel) = overlay();
        let (x, y) = first_cell(&ui, &st, panel);
        ui.event(ScreenEv::Down(x, y), W, H, &st, Some(panel));
        ui.event(ScreenEv::Move(x - 100.0, y), W, H, &st, Some(panel));
        assert!(ui.off(Region::Right) < 0.0, "the page follows the finger");
        ui.event(ScreenEv::Up(x - 400.0, y), W, H, &st, Some(panel));
        assert_eq!(ui.page, 1);
        assert!(ui.view.is_none(), "a swipe is no tap");
        ui.event(ScreenEv::Down(x, y), W, H, &st, Some(panel));
        ui.event(ScreenEv::Up(x + 60.0, y), W, H, &st, Some(panel));
        assert_eq!(ui.page, 1, "a short drag turns nothing");
        ui.event(ScreenEv::Down(x, y), W, H, &st, Some(panel));
        ui.event(ScreenEv::Up(x + 400.0, y), W, H, &st, Some(panel));
        assert_eq!(ui.page, 0, "and back");
    }

    #[test]
    fn the_craftable_filter_keeps_what_the_stock_makes() {
        let mut ui = ReiUi::overlay(Ctx { have: vec![(5, 4)], ..Ctx::default() });
        let all = ui.list.len();
        ui.craftable = true;
        ui.refilter();
        assert!(ui.list.contains(&Key { id: 58, dmg: 0 }), "four planks make a crafting table");
        assert!(ui.list.len() < all && !ui.list.contains(&Key { id: 276, dmg: 0 }), "no diamond sword");
        ui.sync(2, vec![]);
        assert!(ui.list.is_empty() || !ui.list.contains(&Key { id: 58, dmg: 0 }), "the stock changed: the list follows");
    }

    #[test]
    fn the_recipe_view_has_back_close_and_tabs() {
        let (mut ui, st, panel) = overlay();
        ui.open(Key { id: 58, dmg: 0 }, false);
        ui.open(Key { id: 5, dmg: 0 }, true);
        let g = ui.geo(W, H, &st, None);
        let back = ui.bottom_left(&g).iter().find(|b| b.0 == Bl::Back).map(|b| b.1).expect("back shows after two views");
        assert!(tap(&mut ui, &st, Some(panel), (back[0] + 5.0) * g.s, (back[1] + 5.0) * g.s).taken);
        assert_eq!(ui.view.as_ref().map(|v| v.target), Some(Key { id: 58, dmg: 0 }));
        let close = ui.bottom_left(&g).iter().find(|b| b.0 == Bl::CloseView).map(|b| b.1).unwrap();
        tap(&mut ui, &st, Some(panel), (close[0] + 5.0) * g.s, (close[1] + 5.0) * g.s);
        assert!(ui.view.is_none() && !ui.covers());
        // tabs: the crafting table has crafting (first) and a block drops tab or more; a tap on the second selects it
        ui.open(Key { id: 4, dmg: 0 }, false);
        let n = ui.view.as_ref().unwrap().tabs.len();
        if n > 1 {
            let g = ui.geo(W, H, &st, None);
            let vg = ui.vgeo(&g, n);
            let r = vg.tabs[1].1;
            tap(&mut ui, &st, None, (r[0] + 10.0) * g.s, (r[1] + 10.0) * g.s);
            assert_eq!(ui.view.as_ref().unwrap().cat, 1);
        }
    }

    #[test]
    fn standalone_takes_every_finger_and_has_its_exit() {
        let mut ui = ReiUi::new(true, Ctx::default());
        ui.opened = Instant::now() - std::time::Duration::from_secs(2);
        let st = Settings::default();
        assert!(tap(&mut ui, &st, None, 5.0, 5.0).taken);
        let g = ui.geo(W, H, &st, None);
        let exit = ui.bottom_left(&g)[0];
        assert_eq!(exit.0, Bl::Exit);
        let out = tap(&mut ui, &st, None, (exit.1[0] + 5.0) * g.s, (exit.1[1] + 5.0) * g.s).out;
        assert_eq!(out, Some(Out::Close));
        assert!(g.left.is_some() && g.right.is_some(), "favorites left, entries right");
    }

    #[test]
    fn the_overlay_asks_for_room_only_on_a_narrow_screen() {
        let (ui, st, panel) = overlay();
        assert_eq!(ui.target_shift_px(W, H, &st, Some(panel)), 0.0, "a phone in landscape has plenty");
        // 1920 x 1080: a 1026 px panel centred leaves 447 px a side = 111 units at scale 4; the list wants 146
        let narrow = [447.0, 30.0, 1026.0, 1020.0];
        let t = ui.target_shift_px(1920.0, 1080.0, &st, Some(narrow));
        assert!(t < -100.0 && t > -400.0, "pushed left by what is missing: {t}");
    }
}
