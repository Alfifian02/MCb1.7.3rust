#!/usr/bin/env python3
"""Writes mc-rs/assets/dpad.rgba (256 x 256 RGBA): the touch d-pad sprites of the Stone D-pad pack (ThreeSeven0Zero, `default_size`).
Row 0: the 8 directions in the order of `hud::sprites::DPAD` (up, down, left, right, up_left, up_right, down_left, down_right), one
22-px cell each (the four arrows are 22 x 22, the four corners 18 x 18 at the cell's top-left); row 1 (y 22): the same pressed; row 2
(y 64): sneak, sneak pressed, jump, jump pressed, 64 x 64 each.
The pack only draws the two FORWARD corners (up_left, up_right; its down_left / down_right are 1 x 1 blanks, Bedrock only shows the
forward ones). mc-rs wants all eight, so the lower two are the upper ones flipped vertically: the arrow glyph only, painted on the
same blank button (the bevel stays lit from the top left).
Run from the repo root: python3 tools/gen_dpad.py Stone_D-pad.mcpack  (needs Pillow)."""
import io, sys, zipfile
from PIL import Image

z = zipfile.ZipFile(sys.argv[1] if len(sys.argv) > 1 else "Stone_D-pad.mcpack")
ctl = lambda n: Image.open(io.BytesIO(z.read(n))).convert("RGBA")
sub = "subpacks/default_size/textures/gui/controls/"
out = Image.new("RGBA", (256, 256))


def corner(name):
    """The 18 x 18 button of a forward corner and its glyph (the pixels darker than the button), as a bool mask."""
    im = ctl(sub + name + ".png")
    im = im.crop(im.getchannel("A").getbbox())
    lum = lambda p: (p[0] + p[1] + p[2]) / 3
    glyph = {(x, y) for y in range(2, 16) for x in range(2, 16) if lum(im.getpixel((x, y))) < 70}
    return im, glyph


def corners(suffix):
    """up_left, up_right, down_left, down_right buttons."""
    res = {}
    for n in ("up_left", "up_right"):
        im, g = corner(n + suffix)
        blank = im.copy()
        flat = [im.getpixel((x, y)) for y in range(2, 16) for x in range(2, 16) if (x, y) not in g]
        fill = tuple(sum(c[i] for c in flat) // len(flat) for i in range(4))
        for x, y in g:
            blank.putpixel((x, y), fill)
        dark = [im.getpixel(q) for q in g]
        dark = tuple(sum(c[i] for c in dark) // len(dark) for i in range(4))
        res[n] = im
        low = blank.copy()
        for x, y in g:
            low.putpixel((x, 17 - y), im.getpixel((x, y)))
        res["down" + n[2:]] = low
    return res


for suffix, y in (("", 0), ("_pressed", 22)):
    for i, d in enumerate(["up", "down", "left", "right"]):
        out.paste(ctl(sub + d + suffix + ".png"), (22 * i, y))
    c = corners(suffix)
    for i, d in enumerate(["up_left", "up_right", "down_left", "down_right"]):
        out.paste(c[d], (22 * (4 + i), y))
for i, n in enumerate(["sneak_dpad", "sneak_dpad_pressed", "jump_dpad", "jump_dpad_pressed"]):
    out.paste(ctl(sub + n + ".png"), (64 * i, 64))
open("mc-rs/assets/dpad.rgba", "wb").write(out.tobytes())
print("wrote mc-rs/assets/dpad.rgba")
