# Spesifikasi: `Layer.BlackForAlpha` (layerExAlpha.dll, Alpha-1)

Plugin `layerExAlpha.dll` · method `BlackForAlpha` · implementasi `0x5d57cc` (200 B) · registrasi `0x5d43b0`. Hal umum: [layerExAlpha_umum.md](layerExAlpha_umum.md).
Nama parameter **diperkirakan** (binary tidak menyimpan nama).

## Tanda tangan
`this.BlackForAlpha()` — tanpa argumen.

## Efek
Seluruh gambar: `A = (R + G + B) / 3` (pembagian bulat ke bawah); RGB = 0. Mengubah citra abu-abu/warna menjadi alfa.

## Kasus tepi
- Alfa lama dibuang. Tidak ada jalur GPU.

## Uji
Nilai tangan + vektor emas di `tests/plugins/layerExAlpha/` (inti-host + emulasi instruksi). NDK/perangkat: belum.
