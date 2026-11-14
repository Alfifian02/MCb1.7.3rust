#!/usr/bin/env python3
"""Acidul's Wolves (Bedrock port, `aciduls_wolves-v2_3.zip`) -> `mc-rs/assets/wolf.rgba` + `mc-rs/src/render/wolf_model.rs`.

NOT b1.7.3. Usage: `python3 tools/gen_wolf.py PACK.zip [--write] [--preview DIR]` (without --write it only prints the box bounds).

Textures: six variants x (wild, tame, angry), 64 x 48 each, four to a row (`wolf.rgba` is 256 x 240; `atlas::WOLF_Y`).
Model: `models/entity/wolf.geo.json` (the one geometry of the six variants), flattened to a list of boxes in b1.7.3 model space
(x and y mirrored: x_j = -x_b, y_j = 24 - y_b; the Bedrock rotation numbers equal `ModelRenderer.rotateAngle*` in degrees, which
`ModelBase`'s order z, y, x already is). Each box is the cube seen through its chain of bones (rotation about the bone's pivot, then
the bone's `position`), once for the standing pose and once for `animation.wolf.sitting`, as
(min, size, rotation point, rotation, swing phase + amplitude, uv, inflate); the rotation point is the root bone's pivot, so the head
turns, the legs swing and the tail wags about the right place. The head's boxes come first, the tail's (`TAIL_N`) last.
"""
import io, json, math, sys, zipfile
import numpy as np
from PIL import Image

VARIANTS = ["", "ashen", "black", "chestnut", "snowy", "woods"]  # "" = the pale default; the biome picks one (`wolf_look`)
STATES = ["", "tame", "angry"]
CELL = (64, 48)
SHEET = (256, 48 * 5)
GROUPS = ["head", "body", "leg0", "leg1", "leg2", "leg3", "tail"]
SWING = {"leg0": (0.0, 1.4), "leg1": (math.pi, 1.4), "leg2": (math.pi, 1.4), "leg3": (0.0, 1.4)}  # animation.wolf.leg_default
# animation.wolf.sitting, in the pack's own numbers. A root bone's `position` there is where its rotation point goes (x and y negated: it
# equals `ModelWolf`'s sitting rotation points, body y 18, back legs 22, front legs 17, tail 21); `mybody2`'s is a plain nudge.
SIT = {
    "body": ((-45, 0, 0), (0, -18, 0), True),
    "leg0": ((270, 0, 0), (-1.5, -22, 2), True),
    "leg1": ((270, 0, 0), (1.5, -22, 2), True),
    "leg2": ((333, 0, 0), (-1.49, -17, -4), True),
    "leg3": ((333, 0, 0), (1.51, -17, -4), True),
    "tail": ((0, 0, 0), (0, -21, 6), True),
    "mybody2": ((0, 0, 0), (0, 0, -1), False),
}


def rot(r):
    """Rz . Ry . Rx of degrees (the Java order, `Mat4::from_rotation_z * y * x`)."""
    x, y, z = (math.radians(a) for a in r)
    cx, sx, cy, sy, cz, sz = math.cos(x), math.sin(x), math.cos(y), math.sin(y), math.cos(z), math.sin(z)
    rx = np.array([[1, 0, 0], [0, cx, -sx], [0, sx, cx]])
    ry = np.array([[cy, 0, sy], [0, 1, 0], [-sy, 0, cy]])
    rz = np.array([[cz, -sz, 0], [sz, cz, 0], [0, 0, 1]])
    return rz @ ry @ rx


def euler(a):
    """The (x, y, z) radians with `a = Rz Ry Rx`."""
    ry = -math.asin(max(-1.0, min(1.0, a[2][0])))
    if abs(a[2][0]) > 0.99999:
        raise ValueError("gimbal")
    return math.atan2(a[2][1], a[2][2]), ry, math.atan2(a[1][0], a[0][0])


def j(p):
    """Bedrock point (y up, x mirrored) -> model space of `ModelBase`."""
    return np.array([-p[0], 24.0 - p[1], p[2]])


def bones_of(geo):
    return {b["name"]: b for b in geo["bones"]}


def chain(bones, name):
    out = []
    while name:
        out.append(name)
        name = bones[name].get("parent")
    return out  # leaf first


def root_of(bones, name):
    return chain(bones, name)[-1]


def world(bones, name, pose):
    """The transform (A, t) of bone `name`'s space into model space: x -> A x + t. Each bone turns about its pivot (its rest rotation
    plus the pose's), then moves by the pose's `position`; the parents' transforms come on top."""
    a, t = np.eye(3), np.zeros(3)
    for n in reversed(chain(bones, name)):  # root first
        b = bones[n]
        r, pos, absolute = pose.get(n, ((0, 0, 0), (0, 0, 0), False))
        rest = b.get("rotation") or (0, 0, 0)
        m = rot(tuple(rest[i] + r[i] for i in range(3)))
        p = j(b["pivot"])
        off = shift(b, pose.get(n))
        a, t = a @ m, a @ (p - m @ p + off) + t
    return a, t


def shift(b, entry):
    """How far a pose moves bone `b`, in model space."""
    if entry is None:
        return np.zeros(3)
    _, pos, absolute = entry
    new = np.array([-pos[0], -pos[1], pos[2]])
    return new - j(b["pivot"]) if absolute else new


def lowest(boxes):
    """The largest model y (the lowest point) of the boxes that are not the tail's, which wags at run time."""
    ys = []
    for d in boxes:
        if d["g"] == "tail":
            continue
        m = rot(np.degrees(d["rot"]))
        for cx in (0, 1):
            for cy in (0, 1):
                for cz in (0, 1):
                    q = d["min"] - d["grow"] + np.array([cx, cy, cz]) * (d["size"] + 2 * d["grow"])
                    ys.append((d["rp"] + m @ q)[1])
    return max(ys)


def flatten(geo, pose):
    bones = bones_of(geo)
    out = []
    for name, b in bones.items():
        for c in b.get("cubes", []):
            a, t = world(bones, name, pose)
            o, s = c["origin"], c["size"]
            lo = j([o[0] + s[0], o[1] + s[1], o[2]])  # min corner in model space (x and y mirrored)
            hi = j([o[0], o[1], o[2] + s[2]])
            size = hi - lo
            g = root_of(bones, name)
            if g not in GROUPS:
                continue
            rp = j(bones[g]["pivot"]) + shift(bones[g], pose.get(g))
            # rp + A q = A x + t with q = x + s_  ->  s_ = A^-1 (t - rp)
            sh = np.linalg.solve(a, t - rp)
            out.append(dict(g=g, min=lo + sh, size=size, rp=rp, rot=euler(a), uv=c["uv"], grow=c.get("inflate") or 0.0, bone=name))
    out.sort(key=lambda d: GROUPS.index(d["g"]))
    low = lowest(out)
    if low > 24.0:  # the pack's sitting numbers put the 12 px front legs a little under the ground: lift the whole pose
        for d in out:
            d["rp"] = d["rp"] - np.array([0.0, low - 24.0, 0.0])
    return out


def f(x):
    s = f"{x:.4f}".rstrip("0")
    return s + "0" if s.endswith(".") else s


def vec(v):
    return "[" + ", ".join(f(x) if abs(x) > 1e-6 else "0.0" for x in v) + "]"


def emit(name, boxes):
    lines = [f"pub const {name}: [Wb; {len(boxes)}] = ["]
    for d in boxes:
        sw = SWING.get(d["g"], (0.0, 0.0))
        if name == "SIT":
            sw = (0.0, 0.0)  # a sitting wolf's legs do not swing
        lines.append(
            f"    ({vec(d['min'])}, {vec(d['size'])}, {vec(d['rp'])}, {vec(d['rot'])}, [{f(sw[0]) if sw[0] else '0.0'}, {f(sw[1]) if sw[1] else '0.0'}], ({f(d['uv'][0])}, {f(d['uv'][1])}), {f(d['grow']) if d['grow'] else '0.0'}),  // {d['g']} {d['bone']}"
        )
    lines.append("];")
    return "\n".join(lines)


def preview(boxes, sheet, k, yaw, path, tail=0.2 * math.pi, limb=0.0, scale=14):
    """A software render of `boxes` as `render/items.rs::draw_parts` + `push_skin_box` draw them (both windings, alpha < 0.5 cut out),
    seen from the side at `yaw` degrees, skin `k` of `sheet`. For eyeballing the table; the game does not use it."""
    tex = np.asarray(sheet.crop(((k % 4) * 64, (k // 4) * 48, (k % 4) * 64 + 64, (k // 4) * 48 + 48)))
    W, H = 40 * scale, 34 * scale
    img = np.zeros((H, W, 4), np.uint8)
    img[..., :3] = 70
    img[..., 3] = 255
    zb = np.full((H, W), 1e9)
    cy, sy_ = math.cos(math.radians(yaw)), math.sin(math.radians(yaw))
    for i, (mn, sz, rp, r, sw, uv, g) in enumerate(boxes):
        rx = r[0] + math.cos(limb * 0.6662 + sw[0]) * sw[1] * 1.0 * (limb > 0)
        if i >= len(boxes) - 2:
            rx = tail
        m = rot((math.degrees(rx), math.degrees(r[1]), math.degrees(r[2])))
        (x1, y1, z1), (x2, y2, z2) = (np.array(mn) - g), (np.array(mn) + np.array(sz) + g)
        dx, dy, dz = sz
        ox, oy = uv
        c = [[x1, y1, z1], [x2, y1, z1], [x2, y2, z1], [x1, y2, z1], [x1, y1, z2], [x2, y1, z2], [x2, y2, z2], [x1, y2, z2]]
        faces = [([5, 1, 2, 6], [ox + dz + dx, oy + dz, ox + dz + dx + dz, oy + dz + dy]), ([0, 4, 7, 3], [ox, oy + dz, ox + dz, oy + dz + dy]),
                 ([5, 4, 0, 1], [ox + dz, oy, ox + dz + dx, oy + dz]), ([2, 3, 7, 6], [ox + dz + dx, oy, ox + dz + dx + dx, oy + dz]),
                 ([1, 0, 3, 2], [ox + dz, oy + dz, ox + dz + dx, oy + dz + dy]), ([4, 5, 6, 7], [ox + dz + dx + dz, oy + dz, ox + dz + dx + dz + dx, oy + dz + dy])]
        for vi, (u0, v0, u1, v1) in faces:
            uvs = [(u1 - 0.1, v0 + 0.1), (u0 + 0.1, v0 + 0.1), (u0 + 0.1, v1 - 0.1), (u1 - 0.1, v1 - 0.1)]
            pts = []
            for q in vi:
                p = np.array(rp) + m @ np.array(c[q])  # model space
                w = np.array([-p[0], -(p[1] - 23.875), p[2]])  # root: scale (-1, -1, 1), feet at 23.875
                xs, zs = w[0] * cy + w[2] * sy_, -w[0] * sy_ + w[2] * cy
                pts.append((W / 2 + xs * scale, H - 3 * scale - w[1] * scale, zs))
            for tri in ((0, 1, 2), (0, 2, 3)):
                raster(img, zb, tex, [pts[t] for t in tri], [uvs[t] for t in tri])
    Image.fromarray(img).save(path)


def raster(img, zb, tex, p, uv):
    (x0, y0, z0), (x1, y1, z1), (x2, y2, z2) = p
    d = (y1 - y2) * (x0 - x2) + (x2 - x1) * (y0 - y2)
    if abs(d) < 1e-9:
        return
    for y in range(max(0, int(min(y0, y1, y2))), min(img.shape[0], int(max(y0, y1, y2)) + 1)):
        for x in range(max(0, int(min(x0, x1, x2))), min(img.shape[1], int(max(x0, x1, x2)) + 1)):
            a = ((y1 - y2) * (x + 0.5 - x2) + (x2 - x1) * (y + 0.5 - y2)) / d
            b = ((y2 - y0) * (x + 0.5 - x2) + (x0 - x2) * (y + 0.5 - y2)) / d
            c = 1 - a - b
            if a < 0 or b < 0 or c < 0:
                continue
            z = a * z0 + b * z1 + c * z2
            if z >= zb[y, x]:
                continue
            u, v = a * uv[0][0] + b * uv[1][0] + c * uv[2][0], a * uv[0][1] + b * uv[1][1] + c * uv[2][1]
            t = tex[min(47, max(0, int(v))), min(63, max(0, int(u)))]
            if t[3] >= 128:
                zb[y, x] = z
                img[y, x, :3] = t[:3]


def main():
    args = sys.argv[1:]
    zf = zipfile.ZipFile(args[0])
    geo = json.loads(zf.read("models/entity/wolf.geo.json"))["minecraft:geometry"][0]
    stand, sit = flatten(geo, {}), flatten(geo, SIT)
    # the Bedrock `head` has no sitting override, so the two poses share its boxes and tail pivot logic
    head_n = sum(d["g"] == "head" for d in stand)
    assert head_n == sum(d["g"] == "head" for d in sit) and len(stand) == len(sit)
    tail_n = sum(d["g"] == "tail" for d in stand)
    assert stand[-1]["g"] == "tail" and tail_n == 2 and sum(d["g"] == "tail" for d in sit) == tail_n
    for nm, bx in (("stand", stand), ("sit", sit)):
        lo = np.min([d["min"] + d["rp"] * 0 for d in bx], axis=0)
        print(nm, len(bx), "boxes, head", head_n)
        for d in bx:
            print("  %-5s %-10s min %s size %s rp %s rot %s uv %s" % (d["g"], d["bone"], np.round(d["min"], 2), np.round(d["size"], 2), np.round(d["rp"], 2), np.round(np.degrees(d["rot"]), 1), d["uv"]))
    if "--preview" in args:
        sheet = Image.open(io.BytesIO(zf.read("textures/entity/wolf/wolf.png"))).convert("RGBA")
        full = Image.new("RGBA", SHEET)
        full.paste(sheet, (0, 0))
        out = args[args.index("--preview") + 1]
        tb = lambda b: [(tuple(d["min"]), tuple(d["size"]), tuple(d["rp"]), tuple(d["rot"]), SWING.get(d["g"], (0.0, 0.0)), tuple(d["uv"]), d["grow"]) for d in b]
        for k, tag in ((0, "wild"), (2, "angry")):
            sh = Image.new("RGBA", SHEET)
            sh.paste(Image.open(io.BytesIO(zf.read("textures/entity/wolf/wolf" + ("_angry" if k else "") + ".png"))).convert("RGBA"), (0, 0))
            for name, bx, yaw, lb in (("stand_side", stand, 90, 0.0), ("walk_34", stand, 35, 1.3), ("sit_side", sit, 90, 0.0), ("sit_34", sit, 35, 0.0)):
                preview(tb(bx), sh, 0, yaw, f"{out}/{tag}_{name}.png", limb=lb, tail=(0.49 if k else 0.2) * math.pi)
    if "--write" in args:
        sheet = Image.new("RGBA", SHEET, (0, 0, 0, 0))
        for v, var in enumerate(VARIANTS):
            for s, st in enumerate(STATES):
                n = "textures/entity/wolf/wolf" + ("_" + var if var else "") + ("_" + st if st else "") + ".png"
                im = Image.open(io.BytesIO(zf.read(n))).convert("RGBA")
                assert im.size == CELL, (n, im.size)
                k = v * 3 + s
                sheet.paste(im, ((k % 4) * 64, (k // 4) * 48))
        open("mc-rs/assets/wolf.rgba", "wb").write(sheet.tobytes())
        src = (
            "//! GENERATED by `tools/gen_wolf.py` from Acidul's Wolves (Bedrock port by Parzival_, geometry `wolf.geo.json`). NOT b1.7.3. Do not edit.\n"
            "//!\n"
            "//! The wolf as boxes, standing and sitting: `(min, size, rotation point, rotation, swing phase + amplitude, uv, inflate)` in model pixels,\n"
            "//! the shape `render/items.rs` calls a `Part`. The first `HEAD_N` boxes are the head's, the last `TAIL_N` the tail's.\n\n"
            "pub type Wb = ([f32; 3], [f32; 3], [f32; 3], [f32; 3], [f32; 2], (f32, f32), f32);\n"
            f"pub const HEAD_N: usize = {head_n};\npub const TAIL_N: usize = {tail_n};\n"
            f"{emit('STAND', stand)}\n{emit('SIT', sit)}\n"
        )
        open("mc-rs/src/render/wolf_model.rs", "w").write(src)
        print("written")


if __name__ == "__main__":
    main()
