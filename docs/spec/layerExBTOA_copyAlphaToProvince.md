# copyAlphaToProvince — spesifikasi perilaku

- Modul `layerExBTOA.dll`; nama @`0x14ca6b4`; implementasi `0x5e1f74` (1.356 B; kode NEON untuk bagian lebar ≥ 32).
- Menjawab pertanyaan S3: method ini memang memakai **gambar province** (8 bpp) sebagai tujuan; sumber = alfa gambar utama **layer yang sama**.

## Tanda tangan
`Layer.copyAlphaToProvince([level])` — `level` opsional. Tidak ada `TJS_E_BADPARAMCOUNT`.
- `level`: absen **atau void** → `-1`; selain itu `tTVInteger` dipotong ke 32 bit (objek/oktet → galat konversi; string di-parse; real dipotong).
- Mode dari `level`: **0** bila `level < 0`; **1** bila `0 ≤ level ≤ 255`; **2** bila `level > 255`.

## Urutan operasi (dan galat)
1. Ambil instance → gagal: `"dest must be Layer."`.
2. Konversi `level` (galat konversi bisa terjadi di sini, setelah langkah 1).
3. `ClipRect` → `left, top, w = right-left, h = bottom-top`. Pitch gambar utama.
4. `GetMainImagePixelBuffer()` null → `"src has no image."` (@`0x14ca6dc`). Tanpa gambar utama, w/h/pitch di biner tak terinisialisasi tetapi sudah tak terpakai karena galat ini.
5. `GetProvinceImagePixelBufferForWrite()` (mengalokasikan province bila belum ada; menyetel `ImageModified`); null → `"dst has no province image."` (@`0x14ca700`).
6. Pitch province == 0 → `"dst has no province pitch."` (@`0x14ca736`).
7. Loop (di bawah). 8. `Update(rect(left, top, left+w, top+h), false)` — **selalu** dipanggil, juga bila `w < 1` atau `h < 1` (rect kosong).

## Efek piksel (area klip; baris y∈[0,h), kolom x∈[0,w))
`a = alfa utama(left+x, top+y)`; `P = province(left+x, top+y)`:
| mode | `P :=` |
|---|---|
| 0 (`level<0`, default) | `a` |
| 1 (`0..255`) | `a >= level ? 1 : 0` (bandingkan tak bertanda dengan `level & 0xFF`) |
| 2 (`>255`) | `0` (di biner dengan `memset` per baris `w` byte) |
`h < 1` atau `w < 1` → tidak ada yang ditulis. Hasil jalur NEON (blok 32 piksel) = jalur skalar (sisa 1..32 piksel); diverifikasi emulasi.
Byte di luar area klip (termasuk padding pitch) tidak disentuh.

## Uji
[SPEC] tiga mode + [EMU] 1.155 kasus: lebar {1,2,3,31,32,33,37,63,64,65,70,100} × tinggi 1–3 × level {absen, void, -5, -1, 0, 1, 77, 128, 255, 256, 1000}
× klip {penuh, kiri+1, atas+1} × padding utama/province. Pelacak akses menegaskan biner tidak membaca/menulis di luar buffer.
