# layerExAlpha — AlphaBorderImageCoverRect(A, B, C, p, q)

Biner: `libgame.so` arm64 1.3.9, fungsi `0x5dbea0`, 2820 B (`recon_alpha5.txt`; badan fungsi berakhir di `0x5dc77c` (±2,3 KB); sisanya *landing pad* pengecualian). Status: **terbaca penuh dari recon**; belum diuji di perangkat.

## Tanda tangan (TJS)
`Layer.AlphaBorderImageCoverRect(A, B, C, p, q)` — `x1 = A`, `x2 = B`, `x3 = C` (tiga Layer; tiap yang bukan Layer → `Not Layer`, diperiksa berurutan A, B, C), `w4 = p`, `w5 = q`. Nama parameter = ASUMSI (`p` = pusat jendela, `q` = lebar).

## Validasi (sebelum menyentuh piksel)
Dimensi diambil dari `0x806828/0x806920`. Bila `this.w != A.w`, `this.w != C.w`, `this.h != A.h`, atau `this.h != C.h` (**B tidak diperiksa**): tiga baris log lalu **kembali tanpa redraw**:
`SW:<this.w> SH:<this.h>`, `DW:<A.w> DH:<A.h>`, `AW:<C.w> AH:<C.h>` (bilangan desimal ASUMSI; fungsi log `0xa16ca4` = `TVPAddLog` ASUMSI, kembali normal sehingga bukan pelempar).
Bila `q < 1`, `q+p < 1`, atau `p-q > 254` (int32): log satu baris `Out of range`, kembali tanpa redraw.

## Jendela
Dari `p, q` (int32 melingkar) dihitung `L`, `R`:
- `w9 = q+p`. `p < 0`: `w8 = 0`. `p >= q`: bila `p >= 256` → `w8 = p+q-255`, `w9 = 0`; selain itu `w8 = q`. `0 <= p < q`: `w8 = p`, dan bila `p > 255` → `w9 = 0`.
- Bila `w9` belum di-nol-kan: `w9 = (w9 < 256) ? q : 255-p+q`.
- `L = max(0,w8)`, `R = max(0,w9)`. Piksel dianggap dalam jendela bila `p-L <= m < p+R`.
Contoh: `(p,q)=(100,3)→L=3,R=3`; `(-1,3)→0,3`; `(200,100)→100,155`; `(300,50)→95,0`.

## Tabel
`n = L+R`; `T[0..n]` (n+1 float32), `s1 = 1.0f/(float)q`:
- `i < L`: `T = s1*(float)(q-L+i)`; `i == L`: `T = 1.0`; `i > L`: `T = s1*(float)(q+L+1-i)`
- lalu `T = min(T,1)`; `T <= 0 → 0`.
(Dua entri berturut-turut = 1,0 di sekitar `i = L, L+1` bila `R >= 1`.)

## Piksel
Buffer: tujuan = `this+0x18` **tersimpan**, pitch `this+0x20`; A, B, C = buffer dan pitch masing-masing. Untuk tiap piksel:
- lewati bila alfa tujuan `== 0` **dan** alfa A `== 0`;
- `m` = alfa C; lewati bila `p-L > m` atau `p+R <= m` (signed);
- `tujuan.RGB := A.RGB` (byte 0..2 disalin); `tujuan.alfa := (uint8)⌊T[m-(p-L)] * alfaB + 0.5f⌋` (`fcvtzs`, low-byte).
Lalu `redraw()` (hanya bila validasi lolos).

## Ringkas
Menyalin warna dari A ke layer ini di tempat alfa C berada dalam jendela `[p-L, p+R)`, dengan alfa baru = alfa B diskalakan tabel "tenda" yang naik hingga pusat lalu turun.

## Kasus tepi
- `n+1` entri; indeks `m-(p-L) ∈ [0, L+R-1]`, selalu dalam tabel. Tabel > 2^24 entri → fork: `TJS_E_INVALIDPARAM`.
- B lebih kecil dari tujuan: aslinya membaca di luar buffer; fork memotong ke irisan. Layer tanpa gambar (buffer null): no-op (aslinya crash).

## Tingkat keyakinan
- **Dibaca dari recon** (`recon_alpha2.txt`, 2026-11-22): alur, rumus, konstanta, urutan operasi, offset medan.
- **ASUMSI**: nama parameter TJS (diturunkan dari pemakaian); semantik `redraw()` = `Update(klip)` (vtable[1] objek `layerExBase_GL`, cocok dengan `layerExBase.hpp` fork, tidak dibongkar); `reset()` dianggap dipanggil pembungkus sebelum method (aslinya method ini hanya memakai `_width/_height` dan, kecuali LevelMaskRect(2), `_buffer/_pitch` yang tersimpan).
- Pembungkus Layer (dibaca tangan, lihat `layerExAlpha_S7_profil_xref.md`): `0x807c00` = pointer tulis buffer utama (menandai berubah), `0x807c20` = pitch.
- Perilaku sebelum `redraw`: tidak ada pembatasan ke klip; seluruh gambar `0..W-1 x 0..H-1` diproses.
- Belum diuji di perangkat/NDK; uji inti host: `tests/plugins/layerExAlpha/test_alpha2_s7s10.cpp`.
