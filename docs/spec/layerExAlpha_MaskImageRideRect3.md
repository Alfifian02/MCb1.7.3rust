# layerExAlpha — MaskImageRideRect3(src, a, w, c1, c2, c3)

Biner: `libgame.so` arm64 1.3.9, fungsi `0x5d8b10`, 1156 B (`recon_alpha2c.txt`). Status: **terbaca penuh dari recon**; belum diuji di perangkat.

## Tanda tangan (TJS)
`Layer.MaskImageRideRect3(src, a, w, red, green, blue)` — `x1 = src`, `w2 = a`, `w3 = w`, `w4 = c1`, `w5 = c2`, `w6 = c3` (c1→R, c2→G, c3→B). Sama dengan MaskImageRideRect2, tetapi arah rampa terbalik.

## Alur
1. `src` Layer (`Not Layer` bila bukan); pitch/buffer sumber lewat `0x807c20/0x807c00`; buffer/pitch tujuan = `this+0x18`/`this+0x20` tersimpan.
2. `n = w+1` (`n == 0` → tabel kosong; `n < 0` → crash di aslinya). Float32: `s1 = 1/(float)n`, `s2 = (float)c1/n`, `s3 = (float)c2/n`, `s4 = (float)c3/n`.
   - `i == 0`: `T0 = 0`, `T1 = c1`, `T2 = c2`, `T3 = c3`
   - `i > 0`: `T0 = s1*(float)i`; `k = (float)(n-i)`; `T1 = ⌊s2*k+0.5f⌋`, `T2 = ⌊s3*k+0.5f⌋`, `T3 = ⌊s4*k+0.5f⌋`
   - penjepit sama dengan MaskImageRideRect (`T0 >= 1 → 1`; warna `>= 255 → 255`; `!(T0 > 0) → 0`; warna `<= 0 → 0`).
3. `hi = min(w+a, 255)`. Untuk tiap piksel tujuan dengan alfa ≠ 0, `m` = alfa sumber (tanpa batas bawah `lo`):
   - `m < a` (signed) → tulis **warna penuh** `B=(uint8)c3, G=(uint8)c2, R=(uint8)c1` (nilai mentah, low-byte);
   - `m >= a` dan `m > hi` → lewati (**`m == hi` diproses**, berbeda dengan MaskImageRideRect);
   - `m >= a` dan `m <= hi` → `idx = m-a`; `B' = T3 + ⌊T0*B+0.5f⌋`, `G' = T2 + ⌊T0*G+0.5f⌋`, `R' = T1 + ⌊T0*R+0.5f⌋` (low-byte, tanpa clamp). Alfa tidak diubah.
4. `redraw()`.

## Ringkas
Warna penuh untuk mask di bawah `a`; di atas `a` memudar linear selama `w` langkah (warna×`(n-i)/n` ditambah tujuan×`i/n`); lebih dari `a+w` tidak berubah. Kebalikan MaskImageRideRect2.

## Kasus tepi
- `w = -1`: tanpa tabel; hanya `m < a` yang menulis warna (`hi = a-1 < m` untuk `m >= a`). `w < -1` → fork: `TJS_E_INVALIDPARAM`.
- `idx <= w` selalu (karena `m <= hi <= a+w`).
- Sumber lebih kecil dari tujuan: fork memotong ke irisan (aslinya baca di luar buffer). Layer tanpa gambar: no-op.

## Tingkat keyakinan
- **Dibaca dari recon** (`recon_alpha2.txt`, 2026-11-22): alur, rumus, konstanta, urutan operasi, offset medan.
- **ASUMSI**: nama parameter TJS (diturunkan dari pemakaian); semantik `redraw()` = `Update(klip)` (vtable[1] objek `layerExBase_GL`, cocok dengan `layerExBase.hpp` fork, tidak dibongkar); `reset()` dianggap dipanggil pembungkus sebelum method (aslinya method ini hanya memakai `_width/_height` dan, kecuali LevelMaskRect(2), `_buffer/_pitch` yang tersimpan).
- Pembungkus Layer (dibaca tangan, lihat `layerExAlpha_S7_profil_xref.md`): `0x807c00` = pointer tulis buffer utama (menandai berubah), `0x807c20` = pitch.
- Perilaku sebelum `redraw`: tidak ada pembatasan ke klip; seluruh gambar `0..W-1 x 0..H-1` diproses.
- Belum diuji di perangkat/NDK; uji inti host: `tests/plugins/layerExAlpha/test_alpha2_s7s10.cpp`.
