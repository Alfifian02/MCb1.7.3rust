#!/bin/bash
# Pemeriksa BAHASA skrip uji TJS di host (tanpa perangkat). Mengompilasi mesin TJS2 repo (src/core/tjs2) tanpa RegExp,
# menautkan pelari mini (runner.cpp: Scripts.eval/execStorage + hook Array.save) dan menjalankan tests/tjs dengan tiruan mock.tjs.
# TIDAK menguji perilaku piksel plugin (tiruan memakai semantik minimal dari spesifikasi). Satu-satunya FAIL yang diharapkan: alias === (stretchCopyAA).
# Semantik piksel tiruan = semantik ASLI fork: getMainPixel/setMainPixel hanya RGB 24 bit, alfa via getMaskPixel/setMaskPixel. RBSWAP=1 meniru R/B terbalik.
# Pakai: tests/tjs_host/build_and_run.sh   (butuh g++; hasil: tests/tjs_host/work/log.txt)
set -e
HERE="$(cd "$(dirname "$0")" && pwd)"; ROOT="$HERE/../.."; W="$HERE/work"; mkdir -p "$W/obj" "$W/run"
F="-std=c++14 -w -fpermissive -O0 -DTJS_NO_REGEXP -DTJS_TEXT_OUT_CRLF=0 -include $HERE/force.h -I$ROOT/src/core -I$ROOT/src/core/tjs2 -I$ROOT/src/core/utils -I$ROOT/src/core/environ -I$ROOT/src/core/base"
# Khusus host x86-64: va_list *pap = (va_list *)&ap salah (benar di ARM64). Salinan ditambal; repo tidak diubah.
python3 - "$ROOT/src/core/tjs2/tjsConfig.cpp" "$W/tjsConfig_host.cpp" <<'PY'
import sys
s=open(sys.argv[1],encoding='latin-1').read(); o='va_list *pap = (va_list *)&ap;'
assert o in s; open(sys.argv[2],'w',encoding='latin-1').write(s.replace(o,'va_list apc; va_copy(apc, ap); va_list *pap = &apc;'))
PY
for f in "$ROOT"/src/core/tjs2/*.cpp; do b=$(basename "$f" .cpp); [ "$b" = tjsConfig ] && continue; echo "g++ $F -c $f -o $W/obj/$b.o"; done | xargs -P8 -I{} sh -c '{}'
g++ $F -c "$W/tjsConfig_host.cpp" -o "$W/obj/tjsConfig.o"
g++ $F -c "$HERE/runner.cpp" -o "$W/obj/zz_runner.o"
g++ "$W"/obj/*.o -o "$W/runner"
rm -rf "$W/run"/*; cp "$ROOT"/tests/tjs/*.tjs "$HERE/mock.tjs" "$W/run/"
[ "${RBSWAP:-0}" = 1 ] && sed -i 's/var MOCK_RBSWAP = false;/var MOCK_RBSWAP = true;/' "$W/run/mock.tjs"   # RBSWAP=1: meniru R/B terbalik di fork
( cd "$W/run" && "$W/runner" mock.tjs startup.tjs >/dev/null 2>&1 || true; tr -d '\r' < out_uji_log.txt > "$W/log.txt" )
echo "== galat bahasa (harus kosong):"; grep -E "ERROR|Cannot convert|Syntax" "$W/log.txt" | grep -v "copyBlueToAlpha.bukan_layer" || true   # bukan_layer = probe disengaja
grep "does not exist" "$W/log.txt" | grep -v closure_tanpa_args || true
grep SELESAI "$W/log.txt"; grep "CHECK FAIL" "$W/log.txt" || true
