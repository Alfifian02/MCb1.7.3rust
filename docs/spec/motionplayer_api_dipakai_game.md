# API `Motion` yang dipakai game vs stub `motionplayer.dll` (S17, 2026-11-25)

**Status:** diturunkan dari **bytecode skrip game**, bukan dari log runtime (log belum pernah mencapai `Motion`; lihat `motionplayer_diagnosis_S17.md`). Pelacakan nama pada objek memakai register di bytecode (`script/tjs2dis.py`) dan bersifat **heuristik**: register dipakai ulang, jadi sebagian nama bisa milik objek lain (**ASUMSI** untuk setiap baris "player" di bawah). Perilaku sebenarnya tiap member (argumen, nilai kembali) **tidak diketahui**; stub hanya mencatat panggilan dan mengembalikan void.

Sumber: `system/motion.tjs`, `system/affinesourcemotion.tjs`, `system/d3daffinesourcemotion.tjs`, `system/d3daffinesourceemote.tjs`, `system/mainwindow.tjs`, `system/initialize.tjs`.

## 1. Alur pemuatan (`system/motion.tjs` dan `initialize.tjs`)

1. `CanLoadPlugin(nama)` (di `initialize.tjs`): benar bila DLL **tidak** ada sebagai berkas lepas di `exePath`, `exePath/plugin/` atau `exePath/system/` **dan** `Storages.getPlacedPath(nama)` menemukannya (dibaca dari bytecode; di fork perilakunya perlu dicek di kode, **ASUMSI**). Daftar berkas game tidak memuat satu pun `.dll`, tetapi log mencetak `use:motionplayer.dll`; jadi di fork `CanLoadPlugin("motionplayer.dll")` bernilai true oleh sebab yang belum dipastikan (**terbuka**).
2. Urutan pilihan: `emoteplayer.dll` -> `motionplayer.dll` (bila `-nod3dm` != `yes`; `Plugins.link`, lalu `Debug.notice("use:" + nama)`; bila melempar dan `motionplayer_nod3d.dll` bisa dimuat -> coba itu, bila tidak lempar ulang) -> `motionplayer_nod3d.dll`.
3. `Scripts.execStorage("AffineSourceMotion.tjs")`. Awal skrip itu: `Motion.enableD3D = (typeof Motion.Player.useD3D == "Object")`; pada galat `Motion.enableD3D = 0`. Lalu memetakan ekstensi `.PSB` dan `.MTN` ke kelas `AffineSourceMotion` (`extSourceMap`).
4. `_useD3D` di `AffineSourceMotion` benar hanya bila `Motion.enableD3D` dan `kag.d3dMotion` (default `KAGWindow.d3dMotion` = 0) sama-sama benar.

## 2. `Motion` (global, dari plugin)

| Nama | Dipakai sebagai | Status stub S17 |
|---|---|---|
| `Motion` | objek global | terdaftar sejak stub bercatat (run 1 gagal: tidak ada) |
| `Motion.Player` | kelas; `new Motion.Player(resourceManager)` | ada |
| `Motion.EmotePlayer` | kelas; `new Motion.EmotePlayer(resourceManager)` bila `_storageType == "emote"`, lalu `maskMode = Motion.MaskModeAlpha` | **DITAMBAH S17** (anggota = Player) |
| `Motion.MaskModeAlpha` | konstanta dibaca | **DITAMBAH S17**, NILAI PLACEHOLDER (1) |
| `Motion.PlayFlagForce` | konstanta dibaca | sudah ada |
| `Motion.enableD3D` | **ditulis oleh skrip** (bukan plugin) | n/a |
| `Motion.Player.useD3D` | dicek keberadaannya (`typeof == "Object"`) | ada -> `enableD3D = true` |
| `Motion.ResourceManager` | kelas | ada |
| `Motion.D3DAdaptor`, `Motion.SeparateLayerAdaptor` | dibaca di jalur D3D / `mainwindow` | sudah ada (jalur D3D off secara default) |
| `D3DEmotePlayer` | **kelas global** (bukan `Motion.`), dipakai `d3daffinesourceemote.tjs` | tidak ada; hanya jalur D3D, di luar cakupan stub |

## 3. `Motion.ResourceManager` (dibungkus `MotionResourceManager` di `motion.tjs`)

- Konstruktor skrip: `new Motion.ResourceManager(opsi, cacheSize)`. `opsi` = argumen yang diterima `MotionResourceManager` (harus bisa diindeks: skrip membaca `opsi["motionCacheSize"]`); `cacheSize` = `opsi.motionCacheSize` bila ada, jika tidak **20971520** (20 MiB). Nilai ini diverifikasi dari bytecode.
- Prefiks path: `lzfs://./` bila global `uselzfs` terdefinisi, jika tidak string kosong (lihat diagnosis bagian 6).
- Instance dipanggil: `load(prefix + path)` (nilai kembali diteruskan ke pemanggil; skrip menghitung `pathRefCount[path]`), `unload(prefix + path)` (dipanggil saat hitungan mencapai 0), `clearCache()`. Stub S17: `load` dan `clearCache` **DITAMBAH**; `unload` sudah ada (terdaftar static).

## 4. Anggota objek player (`_player`)

Total nama terlacak: 65. Kolom status membandingkan dengan stub S11 (sebelum revisi) dan stub S17 (`motionplayer_stub.cpp`). Pola panggilan: sebagian dipanggil lewat `calld`, sebagian lewat `gpd` lalu `call` (diperiksa terpisah); tiga nama `count*Timelines` dan lima nama `*At` / `getTimelineBlendRatio` ternyata **method**, bukan properti.

| Nama | Jenis | Status stub |
|---|---|---|
| `allplaying` | properti (dibaca) | sudah ada (S11) |
| `animating` | properti (dibaca) | DITAMBAH S17 |
| `bustScale` | diset game | DITAMBAH S17 |
| `chara` | properti (dibaca) | DITAMBAH S17 |
| `clear` | method | tidak ditambah (lihat catatan) |
| `clone` | method | tidak ditambah (lihat catatan) |
| `colorWeight` | diset game | DITAMBAH S17 |
| `completionType` | diset game | DITAMBAH S17 |
| `contains` | method | tidak ditambah (lihat catatan) |
| `countDiffTimelines` | method | DITAMBAH S17 |
| `countMainTimelines` | method | DITAMBAH S17 |
| `countPlayingTimelines` | method | DITAMBAH S17 |
| `draw` | method | DITAMBAH S17 |
| `fadeOutTimeline` | method | DITAMBAH S17 |
| `getDiffTimelineLabelAt` | method | DITAMBAH S17 |
| `getDiffTimelineLabelList` | method | sudah ada (S11) |
| `getLoopTimeline` | method | sudah ada (S11) |
| `getMainTimelineLabelAt` | method | DITAMBAH S17 |
| `getMainTimelineLabelList` | method | sudah ada (S11) |
| `getPlayingTimelineFlagsAt` | method | DITAMBAH S17 |
| `getPlayingTimelineInfoList` | method | sudah ada (S11) |
| `getPlayingTimelineLabelAt` | method | DITAMBAH S17 |
| `getTimelineBlendRatio` | method | DITAMBAH S17 |
| `getTimelinePlaying` | method | sudah ada (S11) |
| `getVariable` | method | DITAMBAH S17 |
| `getVariableFrameList` | method | sudah ada (S11) |
| `hairScale` | diset game | DITAMBAH S17 |
| `initPhysics` | method | DITAMBAH S17 |
| `isTimelinePlaying` | method | DITAMBAH S17 |
| `load` | method | tidak ditambah (lihat catatan) |
| `loopTime` | properti (dibaca) | DITAMBAH S17 |
| `maskMode` | diset game | DITAMBAH S17 |
| `meshDivisionRatio` | diset game | DITAMBAH S17 |
| `motion` | properti (dibaca) | tidak ditambah (lihat catatan) |
| `onAction` | diset game | tidak ditambah (lihat catatan) |
| `outline` | properti (dibaca) | DITAMBAH S17 |
| `partsScale` | diset game | DITAMBAH S17 |
| `pass` | method | tidak ditambah (lihat catatan) |
| `play` | method | DITAMBAH S17 |
| `playTimeline` | method | DITAMBAH S17 |
| `playing` | properti (dibaca) | DITAMBAH S17 |
| `progress` | method | DITAMBAH S17 |
| `scale` | properti (dibaca) | DITAMBAH S17 |
| `serialize` | method | DITAMBAH S17 |
| `setChara` | method | tidak ditambah (lihat catatan) |
| `setColor` | method | DITAMBAH S17 |
| `setCoord` | method | DITAMBAH S17 |
| `setDrawAffineTranslateMatrix` | method | sudah ada (S11) |
| `setRot` | method | DITAMBAH S17 |
| `setRotate` | method | DITAMBAH S17 |
| `setScale` | method | DITAMBAH S17 |
| `setTimelineBlendRatio` | method | DITAMBAH S17 |
| `setVariable` | method | DITAMBAH S17 |
| `show` | method | tidak ditambah (lihat catatan) |
| `skip` | method | DITAMBAH S17 |
| `skipToSync` | method | sudah ada (S11) |
| `smoothing` | diset game | DITAMBAH S17 |
| `speed` | properti (dibaca) | DITAMBAH S17 |
| `startWind` | method | DITAMBAH S17 |
| `stop` | method | DITAMBAH S17 |
| `stopTimeline` | method | DITAMBAH S17 |
| `tags` | properti (dibaca) | DITAMBAH S17 |
| `tickCount` | properti (dibaca) | sudah ada (S11) |
| `unserialize` | method | DITAMBAH S17 |
| `variableKeys` | properti (dibaca) | sudah ada (S11) |

Catatan:
- Nama berstatus "tidak ditambah" (`clear`, `clone`, `contains`, `load`, `motion`, `onAction`, `pass`, `setChara`, `show`): kemungkinan besar milik objek skrip lain atau diisi oleh game; **ASUMSI**, tidak ditelusuri satu per satu. `onAction` diset oleh game (callback dari native ke skrip; stub tidak pernah memanggilnya).
- Properti yang hanya **diset** game (`bustScale`, `hairScale`, `partsScale`, `meshDivisionRatio`, `outline`, `smoothing`, `colorWeight`, `completionType`, `maskMode`) didaftarkan sebagai properti sederhana agar pembacaan balik tidak gagal; stub S11 menyediakan `setBustScale()` dst. sebagai method, yang tidak dipakai game dengan nama itu (**ASUMSI**).
- Tidak ada makna perilaku yang diketahui untuk `play`, `progress`, `draw`, `serialize`, ...; semuanya hanya dicatat sebagai `[motion] Motion.Player.<nama>(...)` ke log aplikasi (jumlah per member dibatasi di sisi stub).

## 5. Data yang akan dibaca

`.mtn` (44 berkas, 341,2 MiB), `.psb` (151, 188,5 MiB), `.pimg` (125, 120,7 MiB), `.pbd` (89, 3,0 MiB). `envinit.tjs` memuat 617 baris yang menyebut `.mtn` (mis. `title_bg*.mtn`). Format dan renderer **belum dianalisis**; rencana 5.9.

## 6. Uji

`tests/tjs/t_motionplayer_stub.tjs` memeriksa keberadaan anggota di atas dan mereproduksi pemeriksaan awal `affinesourcemotion.tjs`. **Belum dijalankan.**
