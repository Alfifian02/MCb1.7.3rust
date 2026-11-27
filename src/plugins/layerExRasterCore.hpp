// layerExRasterCore.hpp
// Dependency-free core of Layer.copyRaster (horizontal "raster scroll" wave).
// Behaviour reconstructed from libgame.so 1.3.9 (arm64); see docs/spec/layerExRaster_copyRaster.md
#ifndef LAYEREX_RASTER_CORE_HPP
#define LAYEREX_RASTER_CORE_HPP

#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>

namespace layerex_raster {

static const double kTwoPi      = 6.283185307179586;
static const double kPhaseScale = -1.0471975511965976; // -pi/3

struct Wave {
	double step;   // phase advance per row  = 2*pi / lines
	double phase0; // phase of the first processed row (row index == clipTop)
};

// time % (3*cycle), truncating like C. cycle == 0 -> returns time (AArch64 sdiv-by-zero gives 0).
inline int64_t time_mod(int64_t time, int cycle) {
	const int32_t p32 = (int32_t)((uint32_t)cycle * 3u); // original computes 3*cycle in 32 bit
	const int64_t period = p32;
	if (period == 0) return time;
	return time - (time / period) * period;
}

inline Wave make_wave(int clipTop, int imageHeight, int lines, int cycle, int64_t time) {
	Wave w;
	w.step = kTwoPi / (double)lines; // lines == 0 -> +inf (kept; rows then get shift 0)
	const double tmod = (double)time_mod(time, cycle);
	const double half = (double)(imageHeight / 2);
	const double shift = ((w.step * tmod) / (double)cycle) * half * kPhaseScale;
	w.phase0 = w.step * (double)clipTop + shift;
	return w;
}

// trunc(sin(phase) * maxh) with the saturating / NaN->0 behaviour of AArch64 fcvtzs
inline int row_shift(double phase, int maxh) {
	const double v = std::sin(phase) * (double)maxh;
	if (v != v) return 0;
	if (v >= 2147483647.0) return 2147483647;
	if (v <= -2147483648.0) return (int)(-2147483647 - 1);
	return (int)v;
}

// CPU path. dst/src point at the clip-rect origin of 32bpp buffers.
// Row i is shifted horizontally by row_shift(phase0 + i*step, maxh):
//   shift >= 0 : dst[x + s] = src[x]   (left s pixels of dst row stay untouched)
//   shift <  0 : dst[x]     = src[x - s]  (right |s| pixels of dst row stay untouched)
inline void copy_rows(uint8_t *dst, int dstPitch, const uint8_t *src, int srcPitch,
                      int clipWidth, int clipHeight, const Wave &w, int maxh) {
	double phase = w.phase0;
	for (int i = 0; i < clipHeight; ++i, phase += w.step) {
		const int s = row_shift(phase, maxh);
		uint8_t *drow = dst + (ptrdiff_t)i * dstPitch;
		const uint8_t *srow = src + (ptrdiff_t)i * srcPitch;
		if (s >= 0) {
			const int n = clipWidth - s;
			if (n < 1) continue;
			std::memmove(drow + (size_t)s * 4, srow, (size_t)n * 4);
		} else {
			const int n = clipWidth + s;
			if (n < 1) continue;
			std::memmove(drow, srow + (size_t)(-(int64_t)s) * 4, (size_t)n * 4);
		}
	}
}

// GLSL used on the GPU path (uniforms: omega, phase, h). Same maths as the CPU path,
// in normalised texture coordinates.
static const char *const kShaderName = "ExRaster";
static const char *const kShaderSrc =
	"uniform float omega;\n"
	"uniform float phase;\n"
	"uniform float h;\n"
	"void main()\n"
	"{\n"
	"    float dx = sin(v_texCoord0.y * omega + phase) * h + v_texCoord0.x;\n"
	"    gl_FragColor = texture2D(tex0, vec2(dx, v_texCoord0.y));\n"
	"}\n";

} // namespace layerex_raster

#endif
