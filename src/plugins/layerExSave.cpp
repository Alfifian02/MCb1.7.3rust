// layerExSave.cpp - modul LayerExSave.dll, bagian A (utilitas): metode Layer (sesi S11)
//   getCropRect()                              0x5eab30  -> Dictionary {x,y,w,h} atau void   (alfa != 0)
//   getCropRectZero()                          0x5eb184  -> idem, piksel 32-bit != 0
//   getDiffRect(layer)                         0x5eb454  -> idem, beda piksel antara layer dan this
//   getDiffPixel(layer [,same [,diff]])        0x5eb86c  -> jumlah piksel beda; menulis ke this
//   oozeColor(level [,threshold=1 [,color=0]]) 0x5ebbf0  -> (tanpa nilai kembali)
//   copyBlueToAlpha(layer)                     0x5ebfec
//   isBlank(x, y, w, h)                        0x5ec190  -> 1/0 (periksa byte BIRU)
//   clearAlpha([threshold=0 [,color=0]])       0x5ec50c
// Spesifikasi: docs/spec/layerExSave_*.md (konvensi: layerExSave_common.md). Inti piksel: layerExSaveCore.hpp.
// Bagian B (penyimpanan gambar, thread, PNG, TLG5) = sesi S12, modul yang sama (nama sengaja tidak ditulis di komentar: parity 7.1 memakai grep).
// Seperti binary asli, TIDAK ada metode yang memanggil Update().
#include <stdio.h>
#include <stdint.h>
#include <vector>

#define NCB_MODULE_NAME TJS_W("LayerExSave.dll")

#include "ncbind/ncbind.hpp"

#include "LayerExBase.h"
#include "LayerImpl.h"
#include "tjsNative.h"
#include "tjsDictionary.h"
#include "layerExSaveCore.hpp"

using namespace layerex_save;

// Biner (0x5eb454 bug) memindai baris-bawah getDiffRect dari kolom KANAN, bukan kiri. 1 = meniru (default, tujuan
// recomp = perilaku 1.3.9); 0 = rect benar. Putuskan lewat uji perangkat/diferensial (rencana 7.3).
#ifndef LAYEREXSAVE_DIFFRECT_BOTTOM_FROM_RIGHT
#define LAYEREXSAVE_DIFFRECT_BOTTOM_FROM_RIGHT 1
#endif

namespace {

// Padanan pembantu biner 0x5ead0c: native instance layer + ukuran gambar utama + larik penunjuk baris.
struct Rows {
	std::vector<uint8_t*> p;
	int w, h;
	Rows() : w(0), h(0) {}
	CRows c() const { return (CRows)p.data(); }
	WRows wr() { return p.data(); }
};

static tTJSNI_BaseLayer *GetLayer(iTJSDispatch2 *obj)
{
	tTJSNI_BaseLayer *ni = nullptr;
	// binary: objek null tidak diperiksa (crash); kegagalan lain -> pengecualian "specify layer" (ASUMSI: pesan TVPSpecifyLayer)
	if (!obj || TJS_FAILED(obj->NativeInstanceSupport(TJS_NIS_GETINSTANCE,
			tTJSNC_Layer::ClassID, (iTJSNativeInstance**)&ni)) || !ni)
		TVPThrowExceptionMessage(TVPSpecifyLayer);
	return ni;
}

static tTJSNI_BaseLayer *GetLayerParam(tTJSVariant *v)
{
	tTJSVariantClosure clo = v->AsObjectClosureNoAddRef();     // bukan objek -> galat konversi (seperti biner)
	return GetLayer(clo.Object);
}

// forWrite: biner memakai jalur tulis (pointer bitmap untuk-tulis); fork: GetMainImagePixelBufferForWrite() (menyetel ImageModified).
static void MakeRows(tTJSNI_BaseLayer *ni, bool forWrite, Rows &r)
{
	r.w = (int)ni->GetImageWidth();                              // melempar bila layer tak punya gambar (biner: crash)
	r.h = (int)ni->GetImageHeight();
	uint8_t *base = forWrite ? (uint8_t*)ni->GetMainImagePixelBufferForWrite()
	                         : (uint8_t*)const_cast<void*>(ni->GetMainImagePixelBuffer());
	const ptrdiff_t pitch = (ptrdiff_t)ni->GetMainImagePixelBufferPitch();
	r.p.assign((size_t)(r.h > 0 ? r.h : 0), nullptr);
	for (int y = 0; y < r.h; ++y) r.p[(size_t)y] = base ? base + (ptrdiff_t)y * pitch : nullptr;
	if (!base && r.h > 0 && r.w > 0) { r.w = r.h = 0; r.p.clear(); }   // tanpa buffer: tidak ada yang diproses (biner: crash)
}

static void SetRect(tTJSVariant *result, const Rect &r)
{
	if (!result) return;
	iTJSDispatch2 *dic = TJSCreateDictionaryObject();
	const tTVInteger vals[4] = { r.x, r.y, r.w, r.h };
	static const tjs_char *keys[4] = { TJS_W("x"), TJS_W("y"), TJS_W("w"), TJS_W("h") };
	for (int i = 0; i < 4; ++i) {
		tTJSVariant v(vals[i]);
		dic->PropSet(TJS_MEMBERENSURE, keys[i], nullptr, &v, dic);
	}
	*result = tTJSVariant(dic, dic);
	dic->Release();
}

// ---------------------------------------------------------------------------------------------------------------
static tjs_error GetCropRect(tTJSVariant *result, tjs_int, tTJSVariant **, iTJSDispatch2 *objthis)
{
	tTJSNI_BaseLayer *ni = GetLayer(objthis);
	Rows r; MakeRows(ni, false, r);
	if (result) result->Clear();
	Rect rc;
	if (crop_rect_alpha(r.c(), r.w, r.h, &rc)) SetRect(result, rc);
	return TJS_S_OK;
}

static tjs_error GetCropRectZero(tTJSVariant *result, tjs_int, tTJSVariant **, iTJSDispatch2 *objthis)
{
	tTJSNI_BaseLayer *ni = GetLayer(objthis);
	Rows r; MakeRows(ni, false, r);
	if (result) result->Clear();
	Rect rc;
	if (crop_rect_zero(r.c(), r.w, r.h, &rc)) SetRect(result, rc);
	return TJS_S_OK;
}

static tjs_error GetDiffRect(tTJSVariant *result, tjs_int numparams, tTJSVariant **param, iTJSDispatch2 *objthis)
{
	if (numparams < 1) return TJS_E_BADPARAMCOUNT;
	tTJSNI_BaseLayer *other = GetLayerParam(param[0]);
	Rows a; MakeRows(other, false, a);
	tTJSNI_BaseLayer *self = GetLayer(objthis);
	Rows b; MakeRows(self, false, b);
	if (a.w != b.w || a.h != b.h) TVPThrowExceptionMessage(TJS_W("Different layer size."));
	if (result) result->Clear();
	Rect rc;
	if (diff_rect(a.c(), b.c(), b.w, b.h, LAYEREXSAVE_DIFFRECT_BOTTOM_FROM_RIGHT != 0, &rc)) SetRect(result, rc);
	return TJS_S_OK;
}

static tjs_error GetDiffPixel(tTJSVariant *result, tjs_int numparams, tTJSVariant **param, iTJSDispatch2 *objthis)
{
	if (numparams < 1) return TJS_E_BADPARAMCOUNT;
	// void pada param[1]/[2] = "tidak diberikan" (biner: jenis void melewati penanda)
	bool hasSame = false, hasDiff = false; uint32_t same = 0, diff = 0;
	if (numparams >= 2 && param[1]->Type() != tvtVoid) { hasSame = true; same = (uint32_t)(tTVInteger)param[1]->AsInteger(); }
	if (numparams >= 3 && param[2]->Type() != tvtVoid) { hasDiff = true; diff = (uint32_t)(tTVInteger)param[2]->AsInteger(); }
	tTJSNI_BaseLayer *other = GetLayerParam(param[0]);
	Rows a; MakeRows(other, false, a);
	tTJSNI_BaseLayer *self = GetLayer(objthis);
	Rows b; MakeRows(self, true, b);
	if (a.w != b.w || a.h != b.h) TVPThrowExceptionMessage(TJS_W("Different layer size."));
	const int64_t n = diff_pixel(a.c(), b.wr(), b.w, b.h, hasSame, same, hasDiff, diff);
	if (result) *result = tTJSVariant((tTVInteger)n);
	return TJS_S_OK;
}

static tjs_error OozeColor(tTJSVariant *, tjs_int numparams, tTJSVariant **param, iTJSDispatch2 *objthis)
{
	if (numparams < 1) return TJS_E_BADPARAMCOUNT;
	const int32_t level = (int32_t)(tTVInteger)param[0]->AsInteger();
	if (level < 1) TVPThrowExceptionMessage(TJS_W("Invalid level count."));
	// void pada param[1]/[2] -> 0 (bukan nilai bawaan); nilai bawaan hanya bila parameter tidak ada
	const uint32_t threshold = numparams >= 2 ? (uint32_t)(tTVInteger)param[1]->AsInteger() : 1u;
	const uint32_t color     = numparams >= 3 ? (uint32_t)(tTVInteger)param[2]->AsInteger() : 0u;
	tTJSNI_BaseLayer *self = GetLayer(objthis);
	Rows r; MakeRows(self, true, r);
	ooze_color(r.wr(), r.w, r.h, threshold, color);           // `level` tidak berefek pada piksel (lihat spesifikasi)
	return TJS_S_OK;
}

static tjs_error CopyBlueToAlpha(tTJSVariant *, tjs_int numparams, tTJSVariant **param, iTJSDispatch2 *objthis)
{
	// biner: tidak memeriksa numparams == 0 (membaca param[0] tak sah); fork: BADPARAMCOUNT
	if (numparams < 1) return TJS_E_BADPARAMCOUNT;
	tTJSNI_BaseLayer *src = GetLayerParam(param[0]);
	Rows s; MakeRows(src, false, s);
	tTJSNI_BaseLayer *self = GetLayer(objthis);
	Rows d; MakeRows(self, true, d);
	blue_to_alpha(s.c(), s.w, s.h, d.wr(), d.w, d.h);
	return TJS_S_OK;
}

static tjs_error IsBlank(tTJSVariant *result, tjs_int numparams, tTJSVariant **param, iTJSDispatch2 *objthis)
{
	if (numparams < 4) return TJS_E_BADPARAMCOUNT;
	tTJSNI_BaseLayer *self = GetLayer(objthis);
	Rows r; MakeRows(self, false, r);
	const int32_t x = (int32_t)(tTVInteger)param[0]->AsInteger(), y = (int32_t)(tTVInteger)param[1]->AsInteger();
	const int32_t w = (int32_t)(tTVInteger)param[2]->AsInteger(), h = (int32_t)(tTVInteger)param[3]->AsInteger();
	if (!blank_range_ok(x, y, w, h, r.w, r.h)) TVPThrowExceptionMessage(TJS_W("invalid layer range"));
	const bool blank = is_blank(r.c(), x, y, w, h);
	if (result) *result = tTJSVariant((tjs_int)(blank ? 1 : 0));
	return TJS_S_OK;
}

static tjs_error ClearAlpha(tTJSVariant *, tjs_int numparams, tTJSVariant **param, iTJSDispatch2 *objthis)
{
	// semua parameter opsional (biner tidak mengembalikan BADPARAMCOUNT); void -> 0
	const int32_t threshold = numparams >= 1 ? (int32_t)(tTVInteger)param[0]->AsInteger() : 0;
	const uint32_t color    = numparams >= 2 ? ((uint32_t)(tTVInteger)param[1]->AsInteger() & 0xFFFFFFu) : 0u;
	tTJSNI_BaseLayer *self = GetLayer(objthis);
	Rows r; MakeRows(self, true, r);
	clear_alpha(r.wr(), r.w, r.h, threshold, color);
	return TJS_S_OK;
}

struct MethodEntry { const tjs_char *name; tTJSNativeClassMethodCallback cb; };
static const MethodEntry kMethods[] = {
	{ TJS_W("getCropRect"),     GetCropRect },
	{ TJS_W("getCropRectZero"), GetCropRectZero },
	{ TJS_W("getDiffRect"),     GetDiffRect },
	{ TJS_W("getDiffPixel"),    GetDiffPixel },
	{ TJS_W("oozeColor"),       OozeColor },
	{ TJS_W("copyBlueToAlpha"), CopyBlueToAlpha },
	{ TJS_W("isBlank"),         IsBlank },
	{ TJS_W("clearAlpha"),      ClearAlpha },
};

} // namespace

//---------------------------------------------------------------------------
static void InitPlugin_SaveA()
{
	iTJSDispatch2 *global = TVPGetScriptDispatch();
	tTJSVariant varLayer;
	global->PropGet(0, TJS_W("Layer"), nullptr, &varLayer, global);
	iTJSDispatch2 *layer = varLayer.AsObjectNoAddRef();
	if (!layer) return;
	for (size_t i = 0; i < sizeof(kMethods) / sizeof(kMethods[0]); ++i) {
		tTJSVariant var(TJSCreateNativeClassMethod(kMethods[i].cb));
		layer->PropSet(TJS_MEMBERENSURE, kMethods[i].name, nullptr, &var, layer);
	}
}

static void ExitPlugin_SaveA()
{
	iTJSDispatch2 *global = TVPGetScriptDispatch();
	tTJSVariant varLayer;
	global->PropGet(0, TJS_W("Layer"), nullptr, &varLayer, global);
	iTJSDispatch2 *layer = varLayer.AsObjectNoAddRef();
	if (!layer) return;
	for (size_t i = 0; i < sizeof(kMethods) / sizeof(kMethods[0]); ++i)
		layer->DeleteMember(0, kMethods[i].name, nullptr, layer);
}

NCB_PRE_REGIST_CALLBACK(InitPlugin_SaveA);
NCB_POST_UNREGIST_CALLBACK(ExitPlugin_SaveA);
