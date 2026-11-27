// layerExBTOACore.hpp
// Dependency-free pixel core of the layerExBTOA module (layerExBTOA.dll): copyRightBlueToLeftAlpha,
// copyBottomBlueToTopAlpha, copyAlphaToProvince, fillByProvince, clipAlphaRect (CPU paths).
// Behaviour reconstructed from libgame.so 1.3.9 (arm64); see docs/spec/layerExBTOA_*.md.
//
// Pixel layout: 32bpp, bytes [0][1][2][3]. The binary copies byte[2] ("blue") into byte[3] (alpha) and
// swaps R/B of colour arguments (same as TVP_REVRGB in the fork), i.e. the CPU buffer is RGBA in memory order
// (docs/spec/layerExBTOA_module.md).
// The code below never needs to know the channel names: it only reproduces the byte moves of the binary.
#ifndef LAYEREX_BTOA_CORE_HPP
#define LAYEREX_BTOA_CORE_HPP

#include <cstddef>
#include <cstdint>
#include <cstring>

namespace layerex_btoa {

struct Rect { int left, top, right, bottom; };

// --------------------------------------------------------------------------------------------
// GLSL used on the GPU paths (strings read verbatim from libgame.so .rodata)
// --------------------------------------------------------------------------------------------
static const char *const kCopyAlphaFromBlueName = "CopyAlphaFromBlue";           // @0x14ca7a6
static const char *const kCopyAlphaFromBlueSrc =                                  // @0x14ca7b8
	"void main()\n"
	"{\n"
	"    vec4 s = texture2D(tex0, v_texCoord0);\n"
	"    s.a = s.b;\n"
	"    gl_FragColor = s;\n"
	"}";
static const char *const kFillByProvinceName = "FillByProvince";                  // @0x14ca820
static const char *const kFillByProvinceSrc =                                     // @0x14ca82f
	"uniform vec4 u_color;\n"
	"uniform float opa;\n"
	"void main()\n"
	"{\n"
	"    vec4 s = texture2D(tex0, v_texCoord0);\n"
	"    s.a = step(abs(s.r - opa), 0.75 / 255.0);\n"
	"    s.rgb = u_color.rgb;\n"
	"    gl_FragColor = s;\n"
	"}";

// GL enums passed to SetBlendFuncSeparate(func, srcRGB, dstRGB, srcA, dstA) by the binary
static const int kGL_FUNC_ADD = 0x8006, kGL_ZERO = 0, kGL_ONE = 1, kGL_SRC_ALPHA = 0x302, kGL_ONE_MINUS_SRC_ALPHA = 0x303;

// --------------------------------------------------------------------------------------------
// copyRightBlueToLeftAlpha / copyBottomBlueToTopAlpha   (CPU path)
// buf = first pixel of the texture, pitch in bytes, w/h = texture size. Returns the rect handed to Update().
// --------------------------------------------------------------------------------------------
inline Rect copy_right_blue_to_left_alpha(uint8_t *buf, ptrdiff_t pitch, int w, int h) {
	const int half = (w + (w < 0 ? 1 : 0)) >> 1;                  // signed w/2 (cmp/cinc/asr)
	if (h >= 1 && w >= 2) {
		for (int y = 0; y < h; ++y) {
			uint8_t *row = buf + (ptrdiff_t)y * pitch;
			for (int x = 0; x < half; ++x)
				row[(size_t)x * 4 + 3] = row[((size_t)x + (size_t)half) * 4 + 2];   // dst alpha <- pixel (x+half) byte[2]
		}
	}
	Rect r = { 0, 0, half, h };
	return r;
}

inline Rect copy_bottom_blue_to_top_alpha(uint8_t *buf, ptrdiff_t pitch, int w, int h) {
	const int half = (h + (h < 0 ? 1 : 0)) >> 1;                  // signed h/2
	if (w >= 1 && h >= 2) {
		for (int y = 0; y < half; ++y) {
			uint8_t *d = buf + (ptrdiff_t)y * pitch;
			const uint8_t *s = buf + (ptrdiff_t)(y + half) * pitch;
			for (int x = 0; x < w; ++x)
				d[(size_t)x * 4 + 3] = s[(size_t)x * 4 + 2];                       // dst alpha <- row (y+half) byte[2]
		}
	}
	Rect r = { 0, 0, w, half };
	return r;
}

// --------------------------------------------------------------------------------------------
// copyAlphaToProvince(level = -1)
//   mode 0 (level < 0)       : province = alpha
//   mode 1 (0 <= level<=255) : province = (alpha >= level) ? 1 : 0
//   mode 2 (level > 255)     : province = 0
// src/dst point at the clip origin of the 32bpp main image / 8bpp province image.
// --------------------------------------------------------------------------------------------
inline int province_mode(int level) { return level < 0 ? 0 : (level > 255 ? 2 : 1); }

inline void copy_alpha_to_province(const uint8_t *src, ptrdiff_t srcPitch, uint8_t *dst, ptrdiff_t dstPitch,
                                   int w, int h, int level) {
	if (h < 1 || w < 1) return;
	const int mode = province_mode(level);
	const uint8_t lv = (uint8_t)(level & 0xff);
	for (int y = 0; y < h; ++y) {
		const uint8_t *s = src + (ptrdiff_t)y * srcPitch;
		uint8_t *d = dst + (ptrdiff_t)y * dstPitch;
		if (mode == 2) { std::memset(d, 0, (size_t)w); continue; }
		for (int x = 0; x < w; ++x) {
			const uint8_t a = s[(size_t)x * 4 + 3];
			d[x] = (mode == 0) ? a : (uint8_t)(a >= lv ? 1 : 0);
		}
	}
}

// --------------------------------------------------------------------------------------------
// fillByProvince(province, color)
// colour argument: R and B swapped (0xAARRGGBB -> 0xAABBGGRR), see swap_rb().
// --------------------------------------------------------------------------------------------
// identical to TVP_REVRGB in the fork (LayerBitmapIntf.h:21); the cpp static_asserts the equivalence
constexpr uint32_t swap_rb(uint32_t c) { return (c & 0xff00ff00u) | ((c >> 16) & 0xffu) | ((c & 0xffu) << 16); }

// CPU path. The binary walks the whole *texture* (texW x texH) writing pixel (x,y) of `dst`, but samples the
// province at (left + x, top + y) (clip origin), i.e. the province pointer is offset by the clip origin while
// the destination is not. With the default clip (left = top = 0) that is a plain full-layer fill.
// The binary reads past the province buffer when the clip origin is not (0,0); here such samples never match.
inline void fill_by_province(uint8_t *dst, ptrdiff_t dstPitch, int texW, int texH,
                             const uint8_t *prov, ptrdiff_t provPitch, int provW, int provH,
                             int left, int top, uint8_t value, uint32_t colorSwapped) {
	if (!dst || !prov) return;
	for (int y = 0; y < texH; ++y) {
		const long long py = (long long)top + y;
		if (py < 0 || py >= provH) continue;
		const uint8_t *prow = prov + (ptrdiff_t)py * provPitch;
		uint8_t *drow = dst + (ptrdiff_t)y * dstPitch;
		for (int x = 0; x < texW; ++x) {
			const long long px = (long long)left + x;
			if (px < 0 || px >= provW) continue;
			if (prow[px] == value) std::memcpy(drow + (size_t)x * 4, &colorSwapped, 4);   // 32-bit store (host byte order = ARM LE)
		}
	}
}

// --------------------------------------------------------------------------------------------
// clipAlphaRect(dx, dy, src, sx, sy, w, h [, outside])
//   dst.alpha = mul_alpha(dst.alpha, src.alpha) over the visible part of the rect;
//   if `outside` (0..255) is given, the rest of the clip rect gets alpha = outside.
// --------------------------------------------------------------------------------------------
inline uint8_t mul_alpha(uint8_t d, uint8_t s) {
	uint32_t v = (uint32_t)s * (uint32_t)d;
	v += v >> 7;
	return (uint8_t)(v >> 8);
}

// TJS result codes returned by clipAlphaRect before it touches any pixel (verified by emulation of 0x5e2544)
static const int kTJS_E_BADPARAMCOUNT = -1004, kTJS_E_INVALIDPARAM = -1003;
inline int clip_alpha_rect_precheck(int numparams, int w, int h) {
	if (numparams < 7) return kTJS_E_BADPARAMCOUNT;
	if (w < 1 || h < 1) return kTJS_E_INVALIDPARAM;
	return 0;
}
// optional 8th parameter: used only when it is an integer-convertible value with (uint64)value < 256; void/absent: unused
inline bool clip_alpha_rect_has_outside(int numparams, bool isVoid, long long value) {
	return numparams >= 8 && !isVoid && (unsigned long long)value < 256ull;
}

struct ClipIn {
	int dx, dy, sx, sy, w, h;     // parameters (w, h must be >= 1: checked by the caller)
	int srcW, srcH;               // size of the source bitmap
	int cw, ch;                   // clip size of `this` (clip origin is where dstBase points)
};
struct ClipOut {
	bool visible;                 // false: nothing is drawn (the "skipped" branch of the binary)
	int dxc, dyc, wf, hf;         // destination rect (clip-relative) after clipping
	int sxs, sys;                 // first source pixel
};

// 32-bit wrapping arithmetic like the binary (w-registers)
inline int add32(int a, int b) { return (int)((uint32_t)a + (uint32_t)b); }
inline int sub32(int a, int b) { return (int)((uint32_t)a - (uint32_t)b); }
inline int neg32(int a) { return (int)(0u - (uint32_t)a); }
inline int max0(int a) { return a < 0 ? 0 : a; }

// `cl`, `ct` = clip origin of `this` (only used to make dx, dy clip-relative)
inline ClipOut clip_alpha_rect_clip(const ClipIn &in, int cl, int ct) {
	ClipOut o = { false, 0, 0, 0, 0, 0, 0 };
	const int a = add32(in.w, in.sx);                       // w9
	const int b = add32(in.h, in.sy);                       // w8
	if (a < 1 || b < 1) return o;
	if (in.srcW <= in.sx || in.srcH <= in.sy) return o;
	// ---- horizontal
	const int dxrel = sub32(in.dx, cl);
	const int ovR   = max0(sub32(a, in.srcW));
	const int wsrc  = sub32(in.sx < 0 ? a : in.w, ovR);
	const int dxr   = add32(dxrel, in.sx < 0 ? neg32(in.sx) : 0);
	const int sxs   = in.sx < 0 ? 0 : in.sx;
	const int dr    = add32(wsrc, dxr);
	const int hsrc0 = in.sy < 0 ? b : in.h;
	const int sys   = in.sy < 0 ? 0 : in.sy;
	if (dr < 1) return o;
	// ---- vertical
	const int dyrel = sub32(in.dy, ct);
	const int dyr   = add32(dyrel, in.sy < 0 ? neg32(in.sy) : 0);
	const int ovB   = max0(sub32(b, in.srcH));
	const int hsrc  = sub32(hsrc0, ovB);
	const int db    = add32(hsrc, dyr);
	if (db < 1) return o;
	if (dxr >= in.cw || dyr >= in.ch) return o;
	// ---- clip to the clip rect
	const int wf  = sub32(dxr < 0 ? dr : wsrc, max0(sub32(dr, in.cw)));
	const int dxc = dxr < 0 ? 0 : dxr;
	const int hf  = sub32(dyr < 0 ? db : hsrc, max0(sub32(db, in.ch)));
	const int dyc = dyr < 0 ? 0 : dyr;
	if (wf < 1 || hf <= 0) return o;
	o.visible = true; o.dxc = dxc; o.dyc = dyc; o.wf = wf; o.hf = hf;
	o.sxs = add32(sxs, dxr < 0 ? neg32(dxr) : 0);           // source column shifts by the part cut at the left
	o.sys = add32(sys, dyr < 0 ? neg32(dyr) : 0);           // ... and the source row by the part cut at the top
	return o;
}

inline void fill_alpha_run(uint8_t *row, int x0, int x1, uint8_t v) {   // pixels [x0, x1)
	for (int x = x0; x < x1; ++x) row[(size_t)x * 4 + 3] = v;
}

// dstBase = clip origin pixel of `this`, dstPitch bytes; srcRows[y] = first pixel of source row y (srcH rows).
// hasOutside/outside: optional 8th parameter.
// Result: what the plugin must pass to the TJS method `update(cl + x, ct + y, w, h)`; `update` is false when the
// binary does not call update at all (rect completely invisible and no `outside` value).
struct ClipResult { bool update; int x, y, w, h; };

inline ClipResult clip_alpha_rect(uint8_t *dstBase, ptrdiff_t dstPitch, const uint8_t *const *srcRows,
                                  const ClipIn &in, bool hasOutside, uint8_t outside, int cl, int ct) {
	const ClipOut o = clip_alpha_rect_clip(in, cl, ct);
	ClipResult res = { false, 0, 0, 0, 0 };
	if (!o.visible) {
		if (hasOutside) {          // the whole clip rect (cw x ch) becomes `outside`, then update(cl, ct, cw, ch)
			for (int y = 0; y < in.ch; ++y) fill_alpha_run(dstBase + (ptrdiff_t)y * dstPitch, 0, in.cw, outside);
			res.update = true; res.w = in.cw; res.h = in.ch;
		}
		return res;
	}
	if (hasOutside) {
		for (int y = 0; y < o.dyc; ++y) fill_alpha_run(dstBase + (ptrdiff_t)y * dstPitch, 0, in.cw, outside);
		for (int y = o.hf + o.dyc; y < in.ch; ++y) fill_alpha_run(dstBase + (ptrdiff_t)y * dstPitch, 0, in.cw, outside);
	}
	for (int i = 0; i < o.hf; ++i) {
		uint8_t *d = dstBase + (ptrdiff_t)(o.dyc + i) * dstPitch;
		const uint8_t *s = srcRows[o.sys + i];
		if (hasOutside && o.dxc >= 1) fill_alpha_run(d, 0, o.dxc, outside);
		for (int x = 0; x < o.wf; ++x) {
			uint8_t &da = d[(size_t)(o.dxc + x) * 4 + 3];
			da = mul_alpha(da, s[(size_t)(o.sxs + x) * 4 + 3]);
		}
		if (hasOutside && (o.dxc + o.wf) < in.cw) fill_alpha_run(d, o.dxc + o.wf, in.cw, outside);
	}
	res.update = true;
	if (hasOutside) { res.x = 0; res.y = 0; res.w = in.cw; res.h = in.ch; }
	else { res.x = o.dxc; res.y = o.dyc; res.w = o.wf; res.h = o.hf; }
	return res;
}

} // namespace layerex_btoa

#endif
