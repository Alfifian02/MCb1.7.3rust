//! GLSL ES 3.00 sources of the OpenGL ES renderer (valid on any 3.x context, 3.2 included): ports of the WGSL of `gpu/pipeline.rs`,
//! `render/{present,hud,weather,panorama}.rs` and the legacy half of `render/sky.wgsl`, line for line, with the same constants spliced in.
//! Conventions that differ from wgpu: GL clip depth is -1..1 (`depth_fix` on the CPU side), the window surface is not sRGB (the final
//! shader encodes), the scene target is `SRGB8_ALPHA8`. `tools/gles_check.py` compiles and draws every program on Mesa (`cargo test`
//! does the same through `gles::tests` when an EGL driver exists).

use crate::render::{atlas, look};

const HEAD_V: &str = "#version 300 es\nprecision highp float;\nprecision highp int;\n";
const HEAD_F: &str = "#version 300 es\nprecision highp float;\nprecision highp int;\nprecision highp sampler2D;\nprecision highp sampler2DArray;\n";

/// A vertex + fragment source pair.
pub struct Src {
    pub vs: String,
    pub fs: String,
}

/// Which chunk-shader program: the seven pipelines of `gpu::pipeline::ChunkPipeline`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Chunk {
    Solid,
    Cutout,
    Entity,
    Lod,
    Water,
    LodWater,
    /// Additive soft discs (`render::look`: halos, motes); the entity vertex, `fs_glow`.
    Glow,
}

impl Chunk {
    pub const ALL: [Chunk; 7] = [Chunk::Solid, Chunk::Cutout, Chunk::Entity, Chunk::Lod, Chunk::Water, Chunk::LodWater, Chunk::Glow];
    /// The 12-byte `embeddium` vertex + chunk origin (else the 32-byte float vertex).
    pub fn compact(self) -> bool {
        matches!(self, Chunk::Solid | Chunk::Cutout | Chunk::Water)
    }
    /// The opaque and cut-out chunk layers: the compact vertex and the lean varyings (no world position, flat tint and tile).
    pub fn lean(self) -> bool {
        self.compact() && !self.water()
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
layout(std140) uniform U { mat4 vp; vec4 eye; vec4 fog; vec4 fogp; vec4 warm; vec4 cool; vec4 sun; vec4 fx; vec4 bolt; };
// `render::look`: the extra exponent of the face shades (`warm.w`).
float boost(float s) { return pow(s, 1.0 + warm.w); }
float boosted(float s, float sb) { return warm.w > 0.0 ? sb : s; }
float fog_factor(float d) {
    if (fog.w < 0.0) { return 1.0 - exp(fog.w * d); }
    if (fog.w == 0.0) { return 0.0; }
    float s0 = fogp.x > 0.0 ? fogp.x : 0.25;
    float lin = clamp((d - s0 * fog.w) / ((1.0 - s0) * fog.w), 0.0, 1.0);
    float k = d * fogp.z; // the biome's haze (`world::atmos`)
    return 1.0 - (1.0 - lin) * exp(-k * k);
}
// World.lightBrightnessTable; sky and blk are light levels / 15, eye.w the sky light the time of day takes away.
float brightness(float sky, float blk) {
    float l = max(max(sky - eye.w, 0.0), blk);
    return (l / (4.0 - 3.0 * l)) * 0.95 + 0.05;
}
// A lightning bolt's light (`bolt`, see `bolt_lit` in gpu/pipeline.rs, line for line).
float bolt_lit(float lit, vec3 wp, float shade) {
    if (bolt.w <= 0.0) { return lit; }
    float f = clamp(1.0 - length(wp - bolt.xyz) / 96.0, 0.0, 1.0);
    return lit + (1.0 - lit) * bolt.w * (0.15 + 0.7 * f * f) * shade;
}
"#;

const CHUNK_VS: &str = r#"
#ifdef COMPACT
layout(location = 0) in uvec3 a_w;
uniform vec3 u_origin;
#else
layout(location = 0) in vec3 a_p;
layout(location = 1) in vec2 a_uv;
layout(location = 2) in float a_l;
layout(location = 3) in float a_lm;
layout(location = 4) in float a_t;
#endif
#ifdef LEAN
centroid out vec2 v_uv;
flat out vec3 v_tint;
centroid out float v_lit;
centroid out float v_fg;
flat out float v_k;
out vec2 v_uvd;
#else
centroid out vec2 v_uv;
centroid out vec3 v_tint;
centroid out float v_lit;
centroid out float v_fg;
flat out float v_k;
out vec3 v_wp;
out vec2 v_uvd;
#endif
#ifdef COMPACT
const vec2 SIZE = vec2(@AW@.0, @AH@.0);
// `vertex::shade_value`: face shade 1, 0.8, 0.6, 0.5 times 1 - 0.2 * ambient occlusion step.
float shade_of(uint code) {
    uint f = code & 3u;
    float s = 1.0;
    if (f == 1u) { s = 0.8; } else if (f == 2u) { s = 0.6; } else if (f == 3u) { s = 0.5; }
    return boost(s) * (1.0 - 0.2 * float(code >> 2u));
}
#endif
void main() {
#ifdef COMPACT
    // Three words (`embeddium::vertex`): position + repeat bit, tile + texel position, colour + light + shade.
    uint w0 = a_w.x;
    uint w1 = a_w.y;
    uint w2 = a_w.z;
    vec3 vpos = u_origin + vec3(float(w0 & 511u) / 16.0 - 8.0, float((w0 >> 18u) & 8191u) / 32.0 - 8.0, float((w0 >> 9u) & 511u) / 16.0 - 8.0);
    vec2 tl = vec2(float((w1 >> 10u) & 2047u), float(w1 >> 21u)) * 0.25 - 240.0;
    uint tile = w1 & 1023u;
    // Leaves (tiles 53, 133, 255) sway a little (`fx.z` blocks, 0 = off): a smooth function of the position, so shared corners move together.
    if (fx.z > 0.0 && (tile == 53u || tile == 133u || tile == 255u)) {
        float sx = sin(fx.x * 1.3 + vpos.x * 0.9 + vpos.z * 0.6);
        float sz = cos(fx.x * 1.1 + vpos.z * 0.8 + vpos.y * 0.5);
        vpos += vec3(sx, 0.25 * (sx - sz), sz) * fx.z;
    }
    v_tint = pow(vec3(float(w2 & 31u) / 31.0, float((w2 >> 5u) & 63u) / 63.0, float((w2 >> 11u) & 31u) / 31.0), vec3(2.2));
    float sh = shade_of(w2 >> 27u);
    v_lit = bolt_lit(pow(brightness(float((w2 >> 16u) & 63u) / 60.0, float((w2 >> 22u) & 31u) / 30.0) * sh, 2.2), vpos, sh);
#ifdef LEAN
    v_uv = tl / 16.0;
    v_k = ((w0 >> 31u) != 0u) ? -float(tile) - 1.0 : float(tile);
#else
    v_uv = (vec2(float(tile % 16u), float(tile / 16u)) * 16.0 + tl) / SIZE;
    v_k = -1.0;
#endif
#else
    vec3 vpos = a_p;
    uint tp = uint(a_t);
    v_tint = pow(vec3(float((tp >> 16u) & 255u), float((tp >> 8u) & 255u), float(tp & 255u)) / 255.0, vec3(2.2));
    v_uv = a_uv;
#ifndef WATER
    if (a_lm >= 256.0) {
        float v = a_lm - 256.0;
        float s = floor(v / 32.0 + 0.001);
        v_lit = bolt_lit(pow(brightness(s / 60.0, (v - s * 32.0) / 30.0) * a_l, 2.2), vpos, a_l);
        v_k = -1.0;
    } else
#endif
    {
        float s = floor(a_lm / 16.0 + 0.001);
        v_lit = bolt_lit(pow(brightness(s / 15.0, (a_lm - s * 16.0) / 15.0), 2.2), vpos, 1.0);
        v_k = a_l;
    }
#endif
    gl_Position = vp * vec4(vpos, 1.0);
    v_uvd = v_uv;
#ifndef LEAN
    v_wp = vpos;
#endif
    v_fg = fog_factor(length(vpos - eye.xyz));
}
"#;

const CHUNK_FS: &str = r#"
uniform mediump sampler2D u_tex;
const float TW = @TW@.0;
const float TH = @TH@.0;
const float MIPS = @MIPS@.0;
const float ITEMS_Y = @IY@.0;
const float CHAR_Y = @CY@.0;
const vec2 SIZE = vec2(@AW@.0, @AH@.0);
const float S_BOT = @SB@;
const float S_Z = @SZ@;
const float S_X = @SX@;
// With Warm Light's extra exponent baked (`look::boosted`): `flat_shade` runs per pixel on the far terrain, a `pow` there is too dear.
const float S_BOT_B = @SBB@;
const float S_Z_B = @SZB@;
const float S_X_B = @SXB@;
// Rec. 709 -> XYZ -> Rec. 2020 folded into one matrix (the product of the two the wgpu path multiplies by), and run at half precision.
const mat3 W709_TO_2020 = mat3(vec3(0.627425692, 0.329289106, 0.0433502375), vec3(0.0690274419, 0.91957858, 0.0113613785), vec3(0.0163664152, 0.0880288984, 0.895684319));
mediump vec3 to_working(mediump vec3 c) { return c * W709_TO_2020; }
// `render::look` (see `finish` in gpu/pipeline.rs): lit side `warm.rgb`, shaded side `cool.rgb`, a faint blue, brighter tint with the fog factor.
const vec3 AER = vec3(0.95, 0.99, 1.07);
const vec3 WATER_DEEP = vec3(0.55, 0.65, 0.85); // multipliers of the biome-coloured water, as in gpu/pipeline.rs
const vec3 WATER_SHALLOW = vec3(0.80, 1.15, 1.10);
mediump vec3 finish(mediump vec3 rgb, mediump float lit, mediump float fg) {
    mediump vec3 c = to_working(rgb * lit * mix(cool.rgb, warm.rgb, smoothstep(0.15, 0.7, lit)));
    return mix(c * mix(vec3(1.0), AER, fg * cool.w), fog.rgb, fg);
}

#ifdef LEAN
centroid in vec2 v_uv;
flat in mediump vec3 v_tint;
centroid in mediump float v_lit;
centroid in mediump float v_fg;
flat in float v_k;
in vec2 v_uvd;
#else
centroid in vec2 v_uv;
centroid in mediump vec3 v_tint;
centroid in mediump float v_lit;
centroid in mediump float v_fg;
flat in float v_k;
in vec3 v_wp;
in vec2 v_uvd;
#endif
layout(location = 0) out vec4 o_color;

// The one atlas read: tile-clamped by half a texel of the coarsest mip touched (see gpu/pipeline.rs).
// Squared texel footprint (no sqrt: `log2(max(g, 1)) == 0.5 * log2(max(g * g, 1))`, `g > 1` == `g * g > 1`).
float grad2(vec2 gx, vec2 gy) {
    vec2 a = gx * SIZE;
    vec2 b = gy * SIZE;
    return max(dot(a, a), dot(b, b));
}
mediump vec4 atlas_sample_g(vec2 uv, vec2 gx, vec2 gy) {
    vec2 p = uv * SIZE;
    if ((p.y >= 256.0 && p.y < ITEMS_Y) || p.y >= CHAR_Y) { return textureLod(u_tex, uv, 0.0); }
    float pad = 0.5 * exp2(min(ceil(0.5 * log2(max(grad2(gx, gy), 1.0))), MIPS - 1.0));
    vec2 o = floor(p / 16.0) * 16.0;
    return textureGrad(u_tex, clamp(p, o + vec2(pad), o + vec2(16.0 - pad)) / SIZE, gx, gy);
}

float flat_shade(vec3 wp, vec3 dx, vec3 dy) {
    vec3 c = cross(dx, dy);
    if (dot(c, c) < 1e-20) { return 1.0; }
    vec3 a = abs(c);
    if (a.x >= a.y && a.x >= a.z) { return boosted(S_X, S_X_B); }
    if (a.y >= a.z) { return (c.y * dot(c, eye.xyz - wp) > 0.0) ? 1.0 : boosted(S_BOT, S_BOT_B); }
    return boosted(S_Z, S_Z_B);
}

vec2 far_uv(vec2 uv, float k) {
    float t = floor(k - 1000.0 + 0.5);
    vec2 o = vec2(mod(t, 16.0), floor(t / 16.0)) * 16.0;
    return (o + clamp(vec2(fract(uv.x), 1.0 - fract(uv.y)) * 16.0, vec2(0.02), vec2(15.98))) / SIZE;
}

#ifndef LEAN
vec4 frag(bool cut, float shade) {
    mediump vec3 fc = fog.rgb;
    vec2 uv = v_uv;
    vec2 gx = dFdx(v_uvd);
    vec2 gy = dFdy(v_uvd);
    if (v_k >= 1000.0) {
        uv = far_uv(v_uv, v_k);
        gx = gx * 16.0 / SIZE;
        gy = gy * 16.0 / SIZE;
    }
    mediump vec4 c = atlas_sample_g(uv, gx, gy);
    float cover = 1.0;
    if (cut) {
        float a = c.a;
        if (grad2(gx, gy) > 1.0) { a = textureLod(u_tex, uv, 0.0).a; }
#if A2C
        cover = clamp((a - 0.5) * 3.0 + 0.5, 0.0, 1.0);
        if (cover <= 0.0) { discard; }
#else
        if (a < 0.5) { discard; }
#endif
    }
    mediump float lit = v_lit * shade;
    mediump vec3 rgb = c.rgb * v_tint;
    // Grass side (tile 3, untinted): tile 38 over it, cut out and tinted, on the one quad.
    if (uint(uv.y * TH) * uint(TW) + uint(uv.x * TW) == 3u) {
        vec2 ouv = uv + vec2(3.0 / TW, 2.0 / TH);
        rgb = c.rgb;
        if (textureLod(u_tex, ouv, 0.0).a >= 0.5) { rgb = atlas_sample_g(ouv, gx, gy).rgb * v_tint; }
    }
    // `RenderLiving`: a hurt mob is drawn again in red at alpha 0.4 over the same pixels (`render::items::HURT`: `v_k` is 2..3), blended in
    // gamma space: 0.6 of the texel and 0.4 of red. Twin of `frag` in gpu/pipeline.rs.
    if (v_k >= 2.0 && v_k < 4.0) { rgb = pow(pow(rgb, vec3(1.0 / 2.2)) * 0.6 + vec3(0.4, 0.0, 0.0), vec3(2.2)); }
    return vec4(finish(rgb, lit, v_fg), cover);
}

// A biome's water: the tile keeps its own light and dark (its luminance against the tile's average, 0.1085 in linear light) and takes the
// biome's colour (`v_tint`, linear). White (no biome) leaves the tile as it is. Twin of `water_rgb` in gpu/pipeline.rs.
mediump vec3 water_rgb(mediump vec3 c, mediump vec3 tint) {
    if (tint.r + tint.g + tint.b >= 2.99) { return c; }
    return tint * (dot(c, vec3(0.2126, 0.7152, 0.0722)) / 0.1085);
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
#elif defined(FS_GLOW)
    float r = 1.0 - dot(v_uv, v_uv);
    if (r <= 0.0) { discard; }
    bool nofog = v_k >= 10.0;
    o_color = vec4(to_working(v_tint), (v_k - (nofog ? 10.0 : 0.0)) * (nofog ? 1.0 : 1.0 - v_fg) * r * r);
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
    mediump vec4 c = atlas_sample_g(uv, gx, gy);
    // The LoD debug sheet (tile 1066) is multiplied by its cyan; every water (real or a sheet, tile 1205) takes its biome's colour.
    if (sheet && v_k < 1200.0) { c = vec4(c.rgb * v_tint, c.a); } else { c = vec4(water_rgb(c.rgb, v_tint), c.a); }
    // Depth gradient (`fx.y`, it scales the biome's colour) and sun glint (`sun.w`), as `fs_water` in gpu/pipeline.rs.
    mediump vec3 col = c.rgb;
    mediump float a = c.a;
    vec3 v = normalize(eye.xyz - v_wp);
    if (fx.y > 0.0) {
        float t = smoothstep(0.1, 0.9, abs(v.y));
        col = col * mix(vec3(1.0), mix(WATER_DEEP, WATER_SHALLOW, t), fx.y);
        a = mix(a, a * 0.82, t * fx.y);
    }
    mediump vec3 rgb = finish(col, v_lit * flat_shade(v_wp, dx, dy), v_fg);
    // The ripples tilt the normal by at most 0.1 in x and z, so a half vector with y <= 0.85 reaches 0.93 at best (0.93^160 < 1e-5): those
    // pixels (nearly all of them) skip the trig, the normalize and the pow.
    if (sun.w > 0.0) {
        vec3 hv = normalize(sun.xyz + v);
        vec3 n3 = cross(dx, dy);
        if (hv.y > 0.85 && abs(n3.y) > 0.8 * length(n3)) {
            vec3 n = normalize(vec3(0.06 * sin(v_wp.x * 2.3 + fx.x * 1.3) + 0.04 * sin(v_wp.z * 3.7 - fx.x * 0.9), 1.0, 0.06 * cos(v_wp.z * 2.1 + fx.x * 1.1) + 0.04 * cos(v_wp.x * 3.1 + fx.x * 0.8)));
            float g = pow(max(dot(n, hv), 0.0), 160.0) * sun.w * (1.0 - v_fg);
            rgb += vec3(1.0, 0.95, 0.8) * g;
            a = max(a, min(g, 1.0));
        }
    }
    o_color = vec4(rgb, a);
#endif
}
#else
// The opaque and cut-out chunk layers (`VsC` of gpu/pipeline.rs): the tile is flat, `v_uv` is the position inside it in tiles, and a
// repeating quad (`v_k < 0`: a merged run of faces) takes its fraction. The face shade and the light are in the vertex.
mediump vec4 tile_sample(vec2 o, vec2 t, vec2 gx, vec2 gy) {
    vec2 p = o + t * 16.0;
    if ((p.y >= 256.0 && p.y < ITEMS_Y) || p.y >= CHAR_Y) { return textureLod(u_tex, p / SIZE, 0.0); }
    float g2 = max(dot(gx, gx), dot(gy, gy));
    float pad = 0.5 * exp2(min(ceil(0.5 * log2(max(g2, 1.0))), MIPS - 1.0));
    return textureGrad(u_tex, (o + clamp(t * 16.0, vec2(pad), vec2(16.0 - pad))) / SIZE, gx / SIZE, gy / SIZE);
}

void main() {
    vec2 gx = dFdx(v_uvd) * 16.0;
    vec2 gy = dFdy(v_uvd) * 16.0;
    bool rep = v_k < 0.0;
    float tile = rep ? -v_k - 1.0 : v_k;
    float row = floor((tile + 0.5) / TW);
    vec2 o = vec2(tile - row * TW, row) * 16.0;
    vec2 t = rep ? fract(v_uv) : v_uv;
    mediump vec4 c = tile_sample(o, t, gx, gy);
    float cover = 1.0;
#if defined(FS_CUT)
    float a = c.a;
    if (max(dot(gx, gx), dot(gy, gy)) > 1.0) { a = textureLod(u_tex, (o + t * 16.0) / SIZE, 0.0).a; }
#if A2C
    cover = clamp((a - 0.5) * 3.0 + 0.5, 0.0, 1.0);
    if (cover <= 0.0) { discard; }
#else
    if (a < 0.5) { discard; }
#endif
#endif
    mediump vec3 rgb = c.rgb * v_tint;
    // Grass side (tile 3, untinted): tile 38 over it, cut out and tinted, on the one quad.
    if (abs(tile - 3.0) < 0.5) {
        vec2 oo = o + vec2(48.0, 32.0);
        rgb = c.rgb;
        if (textureLod(u_tex, (oo + t * 16.0) / SIZE, 0.0).a >= 0.5) { rgb = tile_sample(oo, t, gx, gy).rgb * v_tint; }
    }
    o_color = vec4(finish(rgb, v_lit, v_fg), cover);
}
#endif
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
        .replace("@SBB@", &format!("{:?}", look::boosted(0.5f32.powf(2.2))))
        .replace("@SZB@", &format!("{:?}", look::boosted(0.8f32.powf(2.2))))
        .replace("@SXB@", &format!("{:?}", look::boosted(0.6f32.powf(2.2))))
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
    if which.lean() {
        defs += "#define LEAN\n";
    }
    let frag = match which {
        Chunk::Solid => "#define FS_SOLID\n",
        Chunk::Cutout => "#define FS_CUT\n",
        Chunk::Entity | Chunk::Lod => "#define FS_CUT_FLAT\n",
        Chunk::Glow => "#define FS_GLOW\n",
        Chunk::Water | Chunk::LodWater => "",
    };
    Src {
        vs: splice(&format!("{HEAD_V}{defs}{CHUNK_COMMON}{CHUNK_VS}")),
        fs: splice(&format!("{HEAD_F}{defs}{frag}#define A2C {}\n{CHUNK_COMMON}{CHUNK_FS}", a2c as u8)),
    }
}

const FULL_VS: &str = "layout(location = 0) in vec3 unused;\nvoid main() { vec2 q = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2)); gl_Position = vec4(q * 2.0 - 1.0, 0.0, 1.0); }\n";

/// The final pass: (FXAA +) CAS + upscale + 2020 -> 709 + vignette + sRGB encode (the window surface is not sRGB).
pub fn present(fxaa: bool, fast: bool) -> Src {
    let fs = r#"
uniform sampler2D u_src;
uniform vec4 u_px; // 1 / scene size, 1 / surface size
uniform vec4 u_grade; // the Vibrance dial's grade (`render::look`)
layout(location = 0) out vec4 o_color;
const float CAS_SHARPNESS = 0.5;
const float VIGNETTE = 0.04;
const mat3 R2020_TO_709 = mat3(vec3(1.66034406, -0.587571585, -0.0728802431), vec3(-0.124378681, 1.13283707, -0.00835867706), vec3(-0.0181127169, -0.100583791, 1.11862127));
vec3 tap(vec2 uv) { return sqrt(max(textureLod(u_src, uv, 0.0).rgb, vec3(0.0))); }
float luma(vec3 c) { return dot(c, vec3(0.299, 0.587, 0.114)); }
// Vibrance, S-curve and the warm / cool split (see `grade` in render/present.rs), in sqrt space; `u_grade.x` = 0 skips it.
vec3 grade(vec3 c) {
    vec3 g = sqrt(c);
    float l = luma(g);
    g = mix(vec3(l), g, 1.0 + u_grade.x * (1.0 - (max(g.r, max(g.g, g.b)) - min(g.r, min(g.g, g.b)))));
    g = mix(g, g * g * (3.0 - 2.0 * g), u_grade.y);
    float k = (smoothstep(0.2, 0.8, l) - 0.5) * 2.0 * u_grade.z;
    g = clamp(g + vec3(k, 0.0, -k), vec3(0.0), vec3(1.0));
    return g * g;
}
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
    vec3 c = clamp(l * R2020_TO_709, vec3(0.0), vec3(1.0));
    if (u_grade.x > 0.0) { c = grade(c); }
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
layout(std140) uniform S { mat4 inv_vp; vec4 sun_dir; vec4 moon_dir; mat4 celestial; vec4 misc; vec4 zenith; vec4 horizon; vec4 stars; mat4 vp; vec4 eye; vec4 cloud; vec4 cloudp; vec4 haze; };
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
    // The biome's sky, haze and clouds: the same code as `fs_legacy` / `cloud_march` in `render/sky.wgsl`.
    let fs = r#"
in vec2 v_ndc;
layout(location = 0) out vec4 o_color;
float cloud_hash(ivec2 p) {
    uint h = uint(p.x) * 374761393u + uint(p.y) * 668265263u;
    h = (h ^ (h >> 13u)) * 1274126177u;
    return float(h ^ (h >> 16u)) * (1.0 / 4294967296.0);
}
float cloud_noise(vec2 p) {
    ivec2 c = ivec2(floor(p));
    vec2 f = fract(p);
    vec2 u = f * f * (3.0 - 2.0 * f);
    float a = mix(cloud_hash(c), cloud_hash(c + ivec2(1, 0)), u.x);
    float b = mix(cloud_hash(c + ivec2(0, 1)), cloud_hash(c + ivec2(1, 1)), u.x);
    return mix(a, b, u.y);
}
bool cloud_filled(ivec2 c) {
    vec2 p = vec2(c);
    return 0.65 * cloud_noise(p * 0.2) + 0.35 * cloud_noise(p * 0.55 + vec2(31.0, 17.0)) > cloudp.x;
}
vec2 cloud_march(vec3 ro, vec3 rd) {
    float y0 = cloudp.y;
    float y1 = y0 + 4.0;
    if (rd.y < 0.01 || ro.y > y1) { return vec2(0.0); }
    float t_end = (y1 - ro.y) / rd.y;
    float t = max((y0 - ro.y) / rd.y, 0.0);
    vec2 q0 = (ro.xz + rd.xz * t - vec2(cloudp.z, 0.0)) / 12.0;
    vec2 d = rd.xz / 12.0;
    if (abs(d.x) < 1e-6) { d.x = 1e-6; }
    if (abs(d.y) < 1e-6) { d.y = 1e-6; }
    bvec2 fwd = greaterThan(d, vec2(0.0));
    ivec2 st = ivec2(fwd.x ? 1 : -1, fwd.y ? 1 : -1);
    ivec2 cell = ivec2(floor(q0));
    vec2 tmax = vec2(t) + (vec2(cell) + vec2(fwd.x ? 1.0 : 0.0, fwd.y ? 1.0 : 0.0) - q0) / d;
    vec2 tdelta = abs(1.0 / d);
    float face = ro.y < y0 ? 0.62 : 0.8;
    for (int i = 0; i < 8; i++) {
        if (t >= t_end) { break; }
        if (cloud_filled(cell)) { return vec2(face, t); }
        if (tmax.x < tmax.y) { t = tmax.x; tmax.x += tdelta.x; cell.x += st.x; face = 0.86; }
        else { t = tmax.y; tmax.y += tdelta.y; cell.y += st.y; face = 0.76; }
    }
    return vec2(0.0);
}
void main() {
    vec4 p = inv_vp * vec4(v_ndc, 1.0, 1.0);
    vec3 r = normalize(p.xyz / p.w);
    vec3 col = mix(horizon.rgb, zenith.rgb, pow(clamp(r.y * 2.5, 0.0, 1.0), cloudp.w));
    col = mix(col, haze.rgb, haze.a * smoothstep(0.0, 0.05, r.y) * exp(-r.y * 7.0));
    vec2 c = cloud_march(eye.xyz, r);
    if (c.x > 0.0) {
        float k = c.y * eye.w * 1.5;
        vec3 lit = mix(cloud.rgb * c.x, horizon.rgb, 1.0 - exp(-k * k));
        col = mix(col, lit, cloud.a * (1.0 - smoothstep(1500.0, 2600.0, c.y)));
    }
    o_color = vec4(col, 1.0);
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
