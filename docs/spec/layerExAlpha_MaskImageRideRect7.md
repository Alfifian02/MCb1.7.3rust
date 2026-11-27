# layerExAlpha.dll — `Layer.MaskImageRideRect7(image, mask, a, b)`

- impl `0x5da79c` (2.548 B), registrasi lewat fungsi `0x5d43b0` (bersama method Alpha lain). Hal umum: [`layerExAlpha_common.md`](layerExAlpha_common.md).
- Kode: `src/plugins/layerExAlphaCore.hpp` (`ride_table`, `ride_range`, `ride7`), `src/plugins/layerExAlpha.cpp`. Uji: `tests/plugins/layerExAlpha/test_alpha4_s9.cpp`, `tests/tjs/t_alpha4.tjs`.
- Nama parameter **perkiraan**. Tingkat: baca disassembly; belum dibandingkan dengan biner.

## Argumen
`this` = tujuan. `image` = Layer sumber warna+alfa (x1). `mask` = Layer yang **hanya alfanya** dibaca (x2). `a`, `b` = int (w3, w4). Tidak ada rect meski namanya "Rect".

## Langkah
1. Ambil native instance kedua Layer (gagal → "Not Layer"). Cek ukuran (common). Tidak cocok → log 3 baris, selesai.
2. `n = 2*b - 1` (int 32-bit, lalu `sxtw`) → `vector<float>` berukuran `n` (`0x406260`). `b <= 0` atau `b >= 2^30` → `n` negatif → ukuran ≈ 2^64 → **`std::length_error`** (tabel tak dibuat, tak ada redraw). Hanya bila `b >= 1` tabel diisi:
   `inv = 1.0f/(float)b`; `i < b-1`: `inv*(float)(i+1)`; `i == b-1`: `1.0f` (tanpa perkalian); `i > b-1`: `inv*(float)(n-i)`; lalu `min(v,1.0f)`; `v <= 0 → 0`. (Jalur NEON 4-lebar di `0x5dad04` dan jalur skalar identik.)
3. Ambang (int 32-bit): `lo = (a-b < 0) ? 0 : min(a-b+1, 255)`; `hi = min(max(a+b, 0), 255)`.
4. Untuk setiap piksel (baris atas→bawah, kolom kiri→kanan), **hanya bila** `dst.A != 0` **atau** `image.A != 0`, dengan `m = mask.A`:
   - `m < lo` (bertanda): tidak berubah.
   - `m >= hi`: `dst.A = 0`, warna dst tetap.
   - selain itu: `dst.B,G,R = image.B,G,R`; `dst.A = trunc( tab[m-lo] * (float)image.A + 0.5f )` (fmul lalu fadd **terpisah**, bukan fmadd; `fcvtzs`; simpan byte rendah).
5. `redraw()`.

Indeks `m-lo` selalu `0 … 254` dan `< n` (dibuktikan lengkap untuk `a∈[-700,900]`, `b∈[1,300]` di uji); karena itu tabel boleh dipotong 256 entri di fork.

## Kasus tepi
| kasus | binary | fork |
|---|---|---|
| `b <= 0` | `std::length_error` | `TJS_E_INVALIDPARAM` (DEVIASI; jenis galat ncbind **tidak diketahui**) |
| `b >= 2^30` | `length_error`/`bad_alloc` | `TJS_E_INVALIDPARAM` untuk `>= 2^30`; di bawahnya diproses tanpa alokasi besar |
| `a` mendekati ±2^31 (lo/hi melingkar) | `lo` bisa `INT_MIN` → indeks tabel di luar batas (UB) | bobot 0 untuk indeks di luar tabel (DEVIASI) |
| argumen null/bukan Layer | null → crash; non-Layer → "Not Layer" | "Not Layer" untuk keduanya (DEVIASI untuk null) |
| layer lebih besar dari gambarnya | tulis di luar buffer | dianggap ukuran tak cocok (log) |
| `image == this` / `mask == this` | per piksel baca-sebelum-tulis → aman | sama |

## ASUMSI
`reset()` oleh pengait; slot `+8` = redraw; `0xa16ca4` = log; perilaku GPU; nama/urutan parameter; jenis galat untuk `b <= 0`.
