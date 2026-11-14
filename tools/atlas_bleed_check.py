# Emulates the chunk shader's atlas read (bilinear taps, trilinear mips, Repeat) on the real terrain.rgba, before and after the
# per-tile clamp of `atlas_sample` (mc-rs/src/gpu/pipeline.rs). Run from anywhere: python3 tools/atlas_bleed_check.py (needs numpy)
# per-tile clamp.  -> prints the worst colour error of an edge sample against the tile's own edge texel.
import numpy as np, math, pathlib
A = np.fromfile(pathlib.Path(__file__).resolve().parents[1] / 'mc-rs/assets/terrain.rgba', dtype=np.uint8).reshape(256, 256, 4).astype(np.float64)
MIPS = 3
def mip(a):  # = atlas::mip (alpha-weighted colour, averaged alpha)
    h, w = a.shape[:2]; o = np.zeros((h // 2, w // 2, 4))
    for y in range(h // 2):
        for x in range(w // 2):
            p = a[2*y:2*y+2, 2*x:2*x+2].reshape(4, 4); al = p[:, 3].sum()
            o[y, x, :3] = (p[:, :3] * p[:, 3:4]).sum(0) / max(al, 1); o[y, x, 3] = al / 4
    return o
L = [A]
for _ in range(MIPS - 1): L.append(mip(L[-1]))
def bilinear(img, px, py):  # px,py in this level's texels, Repeat
    h, w = img.shape[:2]; x = px - .5; y = py - .5
    x0, y0 = math.floor(x), math.floor(y); fx, fy = x - x0, y - y0
    g = lambda i, j: img[j % h, i % w]
    return (g(x0, y0) * (1-fx) + g(x0+1, y0) * fx) * (1-fy) + (g(x0, y0+1) * (1-fx) + g(x0+1, y0+1) * fx) * fy
def sample(u, v, lod):  # u,v in 0..1 over the 256x256 png; mag = Nearest at lod <= 0, else linear + linear mips
    if lod <= 0: return A[int(v * 256) % 256, int(u * 256) % 256]
    l = min(lod, MIPS - 1); l0 = int(l); l1 = min(l0 + 1, MIPS - 1); f = l - l0
    s = lambda k: bilinear(L[k], u * L[k].shape[1], v * L[k].shape[0])
    return s(l0) * (1 - f) + s(l1) * f
def old_uv(t, fu, fv): return (((t % 16) * 16 + .02 + fu * 15.96) / 256, ((t // 16) * 16 + .02 + (1 - fv) * 15.96) / 256)
def new_uv(t, fu, fv, lod):  # the shader's clamp: keep the bilinear footprint of the coarsest mip inside the tile
    px, py = old_uv(t, fu, fv); px *= 256; py *= 256
    half = .5 * 2.0 ** min(math.ceil(max(lod, 0)), MIPS - 1)
    x0, y0 = (t % 16) * 16, (t // 16) * 16
    return (min(max(px, x0 + half), x0 + 16 - half) / 256, min(max(py, y0 + half), y0 + 16 - half) / 256)
def err(uvf, tiles):
    worst = {}
    for t in tiles:
        tile = A[(t//16)*16:(t//16)*16+16, (t%16)*16:(t%16)*16+16]
        if (tile[..., 3] < 255).any(): continue            # opaque tiles only: the reference is the tile's own colours
        lo, hi = tile[..., :3].min((0, 1)), tile[..., :3].max((0, 1))
        for lod in (0.3, 0.8, 1.5, 2.5):
            e = 0
            for fu in np.linspace(0, 1, 17):
                for fv in np.linspace(0, 1, 17):
                    if min(fu, fv, 1 - fu, 1 - fv) > 0.04: continue   # edge samples only
                    u, v = uvf(t, fu, fv, lod); c = sample(u, v, lod)[:3]
                    e = max(e, float(np.maximum(np.maximum(lo - c, c - hi), 0).max()))  # distance outside the tile's own colour range
            worst[(t, lod)] = e
    return worst
tiles = [0, 1, 2, 3, 4, 16, 17, 18, 19, 20, 21, 52]
old = err(lambda t, a, b, l: old_uv(t, a, b), tiles); new = err(new_uv, tiles)
print('worst colour error outside the tile\'s own range (0..255):')
print('  tile  lod   old    new')
for k in sorted(old): print(f'  {k[0]:4d} {k[1]:4.1f} {old[k]:6.1f} {new[k]:6.1f}')
print('MAX old', max(old.values()), 'new', max(new.values()))
assert max(new.values()) < 1.0 <= max(old.values()), 'fix does not hold'
print('OK')
