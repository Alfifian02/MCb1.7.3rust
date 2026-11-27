# fillAlpha — spesifikasi perilaku

- Modul `layerExBTOA.dll`; nama @`0x14ca6a0`; implementasi `0x5e1e34` (188 B).

## Tanda tangan
`Layer.fillAlpha()` — **tanpa parameter** (tidak ada argumen alfa; nilai tetap `0xFF`). `TJS_S_OK`; instance gagal → `"dest must be Layer."`.

## Efek
1. `SyncImage(layer)` (`0x80c848`).
2. `MainImage->FillMask(ClipRect, 255)` (`0x7f84b0`; `ClipRect` diteruskan **by value** di `x1,x2`; hasil bool diabaikan).
   `FillMask`: rect dipotong ke `[0,lebar)×[0,tinggi)` gambar; kosong → tidak melakukan apa pun; gambar bukan 32bpp → pengecualian;
   selain itu render method bawaan `"FillMask"` dengan parameter `opacity = 255` → **alfa area klip := 255** (RGB tetap).
3. `Update(ClipRect, false)` (native).
- **Tidak** menyetel `ImageModified`/flag apa pun. Satu jalur untuk CPU dan GPU (tanpa `IsSoftware()`); perbedaan ditangani `FillMask`.

## Kasus tepi
Klip kosong → `FillMask` tidak mengubah apa pun, `Update` dengan rect kosong tetap dipanggil. Layer tanpa gambar utama → biner crash (fork: tidak melakukan apa pun, D5).

## Uji
Tidak ada inti piksel di plugin (seluruhnya `FillMask` milik inti fork); **belum teruji** selain sintaks. Emulasi tidak mencakup `FillMask` (render manager).
