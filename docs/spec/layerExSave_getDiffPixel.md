# getDiffPixel — spesifikasi (S11)

- `LayerExSave.dll` · impl `0x5eb86c` (768 B) · konvensi: `layerExSave_common.md`.
- Tanda tangan: `Layer.getDiffPixel(layer [, same [, diff]])` (nama `same`/`diff` = **ASUMSI**, dari perilaku). `numparams < 1` → `-1004`.
- `same` dan `diff`: integer 32-bit; **jenis `void` = tidak diberikan** (tidak menulis); `numparams < 2/3` = tidak diberikan.
- Kerja: untuk tiap piksel (baris luar, kolom dalam) bandingkan `layer` dengan `this` (kesamaan = `getDiffRect`: alfa sama; alfa 0 → warna diabaikan):
  - **sama** → bila `same` diberikan: piksel `this` := `same` (32-bit penuh, termasuk alfa);
  - **beda** → bila `diff` diberikan: piksel `this` := `diff`; **hitung++** (dihitung walau `diff` tidak diberikan).
- Hasil: **jumlah piksel beda** (integer). `this` ditulis (jalur tulis); `layer` hanya dibaca. Ukuran beda → `Different layer size.`
  (urutan: `layer` dibaca dulu, lalu `this` jalur tulis, baru ukuran dibandingkan).
- `layer == this` boleh: semua sama, hasil 0, piksel := `same`.
- Kode: `layerex_save::diff_pixel`. Uji: emulator (48 kombinasi incl. alias, void+int) + golden 2 kasus; TJS.
