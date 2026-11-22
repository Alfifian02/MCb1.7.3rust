# Kamus helper: alamat di `libgame.so` 1.3.9 (arm64) → fungsi di fork (langkah 4.7)

Dibuat di sesi S1, **diperluas di S8** (recon `MaskImageRideRect4/5`). Dipakai ulang oleh S3–S12 (BTOA, Image, Shimmer, Alpha, Save): sebelum menulis plugin baru, cocokkan helper yang
dipanggil recon dengan tabel ini, lalu **tambahkan baris baru** untuk helper yang belum ada.

## Cara membaca keyakinan

| Label | Arti |
|---|---|
| **[S1]** / **[S8]** | Diverifikasi di sesi S1 / S8: dari `recon_6023ec.txt`, atau dari **sumber fork yang dibaca langsung** (nomor baris disebut). |
| **[warisan]** | Peran helper berasal dari sesi sebelumnya (recon `0x60185c`, `0x5ffcf8`, `0x5e6bb0`). Recon itu **tidak diunggah ulang** di S1, jadi **tidak diverifikasi ulang**. |
| **ASUMSI** | Dugaan dari nama/peran; belum ada bukti baca. Jangan dijadikan dasar perilaku sebelum dibuktikan. |

## ⚠️ Daftar simbol ekspor tidak bisa menamai helper (temuan S1)

`libgame_symbol_lists.zip` (`symbols_addr.txt`, 26.422 simbol ekspor, format `alamat ukuran tipe nama`) diperiksa dengan pencarian
**rentang** untuk seluruh alamat di bawah, termasuk alamat implementasi `0x5e6bb0`, `0x5ffcf8`, `0x60185c`, `0x601d90`, `0x6023ec`.
**Tidak ada satu pun yang jatuh di dalam simbol ekspor**; semuanya ada di celah antar-simbol (kode non-ekspor, visibilitas tersembunyi).
API layer (`GetImageWidth`, `SetImageSize`, `GetMainImagePixelBuffer*`, `TVPIsSoftwareRenderManager`, dst.) juga tidak ada di ekspor
(hanya `GdipGetImageWidth/Height`, tak terkait).

Akibat praktis:
- Peran helper hanya bisa disimpulkan dari **perilaku** di recon, bukan dari nama simbol.
- Anotasi `<std::vector<...>::_M_default_append+0x...>` di keluaran `objdump`/recon (mis. pada target cabang `0x6025b4`) hanyalah
  **simbol ekspor terdekat sebelum alamat itu**, bukan nama fungsinya. Abaikan anotasi itu saat membaca recon.

## Tabel helper

Kolom *Cocok*: ✅ padanan jelas · ⚠️ ada beda perilaku/semantik yang harus diingat · ❓ padanan belum ditemukan.

| Alamat | Peran di binary | Padanan di fork | Cocok | Keyakinan & catatan |
|---|---|---|:---:|---|
| `0x806828` | lebar **gambar utama** layer | `tTJSNI_BaseLayer::GetImageWidth()` (`LayerIntf.h:450`; impl `LayerIntf.cpp:2336` = `MainImage->GetWidth()`) | ✅ | Peran **[warisan]**; padanan **[S1]**. **[S8]** pemakaian terkonfirmasi: nilainya dibandingkan dengan lebar cache `this` (`+0x10`) dan dicetak sebagai `DW:`/`AW:` di log ketidakcocokan → memang **lebar gambar utama**. Fork melempar `TVPNotDrawableLayerType` bila `MainImage` null; perilaku binary pada kasus itu tidak diketahui. |
| `0x806920` | tinggi gambar utama | `GetImageHeight()` (`LayerIntf.h:452`; `LayerIntf.cpp:2364`) | ✅ | Idem (`DH:`/`AH:`); **[S8]**. |
| — | *(bukan helper binary)* lebar/tinggi **rect layer** | `GetWidth()`/`GetHeight()` (`LayerIntf.h:388-389` = `Rect.get_width()/get_height()`) | ⚠️ | **[S1]** Berbeda dari `GetImageWidth/Height`. **`layerExBase_GL::reset()` (`layerExBase.hpp`) mengisi `_width/_height` dari `GetWidth()/GetHeight()`**, yaitu ukuran rect, bukan gambar utama. Untuk method yang di binary memakai `0x806828/0x806920`, **jangan** pakai `layerExBase_GL::_width/_height` begitu saja. |
| `0x80c848` | sinkronisasi gambar sebelum diakses | — | ❓ | **[warisan]**. Tidak ditemukan padanan eksplisit di API layer fork (`GetMainImagePixelBuffer*` langsung mengambil scanline, tanpa langkah sinkron terlihat). **ASUMSI** bahwa efeknya tertampung oleh `GetScanLine*` pada tekstur/bitmap. Putuskan saat ada recon yang bergantung padanya. |
| `0xa75340` | pointer tulis buffer utama | `GetMainImagePixelBufferForWrite()` (`LayerIntf.h:485`; `LayerIntf.cpp:2699`) | ⚠️ | Peran **[warisan]**; padanan **[S1]**. Fork **menyetel `ImageModified = true`** (`LayerIntf.cpp:2702`) sebelum mengembalikan `GetScanLineForWrite(0)`. Belum diketahui apakah `0xa75340` (level bitmap) menyetel flag yang sama. Dampak pada redraw: periksa di S2/S13. |
| — | pointer baca buffer sumber | `GetMainImagePixelBuffer()` (`LayerIntf.h:484`; `:2693`, const, tanpa flag) | ✅ | **[S1]** Dipakai `layerExShrink.cpp` untuk `src`. |
| `0xa7546c` | pitch buffer utama | `GetMainImagePixelBufferPitch()` (`LayerIntf.h:486`; `:2706` = `MainImage->GetPitchBytes()`) | ✅ | Peran **[warisan]**; padanan **[S1]**. Pitch dalam **byte**, bisa ≠ `lebar*4` (uji `shrink_fast` memakai pitch dengan padding). |
| `0x848bdc` | `IsSoftware()` | `TVPIsSoftwareRenderManager()` (`RenderManager.h:287`; `RenderManager.cpp:4402`) | ✅ | Peran **[warisan]**; padanan **[S1]**. Fork menyimpan hasil di variabel `static` pada panggilan pertama; selisih terlihat hanya bila render manager berganti saat runtime. |
| `0x848834` | `TVPGetRenderManager()` | `TVPGetRenderManager()` (`RenderManager.h:284`) | ✅ | Nama sama; peran **[warisan]**. |
| `0x807c00` | **pointer piksel** gambar utama (layer sumber, **hanya dibaca**) | `GetMainImagePixelBuffer()` (`LayerIntf.h:484`, `const void*`) | ✅ | **[S8]** Peran dari pemakaian: hasilnya dipakai sebagai basis alamat `+3` untuk membaca byte alfa/rgb piksel. ⚠️ Belum dibedakan dari varian `ForWrite` (kedua layer argumen hanya dibaca di Rect4/5, jadi varian baca yang paling mungkin). |
| `0x807c20` | **pitch** gambar utama (byte) | `GetMainImagePixelBufferPitch()` (`LayerIntf.h:486`) | ✅ | **[S8]** Hasilnya di-`sxtw` lalu ditambahkan ke pointer baris. Urutan alamat (`0x807c00` < `0x807c20`) konsisten dengan urutan deklarasi di fork. |
| `0x953d0c` | `TVPThrowExceptionMessage(const tjs_char*)` | idem | ✅ | **[S8]** Dipanggil dengan string `"Not Layer"` (`0x14c494e`) bila `NativeInstanceSupport` gagal — **sama** dengan pesan di ctor `layerExBase_GL` fork. |
| `0xa0a490` | `ttstr(int)` | `ttstr(tjs_int)` | ✅ | **[S8]** Mengubah lebar/tinggi menjadi string untuk log ketidakcocokan. |
| `0xa0b568`, `0xa11e9c`, `0xa11fc0`, `0xa11c90` | penyusun string TJS: `operator+(const tjs_char*, const ttstr&)`, penggabung dua `tjs_char*`, alokasi `tTJSVariantString` dari literal, ambil `tjs_char*` | operator `+`/`ttstr` di `tjsString.h` | ❓ | **[S8] ASUMSI** (dari pola pemanggilan; tak mempengaruhi hasil piksel). |
| `0xa16ca4` | mencatat log | `TVPAddLog(const ttstr&)` (`DebugIntf.h:23`) | ❓ | **[S8]** Dipanggil 3× berturut-turut lalu alur **berlanjut** ke epilog → bukan fungsi yang melempar. ⚠️ Identifikasi sebagai `TVPAddLog` adalah ASUMSI dari perilaku (tidak melempar, menerima `ttstr&`). |
| `0xa11b74` | `tTJSVariantString::Release()` | — | ❓ | **[S8]** 36×/fungsi, selalu setelah pemakaian string; **ASUMSI** dari pola refcount (`ldaxr/stlxr`). |
| `0x406260`, `0x4228b0`, `0x415b10`, `0x407130`, `0x41ad30` | PLT: `vector<float>::_M_default_append`, `vector<int>::_M_default_append`, `operator delete`, `__stack_chk_fail`, `_Unwind_Resume` | `std::vector<float>/<int>`, dll. | ✅ | **[S8]** Nama diambil dari **anotasi PLT di recon** (`…@plt`), yang berbeda dari label "simbol ekspor terdekat" dan **dapat dipercaya**. |
| `0x52138c` | tampaknya thunk `terminate` pada *landing pad* | — | ❓ | **[S8] ASUMSI:** 36 panggilan berderet di ekor fungsi (setelah `__stack_chk_fail`), tidak pernah di jalur utama. |
| `0x806960` | set ukuran gambar utama **hanya bila beda** | `tTJSNI_BaseLayer::SetImageSize(w,h)` (`LayerIntf.h:457`; `LayerIntf.cpp:2396`) | ✅ | **[S1]** Fork juga langsung `return` bila ukuran sama, lalu `InternalSetImageSize`. Cocok dengan perilaku yang tercatat di spesifikasi `shrinkCopyFast`. |
| `0xa74f34` | ubah ukuran gambar utama (level bitmap, dipakai jalur GPU) | `InternalSetImageSize` (`LayerIntf.h:455`; `:2370`) | ❓ | **ASUMSI** dari nama & urutan panggilan; fork memanggil `SetImageSize`, yang meneruskan ke `InternalSetImageSize`. |
| `0xa74e90` / `0xa75020` | lebar / tinggi bitmap (`[bitmap+0x58]+0xc/+0x10`) | `MainImage->GetWidth()/GetHeight()` (dipakai `GetImageWidth`, `LayerIntf.cpp:2339`) | ❓ | **ASUMSI**: peran level bitmap/tekstur; tidak dipakai jalur CPU `shrinkCopyFast`. |
| `0xa0cd8c`, `0xa11b94`, `0x9b07c8` | konversi `tTJSVariant` (parameter TJS → integer/objek) | `tTJSVariant::AsInteger()`, `AsObjectClosureNoAddRef()` (`tjsVariant.h`) | ✅ (semantik) | **[S1]** semantik `AsInteger()` fork: `void`→0, string→`ToInteger()`, integer apa adanya, **real dipotong ke nol** (`(tTVInteger)Real`), `object`/`octet`→ melempar. Sama dengan yang tercatat di spesifikasi. ⚠️ Pemetaan **alamat mana = fungsi mana** tidak dipastikan di S1. ⚠️ `AsInteger()` fork 64-bit, lalu `layerExShrink.cpp` memotong ke `int`; apakah binary membaca 32- atau 64-bit **belum diketahui**. |
| `0x41f9e0` / `0x426f20` | PLT: dugaan alokasi/pembebasan buffer sementara | `std::vector<uint8_t> tmp` di `shrink_fast` | ❓ | **ASUMSI** (dugaan `malloc`/`free` dari spesifikasi lama). Fork mengalokasikan `min(vs,sh)` baris, hasil identik. |
| `0x6023ec` | **`hrow`**: pengecil baris horizontal | `layerex_shrink::hrow()` (`layerExShrinkFastCore.hpp`) | ✅ | **[S1] terbaca penuh + diverifikasi emulasi.** Fungsi **daun**: 0 `bl`/`blr`, 0 PLT, tidak memanggil helper mana pun. Lihat spesifikasi `layerExShrinkFast_shrinkCopyFast.md`. |
| `0x601d90` | worker CPU `shrinkCopyFast` | `layerex_shrink::shrink_fast()` | ✅ | **[warisan]**; kode diuji di inti-host. |
| `0x60185c` | implementasi method `shrinkCopyFast` | `ShrinkCopyFast` di `layerExShrink.cpp` | ✅ | **[warisan]**. |

## Cocokkan dengan `layerExBase.hpp` (hasil 4.7)

`layerExBase.hpp` punya dua dasar, dan **`layerExShrink.cpp` tidak memakai keduanya** (memanggil API `tTJSNI_Layer` langsung):

| Dasar | Cara akses | Cocok untuk | Catatan |
|---|---|---|---|
| `layerExBase` | properti TJS lewat `ObjectCache` (`imageWidth`, `imageHeight`, `update`, `clip*`); buffer **dikomentari** | plugin gaya `layerExMovie`/`Perspective` lama | Lambat (setiap akses = panggilan properti TJS). `_pWidth/_pHeight` = `imageWidth/imageHeight` → **ukuran gambar utama** (sesuai binary). |
| `layerExBase_GL` | `tTJSNI_Layer*` langsung; buffer/pitch hanya bila `TVPIsSoftwareRenderManager()` | kerangka plugin native baru | ⚠️ `_width/_height` dari **`GetWidth()/GetHeight()` (rect)**, bukan ukuran gambar utama; `_buffer`/`_pitch` = `…ForWrite`/`…Pitch` (menyetel `ImageModified`); `redraw()` memanggil `Update(rc)` dengan rect clip. |

Rekomendasi untuk S3+ (belum ditegakkan di kode; **ASUMSI** sampai recon tiap method dibaca):
1. Ambil dimensi dari `GetImageWidth()/GetImageHeight()` kecuali recon menunjukkan helper rect layer.
2. Untuk gambar *province* (kemungkinan dipakai `copyAlphaToProvince`/`fillByProvince`, nama menyiratkan): fork **sudah punya**
   `GetProvinceImagePixelBuffer()` (`:2712`), `…ForWrite()` (`:2718`, **mengalokasikan** gambar province bila belum ada dan menyetel
   `ImageModified`), `…Pitch()` (`:2725`). Cocokkan dengan helper di recon BTOA di S3.
3. Beda yang harus diputuskan per method dari recon: apakah binary memanggil `Update()` (di `shrinkCopy*` **tidak**, di `copyRaster` fork memanggilnya).
4. Clip: `GetClipLeft/Top/Width/Height()` ada di fork (`LayerIntf.h:714+`); padanan helper binary untuk `clipAlphaRect` belum diketahui.

## Temuan S8: konteks `layerExAlpha` = `layerExBase_GL` (tata letak) ✅

Dua fungsi `MaskImageRideRect4/5` membaca konteks (`x0`) pada offset yang **persis** sama dengan anggota `layerExBase_GL` di fork:
`+0x00` vptr · `+0x08` `_this` · `+0x10` `_width` · `+0x14` `_height` · `+0x18` `_buffer` · `+0x20` `_pitch`, dan satu panggilan virtual `[vtable+8]` setelah loop
(= `redraw()`; slot 0 = `reset()`). Konsekuensi: **kelas plugin Alpha di fork cukup diturunkan dari `layerExBase_GL`** (dikerjakan di `layerExAlpha.cpp`).
Catatan hati-hati yang tetap berlaku (lihat tabel di atas): `_width/_height` diisi dari `GetWidth()/GetHeight()` (rect), sedangkan dimensi kedua layer argumen berasal dari
`GetImageWidth/Height()`; keduanya dibandingkan apa adanya (beda → log `SW/SH` vs `DW/DH`/`AW/AH`, tanpa efek).

⚠️ **Siapa yang menyegarkan cache ctx?** Method Rect4/5 **tidak** memanggil `reset()`. Di pola layerEx upstream, *instance-getter hook* ncbind memanggil `reset()` pada **setiap** pemanggilan method; fork meniru itu (`layerExAlpha.cpp`). Hook binary ada di fungsi registrasi `0x5d43b0` (1.832 B, **belum dibaca**) → **ASUMSI** sampai direcon (`bash script/k2recon.sh 0x5d43b0 0 > recon_5d43b0.txt`).

## Yang belum bisa dipastikan (jadi tugas sesi lain)

| Item | Kenapa belum | Dipastikan di |
|---|---|---|
| Peran `0x80c848` (sinkron) | tidak ada padanan eksplisit di fork; recon `0x60185c` tidak diunggah ulang | saat recon yang memanggilnya dibaca ulang (mis. S2/S13) |
| `ImageModified` pada jalur tulis binary vs fork | butuh baca `0xa75340` | S13 (diferensial) |
| 32- vs 64-bit pada konversi `AsInteger` | butuh baca konversi `0xa0cd8c`/`0xa11b94`/`0x9b07c8` | bila ada game dengan argumen besar; rendah prioritas |
| Pemetaan alamat↔fungsi konversi `tTJSVariant` | idem | idem |
| Hook/ctor `layerExAlpha` (siapa memanggil `reset()`), nama parameter method | butuh recon registrasi `0x5d43b0` | sesi berikut yang memerlukan (mis. S6 saat merangkum pola bersama Alpha) |
| Varian `GetMainImagePixelBuffer` vs `…ForWrite` pada `0x807c00` | hanya dipakai sebagai sumber baca di Rect4/5 | bila ada method Alpha yang menulis ke layer argumen |
