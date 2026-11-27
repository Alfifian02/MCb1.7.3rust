// layerExImageCore.hpp - inti piksel modul layerExImage.dll (tanpa dependensi inti Kirikiri).
// Direkonstruksi dari perilaku jalur CPU libgame.so 1.3.9 arm64; spesifikasi: docs/spec/layerExImage_*.md
//   light            0x5920b4 (1060 B)   colorize         0x5924d8 (1268 B) + helper 0x5929cc/0x592afc
//   modulate         0x592cf0 (1908 B)   noise            0x593464 (1840 B)
//   generateWhiteNoise 0x593b94 (716 B)  gaussianBlur     0x593e60 (1436 B) + filter 1-D 0x5943fc (1880 B)
// Format piksel: 4 byte per piksel (indeks byte 0..3); byte 3 (alfa) tidak pernah disentuh.
//
// PENTING (aritmetika float): hasil harus identik bit-per-bit dengan binary, jadi tidak boleh ada FMA/kontraksi.
// Tiap perkalian dan penjumlahan ditulis sebagai pernyataan terpisah dan berkas ini mematikan kontraksi.
#ifndef LAYEREX_IMAGE_CORE_HPP
#define LAYEREX_IMAGE_CORE_HPP

#if defined(__clang__)
#pragma clang fp contract(off)
#elif defined(__GNUC__)
#pragma GCC optimize ("fp-contract=off")
#endif

#include <stdint.h>
#include <stddef.h>
#include <math.h>
#include <vector>

namespace layerex_image {

// ---------------------------------------------------------------------------------------------
// fcvtzs saturasi (AArch64): NaN -> 0, di luar jangkauan -> batas int32, selain itu memotong ke nol.
static inline int32_t cvt_s32(double d)
{
	if (d != d) return 0;
	if (d >= 2147483648.0) return INT32_MAX;
	if (d <= -2147483648.0) return INT32_MIN;
	return (int32_t)d;
}
static inline int32_t cvt_s32f(float f)
{
	if (f != f) return 0;
	if (f >= 2147483648.0f) return INT32_MAX;
	if (f <= -2147483648.0f) return INT32_MIN;
	return (int32_t)f;
}
static inline uint8_t low8(int32_t v) { return (uint8_t)(uint32_t)v; }   // strb: simpan byte rendah (melingkar)

// =============================================================================================
// light(brightness, contrast)           [0x5920b4]
// =============================================================================================
// LUT 256 entri: lut[i] = clamp0_255( trunc( (i-128)*k + (brightness+128) ) ),  k = (contrast+100)/100  (float)
static inline void light_lut(uint8_t lut[256], int brightness, int contrast)
{
	const float k    = (float)(int32_t)((uint32_t)contrast + 100u) / 100.0f;
	const float bias = (float)(int32_t)((uint32_t)brightness + 128u);
	for (int i = 0; i < 256; ++i) {
		const float x = (float)(i - 128);
		float v = x * k;
		v = v + bias;
		const int32_t iv = cvt_s32f(v);
		lut[i] = (uint8_t)(iv > 0 ? (iv < 255 ? iv : 255) : 0);
	}
}

// brightness == 0 && contrast == 0 -> binary kembali tanpa melakukan apa pun (sebelum cek IsSoftware)
static inline bool light_is_noop(int brightness, int contrast) { return (brightness | contrast) == 0; }

// seluruh gambar w x h (bukan clip); kanal byte 0..2 lewat LUT
static inline void light_apply(uint8_t *base, ptrdiff_t pitch, int w, int h, const uint8_t lut[256])
{
	for (int y = 0; y < h; ++y) {
		uint8_t *p = base + (ptrdiff_t)y * pitch;
		for (int x = 0; x < w; ++x, p += 4) { p[0] = lut[p[0]]; p[1] = lut[p[1]]; p[2] = lut[p[2]]; }
	}
}

// =============================================================================================
// Helper HLS (dipakai colorize)
// Susunan "packed": byte tinggi (bit 16..23) = 'R', tengah (8..15) = 'G', rendah (0..7) = 'B' untuk rgb_to_hls;
// hasilnya: H di bit 16..23, S di bit 8..15, L di bit 0..7, bit 24..31 = 0.
// =============================================================================================
// 0x5929cc : RGB -> HLS bilangan bulat (skala 255). Mirip ColorRGBToHLS, TETAPI H negatif tidak dinormalisasi
// (hanya diambil 8 bit rendahnya) dan beberapa suku memakai penutup 16-bit; ditiru persis.
static inline uint32_t rgb_to_hls(uint32_t v)
{
	const uint32_t lo = v & 0xff, mid = (v >> 8) & 0xff, hi = (v >> 16) & 0xff;
	uint32_t cmax = hi < mid ? mid : hi; if (cmax < lo) cmax = lo;
	uint32_t cmin = mid < hi ? mid : hi; if (cmin > lo) cmin = lo;
	const uint32_t sum = cmax + cmin;
	const uint32_t L = (uint32_t)(((uint64_t)(sum * 255u + 255u) * 0x80808081ull) >> 40);
	const int32_t delta = (int32_t)(cmax - cmin);
	uint32_t S, H;
	if (delta == 0) {
		S = 0; H = 0xaa;
	} else {
		const int32_t dl = delta * 255;
		int32_t den, half;
		if (L & 0x80) { den = 510 - (int32_t)sum; half = (den + (den < 0 ? 1 : 0)) >> 1; }
		else          { den = (int32_t)sum;       half = (int32_t)(sum >> 1); }
		S = (uint32_t)((dl + half) / den);
		const int32_t h2 = delta >> 1;
		const int32_t xmid = ((int32_t)(cmax - mid) * 42 + h2) / delta;
		const int32_t xlow = ((int32_t)(cmax - lo)  * 42 + h2) / delta;
		if (cmax == hi) {
			H = (uint32_t)((int32_t)(uint16_t)xlow - (int32_t)(uint16_t)xmid);
		} else {
			const int32_t xhi = ((int32_t)(cmax - hi) * 42 + h2) / delta;
			if (cmax == mid) H = (uint32_t)((int32_t)(uint16_t)xhi + 0x55 - (int32_t)(uint16_t)xlow);
			else             H = (uint32_t)((0xaa - xhi) + xmid);
		}
	}
	return (L & 0xff) | ((S & 0xff) << 8) | ((H & 0xff) << 16);
}

// f(t) potongan-potongan HSL->RGB, t dalam derajat (0x592afc, satu presisi)
static inline float hls_f(float t, float p, float q)
{
	if (t < 60.0f) {
		float d = q - p; float m = t * d; m = m / 60.0f; return p + m;
	}
	if (t < 180.0f) return q;
	if (t < 240.0f) {
		float d = q - p; float u = 240.0f - t; float m = u * d; m = m / 60.0f; return p + m;
	}
	return p;
}

// 0x592afc : HLS -> RGB (float). Hasil: 'R' di bit 16..23, 'G' di 8..15, 'B' di 0..7.
static inline uint32_t hls_to_rgb(uint32_t v)
{
	const float l = (float)(v & 0xff) / 255.0f;
	const float s = (float)((v >> 8) & 0xff) / 255.0f;
	float q;
	if (l > 0.5f) { float a = l + s; float m = l * s; q = a - m; }
	else          { float a = s + 1.0f; q = l * a; }
	if (s == 0.0f) {
		const int32_t g = cvt_s32f(l * 255.0f);
		return ((uint32_t)g << 16) | (((uint32_t)g & 0xff) << 8) | (uint32_t)g;
	}
	float hd = (float)((v >> 16) & 0xff);
	hd = hd * 360.0f; hd = hd / 255.0f;
	float p = l + l; p = p - q;
	float t7 = hd + 120.0f; float t7b = t7 + (-360.0f);
	const float tR = (t7 > 360.0f) ? t7b : t7;
	const float fR = hls_f(tR, p, q);
	float tGb = hd + (-360.0f);
	const float tG = (hd > 360.0f) ? tGb : hd;
	const float fG = hls_f(tG, p, q);
	float tB = hd + (-120.0f);
	if (tB > 360.0f) tB = tB + (-360.0f);
	else if (!(tB >= 0.0f)) tB = tB + 360.0f;
	const float fB = hls_f(tB, p, q);
	const int32_t R = cvt_s32f(fR * 255.0f), G = cvt_s32f(fG * 255.0f), B = cvt_s32f(fB * 255.0f);
	return ((uint32_t)R << 16) | (((uint32_t)G & 0xff) << 8) | (uint32_t)B;
}

// =============================================================================================
// colorize(hue, sat, blend)             [0x5924d8]
// =============================================================================================
// blend <= 0 (bukan NaN) -> tidak melakukan apa pun. Mengembalikan false bila tidak ada kerja.
static inline bool colorize_is_noop(double blend) { return blend <= 0.0; }

// seluruh gambar w x h. CATATAN byte: 'R' helper = byte 0, 'B' helper = byte 2 (kebalikan dari modulate).
static inline void colorize_apply(uint8_t *base, ptrdiff_t pitch, int w, int h, double blend, int hue, int sat)
{
	double bd = blend;
	if (!(bd != bd)) { if (!(bd < 1.0)) bd = 1.0; }          // fmin(blend,1.0); NaN dipertahankan (AArch64)
	const int32_t bi = (bd != bd) ? 0 : cvt_s32(bd * 256.0); // fcvtzs w,d,#8
	const int32_t inv = 256 - bi;
	const bool full = (bd > 0.9990000128746033);              // d9 di 0x14c3150
	const uint32_t hs = (((uint32_t)hue & 0xff) << 16) | (((uint32_t)sat & 0xff) << 8);
	for (int y = 0; y < h; ++y) {
		uint8_t *p = base + (ptrdiff_t)y * pitch;
		for (int x = 0; x < w; ++x, p += 4) {
			const uint32_t pk = (uint32_t)p[2] | ((uint32_t)p[1] << 8) | ((uint32_t)p[0] << 16);
			const uint32_t hl = rgb_to_hls(pk);
			const uint32_t rg = hls_to_rgb(hs | (hl & 0xff));
			const uint32_t n0 = (rg >> 16) & 0xff, n1 = (rg >> 8) & 0xff, n2 = rg & 0xff;
			if (full) {
				p[0] = (uint8_t)n0; p[1] = (uint8_t)n1; p[2] = (uint8_t)n2;
			} else {
				p[0] = (uint8_t)((n0 * (uint32_t)bi + (uint32_t)inv * p[0]) >> 8);
				p[1] = (uint8_t)((n1 * (uint32_t)bi + (uint32_t)inv * p[1]) >> 8);
				p[2] = (uint8_t)((n2 * (uint32_t)bi + (uint32_t)inv * p[2]) >> 8);
			}
		}
	}
}

// =============================================================================================
// modulate(hue, sat, luminance)         [0x592cf0]
// =============================================================================================
static inline double mod_f(double t, double p, double q)
{
	if (t < 0.16666666666666666) { double d = q - p; double m = d * t; m = m * 6.0; return p + m; }
	if (t < 0.5) return q;
	if (t < 0.6666666666666666) { double d = q - p; double u = 0.6666666666666666 - t; double m = d * u; m = m * 6.0; return p + m; }
	return p;
}

// normalisasi ke (0,1] / [0,1) persis hasil pengulangan "+1"/"-1" binary (pengurangan 1.0 eksak untuk |x| < 2^23)
static inline double mod_wrap_hue(double x)
{
	if (x < 0.0) { x = x + ceil(-x); }                // binary: do { x += 1 } while (x < 0)
	if (x > 1.0) { x = x - (ceil(x) - 1.0); }          // binary: while (x > 1) x -= 1
	return x;
}

struct ModulateParams {
	double hue_mod;       // (double)(float)(hue / 360.0f)
	double sat_mod;       // (double)(float)(sat / 100.0f)
	double lum_mod;       // (double)(float)(lum / 100.0f)
	bool sat_pos, lum_pos;   // float > 0
};
static inline ModulateParams modulate_params(int hue, int sat, int lum)
{
	ModulateParams m;
	const float fh = (float)hue / 360.0f, fs = (float)sat / 100.0f, fl = (float)lum / 100.0f;
	m.hue_mod = (double)fh; m.sat_mod = (double)fs; m.lum_mod = (double)fl;
	m.sat_pos = fs > 0.0f; m.lum_pos = fl > 0.0f;
	return m;
}

// satu piksel; 'R' = byte 2, 'G' = byte 1, 'B' = byte 0
static inline void modulate_pixel(uint8_t *p, const ModulateParams &m)
{
	const double r = (double)p[2] / 255.0, g = (double)p[1] / 255.0, b = (double)p[0] / 255.0;
	double t = (r < g) ? g : r;  const double mx = (t < b) ? b : t;
	t = (g < r) ? g : r;         const double mn = (b < t) ? b : t;
	const double delta = mx - mn;
	const double sum = mx + mn;
	const double l = sum * 0.5;
	double s = 0.0, h = 0.0;
	if (delta != 0.0) {
		double den = 2.0 - sum;
		if (l < 0.5) den = sum;
		double hh;
		if (r == mx)      { hh = g - b; hh = hh / delta; }
		else if (g == mx) { hh = b - r; hh = hh / delta; hh = hh + 2.0; }
		else              { hh = r - g; hh = hh / delta; hh = hh + 4.0; }
		s = delta / den;
		h = hh / 6.0;
	}
	h = h + m.hue_mod;
	h = mod_wrap_hue(h);
	double ds = m.sat_pos ? (1.0 - s) : s;
	double dl = m.lum_pos ? (1.0 - l) : l;
	ds = ds * m.sat_mod;
	const double s2 = s + ds;
	double dl2 = dl * m.lum_mod;
	const double l2 = l + dl2;
	if (s2 == 0.0) {
		const int32_t v = cvt_s32(l2 * 255.0);
		p[0] = low8(v); p[1] = low8(v); p[2] = low8(v);
		return;
	}
	double q;
	if (l2 > 0.5) { double a = l2 + s2; double mm = l2 * s2; q = a - mm; }
	else          { double a = s2 + 1.0; q = l2 * a; }
	const double pp = (l2 + l2) - q;
	double tR = h + 0.3333333333333333;
	if (tR < 0.0) tR = tR + 1.0; else if (tR > 1.0) tR = tR + (-1.0);
	const double tG = (h < 0.0) ? (h + 1.0) : h;
	double tB = h + (-0.3333333333333333);
	if (tB < 0.0) tB = tB + 1.0; else if (tB > 1.0) tB = tB + (-1.0);
	const int32_t R = cvt_s32(mod_f(tR, pp, q) * 255.0);
	const int32_t G = cvt_s32(mod_f(tG, pp, q) * 255.0);
	const int32_t B = cvt_s32(mod_f(tB, pp, q) * 255.0);
	p[0] = low8(B); p[1] = low8(G); p[2] = low8(R);
}

static inline void modulate_apply(uint8_t *base, ptrdiff_t pitch, int w, int h, int hue, int sat, int lum)
{
	const ModulateParams m = modulate_params(hue, sat, lum);
	for (int y = 0; y < h; ++y) {
		uint8_t *p = base + (ptrdiff_t)y * pitch;
		for (int x = 0; x < w; ++x, p += 4) modulate_pixel(p, m);
	}
}

// =============================================================================================
// noise(level)                          [0x593464]
// =============================================================================================
// Seed = kata ke-0,1,2 dari TVPGetRandomBits128 (0xa28f38) -- tidak deterministik di binary.
struct NoiseSeed { uint32_t c, a, b; };

static inline uint8_t noise_add(uint8_t px, int8_t n, int32_t level)
{
	const int32_t prod = (int32_t)((uint32_t)(int32_t)n * (uint32_t)level);
	const int32_t v = (int32_t)px + (prod >> 8);
	return (uint8_t)(v > 0 ? (v < 255 ? v : 255) : 0);
}

// rnd(): pengganti rand() libc (0x416e40), dipanggil hanya untuk sisa piksel (< 4 di ujung baris)
template <class Rand>
static inline void noise_apply(uint8_t *base, ptrdiff_t pitch, int w, int h, int level, NoiseSeed seed, Rand &&rnd)
{
	uint32_t C = seed.c;
	for (int y = 0; y < h; ++y) {
		uint8_t *p = base + (ptrdiff_t)y * pitch;
		int x = 0;
		if (w >= 4) {
			do {
				const uint32_t t = (uint32_t)x + seed.a;
				const uint32_t u = seed.b ^ (uint32_t)y;
				const uint32_t n0 = t * C + u;
				const uint32_t n1 = t * n0 + u;
				const uint32_t n2 = t * n1 + u;
				C = n2;
				const uint32_t nw[3] = { n0, n1, n2 };
				for (int j = 0; j < 4; ++j)
					for (int ch = 0; ch < 3; ++ch) {
						const int bi = j * 3 + ch;
						const int8_t nb = (int8_t)(uint8_t)(nw[bi >> 2] >> ((bi & 3) * 8));
						p[j * 4 + ch] = noise_add(p[j * 4 + ch], nb, level);
					}
				p += 16; x += 4;
			} while (x < w - 3);
		}
		for (; x < w; ++x, p += 4) {
			const uint32_t r1 = (uint32_t)rnd();
			const uint32_t r2 = (uint32_t)rnd();
			const int8_t d0 = (int8_t)(uint8_t)(r2);
			const int8_t d1 = (int8_t)(uint8_t)(r2 >> 8);
			const int8_t d2 = (int8_t)(uint8_t)((r2 | (r1 << 16)) >> 16);
			p[0] = noise_add(p[0], d0, level);
			p[1] = noise_add(p[1], d1, level);
			p[2] = noise_add(p[2], d2, level);
		}
	}
}

// =============================================================================================
// generateWhiteNoise()                  [0x593b94]
// =============================================================================================
// piksel = rand() * 0xff0001 >> 47 (dipotong ke nol); sama untuk byte 0,1,2; alfa tetap
static inline uint8_t white_value(int32_t r)
{
	const int64_t m = (int64_t)r * 0xff0001ll;
	return (uint8_t)(int32_t)((m >> 47) + (int64_t)((uint64_t)m >> 63));
}
template <class Rand>
static inline void white_noise_apply(uint8_t *base, ptrdiff_t pitch, int w, int h, Rand &&rnd)
{
	for (int y = 0; y < h; ++y) {
		uint8_t *p = base + (ptrdiff_t)y * pitch;
		for (int x = 0; x < w; ++x, p += 4) { const uint8_t v = white_value((int32_t)rnd()); p[0] = p[1] = p[2] = v; }
	}
}

// float seed shader: (float)((double)u32 * 2^-28 + 16.0)   (0x14c3180 = 2^-28 = 3.725290299329276e-09 sebagai double)
static inline float noise_seed_float(uint32_t u) { double d = (double)u; d = d * 3.725290299329276e-09; d = d + 16.0; return (float)d; }

// =============================================================================================
// gaussianBlur(sigma)                   [0x593e60 + filter 0x5943fc]
// =============================================================================================
static const int kMaxKernelTaps = 16385;   // DEVIASI fork: binary tidak membatasi (alokasi raksasa -> crash)

struct GaussKernel {
	int n;                         // panjang kernel (ganjil >= 1)
	std::vector<float> k;          // bobot ternormalisasi
	std::vector<float> table;      // table[k*256 + v] = k[k] * v
};

// panjang kernel; 0 bila melebihi kMaxKernelTaps
static inline int gauss_taps(float sigma)
{
	float r = fabsf(sigma * 0.5f); r = r + 0.25f;
	const float r2 = r + r;
	const double d = ceil((double)r2 + (-0.5));
	double t = d + d; t = t + 1.0;
	const int32_t w = cvt_s32(t);
	const int32_t n = (w > 1) ? w : 1;
	return (n > kMaxKernelTaps) ? 0 : (int)n;
}

static inline bool gauss_build(GaussKernel &g, float sigma)
{
	const int n = gauss_taps(sigma);
	if (n <= 0) return false;
	float r = fabsf(sigma * 0.5f); r = r + 0.25f;
	const float r2 = r + r;
	const double d11 = (double)r2;
	const float rr = r * r2;
	const double dd = (double)rr;
	const double step = 0.02;
	const int c = n >> 1;
	g.n = n; g.k.assign((size_t)n, 0.0f);
	for (int i = c + 1; i < n; ++i) {
		float xf = (float)i; xf = xf - (float)c; xf = xf + (-0.5f);
		const double d8 = (double)xf;
		float acc = 0.0f; double d9 = 1.0;
		for (int j = 0; j < 50; ++j) {
			double d0 = d9 * step; d0 = d0 + d8;
			if (d0 <= d11) {
				const double m = d0 * d0; d0 = -m;
				d0 = d0 / dd;
				d0 = exp(d0);
				acc = acc + (float)d0;
			}
			d9 = d9 + 1.0;
		}
		g.k[(size_t)i] = acc / 50.0f;
	}
	for (int i = 0; i <= c; ++i) g.k[(size_t)i] = g.k[(size_t)(n - 1 - i)];
	{   // bobot tengah: ASLI memakai x = k*0.02 + 0.5 (bukan -0.5); ditiru
		float acc = 0.0f; double d8 = 0.0;
		for (int j = 0; j < 51; ++j) {
			double d0 = d8 * step; d0 = d0 + 0.5;
			const double m = d0 * d0; d0 = -m;
			d0 = d0 / dd;
			d0 = exp(d0);
			acc = acc + (float)d0;
			d8 = d8 + 1.0;
		}
		g.k[(size_t)c] = acc / 51.0f;
	}
	float sum = 0.0f;
	for (int i = 0; i < n; ++i) sum = sum + g.k[(size_t)i];
	for (int i = 0; i < n; ++i) g.k[(size_t)i] = g.k[(size_t)i] / sum;
	g.table.resize((size_t)n * 256);
	for (int i = 0; i < n; ++i)
		for (int v = 0; v < 256; ++v) g.table[(size_t)i * 256 + (size_t)v] = g.k[(size_t)i] * (float)v;
	return true;
}

static inline uint8_t gauss_store(float v) { v = v + 0.5f; return low8(cvt_s32f(v)); }

// filter 1-D atas len piksel 4-kanal (0x5943fc). in != out.
static inline void gauss_line(const float *table, const float *kern, int n, const uint8_t *in, uint8_t *out, int len)
{
	const int c = n / 2;
	if (len < 1) return;
	if (n > len) {
		// ---- kernel lebih panjang dari baris. ANOMALI asli: pembilang memakai kern[j] (indeks sumber),
		//      penyebut memakai kern[c-i+j]; ditiru apa adanya. ----
		for (int i = 0; i < len; ++i) {
			float s1 = 0.0f; int idx = c - i;
			for (int j = 0; j < len; ++j, ++idx) if (idx >= 0 && idx < n) s1 = s1 + kern[idx];
			for (int ch = 0; ch < 4; ++ch) {
				float s2 = 0.0f;
				for (int j = 0; j < len; ++j)
					if (j >= i - c && j <= i + c) { float px = (float)in[4 * j + ch]; float m = kern[j] * px; s2 = s2 + m; }
				out[4 * i + ch] = gauss_store(s2 / s1);
			}
		}
		return;
	}
	int i = 0;
	if (n >= 2) {   // tepi kiri: jendela terpotong, dinormalisasi
		for (; i < c; ++i) {
			const int ks = c - i, cnt = n - ks;
			const float *kp = kern + ks;
			float s2 = 0.0f; for (int m = 0; m < cnt; ++m) s2 = s2 + kp[m];
			for (int ch = 0; ch < 4; ++ch) {
				float s3 = 0.0f;
				for (int m = 0; m < cnt; ++m) { float px = (float)in[4 * m + ch]; float mm = kp[m] * px; s3 = s3 + mm; }
				out[4 * i + ch] = gauss_store(s3 / s2);
			}
		}
	}
	for (; i < len - c; ++i) {   // interior: jumlah tabel
		const uint8_t *src = in + 4 * (ptrdiff_t)(i - c);
		for (int ch = 0; ch < 4; ++ch) {
			float s = 0.0f;
			for (int k = 0; k < n; ++k) s = s + table[(size_t)k * 256 + src[4 * k + ch]];
			out[4 * i + ch] = gauss_store(s);
		}
	}
	for (; i < len; ++i) {       // tepi kanan
		const int cnt = len - i + c;
		const uint8_t *src = in + 4 * (ptrdiff_t)(i - c);
		float s2 = 0.0f; for (int k = 0; k < cnt; ++k) s2 = s2 + kern[k];
		for (int ch = 0; ch < 4; ++ch) {
			float s3 = 0.0f;
			for (int k = 0; k < cnt; ++k) { float px = (float)src[4 * k + ch]; float mm = kern[k] * px; s3 = s3 + mm; }
			out[4 * i + ch] = gauss_store(s3 / s2);
		}
	}
}

// seluruh gambar w x h, semua 4 kanal termasuk alfa. Horizontal dulu (ke buffer sementara), lalu vertikal.
// false bila sigma terlalu besar (kMaxKernelTaps) -> gambar tidak diubah.
static inline bool gaussian_apply(uint8_t *base, ptrdiff_t pitch, int w, int h, float sigma)
{
	if (w < 1 || h < 1) return true;
	GaussKernel g;
	if (!gauss_build(g, sigma)) return false;
	std::vector<uint8_t> tmp((size_t)w * (size_t)h * 4);
	for (int y = 0; y < h; ++y)
		gauss_line(g.table.data(), g.k.data(), g.n, base + (ptrdiff_t)y * pitch, tmp.data() + (size_t)y * (size_t)w * 4, w);
	std::vector<uint8_t> col((size_t)h * 4), col2((size_t)h * 4);
	for (int x = 0; x < w; ++x) {
		for (int y = 0; y < h; ++y)
			for (int ch = 0; ch < 4; ++ch) col[(size_t)y * 4 + (size_t)ch] = tmp[((size_t)y * (size_t)w + (size_t)x) * 4 + (size_t)ch];
		gauss_line(g.table.data(), g.k.data(), g.n, col.data(), col2.data(), h);
		for (int y = 0; y < h; ++y)
			for (int ch = 0; ch < 4; ++ch) base[(ptrdiff_t)y * pitch + (ptrdiff_t)x * 4 + ch] = col2[(size_t)y * 4 + (size_t)ch];
	}
	return true;
}

// area box-blur untuk jalur non-software: radius = trunc(sigma + 0.99f)  (0x14c30ec)
static inline void gauss_box_radius(float sigma, int *neg, int *pos)
{
	const float s = sigma + 0.9900000095367432f;
	*pos = cvt_s32f(s); *neg = cvt_s32f(-s);
}

} // namespace layerex_image
#endif
