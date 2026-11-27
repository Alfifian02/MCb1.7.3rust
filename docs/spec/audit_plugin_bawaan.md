# Audit asal-usul 14 plugin bawaan fork (sesi S14)

Dibuat: 2026-11-22. Metode: pencarian kata kunci (copyright, license, (c), ©, author, based on, original, nama penulis…) di **27 berkas**
`src/plugins/*.{cpp,h,hpp}` (berkas cp932 didekode ulang), lalu dicocokkan dengan `survei_sumber_publik.md`.
Bukan audit hukum; proyek ini pribadi, jadi hasilnya hanya **catatan asal-usul**.

Hasil utama: **hanya 1 berkas yang membawa pemberitahuan hak cipta** (`layerExPerspective.cpp`, AGG). Tidak ada berkas lain yang menyebut penulis/lisensi.
(Hitungan "copyright" di draf awal sesi ini terlalu tinggi karena pola `(c) ` cocok dengan kode seperti `(c = getc())`; sudah dikoreksi.)

| Berkas | Modul | Asal-usul (bukti) | Pemberitahuan di berkas | Catatan |
|---|---|---|---|---|
| `addFont.cpp` | `addFont.dll` | Versi asli `wtnbgo/addFont` (gist). Versi fork memakai `FontImpl.h`/`TVPEnumFontsProc` → **ditulis ulang untuk Android** (ASUMSI) | tidak ada | |
| `csvParser.cpp` | `csvParser.dll` | `wamsoft/csvParser` (web, penulis Go Watanabe). Fork: `PluginStub.h`, cp932 → port | tidak ada | |
| `dirlist.cpp` | `dirlist.dll` | `krkrz` `last_hodgepodge_repository/.../dirlist` (gist). `#include <windows.h>` dikomentari → port | tidak ada | |
| `getSample.cpp` | `getSample.dll` | `wtnbgo/getSample` (gist) | tidak ada | |
| `layerExMovie.cpp` | `layerExMovie.dll` | `krkrz` `.../layerExMovie` (gist). Fork menambah `krmovie.h`, `KRMovieLayer.h`, `layerExBase_GL` → **dimodifikasi** | tidak ada | |
| `layerExPerspective.cpp` | `perspective.dll` | `krkrz` `.../layerExPerspective` (gist) | **ada**: AGG 2.4 © 2002–2005 Maxim Shemanarev, tertanam sebagai string `copyright` | Seluruh kode `agg::` ada di blok **`#if 0` (baris 26–145)** → tidak terkompilasi; header AGG tidak ada di repo. Pertahankan string pemberitahuan. |
| `saveStruct.cpp` | `saveStruct.dll` | `wtnbgo/saveStruct` (gist); cp932 | tidak ada | Di biner ada dua ejaan: `savestruct.dll` dan `saveStruct.dll` (6.2). |
| `varfile.cpp` | `varfile.dll` | `wtnbgo/varfile` (web) | tidak ada | |
| `wutcwf.cpp` | `wutcwf.dll` | `krkrz/SamplePlugin/wutcwf` (gist) | tidak ada | |
| `xp3filter.cpp/.h` | `xp3filter.dll` | **Tidak teridentifikasi.** Daftar komunitas kosong; kode memakai `XP3Archive.h`, `ThreadIntf.h`, `<thread>` → khas fork | tidak ada | Satu-satunya yang dimuat eksplisit di `TVPLoadInternalPlugins`. |
| `getabout.cpp` | `getabout.dll` | Stub 5 baris (tulisan fork) | — | |
| `fftgraph.cpp` | `fftgraph.dll` | Stub 10 baris; aslinya `krkrz/fftgraph` (gist) | — | |
| `win32dialog.cpp` | `win32dialog.dll` | Stub 15 baris; aslinya `wamsoft/win32dialog` (web, miahmie) | — | |
| `motionplayer_stub.cpp` | `motionplayer.dll` | Dibuat di proyek ini (pencatat log) | — | |

Berkas pendukung (bukan modul): `LayerExBase.cpp/.h` = isi `wamsoft/layerEx` (3.4); `layerExBase.hpp` kemungkinan turunan berkas
bernama sama di repo `layerExRaster`/`layerExImage` (web) — **ASUMSI**, bandingkan di S1/4.7; `ncbind/`, `tp_stub.h`, `PluginStub.h`, `InternalPlugins.cpp`: asal tidak diaudit.
Berkas baru proyek ini (`layerExRaster/Shrink/AreaAverage*`): rekonstruksi dari perilaku biner.

## Tindak lanjut
- Pertahankan string AGG di `layerExPerspective.cpp`.
- `xp3filter`: bila suatu hari didistribusikan, catat asalnya dulu (belum diketahui).
- Uji asap 14 plugin masih tugas S2 (tidak ada uji sama sekali saat ini).
