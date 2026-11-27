# oozeColor — spesifikasi (S11)

- `LayerExSave.dll` · impl `0x5ebbf0` (888 B) · konvensi: `layerExSave_common.md`.
- Tanda tangan: `Layer.oozeColor(level [, threshold [, color]])`. `numparams < 1` → `-1004`. Tanpa nilai kembali. Menulis `this`.
- `level` = integer 32-bit; **`level < 1` → `Invalid level count.`** (diperiksa sebelum apa pun disentuh).
- `threshold`: bila **tidak ada** (numparams ≤ 1) → **1**; bila ada: nilai param (`void` → **0**, bukan default). `color`: tidak ada → 0; ada: nilai (`void` → 0).
- **Efek piksel (terbaca penuh & dieksekusi):** `t = (threshold & 0xFF) != 0 ? threshold & 0xFF : 1`. Piksel dengan **alfa < t** (tak bertanda):
  `B = color & 0xFF`, `G = (color>>8) & 0xFF`, `R = (color>>16) & 0xFF`; **alfa tidak diubah**. Piksel dengan alfa ≥ t: tidak diubah.
- ⚠️ **Temuan:** biner membangun peta bantu `(W+2)×(H+2)` (di-`memset` 0; piksel alfa ≥ t → `0xFF`) lalu mengulang `level` kali
  "bila peta != 0 maka peta = 0xFF" — **idempoten, tanpa efek pada gambar** (tidak ada penyebaran ke piksel tetangga). Jadi `level` hanya divalidasi (≥ 1) dan menambah waktu;
  nama "ooze" (merembes) menyiratkan penyebaran yang **tidak ada** di 1.3.9. Fork tidak membangun peta dan tidak mengulang. (Level sangat besar di biner = lambat; di fork instan.)
- Kode: `layerex_save::ooze_color`. Uji: emulator (≈108 kombinasi, ambang 0/1/2/77/255/256/257/0x1FF, level 1–5, param void/real/string/objek) + 3 golden; TJS `oozeColor.*`.
