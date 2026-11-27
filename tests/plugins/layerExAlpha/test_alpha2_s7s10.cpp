// g++ -std=c++14 -fsanitize=address,undefined -I src/plugins tests/plugins/layerExAlpha/test_alpha2_s7s10.cpp && ./a.out
// Uji inti layerExAlpha Alpha-2 (S7). Ekspektasi dihitung tangan dari docs/spec/layerExAlpha_*.md, bukan dari kode.
// Format piksel: B,G,R,A.
#include <stdio.h>
#include <string.h>
#include <vector>
#include "layerExAlphaCore.hpp"
using namespace layerex_alpha;
static int fails = 0;
#define CHECK(c) do { if (!(c)) { printf("FAIL %s:%d %s\n", __FILE__, __LINE__, #c); ++fails; } } while (0)
typedef std::vector<int> VI;

static bool eq(const std::vector<int> &a, std::initializer_list<int> b) { return a == VI(b); }

int main()
{
	// ---- konstanta .rodata ----
	CHECK(kLumaC0 == 0.298911988735199f && kLumaC1 == 0.5866109728813171f && kLumaC2 == 0.11447799950838089f);
	// ---- cvtzs: potong ke nol, jenuh, NaN -> 0 ----
	CHECK(cvtzs(2.9f) == 2 && cvtzs(-2.9f) == -2 && cvtzs(3e10f) == INT_MAX && cvtzs(-3e10) == INT_MIN && cvtzs(NAN) == 0);

	// ---- LightForAlpha: (0,0,0,src.G) ----
	{
		uint8_t src[8] = { 10, 20, 30, 40,  1, 2, 3, 4 }, dst[8] = { 9, 9, 9, 9,  9, 9, 9, 9 };
		light_for_alpha(dst, 8, src, 8, 2, 1);
		const uint8_t e[8] = { 0, 0, 0, 20,  0, 0, 0, 2 };
		CHECK(memcmp(dst, e, 8) == 0);
	}
	// pitch berbeda & beberapa baris; byte padding tidak disentuh
	{
		uint8_t src[2 * 6], dst[2 * 6]; memset(src, 0, sizeof src); memset(dst, 0xEE, sizeof dst);
		src[1] = 77; src[6 + 1] = 88;
		light_for_alpha(dst, 6, src, 6, 1, 2);
		CHECK(dst[3] == 77 && dst[6 + 3] == 88 && dst[4] == 0xEE && dst[5] == 0xEE && dst[0] == 0);
	}

	// ---- LuminanceForAlpha: A = trunc(B*0.2989 + G*0.5866 + R*0.1145); bobot R dikenakan pada byte 0 (B) ----
	{
		uint8_t s[4], d[4];
		s[0] = 255; s[1] = 255; s[2] = 255; s[3] = 7; luminance_for_alpha(d, 4, s, 4, 1, 1);
		CHECK(d[0] == 0 && d[1] == 0 && d[2] == 0 && d[3] == 255);           // putih -> 255.00026 -> 255
		s[0] = 100; s[1] = 0; s[2] = 0; luminance_for_alpha(d, 4, s, 4, 1, 1); CHECK(d[3] == 29);   // 29.89
		s[0] = 0; s[1] = 100; s[2] = 0; luminance_for_alpha(d, 4, s, 4, 1, 1); CHECK(d[3] == 58);   // 58.66
		s[0] = 0; s[1] = 0; s[2] = 100; luminance_for_alpha(d, 4, s, 4, 1, 1); CHECK(d[3] == 11);   // 11.44
		s[0] = 0; s[1] = 0; s[2] = 0; luminance_for_alpha(d, 4, s, 4, 1, 1); CHECK(d[3] == 0);
	}

	// ---- LevelMaskRect: tabel simetris ----
	{
		VI t;
		CHECK(level_sym_table(0, t) && eq(t, { 255 }));
		CHECK(level_sym_table(1, t) && eq(t, { 128, 255, 255 }));            // 127.5+.5=128 ; 255.5->255 ; plateau 255
		CHECK(level_sym_table(2, t) && eq(t, { 85, 170, 255, 255, 85 }));
		CHECK(!level_sym_table(-1, t) && t.empty());
	}
	// LevelMaskRect(a=100,b=2): alfa 98..102 -> tabel[alfa-98]; di luar -> 0; warna tidak berubah
	{
		VI t; level_sym_table(2, t);
		uint8_t px[8][4]; const int al[8] = { 97, 98, 99, 100, 101, 102, 103, 0 };
		for (int i = 0; i < 8; ++i) { px[i][0] = 1; px[i][1] = 2; px[i][2] = 3; px[i][3] = (uint8_t)al[i]; }
		level_mask_apply(&px[0][0], 32, 8, 1, 100, 2, 2, t);
		const int e[8] = { 0, 85, 170, 255, 255, 85, 0, 0 };
		for (int i = 0; i < 8; ++i) CHECK(px[i][3] == e[i] && px[i][0] == 1 && px[i][1] == 2 && px[i][2] == 3);
	}
	// batas: a=1,b=3 -> lo = 0 (a-b<0), indeks = alfa-(a-b) = alfa+2 ; a=255,b=3 -> hi = 255
	{
		VI t; level_sym_table(3, t);                                         // n=7, s0=63.75
		// tabel: i0=64 (63.75+.5=64.25) i1=128 (127.5+.5) i2=191 (191.25+.5=191.75) i3=255 i4=255 i5: n-i=2 ->128 i6: 1 -> 64
		CHECK(eq(t, { 64, 128, 191, 255, 255, 128, 64 }));
		uint8_t px[2][4] = { { 0, 0, 0, 0 }, { 0, 0, 0, 255 } };
		level_mask_apply(&px[0][0], 8, 2, 1, 1, 3, 3, t);                    // a=1: alfa 0 -> idx 2 = 191 ; alfa 255 > hi=4 -> 0
		CHECK(px[0][3] == 191 && px[1][3] == 0);
		uint8_t q[1][4] = { { 0, 0, 0, 255 } };
		level_mask_apply(&q[0][0], 4, 1, 1, 255, 3, 3, t);                   // off=252, hi=min(258,255)=255 -> idx 3 = 255
		CHECK(q[0][3] == 255);
	}

	// ---- LevelMaskRect2: tabel asimetris ----
	{
		VI t;
		CHECK(level_asym_table(2, 1, t) && eq(t, { 85, 170, 255, 128 }));
		CHECK(level_asym_table(0, 0, t) && eq(t, { 255 }));
		CHECK(level_asym_table(0, 2, t) && eq(t, { 255, 170, 85 }));   // i0==l ->255 ; i1: n-i=2 -> 255/3*2=170 ; i2: 85
		CHECK(level_asym_table(-1, 0, t) && t.empty());                      // n == 0 -> kosong
		CHECK(!level_asym_table(-3, 1, t));                                  // n < 0 -> ditolak
	}
	{   // a=50, l=2, r=1 : off=48, hi=51
		VI t; level_asym_table(2, 1, t);
		uint8_t px[6][4]; const int al[6] = { 47, 48, 49, 50, 51, 52 };
		for (int i = 0; i < 6; ++i) { px[i][0] = px[i][1] = px[i][2] = 9; px[i][3] = (uint8_t)al[i]; }
		level_mask_apply(&px[0][0], 24, 6, 1, 50, 2, 1, t);
		const int e[6] = { 0, 85, 170, 255, 128, 0 };
		for (int i = 0; i < 6; ++i) CHECK(px[i][3] == e[i]);
	}

	// ---- MaskImageRideRect2 (satu sisi) : tabel w=2, warna (200,100,50) ----
	{
		RideTables T; CHECK(ride1_tables(2, 200, 100, 50, T));
		// s5=3 ; s2=66.666668 s3=33.333332 s4=16.666666 ; 1/3 = 0.33333334
		CHECK(T.t0.size() == 3);
		CHECK(fabsf(T.t0[0] - 2.0f / 3.0f) < 1e-6f && fabsf(T.t0[1] - 1.0f / 3.0f) < 1e-6f && T.t0[2] == 0.0f);
		CHECK(T.t1 == VI({ 67, 133, 200 }));                                 // 67.17 ; 133.83 ; (center)
		CHECK(T.t2 == VI({ 33, 67, 100 }));                                  // 33.83 ; 67.17
		CHECK(T.t3 == VI({ 17, 33, 50 }));                                   // 17.17 ; 33.83
		// penjepit: warna > 255 -> 255, negatif -> 0
		CHECK(ride1_tables(1, 999, -5, 300, T) && T.t1[1] == 255 && T.t2[1] == 0 && T.t3[1] == 255);
		CHECK(ride1_tables(-1, 1, 2, 3, T) && T.t0.empty());                 // w=-1 : tanpa tabel
		CHECK(!ride1_tables(-2, 1, 2, 3, T));
	}
	{   // terapkan: a=100, w=2 -> lo=98
		RideTables T; ride1_tables(2, 200, 100, 50, T);
		uint8_t src[7][4], dst[7][4];
		const int m[7]  = { 97, 98, 99, 100, 101, 99, 99 };
		const int da[7] = { 255, 255, 255, 255, 255, 0, 255 };
		for (int i = 0; i < 7; ++i) { src[i][0] = src[i][1] = src[i][2] = 0; src[i][3] = (uint8_t)m[i];
		                              dst[i][0] = 10; dst[i][1] = 20; dst[i][2] = 30; dst[i][3] = (uint8_t)da[i]; }
		ride1_apply(&dst[0][0], 28, &src[0][0], 28, 7, 1, 100, 2, 200, 100, 50, T);
		// m=97 < lo -> tetap
		CHECK(dst[0][0] == 10 && dst[0][1] == 20 && dst[0][2] == 30);
		// m=98 idx0: t0=0.6666667 -> B=17+trunc(6.667+.5=7.17)=24 ; G=33+trunc(13.33+.5)=46 ; R=67+trunc(20+.5)=87
		CHECK(dst[1][0] == 24 && dst[1][1] == 46 && dst[1][2] == 87);
		// m=99 idx1: t0=0.33333334 -> B=33+trunc(3.33+.5)=36 ; G=67+trunc(6.67+.5)=74 ; R=133+trunc(10+.5)=143
		CHECK(dst[2][0] == 36 && dst[2][1] == 74 && dst[2][2] == 143);
		// m=100 (== a) idx2: t0=0 -> warna penuh (50,100,200)
		CHECK(dst[3][0] == 50 && dst[3][1] == 100 && dst[3][2] == 200);
		// m=101 > a -> isi literal c3,c2,c1 ke B,G,R
		CHECK(dst[4][0] == 50 && dst[4][1] == 100 && dst[4][2] == 200);
		// alfa tujuan 0 -> tidak disentuh ; alfa tidak pernah diubah
		CHECK(dst[5][0] == 10 && dst[5][1] == 20 && dst[5][2] == 30 && dst[5][3] == 0 && dst[3][3] == 255);
	}
	{   // isi literal memakai low-byte parameter mentah (tidak dijepit): c=300 -> 44 ; c=-1 -> 255
		RideTables T; ride1_tables(0, 300, -1, 513, T);
		uint8_t src[1][4] = { { 0, 0, 0, 200 } }, dst[1][4] = { { 1, 2, 3, 255 } };
		ride1_apply(&dst[0][0], 4, &src[0][0], 4, 1, 1, 100, 0, 300, -1, 513, T);   // m=200 > a
		CHECK(dst[0][0] == (513 & 255) && dst[0][1] == 255 && dst[0][2] == (300 & 255));
	}
	{   // wrap: warna 128 + 128 = 256 -> byte 0 (tidak dijepit)
		RideTables T; ride1_tables(1, 255, 255, 255, T);                     // n=2: t0[0]=0.5 ; t1[0]=trunc(127.5+.5)=128
		CHECK(T.t1[0] == 128);
		uint8_t src[1][4] = { { 0, 0, 0, 99 } }, dst[1][4] = { { 255, 255, 255, 255 } };
		ride1_apply(&dst[0][0], 4, &src[0][0], 4, 1, 1, 100, 1, 255, 255, 255, T);  // m=99, off=99 -> idx0
		CHECK(dst[0][0] == 0 && dst[0][1] == 0 && dst[0][2] == 0);
	}

	// ---- MaskImageRideRect (dua sisi): l=1,r=1,warna (255,0,0) ----
	{
		RideTables T; CHECK(ride2_tables(1, 1, 255, 0, 0, T));
		CHECK(T.t0.size() == 3 && T.t0[0] == 0.5f && T.t0[1] == 0.0f && T.t0[2] == 0.5f);
		CHECK(T.t1 == VI({ 128, 255, 128 }) && T.t2 == VI({ 0, 0, 0 }) && T.t3 == VI({ 0, 0, 0 }));
		uint8_t src[4][4], dst[4][4];
		const int m[4] = { 98, 99, 100, 101 };
		for (int i = 0; i < 4; ++i) { src[i][0] = src[i][1] = src[i][2] = 0; src[i][3] = (uint8_t)m[i];
		                              dst[i][0] = 10; dst[i][1] = 20; dst[i][2] = 30; dst[i][3] = 255; }
		ride2_apply(&dst[0][0], 16, &src[0][0], 16, 4, 1, 100, 1, 1, T);     // lo=99, hi=101
		CHECK(dst[0][0] == 10 && dst[0][1] == 20 && dst[0][2] == 30);        // m=98 < lo
		CHECK(dst[1][0] == 5 && dst[1][1] == 10 && dst[1][2] == 143);        // idx0: 0+trunc(5.5) ; 0+trunc(10.5) ; 128+trunc(15.5)
		CHECK(dst[2][0] == 0 && dst[2][1] == 0 && dst[2][2] == 255);         // m==a idx1: warna penuh
		CHECK(dst[3][0] == 10 && dst[3][1] == 20 && dst[3][2] == 30);        // m==hi (>a) dilewati
	}
	{   // tabel dua sisi asimetris l=2,r=1 warna (90,60,30): hitung dengan campuran double/float
		RideTables T; CHECK(ride2_tables(2, 1, 90, 60, 30, T));
		CHECK(T.t0.size() == 4);
		// i0<l: t0=(1/3)*(2-0)=0.6666667 ; t1=trunc(30*1+.5)=30 ; t2=trunc(20+.5)=20 ; t3=trunc(10+.5)=10
		CHECK(T.t1[0] == 30 && T.t2[0] == 20 && T.t3[0] == 10 && fabsf(T.t0[0] - 2.0f / 3.0f) < 1e-6f);
		// i1<l: t0=1/3 ; t1=60+.5->60 ; t2=40 ; t3=20
		CHECK(T.t1[1] == 60 && T.t2[1] == 40 && T.t3[1] == 20 && fabsf(T.t0[1] - 1.0f / 3.0f) < 1e-6f);
		// i2==l: pusat
		CHECK(T.t0[2] == 0.0f && T.t1[2] == 90 && T.t2[2] == 60 && T.t3[2] == 30);
		// i3>l: n-i=1, r+1=2 -> 45+.5 -> 45 ; 30+.5 -> 30 ; 15+.5 -> 15 ; t0=(1/2)*(3-2)=0.5
		CHECK(T.t1[3] == 45 && T.t2[3] == 30 && T.t3[3] == 15 && T.t0[3] == 0.5f);
		CHECK(!ride2_tables(-5, 1, 1, 1, 1, T));
		CHECK(ride2_tables(-1, 0, 1, 1, 1, T) && T.t0.empty());
	}

	// ---- MaskImageRideRect3 (satu sisi terbalik): w=2, warna (200,100,50) ----
	{
		RideTables T; CHECK(ride3_tables(2, 200, 100, 50, T));
		CHECK(T.t0.size() == 3 && T.t0[0] == 0.0f && fabsf(T.t0[1] - 1.0f / 3.0f) < 1e-6f && fabsf(T.t0[2] - 2.0f / 3.0f) < 1e-6f);
		CHECK(T.t1 == VI({ 200, 133, 67 }) && T.t2 == VI({ 100, 67, 33 }) && T.t3 == VI({ 50, 33, 17 }));
		CHECK(ride3_tables(-1, 1, 2, 3, T) && T.t0.empty());
		CHECK(!ride3_tables(-2, 1, 2, 3, T));
		CHECK(ride3_tables(2, 200, 100, 50, T));                              // T sebelumnya dikosongkan oleh w=-1
		uint8_t src[7][4], dst[7][4];
		const int m[7]  = { 99, 100, 101, 102, 103, 101, 0 };
		const int da[7] = { 255, 255, 255, 255, 255, 0, 255 };
		for (int i = 0; i < 7; ++i) { src[i][0] = src[i][1] = src[i][2] = 0; src[i][3] = (uint8_t)m[i];
		                              dst[i][0] = 10; dst[i][1] = 20; dst[i][2] = 30; dst[i][3] = (uint8_t)da[i]; }
		ride3_apply(&dst[0][0], 28, &src[0][0], 28, 7, 1, 100, 2, 200, 100, 50, T);   // a=100, hi=102
		CHECK(dst[0][0] == 50 && dst[0][1] == 100 && dst[0][2] == 200);       // m<a : warna penuh
		CHECK(dst[1][0] == 50 && dst[1][1] == 100 && dst[1][2] == 200);       // m==a idx0: t0=0 -> warna penuh
		CHECK(dst[2][0] == 36 && dst[2][1] == 74 && dst[2][2] == 143);        // idx1
		CHECK(dst[3][0] == 24 && dst[3][1] == 46 && dst[3][2] == 87);         // m==hi diproses, idx2
		CHECK(dst[4][0] == 10 && dst[4][1] == 20 && dst[4][2] == 30);         // m>hi dilewati
		CHECK(dst[5][0] == 10 && dst[5][1] == 20 && dst[5][2] == 30);         // alfa tujuan 0
		CHECK(dst[6][0] == 50 && dst[6][2] == 200 && dst[0][3] == 255);       // m=0<a : warna penuh ; alfa tetap
	}

	// ---- cvtzu / sdiv32 ----
	CHECK(cvtzu(2.9f) == 2u && cvtzu(-5.0f) == 0u && cvtzu(1e20f) == 0xFFFFFFFFu && cvtzu(NAN) == 0u);
	CHECK(sdiv32(7, 2) == 3 && sdiv32(-7, 2) == -3 && sdiv32(5, 0) == 0 && sdiv32(INT_MIN, -1) == INT_MIN);

	// ---- MaskImageEffectRect: tabel l=2, r=1, warna (200,100,50) ----
	{
		EffectTables T; CHECK(effect_tables(2, 1, 200, 100, 50, T));
		CHECK(T.t0.size() == 4);
		CHECK(fabsf(T.t0[0] - 2.0f / 3.0f) < 1e-6f && fabsf(T.t0[1] - 1.0f / 3.0f) < 1e-6f && T.t0[2] == 0.0f && T.t0[3] == 0.5f);
		CHECK(T.ta == std::vector<uint32_t>({ 85, 170, 255, 255 }));         // 85, 170.5->170, tengah, sisi kanan 255
		CHECK(T.tr == std::vector<uint32_t>({ 66, 132, 200, 100 }));         // R: 200/3 = 66 (bagi INTEGER) *(i+1)
		CHECK(T.tg == std::vector<uint32_t>({ 33, 67, 100, 50 }));
		CHECK(T.tb == std::vector<uint32_t>({ 17, 33, 50, 25 }));
		// terapkan: a=50, l=2, r=1 -> off=48, lo=48, hi=51
		uint8_t src[7][4], dst[7][4];
		const int m[7]  = { 47, 48, 49, 50, 51, 52, 49 };
		const int da[7] = { 200, 200, 200, 200, 200, 200, 0 };
		for (int i = 0; i < 7; ++i) { src[i][0] = src[i][1] = src[i][2] = 0; src[i][3] = (uint8_t)m[i];
		                              dst[i][0] = 10; dst[i][1] = 20; dst[i][2] = 30; dst[i][3] = (uint8_t)da[i]; }
		effect_apply(&dst[0][0], 28, &src[0][0], 28, 7, 1, 50, 2, 1, T);
		CHECK(dst[0][3] == 0 && dst[0][0] == 10 && dst[0][1] == 20 && dst[0][2] == 30);                 // m<lo: alfa 0
		CHECK(dst[1][0] == 24 && dst[1][1] == 46 && dst[1][2] == 86 && dst[1][3] == 85);                // idx0
		CHECK(dst[2][0] == 36 && dst[2][1] == 74 && dst[2][2] == 142 && dst[2][3] == 170);              // idx1
		CHECK(dst[3][0] == 50 && dst[3][1] == 100 && dst[3][2] == 200 && dst[3][3] == 255);             // m==a: tengah
		CHECK(dst[4][0] == 10 && dst[4][3] == 200);                                                     // m==hi dilewati
		CHECK(dst[5][0] == 10 && dst[5][3] == 200);                                                     // m>hi
		CHECK(dst[6][0] == 10 && dst[6][3] == 0);                                                       // alfa tujuan 0
	}
	{   // sisi kanan: a=50 l=2 r=2 -> hi=52, m=51 (a<m<hi) : RGB dicampur, alfa TETAP
		EffectTables T; effect_tables(2, 2, 200, 100, 50, T);                 // n=5 ; i=3: n-i=2 ; r+1=3 -> tr=trunc(66.67*2+.5)=133
		CHECK(T.tr[3] == 133 && T.tg[3] == 67 && T.tb[3] == 33 && T.ta[3] == 255 && fabsf(T.t0[3] - 1.0f / 3.0f) < 1e-6f);
		uint8_t src[1][4] = { { 0, 0, 0, 51 } }, dst[1][4] = { { 10, 20, 30, 123 } };
		effect_apply(&dst[0][0], 4, &src[0][0], 4, 1, 1, 50, 2, 2, T);
		CHECK(dst[0][0] == 36 && dst[0][1] == 74 && dst[0][2] == 143 && dst[0][3] == 123);
	}
	{   // jenuh unsigned: parameter negatif -> dijepit 255 ; l=0
		EffectTables T; CHECK(effect_tables(0, 0, -1, 300, 0, T));            // n=1, i==l: tr=0xFFFFFFFF->255 ; tg=300->255
		CHECK(T.tr[0] == 255 && T.tg[0] == 255 && T.tb[0] == 0 && T.ta[0] == 255 && T.t0[0] == 0.0f);
		CHECK(effect_tables(-1, 0, 1, 2, 3, T) && T.t0.empty());
		CHECK(!effect_tables(-3, 1, 1, 2, 3, T));
	}

	// ---- AlphaBorderImageCoverRect: jendela ----
	{
		int L, R;
		CHECK(cover_window(100, 3, L, R) && L == 3 && R == 3);
		CHECK(cover_window(-1, 3, L, R) && L == 0 && R == 3);
		CHECK(cover_window(5, 10, L, R) && L == 5 && R == 10);
		CHECK(cover_window(0, 10, L, R) && L == 0 && R == 10);
		CHECK(cover_window(200, 100, L, R) && L == 100 && R == 155);         // q+p>=256 -> 255-p+q
		CHECK(cover_window(300, 50, L, R) && L == 95 && R == 0);             // p>=256 -> p+q-255
		CHECK(cover_window(255, 300, L, R) && L == 255 && R == 300);
		CHECK(cover_window(300, 400, L, R) && L == 300 && R == 0);
		CHECK(!cover_window(100, 0, L, R) && !cover_window(-5, 3, L, R) && !cover_window(300, 3, L, R));   // q<1 ; q+p<1 ; p-q>254
		CHECK(cover_window(257, 3, L, R) && L == 5 && R == 0);               // p-q=254 masih valid ; p>=256: 257+3-255=5
		CHECK(!cover_window(258, 3, L, R));                                  // p-q=255 > 254
	}

	// ---- AlphaBorderImageCoverRect: tabel dan penerapan ----
	{
		std::vector<float> T; int L, R;
		cover_window(100, 3, L, R);                                          // L=3,R=3
		CHECK(cover_table(3, L, R, T) && T.size() == 7);
		CHECK(T[0] == 0.0f && fabsf(T[1] - 1.0f / 3.0f) < 1e-6f && fabsf(T[2] - 2.0f / 3.0f) < 1e-6f);
		CHECK(T[3] == 1.0f && T[4] == 1.0f && fabsf(T[5] - 2.0f / 3.0f) < 1e-6f && fabsf(T[6] - 1.0f / 3.0f) < 1e-6f);
		cover_window(-1, 3, L, R);                                           // L=0,R=3
		CHECK(cover_table(3, L, R, T) && T.size() == 4 && T[0] == 1.0f && T[1] == 1.0f && fabsf(T[2] - 2.0f / 3.0f) < 1e-6f && fabsf(T[3] - 1.0f / 3.0f) < 1e-6f);
		// terapkan p=100,q=3 -> jendela [97,103)
		cover_window(100, 3, L, R); cover_table(3, L, R, T);
		uint8_t A[7][4], B[7][4], C[7][4], D[7][4];
		const int mc[7] = { 96, 97, 99, 102, 103, 99, 99 };
		const int da[7] = { 255, 255, 255, 255, 255, 0, 255 };
		const int aa[7] = { 7, 7, 7, 7, 7, 0, 0 };
		for (int i = 0; i < 7; ++i) {
			A[i][0] = 1; A[i][1] = 2; A[i][2] = 3; A[i][3] = (uint8_t)aa[i];
			B[i][0] = B[i][1] = B[i][2] = 0; B[i][3] = 200;
			C[i][0] = C[i][1] = C[i][2] = 0; C[i][3] = (uint8_t)mc[i];
			D[i][0] = 9; D[i][1] = 9; D[i][2] = 9; D[i][3] = (uint8_t)da[i];
		}
		cover_apply(&D[0][0], 28, &A[0][0], 28, &B[0][0], 28, &C[0][0], 28, 7, 1, 100, L, R, T);
		CHECK(D[0][0] == 9 && D[0][3] == 255);                               // 96 < 97 : dilewati
		CHECK(D[1][0] == 1 && D[1][1] == 2 && D[1][2] == 3 && D[1][3] == 0); // idx0: T=0 -> alfa 0, RGB disalin
		CHECK(D[2][0] == 1 && D[2][3] == 133);                               // idx2: .6666667*200+.5 = 133.83
		CHECK(D[3][0] == 1 && D[3][3] == 133);                               // m=102 idx5: .6666667
		CHECK(D[4][0] == 9 && D[4][3] == 255);                               // 103 >= p+R : dilewati
		CHECK(D[5][0] == 9 && D[5][3] == 0);                                 // alfa tujuan 0 DAN alfa A 0 -> dilewati
		CHECK(D[6][0] == 1 && D[6][3] == 133);                               // alfa A 0 tetapi tujuan != 0 -> diproses
	}

	// ---- batas ukuran nol / negatif: tidak menyentuh memori ----
	{
		VI t; level_sym_table(1, t); RideTables T; ride1_tables(1, 1, 1, 1, T);
		uint8_t dummy[4] = { 1, 2, 3, 4 };
		level_mask_apply(dummy, 4, 0, 1, 1, 1, 1, t); level_mask_apply(dummy, 4, 1, 0, 1, 1, 1, t);
		light_for_alpha(dummy, 4, dummy, 4, 0, 0); luminance_for_alpha(dummy, 4, dummy, 4, -1, -1);
		ride1_apply(dummy, 4, dummy, 4, 0, 5, 1, 1, 1, 1, 1, T);
		CHECK(dummy[0] == 1 && dummy[3] == 4);
	}

	if (fails == 0) printf("OK: semua uji inti layerExAlpha (Alpha-2) lolos\n");
	return fails ? 1 : 0;
}
