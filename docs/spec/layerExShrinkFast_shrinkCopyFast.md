# shrinkCopyFast — spesifikasi perilaku (jalur CPU **lengkap**; jalur GPU punya satu anomali terbuka)

- Modul: `shrinkCopy.dll` (sama dengan `shrinkCopy`; satu fungsi registrasi `0x601704`, nama UTF-16 @`0x14cbe16`).
- Implementasi: **`0x60185c`** (1.332 B). Worker CPU **`0x601d90`** (1.628 B). Pengecil satu baris **`0x6023ec`** (792 B) — **terbaca penuh di S1** (`recon_6023ec.txt`, depth 0, 1 fungsi).
- Sumber: rekonstruksi dari `libgame.so` arm64 1.3.9 (`recon_60185c.txt` depth 1 [sesi lalu]; `recon_6023ec.txt` depth 0 [S1]). Kode: `src/plugins/layerExShrink.cpp` + `layerExShrinkFastCore.hpp`.

Legenda keyakinan: ✅ terbaca penuh dari disassembly · ⚠️ asumsi / belum diverifikasi. **Semua bagian selain `0x6023ec` berasal dari recon sesi lalu (warisan) dan tidak dibaca ulang di S1** (recon-nya tidak diunggah ulang); hanya `0x6023ec` yang dibaca dan diverifikasi di S1.

## Tanda tangan ✅
`Layer.shrinkCopyFast(src, hs [, vs])`  (nama parameter perkiraan)

| # | Tipe | Arti | Default |
|---|---|---|---|
| 0 | Layer | sumber | — |
| 1 | int | `hs`: faktor kecil horizontal | — |
| 2 | int | `vs`: faktor kecil vertikal | tidak ada **atau bernilai 0** → `vs = hs` |

- Parameter integer dibaca seperti `AsInteger` (real dipotong ke nol; string diparse; `void` → 0).
- `numparams < 2` → `TJS_E_BADPARAMCOUNT` (-1004).
- `objthis` null / bukan Layer → `TJS_E_NATIVECLASSCRASH` (-1008).
- `src` (param[0]) bukan objek → pengecualian konversi tipe; bukan Layer → pengecualian "specify layer". Objek null **tidak diperiksa**
  di binary (crash); fork melempar `TVPSpecifyLayer` (perbaikan, bukan perilaku asli).
- **Tidak memanggil `Update()`** di kedua jalur (sama dengan `shrinkCopy`). Kembalian `TJS_S_OK` bila lolos.
- Jalur dipilih oleh `IsSoftware()` (`0x848bdc`).

## Ukuran tujuan ✅
`dw = sdiv(sw + hs - 1, hs)`, `dh = sdiv(sh + vs - 1, vs)` — **32-bit bertanda, penjumlahan melingkar**, `sdiv` AArch64
(bagi nol → 0, tanpa trap). `sw,sh` = ukuran gambar utama `src`. Gambar `this` diubah ukurannya menjadi `dw × dh`
(`0x806960` → `0x805e44`, hanya bila ukuran berbeda). `sw, sh` dibaca **sebelum** `this` diubah ukurannya.

## Jalur CPU ✅
1. `hs < 1` atau `vs <= 0` → `TJS_E_INVALIDPARAM` (-1003).
2. Hitung `dw,dh`; ubah ukuran `this`; panggil worker `0x601d90` dengan blok konteks 0x30 B (`+0x00` dest, `+0x08` src, `+0x10` hs,
   `+0x14` vs, `+0x18` `sw/hs`, `+0x1c` `sw%hs`, `+0x20` sw, `+0x24` sh, `+0x28` dw, `+0x2c` dh).
3. Worker, per `vs`:
   - **`vs <= 1`**: tiap baris sumber → satu baris tujuan lewat `hrow` (`0x6023ec`, lihat bagian di bawah). Tinggi tujuan = `sh`.
   - **`vs >= 2`**: buffer sementara `dw·4·vs` byte (PLT `0x41f9e0` / `0x426f20`, dugaan malloc/free). `nfull = sh / vs` grup penuh:
     tiap grup = `vs` baris sumber → `hrow` ke buffer → **rata-rata vertikal** → satu baris tujuan.
     Sisa `rem = sh − vs·nfull`; bila `rem > 0`, satu baris tujuan lagi dari `rem` baris (**dibagi `rem`, bukan `vs`**).
4. **Rata-rata vertikal** ✅ (`0x602204–0x602280`, `0x6022d0–0x602348`): per kanal byte 0,1,2 jumlahkan baris lalu **`udiv` 32-bit**
   (memotong, tanpa pembulatan). **Byte 3 (alfa) tujuan = `0xFF`** — alfa sumber diabaikan.
   Jalur `rem == 1` (`0x601fe4`) dan `vs` penuh memakai aturan sama (NEON `ld4/st4` dengan lane alfa = 0xFF).
5. Tidak ada pemeriksaan `src == this` di binary: gambar sumber diubah ukurannya dulu lalu dibaca (baca di luar batas). **Fork menolak
   dengan `TJS_E_INVALIDPARAM`** (deviasi disengaja, lihat 7.3).

### ✅ Pengecil baris horizontal `0x6023ec` (`hrow`) — terbaca penuh (S1, langkah 4.6b-2)

**Bentuk:** fungsi **daun** (tanpa `bl`/`blr`, tanpa PLT, tanpa helper), skalar murni (**tanpa NEON**/instruksi vektor), 792 B. Kode fork: `hrow()` di `layerExShrinkFastCore.hpp`.

**Argumen:** `x0` = blok konteks 0x30 B, `x1` = pointer baris tujuan, `x2` = pointer baris sumber. Dari blok konteks hanya tiga field dibaca:
`+0x10` = `hs` (`ldrsw`, 0x602400), `+0x18` = `quot = sw/hs` (0x602404), `+0x1c` = `rem = sw%hs` (0x6025b8). Ini **mengonfirmasi** tata letak konteks di langkah 2 jalur CPU.
Format piksel: 4 byte, urutan indeks 0,1,2 = kanal yang dijumlahkan (di tabel ini disebut B,G,R sesuai BGRA), indeks 3 = alfa.

**Algoritma** (alamat di `libgame.so` arm64):

| Tahap | Alamat | Perilaku |
|---|---|---|
| `hs == 1` | `0x602410`→`0x602414–0x60245c` | `quot` kali: salin 3 byte (indeks 0,1,2), tulis `0xFF` di indeks 3 (`orr w11,wzr,#0xff` @`0x602424`; `strb` @`0x602448`). Sama dengan rata-rata 1 piksel. |
| blok penuh, `hs ≥ 2` | `0x6024a0–0x6025ac` | `quot` kali: jumlahkan `hs` piksel per kanal di akumulator **32-bit**; `udiv` **32-bit tak bertanda** dengan `hs` (`0x60257c–0x602584`) → **memotong, tanpa pembulatan**; indeks 3 = `0xFF` (`strb w18` @`0x60258c`). Loop dibuka 2× (`0x6024d8–0x602510`, akumulator genap/ganjil terpisah) + 1 piksel ekor bila `hs` ganjil (`0x602540–0x602568`); hasilnya sama dengan penjumlahan berurutan (komutatif). Sumber maju `hs*4` B, tujuan 4 B per iterasi. |
| sisa (`rem ≥ 1`) | `0x6025b8–0x6026e8` | Satu piksel tujuan lagi dari `rem` piksel terakhir. `rem == 1` (`0x6025c8–0x6025dc`): salin 3 byte apa adanya. `rem ≥ 2` (`0x6025e0–0x6026cc`): jumlahkan (loop 2× + ekor ganjil) lalu `udiv` dengan **`rem`** (BUKAN `hs`), memotong. Indeks 3 = `0xFF` (`0x6026e4–0x6026e8`). |
| `rem < 1` | `0x6025bc`→`0x6026ec` | Selesai tanpa menulis apa pun. |

**Sifat yang sudah pasti**
- Alfa **tujuan** selalu `0xFF` (tepat 3 titik `strb …,#3`, semuanya `0xFF`). Byte alfa **sumber tidak pernah dibaca** (0 `ldrb` ber-offset 3).
- Membaca tepat `sw*3` byte sumber dan menulis tepat `ceil(sw/hs)*4` byte tujuan (tiap byte sekali); tidak menyentuh memori lain, tidak mengalokasikan.
- `quot = 0` (mis. `hs > sw`): hanya blok sisa, satu piksel dari `sw` piksel dibagi `sw`.
- `sw = 0` (`quot = rem = 0`): tidak menulis apa pun.
- Akumulator 32-bit: tidak meluap untuk `hs·255 < 2³²` (`hs < ≈16,8 juta`), jauh di atas lebar gambar nyata.
- Cabang tak terjangkau di jalur CPU yang tervalidasi (`hs ≥ 1`): `0x602570` (jumlah nol untuk `hs < 1`) dan `0x60252c` (jalur generik bila `hs ≤ 1` atau bagian genap = 0); dicatat agar tidak membingungkan saat membaca ulang.
- Catatan anotasi: label `<std::vector<…>::_M_default_append+0x…>` pada target cabang di recon adalah simbol ekspor terdekat, **bukan** nama fungsi (lihat `helpers_map.md`).

**Cara verifikasi (S1):** disassembly asli di `recon_6023ec.txt` **dieksekusi** di emulator AArch64 mini (`tests/plugins/layerExShrinkFast/emu_hrow_6023ec.py`,
hanya himpunan instruksi fungsi ini) pada 1.605 kasus (`sw` 0..40 × `hs` 1..13 × 3 sumber acak/semua-0xFF, plus 6 ukuran besar s.d. `sw=4096`) dan dibandingkan dengan model
rata-rata kotak: **0 beda**; dicek juga tidak ada baca/tulis di luar batas, alfa sumber tidak terbaca, tiap byte tujuan ditulis sekali. Kepekaan emulator diperiksa dengan **mutasi**
(pembulatan, sisa dibagi `hs`, alfa dirata-rata, `udiv`→`mov`, `add`→`sub`): semua terdeteksi (157–232 dari 232 kasus per mutasi). Hash golden hasil emulasi ditanam di uji C++.

**Batas bukti:** ini bukti **eksekusi instruksi terhadap model**, bukan uji di perangkat. Yang tidak ikut terbukti: nilai `hs`/`quot`/`rem` yang diisi pemanggil `0x60185c`
(warisan; konsisten dengan tata letak konteks di atas) dan perilaku end-to-end di APK 1.3.9 (langkah 7.3).

## Jalur GPU ✅ (dengan satu anomali ⚠️)
- Tanpa validasi `hs/vs`. `sw,sh` dari `src`; `sw<1` atau `sh<1` → return `TJS_S_OK` tanpa kerja.
- `dw,dh` seperti di atas; `dw<1` atau `dh<1` (termasuk `hs=0`, `hs<0`) → return `TJS_S_OK` tanpa kerja.
- `refcount++` tekstur → ubah ukuran `this` ke `dw×dh` (`0xa74f34` + `0x806960`) → render method bawaan **`"Copy"`** (string ASCII
  @`0x14dc74b`, `GetRenderMethod`, di-cache statis) dengan rect tujuan `(0,0,dw,dh)` dan rect sumber `(0,0,sw,sh)`;
  `OperateRect` (vtable +0xa0) → `Release` tekstur. Penskalaan oleh sampler GPU, **bukan** rata-rata kotak.
- ⚠️ **Anomali:** tekstur yang di-`AddRef` dan dipakai sebagai sumber diambil dari `[ [this+0x118] + 0x58 ]` — yaitu gambar **`this`**
  (dest), sedangkan `sw,sh` diambil dari `src`. Kemungkinan bug asli (atau salah baca). Fork memakai tekstur `src` secara default;
  `-DSHRINKFAST_GPU_USE_DEST_TEXTURE=1` meniru binary. **Putuskan lewat uji perangkat GPU (7.3)** dengan `dest ≠ src`.

## Fungsi pembantu yang dipakai (peta 4.7; **kamus lengkap + padanan fork + tingkat keyakinan: [`helpers_map.md`](helpers_map.md)**)
`0x848bdc` IsSoftware · `0x848834` TVPGetRenderManager · `0x806828`/`0x806920` lebar/tinggi gambar · `0x80c848` sinkron gambar ·
`0xa74e90`/`0xa75020` lebar/tinggi bitmap (`[bitmap+0x58]+0xc/+0x10`) · `0xa75340`/`0xa7546c` pointer tulis/pitch ·
`0x806960` set ukuran bila beda · `0xa74f34` ubah ukuran gambar utama · `0xa0cd8c`/`0xa11b94`/`0x9b07c8` konversi `tTJSVariant`.

## Uji
- `tests/plugins/layerExShrinkFast/test_core.cpp` (ASan/UBSan, lolos): `sdiv`/`ceil` melingkar, alfa 0xFF, pembagian memotong per kanal,
  sisa grup dibagi `rem`, `vs > sh`, pitch dengan padding, warna rata tetap rata untuk 1.296 kombinasi ukuran/faktor.
  **Bagian `[BINARY-H]` (S1):** vektor tangan (blok + sisa 1, pemotongan 254/1/75, `hs > sw`, `hs = 1`, `sw = 0`, blok 70.000 piksel),
  13 hash golden FNV-1a dari **eksekusi instruksi asli** `0x6023ec` (alfa sumber dibalik → hasil tak berubah), dan end-to-end `shrink_fast` `hs>1, vs=1`.
  Buffer berukuran pas sehingga ASan menangkap baca/tulis di luar batas. Uji dibuktikan **bisa gagal** dengan 3 mutasi kode (pembulatan, sisa÷`hs`, alfa sumber).
- `tests/plugins/layerExShrinkFast/emu_hrow_6023ec.py <recon_6023ec.txt> [--golden]`: emulator + pembanding (butuh recon; recon **tidak** di-commit).
- **Belum ada:** build NDK, TJS di perangkat, differential test terhadap APK 1.3.9 (7.2–7.3).
