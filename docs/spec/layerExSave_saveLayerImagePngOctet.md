# saveLayerImagePngOctet — spesifikasi (S12)

- `LayerExSave.dll` · kelas **Layer** · impl `0x5ecc78` (484 B) → inti `0x5e9aa4` (404 B) · konvensi: `layerExSave_B_common.md`; format PNG: `layerExSave_saveLayerImagePng.md`.
- Tanda tangan: `Layer.saveLayerImagePngOctet([level])` → **octet** berisi PNG. Fungsi mentah `(result, numparams, param, objthis)`.
- `level`: `numparams == 0` → **1**; selain itu `AsInteger` (void → 0). Dipakai sebagai level `deflateInit` (tak sah → `deflate initialize`).
- Bila `result == null` → **tidak melakukan apa pun** (layer tidak diperiksa). Selain itu: `result = ""` (string kosong, `0xa0e778`) lebih dulu; `NativeInstanceSupport` Layer (gagal → pesan "specify layer", hasil tetap string kosong);
  PNG disusun dengan IHDR + IDAT + IEND (`0x5e843c`, `0x5e8fc4`) — **tanpa chunk metadata** (`0x5e87a0` tidak dipanggil) dan tanpa callback; hasil = octet dari buffer (`0xa0c9f4`/`0xa0e6e4`).
- Kode: `SaveLayerImagePngOctet`, `png_encode(meta = PngOptions(comp_lv = level))`. Uji: inti-host (level, struktur), TJS (tipe `Octet`, level 0/9, level tak sah melempar; tanda tangan PNG diperiksa lewat `Scripts.getTraceString` bila tersedia).
