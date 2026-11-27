# fillByProvince — spesifikasi perilaku

- Modul `layerExBTOA.dll`; nama @`0x14ca788`; implementasi `0x5e31a8` (1.536 B). Dua jalur (`IsSoftware()`).

## Tanda tangan
`Layer.fillByProvince(province, color)` (nama perkiraan; param[0] = nomor province, param[1] = warna ARGB).
1. `numparams < 2` → `TJS_E_BADPARAMCOUNT` (-1004).
2. Konversi `province` lalu `color` ke integer (void → **0** di sini, bukan -1; objek/oktet → galat konversi). `value = province & 0xFF`;
   `color2 = swap_rb(color32)` = `(c & 0xFF00FF00) | ((c>>16)&0xFF) | ((c&0xFF)<<16)` (= `TVP_REVRGB`).
3. `0x5e14a4()` dipanggil dan hasilnya dibuang (tanpa efek).
4. Ambil instance → `"dest must be Layer."`.
5. **Kekhasan**: tanpa gambar utama, pitch 0, **lebar klip < 1 atau tinggi klip < 1** → `TVPThrowExceptionMessage(L"must be Layer.")` (@`0x14ca650`, ekor string `"dest must be Layer."`).
6. Pointer province (`GetProvinceImagePixelBuffer`, tidak dialokasikan!) dan pitch-nya. Province tidak ada → biner membaca `null+offset` (crash; D1).

## Jalur CPU (`IsSoftware()`)
Tujuan diambil dari tekstur (`rowsOf`, flag 1); `W,H` = ukuran **tekstur**. Pointer province dimulai di `(left, top)` klip, **tujuan tidak**:
```
untuk y di [0,H), x di [0,W):  jika province(left+x, top+y) == value  ->  pixel32(x,y) = color2     (penulisan 32-bit penuh, termasuk alfa dari color)
CallTJS: this.update(left, top, W, H)          // method TJS, bukan Update() native
```
Dengan klip bawaan (seluruh layer) ini pengisian penuh layer. Dengan klip lain, sampel province bergeser terhadap tujuan (kekhasan biner) dan bisa keluar buffer (D4).

## Jalur GPU
Render method `"FillByProvince"` (@`0x14ca820`), GLSL @`0x14ca82f`:
```
uniform vec4 u_color; uniform float opa;
vec4 s = texture2D(tex0, v_texCoord0);  s.a = step(abs(s.r - opa), 0.75 / 255.0);  s.rgb = u_color.rgb;  gl_FragColor = s;
```
Blend dibuat sekali: `SetBlendFuncSeparate(ADD, ONE_MINUS_SRC_ALPHA, SRC_ALPHA, ZERO, ONE)`. Tekstur `Gray` (8 bit) dibuat dari area klip province
(`CreateTexture2D(prov + top*pitch + left, pitch, cw, ch, Gray)`); `opa = value/255.0f`; `u_color = color2` (`SetParameterColor4B`);
tujuan `GetTextureForRender(method->IsBlendTarget(), &rect)` dengan rect = klip; `OperateRect`; `Update(klip, false)`; tekstur province di-`Release`.
⚠️ Dengan arti blend GL standar, `s.a = 1` (cocok) memberi `dst` tak berubah dan `s.a = 0` (tidak cocok) memberi `u_color` → **kebalikan jalur CPU** (keputusan terbuka #1 di modul).
Alfa tujuan tidak berubah (`srcA=ZERO, dstA=ONE`), sedangkan jalur CPU menulis alfa dari `color`.

## Uji
[SPEC] contoh (cocok/tidak cocok, sampel di luar buffer) + [EMU] 672 kasus CPU (province ∈ {0..3}, nilai {0..3, 259, -1}, klip penuh dan atas+{2,3} dengan province lebih tinggi,
padding), termasuk argumen update `(left, top, W, H)`. Klip dengan `left ≠ 0` tidak masuk vektor emas (biner membaca padding/di luar buffer). GPU belum diuji.
