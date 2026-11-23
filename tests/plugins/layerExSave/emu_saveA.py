#!/usr/bin/env python3
"""Emulator AArch64 mini: mengeksekusi disassembly ASLI 8 fungsi layerExSave bagian A (sesi S11).

Pemakaian:
    python3 tests/plugins/layerExSave/emu_saveA.py <recon_saveA.txt> [--golden]

recon_saveA.txt (keluaran `ONLY="getCropRect ..." bash script/k2recon.sh @save`) TIDAK ada di repo (keluaran
disassembler tidak di-commit). Skrip hanya membaca baris `alamat: mnemonik operand`, mengeksekusinya pada layer
tiruan, dan membandingkan dengan model Python yang sama dengan layerExSaveCore.hpp.

Fungsi pembantu yang DIPANGGIL (bl) tidak ada di recon (depth 0) -> di-stub menurut perannya yang dibaca dari byte biner:
  0x5ead0c(obj,&w,&h,forWrite) -> larik penunjuk baris      0x5eaea4(result,x,y,w,h) -> Dictionary {x,y,w,h}
  0xa0e090 result.Clear()   0xa0e828/0xa0e860 result = integer   0x953d0c lempar pesan   0xa0cd8c galat konversi variant
  0xa11b94 string->integer   0x9b07c8 (praluar real->int, diabaikan)   0x41f9e0 new[]   0x426f20 delete[]   0x40cdc0 memset
Jadi yang DIBUKTIKAN di sini = isi fungsi 8 method itu; peran stub-nya sendiri berasal dari bacaan byte (lihat spesifikasi).
Tabel lompat jenis-variant dimuat dari nilai yang dibaca di libgame.so (tertanam di JUMPTABLES).
`--golden` mencetak hash FNV-1a yang ditanam di tests/plugins/layerExSave/test_core.cpp (bagian [BINARY]).
"""
import re, sys, random, struct

M32, M64 = 0xFFFFFFFF, 0xFFFFFFFFFFFFFFFF
LINE = re.compile(r'^\s*([0-9a-f]+):\s*\t(\S+)\t?(.*)$')

ENTRY = {'getCropRect': 0x5eab30, 'getCropRectZero': 0x5eb184, 'getDiffRect': 0x5eb454, 'getDiffPixel': 0x5eb86c,
         'oozeColor': 0x5ebbf0, 'copyBlueToAlpha': 0x5ebfec, 'isBlank': 0x5ec190, 'clearAlpha': 0x5ec50c}
MSG = {0x14cb00e: 'Different layer size.', 0x14cb068: 'Invalid level count.', 0x14cb0c2: 'invalid layer range'}
# tabel lompat (alamat dasar -> target absolut tiap jenis variant), dibaca dari libgame.so arm64 1.3.9
JUMPTABLES = {
    0x14cacec: [0x5eb940, 0x5eb8f8, 0x5eb904, 0x5eb914, 0x5eb928, 0x5eb930],
    0x14cad04: [0x5eb9c8, 0x5eb974, 0x5eb980, 0x5eb99c, 0x5eb9b0, 0x5eb9b8],
    0x14cad1c: [0x5ebc58, 0x5ebc64, 0x5ebc84, 0x5ebd0c, 0x5ebd1c],
    0x14cad30: [0x5ebcdc, 0x5ebce8, 0x5ebd34, 0x5ebd50, 0x5ebd60],
    0x14cad44: [0x5ebd9c, 0x5ebda8, 0x5ebdb8, 0x5ebf4c, 0x5ebf54],
    0x14cad58: [0x5ec210, 0x5ec21c, 0x5ec22c, 0x5ec240, 0x5ec248],
    0x14cad6c: [0x5ec27c, 0x5ec288, 0x5ec298, 0x5ec2ac, 0x5ec2b4],
    0x14cad80: [0x5ec2e8, 0x5ec2f4, 0x5ec304, 0x5ec318, 0x5ec320],
    0x14cad94: [0x5ec354, 0x5ec360, 0x5ec370, 0x5ec384, 0x5ec38c],
    0x14cada8: [0x5ec56c, 0x5ec578, 0x5ec5a0, 0x5ec5f8, 0x5ec608],
    0x14cadbc: [0x5ec5e0, 0x5ec5ec, 0x5ec620, 0x5ec634, 0x5ec63c],
}
VOID, OBJECT, STRING, OCTET, INTEGER, REAL = range(6)


class Thrown(Exception):
    def __init__(self, kind, info=None): Exception.__init__(self, '%s %s' % (kind, info)); self.kind, self.info = kind, info
class Crash(Exception): pass


def split_ops(s):
    out, cur, depth = [], '', 0
    for ch in s:
        if ch == '[': depth += 1
        if ch == ']': depth -= 1
        if ch == ',' and depth == 0: out.append(cur.strip()); cur = ''
        else: cur += ch
    if cur.strip(): out.append(cur.strip())
    return out

def load(path):
    prog = {}
    for ln in open(path, encoding='utf-8', errors='replace'):
        m = LINE.match(ln.rstrip('\n'))
        if not m: continue
        body = re.split(r'\s+<', m.group(3), 1)[0].split('//')[0].strip()
        prog[int(m.group(1), 16)] = (m.group(2), split_ops(body))
    for n, a in ENTRY.items():
        if a not in prog: sys.exit('recon tidak memuat %s @%#x' % (n, a))
    return prog

def imm(s): return int(s.lstrip('#'), 0)
def s32(v): v &= M32; return v - (1 << 32) if v >> 31 else v
def s64(v): v &= M64; return v - (1 << 64) if v >> 63 else v


class Layer:
    def __init__(self, w, h, pix=None, pad=8, notlayer=False):
        self.w, self.h, self.pad, self.notlayer = w, h, pad, notlayer
        self.pix = bytearray(pix if pix is not None else bytes(w * h * 4))
        assert len(self.pix) == w * h * 4
        self.pitch = w * 4 + pad
        self.base = None


class CPU:
    TLS, STACK_TOP, VAR0, PARR, RES, HEAP0 = 0x600000, 0x7F0000, 0x500000, 0x510000, 0x520000, 0x1000000

    def __init__(self, prog):
        self.p = prog
        self.x = [0] * 32
        self.mem = {}
        self.n = self.z = self.c = self.v = 0
        self.layers, self.handles = [], {}
        self.heap, self.allocs, self.freed = self.HEAP0, {}, []
        self.result = ('void',)
        self.calls = []           # log pemanggilan 0x5ead0c: (id layer, forWrite)
        self.oob = []             # alamat baca/tulis di area padding/luar layer
        for base, tg in JUMPTABLES.items():
            for i, t in enumerate(tg): self.stw(base + 4 * i, (t - base) & M32, 4)
        self.stw(self.TLS + 0x28, 0x1122334455667788, 8)              # kenari tumpukan

    # ---- register ----
    def get(self, r):
        if r in ('wzr', 'xzr'): return 0
        v = self.x[31 if r == 'sp' else int(r[1:])]
        return v & M32 if r[0] == 'w' else v
    def set(self, r, v):
        if r in ('wzr', 'xzr'): return
        v = (v & M32) if r[0] == 'w' else (v & M64)
        self.x[31 if r == 'sp' else int(r[1:])] = v
    def val(self, s): return imm(s) if s.startswith('#') else self.get(s)
    # ---- memori ----
    def stw(self, a, v, size):
        for i in range(size): self.mem[a + i] = (v >> (8 * i)) & 0xFF
    def ldw(self, a, size):
        for i in range(size):
            if (a + i) not in self.mem: raise Crash('baca memori tak terdefinisi %#x' % (a + i))
        return sum(self.mem[a + i] << (8 * i) for i in range(size))
    def alloc(self, n):
        a = self.heap; self.heap += (n + 15) & ~15; self.allocs[a] = n; return a
    def classify(self, a):
        for L in self.layers:
            if L.base <= a < L.base + L.pitch * L.h:
                if (a - L.base) % L.pitch >= L.w * 4: return 'pad'
                return 'pix'
        return None
    def ea(self, s):
        m = re.match(r'\[(\w+)(?:,\s*([^\]]+))?\](,\s*#-?0x[0-9a-f]+|,\s*#-?\d+)?$', s)
        base = self.get(m.group(1)); post = 0
        if m.group(3): post = imm(m.group(3).lstrip(', ').strip())
        off = 0
        if m.group(2):
            parts = [t.strip() for t in m.group(2).split(',')]
            if parts[0].startswith('#'): off = imm(parts[0])
            else:
                off = self.get(parts[0])
                if len(parts) > 1 and parts[1].startswith('lsl'): off <<= imm(parts[1].split()[1])
        return (base + off) & M64, post, m.group(1)
    def rd(self, a, size):
        if self.classify(a) == 'pad': self.oob.append(('r', a))
        return self.ldw(a, size)
    def wr(self, a, v, size):
        if self.classify(a) == 'pad': self.oob.append(('w', a))
        self.stw(a, v, size)
    # ---- bendera ----
    def addsub(self, a, b, bits, sub):
        mask = (1 << bits) - 1; a &= mask; b &= mask
        r = a - b if sub else a + b
        res = r & mask
        self.n, self.z = res >> (bits - 1), int(res == 0)
        sa, sb, sr = a >> (bits - 1), b >> (bits - 1), res >> (bits - 1)
        if sub: self.c = int(a >= b); self.v = int(sa != sb and sr != sa)
        else:   self.c = int(r > mask); self.v = int(sa == sb and sr != sa)
        return res
    def cond(self, c):
        n, z, cc, v = self.n, self.z, self.c, self.v
        return {'eq': z, 'ne': not z, 'hs': cc, 'cs': cc, 'lo': not cc, 'cc': not cc, 'hi': cc and not z, 'ls': (not cc) or z,
                'ge': n == v, 'lt': n != v, 'gt': (not z) and n == v, 'le': z or n != v}[c]
    # ---- stub pemanggilan ----
    def stub(self, tgt):
        a = self.x
        if tgt == 0x5ead0c:
            L = self.handles.get(a[0])
            if a[0] == 0: raise Crash('0x5ead0c(NULL): dereferensi null')
            if L.notlayer: raise Thrown('specify-layer')
            self.calls.append((self.layers.index(L), a[3] & 1))
            self.stw(a[1], L.w, 4); self.stw(a[2], L.h, 4)
            arr = self.alloc(max(L.h, 1) * 8)
            for y in range(L.h): self.stw(arr + 8 * y, L.base + y * L.pitch, 8)
            a[0] = arr
        elif tgt == 0x953d0c: raise Thrown('msg', MSG.get(a[0], 'msg@%#x' % a[0]))
        elif tgt == 0xa0cd8c: raise Thrown('convert', a[1] & M32)
        elif tgt == 0xa11b94: a[0] = (self.strings[a[0]]) & M64
        elif tgt == 0x9b07c8: pass
        elif tgt == 0xa0e090: self.result = ('void',)
        elif tgt == 0xa0e828: self.result = ('int', s32(a[1]))
        elif tgt == 0xa0e860: self.result = ('int', s64(a[1]))
        elif tgt == 0x5eaea4: self.result = ('dict', (s64(a[1]), s64(a[2]), s64(a[3]), s64(a[4])))
        elif tgt == 0x426f20:
            if a[0] not in self.allocs: raise Crash('delete[] pada penunjuk tak dikenal %#x' % a[0])
            self.freed.append(a[0])
        elif tgt == 0x41f9e0: a[0] = self.alloc(a[0])
        elif tgt == 0x40cdc0:
            for i in range(a[2]): self.stw(a[0] + i, a[1] & 0xFF, 1)
        elif tgt == 0x407130: raise Crash('__stack_chk_fail')
        else: raise Crash('bl ke fungsi tak di-stub %#x' % tgt)
    # ---- eksekusi ----
    def run(self, pc, maxsteps=3_000_000):
        bits = lambda r: 32 if r[0] == 'w' else 64
        for _ in range(maxsteps):
            if pc not in self.p: raise Crash('pc di luar fungsi: %#x' % pc)
            op, a = self.p[pc]; nxt = pc + 4
            if op == 'ret': return
            elif op == 'bl': self.stub(imm(a[0]))
            elif op == 'mrs': self.set(a[0], self.TLS)
            elif op == 'adrp': self.set(a[0], imm(a[1]))
            elif op in ('ldr', 'ldrb', 'ldurb', 'ldrsw'):
                if a[0] == 'd0':
                    ea, post, _ = self.ea(a[1]); self.d0 = self.rd(ea, 8)
                else:
                    ea, post, b = self.ea(a[1])
                    if op == 'ldrsw': self.set(a[0], s32(self.rd(ea, 4)))
                    else: self.set(a[0], self.rd(ea, 1 if op != 'ldr' else (8 if a[0][0] == 'x' else 4)))
                    if len(a) > 2 and a[2].startswith('#'): post = imm(a[2])
                    if post: self.set(b, self.get(b) + post)
            elif op in ('str', 'strb', 'sturb'):
                ea, post, b = self.ea(a[1]); size = 1 if op != 'str' else (8 if a[0][0] == 'x' else 4)
                self.wr(ea, self.get(a[0]), size)
                if len(a) > 2 and a[2].startswith('#'): post = imm(a[2])
                if post: self.set(b, self.get(b) + post)
            elif op in ('ldp', 'ldpsw', 'stp'):
                ea, post, _ = self.ea(a[2]); sz = 8 if (a[0][0] == 'x' and op != 'ldpsw') else 4
                for i, r in enumerate(a[:2]):
                    if op == 'stp': self.wr(ea + i * sz, self.get(r), sz)
                    else:
                        v = self.rd(ea + i * sz, sz)
                        self.set(r, s32(v) if op == 'ldpsw' else v)
            elif op == 'fcvtzs':
                f = struct.unpack('<d', (self.d0 & M64).to_bytes(8, 'little'))[0]
                self.set(a[0], int(f) if f == f and abs(f) < 2 ** 63 else 0)
            elif op == 'cmp':
                b = self.get(a[1]) if not a[1].startswith('#') else imm(a[1])
                if len(a) > 2 and a[2] == 'sxtw': b = s32(b)
                self.addsub(self.get(a[0]), b, bits(a[0]), True)
            elif op == 'tst': r = self.get(a[0]) & imm(a[1]); self.n, self.z, self.c, self.v = r >> (bits(a[0]) - 1), int(r == 0), 0, 0
            elif op == 'add': self.set(a[0], self.get(a[1]) + self.val(a[2]))
            elif op == 'sub': self.set(a[0], self.get(a[1]) - self.val(a[2]))
            elif op == 'mul': self.set(a[0], self.get(a[1]) * self.get(a[2]))
            elif op == 'lsl': self.set(a[0], self.get(a[1]) << imm(a[2]))
            elif op == 'lsr': self.set(a[0], self.get(a[1]) >> imm(a[2]))
            elif op == 'sxtw': self.set(a[0], s32(self.get(a[1])))
            elif op == 'and': self.set(a[0], self.get(a[1]) & self.val(a[2]))
            elif op == 'orr': self.set(a[0], self.get(a[1]) | self.val(a[2]))
            elif op == 'mov': self.set(a[0], self.val(a[1]))
            elif op == 'bfi':                                   # bfi Xd, Xn, #lsb, #width
                lsb, wd = imm(a[2]), imm(a[3]); m = ((1 << wd) - 1) << lsb
                self.set(a[0], (self.get(a[0]) & ~m) | ((self.get(a[1]) << lsb) & m))
            elif op == 'csel': self.set(a[0], self.get(a[1]) if self.cond(a[3]) else self.get(a[2]))
            elif op == 'csinc': self.set(a[0], self.get(a[1]) if self.cond(a[3]) else self.get(a[2]) + 1)
            elif op == 'cset': self.set(a[0], 1 if self.cond(a[1]) else 0)
            elif op == 'cbz':
                if self.get(a[0]) == 0: nxt = imm(a[1])
            elif op == 'cbnz':
                if self.get(a[0]) != 0: nxt = imm(a[1])
            elif op == 'tbnz':
                if (self.get(a[0]) >> imm(a[1])) & 1: nxt = imm(a[2])
            elif op == 'b': nxt = imm(a[0])
            elif op == 'br': nxt = self.get(a[0])
            elif op.startswith('b.'):
                if self.cond(op[2:]): nxt = imm(a[0])
            else: raise Crash('instruksi tak didukung %s %s @%#x' % (op, a, pc))
            pc = nxt
        raise Crash('terlalu banyak langkah')

    # ---- API ----
    def add_layer(self, L):
        L.base = 0x2000000 + len(self.layers) * 0x100000
        self.layers.append(L)
        self.handles[0x5000_0000 + 0x10 * len(self.layers)] = L
        for i in range(L.pitch * L.h): self.mem[L.base + i] = 0xEE                 # padding 0xEE
        for y in range(L.h):
            for i in range(L.w * 4): self.mem[L.base + y * L.pitch + i] = L.pix[y * L.w * 4 + i]
        for i in range(64): self.mem[L.base + L.pitch * L.h + i] = 0xEE           # pagar setelah baris terakhir
        return 0x5000_0000 + 0x10 * len(self.layers)

    def call(self, name, params, this_h, with_result=True):
        """params: daftar (tipe, nilai); tipe = 'void' 'int' 'real' 'str' 'obj' 'octet'."""
        self.strings = {}
        ptrs = []
        for i, (t, v) in enumerate(params):
            va = self.VAR0 + 0x20 * i
            tid = {'void': VOID, 'obj': OBJECT, 'str': STRING, 'octet': OCTET, 'int': INTEGER, 'real': REAL}[t]
            if t == 'real': self.stw(va, struct.unpack('<Q', struct.pack('<d', v))[0], 8)
            elif t == 'str': self.strings[0x7000 + i] = v[0]; self.stw(va, 0x7000 + i, 8)
            elif t == 'obj': self.stw(va, v, 8); self.stw(va + 8, v, 8)
            elif t in ('int', 'octet'): self.stw(va, v & M64, 8)
            else: self.stw(va, 0, 8)
            self.stw(va + 0x10, tid, 4)
            self.stw(self.PARR + 8 * i, va, 8); ptrs.append(va)
        self.x = [0] * 32
        self.x[0] = self.RES if with_result else 0
        self.x[1], self.x[2], self.x[3], self.x[31] = len(params), self.PARR, this_h, self.STACK_TOP
        self.result = ('void',); threw = None; ret = None
        pre_allocs = len(self.allocs)
        try:
            self.run(min(ENTRY[name], ENTRY[name]) if False else ENTRY[name])
            ret = s32(self.x[0])
        except Thrown as e: threw = (e.kind, e.info)
        leaks = [k for k in self.allocs if k not in self.freed]
        return dict(ret=ret, threw=threw, result=self.result, leaks=len(leaks), calls=list(self.calls), oob=list(self.oob))


# ---------------------------------------------------------------------------------------------------------------
# generator deterministik: HARUS sama persis dengan di test_core.cpp
def lcg(s): return (s * 1664525 + 1013904223) & M32

def gen_sparse(seed, W, H, k):
    """piksel aktif (peluang 1/k) = 4 byte acak; selain itu alfa 0 dan warna 0 atau acak."""
    s, out = seed & M32, bytearray()
    for _ in range(W * H):
        s = lcg(s); d = (s >> 24)
        if k and d % k == 0:
            for _ in range(4): s = lcg(s); out.append((s >> 24) & 0xFF)
        else:
            s = lcg(s); e = (s >> 24)
            if e & 1:
                for _ in range(3): s = lcg(s); out.append((s >> 24) & 0xFF)
            else: out += b'\0\0\0'
            out.append(0)
    return bytes(out)

def gen_mutate(seed, base, m):
    """salin `base`; tiap piksel dengan peluang 1/m diubah: mode 0 = hanya warna B,G,R; mode 1 = alfa; mode 2 = semua."""
    s, out = seed & M32, bytearray(base)
    for i in range(len(base) // 4):
        s = lcg(s); d = (s >> 24)
        if m and d % m == 0:
            s = lcg(s); mode = (s >> 24) % 3
            if mode in (0, 2):
                for c in range(3): s = lcg(s); out[i * 4 + c] = (s >> 24) & 0xFF
            if mode in (1, 2): s = lcg(s); out[i * 4 + 3] = (s >> 24) & 0xFF
    return bytes(out)

def fnv1a(b, h=0xCBF29CE484222325):
    for x in b: h = ((h ^ x) * 0x100000001B3) & M64
    return h

def le(v, n): return (v & ((1 << (8 * n)) - 1)).to_bytes(n, 'little')


# ---------------------------------------------------------------------------------------------------------------
# model Python (sama dengan layerExSaveCore.hpp)
def px(buf, W, x, y): return buf[(y * W + x) * 4:(y * W + x) * 4 + 4]
def same(a, b): return a[3] == b[3] and (a[3] == 0 or a[:3] == b[:3])

def m_scan(W, H, active, bottom_right):
    if W < 1 or H < 1: return None
    left = next((x for x in range(W) if any(active(x, y) for y in range(H))), None)
    if left is None: return None
    right = next(x for x in range(W - 1, -1, -1) if any(active(x, y) for y in range(H)))
    cnt = right - left + 1
    top = next(y for y in range(H) if any(active(left + k, y) for k in range(cnt)))
    x0 = right if bottom_right else left
    bottom = top
    for y in range(H - 1, -1, -1):
        if any(x0 + k < W and active(x0 + k, y) for k in range(cnt)): bottom = y; break
    return (left, top, cnt, bottom - top + 1)

def m_crop(buf, W, H, zero):
    return m_scan(W, H, (lambda x, y: any(px(buf, W, x, y))) if zero else (lambda x, y: px(buf, W, x, y)[3] != 0), False)

def m_diffrect(a, b, W, H, bug):
    return m_scan(W, H, lambda x, y: not same(px(a, W, x, y), px(b, W, x, y)), bug)

def m_diffpixel(a, b, W, H, hs, sv, hd, dv):
    b = bytearray(b); cnt = 0
    for i in range(W * H):
        pa, pb = a[i * 4:i * 4 + 4], b[i * 4:i * 4 + 4]
        if same(pa, pb):
            if hs: b[i * 4:i * 4 + 4] = le(sv, 4)
        else:
            if hd: b[i * 4:i * 4 + 4] = le(dv, 4)
            cnt += 1
    return bytes(b), cnt

def m_ooze(buf, thr, color):
    t = (thr & 0xFF) or 1; b = bytearray(buf)
    for i in range(len(b) // 4):
        if b[i * 4 + 3] < t: b[i * 4], b[i * 4 + 1], b[i * 4 + 2] = color & 0xFF, (color >> 8) & 0xFF, (color >> 16) & 0xFF
    return bytes(b)

def m_blue2alpha(src, sw, sh, dst, dw, dh):
    d = bytearray(dst)
    for y in range(min(sh, dh)):
        for x in range(min(sw, dw)): d[(y * dw + x) * 4 + 3] = src[(y * sw + x) * 4]
    return bytes(d)

def m_blank(buf, W, x, y, w, h):
    return all(buf[((j * W) + i) * 4] == 0 for j in range(y, y + h) for i in range(x, x + w)) if (w >= 1 and h >= 1) else True

def m_clear(buf, thr, color):
    b = bytearray(buf)
    for i in range(len(b) // 4):
        if not (thr < b[i * 4 + 3]): b[i * 4:i * 4 + 4] = le(color & 0xFFFFFF, 4)
    return bytes(b)


def pixels_of(cpu, L):
    out = bytearray()
    for y in range(L.h): out += bytes(cpu.mem[L.base + y * L.pitch + i] for i in range(L.w * 4))
    return bytes(out)

def pad_ok(cpu, L):
    return all(cpu.mem[L.base + y * L.pitch + i] == 0xEE for y in range(L.h) for i in range(L.w * 4, L.pitch))

def fresh(prog): return CPU(prog)


def run_all(prog, golden=False):
    rnd = random.Random(11)
    n = bad = 0
    gold = []
    def check(cond, msg):
        nonlocal n, bad
        n += 1
        if not cond: bad += 1; print('BEDA', msg)

    # ---- getCropRect / getCropRectZero ----
    for name, zero in (('getCropRect', False), ('getCropRectZero', True)):
        for (W, H) in ((1, 1), (2, 3), (5, 4), (8, 8), (13, 7), (16, 16)):
            for k in (0, 2, 7, 40):
                for t in range(2):
                    seed = W * 1000 + H * 100 + k * 7 + t + (5 if zero else 0)
                    buf = gen_sparse(seed, W, H, k)
                    cpu = fresh(prog); L = Layer(W, H, buf, pad=8); h = cpu.add_layer(L)
                    r = cpu.call(name, [], h)
                    exp = m_crop(buf, W, H, zero)
                    got = r['result'][1] if r['result'][0] == 'dict' else None
                    if r['result'][0] == 'dict': got = tuple(got)
                    check(r['ret'] == 0 and got == exp and not r['threw'] and r['leaks'] == (1 if exp is None else 0) and not r['oob'] and r['calls'] == [(0, 0)],   # biner membocorkan larik baris bila tak ada piksel aktif (jalur void)
                          '%s W=%d H=%d k=%d got=%s exp=%s %s' % (name, W, H, k, got, exp, r))
                    if golden and t == 0 and (W, H) in ((5, 4), (13, 7), (16, 16)):
                        gold.append((name, seed, W, H, k, None if exp is None else exp))
    # ---- getDiffRect (bug-compat: bawah mulai dari kolom KANAN; padding besar agar baca-luar-batas terbaca 0xEE) ----
    for (W, H) in ((1, 1), (3, 3), (5, 4), (9, 6), (13, 7), (16, 12)):
        for m in (0, 3, 9, 60):
            for t in range(3):
                seed = W * 977 + H * 31 + m + t
                A = gen_sparse(seed, W, H, 3)
                B = gen_mutate(seed + 99, A, m)
                cpu = fresh(prog); LA = Layer(W, H, A, pad=W * 4 + 16); LB = Layer(W, H, B, pad=W * 4 + 16)
                ha = cpu.add_layer(LA); hb = cpu.add_layer(LB)
                r = cpu.call('getDiffRect', [('obj', ha)], hb)
                got = tuple(r['result'][1]) if r['result'][0] == 'dict' else None
                exp = m_diffrect(A, B, W, H, True)
                check(r['ret'] == 0 and got == exp and not r['threw'] and r['leaks'] == (2 if exp is None else 0) and r['calls'] == [(0, 0), (1, 0)],   # idem: 2 larik bocor di jalur void
                      'getDiffRect W=%d H=%d m=%d got=%s exp=%s %s' % (W, H, m, got, exp, r))
                if golden and t == 0 and (W, H) in ((5, 4), (13, 7), (16, 12)): gold.append(('getDiffRect', seed, W, H, m, exp))
    # contoh yang MEMBUKTIKAN bug: beda di (2,0) dan (0,5) -> kiri=0, kanan=2; kolom kanan (2) hanya beda di baris 0
    W = H = 6
    A = bytearray(W * H * 4); B = bytearray(A)
    B[(0 * W + 2) * 4 + 3] = 0xFF; B[(5 * W + 0) * 4 + 3] = 0xFF
    cpu = fresh(prog); la = Layer(W, H, bytes(A), pad=W * 4 + 16); lb = Layer(W, H, bytes(B), pad=W * 4 + 16)
    ha, hb = cpu.add_layer(la), cpu.add_layer(lb)
    r = cpu.call('getDiffRect', [('obj', ha)], hb)
    print('# getDiffRect (beda di (2,0) & (0,5)): biner -> %s ; rect benar = %s ; model bug = %s' %
          (r['result'], m_diffrect(bytes(A), bytes(B), W, H, False), m_diffrect(bytes(A), bytes(B), W, H, True)))
    check(tuple(r['result'][1]) == m_diffrect(bytes(A), bytes(B), W, H, True), 'getDiffRect contoh bug: biner == model bug')
    check(m_diffrect(bytes(A), bytes(B), W, H, True) != m_diffrect(bytes(A), bytes(B), W, H, False), 'bug getDiffRect harus terlihat')
    # ukuran beda -> pesan
    cpu = fresh(prog); la = Layer(4, 4, bytes(64)); lb = Layer(4, 5, bytes(80))
    ha, hb = cpu.add_layer(la), cpu.add_layer(lb)
    r = cpu.call('getDiffRect', [('obj', ha)], hb)
    check(r['threw'] == ('msg', 'Different layer size.') and r['leaks'] == 2, 'getDiffRect ukuran beda %s' % r)
    # numparams 0 -> -1004 ; param bukan objek -> galat konversi ke object (1)
    cpu = fresh(prog); hb = cpu.add_layer(Layer(2, 2, bytes(16)))
    check(cpu.call('getDiffRect', [], hb)['ret'] == -1004, 'getDiffRect 0 param')
    cpu = fresh(prog); hb = cpu.add_layer(Layer(2, 2, bytes(16)))
    check(cpu.call('getDiffRect', [('int', 5)], hb)['threw'] == ('convert', 1), 'getDiffRect param bukan objek')

    # ---- getDiffPixel ----
    for (W, H) in ((1, 1), (4, 3), (7, 5), (12, 9)):
        for m in (0, 4, 30):
            for mask in range(4):                                         # bit0: has_same, bit1: has_diff
                seed = W * 313 + H * 17 + m * 5 + mask
                A = gen_sparse(seed, W, H, 3); B = gen_mutate(seed + 7, A, m)
                sv, dv = (0x80FF8040 + seed) & M32, (0x7F102030 + seed * 3) & M32
                prm = [('obj', None)]
                cpu = fresh(prog); la = Layer(W, H, A, pad=8); lb = Layer(W, H, B, pad=8)
                ha, hb = cpu.add_layer(la), cpu.add_layer(lb)
                prm = [('obj', ha)]
                if mask & 1 or mask & 2:
                    prm.append(('int', sv) if mask & 1 else ('void', 0))
                if mask & 2: prm.append(('int', dv))
                r = cpu.call('getDiffPixel', prm, hb)
                eb, ec = m_diffpixel(A, B, W, H, bool(mask & 1), sv, bool(mask & 2), dv)
                check(r['ret'] == 0 and r['result'] == ('int', ec) and pixels_of(cpu, lb) == eb and pad_ok(cpu, lb) and pad_ok(cpu, la)
                      and r['leaks'] == 0 and r['calls'] == [(0, 0), (1, 1)] and not r['oob'],
                      'getDiffPixel W=%d H=%d m=%d mask=%d res=%s exp=%d %s' % (W, H, m, mask, r['result'], ec, r['threw']))
                if golden and mask == 3 and m == 4 and (W, H) in ((7, 5), (12, 9)):
                    gold.append(('getDiffPixel', seed, W, H, m, (sv, dv, fnv1a(eb), ec)))
    # getDiffPixel: param 1 void + param 2 diberikan (has_same=false, has_diff=true)
    W = H = 4; A = gen_sparse(5, W, H, 2); B = gen_mutate(6, A, 3)
    cpu = fresh(prog); la = Layer(W, H, A); lb = Layer(W, H, B); ha, hb = cpu.add_layer(la), cpu.add_layer(lb)
    r = cpu.call('getDiffPixel', [('obj', ha), ('void', 0), ('int', 0x11223344)], hb)
    eb, ec = m_diffpixel(A, B, W, H, False, 0, True, 0x11223344)
    check(pixels_of(cpu, lb) == eb and r['result'] == ('int', ec), 'getDiffPixel void+int')
    cpu = fresh(prog); hb = cpu.add_layer(Layer(2, 2, bytes(16)))
    check(cpu.call('getDiffPixel', [], hb)['ret'] == -1004, 'getDiffPixel 0 param')
    # alias: this == param
    W = H = 3; A = gen_sparse(8, W, H, 2)
    cpu = fresh(prog); la = Layer(W, H, A); ha = cpu.add_layer(la)
    r = cpu.call('getDiffPixel', [('obj', ha), ('int', 0xDEADBEEF)], ha)
    check(r['result'] == ('int', 0) and pixels_of(cpu, la) == m_diffpixel(A, A, W, H, True, 0xDEADBEEF, False, 0)[0], 'getDiffPixel alias')

    # ---- oozeColor ----
    for (W, H) in ((1, 1), (3, 2), (6, 5), (11, 8)):
        for thr in (None, 0, 1, 2, 77, 256, 257, 255, 0x1FF):
            for lv in (1, 2, 5):
                seed = W * 71 + H * 13 + (thr if thr is not None else 999) + lv
                buf = gen_sparse(seed, W, H, 2)
                # beri variasi alfa kecil supaya ambang terasa
                buf = bytes((b if i % 4 != 3 else (b % 5) * (1 if (i // 4) % 3 else 40)) for i, b in enumerate(buf))
                color = (0x00AABBCC + seed * 12345) & 0xFFFFFF
                cpu = fresh(prog); L = Layer(W, H, buf, pad=8); h = cpu.add_layer(L)
                prm = [('int', lv)]
                if thr is not None: prm += [('int', thr), ('int', color)]
                r = cpu.call('oozeColor', prm, h)
                et = 1 if thr is None else thr
                exp = m_ooze(buf, et, color if thr is not None else 0)
                check(r['ret'] == 0 and pixels_of(cpu, L) == exp and pad_ok(cpu, L) and r['result'] == ('void',) and r['calls'] == [(0, 1)]
                      and not r['oob'], 'oozeColor W=%d H=%d thr=%s lv=%d' % (W, H, thr, lv))
                if golden and lv == 2 and thr in (None, 2, 257) and (W, H) == (11, 8): gold.append(('oozeColor', seed, W, H, thr, (lv, color, fnv1a(exp))))
    # oozeColor: levelCount invalid, parameter bervariasi
    for lv in (0, -3):
        cpu = fresh(prog); h = cpu.add_layer(Layer(2, 2, bytes(16)))
        r = cpu.call('oozeColor', [('int', lv)], h)
        check(r['threw'] == ('msg', 'Invalid level count.'), 'oozeColor level %d: %s' % (lv, r))
    cpu = fresh(prog); h = cpu.add_layer(Layer(2, 2, bytes(16)))
    check(cpu.call('oozeColor', [], h)['ret'] == -1004, 'oozeColor 0 param')
    # void pada param[1] -> ambang 0 (efektif 1), bukan default
    buf = gen_sparse(3, 4, 4, 2); buf = bytes((b if i % 4 != 3 else b % 3) for i, b in enumerate(buf))
    cpu = fresh(prog); L = Layer(4, 4, buf); h = cpu.add_layer(L)
    cpu.call('oozeColor', [('int', 1), ('void', 0), ('int', 0x0A0B0C)], h)
    check(pixels_of(cpu, L) == m_ooze(buf, 0, 0x0A0B0C), 'oozeColor param1 void')
    # real & string konversi
    cpu = fresh(prog); L = Layer(4, 4, buf); h = cpu.add_layer(L)
    cpu.call('oozeColor', [('real', 2.9), ('real', 2.9), ('str', (0x445566,))], h)
    check(pixels_of(cpu, L) == m_ooze(buf, 2, 0x445566), 'oozeColor real/string')
    cpu = fresh(prog); h = cpu.add_layer(Layer(2, 2, bytes(16)))
    check(cpu.call('oozeColor', [('obj', h)], h)['threw'] == ('convert', 4), 'oozeColor param objek')

    # ---- copyBlueToAlpha ----
    for (sw, sh, dw, dh) in ((1, 1, 1, 1), (4, 4, 4, 4), (6, 3, 4, 5), (3, 7, 5, 2), (8, 8, 8, 8)):
        seed = sw * 100 + sh * 10 + dw + dh
        S = gen_sparse(seed, sw, sh, 2); D = gen_sparse(seed + 1, dw, dh, 2)
        cpu = fresh(prog); ls = Layer(sw, sh, S, pad=8); ld = Layer(dw, dh, D, pad=8)
        hs, hd = cpu.add_layer(ls), cpu.add_layer(ld)
        r = cpu.call('copyBlueToAlpha', [('obj', hs)], hd)
        exp = m_blue2alpha(S, sw, sh, D, dw, dh)
        check(r['ret'] == 0 and pixels_of(cpu, ld) == exp and pixels_of(cpu, ls) == S and pad_ok(cpu, ld) and pad_ok(cpu, ls)
              and r['leaks'] == 0 and r['calls'] == [(0, 0), (1, 1)] and not r['oob'], 'copyBlueToAlpha %s' % ((sw, sh, dw, dh),))
        if golden and (sw, sh, dw, dh) in ((6, 3, 4, 5), (8, 8, 8, 8)): gold.append(('copyBlueToAlpha', seed, sw, sh, (dw, dh), fnv1a(exp)))
    S = gen_sparse(2, 3, 3, 2); cpu = fresh(prog); ls = Layer(3, 3, S); hs = cpu.add_layer(ls)
    r = cpu.call('copyBlueToAlpha', [('obj', hs)], hs)                                    # alias
    check(pixels_of(cpu, ls) == m_blue2alpha(S, 3, 3, S, 3, 3), 'copyBlueToAlpha alias')
    cpu = fresh(prog); hd = cpu.add_layer(Layer(2, 2, bytes(16)))
    check(cpu.call('copyBlueToAlpha', [('int', 3)], hd)['threw'] == ('convert', 1), 'copyBlueToAlpha param bukan objek')

    # ---- isBlank ----
    for (W, H) in ((1, 1), (5, 4), (9, 7), (16, 10)):
        for dens in (0, 3, 25):
            for t in range(4):
                seed = W * 53 + H * 7 + dens * 3 + t
                buf = gen_sparse(seed, W, H, dens)
                if dens == 0:
                    buf = bytes((b if i % 4 != 0 else 0) for i, b in enumerate(buf))            # biru nol semua, warna lain/alfa acak
                if t == 3: buf = bytes((b if i % 4 != 3 else 0xFF) for i, b in enumerate(buf))
                rr = random.Random(seed)
                x, y = rr.randrange(W), rr.randrange(H); w, h = rr.randrange(0, W - x + 1), rr.randrange(0, H - y + 1)
                cpu = fresh(prog); L = Layer(W, H, buf, pad=8); hh = cpu.add_layer(L)
                r = cpu.call('isBlank', [('int', x), ('int', y), ('int', w), ('int', h)], hh)
                exp = m_blank(buf, W, x, y, w, h)
                lk = 0 if exp else 1                                                           # biner membocorkan larik baris pada hasil non-blank
                check(r['ret'] == 0 and r['result'] == ('int', 1 if exp else 0) and r['leaks'] == lk and not r['oob'] and pad_ok(cpu, L),
                      'isBlank W=%d H=%d %s exp=%s got=%s leaks=%d' % (W, H, (x, y, w, h), exp, r['result'], r['leaks']))
                if golden and t < 2 and (W, H) in ((9, 7), (16, 10)): gold.append(('isBlank', seed, W, H, dens, (x, y, w, h, int(exp), buf is not None)))
    # rentang tidak sah
    W, H = 6, 5
    for rect, bad_ in (((-1, 0, 2, 2), 1), ((0, -1, 2, 2), 1), ((0, 0, -1, 2), 1), ((0, 0, 2, -1), 0), ((5, 0, 2, 2), 1), ((0, 4, 2, 2), 1),
                       ((0, 0, 6, 5), 0), ((6, 0, 0, 0), 0), ((0, 5, 0, 0), 0), ((0, 0, 0, 0), 0), ((2, 2, 0, 3), 0)):
        cpu = fresh(prog); L = Layer(W, H, bytes(W * H * 4)); hh = cpu.add_layer(L)
        r = cpu.call('isBlank', [('int', v) for v in rect], hh)
        okr = (r['threw'] == ('msg', 'invalid layer range')) == bool(bad_)
        check(okr, 'isBlank rentang %s -> %s' % (rect, r['threw'] or r['result']))
        if not bad_: check(r['result'] == ('int', 1), 'isBlank rentang %s hasil %s' % (rect, r['result']))
    cpu = fresh(prog); hh = cpu.add_layer(Layer(2, 2, bytes(16)))
    check(cpu.call('isBlank', [('int', 0), ('int', 0), ('int', 1)], hh)['ret'] == -1004, 'isBlank 3 param')
    # x+w melingkar (32-bit): biner lolos pemeriksaan lalu membaca di luar batas -> di emulator: baca memori tak terdefinisi
    cpu = fresh(prog); hh = cpu.add_layer(Layer(6, 5, bytes(120)))
    try:
        r = cpu.call('isBlank', [('int', 0x7FFFFFFF), ('int', 0), ('int', 2), ('int', 1)], hh); crashed = False
    except Crash: crashed = True
    check(crashed, 'isBlank x+w melingkar: biner membaca di luar batas (UB)')

    # ---- clearAlpha ----
    for (W, H) in ((1, 1), (4, 3), (9, 7)):
        for prm_ in ([], [0], [5], [128, 0x00FF8000], [255, 0x12345678], [-1], [-1, 0xABCDEF], [300, 0x010203], [255, 0xFFFFFFFF]):
            seed = W * 29 + H * 3 + len(prm_) * 101 + sum(prm_) % 997
            buf = gen_sparse(seed, W, H, 2)
            buf = bytes((b if i % 4 != 3 else (b if (i // 4) % 2 else b % 4)) for i, b in enumerate(buf))
            cpu = fresh(prog); L = Layer(W, H, buf, pad=8); hh = cpu.add_layer(L)
            prm = [('int', v) for v in prm_]
            r = cpu.call('clearAlpha', prm, hh)
            thr = prm_[0] if prm_ else 0; col = (prm_[1] & 0xFFFFFF) if len(prm_) > 1 else 0
            exp = m_clear(buf, thr, col)
            check(r['ret'] == 0 and pixels_of(cpu, L) == exp and pad_ok(cpu, L) and r['calls'] == [(0, 1)] and r['leaks'] == 0,
                  'clearAlpha W=%d H=%d %s' % (W, H, prm_))
            if golden and (W, H) == (9, 7) and len(prm_) in (0, 2): gold.append(('clearAlpha', seed, W, H, tuple(prm_), fnv1a(exp)))
    print('emulasi %d pemeriksaan, %d beda' % (n, bad))
    return bad, gold


if __name__ == '__main__':
    if len(sys.argv) < 2: sys.exit(__doc__)
    prog = load(sys.argv[1])
    bad, gold = run_all(prog, '--golden' in sys.argv)
    if '--golden' in sys.argv:
        print('# GOLDEN'); [print(g) for g in gold]
    sys.exit(1 if bad else 0)
