//! Milestone 6, part 1: ground-truth ambient occlusion, a line-for-line port of Photon's `program/d3_ao.fsh`
//! (half-resolution pass + temporal accumulation) with `include/lighting/ao/gtao.glsl` (`integrate_arc`,
//! `compute_maximum_horizon_angle`, `compute_gtao`) and `include/utility/slerp.glsl`, `bicubic.glsl`
//! (`catmull_rom_filter_fast`), `random.glsl` (`r2`). Constants are the pack's defaults (`GTAO_SLICES 2`, `GTAO_HORIZON_STEPS 3`,
//! `GTAO_RADIUS 2.0`, `ao_render_scale 0.5`, `max_accumulated_frames 10`, `depth_rejection_strength 16`,
//! `offcenter_rejection_strength 0.25`).
//!
//! Output (what `colortex6` / `colortex14` are in the pack), both half resolution, ping-ponged so last frame is the history:
//! - `ambient`: Rgba16Float = (ao, ambient SSS, bent normal xy * 0.5 + 0.5, view space);
//! - `history_data`: Rg16Float = (1 - depth, pixel age).
//! `d4_deferred_shading` (milestone 1) consumes them with `AO_UPSAMPLE_WGSL` below (its depth-aware 2x2 upsample).
//!
//! Differences from the pack, each because mc-rs has no such input yet:
//! - the normal is rebuilt from the depth buffer and snapped to an axis (the same `face_normal` as `render::vl`; every face in this
//!   world is axis aligned, so it equals the pack's flat gbuffer normal); when milestone 1's G-buffer exists, replace
//!   `face_normal` by the decode of its normal and nothing else changes;
//! - no hand (`hand_depth` tests dropped: nothing is drawn as a hand), no Distant Horizons (`LOD_MOD_ACTIVE` branches dropped);
//! - the jitter (`taa_offset`) is an argument of `run` (zero until milestone 5's TAA exists);
//! - GL depth is -1..1, wgpu's is 0..1 and y is down: `view_at` / `view_to_screen` are `screen_to_view_space` /
//!   `view_to_screen_space` with those two conventions; reprojection is `prev_proj * prev_view * world` because the camera matrix
//!   here is in world space (no `cameraPosition - previousCameraPosition` offset).
//! UNVERIFIED: written WITHOUT a Rust toolchain, never compiled or run.

use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use wgpu::{BindGroup, BindGroupLayout, Buffer, CommandEncoder, Device, RenderPipeline, Sampler, TextureView};

use crate::gpu::context::Gpu;
use crate::render::camera::FirstPersonCamera;

const SLICES: u32 = 2; // GTAO_SLICES
const STEPS: u32 = 3; // GTAO_HORIZON_STEPS
const RADIUS: f32 = 2.0; // GTAO_RADIUS

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct U {
    view: [[f32; 4]; 4],
    cam: [[f32; 4]; 4],
    proj: [[f32; 4]; 4],
    prev_vp: [[f32; 4]; 4],
    prev_view: [[f32; 4]; 4],
    /// 1 / proj[0][0], 1 / proj[1][1], near, far.
    ray: [f32; 4],
    /// x: frameCounter, yz: `taa_offset` (ndc), w: unused.
    p: [f32; 4],
}

/// The depth-aware upsample of `d4_deferred_shading.fsh` (lines "Upscale ambient occlusion"), as a WGSL function for that pass to
/// paste: `ao_upsample(amb, dat, frag_xy, lin_z)` -> the 4 channels of `ambient`. `frag_xy` is `@builtin(position).xy`,
/// `lin_z` the pixel's linear depth (`near * far / (far - z * (far - near))`).
pub const AO_UPSAMPLE_WGSL: &str = r#"
fn ao_upsample(amb: texture_2d<f32>, dat: texture_2d<f32>, frag_xy: vec2<f32>, lin_z: f32, near: f32, far: f32) -> vec4<f32> {
    let half_res_pos = frag_xy * 0.5 - 0.5;           // gl_FragCoord.xy * (0.5 / taau_render_scale) - 0.5
    let i = vec2<i32>(floor(half_res_pos));
    let f = fract(half_res_pos);
    let dims = vec2<i32>(textureDimensions(amb));
    let p00 = clamp(i, vec2<i32>(0), dims - 1);
    let p10 = clamp(i + vec2<i32>(1, 0), vec2<i32>(0), dims - 1);
    let p01 = clamp(i + vec2<i32>(0, 1), vec2<i32>(0), dims - 1);
    let p11 = clamp(i + vec2<i32>(1, 1), vec2<i32>(0), dims - 1);
    let a00 = textureLoad(amb, p00, 0); let a10 = textureLoad(amb, p10, 0);
    let a01 = textureLoad(amb, p01, 0); let a11 = textureLoad(amb, p11, 0);
    // depth_weight(reversed_depth) = exp2(-10 * |linear(1 - reversed_depth) - lin_z|)
    let w00 = exp2(-10.0 * abs(near * far / (far - (1.0 - textureLoad(dat, p00, 0).x) * (far - near)) - lin_z)) * (1.0 - f.x) * (1.0 - f.y);
    let w10 = exp2(-10.0 * abs(near * far / (far - (1.0 - textureLoad(dat, p10, 0).x) * (far - near)) - lin_z)) * (f.x - f.x * f.y);
    let w01 = exp2(-10.0 * abs(near * far / (far - (1.0 - textureLoad(dat, p01, 0).x) * (far - near)) - lin_z)) * (f.y - f.x * f.y);
    let w11 = exp2(-10.0 * abs(near * far / (far - (1.0 - textureLoad(dat, p11, 0).x) * (far - near)) - lin_z)) * (f.x * f.y);
    let s = w00 + w10 + w01 + w11;
    if abs(s) > 1e-6 { return (a00 * w00 + a10 * w10 + a01 * w01 + a11 * w11) / s; }
    return a00;
}
"#;

const SHADER: &str = r#"
const PI: f32 = 3.14159265359;
const HALF_PI: f32 = 1.57079632679;
const RCP_PI: f32 = 0.31830988618;
const SLICES: i32 = $SLICES$;
const STEPS: i32 = $STEPS$;
const GTAO_RADIUS: f32 = $RADIUS$;
const GTAO_FALLOFF_START: f32 = 0.75;
const MAX_FRAMES: f32 = 10.0;           // max_accumulated_frames
const DEPTH_REJECTION: f32 = 16.0;      // depth_rejection_strength
const OFFCENTER_REJECTION: f32 = 0.25;  // offcenter_rejection_strength

struct U {
    view: mat4x4<f32>,
    cam: mat4x4<f32>,
    proj: mat4x4<f32>,
    prev_vp: mat4x4<f32>,
    prev_view: mat4x4<f32>,
    ray: vec4<f32>,
    p: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var depth_tex: texture_depth_2d;
@group(0) @binding(2) var noise_tex: texture_2d<f32>;
@group(0) @binding(3) var hist_amb: texture_2d<f32>;   // colortex6, last frame
@group(0) @binding(4) var hist_dat: texture_2d<f32>;   // colortex14, last frame
@group(0) @binding(5) var samp: sampler;               // linear, clamp

struct FsIn { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> FsIn {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: FsIn;
    o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}

fn clamp01(x: f32) -> f32 { return clamp(x, 0.0, 1.0); }
fn linear_step(a: f32, b: f32, x: f32) -> f32 { return clamp01((x - a) / (b - a)); }

// fast_math.glsl
fn fast_acos(x: f32) -> f32 {
    let C0 = 1.57018; let C1 = -0.201877; let C2 = 0.0464619;
    var res = (C2 * abs(x) + C1) * abs(x) + C0;
    res = res * sqrt(1.0 - abs(x));
    return select(PI - res, res, x >= 0.0);
}

// random.glsl r2(n, seed)
fn r2(n: f32, seed: vec2<f32>) -> vec2<f32> {
    let phi2 = 1.3247179572;
    return fract(seed + n * (1.0 / vec2<f32>(phi2, phi2 * phi2)));
}

// slerp.glsl
fn slerp(v0: vec3<f32>, v1: vec3<f32>, t: f32) -> vec3<f32> {
    let cos_theta = dot(v0, v1);
    if cos_theta > 0.999 { return v0; }
    let theta = fast_acos(cos_theta);
    let rcp_sin = 1.0 / sin(theta);
    return v0 * (rcp_sin * sin((1.0 - t) * theta)) + v1 * (rcp_sin * sin(t * theta));
}

// bicubic.glsl catmull_rom_filter_fast (5 bilinear taps)
fn catmull_rom(tex: texture_2d<f32>, coord: vec2<f32>, sharpness: f32) -> vec4<f32> {
    let res = vec2<f32>(textureDimensions(tex));
    let px = 1.0 / res;
    let position = res * coord;
    let center = floor(position - 0.5) + 0.5;
    let f = position - center;
    let f2 = f * f;
    let f3 = f * f2;
    let w0 = -sharpness * f3 + 2.0 * sharpness * f2 - sharpness * f;
    let w1 = (2.0 - sharpness) * f3 - (3.0 - sharpness) * f2 + 1.0;
    let w2 = -(2.0 - sharpness) * f3 + (3.0 - 2.0 * sharpness) * f2 + sharpness * f;
    let w3 = sharpness * f3 - sharpness * f2;
    let w12 = w1 + w2;
    let tc12 = px * (center + w2 / w12);
    let center_color = textureSampleLevel(tex, samp, tc12, 0.0);
    let tc0 = px * (center - 1.0);
    let tc3 = px * (center + 2.0);
    let l0 = w12.x * w0.y;
    let l1 = w0.x * w12.y;
    let l2 = w12.x * w12.y;
    let l3 = w3.x * w12.y;
    let l4 = w12.x * w3.y;
    let color = textureSampleLevel(tex, samp, vec2<f32>(tc12.x, tc0.y), 0.0) * l0
        + textureSampleLevel(tex, samp, vec2<f32>(tc0.x, tc12.y), 0.0) * l1 + center_color * l2
        + textureSampleLevel(tex, samp, vec2<f32>(tc3.x, tc12.y), 0.0) * l3
        + textureSampleLevel(tex, samp, vec2<f32>(tc12.x, tc3.y), 0.0) * l4;
    return color / (l0 + l1 + l2 + l3 + l4);
}

// space_conversion.glsl, wgpu conventions (depth 0..1, uv y down). Jitter as `handle_jitter = true`.
fn lin_depth(z: f32) -> f32 { return u.ray.z * u.ray.w / (u.ray.w - z * (u.ray.w - u.ray.z)); }

fn view_at(uv: vec2<f32>, z: f32) -> vec3<f32> {
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0) - u.p.yz;
    return vec3<f32>(ndc.x * u.ray.x, ndc.y * u.ray.y, -1.0) * lin_depth(z);
}

fn view_to_screen(v: vec3<f32>) -> vec3<f32> {
    let c = u.proj * vec4<f32>(v, 1.0);
    let ndc = c.xyz / c.w + vec3<f32>(u.p.yz, 0.0);
    return vec3<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5, ndc.z);
}

// reproject_scene_space: the camera matrices here are world space, so the camera offset is already inside them.
fn reproject(world: vec3<f32>) -> vec2<f32> {
    let c = u.prev_vp * vec4<f32>(world, 1.0);
    let ndc = c.xy / c.w;
    return vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
}

fn view_pos(ip: vec2<i32>) -> vec3<f32> {
    let dims = vec2<i32>(textureDimensions(depth_tex));
    let q = clamp(ip, vec2<i32>(0), dims - 1);
    return view_at((vec2<f32>(q) + 0.5) / vec2<f32>(dims), textureLoad(depth_tex, q, 0));
}

// ponytail: the gbuffer's flat normal, rebuilt from depth (same as render::vl `face_normal`); a one-pixel line on the edge between
// two faces can pick the neighbour's normal. Upgrade: decode the G-buffer normal of milestone 1 here.
fn face_normal(ip: vec2<i32>, p0: vec3<f32>) -> vec3<f32> {
    let a = view_pos(ip + vec2<i32>(2, 0)) - p0;
    let b = p0 - view_pos(ip - vec2<i32>(2, 0));
    let c = view_pos(ip + vec2<i32>(0, 2)) - p0;
    let d = p0 - view_pos(ip - vec2<i32>(0, 2));
    let cr = cross(select(b, a, abs(a.z) < abs(b.z)), select(d, c, abs(c.z) < abs(d.z)));
    if dot(cr, cr) < 1e-12 { return vec3<f32>(0.0); }
    var n = normalize(cr);
    if dot(n, p0) > 0.0 { n = -n; }
    let w = (u.cam * vec4<f32>(n, 0.0)).xyz;
    let m = abs(w);
    if m.x >= m.y && m.x >= m.z { return vec3<f32>(sign(w.x), 0.0, 0.0); }
    if m.y >= m.z { return vec3<f32>(0.0, sign(w.y), 0.0); }
    return vec3<f32>(0.0, 0.0, sign(w.z));
}

// gtao.glsl
fn integrate_arc(h: vec2<f32>, n: f32, cos_n: f32) -> f32 {
    let tmp = cos_n + 2.0 * h * sin(n) - cos(2.0 * h - n);
    return 0.25 * (tmp.x + tmp.y);
}

fn max_horizon_angle(view_slice_dir: vec3<f32>, viewer_dir: vec3<f32>, screen_pos: vec3<f32>, vpos: vec3<f32>, radius: f32, dither: f32) -> f32 {
    let step_size = (GTAO_RADIUS / f32(STEPS)) * radius;
    var max_cos_theta = -1.0;
    let ray_step = (view_to_screen(vpos + view_slice_dir * step_size) - screen_pos).xy;
    let pixel = max(1.0 / f32(textureDimensions(depth_tex).x), 1.0 / f32(textureDimensions(depth_tex).y)); // max_of(view_pixel_size)
    let ray_pos0 = screen_pos.xy + ray_step * (dither + pixel * inverseSqrt(dot(ray_step, ray_step)));
    let dims = vec2<f32>(textureDimensions(depth_tex));
    for (var i = 0; i < STEPS; i = i + 1) {
        // `ray_pos += ray_step` of the pack's loop, written as a product so `continue` cannot skip it.
        let ray_pos = ray_pos0 + ray_step * f32(i);
        let texel = vec2<i32>(clamp(ray_pos, vec2<f32>(0.0), vec2<f32>(1.0)) * dims - 0.5);
        let depth = textureLoad(depth_tex, clamp(texel, vec2<i32>(0), vec2<i32>(dims) - 1), 0);
        if depth == 1.0 || depth == screen_pos.z { continue; } // `depth < hand_depth` dropped: no hand
        let offset = view_at(ray_pos, depth) - vpos;
        let len_sq = dot(offset, offset);
        let norm = inverseSqrt(len_sq);
        let distance_falloff = linear_step(GTAO_FALLOFF_START * GTAO_RADIUS, GTAO_RADIUS, len_sq * norm / radius);
        var cos_theta = dot(viewer_dir, offset) * norm;
        cos_theta = mix(cos_theta, -1.0, distance_falloff);
        max_cos_theta = max(cos_theta, max_cos_theta);
    }
    return fast_acos(clamp(max_cos_theta, -1.0, 1.0));
}

struct Gtao { ao: vec2<f32>, bent: vec3<f32> };

fn compute_gtao(screen_pos: vec3<f32>, vpos: vec3<f32>, view_normal: vec3<f32>, dither: vec2<f32>) -> Gtao {
    var ao = 0.0;
    var ambient_sss = 0.0;
    var bent_normal = vec3<f32>(0.0);

    let viewer_dir = normalize(-vpos);
    let viewer_right = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), viewer_dir));
    let viewer_up = cross(viewer_dir, viewer_right);
    let local_to_view = mat3x3<f32>(viewer_right, viewer_up, viewer_dir);

    // Reduce AO radius very close up, makes some screen-space artifacts less obvious
    let ao_radius = max(0.25 + 0.75 * smoothstep(0.0, 81.0, dot(vpos, vpos)), 0.5);

    for (var i = 0; i < SLICES; i = i + 1) {
        let slice_angle = (f32(i) + dither.x) * (PI / f32(SLICES));
        let slice_dir = vec3<f32>(cos(slice_angle), sin(slice_angle), 0.0);
        let view_slice_dir = local_to_view * slice_dir;

        let ortho_dir = slice_dir - dot(slice_dir, viewer_dir) * viewer_dir;
        let axis = cross(slice_dir, viewer_dir);
        let projected_normal = view_normal - axis * dot(view_normal, axis);

        let len_sq = dot(projected_normal, projected_normal);
        let norm = inverseSqrt(len_sq);

        let sgn_gamma = sign(dot(ortho_dir, projected_normal));
        let cos_gamma = clamp01(dot(projected_normal, viewer_dir) * norm);
        let gamma = sgn_gamma * fast_acos(cos_gamma);

        var h: vec2<f32>;
        h.x = max_horizon_angle(-view_slice_dir, viewer_dir, screen_pos, vpos, ao_radius, dither.y);
        h.y = max_horizon_angle(view_slice_dir, viewer_dir, screen_pos, vpos, ao_radius, dither.y);

        ambient_sss += max(h.y - HALF_PI, 0.0) * RCP_PI;

        h = gamma + clamp(vec2<f32>(-1.0, 1.0) * h - gamma, vec2<f32>(-HALF_PI), vec2<f32>(HALF_PI));
        ao += integrate_arc(h, gamma, cos_gamma) * len_sq * norm;

        let bent_angle = dot(h, vec2<f32>(0.5));
        bent_normal += viewer_dir * cos(bent_angle) + ortho_dir * sin(bent_angle);
    }

    let albedo = 0.2; // albedo of surroundings (for multibounce approx)
    ao = ao / f32(SLICES);
    ambient_sss = ambient_sss / f32(SLICES);
    ao = ao / (albedo * ao + (1.0 - albedo));
    bent_normal = normalize(normalize(bent_normal) - 0.5 * viewer_dir);

    var g: Gtao;
    g.ao = clamp(vec2<f32>(ao, ambient_sss), vec2<f32>(0.0), vec2<f32>(1.0));
    g.bent = bent_normal;
    return g;
}

struct Out {
    @location(0) ambient: vec4<f32>, // ao, ambient SSS, bent normal xy * 0.5 + 0.5 (view space)
    @location(1) data: vec2<f32>,    // 1 - depth, pixel age
};

@fragment
fn fs_ao(in: FsIn) -> Out {
    var o: Out;
    o.ambient = vec4<f32>(1.0, 0.0, 0.0, 0.0);
    o.data = vec2<f32>(0.0);

    let texel = vec2<i32>(in.pos.xy);
    let dims = vec2<i32>(textureDimensions(depth_tex));
    // view_texel = ivec2(gl_FragCoord.xy * (taau_render_scale / ao_render_scale))
    let view_texel = vec2<i32>(in.pos.xy * 2.0);
    if any(view_texel >= dims) { return o; }

    let depth = textureLoad(depth_tex, view_texel, 0);
    if depth == 1.0 { return o; } // sky: ambient = (1, 0, 0, 0), history data 0

    let dither0 = vec2<f32>(
        textureLoad(noise_tex, texel & vec2<i32>(511), 0).b,
        textureLoad(noise_tex, (texel + vec2<i32>(249)) & vec2<i32>(511), 0).b,
    );
    let dither = r2(u.p.x, dither0);

    let screen_pos = vec3<f32>(in.uv, depth);
    let vpos = view_at(in.uv, depth);
    let world = (u.cam * vec4<f32>(vpos, 1.0)).xyz;
    let prev = reproject(world);

    let world_normal = face_normal(view_texel, vpos);
    var view_normal = (u.view * vec4<f32>(world_normal, 0.0)).xyz;
    if dot(world_normal, world_normal) < 0.5 { view_normal = -normalize(vpos); } // degenerate depth: face the eye

    let g = compute_gtao(screen_pos, vpos, view_normal, dither);
    var ao = g.ao;
    var bent_normal = g.bent;

    // Temporal accumulation
    let history = max(catmull_rom(hist_amb, prev, 0.65), vec4<f32>(0.0));
    let history_data = max(textureSampleLevel(hist_dat, samp, prev, 0.0).xy, vec2<f32>(0.0));

    if all(clamp(prev, vec2<f32>(0.0), vec2<f32>(1.0)) == prev) {
        let history_depth = 1.0 - history_data.x;
        var pixel_age = min(history_data.y, MAX_FRAMES);

        var history_bent: vec3<f32>;
        history_bent.x = history.z * 2.0 - 1.0;
        history_bent.y = history.w * 2.0 - 1.0;
        history_bent.z = sqrt(clamp01(1.0 - dot(history_bent.xy, history_bent.xy)));

        // Reproject bent normal: `v * mat3(previous)` is the transposed product, i.e. previous view -> world, then current view.
        let pv = mat3x3<f32>(u.prev_view[0].xyz, u.prev_view[1].xyz, u.prev_view[2].xyz);
        let cv = mat3x3<f32>(u.view[0].xyz, u.view[1].xyz, u.view[2].xyz);
        history_bent = cv * (history_bent * pv);

        // Depth rejection
        let view_norm = inverseSqrt(dot(vpos, vpos));
        let NoV = abs(dot(view_normal, vpos)) * view_norm;
        let z0 = lin_depth(depth);
        let z1 = lin_depth(history_depth);
        let depth_weight = exp2(-abs(z0 - z1) * DEPTH_REJECTION * NoV * view_norm);

        // Offcenter rejection from Jessie, which is originally by Zombye. Reduces blur in motion
        let hres = vec2<f32>(textureDimensions(hist_amb));
        let pixel_offset = 1.0 - abs(2.0 * fract(hres * prev) - 1.0);
        let offcenter_rejection = sqrt(pixel_offset.x * pixel_offset.y) * OFFCENTER_REJECTION + (1.0 - OFFCENTER_REJECTION);

        pixel_age = pixel_age * depth_weight * offcenter_rejection * select(0.0, 1.0, history_depth != 1.0);

        // Blend with history
        let history_weight = pixel_age / (pixel_age + 1.0);
        ao = mix(ao, history.xy, history_weight);
        bent_normal = slerp(bent_normal, history_bent, history_weight);

        o.ambient = vec4<f32>(ao, bent_normal.xy * 0.5 + 0.5);
        o.data = vec2<f32>(1.0 - depth, pixel_age + 1.0);
    } else {
        o.ambient = vec4<f32>(ao, bent_normal.xy * 0.5 + 0.5);
        o.data = vec2<f32>(0.0);
    }
    return o;
}
"#;

struct Sets {
    /// Frame size this was built for (the depth view in the bind groups goes stale when it changes).
    size: (u32, u32),
    ambient: [TextureView; 2],
    data: [TextureView; 2],
    /// `bind[k]` writes set `k` and reads set `1 - k` as the history.
    bind: [BindGroup; 2],
    /// The set written by the last `run`.
    cur: usize,
}

pub struct Gtao {
    pipeline: RenderPipeline,
    layout: BindGroupLayout,
    ubuf: Buffer,
    samp: Sampler,
    noise: TextureView,
    sets: Option<Sets>,
    prev: Option<(Mat4, Mat4)>, // previous (proj * view, view)
    frame: u32,
}

impl Gtao {
    pub fn new(device: &Device, noise: TextureView) -> Self {
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gtao_uniform"),
            size: std::mem::size_of::<U>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty, count: None };
        let tex = |sample_type| wgpu::BindingType::Texture { sample_type, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gtao_layout"),
            entries: &[
                entry(0, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }),
                entry(1, tex(wgpu::TextureSampleType::Depth)),
                entry(2, tex(wgpu::TextureSampleType::Float { filterable: false })),
                entry(3, tex(wgpu::TextureSampleType::Float { filterable: true })),
                entry(4, tex(wgpu::TextureSampleType::Float { filterable: true })),
                entry(5, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
            ],
        });
        let samp = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let src = shader_source();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("gtao_shader"), source: wgpu::ShaderSource::Wgsl(src.into()) });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("gtao"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("gtao"), bind_group_layouts: &[&layout], push_constant_ranges: &[] })),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_full"), buffers: &[], compilation_options: wgpu::PipelineCompilationOptions::default() },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_ao"),
                targets: &[
                    Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba16Float, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                    Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rg16Float, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                ],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        Self { pipeline, layout, ubuf, samp, noise, sets: None, prev: None, frame: 0 }
    }

    /// Run after the opaque terrain pass (the depth buffer must have been stored). `jitter` is `taa_offset` in ndc (zero without TAA).
    pub fn run(&mut self, gpu: &Gpu, enc: &mut CommandEncoder, cam: &FirstPersonCamera, jitter: [f32; 2]) {
        let (w, h) = (gpu.config.width, gpu.config.height);
        if self.sets.as_ref().map_or(true, |s| s.size != (w, h)) {
            self.prev = None; // history is the wrong size
            let (hw, hh) = ((w / 2).max(1), (h / 2).max(1)); // ao_render_scale = 0.5
            let make = |format, label| {
                [0, 1].map(|_| {
                    gpu.device
                        .create_texture(&wgpu::TextureDescriptor {
                            label: Some(label),
                            size: wgpu::Extent3d { width: hw, height: hh, depth_or_array_layers: 1 },
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format,
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                            view_formats: &[],
                        })
                        .create_view(&wgpu::TextureViewDescriptor::default())
                })
            };
            let ambient = make(wgpu::TextureFormat::Rgba16Float, "gtao_ambient");
            let data = make(wgpu::TextureFormat::Rg16Float, "gtao_history_data");
            let bind = [0usize, 1].map(|k| {
                let r = 1 - k;
                gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("gtao_bind"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: self.ubuf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&gpu.depth_view) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.noise) },
                        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&ambient[r]) },
                        wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&data[r]) },
                        wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&self.samp) },
                    ],
                })
            });
            self.sets = Some(Sets { size: (w, h), ambient, data, bind, cur: 1 });
        }
        let (view, proj) = cam.build_view_proj();
        let (prev_vp, prev_view) = self.prev.unwrap_or((proj * view, view));
        let u = U {
            view: view.to_cols_array_2d(),
            cam: view.inverse().to_cols_array_2d(),
            proj: proj.to_cols_array_2d(),
            prev_vp: prev_vp.to_cols_array_2d(),
            prev_view: prev_view.to_cols_array_2d(),
            ray: [1.0 / proj.x_axis.x, 1.0 / proj.y_axis.y, cam.znear, cam.zfar],
            p: [self.frame as f32, jitter[0], jitter[1], 0.0],
        };
        gpu.queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&u));
        self.prev = Some((proj * view, view));
        self.frame = self.frame.wrapping_add(1);

        let s = self.sets.as_mut().unwrap();
        let k = 1 - s.cur;
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("gtao"),
                color_attachments: &[
                    Some(wgpu::RenderPassColorAttachment { view: &s.ambient[k], resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } }),
                    Some(wgpu::RenderPassColorAttachment { view: &s.data[k], resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } }),
                ],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rp.set_pipeline(&self.pipeline);
            rp.set_bind_group(0, &s.bind[k], &[]);
            rp.draw(0..3, 0..1);
        }
        s.cur = k;
    }

    /// `colortex6` of this frame (None before the first `run`): half resolution Rgba16Float.
    pub fn ambient(&self) -> Option<&TextureView> {
        self.sets.as_ref().map(|s| &s.ambient[s.cur])
    }

    /// `colortex14` of this frame: half resolution Rg16Float, (1 - depth, pixel age).
    pub fn history_data(&self) -> Option<&TextureView> {
        self.sets.as_ref().map(|s| &s.data[s.cur])
    }
}

fn shader_source() -> String {
    SHADER
        .replace("$SLICES$", &SLICES.to_string())
        .replace("$STEPS$", &STEPS.to_string())
        .replace("$RADIUS$", &format!("{RADIUS:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one check: every `$NAME$` placeholder is filled, the constants are the pack's defaults, and the half-resolution upsample
    /// helper carries the pack's weight (`exp2(-10 * |dz|)`).
    #[test]
    fn shader_text_is_complete() {
        let s = shader_source();
        assert!(!s.contains('$'), "unfilled placeholder");
        assert!(s.contains("const SLICES: i32 = 2;") && s.contains("const STEPS: i32 = 3;") && s.contains("const GTAO_RADIUS: f32 = 2.0;"));
        assert!(AO_UPSAMPLE_WGSL.contains("exp2(-10.0"));
    }
}
