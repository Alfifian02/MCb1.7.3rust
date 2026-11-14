//! Controlify's bindings (isXander's Fabric mod, LGPL-3.0-or-later): the registry of every bind, what each one is bound to, and the contexts that
//! switch them on. Ported from `ControlifyBindings` (the ids, categories and allowed contexts, generated into `bind_defs.rs` by
//! `tools/gen_bindings.py` together with `default_bind/default.json`), `BindContext`, `InputBindingImpl` and `BindController` / `ControllerConfigScreenFactory`
//! (the conflict rule and the capture of a new input).
//!
//! The model is Controlify's: a bind holds exactly ONE input, a button or an axis (here: the sixteen buttons of `Btn`, where the two triggers are
//! analogue, and the four directions of each stick), or none. An axis bind reads 0..1 (the stick's value in that direction after its deadzone); a button
//! bind reads 0 or 1. A bind is DIGITAL while its value is over the button activation threshold (`InputBindingImpl.StateAccessImpl.digital`: strictly over),
//! so a stick direction can be a button (`gui_navi_*`) and a button can be an axis (`look_up` on a face button looks at full speed).
//!
//! A bind only acts in the contexts it allows (`BindContext`): the pad works them out each frame (`Pad::contexts`) and masks the others (`InputBindingImpl.pushState`'s
//! `suppressed`).
//!
//! Not ported: HAT inputs (an Android hat is the d-pad buttons), the mixed-input and keyboard-emulation outputs (`KeyMappingEmulationOutput`), the bind API for other
//! mods, `fakePress`, the radial-menu icons of a bind (phase 4).

use super::bind_defs::{B, COUNT, DEFS};
use super::gamepad::Btn;

/// `BindContext`s, as bits of `Def::ctx`.
pub mod ctx {
    /// A world is loaded and no screen is open (`IN_GAME`).
    pub const IN_GAME: u8 = 1;
    /// Any screen is open (`ANY_SCREEN`).
    pub const SCREEN: u8 = 2;
    /// A screen is open and the virtual mouse is off (`REGULAR_SCREEN`: focus navigation).
    pub const REGULAR: u8 = 4;
    /// A container screen (`CONTAINER`).
    pub const CONTAINER: u8 = 8;
    /// The radial menu is open (`RADIAL_MENU`).
    pub const RADIAL: u8 = 16;
    /// The virtual mouse is on and the screen wants its cursor (`V_MOUSE_CURSOR`).
    pub const VM_CURSOR: u8 = 32;
    /// The virtual mouse is on and the screen asked for it outright (`V_MOUSE_COMPAT`).
    pub const VM_COMPAT: u8 = 64;
}

/// The four directions of the two sticks as inputs (`GamepadInputs.LEFT_STICK_AXIS_UP`...): each reads 0..1 on its side of the stick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ax {
    LeftUp,
    LeftDown,
    LeftLeft,
    LeftRight,
    RightUp,
    RightDown,
    RightLeft,
    RightRight,
}

impl Ax {
    pub const ALL: [Ax; 8] = [Ax::LeftUp, Ax::LeftDown, Ax::LeftLeft, Ax::LeftRight, Ax::RightUp, Ax::RightDown, Ax::RightLeft, Ax::RightRight];

    pub fn name(self) -> &'static str {
        ["LS Up", "LS Down", "LS Left", "LS Right", "RS Up", "RS Down", "RS Left", "RS Right"][self as usize]
    }

    /// The Controlify input (`controlify:axis/left_stick_up`), which says its glyph in `glyph_map`.
    pub fn input(self) -> &'static str {
        [
            "axis/left_stick_up",
            "axis/left_stick_down",
            "axis/left_stick_left",
            "axis/left_stick_right",
            "axis/right_stick_up",
            "axis/right_stick_down",
            "axis/right_stick_left",
            "axis/right_stick_right",
        ][self as usize]
    }
}

/// What a bind is bound to (`Input`: `ButtonInput`, `AxisInput`, `EmptyInput`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Input {
    None,
    Btn(Btn),
    Stick(Ax),
}

/// The code of `Input::None` in `options.txt` and in `Input::from_code`; buttons are 0..15 (`Btn` order, as before), stick directions 16..23.
pub const NONE_CODE: u8 = 24;
/// How many inputs a pad can press at once: the bits of `Input::bit`.
pub const N_INPUTS: usize = 24;

impl Input {
    pub fn code(self) -> u8 {
        match self {
            Input::None => NONE_CODE,
            Input::Btn(b) => b as u8,
            Input::Stick(a) => 16 + a as u8,
        }
    }

    /// A bad code is no input.
    pub fn from_code(n: u8) -> Input {
        match n {
            0..=15 => Input::Btn(Btn::ALL[n as usize]),
            16..=23 => Input::Stick(Ax::ALL[n as usize - 16]),
            _ => Input::None,
        }
    }

    /// The input's bit in the masks of `Pad`: buttons keep `Btn::bit`, a stick direction follows them. None has no bit.
    pub fn bit(self) -> u32 {
        if self == Input::None {
            0
        } else {
            1u32 << self.code()
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Input::None => "None",
            Input::Btn(b) => b.name(),
            Input::Stick(a) => a.name(),
        }
    }

    /// The Controlify input id of its glyph (`button/south`), none for an empty bind.
    pub fn glyph(self) -> Option<&'static str> {
        match self {
            Input::None => None,
            Input::Btn(b) => Some(b.input()),
            Input::Stick(a) => Some(a.input()),
        }
    }

    /// Every input a bind can be bound to, in the order the capture looks at them.
    pub fn all() -> impl Iterator<Item = Input> {
        (0..N_INPUTS as u8).map(Input::from_code)
    }
}

/// `KeyMapping.Category`s and Controlify's own: how the Controls page groups the binds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cat {
    Movement,
    Gameplay,
    Inventory,
    Creative,
    Misc,
    Debug,
    Gui,
    Radial,
    VMouse,
}

impl Cat {
    pub const ALL: [Cat; 9] = [Cat::Movement, Cat::Gameplay, Cat::Inventory, Cat::Creative, Cat::Misc, Cat::Debug, Cat::Gui, Cat::Radial, Cat::VMouse];

    pub fn name(self) -> &'static str {
        ["Movement", "Gameplay", "Inventory", "Creative Mode", "Miscellaneous", "Debug", "GUI", "Radial Menu", "Virtual Mouse"][self as usize]
    }
}

/// One entry of the registry (`bind_defs::DEFS`, in `B` order).
#[derive(Clone, Copy, Debug)]
pub struct Def {
    /// Controlify's id without its namespace (`jump`); the key in `options.txt` is `pad_<id>`.
    pub id: &'static str,
    pub name: &'static str,
    pub cat: Cat,
    /// `ctx` bits: the contexts the bind acts in.
    pub ctx: u8,
    pub default: Input,
    /// The bind does something in this port; the others are kept and saved but not shown (no chat, swap hands, pick block, game modes...).
    pub live: bool,
}

impl B {
    pub fn def(self) -> &'static Def {
        &DEFS[self as usize]
    }

    pub fn from_id(id: &str) -> Option<B> {
        DEFS.iter().position(|d| d.id == id).map(|i| B::ALL[i])
    }
}

/// The ids this port's earlier builds saved (`pad_wheel:`...): the registry's id that took over, and the ones that went with it. `gui_select` was one button for
/// every click of a screen, which Controlify splits into three (`gui_press`, `inv_select`, `vmouse_lclick`).
const LEGACY: [(&str, &[&str]); 4] = [
    ("wheel", &["hotbar_item_select_radial"]),
    ("view", &["change_perspective"]),
    ("info", &["toggle_debug_menu"]),
    ("gui_select", &["gui_press", "inv_select", "vmouse_lclick"]),
];

/// What every bind is bound to. Lives in `PadCfg`.
#[derive(Clone, Debug, PartialEq)]
pub struct Binds {
    inputs: [Input; COUNT],
}

impl Default for Binds {
    fn default() -> Self {
        Self { inputs: std::array::from_fn(|i| DEFS[i].default) }
    }
}

impl Binds {
    pub fn get(&self, b: B) -> Input {
        self.inputs[b as usize]
    }

    pub fn set(&mut self, b: B, i: Input) {
        self.inputs[b as usize] = i;
    }

    /// The bind's bit in the masks of `Pad` (0 while unbound).
    pub fn bit(&self, b: B) -> u32 {
        self.get(b).bit()
    }

    pub fn is_default(&self, b: B) -> bool {
        self.get(b) == b.def().default
    }

    pub fn reset(&mut self, b: B) {
        self.set(b, b.def().default);
    }

    /// `controlify.gui.reset_all_binds`.
    pub fn reset_all(&mut self) {
        *self = Self::default();
    }

    /// `ControllerConfigScreenFactory.updateConflictingBinds`: another bind shares an allowed context and the same non-empty input.
    pub fn conflicts(&self, b: B) -> bool {
        let (me, d) = (self.get(b), b.def());
        me != Input::None && B::ALL.iter().any(|&o| o != b && o.def().live && o.def().ctx & d.ctx != 0 && self.get(o) == me)
    }

    /// The registry's binds of one category that do something here, in Controlify's order (the Controls page).
    pub fn of(cat: Cat) -> impl Iterator<Item = B> {
        B::ALL.into_iter().filter(move |b| b.def().live && b.def().cat == cat)
    }

    /// `options.txt` line `pad_<id>:<code>`; an id this port saved before Controlify's registry is moved to the bind(s) that took over.
    /// Returns whether the line was a bind (a knob or a switch is `PadCfg::load`'s).
    pub fn load(&mut self, id: &str, v: &str) -> bool {
        let Ok(n) = v.parse::<u8>() else { return false };
        let input = Input::from_code(n);
        if let Some(b) = B::from_id(id) {
            self.set(b, input);
            return true;
        }
        if let Some((_, to)) = LEGACY.iter().find(|(old, _)| *old == id) {
            for t in to.iter() {
                if let Some(b) = B::from_id(t) {
                    self.set(b, input);
                }
            }
            return true;
        }
        false
    }

    /// Only what differs from the default is saved (`keepDefaultBindings` off), so a default Controlify changes later reaches the player.
    pub fn save_into(&self, t: &mut String) {
        for b in B::ALL {
            if !self.is_default(b) {
                t.push_str(&format!("pad_{}:{}\n", b.def().id, self.get(b).code()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_is_controlifys() {
        assert_eq!(COUNT, DEFS.len());
        // the ids are unique and every `B` indexes its own entry
        for (i, b) in B::ALL.iter().enumerate() {
            assert_eq!(*b as usize, i);
            assert_eq!(B::from_id(b.def().id), Some(*b), "{}", b.def().id);
        }
        assert!(B::ALL.iter().filter(|b| b.def().live).count() >= 40, "the port wires more than 40 binds");
        assert_eq!(B::Jump.def().default, Input::Btn(Btn::A));
        assert_eq!(B::Attack.def().default, Input::Btn(Btn::Rt));
        assert_eq!(B::WalkForward.def().default, Input::Stick(Ax::LeftUp));
        assert_eq!(B::VmouseToggle.def().ctx & ctx::SCREEN, ctx::SCREEN);
    }

    #[test]
    fn codes_round_trip_and_keep_the_old_button_indices() {
        for i in Input::all() {
            assert_eq!(Input::from_code(i.code()), i);
        }
        // the options.txt of the builds before this one saved `Btn as usize` (A = 0 ... Right = 15)
        assert_eq!(Input::from_code(0), Input::Btn(Btn::A));
        assert_eq!(Input::from_code(15), Input::Btn(Btn::Right));
        assert_eq!(Input::Btn(Btn::Y).bit(), 1u32 << (Btn::Y as u32));
        assert_eq!(Input::from_code(200), Input::None);
        assert_eq!(Input::None.bit(), 0);
        // a stick direction's bit is above the 16 buttons'
        assert!(Input::Stick(Ax::LeftUp).bit() >= 1 << 16);
        assert!(Input::all().map(Input::bit).filter(|b| *b != 0).fold(0u32, |a, b| a | b).count_ones() == N_INPUTS as u32);
    }

    #[test]
    fn conflicts_follow_controlify() {
        let mut b = Binds::default();
        // the defaults: Y is `inventory` (in game) and `inv_quick_move` / `drop_inventory` (containers): different contexts, no conflict
        assert!(!b.conflicts(B::Inventory));
        // `inv_quick_move` and `drop_inventory` share the container context and Y by default
        assert!(b.conflicts(B::InvQuickMove) && b.conflicts(B::DropInventory));
        b.set(B::Sneak, Input::Btn(Btn::A));
        assert!(b.conflicts(B::Sneak) && b.conflicts(B::Jump));
        b.set(B::Sneak, Input::None);
        assert!(!b.conflicts(B::Sneak) && !b.conflicts(B::Jump));
    }

    #[test]
    fn only_changes_are_saved_and_the_old_ids_still_load() {
        let mut b = Binds::default();
        let mut t = String::new();
        b.save_into(&mut t);
        assert!(t.is_empty(), "defaults are not written");
        b.set(B::Jump, Input::Btn(Btn::B));
        b.set(B::Sprint, Input::Stick(Ax::RightDown));
        b.set(B::Drop, Input::None);
        b.save_into(&mut t);
        let mut c = Binds::default();
        for l in t.lines() {
            let (k, v) = l.split_once(':').unwrap();
            assert!(c.load(k.strip_prefix("pad_").unwrap(), v));
        }
        assert_eq!(b, c);
        // `pad_wheel:3` of the old build is the hotbar radial now; `pad_gui_select` was every click of a screen
        let mut d = Binds::default();
        assert!(d.load("wheel", "3") && d.get(B::HotbarItemSelectRadial) == Input::Btn(Btn::Y));
        assert!(d.load("gui_select", "1"));
        assert_eq!((d.get(B::GuiPress), d.get(B::InvSelect), d.get(B::VmouseLclick)), (Input::Btn(Btn::B), Input::Btn(Btn::B), Input::Btn(Btn::B)));
        assert!(!d.load("nonsense", "1") && !d.load("jump", "x"));
    }
}
