# Spesifikasi: `Layer.gaussianBlur` (layerExImage.dll)

Bagian umum: [`layerExImage_light.md`](layerExImage_light.md). Implementasi **`0x593e60`** (1.436 B), thunk `0x59664c` (388 B).
Helper: **filter 1-D `0x5943fc`** (1.880 B), `exp` (PLT `0x41a120`), `new[]` (`0x41f9e0`) / `delete[]` (`0x426f20`), dan untuk jalur non-software **`0x7faff8`** (24 B).
Kode: `gauss_taps`, `gauss_build`, `gauss_line`, `gaussian_apply`, `gauss_box_radius`.

## Tanda tangan
`this.gaussianBlur(sigma)` — satu argumen **wajib**, real (double → `float` 32-bit, `fcvt`).

## Perilaku CPU (software render manager; seluruh gambar, **semua 4 kanal termasuk alfa**)
**Kernel** (float 32-bit kecuali disebut double):
1. `r = |sigma·0.5| + 0.25`, `r2 = r + r`. Panjang `n = max(1, fcvtzs(2·ceil((double)r2 − 0.5) + 1))` (selalu ganjil; `sigma = 0` → 1, `1` → 3, `2` → 5, `7.3` → 17). `c = n >> 1`.
2. Untuk `i = c+1 … n−1`: `x = (float)(i−c) − 0.5`; jumlah 50 sampel `j = 1..50`: `d = j·0.02 + x` (double); bila `d ≤ r2`: `acc += (float) exp(−d² / (double)(r·r2))`; `k[i] = acc / 50.0f`.
   (sampel dengan `d > r2` atau NaN dilewati — pemotongan pada `2r`.)
3. Cermin: `k[i] = k[n−1−i]` untuk `i = 0..c`. **Bobot tengah (KUIRK asli)**: `k[c] = (Σ_{j=0..50} exp(−(j·0.02 + 0.5)² / (r·r2))) / 51.0f` — memakai `+0.5` (bukan −0.5),
   sehingga bobot tengah hampir sama dengan tetangganya (puncak datar; mis. sigma 6: `0.1231 0.1232 0.1231`). Ditiru persis.
4. Normalisasi: `k[i] /= Σk` (penjumlahan float berurutan). Tabel `T[i][v] = k[i] · (float)v` (v = 0..255).

**Dua lintasan**: horizontal tiap baris → buffer sementara (`lebar·tinggi·4` byte); lalu vertikal tiap kolom (disalin ke buffer kolom, difilter, ditulis balik ke gambar). Filter 1-D (`len` piksel, 4 kanal, hasil = `trunc(jumlah + 0.5f)` disimpan sebagai **byte rendah**):
- **`n ≤ len`**: tepi kiri `i < c`: jendela terpotong, jumlah `k[c−i+m]·px[m]` untuk `m = 0 .. n−(c−i)−1`, dibagi jumlah bobot yang terpakai; interior `c ≤ i < len−c`: jumlah **tabel** `T[k][px[i−c+k]]` (k = 0..n−1, tanpa normalisasi); tepi kanan: jendela terpotong, bobot `k[0..cnt−1]`, `cnt = len−i+c`, dinormalisasi.
- **`n > len`** (kernel lebih panjang dari baris/kolom): **ANOMALI asli** — pembilang memakai `k[j]` dengan `j` indeks *sumber* untuk `j ∈ [i−c, i+c]`, penyebut `Σ k[c−i+j]` (indeks *relatif*). Tidak sama dengan blur sebenarnya; ditiru apa adanya (diuji terhadap fungsi asli).
- Lalu `redraw()`.

## Jalur non-software (render manager GPU) — **bukan Gaussian**
`rect = clip (clipLeft, clipTop, clipLeft+clipWidth, clipTop+clipHeight)`, `r = fcvtzs(sigma + 0.99f)` (`0x14c30ec`), `area = (−r', −r', r, r)` dengan `r' = fcvtzs(−(sigma+0.99f))`;
panggil `MainImage->DoBoxBlurForAlpha(rect, area)` (`0x7faff8` = pembungkus `InternalDoBoxBlur(rect, area, hasalpha = true)`), lalu `redraw()`. (`0x80c848` dipanggil lebih dulu untuk menyinkronkan gambar; di fork `GetMainImage()` ASUMSI cukup.)

## DEVIASI SADAR di fork
Binary tidak membatasi ukuran kernel: `sigma` besar/∞ menghasilkan alokasi `n·1 KB` (hingga ~GB) → crash. Fork menolak (**tidak mengubah gambar, tanpa redraw**) bila `n > 16385` (`sigma` ≳ 8192).
`sigma` NaN → `n = 1` (identitas). Selain itu identik.

## Kasus tepi
`sigma = 0` → `n = 1`, kernel `[1]`, gambar tidak berubah (tetap `redraw()`). Gambar rata tetap rata (rezim `n ≤ len`). Alfa ikut diburamkan. Selisih mungkin hanya bila `exp()` host/Bionic berbeda di ulp terakhir (di sandbox: `libm` host = nol selisih).

## Uji
20 kasus golden (sigma 0, 1, 2, 3, 7.5 × ukuran 1×1 … 24×12, mencakup `n ≤ len` dan `n > len`) + fuzz filter 1-D langsung (400 kernel/ukuran acak, 0 selisih) + analitik (panjang kernel, simetri, puncak datar, batas deviasi, radius box). **Belum**: jalur non-software, NDK, perangkat.
