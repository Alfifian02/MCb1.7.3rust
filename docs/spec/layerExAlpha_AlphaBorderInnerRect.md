# layerExAlpha.dll — `Layer.AlphaBorderInnerRect(mask, th)`

- impl `0x5db974` (1.324 B). Hal umum: [`layerExAlpha_common.md`](layerExAlpha_common.md). Kode: `border_inner` di `layerExAlphaCore.hpp`. Nama parameter perkiraan.

## Langkah
1. Ambil Layer `mask` (x1; bukan Layer → "Not Layer"), `th` = w2.
2. **Hanya lebar** dibandingkan: `this._width == mask.imageWidth`. Tidak cocok → log 2 baris (`SW:<w> SH:<h>`, `AW:<w> AH:<h>`), selesai tanpa redraw. **Tinggi tidak diperiksa.**
3. Untuk setiap baris `0 … this._height-1` dan kolom `0 … this._width-1`: bila `mask.A >= th` (bertanda) → `dst.A = 0`; warna dst tetap. Tanpa syarat alfa dst/mask lain (berbeda dari Rect7/8).
4. `redraw()` (juga bila tinggi `< 1`).

`th <= 0` → seluruh alfa dst menjadi 0; `th > 255` → tak ada yang berubah.

## Kasus tepi / deviasi
- `mask` **lebih pendek** dari `this`: binary membaca di luar buffer `mask` (UB). **Fork:** dianggap tidak cocok → log + selesai. `mask` lebih tinggi tetap diterima seperti di binary (hanya `this._height` baris diproses).
- Lainnya sama dengan Rect7 (null → crash di binary; layer > gambar).
- Nama "InnerRect" tidak terlihat di kode: tidak ada rect/clip di impl (**ASUMSI** hanya penamaan).
