#!/usr/bin/env python3
"""Python twin of the options list (`options.rs`: `frame`, `flow`, `which`; `ui.rs`: `wrap`, `fit`, `thumb`, `clip`), run with no toolchain.

It reads the REAL font widths (`render/font.rs`), the tip sentences (`TIPS`), the setting names (`Opt::name`, the pad's `KNOBS` /
`TOGGLES` / `ACTS`, `KEY_NAMES`) out of the Rust source, and checks what the Rust tests check and a few things they cannot see without a
screen: every name fits beside its control, every tip fits its pane, the parts of the screen do not touch, a jump reaches every section.

    python3 tools/optlist_proto.py        # prints "ok" or fails on the first broken claim
"""
import math
import re
from pathlib import Path

SRC = Path(__file__).resolve().parent.parent / "mc-rs" / "src"
read = lambda p: (SRC / p).read_text()

WIDTH = [int(x) for x in re.search(r"pub const WIDTH: \[u8; 95\] = \[([^\]]*)\]", read("render/font.rs")).group(1).split(",")]


def tw(s):
    return sum(WIDTH[(ord(c) if 32 <= ord(c) <= 126 else ord("?")) - 32] for c in s)


# ---- ui.rs ----
def wrap(text, w):
    lines, line = [], ""
    for word in text.split():
        joined = word if not line else f"{line} {word}"
        if not line or tw(joined) <= w:
            line = joined
        else:
            lines.append(line)
            line = word
    if line:
        lines.append(line)
    return lines


def fit(s, w):
    if tw(s) <= w:
        return s
    while s and tw(s + "..") > w:
        s = s[:-1]
    return s + ".."


def thumb(view, content, scroll, mx):
    th = math.floor(min(max(view * view / content, 32.0), view - 8.0))
    return min(max(scroll * (view - th) / mx, 0.0), view - th), th


def lerp(a, b, t):
    return [x + (y - x) * t for x, y in zip(a, b)]


def clip(quads, r):
    """quads: (x0, y0, x1, y1, uv0, uv1) with uv0 at the top-left corner, uv1 at the bottom-right; the visible part keeps its texture."""
    out = []
    for x0, y0, x1, y1, u0, u1 in quads:
        nx0, ny0, nx1, ny1 = max(x0, r[0]), max(y0, r[1]), min(x1, r[0] + r[2]), min(y1, r[1] + r[3])
        if nx0 >= nx1 or ny0 >= ny1:
            continue
        a, b, c, d = (nx0 - x0) / (x1 - x0), (nx1 - x0) / (x1 - x0), (ny0 - y0) / (y1 - y0), (ny1 - y0) / (y1 - y0)
        uv = lambda tx, ty: [u0[0] + (u1[0] - u0[0]) * tx, u0[1] + (u1[1] - u0[1]) * ty]
        out.append((nx0, ny0, nx1, ny1, uv(a, c), uv(b, d)))
    return out


# ---- options.rs ----
N_PAGES, PAGE_H, GROUP_H, ROW_H, ROW_PITCH, SECTION_GAP = 7, 18.0, 13.0, 20.0, 22.0, 6.0


def frame(gw, gh):
    wide = gw >= 520.0
    side_w, tip_w, gap = (100.0 if wide else 80.0), (170.0 if wide else 0.0), 6.0
    list_w = min(gw - side_w - tip_w - gap * (4.0 if wide else 3.0), 300.0)
    total = side_w + gap + list_w + (gap + tip_w if wide else 0.0)
    x0 = math.floor((gw - total) / 2.0)
    top, h = 30.0, gh - 30.0 - 38.0
    lst = [x0 + side_w + gap, top, list_w, h]
    tip = [lst[0] + list_w + gap, top, tip_w, h] if wide else [x0, gh - 37.0, total - 86.0, 35.0]
    return dict(side=[x0, top, side_w, h], list=lst, tip=tip, wide=wide, done=[x0 + total - 80.0, gh - 28.0, 80.0, 20.0],
                ctl=min(max(math.floor(list_w * 0.42), 80.0), 104.0))


def flow(items):
    y, at, starts = 0.0, [], [0.0] * N_PAGES
    for kind, arg in items:
        if kind == "page":
            y += SECTION_GAP if arg > 0 else 0.0
            starts[arg] = y
            at.append(y)
            y += PAGE_H + 3.0
        elif kind == "group":
            y += 2.0
            at.append(y)
            y += GROUP_H
        else:
            at.append(y)
            y += ROW_PITCH
    return at, starts, y + 4.0


def which(starts, scroll, view):
    probe = scroll + min(3.0 * ROW_PITCH, view / 2.0)
    return next((i for i in reversed(range(N_PAGES)) if starts[i] <= probe), 0)


def scaled_gs(w, h, gs=0):
    mx, s = (1000.0 if gs == 0 else float(gs)), 1.0
    while s < mx and math.floor(w / (s + 1)) >= 320 and math.floor(h / (s + 1)) >= 240:
        s += 1
    return s, math.ceil(w / s), math.ceil(h / s)


# ---- what the Rust source says ----
opt = read("options.rs")
names = dict(re.findall(r"Opt::(\w+) => \"([^\"]+)\",", opt))
key_names = re.search(r"const KEY_NAMES: \[&str; \d+\] = \[([^\]]*)\]", opt).group(1).replace('"', "").split(", ")
pad = read("input/gamepad.rs")
knobs = re.search(r"const KNOBS: \[&str; \d+\] = \[([^\]]*)\]", pad).group(1).replace('"', "").split(", ")
toggles = re.search(r"const TOGGLES: \[&str; \d+\] = \[([^\]]*)\]", pad).group(1).replace('"', "").split(", ")
acts = re.search(r"\[(\"Jump\", \"Sneak\"[^\]]*)\]\[self as usize\]", pad).group(1).replace('"', "").split(", ")
tips = dict(re.findall(r'^\s*\("([^"]+)", "([^"]+)"\),$', opt[opt.index("const TIPS"):], re.M))
assert len(tips) == int(re.search(r"const TIPS: \[\(&str, &str\); (\d+)\]", opt).group(1)), "TIPS length does not match its table"

ROWS = [  # the Opt variants in `items()`, per section, and the pad's names
    ("page", 0), *[("row", n) for n in ["Music", "Sound", "Difficulty", "GuiScale", "ViewBobbing"]],
    ("page", 1), ("group", "World"), ("row", "Graphics"), ("row", "RenderDistance"), ("row", "LoD"), ("group", "Lighting"), *[("row", n) for n in ["AmbientOcclusion", "AdvancedOpengl", "Anaglyph"]],
    ("page", 2), ("group", "Frame rate"), ("row", "FramerateLimit"), ("row", "DynamicRes"), ("group", "Next start"), *[("row", n) for n in ["Msaa", "Fxaa", "Post", "Renderer"]],
    ("page", 3), ("row", "Vibrance"), ("group", "Switches"), *[("row", n) for n in ["FxWarm", "FxWater", "FxGlow", "FxMotion", "FxHaze"]],
    ("page", 4), ("group", "Look and dig"), *[("row", n) for n in ["Sensitivity", "InvertMouse", "DigDelay"]], ("group", "Keyboard"), *[("key", n) for n in key_names],
    ("page", 5), *[("pad", n) for n in acts],
    ("page", 6), *[("pad", n) for n in knobs], *[("pad", n) for n in toggles],
]
# the rows' names as drawn on the left of a row
shown = [(k, "LoD" if n == "LoD" else names[n] if k == "row" else n) for k, n in ROWS if k not in ("page", "group")]
items = [(k, n) if k in ("page", "group") else ("row", n) for k, n in ROWS]
ROWS_ITEMS = [(k, a) if k in ('page', 'group') else ('row', (k if k != 'row' else 'row', ('LoD' if a == 'LoD' else names[a]) if k == 'row' else a)) for k, a in ROWS]
unused = {"Graphics", "Smooth Lighting", "Advanced OpenGL", "3D Anaglyph"}


def main():
    # 1. every name that is not dim has a sentence, and every sentence is for a name that exists
    drawn = {n for k, n in shown if k in ("row", "pad")}
    missing = sorted(n for n in drawn - unused if n not in tips and n not in acts)
    assert not missing, f"no tip for {missing}"
    extra = sorted(set(tips) - drawn)
    assert not extra, f"a tip for a name that is no row: {extra}"

    for w, h in [(320, 240), (2400, 1080), (1280, 720), (1920, 1080), (800, 480), (960, 540)]:
        s, gw, gh = scaled_gs(w, h)
        f = frame(gw, gh)
        # 2. the parts lie inside the screen and do not touch
        boxes = {"side": [f["side"][0], f["side"][1], f["side"][2], 7 * 20.0], "list": f["list"], "tip": f["tip"], "done": f["done"]}
        keys = list(boxes)
        for i, a in enumerate(keys):
            x, y, bw, bh = boxes[a]
            assert x >= 0 and y >= 0 and x + bw <= gw and y + bh <= gh, f"{a} off screen at {gw}x{gh}: {boxes[a]}"
            for b in keys[i + 1:]:
                p, q = boxes[a], boxes[b]
                assert p[0] + p[2] <= q[0] or q[0] + q[2] <= p[0] or p[1] + p[3] <= q[1] or q[1] + q[3] <= p[1], f"{a} touches {b} at {gw}x{gh}"
        assert f["side"][3] >= 7 * 20.0, f"the page list does not fit at {gw}x{gh}"
        # 3. every name fits beside its control, a tip fits the pane
        room = f["list"][2] - 11.0 - f["ctl"] - 14.0
        for k, n in shown:
            assert tw(n) <= room, f"'{n}' is {tw(n)} wide, {room} fits at {gw}x{gh}"
        for n, t in tips.items():
            lines = wrap(t, f["tip"][2] - 12.0)
            cap = 8 if f["wide"] else 2
            assert len(lines) <= cap, f"tip '{n}': {len(lines)} lines in {f['tip'][2] - 12}: {lines}"
        # 4. the list scrolls, a jump reaches every section, and the lit page follows
        at, starts, total = flow(items)
        view = f["list"][3]
        mx = max(total - view, 0.0)
        assert mx > 0, "the list should need scrolling at every size"
        assert all(a < b for a, b in zip(starts, starts[1:]))
        for i in range(N_PAGES):
            assert which(starts, min(starts[i], mx), view) == i or i == N_PAGES - 1, f"jump to {i} lights another page at {gw}x{gh}"
            assert which(starts, starts[i], view) == i
        assert which(starts, 0.0, view) == 0 and which(starts, mx, view) == N_PAGES - 1
        last = N_PAGES - 1
        assert starts[last] < mx, "the last section starts above the bottom, so a tap on it is not clamped"
        # 5. rows are inside the list's content and 2 units apart (a row is 20, the pitch 22)
        rows = [y for (kind, _), y in zip(items, at) if kind == "row"]
        assert all(b - a >= ROW_PITCH - 1e-6 for a, b in zip(rows, rows[1:])), "rows overlap"
        assert thumb(view, total, 0.0, mx)[0] == 0.0 and sum(thumb(view, total, mx, mx)) <= view + 1e-6

    # 6. wrap / fit
    t = "Draws far terrain as simplified blocks, out to 768 blocks."
    assert " ".join(wrap(t, 100.0)) == t and all(tw(l) <= 100.0 for l in wrap(t, 100.0))
    assert tw(fit("A very long world name indeed", 60.0)) <= 60.0

    # 7. clip: a quad over the left and right edges keeps the middle of its texture
    q = clip([(-10.0, 0.0, 30.0, 10.0, [0.0, 0.0], [1.0, 1.0])], [0.0, 0.0, 30.0, 30.0])[0]
    assert (q[0], q[2]) == (0.0, 30.0) and abs(q[4][0] - 0.25) < 1e-9 and q[5][0] == 1.0
    assert clip([(100.0, 0.0, 110.0, 10.0, [0, 0], [1, 1])], [0.0, 0.0, 30.0, 30.0]) == []
    print("ok:", len(shown), "rows,", len(tips), "tips,", N_PAGES, "pages; widest name", max(tw(n) for _, n in shown), "units")


if __name__ == "__main__":
    main()
