// layerExShrink.cpp - modul shrinkCopy.dll: Layer.shrinkCopy dan Layer.shrinkCopyFast
//   shrinkCopy(dx, dy, dw, dh, src, sx, sy, sw, sh)  -- 0x5ffcf8, docs/spec/layerExShrink_shrinkCopy.md
//   shrinkCopyFast(src, hs [, vs])                    -- 0x60185c, docs/spec/layerExShrinkFast_shrinkCopyFast.md
// Direkonstruksi dari perilaku Kirikiroid2 1.3.9 (libgame.so arm64).
// shrinkCopy     : jalur CPU area-average bobot tetap (layerExShrinkCore.hpp); jalur GPU render method bawaan "Copy".
// shrinkCopyFast : jalur CPU rata-rata kotak integer (layerExShrinkFastCore.hpp; vertikal 0x601d90 dan horizontal
//                  0x6023ec sama-sama terbaca dari binary); jalur GPU "Copy" dengan skala. Mengubah ukuran gambar `this`.
// Catatan: seperti binary asli, kedua method TIDAK memanggil Update().
#include <stdio.h>
#include <stdint.h>
#include <vector>

#define NCB_MODULE_NAME TJS_W("shrinkCopy.dll")

#include "ncbind/ncbind.hpp"

#include "LayerExBase.h"
#include "LayerImpl.h"
#include "tjsNative.h"
#include "RenderManager.h"
#include "layerExShrinkCore.hpp"
#include "layerExShrinkFastCore.hpp"

using namespace layerex_shrink;

static tjs_error ShrinkCopy(tTJSVariant *result, tjs_int numparams, tTJSVariant **param,
                            iTJSDispatch2 *objthis)
{
	if (numparams < 9) return TJS_E_BADPARAMCOUNT;
	if (!objthis) return TJS_E_NATIVECLASSCRASH;

	tTJSNI_Layer *_this = nullptr;
	if (TJS_FAILED(objthis->NativeInstanceSupport(TJS_NIS_GETINSTANCE,
			tTJSNC_Layer::ClassID, (iTJSNativeInstance**)&_this)) || !_this)
		return TJS_E_NATIVECLASSCRASH;

	tTJSNI_BaseLayer *src = nullptr;
	tTJSVariantClosure clo = param[4]->AsObjectClosureNoAddRef();
	// binary asli tidak memeriksa objek null (crash); fork melempar pengecualian yang sama dengan kegagalan lain
	if (!clo.Object ||
		TJS_FAILED(clo.Object->NativeInstanceSupport(TJS_NIS_GETINSTANCE,
			tTJSNC_Layer::ClassID, (iTJSNativeInstance**)&src)) || !src)
		TVPThrowExceptionMessage(TVPSpecifyLayer);

	if (TVPIsSoftwareRenderManager()) {
		// ---------------- jalur CPU ----------------
		Ctx c = {};
		c.dx = param[0]->AsReal();  c.dy = param[1]->AsReal();
		c.dw = param[2]->AsReal();  c.dh = param[3]->AsReal();
		c.sx = (int)param[5]->AsInteger(); c.sy = (int)param[6]->AsInteger();
		c.sw = (int)param[7]->AsInteger(); c.sh = (int)param[8]->AsInteger();

		// urutan validasi sama dengan binary (semua -> TJS_E_INVALIDPARAM)
		if (c.sh < 1 || c.sw < 1) return TJS_E_INVALIDPARAM;
		if (c.dw <= 0.0 || c.sw < (int)c.dw) return TJS_E_INVALIDPARAM;
		if (c.dh <= 0.0 || c.sh < (int)c.dh) return TJS_E_INVALIDPARAM;

		c.srcW = (int)src->GetImageWidth();    c.srcH = (int)src->GetImageHeight();
		c.dstW = (int)_this->GetImageWidth();  c.dstH = (int)_this->GetImageHeight();
		if (!clip(c)) return TJS_S_OK;

		uint8_t *dbuf = (uint8_t*)_this->GetMainImagePixelBufferForWrite();
		const uint8_t *sbuf = (const uint8_t*)src->GetMainImagePixelBuffer();
		if (!dbuf || !sbuf) return TJS_S_OK;
		const ptrdiff_t dpitch = _this->GetMainImagePixelBufferPitch();
		const ptrdiff_t spitch = src->GetMainImagePixelBufferPitch();

		const int nx = c.outW - c.clipX, ny = c.outH - c.clipY;
		if (nx < 1 || ny < 1) return TJS_S_OK;

		std::vector<AxisEntry> xe((size_t)nx), ye((size_t)ny);
		const uint32_t fx = build_axis(xe.data(), make_axis_params(c, true),  c.clipX, c.outW);
		const uint32_t fy = build_axis(ye.data(), make_axis_params(c, false), c.clipY, c.outH);

		std::vector<const uint8_t*> rows((size_t)c.srcH);
		for (int y = 0; y < c.srcH; ++y) rows[(size_t)y] = sbuf + (ptrdiff_t)y * spitch;

		uint8_t *dst = dbuf + (ptrdiff_t)(c.iy + c.clipY) * dpitch + (ptrdiff_t)(c.ix + c.clipX) * 4;
		accumulate(dst, dpitch, xe.data(), nx, ye.data(), ny, rows.data(), fx, fy);
		return TJS_S_OK;
	}

	// ---------------- jalur GPU ----------------
	// parameter dibaca sebagai integer; tanpa validasi (seperti binary asli)
	const int dx = (int)param[0]->AsInteger(), dy = (int)param[1]->AsInteger();
	const int dw = (int)param[2]->AsInteger(), dh = (int)param[3]->AsInteger();
	const int sx = (int)param[5]->AsInteger(), sy = (int)param[6]->AsInteger();
	const int sw = (int)param[7]->AsInteger(), sh = (int)param[8]->AsInteger();

	iTVPRenderManager *mgr = TVPGetRenderManager();
	static iTVPRenderMethod *method = mgr->GetRenderMethod("Copy");
	if (!method) return TJS_S_OK;

	iTVPTexture2D *stex = src->GetMainImage()->GetTexture();
	if (!stex) return TJS_S_OK;

	tTVPRect dst(dx, dy, dx + dw, dy + dh);
	tTVPRect srect(sx, sy, sx + sw, sy + sh);
	tRenderTexRectArray::Element tex[] = { tRenderTexRectArray::Element(stex, srect) };
	iTVPTexture2D *dtex = _this->GetMainImage()->GetTextureForRender(method->IsBlendTarget(), &dst);
	mgr->OperateRect(method, dtex, nullptr, dst, tRenderTexRectArray(tex));
	return TJS_S_OK;
}

//---------------------------------------------------------------------------
// shrinkCopyFast(src, hs [, vs])  -- binary 0x60185c
// Mengecilkan `src` dengan faktor bulat hs (horizontal) dan vs (vertikal; 0/absen -> sama dengan hs) ke `this`.
// Ukuran gambar `this` diubah menjadi ceil(sw/hs) x ceil(sh/vs).
//
// GPU: di binary asli, tekstur sumber "Copy" diambil dari gambar `this` (dest), padahal ukuran sumber diambil dari
// `src`. Itu hampir pasti salah (kemungkinan bug asli, atau salah baca). Default fork: pakai tekstur `src` (maksud
// yang jelas). Definisikan SHRINKFAST_GPU_USE_DEST_TEXTURE=1 untuk meniru binary; putuskan lewat uji perangkat (7.3).
#ifndef SHRINKFAST_GPU_USE_DEST_TEXTURE
#define SHRINKFAST_GPU_USE_DEST_TEXTURE 0
#endif

static tjs_error ShrinkCopyFast(tTJSVariant *result, tjs_int numparams, tTJSVariant **param,
                                iTJSDispatch2 *objthis)
{
	if (numparams < 2) return TJS_E_BADPARAMCOUNT;
	if (!objthis) return TJS_E_NATIVECLASSCRASH;

	tTJSNI_Layer *_this = nullptr;
	if (TJS_FAILED(objthis->NativeInstanceSupport(TJS_NIS_GETINSTANCE,
			tTJSNC_Layer::ClassID, (iTJSNativeInstance**)&_this)) || !_this)
		return TJS_E_NATIVECLASSCRASH;

	tTJSNI_BaseLayer *src = nullptr;
	tTJSVariantClosure clo = param[0]->AsObjectClosureNoAddRef();
	if (!clo.Object ||                                   // binary: objek null tidak diperiksa (crash)
		TJS_FAILED(clo.Object->NativeInstanceSupport(TJS_NIS_GETINSTANCE,
			tTJSNC_Layer::ClassID, (iTJSNativeInstance**)&src)) || !src)
		TVPThrowExceptionMessage(TVPSpecifyLayer);

	const int hs = (int)param[1]->AsInteger();
	const int vs = default_vs(hs, numparams >= 3 ? (int)param[2]->AsInteger() : 0);

	if (TVPIsSoftwareRenderManager()) {
		// ---------------- jalur CPU ----------------
		if (hs < 1 || vs < 1) return TJS_E_INVALIDPARAM;          // binary: cmp w23,#1 / cmp w19,#0 -> -1003
		// DEVIASI: this == src di binary mengubah ukuran sumber lebih dulu lalu membacanya (baca di luar batas).
		if (static_cast<tTJSNI_BaseLayer*>(_this) == src) return TJS_E_INVALIDPARAM;

		const int sw = (int)src->GetImageWidth(), sh = (int)src->GetImageHeight();   // dibaca SEBELUM ukuran this diubah
		const int dw = ceil_div_wrap(sw, hs), dh = ceil_div_wrap(sh, vs);
		if (dw < 1 || dh < 1) return TJS_S_OK;                                       // gambar sumber kosong
		_this->SetImageSize((tjs_uint)dw, (tjs_uint)dh);

		uint8_t *dbuf = (uint8_t*)_this->GetMainImagePixelBufferForWrite();
		const uint8_t *sbuf = (const uint8_t*)src->GetMainImagePixelBuffer();
		if (!dbuf || !sbuf) return TJS_S_OK;
		shrink_fast(dbuf, (ptrdiff_t)_this->GetMainImagePixelBufferPitch(),
		            sbuf, (ptrdiff_t)src->GetMainImagePixelBufferPitch(), sw, sh, hs, vs);
		return TJS_S_OK;                                           // tanpa Update(), seperti binary
	}

	// ---------------- jalur GPU ----------------
	const int sw = (int)src->GetImageWidth(), sh = (int)src->GetImageHeight();
	if (sw < 1 || sh < 1) return TJS_S_OK;                         // binary: return 0 tanpa kerja
	const int dw = ceil_div_wrap(sw, hs), dh = ceil_div_wrap(sh, vs);
	if (dw < 1 || dh < 1) return TJS_S_OK;                         // hs/vs 0 atau negatif -> sdiv memberi <1

	iTVPRenderManager *mgr = TVPGetRenderManager();
	static iTVPRenderMethod *method = mgr->GetRenderMethod("Copy");
	if (!method) return TJS_S_OK;

#if SHRINKFAST_GPU_USE_DEST_TEXTURE
	iTVPTexture2D *stex = _this->GetMainImage()->GetTexture();     // meniru binary apa adanya
#else
	iTVPTexture2D *stex = src->GetMainImage()->GetTexture();       // maksud yang jelas
#endif
	if (!stex) return TJS_S_OK;
	stex->AddRef();                                                // binary: ++refcount sebelum ukuran this diubah
	_this->SetImageSize((tjs_uint)dw, (tjs_uint)dh);

	tTVPRect dst(0, 0, dw, dh);
	tTVPRect srect(0, 0, sw, sh);
	tRenderTexRectArray::Element tex[] = { tRenderTexRectArray::Element(stex, srect) };
	iTVPTexture2D *dtex = _this->GetMainImage()->GetTextureForRender(method->IsBlendTarget(), &dst);
	mgr->OperateRect(method, dtex, nullptr, dst, tRenderTexRectArray(tex));
	stex->Release();
	return TJS_S_OK;
}

//---------------------------------------------------------------------------
static void InitPlugin_Shrink()
{
	iTJSDispatch2 *global = TVPGetScriptDispatch();
	tTJSVariant varLayer;
	global->PropGet(0, TJS_W("Layer"), nullptr, &varLayer, global);
	iTJSDispatch2 *layer = varLayer.AsObjectNoAddRef();
	if (layer) {
		tTJSVariant var(TJSCreateNativeClassMethod(ShrinkCopy));
		layer->PropSet(TJS_MEMBERENSURE, TJS_W("shrinkCopy"), nullptr, &var, layer);
		tTJSVariant varFast(TJSCreateNativeClassMethod(ShrinkCopyFast));
		layer->PropSet(TJS_MEMBERENSURE, TJS_W("shrinkCopyFast"), nullptr, &varFast, layer);
	}
}

static void ExitPlugin_Shrink()
{
	iTJSDispatch2 *global = TVPGetScriptDispatch();
	tTJSVariant varLayer;
	global->PropGet(0, TJS_W("Layer"), nullptr, &varLayer, global);
	iTJSDispatch2 *layer = varLayer.AsObjectNoAddRef();
	if (layer) {
		layer->DeleteMember(0, TJS_W("shrinkCopy"), nullptr, layer);
		layer->DeleteMember(0, TJS_W("shrinkCopyFast"), nullptr, layer);
	}
}

NCB_PRE_REGIST_CALLBACK(InitPlugin_Shrink);
NCB_POST_UNREGIST_CALLBACK(ExitPlugin_Shrink);
