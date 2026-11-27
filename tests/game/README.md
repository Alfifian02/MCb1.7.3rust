# tests/game - patch uji untuk game nyata (S17)

**BELUM diuji di perangkat.** Dipakai bersama `docs/spec/motionplayer_diagnosis_S17.md` bagian 5.

- `zz_k2patch/yuzu_default.tjs` - sumber: `yuzu_default.tjs` asli (game Limelight Lemonade Jam, `main/yuzu_default.tjs`) + patch lama `.ShortCutInitialPadKeyMap = %[]` + blok diagnostik `[k2probe]` di AWAL berkas (berkas asli diakhiri `return` di dalam `with`, kode setelahnya tidak akan jalan). Disimpan UTF-16LE + BOM, akhir baris CRLF, seperti aslinya.
- `zz_k2patch.xp3` - hasil `python3 xp3tool.py pack tests/game/zz_k2patch tests/game/zz_k2patch.xp3` (XP3 klasik, terkompresi; `xp3tool verify` = 0 rusak; isi diekstrak identik dengan sumber). Menggantikan `zz_k2patch.xp3` lama.

Pasang: salin `zz_k2patch.xp3` ke folder game (sejajar `data.xp3`), jalankan game sampai berhenti, lalu `grep "k2probe" krkr.console.log`. Tabel pembacaan hasil ada di diagnosis bagian 5.
