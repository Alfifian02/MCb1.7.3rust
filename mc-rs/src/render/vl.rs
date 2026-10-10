//! Volumetric light ("light shafts"), a port of AstraLex's `lib/atmospherics/volumetricLight.glsl` (the ray march,
//! `program/composite.glsl`) and the `LIGHT_SHAFT` part of `program/composite1.glsl` (blur, colour, blend), overworld,
//! eye not under water. Two passes after the terrain: a half-resolution march through the sun's shadow map into an
//! RGBA8 target (what `colortex1` is in the pack: `march`), then a full-screen blend onto the frame (`composite`, drawn inside the HUD pass).
//! Everything that depends only on the time of day and the weather (`lightCol`, `shadowFade`, the multipliers) is worked
//! out once per frame on the CPU in `params` and is the part the tests check.
//! Deviations from the pack, all because mc-rs has no such input:
//! - dither is `InterleavedGradientNoise` (the pack's `TEXTURED_DITHERING 1` branch), there is no blue-noise texture;
//! - the shadow lookup is mc-rs's (`distort()` of Shadow-Tutorial, depth `z * 0.5 + 0.25`), not `DistortShadow`, and
//!   the march stops at `SHADOW_RADIUS` (the pack's `shadowDistance` is 512): past the map a sample counts as lit;
//! - no coloured shadows / translucent albedo (water and glass are opaque cubes): `shadowCol` is 0, `vlAlbedo` is 0.
//! Not ported: underwater shafts, `SMOKER_LIGHT_SHAFT` (the animated noise; needs `noise.png`), the cave fade
//! (`isEyeInCave` is 0 above y = 5 in the pack), End shafts, the moon-phase fade (b1.7.3 has one moon phase).

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::{BindGroup, BindGroupLayout, Buffer, CommandEncoder, Device, RenderPipeline, Sampler, TextureView};

use crate::gpu::context::Gpu;
use crate::gpu::pipeline::{SHADOW_DISTORT, SHADOW_RES};
use crate::render::camera::{FirstPersonCamera, SHADOW_RADIUS};
use crate::world::sky;

/// `#define LIGHT_SHAFT` (off = no march, and the shadow pass is back to running only in sunlight).
pub const LIGHT_SHAFT: bool = true;
/// `LIGHT_SHAFT_STRENGTH`, the pack's default.
const STRENGTH: f32 = 1.0;
/// Added to the sample's shadow-map depth, like `shadowPosition.z += 0.0001`: the pack's depth range is not ours,
/// this is ~0.25 block (one block = 0.5 / 256 of depth here). UNVERIFIED on a device.
const DEPTH_BIAS: f32 = 0.0005;
/// The surface's own shadow test moves its lookup point this many shadow-map texels along the face normal (Shadow-Tutorial's
/// normal bias) and adds a slope-scaled depth bias; the normal comes from the depth buffer, snapped to an axis. UNVERIFIED on a device.
const NORMAL_BIAS: f32 = 1.0;
/// How dark a fully shadowed, fully sunlit surface gets (the old per-pixel shadow was 0.25). UNVERIFIED: tune on a device.
const SHADOW_DARK: f32 = 0.55;

/// What the shader needs besides the camera: the light, and the per-frame scalars of `composite1.glsl`.
pub struct Params {
    /// Direction toward the light that casts the shadow map: the sun, or the moon from timeAngle 0.5325 to 0.9675.
    pub light: Vec3,
    /// The march runs this frame (and so does the shadow pass, whatever the sun strength): the light is above the horizon.
    pub active: bool,
    /// `vl` multiplier, rgb (`vlColor` and every scalar of `composite1.glsl`); `lightShaftTime`.
    scale: [f32; 3],
    lst: f32,
    /// Weight of the additive branch (`sunVisibility * (1 - rain)`) and `mixedTime`.
    w: f32,
    mt: f32,
    /// `endurance` and `vlPower` of `getVolumetricRays`.
    endurance: f32,
    power: f32,
    rain: f32,
    /// Darkness of a shadowed surface, 0..1 (0 = no shadows: night, rain). The caller sets it (`world::sky::shadow_strength`).
    pub shadow: f32,
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn powv(v: Vec3, e: f32) -> Vec3 {
    Vec3::new(v.x.powf(e), v.y.powf(e), v.z.powf(e))
}

/// `sqrt1` of `functions.glsl`.
fn sqrt1(x: f32) -> f32 {
    1.0 - (1.0 - x) * (1.0 - x)
}

/// `time`: world ticks with the fraction; `angle`: `sky::celestial_angle`; `sky_rgb`: `sky::sky_color`; `eye_y`: camera height.
/// The rest of the pack's uniforms are fixed here: `screenBrightness` 0, no `blindFactor`, default colour settings.
pub fn params(time: f64, angle: f32, rain: f32, sky_rgb: [f32; 3], eye_y: f32) -> Params {
    let wt = (time % 24000.0) as f32; // worldTime
    let ta = wt / 24000.0; // timeAngle
    let sun = sky::sun_dir(angle);
    let light = if ta > 0.5325 && ta < 0.9675 { -sun } else { sun }; // lightVec
    let sv = (sun.y + 0.0625).clamp(0.0, 0.125) * 8.0; // sunVisibility
    let svlsm = (sun.y + 0.125).clamp(0.0, 0.25) * 4.0; // sunVisibilityLSM
    let s = (ta * std::f32::consts::TAU).sin();
    let (day, night) = (s.max(0.0).sqrt(), (-s).max(0.0)); // dayFactor, nightFactor

    // lightColor.glsl with lightColorSettings defaults.
    let c = |r: f32, g: f32, b: f32, k: f32| Vec3::new(r, g, b) * (k / 255.0);
    let (l_m, l_d, l_e) = (c(236.0, 184.0, 132.0, 1.05), c(180.0, 172.0, 164.0, 1.40), c(236.0, 188.0, 132.0, 1.05));
    let l_n = c(156.0, 192.0, 240.0, 1.20 * 0.80 * 0.4 * 0.30); // LIGHT_NI, (brightness * 0.125 + 0.80), 0.4, NIGHT_BRIGHTNESS
    let (a_m, a_d) = (c(212.0, 196.0, 228.0, 0.25 * 1.1), c(156.0, 188.0, 228.0, 0.40 * 1.1));
    let a_n = c(120.0, 164.0, 228.0, 0.65 * 0.70 * 0.495 * 0.30);
    let wc = Vec3::new(168.0, 205.0, 255.0) / 255.0 * 2.0; // weatherCol
    let luma = |v: Vec3| v.dot(Vec3::new(0.299, 0.587, 0.114));
    let mefade = 1.0 - ((ta - 0.5).abs() * 8.0 - 1.5).clamp(0.0, 1.0);
    let dfade = 1.0 - day;
    let dfade_m2 = 1.0 - dfade * dfade.sqrt();
    let c_l = mix_v(l_n, mix_v(mix_v(l_m, l_e, mefade), l_d, dfade_m2), sv);
    let c_l2 = mix_v(c_l, wc * (luma(c_l) * 0.9), rain * 0.6);
    let light_col = c_l2 * c_l2;
    let c_a = mix_v(a_n, mix_v(mix_v(a_m, a_m, mefade), a_d, dfade_m2), sv);
    let c_a2 = mix_v(c_a, wc * (luma(c_a) * 0.9), rain * 0.6);
    let ambient_col = c_a2 * c_a2;

    // composite1.glsl, isEyeInWater == 0.
    let day_col = powv(light_col * light_col, 1.5); // LIGHTSHAFT_CONTRAST_DAY
    let night_col = powv(light_col * light_col * light_col * 10.0, 0.5); // LIGHTSHAFT_CONTRAST_NIGHT
    let mut vl_color = mix_v(night_col, day_col, sv);
    let wsq = wc * wc;
    let mut wsky = wsq * (( ambient_col / wsq).dot(Vec3::new(0.2126729, 0.7151522, 0.0721750)) * 1.4); // GetLuminance
    wsky *= mix(0.70, 1.50, sv); // SKY_RAIN_NIGHT, SKY_RAIN_DAY
    let sky_v = Vec3::from(sky_rgb);
    wsky = wsky.max(sky_v * sky_v * 0.5625) * rain;
    vl_color = mix_v(vl_color * 0.75, wsky, rain * rain);
    let mut sc = vl_color * (2.0 * eye_y - 2.0).exp().clamp(0.0, 1.0);
    let rain_mult = mix(20.0 * 0.25, 0.75 * 0.65, sv); // LIGHT_SHAFT_NIGHT_RAIN_MULTIPLIER, ..._DAY_RAIN_MULTIPLIER
    sc *= mix(1.0, 0.50 * 0.4, sqrt1(day) * (1.0 - rain * 0.8)); // LIGHT_SHAFT_NOON_MULTIPLIER
    sc *= mix(1.00 * 10.0 * (0.91 - night * 0.39), 2.0, sv); // LIGHT_SHAFT_NIGHT_MULTIPLIER
    sc *= mix(1.0, rain_mult * 0.25, rain * rain);
    let (fo1, fi1) = (((wt - 12330.0) / 230.0).clamp(0.0, 1.0), ((wt - 13010.0) / 220.0).clamp(0.0, 1.0));
    let (fo2, fi2) = (((wt - 22770.0) / 220.0).clamp(0.0, 1.0), ((wt - 23440.0) / 230.0).clamp(0.0, 1.0));
    let shadow_fade = 1.1 - (fo1 - fi1 + fo2 - fi2);
    sc *= STRENGTH * 60.0 * (0.25 + day) * shadow_fade; // SUNSET_SUNRISE_LIGHTSHAFT_STRENGTH
    sc *= mix(1.0 - (day * 2.0).min(0.75), 0.05, rain) * 1.75;
    let lst = ((sv - 0.5).abs() * 2.0).powf(10.0);
    let mt = if sv < 0.5 {
        let x = 1.0 - (night - 0.3).max(0.0) / 0.7;
        (1.0 - x * x * x * x) * lst // sqrt3
    } else {
        ((svlsm - 0.5) * 2.0).powi(4)
    };
    Params {
        light,
        active: LIGHT_SHAFT && light.y > 0.0,
        scale: sc.to_array(),
        lst,
        w: sv * (1.0 - rain),
        mt,
        endurance: 1.2 * (2.0 + rain * rain - sv * sv).min(2.0),
        power: (1.75 - rain + sv * 0.25).max(1.0),
        rain,
        shadow: 0.0,
    }
}

fn mix_v(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    a.lerp(b, t)
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct U {
    /// Inverse view: camera space to world.
    cam: [[f32; 4]; 4],
    /// `shadow_vp * cam`: view space straight to shadow clip space (the march needs no world position).
    shadow_cam: [[f32; 4]; 4],
    /// xyz: toward the light, w: depth bias.
    light: [f32; 4],
    /// x, y: tan(fov / 2) x aspect and tan(fov / 2); z, w: near and far.
    ray: [f32; 4],
    /// x: distort K, y: max march distance, z: endurance, w: vlPower.
    a: [f32; 4],
    /// x: additive weight, y: mixedTime, z: rain, w: exponent of the fov falloff.
    b: [f32; 4],
    /// x: `minDistFactor` before the fov falloff, y: `addition`.
    c: [f32; 4],
    /// rgb: colour scale, w: lightShaftTime.
    s: [f32; 4],
    /// xy: frame size in pixels, z: surface shadow darkness (`SHADOW_DARK` x sun strength).
    res: [f32; 4],
}

const SHADER: &str = r#"
const NORMAL_BIAS: f32 = @NB@; // `NORMAL_BIAS` of vl.rs
const SH_RES: f32 = @RES@; // `SHADOW_RES`
struct U {
    cam: mat4x4<f32>,
    shadow_cam: mat4x4<f32>,
    light: vec4<f32>,
    ray: vec4<f32>,
    a: vec4<f32>,
    b: vec4<f32>,
    c: vec4<f32>,
    s: vec4<f32>,
    res: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var depth_tex: texture_depth_2d;
@group(0) @binding(2) var vl_tex: texture_2d<f32>;
@group(0) @binding(3) var vl_samp: sampler;
@group(1) @binding(0) var sh_tex: texture_depth_2d;
@group(1) @binding(1) var sh_samp: sampler_comparison;
@group(1) @binding(2) var ent_tex: texture_depth_2d; // items, mobs, falling blocks: same matrix as sh_tex, redrawn every frame

struct FsIn {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// One triangle over the whole target.
@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> FsIn {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: FsIn;
    o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}

// View-space ray through `uv` (z = -1): view-space depth d is `ray * d`.
fn view_ray(uv: vec2<f32>) -> vec3<f32> {
    return vec3<f32>((uv.x * 2.0 - 1.0) * u.ray.x, (1.0 - uv.y * 2.0) * u.ray.y, -1.0);
}

// View-space position of a full-resolution pixel.
fn view_pos(ip: vec2<i32>) -> vec3<f32> {
    let dims = vec2<i32>(textureDimensions(depth_tex));
    let q = clamp(ip, vec2<i32>(0), dims - 1);
    let z = textureLoad(depth_tex, q, 0);
    return view_ray((vec2<f32>(q) + 0.5) / vec2<f32>(dims)) * (u.ray.z * u.ray.w / (u.ray.w - z * (u.ray.w - u.ray.z)));
}

// World-space normal of the face at `ip` (view position `p0`), snapped to an axis: every face in this world is axis aligned. Each
// screen axis steps toward the neighbour with the smaller depth jump, so a silhouette does not tilt it. ponytail: a pixel on an
// edge between two faces can pick the other face's normal (a one-pixel line, blurred by `fs_comp`); a normal buffer is the upgrade.
fn face_normal(ip: vec2<i32>, p0: vec3<f32>) -> vec3<f32> {
    let a = view_pos(ip + vec2<i32>(2, 0)) - p0;
    let b = p0 - view_pos(ip - vec2<i32>(2, 0));
    let c = view_pos(ip + vec2<i32>(0, 2)) - p0;
    let d = p0 - view_pos(ip - vec2<i32>(0, 2));
    let cr = cross(select(b, a, abs(a.z) < abs(b.z)), select(d, c, abs(c.z) < abs(d.z)));
    if dot(cr, cr) < 1e-12 { return vec3<f32>(0.0); }
    var n = normalize(cr);
    if dot(n, p0) > 0.0 { n = -n; } // toward the eye
    let w = (u.cam * vec4<f32>(n, 0.0)).xyz;
    let m = abs(w);
    if m.x >= m.y && m.x >= m.z { return vec3<f32>(sign(w.x), 0.0, 0.0); }
    if m.y >= m.z { return vec3<f32>(0.0, sign(w.y), 0.0); }
    return vec3<f32>(0.0, 0.0, sign(w.z));
}

// InterleavedGradientNoise() without the frame term (there is no TAA to average it away).
fn ign(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(0.06711056 * p.x + 0.00583715 * p.y));
}

// composite.glsl + getVolumetricRays(), isEyeInWater == 0.
@fragment
fn fs_march(in: FsIn) -> @location(0) vec4<f32> {
    let z = textureLoad(depth_tex, min(vec2<i32>(in.pos.xy) * 2, vec2<i32>(textureDimensions(depth_tex)) - 1), 0);
    let near = u.ray.z;
    let far = u.ray.w;
    let depth = near * far / (far - z * (far - near)); // GetDepth(): view-space distance, `far` for the sky
    let r = view_ray(in.uv);
    let dirw = normalize((u.cam * vec4<f32>(r, 0.0)).xyz);
    let visibility = 0.045 * max(0.0, (dot(dirw, u.light.xyz) + u.a.z) / (u.a.z + 1.0));

    let x = pow(1.0 - (in.uv.x - 0.5) * (in.uv.x - 0.5), u.b.w);
    let max_dist = u.a.y * x;
    let min_dist_factor = u.c.x * x;
    let dither = ign(in.pos.xy);
    var sum = 0.0;
    for (var i = 0; i < 10; i = i + 1) {
        let t = f32(i) + dither + u.c.y;
        let d = t * sqrt(t) * min_dist_factor; // pow(t, 1.5)
        if d >= max_dist || depth < d { break; }
        let c = u.shadow_cam * vec4<f32>(r * d, 1.0);
        if abs(c.x) < 1.0 && abs(c.y) < 1.0 {
            let p = vec3<f32>(c.xy / (length(c.xy) + u.a.x) * 0.5 + 0.5, c.z * 0.5 + 0.25 + u.light.w);
            let q = vec2<f32>(p.x, 1.0 - p.y);
            // An item or mob in the way darkens the sample too, so it throws a shaft-shaped shadow volume along the light.
            let lit = min(textureSampleCompareLevel(sh_tex, sh_samp, q, p.z), textureSampleCompareLevel(ent_tex, sh_samp, q, p.z));
            sum = sum + lit * lit * sqrt(d / max_dist) * 1.5;
        } else {
            sum = sum + 1.0; // outside the shadow map: lit
        }
    }
    var v = pow(sqrt(sum * visibility), u.a.w) * 0.9;
    if v > 0.0 { v = v + (dither - 0.19) / 128.0; }
    // The surface's own shadow (g), 1 = in shadow, 0 = lit or off the map. A face turned away from the light (or parallel to it,
    // like every X face while the sun moves in the YZ plane) has no front face in the culled shadow map to compare with, so it is
    // shadowed outright, like N.L <= 0; a lit face looks the map up one texel out along its normal, with a slope-scaled depth bias.
    var sh = 0.0;
    if u.res.z > 0.0 && z < 1.0 {
        let ip = vec2<i32>(in.pos.xy) * 2;
        let p0 = view_pos(ip);
        let n = face_normal(ip, p0);
        let nl = dot(n, u.light.xyz);
        if nl <= 0.0 {
            sh = 1.0;
        } else {
            let c0 = u.shadow_cam * vec4<f32>(p0, 1.0);
            let k = length(c0.xy) + u.a.x;
            let texel = u.a.y * 2.0 / SH_RES * k * k / u.a.x; // one map texel here, in blocks (distort() stretches the map)
            let nv = vec3<f32>(dot(u.cam[0].xyz, n), dot(u.cam[1].xyz, n), dot(u.cam[2].xyz, n)); // n in view space
            let c = c0 + u.shadow_cam * vec4<f32>(nv * (texel * NORMAL_BIAS), 0.0);
            if abs(c.x) < 1.0 && abs(c.y) < 1.0 {
                let slope = min(sqrt(max(1.0 - nl * nl, 0.0)) / nl, 4.0);
                let bias = (0.05 + texel * slope) * (0.5 / 256.0); // blocks -> depth: the box is 256 blocks deep, mapped to 0.5
                // MINUS: the sampler is LessEqual (lit when ref <= stored) and a point nearer the sun has the smaller depth, so the
                // bias has to pull the reference toward the sun. Added, it pushed every face into its own shadow while the 0.05 block
                // floor outweighed the normal offset (texel < ~0.12 block, i.e. within ~11 blocks of the map centre): sun-facing
                // walls went dark as you walked up to them, and flat ground was all acne.
                let p = vec3<f32>(c.xy / (length(c.xy) + u.a.x) * 0.5 + 0.5, c.z * 0.5 + 0.25 - bias);
                let q = vec2<f32>(p.x, 1.0 - p.y);
                // Lit only if neither the terrain map nor the entity map has something nearer the sun.
                sh = 1.0 - min(textureSampleCompareLevel(sh_tex, sh_samp, q, p.z), textureSampleCompareLevel(ent_tex, sh_samp, q, p.z));
            }
            // ponytail: a face lit at a grazing angle gets almost no light anyway, so fade from the map's answer to plain shadow like
            // N.L going to 0. Ceiling: the bias above covers texel quantisation, but `distort()` is applied per vertex, so across a
            // 1-block face the depth is interpolated in distorted screen space and errs by ~0.02 block x tan(angle) within ~2 blocks of
            // the map centre; below nl ~0.3 (sun < ~18 degrees over flat ground) that beats the bias. Upgrade = tessellate the shadow
            // pass finer near the centre; then this line can go.
            sh = mix(1.0, sh, smoothstep(0.05, 0.3, nl));
        }
    }
    return vec4<f32>(v, sh, 0.0, 1.0);
}

// composite1.glsl: 4-tap blur, square, colour, and the blend, written as `rgb + dst * (1 - a)`.
@fragment
fn fs_comp(in: FsIn) -> @location(0) vec4<f32> {
    let px = 1.0 / u.res.xy;
    let m = textureSampleLevel(vl_tex, vl_samp, in.uv + vec2<f32>(0.0, px.y), 0.0).rg
          + textureSampleLevel(vl_tex, vl_samp, in.uv - vec2<f32>(0.0, px.y), 0.0).rg
          + textureSampleLevel(vl_tex, vl_samp, in.uv + vec2<f32>(px.x, 0.0), 0.0).rg
          + textureSampleLevel(vl_tex, vl_samp, in.uv - vec2<f32>(px.x, 0.0), 0.0).rg;
    let vlp = (m.x * 0.25) * (m.x * 0.25);
    let dark = m.y * 0.25 * u.res.z; // blurred surface shadow x strength
    let dirw = normalize((u.cam * vec4<f32>(view_ray(in.uv), 0.0)).xyz);
    var nu = 1.0 - max(dirw.y, 0.0); // NdotU
    if nu > 0.5 { nu = smoothstep(0.0, 1.0, nu); }
    nu = nu * nu; // DIRECTION_LIGHTSHAFT 2
    nu = mix(nu, 1.0, u.b.z * u.b.z * 0.75);
    let vl = vlp * (nu * nu) * u.s.rgb;
    let k = (1.0 - u.b.x) * vlp * (1.0 - 0.5 * u.b.z) * u.b.y; // (1 - w) * vlMixBlend * mixedTime
    // Blend is `rgb + dst * (1 - a)`: the shafts' glow is unchanged, the frame under it is scaled by (1 - k) and by (1 - dark).
    return vec4<f32>(k * (vl / max(vlp, 0.01)) + u.b.x * u.s.w * vl, 1.0 - (1.0 - k) * (1.0 - dark));
}
"#;

struct Target {
    /// Frame size this was built for (the depth view in `march_bind` goes stale when it changes).
    size: (u32, u32),
    view: TextureView,
    march_bind: BindGroup,
    comp_bind: BindGroup,
}

pub struct LightShafts {
    march: RenderPipeline,
    comp: RenderPipeline,
    ubuf: Buffer,
    march_layout: BindGroupLayout,
    comp_layout: BindGroupLayout,
    samp: Sampler,
    target: Option<Target>,
}

impl LightShafts {
    pub fn new(device: &Device, surface_format: wgpu::TextureFormat, shadow_layout: &BindGroupLayout) -> Self {
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vl_uniform"),
            size: std::mem::size_of::<U>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty, count: None };
        let uniform = || wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None };
        let tex = |sample_type| wgpu::BindingType::Texture { sample_type, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false };
        let march_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("vl_march_layout"),
            entries: &[entry(0, uniform()), entry(1, tex(wgpu::TextureSampleType::Depth))],
        });
        let comp_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("vl_comp_layout"),
            entries: &[
                entry(0, uniform()),
                entry(2, tex(wgpu::TextureSampleType::Float { filterable: true })),
                entry(3, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
            ],
        });
        let samp = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("vl_shader"), source: wgpu::ShaderSource::Wgsl(SHADER.replace("@NB@", &format!("{NORMAL_BIAS:?}")).replace("@RES@", &format!("{:?}", SHADOW_RES as f32)).into()) });
        let pipeline = |label, layouts: &[&BindGroupLayout], fs: &str, format, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(label), bind_group_layouts: layouts, push_constant_ranges: &[] })),
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_full"), buffers: &[], compilation_options: wgpu::PipelineCompilationOptions::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        // `rgb + dst * (1 - a)`; the frame's alpha stays as it was.
        let over = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
        };
        let march = pipeline("vl_march", &[&march_layout, shadow_layout], "fs_march", wgpu::TextureFormat::Rgba8Unorm, None);
        let comp = pipeline("vl_comp", &[&comp_layout], "fs_comp", surface_format, Some(over));
        Self { march, comp, ubuf, march_layout, comp_layout, samp, target: None }
    }

    /// Run after the terrain pass: the half-resolution march. `shadow_vp` is what `ChunkPipeline::upload_uniforms` returned this
    /// frame (the shadow map has to exist: `Params::active`), `shadow_bind` its map. Then `composite` blends it onto the frame.
    pub fn march(&mut self, gpu: &Gpu, enc: &mut CommandEncoder, shadow_bind: &BindGroup, cam: &FirstPersonCamera, shadow_vp: Mat4, p: &Params) {
        let (w, h) = (gpu.config.width, gpu.config.height);
        if self.target.as_ref().map_or(true, |t| t.size != (w, h)) {
            // ponytail: half resolution, nearest depth tap (the pack marches at full resolution, 4-tap blurred); a
            // min-of-4 depth tap is the upgrade if shafts leak over thin silhouettes.
            let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("vl_target"),
                size: wgpu::Extent3d { width: (w / 2).max(1), height: (h / 2).max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            let march_bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("vl_march_bind"),
                layout: &self.march_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.ubuf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&gpu.depth_view) },
                ],
            });
            let comp_bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("vl_comp_bind"),
                layout: &self.comp_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.ubuf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.samp) },
                ],
            });
            self.target = Some(Target { size: (w, h), view, march_bind, comp_bind });
        }
        let (view, proj) = cam.build_view_proj();
        let u = U {
            cam: view.inverse().to_cols_array_2d(),
            shadow_cam: (shadow_vp * view.inverse()).to_cols_array_2d(),
            light: p.light.extend(DEPTH_BIAS).to_array(),
            ray: [1.0 / proj.x_axis.x, 1.0 / proj.y_axis.y, cam.znear, cam.zfar],
            a: [SHADOW_DISTORT, SHADOW_RADIUS, p.endurance, p.power],
            b: [p.w, p.mt, p.rain, (3.0 - proj.y_axis.y / 1.37).max(0.0)],
            // minDistFactor = 8 * clamp(far, 0, 512) / 192, then * 0.5 / 1.7; addition = 0.5 * 1.42857.
            c: [8.0 * cam.zfar.clamp(0.0, 512.0) / 192.0 * 0.5 / 1.7, 0.5 * 1.42857, 0.0, 0.0],
            s: [p.scale[0], p.scale[1], p.scale[2], p.lst],
            res: [w as f32, h as f32, p.shadow * SHADOW_DARK, 0.0],
        };
        gpu.queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&u));
        let t = self.target.as_ref().unwrap();
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("vl_march"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &t.view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rp.set_pipeline(&self.march);
            rp.set_bind_group(0, &t.march_bind, &[]);
            rp.set_bind_group(1, shadow_bind, &[]);
            rp.draw(0..3, 0..1);
        }
    }

    /// The blend onto the frame, drawn inside a pass that already loads it (the HUD's), so the frame is not loaded and stored
    /// a second time just for this. Only after `march` in the same frame.
    pub fn composite(&self, rp: &mut wgpu::RenderPass<'_>) {
        let Some(t) = &self.target else { return };
        rp.set_pipeline(&self.comp);
        rp.set_bind_group(0, &t.comp_bind, &[]);
        rp.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(t: f64, rain: f32) -> Params {
        params(t, sky::celestial_angle(t as u64, 0.0), rain, [0.47, 0.65, 1.0], 70.0)
    }
    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() <= 0.01 * b.abs().max(0.01)
    }

    /// Reference numbers: `composite1.glsl` / `lightColor.glsl` transcribed line by line (default settings, f64) for the same inputs.
    #[test]
    fn scalars_match_the_glsl() {
        let p = at(6000.0, 0.0); // noon
        assert!(near(p.scale[0], 10.0859) && near(p.scale[1], 7.6781) && near(p.scale[2], 5.7696), "{:?}", p.scale);
        assert!(near(p.lst, 1.0) && near(p.w, 1.0) && near(p.mt, 1.0) && near(p.endurance, 1.2) && near(p.power, 2.0));
        let p = at(0.0, 0.0); // sunrise: orange
        assert!(near(p.scale[0], 36.4737) && near(p.scale[1], 8.1925) && near(p.scale[2], 1.1167), "{:?}", p.scale);
        let p = at(12600.0, 0.0); // the shadow fade between sun and moon
        assert!(near(p.scale[0], 3.3336) && near(p.scale[1], 1.1155) && near(p.lst, 0.15172) && near(p.mt, 0.0294) && near(p.w, 0.9141), "{:?} {}", p.scale, p.lst);
        let p = at(18000.0, 1.0); // rainy midnight: the grey-blue weather colour, no additive branch
        assert!(near(p.scale[0], 1.1661) && near(p.scale[1], 2.2303) && near(p.scale[2], 5.2787), "{:?}", p.scale);
        assert!(p.w == 0.0 && near(p.endurance, 2.4) && near(p.power, 1.0));
    }

    /// The shadow sampler is LessEqual and a point nearer the sun has the smaller depth (`camera::shadow_matrix_follows_the_sun`),
    /// so the surface bias must be subtracted from the reference depth. Added, every face shadowed itself near the eye.
    #[test]
    fn surface_bias_pulls_the_reference_toward_the_sun() {
        assert!(SHADER.contains("c.z * 0.5 + 0.25 - bias") && !SHADER.contains("+ bias"));
        assert_eq!(SHADER.matches("ent_tex, sh_samp, q, p.z").count(), 2, "entities shade both the surface and the shaft march");
    }

    #[test]
    fn the_moon_takes_over_at_night() {
        let (day, night) = (at(6000.0, 0.0), at(18000.0, 0.0));
        assert!(day.active && day.light.y > 0.99, "sun at noon");
        assert!(night.active && night.light.y > 0.99, "moon at midnight is opposite the sun");
        // lightVec flips between timeAngle 0.5325 (tick 12780) and 0.9675 (tick 23220), the moment the pack swaps its shadow light.
        let sun = |t: f64| sky::sun_dir(sky::celestial_angle(t as u64, 0.0));
        assert_eq!(at(12700.0, 0.0).light, sun(12700.0));
        assert_eq!(at(12800.0, 0.0).light, -sun(12800.0));
        assert_eq!(at(23300.0, 0.0).light, sun(23300.0));
    }

    #[test]
    fn uniform_block_is_what_the_shader_declares() {
        assert_eq!(std::mem::size_of::<U>(), 2 * 64 + 7 * 16);
    }
}
