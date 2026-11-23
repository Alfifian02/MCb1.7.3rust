// g++ -std=c++14 -fsanitize=address,undefined -I src/plugins tests/plugins/layerExSave/test_core.cpp && ./a.out
// Uji inti layerExSave bagian A (layerExSaveCore.hpp). Bagian [BINARY] = ekspektasi yang dihasilkan dengan MENGEKSEKUSI
// instruksi asli 8 fungsi libgame.so 1.3.9 di emulator (tests/plugins/layerExSave/emu_saveA.py <recon_saveA.txt> --golden),
// memakai generator LCG yang sama persis dengan emu_saveA.py. Tingkat bukti: inti-host (ASan/UBSan) + emulasi instruksi
// asli; BUKAN build NDK dan BUKAN uji perangkat. Peran pembantu (0x5ead0c dst.) di emulator = stub (lihat spesifikasi).
#include <stdio.h>
#include <string.h>
#include <vector>
#include "layerExSaveCore.hpp"
using namespace layerex_save;
static int fails = 0;
#define CHECK(c) do { if (!(c)) { printf("FAIL %s:%d %s\n", __FILE__, __LINE__, #c); ++fails; } } while (0)
typedef std::vector<uint8_t> Buf;

// ---- generator: identik dengan emu_saveA.py ----
static uint32_t lcg(uint32_t s) { return s * 1664525u + 1013904223u; }
static Buf gen_sparse(uint32_t seed, int W, int H, int k)
{
	Buf out; uint32_t s = seed;
	for (int i = 0; i < W * H; ++i) {
		s = lcg(s); uint32_t d = s >> 24;
		if (k && d % (uint32_t)k == 0) { for (int c = 0; c < 4; ++c) { s = lcg(s); out.push_back((uint8_t)(s >> 24)); } }
		else {
			s = lcg(s); uint32_t e = s >> 24;
			if (e & 1) { for (int c = 0; c < 3; ++c) { s = lcg(s); out.push_back((uint8_t)(s >> 24)); } }
			else { out.push_back(0); out.push_back(0); out.push_back(0); }
			out.push_back(0);
		}
	}
	return out;
}
static Buf gen_mutate(uint32_t seed, const Buf &base, int m)
{
	Buf out(base); uint32_t s = seed;
	for (size_t i = 0; i < base.size() / 4; ++i) {
		s = lcg(s); uint32_t d = s >> 24;
		if (m && d % (uint32_t)m == 0) {
			s = lcg(s); uint32_t mode = (s >> 24) % 3;
			if (mode == 0 || mode == 2) for (int c = 0; c < 3; ++c) { s = lcg(s); out[i * 4 + c] = (uint8_t)(s >> 24); }
			if (mode == 1 || mode == 2) { s = lcg(s); out[i * 4 + 3] = (uint8_t)(s >> 24); }
		}
	}
	return out;
}
static uint64_t fnv1a(const Buf &b) { uint64_t h = 0xCBF29CE484222325ULL; for (size_t i = 0; i < b.size(); ++i) { h ^= b[i]; h *= 0x100000001B3ULL; } return h; }

// larik penunjuk baris dengan pitch berpadding (padding 0xEE harus tetap utuh -> ASan + pemeriksaan)
struct Img {
	int w, h; size_t pitch; Buf mem; std::vector<uint8_t*> rows;
	Img(int w_, int h_, const Buf &pix, size_t pad = 8) : w(w_), h(h_), pitch((size_t)w_ * 4 + pad), mem(pitch * (h_ > 0 ? h_ : 1), 0xEE) {
		for (int y = 0; y < h; ++y) { memcpy(&mem[y * pitch], &pix[(size_t)y * w * 4], (size_t)w * 4); rows.push_back(&mem[y * pitch]); }
	}
	Buf pix() const { Buf o; for (int y = 0; y < h; ++y) o.insert(o.end(), rows[y], rows[y] + w * 4); return o; }
	bool pad_ok() const { for (int y = 0; y < h; ++y) for (size_t i = (size_t)w * 4; i < pitch; ++i) if (mem[y * pitch + i] != 0xEE) return false; return true; }
	CRows c() const { return (CRows)rows.data(); }
	WRows wr() { return rows.data(); }
};
static bool eq(const Rect &r, int x, int y, int w, int h) { return r.x == x && r.y == y && r.w == w && r.h == h; }

int main()
{
	// ---- vektor tangan ----
	{	// getCropRect: alfa hanya di (1,1) dan (2,2) pada 4x3; warna tanpa alfa tidak dihitung
		Buf b(4 * 3 * 4, 0); b[(1 * 4 + 1) * 4 + 3] = 9; b[(2 * 4 + 2) * 4 + 3] = 1; b[(0 * 4 + 0) * 4 + 0] = 77;
		Img im(4, 3, b); Rect r = {};
		CHECK(crop_rect_alpha(im.c(), 4, 3, &r) && eq(r, 1, 1, 2, 2));
		CHECK(crop_rect_zero(im.c(), 4, 3, &r) && eq(r, 0, 0, 3, 3));            // piksel (0,0) punya biru != 0
		Img z(4, 3, Buf(48, 0));
		CHECK(!crop_rect_alpha(z.c(), 4, 3, &r) && !crop_rect_zero(z.c(), 4, 3, &r));
		CHECK(!crop_rect_alpha(im.c(), 0, 3, &r) && !crop_rect_alpha(im.c(), 4, 0, &r));
		Img one(1, 1, Buf{0, 0, 0, 1}); CHECK(crop_rect_alpha(one.c(), 1, 1, &r) && eq(r, 0, 0, 1, 1));
	}
	{	// kesamaan piksel: alfa 0 -> warna diabaikan; alfa != 0 -> warna dibandingkan
		const uint8_t a0[4] = {1, 2, 3, 0}, a1[4] = {9, 8, 7, 0}, b0[4] = {1, 2, 3, 5}, b1[4] = {1, 2, 4, 5};
		CHECK(pixel_same(a0, a1)); CHECK(!pixel_same(a0, b0)); CHECK(pixel_same(b0, b0)); CHECK(!pixel_same(b0, b1));
	}
	{	// getDiffRect: beda di (2,0) dan (0,5) pada 6x6. Rect benar = (0,0,3,6); biner (bug, bawah dari kolom kanan) = (0,0,3,1)
		Buf A(6 * 6 * 4, 0), B(A); B[(0 * 6 + 2) * 4 + 3] = 0xFF; B[(5 * 6 + 0) * 4 + 3] = 0xFF;
		Img ia(6, 6, A), ib(6, 6, B); Rect r = {};
		CHECK(diff_rect(ia.c(), ib.c(), 6, 6, true, &r) && eq(r, 0, 0, 3, 1));
		CHECK(diff_rect(ia.c(), ib.c(), 6, 6, false, &r) && eq(r, 0, 0, 3, 6));
		CHECK(!diff_rect(ia.c(), ia.c(), 6, 6, true, &r));
	}
	{	// getDiffPixel: nilai ditulis penuh 32-bit; alias a==b; hitungan hanya piksel beda
		Buf A = {10, 20, 30, 40, 1, 2, 3, 0}, B = {10, 20, 30, 40, 9, 9, 9, 0};   // piksel 0 sama; piksel 1: alfa 0 di kedua -> sama
		Img ia(2, 1, A), ib(2, 1, B);
		CHECK(diff_pixel(ia.c(), ib.wr(), 2, 1, true, 0xAABBCCDDu, true, 0x11223344u) == 0);
		Buf e = {0xDD, 0xCC, 0xBB, 0xAA, 0xDD, 0xCC, 0xBB, 0xAA}; CHECK(ib.pix() == e);
		Buf B2 = {10, 20, 31, 40, 1, 2, 3, 7}; Img ib2(2, 1, B2);
		CHECK(diff_pixel(ia.c(), ib2.wr(), 2, 1, false, 0, true, 0x01020304u) == 2);
		Buf e2 = {4, 3, 2, 1, 4, 3, 2, 1}; CHECK(ib2.pix() == e2);
		Img ib3(2, 1, B2); CHECK(diff_pixel(ia.c(), ib3.wr(), 2, 1, false, 0, false, 0) == 2 && ib3.pix() == B2);
	}
	{	// oozeColor: ambang efektif
		Buf b = {1, 2, 3, 0,  1, 2, 3, 1,  1, 2, 3, 254,  1, 2, 3, 255};
		Img i1(4, 1, b); ooze_color(i1.wr(), 4, 1, 1, 0x00112233u);   // t=1: hanya alfa 0 diganti
		Buf e1 = {0x33, 0x22, 0x11, 0,  1, 2, 3, 1,  1, 2, 3, 254,  1, 2, 3, 255}; CHECK(i1.pix() == e1);
		Img i2(4, 1, b); ooze_color(i2.wr(), 4, 1, 0x100, 0x00112233u);   // 0x100 & 0xFF = 0 -> t=1
		CHECK(i2.pix() == e1);
		Img i3(4, 1, b); ooze_color(i3.wr(), 4, 1, 0x1FF, 0x00112233u);   // t=255: alfa<255 diganti, alfa 255 tetap
		Buf e3 = {0x33, 0x22, 0x11, 0,  0x33, 0x22, 0x11, 1,  0x33, 0x22, 0x11, 254,  1, 2, 3, 255}; CHECK(i3.pix() == e3);
		Img i4(4, 1, b); ooze_color(i4.wr(), 4, 1, 2, 0xFFAABBCCu);       // bit atas warna diabaikan; alfa tidak diubah
		Buf e4 = {0xCC, 0xBB, 0xAA, 0,  0xCC, 0xBB, 0xAA, 1,  1, 2, 3, 254,  1, 2, 3, 255}; CHECK(i4.pix() == e4 && i4.pad_ok());
	}
	{	// copyBlueToAlpha: min ukuran, dari (0,0)
		Buf S = {5, 0, 0, 0,  6, 0, 0, 0,  7, 0, 0, 0,  8, 0, 0, 0}, D(3 * 3 * 4, 0xAA);
		Img s(2, 2, S), d(3, 3, D);
		blue_to_alpha(s.c(), 2, 2, d.wr(), 3, 3);
		Buf e = d.pix(); CHECK(e[3] == 5 && e[7] == 6 && e[11] == 0xAA && e[(1 * 3 + 0) * 4 + 3] == 7 && e[(1 * 3 + 1) * 4 + 3] == 8
		                       && e[(2 * 3 + 0) * 4 + 3] == 0xAA && e[0] == 0xAA && d.pad_ok());
		blue_to_alpha(s.c(), 2, 2, d.wr(), 0, 0);
	}
	{	// isBlank: BIRU (byte 0), bukan alfa; rentang
		Buf b(4 * 4 * 4, 0); for (int i = 0; i < 16; ++i) b[i * 4 + 3] = 255;       // alfa penuh, biru nol -> tetap "blank"
		Img im(4, 4, b); CHECK(is_blank(im.c(), 0, 0, 4, 4));
		im.rows[2][2 * 4 + 0] = 1; CHECK(!is_blank(im.c(), 0, 0, 4, 4) && !is_blank(im.c(), 2, 2, 1, 1) && is_blank(im.c(), 0, 0, 2, 4));
		CHECK(is_blank(im.c(), 0, 0, 0, 4) && is_blank(im.c(), 0, 0, 4, 0) && is_blank(im.c(), 2, 2, 1, -5));
		CHECK(blank_range_ok(0, 0, 4, 4, 4, 4) && blank_range_ok(4, 4, 0, 0, 4, 4) && blank_range_ok(0, 0, 0, -1, 4, 4));
		CHECK(!blank_range_ok(-1, 0, 1, 1, 4, 4) && !blank_range_ok(0, -1, 1, 1, 4, 4) && !blank_range_ok(0, 0, -1, 1, 4, 4));
		CHECK(!blank_range_ok(3, 0, 2, 1, 4, 4) && !blank_range_ok(0, 3, 1, 2, 4, 4));
		CHECK(!blank_range_ok(0x7FFFFFFF, 0, 2, 1, 4, 4));                              // biner: lolos (32-bit melingkar) lalu baca luar batas
	}
	{	// clearAlpha: perbandingan bertanda; menimpa seluruh piksel
		Buf b = {1, 2, 3, 0,  1, 2, 3, 5,  1, 2, 3, 255};
		Img a(3, 1, b); clear_alpha(a.wr(), 3, 1, 0, 0x00FFFFFFu);
		Buf e = {0xFF, 0xFF, 0xFF, 0,  1, 2, 3, 5,  1, 2, 3, 255}; CHECK(a.pix() == e);
		Img n(3, 1, b); clear_alpha(n.wr(), 3, 1, -1, 0); CHECK(n.pix() == b);          // threshold < 0 -> tak ada yang cocok
		Img f(3, 1, b); clear_alpha(f.wr(), 3, 1, 255, 0xFF123456u);
		Buf ef = {0x56, 0x34, 0x12, 0,  0x56, 0x34, 0x12, 0,  0x56, 0x34, 0x12, 0}; CHECK(f.pix() == ef);   // bit atas warna dibuang
		Img big(3, 1, b); clear_alpha(big.wr(), 3, 1, 300, 0); Buf z(12, 0); CHECK(big.pix() == z && big.pad_ok());
	}

	// ---- [BINARY] ekspektasi dari emulasi instruksi asli ----
	struct CropG { int zero; uint32_t seed; int W, H, k; int found, x, y, w, h; };
	static const CropG crop[] = {
		{0, 5400, 5, 4, 0, 0,0,0,0,0}, {0, 5414, 5, 4, 2, 1,0,0,4,3}, {0, 5449, 5, 4, 7, 1,3,2,1,2}, {0, 5680, 5, 4, 40, 0,0,0,0,0},
		{0, 13700, 13, 7, 0, 0,0,0,0,0}, {0, 13714, 13, 7, 2, 1,0,0,13,7}, {0, 13749, 13, 7, 7, 1,3,1,9,6}, {0, 13980, 13, 7, 40, 1,1,0,12,6},
		{0, 17600, 16, 16, 0, 0,0,0,0,0}, {0, 17614, 16, 16, 2, 1,0,0,16,16}, {0, 17649, 16, 16, 7, 1,0,0,16,16}, {0, 17880, 16, 16, 40, 1,10,11,3,4},
		{1, 5405, 5, 4, 0, 1,0,0,5,4}, {1, 5419, 5, 4, 2, 1,0,0,5,4}, {1, 13705, 13, 7, 0, 1,0,0,13,7}, {1, 13985, 13, 7, 40, 1,0,0,13,7},
		{1, 17885, 16, 16, 40, 1,0,0,16,16},
	};
	for (size_t i = 0; i < sizeof(crop) / sizeof(crop[0]); ++i) {
		const CropG &g = crop[i]; Img im(g.W, g.H, gen_sparse(g.seed, g.W, g.H, g.k)); Rect r = {};
		bool f = g.zero ? crop_rect_zero(im.c(), g.W, g.H, &r) : crop_rect_alpha(im.c(), g.W, g.H, &r);
		CHECK(f == (g.found != 0)); if (f) CHECK(eq(r, g.x, g.y, g.w, g.h));
	}
	struct DiffG { uint32_t seed; int W, H, m; int found, x, y, w, h; };
	static const DiffG drect[] = {
		{5009, 5, 4, 0, 0,0,0,0,0}, {5012, 5, 4, 3, 1,1,0,4,4}, {5018, 5, 4, 9, 1,2,2,1,1}, {5069, 5, 4, 60, 0,0,0,0,0},
		{12918, 13, 7, 0, 0,0,0,0,0}, {12921, 13, 7, 3, 1,0,0,13,7}, {12927, 13, 7, 9, 1,1,1,9,1}, {12978, 13, 7, 60, 0,0,0,0,0},
		{16004, 16, 12, 0, 0,0,0,0,0}, {16007, 16, 12, 3, 1,0,0,16,6}, {16013, 16, 12, 9, 1,0,1,16,3}, {16064, 16, 12, 60, 1,9,3,7,1},
	};
	for (size_t i = 0; i < sizeof(drect) / sizeof(drect[0]); ++i) {
		const DiffG &g = drect[i];
		Buf A = gen_sparse(g.seed, g.W, g.H, 3), B = gen_mutate(g.seed + 99, A, g.m);
		// padding besar berisi 0xEE di kedua gambar (baca-luar-batas biner terbaca sama -> "tak beda", setara jepit kolom >= W)
		Img ia(g.W, g.H, A, (size_t)g.W * 4 + 16), ib(g.W, g.H, B, (size_t)g.W * 4 + 16); Rect r = {};
		bool f = diff_rect(ia.c(), ib.c(), g.W, g.H, true, &r);
		CHECK(f == (g.found != 0)); if (f) CHECK(eq(r, g.x, g.y, g.w, g.h));
	}
	struct DpG { uint32_t seed; int W, H; uint32_t sv, dv; uint64_t hash; int64_t count; };
	static const DpG dp[] = {
		{2299, 7, 5, 2164230459u, 2131770145u, 15436117575037390754ULL, 6},
		{3932, 12, 9, 2164232092u, 2131775044u, 15491528621270935816ULL, 17},
	};
	for (size_t i = 0; i < sizeof(dp) / sizeof(dp[0]); ++i) {
		const DpG &g = dp[i];
		Buf A = gen_sparse(g.seed, g.W, g.H, 3), B = gen_mutate(g.seed + 7, A, 4);
		Img ia(g.W, g.H, A), ib(g.W, g.H, B);
		int64_t c = diff_pixel(ia.c(), ib.wr(), g.W, g.H, true, g.sv, true, g.dv);
		CHECK(c == g.count && fnv1a(ib.pix()) == g.hash && ib.pad_ok() && ia.pix() == A);
	}
	struct OzG { uint32_t seed; int thr; int lv; uint32_t hash_lo; uint64_t hash; };
	static const struct { uint32_t seed; int has; uint32_t thr; uint64_t hash; } oz[] = {
		{1886, 0, 0, 14691228186614291821ULL}, {889, 1, 2, 9979881556190315012ULL}, {1144, 1, 257, 3423950589069357505ULL},
	};
	for (size_t i = 0; i < sizeof(oz) / sizeof(oz[0]); ++i) {
		const int W = 11, H = 8; Buf b = gen_sparse(oz[i].seed, W, H, 2);
		for (size_t j = 3; j < b.size(); j += 4) b[j] = (uint8_t)((b[j] % 5) * (((j / 4) % 3) ? 1 : 40));
		const uint32_t color = (0x00AABBCCu + oz[i].seed * 12345u) & 0xFFFFFFu;
		Img im(W, H, b);
		ooze_color(im.wr(), W, H, oz[i].has ? oz[i].thr : 1u, oz[i].has ? color : 0u);
		CHECK(fnv1a(im.pix()) == oz[i].hash && im.pad_ok());
	}
	static const struct { uint32_t seed; int sw, sh, dw, dh; uint64_t hash; } bl[] = {
		{639, 6, 3, 4, 5, 17244712729278516550ULL}, {896, 8, 8, 8, 8, 11068718211604765109ULL},
	};
	for (size_t i = 0; i < sizeof(bl) / sizeof(bl[0]); ++i) {
		const auto &g = bl[i];
		Img s(g.sw, g.sh, gen_sparse(g.seed, g.sw, g.sh, 2)), d(g.dw, g.dh, gen_sparse(g.seed + 1, g.dw, g.dh, 2));
		blue_to_alpha(s.c(), g.sw, g.sh, d.wr(), g.dw, g.dh);
		CHECK(fnv1a(d.pix()) == g.hash && d.pad_ok());
	}
	static const struct { uint32_t seed; int W, H, dens, x, y, w, h, blank; } ib[] = {
		{526, 9, 7, 0, 2,1,2,0, 1}, {527, 9, 7, 0, 6,6,2,0, 1}, {535, 9, 7, 3, 2,6,6,1, 0}, {536, 9, 7, 3, 3,5,4,2, 0},
		{601, 9, 7, 25, 3,5,4,2, 0}, {602, 9, 7, 25, 6,1,2,4, 0}, {918, 16, 10, 0, 14,1,0,5, 1}, {919, 16, 10, 0, 8,4,3,4, 1},
		{927, 16, 10, 3, 4,1,1,3, 0}, {928, 16, 10, 3, 9,6,2,1, 0}, {993, 16, 10, 25, 1,1,4,8, 0}, {994, 16, 10, 25, 4,8,12,1, 0},
	};
	for (size_t i = 0; i < sizeof(ib) / sizeof(ib[0]); ++i) {
		const auto &g = ib[i]; Buf b = gen_sparse(g.seed, g.W, g.H, g.dens);
		if (g.dens == 0) for (size_t j = 0; j < b.size(); j += 4) b[j] = 0;
		Img im(g.W, g.H, b);
		CHECK(blank_range_ok(g.x, g.y, g.w, g.h, g.W, g.H));
		CHECK(is_blank(im.c(), g.x, g.y, g.w, g.h) == (g.blank != 0));
	}
	struct CaG { uint32_t seed; int n; int32_t thr; uint32_t col; uint64_t hash; };
	static const CaG ca[] = {
		{282, 0, 0, 0u, 9855679781739056957ULL}, {1442, 2, 128, 16744448u, 2220760515849237172ULL},
		{652, 2, 255, 305419896u, 17550493994121428223ULL}, {737, 2, -1, 11259375u, 9718021079362808320ULL},
		{1033, 2, 300, 66051u, 8366641967037197873ULL}, {707, 2, 255, 4294967295u, 8612182198996798476ULL},
	};
	for (size_t i = 0; i < sizeof(ca) / sizeof(ca[0]); ++i) {
		const int W = 9, H = 7; Buf b = gen_sparse(ca[i].seed, W, H, 2);
		for (size_t j = 3; j < b.size(); j += 4) b[j] = (uint8_t)(((j / 4) % 2) ? b[j] : b[j] % 4);
		Img im(W, H, b);
		clear_alpha(im.wr(), W, H, ca[i].thr, ca[i].n ? ca[i].col : 0u);
		CHECK(fnv1a(im.pix()) == ca[i].hash && im.pad_ok());
	}

	printf(fails ? "GAGAL: %d\n" : "OK semua\n", fails);
	return fails ? 1 : 0;
}
