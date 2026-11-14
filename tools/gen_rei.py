"""Writes mc-rs/assets/rei.rgba (256 x 128 RGBA): the REI block of the HUD texture (`render::hud::sprites::rei`).
Source: the "Immersive Interfaces REI support" pack, bundled at tools/packs/immersive_rei (light and `_dark`), plus three small REI 26.2 sprites
the pack leaves alone (`arrow_left.png`, `arrow_right.png`, `shapeless_icon_1x.png`, MIT, shedaniel; copied into tools/packs/immersive_rei/rei_base/).
Two themes of 64 rows (light at 0, dark at 64). Per theme, with y0 the theme's first row:
  y0 + 0   4 panels as 12 x 12 nine-slices (border 4, one 4 px texel row of edge / centre each) at x = 0, 12, 24, 36:
           default, lighter, none, slot (`sprites/widget/panel_*.png`, 150 x 66, border 4 in the .mcmeta);
           the dark arrow (22 x 15, `display*.png` at 107, 92) at x = 48 and the white arrow (22 x 16, at 83, 91) at x = 72;
           the favorite star (9 x 9, drawn here: the pack has none): filled gold at x = 96, outline at x = 106 (grey per theme).
  y0 + 16  3 buttons as 6 x 6 nine-slices (border 2) from the pack's `button*.png` strips: normal x = 0, lit (hover / selected) x = 6, disabled x = 12;
           arrow left / right 8 x 8 at x = 20 / 28; the shapeless icon 8 x 8 at x = 36; the config gear 14 x 14 (`recipecontainer*.png` 0, 0) at x = 46.
  y0 + 32  the category tabs of `TabWidget`: unselected 28 x 26 at x = 0, selected 28 x 30 at x = 28 (`recipecontainer*.png`, 0, 192 and 28, 192).
Run from the repo root (needs Pillow):  python3 tools/gen_rei.py"""
from PIL import Image

root = "tools/packs/immersive_rei/assets/roughlyenoughitems/textures/gui/"
base = "tools/packs/immersive_rei/rei_base/"
THEME_H = 64
sheet = Image.new("RGBA", (256, 2 * THEME_H), (0, 0, 0, 0))


def nine(im):
    """150 x 66, border 4 -> 12 x 12: corners, one 4 px slice of each edge, one of the centre."""
    assert im.size == (150, 66)
    o = Image.new("RGBA", (12, 12), (0, 0, 0, 0))
    xs, ys = [(0, 0), (70, 4), (146, 8)], [(0, 0), (31, 4), (62, 8)]
    for sx, dx in xs:
        for sy, dy in ys:
            o.paste(im.crop((sx, sy, sx + 4, sy + 4)), (dx, dy))
    return o


def nine_button(strip):
    """A 256 x 80 button strip, border 2 -> 6 x 6."""
    assert strip.size == (256, 80)
    o = Image.new("RGBA", (6, 6), (0, 0, 0, 0))
    xs, ys = [(0, 0), (127, 2), (254, 4)], [(0, 0), (39, 2), (78, 4)]
    for sx, dx in xs:
        for sy, dy in ys:
            o.paste(strip.crop((sx, sy, sx + 2, sy + 2)), (dx, dy))
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
    y = t * THEME_H
    for i, name in enumerate(["default", "lighter", "none", "slot"]):
        sheet.paste(nine(Image.open(f"{root}sprites/widget/panel_{name}{suf}.png").convert("RGBA")), (i * 12, y))
    d = Image.open(f"{root}display{suf}.png").convert("RGBA")
    sheet.paste(d.crop((107, 92, 129, 107)), (48, y))
    sheet.paste(d.crop((83, 91, 105, 107)), (72, y))
    sheet.paste(star(True, (0, 0, 0)), (96, y))
    sheet.paste(star(False, (90, 90, 90) if t == 0 else (200, 200, 200)), (106, y))

    b = Image.open(f"{root}button{suf}.png").convert("RGBA")
    y1 = y + 16
    for i, row in enumerate([80, 160, 0]):  # normal, lit, disabled
        sheet.paste(nine_button(b.crop((0, row, 256, row + 80))), (i * 6, y1))
    sheet.paste(Image.open(base + "arrow_left.png").convert("RGBA"), (20, y1))
    sheet.paste(Image.open(base + "arrow_right.png").convert("RGBA"), (28, y1))
    sheet.paste(Image.open(base + "shapeless_icon_1x.png").convert("RGBA"), (36, y1))
    rc = Image.open(f"{root}recipecontainer{suf}.png").convert("RGBA")
    sheet.paste(rc.crop((0, 0, 14, 14)), (46, y1))

    y2 = y + 32
    sheet.paste(rc.crop((0, 192, 28, 192 + 26)), (0, y2))
    sheet.paste(rc.crop((28, 192, 56, 192 + 30)), (28, y2))

open("mc-rs/assets/rei.rgba", "wb").write(sheet.tobytes())
print("rei.rgba", len(sheet.tobytes()), "bytes")
