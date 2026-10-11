// Photon Shaders by SixthSurge, ported to WGSL (see LICENSE-PHOTON, which must stay with this file).
// Sources: include/sky/{atmosphere,projection,stars,sky}.glsl, include/lighting/colors/{light,weather}_color.glsl,
// program/d0_sky_map.{vsh,fsh}. Group 0 only, so a later pass (d4 deferred shading) can paste this file into its own
// module and call `draw_sky`, `atmosphere_scattering`, `light_color`, `project_sky`, `sh_evaluate_irradiance`.
//
// What differs from the pack (everything else is line for line):
//  - Values the pack gets from Iris uniforms (`sun_dir`, `time_*`, `sunAngle`...) come from the `Sky` block, filled on the
//    CPU by `render/sky.rs::params`. mc-rs has no moon phases (b1.7.3): moonPhase is 0, so `moon_phase_brightness` is 1.
//  - Clouds, aurora, rainbow, the analytic air fog over the sky map and the cave fade are NOT here yet (clouds and fog are
//    milestone 4, rainbow is weather, milestone 6); `draw_sky` takes their slot as the (0, 0, 0, 1) of `clouds_and_aurora`.
//  - `fast_acos` is `acos`; the user tints (SUN_*, all 1.0 in the pack's defaults) are left out.

struct Sky {
    inv_vp: mat4x4<f32>,    // clip (xy, z = 1) -> world direction (view rotation only)
    sun_dir: vec4<f32>,
    moon_dir: vec4<f32>,
    light_dir: vec4<f32>,
    celestial: mat4x4<f32>, // world direction -> the frame the stars are fixed in (turns with the sun and moon)
    times: vec4<f32>,       // time_sunrise, time_noon, time_sunset, time_midnight
    misc: vec4<f32>,        // rainStrength, sunAngle (OptiFine: 0 sunrise, 0.5 sunset), seconds, atmosphere_saturation_boost_amount
};

@group(0) @binding(0) var<uniform> sky: Sky;
@group(0) @binding(1) var scattering_lut: texture_3d<f32>; // 32 x 64 x 32, Rayleigh + multiple scattering | single Mie
@group(0) @binding(2) var lut_samp: sampler;               // linear, clamp
@group(0) @binding(3) var noise_tex: texture_2d<f32>;      // noisetex (512 x 512)
@group(0) @binding(4) var noise_samp: sampler;             // linear, repeat

const PI: f32 = 3.14159265358979;
const TAU: f32 = 6.28318530717959;
const HALF_PI: f32 = 1.57079632679490;
const RCP_PI: f32 = 0.318309886183791;
const DEGREE: f32 = 0.0174532925199433;
const EPS: f32 = 1e-6;

// Size of the sky map, one column wider than the sky itself: column 191 holds the light colours (rows 0, 1).
const SKY_MAP_RES: vec2<f32> = vec2<f32>(191.0, 108.0);

fn sqr(x: f32) -> f32 { return x * x; }
fn cube(x: f32) -> f32 { return x * x * x; }
fn clamp01(x: f32) -> f32 { return clamp(x, 0.0, 1.0); }
fn max0(x: f32) -> f32 { return max(x, 0.0); }
fn rcp(x: f32) -> f32 { return 1.0 / x; }
fn pow1d5(x: f32) -> f32 { return x * sqrt(x); }
fn linear_step(e0: f32, e1: f32, x: f32) -> f32 { return clamp01((x - e0) / (e1 - e0)); }
fn cubic_smooth(x: f32) -> f32 { return sqr(x) * (3.0 - 2.0 * x); }
fn dampen(x: f32) -> f32 { let y = clamp01(x); return y * (2.0 - y); }
fn pulse(x: f32, center: f32, width: f32) -> f32 {
    let d = abs(x - center) / width;
    return select(1.0 - cubic_smooth(d), 0.0, d > 1.0);
}
fn get_uv_from_unit_range(v: f32, res: f32) -> f32 { return v * (1.0 - 1.0 / res) + 0.5 / res; }
fn safe_acos(x: f32) -> f32 { return acos(clamp(x, -1.0, 1.0)); }

// Rec. 709 -> Rec. 2020 (the pack's working colour space), `rec709_to_rec2020` as a row-vector product.
fn to_working(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        dot(vec3<f32>(0.62742569, 0.32928911, 0.04335024), c),
        dot(vec3<f32>(0.06902744, 0.91957858, 0.01136138), c),
        dot(vec3<f32>(0.01636642, 0.08802890, 0.89568432), c),
    );
}
const LUMINANCE_REC2020: vec3<f32> = vec3<f32>(0.2627, 0.6780, 0.0593);

// intersect_sphere(mu, r, R) of Bruneton's atmosphere: distances along a ray at height r with cosine mu.
fn intersect_sphere_mu(mu: f32, r: f32, radius: f32) -> vec2<f32> {
    let discriminant = r * r * (mu * mu - 1.0) + radius * radius;
    if discriminant < 0.0 { return vec2<f32>(-1.0); }
    return -r * mu + vec2<f32>(-sqrt(discriminant), sqrt(discriminant));
}
fn intersect_sphere_ray(origin: vec3<f32>, dir: vec3<f32>, radius: f32) -> vec2<f32> {
    let b = dot(origin, dir);
    let discriminant = b * b - dot(origin, origin) + radius * radius;
    if discriminant < 0.0 { return vec2<f32>(-1.0); }
    return -b + vec2<f32>(-sqrt(discriminant), sqrt(discriminant));
}

// ---------------------------------------------------------------------------------------------------------------------
//   Atmosphere (include/sky/atmosphere.glsl)
// ---------------------------------------------------------------------------------------------------------------------

const SUNLIGHT_COLOR: vec3<f32> = vec3<f32>(1.051, 0.985, 0.940); // sunlight in space (AM0)
const SUN_ANGULAR_RADIUS: f32 = 2.0 * DEGREE;
const MOON_ANGULAR_RADIUS: f32 = 3.0 * DEGREE;

const PLANET_RADIUS: f32 = 6371e3;
const ATM_INNER: f32 = 6370e3; // planet_radius - 1e3
const ATM_OUTER: f32 = 6481e3; // planet_radius + 110e3
const MIN_MU_S: f32 = -0.35;

// Rayleigh, Mie and ozone extinction in Rec. 2020 (`air_*_coefficient`), for the analytic transmittance.
const AIR_RAYLEIGH: vec3<f32> = vec3<f32>(1.2328517e-05, 1.6387961e-05, 3.8148164e-05);
const AIR_MIE: vec3<f32> = vec3<f32>(1.7273862e-06, 1.8041928e-06, 1.9414234e-06);
const AIR_OZONE: vec3<f32> = vec3<f32>(9.563766e-07, 1.2671053e-06, 1.7807270e-07);
const AIR_SCALE_HEIGHTS: vec2<f32> = vec2<f32>(8.4e3, 1.25e3);

// ATMOSPHERE_SATURATION_BOOST is on (intensity 1) and ATMOSPHERE_RAIN_DESATURATION is off in the pack's settings.
fn atmosphere_post_processing(atmosphere: vec3<f32>) -> vec3<f32> {
    return mix(vec3<f32>(dot(atmosphere, LUMINANCE_REC2020)), atmosphere, 1.0 + sky.misc.w);
}

fn nvidia_phase_area(nu: f32, g: f32, a: f32, radius: f32) -> f32 {
    let radius_eff = max(radius, EPS);
    let cosr = cos(radius_eff);
    let sinr = sin(radius_eff);
    var mu = nu * cosr + sqrt(max(0.0, 1.0 - nu * nu)) * sinr;
    if nu > cosr { mu = 1.0; } // keep the centre bright
    let gg = g * g;
    return ((1.0 - gg) * (1.0 + a * mu * mu)) / (PI * pow1d5(1.0 + gg - 2.0 * g * mu) * 4.0 * (1.0 + (a * (1.0 + 2.0 * gg)) / 3.0));
}

fn atmosphere_mie_phase_sun(nu: f32) -> vec3<f32> {
    return 0.2 * vec3<f32>(
        nvidia_phase_area(nu, 0.85, 1.0, SUN_ANGULAR_RADIUS),
        nvidia_phase_area(nu, 0.86, 1.0, SUN_ANGULAR_RADIUS),
        nvidia_phase_area(nu, 0.87, 1.0, SUN_ANGULAR_RADIUS),
    );
}

// moonPhase is always 0 here: the pack's phase factor t is exactly 1.
fn atmosphere_mie_phase_moon(nu: f32) -> vec3<f32> {
    return 1.02 * vec3<f32>(
        nvidia_phase_area(nu, 0.895, 1.0, 2.0 * MOON_ANGULAR_RADIUS),
        nvidia_phase_area(nu, 0.90, 1.0, 2.0 * MOON_ANGULAR_RADIUS),
        nvidia_phase_area(nu, 0.905, 1.0, 2.0 * MOON_ANGULAR_RADIUS),
    );
}

// Mapping functions of the LUT (Bruneton 2020, nu from Zombye's Spectrum). nu: view-light, mu: view-zenith, mu_s: light-zenith.
fn lut_u_nu(nu: f32, mu: f32, mu_s: f32) -> f32 {
    let half_range = sqrt((1.0 - mu * mu) * (1.0 - mu_s * mu_s));
    let nu_min = mu * mu_s - half_range;
    let nu_max = mu * mu_s + half_range;
    return get_uv_from_unit_range(select((nu - nu_min) / (nu_max - nu_min), nu_min, nu_min == nu_max), 16.0);
}

fn lut_u_mu(mu_in: f32) -> f32 {
    var mu = mu_in;
    if mu > 0.0 { mu *= sqrt(sqrt(mu)); } // stretch the sky near the horizon upwards
    let r = PLANET_RADIUS;
    let H = sqrt(ATM_OUTER * ATM_OUTER - ATM_INNER * ATM_INNER);
    let rho = sqrt(max0(PLANET_RADIUS * PLANET_RADIUS - ATM_INNER * ATM_INNER));
    let rmu = r * mu;
    let discriminant = rmu * rmu - r * r + ATM_INNER * ATM_INNER;
    if mu < 0.0 && discriminant >= 0.0 { // the ray hits the ground
        let d = -rmu - sqrt(max0(discriminant));
        let d_min = r - ATM_INNER;
        let d_max = rho;
        let u = select((d - d_min) / (d_max - d_min), 0.0, d_max == d_min);
        return 0.5 - 0.5 * get_uv_from_unit_range(u, 32.0);
    }
    let d = -rmu + sqrt(discriminant + H * H);
    let d_min = ATM_OUTER - r;
    let d_max = rho + H;
    return 0.5 + 0.5 * get_uv_from_unit_range((d - d_min) / (d_max - d_min), 32.0);
}

fn lut_u_mu_s(mu_s: f32) -> f32 {
    let H = sqrt(ATM_OUTER * ATM_OUTER - ATM_INNER * ATM_INNER);
    let d_min = ATM_OUTER - ATM_INNER;
    let d_max = H;
    let A = (intersect_sphere_mu(MIN_MU_S, ATM_INNER, ATM_OUTER).y - d_min) / (d_max - d_min);
    let a = (intersect_sphere_mu(mu_s, ATM_INNER, ATM_OUTER).y - d_min) / (d_max - d_min);
    return get_uv_from_unit_range(max0(1.0 - a / A) / (1.0 + a), 32.0);
}

// Sky radiance for both lights at once (`atmosphere_scattering(ray_dir, sun_color, sun_dir, moon_color, moon_dir, ...)`).
fn atmosphere_scattering(ray_dir: vec3<f32>, sun_color: vec3<f32>, moon_color: vec3<f32>) -> vec3<f32> {
    let sun_dir = sky.sun_dir.xyz;
    let moon_dir = sky.moon_dir.xyz;
    let mu_sun = sun_dir.y;
    let mu_moon = moon_dir.y;
    // No SKY_GROUND: the lower half of the sky repeats the horizon.
    let horizon_mu = mix(-0.01, 0.03, clamp01(smoothstep(-0.05, 0.1, mu_sun) + smoothstep(0.05, 0.1, mu_moon)));
    let mu = max(ray_dir.y, horizon_mu);
    let nu_sun = dot(ray_dir, sun_dir);
    let nu_moon = dot(ray_dir, moon_dir);

    let u_mu = lut_u_mu(mu);
    let u_sun = vec2<f32>(lut_u_nu(nu_sun, mu, mu_sun) * 0.5, lut_u_mu_s(mu_sun));
    let u_moon = vec2<f32>(lut_u_nu(nu_moon, mu, mu_moon) * 0.5, lut_u_mu_s(mu_moon));

    // x: Rayleigh + multiple scattering, x + 0.5: single Mie scattering (times its phase function)
    let sun_c = textureSampleLevel(scattering_lut, lut_samp, vec3<f32>(u_sun.x, u_mu, u_sun.y), 0.0).rgb;
    let sun_m = textureSampleLevel(scattering_lut, lut_samp, vec3<f32>(u_sun.x + 0.5, u_mu, u_sun.y), 0.0).rgb;
    let moon_c = textureSampleLevel(scattering_lut, lut_samp, vec3<f32>(u_moon.x, u_mu, u_moon.y), 0.0).rgb;
    let moon_m = textureSampleLevel(scattering_lut, lut_samp, vec3<f32>(u_moon.x + 0.5, u_mu, u_moon.y), 0.0).rgb;

    let atmosphere = (sun_c + sun_m * atmosphere_mie_phase_sun(nu_sun)) * sun_color
        + (moon_c + moon_m * atmosphere_mie_phase_moon(nu_moon)) * moon_color;
    return atmosphere_post_processing(atmosphere);
}

// Chapman function approximation, http://www.thetenthplanet.de/archives/4519
fn chapman_function_approx(x: f32, cos_theta: f32) -> f32 {
    let c = sqrt(HALF_PI * x);
    if cos_theta >= 0.0 { return c / ((c - 1.0) * cos_theta + 1.0); }
    let sin_theta = sqrt(clamp01(1.0 - sqr(cos_theta)));
    return c / ((c - 1.0) * cos_theta - 1.0) + 2.0 * c * exp(x - x * sin_theta) * sqrt(sin_theta);
}

// Transmittance from height r to space along a ray with cosine mu (the pack's non-LUT version, ATMOSPHERE_TRANSMITTANCE_LUT is off).
fn atmosphere_transmittance(mu: f32, r: f32) -> vec3<f32> {
    if intersect_sphere_mu(mu, max(r, PLANET_RADIUS + 10.0), PLANET_RADIUS).x >= 0.0 { return vec3<f32>(0.0); }
    let rcp_h = 1.0 / AIR_SCALE_HEIGHTS;
    let density = exp(r * -rcp_h + PLANET_RADIUS * rcp_h);
    var airmass = AIR_SCALE_HEIGHTS * density;
    airmass.x *= chapman_function_approx(r * rcp_h.x, mu);
    airmass.y *= chapman_function_approx(r * rcp_h.y, mu);
    return clamp(exp(-(AIR_RAYLEIGH * airmass.x + AIR_MIE * airmass.y + AIR_OZONE * airmass.x)), vec3<f32>(0.0), vec3<f32>(1.0));
}

// ---------------------------------------------------------------------------------------------------------------------
//   Light colours (include/lighting/colors/light_color.glsl, weather_color.glsl)
// ---------------------------------------------------------------------------------------------------------------------

fn get_sun_exposure() -> f32 {
    let blue_hour = linear_step(0.05, 1.0, exp(-190.0 * sqr(sky.sun_dir.y + 0.09604)));
    return 7.0 * (1.0 + 0.5 * (sky.times.z + sky.times.x) + 40.0 * blue_hour);
}

fn get_sun_tint() -> vec3<f32> {
    let blue_hour = linear_step(0.05, 1.0, exp(-190.0 * sqr(sky.sun_dir.y + 0.09604)));
    let morning_evening = mix(vec3<f32>(1.0), vec3<f32>(1.05, 0.84, 0.93) * 1.2, sqr(pulse(sky.sun_dir.y, 0.17, 0.40)));
    return morning_evening * mix(vec3<f32>(1.0), vec3<f32>(0.95, 0.80, 1.0), blue_hour);
}

fn get_moon_exposure() -> f32 {
    return 0.66 * (1.0 + 0.33 * rcp(clamp01(1.25 * max(-sky.sun_dir.y, 0.1))));
}

fn get_moon_tint() -> vec3<f32> { return to_working(pow(vec3<f32>(0.75, 0.83, 1.0), vec3<f32>(2.2))); }

fn get_light_color() -> vec3<f32> {
    let light_dir = sky.light_dir.xyz;
    var c = atmosphere_post_processing(SUNLIGHT_COLOR * atmosphere_transmittance(light_dir.y, PLANET_RADIUS));
    c *= mix(get_sun_exposure() * get_sun_tint(), get_moon_exposure() * get_moon_tint(), step(0.5, sky.misc.y));
    c *= clamp01(light_dir.y / 0.02); // fade away during the day/night transition
    return c * (1.0 - 0.25 * pulse(abs(light_dir.y), 0.15, 0.11));
}

fn get_skylight_boost() -> f32 {
    return 1.0 + 0.5 * linear_step(0.05, 1.0, exp(-25.0 * sqr(sky.sun_dir.y + 0.3)));
}

// Rain colour only: snow and sandstorm come with the weather milestone (biome_may_snow / biome_may_sandstorm are 0).
fn get_weather_color() -> vec3<f32> {
    return mix(0.033, 0.66, smoothstep(-0.1, 0.5, sky.sun_dir.y)) * SUNLIGHT_COLOR * vec3<f32>(0.49, 0.65, 1.00);
}

fn get_ambient_color(sun_color: vec3<f32>, moon_color: vec3<f32>) -> vec3<f32> {
    // not straight up, to avoid the sun halo when the sun path rotation is 0
    let c = TAU * atmosphere_scattering(normalize(vec3<f32>(0.0, 1.0, -0.8)), sun_color, moon_color) * 1.13;
    return mix(c, TAU * get_weather_color(), sky.misc.x);
}

// ---------------------------------------------------------------------------------------------------------------------
//   Sky map projection (include/sky/projection.glsl), https://sebh.github.io/publications/egsr2020.pdf
// ---------------------------------------------------------------------------------------------------------------------

fn project_sky(direction: vec3<f32>) -> vec2<f32> {
    let projected = normalize(direction.xz);
    let azimuth = PI + atan2(projected.x, -projected.y);
    let altitude = HALF_PI - safe_acos(direction.y);
    var coord = vec2<f32>(azimuth * (1.0 / TAU), 0.5 + 0.5 * sign(altitude) * sqrt(2.0 * RCP_PI * abs(altitude)));
    let pad = 2.0 / SKY_MAP_RES.x;
    coord.x = coord.x * (1.0 - 2.0 * pad) + pad;
    coord.x *= 191.0 / 192.0;
    return coord;
}

fn unproject_sky(uv: vec2<f32>) -> vec3<f32> {
    var coord = uv;
    coord.x *= 192.0 / 191.0;
    let pad = 2.0 / SKY_MAP_RES.x;
    let mul = 1.0 / (1.0 - 2.0 * pad);
    coord.x = fract(coord.x * mul - pad * mul);
    coord.y = select(sqr(2.0 * coord.y - 1.0), -sqr(1.0 - 2.0 * coord.y), coord.y < 0.5);
    let azimuth = coord.x * TAU - PI;
    let altitude = coord.y * HALF_PI;
    return vec3<f32>(cos(altitude) * sin(azimuth), sin(altitude), -cos(altitude) * cos(azimuth));
}

// ---------------------------------------------------------------------------------------------------------------------
//   Stars (include/sky/stars.glsl), based on https://www.shadertoy.com/view/Md2SR3
// ---------------------------------------------------------------------------------------------------------------------

const STARS_INTENSITY: f32 = 1.0;
const STARS_COVERAGE: f32 = 0.5;

fn hash4(p: vec2<f32>) -> vec4<f32> {
    var p4 = fract(p.xyxy * vec4<f32>(.1031, .1030, .0973, .1099));
    p4 += dot(p4, p4.wzxy + 33.33);
    return fract((p4.xxyz + p4.yzzw) * p4.zywx);
}

fn blackbody(temperature: f32) -> vec3<f32> {
    let lambda = vec3<f32>(660.0, 550.0, 440.0);
    let lambda2 = lambda * lambda;
    let lambda5 = lambda2 * lambda2 * lambda;
    let h = 6.63e-16; // Planck constant
    let k = 1.38e-5;  // Boltzmann constant
    let c = 3.0e17;   // speed of light
    let a = lambda5 / (2.0 * h * c * c);
    let b = (h * c) / (k * lambda);
    let rgb = a * exp(b / temperature) - a;
    return min(rgb.x, min(rgb.y, rgb.z)) / rgb;
}

fn unstable_star_field(coord: vec2<f32>, star_threshold: f32) -> vec3<f32> {
    let noise = hash4(coord);
    var star = linear_step(star_threshold, 1.0, noise.x);
    star = pow(star, 16.0) * STARS_INTENSITY;
    let color = blackbody(mix(4500.0, 8500.0, noise.y));
    star *= 1.0 - noise.z * cos(sky.misc.z * 2.0 + TAU * noise.w); // twinkle
    return star * color;
}

// Samples the four neighbouring integer coordinates and interpolates, so the field does not shimmer.
fn stable_star_field(coord_in: vec2<f32>, star_threshold: f32) -> vec3<f32> {
    let coord = abs(coord_in) + 33.3 * step(vec2<f32>(0.0), coord_in);
    let i = floor(coord);
    let f = vec2<f32>(cubic_smooth(coord.x - i.x), cubic_smooth(coord.y - i.y));
    return unstable_star_field(i, star_threshold) * (1.0 - f.x) * (1.0 - f.y)
        + unstable_star_field(i + vec2<f32>(1.0, 0.0), star_threshold) * f.x * (1.0 - f.y)
        + unstable_star_field(i + vec2<f32>(0.0, 1.0), star_threshold) * f.y * (1.0 - f.x)
        + unstable_star_field(i + vec2<f32>(1.0, 1.0), star_threshold) * f.x * f.y;
}

// `ray_dir` in the celestial frame; the brightest stars come first as the sun sets (no GALAXY, so galaxy_luminance is 0).
fn draw_stars(ray_dir: vec3<f32>) -> vec3<f32> {
    let star_threshold = 1.0 - 0.05 * STARS_COVERAGE * smoothstep(-0.2, 0.05, -sky.sun_dir.y);
    // project the direction onto a plane
    let coord = (ray_dir.xy / (abs(ray_dir.z) + length(ray_dir.xy)) + 41.21 * sign(ray_dir.z)) * 600.0;
    return stable_star_field(coord, star_threshold);
}

// ---------------------------------------------------------------------------------------------------------------------
//   Sun and moon (include/sky/sky.glsl; the pack's own, VANILLA_SUN / VANILLA_MOON are off)
// ---------------------------------------------------------------------------------------------------------------------

const MOON_LUMINANCE: f32 = 10.0;

fn draw_sun(ray_dir: vec3<f32>, sun_color: vec3<f32>) -> vec3<f32> {
    let nu = dot(ray_dir, sky.sun_dir.xyz);
    // limb darkening, http://www.physics.hmc.edu/faculty/esin/a101/limbdarkening.pdf
    let alpha = vec3<f32>(0.429, 0.522, 0.614);
    let center_to_edge = max0(SUN_ANGULAR_RADIUS - safe_acos(nu));
    let limb_darkening = pow(vec3<f32>(1.0 - sqr(1.0 - center_to_edge)), 0.5 * alpha);
    let sun_luminance = 1.0 / (TAU * (1.0 - cos(SUN_ANGULAR_RADIUS))); // luminance of the sun disk
    return sun_luminance * sun_color * step(0.0, center_to_edge) * limb_darkening;
}

fn draw_moon(ray_dir: vec3<f32>) -> vec4<f32> {
    let moon_dir = sky.moon_dir.xyz;
    let lit_color = vec3<f32>(0.75, 0.80, 1.0);  // MOON_R, MOON_G - 0.03, MOON_B
    let glow_color = vec3<f32>(0.70, 0.83, 1.0); // MOON_R - 0.05, MOON_G, MOON_B
    let MoV = dot(ray_dir, moon_dir);
    if MoV < cos(MOON_ANGULAR_RADIUS) { return vec4<f32>(0.0); }
    let dist = clamp01(safe_acos(MoV) / MOON_ANGULAR_RADIUS);

    // z parallel to moon_dir
    let tangent = select(normalize(cross(vec3<f32>(0.0, 1.0, 0.0), moon_dir)), vec3<f32>(1.0, 0.0, 0.0), moon_dir.y == 1.0);
    let bitangent = normalize(cross(tangent, moon_dir));
    // the pack subtracts sun_dir here, not moon_dir: kept, it only picks which part of the noise the moon shows
    let v = ray_dir - sky.sun_dir.xyz;
    let offset = fract(vec2<f32>(dot(tangent, v), dot(bitangent, v)) + 0.5);
    let noise = textureSampleLevel(noise_tex, noise_samp, 2.0 * offset, 0.0).xyz;
    let moon_texture = pow1d5(noise.x) * 0.75 + 0.6 * cube(noise.y) - 0.1 * noise.z;

    // the moon as a sphere 1 unit away, lit from the viewer (moonPhase 0: the pack's light_dir is -ray_dir)
    let moon_dist = intersect_sphere_ray(-moon_dir, ray_dir, MOON_ANGULAR_RADIUS).x;
    let moon_normal = normalize(ray_dir * moon_dist - moon_dir);
    let moon_shadow = dampen(max0(dot(moon_normal, -ray_dir)));
    let edge_glow = sqr(sqr(sqr(dist)));

    var color = max(moon_shadow * lit_color * (1.0 + 4.0 * edge_glow), 0.5 * glow_color * (0.1 + 0.1 * edge_glow))
        * (0.2 + 0.8 * moon_texture);
    color = MOON_LUMINANCE * color * color;
    return vec4<f32>(color, 1.0);
}

// d4's sky branch for a ray (draw_sky with PROGRAM_DEFERRED4): stars, sun and moon seen through the atmosphere, plus the
// atmosphere itself. `clouds_and_aurora` is the clouds' slot: scattering in rgb, transmittance in a (milestone 4).
fn draw_sky(ray_dir: vec3<f32>, atmosphere: vec3<f32>, clouds_and_aurora: vec4<f32>, sun_color: vec3<f32>) -> vec3<f32> {
    let celestial_dir = (sky.celestial * vec4<f32>(ray_dir, 0.0)).xyz;
    var s = draw_stars(celestial_dir) + draw_sun(ray_dir, sun_color);
    let moon = draw_moon(ray_dir);
    s = s * (1.0 - moon.a) + moon.rgb;
    s *= atmosphere_transmittance(ray_dir.y, PLANET_RADIUS) * (1.0 - sky.misc.x);
    s += atmosphere;
    return s * clouds_and_aurora.a + clouds_and_aurora.rgb;
}

// ---------------------------------------------------------------------------------------------------------------------
//   Spherical harmonics (include/utility/spherical_harmonics.glsl), used by d4a and later by d4 for ambient light
// ---------------------------------------------------------------------------------------------------------------------

fn sh_coeff_order_2(d: vec3<f32>) -> array<f32, 9> {
    return array<f32, 9>(
        0.2820947918,
        0.4886025119 * d.x,
        0.4886025119 * d.z,
        0.4886025119 * d.y,
        1.0925484310 * d.x * d.y,
        1.0925484310 * d.y * d.z,
        0.3153915653 * (3.0 * d.z * d.z - 1.0),
        0.7725484040 * d.x * d.z,
        0.3862742020 * (d.x * d.x - d.y * d.y),
    );
}

// Irradiance for a normal with a cone of visibility (Activision, "Practical Real-Time Strategies for Accurate Indirect Occlusion", section 5).
fn sh_evaluate_irradiance(sh: array<vec3<f32>, 9>, bent_normal: vec3<f32>, visibility: f32) -> vec3<f32> {
    let sin_sq = clamp01(visibility);
    let cos_sq = 1.0 - sin_sq;
    let kernel = vec3<f32>(
        (sqrt(1.0 * PI) / 2.0) * sin_sq,
        (sqrt(3.0 * PI) / 3.0) * (1.0 - cos_sq * sqrt(cos_sq)),
        (sqrt(5.0 * PI) / 16.0) * sin_sq * (2.0 + 6.0 * cos_sq),
    );
    let mul = sqrt(4.0 * PI / vec3<f32>(1.0, 3.0, 5.0)) * kernel;
    let c = sh_coeff_order_2(bent_normal);
    var result = c[0] * sh[0] * mul.x;
    for (var i = 1; i < 4; i++) { result += c[i] * sh[i] * mul.y; }
    for (var i = 4; i < 9; i++) { result += c[i] * sh[i] * mul.z; }
    return result;
}

// ---------------------------------------------------------------------------------------------------------------------
//   Passes
// ---------------------------------------------------------------------------------------------------------------------

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) ndc: vec2<f32>,
    @location(1) @interpolate(flat) light_color: vec3<f32>,
    @location(2) @interpolate(flat) ambient_color: vec3<f32>,
    @location(3) @interpolate(flat) sun_color: vec3<f32>,
    @location(4) @interpolate(flat) moon_color: vec3<f32>,
};

// One triangle over the screen at depth 1, with d0.vsh's per-frame colours (flat, so the same on all three vertices).
@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    var o: VsOut;
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0;
    o.pos = vec4<f32>(p, 1.0, 1.0);
    o.ndc = p;
    o.sun_color = get_sun_exposure() * get_sun_tint();
    o.moon_color = get_moon_exposure() * get_moon_tint();
    o.light_color = get_light_color();
    o.ambient_color = get_ambient_color(o.sun_color, o.moon_color);
    return o;
}

// d0_sky_map: the whole sky around the viewer in 192 x 108 texels, for reflections and SH lighting.
@fragment
fn fs_d0(in: VsOut) -> @location(0) vec4<f32> {
    let texel = vec2<i32>(in.pos.xy);
    if texel.x == 191 { // lighting colours
        if texel.y == 0 { return vec4<f32>(in.light_color, 1.0); }
        if texel.y == 1 { return vec4<f32>(in.ambient_color, 1.0); }
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let ray_dir = unproject_sky(in.pos.xy / vec2<f32>(192.0, 108.0));
    // clouds, aurora and the analytic fog over the sky go here (milestones 4 and 6)
    return vec4<f32>(atmosphere_scattering(ray_dir, in.sun_color, in.moon_color), 1.0);
}

// Temporary display of d4's sky branch, drawn where the depth buffer is still 1. The scene is not HDR yet (milestone 1)
// and has no tone map (milestone 5), so the pack's exposure and ACES colour grading are stood in for by a fixed exposure
// and the Narkowicz ACES fit on Rec. 709; the surface does the sRGB encoding. ponytail: delete with c19.
fn stand_in_tonemap(c: vec3<f32>) -> vec3<f32> {
    // Rec. 2020 -> Rec. 709
    let x = vec3<f32>(
        dot(vec3<f32>(1.6604, -0.5876, -0.0729), c),
        dot(vec3<f32>(-0.1244, 1.1328, -0.0083), c),
        dot(vec3<f32>(-0.0181, -0.1006, 1.1186), c),
    ) * 1.0;
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs_sky(in: VsOut) -> @location(0) vec4<f32> {
    let p = sky.inv_vp * vec4<f32>(in.ndc, 1.0, 1.0);
    let ray_dir = normalize(p.xyz / p.w);
    let atmosphere = atmosphere_scattering(ray_dir, in.sun_color, in.moon_color);
    return vec4<f32>(stand_in_tonemap(draw_sky(ray_dir, atmosphere, vec4<f32>(0.0, 0.0, 0.0, 1.0), in.sun_color)), 1.0);
}
