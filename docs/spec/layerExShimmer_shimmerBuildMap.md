# shimmerBuildMap — spesifikasi perilaku

- Modul: `layerExShimmer.dll` ✅. Method `shimmerBuildMap` (@`0x14cb42c`), thunk registrasi `0x5eec10`.
- Implementasi: **`0x5f0010`** (568 B), dibaca penuh dari `recon_shimmer.txt` (142 instruksi).
- Kode: `layerExShimmerCore.hpp::build_map` · `layerExShimmer.cpp` · uji `tests/plugins/layerExShimmer/test_core.cpp`.

Legenda: ✅ terbaca · ⚠️ ASUMSI · ❓ belum dibaca.

## Tanda tangan
`shimmerBuildMap(src, ox, oy [, x, a, b])` — `x0`=this, `x1`=variant (src), `w2`=ox, `w3`=oy, `x4`=variant opsional, `w5`,`w6`=int ✅ (urutan dari prolog). Nama = tafsiran ⚠️. Nilai bawaan/jumlah minimum argumen: di wrapper ncbind, tidak terbaca; fork: `numparams<3` → `TJS_E_BADPARAMCOUNT`.

## Jalur 2-sumber (argumen ke-4 void) ✅
```
sw,sh = ukuran src ; dw,dh = ukuran this  (dari helper 0x5f0248; `this` ditulis, src dibaca)
oxm = ((ox % sw) + sw) % sw ;  oym = ((oy % sh) + sh) % sh        // modulo positif; % = sisa C (ikut tanda)
untuk y=0..dh-1, x=0..dw-1:
    this[y][x].byte0 = src[ pmod(y - oym, sh) ][ pmod(x - oxm, sw) ].byte0
```
- ✅ **Hanya byte 0 (biru)** piksel tujuan yang ditulis (`strb`, maju 4 byte); byte 1–3 **tidak berubah**. Jadi hasilnya adalah ubin (tiling) kanal biru `src` bergeser (ox,oy) yang menutupi seluruh `this`.
- ✅ Tanpa `Update()`, tanpa string error. Tabel baris dibebaskan (`delete[]`) di akhir.
- ✅ `sw=0` atau `sh=0`: AArch64 `sdiv` bagi-nol = 0 (tidak trap) lalu indeks baris di luar batas (UB). **Fork:** kembali tanpa kerja (deviasi disengaja, juga untuk `dw/dh<1`).

## Jalur argumen ke-4 non-void ❓
`[x4+0x10]!=0` → ukuran/tabel argumen ke-4 dibaca (`0x5f0248`), `w5/w6` disimpan, lalu **`0x5f0798(this, &konteks)` (372 B) dipanggil dan mengerjakan semuanya** (jalur 2-sumber di atas dilewati; `0x5f0798` **belum dibaca**). Fork: `TJS_E_NOTIMPL`. ⚠️ Tebakan fungsi (mis. mengisi kanal biru dari ubin yang dimodulasi mask) **tidak** dipakai.

## Uji
Ubin tanpa geser, geser (+1,+1), geser negatif, byte lain tak berubah, sumber ukuran 0 (ASan/UBSan lolos).
