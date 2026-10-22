//! GLSL ES 3.00 sources of the OpenGL ES renderer (valid on any 3.x context, 3.2 included): ports of the WGSL of `gpu/pipeline.rs`,
//! `render/{present,hud,weather,panorama}.rs` and the legacy half of `render/sky.wgsl`, line for line, with the same constants spliced in.
//! Conventions that differ from wgpu: GL clip depth is -1..1 (`depth_fix` on the CPU side), the window surface is not sRGB (the final
//! shader encodes), the scene target is `SRGB8_ALPHA8`. `tools/gles_check.py` compiles and draws every program on Mesa (`cargo test`
//! does the same through `gles::tests` when an EGL driver exists).

use crate::render::atlas;

const HEAD_V: &str = "#version 300 es\nprecision highp float;\nprecision highp int;\n";
const HEAD_F: &str = "#version 300 es\nprecision highp float;\nprecision highp int;\nprecision highp sampler2D;\nprecision highp sampler2DArray;\n";

/// A vertex + fragment source pair.
pub struct Src {
    pub vs: String,
    pub fs: String,
}

/// Which chunk-shader program: the six pipelines of `gpu::pipeline::ChunkPipeline`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Chunk {
    Solid,
    Cutout,
    Entity,
    Lod,
    Water,
    LodWater,
}

impl Chunk {
    pub const ALL: [Chunk; 6] = [Chunk::Solid, Chunk::Cutout, Chunk::Entity, Chunk::Lod, Chunk::Water, Chunk::LodWater];
    /// The 16-byte `embeddium` vertex + chunk origin (else the 32-byte float vertex).
    pub fn compact(self) -> bool {
        matches!(self, Chunk::Solid | Chunk::Cutout | Chunk::Water)
    }
    pub fn water(self) -> bool {
        matches!(self, Chunk::Water | Chunk::LodWater)
    }
    /// Cut-out fragments write their alpha as coverage when multisampled.
    pub fn cut(self) -> bool {
        matches!(self, Chunk::Cutout | Chunk::Entity | Chunk::Lod)
    }
}

const CHUNK_COMMON: &str = r#"
layout(std140) uniform U { mat4 vp; vec4 eye; vec4 fog; vec4 fogp; };
float fog_factor(float d) {
    if (fog.w < 0.0) { return 1.0 - exp(fog.w * d); }
    if (fog.w == 0.0) { return 0.0; }
    float s0 = fogp.x > 0.0 ? fogp.x : 0.25;
    return clamp((d - s0 * fog.w) / ((1.0 - s0) * fog.w), 0.0, 1.0);
}
// World.lightBrightnessTable; sky and blk are light levels / 15, eye.w the sky light the time of day takes away.
float brightness(float sky, float blk) {
    float l = max(max(sky - eye.w, 0.0), blk);
    return (l / (4.0 - 3.0 * l)) * 0.95 + 0.05;
}
"#;

const CHUNK_VS: &str = r#"
#ifdef COMPACT
layout(location = 0) in uvec4 a_p;
layout(location = 1) in uvec2 a_t;
layout(location = 2) in vec4 a_c;
uniform vec3 u_origin;
#else
layout(location = 0) in vec3 a_p;
layout(location = 1) in vec2 a_uv;
layout(location = 2) in float a_l;
layout(location = 3) in float a_lm;
layout(location = 4) in float a_t;
#endif
centroid out vec2 v_uv;
centroid out vec3 v_tint;
centroid out float v_lit;
centroid out float v_fg;
flat out float v_k;
out vec3 v_wp;
out vec2 v_uvd;
void main() {
#ifdef COMPACT
    vec3 vpos = u_origin + vec3(float(a_p.x) / 2048.0 - 8.0, float(a_p.y) / 256.0 - 8.0, float(a_p.z) / 2048.0 - 8.0);
    v_tint = pow(a_c.rgb, vec3(2.2));
    v_uv = vec2(a_t) / 65536.0;
    uint w = a_p.w;
    bool sm = (w & 0x8000u) != 0u;
#ifdef WATER
    sm = false;
#endif
    if (sm) {
        uint v = w & 0x7FFFu;
        v_lit = pow(brightness(float(v >> 5u) / 60.0, float(v & 31u) / 30.0) * a_c.a, 2.2);
    } else {
        v_lit = pow(brightness(float((w >> 4u) & 15u) / 15.0, float(w & 15u) / 15.0) * a_c.a, 2.2);
    }
    v_k = -1.0;
#else
    vec3 vpos = a_p;
    uint tp = uint(a_t);
    v_tint = pow(vec3(float((tp >> 16u) & 255u), float((tp >> 8u) & 255u), float(tp & 255u)) / 255.0, vec3(2.2));
    v_uv = a_uv;
#ifndef WATER
    if (a_lm >= 256.0) {
        float v = a_lm - 256.0;
        float s = floor(v / 32.0 + 0.001);
        v_lit = pow(brightness(s / 60.0, (v - s * 32.0) / 30.0) * a_l, 2.2);
        v_k = -1.0;
    } else
#endif
    {
        float s = floor(a_lm / 16.0 + 0.001);
        v_lit = pow(brightness(s / 15.0, (a_lm - s * 16.0) / 15.0), 2.2);
        v_k = a_l;
    }
#endif
    gl_Position = vp * vec4(vpos, 1.0);
    v_uvd = v_uv;
    v_wp = vpos;
    v_fg = fog_factor(length(vpos - eye.xyz));
}
"#;

const CHUNK_FS: &str = r#"
uniform sampler2D u_tex;
const float TW = @TW@.0;
const float TH = @TH@.0;
const float MIPS = @MIPS@.0;
const float ITEMS_Y = @IY@.0;
const float CHAR_Y = @CY@.0;
const vec2 SIZE = vec2(@AW@.0, @AH@.0);
const float S_BOT = @SB@;
const float S_Z = @SZ@;
const float S_X = @SX@;
const mat3 R709_TO_XYZ = mat3(vec3(0.4124, 0.3576, 0.1805), vec3(0.2126, 0.7152, 0.0722), vec3(0.0193, 0.1192, 0.9505));
const mat3 XYZ_TO_2020 = mat3(vec3(1.7166084, -0.3556621, -0.2533601), vec3(-0.6666829, 1.6164776, 0.0157685), vec3(0.0176422, -0.0427763, 0.94222867));
vec3 to_working(vec3 c) { return (c * R709_TO_XYZ) * XYZ_TO_2020; }

centroid in vec2 v_uv;
centroid in vec3 v_tint;
centroid in float v_lit;
centroid in float v_fg;
flat in float v_k;
in vec3 v_wp;
in vec2 v_uvd;
layout(location = 0) out vec4 o_color;

// The one atlas read: tile-clamped by half a texel of the coarsest mip touched (see gpu/pipeline.rs).
vec4 atlas_sample_g(vec2 uv, vec2 gx, vec2 gy) {
    vec2 p = uv * SIZE;
    if ((p.y >= 256.0 && p.y < ITEMS_Y) || p.y >= CHAR_Y) { return textureLod(u_tex, uv, 0.0); }
    float g = max(length(gx * SIZE), length(gy * SIZE));
    float pad = 0.5 * exp2(min(ceil(log2(max(g, 1.0))), MIPS - 1.0));
    vec2 o = floor(p / 16.0) * 16.0;
    return textureGrad(u_tex, clamp(p, o + vec2(pad), o + vec2(16.0 - pad)) / SIZE, gx, gy);
}

float flat_shade(vec3 wp, vec3 dx, vec3 dy) {
    vec3 c = cross(dx, dy);
    if (dot(c, c) < 1e-20) { return 1.0; }
    vec3 a = abs(c);
    if (a.x >= a.y && a.x >= a.z) { return S_X; }
    if (a.y >= a.z) { return (c.y * dot(c, eye.xyz - wp) > 0.0) ? 1.0 : S_BOT; }
    return S_Z;
}

vec2 far_uv(vec2 uv, float k) {
    float t = floor(k - 1000.0 + 0.5);
    vec2 o = vec2(mod(t, 16.0), floor(t / 16.0)) * 16.0;
    return (o + clamp(vec2(fract(uv.x), 1.0 - fract(uv.y)) * 16.0, vec2(0.02), vec2(15.98))) / SIZE;
}

vec4 frag(bool cut, float shade) {
    vec2 uv = v_uv;
    vec2 gx = dFdx(v_uvd);
    vec2 gy = dFdy(v_uvd);
    if (v_k >= 1000.0) {
        uv = far_uv(v_uv, v_k);
        gx = gx * 16.0 / SIZE;
        gy = gy * 16.0 / SIZE;
    }
    vec4 c = atlas_sample_g(uv, gx, gy);
    float cover = 1.0;
    if (cut) {
        float a = c.a;
        if (max(length(gx * SIZE), length(gy * SIZE)) > 1.0) { a = textureLod(u_tex, uv, 0.0).a; }
#if A2C
        cover = clamp((a - 0.5) * 3.0 + 0.5, 0.0, 1.0);
        if (cover <= 0.0) { discard; }
#else
        if (a < 0.5) { discard; }
#endif
    }
    float lit = v_lit * shade;
    vec3 rgb = c.rgb * v_tint;
    // Grass side (tile 3, untinted): tile 38 over it, cut out and tinted, on the one quad.
    if (uint(uv.y * TH) * uint(TW) + uint(uv.x * TW) == 3u) {
        vec2 ouv = uv + vec2(3.0 / TW, 2.0 / TH);
        rgb = c.rgb;
        if (textureLod(u_tex, ouv, 0.0).a >= 0.5) { rgb = atlas_sample_g(ouv, gx, gy).rgb * v_tint; }
    }
    return vec4(mix(to_working(rgb * lit), fog.rgb, v_fg), cover);
}

void main() {
#if defined(FS_SOLID)
    o_color = frag(false, 1.0);
#elif defined(FS_CUT)
    o_color = frag(true, 1.0);
#elif defined(FS_CUT_FLAT)
    vec3 dx = dFdx(v_wp);
    vec3 dy = dFdy(v_wp);
    float s = 1.0;
    if (v_k >= 0.0) { s = flat_shade(v_wp, dx, dy); }
    o_color = frag(true, s);
#else
    vec3 dx = dFdx(v_wp);
    vec3 dy = dFdy(v_wp);
    bool sheet = v_k >= 1000.0;
    if ((length(v_wp - eye.xyz) > fogp.y) != sheet) { discard; }
    vec2 uv = v_uv;
    vec2 gx = dFdx(v_uvd);
    vec2 gy = dFdy(v_uvd);
    if (sheet) {
        uv = far_uv(v_uv, v_k);
        gx = gx * 16.0 / SIZE;
        gy = gy * 16.0 / SIZE;
    }
    vec4 c = atlas_sample_g(uv, gx, gy);
    if (sheet) { c = vec4(c.rgb * v_tint, c.a); }
    o_color = vec4(mix(to_working(c.rgb * v_lit * flat_shade(v_wp, dx, dy)), fog.rgb, v_fg), c.a);
#endif
}
"#;

fn splice(s: &str) -> String {
    s.replace("@TW@", &atlas::TILES_W.to_string())
        .replace("@TH@", &(atlas::ATLAS_H / 16).to_string())
        .replace("@MIPS@", &atlas::MIPS.to_string())
        .replace("@IY@", &atlas::ITEMS_Y.to_string())
        .replace("@CY@", &atlas::CHAR_Y.to_string())
        .replace("@AW@", &atlas::ATLAS_W.to_string())
        .replace("@AH@", &atlas::ATLAS_H.to_string())
        .replace("@SB@", &format!("{:?}", 0.5f32.powf(2.2)))
        .replace("@SZ@", &format!("{:?}", 0.8f32.powf(2.2)))
        .replace("@SX@", &format!("{:?}", 0.6f32.powf(2.2)))
}

/// The chunk program `which`; `a2c`: the scene pass is multisampled and the cut-out layers write coverage.
pub fn chunk(which: Chunk, a2c: bool) -> Src {
    let mut defs = String::new();
    if which.compact() {
        defs += "#define COMPACT\n";
    }
    if which.water() {
        defs += "#define WATER\n";
    }
    let frag = match which {
        Chunk::Solid => "#define FS_SOLID\n",
        Chunk::Cutout => "#define FS_CUT\n",
        Chunk::Entity | Chunk::Lod => "#define FS_CUT_FLAT\n",
        Chunk::Water | Chunk::LodWater => "",
    };
    Src {
        vs: format!("{HEAD_V}{defs}{CHUNK_COMMON}{CHUNK_VS}"),
        fs: splice(&format!("{HEAD_F}{defs}{frag}#define A2C {}\n{CHUNK_COMMON}{CHUNK_FS}", a2c as u8)),
    }
}

const FULL_VS: &str = "layout(location = 0) in vec3 unused;\nvoid main() { vec2 q = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2)); gl_Position = vec4(q * 2.0 - 1.0, 0.0, 1.0); }\n";

/// The final pass: (FXAA +) CAS + upscale + 2020 -> 709 + vignette + sRGB encode (the window surface is not sRGB).
pub fn present(fxaa: bool, fast: bool) -> Src {
    let fs = r#"
uniform sampler2D u_src;
uniform vec4 u_px; // 1 / scene size, 1 / surface size
layout(location = 0) out vec4 o_color;
const float CAS_SHARPNESS = 0.5;
const float VIGNETTE = 0.04;
const mat3 R2020_TO_XYZ = mat3(vec3(0.6369736, 0.1446172, 0.1688585), vec3(0.2627066, 0.6779996, 0.0592938), vec3(0.0, 0.0280728, 1.0608437));
const mat3 XYZ_TO_709 = mat3(vec3(3.2406, -1.5372, -0.4986), vec3(-0.9689, 1.8758, 0.0415), vec3(0.0557, -0.2040, 1.0570));
vec3 tap(vec2 uv) { return sqrt(max(textureLod(u_src, uv, 0.0).rgb, vec3(0.0))); }
float luma(vec3 c) { return dot(c, vec3(0.299, 0.587, 0.114)); }
vec3 enhanced(vec2 uv, vec2 o) {
    vec3 m = tap(uv);
    vec3 nw = tap(uv + vec2(-o.x, -o.y));
    vec3 ne = tap(uv + vec2(o.x, -o.y));
    vec3 sw = tap(uv + vec2(-o.x, o.y));
    vec3 se = tap(uv + vec2(o.x, o.y));
    vec3 f = m;
#if FXAA
    float l_nw = luma(nw); float l_ne = luma(ne); float l_sw = luma(sw); float l_se = luma(se);
    float lo = min(luma(m), min(min(l_nw, l_ne), min(l_sw, l_se)));
    float hi = max(luma(m), max(max(l_nw, l_ne), max(l_sw, l_se)));
    if (hi - lo >= max(0.04, hi * 0.125)) {
        vec2 dir = vec2(-((l_nw + l_ne) - (l_sw + l_se)), (l_nw + l_sw) - (l_ne + l_se));
        float reduce = max((l_nw + l_ne + l_sw + l_se) * (0.25 * 0.125), 1.0 / 128.0);
        dir = clamp(dir / (min(abs(dir.x), abs(dir.y)) + reduce), vec2(-8.0), vec2(8.0)) * o;
        vec3 a = 0.5 * (tap(uv + dir * (1.0 / 3.0 - 0.5)) + tap(uv + dir * (2.0 / 3.0 - 0.5)));
        vec3 b = a * 0.5 + 0.25 * (tap(uv - dir * 0.5) + tap(uv + dir * 0.5));
        f = (luma(b) < lo || luma(b) > hi) ? a : b;
    }
#endif
    vec3 mn = min(min(min(nw, ne), min(sw, se)), f);
    vec3 mx = max(max(max(nw, ne), max(sw, se)), f);
    vec3 w = clamp(min(mn, vec3(1.0) - mx) / max(mx, vec3(1e-4)), vec3(0.0), vec3(1.0));
    w = (vec3(1.0) - (vec3(1.0) - w) * (vec3(1.0) - w)) * (-1.0 / mix(8.0, 5.0, CAS_SHARPNESS));
    vec3 g = clamp(((nw + ne + sw + se) * w + f) / (vec3(1.0) + 4.0 * w), vec3(0.0), vec3(1.0));
    return g * g;
}
void main() {
    vec2 uv = gl_FragCoord.xy * u_px.zw;
#if FAST
    vec3 l = max(textureLod(u_src, uv, 0.0).rgb, vec3(0.0));
#else
    vec3 l = enhanced(uv, u_px.xy);
#endif
    vec3 c = clamp((l * R2020_TO_XYZ) * XYZ_TO_709, vec3(0.0), vec3(1.0));
    float k = uv.x * uv.y;
    c = c * pow(max(16.0 * (k - uv.x) * (k - uv.y), 1e-4), VIGNETTE);
    c = mix(1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, 12.92 * c, vec3(lessThanEqual(c, vec3(0.0031308))));
    o_color = vec4(c, 1.0);
}
"#;
    Src { vs: format!("{HEAD_V}{FULL_VS}"), fs: format!("{HEAD_F}#define FXAA {}\n#define FAST {}\n{fs}", fxaa as u8, fast as u8) }
}

/// The HUD quads over the final frame (`render/hud.rs`); `sheet_h` = the HUD texture's height. The surface is not sRGB, so no `lin`.
pub fn hud(sheet_h: f32) -> Src {
    let vs = r#"
layout(location = 0) in vec2 a_pos;
layout(location = 1) in vec4 a_color;
layout(location = 2) in vec2 a_uv;
uniform vec2 u_size;
out vec4 v_color;
out vec2 v_uv;
void main() {
    gl_Position = vec4(a_pos.x * 2.0 / u_size.x - 1.0, 1.0 - a_pos.y * 2.0 / u_size.y, 0.0, 1.0);
    v_color = a_color;
    v_uv = a_uv;
}
"#;
    let fs = format!(
        r#"
uniform sampler2D u_items;
const vec2 SHEET = vec2(256.0, {sheet_h:.1});
in vec4 v_color;
in vec2 v_uv;
layout(location = 0) out vec4 o_color;
void main() {{
    vec2 p = v_uv * SHEET;
    vec2 w = max(fwidth(p), vec2(1e-4));
    vec2 e = floor(p + 0.5);
    vec2 q = (e + clamp((p - e) / w, vec2(-0.5), vec2(0.5))) / SHEET;
    if (v_uv.x < 0.0) {{ o_color = v_color; return; }}
    vec4 t = textureLod(u_items, q, 0.0);
    if (t.a < 0.5) {{ discard; }}
    o_color = vec4(t.rgb * v_color.rgb, t.a * v_color.a);
}}
"#
    );
    Src { vs: format!("{HEAD_V}{vs}"), fs: format!("{HEAD_F}{fs}") }
}

/// The title screen's cube map (`render/panorama.rs`).
pub fn panorama() -> Src {
    let vs = r#"
uniform mat4 u_m;
uniform vec4 u_t;
out vec2 v_e;
void main() {
    vec2 q = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2)) * 2.0 - 1.0;
    gl_Position = vec4(q, 0.0, 1.0);
    v_e = q * u_t.xy;
}
"#;
    let fs = r#"
uniform sampler2DArray u_tex;
uniform mat4 u_m;
in vec2 v_e;
layout(location = 0) out vec4 o_color;
void main() {
    vec3 w = (u_m * vec4(v_e, -1.0, 0.0)).xyz;
    vec3 a = abs(w);
    float f = 0.0;
    vec3 c = w;
    if (a.z >= a.x && a.z >= a.y) {
        if (w.z < 0.0) { f = 2.0; c = vec3(-w.x, w.y, -w.z); }
    } else if (a.x >= a.y) {
        if (w.x > 0.0) { f = 1.0; c = vec3(-w.z, w.y, w.x); } else { f = 3.0; c = vec3(w.z, w.y, -w.x); }
    } else if (w.y > 0.0) { f = 5.0; c = vec3(w.x, -w.z, w.y); } else { f = 4.0; c = vec3(w.x, w.z, -w.y); }
    vec2 uv = (c.xy / c.z + 1.0) * 0.5;
    o_color = vec4(textureLod(u_tex, vec3(uv, f), 0.0).rgb, 1.0);
}
"#;
    Src { vs: format!("{HEAD_V}{vs}"), fs: format!("{HEAD_F}{fs}") }
}

/// Rain and snow (`render/weather.rs`); premultiplied output.
pub fn weather(snow_opacity: f32, rain_opacity: f32) -> Src {
    let vs = r#"
layout(location = 0) in vec3 a_p;
layout(location = 1) in vec2 a_uv;
layout(location = 2) in vec4 a_tint;
uniform mat4 u_vp;
uniform vec4 u_eye;
out vec2 v_uv;
out vec4 v_tint;
void main() {
    vec2 tilt = 0.25 * vec2(cos(0.5235987756), sin(0.5235987756));
    float wave = 0.7 + 0.3 * sin(dot(a_p, vec3(5.0)));
    float dy = a_p.y - u_eye.y;
    vec3 w = vec3(a_p.x - tilt.x * wave * dy, a_p.y, a_p.z - tilt.y * wave * dy);
    gl_Position = u_vp * vec4(w, 1.0);
    v_uv = a_uv;
    v_tint = a_tint;
}
"#;
    let fs = format!(
        r#"
uniform sampler2D u_tex;
uniform vec4 u_rain;
uniform vec4 u_snow;
in vec2 v_uv;
in vec4 v_tint;
layout(location = 0) out vec4 o_color;
void main() {{
    vec4 base = texture(u_tex, v_uv);
    if (base.a < 0.1) {{ discard; }}
    bool is_rain = abs(base.r - base.b) > 1e-6;
    vec4 c = is_rain ? vec4(u_rain.rgb, {rain_opacity:?} * base.a) * v_tint : vec4(u_snow.rgb, {snow_opacity:?} * base.a) * v_tint;
    o_color = vec4(c.rgb * c.a, c.a);
}}
"#
    );
    Src { vs: format!("{HEAD_V}{vs}"), fs: format!("{HEAD_F}{fs}") }
}

const SKY_BLOCK: &str = r#"
layout(std140) uniform S { mat4 inv_vp; vec4 sun_dir; vec4 moon_dir; mat4 celestial; vec4 misc; vec4 zenith; vec4 horizon; vec4 stars; mat4 vp; };
"#;

/// The sky dome (`fs_legacy`): one triangle at depth 1.
pub fn sky_dome() -> Src {
    let vs = r#"
out vec2 v_ndc;
void main() {
    vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2)) * 2.0 - 1.0;
    gl_Position = vec4(p, 1.0, 1.0);
    v_ndc = p;
}
"#;
    let fs = r#"
in vec2 v_ndc;
layout(location = 0) out vec4 o_color;
void main() {
    vec4 p = inv_vp * vec4(v_ndc, 1.0, 1.0);
    vec3 r = normalize(p.xyz / p.w);
    o_color = vec4(mix(horizon.rgb, zenith.rgb, clamp(r.y * 2.5, 0.0, 1.0)), 1.0);
}
"#;
    Src { vs: format!("{HEAD_V}{SKY_BLOCK}{vs}"), fs: format!("{HEAD_F}{SKY_BLOCK}{fs}") }
}

const GEO_CLIP: &str = "vec4 geo_clip(vec3 w) { vec4 c = vp * vec4(w, 1.0); return vec4(c.xy, c.w * 0.99998, c.w); }\n";

/// Stars (`fan` false) or the sunrise fan (`fan` true): real geometry just under the far plane, over the dome.
pub fn sky_geo(fan: bool) -> Src {
    let vs = if fan {
        "layout(location = 0) in vec3 a_p;\nlayout(location = 1) in vec4 a_col;\nout vec4 v_col;\nvoid main() { gl_Position = geo_clip(a_p); v_col = a_col; }\n"
    } else {
        "layout(location = 0) in vec3 a_p;\nout vec4 v_col;\nvoid main() { mat3 m = mat3(celestial[0].xyz, celestial[1].xyz, celestial[2].xyz); gl_Position = geo_clip(transpose(m) * a_p); v_col = stars; }\n"
    };
    let fs = "in vec4 v_col;\nlayout(location = 0) out vec4 o_color;\nvoid main() { o_color = v_col; }\n";
    Src { vs: format!("{HEAD_V}{SKY_BLOCK}{GEO_CLIP}{vs}"), fs: format!("{HEAD_F}{fs}") }
}

/// The sun and moon quads, added over the sky.
pub fn sky_body() -> Src {
    let vs = r#"
layout(location = 0) in vec3 a_c;
out vec2 v_uv;
void main() {
    bool moon = a_c.z > 0.5;
    vec3 dir = moon ? moon_dir.xyz : sun_dir.xyz;
    float extent = moon ? 20.0 : 30.0;
    vec3 side = vec3(1.0, 0.0, 0.0);
    gl_Position = geo_clip(dir * 100.0 + side * (a_c.x * extent) + cross(dir, side) * (a_c.y * extent));
    v_uv = vec2((a_c.x * 0.5 + 0.5) * 0.5 + (moon ? 0.5 : 0.0), 0.5 - a_c.y * 0.5);
}
"#;
    let fs = r#"
uniform sampler2D u_body;
in vec2 v_uv;
layout(location = 0) out vec4 o_color;
vec3 to_working(vec3 c) {
    return vec3(dot(vec3(0.62742569, 0.32928911, 0.04335024), c), dot(vec3(0.06902744, 0.91957858, 0.01136138), c), dot(vec3(0.01636642, 0.08802890, 0.89568432), c));
}
void main() { o_color = vec4(to_working(texture(u_body, v_uv).rgb), 1.0 - misc.x); }
"#;
    Src { vs: format!("{HEAD_V}{SKY_BLOCK}{GEO_CLIP}{vs}"), fs: format!("{HEAD_F}{SKY_BLOCK}{fs}") }
}
