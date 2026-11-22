// g++ -std=c++14 -fsanitize=address,undefined -I src/plugins tests/plugins/layerExShrinkFast/test_core.cpp && ./a.out
// Uji inti shrinkCopyFast. Bagian [BINARY-H] = pengecil baris horizontal hrow() = fungsi 0x6023ec, dengan ekspektasi
// dari binary 1.3.9: (1) vektor tangan yang bisa dihitung ulang dari spesifikasi, (2) hash golden yang dihasilkan dengan
// MENGEKSEKUSI instruksi asli 0x6023ec di emulator (tests/plugins/layerExShrinkFast/emu_hrow_6023ec.py <recon> --golden).
// Tingkat bukti: inti-host (ASan/UBSan) + emulasi instruksi asli; BUKAN build NDK dan BUKAN uji perangkat.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <vector>
#include "layerExShrinkFastCore.hpp"
using namespace layerex_shrink;
static int fails = 0;
#define CHECK(c) do { if (!(c)) { printf("FAIL %s:%d %s\n", __FILE__, __LINE__, #c); ++fails; } } while (0)

// gambar BGRA; fungsi nilai per piksel
static std::vector<uint8_t> img(int w, int h, uint8_t (*f)(int x, int y, int ch)) {
	std::vector<uint8_t> v((size_t)w * h * 4);
	for (int y = 0; y < h; ++y) for (int x = 0; x < w; ++x) for (int c = 0; c < 4; ++c) v[((size_t)y * w + x) * 4 + c] = f(x, y, c);
	return v;
}
static uint8_t px(const std::vector<uint8_t> &v, int w, int x, int y, int c) { return v[((size_t)y * w + x) * 4 + c]; }

// sumber deterministik & hash: HARUS sama persis dengan lcg_bytes()/fnv1a() di emu_hrow_6023ec.py
static void lcg_fill(uint32_t seed, uint8_t *out, size_t n) { uint32_t s = seed; for (size_t i = 0; i < n; ++i) { s = s * 1664525u + 1013904223u; out[i] = (uint8_t)(s >> 24); } }
static uint64_t fnv1a(const uint8_t *p, size_t n) { uint64_t h = 0xCBF29CE484222325ULL; for (size_t i = 0; i < n; ++i) { h ^= p[i]; h *= 0x100000001B3ULL; } return h; }
// hrow dengan buffer berukuran PAS (ASan menangkap baca/tulis sekecil apa pun di luar batas)
static std::vector<uint8_t> run_hrow(const std::vector<uint8_t> &src, int sw, int hs)
{
	std::vector<uint8_t> in(src), out((size_t)ceil_div_wrap(sw, hs) * 4);
	hrow(out.data(), in.data(), hs, sw / hs, sw % hs);
	return out;
}

int main()
{
	// ---- ukuran tujuan: sdiv/ceil ala AArch64 ----
	CHECK(ceil_div_wrap(5, 2) == 3);  CHECK(ceil_div_wrap(4, 2) == 2);  CHECK(ceil_div_wrap(1, 7) == 1);
	CHECK(ceil_div_wrap(0, 3) == 0);                       // sw=0 -> 0 (jalur GPU lalu return tanpa kerja)
	CHECK(ceil_div_wrap(10, 0) == 0);                      // bagi nol -> 0, tidak trap
	CHECK(ceil_div_wrap(10, -1) == -8);                    // 10 + (-1) - 1 = 8 ; 8 / -1 = -8 (jalur GPU lalu return: dw < 1)
	CHECK(sdiv32(INT32_MIN, -1) == INT32_MIN);             // tanpa UB
	CHECK(ceil_div_wrap(10, INT32_MAX) == 0);              // 10+INT_MAX-1 melingkar = -2147483640 ; / INT_MAX memotong ke nol = 0
	CHECK(default_vs(3, 0) == 3 && default_vs(3, 2) == 2 && default_vs(3, -1) == -1);

	// ---- vs=1,hs=1: salinan RGB, alfa dipaksa 0xFF walau alfa sumber 0 ----
	{
		const int W = 3, H = 2;
		auto s = img(W, H, [](int x, int y, int c) -> uint8_t { return c == 3 ? 0 : (uint8_t)(10 * (x + 1) + 3 * y + c); });
		std::vector<uint8_t> d((size_t)W * H * 4, 0x77);
		shrink_fast(d.data(), W * 4, s.data(), W * 4, W, H, 1, 1);
		for (int y = 0; y < H; ++y) for (int x = 0; x < W; ++x) {
			for (int c = 0; c < 3; ++c) CHECK(px(d, W, x, y, c) == px(s, W, x, y, c));
			CHECK(px(d, W, x, y, 3) == 0xFF);
		}
	}
	// ---- vertikal: pembagian MEMOTONG (udiv), tiap kanal terpisah ----
	{   // hs=1, vs=2; kolom tunggal, 2 baris: B 1,2 -> 3/2=1 ; G 5,6 -> 11/2=5 ; R 255,254 -> 509/2=254
		uint8_t s[2][4] = { {1, 5, 255, 9}, {2, 6, 254, 9} };
		uint8_t d[4] = {0};
		shrink_fast(d, 4, &s[0][0], 4, 1, 2, 1, 2);
		CHECK(d[0] == 1 && d[1] == 5 && d[2] == 254 && d[3] == 0xFF);
	}
	// ---- sisa grup: sh=3, vs=2 -> dh=2; baris tujuan terakhir = rata-rata `rem`=1 baris (BUKAN dibagi vs) ----
	{
		uint8_t s[3][4] = { {10,10,10,0}, {20,20,20,0}, {30,30,30,0} };
		uint8_t d[2][4] = {};
		CHECK(ceil_div_wrap(3, 2) == 2);
		shrink_fast(&d[0][0], 4, &s[0][0], 4, 1, 3, 1, 2);
		CHECK(d[0][0] == 15 && d[0][1] == 15 && d[0][2] == 15 && d[0][3] == 0xFF);
		CHECK(d[1][0] == 30 && d[1][2] == 30 && d[1][3] == 0xFF);
	}
	// ---- vs > sh: tidak ada grup penuh; semua baris = satu sisa, dibagi sh (=2), bukan vs (=5) ----
	{
		uint8_t s[2][4] = { {100,0,0,0}, {50,0,0,0} };
		uint8_t d[4] = {};
		CHECK(ceil_div_wrap(2, 5) == 1);
		shrink_fast(d, 4, &s[0][0], 4, 1, 2, 1, 5);
		CHECK(d[0] == 75 && d[3] == 0xFF);
	}
	// ---- sisa 2 baris: sh=5, vs=3 -> dh=2; baris 2 = rata-rata baris 4,5 (rem=2) ----
	{
		uint8_t s[5][4]; for (int i = 0; i < 5; ++i) { s[i][0] = (uint8_t)(i * 10); s[i][1] = s[i][2] = s[i][3] = 0; }
		uint8_t d[2][4] = {};
		shrink_fast(&d[0][0], 4, &s[0][0], 4, 1, 5, 1, 3);
		CHECK(d[0][0] == (0 + 10 + 20) / 3);          // 10
		CHECK(d[1][0] == (30 + 40) / 2);              // 35
	}
	// ---- pitch bukan dw*4: padding tujuan tidak boleh tersentuh ----
	{
		const int W = 4, H = 4, hs = 2, vs = 2, DW = 2, DH = 2, DP = 16;       // pitch 16 B, piksel 8 B -> 8 B padding
		auto s = img(W, H, [](int, int, int c) -> uint8_t { return (uint8_t)(c == 3 ? 7 : 80); });
		std::vector<uint8_t> d((size_t)DP * DH, 0xAB);
		shrink_fast(d.data(), DP, s.data(), W * 4, W, H, hs, vs);
		for (int y = 0; y < DH; ++y) {
			for (int x = 0; x < DW; ++x) { CHECK(d[y*DP+x*4] == 80 && d[y*DP+x*4+3] == 0xFF); }   // [BINARY-H] rata tetap rata
			for (int p = DW * 4; p < DP; ++p) CHECK(d[y * DP + p] == 0xAB);
		}
	}
	// ---- [BINARY-H] warna rata tetap rata untuk berbagai ukuran/faktor; ASan menangkap tulis/baca di luar batas ----
	for (int sw = 1; sw <= 9; ++sw) for (int sh = 1; sh <= 9; ++sh) for (int hs = 1; hs <= 4; ++hs) for (int vs = 1; vs <= 4; ++vs) {
		auto s = img(sw, sh, [](int, int, int c) -> uint8_t { return (uint8_t)(c == 0 ? 11 : c == 1 ? 22 : c == 2 ? 33 : 44); });
		const int dw = ceil_div_wrap(sw, hs), dh = ceil_div_wrap(sh, vs);
		std::vector<uint8_t> d((size_t)dw * dh * 4);
		shrink_fast(d.data(), dw * 4, s.data(), sw * 4, sw, sh, hs, vs);
		for (int i = 0; i < dw * dh; ++i) CHECK(d[i*4] == 11 && d[i*4+1] == 22 && d[i*4+2] == 33 && d[i*4+3] == 0xFF);
	}

	// ======== [BINARY-H] hrow() = 0x6023ec ========
	// -- vektor tangan (rumus dari spesifikasi: blok penuh dibagi hs, sisa dibagi rem, udiv memotong, alfa 0xFF) --
	{   // sw=5, hs=2: dua blok penuh + sisa 1 piksel (disalin apa adanya). Alfa sumber 1..5 diabaikan.
		const uint8_t s[5][4] = { {10,20,30,1}, {11,21,31,2}, {200,100,0,3}, {201,101,1,4}, {7,8,9,5} };
		std::vector<uint8_t> in(&s[0][0], &s[0][0] + 20);
		auto d = run_hrow(in, 5, 2);
		const uint8_t e[3][4] = { {10,20,30,0xFF}, {200,100,0,0xFF}, {7,8,9,0xFF} };       // (10+11)/2=10, (20+21)/2=20, (30+31)/2=30 ...
		CHECK(d.size() == 12 && memcmp(d.data(), e, 12) == 0);
	}
	{   // sw=8, hs=3: B = 255,255,254 | 1,2,2 | 100,51 -> 764/3=254 (254,67 DIPOTONG), 5/3=1 (1,67 dipotong), sisa 2 piksel: 151/2=75 (75,5 dipotong)
		const uint8_t b[8] = { 255, 255, 254, 1, 2, 2, 100, 51 };
		std::vector<uint8_t> in(32, 0); for (int i = 0; i < 8; ++i) in[i * 4] = b[i];
		auto d = run_hrow(in, 8, 3);
		CHECK(d.size() == 12 && d[0] == 254 && d[4] == 1 && d[8] == 75);
		CHECK(d[3] == 0xFF && d[7] == 0xFF && d[11] == 0xFF && d[1] == 0 && d[2] == 0);
	}
	{   // hs > sw: tidak ada blok penuh (quot=0); satu piksel tujuan dari rem=2 piksel, dibagi 2 (BUKAN hs=4)
		std::vector<uint8_t> in = { 10, 0, 0, 0,  21, 0, 0, 0 };
		auto d = run_hrow(in, 2, 4);
		CHECK(d.size() == 4 && d[0] == 15 && d[3] == 0xFF);
	}
	{   // hs=1: salinan B,G,R; alfa sumber 0 -> tujuan 0xFF
		std::vector<uint8_t> in = { 1, 2, 3, 0,  4, 5, 6, 0 };
		auto d = run_hrow(in, 2, 1);
		const uint8_t e[8] = { 1, 2, 3, 0xFF, 4, 5, 6, 0xFF };
		CHECK(d.size() == 8 && memcmp(d.data(), e, 8) == 0);
	}
	{   // sw=0 (quot=0, rem=0): tidak menulis apa pun
		uint8_t sentinel[4] = { 0x5A, 0x5A, 0x5A, 0x5A };
		hrow(sentinel, nullptr, 3, 0, 0);
		CHECK(sentinel[0] == 0x5A && sentinel[3] == 0x5A);
	}
	{   // blok besar tidak meluap: 70000 piksel 255 / hs=70000 -> 255 (jumlah 17.850.000 muat 32-bit)
		const int sw = 70000;
		std::vector<uint8_t> in((size_t)sw * 4, 0xFF);
		auto d = run_hrow(in, sw, sw);
		CHECK(d.size() == 4 && d[0] == 255 && d[1] == 255 && d[2] == 255 && d[3] == 0xFF);
	}
	// -- hash golden dari EKSEKUSI instruksi asli 0x6023ec (emulator) -- sumber = lcg(seed = sw*31+hs, sw*4 byte) --
	{
		static const struct { int sw, hs; uint64_t h; } G[] = {
			{1, 1, 0xE164F72309E9C483ULL},
			{7, 3, 0xF98564C05555CE1CULL},
			{8, 3, 0xF0FF70F82052E46BULL},
			{9, 3, 0x0888B226F70C6FBCULL},
			{2, 5, 0x14E9A96672BE34C0ULL},
			{10, 2, 0x6A05650B4C183AE4ULL},
			{13, 5, 0xF8CF88903F0872ECULL},
			{16, 4, 0xFF8A031066FFAF5CULL},
			{17, 4, 0x3821A3B4B35F4B9BULL},
			{64, 7, 0xA532A223B474CF17ULL},
			{255, 7, 0xA6AECF24953846B1ULL},
			{1000, 3, 0xB862B2DB6D793112ULL},
			{1023, 1000, 0xD595EC2CFA6A0F8EULL},
		};
		for (const auto &g : G) {
			std::vector<uint8_t> in((size_t)g.sw * 4);
			lcg_fill((uint32_t)(g.sw * 31 + g.hs), in.data(), in.size());
			auto d = run_hrow(in, g.sw, g.hs);
			if (fnv1a(d.data(), d.size()) != g.h) { printf("FAIL golden hrow sw=%d hs=%d\n", g.sw, g.hs); ++fails; }
			for (size_t i = 3; i < in.size(); i += 4) in[i] ^= 0xA5;                  // alfa sumber tidak boleh berpengaruh
			auto d2 = run_hrow(in, g.sw, g.hs);
			CHECK(d2 == d);
		}
	}
	// -- end-to-end lewat shrink_fast: hs>1, vs=1 (jalur 0x601e18) memakai hrow langsung pada tiap baris --
	{
		const int sw = 7, sh = 3, hs = 3;                                              // dw = 3 (2 blok + sisa 1)
		std::vector<uint8_t> in((size_t)sw * sh * 4); lcg_fill(777, in.data(), in.size());
		std::vector<uint8_t> d((size_t)3 * sh * 4);
		shrink_fast(d.data(), 3 * 4, in.data(), sw * 4, sw, sh, hs, 1);
		for (int y = 0; y < sh; ++y) {
			std::vector<uint8_t> row(in.begin() + (size_t)y * sw * 4, in.begin() + (size_t)(y + 1) * sw * 4);
			auto e = run_hrow(row, sw, hs);
			CHECK(memcmp(&d[(size_t)y * 12], e.data(), 12) == 0);
		}
	}
	printf(fails ? "GAGAL: %d\n" : "OK semua\n", fails);
	return fails != 0;
}
