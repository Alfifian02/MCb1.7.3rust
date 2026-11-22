# Spesifikasi: `Layer.ExtraColorVectorEffectAdd` (layerExAlpha.dll, Alpha-1)

Plugin `layerExAlpha.dll` · method `ExtraColorVectorEffectAdd` · implementasi `0x5d5cdc` (700 B) · registrasi `0x5d43b0`. Hal umum: [layerExAlpha_umum.md](layerExAlpha_umum.md).
Nama parameter **diperkirakan** (binary tidak menyimpan nama).

## Tanda tangan
`this.ExtraColorVectorEffectAdd(r0, r1, g0, g1, b0, b1, add)` — tanda tangan & pengelompokan sama dengan `ColorVectorEffectAdd`.

## Efek
Prakomputasi: sama seperti `ColorVectorEffectAdd` (`step`, `dR/dG/dB`), ditambah `thr = (r1 + g1 + b1) / 3` (dibulatkan ke nol).
Per piksel dengan `A ≠ 0`: `A' = clamp(A − add)` (disimpan), `t = (255 − A')/add`, `avg = (R+G+B)/3`.
- **Cabang gradien** (`t ≠ 0` dan `avg > thr`): warna = gradien seperti `ColorVectorEffectAdd`.
- **Cabang lain:** (1) tiap kanal yang nilainya ≤ `avg` diganti konstanta: byte0 ← arg1, byte1 ← arg3, byte2 ← arg5 (8 bit rendah); (2) satu kanal dipilih lewat jaringan perbandingan `sel` atas nilai baru P0,P1,P2: `sel = (P0>P1) ? 2·(P0≤P2) : 1+(P1≥P2)`; (3) kanal itu diubah menjadi `max(nilai ± add, avg)` (`+` bila `avg ≥ nilai`, bila tidak `−`).

## Kasus tepi
- Pemetaan kanal ↔ argumen **berbeda antara kedua cabang** (gradien: byte2 ← arg1; cabang lain: byte0 ← arg1). Ini dibaca persis dari binary dan **dikonfirmasi emulasi**; bukan salah ketik.
- `add = 0`: `t = 0` → selalu cabang lain.
- Beberapa store dalam cabang gradien di binary berulang (artefak kompiler) dan tidak mengubah hasil.
- Tidak ada jalur GPU.

## ASUMSI
**Maksud** jaringan pemilihan `sel` (apa yang dimaksud "white to blue blur") tidak diketahui; **perilaku** ditiru persis (dibuktikan emulasi pada ribuan kasus acak).

## Uji
Nilai tangan + vektor emas di `tests/plugins/layerExAlpha/` (inti-host + emulasi instruksi). NDK/perangkat: belum.
