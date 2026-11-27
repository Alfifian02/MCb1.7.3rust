# Spesifikasi: `Layer.light` (layerExImage.dll) — dan bagian umum layerExImage

Sumber: disassembly `libgame.so` 1.3.9 arm64, dibaca langsung (capstone, `script/k2dis.py`) dan dijalankan di emulator (`script/k2emu_unicorn.py`).
Modul: **`layerExImage.dll`** (string UTF-16 terkonfirmasi di `0x14c5b92`). Registrasi enam method di satu fungsi `0x594bf4` (1.176 B).
`light`: implementasi **`0x5920b4`** (1.060 B), thunk pemanggil `0x5956f0` → `0x59579c`. Kode: `layerExImageCore.hpp` (`light_lut`, `light_apply`).

## Bagian umum (berlaku untuk enam method layerExImage)
- **Kelas**: `layerExImage : layerExBase_GL`. Tata letak yang dipakai kode: `+0x08` `tTJSNI_Layer*`, `+0x10` lebar, `+0x14` tinggi,
  `+0x18` buffer, `+0x20` pitch (int32), `+0x24` clipLeft, `+0x28` clipTop, `+0x2c` clipWidth, `+0x30` clipHeight. Cocok dengan `layerExBase_GL` di fork.
- **Instance**: dibuat sekali per objek Layer (`"Not Layer"` bila bukan Layer, `0x5959b0`), dan **`reset()` (vtable[0]) dipanggil pada setiap akses**
  (ukuran/buffer/clip disegarkan sebelum method jalan). Fork: hook `NCB_GET_INSTANCE_HOOK` memanggil `obj->reset()`.
- **Pemanggil (thunk ncbind)**: `membername` non-null → `TJS_E_MEMBERNOTFOUND` (−1001); `objthis` null → `TJS_E_NATIVECLASSCRASH` (−1008);
  argumen kurang → `TJS_E_BADPARAMCOUNT` (−1004). Argumen integer = `AsInteger` dipotong ke int32; real = `AsReal`. Argumen berlebih diabaikan.
  Nama parameter **tidak disimpan** di binary; nama di bawah diambil dari nama uniform shader (urutan argumen dari thunk).
- **Dua jalur**, dipilih `IsSoftware()` (`0x848bdc`) pada render manager:
  - **CPU**: memproses **seluruh gambar** `lebar × tinggi` (clip **diabaikan**), piksel 4 byte, **byte 3 (alfa) tidak pernah diubah**
    (kecuali `gaussianBlur`), lalu `redraw()` (vtable[1] → `Update(clipRect)`).
  - **GPU**: render method `LayerEx_*` (GLSL di binary) pada tekstur gambar utama; rect = `(clipLeft, clipTop, clipLeft+lebar, clipTop+tinggi)`
    (ukuran **gambar** di titik asal clip, bukan ukuran clip); **tidak ada `Update()`** (fork: saklar `LAYEREXIMAGE_GPU_UPDATE`).
  - Method GPU: `light`, `colorize`, `modulate`, `noise`, `generateWhiteNoise`. `gaussianBlur` jalur non-software memakai box blur (lihat spesifikasinya).
- **Aritmetika float**: harus tanpa FMA/kontraksi (identik bit dengan binary). Inti mematikan kontraksi (`#pragma`) dan memisahkan tiap operasi.
- **Urutan byte**: lihat catatan di `layerExImage_colorize.md` dan `layerExImage_modulate.md` (kedua method memperlakukan R/B secara berbeda).
- **Pembanding**: tiap method CPU diuji dengan menjalankan **fungsi asli** di emulator dan membandingkan keluaran piksel-per-piksel (`tests/plugins/layerExImage/golden.inc`).

## Tanda tangan
`this.light(brightness, contrast)` — kedua argumen **wajib** (< 2 → `TJS_E_BADPARAMCOUNT`), keduanya integer (int32).

## Perilaku
1. `brightness == 0 && contrast == 0` → **tidak melakukan apa pun** (kembali sebelum cek jalur; tanpa redraw).
2. CPU: bangun LUT 256 entri (float 32-bit, tiap operasi dibulatkan terpisah):
   `k = (contrast + 100) / 100.0f` · `bias = (float)(brightness + 128)` · `v = (float)(i − 128) · k + bias` · `iv = fcvtzs(v)` (memotong ke nol, saturasi) ·
   `lut[i] = iv > 0 ? min(iv, 255) : 0`. (`contrast + 100` dan `brightness + 128` dihitung int32 melingkar.)
3. Untuk tiap piksel seluruh gambar: `B,G,R = lut[B], lut[G], lut[R]` (byte 0,1,2); byte 3 tetap. Lalu `redraw()`.
4. GPU: shader `LayerEx_Light`: `rgb = (rgb − 0.5)·contrast + brightness`, uniform `brightness = (brightness+128) · 0.00390625f` (**1/256**, bukan 1/255) dan
   `contrast = (contrast+100) / 100.0f`.

## Kasus tepi
- `(0,0)` → LUT identitas, tetapi binary tidak sampai ke sana (no-op awal).
- `contrast = −100` → `k = 0` → semua kanal = `trunc(brightness + 128)` terjepit 0..255.
- Nilai ekstrem (`int32` min/maks) → hasil saturasi `fcvtzs`; tidak ada crash.
- Perbedaan kecil CPU vs GPU: CPU memotong (trunc) ke integer; GPU bekerja float lalu dibulatkan oleh tekstur. Dan GPU memakai 1/256 untuk brightness sementara LUT CPU memakai `+128` penuh — keduanya sama di tengah rentang, bisa beda ±1.

## Uji
`tests/plugins/layerExImage/test_core.cpp`: 6 kasus golden dari fungsi asli (emulasi) + analitik (identitas, kontras −100, alfa utuh). **Belum**: jalur GPU, build NDK, perangkat.
