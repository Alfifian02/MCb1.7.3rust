#!/bin/sh
# Menjalankan semua uji inti layerExAlpha (host, ASan+UBSan) terhadap layerExAlphaCore.hpp gabungan.
# Pakai dari akar repo:  sh tests/plugins/layerExAlpha/run_tests.sh
set -e
T=tests/plugins/layerExAlpha; OUT=${TMPDIR:-/tmp}/layerexalpha_tests; mkdir -p "$OUT"
CXX=${CXX:-g++}; FLAGS="-std=c++14 -O1 -g -fsanitize=address,undefined -fno-sanitize-recover=undefined -Wall -Wextra -Isrc/plugins"
rc=0
for t in test_alpha1_s6 test_alpha2_s7s10 test_alpha3_s8 test_alpha4_s9; do
  $CXX $FLAGS $T/$t.cpp -o "$OUT/$t"
  printf '%s: ' "$t"
  if [ "$t" = test_alpha1_s6 ]; then "$OUT/$t" $T/golden_cpu.txt | tail -1; else "$OUT/$t" | tail -1; fi || rc=1
done
exit $rc
