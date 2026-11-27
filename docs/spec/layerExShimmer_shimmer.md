# shimmer — spesifikasi perilaku

- Modul: `layerExShimmer.dll` ✅ (UTF-16 @`0x14cb3d8`). Method: `shimmer` (@`0x14cb41c`). Didaftarkan oleh thunk `0x5eec10`.
- Implementasi: **`0x5eedb8`** (4.696 B). Sumber bukti: `recon_shimmer.txt` (disassembly objdump, depth 0) — dibaca penuh, 1.174 instruksi.
- Kode: `src/plugins/layerExShimmerCore.hpp` (inti) · `layerExShimmer.cpp` (pembungkus) · uji `tests/plugins/layerExShimmer/test_core.cpp`.

Legenda: ✅ terbaca di disassembly · ⚠️ ASUMSI (tidak terbaca/ditafsirkan) · 🐞 perilaku "apa adanya" yang tampak seperti bug asli, **dipertahankan**.

## Tanda tangan ✅/⚠️
`shimmer(src, map, mask, dx, dy, w, h, sx, sy)` — register: `x0`=this(`layerExBase`), `x1,x2,x3`=`tTJSVariant const&` (src, map, mask), `w4..w7`=int (dx, dy, w, h), `s0,s1`=float (sx, sy).
- ✅ Urutan dan tipe dari prolog (`mov x28,x1; x22,x2; x24,x3; w26,w4; w25,w5; w19,w6; w21,w7; v9=s0; v8=s1`).
- ⚠️ Nama parameter (src/map/mask/dx/dy/w/h/sx/sy) = tafsiran dari pemakaian. Jumlah minimum argumen dan nilai bawaan bila tidak diberikan **tidak terbaca** (ada di wrapper ncbind, bukan di fungsi ini). Fork: argumen yang tidak ada dianggap void/0; `numparams < 2` → `TJS_E_BADPARAMCOUNT`.
- ✅ `mask` boleh void (`[x24+0x10]` = tipe variant; 0 = void).
- ✅ Tanpa `Update()`, tanpa `IsSoftware` (selalu jalur CPU). Tanpa string error `TJS_E_*` di fungsi ini.

## Data yang dibaca ✅
- `this`: `imageWidth/Height` lewat helper `0x5f0248` (menulis ke `this+0x58/+0x5c`), tabel baris **tulis** (flag=1).
- `src` (A): properti `imageWidth` (→`Aw`), `imageHeight` (→`Ah`) dibaca dari objek; tabel baris baca.
- `map` (B): `imageWidth`/`imageHeight`; tabel baris baca. Hanya **byte 0** (kanal biru) tiap piksel 32-bit yang dipakai.
- `mask` (C), bila bukan void: `imageWidth`/`imageHeight` (hanya untuk validasi ukuran) dan byte 0.
- ⚠️ `0x5f0248(variant, int*w, int*h, bool write)` **belum dibaca**: ditafsir sebagai "alokasi `new T*[h]` tabel penunjuk baris dari buffer utama layer". Terbukti: hasilnya di-`delete[]` (`0x426f20`) di akhir (4 tabel). Di fork diganti `pointer + pitch*y`.
- 🐞/⚠️ **Panggilan ke-4 `0x5f0248` memakai argumen ke-2 (`x22` = map), bukan argumen ke-3.** Jadi "tabel mask" adalah tabel baris dari **map** (B) lagi; layer `mask` hanya dipakai sebagai penanda ada/tidak dan untuk ukuran. Fork meniru ini (`SHIMMER_MASK_FROM_C=0`); `=1` memakai layer C (maksud yang masuk akal, **bukan** perilaku 1.3.9).

## Region tulis ✅ (`0x5ef3b0–0x5ef458`)
```
W = (w==0) ? Aw : w ;  H = (h==0) ? Ah : h
dxc = max(dx,0) ; dyc = max(dy,0)
rw = W + min(dx,0) ; rh = H + min(dy,0)            // dx/dy negatif memotong kiri/atas
bila rw<1 atau rh<1                  -> selesai tanpa menulis
bila thisW-dxc<=0 atau thisH-dyc<=0  -> selesai
Wc = (rw+dxc > thisW) ? thisW-dxc : rw ;  Hc = (rh+dyc > thisH) ? thisH-dyc : rh
bila Wc > Bw atau Hc > Bh            -> selesai   // map harus ≥ region
bila tabel mask ada (selalu) dan (Wc > Cw atau Hc > Ch) -> selesai
```
- ✅ **Akibat:** jika argumen `mask` void, `Cw=Ch=0` sehingga `Wc>0` → **selalu tanpa kerja**. Cabang "tanpa mask" di loop (`cbz w1`) **tidak terjangkau** dari binary ini (tetap dimodelkan di inti). ⚠️ tafsir ini bergantung pada panggilan ke-4 di atas.
- Fork menambah penjaga: `Wc<2` atau `Hc<2` → tanpa kerja (binary membaca B di luar batas, UB). `Aw<1`/`Ah<1` → tanpa kerja.
- Koordinat: B dan C diindeks **relatif** (0,0) = sudut region `(dxc,dyc)`; A diindeks **absolut** (koordinat `this`).

## Rumus piksel ✅ (`0x5ef4dc–0x5ef5b0`)
`sxf = fcvtzs(sx, #16)`, `syf = fcvtzs(sy, #16)` (float→16.16, ke nol, saturasi, NaN→0).
Untuk piksel tujuan (X,Y) dengan gradien `gx,gy` (selisih byte-biru B) dan `m` = byte-biru mask:
```
tanpa mask : ox = (gx*sxf) >> 16                 ; oy = (gy*syf) >> 16
dengan mask: ox = ((gx*sxf)*m / 255) >> 16        ; oy = ((m*syf)*gy / 255) >> 16
             (perkalian int32 melingkar; "/255" bertanda dipotong ke nol — idiom smull 0x80808081)
sx' = X+ox ; sy' = Y+oy ; sy' dijepit ke [0,Ah-1] ; sx' dijepit ke [0,Aw-1]   (negatif→0, ≥ukuran→ukuran-1)
tujuan(X,Y) = A[sy'][sx']   (seluruh 32 bit)
```
- Selisih: `gx = B[y][x+1]-B[y][x-1]` (terpusat) · `gy = B[y+1][x]-B[y-1][x]`; di tepi: maju (`B[1]-B[0]`) atau mundur (`B[H-1]-B[H-2]`). Tanda: yang lebih besar indeksnya dikurangi yang lebih kecil.

## Urutan dan cakupan tulis ✅ (relatif terhadap region; W=Wc, H=Hc)
1. **Baris atas** y=0: x = 1…**W-3** (`gx` terpusat, `gy` maju). Hanya bila W>3.
2. **Baris bawah** y=H-1: x = 1…W-3 (`gy` mundur).
3. **Kolom kiri** x=0 dan 4. **kolom kanan** x=W-1, hanya bila H≥4, **H-3** iterasi (k=0…H-4).
5. **Sudut atas** kiri lalu kanan; 6. **sudut bawah** kiri lalu kanan.
7. **Interior** (W≥3 dan H≥3) dibagi ke thread (⚠️ tafsiran: `0xa34aa4` = jumlah thread, `0xa34ad0` = jalankan paralel; lambda di `0x5f05ac/0x5f05c0` bila mask ada, `0x5f0410/0x5f0424` bila tidak), baris 1…H-2 dibagi rata, sisa ke potongan terakhir. ⚠️ Hasil tak bergantung jumlah thread (ASUMSI: baris tak saling bergantung; sisa ke potongan terakhir terbukti dari `sdiv/mul/sub`).

🐞 **Perilaku apa adanya yang dipertahankan** (kunci pengujian `[BIN-L1..L3]`):
- **L1** — loop baris atas/bawah berhenti satu piksel lebih awal: kolom relatif **W-2** pada baris 0 dan H-1 **tidak pernah ditulis** (dan untuk W=3 loop tidak berjalan).
- **L2** — loop kolom kiri/kanan menulis baris relatif 1…**H-3**; baris **H-2** (dan seluruh kolom tepi bila H<4) **tidak ditulis**. Baris sumber gradien B/C = k+2, sedangkan baris tujuan = k+1 (tidak simetris; gradien-y = `B[k+3]-B[k+1]`).
- **L3** — loop kolom kiri/kanan menulis ke baris tujuan `1+k` **tanpa menambah `dyc`** (sedangkan Y sumber = `dyc+1+k`). Efektif hanya bila `dy>0`. Kunci build `SHIMMER_FIX_EDGE_COL_DY=1` menulis ke `dyc+1+k` (perbaikan, bukan 1.3.9).
- Piksel yang tidak ditulis **mempertahankan isi lama** `this`.

## ⚠️ ASUMSI-INT — interior
Worker interior (`0x5f0494` 280 B tanpa mask, `0x5f0630` 360 B dengan mask) **tidak ada** di recon. Diasumsikan: untuk y=1…H-2, x=1…W-2, `gx`/`gy` terpusat pada kedua sumbu, `m` = byte-biru mask di (x,y), rumus piksel sama. Bukti tidak langsung: struktur tugas (`+0x48` = W-2 lebar interior, `+0x4c` = jumlah baris potongan, `+0x50/+0x54` = sx, sy float mentah) dan pola tepi yang memakai selisih terpusat sepanjang tepi. **Bila worker ternyata berbeda (mis. juga L1/L2 di dalamnya), hasil interior berbeda** → uji diferensial 7.3 wajib.

## Error & tepi
- Layer bukan objek: konversi variant gagal (`0xa0cd8c(…,1)`) → pengecualian; fork: `TVPSpecifyLayer`.
- Ukuran 0 / region kosong: kembali tanpa menulis (tanpa pengecualian). `this` sama dengan `src`/`map`: dibaca/ditulis lewat tabel terpisah tanpa salinan; hasil mengikuti urutan tulis di atas (alias).
- Pengecualian di tengah pekerjaan membebaskan ke-4 tabel (jalur unwind) — tidak relevan di fork.

## Uji (inti-host, ASan/UBSan lolos)
Aritmetika (`fcvtzs #16`, `/255`, wrap), pemotongan region, peta datar → salinan (semua W,H 2…9, termasuk piksel yang sengaja tidak tertulis), gradien-x/y, mask 0 dan 255, L3. **Belum:** kesetaraan dengan binary (7.3), jalur `layerExShimmer.cpp` (tidak dikompilasi di sandbox).
