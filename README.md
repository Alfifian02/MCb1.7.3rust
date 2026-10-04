# Minecraft b1.7.3 -> Rust (Android)

Port bertahap dari decomp MCP b1.7.3. Lihat `ROADMAP.md` untuk urutan dan status.

## Struktur
- `mc-core/`    logika dunia (tanpa GL/Android): RNG, noise, blok, chunk, cahaya, tabrakan, raycast, tick.
- `mc-android/` cangkang APK (menjalankan `mc_core::selftest` dan menampilkan hasilnya di layar).
- `tools/`      generator data dari jar asli (butuh JDK + Python di PC; hasilnya sudah ikut di repo).
- `.github/workflows/ci.yml` build + tes otomatis di GitHub.

## Cara mencoba hanya dengan Android

### Jalur A: APK lewat GitHub (disarankan)
1. Buat repositori baru di github.com (browser ponsel, mode "Situs desktop" bila perlu).
2. Unggah isi proyek. Cara paling mudah di ponsel adalah Termux (lihat jalur B) lalu `git push`.
   Unggah file satu-satu lewat web tidak mempertahankan folder.
3. Setelah push, buka tab **Actions**: job `test` menjalankan semua tes unit, job `apk` membangun APK.
4. Buka tab **Releases**, unduh `MinecraftB173Rust.apk`, lalu pasang (izinkan "pasang dari sumber tidak dikenal").
5. Buka aplikasi: baris hijau `[OK]` = lulus, merah `[GAGAL]` = ada bug (kirim foto layar ke saya).

### Jalur B: tes langsung di ponsel dengan Termux
```
pkg update && pkg install rust git unzip
unzip mc-rs.zip && cd mc-rs
cargo test            # hanya mc-core (default-members)
```
Kirim keluaran error/gagal ke saya bila ada. Termux tidak membangun APK; untuk itu pakai Jalur A.

## Catatan jujur
Kode ditulis tanpa kompilator di sisi saya (sandbox tanpa cargo/NDK). Kemungkinan ada error kompilasi
pertama; perbaikannya cepat bila Anda mengirim log dari Actions atau Termux.
