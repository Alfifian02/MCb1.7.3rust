# saveLayerImagePng — spesifikasi (S12)

- `LayerExSave.dll` · kelas **Layer** · impl `0x5ecb4c` (168 B) → jalur file `0x5ea138` (348 B) → `0x5ed5e0` (424 B) · penyusun PNG `0x5e8210` dkk · konvensi: `layerExSave_B_common.md`.
- Tanda tangan: `Layer.saveLayerImagePng(nama [, opsi])`; `numparams < 1` → `-1004`; `nama` string/void, `opsi` objek (aturan sama dengan `saveLayerImageTlg5`). Tanpa nilai kembali. Sinkron, tanpa callback (callback/tugas = null).

## Alur
`0x5ed5e0`: (1) `NativeInstanceSupport` Layer (gagal → pesan "specify layer"); `0x80c848`; bitmap = `[[ni+0x118]+0x58]`; `lebar = bitmap[+0xC]`, `tinggi = bitmap[+0x10]`, `pitch = vfn#10()`, piksel = `vfn#8()`;
(2) penyusun PNG (`vtable 0x1a11a10`, slot 2 = `0x5e8210`) menyusun **seluruh PNG di memori**; (3) hanya bila **tidak dibatalkan**: `TVPCreateStream(nama, WRITE)` lalu satu `Write(buffer, ukuran)`. Dibatalkan → berkas **tidak dibuat**.

## Format keluaran (`png_encode`)
- Tanda tangan PNG, lalu **IHDR** `0x5e843c`: lebar, tinggi (BE 32-bit), kedalaman **8**, tipe warna **6 (RGBA)**, kompresi 0, filter 0, interlace **0**.
- Chunk metadata (`0x5e87a0`, urutan **pHYs → oFFs → vpAg**), hanya bila `opsi` ada; satu chunk ditulis bila **setidaknya salah satu** anggota nilainya ada (`PropGet` dengan `TJS_MEMBERMUSTEXIST` berhasil); nilai yang tidak ada = 0:
  | Chunk | Anggota kamus | Isi (BE) | Byte satuan |
  |---|---|---|---|
  | `pHYs` | `reso_x`, `reso_y` | x, y (32-bit) | 1 bila `reso_unit` **persis** `"meter"`, selain itu 0 |
  | `oFFs` | `offs_x`, `offs_y` | x, y (**bertanda**) | 1 bila `offs_unit == "micrometer"`, selain itu 0 |
  | `vpAg` | `vpag_w`, `vpag_h` | lebar, tinggi | 1 bila `vpag_unit == "micrometer"`, selain itu 0 |
  Nilai integer lewat `0x533228(kamus, kunci, bawaan 0)`; pembanding string `0x5e9504` (anggota tak ada → 0).
- `comp_lv` (integer, bawaan **-1**) = level `deflateInit` (zlib; -1 = bawaan). Level tak sah (mis. 42) → `deflateInit` gagal → melempar **`deflate initialize`**.
- **IDAT** (`0x5e8fc4` + zlib `0x5e9770`): per baris 1 byte filter **0 (None)** + `lebar*4` byte piksel **disalin apa adanya** (tanpa penukaran R/B); aliran zlib (`deflateInit`, bukan raw); data diumpan **4096 byte** per putaran
  (`Z_NO_FLUSH`, putaran terakhir `Z_FINISH`); buffer keluaran tumbuh per 256 KB. Lalu **IEND** (kosong). CRC32 tiap chunk lewat `crc32` zlib (`0x5e91c4`).
- **Kemajuan:** setelah tiap putaran, bila callback ada: `persen = (diproses*100)/total` (0 bila total 0); callback mengembalikan `true` → `deflateEnd`, hasil "dibatalkan" (tanpa IDAT/IEND). Setelah `Z_STREAM_END` callback dipanggil sekali lagi dengan **100** (jadi 100 dapat terkirim dua kali; lapis progres tugas membuangnya karena sama).
- Byte piksel: lihat bagian "Urutan kanal" di `layerExSave_B_common.md` (memori R,G,B,A ⇒ PNG benar).
- Perbedaan byte dengan biner mungkin ada bila versi zlib berbeda (biner: string versi `"1.2.3"`); piksel hasil dekode identik.

Kode: `png_encode` + `PngOptions` (`layerExSaveBCore.hpp`), `ReadPngOptions`, `SavePngFile`, `SaveLayerImagePng` (`layerExSave.cpp`). Uji inti: struktur chunk + CRC + isi IDAT terdekompresi (66 pemeriksaan, 7 mutasi: 6 terdeteksi + 1 setara), PIL membaca keluaran sebagai RGBA; TJS: pulang-pergi.
ASUMSI: urutan chunk offs/vpag setelah pHYs dibaca dari urutan string di `0x5e87a0`; perbandingan satuan memakai konversi string (anggota non-string tidak teruji).
