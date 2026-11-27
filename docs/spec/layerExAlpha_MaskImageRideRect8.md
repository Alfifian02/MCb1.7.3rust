# layerExAlpha.dll — `Layer.MaskImageRideRect8(image, mask, th)`

- impl `0x5db190` (2.020 B). Hal umum: [`layerExAlpha_common.md`](layerExAlpha_common.md). Kode: `ride8` di `layerExAlphaCore.hpp`. Nama parameter perkiraan.

## Langkah
1. Sama dengan Rect7: ambil Layer `image` (x1) dan `mask` (x2), cek ukuran, log 3 baris bila tidak cocok. **Tidak ada tabel bobot** dan tidak ada galat `b`.
2. Per piksel, **hanya bila** `dst.A != 0` atau `image.A != 0`: bila `mask.A > th` (bertanda) → salin **keempat byte** `image → dst` (B,G,R,**A**); selain itu tidak berubah.
3. `redraw()`.

`th < 0` → semua piksel aktif disalin; `th >= 255` → tidak ada yang berubah. Dibanding Rect7: tanpa pemetaan bobot dan alfa disalin utuh (bukan dikali).

## Kasus tepi / deviasi
Sama dengan Rect7 untuk argumen null/non-Layer, layer lebih besar dari gambar, dan aliasing. ASUMSI sama.
