#!/usr/bin/env python3
# k2emu.py - emulator AArch64 MINI untuk menjalankan disassembly hasil k2recon.sh di sandbox/PC (tanpa perangkat).
#
# Tujuan: pembanding MEKANIS terhadap transkripsi manual (spesifikasi + inti C++). Hanya subset instruksi yang
# muncul di recon layerEx (aritmetika, load/store, cabang, csel/ccmp, madd/sdiv). Instruksi lain -> error keras.
# Panggilan keluar fungsi (bl/blr/br ke alamat bukan bagian fungsi) ditangani HOOK Python, bukan dijalankan.
#
# Yang dibuktikan: hanya bahwa kode C++ sama dengan yang dihasilkan instruksi di recon. BUKAN uji di perangkat
# dan BUKAN verifikasi arti helper (helper = hook yang diasumsikan, lihat docs/spec/layerExAlpha_umum.md).
# Jangan commit recon_*.txt (output disassembler); skrip ini membacanya dari path argumen.
import re, sys, struct

M64 = (1 << 64) - 1
M32 = (1 << 32) - 1


def sx(v, bits):
    v &= (1 << bits) - 1
    return v - (1 << bits) if v >> (bits - 1) else v


class Stop(Exception):
    pass


class EmuError(Exception):
    pass


class Mem:
    """Memori jarang. Rentang 'guard' mencatat setiap akses (untuk mendeteksi baca/tulis di luar batas)."""

    def __init__(self):
        self.b = {}
        self.guards = []      # (lo, hi, nama)
        self.violations = []

    def _chk(self, a, n, kind):
        for lo, hi, nm in self.guards:
            if a < hi and a + n > lo:
                self.violations.append((kind, a, n, nm))

    def rd(self, a, n):
        self._chk(a, n, 'R')
        v = 0
        for i in range(n):
            v |= self.b.get(a + i, 0) << (8 * i)
        return v

    def wr(self, a, n, v):
        self._chk(a, n, 'W')
        for i in range(n):
            self.b[a + i] = (v >> (8 * i)) & 0xFF

    def load(self, a, data):
        for i, x in enumerate(data):
            self.b[a + i] = x

    def dump(self, a, n):
        return bytes(self.b.get(a + i, 0) for i in range(n))


COND = {
    'eq': lambda n, z, c, v: z, 'ne': lambda n, z, c, v: not z,
    'hs': lambda n, z, c, v: c, 'cs': lambda n, z, c, v: c,
    'lo': lambda n, z, c, v: not c, 'cc': lambda n, z, c, v: not c,
    'mi': lambda n, z, c, v: n, 'pl': lambda n, z, c, v: not n,
    'vs': lambda n, z, c, v: v, 'vc': lambda n, z, c, v: not v,
    'hi': lambda n, z, c, v: c and not z, 'ls': lambda n, z, c, v: (not c) or z,
    'ge': lambda n, z, c, v: n == v, 'lt': lambda n, z, c, v: n != v,
    'gt': lambda n, z, c, v: (not z) and n == v, 'le': lambda n, z, c, v: z or n != v,
}


def parse_recon(path, start_hex):
    """Ambil instruksi fungsi yang diawali '=== fungsi 0x<start>' sampai fungsi/METHOD berikutnya."""
    ins = {}
    cur = False
    for ln in open(path, encoding='utf-8', errors='replace'):
        ln = ln.rstrip('\n')
        if ln.startswith('=== fungsi'):
            cur = ln.split()[2].lower() == '0x' + start_hex.lower()
            continue
        if ln.startswith('#####'):
            cur = False
            continue
        if not cur:
            continue
        m = re.match(r'^\s*([0-9a-f]+):\s*(.*)$', ln)
        if not m:
            continue
        a = int(m.group(1), 16)
        txt = m.group(2).replace('\t', ' ')
        txt = re.sub(r'\s+<.*$', '', txt)          # buang anotasi simbol objdump
        txt = txt.split('//')[0].strip()           # buang komentar
        ins[a] = txt
    return ins


class CPU:
    def __init__(self, mem, hooks):
        self.x = [0] * 31
        self.sp = 0
        self.n = self.z = self.c = self.v = False
        self.mem = mem
        self.hooks = hooks           # alamat -> fn(cpu) -> nilai x0 (atau None)
        self.steps = 0
        self.stopped = False

    # --- register ---
    def getr(self, name):
        if name in ('xzr',): return 0
        if name in ('wzr',): return 0
        if name == 'sp': return self.sp
        k = int(name[1:])
        return self.x[k] & (M64 if name[0] == 'x' else M32)

    def setr(self, name, val):
        if name in ('xzr', 'wzr'): return
        if name == 'sp':
            self.sp = val & M64
            return
        k = int(name[1:])
        self.x[k] = (val & M64) if name[0] == 'x' else (val & M32)

    def width(self, name):
        return 64 if name[0] in 'x' or name == 'sp' else 32

    # --- operan ---
    def isreg(self, s):
        return re.fullmatch(r'[xw](\d+|zr)|sp', s) is not None

    def val(self, s, ext=None):
        s = s.strip()
        if s.startswith('#'):
            return int(s[1:], 0)
        return self.getr(s)

    def shifted(self, parts, bits):
        """parts = ['wm'] atau ['wm','asr #31'] atau ['wm','sxtw'] dst."""
        v = self.val(parts[0])
        mask = (1 << bits) - 1
        if len(parts) > 1:
            op = parts[1].split()
            k = op[0]
            n = int(op[1][1:], 0) if len(op) > 1 else 0
            if k == 'lsl': v = (v << n) & mask
            elif k == 'lsr': v = (v & mask) >> n
            elif k == 'asr': v = (sx(v, bits) >> n) & mask
            elif k == 'sxtw': v = sx(v, 32) & mask
            elif k == 'uxtb': v = v & 0xFF
            elif k == 'uxtw': v = v & M32
            else: raise EmuError('shift/ext ' + k)
        return v & mask

    def setflags_sub(self, a, b, bits):
        mask = (1 << bits) - 1
        a &= mask; b &= mask
        r = (a - b) & mask
        self.n = bool(r >> (bits - 1)); self.z = r == 0
        self.c = a >= b
        sa, sb, sr = sx(a, bits), sx(b, bits), sx(r, bits)
        self.v = (sa - sb) != sr
        return r

    def setflags_add(self, a, b, bits):
        mask = (1 << bits) - 1
        a &= mask; b &= mask
        r = (a + b) & mask
        self.n = bool(r >> (bits - 1)); self.z = r == 0
        self.c = (a + b) > mask
        self.v = (sx(a, bits) + sx(b, bits)) != sx(r, bits)
        return r

    def cond(self, cc):
        return COND[cc](self.n, self.z, self.c, self.v)

    # --- memori ---
    def addr(self, mtxt, post=None):
        """'[x8, #0x10]', '[x8, x10]', '[x10, x9, lsl #2]', '[x8, #-0x1]', '[sp, #-0x20]!' -> (alamat, wb_reg, wb_val)"""
        m = re.fullmatch(r'\[(.*?)\](!?)', mtxt.strip())
        inner = [p.strip() for p in m.group(1).split(',')]
        pre = m.group(2) == '!'
        base = inner[0]
        b = self.getr(base)
        if len(inner) == 1:
            off = 0
        elif inner[1].startswith('#'):
            off = int(inner[1][1:], 0)
        else:
            rn = inner[1]
            off = self.val(rn)
            if self.width(rn) == 32: off &= M32
            if len(inner) > 2:
                op = inner[2].split()
                if op[0] == 'lsl': off = off << int(op[1][1:], 0)
                elif op[0] == 'sxtw': off = sx(off, 32)
        a = (b + off) & M64
        if pre:
            return a, base, a
        if post is not None:
            return b, base, (b + post) & M64
        return a, None, None

    # --- execução ---
    def run(self, ins, entry, maxsteps=2000000):
        pc = entry
        self.fn_lo, self.fn_hi = min(ins), max(ins) + 4
        while True:
            self.steps += 1
            if self.steps > maxsteps:
                raise EmuError('terlalu banyak langkah')
            if pc not in ins:
                raise EmuError('PC di luar fungsi: 0x%x' % pc)
            t = ins[pc]
            npc = self.step(t, pc, ins)
            if self.stopped:
                return
            pc = npc

    def call_hook(self, target):
        h = self.hooks.get(target)
        if h is None:
            raise EmuError('panggilan tanpa hook: 0x%x' % target)
        r = h(self)
        if r is not None:
            self.x[0] = r & M64

    def step(self, t, pc, ins):
        mn, _, rest = t.partition(' ')
        ops = self.split_ops(rest)
        nxt = pc + 4
        g = self.getr
        s = self.setr
        W = lambda r: 64 if r[0] == 'x' else 32

        if mn == 'ret':
            self.stopped = True
            return nxt
        if mn == 'mov':
            s(ops[0], self.val(ops[1])); return nxt
        if mn == 'movk':
            d = ops[0]; imm = int(ops[1][1:], 0); sh = int(ops[2].split()[1][1:], 0) if len(ops) > 2 else 0
            cur = g(d); cur = (cur & ~(0xFFFF << sh)) | (imm << sh); s(d, cur); return nxt
        if mn in ('add', 'sub'):
            d = ops[0]; bits = W(d)
            a = g(ops[1])
            if self.isreg(ops[1]) and ops[1][0] == 'w' and bits == 64: raise EmuError('add x,w')
            b = self.shifted(ops[2:], bits) if self.isreg(ops[2]) else int(ops[2][1:], 0)
            r = (a + b if mn == 'add' else a - b) & ((1 << bits) - 1)
            s(d, r); return nxt
        if mn == 'cmp':
            a = g(ops[0]); bits = W(ops[0])
            b = self.shifted(ops[1:], bits) if self.isreg(ops[1]) else int(ops[1][1:], 0)
            self.setflags_sub(a, b, bits); return nxt
        if mn == 'cmn':
            a = g(ops[0]); bits = W(ops[0])
            b = self.shifted(ops[1:], bits) if self.isreg(ops[1]) else int(ops[1][1:], 0)
            self.setflags_add(a, b, bits); return nxt
        if mn == 'ccmp':
            a = g(ops[0]); bits = W(ops[0])
            b = self.shifted([ops[1]], bits) if self.isreg(ops[1]) else int(ops[1][1:], 0)
            nzcv = int(ops[2][1:], 0)
            if self.cond(ops[3]):
                self.setflags_sub(a, b, bits)
            else:
                self.n, self.z, self.c, self.v = bool(nzcv & 8), bool(nzcv & 4), bool(nzcv & 2), bool(nzcv & 1)
            return nxt
        if mn in ('and', 'orr', 'eor', 'bic'):
            d = ops[0]; bits = W(d); mask = (1 << bits) - 1
            a = g(ops[1])
            if self.isreg(ops[2]):
                b = self.shifted(ops[2:], bits)
            else:
                b = int(ops[2][1:], 0) & mask
            r = {'and': a & b, 'orr': a | b, 'eor': a ^ b, 'bic': a & ~b}[mn] & mask
            s(d, r); return nxt
        if mn == 'mvn':
            d = ops[0]; bits = W(d); s(d, ~self.shifted(ops[1:], bits)); return nxt
        if mn in ('lsl', 'lsr'):
            d = ops[0]; bits = W(d); a = g(ops[1]); n = int(ops[2][1:], 0)
            s(d, (a << n) if mn == 'lsl' else (a >> n)); return nxt
        if mn == 'sxtw':
            s(ops[0], sx(g(ops[1]), 32) & M64); return nxt
        if mn == 'mul':
            s(ops[0], g(ops[1]) * g(ops[2])); return nxt
        if mn == 'madd':
            s(ops[0], g(ops[1]) * g(ops[2]) + g(ops[3])); return nxt
        if mn == 'smull':
            s(ops[0], (sx(g(ops[1]), 32) * sx(g(ops[2]), 32)) & M64); return nxt
        if mn == 'umull':
            s(ops[0], (g(ops[1]) & M32) * (g(ops[2]) & M32)); return nxt
        if mn == 'sdiv':
            d = ops[0]; bits = W(d)
            a, b = sx(g(ops[1]), bits), sx(g(ops[2]), bits)
            if b == 0: r = 0                       # AArch64: pembagian nol -> 0 (tanpa trap)
            else:
                r = abs(a) // abs(b)
                if (a < 0) != (b < 0): r = -r
            s(d, r); return nxt
        if mn == 'bfi':
            d = ops[0]; lsb = int(ops[2][1:], 0); wid = int(ops[3][1:], 0)
            bits = W(d); m = ((1 << wid) - 1) << lsb
            cur = g(d); src = (g(ops[1]) << lsb) & m
            s(d, (cur & ~m) | src); return nxt
        if mn in ('csel', 'csinv', 'csneg'):
            d = ops[0]; bits = W(d); mask = (1 << bits) - 1
            if self.cond(ops[3]): r = g(ops[1])
            else:
                o = g(ops[2])
                r = o if mn == 'csel' else (~o if mn == 'csinv' else -o)
            s(d, r & mask); return nxt
        if mn == 'csetm':
            s(ops[0], -1 if self.cond(ops[1]) else 0); return nxt
        if mn == 'cset':
            s(ops[0], 1 if self.cond(ops[1]) else 0); return nxt
        if mn == 'cinc':
            r = g(ops[1]) + (1 if self.cond(ops[2]) else 0); s(ops[0], r); return nxt
        if mn == 'adrp':
            s(ops[0], int(ops[1], 16)); return nxt          # objdump sudah memberi alamat halaman
        if mn == 'mrs':
            s(ops[0], 0x900000); return nxt                 # TPIDR_EL0 palsu (jauh dari buffer uji)
        # --- load / store ---
        if mn in ('ldr', 'ldrb', 'ldurb', 'ldrsw', 'ldarb', 'str', 'strb', 'sturb', 'sturh', 'stp', 'ldp'):
            return self.ldst(mn, ops, rest, nxt)
        # --- cabang ---
        if mn == 'b':
            tgt = int(ops[0], 16)
            if tgt in ins: return tgt
            self.call_hook(tgt); self.stopped = True; return nxt      # tail call keluar
        if mn.startswith('b.'):
            tgt = int(ops[0], 16)
            return tgt if self.cond(mn[2:]) else nxt
        if mn == 'cbz':
            return int(ops[1], 16) if g(ops[0]) == 0 else nxt
        if mn == 'cbnz':
            return int(ops[1], 16) if g(ops[0]) != 0 else nxt
        if mn == 'tbz':
            return int(ops[2], 16) if not (g(ops[0]) >> int(ops[1][1:], 0)) & 1 else nxt
        if mn == 'tbnz':
            return int(ops[2], 16) if (g(ops[0]) >> int(ops[1][1:], 0)) & 1 else nxt
        if mn == 'bl':
            tgt = int(ops[0], 16)
            if tgt in ins: raise EmuError('bl rekursif')
            self.x[30] = nxt; self.call_hook(tgt); return nxt
        if mn == 'blr':
            self.call_hook(g(ops[0])); return nxt
        if mn == 'br':
            tgt = g(ops[0])
            if tgt in ins: return tgt
            if tgt in self.hooks:                     # jump tabel / tail call
                self.call_hook(tgt); self.stopped = True; return nxt
            raise EmuError('br ke 0x%x' % tgt)
        raise EmuError('instruksi tidak didukung: %s' % t)

    @staticmethod
    def split_ops(rest):
        out, depth, cur = [], 0, ''
        for ch in rest:
            if ch == '[': depth += 1
            if ch == ']': depth -= 1
            if ch == ',' and depth == 0:
                out.append(cur.strip()); cur = ''
            else:
                cur += ch
        if cur.strip(): out.append(cur.strip())
        return out

    def ldst(self, mn, ops, rest, nxt):
        g, s = self.getr, self.setr
        if mn in ('stp', 'ldp'):
            # stp x20, x19, [sp, #0x40]  /  ldp ..., [sp], #0x30  /  stp ..., [sp, #-0x50]!
            r1, r2 = ops[0], ops[1]
            post = int(ops[3][1:], 0) if len(ops) > 3 else None
            a, wb, wv = self.addr(ops[2], post)
            sz = 8 if r1[0] == 'x' else 4
            if mn == 'stp':
                self.mem.wr(a, sz, g(r1)); self.mem.wr(a + sz, sz, g(r2))
            else:
                s(r1, self.mem.rd(a, sz)); s(r2, self.mem.rd(a + sz, sz))
            if wb: s(wb, wv)
            return nxt
        r = ops[0]
        post = int(ops[2][1:], 0) if len(ops) > 2 else None
        a, wb, wv = self.addr(ops[1], post)
        if mn in ('ldr',):
            sz = 8 if r[0] == 'x' else 4
            s(r, self.mem.rd(a, sz))
        elif mn in ('ldrb', 'ldurb', 'ldarb'):
            s(r, self.mem.rd(a, 1))
        elif mn == 'ldrsw':
            s(r, sx(self.mem.rd(a, 4), 32) & M64)
        elif mn == 'str':
            self.mem.wr(a, 8 if r[0] == 'x' else 4, g(r))
        elif mn in ('strb', 'sturb'):
            self.mem.wr(a, 1, g(r) & 0xFF)
        elif mn == 'sturh':
            self.mem.wr(a, 2, g(r) & 0xFFFF)
        else:
            raise EmuError(mn)
        if wb: s(wb, wv)
        return nxt
