#!/usr/bin/env python3
"""Tectonic terrain: bundle the datapack for the Rust crate, and a slow reference evaluator of it.

  tectonic.py bundle <resourcepacks/tectonic> [out]   -> tectonic/terrain.json (default out)
  tectonic.py golden [bundle]                         -> prints the lines `tectonic/src/lib.rs` tests compare against
  tectonic.py map [bundle] [out.pgm] [step]           -> height map of the 128-high world (eyeballing the config)

The reference is the same arithmetic as `tectonic/src/lib.rs` written a second time, in Python: when the two disagree one of
them has a typo. Neither is checked against Minecraft itself (see the UNVERIFIED notes in lib.rs).
"""
import hashlib, json, math, os, sys

M = (1 << 64) - 1
PACK_OVERLAYS = ["data", "overlay.mod/data", "overlay.1_21_9_mod/data", "overlay.1_21_9/data", "overlay.26_2/data"]  # later wins
# Not ported (b1.7.3 keeps its own caves; the mod's cave / river / lava-tunnel noise needs vanilla functions this port does not have).
STUBS = {"tectonic:caves": 1000.0, "tectonic:cave/noodle": 64.0, "tectonic:underground_river/total": 1.0, "tectonic:lava_tunnel/total": 0.0}
ROOT = "minecraft:overworld/noise_router/final_density"
# UNVERIFIED: vanilla 1.21 noise parameters, from memory (the pack does not carry them).
VANILLA_NOISES = {
    "minecraft:offset": (-3, [1, 1, 1, 0]),
    "minecraft:jagged": (-16, [1.0] * 16),
    "minecraft:temperature": (-10, [1.5, 0, 1, 0, 0, 0]),
    "minecraft:vegetation": (-8, [1, 1, 0, 0, 0, 0]),
}
BUILTINS = {"minecraft:y", "minecraft:shift_x", "minecraft:shift_z", "minecraft:overworld/base_3d_noise"}


def md5_pair(s):
    d = hashlib.md5(s.encode()).digest()
    return [format(int.from_bytes(d[:8], "big"), "x"), format(int.from_bytes(d[8:], "big"), "x")]


def bundle(pack, out):
    def find(kind, ident):
        ns, p = ident.split(":") if ":" in ident else ("minecraft", ident)
        for r in reversed(PACK_OVERLAYS):
            f = f"{pack}/{r}/{ns}/worldgen/{kind}/{p}.json"
            if os.path.exists(f):
                return f
    fns, noises = {}, {}

    def walk(n):
        if isinstance(n, str):
            if n in fns or n in STUBS or n in BUILTINS:
                return
            f = find("density_function", n)
            assert f, "missing density function " + n
            fns[n] = json.load(open(f))
            walk(fns[n])
        elif isinstance(n, list):
            for x in n:
                walk(x)
        elif isinstance(n, dict):
            if "noise" in n and isinstance(n["noise"], str):
                noises.setdefault(n["noise"], None)
            for k, v in n.items():
                if k not in ("type", "noise", "key"):
                    walk(v)
    walk(ROOT)
    noises.setdefault("minecraft:offset", None)  # shift_x / shift_z
    for k in noises:
        if k in VANILLA_NOISES:
            o, a = VANILLA_NOISES[k]
        else:
            j = json.load(open(find("noise", k)))
            o, a = j["firstOctave"], j["amplitudes"]
        noises[k] = {"o": o, "a": a, "h": md5_pair(k)}
    octs = {str(i): md5_pair(f"octave_{i}") for i in range(-16, 1)}
    json.dump({"root": ROOT, "stubs": STUBS, "f": fns, "n": noises, "oct": octs, "terrain": md5_pair("minecraft:terrain")}, open(out, "w"), separators=(",", ":"))
    print(f"{out}: {len(fns)} functions, {len(noises)} noises, {os.path.getsize(out)} bytes")


# ---------------------------------------------------------------- noise (net.minecraft.world.level.levelgen)
def rotl(v, k):
    return ((v << k) | (v >> (64 - k))) & M


def stafford(z):
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & M
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & M
    return z ^ (z >> 31)


class Xoro:
    def __init__(s, lo, hi):
        if lo == 0 and hi == 0:
            lo, hi = 0x9E3779B97F4A7C15, 0x6A09E667F3BCC909
        s.lo, s.hi = lo, hi

    @staticmethod
    def from_seed(seed):
        m = (seed & M) ^ 0x6A09E667F3BCC909
        return Xoro(stafford(m), stafford((m + 0x9E3779B97F4A7C15) & M))

    def next_long(s):
        l, m = s.lo, s.hi
        n = (rotl((l + m) & M, 17) + l) & M
        m ^= l
        s.lo = rotl(l, 49) ^ m ^ ((m << 21) & M)
        s.hi = rotl(m, 28)
        return n

    def next_int(s, bound):
        l = s.next_long() & 0xFFFFFFFF
        m = l * bound
        n = m & 0xFFFFFFFF
        if n < bound:
            j = ((~bound + 1) & 0xFFFFFFFF) % bound
            while n < j:
                l = s.next_long() & 0xFFFFFFFF
                m = l * bound
                n = m & 0xFFFFFFFF
        return m >> 32

    def next_double(s):
        return (s.next_long() >> 11) * 1.1102230246251565e-16

    def fork(s):  # forkPositional
        return (s.next_long(), s.next_long())


def from_hash(fac, h):  # PositionalRandomFactory.fromHashOf with a precomputed md5
    return Xoro(int(h[0], 16) ^ fac[0], int(h[1], 16) ^ fac[1])


GRAD = [(1, 1, 0), (-1, 1, 0), (1, -1, 0), (-1, -1, 0), (1, 0, 1), (-1, 0, 1), (1, 0, -1), (-1, 0, -1),
        (0, 1, 1), (0, -1, 1), (0, 1, -1), (0, -1, -1), (1, 1, 0), (0, -1, 1), (-1, 1, 0), (0, -1, -1)]


def lerp(t, a, b):
    return a + t * (b - a)


def smooth(t):
    return t * t * t * (t * (t * 6.0 - 15.0) + 10.0)


class Improved:
    def __init__(s, r):
        s.xo, s.yo, s.zo = r.next_double() * 256.0, r.next_double() * 256.0, r.next_double() * 256.0
        p = list(range(256))
        for j in range(256):
            k = r.next_int(256 - j)
            p[j], p[j + k] = p[j + k], p[j]
        s.p = p

    def noise(s, x, y, z, ys=0.0, ym=0.0):
        d, e, f = x + s.xo, y + s.yo, z + s.zo
        i, j, k = math.floor(d), math.floor(e), math.floor(f)
        g, h, l = d - i, e - j, f - k
        o = 0.0
        if ys != 0.0:
            m = ym if 0.0 <= ym < h else h
            o = math.floor(m / ys + 1.0e-7) * ys
        p = s.p
        a = p[i & 255]; b = p[(i + 1) & 255]
        k0 = p[(a + j) & 255]; k1 = p[(a + j + 1) & 255]; m0 = p[(b + j) & 255]; m1 = p[(b + j + 1) & 255]
        d0, e0, f0 = g, h - o, l

        def gd(hh, x_, y_, z_):
            q = GRAD[p[hh & 255] & 15]
            return q[0] * x_ + q[1] * y_ + q[2] * z_
        c000 = gd(k0 + k, d0, e0, f0); c100 = gd(m0 + k, d0 - 1, e0, f0)
        c010 = gd(k1 + k, d0, e0 - 1, f0); c110 = gd(m1 + k, d0 - 1, e0 - 1, f0)
        c001 = gd(k0 + k + 1, d0, e0, f0 - 1); c101 = gd(m0 + k + 1, d0 - 1, e0, f0 - 1)
        c011 = gd(k1 + k + 1, d0, e0 - 1, f0 - 1); c111 = gd(m1 + k + 1, d0 - 1, e0 - 1, f0 - 1)
        sx, sy, sz = smooth(d0), smooth(h), smooth(f0)
        return lerp(sz, lerp(sy, lerp(sx, c000, c100), lerp(sx, c010, c110)), lerp(sy, lerp(sx, c001, c101), lerp(sx, c011, c111)))


def wrap(v):
    return v - math.floor(v / 33554432.0 + 0.5) * 33554432.0


class Perlin:
    def __init__(s, first, amps, fac, octs):  # modern init: one hashed random per octave
        s.amps = amps
        s.levels = [Improved(from_hash(fac, octs[str(first + i)])) if a != 0 else None for i, a in enumerate(amps)]
        n = len(amps)
        s.fin = 2.0 ** first
        s.fvf = 2.0 ** (n - 1) / (2.0 ** n - 1.0)

    def value(s, x, y, z):
        d, e, f = 0.0, s.fin, s.fvf
        for a, nz in zip(s.amps, s.levels):
            if nz is not None:
                d += a * nz.noise(wrap(x * e), wrap(y * e), wrap(z * e)) * f
            e *= 2.0
            f /= 2.0
        return d


class Normal:
    def __init__(s, r, first, amps):
        octs = Normal.octs
        s.a = Perlin(first, amps, r.fork(), octs)
        s.b = Perlin(first, amps, r.fork(), octs)
        nz = [i for i, a in enumerate(amps) if a != 0]
        s.vf = (1.0 / 6.0) / (0.1 * (1.0 + 1.0 / (nz[-1] - nz[0] + 1)))  # NormalNoise.expectedDeviation(k - j) = 0.1 * (1 + 1 / (octaves + 1))

    def value(s, x, y, z):
        return (s.a.value(x, y, z) + s.b.value(x * 1.0181268882175227, y * 1.0181268882175227, z * 1.0181268882175227)) * s.vf


class Blended:  # BlendedNoise(0.25, 0.125, 80, 160, 8): the old b1.7.3 terrain noise
    def __init__(s, r):
        def legacy(n):  # PerlinNoise.createLegacyForBlendedNoise: octave 0 first, then -1, -2, ...
            lv = [None] * n
            lv[n - 1] = Improved(r)
            for k in range(n - 2, -1, -1):
                lv[k] = Improved(r)
            return lv
        s.lo, s.hi, s.main = legacy(16), legacy(16), legacy(8)

    def value(s, x, y, z):
        xm, ym = 684.412 * 0.25, 684.412 * 0.125
        d0, d1, d2 = x * xm, y * ym, z * xm
        d3, d4, d5 = d0 / 80.0, d1 / 160.0, d2 / 80.0
        d6 = ym * 8.0
        d7 = d6 / 160.0
        d8, d9 = 0.0, 1.0
        for i in range(8):
            n = s.main[len(s.main) - 1 - i]
            d8 += n.noise(wrap(d3 * d9), wrap(d4 * d9), wrap(d5 * d9), d7 * d9, d4 * d9) / d9
            d9 /= 2.0
        d10 = (d8 / 10.0 + 1.0) / 2.0
        f1, f2 = d10 >= 1.0, d10 <= 0.0
        d9, d11, d12 = 1.0, 0.0, 0.0
        for i in range(16):
            x2, y2, z2 = wrap(d0 * d9), wrap(d1 * d9), wrap(d2 * d9)
            if not f1:
                d11 += s.lo[15 - i].noise(x2, y2, z2, d6 * d9, d1 * d9) / d9
            if not f2:
                d12 += s.hi[15 - i].noise(x2, y2, z2, d6 * d9, d1 * d9) / d9
            d9 /= 2.0
        a, b, t = d11 / 512.0, d12 / 512.0, d10
        return (a if t < 0 else b if t > 1 else lerp(t, a, b)) / 128.0


# ---------------------------------------------------------------- config (ConfigState defaults)
def default_config(min_y=-1.0, max_y=127.0, vertical_scale=1.125, elevation_boost=0.0):
    c = {
        "vertical_scale": vertical_scale, "elevation_boost": elevation_boost, "min_y": min_y, "max_y": max_y,
        "ocean_offset": -0.8, "flat_terrain_skew": 0.1, "rolling_hills": 1.0, "jungle_pillars": 1.0,
        "ocean_depth": -0.22, "deep_ocean_depth": -0.45,
    }
    # config_noise: (scale, multiplier, offset)
    n = {"continents": (0.13, 1, 0), "erosion": (0.25, 1, 0), "ridge": (0.25, 1, 0), "island": (0.11, 1, 0), "temperature": (0.25, 1, 0), "vegetation": (0.25, 1, 0)}
    return c, n


# ---------------------------------------------------------------- density functions
def f32(v):
    import struct
    return struct.unpack("f", struct.pack("f", v))[0]


class Terrain:
    def __init__(s, bundle_path, seed, config=None):
        b = json.load(open(bundle_path))
        s.path = bundle_path
        s.b = b
        s.cfg, s.ncfg = config or default_config()
        Normal.octs = b["oct"]
        root = Xoro.from_seed(seed).fork()
        s.noise = {k: Normal(from_hash(root, v["h"]), v["o"], v["a"]) for k, v in b["n"].items()}
        s.blended = Blended(from_hash(root, b["terrain"]))
        s.memo = {}
        s.y2 = {}
        s.interp = {}
        s.interp_mode = None

    def reset(s):
        s.memo = {}

    def deps_y(s, n):  # does a node depend on y? (2D nodes are memoised per column)
        k = id(n)
        if k in s.y2:
            return s.y2[k]
        s.y2[k] = True  # cycles cannot exist; pessimistic default
        r = s._deps_y(n)
        s.y2[k] = r
        return r

    def _deps_y(s, n):
        if isinstance(n, (int, float)):
            return False
        if isinstance(n, str):
            if n in s.b["stubs"]:
                return False
            if n == "minecraft:y" or n == "minecraft:overworld/base_3d_noise":
                return True
            if n in ("minecraft:shift_x", "minecraft:shift_z"):
                return False
            return s.deps_y(s.b["f"][n])
        t = n["type"]
        if t in ("minecraft:y_clamped_gradient", "minecraft:old_blended_noise", "minecraft:interpolated"):
            return True
        if t == "minecraft:noise" and n.get("y_scale", 1) != 0:
            return True
        if t == "minecraft:shifted_noise" and (n.get("y_scale", 1) != 0 or s.deps_y(n["shift_y"])):
            return True
        if t == "minecraft:spline":
            return s.spline_deps_y(n["spline"])
        return any(s.deps_y(v) for k, v in n.items() if k not in ("type", "noise", "key") and isinstance(v, (str, dict, list, int, float)))

    def spline_deps_y(s, sp):
        if isinstance(sp, (int, float)):
            return False
        return s.deps_y(sp["coordinate"]) or any(s.spline_deps_y(p["value"]) for p in sp["points"])

    def ev(s, n, x, y, z):
        if isinstance(n, (int, float)):
            return float(n)
        if isinstance(n, str):
            if n in s.b["stubs"]:
                return s.b["stubs"][n]
            if n == "minecraft:y":
                return float(y)
            if n == "minecraft:shift_x":
                return s.noise["minecraft:offset"].value(x * 0.25, 0.0, z * 0.25) * 4.0
            if n == "minecraft:shift_z":
                return s.noise["minecraft:offset"].value(z * 0.25, x * 0.25, 0.0) * 4.0
            if n == "minecraft:overworld/base_3d_noise":
                return s.blended.value(x, y, z)
            n = s.b["f"][n]
            if isinstance(n, (int, float)):
                return float(n)
        k = None
        if not s.deps_y(n):
            k = (id(n), x, z)
            if k in s.memo:
                return s.memo[k]
        v = s.node(n, x, y, z)
        if k is not None:
            s.memo[k] = v
        return v

    def node(s, n, x, y, z):
        t = n["type"][n["type"].index(":") + 1:] if n["type"].startswith("minecraft:") else n["type"]
        e = lambda a: s.ev(n[a], x, y, z)
        if t == "add": return e("argument1") + e("argument2")
        if t == "mul": return e("argument1") * e("argument2")
        if t == "min": return min(e("argument1"), e("argument2"))
        if t == "max": return max(e("argument1"), e("argument2"))
        if t in ("flat_cache", "cache_2d", "cache_once", "cache_all_in_cell", "blend_density"): return e("argument")
        if t == "interpolated":
            if s.interp_mode is not None:
                return s.interp_mode(n, x, y, z)
            return e("argument")
        if t == "abs": return abs(e("argument"))
        if t == "square": return e("argument") ** 2
        if t == "cube": return e("argument") ** 3
        if t == "half_negative": v = e("argument"); return v * 0.5 if v < 0 else v
        if t == "quarter_negative": v = e("argument"); return v * 0.25 if v < 0 else v
        if t == "squeeze": c = min(max(e("argument"), -1.0), 1.0); return c / 2.0 - c * c * c / 24.0
        if t == "clamp": return min(max(e("input"), n["min"]), n["max"])
        if t == "blend_alpha": return 1.0
        if t == "blend_offset": return 0.0
        if t == "y_clamped_gradient":
            a, b = n["from_y"], n["to_y"]
            u = min(max((y - a) / (b - a), 0.0), 1.0)
            return n["from_value"] + u * (n["to_value"] - n["from_value"])
        if t == "range_choice":
            v = e("input")
            return e("when_in_range") if n["min_inclusive"] <= v < n["max_exclusive"] else e("when_out_of_range")
        if t == "noise":
            xs, ys = n["xz_scale"], n["y_scale"]
            return s.noise[n["noise"]].value(x * xs, y * ys, z * xs)
        if t == "shifted_noise":
            xs, ys = n["xz_scale"], n["y_scale"]
            return s.noise[n["noise"]].value(x * xs + e("shift_x"), y * ys + e("shift_y"), z * xs + e("shift_z"))
        if t == "spline": return s.spline(n["spline"], x, y, z)
        if t == "tectonic:config_constant": return s.cfg[n["key"]]
        if t == "tectonic:config_noise":
            sc, mu, of = s.ncfg[n["key"]]
            return s.noise[n["noise"]].value(x * sc + e("shift_x"), 0.0, z * sc + e("shift_z")) * mu + of
        if t == "tectonic:config_clamp": return min(max(e("input"), e("min")), e("max"))
        if t == "tectonic:invert": return 1.0 / e("argument")
        raise ValueError(t)

    def spline(s, sp, x, y, z):
        if isinstance(sp, (int, float)):
            return f32(sp)
        f = f32(s.ev(sp["coordinate"], x, y, z))
        pts = sp["points"]
        loc = [f32(p["location"]) for p in pts]
        i = -1
        for k, l in enumerate(loc):
            if l <= f:
                i = k
        last = len(pts) - 1
        val = lambda k: s.spline(pts[k]["value"], x, y, z)
        der = lambda k: f32(pts[k]["derivative"])
        if i < 0 or i == last:
            k = 0 if i < 0 else last
            d = der(k); v = val(k)
            return v if d == 0 else f32(v + d * (f - loc[k]))
        l0, l1 = loc[i], loc[i + 1]
        t = f32((f - l0) / (l1 - l0))
        y0, y1 = val(i), val(i + 1)
        d0, d1 = der(i), der(i + 1)
        a = f32(d0 * (l1 - l0) - (y1 - y0))
        b = f32(-d1 * (l1 - l0) + (y1 - y0))
        return f32(f32(y0 + t * (y1 - y0)) + f32(f32(t * (1 - t)) * f32(a + t * (b - a))))

    # ---- chunk grid
    def interp_nodes(s):
        out, seen = [], set()

        def w(n):
            if isinstance(n, str):
                if n in s.b["stubs"] or n.startswith("minecraft:") and n not in s.b["f"]:
                    return
                n = s.b["f"][n]
            if isinstance(n, list):
                for v in n: w(v)
                return
            if not isinstance(n, dict) or id(n) in seen:
                return
            seen.add(id(n))
            if n.get("type") == "minecraft:interpolated":
                out.append(n)
                return
            for k, v in n.items():
                if k not in ("type", "noise", "key"): w(v)
        w(s.b["root"])
        return out

    def chunk(s, cx, cz, height=128, sea=64):
        """Density of every block of the chunk, [x][z][y]; y of the tectonic frame = y - (sea - 63)."""
        s.reset()
        sh = sea - 63
        nodes = s.interp_nodes()
        tables = {}
        ny = height // 8 + 1
        for n in nodes:
            tab = {}
            for gx in range(5):
                for gz in range(5):
                    for gy in range(ny):
                        s.interp_mode = None
                        tab[(gx, gz, gy)] = s.ev(n["argument"], cx * 16 + gx * 4, gy * 8 - sh, cz * 16 + gz * 4)
            tables[id(n)] = tab

        def look(n, x, y, z):
            fx, fz, fy = (x - cx * 16) / 4.0, (z - cz * 16) / 4.0, (y + sh) / 8.0
            ix, iz, iy = min(int(fx), 3), min(int(fz), 3), min(int(fy), ny - 2)
            tx, tz, ty = fx - ix, fz - iz, fy - iy
            t = tables[id(n)]
            c = lambda a, b, d: t[(ix + a, iz + b, iy + d)]
            return lerp(tx, lerp(tz, lerp(ty, c(0, 0, 0), c(0, 0, 1)), lerp(ty, c(0, 1, 0), c(0, 1, 1))),
                        lerp(tz, lerp(ty, c(1, 0, 0), c(1, 0, 1)), lerp(ty, c(1, 1, 0), c(1, 1, 1))))
        s.interp_mode = look
        out = [[[0.0] * height for _ in range(16)] for _ in range(16)]
        for x in range(16):
            for z in range(16):
                for y in range(height):
                    out[x][z][y] = s.ev(s.b["root"], cx * 16 + x, y - sh, cz * 16 + z)
        s.interp_mode = None
        return out


def surface(col, sea=64):
    for y in range(len(col) - 1, -1, -1):
        if col[y] > 0:
            return y
    return -1


def main(a):
    here = os.path.dirname(os.path.abspath(__file__))
    default = os.path.join(here, "..", "tectonic", "terrain.json")
    if a[0] == "bundle":
        bundle(a[1], a[2] if len(a) > 2 else default)
    elif a[0] == "golden":
        t = Terrain(a[1] if len(a) > 1 else default, 7)
        print(golden(t))
    elif a[0] == "map":
        t = Terrain(a[1] if len(a) > 1 else default, 7)
        out = a[2] if len(a) > 2 else "map.pgm"
        step = int(a[3]) if len(a) > 3 else 8
        heightmap(t, out, step)


def golden(t):
    n = t.noise["tectonic:parameter/continentalness"]
    lines = [f"NOISE {n.value(1234.5, 0.0, -987.25):.9f}", f"NOISE {n.value(-3.0, 0.0, 17.0):.9f}"]
    t.reset()
    lines.append(f"BLEND {t.blended.value(100.0, 70.0, -40.0):.9f}")
    lines.append(f"SHIFT {t.ev('minecraft:shift_x', 64, 0, -32):.9f}")
    # the game's own world: seed 0xCAFEBABE, Config::compact(128, 64)
    c = Terrain(t.path, 0xCAFEBABE, default_config(vertical_scale=0.3))
    for cx, cz in ((0, 0), (-2, 3)):
        ch = c.chunk(cx, cz)
        for x, z in ((0, 0), (7, 9), (15, 15)):
            lines.append(f"TOP {cx} {cz} {x} {z} {surface(ch[x][z])}")
        lines.append(f"DENS {cx} {cz} 8 70 8 {ch[8][8][70]:.6f}")
        lines.append(f"DENS {cx} {cz} 3 20 12 {ch[3][12][20]:.6f}")
    return "\n".join(lines)


def heightmap(t, out, step):
    # one column per `step` blocks: the surface from the cell-corner densities only (no block interpolation), enough to see the shape
    import random
    n = 64
    rows = []
    sea = 64
    sh = sea - 63
    t.reset()
    hs = []
    for iz in range(n):
        row = []
        for ix in range(n):
            x, z = (ix - n // 2) * step, (iz - n // 2) * step
            h = -1
            # the density is the interpolated node's argument at corners; take its sign through the outer function
            col = []
            for gy in range(17):
                t.interp_mode = lambda nd, X, Y, Z: t.ev(nd["argument"], X, Y, Z)
                col.append(t.ev(t.b["root"], x, gy * 8 - sh, z))
            for gy in range(16, -1, -1):
                if col[gy] > 0:
                    h = gy * 8
                    # refine inside the cell by linear interpolation toward the next level
                    if gy < 16:
                        a, b = col[gy], col[gy + 1]
                        h += 8 * a / (a - b)
                    break
            row.append(h)
        hs.append(row)
        if iz % 8 == 0:
            t.reset()
    flat = [h for r in hs for h in r]
    land = [h for h in flat if h >= sea]
    print(f"columns {len(flat)}: min {min(flat):.0f} max {max(flat):.0f} sea {100*sum(h < sea for h in flat)/len(flat):.0f}%"
          f" land mean {sum(land)/max(1,len(land)):.1f}; p50 {sorted(flat)[len(flat)//2]:.0f} p90 {sorted(flat)[int(len(flat)*.9)]:.0f} p99 {sorted(flat)[int(len(flat)*.99)]:.0f}")
    with open(out, "w") as f:
        f.write(f"P2\n{n} {n}\n127\n")
        for r in hs:
            f.write(" ".join(str(max(0, min(127, int(h)))) for h in r) + "\n")
    return hs


if __name__ == "__main__":
    main(sys.argv[1:])
