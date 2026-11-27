// g++ -std=c++14 -fsanitize=address,undefined -I src/plugins tests/plugins/layerExShrink/test_core.cpp && ./a.out
#include <stdio.h>
#include <math.h>
#include <vector>
#include <stdlib.h>
#include "layerExShrinkCore.hpp"
using namespace layerex_shrink;
static int fails = 0;
#define CHECK(c) do { if (!(c)) { printf("FAIL %s:%d %s\n", __FILE__, __LINE__, #c); ++fails; } } while (0)

static Ctx mk(double dx, double dy, double dw, double dh, int sx, int sy, int sw, int sh, int SW, int SH, int DW, int DH) {
	Ctx c = {}; c.dx = dx; c.dy = dy; c.dw = dw; c.dh = dh; c.sx = sx; c.sy = sy; c.sw = sw; c.sh = sh;
	c.srcW = SW; c.srcH = SH; c.dstW = DW; c.dstH = DH; return c;
}

int main()
{
	{ Ctx c = mk(0, 0, 4, 4, 0, 0, 8, 8, 8, 8, 16, 16); CHECK(clip(c));
	  CHECK(c.ix == 0 && c.iy == 0 && c.iw == 4 && c.ih == 4 && c.clipX == 0 && c.outW == 4 && c.outH == 4); }
	// dest pecahan: x 0.5..2.7 -> ix=0, right=2 -> iw=2, 2.7>2 -> 3
	{ Ctx c = mk(0.5, 0, 2.2, 1, 0, 0, 4, 1, 8, 8, 16, 16); CHECK(clip(c)); CHECK(c.ix == 0 && c.iw == 3); }
	// sumber negatif memotong dan menggeser tujuan: skala 0.5, sx=-2 -> dx+=1, dw-=1
	{ Ctx c = mk(0, 0, 4, 4, -2, 0, 8, 8, 8, 8, 16, 16); CHECK(clip(c));
	  CHECK(c.sx == 0 && c.sw == 6 && fabs(c.dx - 1.0) < 1e-12 && fabs(c.dw - 3.0) < 1e-12 && c.ix == 1 && c.iw == 3); }
	// sumber melewati tepi kanan
	{ Ctx c = mk(0, 0, 4, 4, 4, 0, 8, 8, 8, 8, 16, 16); CHECK(clip(c)); CHECK(c.sw == 4 && fabs(c.dw - 2.0) < 1e-12); }
	// tujuan di luar kiri: ix negatif -> clipX
	{ Ctx c = mk(-2, 0, 4, 4, 0, 0, 8, 8, 8, 8, 16, 16); CHECK(clip(c)); CHECK(c.ix == -2 && c.clipX == 2 && c.outW == c.iw); }
	// tujuan melewati tepi kanan layer
	{ Ctx c = mk(14, 0, 4, 4, 0, 0, 8, 8, 8, 8, 16, 16); CHECK(clip(c)); CHECK(c.outW == 2); }
	// menolak
	{ Ctx c = mk(0, 0, 4, 4, 8, 0, 8, 8, 8, 8, 16, 16); CHECK(!clip(c)); }
	{ Ctx c = mk(0, 0, 4, 4, 0, -8, 8, 8, 8, 8, 16, 16); CHECK(!clip(c)); }
	{ Ctx c = mk(16, 0, 4, 4, 0, 0, 8, 8, 8, 8, 16, 16); CHECK(!clip(c)); }

	// ---- accumulate: 3 -> 2 piksel (1,5 px sumber per piksel tujuan), 1 baris, satuan bobot=2 ----
	{
		const uint8_t row0[3 * 4] = { 10,20,30,40,  50,60,70,80,  90,100,110,120 };
		// baris tambahan sebelum/sesudah (dipakai hanya bila bobot nonnol)
		std::vector<uint8_t> pad(12, 0xEE);
		const uint8_t *rows[3] = { pad.data(), row0, pad.data() };
		AxisEntry xe[2] = { {0, 1, 0, 0, 1, 1, 3}, {8, 1, 1, 1, 0, 0, 3} };
		AxisEntry ye[1] = { {1, 1, 0, 0, 0, 0, 2} }; // satu baris penuh, fy=2
		uint8_t out[8];
		accumulate(out, 8, xe, 2, ye, 1, rows + 0, 2, 2);
		// rows di accumulate = larik penuh; ye.off=1 -> rows[1]=row0
		for (int ch = 0; ch < 4; ++ch) {
			double e0 = (row0[ch] * 2.0 + row0[4 + ch] * 1.0) / 3.0;
			double e1 = (row0[4 + ch] * 1.0 + row0[8 + ch] * 2.0) / 3.0;
			CHECK(fabs(out[ch] - e0) < 1.0 + 1e-9); CHECK(fabs(out[4 + ch] - e1) < 1.0 + 1e-9);
		}
	}
	// ---- accumulate: reduksi 2:1 murni 4x2 -> 2x1 ----
	{
		uint8_t img[2][16]; for (int y = 0; y < 2; ++y) for (int i = 0; i < 16; ++i) img[y][i] = (uint8_t)(y * 100 + i * 5);
		AxisEntry xe[2] = { {0, 2, 0, 0, 0, 0, 2}, {8, 2, 0, 0, 0, 0, 2} };
		AxisEntry ye[1] = { {1, 2, 0, 0, 0, 0, 2} };
		uint8_t out[8];
		// ye.n=2 membaca rows[off..off+1] = img[0], img[1]; bawah rows[off+n]=rows[3] tidak ada -> beri larik 4
		const uint8_t *r4[4] = { nullptr, img[0], img[1], nullptr };
		accumulate(out, 8, xe, 2, ye, 1, r4, 1, 1);
		for (int ch = 0; ch < 4; ++ch) {
			int e0 = (img[0][ch] + img[0][4 + ch] + img[1][ch] + img[1][4 + ch]) / 4;
			CHECK(out[ch] == e0);
		}
	}
	// ---- tabel: unit & entri interior ----
	CHECK(unit_for_scale(2.0) == 256); CHECK(unit_for_scale(1.0) == 256);
	{ AxisParams p = {8.0, 2.0, 0.0, 0, 4, 256}; AxisEntry e = interior_entry(p, 1);
	  CHECK(e.off == 12 && e.n == 1 && e.wa_lo == 256 && e.wa_hi == 0 && e.div == 512); }
	{ AxisParams p = {99.0, 1.5, 0.25, 0, 1, 256}; AxisEntry e = interior_entry(p, 2);
	  CHECK(e.off == 3 && e.n == 1 && e.wa_lo == 96 && e.wc_lo == 96 && e.wa_hi == 32 && e.div == 384); }
	for (int i = 1; i < 40; ++i) { // total bobot ~= scale*unit (selisih <= 2 karena pemotongan)
		AxisParams p = {999.0, 1.7, 0.4, 3, 4, 256}; AxisEntry e = interior_entry(p, i);
		CHECK(abs(e.div - (int)(1.7 * 256)) <= 2); CHECK(e.n >= 0);
	}
	// ---- tabel penuh: edge_entry & build_axis ----
	{ // skala 2, tanpa pecahan: entri tepi harus sama dengan interior, bobot alfa = warna
		AxisParams p = {8.0, 2.0, 0.0, 0, 4, 256}; AxisEntry a = edge_entry(p, 0), b = interior_entry(p, 1), c = edge_entry(p, 1);
		CHECK(a.off == 4 && a.n == 1 && a.wa_lo == 256 && a.wc_lo == 256 && a.wa_hi == 0 && a.div == 512);
		CHECK(c.off == b.off && c.n == b.n && c.div == b.div && c.wc_lo == b.wc_lo);
	}
	{ // frac>0: awal mentah negatif -> warna memperpanjang piksel 0 (>256), alfa tetap 256
		AxisParams p = {10.0, 2.0, 0.3, 0, 4, 256}; AxisEntry e = edge_entry(p, 0);
		// st=-0.6 -> ST=0, stC=0; wc_lo = 256 - int(-0.6*256) = 256+153 = 409; wa_lo = 256
		CHECK(e.wc_lo == 409 && e.wa_lo == 256 && e.off == 4);
	}
	{ // akhir melewati sumber: alfa terpotong ke srcLen (wa_hi=0), warna tetap punya wc_hi
		AxisParams p = {4.0, 2.0, 0.0, 0, 4, 256}; AxisEntry e = edge_entry(p, 2);
		// st=4, en=6 > 4 -> enC=4; STc=4, ENc=4, n=-1 ... ; di binary pemanggil tidak memakai indeks di luar rentang
		(void)e;
		AxisEntry l = edge_entry(p, 1); // st=2,en=4 : tepat di tepi
		CHECK(l.n == 1 && l.wa_hi == 0 && l.wc_hi == 0 && l.div == 512);
	}
	{ // build_axis: jumlah entri, tepi == edge_entry, tengah == interior_entry
		AxisParams p = {12.0, 1.5, 0.25, 0, 4, 256}; AxisEntry t[9];
		CHECK(build_axis(t, p, 0, 8) == 256);
		CHECK(t[0].off == edge_entry(p, 0).off && t[7].div == edge_entry(p, 7).div && t[3].off == interior_entry(p, 3).off);
		AxisEntry t2[2]; build_axis(t2, p, 3, 5); CHECK(t2[0].off == edge_entry(p, 3).off && t2[1].off == edge_entry(p, 4).off);
		AxisEntry t1[1]; build_axis(t1, p, 4, 5); CHECK(t1[0].off == edge_entry(p, 4).off);
	}
	{ // end-to-end: 8x8 abu rata -> 4x4 (dw=4): semua piksel tujuan harus sama dengan sumber (alfa & warna) kecuali tepi alfa
		const int SW = 8, DW = 4; std::vector<uint8_t> img(SW * SW * 4, 0);
		for (int i = 0; i < SW * SW; ++i) { img[i*4+0] = 50; img[i*4+1] = 100; img[i*4+2] = 150; img[i*4+3] = 200; }
		std::vector<const uint8_t*> rows(SW + 1, nullptr); for (int y = 0; y < SW; ++y) rows[y] = &img[y * SW * 4]; // rows[SW] tidak boleh dibaca (bobot nol)
		Ctx c = mk(0, 0, DW, DW, 0, 0, SW, SW, SW, SW, 16, 16); CHECK(clip(c));
		std::vector<AxisEntry> xe(c.outW - c.clipX), ye(c.outH - c.clipY);
		AxisParams px = make_axis_params(c, true), py = make_axis_params(c, false);
		uint32_t fx = build_axis(xe.data(), px, c.clipX, c.outW), fy = build_axis(ye.data(), py, c.clipY, c.outH);
		std::vector<uint8_t> out(DW * DW * 4, 0);
		accumulate(out.data(), DW * 4, xe.data(), (int)xe.size(), ye.data(), (int)ye.size(), rows.data(), fx, fy);
		for (int i = 0; i < DW * DW; ++i) CHECK(out[i*4] == 50 && out[i*4+1] == 100 && out[i*4+2] == 150 && out[i*4+3] == 200);
	}
	printf(fails ? "GAGAL: %d\n" : "OK semua\n", fails);
	return fails != 0;
}
