# saveLayerImageTlg5 — spesifikasi (S12)

- `LayerExSave.dll` · kelas **Layer** · impl `0x5ec768` (516 B; dibaca penuh dari byte) · konvensi: `layerExSave_B_common.md`.
- Tanda tangan: `Layer.saveLayerImageTlg5(nama [, opsi])`; fungsi mentah `tjs_error (result, numparams, param, objthis)`; `numparams < 1` → `-1004`. Tidak ada nilai kembali, tidak memanggil `Update()`.
- Parameter: `nama` = string atau `void` (kosong); tipe lain → galat konversi. `opsi` (bila `numparams ≥ 2`) harus **objek**, selain itu galat konversi; diteruskan sebagai `meta` (kamus tag TLG).
- Langkah: (1) `TVPCreateStream(nama, WRITE)` (`0x8ed170`) **lebih dulu** — layer yang tak sah baru ketahuan setelahnya, jadi berkas sudah terbentuk/terpotong; (2) `NativeInstanceSupport(GETINSTANCE, LayerClassID)` gagal → pesan "specify layer"
  (stream dilepas lewat jalur pembersihan); (3) `0x80c848(ni)` lalu `MainImage = [ni+0x118]`; (4) `TVPSaveAsTLG(nullptr, stream, MainImage, "tlg5", opsi)` (`0x869f04`; mode string `0x1503412`); (5) stream dilepas (`vfn#6`, destruktor/`Destruct`).
- Pengecualian dari penyimpan merambat ke skrip (stream dilepas). Cabang non-PNG pada worker `startSaveLayerImage` menjalankan urutan yang sama.
- ASUMSI: identitas `0x8ed170` (= `TVPCreateStream` mode tulis) dan `0x869f04` (= `TVPSaveAsTLG`) dari tanda tangan panggilan + literal `tlg5`; badan keduanya tidak dibaca. Fork memakai API aslinya.
- Kode: `SaveLayerImageTlg5`, `SaveTlg5File`. Uji: TJS (pulang-pergi `.tlg`, galat tanpa argumen).
