// motionplayer_stub.cpp  --  LOGGING STUB (Fase 0)
//
// Tujuan: menggantikan motionplayer_stub.cpp yang kosong. Stub ini TIDAK merender
// apa pun. Ia mendaftarkan kelas `Motion`, `Motion.Player`, `Motion.ResourceManager`,
// `Motion.D3DAdaptor`, `Motion.SeparateLayerAdaptor` beserta kandidat member-nya,
// lalu MENCATAT setiap panggilan (nama, argumen, urutan) lewat TVPAddLog.
// Hasil log dipakai untuk menentukan apa yang benar-benar dipanggil game.
//
// PENTING - yang BELUM terverifikasi (periksa lewat log, lalu koreksi):
//  1. Daftar member diambil dari string UTF-16 di libgame.so (jendela +-12 KB di
//     sekitar nama API). Daftarnya bisa belum lengkap (mis. play/stop/progress
//     tidak muncul di jendela itu). Member yang hilang akan muncul sebagai error
//     TJS `Member "xxx" does not exist` -> tambahkan ke daftar X-macro di bawah.
//  2. Penggolongan method vs properti adalah TEBAKAN. Kalau game memanggil sebuah
//     properti sebagai fungsi (atau sebaliknya), error TJS akan menyebut namanya.
//  3. Nilai konstanta (LayerType*, PlayFlag*, ...) adalah placeholder, bukan nilai
//     asli. Konstanta didaftarkan di `Motion` dan `Motion.Player` karena letak
//     aslinya belum diketahui.
//  4. Kelas mana memiliki member ResourceManager lain (setClearColor, registerBg,
//     ...) hanyalah dugaan dari urutan string.
//
// REVISI S17 (2026-11-25) - dasar: bytecode skrip game (system/affinesourcemotion.tjs,
// d3daffinesourcemotion.tjs, d3daffinesourceemote.tjs, motion.tjs), dibongkar dengan
// script/tjs2dis.py. Bukan hasil log runtime (log game belum pernah mencapai Motion,
// 0 baris "[motion]"). Ditambahkan:
//  - Motion.Player: +29 method dan +16 properti yang DIPANGGIL/DIBACA game pada `_player`
//    (play, stop, skip, progress, draw, serialize, ... lihat daftar di bawah). Pelacakan
//    register bersifat heuristik; beberapa nama bisa milik objek lain.
//  - Motion.ResourceManager: `load` dan `clearCache` (dipanggil motion.tjs pada
//    instance: load(path) -> hasilnya dikembalikan ke pemanggil; clearCache()).
//  - Motion.EmotePlayer (dibuat `new Motion.EmotePlayer(resourceManager)` bila
//    _storageType == "emote") dan konstanta Motion.MaskModeAlpha (NILAI PLACEHOLDER).
// Catatan: stub mendaftarkan Motion.Player.useD3D, sehingga skrip game menyetel
// Motion.enableD3D = true. Jalur D3D baru aktif bila kag.d3dMotion juga true
// (default 0 di KAGWindow); jika game kelak menyalakannya, pertimbangkan
// menghapus useD3D dari PLAYER_METHODS.
#include "ncbind/ncbind.hpp"
#include <map>
#include <string>

#define NCB_MODULE_NAME TJS_W("motionplayer.dll")

// ---------------------------------------------------------------- logging
static std::map<std::string, int> g_counts;

static ttstr fmtVar(const tTJSVariant &v) {
	switch (v.Type()) {
	case tvtVoid:    return ttstr(TJS_W("void"));
	case tvtObject:  return ttstr(TJS_W("<object>"));
	case tvtOctet:   return ttstr(TJS_W("<octet>"));
	case tvtString:  return ttstr(TJS_W("\"")) + ttstr(v) + ttstr(TJS_W("\""));
	default:         return ttstr(v); // integer / real
	}
}

// Catat 20 panggilan pertama tiap member, lalu tiap kelipatan 500.
static bool shouldLog(const char *name, int &count) {
	count = ++g_counts[name];
	return count <= 20 || (count % 500) == 0;
}

static void logCall(const char *name, tjs_int n, tTJSVariant **p, const char *kind = "call") {
	int c;
	if (!shouldLog(name, c)) return;
	ttstr line(TJS_W("[motion] "));
	line += ttstr(TJS_W(""));
	// nama ASCII -> ttstr
	for (const char *s = name; *s; ++s) line += ttstr(tjs_char(*s));
	line += ttstr(TJS_W("("));
	for (tjs_int i = 0; i < n; ++i) {
		if (i) line += ttstr(TJS_W(", "));
		line += fmtVar(*p[i]);
	}
	line += ttstr(TJS_W(")"));
	line += ttstr(TJS_W(" #")) + ttstr((tjs_int)c);
	if (kind[0] != 'c') { line += ttstr(TJS_W(" ")); for (const char *s = kind; *s; ++s) line += ttstr(tjs_char(*s)); }
	TVPAddLog(line);
}

// ---------------------------------------------------------------- kelas native
struct MotionPlayerStub {
	std::map<std::string, tTJSVariant> props; // nilai terakhir yang di-set game
};
struct MotionEmotePlayerStub { // S17: Motion.EmotePlayer, anggota sama dengan Player
	std::map<std::string, tTJSVariant> props;
};
struct MotionResourceManagerStub {};
struct MotionD3DAdaptorStub {};
struct MotionSeparateLayerAdaptorStub {};
struct MotionStub {};

// ---------------------------------------------------------------- daftar member (X-macro)
// Method Motion.Player. Awal daftar = dari rencana + string libgame.so.
#define PLAYER_METHODS(X) \
	X(setMirror) X(setBustScale) X(setHairScale) X(setPartsScale) \
	X(getVariableRange) X(getVariableFrameList) \
	X(getMainTimelineLabelList) X(getDiffTimelineLabelList) X(getLoopTimeline) \
	X(getTimelinePlaying) X(getPlayingTimelineInfoList) \
	X(isSelectorTarget) X(deactivateSelectorTarget) X(getCommandList) \
	X(getCameraOffset) X(setCameraOffset) X(setDrawAffineTranslateMatrix) X(modifyRoot) \
	X(setFlip) X(setOpacity) X(setVisible) X(setSlant) X(setZoom) \
	X(getLayerNames) X(getLayerMotion) X(getLayerGetter) X(getLayerGetterList) \
	X(releaseSyncWait) X(calcViewParam) X(skipToSync) \
	X(setStereovisionCameraPosition) X(useD3D) \
	/* S17: dipanggil game pada _player (bytecode) */ \
	X(play) X(stop) X(skip) X(progress) X(draw) X(serialize) X(unserialize) \
	X(initPhysics) X(startWind) X(setCoord) X(setScale) X(setRotate) X(setRot) X(setColor) \
	X(playTimeline) X(stopTimeline) X(fadeOutTimeline) X(isTimelinePlaying) \
	X(setTimelineBlendRatio) X(getTimelineBlendRatio) X(getVariable) X(setVariable) \
	X(getMainTimelineLabelAt) X(getDiffTimelineLabelAt) \
	X(getPlayingTimelineLabelAt) X(getPlayingTimelineFlagsAt) \
	/* dipanggil game sebagai method tanpa argumen: gpd lalu call () */ \
	X(countMainTimelines) X(countDiffTimelines) X(countPlayingTimelines)

// Properti Motion.Player (disimpan & dikembalikan apa adanya). TEBAKAN.
#define PLAYER_PROPS(X) \
	X(debugPrint) X(queuing) X(directEdit) X(selectorEnabled) X(variableKeys) \
	X(tickCount) X(frameTickCount) X(cameraActive) X(stereovisionActive) \
	X(resourceManager) X(stealthChara) X(stealthMotion) X(defaultSyncActive) \
	X(defaultTransformOrder) X(outsideFactor) X(syncActive) X(allplaying) \
	X(syncWaiting) X(hasCamera) X(hitThreshold) \
	/* S17: dibaca/diset game pada _player (bytecode) */ \
	X(animating) X(playing) X(loopTime) X(tags) X(scale) \
	X(bustScale) X(hairScale) X(partsScale) X(meshDivisionRatio) X(outline) X(smoothing) \
	X(speed) X(colorWeight) X(completionType) X(maskMode) X(chara)

// Method ResourceManager (statis). Termasuk dugaan dari urutan string.
#define RM_METHODS(X) \
	X(setEmotePSBDecryptSeed) X(setEmotePSBDecryptFunc) \
	X(unload) X(unloadAll) X(isExistMotion) X(findMotion) \
	X(requireLayerId) X(releaseLayerId) X(setClearColor) X(setResizable) \
	X(removeAllTextures) X(removeAllBg) X(removeAllCaption) \
	X(registerBg) X(registerCaption) X(unloadUnusedTextures) \
	X(captureCanvas) X(canvasCaptureEnabled) X(clearEnabled) \
	/* S17: dipanggil motion.tjs pada instance ResourceManager */ \
	X(load) X(clearCache)

// Method D3DAdaptor / SeparateLayerAdaptor.
#define D3D_METHODS(X) X(doAlphaMaskOperation)

// ---------------------------------------------------------------- pembangkit callback
#define DEF_PLAYER_M(name) \
	static tjs_error TJS_INTF_METHOD pm_##name(tTJSVariant *r, tjs_int n, tTJSVariant **p, MotionPlayerStub *s) { \
		logCall("Motion.Player." #name, n, p); return TJS_S_OK; }
PLAYER_METHODS(DEF_PLAYER_M)

#define DEF_PLAYER_P(name) \
	static tjs_error TJS_INTF_METHOD pg_##name(tTJSVariant *r, tjs_int n, tTJSVariant **p, MotionPlayerStub *s) { \
		logCall("Motion.Player." #name, 0, p, "get"); \
		if (s && r) { auto it = s->props.find(#name); if (it != s->props.end()) *r = it->second; } \
		return TJS_S_OK; } \
	static tjs_error TJS_INTF_METHOD ps_##name(tTJSVariant *r, tjs_int n, tTJSVariant **p, MotionPlayerStub *s) { \
		if (n < 1) return TJS_E_BADPARAMCOUNT; \
		logCall("Motion.Player." #name, 1, p, "set"); \
		if (s) s->props[#name] = *p[0]; return TJS_S_OK; }
PLAYER_PROPS(DEF_PLAYER_P)

// S17: Motion.EmotePlayer memakai daftar member yang sama dengan Player (ASUMSI).
#define DEF_EMOTE_M(name) \
	static tjs_error TJS_INTF_METHOD em_##name(tTJSVariant *r, tjs_int n, tTJSVariant **p, MotionEmotePlayerStub *s) { \
		logCall("Motion.EmotePlayer." #name, n, p); return TJS_S_OK; }
PLAYER_METHODS(DEF_EMOTE_M)

#define DEF_EMOTE_P(name) \
	static tjs_error TJS_INTF_METHOD eg_##name(tTJSVariant *r, tjs_int n, tTJSVariant **p, MotionEmotePlayerStub *s) { \
		logCall("Motion.EmotePlayer." #name, 0, p, "get"); \
		if (s && r) { auto it = s->props.find(#name); if (it != s->props.end()) *r = it->second; } \
		return TJS_S_OK; } \
	static tjs_error TJS_INTF_METHOD es_##name(tTJSVariant *r, tjs_int n, tTJSVariant **p, MotionEmotePlayerStub *s) { \
		if (n < 1) return TJS_E_BADPARAMCOUNT; \
		logCall("Motion.EmotePlayer." #name, 1, p, "set"); \
		if (s) s->props[#name] = *p[0]; return TJS_S_OK; }
PLAYER_PROPS(DEF_EMOTE_P)

#define DEF_RM_M(name) \
	static tjs_error TJS_INTF_METHOD rm_##name(tTJSVariant *r, tjs_int n, tTJSVariant **p, MotionResourceManagerStub *s) { \
		logCall("Motion.ResourceManager." #name, n, p); return TJS_S_OK; }
RM_METHODS(DEF_RM_M)

#define DEF_D3D_M(name) \
	static tjs_error TJS_INTF_METHOD d3d_##name(tTJSVariant *r, tjs_int n, tTJSVariant **p, MotionD3DAdaptorStub *s) { \
		logCall("Motion.D3DAdaptor." #name, n, p); return TJS_S_OK; } \
	static tjs_error TJS_INTF_METHOD sl_##name(tTJSVariant *r, tjs_int n, tTJSVariant **p, MotionSeparateLayerAdaptorStub *s) { \
		logCall("Motion.SeparateLayerAdaptor." #name, n, p); return TJS_S_OK; }
D3D_METHODS(DEF_D3D_M)

// getD3DAvailable: kembalikan false supaya game memilih jalur non-D3D.
static tjs_error TJS_INTF_METHOD d3d_getD3DAvailable(tTJSVariant *r, tjs_int n, tTJSVariant **p, MotionD3DAdaptorStub *s) {
	logCall("Motion.D3DAdaptor.getD3DAvailable", n, p);
	if (r) *r = (tjs_int)0;
	return TJS_S_OK;
}

// ---------------------------------------------------------------- factory (log argumen konstruktor)
#define DEF_FACTORY(Cls, label) \
	static tjs_error TJS_INTF_METHOD factory_##Cls(Cls **result, tjs_int n, tTJSVariant **p, iTJSDispatch2 *objthis) { \
		logCall(label ".<new>", n, p); *result = new Cls(); return TJS_S_OK; }
DEF_FACTORY(MotionPlayerStub,              "Motion.Player")
DEF_FACTORY(MotionEmotePlayerStub,         "Motion.EmotePlayer")
DEF_FACTORY(MotionResourceManagerStub,     "Motion.ResourceManager")
DEF_FACTORY(MotionD3DAdaptorStub,          "Motion.D3DAdaptor")
DEF_FACTORY(MotionSeparateLayerAdaptorStub,"Motion.SeparateLayerAdaptor")

// ---------------------------------------------------------------- registrasi
#define REG_PLAYER_M(name) NCB_METHOD_RAW_CALLBACK(name, &pm_##name, 0);
#define REG_PLAYER_P(name) NCB_PROPERTY_RAW_CALLBACK(name, &pg_##name, &ps_##name, 0);
#define REG_RM_M(name)     NCB_METHOD_RAW_CALLBACK(name, &rm_##name, TJS_STATICMEMBER);

// Konstanta: NILAI PLACEHOLDER (belum diverifikasi). Didaftarkan di Motion dan Motion.Player.
#define CONSTS_SEQ(X) \
	X(LayerTypeObj,0) X(LayerTypeShape,1) X(LayerTypeLayout,2) X(LayerTypeMotion,3) \
	X(LayerTypeParticle,4) X(LayerTypeCamera,5) \
	X(ShapeTypePoint,0) X(ShapeTypeCircle,1) X(ShapeTypeRect,2) X(ShapeTypeQuad,3) \
	X(PlayFlagForce,1) X(PlayFlagChain,2) X(PlayFlagAsCan,4) X(PlayFlagJoin,8) X(PlayFlagStealth,16) \
	X(TransformOrderFlip,0) X(TransformOrderSlant,1) X(TransformOrderZoom,2) X(TransformOrderAngle,3) \
	X(CoordinateRecutangularXY,0) X(CoordinateRecutangularXZ,1) \
	X(MaskModeAlpha,1)
#define REG_CONST(name, val) Variant(TJS_W(#name), (tjs_int)(val));

NCB_REGISTER_SUBCLASS(MotionPlayerStub) {
	Factory(&factory_MotionPlayerStub);
	PLAYER_METHODS(REG_PLAYER_M)
	PLAYER_PROPS(REG_PLAYER_P)
	CONSTS_SEQ(REG_CONST)
}

#define REG_EMOTE_M(name) NCB_METHOD_RAW_CALLBACK(name, &em_##name, 0);
#define REG_EMOTE_P(name) NCB_PROPERTY_RAW_CALLBACK(name, &eg_##name, &es_##name, 0);
NCB_REGISTER_SUBCLASS(MotionEmotePlayerStub) {
	Factory(&factory_MotionEmotePlayerStub);
	PLAYER_METHODS(REG_EMOTE_M)
	PLAYER_PROPS(REG_EMOTE_P)
	CONSTS_SEQ(REG_CONST)
}

NCB_REGISTER_SUBCLASS(MotionResourceManagerStub) {
	Factory(&factory_MotionResourceManagerStub);
	RM_METHODS(REG_RM_M)
}

NCB_REGISTER_SUBCLASS(MotionD3DAdaptorStub) {
	Factory(&factory_MotionD3DAdaptorStub);
	NCB_METHOD_RAW_CALLBACK(doAlphaMaskOperation, &d3d_doAlphaMaskOperation, 0);
	NCB_METHOD_RAW_CALLBACK(getD3DAvailable,      &d3d_getD3DAvailable,      TJS_STATICMEMBER);
}

NCB_REGISTER_SUBCLASS(MotionSeparateLayerAdaptorStub) {
	Factory(&factory_MotionSeparateLayerAdaptorStub);
	NCB_METHOD_RAW_CALLBACK(doAlphaMaskOperation, &sl_doAlphaMaskOperation, 0);
}

NCB_REGISTER_CLASS_DIFFER(Motion, MotionStub) {
	NCB_SUBCLASS(Player,               MotionPlayerStub);
	NCB_SUBCLASS(EmotePlayer,          MotionEmotePlayerStub);
	NCB_SUBCLASS(ResourceManager,      MotionResourceManagerStub);
	NCB_SUBCLASS(D3DAdaptor,           MotionD3DAdaptorStub);
	NCB_SUBCLASS(SeparateLayerAdaptor, MotionSeparateLayerAdaptorStub);
	CONSTS_SEQ(REG_CONST)
}
