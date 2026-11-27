// layerExShimmerCore.hpp - inti piksel Layer.shimmer / Layer.shimmerBuildMap (modul layerExShimmer.dll)
// Direkonstruksi dari libgame.so arm64 1.3.9: shimmer @0x5eedb8 (4.696 B), shimmerBuildMap @0x5f0010 (568 B),
// dibaca dari recon_shimmer.txt. Spesifikasi: docs/spec/layerExShimmer_*.md. Tanpa dependensi (hanya <stdint.h>).
//
// Label keyakinan:  [BIN] dibaca langsung dari disassembly  ·  [ASUMSI] bagian yang TIDAK terbaca (lihat spesifikasi).
//   [ASUMSI-INT]  interior (W-2)x(H-2) dikerjakan worker thread 0x5f0494/0x5f0630 yang belum di-dump -> diasumsikan
//                 selisih terpusat pada kedua sumbu (sama dengan yang dipakai baris/kolom tepi). Hasil tidak bergantung pada
//                 jumlah thread (baris tak saling bergantung).
//   Beda perilaku "apa adanya" yang SENGAJA dipertahankan (bisa bug asli, putuskan di uji diferensial 7.3):
//     [BIN-L1] loop baris atas/bawah berhenti satu piksel lebih awal: kolom relatif W-2 pada baris 0 dan H-1 TIDAK ditulis.
//     [BIN-L2] loop kolom kiri/kanan: baris relatif H-2 tidak ditulis; baris sumber B/C = k+2 sedangkan baris tujuan = k+1.
//     [BIN-L3] loop kolom kiri/kanan menulis ke baris tujuan (1+k) TANPA menambah dy terpotong (kunci SHIMMER_FIX_EDGE_COL_DY).
#ifndef LAYEREX_SHIMMER_CORE_HPP
#define LAYEREX_SHIMMER_CORE_HPP
#include <stdint.h>
#include <stddef.h>
#include <string.h>
#include <math.h>

#ifndef SHIMMER_FIX_EDGE_COL_DY
#define SHIMMER_FIX_EDGE_COL_DY 0   // 0 = persis binary ([BIN-L3]); 1 = tulis ke dy+1+k (perbaikan, BUKAN perilaku 1.3.9)
#endif

namespace layerex_shimmer {

// fcvtzs w, s, #16 : float -> titik-tetap 16.16, pembulatan ke nol, saturasi, NaN -> 0 [BIN @0x5ef474]
static inline int32_t to_fixed16(float f) {
	if (f != f) return 0;
	double v = (double)f * 65536.0;
	if (v >= 2147483647.0) return INT32_MAX;
	if (v <= -2147483648.0) return INT32_MIN;
	return (int32_t)v;                                   // C: pemotongan ke nol
}
// pembagian bertanda 255 yang dipotong ke nol (idiom smull 0x80808081; asr 7; + bit tanda) [BIN @0x5ef508]
static inline int32_t div255(int32_t v) { return v / 255; }
static inline int32_t mulw(int32_t a, int32_t b) { return (int32_t)((uint32_t)a * (uint32_t)b); }   // mul w,w,w melingkar

struct Region { int dx, dy, w, h; };                     // dx,dy sudah terpotong (>=0); w,h terpotong

// Hitung region tulis. false = tidak ada pekerjaan (binary: kembali tanpa menulis) [BIN @0x5ef3b0-0x5ef458]
//   w==0 -> lebar A ; h==0 -> tinggi A ; dx/dy negatif memotong sisi kiri/atas ; dibatasi ke ukuran `this`.
//   Region harus <= ukuran B; jika tabel mask ada, juga <= ukuran C (C void -> cw=ch=0 -> selalu false) [lihat spesifikasi].
static inline bool compute_region(int dx, int dy, int w, int h, int aw, int ah, int tw, int th,
                                  int bw, int bh, bool mask_table, int cw, int ch, Region *out)
{
	int64_t W = (w == 0) ? aw : w, H = (h == 0) ? ah : h;
	const int64_t dxc = dx < 0 ? 0 : dx, dyc = dy < 0 ? 0 : dy;
	int64_t rw = W + (dx < 0 ? dx : 0), rh = H + (dy < 0 ? dy : 0);
	if (rw < 1 || rh < 1) return false;
	if (tw - dxc <= 0 || th - dyc <= 0) return false;
	const int64_t Wc = (rw + dxc > tw) ? tw - dxc : rw;
	const int64_t Hc = (rh + dyc > th) ? th - dyc : rh;
	if (Wc > bw || Hc > bh) return false;
	if (mask_table && (Wc > cw || Hc > ch)) return false;
	out->dx = (int)dxc; out->dy = (int)dyc; out->w = (int)Wc; out->h = (int)Hc;
	return true;
}

struct Ctx {
	uint8_t *dst; ptrdiff_t dpitch; int dw, dh;                 // this (tulis)
	const uint8_t *a; ptrdiff_t apitch; int aw, ah;             // A: gambar yang digeser
	const uint8_t *b; ptrdiff_t bpitch;                         // B: peta (kanal biru byte 0)
	const uint8_t *c; ptrdiff_t cpitch; bool has_mask;          // C: mask (kanal biru); has_mask = argumen C bukan void
	Region r;
	int32_t sxf, syf;                                           // skala 16.16 (to_fixed16 dari argumen float)
};

static inline int bb(const uint8_t *p, ptrdiff_t pitch, int y, int x) { return p[(ptrdiff_t)y * pitch + (ptrdiff_t)x * 4]; }

// Satu piksel keluaran: tujuan(X,Y) = A[clamp(Y+oy)][clamp(X+ox)]   [BIN @0x5ef548-0x5ef5b0]
//   tanpa mask : ox = (gx*sxf)>>16 ; oy = (gy*syf)>>16
//   dengan mask: ox = ((gx*sxf*m)/255)>>16 ; oy = ((m*syf*gy)/255)>>16   (perkalian 32-bit melingkar, bagi dipotong ke nol)
static inline void emit(const Ctx &k, int dst_row, int X, int Y, int gx, int gy, int m)
{
	int32_t ox, oy;
	if (k.has_mask) {
		ox = div255(mulw(mulw(gx, k.sxf), m)) >> 16;
		oy = div255(mulw(mulw(m, k.syf), gy)) >> 16;
	} else {
		ox = mulw(gx, k.sxf) >> 16;
		oy = mulw(gy, k.syf) >> 16;
	}
	int32_t sx = X + ox, sy = Y + oy;
	if (sy < 0) sy = 0; else if (sy >= k.ah) sy = k.ah - 1;
	if (sx < 0) sx = 0; else if (sx >= k.aw) sx = k.aw - 1;
	uint32_t px;
	memcpy(&px, k.a + (ptrdiff_t)sy * k.apitch + (ptrdiff_t)sx * 4, 4);
	memcpy(k.dst + (ptrdiff_t)dst_row * k.dpitch + (ptrdiff_t)X * 4, &px, 4);
}

// Jalankan shimmer pada konteks yang SUDAH tervalidasi (compute_region true; aw,ah>=1).
// Urutan tulis sama dengan binary (penting bila [BIN-L3] membuat area tulis saling timpa): baris atas, baris bawah,
// kolom kiri, kolom kanan, sudut atas, sudut bawah, interior.
static inline void apply(const Ctx &k)
{
	const int W = k.r.w, H = k.r.h, X0 = k.r.dx, Y0 = k.r.dy;
	if (W < 2 || H < 2 || k.aw < 1 || k.ah < 1) return;   // DEVIASI: binary membaca B/C di luar batas untuk W<2 atau H<2
	auto cm = [&](int y, int x) { return k.has_mask ? bb(k.c, k.cpitch, y, x) : 0; };
	const uint8_t *B = k.b; const ptrdiff_t bp = k.bpitch;

	// baris atas (y=0): selisih-x terpusat, selisih-y maju.  x = 1 .. W-3  [BIN-L1]
	for (int x = 1; x < W - 2; ++x)
		emit(k, Y0, X0 + x, Y0, bb(B,bp,0,x+1) - bb(B,bp,0,x-1), bb(B,bp,1,x) - bb(B,bp,0,x), cm(0, x));
	// baris bawah (y=H-1): selisih-y mundur
	for (int x = 1; x < W - 2; ++x)
		emit(k, Y0 + H - 1, X0 + x, Y0 + H - 1, bb(B,bp,H-1,x+1) - bb(B,bp,H-1,x-1), bb(B,bp,H-1,x) - bb(B,bp,H-2,x), cm(H-1, x));
	// kolom kiri/kanan untuk baris tengah, hanya bila H >= 4; H-3 iterasi  [BIN-L2][BIN-L3]
	if (H >= 4) {
		for (int kk = 0; kk <= H - 4; ++kk) {           // kiri
			const int dr = SHIMMER_FIX_EDGE_COL_DY ? Y0 + 1 + kk : 1 + kk;
			emit(k, dr, X0, Y0 + 1 + kk, bb(B,bp,kk+2,1) - bb(B,bp,kk+2,0), bb(B,bp,kk+3,0) - bb(B,bp,kk+1,0), cm(kk+2, 0));
		}
		for (int kk = 0; kk <= H - 4; ++kk) {           // kanan
			const int dr = SHIMMER_FIX_EDGE_COL_DY ? Y0 + 1 + kk : 1 + kk;
			emit(k, dr, X0 + W - 1, Y0 + 1 + kk, bb(B,bp,kk+2,W-1) - bb(B,bp,kk+2,W-2),
			     bb(B,bp,kk+3,W-1) - bb(B,bp,kk+1,W-1), cm(kk+2, W-1));
		}
	}
	// sudut atas: kiri (maju,maju) lalu kanan (mundur,maju)
	emit(k, Y0, X0, Y0, bb(B,bp,0,1) - bb(B,bp,0,0), bb(B,bp,1,0) - bb(B,bp,0,0), cm(0, 0));
	emit(k, Y0, X0 + W - 1, Y0, bb(B,bp,0,W-1) - bb(B,bp,0,W-2), bb(B,bp,1,W-1) - bb(B,bp,0,W-1), cm(0, W-1));
	// sudut bawah: kiri (maju,mundur) lalu kanan (mundur,mundur)
	emit(k, Y0 + H - 1, X0, Y0 + H - 1, bb(B,bp,H-1,1) - bb(B,bp,H-1,0), bb(B,bp,H-1,0) - bb(B,bp,H-2,0), cm(H-1, 0));
	emit(k, Y0 + H - 1, X0 + W - 1, Y0 + H - 1, bb(B,bp,H-1,W-1) - bb(B,bp,H-1,W-2), bb(B,bp,H-1,W-1) - bb(B,bp,H-2,W-1), cm(H-1, W-1));

	// interior [ASUMSI-INT]: W>=3 dan H>=3 ; selisih terpusat pada kedua sumbu
	if (W >= 3 && H >= 3)
		for (int y = 1; y <= H - 2; ++y)
			for (int x = 1; x <= W - 2; ++x)
				emit(k, Y0 + y, X0 + x, Y0 + y, bb(B,bp,y,x+1) - bb(B,bp,y,x-1), bb(B,bp,y+1,x) - bb(B,bp,y-1,x), cm(y, x));
}

// ---------------- shimmerBuildMap (jalur 2 sumber; jalur argumen ke-4 non-void = 0x5f0798 BELUM DIBACA) ----------------
static inline int64_t pmod(int64_t a, int64_t n) { return ((a % n) + n) % n; }   // msub/sdiv dua kali [BIN @0x5f0138-0x5f0188]

// tujuan.byte0(x,y) = sumber.byte0( (y-oy) mod sh , (x-ox) mod sw ). Hanya kanal biru (byte 0) tujuan yang ditulis. [BIN @0x5f0164-0x5f01c8]
static inline void build_map(uint8_t *dst, ptrdiff_t dpitch, int dw, int dh,
                             const uint8_t *src, ptrdiff_t spitch, int sw, int sh, int ox, int oy)
{
	if (dw < 1 || dh < 1 || sw < 1 || sh < 1) return;   // DEVIASI: binary membagi nol (hasil 0) lalu membaca baris -1/di luar batas bila sw/sh = 0
	const int64_t oxm = pmod(ox, sw), oym = pmod(oy, sh);
	for (int y = 0; y < dh; ++y) {
		const uint8_t *srow = src + (ptrdiff_t)pmod((int64_t)y - oym, sh) * spitch;
		uint8_t *drow = dst + (ptrdiff_t)y * dpitch;
		for (int x = 0; x < dw; ++x)
			drow[(ptrdiff_t)x * 4] = srow[(ptrdiff_t)pmod((int64_t)x - oxm, sw) * 4];
	}
}

} // namespace layerex_shimmer
#endif
