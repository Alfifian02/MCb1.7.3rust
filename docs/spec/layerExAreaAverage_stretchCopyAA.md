# layerExAreaAverage — `Layer.stretchCopyAA`

- Modul: `layerExAreaAverage.dll` (string UTF-16 di `libgame.so` arm64 @ `0x14c04a2`)
- Fungsi registrasi: `0x56ac00` (140 B), hanya merujuk satu string TJS
- Sumber: rekonstruksi dari binary 1.3.9 (bukan source publik)

## Perilaku
Binary mengeksekusi satu ekspresi TJS saat modul dimuat:

```
Layer.stretchCopyAA = Layer.stretchCopy;
```

`stretchCopyAA` adalah **alias** dari `stretchCopy` bawaan inti. Parameter, mode interpolasi,
error, dan kasus tepi identik dengan `Layer.stretchCopy`. Tidak ada algoritma area-average
terpisah di plugin ini.

## Implementasi fork
`src/plugins/layerExAreaAverage.cpp`: callback `PostRegist` menjalankan ekspresi di atas;
callback `PreUnregist` menghapus anggota `stretchCopyAA` dari `Layer`.

**Waktu eksekusi di fork (koreksi S2):** callback `PostRegist` berjalan saat `Plugins.link("layerExAreaAverage.dll")`
(`ncbAutoRegister::LoadModule`), **bukan** saat `AllRegist()` di startup — lihat `plugin_registrasi.md`. Jadi `stretchCopyAA` tidak ada sebelum modul ditautkan.
Waktu di `libgame.so` 1.3.9 **belum diketahui** (ASUMSI sama); `tests/tjs/t_stretchCopyAA.tjs` mencatat `stretchCopyAA.ada_sebelum_link` untuk dibandingkan.

## Uji (7.2/7.3)
Skrip lengkap: `tests/tjs/t_stretchCopyAA.tjs`. Catatan S2: contoh lama memakai `stSoftBilinear`, yang **tidak ditemukan** di inti fork (`LayerBitmapIntf.h` hanya punya `stNearest`, `stFastLinear`, `stLinear`, …); diganti `stLinear`.
```
var a = new Layer(win, null); a.setImageSize(16,16); a.setSizeToImageSize();
var b = new Layer(win, null); b.setImageSize(8,8);  b.setSizeToImageSize();
a.fillRect(0,0,16,16,0xff336699);
b.stretchCopyAA(0,0,8,8,a,0,0,16,16,stLinear);
var c = new Layer(win, null); c.setImageSize(8,8); c.setSizeToImageSize();
c.stretchCopy(0,0,8,8,a,0,0,16,16,stLinear);
// harapan: hash piksel b == hash piksel c; dan Layer.stretchCopyAA === Layer.stretchCopy
```
