// layerExAlpha.cpp - modul layerExAlpha.dll (Layer.*): 28 dari 31 method Alpha, SATU kelas `layerExAlpha : layerExBase_GL`.
// Hasil penggabungan S6 (Alpha-1), S7 (Alpha-2), S8 (Alpha-3), S9 (Alpha-4), S10 (Alpha-5).
//   Alpha-1 (S6) : turnAlpha turnAlpha2 turnAlpha3 turnOutline AlphaColorRect AlphaDivide AlphaAdd AlphaAdd2 BlackForAlpha
//                  ColorAlphaAdd ColorVectorEffectAdd ExtraColorVectorEffectAdd MaskRect ExMaskRect
//   Alpha-2 (S7) : LightForAlpha LuminanceForAlpha LevelMaskRect LevelMaskRect2 MaskImageRideRect MaskImageRideRect2 MaskImageRideRect3
//   Alpha-3 (S8) : MaskImageRideRect4 MaskImageRideRect5
//   Alpha-4 (S9) : MaskImageRideRect7 MaskImageRideRect8 AlphaBorderInnerRect
//   Alpha-5 (S10): MaskImageEffectRect AlphaBorderImageCoverRect
// BELUM ada (3): AlphaBlend (0x5d5894) WhiteToBlueBlurAdd (0x5d5f98) ExMaskRect2 (0x5d6ea0).
// Nama method memakai pasangan NAME->CODE dari blok registrasi 0x5d43b0 (dibaca langsung dari biner; peta lama S6 bergeser satu slot).
// Direkonstruksi dari perilaku Kirikiroid2 1.3.9 (libgame.so arm64, 0x5d4c04..0x5dc9a4+); spesifikasi: docs/spec/layerExAlpha_*.md.
// Di binary tiap method adalah anggota kelas turunan layerExBase_GL yang didaftarkan pada Layer lewat ncbind; offset anggota yang
// dipakai (+8 _this, +0x10 lebar, +0x14 tinggi, +0x18 buffer, +0x20 pitch, +0x24..0x30 klip, vtable slot 1 = redraw) cocok dengan
// layerExBase_GL di fork. Algoritma piksel: layerExAlphaCore.hpp (diuji; tests/plugins/layerExAlpha/run_tests.sh).
// Status: sintaks diperiksa terhadap header fork; BELUM dibangun dengan NDK dan BELUM diuji di perangkat.
#include <stdio.h>
#include <stdint.h>
#include <vector>
#include <algorithm>

#include "layerExBase.hpp"
#include "ncbind/ncbind.hpp"
#include "DebugIntf.h"
#include "LayerBitmapIntf.h"

#define NCB_MODULE_NAME TJS_W("layerExAlpha.dll")

#include "layerExAlphaCore.hpp"

using namespace layerex_alpha;

// Batas n di fork (S8; binary: tanpa batas selain memori; n <= 0 melempar std::length_error yang tidak ditangkap).
// Hanya 256 entri pertama tabel yang pernah dibaca (indeks = alfa - lo), jadi batas ini tidak mengubah hasil nyata.
static const int kMaxBand = 1 << 20;

namespace {

// konstanta GL yang dipakai binary pada SetBlendFuncSeparate (nilai literal di binary; bukan header GL)
const int kFuncAdd = 0x8006, kFuncReverseSubtract = 0x800B;
const int kZero = 0, kOne = 1, kSrcAlpha = 0x302, kOneMinusDstAlpha = 0x305;

// GLSL dari .rodata binary (0x14ca468, 0x14ca4ad, 0x14ca4f6)
const char *kShaderBlack = "void main() { gl_FragColor = vec4(0,0,0,1); }\n";
const char *kShaderCopy  = "void main() { gl_FragColor = texture2D(tex0, v_texCoord0); }\n";
const char *kShaderInvA  = "void main() { gl_FragColor = vec4(0,0,0,1-texture2D(tex0, v_texCoord0).a); }\n";

// Render method singleton (dibuat sekali, seperti guard statis di binary).
iTVPRenderMethod *CompileBlend(const char *name, const char *glsl, int ntex, int func, int srcRGB, int dstRGB, int srcA, int dstA)
{
	iTVPRenderMethod *m = TVPGetRenderManager()->CompileRenderMethod(name, glsl, ntex, 0);
	return m ? m->SetBlendFuncSeparate(func, srcRGB, dstRGB, srcA, dstA) : nullptr;
}
iTVPRenderMethod *MethodTurnAlpha()  { static iTVPRenderMethod *m = CompileBlend("turnAlpha",  kShaderBlack, 0, kFuncAdd, kZero, kZero, kOneMinusDstAlpha, kZero); return m; }
iTVPRenderMethod *MethodTurnAlpha2() { static iTVPRenderMethod *m = CompileBlend("turnAlpha2", kShaderBlack, 0, kFuncAdd, kZero, kOne,  kOneMinusDstAlpha, kZero); return m; }
iTVPRenderMethod *MethodExAlphaAdd() { static iTVPRenderMethod *m = CompileBlend("ExAlphaAdd", kShaderCopy,  1, kFuncAdd, kZero, kOne, kOne, kOne); return m; }
iTVPRenderMethod *MethodExAlphaSub() { static iTVPRenderMethod *m = CompileBlend("ExAlphaSub", kShaderInvA,  1, kFuncReverseSubtract, kZero, kOne, kOne, kOne); return m; }
iTVPRenderMethod *MethodAlphaMask()
{
	static iTVPRenderMethod *m = nullptr;
	if (!m) {
		static uint32_t hint = 0;                         // binary: GetOrCompileRenderMethod("AlphaMask", &hint, ...)
		m = TVPGetRenderManager()->GetOrCompileRenderMethod("AlphaMask", &hint, kShaderCopy, 1, 0);
		if (m) m = m->SetBlendFuncSeparate(kFuncAdd, kZero, kOne, kZero, kSrcAlpha);
	}
	return m;
}

void LogMessage(const tjs_char *msg) { TVPAddLog(ttstr(msg)); }

} // namespace

struct layerExAlpha : public layerExBase_GL
{
	layerExAlpha(DispatchT obj) : layerExBase_GL(obj) {}
	virtual ~layerExAlpha() {}          // ncbind men-delete lewat tipe ini; basis tidak punya destruktor virtual

	// --- akses piksel CPU. Binary memanggil GetMainImagePixelBufferForWrite()/...Pitch() di tiap method (kecuali
	//     turnAlpha/turnAlpha2/ExMask*, yang memakai cache reset(); di fork reset() hanya mengisi buffer pada mode software,
	//     jadi di sini selalu diambil ulang). Ukuran dibatasi ke ukuran gambar (DEVIASI KEAMANAN).
	bool cpu(u8 *&buf, ptrdiff_t &pitch, int &w, int &h)
	{
		buf = (u8*)_this->GetMainImagePixelBufferForWrite();
		pitch = (ptrdiff_t)_this->GetMainImagePixelBufferPitch();
		_buffer = (BufferT)buf; _pitch = (PitchT)pitch;
		w = (int)_width; h = (int)_height;
		const int iw = (int)_this->GetImageWidth(), ih = (int)_this->GetImageHeight();
		if (w > iw) w = iw;
		if (h > ih) h = ih;
		return buf && w > 0 && h > 0;
	}

	// --- 4 method pembalik alfa / outline ----------------------------------------------------------------------
	void turnAlpha()
	{
		if (TVPIsSoftwareRenderManager()) { u8 *b; ptrdiff_t p; int w, h; if (cpu(b, p, w, h)) turn_alpha(b, p, w, h); }
		else gpuFill(MethodTurnAlpha());
		redraw();
	}
	void turnAlpha2()
	{
		if (TVPIsSoftwareRenderManager()) { u8 *b; ptrdiff_t p; int w, h; if (cpu(b, p, w, h)) turn_alpha2(b, p, w, h); }
		else gpuFill(MethodTurnAlpha2());
		redraw();
	}
	void turnAlpha3()   { u8 *b; ptrdiff_t p; int w, h; if (cpu(b, p, w, h)) turn_alpha3(b, p, w, h); redraw(); }   // binary: tanpa jalur GPU
	void turnOutline()  { u8 *b; ptrdiff_t p; int w, h; if (cpu(b, p, w, h)) turn_outline(b, p, w, h); redraw(); }  // binary: tanpa jalur GPU

	// --- operasi alfa/warna seluruh gambar (hanya CPU di binary) ------------------------------------------------
	void AlphaColorRect(int r, int g, int b_)
	{ u8 *b; ptrdiff_t p; int w, h; if (cpu(b, p, w, h)) alpha_color_rect(b, p, w, h, r, g, b_); redraw(); }
	void AlphaDivide(int th)
	{ u8 *b; ptrdiff_t p; int w, h; if (cpu(b, p, w, h)) alpha_divide(b, p, w, h, th); redraw(); }
	void AlphaAdd(int add, int flag)
	{ u8 *b; ptrdiff_t p; int w, h; if (cpu(b, p, w, h)) alpha_add(b, p, w, h, add, flag); redraw(); }
	void AlphaAdd2(int add, int thr)
	{ u8 *b; ptrdiff_t p; int w, h; if (cpu(b, p, w, h)) alpha_add2(b, p, w, h, add, thr); redraw(); }
	void BlackForAlpha()
	{ u8 *b; ptrdiff_t p; int w, h; if (cpu(b, p, w, h)) black_for_alpha(b, p, w, h); redraw(); }
	void ColorAlphaAdd(int rmax, int gmax, int bmax, int add)
	{ u8 *b; ptrdiff_t p; int w, h; if (cpu(b, p, w, h)) color_alpha_add(b, p, w, h, rmax, gmax, bmax, add); redraw(); }
	void ColorVectorEffectAdd(int r0, int r1, int g0, int g1, int b0, int b1, int add)
	{ u8 *b; ptrdiff_t p; int w, h; if (cpu(b, p, w, h)) color_vector_effect_add(b, p, w, h, r0, r1, g0, g1, b0, b1, add); redraw(); }
	void ExtraColorVectorEffectAdd(int r0, int r1, int g0, int g1, int b0, int b1, int add)
	{ u8 *b; ptrdiff_t p; int w, h; if (cpu(b, p, w, h)) extra_color_vector_effect_add(b, p, w, h, r0, r1, g0, g1, b0, b1, add); redraw(); }

	// --- MaskRect(dx, dy, src, sx, sy, sw, sh, mode) ------------------------------------------------------------
	// mode 0: salin bidang mask (alfa) src -> this lewat CopyRect(plane=TVP_BB_COPY_MASK); 1: tambah; 2: kurang; 3: kali.
	void MaskRect(int dx, int dy, iTJSDispatch2 *srcobj, int sx, int sy, int sw, int sh, int mode)
	{
		tTJSNI_BaseLayer *src = getSrc(srcobj);                 // binary: gagal -> pengecualian "Not Layer"
		const int kind = mask_kind(mode);
		if (kind == 0) {
			tTVPRect rc(sx, sy, sx + sw, sy + sh);
			_this->GetMainImage()->CopyRect(dx, dy, src->GetMainImage(), rc, TVP_BB_COPY_MASK);
		} else if (kind == 2) {
			LogMessage(TJS_W("layerExAlpha: noting type"));     // GPU: binary tak terdefinisi; fork = sama dengan CPU
		} else if (TVPIsSoftwareRenderManager()) {
			u8 *b; ptrdiff_t p; int w, h;
			const u8 *sb = (const u8*)src->GetMainImagePixelBufferForWrite();   // binary memakai versi "ForWrite" juga untuk sumber
			if (cpu(b, p, w, h) && sb)
				mask_rect_cpu(b, p, w, h, sb, (ptrdiff_t)src->GetMainImagePixelBufferPitch(), (int)src->GetImageWidth(), (int)src->GetImageHeight(),
								dx, dy, sx, sy, sw, sh, mode);
		} else {
			iTVPRenderMethod *m = mode == MASK_ADD ? MethodExAlphaAdd() : (mode == MASK_SUB ? MethodExAlphaSub() : MethodAlphaMask());
			iTVPTexture2D *stex = src->GetMainImage()->GetTexture();
			if (m && stex) {
				tTVPRect dst(dx, dy, dx + sw, dy + sh), srect(sx, sy, sx + sw, sy + sh);
				tRenderTexRectArray::Element tex[] = { tRenderTexRectArray::Element(stex, srect) };
				iTVPTexture2D *dtex = _this->GetMainImage()->GetTextureForRender(true, nullptr);
				TVPGetRenderManager()->OperateRect(m, dtex, nullptr, dst, tRenderTexRectArray(tex));
			}
		}
		redraw();
	}

	// --- ExMaskRect(dx, dy, src, <tak dipakai>, sy, sw, sh, mode) ------------------------------------------------
	// Seluruh layer diproses (lihat spesifikasi). Argumen ke-4 dibaca TJS tetapi tidak dipakai binary.
	void ExMaskRect(int dx, int dy, iTJSDispatch2 *srcobj, int /*arg4*/, int sy, int sw, int sh, int mode)
	{
		const unsigned pre = exmask_precheck((int)_width, (int)_height, dx, dy);
		if (pre) { logExMask(pre); return; }                     // binary: log, tanpa pengecualian & tanpa redraw
		tTJSNI_BaseLayer *src = getSrc(srcobj);
		const u8 *sb = (const u8*)src->GetMainImagePixelBufferForWrite();
		u8 *b; ptrdiff_t p; int w, h;
		if (!cpu(b, p, w, h) || !sb) { redraw(); return; }
		const unsigned f = exmask_rect(b, p, w, h, sb, (ptrdiff_t)src->GetMainImagePixelBufferPitch(),
										(int)src->GetImageWidth(), (int)src->GetImageHeight(), dx, dy, sy, sw, sh, mode);
		logExMask(f);
		if (!(f & EXMASK_NO_REDRAW)) redraw();
	}

	// =====================================================================================================================
	// Alpha-2 (S7), Alpha-3 (S8), Alpha-4 (S9), Alpha-5 (S10): diubah dari fungsi `tjs_error` mentah / kelas terpisah menjadi
	// anggota kelas ini, supaya SELURUH method Alpha memakai satu mekanisme registrasi (hook ncbind -> reset() tiap panggilan)
	// dan satu cara mengakses tujuan. Inti piksel tidak berubah (layerExAlphaCore.hpp).
	// Ukuran tujuan = konteks layerExBase_GL (_width/_height = ukuran LAYER, GetWidth/GetHeight), cocok dengan binary (+0x10/+0x14)
	// dan dengan spesifikasi S7/S9/S10; sebelum penggabungan S7/S10 memakai GetImageWidth/Height (sama bila layer == gambar).
	// =====================================================================================================================

	// --- Alpha-2 (S7) --------------------------------------------------------------------------------------------------
	// LightForAlpha(src) : piksel = (0,0,0, src.G)
	void LightForAlpha(iTJSDispatch2 *srcobj)
	{
		tTJSNI_BaseLayer *src = getSrc(srcobj);
		Buf d, s;
		if (dstBuf(d) && srcBuf(src, d, s)) light_for_alpha(d.p, d.pitch, s.p, s.pitch, s.w, s.h);
		redraw();
	}
	// LuminanceForAlpha(src) : piksel = (0,0,0, trunc(0.2989*B + 0.5866*G + 0.1145*R))
	void LuminanceForAlpha(iTJSDispatch2 *srcobj)
	{
		tTJSNI_BaseLayer *src = getSrc(srcobj);
		Buf d, s;
		if (dstBuf(d) && srcBuf(src, d, s)) luminance_for_alpha(d.p, d.pitch, s.p, s.pitch, s.w, s.h);
		redraw();
	}
	// LevelMaskRect(a, b) : alfa layer ini dipetakan lewat tabel "tenda" simetris di sekitar a, lebar b
	void LevelMaskRect(int a, int b)
	{
		std::vector<int> t;
		if (!level_sym_table(b, t)) TJSThrowFrom_tjs_error(TJS_E_INVALIDPARAM);
		Buf d;
		if (dstBuf(d)) level_mask_apply(d.p, d.pitch, d.w, d.h, a, b, b, t);
		redraw();
	}
	// LevelMaskRect2(a, l, r) : seperti LevelMaskRect, sisi kiri l dan sisi kanan r
	void LevelMaskRect2(int a, int l, int r)
	{
		std::vector<int> t;
		if (!level_asym_table(l, r, t)) TJSThrowFrom_tjs_error(TJS_E_INVALIDPARAM);
		Buf d;
		if (dstBuf(d)) level_mask_apply(d.p, d.pitch, d.w, d.h, a, l, r, t);
		redraw();
	}
	// MaskImageRideRect(src, a, l, r, red, green, blue)   (nama parameter diduga)
	void MaskImageRideRect(iTJSDispatch2 *srcobj, int a, int l, int r, int c1, int c2, int c3)
	{
		tTJSNI_BaseLayer *src = getSrc(srcobj);
		RideTables T;
		if (!ride2_tables(l, r, c1, c2, c3, T)) TJSThrowFrom_tjs_error(TJS_E_INVALIDPARAM);
		Buf d, s;
		if (dstBuf(d) && srcBuf(src, d, s)) ride2_apply(d.p, d.pitch, s.p, s.pitch, s.w, s.h, a, l, r, T);
		redraw();
	}
	// MaskImageRideRect2(src, a, w, red, green, blue)   (nama parameter diduga)
	void MaskImageRideRect2(iTJSDispatch2 *srcobj, int a, int w, int c1, int c2, int c3)
	{
		tTJSNI_BaseLayer *src = getSrc(srcobj);
		RideTables T;
		if (!ride1_tables(w, c1, c2, c3, T)) TJSThrowFrom_tjs_error(TJS_E_INVALIDPARAM);
		Buf d, s;
		if (dstBuf(d) && srcBuf(src, d, s)) ride1_apply(d.p, d.pitch, s.p, s.pitch, s.w, s.h, a, w, c1, c2, c3, T);
		redraw();
	}
	// MaskImageRideRect3(src, a, w, red, green, blue)   (kebalikan RideRect2: warna penuh di bawah a)
	void MaskImageRideRect3(iTJSDispatch2 *srcobj, int a, int w, int c1, int c2, int c3)
	{
		tTJSNI_BaseLayer *src = getSrc(srcobj);
		RideTables T;
		if (!ride3_tables(w, c1, c2, c3, T)) TJSThrowFrom_tjs_error(TJS_E_INVALIDPARAM);
		Buf d, s;
		if (dstBuf(d) && srcBuf(src, d, s)) ride3_apply(d.p, d.pitch, s.p, s.pitch, s.w, s.h, a, w, c1, c2, c3, T);
		redraw();
	}

	// --- Alpha-3 (S8) --------------------------------------------------------------------------------------------------
	void MaskImageRideRect4(iTJSDispatch2 *layer1, iTJSDispatch2 *layer2, int a, int n, int r, int g, int b)
	{
		const Img l1 = grab(layer1);
		const Img l2 = grab(layer2);
		if (!sizesMatch(l1, l2)) return;
		if (n < 1 || n > kMaxBand) TVPThrowExceptionMessage(TJS_W("layerExAlpha: n out of range"));   // binary: std::length_error
		u8 *buf; ptrdiff_t pitch;
		if (!destBuffer(buf, pitch)) return;
		// urutan kanal (hasil baca): r -> byte 2, g -> byte 1, b -> byte 0 (BGRA); nama parameter DIPERKIRAKAN
		ride4(buf, pitch, l1.buf, l1.pitch, l2.buf, l2.pitch, _width, _height, a, n, r, g, b);
		redraw();                        // binary: panggilan virtual [vtable+8] pada `this` setelah loop (juga bila tinggi < 1)
	}
	void MaskImageRideRect5(iTJSDispatch2 *layer1, iTJSDispatch2 *layer2, int a, int n)
	{
		const Img l1 = grab(layer1);
		const Img l2 = grab(layer2);
		if (!sizesMatch(l1, l2)) return;
		if (n < 1 || n > kMaxBand) TVPThrowExceptionMessage(TJS_W("layerExAlpha: n out of range"));   // binary: std::length_error
		u8 *buf; ptrdiff_t pitch;
		if (!destBuffer(buf, pitch)) return;
		// ANOMALI BINARY: kolom diproses 0 .. 2n-2 (ukuran tabel), BUKAN 0 .. lebar-1. Bila 2n-1 > lebar, binary membaca/menulis melewati
		// ujung baris. Default fork: dibatasi lebar. LAYEREXALPHA_RIDE5_BINARY_XLIMIT hanya untuk uji diferensial terhadap APK (7.3); TIDAK aman.
#ifdef LAYEREXALPHA_RIDE5_BINARY_XLIMIT
		const int xcount = 2 * n - 1;
#else
		const int xcount = std::min(2 * n - 1, (int)_width);
#endif
		ride5(buf, pitch, l1.buf, l1.pitch, l2.buf, l2.pitch, xcount, _height, a, n);
		redraw();
	}

	// --- Alpha-4 (S9) --------------------------------------------------------------------------------------------------
	// MaskImageRideRect7(image, mask, a, b)   (nama parameter diperkirakan)
	void MaskImageRideRect7(iTJSDispatch2 *imgobj, iTJSDispatch2 *maskobj, int a, int b)
	{
		const Img img = grab(imgobj);
		const Img mask = grab(maskobj);
		if (!sizesMatch(img, mask)) return;                     // log 3 baris, tanpa redraw (seperti binary)
		std::vector<float> tab;
		if (!ride_table(b, tab)) TJSThrowFrom_tjs_error(TJS_E_INVALIDPARAM);   // binary: std::length_error
		u8 *buf; ptrdiff_t pitch;
		if (!destBuffer(buf, pitch) || !img.buf || !mask.buf) return;
		if (_height >= 1 && _width >= 1)
			ride7(buf, pitch, img.buf, img.pitch, mask.buf, mask.pitch, _width, _height, a, b, tab);
		redraw();
	}
	// MaskImageRideRect8(image, mask, th)
	void MaskImageRideRect8(iTJSDispatch2 *imgobj, iTJSDispatch2 *maskobj, int th)
	{
		const Img img = grab(imgobj);
		const Img mask = grab(maskobj);
		if (!sizesMatch(img, mask)) return;
		u8 *buf; ptrdiff_t pitch;
		if (!destBuffer(buf, pitch) || !img.buf || !mask.buf) return;
		if (_height >= 1 && _width >= 1)
			ride8(buf, pitch, img.buf, img.pitch, mask.buf, mask.pitch, _width, _height, th);
		redraw();
	}
	// AlphaBorderInnerRect(mask, th)
	// Binary hanya membandingkan LEBAR (this vs mask). DEVIASI: mask yang lebih pendek dari this -> binary membaca di luar buffer;
	// di sini diperlakukan sebagai ukuran tak cocok (mask lebih tinggi tetap diterima seperti di binary).
	void AlphaBorderInnerRect(iTJSDispatch2 *maskobj, int th)
	{
		const Img mask = grab(maskobj);
		if (!ctxFits() || _width != mask.w || mask.h < _height) {
			logDims(TJS_W("SW:"), TJS_W(" SH:"), _width, _height);
			logDims(TJS_W("AW:"), TJS_W(" AH:"), mask.w, mask.h);
			return;
		}
		u8 *buf; ptrdiff_t pitch;
		if (!destBuffer(buf, pitch) || !mask.buf) return;
		if (_height >= 1 && _width >= 1) border_inner(buf, pitch, mask.buf, mask.pitch, _width, _height, th);
		redraw();
	}

	// --- Alpha-5 (S10) -------------------------------------------------------------------------------------------------
	// MaskImageEffectRect(src, a, l, r, red, green, blue)   (nama parameter diduga)
	void MaskImageEffectRect(iTJSDispatch2 *srcobj, int a, int l, int r, int c1, int c2, int c3)
	{
		tTJSNI_BaseLayer *src = getSrc(srcobj);
		EffectTables T;
		if (!effect_tables(l, r, c1, c2, c3, T)) TJSThrowFrom_tjs_error(TJS_E_INVALIDPARAM);
		Buf d, s;
		if (dstBuf(d) && srcBuf(src, d, s)) effect_apply(d.p, d.pitch, s.p, s.pitch, s.w, s.h, a, l, r, T);
		redraw();
	}
	// AlphaBorderImageCoverRect(A, B, C, p, q)
	//   Ketiga layer harus Layer dan berukuran sama dengan layer ini; bila tidak, aslinya mencatat tiga baris log (SW:/SH:, DW:/DH:,
	//   AW:/AH:) lalu kembali TANPA redraw. q < 1, q+p < 1 atau p-q > 254: log "Out of range", kembali tanpa redraw.
	//   Ukuran layer B tidak diperiksa di aslinya; fork memotong ke irisannya agar tidak membaca di luar buffer.
	void AlphaBorderImageCoverRect(iTJSDispatch2 *aobj, iTJSDispatch2 *bobj, iTJSDispatch2 *cobj, int p, int q)
	{
		const Img la = grab(aobj);
		const Img lb = grab(bobj);
		const Img lc = grab(cobj);
		if (!ctxFits() || _width != la.w || _width != lc.w || _height != la.h || _height != lc.h) {
			logDims(TJS_W("SW:"), TJS_W(" SH:"), _width, _height);
			logDims(TJS_W("DW:"), TJS_W(" DH:"), la.w, la.h);
			logDims(TJS_W("AW:"), TJS_W(" AH:"), lc.w, lc.h);
			return;
		}
		int L, R;
		if (!cover_window(p, q, L, R)) { LogMessage(TJS_W("Out of range")); return; }
		std::vector<float> T;
		if (!cover_table(q, L, R, T)) TJSThrowFrom_tjs_error(TJS_E_INVALIDPARAM);
		u8 *buf; ptrdiff_t pitch;
		if (!destBuffer(buf, pitch) || !la.buf || !lb.buf || !lc.buf) return;
		const int bw = std::min(_width, lb.w), bh = std::min(_height, lb.h);
		cover_apply(buf, pitch, la.buf, la.pitch, lb.buf, lb.pitch, lc.buf, lc.pitch, bw, bh, p, L, R, T);   // bw/bh = irisan dengan B
		redraw();
	}

private:
	// ---- pembantu bersama Alpha-2..5 ------------------------------------------------------------------------------------
	struct Buf { u8 *p; ptrdiff_t pitch; int w, h; };                         // tujuan / sumber tertulis (S7)
	struct Img { const u8 *buf; ptrdiff_t pitch; int w, h; };                 // layer argumen, hanya dibaca (S8)

	// tujuan: buffer tulis diambil ulang (reset() di fork hanya mengisi buffer pada mode software), ukuran = layer dibatasi gambar
	bool dstBuf(Buf &b) { return cpu(b.p, b.pitch, b.w, b.h); }
	// sumber: ukuran dipotong ke irisan dengan tujuan (aslinya membaca ukuran tujuan tanpa cek -> baca di luar buffer)
	static bool srcBuf(tTJSNI_BaseLayer *src, const Buf &d, Buf &s)
	{
		s.w = std::min(d.w, (int)src->GetImageWidth());
		s.h = std::min(d.h, (int)src->GetImageHeight());
		s.p = (u8*)const_cast<void*>(src->GetMainImagePixelBuffer());
		s.pitch = (ptrdiff_t)src->GetMainImagePixelBufferPitch();
		return s.p != nullptr;
	}
	// tujuan untuk method dengan pemeriksaan ukuran (S8/S9/S10): hanya buffer + pitch
	bool destBuffer(u8 *&buf, ptrdiff_t &pitch)
	{
		buf = (u8*)_this->GetMainImagePixelBufferForWrite();
		pitch = (ptrdiff_t)_this->GetMainImagePixelBufferPitch();
		_buffer = (BufferT)buf; _pitch = (PitchT)pitch;
		return buf != nullptr;
	}
	// layer ini lebih besar dari gambar utamanya -> binary menulis di luar buffer; diperlakukan sebagai ukuran tak cocok (DEVIASI KEAMANAN, S9)
	bool ctxFits() const { return _width <= (int)_this->GetImageWidth() && _height <= (int)_this->GetImageHeight(); }

	// Urutan seperti binary: NativeInstanceSupport (gagal -> "Not Layer"), lalu pointer piksel, pitch, lebar, tinggi gambar utama.
	// Objek null: binary crash; fork melempar pengecualian yang sama dengan kegagalan lain.
	static Img grab(iTJSDispatch2 *obj)
	{
		tTJSNI_BaseLayer *l = getSrc(obj);
		Img i;
		i.buf   = (const u8*)l->GetMainImagePixelBuffer();
		i.pitch = (ptrdiff_t)l->GetMainImagePixelBufferPitch();
		i.w     = (int)l->GetImageWidth();
		i.h     = (int)l->GetImageHeight();
		return i;
	}
	static void logDims(const tjs_char *wl, const tjs_char *hl, int w, int h)
	{
		TVPAddLog(ttstr(wl) + ttstr((tjs_int)w) + ttstr(hl) + ttstr((tjs_int)h));
	}
	// Pemeriksaan ukuran seperti binary: ukuran konteks `this` harus sama dengan ukuran gambar KEDUA layer argumen.
	// Tidak cocok -> tiga baris log (SW/SH, DW/DH, AW/AH), lalu keluar TANPA mengubah apa pun dan TANPA redraw.
	bool sizesMatch(const Img &l1, const Img &l2) const
	{
		if (ctxFits() && _width == l1.w && _width == l2.w && _height == l1.h && _height == l2.h) return true;
		logDims(TJS_W("SW:"), TJS_W(" SH:"), _width, _height);
		logDims(TJS_W("DW:"), TJS_W(" DH:"), l1.w, l1.h);
		logDims(TJS_W("AW:"), TJS_W(" AH:"), l2.w, l2.h);
		return false;
	}
	static tTJSNI_BaseLayer *getSrc(iTJSDispatch2 *obj)
	{
		tTJSNI_BaseLayer *src = nullptr;
		if (!obj || TJS_FAILED(obj->NativeInstanceSupport(TJS_NIS_GETINSTANCE, tTJSNC_Layer::ClassID, (iTJSNativeInstance**)&src)) || !src)
			TVPThrowExceptionMessage(TJS_W("Not Layer"));
		return src;
	}
	static void logExMask(unsigned f)
	{
		if (f & EXMASK_LOG_LEFT) LogMessage(TJS_W("layerExAlpha: Src is position for OutArea:[Left]"));
		if (f & EXMASK_LOG_TOP)  LogMessage(TJS_W("layerExAlpha: Src is position for OutArea:[Top]"));
		if (f & EXMASK_LOG_MODE) LogMessage(TJS_W("layerExAlpha: noting type"));
	}
	// operasi GPU pada klip (turnAlpha/turnAlpha2): OperateRect(method, texTujuan, nullptr, klip, tanpa tekstur sumber)
	void gpuFill(iTVPRenderMethod *m)
	{
		if (!m) return;
		iTVPTexture2D *dtex = _this->GetMainImage()->GetTextureForRender(true, nullptr);
		tTVPRect rc(_clipLeft, _clipTop, _clipLeft + _clipWidth, _clipTop + _clipHeight);
		TVPGetRenderManager()->OperateRect(m, dtex, nullptr, rc, tRenderTexRectArray());
	}
};

// ---------------------------------------------------------------- pendaftaran
NCB_GET_INSTANCE_HOOK(layerExAlpha)
{
	// instance getter: seperti layerEx asli, info layer disegarkan (reset) sebelum tiap method
	NCB_INSTANCE_GETTER(objthis) {
		ClassT *obj = GetNativeInstance(objthis);
		if (!obj) {
			obj = new ClassT(objthis);
			SetNativeInstance(objthis, obj);
		}
		obj->reset();
		return obj;
	}
	~NCB_GET_INSTANCE_HOOK_CLASS() {}
};

NCB_ATTACH_CLASS_WITH_HOOK(layerExAlpha, Layer) {
	NCB_METHOD(turnAlpha);
	NCB_METHOD(turnAlpha2);
	NCB_METHOD(turnAlpha3);
	NCB_METHOD(turnOutline);
	NCB_METHOD(AlphaColorRect);
	NCB_METHOD(AlphaDivide);
	NCB_METHOD(AlphaAdd);
	NCB_METHOD(AlphaAdd2);
	NCB_METHOD(BlackForAlpha);
	NCB_METHOD(ColorAlphaAdd);
	NCB_METHOD(ColorVectorEffectAdd);
	NCB_METHOD(ExtraColorVectorEffectAdd);
	NCB_METHOD(MaskRect);
	NCB_METHOD(ExMaskRect);
	NCB_METHOD(LightForAlpha);
	NCB_METHOD(LuminanceForAlpha);
	NCB_METHOD(LevelMaskRect);
	NCB_METHOD(LevelMaskRect2);
	NCB_METHOD(MaskImageRideRect);
	NCB_METHOD(MaskImageRideRect2);
	NCB_METHOD(MaskImageRideRect3);
	NCB_METHOD(MaskImageRideRect4);
	NCB_METHOD(MaskImageRideRect5);
	NCB_METHOD(MaskImageRideRect7);
	NCB_METHOD(MaskImageRideRect8);
	NCB_METHOD(AlphaBorderInnerRect);
	NCB_METHOD(MaskImageEffectRect);
	NCB_METHOD(AlphaBorderImageCoverRect);
}
