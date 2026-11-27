# clipAlphaRect — spesifikasi perilaku

- Modul `layerExBTOA.dll`; nama @`0x14ca76c`; implementasi `0x5e2544` (3.040 B). Hanya satu jalur (tanpa `IsSoftware()`): piksel diakses lewat tekstur.

## Tanda tangan
`Layer.clipAlphaRect(dx, dy, src, sx, sy, w, h [, outside])` (nama perkiraan). `this` = tujuan, `src` = objek Layer sumber.
Urutan evaluasi di biner:
1. `numparams < 7` → `TJS_E_BADPARAMCOUNT` (-1004).
2. Konversi berurutan: `dx, dy` (integer), `src` (**harus objek**; bukan objek → galat konversi), `sx, sy, w, h` (integer, dipotong ke 32 bit).
3. `outside` bila `numparams ≥ 8`: void → tidak dipakai; selain itu integer `v`; **dipakai hanya bila `(uint64)v < 256`** (nilai lain, mis. -1 atau 256, → diperlakukan seperti absen); byte = `v & 0xFF`.
4. `w < 1` atau `h < 1` → `TJS_E_INVALIDPARAM` (-1003) (sebelum piksel disentuh).
5. `rowsOf(src, flag 0)` lalu `rowsOf(this, flag 1)`; bukan Layer → `TVPSpecifyLayer` ("Specify Layer class object"). Sumber memakai `GetScanLineForRead(i)` tekstur gambar utama
   (`SW,SH` = ukuran tekstur sumber); tujuan memakai `GetScanLineForWrite(0)`+`GetPitch()` dari `GetTextureForRender(false,nullptr)`.
6. Instance `this` lagi; **tanpa gambar utama, pitch 0, lebar klip < 1, atau tinggi klip ≤ 0 → `"dest must be Layer."`**.

## Geometri (semua aritmetika 32-bit melingkar; `cl,ct,cw,ch` = klip `this`)
Notasi: `a = w+sx`, `b = h+sy`. **Tidak terlihat** (cabang "skip") bila: `a<1`, `b<1`, `SW<=sx`, `SH<=sy`, lalu
```
dxrel = dx - cl;   ovR = max(0, a-SW);   wsrc = (sx<0 ? a : w) - ovR;   dxr = dxrel + (sx<0 ? -sx : 0);   dr = wsrc + dxr;   ... dr < 1  -> skip
dyrel = dy - ct;   ovB = max(0, b-SH);   hsrc = (sy<0 ? b : h) - ovB;   dyr = dyrel + (sy<0 ? -sy : 0);   db = hsrc + dyr;   ... db < 1  -> skip
dxr >= cw atau dyr >= ch -> skip
wf = (dxr<0 ? dr : wsrc) - max(0, dr-cw);   dxc = max(dxr,0);   hf = (dyr<0 ? db : hsrc) - max(0, db-ch);   dyc = max(dyr,0)
wf < 1 atau hf <= 0 -> skip
kolom sumber awal = max(sx,0) + max(0,-dxr);   baris sumber awal = max(sy,0) + max(0,-dyr)
```
Koordinat tujuan **relatif terhadap asal klip** (`dx - cl`); piksel ditulis di `(cl + dxc + x, ct + dyc + i)`.
(Beberapa penjaga redundan: `dxr>=cw`, `dr<1`, `SW<=sx` memberi hasil sama dengan pemeriksaan sesudahnya — uji mutasi: mutan ekuivalen.)

## Efek piksel (terlihat)
Untuk `i∈[0,hf), x∈[0,wf)`: `dstAlpha := (v + (v>>7)) >> 8` dengan `v = srcAlpha * dstAlpha` (hanya byte[3]; RGB tak berubah; ≈ ×/255, `255·x = x`).
Bila `outside` dipakai, area klip `cw×ch` di luar rect yang digambar diberi **alfa := outside**:
baris `[0,dyc)` penuh, baris `[hf+dyc, ch)` penuh, dan per baris kolom `[0,dxc)` (bila `dxc ≥ 1`) serta `[dxc+wf, cw)`.
Bila rect tidak terlihat: dengan `outside` → seluruh klip (`cw×ch`) diisi `outside`; tanpa `outside` → tidak ada perubahan.

## Pembaruan tampilan
Lewat **method TJS** `update` pada `this` (bukan `Update()` native; override skrip tetap terpanggil), 4 integer `(x, y, w, h)`:
- digambar & tanpa `outside`: `(cl + dxc, ct + dyc, wf, hf)`;
- ada `outside` (digambar atau tidak): `(cl, ct, cw, ch)`;
- tidak terlihat & tanpa `outside`: **tidak ada pemanggilan**. Kembali `TJS_S_OK`.

## Kekhasan biner (deviasi D3, D6 — lihat modul)
Alamat dasar tujuan dihitung `rows + (ct*pitch + cl*4)*8` (offset **byte** dipakai sebagai indeks larik pointer baris) → benar hanya bila klip berawal di (0,0).
Fork memakai asal klip yang dimaksud; hasil identik dengan biner untuk klip (0,0), dan itulah satu-satunya yang diuji emulasi.

## Uji
[SPEC] contoh tulis-tangan (rect parsial, `outside`, di luar) + [EMU] 2.500 kasus: dst ≤16×12, src ≤12×9, `dx,dy,sx,sy` negatif/besar, `w,h` ∈ [-1,16], pitch berpadding,
parameter ke-8 {absen, void, 0..255, 256/300/-1/2⁴⁰}, plus 700 kasus ekstrem int64/int32 (±2³¹, 2³²+3, …). Pelacak akses menegaskan tidak ada baca/tulis luar buffer;
kode kembali -1003 terverifikasi. Mutasi pada rumus inti membuat uji gagal (kecuali penjaga redundan di atas).
