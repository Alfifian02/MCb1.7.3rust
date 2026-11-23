# copyBlueToAlpha — spesifikasi (S11)

- `LayerExSave.dll` · impl `0x5ebfec` (288 B) · konvensi: `layerExSave_common.md`.
- Tanda tangan: `Layer.copyBlueToAlpha(layer)`. Param[0] harus objek (bukan → galat konversi); bukan Layer → pesan "specify layer" (ASUMSI).
- ⚠️ Biner **tidak memeriksa `numparams == 0`** (hanya `< 0`, tak pernah benar) → membaca param tak sah (UB). Fork: `TJS_E_BADPARAMCOUNT`.
- Efek: untuk wilayah `min(lebar) × min(tinggi)` dari (0,0): `this.alfa := layer.biru` (byte 0 dari piksel sumber). Piksel lain tak berubah; B,G,R `this` tak berubah.
  **Tidak ada** galat ukuran beda (berbeda dengan `getDiffRect/getDiffPixel`). `layer` dibaca, `this` jalur tulis. `layer == this` boleh.
- Kode: `layerex_save::blue_to_alpha`. Uji: emulator (5 ukuran + alias) + 2 golden; TJS.
