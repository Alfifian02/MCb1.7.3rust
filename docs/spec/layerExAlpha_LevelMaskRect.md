# layerExAlpha — LevelMaskRect(a, b)

Biner: `libgame.so` arm64 1.3.9, fungsi `0x5d78d0`, 728 B. Status: **terbaca penuh dari recon**; belum diuji di perangkat.

## Tanda tangan (TJS)
`Layer.LevelMaskRect(a, b)` — dua integer (`w1 = a`, `w2 = b`); **tidak ada argumen Layer/rect** meski namanya berakhiran `Rect`. Bekerja pada layer ini saja.

## Alur
1. Segarkan `this+0x18 = GetMainImagePixelBufferForWrite(this+0x8)`, `this+0x20 = GetMainImagePixelBufferPitch(this+0x8)`.
2. Tabel `int T[n]`, `n = 2b+1`. `s0 = 255.0f / (float)(b+1)` (255.0 dari `0x14c30cc`). Untuk `i = 0..n-1`:
   - `i <= b`: `T[i] = fcvtzs(s0*(float)(i+1) + 0.5f)`
   - `i == b+1`: `T[i] = 255` (**plateau dua entri**: `T[b]` ≈ 255 dan `T[b+1]` = 255)
   - `i > b+1`: `T[i] = fcvtzs(s0*(float)(n-i) + 0.5f)`
3. `hi = min(a+b, 255)`; `off = a-b`; `lo = max(0, off)` (int32 melingkar).
4. Untuk tiap piksel (seluruh gambar), `v = alfa`: bila `lo <= v <= hi` → `alfa = (uint8)T[v-off]`, selain itu `alfa = 0`. Byte B,G,R tidak disentuh.
5. `redraw()`.

## Kasus tepi
- `b < 0`: `n` negatif → `std::vector` melempar `length_error` → crash. **Penyimpangan fork:** `TJS_E_INVALIDPARAM`.
- `b = 0`: `T = {255}`; hanya `alfa == a` lolos (→ 255).
- Indeks `v-off` selalu dalam `[0, n-1]` karena `lo >= off` dan `hi <= a+b`.
- Tabel besar (b sangat besar) dibatasi fork pada 2^24 entri (aslinya tanpa batas).
- Dua jalur (vektor NEON 4-lebar + sisa skalar) di biner menghitung rumus yang sama; fork memakai satu jalur skalar.

## Tingkat keyakinan
- **Dibaca dari recon** (`recon_alpha2.txt`, 2026-11-22): alur, rumus, konstanta, urutan operasi, offset medan.
- **ASUMSI**: nama parameter TJS (diturunkan dari pemakaian); semantik `redraw()` = `Update(klip)` (vtable[1] objek `layerExBase_GL`, cocok dengan `layerExBase.hpp` fork, tidak dibongkar); `reset()` dianggap dipanggil pembungkus sebelum method (aslinya method ini hanya memakai `_width/_height` dan, kecuali LevelMaskRect(2), `_buffer/_pitch` yang tersimpan).
- Pembungkus Layer (dibaca tangan, lihat `layerExAlpha_S7_profil_xref.md`): `0x807c00` = pointer tulis buffer utama (menandai berubah), `0x807c20` = pitch.
- Perilaku sebelum `redraw`: tidak ada pembatasan ke klip; seluruh gambar `0..W-1 x 0..H-1` diproses.
- Belum diuji di perangkat/NDK; uji inti host: `tests/plugins/layerExAlpha/test_alpha2_s7s10.cpp`.
