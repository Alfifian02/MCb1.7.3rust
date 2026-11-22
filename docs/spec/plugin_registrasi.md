# Mekanisme registrasi plugin internal (fork) — hasil baca source, sesi S2

Tingkat bukti: **baca-source** (`src/plugins/ncbind/*`, `src/plugins/InternalPlugins.cpp`, `src/core/base/win32/PluginImpl.cpp`,
`src/core/base/StorageIntf.cpp`) + **kompilasi objek di host** (`g++ -c`, x86). **Belum dijalankan** di perangkat; belum build NDK.
Ini perilaku **fork**, bukan perilaku `libgame.so` 1.3.9 (yang belum dicocokkan, lihat "Belum diketahui").

## Alur dari `Plugins.link("x.dll")` sampai kode plugin jalan
1. Startup: `TVPLoadPluigins()` → `TVPLoadInternalPlugins()` (`InternalPlugins.cpp`) → `ncbAutoRegister::AllRegist()`.
2. **`AllRegist()` TIDAK menjalankan registrasi.** Ia hanya menelusuri daftar objek statis (`_top[line]`, tiga baris:
   `PreRegist`, `ClassRegist`, `PostRegist`) dan memasukkannya ke `_internal_plugins[<nama modul huruf kecil>].lists[line]`.
3. Satu-satunya modul yang dimuat otomatis saat startup: `xp3filter.dll` (`LoadModule` eksplisit di `TVPLoadInternalPlugins`).
4. Skrip game memanggil `Plugins.link("nama.dll")` → `TVPLoadPlugin(name)` → `TVPLoadInternalPlugin(name)` →
   `ncbAutoRegister::LoadModule(TVPExtractStorageName(name))`.
5. `LoadModule`: nama diambil **tanpa path** (`TVPExtractStorageName` memotong di `\`, `/`, `>`), diubah ke **huruf kecil**,
   lalu jika belum ada di `TVPRegisteredPlugins` dan ada di `_internal_plugins`: jalankan `Regist()` semua entri **berurutan baris**
   `PreRegist` → `ClassRegist` → `PostRegist`, lalu catat nama. Panggilan kedua untuk nama yang sama mengembalikan `false` (tanpa efek, tanpa error).
6. Nama modul tak dikenal: `TVPLoadPlugin` memanggil `TVPLoadInternalPlugin`, mengabaikan hasilnya lalu `return` (cabang pemuat DLL dimatikan
   dengan `#if 0`). Jadi `Plugins.link` untuk modul yang belum ada di fork **diam saja**, tanpa pengecualian.
7. `NCB_PRE_REGIST_CALLBACK(f)` / `NCB_POST_REGIST_CALLBACK(f)` membuat objek statis `ncbCallbackAutoRegister` yang `Regist()`-nya memanggil `f()`.

## Akibat untuk plugin baru
| Plugin | Callback | Kapan jalan di fork |
|---|---|---|
| `layerExAreaAverage.dll` | `PostRegist` → `TVPExecuteExpression("Layer.stretchCopyAA = Layer.stretchCopy;")` | saat `Plugins.link("layerExAreaAverage.dll")` — **bukan** saat startup/`AllRegist` |
| `layerExRaster.dll` | `PreRegist` → `PropSet` `copyRaster` pada `Layer` | saat `Plugins.link("layerExRaster.dll")` |
| `shrinkCopy.dll` | `PreRegist` → `PropSet` `shrinkCopy`, `shrinkCopyFast` | saat `Plugins.link("shrinkCopy.dll")` |

Koreksi: rencana 5.1 dan `layerExAreaAverage_stretchCopyAA.md` semula menulis "PostRegist saat `AllRegist`". Itu keliru untuk fork ini.

## 6.2 Huruf besar-kecil nama modul — terverifikasi (source)
Kunci pendaftaran (`AllRegist`: `name.ToLowerCase()`) dan nama yang dicari (`LoadModule`: `_name.AsLowerCase()`) sama-sama huruf kecil, dan path dipotong.
Maka `Plugins.link("LayerExRaster.DLL")` dan `Plugins.link("plugin/layerExRaster.dll")` memuat modul yang sama.
**Yang masih terbuka:** nama persis yang dipakai game nyata (mis. apakah ada game yang memanggil `layerExShrink.dll` alih-alih `shrinkCopy.dll`)
— butuh skrip game (**ASUMSI**: nama biner `shrinkCopy.dll` benar untuk method `shrinkCopy*`, sesuai string `0x14c5b54`).

## 6.3 Pengikatan & build — yang terbaca dari konfigurasi
- `CMakeLists.txt` akar menautkan `cpp_android_spec krkr2plugin` di antara `-Wl,--whole-archive` … `--no-whole-archive`. Jadi objek plugin statis
  **tidak dibuang linker** walau tak ada simbol yang dirujuk. Tidak perlu `--whole-archive` tambahan untuk jalur CMake.
- `src/plugins/CMakeLists.txt`: `file(GLOB … *.cpp)` memungut berkas baru **hanya saat CMake dikonfigurasi ulang** (hapus `.cxx`/`build_android` atau sentuh `CMakeLists.txt`).
  Opsi `-std=c++14 -fvisibility=hidden -fexceptions`; Gradle menambah `-frtti -fexceptions -fsigned-char`, `-DANDROID_ARM_NEON=TRUE`.
- `src/plugins/Android.mk` (`-std=c++11`, tanpa whole-archive) adalah jalur lama; `gradle.properties` memilih `PROP_BUILD_TYPE=cmake`, jadi **tidak dipakai**.
  Tiga berkas baru lolos cek sintaks di `-std=c++11` **dan** `-std=c++14` (g++ 13, tanpa `-fpermissive`).
- ABI: `PROP_APP_ABI=arm64-v8a` (default). **`armeabi-v7a` belum didukung konfigurasi saat ini**: cabang `else()` di `project/android/app/cpp/CMakeLists.txt`
  tetap memakai `vendor/build/arch_androida64` (pustaka arm64). Menambah armv7 butuh build `vendor` 32-bit (keputusan #3 di `sesi_kerja.md`).
- NDK `25.2.9519653` (clang 14), CMake `3.22.1`. **Clang tidak ada di sandbox**; semua cek sintaks memakai g++ → perbedaan clang tidak teruji (**ASUMSI**: kecil, karena lolos tanpa `-fpermissive`).
- CI: `.github/workflows/release-apk.yml` membangun APK arm64 (Gradle+CMake) lewat `workflow_dispatch`; butuh `vendor/` (cache atau variabel repo `VENDOR_URL`).

## 6.4 Duplikasi `LayerExBase` — tidak ada
- `LayerExBase.cpp` hanya satu (`src/plugins/`), `src/plugins/ext/` kosong → tidak ada definisi ganda `NI_LayerExBase`.
- `layerExBase.hpp` (huruf kecil, dipakai `layerExMovie.cpp`) tidak mendefinisikan `NI_LayerExBase`; berbeda berkas dari `LayerExBase.h` (ekstensi beda → tak bentrok di sistem berkas tak peka huruf).
- `layerExRaster.cpp` dan `layerExShrink.cpp` hanya meng-`include` `LayerExBase.h` dan tidak memakai `NI_LayerExBase`.
- Objek hasil `g++ -c` dari `layerExAreaAverage`, `layerExRaster`, `layerExShrink`, `LayerExBase`: tidak ada simbol kuat (T/D/B) ganda antar objek
  (satu-satunya "duplikat" adalah alias konstruktor C1/C2 di alamat sama dalam `LayerExBase.o`). Ini **bukan** link penuh.

## Catatan risiko rendah (tidak diubah)
- `ExitPlugin_Raster` / `ExitPlugin_Shrink` memakai `global` tanpa cek null (AreaAverage memeriksanya). Saat ini tak terjangkau: `TVPUnloadInternalPlugins()` tidak dipanggil siapa pun di repo.
- `AllUnregist()` memanggil `Unregist()` untuk **semua** objek statis, juga modul yang tidak pernah dimuat (`DeleteMember` pada anggota yang tak ada tidak berbahaya).

## Belum diketahui
1. Kapan `libgame.so` 1.3.9 menjalankan `Layer.stretchCopyAA = Layer.stretchCopy;` (saat link atau startup). Skrip `tests/tjs/t_stretchCopyAA.tjs` mencatat `ada_sebelum_link` untuk dibandingkan di APK.
2. Apakah `Plugins.link` modul yang belum ada di fork juga diam di 1.3.9 (`tests/tjs/t_registrasi.tjs`).
