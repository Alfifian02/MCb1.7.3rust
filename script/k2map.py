#!/usr/bin/env python3
"""k2map.py - peta nama method layerEx -> alamat implementasi -> ukuran (langkah 4.9a).

Tanpa disassembler: hanya mendekode 4 jenis instruksi AArch64 langsung dari byte
(ADRP, ADD imm64, B, BL) pada fungsi registrasi ncbind di libgame.so arm64 1.3.9.

Pakai:
  python3 script/k2map.py [--lib PATH] [--out script/layerex_impl_map.tsv] [--verbose]

Cara kerja
  1. Fungsi = daftar awal fungsi dari script/funcs_arm64.txt (hasil .eh_frame);
     ukuran = awal fungsi berikutnya - awal fungsi ini.
  2. Untuk tiap fungsi registrasi di script/layerex_binary_map.txt, lacak pasangan
     adrp+add. Target yang jatuh tepat di awal fungsi lain  = 'penunjuk kode'.
     Target di .rodata yang terbaca sebagai string UTF-16 = 'nama'.
  3. Thunk kecil (<= 32 B, berakhir 'b <alamat>') diikuti ke tujuan b.
  4. Fungsi registrasi dikelompokkan per nama; tiap nama dipetakan ke penunjuk kode
     yang BUKAN fungsi registrasi/unregistrasi itu sendiri.

Batasan (jujur): ini heuristik. Selalu cocokkan dengan uji-diri (--selftest, default
aktif) sebelum percaya pada baris lain. Alamat tetap perlu diverifikasi dengan k2recon.sh
atau Ghidra sebelum dipakai sebagai spesifikasi.
"""
import argparse, ast, os, re, struct, sys

HERE = os.path.dirname(os.path.abspath(__file__))

# Nilai uji-diri: sudah dibaca manual dari disassembly (rencana 4.3 / 4.6 / 4.6b).
KNOWN = {"copyRaster": 0x5E6BB0, "shrinkCopy": 0x5FFCF8, "shrinkCopyFast": 0x60185C}


def load_elf(path):
    d = open(path, "rb").read()
    if d[:4] != b"\x7fELF" or d[4] != 2 or struct.unpack_from("<H", d, 0x12)[0] != 183:
        sys.exit("bukan ELF64 AArch64: " + path)
    phoff = struct.unpack_from("<Q", d, 0x20)[0]
    phentsize, phnum = struct.unpack_from("<HH", d, 0x36)
    segs = []
    for i in range(phnum):
        t, fl, off, va, pa, fsz, msz, al = struct.unpack_from("<IIQQQQQQ", d, phoff + i * phentsize)
        if t == 1:
            segs.append((va, off, fsz, fl))
    return d, segs


def va2off(segs, va):
    for v, o, s, _ in segs:
        if v <= va < v + s:
            return o + va - v
    return None


def load_funcs(path):
    starts = []
    for ln in open(path):
        p = ln.split()
        if p and p[0].isdigit():
            starts.append(int(p[0]))
    starts = sorted(set(starts))
    return starts


def sext(v, bits):
    return v - (1 << bits) if v & (1 << (bits - 1)) else v


def scan_fn(d, segs, s, e):
    """Kembalikan (daftar target adrp+add, daftar tujuan b/bl) untuk fungsi [s,e)."""
    o = va2off(segs, s)
    pg, targets, branches = {}, [], []
    words = struct.unpack_from("<%dI" % ((e - s) // 4), d, o)
    for i, w in enumerate(words):
        pc = s + 4 * i
        if (w & 0x9F000000) == 0x90000000:                     # ADRP
            immlo, immhi, rd = (w >> 29) & 3, (w >> 5) & 0x7FFFF, w & 31
            pg[rd] = (pc & ~0xFFF) + (sext((immhi << 2) | immlo, 21) << 12)
        elif (w & 0xFF000000) == 0x91000000:                   # ADD Xd, Xn, #imm{,lsl 12}
            sh, imm, rn, rd = (w >> 22) & 1, (w >> 10) & 0xFFF, (w >> 5) & 31, w & 31
            if rn in pg:
                targets.append((pc, pg[rn] + (imm << (12 if sh else 0))))
                if rd == rn:                 # reg sekarang berisi alamat penuh, bukan halaman lagi
                    pg.pop(rn, None)
        elif (w & 0x7C000000) == 0x14000000:                   # B / BL
            branches.append((pc, pc + (sext(w & 0x3FFFFFF, 26) << 2), bool(w & 0x80000000)))
    return targets, branches


def utf16_name(d, segs, va, maxlen=48):
    o = va2off(segs, va)
    if o is None:
        return None
    out = []
    for i in range(maxlen):
        c = struct.unpack_from("<H", d, o + 2 * i)[0]
        if c == 0:
            break
        if not (0x20 <= c < 0x7F):
            return None
        out.append(chr(c))
    s = "".join(out)
    return s if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_.= ;]*", s) else None


def parse_map(path):
    rows = []
    for ln in open(path):
        m = re.match(r"0x([0-9a-f]+) size=(\d+) (\[.*\])", ln.strip())
        if m:
            rows.append((int(m.group(1), 16), int(m.group(2)), ast.literal_eval(m.group(3))))
    return rows


def parse_groups(path):
    """layerex_expected_methods.txt -> {nama_method: modul}."""
    txt = " ".join(l for l in open(path) if not l.startswith("#"))
    g = {}
    for m in re.finditer(r"(layerEx\w+|shrinkCopy):\s*([^:]*?)(?=\s+(?:layerEx\w+|shrinkCopy):|$)", txt):
        for n in m.group(2).split():
            g[n] = m.group(1)
    return g


def pair_events(events):
    """events: [(pc,'CODE',addr)|(pc,'NAME',str)] berurut. Kembalikan [(nama, kode)].
    Gaya A/C: CODE mendahului NAME. Gaya B: NAME mendahului CODE (fungsi tipis).
    Nama yang sudah terpasang di fungsi yang sama diabaikan (itu salinan unregistrasi)."""
    out, pend_code, wait_name, done = [], None, None, set()
    for _, kind, v in events:
        if kind == "CODE":
            if wait_name is not None:
                out.append((wait_name, v)); done.add(wait_name); wait_name = None
            else:
                pend_code = v
        else:
            if v in done:
                continue
            if pend_code is not None:
                out.append((v, pend_code)); done.add(v); pend_code = None
            else:
                wait_name = v
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--lib", default=os.path.join(HERE, "..", "Kirikiroid2_1.3.9", "lib", "arm64-v8a", "libgame.so"))
    ap.add_argument("--out", default=os.path.join(HERE, "layerex_impl_map.tsv"))
    ap.add_argument("--verbose", action="store_true")
    a = ap.parse_args()

    d, segs = load_elf(a.lib)
    starts = load_funcs(os.path.join(HERE, "funcs_arm64.txt"))
    sset = set(starts)
    nxt = {s: starts[i + 1] for i, s in enumerate(starts[:-1])}
    size = lambda s: nxt[s] - s
    groups = parse_groups(os.path.join(HERE, "layerex_expected_methods.txt"))
    wanted = set(groups)

    def follow(t, depth=3):                      # ikuti thunk 'b' kecil (<= 32 B)
        while depth and t in nxt and size(t) <= 32:
            tg, br = scan_fn(d, segs, t, nxt[t])
            tails = [b for (pc, b, link) in br if not link and pc == nxt[t] - 4]
            if not tails:
                break
            t, depth = tails[0], depth - 1
        return t

    name_cache = {}
    def name_at(t):
        if t not in name_cache:
            n = utf16_name(d, segs, t)
            name_cache[t] = n if n in wanted else None
        return name_cache[t]

    found = {}                                   # method -> [(fungsi_registrasi, ukuran_reg, impl)]
    for s in starts[:-1]:
        e = nxt[s]
        if e - s > 4096 or va2off(segs, s) is None:
            continue
        tg, _ = scan_fn(d, segs, s, e)
        ev = []
        for pc, t in tg:
            n = name_at(t)
            if n:
                ev.append((pc, "NAME", n))
            elif t in nxt and t != s:
                ev.append((pc, "CODE", t))
        if not any(k == "NAME" for _, k, _ in ev):
            continue
        for n, c in pair_events(sorted(ev)):
            found.setdefault(n, []).append((s, e - s, follow(c)))

    # ----- uji-diri -----
    ok = True
    print("# UJI-DIRI terhadap hasil baca manual:")
    for n, want in KNOWN.items():
        got = [x[2] for x in found.get(n, [])]
        hit = want in got
        ok &= hit
        print("#   %-16s harap 0x%x -> %s  %s" % (n, want, ["0x%x" % x for x in got], "OK" if hit else "GAGAL"))
    print("# status uji-diri:", "LOLOS" if ok else "ADA YANG GAGAL")

    order = [n for n in groups] + [n for n in found if n not in groups]
    missing = [n for n in groups if n not in found]
    with open(a.out, "w") as f:
        f.write("# Peta method layerEx -> implementasi, libgame.so arm64 1.3.9 (k2map.py; heuristik ADRP+ADD)\n")
        f.write("# kolom: modul\tmethod\tfungsi_registrasi\tukuran_reg\timplementasi\tukuran_impl\tcatatan\n")
        f.write("# modul = tebakan dari urutan string (belum pasti). ukuran = awal fungsi berikutnya - awal (.eh_frame).\n")
        for n in order:
            for rs, rz, c in found.get(n, []):
                f.write("%s\t%s\t0x%x\t%d\t0x%x\t%d\t%s\n" % (groups.get(n, "?"), n, rs, rz, c, size(c) if c in nxt else 0,
                        "kecil(thunk?)" if c in nxt and size(c) <= 32 else ""))
            if n not in found:
                f.write("%s\t%s\t-\t-\t-\t-\ttidak ditemukan pasangan adrp+add\n" % (groups.get(n, "?"), n))
    if a.verbose:
        print(open(a.out).read())
    print("# tulis:", a.out, "| ditemukan %d dari %d nama; tidak ditemukan: %s" % (len(found), len(groups), ", ".join(missing) or "-"))
    sys.exit(0 if ok else 2)


if __name__ == "__main__":
    main()
