#!/usr/bin/env python3
"""Writes mc-rs/assets/satchel.rgba: pixel art of the satchel and its recipe book (`satchel.rs`), 256 x 256 RGBA, the last block of the
HUD texture (`hud::SATCHEL_Y`). Everything is drawn from code. Style (2026-10-26 restyle, Minecraft GUI-pack look): light-grey bevelled
panels with per-pixel stone noise (3-4 tones), a dark outline, small pixel corner ornaments and a red stitched seam; vanilla b1.7.3 inset
slots; a parchment book with a brown frame and a red stitched binding; the closed book is a parchment scroll with a red cord.
Sprite names, sizes and places are the old `SPRITES` table, so `hud::sprites::sat` needs no change.
  python3 tools/gen_satchel_ui.py --preview DIR    writes DIR/satchel_sheet.png (4x), DIR/mock_open.png, DIR/mock_closed.png (3x); NO asset
  python3 tools/gen_satchel_ui.py --write          writes mc-rs/assets/satchel.rgba (run `cargo test --lib hud satchel` after)
Needs Pillow. The mock screens use the real b1.7.3 font (`render/font.rs`) at integer scale and NO text shadow."""
import re
import sys
from PIL import Image

W = H = 256
sheet = Image.new("RGBA", (W, H), (0, 0, 0, 0))
SPRITES = {
    "BAG": (0, 0, 32, 32), "COVER": (32, 0, 32, 32), "PARCH": (64, 0, 32, 32), "PAGE_EDGE": (96, 0, 16, 16),
    "BINDING": (112, 0, 8, 16), "CURL": (120, 0, 12, 12), "ARROW": (132, 0, 16, 12),
    "POCKET": (0, 32, 16, 16), "BELT": (16, 32, 16, 16), "BUCKLE": (32, 32, 12, 16), "CLASP": (44, 32, 12, 20), "STRAP": (56, 32, 8, 16),
    "SLOT": (64, 32, 18, 18), "RIM": (82, 32, 18, 18), "BTN": (100, 32, 16, 16), "BTN_ON": (116, 32, 16, 16), "TAB": (132, 32, 16, 16),
    "ENTRY": (148, 32, 16, 16), "ENTRY_SEL": (164, 32, 16, 16), "CARD": (180, 32, 16, 16), "GEM": (196, 32, 8, 8), "RULE": (204, 32, 16, 4)
}
CLEAR = (0, 0, 0, 0)


def hx(s):
    return tuple(int(s[i:i + 2], 16) for i in (1, 3, 5)) + (255,)


def h2(x, y, seed=0):
    n = (x * 374761393 + y * 668265263 + seed * 2147483647) & 0xFFFFFFFF
    n = ((n ^ (n >> 13)) * 1274126177) & 0xFFFFFFFF
    return ((n ^ (n >> 16)) & 0xFFFF) / 65535.0


def new(name):
    return Image.new("RGBA", SPRITES[name][2:], CLEAR)


def put(name, img):
    x, y, w, h = SPRITES[name]
    assert img.size == (w, h), name
    sheet.paste(img, (x, y))


def shade(c, f):
    return tuple(max(0, min(255, int(v * f))) for v in c[:3]) + (255,)


def pick(tones, r, wts=(0.5, 0.2, 0.2, 0.1)):
    t = 0.0
    for c, wt in zip(tones, wts):
        t += wt
        if r < t:
            return c
    return tones[-1]


def noise_fill(img, tones, seed, period=16, wts=(0.5, 0.2, 0.2, 0.1)):
    """Per-pixel noise in 3-4 tones, periodic so a stretched 9-slice edge tiles without a seam."""
    w, h = img.size
    for y in range(h):
        for x in range(w):
            img.putpixel((x, y), pick(tones, h2(x % period, y % period, seed), wts))


RED, RED_D = hx("#d8433f"), hx("#8e2623")
INK = hx("#2b1b0c")


def corner_mark(img, base, sx, sy, main, hi):
    """A small pixel bracket + dot, like the corner marks of the GUI pack's panels."""
    w, h = img.size
    ox, oy = (0 if sx > 0 else w - 1), (0 if sy > 0 else h - 1)
    pts = [(3, 3), (4, 3), (5, 3), (6, 3), (3, 4), (3, 5), (3, 6), (5, 5)]
    for dx, dy in pts:
        img.putpixel((ox + sx * (dx + 1), oy + sy * (dy + 1)), hi)
    for dx, dy in pts:
        img.putpixel((ox + sx * dx, oy + sy * dy), main)


def panel(name, tones, outline, hi, lo, mark, seam=True, seed=3, wts=(0.5, 0.2, 0.2, 0.1)):
    """A 9-slice (border 8): noise, dark outline, lit top-left / shadowed bottom-right edge, corner marks, a red stitched seam."""
    img = new(name)
    w, h = img.size
    noise_fill(img, tones, seed, 16, wts)
    for y in range(h):
        for x in range(w):
            d = min(x, y, w - 1 - x, h - 1 - y)
            if d == 0:
                img.putpixel((x, y), outline)
            elif d == 1:
                img.putpixel((x, y), hi if min(x, y) <= min(w - 1 - x, h - 1 - y) else lo)
    if seam:
        for i in range(8, w - 8):
            if (i // 2) % 2 == 0:
                img.putpixel((i, 4), RED)
                img.putpixel((i, 5), RED_D)
                img.putpixel((i, h - 5), RED)
                img.putpixel((i, h - 6), RED_D)
        for j in range(8, h - 8):
            if (j // 2) % 2 == 0:
                img.putpixel((4, j), RED)
                img.putpixel((5, j), RED_D)
                img.putpixel((w - 5, j), RED)
                img.putpixel((w - 6, j), RED_D)
    for sx, sy in ((1, 1), (-1, 1), (1, -1), (-1, -1)):
        corner_mark(img, None, sx, sy, mark, hi)
    img.putpixel((0, 0), CLEAR)
    img.putpixel((w - 1, 0), CLEAR)
    img.putpixel((0, h - 1), CLEAR)
    img.putpixel((w - 1, h - 1), CLEAR)
    put(name, img)
    return img


# ---- the bag: a light-grey stone panel ----
GREY = [hx("#c6c6c6"), hx("#bbbbc0"), hx("#d0d0d2"), hx("#aeaeb6")]
panel("BAG", GREY, hx("#14141c"), hx("#fbfbfb"), hx("#6a6a78"), hx("#4a5470"))


def pocket():
    """A darker stone band behind a row: a lit lip on top, red stitches along the foot."""
    img = new("POCKET")
    noise_fill(img, [hx("#9a9aa2"), hx("#8e8e98"), hx("#a6a6ae"), hx("#82828c")], 5)
    w, h = img.size
    for x in range(w):
        img.putpixel((x, 0), hx("#5a5a66"))
        img.putpixel((x, 1), hx("#e4e4e8"))
        img.putpixel((x, h - 1), hx("#6a6a78"))
        if (x // 2) % 2 == 0:
            img.putpixel((x, h - 4), RED)
    put("POCKET", img)


pocket()


def belt():
    """The quick belt's band: darker slate, two rows of red stitches."""
    img = new("BELT")
    noise_fill(img, [hx("#60626f"), hx("#555763"), hx("#6a6c7a"), hx("#4a4c58")], 6)
    w, h = img.size
    for x in range(w):
        img.putpixel((x, 0), hx("#2c2e38"))
        img.putpixel((x, h - 1), hx("#2c2e38"))
        if (x // 2) % 2 == 0:
            img.putpixel((x, 3), RED)
            img.putpixel((x, h - 4), RED)
    put("BELT", img)


belt()


def small_plate(name, fill, edge, mark):
    """BUCKLE / CLASP: a small plate with a corner-mark, a hole for the cord."""
    img = new(name)
    w, h = img.size
    noise_fill(img, fill, 8, 8)
    for y in range(h):
        for x in range(w):
            d = min(x, y, w - 1 - x, h - 1 - y)
            if d == 0:
                img.putpixel((x, y), edge)
            elif d == 1:
                img.putpixel((x, y), hx("#f4f4f6") if min(x, y) <= min(w - 1 - x, h - 1 - y) else hx("#5e5e6c"))
    for x, y in ((0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)):
        img.putpixel((x, y), CLEAR)
    for dx, dy in ((3, 3), (4, 3), (3, 4)):
        img.putpixel((dx, dy), mark)
        img.putpixel((w - 1 - dx, h - 1 - dy), mark)
    put(name, img)


small_plate("BUCKLE", GREY, hx("#14141c"), hx("#4a5470"))
small_plate("CLASP", [hx("#8a2f2c"), hx("#7a2623"), hx("#9a3835"), hx("#6a1f1c")], hx("#2a0c0b"), hx("#f0c8c4"))

# the cord of the closed scroll: a red band with darker stitched edges
img = new("STRAP")
for y in range(16):
    for x in range(8):
        c = pick([hx("#c23c38"), hx("#b23430"), hx("#d24a45"), hx("#9c2a27")], h2(x, y, 3))
        if x in (0, 7):
            c = hx("#561513")
        elif x in (1, 6):
            c = hx("#e66a64") if x == 1 else hx("#7a1f1c")
        img.putpixel((x, y), c)
put("STRAP", img)

# ---- slots (vanilla b1.7.3 inset), the rim shown only on a touch ----
img = new("SLOT")
for y in range(18):
    for x in range(18):
        c = pick([hx("#8b8b8b"), hx("#848484"), hx("#929292"), hx("#7d7d7d")], h2(x, y, 9), (0.62, 0.15, 0.15, 0.08))
        if x == 0 or y == 0:
            c = hx("#373737")
        elif x == 17 or y == 17:
            c = hx("#ffffff")
        elif x == 1 or y == 1:
            c = shade(c, 0.88)
        img.putpixel((x, y), c)
img.putpixel((17, 0), hx("#8b8b8b"))
img.putpixel((0, 17), hx("#8b8b8b"))
put("SLOT", img)

img = new("RIM")
for i in range(18):
    for p in ((i, 0), (i, 17), (0, i), (17, i), (i, 1), (i, 16), (1, i), (16, i)):
        img.putpixel(p, (255, 255, 255, 255))
put("RIM", img)


def plate(name, tones, outline, hi, lo):
    img = new(name)
    w, h = img.size
    noise_fill(img, tones, 5, 8, (0.6, 0.2, 0.2, 0.0))
    for y in range(h):
        for x in range(w):
            d = min(x, y, w - 1 - x, h - 1 - y)
            if d == 0:
                img.putpixel((x, y), outline)
            elif d == 1:
                img.putpixel((x, y), hi if min(x, y) <= min(w - 1 - x, h - 1 - y) else lo)
    for x, y in ((0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)):
        img.putpixel((x, y), CLEAR)
    put(name, img)


plate("BTN", [hx("#707070"), hx("#686868"), hx("#787878"), hx("#606060")], hx("#101010"), hx("#b4b4b4"), hx("#383838"))
plate("BTN_ON", [hx("#7a82c8"), hx("#7078bc"), hx("#868ed2"), hx("#666eb0")], hx("#101018"), hx("#c8ceff"), hx("#3a4078"))
plate("TAB", [hx("#b6c2d6"), hx("#aab6cc"), hx("#c2ccdc"), hx("#9eaac0")], hx("#1c2438"), hx("#ffffff"), hx("#6a7690"))

# ---- parchment ----


def parch_tile():
    """Seamless parchment, 4 tones, short horizontal fibres, a few specks."""
    img = new("PARCH")
    tones = [hx("#d9c99c"), hx("#cdbb8c"), hx("#e3d5ab"), hx("#bfae80")]
    for y in range(32):
        for x in range(32):
            r = 0.55 * h2(x // 2 % 16, y, 11) + 0.45 * h2(x, y, 12)
            c = pick(tones, r, (0.46, 0.24, 0.2, 0.1))
            if h2(x, y, 13) > 0.985:
                c = hx("#efe4c0")
            elif h2(x, y, 14) < 0.01:
                c = hx("#a8976a")
            img.putpixel((x, y), c)
    put("PARCH", img)


parch_tile()

# the book's frame: dark outline, brown leather with grain, a dark inner shadow, a light tan lip; the inside is clear
img = new("PAGE_EDGE")
LEATHER = [hx("#8a5a2e"), hx("#7c4f27"), hx("#97642f"), hx("#6c4420")]
for y in range(16):
    for x in range(16):
        d = min(x, y, 15 - x, 15 - y)
        c = None
        if d == 0:
            c = hx("#24140a")
        elif d in (1, 2, 3):
            c = pick(LEATHER, h2(x % 8, y % 8, 21))
            if d == 1:
                c = shade(c, 1.25) if min(x, y) < min(15 - x, 15 - y) else shade(c, 0.8)
        elif d == 4:
            c = hx("#4e3016")
        elif d == 5:
            c = hx("#e8d8a4")
        img.putpixel((x, y), c if c else CLEAR)
for x, y in ((0, 0), (15, 0), (0, 15), (15, 15)):
    img.putpixel((x, y), CLEAR)
put("PAGE_EDGE", img)

# the binding: both pages sink into the crease, red stitches shaped like "("
img = new("BINDING")
for y in range(16):
    for x in range(8):
        d = min(x, 7 - x)
        c = [hx("#d6c79c"), hx("#bfae82"), hx("#a39468"), hx("#8a7a52")][d]
        if h2(x, y, 31) > 0.9:
            c = shade(c, 0.93)
        img.putpixel((x, y), c)
for y0 in (0, 8):
    for i in range(6):
        x = 3 if i in (0, 5) else 2
        img.putpixel((x, y0 + i), hx("#ec5a56"))
        img.putpixel((x + 1, y0 + i), RED_D)
put("BINDING", img)

CURL_ROWS = ["............", "..........x.", ".........xx.", "........xxx.", ".......xxxx.", "......xxxxx.", ".....xxxxxx.",
             "....xxxxxxx.", "...xxxxxxxx.", "..xxxxxxxxx.", ".xxxxxxxxxxx", "............"]
img = new("CURL")
for y, row in enumerate(CURL_ROWS):
    for x, ch in enumerate(row):
        if ch == "x":
            fold = (x + y) in (11, 12)
            img.putpixel((x, y), hx("#7a6638") if fold else hx("#f1e8cf") if (x + y) > 13 else hx("#d6c79c"))
put("CURL", img)

ARROW_ROWS = ["......x.........", ".....xxx........", "....xxxxxxxxxx..", "...xxxxxxxxxxxx.", "....xxx......xxx", ".....x........xx",
              "..............xx", "..............xx", ".............xxx", "...........xxxx.", "..xxxxxxxxxxx...", "................"]
img = new("ARROW")
for y, row in enumerate(ARROW_ROWS):
    for x, ch in enumerate(row):
        if ch == "x":
            img.putpixel((x, y), (255, 255, 255, 255))
put("ARROW", img)


def card(name, tones, outline, light, dark):
    img = new(name)
    w, h = img.size
    noise_fill(img, tones, 17, 8, (0.55, 0.25, 0.2, 0.0))
    for y in range(h):
        for x in range(w):
            d = min(x, y, w - 1 - x, h - 1 - y)
            if d == 0:
                img.putpixel((x, y), outline)
            elif d == 1:
                img.putpixel((x, y), light if min(x, y) < min(w - 1 - x, h - 1 - y) else dark)
    for x, y in ((0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)):
        img.putpixel((x, y), CLEAR)
    put(name, img)


card("ENTRY", [hx("#d8c9a2"), hx("#cfbf94"), hx("#e0d3ae"), hx("#c4b488")], hx("#6a4e26"), hx("#f2e8c8"), hx("#b09c6c"))
card("ENTRY_SEL", [hx("#f2e3a8"), hx("#e8d894"), hx("#f8ecbc"), hx("#dccc82")], hx("#b07a14"), hx("#fff8d0"), hx("#cca642"))
card("CARD", [hx("#c4b488"), hx("#baa97c"), hx("#cdbf94"), hx("#b09e72")], hx("#5e4422"), hx("#9c8a5c"), hx("#e6dab4"))

img = new("GEM")
for y in range(8):
    for x in range(8):
        dx, dy = x - 3.5, y - 3.5
        if abs(dx) + abs(dy) <= 4:
            c = (150, 150, 150, 255) if abs(dx) + abs(dy) > 3.2 else (255, 255, 255, 255) if dx < 0 and dy < 0 else (205, 205, 205, 255)
            img.putpixel((x, y), c)
put("GEM", img)

img = new("RULE")
for x in range(16):
    img.putpixel((x, 1), hx("#5e4425"))
    img.putpixel((x, 2), hx("#5e4425") if x % 8 == 0 else hx("#9b8456"))
put("RULE", img)


def book_cover():
    """COVER (9-slice, border 8): a plain brown book. The spine is the left border: darker leather with a crease and a column of red
    stitches. A sliver of cream page edge shows on the right and the foot. No clasp, no strap."""
    img = new("COVER")
    brown = [hx("#7a4a26"), hx("#6e4222"), hx("#855330"), hx("#623a1c")]
    spine = [hx("#4a2a14"), hx("#42250f"), hx("#523018"), hx("#3a200d")]
    for y in range(32):
        for x in range(32):
            c = pick(spine if x < 8 else brown, h2(x % 16, y % 16, 7), (0.5, 0.2, 0.2, 0.1))
            d = min(x, y, 31 - x, 31 - y)
            if d == 0:
                c = hx("#24140a")
            elif d == 1:
                c = hx("#a8703c") if min(x, y) <= min(31 - x, 31 - y) else hx("#3e2410")
            elif (x == 29 and 2 <= y <= 29) or (y == 29 and 8 <= x <= 29):
                c = hx("#e8d8a4") if h2(x, y, 9) > 0.2 else hx("#cdbd88")  # page edge
            if x == 6 and 2 <= y <= 29:
                c = hx("#1e0f06")  # crease
            elif x == 7 and 2 <= y <= 29:
                c = hx("#8a5a30")
            if x in (3, 4) and 8 <= y < 24 and (y // 2) % 2 == 0:
                c = RED if x == 3 else RED_D  # the stitches of the spine, periodic in y so the edge tiles
            img.putpixel((x, y), c)
    for x, y in ((0, 0), (31, 0), (0, 31), (31, 31)):
        img.putpixel((x, y), CLEAR)
    put("COVER", img)


book_cover()

# ======================= preview =======================


def load_font():
    src = open("mc-rs/src/render/font.rs").read()
    rows = [[int(v) for v in m.split(",")] for m in re.findall(r"\[([0-9, ]+)\]", src.split("pub const WIDTH")[0])]
    wid = [int(v) for v in re.findall(r"\d+", src.split("pub const WIDTH")[1].split("=")[1])]
    assert len(rows) == 95 and len(wid) == 95
    return rows, wid


GLYPHS, WIDTH = load_font()


def tw(s):
    return sum(WIDTH[ord(c) - 32] for c in s)


def text(dst, x, y, s, col):
    for ch in s:
        g = ord(ch) - 32
        for r, bits in enumerate(GLYPHS[g]):
            for b in range(8):
                if bits >> b & 1:
                    dst.putpixel((x + b, y + r), col)
        x += WIDTH[g]


def wrap(s, maxw):
    lines, cur = [], ""
    for word in s.split():
        t = (cur + " " + word).strip()
        if tw(t) <= maxw:
            cur = t
        else:
            lines.append(cur)
            cur = word
    return lines + [cur]


def spr(name):
    x, y, w, h = SPRITES[name]
    return sheet.crop((x, y, x + w, y + h))


def nine(dst, name, x, y, w, h, b):
    s = spr(name)
    sw, sh = s.size

    def tile(box, dx, dy, dw, dh):
        t = s.crop(box)
        for yy in range(0, dh, t.size[1]):
            for xx in range(0, dw, t.size[0]):
                dst.alpha_composite(t.crop((0, 0, min(t.size[0], dw - xx), min(t.size[1], dh - yy))), (dx + xx, dy + yy))
    tile((0, 0, b, b), x, y, b, b)
    tile((sw - b, 0, sw, b), x + w - b, y, b, b)
    tile((0, sh - b, b, sh), x, y + h - b, b, b)
    tile((sw - b, sh - b, sw, sh), x + w - b, y + h - b, b, b)
    tile((b, 0, sw - b, b), x + b, y, w - 2 * b, b)
    tile((b, sh - b, sw - b, sh), x + b, y + h - b, w - 2 * b, b)
    tile((0, b, b, sh - b), x, y + b, b, h - 2 * b)
    tile((sw - b, b, sw, sh - b), x + w - b, y + b, b, h - 2 * b)
    tile((b, b, sw - b, sh - b), x + b, y + b, w - 2 * b, h - 2 * b)


def tile_rect(dst, name, x, y, w, h):
    t = spr(name)
    for yy in range(0, h, t.size[1]):
        for xx in range(0, w, t.size[0]):
            dst.alpha_composite(t.crop((0, 0, min(t.size[0], w - xx), min(t.size[1], h - yy))), (x + xx, y + yy))


def tint(img, col):
    px = img.load()
    for y in range(img.size[1]):
        for x in range(img.size[0]):
            r, g, b, a = px[x, y]
            px[x, y] = (r * col[0] // 255, g * col[1] // 255, b * col[2] // 255, a)
    return img


def item(dst, x, y, col):
    for yy in range(12):
        for xx in range(12):
            c = shade(col, 1.3 if (xx + yy) < 6 else 0.7 if (xx + yy) > 16 else 1.0)
            if xx in (0, 11) or yy in (0, 11):
                c = shade(col, 0.4)
            dst.putpixel((x + 3 + xx, y + 3 + yy), c)


ITEMS = [(0, 0, hx("#8a5a2e")), (1, 0, hx("#3a8a32")), (2, 0, hx("#e08a8a")), (3, 0, hx("#2c2c2c")), (4, 0, hx("#c8a060")), (6, 0, hx("#b87a30"))]


def mock(open_book):
    cv = Image.new("RGBA", (400, 188), CLEAR)
    for y in range(188):
        for x in range(400):
            cv.putpixel((x, y), pick([hx("#4a3426"), hx("#3e2c20"), hx("#54402e"), hx("#33261b")], h2(x // 2, y // 2, 2)))
    cv.alpha_composite(Image.new("RGBA", (400, 188), (0, 0, 0, 90)))
    bx, by, bw, bh = 6, 20, 186, 130
    nine(cv, "BAG", bx, by, bw, bh, 8)
    nine(cv, "TAB", 66, 11, 66, 15, 4)
    text(cv, 66 + (66 - tw("Inventory")) // 2, 14, "Inventory", hx("#1c2438"))
    gx, gy = bx + 16, by + 34
    for i in range(4):
        cv.alpha_composite(spr("SLOT"), (gx + i * 17, by + 12))
    text(cv, gx + 4 * 17 + 6, by + 16, "Armor", hx("#3a3a4a"))
    tile_rect(cv, "POCKET", gx - 3, gy - 3, 9 * 17 + 7, 3 * 17 + 7)
    for r in range(3):
        for c in range(9):
            cv.alpha_composite(spr("SLOT"), (gx + c * 17, gy + r * 17))
    tile_rect(cv, "BELT", gx - 3, gy + 3 * 17 + 6, 9 * 17 + 7, 24)
    for c in range(9):
        cv.alpha_composite(spr("SLOT"), (gx + c * 17, gy + 3 * 17 + 9))
    for c, r, col in ITEMS:
        item(cv, gx + c * 17, gy + r * 17 if r else gy + 3 * 17 + 9, col)
    cv.alpha_composite(tint(spr("RIM"), (90, 160, 255)), (gx + 2 * 17, gy + 3 * 17 + 9))  # the rim, only on a touch
    nine(cv, "BTN", 6, 3, 30, 14, 3)
    text(cv, 6 + (30 - tw("Sort")) // 2, 6, "Sort", hx("#f0f0f0"))
    nine(cv, "BTN", 372, 3, 20, 14, 3)
    text(cv, 372 + (20 - tw("X")) // 2, 6, "X", hx("#f0f0f0"))
    if open_book:
        L, T, R, B = 198, 20, 396, 182
        tile_rect(cv, "PARCH", L + 4, T + 4, R - L - 8, B - T - 8)
        nine(cv, "PAGE_EDGE", L, T, R - L, B - T, 6)
        sp = 297
        tile_rect(cv, "BINDING", sp, T + 6, 8, B - T - 12)
        text(cv, L + 12, T + 12, "Recipes", INK)
        cats = [("Tools & gear", "50"), ("Food", "6"), ("Blocks", "43"), ("Redstone & rails", "15"), ("Furnace", "10")]
        for i, (n, k) in enumerate(cats):
            ey = T + 26 + i * 25
            nine(cv, "ENTRY_SEL" if i == 0 else "ENTRY", L + 10, ey, 84, 22, 3)
            lab = n
            while tw(lab) > 84 - 8 - tw(k) - 8:  # shrink-to-fit stand-in: cut with a dot
                lab = lab[:-2] + "."
            text(cv, L + 16, ey + 7, lab, INK)
            text(cv, L + 10 + 84 - 6 - tw(k), ey + 7, k, hx("#5e4425"))
        rx = sp + 8 + 6  # left edge of the right page's content
        right = R - 6 - 4  # the page's content ends here (frame 6 + a margin 4)
        text(cv, rx, T + 12, "Crafting", INK)
        for r in range(2):
            for c in range(2):
                cv.alpha_composite(spr("SLOT"), (rx + c * 17, T + 28 + r * 17))
        ax = rx + 2 * 17 + 2
        cv.alpha_composite(tint(spr("ARROW"), (60, 40, 20)), (ax, T + 40))
        res = right - 18
        assert ax + 16 <= res and res + 18 <= right, (ax, res, right)
        cv.alpha_composite(spr("SLOT"), (res, T + 36))
        nine(cv, "CARD", rx, T + 70, right - rx, 70, 3)
        for i, ln in enumerate(wrap("Pick a recipe on the left page: its items go into the grid.", right - rx - 8)):
            text(cv, rx + 4, T + 76 + i * 10, ln, INK)
        cv.alpha_composite(spr("CURL"), (R - 18, B - 18))
    else:
        sx, sy, sw, sh = 232, 36, 112, 112
        nine(cv, "COVER", sx, sy, sw, sh, 8)
        tcol = hx("#e8d8a4")
        text(cv, sx + 12 + (sw - 12 - tw("Recipes")) // 2, sy + sh // 2 - 12, "Recipes", tcol)
        text(cv, sx + 12 + (sw - 12 - tw("Tap to open")) // 2, sy + sh // 2 + 4, "Tap to open", shade(tcol, 0.75))
    return cv


if "--preview" in sys.argv:
    out = sys.argv[sys.argv.index("--preview") + 1]
    bg = Image.new("RGBA", (W, H), (70, 90, 70, 255))
    bg.alpha_composite(sheet)
    bg.resize((W * 4, H * 4), Image.NEAREST).save(out + "/satchel_sheet.png")
    mock(True).resize((1200, 564), Image.NEAREST).save(out + "/mock_open.png")
    mock(False).resize((1200, 564), Image.NEAREST).save(out + "/mock_closed.png")
    print("wrote previews to", out)
if "--write" in sys.argv:
    open("mc-rs/assets/satchel.rgba", "wb").write(sheet.tobytes())
    print("wrote mc-rs/assets/satchel.rgba")
