#!/usr/bin/env python3
"""Generates mc-rs/src/input/bind_defs.rs from Controlify's own sources (LGPL-3.0-or-later, isXander):

  * the registry: `bindings/ControlifyBindings.java` (every `registerBinding`: id, category, allowed contexts);
  * the defaults: `assets/controlify/controllers/default_bind/default.json`;
  * the display names: `assets/controlify/lang/en_us.json`.

What this port adds by hand lives in the tables below (`LIVE`, `DEFAULT_OVERRIDE`, `EXTRA`): whether the bind does anything in
b1.7.3 / mc-rs, and the defaults of the binds default.json leaves empty but this port needs a button for.

Usage: python3 tools/gen_bindings.py <Controlify-main dir> [out.rs]
"""
import json
import re
import sys

root = sys.argv[1]
out = sys.argv[2] if len(sys.argv) > 2 else "mc-rs/src/input/bind_defs.rs"
src = open(f"{root}/src/main/java/dev/isxander/controlify/bindings/ControlifyBindings.java", encoding="utf-8").read()
res = f"{root}/src/main/resources/assets/controlify"
defaults = json.load(open(f"{res}/controllers/default_bind/default.json", encoding="utf-8"))["defaults"]
lang = json.load(open(f"{res}/lang/en_us.json", encoding="utf-8"))

# ---- what this port wires (the others are in the registry, saved and rebindable in the file, but hidden from the page) ----
LIVE = set("""
walk_forward walk_backward strafe_left strafe_right look_up look_down look_left look_right
jump sprint sneak attack use drop drop_stack pause change_perspective next_slot prev_slot hotbar_item_select_radial
hotbar_slot_1 hotbar_slot_2 hotbar_slot_3 hotbar_slot_4 hotbar_slot_5 hotbar_slot_6 hotbar_slot_7 hotbar_slot_8 hotbar_slot_9
inventory inv_select inv_quick_move inv_take_half drop_inventory toggle_debug_menu
gui_press gui_back gui_next_tab gui_prev_tab gui_abstract_action_1 gui_abstract_action_2 gui_navi_up gui_navi_down gui_navi_left gui_navi_right
radial_axis_up radial_axis_down radial_axis_left radial_axis_right
vmouse_move_up vmouse_move_down vmouse_move_left vmouse_move_right vmouse_snap_up vmouse_snap_down vmouse_snap_left vmouse_snap_right
vmouse_lclick vmouse_rclick vmouse_shift_click vmouse_scroll_up vmouse_scroll_down vmouse_toggle
zoom
""".split())

# default.json has nothing for these; the port does (the old phase 1-2 buttons, which the hotbar wheel / info screen / zoom had)
DEFAULT_OVERRIDE = {
    "hotbar_item_select_radial": "button/dpad_up",
    "toggle_debug_menu": "button/dpad_left",
    "zoom": "button/dpad_right",
}

# binds that are this port's own (no Controlify id): id, display name, category, contexts, description
EXTRA = [
    ("zoom", "Zoom", "Gameplay", ["IN_GAME"]),
]

CAT = {
    "MOVEMENT_CATEGORY": "Movement",
    "GAMEPLAY_CATEGORY": "Gameplay",
    "INVENTORY_CATEGORY": "Inventory",
    "CREATIVE_CATEGORY": "Creative",
    "MISC_CATEGORY": "Misc",
    "DEBUG_CATEGORY": "Debug",
    "GUI_CATEGORY": "Gui",
    "RADIAL_CATEGORY": "Radial",
    "VMOUSE_CATEGORY": "VMouse",
}
CTX = {
    "IN_GAME": "IN_GAME",
    "ANY_SCREEN": "SCREEN",
    "REGULAR_SCREEN": "REGULAR",
    "CONTAINER": "CONTAINER",
    "RADIAL_MENU": "RADIAL",
    "V_MOUSE_CURSOR": "VM_CURSOR",
    "V_MOUSE_COMPAT": "VM_COMPAT",
}
BTN = {
    "south": "Btn::A", "east": "Btn::B", "west": "Btn::X", "north": "Btn::Y",
    "left_shoulder": "Btn::Lb", "right_shoulder": "Btn::Rb", "left_stick": "Btn::L3", "right_stick": "Btn::R3",
    "start": "Btn::Start", "back": "Btn::Back",
    "dpad_up": "Btn::Up", "dpad_down": "Btn::Down", "dpad_left": "Btn::Left", "dpad_right": "Btn::Right",
}
AXIS = {
    "left_trigger": "Input::Btn(Btn::Lt)", "right_trigger": "Input::Btn(Btn::Rt)",
    "left_stick_up": "Input::Stick(Ax::LeftUp)", "left_stick_down": "Input::Stick(Ax::LeftDown)",
    "left_stick_left": "Input::Stick(Ax::LeftLeft)", "left_stick_right": "Input::Stick(Ax::LeftRight)",
    "right_stick_up": "Input::Stick(Ax::RightUp)", "right_stick_down": "Input::Stick(Ax::RightDown)",
    "right_stick_left": "Input::Stick(Ax::RightLeft)", "right_stick_right": "Input::Stick(Ax::RightRight)",
}


def rust_input(spec):
    """`button/south` or `axis/left_stick_up` -> the Rust expression."""
    kind, name = spec.split("/")
    if kind == "button":
        return f"Input::Btn({BTN[name]})"
    return AXIS[name]


def camel(s):
    return "".join(p.capitalize() for p in s.split("_"))


binds = []
pat = re.compile(r"public static final InputBindingSupplier (\w+) =\s*ControlifyBindApi\.get\(\)\.registerBinding\((.*?)\);\n", re.S)
for m in pat.finditer(src):
    body = m.group(2)
    id_ = re.search(r'\.id\("controlify", "(\w+)"\)', body)
    cat = re.search(r"\.category\((\w+)\)", body)
    ctxs = re.findall(r"BindContext\.(\w+)", body)
    if not (id_ and cat and ctxs):
        continue
    id_ = id_.group(1)
    name = lang.get(f"controlify.binding.controlify.{id_}")
    if name is None:  # the debug radial's entries carry their own `.name(...)`: the radial's label
        name = lang.get(re.search(r'\.name\(Component\.translatable\("([^"]+)"\)\)', body).group(1)) if re.search(r'\.name\(Component\.translatable\("([^"]+)"\)\)', body) else id_.replace("_", " ").title()
    binds.append((id_, name, CAT[cat.group(1)], [CTX[c] for c in ctxs]))

for id_, name, cat, ctxs in EXTRA:
    binds.append((id_, name, cat, ctxs))

lines = []
w = lines.append
w("//! GENERATED by `tools/gen_bindings.py` from Controlify's `ControlifyBindings`, `default_bind/default.json` and `en_us.json` (LGPL-3.0-or-later, isXander). Do not edit.")
w("//!")
w("//! Every binding Controlify registers (its id, category, allowed contexts and default input), plus this port's own (`zoom`). `live` is whether the bind")
w("//! does anything in b1.7.3 / mc-rs (the others are saved and kept, hidden from the Controls page: no chat, swap hands, pick block, game modes...).")
w("")
w("use super::bindings::{ctx::*, Ax, Cat, Def, Input};")
w("use super::gamepad::Btn;")
w("")
w("#[derive(Clone, Copy, PartialEq, Eq, Debug)]")
w("pub enum B {")
for b in binds:
    w(f"    {camel(b[0])},")
w("}")
w("")
w(f"pub const COUNT: usize = {len(binds)};")
w("")
w("impl B {")
w("    pub const ALL: [B; COUNT] = [")
for b in binds:
    w(f"        B::{camel(b[0])},")
w("    ];")
w("}")
w("")
w("pub static DEFS: [Def; COUNT] = [")
n_live = 0
for id_, name, cat, ctxs in binds:
    if id_ in DEFAULT_OVERRIDE:
        d = rust_input(DEFAULT_OVERRIDE[id_])
    else:
        e = defaults.get(f"controlify:{id_}")
        if e is None or "type" in e:
            d = "Input::None"
        elif "button" in e:
            d = rust_input("button/" + e["button"].split("/")[-1])
        else:
            d = rust_input("axis/" + e["axis"].split("/")[-1])
    live = id_ in LIVE
    n_live += live
    ctx = " | ".join(ctxs)
    w(f'    Def {{ id: "{id_}", name: "{name}", cat: Cat::{cat}, ctx: {ctx}, default: {d}, live: {str(live).lower()} }},')
w("];")
missing = LIVE - {b[0] for b in binds}
if missing:
    sys.exit(f"LIVE names no such bind: {sorted(missing)}")
open(out, "w", encoding="utf-8").write("\n".join(lines) + "\n")
print(f"{len(binds)} binds ({n_live} live) -> {out}")
