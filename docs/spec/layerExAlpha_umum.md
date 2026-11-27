# layerExAlpha.dll — hal umum kelompok Alpha-1 (14 method)

Sumber: disassembly `libgame.so` 1.3.9 arm64 (`recon_alpha1.txt`, fungsi `0x5d4c04`–`0x5d6e9c`), string biner, dan **emulasi instruksi**
(`script/k2emu.py` + `script/k2emu_alpha1.py`). Kode: `src/plugins/layerExAlpha.cpp` + `layerExAlphaCore.hpp`. Uji: `tests/plugins/layerExAlpha/`.

## Nama modul & registrasi (terverifikasi dari string biner)
- `layerExAlpha.dll` = string UTF-16 di `0x14c9f5c`; `layerExAlpha` (tanpa `.dll`, nama kelas registrasi — arti = ASUMSI) di `0x14c9f7e`.
- 14 nama method **berurutan rapat** `0x14c9f98` (`turnAlpha`) … `0x14ca0f8` (`ExMaskRect`); string berikutnya `LightForAlpha` (`0x14ca110`) = awal S7.
- Semua method didaftarkan oleh **satu** fungsi registrasi `0x5d43b0` (gaya ncbind: banyak method per fungsi).
- Tiap method adalah **anggota kelas turunan `layerExBase_GL`** (argumen pertama = `this` kelas, bukan `tTJSVariant`), terpasang pada `Layer`.

## Tata letak objek `this` (cocok dengan `layerExBase_GL` di `src/plugins/layerExBase.hpp`)
| Offset | Anggota | Dipakai |
|---|---|---|
| `+0x00` | vptr: slot 0 = `reset()`, slot 1 = **`redraw()`** | tiap method memanggil slot 1 di akhir (kecuali jalur tanpa redraw) |
| `+0x08` | `_this` (`tTJSNI_Layer*`) | semua |
| `+0x10` / `+0x14` | `_width` / `_height` | loop piksel; batas ExMaskRect |
| `+0x18` / `+0x20` | `_buffer` / `_pitch` (int32 bertanda) | turnAlpha, turnAlpha2, ExMask* (cache `reset()`) |
| `+0x24 … +0x30` | klip kiri, atas, lebar, tinggi | jalur GPU (`rect = kiri,atas,kiri+lebar,atas+tinggi`) |

## Kamus helper (alamat di binary → fungsi di fork) — tingkat bukti
| Alamat | Fungsi | Bukti |
|---|---|---|
| `0x848bdc` | `TVPIsSoftwareRenderManager()` | **dari pemakaian** (hasilnya memilih CPU/GPU) — ASUMSI kuat, tidak didekode |
| `0x807c00` | `tTJSNI_Layer::GetMainImagePixelBufferForWrite()` | **didekode dari byte**: baca `[layer+0x118]` (MainImage), null → return 0; set byte `[layer+0x199]=1`; tail-call `0xa75340(image, 0)` |
| `0x807c20` | `GetMainImagePixelBufferPitch()` | **didekode dari byte**: `[layer+0x118]` null → 0; tail-call `0xa7546c(image)` |
| `0x80c848` | `GetMainImage()` (= `ApplyFont()` lalu `MainImage`) | dari pola pemakaian + `LayerIntf.h` di fork — ASUMSI, tidak didekode |
| `0x848834` | `TVPGetRenderManager()` | dari pemakaian |
| `0x848454` | `iTVPRenderManager::CompileRenderMethod(name, glsl, nTex, flags)` | dari argumen (nama, GLSL, 0/1, 0); ASUMSI |
| `0x848540` | `GetOrCompileRenderMethod(name, &hint, glsl, nTex, flags)` | dari argumen; ASUMSI |
| `[renderMethod+0x50]` | `SetBlendFuncSeparate(func, srcRGB, dstRGB, srcA, dstA)` | slot vtable ke-10; argumen = enum GL (`0x8006`, `0x305`, …) |
| `[renderManager+0xa0]` | `OperateRect(method, dst, ref, rect, textures)` | argumen cocok (method, tekstur, null, `tTVPRect*`, `{ptr,count}`) |
| `[image+0x10]` | `GetTextureForRender(isBlendTarget, rect*)` | dipanggil `(true, nullptr)` |
| `[image+0x28]` | `CopyRect(x, y, ref, refrect(by value), plane)` | `plane = 2` = `TVP_BB_COPY_MASK` |
| `[image+0x58]` | anggota tekstur gambar (`GetTexture()`) | dibaca langsung |
| `[dispatch+0xc8]` | `iTJSDispatch2::NativeInstanceSupport(TJS_NIS_GETINSTANCE, Layer::ClassID, &inst)` | dari argumen `(2, ClassID, &out)` |
| `0x953d0c` | `TVPThrowExceptionMessage(const tjs_char*)` | **didekode**: mengalokasikan pengecualian (`0x41f4f0`, 16 B) + string; pesan `"Not Layer"` |
| `0xa16ca4` | `TVPAddLog(const ttstr&)` | **didekode**: pembungkus `w1=0` ke `0xa164b8` (pasangan `0xa16cac` dengan `w1=1`) |

Catatan: nomor slot vtable `iTVPRenderManager` di 1.3.9 (`+0xa0`) berbeda dari fork (header fork punya 2 virtual tambahan); tidak masalah karena kode dikompilasi terhadap header fork.

## Format piksel
32bpp little-endian: byte0 = B, byte1 = G, byte2 = R, byte3 = A. Pitch = int32 bertanda, dalam byte.

## Aritmetika meniru AArch64
`add/sub/mul` 32-bit melingkar; `sdiv` oleh 0 menghasilkan **0** (tanpa trap), `INT_MIN / -1 = INT_MIN`. Pola `clamp` biner:
`x<0 → 0`, `x<256 → x`, selain itu `255` (`cmp #0x100; csinv; bic asr#31`).

## Deviasi dari binary (semua disengaja, dicatat di tiap spesifikasi)
1. **Keamanan memori:** ukuran loop dibatasi ke ukuran gambar; `MaskRect` memotong wilayah ke kedua gambar; `ExMaskRect` tidak membaca di luar sumber (dianggap alfa 0) dan tidak menulis di luar buffer (binary: menulis melewati ujung baris/buffer bila `sh+min(dy,0) < 0` atau lebar salin negatif). Untuk masukan dalam batas hasilnya **identik** (dibuktikan emulasi).
2. Method CPU mengambil ulang pointer/pitch buffer tiap panggilan (`reset()` di fork hanya mengisinya pada mode software).
3. `MaskRect` mode tak dikenal di jalur GPU: binary tak terdefinisi (pointer render method tak terinisialisasi); fork = perilaku CPU (log + redraw).
4. `AlphaDivide` meniru binary (alfa piksel pertama baris untuk seluruh baris); sakelar `LAYEREX_ALPHADIVIDE_PER_PIXEL=1` untuk per piksel.

## Tingkat bukti (jangan baca lebih dari ini)
- **Inti-host:** `tests/plugins/layerExAlpha/test_alpha1_s6.cpp` — nilai tangan + 159 vektor emas, ASan/UBSan. ✔️
- **Emulasi instruksi:** vektor emas = keluaran emulator AArch64-mini terhadap disassembly asli; kampanye acak 3 seed × ±6.000 kasus (±18.000 pemeriksaan per seed; ±0,5 % kasus dikecualikan karena perilaku tak terdefinisi di binary) cocok semua. Ini membuktikan **kode C++ = instruksi di recon**, di bawah hook helper di atas. Emulator buatan sendiri; bukan perangkat.
- **Sintaks:** `layerExAlpha.cpp` lolos `g++ -fsyntax-only -fpermissive` terhadap header inti fork (dengan kontrol negatif). ✔️
- **NDK / perangkat / diferensial APK:** ❌ belum (S2, S13).
- **Jalur GPU:** hanya jejak perintah dari emulasi (nama render method, GLSL, `SetBlendFuncSeparate`, `OperateRect`) yang dicocokkan; **belum dieksekusi** di GL.

## Cara mengulang (PC/Termux; `recon_*.txt` hasil disassembler **tidak** dicommit)
```
python3 script/k2emu_alpha1.py recon_alpha1.txt --lib libgame.so --golden tests/plugins/layerExAlpha/golden_cpu.txt --n 8   # vektor emas
python3 script/k2emu_alpha1.py recon_alpha1.txt --lib libgame.so --trace                                                  # jejak perintah GPU
python3 script/k2emu_alpha1.py recon_alpha1.txt --lib libgame.so --golden /tmp/besar.txt --n 300 --seed 11               # kampanye acak
g++ -std=c++14 -fsanitize=address,undefined -I src/plugins tests/plugins/layerExAlpha/test_alpha1_s6.cpp && ./a.out [berkas_emas]
```
Batas emulator: hanya subset instruksi layerEx; helper = hook (tabel di atas); GPU tidak dieksekusi. Kasus yang membuat binary membaca/menulis byte padding atau di luar buffer
dikecualikan dari vektor emas (perilaku tak terdefinisi) — angkanya dicetak saat membuat vektor.

## Pemetaan nama → kode (dikoreksi saat penggabungan S1–S10)

Sesi S6 memakai `layerex_impl_map.tsv` versi lama yang **bergeser satu slot** untuk `AlphaAdd2 … MaskImageRideRect3`; S7 mengoreksi TSV. Saat penggabungan pasangan itu **diperiksa ulang langsung dari biner** (dekode ADRP/ADD fungsi registrasi `0x5d43b0`, 1.832 B): tiap blok registrasi 28 B memuat nama (`x1`, string UTF-16) dan penunjuk kode (`x2`) berdampingan; blok bergaya `Function` (`AlphaAdd`, `ColorAlphaAdd`, `MaskImageRideRect4`/`8`) menaruh kode sebelum nama dan dibaca manual. Dekode otomatis cocok dengan TSV terkoreksi untuk 28 dari 31 nama; tiga sisanya adalah blok `Function` yang dibaca manual dan juga cocok. Bukti perilaku: kode `0x5d57cc` (`A = (R+G+B)/3`, RGB = 0) cocok dengan nama `BlackForAlpha`, bukan `AlphaBlend`.

Nama yang dipakai S6 → nama yang benar (kode piksel tidak berubah):

| Nama lama S6 | Nama benar | Kode |
|---|---|---|
| `AlphaAdd2` | **`AlphaAdd`** | `0x5d54a8` |
| `BlackForAlpha` | **`AlphaAdd2`** | `0x5d5708` |
| `AlphaBlend` | **`BlackForAlpha`** | `0x5d57cc` |
| `ExtraColorVectorEffectAdd` | **`ColorVectorEffectAdd`** | `0x5d5b8c` |
| `WhiteToBlueBlurAdd` | **`ExtraColorVectorEffectAdd`** | `0x5d5cdc` |
| `ExMaskRect` | **`MaskRect`** | `0x5d621c` |
| `ExMaskRect2` | **`ExMaskRect`** | `0x5d6784` |

Seluruh 31 nama (urutan registrasi) dan status:

| Nama | Kode | Status | Spesifikasi |
|---|---|---|---|
| `turnAlpha` `turnAlpha2` `turnAlpha3` `turnOutline` | `0x5d4c04` `0x5d4dc4` `0x5d4f80` `0x5d502c` | ✅ S6 | `layerExAlpha_<nama>.md` |
| `AlphaColorRect` `AlphaDivide` | `0x5d5204` `0x5d5400` | ✅ S6 | idem |
| `AlphaAdd` `AlphaAdd2` `BlackForAlpha` | `0x5d54a8` `0x5d5708` `0x5d57cc` | ✅ S6 | idem |
| **`AlphaBlend`** | `0x5d5894` (188 B) | ❌ belum | — |
| `ColorAlphaAdd` `ColorVectorEffectAdd` `ExtraColorVectorEffectAdd` | `0x5d5950` `0x5d5b8c` `0x5d5cdc` | ✅ S6 | idem |
| **`WhiteToBlueBlurAdd`** | `0x5d5f98` (344 B) | ❌ belum | — |
| `MaskRect` `ExMaskRect` | `0x5d621c` `0x5d6784` | ✅ S6 | idem |
| **`ExMaskRect2`** | `0x5d6ea0` (1.664 B; recon di `recon_alpha2.txt`) | ❌ belum | — |
| `LightForAlpha` … `MaskImageRideRect3` (7) | `0x5d764c` … `0x5d8b10` | ✅ S7 | `layerExAlpha_<nama>.md`, `layerExAlpha_S7_profil_xref.md` |
| `MaskImageRideRect4` `MaskImageRideRect5` | `0x5d8f94` `0x5d9cb8` | ✅ S8 | `…Rect4.md`, `…Rect5.md` |
| `MaskImageRideRect7` `MaskImageRideRect8` `AlphaBorderInnerRect` | `0x5da79c` `0x5db190` `0x5db974` | ✅ S9 | `layerExAlpha_common.md` + per method |
| `AlphaBorderImageCoverRect` `MaskImageEffectRect` | `0x5dbea0` `0x5dc9a4` | ✅ S10 | per method |

(`MaskImageRideRect6` tidak ada di biner.)

## Arsitektur kode setelah penggabungan

- **Satu kelas** `layerExAlpha : layerExBase_GL` di `src/plugins/layerExAlpha.cpp`, didaftarkan lewat `NCB_ATTACH_CLASS_WITH_HOOK(layerExAlpha, Layer)`; hook memanggil `reset()` pada **setiap** pemanggilan (ASUMSI — pola layerEx upstream; hook biner ada di registrasi `0x5d43b0`).
- **Satu Core** `layerExAlphaCore.hpp` (tanpa dependensi Kirikiri); pembantu numerik (`add32`, `sub32`, `madd32`, `sdiv32`, `clamp256`, `cvtzs`, `cvtzu`, `mulf`, `muld`) didefinisikan **sekali**. `cvtzs(double)` versi S8 dan S7 ekuivalen di seluruh domain.
- **Tujuan penulisan:** buffer tulis diambil segar (`GetMainImagePixelBufferForWrite()`) — `layerExBase_GL::reset()` fork hanya mengisi `_buffer` pada mode software. Ukuran tujuan = `_width/_height` (**rect layer**, `GetWidth()`), dibatasi ke ukuran gambar; layer yang lebih besar dari gambarnya dianggap ukuran tak cocok pada method yang memeriksa ukuran. ⚠️ **ASUMSI:** `reset()` di biner belum dibaca; S1 mencatat bahwa `shrinkCopy` di biner memakai ukuran gambar. Identik untuk layer normal.
- **Uji:** `tests/plugins/layerExAlpha/run_tests.sh` menjalankan `test_alpha1_s6.cpp` (+ `golden_cpu.txt`), `test_alpha2_s7s10.cpp`, `test_alpha3_s8.cpp`, `test_alpha4_s9.cpp` terhadap Core yang sama (ASan/UBSan).
