#!/usr/bin/env python3
"""Writes mc-rs/assets/gui.rgba: the pack's gui/widgets.png (hotbar strip, selector) over gui/icons.png (crosshair, hearts, armor,
air bubbles, and Stamina's overlay), 256 x 512 RGBA, from a modern-named resource pack (Legacy Restoration). Run from the repo root:
python3 tools/gen_gui.py Legacy_Restoration.zip  (needs Pillow). The sprites' places are b1.7.3's (`GuiIngame`): they have not moved.
Also writes mc-rs/assets/containers.rgba: gui/container/{inventory,crafting_table,furnace,generic_54}.png stacked (256 x 1024). The
pack's inventory keeps the offhand slot of later versions at (76, 61); b1.7.3 has none, so it is painted over with the panel grey."""
import io, sys, zipfile
from PIL import Image

z = zipfile.ZipFile(sys.argv[1] if len(sys.argv) > 1 else "Legacy_Restoration.zip")
root = next(n for n in z.namelist() if n.endswith("textures/gui/widgets.png"))[: -len("widgets.png")]
png = lambda n: Image.open(io.BytesIO(z.read(root + n))).convert("RGBA")
out = Image.new("RGBA", (256, 512))
out.paste(png("widgets.png"), (0, 0))
out.paste(png("icons.png"), (0, 256))
# Stamina's hearts overlay (the mod's heart_overlay.png: a grey 0x7a heart, drawn tinted to alpha 0x9c / 0xcc) in the corner icons.png leaves
# empty: HUD texture (192, 512), `hud::sprites::STAMINA`.
STAMINA = ['         ', '  XX XX  ', ' XXXXXXX ', ' XXXXXXX ', ' XXXXXXX ', '  XXXXX  ', '   XXX   ', '    X    ', '         ']
for y, row in enumerate(STAMINA):
    for x, c in enumerate(row):
        if c == "X":
            out.putpixel((192 + x, 256 + y), (0x7A, 0x7A, 0x7A, 255))
open("mc-rs/assets/gui.rgba", "wb").write(out.tobytes())
print("wrote mc-rs/assets/gui.rgba")

box = Image.new("RGBA", (256, 1024))
for i, name in enumerate(["inventory", "crafting_table", "furnace", "generic_54"]):
    box.paste(png("container/" + name + ".png"), (0, 256 * i))
grey = box.getpixel((170, 8))  # the panel's own colour, inside its right margin
box.paste(Image.new("RGBA", (18, 18), grey), (76, 61))
# The inventory's crafting block (2x2 grid, arrow, result frame: x 96..172, y 16..55) sits CRAFT_SHIFT units further left than the
# pack has it (keep in step with `craft::CRAFT_SHIFT`); the strip it leaves on the right is painted over with the panel grey.
CRAFT_SHIFT = 6
blk = box.crop((96, 16, 173, 56))
box.paste(Image.new("RGBA", (77, 40), grey), (96, 16))
box.paste(blk, (96 - CRAFT_SHIFT, 16))
open("mc-rs/assets/containers.rgba", "wb").write(box.tobytes())
print("wrote mc-rs/assets/containers.rgba")
