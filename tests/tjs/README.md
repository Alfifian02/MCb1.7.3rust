# Skrip uji TJS (rencana 7.2) — **belum dijalankan**

Skrip ini ditulis di sandbox tanpa mesin TJS: kurung seimbang saja yang dicek. Semantik API Layer dibaca dari `src/core/visual/LayerIntf.cpp`;
nama konstanta `Layer.ltAlpha` / `Layer.ltOpaque` / `Layer.stLinear` dan `Debug.message` adalah **ASUMSI** (nama standar TJS2, belum diverifikasi di build ini).
Bila ada galat sintaks/runtime saat dijalankan, kirim log-nya ke sesi berikutnya.

## Cara pakai
1. Buat folder game uji berisi semua `*.tjs` di sini (`startup.tjs` adalah titik masuk).
2. Edit `startup.tjs`: `RENDERER` = `"software"` atau `"opengl"` (samakan dengan pengaturan aplikasi; kunci preferensi `renderer`, default `software`).
3. **APK resmi 1.3.9:** biarkan `RUN_UNSAFE_CASES = false` (di `harness.tjs`) — kasus `src` null / `src == this` bisa crash atau membaca di luar batas di biner asli.
   **Fork:** boleh `true` untuk memeriksa pengecualian yang ditambahkan fork.
4. Jalankan, salin log aplikasi (baris `CASE`, `CHECK`, `#####`) ke berkas teks — satu untuk APK 1.3.9, satu untuk fork, per `RENDERER`.
5. Bandingkan: `diff <(grep '^CASE' apk.txt) <(grep '^CASE' fork.txt)`. Baris `CHECK FAIL` pada fork = pelanggaran spesifikasi (cek dulu apakah spesifikasinya yang salah).

## Isi
| Berkas | Yang diuji | Catatan |
|---|---|---|
| `harness.tjs` | hash FNV-1a piksel, pola deterministik, `check`/`obs`/`attempt` | `RUN_UNSAFE_CASES` |
| `t_registrasi.tjs` | 6.1/6.2: huruf besar-kecil nama, path, modul yang belum ada | `docs/spec/plugin_registrasi.md` |
| `t_stretchCopyAA.tjs` | alias, piksel == `stretchCopy`, ada/tidak sebelum `Plugins.link` | membandingkan waktu eksekusi dengan APK |
| `t_copyRaster.tjs` | normal, `maxh=0`, `lines=0`, `cycle=0`, ukuran beda, clip, clip kosong, alfa/opaque | arah geser CPU vs GPU: bandingkan `RENDERER` |
| `t_shrinkCopy.tjs` | 2:1, 1:1, pecahan, sub-rect, rect di luar sumber/tujuan, `sw/sh/dw/dh=0`, perbesar | CPU melempar, GPU tanpa validasi |
| `t_alpha4.tjs` (S9) | `MaskImageRideRect7/8`, `AlphaBorderInnerRect`: nilai tangan, ukuran beda, bukan Layer, argumen kurang | kasus null/mask pendek hanya dengan `RUN_UNSAFE_CASES`; di APK mungkin hanya renderer `software` |
| `t_shrinkCopyFast.tjs` | `hs`/`vs` (termasuk sisa grup, `vs` absen/0), ukuran tujuan, alfa 0xFF, `hs<=0` | **hash `hs>=2` = uji langsung asumsi `hrow_box` terhadap APK** |

Kasus dari 7.2 yang tercakup: rect normal, di luar layer, ukuran 0, layer tanpa alfa (`ltOpaque`).
`CHECK` hanya untuk hal yang spesifikasinya yakin dan tak bergantung pada angka biner; sisanya `CASE` (pengamatan, dibanding APK).
