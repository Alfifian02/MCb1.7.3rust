# copyRightBlueToLeftAlpha — spesifikasi perilaku

- Modul `layerExBTOA.dll` (lihat [`layerExBTOA_module.md`](layerExBTOA_module.md)); nama @`0x14ca614`; implementasi `0x5e16e8` (596 B).

## Tanda tangan
`Layer.copyRightBlueToLeftAlpha()` — **tanpa parameter**; `numparams`/`param` tidak dibaca. Mengembalikan `TJS_S_OK`; `result` tidak ditulis.
Gagal mengambil instance → `"dest must be Layer."`.

## Efek piksel
Alfa (byte[3]) setiap piksel di **separuh kiri** := byte[2] ("biru") piksel pada posisi yang sama di **separuh kanan**.
`half = W/2` (pembagian bertanda; untuk W ganjil kolom terakhir tidak dipakai dan tidak berubah).

**Jalur CPU** (`IsSoftware()` benar): baris/lebar/tinggi dari **tekstur** (`rowsOf` flag 1; di software = ukuran gambar).
```
untuk y di [0,H): untuk x di [0,half):  p(x,y).byte[3] = p(x+half,y).byte[2]       (hanya bila H >= 1 dan W >= 2)
flag byte layer+0x261 := 1  (bukan ImageModified; tidak ditiru di fork, D7);  Update(rect(0, 0, half, H), false)
```
**Jalur GPU**: `W,H = GetImageWidth/Height`; `half = W >> 1` (tak bertanda). Render method `"CopyAlphaFromBlue"` (@`0x14ca7a6`), GLSL @`0x14ca7b8`:
`vec4 s = texture2D(tex0, v_texCoord0); s.a = s.b; gl_FragColor = s;`. Blend (dibuat sekali, cache global):
`SetBlendFuncSeparate(GL_FUNC_ADD, ZERO, ONE, ONE, ZERO)` → RGB tujuan tetap, alfa tujuan := alfa shader.
Sumber = tekstur gambar utama `this` (`+0x58`), rect sumber `(half, 0, W & ~1, H)`; tujuan `GetTextureForRender(false, nullptr)`, rect tujuan `(0, 0, half, H)`;
`OperateRect(method, dtex, nullptr, dstRect, [src, srcRect])`; lalu `Update((0, 0, half, H), false)` (tanpa menyetel flag).

## Kasus tepi
- `W < 2`: tidak ada piksel berubah, tetapi `Update((0,0,half,H))` tetap dipanggil (`half` = 0). `H = 0`: idem (rect kosong).
- Padding pitch tidak pernah disentuh (diverifikasi: hash seluruh buffer termasuk padding).
- Tidak ada validasi ukuran; tidak ada pemotongan klip (area = seluruh tekstur, **bukan** `ClipRect`).

## Uji
`tests/plugins/layerExBTOA/test_core.cpp`: [SPEC] contoh tulis-tangan (lebar genap/ganjil/1) + [EMU] 154 kasus dari eksekusi fungsi asli (W ∈ {1..9,33,64}, H ∈ {0..5,9}, pitch +0/+12 B).
Jalur GPU: belum diuji (butuh perangkat).
