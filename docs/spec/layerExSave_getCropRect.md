# getCropRect / getCropRectZero — spesifikasi (S11)

- Modul `LayerExSave.dll` · `getCropRect` impl `0x5eab30` (476 B) · `getCropRectZero` impl `0x5eb184` (588 B) · konvensi: `layerExSave_common.md`.
- Tanda tangan: `Layer.getCropRect()`, `Layer.getCropRectZero()` — **tanpa parameter**; `numparams` **tidak diperiksa**. Dipanggil pada `this`.
- Hasil: `Dictionary {x, y, w, h}` = kotak pembatas piksel "aktif" pada **gambar utama**; **`void`** bila tidak ada piksel aktif (hasil dikosongkan dulu).
- Piksel aktif: `getCropRect` → **alfa != 0**; `getCropRectZero` → **piksel 32-bit != 0** (salah satu byte B,G,R,A tak nol).
- Algoritma (kedelapan fungsi persis begini, dibaca penuh): `kiri` = kolom pertama (dari 0) yang punya piksel aktif · `kanan` = kolom terakhir ·
  `atas` = baris pertama yang punya piksel aktif di kolom [kiri,kanan] · `bawah` = baris terakhir di kolom [kiri,kanan] ·
  `{x:kiri, y:atas, w:kanan-kiri+1, h:bawah-atas+1}`. `lebar<1` atau `tinggi<1` → `void`.
- Kasus tepi: layer tanpa gambar → biner crash (fork: melempar `TVPNotDrawableLayerType`). Tidak ada `Update()`.
- Kode: `layerex_save::crop_rect_alpha/crop_rect_zero` (`layerExSaveCore.hpp`). Uji: `test_core.cpp` + emulator (48 kombinasi/fungsi);
  TJS `t_layerExSaveA.tjs`.
- Belum diketahui: peran persis `0x80c848` (sinkronisasi) — tidak memengaruhi hasil di inti.
