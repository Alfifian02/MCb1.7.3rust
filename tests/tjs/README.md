# Skrip uji TJS (rencana 7.2) — dijalankan di perangkat, lalu diperbaiki

**Riwayat.** Log pertama di perangkat (`uji_log.txt`): `PASS=7 FAIL=0` tetapi **5 `ERROR`** (`Member "ltAlpha"/"stLinear" does not exist`) —
lima fungsi `run_*` berhenti di tengah, jadi hampir semua `CHECK` tidak pernah jalan. Penyebab dan perbaikan ada di bagian "Aturan TJS".
**Status sekarang:** versi yang diperbaiki **lulus pemeriksaan tingkat bahasa** di mesin TJS2 milik repo (dikompilasi di host dengan tiruan
`Window/Layer/Plugins/Debug`; 134 PASS, 0 galat bahasa, 8 STEP selesai; 4 FAIL sisanya hanya karena tiruan tidak punya semantik piksel Alpha-4 dan alias `===`).
Semantik tiruan untuk `LayerExSave` A diturunkan dari ekspektasi tes itu sendiri, jadi PASS-nya tidak membuktikan apa pun tentang plugin).
**Log kedua di perangkat (2026-11-23): `PASS=80 FAIL=27`, 1 `ERROR`** — sudah dianalisis (lihat "Aturan TJS" 6–8): 19 FAIL = salah API piksel di skrip uji (bukan galat plugin), 8 FAIL + 1 ERROR = `LayerExSave.dll` tidak termuat di APK itu.
Perbaikannya sudah ada di berkas-berkas ini; **belum dijalankan ulang di perangkat**; kirim log berikutnya. Perilaku piksel plugin hanya bisa dinilai di perangkat.

## Aturan TJS (dipelajari dari log; jangan dilanggar)
1. **Konstanta `lt*`/`st*` adalah GLOBAL** (`ScriptMgnIntf.cpp`: `ltOpaque=1`, `ltAlpha=2`, `stLinear=2`), bukan anggota `Layer`. Tulis `ltAlpha`, bukan `Layer.ltAlpha`.
2. **Fungsi anonim tidak melihat variabel lokal fungsi pembungkusnya** (`Member "n" does not exist`). Galat ini dulu *tertelan* oleh `attempt` jadi `throw:...`
   sehingga `CHECK ... melempar` bisa lulus palsu dan baris `link.*`/`ada.*` di `t_registrasi` tidak valid. Pakai:
   - `callm(obj, "metode", [ arg1, arg2 ])` untuk memanggil method sambil menangkap lemparan, atau
   - `attempt(function(m) { ... }, nilai)` (nilai dioper sebagai parameter, bukan dari lokal).
3. String TJS2 tidak punya `lastIndexOf`; `arguments` hanya ada pada fungsi bertanda `*`. Gunakan `rest*` dan `fn(rest*)`.
4. `obs()` kini menandai **FAIL** bila nilai pengamatan memuat `does not exist`, agar galat nama/lingkup tidak tertelan lagi.
6. **`getMainPixel`/`setMainPixel` hanya RGB 24-bit; ALFA lewat `getMaskPixel`/`setMaskPixel`** (`LayerIntf.cpp:2615-2656`). Skrip lama memasukkan alfa lewat `setMainPixel(…, 0xAARRGGBB)` dan membacanya dengan `getMainPixel >> 24`: alfa tak pernah terisi/terbaca (selalu bawaan 0), dan bit atas malah dibaca `TVPToActualColor` sebagai ID warna sistem.
   Akibatnya `shrinkCopyFast ... alfa tujuan 0xFF` (16 FAIL), `rect7`/`rect8`/`border` (3 FAIL) gagal padahal plugin benar, dan `hash` alpha == opaque di log lama (hash tidak memuat alfa). Pakai `setPx`/`getPx`/`pixelAlpha` dari `harness.tjs`. **Hash baru ≠ hash log lama.**
7. **Urutan R/B**: `setMainPixel` menukar R/B (`TVP_REVRGB`) tetapi `getMainPixel` hanya menutup 24 bit, jadi bolak-balik bisa terbalik. `harness.tjs` mengukurnya saat dimuat (`HARNESS probe RGB…`, `selfcheck.getMainPixel_urutan`) dan `getPx` menormalkannya.
8. **Kanal `isBlank`/`copyBlueToAlpha` tidak diasumsikan**: spesifikasi menulis "byte 0 = biru", tetapi memori layer R,G,B,A (BTOA: biru = byte[2]) → byte 0 = **merah**. `t_layerExSaveA` mengukur kanalnya (`CASE isBlank.probe_R/G/B`, `copyBlueToAlpha.probe_*`, `layerExSave.kanal_byte0`) dan hanya meng-CHECK bahwa tepat satu kanal terlihat dan keduanya sama. Warna tulis-mentah (`oozeColor`, `clearAlpha`) diterima dalam urutan konvensional ATAU R/B terbalik dan urutannya dicatat (`CASE …urutan_warna`).
9. Bila `LayerExSave.dll` tidak termuat, `run_layerExSaveA` mencatat **satu** `CHECK FAIL` dan berhenti (tidak lagi berantai jadi `ERROR`). Sumber `layerExSave.cpp` ada di repo; APK harus dibangun dari pohon itu dengan **CMake dikonfigurasi ulang** (`file(GLOB)`).
5. Langkah pertama `selfcheck` (di `harness.tjs`) memeriksa konstanta global, `attempt`/`callm`, dan bahwa `callm` memanggil method **native** dengan `this` benar
   (`setImageSize`). Bila `selfcheck` FAIL di perangkat, jangan percaya hasil lain sebelum itu diperbaiki.

## Cara pakai
1. Buat folder game uji berisi semua `*.tjs` di sini (`startup.tjs` adalah titik masuk).
2. Edit `startup.tjs`: `RENDERER` = `"software"` atau `"opengl"` (samakan dengan pengaturan aplikasi; kunci preferensi `renderer`, default `software`).
3. **APK resmi 1.3.9:** biarkan `RUN_UNSAFE_CASES = false` (di `harness.tjs`) — kasus `src` null / `src == this` bisa crash atau membaca di luar batas di biner asli.
   **Fork:** boleh `true` untuk memeriksa pengecualian yang ditambahkan fork.
4. Jalankan. Hasil ditulis ke berkas **`uji_log.txt`** (tanpa PC): di folder game uji, dan salinannya di `/sdcard/` dan `/storage/emulated/0/`. Buka dengan pengelola berkas/penyunting teks lalu kirim isinya. Berkas ditulis ulang tiap baris, jadi bila skrip macet, baris terakhir menunjukkan posisinya. Baris `BOOT`/`STEP` = kemajuan, `ERROR` = galat skrip, `CHECK FAIL obs tanpa galat nama/lingkup` = galat bahasa tertelan. Di akhir, ringkasan (PASS/FAIL/ERROR) juga digambar di layar. Simpan satu log untuk APK 1.3.9 dan satu untuk fork, per `RENDERER`.
5. Bandingkan: `diff <(grep '^CASE' apk.txt) <(grep '^CASE' fork.txt)`. Baris `CHECK FAIL` pada fork = pelanggaran spesifikasi (cek dulu apakah spesifikasinya yang salah).

## Isi
| Berkas | Yang diuji | Catatan |
|---|---|---|
| `harness.tjs` | hash FNV-1a piksel, pola deterministik, `check`/`obs`/`attempt`/`callm`, `run_selfcheck` | `RUN_UNSAFE_CASES` |
| `t_registrasi.tjs` | 6.1/6.2: huruf besar-kecil nama, path, modul yang belum ada | `docs/spec/plugin_registrasi.md` |
| `t_stretchCopyAA.tjs` | alias, piksel == `stretchCopy`, ada/tidak sebelum `Plugins.link` | membandingkan waktu eksekusi dengan APK |
| `t_copyRaster.tjs` | normal, `maxh=0`, `lines=0`, `cycle=0`, ukuran beda, clip, clip kosong, alfa/opaque | arah geser CPU vs GPU: bandingkan `RENDERER` |
| `t_shrinkCopy.tjs` | 2:1, 1:1, pecahan, sub-rect, rect di luar sumber/tujuan, `sw/sh/dw/dh=0`, perbesar | CPU melempar, GPU tanpa validasi |
| `t_alpha4.tjs` (S9) | `MaskImageRideRect7/8`, `AlphaBorderInnerRect`: nilai tangan, ukuran beda, bukan Layer, argumen kurang | kasus null/mask pendek hanya dengan `RUN_UNSAFE_CASES`; di APK mungkin hanya renderer `software` |
| `t_shrinkCopyFast.tjs` | `hs`/`vs` (termasuk sisa grup, `vs` absen/0), ukuran tujuan, alfa 0xFF, `hs<=0` | **hash `hs>=2` = uji langsung asumsi `hrow_box` terhadap APK** |
| `t_layerExSaveA.tjs` | `LayerExSave.dll` bagian A: rect pembatas (alfa/nol/diff), `getDiffPixel` (void = tak diberikan), `oozeColor` (ambang, level tanpa efek), `copyBlueToAlpha`, `isBlank` (**kanal diukur**, lihat aturan 8), `clearAlpha` | `getDiffRect.BUG_BINER`: benar `0,0,3,6`, biner `0,0,3,1` — bandingkan APK vs fork. `copyBlueToAlpha` tanpa param hanya di `RUN_UNSAFE_CASES` |

Kasus dari 7.2 yang tercakup: rect normal, di luar layer, ukuran 0, layer tanpa alfa (`ltOpaque`).
`CHECK` hanya untuk hal yang spesifikasinya yakin dan tak bergantung pada angka biner; sisanya `CASE` (pengamatan, dibanding APK).

## Memeriksa bahasa skrip tanpa perangkat
`bash tests/tjs_host/build_and_run.sh` mengompilasi mesin TJS2 repo di host (g++ saja), menjalankan suite ini dengan tiruan `Window/Layer/Plugins/Debug`
(`tests/tjs_host/mock.tjs`) dan mencetak galat bahasa (harus kosong; probe disengaja `copyBlueToAlpha.bukan_layer` dikecualikan). Hanya memeriksa sintaks, lingkup, dan nama; **bukan** perilaku piksel plugin.
Dipakai sebelum menyerahkan perubahan skrip, agar kesalahan seperti `Layer.ltAlpha` atau closure tidak sampai ke perangkat. FAIL yang diharapkan di tiruan: hanya alias `===` (`stretchCopyAA`). Tiruan kini memakai semantik piksel ASLI fork (RGB 24-bit + mask); `RBSWAP=1 bash tests/tjs_host/build_and_run.sh` meniru R/B terbalik. Hasil terakhir: 146 PASS / 1 FAIL pada kedua mode.
Uji regresi tiruan: skrip LAMA dijalankan pada tiruan baru mereproduksi 34 FAIL (16 `shrinkCopyFast`, `rect7`, `rect8`, …) — jadi tiruan kini menangkap kelas galat ini.
