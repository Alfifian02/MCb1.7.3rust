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
// Bagian B (S12; 3 method Layer + 3 method Window; inti: layerExSaveBCore.hpp): lihat blok "Bagian B" di bawah dan docs/spec/layerExSave_B_common.md.
// Seperti binary asli, TIDAK ada metode yang memanggil Update().
#include <stdio.h>
#include <stdint.h>
#include <vector>
#include <map>
#include <atomic>
#include <thread>
#include <functional>

#define NCB_MODULE_NAME TJS_W("LayerExSave.dll")

#include "ncbind/ncbind.hpp"

#include "LayerExBase.h"
#include "LayerImpl.h"
#include "tjsNative.h"
#include "tjsDictionary.h"
#include "layerExSaveCore.hpp"
#include "layerExSaveBCore.hpp"
#include "GraphicsLoaderIntf.h"
#include "StorageIntf.h"
#include "DebugIntf.h"

using namespace layerex_save;

// Kirim fungsi ke thread utama (cocos); didefinisikan di AndroidUtils.cpp.
extern void Android_PushEvents(const std::function<void()> &func);

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

// ===============================================================================================================
// Bagian B (S12): penyimpanan gambar. Spesifikasi: docs/spec/layerExSave_B_common.md dan satu berkas per method.
//   Layer.saveLayerImageTlg5(nama [,opsi])        0x5ec768   sinkron, TLG5
//   Layer.saveLayerImagePng(nama [,opsi])         0x5ecb4c   sinkron, PNG (penyusun sendiri, jalur 0x5ea138)
//   Layer.saveLayerImagePngOctet([level=1])       0x5ecc78   -> octet PNG
//   Window.startSaveLayerImage(layer [,nama [,opsi]])  0x5ea458  -> indeks tugas; thread terlepas (worker 0x5e9db8)
//   Window.cancelSaveLayerImage(indeks)           0x5eaa14   Window.stopSaveLayerImage(indeks)  0x5eaa3c
//   Callback ke skrip (Window): onSaveLayerImageProgress(indeks, persen, layer, nama), onSaveLayerImageDone(indeks, batal, layer, nama)
// ===============================================================================================================

// ASUMSI/deviasi sadar: biner tidak menangkap pengecualian di thread worker (-> std::terminate). 1 = tangkap, catat di log,
// lalu tetap laporkan selesai (batal=1); 0 = tidak menangkap (meniru biner, aplikasi mati bila file tak bisa dibuka).
#ifndef LAYEREXSAVE_WORKER_CATCH
#define LAYEREXSAVE_WORKER_CATCH 1
#endif

struct WindowState;

// Padanan struktur tugas biner (0x70 byte). Pada biner: owner(+0), layer(+8), nama(+0x1c), opsi(+0x30), flag batal(+0x44),
// indeks(+0x48), kemajuan(+0x5c). Fork: seluruh variant TJS dibuat dan dilepas di thread utama.
struct SaveTask {
	WindowState       *state;
	tTJSVariant        window;      // menahan objek Window selama tugas hidup (biner tidak; mencegah pointer menggantung)
	tTJSVariant        layer;       // layer penyimpan "saveLayer:<nama>"
	ttstr              name;
	tTJSVariant        option;
	int                index;
	std::atomic<bool>  cancelled;   // cancelSaveLayerImage / stopSaveLayerImage
	std::atomic<bool>  detached;    // stopSaveLayerImage (padanan owner = null): tidak ada callback progres
	std::atomic<int>   progress;
	bool               failed;
	SaveTask() : state(nullptr), index(0), cancelled(false), detached(false), progress(0), failed(false) {}
};

struct WindowState {
	iTJSDispatch2          *objthis;
	std::vector<SaveTask *> threads;     // hanya diakses di thread utama
	WindowState() : objthis(nullptr) {}
};

static std::map<iTJSDispatch2 *, WindowState *> &States()
{
	static std::map<iTJSDispatch2 *, WindowState *> m;   // dibiarkan sampai proses selesai (tugas bisa masih menunjuknya)
	return m;
}

static WindowState *FindState(iTJSDispatch2 *objthis)
{
	std::map<iTJSDispatch2 *, WindowState *>::iterator it = States().find(objthis);
	return it == States().end() ? nullptr : it->second;
}

static WindowState *GetState(iTJSDispatch2 *objthis)
{
	WindowState *s = FindState(objthis);
	if (!s) { s = new WindowState(); s->objthis = objthis; States()[objthis] = s; }
	return s;
}

static void CallWindowEvent(WindowState *st, const tjs_char *name, tTJSVariant **args, tjs_int n)
{
	iTJSDispatch2 *o = st->objthis;
	if (!o) return;
	try {
		o->FuncCall(0, name, nullptr, nullptr, n, args, o);   // hasil (mis. anggota tak ada) diabaikan seperti biner
	} catch (eTJS &e) {
		TVPAddLog(e.GetMessage());                              // biner tidak menangkap; di sini agar loop utama tidak mati
	}
}

// ---------------------------------------------------------------------------------------------------------------
// Opsi PNG dari kamus TJS (padanan 0x5e87a0): anggota "ada" = PropGet dengan TJS_MEMBERMUSTEXIST berhasil.
static bool DicGet(iTJSDispatch2 *dic, const tjs_char *key, tTJSVariant &v)
{
	return TJS_SUCCEEDED(dic->PropGet(TJS_MEMBERMUSTEXIST, key, nullptr, &v, dic));
}

static int32_t DicInt(iTJSDispatch2 *dic, const tjs_char *key, bool &present, int32_t def)
{
	tTJSVariant v;
	present = DicGet(dic, key, v);
	return present ? (int32_t)(tTVInteger)v.AsInteger() : def;
}

static bool DicStrEq(iTJSDispatch2 *dic, const tjs_char *key, const tjs_char *want)
{
	tTJSVariant v;
	if (!DicGet(dic, key, v)) return false;
	return ttstr(v) == ttstr(want);
}

static void ReadPngOptions(iTJSDispatch2 *dic, PngOptions &o)
{
	if (!dic) return;
	o.reso_x = DicInt(dic, TJS_W("reso_x"), o.reso_x_present, 0);
	o.reso_y = DicInt(dic, TJS_W("reso_y"), o.reso_y_present, 0);
	o.reso_unit_meter = DicStrEq(dic, TJS_W("reso_unit"), TJS_W("meter"));
	o.offs_x = DicInt(dic, TJS_W("offs_x"), o.offs_x_present, 0);
	o.offs_y = DicInt(dic, TJS_W("offs_y"), o.offs_y_present, 0);
	o.offs_unit_micrometer = DicStrEq(dic, TJS_W("offs_unit"), TJS_W("micrometer"));
	o.vpag_w = DicInt(dic, TJS_W("vpag_w"), o.vpag_w_present, 0);
	o.vpag_h = DicInt(dic, TJS_W("vpag_h"), o.vpag_h_present, 0);
	o.vpag_unit_micrometer = DicStrEq(dic, TJS_W("vpag_unit"), TJS_W("micrometer"));
	bool dummy;
	o.comp_lv = DicInt(dic, TJS_W("comp_lv"), dummy, -1);
}

// Penyusun PNG di memori. Kembali: 0 ok, 1 dibatalkan, melempar "deflate initialize" bila zlib menolak level.
static int BuildPng(tTJSNI_BaseLayer *ni, const PngOptions *po, PngProgress cb, void *ud, std::vector<uint8_t> &out)
{
	int w = (int)ni->GetImageWidth(), h = (int)ni->GetImageHeight();
	const uint8_t *base = (const uint8_t *)ni->GetMainImagePixelBuffer();
	const ptrdiff_t pitch = (ptrdiff_t)ni->GetMainImagePixelBufferPitch();
	if (!base) { w = h = 0; }                                   // biner: crash
	const int rc = png_encode(w, h, base, pitch, po, cb, ud, out);
	if (rc == PNG_DEFLATE_INIT) TVPThrowExceptionMessage(TJS_W("deflate initialize"));
	return rc;
}

// Padanan 0x5ea138 + 0x5ed5e0: kembali true bila dibatalkan (berkas tidak ditulis).
static bool SavePngFile(tTJSNI_BaseLayer *ni, const ttstr &name, iTJSDispatch2 *opt, PngProgress cb, void *ud)
{
	PngOptions po;
	ReadPngOptions(opt, po);
	std::vector<uint8_t> buf;
	if (BuildPng(ni, &po, cb, ud, buf) != PNG_OK) return true;
	tTJSBinaryStream *st = TVPCreateStream(name, TJS_BS_WRITE);
	try { st->WriteBuffer(buf.data(), (tjs_uint)buf.size()); } catch (...) { delete st; throw; }
	delete st;
	return false;
}

// Padanan jalur TLG5 (0x5ec768 dan cabang non-PNG di worker): aliran tulis dibuka dahulu, lalu pengecekan layer.
static void SaveTlg5File(iTJSDispatch2 *layerObj, const ttstr &name, iTJSDispatch2 *opt)
{
	tTJSBinaryStream *st = TVPCreateStream(name, TJS_BS_WRITE);
	try {
		tTJSNI_BaseLayer *ni = GetLayer(layerObj);
		TVPSaveAsTLG(nullptr, st, ni->GetMainImage(), TJS_W("tlg5"), opt);
	} catch (...) { delete st; throw; }
	delete st;
}

static ttstr ParamNameOrEmpty(tTJSVariant *v)
{
	// biner: void -> kosong; string -> salinan; tipe lain -> galat konversi
	const tTJSVariantType t = v->Type();
	if (t == tvtVoid) return ttstr();
	if (t != tvtString) TJSThrowVariantConvertError(*v, tvtString);
	return ttstr(*v);
}

static iTJSDispatch2 *ParamOptOrNull(tTJSVariant *v)
{
	// biner: harus objek, selain itu galat konversi
	const tTJSVariantType t = v->Type();
	if (t != tvtObject) TJSThrowVariantConvertError(*v, tvtObject);
	return v->AsObjectNoAddRef();
}

// ---------------------------------------------------------------------------------------------------------------
static tjs_error SaveLayerImageTlg5(tTJSVariant *, tjs_int numparams, tTJSVariant **param, iTJSDispatch2 *objthis)
{
	if (numparams < 1) return TJS_E_BADPARAMCOUNT;
	const ttstr name = ParamNameOrEmpty(param[0]);
	iTJSDispatch2 *opt = numparams >= 2 ? ParamOptOrNull(param[1]) : nullptr;
	SaveTlg5File(objthis, name, opt);
	return TJS_S_OK;
}

static tjs_error SaveLayerImagePng(tTJSVariant *, tjs_int numparams, tTJSVariant **param, iTJSDispatch2 *objthis)
{
	if (numparams < 1) return TJS_E_BADPARAMCOUNT;
	const ttstr name = ParamNameOrEmpty(param[0]);
	iTJSDispatch2 *opt = numparams >= 2 ? ParamOptOrNull(param[1]) : nullptr;
	tTJSNI_BaseLayer *ni = GetLayer(objthis);
	SavePngFile(ni, name, opt, nullptr, nullptr);
	return TJS_S_OK;
}

static tjs_error SaveLayerImagePngOctet(tTJSVariant *result, tjs_int numparams, tTJSVariant **param, iTJSDispatch2 *objthis)
{
	const int32_t level = numparams >= 1 ? (int32_t)(tTVInteger)param[0]->AsInteger() : 1;   // void -> 0; tanpa argumen -> 1
	if (!result) return TJS_S_OK;                                // biner: tanpa penerima hasil tidak ada pekerjaan (layer tidak diperiksa)
	*result = tTJSVariant(TJS_W(""));                            // biner mengisi string kosong dahulu
	tTJSNI_BaseLayer *ni = GetLayer(objthis);
	PngOptions po; po.comp_lv = level;                           // tanpa metadata, tanpa callback
	std::vector<uint8_t> buf;
	if (BuildPng(ni, &po, nullptr, nullptr, buf) != PNG_OK) return TJS_S_OK;
	tTJSVariantOctet *o = TJSAllocVariantOctet(buf.data(), (tjs_uint)buf.size());
	*result = tTJSVariant(o);
	o->Release();
	return TJS_S_OK;
}

// ---------------------------------------------------------------------------------------------------------------
// Tugas asinkron
static void PostMain(const std::function<void()> &f) { Android_PushEvents(f); }

// Callback kemajuan (worker): padanan 0x5e9c38. Mengembalikan flag batal; mengirim kejadian hanya bila persen berubah.
static bool OnPngProgress(int percent, void *ud)
{
	SaveTask *t = (SaveTask *)ud;
	if (percent != t->progress.load()) {
		t->progress = percent;
		if (!t->detached) {
			PostMain([t]() {                                       // padanan 0x5ed788 (thread utama)
				WindowState *st = t->state;
				if (t->index < 0 || (size_t)t->index >= st->threads.size() || st->threads[(size_t)t->index] != t) return;
				tTJSVariant vi((tjs_int)t->index), vp((tjs_int)t->progress.load());
				tTJSVariant vn(t->name);
				tTJSVariant *args[4] = { &vi, &vp, &t->layer, &vn };
				CallWindowEvent(st, TJS_W("onSaveLayerImageProgress"), args, 4);
			});
		}
	}
	return t->cancelled.load();
}

// Worker (padanan 0x5e9db8). Objek tugas dilepas SELALU di thread utama oleh lambda penutup.
static void SaveWorker(SaveTask *t)
{
#if LAYEREXSAVE_WORKER_CATCH
	try {
#endif
		iTJSDispatch2 *opt = t->option.Type() == tvtObject ? t->option.AsObjectNoAddRef() : nullptr;
		iTJSDispatch2 *layerObj = t->layer.AsObjectNoAddRef();
		const ttstr ext = TVPExtractStorageExt(t->name);
		const tjs_char *e = ext.c_str();
		if (e && is_png_ext(e, ext.GetLen())) {
			SavePngFile(GetLayer(layerObj), t->name, opt, OnPngProgress, t);
		} else {
			SaveTlg5File(layerObj, t->name, opt);
		}
#if LAYEREXSAVE_WORKER_CATCH
	} catch (...) { t->failed = true; }
#endif
	PostMain([t]() {                                               // padanan 0x5ed98c (thread utama)
		WindowState *st = t->state;
		if (t->failed) TVPAddLog(ttstr(TJS_W("LayerExSave: startSaveLayerImage failed: ")) + t->name);
		if (t->index >= 0 && (size_t)t->index < st->threads.size() && st->threads[(size_t)t->index] == t) {
			st->threads[(size_t)t->index] = nullptr;
			tTJSVariant vi((tjs_int)t->index), vc((tjs_int)((t->cancelled.load() || t->failed) ? 1 : 0));
			tTJSVariant vn(t->name);
			tTJSVariant *args[4] = { &vi, &vc, &t->layer, &vn };
			CallWindowEvent(st, TJS_W("onSaveLayerImageDone"), args, 4);
		}
		delete t;
	});
}

static tjs_error StartSaveLayerImage(tTJSVariant *result, tjs_int numparams, tTJSVariant **param, iTJSDispatch2 *objthis)
{
	if (numparams < 1) return TJS_E_BADPARAMCOUNT;               // ASUMSI: arity biner (ncbind) tidak terbaca; parameter 2-3 opsional di sini
	WindowState *st = GetState(objthis);
	const int idx = slot_acquire(st->threads);

	ttstr fname;
	if (numparams >= 2 && param[1]->Type() != tvtVoid) fname = ttstr(*param[1]);

	tTJSVariant win(objthis, objthis);
	tTJSVariant primary;
	objthis->PropGet(0, TJS_W("primaryLayer"), nullptr, &primary, objthis);
	iTJSDispatch2 *global = TVPGetScriptDispatch();
	tTJSVariant varLayerClass;
	global->PropGet(0, TJS_W("Layer"), nullptr, &varLayerClass, global);
	iTJSDispatch2 *layerClass = varLayerClass.AsObjectNoAddRef();

	tTJSVariant *cargs[2] = { &win, &primary };
	iTJSDispatch2 *newLayer = nullptr;
	if (!layerClass || TJS_FAILED(layerClass->CreateNew(0, nullptr, nullptr, &newLayer, 2, cargs, objthis)) || !newLayer)
		TVPThrowExceptionMessage(TJS_W("保存処理用レイヤの生成に失敗しました"));
	tTJSVariant saveLayer(newLayer, newLayer);
	newLayer->Release();

	tTJSVariant vname(ttstr(TJS_W("saveLayer:")) + fname);
	saveLayer.AsObjectNoAddRef()->PropSet(0, TJS_W("name"), nullptr, &vname, saveLayer.AsObjectNoAddRef());

	iTJSDispatch2 *sl = saveLayer.AsObjectNoAddRef();
	if (TJS_FAILED(sl->FuncCall(0, TJS_W("assignImages"), nullptr, nullptr, 1, param, sl)))
		TVPThrowExceptionMessage(TJS_W("保存処理用レイヤへの画像の複製に失敗しました"));

	SaveTask *t = new SaveTask();
	t->state = st; t->window = win; t->layer = saveLayer; t->name = fname;
	if (numparams >= 3) t->option = *param[2];
	t->index = idx;
	st->threads[(size_t)idx] = t;
	std::thread(SaveWorker, t).detach();                         // biner: std::thread + detach()
	if (result) *result = tTJSVariant((tjs_int)idx);
	return TJS_S_OK;
}

static tjs_error CancelSaveLayerImage(tTJSVariant *, tjs_int numparams, tTJSVariant **param, iTJSDispatch2 *objthis)
{
	if (numparams < 1) return TJS_E_BADPARAMCOUNT;
	WindowState *st = FindState(objthis);
	const int64_t idx = (int64_t)(tTVInteger)param[0]->AsInteger();
	if (st && slot_valid(st->threads.size(), idx)) {
		SaveTask *t = st->threads[(size_t)idx];
		if (t) t->cancelled = true;
	}
	return TJS_S_OK;
}

static tjs_error StopSaveLayerImage(tTJSVariant *, tjs_int numparams, tTJSVariant **param, iTJSDispatch2 *objthis)
{
	if (numparams < 1) return TJS_E_BADPARAMCOUNT;
	WindowState *st = FindState(objthis);
	const int64_t idx = (int64_t)(tTVInteger)param[0]->AsInteger();
	if (st && slot_valid(st->threads.size(), idx)) {
		SaveTask *t = st->threads[(size_t)idx];
		if (t) {
			t->detached = true;                                  // biner: owner = null
			t->cancelled = true;
			st->threads[(size_t)idx] = nullptr;
		}
	}
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
	{ TJS_W("saveLayerImageTlg5"),     SaveLayerImageTlg5 },
	{ TJS_W("saveLayerImagePng"),      SaveLayerImagePng },
	{ TJS_W("saveLayerImagePngOctet"), SaveLayerImagePngOctet },
};

// Metode kelas Window (padanan NCB_ATTACH_CLASS(WindowSaveImage, Window) pada biner).
static const MethodEntry kWindowMethods[] = {
	{ TJS_W("startSaveLayerImage"),  StartSaveLayerImage },
	{ TJS_W("cancelSaveLayerImage"), CancelSaveLayerImage },
	{ TJS_W("stopSaveLayerImage"),   StopSaveLayerImage },
};

} // namespace

//---------------------------------------------------------------------------
static void AttachMethods(const tjs_char *className, const MethodEntry *table, size_t n)
{
	iTJSDispatch2 *global = TVPGetScriptDispatch();
	tTJSVariant varClass;
	global->PropGet(0, className, nullptr, &varClass, global);
	iTJSDispatch2 *cls = varClass.AsObjectNoAddRef();
	if (!cls) return;
	for (size_t i = 0; i < n; ++i) {
		tTJSVariant var(TJSCreateNativeClassMethod(table[i].cb));
		cls->PropSet(TJS_MEMBERENSURE, table[i].name, nullptr, &var, cls);
	}
}

static void DetachMethods(const tjs_char *className, const MethodEntry *table, size_t n)
{
	iTJSDispatch2 *global = TVPGetScriptDispatch();
	tTJSVariant varClass;
	global->PropGet(0, className, nullptr, &varClass, global);
	iTJSDispatch2 *cls = varClass.AsObjectNoAddRef();
	if (!cls) return;
	for (size_t i = 0; i < n; ++i)
		cls->DeleteMember(0, table[i].name, nullptr, cls);
}

static void InitPlugin_Save()
{
	AttachMethods(TJS_W("Layer"),  kMethods,       sizeof(kMethods) / sizeof(kMethods[0]));
	AttachMethods(TJS_W("Window"), kWindowMethods, sizeof(kWindowMethods) / sizeof(kWindowMethods[0]));
}

static void ExitPlugin_Save()
{
	// Tugas yang masih berjalan dilepas dari jendelanya: tidak ada callback lagi (objek tugas tetap dilepas oleh lambda penutup).
	for (std::map<iTJSDispatch2 *, WindowState *>::iterator it = States().begin(); it != States().end(); ++it) {
		for (size_t i = 0; i < it->second->threads.size(); ++i)
			if (it->second->threads[i]) { it->second->threads[i]->detached = true; it->second->threads[i]->cancelled = true; it->second->threads[i] = nullptr; }
		it->second->objthis = nullptr;
	}
	DetachMethods(TJS_W("Layer"),  kMethods,       sizeof(kMethods) / sizeof(kMethods[0]));
	DetachMethods(TJS_W("Window"), kWindowMethods, sizeof(kWindowMethods) / sizeof(kWindowMethods[0]));
}

NCB_PRE_REGIST_CALLBACK(InitPlugin_Save);
NCB_POST_UNREGIST_CALLBACK(ExitPlugin_Save);
