# Spesifikasi: `Layer.AlphaColorRect` (layerExAlpha.dll, Alpha-1)

Plugin `layerExAlpha.dll` · method `AlphaColorRect` · implementasi `0x5d5204` (208 B) · registrasi `0x5d43b0`. Hal umum: [layerExAlpha_umum.md](layerExAlpha_umum.md).
Nama parameter **diperkirakan** (binary tidak menyimpan nama).

## Tanda tangan
`this.AlphaColorRect(r, g, b)`

| # | Tipe | Arti | Byte piksel |
|---|---|---|---|
| 1 | int | `r` | byte2 (R) |
| 2 | int | `g` | byte1 (G) |
| 3 | int | `b` | byte0 (B) |

## Efek
Seluruh gambar (bukan klip, meski namanya "Rect"): untuk tiap kanal, bila nilainya **≠ −1** maka disimpan (8 bit rendah, mis. 300 → 44); bila **= −1** kanal dibiarkan. **Alfa tidak pernah disentuh.**

## Kasus tepi
- Semua −1 → tidak ada perubahan (tetap `redraw()`).
- Nilai di luar 0..255 dipotong ke 8 bit rendah (tanpa saturasi).
- Tidak ada jalur GPU.

## Uji
Nilai tangan + vektor emas di `tests/plugins/layerExAlpha/` (inti-host + emulasi instruksi). NDK/perangkat: belum.
