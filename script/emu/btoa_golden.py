#!/usr/bin/env python3
# btoa_golden.py - membuat tests/plugins/layerExBTOA/golden_emu.inc dengan MENJALANKAN fungsi asli libgame.so (Unicorn).
# Pakai: K2_LIBGAME=/jalur/libgame.so python3 script/emu/btoa_golden.py > tests/plugins/layerExBTOA/golden_emu.inc
# Masukan uji dibangkitkan dari PRNG deterministik yang sama dengan test_core.cpp; yang disimpan hanya hash FNV-1a 64
# dari seluruh buffer keluaran (termasuk padding pitch), rect update, dan kode kembali.
import os, sys, random, struct
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from harness import Harness

M = (1 << 64) - 1
class Rng:
    def __init__(s, seed): s.s = seed & M
    def byte(s):
        s.s = (s.s * 6364136223846793005 + 1442695040888963407) & M
        return (s.s >> 56) & 0xff
    def bytes(s, n): return bytes(s.byte() for _ in range(n))
def fnv(b):
    h = 0xcbf29ce484222325
    for c in b: h = ((h ^ c) * 0x100000001b3) & M
    return h

A_RIGHT, A_BOTTOM, A_PROV, A_FILL, A_CLIP = 0x5e16e8, 0x5e1b58, 0x5e1f74, 0x5e31a8, 0x5e2544
h = Harness()
rnd = random.Random(20261122)
out = []
def emit(s): out.append(s)

emit('// DIBUAT oleh script/emu/btoa_golden.py dari eksekusi fungsi asli libgame.so 1.3.9 (arm64) di Unicorn. Jangan diedit.')
emit('// Bukan hasil dari kode fork: ini ekspektasi yang diturunkan dari biner. Regenerasi: lihat kepala skrip.')

# ---------------- copyRight / copyBottom ----------------
def gen_copy(addr, name):
    emit('static const GCopy %s[] = {' % name)
    n = 0
    cases = [(w, hh) for w in (1, 2, 3, 4, 5, 6, 7, 8, 9, 33, 64) for hh in (0, 1, 2, 3, 4, 5, 9)]
    for (w, hh) in cases:
        for pad in (0, 12):
            seed = rnd.getrandbits(32); r = Rng(seed)
            pitch = w * 4 + pad; data = r.bytes(pitch * hh)
            h.reset_case(); lay = h.mk_layer(w, max(hh, 0), data if hh > 0 else b'\0' * 4, None, None)
            if hh == 0:
                # tekstur 0 baris: buat manual agar tinggi = 0
                m = h.u64(lay['L'] + 0x118); t = h.reg[m]['tex']; h.p32(t + 0x10, 0)
                h.reg[t]['h'] = 0; h.reg[t]['pitch'] = pitch
            else:
                m = h.u64(lay['L'] + 0x118); t = h.reg[m]['tex']; h.reg[t]['pitch'] = pitch
            ret = h.call(addr, lay, [])
            assert ret == 0 and h.thrown is None and not h.stackfail and not h.oob, (hex(addr), w, hh, ret, h.thrown, h.oob)
            m = h.u64(lay['L'] + 0x118); t = h.reg[m]['tex']; d = h.reg[t]
            res = h.r(d['base'], pitch * hh) if hh > 0 else b''
            assert len(h.updates) == 1
            ul, ut, ur, ub = h.updates[0]
            emit('\t{%du, %d, %d, %d, 0x%016xULL, %d, %d, %d, %d},' % (seed, w, hh, pad, fnv(res), ul, ut, ur, ub)); n += 1
    emit('};'); return n
n1 = gen_copy(A_RIGHT, 'kGoldRight'); n2 = gen_copy(A_BOTTOM, 'kGoldBottom')

# ---------------- copyAlphaToProvince ----------------
emit('static const GProv kGoldProv[] = {')
n3 = 0
widths = (1, 2, 3, 31, 32, 33, 37, 63, 64, 65, 70, 100)
levels = [None, 'void', -5, -1, 0, 1, 77, 128, 255, 256, 1000]
for w in widths:
    for hh in (1, 2, 3):
        for lv in levels:
            for kind in range(3):
                if kind == 0: clip = (0, 0, w, hh)
                elif kind == 1: clip = (min(1, w - 1) if w > 1 else 0, 0, w, hh)
                else: clip = (0, min(1, hh - 1), w, hh)
                if w <= 1 and kind == 1: continue
                padm, padp = rnd.choice((0, 8)), rnd.choice((0, 5))
                seed = rnd.getrandbits(32); r = Rng(seed)
                mp, pp = w * 4 + padm, w + padp
                main = r.bytes(mp * hh); prov = r.bytes(pp * hh)
                h.reset_case(); lay = h.mk_layer(w, hh, main, prov, clip, ppitch=pp)
                m = h.u64(lay['L'] + 0x118); t = h.reg[m]['tex']; h.reg[t]['pitch'] = mp
                params = [] if lv is None else ([h.variant_void()] if lv == 'void' else [h.variant_int(lv)])
                ret = h.call(A_PROV, lay, params)
                assert ret == 0 and h.thrown is None and not h.stackfail and not h.oob, (w, hh, lv, clip, h.thrown, h.oob)
                assert len(h.updates) == 1
                ul, ut, ur, ub = h.updates[0]
                lvc = 0 if lv is None else (1 if lv == 'void' else 2)
                lvv = 0 if lvc != 2 else lv
                emit('\t{%du, %d, %d, %d, %d, %d,%d,%d,%d, %d, %d, 0x%016xULL, 0x%016xULL, %d,%d,%d,%d},' % (
                    seed, w, hh, padm, padp, clip[0], clip[1], clip[2], clip[3], lvc, lvv, fnv(h.texdata(lay)), fnv(h.provdata(lay)), ul, ut, ur, ub)); n3 += 1
emit('};')

# ---------------- fillByProvince ----------------
emit('static const GFill kGoldFill[] = {')
n4 = 0
for w in (1, 2, 3, 5, 8, 17, 40):
    for hh in (1, 2, 3, 4):
        for extra_rows, ctop in ((0, 0), (3, 0), (3, 2), (3, 3)):
            for pv in (0, 1, 2, 3, 259, -1):
                seed = rnd.getrandbits(32); r = Rng(seed)
                padm, padp = rnd.choice((0, 8)), rnd.choice((0, 5))
                provH = hh + extra_rows
                mp, pp = w * 4 + padm, w + padp
                main = r.bytes(mp * hh); prov = bytes(b & 3 for b in r.bytes(pp * provH))
                color = r.byte() | r.byte() << 8 | r.byte() << 16 | r.byte() << 24
                clip = (0, ctop, w, ctop + hh)          # kiri = 0 (lihat spec: kiri != 0 membaca di luar province)
                h.reset_case(); lay = h.mk_layer(w, hh, main, prov, clip, ppitch=pp)
                # province lebih tinggi dari layer pada uji ini (hanya agar offset clip-top terbaca di dalam buffer)
                pm = h.u64(lay['L'] + 0x128); pt = h.reg[pm]['tex']; h.p32(pt + 0x10, provH); h.reg[pt]['h'] = provH
                m = h.u64(lay['L'] + 0x118); t = h.reg[m]['tex']; h.reg[t]['pitch'] = mp
                ret = h.call(A_FILL, lay, [h.variant_int(pv), h.variant_int(color)])
                assert ret == 0 and h.thrown is None and not h.stackfail and not h.oob, (w, hh, pv, clip, h.thrown, h.oob)
                assert len(h.calls) == 1 and h.calls[0][0] == 'update'
                vals = [v for (_, v) in h.calls[0][1]]
                emit('\t{%du, %d, %d, %d, %d, %d, %d, %d, %d, 0x%08xU, 0x%016xULL, %d,%d,%d,%d},' % (
                    seed, w, hh, provH, padm, padp, ctop, pv, 0, color, fnv(h.texdata(lay)), vals[0], vals[1], vals[2], vals[3])); n4 += 1
emit('};')

# ---------------- clipAlphaRect ----------------
emit('static const GClip kGoldClip[] = {')
n5 = 0; codes = {}
for it in range(1800):
    dw, dh = rnd.randint(1, 16), rnd.randint(1, 12)
    sw, sh = rnd.randint(1, 12), rnd.randint(1, 9)
    dx, dy = rnd.randint(-14, 18), rnd.randint(-12, 14)
    sx, sy = rnd.randint(-7, 14), rnd.randint(-6, 11)
    w, hh = rnd.randint(-1, 16), rnd.randint(-1, 12)
    if rnd.random() < 0.05: dx, sx = rnd.choice((0, 1, dw)), rnd.choice((0, 1))
    fk = rnd.choice((0, 1, 2, 2, 2, 3))     # 0 tanpa param ke-8, 1 void, 2 nilai 0..255, 3 nilai di luar byte
    val = rnd.randint(0, 255) if fk == 2 else (rnd.choice((256, 300, -1, 1 << 40)) if fk == 3 else 0)
    padd, pads = rnd.choice((0, 8)), rnd.choice((0, 4))
    seed = rnd.getrandbits(32); r = Rng(seed)
    dp, sp = dw * 4 + padd, sw * 4 + pads
    dd = r.bytes(dp * dh); sd = r.bytes(sp * sh)
    h.reset_case()
    dl = h.mk_layer(dw, dh, dd); sl = h.mk_layer(sw, sh, sd)
    m = h.u64(dl['L'] + 0x118); t = h.reg[m]['tex']; h.reg[t]['pitch'] = dp
    m = h.u64(sl['L'] + 0x118); t = h.reg[m]['tex']; h.reg[t]['pitch'] = sp
    ps = [h.variant_int(dx), h.variant_int(dy), h.variant_obj(sl['obj']), h.variant_int(sx), h.variant_int(sy), h.variant_int(w), h.variant_int(hh)]
    if fk == 1: ps.append(h.variant_void())
    elif fk >= 2: ps.append(h.variant_int(val))
    ret = h.call(A_CLIP, dl, ps)
    assert h.thrown is None and not h.stackfail and not h.oob, (it, h.thrown, h.oob)
    codes[ret] = codes.get(ret, 0) + 1
    if len(h.calls): assert len(h.calls) == 1 and h.calls[0][0] == 'update'
    upd = [v for (_, v) in h.calls[0][1]] if h.calls else [0, 0, 0, 0]
    emit('\t{%du, %d, %d, %d, %d, %d, %d, %dLL,%dLL,%dLL,%dLL,%dLL,%dLL, %d, %dLL, %d, 0x%016xULL, %d, %d,%d,%d,%d},' % (
        seed, dw, dh, sw, sh, padd, pads, dx, dy, sx, sy, w, hh, fk, val, ret, fnv(h.texdata(dl)), 1 if h.calls else 0, *upd)); n5 += 1
POOL = [-2**31, -2**31 + 1, -2**32 + 2, -100, -1, 0, 1, 2, 3, 100, 2**31 - 2, 2**31 - 1, 2**32 + 3, 2**32 + 2**31]
skipped = 0
for it in range(700):
    dw, dh = rnd.randint(1, 8), rnd.randint(1, 8); sw, sh = rnd.randint(1, 8), rnd.randint(1, 8)
    dx, dy, sx, sy, w, hh = (rnd.choice(POOL) if rnd.random() < 0.6 else rnd.randint(-6, 10) for _ in range(6))
    fk = rnd.choice((0, 1, 2, 3)); val = rnd.randint(0, 255) if fk == 2 else (rnd.choice(POOL) if fk == 3 else 0)
    padd, pads = rnd.choice((0, 8)), rnd.choice((0, 4))
    seed = rnd.getrandbits(32); r = Rng(seed)
    dp, sp = dw * 4 + padd, sw * 4 + pads
    dd = r.bytes(dp * dh); sd = r.bytes(sp * sh)
    h.reset_case()
    dl = h.mk_layer(dw, dh, dd); sl = h.mk_layer(sw, sh, sd)
    m = h.u64(dl['L'] + 0x118); t = h.reg[m]['tex']; h.reg[t]['pitch'] = dp
    m = h.u64(sl['L'] + 0x118); t = h.reg[m]['tex']; h.reg[t]['pitch'] = sp
    ps = [h.variant_int(dx), h.variant_int(dy), h.variant_obj(sl['obj']), h.variant_int(sx), h.variant_int(sy), h.variant_int(w), h.variant_int(hh)]
    if fk == 1: ps.append(h.variant_void())
    elif fk >= 2: ps.append(h.variant_int(val))
    ret = h.call(A_CLIP, dl, ps)
    if h.thrown is not None or h.stackfail or h.oob: skipped += 1; continue
    codes[ret] = codes.get(ret, 0) + 1
    upd = [v for (_, v) in h.calls[0][1]] if h.calls else [0, 0, 0, 0]
    emit('\t{%du, %d, %d, %d, %d, %d, %d, %dLL,%dLL,%dLL,%dLL,%dLL,%dLL, %d, %dLL, %d, 0x%016xULL, %d, %d,%d,%d,%d},' % (
        seed, dw, dh, sw, sh, padd, pads, dx, dy, sx, sy, w, hh, fk, val, ret, fnv(h.texdata(dl)), 1 if h.calls else 0, *upd)); n5 += 1
emit('};')
sys.stderr.write('clip ekstrem dilewati (biner di luar batas): %d\n' % skipped)
sys.stderr.write('kasus: right=%d bottom=%d prov=%d fill=%d clip=%d  ret-clip=%s\n' % (n1, n2, n3, n4, n5, codes))
print('\n'.join(out))
