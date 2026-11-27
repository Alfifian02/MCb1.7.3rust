# Spesifikasi: `Layer.noise` (layerExImage.dll)

Bagian umum: [`layerExImage_light.md`](layerExImage_light.md). Implementasi **`0x593464`** (1.840 B), thunk `0x5963ac`.
Callee: **`0xa28f38`** = `TVPGetRandomBits128(void*)` (260 B, 16 byte acak; sudah ada di fork `src/core/utils/Random.cpp`), **`0x416e40`** = `rand()` libc (PLT).
Kode: `noise_apply`, `noise_add`, `noise_seed_float`.

## Tanda tangan
`this.noise(level)` — satu argumen **wajib** (integer int32). `level == 0` → kembali tanpa apa pun (kedua jalur).

## Perilaku CPU (seluruh gambar)
1. `w[0..3] = TVPGetRandomBits128()`; `C = w[0]`, `A = w[1]`, `B = w[2]` (`w[3]` tidak dipakai). **Tidak deterministik** (seed acak tiap panggilan).
2. Per baris `y`, `x = 0`. Selama sisa piksel ≥ 4 (`do { … } while (x < lebar − 3)`, hanya bila `lebar ≥ 4`), per **blok 4 piksel**:
   `t = x + A`, `u = B ^ y`, `n0 = t·C + u`, `n1 = t·n0 + u`, `n2 = t·n1 + u` (uint32 melingkar), lalu **`C = n2`** (keadaan berlanjut lintas blok dan baris).
   12 byte `n0,n1,n2` (little-endian) menjadi derau bertanda (`int8`) untuk kanal byte 0,1,2 dari 4 piksel berurutan (alfa dilewati).
3. Sisa piksel di ujung baris (0..3, atau seluruh baris bila `lebar < 4`): tiap piksel memanggil `rand()` dua kali (`r1`, `r2`):
   `d0 = int8(r2)`, `d1 = int8(r2 >> 8)`, `d2 = int8(((r2 | r1<<16) >> 16))` untuk byte 0,1,2.
4. Penambahan: `v = piksel + ((int32)(d · level) >> 8)` (geser aritmetik, perkalian int32 melingkar), dijepit 0..255. Alfa utuh. Lalu `redraw()`.

## GPU
Shader `LayerEx_Noise` (tiga seed `vec2`, `level`): `kanal += (fract(sin(dot(uv, seedN)) · 43758.5453) − 0.5) · level`. Uniform `level = level/255.0f`;
seed1 = (`w[0]`,`w[1]`), seed2 = (`w[2]`,`w[3]`) dari panggilan `TVPGetRandomBits128` pertama, seed3 = (`w[0]`,`w[1]`) dari panggilan kedua;
tiap komponen `(float)(u32 · 2⁻²⁸ + 16.0)` (konstanta double `0x14c3180`). Teks shader asli dibaca dari `0x14c44d5` (nama `LayerEx_Noise`, `0x14c44c7`); shader fork setara (hanya format). **Belum diuji di perangkat**.

## Kasus tepi / catatan uji
- `level` negatif: derau dibalik tanda. `level` ekstrem: perkalian melingkar (tidak crash).
- Karena seed acak, uji diferensial memakai seed dan urutan `rand()` yang **disuntikkan** (stub di emulator; `noise_apply` menerima seed dan fungsi rand).
- Distribusi derau sebenarnya (pada perangkat) tidak diuji.

## Uji
15 kasus golden (ukuran 1×1 … 11×3, level 10/255/−30, termasuk jalur sisa) + analitik (alfa utuh, jepit, geser aritmetik). **Belum**: GPU, perangkat.
