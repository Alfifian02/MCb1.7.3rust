# layerExAlpha — LevelMaskRect2(a, l, r)

Biner: `libgame.so` arm64 1.3.9, fungsi `0x5d7ba8`, 788 B. Status: **terbaca penuh dari recon**; belum diuji di perangkat.

## Tanda tangan (TJS)
`Layer.LevelMaskRect2(a, l, r)` — tiga integer (`w1 = a`, `w2 = l`, `w3 = r`). Layer ini saja.

## Alur
1. Segarkan buffer/pitch seperti LevelMaskRect.
2. `n = (l+r)+1` (int32). `n == 0` → tabel kosong. `n < 0` → crash di aslinya.
3. `s0 = 255.0f/(float)(l+1)`, `s1 = 255.0f/(float)(r+1)`. Untuk `i = 0..n-1`:
   - `i < l`: `v = fcvtzs(s0*(float)(i+1) + 0.5f)`
   - `i == l`: `v = 255`
   - `i > l`: `v = fcvtzs(s1*(float)(n-i) + 0.5f)`
   - `T[i] = (v > 0) ? min(v,255) : 0`
4. `hi = min(a+r, 255)`, `off = a-l`, `lo = max(0, off)`; per piksel: `lo <= alfa <= hi` → `alfa = (uint8)T[alfa-off]`, selain itu 0. Hanya alfa yang ditulis.
5. `redraw()`.

## Kasus tepi
- Beda dengan LevelMaskRect: puncak satu entri (`T[l]=255`), sisi kiri naik dari `255/(l+1)` dan sisi kanan turun ke `255/(r+1)`, hasil dijepit 0..255.
- `l+r+1 < 0` → fork: `TJS_E_INVALIDPARAM`. `n == 0` → tidak ada indeks tabel yang valid; fork menghasilkan alfa 0 untuk semua piksel (sama dengan aslinya kecuali kasus `alfa == a` yang aslinya membaca pointer null).

## Tingkat keyakinan
- **Dibaca dari recon** (`recon_alpha2.txt`, 2026-11-22): alur, rumus, konstanta, urutan operasi, offset medan.
- **ASUMSI**: nama parameter TJS (diturunkan dari pemakaian); semantik `redraw()` = `Update(klip)` (vtable[1] objek `layerExBase_GL`, cocok dengan `layerExBase.hpp` fork, tidak dibongkar); `reset()` dianggap dipanggil pembungkus sebelum method (aslinya method ini hanya memakai `_width/_height` dan, kecuali LevelMaskRect(2), `_buffer/_pitch` yang tersimpan).
- Pembungkus Layer (dibaca tangan, lihat `layerExAlpha_S7_profil_xref.md`): `0x807c00` = pointer tulis buffer utama (menandai berubah), `0x807c20` = pitch.
- Perilaku sebelum `redraw`: tidak ada pembatasan ke klip; seluruh gambar `0..W-1 x 0..H-1` diproses.
- Belum diuji di perangkat/NDK; uji inti host: `tests/plugins/layerExAlpha/test_alpha2_s7s10.cpp`.
