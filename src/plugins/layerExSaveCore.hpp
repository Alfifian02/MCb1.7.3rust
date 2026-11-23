// layerExSaveCore.hpp - inti piksel layerExSave bagian A / utilitas (tanpa dependensi TJS/Layer; bisa diuji di host).
// Sumber: rekonstruksi dari Kirikiroid2 1.3.9 arm64, sesi S11 (recon_saveA.txt, depth 0):
//   getCropRect 0x5eab30 · getCropRectZero 0x5eb184 · getDiffRect 0x5eb454 · getDiffPixel 0x5eb86c
//   oozeColor 0x5ebbf0 · copyBlueToAlpha 0x5ebfec · isBlank 0x5ec190 · clearAlpha 0x5ec50c
// Spesifikasi: docs/spec/layerExSave_*.md (konvensi bersama: layerExSave_common.md)
//
// MODEL DATA (sama dengan pembantu biner 0x5ead0c): sebuah gambar = larik penunjuk baris `rows[y]` (y = 0..H-1),
// tiap baris W piksel 32-bit, urutan byte B,G,R,A (alfa = byte ke-3). Lebar/tinggi = ukuran GAMBAR UTAMA layer.
//
// STATUS KEYAKINAN
//   TERBACA PENUH dari disassembly: semua algoritma piksel di bawah (urutan pindai, predikat, nilai tulis).
//   BUKAN dari recon (dibaca dari byte biner dengan dekoder mini; lihat spesifikasi): pembantu 0x5ead0c dan 0x5eaea4.
//   ASUMSI: nama parameter TJS, peran semantik `getDiffPixel` (nama "same"/"diff" perkiraan).
#pragma once
#include <stdint.h>
#include <stddef.h>

namespace layerex_save {

struct Rect { int32_t x, y, w, h; };

typedef const uint8_t *const *CRows;   // baris baca
typedef uint8_t *const *WRows;         // baris tulis

// ---- pemindai rect pembatas -----------------------------------------------------------------------------
// Pola yang sama dipakai getCropRect / getCropRectZero / getDiffRect (biner: tiga salinan kode):
//   1. kiri  = kolom pertama (0..W-1) yang memuat piksel "aktif" (kolom di luar, baris dalam);
//   2. kanan = kolom terakhir yang memuat piksel aktif;
//   3. atas  = baris pertama yang memuat piksel aktif di kolom [kiri, kanan];
//   4. bawah = baris terakhir yang memuat piksel aktif di kolom yang dipindai (lihat bottom_x0).
//   hasil = {kiri, atas, kanan-kiri+1, bawah-atas+1}. Tidak ada piksel aktif -> false (hasil TJS: void).
// `bottom_x0` = kolom awal pindai baris-bawah, sebanyak (kanan-kiri+1) kolom; normalnya == kiri.
// Biner getDiffRect memakai KANAN (bug, 0x5eb708); kolom >= W dianggap tidak aktif (biner membaca di luar batas).
template <class Active>
static inline bool scan_rect(int W, int H, Active active, bool bottom_from_right, Rect *out)
{
	if (W < 1 || H < 1) return false;
	int left = -1;
	for (int x = 0; x < W && left < 0; ++x)
		for (int y = 0; y < H; ++y) if (active(x, y)) { left = x; break; }
	if (left < 0) return false;
	int right = left;                                   // biner: kolom `left` pasti aktif, jadi pindai mundur selalu berhenti >= left
	for (int x = W - 1; x >= 0; --x) {
		bool hit = false;
		for (int y = 0; y < H; ++y) if (active(x, y)) { hit = true; break; }
		if (hit) { right = x; break; }
	}
	const int cnt = right - left + 1;
	int top = H;                                        // pasti ketemu (kolom `left` punya piksel aktif)
	for (int y = 0; y < H && top == H; ++y)
		for (int k = 0; k < cnt; ++k) if (active(left + k, y)) { top = y; break; }
	const int x0 = bottom_from_right ? right : left;
	int bottom = top;
	for (int y = H - 1; y >= 0; --y) {
		bool hit = false;
		for (int k = 0; k < cnt; ++k) {
			const int x = x0 + k;
			if (x >= W) break;                          // hanya terjadi bila bottom_from_right (biner: baca di luar batas)
			if (active(x, y)) { hit = true; break; }
		}
		if (hit) { bottom = y; break; }
	}
	out->x = left; out->y = top; out->w = cnt; out->h = bottom - top + 1;
	return true;
}

// getCropRect (0x5eab30): piksel aktif = alfa != 0.
static inline bool crop_rect_alpha(CRows rows, int W, int H, Rect *out)
{
	struct P { CRows r; bool operator()(int x, int y) const { return r[y][(size_t)x * 4 + 3] != 0; } } p = { rows };
	return scan_rect(W, H, p, false, out);
}

// getCropRectZero (0x5eb184): piksel aktif = salah satu byte B,G,R,A != 0 (piksel 32-bit != 0).
static inline bool crop_rect_zero(CRows rows, int W, int H, Rect *out)
{
	struct P { CRows r; bool operator()(int x, int y) const {
		const uint8_t *q = r[y] + (size_t)x * 4; return q[0] | q[1] | q[2] | q[3]; } } p = { rows };
	return scan_rect(W, H, p, false, out);
}

// Kesamaan piksel ala getDiffRect/getDiffPixel: alfa harus sama; bila alfa == 0, warna diabaikan (dianggap sama);
// bila alfa != 0, byte B,G,R juga harus sama.
static inline bool pixel_same(const uint8_t *a, const uint8_t *b)
{
	if (a[3] != b[3]) return false;
	if (a[3] == 0) return true;
	return a[2] == b[2] && a[1] == b[1] && a[0] == b[0];
}

// getDiffRect (0x5eb454). `bottom_from_right` = meniru bug biner (default fork: lihat layerExSave.cpp).
static inline bool diff_rect(CRows a, CRows b, int W, int H, bool bottom_from_right, Rect *out)
{
	struct P { CRows a, b; bool operator()(int x, int y) const {
		return !pixel_same(a[y] + (size_t)x * 4, b[y] + (size_t)x * 4); } } p = { a, b };
	return scan_rect(W, H, p, bottom_from_right, out);
}

// getDiffPixel (0x5eb86c): bandingkan `a` (param) dengan `b` (this). Piksel sama -> b := same (bila has_same);
// piksel beda -> b := diff (bila has_diff) dan dihitung. Mengembalikan jumlah piksel beda.
// Nilai ditulis penuh 32-bit (termasuk alfa). Urutan pindai: baris luar, kolom dalam. `a` dan `b` boleh alias.
static inline int64_t diff_pixel(CRows a, WRows b, int W, int H,
                                 bool has_same, uint32_t same, bool has_diff, uint32_t diff)
{
	int64_t count = 0;
	for (int y = 0; y < H; ++y) {
		const uint8_t *pa = a[y];
		uint8_t *pb = b[y];
		for (int x = 0; x < W; ++x, pa += 4, pb += 4) {
			const bool eq = pixel_same(pa, pb);
			if (eq) {
				if (has_same) { pb[0] = (uint8_t)same; pb[1] = (uint8_t)(same >> 8); pb[2] = (uint8_t)(same >> 16); pb[3] = (uint8_t)(same >> 24); }
			} else {
				if (has_diff) { pb[0] = (uint8_t)diff; pb[1] = (uint8_t)(diff >> 8); pb[2] = (uint8_t)(diff >> 16); pb[3] = (uint8_t)(diff >> 24); }
				++count;
			}
		}
	}
	return count;
}

// oozeColor (0x5ebbf0): ambang efektif t = (threshold & 0xFF) ? (threshold & 0xFF) : 1.
// Piksel dengan alfa < t: B,G,R := warna (color = 0x00RRGGBB), alfa TIDAK diubah. Piksel lain: tetap.
// Catatan: biner membangun peta bantu (W+2)x(H+2) dan mengulang `level` kali "bila peta != 0 maka peta = 0xFF"
// (idempoten, tanpa efek pada gambar). Fork tidak mengulangnya; `level` hanya divalidasi (>= 1) oleh pemanggil.
static inline void ooze_color(WRows rows, int W, int H, uint32_t threshold, uint32_t color)
{
	const uint32_t t = (threshold & 0xFF) ? (threshold & 0xFF) : 1u;
	const uint8_t cb = (uint8_t)color, cg = (uint8_t)(color >> 8), cr = (uint8_t)(color >> 16);
	for (int y = 0; y < H; ++y) {
		uint8_t *p = rows[y];
		for (int x = 0; x < W; ++x, p += 4)
			if (p[3] < t) { p[0] = cb; p[1] = cg; p[2] = cr; }
	}
}

// copyBlueToAlpha (0x5ebfec): dst.alfa := src.B untuk wilayah tumpang tindih min(lebar) x min(tinggi) dari (0,0).
static inline void blue_to_alpha(CRows src, int sw, int sh, WRows dst, int dw, int dh)
{
	const int w = sw < dw ? sw : dw, h = sh < dh ? sh : dh;
	for (int y = 0; y < h; ++y) {
		const uint8_t *s = src[y];
		uint8_t *d = dst[y];
		for (int x = 0; x < w; ++x, s += 4, d += 4) d[3] = s[0];
	}
}

// Validasi rect isBlank: (x|y|w) tidak boleh negatif (h TIDAK diperiksa tandanya), x+w <= W dan y+h <= H.
// DEVIASI: biner menjumlahkan dalam 32-bit melingkar sehingga x+w yang meluap lolos lalu membaca di luar batas (UB,
// terbukti di emulator). Fork menjumlahkan dalam 64-bit dan menolak kasus itu.
static inline bool blank_range_ok(int32_t x, int32_t y, int32_t w, int32_t h, int32_t W, int32_t H)
{
	if (x < 0 || y < 0 || w < 0) return false;
	if ((int64_t)x + w > W) return false;
	if ((int64_t)y + h > H) return false;
	return true;
}

// isBlank (0x5ec190): true bila byte BIRU (byte 0, bukan alfa!) semua nol di rect. w<1 atau h<1 -> true.
// Pemanggil wajib memanggil blank_range_ok lebih dulu (rect harus di dalam gambar).
static inline bool is_blank(CRows rows, int32_t x, int32_t y, int32_t w, int32_t h)
{
	if (h < 1 || w < 1) return true;
	for (int32_t j = y; j < y + h; ++j) {
		const uint8_t *p = rows[j] + (size_t)x * 4;
		for (int32_t i = 0; i < w; ++i, p += 4) if (p[0]) return false;
	}
	return true;
}

// clearAlpha (0x5ec50c): piksel dengan alfa <= threshold (perbandingan BERTANDA 32-bit; threshold < alfa -> dilewati)
// ditimpa seluruhnya dengan (color & 0xFFFFFF), yaitu B,G,R dari warna dan alfa = 0.
static inline void clear_alpha(WRows rows, int W, int H, int32_t threshold, uint32_t color)
{
	const uint32_t v = color & 0xFFFFFFu;
	const uint8_t b0 = (uint8_t)v, b1 = (uint8_t)(v >> 8), b2 = (uint8_t)(v >> 16);
	for (int y = 0; y < H; ++y) {
		uint8_t *p = rows[y];
		for (int x = 0; x < W; ++x, p += 4)
			if (!(threshold < (int32_t)p[3])) { p[0] = b0; p[1] = b1; p[2] = b2; p[3] = 0; }
	}
}

} // namespace layerex_save
