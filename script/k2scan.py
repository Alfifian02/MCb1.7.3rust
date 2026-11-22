#!/usr/bin/env python3
"""k2scan.py - pindai STRUKTURAL satu fungsi di libgame.so arm64 (tanpa disassembler).

Dipakai ketika recon (objdump) belum tersedia. BUKAN pengganti membaca disassembly:
hanya melaporkan fakta yang bisa didekode dari byte -
  * referensi adrp+add  -> string UTF-16 (bila terbaca) / awal fungsi lain / tabel data
  * tujuan B/BL         -> daftar callee (dan lompatan dalam fungsi)
  * kelas instruksi kasar (bit 28:25): FP/SIMD skalar, vektor SIMD, load/store FP/SIMD
Pakai:  python3 script/k2scan.py 0x5eedb8 [--lib PATH] [--size N] [--insn]
"""
import argparse, os, struct, sys
from collections import Counter
HERE = os.path.dirname(os.path.abspath(__file__)); sys.path.insert(0, HERE)
import k2map as K


def utf16(d, segs, va, n=64):
    o = K.va2off(segs, va)
    if o is None:
        return None
    out = []
    for i in range(n):
        c = struct.unpack_from("<H", d, o + 2 * i)[0]
        if c == 0:
            break
        if not 0x20 <= c < 0x7F:
            return None
        out.append(chr(c))
    return "".join(out) or None


def cls(w):
    g = (w >> 25) & 0xF
    if g == 0x7 and not (w >> 31):
        return "simd-vec"
    if g == 0xF:
        return "fp/simd-skalar"
    if g in (0x4, 0x6, 0xC, 0xE) and (w & 0x04000000):
        return "ldst-fp/simd"
    return "lain"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("addr")
    ap.add_argument("--lib", default=os.path.join(HERE, "..", "Kirikiroid2_1.3.9", "lib", "arm64-v8a", "libgame.so"))
    ap.add_argument("--size", type=int)
    ap.add_argument("--insn", action="store_true", help="cetak kata non-'lain'")
    a = ap.parse_args()
    d, segs = K.load_elf(a.lib)
    starts = K.load_funcs(os.path.join(HERE, "funcs_arm64.txt"))
    nxt = {s: starts[i + 1] for i, s in enumerate(starts[:-1])}
    s = int(a.addr, 16)
    e = s + a.size if a.size else nxt.get(s)
    if e is None:
        sys.exit("bukan awal fungsi dan --size tidak diberikan")
    print("fungsi 0x%x ukuran %d" % (s, e - s))
    tg, br = K.scan_fn(d, segs, s, e)
    for pc, t in tg:
        u = utf16(d, segs, t)
        kind = "fungsi" if t in nxt else ("string UTF-16 %r" % u if u else "data")
        print("  ref  %x -> 0x%x  %s" % (pc, t, kind))
    ext = Counter(t for _, t, link in br if link or not (s <= t < e))
    for t, n in sorted(ext.items()):
        print("  call 0x%x x%d%s" % (t, n, "  (fungsi, ukuran %d)" % (nxt[t] - t) if t in nxt else ""))
    o = K.va2off(segs, s)
    ws = struct.unpack_from("<%dI" % ((e - s) // 4), d, o)
    print("  kelas kata:", dict(Counter(cls(w) for w in ws)))
    if a.insn:
        for i, w in enumerate(ws):
            if cls(w) != "lain":
                print("    %x %08x %s" % (s + 4 * i, w, cls(w)))


if __name__ == "__main__":
    main()
