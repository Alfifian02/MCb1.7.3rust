# copyBottomBlueToTopAlpha — spesifikasi perilaku

- Modul `layerExBTOA.dll`; nama @`0x14ca66e`; implementasi `0x5e1b58` (600 B). Struktur identik dengan `copyRightBlueToLeftAlpha` (bagian bawah → atas).

## Tanda tangan
`Layer.copyBottomBlueToTopAlpha()` — tanpa parameter; `TJS_S_OK`; instance gagal → `"dest must be Layer."`.

## Efek piksel
Alfa (byte[3]) tiap piksel di **separuh atas** := byte[2] piksel di baris `y + half` pada kolom yang sama. `half = H/2` (bertanda).
**CPU**: `untuk y di [0,half): untuk x di [0,W): p(x,y).byte[3] = p(x,y+half).byte[2]` (hanya bila W ≥ 1 dan H ≥ 2);
flag `+0x261 := 1` (tidak ditiru, D7); `Update(rect(0, 0, W, half), false)`.
**GPU**: `half = H >> 1` (tak bertanda); render method yang sama `"CopyAlphaFromBlue"` (satu objek di-cache untuk kedua method, blend sama);
rect sumber `(0, half, W, H & ~1)`, rect tujuan `(0, 0, W, half)`; `Update((0, 0, W, half), false)`.

## Kasus tepi
`H < 2` → piksel tidak berubah tetapi `Update` tetap dipanggil. Baris ganjil terakhir (H ganjil) tidak dipakai/diubah. Padding pitch tidak disentuh.

## Uji
[SPEC] contoh tulis-tangan + [EMU] 154 kasus (jalur CPU asli di emulator). GPU belum diuji.
