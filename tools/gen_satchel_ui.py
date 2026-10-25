#!/usr/bin/env python3
"""Writes mc-rs/assets/satchel.rgba: the pixel art of the satchel and its recipe book (`satchel.rs`), 256 x 256 RGBA, the last block of the
HUD texture (`hud::SATCHEL_Y`). Everything is drawn here from code (no outside art, nothing to license): leather with grain, stitches and
rivets, an inset Minecraft-style slot, parchment with a red stitched binding, a burgundy cover. Run from the repo root:
python3 tools/gen_satchel_ui.py   (needs Pillow; `--preview out.png` also writes a 4x picture of the sheet).
The sprites' places are the `SPRITES` table below; `hud::sprites::sat` repeats them (a test in `hud.rs` checks the two agree on what is
opaque). Tiles (`PARCH`, the 9-slice edges) are periodic, so `hud::push_tiled` / `push_nine` repeat them without a seam."""
import sys
from PIL import Image

W = H = 256
sheet = Image.new("RGBA", (W, H), (0, 0, 0, 0))

# name: (x, y, w, h)
SPRITES = {
    "BAG": (0, 0, 32, 32), "COVER": (32, 0, 32, 32), "PARCH": (64, 0, 32, 32), "PAGE_EDGE": (96, 0, 16, 16),
    "BINDING": (112, 0, 8, 16), "CURL": (120, 0, 12, 12), "ARROW": (132, 0, 16, 12),
    "POCKET": (0, 32, 16, 16), "BELT": (16, 32, 16, 16), "BUCKLE": (32, 32, 12, 16), "CLASP": (44, 32, 12, 20), "STRAP": (56, 32, 8, 16),
    "SLOT": (64, 32, 18, 18), "RIM": (82, 32, 18, 18), "BTN": (100, 32, 16, 16), "BTN_ON": (116, 32, 16, 16), "TAB": (132, 32, 16, 16),
    "ENTRY": (148, 32, 16, 16), "ENTRY_SEL": (164, 32, 16, 16), "CARD": (180, 32, 16, 16), "GEM": (196, 32, 8, 8), "RULE": (204, 32, 16, 4)
}


def hx(s):
    return tuple(int(s[i:i + 2], 16) for i in (1, 3, 5)) + (255,)


def h2(x, y, seed=0):
    """A deterministic hash in 0..1."""
    n = (x * 374761393 + y * 668265263 + seed * 2147483647) & 0xFFFFFFFF
    n = ((n ^ (n >> 13)) * 1274126177) & 0xFFFFFFFF
    return ((n ^ (n >> 16)) & 0xFFFF) / 65535.0


def new(name):
    return Image.new("RGBA", SPRITES[name][2:], (0, 0, 0, 0))


def put(name, img):
    x, y, w, h = SPRITES[name]
    assert img.size == (w, h), name
    sheet.paste(img, (x, y))


def shade(c, f):
    return tuple(max(0, min(255, int(v * f))) for v in c[:3]) + (255,)


def mix(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3)) + (255,)


# ---- leather ----

def leather(name, base, border, outline_w=1, studs=True, seam=True, seam_col=None, tooled=None, grain=0.12, period=16):
    """A 9-slice of leather: dark outline, a lit top / left and a shadowed bottom / right edge, grain that repeats every `period`
    pixels (so the stretched edges tile), a seam of stitches `border / 2` in, a stud in each corner."""
    img = new(name)
    w, h = img.size
    dark, light, edge = shade(base, 0.62), shade(base, 1.32), shade(base, 0.32)
    seam_col = seam_col or (214, 172, 106, 255)
    for y in range(h):
        for x in range(w):
            # grain: short horizontal fibres, periodic along both axes
            gx, gy = x % period, y % period
            r = h2(gx // 3, gy, 1)
            c = base
            if r < 0.18:
                c = shade(base, 1 - grain)
            elif r > 0.86:
                c = shade(base, 1 + grain)
            if h2(gx, gy, 7) > 0.93:
                c = shade(base, 0.8)
            img.putpixel((x, y), c)
    for y in range(h):
        for x in range(w):
            d = min(x, y, w - 1 - x, h - 1 - y)
            if d < outline_w:
                img.putpixel((x, y), edge)
            elif d == outline_w:
                # lit on the top / left, shadowed on the bottom / right
                top_left = min(x, y) <= min(w - 1 - x, h - 1 - y)
                img.putpixel((x, y), light if top_left else dark)
    if tooled:  # a blind-tooled line, a dark groove with a lit lip
        o = tooled
        for i in range(o, w - o):
            for j in (o, h - 1 - o):
                img.putpixel((i, j), shade(base, 0.7))
        for j in range(o, h - o):
            for i in (o, w - 1 - o):
                img.putpixel((i, j), shade(base, 0.7))
    if seam:
        s = border // 2 + 1
        for i in range(s + 2, w - s - 2):
            if (i // 2) % 2 == 0:
                for j in (s, h - 1 - s):
                    img.putpixel((i, j), seam_col)
                    img.putpixel((i, j + 1 if j == s else j - 1), shade(base, 0.55))
        for j in range(s + 2, h - s - 2):
            if (j // 2) % 2 == 0:
                for i in (s, w - 1 - s):
                    img.putpixel((i, j), seam_col)
                    img.putpixel((i + 1 if i == s else i - 1, j), shade(base, 0.55))
    if studs:
        s = border // 2 + 1
        for cx, cy in ((s, s), (w - 1 - s, s), (s, h - 1 - s), (w - 1 - s, h - 1 - s)):
            for dy in range(-1, 2):
                for dx in range(-1, 2):
                    col = (232, 196, 100, 255)
                    if dx == -1 and dy == -1:
                        col = (255, 240, 170, 255)
                    elif dx == 1 and dy == 1:
                        col = (130, 92, 30, 255)
                    elif dx != 0 and dy != 0 and (dx, dy) in ((1, -1), (-1, 1)):
                        col = (190, 150, 62, 255)
                    img.putpixel((cx + dx, cy + dy), col)
    put(name, img)
    return img


leather("BAG", hx("#5c3d24"), 8)
leather("COVER", hx("#6a2a24"), 8, tooled=5, seam_col=(228, 190, 100, 255), grain=0.1)


def pocket():
    """A darker leather pocket band: a lit lip along the top, stitched along the foot."""
    img = leather("POCKET", hx("#3f2817"), 4, studs=False, seam=False, grain=0.1, period=16)
    w, h = img.size
    for x in range(w):
        img.putpixel((x, 1), hx("#8c6538"))
        img.putpixel((x, 2), hx("#5d3f22"))
        if (x // 2) % 2 == 0:
            img.putpixel((x, h - 4), (200, 160, 98, 255))
    put("POCKET", img)


pocket()


def belt():
    img = leather("BELT", hx("#2c1a0e"), 4, studs=False, seam=False, grain=0.08, period=16)
    w, h = img.size
    for x in range(w):  # two rows of stitches, holes every 8
        if (x // 2) % 2 == 0:
            img.putpixel((x, 3), (200, 160, 98, 255))
            img.putpixel((x, h - 4), (200, 160, 98, 255))
    for x0 in (3, 11):
        for dx in (0, 1):
            img.putpixel((x0 + dx, 7), hx("#120905"))
            img.putpixel((x0 + dx, 8), hx("#120905"))
    put("BELT", img)


belt()

# ---- metal ----


def metal(name, rect_outer, rect_inner=None, extra=None):
    img = new(name)
    w, h = img.size
    for y in range(h):
        for x in range(w):
            c = None
            if x < w and y < h:
                inner_hole = rect_inner and rect_inner[0] <= x < rect_inner[0] + rect_inner[2] and rect_inner[1] <= y < rect_inner[1] + rect_inner[3]
                if not inner_hole:
                    c = (232, 196, 100, 255)
                    if x == 0 or y == 0:
                        c = (255, 240, 170, 255)
                    if x == w - 1 or y == h - 1:
                        c = (130, 92, 30, 255)
                    if (x + y) % 5 == 0 and 0 < x < w - 1 and 0 < y < h - 1:
                        c = (214, 176, 80, 255)
                else:
                    c = (60, 38, 16, 255)
            img.putpixel((x, y), c if c else (0, 0, 0, 0))
    if extra:
        extra(img)
    put(name, img)


def buckle_extra(img):
    w, h = img.size
    for y in range(5, 11):  # the prong across the middle hole
        img.putpixel((w // 2, y), (255, 240, 170, 255))
        img.putpixel((w // 2 + 1, y), (130, 92, 30, 255))


metal("BUCKLE", None, (3, 3, 6, 10), buckle_extra)


def clasp_extra(img):
    w, h = img.size
    for (x, y) in ((0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)):  # round the corners
        img.putpixel((x, y), (0, 0, 0, 0))
    hole = (60, 38, 16, 255)
    for (x, y) in [(x, y) for y in (6, 7, 8, 9) for x in (4, 5, 6, 7) if not (y in (6, 9) and x in (4, 7))] + [(5, y) for y in (10, 11, 12, 13)] + [(6, y) for y in (10, 11, 12, 13)]:
        img.putpixel((x, y), hole)  # a keyhole


metal("CLASP", None, None, clasp_extra)

# the strap on the cover: a darker vertical band with stitched edges
img = new("STRAP")
for y in range(16):
    for x in range(8):
        c = hx("#3a1814")
        if h2(x, y, 3) > 0.85:
            c = hx("#2a100d")
        if x in (0, 7):
            c = hx("#1c0a08")
        if x in (1, 6) and (y // 2) % 2 == 0:
            c = (214, 172, 106, 255)
        img.putpixel((x, y), c)
put("STRAP", img)

# ---- slots, buttons, plates ----

img = new("SLOT")
for y in range(18):
    for x in range(18):
        c = hx("#2e2a30")
        if h2(x, y, 9) > 0.9:
            c = hx("#35303a")
        if y == 0 or x == 0:
            c = hx("#0e0b0d")
        elif y == 1 or x == 1:
            c = hx("#1b171c")
        if y == 17 or x == 17:
            c = hx("#8a6a48")
        elif y == 16 or x == 16:
            c = hx("#5a432b")
        img.putpixel((x, y), c)
put("SLOT", img)

img = new("RIM")
for i in range(18):
    for p in ((i, 0), (i, 17), (0, i), (17, i), (i, 1), (i, 16), (1, i), (16, i)):
        img.putpixel(p, (255, 255, 255, 255))
for p in ((1, 1), (16, 1), (1, 16), (16, 16)):
    pass
put("RIM", img)


def plate(name, base, outline, border=3, gloss=True, noise=0.06):
    img = new(name)
    w, h = img.size
    for y in range(h):
        for x in range(w):
            c = base if h2(x % 8, y % 8, 5) > 0.15 else shade(base, 1 - noise * 2)
            d = min(x, y, w - 1 - x, h - 1 - y)
            if d == 0:
                c = outline
            elif d == 1:
                c = shade(base, 1.28) if min(x, y) <= min(w - 1 - x, h - 1 - y) else shade(base, 0.7)
            img.putpixel((x, y), c)
    for (x, y) in ((0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)):
        img.putpixel((x, y), (0, 0, 0, 0))
    put(name, img)
    return img


plate("BTN", hx("#6f4c2c"), hx("#1d110a"))
plate("BTN_ON", hx("#9a6f2e"), hx("#2a1a08"))
plate("TAB", hx("#c19a62"), hx("#3a2410"))

# ---- parchment ----


def periodic_noise(x, y, cell, period, seed):
    """Smooth value noise that repeats every `period` pixels."""
    n = period // cell
    gx, gy = x / cell, y / cell
    x0, y0 = int(gx) % n, int(gy) % n
    x1, y1 = (x0 + 1) % n, (y0 + 1) % n
    fx, fy = gx - int(gx), gy - int(gy)
    fx, fy = fx * fx * (3 - 2 * fx), fy * fy * (3 - 2 * fy)
    a, b = h2(x0, y0, seed), h2(x1, y0, seed)
    c, d = h2(x0, y1, seed), h2(x1, y1, seed)
    return (a + (b - a) * fx) * (1 - fy) + (c + (d - c) * fx) * fy


TONES = [hx("#cfc19f"), hx("#d9ccab"), hx("#e3d7b7"), hx("#ece1c4")]
img = new("PARCH")
for y in range(32):
    for x in range(32):
        v = 0.6 * periodic_noise(x, y, 8, 32, 11) + 0.4 * periodic_noise(x, y, 4, 32, 12)
        c = TONES[min(3, int(v * 4.4))]
        r = h2(x, y, 13)
        if r > 0.985:
            c = hx("#f4ecd4")
        elif r < 0.012:
            c = hx("#bfb08c")
        img.putpixel((x, y), c)
put("PARCH", img)

# the page's rim: aged gold, then a lighter band; the inside is clear (the tile shows through); the corners are cut round
img = new("PAGE_EDGE")
for y in range(16):
    for x in range(16):
        d = min(x, y, 15 - x, 15 - y)
        c = None
        if d == 0:
            c = hx("#7a5a2e")
        elif d == 1:
            c = hx("#b99a58")
        elif d == 2:
            c = hx("#d2b676")
        elif d == 3:
            c = hx("#e0cc98")
        if c is not None and h2(x % 8, y % 8, 21) > 0.82 and d > 0:
            c = shade(c, 0.93)
        img.putpixel((x, y), c if c else (0, 0, 0, 0))
for (x, y) in ((0, 0), (15, 0), (0, 15), (15, 15)):
    img.putpixel((x, y), (0, 0, 0, 0))
for (x, y) in ((1, 1), (14, 1), (1, 14), (14, 14)):
    img.putpixel((x, y), hx("#7a5a2e"))
put("PAGE_EDGE", img)

# the binding at the spine: both pages sink into it, a red thread stitches it
RED, RED_D = hx("#ec5a56"), hx("#a8322f")
img = new("BINDING")
for y in range(16):
    for x in range(8):
        d = min(x, 7 - x)  # 0 at the outer edges, 3 at the crease
        c = [hx("#d2c5a2"), hx("#bcae88"), hx("#a39470"), hx("#8d7e5b")][d]
        if h2(x, y, 31) > 0.9:
            c = shade(c, 0.94)
        img.putpixel((x, y), c)
# two stitches a period, each leaning like a "(": 6 rows long, a gap of 2
for y0 in (0, 8):
    for i in range(6):
        y = y0 + i
        x = 3 if i in (0, 5) else 2 if i in (1, 4) else 2
        img.putpixel((x, y), RED)
        img.putpixel((x + 1, y), RED_D)
put("BINDING", img)

# a page corner turned up, and the arrow that turns it back
CURL_ROWS = [
    "............",
    "..........x.",
    ".........xx.",
    "........xxx.",
    ".......xxxx.",
    "......xxxxx.",
    ".....xxxxxx.",
    "....xxxxxxx.",
    "...xxxxxxxx.",
    "..xxxxxxxxx.",
    ".xxxxxxxxxxx",
    "............",
]
img = new("CURL")
for y, row in enumerate(CURL_ROWS):
    for x, ch in enumerate(row):
        if ch == "x":
            on_fold = (x + y) == 11 or (x + y) == 12
            img.putpixel((x, y), hx("#8f7a4a") if on_fold else hx("#f1e8cf") if (x + y) > 13 else hx("#dccfa8"))
put("CURL", img)

ARROW_ROWS = [
    "......x.........",
    ".....xxx........",
    "....xxxxxxxxxx..",
    "...xxxxxxxxxxxx.",
    "....xxx......xxx",
    ".....x........xx",
    "..............xx",
    "..............xx",
    ".............xxx",
    "...........xxxx.",
    "..xxxxxxxxxxx...",
    "................",
]
img = new("ARROW")
for y, row in enumerate(ARROW_ROWS):
    for x, ch in enumerate(row):
        if ch == "x":
            img.putpixel((x, y), (255, 255, 255, 255))
put("ARROW", img)

# ---- parchment pieces on the pages ----


def card(name, base, light, dark, outline, border=3):
    img = new(name)
    w, h = img.size
    for y in range(h):
        for x in range(w):
            c = base if h2(x % 8, y % 8, 17) > 0.12 else shade(base, 0.96)
            d = min(x, y, w - 1 - x, h - 1 - y)
            if d == 0:
                c = outline
            elif d == 1:
                c = light if min(x, y) < min(w - 1 - x, h - 1 - y) else dark
            img.putpixel((x, y), c)
    for (x, y) in ((0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)):
        img.putpixel((x, y), (0, 0, 0, 0))
    put(name, img)


card("ENTRY", hx("#d8c9a2"), hx("#efe3c1"), hx("#b9a67a"), hx("#8a6b3c"))
card("ENTRY_SEL", hx("#f2e3a8"), hx("#fff6cc"), hx("#d6b85a"), hx("#c08a1a"))
# the card of the picked recipe is sunk into the page: shadow on the top / left, lip on the bottom / right
card("CARD", hx("#c9b98f"), hx("#a5936a"), hx("#e9dcb8"), hx("#7b5e34"))

img = new("GEM")
for y in range(8):
    for x in range(8):
        dx, dy = x - 3.5, y - 3.5
        if abs(dx) + abs(dy) <= 4:
            c = (255, 255, 255, 255)
            if abs(dx) + abs(dy) > 3.2:
                c = (150, 150, 150, 255)
            elif dx < 0 and dy < 0:
                c = (255, 255, 255, 255)
            else:
                c = (205, 205, 205, 255)
            img.putpixel((x, y), c)
put("GEM", img)

# a title's rule: an ink line with a dot every 8 pixels
img = new("RULE")
for x in range(16):
    img.putpixel((x, 1), hx("#5e4425"))
    img.putpixel((x, 2), hx("#9b8456")) if x % 8 else img.putpixel((x, 2), hx("#5e4425"))
put("RULE", img)

open("mc-rs/assets/satchel.rgba", "wb").write(sheet.tobytes())
print("wrote mc-rs/assets/satchel.rgba")

if "--preview" in sys.argv:
    out = sys.argv[sys.argv.index("--preview") + 1]
    bg = Image.new("RGBA", (W, H), (70, 90, 70, 255))
    bg.alpha_composite(sheet)
    bg.resize((W * 4, H * 4), Image.NEAREST).save(out)
    print("wrote", out)
