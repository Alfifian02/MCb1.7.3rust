# layerExBTOA — spesifikasi modul (berlaku untuk keenam method)

- Modul: **`layerExBTOA.dll`** — string UTF-16 @`0x14c5b72` (terverifikasi dari `libgame.so`; huruf besar-kecil persis seperti ini).
- Sumber: rekonstruksi dari `libgame.so` arm64 1.3.9. Kode: `src/plugins/layerExBTOA.cpp` + `layerExBTOACore.hpp`; uji: `tests/plugins/layerExBTOA/`.
- Registrasi gaya B (`NAME`→`CODE`, thunk 28 B) lalu implementasi; semua method ditambahkan ke kelas `Layer` (`PropSet` flag `TJS_MEMBERENSURE`).

| Method | String nama | Implementasi | Ukuran | Spesifikasi |
|---|---|---|---:|---|
| `copyRightBlueToLeftAlpha` | `0x14ca614` | `0x5e16e8` | 596 B | [`layerExBTOA_copyRightBlueToLeftAlpha.md`](layerExBTOA_copyRightBlueToLeftAlpha.md) |
| `copyBottomBlueToTopAlpha` | `0x14ca66e` | `0x5e1b58` | 600 B | [`layerExBTOA_copyBottomBlueToTopAlpha.md`](layerExBTOA_copyBottomBlueToTopAlpha.md) |
| `fillAlpha` | `0x14ca6a0` | `0x5e1e34` | 188 B | [`layerExBTOA_fillAlpha.md`](layerExBTOA_fillAlpha.md) |
| `copyAlphaToProvince` | `0x14ca6b4` | `0x5e1f74` | 1.356 B | [`layerExBTOA_copyAlphaToProvince.md`](layerExBTOA_copyAlphaToProvince.md) |
| `clipAlphaRect` | `0x14ca76c` | `0x5e2544` | 3.040 B | [`layerExBTOA_clipAlphaRect.md`](layerExBTOA_clipAlphaRect.md) |
| `fillByProvince` | `0x14ca788` | `0x5e31a8` | 1.536 B | [`layerExBTOA_fillByProvince.md`](layerExBTOA_fillByProvince.md) |

## Konvensi bersama
- Semua method **tidak pernah menulis `result`** dan mengembalikan `TJS_S_OK`, kecuali yang disebut di spesifikasi masing-masing
  (`TJS_E_BADPARAMCOUNT` -1004, `TJS_E_INVALIDPARAM` -1003 — nilai diverifikasi dari kode kembali biner).
- Argumen integer dibaca sebagai `tTVInteger` (int64) lalu **dipotong ke 32 bit** (register `w`). Konversi memakai aturan `tTJSVariant`:
  void→0, integer→nilai, real→dipotong ke nol, string→di-parse, **objek/oktet → galat konversi** (`0xa0cd8c(var, 4)`; di fork
  `tTJSVariant::AsInteger()` melempar `TJSThrowVariantConvertError` yang sama).
- Instance Layer diambil dengan `objthis->NativeInstanceSupport(TJS_NIS_GETINSTANCE, tTJSNC_Layer::ClassID, &p)` (vtable +0xc8).
  Pointer keluaran **tidak diinisialisasi** sebelum panggilan (biner hanya memeriksa pointer, bukan kode kembali).
  Gagal → `TVPThrowExceptionMessage(L"dest must be Layer.")` @`0x14ca646` (terverifikasi emulasi untuk 4 method).
- Sebagian method memilih jalur dengan `IsSoftware()` (`0x848bdc`); `clipAlphaRect`, `copyAlphaToProvince`, `fillAlpha` tidak.

## Urutan kanal piksel (temuan)
- Biner menyalin **byte[2]** piksel (disebut "Blue") ke **byte[3]** (alfa); shader GPU `CopyAlphaFromBlue` memakai `s.a = s.b`.
  `fillByProvince` menukar R dan B pada warna argumen (`0xAARRGGBB → 0xAABBGGRR`) sebelum menulis 32-bit.
- Ekspresi penukar itu **identik dengan `TVP_REVRGB`** di fork (`LayerBitmapIntf.h:21`). Jadi buffer CPU layer berurutan **R,G,B,A**
  dalam memori (byte[2] = biru) di 1.3.9 maupun fork. `layerExBTOA.cpp` memuat `static_assert(swap_rb == TVP_REVRGB)`.

## Peta helper (alamat 1.3.9 → padanan di fork) — menjawab 4.7 untuk BTOA
| Alamat | Fungsi | Padanan di fork |
|---|---|---|
| `0x848bdc` | `IsSoftware()` | `TVPIsSoftwareRenderManager()` |
| `0x848834` | `TVPGetRenderManager()` | idem |
| `0x848454` | `GetOrCompileRenderMethod(mgr, nama, glsl, nTex, flags)` | `iTVPRenderManager::GetOrCompileRenderMethod(nama, &hint, glsl, nTex, flags)` (fork menambah `hint`) |
| `0x806828` / `0x806920` | lebar / tinggi gambar | `GetImageWidth()` / `GetImageHeight()` |
| `0x806358` | `MainImage != null` | `GetMainImage() != nullptr` |
| `0x807bec` / `0x807c20` | buffer baca / pitch gambar utama | `GetMainImagePixelBuffer()` / `GetMainImagePixelBufferPitch()` |
| `0x807c30` / `0x807c80` | buffer baca / pitch province (null bila tidak ada) | `GetProvinceImagePixelBuffer()` / `GetProvinceImagePixelBufferPitch()` |
| `0x807c44` | alokasi province bila perlu + `ImageModified`(+0x199) + buffer tulis | `GetProvinceImagePixelBufferForWrite()` (persis sama) |
| `0x807818` | `Update(layer, &rect, tempupdate)` | `Update(const tTVPRect&, bool)` |
| `0x7f84b0` | `FillMask(rect_by_value, value)` → render method `"FillMask"`, param `opacity` | `iTVPBaseBitmap::FillMask(tTVPRect, tjs_int)` |
| `0x80c848` | sinkronisasi (flag +0x1c0, rect +0x1ac → `MainImage->…(rect)` @`0xa759ac`) | **tidak ada padanan** (ASUMSI: tidak perlu di fork) |
| `0x5e193c` | helper lokal "rowsOf(obj,&w,&h,flag)" → larik pointer baris (`new[]`, di-`delete[]` pemanggil). `flag=1`: `GetTextureForRender(false,nullptr)` + `GetScanLineForWrite(0)` + `GetPitch()`; `flag=0`: `MainImage->Texture(+0x58)->GetScanLineForRead(i)`. Gagal ambil instance → `TVPSpecifyLayer` ("Specify Layer class object", string @`0x15186a8`) | `OpenDstPixels()` di `layerExBTOA.cpp` + `GetLayerSpecify()` |
| `0x5e14a4` | `TVPExecuteExpression("Layer")` → objek kelas (hasil **diabaikan** oleh pemanggil: efek samping nol) | tidak ditiru |
| `0x953d0c` | `TVPThrowExceptionMessage(const tjs_char*)` (tidak ditandai noreturn; kode sesudahnya tak terjangkau) | idem |
| `0xa0cd8c` / `0xa11b94` / `0xa0e078` | galat konversi variant / string→integer / `~tTJSVariant` | `TJSThrowVariantConvertError` / `AsInteger` / dtor |
Offset field Layer di biner: `MainImage +0x118`, `ProvinceImage +0x128`, `HoldAlpha +0x198`, `ImageModified +0x199`, `ClipRect +0x19c` (l,t,r,b),
flag byte `+0x261` (disetel 1 hanya oleh jalur CPU `copyRight*/copyBottom*`; identitasnya **tidak diketahui**. Terverifikasi emulasi bahwa itu **bukan** `ImageModified`: `+0x199` tidak berubah di kedua method, dan hanya `copyAlphaToProvince` yang menyetelnya lewat `0x807c44`). Fork **tidak meniru** flag ini.
Slot vtable: `iTJSDispatch2` AddRef +0, Release +8, FuncCall +0x10, NativeInstanceSupport +0xc8; `iTVPTexture2D` Release +0x10,
GetScanLineForRead +0x38, GetScanLineForWrite +0x48, GetPitch +0x50; RenderManager CreateTexture2D(pixel…) +0x18, OperateRect +0xa0;
RenderMethod EnumParameterID +0x10, SetParameterFloat +0x30, SetParameterColor4B +0x38, SetBlendFuncSeparate +0x50, IsBlendTarget +0x58.

## Cara verifikasi (baru di S3)
Sandbox ternyata **bisa** `pip install capstone pyelftools unicorn` (jaringan PyPI diizinkan), sehingga catatan lama "tidak ada disassembler AArch64"
tidak berlaku. `script/emu/k2dis.py` (disassembly beranotasi; 1.829 instruksi BTOA identik dengan `objdump`, selisih hanya alias `mov`/`orr`) dan
`script/emu/harness.py` (Unicorn; menjalankan **fungsi asli** `libgame.so` dengan objek Layer/Texture/TJS tiruan) menghasilkan
`tests/plugins/layerExBTOA/golden_emu.inc` lewat `script/emu/btoa_golden.py`. Dicakup emulasi: jalur CPU kelima method piksel.
**Tidak** dicakup: jalur GPU (shader/blend), `fillAlpha` (bergantung `FillMask`), dan perilaku di perangkat nyata.

## Deviasi sadar fork vs biner
| # | Kasus | Biner | Fork |
|---|---|---|---|
| D1 | `fillByProvince` tanpa gambar province | membaca `null+offset` (crash) | tidak melakukan apa pun |
| D2 | `clipAlphaRect`, `src` objek null | tidak diperiksa (crash) | `TVPSpecifyLayer` |
| D3 | `clipAlphaRect` dengan klip tidak berawal di (0,0) | indeks larik baris dijumlah offset byte `(ct*pitch + cl*4)` → baca di luar batas | memakai asal klip yang dimaksud |
| D4 | `fillByProvince` CPU, klip kiri ≠ 0 / atas+tinggi melewati province | membaca di luar buffer province | sampel di luar buffer tidak cocok |
| D5 | `fillAlpha`/`clip*`/`fill*` pada layer tanpa gambar utama | crash | tidak melakukan apa pun (kecuali pesan yang memang dilempar biner) |
| D6 | tekstur sumber tanpa salinan CPU (`GetScanLineForRead` null) di `clipAlphaRect` | crash | tidak melakukan apa pun |
| D7 | flag `+0x261 := 1` di jalur CPU `copyRight*/copyBottom*` | disetel | tidak ditiru (tanpa padanan; ASUMSI tidak berpengaruh) |

## Keputusan terbuka (S13 / perangkat)
1. `fillByProvince` GPU: faktor blend biner `(ADD, ONE_MINUS_SRC_ALPHA, SRC_ALPHA, ZERO, ONE)` — dengan arti GL standar yang terisi adalah
   piksel yang **tidak** cocok, kebalikan jalur CPU. Saklar `LAYEREX_BTOA_FILLBYPROVINCE_GPU_STD_BLEND` (default 0 = meniru biner).
2. `clipAlphaRect` di render manager GPU: menulis lewat `GetScanLineForWrite` tekstur GL; apakah perubahan sampai ke GPU di fork belum terbukti.
3. Identitas flag `+0x261` dan peran `SyncImage` (`0x80c848`): apakah ketiadaannya di fork mengubah tampilan (terutama jalur software setelah penulisan CPU).
