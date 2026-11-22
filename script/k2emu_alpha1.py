#!/usr/bin/env python3
# k2emu_alpha1.py - harness S6 (layerExAlpha, 14 method Alpha-1) di atas k2emu.py.
#
#   python3 script/k2emu_alpha1.py RECON_ALPHA1.txt [--lib libgame.so] --golden OUT.txt [--seed N] [--trace]
#
# * --golden : tulis vektor emas CPU (masukan + keluaran hasil emulasi instruksi) untuk tests/plugins/layerExAlpha.
# * --trace  : cetak jejak perintah GPU (CompileRenderMethod/SetBlendFuncSeparate/OperateRect) per method.
# Asumsi hook (BUKAN hasil baca kode helper, kecuali yang tertulis di docs/spec/layerExAlpha_umum.md):
#   0x848bdc=TVPIsSoftwareRenderManager 0x807c00=GetMainImagePixelBufferForWrite 0x807c20=...Pitch
#   0x80c848=ApplyFont() di GetMainImage() 0x848834=TVPGetRenderManager 0x848454=CompileRenderMethod
#   0x848540=GetOrCompileRenderMethod 0x953d0c=TVPThrowExceptionMessage 0xa16ca4=TVPAddLog
import sys, random, argparse, struct, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from k2emu import CPU, Mem, parse_recon, EmuError, M32, M64, sx

METHODS = [  # nama, entri
    ('turnAlpha', 0x5d4c04), ('turnAlpha2', 0x5d4dc4), ('turnAlpha3', 0x5d4f80), ('turnOutline', 0x5d502c),
    ('AlphaColorRect', 0x5d5204), ('AlphaDivide', 0x5d5400), ('AlphaAdd', 0x5d54a8),
    ('AlphaAdd2', 0x5d5708), ('BlackForAlpha', 0x5d57cc), ('ColorAlphaAdd', 0x5d5950),
    ('ColorVectorEffectAdd', 0x5d5b8c), ('ExtraColorVectorEffectAdd', 0x5d5cdc),
    ('MaskRect', 0x5d621c), ('ExMaskRect', 0x5d6784)]
NPARAM = {'turnAlpha': 0, 'turnAlpha2': 0, 'turnAlpha3': 0, 'turnOutline': 0, 'AlphaColorRect': 3,
          'AlphaDivide': 1, 'AlphaAdd': 2, 'AlphaAdd2': 2, 'BlackForAlpha': 0, 'ColorAlphaAdd': 4,
          'ColorVectorEffectAdd': 7, 'ExtraColorVectorEffectAdd': 7}

THIS, LAYER_NI, IMG, SRC_OBJ, SRC_NI, SRC_IMG = 0x10000, 0x30000, 0x31000, 0x40000, 0x50000, 0x51000
DBUF, SBUF, GUARD = 0x100000, 0x200000, 64
H_RESET, H_REDRAW, H_COPYRECT, H_GETTEX, H_NIS, H_OPERATE, H_SETBLEND = (0xF0000000 + 8 * i for i in range(7))
JUMP_TABLE = (0x14c9b7c, {0: 0x5d6950, 1: 0x5d6c6c, 2: 0x5d6a50, 3: 0x5d6b60})   # dibaca dari libgame.so 1.3.9


class Thrown(Exception):
    pass


def u16(lib, a):
    e = a
    while lib[e:e + 2] != b'\0\0' or (e - a) % 2: e += 1
    return lib[a:e].decode('utf-16-le', 'replace')


def a8(lib, a):
    return lib[a:lib.find(b'\0', a)].decode('latin1')


def setup(W, H, pitch, buf, soft, src=None, nis_ok=True, lib=None):
    mem = Mem()
    L = {'redraw': 0, 'logs': [], 'copyrect': [], 'gpu': [], 'throw': None, 'strh': {}}
    wr = mem.wr
    wr(THIS + 0x00, 8, 0x20000); wr(THIS + 0x08, 8, LAYER_NI)
    wr(THIS + 0x10, 4, W); wr(THIS + 0x14, 4, H); wr(THIS + 0x18, 8, DBUF); wr(THIS + 0x20, 4, pitch)
    wr(THIS + 0x24, 4, 0); wr(THIS + 0x28, 4, 0); wr(THIS + 0x2c, 4, W); wr(THIS + 0x30, 4, H)   # klip = seluruh gambar
    wr(0x20000, 8, H_RESET); wr(0x20008, 8, H_REDRAW)
    wr(LAYER_NI + 0x118, 8, IMG); wr(IMG, 8, 0x32000); wr(0x32000 + 0x10, 8, H_GETTEX); wr(0x32000 + 0x28, 8, H_COPYRECT)
    wr(IMG + 0x58, 8, 0x33000)
    wr(SRC_OBJ, 8, 0x41000); wr(0x41000 + 0xc8, 8, H_NIS)
    wr(SRC_NI + 0x118, 8, SRC_IMG); wr(SRC_IMG, 8, 0x32000); wr(SRC_IMG + 0x58, 8, 0x52000)
    wr(0x1aa0bd8, 8, 0x60000); wr(0x60000, 4, 0x1234)                      # *ClassID
    wr(0x71000 + 0xa0, 8, H_OPERATE); wr(0x73000 + 0x50, 8, H_SETBLEND)
    wr(0x70000, 8, 0x71000); wr(0x72000, 8, 0x73000)
    off, tab = JUMP_TABLE
    for m, tgt in tab.items(): wr(off + 4 * m, 4, (tgt - off) & M32)
    mem.load(DBUF, buf)
    mem.guards += [(DBUF - GUARD, DBUF, 'dst-before'), (DBUF + len(buf), DBUF + len(buf) + GUARD, 'dst-after')]
    for y in range(H):                                   # byte padding antar-baris tidak boleh disentuh
        if pitch > 4 * W: mem.guards.append((DBUF + y * pitch + 4 * W, DBUF + (y + 1) * pitch, 'dst-pad'))
    sp = None
    if src is not None:
        sw_, sh_, spitch, sbuf = src
        mem.load(SBUF, sbuf)
        mem.guards += [(SBUF - GUARD, SBUF, 'src-before'), (SBUF + len(sbuf), SBUF + len(sbuf) + GUARD, 'src-after')]
        for y in range(sh_):
            if spitch > 4 * sw_: mem.guards.append((SBUF + y * spitch + 4 * sw_, SBUF + (y + 1) * spitch, 'src-pad'))
        L['sp'] = spitch
    L['dp'] = pitch

    def strof(a):
        if lib is None: return '@0x%x' % a
        try: return u16(lib, a)
        except Exception: return '@0x%x' % a

    def r4(a, n): return [sx(mem.rd(a + 4 * i, 4), 32) for i in range(n)]

    def h_soft(c): return 1 if soft else 0
    def h_buf(c): return DBUF if c.x[0] == LAYER_NI else SBUF
    def h_pitch(c): return L['dp'] if c.x[0] == LAYER_NI else L['sp']
    def h_throw(c): L['throw'] = strof(c.x[0]); raise Thrown()
    def h_alloc(c):
        h = 0x7000 + len(L['strh']); L['strh'][h] = c.x[0]; return h
    def h_log(c): L['logs'].append(strof(L['strh'][mem.rd(c.x[0], 8)]))
    def h_redraw(c): L['redraw'] += 1
    def h_copyrect(c):
        L['copyrect'].append(dict(dx=c.x[1] & M32, dy=c.x[2] & M32, ref=c.x[3], rect=(c.x[4] & M32, (c.x[4] >> 32) & M32, c.x[5] & M32, (c.x[5] >> 32) & M32), plane=c.x[6] & M32))
    def h_gettex(c):
        L['gpu'].append(('GetTextureForRender', c.x[1] & M32, c.x[2])); return 0x34000
    def h_nis(c):
        if not nis_ok: return (-1) & M64
        mem.wr(c.x[3], 8, SRC_NI); L['nis'] = (c.x[1] & M32, mem.rd(c.x[2] and 0x60000, 4)); return 0
    def h_compile(c):
        L['gpu'].append(('Compile', a8(lib, c.x[1]) if lib else hex(c.x[1]), a8(lib, c.x[2]) if lib else hex(c.x[2]), c.x[3] & M32, c.x[4] & M32)); return 0x72000
    def h_getorcompile(c):
        L['gpu'].append(('GetOrCompile', a8(lib, c.x[1]) if lib else hex(c.x[1]), a8(lib, c.x[3]) if lib else hex(c.x[3]), c.x[4] & M32, c.x[5] & M32)); return 0x72000
    def h_setblend(c):
        L['gpu'].append(('SetBlendFuncSeparate',) + tuple(c.x[i] & M32 for i in range(1, 6))); return c.x[0]
    def h_operate(c):
        tex = None
        arr, cnt = mem.rd(c.x[5], 8), mem.rd(c.x[5] + 8, 8)
        if cnt: tex = (mem.rd(arr, 8) != 0, r4(arr + 8, 4), cnt)
        L['gpu'].append(('OperateRect', 'rect', r4(c.x[4], 4), 'ref', c.x[3], 'tex', tex))
    def h_nop(c): return None
    def h_one(c): return 1
    def h_bad(c): raise EmuError('chamada inesperada')

    hooks = {0x848bdc: h_soft, 0x807c00: h_buf, 0x807c20: h_pitch, 0x80c848: h_nop, 0x953d0c: h_throw,
             0xa11fc0: h_alloc, 0xa16ca4: h_log, 0xa11b74: h_nop, 0x4164f0: h_one, 0x425650: h_nop,
             0x848834: lambda c: 0x70000, 0x848454: h_compile, 0x848540: h_getorcompile,
             H_REDRAW: h_redraw, H_COPYRECT: h_copyrect, H_GETTEX: h_gettex, H_NIS: h_nis,
             H_OPERATE: h_operate, H_SETBLEND: h_setblend, 0x407130: h_bad, 0x401780: h_bad}
    return mem, hooks, L


def run(ins, entry, name, params, W, H, pitch, buf, soft=True, src=None, nis_ok=True, lib=None, mode_stack=None):
    mem, hooks, L = setup(W, H, pitch, buf, soft, src, nis_ok, lib)
    cpu = CPU(mem, hooks)
    sp = 0x7f0000
    cpu.sp = sp
    regs = [THIS] + list(params)
    for i, v in enumerate(regs[:8]): cpu.x[i] = v & M64
    if mode_stack is not None: mem.wr(sp, 8, mode_stack & M32)       # argumen ke-9 (mode) di stack
    cpu.x[30] = 0xDEAD0000
    try:
        cpu.run(ins, entry)
    except Thrown:
        pass
    out = mem.dump(DBUF, len(buf))
    L['viol'] = [v for v in mem.violations if v[3] != 'x'] 
    return out, L


# ---------------------------------------------------------------- pembangkit kasus
EDGE = [0, 1, 2, 3, 127, 128, 253, 254, 255]


def rpix(rng, bias=0.6):
    return rng.choice(EDGE) if rng.random() < bias else rng.randrange(256)


def rbuf(rng, w, h, pitch):
    b = bytearray()
    for y in range(h):
        row = bytearray()
        for x in range(w): row += bytes(rpix(rng) for _ in range(4))
        row += bytes([0x5A]) * (pitch - 4 * w)
        b += row
    return bytes(b)


def rint(rng, lo, hi, edge=()):
    return rng.choice(list(edge)) if edge and rng.random() < 0.35 else rng.randint(lo, hi)


def gen_params(rng, name):
    if name in ('turnAlpha', 'turnAlpha2', 'turnAlpha3', 'turnOutline', 'BlackForAlpha'): return []
    if name == 'AlphaColorRect': return [rint(rng, -3, 300, (-1, 0, 255, 256, -1)) for _ in range(3)]
    if name == 'AlphaDivide': return [rint(rng, -5, 260, (0, 1, 127, 128, 255, 256))]
    if name == 'AlphaAdd': return [rint(rng, -300, 300, (0, 1, -1, 255, -255)), rng.choice([0, 1, 2, 3, -1, 5])]
    if name == 'AlphaAdd2': return [rint(rng, -300, 300, (0, 1, -1, 255)), rint(rng, -40, 300, (0, 1, 128, 255, 256))]
    if name == 'ColorAlphaAdd': return [rint(rng, -10, 300, (0, 255, 256)) for _ in range(3)] + [rint(rng, -300, 300, (0, 1, -1, 255))]
    if name in ('ColorVectorEffectAdd', 'ExtraColorVectorEffectAdd'):
        p = [rint(rng, -40, 300, (0, 255, 256)) for _ in range(6)]
        p.append(rng.choice([0, 1, 2, 3, 5, 16, 64, 85, 127, 128, 255, 256, -1, -3, rng.randint(1, 300)]))
        return p
    raise KeyError(name)


def gen_case(rng, name):
    W, H = rng.randint(1, 6), rng.randint(1, 5)
    pad = rng.choice([0, 0, 4, 8])
    pitch = W * 4 + pad
    buf = rbuf(rng, W, H, pitch)
    return W, H, pitch, buf, gen_params(rng, name)


def gen_mask_case(rng, name):
    W, H = rng.randint(1, 7), rng.randint(1, 6)
    pitch = W * 4 + rng.choice([0, 0, 4])
    Ws, Hs = W + 2, H + 6
    spitch = Ws * 4 + rng.choice([0, 0, 8])
    buf, sbuf = rbuf(rng, W, H, pitch), rbuf(rng, Ws, Hs, spitch)
    mode = rng.choice([0, 1, 2, 3, 1, 2, 3, 4, 9])
    if name == 'MaskRect':   # dx,dy,src,sx,sy,sw,sh,mode ; wilayah selalu di dalam batas (di luar = UB asli)
        sw, sh = rng.randint(0, W), rng.randint(0, H)
        dx, dy = rng.randint(0, W - sw), rng.randint(0, H - sh)
        sx_, sy_ = rng.randint(0, Ws - sw), rng.randint(0, Hs - sh)
        return W, H, pitch, buf, (Ws, Hs, spitch, sbuf), [dx, dy, SRC_OBJ, sx_, sy_, sw, sh], mode
    dx, dy = rng.randint(-W - 2, W + 2), rng.randint(-H - 2, H + 2)
    sw, sh = rng.randint(0, W + 3), rng.randint(0, H + 3)
    sy_ = rng.randint(0, 3)
    return W, H, pitch, buf, (Ws, Hs, spitch, sbuf), [dx, dy, SRC_OBJ, rng.randint(-2, 5), sy_, sw, sh], mode


def hexs(b): return b.hex()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('recon'); ap.add_argument('--lib'); ap.add_argument('--golden'); ap.add_argument('--seed', type=int, default=6)
    ap.add_argument('--n', type=int, default=14); ap.add_argument('--trace', action='store_true')
    a = ap.parse_args()
    lib = open(a.lib, 'rb').read() if a.lib else None
    rng = random.Random(a.seed)
    gold = []
    nexcl = nover = 0
    for name, entry in METHODS:
        ins = parse_recon(a.recon, '%x' % entry)
        if not ins: sys.exit('fungsi %s (0x%x) tidak ada di recon' % (name, entry))
        n = a.n if not name.startswith('ExMask') else a.n * 4
        for i in range(n):
            if name.startswith('ExMask'):
                W, H, pitch, buf, src, params, mode = gen_mask_case(rng, name)
                out, L = run(ins, entry, name, params, W, H, pitch, buf, True, src, True, lib, mode_stack=mode)
                rec = [name, ','.join(map(str, params[:2] + params[3:] + [mode])), '%d,%d,%d' % (W, H, pitch), hexs(buf),
                       '%d,%d,%d' % src[:3], hexs(src[3]), hexs(out), str(L['redraw']), ','.join(L['logs']) or '-',
                       ';'.join('%d,%d,%s,%d' % (c['dx'], c['dy'], c['rect'], c['plane']) for c in L['copyrect']) or '-']
            else:
                W, H, pitch, buf, params = gen_case(rng, name)
                out, L = run(ins, entry, name, params, W, H, pitch, buf, True, None, True, lib)
                rec = [name, ','.join(map(str, params)) or '-', '%d,%d,%d' % (W, H, pitch), hexs(buf), '-', '-', hexs(out), str(L['redraw']), '-', '-']
            kinds = {v[3] for v in L['viol']}
            if kinds - {'dst-after'}:         # baca/tulis di luar buffer atau byte padding: perilaku tak terdefinisi -> dikecualikan
                nexcl += 1
                continue
            if kinds: nover += 1              # hanya tulis melewati ujung buffer tujuan; bagian di dalam buffer tetap sah
            gold.append('|'.join(rec))
        if a.trace:   # jalur GPU: satu kasus per mode
            print('== GPU', name)
            modes = [0, 1, 2, 3, 4] if name.startswith('ExMask') else [None]
            for md in modes:
                W, H, pitch = 4, 3, 16
                buf = bytes(range(48))
                if name.startswith('ExMask'):
                    src = (6, 9, 24, bytes(6 * 9 * 4))
                    params = [1, 1, SRC_OBJ, 0, 1, 2, 2] if name == 'MaskRect' else [1, 1, SRC_OBJ, 0, 1, 2, 2]
                    out, L = run(ins, entry, name, params, W, H, pitch, buf, False, src, True, lib, mode_stack=md)
                else:
                    out, L = run(ins, entry, name, [], W, H, pitch, buf, False, None, True, lib)
                if not L['gpu'] and not L['copyrect'] and name not in ('turnAlpha', 'turnAlpha2'): continue
                print(' mode', md, 'redraw', L['redraw'], 'logs', L['logs'])
                for g in L['gpu']: print('   ', g)
                for g in L['copyrect']: print('    CopyRect', g)
    if a.golden:
        with open(a.golden, 'w') as f:
            f.write('# vektor emas CPU layerExAlpha (Alpha-1) hasil emulasi instruksi libgame.so 1.3.9 arm64; seed=%d\n' % a.seed)
            f.write('# metode|param|W,H,pitch|dst_in|Ws,Hs,pitch|src|dst_out|redraw|log|copyrect\n')
            f.write('\n'.join(gold) + '\n')
        print('golden: %d kasus -> %s ; dikecualikan (UB asli: baca/tulis di luar batas/padding): %d ; ikut, tetapi biner menulis melewati ujung buffer: %d' % (len(gold), a.golden, nexcl, nover))


if __name__ == '__main__':
    main()
