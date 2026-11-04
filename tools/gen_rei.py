"""Writes mc-rs/assets/rei.rgba (256 x 64 RGBA): the REI block of the HUD texture (`render::hud::sprites::rei`).
Source: the "Immersive Interfaces REI support" pack, bundled at tools/packs/immersive_rei (light and `_dark`).
Two themes of 32 rows (light at 0, dark at 32). Per theme:
  4 panels as 12 x 12 nine-slices (border 4, one 4 px texel row of edge / centre each) at x = 0, 12, 24, 36:
  default, lighter, none, slot (`sprites/widget/panel_*.png`, 150 x 66, border 4 in the .mcmeta);
  the dark arrow (22 x 15, `display*.png` at 107, 92) at x = 48 and the white arrow (22 x 16, at 83, 91) at x = 72.
  the favorite star (9 x 9, drawn here: the pack has none): filled gold at x = 96, outline at x = 106 (grey per theme).
Run from the repo root (needs Pillow):  python3 tools/gen_rei.py"""
from PIL import Image

root = "tools/packs/immersive_rei/assets/roughlyenoughitems/textures/gui/"
sheet = Image.new("RGBA", (256, 64), (0, 0, 0, 0))

def nine(im):
    """150 x 66, border 4 -> 12 x 12: corners, one 4 px slice of each edge, one of the centre."""
    assert im.size == (150, 66)
    o = Image.new("RGBA", (12, 12), (0, 0, 0, 0))
    xs, ys = [(0, 0), (70, 4), (146, 8)], [(0, 0), (31, 4), (62, 8)]
    for sx, dx in xs:
        for sy, dy in ys:
            o.paste(im.crop((sx, sy, sx + 4, sy + 4)), (dx, dy))
    return o

STAR = [
    "....#....",
    "....#....",
    "...###...",
    "#########",
    ".#######.",
    "..#####..",
    "..#####..",
    ".###.###.",
    ".##...##.",
]

def star(on, ink):
    o = Image.new("RGBA", (9, 9), (0, 0, 0, 0))
    lit = lambda x, y: 0 <= x < 9 and 0 <= y < 9 and STAR[y][x] == "#"
    for y in range(9):
        for x in range(9):
            if not lit(x, y):
                continue
            edge = not all(lit(x + dx, y + dy) for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)))
            if on:
                o.putpixel((x, y), (190, 130, 20, 255) if edge else (255, 214, 64, 255))
            elif edge:
                o.putpixel((x, y), ink + (255,))
    return o

for t, suf in enumerate(["", "_dark"]):
    y = t * 32
    for i, name in enumerate(["default", "lighter", "none", "slot"]):
        sheet.paste(nine(Image.open(f"{root}sprites/widget/panel_{name}{suf}.png").convert("RGBA")), (i * 12, y))
    d = Image.open(f"{root}display{suf}.png").convert("RGBA")
    sheet.paste(d.crop((107, 92, 129, 107)), (48, y))
    sheet.paste(d.crop((83, 91, 105, 107)), (72, y))
    sheet.paste(star(True, (0, 0, 0)), (96, y))
    sheet.paste(star(False, (90, 90, 90) if t == 0 else (200, 200, 200)), (106, y))
open("mc-rs/assets/rei.rgba", "wb").write(sheet.tobytes())
print("rei.rgba", len(sheet.tobytes()), "bytes")
