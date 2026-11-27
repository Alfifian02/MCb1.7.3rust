# cancelSaveLayerImage — spesifikasi (S12)

- `LayerExSave.dll` · kelas **Window** · impl `0x5eaa14` (40 B; dibaca penuh dari byte) · konvensi: `layerExSave_B_common.md`.
- Tanda tangan: `void cancelSaveLayerImage(int indeks)`.
- Perilaku: bila `ukuran(threads) > indeks` (perbandingan **bertanda**) dan `threads[indeks] != null` → `tugas.batal = 1` (byte `+0x44`). Selain itu tidak melakukan apa pun. Tidak mengubah slot dan tidak menghentikan thread.
- Efek: callback progres (`0x5e9c38`) mengembalikan flag itu → penyusun PNG berhenti di putaran zlib berikutnya (file **tidak** ditulis); jalur TLG5 tidak memeriksa flag (file tetap tersimpan).
  `onSaveLayerImageDone` tetap dikirim (dengan `batal = 1`) karena slot masih terisi.
- ⚠️ **Bug biner:** indeks negatif lolos pemeriksaan (bertanda) lalu membaca `threads[indeks]` di luar vektor. Fork: `slot_valid` menolak negatif (tidak berbuat apa pun).
- Tidak memiliki nilai kembali. Kode: `CancelSaveLayerImage`, `slot_valid`.
