// layerExAreaAverage.cpp - Layer.stretchCopyAA
// Di Kirikiroid2 1.3.9 modul ini hanya satu ekspresi TJS (string di libgame.so @0x56ac00):
//     Layer.stretchCopyAA = Layer.stretchCopy;
// Jadi stretchCopyAA adalah alias persis dari stretchCopy, bukan algoritma area-average terpisah.
// Lihat docs/spec/layerExAreaAverage_stretchCopyAA.md.
#include <stdio.h>

#define NCB_MODULE_NAME TJS_W("layerExAreaAverage.dll")

#include "ncbind/ncbind.hpp"

//---------------------------------------------------------------------------
// Dijalankan setelah semua modul terdaftar: kelas Layer dan stretchCopy sudah ada.
static void PostRegist_AreaAverage()
{
	TVPExecuteExpression(TJS_W("Layer.stretchCopyAA = Layer.stretchCopy;"));
}

static void PreUnregist_AreaAverage()
{
	iTJSDispatch2 *global = TVPGetScriptDispatch();
	if (!global) return;
	tTJSVariant varLayer;
	if (TJS_FAILED(global->PropGet(0, TJS_W("Layer"), nullptr, &varLayer, global))) return;
	iTJSDispatch2 *layer = varLayer.AsObjectNoAddRef();
	if (layer) layer->DeleteMember(0, TJS_W("stretchCopyAA"), nullptr, layer);
}

NCB_POST_REGIST_CALLBACK(PostRegist_AreaAverage);
NCB_PRE_UNREGIST_CALLBACK(PreUnregist_AreaAverage);
