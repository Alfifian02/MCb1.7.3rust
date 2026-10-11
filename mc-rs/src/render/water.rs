//! Milestone 6, part 2: water and the translucent layer. Port of Photon's `gbuffers_all_translucent.fsh/.vsh` (water path),
//! `include/surface/water_normal.glsl` (Gerstner waves, height variation, parallax), `include/vertex/displacement.glsl`
//! (`get_water_displacement`), `water_fog_simple` / `biome_water_coeff` of `simple_fog.glsl`, the GGX specular highlight of
//! `specular_lighting.glsl` (+ `bsdf.glsl`), the diffuse term of `diffuse_lighting.glsl`, and the translucent-layer half of
//! `c1_blend_layers.fsh` (refraction + `scene * (1 - a) + rgb`).
//!
//! Flow (the pack's): `begin` draws the water into the translucent layer (premultiplied rgb + a, `colortex13`) and the refraction
//! normal (`colortex3`) with its own depth (`depthtex0`), discarding what is behind the opaque depth (`depthtex1`); weather draws
//! into the same pass; `composite` then writes `scene` blended with the layer, refracted, into the target.
//! ```text
//! water.update(&queue, &cam, w, h, &frame);
//! water.prepare(&device, &opaque_depth_view, w, h);       // opaque depth must be stored by the terrain pass
//! let mut rp = water.begin(&mut enc);
//! water.setup(&mut rp);                                   // pipeline + group 1
//! rp.set_bind_group(0, &pipe.bind_group, &[]);            // the chunk pipeline's group 0
//! chunks.draw_ranges(.., true /* water */, ..);
//! weather.draw(&mut rp);
//! drop(rp);
//! water.composite(&device, &mut enc, &scene_view, &dst_view, dst_format);
//! ```
//! Differences from the pack, each because the input does not exist yet:
//! - no sky / environment reflections (`SKY_REFLECTIONS`, `ENVIRONMENT_REFLECTIONS`): they need milestone 2's sky map and milestone 5's
//!   scene history; the specular highlight of the sun is in;
//! - `shadows` is the pack's no-shadow branch `pow8(skylight)` until milestone 3's PCSS exists; no cloud shadows (milestone 4);
//! - `light_color` / `ambient_color` are uniforms (`Frame`) until milestone 2's sky lookup table provides them;
//! - the mesh has one light value (sky and block merged): `light_levels = (0, light)`;
//! - no biome water colour in b1.7.3 water: `biome_water_coeff` returns the base absorption;
//! - `common_fog` is the terrain fog already in the chunk shader (milestone 4 replaces both), no `air_fog_analytic` behind water;
//! - no Purkinje shift (needs the Rec.2020 working space of milestone 5), no `SNELLS_WINDOW` (off in the pack), no nether portal;
//! - the working colour space is Rec.709 (`rec709_to_working_color` is the identity);
//! - two water layers in one pixel: the nearer wins by depth (the layer has its own depth), nothing is sorted.
//! UNVERIFIED: written WITHOUT a Rust toolchain, never compiled or run.

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use wgpu::{BindGroup, BindGroupLayout, Buffer, CommandEncoder, Device, Queue, RenderPass, RenderPipeline, Sampler, TextureView};

use crate::gpu::pipeline::Vertex;
use crate::render::camera::FirstPersonCamera;

pub const TRANSLUCENT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub const REFRACTION_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Per-frame inputs that the pack reads from its sky lookup table and uniforms.
pub struct Frame {
    /// Toward the light that is up (sun, or moon when `moon`).
    pub light_dir: Vec3,
    pub moon: bool,
    pub light_color: Vec3,
    pub ambient_color: Vec3,
    /// `frameTimeCounter`, seconds.
    pub time: f32,
    pub rain: f32,
    pub eye_in_water: bool,
    pub eye_skylight: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct W {
    cam: [[f32; 4]; 4],
    /// 1 / proj[0][0], 1 / proj[1][1], near, far.
    ray: [f32; 4],
    /// width, height, 1 / width, 1 / height.
    res: [f32; 4],
    /// xyz toward the light, w 1 when it is the moon.
    light_dir: [f32; 4],
    /// rgb light colour, w time.
    light_color: [f32; 4],
    /// rgb ambient colour, w rain.
    ambient: [f32; 4],
    /// x isEyeInWater, y eye_skylight.
    misc: [f32; 4],
}

const COMMON: &str = r#"
const PI: f32 = 3.14159265359;
const TAU: f32 = 6.28318530718;
struct W {
    cam: mat4x4<f32>,
    ray: vec4<f32>,
    res: vec4<f32>,
    light_dir: vec4<f32>,
    light_color: vec4<f32>,
    ambient: vec4<f32>,
    misc: vec4<f32>,
};
fn clamp01(x: f32) -> f32 { return clamp(x, 0.0, 1.0); }
fn sqr(x: f32) -> f32 { return x * x; }
fn cube(x: f32) -> f32 { return x * x * x; }
fn linear_step(a: f32, b: f32, x: f32) -> f32 { return clamp01((x - a) / (b - a)); }
fn dampen(x: f32) -> f32 { let c = clamp01(x); return c * (2.0 - c); }
fn lift(x: f32, amount: f32) -> f32 { return (x + x * amount) / (1.0 + x * amount); }
"#;

const WATER: &str = r#"
const WATER_LIGHT: f32 = $WL$;
const FOG_DENSITY: f32 = 2.0;
struct Uniforms {
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    shadow_vp: mat4x4<f32>,
    sun: vec4<f32>,
    eye: vec4<f32>,
    sh: vec4<f32>,
    plant: array<vec4<u32>, 2>,
    fog: vec4<f32>,
    fogp: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var tex: texture_2d<f32>;
@group(1) @binding(0) var<uniform> w: W;
@group(1) @binding(1) var depth1: texture_depth_2d;    // depthtex1: opaque depth
@group(1) @binding(2) var noisetex: texture_2d<f32>;
@group(1) @binding(3) var nsamp: sampler;              // linear, repeat

// ---- settings.glsl defaults ----
const WATER_WAVE_ITERATIONS: i32 = 3;
const WATER_WAVE_STRENGTH: f32 = 1.0;
const WATER_WAVE_FREQUENCY: f32 = 1.0;
const WATER_WAVE_SPEED_STILL: f32 = 1.0;
const WATER_WAVE_SPEED_FLOWING: f32 = 1.0;
const WATER_WAVE_PERSISTENCE: f32 = 1.0;
const WATER_WAVE_LACUNARITY: f32 = 1.0;
const WATER_EDGE_HIGHLIGHT_INTENSITY: f32 = 1.0;
const ABSORPTION: vec3<f32> = vec3<f32>(0.39, 0.14, 0.07);
const SCATTERING: f32 = 0.01;
const GOLDEN_ANGLE: f32 = 2.39996323;
const DEGREE: f32 = 0.01745329252;
const SUN_ANGULAR_RADIUS: f32 = 2.0;
const MOON_ANGULAR_RADIUS: f32 = 3.0;
const WATER_F0: f32 = 0.02;
const WATER_ROUGHNESS: f32 = 0.002;

// ---- vertex: displacement.glsl get_water_displacement (note: the pack feeds it world_pos.xy) ----
fn gerstner_wave(coord: vec2<f32>, wave_dir: vec2<f32>, t: f32, noise: f32, wavelength: f32) -> f32 {
    let g = 9.8;
    let k = TAU / wavelength;
    let wv = sqrt(g * k);
    let x = wv * t - k * (dot(wave_dir, coord) + noise);
    return sqr(sin(x) * 0.5 + 0.5);
}

fn water_displacement(wp: vec3<f32>, skylight: f32) -> f32 {
    let wave_frequency = 0.3 * WATER_WAVE_FREQUENCY;
    let wave_speed = 0.37 * WATER_WAVE_SPEED_STILL;
    let wave_angle = 30.0 * DEGREE;
    let wave_dir = vec2<f32>(cos(wave_angle), sin(wave_angle));
    var wave = gerstner_wave(wp.xy * wave_frequency, wave_dir, w.light_color.w * wave_speed, 0.0, 1.0);
    wave = (wave * 0.05 - 0.025) * (skylight * 0.9 + 0.1);
    return wave;
}

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) light: f32,
    @location(2) wp: vec3<f32>,
};

@vertex
fn vs_water(@location(0) vpos: vec3<f32>, @location(1) vuv: vec2<f32>, @location(2) vlight: f32) -> VsOut {
    // `mesh::build_split` packs `light * WATER_LIGHT + column depth`; the depth is measured per pixel here instead.
    let d = floor(vlight + 0.05);
    let light = (vlight - d) / WATER_LIGHT;
    var p = vpos;
    p.y = p.y + water_displacement(p, light);
    var o: VsOut;
    o.pos = u.proj * u.view * vec4<f32>(p, 1.0);
    o.uv = vuv;
    o.light = light;
    o.wp = p;
    return o;
}

// ---- water_normal.glsl ----
fn get_water_height(coord: vec2<f32>, wave_dir_in: vec2<f32>, wave_rot: mat2x2<f32>, t: f32) -> f32 {
    let wave_frequency = 0.7 * WATER_WAVE_FREQUENCY;
    let persistence = 0.5 * WATER_WAVE_PERSISTENCE;
    let lacunarity = 1.7 * WATER_WAVE_LACUNARITY;
    let noise_frequency = 0.007;
    let noise_strength = 2.0;
    let height_variation_frequency = 0.001;
    let min_height = 0.4;
    let height_variation_scale = 2.0;
    let height_variation_offset = -0.5;
    let height_variation_scroll_speed = 0.1;
    let amplitude_normalization_factor = (1.0 - persistence) / (1.0 - pow(persistence, f32(WATER_WAVE_ITERATIONS)));

    var wave_noise: array<f32, 3>;
    var noise_coord = (coord + vec2<f32>(0.0, 0.25 * t)) * noise_frequency;
    for (var i = 0; i < WATER_WAVE_ITERATIONS; i = i + 1) {
        wave_noise[i] = textureSampleLevel(noisetex, nsamp, noise_coord, 0.0).y;
        noise_coord = noise_coord * 2.5;
    }
    let height_variation_noise = textureSampleLevel(noisetex, nsamp, (coord + vec2<f32>(0.0, height_variation_scroll_speed * t)) * height_variation_frequency, 0.0).y;

    var height = 0.0;
    var wave_length = 1.0;
    var amplitude = 1.0;
    var frequency = wave_frequency;
    var wave_dir = wave_dir_in;
    for (var i = 0; i < WATER_WAVE_ITERATIONS; i = i + 1) {
        height = height + gerstner_wave(coord * frequency, wave_dir, t, wave_noise[i] * noise_strength, wave_length) * amplitude;
        amplitude = amplitude * persistence;
        frequency = frequency * lacunarity;
        wave_length = wave_length * 1.5;
        wave_dir = wave_dir * wave_rot;
    }
    height = height * max(min_height, height_variation_noise * height_variation_scale + height_variation_offset);
    return height * amplitude_normalization_factor;
}

struct Waves { dir: vec2<f32>, rot: mat2x2<f32>, t: f32 };

fn waves_setup(flowing: bool, flow_dir: vec2<f32>) -> Waves {
    let speed_still = 0.5 * WATER_WAVE_SPEED_STILL;
    let speed_flowing = 0.7 * WATER_WAVE_SPEED_FLOWING;
    let angle = 30.0 * DEGREE;
    var o: Waves;
    o.t = select(speed_still, speed_flowing, flowing) * w.light_color.w;
    o.dir = select(vec2<f32>(cos(angle), sin(angle)), flow_dir, flowing);
    o.rot = mat2x2<f32>(vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0));
    if !flowing {
        o.rot = mat2x2<f32>(vec2<f32>(cos(GOLDEN_ANGLE), sin(GOLDEN_ANGLE)), vec2<f32>(-sin(GOLDEN_ANGLE), cos(GOLDEN_ANGLE)));
    }
    return o;
}

fn get_water_normal(world_pos: vec3<f32>, flat_normal: vec3<f32>, coord: vec2<f32>, flow_dir: vec2<f32>, skylight: f32, flowing: bool) -> vec3<f32> {
    let wv = waves_setup(flowing, flow_dir);
    let h = 0.1;
    let wave0 = get_water_height(coord, wv.dir, wv.rot, wv.t);
    let wave1 = get_water_height(coord + vec2<f32>(h, 0.0), wv.dir, wv.rot, wv.t);
    let wave2 = get_water_height(coord + vec2<f32>(0.0, h), wv.dir, wv.rot, wv.t);
    var normal_influence = select(mix(0.01, 0.04 + 0.15 * w.ambient.w, dampen(skylight)), 0.1, flowing);
    normal_influence = normal_influence * smoothstep(0.0, 0.15, abs(dot(flat_normal, normalize(world_pos - u.eye.xyz))));
    normal_influence = normal_influence * WATER_WAVE_STRENGTH;
    var normal = vec3<f32>(wave1 - wave0, wave2 - wave0, h);
    normal = vec3<f32>(normal.xy * normal_influence, normal.z);
    return normalize(normal);
}

fn get_water_parallax_coord(tangent_dir: vec3<f32>, coord_in: vec2<f32>, flow_dir: vec2<f32>, flowing: bool) -> vec2<f32> {
    let step_count = 4.0;
    let parallax_depth = 0.2;
    let wv = waves_setup(flowing, flow_dir);
    let ray_step = tangent_dir.xy / (-tangent_dir.z) * parallax_depth / step_count;
    var coord = coord_in;
    var depth_value = get_water_height(coord, wv.dir, wv.rot, wv.t);
    var depth_march = 0.0;
    var depth_previous = depth_value;
    for (var i = 0; i < 8; i = i + 1) { // the pack's `while`; depth_value <= 1 so it ends within 4 steps
        if depth_march >= depth_value { break; }
        coord = coord + ray_step;
        depth_previous = depth_value;
        depth_value = get_water_height(coord, wv.dir, wv.rot, wv.t);
        depth_march = depth_march + 1.0 / step_count;
    }
    let depth_before = depth_previous - depth_march + 1.0 / step_count;
    let depth_after = depth_value - depth_march;
    return mix(coord, coord - ray_step, depth_after / (depth_after - depth_before));
}

// ---- space_conversion.glsl, wgpu conventions ----
fn lin_depth(z: f32) -> f32 { return w.ray.z * w.ray.w / (w.ray.w - z * (w.ray.w - w.ray.z)); }
fn view_at(uv: vec2<f32>, z: f32) -> vec3<f32> {
    return vec3<f32>((uv.x * 2.0 - 1.0) * w.ray.x, (1.0 - uv.y * 2.0) * w.ray.y, -1.0) * lin_depth(z);
}
fn tbn_of(n: vec3<f32>) -> mat3x3<f32> {
    // get_tbn_matrix; the pack's `normal.y == 1.0` test widened to either pole so a bottom face does not divide by zero
    var t = vec3<f32>(1.0, 0.0, 0.0);
    if abs(n.y) < 0.9999 { t = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), n)); }
    let b = normalize(cross(t, n));
    return mat3x3<f32>(t, b, n);
}

// ---- bsdf.glsl / specular_lighting.glsl ----
fn fresnel_dielectric_n(cos_theta: f32, n: f32) -> f32 {
    let g_sq = sqr(n) + sqr(cos_theta) - 1.0;
    if g_sq < 0.0 { return 1.0; }
    let g = sqrt(g_sq);
    let a = g - cos_theta;
    let b = g + cos_theta;
    return 0.5 * sqr(a / b) * (1.0 + sqr((b * cos_theta - 1.0) / (a * cos_theta + 1.0)));
}
fn fresnel_dielectric(cos_theta: f32, f0: f32) -> f32 {
    let s = sqrt(f0) * 0.99999;
    return fresnel_dielectric_n(cos_theta, (1.0 + s) / (1.0 - s));
}
fn distribution_ggx(NoH_sq: f32, alpha_sq: f32) -> f32 { return alpha_sq / (PI * sqr(1.0 - NoH_sq + NoH_sq * alpha_sq)); }
fn v2_smith_ggx(NoL: f32, NoV: f32, alpha_sq: f32) -> f32 {
    let ggx_l = NoV * sqrt((-NoL * alpha_sq + NoL) * NoL + alpha_sq);
    let ggx_v = NoL * sqrt((-NoV * alpha_sq + NoV) * NoV + alpha_sq);
    return 0.5 / (ggx_l + ggx_v);
}

// GGX spherical area light approximation from Horizon: Zero Dawn
fn get_NoH_squared(NoL: f32, NoV: f32, LoV: f32, light_radius: f32) -> f32 {
    let radius_cos = cos(light_radius);
    let radius_tan = tan(light_radius);
    let RoL = 2.0 * NoL * NoV - LoV;
    if RoL >= radius_cos { return 1.0; }
    let r_over_length_t = radius_cos * radius_tan * inverseSqrt(1.0 - RoL * RoL);
    var not_r = r_over_length_t * (NoV - RoL * NoL);
    var vot_r = r_over_length_t * (2.0 * NoV * NoV - 1.0 - RoL * LoV);
    let triple = sqrt(clamp01(1.0 - NoL * NoL - NoV * NoV - LoV * LoV + 2.0 * NoL * NoV * LoV));
    let NoB_r = r_over_length_t * triple;
    let VoB_r = r_over_length_t * (2.0 * triple * NoV);
    let NoL_vt_r = NoL * radius_cos + NoV + not_r;
    let LoV_vt_r = LoV * radius_cos + 1.0 + vot_r;
    let p = NoB_r * LoV_vt_r;
    let q = NoL_vt_r * LoV_vt_r;
    let s = VoB_r * NoL_vt_r;
    let x_num = q * (-0.5 * p + 0.25 * VoB_r * NoL_vt_r);
    let x_denom = p * p + s * (s - 2.0 * p) + NoL_vt_r * ((NoL * radius_cos + NoV) * LoV_vt_r * LoV_vt_r + q * (-0.5 * (LoV_vt_r + LoV * radius_cos) - 0.5));
    let two_x_1 = 2.0 * x_num / (x_denom * x_denom + x_num * x_num);
    let sin_theta = two_x_1 * x_denom;
    let cos_theta = 1.0 - two_x_1 * x_num;
    not_r = cos_theta * not_r + sin_theta * NoB_r;
    vot_r = cos_theta * vot_r + sin_theta * VoB_r;
    let new_NoL = NoL * radius_cos + not_r;
    let new_LoV = LoV * radius_cos + vot_r;
    let NoH = NoV + new_NoL;
    let HoH = 2.0 * new_LoV + 2.0;
    return clamp01(NoH * NoH / HoH);
}

fn specular_highlight(roughness: f32, NoL: f32, NoV: f32, LoV: f32, LoH: f32) -> f32 {
    let light_radius = select(SUN_ANGULAR_RADIUS, MOON_ANGULAR_RADIUS, w.light_dir.w > 0.5) * DEGREE;
    let fresnel = fresnel_dielectric(LoH, WATER_F0);
    if NoL <= 1e-6 || fresnel < 1e-2 { return 0.0; }
    let NoH_squared = get_NoH_squared(NoL, NoV, LoV, light_radius);
    let alpha_squared = roughness * roughness;
    let d = distribution_ggx(NoH_squared, alpha_squared);
    let v = v2_smith_ggx(max(NoL, 1e-2), max(NoV, 1e-2), alpha_squared);
    return min(NoL * d * v * fresnel, 4.0);
}

// ---- simple_fog.glsl water_fog_simple (scattering rgb, transmittance rgb) ----
fn henyey_greenstein_phase(nu: f32, g: f32) -> f32 {
    let gg = g * g;
    let iso = 0.25 / PI;
    let d = 1.0 + gg - 2.0 * g * nu;
    return (iso - iso * gg) / (d * sqrt(d));
}

struct Fog { scattering: vec3<f32>, transmittance: vec3<f32> };

fn water_fog_simple(light_color: vec3<f32>, ambient_color: vec3<f32>, absorption_coeff: vec3<f32>, light_levels: vec2<f32>, dist_in: f32, LoV: f32, sss_depth: f32) -> Fog {
    let skylight_factor = cube(light_levels.y);
    let scattering_coeff = vec3<f32>(SCATTERING);
    let extinction_coeff = scattering_coeff + absorption_coeff;
    let scattering_albedo = scattering_coeff / extinction_coeff;
    let multiple_scattering_factor = 0.84 * scattering_albedo;
    let multiple_scattering_energy = multiple_scattering_factor / (1.0 - multiple_scattering_factor);
    let dist = max(dist_in, 2.0 - 1.0 * skylight_factor);
    var light_ambient = ambient_color * light_levels.y;
    // blocklight term: light_levels.x is 0 here (one merged light value in the mesh)
    let transmittance = exp(-extinction_coeff * dist);
    let iso = 0.25 / PI;
    var scattering = light_color * exp(-extinction_coeff * sss_depth) * smoothstep(0.0, 0.25, light_levels.y);
    scattering = scattering * (0.7 * henyey_greenstein_phase(LoV, 0.4) + 0.3 * iso);
    scattering = scattering + light_ambient * iso;
    scattering = scattering * (1.0 - transmittance) * scattering_coeff / extinction_coeff;
    scattering = scattering * (1.0 + multiple_scattering_energy);
    var f: Fog;
    f.scattering = scattering;
    f.transmittance = transmittance;
    return f;
}

fn split_2x8(x: f32) -> vec2<f32> {
    let i = u32(x * 65535.0);
    return vec2<f32>(f32(i & 255u), f32(i >> 8u)) / 255.0;
}

fn srgb_encode(x: f32) -> f32 { return select(1.055 * pow(x, 1.0 / 2.4) - 0.055, 12.92 * x, x <= 0.0031308); }

struct FsOut {
    @location(0) refraction: vec4<f32>,
    @location(1) color: vec4<f32>,
};

@fragment
fn fs_water(in: VsOut, @builtin(front_facing) front: bool) -> FsOut {
    // derivatives before any discard (uniform control flow)
    let ng = normalize(cross(dpdy(in.wp), dpdx(in.wp))); // faces the viewer
    let base_srgb = textureSample(tex, samp, in.uv);

    let d1 = textureLoad(depth1, vec2<i32>(in.pos.xy), 0);
    if in.pos.z > d1 { discard; } // behind the opaque layer

    let uv = in.pos.xy * w.res.zw;
    let eye = u.eye.xyz;
    let direction_world = normalize(in.wp - eye);
    let back_world = (w.cam * vec4<f32>(view_at(uv, d1), 1.0)).xyz;
    let layer_dist = distance(in.wp, back_world); // distance to the solid layer along the view ray

    let flat_normal = select(-ng, ng, front);
    let light_levels = vec2<f32>(0.0, in.light);
    let light_color = w.light_color.rgb;
    let ambient_color = w.ambient.rgb;
    let light_dir = w.light_dir.xyz;

    // ---- get_water_material, WATER_TEXTURE_HIGHLIGHT_UNDERGROUND ----
    let r_enc = srgb_encode(base_srgb.r); // the pack reads the stored (non-linear) value
    var texture_highlight = dampen(0.5 * sqr(linear_step(0.63, 1.0, r_enc)) + 0.03 * r_enc);
    texture_highlight = texture_highlight * (1.0 - cube(linear_step(0.0, 0.5, light_levels.y)));
    let absorption_coeff = ABSORPTION; // biome_water_coeff without a biome colour
    var albedo = clamp(vec3<f32>(0.5 * exp(-2.0 * absorption_coeff) * texture_highlight), vec3<f32>(0.0), vec3<f32>(1.0));
    var roughness = WATER_ROUGHNESS + 0.3 * texture_highlight;
    var alpha = 0.01 + texture_highlight;

    // WATER_EDGE_HIGHLIGHT
    let dist = layer_dist * max(abs(direction_world.y), 1e-6);
    var edge_highlight = cube(max(1.0 - 2.0 * dist, 0.0)) * (1.0 + 8.0 * texture_highlight);
    edge_highlight = edge_highlight * WATER_EDGE_HIGHLIGHT_INTENSITY * max(flat_normal.y, 0.0) * (1.0 - 0.5 * sqr(light_levels.y));
    albedo = clamp(albedo + 0.1 * edge_highlight / mix(1.0, max(dot(ambient_color, vec3<f32>(0.2627, 0.678, 0.0593)), 0.5), light_levels.y), vec3<f32>(0.0), vec3<f32>(1.0));
    alpha = alpha + edge_highlight;

    // ---- WATER_WAVES (the pack's `if (true)` block) ----
    let tbn = tbn_of(flat_normal);
    let world_pos = in.wp;
    var coord = -vec2<f32>(dot(world_pos, tbn[0]), dot(world_pos, tbn[1]));
    let vertical_side = abs(flat_normal.y) < 1e-6;
    let flowing_water = abs(flat_normal.y) < 0.99 || vertical_side;
    var flow_dir = vec2<f32>(0.0);
    if vertical_side {
        flow_dir = vec2<f32>(tbn[0].y, tbn[1].y); // (0, 1, 0) * tbn
    } else if flowing_water {
        flow_dir = normalize(flat_normal.xz);
    }
    let direction_tangent = vec3<f32>(dot(direction_world, tbn[0]), dot(direction_world, tbn[1]), dot(direction_world, tbn[2]));
    coord = get_water_parallax_coord(direction_tangent, coord, flow_dir, flowing_water);
    let normal_tangent = get_water_normal(world_pos, flat_normal, coord, flow_dir, light_levels.y, flowing_water);
    let normal = tbn * normal_tangent;

    // ---- angles ----
    let NoL = dot(normal, light_dir);
    let NoV = clamp01(dot(normal, -direction_world));
    let LoV = dot(light_dir, -direction_world);
    let halfway_norm = inverseSqrt(2.0 * LoV + 2.0);
    let LoH = LoV * halfway_norm + halfway_norm;

    // shadows: the pack's no-shadow branch until milestone 3; no cloud shadows
    let shadows = vec3<f32>(pow(light_levels.y, 8.0));

    // ---- get_diffuse_lighting (PROGRAM_GBUFFERS_WATER skips it when the albedo is 0) ----
    var rgb = vec3<f32>(0.0);
    if max(albedo.x, max(albedo.y, albedo.z)) >= 1e-6 {
        let directional_lighting = (0.9 + 0.1 * normal.x) * (0.8 + 0.2 * abs(flat_normal.y));
        let diffuse = vec3<f32>(lift(max(NoL, 0.0), 0.25));
        let bounced = 0.033 * (1.0 - shadows) * (1.0 - 0.1 * max(normal.y, 0.0)) * pow(1.0 + 1e-6, 1.5) * pow(light_levels.y, 4.0);
        var lighting = (diffuse * shadows + bounced) * light_color;
        lighting = lighting + ambient_color * sqr(light_levels.y); // get_sky_lighting, ao = 1
        lighting = lighting + 0.15 * directional_lighting * (1.0 - light_levels.y * light_levels.y); // cave lighting
        rgb = max(lighting, vec3<f32>(0.0)) * albedo / PI;
    }
    rgb = rgb * alpha;

    // specular highlight of the sun / moon
    rgb = rgb + specular_highlight(roughness, NoL, NoV, LoV, LoH) * light_color * shadows;

    // ---- water_absorption_approx ----
    let flat_NoV = dot(flat_normal, direction_world);
    let fdist = layer_dist * select(0.0, 1.0, w.misc.x < 0.5 || flat_NoV >= 0.0);
    let fog = water_fog_simple(light_color, ambient_color, absorption_coeff, light_levels, fdist, -LoV, 0.0);
    var brightness_control = 1.0 - exp(-0.33 * layer_dist);
    brightness_control = brightness_control * max(light_levels.x, light_levels.y);
    let sq = fog.transmittance * fog.transmittance;
    // sqr(water_fog[1]) is a vec3 in the pack
    rgb = rgb + fog.scattering * (1.0 + 6.0 * sq) * brightness_control;
    let out_alpha = 1.0 - fog.transmittance.x;

    // ---- fog (the chunk shader's terrain fog; the pack's common_fog arrives with milestone 4) ----
    if u.fog.w > 0.0 {
        let d = length(in.wp - eye);
        let f = clamp(1.0 - (u.fog.w - d) * 5.0 / (FOG_DENSITY * (1.0 + 0.5 * u.fogp.x) * u.fog.w), 0.0, 1.0);
        rgb = mix(rgb, u.fog.rgb, f * f * (3.0 - 2.0 * f));
    }

    var o: FsOut;
    o.refraction = vec4<f32>(split_2x8(normal_tangent.x * 0.5 + 0.5), split_2x8(normal_tangent.y * 0.5 + 0.5));
    o.color = vec4<f32>(rgb, out_alpha);
    return o;
}
"#;

const COMPOSITE: &str = r#"
@group(0) @binding(0) var<uniform> w: W;
@group(0) @binding(1) var scene: texture_2d<f32>;       // colortex0
@group(0) @binding(2) var refr: texture_2d<f32>;        // colortex3
@group(0) @binding(3) var layer: texture_2d<f32>;       // colortex13
@group(0) @binding(4) var depth0: texture_depth_2d;     // front depth (water)
@group(0) @binding(5) var depth1: texture_depth_2d;     // opaque depth
@group(0) @binding(6) var samp: sampler;

struct FsIn { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> FsIn {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: FsIn;
    o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}

fn lin_depth(z: f32) -> f32 { return w.ray.z * w.ray.w / (w.ray.w - z * (w.ray.w - w.ray.z)); }
fn view_at(uv: vec2<f32>, z: f32) -> vec3<f32> {
    return vec3<f32>((uv.x * 2.0 - 1.0) * w.ray.x, (1.0 - uv.y * 2.0) * w.ray.y, -1.0) * lin_depth(z);
}
fn unsplit_2x8(v: vec2<f32>) -> f32 {
    let lower = u32(v.x * 255.0 + 0.5);
    let upper = u32(v.y * 255.0 + 0.5);
    return f32(lower | (upper << 8u)) / 65535.0;
}

// c1_blend_layers.fsh: refraction (REFRACTION_ALL, intensity 1), then `background * (1 - a) + translucent.rgb`.
@fragment
fn fs_comp(in: FsIn) -> @location(0) vec4<f32> {
    let texel = vec2<i32>(in.pos.xy);
    let dims = vec2<i32>(textureDimensions(scene));
    let front_depth = textureLoad(depth0, texel, 0);
    let back_depth = textureLoad(depth1, texel, 0);
    let refraction_data = textureLoad(refr, texel, 0);
    let translucent = textureLoad(layer, texel, 0);
    let is_translucent = front_depth != back_depth;

    let view_distance = length(view_at(in.uv, front_depth));
    let layer_dist = abs(view_distance - length(view_at(in.uv, back_depth)));

    var refracted_uv = in.uv;
    if is_translucent && any(refraction_data != vec4<f32>(0.0)) {
        let nt = vec2<f32>(unsplit_2x8(refraction_data.xy) * 2.0 - 1.0, unsplit_2x8(refraction_data.zw) * 2.0 - 1.0);
        // uv is y down here, the pack's is y up
        let cand = in.uv + vec2<f32>(nt.x, -nt.y) / max(view_distance, 1.0) * min(layer_dist, 8.0) * 0.1;
        // make sure the refracted fragment is behind the fragment position
        let depth_refracted = textureLoad(depth1, clamp(vec2<i32>(cand * vec2<f32>(dims)), vec2<i32>(0), dims - 1), 0);
        refracted_uv = select(cand, in.uv, depth_refracted < front_depth);
    }
    let background = textureSampleLevel(scene, samp, refracted_uv, 0.0).rgb;
    // air_fog_analytic behind translucents: milestone 4
    return vec4<f32>(background * (1.0 - translucent.a) + translucent.rgb, 1.0);
}
"#;

struct Targets {
    size: (u32, u32),
    depth0: TextureView,
    layer: TextureView,
    refraction: TextureView,
}

pub struct WaterLayer {
    pipeline: RenderPipeline,
    layout1: BindGroupLayout,
    comp_layout: BindGroupLayout,
    comp_module: wgpu::ShaderModule,
    ubuf: Buffer,
    samp: Sampler,
    nsamp: Sampler,
    noise: TextureView,
    targets: Option<Targets>,
    bind1: Option<BindGroup>,
    depth1: Option<TextureView>,
}

impl WaterLayer {
    pub fn new(device: &Device, chunk_layout: &BindGroupLayout, noise: TextureView) -> Self {
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("water_u"), size: std::mem::size_of::<W>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let entry = |binding, visibility, ty| wgpu::BindGroupLayoutEntry { binding, visibility, ty, count: None };
        let tex = |sample_type| wgpu::BindingType::Texture { sample_type, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false };
        let uniform = wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None };
        let vf = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let f = wgpu::ShaderStages::FRAGMENT;
        let layout1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("water_layout1"),
            entries: &[
                entry(0, vf, uniform),
                entry(1, f, tex(wgpu::TextureSampleType::Depth)),
                entry(2, vf, tex(wgpu::TextureSampleType::Float { filterable: true })),
                entry(3, vf, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
            ],
        });
        let comp_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("water_comp_layout"),
            entries: &[
                entry(0, f, uniform),
                entry(1, f, tex(wgpu::TextureSampleType::Float { filterable: true })),
                entry(2, f, tex(wgpu::TextureSampleType::Float { filterable: true })),
                entry(3, f, tex(wgpu::TextureSampleType::Float { filterable: true })),
                entry(4, f, tex(wgpu::TextureSampleType::Depth)),
                entry(5, f, tex(wgpu::TextureSampleType::Depth)),
                entry(6, f, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
            ],
        });
        let samp = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, address_mode_u: wgpu::AddressMode::ClampToEdge, address_mode_v: wgpu::AddressMode::ClampToEdge, ..Default::default() });
        let nsamp = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, address_mode_u: wgpu::AddressMode::Repeat, address_mode_v: wgpu::AddressMode::Repeat, ..Default::default() });

        let src = format!("{COMMON}{}", WATER.replace("$WL$", &format!("{:?}", crate::render::mesh::WATER_LIGHT)));
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("water_shader"), source: wgpu::ShaderSource::Wgsl(src.into()) });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("water"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("water"), bind_group_layouts: &[chunk_layout, &layout1], push_constant_ranges: &[] })),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_water"), buffers: &[Vertex::layout()], compilation_options: wgpu::PipelineCompilationOptions::default() },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_water"),
                // no blending: the layer holds the nearest water, the composite blends it
                targets: &[
                    Some(wgpu::ColorTargetState { format: REFRACTION_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                    Some(wgpu::ColorTargetState { format: TRANSLUCENT_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                ],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() }, // seen from below too
            depth_stencil: Some(wgpu::DepthStencilState { format: DEPTH_FORMAT, depth_write_enabled: true, depth_compare: wgpu::CompareFunction::Less, stencil: wgpu::StencilState::default(), bias: wgpu::DepthBiasState::default() }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let comp_module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("water_comp_shader"), source: wgpu::ShaderSource::Wgsl(format!("{COMMON}{COMPOSITE}").into()) });
        Self { pipeline, layout1, comp_layout, comp_module, ubuf, samp, nsamp, noise, targets: None, bind1: None, depth1: None }
    }

    pub fn update(&self, queue: &Queue, cam: &FirstPersonCamera, w: u32, h: u32, f: &Frame) {
        let (view, proj) = cam.build_view_proj();
        let u = W {
            cam: view.inverse().to_cols_array_2d(),
            ray: [1.0 / proj.x_axis.x, 1.0 / proj.y_axis.y, cam.znear, cam.zfar],
            res: [w as f32, h as f32, 1.0 / w as f32, 1.0 / h as f32],
            light_dir: f.light_dir.extend(if f.moon { 1.0 } else { 0.0 }).to_array(),
            light_color: f.light_color.extend(f.time).to_array(),
            ambient: f.ambient_color.extend(f.rain).to_array(),
            misc: [if f.eye_in_water { 1.0 } else { 0.0 }, f.eye_skylight, 0.0, 0.0],
        };
        queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&u));
    }

    /// Build the size-dependent targets and the bind group that reads the opaque depth. Call every frame before `begin`
    /// (ponytail: one small bind group per frame, because the depth view is replaced on resize).
    pub fn prepare(&mut self, device: &Device, opaque_depth: &TextureView, w: u32, h: u32) {
        if self.targets.as_ref().map_or(true, |t| t.size != (w, h)) {
            let make = |format, label| {
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some(label),
                        size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    })
                    .create_view(&wgpu::TextureViewDescriptor::default())
            };
            self.targets = Some(Targets { size: (w, h), depth0: make(DEPTH_FORMAT, "water_depth0"), layer: make(TRANSLUCENT_FORMAT, "water_layer"), refraction: make(REFRACTION_FORMAT, "water_refraction") });
        }
        self.bind1 = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("water_bind1"),
            layout: &self.layout1,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(opaque_depth) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.noise) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.nsamp) },
            ],
        }));
        self.depth1 = Some(opaque_depth.clone());
    }

    /// The pass that fills the layer: refraction normal + translucent colour cleared to 0, depth to 1.
    pub fn begin<'a>(&'a self, enc: &'a mut CommandEncoder) -> RenderPass<'a> {
        let t = self.targets.as_ref().expect("prepare before begin");
        let clear = |view| Some(wgpu::RenderPassColorAttachment { view, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } });
        enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("water_layer"),
            color_attachments: &[clear(&t.refraction), clear(&t.layer)],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &t.depth0,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        })
    }

    /// Water pipeline + group 1; the caller sets group 0 (the chunk pipeline's) and draws the water index ranges.
    pub fn setup<'a>(&'a self, rp: &mut RenderPass<'a>) {
        rp.set_pipeline(&self.pipeline);
        rp.set_bind_group(1, self.bind1.as_ref().expect("prepare before setup"), &[]);
    }

    /// `c1_blend_layers`: `scene` refracted and blended with the layer, into `dst` (must not be `scene`).
    pub fn composite(&self, device: &Device, enc: &mut CommandEncoder, scene: &TextureView, dst: &TextureView, dst_format: wgpu::TextureFormat) {
        let t = self.targets.as_ref().expect("prepare before composite");
        let d1 = self.depth1.as_ref().expect("prepare before composite");
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("water_comp_bind"),
            layout: &self.comp_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(scene) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&t.refraction) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&t.layer) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&t.depth0) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(d1) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::Sampler(&self.samp) },
            ],
        });
        // ponytail: the pipeline is rebuilt per call; cache it per `dst_format` when this runs every frame.
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("water_comp"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("water_comp"), bind_group_layouts: &[&self.comp_layout], push_constant_ranges: &[] })),
            vertex: wgpu::VertexState { module: &self.comp_module, entry_point: Some("vs_full"), buffers: &[], compilation_options: wgpu::PipelineCompilationOptions::default() },
            fragment: Some(wgpu::FragmentState { module: &self.comp_module, entry_point: Some("fs_comp"), targets: &[Some(wgpu::ColorTargetState { format: dst_format, blend: None, write_mask: wgpu::ColorWrites::ALL })], compilation_options: wgpu::PipelineCompilationOptions::default() }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("water_composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: dst, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        rp.set_pipeline(&pipeline);
        rp.set_bind_group(0, &bind, &[]);
        rp.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one check: the shader text has no unfilled placeholder, the pack's wave constants are what `settings.glsl` has, and the
    /// refraction normal packs in 2 x 8 bits exactly like `split_2x8` / `unsplit_2x8` (checked here on the CPU).
    #[test]
    fn water_text_and_pack() {
        let s = format!("{COMMON}{}", WATER.replace("$WL$", "0.9"));
        assert!(!s.contains('$'));
        assert!(s.contains("WATER_WAVE_ITERATIONS: i32 = 3") && s.contains("vec3<f32>(0.39, 0.14, 0.07)"));
        let split = |x: f32| {
            let i = (x * 65535.0) as u32;
            [(i & 255) as f32 / 255.0, (i >> 8) as f32 / 255.0]
        };
        let unsplit = |v: [f32; 2]| (((v[0] * 255.0 + 0.5) as u32) | (((v[1] * 255.0 + 0.5) as u32) << 8)) as f32 / 65535.0;
        for x in [0.0f32, 0.25, 0.5, 0.731, 1.0] {
            assert!((unsplit(split(x)) - x).abs() < 2.0 / 65535.0, "{x}");
        }
    }
}
