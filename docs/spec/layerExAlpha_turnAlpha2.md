# Spesifikasi: `Layer.turnAlpha2` (layerExAlpha.dll, Alpha-1)

Plugin `layerExAlpha.dll` · method `turnAlpha2` · implementasi `0x5d4dc4` (444 B) · registrasi `0x5d43b0`. Hal umum: [layerExAlpha_umum.md](layerExAlpha_umum.md).
Nama parameter **diperkirakan** (binary tidak menyimpan nama).

## Tanda tangan
`this.turnAlpha2()` — tanpa argumen.

## Efek
- **CPU:** `A ^= 0xFF` untuk seluruh gambar (A → 255 − A, **warna tetap**).
- **GPU:** seperti `turnAlpha` tetapi RGB dipertahankan.

## Kasus tepi
- Lebar/tinggi < 1 → tidak ada perubahan; `redraw()` tetap dipanggil.

## Jalur GPU
Render method `"turnAlpha2"` dengan GLSL yang sama dengan `turnAlpha`; blend `(ADD, ZERO, ONE, ONE_MINUS_DST_ALPHA, ZERO)` → RGB = tujuan, A = 1 − A_tujuan.

## Uji
Nilai tangan + vektor emas di `tests/plugins/layerExAlpha/` (inti-host + emulasi instruksi). NDK/perangkat: belum.
