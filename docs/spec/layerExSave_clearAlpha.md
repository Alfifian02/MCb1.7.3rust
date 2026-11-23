# clearAlpha — spesifikasi (S11)

- `LayerExSave.dll` · impl `0x5ec50c` (472 B) · konvensi: `layerExSave_common.md`.
- Tanda tangan: `Layer.clearAlpha([threshold [, color]])` — **semua parameter opsional**, tidak ada `BADPARAMCOUNT`. Tanpa nilai kembali. Menulis `this`.
- `threshold` (tidak ada → 0): integer 32-bit **bertanda**. `color` (tidak ada → 0): `color & 0xFFFFFF`. `void` → 0 untuk keduanya.
- Efek: untuk tiap piksel dengan **`threshold >= alfa`** (perbandingan bertanda; biner: `cmp w20,w13; b.lt` melewati bila `threshold < alfa`): piksel **seluruhnya** := `color & 0xFFFFFF`
  → B,G,R dari warna dan **alfa = 0**. Piksel lain tak berubah.
  Default: alfa 0 → `0x00000000`. `threshold >= 255` → semua piksel. `threshold < 0` → tidak ada.
- Kode: `layerex_save::clear_alpha`. Uji: emulator (27 kombinasi) + 6 golden; TJS.
