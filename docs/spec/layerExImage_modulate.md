# Spesifikasi: `Layer.modulate` (layerExImage.dll)

Bagian umum: [`layerExImage_light.md`](layerExImage_light.md). Implementasi **`0x592cf0`** (1.908 B), thunk `0x596054`. Kode: `modulate_apply`, `modulate_pixel`.

## Tanda tangan
`this.modulate(hue, sat, luminance)` — **tiga argumen wajib**, integer (int32). Nama dari uniform shader; urutan dari thunk (`w1=hue, w2=sat, w3=luminance`).

## Perilaku CPU (seluruh gambar; **tanpa pintasan** untuk nol — tetap membulatkan RGB→HSL→RGB)
Konversi parameter (float 32-bit lalu dilebarkan ke double): `hm = hue/360.0f`, `sm = sat/100.0f`, `lm = luminance/100.0f`.
Per piksel (double; `R = byte2, G = byte1, B = byte0`, tiap kanal `/255.0`):
1. `max`, `min`, `delta = max−min`, `sum = max+min`, `l = sum·0.5`. `delta == 0` → `s = 0`, `h = 0`.
   Selain itu `den = (l < 0.5) ? sum : 2−sum`; `hh = (G−B)/delta` bila `R==max`, `(B−R)/delta + 2` bila `G==max`, selain itu `(R−G)/delta + 4`; `s = delta/den`; `h = hh/6`.
2. `h += hm`; lipat ke [0,1] (naik 1 selama < 0; turun 1 selama > 1). Fork memakai bentuk tertutup (`x + ceil(−x)`, `x − (ceil(x)−1)`) yang **bit-identik**
   (pengurangan 1.0 eksak untuk |x| < 2²³) dan menghindari ratusan ribu iterasi per piksel untuk `hue` ekstrem.
3. `s' = s + ((sm>0) ? (1−s) : s) · sm` ; `l' = l + ((lm>0) ? (1−l) : l) · lm` (tanpa penjepitan; bisa keluar dari [0,1]).
4. `s' == 0` → ketiga kanal = `fcvtzs(l'·255)` (byte rendah; **melingkar** bila di luar 0..255).
5. Selain itu HSL→RGB standar (`q = l'>0.5 ? l'+s'−l's' : l'(1+s')`, `p = 2l'−q`, kanal R pada `h+1/3`, G pada `h`, B pada `h−1/3`, potongan 1/6, 1/2, 2/3,
   urutan perkalian `((q−p)·t)·6` persis binary). Hasil `fcvtzs(f·255)` disimpan sebagai byte rendah. Alfa utuh. Lalu `redraw()`.

## GPU
Shader `LayerEx_Modulate`: `hsl.r = clamp(hsl.r + hue, 0, 1)`; `hsl.g += (S1 − hsl.g)·sat`; `hsl.b += (L1 − hsl.b)·luminance`.
Uniform: `hue = hue/360`, `sat = sat/100`, `luminance = luminance/100`, `S1 = sat>0 ? 1 : 0`, `L1 = luminance>0 ? 1 : 0`.

## ⚠️ Perbedaan CPU vs GPU yang terbaca
- `hue`: CPU **melipat** (wrap), GPU **menjepit** ke [0,1].
- `sat`/`luminance` **negatif**: CPU mengurangi (`s·(1+sm)`), sedangkan rumus GPU `g += (0 − g)·sm` dengan `sm < 0` justru **menaikkan** nilai. Arah berlawanan; kemungkinan bug asli di salah satu jalur.
- Urutan byte R/B berbeda dari `colorize` (lihat sana).

## Kasus tepi
`delta == 0` dengan `sat > 0` → abu-abu menjadi berwarna pada `hue` yang diminta. `|hue|` besar (mis. 2³¹) valid.

## Uji
7 kasus golden (hue/sat/lum campuran, ±, 720, 300/−300) + analitik. Seluruh fungsi asli dijalankan untuk gambar acak 9×5; 0 selisih. **Belum**: GPU, NDK, perangkat.
