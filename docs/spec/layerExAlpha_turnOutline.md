# Spesifikasi: `Layer.turnOutline` (layerExAlpha.dll, Alpha-1)

Plugin `layerExAlpha.dll` · method `turnOutline` · implementasi `0x5d502c` (172 B) · registrasi `0x5d43b0`. Hal umum: [layerExAlpha_umum.md](layerExAlpha_umum.md).
Nama parameter **diperkirakan** (binary tidak menyimpan nama).

## Tanda tangan
`this.turnOutline()` — tanpa argumen.

## Efek
Seluruh gambar: warna dibuang (RGB = 0); `A == 255` → seluruh piksel = 0 (A = 0); `A < 255` → A tetap. Efeknya: hanya tepi semi-transparan yang tersisa (garis tepi), area padat menjadi kosong.

## Kasus tepi
- Seperti `turnAlpha3`: tanpa jalur GPU, selalu CPU.

## ASUMSI
Maksud "outline" disimpulkan dari nama; perilaku piksel di atas persis dari binary.

## Uji
Nilai tangan + vektor emas di `tests/plugins/layerExAlpha/` (inti-host + emulasi instruksi). NDK/perangkat: belum.
