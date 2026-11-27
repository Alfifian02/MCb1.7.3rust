# Spesifikasi: `Layer.ExMaskRect` (layerExAlpha.dll, Alpha-1)

Plugin `layerExAlpha.dll` · method `ExMaskRect` · implementasi `0x5d6784` (1820 B) · registrasi `0x5d43b0`. Hal umum: [layerExAlpha_umum.md](layerExAlpha_umum.md).
Nama parameter **diperkirakan** (binary tidak menyimpan nama).

## Tanda tangan
`this.ExMaskRect(dx, dy, src, arg4, sy, sw, sh, mode)` — 8 argumen.

| # | Tipe | Arti |
|---|---|---|
| 1,2 | int | `dx, dy` — posisi wilayah pada `this` (boleh negatif) |
| 3 | Layer | `src` (bukan Layer → **`Not Layer`**, dicek setelah pemeriksaan batas) |
| 4 | int | **tidak dipakai** binary (ASUMSI: sisa API, mungkin `sx`) |
| 5 | int | `sy` — baris awal sumber bila `dy ≥ 0` |
| 6,7 | int | `sw, sh` |
| 8 | int | `mode` 0 salin · 1 tambah · 2 kurang · 3 kali (sama dengan `MaskRect`) |

## Efek
Memproses **seluruh layer** (`_width × _height`), hanya CPU:
1. **Pemeriksaan awal:** bila `W < dx` atau `H < dy` → log (`...[Left]` bila `W ≤ dx`; `...[Top]` bila `H ≤ dy`), **tanpa perubahan, tanpa `redraw()`**.
2. Wilayah tujuan: kolom `x0 = max(dx,0)`, lebar `cw = (sw + min(dx,0) + x0 > W) ? W − x0 : sw + min(dx,0)`; baris `y0 = max(dy,0)`, tinggi `rows = (sh + min(dy,0) + y0 > H) ? H − y0 : sh + min(dy,0)`.
3. Baris `0 .. y0−1`: **alfa = 0** (seluruh lebar).
4. `mode > 3` (tak bertanda) → log `noting type`, **berhenti tanpa `redraw()`** (baris atas sudah dibersihkan).
5. Tiap baris wilayah: kolom `< x0` → alfa 0; kolom `x0..x0+cw−1` → `mask_op(mode, A_tujuan, A_sumber)`; kolom `≥ x0+cw` → alfa 0.
6. Baris di bawah wilayah → alfa 0. Lalu `redraw()`.
Sumber dibaca di **kolom `max(dx,0)+i`** dan **baris `(dy ≥ 0 ? sy : −dy) + j`** (arg4 tidak dipakai). Warna tidak pernah diubah.

## Kasus tepi
- Pesan log (teks asli): `layerExAlpha: Src is position for OutArea:[Left]`, `...[Top]`, `layerExAlpha: noting type`.
- **Bug asli (tidak ditiru):** bila `sh + min(dy,0) < 0` atau lebar salin negatif, binary menulis melewati ujung baris/buffer. Fork: mengisi 0 hanya di dalam gambar; hasil di dalam buffer identik (dibuktikan emulasi), byte padding antar-baris tidak disentuh.
- Bacaan di luar gambar sumber dianggap alfa 0 (binary: membaca memori liar).
- `dx < 0`: kolom sumber mulai di 0, bukan `−dx` (perilaku binary; kemungkinan bug asli, tetap ditiru).

## ASUMSI
Arti arg4 dan maksud posisi sumber di atas adalah **ASUMSI**; perilaku piksel persis dari binary dan emulasi.

## Uji
Nilai tangan + vektor emas di `tests/plugins/layerExAlpha/` (inti-host + emulasi instruksi). NDK/perangkat: belum.
