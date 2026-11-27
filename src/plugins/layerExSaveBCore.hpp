// layerExSaveBCore.hpp - inti bagian B modul LayerExSave.dll (sesi S12), tanpa dependensi TJS.
// Bergantung pada zlib saja (deflate + crc32), sama seperti biner (PLT deflateInit_/deflate/deflateEnd/crc32).
// Spesifikasi: docs/spec/layerExSave_B_common.md, layerExSave_saveLayerImagePng.md, layerExSave_startSaveLayerImage.md.
//
// Isi:
//   slot_acquire / slot_valid        padanan bagian awal `startSaveLayerImage` (0x5ea458) dan pemeriksaan indeks cancel/stop
//   is_png_ext                       padanan perbandingan ekstensi di worker (0x5e9db8): huruf kecil ASCII == ".png"
//   png_encode                       padanan penyusun PNG biner (0x5e8210 -> 0x5e843c IHDR, 0x5e87a0 metadata,
//                                    0x5e8fc4 IDAT, 0x5e9770 zlib, 0x5e91c4 chunk)
#pragma once
#include <stdint.h>
#include <stddef.h>
#include <string.h>
#include <vector>
#include <zlib.h>

namespace layerex_save {

// ---------------------------------------------------------------------------------------------------------------
// Slot tugas (vektor penunjuk di WindowSaveImage, offset +8 pada objek biner).
// Biner: cari slot null pertama; bila tidak ada, ukuran vektor + 1 dan indeks = ukuran lama.
template <class T>
static inline int slot_acquire(std::vector<T *> &v)
{
	size_t i = 0;
	while (i < v.size() && v[i]) ++i;
	if (i == v.size()) v.push_back(nullptr);
	return (int)i;
}

// cancel/stop: biner membandingkan `ukuran > indeks` bertanda; indeks NEGATIF lolos lalu membaca di luar vektor (bug biner).
// Fork menolak indeks negatif.
static inline bool slot_valid(size_t n, int64_t idx) { return idx >= 0 && (uint64_t)idx < (uint64_t)n; }

// ---------------------------------------------------------------------------------------------------------------
// Ekstensi: biner memanggil TVPExtractStorageExt (hasil termasuk titik, mis. ".PNG"), mengubah A-Z ke a-z (ASCII saja),
// lalu membandingkan dengan ".png". Tanpa ekstensi (null) -> bukan PNG -> jalur TLG5.
template <class C>
static inline bool is_png_ext(const C *ext, size_t n)
{
	static const char png[] = ".png";
	if (!ext || n != 4) return false;
	for (size_t i = 0; i < 4; ++i) {
		int c = (int)ext[i];
		if (c >= 'A' && c <= 'Z') c += 'a' - 'A';
		if (c != (int)png[i]) return false;
	}
	return true;
}

// ---------------------------------------------------------------------------------------------------------------
// Penyusun PNG
// Opsi (kamus TJS): reso_x/reso_y/reso_unit -> pHYs; offs_x/offs_y/offs_unit -> oFFs; vpag_w/vpag_h/vpag_unit -> vpAg;
// comp_lv -> level deflate (bawaan -1). Chunk ditulis bila setidaknya salah satu dari dua anggota nilai ada;
// nilai yang tidak ada = 0; byte satuan = 1 bila string satuan persis "meter" (pHYs) / "micrometer" (oFFs, vpAg), selain itu 0.
struct PngOptions {
	bool    reso_x_present, reso_y_present;
	int32_t reso_x, reso_y;
	bool    reso_unit_meter;
	bool    offs_x_present, offs_y_present;
	int32_t offs_x, offs_y;
	bool    offs_unit_micrometer;
	bool    vpag_w_present, vpag_h_present;
	int32_t vpag_w, vpag_h;
	bool    vpag_unit_micrometer;
	int32_t comp_lv;
	PngOptions()
		: reso_x_present(false), reso_y_present(false), reso_x(0), reso_y(0), reso_unit_meter(false),
		  offs_x_present(false), offs_y_present(false), offs_x(0), offs_y(0), offs_unit_micrometer(false),
		  vpag_w_present(false), vpag_h_present(false), vpag_w(0), vpag_h(0), vpag_unit_micrometer(false),
		  comp_lv(-1) {}
};

// Callback kemajuan: persen 0..100; kembalikan true untuk membatalkan (padanan 0x5e9c38 yang mengembalikan flag batal).
typedef bool (*PngProgress)(int percent, void *user);

enum { PNG_OK = 0, PNG_CANCELLED = 1, PNG_DEFLATE_INIT = -1 };

namespace png_detail {

static inline void put_be32(std::vector<uint8_t> &b, uint32_t v)
{
	b.push_back((uint8_t)(v >> 24)); b.push_back((uint8_t)(v >> 16));
	b.push_back((uint8_t)(v >> 8));  b.push_back((uint8_t)v);
}

// Chunk: panjang(BE) + tipe + data + CRC32(tipe+data)  (padanan 0x5e91c4)
static inline void put_chunk(std::vector<uint8_t> &out, const char type[4], const std::vector<uint8_t> &data)
{
	put_be32(out, (uint32_t)data.size());
	const size_t at = out.size();
	out.insert(out.end(), type, type + 4);
	out.insert(out.end(), data.begin(), data.end());
	uLong crc = crc32(0L, Z_NULL, 0);
	crc = crc32(crc, &out[at], (uInt)(out.size() - at));
	put_be32(out, (uint32_t)crc);
}

} // namespace png_detail

// Mengisi `out` dengan berkas PNG utuh: 8-bit RGBA (tipe warna 6), tanpa interlace, filter 0 pada tiap baris.
// Byte piksel disalin APA ADANYA dari memori layer (tanpa penukaran kanal) - persis biner.
// `meta` boleh null (PngOctet tidak menulis metadata dan tidak punya callback). Hasil PNG_CANCELLED: `out` tidak berisi IDAT/IEND
// (pemanggil membuangnya; file tidak ditulis).
static inline int png_encode(int w, int h, const uint8_t *pix, ptrdiff_t pitch, const PngOptions *meta,
                             PngProgress cb, void *user, std::vector<uint8_t> &out)
{
	using namespace png_detail;
	out.clear();
	static const uint8_t sig[8] = { 0x89, 'P', 'N', 'G', 0x0D, 0x0A, 0x1A, 0x0A };
	out.insert(out.end(), sig, sig + 8);

	{ // IHDR
		std::vector<uint8_t> d;
		put_be32(d, (uint32_t)w); put_be32(d, (uint32_t)h);
		d.push_back(8); d.push_back(6); d.push_back(0); d.push_back(0); d.push_back(0);
		put_chunk(out, "IHDR", d);
	}
	if (meta) {
		if (meta->reso_x_present || meta->reso_y_present) {
			std::vector<uint8_t> d;
			put_be32(d, (uint32_t)meta->reso_x); put_be32(d, (uint32_t)meta->reso_y);
			d.push_back(meta->reso_unit_meter ? 1 : 0);
			put_chunk(out, "pHYs", d);
		}
		if (meta->offs_x_present || meta->offs_y_present) {
			std::vector<uint8_t> d;
			put_be32(d, (uint32_t)meta->offs_x); put_be32(d, (uint32_t)meta->offs_y);
			d.push_back(meta->offs_unit_micrometer ? 1 : 0);
			put_chunk(out, "oFFs", d);
		}
		if (meta->vpag_w_present || meta->vpag_h_present) {
			std::vector<uint8_t> d;
			put_be32(d, (uint32_t)meta->vpag_w); put_be32(d, (uint32_t)meta->vpag_h);
			d.push_back(meta->vpag_unit_micrometer ? 1 : 0);
			put_chunk(out, "vpAg", d);
		}
	}

	// Data mentah: per baris 1 byte filter 0 + w*4 byte piksel.
	const size_t rowbytes = (size_t)(w > 0 ? w : 0) * 4;
	std::vector<uint8_t> raw;
	raw.reserve((size_t)(h > 0 ? h : 0) * (rowbytes + 1));
	for (int y = 0; y < h; ++y) {
		raw.push_back(0);
		const uint8_t *row = pix + (ptrdiff_t)y * pitch;
		raw.insert(raw.end(), row, row + rowbytes);
	}

	// zlib: umpan 4096 byte per putaran, Z_FINISH pada putaran terakhir; kemajuan = (diproses*100)/total (0 bila total 0).
	z_stream zs; memset(&zs, 0, sizeof(zs));
	const int level = meta ? meta->comp_lv : -1;
	if (deflateInit(&zs, level) != Z_OK) return PNG_DEFLATE_INIT;
	std::vector<uint8_t> comp;
	const size_t total = raw.size();
	size_t done = 0;
	bool cancelled = false;
	std::vector<uint8_t> chunkbuf(65536);
	do {
		const size_t n = (total - done) < 4096 ? (total - done) : 4096;
		const bool last = (done + n == total);
		zs.next_in = total ? (Bytef *)&raw[done] : Z_NULL;
		zs.avail_in = (uInt)n;
		int rc;
		do {
			zs.next_out = &chunkbuf[0];
			zs.avail_out = (uInt)chunkbuf.size();
			rc = deflate(&zs, last ? Z_FINISH : Z_NO_FLUSH);
			comp.insert(comp.end(), chunkbuf.begin(), chunkbuf.begin() + (chunkbuf.size() - zs.avail_out));
		} while (zs.avail_out == 0);
		done += n;
		if (cb && cb(total ? (int)((done * 100) / total) : 0, user)) { cancelled = true; break; }
		if (rc == Z_STREAM_END) break;
	} while (done < total || total == 0);
	deflateEnd(&zs);
	if (cancelled) return PNG_CANCELLED;
	if (cb) cb(100, user);                       // biner: panggilan akhir 100 setelah Z_STREAM_END

	put_chunk(out, "IDAT", comp);
	put_chunk(out, "IEND", std::vector<uint8_t>());
	return PNG_OK;
}

} // namespace layerex_save
