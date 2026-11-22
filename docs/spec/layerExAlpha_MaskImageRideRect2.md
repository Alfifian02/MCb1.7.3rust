# layerExAlpha — MaskImageRideRect2(src, a, w, c1, c2, c3)

Biner: `libgame.so` arm64 1.3.9, fungsi `0x5d867c`, 1172 B. Status: **terbaca penuh dari recon**; belum diuji di perangkat.

## Tanda tangan (TJS)
`Layer.MaskImageRideRect2(src, a, w, red, green, blue)` — `x1 = src`, `w2 = a`, `w3 = w`, `w4 = c1`, `w5 = c2`, `w6 = c3` (c1→R, c2→G, c3→B seperti MaskImageRideRect).

## Alur
1. Ambil `src` (`Not Layer`), buffer/pitch sumber; buffer/pitch tujuan tersimpan.
2. `n = w+1` (`n == 0` → tabel kosong; `n < 0` → crash di aslinya). Semua **float32**: `s5=(float)n`, `s2=(float)c1/s5`, `s3=(float)c2/s5`, `s4=(float)c3/s5`, `s5r=1.0f/s5`.
   Untuk `i = 0..n-1`:
   - `i < w`: `T0 = s5r*(float)(w-i)`; `s7=(float)(i+1)`; `T1=⌊s2*s7+0.5f⌋`; `T2=⌊s3*s7+0.5f⌋`; `T3=⌊s4*s7+0.5f⌋`
   - `i == w`: `T0=0`, `T1=c1`, `T2=c2`, `T3=c3`
   - penjepit sama dengan MaskImageRideRect.
3. `off = a-w`, `lo = max(0,off)`. Untuk tiap piksel tujuan dengan alfa ≠ 0, `m` = alfa sumber:
   - `lo > m` → lewati;
   - `m > a` → tulis **warna penuh** `B=(uint8)c3, G=(uint8)c2, R=(uint8)c1` (nilai mentah, low-byte, tidak dijepit);
   - selain itu (`lo <= m <= a`): `idx = m-off`, campuran seperti MaskImageRideRect (`T3/T2/T1 + ⌊T0*kanal+0.5f⌋`, low-byte).
4. `redraw()`.

## Ringkas
Versi satu sisi: tanjakan lebar `w` di bawah `a`, warna penuh di atas `a` (tanpa batas atas `hi`).

## Kasus tepi
- `w = -1`: tanpa tabel; hanya `m > a` yang menulis warna (karena `lo = a+1`). `w < -1` → fork: `TJS_E_INVALIDPARAM`.
- `idx` selalu `<= w` bila `m <= a`. Sumber/tujuan: sama dengan MaskImageRideRect.

## Tingkat keyakinan
- **Dibaca dari recon** (`recon_alpha2.txt`, 2026-11-22): alur, rumus, konstanta, urutan operasi, offset medan.
- **ASUMSI**: nama parameter TJS (diturunkan dari pemakaian); semantik `redraw()` = `Update(klip)` (vtable[1] objek `layerExBase_GL`, cocok dengan `layerExBase.hpp` fork, tidak dibongkar); `reset()` dianggap dipanggil pembungkus sebelum method (aslinya method ini hanya memakai `_width/_height` dan, kecuali LevelMaskRect(2), `_buffer/_pitch` yang tersimpan).
- Pembungkus Layer (dibaca tangan, lihat `layerExAlpha_S7_profil_xref.md`): `0x807c00` = pointer tulis buffer utama (menandai berubah), `0x807c20` = pitch.
- Perilaku sebelum `redraw`: tidak ada pembatasan ke klip; seluruh gambar `0..W-1 x 0..H-1` diproses.
- Belum diuji di perangkat/NDK; uji inti host: `tests/plugins/layerExAlpha/test_alpha2_s7s10.cpp`.
