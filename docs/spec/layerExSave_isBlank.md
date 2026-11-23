# isBlank — spesifikasi (S11)

- `LayerExSave.dll` · impl `0x5ec190` (760 B) · konvensi: `layerExSave_common.md`.
- Tanda tangan: `Layer.isBlank(x, y, w, h)`; `numparams < 4` → `-1004`. Hasil integer **1** (blank) atau **0**. Hanya membaca `this`.
- ⚠️ **Yang diperiksa adalah byte BIRU (byte 0 piksel), bukan alfa.** Blank ⇔ semua byte biru di rect = 0 (cocok dengan `copyBlueToAlpha`/peta biru).
- Validasi (setelah lebar/tinggi diambil; semua → `invalid layer range`): `x<0` atau `y<0` atau `w<0` (tanda `h` **tidak** diperiksa), `x+w > lebarGambar`, `y+h > tinggiGambar`.
  `w < 1` atau `h < 1` (termasuk `h` negatif yang lolos) → **1** tanpa memindai.
- ⚠️ Biner menjumlahkan `x+w` dalam 32-bit melingkar: `x=0x7FFFFFFF, w=2` lolos validasi lalu membaca di luar batas (UB; terbukti di emulator). Fork menjumlah 64-bit dan menolak (deviasi sah).
- Biner membocorkan larik baris pada hasil 0 dan pada galat (tidak ditiru). Pemindaian: baris luar, kolom dalam; berhenti di biru ≠ 0 pertama.
- Kode: `layerex_save::is_blank`, `blank_range_ok`. Uji: emulator (rentang 11 kasus + 48 kombinasi isi) + 12 golden; TJS.
