# layerExAlpha — LightForAlpha(src)

Biner: `libgame.so` arm64 1.3.9, fungsi `0x5d764c`, 288 B. Status: **terbaca penuh dari recon**; belum diuji di perangkat.

## Tanda tangan (TJS)
`Layer.LightForAlpha(src)` — `src` harus Layer, kalau tidak: pengecualian `Not Layer` (string `0x14c494e`).
Pembungkus ncbind mengirim `x0 = this (layerExBase_GL)`, `x1 = objek src` (tidak ada argumen lain).

## Alur
1. `src.NativeInstanceSupport(GETINSTANCE=2, Layer::ClassID)`; gagal → `Not Layer`.
2. `spitch = GetMainImagePixelBufferPitch(src)`; `sbuf = GetMainImagePixelBufferForWrite(src)`.
3. Untuk `y in [0,H)`, `x in [0,W)` (W,H = `this+0x10`, `this+0x14`; tujuan = `this+0x18`, pitch `this+0x20`):
   `dst[y][x] = { B=0, G=0, R=0, A = src[y][x].G }` (byte 1 sumber → byte 3 tujuan; byte 0..2 tujuan = 0).
4. `redraw()` (`[vtable+8]`).

## Kasus tepi
- Sumber dibaca dengan W,H **tujuan** tanpa cek ukuran sumber → baca di luar buffer bila sumber lebih kecil (UB aslinya). **Penyimpangan fork:** hanya irisan ukuran yang diproses.
- Layer tujuan tanpa gambar: `0x807c00` mengembalikan 0 → aslinya menulis ke alamat nol (crash). **Penyimpangan fork:** tidak melakukan apa-apa.
- W atau H < 1: loop dilewati; `redraw()` tetap dipanggil.

## Tingkat keyakinan
- **Dibaca dari recon** (`recon_alpha2.txt`, 2026-11-22): alur, rumus, konstanta, urutan operasi, offset medan.
- **ASUMSI**: nama parameter TJS (diturunkan dari pemakaian); semantik `redraw()` = `Update(klip)` (vtable[1] objek `layerExBase_GL`, cocok dengan `layerExBase.hpp` fork, tidak dibongkar); `reset()` dianggap dipanggil pembungkus sebelum method (aslinya method ini hanya memakai `_width/_height` dan, kecuali LevelMaskRect(2), `_buffer/_pitch` yang tersimpan).
- Pembungkus Layer (dibaca tangan, lihat `layerExAlpha_S7_profil_xref.md`): `0x807c00` = pointer tulis buffer utama (menandai berubah), `0x807c20` = pitch.
- Perilaku sebelum `redraw`: tidak ada pembatasan ke klip; seluruh gambar `0..W-1 x 0..H-1` diproses.
- Belum diuji di perangkat/NDK; uji inti host: `tests/plugins/layerExAlpha/test_alpha2_s7s10.cpp`.
