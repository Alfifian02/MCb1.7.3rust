Paket copyRaster (langkah 4.5 + 5.2)  -- salin ke root repo (struktur sama):
  src/plugins/layerExRasterCore.hpp   inti algoritma (tanpa dependensi, sudah diuji)
  src/plugins/layerExRaster.cpp       plugin: Layer.copyRaster  (BELUM dikompilasi terhadap fork)
  docs/spec/layerExRaster_copyRaster.md
  tests/plugins/layerExRaster/test_core.cpp
Uji inti (di PC/Termux):  cd tests/plugins/layerExRaster && g++ -std=c++14 -I../../../src/plugins test_core.cpp && ./a.out
Pakai di game:  Plugins.link("layerExRaster.dll");  dest.copyRaster(src, maxh, lines, cycle, time);
