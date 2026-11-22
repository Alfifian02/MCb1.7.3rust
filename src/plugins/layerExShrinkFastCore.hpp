// layerExShrinkFastCore.hpp - inti piksel Layer.shrinkCopyFast (tanpa dependensi TJS/Layer; bisa diuji di host).
// Sumber: rekonstruksi dari Kirikiroid2 1.3.9 arm64: implementasi 0x60185c (1.332 B), worker CPU 0x601d90 (1.628 B).
// Spesifikasi: docs/spec/layerExShrinkFast_shrinkCopyFast.md
//
// STATUS KEYAKINAN
//   TERBACA PENUH : validasi, ukuran tujuan, alur baris (vs<=1 / grup penuh / sisa grup), pembagian vertikal
//                   (udiv 32-bit, memotong), alfa tujuan = 0xFF -- dan, sejak langkah 4.6b-2, pengecil baris
//                   horizontal hrow() = fungsi 0x6023ec (792 B).
//   TIDAK ADA LAGI ASUMSI pada jalur CPU. Hasil baca 0x6023ec: skalar murni (tanpa NEON), membaca hanya byte
//                   B,G,R tiap piksel (alfa sumber tidak pernah dibaca), alfa tujuan = 0xFF. Diverifikasi dengan
//                   mengeksekusi instruksi aslinya di emulator (tests/plugins/layerExShrinkFast/emu_hrow_6023ec.py).
//   YANG MASIH DIWARISI (bukan dari sesi ini): pembagian sw/hs & sw%hs oleh pemanggil 0x60185c (blok konteks +0x18/+0x1c).
#pragma once
#include <stdint.h>
#include <stddef.h>
#include <limits.h>
#include <vector>

namespace layerex_shrink {

// sdiv 32-bit ala AArch64: bagi nol -> 0 (tidak trap); INT_MIN / -1 -> INT_MIN (tanpa UB C++).
static inline int32_t sdiv32(int32_t a, int32_t b)
{
	if (b == 0) return 0;
	if (a == INT32_MIN && b == -1) return INT32_MIN;
	return a / b;
}

// ceil(n / d) persis seperti binary: sdiv(n + d - 1, d) dengan penjumlahan 32-bit yang melingkar (wrap).
static inline int32_t ceil_div_wrap(int32_t n, int32_t d)
{
	return sdiv32((int32_t)((uint32_t)n + (uint32_t)d - 1u), d);
}

// Faktor vs bawaan: bila parameter ke-3 tidak ada atau bernilai 0, vs = hs (binary: cbnz w19 -> mov w19,w23).
static inline int32_t default_vs(int32_t hs, int32_t vs) { return vs == 0 ? hs : vs; }

// ---- TERBACA PENUH (0x6023ec): satu baris sumber (sw piksel BGRA) -> ceil(sw/hs) piksel BGRA --------------------
// Argumen binary: x0 = blok konteks (hs @+0x10, quot = sw/hs @+0x18, rem = sw%hs @+0x1c), x1 = baris tujuan, x2 = baris sumber.
// Prasyarat (dijamin pemanggil CPU: hs >= 1): hs >= 1, quot >= 0, rem >= 0, quot*hs + rem = sw.
//   * `quot` blok penuh (0x6024a0..0x602590): jumlah hs piksel per kanal B,G,R dalam akumulator 32-bit, lalu `udiv` 32-bit
//     dengan hs (memotong, TANPA pembulatan). Alfa tujuan = 0xFF; alfa sumber (byte 3) tidak dibaca.
//   * hs == 1 (0x602410..0x60245c) adalah jalur khusus di binary: salin B,G,R, alfa 0xFF -- hasilnya sama dengan membagi 1.
//   * bila rem >= 1 (0x6025b8..0x6026e8): satu piksel tujuan lagi dari `rem` piksel terakhir, dibagi `rem` (BUKAN hs);
//     rem == 1 menyalin B,G,R apa adanya. Alfa 0xFF.
//   * Binary membuka loop 2x dengan akumulator genap/ganjil terpisah lalu menjumlahkannya; penjumlahan bilangan bulat
//     bersifat komutatif, jadi satu loop skalar di bawah memberi byte yang sama.
//   * Tidak ada NEON, tidak ada alokasi. Hanya sw*3 byte sumber dibaca dan ceil(sw/hs)*4 byte tujuan ditulis (tiap satu kali).
static inline void hrow(uint8_t *dst, const uint8_t *src, int hs, int quot, int rem)
{
	for (int i = 0; i < quot; ++i) {
		uint32_t b = 0, g = 0, r = 0;
		for (int k = 0; k < hs; ++k) { b += src[0]; g += src[1]; r += src[2]; src += 4; }
		dst[0] = (uint8_t)(b / (uint32_t)hs); dst[1] = (uint8_t)(g / (uint32_t)hs); dst[2] = (uint8_t)(r / (uint32_t)hs); dst[3] = 0xFF;
		dst += 4;
	}
	if (rem > 0) {
		uint32_t b = 0, g = 0, r = 0;
		for (int k = 0; k < rem; ++k) { b += src[0]; g += src[1]; r += src[2]; src += 4; }
		dst[0] = (uint8_t)(b / (uint32_t)rem); dst[1] = (uint8_t)(g / (uint32_t)rem); dst[2] = (uint8_t)(r / (uint32_t)rem); dst[3] = 0xFF;
	}
}

// ---- TERBACA PENUH: rata-rata `n` baris tmp (masing-masing dw piksel) -> satu baris tujuan --------------------------
// 0x602204-0x602280 / 0x6022d0-0x602348: jumlah per kanal B,G,R, udiv 32-bit tanpa pembulatan, alfa = 0xFF.
static inline void vavg(uint8_t *dst, const uint8_t *tmp, int dw, int n)
{
	const size_t rowbytes = (size_t)dw * 4;
	for (int x = 0; x < dw; ++x) {
		uint32_t b = 0, g = 0, r = 0;
		for (int k = 0; k < n; ++k) { const uint8_t *p = tmp + (size_t)k * rowbytes + (size_t)x * 4; b += p[0]; g += p[1]; r += p[2]; }
		uint8_t *o = dst + (size_t)x * 4;
		o[0] = (uint8_t)(b / (uint32_t)n); o[1] = (uint8_t)(g / (uint32_t)n); o[2] = (uint8_t)(r / (uint32_t)n); o[3] = 0xFF;
	}
}

// Worker CPU 0x601d90. Prasyarat (dijamin pemanggil): hs>=1, vs>=1, sw>=1, sh>=1; dst berukuran
// ceil(sw/hs) x ceil(sh/vs) piksel BGRA. dst dan src tidak boleh tumpang tindih.
static inline void shrink_fast(uint8_t *dst, ptrdiff_t dpitch, const uint8_t *src, ptrdiff_t spitch,
                               int sw, int sh, int hs, int vs)
{
	const int dw = ceil_div_wrap(sw, hs);
	const int hq = sw / hs, hr = sw % hs;                // blok konteks binary: +0x18 = sw/hs, +0x1c = sw%hs
	if (vs <= 1) {                                       // 0x601e18: tiap baris sumber -> satu baris tujuan
		for (int y = 0; y < sh; ++y) hrow(dst + (ptrdiff_t)y * dpitch, src + (ptrdiff_t)y * spitch, hs, hq, hr);
		return;
	}
	// Binary memakai malloc(dw*4*vs); di sini cukup min(vs, sh) baris (hasil identik, tanpa alokasi raksasa saat vs >> sh).
	const int trows = vs < sh ? vs : sh;
	std::vector<uint8_t> tmp((size_t)dw * 4 * (size_t)trows);
	const size_t rowbytes = (size_t)dw * 4;
	const int nfull = sh / vs;
	int y = 0, oy = 0;
	for (int g = 0; g < nfull; ++g, ++oy) {              // 0x602084: grup penuh vs baris
		for (int k = 0; k < vs; ++k, ++y) hrow(tmp.data() + (size_t)k * rowbytes, src + (ptrdiff_t)y * spitch, hs, hq, hr);
		vavg(dst + (ptrdiff_t)oy * dpitch, tmp.data(), dw, vs);
	}
	const int rem = sh - nfull * vs;                     // 0x601eec: sisa baris (dibagi `rem`, BUKAN vs)
	if (rem > 0) {
		for (int k = 0; k < rem; ++k, ++y) hrow(tmp.data() + (size_t)k * rowbytes, src + (ptrdiff_t)y * spitch, hs, hq, hr);
		vavg(dst + (ptrdiff_t)oy * dpitch, tmp.data(), dw, rem);
	}
}

} // namespace layerex_shrink
