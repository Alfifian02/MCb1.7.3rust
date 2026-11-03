#!/usr/bin/env python3
"""Twin of `mc-rs/src/world/gen/natural.rs`: the climate pipeline of the Natural Temperature mod (Fabric 1.2.1), read from its bytecode.

Run `python3 tools/natural_proto.py tools/natural_golden.txt`: every line of the file is one case of the mod's own `PainterCore`
(`calculateTemperature` / `calculateHumidity`, run on a JVM with the Minecraft noise replaced by the two sine stubs below), and this
file must give the same two numbers (to a float's precision). Nothing else is checked here; the Rust port mirrors this file line by line.
"""
import math, struct, sys
import numpy as np  # Java's double arithmetic: x / 0.0 is inf and fmod(x, 0.0) is NaN, Python's math raises

M64 = (1 << 64) - 1
K1, K2 = -3335678366873096957 & M64, -7046029254386353131 & M64
C1, C2 = -4658895280553007687 & M64, -7723592293110705685 & M64
NAN = float("nan")


def f32(v):
    return struct.unpack("f", struct.pack("f", v))[0]


def clamp(v, lo, hi):
    return max(lo, min(hi, v))


# ---- MathUtils ----
def random01(seed, idx, salt):
    h = (seed + salt * K1 + idx * K2) & M64
    h = ((h ^ (h >> 30)) * C1) & M64
    h = ((h ^ (h >> 27)) * C2) & M64
    h ^= h >> 31
    return (h >> 11) / 9007199254740992.0


def point_position(idx, scale, seed):
    return (idx + (0.25 + 0.5 * random01(seed, idx, 1))) * scale


def natural_wave(x, amp, freq, seed):
    if amp == 0.0 or not freq > 0.0:
        return 0.0
    w = 1.0 / freq
    i = math.floor(x / w)
    if x < point_position(i, w, seed):
        i -= 1
    p0, p1 = point_position(i, w, seed), point_position(i + 1, w, seed)
    v0 = (random01(seed, i, 2) * 2.0 - 1.0) * amp
    v1 = (random01(seed, i + 1, 2) * 2.0 - 1.0) * amp
    t = (x - p0) / (p1 - p0)
    return v0 + (v1 - v0) * (t * t * (3.0 - 2.0 * t))


def border(a, b, borders, i, j):
    mid = (a + b) / 2.0
    if j == i + 1 and borders is not None and i < len(borders) and not math.isnan(borders[i]):
        mid = borders[i]
    return max(min(a, b), min(max(a, b), mid))


def interp(x, vals, widths, borders):
    n = len(vals)
    total = sum(max(0.0, w) for w in widths)
    if not total > 0.0:
        return vals[0]
    pos, val = [], []
    cum, prev = 0.0, -1
    for j in range(n):
        w = max(0.0, widths[j]) / total
        if w < 1e-9:
            continue
        end = cum + w
        peak = cum if j == 0 else end if j == n - 1 else (cum + end) / 2.0
        if prev >= 0:
            pos.append(cum)
            val.append(border(vals[prev], vals[j], borders, prev, j))
        pos.append(peak)
        val.append(vals[j])
        prev, cum = j, end
    if x <= pos[0]:
        return val[0]
    if x >= pos[-1]:
        return val[-1]
    for k in range(len(pos) - 1):
        if x <= pos[k + 1]:
            return val[k] + (val[k + 1] - val[k]) * ((x - pos[k]) / (pos[k + 1] - pos[k]))
    return val[-1]


# ---- the calculators; `c` is the context record, `g` the config ----
def pick(v, rnd):
    return rnd if v == 8.0 else v


def tp_border(tp, first):  # the transition points: 8.0 = automatic
    return [first if k == 0 and v == 8.0 else NAN if v == 8.0 else v for k, v in enumerate(tp)]


def fold(c):  # distance from the pole, 0..2a, for banded and discrete
    a = c.a
    l2 = np.fmod(np.fmod(c.mad_z - a, 4.0 * a) + 4.0 * a, 4.0 * a)
    return 4.0 * a - l2 if l2 > 2.0 * a else l2


def tri(c):
    return 0.6366197723675814 * np.arcsin(np.sin(3.141592653589793 * c.adj_z / (2.0 * c.a)))


def outside(c):
    return c.complex_z < -c.a or c.complex_z > 3.0 * c.a


def modded(v, c):
    return clamp(v * c.mit + c.modi, -1.0, 1.0)


def t_default(g, c):
    a, mad = c.a, c.mad_z
    if a == 0.0:
        v = -1.0
    else:
        if mad < 0.0:
            sine = False
        elif mad <= c.jungle:
            sine = True
        else:
            sine = not (mad < c.desert2) and not (mad > 4.0 * a)
        if sine:
            v = 0.7639437268410976 * np.arcsin(np.sin(3.141592653589793 * c.adj_z / (2.0 * a))) + 0.2
            v = max(min(1.0, v * c.mit + c.modi), -1.0)
        elif mad <= c.jungle or mad >= c.desert2:
            v = -1.0
        else:
            v = 2.0 * np.tanh(2.5 * np.cos(3.141592653589793 * c.adj_z / a)) + 2.3
            v = 0.9 * v + 0.1 * c.rnd
    if not g.looping and (c.zp <= -a or c.zp >= 3.0 * a):
        v = -1.0
    return v


def t_simplified(g, c):
    if c.a == 0.0:
        return -1.0
    if g.looping or not outside(c):
        return modded(tri(c), c)
    return -1.0


def t_linear(g, c):
    return modded(clamp((1.0 / c.a) * c.adj_z, -1.0, 1.0), c)


def ring(g, c, n, custom, preset, key, vcol, defaults, preset_vals, x):
    """circular (n 4) / circular6 / square6: the zones are rings round the origin."""
    cov = g.cov[:n] if c.m == custom else [1.0] * n if c.m == preset else g.cov[:n]
    vals = list(preset_vals) if c.m == preset else list(defaults)
    if c.m == custom:
        vals = [pick(v, c.rnd) for v in vcol[:n]]
    tp = tp_border(key[: n - 1], vals[0]) if c.m == custom else [vals[0]] + [NAN] * (n - 2)
    if c.m == preset:
        cov = [1.0] * n
    return -1.0 if c.a == 0.0 else interp(clamp(x / c.radius, 0.0, 1.0), vals, cov, tp)


def t_circular(g, c):
    return ring(g, c, 4, 3, 4, g.tp, g.tz, [0.0, c.rnd, 1.0, -1.0], [0.0, 1.0, -0.5, -1.0], c.r)


def t_circular6(g, c):
    return ring(g, c, 6, 7, 8, g.tp, g.tz, [0.0, c.rnd, 0.35, 1.0, c.rnd, -1.0], [0.0, 0.25, 0.4, 1.0, -0.25, -1.0], c.r)


def t_square6(g, c):
    return ring(g, c, 6, 9, 10, g.tp, g.tz, [0.0, c.rnd, 0.35, 1.0, c.rnd, -1.0], [0.0, 0.25, 0.4, 1.0, -0.25, -1.0], c.r_sq)


def banded_widths(g):
    w = g.cov[:6]
    if g.looping:
        w[0] /= 2.0
        w[5] /= 2.0
    return w


def t_banded(g, c):
    if c.a == 0.0:
        return -1.0
    x = clamp(fold(c) / (2.0 * c.a), 0.0, 1.0)
    if c.m == 6:
        vals, tp = [0.25, 0.7, 0.5, 0.0, -0.25, -0.7], [0.55, 0.55, 0.2, -0.15, -0.45]
    else:
        vals = [pick(v, c.rnd) for v in g.tz[:6]]
        tp = [NAN if v == 8.0 else v for v in g.tp]
    v = modded(interp(x, vals, banded_widths(g), tp), c)
    return vals[5] if not g.looping and outside(c) else v


def discrete(g, c, vals, var, rnd, bands):
    if c.a == 0.0:
        return -1.0
    w = list(g.cov)
    if g.looping:
        w[0] /= 2.0
        w[11] /= 2.0
    k = 2.0 * c.a / sum(w)
    cut, run = [], 0.0
    for i in range(11):
        run += w[i]
        cut.append(run * k)
    f = fold(c)
    z = 11
    for i in range(11):
        if f < cut[i]:
            z = i
            break
    v = clamp(vals[z] + var[z] * rnd, -1.0, 1.0)
    if bands:
        v = 0.6366197723675814 * np.arcsin(np.sin(3.141592653589793 * c.adj_x / (2.0 * c.hbw)))
    return vals[11] if not g.looping and outside(c) else v


def t_discrete(g, c):
    vals = [0.35, 0.35, 0.35, 0.55, 0.9, 0.82, 0.1, -0.1, -0.3, -0.75, -0.75, -0.8] if c.m == 12 else [pick(v, c.rnd) for v in g.tz]
    var = [0.1, 0.1, 0.1, 0.25, 0.1, 0.25, 0.05, 0.1, 0.15, 0.25, 0.25, 0.3] if c.m == 12 else g.tvar
    return discrete(g, c, vals, var, c.rnd, False)


TEMP = {1: t_simplified, 2: t_linear, 3: t_circular, 4: t_circular, 5: t_banded, 6: t_banded, 7: t_circular6, 8: t_circular6, 9: t_square6, 10: t_square6, 11: t_discrete, 12: t_discrete}


def temperature(g, c):
    return TEMP.get(c.m, t_default)(g, c)


# humidity
def band_wave(c):
    return 0.6366197723675814 * np.arcsin(np.sin(3.141592653589793 * c.adj_x / (2.0 * c.hbw)))


def h_default(g, c):
    a, mad, u = c.a, c.mad_z, c.rnd_u
    if a == 0.0:
        v = -1.0
    elif (mad >= 0.0 and mad <= c.desert) or not (mad < c.sav2 or mad > c.temp2):
        v = -1.0
    elif g.extra and c.jungle <= mad <= c.desert2:
        v = 0.7 * (0.8 * np.tanh(2.0 * np.cos(6.283185307179586 * c.adj_z / a)) + 0.15) + 0.3 * u
    elif c.jungle <= mad <= c.desert2:
        sg = np.cos(6.283185307179586 * c.adj_z / a)
        v = 0.7 * (1.0 if sg > 0.0 else -1.0 if sg < 0.0 else sg) + 0.3 * u
    elif c.pole2 <= mad <= c.cold3:
        v = -1.0
    elif (c.desert <= mad <= c.jungle) or (c.desert2 <= mad <= c.sav2):
        v = u if c.t > 0.65 else -1.0
    else:
        v = u
    if not g.looping and (c.zp >= c.s_pole or c.zp <= c.n_pole):
        v = -1.0
    return v


def h_simplified(g, c):
    v = band_wave(c) if g.hbands else c.rnd_u
    return -1.0 if not g.looping and outside(c) else v


def h_linear(g, c):
    v = band_wave(c) if g.hbands else c.rnd_u
    return -1.0 if c.complex_z < -c.a else v


def h_ring(g, c, n, custom, preset, x, defaults):
    u = c.rnd_u
    cov = [1.0] * n if c.m == preset else g.cov[:n]
    vals = [u] * n if c.m == preset else list(defaults)
    if c.m == custom:
        vals = [pick(v, u) for v in g.hz[:n]]
    tp = tp_border(g.ht[: n - 1], vals[0]) if c.m == custom else [vals[0]] + [NAN] * (n - 2)
    v = -1.0 if c.a == 0.0 else interp(clamp(x / c.radius, 0.0, 1.0), vals, cov, tp)
    return band_wave(c) if g.hbands else v


def h_circular(g, c):
    return h_ring(g, c, 4, 3, 4, c.r, [c.rnd_u, c.rnd_u, c.rnd_u, -1.0])


def h_circular6(g, c):
    return h_ring(g, c, 6, 7, 8, c.r, [c.rnd_u] * 5 + [-1.0])


def h_square6(g, c):
    return h_ring(g, c, 6, 9, 10, c.r_sq, [c.rnd_u] * 5 + [-1.0])


def h_banded(g, c):
    if c.a == 0.0:
        return -1.0
    u = c.rnd_u
    x = clamp(fold(c) / (2.0 * c.a), 0.0, 1.0)
    if c.m == 6:
        vals, tp = [1.0, u, -0.7, u, u, -0.15], [NAN, -0.2, -0.2, NAN, NAN]
    else:
        vals = [pick(v, u) for v in g.hz[:6]]
        tp = [NAN if v == 8.0 else v for v in g.ht]
    v = interp(x, vals, banded_widths(g), tp)
    if g.hbands:
        v = band_wave(c)
    return vals[5] if not g.looping and outside(c) else v


def h_discrete(g, c):
    u = c.rnd_u
    vals = [0.7, 0.2, -0.55, -0.5, -0.9, u, u, u, u, 0.5, 0.0, -1.0] if c.m == 12 else [pick(v, u) for v in g.hz]
    var = [0.35, 0.2, 0.5, 0.3, 0.1, 0.15, 1.0, 1.0, 1.0, 0.6, 0.4, 0.55] if c.m == 12 else g.uvar
    return discrete(g, c, vals, var, u, g.hbands)


HUM = {1: h_simplified, 2: h_linear, 3: h_circular, 4: h_circular, 5: h_banded, 6: h_banded, 7: h_circular6, 8: h_circular6, 9: h_square6, 10: h_square6, 11: h_discrete, 12: h_discrete}


def humidity(g, c):
    return HUM.get(c.m, h_default)(g, c)


# ---- PainterCore ----
class O:
    pass


def stub_large(x, z, s):  # the harness's stand-in for the two noises
    return np.sin(x * s * 0.00013 + z * s * 0.00007) * 3.0 + np.cos(z * s * 0.00011)


def stub_detail(x, z, s):
    return np.sin(x * s * 0.011) * np.cos(z * s * 0.017) * 0.8


def climate(g, x, z, seed, raw_t, raw_u, large=stub_large, detail=stub_detail):
    """-> (temperature, humidity) of the mod at block (x, z), before the influence blend."""
    a, off = g.a, g.off
    nl, nd = large(float(x), float(z), 6.0), detail(float(x), float(z), 6.0)
    wm = g.wm if g.wm != 0.0 else 0.01
    amp = g.wa * (30.0 * wm)
    wave_t = natural_wave(float(x), amp, g.wf * (0.001 * (1.0 + raw_t * 0.01) / wm), seed)
    wave_h = natural_wave(float(x), amp, g.wa * (0.001 * (1.0 + raw_u * 0.01) / wm), seed)  # sic: wave_amplitude, not frequency
    dist = (nl * 100.0 + nd * 30.0) * g.dist
    fx, fz = float(x), float(z)
    c = O()
    a = np.float64(a)
    c.a, c.m, c.rnd, c.rnd_u = a, g.mode, raw_t, raw_u
    c.mit, c.modi = 1.0 - abs(g.mit / 100.0), g.modi / 100.0
    c.adj_z = ((fz + dist) + wave_t) + off
    c.complex_z = ((fz + off) + dist) + wave_t
    c.mad_z = np.fmod(np.fmod(((fz + off) + dist) + wave_t, 4.0 * a) + 4.0 * a, 4.0 * a)
    c.jungle, c.desert2, c.zp = 0.6666666666666666 * a, 1.3333333333333333 * a, fz + off
    c.radius = (a + dist) + wave_t
    c.r, c.r_sq = math.sqrt(fx * fx + fz * fz), max(abs(fx), abs(fz))
    c.adj_x = ((fx + dist) + wave_h) + g.hoff
    c.hbw = np.float64(g.hbw)
    t = f32(temperature(g, c))
    h = O()
    h.__dict__.update(c.__dict__)
    h.desert, h.sav2, h.temp2 = 0.3333333333333333 * a, 1.6666666666666667 * a, 2.0 * a
    h.pole2, h.cold3 = 2.6666666666666665 * a, 3.3333333333333335 * a
    h.s_pole, h.n_pole = (dist + wave_t) + 3.0 * a, (dist + wave_t) - 1.0 * a
    h.radius = (a + dist) + wave_t
    h.t = t
    return t, f32(humidity(g, h))


# ---- the golden file ----
def parse(tok):
    it = iter(tok)
    nx = lambda: float(next(it))
    g = O()
    g.mode = int(next(it))
    g.a, g.off, g.wm, g.wf, g.wa = nx(), nx(), nx(), nx(), nx()
    g.looping, g.dist, g.modi, g.mit = int(next(it)) == 1, int(next(it)), nx(), nx()
    g.tz, g.tp, g.tvar = [nx() for _ in range(12)], [nx() for _ in range(5)], [nx() for _ in range(12)]
    g.hbands, g.hbw = int(next(it)) == 1, nx()
    g.hz, g.ht, g.uvar = [nx() for _ in range(12)], [nx() for _ in range(5)], [nx() for _ in range(12)]
    g.extra = int(next(it)) == 1
    g.cov = [nx() for _ in range(12)]
    g.hoff = nx()
    seed, x, _y, z = int(next(it)), int(next(it)), int(next(it)), int(next(it))
    rt, ru, T, H = nx(), nx(), nx(), nx()
    return g, seed & M64, x, z, rt, ru, T, H


def main(path):
    np.seterr(all="ignore")
    bad = n = 0
    worst = 0.0
    for line in open(path):
        g, seed, x, z, rt, ru, T, H = parse(line.split())
        t, h = climate(g, x, z, seed, rt, ru)
        n += 1
        e = max(abs(t - T), abs(h - H))
        worst = max(worst, e)
        if not e <= 2e-5:  # NaN fails too
            bad += 1
            if bad <= 8:
                print("MISMATCH mode %d a=%g looping=%s hb=%s: T %.7f vs %.7f  H %.7f vs %.7f" % (g.mode, g.a, g.looping, g.hbands, t, T, h, H))
    print("%d cases, %d mismatches, worst %.2e" % (n, bad, worst))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
