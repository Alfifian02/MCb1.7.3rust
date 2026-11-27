# Status plugin

Diperbarui: 2026-11-25 (S12, S14 dan S17 digabung ke S11) · Target: fork Kirikiroid2 1.4.0 beta · Pembanding: `libgame.so` 1.3.9 (arm64).
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
| `LayerExSave.dll` | `layerExSave.cpp` + `layerExSaveCore.hpp` + `layerExSaveBCore.hpp` | **Layer:** `getCropRect`, `getCropRectZero`, `getDiffRect`, `getDiffPixel`, `oozeColor`, `copyBlueToAlpha`, `isBlank`, `clearAlpha` (A, S11), `saveLayerImageTlg5`, `saveLayerImagePng`, `saveLayerImagePngOctet` (B, S12). **Window:** `startSaveLayerImage`, `cancelSaveLayerImage`, `stopSaveLayerImage` (B, S12) + callback ke skrip `onSaveLayerImageProgress`, `onSaveLayerImageDone` (14 method + 2 callback) | rekonstruksi | A: inti-host, **emulasi-biner** (439 pemeriksaan, `emu_saveA.py`), sintaks, objek-host. **B: inti-host** (`test_B_core.cpp`, 66 pemeriksaan ASan/UBSan, cek mutasi), sintaks C++11/14, objek-host, skrip TJS di pelari host (174 PASS / 1 FAIL alias); **bacaan biner langsung, tanpa emulasi instruksi** | Nama modul asli **`LayerExSave.dll`** (L besar). Spesifikasi `spec/layerExSave_*.md` (A: 8, B: 7). **A:** `getDiffRect` meniru **bug biner** (saklar `LAYEREXSAVE_DIFFRECT_BOTTOM_FROM_RIGHT`, putuskan di 7.3); `oozeColor`: `level` tanpa efek; `isBlank` memeriksa byte 0 (dinamai "biru"; memori kemungkinan R,G,B,A → merah, lihat `layerExSave_common.md`). **B:** worker `std::thread` terlepas + `Android_PushEvents` (butuh perangkat); PNG disusun sendiri (8-bit RGBA, filter 0, zlib; chunk `pHYs`/`oFFs`/`vpAg` dari `reso_*`/`offs_*`/`vpag_*`, `comp_lv`) dan byte piksel disalin **tanpa penukaran R/B**; `Done.batal` = flag `cancelSaveLayerImage`, plugin **tidak** meng-invalidate layer `saveLayer:<nama>`. Deviasi sadar dari biner (bahaya konkurensi, lihat `spec/layerExSave_B_common.md`): pengecualian worker ditangkap (`LAYEREXSAVE_WORKER_CATCH=1`), tugas dibebaskan di thread utama, indeks negatif ditolak. ASUMSI: arity ncbind tiga method `Window` (fork: 1–3 parameter), identitas `0x8ed170`/`0x869f04` (`TVPCreateStream`/`TVPSaveAsTLG`), perilaku GPU, `ImageModified`, pesan galat bukan-Layer, nama parameter; konstruktor/destruktor `WindowSaveImage` tak dibaca (fork: `WindowState`). Tanpa `Update()`. |

### Salinan source publik (diadaptasi)

| Modul | Berkas | Method | Sumber | Uji | Catatan |
|---|---|---|---|---|---|
| `scriptsEx.dll` | `scriptsEx.cpp` + `bitap_fuzzy.hpp` | `Scripts.getObjectContext`, `isNullContext`, `equalStruct`, `equalStructNumericLoose`, `getMD5HashString`, `clone`, `propSet`, `propGet`, `pf*` (konstanta), `safeEvalStorage`, `stringFuzzySearch`, `rehash` | **salinan source publik** `wtnbgo/scriptsEx` (lisensi: "ikuti Kirikiri" menurut readme-nya; belum diaudit lebih jauh) | sintaks, objek-host (S11-log) | Disesuaikan ke fork: MD5 lewat `utils/md5.h`; pengganti `Try_iTJSDispatch2_*`; `getObjectKeys`/`getObjectCount`/`foreach` **tidak** didaftarkan ulang karena sudah native di `ScriptMgnIntf.cpp`. Terdaftar saat `Plugins.link("scriptsEx.dll")`. Belum NDK/perangkat. |

### Bawaan fork

| Modul | Berkas | Sumber | Uji | Catatan |
|---|---|---|---|---|
| `addFont.dll` | `addFont.cpp` | bawaan fork | belum teruji | `System.addFont` via `TVPEnumFontsProc`. |
| `csvParser.dll` | `csvParser.cpp` | bawaan fork | belum teruji | |
| `dirlist.dll` | `dirlist.cpp` | bawaan fork | belum teruji | |
| `getSample.dll` | `getSample.cpp` | bawaan fork | belum teruji | |
| `layerExMovie.dll` | `layerExMovie.cpp` | bawaan fork | belum teruji | |
| `perspective.dll` | `layerExPerspective.cpp` | bawaan fork | belum teruji | Memuat pemberitahuan hak cipta AGG di berkasnya (satu-satunya berkas bawaan dengan pemberitahuan hak cipta, sebagai string; S14). Kode `agg::` ada di blok `#if 0` (tidak terkompilasi; S14). Nama modul `perspective.dll`, **bukan** `layerExPerspective.dll`. |
| `saveStruct.dll` | `saveStruct.cpp` | bawaan fork | belum teruji | |
| `varfile.dll` | `varfile.cpp` | bawaan fork | belum teruji | |
| `wutcwf.dll` | `wutcwf.cpp` | bawaan fork | belum teruji | |
| `xp3filter.dll` | `xp3filter.cpp` + `xp3filter.h` | bawaan fork | belum teruji | Satu-satunya modul yang dimuat eksplisit di `TVPLoadInternalPlugins`. Asal-usul **tidak teridentifikasi** (audit S14). |

### Stub

| Modul | Berkas | Sumber | Uji | Catatan |
|---|---|---|---|---|
| `getabout.dll` | `getabout.cpp` | stub ringan | belum teruji | Mengalias `System.getAboutString` ke `TVPGetAboutString`. |
| `fftgraph.dll` | `fftgraph.cpp` | stub | belum teruji | Hanya mendefinisikan `drawFFTGraph(){}` kosong. |
| `win32dialog.dll` | `win32dialog.cpp` | stub | belum teruji | `WIN32Dialog.messageBox` dialihkan ke `System.inform`. |
| `motionplayer.dll` | `motionplayer_stub.cpp` | stub (**pencatat log**) | belum teruji | Tidak merender. Mendaftarkan `Motion.*` (`Player`, `EmotePlayer`, `ResourceManager`, ...) dan mencatat panggilan (`[motion] ...` di log aplikasi). **S17 (2026-11-25):** ditambah anggota yang terbukti dipakai game menurut bytecode skrip: +29 method dan +16 properti `Player`/`EmotePlayer`, `ResourceManager.load`/`clearCache`, `Motion.MaskModeAlpha` (nilai placeholder). Lolos `g++ -fsyntax-only` (C++11/14); **belum dijalankan**. Run 1 `krkr.console.log` gagal di `Motion` (global tidak ada setelah `Plugins.link`; ASUMSI: stub lama kosong); run 2-10 tidak lagi, tetapi game belum pernah memakai `Motion` (0 baris `[motion]`) karena berhenti di `keybinder.tjs`. Stub mendaftarkan `Player.useD3D` -> `Motion.enableD3D = true`; jalur D3D baru aktif bila `kag.d3dMotion` juga true (default 0). Merender (`.mtn`/`.psb`/`.pimg`) belum ada. Spesifikasi: [`spec/motionplayer_api_dipakai_game.md`](spec/motionplayer_api_dipakai_game.md); diagnosis: [`spec/motionplayer_diagnosis_S17.md`](spec/motionplayer_diagnosis_S17.md). |


## layerEx yang belum selesai (rekonstruksi dari binary — sedang berjalan)

Alamat implementasi per method: [`script/layerex_impl_map.tsv`](../script/layerex_impl_map.tsv) (dibuat `script/k2map.py`; heuristik, uji-diri 3/3, belum diverifikasi di Ghidra).
Source publik `wamsoft/layerEx` **tidak** memuatnya (hanya `LayerExBase`), jadi semuanya harus **rekonstruksi**.

| Kelompok | Method (dengan impl) | Kode biner | Status | Langkah rencana |
|---|---:|---:|---|---|
| ~~`layerExBTOA`~~ | 6 | 7.316 B | **ditulis (S3)** — lihat tabel rekonstruksi di atas; belum NDK/perangkat | 4.9c ✔ → 5.4 ✔ |
| ~~`layerExImage`~~ | 6 | 8.228 B | **ditulis (S4)** — lihat tabel di atas; belum NDK/perangkat | 4.9d ✔ → 5.5 ✔ |
| `layerExShimmer` | 2 | 5.264 B (+ fungsi lokal ≈ 1,7 KB di luar hitungan) | **rekonstruksi sebagian** (S5): inti-host lolos uji; interior `shimmer` ASUMSI | 4.9e-2 → 5.7 |
| `layerExAlpha` | 31 nama (28 ditulis) | ≈ 29,4 KB (28 ditulis) | **sebagian**: 28 dari 31 ditulis (S6–S10); sisa `AlphaBlend`, `WhiteToBlueBlurAdd`, `ExMaskRect2` | 4.9f → 5.6 (sesi "tutup Alpha") |
| `layerExSave` | 14 + callback `onSaveLayerImage*` (+ nama kelas `WindowSaveImage` tanpa literal): **14 method + 2 callback ditulis (A: S11, B: S12)** | 7.584 B (A 5.156 B ✅; B 2.428 B ✅ + worker/penyusun PNG) | **rekonstruksi selesai** (tabel di atas); B tanpa emulasi instruksi, thread belum diuji di perangkat | 4.9g ✅ → 5.8 ✅ |
| `multiImage`, `layerExDraw` | — | — | belum ada; `layerExDraw` memakai GDI+ (tidak bisa dibangun langsung) | tertunda |

**Sumber publik per kelompok (survei S14, `spec/survei_sumber_publik.md`).** Kolom Status/Langkah tabel S14 sudah usang (BTOA, Image, Raster, shrinkCopy, Shimmer, AreaAverage sudah ditulis; lihat tabel di atas); hanya kolom sumber yang dipakai di sini:

| Kelompok | Sumber publik (S14) |
|---|---|
| `layerExBTOA` | `wamsoft/layerExBTOA` (web) |
| `layerExImage` | `wamsoft/layerExImage` (web; sebagian kode CxImage) |
| `layerExShimmer` | zip di keepcreating.g2.xrea.com (gist; ketersediaan belum dicek) |
| `layerExAlpha` | **tidak ada** |
| `layerExSave` | `wamsoft/layerExSave` (web; LodePNG) |
| `multiImage` | tidak ada |
| `layerExDraw` | `wtnbgo/layerExDraw` (GDI+, Windows-only); alternatif `wamsoft/layerExVector` (ThorVG) |

## Backlog lain (36 nama di binary, belum ada di fork)

Window/script: `windowEx`, `kagparserex`, `kagparserexb`, `extkagparser`, `textrender` ·
Util/data: `json`, `sqlite3`, `minizip`, `expat`, `lzfs`, `fstat`, `util_generic`, `util_graph`, `util_system`, `fpackinone` ·
Game/media: `emoteplayer`, `psbfile`, `kaichotrans`, `extnagano`, `extrans`, `gfxeffect`, `alphamovie` ·
Stub Windows: `drawdeviced3d`, `drawdeviced3dz`, `kirikiroid2` ·
layerEx (di atas): `layerExAlpha` (28/31 ditulis), ~~`BTOA`~~ (ditulis), ~~`Image`~~ (ditulis), `Save` (A dan B ditulis), `Shimmer` (sebagian), `multiImage`, `layerExDraw`.

Status di fork seluruhnya **belum ada** (kecuali yang tertulis sebagai sudah ada di tabel di atas, mis. `scriptsEx` sejak S11). Hasil survei sumber publik (3.6, S14) — rincian 50 modul di [`spec/survei_sumber_publik.md`](spec/survei_sumber_publik.md):

| Modul | Sumber publik | Bukti |
|---|---|---|
| `json`, `scriptsEx` | `wamsoft/json`, `wamsoft/scriptsEx` (masing-masing 1 `.cpp`) | log |
| `minizip`, `expat` | `wamsoft/minizip` (4 `.cpp`), `wamsoft/expat` (2 `.cpp`); keduanya memakai vcpkg | log |
| `windowEx` | `wamsoft/windowEx` (1 `.cpp`; kemungkinan Windows-only — ASUMSI) | log |
| `sqlite3` | **bukan** `wamsoft/sqlite3` (tidak ada); `krkrz/krkrz` cabang `last_hodgepodge_repository` | log (tidak ada) + gist |
| `extrans` | `wamsoft/extrans` (7 transisi, penulis W.Dee) | web |
| `fstat`, `KAGParserEx` | `wtnbgo/fstat`, `wtnbgo/KAGParserEx` | gist |
| `kaichotrans`, `gfxeffect`, `alphamovie`, `extnagano`, `kagparserexb`, `extkagparser` | zip/arsip situs lama atau `sakano/krkr_archives` (AlphaMovie: bukan open source) | gist |
| `lzfs`, `util_generic/graph/system`, `fpackinone`, `multiimage`, `emoteplayer`, `psbfile`, `textrender` | **tidak ditemukan** (`psbfile`/`textrender` ada sebagai plugin statis di `wamsoft/krkrz_web`; repo-nya belum ketemu) | — |

## Risiko build ext (jangan lewatkan saat clone ke `src/plugins/ext/`)

`patch/plugins_cmake_ext.patch` mengglob **semua** `ext/**/*.cpp`. Belum diuji (tak ada `cmake` di sandbox), tetapi dari daftar berkas:
- `LicensesGen.cpp` (di `minizip`, `expat`) bukan bagian plugin → kemungkinan harus dikecualikan.
- `windowEx` kemungkinan Windows-only (ASUMSI) → jangan dibiarkan di `ext/` saat build Android.
- `layerEx` (clone lama) → `LayerExBase.cpp` ganda (3.5).
- `minizip` ext memuat `ioapi.cpp`; core fork juga punya `src/core/utils/minizip/ioapi.cpp` → kemungkinan simbol ganda.
- `minizip`/`expat` bergantung pada vcpkg; di fork, expat hanya terlihat di `Android.mk` (`expat_static`), build CMake belum terverifikasi.
Pakai `script/fetch_plugins.sh <nama>` (alamat di `script/ext_sources.tsv`), lalu hapus yang tidak dipakai.
