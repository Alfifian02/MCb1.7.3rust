#!/usr/bin/env python3
"""Python reference of mc-rs `world/gen/hydro.rs` (a port of the Fabric mod Streams Reflowing 2.15.2).

Every constant marked `mod:` was read from the mod's class files (bytecode), not guessed. `python3 tools/hydro_proto.py`
routes a synthetic terrain, prints the stream statistics and writes /tmp/hydro_proto.png.
"""
import heapq, math, sys
import numpy as np

M64 = (1 << 64) - 1
K1, K2, K3, K4 = 0x9E3779B97F4A7C15, 0xD1B54A32D192ED03, 0xA0761D6478BD642F, 0xE7037ED1A0B428DB  # mod: core.math.Hashing
C1, C2 = 0xBF58476D1CE4E5B9, 0x94D049BB133111EB

def mix(x):
    x &= M64
    x ^= x >> 30; x = (x * C1) & M64
    x ^= x >> 27; x = (x * C2) & M64
    x ^= x >> 31
    return x

def hash3(a, b, c):
    h = (a * K1) & M64
    h = mix(h ^ ((b * K2) & M64))
    h = mix(h ^ ((c * K3) & M64))
    return mix(h)

def hash4(a, b, c, d):
    return mix(hash3(a, b, c) ^ ((d * K4) & M64))

def unit3(a, b, c): return (hash3(a, b, c) >> 11) * 1.1102230246251565e-16
def unit4(a, b, c, d): return (hash4(a, b, c, d) >> 11) * 1.1102230246251565e-16

# mod: StreamsConfig.defaults()
SEA = 64            # mod seaLevel 63 in a 1.18 world = this world's SEA_LEVEL
PLATE = 768.0       # WatershedSize.DEFAULT
JITTER = 0.45       # plateJitter
SPACING = 16        # QualityPreset.MEDIUM
MIN_ACC = 6.0; GROW_DENSITY = 0.75
BASE_W, MAX_W, WIDTH_SCALE = 0.4, 16.0, 1.0
STREAM_DEPTH = 0.24; BANK_WIDTH = 2.0; DITCH_INSET = 3.0; A_RIVER = 1.0
SMOOTH_PASSES = 3; MEANDER = 1.2
WET_CUT, WET_REF, WET_FLOOR = 0.2, 0.7, 0.1

def seed_xz(seed, i, j):
    j_ = min(0.49, max(0.0, JITTER))
    sx = (i + 0.5 + (unit4(seed, i & M64, j & M64, 1) - 0.5) * 2 * j_) * PLATE
    sz = (j + 0.5 + (unit4(seed, i & M64, j & M64, 2) - 0.5) * 2 * j_) * PLATE
    return sx, sz

def plate_at(seed, x, z):
    ci, cj = math.floor(x / PLATE), math.floor(z / PLATE)
    best = None
    for dj in (-1, 0, 1):
        for di in (-1, 0, 1):
            i, j = ci + di, cj + dj
            sx, sz = seed_xz(seed, i, j)
            d = (sx - x) ** 2 + (sz - z) ** 2
            if best is None or d < best[0] or (d == best[0] and (i, j) < best[1]):
                best = (d, (i, j))
    return best[1]

NB = [(-1,-1),(-1,0),(-1,1),(0,-1),(0,1),(1,-1),(1,0),(1,1)]

def width_of(acc):  # mod: NetworkTracer.b(acc, cfg)
    area = acc * SPACING * SPACING
    return min(MAX_W, BASE_W + WIDTH_SCALE * math.sqrt(area / 2500.0))

def chaikin(pts, passes):  # mod: NetworkTracer.a(List, int)
    for _ in range(passes):
        if len(pts) < 3: break
        out = [pts[0]]
        for a, b in zip(pts, pts[1:]):
            out.append(tuple(a[k] + (b[k] - a[k]) * 0.25 for k in range(4)))
            out.append(tuple(a[k] + (b[k] - a[k]) * 0.75 for k in range(4)))
        out.append(pts[-1]); pts = out
    return pts

def meander(pts, seed, cid):  # mod: NetworkTracer.a(List, long, int, cfg)
    n = len(pts)
    if n < 3: return pts
    amp = SPACING * MEANDER
    p1 = unit3(seed, cid, 11) * math.pi * 2
    p2 = unit3(seed, cid, 12) * math.pi * 2
    p3 = unit3(seed, cid, 13) * math.pi * 2
    cum = [0.0] * n
    for i in range(1, n):
        cum[i] = cum[i-1] + math.hypot(pts[i][0]-pts[i-1][0], pts[i][1]-pts[i-1][1])
    out = [pts[0]]
    for i in range(1, n - 1):
        a, c, b = pts[i-1], pts[i], pts[i+1]
        dx, dz = b[0]-a[0], b[1]-a[1]; ln = math.hypot(dx, dz)
        if ln < 1e-6: out.append(c); continue
        nx, nz = -dz / ln, dx / ln
        taper = math.sin(math.pi * i / (n - 1))
        slope = abs(a[2] - b[2]) / ln
        flat = max(0.0, 1.0 - slope / 1.0)
        off = amp * taper * flat * (0.5 * math.sin(cum[i]*0.03 + p1) + 0.3 * math.sin(cum[i]*0.07 + p2) + 0.2 * math.sin(cum[i]*0.017 + p3))
        out.append((c[0] + nx * off, c[1] + nz * off, c[2], c[3]))
    out.append(pts[-1])
    return out


TAIL_CAP = 6000

def trace_tail(probe, gi, gj, sea_h):
    """Best-first walk over the real terrain from (gi, gj) to the first sea cell, lowest pass first; [(gi, gj, y)] or None."""
    y0 = probe(gi, gj)[0]
    heap = [(y0, gi, gj)]; parent = {(gi, gj): None}; level = {(gi, gj): y0}; n = 0
    while heap and n < TAIL_CAP:
        lv, a, b = heapq.heappop(heap); n += 1
        if probe(a, b)[0] < sea_h:
            path = []; c = (a, b)
            while c is not None: path.append((c[0], c[1], level[c])); c = parent[c]
            return path[::-1]
        for da, db in NB:
            c = (a + da, b + db)
            if c in parent: continue
            parent[c] = (a, b); level[c] = max(probe(*c)[0], 0.0) if False else min(lv, probe(*c)[0]) if False else lv if probe(*c)[0] <= lv else probe(*c)[0]
            heapq.heappush(heap, (level[c], c[0], c[1]))
    return None

def separate(stream, recv, order, nx, nz):
    """mod: build() stage 4b6 / 4b7. Two stream systems (trees that drain to different mouths) must not touch: the smaller one
    yields its contact cell, unless one of the two cells is a mouth; the cells it cut off from the sea are pruned (re-prune)."""
    stream = stream.copy()
    n = nx * nz
    system = -np.ones(n, int)
    for (i, j) in order:  # low -> high: an outlet-side cell is labelled before the cells that drain into it
        k = i * nz + j
        if not stream[k]: continue
        r = recv[k]
        system[k] = system[r] if (r >= 0 and stream[r]) else k
    size = {}
    for k in np.nonzero(stream)[0]: size[system[k]] = size.get(system[k], 0) + 1
    mouth = lambda k: recv[k] < 0 or not stream[recv[k]]
    drop = np.zeros(n, bool)
    for k in np.nonzero(stream)[0]:
        i, j = divmod(k, nz)
        for a, b in NB:
            p, q = i + a, j + b
            if not (0 <= p < nx and 0 <= q < nz): continue
            m = p * nz + q
            if not stream[m] or system[m] == system[k]: continue
            if mouth(k) or mouth(m): continue
            if (size[system[k]], system[k]) < (size[system[m]], system[m]): drop[k] = True
    return stream & ~drop

def build(seed, key, probe, halo=10):
    """Route one plate. probe(gi, gj) -> (height, wetness) at world (12 gi, 12 gj)."""
    cx, cz = seed_xz(seed, *key)
    g0i, g1i = math.floor((cx - PLATE) / SPACING) - halo, math.ceil((cx + PLATE) / SPACING) + halo
    g0j, g1j = math.floor((cz - PLATE) / SPACING) - halo, math.ceil((cz + PLATE) / SPACING) + halo
    nx, nz = g1i - g0i + 1, g1j - g0j + 1
    h = np.zeros((nx, nz)); wet = np.zeros((nx, nz)); inside = np.zeros((nx, nz), bool)
    for i in range(nx):
        for j in range(nz):
            gi, gj = g0i + i, g0j + j
            h[i, j], wet[i, j] = probe(gi, gj)
            wx, wz = gi * SPACING, gj * SPACING
            # the plate's own cell plus a margin of `halo` cells round it (flow carries on into the neighbour for a while)
            inside[i, j] = True
    own = np.zeros((nx, nz), bool)
    for i in range(nx):
        for j in range(nz):
            own[i, j] = plate_at(seed, (g0i+i)*SPACING, (g0j+j)*SPACING) == key
    # margin: cells within `halo` cells (Chebyshev) of an owned cell
    from scipy.ndimage import binary_dilation
    dom = binary_dilation(own, structure=np.ones((3, 3), bool), iterations=halo)
    sea_h = SEA - 1.0
    fill = np.full((nx, nz), np.inf); recv = -np.ones(nx * nz, int)
    order = []; heap = []
    done = np.zeros((nx, nz), bool)
    for i in range(nx):
        for j in range(nz):
            if not dom[i, j]: continue
            edge = any(not (0 <= i+a < nx and 0 <= j+b < nz and dom[i+a, j+b]) for a, b in NB)
            if h[i, j] < sea_h or edge:
                fill[i, j] = h[i, j]; heapq.heappush(heap, (h[i, j], i, j))
    while heap:
        f, i, j = heapq.heappop(heap)
        if done[i, j]: continue
        done[i, j] = True; order.append((i, j))
        for a, b in NB:
            p, q = i + a, j + b
            if not (0 <= p < nx and 0 <= q < nz) or not dom[p, q] or np.isfinite(fill[p, q]): continue
            fill[p, q] = max(h[p, q], fill[i, j] + 0.001)
            heapq.heappush(heap, (fill[p, q], p, q))
    # D8 receivers on the filled surface
    for (i, j) in order:
        if h[i, j] < sea_h: continue
        best, bs = -1, 0.0
        for a, b in NB:
            p, q = i + a, j + b
            if not (0 <= p < nx and 0 <= q < nz) or not dom[p, q] or not np.isfinite(fill[p, q]): continue
            d = 1.4142135623730951 if a and b else 1.0
            s = (fill[i, j] - fill[p, q]) / d
            if s > bs: bs, best = s, p * nz + q
        recv[i * nz + j] = best
    acc = np.where(dom, 1.0, 0.0).ravel()
    for (i, j) in reversed(order):
        r = recv[i * nz + j]
        if r >= 0: acc[r] += acc[i * nz + j]
    thr = max(1.0, MIN_ACC * GROW_DENSITY) * (16.0 / SPACING) ** 2
    stream = np.zeros(nx * nz, bool)
    for (i, j) in order:
        k = i * nz + j
        stream[k] = fill[i, j] > SEA - 0.5 and acc[k] >= thr and h[i, j] >= sea_h and recv[k] >= -1
    # wetness pruning of headwaters
    has_up = np.zeros(nx * nz, bool)
    for (i, j) in order:
        k = i * nz + j
        if stream[k] and recv[k] >= 0 and stream[recv[k]]: has_up[recv[k]] = True
    keep = np.zeros(nx * nz, bool)
    plate_seed = hash3(seed, key[0] & M64, key[1] & M64)
    for (i, j) in order[::-1][::-1]:
        k = i * nz + j
        if not stream[k] or has_up[k]: continue
        p = max(WET_FLOOR, min(1.0, (wet[i, j] - WET_CUT) / (WET_REF - WET_CUT)))
        if unit3(plate_seed, k, 71) < p: keep[k] = True
    for (i, j) in reversed(order):  # high -> low: a kept cell keeps its receiver
        k = i * nz + j
        if keep[k] and recv[k] >= 0 and stream[recv[k]]: keep[recv[k]] = True
    stream &= keep
    stream = separate(stream, recv, order, nx, nz)
    # Every river must drain to the sea (mod: 4b5 CULL "drainage reaches no water"). A river that leaves the window is traced on over
    # the real terrain (best-first flood, lowest pass first); one that finds no sea within the cap is culled with everything upstream.
    memo = {}
    def pr(gi, gj):
        if (gi, gj) not in memo: memo[(gi, gj)] = probe(gi, gj)
        return memo[(gi, gj)]
    tails = {}
    for k in np.nonzero(stream)[0]:
        if recv[k] < 0:
            i, j = divmod(k, nz)
            tails[k] = trace_tail(pr, g0i + i, g0j + j, sea_h)
    alive = np.zeros(nx * nz, bool)
    for (i, j) in order:
        k = i * nz + j
        if not stream[k]: continue
        r = recv[k]
        if r < 0: alive[k] = tails[k] is not None
        elif stream[r]: alive[k] = alive[r]
        else: alive[k] = h[r // nz, r % nz] < sea_h
    stream &= alive
    # chains: start at a headwater or a confluence, end at the next confluence / the sea / the margin
    inflow = np.zeros(nx * nz, int)
    for (i, j) in order:
        k = i * nz + j
        if stream[k] and recv[k] >= 0 and stream[recv[k]]: inflow[recv[k]] += 1
    chains = []
    for (i, j) in reversed(order):
        k = i * nz + j
        if not stream[k]: continue
        if inflow[k] == 1: continue  # in the middle of a chain
        path = [k]
        while True:
            r = recv[path[-1]]
            if r < 0: break
            path.append(r)
            if not stream[r] or inflow[r] != 1: break
        chains.append(path)
    segs = []
    for cid, path in enumerate(chains):
        pts = []
        for k in path:
            i, j = divmod(k, nz)
            y = fill[i, j] if stream[k] else max(SEA - 1.0, fill[i, j])
            pts.append(((g0i + i) * SPACING, (g0j + j) * SPACING, y, acc[k] if stream[k] else acc[path[-2]]))
        # grade never rises downstream
        for t in range(1, len(pts)):
            if pts[t][2] > pts[t-1][2]: pts[t] = (pts[t][0], pts[t][1], pts[t-1][2], pts[t][3])
        t = tails.get(path[-1])
        if t:
            ly = pts[-1][2]; la = pts[-1][3]
            for (gi, gj, ty) in t[1:]:
                ly = min(ly, ty if ty >= sea_h else sea_h); pts.append((gi * SPACING, gj * SPACING, ly, la))
        pts = chaikin(pts, SMOOTH_PASSES)
        pts = meander(pts, plate_seed, cid)
        segs.append(pts)
    return dict(sea_h=sea_h, h=h, fill=fill, stream=stream.reshape(nx, nz), acc=acc.reshape(nx, nz), g0=(g0i, g0j), chains=segs, own=own)

# ------------------------------------------------------------------------------------------------------------------------------
def synth_probe(seed):
    rng = np.random.default_rng(seed)
    from scipy.ndimage import gaussian_filter
    N = 1024
    f = sum(gaussian_filter(rng.standard_normal((N, N)), s, mode='wrap') * s * a for s, a in ((64, 1.0), (24, .5), (8, .2)))
    f = (f - f.mean()) / f.std()
    def probe(gi, gj):
        x, z = gi % N, gj % N
        return 70 + 14 * f[x, z] + 8 * math.sin(gi / 40), 0.55 + 0.35 * math.sin(gj / 55)
    return probe

if __name__ == '__main__':
    seed = 0xCAFEBABE
    probe = synth_probe(1)
    key = plate_at(seed, 0, 0)
    print('plate', key, seed_xz(seed, *key))
    r = build(seed, key, probe)
    st = r['stream']; land = (r['h'] >= SEA - 1) & r['own']
    print('land cells %d, stream cells %d (%.1f%% of land), chains %d' % (land.sum(), (st & r['own']).sum(), 100 * (st & r['own']).sum() / max(1, land.sum()), len(r['chains'])))
    ws = [width_of(r['acc'][np.unravel_index(0, r['acc'].shape)])]
    wid = np.array([width_of(a) for a in r['acc'][st]])
    print('half widths: min %.2f median %.2f max %.2f' % (wid.min(), np.median(wid), wid.max()))
    import matplotlib; matplotlib.use('Agg'); import matplotlib.pyplot as plt
    fig, ax = plt.subplots(figsize=(9, 9))
    ax.imshow(r['h'].T, origin='lower', cmap='terrain', vmin=40, vmax=110)
    for pts in r['chains']:
        xs = [(p[0] / SPACING - r['g0'][0]) for p in pts]; zs = [(p[1] / SPACING - r['g0'][1]) for p in pts]
        ax.plot(xs, zs, 'b-', lw=0.6 + 0.35 * width_of(pts[-1][3]))
    ax.set_title('hydro_proto: plate %s' % (key,)); fig.savefig('/tmp/hydro_proto.png', dpi=80)
