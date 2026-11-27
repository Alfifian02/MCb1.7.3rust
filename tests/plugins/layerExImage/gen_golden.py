#!/usr/bin/env python3
"""gen_golden.py - membangkitkan golden.inc: hash keluaran FUNGSI ASLI libgame.so 1.3.9 (dijalankan di emulator unicorn)
untuk gambar masukan deterministik. Dipakai test_core.cpp sebagai ekspektasi dari binary (bukan dari kode fork).
Pakai:  LIBGAME=~/k2_out/libgame.so python3 tests/plugins/layerExImage/gen_golden.py > tests/plugins/layerExImage/golden.inc
Perlu:  pip install unicorn   (script/k2emu_unicorn.py)"""
import sys, os, struct, math
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..', 'script'))
from k2emu_unicorn import *

M32 = 0xFFFFFFFF
class Xs:                       # xorshift32; harus sama dengan test_core.cpp
    def __init__(s, seed): s.x = seed & M32 or 1
    def next(s):
        x = s.x; x ^= (x << 13) & M32; x ^= x >> 17; x ^= (x << 5) & M32; s.x = x; return x
def make_img(xs, w, h): return bytes((xs.next() >> 24) & 0xff for _ in range(w * h * 4))
def fnv(b):
    h = 0xcbf29ce484222325
    for c in b: h = ((h ^ c) * 0x100000001b3) & 0xFFFFFFFFFFFFFFFF
    return h

STUB = 0x73000000
e = Emu()
def new_obj(w, h, img):
    e.heap = 0x72000000; e.stubs = {}
    try: e.uc.mem_map(STUB, 0x1000)
    except Exception: pass
    buf = e.alloc(w * h * 4 + 64); e.w(buf, img)
    vt = e.alloc(32); e.w64(vt, STUB); e.w64(vt + 8, STUB + 4)
    ob = e.alloc(0x40); e.w64(ob, vt); e.w64(ob + 8, 0xdead0000)
    e.w32(ob + 0x10, w); e.w32(ob + 0x14, h); e.w64(ob + 0x18, buf); e.w32(ob + 0x20, w * 4)
    e.stubs[0x848bdc] = lambda m: 1
    e.stubs[STUB + 4] = lambda m: 0
    e.stubs[0x41f9e0] = lambda m: m.alloc(m.R(UC_ARM64_REG_X0) + 16)
    e.stubs[0x426f20] = lambda m: 0
    def ex(m):
        d = struct.unpack('<d', struct.pack('<Q', m.R(UC_ARM64_REG_D0)))[0]
        m.uc.reg_write(UC_ARM64_REG_D0, struct.unpack('<Q', struct.pack('<d', math.exp(d)))[0])
    e.stubs[0x41a120] = ex
    return ob, buf
def fb(f): return struct.unpack('<I', struct.pack('<f', f))[0]

K_LIGHT, K_COLORIZE, K_MODULATE, K_NOISE, K_WHITE, K_GAUSS = range(6)
cases = []   # (kind, w, h, seed, a(double), b, c)
for br, co in ((10, 0), (0, 10), (-50, 30), (200, -100), (-300, 500), (127, -99)): cases.append((K_LIGHT, 9, 5, 11, 0.0, br, co))
for bl, hu, sa in ((1.0, 128, 255), (0.5, 10, 200), (0.999, 77, 77), (0.9995, 200, 10), (0.2, 85, 128), (0.75, 256, 257)): cases.append((K_COLORIZE, 9, 5, 12, bl, hu, sa))
for hu, sa, lu in ((10, 0, 0), (0, 50, 0), (0, 0, 50), (-90, -50, -50), (180, 100, 100), (720, 30, 60), (45, 300, -300)): cases.append((K_MODULATE, 9, 5, 13, float(hu), sa, lu))
for w, h in ((1, 1), (3, 2), (4, 2), (7, 4), (11, 3)):
    for lv in (10, 255, -30): cases.append((K_NOISE, w, h, 14, 0.0, lv, 0))
for w, h in ((1, 1), (5, 3), (8, 4)): cases.append((K_WHITE, w, h, 15, 0.0, 0, 0))
for sg in (0.0, 1.0, 2.0, 3.0, 7.5):
    for w, h in ((1, 1), (5, 4), (16, 5), (24, 12)): cases.append((K_GAUSS, w, h, 16, sg, 0, 0))

print('// dibangkitkan oleh gen_golden.py dari libgame.so 1.3.9 (emulasi fungsi asli). JANGAN edit manual.')
print('// {jenis, w, h, seed, a, b, c, fnv1a64(keluaran)}   (modulate: a = hue, b = sat, c = lum)')
for case in cases:
    kind, w, h, seed, a, b, c = case
    xs = Xs(seed); img = make_img(xs, w, h)
    ob, buf = new_obj(w, h, img)
    if kind == K_LIGHT:   e.call(0x5920b4, [ob, b & M32, c & M32])
    elif kind == K_COLORIZE: e.call(0x5924d8, [ob, b & M32, c & M32], d=[a])
    elif kind == K_MODULATE: e.call(0x592cf0, [ob, int(a) & M32, b & M32, c & M32])
    elif kind == K_NOISE:
        words = [xs.next() for _ in range(4)]; rs = [xs.next() & 0x7fffffff for _ in range(2 * w * h + 4)]; it = iter(rs)
        e.stubs[0xa28f38] = lambda m, words=words: m.w(m.R(UC_ARM64_REG_X0), struct.pack('<4I', *words))
        e.stubs[0x416e40] = lambda m, it=it: next(it)
        e.call(0x593464, [ob, b & M32])
    elif kind == K_WHITE:
        rs = [xs.next() & 0x7fffffff for _ in range(w * h + 4)]; it = iter(rs)
        e.stubs[0x416e40] = lambda m, it=it: next(it)
        e.call(0x593b94, [ob])
    else:
        e.uc.reg_write(UC_ARM64_REG_Q0, fb(a)); e.call(0x593e60, [ob])
    out = e.r(buf, w * h * 4)
    print('{%d, %d, %d, %du, %r, %d, %d, 0x%016xull},' % (kind, w, h, seed, a, b, c, fnv(out)))
