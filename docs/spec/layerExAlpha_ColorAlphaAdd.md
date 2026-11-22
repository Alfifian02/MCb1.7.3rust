# Spesifikasi: `Layer.ColorAlphaAdd` (layerExAlpha.dll, Alpha-1)

Plugin `layerExAlpha.dll` · method `ColorAlphaAdd` · implementasi `0x5d5950` (272 B) · registrasi `0x5d43b0`. Hal umum: [layerExAlpha_umum.md](layerExAlpha_umum.md).
Nama parameter **diperkirakan** (binary tidak menyimpan nama).

## Tanda tangan
`this.ColorAlphaAdd(rmax, gmax, bmax, add)`

| # | Arti | Byte |
|---|---|---|
| 1 | batas atas R | byte2 |
| 2 | batas atas G | byte1 |
| 3 | batas atas B | byte0 |
| 4 | `add` (int, boleh negatif) | — |

## Efek
Seluruh gambar, per piksel: `kanal = min(kanal + add, maks)` (perbandingan **bertanda**, **tanpa batas bawah**: hasil negatif disimpan sebagai 8 bit rendah/melingkar); `A = clamp(A − add, 0, 255)` (`A − add ≤ 0 → 0`).

## Kasus tepi
- `add` positif menambah warna dan mengurangi alfa; negatif sebaliknya.
- Tidak ada jalur GPU.

## Uji
Nilai tangan + vektor emas di `tests/plugins/layerExAlpha/` (inti-host + emulasi instruksi). NDK/perangkat: belum.
