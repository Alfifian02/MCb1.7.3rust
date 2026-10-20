# Mode debug LoD

Alat untuk mencari tahu **dari mana** sebuah artefak LoD berasal. Mode ini mewarnai setiap face LoD menurut apa face itu, sehingga satu screenshot cukup untuk membedakan skirt, dinding, permukaan atas, batas step, dan tepi tile.

Status: ditulis tanpa Rust toolchain. Belum dikompilasi dan belum dijalankan; jalankan `cargo test --lib --release lod` dulu.

---

## 1. Cara memakai

1. Masuk ke dunia overworld. LoD tidak ada di Nether, dan di sana tombolnya hanya menampilkan "LoD debug: off".
2. Buka menu pause.
3. Ketuk tombol **"LoD debug"** di bawah tombol Pregen (kiri atas).
4. Setiap ketukan pindah ke mode berikutnya, berurutan: mati, 1, 2, 3, kembali mati.
5. Kembali ke permainan dan tunggu sebentar. Semua tile LoD dibuang dan dibangun ulang dalam mode baru, jadi pemandangan jauh kosong dulu lalu terisi dari yang terdekat.
6. Ambil screenshot. Untuk perbandingan, ambil dari titik dan arah pandang yang sama di setiap mode.

Mode tidak tersimpan. Setelah aplikasi dimulai ulang, mode kembali mati.

---

## 2. Empat mode

Teks pada tombol menunjukkan mode yang aktif.

| Mode | Teks tombol | Yang diwarnai |
|---|---|---|
| mati | `LoD debug: off` | Tampilan normal. |
| 1 | `LoD dbg 1: step g1 y2 o4 r8` | Warna menurut **step** tile. |
| 2 | `LoD dbg 2: top wht wall blu skirt pnk` | Warna menurut **jenis face**. |
| 3 | `LoD dbg 3: tiles red/blue` | Tile **berselang-seling merah dan biru**. |

### Mode 1: per step

Seluruh face satu tile memakai satu warna sesuai step-nya (jarak antar sel, dalam blok):

| Warna | Step | Kira-kira jarak dari pemain (`step_for`) |
|---|---|---|
| hijau | 1 | dekat (sampai 200 blok) |
| kuning | 2 | sekitar 200–320 blok |
| oranye | 4 | sekitar 320–480 blok (hutan jadi karpet daun, `CARPET_FROM`) |
| merah | 8 | lebih jauh dari itu |

Dipakai untuk melihat di mana step berganti dan seberapa kasar sel di tempat artefak muncul.

### Mode 2: per jenis face

| Warna | Jenis |
|---|---|
| putih | permukaan atas (top) |
| abu-abu | permukaan bawah (bottom) |
| biru | dinding samping (wall) |
| pink | **skirt** di tepi tile |
| cyan | lembar air |

Dipakai untuk menjawab apakah artefak itu skirt, dinding, atau top.

### Mode 3: per tile

Tile bergantian merah dan biru menurut paritas `(x + z)` dari kunci tile. Setiap tile 64×64 blok, jadi semua tepi tile terlihat sebagai pergantian warna. Skirt tetap pink dan air tetap cyan.

Dipakai untuk melihat apakah garis, celah, atau lompatan tinggi berada tepat di batas tile.

### Hal yang sama di semua mode

- Face digambar dengan **texel putih polos** (texel datar blok salju) yang dikalikan warna debug. Jadi tidak ada tekstur, dan pola tekstur tidak mengganggu pembacaan.
- Shading per sisi tetap berlaku: shader menurunkannya dari arah face. Atas lebih terang daripada samping, jadi bentuk tetap terbaca.
- Kabut tetap berlaku, sehingga warna jauh memudar menuju warna kabut.
- Tekstur per blok (tiling) tidak aktif, karena `light` diset 1,0, bukan 1000 ditambah nomor tile.
- Chunk asli di sekitar pemain tidak diwarnai. Hanya mesh LoD yang berubah.

---

## 3. Cara membaca hasilnya

| Yang terlihat | Artinya | Tindakan |
|---|---|---|
| Garis atau strip **pink** yang menonjol di atas permukaan | Skirt yang terlihat. Seharusnya tersembunyi di dalam tanah. | Skirt salah ketinggian atau salah posisi. |
| Garis pucat atau celah menampakkan langit, dan **tidak ada pink** di situ | Retakan yang tidak tertutup skirt. | Panjang skirt kurang, atau skirt tidak terbentuk di kolom itu. |
| Celah tepat di pergantian warna **mode 3** | Masalah di batas tile. | Cek logika skirt dan margin tile. |
| Lompatan tinggi tepat di pergantian warna **mode 1** | Beda step antar tile. | Cek `reduce` dan pembulatan di batas step. |
| Dinding **biru** yang sangat tinggi atau sangat banyak | Dinding tak terlihat normal. | Cek `reduce` dan tinggi sel. |
| Bentuk "melayang" tanpa pink dan tanpa biru di bawahnya | Mahkota pohon tanpa batang. | Itu perilaku `reduce` dari step 4 ke atas. |
| Kotak besar berwarna sama di mode 1 | Sel kasar (step 4–8). | Wajar untuk jarak jauh. |
| Lembar **cyan** melayang di atas daratan | Tinggi air salah. | Cek tinggi lembar air. Sejak 2026-10-18 lembar air normal transparan (tile air asli) dan hanya cyan buram di mode debug. |

Screenshot yang paling berguna adalah satu titik dan satu arah pandang di tiga mode (1, 2, 3) ditambah satu tanpa debug.

---

## 4. Cara kerja

### Data

| Elemen | Tempat | Fungsi |
|---|---|---|
| `Shared.dbg: AtomicU8` | `world/lod.rs` | Mode aktif (0–3), dibaca oleh thread pembangun mesh. |
| `Built.dbg: u8` | `world/lod.rs` | Mode saat tile itu dibangun. |
| `Quads.dbg`, `step`, `par`, `kind` | `world/lod.rs` | Mode, step tile, paritas tile, dan jenis quad yang sedang dibuat. |
| Konstanta `K_TOP`, `K_BOTTOM`, `K_WALL`, `K_SKIRT`, `K_WATER` | `world/lod.rs` | Jenis quad. |
| `debug_rgb(dbg, step, par, kind)` | `world/lod.rs` | Menentukan warna RGB. |

### Alur

1. `Lod::cycle_debug()` menaikkan mode (mod 4), membuang semua tile, daftar `dirty`, dan daftar `visible`.
2. `Lod::update` meminta tile lagi seperti biasa karena `tiles` kosong.
3. `build()` membaca `store.sh.dbg` dan membuat `Quads` dengan mode itu.
4. Saat membuat face, kode menetapkan jenisnya:
   - `flat` menetapkan atas atau bawah,
   - `wall` menetapkan dinding,
   - kode skirt menetapkan `K_SKIRT` sebelum memanggil `wall`,
   - lembar air menetapkan `K_WATER`.
5. `Quads::quad` (jalur akhir semua face) memeriksa mode. Kalau tidak nol, ia mengganti UV menjadi texel putih datar, `light` menjadi 1,0, dan `tint` menjadi warna dari `debug_rgb`.
6. Tile yang sedang dibangun saat mode berubah selesai dengan `Built.dbg` lama. `Lod::update` membuangnya (`b.dbg != mode saat ini`) dan tile itu diminta lagi.

### UI

| Berkas | Perubahan |
|---|---|
| `input/touch_ui.rs` | `PointerRole::LodDebug`, rect `Layout.dbg` (di bawah `pregen`), bendera `lod_debug`, `take_lod_debug()`. |
| `lib.rs` | Memanggil `lod.cycle_debug()` saat tombol diketuk, dan menggambar label tombol sesuai mode. |

Tombol hanya aktif saat menu pause terbuka, seperti Pregen dan Fly.

---

## 5. Tes

`debug_mode_colours_skirts_and_walls` (di `world/lod.rs`) membangun satu tile tanah datar dalam mode 2 dan memeriksa:

- `Built.dbg` bernilai 2,
- tidak ada vertex yang masih memakai `light` tiling (`light < 1000`),
- jumlah vertex skirt tepat 4 × 64 × 4 (4 sisi × 64 kolom × 4 vertex),
- jumlah vertex permukaan atas tepat 64 × 64 × 4.

Tes ini hanya memeriksa pewarnaan. Ia tidak memeriksa tampilan di layar.

---

## 6. Batasan

- **Warna belum terbukti benar di layar.** Metode ini meniru lembar air, yang memakai texel putih dan tint sebagai warnanya. Kalau shader tidak mengalikan tint untuk texel itu pada face tertentu, warna bisa meleset. Kalau warna terlihat salah, beri tahu saya.
- **Pembangunan ulang memakan waktu.** Setiap pergantian mode mengosongkan LoD dan membangunnya lagi dari nol, jadi tunggu beberapa saat sebelum mengambil screenshot.
- **Tile yang belum selesai** tampil sebagai lubang. Itu bukan artefak mode debug, melainkan antrean pembangunan tile.
- **Hanya LoD.** Mode ini tidak mewarnai chunk asli, entitas, atau langit.
- **Tidak ada penanda untuk batas "dalam jangkauan chunk asli"**, yaitu tempat LoD dipotong agar chunk asli menggantikannya. Kalau artefak ada di zona itu, sebutkan jaraknya.

---

## 7. Menghapus atau mengubah

- **Menghapus:** buang `Shared.dbg`, `Built.dbg`, field debug di `Quads`, `debug_rgb`, konstanta `K_*`, `cycle_debug`, `debug`, pemeriksaan di `Lod::update`, tes `debug_mode_colours_skirts_and_walls`, serta tombolnya di `touch_ui.rs` dan `lib.rs`.
- **Menambah mode:** tambahkan cabang di `debug_rgb`, naikkan `% 4` di `cycle_debug`, dan tambahkan teks tombol di `lib.rs`.
- **Mengubah warna:** hanya `debug_rgb`.
