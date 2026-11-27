// Uji inti layerExAlphaCore.hpp (S9). Bangun: g++ -std=c++14 -O1 -g -fsanitize=address,undefined -Isrc/plugins test_core.cpp
// Catatan tingkat bukti: ini menguji KONSISTENSI kode dengan spesifikasi (docs/spec/layerExAlpha_*.md) yang ditulis dari
// disassembly. Bukan uji terhadap biner 1.3.9 (itu 7.3, butuh perangkat).
#include "layerExAlphaCore.hpp"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <limits.h>
#include <vector>
using namespace layerex_alpha;

static int g_fail = 0;
#define CHECK(c) do { if (!(c)) { printf("GAGAL %s:%d  %s\n", __FILE__, __LINE__, #c); g_fail++; } } while (0)

static void px(std::vector<uint8_t> &v, int i, int b, int g, int r, int a) { v[i*4+0]=b; v[i*4+1]=g; v[i*4+2]=r; v[i*4+3]=a; }

static void test_table()
{
	std::vector<float> t;
	CHECK(ride_table(1, t) && t.size() == 1 && t[0] == 1.0f);
	CHECK(ride_table(2, t) && t.size() == 3 && t[0] == 0.5f && t[1] == 1.0f && t[2] == 0.5f);
	CHECK(ride_table(3, t) && t.size() == 5);
	const float inv3 = 1.0f / 3.0f;
	CHECK(t[0] == 1 * inv3 && t[1] == 2 * inv3 && t[2] == 1.0f && t[3] == 2 * inv3 && t[4] == 1 * inv3);
	CHECK(!ride_table(0, t)); CHECK(!ride_table(-1, t)); CHECK(!ride_table(INT_MIN, t)); CHECK(!ride_table(1 << 30, t));
	// simetris, puncak 1.0 di b-1, semua dalam [0,1]
	for (int b = 1; b <= 120; b++) {
		CHECK(ride_table(b, t));
		const int n = 2 * b - 1;
		CHECK((int)t.size() == n);
		CHECK(t[b - 1] == 1.0f);
		for (int i = 0; i < n; i++) { CHECK(t[i] > 0.0f && t[i] <= 1.0f); CHECK(t[i] == t[n - 1 - i]); }
	}
	// dipotong ke 256 entri untuk b besar, nilai sama dengan rumus
	CHECK(ride_table(200, t) && t.size() == 256);
	const float inv = 1.0f / 200.0f;
	CHECK(t[0] == 1 * inv && t[198] == 199 * inv && t[199] == 1.0f && t[200] == (float)(399 - 200) * inv);
	CHECK(ride_table(1 << 29, t) && t.size() == 256);
}

static void test_range()
{
	RideRange r;
	r = ride_range(100, 10); CHECK(r.lo == 91 && r.hi == 110);
	r = ride_range(5, 10);   CHECK(r.lo == 0  && r.hi == 15);
	r = ride_range(-20, 10); CHECK(r.lo == 0  && r.hi == 0);
	r = ride_range(300, 10); CHECK(r.lo == 255 && r.hi == 255);
	r = ride_range(10, 10);  CHECK(r.lo == 1  && r.hi == 20);
	r = ride_range(250, 10); CHECK(r.lo == 241 && r.hi == 255);
	// Tidak boleh ada indeks tabel di luar jangkauan untuk |a| wajar (bukti dari aritmetika, diuji lengkap).
	std::vector<float> t;
	for (int b = 1; b <= 300; b++) {
		CHECK(ride_table(b, t));
		for (int a = -700; a <= 900; a++) {
			RideRange q = ride_range(a, b);
			for (int m = 0; m < 256; m++) {
				if (q.lo > m) continue;
				if (q.hi <= m) continue;
				const long idx = (long)m - q.lo;
				if (!(idx >= 0 && idx < (long)t.size())) { printf("indeks luar a=%d b=%d m=%d idx=%ld n=%zu\n", a, b, m, idx, t.size()); g_fail++; goto next; }
			}
		}
		next:;
	}
}

static void test_ride7_hand()
{
	// b=3, a=100 -> lo=98 hi=103, tabel [1/3,2/3,1,2/3,1/3]
	const int W = 8;
	std::vector<uint8_t> dst(W*4), img(W*4), mask(W*4);
	for (int i = 0; i < W; i++) { px(dst, i, 1,2,3, 255); px(img, i, 10+i, 20+i, 30+i, 200); px(mask, i, 0,0,0, 0); }
	const int ma[W] = { 97, 98, 100, 102, 103, 255, 100, 100 };
	for (int i = 0; i < W; i++) mask[i*4+3] = (uint8_t)ma[i];
	px(dst, 6, 7,7,7, 0); px(img, 6, 9,9,9, 0);      // dst.a=0 dan img.a=0 -> tak disentuh
	px(dst, 7, 7,7,7, 0);                            // dst.a=0 tapi img.a=200 -> diproses
	std::vector<float> t; CHECK(ride_table(3, t));
	std::vector<uint8_t> d0 = dst;
	ride7(dst.data(), W*4, img.data(), W*4, mask.data(), W*4, W, 1, 100, 3, t);
	// m=97 (<lo): tak berubah
	CHECK(dst[0]==1 && dst[1]==2 && dst[2]==3 && dst[3]==255);
	// m=98 idx0: 0.33333334*200+0.5 = 67.17 -> 67, warna disalin dari img
	CHECK(dst[4]==11 && dst[5]==21 && dst[6]==31 && dst[7]==67);
	// m=100 idx2: 1.0*200+0.5 -> 200
	CHECK(dst[8]==12 && dst[9]==22 && dst[10]==32 && dst[11]==200);
	// m=102 idx4: 67
	CHECK(dst[12]==13 && dst[13]==23 && dst[14]==33 && dst[15]==67);
	// m=103 (>=hi): alfa 0, warna tetap
	CHECK(dst[16]==1 && dst[17]==2 && dst[18]==3 && dst[19]==0);
	// m=255: alfa 0
	CHECK(dst[20]==1 && dst[23]==0);
	// piksel 6: tidak disentuh sama sekali
	CHECK(memcmp(&dst[24], &d0[24], 4) == 0);
	// piksel 7: dst.a=0, img.a=200, m=100 idx2 -> warna img, alfa 200
	CHECK(dst[28]==17 && dst[29]==27 && dst[30]==37 && dst[31]==200);
}

static void test_ride8_hand()
{
	const int W = 6;
	std::vector<uint8_t> dst(W*4), img(W*4), mask(W*4);
	for (int i = 0; i < W; i++) { px(dst, i, 1,2,3, 255); px(img, i, 10+i,20+i,30+i, 77); px(mask, i, 0,0,0, 0); }
	const int ma[W] = { 100, 101, 0, 255, 150, 150 };
	for (int i = 0; i < W; i++) mask[i*4+3] = (uint8_t)ma[i];
	px(dst, 4, 5,5,5, 0); px(img, 4, 8,8,8, 0);     // kedua alfa 0 -> lewati meski mask 150
	px(dst, 5, 5,5,5, 0);                            // dst.a=0 img.a=77 -> diproses
	std::vector<uint8_t> d0 = dst;
	ride8(dst.data(), W*4, img.data(), W*4, mask.data(), W*4, W, 1, 100);
	CHECK(memcmp(&dst[0], &d0[0], 4) == 0);                       // m=100 tidak > 100
	CHECK(dst[4]==11 && dst[5]==21 && dst[6]==31 && dst[7]==77);  // m=101 > 100 -> salin 4 byte
	CHECK(memcmp(&dst[8], &d0[8], 4) == 0);
	CHECK(dst[12]==13 && dst[15]==77);                            // m=255
	CHECK(memcmp(&dst[16], &d0[16], 4) == 0);                     // kedua alfa 0
	CHECK(dst[20]==15 && dst[23]==77);
	// th negatif: semua piksel aktif disalin; th=255: tak ada
	std::vector<uint8_t> a = d0, b = d0;
	ride8(a.data(), W*4, img.data(), W*4, mask.data(), W*4, W, 1, -5);
	CHECK(a[7]==77 && a[3]==77 && memcmp(&a[16], &d0[16], 4) == 0);
	ride8(b.data(), W*4, img.data(), W*4, mask.data(), W*4, W, 1, 255);
	CHECK(b == d0);
}

static void test_border_hand()
{
	const int W = 5;
	std::vector<uint8_t> dst(W*4), mask(W*4);
	for (int i = 0; i < W; i++) { px(dst, i, 1,2,3, 200); px(mask, i, 0,0,0, 0); }
	const int ma[W] = { 0, 127, 128, 129, 255 };
	for (int i = 0; i < W; i++) mask[i*4+3] = (uint8_t)ma[i];
	std::vector<uint8_t> d0 = dst, a = dst, b = dst;
	border_inner(dst.data(), W*4, mask.data(), W*4, W, 1, 128);
	CHECK(dst[3]==200 && dst[7]==200 && dst[11]==0 && dst[15]==0 && dst[19]==0);
	CHECK(dst[8]==1 && dst[9]==2 && dst[10]==3);                  // warna tetap
	border_inner(a.data(), W*4, mask.data(), W*4, W, 1, 0);       // th<=0: semua alfa 0
	for (int i = 0; i < W; i++) CHECK(a[i*4+3] == 0);
	border_inner(b.data(), W*4, mask.data(), W*4, W, 1, 256);     // th>255: tak ada
	CHECK(b == d0);
}

// Referensi independen (rumus langsung tanpa tabel) untuk acak.
static void ref7(uint8_t *dst, int dp, const uint8_t *img, int ip, const uint8_t *mask, int mp, int w, int h, int a, int b)
{
	const long n = 2L * b - 1;
	const float inv = 1.0f / (float)b;
	auto wt = [&](long i) -> float {
		float v;
		if (i == b - 1) v = 1.0f;
		else if (i < b - 1) v = inv * (float)(i + 1);
		else v = inv * (float)(n - i);
		if (v > 1.0f) v = 1.0f;
		return v > 0.0f ? v : 0.0f;
	};
	long lo = (a - b < 0) ? 0 : (a - b + 1); if (lo > 255) lo = 255;
	long hi = a + b; if (hi < 0) hi = 0; if (hi > 255) hi = 255;
	for (int y = 0; y < h; y++) for (int x = 0; x < w; x++) {
		uint8_t *d = dst + (long)y*dp + x*4; const uint8_t *s = img + (long)y*ip + x*4; const uint8_t *m = mask + (long)y*mp + x*4;
		if (!d[3] && !s[3]) continue;
		long mv = m[3];
		if (mv < lo) continue;
		if (mv >= hi) { d[3] = 0; continue; }
		d[0]=s[0]; d[1]=s[1]; d[2]=s[2];
		volatile float p = wt(mv - lo) * (float)s[3];
		d[3] = (uint8_t)(int)(p + 0.5f);
	}
}

static void test_random()
{
	srand(12345);
	for (int iter = 0; iter < 4000; iter++) {
		const int w = 1 + rand() % 9, h = 1 + rand() % 9;
		const int dp = w*4 + 4*(rand()%3), ip = w*4 + 4*(rand()%3), mp = w*4 + 4*(rand()%3);   // pitch dengan padding
		std::vector<uint8_t> dst((size_t)dp*h), img((size_t)ip*h), mask((size_t)mp*h);
		for (auto &v : dst) v = (uint8_t)(rand() % 3 == 0 ? 0 : rand());
		for (auto &v : img) v = (uint8_t)(rand() % 3 == 0 ? 0 : rand());
		for (auto &v : mask) v = (uint8_t)(rand() % 256);
		const int a = rand() % 700 - 200, b = 1 + rand() % 140;
		std::vector<uint8_t> d1 = dst, d2 = dst;
		std::vector<float> t; CHECK(ride_table(b, t));
		ride7(d1.data(), dp, img.data(), ip, mask.data(), mp, w, h, a, b, t);
		ref7(d2.data(), dp, img.data(), ip, mask.data(), mp, w, h, a, b);
		CHECK(d1 == d2);                                          // padding tak disentuh juga
		// ride8 / border: padding tidak boleh berubah
		std::vector<uint8_t> e = dst; ride8(e.data(), dp, img.data(), ip, mask.data(), mp, w, h, rand()%300-20);
		for (int y = 0; y < h; y++) for (int i = w*4; i < dp; i++) CHECK(e[(size_t)y*dp+i] == dst[(size_t)y*dp+i]);
		std::vector<uint8_t> f = dst; border_inner(f.data(), dp, mask.data(), mp, w, h, rand()%300-20);
		for (int y = 0; y < h; y++) for (int i = w*4; i < dp; i++) CHECK(f[(size_t)y*dp+i] == dst[(size_t)y*dp+i]);
	}
	// dst == img == mask (alias) tidak boleh merusak memori
	std::vector<uint8_t> x(5*4*5); for (auto &v : x) v = (uint8_t)rand();
	std::vector<float> t; CHECK(ride_table(4, t));
	ride7(x.data(), 20, x.data(), 20, x.data(), 20, 5, 5, 128, 4, t);
	ride8(x.data(), 20, x.data(), 20, x.data(), 20, 5, 5, 10);
	border_inner(x.data(), 20, x.data(), 20, 5, 5, 10);
}

static void test_extremes()
{
	// a dekat batas int: tidak boleh baca luar tabel (ASan akan menangkap)
	std::vector<float> t; CHECK(ride_table(5, t));
	std::vector<uint8_t> dst(16*4, 200), img(16*4, 200), mask(16*4);
	for (int i = 0; i < 64; i++) mask[i] = (uint8_t)(i * 7);
	const int as[] = { INT_MIN, INT_MIN + 1, INT_MAX, INT_MAX - 1, -1000000, 1000000 };
	for (int a : as) { auto d = dst; ride7(d.data(), 64, img.data(), 64, mask.data(), 64, 16, 1, a, 5, t); }
	// ukuran nol
	ride7(dst.data(), 0, img.data(), 0, mask.data(), 0, 0, 0, 100, 5, t);
	ride8(dst.data(), 0, img.data(), 0, mask.data(), 0, 0, 5, 100);
	border_inner(dst.data(), 0, mask.data(), 0, 5, 0, 100);
}

int main()
{
	test_table(); test_range(); test_ride7_hand(); test_ride8_hand(); test_border_hand(); test_random(); test_extremes();
	if (g_fail) { printf("%d GAGAL\n", g_fail); return 1; }
	printf("OK semua\n");
	return 0;
}
