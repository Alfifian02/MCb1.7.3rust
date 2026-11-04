#!/usr/bin/env python3
"""Writes mc-rs/assets/immersive.rgba: the Immersive Interfaces resource pack (Shrimpsnail, Beta V0.8.2) as the HUD's SECOND texture,
256 x 1600 RGBA8 (`render::hud::IMMERSIVE`, `IMM_H`). Run from the repo root:
    python3 tools/gen_immersive_ui.py _6Immersive_8__6Interfaces_8.zip      (needs Pillow)

The pack is a Java resource pack: its panels are 512 x 512 files whose top-left 256 x 256 quarter is the picture (1 texel = 1 GUI unit), and a
core shader (`shaders/include/interfaces.glsl`) draws that quarter over the vanilla quad, 40 units left of and 45 units above the vanilla panel's
origin. mc-rs has no such shader, so this script cuts the pictures out and `craft.rs` / `lib.rs` place them (the same 40 / 45 offsets).

Rows of the sheet (each panel 256 rows):
    0    inventory        (a modern-looking bag: the 2x2 grid sits where the workbench's 3x3 does, `craft::layout`)
    256  crafting table
    512  furnace          + 1.20's `lit_progress` (14 x 14) at (232, 512) and `burn_progress` (24 x 16) at (232, 526), in the 28 free columns
    768  dispenser
    1024 chest, 3 rows    = strip 2 of `generic_54.png` (1536 x 256: six 256-wide strips, strip n = n + 1 rows)
    1280 chest, 6 rows    = strip 5
    1536 hotbar (182 x 22) at (0, 1536), selector (24 x 23) at (0, 1558)
The marker pixel in each panel's corner (the shader's flags) has alpha 1, so the HUD shader (it drops alpha < 0.5) never draws it.
Credit and licence: Immersive Interfaces by Shrimpsnail; the owner has the author's permission (see README, Credits)."""
import io, sys, zipfile
from PIL import Image

PACK = sys.argv[1] if len(sys.argv) > 1 else "_6Immersive_8__6Interfaces_8.zip"
z = zipfile.ZipFile(PACK)
root = "assets/minecraft/textures/gui/"


def png(name):
    return Image.open(io.BytesIO(z.read(root + name))).convert("RGBA")


W, H = 256, 1600
out = Image.new("RGBA", (W, H), (0, 0, 0, 0))


def clear_marker(im):
    """The shader's flag pixel (alpha 1 or 2 ...) is not part of the picture: make it clear."""
    px = im.load()
    for x in range(min(2, im.width)):
        for y in range(min(2, im.height)):
            if px[x, y][3] < 8:
                px[x, y] = (0, 0, 0, 0)
    return im


for row, name in enumerate(["inventory", "crafting_table", "furnace", "dispenser"]):
    im = png("container/" + name + ".png").crop((0, 0, 256, 256))
    out.paste(clear_marker(im), (0, 256 * row))

g = png("container/generic_54.png")
for row, strip in ((4, 2), (5, 5)):
    im = g.crop((256 * strip, 0, 256 * strip + 256, 256))
    out.paste(clear_marker(im), (0, 256 * row))

out.paste(png("sprites/container/furnace/lit_progress.png"), (232, 512))
out.paste(png("sprites/container/furnace/burn_progress.png"), (232, 526))
out.paste(png("sprites/hud/hotbar.png"), (0, 1536))
out.paste(png("sprites/hud/hotbar_selection.png"), (0, 1558))

# The art's right 28 columns are free (the pictures end at column 227): the furnace's two sprites sit there, so nothing may overlap them.
assert all(out.getpixel((x, 512 + y))[3] == 0 for x in range(228, 232) for y in range(0, 256)), "furnace art reaches column 228"

open("mc-rs/assets/immersive.rgba", "wb").write(out.tobytes())
print("wrote mc-rs/assets/immersive.rgba", out.size)

if "--preview" in sys.argv:
    # Slot boxes of `craft::layout` over the art, for a look: (panel row, [(ux, uy) ...], art offset in y)
    from PIL import ImageDraw
    bag = [(8 + 18 * (i % 9), 84 + 18 * (i // 9 - 1)) for i in range(9, 36)] + [(8 + 18 * i, 142) for i in range(9)]
    wb = [(124, 35)] + [(30 + 18 * (i % 3), 17 + 18 * (i // 3)) for i in range(9)]
    inv = [(124, 35)] + [(30 + 18 * (i % 2), 17 + 18 * (i // 2)) for i in range(4)] + [(8, 8 + 18 * k) for k in range(4)]
    fur = [(56, 17), (56, 53), (116, 35)]
    disp = [(62 + 18 * (i % 3), 17 + 18 * (i // 3)) for i in range(9)]
    chest = lambda n: [(8 + 18 * (i % 9), 18 + 18 * (i // 9)) for i in range(9 * n)] + [(x, y + (n - 3) * 18 - 0) for x, y in []]
    sheet = Image.new("RGB", (256 * 3 + 12, 256 * 2 + 6), (40, 40, 40))
    # (row index, slots, offset y): chest bags sit 18 per row higher than the 3-row one
    jobs = [(0, inv + bag, 45), (1, wb + bag, 45), (2, fur + bag, 45), (3, disp + bag, 45)]
    for n, row in ((3, 4), (6, 5)):
        d = (n - 4) * 18
        slots = chest(n) + [(8 + 18 * (i % 9), 103 + d + 18 * (i // 9 - 1)) for i in range(9, 36)] + [(8 + 18 * i, 161 + d) for i in range(9)]
        jobs.append((row, slots, 44 + (3 - n) * 18))
    for k, (row, slots, oy) in enumerate(jobs):
        bg = Image.new("RGBA", (256, 256), (110, 110, 150, 255))
        bg.alpha_composite(out.crop((0, 256 * row, 256, 256 * row + 256)))
        d = ImageDraw.Draw(bg)
        for ux, uy in slots:
            d.rectangle([ux + 40, uy + oy, ux + 55, uy + oy + 15], outline=(0, 255, 0, 255))
        sheet.paste(bg.convert("RGB"), ((k % 3) * 262, (k // 3) * 262))
    sheet.save("immersive_preview.png")
    print("wrote immersive_preview.png")
