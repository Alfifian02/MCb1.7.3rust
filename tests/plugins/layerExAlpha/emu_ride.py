#!/usr/bin/env python3
"""Emulator AArch64 mini untuk mengeksekusi disassembly asli layerExAlpha::MaskImageRideRect4 (0x5d8f94) dan
MaskImageRideRect5 (0x5d9cb8) dari recon_alpha3.txt, lalu membandingkannya dengan model (rideN_model).

Pemakaian:
    python3 tests/plugins/layerExAlpha/emu_ride.py <recon_alpha3.txt> [--golden]

`--golden` mencetak hash FNV-1a keluaran (seluruh buffer tujuan) untuk konfigurasi besar; nilai itu ditanam di test_core.cpp.

Recon TIDAK ada di repo (keluaran disassembler tidak di-commit); skrip hanya membaca baris `alamat: mnemonik operand`.
Seluruh fungsi dieksekusi. Panggilan keluar diganti stub (dikenali dari alamat `bl`/`blr`):
  NativeInstanceSupport (vtable+0xc8), 0x807c00 (pointer piksel), 0x807c20 (pitch), 0x806828/0x806920 (lebar/tinggi gambar),
  _M_default_append (vector<float>/<int>), operator delete, `redraw` (ctx.vtable+8), TVPThrowExceptionMessage.
Memori berbasis wilayah dengan batas persis: baca/tulis di luar wilayah => galat (mendeteksi anomali batas kolom Rect5).
Hanya himpunan instruksi kedua fungsi ini yang didukung; instruksi lain => berhenti dengan galat.
Aritmatika float memakai IEEE biner32/biner64 (hitung di double lalu bulatkan ke float32: benar untuk + - * /).
"""
import re, sys, struct, math, random

M32, M64 = 0xFFFFFFFF, 0xFFFFFFFFFFFFFFFF
LINE = re.compile(r'^\s*([0-9a-f]+):\s*\t(\S+)\t?(.*)$')

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
    """Kembalikan {alamat: (mnemonik, [operand])} untuk SEMUA fungsi dalam berkas."""
    prog = {}
    for ln in open(path, encoding='utf-8', errors='replace'):
        m = LINE.match(ln.rstrip('\n'))
        if not m: continue
        ops = split_ops(re.split(r'\s+<', m.group(3), 1)[0].split('//')[0].strip())   # buang ' <simbol+off>'
        prog[int(m.group(1), 16)] = (m.group(2), ops)
    return prog

def imm(s): return int(s.lstrip('#'), 0)
def s32(v): v &= M32; return v - (1 << 32) if v >> 31 else v
def f32(x):
    try: return struct.unpack('<f', struct.pack('<f', x))[0]
    except OverflowError: return math.copysign(math.inf, x)
def f32bits(x): return struct.unpack('<I', struct.pack('<f', x))[0]
def bits32f(b): return struct.unpack('<f', struct.pack('<I', b & M32))[0]
def f64bits(x): return struct.unpack('<Q', struct.pack('<d', x))[0]
def bits64f(b): return struct.unpack('<d', struct.pack('<Q', b & M64))[0]
def fcvtzs(x, bits):
    if x != x: return 0
    lo, hi = -(1 << (bits - 1)), (1 << (bits - 1)) - 1
    if x == math.inf: return hi
    if x == -math.inf: return lo
    return max(lo, min(hi, int(x)))                       # int() memotong ke nol

class Mem:
    def __init__(self): self.reg = []                     # [(base, bytearray, nama)]
    def add(self, base, size, name): b = bytearray(size); self.reg.append((base, b, name)); return b
    def _find(self, a, n):
        for base, b, name in self.reg:
            if base <= a and a + n <= base + len(b): return b, a - base
        raise RuntimeError('akses memori di luar wilayah: %#x (+%d)' % (a, n))
    def rd(self, a, n): b, o = self._find(a, n); return int.from_bytes(b[o:o + n], 'little')
    def wr(self, a, v, n): b, o = self._find(a, n); b[o:o + n] = (v & ((1 << (8 * n)) - 1)).to_bytes(n, 'little')

class Stop(Exception): pass

class CPU:
    def __init__(self, prog, mem, stubs):
        self.p, self.m, self.stubs = prog, mem, stubs
        self.x = [0] * 32; self.v = [0] * 32
        self.n = self.z = self.c = self.vf = 0
        self.log = []
    # ---- register ----
    def gp(self, r):
        if r in ('wzr', 'xzr'): return 0
        val = self.x[31 if r == 'sp' else int(r[1:])]
        return val & M32 if r[0] == 'w' else val
    def sgp(self, r, val):
        if r in ('wzr', 'xzr'): return
        self.x[31 if r == 'sp' else int(r[1:])] = (val & M32) if r[0] == 'w' else (val & M64)
    def val(self, s): return imm(s) if s.startswith('#') else self.gp(s)
    def shifted(self, ops, i):
        v = self.val(ops[i])
        if len(ops) > i + 1:
            e = ops[i + 1].split()
            if e[0] == 'lsl': v <<= imm(e[1])
            elif e[0] == 'asr': v = s32(v) >> imm(e[1])
        return v
    def isfp(self, r): return r[0] in 'bhsdqv'
    # ---- SIMD (low bits) ----
    def vget(self, r, bits): return self.v[int(r[1:])] & ((1 << bits) - 1)
    def vset(self, r, val, bits): self.v[int(r[1:])] = val & ((1 << bits) - 1)       # sisa bit dikosongkan (seperti AArch64)
    def sget(self, r): return bits32f(self.vget(r, 32))
    def dget(self, r): return bits64f(self.vget(r, 64))
    # ---- memori ----
    def ea(self, ops, i):
        """operand memori ke-i: [base], [base,#imm], [base,xM,lsl #2], [base,wM,sxtw #2]"""
        parts = split_ops(ops[i].strip('[]'))
        base = self.gp(parts[0]); off = 0
        if len(parts) > 1:
            if parts[1].startswith('#'): off = imm(parts[1])
            else:
                rv = self.gp(parts[1])
                ext = parts[2].split() if len(parts) > 2 else []
                if parts[1][0] == 'w' or (ext and ext[0] == 'sxtw'): rv = s32(rv)
                if ext: rv <<= imm(ext[-1]) if len(ext) > 1 else 0
                off = rv
        return (base + off) & M64
    # ---- bendera ----
    def addsub(self, a, b, bits, sub):
        mask = (1 << bits) - 1; a &= mask; b &= mask
        r = a - b if sub else a + b; res = r & mask
        self.n, self.z = res >> (bits - 1), int(res == 0)
        sa, sb, sr = a >> (bits - 1), b >> (bits - 1), res >> (bits - 1)
        if sub: self.c = int(a >= b); self.vf = int(sa != sb and sr != sa)
        else:   self.c = int(r > mask); self.vf = int(sa == sb and sr != sa)
        return res
    def fcmp(self, a, b):
        if a != a or b != b: self.n, self.z, self.c, self.vf = 0, 0, 1, 1
        elif a == b: self.n, self.z, self.c, self.vf = 0, 1, 1, 0
        elif a < b: self.n, self.z, self.c, self.vf = 1, 0, 0, 0
        else: self.n, self.z, self.c, self.vf = 0, 0, 1, 0
    def cond(self, c):
        n, z, cc, v = self.n, self.z, self.c, self.vf
        return {'eq': z, 'ne': not z, 'hs': cc, 'lo': not cc, 'hi': cc and not z, 'ls': (not cc) or z,
                'ge': n == v, 'lt': n != v, 'gt': (not z) and n == v, 'le': z or n != v}[c]
    # ---- eksekusi ----
    def call(self, target):
        if target not in self.stubs: raise RuntimeError('panggilan ke alamat tanpa stub: %#x' % target)
        self.stubs[target](self)
    def run(self, pc, stop_pcs=None, maxsteps=3_000_000):
        for _ in range(maxsteps):
            if stop_pcs and pc in stop_pcs: return stop_pcs[pc]
            if pc not in self.p: raise RuntimeError('pc di luar fungsi: %#x' % pc)
            op, a = self.p[pc]; nxt = pc + 4
            w = lambda r: 32 if r[0] == 'w' else 64
            if op == 'ret': return 'ret'
            elif op == 'mrs': self.sgp(a[0], self.tls)
            elif op == 'adrp': self.sgp(a[0], imm(a[1]))
            elif op == 'bl':
                tgt = imm(a[0]); self.x[30] = nxt; self.call(tgt)
            elif op == 'blr':
                tgt = self.gp(a[0]); self.x[30] = nxt; self.call(tgt)
            elif op in ('stp', 'ldp'):
                base = self.ea(a, 2)
                for i in range(2):
                    if op == 'stp': self.m.wr(base + 8 * i, self.gp(a[i]), 8)
                    else: self.sgp(a[i], self.m.rd(base + 8 * i, 8))
            elif op in ('ldr', 'ldur', 'ldrb', 'ldurb', 'ldrsw'):
                ea = self.ea(a, 1)
                if op == 'ldrsw': self.sgp(a[0], s32(self.m.rd(ea, 4)))
                elif op in ('ldrb', 'ldurb'): self.sgp(a[0], self.m.rd(ea, 1))
                elif self.isfp(a[0]):
                    sz = {'b': 1, 'h': 2, 's': 4, 'd': 8, 'q': 16}[a[0][0]]; self.v[int(a[0][1:])] = self.m.rd(ea, sz)
                else: self.sgp(a[0], self.m.rd(ea, 8 if a[0][0] == 'x' else 4))
            elif op in ('str', 'stur', 'strb', 'sturb'):
                ea = self.ea(a, 1)
                if op in ('strb', 'sturb'): self.m.wr(ea, self.gp(a[0]), 1)
                elif self.isfp(a[0]):
                    sz = {'b': 1, 'h': 2, 's': 4, 'd': 8, 'q': 16}[a[0][0]]; self.m.wr(ea, self.v[int(a[0][1:])], sz)
                else: self.m.wr(ea, self.gp(a[0]), 8 if a[0][0] == 'x' else 4)
            elif op == 'cmp': self.addsub(self.gp(a[0]), self.val(a[1]), w(a[0]), True)
            elif op == 'subs': self.sgp(a[0], self.addsub(self.gp(a[1]), self.val(a[2]), w(a[0]), True))
            elif op == 'add': self.sgp(a[0], self.gp(a[1]) + self.shifted(a, 2))
            elif op == 'sub': self.sgp(a[0], self.gp(a[1]) - self.shifted(a, 2))
            elif op == 'lsl': self.sgp(a[0], self.gp(a[1]) << imm(a[2]))
            elif op == 'sxtw': self.sgp(a[0], s32(self.gp(a[1])))
            elif op == 'sbfiz':                                   # sbfiz xd, xn, #lsb, #width  (width bit dari bawah, tanda diperluas, lalu geser)
                lsb, wd = imm(a[2]), imm(a[3]); v = self.gp(a[1]) & ((1 << wd) - 1)
                if v >> (wd - 1): v -= 1 << wd
                self.sgp(a[0], v << lsb)
            elif op == 'bic':
                self.sgp(a[0], self.gp(a[1]) & ~self.shifted(a, 2))
            elif op == 'orr':
                b = imm(a[2]) if a[2].startswith('#') else self.gp(a[2]); self.sgp(a[0], self.gp(a[1]) | b)
            elif op == 'mov':
                if a[0].startswith('v'):                          # mov v2.16b, v0.16b
                    self.v[int(a[0][1:].split('.')[0])] = self.v[int(a[1][1:].split('.')[0])]
                else: self.sgp(a[0], self.val(a[1]))
            elif op == 'csel': self.sgp(a[0], self.gp(a[1]) if self.cond(a[3]) else self.gp(a[2]))
            elif op == 'cbz':
                if self.gp(a[0]) == 0: nxt = imm(a[1])
            elif op == 'cbnz':
                if self.gp(a[0]) != 0: nxt = imm(a[1])
            elif op == 'tbz':
                if not (self.gp(a[0]) >> imm(a[1])) & 1: nxt = imm(a[2])
            elif op == 'b': nxt = imm(a[0])
            elif op.startswith('b.'):
                if self.cond(op[2:]): nxt = imm(a[0])
            # ---- float ----
            elif op == 'fmov':
                if a[1].startswith('#'):
                    x = float(a[1].lstrip('#'))
                    if a[0][0] == 's': self.vset(a[0], f32bits(x), 32)
                    else: self.vset(a[0], f64bits(x), 64)
                else: raise RuntimeError('fmov reg tak didukung')
            elif op in ('fadd', 'fsub', 'fmul', 'fdiv'):
                if a[0][0] == 's': x, y = self.sget(a[1]), self.sget(a[2])
                else: x, y = self.dget(a[1]), self.dget(a[2])
                if op == 'fdiv' and y == 0: raise RuntimeError('fdiv by 0')
                r = {'fadd': x + y, 'fsub': x - y, 'fmul': x * y, 'fdiv': (x / y if y else 0.0)}[op]
                if a[0][0] == 's': self.vset(a[0], f32bits(f32(r)), 32)
                else: self.vset(a[0], f64bits(r), 64)
            elif op == 'fcmp':
                x = self.sget(a[0]) if a[0][0] == 's' else self.dget(a[0])
                y = 0.0 if a[1].startswith('#') else (self.sget(a[1]) if a[1][0] == 's' else self.dget(a[1]))
                self.fcmp(x, y)
            elif op == 'fcvt':                                    # fcvt d3, s3  (single -> double)
                assert a[0][0] == 'd' and a[1][0] == 's'; self.vset(a[0], f64bits(self.sget(a[1])), 64)
            elif op == 'scvtf':
                iv = s32(self.gp(a[1])) if a[1][0] == 'w' else self.gp(a[1])
                if a[0][0] == 's': self.vset(a[0], f32bits(f32(float(iv))), 32)
                else: self.vset(a[0], f64bits(float(iv)), 64)
            elif op == 'ucvtf':
                if a[1][0] in 'wx': iv = self.gp(a[1])
                else: iv = self.vget(a[1], 32 if a[1][0] == 's' else 64)
                if a[0][0] == 's': self.vset(a[0], f32bits(f32(float(iv))), 32)
                else: self.vset(a[0], f64bits(float(iv)), 64)
            elif op == 'fcvtzs':
                x = self.sget(a[1]) if a[1][0] == 's' else self.dget(a[1])
                self.sgp(a[0], fcvtzs(x, 32 if a[0][0] == 'w' else 64))
            else: raise RuntimeError('instruksi tak didukung %s %s @%#x' % (op, a, pc))
            pc = nxt
        raise RuntimeError('terlalu banyak langkah')

# ----------------------------------------------------------------------------------------------------------------
# Lingkungan: ctx (instance layerExAlpha), dua layer, vtable, stub
# ----------------------------------------------------------------------------------------------------------------
CTX, VT_CTX, OBJ1, OBJ2, VT_OBJ = 0x100000, 0x110000, 0x120000, 0x121000, 0x122000
NI1, NI2, CLSVAR, GOT = 0x130000, 0x131000, 0x132000, 0x1aa0bd8
F_NIS, F_RESET, F_REDRAW = 0xF0000001, 0xF0000002, 0xF0000003
STACK, TLS = 0x7f0000, 0x7e0000
BUF = {'D': 0x200000, 'L1': 0x300000, 'L2': 0x400000}

def build(prog_entry, ctx_w, ctx_h, ctx_pitch, layers, dest, a_args, nis_ret=0):
    """layers: {'L1': (w,h,pitch,bytes), 'L2': (...)}; dest: bytes (pitch*ctx_h). a_args: dict regs x3..x7."""
    mem = Mem()
    mem.add(STACK, 0x2000, 'stack'); mem.add(TLS, 0x100, 'tls'); mem.wr(TLS + 0x28, 0x1122334455667788, 8)
    mem.add(CTX, 0x40, 'ctx'); mem.add(VT_CTX, 0x20, 'vt_ctx'); mem.add(OBJ1, 8, 'obj1'); mem.add(OBJ2, 8, 'obj2')
    mem.add(VT_OBJ, 0x100, 'vt_obj'); mem.add(CLSVAR, 8, 'cls'); mem.add(GOT, 8, 'got')
    mem.wr(GOT, CLSVAR, 8); mem.wr(CLSVAR, 0x77, 4)
    mem.wr(OBJ1, VT_OBJ, 8); mem.wr(OBJ2, VT_OBJ, 8); mem.wr(VT_OBJ + 0xc8, F_NIS, 8)
    mem.wr(CTX, VT_CTX, 8); mem.wr(VT_CTX, F_RESET, 8); mem.wr(VT_CTX + 8, F_REDRAW, 8)
    mem.wr(CTX + 0x10, ctx_w, 4); mem.wr(CTX + 0x14, ctx_h, 4); mem.wr(CTX + 0x20, ctx_pitch, 4)
    d = mem.add(BUF['D'], max(len(dest), 1), 'dest'); d[:len(dest)] = dest
    mem.wr(CTX + 0x18, BUF['D'], 8)
    info = {}
    for key, ni in (('L1', NI1), ('L2', NI2)):
        w, h, pitch, data = layers[key]
        b = mem.add(BUF[key], max(len(data), 1), key); b[:len(data)] = data
        info[ni] = dict(w=w, h=h, pitch=pitch, buf=BUF[key])
    heap = [0x1000000]
    st = {'redraw': 0, 'throw': None, 'alloc': [], 'frees': 0}
    def s_nis(c):                                                  # NativeInstanceSupport(obj, 2, classid, out)
        obj, out = c.gp('x0'), c.gp('x3')
        assert c.gp('w1') == 2 and c.m.rd(CLSVAR, 4) == c.gp('w2')
        if nis_ret >= 0: c.m.wr(out, NI1 if obj == OBJ1 else NI2, 8)
        c.sgp('x0', nis_ret)                                       # < 0 => gagal: binary memanggil TVPThrowExceptionMessage("Not Layer")
    def s_ptr(c): c.sgp('x0', info[c.gp('x0')]['buf'])             # 0x807c00
    def s_pitch(c): c.sgp('x0', info[c.gp('x0')]['pitch'])         # 0x807c20
    def s_w(c): c.sgp('x0', info[c.gp('x0')]['w'])                 # 0x806828
    def s_h(c): c.sgp('x0', info[c.gp('x0')]['h'])                 # 0x806920
    def s_vec(esz):
        def f(c):
            vec, n = c.gp('x0'), c.gp('x1')
            if n > 0x7FFFFFFF: raise Stop('length_error: vector::_M_default_append n=%d' % n)   # std::length_error di binary
            base = heap[0]; heap[0] += (n * esz + 0xFF) & ~0xF
            c.m.add(base, max(n * esz, 1), 'vec'); st['alloc'].append((base, n * esz))
            c.m.wr(vec, base, 8); c.m.wr(vec + 8, base + n * esz, 8); c.m.wr(vec + 16, base + n * esz, 8)
        return f
    def s_free(c): st['frees'] += 1
    def s_redraw(c): st['redraw'] += 1
    def s_throw(c): raise Stop('throw: TVPThrowExceptionMessage(Not Layer)')
    stubs = {F_NIS: s_nis, F_REDRAW: s_redraw, 0x807c00: s_ptr, 0x807c20: s_pitch, 0x806828: s_w, 0x806920: s_h,
             0x406260: s_vec(4), 0x4228b0: s_vec(4), 0x415b10: s_free, 0x953d0c: s_throw}
    cpu = CPU(prog_entry, mem, stubs); cpu.tls = TLS
    cpu.x[0], cpu.x[1], cpu.x[2], cpu.x[31] = CTX, OBJ1, OBJ2, STACK + 0x1800
    for r, v in a_args.items(): cpu.x[int(r[1:])] = v & M64
    return cpu, mem, st

ENTRY = {4: (0x5d8f94, 0x5d9518), 5: (0x5d9cb8, 0x5da14c)}      # (awal fungsi, cabang ketidakcocokan ukuran)

def run_fn(prog, which, ctx, l1, l2, dest, args, nis_ret=0):
    """Eksekusi satu fungsi utuh. ctx=(w,h,pitch); l1,l2=(w,h,pitch,bytes); dest=bytes; args={'x3':..}.
    -> dict(status: 'ok'|'mismatch'|'<Stop>', dest, redraw)."""
    entry, mis = ENTRY[which]
    cpu, mem, st = build(prog, ctx[0], ctx[1], ctx[2], {'L1': l1, 'L2': l2}, dest, args, nis_ret)
    try: r = cpu.run(entry, {mis: 'mismatch'})
    except Stop as e: return dict(status=str(e), dest=None, redraw=st['redraw'])
    if r == 'mismatch': return dict(status='mismatch', dest=None, redraw=st['redraw'])
    b, o = mem._find(BUF['D'], len(dest))
    return dict(status='ok', dest=bytes(b[o:o + len(dest)]), redraw=st['redraw'])

# ----------------------------------------------------------------------------------------------------------------
# Model (kode yang sama dengan layerExAlphaCore.hpp: ride4()/ride5()), ditulis ulang di Python
# ----------------------------------------------------------------------------------------------------------------
def lohi(a, n):
    lo = 0 if s32(a - n) < 0 else s32(a - n + 1)
    lo = min(lo, 255)
    hi = s32(n + a); hi = 0 if hi < 0 else hi; hi = min(hi, 255)
    return lo, hi

def table4(n, c0, c1, c2):
    m = 2 * n - 1
    fn = f32(float(n)); s5 = f32(1.0 / fn)
    s2, s3, s4 = f32(f32(float(c0)) / fn), f32(f32(float(c1)) / fn), f32(f32(float(c2)) / fn)
    vf, i1, i2, i3 = [0.0] * m, [0] * m, [0] * m, [0] * m
    for i in range(m):
        if i == n - 1:
            vf[i], i1[i], i2[i], i3[i] = 0.0, s32(c0), s32(c1), s32(c2)
        else:
            if i < n - 1: v = f32(s5 * f32(float(n - 1 - i))); t = i + 1
            else:         v = f32(s5 * f32(float(i - n + 1))); t = m - i
            ft = f32(float(t))
            i1[i] = fcvtzs(f32(f32(s2 * ft) + 0.5), 32); i2[i] = fcvtzs(f32(f32(s3 * ft) + 0.5), 32); i3[i] = fcvtzs(f32(f32(s4 * ft) + 0.5), 32)
            vf[i] = 1.0 if v >= 1.0 else v                             # fcmp v,1.0 ; b.lt -> sisanya: v = 1.0
        for arr in (i1, i2, i3):
            if arr[i] >= 255: arr[i] = 255
        if not vf[i] > 0.0: vf[i] = 0.0
        for arr in (i1, i2, i3):
            if arr[i] <= 0: arr[i] = 0
    return vf, i1, i2, i3

def ride4_model(dest, l1, l2, w, h, dpitch, p1, p2, a, n, c0, c1, c2):
    """dest/l1/l2: bytearray; piksel BGRA. c0=byte2(R), c1=byte1(G), c2=byte0(B). -> dest baru"""
    d = bytearray(dest); vf, i1, i2, i3 = table4(n, c0, c1, c2); lo, hi = lohi(a, n)
    I = (i3, i2, i1)                                                   # indeks byte 0,1,2
    for y in range(h):
        for x in range(w):
            do, o1, o2 = y * dpitch + x * 4, y * p1 + x * 4, y * p2 + x * 4
            if d[do + 3] == 0 and l1[o1 + 3] == 0: continue
            a2 = l2[o2 + 3]
            if lo > a2:
                d[do:do + 3] = l1[o1:o1 + 3]
            elif a2 < a:                                               # lo <= a2 < a : double
                idx = a2 - lo
                for k in range(3):
                    t = fcvtzs(float(l1[o1 + k]) * vf[idx] + 0.5, 32) if False else fcvtzs((float(l1[o1 + k]) * float(vf[idx])) + 0.5, 32)
                    d[do + k] = (I[k][idx] + t) & 0xFF
            elif hi <= a2: pass
            else:                                                      # a <= a2 < hi : float
                idx = a2 - lo
                for k in range(3):
                    t = fcvtzs(f32(f32(vf[idx] * f32(float(d[do + k]))) + 0.5), 32)
                    d[do + k] = (I[k][idx] + t) & 0xFF
    return bytes(d)

def table5(n):
    m = 2 * n - 1
    s1 = f32(1.0 / f32(float(n)))
    A, B, C = [0.0] * m, [0.0] * m, [0.0] * m
    for i in range(m):
        if i < n - 1:
            B[i] = f32(s1 * f32(float(n - 1 - i))); C[i] = f32(1.0 - B[i]); A[i] = 1.0
        elif i == n - 1:
            B[i], C[i], A[i] = 0.0, 1.0, 1.0
        else:
            B[i], C[i] = 0.0, 1.0; A[i] = f32(s1 * f32(float(m - i)))
        for arr in (B, C, A):
            if arr[i] >= 1.0: arr[i] = 1.0
        for arr in (B, C, A):
            if not arr[i] > 0.0: arr[i] = 0.0
    return A, B, C

def ride5_model(dest, l1, l2, w, h, dpitch, p1, p2, a, n, xlimit=None):
    """xlimit=None -> perilaku binary: kolom 0 .. 2n-2 (bisa melewati lebar!)."""
    d = bytearray(dest); A, B, C = table5(n); lo, hi = lohi(a, n)
    xc = (2 * n - 1) if xlimit is None else xlimit
    for y in range(h):
        for x in range(xc):
            do, o1, o2 = y * dpitch + x * 4, y * p1 + x * 4, y * p2 + x * 4
            if d[do + 3] == 0 and l1[o1 + 3] == 0: continue
            a2 = l2[o2 + 3]
            if lo > a2: continue
            if a2 < a:
                idx = a2 - lo
                for k in range(3):
                    t1 = fcvtzs(f32(f32(C[idx] * f32(float(l1[o1 + k]))) + 0.5), 32)
                    t2 = fcvtzs(f32(f32(B[idx] * f32(float(d[do + k]))) + 0.5), 32)
                    d[do + k] = (t2 + t1) & 0xFF
            elif hi <= a2: d[do + 3] = 0
            else:
                idx = a2 - lo
                d[do:do + 3] = l1[o1:o1 + 3]
                d[do + 3] = fcvtzs(f32(f32(A[idx] * f32(float(l1[o1 + 3]))) + 0.5), 32) & 0xFF
    return bytes(d)

M64B = M64
def fnv1a(b, h=0xCBF29CE484222325):
    for x in b: h = ((h ^ x) * 0x100000001B3) & M64B
    return h

def lcg_bytes(seed, n):                 # sama persis dengan di test_core.cpp
    st, out = seed & M32, bytearray()
    for _ in range(n):
        st = (st * 1664525 + 1013904223) & M32
        out.append((st >> 24) & 0xFF)
    return bytes(out)

def make_case(seed, w, h, pad=0, alpha_mode=0):
    """Tiga gambar deterministik. pad = byte tambahan di akhir tiap baris (pitch = w*4+pad).
    alpha_mode 0: alfa acak penuh; 1: alfa banyak nol/rendah (agar cabang beragam)."""
    pitch = w * 4 + pad
    imgs = []
    for k in range(3):
        raw = bytearray(lcg_bytes(seed * 3 + k, pitch * h))
        if alpha_mode:
            for y in range(h):
                for x in range(w):
                    o = y * pitch + x * 4 + 3
                    r = raw[o]
                    raw[o] = 0 if (r % 5 == 0 and k != 2) else (r if k == 2 else r % 200)
        imgs.append(bytes(raw))
    return pitch, imgs

# (seed, w, h, padD, pad1, pad2, alpha_mode, a, n, c0, c1, c2)  -- C++ (test_core.cpp) memakai daftar yang SAMA
GOLDEN4 = [
    (11, 40, 30, 0, 0, 0, 1, 128, 20, 200, 100, 50),
    (12, 33, 17, 8, 4, 12, 1, 60, 5, 255, 0, 255),
    (13, 64, 8, 0, 0, 0, 0, 200, 100, 1000, -5, 300),
    (14, 16, 16, 4, 0, 8, 1, 3, 8, 77, 88, 99),
    (15, 50, 5, 0, 8, 0, 1, 255, 1, 10, 20, 30),
    (16, 31, 31, 0, 0, 0, 1, 100, 40, 0, 0, 0),
    (17, 48, 24, 0, 0, 0, 1, 40, 25, 12345, 255, 1),
]
# Rect5: tanpa c0..c2. Kasus dengan 2n-1 > w memakai pad >= (2n-1-w)*4 agar tidak keluar buffer (perilaku binary dipertahankan).
GOLDEN5 = [
    (21, 40, 30, 0, 0, 0, 1, 128, 20, 0, 0, 0),
    (22, 33, 17, 8, 4, 12, 1, 60, 5, 0, 0, 0),
    (23, 64, 8, 0, 0, 0, 0, 200, 30, 0, 0, 0),
    (24, 16, 16, 4, 0, 8, 1, 3, 8, 0, 0, 0),
    (25, 50, 5, 0, 8, 0, 1, 255, 1, 0, 0, 0),
    (26, 10, 6, 24, 24, 24, 1, 20, 8, 0, 0, 0),
    (27, 48, 24, 8, 8, 8, 1, 40, 25, 0, 0, 0),
]

def golden_inputs(cfg):
    seed, w, h, pd, p1_, p2_, mode, a, n, c0, c1, c2 = cfg
    dp, (D, _, _) = make_case(seed, w, h, pd, mode)
    p1, (_, L1, _) = make_case(seed + 1, w, h, p1_, mode)
    p2, (_, _, L2) = make_case(seed + 2, w, h, p2_, mode)
    return w, h, dp, p1, p2, D, L1, L2, a, n, c0, c1, c2

def golden(prog):
    ok = True
    for which, cfgs in ((4, GOLDEN4), (5, GOLDEN5)):
        print('// Rect%d : {seed, w, h, padD, pad1, pad2, mode, a, n, c0, c1, c2, fnv1a(tujuan)}' % which)
        for cfg in cfgs:
            w, h, dp, p1, p2, D, L1, L2, a, n, c0, c1, c2 = golden_inputs(cfg)
            args = {'x3': a, 'x4': n}
            if which == 4: args.update({'x5': c0, 'x6': c1, 'x7': c2})
            r = run_fn(prog, which, (w, h, dp), (w, h, p1, L1), (w, h, p2, L2), D, args)
            exp = (ride4_model(D, L1, L2, w, h, dp, p1, p2, a, n, c0, c1, c2) if which == 4
                   else ride5_model(D, L1, L2, w, h, dp, p1, p2, a, n, None))
            if r['status'] != 'ok' or r['dest'] != exp: ok = False; print('// !! BEDA/GALAT', cfg, r['status'])
            print('\t{%s, 0x%016XULL},' % (', '.join(str(v) for v in cfg), fnv1a(r['dest'] or b'')))
    return ok

def probe_table(prog):
    """Baca nilai tabel warna Rect4 LEWAT piksel dari eksekusi asli (l1 = 0, tujuan = 0 -> dst.k = i_k[idx]).
    n=48, c=(R=100, G=200, B=255): pada indeks tertentu urutan (c/n)*t memberi 62/62/112 sedangkan (c*t)/n memberi 63/63/113."""
    n, a, (c0, c1, c2) = 48, 60, (100, 200, 255)
    lo = lohi(a, n)[0]
    idxs = [29, 14, 26, 65, 68, 47]
    w, h = len(idxs), 1
    D = bytearray(); L1 = bytearray(); L2 = bytearray()
    for idx in idxs:
        D += bytes([0, 0, 0, 255]); L1 += bytes([0, 0, 0, 255]); L2 += bytes([0, 0, 0, lo + idx])
    r = run_fn(prog, 4, (w, h, w * 4), (w, h, w * 4, bytes(L1)), (w, h, w * 4, bytes(L2)), bytes(D), {'x3': a, 'x4': n, 'x5': c0, 'x6': c1, 'x7': c2})
    assert r['status'] == 'ok', r['status']
    vf, i1, i2, i3 = table4(n, c0, c1, c2)
    got = [(r['dest'][4 * j + 2], r['dest'][4 * j + 1], r['dest'][4 * j]) for j in range(w)]       # (R=i1, G=i2, B=i3)
    exp = [(i1[i], i2[i], i3[i]) for i in idxs]
    assert got == exp, (got, exp)
    assert got[0][0] == 62 and got[1][1] == 62 and got[2][1] == 112 and got[3][0] == 62 and got[4][1] == 112, got
    return got

if __name__ == '__main__':
    if len(sys.argv) < 2: sys.exit(__doc__)
    prog = load(sys.argv[1])
    for e in (0x5d8f94, 0x5d9cb8):
        if e not in prog: sys.exit('recon tidak memuat %#x' % e)
    rnd = random.Random(2026); n_ok = n_bad = 0; bad = []; oob = {4: 0, 5: 0}; oob_pred = {4: 0, 5: 0}; spill = 0
    for which in (4, 5):
        for trial in range(500):
            w, h = rnd.randint(1, 9), rnd.randint(1, 6)
            pad, p1pad, p2pad = rnd.choice([0, 0, 4, 8]), rnd.choice([0, 4]), rnd.choice([0, 8])
            a = rnd.choice([rnd.randint(-5, 300), rnd.randint(0, 255), rnd.randint(0, 40)])
            n = rnd.choice([1, 2, 3, 4, 5, 8, 16, 40, 100, 300])
            cs = [rnd.choice([0, 1, 100, 255, 256, 300, -7, 1000]) for _ in range(3)]
            dpitch, (D, _, _) = make_case(rnd.randrange(1 << 20), w, h, pad, rnd.randint(0, 1))
            p1, (_, L1, _) = make_case(rnd.randrange(1 << 20), w, h, p1pad, rnd.randint(0, 1))
            p2, (_, _, L2) = make_case(rnd.randrange(1 << 20), w, h, p2pad, rnd.randint(0, 1))
            args = {'x3': a, 'x4': n}
            if which == 4: args.update({'x5': cs[0], 'x6': cs[1], 'x7': cs[2]})
            try:
                r = run_fn(prog, which, (w, h, dpitch), (w, h, p1, L1), (w, h, p2, L2), D, args)
            except RuntimeError as e:
                if 'luar wilayah' not in str(e): raise
                r = dict(status='oob', dest=None, redraw=0)
            try:
                exp = (ride4_model(D, L1, L2, w, h, dpitch, p1, p2, a, n, *cs) if which == 4
                       else ride5_model(D, L1, L2, w, h, dpitch, p1, p2, a, n, None))
            except IndexError:
                exp = 'oob'
            if which == 5 and 2 * n - 1 > w: spill += 1
            if r['status'] == 'oob': oob[which] += 1
            if exp == 'oob': oob_pred[which] += 1
            ok = (r['status'] == 'oob' and exp == 'oob') or (r['status'] == 'ok' and r['dest'] == exp and r['redraw'] == 1)
            if ok: n_ok += 1
            else: n_bad += 1; bad.append((which, r['status'], w, h, n, a, r['redraw']))
    print('probe tabel (R,G,B) di idx 29,14,26,65,68,47:', probe_table(prog))
    if '--golden' in sys.argv:
        if not golden(prog): n_bad += 1
    print('emulasi: %d cocok, %d beda/galat' % (n_ok, n_bad))
    print('akses di luar buffer (emulator/model): Rect4 %d/%d, Rect5 %d/%d  (kasus Rect5 dengan 2n-1 > lebar: %d)' % (oob[4], oob_pred[4], oob[5], oob_pred[5], spill))
    for b in bad[:12]: print('  ', b)
    sys.exit(1 if n_bad else 0)
