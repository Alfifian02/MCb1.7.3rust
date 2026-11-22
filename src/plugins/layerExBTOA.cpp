// layerExBTOA.cpp - modul layerExBTOA.dll: enam method pada kelas Layer
//   copyRightBlueToLeftAlpha()               -- 0x5e16e8  docs/spec/layerExBTOA_copyRightBlueToLeftAlpha.md
//   copyBottomBlueToTopAlpha()               -- 0x5e1b58  docs/spec/layerExBTOA_copyBottomBlueToTopAlpha.md
//   fillAlpha()                              -- 0x5e1e34  docs/spec/layerExBTOA_fillAlpha.md
//   copyAlphaToProvince([level])             -- 0x5e1f74  docs/spec/layerExBTOA_copyAlphaToProvince.md
//   clipAlphaRect(dx, dy, src, sx, sy, w, h [, outside])  -- 0x5e2544  docs/spec/layerExBTOA_clipAlphaRect.md
//   fillByProvince(province, color)          -- 0x5e31a8  docs/spec/layerExBTOA_fillByProvince.md
// Direkonstruksi dari perilaku Kirikiroid2 1.3.9 (libgame.so arm64); inti piksel di layerExBTOACore.hpp, diuji terhadap
// eksekusi emulasi fungsi asli (tests/plugins/layerExBTOA). Nama parameter adalah perkiraan (biner tidak menyimpannya).
#include <stdio.h>
#include <stdint.h>
#include <vector>

#define NCB_MODULE_NAME TJS_W("layerExBTOA.dll")

#include "ncbind/ncbind.hpp"

#include "LayerExBase.h"
#include "LayerImpl.h"
#include "tjsNative.h"
#include "RenderManager.h"
#include "layerExBTOACore.hpp"

using namespace layerex_btoa;

// the fork's own colour conversion and the one reconstructed from the binary must be the same expression
static_assert(swap_rb(0x11223344u) == (uint32_t)TVP_REVRGB(0x11223344u), "swap_rb must equal TVP_REVRGB");

// Saklar: 0 = meniru biner (faktor blend GPU FillByProvince seperti di biner); 1 = blend alfa standar
// (SRC_ALPHA, ONE_MINUS_SRC_ALPHA) sehingga piksel yang COCOK yang diisi, seperti jalur CPU. Putuskan di 7.3 (S13).
#ifndef LAYEREX_BTOA_FILLBYPROVINCE_GPU_STD_BLEND
#define LAYEREX_BTOA_FILLBYPROVINCE_GPU_STD_BLEND 0
#endif

//---------------------------------------------------------------------------
// helper
//---------------------------------------------------------------------------
// int64 (tTVInteger) -> w-register 32-bit seperti di biner
static inline int T32(tTVInteger v) { return (int)(uint32_t)(uint64_t)v; }

// biner: gagal mengambil instance -> TVPThrowExceptionMessage("dest must be Layer.")
static tTJSNI_Layer *GetThisLayer(iTJSDispatch2 *objthis)
{
	tTJSNI_Layer *l = nullptr;
	if (!objthis || TJS_FAILED(objthis->NativeInstanceSupport(TJS_NIS_GETINSTANCE,
			tTJSNC_Layer::ClassID, (iTJSNativeInstance**)&l)) || !l)
		TVPThrowExceptionMessage(TJS_W("dest must be Layer."));
	return l;
}

// padanan helper 0x5e193c ("rowsOf"): gagal -> TVPSpecifyLayer ("Specify Layer class object")
static tTJSNI_Layer *GetLayerSpecify(iTJSDispatch2 *obj)
{
	tTJSNI_Layer *l = nullptr;
	if (!obj || TJS_FAILED(obj->NativeInstanceSupport(TJS_NIS_GETINSTANCE,
			tTJSNC_Layer::ClassID, (iTJSNativeInstance**)&l)) || !l)
		TVPThrowExceptionMessage(TVPSpecifyLayer);
	return l;
}

struct Pix { uint8_t *base; ptrdiff_t pitch; int w, h; };

// padanan rowsOf(flag=1): tekstur tujuan untuk ditulis (GetTextureForRender(false, nullptr)) -> baris linear
static bool OpenDstPixels(tTJSNI_Layer *l, Pix &p)
{
	tTVPBaseTexture *img = l->GetMainImage();
	if (!img) return false;                                   // biner: crash
	iTVPTexture2D *t = img->GetTextureForRender(false, nullptr);
	if (!t) return false;
	p.base = (uint8_t*)t->GetScanLineForWrite(0);
	p.pitch = t->GetPitch();
	p.w = (int)t->GetWidth(); p.h = (int)t->GetHeight();
	return p.base != nullptr;
}

// biner memanggil method TJS "update" pada objthis (bukan Update() native) di fillByProvince CPU dan clipAlphaRect;
// dipertahankan agar override `update` di skrip game tetap terpanggil.
static void CallUpdate(iTJSDispatch2 *objthis, tjs_int x, tjs_int y, tjs_int w, tjs_int h)
{
	tTJSVariant vx(x), vy(y), vw(w), vh(h);
	tTJSVariant *params[4] = { &vx, &vy, &vw, &vh };
	objthis->FuncCall(0, TJS_W("update"), nullptr, nullptr, 4, params, objthis);
}

// render method "CopyAlphaFromBlue" (cache global seperti di biner; blend diatur sekali saat dibuat)
static iTVPRenderMethod *GetCopyAlphaMethod(iTVPRenderManager *mgr)
{
	static uint32_t hint = 0;
	static iTVPRenderMethod *method = nullptr;
	if (!method) {
		method = mgr->GetOrCompileRenderMethod(kCopyAlphaFromBlueName, &hint, kCopyAlphaFromBlueSrc, 1);
		if (method)   // RGB tujuan tetap (ZERO, ONE), alfa tujuan := alfa shader (ONE, ZERO)
			method->SetBlendFuncSeparate(kGL_FUNC_ADD, kGL_ZERO, kGL_ONE, kGL_ONE, kGL_ZERO);
	}
	return method;
}

// jalur GPU copyRight/copyBottom: sumber = tekstur gambar utama `this`, tujuan = tekstur yang sama (alfa saja)
static void BlueToAlphaGPU(tTJSNI_Layer *_this, const tTVPRect &srcRect, const tTVPRect &dstRect)
{
	iTVPRenderManager *mgr = TVPGetRenderManager();
	iTVPRenderMethod *method = GetCopyAlphaMethod(mgr);
	if (!method) return;
	tTVPBaseTexture *img = _this->GetMainImage();
	if (!img) return;
	iTVPTexture2D *stex = img->GetTexture();
	if (!stex) return;
	tRenderTexRectArray::Element tex[] = { tRenderTexRectArray::Element(stex, srcRect) };
	iTVPTexture2D *dtex = img->GetTextureForRender(false, nullptr);   // biner: (false, nullptr), bukan IsBlendTarget()
	mgr->OperateRect(method, dtex, nullptr, dstRect, tRenderTexRectArray(tex));
}

//---------------------------------------------------------------------------
// copyRightBlueToLeftAlpha()   -- tanpa parameter (numparams diabaikan)
//---------------------------------------------------------------------------
static tjs_error CopyRightBlueToLeftAlpha(tTJSVariant * /*result*/, tjs_int /*numparams*/, tTJSVariant ** /*param*/,
                                          iTJSDispatch2 *objthis)
{
	tTJSNI_Layer *_this = GetThisLayer(objthis);
	if (TVPIsSoftwareRenderManager()) {
		Pix p;
		if (!OpenDstPixels(_this, p)) return TJS_S_OK;
		const Rect u = copy_right_blue_to_left_alpha(p.base, p.pitch, p.w, p.h);
		// biner menyetel byte +0x261 := 1 di sini. Itu BUKAN ImageModified (+0x199 tidak disetel; terverifikasi emulasi) dan tidak
		// ada padanannya di fork; tidak ditiru (ASUMSI: hanya soal sinkronisasi internal 1.3.9).
		_this->Update(tTVPRect(u.left, u.top, u.right, u.bottom), false);
	} else {
		const tjs_uint W = _this->GetImageWidth(), H = _this->GetImageHeight();
		const tjs_int half = (tjs_int)(W >> 1);
		BlueToAlphaGPU(_this, tTVPRect(half, 0, (tjs_int)(W & ~1u), (tjs_int)H), tTVPRect(0, 0, half, (tjs_int)H));
		_this->Update(tTVPRect(0, 0, half, (tjs_int)H), false);
	}
	return TJS_S_OK;
}

//---------------------------------------------------------------------------
// copyBottomBlueToTopAlpha()   -- tanpa parameter
//---------------------------------------------------------------------------
static tjs_error CopyBottomBlueToTopAlpha(tTJSVariant * /*result*/, tjs_int /*numparams*/, tTJSVariant ** /*param*/,
                                          iTJSDispatch2 *objthis)
{
	tTJSNI_Layer *_this = GetThisLayer(objthis);
	if (TVPIsSoftwareRenderManager()) {
		Pix p;
		if (!OpenDstPixels(_this, p)) return TJS_S_OK;
		const Rect u = copy_bottom_blue_to_top_alpha(p.base, p.pitch, p.w, p.h);
		// flag +0x261 := 1 di biner: lihat CopyRightBlueToLeftAlpha (tidak ditiru)
		_this->Update(tTVPRect(u.left, u.top, u.right, u.bottom), false);
	} else {
		const tjs_uint W = _this->GetImageWidth(), H = _this->GetImageHeight();
		const tjs_int half = (tjs_int)(H >> 1);
		BlueToAlphaGPU(_this, tTVPRect(0, half, (tjs_int)W, (tjs_int)(H & ~1u)), tTVPRect(0, 0, (tjs_int)W, half));
		_this->Update(tTVPRect(0, 0, (tjs_int)W, half), false);
	}
	return TJS_S_OK;
}

//---------------------------------------------------------------------------
// fillAlpha()   -- tanpa parameter: alfa area klip := 0xFF (FillMask) lalu Update(klip)
//---------------------------------------------------------------------------
static tjs_error FillAlpha(tTJSVariant * /*result*/, tjs_int /*numparams*/, tTJSVariant ** /*param*/, iTJSDispatch2 *objthis)
{
	tTJSNI_Layer *_this = GetThisLayer(objthis);
	tTVPBaseTexture *img = _this->GetMainImage();
	if (!img) return TJS_S_OK;                                 // biner: crash
	const tTVPRect clip = _this->GetClip();
	img->FillMask(clip, 0xff);                                 // hasil bool diabaikan; ImageModified tidak disetel (seperti biner)
	_this->Update(clip, false);
	return TJS_S_OK;
}

//---------------------------------------------------------------------------
// copyAlphaToProvince([level])
//   level absen/void : province := alfa
//   0..255           : province := (alfa >= level) ? 1 : 0
//   > 255            : province := 0
// Area = klip layer; gambar province dibuat bila belum ada.
//---------------------------------------------------------------------------
static tjs_error CopyAlphaToProvince(tTJSVariant * /*result*/, tjs_int numparams, tTJSVariant **param,
                                     iTJSDispatch2 *objthis)
{
	tTJSNI_Layer *_this = GetThisLayer(objthis);
	tjs_int level = -1;
	if (numparams >= 1 && param[0]->Type() != tvtVoid)
		level = T32((tTVInteger)*param[0]);                   // objek/oktet -> galat konversi (seperti biner)

	const tTVPRect clip = _this->GetClip();
	const uint8_t *src = (const uint8_t*)_this->GetMainImagePixelBuffer();
	if (!src) TVPThrowExceptionMessage(TJS_W("src has no image."));
	const tjs_int srcPitch = _this->GetMainImagePixelBufferPitch();
	uint8_t *dst = (uint8_t*)_this->GetProvinceImagePixelBufferForWrite();   // alokasi bila belum ada
	if (!dst) TVPThrowExceptionMessage(TJS_W("dst has no province image."));
	const tjs_int dstPitch = _this->GetProvinceImagePixelBufferPitch();
	if (!dstPitch) TVPThrowExceptionMessage(TJS_W("dst has no province pitch."));

	const int w = clip.right - clip.left, h = clip.bottom - clip.top;
	copy_alpha_to_province(src + (ptrdiff_t)clip.top * srcPitch + (ptrdiff_t)clip.left * 4, srcPitch,
	                       dst + (ptrdiff_t)clip.top * dstPitch + clip.left, dstPitch, w, h, level);
	_this->Update(clip, false);
	return TJS_S_OK;
}

//---------------------------------------------------------------------------
// clipAlphaRect(dx, dy, src, sx, sy, w, h [, outside])
//   alfa tujuan := alfa tujuan * alfa sumber / 255 pada bagian rect yang terlihat;
//   bila `outside` (0..255) ada, sisa area klip diberi alfa `outside`.
//   Tidak ada jalur GPU terpisah di biner: piksel diakses lewat tekstur (GetScanLineFor*).
//---------------------------------------------------------------------------
static tjs_error ClipAlphaRect(tTJSVariant * /*result*/, tjs_int numparams, tTJSVariant **param, iTJSDispatch2 *objthis)
{
	if (numparams < 7) return TJS_E_BADPARAMCOUNT;
	const tTVInteger dx = (tTVInteger)*param[0], dy = (tTVInteger)*param[1];
	tTJSVariantClosure sclo = param[2]->AsObjectClosureNoAddRef();   // bukan objek -> galat konversi (seperti biner)
	const tTVInteger sx = (tTVInteger)*param[3], sy = (tTVInteger)*param[4];
	const tTVInteger w = (tTVInteger)*param[5], h = (tTVInteger)*param[6];
	bool hasOutside = false; uint8_t outside = 0;
	if (numparams >= 8 && param[7]->Type() != tvtVoid) {
		const tTVInteger v = (tTVInteger)*param[7];
		hasOutside = clip_alpha_rect_has_outside(numparams, false, (long long)v);
		outside = (uint8_t)(v & 0xff);
	}
	const int pre = clip_alpha_rect_precheck(numparams, T32(w), T32(h));
	if (pre != 0) return pre;

	// biner: helper rowsOf(src) lalu rowsOf(this); keduanya melempar TVPSpecifyLayer bila bukan Layer.
	// Objek null tidak diperiksa di biner (crash); fork melempar pengecualian yang sama.
	tTJSNI_Layer *src = GetLayerSpecify(sclo.Object);
	tTJSNI_Layer *_this = GetLayerSpecify(objthis);

	tTVPBaseTexture *simg = src->GetMainImage();
	iTVPTexture2D *stex = simg ? simg->GetTexture() : nullptr;
	if (!stex) return TJS_S_OK;                                // biner: crash
	Pix d;
	if (!OpenDstPixels(_this, d)) return TJS_S_OK;

	const tTVPRect clip = _this->GetClip();
	const int cw = clip.right - clip.left, ch = clip.bottom - clip.top;
	// binary: layer tanpa gambar, pitch 0, atau klip kosong -> "dest must be Layer."
	if (_this->GetMainImagePixelBufferPitch() == 0 || cw < 1 || ch <= 0)
		TVPThrowExceptionMessage(TJS_W("dest must be Layer."));

	const int srcW = (int)stex->GetWidth(), srcH = (int)stex->GetHeight();
	std::vector<const uint8_t *> rows((size_t)srcH);
	for (int y = 0; y < srcH; ++y) {
		rows[y] = (const uint8_t*)stex->GetScanLineForRead((tjs_uint)y);
		if (!rows[y]) return TJS_S_OK;                         // tekstur tanpa salinan CPU: tidak ada yang bisa dibaca
	}
	// Biner menghitung alamat dasar tujuan dengan menjumlah offset byte (ct*pitch + cl*4) ke indeks array baris
	// (kesalahan di biner; hanya benar bila klip berawal di (0,0)). Fork memakai semantik yang dimaksud: asal klip.
	uint8_t *dstBase = d.base + (ptrdiff_t)clip.top * d.pitch + (ptrdiff_t)clip.left * 4;

	ClipIn in = { T32(dx), T32(dy), T32(sx), T32(sy), T32(w), T32(h), srcW, srcH, cw, ch };
	const ClipResult r = clip_alpha_rect(dstBase, d.pitch, rows.data(), in, hasOutside, outside, clip.left, clip.top);
	if (r.update) CallUpdate(objthis, clip.left + r.x, clip.top + r.y, r.w, r.h);
	return TJS_S_OK;
}

//---------------------------------------------------------------------------
// fillByProvince(province, color)
//   piksel yang province-nya == (province & 0xFF) diisi `color` (ARGB; R dan B ditukar ke urutan memori).
//---------------------------------------------------------------------------
static tjs_error FillByProvince(tTJSVariant * /*result*/, tjs_int numparams, tTJSVariant **param, iTJSDispatch2 *objthis)
{
	if (numparams < 2) return TJS_E_BADPARAMCOUNT;
	const tTVInteger provValue = (tTVInteger)*param[0];       // void -> 0; objek/oktet -> galat konversi
	const tTVInteger colorArg = (tTVInteger)*param[1];
	const uint8_t value = (uint8_t)(provValue & 0xff);
	const uint32_t color2 = swap_rb((uint32_t)(uint64_t)colorArg);

	tTJSNI_Layer *_this = GetThisLayer(objthis);
	const tTVPRect clip = _this->GetClip();
	const int cw = clip.right - clip.left, ch = clip.bottom - clip.top;
	// kekhasan biner: tanpa gambar utama, pitch 0, atau klip kosong -> pesan "must be Layer." (ekor dari "dest must be Layer.")
	if (!_this->GetMainImage() || _this->GetMainImagePixelBufferPitch() == 0 || cw < 1 || ch < 1)
		TVPThrowExceptionMessage(TJS_W("must be Layer."));

	const uint8_t *prov = (const uint8_t*)_this->GetProvinceImagePixelBuffer();
	const tjs_int provPitch = _this->GetProvinceImagePixelBufferPitch();
	// biner: tanpa gambar province membaca null+offset (crash). Fork: tidak melakukan apa pun.
	if (!prov || !provPitch) return TJS_S_OK;

	if (TVPIsSoftwareRenderManager()) {
		Pix d;
		if (!OpenDstPixels(_this, d)) return TJS_S_OK;
		fill_by_province(d.base, d.pitch, d.w, d.h, prov, provPitch, (int)_this->GetImageWidth(), (int)_this->GetImageHeight(),
		                 clip.left, clip.top, value, color2);
		// biner: method TJS update(left, top, lebarTekstur, tinggiTekstur)
		CallUpdate(objthis, clip.left, clip.top, d.w, d.h);
	} else {
		iTVPRenderManager *mgr = TVPGetRenderManager();
		static uint32_t hint = 0;
		static iTVPRenderMethod *method = nullptr;
		if (!method) {
			method = mgr->GetOrCompileRenderMethod(kFillByProvinceName, &hint, kFillByProvinceSrc, 1);
			if (method) {
#if LAYEREX_BTOA_FILLBYPROVINCE_GPU_STD_BLEND
				method->SetBlendFuncSeparate(kGL_FUNC_ADD, kGL_SRC_ALPHA, kGL_ONE_MINUS_SRC_ALPHA, kGL_ZERO, kGL_ONE);
#else
				method->SetBlendFuncSeparate(kGL_FUNC_ADD, kGL_ONE_MINUS_SRC_ALPHA, kGL_SRC_ALPHA, kGL_ZERO, kGL_ONE);   // seperti biner
#endif
			}
		}
		if (!method) return TJS_S_OK;
		static int id_opa = method->EnumParameterID("opa");
		static int id_color = method->EnumParameterID("u_color");

		// tekstur Gray 8-bit dari area klip gambar province (pointer awal = asal klip)
		const uint8_t *pstart = prov + (ptrdiff_t)clip.top * provPitch + clip.left;
		iTVPTexture2D *ptex = mgr->CreateTexture2D(pstart, provPitch, (unsigned)cw, (unsigned)ch, TVPTextureFormat::Gray);
		if (!ptex) return TJS_S_OK;
		method->SetParameterFloat(id_opa, (float)value / 255.0f);
		method->SetParameterColor4B(id_color, color2);

		tTVPRect dst(clip.left, clip.top, clip.left + cw, clip.top + ch);
		tTVPRect srect(0, 0, cw, ch);
		tRenderTexRectArray::Element tex[] = { tRenderTexRectArray::Element(ptex, srect) };
		iTVPTexture2D *dtex = _this->GetMainImage()->GetTextureForRender(method->IsBlendTarget(), &dst);
		mgr->OperateRect(method, dtex, nullptr, dst, tRenderTexRectArray(tex));
		_this->Update(dst, false);
		ptex->Release();
	}
	return TJS_S_OK;
}

//---------------------------------------------------------------------------
// registrasi (gaya layerExShrink.cpp)
//---------------------------------------------------------------------------
typedef tjs_error (*BtoaMethod)(tTJSVariant *, tjs_int, tTJSVariant **, iTJSDispatch2 *);
struct BtoaEntry { const tjs_char *name; BtoaMethod fn; };
static const BtoaEntry kEntries[] = {
	{ TJS_W("copyRightBlueToLeftAlpha"), CopyRightBlueToLeftAlpha },
	{ TJS_W("copyBottomBlueToTopAlpha"), CopyBottomBlueToTopAlpha },
	{ TJS_W("fillAlpha"),                FillAlpha },
	{ TJS_W("copyAlphaToProvince"),      CopyAlphaToProvince },
	{ TJS_W("clipAlphaRect"),            ClipAlphaRect },
	{ TJS_W("fillByProvince"),           FillByProvince },
};

static void InitPlugin_BTOA()
{
	iTJSDispatch2 *global = TVPGetScriptDispatch();
	tTJSVariant varLayer;
	global->PropGet(0, TJS_W("Layer"), nullptr, &varLayer, global);
	iTJSDispatch2 *layer = varLayer.AsObjectNoAddRef();
	if (!layer) return;
	for (size_t i = 0; i < sizeof(kEntries) / sizeof(kEntries[0]); ++i) {
		tTJSVariant var(TJSCreateNativeClassMethod(kEntries[i].fn));
		layer->PropSet(TJS_MEMBERENSURE, kEntries[i].name, nullptr, &var, layer);
	}
}

static void ExitPlugin_BTOA()
{
	iTJSDispatch2 *global = TVPGetScriptDispatch();
	tTJSVariant varLayer;
	global->PropGet(0, TJS_W("Layer"), nullptr, &varLayer, global);
	iTJSDispatch2 *layer = varLayer.AsObjectNoAddRef();
	if (!layer) return;
	for (size_t i = 0; i < sizeof(kEntries) / sizeof(kEntries[0]); ++i)
		layer->DeleteMember(0, kEntries[i].name, nullptr, layer);
}

NCB_PRE_REGIST_CALLBACK(InitPlugin_BTOA);
NCB_POST_UNREGIST_CALLBACK(ExitPlugin_BTOA);
