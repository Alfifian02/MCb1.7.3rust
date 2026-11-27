# Survei sumber publik plugin (langkah 3.6, sesi S14)

Dibuat: 2026-11-22 · Cakupan: **50 nama plugin** yang tersimpan sebagai string UTF-16 `*.dll` di `libgame.so` arm64 1.3.9
(52 string; `layerExImage`/`LayerExImage` dan `savestruct`/`saveStruct` hanya beda huruf besar-kecil).

## Tingkat bukti (baca ini dulu)

| Kode | Arti |
|---|---|
| **log** | Anda menjalankan `fetch_plugins.sh` dan clone berhasil (`fetch_s14.txt`): daftar berkas dan jumlah `.cpp` itu fakta. **Isi berkas tidak ikut terunggah**, jadi kompatibilitas kode belum diperiksa. |
| **web** | Halaman repo dibuka/dibaca di sesi ini (daftar berkas dan/atau readme). Isi kode belum dibaca. |
| **gist** | Hanya disebut di daftar komunitas *Kirikiroid2 dll list.csv* (uyjulian). **Belum dibuka satu per satu**; anggap petunjuk. |
| **—** | Tidak ada sumber publik yang ditemukan. |

Lisensi: semua readme yang terbaca (json, csvParser, layerExRaster, layerExBTOA, layerExImage, layerExSave, shrinkCopy, varfile, win32dialog, layerExDraw)
menulis "ikut lisensi Kirikiri". Proyek ini pribadi, jadi tidak dibahas lebih jauh. Pengecualian yang perlu diingat saja:
`layerExImage` memuat kode turunan CxImage (gaya zlib), `layerExSave` membawa LodePNG, `minizip`/`expat` membawa `licenses/texts/*.txt` sendiri.

## Koreksi atas langkah 3.4

3.4 menyimpulkan "layerEx tidak punya source publik" karena **`wamsoft/layerEx` hanya berisi `LayerExBase`**. Itu benar untuk repo tersebut,
tetapi tiap modul layerEx punya **repo sendiri**: `layerExBTOA`, `layerExImage`, `layerExSave`, `layerExRaster` (web), `shrinkCopy`, `layerExAreaAverage` (web/gist).
Yang benar-benar tanpa sumber publik hanya **`layerExAlpha`** (dan `layerExShimmer` hanya sebagai zip di situs lama, bukan GitHub).

## Tabel 50 modul

Nama = huruf besar-kecil **asli di biner** (relevan untuk 6.2). "Di fork" = sudah ada berkas di `src/plugins/`.

### A. Punya sumber publik yang bukti-nya log/web (15)

| Modul | Sumber | Bukti | Di fork? | Catatan untuk fork |
|---|---|---|---|---|
| `json.dll` | `wamsoft/json` | log + web | belum | 1 `.cpp` (`Main.cpp`) + `Writer.hpp`. Kandidat termudah. |
| `scriptsEx.dll` | `wamsoft/scriptsEx` | log | belum | 1 `.cpp` (`Main.cpp`) + `bitap_fuzzy.hpp`. |
| `windowEx.dll` | `wamsoft/windowEx` | log | belum | 1 `.cpp` (`main.cpp`), ikon uji. **ASUMSI:** memakai Win32 (HWND, multi-monitor) → tidak bisa dibangun apa adanya di Android. |
| `minizip.dll` | `wamsoft/minizip` | log | belum | 4 `.cpp` (`main.cpp`, `storage.cpp`, `ioapi.cpp`, `LicensesGen.cpp`), `.gitmodules` + `vcpkg.json` (minizip-ng). **Risiko:** `src/core/utils/minizip/ioapi.cpp` sudah ada di core fork → kemungkinan simbol ganda (ASUMSI, belum dicek isinya). |
| `expat.dll` | `wamsoft/expat` | log | belum | 2 `.cpp` (`Main.cpp`, `LicensesGen.cpp`), `XML.tjs`, `vcpkg.json`. libexpat dari vcpkg; di fork hanya `Android.mk` yang menyebut `expat_static` (build CMake: **belum terverifikasi**). `libgame.so` mengekspor `XML_ParserCreate*` → expat tertanam di biner asli. |
| `layerExBTOA.dll` | `wamsoft/layerExBTOA` | web | belum | 26 commit. Mencakup 6 method S3. |
| `layerExImage.dll` | `wamsoft/layerExImage` | web | belum | `LayerExImage.cpp/.h`, `Main.cpp`, `layerExBase.hpp`. Sebagian kode dari CxImage. |
| `LayerExSave.dll` | `wamsoft/layerExSave` | web | belum | LodePNG 20161127 di `./LodePNG/`; `Window.startSaveLayerImage` = PNG buatan sendiri. Mencakup S11–S12. |
| `layerExRaster.dll` | `wamsoft/layerExRaster` | web | **ya (rekonstruksi)** | `main.cpp`, `layerExBase.hpp`. Bisa jadi pembanding `layerExRasterCore.hpp`. |
| `shrinkCopy.dll` | `wtnbgo/shrinkCopy` | web | **ya (rekonstruksi)** | Penulis miahmie; "1/256 px" = cocok dengan `unit=256` di rekonstruksi. Dapat menutup ASUMSI `hrow_box` (S1) **bila** versi GitHub = versi 1.3.9 (belum tentu: biner 2018, repo 2026). |
| `csvParser.dll` | `wamsoft/csvParser` | web | ya (bawaan) | Pembanding saja. |
| `varfile.dll` | `wtnbgo/varfile` | web | ya (bawaan) | Pembanding saja. |
| `win32dialog.dll` | `wamsoft/win32dialog` | web | ya (stub) | Readme: "belum selesai". Stub fork sudah cukup. |
| `extrans.dll` | `wamsoft/extrans` | web | belum | Penulis W.Dee; 7 transisi; MMX hanya x86, arm64 memakai versi C. |
| `LayerExDraw.dll` | `wtnbgo/layerExDraw` | web | belum | GDI+ → Windows-only. **Alternatif:** `wamsoft/layerExVector` (ThorVG, subset layerExDraw, nama kelas tetap `GdiPlus`) → relevan untuk S18. |

### B. Hanya disebut daftar komunitas (gist) (21)

| Modul | Petunjuk sumber | Di fork? |
|---|---|---|
| `layerExAreaAverage.dll` | `wtnbgo/layerExAreaAverage` | ya (alias TJS) |
| `fstat.dll` | `wtnbgo/fstat` | belum |
| `KAGParserEx.dll` | `wtnbgo/KAGParserEx` (juga terdaftar sebagai plugin statis di `wamsoft/krkrz_web`) | belum |
| `KAGParserExb.dll` | `sakano/krkr_archives` (`kagex_plugin/KAGParserExb`) | belum |
| `ExtKAGParser.dll` | zip di keepcreating.g2.xrea.com | belum |
| `saveStruct.dll` / `savestruct.dll` | `wtnbgo/saveStruct` | ya |
| `addFont.dll` | `wtnbgo/addFont` | ya |
| `getSample.dll` | `wtnbgo/getSample` | ya |
| `dirlist.dll` | `krkrz/krkrz` cabang `last_hodgepodge_repository`, `src/plugins/win32/dirlist` | ya |
| `layerExMovie.dll` | idem, `.../layerExMovie` | ya |
| `perspective.dll` | idem, `.../layerExPerspective` | ya |
| `fftgraph.dll` | `krkrz/fftgraph` | ya (stub) |
| `wutcwf.dll` | `krkrz/SamplePlugin/wutcwf` | ya |
| `sqlite3.dll` | `krkrz/krkrz` cabang `last_hodgepodge_repository`, `src/plugins/win32/sqlite3` | belum. **`wamsoft/sqlite3` tidak ada** (terbukti di log). |
| `layerExShimmer.dll` | zip di keepcreating.g2.xrea.com (bukan GitHub; ketersediaan belum dicek) | belum |
| `extNagano.dll` | arsip web.archive.org (ymtkyk.sakura.ne.jp) — komunitas: salinannya sulit ditemukan | belum |
| `KaichoTrans.dll` | zip di keepcreating.g2.xrea.com | belum |
| `gfxEffect.dll` | zip di kaede-software.com | belum |
| `AlphaMovie.dll` | zip di kaede-software.com — **gratis tetapi bukan open source** | belum |
| `DrawDeviceD3D.dll` / `DrawDeviceD3DZ.dll` | `krkrz/krkr2` / `krkrz/krkrz` — Direct3D, Windows-only | belum (cukup stub) |

### C. Tidak ada sumber publik yang ditemukan (14)

| Modul | Catatan |
|---|---|
| `layerExAlpha.dll` | 28 method (S6–S10). **Satu-satunya grup layerEx yang tetap harus didekompilasi.** |
| `PSBFile.dll`, `TextRender.dll` | Daftar komunitas kosong, tetapi `wamsoft/krkrz_web` menyebut `psbfile`, `textrender` (juga `extrans`, `extNagano`, `KAGParserEx`) sebagai plugin statis → **kemungkinan ada repo wamsoft; belum ditemukan**. |
| `emoteplayer.dll`, `motionplayer.dll` | E-mote, proprietary. |
| `fPackinOne.dll` | Tanpa sumber (komunitas juga bertanya). Di biner dekat string `psbfile`. |
| `lzfs.dll` | Komunitas: "No source found". Biner: pesan `lzfs: Decompress fail, data may be damaged.` + kelas `lzfs`. |
| `util_generic.dll`, `util_graph.dll`, `util_system.dll` | Tidak ada sumber. Dari string biner (**pengelompokan modul ASUMSI**, posisi string nama modul terhadap string method belum terbukti): `RandomVariate.generate`, `Debug.hexDump/getClassInstanceInfo`, `Array.shuffle/map/grep/unique/aggregate`, `Dictionary` (keys/values/dump) · `tpgt*` (gradien), `tpft*` (Prewitt/Sobel/Robert/Mosaic/Median), `tplc*`, `tpct*` (kurva) · `tpctCPUIs*` (deteksi CPU). |
| `multiimage.dll` | Tidak ada sumber; namanya di biner huruf kecil. |
| `xp3filter.dll` | Sudah ada di fork, asal-usul tidak teridentifikasi (lihat `audit_plugin_bawaan.md`). |
| `getabout.dll`, `kirikiroid2.dll` | Khusus aplikasi; `getabout` sudah stub. |

Hitungan: 15 + 21 + 14 = 50.

## Yang belum bisa dibuktikan di sesi ini (berkas kurang)

1. **Isi clone** (`json`, `scriptsEx`, `minizip`, `expat`, `windowEx`, …) tidak diunggah, hanya log. Cek sintaks terhadap header fork, cek bentrok simbol `ioapi`, dan keputusan "bangun / tidak" per plugin **belum dikerjakan** → perlu unggahan zip clone tanpa `.git` (rencana 3.7).
2. `readme.txt` windowEx/scriptsEx/minizip/expat tidak terbaca; "ikut lisensi Kirikiri" untuk keempatnya adalah **ASUMSI** dari pola repo lain.
3. Versi GitHub (2026) belum tentu sama dengan kode di biner 1.3.9 (2018). Uji diferensial (7.3) tetap berlaku.
