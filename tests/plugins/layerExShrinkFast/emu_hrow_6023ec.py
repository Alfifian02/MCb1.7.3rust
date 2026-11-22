#!/usr/bin/env python3
"""Emulator AArch64 mini: mengeksekusi disassembly asli 0x6023ec (pengecil baris horizontal shrinkCopyFast).

Pemakaian:
    python3 tests/plugins/layerExShrinkFast/emu_hrow_6023ec.py <recon_6023ec.txt> [--golden]

Berkas recon (keluaran `bash script/k2recon.sh 0x6023ec 0`) TIDAK ada di repo (keluaran disassembler tidak
di-commit). Skrip ini hanya membaca baris `alamat: mnemonik operand` darinya, mengeksekusinya pada sumber acak,
dan membandingkan hasilnya dengan model hrow (rata-rata kotak, udiv 32-bit memotong, alfa 0xFF) --
model yang sama dengan `hrow()` di src/plugins/layerExShrinkFastCore.hpp.
Selain nilai keluaran, diperiksa: tidak ada baca di luar [src, src+sw*4), tidak ada baca byte alfa (byte 3),
tidak ada tulis di luar [dst, dst+dw*4), dan tiap byte tujuan ditulis tepat sekali.
Hanya himpunan instruksi yang dipakai fungsi ini yang didukung; instruksi lain -> berhenti dengan galat.
`--golden` mencetak ringkasan FNV-1a yang ditanam di test_core.cpp (bagian [BINARY-H]).
"""
import re, sys, random

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
    prog, order = {}, []
    for ln in open(path, encoding='utf-8', errors='replace'):
        m = LINE.match(ln.rstrip('\n'))
        if not m: continue
        a = int(m.group(1), 16)
        ops = split_ops(re.split(r'\s+<', m.group(3), 1)[0].split('//')[0].strip())   # buang ' <simbol+off>' (bisa memuat koma/'>' bersarang)
        prog[a] = (m.group(2), ops); order.append(a)
    if 0x6023ec not in prog: sys.exit('recon tidak memuat 0x6023ec')
    return prog

def imm(s): return int(s.lstrip('#'), 0)

class CPU:
    def __init__(self, prog):
        self.p, self.x, self.mem = prog, [0] * 32, {}
        self.n = self.z = self.c = self.v = 0
        self.reads, self.writes = [], []
    # ---- register ----
    def get(self, r):
        if r in ('wzr', 'xzr'): return 0
        v = self.x[31 if r == 'sp' else int(r[1:])]
        return v & M32 if r[0] == 'w' else v
    def set(self, r, v):
        if r in ('wzr', 'xzr'): return
        v = (v & M32) if r[0] == 'w' else (v & M64)
        self.x[31 if r == 'sp' else int(r[1:])] = v
    def val(self, s):                       # register atau imediat
        return imm(s) if s.startswith('#') else self.get(s)
    def shifted(self, ops, i):              # operand ke-i, dengan "lsl #n" opsional di i+1
        v = self.val(ops[i])
        if len(ops) > i + 1 and ops[i + 1].startswith('lsl'): v <<= imm(ops[i + 1].split()[1])
        return v
    # ---- memori ----
    def addr(self, s):
        m = re.match(r'\[(\w+)(?:,\s*(#-?0x[0-9a-f]+|#-?\d+))?\](!?)$', s)
        return self.get(m.group(1)), (imm(m.group(2)) if m.group(2) else 0), m.group(3) == '!'
    def ld(self, a, size):
        for i in range(size):
            if (a + i) not in self.mem: raise RuntimeError('baca memori tak terdefinisi %#x' % (a + i))
            self.reads.append(a + i)
        return sum(self.mem[a + i] << (8 * i) for i in range(size))
    def st(self, a, v, size):
        for i in range(size): self.mem[a + i] = (v >> (8 * i)) & 0xFF; self.writes.append(a + i)
    # ---- bendera ----
    def addsub(self, a, b, bits, sub):
        mask = (1 << bits) - 1; a &= mask; b &= mask
        r = (a - b if sub else a + b)
        res = r & mask
        self.n, self.z = res >> (bits - 1), int(res == 0)
        sa, sb, sr = a >> (bits - 1), b >> (bits - 1), res >> (bits - 1)
        if sub: self.c = int(a >= b); self.v = int(sa != sb and sr != sa)
        else:   self.c = int(r > mask); self.v = int(sa == sb and sr != sa)
        return res
    def cond(self, c):
        n, z, cc, v = self.n, self.z, self.c, self.v
        return {'eq': z, 'ne': not z, 'hs': cc, 'lo': not cc, 'hi': cc and not z, 'ls': (not cc) or z,
                'ge': n == v, 'lt': n != v, 'gt': (not z) and n == v, 'le': z or n != v}[c]
    # ---- eksekusi ----
    def run(self, pc, stop_sp, maxsteps=2_000_000):
        for _ in range(maxsteps):
            if pc not in self.p: raise RuntimeError('pc di luar fungsi: %#x' % pc)
            op, a = self.p[pc]; nxt = pc + 4
            bits = lambda r: 32 if r[0] == 'w' else 64
            if op == 'ret': return
            elif any('[sp' in o for o in a):
                pass                                    # prolog/epilog (str/stp/ldp/ldr berbasis sp): register callee-saved, tak berpengaruh
            elif op in ('strb', 'ldrb', 'ldurb', 'ldr', 'ldrsw'):
                base, off, _ = self.addr(a[1]); ea = (base + off) & M64
                if op == 'strb': self.st(ea, self.get(a[0]), 1)
                elif op == 'ldrsw':
                    v = self.ld(ea, 4); self.set(a[0], v - (1 << 32) if v >> 31 else v)
                else: self.set(a[0], self.ld(ea, {'ldr': 4}.get(op, 1)))   # ldr wN (32-bit); ldrb/ldurb 8-bit
            elif op == 'cmp': self.addsub(self.get(a[0]), self.val(a[1]), bits(a[0]), True)
            elif op == 'cmn': self.addsub(self.get(a[0]), self.val(a[1]), bits(a[0]), False)
            elif op == 'subs': self.set(a[0], self.addsub(self.get(a[1]), self.val(a[2]), bits(a[0]), True))
            elif op == 'add': self.set(a[0], self.get(a[1]) + self.shifted(a, 2))
            elif op == 'sub': self.set(a[0], self.get(a[1]) - self.shifted(a, 2))
            elif op == 'lsl': self.set(a[0], self.get(a[1]) << imm(a[2]))
            elif op == 'sxtw':
                v = self.get(a[1]) & M32; self.set(a[0], v - (1 << 32) if v >> 31 else v)
            elif op == 'mvn': self.set(a[0], ~self.get(a[1]))
            elif op == 'and': self.set(a[0], self.get(a[1]) & imm(a[2]))
            elif op == 'orr': self.set(a[0], self.get(a[1]) | imm(a[2]))
            elif op == 'mov': self.set(a[0], self.val(a[1]))
            elif op == 'udiv':
                d = self.get(a[2]); self.set(a[0], 0 if d == 0 else self.get(a[1]) // d)
            elif op == 'csinv': self.set(a[0], self.get(a[1]) if self.cond(a[3]) else ~self.get(a[2]))
            elif op == 'cbz': 
                if self.get(a[0]) == 0: nxt = imm(a[1])
            elif op == 'cbnz':
                if self.get(a[0]) != 0: nxt = imm(a[1])
            elif op == 'b': nxt = imm(a[0])
            elif op.startswith('b.'):
                if self.cond(op[2:]): nxt = imm(a[0])
            else: raise RuntimeError('instruksi tak didukung %s %s @%#x' % (op, a, pc))
            pc = nxt
        raise RuntimeError('terlalu banyak langkah (loop tak berhenti?)')

# ---- model (sama dengan hrow() di layerExShrinkFastCore.hpp) ----
def model_hrow(src, hs, quot, rem):
    out = bytearray()
    def box(px):
        ch = [sum(p[c] for p in px) // len(px) for c in range(3)]    # udiv 32-bit memotong
        return bytes(ch + [0xFF])
    pix = [src[i:i + 4] for i in range(0, len(src), 4)]
    for i in range(quot): out += box(pix[i * hs:(i + 1) * hs])
    if rem > 0: out += box(pix[quot * hs:quot * hs + rem])
    return bytes(out)

def emulate(prog, src, sw, hs):
    quot, rem = sw // hs, sw % hs
    dw = quot + (1 if rem else 0)
    SRC, DST, CTX, SP = 0x100000, 0x200000, 0x300000, 0x7FF000
    cpu = CPU(prog)
    for i, b in enumerate(src): cpu.mem[SRC + i] = b
    for i in range(dw * 4): cpu.mem[DST + i] = 0xEE                     # racun tujuan; 0xEE = belum ditulis
    ctx = bytearray(0x30)
    for off, v in ((0x10, hs), (0x14, 1), (0x18, quot), (0x1c, rem), (0x20, sw)):
        ctx[off:off + 4] = (v & M32).to_bytes(4, 'little')
    for i, b in enumerate(ctx): cpu.mem[CTX + i] = b
    cpu.x[0], cpu.x[1], cpu.x[2], cpu.x[31] = CTX, DST, SRC, SP
    cpu.writes.clear(); cpu.reads.clear()
    cpu.run(0x6023ec, SP)
    got = bytes(cpu.mem[DST + i] for i in range(dw * 4))
    ws = list(cpu.writes)
    rs = [r for r in cpu.reads if not (CTX <= r < CTX + 0x30)]
    assert all(DST <= w < DST + dw * 4 for w in ws), 'tulis di luar baris tujuan'
    assert len(ws) == len(set(ws)) == dw * 4, 'tiap byte tujuan harus ditulis tepat sekali (%d/%d)' % (len(set(ws)), dw * 4)
    assert all(SRC <= r < SRC + sw * 4 for r in rs), 'baca di luar baris sumber'
    assert all((r - SRC) % 4 != 3 for r in rs), 'alfa sumber ikut dibaca'
    assert len(rs) == len(set(rs)) == sw * 3, 'tiap byte B,G,R sumber harus dibaca tepat sekali'
    return got, model_hrow(src, hs, quot, rem)

def fnv1a(b, h=0xCBF29CE484222325):
    for x in b: h = ((h ^ x) * 0x100000001B3) & M64
    return h

def lcg_bytes(seed, n):                 # sama persis dengan di test_core.cpp
    s, out = seed & M32, bytearray()
    for _ in range(n):
        s = (s * 1664525 + 1013904223) & M32
        out.append((s >> 24) & 0xFF)
    return bytes(out)

if __name__ == '__main__':
    if len(sys.argv) < 2: sys.exit(__doc__)
    prog = load(sys.argv[1])
    n = bad = 0
    rnd = random.Random(1)
    for sw in range(0, 41):
        for hs in range(1, 14):
            for trial in range(3):
                src = bytes(rnd.randrange(256) for _ in range(sw * 4)) if trial else (b'\xff' * (sw * 4))
                got, exp = emulate(prog, src, sw, hs)
                n += 1
                if got != exp: bad += 1; print('BEDA sw=%d hs=%d' % (sw, hs), got.hex(), exp.hex())
    for sw, hs in ((255, 7), (256, 256), (257, 16), (1000, 3), (1023, 1000), (4096, 64)):    # ukuran lebih besar
        src = lcg_bytes(sw * 31 + hs, sw * 4)
        got, exp = emulate(prog, src, sw, hs); n += 1
        if got != exp: bad += 1; print('BEDA sw=%d hs=%d' % (sw, hs))
    print('emulasi %d kasus, %d beda' % (n, bad))
    if '--golden' in sys.argv:
        print('// golden (FNV-1a keluaran baris, sumber = lcg_bytes(seed=sw*31+hs, sw*4))')
        for sw, hs in ((1, 1), (7, 3), (8, 3), (9, 3), (2, 5), (10, 2), (13, 5), (16, 4), (17, 4), (64, 7), (255, 7), (1000, 3), (1023, 1000)):
            src = lcg_bytes(sw * 31 + hs, sw * 4)
            got, _ = emulate(prog, src, sw, hs)
            print('\t{%d, %d, 0x%016XULL},' % (sw, hs, fnv1a(got)))
    sys.exit(1 if bad else 0)
