//! Photon (SixthSurge) port, milestone 1: G-buffer + deferred lighting into the HDR frame. Written WITHOUT a Rust toolchain.
//!
//! Frame order (`lib.rs::render`): G-buffer pass (terrain, items, outline: albedo + packed light + face normal) ->
//! `render::vl` march (kept as it was; it also yields the surface shadow in its green channel) -> HDR scene pass: sky,
//! `fs_deferred`, water, the light shafts' blend, all into `post::Post::scene_view()` -> the post chain (`render::post`: TAA, exposure,
//! bloom, grading, tone mapping) ends on the swap chain -> the HUD. (M1's own tone map and grading were dropped when M5 landed.)
//!
//! Ported line by line (names kept):
//! - `d4_deferred_shading.fsh` terrain branch + `diffuse_lighting.glsl` `get_diffuse_lighting` / `get_block_lighting` /
//!   `get_sky_lighting` / `get_blocklight_falloff` with the settings' defaults, without SSS, sheen, AO, bent normals, cloud
//!   shadows, coloured lights, handheld light and Photonics (none of those inputs exist yet: AO is `d3_ao`, clouds `d1`..);
//! - `light_color.glsl` (`get_light_color`, sun/moon exposure and tint), `atmosphere.glsl` (`atmosphere_transmittance`
//!   analytic branch, the LUT mapping of `atmosphere_scattering` for sun + moon, `atmosphere_post_processing`) and
//!   `d0_sky_map.vsh` `get_ambient_color`, evaluated on the CPU once per frame from the pack's own `scattering.dat`
//!   (`assets/scattering.dat`, RGB16F 32 x 64 x 32, trilinear, clamped);
//!
//! Deviations (UNVERIFIED on a device, nothing was compiled or run):
//! - the sky pass is still mc-rs's and writes vanilla-style LDR colours into the HDR frame (milestone 2: `d0`, `d4a`, `d4` sky);
//! - ambient light is `ambient_color * sqr(sky)` as the pack's non-SH branch (`PROGRAM_DEFERRED4 && SH_SKYLIGHT` reads the sky SH
//!   of the `d4a` compute pass, milestone 2);
//! - light levels are the mesher's cell light, raw sky (0..15) + block (0..15) packed in `Vertex.lm`, /15; the pack reads
//!   Minecraft's `lmcoord`, which is the same pair, and the normal is the axis the face looks along (`dpdx`/`dpdy` of the world
//!   position), photon's `flat_normal`; there is no normal map;
//! - the shadow is the one `render::vl` already computes at half resolution (bilinear, `PCSS`/`SSRT` are milestone 3);
//! - rain: no cloud shadows yet, so the direct light is scaled by `1 - 0.9 rain`; there is one moon phase (b1.7.3): full;
//! - not ported yet: white balance (6500 K, the default, is the identity), bloom, TAA, DOF, motion blur, FXAA, purkinje shift.

use glam::Vec3;
use wgpu::{BindGroup, BindGroupLayout, Device, RenderPipeline, Sampler, TextureView};

use crate::world::sky;

/// Format of the G-buffer and the HDR scene (`colortex0` is RGB16F in the pack).
pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

// ------------------------------------------------------------------------------------------------------------------
//   CPU side: the per-frame colours (`light_color`, `ambient_color`, `blocklight_color`, `time_noon`)
// ------------------------------------------------------------------------------------------------------------------

/// `scattering.dat`: RGB16F, x = 32 (nu: rayleigh+multiple scattering in 0..16, mie in 16..32), y = 64 (mu), z = 32 (mu_s).
static LUT: &[u8] = include_bytes!("../../assets/scattering.dat");

/// Column-major 3x3 as GLSL's `mat3(a, b, c, ...)` is written; `vm` is GLSL's `v * M`.
type M3 = [f64; 9];
const XYZ_TO_REC2020: M3 = [1.7166084, -0.3556621, -0.2533601, -0.6666829, 1.6164776, 0.0157685, 0.0176422, -0.0427763, 0.94222867];
const REC709_TO_XYZ: M3 = [0.4124, 0.3576, 0.1805, 0.2126, 0.7152, 0.0722, 0.0193, 0.1192, 0.9505];

fn vm(v: [f64; 3], m: &M3) -> [f64; 3] {
    std::array::from_fn(|j| v[0] * m[3 * j] + v[1] * m[3 * j + 1] + v[2] * m[3 * j + 2])
}
/// `from_srgb` of the pack: `pow(x, 2.2) * rec709_to_rec2020`.
fn from_srgb(c: [f64; 3]) -> [f64; 3] {
    vm(vm(c.map(|x| x.powf(2.2)), &REC709_TO_XYZ), &XYZ_TO_REC2020)
}
fn to_working(c: [f64; 3]) -> [f64; 3] {
    vm(vm(c, &REC709_TO_XYZ), &XYZ_TO_REC2020)
}

const SUNLIGHT_COLOR: [f64; 3] = [1.051, 0.985, 0.940];
const LUMINANCE_REC2020: [f64; 3] = [0.2627, 0.6780, 0.0593];
const PLANET_R: f64 = 6371e3;
const INNER_R: f64 = PLANET_R - 1e3;
const OUTER_R: f64 = PLANET_R + 110e3;
const MIN_MU_S: f64 = -0.35;
const SCALE_HEIGHTS: [f64; 2] = [8.4e3, 1.25e3];
const SUN_ANGULAR_RADIUS: f64 = 2.0 * std::f64::consts::PI / 180.0;
const MOON_ANGULAR_RADIUS: f64 = 3.0 * std::f64::consts::PI / 180.0;

fn mix(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}
fn clamp01(x: f64) -> f64 {
    x.clamp(0.0, 1.0)
}
fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = clamp01((x - a) / (b - a));
    t * t * (3.0 - 2.0 * t)
}
fn linear_step(a: f64, b: f64, x: f64) -> f64 {
    clamp01((x - a) / (b - a))
}
/// `pulse(x, center, width)` of `global.glsl`.
fn pulse(x: f64, center: f64, width: f64) -> f64 {
    let t = (x - center).abs() / width;
    if t > 1.0 { 0.0 } else { 1.0 - t * t * (3.0 - 2.0 * t) }
}
fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn f16_to_f64(b: u16) -> f64 {
    let (s, e, m) = ((b >> 15) & 1, ((b >> 10) & 31) as i32, (b & 1023) as f64);
    let v = match e {
        0 => m * 2f64.powi(-24),
        31 => if m == 0.0 { f64::INFINITY } else { f64::NAN },
        _ => (1.0 + m / 1024.0) * 2f64.powi(e - 15),
    };
    if s == 1 { -v } else { v }
}

fn lut_texel(x: usize, y: usize, z: usize) -> [f64; 3] {
    let i = ((z * 64 + y) * 32 + x) * 6;
    std::array::from_fn(|c| f16_to_f64(u16::from_le_bytes([LUT[i + 2 * c], LUT[i + 2 * c + 1]])))
}

/// `texture(sampler3D, uv)` with linear filtering and clamp-to-edge.
fn lut_sample(uv: [f64; 3]) -> [f64; 3] {
    let dims = [32usize, 64, 32];
    let p: [f64; 3] = std::array::from_fn(|a| (uv[a] * dims[a] as f64 - 0.5).clamp(0.0, (dims[a] - 1) as f64));
    let (i, f): ([usize; 3], [f64; 3]) = (std::array::from_fn(|a| p[a].floor() as usize), std::array::from_fn(|a| p[a] - p[a].floor()));
    let mut out = [0.0; 3];
    for corner in 0..8 {
        let (dx, dy, dz) = (corner & 1, (corner >> 1) & 1, (corner >> 2) & 1);
        let w = (if dx == 1 { f[0] } else { 1.0 - f[0] }) * (if dy == 1 { f[1] } else { 1.0 - f[1] }) * (if dz == 1 { f[2] } else { 1.0 - f[2] });
        let t = lut_texel((i[0] + dx).min(31), (i[1] + dy).min(63), (i[2] + dz).min(31));
        for c in 0..3 {
            out[c] += w * t[c];
        }
    }
    out
}

fn uv_from_unit_range(v: f64, res: f64) -> f64 {
    v * (1.0 - 1.0 / res) + 0.5 / res
}
/// `intersect_sphere(mu, r, radius)` of `geometry.glsl`.
fn intersect_sphere(mu: f64, r: f64, radius: f64) -> (f64, f64) {
    let disc = r * r * (mu * mu - 1.0) + radius * radius;
    if disc < 0.0 {
        return (-1.0, -1.0);
    }
    let d = disc.sqrt();
    (-r * mu - d, -r * mu + d)
}

/// `atmosphere_scattering_uv`'s three coordinates for the sun or the moon, as `atmosphere_scattering(ray, sun.., moon..)` builds them.
fn lut_uv(ray_y: f64, mu_light: f64, nu: f64) -> [f64; 3] {
    let h = (OUTER_R * OUTER_R - INNER_R * INNER_R).max(0.0).sqrt();
    let rho = (PLANET_R * PLANET_R - INNER_R * INNER_R).max(0.0).sqrt();
    // nu (before the horizon stretch of mu)
    let half = ((1.0 - ray_y * ray_y) * (1.0 - mu_light * mu_light)).sqrt();
    let (lo, hi) = (ray_y * mu_light - half, ray_y * mu_light + half);
    let u_nu = uv_from_unit_range(if lo == hi { lo } else { (nu - lo) / (hi - lo) }, 16.0);
    // mu
    let mut mu = ray_y;
    if mu > 0.0 {
        mu *= mu.sqrt().sqrt();
    }
    let rmu = PLANET_R * mu;
    let disc = rmu * rmu - PLANET_R * PLANET_R + INNER_R * INNER_R;
    let u_mu = if mu < 0.0 && disc >= 0.0 {
        let d = -rmu - disc.max(0.0).sqrt();
        let (d_min, d_max) = (PLANET_R - INNER_R, rho);
        let u = if d_max == d_min { 0.0 } else { (d - d_min) / (d_max - d_min) };
        0.5 - 0.5 * uv_from_unit_range(u, 32.0)
    } else {
        let d = -rmu + (disc + h * h).sqrt();
        let (d_min, d_max) = (OUTER_R - PLANET_R, rho + h);
        0.5 + 0.5 * uv_from_unit_range((d - d_min) / (d_max - d_min), 32.0)
    };
    // mu_s
    let (d_min, d_max) = (OUTER_R - INNER_R, h);
    let big_a = (intersect_sphere(MIN_MU_S, INNER_R, OUTER_R).1 - d_min) / (d_max - d_min);
    let a = (intersect_sphere(mu_light, INNER_R, OUTER_R).1 - d_min) / (d_max - d_min);
    let u_mu_s = uv_from_unit_range((1.0 - a / big_a).max(0.0) / (1.0 + a), 32.0);
    [u_nu, u_mu, u_mu_s]
}

fn rayleigh() -> [f64; 3] {
    to_working([8.059375432e-06, 1.671209429e-05, 4.080133294e-05])
}
fn mie() -> [f64; 3] {
    to_working([1.666442358e-06, 1.812685127e-06, 1.958927896e-06])
}
fn ozone() -> [f64; 3] {
    to_working([8.304280072e-07, 1.314911970e-06, 5.440679729e-08])
}
/// `chapman_function_approx`.
fn chapman(x: f64, cos_theta: f64) -> f64 {
    let c = (std::f64::consts::FRAC_PI_2 * x).sqrt();
    if cos_theta >= 0.0 {
        c / ((c - 1.0) * cos_theta + 1.0)
    } else {
        let sin = clamp01(1.0 - cos_theta * cos_theta).sqrt();
        c / ((c - 1.0) * cos_theta - 1.0) + 2.0 * c * (x - x * sin).exp() * sin.sqrt()
    }
}

/// `atmosphere_transmittance(mu, r)`, the analytic branch (`ATMOSPHERE_TRANSMITTANCE_LUT` is not defined for the light colour).
fn atmosphere_transmittance(mu: f64, r: f64) -> [f64; 3] {
    if intersect_sphere(mu, r.max(PLANET_R + 10.0), PLANET_R).0 >= 0.0 {
        return [0.0; 3];
    }
    let density = [(-(r - PLANET_R) / SCALE_HEIGHTS[0]).exp(), (-(r - PLANET_R) / SCALE_HEIGHTS[1]).exp()];
    let airmass = [
        SCALE_HEIGHTS[0] * density[0] * chapman(r / SCALE_HEIGHTS[0], mu),
        SCALE_HEIGHTS[1] * density[1] * chapman(r / SCALE_HEIGHTS[1], mu),
    ];
    let (ra, mi, oz) = (rayleigh(), mie(), ozone());
    // air_extinction_coefficients * airmass.xyx
    std::array::from_fn(|c| clamp01((-(ra[c] * airmass[0] + mi[c] * airmass[1] + oz[c] * airmass[0])).exp()))
}

/// `atmosphere_post_processing` (`ATMOSPHERE_SATURATION_BOOST` on, `ATMOSPHERE_RAIN_DESATURATION` off).
fn atmosphere_post(c: [f64; 3], boost: f64) -> [f64; 3] {
    let l = dot3(c, LUMINANCE_REC2020);
    c.map(|x| mix(l, x, 1.0 + 1.0 * boost))
}

/// `nvidia_phase_area`.
fn nvidia_phase_area(nu: f64, g: f64, a: f64, radius: f64) -> f64 {
    let radius = radius.max(1e-6);
    let (cosr, sinr) = (radius.cos(), radius.sin());
    let mut mu = nu * cosr + (1.0 - nu * nu).max(0.0).sqrt() * sinr;
    if nu > cosr {
        mu = 1.0;
    }
    let gg = g * g;
    let base = 1.0 + gg - 2.0 * g * mu;
    ((1.0 - gg) * (1.0 + a * mu * mu)) / (std::f64::consts::PI * base * base.sqrt() * 4.0 * (1.0 + a * (1.0 + 2.0 * gg) / 3.0))
}

/// `atmosphere_scattering(ray, sun_color, sun_dir, moon_color, moon_dir, ..)`, the sun + moon version.
fn atmosphere_scattering(ray: [f64; 3], sun_color: [f64; 3], sun: [f64; 3], moon_color: [f64; 3], moon: [f64; 3], boost: f64) -> [f64; 3] {
    let (nu_sun, nu_moon) = (dot3(ray, sun), dot3(ray, moon));
    let horizon_mu = mix(-0.01, 0.03, clamp01(smoothstep(-0.05, 0.1, sun[1]) + smoothstep(0.05, 0.1, moon[1])));
    let mu = ray[1].max(horizon_mu);
    let (uv_s, uv_m) = (lut_uv(mu, sun[1], nu_sun), lut_uv(mu, moon[1], nu_moon));
    let tex = |u: [f64; 3], mie_half: bool| lut_sample([u[0] * 0.5 + if mie_half { 0.5 } else { 0.0 }, u[1], u[2]]);
    let (sc, sm, mc, mm) = (tex(uv_s, false), tex(uv_s, true), tex(uv_m, false), tex(uv_m, true));
    // atmosphere_mie_phase_sun / _moon (moon phase 0: t = 1)
    let phase_sun = [nvidia_phase_area(nu_sun, 0.85, 1.0, SUN_ANGULAR_RADIUS), nvidia_phase_area(nu_sun, 0.86, 1.0, SUN_ANGULAR_RADIUS), nvidia_phase_area(nu_sun, 0.87, 1.0, SUN_ANGULAR_RADIUS)].map(|p| p * 0.2);
    let phase_moon = [nvidia_phase_area(nu_moon, 0.895, 1.0, 2.0 * MOON_ANGULAR_RADIUS), nvidia_phase_area(nu_moon, 0.90, 1.0, 2.0 * MOON_ANGULAR_RADIUS), nvidia_phase_area(nu_moon, 0.905, 1.0, 2.0 * MOON_ANGULAR_RADIUS)].map(|p| p * 1.02);
    let a: [f64; 3] = std::array::from_fn(|c| (sc[c] + sm[c] * phase_sun[c]) * sun_color[c] + (mc[c] + mm[c] * phase_moon[c]) * moon_color[c]);
    atmosphere_post(a, boost)
}

/// What the shaders need once per frame, all in the working space (Rec. 2020), as `light_color`, `ambient_color`,
/// `blocklight_color`, `time_noon` and `light_dir.y` are in the pack.
#[derive(Clone, Copy, Debug)]
pub struct Lighting {
    pub light_color: [f32; 3],
    pub ambient: [f32; 3],
    pub blocklight: [f32; 3],
    pub noon: f32,
    pub exposure: f32,
}

/// `angle` = `sky::celestial_angle` (0 noon, 0.25 sunset, 0.5 midnight), `rain` 0..1, `light` = the light the shadow map was
/// drawn for (`render::vl::Params::light`: the sun, or the moon at night), so the direct light and the shadow agree.
pub fn lighting(angle: f32, rain: f32, light: Vec3) -> Lighting {
    let sun = sky::sun_dir(angle).as_dvec3();
    let sun = [sun.x, sun.y, sun.z];
    let moon = sun.map(|x| -x);
    let l = light.as_dvec3();
    let y = sun[1];
    let rain = rain as f64;
    // Time uniforms of shaders.properties. sun_dir.x > 0 is "rising": the sun's y grows from angle 0.5 to 1.
    let rising = angle >= 0.5;
    let me_fade = if y < 0.18 { 0.37 + 1.2 * (-y).max(0.0) } else { 1.7 };
    let me_weight = clamp01(1.0 - me_fade * (y - 0.18).abs()).powi(2);
    let (time_sunrise, time_sunset) = if rising { (me_weight, 0.0) } else { (0.0, me_weight) };
    let time_noon = if y > 0.0 { 1.0 - me_weight } else { 0.0 };
    let sat_boost = 0.10 + 0.20 * (-150.0 * (y + 0.07283) * (y + 0.07283)).exp(); // atmosphere_saturation_boost_amount

    // light_color.glsl
    let blue_hour = linear_step(0.05, 1.0, (-190.0 * (y + 0.09604) * (y + 0.09604)).exp());
    let sun_exposure = 7.0 * (1.0 + 0.5 * (time_sunset + time_sunrise) + 40.0 * blue_hour);
    let met = {
        let t = pulse(y, 0.17, 0.40);
        [1.05, 0.84, 0.93].map(|c| mix(1.0, c * 1.2, t * t))
    };
    let bh = [0.95, 0.80, 1.0].map(|c| mix(1.0, c, blue_hour));
    let user = from_srgb([1.0; 3]); // SUN_* tints are all 1.0
    let sun_tint: [f64; 3] = std::array::from_fn(|c| met[c] * bh[c] * user[c]);
    let sun_color: [f64; 3] = sun_tint.map(|t| t * sun_exposure);
    let moon_exposure = 0.66 * 1.0 * (1.0 + 0.33 / clamp01(1.25 * (-y).max(0.1))); // moon_phase_brightness = 1
    let moon_color: [f64; 3] = from_srgb([0.75, 0.83, 1.0]).map(|t| t * moon_exposure);

    let night = dot3(l, sun) < 0.0; // `step(0.5, sunAngle)` for the light the shadow map uses
    let trans = atmosphere_transmittance(l[1], PLANET_R);
    let base: [f64; 3] = std::array::from_fn(|c| SUNLIGHT_COLOR[c] * trans[c]);
    let mut lc = atmosphere_post(base, sat_boost);
    let fade = clamp01(l[1] / 0.02) * (1.0 - 0.25 * pulse(l[1].abs(), 0.15, 0.11)) * (1.0 - 0.9 * rain);
    for c in 0..3 {
        lc[c] *= (if night { moon_color[c] } else { sun_color[c] }) * fade;
    }

    // d0_sky_map.vsh get_ambient_color
    let n = (0.0f64 * 0.0 + 1.0 + 0.8 * 0.8).sqrt();
    let sky_dir = [0.0, 1.0 / n, -0.8 / n];
    let sky_col = atmosphere_scattering(sky_dir, sun_color, sun, moon_color, moon, sat_boost);
    let weather: [f64; 3] = std::array::from_fn(|c| mix(0.033, 0.66, smoothstep(-0.1, 0.5, y)) * SUNLIGHT_COLOR[c] * [0.49, 0.65, 1.00][c]);
    let tau = std::f64::consts::TAU;
    let ambient: [f64; 3] = std::array::from_fn(|c| mix(tau * sky_col[c] * 1.13, tau * weather[c], rain));

    let blk = from_srgb([1.0, 0.75, 0.63]); // BLOCKLIGHT_R/G/B x BLOCKLIGHT_I
    Lighting {
        light_color: lc.map(|x| x as f32),
        ambient: ambient.map(|x| x as f32),
        blocklight: blk.map(|x| x as f32),
        noon: time_noon as f32,
        exposure: 1.0, // get_exposure_from_ev_100(MANUAL_EXPOSURE_VALUE = 0)
    }
}

// ------------------------------------------------------------------------------------------------------------------
//   GPU side
// ------------------------------------------------------------------------------------------------------------------

/// WGSL shared by the chunk shader (water) and this module: the uniform block, colour spaces, fog and the lighting function.
/// `gpu::pipeline::Uniforms` is its Rust twin: keep them in step.
pub const WGSL_COMMON: &str = r#"
struct Uniforms {
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    shadow_vp: mat4x4<f32>,
    sun: vec4<f32>,   // xyz: toward the light that casts the shadow, w: shadow strength
    eye: vec4<f32>,
    plant: array<vec4<u32>, 2>,
    fog: vec4<f32>,   // rgb fog colour, w far (0 = off)
    fogp: vec4<f32>,  // x rain
    lcol: vec4<f32>,  // rgb light_color, w exposure
    amb: vec4<f32>,   // rgb ambient_color, w time_noon
    blk: vec4<f32>,   // rgb blocklight_color
};
@group(0) @binding(0) var<uniform> u: Uniforms;

const PI: f32 = 3.14159265358979;
// utility/color.glsl: Rec. 709 <-> XYZ <-> Rec. 2020, written as the GLSL constructors (`v * M` is the same in WGSL)
const R709_TO_XYZ = mat3x3<f32>(vec3<f32>(0.4124, 0.3576, 0.1805), vec3<f32>(0.2126, 0.7152, 0.0722), vec3<f32>(0.0193, 0.1192, 0.9505));
const XYZ_TO_2020 = mat3x3<f32>(vec3<f32>(1.7166084, -0.3556621, -0.2533601), vec3<f32>(-0.6666829, 1.6164776, 0.0157685), vec3<f32>(0.0176422, -0.0427763, 0.94222867));
const R2020_TO_XYZ = mat3x3<f32>(vec3<f32>(0.6369736, 0.1446172, 0.1688585), vec3<f32>(0.2627066, 0.6779996, 0.0592938), vec3<f32>(0.0, 0.0280728, 1.0608437));
const XYZ_TO_709 = mat3x3<f32>(vec3<f32>(3.2406, -1.5372, -0.4986), vec3<f32>(-0.9689, 1.8758, 0.0415), vec3<f32>(0.0557, -0.2040, 1.0570));
fn to_working(c: vec3<f32>) -> vec3<f32> { return (c * R709_TO_XYZ) * XYZ_TO_2020; }
fn to_display(c: vec3<f32>) -> vec3<f32> { return (c * R2020_TO_XYZ) * XYZ_TO_709; }

// diffuse_lighting.glsl, settings.glsl defaults
const SHADING_STRENGTH: f32 = 1.0;
const BOUNCED_LIGHT_I: f32 = 1.0;
const CAVE_LIGHTING_I: f32 = 1.0;
const BLOCKLIGHT_SCALE: f32 = 6.0; // blocklight_color.glsl
// AstraLex `OVERWORLD_FOG_DENSITY`: the pack's default 0.5 only fogs the last 6 blocks of a 64 block view, too late here.
const FOG_DENSITY: f32 = 2.0;

fn dampen(x: f32) -> f32 { let c = clamp(x, 0.0, 1.0); return c * (2.0 - c); }
fn lift(x: f32, amount: f32) -> f32 { return (x + x * amount) / (1.0 + x * amount); }

// Face index 0..5 = +x -x +y -y +z -z from the screen-space derivatives of the world position (every face here is axis
// aligned): photon's `flat_normal`.
fn face_index(wp: vec3<f32>, dx: vec3<f32>, dy: vec3<f32>) -> u32 {
    let c = cross(dx, dy);
    if dot(c, c) < 1e-20 { return 2u; }
    var n = normalize(c);
    if dot(n, u.eye.xyz - wp) < 0.0 { n = -n; }
    let a = abs(n);
    if a.x >= a.y && a.x >= a.z { return select(1u, 0u, n.x > 0.0); }
    if a.y >= a.z { return select(3u, 2u, n.y > 0.0); }
    return select(5u, 4u, n.z > 0.0);
}
fn face_normal(i: u32) -> vec3<f32> {
    var n = array<vec3<f32>, 6>(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(-1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, -1.0, 0.0), vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(0.0, 0.0, -1.0));
    return n[i];
}

// get_blocklight_falloff with ao = 1 and darknessFactor = 0
fn blocklight_falloff(bl: f32, sky: f32) -> f32 {
    var f = pow(bl, 8.0) + 0.18 * bl * bl + 0.16 * dampen(bl);
    f = f * (1.0 - 0.2 * u.amb.w * sky - 0.2 * sky);
    f = f + min(2.7 * pow(bl, 12.0), 0.9);
    return f * smoothstep(0.0, 0.125, bl);
}

// get_diffuse_lighting, overworld, SHADOW_VPS, no SSS / AO / cloud shadows. `albedo` is in the working space, `sky` and `bl`
// are the light levels / 15, `shadow` is 1 where the light arrives.
fn photon_light(albedo: vec3<f32>, face: u32, sky: f32, bl: f32, shadow_in: f32) -> vec3<f32> {
    let n = face_normal(face);
    let nol = dot(n, u.sun.xyz);
    let directional = (0.9 + 0.1 * n.x) * (0.8 + 0.2 * abs(n.y));
    let shadow = select(0.0, shadow_in, nol > 1e-3);
    let diffuse = lift(max(nol, 0.0), 0.25 / SHADING_STRENGTH);
    let bounced = 0.033 * (1.0 - shadow) * (1.0 - 0.1 * max(n.y, 0.0)) * pow(1.0 + 1e-6, 1.5) * pow(sky, 4.0) * BOUNCED_LIGHT_I;
    var lighting = (vec3<f32>(diffuse * shadow + bounced)) * u.lcol.rgb;
    lighting = lighting + u.amb.rgb * (sky * sky);                                    // get_sky_lighting: ambient_color * sqr(skylight)
    lighting = lighting + (blocklight_falloff(bl, sky) * directional * BLOCKLIGHT_SCALE) * u.blk.rgb; // get_block_lighting
    lighting = lighting + vec3<f32>(0.15 * CAVE_LIGHTING_I * directional * (1.0 - sky * sky));        // cave lighting
    return max(lighting, vec3<f32>(0.0)) * albedo * (1.0 / PI);
}

// fog.glsl `NormalFog`, `DISTANT_FADE` vanilla part with `fogOffset = 0`: 1 - (far - d) * 5 / (density * far). Rain thickens it.
fn apply_fog(rgb: vec3<f32>, d: f32) -> vec3<f32> {
    if u.fog.w <= 0.0 { return rgb; }
    let f = clamp(1.0 - (u.fog.w - d) * 5.0 / (FOG_DENSITY * (1.0 + 0.5 * u.fogp.x) * u.fog.w), 0.0, 1.0);
    return mix(rgb, u.fog.rgb, f * f * (3.0 - 2.0 * f));
}
"#;

const SHADER: &str = r#"
@group(1) @binding(0) var gbuf: texture_2d<f32>;
@group(1) @binding(1) var depth_tex: texture_depth_2d;
@group(1) @binding(2) var vl_tex: texture_2d<f32>;
@group(1) @binding(3) var samp: sampler;

struct FsIn {
    @builtin(position) pos: vec4<f32>,
};

// One triangle over the whole target.
@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> FsIn {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: FsIn;
    o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    return o;
}

// d4_deferred_shading.fsh, terrain branch. Sky pixels (depth 1) keep what the sky pass drew.
@fragment
fn fs_deferred(in: FsIn) -> @location(0) vec4<f32> {
    let ip = vec2<i32>(in.pos.xy);
    let z = textureLoad(depth_tex, ip, 0);
    if z >= 1.0 { discard; }
    let g = textureLoad(gbuf, ip, 0);
    // alpha = face * 256 + sky * 16 + block (`Vertex.lm`, exact in f16)
    let face = u32(floor(g.a / 256.0 + 0.001));
    let lm = g.a - f32(face) * 256.0;
    let sky_i = floor(lm / 16.0 + 0.001);
    let sky = sky_i / 15.0;
    let bl = (lm - sky_i * 16.0) / 15.0;
    let dims = vec2<f32>(textureDimensions(depth_tex));
    let uv = in.pos.xy / dims;
    // The surface shadow `render::vl`'s march wrote (g, 1 = in shadow), bilinear from half resolution.
    let shadow = 1.0 - textureSampleLevel(vl_tex, samp, uv, 0.0).g;
    let rgb = photon_light(to_working(g.rgb), face, sky, bl, shadow);
    // view-space distance for the fog: z_view = -B / (z + A) for glam's depth 0..1 projection
    let zv = -u.proj[3][2] / (z + u.proj[2][2]);
    let nd = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let d = length(vec3<f32>(nd.x * -zv / u.proj[0][0], nd.y * -zv / u.proj[1][1], zv));
    return vec4<f32>(apply_fog(rgb, d), 1.0);
}

"#;

/// Size-dependent targets + the two full-screen pipelines.
pub struct Photon {
    size: (u32, u32),
    /// albedo (linear Rec. 709) in rgb, `face * 256 + lm` in a.
    pub gbuf: TextureView,
    deferred: RenderPipeline,
    deferred_layout: BindGroupLayout,
    samp: Sampler,
    deferred_bind: Option<BindGroup>,
}

fn gbuffer(device: &Device, w: u32, h: u32) -> TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("gbuffer"),
            size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

impl Photon {
    /// `uniform_layout` is `ChunkPipeline::bind_layout` (group 0: the shared uniform block).
    pub fn new(device: &Device, uniform_layout: &BindGroupLayout, w: u32, h: u32) -> Self {
        let gbuf = gbuffer(device, w, h);
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty, count: None };
        let tex = |sample_type| wgpu::BindingType::Texture { sample_type, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false };
        let deferred_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("photon_deferred_layout"),
            entries: &[
                entry(0, tex(wgpu::TextureSampleType::Float { filterable: false })),
                entry(1, tex(wgpu::TextureSampleType::Depth)),
                entry(2, tex(wgpu::TextureSampleType::Float { filterable: true })),
                entry(3, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
            ],
        });
        let samp = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let src = format!("{WGSL_COMMON}{SHADER}");
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("photon_shader"), source: wgpu::ShaderSource::Wgsl(src.into()) });
        let pipeline = |label, group1: &BindGroupLayout, fs: &str, format, depth: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(label), bind_group_layouts: &[uniform_layout, group1], push_constant_ranges: &[] })),
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_full"), buffers: &[], compilation_options: wgpu::PipelineCompilationOptions::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                // The scene pass has the terrain's depth attached read-only (the shader reads it as a texture too).
                depth_stencil: depth.then(|| wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::Always,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let deferred = pipeline("photon_deferred", &deferred_layout, "fs_deferred", HDR_FORMAT, true);
        Self { size: (w, h), gbuf, deferred, deferred_layout, samp, deferred_bind: None }
    }

    /// Call every frame with the frame's size, before the G-buffer pass.
    pub fn resize(&mut self, device: &Device, w: u32, h: u32) {
        if self.size != (w, h) {
            self.gbuf = gbuffer(device, w, h);
            self.size = (w, h);
            self.deferred_bind = None;
        }
    }

    /// The depth view was recreated (`Gpu::resize`), possibly at the same size: drop the bind groups so `bind` makes them again.
    pub fn invalidate(&mut self) {
        self.deferred_bind = None;
    }

    /// Call every frame after `LightShafts::march` (it makes its target on the first frame and on a resize); the bind groups
    /// are only rebuilt when `resize` dropped them. `depth` is `Gpu::depth_view`, `vl` the march target.
    pub fn bind(&mut self, device: &Device, depth: &TextureView, vl: &TextureView) {
        if self.deferred_bind.is_none() {
            self.deferred_bind = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("photon_deferred_bind"),
                layout: &self.deferred_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&self.gbuf) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(depth) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(vl) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.samp) },
                ],
            }));
        }
    }

    /// Inside the scene pass, after the sky: `uniforms` is `ChunkPipeline::bind_group`.
    pub fn draw_deferred(&self, rp: &mut wgpu::RenderPass<'_>, uniforms: &BindGroup) {
        let Some(b) = &self.deferred_bind else { return };
        rp.set_pipeline(&self.deferred);
        rp.set_bind_group(0, uniforms, &[]);
        rp.set_bind_group(1, b, &[]);
        rp.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The embedded table is the pack's `scattering.dat` (32 x 64 x 32 x RGB16F), and decoding it gives light.
    #[test]
    fn lut_is_the_pack_table() {
        assert_eq!(LUT.len(), 32 * 64 * 32 * 3 * 2);
        assert!(f16_to_f64(0x3c00) == 1.0 && f16_to_f64(0xc000) == -2.0);
        let up = lut_sample(lut_uv(1.0, 1.0, 0.0));
        assert!(up.iter().all(|c| c.is_finite() && *c >= 0.0), "{up:?}");
    }

    /// Light colour: bright and slightly warm at noon, gone with the sun below the horizon, the moon dim and blue at night.
    #[test]
    fn light_colours_follow_the_time_of_day() {
        let at = |angle: f32, rain: f32| {
            let sun = sky::sun_dir(angle);
            lighting(angle, rain, if sun.y >= 0.0 { sun } else { -sun })
        };
        let noon = at(0.0, 0.0);
        assert!(noon.light_color.iter().all(|c| c.is_finite() && *c > 0.0), "{noon:?}");
        assert!(noon.light_color[0] >= noon.light_color[2], "noon sunlight is not bluer than red: {:?}", noon.light_color);
        assert!(noon.ambient.iter().all(|c| c.is_finite() && *c > 0.0) && noon.ambient[2] > noon.ambient[0], "blue sky: {:?}", noon.ambient);
        let night = at(0.5, 0.0);
        assert!(night.light_color[1] > 0.0 && night.light_color[1] < 0.2 * noon.light_color[1], "{:?} {:?}", night.light_color, noon.light_color);
        assert!(night.ambient[1] < 0.3 * noon.ambient[1]);
        let rain = at(0.0, 1.0);
        assert!(rain.light_color[1] < 0.2 * noon.light_color[1]);
        // the uniform block of the shader is the Rust one (gpu::pipeline::Uniforms)
        assert!(WGSL_COMMON.contains("blk: vec4<f32>,"));
    }
}
