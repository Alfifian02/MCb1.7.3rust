# Inventaris status & pembagian sesi kerja

Diperbarui: 2026-11-23 (S11 digabung dengan S1–S10) · Dokumen ini pelengkap [`rencana_decomp_recomp.md`](rencana_decomp_recomp.md) (rencana induk, bernomor langkah)
dan [`plugins.md`](plugins.md) (status per plugin). **Rencana induk tetap sumber kebenaran**; dokumen ini hanya mengelompokkan
langkahnya menjadi sesi yang bisa dikerjakan terpisah, masing-masing di percakapan Claude yang baru.

Legenda: ✅ ada & terbukti sejauh yang disebut · ⚠️ ada tetapi belum tuntas/belum terbukti · ❌ belum ada sama sekali.

---

## 1. Inventaris status

### 1.1 Cakupan dalam angka

| Ukuran | Nilai |
|---|---|
| Method layerEx dengan alamat implementasi (`script/layerex_impl_map.tsv`) | **62** (59 + 3 nama Alpha baru dari S7: `AlphaAdd`, `ColorVectorEffectAdd`, `MaskRect`) |
| Sudah direkonstruksi sebagai kode | **53 dari 62 nama**: `copyRaster`, `shrinkCopy`, `shrinkCopyFast`, 6 BTOA, 6 Image, 2 Shimmer (**sebagian**, interior ASUMSI), **28 Alpha**, **8 Save-A** (S11) (+ alias `stretchCopyAA`) |
| Belum ditulis | **9 nama**: Save-B 6 (+ `WindowSaveImage` dan callback `onSaveLayerImage*` di luar peta) + Alpha 3 (`AlphaBlend` 188 B, `WhiteToBlueBlurAdd` 344 B, `ExMaskRect2` 1.664 B) |
| Kode biner yang belum tercakup (perkiraan kasar) | **≈ 6,3 KB** (setelah S11): Save-B 2.428 B + 3 Alpha 2.196 B + helper Shimmer ≈ 1,7 KB (Save-A 5.156 B sudah tercakup). Persentase terhadap 63.532 B **tidak dihitung ulang**: angka itu memakai peta Alpha lama (bergeser satu slot), jadi ukuran per method Alpha berubah |
| Nama plugin backlog (36) yang masih belum ada di fork | **29** (7 sudah ditulis: Raster, AreaAverage, shrinkCopy, BTOA, Image, Alpha [28/31], Shimmer [sebagian]) |
| Plugin yang sudah lolos build NDK atau uji perangkat | **0** |
| Uji inti host pada pohon gabungan | **semua lolos** (BTOA, Image, Raster, Shimmer, Shrink, ShrinkFast, 4 uji Alpha; ASan/UBSan) |

Catatan: ukuran di atas hanya badan implementasi method. Fungsi pembantu yang dipanggilnya tidak dihitung, jadi pekerjaan nyata lebih besar (contoh Image: +2.684 B helper = 304 + 500 + 1.880).
Persentase ini ukuran **kode biner**, bukan jam kerja; fungsi berulang-pola (keluarga `MaskImageRideRect`) bisa lebih cepat dari perkiraan.

### 1.2 Sudah ada tetapi BELUM tuntas (⚠️)

| # | Item | Sudah ada | Yang kurang | Pemblokir | Sesi |
|---|---|---|---|---|---|
| 1 | `shrinkCopyFast` — CPU | ✅ **Tuntas di S1:** kode seluruhnya dari binary (vertikal `0x601d90` + horizontal `0x6023ec`), spesifikasi lengkap, uji inti `[BINARY-H]`, `hrow` diverifikasi emulasi instruksi asli | Tidak ada asumsi tersisa di jalur CPU. Yang belum: build NDK & uji perangkat (identik-piksel dengan 1.3.9 belum terbukti) | Build, perangkat | S2, S13 |
| 2 | `shrinkCopyFast` — GPU | Kode dengan saklar `SHRINKFAST_GPU_USE_DEST_TEXTURE` | Binary memakai tekstur `this` sebagai sumber (kemungkinan bug asli); belum diputuskan | Perangkat GPU + APK 1.3.9 | S13 |
| 3 | `copyRaster` | Kode, spesifikasi, uji inti (ditulis ulang), cek sintaks | Arah geser CPU vs GPU tampak berlawanan; ukuran tekstur GPU (`GetWidth` vs `GetInternalWidth`); nama parameter perkiraan | Perangkat | S13 |
| 4 | `shrinkCopy` | Kode, spesifikasi, uji inti, cek sintaks | Tanpa build NDK/uji TJS; deviasi sadar: `src` null melempar pengecualian (binary: crash) | Build, perangkat | S2, S13 |
| 5 | `stretchCopyAA` | Alias TJS (`layerExAreaAverage.cpp`) | S2: di **fork** jalan saat `Plugins.link` (baca-source). Waktu di **1.3.9** belum dicocokkan (`t_stretchCopyAA.tjs` siap) | Perangkat | S13 |
| 6 | **Semua kode baru** (Shrink, Raster, AreaAverage, BTOA, Image, Shimmer, Alpha) | Lolos `-fsyntax-only` terhadap header fork; `g++ -c` 9 objek layerEx OK, **tak ada simbol kuat ganda** antar-objek (penggabungan); semua uji inti host lolos | **Belum link, belum NDK (clang), belum dijalankan.** armeabi-v7a belum didukung konfigurasi | Anda perlu build (CI `release-apk.yml` + `vendor/`) | **S2 (sisa: Anda)** |
| 7 | `layerex_impl_map.tsv` | 59 alamat (+3 baris Alpha baru dari S7, pemetaan Alpha dikoreksi dan **diverifikasi ulang langsung dari biner** saat penggabungan); uji-diri 3/3 | Belum diverifikasi Ghidra; kolom `modul` hanya tebakan; **nama modul `.dll` terkonfirmasi untuk 7 modul** (Raster, AreaAverage, shrinkCopy, BTOA, Image, Shimmer, Alpha) | Ghidra (opsional) | Tiap sesi grup memverifikasi nama modulnya dari string biner |
| 8 | `layerExSave` bagian A (8 method) | ✅ Kode, spesifikasi, uji inti + emulasi instruksi asli, cek sintaks, skrip TJS (S11) | Build NDK/uji perangkat; **keputusan 7.3:** bug `getDiffRect` ditiru atau tidak; jalur GPU; `ImageModified` | Build, perangkat, APK 1.3.9 | S13 |
| 8 | 14 plugin bawaan fork | Kode ada | **Tidak ada uji sama sekali** di repo ini; asal-usul/lisensi belum diaudit | — | S2 (uji asap), S14 (audit) |
| 9 | Stub: `fftgraph`, `win32dialog`, `getabout`, `motionplayer` | Berjalan sebagai pengganti | Bukan implementasi. `motionplayer` hanya **mencatat log**, tidak merender → game ber-`Motion`/emote tidak akan tampil benar | Data log dari game | S17 |
| 10 | Tahap 1 | 1.1–1.3 | **1.4** Ghidra, **1.5** armv7 pembanding, **1.6** baseline build | Alat di PC Anda | S2, Opsional A |
| 11 | Tahap 4 sisa | 4.1–4.7 (4.7 selesai di S1: `docs/spec/helpers_map.md`, sebagian baris masih ASUMSI), 4.9a–b | **4.8** silang armv7 | 4.8 butuh 1.5 | Opsional A (4.8) |
| 12 | Tahap 6 (build & pendaftaran) | 6.1, 6.2, 6.4 diverifikasi dari source/objek-host (S2); `--whole-archive` sudah ada | **6.3** build arm64 (+ armv7 butuh `vendor` 32-bit); **6.2b** nama modul yang dipakai game | Build, skrip game | S2 (Anda) |
| 13 | Tahap 7 (verifikasi) | 7.1 dijalankan dengan skrip ketat: **46 OK / 20 KOSONG dari 66 nama**; 7.2 skrip ada di `tests/tjs/` (termasuk `t_alpha4`, `t_registrasi` diperbarui) | 7.2 **dijalankan**, 7.3 diferensial, 7.4 game nyata | Perangkat | S2 (jalankan 7.2), S13 (7.3–7.4) |
| 14 | 3.6 | — | Survei source publik `windowEx`, `scriptsEx`, `json`, `sqlite3`, `minizip`, `expat` + lisensi | — | S14 |
| 15 | 8.3, 8.4 | — | Tag rilis & changelog; periksa Java bila ada beda perilaku | — | S19 |
| 16 | **`layerExImage`** (S4) | Kode, 6 spesifikasi, uji inti + golden dari fungsi asli (jalur CPU **identik di emulator**), cek sintaks | Jalur **GPU** tak teruji; urutan byte R/B (colorize vs modulate) & CPU≠GPU perlu diputuskan; `Update()` di GPU; NDK; perangkat | Perangkat | S2, S13 |
| 17 | `layerExBTOA` (S3) | Kode, 7 spesifikasi, uji inti **14.526 pemeriksaan** (termasuk 4.635 kasus dari eksekusi fungsi asli di emulator), cek sintaks | GPU (`CopyAlphaFromBlue`, `FillByProvince` — arah blend kemungkinan terbalik di biner) tak teruji; NDK; perangkat | Perangkat | S2, S13 |
| 18 | `layerExAlpha` (S6–S10) — 28 dari 31 nama | Satu kelas, satu Core, 4 uji inti, emulasi instruksi biner untuk Alpha-1 dan Alpha-3, cek sintaks | **(a)** 3 nama belum ditulis; **(b)** ukuran tujuan = ASUMSI (rect layer vs gambar; `reset()` biner belum dibaca); **(c)** `AlphaAdd`/`ColorAlphaAdd` lewat objek `Function`; **(d)** anomali kolom `MaskImageRideRect5` dan `AlphaDivide` per-baris (keputusan fork vs biner); **(e)** NDK/perangkat/GPU | (a) recon `ONLY="AlphaBlend WhiteToBlueBlurAdd ExMaskRect2"`, (b) baca `layerExBase_GL::reset()` di biner | sesi "tutup Alpha", S2, S13 |
| 19 | `layerExShimmer` (S5) — **sebagian** | Dua fungsi utama terbaca penuh; inti-host + sintaks | Interior `shimmer` (worker `0x5f0494/0x5f0630`), jalur argumen ke-4 `shimmerBuildMap` (`0x5f0798`): ASUMSI | Dump helper (4.9e-2) | S5 lanjutan |

### 1.3 Sama sekali BELUM ada (❌)

| Kelompok | Isi | Ukuran kode biner | Sesi |
|---|---|---:|---|
| `layerExShimmer` | `shimmer` (4.696 B, skalar — terbukti dari disassembly), `shimmerBuildMap` — **S5 sebagian** (helper belum dibaca) | 5.264 B (+ fungsi lokal ≈ 1,7 KB di luar hitungan) | S5 lanjutan |
| `layerExAlpha` (3 nama sisa) | `AlphaBlend` (`0x5d5894`, 188 B), `WhiteToBlueBlurAdd` (`0x5d5f98`, 344 B), `ExMaskRect2` (`0x5d6ea0`, 1.664 B; recon ada di `recon_alpha2.txt`) | 2.196 B | sesi "tutup Alpha" |
| `layerExSave` — **bagian B** | 6 method (`startSaveLayerImage`, `cancelSaveLayerImage`, `stopSaveLayerImage`, `saveLayerImageTlg5`, `saveLayerImagePng`, `saveLayerImagePngOctet`) + `WindowSaveImage` + callback `onSaveLayerImage*` (thread, PNG, TLG5). *(Bagian A, 8 method, **selesai di S11**.)* | 2.428 B | S12 |
| `multiImage`, `layerExDraw` | `layerExDraw` memakai GDI+ (tidak bisa dibangun langsung) | — | S18 (kajian kelayakan) |
| Window/script | `windowEx`, `scriptsEx`, `kagparserex`, `kagparserexb`, `extkagparser`, `textrender` | — | S15 |
| Util/data | `json`, `sqlite3`, `minizip`, `expat`, `lzfs`, `fstat`, `util_generic`, `util_graph`, `util_system`, `fpackinone` | — | S14 |
| Game/media | `kaichotrans`, `extnagano`, `extrans`, `gfxeffect`, `alphamovie` | — | S16 |
| Stub Windows | `drawdeviced3d`, `drawdeviced3dz`, `kirikiroid2` | — | S16 |
| Besar/proprietary | `emoteplayer`, `psbfile` | — | S17 |

Di luar cakupan dekompilasi (butir Roadmap `readme.md` yang bukan plugin): ganti rendering cocos→SDL2, konfigurasi via command line, dekripsi game cx berhash, akses sdcard lewat SAF, target Windows/Linux. Tidak dijadwalkan di sini.

---

## 2. Aturan lintas-sesi

**Kenapa perlu:** sandbox Claude **tidak menyimpan berkas antar percakapan**. Kesinambungan bergantung pada berkas yang Anda unggah kembali.

**Awal sesi (Anda):**
1. Salin zip perubahan sesi sebelumnya (mis. `perubahan_*.zip`) **menimpa** repo Anda, lalu zip ulang repo → unggah sebagai `Repo.zip`.
2. Unggah `Kirikiroid2_1_3_9.zip` (semua sesi dekompilasi). Sejak S4: Claude memasang `capstone`/`unicorn` lewat pip di sandbox, jadi bisa **mendisassembly dan menjalankan fungsi** `libgame.so` sendiri (`script/k2dis.py`, `script/k2emu_unicorn.py`).
3. Berkas recon (`k2recon.sh`) kini **opsional** (cadangan bila pip tidak tersedia); jika ada, unggah juga.
4. Tempel prompt pembuka (bagian 4).
5. Bila beberapa sesi dikerjakan paralel dari `Repo.zip` yang sama, minta sesi **penggabungan** — jangan menimpa zip begitu saja: S6–S10 saling menimpa `layerExAlphaCore.hpp`/`layerExAlpha.cpp`/`test_core.cpp`, S4 dan S6 sama-sama membuat `script/k2emu.py`, dan ketiga dokumen induk bentrok.

**Akhir sesi (Claude):** memperbarui `rencana_decomp_recomp.md` (centang, temuan, log), `plugins.md`, dan spesifikasi di `docs/spec/`; memberi satu zip berisi **hanya** berkas yang berubah, dengan path repo.

**Yang Claude tidak bisa di sandbox:** build NDK, menjalankan TJS/GPU di perangkat, uji diferensial penuh terhadap APK (GPU, sistem, waktu). Itu tugas Anda; hasilnya (log/teks) Anda unggah.
**Yang bisa:** disassembly AArch64 (capstone) dan **menjalankan fungsi asli di emulator** untuk uji diferensial jalur CPU (`k2emu_unicorn.py`; bergantung pada pip di sandbox, tidak dijamin), membaca recon, menulis spesifikasi dan kode, uji inti di host (ASan/UBSan), cek sintaks terhadap header inti fork, memetakan alamat dengan `k2map.py`.
**Hati-hati:** recon depth-0 tidak memuat helper yang dipanggil method; `k2dis.py` ke alamat helper menutup celah itu.

**Disiplin kualitas:** bagian yang belum dibaca diberi label **ASUMSI**; kata "teruji" hanya untuk yang benar-benar punya uji dan disebut tingkatnya (inti-host / sintaks / NDK / perangkat) seperti di `plugins.md`.

**Ukuran sesi:** patokan ≤ ~7 KB kode biner per sesi (recon BTOA 7,3 KB = 137 KB teks). Bila recon terasa terlalu besar, pecah dengan `ONLY="..."`.

**Templat sesi dekompilasi** (S3–S12), langkahnya sama:
1. Baca recon → verifikasi **nama modul** (string UTF-16) dan nama method.
2. Tulis `docs/spec/<modul>_<method>.md` memakai templat di rencana induk (parameter & default, efek piksel, error `TJS_E_*`, kasus tepi, uji).
3. Tulis `src/plugins/<modul>Core.hpp` (inti tanpa dependensi, skalar dulu) + `<modul>.cpp` (gaya `layerExShrink.cpp`) + `tests/plugins/<modul>/test_core.cpp`.
4. Uji inti (ASan/UBSan) + cek sintaks terhadap header fork.
5. Perbarui rencana, `plugins.md`, log; zip perubahan.

---

## 3. Peta sesi

| Sesi | Judul | Langkah rencana | Pelaku utama | Masukan siap? | Ukuran |
|---|---|---|---|---|---|
| **S1** ✅ | Tutup `shrinkCopyFast` + kamus helper — **selesai** | 4.6b-2, 5.3c, 4.7 | Claude | ✅ `recon_6023ec.txt` | S |
| **S2** | Build & verifikasi dasar | 1.6, 6.1–6.4, 7.1, 7.2 | **Anda** (+ Claude triase error) | ✅ bagian Claude selesai; **menunggu build & log Anda** | M |
| Opsional A | Ghidra & armv7 | 1.4, 1.5, 4.8 | Anda | — | M |
| **S3** ✅ | BTOA | 4.9c, 5.4 | Claude | ✅ **selesai** (`recon_btoa.txt` dibaca) | M |
| **S4** | Image | 4.9d, 5.5 | Claude | ✅ **selesai** (lihat bawah) | M |
| **S5** | Shimmer — **sebagian** (tinggal 4.9e-2) | 4.9e, 5.7 | Claude | ✔️ `recon_shimmer.txt` ada; **kurang:** `recon_shimmer_helpers.txt` | M |
| **S6–S10** ✅ | Alpha-1 … Alpha-5 — **28 dari 31 nama selesai** | 4.9f, 5.6 | Claude | ✅ diterima semua | 5 × M |
| **S-Alpha** (baru) | Tutup Alpha: `AlphaBlend`, `WhiteToBlueBlurAdd`, `ExMaskRect2` + putuskan ASUMSI ukuran tujuan (baca `layerExBase_GL::reset()`) | 4.9f, 5.6 | Claude | `libgame.so` (capstone bisa dipasang lewat pip) atau `ONLY="AlphaBlend WhiteToBlueBlurAdd ExMaskRect2"` | S |
| **S11** ✅ | Save-A (utilitas) — **selesai** | 4.9g-A, 5.8a | Claude | ✅ `recon_saveA.txt` | M |
| **S12** | Save-B (thread, PNG, TLG5) | 5.8 | Claude | `libgame.so` (recon opsional) | M |
| **S13** | Verifikasi diferensial layerEx | 7.3, 7.4 | **Anda** (+ Claude analisis hasil) | — | M |
| **S14** | Survei sumber publik + util/data | 3.6, 5.9 | Claude (+ pencarian web) | — | M |
| **S15** | Window/script | 5.9 | Claude | tergantung S14 | L |
| **S16** | Media/game kecil + stub Windows | 5.9 | Claude | tergantung S14 | M |
| **S17** | `emoteplayer` / `psbfile` | 5.9 | Claude | log `motionplayer` | XL (kemungkinan >1 sesi) |
| **S18** | `layerExDraw` & `multiImage` | kajian kelayakan | Claude | — | S–M |
| **S19** | Rilis & pemeliharaan | 8.3, 8.4, ulang 7.1 | Anda + Claude | — | S |

### Urutan & ketergantungan yang disarankan
```
S1 ──► S2 ──► S3 ──► S4 ──► S5 ──► S6…S10 ──► S11 ──► S12 ──► S13 ──► S19
        │                                                      ▲
        └─ (kapan saja, sebelum S13) Opsional A                │
S14 ──► S15 / S16 / S17 / S18   (jalur paralel; tidak bergantung pada layerEx)
```
- **S2 sebaiknya sebelum S3**: bila kode baru ternyata tidak terkompilasi di NDK, kesalahan yang sama akan terulang di semua grup berikutnya.
- Urutan S4–S12 bisa diubah menurut **game yang Anda pakai**: cari `Plugins.link("...")` di skrip game dan dahulukan modul yang benar-benar dipanggil.
- S6–S10 (Alpha) mungkin bisa digabung bila keluarga `MaskImageRideRect` ternyata berbagi pola.

---

## 4. Detail per sesi

### Prompt pembuka (tempel, ganti `<Sx>`)
```
Lanjutkan proyek decomp/recomp Kirikiroid2. Ekstrak semua arsip.
Baca docs/sesi_kerja.md dan docs/rencana_decomp_recomp.md, lalu kerjakan sesi <Sx> persis seperti di bagian 4.
Di akhir: perbarui rencana (centang, temuan, log), docs/plugins.md dan docs/spec/, lalu beri satu zip berisi hanya berkas yang berubah.
Bagian yang belum terbaca tandai ASUMSI. Jika ada berkas yang kurang, katakan.
```

### S1 — Tutup `shrinkCopyFast` + kamus helper  ✅ selesai
- **Status:** selesai; rincian hasil di log `rencana_decomp_recomp.md` (4.6b-2, 5.3c, 4.7) dan `docs/spec/helpers_map.md`. Teks di bawah dipertahankan sebagai catatan tugas aslinya.
- **Unggah:** `Repo.zip` (terbaru), `Kirikiroid2_1_3_9.zip`, `recon_6023ec.txt` (**sudah ada**).
- **Kerjakan:** (a) baca `0x6023ec` → tulis ulang `hrow_box()` persis (bobot, pembulatan, alfa, penanganan blok sisa, NEON vs skalar); (b) perbarui spesifikasi, ubah label `[ASUMSI-H]` di uji menjadi uji ekspektasi dari binary; (c) 4.7: tulis `docs/spec/helpers_map.md` (alamat helper → fungsi di fork: lebar/tinggi gambar, sinkron, pointer/pitch, `IsSoftware`, konversi `tTJSVariant`) dan cocokkan dengan `layerExBase.hpp`.
- **Keluaran:** `layerExShrinkFastCore.hpp` (tanpa asumsi), uji, spesifikasi lengkap, `helpers_map.md`.
- **Selesai bila:** tidak ada lagi label ASUMSI pada `shrinkCopyFast` CPU; 4.6b-2, 5.3c, 4.7 tercentang.

### S2 — Build & verifikasi dasar (utamanya tugas Anda)
- **Anda:** pindahkan repo ke `$HOME` Termux atau bangun di PC (1.6, hindari `/storage/emulated/0`); build baseline tanpa perubahan; lalu build dengan modul baru (`layerExAreaAverage`, `layerExRaster`, `layerExShrink`); jalankan ulang CMake (GLOB). Kirim log error.
- **Claude (sesi ini):** memperbaiki error kompilasi/link dari log; memeriksa 6.1–6.4 (registrasi lewat `AllRegist`, huruf besar-kecil nama modul sesuai skrip game, `--whole-archive` bila modul tidak termuat, tidak ada duplikasi `LayerExBase`); menjalankan `FULL=1 bash script/check_layerex_parity.sh` (7.1); menulis skrip TJS kecil per method (7.2: rect normal, di luar layer, ukuran 0, layer tanpa alfa → cetak hash piksel).
- **Unggah:** `Repo.zip`, log build (teks), hasil parity & TJS bila sudah dijalankan.
- **Selesai bila:** 3 modul baru terbangun untuk `arm64-v8a` (dan `armeabi-v7a` bila Anda targetkan) dan `Plugins.link(...)` memuatnya di perangkat. Setelah itu `plugins.md` boleh menaikkan tingkat "NDK".
- **Status (bagian Claude, dikerjakan tanpa log build):** 6.1, 6.2, 6.4 diverifikasi dari source + objek-host; 7.1 dijalankan (4/63 OK); skrip 7.2 ditulis di `tests/tjs/` (belum dijalankan); temuan di `docs/spec/plugin_registrasi.md`. **Belum terpenuhi:** kriteria "selesai" di atas.
- **Yang Anda lakukan berikutnya:** (1) bangun lewat `workflow_dispatch` `release-apk.yml` (butuh `vendor/`/`VENDOR_URL`) atau di PC/Termux; kirim log Gradle/CMake (teks) — Claude akan memperbaiki error di sesi lanjutan S2. (2) Jalankan `tests/tjs/startup.tjs` (lihat `tests/tjs/README.md`) di fork, dan bila bisa di APK 1.3.9 (`RUN_UNSAFE_CASES=false`); kirim log.
- **Berkas yang kurang di sesi ini:** `vendor/` (tidak ada di `Repo.zip`), log build, log TJS, skrip game (untuk 6.2b).

### Opsional A — Ghidra & armv7 (Anda; tanpa Claude)
Pasang Ghidra, import `libgame.so` arm64 (AARCH64:LE:64:v8A, base 0) dan armv7 (1.4, 1.5); cocokkan beberapa alamat `layerex_impl_map.tsv`; bantu 4.8 untuk method ambigu. Sangat disarankan sebelum S5/S6 (fungsi besar, NEON).

### S3 — BTOA  ✅ selesai (2026-11-22)
- **Unggah:** `Repo.zip`, `Kirikiroid2_1_3_9.zip`, `recon_btoa.txt` (**sudah ada**; 6 method, 7.316 B).
- **Kerjakan:** templat sesi dekompilasi untuk `copyRightBlueToLeftAlpha`, `copyBottomBlueToTopAlpha`, `fillAlpha`, `copyAlphaToProvince`, `clipAlphaRect` (3.040 B, terbesar), `fillByProvince`. Dari namanya, `copyAlphaToProvince`/`fillByProvince` kemungkinan memakai gambar *province* (bukan piksel utama) — verifikasi dari recon dan cocokkan helper-nya dengan `layerExBase.hpp`.
- **Keluaran:** `docs/spec/layerExBTOA_*.md`, `layerExBTOACore.hpp`, `layerExBTOA.cpp`, uji.
- **Selesai bila:** 6 method ditulis, uji inti lolos, sintaks lolos, nama modul terverifikasi dari string biner.
- **Status:** ✅ selesai (BTOA lengkap; lihat `rencana_decomp_recomp.md` 4.9c/5.4 dan `docs/spec/layerExBTOA_module.md`). Alat sesi ini: `script/emu/` (harness Unicorn + `k2dis.py` + `btoa_golden.py`).

### S4 — Image  ✅ selesai (2026-11-22)
- **Hasil:** 6 spesifikasi, `layerExImageCore.hpp`, `layerExImage.cpp`, uji + golden dari fungsi asli, `k2dis.py`, `k2emu_unicorn.py` (sebelumnya `k2emu.py`; diganti nama saat penggabungan). Jalur CPU identik dengan biner di emulator; GPU/NDK/perangkat belum. Rincian dan temuan: `rencana_decomp_recomp.md` 4.9d dan 5.5.
- **Tersisa dari S4 (bukan S4 lagi):** keputusan R/B dan CPU≠GPU, `Update()` di GPU → S13; build → S2.
- (Catatan lama, untuk arsip) Recon yang diminta ternyata tidak dibutuhkan setelah `k2dis.py` ada:
- **Recon Anda jalankan:** `bash script/k2recon.sh @image > recon_image.txt` (6 method, 8.228 B — batas atas; bila terlalu besar pecah: `ONLY="light colorize modulate"` lalu `ONLY="noise generateWhiteNoise gaussianBlur"`).
- **Catatan:** `light` baru ditemukan oleh 4.9a (tidak ada di daftar 61 nama awal). `noise`/`generateWhiteNoise` kemungkinan memakai RNG → tentukan algoritma & seed agar uji diferensial bermakna. `gaussianBlur` kemungkinan memakai tabel bobot.

### S5 — Shimmer
- **Recon:** `bash script/k2recon.sh @shimmer > recon_shimmer.txt` (`shimmer` 4.696 B, `shimmerBuildMap` 568 B; keduanya di bawah `MAXSZ` bawaan 6000).
- **Catatan:** pindai struktural (`script/k2scan.py`) **tidak mendukung** dugaan NEON: `shimmer` hampir seluruhnya integer skalar + beberapa `fcvtzs`; tetap tulis skalar dulu.
- **Status 2026-11-21 — sebagian:** `shimmer` dan `shimmerBuildMap` sudah dibaca dan ditulis (`layerExShimmerCore.hpp`, `layerExShimmer.cpp`, uji inti lolos ASan/UBSan). Interior `shimmer` dikerjakan worker thread yang **tidak** ada di recon (ASUMSI-INT), dan jalur argumen ke-4 `shimmerBuildMap` memanggil `0x5f0798` (belum dibaca → `TJS_E_NOTIMPL`). Rincian: `docs/spec/layerExShimmer_*.md`.
- **Untuk menutup (Anda jalankan):** `for a in 0x5f0248 0x5f0494 0x5f0630 0x5f0798 0x5f0410 0x5f0424 0x5f05ac 0x5f05c0; do bash script/k2recon.sh $a 0; done > recon_shimmer_helpers.txt`, unggah bersama `Repo.zip` terbaru. Helper umum (`0xa0cd8c`, `0xa0e078`, `0xa11b94`, `0x9b07c8`, `0xa0dee0`, `0xa34aa4`, `0xa34ad0`) dipakai juga di BTOA; kamusnya dikerjakan di S1 (4.7). Setelah itu: sesi pendek "tutup Shimmer" (hapus ASUMSI-INT, putuskan L1–L3 lewat uji diferensial).

### S6–S10 — Alpha (**31 nama** — biner mendaftarkan 31, bukan 28; 28 ditulis, 3 sisa)
Recon: `bash script/k2recon.sh @alpha` dengan `ONLY=` berikut, satu per sesi (`> recon_alpha1.txt` dst.). Nama modul `layerExAlpha.dll` ✔️ **terkonfirmasi** dari string UTF-16 (`0x14c9f5c`; 14 nama method S6 kontigu `0x14c9f98…0x14ca0f8`, lalu `LightForAlpha` = awal S7).

**Status S6:** ✅ **selesai** (14 method; `recon_alpha1.txt` diterima). Keluaran: `src/plugins/layerExAlpha.cpp` + `layerExAlphaCore.hpp`, `docs/spec/layerExAlpha_*.md`, `tests/plugins/layerExAlpha/`, alat `script/k2emu.py` + `k2emu_alpha1.py`. Bukti: inti-host (ASan/UBSan) + emulasi instruksi + sintaks; NDK/perangkat/GL belum. **Pelajaran untuk S7–S10:** `k2emu_alpha1.py` mengeksekusi recon di host dan membandingkannya dengan kode C++ → pakai ulang (tambah entri `METHODS`, generator kasus, hook bila ada helper baru). Perintah recon Alpha-2..5 di tabel berikut.

**Koreksi penggabungan:** daftar `ONLY=` S6 lama memakai peta bergeser satu slot; tabel di bawah sudah memakai pasangan NAME→CODE yang dibaca langsung dari registrasi `0x5d43b0`. Dari 17 badan fungsi di rentang S6, S6 mentranskripsi 14; yang belum: `AlphaBlend`, `WhiteToBlueBlurAdd` (di tengah rentang) dan `ExMaskRect2` (sesudahnya).

| Sesi | Isi | Ukuran | `ONLY=` |
|---|---|---:|---|
| S6 Alpha-1 ✅ | 14 method | 6.828 B | `"turnAlpha turnAlpha2 turnAlpha3 turnOutline AlphaColorRect AlphaDivide AlphaAdd AlphaAdd2 BlackForAlpha ColorAlphaAdd ColorVectorEffectAdd ExtraColorVectorEffectAdd MaskRect ExMaskRect"` |
| S7 Alpha-2 ✅ | 7 method | 5.872 B | `"LightForAlpha LuminanceForAlpha LevelMaskRect LevelMaskRect2 MaskImageRideRect MaskImageRideRect2 MaskImageRideRect3"` |
| S8 Alpha-3 ✅ | 2 method | 5.852 B | `"MaskImageRideRect4 MaskImageRideRect5"` — rincian di rencana (4.9f/S8) dan `docs/spec/layerExAlpha_MaskImageRideRect4.md`, `…5.md` |
| S9 Alpha-4 ✅ | 3 method | 5.892 B | `"MaskImageRideRect7 MaskImageRideRect8 AlphaBorderInnerRect"` |
| S10 Alpha-5 ✅ | 2 method | 4.488 B | `"AlphaBorderImageCoverRect MaskImageEffectRect"` |
| **Sisa** | 3 method | 2.196 B | `"AlphaBlend WhiteToBlueBlurAdd ExMaskRect2"` (nama sesuai TSV terkoreksi) |

Catatan: nama terurut `MaskImageRideRect`, `2`, `3`, `4`, `5`, `7`, `8` — **tidak ada `6`** di daftar nama maupun di peta; ✔️ **terkonfirmasi (S7)**: string `MaskImageRideRect6` tidak ada di biner (UTF-16 maupun ASCII). Rangkum pola bersama dulu (S6/S7) agar S8–S10 bisa memakai ulang inti piksel.

**Status S7 (2026-11-22): SELESAI, 7/7 method** (kode + uji inti host; `MaskImageRideRect3` dari `recon_alpha2c.txt`). ⚠️ Pemetaan TSV Alpha dikoreksi S7 (bergeser satu slot untuk `AlphaAdd2`…`MaskImageRideRect3`) dan **diverifikasi ulang langsung dari biner** pada penggabungan (lihat rencana 4.9f, Temuan penggabungan). Pasang TSV terkoreksi sebelum memakai `ONLY=` untuk Alpha.



**Status S8–S10:** ✅ selesai (S8 2/2, S9 3/3, S10 2/2). Zip S6–S10 semuanya menimpa `layerExAlpha*` → **sudah digabung** dalam sesi penggabungan (satu kelas, satu Core, empat berkas uji + `tests/plugins/layerExAlpha/run_tests.sh`). Catatan arsip S8: `MaskImageRideRect4` dan `5` **bukan** satu pola (Rect4 = tabel int + warna tetap; Rect5 = tiga tabel float tanpa warna) — baca per method; yang dapat dipakai ulang hanya kerangka (cek layer, cek ukuran + log `SW/DW/AW`, `band(a,n)`, loop berpitch, `redraw`) dan `tests/plugins/layerExAlpha/emu_ride.py`.

### S-Alpha — Tutup Alpha (baru, setelah penggabungan)
- **Kerjakan:** (a) baca `AlphaBlend` (`0x5d5894`, 188 B), `WhiteToBlueBlurAdd` (`0x5d5f98`, 344 B), `ExMaskRect2` (`0x5d6ea0`, 1.664 B; recon di `recon_alpha2.txt`); tambahkan ke `layerExAlphaCore.hpp`/`layerExAlpha.cpp` (anggota kelas + `NCB_METHOD`), spesifikasi, uji, **dan** perbarui `script/check_layerex_parity.sh` hasilnya (target Alpha 31/31); (b) baca `layerExBase_GL::reset()` di biner → putuskan apakah ukuran tujuan = rect atau gambar; (c) selidiki objek `Function` pada `AlphaAdd`/`ColorAlphaAdd` (registrasi `0x5d43b0`).
- **Unggah:** `Repo.zip` hasil penggabungan, `Kirikiroid2_1_3_9.zip` (capstone/unicorn via pip bila tersedia).
- **Selesai bila:** parity ketat Alpha 31/31; ASUMSI ukuran tujuan tertutup atau tercatat jelas.

### S11 — Save-A (utilitas) ✅ selesai
- **Hasil (S11 ✅):** 8 method terekonstruksi dan **dieksekusi di emulator dari instruksi asli** (439 pemeriksaan, 0 beda); spesifikasi `docs/spec/layerExSave_*.md`; kode `layerExSave.cpp` + `layerExSaveCore.hpp` (modul **`LayerExSave.dll`**); uji inti ASan/UBSan + mutasi; skrip TJS `t_layerExSaveA.tjs`. Temuan & keputusan terbuka: rencana 4.9g-A, 5.8a dan catatan 29–30.
- **Tersisa untuk S13:** saklar `LAYEREXSAVE_DIFFRECT_BOTTOM_FROM_RIGHT` (bug biner `getDiffRect`), perilaku GPU, `ImageModified`; usul recon depth-1 `0x80c848` + badan vfn bitmap bila perlu. Catatan: `Kirikiroid2 dll list.csv` menyebut source publik `layerExSave` (wtnbgo/krkrz) — **belum dibandingkan** dengan rekonstruksi S11; lakukan di S14 bila lisensinya cocok.
- **Recon:** `ONLY="getCropRect getCropRectZero getDiffRect getDiffPixel oozeColor copyBlueToAlpha isBlank clearAlpha" bash script/k2recon.sh @save > recon_saveA.txt` (8 method, 5.156 B).

### S12 — Save-B (thread, PNG, TLG5)
- **Recon:** `ONLY="startSaveLayerImage cancelSaveLayerImage stopSaveLayerImage saveLayerImageTlg5 saveLayerImagePng saveLayerImagePngOctet" bash script/k2recon.sh @save 1 > recon_saveB.txt` (6 method, 2.428 B; **depth 1** karena pemanggilan encoder PNG/TLG5 dan thread ada di callee).
- **Catatan:** `WindowSaveImage` (`0x42dd34`, terdaftar di kelas Window) dan callback `onSaveLayerImageProgress/Done` (sisi TJS) tidak ada di peta; periksa terpisah. Bagian thread/NEON — hati-hati keamanan (race).

### S13 — Verifikasi diferensial (utamanya tugas Anda)
- **Anda:** jalankan skrip TJS 7.2 yang sama di **APK resmi 1.3.9** dan build fork; kumpulkan hash piksel & tangkapan layar; uji game nyata yang memakai plugin tersebut (7.4). Tentukan toleransi pembulatan di awal.
- **Claude:** menganalisis perbedaan, memutuskan keputusan terbuka: arah geser CPU/GPU `copyRaster`, tekstur sumber GPU `shrinkCopyFast`, waktu pemanggilan `stretchCopyAA`, ukuran tekstur GPU; menambah deviasi yang sah ke spesifikasi.
- **Selesai bila:** tiap method punya hasil banding (sama / beda diterima / beda diperbaiki).

### S14 — Survei sumber publik + util/data
- **Kerjakan:** 3.6 (`windowEx`, `scriptsEx`, `json`, `sqlite3`, `minizip`, `expat`: ada tidaknya source publik, jumlah file `.cpp`, **lisensi**); audit asal-usul 14 plugin bawaan; lalu plugin util/data (`json`, `sqlite3`, `minizip`, `expat`, `lzfs`, `fstat`, `util_*`, `fpackinone`) memakai source publik bila lisensinya cocok, rekonstruksi bila tidak.
- **Catatan:** `fetch_plugins.sh` perlu jaringan (dijalankan Anda; sandbox Claude tidak punya jaringan untuk bash). Hapus clone setelahnya (3.5, hindari *duplicate symbol*). Bukan nasihat hukum.

### S15 — Window/script · S16 — Media kecil + stub Windows
Daftar di bagian 1.3. Mulai dengan menentukan apa yang benar-benar dipanggil game (skrip game + log). Stub Windows (`drawdeviced3d*`, `kirikiroid2`) cukup berupa stub yang jujur dan terdokumentasi.

### S17 — `emoteplayer` / `psbfile`
Besar dan berformat proprietary; kerjakan terakhir. Prasyarat: **log `motionplayer_stub`** dari game nyata (apa yang dipanggil), agar cakupan ditentukan oleh pemakaian, bukan seluruh API. Kemungkinan perlu beberapa sesi.

### S18 — `layerExDraw` & `multiImage`
`layerExDraw` memakai GDI+ (readme: *member GdiPlus does not exist*). Keluaran: kajian kelayakan (lapisan kompatibilitas / alternatif Cairo/Skia/AGG / tidak dikerjakan) — bukan implementasi.

### S19 — Rilis & pemeliharaan
Tag rilis per kelompok plugin + changelog (8.3); periksa Java `classes.dex`/`KR2Activity` hanya bila ada beda perilaku (8.4); ulang parity 7.1 penuh; sinkronkan `readme.md`.

---

## 5. Keputusan yang saya perlukan dari Anda
1. **Lokasi build:** PC atau Termux `$HOME`? (menentukan S2.)
2. **Perangkat uji:** punya perangkat dengan jalur GPU dan APK resmi 1.3.9 terpasang? (menentukan S13.)
3. **Target ABI:** cukup `arm64-v8a`, atau `armeabi-v7a` juga? (armv7 menambah 6.3 dan 1.5/4.8.)
4. **Game acuan:** game mana yang dipakai? Daftar `Plugins.link(...)` di skripnya menentukan urutan S4–S12.
5. **Lisensi/distribusi:** apakah fork akan didistribusikan? (menentukan seberapa ketat audit di S14.)

---

## Catatan sesi log game (2026-11-23)
- Log `krkr_console.log` (UTF-16, 7 run, game Limelight Lemonade Jam) dibaca. Run lama gagal di `Motion` (stub), `Scripts.getObjectKeys`/`foreach` (kini native di core). Dua run terakhir berhenti di `keybinder.tjs`: `Dictionary.assignStruct incontextof SystemConfig.ShortCutInitialPadKeyMap` dengan nilai void. `incontextof void` melempar di TJS2 standar, jadi sesuatu yang seharusnya mendefinisikan `ShortCutInitialPadKeyMap` tidak termuat. **Penyebab belum terbukti**; butuh skrip game (`keybinder.tjs`, `override.tjs`, config `SystemConfig`) dan daftar `Plugins.link` game.
- `Kirikiroid2 dll list.csv` dari Anda: source publik ada untuk `scriptsEx`, `windowEx`, `json`, `sqlite3`, `minizip`, `expat`, `layerExImage/BTOA/Raster/Save/AreaAverage`, `shrinkCopy`, dll. (wtnbgo/krkrz). Ini mengoreksi catatan 3.4 bahwa hanya `LayerExBase` yang publik; **perlu diaudit di S14**.
- `scriptsEx` dari `github.com/wtnbgo/scriptsEx` sudah diadaptasi ke `src/plugins/scriptsEx.cpp` (lolos `g++ -fsyntax-only` dan `-c`; belum NDK/perangkat). `windowEx` berisi API Win32 (HWND, menu, IME), tidak bisa disalin begitu saja; belum dikerjakan.
