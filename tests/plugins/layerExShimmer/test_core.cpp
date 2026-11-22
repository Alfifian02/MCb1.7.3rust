// g++ -std=c++14 -fsanitize=address,undefined -I src/plugins tests/plugins/layerExShimmer/test_core.cpp && ./a.out
// Uji inti shimmer/shimmerBuildMap. Tingkat: inti-host. Ekspektasi ditulis dari DISASSEMBLY (0x5eedb8, 0x5f0010), bukan dari APK.
// Label: [BIN] diturunkan dari instruksi · [ASUMSI-INT] bergantung pada worker interior yang belum dibaca (0x5f0494/0x5f0630)
//        · [BIN-L1..L3] perilaku "apa adanya" binary yang tampak seperti bug (lihat kepala layerExShimmerCore.hpp).
#include <stdio.h>
#include <stdlib.h>
#include <vector>
#include "layerExShimmerCore.hpp"
using namespace layerex_shimmer;
static int fails = 0;
#define CHECK(c) do { if (!(c)) { printf("FAIL %s:%d %s\n", __FILE__, __LINE__, #c); ++fails; } } while (0)

typedef std::vector<uint8_t> Img;
static Img mk(int w, int h, uint32_t fill) { Img v((size_t)w * h * 4); for (size_t i = 0; i < (size_t)w * h; ++i) memcpy(&v[i * 4], &fill, 4); return v; }
static uint32_t get(const Img &v, int w, int x, int y) { uint32_t p; memcpy(&p, &v[((size_t)y * w + x) * 4], 4); return p; }
static void put(Img &v, int w, int x, int y, uint32_t p) { memcpy(&v[((size_t)y * w + x) * 4], &p, 4); }
static uint32_t idpix(int x, int y) { return 0xFF000000u | (uint32_t)(y << 8) | (uint32_t)x; }   // piksel unik per (x,y), x,y < 256

struct Run {
	int tw, th, aw, ah, bw, bh;
	Img dst, a, b, c; bool mask;
	Run(int tw_, int th_, int aw_, int ah_, int bw_, int bh_) : tw(tw_), th(th_), aw(aw_), ah(ah_), bw(bw_), bh(bh_),
		dst(mk(tw_, th_, 0xDEADBEEFu)), a(mk(aw_, ah_, 0)), b(mk(bw_, bh_, 0)), c(mk(bw_, bh_, 0xFF)), mask(false) {
		for (int y = 0; y < ah; ++y) for (int x = 0; x < aw; ++x) put(a, aw, x, y, idpix(x, y));
	}
	bool go(int dx, int dy, int w, int h, float sx, float sy) {
		Ctx k = {}; k.dst = dst.data(); k.dpitch = tw * 4; k.dw = tw; k.dh = th;
		k.a = a.data(); k.apitch = aw * 4; k.aw = aw; k.ah = ah; k.b = b.data(); k.bpitch = bw * 4;
		k.c = c.data(); k.cpitch = bw * 4; k.has_mask = mask; k.sxf = to_fixed16(sx); k.syf = to_fixed16(sy);
		// mask=false menguji aritmetika tanpa-mask langsung (di binary cabang ini tak terjangkau: C void -> compute_region menolak)
		if (!compute_region(dx, dy, w, h, aw, ah, tw, th, bw, bh, mask, mask ? bw : 0, mask ? bh : 0, &k.r)) return false;
		apply(k); return true;
	}
};

int main()
{
	// ---- aritmetika bantu ----
	CHECK(to_fixed16(1.0f) == 65536 && to_fixed16(-1.0f) == -65536 && to_fixed16(0.5f) == 32768);
	CHECK(to_fixed16(0.99999f) == 65535);                         // dipotong ke nol, bukan dibulatkan
	CHECK(to_fixed16(-0.99999f) == -65535);
	CHECK(to_fixed16(1e9f) == INT32_MAX && to_fixed16(-1e9f) == INT32_MIN);  // saturasi (fcvtzs)
	CHECK(to_fixed16(NAN) == 0);
	for (int v = -70000; v <= 70000; v += 7) CHECK(div255(v) == v / 255);
	CHECK(div255(-254) == 0 && div255(-255) == -1 && div255(254) == 0);       // dipotong ke nol
	CHECK(mulw(INT32_MAX, 2) == -2);                                           // melingkar tanpa UB

	// ---- compute_region [BIN @0x5ef3b0] ----
	Region r;
	CHECK(compute_region(0,0,0,0, 8,6, 10,10, 8,6, true,8,6, &r) && r.dx==0 && r.dy==0 && r.w==8 && r.h==6);   // w=h=0 -> ukuran A
	CHECK(compute_region(-2,-1,5,4, 8,6, 10,10, 8,6, true,8,6, &r) && r.dx==0 && r.dy==0 && r.w==3 && r.h==3); // negatif memotong
	CHECK(compute_region(7,8,5,5, 8,6, 10,10, 8,6, true,8,6, &r) && r.dx==7 && r.dy==8 && r.w==3 && r.h==2);   // dibatasi `this`
	CHECK(!compute_region(10,0,3,3, 8,6, 10,10, 8,6, true,8,6, &r));          // seluruhnya di luar kanan
	CHECK(!compute_region(0,0,-3,3, 8,6, 10,10, 8,6, true,8,6, &r));          // lebar negatif
	CHECK(!compute_region(0,0,9,3, 8,6, 20,20, 8,6, true,8,6, &r));           // region 9 > lebar B 8 -> tidak melakukan apa-apa
	CHECK(!compute_region(0,0,3,3, 8,6, 10,10, 8,6, true,0,0, &r));           // C void (cw=ch=0) -> selalu tanpa kerja [BIN, tafsir]
	CHECK(compute_region(0,0,3,3, 8,6, 10,10, 8,6, false,0,0, &r));           // jika tabel mask tidak ada, C tidak diperiksa

	// ---- peta datar -> salinan A di region (tanpa mask), semua ukuran, ASan/UBSan [BIN + ASUMSI-INT] ----
	for (int W = 2; W <= 9; ++W) for (int H = 2; H <= 9; ++H) {
		Run t(12, 11, 12, 11, 12, 11);
		CHECK(t.go(2, 0, W, H, 1.0f, 1.0f));
		for (int y = 0; y < H; ++y) for (int x = 0; x < W; ++x) {
			uint32_t v = get(t.dst, t.tw, 2 + x, y);
			const bool lost_top_bottom = (W >= 3 && x == W - 2 && (y == 0 || y == H - 1));      // [BIN-L1] (W=3: loop atas tak berjalan sama sekali)
			const bool lost_col = (H >= 3 && y == H - 2 && (x == 0 || x == W - 1));            // [BIN-L2] (H=3: loop kolom tak berjalan)
			// dy=0: [BIN-L3] tidak membedakan; kolom tengah ditulis ke baris (1+k) = baris y yang sama
			if (lost_top_bottom || lost_col) CHECK(v == 0xDEADBEEFu);
			else CHECK(v == idpix(2 + x, y));
		}
	}

	// ---- gradien datar-x: B.b = 4*x  -> gx = 8 (interior) ; sx=1.0 -> ox = (8*65536)>>16 = 8  [ASUMSI-INT untuk interior] ----
	{
		Run t(40, 8, 40, 8, 40, 8);
		for (int y = 0; y < 8; ++y) for (int x = 0; x < 40; ++x) put(t.b, 40, x, y, (uint32_t)(4 * x));
		CHECK(t.go(0, 0, 40, 8, 1.0f, 1.0f));
		for (int y = 1; y <= 6; ++y) for (int x = 1; x <= 20; ++x) CHECK(get(t.dst, 40, x, y) == idpix(x + 8, y));   // gx=8, gy=0
		CHECK(get(t.dst, 40, 39, 0) == idpix(39, 0));          // sudut kanan atas: gx mundur=4 -> ox=4 -> 43 -> dijepit ke 39
		CHECK(get(t.dst, 40, 0, 0) == idpix(4, 0));            // sudut kiri atas: gx maju=4 -> ox=4
	}
	// ---- gradien-y: B.b = 4*y -> gy = 8 ; sy=0.5 -> oy = (8*32768)>>16 = 4 ----
	{
		Run t(16, 40, 16, 40, 16, 40);
		for (int y = 0; y < 40; ++y) for (int x = 0; x < 16; ++x) put(t.b, 16, x, y, (uint32_t)(4 * y));
		CHECK(t.go(0, 0, 16, 40, 0.0f, 0.5f));
		for (int y = 2; y <= 20; ++y) for (int x = 1; x <= 12; ++x) CHECK(get(t.dst, 16, x, y) == idpix(x, y + 4));
		CHECK(get(t.dst, 16, 5, 0) == idpix(5, 2));            // baris atas: gy maju=4 -> oy=2
	}
	// ---- mask: m=0 -> tanpa pergeseran ; m=255 -> sama dengan tanpa mask ----
	{
		Run t(40, 8, 40, 8, 40, 8); t.mask = true;
		for (int y = 0; y < 8; ++y) for (int x = 0; x < 40; ++x) { put(t.b, 40, x, y, (uint32_t)(4 * x)); put(t.c, 40, x, y, 0); }
		CHECK(t.go(0, 0, 40, 8, 1.0f, 1.0f));
		for (int x = 1; x <= 20; ++x) CHECK(get(t.dst, 40, x, 3) == idpix(x, 3));
		Run u(40, 8, 40, 8, 40, 8); u.mask = true;
		for (int y = 0; y < 8; ++y) for (int x = 0; x < 40; ++x) { put(u.b, 40, x, y, (uint32_t)(4 * x)); put(u.c, 40, x, y, 255); }
		CHECK(u.go(0, 0, 40, 8, 1.0f, 1.0f));
		for (int x = 1; x <= 20; ++x) CHECK(get(u.dst, 40, x, 3) == idpix(x + 8, 3));
	}
	// ---- [BIN-L3] dy > 0: kolom kiri/kanan menulis ke baris (1+k) tanpa dy ----
	{
		Run t(10, 12, 10, 12, 10, 12);
		CHECK(t.go(0, 3, 10, 8, 1.0f, 1.0f));                  // region baris 3..10, H=8 -> kk=0..4
		// persis binary: baris tujuan 1..5 kolom 0 ditulis (dengan sumber Y = 4..8), padahal region mulai di baris 3
		CHECK(get(t.dst, 10, 0, 1) == idpix(0, 4));
		CHECK(get(t.dst, 10, 0, 0) == 0xDEADBEEFu);
	}

	// ---- shimmerBuildMap [BIN @0x5f0010] ----
	{
		const int sw = 3, sh = 2, dw = 7, dh = 5;
		Img src = mk(sw, sh, 0); for (int y = 0; y < sh; ++y) for (int x = 0; x < sw; ++x) put(src, sw, x, y, 0x11223300u | (uint32_t)(y * 16 + x));
		Img d = mk(dw, dh, 0xAABBCC00u);
		build_map(d.data(), dw * 4, dw, dh, src.data(), sw * 4, sw, sh, 0, 0);
		for (int y = 0; y < dh; ++y) for (int x = 0; x < dw; ++x) {
			uint32_t p = get(d, dw, x, y);
			CHECK((p & 0xFF) == (uint32_t)((y % sh) * 16 + (x % sw)));          // hanya byte 0 berubah
			CHECK((p & 0xFFFFFF00u) == 0xAABBCC00u);                              // byte lain tetap
		}
		build_map(d.data(), dw * 4, dw, dh, src.data(), sw * 4, sw, sh, 1, 1);  // geser (+1,+1): tujuan(x,y)=sumber((x-1) mod, (y-1) mod)
		CHECK((get(d, dw, 0, 0) & 0xFF) == (uint32_t)(1 * 16 + 2) && (get(d, dw, 1, 1) & 0xFF) == 0u);
		build_map(d.data(), dw * 4, dw, dh, src.data(), sw * 4, sw, sh, -4, -3); // negatif -> modulo positif
		// ox=-4 -> oxm = 2 ; oy=-3 -> oym = 1 ; tujuan(0,0)=sumber((0-1) mod 2=1, (0-2) mod 3=1)
		CHECK((get(d, dw, 0, 0) & 0xFF) == (uint32_t)(1 * 16 + 1));
		build_map(d.data(), dw * 4, dw, dh, src.data(), sw * 4, 0, sh, 0, 0);   // ukuran sumber 0: tanpa kerja (DEVIASI sah)
	}

	printf(fails ? "GAGAL: %d\n" : "OK\n", fails);
	return fails != 0;
}
