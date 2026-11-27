// layerExShimmer.cpp - modul layerExShimmer.dll: Layer.shimmer dan Layer.shimmerBuildMap
//   shimmer(src, map [, mask [, dx, dy, w, h, sx, sy]])  -- 0x5eedb8, docs/spec/layerExShimmer_shimmer.md
//   shimmerBuildMap(src, ox, oy [, x, a, b])             -- 0x5f0010, docs/spec/layerExShimmer_shimmerBuildMap.md
// Direkonstruksi dari Kirikiroid2 1.3.9 (libgame.so arm64) berdasarkan recon_shimmer.txt.
// STATUS: SEBAGIAN. Terbukti dari disassembly: pembacaan argumen, pemotongan region, rumus piksel, tepi, shimmerBuildMap jalur 2-sumber.
//   Belum terbaca (ASUMSI, lihat spesifikasi): helper 0x5f0248 (pembuat tabel baris), worker interior 0x5f0494/0x5f0630,
//   jalur argumen ke-4 shimmerBuildMap (0x5f0798 -> di sini TJS_E_NOTIMPL), nilai bawaan argumen yang tidak diberikan.
// Seperti binary asli: tidak memanggil Update(); tidak memeriksa IsSoftware (selalu jalur CPU lewat buffer piksel utama).
#include <stdint.h>

#define NCB_MODULE_NAME TJS_W("layerExShimmer.dll")

#include "ncbind/ncbind.hpp"

#include "LayerExBase.h"
#include "LayerImpl.h"
#include "tjsNative.h"
#include "layerExShimmerCore.hpp"

using namespace layerex_shimmer;

// 1 = tabel baris "mask" diambil dari layer C (maksud yang masuk akal); 0 = dari layer B (persis binary: panggilan ke-4
// helper 0x5f0248 memakai argumen ke-2, bukan ke-3 — lihat spesifikasi, ASUMSI).
#ifndef SHIMMER_MASK_FROM_C
#define SHIMMER_MASK_FROM_C 0
#endif

static tTJSNI_BaseLayer* LayerOf(tTJSVariant *v)
{
	tTJSNI_BaseLayer *n = nullptr;
	tTJSVariantClosure clo = v->AsObjectClosureNoAddRef();
	if (!clo.Object ||
		TJS_FAILED(clo.Object->NativeInstanceSupport(TJS_NIS_GETINSTANCE,
			tTJSNC_Layer::ClassID, (iTJSNativeInstance**)&n)) || !n)
		TVPThrowExceptionMessage(TVPSpecifyLayer);
	return n;
}
static tjs_int  ArgInt(tjs_int n, tTJSVariant **p, tjs_int i)   { return i < n ? (tjs_int)p[i]->AsInteger() : 0; }
static float    ArgFloat(tjs_int n, tTJSVariant **p, tjs_int i) { return i < n ? (float)p[i]->AsReal() : 0.0f; }
static bool     ArgVoid(tjs_int n, tTJSVariant **p, tjs_int i)  { return i >= n || p[i]->Type() == tvtVoid; }

static tjs_error Shimmer(tTJSVariant *result, tjs_int numparams, tTJSVariant **param, iTJSDispatch2 *objthis)
{
	if (numparams < 2) return TJS_E_BADPARAMCOUNT;               // ASUMSI: batas bawah (wrapper ncbind belum dibaca)
	if (!objthis) return TJS_E_NATIVECLASSCRASH;
	tTJSNI_Layer *_this = nullptr;
	if (TJS_FAILED(objthis->NativeInstanceSupport(TJS_NIS_GETINSTANCE,
			tTJSNC_Layer::ClassID, (iTJSNativeInstance**)&_this)) || !_this)
		return TJS_E_NATIVECLASSCRASH;

	tTJSNI_BaseLayer *a = LayerOf(param[0]);
	tTJSNI_BaseLayer *b = LayerOf(param[1]);
	const bool has_c = !ArgVoid(numparams, param, 2);
	tTJSNI_BaseLayer *c = has_c ? LayerOf(param[2]) : nullptr;

	const int dx = (int)ArgInt(numparams, param, 3), dy = (int)ArgInt(numparams, param, 4);
	const int w  = (int)ArgInt(numparams, param, 5), h  = (int)ArgInt(numparams, param, 6);
	const float sx = ArgFloat(numparams, param, 7), sy = ArgFloat(numparams, param, 8);

	Ctx k = {};
	k.aw = (int)a->GetImageWidth();  k.ah = (int)a->GetImageHeight();
	const int bw = (int)b->GetImageWidth(), bh = (int)b->GetImageHeight();
	const int cw = has_c ? (int)c->GetImageWidth() : 0, ch = has_c ? (int)c->GetImageHeight() : 0;
	k.dw = (int)_this->GetImageWidth(); k.dh = (int)_this->GetImageHeight();
	// tabel mask selalu ada di binary (true); C void -> cw=ch=0 -> compute_region menolak (tanpa kerja)
	if (!compute_region(dx, dy, w, h, k.aw, k.ah, k.dw, k.dh, bw, bh, true, cw, ch, &k.r)) return TJS_S_OK;

	k.dst = (uint8_t*)_this->GetMainImagePixelBufferForWrite();
	k.a = (const uint8_t*)a->GetMainImagePixelBuffer();
	k.b = (const uint8_t*)b->GetMainImagePixelBuffer();
	if (!k.dst || !k.a || !k.b) return TJS_S_OK;
	k.dpitch = _this->GetMainImagePixelBufferPitch();
	k.apitch = a->GetMainImagePixelBufferPitch();
	k.bpitch = b->GetMainImagePixelBufferPitch();
#if SHIMMER_MASK_FROM_C
	k.c = (const uint8_t*)c->GetMainImagePixelBuffer(); k.cpitch = c->GetMainImagePixelBufferPitch();
#else
	k.c = k.b; k.cpitch = k.bpitch;
#endif
	if (!k.c) return TJS_S_OK;
	k.has_mask = has_c;
	k.sxf = to_fixed16(sx); k.syf = to_fixed16(sy);
	apply(k);
	return TJS_S_OK;                                              // tanpa Update(), seperti binary
}

static tjs_error ShimmerBuildMap(tTJSVariant *result, tjs_int numparams, tTJSVariant **param, iTJSDispatch2 *objthis)
{
	if (numparams < 3) return TJS_E_BADPARAMCOUNT;               // ASUMSI
	if (!objthis) return TJS_E_NATIVECLASSCRASH;
	tTJSNI_Layer *_this = nullptr;
	if (TJS_FAILED(objthis->NativeInstanceSupport(TJS_NIS_GETINSTANCE,
			tTJSNC_Layer::ClassID, (iTJSNativeInstance**)&_this)) || !_this)
		return TJS_E_NATIVECLASSCRASH;

	tTJSNI_BaseLayer *src = LayerOf(param[0]);
	const int ox = (int)ArgInt(numparams, param, 1), oy = (int)ArgInt(numparams, param, 2);
	// argumen ke-4 non-void memicu 0x5f0798 (372 B) yang belum dibaca -> tidak diimplementasikan
	if (!ArgVoid(numparams, param, 3)) return TJS_E_NOTIMPL;

	uint8_t *dbuf = (uint8_t*)_this->GetMainImagePixelBufferForWrite();
	const uint8_t *sbuf = (const uint8_t*)src->GetMainImagePixelBuffer();
	if (!dbuf || !sbuf) return TJS_S_OK;
	build_map(dbuf, (ptrdiff_t)_this->GetMainImagePixelBufferPitch(), (int)_this->GetImageWidth(), (int)_this->GetImageHeight(),
	          sbuf, (ptrdiff_t)src->GetMainImagePixelBufferPitch(), (int)src->GetImageWidth(), (int)src->GetImageHeight(), ox, oy);
	return TJS_S_OK;
}

//---------------------------------------------------------------------------
static void InitPlugin_Shimmer()
{
	iTJSDispatch2 *global = TVPGetScriptDispatch();
	tTJSVariant varLayer;
	global->PropGet(0, TJS_W("Layer"), nullptr, &varLayer, global);
	iTJSDispatch2 *layer = varLayer.AsObjectNoAddRef();
	if (layer) {
		tTJSVariant v1(TJSCreateNativeClassMethod(Shimmer));
		layer->PropSet(TJS_MEMBERENSURE, TJS_W("shimmer"), nullptr, &v1, layer);
		tTJSVariant v2(TJSCreateNativeClassMethod(ShimmerBuildMap));
		layer->PropSet(TJS_MEMBERENSURE, TJS_W("shimmerBuildMap"), nullptr, &v2, layer);
	}
}

static void ExitPlugin_Shimmer()
{
	iTJSDispatch2 *global = TVPGetScriptDispatch();
	tTJSVariant varLayer;
	global->PropGet(0, TJS_W("Layer"), nullptr, &varLayer, global);
	iTJSDispatch2 *layer = varLayer.AsObjectNoAddRef();
	if (layer) {
		layer->DeleteMember(0, TJS_W("shimmer"), nullptr, layer);
		layer->DeleteMember(0, TJS_W("shimmerBuildMap"), nullptr, layer);
	}
}

NCB_PRE_REGIST_CALLBACK(InitPlugin_Shimmer);
NCB_POST_UNREGIST_CALLBACK(ExitPlugin_Shimmer);
