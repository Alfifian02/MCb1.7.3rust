# Spesifikasi: `Layer.ColorVectorEffectAdd` (layerExAlpha.dll, Alpha-1)

Plugin `layerExAlpha.dll` · method `ColorVectorEffectAdd` · implementasi `0x5d5b8c` (336 B) · registrasi `0x5d43b0`. Hal umum: [layerExAlpha_umum.md](layerExAlpha_umum.md).
Nama parameter **diperkirakan** (binary tidak menyimpan nama).

## Tanda tangan
`this.ColorVectorEffectAdd(r0, r1, g0, g1, b0, b1, add)` (arg 1..7)

Pengelompokan (awal, akhir) per kanal disimpulkan dari pemakaian: arg1/arg2 → **R** (byte2), arg3/arg4 → **G** (byte1), arg5/arg6 → **B** (byte0); arg7 = `add` (langkah).

## Efek
Prakomputasi: `step = 255 / add`; `dR = (r1−r0)/step`, `dG = (g1−g0)/step`, `dB = (b1−b0)/step` (semua `sdiv`, bagi nol → 0).
Per piksel dengan `A ≠ 0`: `A' = clamp(A − add, 0..255)`; `t = (255 − A') / add`; `R = clamp(dR·t + r0)`, `G = clamp(dG·t + g0)`, `B = clamp(dB·t + b0)` (clamp 0..255); alfa diganti `A'`. Piksel `A == 0` tidak disentuh.
Efek: warna menurut alfa — makin transparan, makin dekat ke warna akhir.

## Kasus tepi
- `add = 0`: `step = 0`, semua pembagian = 0 → warna = (r0, g0, b0) tetap, A tidak berubah; **tanpa trap**.
- `add` negatif / ekstrem: aritmetika melingkar 32-bit; diuji di bawah UBSan.
- Tidak ada jalur GPU.

## Uji
Nilai tangan + vektor emas di `tests/plugins/layerExAlpha/` (inti-host + emulasi instruksi). NDK/perangkat: belum.
