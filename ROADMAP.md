# Roadmap: Minecraft b1.7.3 -> Rust (Android low-to-mid end)

Sumber: decomp MCP b1.7.3 (678 file Java, +-65 rb baris). Strategi: port per lapisan, logika dunia dulu
(deterministik), tetapi **APK uji dikirim sedini mungkin** karena pengembang hanya punya Android.

## Prinsip
- `mc-core`: logika murni, tanpa GL/Android. Bit-exact dengan Java untuk hal yang memengaruhi seed.
- Hapus pooling Java (Vec3D/AxisAlignedBB): struct `Copy` di stack.
- Satu `Vec<u8>` per chunk (16x128x16), tanpa objek per blok. Mesh chunk di thread terpisah.
- Hemat: draw distance kecil default, face culling, tanpa alokasi di loop tick/render.
- Data statis (blok, material, tabrakan) dihasilkan dari dump Java asli (`tools/`), bukan disalin manual.

## Urutan baru (APK lebih awal)
| # | Fase | Isi | Status |
|---|------|-----|--------|
| 1 | Fondasi | JRandom, NibbleArray, Perlin/Octaves | ditulis, belum dikompilasi |
| 2 | Matematika | Vec3, Aabb, MathHelper | ditulis, belum dikompilasi |
| 3 | Blok & material | registri 96 blok, 28 material (data dari dump asli) | data ditulis, belum dikompilasi |
| 4a | Chunk & cahaya | Chunk, heightmap, skylight/blocklight, antrean cahaya | ditulis, belum dikompilasi |
| 4b | World 1 | set_block*WithNotify, notifikasi tetangga, tick terjadwal, kecerahan | ditulis, belum dikompilasi |
| 4c | World 2 | tabrakan blok, raycast, ChunkSource, dimensi | ditulis, belum dikompilasi |
| **A** | **Cangkang Android + CI (APK-1)** | layar uji mandiri, GitHub Actions (tes + APK + Release) | **ditulis, belum dikompilasi** |
| 5 | Generator dunia | ChunkProviderGenerate, WorldChunkManager, Biome, MapGenCaves, WorldGen*, populate | - |
| **B** | **Peta dari seed (APK-2)** | gambar peta atas terrain dari seed, ketuk untuk geser (renderer perangkat lunak) | - |
| 6 | NBT & save | NBT*, RegionFile, McRegion, WorldInfo | - |
| **C** | **Render voxel GLES + input sentuh (APK-3)** | meshing chunk, joystick virtual, jalan-jalan di dunia, taruh/hancurkan blok | - |
| 7 | Entity & AI | Entity*, fisika pemain, mob, SpawnerAnimals, tabrakan entity (4d) | - |
| 8 | Item, inventori, crafting | Item*, Container*, resep, furnace | - |
| 9 | Redstone, rel, piston, cairan | BlockRedstoneWire, RailLogic, BlockPistonBase, BlockFlowing + perilaku blok 4b/4c | - |
| 10 | Nether & portal | WorldProviderHell, ChunkProviderHell | - |
| 11 | GUI & statistik | Gui*, achievement, stats | - |
| 12 | Penyelesaian Android | audio, siklus hidup, setelan performa, rilis (panic=abort, penandatanganan) | - |
| 13 | Multiplayer | Packet* (protokol 14), NetClientHandler | - |

## Verifikasi
- Uji unit di CI + `selftest` di APK (hasil hijau/merah di layar).
- Uji golden terhadap Java untuk seed tetap; Fase 5 selesai jika chunk (0,0) seed tertentu identik byte-per-byte.
