#!/usr/bin/env python3
"""Writes mc-rs/assets/padbtn.rgba (256 x 156 RGBA): the controller button pictures of AgentMindStorm's "Controller Tooltip Selector" pack, the block
of the HUD texture at `hud::PAD_Y`. Three styles (the pack's `xbox_one_bedrock`, `ps4_bedrock` and `switch_bedrock` subpacks) of 52 rows each, in
that order (`PadCfg::style` 0 / 1 / 2); in a style the 16 buttons are in `Btn::ALL` order, 8 to a row, 26 x 26 each:
  A (face_button_down), B (right), X (left), Y (up), LB, RB, LT, RT, L3 (stick_left), R3 (stick_right), Start, Back (select), D-Up, D-Down, D-Left, D-Right.
Keep in step with `hud::sprites::pad`. The art is clear or opaque (`fs_main` drops texels under alpha 0.5); transparent texels are white, so the
sharp bilinear's blend across a picture's edge does not darken it. A picture a few px over 26 is cut to it (all its ink is inside).
Run from the repo root: python3 tools/gen_pad_buttons.py path/to/controllertooltips (needs Pillow)."""
import os
import sys
from PIL import Image

src = sys.argv[1] if len(sys.argv) > 1 else "controllertooltips"
NAMES = ["face_button_down", "face_button_right", "face_button_left", "face_button_up", "bumper_left", "bumper_right", "left_trigger", "right_trigger",
         "stick_left", "stick_right", "start_button", "select_button", "dpad_up", "dpad_down", "dpad_left", "dpad_right"]
out = Image.new("RGBA", (256, 156), (255, 255, 255, 0))
# (subpack, file prefix): the pack's `switch_` files are the ones with Nintendo's letters in place (B below, A right, Y left, X above)
for s, (sub, pre) in enumerate([("xbox_one_bedrock", "xbox"), ("ps4_bedrock", "ps4"), ("switch_bedrock", "switch")]):
    d = f"{src}/subpacks/{sub}/textures/ui"
    for i, n in enumerate(NAMES):
        f = next(f"{d}/{p}_{n}.png" for p in (pre, "xbox", "generic") if os.path.exists(f"{d}/{p}_{n}.png"))
        im = Image.open(f).convert("RGBA").crop((0, 0, 26, 26))
        for y in range(26):
            for x in range(26):
                r, g, b, a = im.getpixel((x, y))
                if a:
                    out.putpixel((i % 8 * 26 + x, s * 52 + i // 8 * 26 + y), (r, g, b, 255))
open("mc-rs/assets/padbtn.rgba", "wb").write(out.tobytes())
print("wrote mc-rs/assets/padbtn.rgba")
