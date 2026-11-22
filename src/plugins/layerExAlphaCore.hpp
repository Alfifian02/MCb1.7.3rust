// layerExAlphaCore.hpp - inti piksel modul layerExAlpha.dll (SATU berkas untuk seluruh kelompok Alpha).
// Tanpa dependensi Kirikiri/TJS (hanya libstdc++), dapat diuji di host. Spesifikasi: docs/spec/layerExAlpha_*.md.
// Direkonstruksi dari disassembly libgame.so 1.3.9 arm64. Hasil penggabungan sesi S6, S7, S8, S9, S10:
//   Alpha-1 (S6)   turnAlpha turnAlpha2 turnAlpha3 turnOutline AlphaColorRect AlphaDivide AlphaAdd AlphaAdd2 BlackForAlpha
//                  ColorAlphaAdd ColorVectorEffectAdd ExtraColorVectorEffectAdd MaskRect ExMaskRect
//   Alpha-2 (S7)   LightForAlpha LuminanceForAlpha LevelMaskRect LevelMaskRect2 MaskImageRideRect MaskImageRideRect2/3
//   Alpha-3 (S8)   MaskImageRideRect4 MaskImageRideRect5
//   Alpha-4 (S9)   MaskImageRideRect7 MaskImageRideRect8 AlphaBorderInnerRect
//   Alpha-5 (S10)  MaskImageEffectRect AlphaBorderImageCoverRect
// BELUM ada (3 dari 31 nama): AlphaBlend (0x5d5894) WhiteToBlueBlurAdd (0x5d5f98) ExMaskRect2 (0x5d6ea0).
// PERHATIAN nama S6: pemetaan nama->kode S6 memakai peta lama yang bergeser satu slot. Penggabungan memakai pasangan
// NAME->CODE yang dibaca langsung dari blok registrasi 0x5d43b0 (lihat docs/spec/layerExAlpha_umum.md, bagian "Pemetaan").
//
// Konvensi: piksel 32 bit, byte memori B,G,R,A (alfa = offset +3). Pitch dalam BYTE. Pembantu aritmetika di blok bersama
// di bawah (SATU definisi; sebelum penggabungan tiap sesi punya salinan sendiri dengan tipe/kualifikator berbeda).
#ifndef LAYEREX_ALPHA_CORE_HPP
#define LAYEREX_ALPHA_CORE_HPP

#include <stdint.h>
#include <stddef.h>
#include <limits.h>
#include <math.h>
#include <vector>

// Binary memakai fmul lalu fadd terpisah (tanpa FMA). Clang NDK default -ffp-contract=on hanya melebur dalam satu pernyataan;
// pragma ini menutup celah itu (juga mulf/muld di bawah). Dipakai oleh bagian Alpha-3 (S8).
#if defined(__clang__)
#pragma clang fp contract(off)
#endif

namespace layerex_alpha {

typedef uint8_t u8;

// DEVIASI/ASUMSI: AlphaDivide di binary membandingkan alfa piksel PERTAMA tiap baris lalu mengisi SELURUH baris
// dengan hasilnya (kemungkinan bug asli). 0 = meniru binary (default); 1 = per piksel (maksud yang tampak).
#ifndef LAYEREX_ALPHADIVIDE_PER_PIXEL
#define LAYEREX_ALPHADIVIDE_PER_PIXEL 0
#endif

// batas pengaman tabel (aslinya tanpa batas; di atas ini aslinya praktis gagal alokasi)
static const int64_t kMaxTable = (int64_t)1 << 24;

// ---- numerik setara AArch64 (blok bersama) -----------------------------------------------------------------------------
inline float  mulf(float a, float b)   { volatile float  r = a * b; return r; }
inline double muld(double a, double b) { volatile double r = a * b; return r; }
inline int cvtzs(float x) {            // fcvtzs w, s : potong ke nol, jenuh, NaN -> 0
	if (x != x) return 0;
	if (x >= 2147483648.0f) return INT_MAX;
	if (x <= -2147483648.0f) return INT_MIN;
	return (int)x;
}
inline int cvtzs(double x) {           // fcvtzs w, d (setara versi S8 `<= -2147483649.0`: hasil identik di seluruh domain)
	if (x != x) return 0;
	if (x >= 2147483648.0) return INT_MAX;
	if (x <= -2147483648.0) return INT_MIN;
	return (int)x;
}
inline uint32_t cvtzu(float x) {       // fcvtzu w, s : potong ke nol, jenuh 0..UINT32_MAX, NaN -> 0
	if (x != x) return 0;
	if (x <= 0.0f) return 0;
	if (x >= 4294967296.0f) return 0xFFFFFFFFu;
	return (uint32_t)x;
}
// aritmetika int32 melingkar (w-register)
inline int32_t add32(int32_t a, int32_t b) { return (int32_t)((uint32_t)a + (uint32_t)b); }
inline int32_t sub32(int32_t a, int32_t b) { return (int32_t)((uint32_t)a - (uint32_t)b); }
inline int32_t madd32(int32_t a, int32_t b, int32_t c) { return (int32_t)((uint32_t)a * (uint32_t)b + (uint32_t)c); }
inline int32_t sdiv32(int32_t a, int32_t b)   // sdiv w,w,w : bagi nol -> 0 ; INT_MIN / -1 -> INT_MIN (tanpa jebakan)
{
	if (b == 0) return 0;
	if (a == INT32_MIN && b == -1) return INT32_MIN;
	return a / b;
}
// pola "cmp x,#0x100 ; csinv ... ; bic x,asr#31": x<0 -> 0, x<256 -> x, selain itu -> 255
inline int clamp256(int32_t x) { return x < 0 ? 0 : (x < 256 ? x : 255); }

// ============================================================================================================================
// Alpha-1 (S6): operasi seluruh-gambar + MaskRect/ExMaskRect
// ============================================================================================================================
#define LAYEREX_ROWS(buf, pitch, w, h, p)                                                       \
	for (int y_ = 0; y_ < (h); ++y_)                                                            \
		for (u8 *row_ = (buf) + (ptrdiff_t)y_ * (pitch), *p = row_; (w) > 0 && (p) < row_ + 4 * (ptrdiff_t)(w); p += 4)

// ======================= operasi seluruh-gambar (w x h piksel, bukan klip) ==========================================
// turnAlpha (0x5d4c04): piksel = ~piksel & 0xFF000000  -> RGB = 0, A = 255 - A.
static inline void turn_alpha(u8 *buf, ptrdiff_t pitch, int w, int h)
{ LAYEREX_ROWS(buf, pitch, w, h, p) { p[0] = p[1] = p[2] = 0; p[3] = (u8)~p[3]; } }

// turnAlpha2 (0x5d4dc4): A ^= 255 (RGB tetap).
static inline void turn_alpha2(u8 *buf, ptrdiff_t pitch, int w, int h)
{ LAYEREX_ROWS(buf, pitch, w, h, p) { p[3] ^= 0xFF; } }

// turnAlpha3 (0x5d4f80): A != 0 -> A = 255 - A; A == 0 tetap 0.
static inline void turn_alpha3(u8 *buf, ptrdiff_t pitch, int w, int h)
{ LAYEREX_ROWS(buf, pitch, w, h, p) { if (p[3] != 0) p[3] ^= 0xFF; } }

// turnOutline (0x5d502c): A == 255 -> piksel = 0 (semua kanal); selain itu RGB = 0, A tetap.
static inline void turn_outline(u8 *buf, ptrdiff_t pitch, int w, int h)
{ LAYEREX_ROWS(buf, pitch, w, h, p) { p[0] = p[1] = p[2] = 0; if (p[3] > 0xFE) p[3] = 0; } }

// AlphaColorRect (0x5d5204) (r,g,b): kanal yang nilainya == -1 TIDAK diubah; selain itu disimpan (8 bit rendah).
// Urutan argumen binary: arg1 -> byte2 (R), arg2 -> byte1 (G), arg3 -> byte0 (B).
static inline void alpha_color_rect(u8 *buf, ptrdiff_t pitch, int w, int h, int r, int g, int b)
{
	LAYEREX_ROWS(buf, pitch, w, h, p) {
		if (b != -1) p[0] = (u8)b;
		if (g != -1) p[1] = (u8)g;
		if (r != -1) p[2] = (u8)r;
	}
}

// AlphaDivide (0x5d5400) (th): ambang biner pada alfa. Lihat LAYEREX_ALPHADIVIDE_PER_PIXEL di atas.
static inline void alpha_divide(u8 *buf, ptrdiff_t pitch, int w, int h, int th)
{
	for (int y = 0; y < h && w > 0; ++y) {
		u8 *row = buf + (ptrdiff_t)y * pitch;
		u8 v = ((int)row[3] >= th) ? 0xFF : 0x00;            // binary: alfa piksel pertama baris, bandingkan bertanda
		for (int x = 0; x < w; ++x) {
#if LAYEREX_ALPHADIVIDE_PER_PIXEL
			row[4 * x + 3] = ((int)row[4 * x + 3] >= th) ? 0xFF : 0x00;
#else
			row[4 * x + 3] = v;
#endif
		}
	}
}

// AlphaAdd (0x5d54a8) (add, flag): A = clamp(A + add, 0..255). flag bit0 = 1 -> piksel dengan A == 0 dilewati.
static inline void alpha_add(u8 *buf, ptrdiff_t pitch, int w, int h, int add, int flag)
{
	LAYEREX_ROWS(buf, pitch, w, h, p) {
		if ((flag & 1) && p[3] == 0) continue;
		int32_t v = add32(p[3], add);
		if (v < 0) v = 0;
		p[3] = (u8)(v < 255 ? v : 255);
	}
}

// AlphaAdd2 (0x5d5708) (add, thr): v = A + add; (v < thr atau v < 0) -> 0; selain itu min(v, 255).
static inline void alpha_add2(u8 *buf, ptrdiff_t pitch, int w, int h, int add, int thr)
{
	LAYEREX_ROWS(buf, pitch, w, h, p) {
		int32_t v = add32(p[3], add);
		p[3] = (v < thr || v < 0) ? 0 : (u8)(v < 255 ? v : 255);
	}
}

// BlackForAlpha (0x5d57cc): A = (R+G+B)/3 (pembagian bulat ke bawah); RGB = 0.
static inline void black_for_alpha(u8 *buf, ptrdiff_t pitch, int w, int h)
{
	LAYEREX_ROWS(buf, pitch, w, h, p) {
		unsigned s = (unsigned)p[0] + p[1] + p[2];
		p[0] = p[1] = p[2] = 0;
		p[3] = (u8)(s / 3);
	}
}

// ColorAlphaAdd (0x5d5950) (rmax, gmax, bmax, add): tiap kanal warna = min(kanal + add, maks) (bertanda, tanpa batas
// bawah; 8 bit rendah disimpan); A = clamp(A - add, 0..255) (A <= 0 -> 0).
static inline void color_alpha_add(u8 *buf, ptrdiff_t pitch, int w, int h, int rmax, int gmax, int bmax, int add)
{
	LAYEREX_ROWS(buf, pitch, w, h, p) {
		int32_t b = add32(p[0], add); if (b > bmax) b = bmax; p[0] = (u8)b;
		int32_t g = add32(p[1], add); if (g > gmax) g = gmax; p[1] = (u8)g;
		int32_t r = add32(p[2], add); if (r > rmax) r = rmax; p[2] = (u8)r;
		int32_t a = sub32(p[3], add);
		p[3] = a <= 0 ? 0 : (u8)(a < 255 ? a : 255);
	}
}

// ColorVectorEffectAdd (0x5d5b8c) (r0,r1,g0,g1,b0,b1,add)  [arg1..arg7]
//   step = 255/add (sdiv); dR=(r1-r0)/step, dG=(g1-g0)/step, dB=(b1-b0)/step (sdiv).
//   piksel dengan A != 0: A' = clamp(A - add, 0..255); t = (255 - A')/add; R = clamp(dR*t + r0) dst.
//   Kanal: byte2 <- (arg1,arg2), byte1 <- (arg3,arg4), byte0 <- (arg5,arg6).
static inline void color_vector_effect_add(u8 *buf, ptrdiff_t pitch, int w, int h,
												 int r0, int r1, int g0, int g1, int b0, int b1, int add)
{
	const int32_t step = sdiv32(255, add);
	const int32_t dR = sdiv32(sub32(r1, r0), step), dG = sdiv32(sub32(g1, g0), step), dB = sdiv32(sub32(b1, b0), step);
	LAYEREX_ROWS(buf, pitch, w, h, p) {
		if (p[3] == 0) continue;
		const int a2 = clamp256(sub32(p[3], add));
		p[3] = (u8)a2;
		const int32_t t = sdiv32(255 - a2, add);
		p[2] = (u8)clamp256(madd32(dR, t, r0));
		p[1] = (u8)clamp256(madd32(dG, t, g0));
		p[0] = (u8)clamp256(madd32(dB, t, b0));
	}
}

// ExtraColorVectorEffectAdd (0x5d5cdc) (r0,r1,g0,g1,b0,b1,add)  -- parameter & prakomputasi sama dengan di atas, plus
//   thr = (r1+g1+b1)/3 (dibulatkan ke nol). Per piksel dengan A != 0: A' = clamp(A - add), t = (255 - A')/add,
//   avg = (R+G+B)/3. Bila t != 0 dan avg > thr: warna = gradien (seperti ColorVectorEffectAdd).
//   Selain itu ("cabang A"): tiap kanal c (byte0,1,2) dengan avg >= nilainya diganti konstanta (arg1,arg3,arg5);
//   lalu satu kanal dipilih lewat jaringan perbandingan (di bawah; ARTI/maksudnya ASUMSI, tetapi kode meniru binary
//   persis) dan diubah menjadi max(nilai +/- add, avg).
static inline void extra_color_vector_effect_add(u8 *buf, ptrdiff_t pitch, int w, int h,
										  int r0, int r1, int g0, int g1, int b0, int b1, int add)
{
	const int32_t step = sdiv32(255, add);
	const int32_t dR = sdiv32(sub32(r1, r0), step), dG = sdiv32(sub32(g1, g0), step), dB = sdiv32(sub32(b1, b0), step);
	const int32_t sum = add32(add32(g1, r1), b1);                       // (w4+w2)+w6
	const int64_t prod = (int64_t)sum * 0x55555556LL;                   // smull
	const int32_t thr = (int32_t)((uint32_t)((uint64_t)prod >> 32) + (uint32_t)((uint64_t)prod >> 63));
	LAYEREX_ROWS(buf, pitch, w, h, p) {
		if (p[3] == 0) continue;
		const int a2 = clamp256(sub32(p[3], add));
		p[3] = (u8)a2;
		const int32_t t = sdiv32(255 - a2, add);
		const int B = p[0], G = p[1], R = p[2];
		const int32_t avg = (int32_t)(((uint64_t)(unsigned)(G + R + B) * 0xAAAAAAABULL) >> 33);
		if (t != 0 && avg > thr) {
			p[2] = (u8)clamp256(madd32(dR, t, r0));
			p[1] = (u8)clamp256(madd32(dG, t, g0));
			p[0] = (u8)clamp256(madd32(dB, t, b0));
			continue;
		}
		unsigned P0 = B, P1 = G, P2 = R;
		if ((unsigned)avg >= (unsigned)B) { p[0] = (u8)r0; P0 = (u8)r0; }
		if ((unsigned)avg >= (unsigned)G) { p[1] = (u8)g0; P1 = (u8)g0; }
		if ((unsigned)avg >= (unsigned)R) { p[2] = (u8)b0; P2 = (u8)b0; }
		const int c1 = (P0 <= P2) ? 1 : 0;
		const int sel = (P0 > P1) ? 2 * c1 : 1 + (P1 >= P2 ? 1 : 0);
		const unsigned cur = sel == 0 ? P0 : (sel == 1 ? P1 : P2);
		const int32_t tmp = ((unsigned)avg >= cur) ? add32((int32_t)cur, add) : sub32((int32_t)cur, add);
		p[sel] = (u8)(tmp > avg ? tmp : avg);
	}
}

// ======================= MaskRect : operasi pada alfa dua gambar ================================================
// Mode (arg terakhir): 0 = salin bidang mask (diurus pemanggil lewat CopyRect), 1 = tambah, 2 = kurang, 3 = kali.
enum { MASK_COPY = 0, MASK_ADD = 1, MASK_SUB = 2, MASK_MUL = 3 };

static inline u8 mask_op(int mode, unsigned d, unsigned s)
{
	switch (mode) {
	case MASK_COPY: return (u8)s;
	case MASK_ADD: { unsigned v = d + s; return (u8)(v < 255 ? v : 255); }       // min(d+s, 255)
	case MASK_SUB: { int v = (int)(d + s) - 255; return (u8)(v < 0 ? 0 : v); }   // max(d+s-255, 0)
	default:       return (u8)((d * s) >> 8);                                       // MASK_MUL: (d*s)>>8, BUKAN /255
	}
}

// Jenis operasi MaskRect menurut `mode` (cabang di 0x5d6298 dst.): 0 = CopyRect bidang mask (pemanggil), 1 = operasi piksel
// (mode 1..3), 2 = mode tak dikenal -> catat log "layerExAlpha: noting type" lalu redraw (GPU: binary UB, fork sama dengan CPU).
static inline int mask_kind(int mode) { return mode == MASK_COPY ? 0 : (mode >= MASK_ADD && mode <= MASK_MUL ? 1 : 2); }

// MaskRect CPU, mode 1..3 (0x5d62c4..0x5d6358). Piksel (dx+i, dy+j) alfa tujuan = op(alfa tujuan, alfa sumber(sx+i, sy+j)).
// DEVIASI KEAMANAN: binary tidak memeriksa batas (membaca/menulis di luar gambar); di sini wilayah dipotong ke kedua
// gambar (identik untuk masukan di dalam batas).
static inline void mask_rect_cpu(u8 *dst, ptrdiff_t dpitch, int dw_, int dh_, const u8 *src, ptrdiff_t spitch, int sw_, int sh_,
								   int dx, int dy, int sx, int sy, int sw, int sh, int mode)
{
	if (mode < MASK_ADD || mode > MASK_MUL) return;
	if (sw < 1 || sh < 1) return;
	if (dx < 0) { sx = sub32(sx, dx); sw = add32(sw, dx); dx = 0; }
	if (dy < 0) { sy = sub32(sy, dy); sh = add32(sh, dy); dy = 0; }
	if (sx < 0) { dx = sub32(dx, sx); sw = add32(sw, sx); sx = 0; }
	if (sy < 0) { dy = sub32(dy, sy); sh = add32(sh, sy); sy = 0; }
	if (sw < 1 || sh < 1 || dx >= dw_ || dy >= dh_ || sx >= sw_ || sy >= sh_) return;
	if (sw > dw_ - dx) sw = dw_ - dx;
	if (sh > dh_ - dy) sh = dh_ - dy;
	if (sw > sw_ - sx) sw = sw_ - sx;
	if (sh > sh_ - sy) sh = sh_ - sy;
	for (int j = 0; j < sh; ++j) {
		u8 *d = dst + (ptrdiff_t)(dy + j) * dpitch + (ptrdiff_t)dx * 4 + 3;
		const u8 *s = src + (ptrdiff_t)(sy + j) * spitch + (ptrdiff_t)sx * 4 + 3;
		for (int i = 0; i < sw; ++i) d[4 * i] = mask_op(mode, d[4 * i], s[4 * i]);
	}
}

// ======================= ExMaskRect =====================================================================================
// Argumen: (dx, dy, src, <arg4 TIDAK DIPAKAI>, sy, sw, sh, mode). Seluruh layer `this` diproses: baris di atas wilayah,
// kolom kiri/kanan wilayah, dan baris di bawah wilayah diisi alfa = 0; wilayah itu sendiri = mask_op(mode, tujuan, sumber).
// Sumber dibaca pada kolom max(dx,0) dan baris (dy>=0 ? sy : -dy) -- bukan pada arg4 (arg4 tak terbaca; ASUMSI: sisa API).
enum { EXMASK_LOG_LEFT = 1, EXMASK_LOG_TOP = 2, EXMASK_LOG_MODE = 4, EXMASK_NO_REDRAW = 8 };

// Pemeriksaan awal (sebelum src dibaca): dx > W atau dy > H -> catat log, tanpa perubahan & tanpa redraw.
static inline unsigned exmask_precheck(int W, int H, int dx, int dy)
{
	if (W >= dx && H >= dy) return 0;
	unsigned f = EXMASK_NO_REDRAW;
	if (W <= dx) f |= EXMASK_LOG_LEFT;
	if (H <= dy) f |= EXMASK_LOG_TOP;
	return f;
}

// `src_w/src_h` = ukuran gambar sumber (DEVIASI KEAMANAN: binary tidak membatasi bacaan; piksel di luar sumber dianggap
// alfa 0). Mengembalikan bit MASK2_* ; EXMASK_NO_REDRAW -> pemanggil TIDAK memanggil redraw().
static inline unsigned exmask_rect(u8 *dst, ptrdiff_t dpitch, int W, int H, const u8 *src, ptrdiff_t spitch, int src_w, int src_h,
									int dx, int dy, int sy, int sw, int sh, int mode)
{
	unsigned f = exmask_precheck(W, H, dx, dy);
	if (f) return f;

	const int x0 = dx < 0 ? 0 : dx;                                   // w24
	const int w11 = add32(dx < 0 ? dx : 0, sw);
	int cw = (add32(w11, x0) > W) ? (W - x0) : w11;                   // w26 : lebar salin
	const int y0 = dy < 0 ? 0 : dy;                                   // w25
	const int srow0 = dy >= 0 ? sy : sub32(0, dy);                    // w28
	const int w12 = add32(dy < 0 ? dy : 0, sh);
	int rows = (add32(w12, y0) > H) ? (H - y0) : w12;                 // w27 : jumlah baris salin

	// baris di atas wilayah
	for (int y = 0; y < y0; ++y) {
		u8 *row = dst + (ptrdiff_t)y * dpitch;
		for (int x = 0; x < W; ++x) row[4 * x + 3] = 0;
	}
	if ((unsigned)mode > 3u) return f | EXMASK_LOG_MODE | EXMASK_NO_REDRAW;   // setelah pembersihan atas (urutan binary)

	if (cw < 0) cw = 0;                // DEVIASI KEAMANAN: binary menulis melewati ujung baris bila lebar salin negatif
	int yend = y0;
	for (int r = 0; r < rows; ++r) {
		const int y = y0 + r;
		u8 *row = dst + (ptrdiff_t)y * dpitch;
		for (int x = 0; x < x0 && x < W; ++x) row[4 * x + 3] = 0;                 // kiri (hanya bila dx >= 1)
		for (int i = 0; i < cw; ++i) {
			const int sxp = x0 + i, syp = add32(srow0, r);
			unsigned s = 0;
			if (sxp >= 0 && sxp < src_w && syp >= 0 && syp < src_h) s = src[(ptrdiff_t)syp * spitch + (ptrdiff_t)sxp * 4 + 3];
			u8 *d = row + 4 * (x0 + i) + 3;
			*d = mask_op(mode, *d, s);
		}
		for (int x = x0 + cw; x < W; ++x) row[4 * x + 3] = 0;                     // kanan
		yend = y + 1;
	}
	// baris di bawah wilayah (DEVIASI KEAMANAN: bila rows < 1 binary mulai dari indeks baris yang salah -> melampaui buffer;
	// di sini mulai dari baris pertama setelah bagian atas, hasil di dalam buffer identik)
	for (int y = yend; y < H; ++y) {
		u8 *row = dst + (ptrdiff_t)y * dpitch;
		for (int x = 0; x < W; ++x) row[4 * x + 3] = 0;
	}
	return f;
}

#undef LAYEREX_ROWS

// ============================================================================================================================
// Alpha-2 (S7): LightForAlpha .. MaskImageRideRect3
// ============================================================================================================================
// konstanta .rodata 0x14c9b70/74/78 (float32)
static const float kLumaC0 = 0.298911988735199f;   // dikalikan ke byte 0 (B)  <- perilaku binary, lihat spesifikasi
static const float kLumaC1 = 0.5866109728813171f;  // dikalikan ke byte 1 (G)
static const float kLumaC2 = 0.11447799950838089f; // dikalikan ke byte 2 (R)

// ---- LightForAlpha(src) ----------------------------------------------------------------------
// dst piksel = (B=0,G=0,R=0, A = src.G)
inline void light_for_alpha(uint8_t *dst, ptrdiff_t dpitch, const uint8_t *src, ptrdiff_t spitch, int w, int h)
{
	for (int y = 0; y < h; ++y, dst += dpitch, src += spitch)
		for (int x = 0; x < w; ++x) {
			uint8_t *d = dst + (ptrdiff_t)x * 4;
			const uint8_t *s = src + (ptrdiff_t)x * 4;
			d[0] = 0; d[1] = 0; d[2] = 0; d[3] = s[1];
		}
}

// ---- LuminanceForAlpha(src) ------------------------------------------------------------------
// dst piksel = (0,0,0, A = trunc((B*c0 + G*c1) + R*c2)), A diambil low-byte (tanpa clamp)
inline uint8_t luminance_of(const uint8_t *s)
{
	float t = mulf((float)s[0], kLumaC0);
	float u = mulf((float)s[1], kLumaC1);
	t = t + u;
	u = mulf((float)s[2], kLumaC2);
	t = t + u;
	return (uint8_t)cvtzs(t);
}
inline void luminance_for_alpha(uint8_t *dst, ptrdiff_t dpitch, const uint8_t *src, ptrdiff_t spitch, int w, int h)
{
	for (int y = 0; y < h; ++y, dst += dpitch, src += spitch)
		for (int x = 0; x < w; ++x) {
			uint8_t *d = dst + (ptrdiff_t)x * 4;
			const uint8_t a = luminance_of(src + (ptrdiff_t)x * 4);
			d[0] = 0; d[1] = 0; d[3] = a; d[2] = 0;
		}
}

// ---- tabel level (LevelMaskRect, LevelMaskRect2) ---------------------------------------------
// LevelMaskRect(a, b): tabel simetris, n = 2b+1. Kembalikan false bila b < 0 atau terlalu besar
// (aslinya: vector length_error -> crash).
inline bool level_sym_table(int b, std::vector<int> &t)
{
	t.clear();
	if (b < 0 || (int64_t)b * 2 + 1 > kMaxTable) return false;
	const int n = 2 * b + 1;
	t.assign((size_t)n, 0);
	const float s0 = 255.0f / (float)(b + 1);
	for (int i = 0; i < n; ++i) {
		int v;
		if (i <= b)          v = cvtzs(mulf(s0, (float)(i + 1)) + 0.5f);
		else if (i - b == 1) v = 255;                                        // plateau dua entri (i = b dan b+1)
		else                 v = cvtzs(mulf(s0, (float)(n - i)) + 0.5f);
		t[(size_t)i] = v;
	}
	return true;
}

// LevelMaskRect2(a, l, r): tabel asimetris, n = l+r+1 (int32 melingkar). n < 0 -> false (aslinya crash); n == 0 -> kosong.
inline bool level_asym_table(int l, int r, std::vector<int> &t)
{
	t.clear();
	const int n = add32(add32(l, r), 1);
	if (n < 0 || n > kMaxTable) return false;
	if (n == 0) return true;
	t.assign((size_t)n, 0);
	const float s0 = 255.0f / (float)add32(l, 1);
	const float s1 = 255.0f / (float)add32(r, 1);
	for (int i = 0; i < n; ++i) {
		int v;
		if ((int64_t)i < (int64_t)l)       v = cvtzs(mulf(s0, (float)(i + 1)) + 0.5f);
		else if ((uint32_t)i == (uint32_t)l) v = 255;
		else                                v = cvtzs(mulf(s1, (float)sub32(n, i)) + 0.5f);
		if (v > 0) { if (v > 255) v = 255; } else v = 0;                     // clamp 0..255 (bukan sekadar min)
		t[(size_t)i] = v;
	}
	return true;
}

// Terapkan tabel ke kanal alfa layer ini: alfa' = t[alfa-(a-l)] bila lo <= alfa <= hi, selain itu 0.
//   hi = min(a+r, 255); off = a-l; lo = max(0, off). Hanya byte alfa yang ditulis.
inline void level_mask_apply(uint8_t *buf, ptrdiff_t pitch, int w, int h, int a, int l, int r, const std::vector<int> &t)
{
	int hi = add32(a, r); if (hi >= 255) hi = 255;
	const int off = sub32(a, l);
	const int lo = off < 0 ? 0 : off;
	for (int y = 0; y < h; ++y, buf += pitch)
		for (int x = 0; x < w; ++x) {
			uint8_t *p = buf + (ptrdiff_t)x * 4 + 3;
			const int v = *p;
			int out = 0;
			if (!(lo > v) && !(hi < v)) {
				const int64_t idx = (int64_t)v - off;
				if (idx >= 0 && idx < (int64_t)t.size()) out = t[(size_t)idx];   // di luar tabel = UB di aslinya
			}
			*p = (uint8_t)out;
		}
}

// ---- tabel "ride" (MaskImageRideRect, MaskImageRideRect2) ------------------------------------
struct RideTables {
	std::vector<float> t0;                 // bobot piksel tujuan (1 - tent)
	std::vector<int>   t1, t2, t3;         // warna * tent  -> R, G, B
};

inline void ride_clamp(RideTables &T, int i)
{
	float &f = T.t0[(size_t)i];
	if (f >= 1.0f) f = 1.0f;
	int *c[3] = { &T.t1[(size_t)i], &T.t2[(size_t)i], &T.t3[(size_t)i] };
	for (int k = 0; k < 3; ++k) if (*c[k] >= 255) *c[k] = 255;
	if (!(f > 0.0f) && f == f) f = 0.0f;                                     // NaN tetap NaN (b.hi)
	for (int k = 0; k < 3; ++k) if (!(*c[k] > 0)) *c[k] = 0;
}

// MaskImageRideRect (dua sisi): n = l+r+1. Campuran float/double persis seperti aslinya.
inline bool ride2_tables(int l, int r, int c1, int c2, int c3, RideTables &T)
{
	T = RideTables();
	const int n = add32(add32(l, r), 1);
	if (n < 0 || n > kMaxTable) return false;
	if (n == 0) return true;
	T.t0.assign((size_t)n, 0.0f); T.t1.assign((size_t)n, 0); T.t2.assign((size_t)n, 0); T.t3.assign((size_t)n, 0);
	const int L1 = add32(l, 1), R1 = add32(r, 1);
	const float f1 = (float)c1, f2 = (float)c2, f3 = (float)c3;
	const double d2 = 1.0 / (double)L1;
	const double d3 = (double)c1 / (double)L1;
	const double d4  = (double)(f2 / (float)L1);
	const double d5  = (double)(f3 / (float)L1);
	const double d6  = (double)(1.0f / (float)R1);
	const double d7  = (double)(f1 / (float)R1);
	const double d16 = (double)(f2 / (float)R1);
	const double d17 = (double)(f3 / (float)R1);
	for (int i = 0; i < n; ++i) {
		if ((int64_t)i < (int64_t)l) {
			const double d19 = (double)(i + 1);
			T.t0[i] = (float)muld(d2, (double)sub32(l, i));
			T.t1[i] = cvtzs(muld(d3, d19) + 0.5);
			T.t2[i] = cvtzs(muld(d4, d19) + 0.5);
			T.t3[i] = cvtzs(muld(d5, d19) + 0.5);
		} else if ((uint32_t)i == (uint32_t)l) {
			T.t0[i] = 0.0f; T.t1[i] = c1; T.t2[i] = c2; T.t3[i] = c3;
		} else {
			const double d19 = (double)add32(add32(r, sub32(l, i)), 1);      // = n - i
			const double d18 = (double)sub32(i, l);
			T.t1[i] = cvtzs(muld(d7, d19) + 0.5);
			T.t2[i] = cvtzs(muld(d16, d19) + 0.5);
			T.t0[i] = (float)muld(d6, d18);
			T.t3[i] = cvtzs(muld(d17, d19) + 0.5);
		}
		ride_clamp(T, i);
	}
	return true;
}

// MaskImageRideRect2 (satu sisi): n = w+1. Semua float32.
inline bool ride1_tables(int w, int c1, int c2, int c3, RideTables &T)
{
	T = RideTables();
	const int n = add32(w, 1);
	if (n < 0 || n > kMaxTable) return false;
	if (n == 0) return true;
	T.t0.assign((size_t)n, 0.0f); T.t1.assign((size_t)n, 0); T.t2.assign((size_t)n, 0); T.t3.assign((size_t)n, 0);
	const float s5 = (float)n;
	const float s2 = (float)c1 / s5, s3 = (float)c2 / s5, s4 = (float)c3 / s5, s5r = 1.0f / s5;
	for (int i = 0; i < n; ++i) {
		if ((int64_t)i < (int64_t)w) {
			const float s7 = (float)(i + 1);
			T.t0[i] = mulf(s5r, (float)sub32(w, i));
			T.t1[i] = cvtzs(mulf(s2, s7) + 0.5f);
			T.t2[i] = cvtzs(mulf(s3, s7) + 0.5f);
			T.t3[i] = cvtzs(mulf(s4, s7) + 0.5f);
		} else if ((uint32_t)i == (uint32_t)w) {
			T.t0[i] = 0.0f; T.t1[i] = c1; T.t2[i] = c2; T.t3[i] = c3;
		}                                                                    // i > w tidak terjadi untuk n = w+1
		ride_clamp(T, i);
	}
	return true;
}

// campuran satu piksel: kanal' = (uint8)( tabel_warna[idx] + trunc(t0[idx]*kanal + 0.5) )  (low-byte, tanpa clamp)
inline void ride_blend(uint8_t *px, const RideTables &T, size_t idx)
{
	const float t0 = T.t0[idx];
	const int cB = T.t3[idx], cG = T.t2[idx], cR = T.t1[idx];
	const float b = mulf(t0, (float)px[0]) + 0.5f;
	const float g = mulf(t0, (float)px[1]) + 0.5f;
	const float r = mulf(t0, (float)px[2]) + 0.5f;
	px[0] = (uint8_t)add32(cB, cvtzs(b));
	px[1] = (uint8_t)add32(cG, cvtzs(g));
	px[2] = (uint8_t)add32(cR, cvtzs(r));
}

// MaskImageRideRect (dua sisi). Piksel diproses bila alfa tujuan != 0 dan alfa mask m:
//   lo <= m <= a, atau a < m < hi  (m == hi dilewati). idx = m-(a-l).
inline void ride2_apply(uint8_t *dst, ptrdiff_t dpitch, const uint8_t *src, ptrdiff_t spitch, int w, int h,
                        int a, int l, int r, const RideTables &T)
{
	int hi = add32(a, r); if (hi >= 255) hi = 255;
	const int off = sub32(a, l);
	const int lo = off < 0 ? 0 : off;
	for (int y = 0; y < h; ++y, dst += dpitch, src += spitch)
		for (int x = 0; x < w; ++x) {
			uint8_t *p = dst + (ptrdiff_t)x * 4;
			if (p[3] == 0) continue;
			const int m = src[(ptrdiff_t)x * 4 + 3];
			if (lo > m) continue;
			if (m > a && hi <= m) continue;
			const int64_t idx = (int64_t)m - off;
			if (idx < 0 || idx >= (int64_t)T.t0.size()) continue;           // di luar tabel = UB di aslinya
			ride_blend(p, T, (size_t)idx);
		}
}

// MaskImageRideRect2 (satu sisi): m > a -> isi warna penuh (low-byte c3,c2,c1 ke B,G,R); lo <= m <= a -> campuran.
inline void ride1_apply(uint8_t *dst, ptrdiff_t dpitch, const uint8_t *src, ptrdiff_t spitch, int w, int h,
                        int a, int wd, int c1, int c2, int c3, const RideTables &T)
{
	const int off = sub32(a, wd);
	const int lo = off < 0 ? 0 : off;
	for (int y = 0; y < h; ++y, dst += dpitch, src += spitch)
		for (int x = 0; x < w; ++x) {
			uint8_t *p = dst + (ptrdiff_t)x * 4;
			if (p[3] == 0) continue;
			const int m = src[(ptrdiff_t)x * 4 + 3];
			if (lo > m) continue;
			if (m > a) { p[0] = (uint8_t)c3; p[1] = (uint8_t)c2; p[2] = (uint8_t)c1; continue; }
			const int64_t idx = (int64_t)m - off;
			if (idx < 0 || idx >= (int64_t)T.t0.size()) continue;
			ride_blend(p, T, (size_t)idx);
		}
}

// MaskImageRideRect3 (satu sisi, kebalikan RideRect2): n = w+1. Semua float32.
//   i == 0 : T0=0, T1..3 = c1..c3 ; i > 0 : T0 = (1/n)*i ; Tk = floor(ck/n * (n-i) + 0.5)
inline bool ride3_tables(int w, int c1, int c2, int c3, RideTables &T)
{
	T = RideTables();
	const int n = add32(w, 1);
	if (n < 0 || n > kMaxTable) return false;
	if (n == 0) return true;
	T.t0.assign((size_t)n, 0.0f); T.t1.assign((size_t)n, 0); T.t2.assign((size_t)n, 0); T.t3.assign((size_t)n, 0);
	const float s4 = (float)n;
	const float s1 = 1.0f / s4, s2 = (float)c1 / s4, s3 = (float)c2 / s4, s4c = (float)c3 / s4;
	for (int i = 0; i < n; ++i) {
		if (i > 0) {
			const float s7 = (float)(n - i);
			T.t0[i] = mulf(s1, (float)i);
			T.t1[i] = cvtzs(mulf(s2, s7) + 0.5f);
			T.t2[i] = cvtzs(mulf(s3, s7) + 0.5f);
			T.t3[i] = cvtzs(mulf(s4c, s7) + 0.5f);
		} else {
			T.t0[0] = 0.0f; T.t1[0] = c1; T.t2[0] = c2; T.t3[0] = c3;
		}
		ride_clamp(T, i);
	}
	return true;
}

// MaskImageRideRect3: alfa tujuan != 0; m = alfa sumber.
//   m < a  -> warna penuh (low-byte c3,c2,c1 ke B,G,R) ; m >= a dan m <= hi=min(a+w,255) -> campuran idx = m-a ; m > hi -> lewati.
inline void ride3_apply(uint8_t *dst, ptrdiff_t dpitch, const uint8_t *src, ptrdiff_t spitch, int w, int h,
                        int a, int wd, int c1, int c2, int c3, const RideTables &T)
{
	int hi = add32(wd, a); if (hi >= 255) hi = 255;
	for (int y = 0; y < h; ++y, dst += dpitch, src += spitch)
		for (int x = 0; x < w; ++x) {
			uint8_t *p = dst + (ptrdiff_t)x * 4;
			if (p[3] == 0) continue;
			const int m = src[(ptrdiff_t)x * 4 + 3];
			if (m < a) { p[0] = (uint8_t)c3; p[1] = (uint8_t)c2; p[2] = (uint8_t)c1; continue; }
			if (hi < m) continue;
			const int64_t idx = (int64_t)m - a;
			if (idx < 0 || idx >= (int64_t)T.t0.size()) continue;
			ride_blend(p, T, (size_t)idx);
		}
}


// ============================================================================================================================
// Alpha-3 (S8): MaskImageRideRect4, MaskImageRideRect5
// ============================================================================================================================
// ---- ambang alfa layer kedua (`a` = ambang, `n` = lebar pita) ------------------------------------------------------
// Rect4 @0x5d92e4..0x5d9304, Rect5 @0x5d9f5c..0x5d9f7c (identik). Aritmatika int32 melingkar seperti di binary.
struct Band { int lo, hi; };
static inline Band band(int a, int n)
{
	const int32_t amn = (int32_t)((uint32_t)a - (uint32_t)n);                      // a - n
	int32_t lo = (amn < 0) ? 0 : (int32_t)((uint32_t)amn + 1u);                    // a - n < 0 ? 0 : a - n + 1
	if (!(lo < 255)) lo = 255;                                                    // min(lo, 255)
	int32_t hi = (int32_t)((uint32_t)n + (uint32_t)a);                             // n + a, negatif -> 0 (bic ... asr #31)
	if (hi < 0) hi = 0;
	if (!(hi < 255)) hi = 255;                                                    // min(hi, 255)
	Band b; b.lo = lo; b.hi = hi; return b;
}

// =====================================================================================================================
// MaskImageRideRect4  (0x5d8f94)
// =====================================================================================================================
// Tabel (0x5d9164..0x5d92e0), m = 2n-1 entri, indeks i = 0..m-1, jarak dari pusat d = i-(n-1):
//   vf[i]  : bobot float. i<n-1 : (1/n)*(n-1-i) ; i==n-1 : 0 ; i>n-1 : (1/n)*(i-n+1). Lalu klem: >=1 -> 1, !(>0) -> 0.
//   iK[i]  : warna c_K ber-skala. t = i+1 (i<n-1) atau m-i (i>n-1): trunc( (c_K/n)*t + 0.5f ), semuanya float.
//            i==n-1 : c_K mentah. Lalu klem int: >=255 -> 255, <=0 -> 0.
//   Pemetaan kanal piksel (BGRA, indeks byte): byte2 <- c0 (i1), byte1 <- c1 (i2), byte0 <- c2 (i3).
struct Table4 { std::vector<float> vf; std::vector<int> i1, i2, i3; };

static inline void build_table4(int n, int c0, int c1, int c2, Table4 &t)          // prasyarat: 1 <= n <= 0x3FFFFFFF
{
	const size_t m = (size_t)(2 * (int64_t)n - 1);
	t.vf.assign(m, 0.0f); t.i1.assign(m, 0); t.i2.assign(m, 0); t.i3.assign(m, 0);
	const float fn = (float)n;
	const float s5 = 1.0f / fn;                                                    // fdiv s5, 1.0, n
	const float s2 = (float)c0 / fn, s3 = (float)c1 / fn, s4 = (float)c2 / fn;     // fdiv: c/n (float)
	for (int64_t i = 0; i < (int64_t)m; ++i) {
		float v; int a1, a2, a3;
		if (i == n - 1) {                                                           // pusat (0x5d91d4..0x5d91f8)
			v = 0.0f; a1 = c0; a2 = c1; a3 = c2;
		} else {
			int tt; float w;
			if (i < n - 1) { w = (float)(int)(n - 1 - i); tt = (int)(i + 1); }
			else           { w = (float)(int)(i - n + 1); tt = (int)((int64_t)m - i); }
			v = s5 * w;                                                             // fmul s6, s5, s6
			const float ft = (float)tt;
			float p1 = s2 * ft; p1 = p1 + 0.5f; a1 = cvtzs(p1);                    // fmul ; fadd 0.5 ; fcvtzs
			float p2 = s3 * ft; p2 = p2 + 0.5f; a2 = cvtzs(p2);
			float p3 = s4 * ft; p3 = p3 + 0.5f; a3 = cvtzs(p3);
			if (!(v < 1.0f)) v = 1.0f;                                              // fcmp v,1.0 ; b.lt skip ; v = 1.0
		}
		if (a1 >= 255) { a1 = 255; }
		if (a2 >= 255) { a2 = 255; }
		if (a3 >= 255) { a3 = 255; }
		if (!(v > 0.0f)) v = 0.0f;                                                  // fcmp v,#0 ; b.hi skip ; v = 0
		if (a1 <= 0) { a1 = 0; }
		if (a2 <= 0) { a2 = 0; }
		if (a3 <= 0) { a3 = 0; }
		t.vf[(size_t)i] = v; t.i1[(size_t)i] = a1; t.i2[(size_t)i] = a2; t.i3[(size_t)i] = a3;
	}
}

// dst = layer `this` (buffer ctx), l1 = layer pertama, l2 = layer kedua (hanya alfanya yang dibaca sebagai penentu).
// Loop (0x5d9310..0x5d94d4): baris 0..h-1, kolom 0..w-1.
//   lewati bila dst.alfa == 0 && l1.alfa == 0
//   a2 = l2.alfa
//   a2 < lo            : dst.rgb = l1.rgb                                           (alfa dst tidak berubah)
//   lo <= a2 < a       : idx = a2-lo ; dst.k = (uint8)( iK[idx] + trunc_d( (double)l1.k * (double)vf[idx] + 0.5 ) )   -- DOUBLE
//   a <= a2 < hi       : idx = a2-lo ; dst.k = (uint8)( iK[idx] + trunc_f( vf[idx] * (float)dst.k + 0.5f ) )          -- FLOAT
//   a2 >= hi           : tidak berubah
static inline void ride4(uint8_t *dst, ptrdiff_t dpitch, const uint8_t *l1, ptrdiff_t p1, const uint8_t *l2, ptrdiff_t p2,
                         int w, int h, int a, int n, int c0, int c1, int c2)
{
	Table4 t; build_table4(n, c0, c1, c2, t);
	const Band b = band(a, n);
	const uint32_t m = (uint32_t)(2 * (int64_t)n - 1);
	const int *I[3] = { t.i3.data(), t.i2.data(), t.i1.data() };                    // indeks byte 0,1,2
	for (int y = 0; y < h; ++y) {
		uint8_t *d = dst + (ptrdiff_t)y * dpitch;
		const uint8_t *s1 = l1 + (ptrdiff_t)y * p1, *s2 = l2 + (ptrdiff_t)y * p2;
		for (int x = 0; x < w; ++x, d += 4, s1 += 4, s2 += 4) {
			if (d[3] == 0 && s1[3] == 0) continue;
			const int a2 = s2[3];
			if (b.lo > a2) { d[0] = s1[0]; d[1] = s1[1]; d[2] = s1[2]; continue; }
			const uint32_t idx = (uint32_t)a2 - (uint32_t)b.lo;
			if (a2 < a) {
				if (idx >= m) continue;                                              // pengaman (hanya a ~ +-2^31; binary: baca di luar tabel)
				const double vf = (double)t.vf[idx];
				for (int k = 0; k < 3; ++k) {
					double r = (double)s1[k] * vf; r = r + 0.5;
					d[k] = (uint8_t)((uint32_t)I[k][idx] + (uint32_t)cvtzs(r));
				}
				continue;
			}
			if (b.hi <= a2) continue;
			if (idx >= m) continue;
			const float vf = t.vf[idx];
			for (int k = 0; k < 3; ++k) {
				float r = vf * (float)d[k]; r = r + 0.5f;
				d[k] = (uint8_t)((uint32_t)I[k][idx] + (uint32_t)cvtzs(r));
			}
		}
	}
}

// =====================================================================================================================
// MaskImageRideRect5  (0x5d9cb8)
// =====================================================================================================================
// Tabel (0x5d9e64..0x5d9f58), m = 2n-1 entri, tiga vektor float, s1 = 1.0f/n:
//   i < n-1  : B = s1*(n-1-i) ; C = 1.0f - B ; A = 1.0f
//   i == n-1 : B = 0 ; C = 1 ; A = 1
//   i > n-1  : B = 0 ; C = 1 ; A = s1*(m-i)
//   klem tiap vektor: >=1 -> 1, !(>0) -> 0 (urutan B, C, A)
struct Table5 { std::vector<float> A, B, C; };

static inline void build_table5(int n, Table5 &t)                                   // prasyarat: 1 <= n <= 0x3FFFFFFF
{
	const size_t m = (size_t)(2 * (int64_t)n - 1);
	t.A.assign(m, 0.0f); t.B.assign(m, 0.0f); t.C.assign(m, 0.0f);
	const float s1 = 1.0f / (float)n;
	for (int64_t i = 0; i < (int64_t)m; ++i) {
		float A, B, C;
		if (i < n - 1)       { const float w = (float)(int)(n - 1 - i); B = s1 * w; C = 1.0f - B; A = 1.0f; }
		else if (i == n - 1) { B = 0.0f; C = 1.0f; A = 1.0f; }
		else                 { B = 0.0f; C = 1.0f; const float w = (float)(int)((int64_t)m - i); A = s1 * w; }
		if (!(B < 1.0f)) { B = 1.0f; }
		if (!(C < 1.0f)) { C = 1.0f; }
		if (!(A < 1.0f)) { A = 1.0f; }
		if (!(B > 0.0f)) { B = 0.0f; }
		if (!(C > 0.0f)) { C = 0.0f; }
		if (!(A > 0.0f)) { A = 0.0f; }
		t.A[(size_t)i] = A; t.B[(size_t)i] = B; t.C[(size_t)i] = C;
	}
}

// Loop (0x5d9f88..0x5da114): baris 0..h-1, kolom 0..xcount-1.
//   !! ANOMALI BINARY: batas kolom = 2n-1 (ukuran tabel), BUKAN lebar layer (lihat spesifikasi). Pemanggil menentukan `xcount`.
//   lewati bila dst.alfa == 0 && l1.alfa == 0 ; a2 = l2.alfa
//   a2 < lo            : tidak berubah
//   lo <= a2 < a       : idx = a2-lo ; dst.k = (uint8)( trunc_f(B[idx]*(float)dst.k + 0.5f) + trunc_f(C[idx]*(float)l1.k + 0.5f) )   (k = 0..2; alfa tetap)
//   a2 >= hi           : dst.alfa = 0
//   a <= a2 < hi       : idx = a2-lo ; dst.rgb = l1.rgb ; dst.alfa = (uint8) trunc_f( A[idx]*(float)l1.alfa + 0.5f )
static inline void ride5(uint8_t *dst, ptrdiff_t dpitch, const uint8_t *l1, ptrdiff_t p1, const uint8_t *l2, ptrdiff_t p2,
                         int xcount, int h, int a, int n)
{
	Table5 t; build_table5(n, t);
	const Band b = band(a, n);
	const uint32_t m = (uint32_t)(2 * (int64_t)n - 1);
	for (int y = 0; y < h; ++y) {
		uint8_t *d = dst + (ptrdiff_t)y * dpitch;
		const uint8_t *s1 = l1 + (ptrdiff_t)y * p1, *s2 = l2 + (ptrdiff_t)y * p2;
		for (int x = 0; x < xcount; ++x, d += 4, s1 += 4, s2 += 4) {
			if (d[3] == 0 && s1[3] == 0) continue;
			const int a2 = s2[3];
			if (b.lo > a2) continue;
			const uint32_t idx = (uint32_t)a2 - (uint32_t)b.lo;
			if (a2 < a) {
				if (idx >= m) continue;                                              // pengaman (lihat ride4)
				const float vb = t.B[idx], vc = t.C[idx];
				for (int k = 0; k < 3; ++k) {
					float r1 = vc * (float)s1[k]; r1 = r1 + 0.5f;
					float r2 = vb * (float)d[k];  r2 = r2 + 0.5f;
					d[k] = (uint8_t)((uint32_t)cvtzs(r2) + (uint32_t)cvtzs(r1));
				}
				continue;
			}
			if (b.hi <= a2) { d[3] = 0; continue; }
			if (idx >= m) continue;
			d[0] = s1[0]; d[1] = s1[1]; d[2] = s1[2];
			float r = t.A[idx] * (float)s1[3]; r = r + 0.5f;
			d[3] = (uint8_t)cvtzs(r);
		}
	}
}


// ============================================================================================================================
// Alpha-4 (S9): MaskImageRideRect7, MaskImageRideRect8, AlphaBorderInnerRect
// ============================================================================================================================
//---------------------------------------------------------------------------
// Tabel bobot segitiga untuk MaskImageRideRect7.
// Binary membuat vector<float> berukuran n = 2*b-1 (aritmetika 32-bit, lalu sxtw):
//   i <  b-1 : v = (float)(i+1) * (1.0f/(float)b)
//   i == b-1 : v = 1.0f                         (puncak; tanpa perkalian)
//   i >  b-1 : v = (float)(n-i) * (1.0f/(float)b)
//   v = min(v, 1.0f); bila v <= 0 maka v = 0 (fcsel pada "ls").
// Hanya indeks 0..254 yang pernah dibaca (lihat ride7), jadi tabel dipotong ke kRideTableMax entri tanpa mengubah hasil.
// DEVIASI: binary mengalokasikan seluruh n entri (b >= 2^30 -> n negatif -> std::length_error; b <= 0 -> n <= -1 -> length_error).
//   Di sini b < 1 atau b >= 2^30 -> mengembalikan false (pemanggil melempar TJS_E_INVALIDPARAM).
static const int kRideTableMax = 256;

inline bool ride_table(int b, std::vector<float> &t)
{
	if (b < 1 || b >= (1 << 30)) return false;
	const int64_t n = (int64_t)2 * b - 1;
	const int64_t cnt = n < kRideTableMax ? n : kRideTableMax;
	t.assign((size_t)cnt, 0.0f);
	const float inv = 1.0f / (float)b;                 // fdiv s1 = 1.0 / (float)b
	for (int64_t i = 0; i < cnt; i++) {
		float v;
		if (i < (int64_t)b - 1)       { float x = (float)(int)(i + 1);     v = inv * x; }
		else if (i == (int64_t)b - 1) { v = 1.0f; }
		else                          { float x = (float)(int)(n - i);     v = inv * x; }
		if (v > 1.0f) v = 1.0f;                        // fmin(v, 1.0)
		if (!(v > 0.0f)) v = 0.0f;                     // fcmp #0 ; fcsel (ls: <= atau tak terurut)
		t[(size_t)i] = v;
	}
	return true;
}

// Ambang untuk MaskImageRideRect7 (a = param ke-3, b = param ke-4):
//   lo = (a-b < 0) ? 0 : min(a-b+1, 255)      hi = min(max(a+b, 0), 255)       (int 32-bit; penjumlahan melingkar)
struct RideRange { int lo, hi; };
inline RideRange ride_range(int a, int b)
{
	const int32_t d = (int32_t)((uint32_t)a - (uint32_t)b);
	const int32_t s = (int32_t)((uint32_t)a + (uint32_t)b);
	RideRange r;
	int32_t lo = (d < 0) ? 0 : (int32_t)((uint32_t)d + 1u);
	r.lo = lo < 255 ? lo : 255;
	int32_t hi = s < 0 ? 0 : s;
	r.hi = hi < 255 ? hi : 255;
	return r;
}

//---------------------------------------------------------------------------
// MaskImageRideRect7: dst (this), img (layer arg 1: warna+alfa), mask (layer arg 2: hanya alfa dipakai).
// Per piksel; hanya bila alfa dst != 0 ATAU alfa img != 0:
//   m = mask.alpha
//   m <  lo        : tidak berubah
//   m >= hi        : dst.alpha = 0 (warna dst tetap)
//   lo <= m < hi   : dst.BGR = img.BGR ; dst.alpha = trunc( tab[m-lo] * (float)img.alpha + 0.5f )
// Pembandingan m terhadap lo/hi bertanda (m 0..255). tab dari ride_table(b); b harus valid (ride_table true).
// Perkalian dan penjumlahan float dipisah (binary: fmul lalu fadd, bukan fmadd) -> jangan digabung kompilator.
inline void ride7(uint8_t *dst, ptrdiff_t dpitch,
                  const uint8_t *img, ptrdiff_t ipitch,
                  const uint8_t *mask, ptrdiff_t mpitch,
                  int w, int h, int a, int b, const std::vector<float> &tab)
{
	const RideRange r = ride_range(a, b);
	for (int y = 0; y < h; y++) {
		uint8_t *d = dst + (ptrdiff_t)y * dpitch;
		const uint8_t *s = img + (ptrdiff_t)y * ipitch;
		const uint8_t *m = mask + (ptrdiff_t)y * mpitch;
		for (int x = 0; x < w; x++, d += 4, s += 4, m += 4) {
			if (d[3] == 0 && s[3] == 0) continue;
			const int mv = m[3];
			if (r.lo > mv) continue;
			if (r.hi <= mv) { d[3] = 0; continue; }
			d[0] = s[0]; d[1] = s[1]; d[2] = s[2];
			// DEVIASI: a dengan |a| mendekati 2^31 membuat lo/hi melingkar -> binary membaca di luar tabel; di sini bobot 0.
			const int64_t idx = (int64_t)mv - (int64_t)r.lo;
			const float t = (idx >= 0 && idx < (int64_t)tab.size()) ? tab[(size_t)idx] : 0.0f;
			volatile float prod = t * (float)s[3];     // volatile: cegah fusi fmadd (binary: fmul, fadd terpisah)
			float sum = prod + 0.5f;
			d[3] = (uint8_t)(int)sum;                  // fcvtzs lalu strb (byte rendah)
		}
	}
}

//---------------------------------------------------------------------------
// MaskImageRideRect8: dst (this), img (arg 1), mask (arg 2), th (param ke-3).
// Per piksel; hanya bila alfa dst != 0 ATAU alfa img != 0:
//   bila mask.alpha > th (bertanda): dst = img (keempat byte, termasuk alfa); selain itu tidak berubah.
inline void ride8(uint8_t *dst, ptrdiff_t dpitch,
                  const uint8_t *img, ptrdiff_t ipitch,
                  const uint8_t *mask, ptrdiff_t mpitch,
                  int w, int h, int th)
{
	for (int y = 0; y < h; y++) {
		uint8_t *d = dst + (ptrdiff_t)y * dpitch;
		const uint8_t *s = img + (ptrdiff_t)y * ipitch;
		const uint8_t *m = mask + (ptrdiff_t)y * mpitch;
		for (int x = 0; x < w; x++, d += 4, s += 4, m += 4) {
			if (d[3] == 0 && s[3] == 0) continue;
			if ((int)m[3] <= th) continue;
			d[0] = s[0]; d[1] = s[1]; d[2] = s[2]; d[3] = s[3];
		}
	}
}

//---------------------------------------------------------------------------
// AlphaBorderInnerRect: dst (this), mask (arg 1), th (param ke-2).
// Per piksel: bila mask.alpha >= th (bertanda) maka dst.alpha = 0 (warna tetap). Tanpa syarat alfa dst/mask lain.
inline void border_inner(uint8_t *dst, ptrdiff_t dpitch,
                         const uint8_t *mask, ptrdiff_t mpitch,
                         int w, int h, int th)
{
	for (int y = 0; y < h; y++) {
		uint8_t *d = dst + (ptrdiff_t)y * dpitch;
		const uint8_t *m = mask + (ptrdiff_t)y * mpitch;
		for (int x = 0; x < w; x++, d += 4, m += 4)
			if ((int)m[3] >= th) d[3] = 0;
	}
}


// ============================================================================================
// S10: MaskImageEffectRect, AlphaBorderImageCoverRect
// ============================================================================================
// ---- MaskImageEffectRect(src, a, l, r, c1, c2, c3) -------------------------------------------
struct EffectTables {
	std::vector<float>    t0;              // bobot piksel tujuan
	std::vector<uint32_t> ta, tr, tg, tb;  // alfa baru, dan warna R,G,B (unsigned, dijepit 0..255)
};

// n = l+r+1 (int32). n < 0 -> false (aslinya crash); n == 0 -> tabel kosong.
inline bool effect_tables(int l, int r, int c1, int c2, int c3, EffectTables &T)
{
	T = EffectTables();
	const int n = add32(add32(l, r), 1);
	if (n < 0 || n > kMaxTable) return false;
	if (n == 0) return true;
	T.t0.assign((size_t)n, 0.0f);
	T.ta.assign((size_t)n, 0); T.tr.assign((size_t)n, 0); T.tg.assign((size_t)n, 0); T.tb.assign((size_t)n, 0);
	const int L1 = add32(l, 1), R1 = add32(r, 1);
	const float fL1 = (float)L1, fR1 = (float)R1;
	const float f1 = (float)c1, f2 = (float)c2, f3 = (float)c3;
	const int   c1q = sdiv32(c1, L1);                 // R memakai pembagian INTEGER (perilaku biner)
	const float s2 = f2 / fL1, s3 = f3 / fL1;
	const float s4 = f1 / fR1, s5 = f2 / fR1, s6 = f3 / fR1, s7 = 1.0f / fR1;
	const float s16 = 255.0f / fL1, s17 = 1.0f / fL1;
	for (int i = 0; i < n; ++i) {
		uint32_t ta, tr, tg, tb; float t0;
		if ((int64_t)i < (int64_t)l) {
			const float k = (float)(i + 1);
			ta = cvtzu(mulf(s16, k) + 0.5f);
			tr = cvtzu((float)(int)((uint32_t)c1q * (uint32_t)(i + 1)) + 0.5f);
			tg = cvtzu(mulf(s2, k) + 0.5f);
			tb = cvtzu(mulf(s3, k) + 0.5f);
			t0 = mulf(s17, (float)sub32(l, i));
		} else if ((uint32_t)i == (uint32_t)l) {
			ta = 255; tr = (uint32_t)c1; tg = (uint32_t)c2; tb = (uint32_t)c3; t0 = 0.0f;
		} else {
			const float k = (float)add32(add32(r, sub32(l, i)), 1);     // n - i
			tr = cvtzu(mulf(s4, k) + 0.5f);
			tg = cvtzu(mulf(s5, k) + 0.5f);
			tb = cvtzu(mulf(s6, k) + 0.5f);
			ta = 255;
			t0 = mulf(s7, (float)sub32(i, l));
		}
		if (t0 >= 1.0f) t0 = 1.0f;
		if (ta >= 255u) ta = 255u;
		if (tr >= 255u) tr = 255u;
		if (tg >= 255u) tg = 255u;
		if (tb >= 255u) tb = 255u;
		if (t0 <= 0.0f) t0 = 0.0f;
		T.t0[(size_t)i] = t0; T.ta[(size_t)i] = ta; T.tr[(size_t)i] = tr; T.tg[(size_t)i] = tg; T.tb[(size_t)i] = tb;
	}
	return true;
}

inline void effect_blend(uint8_t *px, const EffectTables &T, size_t idx)
{
	const float t0 = T.t0[idx];
	const float b = mulf(t0, (float)px[0]) + 0.5f;
	const float g = mulf(t0, (float)px[1]) + 0.5f;
	const float r = mulf(t0, (float)px[2]) + 0.5f;
	px[0] = (uint8_t)((uint32_t)T.tb[idx] + (uint32_t)cvtzs(b));
	px[1] = (uint8_t)((uint32_t)T.tg[idx] + (uint32_t)cvtzs(g));
	px[2] = (uint8_t)((uint32_t)T.tr[idx] + (uint32_t)cvtzs(r));
}

// Per piksel tujuan (alfa != 0), m = alfa mask sumber, lo = max(0,a-l), off = a-l, hi = min(a+r,255):
//   m < lo          -> alfa tujuan = 0
//   lo <= m <= a    -> RGB dicampur dengan tabel, alfa = ta[m-off]
//   a < m < hi      -> RGB dicampur dengan tabel, alfa tetap
//   m >= hi (m > a) -> tidak berubah
inline void effect_apply(uint8_t *dst, ptrdiff_t dpitch, const uint8_t *src, ptrdiff_t spitch, int w, int h,
                         int a, int l, int r, const EffectTables &T)
{
	int hi = add32(a, r); if (hi >= 255) hi = 255;
	const int off = sub32(a, l);
	const int lo = off < 0 ? 0 : off;
	for (int y = 0; y < h; ++y, dst += dpitch, src += spitch)
		for (int x = 0; x < w; ++x) {
			uint8_t *p = dst + (ptrdiff_t)x * 4;
			if (p[3] == 0) continue;
			const int m = src[(ptrdiff_t)x * 4 + 3];
			if (lo > m) { p[3] = 0; continue; }
			const bool left = (m <= a);
			if (!left && hi <= m) continue;
			const int64_t idx = (int64_t)m - off;
			if (idx < 0 || idx >= (int64_t)T.t0.size()) continue;
			effect_blend(p, T, (size_t)idx);
			if (left) p[3] = (uint8_t)T.ta[(size_t)idx];
		}
}

// ---- AlphaBorderImageCoverRect(A, B, C, p, q) ------------------------------------------------
// Jendela [p-L, p+R) atas alfa C. false -> "Out of range" (q < 1, q+p < 1, atau p-q > 254).
inline bool cover_window(int p, int q, int &L, int &R)
{
	if (q < 1) return false;
	const int qp = add32(q, p);
	if (qp < 1) return false;
	if (sub32(p, q) > 0xfe) return false;
	int w8, w9 = qp;
	bool via_c8 = true;
	if (p < 0) { w8 = 0; }
	else if (p >= q) {
		if (p >= 256) { w8 = sub32(add32(p, q), 255); w9 = 0; via_c8 = false; }
		else w8 = q;
	} else {
		w8 = p;
		if (p > 255) { w9 = 0; via_c8 = false; }
	}
	if (via_c8) w9 = (w9 < 256) ? q : add32(sub32(255, p), q);
	L = w8 < 0 ? 0 : w8;
	R = w9 < 0 ? 0 : w9;
	return true;
}

// T[0..n], n = L+R (n+1 entri). false bila terlalu besar.
inline bool cover_table(int q, int L, int R, std::vector<float> &T)
{
	T.clear();
	const int n = add32(L, R);
	if (n < 0 || (int64_t)n + 1 > kMaxTable) return false;
	T.assign((size_t)n + 1, 0.0f);
	const float s1 = 1.0f / (float)q;
	const int base = sub32(q, L);                       // w9
	const int top  = add32(add32(q, L), 1);             // w13 awal
	for (int i = 0; i <= n; ++i) {
		float v;
		if (i < L)       v = mulf(s1, (float)add32(base, i));
		else if (i == L) v = 1.0f;
		else             v = mulf(s1, (float)sub32(top, i));
		if (v > 1.0f) v = 1.0f;                         // fmin(v, 1)
		if (v <= 0.0f) v = 0.0f;                        // fcsel ... ls
		T[(size_t)i] = v;
	}
	return true;
}

// Piksel diproses bila (alfa tujuan != 0 atau alfa A != 0) dan p-L <= mC < p+R (mC = alfa C):
//   tujuan.RGB = A.RGB ; tujuan.alfa = (uint8) trunc(T[mC-(p-L)] * alfaB + 0.5)
inline void cover_apply(uint8_t *dst, ptrdiff_t dpitch,
                        const uint8_t *A, ptrdiff_t apitch, const uint8_t *B, ptrdiff_t bpitch,
                        const uint8_t *C, ptrdiff_t cpitch, int w, int h,
                        int p, int L, int R, const std::vector<float> &T)
{
	const int lowv = sub32(p, L), highv = add32(R, p);
	for (int y = 0; y < h; ++y, dst += dpitch, A += apitch, B += bpitch, C += cpitch)
		for (int x = 0; x < w; ++x) {
			uint8_t *d = dst + (ptrdiff_t)x * 4;
			const uint8_t *a = A + (ptrdiff_t)x * 4;
			if (d[3] == 0 && a[3] == 0) continue;
			const int m = C[(ptrdiff_t)x * 4 + 3];
			if (lowv > m) continue;
			if (highv <= m) continue;
			const int64_t idx = (int64_t)m - lowv;
			if (idx < 0 || idx >= (int64_t)T.size()) continue;
			d[0] = a[0]; d[1] = a[1]; d[2] = a[2];
			const float v = mulf(T[(size_t)idx], (float)B[(ptrdiff_t)x * 4 + 3]) + 0.5f;
			d[3] = (uint8_t)cvtzs(v);
		}
}


} // namespace layerex_alpha

#endif
