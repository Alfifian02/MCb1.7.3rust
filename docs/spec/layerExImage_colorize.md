# Spesifikasi: `Layer.colorize` (layerExImage.dll)

Bagian umum: [`layerExImage_light.md`](layerExImage_light.md). Implementasi **`0x5924d8`** (1.268 B), thunk `0x595cf8` → `0x595dec`.
Helper CPU: **`0x5929cc`** (304 B, RGB→HLS bilangan bulat) dan **`0x592afc`** (500 B, HLS→RGB float). Kode: `colorize_apply`, `rgb_to_hls`, `hls_to_rgb`.

## Tanda tangan
`this.colorize(hue, sat, blend)` — **tiga argumen wajib**. `hue`, `sat`: integer (int32); `blend`: real (double). (Pada fungsi native: `x0=this, w1=hue, w2=sat, d0=blend`.)

## Perilaku
1. `blend <= 0.0` → tidak melakukan apa pun (NaN **tidak** dianggap ≤ 0; lanjut dan hasilnya tak berubah). Berlaku untuk kedua jalur.
2. `bd = min(blend, 1.0)`. CPU: `bi = fcvtzs(bd · 256)` (0..256), `inv = 256 − bi`.
3. CPU, per piksel seluruh gambar: `pk = byte2 | byte1<<8 | byte0<<16` → `hl = rgb_to_hls(pk)` → ganti **H = hue & 0xff** (bit 16..23) dan **S = sat & 0xff** (bit 8..15), L tetap →
   `rgb = hls_to_rgb(...)` (bit 16..23 → byte 0, 8..15 → byte 1, 0..7 → byte 2).
   - `bd > 0.999` (konstanta double `0x14c3150` = 0.999000012874603) → piksel diganti penuh.
   - selain itu: `byte_i = (baru_i · bi + inv · lama_i) >> 8` (uint32). Alfa tidak diubah.
4. `rgb_to_hls` (skala 255): `L = ((cmax+cmin)·255 + 255) / 510`; abu-abu → `S=0, H=170`; selain itu rumus HLS klasik **kecuali** H negatif **tidak** dinormalisasi
   (hanya 8 bit rendah) dan beberapa suku dipotong 16-bit — ditiru persis (diuji 60.000+ nilai terhadap fungsi asli, 0 selisih).
5. `hls_to_rgb`: float 32-bit; `H` skala 0..255 → derajat `H·360/255`; `S=0` → abu-abu `trunc(L/255·255)`; potongan-potongan standar (60/180/240°); hasil `fcvtzs(f·255)`.
6. GPU: shader `LayerEx_Colorize` (HSL→ ganti H dan S → `mix(rgb, baru, blend)`). Uniform: `blend = min(blend,1)`, `hue = hue>0 ? min(hue,255)/255 : 0`, `sat` sama.

## ⚠️ Perbedaan CPU vs GPU yang terbaca (kemungkinan perilaku asli)
- CPU memotong `hue`/`sat` ke 8 bit (`256 → 0`); GPU menjepit ke 255.
- **Urutan byte**: pada `colorize` CPU, **byte 0 berperan sebagai "R"** dan byte 2 sebagai "B" (rgb_to_hls menganggap byte tinggi = R). Pada `modulate` CPU, **byte 2 = R**.
  Salah satu dari keduanya tidak cocok dengan shader GPU (yang memakai `.rgb` yang sama untuk keduanya) — tergantung urutan byte memori layer di fork. Tidak bisa diputuskan tanpa perangkat (S13).

## Kasus tepi
- `blend` kecil (0 < blend < 1/256) → `bi = 0` → gambar tidak berubah (tetap `redraw()`).
- `hue`/`sat` di luar 0..255 → CPU: dilipat `& 0xff`.

## Uji
6 kasus golden (blend 1.0 / 0.5 / 0.999 / 0.9995 / 0.2 / 0.75 dengan hue/sat berbeda, termasuk 256/257) + analitik. Helper diuji terpisah terhadap fungsi asli. **Belum**: GPU, NDK, perangkat.
