# Spesifikasi: `Layer.turnAlpha3` (layerExAlpha.dll, Alpha-1)

Plugin `layerExAlpha.dll` · method `turnAlpha3` · implementasi `0x5d4f80` (172 B) · registrasi `0x5d43b0`. Hal umum: [layerExAlpha_umum.md](layerExAlpha_umum.md).
Nama parameter **diperkirakan** (binary tidak menyimpan nama).

## Tanda tangan
`this.turnAlpha3()` — tanpa argumen.

## Efek
Seluruh gambar: `A != 0 → A = 255 − A`; `A == 0` tetap 0 (warna tidak berubah).

## Kasus tepi
- **Tidak ada pemeriksaan `IsSoftware()` dan tidak ada jalur GPU**: selalu menulis ke buffer CPU lewat `GetMainImagePixelBufferForWrite()` (juga pada render manager GPU).
- Buffer null → fork melewatkan operasi (binary akan crash); `redraw()` tetap dipanggil.

## ASUMSI
Pada render manager GPU, apakah buffer CPU yang ditulis langsung tampil di layar tergantung implementasi `GetMainImagePixelBufferForWrite()` di fork — **ASUMSI**, harus dicek di S2/S13.

## Uji
Nilai tangan + vektor emas di `tests/plugins/layerExAlpha/` (inti-host + emulasi instruksi). NDK/perangkat: belum.
