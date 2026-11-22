# layerExAlpha · `MaskImageRideRect4` — spesifikasi perilaku (terbaca penuh, S8)

- Modul: **`layerExAlpha.dll`** (nama modul **terkonfirmasi**: string UTF-16 di `0x14c9f5c`; kelas `layerExAlpha`, di-attach ke `Layer`).
- Implementasi: **`0x5d8f94`**, 3.064 B (`libgame.so` 1.3.9 arm64). Fungsi registrasi bersama `0x5d43b0` (1.832 B) **belum dibaca**.
- Sumber: `recon_alpha3.txt` (depth 0, 2 fungsi, tanpa `LEWATI`). Kode fork: `src/plugins/layerExAlpha.cpp` + `layerExAlphaCore.hpp` (`ride4`).
- Legenda: ✅ terbaca penuh dari disassembly · ⚠️ asumsi / deviasi sadar fork · ❓ belum diketahui.

## Panggilan
`this.MaskImageRideRect4(layer1, layer2, a, n, r, g, b)` — 7 argumen, tanpa nilai bawaan, tanpa pemeriksaan jumlah argumen di fungsi ini (konversi parameter dilakukan ncbind).

| Reg | Parameter | Tipe | Peran |
|---|---|---|---|
| `x0` | `this` | instance `layerExAlpha` | konteks: `+0x10` lebar, `+0x14` tinggi, `+0x18` **buffer tujuan**, `+0x20` pitch tujuan ✅ |
| `x1` | `layer1` | `iTJSDispatch2*` (objek Layer) | sumber warna "ride" (hanya dibaca) ✅ |
| `x2` | `layer2` | idem | **peta alfa/mask**: hanya byte alfanya yang dipakai (hanya dibaca) ✅ |
| `w3` | `a` | int32 | ambang alfa pada `layer2` ✅ |
| `w4` | `n` | int32 | lebar pita (setengah-lebar + 1), `n ≥ 1` ✅ |
| `w5`,`w6`,`w7` | `c0`,`c1`,`c2` | int32 | warna: `c0` → byte 2, `c1` → byte 1, `c2` → byte 0 (BGRA) ✅ |

⚠️ **Nama** parameter (`r`,`g`,`b`, `a`, `n`) diperkirakan; posisi, tipe, dan pemetaan kanal **terbaca**. Tujuan = layer `this` (bukan salah satu argumen). Label log di binary menamai ketiga layer **S** (`this`), **D** (`layer1`), **A** (`layer2`).

## Alur ✅
1. `layer1`: `NativeInstanceSupport(2, Layer.ClassID)`; gagal → `TVPThrowExceptionMessage("Not Layer")` (`0x953d0c`, string di `0x14c494e`). Lalu helper: pointer piksel `0x807c00`, pitch `0x807c20`, lebar `0x806828`, tinggi `0x806920` (gambar utama). Hal sama untuk `layer2`.
2. **Pemeriksaan ukuran:** `ctx.lebar == lebar(layer1) == lebar(layer2)` dan `ctx.tinggi == tinggi(layer1) == tinggi(layer2)`. Tidak cocok → **tiga baris log** (`0xa16ca4`, diduga `TVPAddLog`): `SW:<w> SH:<h>` (ctx), `DW:<w> DH:<h>` (layer1), `AW:<w> AH:<h>` (layer2), lalu **keluar tanpa mengubah apa pun, tanpa `redraw`, tanpa exception**.
3. Alokasi 4 vektor berukuran `m = 2n−1`: 1×`vector<float>` + 3×`vector<int>` (PLT bernama: `_ZNSt6vectorIfSaIfEE17_M_default_appendEm`, `…IiSaIiEE…`). Isi nol.
4. Isi tabel, hitung pita `[lo, hi)`, loop piksel (di bawah), lalu **`this->redraw()`** (panggilan virtual `[vtable+8]`; terjadi juga bila tinggi < 1).

## Tabel ✅ (`0x5d9164–0x5d92e0`; semua float 32-bit, tiap operasi terpisah `fmul` lalu `fadd`)
`fn = (float)n`, `s5 = 1.0f/fn`, `sK = (float)cK / fn`. Untuk `i = 0..m−1` (`d = i−(n−1)` = jarak dari pusat):

| `i` | `vf[i]` | `t` | `iK[i]` |
|---|---|---|---|
| `i < n−1` | `s5 · (float)(n−1−i)` | `i+1` | `trunc( sK·(float)t + 0.5f )` |
| `i = n−1` (pusat) | `0` | — | `cK` **mentah** |
| `i > n−1` | `s5 · (float)(i−n+1)` | `m−i` | `trunc( sK·(float)t + 0.5f )` |

Lalu klem per entri: `vf ≥ 1 → 1` (hanya non-pusat), `!(vf > 0) → 0`; `iK ≥ 255 → 255`, `iK ≤ 0 → 0`.
- Pemetaan: `i1`←`c0`, `i2`←`c1`, `i3`←`c2`. Urutan operasi **`(cK/n)·t`**, bukan `(cK·t)/n` — keduanya beda pada input tertentu (mis. `n=48, c=100, t=30` → **62**, bukan 63); terbukti dari instruksi asli (`probe_table`).
- Klem float `vf ≥ 1` dan `vf ≤ 0` praktis tak terpicu (bobot selalu di (0,1) untuk `n` realistis); tetap ditiru.

## Pita ✅ (`0x5d92e4–0x5d9304`; int32 melingkar)
`lo = min( (a−n < 0) ? 0 : a−n+1 , 255 )`, `hi = min( max(n+a, 0) , 255 )`.
⚠️ **Quirk:** indeks tabel = `alfa2 − lo`. Bila `a ≥ n`, `lo = a−n+1` sehingga pusat tabel (`idx = n−1`) jatuh tepat pada `alfa2 = a`. Bila **`a < n`**, `lo` diklem ke 0 tetapi tabel **tidak bergeser** → pusat jatuh pada `alfa2 = n−1`, bukan `a` (pita tak simetris terhadap `a`). Ditiru apa adanya.

## Efek piksel ✅ (`0x5d9310–0x5d94d4`)
Baris `0..tinggi_ctx−1`, kolom `0..lebar_ctx−1` (lebar & tinggi **dari ctx**; tujuan maju `pitch_ctx`, `layer1` maju `pitch1`, `layer2` maju `pitch2`). Piksel BGRA, byte 3 = alfa.
- **Lewati** bila `dst.alfa == 0` **dan** `layer1.alfa == 0`. `a2 = layer2.alfa`.

| Kondisi | Efek (k = byte 0,1,2) | Presisi |
|---|---|---|
| `a2 < lo` | `dst.k = layer1.k` (alfa `dst` tetap) | — |
| `lo ≤ a2 < a` | `idx = a2−lo`; `dst.k = (uint8)( iK[idx] + trunc( (double)layer1.k · (double)vf[idx] + 0.5 ) )` | **double**: `ucvtf d`, `fcvt d,s`, `fmul d`, `fadd d`, `fcvtzs` |
| `a ≤ a2 < hi` | `idx = a2−lo`; `dst.k = (uint8)( iK[idx] + trunc( vf[idx]·(float)dst.k + 0.5f ) )` | **float**: `ucvtf s`, `fmul s`, `fadd s`, `fcvtzs` |
| `a2 ≥ hi` | tidak berubah | — |

- Pemetaan kanal: byte 0 ← `i3`(c2), byte 1 ← `i2`(c1), byte 2 ← `i1`(c0).
- **Tidak ada klem saat menyimpan:** jumlah `iK + trunc(...)` disimpan sebagai 8 bit terendah (`sturb`), jadi nilai > 255 **melingkar**.
- Cabang tengah memakai **double**, cabang atas memakai **float** — perbedaan presisi ini nyata di binary dan ditiru.
- Alfa `dst` **tidak pernah diubah**.
- Tidak ada pemeriksaan `IsSoftware` (tidak ada panggilan `0x848bdc` di fungsi ini).

## Error & kasus tepi
| Kasus | Binary ✅ | Fork |
|---|---|---|
| objek bukan Layer (`NativeInstanceSupport` < 0) | exception `"Not Layer"` | sama (+ objek null → juga `"Not Layer"`; binary crash) ⚠️ |
| ukuran tidak cocok | 3 baris log, return, tanpa redraw | sama |
| `n ≤ 0` | `2n−1 ≤ −1` → `std::length_error` **tak tertangkap** (diverifikasi di emulator: `_M_default_append(0xFFFF…)`) | exception TJS `"layerExAlpha: n out of range"` ⚠️ |
| `n` sangat besar | alokasi `4×(2n−1)×4 B` sampai gagal | `n > 2²⁰` ditolak ⚠️ (hanya 256 entri pertama tabel pernah dibaca; hasil nyata sama) |
| tinggi/lebar ctx < 1 | tak ada piksel; `redraw` tetap dipanggil | sama |
| `a` mendekati ±2³¹ | `lo` bisa negatif → indeks di luar tabel (UB) | piksel dilewati bila `idx ≥ m` ⚠️ |
| buffer tujuan null (render manager bukan software) | tanpa pemeriksaan | return diam-diam ⚠️ |

## Perbedaan sadar fork (⚠️)
1. Hook instance memanggil `reset()` tiap panggilan (mengikuti pola layerEx upstream). **ASUMSI:** di binary hook itu ada di registrasi `0x5d43b0` (belum dibaca); method ini tidak memanggil `reset()` sendiri.
2. Beberapa jalur UB/crash binary menjadi exception/no-op (tabel di atas).
3. Pemetaan helper dan alamatnya: `helpers_map.md`.

## Bukti
- **Seluruh fungsi dieksekusi** di emulator AArch64 mini (`tests/plugins/layerExAlpha/emu_ride.py`) dengan stub untuk panggilan keluar: 500 kasus acak (lebar/tinggi 1–9, pitch dengan padding, `a` −5..300, `n` 1..300, warna termasuk 256/1000/−7), **0 beda** terhadap model; cabang ketidakcocokan (4 variasi), `Not Layer`, dan `n ≤ 0` terverifikasi.
- Cakupan: 346 dari 353 instruksi tubuh utama tereksekusi; sisanya jalur `Not Layer` (diuji terpisah) dan satu klem float tak terjangkau (`0x5d9258`).
- **Cek mutasi:** semua mutasi model/program terdeteksi (presisi double→float, urutan kanal, `lo`, batas `hi`, bobot, `fadd`→`fsub`). Mutasi presisi/urutan hanya muncul pada sebagian kecil input kecil → golden memakai gambar lebih besar dan probe tabel khusus.
- ⚠️ Batas bukti: eksekusi instruksi vs model, **bukan** uji perangkat; ctx dan hook registrasi tidak ikut terbukti; identik-piksel dengan APK 1.3.9 belum terbukti (7.3).

## Uji
`tests/plugins/layerExAlpha/test_alpha3_s8.cpp` (ASan/UBSan): `band()`, `cvtzs`, tabel (n=3 dan n=48 dari probe), vektor tangan 7 piksel (semua cabang + lewati + alfa 0), `n=1`, pembungkus 8-bit, parameter ekstrem, dan 7 hash golden Rect4 dari eksekusi instruksi asli.
