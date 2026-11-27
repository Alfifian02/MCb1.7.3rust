// g++ -std=c++14 -fsanitize=address,undefined -I src/plugins tests/plugins/layerExImage/test_core.cpp && ./a.out
// Uji inti layerExImage. Dua sumber ekspektasi:
//  (1) golden.inc : hash keluaran FUNGSI ASLI libgame.so 1.3.9 (dijalankan di emulator; gen_golden.py) -- tingkat "binary";
//  (2) pemeriksaan analitik dari docs/spec/layerExImage_*.md (konstanta, kasus tepi, deviasi sadar).
#include <stdio.h>
#include <string.h>
#include <math.h>
#include <limits.h>
#include <vector>
#include "layerExImageCore.hpp"
using namespace layerex_image;
static int fails = 0;
#define CHECK(c) do { if (!(c)) { printf("FAIL %s:%d %s\n", __FILE__, __LINE__, #c); ++fails; } } while (0)

struct Xs { uint32_t x; explicit Xs(uint32_t s) : x(s ? s : 1) {}
	uint32_t next() { x ^= x << 13; x ^= x >> 17; x ^= x << 5; return x; } };
static std::vector<uint8_t> make_img(Xs &xs, int w, int h) { std::vector<uint8_t> v((size_t)w * h * 4); for (auto &b : v) b = (uint8_t)(xs.next() >> 24); return v; }
static uint64_t fnv(const std::vector<uint8_t> &b) { uint64_t h = 0xcbf29ce484222325ull; for (uint8_t c : b) { h ^= c; h *= 0x100000001b3ull; } return h; }

struct Golden { int kind, w, h; uint32_t seed; double a; int b, c; uint64_t hash; };
static const Golden kGolden[] = {
#include "golden.inc"
};

int main()
{
	// ---------------- (1) golden dari binary ----------------
	int ng = 0;
	for (const Golden &g : kGolden) {
		Xs xs(g.seed); std::vector<uint8_t> img = make_img(xs, g.w, g.h); const ptrdiff_t pitch = g.w * 4;
		switch (g.kind) {
		case 0: { uint8_t lut[256]; if (!light_is_noop(g.b, g.c)) { light_lut(lut, g.b, g.c); light_apply(img.data(), pitch, g.w, g.h, lut); } } break;
		case 1: if (!colorize_is_noop(g.a)) colorize_apply(img.data(), pitch, g.w, g.h, g.a, g.b, g.c); break;
		case 2: modulate_apply(img.data(), pitch, g.w, g.h, (int)g.a, g.b, g.c); break;
		case 3: { if (g.b == 0) break;
			NoiseSeed s; uint32_t wd[4]; for (auto &x : wd) x = xs.next(); s.c = wd[0]; s.a = wd[1]; s.b = wd[2];
			noise_apply(img.data(), pitch, g.w, g.h, g.b, s, [&]() { return (int)(xs.next() & 0x7fffffff); }); } break;
		case 4: white_noise_apply(img.data(), pitch, g.w, g.h, [&]() { return (int)(xs.next() & 0x7fffffff); }); break;
		case 5: gaussian_apply(img.data(), pitch, g.w, g.h, (float)g.a); break;
		}
		const bool ok = fnv(img) == g.hash;
		if (!ok) printf("FAIL golden jenis=%d %dx%d a=%g b=%d c=%d\n", g.kind, g.w, g.h, g.a, g.b, g.c);
		CHECK(ok); ++ng;
	}
	printf("golden dari binary: %d kasus\n", ng);

	// ---------------- (2) analitik ----------------
	{ // light: (0,0) identitas; LUT monoton untuk kontras >= 0; saturasi
		uint8_t l[256]; light_lut(l, 0, 0); for (int i = 0; i < 256; ++i) CHECK(l[i] == i);
		CHECK(light_is_noop(0, 0) && !light_is_noop(1, 0) && !light_is_noop(0, -1));
		light_lut(l, 127, 0); CHECK(l[0] == 127 && l[128] == 255 && l[255] == 255);
		light_lut(l, 0, -100); for (int i = 0; i < 256; ++i) CHECK(l[i] == 128);      // k = 0 -> abu-abu rata
		light_lut(l, -128, 0); CHECK(l[0] == 0 && l[128] == 0 && l[255] == 127);
		uint8_t px[8] = { 10, 20, 30, 77, 40, 50, 60, 99 }; light_lut(l, 5, 0); light_apply(px, 8, 2, 1, l);
		CHECK(px[3] == 77 && px[7] == 99 && px[0] == 15 && px[1] == 25);                  // alfa tidak disentuh
	}
	{ // helper HLS: abu-abu, merah murni, round-trip kasar
		CHECK(rgb_to_hls(0x808080) == (uint32_t)((0xaa << 16) | (0 << 8) | 128));
		const uint32_t red = rgb_to_hls(0xff0000);                                        // byte tinggi = 'R'
		CHECK((red & 0xff) == 128 && ((red >> 8) & 0xff) == 255);
		CHECK(hls_to_rgb((0u << 16) | (255u << 8) | 128u) == 0xfe0101u);                  // H=0,S=255,L=128 -> merah (q=0.99999994 -> 254; p=1/255 -> 1)
		CHECK(hls_to_rgb(0x00007f) == 0x7f7f7fu);                                         // S=0 -> abu-abu = trunc(L)
	}
	{ // colorize: blend <= 0 no-op; alfa utuh; blend penuh menghasilkan warna seragam untuk S/H tetap
		CHECK(colorize_is_noop(0.0) && colorize_is_noop(-3.0) && !colorize_is_noop(0.1));
		uint8_t px[8] = { 200, 10, 90, 33, 5, 250, 60, 44 };
		colorize_apply(px, 8, 2, 1, 1.0, 0, 0);                                           // S = 0 -> abu-abu
		CHECK(px[3] == 33 && px[7] == 44 && px[0] == px[1] && px[1] == px[2] && px[4] == px[5] && px[5] == px[6]);
		uint8_t q[4] = { 1, 2, 3, 4 }; colorize_apply(q, 4, 1, 1, 0.0039, 0, 0);          // blend kecil: (int)(0.0039*256) = 0 -> tidak berubah
		CHECK(q[0] == 1 && q[1] == 2 && q[2] == 3 && q[3] == 4);
	}
	{ // modulate: S=0 dengan sat>0 memberi warna; luminance 100 -> putih; alfa utuh
		uint8_t px[4] = { 100, 100, 100, 7 }; modulate_apply(px, 4, 1, 1, 0, 0, 100);
		CHECK(px[0] == 255 && px[1] == 255 && px[2] == 255 && px[3] == 7);
		uint8_t g[4] = { 128, 128, 128, 9 }; modulate_apply(g, 4, 1, 1, 0, 100, 0);       // abu-abu jadi tersaturasi penuh
		CHECK(!(g[0] == g[1] && g[1] == g[2]) && g[3] == 9);
		CHECK(mod_wrap_hue(-0.25) == 0.75 && mod_wrap_hue(2.5) == 0.5 && mod_wrap_hue(1.0) == 1.0 && mod_wrap_hue(0.0) == 0.0);
	}
	{ // noise: level 0 ditangani pemanggil; kanal alfa utuh; clamp
		std::vector<uint8_t> v(4 * 4 * 2, 128); for (size_t i = 3; i < v.size(); i += 4) v[i] = 0x5a;
		NoiseSeed s = { 1, 2, 3 }; noise_apply(v.data(), 16, 4, 2, 255, s, []() { return 0; });
		for (size_t i = 3; i < v.size(); i += 4) CHECK(v[i] == 0x5a);
		CHECK(noise_add(250, 127, 255) == 255 && noise_add(3, -128, 255) == 0 && noise_add(100, 0, 255) == 100);
		CHECK(noise_add(100, -1, 1) == 99);                                                 // (-1*1)>>8 = -1 (geser aritmetik)
	}
	{ // white noise: rand 0 -> 0, rand maks -> 255; sama di 3 kanal
		CHECK(white_value(0) == 0 && white_value(0x7fffffff) == 255 && white_value(1) == 0);
		CHECK(white_value(1 << 30) == 127 && white_value(1 << 23) == 0);                  // r*0xff0001/2^47
	}
	{ // gaussian: panjang kernel, normalisasi, bobot tengah (kuirk), batas deviasi
		CHECK(gauss_taps(0.0f) == 1 && gauss_taps(1.0f) == 3 && gauss_taps(2.0f) == 5 && gauss_taps(7.3f) == 17);
		CHECK(gauss_taps(1e9f) == 0 && gauss_taps(INFINITY) == 0 && gauss_taps(NAN) == 1);
		GaussKernel g; CHECK(gauss_build(g, 2.0f));
		float s = 0; for (float x : g.k) s += x; CHECK(fabsf(s - 1.0f) < 1e-5f);
		for (int i = 0; i < g.n; ++i) CHECK(g.k[i] == g.k[g.n - 1 - i]);                  // simetris
		CHECK(fabsf(g.k[g.n / 2] - g.k[g.n / 2 + 1]) < 1e-2f * g.k[g.n / 2]);             // kuirk: puncak datar (tengah ~ tetangga; terukur +0,57 %)
		CHECK(!gauss_build(g, 1e9f));
		// gambar rata tetap rata (semua rezim: n<=len dan n>len tidak dijamin rata -> uji hanya n<=len)
		std::vector<uint8_t> f(16 * 16 * 4, 100); gaussian_apply(f.data(), 64, 16, 16, 2.0f);
		bool flat = true; for (uint8_t b : f) if (b != 100) flat = false; CHECK(flat);
		// n=1 (sigma 0): identitas
		Xs xs(5); std::vector<uint8_t> a = make_img(xs, 6, 6), b = a; gaussian_apply(b.data(), 24, 6, 6, 0.0f); CHECK(a == b);
		// alfa ikut diburamkan
		std::vector<uint8_t> t(8 * 1 * 4, 0); t[3 + 4 * 3] = 255; gaussian_apply(t.data(), 32, 8, 1, 1.0f);
		CHECK(t[3 + 4 * 2] > 0 && t[3 + 4 * 4] > 0 && t[3 + 4 * 3] < 255);
		int ng_, pg_; gauss_box_radius(2.0f, &ng_, &pg_); CHECK(pg_ == 2 && ng_ == -2);
		gauss_box_radius(0.005f, &ng_, &pg_); CHECK(pg_ == 0 && ng_ == 0);
		gauss_box_radius(2.01f, &ng_, &pg_); CHECK(pg_ == 3 && ng_ == -3);
	}
	printf(fails ? "GAGAL: %d\n" : "OK\n", fails);
	return fails ? 1 : 0;
}
