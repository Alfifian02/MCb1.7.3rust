// Entry points of Photon's p0_clouds_prep, d1_clouds and d2_clouds_upscaling, plus two small helper passes mc-rs needs
// (`fs_depth_history`: what d3_ao leaves in colortex14 for d2; `fs_mip`: the mip chain d4 reads colortex11 with).
// The library (`clouds_lib.wgsl`) is joined in front of this file.
// Screen space here is wgpu's: uv origin top-left, ndc y up, depth 0..1 (the pack's is GL: bottom-left, -1..1).

struct U {
    view_inv: mat4x4<f32>,       // gbufferModelViewInverse (rotation only: scene space is world - eye)
    proj_inv: mat4x4<f32>,       // gbufferProjectionInverse
    prev_view: mat4x4<f32>,      // gbufferPreviousModelView (rotation only)
    prev_proj: mat4x4<f32>,      // gbufferPreviousProjection
    shadow_view: mat4x4<f32>,    // shadowModelView: scene -> light space, +z toward the light
    shadow_view_inv: mat4x4<f32>,
    cam: vec4<f32>,              // cameraPosition.xyz, eyeAltitude
    prev_cam: vec4<f32>,         // previousCameraPosition.xyz, near
    sun: vec4<f32>,              // sun_dir.xyz, sunAngle (OptiFine: 0 sunrise .. 0.5 sunset)
    moon: vec4<f32>,             // moon_dir.xyz, rainStrength
    light: vec4<f32>,            // light_dir.xyz, wetness
    res: vec4<f32>,              // view_res.xy, view_pixel_size.xy
    taa: vec4<f32>,              // taa_offset.xy, far, frameTimeCounter
    t: vec4<f32>,                // world_age, frameTime, day_factor, atmosphere_saturation_boost_amount
    time: vec4<f32>,             // time_sunrise, time_noon, time_sunset, time_midnight
    flags: vec4<f32>,            // daylight_cycle_enabled, world_age_changed, moon_phase_brightness, 0
    low: vec4<f32>,              // size of the low-res cloud target (colortex9), 0, 0
    sun_rgb: vec4<f32>,          // get_sun_exposure() * get_sun_tint()
    moon_rgb: vec4<f32>,         // get_moon_exposure() * get_moon_tint()
    weather_rgb: vec4<f32>,      // get_weather_color()
    aur_amount: vec4<f32>,       // aurora_amount
    aur0: vec4<f32>,             // aurora_colors[0]
    aur1: vec4<f32>,             // aurora_colors[1]
    cp_a: vec4<f32>,             // CloudsParameters, packed in declaration order (see `CloudsParameters::pack`)
    cp_b: vec4<f32>,
    cp_c: vec4<f32>,
    cp_d: vec4<f32>,
    cp_e: vec4<f32>,
    ints: vec4<u32>,             // frameCounter, moonPhase, 0, 0
}

@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var samp_repeat: sampler;
@group(0) @binding(2) var samp_clamp: sampler;
@group(0) @binding(3) var noisetex: texture_2d<f32>;        // noise.png, 512^2 RGBA8
@group(0) @binding(4) var worley_bubbly: texture_3d<f32>;   // colortex6 in the pack
@group(0) @binding(5) var worley_swirley: texture_3d<f32>;  // colortex7
@group(0) @binding(6) var atmosphere_lut: texture_3d<f32>;  // depthtex0: scattering.dat, 32 x 64 x 32
@group(0) @binding(7) var cloud_map: texture_2d<f32>;       // colortex8: xy cloud shadow, z cumulus coverage
@group(0) @binding(8) var depth_tex: texture_depth_2d;      // depthtex1
@group(0) @binding(9) var clouds_low: texture_2d<f32>;      // colortex9
@group(0) @binding(10) var clouds_low_data: texture_2d<f32>; // colortex10
@group(0) @binding(11) var hist: texture_2d<f32>;           // colortex11 (previous frame)
@group(0) @binding(12) var hist_data: texture_2d<f32>;      // colortex12 (previous frame)
@group(0) @binding(13) var prev_depth: texture_2d<f32>;     // colortex14: 1 - depth, half resolution

struct FullOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) @interpolate(flat) sky: vec3<f32>,
}

// ---------------------------------------------------------------- space_conversion.glsl
fn screen_to_view_space(screen_pos: vec3<f32>, handle_jitter: bool) -> vec3<f32> {
    var ndc_pos = vec3<f32>(2.0 * screen_pos.x - 1.0, 1.0 - 2.0 * screen_pos.y, screen_pos.z);
    if (handle_jitter) { ndc_pos = vec3<f32>(ndc_pos.xy - u.taa.xy * 0.66, ndc_pos.z); } // TAA without TAAU; 0 until milestone 5
    let h = u.proj_inv * vec4<f32>(ndc_pos, 1.0);
    return h.xyz / h.w;
}
fn view_to_scene_space(view_pos: vec3<f32>) -> vec3<f32> { return transform(u.view_inv, view_pos); }
fn reproject_scene_space(scene_pos: vec3<f32>) -> vec3<f32> {
    let camera_offset = u.cam.xyz - u.prev_cam.xyz;
    let previous_pos = transform(u.prev_view, scene_pos + camera_offset);
    let h = u.prev_proj * vec4<f32>(previous_pos, 1.0);
    let ndc = h.xyz / h.w;
    return vec3<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5, ndc.z);
}

// textureGather on a depth texture, by hand (the pack's `depth_max_4x4` gathers; a Depth view takes no filtering sampler here).
fn depth_gather(coord: vec2<f32>) -> vec4<f32> {
    let dims = vec2<i32>(textureDimensions(depth_tex));
    let i0 = vec2<i32>(floor(coord * vec2<f32>(dims) - 0.5));
    let m = dims - 1;
    let a = textureLoad(depth_tex, clamp(i0 + vec2<i32>(0, 1), vec2<i32>(0), m), 0);
    let b = textureLoad(depth_tex, clamp(i0 + vec2<i32>(1, 1), vec2<i32>(0), m), 0);
    let c = textureLoad(depth_tex, clamp(i0 + vec2<i32>(1, 0), vec2<i32>(0), m), 0);
    let d = textureLoad(depth_tex, clamp(i0, vec2<i32>(0), m), 0);
    return vec4<f32>(a, b, c, d);
}
fn max_of4(v: vec4<f32>) -> f32 { return max(max(v.x, v.y), max(v.z, v.w)); }
fn min_of4(v: vec4<f32>) -> f32 { return min(min(v.x, v.y), min(v.z, v.w)); }

fn checkerboard_offset_4x4(i: u32) -> vec2<i32> {
    var o = array<vec2<i32>, 16>(
        vec2<i32>(0, 0), vec2<i32>(2, 0), vec2<i32>(0, 2), vec2<i32>(2, 2),
        vec2<i32>(1, 1), vec2<i32>(3, 1), vec2<i32>(1, 3), vec2<i32>(3, 3),
        vec2<i32>(1, 0), vec2<i32>(3, 0), vec2<i32>(1, 2), vec2<i32>(3, 2),
        vec2<i32>(0, 1), vec2<i32>(2, 1), vec2<i32>(0, 3), vec2<i32>(2, 3)
    );
    return o[i];
}

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> FullOut {
    var out: FullOut;
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    out.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    out.sky = vec3<f32>(0.0);
    return out;
}

// ---------------------------------------------------------------- d1.vsh: sky colour for the clouds' ambient light
@vertex
fn vs_clouds(@builtin(vertex_index) i: u32) -> FullOut {
    var out: FullOut;
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    out.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);

    let sky_dir = normalize(vec3<f32>(0.0, 1.0, -0.8)); // don't point directly upwards to avoid the sun halo
    var sky = atmosphere_scattering(sky_dir, u.sun_rgb.xyz, sun_dir(), u.moon_rgb.xyz, moon_dir());
    sky = (tau * 1.13) * sky;
    sky = mix(sky, tau * u.weather_rgb.xyz, rain_strength());
    sky += aurora_amount() * 0.40 * mix(u.aur0.xyz, u.aur1.xyz, 0.25); // AURORA_CLOUD_LIGHTING
    out.sky = sky;
    return out;
}

// ---------------------------------------------------------------- p0_clouds_prep: cloud shadow map + cumulus coverage map
@fragment
fn fs_prep(in: FullOut) -> @location(0) vec4<f32> {
    let uv = in.pos.xy / cloud_shadow_res;
    let shadow = render_cloud_shadow_map(uv);   // CLOUD_SHADOWS (BLOCKY_CLOUDS is off)
    let coverage = render_clouds_cumulus_coverage_map(uv); // CLOUDS_CUMULUS_PRECOMPUTE_LOCAL_COVERAGE
    return vec4<f32>(shadow, coverage, 1.0);
}

// ---------------------------------------------------------------- d1_clouds: a quarter of the pixels, raymarched
struct CloudsOut {
    @location(0) clouds: vec4<f32>,       // colortex9
    @location(1) data: vec4<f32>,         // colortex10 (xy)
}

@fragment
fn fs_clouds(in: FullOut) -> CloudsOut {
    sun_color = u.sun_rgb.xyz;
    moon_color = u.moon_rgb.xyz;
    sky_color = in.sky;

    let texel = vec2<i32>(in.pos.xy);
    var clouds = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    var clouds_data = vec2<f32>(0.0);

    let checkerboard_area = u32(CLOUDS_TEMPORAL_UPSCALING * CLOUDS_TEMPORAL_UPSCALING);
    let checkerboard_pos = CLOUDS_TEMPORAL_UPSCALING * texel + checkerboard_offset_4x4(u.ints.x % checkerboard_area);
    let new_uv = vec2<f32>(checkerboard_pos) / u.res.xy; // * rcp(taau_render_scale), which is 1

    // Get maximum depth from area covered by this fragment (the pack's depth_max_4x4)
    let uv = in.pos.xy / u.low.xy;
    let px = u.res.zw;
    let depth_max = max(
        max(max_of4(depth_gather(uv + vec2<f32>(2.0 * px.x, 2.0 * px.y))), max_of4(depth_gather(uv + vec2<f32>(-2.0 * px.x, 2.0 * px.y)))),
        max(max_of4(depth_gather(uv + vec2<f32>(2.0 * px.x, -2.0 * px.y))), max_of4(depth_gather(uv + vec2<f32>(-2.0 * px.x, -2.0 * px.y))))
    );

    let view_pos = screen_to_view_space(vec3<f32>(new_uv, depth_max), false);

    let ray_origin = vec3<f32>(0.0, CLOUDS_SCALE * (eye_altitude() - SEA_LEVEL) + planet_radius, 0.0)
        + CLOUDS_SCALE * u.view_inv[3].xyz;
    let ray_dir = (u.view_inv * vec4<f32>(normalize(view_pos), 0.0)).xyz;

    let distance_to_terrain = select(length(view_pos) * CLOUDS_SCALE, -1.0, depth_max == 1.0);

    let clear_sky = atmosphere_scattering(ray_dir, sun_color, sun_dir(), moon_color, moon_dir());

    var dither = textureLoad(noisetex, checkerboard_pos & vec2<i32>(511), 0).b;
    dither = r1(i32(u.ints.x / checkerboard_area), dither);

    let result = draw_clouds(ray_origin, ray_dir, clear_sky, distance_to_terrain, dither);
    clouds = vec4<f32>(result.scattering.xyz, result.transmittance);
    // f16 targets: 1e6 / CLOUDS_SCALE overflows to inf and `mix` then makes NaN in d2; clamp to the largest finite half
    clouds_data.x = min(result.apparent_distance * rcp(CLOUDS_SCALE), 65000.0);
    clouds_data.y = result.scattering.w;

    // Crepuscular rays
    let crepuscular_rays = draw_crepuscular_rays(ray_dir, distance_to_terrain > 0.0, dither);
    clouds = clouds * crepuscular_rays.w;
    clouds = vec4<f32>(clouds.rgb + crepuscular_rays.xyz, clouds.w);

    // Aurora
    clouds = vec4<f32>(clouds.xyz + draw_aurora(ray_dir, dither) * clouds.w, clouds.w);

    return CloudsOut(clouds, vec4<f32>(clouds_data, 0.0, 0.0));
}

// ---------------------------------------------------------------- d2_clouds_upscaling
fn min5(a: vec4<f32>, b: vec4<f32>, c: vec4<f32>, d: vec4<f32>, e: vec4<f32>) -> vec4<f32> { return min(a, min(b, min(c, min(d, e)))); }
fn max5(a: vec4<f32>, b: vec4<f32>, c: vec4<f32>, d: vec4<f32>, e: vec4<f32>) -> vec4<f32> { return max(a, max(b, max(c, max(d, e)))); }

// textureGather(s, coord).x minimum over the 4 gathers at +-2 pixels (the pack's texture_min_4x4 on colortex12)
fn texture_min_4x4(coord: vec2<f32>) -> f32 {
    let pixel_size = 1.0 / vec2<f32>(textureDimensions(hist_data, 0));
    let s0 = textureGather(0, hist_data, samp_clamp, coord + vec2<f32>(2.0 * pixel_size.x, 2.0 * pixel_size.y));
    let s1 = textureGather(0, hist_data, samp_clamp, coord + vec2<f32>(-2.0 * pixel_size.x, 2.0 * pixel_size.y));
    let s2 = textureGather(0, hist_data, samp_clamp, coord + vec2<f32>(2.0 * pixel_size.x, -2.0 * pixel_size.y));
    let s3 = textureGather(0, hist_data, samp_clamp, coord + vec2<f32>(-2.0 * pixel_size.x, -2.0 * pixel_size.y));
    return min(min(min_of4(s0), min_of4(s1)), min(min_of4(s2), min_of4(s3)));
}

fn reproject_clouds(uv: vec2<f32>, distance_to_cloud: f32) -> vec3<f32> {
    let clouds_cumulus_wind_angle = CLOUDS_CUMULUS_WIND_ANGLE * degree;
    let clouds_altocumulus_wind_angle = CLOUDS_ALTOCUMULUS_WIND_ANGLE * degree;
    let clouds_cirrus_wind_angle = CLOUDS_CIRRUS_WIND_ANGLE * degree;
    let clouds_cumulus_velocity = CLOUDS_CUMULUS_WIND_SPEED * vec3<f32>(cos(clouds_cumulus_wind_angle), 0.0, sin(clouds_cumulus_wind_angle));
    let clouds_altocumulus_velocity = CLOUDS_ALTOCUMULUS_WIND_SPEED * vec3<f32>(cos(clouds_altocumulus_wind_angle), 0.0, sin(clouds_altocumulus_wind_angle));
    let clouds_cirrus_velocity = CLOUDS_CIRRUS_WIND_SPEED * vec3<f32>(cos(clouds_cirrus_wind_angle), 0.0, sin(clouds_cirrus_wind_angle));

    let view_pos = screen_to_view_space(vec3<f32>(uv, 1.0), false);
    let scene_pos = view_to_scene_space(view_pos);
    let world_dir = normalize(scene_pos - u.view_inv[3].xyz);

    var cloud_pos = world_dir * distance_to_cloud + u.view_inv[3].xyz;

    // Work out which layer this cloud belongs to
    var velocity: vec3<f32>;
    let air_cloud_pos = vec3<f32>(0.0, CLOUDS_SCALE * (eye_altitude() - SEA_LEVEL) + planet_radius, 0.0) + CLOUDS_SCALE * cloud_pos;
    let r = length(air_cloud_pos);

    if (r < clouds_altocumulus_radius) {
        velocity = clouds_cumulus_velocity;
    } else if (r < clouds_cirrus_radius) {
        velocity = clouds_altocumulus_velocity;
    } else {
        velocity = clouds_cirrus_velocity;
    }

    cloud_pos += velocity * u.t.y * rcp(CLOUDS_SCALE) * u.flags.x;
    return reproject_scene_space(cloud_pos);
}

struct UpscaleOut {
    @location(0) clouds_history: vec4<f32>, // colortex11
    @location(1) clouds_data: vec4<f32>,    // colortex12 (xyz)
}

@fragment
fn fs_upscale(in: FullOut) -> UpscaleOut {
    let checkerboard_area = u32(CLOUDS_TEMPORAL_UPSCALING * CLOUDS_TEMPORAL_UPSCALING);

    let dst_texel = vec2<i32>(in.pos.xy);
    let uv = in.pos.xy / u.res.xy;
    let low_dims = vec2<i32>(textureDimensions(clouds_low, 0));
    let src_texel = clamp(dst_texel / CLOUDS_TEMPORAL_UPSCALING, vec2<i32>(0), low_dims - 1);

    var current = textureLoad(clouds_low, src_texel, 0);
    let current_data = textureLoad(clouds_low_data, src_texel, 0).xy;
    let depth = textureLoad(depth_tex, dst_texel, 0);
    // (fix_hand_depth: mc-rs has no hand pass, so depth < hand_depth is just near geometry)

    var apparent_distance = current_data.x;
    var ambient_scattering = current_data.y;

    // Find the closest cloud distance between the current frame and a 4x4 area of the previous frame
    let closest_distance = min(apparent_distance, texture_min_4x4(uv));

    // Early exit if clouds covered by terrain
    if (depth != 1.0) {
        let view_distance_squared = length_squared(screen_to_view_space(vec3<f32>(uv, depth), true));
        if (view_distance_squared < sqr(closest_distance)) {
            return UpscaleOut(current, vec4<f32>(65000.0, 0.0, 0.0, 0.0)); // apparent distance, pixel age, ambient scattering
        }
    }

    // Reproject clouds
    let previous_uv = reproject_clouds(uv, closest_distance).xy;

    var history = max(catmull_rom_filter_fast(hist, previous_uv, 0.5), vec4<f32>(0.0));
    let history_data = textureSampleLevel(hist_data, samp_clamp, previous_uv, 0.0).xyz;

    // Depth at the previous position
    let history_depth = 1.0 - max_of4(textureGather(0, prev_depth, samp_clamp, previous_uv));

    // Get distance to terrain in the previous frame
    let distance_to_terrain_squared = length_squared(screen_to_view_space(vec3<f32>(previous_uv, history_depth), true));

    // Work out whether the history should be invalidated
    var disocclusion = outside01(previous_uv);
    disocclusion = disocclusion || (history_depth < 1.0 && distance_to_terrain_squared < sqr(closest_distance));
    disocclusion = disocclusion || history_depth == 0.0; // Signals hand
    disocclusion = disocclusion || any(history != history); // isnan (WGSL has no isnan; a fast-math compiler may fold this away)
    disocclusion = disocclusion || u.flags.y > 0.5; // world_age_changed

    // Replace history if a disocclusion was detected
    if (disocclusion) {
        history = current;
        history.z = ambient_scattering;
    }

    // Perform neighbourhood clamping when moving quickly relative to the clouds
    let velocity = rcp(sqr(u.t.y)) * length_squared(u.cam.xyz - u.prev_cam.xyz);
    var velocity_factor = 75.0 * velocity / max(sqr(closest_distance), eps);
    velocity_factor = velocity_factor / (velocity_factor + 1.0);

    if (velocity_factor > 0.1) {
        // Fetch 3x3 neighborhood
        let a = textureLoad(clouds_low, src_texel + vec2<i32>(-1, -1), 0);
        let b = textureLoad(clouds_low, src_texel + vec2<i32>(0, -1), 0);
        let c = textureLoad(clouds_low, src_texel + vec2<i32>(1, -1), 0);
        let d = textureLoad(clouds_low, src_texel + vec2<i32>(-1, 0), 0);
        let f = textureLoad(clouds_low, src_texel + vec2<i32>(1, 0), 0);
        let g = textureLoad(clouds_low, src_texel + vec2<i32>(-1, 1), 0);
        let h = textureLoad(clouds_low, src_texel + vec2<i32>(0, 1), 0);
        let i = textureLoad(clouds_low, src_texel + vec2<i32>(1, 1), 0);
        let e = current;

        // Soft minimum and maximum ("Hybrid Reconstruction Antialiasing")
        var aabb_min = min5(b, d, e, f, h);
        aabb_min += min5(aabb_min, a, c, g, i);
        aabb_min *= 0.5;
        aabb_min = max(aabb_min, vec4<f32>(0.0));

        var aabb_max = max5(b, d, e, f, h);
        aabb_max += max5(aabb_max, a, c, g, i);
        aabb_max *= 0.5;
        aabb_max = max(aabb_max, vec4<f32>(0.0));

        history = mix(history, clamp(history, aabb_min, aabb_max), velocity_factor);
    }

    // Get previous pixel age and apparent distance
    var apparent_distance_history = history_data.x;
    apparent_distance_history = select(apparent_distance_history, apparent_distance, disocclusion);

    // Checkerboard upscaling
    let offset_0 = dst_texel % vec2<i32>(CLOUDS_TEMPORAL_UPSCALING);
    let offset_1 = checkerboard_offset_4x4(u.ints.x % checkerboard_area);
    if (any(offset_0 != offset_1) && !disocclusion) {
        current = history;
        apparent_distance = min(apparent_distance, apparent_distance_history);
        ambient_scattering = history_data.z;
    }

    var pixel_age = max0(history_data.y) * f32(!disocclusion);
    var history_weight = 1.0 - rcp(max(pixel_age - f32(checkerboard_area), 1.0));

    // Offcenter rejection (#ifndef TAAU)
    let pixel_center_offset = 1.0 - abs(fract(previous_uv * u.res.xy) * 2.0 - 1.0);
    var offcenter_rejection = sqrt(pixel_center_offset.x * pixel_center_offset.y);
    offcenter_rejection = mix(1.0, offcenter_rejection, history_weight);
    history_weight *= offcenter_rejection;

    let clouds_history = max(mix(current, history, history_weight), vec4<f32>(0.0));
    pixel_age += 1.0; // ++pixel_age
    return UpscaleOut(
        clouds_history,
        vec4<f32>(
            mix(apparent_distance, apparent_distance_history, history_weight),
            min(pixel_age, CLOUDS_ACCUMULATION_LIMIT),
            mix(ambient_scattering, history_data.z, history_weight),
            0.0
        )
    );
}

// ---------------------------------------------------------------- helpers that are not Photon programs
// What d3_ao leaves in colortex14.x for the next frame's d2: 1 - depth at half resolution.
@fragment
fn fs_depth_history(in: FullOut) -> @location(0) vec4<f32> {
    let dims = vec2<i32>(textureDimensions(depth_tex));
    let d = textureLoad(depth_tex, min(vec2<i32>(in.pos.xy) * 2, dims - 1), 0);
    return vec4<f32>(1.0 - d, 0.0, 0.0, 0.0);
}

// One level of the mip chain of colortex11 (a bilinear tap at the texel corner is the 2x2 box average).
@fragment
fn fs_mip(in: FullOut) -> @location(0) vec4<f32> {
    let uv = in.pos.xy / vec2<f32>(textureDimensions(hist, 0) / 2u);
    return textureSampleLevel(hist, samp_clamp, uv, 0.0);
}
