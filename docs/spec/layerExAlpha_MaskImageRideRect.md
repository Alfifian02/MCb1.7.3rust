# layerExAlpha — MaskImageRideRect(src, a, l, r, c1, c2, c3)

Biner: `libgame.so` arm64 1.3.9, fungsi `0x5d7fe8`, 1384 B. Status: **terbaca penuh dari recon**; belum diuji di perangkat.

## Tanda tangan (TJS)
`Layer.MaskImageRideRect(src, a, l, r, red, green, blue)` — `x1 = src (Layer)`, `w2 = a`, `w3 = l`, `w4 = r`, `w5 = c1`, `w6 = c2`, `w7 = c3`.
Urutan warna: `c1 → byte 2 (R)`, `c2 → byte 1 (G)`, `c3 → byte 0 (B)` (ASUMSI nama R,G,B dari urutan itu).

## Alur
1. Ambil Layer `src` (`Not Layer` bila gagal); `spitch`, `sbuf` dari pembungkus `0x807c20/0x807c00`. `dbuf`/pitch = `this+0x18`/`this+0x20` **yang tersimpan** (tidak disegarkan di method ini).
2. `n = (l+r)+1`. Tabel `T0 float[n]`, `T1,T2,T3 int[n]` (nol-diisi). Konstanta:
   `d2 = 1.0/(double)(l+1)`, `d3 = (double)c1/(double)(l+1)`,
   `d4 = (double)((float)c2/(float)(l+1))`, `d5 = (double)((float)c3/(float)(l+1))`,
   `d6 = (double)(1.0f/(float)(r+1))`, `d7 = (double)((float)c1/(float)(r+1))`, `d16 = (double)((float)c2/(float)(r+1))`, `d17 = (double)((float)c3/(float)(r+1))`.
3. Untuk `i = 0..n-1`:
   - `i < l`: `k=(double)(i+1)`; `T0=(float)(d2*(l-i))`; `T1=⌊d3*k+0.5⌋`; `T2=⌊d4*k+0.5⌋`; `T3=⌊d5*k+0.5⌋`
   - `i == l`: `T0=0`, `T1=c1`, `T2=c2`, `T3=c3`
   - `i > l`: `m=(double)(n-i)`; `T0=(float)(d6*(double)(i-l))`; `T1=⌊d7*m+0.5⌋`; `T2=⌊d16*m+0.5⌋`; `T3=⌊d17*m+0.5⌋`
   - penjepit: `T0 >= 1 → 1`; `T1..3 >= 255 → 255`; `!(T0 > 0) → 0` (NaN tetap); `T1..3 <= 0 → 0`. (`⌊⌋` = `fcvtzs`, jenuh)
4. `hi = min(a+r,255)`, `off = a-l`, `lo = max(0,off)`. Untuk tiap piksel tujuan:
   - alfa tujuan `== 0` → lewati;
   - `m = alfa sumber`; `lo > m` → lewati; `m > a` dan `hi <= m` → lewati (**`m == hi` dilewati**);
   - `idx = m-off`; `B' = T3[idx] + ⌊T0[idx]*B + 0.5f⌋`, `G' = T2[idx] + ⌊T0[idx]*G+0.5f⌋`, `R' = T1[idx] + ⌊T0[idx]*R+0.5f⌋`, disimpan **low-byte (tanpa clamp, bisa membungkus)**; alfa tidak diubah.
5. `redraw()`.

## Ringkas
Mewarnai piksel (yang alfanya ≠ 0) dengan campuran warna `(c1,c2,c3)` berbobot "tenda" dari alfa mask `src` di sekitar `a` (kiri `l`, kanan `r`); tujuan dikalikan `T0 = 1 - tenda` lalu ditambah warna×tenda.

## Kasus tepi
- `n < 0` → fork: `TJS_E_INVALIDPARAM`; `n == 0` → tabel kosong, piksel yang butuh indeks dilewati (aslinya: deref null bila `m == a`).
- `l+1 == 0` / `r+1 == 0`: pembagian float/double dengan nol (inf/NaN), hanya dipakai pada cabang yang tidak dieksekusi bila `l = -1`; ditiru apa adanya.
- Ukuran sumber < tujuan: aslinya baca di luar buffer; fork memotong ke irisan. Layer tanpa gambar: no-op.

## Tingkat keyakinan
- **Dibaca dari recon** (`recon_alpha2.txt`, 2026-11-22): alur, rumus, konstanta, urutan operasi, offset medan.
- **ASUMSI**: nama parameter TJS (diturunkan dari pemakaian); semantik `redraw()` = `Update(klip)` (vtable[1] objek `layerExBase_GL`, cocok dengan `layerExBase.hpp` fork, tidak dibongkar); `reset()` dianggap dipanggil pembungkus sebelum method (aslinya method ini hanya memakai `_width/_height` dan, kecuali LevelMaskRect(2), `_buffer/_pitch` yang tersimpan).
- Pembungkus Layer (dibaca tangan, lihat `layerExAlpha_S7_profil_xref.md`): `0x807c00` = pointer tulis buffer utama (menandai berubah), `0x807c20` = pitch.
- Perilaku sebelum `redraw`: tidak ada pembatasan ke klip; seluruh gambar `0..W-1 x 0..H-1` diproses.
- Belum diuji di perangkat/NDK; uji inti host: `tests/plugins/layerExAlpha/test_alpha2_s7s10.cpp`.
