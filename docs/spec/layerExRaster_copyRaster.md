# Spesifikasi: `Layer.copyRaster` (layerExRaster.dll)

Sumber: disassembly `libgame.so` 1.3.9 arm64. Fungsi registrasi `0x5e6ac4`, implementasi `0x5e6bb0` (1.900 B).
Nama modul di binary: `layerExRaster.dll`. Nama parameter di bawah **diperkirakan** (binary tidak menyimpan nama parameter).

## Tanda tangan
`this.copyRaster(src, maxh, lines, cycle, time)`

| # | Tipe | Arti (perkiraan) | Dipakai sebagai |
|---|---|---|---|
| 0 | Layer | sumber | gambar yang digeser per baris |
| 1 | int | `maxh` | amplitudo maksimum (piksel) |
| 2 | int | `lines` | panjang gelombang dalam baris |
| 3 | int | `cycle` | periode animasi |
| 4 | int64 | `time` | tick |

Kurang dari 5 argumen → `TJS_E_BADPARAMCOUNT`. `src` bukan Layer → `TVPSpecifyLayer`
(binary asli membaca pointer tak terinisialisasi; ini perbaikan, bukan perilaku asli).

## Perilaku
1. Jika ukuran `this` ≠ ukuran `src` → **tidak melakukan apa pun** (tanpa error, tanpa redraw).
2. Area kerja = **clip rect** layer `this` (kiri, atas, lebar, tinggi). Tinggi < 1 → selesai.
3. Konstanta (dibaca dari `.rodata`): `2π = 6.283185307179586`, `-π/3 = -1.0471975511965976`.
4. `step = 2π / lines` · `tmod = time - trunc(time / (3·cycle)) · 3·cycle` (cycle = 0 → `tmod = time`)
5. `phase0 = step·clipTop + (step·tmod / cycle) · (tinggiGambar / 2) · (-π/3)`
6. Untuk baris `i = 0 .. clipH-1`: `phase = phase0 + i·step`, `s = trunc(sin(phase) · maxh)` (NaN → 0, saturasi seperti `fcvtzs`).
   - `s ≥ 0`: `dst[x + s] = src[x]` untuk `x ∈ [0, W−s)`; `s` piksel kiri `dst` **tidak diubah**.
   - `s < 0`: `dst[x] = src[x − s]` untuk `x ∈ [0, W+s)`; `|s|` piksel kanan **tidak diubah**.
   - Jumlah piksel < 1 → baris dilewati (fase tetap maju).
7. Akhir: layer di-update (redraw).

## Dua jalur (dipilih saat runtime oleh `IsSoftware()` pada render manager)
- **CPU** (software): salin piksel 32bpp seperti di atas.
- **GPU**: shader `ExRaster` dengan uniform `omega`, `phase`, `h`:
  `dx = sin(v_texCoord0.y·omega + phase)·h + v_texCoord0.x; gl_FragColor = texture2D(tex0, vec2(dx, v_texCoord0.y))`
  - `omega = step · tinggiTekstur`, `phase = phase0`, `h = maxh / lebarTekstur`.
  - Operasi: `OperateRect(method, texTujuan, nullptr, clipRect, [(texSumber, clipRect)])`.

## Kasus tepi (sesuai binary)
- `lines = 0` → `step = +inf`, fase NaN/inf → `s = 0` → **salinan biasa** (bukan error).
- `cycle = 0` → tidak ada pembagian integer-nol; fase NaN → salinan biasa.
- `|s| ≥ lebar` → baris dilewati.
- Jika `src` dan `this` memakai buffer yang sama, binary memakai jalur salin skalar saat tumpang-tindih; implementasi baru memakai `memmove`.

## ⚠️ Hal yang perlu diverifikasi di perangkat (langkah 7.3)
1. **Arah geser CPU vs GPU berlawanan** pada binary asli: CPU menggeser ke kanan untuk `sin > 0`, shader mengambil sampel di `x + h·sin` (geser ke kiri). Bisa jadi artefak orientasi tekstur; bandingkan keduanya pada APK 1.3.9.
2. Ukuran tekstur untuk GPU (`GetWidth` vs `GetInternalWidth`) — ukuran internal bisa berbeda bila tekstur dipadatkan.
3. Nama parameter (`maxh`, `lines`, `cycle`, `time`) adalah perkiraan.
