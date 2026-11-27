# shrinkCopy — spesifikasi perilaku (lengkap untuk `shrinkCopy`; `shrinkCopyFast` belum)

- Modul: `shrinkCopy.dll` (string UTF-16 @`0x14c5b54`); method `shrinkCopy` @`0x14cbe00`
- Registrasi → implementasi: `0x5ffcf8` (2.468 B). Sumber: rekonstruksi dari libgame.so arm64 1.3.9.

## Tanda tangan
`Layer.shrinkCopy(dx, dy, dw, dh, src, sx, sy, sw, sh)`  (nama parameter perkiraan)
- `numparams < 9` → `TJS_E_BADPARAMCOUNT` (-1004).
- `objthis` null / bukan Layer → `TJS_E_NATIVECLASSCRASH` (-1008).
- `src` (param[4]): objek Layer; bukan Layer → pengecualian "specify layer" (di binary, objek null tidak diperiksa
  dan akan crash; fork sebaiknya melempar `TVPSpecifyLayer`).
- Fungsi memilih jalur lewat `RenderManager->IsSoftware()` (`0x848bdc`, vtable +0x40).
- **Tidak memanggil `Update()`** (tidak ada pemanggilan di kedua jalur) — beda dengan `copyRaster` di fork.
- Nilai kembali selalu `TJS_S_OK` (0) bila lolos.

## Jalur GPU (bukan software)
- `dx,dy,dw,dh,sx,sy,sw,sh` dibaca sebagai **integer** (real dipotong ke nol, string dikonversi). **Tidak ada validasi.**
- Rect tujuan = (dx, dy, dx+dw, dy+dh); rect sumber = (sx, sy, sx+sw, sy+sh).
- Render method bawaan bernama **`"Copy"`** (string UTF-8 @`0x14dc74b`), tekstur dari `src`, tujuan
  `dest->GetTextureForRender(method->IsBlendTarget(), &destRect)`, `OperateRect(method, dtex, null, destRect, [srcTex,srcRect])`.
  Penskalaan dilakukan sampler GPU (bukan area-average).

## Jalur CPU (software)
- `dx,dy,dw,dh` dibaca sebagai **double**; `sx,sy,sw,sh` sebagai integer.
- Validasi (semua → `TJS_E_INVALIDPARAM` -1003): `sw<1`, `sh<1`, `dw<=0`, `sw < (int)dw`, `dh<=0`, `sh < (int)dh`.
  (jadi hanya **mengecilkan** atau 1:1; tidak bisa memperbesar).
- `clip()` (0x60069c) → `layerExShrinkCore.hpp::clip`: memotong sumber ke gambar sumber (tujuan ikut diskalakan),
  lalu rect tujuan integer = floor(dx) .. ceil(dx+dw), dipotong ke layer tujuan; mengembalikan `clipX/clipY/outW/outH`.
- Tabel per sumbu (28 B/entri) dibangun oleh `0x6011e4(ctx, buf, flag)`: `flag=1` → tabel X (outW-clipX entri), mengembalikan
  bobot piksel penuh `fx`; `flag=0` → tabel Y, mengembalikan `fy`. **Isi fungsi ini belum dibaca.**
- Loop akumulasi (0x600ab8..0x601158) → `layerExShrinkCore.hpp::accumulate`: jumlah berbobot (uint32) per kanal B,G,R,A;
  warna memakai bobot `wc_*`, alfa memakai bobot `wa_*`; hasil = jumlah / (X.div * Y.div), dipotong ke 8 bit.
  Bobot alfa nol **melewati pembacaan** piksel (parsial kiri/atas bisa di luar gambar).
- Larik pointer baris sumber (`srcH` entri) dibangun dari `GetScanLine(y)` gambar utama sumber; tujuan ditulis ke
  (ix+clipX, iy+clipY) dengan pitch tujuan. Buffer tabel dialokasikan `28*(outW-clipX)*(outH-clipY)` (berlebih di binary).

## Pembangun tabel `0x6011e4(ctx, buf, flag)` (sudah terbaca, kecuali entri tepi)
- X (flag=1): `scale = sw/dw`, `frac = dx - ix`, `base = sx`, `stride = 4`. Y (flag=0): `sh/dh`, `dy - iy`, `sy`, `stride = 1`.
- `unit`: bila `scale > 0.0625` (konstanta @`0x14cbda8`) maka 256, selain itu `256/(int)(0.125/scale)` (1 bila pembaginya > 256).
  Pada shrinkCopy `scale >= 1`, jadi `unit = 256`. Nilai kembalian = `unit` (= `fx`/`fy`).
- Entri k (piksel tujuan `clipX+k`) dibangun untuk: entri pertama dan terakhir oleh `0x601454`, entri tengah inline:
  `st=(i-frac)*scale`, `en=st+scale`, `off=(int(st)+base+1)*stride`, `n=int(en)-int(st)-1`,
  `wlo=unit-int(frac(st)*unit)`, `whi=int(frac(en)*unit)`, `div=n*unit+wlo+whi`; `wa=wc` pada entri tengah.
- Tabel Y mengikuti tabel X di buffer yang sama (X: `outW-clipX` entri).

## Entri tepi `0x601454(ctx, params, entry, i)` (edge_entry)
Dipakai untuk entri pertama dan terakhir tiap tabel (dan entri ke-2 bila tabel hanya 2 entri).
- `st=(i-frac)*scale`, `en=scale+st`; `stC=max(st,0)`, `enC=min(en,srcLen)` (srcLen = sw atau sh setelah clip).
- **Bobot WARNA** (`wc_*`) dan `div` memakai posisi mentah: `wc_lo=unit-int(fr(st)*unit)`, `wc_hi=int(fr(en)*unit)`,
  `div=(int(en)-int(st)-1)*unit+wc_lo+wc_hi`. Bagian di luar sumber memperpanjang piksel tepi (bobot warna bisa > unit).
- **Bobot ALFA** (`wa_*`), `off` dan `n` memakai posisi terpotong: `off=(int(stC)+base+1)*stride`,
  `wa_lo=unit-int(fr(stC)*unit)`, `wa_hi=int(fr(enC)*unit)`, `n=int(enC)-int(stC)-1`.
- Akibat: alfa di tepi memudar (bagian luar dihitung transparan) dan, karena piksel parsial berbobot alfa nol **dilewati**,
  bobot warna `wc_hi` di tepi kanan/bawah tidak ikut dijumlahkan walau tetap masuk `div` (warna sedikit gelap di tepi
  pecahan). Ini perilaku asli yang sengaja ditiru; belum diverifikasi di perangkat (7.3).

## Yang belum diketahui
1. ⚠️ **`shrinkCopyFast`** (nama @`0x14cbe16`): registrasi `0x601704` → implementasi `0x60185c` (1.332 B), belum dibaca (4.6b).
2. Perilaku di perangkat nyata (7.3): arah/pembulatan, dan apakah jalur GPU benar-benar "Copy" dengan filter bilinear.
