# layerExAlpha — LuminanceForAlpha(src)

Biner: `libgame.so` arm64 1.3.9, fungsi `0x5d776c`, 356 B. Status: **terbaca penuh dari recon**; belum diuji di perangkat.

## Tanda tangan (TJS)
`Layer.LuminanceForAlpha(src)` — sama dengan LightForAlpha (satu argumen Layer, `Not Layer` bila bukan).

## Alur
Sama dengan LightForAlpha, hanya nilai alfa berbeda. Konstanta float32 dari `.rodata`:
`c0 = 0.298911988735199` (`0x14c9b70`, bits `3e990afe`), `c1 = 0.5866109728813171` (`0x14c9b74`, `3f162c23`), `c2 = 0.11447799950838089` (`0x14c9b78`, `3dea7371`).

Per piksel: `t = (float)s[0]*c0; u = (float)s[1]*c1; t += u; u = (float)s[2]*c2; t += u; A = (uint8)fcvtzs(t)`;
tujuan `= {0,0,0,A}`. Semua operasi float32 terpisah (tanpa FMA); `fcvtzs` memotong ke nol dan jenuh; hasil disimpan **low-byte** (tanpa clamp ke 255).

## Catatan penting
- **Bobot R (0.2989) dikalikan ke byte 0 (B)** dan bobot B (0.1145) ke byte 2 (R) bila memori B,G,R,A — kemungkinan galat bawaan aslinya; **ditiru apa adanya**. (ASUMSI bahwa urutan memori piksel Kirikiri di sini B,G,R,A — itu yang dipakai seluruh fork.)
- Jumlah koefisien 1,000001 → putih (255,255,255) memberi 255,00026 → 255.
- Kasus tepi sumber/tujuan: sama dengan LightForAlpha (penyimpangan fork: irisan ukuran; layer tanpa gambar = no-op).

## Tingkat keyakinan
- **Dibaca dari recon** (`recon_alpha2.txt`, 2026-11-22): alur, rumus, konstanta, urutan operasi, offset medan.
- **ASUMSI**: nama parameter TJS (diturunkan dari pemakaian); semantik `redraw()` = `Update(klip)` (vtable[1] objek `layerExBase_GL`, cocok dengan `layerExBase.hpp` fork, tidak dibongkar); `reset()` dianggap dipanggil pembungkus sebelum method (aslinya method ini hanya memakai `_width/_height` dan, kecuali LevelMaskRect(2), `_buffer/_pitch` yang tersimpan).
- Pembungkus Layer (dibaca tangan, lihat `layerExAlpha_S7_profil_xref.md`): `0x807c00` = pointer tulis buffer utama (menandai berubah), `0x807c20` = pitch.
- Perilaku sebelum `redraw`: tidak ada pembatasan ke klip; seluruh gambar `0..W-1 x 0..H-1` diproses.
- Belum diuji di perangkat/NDK; uji inti host: `tests/plugins/layerExAlpha/test_alpha2_s7s10.cpp`.
