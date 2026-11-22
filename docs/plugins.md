# Status plugin

Diperbarui: 2026-11-22 (penggabungan S1–S10) · Target: fork Kirikiroid2 1.4.0 beta · Pembanding: `libgame.so` 1.3.9 (arm64).
Rencana kerja: [`rencana_decomp_recomp.md`](rencana_decomp_recomp.md) · Pembagian sesi & inventaris yang belum: [`sesi_kerja.md`](sesi_kerja.md) · Spesifikasi perilaku: [`spec/`](spec/) · Kamus helper binary→fork: [`spec/helpers_map.md`](spec/helpers_map.md).

## Cara membaca

**Sumber**

| Label | Arti |
|---|---|
| **bawaan fork** | Kode sumber sudah ada di fork upstream (bukan hasil rekonstruksi dari binary). *Asal-usul/lisensi tiap berkas belum diaudit satu per satu.* |
| **stub** | Pengganti ringan, sengaja tidak mengimplementasikan fungsi aslinya. |
| **rekonstruksi** | Ditulis ulang dari perilaku `libgame.so` 1.3.9 (spesifikasi di `docs/spec/`); bukan salinan pseudo-code. |
| **belum ada** | Tidak ada di fork. Source publik tidak tersedia, atau belum dikerjakan. |

**Uji** — bertingkat; tiap tingkat hanya berlaku bila disebut.

| Tingkat | Arti |
|---|---|
| **inti-host** | Algoritma piksel (header tanpa dependensi) lolos uji di PC dengan ASan/UBSan. |
| **emulasi-binary** | Instruksi asli satu fungsi `libgame.so` (dari recon) dieksekusi di emulator AArch64 mini dan hasilnya dibandingkan dengan model/kode inti. Bukti perilaku **fungsi itu saja**; bukan uji perangkat dan bukan end-to-end. Skrip ada di `tests/plugins/<modul>/`, recon-nya tidak di-commit. |
| **emu-biner** | Keluaran inti dibandingkan dengan hasil *menjalankan fungsi asli* `libgame.so` 1.3.9 di emulator AArch64 (Unicorn, objek Layer/Texture tiruan; `script/emu/harness.py`, S3). **Bukan perangkat nyata**, hanya jalur CPU. |
| **emulasi-biner** | Fungsi **asli** `libgame.so` 1.3.9 dijalankan di emulator (unicorn, `script/k2emu_unicorn.py`, S4) dan keluarannya dibandingkan **byte-per-byte** dengan inti. Terkuat di bawah perangkat, tetapi hanya untuk jalur CPU; GPU, sistem, dan waktu tidak tercakup. |
| **sintaks** | `g++ -fsyntax-only` terhadap header inti fork (host x86). Sejak S2 lolos di `-std=c++11` dan `-std=c++14` **tanpa** `-fpermissive`. **Bukan** build NDK/clang dan bukan link. |
| **objek-host** (S2) | `g++ -c` ke berkas objek x86 berhasil; tidak ada simbol kuat ganda antar objek plugin + `LayerExBase.o`. **Bukan** link penuh. |
| **baca-source** (S2) | Perilaku registrasi dibaca dari source fork (bukan dijalankan): `docs/spec/plugin_registrasi.md`. |
| **NDK** | Terkompilasi dan terlink untuk Android. |
| **perangkat** | Dijalankan lewat TJS di perangkat; **diff terhadap APK 1.3.9** = uji diferensial (7.3). |
| **belum teruji** | Tidak ada uji di repo ini. Bukan berarti rusak — berarti tidak terbukti. |

> Saat ini **tidak ada plugin yang sudah lolos tingkat NDK atau perangkat** di dokumen ini; angka itu butuh build (rencana: Langkah berikutnya #2). Setelah penggabungan S1–S10, **semua uji inti host lolos pada pohon gabungan** dan 9 objek layerEx dikompilasi tanpa simbol kuat ganda (`layerExMovie` tidak dapat dikompilasi di sandbox: header `ffmpeg` tidak ada).
> Skrip uji perangkat sudah ada di `tests/tjs/` tetapi **belum dijalankan**.

## Pemuatan modul (fork)
Modul internal baru **terdaftar saat `Plugins.link("nama.dll")`** (bukan di startup; hanya `xp3filter.dll` dimuat otomatis). Nama dicocokkan **tanpa peduli huruf besar-kecil** dan tanpa path; modul yang belum ada di fork diabaikan tanpa error. Detail: [`spec/plugin_registrasi.md`](spec/plugin_registrasi.md).
Build: jalur CMake (arm64-v8a) menautkan `krkr2plugin` dengan `--whole-archive`; `armeabi-v7a` **belum didukung** konfigurasi build saat ini.

## Plugin di fork (21 modul terdaftar; 7 di antaranya rekonstruksi dari binary)

### Rekonstruksi dari binary 1.3.9

| Modul (`Plugins.link`) | Berkas | Method | Sumber | Uji | Catatan |
|---|---|---|---|---|---|
| `layerExAreaAverage.dll` | `layerExAreaAverage.cpp` | `stretchCopyAA` | rekonstruksi (alias TJS) | sintaks, objek-host, baca-source | Di binary hanya satu ekspresi `Layer.stretchCopyAA = Layer.stretchCopy;`. Tanpa inti piksel, jadi tidak ada uji inti. **Di fork berjalan saat `Plugins.link("layerExAreaAverage.dll")`**, bukan saat startup (koreksi S2). Waktu di 1.3.9 belum dicocokkan (skrip `t_stretchCopyAA.tjs` siap). |
| `layerExRaster.dll` | `layerExRaster.cpp` + `layerExRasterCore.hpp` | `copyRaster` | rekonstruksi | inti-host, sintaks, objek-host | Arah geser CPU vs GPU tampak berlawanan di binary asli (cek 7.3). Nama parameter perkiraan. Uji inti **ditulis ulang** (aslinya tertimpa). |
| `shrinkCopy.dll` | `layerExShrink.cpp` + `layerExShrinkCore.hpp` | `shrinkCopy` | rekonstruksi | inti-host, sintaks, objek-host | Jalur CPU area-average bobot tetap; jalur GPU render method `"Copy"`. Tidak memanggil `Update()` (meniru binary). |
| `shrinkCopy.dll` | `layerExShrink.cpp` + `layerExShrinkFastCore.hpp` | `shrinkCopyFast` | rekonstruksi (jalur CPU **tuntas**) | inti-host, emulasi-binary (`0x6023ec`), sintaks, objek-host | Jalur CPU (vertikal `0x601d90` + horizontal `0x6023ec`) seluruhnya dari binary; `hrow` diverifikasi dengan **mengeksekusi disassembly aslinya** di emulator (1.605 kasus, 0 beda) — bukan uji perangkat, jadi identik-piksel dengan 1.3.9 **belum terbukti** (7.3). ⚠️ Jalur GPU menyimpang dari binary secara sadar (tekstur `src`, bukan `this`; keputusan di S13). |
| `layerExBTOA.dll` | `layerExBTOA.cpp` + `layerExBTOACore.hpp` | `copyRightBlueToLeftAlpha`, `copyBottomBlueToTopAlpha`, `fillAlpha`, `copyAlphaToProvince`, `clipAlphaRect`, `fillByProvince` | rekonstruksi | inti-host, **emu-biner** (CPU), sintaks, objek-host | Spesifikasi: [`spec/layerExBTOA_module.md`](spec/layerExBTOA_module.md). ⚠️ GPU (`CopyAlphaFromBlue`, `FillByProvince`) dan `fillAlpha` tidak tercakup uji; GPU `FillByProvince` kemungkinan terbalik di biner (saklar `LAYEREX_BTOA_FILLBYPROVINCE_GPU_STD_BLEND`). ⚠️ 7 deviasi sadar (6 crash di biner → aman di fork; flag `+0x261` tidak ditiru, dampak ASUMSI nol). Nama parameter perkiraan. |
| `layerExImage.dll` | `layerExImage.cpp` + `layerExImageCore.hpp` | `light`, `colorize`, `modulate`, `noise`, `generateWhiteNoise`, `gaussianBlur` | rekonstruksi | **emulasi-biner (jalur CPU)**, inti-host, sintaks, objek-host | Jalur CPU identik dengan fungsi asli di emulator (0 selisih; `tests/plugins/layerExImage/golden.inc`). ⚠️ Jalur GPU (shader asli diformat ulang) **belum diuji**. ⚠️ Perilaku asli yang ditiru: `colorize`/`modulate` memperlakukan byte R/B berbeda; CPU≠GPU pada beberapa parameter; `gaussianBlur` punya kuirk bobot tengah dan anomali bila kernel > baris; non-software = box blur. ⚠️ `noise`/`generateWhiteNoise` acak di binary (uji memakai seed suntikan). Deviasi sadar: `gaussianBlur` menolak kernel > 16.385 tap. Belum NDK/perangkat. |
| `layerExShimmer.dll` | `layerExShimmer.cpp` + `layerExShimmerCore.hpp` | `shimmer`, `shimmerBuildMap` | rekonstruksi, **sebagian** (S5) | inti-host, sintaks, objek-host | Spesifikasi `spec/layerExShimmer_*.md`. ASUMSI: interior `shimmer` (worker `0x5f0494/0x5f0630` belum dibaca), `shimmerBuildMap` argumen ke-4 → `TJS_E_NOTIMPL`; tiga keganjilan tepi biner dipertahankan. Tinggal 4.9e-2 (dump helper). Belum NDK/perangkat. |
| `layerExAlpha.dll` | `layerExAlpha.cpp` + `layerExAlphaCore.hpp` | **28 dari 31 nama.** Alpha-1 (S6): `turnAlpha`, `turnAlpha2`, `turnAlpha3`, `turnOutline`, `AlphaColorRect`, `AlphaDivide`, `AlphaAdd`, `AlphaAdd2`, `BlackForAlpha`, `ColorAlphaAdd`, `ColorVectorEffectAdd`, `ExtraColorVectorEffectAdd`, `MaskRect`, `ExMaskRect`. Alpha-2 (S7): `LightForAlpha`, `LuminanceForAlpha`, `LevelMaskRect`, `LevelMaskRect2`, `MaskImageRideRect`, `MaskImageRideRect2`, `MaskImageRideRect3`. Alpha-3 (S8): `MaskImageRideRect4`, `MaskImageRideRect5`. Alpha-4 (S9): `MaskImageRideRect7`, `MaskImageRideRect8`, `AlphaBorderInnerRect`. Alpha-5 (S10): `MaskImageEffectRect`, `AlphaBorderImageCoverRect` | rekonstruksi (satu kelas `layerExAlpha : layerExBase_GL`, satu hook, satu `Core`) | inti-host (4 uji ASan/UBSan, `tests/plugins/layerExAlpha/run_tests.sh`), **emulasi-binary** (Alpha-1: 159 vektor emas; Alpha-3: fungsi utuh, 14 hash golden), sintaks, objek-host | Spesifikasi [`spec/layerExAlpha_umum.md`](spec/layerExAlpha_umum.md), `spec/layerExAlpha_common.md`, 1 berkas per method. ⚠️ **Belum ditulis:** `AlphaBlend` (`0x5d5894`), `WhiteToBlueBlurAdd` (`0x5d5f98`), `ExMaskRect2` (`0x5d6ea0`). ⚠️ **Nama S6 dikoreksi saat penggabungan** (peta lama bergeser satu slot; pasangan NAME→CODE dibaca langsung dari registrasi `0x5d43b0`). ⚠️ ASUMSI: ukuran tujuan = rect layer (`reset()` fork) dibatasi ke gambar; `AlphaAdd`/`ColorAlphaAdd` terdaftar via objek `Function`; nama parameter perkiraan. ⚠️ GPU (hanya `turnAlpha`, `turnAlpha2`, `MaskRect` mode 1–3) tidak dijalankan. Deviasi sadar vs biner tercatat di spesifikasi (mis. `AlphaDivide`, anomali kolom `MaskImageRideRect5`). Belum NDK/perangkat. |

### Bawaan fork

| Modul | Berkas | Sumber | Uji | Catatan |
|---|---|---|---|---|
| `addFont.dll` | `addFont.cpp` | bawaan fork | belum teruji | `System.addFont` via `TVPEnumFontsProc`. |
| `csvParser.dll` | `csvParser.cpp` | bawaan fork | belum teruji | |
| `dirlist.dll` | `dirlist.cpp` | bawaan fork | belum teruji | |
| `getSample.dll` | `getSample.cpp` | bawaan fork | belum teruji | |
| `layerExMovie.dll` | `layerExMovie.cpp` | bawaan fork | belum teruji | |
| `perspective.dll` | `layerExPerspective.cpp` | bawaan fork | belum teruji | Memuat pemberitahuan hak cipta AGG di berkasnya. Nama modul `perspective.dll`, **bukan** `layerExPerspective.dll`. |
| `saveStruct.dll` | `saveStruct.cpp` | bawaan fork | belum teruji | |
| `varfile.dll` | `varfile.cpp` | bawaan fork | belum teruji | |
| `wutcwf.dll` | `wutcwf.cpp` | bawaan fork | belum teruji | |
| `xp3filter.dll` | `xp3filter.cpp` + `xp3filter.h` | bawaan fork | belum teruji | Satu-satunya modul yang dimuat eksplisit di `TVPLoadInternalPlugins`. |

### Stub

| Modul | Berkas | Sumber | Uji | Catatan |
|---|---|---|---|---|
| `getabout.dll` | `getabout.cpp` | stub ringan | belum teruji | Mengalias `System.getAboutString` ke `TVPGetAboutString`. |
| `fftgraph.dll` | `fftgraph.cpp` | stub | belum teruji | Hanya mendefinisikan `drawFFTGraph(){}` kosong. |
| `win32dialog.dll` | `win32dialog.cpp` | stub | belum teruji | `WIN32Dialog.messageBox` dialihkan ke `System.inform`. |
| `motionplayer.dll` | `motionplayer_stub.cpp` | stub (**pencatat log**) | belum teruji | Tidak merender. Mendaftarkan kelas `Motion.*` dan mencatat panggilan untuk mencari tahu apa yang dipakai game. Daftar member diambil dari string biner dan bisa belum lengkap. |


## layerEx yang belum selesai (rekonstruksi dari binary — sedang berjalan)

Alamat implementasi per method: [`script/layerex_impl_map.tsv`](../script/layerex_impl_map.tsv) (dibuat `script/k2map.py`; heuristik, uji-diri 3/3, belum diverifikasi di Ghidra).
Source publik `wamsoft/layerEx` **tidak** memuatnya (hanya `LayerExBase`), jadi semuanya harus **rekonstruksi**.

| Kelompok | Method (dengan impl) | Kode biner | Status | Langkah rencana |
|---|---:|---:|---|---|
| ~~`layerExBTOA`~~ | 6 | 7.316 B | **ditulis (S3)** — lihat tabel rekonstruksi di atas; belum NDK/perangkat | 4.9c ✔ → 5.4 ✔ |
| ~~`layerExImage`~~ | 6 | 8.228 B | **ditulis (S4)** — lihat tabel di atas; belum NDK/perangkat | 4.9d ✔ → 5.5 ✔ |
| `layerExShimmer` | 2 | 5.264 B (+ fungsi lokal ≈ 1,7 KB di luar hitungan) | **rekonstruksi sebagian** (S5): inti-host lolos uji; interior `shimmer` ASUMSI | 4.9e-2 → 5.7 |
| `layerExAlpha` | 31 nama (28 ditulis) | ≈ 29,4 KB (28 ditulis) | **sebagian**: 28 dari 31 ditulis (S6–S10); sisa `AlphaBlend`, `WhiteToBlueBlurAdd`, `ExMaskRect2` | 4.9f → 5.6 (sesi "tutup Alpha") |
| `layerExSave` | 14 (+ `WindowSaveImage` dan callback `onSaveLayerImage*` di luar peta) | 7.584 B | belum ada — peta alamat siap; NEON/thread, dikerjakan terakhir | 4.9g → 5.8 |
| `multiImage`, `layerExDraw` | — | — | belum ada; `layerExDraw` memakai GDI+ (tidak bisa dibangun langsung) | tertunda |

## Backlog lain (36 nama di binary, belum ada di fork)

Window/script: `windowEx`, `scriptsEx`, `kagparserex`, `kagparserexb`, `extkagparser`, `textrender` ·
Util/data: `json`, `sqlite3`, `minizip`, `expat`, `lzfs`, `fstat`, `util_generic`, `util_graph`, `util_system`, `fpackinone` ·
Game/media: `emoteplayer`, `psbfile`, `kaichotrans`, `extnagano`, `extrans`, `gfxeffect`, `alphamovie` ·
Stub Windows: `drawdeviced3d`, `drawdeviced3dz`, `kirikiroid2` ·
layerEx (di atas): `layerExAlpha` (28/31 ditulis), ~~`BTOA`~~ (ditulis), ~~`Image`~~ (ditulis), `Save`, `Shimmer` (sebagian), `multiImage`, `layerExDraw`.

Status seluruhnya **belum ada** (kecuali layerEx yang sudah ditulis di atas). Mengecek apakah ada source publik yang lisensinya cocok (`windowEx`, `scriptsEx`, `json`, `sqlite3`, `minizip`, `expat`) adalah langkah 3.6 yang belum dikerjakan.
