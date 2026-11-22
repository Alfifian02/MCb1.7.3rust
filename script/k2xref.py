#!/usr/bin/env python3
"""k2xref.py - profil referensi (xref) per method layerEx, TANPA disassembler.

Memakai ulang dekoder k2map.py (ADRP, ADD imm, B, BL). Untuk tiap method di
script/layerex_impl_map.tsv ia melaporkan: ukuran, string yang dirujuk (UTF-16 atau ASCII),
fungsi yang dipanggil (bl) beserta ukurannya, dan jumlah instruksi NEON/SIMD (heuristik kasar).

BUKAN dekompilasi: hanya menjawab "fungsi ini menyentuh string/helper apa". Alur piksel tetap harus
dibaca dari recon (k2recon.sh) atau Ghidra.

Pakai:  python3 script/k2xref.py --lib PATH/libgame.so [--group layerExAlpha] [method ...]
"""
import argparse, os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from k2map import load_elf, va2off, load_funcs, scan_fn  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
HELPERS = {  # dari rencana 4.7 (belum dicocokkan dengan layerExBase.hpp)
    0x806828: "lebar gambar utama", 0x806920: "tinggi gambar utama", 0x80C848: "sinkron gambar",
    0xA75340: "pointer tulis buffer", 0xA7546C: "pitch buffer", 0x848BDC: "IsSoftware()",
    0x848834: "TVPGetRenderManager()",
}


def string_at(d, segs, va, maxn=70):
    o = va2off(segs, va)
    if o is None:
        return None
    # UTF-16 dulu
    out = []
    for i in range(maxn):
        if o + 2 * i + 2 > len(d):
            break
        c = struct.unpack_from("<H", d, o + 2 * i)[0]
        if c == 0:
            break
        if not (0x20 <= c < 0x7F or c in (9, 10, 13)):
            out = None
            break
        out.append(chr(c))
    if out and len(out) >= 2:
        return "u16", "".join(out)
    out = []
    for i in range(maxn):
        c = d[o + i]
        if c == 0:
            break
        if not (0x20 <= c < 0x7F or c in (9, 10, 13)):
            return None
        out.append(chr(c))
    return ("a8", "".join(out)) if len(out) >= 4 else None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--lib", default=os.path.expanduser("~/k2_out/libgame.so"))
    ap.add_argument("--group", default="layerExAlpha")
    ap.add_argument("names", nargs="*")
    a = ap.parse_args()
    d, segs = load_elf(a.lib)
    starts = load_funcs(os.path.join(HERE, "funcs_arm64.txt"))
    size = {s: (starts[i + 1] - s) for i, s in enumerate(starts[:-1])}
    rows = []
    for ln in open(os.path.join(HERE, "layerex_impl_map.tsv")):
        if ln.startswith("#"):
            continue
        p = ln.rstrip("\n").split("\t")
        if len(p) >= 6 and p[0] == a.group and p[4].startswith("0x") and (not a.names or p[1] in a.names):
            rows.append((p[1], int(p[4], 16), int(p[5])))
    seen = set()
    for name, s, sz in rows:
        if (name, s) in seen:
            continue
        seen.add((name, s))
        tg, br = scan_fn(d, segs, s, s + sz)
        words = struct.unpack_from("<%dI" % (sz // 4), d, va2off(segs, s))
        # NEON/SIMD kasar: kelas enkoding 'Advanced SIMD' (bit28..25 = x111 dengan bit 28=0 atau 0111/1111)
        simd = sum(1 for w in words if ((w >> 25) & 0b0111) == 0b111 and ((w >> 28) & 1) == 0)
        print("== %s  impl=0x%x  ukuran=%d  (~%d instruksi, ~%d SIMD kasar)" % (name, s, sz, sz // 4, simd))
        strs = []
        for pc, t in tg:
            r = string_at(d, segs, t)
            if r and (pc, t) not in strs:
                strs.append((pc, t, r))
        for pc, t, (kind, txt) in strs:
            print("   str  @0x%x -> 0x%x [%s] %r" % (pc, t, kind, txt))
        ps = sorted({t for pc, t, link in br if link and not (s <= t < s + sz)})
        for t in ps:
            print("   bl   0x%x  ukuran=%s  %s" % (t, size.get(t, "?"), HELPERS.get(t, "")))
        # cabang bersyarat (b.cond, cbz/cbnz, tbz/tbnz) TIDAK didekode k2map.scan_fn -> hitung di sini;
        # cabang MUNDUR (target < pc) di dalam fungsi = penutup loop.
        back = fwd = 0
        for i, w in enumerate(words):
            pc = s + 4 * i
            if (w & 0xFF000010) == 0x54000000:
                t = pc + (((w >> 5) & 0x7FFFF) - (1 << 19 if (w >> 5) & 0x40000 else 0)) * 4
            elif (w & 0x7E000000) == 0x34000000:
                t = pc + (((w >> 5) & 0x7FFFF) - (1 << 19 if (w >> 5) & 0x40000 else 0)) * 4
            elif (w & 0x7E000000) == 0x36000000:
                t = pc + (((w >> 5) & 0x3FFF) - (1 << 14 if (w >> 5) & 0x2000 else 0)) * 4
            else:
                continue
            if s <= t < s + sz:
                if t <= pc:
                    back += 1
                else:
                    fwd += 1
        n_in = sum(1 for pc, t, link in br if not link and s <= t < s + sz)
        n_back_b = sum(1 for pc, t, link in br if not link and s <= t <= pc and s <= pc < s + sz)
        for t in sorted({t for pc, t, link in br if not link and not (s <= t < s + sz)}):
            print("   b-keluar (tail call/lompat keluar) 0x%x  ukuran=%s  %s" % (t, size.get(t, "?"), HELPERS.get(t, "")))
        print("   b tanpa-syarat dalam fungsi: %d (mundur: %d) | bersyarat dalam fungsi: maju %d, mundur %d  => perkiraan loop = %d"
              % (n_in, n_back_b, fwd, back, back + n_back_b))


if __name__ == "__main__":
    main()
