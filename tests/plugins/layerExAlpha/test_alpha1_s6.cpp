// Uji inti layerExAlpha (Alpha-1).
//   g++ -std=c++14 -fsanitize=address,undefined -I src/plugins tests/plugins/layerExAlpha/test_alpha1_s6.cpp && ./a.out [golden_cpu.txt]
// Bagian A: nilai tangan dari docs/spec/layerExAlpha_*.md (dihitung dari rumus, bukan dari kode).
// Bagian B: replay vektor emas (golden_cpu.txt) = keluaran EMULASI instruksi libgame.so 1.3.9 (script/k2emu_alpha1.py).
//   Tingkat bukti: inti-host + emulasi instruksi. BUKAN uji di perangkat.
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <string>
#include <vector>
#include <fstream>
#include <sstream>
#include "layerExAlphaCore.hpp"
using namespace layerex_alpha;

static int fails = 0, checks = 0;
#define CHECK(c) do { ++checks; if (!(c)) { printf("FAIL %s:%d %s\n", __FILE__, __LINE__, #c); ++fails; } } while (0)

typedef std::vector<u8> Buf;
static Buf px(std::initializer_list<unsigned> v) { Buf b; for (unsigned x : v) b.push_back((u8)x); return b; }   // B,G,R,A tiap piksel

// ------------------------------------------------------------------ Bagian A
static void partA()
{
	{ Buf b = px({10, 20, 30, 0,   1, 2, 3, 255,   9, 9, 9, 100});
	  turn_alpha(b.data(), 12, 3, 1);                 CHECK(b == px({0,0,0,255,  0,0,0,0,  0,0,0,155})); }
	{ Buf b = px({10, 20, 30, 0,   1, 2, 3, 255,   9, 9, 9, 100});
	  turn_alpha2(b.data(), 12, 3, 1);                CHECK(b == px({10,20,30,255,  1,2,3,0,  9,9,9,155})); }
	{ Buf b = px({10, 20, 30, 0,   1, 2, 3, 255,   9, 9, 9, 100});
	  turn_alpha3(b.data(), 12, 3, 1);                CHECK(b == px({10,20,30,0,  1,2,3,0,  9,9,9,155})); }
	{ Buf b = px({10, 20, 30, 0,   1, 2, 3, 255,   9, 9, 9, 254});
	  turn_outline(b.data(), 12, 3, 1);               CHECK(b == px({0,0,0,0,  0,0,0,0,  0,0,0,254})); }
	// AlphaColorRect: -1 = biarkan; arg1->R(byte2), arg2->G(byte1), arg3->B(byte0); 8 bit rendah
	{ Buf b = px({1, 2, 3, 4});
	  alpha_color_rect(b.data(), 4, 1, 1, 0xAA, -1, 0x1BB);   CHECK(b == px({0xBB, 2, 0xAA, 4})); }
	// AlphaDivide: alfa piksel PERTAMA baris menentukan seluruh baris (perilaku binary)
	{ Buf b = px({0,0,0,100,  0,0,0,10,    0,0,0,99,  0,0,0,200});
	  alpha_divide(b.data(), 8, 2, 2, 100);           CHECK(b == px({0,0,0,255,  0,0,0,255,  0,0,0,0,  0,0,0,0})); }
	// AlphaAdd: flag bit0 -> A==0 dilewati
	{ Buf b = px({0,0,0,0,  0,0,0,250,  0,0,0,5});
	  alpha_add(b.data(), 12, 3, 1, 10, 1);          CHECK(b == px({0,0,0,0,  0,0,0,255,  0,0,0,15})); }
	{ Buf b = px({0,0,0,0,  0,0,0,250,  0,0,0,5});
	  alpha_add(b.data(), 12, 3, 1, -10, 0);         CHECK(b == px({0,0,0,0,  0,0,0,240,  0,0,0,0})); }
	// AlphaAdd2: v=A+add ; v<thr atau v<0 -> 0 ; selain itu min(v,255)
	{ Buf b = px({0,0,0,10,  0,0,0,50,  0,0,0,250});
	  alpha_add2(b.data(), 12, 3, 1, 5, 20);     CHECK(b == px({0,0,0,0,  0,0,0,55,  0,0,0,255})); }
	// BlackForAlpha: A=(R+G+B)/3, RGB=0
	{ Buf b = px({3, 4, 6, 77});
	  black_for_alpha(b.data(), 4, 1, 1);                 CHECK(b == px({0,0,0,4})); }       // 13/3 = 4
	// ColorAlphaAdd(rmax,gmax,bmax,add): kanal=min(kanal+add,maks); A=clamp(A-add)
	{ Buf b = px({100, 100, 100, 50});
	  color_alpha_add(b.data(), 4, 1, 1, 130, 110, 150, 20);   CHECK(b == px({120, 110, 120, 30})); }
	{ Buf b = px({100, 100, 100, 10});
	  color_alpha_add(b.data(), 4, 1, 1, 255, 255, 255, 20);   CHECK(b == px({120, 120, 120, 0})); }
	// ColorVectorEffectAdd: add=51 -> step=5; r:0->255 => dR=51; A=255 -> A'=204, t=(51)/51=1 -> R=51
	{ Buf b = px({0, 0, 0, 255});
	  color_vector_effect_add(b.data(), 4, 1, 1, 0, 255, 0, 255, 0, 0, 51);
	  CHECK(b == px({0, 51, 51, 204})); }           // B=dB*t+b0=0 ; G=51 ; R=51
	{ Buf b = px({7, 7, 7, 0});                     // A==0 -> tidak disentuh
	  color_vector_effect_add(b.data(), 4, 1, 1, 0, 255, 0, 255, 0, 0, 51);   CHECK(b == px({7,7,7,0})); }
	// pembagian dengan nol (add=0): step=0 -> dR=0, t=0 -> warna = konstanta awal; tanpa trap
	{ Buf b = px({7, 7, 7, 200});
	  color_vector_effect_add(b.data(), 4, 1, 1, 11, 99, 22, 99, 33, 99, 0);  CHECK(b == px({33, 22, 11, 200})); }
	// ExtraColorVectorEffectAdd cabang gradien: avg>thr, t!=0
	{ Buf b = px({200, 200, 200, 255});             // add=51: A'=204,t=1 ; thr=(255+255+0)/3=170 ; avg=200>170
	  extra_color_vector_effect_add(b.data(), 4, 1, 1, 0, 255, 0, 255, 0, 0, 51);        CHECK(b == px({0, 51, 51, 204})); }

	// MaskRect CPU (mode 1,2,3), alfa tujuan 100, sumber 200: add=255(min), sub=45, mul=(100*200)>>8=78
	{ Buf d = px({0,0,0,100,  0,0,0,100}), s = px({0,0,0,200});
	  mask_rect_cpu(d.data(), 8, 2, 1, s.data(), 4, 1, 1, 1, 0, 0, 0, 1, 1, MASK_ADD);   CHECK(d == px({0,0,0,100,  0,0,0,255})); }
	{ Buf d = px({0,0,0,100}), s = px({0,0,0,200});
	  mask_rect_cpu(d.data(), 4, 1, 1, s.data(), 4, 1, 1, 0, 0, 0, 0, 1, 1, MASK_SUB);   CHECK(d == px({0,0,0,45})); }
	{ Buf d = px({0,0,0,100}), s = px({0,0,0,200});
	  mask_rect_cpu(d.data(), 4, 1, 1, s.data(), 4, 1, 1, 0, 0, 0, 0, 1, 1, MASK_MUL);   CHECK(d == px({0,0,0,78})); }
	CHECK(mask_kind(0) == 0 && mask_kind(1) == 1 && mask_kind(3) == 1 && mask_kind(4) == 2 && mask_kind(-1) == 2);

	// ExMaskRect: layer 3x2, sumber 3x3 alfa 200 semua; dx=1,dy=1,sw=1,sh=1, mode 0 (salin), sy=0
	{ Buf d = px({0,0,0,90, 0,0,0,90, 0,0,0,90,   0,0,0,90, 0,0,0,90, 0,0,0,90});
	  Buf s(3 * 3 * 4, 0); for (int i = 0; i < 9; ++i) s[4 * i + 3] = 200;
	  unsigned f = exmask_rect(d.data(), 12, 3, 2, s.data(), 12, 3, 3, 1, 1, 0, 1, 1, MASK_COPY);
	  CHECK(f == 0);
	  // baris 0 (atas) -> 0 ; baris 1: kiri(kolom0)=0, kolom1=200 (salin), kanan(kolom2)=0
	  CHECK(d == px({0,0,0,0, 0,0,0,0, 0,0,0,0,   0,0,0,0, 0,0,0,200, 0,0,0,0})); }
	// dx > W -> log kiri, tanpa perubahan & tanpa redraw
	CHECK(exmask_precheck(3, 2, 4, 0) == (EXMASK_NO_REDRAW | EXMASK_LOG_LEFT));
	CHECK(exmask_precheck(3, 2, 3, 3) == (EXMASK_NO_REDRAW | EXMASK_LOG_LEFT | EXMASK_LOG_TOP));
	CHECK(exmask_precheck(3, 2, 3, 2) == 0);
	CHECK(exmask_precheck(3, 2, 0, 3) == (EXMASK_NO_REDRAW | EXMASK_LOG_TOP));
	// mode tak dikenal: baris atas tetap dibersihkan lebih dulu (urutan binary), lalu log + tanpa redraw
	{ Buf d(4 * 2 * 4, 0x55), s(16, 0);
	  unsigned f = exmask_rect(d.data(), 16, 4, 2, s.data(), 16, 4, 1, 0, 1, 0, 1, 1, 7);
	  CHECK(f == (EXMASK_LOG_MODE | EXMASK_NO_REDRAW));
	  CHECK(d[3] == 0 && d[7] == 0 && d[11] == 0 && d[15] == 0);          // baris 0 alfa = 0
	  CHECK(d[16 + 3] == 0x55);                                           // baris 1 tidak disentuh
	  CHECK(d[0] == 0x55 && d[1] == 0x55); }                              // warna tidak pernah disentuh
	// dy < 0 dan sh+dy < 0 : binary menulis di luar buffer; fork membersihkan baris di dalam batas saja
	{ Buf d(2 * 2 * 4, 0x55), s(16, 0);
	  unsigned f = exmask_rect(d.data(), 8, 2, 2, s.data(), 8, 2, 2, 0, -2, 0, 2, 0, MASK_COPY);
	  CHECK(f == 0); CHECK(d[3] == 0 && d[7] == 0 && d[11] == 0 && d[15] == 0); }
	// ekstrem (tidak boleh UB / crash di bawah UBSan)
	{ Buf d(8, 0x55), s(8, 0x55);
	  exmask_rect(d.data(), 8, 2, 1, s.data(), 8, 2, 1, 0, 0, 0, 0x7fffffff, 0x7fffffff, MASK_ADD);
	  exmask_rect(d.data(), 8, 2, 1, s.data(), 8, 2, 1, -0x7fffffff - 1, -0x7fffffff - 1, 0, 5, 5, MASK_SUB);
	  extra_color_vector_effect_add(d.data(), 8, 2, 1, 0x7fffffff, -0x7fffffff - 1, 5, 5, 5, 5, -1);
	  color_vector_effect_add(d.data(), 8, 2, 1, 0x7fffffff, -0x7fffffff - 1, 5, 5, 5, 5, -1); }
}

// ------------------------------------------------------------------ Bagian B
static std::vector<std::string> split(const std::string &s, char c)
{
	std::vector<std::string> r; std::string cur; std::stringstream ss(s);
	while (std::getline(ss, cur, c)) r.push_back(cur);
	if (!s.empty() && s.back() == c) r.push_back("");
	return r;
}
static Buf unhex(const std::string &h)
{
	Buf b; if (h == "-") return b;
	for (size_t i = 0; i + 1 < h.size(); i += 2) b.push_back((u8)strtoul(h.substr(i, 2).c_str(), 0, 16));
	return b;
}
static std::vector<long> ints(const std::string &s) { std::vector<long> v; if (s == "-") return v; for (auto &t : split(s, ',')) v.push_back(strtol(t.c_str(), 0, 10)); return v; }

static int partB(const char *path)
{
	std::ifstream f(path);
	if (!f) { printf("golden tidak ditemukan: %s\n", path); return 1; }
	std::string line; int n = 0;
	while (std::getline(f, line)) {
		if (line.empty() || line[0] == '#') continue;
		auto F = split(line, '|');
		if (F.size() < 10) { printf("baris golden rusak\n"); ++fails; continue; }
		const std::string &name = F[0];
		auto P = ints(F[1]); auto D = ints(F[2]); auto S = ints(F[4]);
		Buf buf = unhex(F[3]), src = unhex(F[5]), exp = unhex(F[6]);
		const int W = (int)D[0], H = (int)D[1]; const ptrdiff_t pitch = D[2];
		u8 *b = buf.data(); int redraw = 1; unsigned logs = 0;
		auto logbits = F[8];
		if (name == "turnAlpha") turn_alpha(b, pitch, W, H);
		else if (name == "turnAlpha2") turn_alpha2(b, pitch, W, H);
		else if (name == "turnAlpha3") turn_alpha3(b, pitch, W, H);
		else if (name == "turnOutline") turn_outline(b, pitch, W, H);
		else if (name == "AlphaColorRect") alpha_color_rect(b, pitch, W, H, P[0], P[1], P[2]);
		else if (name == "AlphaDivide") alpha_divide(b, pitch, W, H, P[0]);
		else if (name == "AlphaAdd") alpha_add(b, pitch, W, H, P[0], P[1]);
		else if (name == "AlphaAdd2") alpha_add2(b, pitch, W, H, P[0], P[1]);
		else if (name == "BlackForAlpha") black_for_alpha(b, pitch, W, H);
		else if (name == "ColorAlphaAdd") color_alpha_add(b, pitch, W, H, P[0], P[1], P[2], P[3]);
		else if (name == "ColorVectorEffectAdd") color_vector_effect_add(b, pitch, W, H, P[0], P[1], P[2], P[3], P[4], P[5], P[6]);
		else if (name == "ExtraColorVectorEffectAdd") extra_color_vector_effect_add(b, pitch, W, H, P[0], P[1], P[2], P[3], P[4], P[5], P[6]);
		else if (name == "MaskRect") {                    // dx,dy,sx,sy,sw,sh,mode
			const int mode = (int)P[6];
			const int kind = mask_kind(mode);
			if (kind == 1) mask_rect_cpu(b, pitch, W, H, src.data(), S[2], (int)S[0], (int)S[1], (int)P[0], (int)P[1], (int)P[2], (int)P[3], (int)P[4], (int)P[5], mode);
			else if (kind == 2) logs = EXMASK_LOG_MODE;
			else {                                          // kind 0: CopyRect(dx,dy,src,(sx,sy,sx+sw,sy+sh),plane=2)
				char want[128]; snprintf(want, sizeof want, "%ld,%ld,(%ld, %ld, %ld, %ld),2", P[0], P[1], P[2], P[3], P[2] + P[4], P[3] + P[5]);
				CHECK(F[9] == want);
			}
		} else if (name == "ExMaskRect") {                 // dx,dy,arg4,sy,sw,sh,mode
			unsigned fl = exmask_rect(b, pitch, W, H, src.data(), S[2], (int)S[0], (int)S[1], (int)P[0], (int)P[1], (int)P[3], (int)P[4], (int)P[5], (int)P[6]);
			logs = fl & 7u; if (fl & EXMASK_NO_REDRAW) redraw = 0;
		} else { printf("metode tak dikenal: %s\n", name.c_str()); ++fails; continue; }

		// log yang diharapkan (teks asli binary dipetakan ke bit)
		unsigned elog = 0;
		if (logbits.find("[Left]") != std::string::npos) elog |= EXMASK_LOG_LEFT;
		if (logbits.find("[Top]") != std::string::npos) elog |= EXMASK_LOG_TOP;
		if (logbits.find("noting type") != std::string::npos) elog |= EXMASK_LOG_MODE;
		++n;
		CHECK(buf == exp);
		CHECK(logs == elog);
		CHECK(redraw == atoi(F[7].c_str()) || (name == "MaskRect" && F[7] == "1"));
		if (buf != exp) printf("  kasus gagal: %s\n", line.substr(0, 90).c_str());
	}
	printf("golden: %d kasus diputar\n", n);
	return n > 0 ? 0 : 1;
}

int main(int argc, char **argv)
{
	partA();
	const char *g = argc > 1 ? argv[1] : "tests/plugins/layerExAlpha/golden_cpu.txt";
	std::ifstream t(g);
	if (!t) g = "golden_cpu.txt";
	if (partB(g)) ++fails;
	printf("%s: %d pemeriksaan, %d gagal\n", fails ? "GAGAL" : "LOLOS", checks, fails);
	return fails ? 1 : 0;
}
