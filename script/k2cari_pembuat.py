#!/usr/bin/env python3
"""k2cari_pembuat.py - cari skrip yang MENETAPKAN (membuat) sebuah nama, mis. SystemConfig.

Pakai (taruh di folder yang sama dengan tjs2dis.py):
    python3 k2cari_pembuat.py <folder_out> [NAMA] [--konteks N]
contoh:
    python3 k2cari_pembuat.py ~/storage/shared/out_extracted/out SystemConfig

Yang dicari:
  * bytecode TJS2: instruksi penetapan properti (spd, spde, spds, spdeh, spis, spie) yang kunci/namanya NAMA.
    Dicetak berikut nama fungsi (obj) dan beberapa baris di sekitarnya. Penetapan pada `%-1` / `global`
    di obj 0 artinya penetapan di tingkat atas skrip (kandidat kuat "pembuat").
  * teks (.tjs UTF-16/UTF-8): `NAMA = ...`, `global.NAMA = ...`, `var NAMA`, `.NAMA =` di dalam with.
Penetapan seperti `SystemConfig.x = 1` BUKAN pembuat, jadi tidak dihitung (hanya `NAMA` sebagai sasaran).
Keluaran dibatasi supaya muat di layar; gunakan --maks untuk menaikkan.
"""
import os, re, sys, types

SET_OPS = ("spd", "spde", "spds", "spdeh", "spis", "spie")


def muat_dis(folder_skrip):
    """Muat fungsi load/dis dari tjs2dis.py tanpa menjalankan bagian main-nya."""
    p = os.path.join(folder_skrip, "tjs2dis.py")
    if not os.path.isfile(p):
        sys.exit("tjs2dis.py tidak ditemukan di " + folder_skrip + " (taruh skrip ini di sebelahnya)")
    code = open(p, encoding="utf-8").read()
    kepala = code.split("if __name__")[0]
    mod = types.ModuleType("tjs2dis_mod")
    mod.__dict__["__name__"] = "tjs2dis_mod"
    exec(kepala, mod.__dict__)
    return mod


def baca_teks(b):
    if b[:2] == b"\xff\xfe":
        return b[2:].decode("utf-16le", "replace")
    if b[:3] == b"\xef\xbb\xbf":
        return b[3:].decode("utf-8", "replace")
    return b.decode("utf-8", "replace")


def cari_bytecode(mod, path, nama, konteks):
    hasil = []
    try:
        S, objs = mod.load(path)
    except Exception as e:  # berkas rusak / bukan TJS2100
        return None, "gagal dibongkar: %s" % e
    for k, d in enumerate(objs):
        baris = mod.dis(d, objs)
        for i, l in enumerate(baris):
            if "'%s'" % nama not in l:
                continue
            op = l.split()[1] if len(l.split()) > 1 else ""
            if op in SET_OPS:
                a, b = max(0, i - konteks), min(len(baris), i + konteks + 1)
                hasil.append((k, d["name"], d["parent"], baris[a:b], i - a))
    return hasil, None


def cari_teks(path, nama):
    t = baca_teks(open(path, "rb").read()).replace("\r", "")
    pola = re.compile(
        r"(global\s*\.\s*%(n)s\s*=[^=]|(?<![\w.])%(n)s\s*=[^=]|var\s+%(n)s\b|^\s*\.\s*%(n)s\s*=[^=])" % {"n": re.escape(nama)})
    out = []
    for no, l in enumerate(t.split("\n"), 1):
        if pola.search(l):
            out.append((no, l.strip()[:140]))
    return out


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    opsi = dict(a[2:].split("=", 1) for a in sys.argv[1:] if a.startswith("--") and "=" in a)
    if not args:
        sys.exit(__doc__)
    folder = os.path.expanduser(args[0])
    nama = args[1] if len(args) > 1 else "SystemConfig"
    konteks = int(opsi.get("konteks", 3))
    maks = int(opsi.get("maks", 40))
    mod = muat_dis(os.path.dirname(os.path.abspath(__file__)))
    nb = nt = ngagal = ncocok = 0
    for d, _, fs in os.walk(folder):
        for f in sorted(fs):
            if not f.lower().endswith(".tjs"):
                continue
            p = os.path.join(d, f)
            b = open(p, "rb").read(8)
            rel = os.path.relpath(p, folder)
            if b[:4] == b"TJS2":
                nb += 1
                h, err = cari_bytecode(mod, p, nama, konteks)
                if err:
                    ngagal += 1
                    print("! %s : %s" % (rel, err))
                    continue
                for (k, on, par, ctx, idx) in h:
                    ncocok += 1
                    if ncocok > maks:
                        break
                    print("\n[BYTECODE] %s  obj %d '%s' (induk obj %d)%s" % (
                        rel, k, on, par, "  <== TINGKAT ATAS" if k == 0 else ""))
                    for j, l in enumerate(ctx):
                        print(("  >> " if j == idx else "     ") + l[:150])
            else:
                nt += 1
                for (no, l) in cari_teks(p, nama):
                    ncocok += 1
                    if ncocok > maks:
                        break
                    print("\n[TEKS] %s:%d  %s" % (rel, no, l))
    print("\n== selesai: %d bytecode, %d teks, %d gagal dibongkar, %d kecocokan%s" % (
        nb, nt, ngagal, min(ncocok, maks), " (dibatasi --maks=%d)" % maks if ncocok > maks else ""))


if __name__ == "__main__":
    main()
