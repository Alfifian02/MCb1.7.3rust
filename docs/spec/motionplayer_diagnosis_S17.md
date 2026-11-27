# Diagnosis galat `Motion` di `krkr.console.log` (S17, 2026-11-25)

**Status:** analisis statis (log + bytecode skrip game + kode stub). Tidak ada kode yang dijalankan di mesin TJS2 repo maupun di perangkat. Hal yang tidak terbukti ditandai **ASUMSI**.
Berkas pendamping: `motionplayer_api_dipakai_game.md` (API yang dipakai game), `docs/plugins.md` (baris `motionplayer.dll`).

## 1. Sumber, alat, dan berkas yang kurang

Dibaca: `krkr.console.log` (UTF-16, game Limelight Lemonade Jam, 10 run), bytecode `system/{motion,affinesourcemotion,d3daffinesourcemotion,d3daffinesourceemote,initialize,mainwindow}.tjs`, `sysscn/{keybinder,override,afterinit,system}.tjs`, `main/{config,yuzu_default}.tjs`, `src/plugins/motionplayer_stub.cpp` dan `InternalPlugins.cpp` dari `Repo.zip`, daftar 60.526 berkas game (`daftar_berkas.tsv`).
Alat: `script/tjs2dis.py` (pembongkar bytecode TJS2100; versi S17 mencetak **nilai** konstanta numerik, versi lama hanya indeks tabel), `script/k2cari_pembuat.py`.

Kurang / tidak tersedia saat analisis:
- Dokumen S13-S16 dan catatan sesi S17 sebelumnya (kerja yang menghasilkan `zz_k2patch.xp3`, `k2tool.sh`, `xp3tool.py` tidak tercatat di dokumen yang saya terima; dasar dokumen ini adalah versi S12).
- `Repo.zip` setara S11 (belum memuat overlay S12). Tidak ada riwayat git di dalamnya, jadi versi stub yang dipakai tiap run log tidak bisa dipastikan.
- Mesin TJS2 yang bisa dijalankan; semua pernyataan perilaku runtime di bawah berasal dari pembacaan bytecode.

## 2. Kronologi 10 run di log

Jam tanpa tanggal; urutan = urutan di berkas. "File" = baris `Total N file(s) found`.

| Run | Jam awal galat | Galat pertama | Catatan |
|---|---|---|---|
| 1 | 05:36:14 | `affinesourcemotion.tjs` tingkat atas, VM ip 19 lalu ip 58 | `Motion` tidak ada. File 61209 |
| 2-4 | 06:15 / 06:15 / 06:32 | `voicemode.tjs` `MergeVoiceSystemAction`, ip 16 | `Scripts.getObjectKeys` (S12: kini native) |
| 5 | 06:56:00 | `kagenvironment.tjs` `Scripts_foreach`, ip 4 | `Scripts.foreach` (S12: kini native) |
| 6 | 07:10:07 | `override.tjs` `resetSysArgRadioMenuParent`, ip 60 | |
| 7-10 | 07:20 / 01:15 / 18:31 / 17:12 | `keybinder.tjs` fungsi anonim, ip 30 | `SystemConfig.ShortCutInitialPadKeyMap` = void |

Fakta terverifikasi dari log:
- Run 2-10 semua `61310 file found, 60363 activated`.
- **Run 7, 8, 9 identik baris demi baris** (selain waktu dan milidetik). Run 10 hanya berbeda 5 baris pemisah di awal. Jadi empat run berturut-turut gagal persis sama.
- `use:motionplayer.dll` tercetak (3x di run 1, 2x di run 2-10) -> cabang `motionplayer.dll` di `motion.tjs` terpilih dan `Plugins.link` tidak melempar.
- Baris berawalan `[motion]` (pencatat stub): **0 di semua run**.

## 3. Galat `Motion` di run 1

Bytecode `affinesourcemotion.tjs`, objek 0, dibongkar:
```
try   { Motion.enableD3D = (typeof Motion.Player.useD3D == "Object"); }     // ip 19..45: gpd Motion
catch { Motion.enableD3D = 0; }                                                 // ip 58: gpd Motion lagi
```
Kedua ip yang gagal (19 dan 58) sama-sama membaca global `Motion`; register hasilnya `void`. Karena blok `catch` juga memakai `Motion`, galat pertama tidak bisa "ditelan". Pesan galat tidak tercetak di log (hanya dump VM); "Member Motion does not exist" adalah tafsiran dari `gpd` yang gagal (**ASUMSI** ringan).

Penyebab (**ASUMSI**, didukung kode): komentar kepala `motionplayer_stub.cpp` menyatakan stub ini "menggantikan motionplayer_stub.cpp yang kosong". `TVPLoadInternalPlugin` (`InternalPlugins.cpp`) hanya memanggil `ncbAutoRegister::LoadModule` dan mengembalikan bool; modul yang ada tetapi tidak mendaftarkan apa pun tidak menghasilkan galat apa pun saat `Plugins.link`. Itu cocok dengan run 1: link sukses, `Motion` tidak ada. Dari run 2 galat ini hilang, konsisten dengan build yang sudah memuat stub bercatat. Bahwa run 1 memakai stub kosong tidak bisa dibuktikan tanpa riwayat build.

Saran (belum dikerjakan): beri peringatan di log bila sebuah modul internal berhasil dimuat tetapi tidak mendaftarkan kelas atau fungsi apa pun, supaya kasus seperti run 1 langsung terlihat.

Temuan sampingan: stub mendaftarkan `Motion.Player.useD3D`, sehingga `Motion.enableD3D = true`. Di konstruktor `AffineSourceMotion`, `_useD3D` hanya menyala bila `Motion.enableD3D` **dan** opsi `d3dMotion` (dari `kag`) sama-sama true; default `KAGWindow.d3dMotion` = 0 (bytecode `mainwindow.tjs`). Jadi jalur D3D tidak aktif otomatis; bila kelak dinyalakan, pertimbangkan membuang `useD3D` dari stub.

## 4. Kenapa tidak ada baris `[motion]`

Karena game tidak pernah sampai memakai `Motion`: run 7-10 berhenti di `keybinder.tjs`, sebelum skrip judul memuat `title_bg*.mtn`. Bukan bukti bahwa stub tidak bekerja; stub belum pernah dipanggil.

## 5. Penghalang saat ini: `keybinder.tjs` (run 7-10)

Terverifikasi dari bytecode:
1. `sysscn/system.tjs` objek 0 ip 1740 membuat `SystemConfig = new Dictionary(512)` **tanpa penjaga** (tidak ada lompatan yang melewatinya) lalu mengisi default (`askQLoad`, ...). Argumen 512 bertipe short; efeknya di fork tidak diperiksa (**ASUMSI**: diabaikan atau petunjuk ukuran). `system.tjs` juga mendefinisikan `SetSystemConfigDefaults`, yang hanya mengisi kunci yang masih `undefined`.
2. `sysscn/keybinder.tjs` memanggil `SetSystemConfigDefaults(...)`, mendaftarkan hook `onKAGSetupConfig` (`addKagHookCallback(..., kagcontext=1)`; fungsi ini mengisi `SystemConfig.ShortCutInitialPadKeyMap = new Dictionary()`), dan callback after-init (di-`chgthis` ke `global`, satu argumen) yang **membaca** `SystemConfig.ShortCutInitialPadKeyMap` lalu memanggil `Dictionary.assignStruct incontextof` padanya.
3. `override.tjs`: `KAGWindow_config_override` (dipanggil di konstruktor `KAGWindow` bila `isMain`) memindahkan `kagHookEntries` ke `addHook`, memanggil `callHook("onKAGSetupConfig", kag)`, lalu mengganti `addKagHookCallback` dengan fungsi "expired". `callAfterInit()` didefinisikan di sini.
4. `callAfterInit()` **dipanggil dari `sysscn/afterinit.tjs`** (pernyataan pertama yang dieksekusi); `system/initialize.tjs` memuat `AfterInit.tjs` setelah log "KAGMainWindow ... konstruktor". Cocok dengan urutan log.
5. `SystemConfig` dibuat hanya di `system.tjs` (pencarian `k2cari_pembuat.py` atas 247 bytecode + 50 teks di `out/`: 1 kecocokan). Kode C++ repo tidak menyebut `SystemConfig`.
6. Log tidak memuat pesan "were expired" -> pendaftaran hook keybinder tidak terlambat.
7. Urutan muat terlihat di run 2-4 (log verbose): `system.tjs` -> `yuzu_default.tjs` -> `default.tjs` -> `keybinder.tjs`, masing-masing sekali. Run 5-10 tidak mencatat muat per skrip, jadi urutannya di run yang gagal tidak teramati.

Belum terbukti: **mengapa** nilainya `void`. Kemungkinan: (a) hook `onKAGSetupConfig` keybinder tidak berjalan; (b) nilai terisi lalu hilang; (c) `SystemConfig` yang dibaca callback berbeda dari yang diisi hook (hook berjalan `incontextof kag`, callback `incontextof global`; tidak ditemukan penetapan `SystemConfig` di `mainwindow`/`override`). Patch lama `zz_k2patch.xp3` (menambah `.ShortCutInitialPadKeyMap = %[]` di `yuzu_default.tjs`) tidak mengubah apa pun yang terlihat di log; kapan ia dipasang tidak diketahui, dan tidak diketahui apakah `yuzu_default.tjs` pernah dieksekusi di run 5-10.

### Probe `[k2probe]` (BELUM diuji)
`tests/game/zz_k2patch/yuzu_default.tjs` (dan `tests/game/zz_k2patch.xp3` hasil `xp3tool pack`) = patch lama + blok diagnostik di **awal** berkas (berkas asli berakhir dengan `return` di dalam `with`, sehingga kode setelahnya tidak akan jalan). Blok ini: mencatat `[k2probe] ...` saat `yuzu_default` dimuat, saat hook `onKAGSetupConfig` (dini), dan saat after-init dini (sebelum callback keybinder, bila `yuzu_default` dimuat sebelum `keybinder`); juga mencatat keadaan `Motion`; dan membuat `ShortCutInitialPadKeyMap = %[]` bila saat after-init dini belum berupa Object. Pasang sebagai `zz_k2patch.xp3` di folder game, jalankan, lalu `grep "k2probe" krkr.console.log`:

| Hasil | Arti |
|---|---|
| tidak ada baris `[k2probe]` | patch tidak termuat atau `yuzu_default.tjs` tidak dieksekusi (cek nama/urutan XP3) |
| ada "yuzu_default dimuat", tidak ada "afterInit(dini)" | `addAfterInitCallback` belum ada saat itu atau callback tidak jalan |
| "afterInit(dini)" `SCIPKM=Object` tetapi keybinder tetap gagal | nilai hilang sesudahnya, atau callback keybinder membaca `SystemConfig` lain |
| keybinder lolos | perbaikan dini cukup; lanjut ke penghalang berikutnya (catat) |

## 6. Prediksi galat `Motion` berikutnya (dari bytecode; BELUM teramati di log)

Setelah keybinder lolos, bagian game yang memakai Motion akan memanggil (detail di `motionplayer_api_dipakai_game.md`): `new Motion.ResourceManager(host, cacheSize)` lalu `load(path)`, `unload(path)`, `clearCache()` pada instance; `new Motion.Player(rm)` / `new Motion.EmotePlayer(rm)`; `Motion.MaskModeAlpha`; dan 65 nama pada objek player. Stub S11 tidak punya `load`, `clearCache`, `EmotePlayer`, `MaskModeAlpha`, dan sebagian besar member player (`play`, `stop`, `progress`, `draw`, ...) -> akan muncul "Member ... does not exist" di titik pertama pemakaian. Revisi stub S17 menambahkannya sebagai pencatat (tetap tidak merender).

`motion.tjs` memberi prefiks `lzfs://./` pada path bila global `uselzfs` terdefinisi. Tidak ada berkas yang saya punya yang mendefinisikan `uselzfs` (hanya membaca), dan `libgame.so` tidak memuat string `lzfs://` (hanya 3 kemunculan `lzfs` UTF-16). **ASUMSI:** `uselzfs` tidak terdefinisi -> prefiks kosong -> aman. Jika ternyata terdefinisi, `load` akan gagal di lapisan storage (protokol `lzfs` belum ada; lihat `docs/plugins.md`, Util/data).

Merender sungguhan adalah pekerjaan besar yang terpisah (rencana 5.9): `envinit.tjs` memuat 617 baris yang menyebut `.mtn` (mis. `title_bg.mtn` untuk latar judul), dan data game berisi 44 `.mtn` (341,2 MiB), 151 `.psb` (188,5 MiB), 125 `.pimg` (120,7 MiB), 89 `.pbd` (3,0 MiB). Stub tidak akan menampilkan latar judul.

## 7. Yang masih dibutuhkan
- Hasil run dengan probe `[k2probe]` (satu `krkr.console.log` baru).
- Konfirmasi apakah `zz_k2patch.xp3` lama terpasang pada run 7-10 (dan di folder mana).
- Isi `appconfig.tjs` (1.084 byte, dimuat sebelum `Config.tjs` per log run 1) bila ingin menutup kemungkinan ia menyentuh `SystemConfig`.
- Dokumen S13-S16 / sesi S17 sebelumnya, agar catatan tidak terputus.
