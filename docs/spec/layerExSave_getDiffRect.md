# getDiffRect — spesifikasi (S11)

- `LayerExSave.dll` · impl `0x5eb454` (916 B) · konvensi: `layerExSave_common.md`.
- Tanda tangan: `Layer.getDiffRect(layer)` — kotak pembatas perbedaan antara `layer` (param) dan `this`. `numparams < 1` → `-1004`.
  Param[0] bukan objek → galat konversi; bukan Layer → pesan "specify layer" (ASUMSI).
- Kedua layer dibaca (tanpa tulis). **Ukuran gambar harus sama**, jika tidak → `Different layer size.`
- **Kesamaan piksel:** alfa harus sama; bila alfa == 0 maka warna **diabaikan** (sama); bila alfa != 0, B,G,R juga harus sama. Piksel "aktif" = tidak sama.
- Rect dicari seperti `getCropRect` (kiri/kanan/atas/bawah), hasil `Dictionary {x,y,w,h}`; **`void`** bila tak ada beda.
- ⚠️ **BUG BINER (terbukti di emulator):** pemindai baris-**bawah** dimulai dari kolom **kanan** (`bfi x11,x10,#2,#62` @`0x5eb708`; `getCropRect*` memakai kolom kiri `x1`)
  dan memeriksa `kanan-kiri+1` kolom ke **kanan**, jadi bawah = baris terakhir yang beda di kolom `kanan..` (bukan di [kiri,kanan]). Contoh: beda hanya di (2,0) dan (0,5)
  pada 6×6 → rect benar `(0,0,3,6)`, biner `(0,0,3,1)`. Kolom ≥ lebar dibaca di luar batas (UB di biner).
  Fork: `LAYEREXSAVE_DIFFRECT_BOTTOM_FROM_RIGHT` = **1** (meniru; kolom ≥ lebar dianggap tak beda) — set 0 untuk rect benar. **Putuskan di 7.3/S13.**
- Kebocoran 2 larik baris pada jalur `void` (tidak ditiru). Tanpa `Update()`.
- Kode: `layerex_save::diff_rect`, `pixel_same`. Uji: `test_core.cpp` (+ 12 golden dari emulasi), TJS `getDiffRect.BUG_BINER`.
