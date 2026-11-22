# Spesifikasi: `Layer.AlphaAdd` (layerExAlpha.dll, Alpha-1)

Plugin `layerExAlpha.dll` · method `AlphaAdd` · implementasi `0x5d54a8` (308 B) · registrasi `0x5d43b0`. Hal umum: [layerExAlpha_umum.md](layerExAlpha_umum.md).
Nama parameter **diperkirakan** (binary tidak menyimpan nama).

## Tanda tangan
`this.AlphaAdd(add, flag)`

| # | Tipe | Arti |
|---|---|---|
| 1 | int | `add` (boleh negatif) |
| 2 | int | `flag` — hanya **bit 0** yang dipakai |

## Efek
Seluruh gambar: `A = clamp(A + add, 0, 255)`. Bila `flag & 1`: piksel dengan `A == 0` **dilewati** (tetap 0).

## Kasus tepi
- `add` sangat besar: penjumlahan 32-bit melingkar, lalu dipotong ke 0..255.
- Tidak ada jalur GPU.

## Uji
Nilai tangan + vektor emas di `tests/plugins/layerExAlpha/` (inti-host + emulasi instruksi). NDK/perangkat: belum.
