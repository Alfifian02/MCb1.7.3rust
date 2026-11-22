# layerExAlpha — catatan S7: pemetaan nama→kode (dikoreksi) dan callee

Berkas ini menggantikan versi sebelumnya (2026-11-21), yang memakai pemetaan `layerex_impl_map.tsv` lama dan
**salah satu slot** untuk method Alpha tengah. Spesifikasi per method ada di `layerExAlpha_<method>.md`.

## 1. Koreksi pemetaan (terverifikasi)
Fungsi registrasi `0x5d43b0` terdiri dari blok 28 B; di tiap blok `ADD` string nama **lalu** `ADD` alamat kode saling bersebelahan
(`NAME` → `CODE`). Pemetaan lama menggeser kode satu slot karena tiga nama tidak ada di daftar harapan
(`AlphaAdd`, `ColorVectorEffectAdd`, `MaskRect`) dan dua kode tidak tercantum (`0x5d5f98`, `0x5d8b10`).
Pemetaan benar untuk S7 (dikonfirmasi juga oleh perilaku: `LightForAlpha` menyalin G→alfa, `LuminanceForAlpha` memakai bobot luminans):

| Method | Kode | Ukuran | Status |
|---|---|---:|---|
| LightForAlpha | `0x5d764c` | 288 | dibaca |
| LuminanceForAlpha | `0x5d776c` | 356 | dibaca |
| LevelMaskRect | `0x5d78d0` | 728 | dibaca |
| LevelMaskRect2 | `0x5d7ba8` | 788 | dibaca |
| MaskImageRideRect | `0x5d7fe8` | 1384 | dibaca |
| MaskImageRideRect2 | `0x5d867c` | 1172 | dibaca |
| MaskImageRideRect3 | `0x5d8b10` | 1156 | dibaca (`recon_alpha2c.txt`) |

Efek ke sesi lain: `ExMaskRect = 0x5d6784`, `ExMaskRect2 = 0x5d6ea0` (1664 B; sudah ada di `recon_alpha2.txt`, belum dibaca), `MaskRect = 0x5d621c`,
`WhiteToBlueBlurAdd = 0x5d5f98`, `ExtraColorVectorEffectAdd = 0x5d5cdc`, `ColorVectorEffectAdd = 0x5d5b8c`, `AlphaBlend = 0x5d5894`, `BlackForAlpha = 0x5d57cc`, `AlphaAdd2 = 0x5d5708`.
Method "Function"-overload (`AlphaAdd`, `ColorAlphaAdd`, `MaskImageRideRect4/8`, `AlphaBorderInnerRect`, `AlphaBorderImageCoverRect`): kodenya muncul tanpa `NAME` di blok sendiri
(dugaan: overload terdaftar lewat `Function`); belum diverifikasi.

## 2. Callee
Impor (PLT→GOT→`.rela.plt`): `0x407130`=`__stack_chk_fail`, `0x415b10`=`operator delete`, `0x41ad30`=`_Unwind_Resume`, `0x406260`=`vector<float>::_M_default_append`, `0x4228b0`=`vector<int>::_M_default_append`.
Pembantu: `0x953d0c` = pelempar `Not Layer` (string `0x14c494e`; terkonfirmasi dari pola `tbz w0,#31` setelah `NativeInstanceSupport`); `0x807c00` = pointer tulis buffer utama (menandai `[ni+0x199]=1`); `0x807c20` = pitch.
Tata letak `layerExBase_GL` (cocok dengan `layerExBase.hpp`): `+0 vptr, +8 _this, +0x10 _width, +0x14 _height, +0x18 _buffer, +0x20 _pitch`; `vtable[1]` = `redraw()`.

Ulangi profil: `python3 script/k2xref.py --lib <libgame.so> MaskImageRideRect3` (setelah TSV dikoreksi).
