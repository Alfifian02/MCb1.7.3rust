// Port of Photon `include/fog/simple_fog.glsl` and `include/fog/overworld/analytic.glsl` (overworld, no LoD mod).
// A library for d4_deferred_shading (milestone 1) and the translucent pass (6): it takes what the pack reads from
// globals as arguments. Join it AFTER `clouds_lib.wgsl` (it uses `sqr`, `max0`, `clamp01`, `linear_step`, `dampen`,
// `cube`, `pow8`, `eps`, `isotropic_phase`, `henyey_greenstein_phase`, `SEA_LEVEL`).
// Not ported: `raymarch_air_fog` (that is the pack's volumetric fog, c0_vl: mc-rs keeps its own `render::vl`),
// Nether / End / LPV / water-VL fog, `CAVE_FOG` under `biome_cave` stays (it is a cheap spherical term).

const lava_fog_start: f32 = 0.33;
const lava_fog_density: f32 = 1.0;
const lava_fog_color: vec3<f32> = vec3<f32>(0.92835836, 0.30398008, 0.04835886); // from_srgb(0.839, 0.373, 0.075) * 2.0
const snow_fog_start: f32 = 0.5;
const snow_fog_density: f32 = 1.0;
const snow_fog_color: vec3<f32> = vec3<f32>(0.27974117, 0.29076162, 0.2918364);   // from_srgb(0.957, 0.988, 0.988) * 0.3
const cave_fog_start: f32 = 1.0;
const cave_fog_density: f32 = 0.0033;
const cave_fog_color: vec3<f32> = vec3<f32>(0.033);
const blindness_fog_start: f32 = 2.0;
const blindness_fog_density: f32 = 1.0;
const darkness_fog_start: f32 = 8.0;
const darkness_fog_density: f32 = 2.0;

// what simple_fog reads from uniforms
struct FogEnv {
    far: f32,
    blindness: f32,
    darkness: f32,
    biome_cave: f32,
    eye_in_water: u32, // isEyeInWater: 0 air, 1 water, 2 lava, 3 powder snow
}

fn spherical_fog(view_dist: f32, fog_start_distance: f32, fog_density: f32) -> f32 {
    return exp2(-fog_density * max0(view_dist - fog_start_distance));
}

fn cubic_length(v: vec2<f32>) -> f32 {
    return pow(cube(abs(v.x)) + cube(abs(v.y)), 1.0 / 3.0);
}

fn border_fog(scene_pos: vec3<f32>, world_dir: vec3<f32>, far: f32) -> f32 {
    var fog = cubic_length(scene_pos.xz) / far;
    fog = exp2(-8.0 * pow8(fog));
    let vertical_cutoff_distance = far;
    // WORLD_OVERWORLD
    fog = mix(fog, 1.0, 0.75 * dampen(linear_step(0.0, 0.2, world_dir.y)));
    let vertical_cutoff = linear_step(-vertical_cutoff_distance, -vertical_cutoff_distance + 16.0, scene_pos.y);
    fog *= vertical_cutoff;
    return fog;
}

// rgb = fog colour to add, a = transmittance
fn common_fog(env: FogEnv, view_dist: f32, sky: bool) -> vec4<f32> {
    var fog = vec4<f32>(vec3<f32>(0.0), 1.0);

    // Lava fog
    let lava_fog = spherical_fog(view_dist, lava_fog_start, lava_fog_density * f32(env.eye_in_water == 2u));
    fog = vec4<f32>(fog.rgb + lava_fog_color - lava_fog_color * lava_fog, fog.a * lava_fog);

    // Powdered snow fog
    let snow_fog = spherical_fog(view_dist, snow_fog_start, snow_fog_density * f32(env.eye_in_water == 3u));
    fog = vec4<f32>(fog.rgb + snow_fog_color - snow_fog_color * snow_fog, fog.a * snow_fog);

    // Blindness fog
    fog = fog * mix(1.0, spherical_fog(view_dist, blindness_fog_start, env.blindness * blindness_fog_density), env.blindness);

    // Darkness fog
    fog = fog * mix(1.0, spherical_fog(view_dist, darkness_fog_start, env.darkness * darkness_fog_density), env.darkness);

    // Cave fog (CAVE_FOG)
    let cave_fog = spherical_fog(view_dist, cave_fog_start, cave_fog_density * env.biome_cave * f32(!sky));
    fog = vec4<f32>(fog.rgb + cave_fog_color - cave_fog_color * cave_fog, fog.a * cave_fog);

    return fog;
}

// Calculates the alpha component only
fn common_fog_alpha(env: FogEnv, view_dist: f32, sky: bool) -> f32 {
    var fog = 1.0;
    fog *= spherical_fog(view_dist, lava_fog_start, lava_fog_density * f32(env.eye_in_water == 2u));
    fog *= spherical_fog(view_dist, snow_fog_start, snow_fog_density * f32(env.eye_in_water == 3u));
    fog *= spherical_fog(view_dist, blindness_fog_start, env.blindness * blindness_fog_density);
    fog *= spherical_fog(view_dist, cave_fog_start, cave_fog_density * env.biome_cave * f32(!sky));
    return fog;
}

// Water fog: WATER_ABSORPTION_{R,G,B} * rec709_to_working_color
const water_absorption_coeff: vec3<f32> = vec3<f32>(0.29383101, 0.156457, 0.08140485);
const water_scattering: f32 = 0.01;

fn biome_water_coeff(biome_water_color: vec3<f32>) -> vec3<f32> {
    let density_scale = 0.15;
    let biome_color_contribution = 0.33;
    let base_absorption_coeff = water_absorption_coeff;
    let forest_absorption_coeff = vec3<f32>(0.31251743, 0.25746997, 0.05120463); // -density_scale * log(0.1245, 0.1797, 0.7108)
    // BIOME_WATER_COLOR
    let biome_absorption_coeff = -density_scale * log(biome_water_color + eps) - forest_absorption_coeff;
    return max(base_absorption_coeff + biome_absorption_coeff * biome_color_contribution, vec3<f32>(0.0));
}

struct Fog2 { scattering: vec3<f32>, transmittance: vec3<f32> }

// Simple water fog applied behind water or when volumetric fog is disabled
fn water_fog_simple(
    light_color: vec3<f32>, ambient_color: vec3<f32>, absorption_coeff: vec3<f32>, light_levels: vec2<f32>,
    dist_in: f32, LoV: f32, sss_depth: f32,
    blocklight_color: vec3<f32>, blocklight_scale: f32 // lighting/colors/blocklight_color.glsl
) -> Fog2 {
    let skylight_factor = cube(light_levels.y);
    let scattering_coeff = vec3<f32>(water_scattering);
    let extinction_coeff = scattering_coeff + absorption_coeff;

    // Multiple scattering approximation from Jessie
    let scattering_albedo = scattering_coeff / extinction_coeff;
    let multiple_scattering_factor = 0.84 * scattering_albedo;
    let multiple_scattering_energy = multiple_scattering_factor / (1.0 - multiple_scattering_factor);

    // Minimum distance so that water is always easily visible
    let dist = max(dist_in, 2.0 - 1.0 * skylight_factor);

    var light_ambient = ambient_color * light_levels.y;
    light_ambient += 1.41 * blocklight_color * blocklight_scale * sqr(light_levels.x);

    let transmittance = exp(-extinction_coeff * dist);

    var scattering = light_color * exp(-extinction_coeff * sss_depth) * smoothstep(0.0, 0.25, light_levels.y); // direct lighting
    scattering *= 0.7 * henyey_greenstein_phase(LoV, 0.4) + 0.3 * isotropic_phase; // phase function for direct lighting
    scattering += light_ambient * isotropic_phase; // ambient lighting
    scattering *= (1.0 - transmittance) * scattering_coeff / extinction_coeff; // scattering integral
    scattering *= 1.0 + multiple_scattering_energy; // multiple scattering
    return Fog2(scattering, transmittance);
}

// ---------------------------------------------------------------- fog/overworld/analytic.glsl
const air_fog_falloff_start: vec2<f32> = vec2<f32>(30.0 + SEA_LEVEL, 7.0 + SEA_LEVEL); // rayleigh, mie
const air_fog_falloff_half_life: vec2<f32> = vec2<f32>(30.0, 7.0);
const OVERWORLD_FOG_INTENSITY: f32 = 1.0;

// OverworldFogParameters (weather/fog.glsl, worked out on the CPU by `render::fog::FogParameters`)
struct OverworldFogParameters {
    rayleigh_scattering_coeff: vec3<f32>,
    mie_scattering_coeff: vec3<f32>,
    mie_extinction_coeff: vec3<f32>,
}

// What air_fog_analytic reads from uniforms
struct AirFogEnv {
    far: f32,
    rain_strength: f32,
    blindness: f32,
    darkness: f32,
    eye_skylight: f32,
    sun_dir_y: f32,
    light_dir: vec3<f32>,
    light_color: vec3<f32>,
    ambient_color: vec3<f32>,
}

fn air_fog_analytic_airmass(ray_origin_world: vec3<f32>, ray_direction_world: vec3<f32>, ray_length: f32) -> vec2<f32> {
    // Integral of density function with respect to t
    let mul = -1.0 / air_fog_falloff_half_life;
    let add = -mul * air_fog_falloff_start;
    let a = ray_origin_world.y * mul + add;
    let p1 = exp2(ray_length * ray_direction_world.y * mul + a);
    let p2 = exp2(a);
    return clamp((p1 - p2) / (log(2.0) * mul * ray_direction_world.y), vec2<f32>(0.0), vec2<f32>(ray_length))
        * (0.5 * OVERWORLD_FOG_INTENSITY);
}

// ray_*_world are world positions (y = altitude). Returns scattering and transmittance.
fn air_fog_analytic(
    fp: OverworldFogParameters, env: AirFogEnv,
    ray_origin_world: vec3<f32>, ray_end_world: vec3<f32>, sky: bool, skylight: f32, shadow: f32
) -> Fog2 {
    let fog_end = env.far;

    let delta = ray_end_world - ray_origin_world;
    let len_sq = dot(delta, delta);
    let rcp_len = inverseSqrt(len_sq);
    let ray_direction_world = delta * rcp_len;
    let ray_length = select(len_sq * rcp_len, fog_end, sky);

    let airmass = air_fog_analytic_airmass(ray_origin_world, ray_direction_world, ray_length);
    let optical_depth = fp.rayleigh_scattering_coeff * airmass.x + fp.mie_extinction_coeff * airmass.y;
    let transmittance = exp(-optical_depth);
    let scattering_integral = (1.0 - transmittance) / max(optical_depth, vec3<f32>(eps));

    let rayleigh_scattering = scattering_integral * airmass.x * fp.rayleigh_scattering_coeff;
    let mie_scattering = scattering_integral * airmass.y * fp.mie_scattering_coeff;

    let LoV = dot(ray_direction_world, env.light_dir);

    // Multiple scattering
    var scattering = vec3<f32>(0.0);
    var scatter_amount = 1.0;
    var anisotropy = 1.0;

    scattering += 2.0 * (rayleigh_scattering + mie_scattering) * isotropic_phase * env.ambient_color;

    for (var i = 0; i < 4; i++) {
        let mie_phase = 0.7 * henyey_greenstein_phase(LoV, 0.5 * anisotropy) + 0.3 * henyey_greenstein_phase(LoV, -0.2 * anisotropy);
        scattering += scatter_amount * (rayleigh_scattering * isotropic_phase + mie_scattering * mie_phase)
            * env.light_color * (1.0 - 0.9 * env.rain_strength) * shadow;
        scatter_amount *= 0.5;
        anisotropy *= 0.7;
    }

    scattering *= max(skylight, env.eye_skylight);
    scattering *= clamp01(1.0 - env.blindness - env.darkness);

    // Artifically brighten fog in the early morning and evening (looks nice)
    let evening_glow = 0.75 * linear_step(0.05, 1.0, exp(-300.0 * sqr(env.sun_dir_y + 0.02)));
    scattering += scattering * evening_glow;

    return Fog2(max(scattering, vec3<f32>(0.0)), max(transmittance, vec3<f32>(0.0)));
}
