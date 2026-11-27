# Spesifikasi: `Layer.AlphaAdd2` (layerExAlpha.dll, Alpha-1)

Plugin `layerExAlpha.dll` · method `AlphaAdd2` · implementasi `0x5d5708` (196 B) · registrasi `0x5d43b0`. Hal umum: [layerExAlpha_umum.md](layerExAlpha_umum.md).
Nama parameter **diperkirakan** (binary tidak menyimpan nama).

## Tanda tangan
`this.AlphaAdd2(add, thr)` — `add`: int; `thr`: int (ambang bawah, bertanda).

## Efek
Seluruh gambar: `v = A + add`; **bila `v < thr` atau `v < 0` → A = 0**; selain itu `A = min(v, 255)`. Warna tidak berubah.

## Kasus tepi
- `thr` negatif: hanya syarat `v < 0` yang berlaku.
- Tidak ada jalur GPU.

## Uji
Nilai tangan + vektor emas di `tests/plugins/layerExAlpha/` (inti-host + emulasi instruksi). NDK/perangkat: belum.
