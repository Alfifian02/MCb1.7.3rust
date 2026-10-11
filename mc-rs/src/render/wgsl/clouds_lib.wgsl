// Port of Photon (SixthSurge) `include/sky/clouds/*`, `include/sky/{atmosphere,aurora,crepuscular_rays}.glsl`,
// `include/lighting/cloud_shadows.glsl` and the helpers they use. Overworld, `BLOCKY_CLOUDS` off, no LoD mod.
// Joined with `clouds_passes.wgsl` (the uniform block, bindings and entry points) by `render::clouds`.
// Axes: scene space is world - eye, y up; sea level is b1.7.3's 64 (the pack's `SEA_LEVEL` is 63).

// ---------------------------------------------------------------- constants (settings.glsl defaults)
override PROGRAM_PREPARE: bool = false;   // p0: no worley detail, local coverage computed instead of read
override PROGRAM_DEFERRED0: bool = false; // d0 sky map: half the primary steps, no light-ray dither (milestone 2)

const pi: f32 = 3.14159265358979;
const tau: f32 = 6.28318530717959;
const half_pi: f32 = 1.5707963267949;
const rcp_pi: f32 = 0.3183098861838;
const degree: f32 = 0.0174532925199433;
const golden_ratio: f32 = 1.61803398874989;
const eps: f32 = 1e-6;
const isotropic_phase: f32 = 0.0795774715459477;

const SEA_LEVEL: f32 = 64.0;
const CLOUDS_SCALE: f32 = 10.0;
const CLOUDS_TEMPORAL_UPSCALING: i32 = 4;
const CLOUDS_AERIAL_PERSPECTIVE_BOOST: u32 = 1u;
const CLOUDS_ACCUMULATION_LIMIT: f32 = 20.0;
const CLOUD_SHADOWS_INTENSITY: f32 = 0.80;

const CLOUDS_CUMULUS_PRIMARY_STEPS_H: u32 = 40u;
const CLOUDS_CUMULUS_PRIMARY_STEPS_Z: u32 = 20u;
const CLOUDS_CUMULUS_LIGHTING_STEPS: u32 = 6u;
const CLOUDS_CUMULUS_AMBIENT_STEPS: u32 = 2u;
const CLOUDS_CUMULUS_ALTITUDE: f32 = 1200.0;
const CLOUDS_CUMULUS_THICKNESS: f32 = 1.00;
const CLOUDS_CUMULUS_DENSITY: f32 = 1.00;
const CLOUDS_CUMULUS_COVERAGE: f32 = 1.00;
const CLOUDS_CUMULUS_SIZE: f32 = 1.00;
const CLOUDS_CUMULUS_DETAIL_STRENGTH: f32 = 1.00;
const CLOUDS_CUMULUS_WIND_SPEED: f32 = 25.0;
const CLOUDS_CUMULUS_WIND_ANGLE: f32 = 30.0;

const CLOUDS_CUMULUS_CONGESTUS_PRIMARY_STEPS: u32 = 20u;
const CLOUDS_CUMULUS_CONGESTUS_LIGHTING_STEPS: u32 = 6u;
const CLOUDS_CUMULUS_CONGESTUS_AMBIENT_STEPS: u32 = 2u;
const CLOUDS_CUMULUS_CONGESTUS_ALTITUDE: f32 = 400.0;
const CLOUDS_CUMULUS_CONGESTUS_THICKNESS: f32 = 32.00;
const CLOUDS_CUMULUS_CONGESTUS_COVERAGE: f32 = 1.00;
const CLOUDS_CUMULUS_CONGESTUS_SIZE: f32 = 1.00;
const CLOUDS_CUMULUS_CONGESTUS_DETAIL_STRENGTH: f32 = 1.00;

const CLOUDS_ALTOCUMULUS_PRIMARY_STEPS_H: u32 = 12u;
const CLOUDS_ALTOCUMULUS_PRIMARY_STEPS_Z: u32 = 6u;
const CLOUDS_ALTOCUMULUS_LIGHTING_STEPS: u32 = 4u;
const CLOUDS_ALTOCUMULUS_AMBIENT_STEPS: u32 = 2u;
const CLOUDS_ALTOCUMULUS_ALTITUDE: f32 = 3200.0;
const CLOUDS_ALTOCUMULUS_THICKNESS: f32 = 0.15;
const CLOUDS_ALTOCUMULUS_DENSITY: f32 = 0.10;
const CLOUDS_ALTOCUMULUS_SIZE: f32 = 1.00;
const CLOUDS_ALTOCUMULUS_DETAIL_STRENGTH: f32 = 1.50;
const CLOUDS_ALTOCUMULUS_WIND_SPEED: f32 = 25.0;
const CLOUDS_ALTOCUMULUS_WIND_ANGLE: f32 = 60.0;

const CLOUDS_CIRRUS_LIGHTING_STEPS: u32 = 6u;
const CLOUDS_CIRRUS_ALTITUDE: f32 = 6000.0;
const CLOUDS_CIRRUS_DENSITY: f32 = 0.50;
const CLOUDS_CIRRUS_SIZE: f32 = 1.00;
const CLOUDS_CIRRUS_CURL_STRENGTH: f32 = 0.80;
const CLOUDS_CIRRUS_THICKNESS: f32 = 1500.0;
const CLOUDS_CIRROCUMULUS_DENSITY: f32 = 1.00;
const CLOUDS_CIRROCUMULUS_SIZE: f32 = 1.00;
const CLOUDS_CIRROCUMULUS_DETAIL_STRENGTH: f32 = 1.00;
const CLOUDS_CIRROCUMULUS_CURL_STRENGTH: f32 = 1.00;
const CLOUDS_CIRRUS_WIND_SPEED: f32 = 40.0;
const CLOUDS_CIRRUS_WIND_ANGLE: f32 = 90.0;

const CREPUSCULAR_RAYS_INTENSITY: f32 = 1.00;
const CREPUSCULAR_RAYS_STEPS_HORIZON: u32 = 20u;
const CREPUSCULAR_RAYS_STEPS_ZENITH: u32 = 4u;
const AURORA_FREQUENCY: f32 = 1.00;
const AURORA_BRIGHTNESS: f32 = 1.00;

const ATMOSPHERE_SATURATION_BOOST_INTENSITY: f32 = 1.0;
const ATMOSPHERE_RAIN_DESATURATION_INTENSITY: f32 = 0.0; // `ATMOSPHERE_RAIN_DESATURATION` is commented out in settings.glsl
const SUN_ANGULAR_RADIUS: f32 = 2.0;
const MOON_ANGULAR_RADIUS: f32 = 3.0;

// ---------------------------------------------------------------- constants (constants.glsl, atmosphere.glsl)
const planet_radius: f32 = 6371000.0;
const clouds_cumulus_radius: f32 = planet_radius + CLOUDS_CUMULUS_ALTITUDE;
const clouds_cumulus_thickness: f32 = CLOUDS_CUMULUS_ALTITUDE * CLOUDS_CUMULUS_THICKNESS;
const clouds_cumulus_top_radius: f32 = clouds_cumulus_radius + clouds_cumulus_thickness;
const clouds_altocumulus_radius: f32 = planet_radius + CLOUDS_ALTOCUMULUS_ALTITUDE;
const clouds_altocumulus_thickness: f32 = CLOUDS_ALTOCUMULUS_ALTITUDE * CLOUDS_ALTOCUMULUS_THICKNESS;
const clouds_altocumulus_top_radius: f32 = clouds_altocumulus_radius + clouds_altocumulus_thickness;
const clouds_cirrus_radius: f32 = planet_radius + CLOUDS_CIRRUS_ALTITUDE;
const clouds_cirrus_extinction_coeff: f32 = 0.15;
const clouds_cirrus_scattering_coeff: f32 = clouds_cirrus_extinction_coeff;
const clouds_noctilucent_altitude: f32 = 80000.0;
const clouds_noctilucent_radius: f32 = planet_radius + clouds_noctilucent_altitude;

const clouds_cumulus_congestus_radius: f32 = planet_radius + CLOUDS_CUMULUS_CONGESTUS_ALTITUDE;
const clouds_cumulus_congestus_thickness: f32 = CLOUDS_CUMULUS_CONGESTUS_ALTITUDE * CLOUDS_CUMULUS_CONGESTUS_THICKNESS;
const clouds_cumulus_congestus_top_radius: f32 = clouds_cumulus_congestus_radius + clouds_cumulus_congestus_thickness;
const clouds_cumulus_congestus_distance: f32 = 30000.0;
const clouds_cumulus_congestus_end_distance: f32 = 50000.0;
const clouds_cumulus_congestus_extinction_coeff: f32 = 0.08;

const sunlight_color: vec3<f32> = vec3<f32>(1.051, 0.985, 0.940);
const sun_angular_radius: f32 = SUN_ANGULAR_RADIUS * degree;
const moon_angular_radius: f32 = MOON_ANGULAR_RADIUS * degree;
const min_mu_s: f32 = -0.35;
const atmosphere_inner_radius: f32 = planet_radius - 1000.0;
const atmosphere_outer_radius: f32 = planet_radius + 110000.0;
const atmosphere_thickness: f32 = atmosphere_outer_radius - atmosphere_inner_radius;
const atmosphere_inner_radius_sq: f32 = atmosphere_inner_radius * atmosphere_inner_radius;
const atmosphere_outer_radius_sq: f32 = atmosphere_outer_radius * atmosphere_outer_radius;
const air_scale_heights: vec2<f32> = vec2<f32>(8400.0, 1250.0);
// Rec. 709 coefficients times `rec709_to_rec2020`, worked out once (numpy) instead of a runtime matrix product.
const air_rayleigh_coefficient: vec3<f32> = vec3<f32>(1.23285173e-05, 1.63879614e-05, 3.81481644e-05);
const air_mie_coefficient: vec3<f32> = vec3<f32>(1.72738620e-06, 1.80419279e-06, 1.94142336e-06);
const air_ozone_coefficient: vec3<f32> = vec3<f32>(9.56376602e-07, 1.26710534e-06, 1.78072697e-07);
const luminance_weights_rec2020: vec3<f32> = vec3<f32>(0.2627, 0.6780, 0.0593);

// ---------------------------------------------------------------- helpers (global.glsl, fast_math, geometry, random, phase)
fn sqr(x: f32) -> f32 { return x * x; }
fn cube(x: f32) -> f32 { return x * x * x; }
fn pow4(x: f32) -> f32 { return sqr(sqr(x)); }
fn pow8(x: f32) -> f32 { return sqr(pow4(x)); }
fn pow1d5(x: f32) -> f32 { return x * sqrt(x); }
fn rcp(x: f32) -> f32 { return 1.0 / x; }
fn clamp01(x: f32) -> f32 { return clamp(x, 0.0, 1.0); }
fn max0(x: f32) -> f32 { return max(x, 0.0); }
fn length_squared2(v: vec2<f32>) -> f32 { return dot(v, v); }
fn length_squared(v: vec3<f32>) -> f32 { return dot(v, v); }
fn rcp_length2(v: vec2<f32>) -> f32 { return inverseSqrt(dot(v, v)); }
fn outside01(v: vec2<f32>) -> bool { return any(clamp(v, vec2<f32>(0.0), vec2<f32>(1.0)) != v); }
fn linear_step(edge0: f32, edge1: f32, x: f32) -> f32 { return clamp01((x - edge0) / (edge1 - edge0)); }
fn dampen(x_in: f32) -> f32 { let x = clamp01(x_in); return x * (2.0 - x); }
fn lift(x: f32, amount: f32) -> f32 { return (x + x * amount) / (1.0 + x * amount); }
fn cubic_smooth(x: f32) -> f32 { return sqr(x) * (3.0 - 2.0 * x); }
fn get_uv_from_unit_range(values: f32, res: f32) -> f32 { return values * (1.0 - 1.0 / res) + (0.5 / res); }
// WGSL leaves smoothstep(low >= high, x) undefined; GLSL implementations (and the pack) evaluate the formula.
fn sstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp01((x - e0) / (e1 - e0));
    return t * t * (3.0 - 2.0 * t);
}
fn pulse(x_in: f32, center: f32, width: f32) -> f32 {
    let x = abs(x_in - center) / width;
    return select(1.0 - cubic_smooth(x), 0.0, x > 1.0);
}
fn transform(m: mat4x4<f32>, pos: vec3<f32>) -> vec3<f32> {
    return (m * vec4<f32>(pos, 1.0)).xyz;
}

fn hash2_v2(p: vec2<f32>) -> vec2<f32> {
    var p3 = fract(vec3<f32>(p.xyx) * vec3<f32>(0.1031, 0.1030, 0.0973));
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.xx + p3.yz) * p3.zy);
}
fn hash2_v3(p: vec3<f32>) -> vec2<f32> {
    var p3 = fract(p * vec3<f32>(0.1031, 0.1030, 0.0973));
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.xx + p3.yz) * p3.zy);
}
fn r1(n: i32, seed: f32) -> f32 {
    let alpha = 1.0 / 1.6180339887;
    return fract(seed + f32(n) * alpha);
}

fn intersect_sphere_mu(mu: f32, r: f32, sphere_radius: f32) -> vec2<f32> {
    let discriminant = r * r * (mu * mu - 1.0) + sqr(sphere_radius);
    if (discriminant < 0.0) { return vec2<f32>(-1.0); }
    let d = sqrt(discriminant);
    return -r * mu + vec2<f32>(-d, d);
}
fn intersect_sphere(ray_origin: vec3<f32>, ray_dir: vec3<f32>, sphere_radius: f32) -> vec2<f32> {
    let b = dot(ray_origin, ray_dir);
    let discriminant = sqr(b) - dot(ray_origin, ray_origin) + sqr(sphere_radius);
    if (discriminant < 0.0) { return vec2<f32>(-1.0); }
    let d = sqrt(discriminant);
    return -b + vec2<f32>(-d, d);
}
fn shell_from(inner_d: vec2<f32>, outer_d: vec2<f32>) -> vec2<f32> {
    let inner_hit = inner_d.y >= 0.0;
    let outer_hit = outer_d.y >= 0.0;
    if (!outer_hit) { return vec2<f32>(-1.0); }
    var dists: vec2<f32>;
    dists.x = select(max0(outer_d.x), inner_d.y, inner_hit && inner_d.x < 0.0);
    dists.y = select(outer_d.y, inner_d.x, inner_hit && inner_d.x > 0.0);
    return dists;
}
fn intersect_spherical_shell_mu(mu: f32, r: f32, inner_r: f32, outer_r: f32) -> vec2<f32> {
    return shell_from(intersect_sphere_mu(mu, r, inner_r), intersect_sphere_mu(mu, r, outer_r));
}
fn intersect_spherical_shell(ray_origin: vec3<f32>, ray_dir: vec3<f32>, inner_r: f32, outer_r: f32) -> vec2<f32> {
    return shell_from(intersect_sphere(ray_origin, ray_dir, inner_r), intersect_sphere(ray_origin, ray_dir, outer_r));
}
fn intersect_cylindrical_shell(ray_origin: vec3<f32>, ray_dir: vec3<f32>, inner_r: f32, outer_r: f32) -> vec2<f32> {
    let len_o = length(ray_origin.xz);
    let rlen_d = rcp_length2(ray_dir.xz);
    return vec2<f32>((inner_r - len_o) * rlen_d, (outer_r - len_o) * rlen_d);
}

fn henyey_greenstein_phase(nu: f32, g: f32) -> f32 {
    let gg = g * g;
    return (isotropic_phase - isotropic_phase * gg) / pow1d5(1.0 + gg - 2.0 * g * nu);
}
fn klein_nishina_phase(nu: f32, e: f32) -> f32 {
    return e / (tau * (e - e * nu + 1.0) * log(2.0 * e + 1.0));
}
fn nvidia_phase_area(nu: f32, g: f32, a: f32, radius: f32) -> f32 {
    let radius_eff = max(radius, eps);
    let cosr = cos(radius_eff);
    let sinr = sin(radius_eff);
    var mu = nu * cosr + sqrt(max(0.0, 1.0 - nu * nu)) * sinr;
    if (nu > cosr) { mu = 1.0; }
    let gg = g * g;
    return ((1.0 - gg) * (1.0 + a * mu * mu))
        / (pi * pow1d5(1.0 + gg - (2.0 * g * mu)) * 4.0 * (1.0 + (a * (1.0 + 2.0 * gg)) / 3.0));
}

// ---------------------------------------------------------------- globals the pack reads from uniforms
// (`u` and the textures are declared in clouds_passes.wgsl)
var<private> sun_color: vec3<f32>;   // flat out of d1.vsh
var<private> moon_color: vec3<f32>;
var<private> sky_color: vec3<f32>;

fn rain_strength() -> f32 { return u.moon.w; }
fn sun_dir() -> vec3<f32> { return u.sun.xyz; }
fn moon_dir() -> vec3<f32> { return u.moon.xyz; }
fn light_dir() -> vec3<f32> { return u.light.xyz; }
fn camera_position() -> vec3<f32> { return u.cam.xyz; }
fn eye_altitude() -> f32 { return u.cam.w; }
fn world_age() -> f32 { return u.t.x; }
fn day_factor() -> f32 { return u.t.z; }
fn time_sunrise() -> f32 { return u.time.x; }
fn time_noon() -> f32 { return u.time.y; }
fn time_sunset() -> f32 { return u.time.z; }

fn cp_congestus_blend() -> f32 { return u.cp_a.x; }
fn cp_l0_coverage() -> vec2<f32> { return u.cp_a.yz; }
fn cp_l0_detail_weights() -> vec2<f32> { return vec2<f32>(u.cp_a.w, u.cp_b.x); }
fn cp_l0_edge_sharpening() -> vec2<f32> { return u.cp_b.yz; }
fn cp_l0_altitude_scale() -> f32 { return u.cp_b.w; }
fn cp_l0_stratus_blend() -> f32 { return u.cp_c.x; }
fn cp_l0_extinction() -> f32 { return u.cp_c.y; }
fn cp_l0_scattering() -> f32 { return u.cp_c.z; }
fn cp_l0_shadow() -> f32 { return u.cp_c.w; }
fn cp_l1_coverage() -> vec2<f32> { return u.cp_d.xy; }
fn cp_l1_stratus_blend() -> f32 { return u.cp_d.z; }
fn cp_cirrus() -> f32 { return u.cp_e.x; }
fn cp_cirrocumulus() -> f32 { return u.cp_e.y; }
fn cp_noctilucent() -> f32 { return u.cp_e.z; }
fn cp_crepuscular() -> f32 { return u.cp_e.w; }

fn noise_tex(uv: vec2<f32>) -> vec4<f32> { return textureSampleLevel(noisetex, samp_repeat, uv, 0.0); }

// ---------------------------------------------------------------- atmosphere.glsl
fn atmosphere_mie_phase_sun(nu: f32) -> vec3<f32> {
    let r = nvidia_phase_area(nu, 0.85, 1.0, sun_angular_radius);
    let g = nvidia_phase_area(nu, 0.86, 1.0, sun_angular_radius);
    let b = nvidia_phase_area(nu, 0.87, 1.0, sun_angular_radius);
    return vec3<f32>(r, g, b) * 0.2; // artistic scale
}
fn atmosphere_mie_phase_moon(nu: f32) -> vec3<f32> {
    let moon_intensity = 1.02;
    var t = f32(u.ints.y) / 4.0; // moonPhase
    t = select(t, 2.0 - t, t > 1.0);
    t = sqr(1.0 - t) * 0.04 + 0.96; // small phase difference
    let r = nvidia_phase_area(nu, 0.895 * t, 1.0, 2.0 * moon_angular_radius);
    let g = nvidia_phase_area(nu, 0.90 * t, 1.0, 2.0 * moon_angular_radius);
    let b = nvidia_phase_area(nu, 0.905 * t, 1.0, 2.0 * moon_angular_radius);
    return vec3<f32>(r, g, b) * moon_intensity;
}
fn atmosphere_post_processing(atmosphere: vec3<f32>) -> vec3<f32> {
    return mix(
        vec3<f32>(dot(atmosphere, luminance_weights_rec2020)),
        atmosphere,
        (1.0 + ATMOSPHERE_SATURATION_BOOST_INTENSITY * u.t.w) * (1.0 - rain_strength() * ATMOSPHERE_RAIN_DESATURATION_INTENSITY)
    );
}
// Samples the scattering LUT for sun and moon together (the pack's 6-argument overload).
fn atmosphere_scattering(ray_dir: vec3<f32>, sun_col: vec3<f32>, sun_d: vec3<f32>, moon_col: vec3<f32>, moon_d: vec3<f32>) -> vec3<f32> {
    var mu = ray_dir.y;
    let nu_sun = dot(ray_dir, sun_d);
    let nu_moon = dot(ray_dir, moon_d);
    let mu_sun = sun_d.y;
    let mu_moon = moon_d.y;

    // no SKY_GROUND
    let horizon_mu = mix(-0.01, 0.03, clamp01(smoothstep(-0.05, 0.1, mu_sun) + smoothstep(0.05, 0.1, mu_moon)));
    mu = max(mu, horizon_mu);

    // Improved mapping for nu from Spectrum by Zombye
    var half_range_nu = sqrt((1.0 - mu * mu) * (1.0 - mu_sun * mu_sun));
    var nu_min = mu * mu_sun - half_range_nu;
    var nu_max = mu * mu_sun + half_range_nu;
    var u_nu_sun = select((nu_sun - nu_min) / (nu_max - nu_min), nu_min, nu_min == nu_max);
    u_nu_sun = get_uv_from_unit_range(u_nu_sun, 16.0);

    half_range_nu = sqrt((1.0 - mu * mu) * (1.0 - mu_moon * mu_moon));
    nu_min = mu * mu_moon - half_range_nu;
    nu_max = mu * mu_moon + half_range_nu;
    var u_nu_moon = select((nu_moon - nu_min) / (nu_max - nu_min), nu_min, nu_min == nu_max);
    u_nu_moon = get_uv_from_unit_range(u_nu_moon, 16.0);

    // Stretch the sky near the horizon upwards
    if (mu > 0.0) { mu *= sqrt(sqrt(mu)); }

    // Mapping for mu
    let r = planet_radius;
    let H = sqrt(atmosphere_outer_radius_sq - atmosphere_inner_radius_sq);
    let rho = sqrt(max0(planet_radius * planet_radius - atmosphere_inner_radius_sq));
    let rmu = r * mu;
    let discriminant = rmu * rmu - r * r + atmosphere_inner_radius_sq;

    var u_mu: f32;
    if (mu < 0.0 && discriminant >= 0.0) {
        let d = -rmu - sqrt(max0(discriminant));
        let d_min = r - atmosphere_inner_radius;
        let d_max = rho;
        u_mu = select((d - d_min) / (d_max - d_min), 0.0, d_max == d_min);
        u_mu = get_uv_from_unit_range(u_mu, 32.0); // scattering_res.y / 2
        u_mu = 0.5 - 0.5 * u_mu;
    } else {
        let d = -rmu + sqrt(discriminant + H * H);
        let d_min = atmosphere_outer_radius - r;
        let d_max = rho + H;
        u_mu = (d - d_min) / (d_max - d_min);
        u_mu = get_uv_from_unit_range(u_mu, 32.0);
        u_mu = 0.5 + 0.5 * u_mu;
    }

    // Mapping for mu_s
    let d_min = atmosphere_thickness;
    let d_max = H;
    let D = intersect_sphere_mu(min_mu_s, atmosphere_inner_radius, atmosphere_outer_radius).y;
    let A = (D - d_min) / (d_max - d_min);

    var d = intersect_sphere_mu(mu_sun, atmosphere_inner_radius, atmosphere_outer_radius).y;
    var a = (d - d_min) / (d_max - d_min);
    let u_mu_sun = get_uv_from_unit_range(max0(1.0 - a / A) / (1.0 + a), 32.0);

    d = intersect_sphere_mu(mu_moon, atmosphere_inner_radius, atmosphere_outer_radius).y;
    a = (d - d_min) / (d_max - d_min);
    let u_mu_moon = get_uv_from_unit_range(max0(1.0 - a / A) / (1.0 + a), 32.0);

    let uv_sc = vec3<f32>(u_nu_sun * 0.5, u_mu, u_mu_sun);
    let uv_sm = vec3<f32>(u_nu_sun * 0.5 + 0.5, u_mu, u_mu_sun);
    let uv_mc = vec3<f32>(u_nu_moon * 0.5, u_mu, u_mu_moon);
    let uv_mm = vec3<f32>(u_nu_moon * 0.5 + 0.5, u_mu, u_mu_moon);

    let scattering_sc = textureSampleLevel(atmosphere_lut, samp_clamp, uv_sc, 0.0).rgb;
    let scattering_sm = textureSampleLevel(atmosphere_lut, samp_clamp, uv_sm, 0.0).rgb;
    let scattering_mc = textureSampleLevel(atmosphere_lut, samp_clamp, uv_mc, 0.0).rgb;
    let scattering_mm = textureSampleLevel(atmosphere_lut, samp_clamp, uv_mm, 0.0).rgb;

    let atmosphere = (scattering_sc + scattering_sm * atmosphere_mie_phase_sun(nu_sun)) * sun_col
        + (scattering_mc + scattering_mm * atmosphere_mie_phase_moon(nu_moon)) * moon_col;
    return atmosphere_post_processing(atmosphere);
}

// Source: http://www.thetenthplanet.de/archives/4519 (the pack's branch without `ATMOSPHERE_TRANSMITTANCE_LUT`)
fn chapman_function_approx(x: f32, cos_theta: f32) -> f32 {
    let c = sqrt(half_pi * x);
    if (cos_theta >= 0.0) {
        return c / ((c - 1.0) * cos_theta + 1.0);
    } else {
        let sin_theta = sqrt(clamp01(1.0 - sqr(cos_theta)));
        return c / ((c - 1.0) * cos_theta - 1.0) + 2.0 * c * exp(x - x * sin_theta) * sqrt(sin_theta);
    }
}
fn atmosphere_transmittance_mu(mu: f32, r: f32) -> vec3<f32> {
    if (intersect_sphere_mu(mu, max(r, planet_radius + 10.0), planet_radius).x >= 0.0) {
        return vec3<f32>(0.0);
    }
    let rcp_scale_heights = 1.0 / air_scale_heights;
    let scaled_planet_radius = planet_radius * rcp_scale_heights;
    let density = exp(r * -rcp_scale_heights + scaled_planet_radius);
    var airmass = air_scale_heights * density;
    airmass.x *= chapman_function_approx(r * rcp_scale_heights.x, mu);
    airmass.y *= chapman_function_approx(r * rcp_scale_heights.y, mu);
    // exp(-air_extinction_coefficients * airmass.xyx)
    let od = air_rayleigh_coefficient * airmass.x + air_mie_coefficient * airmass.y + air_ozone_coefficient * airmass.x;
    return clamp(exp(-od), vec3<f32>(0.0), vec3<f32>(1.0));
}
fn atmosphere_transmittance(ray_origin: vec3<f32>, ray_dir: vec3<f32>) -> vec3<f32> {
    let r_sq = dot(ray_origin, ray_origin);
    let rcp_r = inverseSqrt(r_sq);
    let mu = dot(ray_origin, ray_dir) * rcp_r;
    let r = r_sq * rcp_r;
    return atmosphere_transmittance_mu(mu, r);
}

// ---------------------------------------------------------------- clouds/common.glsl
struct CloudsResult {
    scattering: vec4<f32>, // w = ambient scattering, for lightning flashes
    transmittance: f32,
    apparent_distance: f32,
}
fn clouds_not_hit() -> CloudsResult { return CloudsResult(vec4<f32>(0.0), 1.0, 1e5); }

// from https://iquilezles.org/articles/gradientnoise/
fn perlin_gradient(coord: vec2<f32>) -> vec2<f32> {
    let i = floor(coord);
    let f = fract(coord);
    let uu = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let du = 30.0 * f * f * (f * (f - 2.0) + 1.0);
    let g0 = hash2_v2(i + vec2<f32>(0.0, 0.0));
    let g1 = hash2_v2(i + vec2<f32>(1.0, 0.0));
    let g2 = hash2_v2(i + vec2<f32>(0.0, 1.0));
    let g3 = hash2_v2(i + vec2<f32>(1.0, 1.0));
    let v0 = dot(g0, f - vec2<f32>(0.0, 0.0));
    let v1 = dot(g1, f - vec2<f32>(1.0, 0.0));
    let v2 = dot(g2, f - vec2<f32>(0.0, 1.0));
    let v3 = dot(g3, f - vec2<f32>(1.0, 1.0));
    return g0 + uu.x * (g1 - g0) + uu.y * (g2 - g0) + uu.x * uu.y * (g0 - g1 - g2 + g3)
        + du * (uu.yx * (v0 - v1 - v2 + v3) + vec2<f32>(v1, v2) - v0);
}
fn curl2D(coord: vec2<f32>) -> vec2<f32> {
    let gradient = perlin_gradient(coord);
    return vec2<f32>(gradient.y, -gradient.x);
}
fn clouds_phase_single(cos_theta: f32) -> f32 {
    let forwards_a = klein_nishina_phase(cos_theta, 2600.0);
    let forwards_b = henyey_greenstein_phase(cos_theta, 0.8);
    return 0.8 * max(forwards_a, forwards_b) + 0.2 * henyey_greenstein_phase(cos_theta, -0.2);
}
fn clouds_phase_multi(cos_theta: f32, g: vec3<f32>) -> f32 {
    return 0.65 * henyey_greenstein_phase(cos_theta, g.x)
        + 0.10 * henyey_greenstein_phase(cos_theta, g.y)
        + 0.25 * henyey_greenstein_phase(cos_theta, -g.z);
}
fn clouds_powder_effect(density: f32, cos_theta: f32) -> f32 {
    var powder = pi * density / (density + 0.15);
    powder = mix(powder, 1.0, 0.8 * sqr(cos_theta * 0.5 + 0.5));
    return powder;
}
fn clouds_aerial_perspective(
    clouds_scattering: vec3<f32>, clouds_transmittance: f32,
    ray_origin: vec3<f32>, ray_end_in: vec3<f32>, ray_dir: vec3<f32>, clear_sky_in: vec3<f32>
) -> vec3<f32> {
    var air_transmittance: vec3<f32>;
    var ray_end = ray_end_in;
    if (CLOUDS_AERIAL_PERSPECTIVE_BOOST != 0u) {
        ray_end = mix(ray_origin, ray_end, f32(1u << CLOUDS_AERIAL_PERSPECTIVE_BOOST));
    }
    if (length_squared(ray_origin) < length_squared(ray_end)) {
        let trans_0 = atmosphere_transmittance(ray_origin, ray_dir);
        let trans_1 = atmosphere_transmittance(ray_end, ray_dir);
        air_transmittance = clamp(trans_0 / trans_1, vec3<f32>(0.0), vec3<f32>(1.0));
    } else {
        let trans_0 = atmosphere_transmittance(ray_origin, -ray_dir);
        let trans_1 = atmosphere_transmittance(ray_end, -ray_dir);
        air_transmittance = clamp(trans_1 / trans_0, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    // Blend to rain color during rain
    let clear_sky = mix(clear_sky_in, sky_color * rcp(tau), rain_strength() * mix(1.0, 0.9, time_sunrise() + time_sunset()));
    air_transmittance = mix(air_transmittance, vec3<f32>(air_transmittance.x), 0.8 * rain_strength());
    return mix((1.0 - clouds_transmittance) * clear_sky, clouds_scattering, air_transmittance);
}
fn blend_layers(old: CloudsResult, nw: CloudsResult) -> CloudsResult {
    let new_in_front = nw.apparent_distance < old.apparent_distance;
    let scattering_behind = select(nw.scattering, old.scattering, new_in_front);
    let scattering_in_front = select(old.scattering, nw.scattering, new_in_front);
    let transmittance_in_front = select(old.transmittance, nw.transmittance, new_in_front);
    return CloudsResult(
        scattering_in_front + transmittance_in_front * scattering_behind,
        old.transmittance * nw.transmittance,
        min(old.apparent_distance, nw.apparent_distance)
    );
}

// ---------------------------------------------------------------- clouds/coverage_map.glsl
const clouds_cumulus_coverage_map_scale: f32 = 1.5e5;
const clouds_coverage_map_distortion: f32 = 0.8;

fn clouds_cumulus_local_coverage(pos_in: vec2<f32>) -> f32 {
    let wind_angle = CLOUDS_CUMULUS_WIND_ANGLE * degree;
    let wind_velocity = CLOUDS_CUMULUS_WIND_SPEED * vec2<f32>(cos(wind_angle), sin(wind_angle));
    var pos = pos_in;
    pos += camera_position().xz * CLOUDS_SCALE;
    pos += wind_velocity * world_age();

    let p1 = (0.000002 / CLOUDS_CUMULUS_SIZE) * pos;
    let p2 = (0.000027 / CLOUDS_CUMULUS_SIZE) * pos;
    let noise = vec2<f32>(noise_tex(p1).x, noise_tex(p2).w); // cloud coverage, cloud shape

    var coverage_cu = 0.0;
    var coverage_st = 0.0;
    if (cp_l0_stratus_blend() < 1.0 - eps) {
        coverage_cu = mix(cp_l0_coverage().x, cp_l0_coverage().y, noise.x);
        coverage_cu = linear_step(1.0 - coverage_cu, 1.0, noise.y);
    }
    if (cp_l0_stratus_blend() > eps) {
        coverage_st = cubic_smooth(linear_step(0.9 - cp_l0_coverage().x, 1.0, 2.0 * noise.x * cp_l0_coverage().y));
        coverage_st = 0.5 * coverage_st + 1.0 * coverage_st * linear_step(0.3, 0.6, noise.y);
        coverage_st = coverage_st / (coverage_st + 1.0);
    }
    return mix(coverage_cu, coverage_st, cp_l0_stratus_blend());
}
fn project_clouds_cumulus_coverage_map(pos: vec3<f32>) -> vec2<f32> {
    var coverage_map_uv = pos.xz * rcp(0.5 * clouds_cumulus_coverage_map_scale);
    coverage_map_uv /= clouds_coverage_map_distortion * length(coverage_map_uv) + (1.0 - clouds_coverage_map_distortion);
    return coverage_map_uv * 0.5 + 0.5;
}
fn render_clouds_cumulus_coverage_map(uv: vec2<f32>) -> f32 {
    var pos = uv * 2.0 - 1.0;
    pos *= (1.0 - clouds_coverage_map_distortion) / (1.0 - length(pos) * clouds_coverage_map_distortion);
    pos *= 0.5 * clouds_cumulus_coverage_map_scale;
    return clouds_cumulus_local_coverage(pos);
}

// ---------------------------------------------------------------- clouds/cumulus.glsl (layer 0)
override USE_LOCAL_COVERAGE_MAP: bool = true; // `CLOUDS_USE_LOCAL_COVERAGE_MAP`: read the map p0 drew (off in p0 itself)

fn clouds_cumulus_altitude_shaping(density_in: f32, altitude_fraction: f32) -> f32 {
    var density = density_in;
    // Stratus shapes
    if (cp_l0_stratus_blend() > eps) {
        density = mix(
            density,
            clamp01(density * dampen(
                clamp01(2.0 * altitude_fraction)
                * linear_step(0.0, 0.1, altitude_fraction)
                * linear_step(0.0, 0.6, 1.0 - altitude_fraction)
            )),
            cp_l0_stratus_blend()
        );
    }
    // Carve egg shape
    density -= smoothstep(0.2, 1.0, altitude_fraction) * (0.6 - 0.3 * cp_l0_stratus_blend());
    // Reduce density at the bottom of the cloud
    density *= smoothstep(0.0, 0.2, altitude_fraction);
    return density;
}

fn clouds_cumulus_density(pos_in: vec3<f32>) -> f32 {
    var pos = pos_in;
    let r = length(pos);
    var density: f32;
    if (USE_LOCAL_COVERAGE_MAP) {
        // Get local coverage from precomputed coverage map - saves 1 sample and some maths
        let coverage_map_uv = project_clouds_cumulus_coverage_map(pos);
        if (r < clouds_cumulus_radius || r > clouds_cumulus_top_radius || outside01(coverage_map_uv)) {
            return 0.0;
        }
        density = textureSampleLevel(cloud_map, samp_clamp, coverage_map_uv, 0.0).z;
    } else {
        if (r < clouds_cumulus_radius || r > clouds_cumulus_top_radius) { return 0.0; }
        density = clouds_cumulus_local_coverage(pos.xz);
    }

    // Altitude shaping
    let altitude_fraction = (r - clouds_cumulus_radius) * cp_l0_altitude_scale();
    density = clouds_cumulus_altitude_shaping(density, altitude_fraction);
    if (density < eps) { return 0.0; }

    var worley_0 = 0.5;
    var worley_1 = 0.5;
    if (!PROGRAM_PREPARE) {
        let wind_angle = CLOUDS_CUMULUS_WIND_ANGLE * degree;
        let wind_velocity = CLOUDS_CUMULUS_WIND_SPEED * vec2<f32>(cos(wind_angle), sin(wind_angle));
        let wind = vec3<f32>(wind_velocity * world_age(), 0.0).xzy;
        let pos_xz = pos.xz + camera_position().xz * CLOUDS_SCALE + wind.xz; // pos.xz += cameraPosition.xz * CLOUDS_SCALE + wind.xz
        pos = vec3<f32>(pos_xz.x, pos.y, pos_xz.y);
        // 3D worley noise for detail
        worley_0 = textureSampleLevel(worley_bubbly, samp_repeat, (pos + 0.2 * wind) * 0.0009, 0.0).x;
        worley_1 = textureSampleLevel(worley_swirley, samp_repeat, (pos + 0.4 * wind) * 0.005, 0.0).x;
    }

    let detail_fade = 0.20 * smoothstep(0.85, 1.0, 1.0 - altitude_fraction)
        - 0.35 * smoothstep(0.05, 0.5, altitude_fraction) + 0.6;

    density -= cp_l0_detail_weights().x * sqr(worley_0) * dampen(clamp01(1.0 - density));
    density -= cp_l0_detail_weights().y * sqr(worley_1) * dampen(clamp01(1.0 - density)) * detail_fade;

    // Adjust density so that the clouds are wispy at the bottom and hard at the top
    density = max0(density);
    density = lift(density, mix(cp_l0_edge_sharpening().x, cp_l0_edge_sharpening().y, altitude_fraction));
    density *= 0.1 + 0.9 * smoothstep(0.2, 0.7, altitude_fraction);
    return density;
}

fn clouds_cumulus_optical_depth(ray_origin: vec3<f32>, ray_dir: vec3<f32>, dither: f32, step_count: u32) -> f32 {
    let step_growth = 2.0;
    let step_length = 0.1 * clouds_cumulus_thickness / f32(step_count); // m
    var ray_pos = ray_origin;
    var ray_step = vec4<f32>(ray_dir, 1.0) * step_length;
    var optical_depth = 0.0;
    for (var i = 0u; i < step_count; i++) {
        ray_step *= step_growth;
        optical_depth += clouds_cumulus_density(ray_pos + ray_step.xyz * dither) * ray_step.w;
        ray_pos += ray_step.xyz;
    }
    return optical_depth;
}

fn clouds_cumulus_scattering(
    density: f32, light_optical_depth: f32, sky_optical_depth: f32, ground_optical_depth: f32,
    step_transmittance: f32, cos_theta: f32, bounced_light: f32
) -> vec2<f32> {
    var scattering = vec2<f32>(0.0);
    var scatter_amount = cp_l0_scattering();
    var extinct_amount = cp_l0_extinction();
    let scattering_integral_times_density = (1.0 - step_transmittance) / cp_l0_extinction();

    var powder_effect = clouds_powder_effect(density + density * cp_l0_stratus_blend(), cos_theta);
    let scattering_falloff = 0.55 * mix(lift(clamp01(cp_l0_scattering() / 0.1), 0.33), 1.0, cos_theta * 0.5 + 0.5);

    var phase = clouds_phase_single(cos_theta);
    var phase_g = pow(vec3<f32>(0.6, 0.9, 0.3), vec3<f32>(1.0 + light_optical_depth));

    for (var i = 0u; i < 8u; i++) {
        scattering.x += scatter_amount * exp(-extinct_amount * light_optical_depth) * phase * (1.0 - 0.5 * cp_l0_shadow());
        scattering.x += scatter_amount * exp(-extinct_amount * ground_optical_depth) * isotropic_phase * bounced_light;
        scattering.x += scatter_amount * exp(-extinct_amount * sky_optical_depth) * isotropic_phase * cp_l0_shadow() * 0.5; // fake bounced lighting from the layer above
        scattering.y += scatter_amount * exp(-extinct_amount * sky_optical_depth) * isotropic_phase;

        scatter_amount *= scattering_falloff * powder_effect;
        extinct_amount *= 0.4;
        phase_g *= 0.8;
        powder_effect = mix(powder_effect, sqrt(powder_effect), 0.5);
        phase = clouds_phase_multi(cos_theta, phase_g);
    }
    return scattering * scattering_integral_times_density;
}

fn draw_cumulus_clouds(air_viewer_pos: vec3<f32>, ray_dir: vec3<f32>, clear_sky: vec3<f32>, distance_to_terrain: f32, dither: f32) -> CloudsResult {
    // Raymarching setup
    let primary_steps_horizon = select(CLOUDS_CUMULUS_PRIMARY_STEPS_H, CLOUDS_CUMULUS_PRIMARY_STEPS_H / 2u, PROGRAM_DEFERRED0);
    let primary_steps_zenith = select(CLOUDS_CUMULUS_PRIMARY_STEPS_Z, CLOUDS_CUMULUS_PRIMARY_STEPS_Z / 2u, PROGRAM_DEFERRED0);
    let lighting_steps = CLOUDS_CUMULUS_LIGHTING_STEPS;
    let ambient_steps = CLOUDS_CUMULUS_AMBIENT_STEPS;
    let max_ray_length = 2e4;
    let min_transmittance = 0.075;
    let planet_albedo = 0.4;
    let sky_dir = vec3<f32>(0.0, 1.0, 0.0);

    // Early exit if coverage is 0
    if (cp_l0_coverage().y < eps) { return clouds_not_hit(); }

    let primary_steps = u32(mix(f32(primary_steps_horizon), f32(primary_steps_zenith), abs(ray_dir.y)));
    let r = length(air_viewer_pos);

    let dists = intersect_spherical_shell(air_viewer_pos, ray_dir, clouds_cumulus_radius, clouds_cumulus_top_radius);
    let planet_intersected = intersect_sphere(air_viewer_pos, ray_dir, min(r - 10.0, planet_radius)).y >= 0.0;
    let terrain_intersected = distance_to_terrain >= 0.0 && r < clouds_cumulus_radius && distance_to_terrain < dists.x;

    if (dists.y < 0.0 // volume not intersected
        || (planet_intersected && r < clouds_cumulus_radius) // planet blocking clouds
        || terrain_intersected) { // terrain blocking clouds
        return clouds_not_hit();
    }

    var ray_length = select(dists.y, distance_to_terrain, distance_to_terrain >= 0.0);
    ray_length = clamp(ray_length - dists.x, 0.0, max_ray_length);
    let step_length = ray_length * rcp(f32(primary_steps));

    let ray_step = ray_dir * step_length;
    let ray_origin = air_viewer_pos + ray_dir * (dists.x + step_length * dither);

    var scattering = vec2<f32>(0.0); // x: direct light, y: skylight
    var transmittance = 1.0;
    var distance_sum = 0.0;
    var distance_weight_sum = 0.0;

    // Lighting setup
    let moonlit = sun_dir().y < -0.04;
    let light_d = select(sun_dir(), moon_dir(), moonlit);
    let cos_theta = dot(ray_dir, light_d);
    let bounced_light = planet_albedo * light_d.y * rcp_pi;

    // Raymarching loop
    for (var i = 0u; i < primary_steps; i++) {
        if (transmittance < min_transmittance) { break; }

        let ray_pos = ray_origin + ray_step * f32(i);
        let altitude_fraction = (length(ray_pos) - clouds_cumulus_radius) * rcp(clouds_cumulus_thickness);

        var density = clouds_cumulus_density(ray_pos);
        if (density < eps) { continue; }

        // Fade away in the distance to hide the max ray length cutoff
        let distance_to_sample = distance(ray_origin, ray_pos);
        density *= sstep(1.0, 0.95, distance_to_sample * rcp(max_ray_length));

        if (USE_LOCAL_COVERAGE_MAP) {
            // Fade away at the edges of the local coverage map
            density *= sstep(1.0, 0.9, length(ray_pos.xz) * rcp(0.5 * clouds_cumulus_coverage_map_scale));
        }

        let step_optical_depth = density * cp_l0_extinction() * step_length;
        let step_transmittance = exp(-step_optical_depth);

        var hash = vec2<f32>(0.0);
        if (!PROGRAM_DEFERRED0) { hash = hash2_v3(fract(ray_pos)); } // used to dither the light rays

        let light_optical_depth = clouds_cumulus_optical_depth(ray_pos, light_d, hash.x, lighting_steps);
        let sky_optical_depth = clouds_cumulus_optical_depth(ray_pos, sky_dir, hash.y, ambient_steps);
        // guess optical depth to the ground using altitude fraction and density from this sample
        let ground_optical_depth = mix(density, 1.0, clamp01(altitude_fraction * 2.0 - 1.0)) * altitude_fraction * clouds_cumulus_thickness;

        scattering += clouds_cumulus_scattering(
            density, light_optical_depth, sky_optical_depth, ground_optical_depth, step_transmittance, cos_theta, bounced_light
        ) * transmittance;
        transmittance *= step_transmittance;

        // Update distance to cloud
        distance_sum += distance_to_sample * density;
        distance_weight_sum += density;
    }

    // Get main light color for this layer
    var light_color = sunlight_color * atmosphere_transmittance(ray_origin, light_d);
    light_color = atmosphere_post_processing(light_color);
    light_color *= select(sun_color, moon_color, moonlit);

    // Remap the transmittance so that min_transmittance is 0
    let clouds_transmittance = linear_step(min_transmittance, 1.0, transmittance);

    var clouds_scattering = scattering.x * light_color + scattering.y * sky_color;
    clouds_scattering = clouds_aerial_perspective(clouds_scattering, clouds_transmittance, air_viewer_pos, ray_origin, ray_dir, clear_sky);

    let apparent_distance = select((distance_sum / distance_weight_sum) + distance(air_viewer_pos, ray_origin), 1e6, distance_weight_sum == 0.0);
    return CloudsResult(vec4<f32>(clouds_scattering, scattering.y), clouds_transmittance, apparent_distance);
}

// ---------------------------------------------------------------- clouds/altocumulus.glsl (layer 1)
fn clouds_altocumulus_altitude_shaping(density_in: f32, altitude_fraction: f32) -> f32 {
    var density = density_in;
    density -= smoothstep(0.2, 1.0, altitude_fraction) * 0.6; // carve egg shape
    density *= smoothstep(0.0, 0.2, altitude_fraction);       // reduce density at the bottom
    return density;
}

fn clouds_altocumulus_density(pos_in: vec3<f32>) -> f32 {
    let wind_angle = CLOUDS_ALTOCUMULUS_WIND_ANGLE * degree;
    let wind_velocity = CLOUDS_ALTOCUMULUS_WIND_SPEED * vec2<f32>(cos(wind_angle), sin(wind_angle));
    var pos = pos_in;

    let r = length(pos);
    if (r < clouds_altocumulus_radius || r > clouds_altocumulus_top_radius) { return 0.0; }

    let dynamic_thickness = mix(0.5, 1.0, smoothstep(0.4, 0.6, cp_l1_coverage().y));
    let altitude_fraction = 0.8 * (r - clouds_altocumulus_radius) * rcp(clouds_altocumulus_thickness * dynamic_thickness);

    let pos_xz = pos.xz + camera_position().xz * CLOUDS_SCALE + wind_velocity * world_age();
    pos = vec3<f32>(pos_xz.x, pos.y, pos_xz.y);

    let noise = vec4<f32>(
        // 2D noise for base shape and coverage
        noise_tex((0.000005 / CLOUDS_ALTOCUMULUS_SIZE) * pos.xz + 0.01).x, // cloud coverage
        noise_tex((0.000047 / CLOUDS_ALTOCUMULUS_SIZE) * pos.xz + 0.3).w,  // cloud shape
        noise_tex((0.000047 / CLOUDS_ALTOCUMULUS_SIZE) * pos.xz + 0.3).x,
        noise_tex((0.0001 / CLOUDS_ALTOCUMULUS_SIZE) * pos.xz + 0.6).w     // cloud detail
    );

    // altocumulus
    var density_ac = mix(cp_l1_coverage().x, cp_l1_coverage().y, dampen(noise.x));
    density_ac = linear_step(sqr(1.0 - density_ac), 1.0, noise.y);
    density_ac -= 1.9 * sqr(1.0 - noise.w) * dampen(clamp01(1.0 - density_ac));
    density_ac = clouds_altocumulus_altitude_shaping(density_ac, altitude_fraction);

    // altostratus
    var density_as = cubic_smooth(linear_step(0.9 - cp_l1_coverage().x, 1.0, 2.0 * noise.x * cp_l1_coverage().y));
    density_as = 0.5 * density_as + 1.0 * density_as * linear_step(0.3, 0.6, noise.y);
    density_as = 0.5 * density_as + density_as * linear_step(0.3, 0.7, noise.z);
    density_as = density_as / (density_as + 1.0);
    density_as *= dampen(
        clamp01(2.0 * altitude_fraction)
        * linear_step(0.0, 0.1, altitude_fraction)
        * linear_step(0.0, 0.6, 1.0 - altitude_fraction)
    );
    density_as = clamp01(density_as);

    var density = mix(density_ac, density_as, cp_l1_stratus_blend());
    if (density < eps) { return 0.0; }

    var worley = 0.5;
    if (!PROGRAM_PREPARE) {
        let wind = vec3<f32>(wind_velocity * world_age(), 0.0).xzy;
        // 3D worley noise for detail
        worley = textureSampleLevel(worley_swirley, samp_repeat, (pos + 0.2 * wind) * 0.003, 0.0).x;
    }

    density -= (0.2 * CLOUDS_ALTOCUMULUS_DETAIL_STRENGTH) * sqr(worley)
        * dampen(clamp01(1.0 - density)) * (1.0 - 0.85 * cp_l1_stratus_blend());

    // Adjust density so that the clouds are wispy at the bottom and hard at the top
    let edge_sharpening = mix(vec2<f32>(3.0, 14.0), vec2<f32>(2.0, 7.0), cp_l1_stratus_blend());
    density = max0(density);
    density = lift(density, mix(edge_sharpening.x, edge_sharpening.y, altitude_fraction));
    density *= 0.1 + 0.9 * smoothstep(0.2, 0.7, altitude_fraction);
    return density;
}

fn clouds_altocumulus_optical_depth(ray_origin: vec3<f32>, ray_dir: vec3<f32>, dither: f32, step_count: u32) -> f32 {
    let step_growth = 2.0;
    let step_length = (0.25 + 0.25 * (time_sunrise() + time_sunset())) * clouds_altocumulus_thickness / f32(step_count);
    var ray_pos = ray_origin;
    var ray_step = vec4<f32>(ray_dir, 1.0) * step_length;
    var optical_depth = 0.0;
    for (var i = 0u; i < step_count; i++) {
        ray_step *= step_growth;
        optical_depth += clouds_altocumulus_density(ray_pos + ray_step.xyz * dither) * ray_step.w;
        ray_pos += ray_step.xyz;
    }
    return optical_depth;
}

fn clouds_altocumulus_scattering(
    density: f32, light_optical_depth: f32, sky_optical_depth: f32, ground_optical_depth: f32,
    extinction_coeff: f32, scattering_coeff: f32, step_transmittance: f32, cos_theta: f32, bounced_light: f32
) -> vec2<f32> {
    var scattering = vec2<f32>(0.0);
    var scatter_amount = scattering_coeff;
    var extinct_amount = extinction_coeff;
    let scattering_integral_times_density = (1.0 - step_transmittance) / extinction_coeff;

    var powder_effect = clouds_powder_effect(density * (1.5 + 3.5 * cp_l1_stratus_blend()), cos_theta);
    var phase = clouds_phase_single(cos_theta);
    var phase_g = pow(vec3<f32>(0.6, 0.9, 0.3), vec3<f32>(1.0 + light_optical_depth));

    for (var i = 0u; i < 8u; i++) {
        scattering.x += scatter_amount * exp(-extinct_amount * light_optical_depth) * phase;
        scattering.x += scatter_amount * exp(-extinct_amount * ground_optical_depth) * isotropic_phase * bounced_light;
        scattering.y += scatter_amount * exp(-extinct_amount * sky_optical_depth) * isotropic_phase;

        scatter_amount *= 0.5 * mix(lift(clamp01(scattering_coeff / 0.05), 0.33), 1.0, cos_theta * 0.5 + 0.5) * powder_effect;
        extinct_amount *= 0.45;
        phase_g *= 0.7;
        powder_effect = mix(powder_effect, sqrt(powder_effect), 0.5);
        phase = clouds_phase_multi(cos_theta, phase_g);
    }
    return scattering * scattering_integral_times_density;
}

fn draw_altocumulus_clouds(air_viewer_pos: vec3<f32>, ray_dir: vec3<f32>, clear_sky: vec3<f32>, distance_to_terrain: f32, dither: f32) -> CloudsResult {
    let primary_steps_horizon = select(CLOUDS_ALTOCUMULUS_PRIMARY_STEPS_H, CLOUDS_ALTOCUMULUS_PRIMARY_STEPS_H / 2u, PROGRAM_DEFERRED0);
    let primary_steps_zenith = select(CLOUDS_ALTOCUMULUS_PRIMARY_STEPS_Z, CLOUDS_ALTOCUMULUS_PRIMARY_STEPS_Z / 2u, PROGRAM_DEFERRED0);
    let lighting_steps = CLOUDS_ALTOCUMULUS_LIGHTING_STEPS;
    let ambient_steps = CLOUDS_ALTOCUMULUS_AMBIENT_STEPS;
    let max_ray_length = 2e4;
    let min_transmittance = 0.075;
    let planet_albedo = 0.4;
    let sky_dir = vec3<f32>(0.0, 1.0, 0.0);

    if (cp_l1_coverage().y < eps) { return clouds_not_hit(); }

    let primary_steps = u32(mix(f32(primary_steps_horizon), f32(primary_steps_zenith), abs(ray_dir.y)));
    let r = length(air_viewer_pos);

    let dists = intersect_spherical_shell(air_viewer_pos, ray_dir, clouds_altocumulus_radius, clouds_altocumulus_top_radius);
    let planet_intersected = intersect_sphere(air_viewer_pos, ray_dir, min(r - 10.0, planet_radius)).y >= 0.0;
    let terrain_intersected = distance_to_terrain >= 0.0 && r < clouds_altocumulus_radius && distance_to_terrain < dists.x;

    if (dists.y < 0.0 || (planet_intersected && r < clouds_altocumulus_radius) || terrain_intersected) {
        return clouds_not_hit();
    }

    var ray_length = select(dists.y, distance_to_terrain, distance_to_terrain >= 0.0);
    ray_length = clamp(ray_length - dists.x, 0.0, max_ray_length);
    let step_length = ray_length * rcp(f32(primary_steps));

    let ray_step = ray_dir * step_length;
    let ray_origin = air_viewer_pos + ray_dir * (dists.x + step_length * dither);

    var scattering = vec2<f32>(0.0);
    var transmittance = 1.0;
    var distance_sum = 0.0;
    var distance_weight_sum = 0.0;

    // Lighting setup
    let high_coverage = linear_step(0.5, 0.6, cp_l1_coverage().x);
    let extinction_coeff = mix(0.08, 0.16, day_factor()) * CLOUDS_ALTOCUMULUS_DENSITY * (2.0 - 1.0 * cp_l1_stratus_blend());
    let scattering_coeff = extinction_coeff * mix(1.00, 0.75, rain_strength());

    let moonlit = sun_dir().y < -0.045;
    let light_d = select(sun_dir(), moon_dir(), moonlit);
    let cos_theta = dot(ray_dir, light_d);
    let bounced_light = planet_albedo * light_d.y * rcp_pi;

    for (var i = 0u; i < primary_steps; i++) {
        if (transmittance < min_transmittance) { break; }

        let ray_pos = ray_origin + ray_step * f32(i);
        let altitude_fraction = (length(ray_pos) - clouds_altocumulus_radius) * rcp(clouds_altocumulus_thickness);

        var density = clouds_altocumulus_density(ray_pos);
        if (density < eps) { continue; }

        // fade away in the distance to hide the cutoff
        let distance_to_sample = distance(ray_origin, ray_pos);
        let distance_fade = smoothstep(0.95, 1.0, distance_to_sample * rcp(max_ray_length));
        density *= 1.0 - distance_fade;

        let step_optical_depth = density * extinction_coeff * step_length;
        let step_transmittance = exp(-step_optical_depth);

        var hash = vec2<f32>(0.0);
        if (!PROGRAM_DEFERRED0) { hash = hash2_v3(fract(ray_pos)); }

        let light_optical_depth = clouds_altocumulus_optical_depth(ray_pos, light_d, hash.x, lighting_steps);
        let sky_optical_depth = clouds_altocumulus_optical_depth(ray_pos, sky_dir, hash.y, ambient_steps);
        let ground_optical_depth = mix(density, 1.0, clamp01(altitude_fraction * 2.0 - 1.0)) * altitude_fraction * clouds_altocumulus_thickness;

        scattering += clouds_altocumulus_scattering(
            density, light_optical_depth, sky_optical_depth, ground_optical_depth,
            extinction_coeff, scattering_coeff, step_transmittance, cos_theta, bounced_light
        ) * transmittance;
        transmittance *= step_transmittance;

        distance_sum += distance_to_sample * density;
        distance_weight_sum += density;
    }

    var light_color = sunlight_color * atmosphere_transmittance(ray_origin, light_d);
    light_color = atmosphere_post_processing(light_color);
    light_color *= select(sun_color, moon_color, moonlit);
    light_color *= 1.0 + 0.4 * high_coverage * dampen(time_noon());

    let clouds_transmittance = linear_step(min_transmittance, 1.0, transmittance);
    var clouds_scattering = scattering.x * light_color + scattering.y * sky_color;
    clouds_scattering = clouds_aerial_perspective(clouds_scattering, clouds_transmittance, air_viewer_pos, ray_origin, ray_dir, clear_sky);

    let apparent_distance = select((distance_sum / distance_weight_sum) + distance(air_viewer_pos, ray_origin), 1e6, distance_weight_sum == 0.0);
    return CloudsResult(vec4<f32>(clouds_scattering, scattering.y), clouds_transmittance, apparent_distance);
}

// ---------------------------------------------------------------- clouds/cumulus_congestus.glsl (alternative layer 0)
fn congestus_scattering_coeff() -> f32 { return clouds_cumulus_congestus_extinction_coeff * (1.0 - 0.2 * rain_strength()); }

fn clouds_cumulus_congestus_altitude_shaping(density_in: f32, altitude_fraction: f32) -> f32 {
    var density = density_in;
    // Carve egg shape
    density -= sqr(linear_step(0.3, 1.0, altitude_fraction)) * clamp01(1.0 - 0.2 * cp_congestus_blend());
    // Reduce density at the top and bottom of the cloud
    density *= smoothstep(0.0, 0.1, altitude_fraction);
    density *= smoothstep(0.0, 0.1, 1.0 - altitude_fraction);
    return density;
}

fn clouds_cumulus_congestus_density(pos_in: vec3<f32>) -> f32 {
    let wind_angle = CLOUDS_CUMULUS_WIND_ANGLE * degree;
    let wind_velocity = CLOUDS_CUMULUS_WIND_SPEED * vec2<f32>(cos(wind_angle), sin(wind_angle));
    var pos = pos_in;

    let r = length(pos);
    if (r < clouds_cumulus_congestus_radius || r > clouds_cumulus_congestus_top_radius) { return 0.0; }

    let altitude_fraction = (r - clouds_cumulus_congestus_radius) * rcp(clouds_cumulus_congestus_thickness);
    let distance_fraction = linear_step(clouds_cumulus_congestus_distance, clouds_cumulus_congestus_end_distance, length(pos.xz));

    let pos_xz = pos.xz + camera_position().xz * CLOUDS_SCALE;
    pos = vec3<f32>(pos_xz.x, pos.y, pos_xz.y);

    // 2D noise for base shape and coverage
    let noise = noise_tex((0.000003 / CLOUDS_CUMULUS_CONGESTUS_SIZE) * (pos.xz + wind_velocity * (world_age() + 50.0 * sqr(altitude_fraction)))).w;

    var density = 1.5 * sqr(linear_step(
        0.75 - 0.15 * clamp01(cp_congestus_blend() * CLOUDS_CUMULUS_CONGESTUS_COVERAGE),
        1.0,
        sqrt(noise)
    ));
    density = clouds_cumulus_congestus_altitude_shaping(density, altitude_fraction);
    density *= 4.0 * distance_fraction * (1.0 - distance_fraction);
    density *= linear_step(0.0, 0.3, cp_congestus_blend());
    if (density < eps) { return 0.0; }

    var worley_0 = 0.5;
    var worley_1 = 0.5;
    if (!PROGRAM_PREPARE) {
        let wind = vec3<f32>(wind_velocity * world_age(), 0.0).xzy;
        // 3D worley noise for detail
        worley_0 = textureSampleLevel(worley_swirley, samp_repeat, (pos + 0.2 * wind) * 0.00005, 0.0).x;
        worley_1 = textureSampleLevel(worley_swirley, samp_repeat, (pos + 0.4 * wind) * 0.00023, 0.0).x;
    }

    let detail_fade = 0.20 * smoothstep(0.85, 1.0, 1.0 - altitude_fraction)
        - 0.35 * smoothstep(0.05, 0.5, altitude_fraction) + 0.8;

    density -= (7.0 * CLOUDS_CUMULUS_CONGESTUS_DETAIL_STRENGTH)
        * sqr(clamp01(1.0 - density)) * cube(worley_0) * sqr(altitude_fraction);
    density -= (0.2 * CLOUDS_CUMULUS_CONGESTUS_DETAIL_STRENGTH) * sqr(worley_1)
        * dampen(clamp01(1.0 - density))
        * dampen(dampen(clamp01(detail_fade + 0.5 * sqr(altitude_fraction))));

    // Adjust density so that the clouds are wispy at the bottom and hard at the top
    density = max0(density);
    density = lift(clamp01(density), mix(2.0, 10.0, altitude_fraction));
    density *= sqr(linear_step(0.0, 0.5, altitude_fraction));
    return density;
}

fn clouds_cumulus_congestus_optical_depth(ray_origin: vec3<f32>, ray_dir: vec3<f32>, dither: f32, step_count: u32) -> f32 {
    let step_growth = 2.0;
    let step_length = 0.1 * clouds_cumulus_congestus_thickness / f32(step_count);
    var ray_pos = ray_origin;
    var ray_step = vec4<f32>(ray_dir, 1.0) * step_length;
    var optical_depth = 0.0;
    for (var i = 0u; i < step_count; i++) {
        ray_step *= step_growth;
        optical_depth += clouds_cumulus_congestus_density(ray_pos + ray_step.xyz * dither) * ray_step.w;
        ray_pos += ray_step.xyz;
    }
    return optical_depth;
}

fn clouds_cumulus_congestus_scattering(
    density: f32, light_optical_depth: f32, sky_optical_depth: f32, ground_optical_depth: f32,
    step_transmittance: f32, cos_theta: f32, bounced_light: f32
) -> vec2<f32> {
    var scattering = vec2<f32>(0.0);
    var scatter_amount = congestus_scattering_coeff();
    var extinct_amount = clouds_cumulus_congestus_extinction_coeff;
    let scattering_integral_times_density = (1.0 - step_transmittance) / clouds_cumulus_congestus_extinction_coeff;

    var powder_effect = clouds_powder_effect(2.0 * density, cos_theta);
    var phase = clouds_phase_single(cos_theta);
    var phase_g = pow(vec3<f32>(0.6, 0.9, 0.3), vec3<f32>(1.0 + light_optical_depth));

    for (var i = 0u; i < 8u; i++) {
        scattering.x += scatter_amount * exp(-extinct_amount * light_optical_depth * 0.33) * phase;
        scattering.x += scatter_amount * exp(-extinct_amount * ground_optical_depth * 0.33) * isotropic_phase * bounced_light;
        scattering.y += scatter_amount * exp(-extinct_amount * sky_optical_depth * 0.33) * isotropic_phase;

        scatter_amount *= 0.55 * mix(lift(clamp01(congestus_scattering_coeff() / 0.1), 0.33), 1.0, cos_theta * 0.5 + 0.5) * powder_effect;
        extinct_amount *= 0.4;
        phase_g *= 0.5;
        powder_effect = mix(powder_effect, sqrt(powder_effect), 0.5);
        phase = clouds_phase_multi(cos_theta, phase_g);
    }
    return scattering * scattering_integral_times_density;
}

fn draw_cumulus_congestus_clouds(air_viewer_pos: vec3<f32>, ray_dir: vec3<f32>, clear_sky: vec3<f32>, distance_to_terrain: f32, dither: f32) -> CloudsResult {
    let primary_steps = CLOUDS_CUMULUS_CONGESTUS_PRIMARY_STEPS;
    let lighting_steps = CLOUDS_CUMULUS_CONGESTUS_LIGHTING_STEPS;
    let ambient_steps = CLOUDS_CUMULUS_CONGESTUS_AMBIENT_STEPS;
    let min_transmittance = 0.075;
    let planet_albedo = 0.4;
    let sky_dir = vec3<f32>(0.0, 1.0, 0.0);

    let sphere_dists = intersect_spherical_shell(air_viewer_pos, ray_dir, clouds_cumulus_congestus_radius, clouds_cumulus_congestus_top_radius);
    let cylinder_dists = intersect_cylindrical_shell(air_viewer_pos, ray_dir, clouds_cumulus_congestus_distance, clouds_cumulus_congestus_end_distance);
    let dists = vec2<f32>(max(sphere_dists.x, cylinder_dists.x), min(sphere_dists.y, cylinder_dists.y));

    let planet_intersected = intersect_sphere(air_viewer_pos, ray_dir, min(length(air_viewer_pos) - 10.0, planet_radius)).y >= 0.0;

    if (dists.y < 0.0
        || (planet_intersected && length(air_viewer_pos) < clouds_cumulus_congestus_radius)
        || distance_to_terrain > 0.0) {
        return clouds_not_hit();
    }

    let ray_length = dists.y - dists.x;
    let step_length = ray_length * rcp(f32(primary_steps));
    let ray_step = ray_dir * step_length;
    let ray_origin = air_viewer_pos + ray_dir * (dists.x + step_length * dither);

    var scattering = vec2<f32>(0.0);
    var transmittance = 1.0;
    var distance_sum = 0.0;
    var distance_weight_sum = 0.0;

    let moonlit = sun_dir().y < -0.04;
    let light_d = select(sun_dir(), moon_dir(), moonlit);
    let cos_theta = dot(ray_dir, light_d);
    let bounced_light = planet_albedo * light_d.y * rcp_pi;

    for (var i = 0u; i < primary_steps; i++) {
        if (transmittance < min_transmittance) { break; }

        let ray_pos = ray_origin + ray_step * f32(i);
        let altitude_fraction = (length(ray_pos) - clouds_cumulus_congestus_radius) * rcp(clouds_cumulus_congestus_thickness);

        let density = clouds_cumulus_congestus_density(ray_pos);
        if (density < eps) { continue; }

        let distance_to_sample = distance(ray_origin, ray_pos);
        let step_optical_depth = density * clouds_cumulus_congestus_extinction_coeff * step_length;
        let step_transmittance = exp(-step_optical_depth);

        var hash = vec2<f32>(0.0);
        if (!PROGRAM_DEFERRED0) { hash = hash2_v3(fract(ray_pos)); }

        let light_optical_depth = clouds_cumulus_congestus_optical_depth(ray_pos, light_d, hash.x, lighting_steps);
        let sky_optical_depth = clouds_cumulus_congestus_optical_depth(ray_pos, sky_dir, hash.y, ambient_steps);
        let ground_optical_depth = mix(density, 1.0, clamp01(altitude_fraction * 2.0 - 1.0)) * altitude_fraction * clouds_cumulus_congestus_thickness;

        scattering += clouds_cumulus_congestus_scattering(
            density, light_optical_depth, sky_optical_depth, ground_optical_depth, step_transmittance, cos_theta, bounced_light
        ) * transmittance;
        transmittance *= step_transmittance;

        distance_sum += distance_to_sample * density;
        distance_weight_sum += density;
    }

    var light_color = sunlight_color * atmosphere_transmittance(ray_origin, light_d);
    light_color = atmosphere_post_processing(light_color);
    light_color *= select(sun_color, moon_color, moonlit);

    let clouds_transmittance = linear_step(min_transmittance, 1.0, transmittance);

    // Aerial perspective
    var clouds_scattering = scattering.x * light_color + scattering.y * sky_color;
    clouds_scattering = clouds_aerial_perspective(clouds_scattering, clouds_transmittance, air_viewer_pos, ray_origin, ray_dir, clear_sky);

    // Fade away at the horizon
    let horizon_fade = mix(
        dampen(linear_step(0.0, 0.08, ray_dir.y)),
        1.0,
        smoothstep(
            sqr(clouds_cumulus_congestus_radius),
            sqr(clouds_cumulus_congestus_radius + 0.1 * clouds_cumulus_congestus_thickness),
            length_squared(air_viewer_pos)
        )
    );
    clouds_scattering = mix(clear_sky * (1.0 - clouds_transmittance), clouds_scattering, horizon_fade);

    let apparent_distance = select((distance_sum / distance_weight_sum) + distance(air_viewer_pos, ray_origin), 1e6, distance_weight_sum == 0.0);
    return CloudsResult(vec4<f32>(clouds_scattering, scattering.y), clouds_transmittance, apparent_distance);
}

// ---------------------------------------------------------------- clouds/cirrus.glsl (layer 2, planar)
fn clouds_cirrus_density(coord_in: vec2<f32>, altitude_fraction: f32) -> f32 {
    let wind_angle = CLOUDS_CIRRUS_WIND_ANGLE * degree;
    let wind_velocity = CLOUDS_CIRRUS_WIND_SPEED * vec2<f32>(cos(wind_angle), sin(wind_angle));

    var coord = coord_in + camera_position().xz * CLOUDS_SCALE;
    coord = coord + wind_velocity * world_age();

    let curl = curl2D(0.00002 * coord) * 0.5 + curl2D(0.00004 * coord) * 0.25 + curl2D(0.00008 * coord) * 0.125;
    let height_shaping = 1.0 - abs(1.0 - 2.0 * altitude_fraction);

    // Cirrus
    var density_cirrus = 0.7 * noise_tex((0.000001 / CLOUDS_CIRRUS_SIZE) * coord + (0.004 * CLOUDS_CIRRUS_CURL_STRENGTH) * curl).x
        + 0.3 * noise_tex((0.000008 / CLOUDS_CIRRUS_SIZE) * coord + (0.008 * CLOUDS_CIRRUS_CURL_STRENGTH) * curl).x;
    density_cirrus = linear_step(0.7 - cp_cirrus(), 1.0, density_cirrus);

    var detail_coord = coord;
    var detail_amplitude = 0.2;
    var detail_frequency = 0.00002;
    var curl_strength = 0.1 * CLOUDS_CIRRUS_CURL_STRENGTH;

    for (var i = 0; i < 4; i++) {
        let detail = noise_tex(detail_coord * detail_frequency + curl * curl_strength).x;
        density_cirrus -= detail * detail_amplitude;
        detail_amplitude *= 0.6;
        detail_frequency *= 2.0;
        curl_strength *= 4.0;
        detail_coord += 0.3 * wind_velocity * world_age();
    }

    density_cirrus = mix(1.0, 0.75, day_factor()) * cube(max0(density_cirrus)) * sqr(height_shaping) * CLOUDS_CIRRUS_DENSITY;

    // Cirrocumulus
    var coverage = noise_tex((0.0000026 / CLOUDS_CIRROCUMULUS_SIZE) * coord + 0.25).w;
    coverage = 5.0 * linear_step(0.25, 0.9, cp_cirrocumulus() * coverage);

    var density_cirrocumulus = dampen(noise_tex(
        (0.000025 * rcp(CLOUDS_CIRROCUMULUS_SIZE)) * coord + (0.033 * CLOUDS_CIRROCUMULUS_CURL_STRENGTH) * curl
    ).w);
    density_cirrocumulus = linear_step(1.0 - coverage, 1.0, density_cirrocumulus);

    let curl_cc = curl2D(0.001 * coord);

    density_cirrocumulus -= sqr(noise_tex(coord * 0.00005 + (0.003 * CLOUDS_CIRROCUMULUS_CURL_STRENGTH) * curl_cc).y)
        * (CLOUDS_CIRROCUMULUS_DETAIL_STRENGTH * 1.0);
    density_cirrocumulus -= sqr(noise_tex(coord * 0.0002 + (0.007 * CLOUDS_CIRROCUMULUS_CURL_STRENGTH) * curl_cc).y)
        * (CLOUDS_CIRROCUMULUS_DETAIL_STRENGTH * 0.5);
    density_cirrocumulus -= sqr(noise_tex(coord * 0.0008 + (0.03 * CLOUDS_CIRROCUMULUS_CURL_STRENGTH) * curl_cc).y)
        * (CLOUDS_CIRROCUMULUS_DETAIL_STRENGTH * 0.1);

    density_cirrocumulus = max0(density_cirrocumulus);
    density_cirrocumulus = 0.25 * pow4(max0(density_cirrocumulus)) * height_shaping * CLOUDS_CIRROCUMULUS_DENSITY;

    return density_cirrus + density_cirrocumulus;
}

fn clouds_cirrus_optical_depth(ray_origin: vec3<f32>, ray_dir: vec3<f32>, dither: f32) -> f32 {
    let step_count = CLOUDS_CIRRUS_LIGHTING_STEPS;
    let max_ray_length = 1e3;
    let step_growth = 1.5;

    // Assuming ray_origin is between inner and outer boundary, find distance to closest layer boundary
    let inner_sphere = intersect_sphere(ray_origin, ray_dir, clouds_cirrus_radius - 0.5 * CLOUDS_CIRRUS_THICKNESS);
    let outer_sphere = intersect_sphere(ray_origin, ray_dir, clouds_cirrus_radius + 0.5 * CLOUDS_CIRRUS_THICKNESS);

    var ray_length = select(outer_sphere.y, inner_sphere.x, inner_sphere.y >= 0.0);
    ray_length = min(ray_length, max_ray_length);

    // Find initial step length a so that sum(a r^i) = rayLength
    let step_coeff = (step_growth - 1.0) / (pow(step_growth, f32(step_count)) - 1.0) / step_growth;
    let step_length = ray_length * step_coeff;

    var ray_pos = ray_origin;
    var ray_step = vec4<f32>(ray_dir, 1.0) * step_length;
    var optical_depth = 0.0;

    for (var i = 0u; i < step_count; i++) {
        ray_step *= step_growth;

        let dithered_pos = ray_pos + ray_step.xyz * dither;
        let r = length(dithered_pos);
        let altitude_fraction = (r - clouds_cirrus_radius) * rcp(CLOUDS_CIRRUS_THICKNESS) + 0.5;
        if (clamp01(altitude_fraction) != altitude_fraction) {
            ray_pos += ray_step.xyz; // `continue` still runs the loop's increment in the GLSL `for`
            continue;
        }
        let sphere_pos = dithered_pos * (clouds_cirrus_radius / r);
        optical_depth += clouds_cirrus_density(sphere_pos.xz, altitude_fraction) * ray_step.w;
        ray_pos += ray_step.xyz;
    }
    return optical_depth;
}

fn clouds_cirrus_scattering(density: f32, view_transmittance: f32, light_optical_depth: f32, cos_theta: f32) -> vec2<f32> {
    var scattering = vec2<f32>(0.0);
    var phase = clouds_phase_single(cos_theta);
    var phase_g = vec3<f32>(0.6, 0.9, 0.3);

    var powder_effect = 8.0 * (1.0 - exp(-40.0 * density));
    powder_effect = mix(powder_effect, 1.0, pow1d5(cos_theta * 0.5 + 0.5));

    var scatter_amount = clouds_cirrus_scattering_coeff;
    var extinct_amount = clouds_cirrus_extinction_coeff
        * (1.0 + 0.5 * max0(smoothstep(0.0, 0.15, abs(sun_dir().y)) - smoothstep(0.5, 0.7, cp_cirrus())));

    for (var i = 0u; i < 4u; i++) {
        scattering.x += scatter_amount * exp(-extinct_amount * light_optical_depth) * phase * powder_effect; // direct light
        scattering.y += scatter_amount * exp(-0.33 * CLOUDS_CIRRUS_THICKNESS * extinct_amount * density) * isotropic_phase; // sky light

        scatter_amount *= 0.5;
        extinct_amount *= 0.25;
        phase_g *= 0.5;
        powder_effect = mix(powder_effect, sqrt(powder_effect), 0.5);
        phase = clouds_phase_multi(cos_theta, phase_g);
    }
    let scattering_integral = (1.0 - view_transmittance) / clouds_cirrus_extinction_coeff;
    return scattering * scattering_integral;
}

fn draw_cirrus_clouds(air_viewer_pos: vec3<f32>, ray_dir: vec3<f32>, clear_sky: vec3<f32>, distance_to_terrain: f32, dither: f32) -> CloudsResult {
    // Early exit if coverage is 0
    if (cp_cirrus() < eps && cp_cirrocumulus() < eps) { return clouds_not_hit(); }

    // Ray casting
    let r = length(air_viewer_pos);
    let dists = intersect_sphere(air_viewer_pos, ray_dir, clouds_cirrus_radius);
    let planet_intersected = intersect_sphere(air_viewer_pos, ray_dir, min(r - 10.0, planet_radius)).y >= 0.0;
    let terrain_intersected = distance_to_terrain >= 0.0 && r < clouds_cirrus_radius && distance_to_terrain < dists.y;

    if (dists.y < 0.0 || (planet_intersected && r < clouds_cirrus_radius) || terrain_intersected) {
        return clouds_not_hit();
    }

    let distance_to_sphere = select(dists.x, dists.y, r < clouds_cirrus_radius);
    let sphere_pos = air_viewer_pos + ray_dir * distance_to_sphere;

    // Cloud lighting
    let moonlit = sun_dir().y < -0.049;
    let light_d = select(sun_dir(), moon_dir(), moonlit);
    let cos_theta = dot(ray_dir, light_d);

    let density = clouds_cirrus_density(sphere_pos.xz, 0.5);
    if (density < eps) { return clouds_not_hit(); }

    let light_optical_depth = clouds_cirrus_optical_depth(sphere_pos, light_d, dither);
    let view_optical_depth = 0.5 * density * clouds_cirrus_extinction_coeff * CLOUDS_CIRRUS_THICKNESS * rcp(abs(ray_dir.y) + eps);
    let view_transmittance = exp(-view_optical_depth);

    let scattering = clouds_cirrus_scattering(density, view_transmittance, light_optical_depth, cos_theta);

    // Get main light color for this layer
    var light_color = sunlight_color * atmosphere_transmittance(sphere_pos, light_d);
    light_color = atmosphere_post_processing(light_color);
    light_color *= select(sun_color, moon_color, moonlit);

    var clouds_scattering = scattering.x * light_color + scattering.y * sky_color * 1.41;
    clouds_scattering = clouds_aerial_perspective(clouds_scattering, view_transmittance, air_viewer_pos, sphere_pos, ray_dir, clear_sky);

    return CloudsResult(vec4<f32>(clouds_scattering, scattering.y), view_transmittance, distance_to_sphere);
}

// ---------------------------------------------------------------- clouds/noctilucent.glsl (layer 3)
fn clouds_noctilucent_density(coord_in: vec2<f32>) -> f32 {
    let coord = coord_in * 0.25;
    let curl = curl2D(0.00002 * coord) * 0.5 + curl2D(0.00004 * coord) * 0.25 + curl2D(0.00008 * coord) * 0.125;

    var density = dampen(noise_tex(0.000001 * coord + 0.02 * curl).y);

    var detail_amplitude = 0.3;
    var detail_frequency = 0.00001;
    var curl_strength = 0.05;

    for (var i = 0; i < 4; i++) {
        let detail = noise_tex(coord * detail_frequency + curl * curl_strength).y;
        density += detail * detail_amplitude;
        detail_amplitude *= 0.5;
        detail_frequency *= 1.5;
        curl_strength *= 4.0;
    }

    var highlight = dampen(noise_tex(0.000004 * coord + 0.02 * curl).y);
    highlight -= (1.0 - noise_tex(0.000014 * coord + 0.05 * curl).y) * 0.15;
    density += 2.0 * pow8(max0(highlight));

    return sqr(max0(density)) * cp_noctilucent();
}

fn draw_noctilucent_clouds(air_viewer_pos: vec3<f32>, ray_dir: vec3<f32>, clear_sky: vec3<f32>) -> vec4<f32> {
    let color = 0.35 * vec3<f32>(1.5, 2.0, 9.0);
    let phase_g = 0.8;
    let extinction_coeff = 0.001;

    let visibility = pulse(sun_dir().y, -0.10, 0.1);
    if (visibility < eps || cp_noctilucent() < eps) { return vec4<f32>(0.0, 0.0, 0.0, 1.0); }

    // Ray casting
    let r = length(air_viewer_pos);
    let dists = intersect_sphere(air_viewer_pos, ray_dir, clouds_noctilucent_radius);
    let planet_intersected = intersect_sphere(air_viewer_pos, ray_dir, min(r - 10.0, planet_radius)).y >= 0.0;

    if (dists.y < 0.0 || (planet_intersected && r < clouds_noctilucent_radius)) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }

    let distance_to_sphere = select(dists.x, dists.y, r < clouds_cirrus_radius); // sic: the pack compares with the cirrus radius
    let sphere_pos = air_viewer_pos + ray_dir * distance_to_sphere;

    // Cloud lighting
    let density = clouds_noctilucent_density(sphere_pos.xz);
    let transmittance = exp(-extinction_coeff * density);

    let phase = henyey_greenstein_phase(dot(ray_dir, sun_dir()), phase_g);
    var scattering = visibility * color * density * phase;
    scattering = clouds_aerial_perspective(scattering, transmittance, air_viewer_pos, sphere_pos, ray_dir, clear_sky);
    return vec4<f32>(scattering, transmittance);
}

// ---------------------------------------------------------------- clouds.glsl
fn clouds_early_exit(result: CloudsResult, r: f32, layer_radius: f32) -> bool {
    let has_congestus = cp_congestus_blend() > eps;
    return result.transmittance < 1e-3 && r < layer_radius
        && (result.apparent_distance < clouds_cumulus_congestus_distance || !has_congestus);
}

fn draw_clouds(air_viewer_pos: vec3<f32>, ray_dir: vec3<f32>, clear_sky: vec3<f32>, distance_to_terrain: f32, dither: f32) -> CloudsResult {
    var result = clouds_not_hit();
    let r = length(air_viewer_pos);

    // CLOUDS_CUMULUS
    result = draw_cumulus_clouds(air_viewer_pos, ray_dir, clear_sky, distance_to_terrain, dither);
    if (clouds_early_exit(result, r, clouds_cumulus_radius)) { return result; }

    // CLOUDS_ALTOCUMULUS
    let result_ac = draw_altocumulus_clouds(air_viewer_pos, ray_dir, clear_sky, distance_to_terrain, dither);
    result = blend_layers(result, result_ac);
    if (clouds_early_exit(result, r, clouds_altocumulus_radius)) { return result; }

    // CLOUDS_CUMULUS_CONGESTUS
    if (cp_congestus_blend() > eps) {
        let result_cu_con = draw_cumulus_congestus_clouds(air_viewer_pos, ray_dir, clear_sky, distance_to_terrain, dither);

        // fade existing clouds into congestus
        let distance_fade = mix(
            1.0,
            result_cu_con.transmittance,
            linear_step(0.75, 1.0, result.apparent_distance * rcp(clouds_cumulus_congestus_distance))
        );
        result.scattering = result.scattering * distance_fade;
        result.transmittance += (1.0 - result.transmittance) * (1.0 - distance_fade);
        result.apparent_distance = mix(result_cu_con.apparent_distance, result.apparent_distance, distance_fade);

        result = blend_layers(result, result_cu_con);
        if (result.transmittance < 1e-3) { return result; }
    }

    // CLOUDS_CIRRUS
    let result_ci = draw_cirrus_clouds(air_viewer_pos, ray_dir, clear_sky, distance_to_terrain, dither);
    result = blend_layers(result, result_ci);
    if (result.transmittance < 1e-3) { return result; }

    // CLOUDS_NOCTILUCENT
    let result_nlc = draw_noctilucent_clouds(air_viewer_pos, ray_dir, clear_sky);
    result.scattering = vec4<f32>(result.scattering.rgb + result_nlc.xyz * result.transmittance, result.scattering.w);
    result.transmittance *= result_nlc.w;

    return result;
}

// ---------------------------------------------------------------- bicubic.glsl (the two filters the clouds use)
fn bicubic_weights(v: f32) -> vec4<f32> {
    let n = vec4<f32>(1.0, 2.0, 3.0, 4.0) - v;
    let s = n * n * n;
    let x = s.x;
    let y = s.y - 4.0 * s.x;
    let z = s.z - 4.0 * s.y + 6.0 * s.x;
    let w = 6.0 - x - y - z;
    return vec4<f32>(x, y, z, w) * (1.0 / 6.0);
}
fn bicubic_filter(t: texture_2d<f32>, coord_in: vec2<f32>) -> vec4<f32> {
    let res = vec2<f32>(textureDimensions(t, 0));
    let view_pixel_size = 1.0 / res;
    var coord = coord_in * res - 0.5;
    let fxy = fract(coord);
    coord -= fxy;
    let x_weights = bicubic_weights(fxy.x);
    let y_weights = bicubic_weights(fxy.y);
    let c = coord.xxyy + vec2<f32>(-0.5, 1.5).xyxy;
    let s = vec4<f32>(x_weights.xz + x_weights.yw, y_weights.xz + y_weights.yw);
    var offset = c + vec4<f32>(x_weights.yw, y_weights.yw) / s;
    offset *= view_pixel_size.xxyy;
    let sample0 = textureSampleLevel(t, samp_clamp, offset.xz, 0.0);
    let sample1 = textureSampleLevel(t, samp_clamp, offset.yz, 0.0);
    let sample2 = textureSampleLevel(t, samp_clamp, offset.xw, 0.0);
    let sample3 = textureSampleLevel(t, samp_clamp, offset.yw, 0.0);
    let sx = s.x / (s.x + s.y);
    let sy = s.z / (s.z + s.w);
    return mix(mix(sample3, sample2, sx), mix(sample1, sample0, sx), sy);
}
fn catmull_rom_filter_fast(t: texture_2d<f32>, coord: vec2<f32>, sharpness: f32) -> vec4<f32> {
    let res = vec2<f32>(textureDimensions(t, 0));
    let view_pixel_size = 1.0 / res;
    let position = res * coord;
    let center_position = floor(position - 0.5) + 0.5;
    let f = position - center_position;
    let f2 = f * f;
    let f3 = f * f2;
    let w0 = -sharpness * f3 + 2.0 * sharpness * f2 - sharpness * f;
    let w1 = (2.0 - sharpness) * f3 - (3.0 - sharpness) * f2 + 1.0;
    let w2 = -(2.0 - sharpness) * f3 + (3.0 - 2.0 * sharpness) * f2 + sharpness * f;
    let w3 = sharpness * f3 - sharpness * f2;
    let w12 = w1 + w2;
    let tc12 = view_pixel_size * (center_position + w2 / w12);
    let center_color = textureSampleLevel(t, samp_clamp, vec2<f32>(tc12.x, tc12.y), 0.0);
    let tc0 = view_pixel_size * (center_position - 1.0);
    let tc3 = view_pixel_size * (center_position + 2.0);
    let l0 = w12.x * w0.y;
    let l1 = w0.x * w12.y;
    let l2 = w12.x * w12.y;
    let l3 = w3.x * w12.y;
    let l4 = w12.x * w3.y;
    let color = textureSampleLevel(t, samp_clamp, vec2<f32>(tc12.x, tc0.y), 0.0) * l0
        + textureSampleLevel(t, samp_clamp, vec2<f32>(tc0.x, tc12.y), 0.0) * l1 + center_color * l2
        + textureSampleLevel(t, samp_clamp, vec2<f32>(tc3.x, tc12.y), 0.0) * l3
        + textureSampleLevel(t, samp_clamp, vec2<f32>(tc12.x, tc3.y), 0.0) * l4;
    return color / (l0 + l1 + l2 + l3 + l4);
}

// ---------------------------------------------------------------- lighting/cloud_shadows.glsl
const cloud_shadow_res: f32 = 512.0;
const cloud_shadow_extent: f32 = 256.0 / (CLOUDS_SCALE / 10.0);

fn shadow_view_to_cloud_shadow_space(shadow_view_pos: vec3<f32>) -> vec2<f32> {
    var cloud_shadow_pos = shadow_view_pos.xy / cloud_shadow_extent;
    cloud_shadow_pos /= 1.0 + length(cloud_shadow_pos);
    return cloud_shadow_pos * 0.5 + 0.5;
}
fn project_cloud_shadow_map(scene_pos: vec3<f32>) -> vec2<f32> {
    return shadow_view_to_cloud_shadow_space(transform(u.shadow_view, scene_pos));
}
fn unproject_cloud_shadow_map(cloud_shadow_pos_in: vec2<f32>) -> vec3<f32> {
    var cloud_shadow_pos = cloud_shadow_pos_in * 2.0 - 1.0;
    cloud_shadow_pos /= 1.0 - length(cloud_shadow_pos);
    let shadow_view_pos = vec3<f32>(cloud_shadow_pos * cloud_shadow_extent, 1.0);
    return transform(u.shadow_view_inv, shadow_view_pos);
}
// What d4 (and anything lit by the sun) multiplies the sun light by.
fn get_cloud_shadows(scene_pos: vec3<f32>) -> f32 {
    let cloud_shadow_pos = project_cloud_shadow_map(scene_pos) * cloud_shadow_res / vec2<f32>(textureDimensions(cloud_map, 0));
    if (outside01(cloud_shadow_pos)) { return 1.0; }

    // fade out cloud shadows when the fragment is above the cloud layer, or the sun is near the horizon
    let r = planet_radius + (scene_pos.y + eye_altitude() - SEA_LEVEL) * CLOUDS_SCALE;
    let altitude_fraction = linear_step(clouds_cumulus_radius, clouds_cumulus_top_radius, r);
    let cloud_shadow_fade = smoothstep(0.05, 0.15, light_dir().y) * clamp01(1.0 - altitude_fraction);

    var cloud_shadow = bicubic_filter(cloud_map, cloud_shadow_pos).x;
    cloud_shadow = cloud_shadow * cloud_shadow_fade + (1.0 - cloud_shadow_fade);
    return cloud_shadow * CLOUD_SHADOWS_INTENSITY + (1.0 - CLOUD_SHADOWS_INTENSITY);
}

fn render_cloud_shadow_map(uv: vec2<f32>) -> vec2<f32> {
    // Transform position from scene-space to clouds-space
    var ray_origin = unproject_cloud_shadow_map(uv);
    ray_origin = vec3<f32>(ray_origin.x, ray_origin.z, ray_origin.y + eye_altitude() - SEA_LEVEL).xzy * CLOUDS_SCALE
        + vec3<f32>(0.0, planet_radius, 0.0);
    // ^ the pack writes vec3(ray_origin.xz, ray_origin.y + eyeAltitude - SEA_LEVEL).xzy: (x, y + .., z)

    let light_d = light_dir();
    var pos: vec3<f32>;
    var t: f32;
    var density: f32;
    var extinction_coeff: f32;
    var shadow = 1.0;
    var shadow_cumulus_only = 1.0;
    var distance_fade: f32;
    let distance_fade_strength = 0.00000001 * pulse(light_d.y, -0.01, 0.2);

    // CLOUDS_CUMULUS
    extinction_coeff = 0.25 * cp_l0_extinction();
    t = intersect_sphere(ray_origin, light_d, clouds_cumulus_radius + 0.25 * clouds_cumulus_thickness).y;
    pos = ray_origin + light_d * t;
    distance_fade = exp2(-distance_fade_strength * length(pos.xy));
    density = clouds_cumulus_density(pos);
    shadow *= exp(-1.00 * distance_fade * extinction_coeff * clouds_cumulus_thickness * rcp(abs(light_d.y) + eps) * density);
    shadow_cumulus_only = shadow;

    // CLOUDS_ALTOCUMULUS
    extinction_coeff = mix(0.05, 0.1, day_factor()) * CLOUDS_ALTOCUMULUS_DENSITY * (1.0 - 0.33 * rain_strength());
    t = intersect_sphere(ray_origin, light_d, clouds_altocumulus_radius + 0.5 * clouds_altocumulus_thickness).y;
    pos = ray_origin + light_d * t;
    distance_fade = exp2(-distance_fade_strength * length(pos.xy));
    density = clouds_altocumulus_density(pos);
    shadow *= exp(-1.00 * distance_fade * extinction_coeff * clouds_altocumulus_thickness * rcp(abs(light_d.y) + eps) * density);

    // CLOUDS_CIRRUS
    t = intersect_sphere(ray_origin, light_d, clouds_cirrus_radius).y;
    pos = ray_origin + light_d * t;
    distance_fade = exp2(-distance_fade_strength * length(pos.xy));
    density = clouds_cirrus_density(pos.xz, 0.5);
    shadow *= exp(-1.00 * distance_fade * clouds_cirrus_extinction_coeff * (CLOUDS_CIRRUS_ALTITUDE * CLOUDS_ALTOCUMULUS_THICKNESS)
        * rcp(abs(light_d.y) + eps) * density) * 0.5 + 0.5;

    return vec2<f32>(shadow, shadow_cumulus_only);
}

// ---------------------------------------------------------------- sky/crepuscular_rays.glsl
fn draw_crepuscular_rays(ray_direction_world: vec3<f32>, is_terrain: bool, dither: f32) -> vec4<f32> {
    let step_count_horizon = CREPUSCULAR_RAYS_STEPS_HORIZON;
    let step_count_zenith = CREPUSCULAR_RAYS_STEPS_ZENITH;
    let max_ray_length = 4096.0 / (CLOUDS_SCALE / 10.0);
    // no SKY_GROUND: allow volume to extend indefinitely into planet
    let volume_inner_radius = 1.0;
    let volume_outer_radius = clouds_cumulus_radius + clouds_cumulus_thickness * 0.5;
    let extinction_coeff = 2.0 * dampen(cp_crepuscular())
        * (50.0 * air_rayleigh_coefficient + 400.0 * air_mie_coefficient) * (CLOUDS_SCALE / 10.0);
    let underground_light_fade_distance = 100.0;

    let step_count = u32(mix(f32(step_count_horizon), f32(step_count_zenith), dampen(abs(ray_direction_world.y))));

    // Calculate ray start and ray end
    let r = planet_radius + eye_altitude() * CLOUDS_SCALE;
    let dists = intersect_spherical_shell_mu(ray_direction_world.y, r, volume_inner_radius, volume_outer_radius) * rcp(CLOUDS_SCALE);

    if (dists.y < 0.0 || is_terrain) { return vec4<f32>(0.0, 0.0, 0.0, 1.0); }

    let ray_length = clamp(dists.y - dists.x, 0.0, max_ray_length);
    let step_length = ray_length * rcp(f32(step_count));

    // Transform to shadow view space
    let ray_direction_shadow = (u.shadow_view * vec4<f32>(ray_direction_world, 0.0)).xyz;
    let ray_origin_shadow = u.shadow_view[3].xyz + ray_direction_shadow * dists.x;
    let ray_step_shadow = ray_direction_shadow * step_length;

    let ray_origin_world = vec3<f32>(0.0, planet_radius, 0.0) + ray_direction_world * dists.x;
    let ray_step_planet = ray_direction_world * step_length;

    // Raymarching loop
    let scattering_coeff = extinction_coeff;
    let step_optical_depth = extinction_coeff * step_length;
    let step_transmittance = exp(-step_optical_depth);

    var scattering = vec3<f32>(0.0);
    var transmittance = vec3<f32>(1.0);

    for (var i = 0u; i < step_count; i++) {
        let ray_position_planet = ray_origin_world + ray_step_planet * (f32(i) + dither);
        let ray_position_shadow = ray_origin_shadow + ray_step_shadow * (f32(i) + dither);
        let d = step_length * (f32(i) + dither);

        let cloud_shadow = dot(textureSampleLevel(cloud_map, samp_clamp, shadow_view_to_cloud_shadow_space(ray_position_shadow), 0.0).xy, vec2<f32>(0.5));
        let a = linear_step(
            sqr(planet_radius - underground_light_fade_distance),
            sqr(planet_radius),
            length_squared(ray_position_planet)
        );
        let b = 1.0 - exp2(-0.002 * d);

        scattering += cube(cloud_shadow) * transmittance * a * b;
        transmittance *= step_transmittance;
    }

    // Lighting
    var light_color = sunlight_color * atmosphere_transmittance_mu(light_dir().y, planet_radius);
    light_color = atmosphere_post_processing(light_color);
    light_color *= select(sun_color, moon_color, u.sun.w > 0.5);

    let step_transmitted_fraction = (1.0 - step_transmittance) / max(step_optical_depth, vec3<f32>(eps));
    let LoV = dot(ray_direction_world, light_dir());
    let forwards = henyey_greenstein_phase(LoV, 0.5);
    let phase = mix(0.5, 1.5, time_sunrise() + time_sunset()) * forwards + 0.5 * henyey_greenstein_phase(LoV, -0.2);

    scattering *= scattering_coeff * step_transmitted_fraction * light_color * step_length * cp_crepuscular();
    scattering *= (6.0 * CREPUSCULAR_RAYS_INTENSITY) * phase;
    // (the pack also mixes `transmittance` toward 1 by the amount here, but returns w = 1.0 regardless)
    return vec4<f32>(scattering, 1.0);
}

// ---------------------------------------------------------------- sky/aurora.glsl
fn aurora_amount() -> f32 { return u.aur_amount.x; }

fn aurora_shape(pos: vec3<f32>, altitude_fraction: f32) -> f32 {
    let wind_0 = 0.005 * vec2<f32>(0.7, 0.1);
    let wind_1 = 0.008 * vec2<f32>(-0.1, -0.7);
    let frequency = 0.00003 * AURORA_FREQUENCY;
    let height_fade = cube(1.0 - altitude_fraction) * linear_step(0.0, 0.025, altitude_fraction);
    let worley_0 = noise_tex(pos.xz * frequency + wind_0 * u.taa.w).y;
    let worley_1 = noise_tex(pos.xz * frequency + wind_1 * u.taa.w).y;
    return linear_step(1.0, 2.0, worley_0 + worley_1) * height_fade;
}
fn aurora_color(altitude_fraction: f32) -> vec3<f32> {
    return mix(u.aur0.xyz, u.aur1.xyz, clamp01(dampen(altitude_fraction)));
}
fn draw_aurora(ray_dir: vec3<f32>, dither: f32) -> vec3<f32> {
    let step_count = 64u;
    let rcp_steps = rcp(f32(step_count));
    let volume_bottom = 1000.0;
    let volume_top = 3000.0;
    let volume_radius = 20000.0;

    if (aurora_amount() < 0.01) { return vec3<f32>(0.0); }

    // Calculate distance to enter and exit the volume
    let rcp_dir_y = rcp(ray_dir.y);
    let distance_to_lower_plane = volume_bottom * rcp_dir_y;
    let distance_to_upper_plane = volume_top * rcp_dir_y;
    let distance_to_cylinder = volume_radius * rcp_length2(ray_dir.xz);

    let distance_to_volume_start = distance_to_lower_plane;
    let distance_to_volume_end = min(distance_to_cylinder, distance_to_upper_plane);

    // Make sure that the volume is intersected
    if (distance_to_volume_start > distance_to_volume_end) { return vec3<f32>(0.0); }

    // Raymarching setup
    let ray_length = max0(distance_to_volume_end - distance_to_volume_start);
    let step_length = ray_length * rcp_steps;
    var ray_pos = ray_dir * (distance_to_volume_start + step_length * dither);
    let ray_step = ray_dir * step_length;

    var emission = vec3<f32>(0.0);
    for (var i = 0u; i < step_count; i++) {
        let altitude_fraction = linear_step(volume_bottom, volume_top, ray_pos.y);
        let shape = aurora_shape(ray_pos, altitude_fraction);
        let color = aurora_color(altitude_fraction);

        let d = length(ray_pos.xz);
        let distance_fade = (1.0 - cube(d * rcp(volume_radius))) * (1.0 - exp2(-0.001 * d));
        emission += color * (shape * distance_fade * step_length);
        ray_pos += ray_step;
    }
    return (0.001 * AURORA_BRIGHTNESS) * emission * aurora_amount();
}
