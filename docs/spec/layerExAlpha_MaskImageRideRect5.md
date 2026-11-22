# layerExAlpha · `MaskImageRideRect5` — spesifikasi perilaku (terbaca penuh, S8)

- Modul: **`layerExAlpha.dll`** (terkonfirmasi, string di `0x14c9f5c`). Implementasi: **`0x5d9cb8`**, 2.788 B (`libgame.so` 1.3.9 arm64). Registrasi bersama `0x5d43b0` **belum dibaca**.
- Sumber: `recon_alpha3.txt` (fungsi kedua). Kode fork: `layerExAlpha.cpp` + `layerExAlphaCore.hpp` (`ride5`). Spesifikasi kembar: [`layerExAlpha_MaskImageRideRect4.md`](layerExAlpha_MaskImageRideRect4.md) — bagian yang **sama** tidak diulang di sini.
- Legenda: ✅ terbaca penuh · ⚠️ asumsi / deviasi sadar fork · ❓ belum diketahui.

## Panggilan
`this.MaskImageRideRect5(layer1, layer2, a, n)` — 4 argumen (`x1` layer1, `x2` layer2, `w3` = `a`, `w4` = `n`). **Tidak ada parameter warna** (beda dari Rect4). ✅

## Yang SAMA dengan Rect4 ✅
Urutan alur (cek layer → cek ukuran dengan 3 baris log `SW/SH`, `DW/DH`, `AW/AH` → tabel → pita → loop → `redraw()`), pesan `"Not Layer"`, kondisi lewati (`dst.alfa == 0 && layer1.alfa == 0`), `a2 = layer2.alfa`, rumus pita `lo/hi` (`0x5d9f5c–0x5d9f7c`), tujuan = buffer `this`, `n ≤ 0` → `std::length_error`, tanpa pemeriksaan `IsSoftware`.

## Yang BERBEDA dari Rect4 ✅
1. **Tiga vektor `float`** (bukan 1 float + 3 int): `A`, `B`, `C`, masing-masing `m = 2n−1` entri (PLT `…vector<float>…_M_default_append` dipanggil 3×).
2. **Tidak ada warna tetap.** Tujuan dicampur dengan `layer1` memakai bobot, dan di cabang atas **alfa tujuan** yang diubah (bukan rgb).
3. **Batas kolom = `2n−1`, bukan lebar layer** (lihat anomali).

## Tabel ✅ (`0x5d9e64–0x5d9f58`; float, `s1 = 1.0f/(float)n`)
| `i` | `B[i]` | `C[i]` | `A[i]` |
|---|---|---|---|
| `i < n−1` | `s1·(float)(n−1−i)` | `1.0f − B[i]` | `1.0f` |
| `i = n−1` | `0` | `1` | `1` |
| `i > n−1` | `0` | `1` | `s1·(float)(m−i)` |

Klem tiap vektor (urutan `B`, `C`, `A`): `≥ 1 → 1`, `!(> 0) → 0`. (Praktis tak terpicu untuk `n` realistis; ditiru.)

## Efek piksel ✅ (`0x5d9f88–0x5da114`; semua **float**)
`lo`, `hi` seperti Rect4. Lewati bila `dst.alfa == 0 && layer1.alfa == 0`. `a2 = layer2.alfa`.

| Kondisi | Efek |
|---|---|
| `a2 < lo` | tidak berubah |
| `lo ≤ a2 < a` | `idx = a2−lo`; untuk k = 0..2: `dst.k = (uint8)( trunc(B[idx]·(float)dst.k + 0.5f) + trunc(C[idx]·(float)layer1.k + 0.5f) )` (alfa tetap) |
| `a2 ≥ hi` | **`dst.alfa = 0`** (rgb tetap) |
| `a ≤ a2 < hi` | `idx = a2−lo`; `dst.rgb = layer1.rgb`; `dst.alfa = (uint8) trunc( A[idx]·(float)layer1.alfa + 0.5f )` |

- Tiap operasi float terpisah (`fmul` lalu `fadd`, tanpa FMA). Hasil dua `fcvtzs` dijumlah lalu disimpan 8 bit (**tanpa klem**, melingkar).
- Urutan pemeriksaan: `a2 < lo` dulu, lalu `a2 < a`, lalu `a2 ≥ hi`.
- ⚠️ Quirk pita yang sama dengan Rect4: bila `a < n`, `lo = 0` dan tabel tidak bergeser (pusat jatuh di `a2 = n−1`).

## ⚠️ ANOMALI: batas kolom = `2n−1` ✅ (terbukti dari instruksi, bukan tebakan)
Loop kolom `x` berjalan `0 … 2n−2` — **ukuran tabel `m = 2n−1`**, bukan `lebar` layer. Bukti: (1) pembanding loop (`cmp w1, w24`, `0x5da0e8`) memakai register yang berisi `2n−1` (`sub w24, w8, #1` @`0x5d9e00`); (2) guard masuk baris memakai `n ≥ 1` (`cmp w22, #1`, `0x5d9fa4`), yaitu penyederhanaan kompiler dari `0 < 2n−1`; (3) lebar ctx (`+0x10`) **tidak pernah dimuat ulang** setelah pemeriksaan ukuran, tidak seperti Rect4 yang memuatnya per baris. Tinggi tetap dari ctx (`+0x14`).

Akibat (bila `2n−1 > lebar`):
- kolom ≥ lebar menyentuh **piksel di baris berikutnya** (bila pitch = lebar×4) atau byte padding; piksel awal baris berikutnya terkena dua kali;
- pada **baris terakhir**, binary membaca/menulis **melewati akhir buffer** (di emulator: akses di luar wilayah pada 356 dari 363 kasus acak seperti itu; model memprediksi kasus yang sama).
- Bila `2n−1 ≤ lebar`: tidak ada beda; kolom `≥ 2n−1` **tidak diproses sama sekali** (hanya `2n−1` kolom pertama tiap baris).

Dugaan penyebab (⚠️ ASUMSI): variabel ukuran tabel terpakai sebagai batas loop di sumber asli. **Fork:** default dibatasi `min(2n−1, lebar)` (tidak ada akses di luar baris); `-DLAYEREXALPHA_RIDE5_BINARY_XLIMIT` memakai `2n−1` apa adanya (**tidak aman**, hanya untuk diferensial 7.3). Keputusan akhir di S13.

## Error & kasus tepi, perbedaan fork, bukti
Sama dengan Rect4 (lihat spesifikasi kembar), dengan tambahan:
- **Bukti:** 500 kasus acak (Rect5) dieksekusi utuh di emulator → 0 beda, termasuk kasus `2n−1 > lebar` yang menghasilkan akses di luar buffer yang sama di emulator dan model. Cakupan: 284 dari 293 instruksi tubuh utama (sisanya `Not Layer` + 3 klem float tak terjangkau: `0x5d9ef0`, `0x5d9f30`, `0x5d9f40`).
- **Cek mutasi** terdeteksi: B↔C tertukar, alfa tanpa `A`, `A` sisi kanan, batas kolom = lebar (272/300), `strb wzr`→nop, `cmp w1,w24`→`cmp w1,w22`.
- ⚠️ Batas bukti sama: eksekusi instruksi vs model; bukan uji perangkat.

## Uji
`tests/plugins/layerExAlpha/test_alpha3_s8.cpp`: tabel n=2, vektor tangan 6 piksel (semua cabang + lewati), batas `xcount`, parameter ekstrem, 7 hash golden Rect5 (termasuk kasus `2n−1 > lebar` dengan padding cukup agar tetap di dalam buffer; dipanggil dengan `xcount = 2n−1` seperti binary).
