// test_core.cpp - uji inti layerExBTOA (host, ASan/UBSan).
// Dua sumber ekspektasi, dipisah jelas:
//  [SPEC]   ekspektasi tulis-tangan dari docs/spec/layerExBTOA_*.md
//  [EMU]    vektor emas golden_emu.inc: hasil MENJALANKAN fungsi asli libgame.so 1.3.9 di emulator AArch64
//           (script/emu/btoa_golden.py). Bukan hasil kode fork.
// Build: g++ -std=c++14 -g -fsanitize=address,undefined -I src/plugins tests/plugins/layerExBTOA/test_core.cpp -o /tmp/t && /tmp/t
#include <cstdio>
#include <cstdlib>
#include <vector>
#include <cstdint>
#include "layerExBTOACore.hpp"
using namespace layerex_btoa;

static int g_fail = 0, g_ok = 0;
#define CHECK(c) do { if (c) ++g_ok; else { ++g_fail; if (g_fail <= 25) std::printf("GAGAL %s:%d  %s\n", __FILE__, __LINE__, #c); } } while (0)

// ---- PRNG & hash identik dengan script/emu/btoa_golden.py
struct Rng { uint64_t s; explicit Rng(uint64_t seed) : s(seed) {}
	uint8_t byte() { s = s * 6364136223846793005ULL + 1442695040888963407ULL; return (uint8_t)(s >> 56); }
	std::vector<uint8_t> bytes(size_t n) { std::vector<uint8_t> v(n); for (auto &c : v) c = byte(); return v; } };
static uint64_t fnv(const std::vector<uint8_t> &v) { uint64_t h = 0xcbf29ce484222325ULL; for (uint8_t c : v) { h ^= c; h *= 0x100000001b3ULL; } return h; }

struct GCopy { unsigned seed; int w, h, pad; uint64_t hash; int ul, ut, ur, ub; };
struct GProv { unsigned seed; int w, h, padm, padp, cl, ct, cr, cb, lvc, lvv; uint64_t hmain, hprov; int ul, ut, ur, ub; };
struct GFill { unsigned seed; int w, h, provH, padm, padp, ctop, pv, unused; uint32_t color; uint64_t hash; int u0, u1, u2, u3; };
struct GClip { unsigned seed; int dw, dh, sw, sh, padd, pads; long long dx, dy, sx, sy, w, h; int fk; long long val; int ret; uint64_t hash; int hasUpd; int u0, u1, u2, u3; };
#include "golden_emu.inc"
#define N(a) (sizeof(a) / sizeof((a)[0]))

// ======================= [SPEC] tulis-tangan =======================
static void spec_tests() {
	// copyRightBlueToLeftAlpha: alfa piksel kiri <- byte[2] piksel (x+half); 4 piksel => half = 2
	{ uint8_t b[16] = {0,0,0,0xAA, 0,0,0,0xBB, 0,0,0x11,0, 0,0,0x22,0};
	  Rect r = copy_right_blue_to_left_alpha(b, 16, 4, 1);
	  CHECK(b[3] == 0x11 && b[7] == 0x22 && b[11] == 0 && b[15] == 0 && r.left == 0 && r.top == 0 && r.right == 2 && r.bottom == 1); }
	// lebar ganjil 5: half = 2, piksel ke-5 (indeks 4) tidak dipakai & tidak diubah
	{ uint8_t b[20] = {}; for (int i = 0; i < 20; ++i) b[i] = (uint8_t)(i * 3);
	  uint8_t o[20]; for (int i = 0; i < 20; ++i) o[i] = b[i];
	  Rect r = copy_right_blue_to_left_alpha(b, 20, 5, 1);
	  CHECK(b[3] == o[2 * 4 + 2] && b[7] == o[3 * 4 + 2] && b[19] == o[19] && r.right == 2); }
	// lebar 1: tidak ada yang berubah, rect (0,0,0,h)
	{ uint8_t b[4] = {1,2,3,4}; Rect r = copy_right_blue_to_left_alpha(b, 4, 1, 1); CHECK(b[3] == 4 && r.right == 0 && r.bottom == 1); }
	// copyBottomBlueToTopAlpha: tinggi 4, pitch 4 (1 piksel): baris0.a <- baris2.b2, baris1.a <- baris3.b2
	{ uint8_t b[16] = {0,0,0,9, 0,0,0,9, 0,0,0x55,0, 0,0,0x66,0};
	  Rect r = copy_bottom_blue_to_top_alpha(b, 4, 1, 4);
	  CHECK(b[3] == 0x55 && b[7] == 0x66 && b[11] == 0 && b[15] == 0 && r.right == 1 && r.bottom == 2); }
	// tinggi 1: tidak ada perubahan
	{ uint8_t b[4] = {1,2,3,4}; Rect r = copy_bottom_blue_to_top_alpha(b, 4, 1, 1); CHECK(b[3] == 4 && r.bottom == 0); }
	// copyAlphaToProvince tiga mode
	{ uint8_t px[12] = {0,0,0,0, 0,0,0,100, 0,0,0,255}; uint8_t d[3];
	  copy_alpha_to_province(px, 12, d, 3, 3, 1, -1);  CHECK(d[0] == 0 && d[1] == 100 && d[2] == 255);
	  copy_alpha_to_province(px, 12, d, 3, 3, 1, 100); CHECK(d[0] == 0 && d[1] == 1 && d[2] == 1);   // alfa >= level
	  copy_alpha_to_province(px, 12, d, 3, 3, 1, 101); CHECK(d[0] == 0 && d[1] == 0 && d[2] == 1);
	  copy_alpha_to_province(px, 12, d, 3, 3, 1, 0);   CHECK(d[0] == 1 && d[1] == 1 && d[2] == 1);   // level 0: semua 1
	  d[0] = d[1] = d[2] = 9; copy_alpha_to_province(px, 12, d, 3, 3, 1, 256); CHECK(d[0] == 0 && d[1] == 0 && d[2] == 0); }
	CHECK(province_mode(-1) == 0 && province_mode(0) == 1 && province_mode(255) == 1 && province_mode(256) == 2);
	// swap_rb
	CHECK(swap_rb(0x11223344u) == 0x11443322u); CHECK(swap_rb(swap_rb(0xdeadbeefu)) == 0xdeadbeefu);
	// mul_alpha: 255*x = x, 0*x = 0, titik tengah
	for (int i = 0; i < 256; ++i) { CHECK(mul_alpha((uint8_t)i, 255) == i); CHECK(mul_alpha((uint8_t)i, 0) == 0); CHECK(mul_alpha(255, (uint8_t)i) == i); }
	CHECK(mul_alpha(128, 128) == 64);
	// precheck & parameter opsional
	CHECK(clip_alpha_rect_precheck(6, 5, 5) == kTJS_E_BADPARAMCOUNT);
	CHECK(clip_alpha_rect_precheck(7, 0, 5) == kTJS_E_INVALIDPARAM && clip_alpha_rect_precheck(7, 5, 0) == kTJS_E_INVALIDPARAM);
	CHECK(clip_alpha_rect_precheck(7, 1, 1) == 0);
	CHECK(!clip_alpha_rect_has_outside(7, false, 5) && !clip_alpha_rect_has_outside(8, true, 5));
	CHECK(clip_alpha_rect_has_outside(8, false, 0) && clip_alpha_rect_has_outside(8, false, 255));
	CHECK(!clip_alpha_rect_has_outside(8, false, 256) && !clip_alpha_rect_has_outside(8, false, -1));
	// clipAlphaRect: contoh sederhana 4x1 dst, src 2x1 di dx=1, tanpa nilai-luar
	{ uint8_t d[16], s[8]; for (int i = 0; i < 16; ++i) d[i] = 0; for (int x = 0; x < 4; ++x) d[x * 4 + 3] = 200;
	  s[3] = 255; s[7] = 128; const uint8_t *rows[1] = { s };
	  ClipIn in = { 1, 0, 0, 0, 2, 1, 2, 1, 4, 1 };
	  ClipResult r = clip_alpha_rect(d, 16, rows, in, false, 0, 0, 0);
	  CHECK(d[3] == 200 && d[7] == 200 && d[11] == mul_alpha(200, 128) && d[15] == 200);   // dx=1, w=2 -> piksel 1..2
	  CHECK(d[7] == mul_alpha(200, 255) && r.update && r.x == 1 && r.y == 0 && r.w == 2 && r.h == 1); }
	// dengan nilai-luar 7: sisa area klip menjadi alfa 7
	{ uint8_t d[16] = {}, s[8] = {}; s[3] = s[7] = 255; const uint8_t *rows[1] = { s };
	  ClipIn in = { 1, 0, 0, 0, 2, 1, 2, 1, 4, 1 };
	  ClipResult r = clip_alpha_rect(d, 16, rows, in, true, 7, 0, 0);
	  CHECK(d[3] == 7 && d[15] == 7 && r.update && r.x == 0 && r.w == 4 && r.h == 1); }
	// sepenuhnya di luar: tanpa nilai-luar => tidak ada update; dengan nilai-luar => seluruh klip
	{ uint8_t d[16] = {}, s[8] = {}; const uint8_t *rows[1] = { s };
	  ClipIn in = { 50, 50, 0, 0, 2, 1, 2, 1, 4, 1 };
	  ClipResult r = clip_alpha_rect(d, 16, rows, in, false, 0, 0, 0); CHECK(!r.update);
	  r = clip_alpha_rect(d, 16, rows, in, true, 9, 0, 0); CHECK(r.update && r.w == 4 && r.h == 1 && d[3] == 9 && d[15] == 9); }
	// fillByProvince: piksel dengan province == nilai ditulis 32-bit; yang lain tak berubah
	{ uint8_t d[8] = {1,2,3,4,5,6,7,8}; uint8_t p[2] = {5, 6};
	  fill_by_province(d, 8, 2, 1, p, 2, 2, 1, 0, 0, 6, swap_rb(0xAA112233u));
	  uint32_t px; std::memcpy(&px, d + 4, 4); CHECK(px == swap_rb(0xAA112233u)); CHECK(d[0] == 1 && d[3] == 4); }
	// province di luar rentang tidak cocok (tidak membaca di luar buffer)
	{ uint8_t d[4] = {1,2,3,4}; uint8_t p[1] = {0}; fill_by_province(d, 4, 1, 1, p, 1, 1, 1, 0, 1, 0, 0xffffffffu); CHECK(d[0] == 1 && d[3] == 4); }
}

// ======================= [EMU] vektor emas =======================
static void emu_copy(const GCopy *g, size_t n, bool right) {
	for (size_t i = 0; i < n; ++i) {
		const GCopy &c = g[i]; Rng r(c.seed);
		const int pitch = c.w * 4 + c.pad; std::vector<uint8_t> b = r.bytes((size_t)pitch * c.h);
		b.resize(b.size() + 4);   // jaga pointer valid saat h = 0
		Rect u = right ? copy_right_blue_to_left_alpha(b.data(), pitch, c.w, c.h) : copy_bottom_blue_to_top_alpha(b.data(), pitch, c.w, c.h);
		b.resize((size_t)pitch * c.h);
		CHECK(fnv(b) == c.hash); CHECK(u.left == c.ul && u.top == c.ut && u.right == c.ur && u.bottom == c.ub);
	}
}
static void emu_prov() {
	for (size_t i = 0; i < N(kGoldProv); ++i) {
		const GProv &g = kGoldProv[i]; Rng r(g.seed);
		const int mp = g.w * 4 + g.padm, pp = g.w + g.padp;
		std::vector<uint8_t> main = r.bytes((size_t)mp * g.h), prov = r.bytes((size_t)pp * g.h);
		const int level = g.lvc == 2 ? g.lvv : -1;
		copy_alpha_to_province(main.data() + (ptrdiff_t)g.ct * mp + g.cl * 4, mp, prov.data() + (ptrdiff_t)g.ct * pp + g.cl, pp,
		                       g.cr - g.cl, g.cb - g.ct, level);
		CHECK(fnv(main) == g.hmain); CHECK(fnv(prov) == g.hprov);
		CHECK(g.ul == g.cl && g.ut == g.ct && g.ur == g.cr && g.ub == g.cb);   // update(clip)
	}
}
static void emu_fill() {
	for (size_t i = 0; i < N(kGoldFill); ++i) {
		const GFill &g = kGoldFill[i]; Rng r(g.seed);
		const int mp = g.w * 4 + g.padm, pp = g.w + g.padp;
		std::vector<uint8_t> main = r.bytes((size_t)mp * g.h), prov = r.bytes((size_t)pp * g.provH);
		for (auto &c : prov) c &= 3;
		fill_by_province(main.data(), mp, g.w, g.h, prov.data(), pp, g.w, g.provH, 0, g.ctop, (uint8_t)g.pv, swap_rb(g.color));
		CHECK(fnv(main) == g.hash);
		CHECK(g.u0 == 0 && g.u1 == g.ctop && g.u2 == g.w && g.u3 == g.h);      // update(left, top, texW, texH)
	}
}
static int t32(long long v) { return (int)(uint32_t)(uint64_t)v; }   // tTVInteger (int64) -> w-register, seperti di biner
static void emu_clip() {
	for (size_t i = 0; i < N(kGoldClip); ++i) {
		const GClip &g = kGoldClip[i]; Rng r(g.seed);
		const int dp = g.dw * 4 + g.padd, sp = g.sw * 4 + g.pads;
		std::vector<uint8_t> dst = r.bytes((size_t)dp * g.dh), src = r.bytes((size_t)sp * g.sh);
		const int np = g.fk == 0 ? 7 : 8;
		const int pre = clip_alpha_rect_precheck(np, t32(g.w), t32(g.h));
		CHECK(pre == g.ret);
		if (pre != 0) { CHECK(fnv(dst) == g.hash); CHECK(!g.hasUpd); continue; }
		std::vector<const uint8_t *> rows(g.sh); for (int y = 0; y < g.sh; ++y) rows[y] = src.data() + (size_t)y * sp;
		const bool has = clip_alpha_rect_has_outside(np, g.fk == 1, g.val);
		ClipIn in = { t32(g.dx), t32(g.dy), t32(g.sx), t32(g.sy), t32(g.w), t32(g.h), g.sw, g.sh, g.dw, g.dh };
		ClipResult res = clip_alpha_rect(dst.data(), dp, rows.data(), in, has, (uint8_t)(g.val & 0xff), 0, 0);
		CHECK(fnv(dst) == g.hash);
		CHECK((res.update ? 1 : 0) == g.hasUpd);
		if (res.update) CHECK(res.x == g.u0 && res.y == g.u1 && res.w == g.u2 && res.h == g.u3);
	}
}

int main() {
	spec_tests();
	emu_copy(kGoldRight, N(kGoldRight), true); emu_copy(kGoldBottom, N(kGoldBottom), false);
	emu_prov(); emu_fill(); emu_clip();
	std::printf("layerExBTOA core: %d ok, %d gagal  (emu: right=%zu bottom=%zu prov=%zu fill=%zu clip=%zu)\n", g_ok, g_fail,
	            N(kGoldRight), N(kGoldBottom), N(kGoldProv), N(kGoldFill), N(kGoldClip));
	return g_fail ? 1 : 0;
}
