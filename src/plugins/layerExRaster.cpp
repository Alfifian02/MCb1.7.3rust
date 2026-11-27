// layerExRaster.cpp - Layer.copyRaster(src, maxh, lines, cycle, time)
// Reimplemented from observed behaviour of Kirikiroid2 1.3.9 (see docs/spec/layerExRaster_copyRaster.md).
#include <stdio.h>
#include <stdint.h>

#define NCB_MODULE_NAME TJS_W("layerExRaster.dll")

#include "ncbind/ncbind.hpp"

#include "LayerExBase.h"
#include "LayerImpl.h"
#include "tjsNative.h"
#include "RenderManager.h"
#include "layerExRasterCore.hpp"

using namespace layerex_raster;

//---------------------------------------------------------------------------
// copyRaster(src, maxh, lines, cycle, time)
//   src   : source layer (same size as this layer, otherwise nothing happens)
//   maxh  : maximum horizontal displacement in pixels   (parameter names are inferred)
//   lines : wave length in rows
//   cycle : animation period; the phase uses time % (3*cycle)
//   time  : tick (integer)
//---------------------------------------------------------------------------
static tjs_error CopyRaster(tTJSVariant *result, tjs_int numparams, tTJSVariant **param,
                            iTJSDispatch2 *objthis)
{
	if (!objthis) return TJS_E_NATIVECLASSCRASH;
	if (numparams < 5) return TJS_E_BADPARAMCOUNT;

	tTJSNI_Layer *_this = nullptr;
	if (TJS_FAILED(objthis->NativeInstanceSupport(TJS_NIS_GETINSTANCE,
			tTJSNC_Layer::ClassID, (iTJSNativeInstance**)&_this)) || !_this)
		return TJS_E_NATIVECLASSCRASH;

	tTJSNI_BaseLayer *src = nullptr;
	tTJSVariantClosure clo = param[0]->AsObjectClosureNoAddRef();
	if (!clo.Object ||
		TJS_FAILED(clo.Object->NativeInstanceSupport(TJS_NIS_GETINSTANCE,
			tTJSNC_Layer::ClassID, (iTJSNativeInstance**)&src)) || !src)
		TVPThrowExceptionMessage(TVPSpecifyLayer);

	const int maxh  = (int)param[1]->AsInteger();
	const int lines = (int)param[2]->AsInteger();
	const int cycle = (int)param[3]->AsInteger();
	const int64_t time = (int64_t)param[4]->AsInteger();

	const tTVPRect &clip = _this->GetClip();
	const int clipW = clip.right - clip.left;
	const int clipH = clip.bottom - clip.top;
	if (clipH < 1 || clipW < 1) return TJS_S_OK;

	if (TVPIsSoftwareRenderManager()) {
		// ---- CPU path ----
		if (_this->GetImageWidth() != src->GetImageWidth() ||
			_this->GetImageHeight() != src->GetImageHeight())
			return TJS_S_OK; // original silently does nothing on size mismatch

		const int imgH = (int)src->GetImageHeight();
		uint8_t *dbuf = (uint8_t*)_this->GetMainImagePixelBufferForWrite();
		const uint8_t *sbuf = (const uint8_t*)src->GetMainImagePixelBuffer();
		if (!dbuf || !sbuf) return TJS_S_OK;
		const int dpitch = _this->GetMainImagePixelBufferPitch();
		const int spitch = src->GetMainImagePixelBufferPitch();

		dbuf += (ptrdiff_t)clip.top * dpitch + (ptrdiff_t)clip.left * 4;
		sbuf += (ptrdiff_t)clip.top * spitch + (ptrdiff_t)clip.left * 4;

		const Wave wave = make_wave(clip.top, imgH, lines, cycle, time);
		copy_rows(dbuf, dpitch, sbuf, spitch, clipW, clipH, wave, maxh);
	} else {
		// ---- GPU path ----
		if (_this->GetWidth() != src->GetWidth() || _this->GetHeight() != src->GetHeight())
			return TJS_S_OK;

		iTVPRenderManager *mgr = TVPGetRenderManager();
		static uint32_t hint = 0;
		static iTVPRenderMethod *method = mgr->GetOrCompileRenderMethod(
			kShaderName, &hint, kShaderSrc, 1);
		if (!method) return TJS_S_OK;
		static int id_omega = method->EnumParameterID("omega");
		static int id_phase = method->EnumParameterID("phase");
		static int id_h     = method->EnumParameterID("h");

		iTVPTexture2D *stex = src->GetMainImage()->GetTexture();
		if (!stex) return TJS_S_OK;
		const double texW = (double)stex->GetWidth();
		const double texH = (double)stex->GetHeight();

		const Wave wave = make_wave(clip.top, (int)src->GetImageHeight(), lines, cycle, time);
		// the shader works in normalised coordinates: omega covers the whole texture height
		method->SetParameterFloat(id_omega, (float)(wave.step * texH));
		method->SetParameterFloat(id_phase, (float)wave.phase0);
		method->SetParameterFloat(id_h, (float)((double)maxh / texW));

		tTVPRect rc(clip.left, clip.top, clip.right, clip.bottom);
		tRenderTexRectArray::Element tex[] = { tRenderTexRectArray::Element(stex, rc) };
		iTVPTexture2D *dtex = _this->GetMainImage()->GetTextureForRender(method->IsBlendTarget(), &rc);
		mgr->OperateRect(method, dtex, nullptr, rc, tRenderTexRectArray(tex));
	}
	_this->Update();
	return TJS_S_OK;
}

//---------------------------------------------------------------------------
static void InitPlugin_Raster()
{
	iTJSDispatch2 *global = TVPGetScriptDispatch();
	tTJSVariant varLayer;
	global->PropGet(0, TJS_W("Layer"), nullptr, &varLayer, global);
	iTJSDispatch2 *layer = varLayer.AsObjectNoAddRef();
	if (layer) {
		tTJSVariant var(TJSCreateNativeClassMethod(CopyRaster));
		layer->PropSet(TJS_MEMBERENSURE, TJS_W("copyRaster"), nullptr, &var, layer);
	}
}

static void ExitPlugin_Raster()
{
	iTJSDispatch2 *global = TVPGetScriptDispatch();
	tTJSVariant varLayer;
	global->PropGet(0, TJS_W("Layer"), nullptr, &varLayer, global);
	iTJSDispatch2 *layer = varLayer.AsObjectNoAddRef();
	if (layer) layer->DeleteMember(0, TJS_W("copyRaster"), nullptr, layer);
}

NCB_PRE_REGIST_CALLBACK(InitPlugin_Raster);
NCB_POST_UNREGIST_CALLBACK(ExitPlugin_Raster);
