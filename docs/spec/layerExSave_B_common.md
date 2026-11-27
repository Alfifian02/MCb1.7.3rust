# layerExSave bagian B — konvensi bersama (sesi S12)

- Modul: **`LayerExSave.dll`** (sama dengan bagian A; `0x14caf06`). Urutan string di biner: `LayerExSave.dll`, **`WindowSaveImage`** (`0x14caf26`), `startSaveLayerImage`, … lalu
  `saveLayerImageTlg5` (`0x14cb100`), `deflate initialize` (`0x14cb126`), `saveLayerImagePng` (`0x14cb14c`), `saveLayerImagePngOctet` (`0x14cb170`), `primaryLayer` (`0x14cb19e`),
  dua pesan galat Jepang (`0x14cb1b8`, `0x14cb1e6`), `onSaveLayerImageProgress` (`0x14cb20c`), `onSaveLayerImageDone` (`0x14cb23e`), `saveLayer:` (ASCII `0x14cb28c`).
- **Bukti:** fungsi dibaca langsung dari `libgame.so` dengan disassembler (capstone; `script/emu/k2dis.py`), **tidak** dieksekusi di emulator. `recon_saveB.txt` hanya memuat
  `startSaveLayerImage` + 13 callee-nya: **batas `MAXF=14` terlewati** sehingga lima method lain kosong di berkas itu (lihat Catatan 32 di rencana). Tingkat: bacaan-biner + inti-host.
- Pembagian kelas: tiga method **`Layer`** (fungsi mentah `tjs_error`, seperti bagian A): `saveLayerImageTlg5`, `saveLayerImagePng`, `saveLayerImagePngOctet`.
  Tiga method **`Window`** (member fungsi kelas C++ `WindowSaveImage`, `NCB_ATTACH_CLASS(WindowSaveImage, Window)`): `startSaveLayerImage`, `cancelSaveLayerImage`, `stopSaveLayerImage`; registrasi di `0x5ea344`.
- ⚠️ `0x42dd34` (alamat "WindowSaveImage" di peta lama) **bukan** itu: fungsi tersebut adalah inisialisasi global (mengisi penunjuk vtable ke `.bss`, tanpa string TJS). Konstruktor/destruktor
  `WindowSaveImage` **tidak dibaca** (ASUMSI: objek = `[+0] objthis (Window)` + `[+8] std::vector<Tugas*>`; terbukti dari pemakaian di `0x5ea458`/`0x5eaa14`/`0x5eaa3c`).

## Struktur tugas (`new` 0x70 byte di `0x5ea458`; `tTJSVariant` di biner = 0x14 byte, tipe di +0x10)
| Offset | Isi | Dipakai |
|---|---|---|
| +0x00 | penunjuk pemilik (`WindowSaveImage*`); **null = dilepas (stop)** | worker memutuskan kirim/tidak, `stop` mengosongkan |
| +0x08 | variant objek: layer penyimpan `saveLayer:<nama>` | worker, callback |
| +0x1c | variant string: nama berkas | worker, callback |
| +0x30 | variant opsi (objek kamus atau bukan-objek → null) | worker (`TVPSaveAsTLG` meta / opsi PNG) |
| +0x44 | byte **batal** | `cancel`/`stop` menulis 1; callback progres mengembalikannya |
| +0x48 | variant integer: indeks slot | callback |
| +0x5c | variant integer: persen terakhir (awal: void → 0) | callback progres |

Padanan fork: `SaveTask` (`layerExSave.cpp`). Variant TJS hanya dibuat/dilepas di **thread utama** (deviasi sadar, lihat bawah).

## Siklus hidup
1. `startSaveLayerImage` (thread utama): cari slot bebas → buat layer penyimpan → `assignImages` → isi slot → `std::thread(worker, tugas).detach()`.
2. Worker (`0x5e9db8`): jalur PNG atau TLG5 menurut ekstensi → bila pemilik non-null, kirim lambda **selesai** ke thread utama; bila null, bebaskan tugas di worker.
3. Progres (hanya PNG): callback `0x5e9c38` memperbarui persen; bila berubah **dan** pemilik non-null, kirim lambda **progres** ke thread utama.
4. Lambda (`0x5ed788` progres, `0x5ed98c` selesai) memanggil fungsi pada objek `Window`: `onSaveLayerImageProgress(indeks, persen, layer, nama)` dan
   `onSaveLayerImageDone(indeks, batal, layer, nama)` (`FuncCall`, hasil/anggota-tak-ada diabaikan). Keduanya hanya dikirim bila `threads[indeks] == tugas`. Selesai: `threads[indeks] = null`, lalu tugas dibebaskan.
5. **Plugin tidak meng-invalidate layer penyimpan** (`saveLayer:<nama>`): skrip yang menerima `onSaveLayerImageDone` harus melakukannya (layer tetap anak `primaryLayer`, `visible` bawaan false).
- Pengiriman ke thread utama = `0x914ab8(*0x1a9cfd0, std::function<void()>, 0, 0)` (ASUMSI: penjadwal thread utama; padanan fork `Android_PushEvents` → `performFunctionInCocosThread`, FIFO).

## Bahaya pada biner (ditemukan dari bacaan)
| # | Bahaya | Fork |
|---|---|---|
| 1 | Worker **tanpa try/catch**: berkas tak bisa dibuat (`TVPCreateStream` melempar) → pengecualian keluar dari thread → `std::terminate` | tangkap + log + `Done(batal=1)` (`LAYEREXSAVE_WORKER_CATCH=1`; 0 = meniru biner) |
| 2 | `cancel`/`stop` membandingkan `ukuran > indeks` **bertanda**: indeks negatif lolos → baca di luar vektor | tolak negatif (`slot_valid`) |
| 3 | Lambda progres membaca pemilik **sebelum** memeriksa slot; `stop` yang menulis pemilik = null lalu lambda berjalan → dereferensi null | pakai flag `detached` + cek slot |
| 4 | Worker membebaskan tugas bila pemilik null, padahal lambda progres yang sudah antre masih menunjuk tugas itu (use-after-free) | tugas **selalu** dibebaskan oleh lambda penutup di thread utama (FIFO ⇒ progres lebih dulu) |
| 5 | Tanpa mutex: `stop` (utama) dan worker menulis/membaca pemilik/flag bersamaan | `std::atomic` untuk batal/lepas/persen |
| 6 | Objek TJS (layer, opsi) di-`Release` dari thread worker saat tugas dibebaskan di worker | dilepas di thread utama |
| 7 | Layer penyimpan dipakai dari thread worker (`GetMainImage()` memanggil `0x80c848`) tanpa kunci | sama seperti biner (layer privat untuk tugas ini) |

## Pesan galat (string UTF-16)
| Alamat | Pesan | Dipakai |
|---|---|---|
| `0x14cb1e6` | `保存処理用レイヤの生成に失敗しました` ("gagal membuat layer pemrosesan simpan") | `CreateNew` Layer gagal (`startSaveLayerImage`) |
| `0x14cb1b8` | `保存処理用レイヤへの画像の複製に失敗しました` ("gagal menggandakan gambar ke layer pemrosesan simpan") | `assignImages` mengembalikan galat |
| `0x14cb126` | `deflate initialize` | `deflateInit` ≠ `Z_OK` (level `comp_lv`/argumen tidak sah) |
| `.bss 0x1af33a0` | pesan "specify layer" (`TVPSpecifyLayer`, ASUMSI) | objek bukan Layer (sama seperti bagian A) |

## Konversi parameter
- `tTJSVariant` dengan tipe `void` → kosong; `string` → salinan; **tipe lain → galat konversi** (`0xa0cd8c`) untuk nama berkas. Opsi: harus **objek** (selain itu galat konversi), `numparams` kurang → tak ada opsi.
- `numparams < 1` → `TJS_E_BADPARAMCOUNT` (`-1004`) pada `saveLayerImageTlg5`/`saveLayerImagePng` (dibaca langsung). Untuk tiga method `Window` (ncbind) arity tidak terbaca: **ASUMSI** — fork mewajibkan ≥ 1 parameter dan menerima 2–3 sebagai opsional.

## Urutan kanal piksel
Penyusun PNG menyalin byte piksel **apa adanya** dari memori layer ke baris PNG RGBA (tanpa penukaran). BTOA/log 2 menemukan memori layer = **R,G,B,A** (biru = byte[2]); dengan itu PNG
hasilnya **benar**. Bila log berikutnya menunjukkan B,G,R,A, PNG biner (dan fork) tertukar R/B. `tests/tjs/t_layerExSaveB.tjs` mengukurnya lewat uji pulang-pergi (`CASE Png.urutan_kanal`).
(Catatan: paragraf "Piksel = 32-bit B,G,R,A" di `layerExSave_common.md` bagian A adalah asumsi S11 yang kini diragukan; lihat koreksi di berkas itu.)

## Kode
`src/plugins/layerExSaveBCore.hpp` (`slot_acquire`, `slot_valid`, `is_png_ext`, `png_encode`), `src/plugins/layerExSave.cpp` (blok "Bagian B"). Uji: `tests/plugins/layerExSave/test_B_core.cpp` (66 pemeriksaan, ASan/UBSan, cek mutasi),
`tests/tjs/t_layerExSaveB.tjs`.
