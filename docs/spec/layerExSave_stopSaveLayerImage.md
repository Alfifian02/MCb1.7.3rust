# stopSaveLayerImage — spesifikasi (S12)

- `LayerExSave.dll` · kelas **Window** · impl `0x5eaa3c` (52 B; dibaca penuh dari byte) · konvensi: `layerExSave_B_common.md`.
- Tanda tangan: `void stopSaveLayerImage(int indeks)`.
- Perilaku: bila `ukuran(threads) > indeks` (bertanda) dan `threads[indeks] != null`: `tugas.pemilik = null` (`+0`), `tugas.batal = 1` (`+0x44`), `threads[indeks] = null`. Selain itu tidak melakukan apa pun.
- Efek: tugas **dilepas** dari jendela — slot langsung bebas dan dipakai ulang oleh `startSaveLayerImage` berikutnya; worker terus berjalan sampai selesai (PNG berhenti karena batal = 1) tetapi **tidak ada callback lagi** (`onSaveLayerImageProgress`/`Done` dilewati) dan, di biner, worker membebaskan tugasnya sendiri.
- ⚠️ Bahaya biner: lambda progres yang sudah antre membaca pemilik (null) sebelum memeriksa slot → dereferensi null; tugas bisa dibebaskan di worker sementara lambda masih menunjuknya (`layerExSave_B_common.md`, #3–#4). Indeks negatif: baca di luar vektor (bug yang sama dengan `cancel`).
- Fork: flag `detached` + `cancelled`, slot = null; tugas tetap dibebaskan oleh lambda penutup di thread utama.
- Tidak memiliki nilai kembali. Kode: `StopSaveLayerImage`.
