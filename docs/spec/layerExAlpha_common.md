# layerExAlpha — hal umum (dibaca di S9 dari 3 fungsi; berlaku untuk method Alpha lain bila pola sama)

Sumber: `recon_alpha4.txt` (disassembly `libgame.so` 1.3.9 arm64, depth 0, 3 fungsi). Tingkat bukti: **baca disassembly oleh Claude** (tanpa disassembler otomatis
di sandbox, tanpa emulasi); kode piksel **tidak dibandingkan dengan biner** (7.3). Nama modul `layerExAlpha.dll` @`0x14c9f5c` terverifikasi dari string UTF-16.

## Bentuk fungsi (impl)
Impl adalah **fungsi anggota**: `x0 = this`, argumen Layer sebagai `iTJSDispatch2*` (x1, x2), int di w3/w4. Tidak ada `result/numparams/param` — konversi argumen TJS dilakukan
pembungkus ncbind yang tidak terbaca (depth 0). **ASUMSI:** pembungkus melempar `TJS_E_BADPARAMCOUNT` bila argumen kurang; argumen void/null menjadi pointer null (impl lalu crash).

## Tata letak `this` (cocok dengan `layerExBase_GL` di `src/plugins/layerExBase.hpp`)
| offset | isi | dipakai |
|---|---|---|
| `+0x00` | vptr | slot `+0x8` dipanggil di akhir = **redraw()** (ASUMSI; slot `+0x0` = `reset()`) |
| `+0x10` | `_width` (int) | lebar layer ini |
| `+0x14` | `_height` (int) | tinggi layer ini |
| `+0x18` | `_buffer` (uchar*) | buffer tulis gambar utama = **tujuan** |
| `+0x20` | `_pitch` (int, byte) | pitch tujuan |
Tidak ada pemanggilan `reset()` di dalam impl → **ASUMSI:** `reset()` dipanggil oleh pengait instans ncbind sebelum impl (sehingga `_width/_height/_buffer/_pitch` segar).
`layerExBase_GL::reset()` di fork mengisi `_buffer/_pitch` **hanya jika `TVPIsSoftwareRenderManager()`**; bila binary sama, pada renderer GPU `_buffer` = null dan ketiga method crash (**ASUMSI, tidak teruji**).

## Helper yang dipanggil (alamat → arti, dari pola pakai)
| alamat | arti | alasan |
|---|---|---|
| vtable obj `+0xc8` dengan (`2`, `*GOT[0x1aa0bd8]`, &out) | `NativeInstanceSupport(TJS_NIS_GETINSTANCE, tTJSNC_Layer::ClassID, &ni)` | hasil `<0` → galat |
| `0x953d0c("Not Layer")` | `TVPThrowExceptionMessage(const tjs_char*)`; teks @`0x14c494e` | dipanggil bila argumen bukan Layer |
| `0x806828` / `0x806920` | lebar / tinggi **gambar utama** argumen (`GetImageWidth/Height`) | dibandingkan dengan `this._width/_height` |
| `0x807c00` | `GetMainImagePixelBuffer()` (pointer baca) | dipakai sebagai pointer piksel |
| `0x807c20` | `GetMainImagePixelBufferPitch()` | dipakai sebagai pitch (`sxtw`) |
| `0xa0a490(ttstr*, int)` | `ttstr` dari int | pesan ukuran |
| `0xa0b568`, `0xa11e9c`, `0xa11fc0`, `0xa11c90`, `0xa11b74` | `operator+` / salin / `c_str` / lepas-ref `ttstr` | perakitan pesan |
| `0xa16ca4(ttstr*)` | **log (TVPAddLog)** — **ASUMSI** | tiga panggilan berurutan lalu kembali tanpa redraw; pelempar tidak masuk akal di sini |
| `0x406260(vec*, n)` | `vector<float>::resize(n)` (hanya Rect7) | |
| `0x415b10` | `operator delete` | |
Semua alamat ini tanpa nama simbol di `.dynsym` (diperiksa) → identifikasi berdasar pola, bukan nama.

## Pemeriksaan ukuran dan pesan log
- Argumen bukan Layer → `TVPThrowExceptionMessage("Not Layer")` (urutan: argumen 1 dulu, lalu 2).
- `this._width == arg._imageWidth` dan `this._height == arg._imageHeight` untuk **setiap** argumen Layer (Rect7/Rect8). Bila tidak cocok: **log, kembali, tanpa menggambar dan tanpa redraw**.
  Perhatikan: `this._width` adalah ukuran **layer** (`GetWidth`), argumen memakai ukuran **gambar**; biasanya sama.
- Teks log (UTF-16, @`0x14ca40e…`): baris 1 `SW:<this.w> SH:<this.h>`, baris 2 `DW:<arg1.w> DH:<arg1.h>`, baris 3 `AW:<arg2.w> AH:<arg2.h>`.
  Dalam urutan: this = "S", argumen 1 = "D", argumen 2 (mask) = "A". `AlphaBorderInnerRect` hanya baris 1 dan `AW:/AH:` untuk satu-satunya argumen.
- Pemeriksaan hanya **lebar** dilakukan lebih dulu pada `AlphaBorderInnerRect` (lihat spesifikasinya).

## Akhir
Bila lolos: loop piksel → `redraw()` (slot vptr `+0x8`, ASUMSI = `Update(clip rect)` seperti `layerExBase_GL::redraw`). Loop dilewati bila tinggi `< 1` (redraw tetap dipanggil); loop dalam dilewati bila lebar `< 1`.
Tidak ada sinkronisasi/`0x80c848` dan tidak ada pemeriksaan clip rect: seluruh `lebar x tinggi` diproses.

## Konvensi piksel
32 bit B,G,R,A; alfa pada offset `+3` (`add x,x,#3` lalu `ldrb`). Arah baris naik (pitch ditambah per baris, boleh negatif).
