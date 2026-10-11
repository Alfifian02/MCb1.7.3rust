//! Shadows, a port of photon's `include/lighting/shadows/{common,distortion,pcss}.glsl` and the distortion half of
//! `program/shadow.vsh`, as WGSL text (`distortion_wgsl`, `pcss_wgsl`) plus the uniforms they read (`Shadows`).
//! photon "medium" profile: `SHADOW_PCF` + `SHADOW_VPS` (PCSS: blocker search, penumbra, 6..12 tap vogel-disc PCF), no `SHADOW_COLOR`,
//! no `SHADOW_SSRT`. Both are left for later milestones: coloured shadows need the translucent pass (M6, `shadowcolor0` + the
//! `shadowtex0`/`shadowtex1` split), SSRT needs the G-buffer (M1) and beyond the map `get_lightmap_shadows` is used, as in the pack
//! without `SHADOW_SSRT`. `PIXELATED_SHADOWS` and `WORLD_END` are not ported (overworld only).
//! Deviations, all because mc-rs has no such input:
//! - `noisetex.b` is `assets/noise_b.r8` (the pack's blue channel, `tools/gen_noise.py`), read through `.r`;
//! - `frameCounter` is 0 until there is a TAA to average the PCSS noise away (`TAA`), then it counts like Iris (wraps at 720720);
//! - the shadow map is 1024^2 over 64 blocks per side (the pack: 2048^2 over 128): the same 8 texels per block at the edge;
//! - the map has two depth textures, terrain and entities (`ent_tex`): `shadowtex0` is their `min`, `shadowtex1` compares both;
//! - the shadow pass keeps back-face culling (the pack says `shadow.culling = reversed`): the mesher drops hidden faces, so the
//!   sun-facing hull is the only thing in the map and reversed culling would erase it. The bias below has to cover self-shadowing;
//! - `inout` parameters of `get_filtered_shadows` are returned in `FilteredShadows`; `gl_FragCoord` is the `frag` argument.
//! The shaders include `distortion_wgsl()` (pure functions: `shadow.vsh`, the light-shaft march) and, on top of it,
//! `pcss_wgsl()` (reads `sh_tex`, `sh_samp`, `ent_tex` of group 1 -- declared by the including shader -- and group 2 of `Shadows`).

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use wgpu::util::DeviceExt;
use wgpu::{BindGroup, BindGroupLayout, Buffer, Device, Queue};

use crate::gpu::pipeline::SHADOW_RES;
use crate::render::camera::{shadow_model_view, shadow_projection, SHADOW_RADIUS, SHADOW_Z};

// settings.glsl
pub const SHADOW_DISTORTION: f32 = 0.85;
pub const SHADOW_DEPTH_SCALE: f32 = 0.2;
const SHADOW_PCF: bool = true;
const SHADOW_VPS: bool = true;
const SHADOW_PENUMBRA_SCALE: f32 = 1.0;
const SHADOW_PCF_STEPS_MIN: f32 = 3.0;
const SHADOW_PCF_STEPS_MAX: i32 = 6;
const SHADOW_PCF_STEPS_SCALE: f32 = 1.0;
const SHADOW_BLOCKER_SEARCH_RADIUS: f32 = 0.5;
const SSS_STEPS: i32 = 12;
const MC_SHADOW_QUALITY: f32 = 1.0;
/// `#define TAA`: off until M5. With it off the dither is the blue-noise texture alone (static), the 4-tap blur of `render::vl`
/// takes the grain out; with it on `r1(frameCounter, dither)` moves it every frame for the TAA to average.
const TAA: bool = false;

/// One block of depth in the shadow map's stored depth: the box is `2 * SHADOW_Z` blocks deep and spans `SHADOW_DEPTH_SCALE` of
/// the 0..1 range (`z_gl * SCALE * 0.5 + 0.5`, z_gl over 2).
pub const BLOCK_DEPTH: f32 = SHADOW_DEPTH_SCALE / (2.0 * SHADOW_Z);

const NOISE: &[u8] = include_bytes!("../../assets/noise_b.r8");

fn f(x: f32) -> String {
    format!("{x:?}")
}

const DISTORTION: &str = r#"
// ---- distortion.glsl ----
fn sqr(x: f32) -> f32 { return x * x; }
fn pow4(x: f32) -> f32 { return sqr(sqr(x)); }
fn clamp01(x: f32) -> f32 { return clamp(x, 0.0, 1.0); }

const SHADOW_DISTORTION: f32 = @SHADOW_DISTORTION@;
const SHADOW_DEPTH_SCALE: f32 = @SHADOW_DEPTH_SCALE@;

// Euclidian distance is sqrt(a^2 + b^2 + ...); this is quartic_root(a^4 + b^4 + ...): smaller along the diagonals.
fn quartic_length(v: vec2<f32>) -> f32 { return sqrt(sqrt(pow4(v.x) + pow4(v.y))); }

fn get_distortion_factor(shadow_clip_pos: vec2<f32>) -> f32 {
    return quartic_length(shadow_clip_pos) * SHADOW_DISTORTION + (1.0 - SHADOW_DISTORTION);
}

// WGSL has no overloads: `_with` is the overload that takes the factor.
fn distort_shadow_space_with(shadow_clip_pos: vec3<f32>, distortion_factor: f32) -> vec3<f32> {
    return shadow_clip_pos * vec3<f32>(vec2<f32>(1.0 / distortion_factor), SHADOW_DEPTH_SCALE);
}

fn distort_shadow_space(shadow_clip_pos: vec3<f32>) -> vec3<f32> {
    return distort_shadow_space_with(shadow_clip_pos, get_distortion_factor(shadow_clip_pos.xy));
}

fn undistort_shadow_space(shadow_clip_pos: vec3<f32>) -> vec3<f32> {
    let xy = shadow_clip_pos.xy * ((1.0 - SHADOW_DISTORTION) / (1.0 - quartic_length(shadow_clip_pos.xy)));
    return vec3<f32>(xy, shadow_clip_pos.z / SHADOW_DEPTH_SCALE);
}

// Shadow bias method from Complementary Reimagined by Emin (as in photon): a normal offset without peter-panning.
// The pack's `skylight` argument is only used by its End branch.
fn get_shadow_bias(scene_pos: vec3<f32>, normal: vec3<f32>, NoL: f32, skylight: f32) -> vec3<f32> {
    return 0.25 * normal * clamp01(0.12 + 0.01 * length(scene_pos)) * (2.0 - clamp01(NoL));
}
"#;

const PCSS: &str = r#"
// ---- pcss.glsl + common.glsl ----
const TAU: f32 = 6.28318530718;
const EPS: f32 = 1e-6;
const PHI1: f32 = 1.6180339887;
const SHADOW_PCF: bool = @SHADOW_PCF@;
const SHADOW_VPS: bool = @SHADOW_VPS@;
const SHADOW_PENUMBRA_SCALE: f32 = @SHADOW_PENUMBRA_SCALE@;
const SHADOW_PCF_STEPS_MIN: f32 = @SHADOW_PCF_STEPS_MIN@;
const SHADOW_PCF_STEPS_MAX: i32 = @SHADOW_PCF_STEPS_MAX@;
const SHADOW_PCF_STEPS_SCALE: f32 = @SHADOW_PCF_STEPS_SCALE@;
const SHADOW_BLOCKER_SEARCH_RADIUS: f32 = @SHADOW_BLOCKER_SEARCH_RADIUS@;
const SSS_STEPS: i32 = @SSS_STEPS@;
const SHADOW_MAP_RES: i32 = @SHADOW_MAP_RES@; // int(float(shadowMapResolution) * MC_SHADOW_QUALITY)
const SHADOW_MAP_PIXEL_SIZE: f32 = @SHADOW_MAP_PIXEL_SIZE@;

struct ShadowU {
    mv: mat4x4<f32>,    // shadowModelView: scene space (world - eye) to the sun's view space
    proj: mat4x4<f32>,  // shadowProjection, GL depth range
    light: vec4<f32>,   // xyz: light_dir (toward the light the map was drawn with)
    cam: vec4<f32>,     // xyz: cameraPosition, w: frameCounter
    p: vec4<f32>,       // x: min(shadowDistance, far), y: -shadowProjectionInverse[2].z
};
@group(2) @binding(0) var<uniform> shu: ShadowU;
@group(2) @binding(1) var noisetex: texture_2d<f32>; // R8: the pack's `.b` is `.r` here

fn rcp(x: f32) -> f32 { return 1.0 / x; }
fn max0(x: f32) -> f32 { return max(x, 0.0); }
fn linear_step(edge0: f32, edge1: f32, x: f32) -> f32 { return clamp01((x - edge0) / (edge1 - edge0)); }
fn pow32(x_in: f32) -> f32 {
    var x = x_in;
    for (var i = 0; i < 5; i = i + 1) { x = x * x; }
    return x;
}
fn max_of2(v: vec2<f32>) -> f32 { return max(v.x, v.y); }
fn length_squared3(v: vec3<f32>) -> f32 { return dot(v, v); }
fn transform(m: mat4x4<f32>, pos: vec3<f32>) -> vec3<f32> { return (m * vec4<f32>(pos, 1.0)).xyz; }
fn project_ortho(m: mat4x4<f32>, pos: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(m[0].x, m[1].y, m[2].z) * pos + m[3].xyz;
}

fn r1(n: i32, seed: f32) -> f32 { return fract(seed + f32(n) * (1.0 / PHI1)); }

fn vogel_disc_sample(step_index: i32, step_count: i32, rotation: f32) -> vec2<f32> {
    let golden_angle = 2.4;
    let r = sqrt(f32(step_index) + 0.5) / sqrt(f32(step_count));
    let theta = f32(step_index) * golden_angle + rotation;
    return r * vec2<f32>(cos(theta), sin(theta));
}

// GL shadow coordinates (y up) to texture coordinates (y down).
fn sm_uv(uv: vec2<f32>) -> vec2<f32> { return vec2<f32>(uv.x, 1.0 - uv.y); }

// texelFetch(shadowtex0, texel, 0).x: terrain and entities, whichever is nearer the light.
fn shadow_depth0(texel: vec2<i32>) -> f32 {
    let t = clamp(texel, vec2<i32>(0), vec2<i32>(SHADOW_MAP_RES - 1));
    return min(textureLoad(sh_tex, t, 0), textureLoad(ent_tex, t, 0));
}

// texture(shadowtex1, vec3(uv, z)): hardware-filtered compare (sh_samp is linear), lit only if neither map has a nearer occluder.
fn shadow_compare1(uv: vec2<f32>, z: f32) -> f32 {
    let q = sm_uv(uv);
    return min(textureSampleCompareLevel(sh_tex, sh_samp, q, z), textureSampleCompareLevel(ent_tex, sh_samp, q, z));
}

// Fade from close shadows (shadow maps) to distant shadows (lightmap or SSRT).
fn get_shadow_distance_fade(scene_pos: vec3<f32>, shadow_screen_pos: vec3<f32>) -> f32 {
    let effective_shadow_distance = shu.p.x;
    return linear_step(
        0.1,
        1.0,
        pow32(max(max_of2(abs(shadow_screen_pos.xy * 2.0 - 1.0)),
                  length_squared3(scene_pos) * rcp(sqr(effective_shadow_distance))))
    );
}

// Fake, lightmap-based shadows for outside of the shadow range or when shadows are disabled.
fn get_lightmap_shadows(skylight: f32) -> f32 {
    return smoothstep(13.5 / 15.0, 14.5 / 15.0, skylight);
}

// Prevents light leaking in caves by cancelling out sunlight in very low skylight.
fn get_lightmap_light_leak_prevention(skylight: f32) -> f32 {
    return smoothstep(0.0 / 15.0, 2.0 / 15.0, skylight);
}

fn blocker_search(scene_pos: vec3<f32>, dither: f32, has_sss: bool) -> vec2<f32> {
    var step_count = 3;
    if has_sss { step_count = SSS_STEPS; }

    let shadow_view_pos = transform(shu.mv, scene_pos);
    let shadow_clip_pos = project_ortho(shu.proj, shadow_view_pos);
    let ref_z = shadow_clip_pos.z * (SHADOW_DEPTH_SCALE * 0.5) + 0.5;

    let radius = SHADOW_BLOCKER_SEARCH_RADIUS * shu.proj[0].x * (0.5 + 0.5 * linear_step(0.2, 0.4, shu.light.y));

    var depth_sum = 0.0;
    var weight_sum = 0.0;
    var depth_sum_sss = 0.0;

    for (var i = 0; i < step_count; i = i + 1) {
        let offset = vogel_disc_sample(i, step_count, dither * TAU) * radius;
        var uv = shadow_clip_pos.xy + offset;
        uv = uv / get_distortion_factor(uv);
        uv = uv * 0.5 + 0.5;

        let depth = shadow_depth0(vec2<i32>(sm_uv(uv) * f32(SHADOW_MAP_RES)));
        let weight = step(depth, ref_z);

        depth_sum += weight * depth;
        weight_sum += weight;
        depth_sum_sss += max0(ref_z - depth);
    }

    var blocker_depth = 0.0;
    if weight_sum != 0.0 { blocker_depth = depth_sum / weight_sum; }
    let sss_depth = shu.p.y * depth_sum_sss * rcp(SHADOW_DEPTH_SCALE * f32(step_count));

    return vec2<f32>(blocker_depth, sss_depth);
}

fn shadow_basic(shadow_screen_pos: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(shadow_compare1(shadow_screen_pos.xy, shadow_screen_pos.z));
}

fn shadow_pcf(shadow_screen_pos: vec3<f32>, shadow_clip_pos: vec3<f32>, penumbra_size: f32, dither: f32) -> vec3<f32> {
    // penumbra_size > max_filter_radius: blur; penumbra_size < min_filter_radius: anti-alias (blur then sharpen)
    let distortion_factor = get_distortion_factor(shadow_clip_pos.xy);
    let min_filter_radius = 2.0 * SHADOW_MAP_PIXEL_SIZE * distortion_factor;

    let filter_radius = max(penumbra_size, min_filter_radius);
    let filter_scale = sqr(filter_radius / min_filter_radius);

    // The float is capped before the cast: it is only ever compared with SHADOW_PCF_STEPS_MAX.
    var step_count = i32(min(SHADOW_PCF_STEPS_MIN + SHADOW_PCF_STEPS_SCALE * filter_scale, 64.0));
    step_count = min(step_count, SHADOW_PCF_STEPS_MAX);

    var shadow = 0.0;
    // no SHADOW_COLOR: the colour is white
    let color = vec3<f32>(1.0);

    // perform first 4 iterations
    for (var i = 0; i < 4; i = i + 1) {
        let offset = vogel_disc_sample(i, step_count, dither * TAU) * filter_radius;
        var uv = shadow_clip_pos.xy + offset;
        uv = uv / get_distortion_factor(uv);
        uv = uv * 0.5 + 0.5;
        shadow += shadow_compare1(uv, shadow_screen_pos.z);
    }

    // exit early if outside shadow
    if shadow > 4.0 - EPS {
        return color;
    } else if shadow < EPS {
        return vec3<f32>(0.0);
    }

    // perform remaining iterations
    for (var i = 4; i < step_count; i = i + 1) {
        let offset = vogel_disc_sample(i, step_count, dither * TAU) * filter_radius;
        var uv = shadow_clip_pos.xy + offset;
        uv = uv / get_distortion_factor(uv);
        uv = uv * 0.5 + 0.5;
        shadow += shadow_compare1(uv, shadow_screen_pos.z);
    }

    let rcp_steps = rcp(f32(step_count));

    // sharpening for small penumbra sizes
    let sharpening_threshold = 0.4 * max0((min_filter_radius - penumbra_size) / min_filter_radius);
    shadow = linear_step(sharpening_threshold, 1.0 - sharpening_threshold, shadow * rcp_steps);

    return shadow * color;
}

struct FilteredShadows {
    shadow: vec3<f32>,
    distance_fade: f32, // inout in the pack: 1 = fully outside the shadow range, `shadow` is then not meaningful
    sss_depth: f32,     // inout in the pack
};

// `scene_pos`: world minus the eye. `frag`: gl_FragCoord.xy. `cloud_shadows`: 1 = clear (M4 brings the real term).
fn get_filtered_shadows(scene_pos: vec3<f32>, flat_normal: vec3<f32>, skylight: f32, cloud_shadows: f32, sss_amount: f32, frag: vec2<i32>) -> FilteredShadows {
    var res: FilteredShadows;
    res.shadow = vec3<f32>(0.0);
    res.distance_fade = 1.0;
    res.sss_depth = 0.0;

    let NoL = dot(flat_normal, shu.light.xyz);

    let bias = get_shadow_bias(scene_pos, flat_normal, NoL, skylight);

    // Light leaking prevention from Complementary Reimagined, used with permission
    var edge_factor = 0.1 - 0.2 * fract(scene_pos + shu.cam.xyz + flat_normal * 0.01);
    edge_factor = edge_factor - edge_factor * skylight;

    let shadow_view_pos = transform(shu.mv, scene_pos + bias + edge_factor);
    let shadow_clip_pos = project_ortho(shu.proj, shadow_view_pos);
    let shadow_screen_pos = distort_shadow_space(shadow_clip_pos) * 0.5 + 0.5;

    res.distance_fade = get_shadow_distance_fade(scene_pos, shadow_screen_pos);

    if res.distance_fade >= 1.0 {
        res.shadow = vec3<f32>(1.0);
        return res;
    }

    var dither = textureLoad(noisetex, frag & vec2<i32>(511), 0).r;
    dither = r1(i32(shu.cam.w), dither);

    var penumbra_size = 0.0;
    if SHADOW_VPS {
        let blocker_search_result = blocker_search(scene_pos, dither, sss_amount > EPS);

        res.sss_depth = blocker_search_result.y;

        if NoL < 1e-3 {
            return res; // now we can exit early for SSS blocks (shadow is 0)
        }
        if blocker_search_result.x < EPS {
            res.shadow = vec3<f32>(1.0); // blocker search empty handed => no occluders
            return res;
        }

        penumbra_size = 16.0 * SHADOW_PENUMBRA_SCALE * (shadow_screen_pos.z - blocker_search_result.x) / blocker_search_result.x;
        penumbra_size = penumbra_size * (5.0 - 4.0 * cloud_shadows); // Increase penumbra radius inside cloud shadows
        penumbra_size = min(penumbra_size, SHADOW_BLOCKER_SEARCH_RADIUS);
        penumbra_size = penumbra_size * shu.proj[0].x;
    } else {
        penumbra_size = sqrt(0.5) * SHADOW_MAP_PIXEL_SIZE * SHADOW_PENUMBRA_SCALE;
        // Increase blur radius to approximate subsurface scattering
        penumbra_size = penumbra_size * (1.0 + 7.0 * sss_amount);
    }

    if SHADOW_PCF {
        res.shadow = shadow_pcf(shadow_screen_pos, shadow_clip_pos, penumbra_size, dither);
    } else {
        res.shadow = shadow_basic(shadow_screen_pos);
    }
    return res;
}
"#;

/// Pure shadow-space functions (`distortion.glsl`); `shadow.vsh` and the light-shaft march include only this.
pub fn distortion_wgsl() -> String {
    DISTORTION.replace("@SHADOW_DISTORTION@", &f(SHADOW_DISTORTION)).replace("@SHADOW_DEPTH_SCALE@", &f(SHADOW_DEPTH_SCALE))
}

/// `pcss.glsl` + `common.glsl`, including its own group 2. Needs `distortion_wgsl()` before it and `sh_tex`, `sh_samp`, `ent_tex`.
pub fn pcss_wgsl() -> String {
    let res = (SHADOW_RES as f32 * MC_SHADOW_QUALITY) as i32;
    PCSS.replace("@SHADOW_PCF@", &SHADOW_PCF.to_string())
        .replace("@SHADOW_VPS@", &SHADOW_VPS.to_string())
        .replace("@SHADOW_PENUMBRA_SCALE@", &f(SHADOW_PENUMBRA_SCALE))
        .replace("@SHADOW_PCF_STEPS_MIN@", &f(SHADOW_PCF_STEPS_MIN))
        .replace("@SHADOW_PCF_STEPS_MAX@", &SHADOW_PCF_STEPS_MAX.to_string())
        .replace("@SHADOW_PCF_STEPS_SCALE@", &f(SHADOW_PCF_STEPS_SCALE))
        .replace("@SHADOW_BLOCKER_SEARCH_RADIUS@", &f(SHADOW_BLOCKER_SEARCH_RADIUS))
        .replace("@SSS_STEPS@", &SSS_STEPS.to_string())
        .replace("@SHADOW_MAP_RES@", &res.to_string())
        .replace("@SHADOW_MAP_PIXEL_SIZE@", &f(1.0 / res as f32))
}

/// `ShadowU` of `pcss_wgsl`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct Params {
    mv: [[f32; 4]; 4],
    proj: [[f32; 4]; 4],
    light: [f32; 4],
    cam: [f32; 4],
    p: [f32; 4],
}

/// Group 2 of every shader that includes `pcss_wgsl`: the sun camera, the light, the eye and the blue-noise texture.
pub struct Shadows {
    ubuf: Buffer,
    pub layout: BindGroupLayout,
    pub bind: BindGroup,
}

impl Shadows {
    pub fn new(device: &Device, queue: &Queue) -> Self {
        let noise = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("noisetex"),
                size: wgpu::Extent3d { width: 512, height: 512, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            NOISE,
        );
        let noise_view = noise.create_view(&wgpu::TextureViewDescriptor::default());
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shadow_params"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow_params_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow_params_bind"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&noise_view) },
            ],
        });
        Self { ubuf, layout, bind }
    }

    /// Per frame, with what the shadow map was drawn with (`ShadowCache::center` / `light`): `eye` is the camera, `far` its far plane,
    /// `frame` the frame count.
    pub fn update(&self, queue: &Queue, center: Vec3, light: Vec3, eye: Vec3, far: f32, frame: u64) {
        let p = Params {
            mv: shadow_model_view(center, light, eye).to_cols_array_2d(),
            proj: shadow_projection().to_cols_array_2d(),
            light: light.extend(0.0).to_array(),
            cam: eye.extend(if TAA { (frame % 720_720) as f32 } else { 0.0 }).to_array(),
            p: [SHADOW_RADIUS.min(far), SHADOW_Z, 0.0, 0.0],
        };
        queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&p));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The GLSL of distortion.glsl / sampling.glsl / random.glsl / common.glsl, line for line, in f64.
    fn quartic_length(x: f64, y: f64) -> f64 {
        (x.powi(4) + y.powi(4)).sqrt().sqrt()
    }
    fn factor(x: f64, y: f64) -> f64 {
        quartic_length(x, y) * SHADOW_DISTORTION as f64 + (1.0 - SHADOW_DISTORTION as f64)
    }
    fn undistort(x: f64, y: f64) -> (f64, f64) {
        let k = (1.0 - SHADOW_DISTORTION as f64) / (1.0 - quartic_length(x, y));
        (x * k, y * k)
    }
    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6 * b.abs().max(1.0)
    }

    /// Reference numbers by hand from the GLSL: factor(0.5, 0) = 0.5 * 0.85 + 0.15, a corner is 2^(1/4) long, the bias of
    /// `get_shadow_bias`, `vogel_disc_sample(0, 12, 0)`, `r1(1, 0.25)` and `get_lightmap_shadows(14 / 15)` (smoothstep midpoint).
    #[test]
    fn distortion_matches_the_glsl() {
        assert!(near(factor(0.5, 0.0), 0.575) && near(0.5 / factor(0.5, 0.0), 0.869565217391));
        assert!(near(quartic_length(1.0, 1.0), 2f64.powf(0.25)) && near(factor(1.0, 1.0), 1.160826));
        assert!(near(factor(1.0, 0.0), 1.0), "the box edge does not move");
        // undistort_shadow_space inverts distort_shadow_space on x (the pack's own pair)
        let (x, y) = (0.6, -0.3);
        let (dx, dy) = (x / factor(x, y), y / factor(x, y));
        let (ux, uy) = undistort(dx, dy);
        assert!(near(ux, x) && near(uy, y), "{ux} {uy}");
        // 0.25 * n * clamp(0.12 + 0.01 * 10) * (2 - 0.5)
        assert!(near(0.25 * (0.12 + 0.01 * 10.0) * 1.5, 0.0825));
        assert!(near((0.5f64).sqrt() / 12f64.sqrt(), 0.2041241452));
        assert!(near((0.25 + 1.0 / 1.6180339887f64).fract(), 0.8680339887));
        let t = (14.0 / 15.0 - 13.5 / 15.0) / (1.0 / 15.0);
        assert!(near(t * t * (3.0 - 2.0 * t), 0.5));
    }

    /// The WGSL has no placeholder left, the uniform block is what `ShadowU` declares (2 mat4 + 3 vec4), the noise is 512^2 R8, and
    /// the pieces the shaders call by name exist.
    #[test]
    fn wgsl_is_complete() {
        let (d, p) = (distortion_wgsl(), pcss_wgsl());
        assert!(!d.contains('@') && !p.contains('@'), "unreplaced placeholder");
        assert!(d.contains("const SHADOW_DISTORTION: f32 = 0.85;") && d.contains("const SHADOW_DEPTH_SCALE: f32 = 0.2;"));
        assert!(p.contains("const SHADOW_MAP_RES: i32 = 1024;") && p.contains("fn get_filtered_shadows("));
        assert_eq!(std::mem::size_of::<Params>(), 2 * 64 + 3 * 16);
        assert_eq!(NOISE.len(), 512 * 512);
        assert!(BLOCK_DEPTH > 7.8e-4 && BLOCK_DEPTH < 7.9e-4);
    }
}
