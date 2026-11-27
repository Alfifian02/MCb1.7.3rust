// layerExImage.cpp - modul layerExImage.dll: Layer.light / colorize / modulate / noise / generateWhiteNoise / gaussianBlur
// Direkonstruksi dari perilaku Kirikiroid2 1.3.9 (libgame.so arm64); spesifikasi: docs/spec/layerExImage_*.md
//   light(brightness, contrast)        colorize(hue, sat, blend)        modulate(hue, sat, luminance)
//   noise(level)                       generateWhiteNoise()             gaussianBlur(sigma)
// Jalur CPU (render manager software) memakai layerExImageCore.hpp dan sudah dibandingkan piksel-per-piksel dengan
// fungsi asli di emulator (lihat tests/plugins/layerExImage). Jalur GPU memakai render method bergaya binary
// ("LayerEx_*"); GLSL di bawah = teks asli dari binary yang diformat ulang (light/noise/white noise setara; colorize/modulate
// mempertahankan matematika yang dibaca) dan BELUM diuji di perangkat.
// Seperti binary: kelas turunan layerExBase_GL, reset() dipanggil di setiap akses, jalur CPU memproses SELURUH gambar
// (bukan clip) dan memanggil redraw(); jalur GPU tidak memanggil Update().
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string>

#define NCB_MODULE_NAME TJS_W("layerExImage.dll")

#include "tjsCommHead.h"
#include "ncbind/ncbind.hpp"
#include "layerExBase.hpp"
#include "LayerBitmapIntf.h"
#include "Random.h"
#include "layerExImageCore.hpp"

using namespace layerex_image;

// Saklar: panggil Update() juga di jalur GPU. Binary asli TIDAK melakukannya; aktifkan hanya bila layar tidak
// menyegarkan setelah operasi GPU (putuskan lewat uji perangkat, S13).
#ifndef LAYEREXIMAGE_GPU_UPDATE
#define LAYEREXIMAGE_GPU_UPDATE 0
#endif

// ---------------------------------------------------------------------------------------------
// GLSL. Fungsi RGB<->HSL bergaya Hocevar/Persson (disebut di komentar shader asli); periksa lisensi bila fork didistribusikan.
static const char *const kHslLib =
	"float Epsilon = 1e-10;\n"
	"vec3 HUEtoRGB(in float H)\n"
	"{\n"
	"    float R = abs(H * 6.0 - 3.0) - 1.0;\n"
	"    float G = 2.0 - abs(H * 6.0 - 2.0);\n"
	"    float B = 2.0 - abs(H * 6.0 - 4.0);\n"
	"    return clamp(vec3(R,G,B), 0.0, 1.0);\n"
	"}\n"
	"vec3 RGBtoHCV(vec3 RGB)\n"
	"{\n"
	"    vec4 P = (RGB.g < RGB.b) ? vec4(RGB.bg, -1.0, 2.0/3.0) : vec4(RGB.gb, 0.0, -1.0/3.0);\n"
	"    vec4 Q = (RGB.r < P.x) ? vec4(P.xyw, RGB.r) : vec4(RGB.r, P.yzx);\n"
	"    float C = Q.x - min(Q.w, Q.y);\n"
	"    float H = abs((Q.w - Q.y) / (6.0 * C + Epsilon) + Q.z);\n"
	"    return vec3(H, C, Q.x);\n"
	"}\n"
	"vec3 RGBtoHSL(vec3 RGB)\n"
	"{\n"
	"    vec3 HCV = RGBtoHCV(RGB);\n"
	"    float L = HCV.z - HCV.y * 0.5;\n"
	"    float S = HCV.y / (1.0 - abs(L * 2.0 - 1.0) + Epsilon);\n"
	"    return vec3(HCV.x, S, L);\n"
	"}\n"
	"vec3 HSLtoRGB(vec3 HSL)\n"
	"{\n"
	"    vec3 RGB = HUEtoRGB(HSL.x);\n"
	"    float C = (1.0 - abs(2.0 * HSL.z - 1.0)) * HSL.y;\n"
	"    return (RGB - 0.5) * C + HSL.z;\n"
	"}\n";

static const char *const kLightSrc =
	"uniform float brightness;\n"
	"uniform float contrast;\n"
	"void main()\n"
	"{\n"
	"    vec4 s = texture2D(tex0, v_texCoord0);\n"
	"    s.rgb = (s.rgb - 0.5) * contrast + brightness;\n"
	"    gl_FragColor = s;\n"
	"}\n";

static const char *const kColorizeHead =
	"uniform float hue;\n"
	"uniform float sat;\n"
	"uniform float blend;\n";
static const char *const kColorizeMain =
	"void main()\n"
	"{\n"
	"    vec4 s = texture2D(tex0, v_texCoord0);\n"
	"    vec3 hsl = RGBtoHSL(s.rgb);\n"
	"    hsl.r = hue;\n"
	"    hsl.g = sat;\n"
	"    vec3 rgb = HSLtoRGB(hsl);\n"
	"    s.rgb = mix(s.rgb, rgb, blend);\n"
	"    gl_FragColor = s;\n"
	"}\n";

static const char *const kModulateHead =
	"uniform float hue;\n"
	"uniform float sat;\n"
	"uniform float S1;\n"
	"uniform float luminance;\n"
	"uniform float L1;\n";
static const char *const kModulateMain =
	"void main()\n"
	"{\n"
	"    vec4 s = texture2D(tex0, v_texCoord0);\n"
	"    vec3 hsl = RGBtoHSL(s.rgb);\n"
	"    hsl.r = clamp(hsl.r + hue, 0.0, 1.0);\n"
	"    hsl.g += (S1 - hsl.g) * sat;\n"
	"    hsl.b += (L1 - hsl.b) * luminance;\n"
	"    s.rgb = HSLtoRGB(hsl);\n"
	"    gl_FragColor = s;\n"
	"}\n";

static const char *const kNoiseSrc =
	"uniform vec2 seed1;\n"
	"uniform vec2 seed2;\n"
	"uniform vec2 seed3;\n"
	"uniform float level;\n"
	"void main()\n"
	"{\n"
	"    vec4 s = texture2D(tex0, v_texCoord0);\n"
	"    float noise;\n"
	"    noise = fract(sin(dot(v_texCoord0, seed1)) * 43758.5453);\n"
	"    s.r += (noise - 0.5) * level;\n"
	"    noise = fract(sin(dot(v_texCoord0, seed2)) * 43758.5453);\n"
	"    s.g += (noise - 0.5) * level;\n"
	"    noise = fract(sin(dot(v_texCoord0, seed3)) * 43758.5453);\n"
	"    s.b += (noise - 0.5) * level;\n"
	"    gl_FragColor = s;\n"
	"}\n";

static const char *const kWhiteNoiseSrc =
	"uniform vec2 seed;\n"
	"void main()\n"
	"{\n"
	"    vec4 s = texture2D(tex0, v_texCoord0);\n"
	"    s.rgb = vec3(fract(sin(dot(v_texCoord0, seed)) * 43758.5453));\n"
	"    gl_FragColor = s;\n"
	"}\n";

// ---------------------------------------------------------------------------------------------
struct layerExImage : public layerExBase_GL
{
	layerExImage(DispatchT obj) : layerExBase_GL(obj) {}
	virtual ~layerExImage() {}

	void light(int brightness, int contrast);
	void colorize(int hue, int sat, double blend);
	void modulate(int hue, int sat, int luminance);
	void noise(int level);
	void generateWhiteNoise();
	void gaussianBlur(float sigma);

private:
	// Jalankan render method pada gambar utama: rect = (clipLeft, clipTop, clipLeft + width, clipTop + height)
	// (binary memakai ukuran GAMBAR di titik asal clip, bukan ukuran clip; ditiru).
	void operate(iTVPRenderMethod *method)
	{
		tTVPRect rc(_clipLeft, _clipTop, _clipLeft + _width, _clipTop + _height);
		iTVPRenderManager *mgr = TVPGetRenderManager();
		iTVPTexture2D *stex = _this->GetMainImage()->GetTexture();
		if (!stex) return;
		tRenderTexRectArray::Element tex[] = { tRenderTexRectArray::Element(stex, rc) };
		iTVPTexture2D *dtex = _this->GetMainImage()->GetTextureForRender(method->IsBlendTarget(), &rc);
		mgr->OperateRect(method, dtex, nullptr, rc, tRenderTexRectArray(tex));
#if LAYEREXIMAGE_GPU_UPDATE
		redraw();
#endif
	}
	static iTVPRenderMethod *compile(const char *name, uint32_t *hint, const char *src)
	{
		return TVPGetRenderManager()->GetOrCompileRenderMethod(name, hint, src, 1);
	}
};

void layerExImage::light(int brightness, int contrast)
{
	if (light_is_noop(brightness, contrast)) return;
	if (TVPIsSoftwareRenderManager()) {
		if (!_buffer) return;
		uint8_t lut[256];
		light_lut(lut, brightness, contrast);
		light_apply(_buffer, _pitch, _width, _height, lut);
		redraw();
	} else {
		static uint32_t hint = 0;
		static iTVPRenderMethod *m = compile("LayerEx_Light", &hint, kLightSrc);
		if (!m) return;
		static int idB = m->EnumParameterID("brightness"), idC = m->EnumParameterID("contrast");
		float b = (float)(brightness + 128); b = b * 0.00390625f;
		m->SetParameterFloat(idB, b);
		m->SetParameterFloat(idC, (float)(contrast + 100) / 100.0f);
		operate(m);
	}
}

void layerExImage::colorize(int hue, int sat, double blend)
{
	if (colorize_is_noop(blend)) return;
	if (TVPIsSoftwareRenderManager()) {
		if (!_buffer) return;
		colorize_apply(_buffer, _pitch, _width, _height, blend, hue, sat);
		redraw();
	} else {
		static std::string src = std::string(kColorizeHead) + kHslLib + kColorizeMain;
		static uint32_t hint = 0;
		static iTVPRenderMethod *m = compile("LayerEx_Colorize", &hint, src.c_str());
		if (!m) return;
		static int idH = m->EnumParameterID("hue"), idS = m->EnumParameterID("sat"), idB = m->EnumParameterID("blend");
		const double bd = (blend < 1.0) ? blend : 1.0;
		m->SetParameterFloat(idB, (float)bd);
		const int h = hue < 255 ? hue : 255, s = sat < 255 ? sat : 255;
		m->SetParameterFloat(idH, hue > 0 ? (float)h / 255.0f : 0.0f);
		m->SetParameterFloat(idS, sat > 0 ? (float)s / 255.0f : 0.0f);
		operate(m);
	}
}

void layerExImage::modulate(int hue, int sat, int luminance)
{
	if (TVPIsSoftwareRenderManager()) {
		if (!_buffer) return;
		modulate_apply(_buffer, _pitch, _width, _height, hue, sat, luminance);
		redraw();
	} else {
		static std::string src = std::string(kModulateHead) + kHslLib + kModulateMain;
		static uint32_t hint = 0;
		static iTVPRenderMethod *m = compile("LayerEx_Modulate", &hint, src.c_str());
		if (!m) return;
		static int idH = m->EnumParameterID("hue"), idS = m->EnumParameterID("sat"), idS1 = m->EnumParameterID("S1"),
		           idL = m->EnumParameterID("luminance"), idL1 = m->EnumParameterID("L1");
		m->SetParameterFloat(idH, (float)hue / 360.0f);
		m->SetParameterFloat(idL, (float)luminance / 100.0f);
		m->SetParameterFloat(idS, (float)sat / 100.0f);
		m->SetParameterFloat(idS1, sat > 0 ? 1.0f : 0.0f);
		m->SetParameterFloat(idL1, luminance > 0 ? 1.0f : 0.0f);
		operate(m);
	}
}

void layerExImage::noise(int level)
{
	if (level == 0) return;
	if (TVPIsSoftwareRenderManager()) {
		if (!_buffer) return;
		uint32_t w[4];
		TVPGetRandomBits128(w);                                   // binary: seed dari TVPGetRandomBits128 (tidak deterministik)
		NoiseSeed seed = { w[0], w[1], w[2] };
		noise_apply(_buffer, _pitch, _width, _height, level, seed, []() { return rand(); });
		redraw();
	} else {
		static uint32_t hint = 0;
		static iTVPRenderMethod *m = compile("LayerEx_Noise", &hint, kNoiseSrc);
		if (!m) return;
		static int id1 = m->EnumParameterID("seed1"), id2 = m->EnumParameterID("seed2"),
		           id3 = m->EnumParameterID("seed3"), idL = m->EnumParameterID("level");
		m->SetParameterFloat(idL, (float)level / 255.0f);
		uint32_t w[4];
		TVPGetRandomBits128(w);
		float s1[2] = { noise_seed_float(w[0]), noise_seed_float(w[1]) };
		float s2[2] = { noise_seed_float(w[2]), noise_seed_float(w[3]) };
		m->SetParameterFloatArray(id1, s1, 2);
		m->SetParameterFloatArray(id2, s2, 2);
		TVPGetRandomBits128(w);
		float s3[2] = { noise_seed_float(w[0]), noise_seed_float(w[1]) };
		m->SetParameterFloatArray(id3, s3, 2);
		operate(m);
	}
}

void layerExImage::generateWhiteNoise()
{
	if (TVPIsSoftwareRenderManager()) {
		if (!_buffer) return;
		white_noise_apply(_buffer, _pitch, _width, _height, []() { return rand(); });
		redraw();
	} else {
		static uint32_t hint = 0;
		static iTVPRenderMethod *m = compile("LayerEx_WhiteNoise", &hint, kWhiteNoiseSrc);
		if (!m) return;
		static int idS = m->EnumParameterID("seed");
		uint32_t w[4];
		TVPGetRandomBits128(w);
		float s[2] = { noise_seed_float(w[0]), noise_seed_float(w[1]) };
		m->SetParameterFloatArray(idS, s, 2);
		operate(m);
	}
}

void layerExImage::gaussianBlur(float sigma)
{
	if (TVPIsSoftwareRenderManager()) {
		if (!_buffer) return;
		// DEVIASI fork: sigma yang menghasilkan kernel > kMaxKernelTaps ditolak (binary mencoba alokasi raksasa).
		if (!gaussian_apply(_buffer, _pitch, _width, _height, sigma)) return;
		redraw();
	} else {
		// binary: bukan gaussian, melainkan box blur (alfa ikut) pada area (-r,-r,r,r) dengan r = trunc(sigma + 0.99)
		int neg, pos;
		gauss_box_radius(sigma, &neg, &pos);
		tTVPRect rect(_clipLeft, _clipTop, _clipLeft + _clipWidth, _clipTop + _clipHeight);
		tTVPRect area(neg, neg, pos, pos);
		_this->GetMainImage()->DoBoxBlurForAlpha(rect, area);
		redraw();
	}
}

// ----------------------------------- registrasi kelas
NCB_GET_INSTANCE_HOOK(layerExImage)
{
	NCB_INSTANCE_GETTER(objthis) {
		ClassT *obj = GetNativeInstance(objthis);
		if (!obj) {
			obj = new ClassT(objthis);
			SetNativeInstance(objthis, obj);
		}
		obj->reset();           // binary: vtable[0] (reset) dipanggil di setiap akses
		return obj;
	}
	~NCB_GET_INSTANCE_HOOK_CLASS() {}
};

NCB_ATTACH_CLASS_WITH_HOOK(layerExImage, Layer) {
	NCB_METHOD(light);
	NCB_METHOD(colorize);
	NCB_METHOD(modulate);
	NCB_METHOD(noise);
	NCB_METHOD(generateWhiteNoise);
	NCB_METHOD(gaussianBlur);
}
