# startSaveLayerImage — spesifikasi (S12)

- `LayerExSave.dll` · kelas **Window** · impl `0x5ea458` (1.168 B), registrasi `0x5ea344` · konvensi: `layerExSave_B_common.md`.
- Tanda tangan (member `WindowSaveImage`, ncbind): `int startSaveLayerImage(tTJSVariant layer, const tjs_char *nama, tTJSVariant opsi)`; dari TJS: `win.startSaveLayerImage(layer [, nama [, opsi]])` → **indeks slot** (integer).
  Arti argumen (dibaca): `layer` = sumber yang disalin; `nama` = nama berkas **dan** akhiran nama layer penyimpan (null → string kosong); `opsi` = objek kamus (meta TLG / opsi PNG) atau bukan-objek (→ tak ada opsi).
  ⚠️ ASUMSI: arity ncbind tidak terbaca; fork mewajibkan ≥ 1 parameter.

## Langkah (urutan di biner)
1. **Slot:** cari entri null pertama pada `threads` (vektor di `this+8`). Ada → indeks itu. Tidak ada → indeks = ukuran lama, vektor diperpanjang 1 (`slot_acquire`). Terjadi sebelum apa pun yang bisa gagal: bila langkah 2–3 melempar, slot tetap null (vektor sudah tumbuh).
2. **Buat layer penyimpan:** `Window.primaryLayer` (`PropGet` pada objek Window) → `new Layer(window, primaryLayer)` (kelas `Layer` diambil lewat ekspresi `"Layer"`, `0x5e14a4`; `CreateNew` 2 parameter). Gagal/hasil negatif → `保存処理用レイヤの生成に失敗しました`.
3. `saveLayer.name = "saveLayer:" + nama` (string ASCII `0x14cb28c` digabung dengan `nama`).
4. **Gandakan gambar:** panggil fungsi `Layer.assignImages` (`0x5e813c`: ekspresi `"Layer.assignImages"`) dengan `this = layer penyimpan` dan parameter tunggal `layer` (yang diberikan pemanggil). Hasil negatif → `保存処理用レイヤへの画像の複製に失敗しました`. Fork memanggil `assignImages` sebagai anggota layer penyimpan (setara).
5. **Tugas:** `new` 0x70 B (`layerExSave_B_common.md`): pemilik = `this`, layer = layer penyimpan, nama, opsi (salinan variant), batal = 0, indeks, persen = void. `threads[indeks] = tugas`.
6. **Thread:** `std::thread(0x5e9db8, tugas)` lalu `detach()` (`0x14a1764`; terminate bila masih *joinable* tidak terjadi karena sudah dilepas). Hasil = **indeks**.
- Nilai kembali C++ = `w19` (indeks slot); ncbind meneruskannya sebagai hasil TJS (ASUMSI: perilaku ncbind standar).

## Worker `0x5e9db8` (896 B)
- Ambil layer penyimpan (objek), opsi (objek atau null), nama (**harus string**, selain itu galat konversi).
- `ext = lowercaseASCII(TVPExtractStorageExt(nama))` (`0x8eb564` mengembalikan akhiran **termasuk titik**; `0xa0ab28` hanya A–Z); `ext` null/kosong atau `ext != ".png"` (`0x14caefc`, dibandingkan `0x9b07d0`) → **TLG5**; `.png` → **PNG**.
- **PNG:** `0x5ea138(layer, nama, opsi, 0x5ea294, tugas)` — penyusun PNG di memori dengan callback progres (lihat `layerExSave_saveLayerImagePng.md`); hasil `true` = dibatalkan (file tidak ditulis).
- **TLG5:** `TVPCreateStream(nama, WRITE=1)` (`0x8ed170`) → layer `NativeInstanceSupport(GETINSTANCE, LayerClassID)` → `0x80c848` + `MainImage` → `TVPSaveAsTLG(null, stream, MainImage, "tlg5" (@0x1503412), opsi)` (`0x869f04`) → stream dilepas. **Tanpa callback progres.** ASUMSI: identifikasi `0x8ed170`/`0x869f04` dari tanda tangan + string `tlg5` (badan keduanya tidak dibaca; fork memanggil API-nya).
- Selesai: pemilik ≠ null → kirim lambda selesai ke thread utama; pemilik == null → bebaskan tugas di worker (bahaya #4).
- ⚠️ `batal` pada `onSaveLayerImageDone` adalah flag dari `cancelSaveLayerImage`, **bukan** hasil penyimpanan: pada jalur TLG5 file tetap tersimpan walau `batal = 1`.

## Callback ke skrip (pada objek Window)
- `onSaveLayerImageProgress(indeks, persen, layer, nama)` — hanya jalur PNG; dikirim hanya bila persen **berubah** dari nilai terakhir tugas (awal 0, jadi 0 tidak pernah dikirim) dan pemilik non-null.
- `onSaveLayerImageDone(indeks, batal, layer, nama)` — `batal` = 0/1 (integer).
- Keduanya dilewati bila `threads[indeks] != tugas` (mis. setelah `stopSaveLayerImage`). Layer penyimpan **tidak** di-invalidate oleh plugin.

## Deviasi sadar di fork
| Hal | Biner | Fork |
|---|---|---|
| Pengecualian di worker | `std::terminate` | ditangkap, dicatat di log, `Done(batal=1)` (`LAYEREXSAVE_WORKER_CATCH`) |
| Pelepasan tugas | di worker bila pemilik null | selalu di thread utama (lambda penutup) |
| Objek `Window` | tidak dipegang | variant penutup menahan `Window` selama tugas hidup |
| `ExitPlugin` | — | semua tugas dilepas (tanpa callback lagi); status jendela dibiarkan (kebocoran kecil) |

Kode: `StartSaveLayerImage`, `SaveWorker`, `OnPngProgress` (`layerExSave.cpp`); `slot_acquire`, `is_png_ext` (`layerExSaveBCore.hpp`). Uji: inti-host (slot, ekstensi), TJS (indeks 0,1,2; pemakaian ulang slot; Done sekali per tugas).
