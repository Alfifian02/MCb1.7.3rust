// layerExShrinkCore.hpp - inti murni (tanpa dependensi Kirikiri) untuk Layer.shrinkCopy jalur CPU.
// Direkonstruksi dari libgame.so arm64 1.3.9: 0x60069c (clip) dan 0x600908 (loop akumulasi).
// Lihat docs/spec/layerExShrink_shrinkCopy.md.
// STATUS: clip(), pembangun tabel (0x6011e4 + 0x601454) dan accumulate() sudah ada. Belum: shrinkCopyFast (0x60185c).
#pragma once
#include <stdint.h>
#include <stddef.h>

namespace layerex_shrink {

// Struktur konteks di binary (basis = sp+0x38). Hanya field yang dipakai clip().
struct Ctx {
	double dx, dy, dw, dh;          // +0x08..+0x20 : rect tujuan (real)
	int sx, sy, sw, sh;             // +0x50..+0x5c : rect sumber (int)
	int srcW, srcH, dstW, dstH;     // +0x60..+0x6c : ukuran gambar sumber / tujuan
	// keluaran
	int ix, iy, iw, ih;             // +0x28..+0x34 : rect tujuan integer (floor/ceil)
	int clipX, clipY;               // +0x38,+0x3c  : bagian yang terpotong di kiri/atas
	int outW, outH;                 // +0x40,+0x44  : batas loop (x dari clipX sampai outW)
};

static inline int trunc_i(double v) { return (int)v; } // fcvtzs (ke arah nol)

// Port 0x60069c. Mengembalikan false bila tidak ada yang digambar. Mengubah sx/sy/sw/sh/dx/dy/dw/dh.
static inline bool clip(Ctx &c)
{
	if (c.sx + c.sw < 1) return false;
	if (c.sy + c.sh < 1) return false;
	if (c.sx >= c.srcW) return false;
	if (c.sy >= c.srcH) return false;

	const double scx = c.dw / (double)c.sw;
	const double scy = c.dh / (double)c.sh;

	if (c.sx < 0) {
		const double d = scx * (double)(-c.sx);
		c.dw -= d; c.dx += d;
		c.sw += c.sx; c.sx = 0;
	}
	if (c.sy < 0) {
		const double d = scy * (double)(-c.sy);
		c.dh -= d; c.dy += d;
		c.sh += c.sy; c.sy = 0;
	}
	{
		const int over = (c.sx + c.sw) - c.srcW;
		if (over >= 1) { c.dw -= scx * (double)over; c.sw -= over; }
	}
	{
		const int over = (c.sy + c.sh) - c.srcH;
		if (over >= 1) { c.dh -= scy * (double)over; c.sh -= over; }
	}

	c.ix = trunc_i(c.dx);
	c.iy = trunc_i(c.dy);
	const double right  = c.dx + c.dw;
	const double bottom = c.dy + c.dh;
	const int ri = trunc_i(right), bi = trunc_i(bottom);
	c.iw = ri - c.ix;
	c.ih = bi - c.iy;
	if (right  > (double)ri) c.iw += 1;
	if (bottom > (double)bi) c.ih += 1;

	if (c.ix + c.iw < 1) return false;
	if (c.iy + c.ih < 1) return false;
	if (c.dstW - c.ix <= 0) return false;
	if (c.dstH - c.iy <= 0) return false;

	c.clipX = c.ix < 0 ? -c.ix : 0;
	c.clipY = c.iy < 0 ? -c.iy : 0;
	c.outW = (c.ix + c.iw > c.dstW) ? c.dstW - c.ix : c.iw;
	c.outH = (c.iy + c.ih > c.dstH) ? c.dstH - c.iy : c.ih;
	return true;
}

// Entri tabel per sumbu, 28 byte (stride 0x1c di binary).
//  X: off = offset BYTE piksel penuh pertama di dalam baris; piksel parsial kiri ada di off-4,
//     parsial kanan di off+4*n.
//  Y: off = INDEKS baris penuh pertama; baris parsial atas = rows[off-1], bawah = rows[off+n].
//  wa_* = bobot untuk kanal alfa, wc_* = bobot untuk kanal warna (B,G,R). lo = kiri/atas, hi = kanan/bawah.
struct AxisEntry {
	int32_t off;
	int32_t n;
	int32_t wa_lo, wc_lo;
	int32_t wa_hi, wc_hi;
	int32_t div;
};
static_assert(sizeof(AxisEntry) == 28, "AxisEntry harus 28 byte seperti di binary");

// Port loop 0x600ab8..0x601158 (versi skalar; hasil identik dengan NEON karena aritmetika uint32 melingkar).
// dst  : piksel tujuan (x = clipX, y = clipY), dpitch dalam byte
// xe   : outW-clipX entri;  ye: outH-clipY entri
// rows : larik pointer baris sumber (panjang srcH); fx/fy: bobot piksel/baris penuh (hasil pembangun tabel)
// Bobot nol MELEWATI pembacaan (penting: piksel parsial bisa berada di luar gambar).
static inline void accumulate(uint8_t *dst, ptrdiff_t dpitch,
                              const AxisEntry *xe, int nx, const AxisEntry *ye, int ny,
                              const uint8_t *const *rows, uint32_t fx, uint32_t fy)
{
	for (int y = 0; y < ny; ++y, dst += dpitch) {
		const AxisEntry &Y = ye[y];
		const uint8_t *const *rp = rows + Y.off;
		uint8_t *out = dst;
		for (int x = 0; x < nx; ++x, out += 4) {
			const AxisEntry &X = xe[x];
			uint32_t s[4] = {0, 0, 0, 0}; // B,G,R,A
			auto add = [&](const uint8_t *row, ptrdiff_t byteoff, uint32_t wc, uint32_t wa) {
				const uint8_t *p = row + byteoff;
				s[0] += p[0] * wc; s[1] += p[1] * wc; s[2] += p[2] * wc; s[3] += p[3] * wa;
			};
			const ptrdiff_t L = (ptrdiff_t)X.off - 4;
			const ptrdiff_t R = (ptrdiff_t)X.off + 4 * (ptrdiff_t)X.n;
			const uint32_t Xal = X.wa_lo, Xcl = X.wc_lo, Xar = X.wa_hi, Xcr = X.wc_hi;

			// baris parsial atas
			{
				const uint8_t *row = rp[-1];
				const uint32_t Ya = Y.wa_lo, Yc = Y.wc_lo;
				if ((Ya * Xal) != 0) add(row, L, Yc * Xcl, Ya * Xal);
				if (X.n >= 1 && (Ya * fx) != 0) {
					for (int k = 0; k < X.n; ++k) add(row, X.off + 4 * (ptrdiff_t)k, Yc * fx, Ya * fx);
				}
				if ((Ya * Xar) != 0) add(row, R, Yc * Xcr, Ya * Xar);
			}
			// baris penuh
			if (Y.n >= 1) {
				if ((Xal * fy) != 0) for (int k = 0; k < Y.n; ++k) add(rp[k], L, Xcl * fy, Xal * fy);
				if ((fx * fy) != 0 && X.n >= 1)
					for (int k = 0; k < Y.n; ++k)
						for (int j = 0; j < X.n; ++j) add(rp[k], X.off + 4 * (ptrdiff_t)j, fx * fy, fx * fy);
				if ((Xar * fy) != 0) for (int k = 0; k < Y.n; ++k) add(rp[k], R, Xcr * fy, Xar * fy);
			}
			// baris parsial bawah
			{
				const uint8_t *row = rp[Y.n];
				const uint32_t Ya = Y.wa_hi, Yc = Y.wc_hi;
				if ((Ya * Xal) != 0) add(row, L, Yc * Xcl, Ya * Xal);
				if (X.n >= 1 && (Ya * fx) != 0) {
					for (int k = 0; k < X.n; ++k) add(row, X.off + 4 * (ptrdiff_t)k, Yc * fx, Ya * fx);
				}
				if ((Ya * Xar) != 0) add(row, R, Yc * Xcr, Ya * Xar);
			}
			const uint32_t div = (uint32_t)Y.div * (uint32_t)X.div;
			if (div == 0) { out[0] = out[1] = out[2] = out[3] = 0; continue; } // binary: udiv by 0 = 0 (ARM)
			out[0] = (uint8_t)(s[0] / div); out[1] = (uint8_t)(s[1] / div);
			out[2] = (uint8_t)(s[2] / div); out[3] = (uint8_t)(s[3] / div);
		}
	}
}

// ---- Pembangun tabel per sumbu: bagian interior dari 0x6011e4 ----
// Entri pertama dan terakhir dibangun oleh 0x601454 (edge_entry), entri tengah memakai interior_entry.
struct AxisParams {
	double srcLen;  // sw (X) atau sh (Y) sebagai double, sesudah clip()
	double scale;   // sw/dw (X) atau sh/dh (Y), >= 1 pada shrinkCopy
	double frac;    // dx - ix (X) atau dy - iy (Y)
	int    base;    // sx (X) atau sy (Y), sesudah clip()
	int    stride;  // 4 (X, byte per piksel) atau 1 (Y, indeks baris)
	uint32_t unit;  // bobot satu piksel penuh (juga nilai fx/fy yang dikembalikan 0x6011e4)
};

// 0x6012ac..0x6012d0. Konstanta 0.0625 ada di 0x14cbda8. Untuk shrink (scale>=1) hasilnya selalu 256.
static inline uint32_t unit_for_scale(double scale)
{
	if (scale > 0.0625) return 256;
	const int w9 = (int)(0.125 / scale);
	if (w9 <= 0) return 0;
	return ((unsigned)w9 <= 0x100u) ? 256u / (unsigned)w9 : 1u;
}

// 0x601320..0x6013fc: entri untuk piksel tujuan ke-i (i absolut, bukan relatif clipX); hanya untuk i tengah.
static inline AxisEntry interior_entry(const AxisParams &p, int i)
{
	const double t = (double)i - p.frac;
	const double st = t * p.scale;      // awal di sumber (relatif base)
	const double en = p.scale + st;     // akhir
	const int si = (int)st, ei = (int)en;
	const double u = (double)p.unit;
	AxisEntry e;
	e.off = (si + p.base + 1) * p.stride;
	const int lw = (int)p.unit - (int)((st - (double)si) * u);
	const int rw = (int)((en - (double)ei) * u);
	e.n = ei - si - 1;
	e.wa_lo = e.wc_lo = lw;
	e.wa_hi = e.wc_hi = rw;
	e.div = e.n * (int)p.unit + lw + rw;
	return e;
}

// 0x601454: entri untuk indeks i di tepi tabel. Posisi mentah (st,en) dipakai untuk bobot WARNA dan div (piksel di luar
// sumber dianggap memperpanjang piksel tepi), posisi terpotong ke [0,srcLen] dipakai untuk off, n dan bobot ALFA.
static inline AxisEntry edge_entry(const AxisParams &p, int i)
{
	const double st = ((double)i - p.frac) * p.scale;
	const double en = p.scale + st;
	const double stC = (st < 0.0) ? 0.0 : st;
	const double enC = (en > p.srcLen) ? p.srcLen : en;
	const int ST = (int)st, EN = (int)en, STc = (int)stC, ENc = (int)enC;
	const double u = (double)p.unit;
	AxisEntry e;
	e.off = (STc + p.base + 1) * p.stride;
	e.wc_lo = (int)p.unit - (int)((st - (double)ST) * u);
	e.wc_hi = (int)((en - (double)EN) * u);
	e.div = (EN - ST - 1) * (int)p.unit + e.wc_lo + e.wc_hi;
	e.wa_lo = (int)p.unit - (int)((stC - (double)STc) * u);
	e.wa_hi = (int)((enC - (double)ENc) * u);
	e.n = ENc - STc - 1;
	return e;
}

// Parameter sumbu dari konteks setelah clip(). X: isX=true.
static inline AxisParams make_axis_params(const Ctx &c, bool isX)
{
	AxisParams p;
	p.srcLen = (double)(isX ? c.sw : c.sh);
	p.scale  = p.srcLen / (isX ? c.dw : c.dh);
	p.frac   = isX ? (c.dx - (double)c.ix) : (c.dy - (double)c.iy);
	p.base   = isX ? c.sx : c.sy;
	p.stride = isX ? 4 : 1;
	p.unit   = unit_for_scale(p.scale);
	return p;
}

// Port 0x6011e4: mengisi (out-clip) entri ke buf[0..]. Entri k untuk piksel tujuan clip+k. Mengembalikan unit (fx/fy).
static inline uint32_t build_axis(AxisEntry *buf, const AxisParams &p, int clip, int out)
{
	buf[0] = edge_entry(p, clip);
	if (clip == out - 1) return p.unit;
	if (clip + 1 >= out - 1) { buf[1] = edge_entry(p, clip + 1); return p.unit; }
	for (int i = clip + 1; i <= out - 2; ++i) buf[i - clip] = interior_entry(p, i);
	buf[out - 1 - clip] = edge_entry(p, out - 1);
	return p.unit;
}

} // namespace layerex_shrink
