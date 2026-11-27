# layerExSave bagian A — konvensi bersama (sesi S11)

- Modul: **`LayerExSave.dll`** (string UTF-16 `@0x14caf06`; urutan di biner: `LayerExSave.dll`, `WindowSaveImage`, `startSaveLayerImage`, … `getCropRect` `@0x14cafbe`, …).
  ⚠️ **Huruf besar `L`**, beda dengan nama berkas `layerExSave` di rencana. Pencarian modul fork tidak peka huruf (`plugin_registrasi.md`), jadi aman.
- Bukti: recon `recon_saveA.txt` (depth 0) **dieksekusi di emulator** (`tests/plugins/layerExSave/emu_saveA.py`, 439 pemeriksaan, 0 beda) +
  pembantu `0x5ead0c` / `0x5eaea4` dibaca dari **byte biner** dengan dekoder mini (bukan dari recon). Tingkat: emulasi-binary + inti-host.
- Method terdaftar pada kelas `Layer` (`PropSet` pada `Layer`, seperti plugin lain). Delapan implementasi berurutan di `0x5eab30`–`0x5ec6e4` (5.156 B).
- Tak satu pun memanggil `Update()` / menandai area kotor. Layer tujuan yang ditulis harus di-`update`/dirender ulang oleh skrip.

## Pembantu biner `0x5ead0c(objek, &lebar, &tinggi, untukTulis)` → larik penunjuk baris (dibaca dari byte)
1. `objek->NativeInstanceSupport(2 /*GETINSTANCE*/, LayerClassID, &ni)`; gagal → melempar pesan (ASUMSI: `TVPSpecifyLayer`). `objek == null` → crash (tidak diperiksa).
2. `0x80c848(ni)` (sinkronisasi gambar; peran belum dipastikan — S1) lalu `ni->MainImage` (`[ni+0x118]`).
3. Bitmap data: **tulis** → `MainImage->vfn#2(0,0)` (ASUMSI: ambil bitmap yang boleh ditulis / independ); **baca** → `[MainImage+0x58]`.
4. `*lebar = bitmap[+0xC]`, `*tinggi = bitmap[+0x10]` → **ukuran gambar utama** (bukan rect layer; cocok `GetImageWidth/Height`).
5. `new tjs_uint8*[tinggi]`; tulis: `baris[0] = bitmap->vfn#9(0)`, `pitch = bitmap->vfn#10()`, `baris[i] = baris[i-1] + pitch`;
   baca: `baris[i] = bitmap->vfn#7(i)` (GetScanLine). Pemanggil `delete[]` larik itu.
- Padanan fork: `GetImageWidth()/GetImageHeight()` + `GetMainImagePixelBuffer()` / `…ForWrite()` + `…Pitch()` → `baris[y] = dasar + y*pitch` (`MakeRows` di `layerExSave.cpp`).
  `ForWrite` menyetel `ImageModified` di fork; apakah vfn#2/#9 biner melakukan hal serupa **belum diketahui** (S13).
- Piksel = 32-bit B,G,R,A (alfa byte ke-3). Perulangan biner selalu **kolom luar, baris dalam** untuk pemindai rect, **baris luar, kolom dalam** untuk pengisi.

## Konversi parameter (`tTJSVariant` → integer) — sama di semua method
`void`→0 · `string`→`ToInteger` · `integer` apa adanya · `real`→dipotong ke nol · `object`/`octet`→ galat konversi (di emulator: `0xa0cd8c(var, 4)`).
Nilai 64-bit dipotong ke **32-bit** saat dipakai (`w`-register). Parameter layer harus berjenis **object** (jika bukan: galat konversi ke object).
Jumlah parameter kurang → `TJS_E_BADPARAMCOUNT` (`-1004`) **kecuali** dicatat lain di tiap method.

## Hasil `Dictionary` `{x,y,w,h}` (`0x5eaea4`, dibaca dari byte)
Kunci string `"x"` (`0x1507f7e`), `"y"` (`0x1507f98`), `"w"` (`0x14caec6`), `"h"` (`0x14caed4`); nilai bertipe **integer**; urutan
register `x1→x, x2→y, x3→w, x4→h` (terverifikasi dari prolog). Hasil `void` bila tak ada piksel yang memenuhi.

## Pesan galat (string UTF-16, `0x953d0c` = lempar pesan)
| Alamat | Pesan | Dipakai |
|---|---|---|
| `0x14cb00e` | `Different layer size.` | `getDiffRect`, `getDiffPixel` |
| `0x14cb068` | `Invalid level count.` | `oozeColor` |
| `0x14cb0c2` | `invalid layer range` | `isBlank` |

## ⚠️ Temuan: kebocoran memori di biner (tidak ditiru)
Larik baris dibebaskan hanya pada jalur "ada hasil". Bocor: `getCropRect`/`getCropRectZero` (tak ada piksel aktif → 1 larik), `getDiffRect` (tak ada beda → 2 larik),
`isBlank` (hasil **bukan** blank → 1 larik; juga saat `invalid layer range`). Terbukti di emulator (`leaks`). Fork memakai `std::vector` → tidak bocor.

## Koreksi/peringatan (S12)
- Kalimat "Piksel = 32-bit B,G,R,A" di atas adalah asumsi S11. Temuan BTOA dan log 2 (`tests/tjs/README.md`, aturan 8) menunjukkan memori layer **R,G,B,A** (biru = `byte[2]`). Bila benar, "byte biru"
  (`isBlank`, `copyBlueToAlpha`) adalah **byte 0 = kanal merah**; perilaku kode tidak berubah (semuanya memakai byte 0), hanya penamaan. `t_layerExSaveA.tjs` mengukur kanal yang sebenarnya.
- Bagian B (penyimpanan gambar, thread, PNG, TLG5): `layerExSave_B_common.md` dan satu berkas per method.

