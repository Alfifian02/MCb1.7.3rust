#!/usr/bin/env python3
"""k2dis.py - disassembly AArch64 dengan capstone (pengganti objdump bila tidak ada binutils aarch64).
Pakai: python3 script/k2dis.py LIB.so START SIZE [START SIZE ...]   (hex tanpa/dengan 0x)
Keluaran: satu baris per instruksi 'alamat: mnemonic operand', plus komentar:
  - adrp+add/ldr yang selesai dihitung -> alamat; bila menunjuk string ASCII/UTF-16 pendek -> isinya
  - bl/b target
Vaddr == file offset segmen PT_LOAD (base 0). Hanya untuk dibaca manusia; bukan penerjemah ke C."""
import sys, struct
from capstone import Cs, CS_ARCH_ARM64, CS_MODE_ARM
from capstone.arm64 import ARM64_OP_REG, ARM64_OP_IMM

def load_segments(data):
    assert data[:4] == b'\x7fELF' and data[4] == 2
    phoff, = struct.unpack_from('<Q', data, 0x20)
    phentsize, phnum = struct.unpack_from('<HH', data, 0x36)
    segs = []
    for i in range(phnum):
        t, fl, off, va, pa, fsz, msz, al = struct.unpack_from('<IIQQQQQQ', data, phoff + i * phentsize)
        if t == 1: segs.append((va, off, fsz))
    return segs

def va2off(segs, va):
    for v, o, s in segs:
        if v <= va < v + s: return o + (va - v)
    return None

def peek_string(data, segs, va):
    o = va2off(segs, va)
    if o is None: return None
    b = data[o:o + 96]
    if len(b) >= 4 and b[1] == 0 and 32 <= b[0] < 127:      # UTF-16LE
        s = ''
        for i in range(0, len(b) - 1, 2):
            c = b[i] | (b[i + 1] << 8)
            if c == 0: break
            if not (32 <= c < 127): return None
            s += chr(c)
        return 'u"%s"' % s if len(s) >= 2 else None
    if 32 <= b[0] < 127:                                    # ASCII
        s = ''
        for c in b:
            if c == 0: break
            if not (32 <= c < 127): return None
            s += chr(c)
        return '"%s"' % s if len(s) >= 3 else None
    return None

def main():
    lib = sys.argv[1]
    data = open(lib, 'rb').read()
    segs = load_segments(data)
    md = Cs(CS_ARCH_ARM64, CS_MODE_ARM); md.detail = True
    args = sys.argv[2:]
    for k in range(0, len(args), 2):
        start = int(args[k], 16); size = int(args[k + 1], 0)
        off = va2off(segs, start)
        print('\n=== fungsi 0x%x  ukuran=%d ===' % (start, size))
        pages = {}
        for ins in md.disasm(data[off:off + size], start):
            cmt = ''
            ops = ins.op_str
            if ins.mnemonic == 'adrp':
                pages[ins.operands[0].reg] = ins.operands[1].imm
            elif ins.mnemonic == 'add' and len(ins.operands) == 3 and ins.operands[2].type == ARM64_OP_IMM \
                    and ins.operands[1].type == ARM64_OP_REG and ins.operands[1].reg in pages:
                tgt = pages[ins.operands[1].reg] + ins.operands[2].imm
                s = peek_string(data, segs, tgt)
                cmt = '  ; ->0x%x%s' % (tgt, ' ' + s if s else '')
            elif ins.mnemonic in ('bl', 'b') and ins.operands and ins.operands[0].type == ARM64_OP_IMM:
                cmt = '  ; target 0x%x' % ins.operands[0].imm
            print('  %x:\t%-8s %s%s' % (ins.address, ins.mnemonic, ops, cmt))

if __name__ == '__main__':
    main()
