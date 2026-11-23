// Pelari mini TJS2 (host). Menyediakan: Scripts.eval / Scripts.execStorage (native), hook tulis teks Array.save,
// pencetak keluaran lewat global OUT. Selebihnya (Window/Layer/Debug/Plugins) = tiruan TJS di mock.tjs.
#include "tjsCommHead.h"
#include "tjs.h"
#include "tjsError.h"
#include "tjsMessage.h"
#include "tjsObject.h"
#include <cstdio>
#include <string>
#include <fstream>
#include <sstream>
using namespace TJS;
namespace TJS { void TJSReleaseRegex(){}  void TVPConsoleLog(const tjs_char*m){ } }
ttstr TVPGetMessageByLocale(const std::string &k){ return ttstr(); }
unsigned int TVPGetRoughTickCount32(){ return 0; }

static std::string to8(const tjs_char*m){ std::string s; for(;*m;++m){ unsigned c=*m; if(c<0x80) s+=(char)c; else if(c<0x800){ s+=(char)(0xC0|(c>>6)); s+=(char)(0x80|(c&63)); } else { s+=(char)(0xE0|(c>>12)); s+=(char)(0x80|((c>>6)&63)); s+=(char)(0x80|(c&63)); } } return s; }
static std::basic_string<tjs_char> from8(const std::string&u){ std::basic_string<tjs_char> w; size_t i=0; while(i<u.size()){ unsigned char c=u[i]; unsigned cp; int n; if(c<0x80){cp=c;n=1;} else if((c>>5)==6){cp=c&31;n=2;} else if((c>>4)==14){cp=c&15;n=3;} else {cp=c&7;n=4;} for(int k=1;k<n&&i+k<u.size();k++) cp=(cp<<6)|(u[i+k]&63); i+=n; if(cp>=0x10000){ cp-=0x10000; w.push_back(0xD800+(cp>>10)); w.push_back(0xDC00+(cp&1023)); } else w.push_back((tjs_char)cp);} return w; }

struct Con : iTJSConsoleOutput {
  void ExceptionPrint(const tjs_char *m) override { if(getenv("SHOWEXC")) printf("EXC %s\n", to8(m).c_str()); }
  void Print(const tjs_char *m) override { if(getenv("SHOWEXC")) printf("%s\n", to8(m).c_str()); }
};
struct WS : iTJSTextWriteStream {
  FILE *f; WS(FILE*f):f(f){}
  void TJS_INTF_METHOD Write(const tTJSString &t) override { fputs(to8(t.c_str()).c_str(), f); }
  void TJS_INTF_METHOD Destruct() override { fclose(f); delete this; }
};
static iTJSTextWriteStream* mkws(const tTJSString &name, const tTJSString &mode){ std::string p = "out_" + to8(name.c_str()); for(auto &c: p) if(c=='/') c='_'; FILE*f=fopen(p.c_str(),"wb"); return f? new WS(f) : NULL; }

static tTJS *E;
struct ScriptsObj : tTJSDispatch {
  tjs_error TJS_INTF_METHOD FuncCall(tjs_uint32 flag, const tjs_char *name, tjs_uint32 *hint, tTJSVariant *result, tjs_int n, tTJSVariant **p, iTJSDispatch2 *objthis) override {
    ttstr nm(name);
    if(nm == TJS_W("eval")) { E->EvalExpression(ttstr(*p[0]), result); return TJS_S_OK; }
    if(nm == TJS_W("execStorage")) {
      std::ifstream f(to8(ttstr(*p[0]).c_str())); if(!f) TJS_eTJSError(TJS_W("storage not found"));
      std::stringstream ss; ss<<f.rdbuf(); auto w = from8(ss.str()); ttstr name(*p[0]);
      E->ExecScript(w.c_str(), result, NULL, name.c_str()); return TJS_S_OK; }
    return TJS_E_MEMBERNOTFOUND;
  }
};
int main(int argc,char**argv){
  TJSCreateTextStreamForWrite = mkws;
  E = new tTJS(); Con con; E->SetConsoleOutput(&con);
  iTJSDispatch2 *g = E->GetGlobalNoAddRef();
  ScriptsObj *so = new ScriptsObj(); { tTJSVariant v(so, so); g->PropSet(TJS_MEMBERENSURE, TJS_W("Scripts"), NULL, &v, g); }
  int rc = 0;
  for(int i=1;i<argc;i++){
    std::ifstream f(argv[i]); std::stringstream ss; ss<<f.rdbuf(); auto w = from8(ss.str());
    try { ttstr nm(argv[i]); E->ExecScript(w.c_str(), NULL, NULL, nm.c_str()); }
    catch(eTJSScriptError &x){ printf("SCRIPTERR %s : %s\n", argv[i], to8(x.GetMessage().c_str()).c_str()); rc=2; break; }
    catch(eTJS &x){ printf("TJSERR %s : %s\n", argv[i], to8(x.GetMessage().c_str()).c_str()); rc=3; break; }
  }
  tTJSVariant v; if(TJS_SUCCEEDED(g->PropGet(0, TJS_W("OUT"), NULL, &v, g))) { ttstr t(v); printf("%s\n", to8(t.c_str()).c_str()); }
  return rc;
}
