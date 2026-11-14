//! Controlify's contextual button guide (isXander, LGPL-3.0-or-later): the two columns of "button + what it does now" in the corners.
//!
//! Ported from `RuleEngine`, `ContextualPredicate`, `ContextualStateContributors`, `GuideInstanceImpl`, `PrecomputedLines` and `GuideRenderer`:
//! * the rules and the fact definitions are Controlify's own JSON, turned into `guide_rules.rs` by `tools/gen_guide.py`; their ORDER is their
//!   precedence: the first rule that matches wins its key (the binding and the side), one that does not match leaves the key to a later one;
//! * a rule whose binding is unbound is skipped (the swap-hands and pick-block rules: b1.7.3 has no such buttons);
//! * a line is `glyph text` in the left column and `text glyph` in the right one, 15 units tall with 2 between; the columns sit 2 units from
//!   the side and 5 from the top (or the bottom); the in-game guide puts a 50% black box behind each line's text (`textContrast`);
//! * the glyphs are Controlify's controller fonts (`glyph_map.rs`, `sprites::pad`); the advance of a glyph is its width + 1, a space is 4.
//!
//! The vocabulary of the predicates is Controlify's (`minecraft:chest`, `#minecraft:doors`...): `block_ids`, `item_ids`, `entity_is` below say what each
//! name is in b1.7.3. A name b1.7.3 has no equivalent of (a bell, a crossbow, a happy ghast) matches nothing: its rule stays in, switched off.
//!
//! UNVERIFIED: the fact `can_elytra_fly` and the ones about an off hand, bundles, creative / spectator mode are false here (no such thing);
//! `looking_at_*` use the crosshair's block id and the entity under it (`InGame::hit`).

use super::glyph_map::{ADVANCE, CELL, INPUTS};
use super::guide_rules::{CONTAINER_FACTS, CONTAINER_RULES, IN_GAME_FACTS, IN_GAME_RULES};
use crate::render::hud::{sprites, HudPipeline, HudVertex};

/// `GuideVerbosity`: how much the guide says (`FULL` shows every rule, `MINIMAL` only the essential ones).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Verbosity {
    #[default]
    Full,
    Reduced,
    Minimal,
}

impl Verbosity {
    pub const NAMES: [&'static str; 3] = ["Full", "Reduced", "Minimal"];

    /// `getLevel`: 3, 2, 1.
    fn level(self) -> u8 {
        match self {
            Verbosity::Full => 3,
            Verbosity::Reduced => 2,
            Verbosity::Minimal => 1,
        }
    }

    pub fn from_index(i: u8) -> Self {
        [Verbosity::Full, Verbosity::Reduced, Verbosity::Minimal][i.min(2) as usize]
    }
}

// ---- the rule vocabulary (what `guide_rules.rs` is made of) ----

pub struct Rule {
    pub bind: &'static str,
    pub left: bool,
    pub when: Pred,
    pub text: &'static str,
}

pub enum Pred {
    Fact(&'static str),
    Static(bool),
    Compound { all: &'static [Pred], none: &'static [Pred], any: &'static [Pred] },
    Item { slot: &'static str, spec: ItemSpec },
    Block { slot: &'static str, spec: BlockSpec },
    Entity { slot: &'static str, spec: EntitySpec },
}

pub struct ItemSpec {
    pub items: &'static [&'static str],
    pub count_min: i32,
    pub preds: &'static [&'static str],
    pub empty_comps: &'static [&'static str],
}

pub struct BlockSpec {
    pub blocks: &'static [&'static str],
    pub open: Option<bool>,
}

pub struct EntitySpec {
    pub types: &'static [&'static str],
    pub flags: &'static [(&'static str, bool)],
    pub game_modes: &'static [&'static str],
    pub vehicle: Option<&'static EntitySpec>,
    pub saddle: bool,
    pub body_items: &'static [&'static str],
}

// ---- the state a guide is worked out from ----

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stack {
    pub id: u16,
    pub count: i32,
}

/// An entity under the crosshair or ridden: a mob (by its Minecraft name), a boat, a cart (0 empty, 1 chest, 2 furnace).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ent {
    Mob(&'static str),
    Boat,
    Cart(u8),
}

/// What is under the crosshair (`ContextualStateContributors.IN_GAME`'s hit result).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Hit {
    #[default]
    None,
    /// A block: its id, and for a door or trap door whether it is open.
    Block { id: u8, open: bool },
    /// An entity, and whether it wears a saddle (a pig does once saddled: the `#minecraft:can_equip_saddle` rideable rule needs it).
    Entity { ent: Ent, saddled: bool },
}

/// The player (`SLOT_SELF` and the facts about the controller's settings).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Me {
    pub on_ground: bool,
    pub in_water: bool,
    pub under_water: bool,
    pub sneaking: bool,
    pub sprinting: bool,
    pub flying: bool,
    pub on_climbable: bool,
    /// `input_moving`: the stick is pushed.
    pub moving: bool,
    /// The ridden entity and whether it carries a saddle.
    pub riding: Option<(Ent, bool)>,
    pub toggle_sneak: bool,
    pub toggle_sprint: bool,
    pub fishing_cast: bool,
    /// The held item blocks attacks (Cortes Combat's guard).
    pub guarding_item: bool,
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct InGame {
    pub me: Me,
    pub hit: Hit,
    /// The selected hotbar stack (`SLOT_MAIN_HAND`); there is no off hand.
    pub main: Option<Stack>,
}

/// `ContainerContext`: the slot under the cursor (`None` = no slot; `Some(None)` = an empty one) and the stack the cursor carries.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Container {
    pub hovering: Option<Option<Stack>>,
    pub holding: Option<Stack>,
    pub can_place: bool,
    pub outside: bool,
}

#[derive(Clone, Copy)]
pub enum Domain<'a> {
    InGame(&'a InGame),
    Container(&'a Container),
}

// ---- b1.7.3's names for Controlify's ----

fn block_ids(name: &str) -> &'static [u8] {
    match name {
        "minecraft:chest" => &[54],
        "minecraft:crafting_table" => &[58],
        "minecraft:dispenser" => &[23],
        "minecraft:furnace" => &[61, 62],
        "minecraft:lever" => &[69],
        "#minecraft:buttons" => &[77],
        "#minecraft:beds" => &[26],
        "#minecraft:doors" => &[64, 71],
        "minecraft:iron_door" => &[71],
        "#minecraft:trapdoors" => &[96],
        _ => &[],
    }
}

fn item_ids(name: &str) -> &'static [u16] {
    match name {
        "#c:tools/bow" => &[261],
        "#c:tools/fishing_rod" => &[346],
        "#minecraft:eggs" => &[344],
        "minecraft:snowball" => &[332],
        _ => &[],
    }
}

/// Controlify's item predicates by component (`minecraft:tool`...), for b1.7.3's items.
fn item_pred(name: &str, id: u16, guarding: bool) -> bool {
    match name {
        // swords, shovels, pickaxes, axes, hoes, shears
        "minecraft:tool" => matches!(id, 267 | 268 | 272 | 276 | 283 | 256 | 269 | 273 | 277 | 284 | 257 | 270 | 274 | 278 | 285 | 258 | 271 | 275 | 279 | 286 | 290..=294 | 359),
        "minecraft:consumable" => crate::world::items::heal_amount(id).is_some() || id == 335,
        "minecraft:equippable" => (298..=317).contains(&id),
        "minecraft:blocks_attacks" => guarding,
        _ => false, // charged projectiles, instruments, bundles: not in b1.7.3
    }
}

/// `BlockItem`: a block, or an item that places one (sign, doors, redstone, reeds, cake, bed, repeater, seeds).
fn is_block_item(id: u16) -> bool {
    id < 256 || matches!(id, 323 | 324 | 330 | 331 | 338 | 354 | 355 | 356 | 295)
}

fn ent_is(name: &str, e: Ent) -> bool {
    match (name, e) {
        ("#minecraft:boat", Ent::Boat) => true,
        ("minecraft:minecart", Ent::Cart(0)) | ("minecraft:chest_minecart", Ent::Cart(1)) | ("minecraft:furnace_minecart", Ent::Cart(2)) => true,
        ("#minecraft:can_equip_saddle", Ent::Mob(m)) => m == "minecraft:pig",
        (n, Ent::Mob(m)) => !n.starts_with('#') && n == m,
        _ => false,
    }
}

// ---- the rule engine ----

struct Env<'a> {
    verbosity: Verbosity,
    dom: Domain<'a>,
    facts: &'static [(&'static str, Pred)],
}

/// A slot's item: not contributed (`None`), an empty stack, or a stack.
type SlotItem = Option<Option<Stack>>;

impl Env<'_> {
    /// `ContextualState.facts()`: the facts the code contributes, then the ones the JSON defines from slots; an unknown one is false.
    fn fact(&self, name: &str, depth: u8) -> bool {
        if depth > 16 {
            return false; // `FactDependencyGraph` rejects a cycle; here it just ends
        }
        match name {
            "verbosity_full" => return self.verbosity == Verbosity::Full,
            "verbosity_reduced" => return self.verbosity.level() >= Verbosity::Reduced.level(),
            "verbosity_minimal" => return self.verbosity.level() >= Verbosity::Minimal.level(),
            _ => {}
        }
        match self.dom {
            Domain::InGame(g) => match name {
                "main_hand_is_block_item" => return g.main.is_some_and(|s| s.count > 0 && is_block_item(s.id)),
                "off_hand_is_block_item" | "can_elytra_fly" => return false,
                "fishing_hook_cast" => return g.me.fishing_cast,
                "on_climbable" => return g.me.on_climbable,
                "in_liquid" => return g.me.in_water,
                "under_water" => return g.me.under_water,
                "is_toggle_sneak" => return g.me.toggle_sneak,
                "is_toggle_sprint" => return g.me.toggle_sprint,
                "input_moving" => return g.me.moving,
                "has_hearts" => return true,
                _ => {}
            },
            Domain::Container(c) => match name {
                "can_place_held_item" => return c.hovering.is_some() && c.can_place,
                "can_pickup_slot" => return c.hovering.is_some(),
                "cursor_outside_container" => return c.outside,
                _ => {}
            },
        }
        let defs = self.facts;
        defs.iter().find(|(n, _)| *n == name).is_some_and(|(_, p)| self.pred(p, depth + 1))
    }

    fn slot_item(&self, slot: &str) -> SlotItem {
        match (self.dom, slot) {
            (Domain::InGame(g), "main_hand") => Some(g.main.filter(|s| s.count > 0)),
            (Domain::InGame(_), "off_hand" | "active_item") => Some(None),
            (Domain::Container(c), "hovering_slot") => c.hovering.map(|s| s.filter(|s| s.count > 0)),
            (Domain::Container(c), "holding_item") => Some(c.holding.filter(|s| s.count > 0)),
            (Domain::Container(c), "bundle_selected") => c.hovering.map(|_| None),
            _ => None,
        }
    }

    fn guarding(&self) -> bool {
        matches!(self.dom, Domain::InGame(g) if g.me.guarding_item)
    }

    fn item(&self, slot: &str, s: &ItemSpec) -> bool {
        let Some(stack) = self.slot_item(slot) else { return false };
        if s.count_min > 0 && stack.map_or(0, |t| t.count) < s.count_min {
            return false;
        }
        let needs_stack = !s.items.is_empty() || !s.preds.is_empty() || !s.empty_comps.is_empty();
        let Some(t) = stack else { return !needs_stack };
        (s.items.is_empty() || s.items.iter().any(|n| item_ids(n).contains(&t.id)))
            && s.preds.iter().all(|p| item_pred(p, t.id, self.guarding()))
            && s.empty_comps.is_empty() // a component this game's items never have
    }

    fn block(&self, slot: &str, s: &BlockSpec) -> bool {
        let (Domain::InGame(g), "hit_result") = (self.dom, slot) else { return false };
        let Hit::Block { id, open } = g.hit else { return false };
        (s.blocks.is_empty() || s.blocks.iter().any(|n| block_ids(n).contains(&id))) && s.open.map_or(true, |want| matches!(id, 64 | 71 | 96) && open == want)
    }

    fn entity(&self, slot: &str, s: &EntitySpec) -> bool {
        let Domain::InGame(g) = self.dom else { return false };
        match slot {
            "self" => self.me(&g.me, s),
            "hit_result" => match g.hit {
                Hit::Entity { ent, saddled } => {
                    (s.types.is_empty() || s.types.iter().any(|n| ent_is(n, ent))) && s.flags.is_empty() && s.game_modes.is_empty() && s.vehicle.is_none() && (!s.saddle || saddled) && s.body_items.is_empty()
                }
                _ => false,
            },
            _ => false,
        }
    }

    /// `SLOT_SELF`: the player's flags, game mode and vehicle.
    fn me(&self, me: &Me, s: &EntitySpec) -> bool {
        if !s.types.is_empty() {
            return false; // the player is not one of the named types
        }
        let flag = |k: &str| match k {
            "is_on_ground" => me.on_ground,
            "is_flying" => me.flying,
            "is_fall_flying" => false,
            "is_in_water" => me.in_water,
            "is_sneaking" => me.sneaking,
            "is_sprinting" => me.sprinting,
            _ => false,
        };
        if !s.flags.iter().all(|&(k, v)| flag(k) == v) {
            return false;
        }
        if !s.game_modes.is_empty() && !s.game_modes.contains(&"survival") {
            return false;
        }
        match s.vehicle {
            None => true,
            Some(v) => me.riding.is_some_and(|(e, saddled)| {
                (v.types.is_empty() || v.types.iter().any(|n| ent_is(n, e))) && (!v.saddle || saddled) && v.body_items.is_empty()
            }),
        }
    }

    /// `ContextualPredicate.matches`.
    fn pred(&self, p: &Pred, depth: u8) -> bool {
        match p {
            Pred::Fact(f) => self.fact(f, depth),
            Pred::Static(b) => *b,
            Pred::Compound { all, none, any } => {
                all.iter().all(|x| self.pred(x, depth)) && !none.iter().any(|x| self.pred(x, depth)) && (any.is_empty() || any.iter().any(|x| self.pred(x, depth)))
            }
            Pred::Item { slot, spec } => self.item(slot, spec),
            Pred::Block { slot, spec } => self.block(slot, spec),
            Pred::Entity { slot, spec } => self.entity(slot, spec),
        }
    }
}

/// `RuleEngine.evaluate`: the winning rules, in order. A rule that does not match does not take its key.
fn evaluate<'a>(rules: &'a [Rule], env: &Env) -> Vec<&'a Rule> {
    let mut won: Vec<(&str, bool)> = Vec::new();
    let mut out = Vec::new();
    for r in rules {
        let key = (r.bind, r.left);
        if won.contains(&key) {
            continue;
        }
        if env.pred(&r.when, 0) {
            won.push(key);
            out.push(r);
        }
    }
    out
}

// ---- the lines ----

/// A space in the font.
const SPACE: u32 = 4;
/// `BindingFontHelper.getHeight`: the height of a line with a glyph in it.
const LINE_H: u32 = 15;

/// One precomputed line (`PrecomputedLines.PrecomputedLine`).
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub text: &'static str,
    /// The glyph's cell of the controller type and its advance, none where the type has no picture for the input.
    pub glyph: Option<(usize, u32)>,
    pub width: u32,
    /// Where the text starts and ends in the line: what the contrast box covers.
    pub bg: (u32, u32),
    /// Glyph first (the left column) or last.
    pub glyph_first: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lines {
    pub left: Vec<Line>,
    pub right: Vec<Line>,
}

/// A guide (`GuideInstanceImpl`): the rules of one domain and the lines they gave at the last `update`.
pub struct Guide {
    rules: &'static [Rule],
    facts: &'static [(&'static str, Pred)],
    pub lines: Lines,
}

impl Guide {
    pub fn in_game() -> Self {
        Self { rules: IN_GAME_RULES, facts: IN_GAME_FACTS, lines: Lines::default() }
    }

    pub fn container() -> Self {
        Self { rules: CONTAINER_RULES, facts: CONTAINER_FACTS, lines: Lines::default() }
    }

    /// `GuideInstanceImpl.update`. `input` says which controller input (`controlify:button/south` without the namespace: `button/south`) a binding id is
    /// on, none where it is unbound; `ty` is the controller type (`glyph_map::TYPES`).
    pub fn update(&mut self, dom: Domain, verbosity: Verbosity, ty: usize, input: &dyn Fn(&str) -> Option<&'static str>) {
        let env = Env { verbosity, dom, facts: self.facts };
        let mut lines = Lines::default();
        for r in evaluate(self.rules, &env) {
            let Some(inp) = input(r.bind) else { continue }; // `binding.isUnbound()`: skipped
            let glyph = INPUTS.iter().position(|n| *n == inp).and_then(|i| (CELL[ty][i] >= 0).then(|| (CELL[ty][i] as usize, u32::from(ADVANCE[ty][i]))));
            let text_w = HudPipeline::text_width(r.text);
            // the glyph goes after the text in the right column, before it in the left one (`font.isBidirectional() ^ location == RIGHT`)
            let glyph_first = r.left;
            let (gw, gap) = glyph.map_or((0, 0), |(_, a)| (a, SPACE));
            let bg_l = if glyph_first { gw + gap } else { 0 };
            let line = Line { text: r.text, glyph, width: gw + gap + text_w, bg: (bg_l, bg_l + text_w), glyph_first };
            if r.left { &mut lines.left } else { &mut lines.right }.push(line);
        }
        self.lines = lines;
    }

    /// `GuideRenderer.extractRenderState`: both columns on a `w` x `h` px surface at `s` px per GUI unit. `bottom` is `ingameGuideBottom`;
    /// `contrast` is `textContrast` (a 50% black box behind the text, no shadow: the in-game guide; the others have the shadow).
    pub fn draw(&self, v: &mut Vec<HudVertex>, w: f32, h: f32, s: f32, ty: usize, bottom: bool, contrast: bool) {
        let (sw, sh) = ((w / s).floor(), (h / s).floor());
        Self::column(v, &self.lines.left, (sw, sh), s, ty, bottom, false, contrast);
        Self::column(v, &self.lines.right, (sw, sh), s, ty, bottom, true, contrast);
    }

    #[allow(clippy::too_many_arguments)]
    fn column(v: &mut Vec<HudVertex>, lines: &[Line], (sw, sh): (f32, f32), s: f32, ty: usize, bottom: bool, right: bool, contrast: bool) {
        if lines.is_empty() {
            return;
        }
        let (safe_x, safe_y, between) = (2.0, 5.0, 2.0);
        let all_h = lines.len() as f32 * LINE_H as f32 + (lines.len() - 1) as f32 * between;
        let x = if right { sw - safe_x } else { safe_x };
        let mut y = if bottom { sh - all_h - safe_y } else { safe_y };
        let order: Vec<&Line> = if bottom { lines.iter().rev().collect() } else { lines.iter().collect() };
        for l in order {
            let lx = if right { x - l.width as f32 } else { x };
            if contrast {
                // `graphics.fill(..., 0x80000000)` round the text only; the line height of the font is 9
                HudPipeline::push_quad(v, (lx + l.bg.0 as f32 - 1.0) * s, (y - 1.0) * s, (l.bg.1 - l.bg.0) as f32 * s + 2.0 * s, 11.0 * s, [0.0, 0.0, 0.0, 0.5]);
            }
            let (gw, gap) = l.glyph.map_or((0, 0), |(_, a)| (a, SPACE));
            let (gx, tx) = if l.glyph_first { (lx, lx + (gw + gap) as f32) } else { (lx + (l.bg.1 - l.bg.0 + gap) as f32, lx) };
            if let Some((cell, _)) = l.glyph {
                // the font's glyph: 16 units tall, its top 11 above the baseline (ascent 11; the baseline of text drawn at y is y + 7)
                HudPipeline::push_gui(v, gx * s, (y - 4.0) * s, 16.0 * s, 16.0 * s, sprites::pad::glyph(ty, cell));
            }
            if contrast {
                HudPipeline::push_text_flat(v, tx * s, y * s, s, l.text, [1.0; 4]);
            } else {
                HudPipeline::push_text(v, tx * s, y * s, s, l.text, [1.0; 4]);
            }
            y += LINE_H as f32 + between;
        }
    }
}

// ---- the guide on a button (`ButtonGuideApi.addGuideToButton`) ----

/// What a button's mark is (`GUI_BACK`, `GUI_ABSTRACT_ACTION_1`, `GUI_ABSTRACT_ACTION_2`): B, X, Y by Controlify's defaults.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    Back,
    Action1,
    Action2,
}

/// The controller type and the input of each `Mark` while button marks are on (`set_marks`); `lib.rs` sets it every frame, the menus read it
/// while drawing (they take no pad state, as `craft::set_style` does not either).
static MARKS: std::sync::Mutex<Option<(usize, [&'static str; 3])>> = std::sync::Mutex::new(None);

pub fn set_marks(m: Option<(usize, [&'static str; 3])>) {
    if let Ok(mut g) = MARKS.lock() {
        *g = m;
    }
}

/// A button's label made room for its glyph, and where the glyph goes.
#[derive(Clone, Debug, PartialEq)]
pub struct Marked {
    /// The label with blanks in front (`glyph + " " + label` is centred as one: the blanks stand for the glyph, a space is 4 wide).
    pub label: String,
    pub ty: usize,
    pub cell: usize,
    /// Where the glyph's left edge is (GUI units), for the label centred on `cx`.
    pub x: f32,
}

/// `ButtonGuideRenderer`: the glyph goes in front of the label, the whole centred. None while marks are off or the controller has no glyph for it.
/// The blanks are whole spaces, so the glyph's gap to the text is up to 3 units wider than the Java's single space.
pub fn mark_label(m: Mark, label: &str, cx: f32) -> Option<Marked> {
    let (ty, inputs) = (*MARKS.lock().ok()?)?;
    let i = INPUTS.iter().position(|n| *n == inputs[m as usize])?;
    let cell = usize::try_from(CELL[ty][i]).ok()?;
    let spaces = (u32::from(ADVANCE[ty][i]) + SPACE + 3) / 4;
    let (blank, w) = (spaces * SPACE, HudPipeline::text_width(label));
    Some(Marked { label: format!("{}{label}", " ".repeat(spaces as usize)), ty, cell, x: cx - (w + blank) as f32 / 2.0 })
}

/// The glyph of a `Marked` label whose text is drawn at `text_y` (GUI units), `s` px per unit.
pub fn push_mark(v: &mut Vec<HudVertex>, s: f32, m: &Marked, text_y: f32) {
    HudPipeline::push_gui(v, m.x * s, (text_y - 4.0) * s, 16.0 * s, 16.0 * s, sprites::pad::glyph(m.ty, m.cell));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(id: &str) -> Option<&'static str> {
        Some(match id {
            "jump" | "inv_select" => "button/south",
            "gui_back" => "button/east",
            "sneak" | "radial_menu_not" => "button/right_stick",
            "sprint" => "button/left_stick",
            "attack" => "axis/right_trigger",
            "use" => "axis/left_trigger",
            "inventory" | "inv_quick_move" | "drop_inventory" => "button/north",
            "inv_take_half" => "button/west",
            "drop" => "button/dpad_down",
            "radial_menu" => "button/dpad_right",
            _ => return None, // swap_hands, drop_stack, pick_block...: unbound
        })
    }

    fn texts(g: &Guide) -> (Vec<&'static str>, Vec<&'static str>) {
        (g.lines.left.iter().map(|l| l.text).collect(), g.lines.right.iter().map(|l| l.text).collect())
    }

    fn walking() -> InGame {
        InGame { me: Me { on_ground: true, toggle_sneak: true, toggle_sprint: true, ..Me::default() }, ..InGame::default() }
    }

    fn run(g: &InGame, v: Verbosity) -> (Vec<&'static str>, Vec<&'static str>) {
        let mut guide = Guide::in_game();
        guide.update(Domain::InGame(g), v, 0, &input);
        texts(&guide)
    }

    #[test]
    fn standing_still_with_nothing_shows_the_basics() {
        let (l, r) = run(&walking(), Verbosity::Full);
        assert_eq!(l, ["Jump", "Start Sneaking", "Open Inventory", "Radial Menu"]);
        assert!(r.is_empty(), "nothing to hit, nothing held, no drop bind item: {r:?}");
    }

    #[test]
    fn verbosity_takes_rules_away() {
        let (l, _) = run(&walking(), Verbosity::Reduced);
        assert_eq!(l, ["Jump", "Start Sneaking", "Open Inventory"], "the radial menu is a Full rule");
        let (l, _) = run(&walking(), Verbosity::Minimal);
        assert_eq!(l, ["Jump", "Start Sneaking"], "inventory is a Reduced one");
    }

    #[test]
    fn sprint_appears_while_moving_and_stops_while_sprinting() {
        let mut g = walking();
        g.me.moving = true;
        assert!(run(&g, Verbosity::Full).0.contains(&"Start Sprinting"));
        g.me.sprinting = true;
        let (l, _) = run(&g, Verbosity::Full);
        assert!(l.contains(&"Stop Sprinting") && !l.contains(&"Start Sprinting"), "toggle sprint: {l:?}");
        g.me.toggle_sprint = false;
        assert!(!run(&g, Verbosity::Full).0.contains(&"Stop Sprinting"), "a held sprint has nothing to stop");
    }

    #[test]
    fn water_and_ladders_change_the_jump_and_sneak_rules_in_file_order() {
        let mut g = walking();
        g.me.in_water = true;
        g.me.on_ground = false;
        let (l, _) = run(&g, Verbosity::Full);
        assert_eq!(&l[..2], ["Swim Up", "Swim Down"]);
        g = walking();
        g.me.on_climbable = true;
        let (l, _) = run(&g, Verbosity::Full);
        assert_eq!(&l[..2], ["Climb Up", "Climb Down"], "the climbable rules come before the on-ground one, which loses the key");
        g = walking();
        g.me.flying = true;
        assert_eq!(&run(&g, Verbosity::Full).0[..2], ["Fly Up", "Fly Down"]);
    }

    #[test]
    fn the_crosshair_picks_the_attack_and_use_rules() {
        let mut g = walking();
        g.hit = Hit::Block { id: 1, open: false };
        g.main = Some(Stack { id: 257, count: 1 }); // a pickaxe
        let (_, r) = run(&g, Verbosity::Full);
        assert_eq!(r[0], "Mine", "a block with a tool in hand");
        g.main = Some(Stack { id: 4, count: 5 }); // cobblestone
        let (_, r) = run(&g, Verbosity::Full);
        assert_eq!(&r[..2], ["Break", "Place"]);
        assert!(r.contains(&"Drop Item"), "{r:?}");
        g.hit = Hit::Block { id: 54, open: false }; // a chest
        let (_, r) = run(&g, Verbosity::Full);
        assert_eq!(r[1], "Open");
        g.me.sneaking = true; // sneaking keeps you from opening it: the block-item rule takes over
        let (_, r) = run(&g, Verbosity::Full);
        assert_eq!(r[1], "Place");
        g.me.sneaking = false;
        g.hit = Hit::Block { id: 64, open: true };
        assert_eq!(run(&g, Verbosity::Full).1[1], "Close", "an open door");
        g.hit = Hit::Block { id: 71, open: false };
        assert_ne!(run(&g, Verbosity::Full).1[1], "Open", "an iron door does not open by hand");
        g.hit = Hit::Block { id: 26, open: false };
        assert_eq!(run(&g, Verbosity::Full).1[1], "Sleep");
        g.hit = Hit::Block { id: 61, open: false };
        assert_eq!(run(&g, Verbosity::Full).1[1], "Smelt");
    }

    #[test]
    fn mobs_boats_and_carts() {
        let mut g = walking();
        g.hit = Hit::Entity { ent: Ent::Mob("minecraft:cow"), saddled: false };
        let (_, r) = run(&g, Verbosity::Full);
        assert_eq!(&r[..2], ["Attack", "Interact"]);
        g.hit = Hit::Entity { ent: Ent::Mob("minecraft:pig"), saddled: false };
        assert_eq!(run(&g, Verbosity::Full).1[1], "Interact", "a pig without a saddle is not rideable");
        g.hit = Hit::Entity { ent: Ent::Mob("minecraft:pig"), saddled: true };
        assert_eq!(run(&g, Verbosity::Full).1[1], "Ride", "a saddled one is");
        g.hit = Hit::Entity { ent: Ent::Boat, saddled: false };
        assert_eq!(run(&g, Verbosity::Full).1[1], "Ride");
        g.hit = Hit::Entity { ent: Ent::Cart(1), saddled: false };
        assert_eq!(run(&g, Verbosity::Full).1[1], "Open", "a chest cart");
    }

    #[test]
    fn items_in_hand_name_the_use() {
        let mut g = walking();
        g.main = Some(Stack { id: 261, count: 1 });
        assert_eq!(run(&g, Verbosity::Full).1[0], "Shoot", "a bow");
        g.main = Some(Stack { id: 346, count: 1 });
        assert_eq!(run(&g, Verbosity::Full).1[0], "Cast");
        g.me.fishing_cast = true;
        assert_eq!(run(&g, Verbosity::Full).1[0], "Reel In");
        g.me.fishing_cast = false;
        g.main = Some(Stack { id: 332, count: 16 });
        assert_eq!(run(&g, Verbosity::Full).1[0], "Throw", "a snowball");
        g.main = Some(Stack { id: 297, count: 3 });
        assert_eq!(run(&g, Verbosity::Full).1[0], "Consume", "bread");
        g.main = Some(Stack { id: 298, count: 1 });
        assert_eq!(run(&g, Verbosity::Full).1[0], "Equip", "armour");
    }

    #[test]
    fn a_saddled_pig_ride_has_no_jump_rule_but_a_dismount() {
        let mut g = walking();
        g.me.riding = Some((Ent::Mob("minecraft:pig"), true));
        let (l, _) = run(&g, Verbosity::Full);
        assert_eq!(l[1], "Dismount", "the sneak rule for a vehicle (the jump rule is still the on-ground one)");
        assert!(!l.contains(&"Dash") && !l.contains(&"Fly Up"));
    }

    #[test]
    fn unbound_rules_are_skipped_but_still_take_their_key() {
        // swap_hands is unbound here: with an item in hand its rule is skipped without error
        let mut g = walking();
        g.main = Some(Stack { id: 4, count: 5 });
        let (_, r) = run(&g, Verbosity::Full);
        assert!(!r.contains(&"Swap Hands") && !r.contains(&"Drop Stack"));
    }

    fn cont(hovering: Option<Option<Stack>>, holding: Option<Stack>, can_place: bool, outside: bool) -> (Vec<&'static str>, Vec<&'static str>) {
        let c = Container { hovering, holding, can_place, outside };
        let mut g = Guide::container();
        g.update(Domain::Container(&c), Verbosity::Full, 0, &input);
        texts(&g)
    }

    #[test]
    fn a_marked_button_is_centred_as_glyph_space_label() {
        set_marks(None);
        assert!(mark_label(Mark::Back, "Quit Game", 100.0).is_none(), "off: no mark");
        set_marks(Some((0, ["button/east", "button/west", "button/north"])));
        let m = mark_label(Mark::Back, "Quit Game", 100.0).unwrap();
        let w = HudPipeline::text_width("Quit Game") as f32;
        let blank = (m.label.len() - "Quit Game".len()) as f32 * 4.0;
        let adv = f32::from(ADVANCE[0][INPUTS.iter().position(|n| *n == "button/east").unwrap()]);
        assert!(blank >= adv + 4.0 && blank <= adv + 4.0 + 3.0, "room for the glyph and its gap: {blank} for an advance of {adv}");
        assert!((m.x - (100.0 - (w + blank) / 2.0)).abs() < 1e-4, "its left edge is the label block's");
        assert_ne!(mark_label(Mark::Action1, "Options", 0.0).unwrap().cell, m.cell, "X is not B");
        set_marks(Some((0, ["button/nothing", "button/west", "button/north"])));
        assert!(mark_label(Mark::Back, "x", 0.0).is_none(), "an input the font does not know");
        set_marks(None);
    }

    #[test]
    fn a_container_always_offers_exit() {
        let (l, r) = cont(None, None, false, false);
        assert_eq!(l, ["Exit"]);
        assert!(r.is_empty());
    }

    #[test]
    fn a_container_names_take_place_swap_and_drop() {
        let st = |n| Some(Stack { id: 4, count: n });
        let (l, r) = cont(Some(st(10)), None, true, false);
        assert_eq!(l, ["Exit", "Take"]);
        assert_eq!(r, ["Quick Move", "Take Half"]);
        let (l, r) = cont(Some(st(10)), st(5), true, false);
        assert_eq!(l, ["Exit", "Place All"], "holding many over an item that accepts it");
        assert_eq!(r, ["Drop", "Place One"], "Y drops (quick move needs an empty cursor), X places one");
        let (l, _) = cont(Some(st(10)), st(1), true, false);
        assert_eq!(l[1], "Place");
        let (l, _) = cont(Some(st(10)), st(1), false, false);
        assert_eq!(l[1], "Swap");
        let (l, _) = cont(None, st(3), false, true);
        assert_eq!(l, ["Exit", "Drop"], "cursor outside the panel with a stack");
    }

    #[test]
    fn lines_have_the_java_widths_and_the_right_column_puts_the_glyph_last() {
        let mut g = Guide::in_game();
        g.update(Domain::InGame(&walking()), Verbosity::Minimal, 0, &input);
        let l = &g.lines.left[0];
        assert_eq!(l.text, "Jump");
        let (cell, adv) = l.glyph.expect("the Xbox font has the south button");
        assert!(adv >= 15 && adv <= 17, "a 16 wide glyph advances 15..17: {adv}");
        assert_eq!(l.width, adv + 4 + HudPipeline::text_width("Jump"));
        assert_eq!(l.bg, (adv + 4, adv + 4 + HudPipeline::text_width("Jump")), "the box is round the text only, after the glyph");
        assert!(l.glyph_first && cell < 32);
        g.update(Domain::InGame(&InGame { hit: Hit::Block { id: 1, open: false }, ..walking() }), Verbosity::Minimal, 0, &input);
        let r = &g.lines.right[0];
        assert!(!r.glyph_first);
        assert_eq!(r.bg.0, 0, "a right line starts with its text");
    }

    #[test]
    fn the_columns_sit_where_the_renderer_puts_them() {
        let mut g = Guide::in_game();
        g.update(Domain::InGame(&walking()), Verbosity::Full, 0, &input);
        let mut top = Vec::new();
        g.draw(&mut top, 800.0, 400.0, 2.0, 0, false, true);
        assert!(!top.is_empty());
        let min_y = top.iter().map(|p| p.pos[1]).fold(f32::MAX, f32::min);
        assert!(min_y >= 0.0 && min_y <= 5.0 * 2.0, "the first line is about 5 units from the top: {min_y}");
        let min_x = top.iter().map(|p| p.pos[0]).fold(f32::MAX, f32::min);
        assert!((min_x - 2.0 * 2.0).abs() < 1.0 || min_x < 2.0 * 2.0 + 2.0, "2 units from the left: {min_x}");
        let mut bottom = Vec::new();
        g.draw(&mut bottom, 800.0, 400.0, 2.0, 0, true, true);
        let max_y = bottom.iter().map(|p| p.pos[1]).fold(f32::MIN, f32::max);
        assert!(max_y <= 400.0 - 5.0 * 2.0 + 20.0 && max_y > 300.0, "bottom aligned: {max_y}");
        // with contrast there is a box behind the text, without it the text has a shadow instead
        let n = |v: &Vec<HudVertex>| v.len();
        let mut shadow = Vec::new();
        g.draw(&mut shadow, 800.0, 400.0, 2.0, 0, false, false);
        assert_ne!(n(&top), n(&shadow));
    }
}
