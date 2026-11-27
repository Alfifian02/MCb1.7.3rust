# Spesifikasi: `Layer.MaskRect` (layerExAlpha.dll, Alpha-1)

Plugin `layerExAlpha.dll` · method `MaskRect` · implementasi `0x5d621c` (1384 B) · registrasi `0x5d43b0`. Hal umum: [layerExAlpha_umum.md](layerExAlpha_umum.md).
Nama parameter **diperkirakan** (binary tidak menyimpan nama).

## Tanda tangan
`this.MaskRect(dx, dy, src, sx, sy, sw, sh, mode)` — 8 argumen.

| # | Tipe | Arti |
|---|---|---|
| 1,2 | int | `dx, dy` — posisi tujuan pada `this` |
| 3 | Layer | `src` (bukan Layer → pengecualian **`Not Layer`**) |
| 4,5 | int | `sx, sy` — pojok sumber |
| 6,7 | int | `sw, sh` — ukuran |
| 8 | int | `mode` |

## Efek
Operasi pada **bidang alfa** wilayah `sw × sh`. `mode` (dicek **sebelum** `IsSoftware()`):
- **0:** `this.mainImage.CopyRect(dx, dy, src.mainImage, rect(sx, sy, sx+sw, sy+sh), plane = TVP_BB_COPY_MASK)` — salin bidang mask; jalur yang sama untuk CPU/GPU.
- **1 (tambah):** `A_t = min(A_t + A_s, 255)`; **2 (kurang):** `A_t = max(A_t + A_s − 255, 0)`; **3 (kali):** `A_t = (A_t · A_s) >> 8` (geser 8, bukan /255; 255×255 → 254).
- **lainnya:** log `layerExAlpha: noting type`, tanpa operasi.
Selalu diakhiri `redraw()` (juga setelah log mode tak dikenal).

## Kasus tepi
- Binary **tidak memeriksa batas** (menulis/membaca di luar gambar bila wilayah keluar); fork **memotong** wilayah ke kedua gambar (identik di dalam batas).
- `sw < 1` atau `sh < 1` → tak ada perubahan.
- Sumber dibaca lewat `GetMainImagePixelBufferForWrite()` (menandai sumber "diubah"), seperti binary.
- `src == this` diperbolehkan.

## Jalur GPU
Mode 1–3 saat render manager GPU (mode 0 dan tak dikenal seperti di atas). Tekstur sumber = `src.mainImage.GetTexture()`; `OperateRect(method, GetTextureForRender(true,null), null, rect(dx,dy,dx+sw,dy+sh), [(texSumber, rect(sx,sy,sx+sw,sy+sh))])`.
| mode | Render method | GLSL | `SetBlendFuncSeparate(func, srcRGB, dstRGB, srcA, dstA)` |
|---|---|---|---|
| 1 | `ExAlphaAdd` (Compile) | `gl_FragColor = texture2D(tex0, v_texCoord0)` | ADD, ZERO, ONE, ONE, ONE |
| 2 | `ExAlphaSub` (Compile) | `vec4(0,0,0,1-texture2D(tex0, v_texCoord0).a)` | REVERSE_SUBTRACT `0x800B`, ZERO, ONE, ONE, ONE |
| 3 | `AlphaMask` (GetOrCompile, hint statis) | sama dengan mode 1 | ADD, ZERO, ONE, ZERO, SRC_ALPHA `0x302` |
GPU mode 3 memakai perkalian float (`d·s/255`) sedangkan CPU `(d·s)>>8` → beda ≤ 1 tingkat; diterima, dicek di S13.

## ASUMSI
Teks `noting type` (salah eja) adalah teks asli binary (`0x14ca318`).

## Uji
Nilai tangan + vektor emas di `tests/plugins/layerExAlpha/` (inti-host + emulasi instruksi). NDK/perangkat: belum.
