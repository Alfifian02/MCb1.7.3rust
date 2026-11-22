# Spesifikasi: `Layer.generateWhiteNoise` (layerExImage.dll)

Bagian umum: [`layerExImage_light.md`](layerExImage_light.md). Implementasi **`0x593b94`** (716 B), thunk `0x596550` (208 B). Callee: `rand()` (`0x416e40`), `TVPGetRandomBits128` (`0xa28f38`).
Kode: `white_noise_apply`, `white_value`, `noise_seed_float`.

## Tanda tangan
`this.generateWhiteNoise()` — tanpa argumen.

## Perilaku CPU (seluruh gambar)
Untuk setiap piksel (baris demi baris): `r = rand()`; `v = trunc(r · 0xff0001 / 2⁴⁷)` (`smull` lalu geser aritmetik 47 dengan koreksi tanda → pembagian memotong ke nol);
byte 0, 1, 2 = `v` (abu-abu; satu `rand()` per piksel); **byte 3 (alfa) utuh**. Lalu `redraw()`.
`rand()` bionic ∈ [0, 2³¹−1] → `v` ∈ [0, 255]; nilai 255 hanya muncul untuk `r` mendekati maksimum (terukur: `0x7fffffff` dan `2147483641` → 255).

## GPU
Shader `LayerEx_WhiteNoise`: `rgb = vec3(fract(sin(dot(uv, seed)) · 43758.5453))`; `seed = (w[0], w[1])` dari `TVPGetRandomBits128`, tiap komponen `(float)(u32·2⁻²⁸ + 16.0)`.
Teks asli dibaca dari `0x14c46d3`; aslinya `s.rgb = fract(...)` (skalar ke vec3), fork menulis `vec3(fract(...))` agar sah di GLSL ES. **Belum diuji di perangkat**.

## Kasus tepi
Gambar 0×0 → tidak ada piksel diproses (tetap `redraw()`). Urutan `rand()` global libc (tanpa `srand` di binary yang terlihat) → tidak dapat dibandingkan lintas proses; uji memakai `rand` suntikan.

## Uji
3 ukuran golden + nilai ekstrem `rand` (stub) + analitik (`white_value(0)=0`, `white_value(0x7fffffff)=255`, `white_value(1<<30)=127`). **Belum**: GPU, perangkat.
