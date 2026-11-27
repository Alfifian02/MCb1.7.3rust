// Uji inti layerExSaveBCore.hpp (S12). Bangun dan jalankan dari akar repo:
//   g++ -std=c++14 -g -fsanitize=address,undefined -I src/plugins tests/plugins/layerExSave/test_B_core.cpp -lz -o /tmp/tB && /tmp/tB [dump.png]
// Argumen opsional: tulis satu PNG contoh untuk diperiksa dengan dekoder luar (mis. Python/PIL).
#include <stdio.h>
#include <stdlib.h>
#include <string>
#include <vector>
#include "layerExSaveBCore.hpp"

using namespace layerex_save;

static int g_fail = 0, g_ok = 0;
#define CHECK(c) do { if (c) ++g_ok; else { ++g_fail; printf("FAIL %s:%d  %s\n", __FILE__, __LINE__, #c); } } while (0)

static uint32_t be32(const uint8_t *p) { return ((uint32_t)p[0] << 24) | ((uint32_t)p[1] << 16) | ((uint32_t)p[2] << 8) | p[3]; }

struct Chunk { std::string type; std::vector<uint8_t> data; bool crc_ok; };

// Dekoder minimal: tanda tangan, urutan chunk, CRC.
static bool parse(const std::vector<uint8_t> &b, std::vector<Chunk> &cs)
{
	static const uint8_t sig[8] = { 0x89, 'P', 'N', 'G', 0x0D, 0x0A, 0x1A, 0x0A };
	if (b.size() < 8 || memcmp(&b[0], sig, 8)) return false;
	size_t p = 8;
	while (p + 12 <= b.size()) {
		uint32_t n = be32(&b[p]);
		if (p + 12 + n > b.size()) return false;
		Chunk c; c.type.assign((const char *)&b[p + 4], 4);
		c.data.assign(b.begin() + p + 8, b.begin() + p + 8 + n);
		uLong crc = crc32(0L, Z_NULL, 0);
		crc = crc32(crc, &b[p + 4], n + 4);
		c.crc_ok = (be32(&b[p + 8 + n]) == (uint32_t)crc);
		cs.push_back(c);
		p += 12 + n;
	}
	return p == b.size();
}

static std::vector<uint8_t> inflate_all(const std::vector<uint8_t> &z, size_t expect)
{
	std::vector<uint8_t> o(expect + 16);
	uLongf n = (uLongf)o.size();
	if (uncompress(&o[0], &n, z.data(), (uLong)z.size()) != Z_OK) o.clear(); else o.resize(n);
	return o;
}

struct Prog { std::vector<int> v; int cancel_at; };
static bool on_prog(int pct, void *u)
{
	Prog *p = (Prog *)u;
	p->v.push_back(pct);
	return p->cancel_at >= 0 && pct >= p->cancel_at;
}

int main(int argc, char **argv)
{
	// ---- slot ----
	{
		std::vector<int *> v;
		int a = 1, b = 2, c = 3;
		CHECK(slot_acquire(v) == 0); v[0] = &a;
		CHECK(slot_acquire(v) == 1); v[1] = &b;
		CHECK(slot_acquire(v) == 2); v[2] = &c;
		CHECK(v.size() == 3);
		v[1] = nullptr;                                   // slot tengah bebas -> dipakai ulang, ukuran tetap
		CHECK(slot_acquire(v) == 1); CHECK(v.size() == 3);
		v[0] = nullptr; v[1] = &b;
		CHECK(slot_acquire(v) == 0);
		CHECK(slot_valid(3, 0)); CHECK(slot_valid(3, 2)); CHECK(!slot_valid(3, 3)); CHECK(!slot_valid(3, -1)); CHECK(!slot_valid(0, 0));
	}
	// ---- ekstensi ----
	{
		const char *e[] = { ".png", ".PNG", ".PnG", ".tlg", ".png ", "png", ".pn", "", ".pnG" };
		const bool want[] = { true, true, true, false, false, false, false, false, true };
		for (int i = 0; i < 9; ++i) CHECK(is_png_ext(e[i], strlen(e[i])) == want[i]);
		CHECK(!is_png_ext((const char *)nullptr, 0));
		const wchar_t w[] = L".PNG"; CHECK(is_png_ext(w, 4));
		const char16_t u[] = { '.', 'p', 'n', 'g' }; CHECK(is_png_ext(u, 4));
		const char16_t x[] = { '.', 'p', 'n', 0x0167 }; CHECK(!is_png_ext(x, 4));      // 0x0167 bukan 'g'
		const char16_t k[] = { '.', 'p', 'n', (char16_t)('g' + 0x100) }; CHECK(!is_png_ext(k, 4));
	}
	// ---- PNG: gambar 5x3, pitch lebih lebar dari w*4 ----
	{
		const int W = 5, H = 3, PITCH = 32;
		std::vector<uint8_t> px((size_t)PITCH * H, 0xEE);        // padding 0xEE tidak boleh ikut tersimpan
		for (int y = 0; y < H; ++y) for (int x = 0; x < W; ++x) {
			uint8_t *p = &px[(size_t)y * PITCH + x * 4];
			p[0] = (uint8_t)(10 + x); p[1] = (uint8_t)(20 + y); p[2] = (uint8_t)(30 + x + y); p[3] = (uint8_t)(200 + x);
		}
		std::vector<uint8_t> out;
		Prog pr; pr.cancel_at = -1;
		PngOptions o;
		CHECK(png_encode(W, H, px.data(), PITCH, &o, on_prog, &pr, out) == PNG_OK);
		std::vector<Chunk> cs; CHECK(parse(out, cs));
		CHECK(cs.size() == 3);                                   // IHDR, IDAT, IEND (tanpa metadata)
		if (cs.size() == 3) {
			CHECK(cs[0].type == "IHDR" && cs[0].crc_ok && cs[0].data.size() == 13);
			CHECK(be32(&cs[0].data[0]) == (uint32_t)W && be32(&cs[0].data[4]) == (uint32_t)H);
			CHECK(cs[0].data[8] == 8 && cs[0].data[9] == 6 && cs[0].data[10] == 0 && cs[0].data[11] == 0 && cs[0].data[12] == 0);
			CHECK(cs[1].type == "IDAT" && cs[1].crc_ok);
			CHECK(cs[2].type == "IEND" && cs[2].crc_ok && cs[2].data.empty());
			std::vector<uint8_t> raw = inflate_all(cs[1].data, (size_t)H * (W * 4 + 1));
			CHECK(raw.size() == (size_t)H * (W * 4 + 1));
			bool same = raw.size() == (size_t)H * (W * 4 + 1);
			for (int y = 0; same && y < H; ++y) {
				const uint8_t *r = &raw[(size_t)y * (W * 4 + 1)];
				if (r[0] != 0) same = false;                      // filter None
				for (int i = 0; i < W * 4; ++i) if (r[1 + i] != px[(size_t)y * PITCH + i]) same = false;   // byte persis, tanpa penukaran
			}
			CHECK(same);
		}
		CHECK(!pr.v.empty() && pr.v.back() == 100);
		{ int n100 = 0; for (size_t i = 0; i < pr.v.size(); ++i) n100 += (pr.v[i] == 100); CHECK(n100 == 2); }   // putaran terakhir + panggilan akhir biner
		for (size_t i = 1; i < pr.v.size(); ++i) CHECK(pr.v[i] >= pr.v[i - 1]);
		if (argc > 1) { FILE *f = fopen(argv[1], "wb"); if (f) { fwrite(out.data(), 1, out.size(), f); fclose(f); } }
	}
	// ---- PNG: metadata ----
	{
		const int W = 2, H = 2;
		uint8_t px[16] = { 1,2,3,4, 5,6,7,8, 9,10,11,12, 13,14,15,16 };
		PngOptions o;
		o.reso_x_present = true; o.reso_x = 2835; o.reso_y_present = false; o.reso_y = 0; o.reso_unit_meter = true;   // hanya x ada -> chunk tetap ditulis, y = 0
		o.offs_y_present = true; o.offs_x = 0; o.offs_y = -7; o.offs_unit_micrometer = false;
		o.vpag_w_present = true; o.vpag_h_present = true; o.vpag_w = 640; o.vpag_h = 480; o.vpag_unit_micrometer = true;
		o.comp_lv = 9;
		std::vector<uint8_t> out;
		CHECK(png_encode(W, H, px, 8, &o, nullptr, nullptr, out) == PNG_OK);
		std::vector<Chunk> cs; CHECK(parse(out, cs));
		CHECK(cs.size() == 6);
		if (cs.size() == 6) {
			CHECK(cs[1].type == "pHYs" && cs[1].crc_ok && cs[1].data.size() == 9);
			CHECK(be32(&cs[1].data[0]) == 2835 && be32(&cs[1].data[4]) == 0 && cs[1].data[8] == 1);
			CHECK(cs[2].type == "oFFs" && cs[2].data.size() == 9);
			CHECK(be32(&cs[2].data[0]) == 0 && (int32_t)be32(&cs[2].data[4]) == -7 && cs[2].data[8] == 0);
			CHECK(cs[3].type == "vpAg" && cs[3].data.size() == 9);
			CHECK(be32(&cs[3].data[0]) == 640 && be32(&cs[3].data[4]) == 480 && cs[3].data[8] == 1);
			CHECK(cs[4].type == "IDAT" && cs[5].type == "IEND");
		}
		// tanpa anggota apa pun -> tak ada chunk metadata
		PngOptions none; none.comp_lv = 0;
		std::vector<uint8_t> o2; CHECK(png_encode(W, H, px, 8, &none, nullptr, nullptr, o2) == PNG_OK);
		std::vector<Chunk> c2; CHECK(parse(o2, c2)); CHECK(c2.size() == 3);
		// level 0 (tanpa kompresi) tetap berisi data yang sama
		if (c2.size() == 3) {
			std::vector<uint8_t> raw = inflate_all(c2[1].data, 2 * 9);
			CHECK(raw.size() == 18 && raw[0] == 0 && raw[1] == 1 && raw[9] == 0 && raw[10] == 9);
		}
	}
	// ---- PNG: level tidak sah, pembatalan, gambar kosong ----
	{
		uint8_t px[4] = { 1, 2, 3, 4 };
		std::vector<uint8_t> out;
		PngOptions bad; bad.comp_lv = 42;
		CHECK(png_encode(1, 1, px, 4, &bad, nullptr, nullptr, out) == PNG_DEFLATE_INIT);   // biner: "deflate initialize"
		PngOptions oct; oct.comp_lv = 1;                                                    // PngOctet: level dari argumen, tanpa metadata
		CHECK(png_encode(1, 1, px, 4, &oct, nullptr, nullptr, out) == PNG_OK);
		CHECK(png_encode(1, 1, px, 4, nullptr, nullptr, nullptr, out) == PNG_OK);          // meta null -> level bawaan

		const int W = 300, H = 300;                                                         // > 4096 byte mentah -> beberapa putaran
		std::vector<uint8_t> big((size_t)W * H * 4);
		for (size_t i = 0; i < big.size(); ++i) big[i] = (uint8_t)((i * 2654435761u) >> 13);
		Prog pr; pr.cancel_at = 40;
		PngOptions o;
		CHECK(png_encode(W, H, big.data(), W * 4, &o, on_prog, &pr, out) == PNG_CANCELLED);
		CHECK(!pr.v.empty() && pr.v.back() >= 40 && pr.v.back() < 100);
		Prog pf; pf.cancel_at = -1;
		CHECK(png_encode(W, H, big.data(), W * 4, &o, on_prog, &pf, out) == PNG_OK);
		CHECK(pf.v.size() > 10 && pf.v.back() == 100);
		std::vector<Chunk> cs; CHECK(parse(out, cs));
		if (cs.size() == 3) {
			std::vector<uint8_t> raw = inflate_all(cs[1].data, (size_t)H * (W * 4 + 1));
			CHECK(raw.size() == (size_t)H * (W * 4 + 1));
			bool same = raw.size() == (size_t)H * (W * 4 + 1);
			for (int y = 0; same && y < H; ++y)
				for (int i = 0; i < W * 4; ++i) if (raw[(size_t)y * (W * 4 + 1) + 1 + i] != big[(size_t)y * W * 4 + i]) { same = false; break; }
			CHECK(same);
		}
		std::vector<uint8_t> e;                                                             // 0 x 0
		CHECK(png_encode(0, 0, px, 0, &o, nullptr, nullptr, e) == PNG_OK);
		std::vector<Chunk> ce; CHECK(parse(e, ce)); CHECK(ce.size() == 3);
	}
	printf("test_B_core: %d OK, %d FAIL\n", g_ok, g_fail);
	return g_fail ? 1 : 0;
}
