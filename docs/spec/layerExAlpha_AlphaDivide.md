# Spesifikasi: `Layer.AlphaDivide` (layerExAlpha.dll, Alpha-1)

Plugin `layerExAlpha.dll` · method `AlphaDivide` · implementasi `0x5d5400` (168 B) · registrasi `0x5d43b0`. Hal umum: [layerExAlpha_umum.md](layerExAlpha_umum.md).
Nama parameter **diperkirakan** (binary tidak menyimpan nama).

## Tanda tangan
`this.AlphaDivide(th)` — `th`: int bertanda (ambang).

## Efek
Per **baris**: `v = (A piksel pertama baris ≥ th) ? 255 : 0` (perbandingan **bertanda**), lalu **seluruh alfa baris** = `v`.

## Kasus tepi
- Hanya alfa piksel pertama tiap baris yang dibandingkan — hampir pasti bug asli (maksud wajar: per piksel). Fork **meniru binary**; `-DLAYEREX_ALPHADIVIDE_PER_PIXEL=1` memberi versi per piksel.
- Tidak ada jalur GPU.

## ASUMSI
Apakah bug ini terlihat di game nyata: belum diketahui (cek di S13).

## Uji
Nilai tangan + vektor emas di `tests/plugins/layerExAlpha/` (inti-host + emulasi instruksi). NDK/perangkat: belum.
