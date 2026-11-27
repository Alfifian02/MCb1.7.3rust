// g++ -std=c++14 -fsanitize=address,undefined -I src/plugins tests/plugins/layerExRaster/test_core.cpp && ./a.out
// Uji inti Layer.copyRaster. DITULIS ULANG (8.1): berkas aslinya tertimpa uji Shrink. Ekspektasi diturunkan dari
// docs/spec/layerExRaster_copyRaster.md, bukan dari kode.
#include <stdio.h>
#include <math.h>
#include <string.h>
#include <limits.h>
#include <vector>
#include <string>
#include "layerExRasterCore.hpp"
using namespace layerex_raster;
static int fails = 0;
#define CHECK(c) do { if (!(c)) { printf("FAIL %s:%d %s\n", __FILE__, __LINE__, #c); ++fails; } } while (0)
static const double PI = 3.14159265358979323846;

int main()
{
	// ---- konstanta dari .rodata ----
	CHECK(kTwoPi == 6.283185307179586);
	CHECK(kPhaseScale == -1.0471975511965976);

	// ---- time % (3*cycle): memotong ala C; cycle = 0 -> tmod = time ----
	CHECK(time_mod(10, 2) == 4);                       // 10 - trunc(10/6)*6
	CHECK(time_mod(5, 2) == 5);
	CHECK(time_mod(-10, 2) == -4);                     // memotong ke nol, bukan modulo matematis
	CHECK(time_mod(7, 0) == 7);                        // sdiv nol di AArch64 = 0 -> time - 0
	{   // 3*cycle dihitung 32-bit: 0x60000000*3 = 0x120000000 -> 0x20000000 (melingkar)
		const int64_t period = 0x20000000;
		CHECK(time_mod(period + 5, 0x60000000) == 5);
	}

	// ---- fase awal: step = 2pi/lines ; phase0 = step*clipTop + (step*tmod/cycle)*(H/2)*(-pi/3) ----
	{   Wave w = make_wave(0, 10, 360, 5, 0);
		CHECK(fabs(w.step - 2 * PI / 360) < 1e-15 && w.phase0 == 0.0);
	}
	{   Wave w = make_wave(2, 10, 8, 3, 0);            // time 0 -> bagian geser = 0 ; phase0 = step*2
		CHECK(fabs(w.step - PI / 4) < 1e-15 && fabs(w.phase0 - PI / 2) < 1e-14);
	}
	{   // lines=8, cycle=4, time=2, H=10: tmod=2 ; shift=(pi/4*2/4)*5*(-pi/3) = -5*pi^2/24 ; clipTop=0
		Wave w = make_wave(0, 10, 8, 4, 2);
		CHECK(fabs(w.phase0 - (-5 * PI * PI / 24)) < 1e-12);
	}
	{   // cycle = 0 -> pembagian real by-nol -> inf/NaN -> fase NaN/inf -> s = 0
		Wave w = make_wave(0, 10, 8, 0, 5);
		CHECK(row_shift(w.phase0, 7) == 0);
	}

	// ---- s = trunc(sin(phase)*maxh), NaN -> 0, saturasi fcvtzs ----
	CHECK(row_shift(0.0, 10) == 0);
	CHECK(row_shift(PI / 2, 10) == 10);
	CHECK(row_shift(-PI / 2, 10) == -10);
	CHECK(row_shift(0.3, 10) == 2);                    // 2.955 -> 2
	CHECK(row_shift(-0.3, 10) == -2);                  // -2.955 -> -2 (ke arah nol)
	CHECK(row_shift(NAN, 10) == 0);
	CHECK(row_shift(INFINITY, 10) == 0);               // sin(inf) = NaN
	CHECK(row_shift(PI / 2, INT_MAX) == INT_MAX);
	CHECK(row_shift(PI / 2, INT_MIN) == INT_MIN);      // -2147483648.0 jenuh tanpa UB

	// ---- copy_rows: 4 baris x 4 piksel; step=pi/2, phase0=0, maxh=2 ----
	//   baris0 sin(0)=0 -> s=0 salin biasa ; baris1 sin(pi/2)=1 -> s=+2 ; baris2 sin(pi)~1e-16 -> s=0 ; baris3 sin(3pi/2)=-1 -> s=-2
	{
		const int W = 4, H = 4, SP = W * 4 + 8, DP = W * 4 + 16;   // pitch berbeda dan ada padding
		std::vector<uint8_t> src((size_t)SP * H, 0), dst((size_t)DP * H, 0xEE);
		for (int y = 0; y < H; ++y) for (int x = 0; x < W; ++x) for (int c = 0; c < 4; ++c) src[(size_t)y * SP + x * 4 + c] = (uint8_t)(16 * y + 4 * x + c + 1);
		Wave w; w.step = PI / 2; w.phase0 = 0.0;
		copy_rows(dst.data(), DP, src.data(), SP, W, H, w, 2);
		auto S = [&](int y, int x, int c) { return src[(size_t)y * SP + x * 4 + c]; };
		auto D = [&](int y, int x, int c) { return dst[(size_t)y * DP + x * 4 + c]; };
		for (int x = 0; x < W; ++x) for (int c = 0; c < 4; ++c) { CHECK(D(0, x, c) == S(0, x, c)); CHECK(D(2, x, c) == S(2, x, c)); }
		for (int x = 0; x < 2; ++x) for (int c = 0; c < 4; ++c) {
			CHECK(D(1, x, c) == 0xEE);                 // s>=0: s piksel kiri TIDAK diubah
			CHECK(D(1, x + 2, c) == S(1, x, c));       // dst[x+s] = src[x]
			CHECK(D(3, x, c) == S(3, x + 2, c));       // s<0: dst[x] = src[x-s]
			CHECK(D(3, x + 2, c) == 0xEE);             // |s| piksel kanan TIDAK diubah
		}
		for (int y = 0; y < H; ++y) for (int p = W * 4; p < DP; ++p) CHECK(dst[(size_t)y * DP + p] == 0xEE);   // padding utuh
	}
	// ---- |s| >= lebar -> baris dilewati (fase tetap maju) ----
	{
		const int W = 4, H = 2;
		std::vector<uint8_t> src((size_t)W * H * 4, 0x11), dst((size_t)W * H * 4, 0xEE);
		Wave w; w.step = PI / 2; w.phase0 = PI / 2;    // baris0 sin=1 ; baris1 sin(pi)~0
		copy_rows(dst.data(), W * 4, src.data(), W * 4, W, H, w, 9);      // s = 9 >= 4 -> lewati baris0
		for (int i = 0; i < W * 4; ++i) CHECK(dst[i] == 0xEE);            // baris0 tidak berubah
		for (int i = 0; i < W * 4; ++i) CHECK(dst[(size_t)W * 4 + i] == 0x11);   // baris1 s=0 -> salin
	}
	// ---- lines = 0 -> step = inf -> NaN -> s = 0 -> salinan biasa (bukan error) ----
	{
		const int W = 3, H = 3;
		std::vector<uint8_t> src((size_t)W * H * 4), dst((size_t)W * H * 4, 0);
		for (size_t i = 0; i < src.size(); ++i) src[i] = (uint8_t)(i + 1);
		Wave w = make_wave(0, H, 0, 3, 0);
		copy_rows(dst.data(), W * 4, src.data(), W * 4, W, H, w, 5);
		CHECK(dst == src);
	}
	// ---- src dan dst buffer SAMA (tumpang-tindih): memmove memberi salinan dari isi asli ----
	{
		uint8_t buf[16] = { 1,1,1,1, 2,2,2,2, 3,3,3,3, 4,4,4,4 };
		Wave w; w.step = 0.0; w.phase0 = PI / 2;       // sin=1, maxh=1 -> s=1 untuk semua baris
		copy_rows(buf, 16, buf, 16, 4, 1, w, 1);
		const uint8_t want[16] = { 1,1,1,1, 1,1,1,1, 2,2,2,2, 3,3,3,3 };
		CHECK(memcmp(buf, want, 16) == 0);
	}
	// ---- shader GPU ----
	{
		const std::string src = kShaderSrc;
		CHECK(std::string(kShaderName) == "ExRaster");
		CHECK(src.find("uniform float omega;") != std::string::npos);
		CHECK(src.find("uniform float phase;") != std::string::npos);
		CHECK(src.find("uniform float h;") != std::string::npos);
		CHECK(src.find("sin(v_texCoord0.y * omega + phase) * h + v_texCoord0.x") != std::string::npos);
	}
	printf(fails ? "GAGAL: %d\n" : "OK semua\n", fails);
	return fails != 0;
}
