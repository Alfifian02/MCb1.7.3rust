# Spesifikasi: `Layer.turnAlpha` (layerExAlpha.dll, Alpha-1)

Plugin `layerExAlpha.dll` · method `turnAlpha` · implementasi `0x5d4c04` (448 B) · registrasi `0x5d43b0`. Hal umum: [layerExAlpha_umum.md](layerExAlpha_umum.md).
Nama parameter **diperkirakan** (binary tidak menyimpan nama).

## Tanda tangan
`this.turnAlpha()` — tanpa argumen.

## Efek
Memilih jalur saat runtime lewat `IsSoftware()`, lalu `redraw()`.
- **CPU:** untuk seluruh gambar (`_width × _height`): `piksel = ~piksel & 0xFF000000` → **RGB = 0, A = 255 − A**.
- **GPU:** lihat di bawah (hanya **klip** layer).

## Kasus tepi
- Lebar/tinggi < 1 → tidak ada perubahan; `redraw()` tetap dipanggil.
- Perbedaan asli CPU (seluruh gambar) vs GPU (klip rect) dipertahankan.

## Jalur GPU
`CompileRenderMethod("turnAlpha", "void main() { gl_FragColor = vec4(0,0,0,1); }\n", 0 tekstur, 0)` (sekali, statis) lalu
`SetBlendFuncSeparate(ADD 0x8006, ZERO, ZERO, ONE_MINUS_DST_ALPHA 0x305, ZERO)` → RGB tujuan = 0, A = 1 − A_tujuan (cocok dengan CPU).
`OperateRect(method, GetTextureForRender(true, null), null, klip, tanpa tekstur sumber)`.

## Uji
Nilai tangan + vektor emas di `tests/plugins/layerExAlpha/` (inti-host + emulasi instruksi). NDK/perangkat: belum.
