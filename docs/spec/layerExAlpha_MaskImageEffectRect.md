# layerExAlpha — MaskImageEffectRect(src, a, l, r, c1, c2, c3)

Biner: `libgame.so` arm64 1.3.9, fungsi `0x5dc9a4`, 1668 B (`recon_alpha5.txt`). Status: **terbaca penuh dari recon**; belum diuji di perangkat.

## Tanda tangan (TJS)
`Layer.MaskImageEffectRect(src, a, l, r, red, green, blue)` — `x1 = src (Layer)`, `w2 = a`, `w3 = l`, `w4 = r`, `w5 = c1`, `w6 = c2`, `w7 = c3` (c1→R, c2→G, c3→B; ASUMSI nama). Sama dengan MaskImageRideRect, tetapi **tabel unsigned**, ada tabel alfa, dan R memakai pembagian integer.

## Alur
1. `src` Layer (`Not Layer` bila bukan); pitch/buffer sumber dari `0x807c20/0x807c00`; `0x806828/0x806920` (lebar/tinggi sumber) dipanggil tetapi hasilnya **tidak dipakai**. Buffer/pitch tujuan = `this+0x18`/`this+0x20` tersimpan.
2. `n = (l+r)+1` (int32; `n == 0` kosong; `n < 0` crash di aslinya). Lima tabel: `T0 float`, `Ta,Tr,Tg,Tb uint32`. Konstanta (float32 kecuali dicatat):
   `s2 = c2/(l+1)`, `s3 = c3/(l+1)`, `s4 = c1/(r+1)`, `s5 = c2/(r+1)`, `s6 = c3/(r+1)`, `s7 = 1/(r+1)`, `s16 = 255/(l+1)`, `s17 = 1/(l+1)`, dan **`c1q = sdiv(c1, l+1)` (bagi integer bertanda; bagi nol → 0)**.
3. Untuk `i = 0..n-1` (`⌊⌋u` = `fcvtzu`: negatif/NaN → 0, jenuh ke 2^32-1):
   - `i < l`: `k=(float)(i+1)`; `Ta=⌊s16*k+0.5⌋u`; `Tr=⌊(float)(c1q*(i+1)) + 0.5⌋u` (perkalian int32 melingkar); `Tg=⌊s2*k+0.5⌋u`; `Tb=⌊s3*k+0.5⌋u`; `T0=s17*(float)(l-i)`
   - `i == l`: `T0=0`, `Ta=255`, `Tr=c1`, `Tg=c2`, `Tb=c3` (dibaca sebagai unsigned)
   - `i > l`: `k=(float)(n-i)`; `Tr=⌊s4*k+0.5⌋u`, `Tg=⌊s5*k+0.5⌋u`, `Tb=⌊s6*k+0.5⌋u`; `Ta=255`; `T0=s7*(float)(i-l)`
   - penjepit: `T0 >= 1 → 1`; `Ta,Tr,Tg,Tb >= 255` (unsigned) `→ 255` (parameter negatif jadi 255); `T0 <= 0 → 0` (NaN tetap).
4. `hi = min(a+r,255)`, `off = a-l`, `lo = max(0,off)`. Untuk tiap piksel tujuan:
   - alfa tujuan `== 0` → lewati;
   - `m` = alfa sumber; **`m < lo` → alfa tujuan := 0** (RGB tetap), lanjut;
   - `m <= a` → `idx = m-off`; RGB dicampur: `B' = Tb + ⌊T0*B+0.5f⌋`, `G' = Tg + …`, `R' = Tr + …` (int32, disimpan low-byte, tanpa clamp) **dan alfa := (uint8)Ta[idx]**;
   - `m > a` dan `hi <= m` → lewati (tidak berubah); `m > a` dan `m < hi` → hanya RGB dicampur (alfa tetap).
5. `redraw()`.

## Ringkas
Mirip MaskImageRideRect, tetapi juga **menulis ulang alfa**: di bawah `a-l` alfa dihapus, di sisi kiri alfa mengikuti tanjakan `Ta`, di tengah 255, di sisi kanan alfa tidak diubah.

## Kasus tepi
- `n < 0` → fork: `TJS_E_INVALIDPARAM`. `n == 0`: tabel kosong; piksel yang butuh indeks dilewati (aslinya deref null), `m < lo` tetap menghapus alfa.
- `l+1 == 0`: `sdiv` → 0 dan pembagian float dengan nol (inf/NaN), hanya pada cabang `i < l` yang tidak dieksekusi bila `l = -1`.
- Sumber lebih kecil dari tujuan: fork memotong ke irisan; layer tanpa gambar: no-op.

## Tingkat keyakinan
- **Dibaca dari recon** (`recon_alpha2.txt`, 2026-11-22): alur, rumus, konstanta, urutan operasi, offset medan.
- **ASUMSI**: nama parameter TJS (diturunkan dari pemakaian); semantik `redraw()` = `Update(klip)` (vtable[1] objek `layerExBase_GL`, cocok dengan `layerExBase.hpp` fork, tidak dibongkar); `reset()` dianggap dipanggil pembungkus sebelum method (aslinya method ini hanya memakai `_width/_height` dan, kecuali LevelMaskRect(2), `_buffer/_pitch` yang tersimpan).
- Pembungkus Layer (dibaca tangan, lihat `layerExAlpha_S7_profil_xref.md`): `0x807c00` = pointer tulis buffer utama (menandai berubah), `0x807c20` = pitch.
- Perilaku sebelum `redraw`: tidak ada pembatasan ke klip; seluruh gambar `0..W-1 x 0..H-1` diproses.
- Belum diuji di perangkat/NDK; uji inti host: `tests/plugins/layerExAlpha/test_alpha2_s7s10.cpp`.
