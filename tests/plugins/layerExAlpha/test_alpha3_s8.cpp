// Uji inti layerExAlpha: MaskImageRideRect4 / MaskImageRideRect5 (S8).
// Ekspektasi dari binary 1.3.9: (1) vektor tangan yang bisa dihitung ulang dari spesifikasi, (2) hash golden yang dihasilkan
// dengan MENGEKSEKUSI seluruh fungsi asli di emulator (tests/plugins/layerExAlpha/emu_ride.py <recon_alpha3.txt> --golden).
// Tingkat bukti: inti-host (ASan/UBSan) + emulasi instruksi asli; BUKAN build NDK dan BUKAN uji perangkat.
// Bangun: g++ -std=c++14 -O2 -ffp-contract=off -Wall -Wextra -fsanitize=address,undefined -I src/plugins test_core.cpp
//   (-ffp-contract=off hanya perlu bila kompiler/target punya FMA dan default-nya melebur; inti sudah menutupnya untuk clang.)
#include "layerExAlphaCore.hpp"
#include <stdio.h>
#include <string.h>
#include <vector>
#include <algorithm>

using namespace layerex_alpha;
static int fails = 0;
#define CHECK(c) do { if (!(c)) { printf("FAIL %s:%d  %s\n", __FILE__, __LINE__, #c); ++fails; } } while (0)

// ---- sumber deterministik & hash: HARUS sama persis dengan lcg_bytes()/make_case()/fnv1a() di emu_ride.py ----
static void lcg_fill(uint32_t seed, uint8_t *out, size_t n) { uint32_t s = seed; for (size_t i = 0; i < n; ++i) { s = s * 1664525u + 1013904223u; out[i] = (uint8_t)(s >> 24); } }
static uint64_t fnv1a(const uint8_t *p, size_t n) { uint64_t h = 0xCBF29CE484222325ULL; for (size_t i = 0; i < n; ++i) { h ^= p[i]; h *= 0x100000001B3ULL; } return h; }
// gambar ke-k (0 = tujuan, 1 = layer1, 2 = layer2); pitch = w*4+pad
static std::vector<uint8_t> make_img(uint32_t seed, int k, int w, int h, int pad, int mode)
{
	const size_t pitch = (size_t)w * 4 + (size_t)pad;
	std::vector<uint8_t> raw(pitch * (size_t)h);
	lcg_fill(seed * 3 + (uint32_t)k, raw.data(), raw.size());
	if (mode) for (int y = 0; y < h; ++y) for (int x = 0; x < w; ++x) {
		uint8_t &a = raw[(size_t)y * pitch + (size_t)x * 4 + 3]; const unsigned r = a;
		a = (uint8_t)((r % 5 == 0 && k != 2) ? 0 : (k == 2 ? r : r % 200));
	}
	return raw;
}

// ---- (1) vektor tangan ----
static void hand_vectors()
{
	// band(a, n): lo = (a-n<0 ? 0 : a-n+1) dibatasi 255 ; hi = max(n+a,0) dibatasi 255 ; aritmatika int32 melingkar
	{ Band b = band(10, 2);  CHECK(b.lo == 9   && b.hi == 12); }
	{ Band b = band(3, 8);   CHECK(b.lo == 0   && b.hi == 11); }            // a < n : lo = 0 (tabel TIDAK bergeser -> pita tak simetris)
	{ Band b = band(300, 5); CHECK(b.lo == 255 && b.hi == 255); }
	{ Band b = band(-7, 3);  CHECK(b.lo == 0   && b.hi == 0); }
	{ Band b = band(INT_MIN, 1); CHECK(b.lo == INT_MIN && b.hi == 0); }      // a-n melingkar -> lo negatif (binary memakainya apa adanya)
	// fcvtzs
	CHECK(cvtzs(2.9) == 2 && cvtzs(-2.9) == -2 && cvtzs(0.5) == 0);
	CHECK(cvtzs(1e20) == INT_MAX && cvtzs(-1e20) == INT_MIN);
	{ volatile double z = 0.0; CHECK(cvtzs(z / z) == 0); }                   // NaN -> 0

	// tabel Rect4: n=3, c0=255, c1=0, c2=300  (m=5)
	{
		Table4 t; build_table4(3, 255, 0, 300, t);
		CHECK(t.vf.size() == 5);
		const float third = 1.0f / 3.0f;
		CHECK(t.vf[0] == third * 2.0f && t.vf[1] == third * 1.0f && t.vf[2] == 0.0f && t.vf[3] == third * 1.0f && t.vf[4] == third * 2.0f);
		const int e1[5] = { 85, 170, 255, 170, 85 };                         // trunc((255/3)*t + .5), pusat = 255 mentah
		const int e2[5] = { 0, 0, 0, 0, 0 };
		const int e3[5] = { 100, 200, 255, 200, 100 };                        // pusat 300 -> diklem 255
		for (int i = 0; i < 5; ++i) { CHECK(t.i1[i] == e1[i]); CHECK(t.i2[i] == e2[i]); CHECK(t.i3[i] == e3[i]); }
	}
	// tabel Rect4 n=48, c=(100,200,255): urutan float (c/n)*t memberi 62/62/112 pada idx 29/14/26 (dan simetrinya 65/68);
	// urutan (c*t)/n akan memberi 63/63/113. Nilai ini dibaca dari eksekusi instruksi asli (emu_ride.py: probe_table).
	{
		Table4 t; build_table4(48, 100, 200, 255, t);
		CHECK(t.i1[29] == 62 && t.i1[65] == 62);                              // R: t=30
		CHECK(t.i2[14] == 62 && t.i2[26] == 112 && t.i2[68] == 112);          // G: t=15 dan t=27
	}
	// tabel Rect5: n=2 (m=3): B = .5,0,0 ; C = .5,1,1 ; A = 1,1,.5
	{
		Table5 t; build_table5(2, t);
		CHECK(t.B[0] == 0.5f && t.B[1] == 0.0f && t.B[2] == 0.0f);
		CHECK(t.C[0] == 0.5f && t.C[1] == 1.0f && t.C[2] == 1.0f);
		CHECK(t.A[0] == 1.0f && t.A[1] == 1.0f && t.A[2] == 0.5f);
	}

	// ---- Rect4: n=2, a=10, (c0,c1,c2) = (R=200, G=100, B=50). Tabel: vf = .5,0,.5 ; i1 = 100,200,100 ; i2 = 50,100,50 ; i3 = 25,50,25
	//      pita: lo=9, hi=12. Tiap piksel BGRA. layer1 = (100,60,20, alfa 7) di semua posisi.
	{
		const int W = 7;
		const uint8_t d0[W][4] = { {1,2,3,255}, {9,9,9,255}, {4,4,4,255}, {80,40,21,255}, {5,6,7,255}, {9,9,9,0}, {9,9,9,0} };
		const uint8_t a2s[W]   = { 8, 9, 10, 11, 12, 10, 10 };
		uint8_t l1[W][4], l2[W][4], dst[W][4];
		for (int i = 0; i < W; ++i) { l1[i][0] = 100; l1[i][1] = 60; l1[i][2] = 20; l1[i][3] = (i == 5) ? 0 : 7; l2[i][0] = l2[i][1] = l2[i][2] = 0; l2[i][3] = a2s[i]; }
		memcpy(dst, d0, sizeof dst);
		ride4(&dst[0][0], W * 4, &l1[0][0], W * 4, &l2[0][0], W * 4, W, 1, 10, 2, 200, 100, 50);
		const uint8_t e[W][4] = {
			{100, 60, 20, 255},    // a2=8  < lo : salin rgb layer1
			{ 75, 80, 110, 255},   // a2=9  : double: 25+trunc(100*.5+.5), 50+trunc(60*.5+.5), 100+trunc(20*.5+.5)
			{ 50, 100, 200, 255},  // a2=10 : pusat, vf=0 -> warna mentah (B=50,G=100,R=200), tujuan lama tidak berpengaruh
			{ 65, 70, 111, 255},   // a2=11 : float: 25+trunc(.5*80+.5), 50+trunc(.5*40+.5), 100+trunc(.5*21+.5)
			{  5,  6,   7, 255},   // a2=12 >= hi : tak berubah
			{  9,  9,   9,   0},   // alfa tujuan 0 && alfa layer1 0 : dilewati
			{ 50, 100, 200,   0} };// alfa tujuan 0 tetapi alfa layer1 != 0 : diproses; alfa tujuan tetap 0
		CHECK(memcmp(dst, e, sizeof dst) == 0);
	}
	// ---- Rect4: n=1 -> tabel hanya pusat. a=100: lo=100, hi=101
	{
		uint8_t dst[3][4] = { {1,2,3,255}, {1,2,3,255}, {1,2,3,255} };
		uint8_t l1[3][4]  = { {9,8,7,255}, {9,8,7,255}, {9,8,7,255} };
		uint8_t l2[3][4]  = { {0,0,0,99}, {0,0,0,100}, {0,0,0,101} };
		ride4(&dst[0][0], 12, &l1[0][0], 12, &l2[0][0], 12, 3, 1, 100, 1, 10, 20, 30);
		const uint8_t e[3][4] = { {9,8,7,255}, {30,20,10,255}, {1,2,3,255} };   // salin ; pusat (B=30,G=20,R=10) ; tak berubah
		CHECK(memcmp(dst, e, sizeof dst) == 0);
	}
	// ---- Rect4: nilai melingkar 8-bit (tanpa klem pada penyimpanan): i + trunc(...) > 255
	{
		uint8_t dst[1][4] = { {200, 200, 200, 255} };
		uint8_t l1[1][4]  = { {0, 0, 0, 255} };
		uint8_t l2[1][4]  = { {0, 0, 0, 12} };                                   // n=3, a=10: lo=8 hi=13 ; idx = 4 -> i = 100 (c=255 -> t=1 -> 85?) dihitung di bawah
		ride4(&dst[0][0], 4, &l1[0][0], 4, &l2[0][0], 4, 1, 1, 10, 3, 255, 255, 255);
		Table4 t; build_table4(3, 255, 255, 255, t);
		const int idx = 12 - band(10, 3).lo;
		const float r = t.vf[idx] * 200.0f + 0.5f;
		CHECK(dst[0][0] == (uint8_t)(t.i3[idx] + (int)r) && dst[0][1] == dst[0][0] && dst[0][2] == dst[0][0]);
	}

	// ---- Rect5: n=2, a=10 -> lo=9, hi=12. layer1 = (100,60,20, alfa 200)
	{
		const int W = 6;
		const uint8_t d0[W][4] = { {1,1,1,255}, {50,20,10,255}, {7,7,7,255}, {7,7,7,255}, {7,8,9,255}, {7,7,7,0} };
		const uint8_t a2s[W]   = { 8, 9, 10, 11, 12, 10 };
		uint8_t l1[W][4], l2[W][4], dst[W][4];
		for (int i = 0; i < W; ++i) { l1[i][0] = 100; l1[i][1] = 60; l1[i][2] = 20; l1[i][3] = (i == 5) ? 0 : 200; l2[i][0] = l2[i][1] = l2[i][2] = 0; l2[i][3] = a2s[i]; }
		memcpy(dst, d0, sizeof dst);
		ride5(&dst[0][0], W * 4, &l1[0][0], W * 4, &l2[0][0], W * 4, W, 1, 10, 2);
		const uint8_t e[W][4] = {
			{ 1,  1,  1, 255},     // a2=8 < lo : tak berubah
			{ 75, 40, 15, 255},    // a2=9 : trunc(.5*50+.5)+trunc(.5*100+.5)=25+50 ; 10+30 ; 5+10 ; alfa tetap
			{100, 60, 20, 200},    // a2=10: salin rgb layer1 ; alfa = trunc(1.0*200+.5) = 200
			{100, 60, 20, 100},    // a2=11: alfa = trunc(.5*200+.5) = 100
			{  7,  8,  9,   0},    // a2=12 >= hi : alfa tujuan = 0, rgb tetap
			{  7,  7,  7,   0} };  // alfa tujuan 0 && alfa layer1 0 : dilewati
		CHECK(memcmp(dst, e, sizeof dst) == 0);
	}
	// ---- Rect5: batas kolom = xcount (binary: 2n-1). Kolom di luar xcount tidak disentuh.
	{
		uint8_t dst[4][4], l1[4][4], l2[4][4];
		for (int i = 0; i < 4; ++i) { dst[i][0] = dst[i][1] = dst[i][2] = 3; dst[i][3] = 255; l1[i][0] = 9; l1[i][1] = 9; l1[i][2] = 9; l1[i][3] = 255; l2[i][0] = l2[i][1] = l2[i][2] = 0; l2[i][3] = 20; }
		ride5(&dst[0][0], 16, &l1[0][0], 16, &l2[0][0], 16, 2, 1, 20, 2);      // xcount=2: hanya 2 piksel pertama
		CHECK(dst[0][0] == 9 && dst[1][0] == 9 && dst[2][0] == 3 && dst[3][0] == 3);
	}
	// ---- w, h, xcount = 0 : tidak melakukan apa pun
	{
		uint8_t dst[4] = { 1, 2, 3, 255 }, l1[4] = { 9, 9, 9, 9 }, l2[4] = { 0, 0, 0, 20 };
		ride4(dst, 4, l1, 4, l2, 4, 0, 1, 10, 2, 1, 2, 3); ride4(dst, 4, l1, 4, l2, 4, 1, 0, 10, 2, 1, 2, 3);
		ride5(dst, 4, l1, 4, l2, 4, 0, 1, 10, 2);          ride5(dst, 4, l1, 4, l2, 4, 1, 0, 10, 2);
		CHECK(dst[0] == 1 && dst[1] == 2 && dst[2] == 3 && dst[3] == 255);
	}
	// ---- parameter ekstrem tidak boleh keluar batas (pengaman indeks): a = INT_MIN / INT_MAX, n besar
	{
		std::vector<uint8_t> d(16 * 4, 0x80), s1(16 * 4, 0x90), s2(16 * 4, 0xA0);
		const int as[] = { INT_MIN, INT_MIN + 1, -1, 0, 255, 256, INT_MAX - 1, INT_MAX };
		const int ns[] = { 1, 2, 7, 300 };
		for (int a : as) for (int n : ns) {
			std::vector<uint8_t> x = d; ride4(x.data(), 64, s1.data(), 64, s2.data(), 64, 16, 1, a, n, 5, 6, 7);
			std::vector<uint8_t> y = d; ride5(y.data(), 64, s1.data(), 64, s2.data(), 64, 16, 1, a, n);
		}
	}
}

// ---- (2) hash golden dari EKSEKUSI instruksi asli (emulator) ----
struct G { uint32_t seed; int w, h, padD, pad1, pad2, mode, a, n, c0, c1, c2; uint64_t hash; };
static const G G4[] = {
	{11, 40, 30, 0, 0, 0, 1, 128, 20, 200, 100, 50, 0x91EBE9E40FE5BAE4ULL},
	{12, 33, 17, 8, 4, 12, 1, 60, 5, 255, 0, 255, 0x26C6B46BC133A9BAULL},
	{13, 64, 8, 0, 0, 0, 0, 200, 100, 1000, -5, 300, 0xD68AC0083430E865ULL},
	{14, 16, 16, 4, 0, 8, 1, 3, 8, 77, 88, 99, 0x2D23C1E4662AA752ULL},
	{15, 50, 5, 0, 8, 0, 1, 255, 1, 10, 20, 30, 0x8E4497CF5101F18EULL},
	{16, 31, 31, 0, 0, 0, 1, 100, 40, 0, 0, 0, 0x8249FD4345193901ULL},
	{17, 48, 24, 0, 0, 0, 1, 40, 25, 12345, 255, 1, 0x8CD8B149AB502C0EULL},
};
static const G G5[] = {
	{21, 40, 30, 0, 0, 0, 1, 128, 20, 0, 0, 0, 0x8D71297988AF025EULL},
	{22, 33, 17, 8, 4, 12, 1, 60, 5, 0, 0, 0, 0x47A52766E92CF0BCULL},
	{23, 64, 8, 0, 0, 0, 0, 200, 30, 0, 0, 0, 0xD2D67AF027CBB90DULL},
	{24, 16, 16, 4, 0, 8, 1, 3, 8, 0, 0, 0, 0x13801D1041CECAC2ULL},
	{25, 50, 5, 0, 8, 0, 1, 255, 1, 0, 0, 0, 0x1A73817234FAA351ULL},
	{26, 10, 6, 24, 24, 24, 1, 20, 8, 0, 0, 0, 0xF0B9CF7FA70B926EULL},
	{27, 48, 24, 8, 8, 8, 1, 40, 25, 0, 0, 0, 0xA23CAB638F7A7CC7ULL},
};

static void golden()
{
	for (const G &g : G4) {
		std::vector<uint8_t> D = make_img(g.seed, 0, g.w, g.h, g.padD, g.mode);
		std::vector<uint8_t> L1 = make_img(g.seed + 1, 1, g.w, g.h, g.pad1, g.mode), L2 = make_img(g.seed + 2, 2, g.w, g.h, g.pad2, g.mode);
		ride4(D.data(), g.w * 4 + g.padD, L1.data(), g.w * 4 + g.pad1, L2.data(), g.w * 4 + g.pad2, g.w, g.h, g.a, g.n, g.c0, g.c1, g.c2);
		if (fnv1a(D.data(), D.size()) != g.hash) { printf("FAIL golden Rect4 seed=%u\n", g.seed); ++fails; }
	}
	for (const G &g : G5) {
		std::vector<uint8_t> D = make_img(g.seed, 0, g.w, g.h, g.padD, g.mode);
		std::vector<uint8_t> L1 = make_img(g.seed + 1, 1, g.w, g.h, g.pad1, g.mode), L2 = make_img(g.seed + 2, 2, g.w, g.h, g.pad2, g.mode);
		// perilaku binary: kolom 0 .. 2n-2 (golden dipilih agar tetap di dalam buffer; lihat spesifikasi)
		ride5(D.data(), g.w * 4 + g.padD, L1.data(), g.w * 4 + g.pad1, L2.data(), g.w * 4 + g.pad2, 2 * g.n - 1, g.h, g.a, g.n);
		if (fnv1a(D.data(), D.size()) != g.hash) { printf("FAIL golden Rect5 seed=%u\n", g.seed); ++fails; }
	}
}

int main()
{
	hand_vectors();
	golden();
	if (fails) { printf("GAGAL: %d\n", fails); return 1; }
	printf("OK semua\n");
	return 0;
}
