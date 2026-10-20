//! Photon's air fog coefficients (`include/weather/fog.glsl`, `get_fog_parameters`) on the CPU, for the WGSL in
//! `wgsl/fog.wgsl` (`OverworldFogParameters`; `simple_fog.glsl` and `analytic.glsl` live there). d4_deferred_shading
//! (milestone 1) uploads `FogParameters::pack` and calls `air_fog_analytic`. The pack's raymarched fog (`c0_vl`) is not
//! ported: mc-rs keeps `render::vl`. Settings are `settings.glsl`'s defaults.

use crate::render::pack::{from_srgb, linear_step, sqr, BiomeEnv, PackWeather, TimeOfDay};
use glam::Vec3;

const OVERWORLD_FOG_INTENSITY: f32 = 1.0;

/// (AIR_FOG_RAYLEIGH_{R,G,B}, density) per biome: normal / rain, arid, snowy, taiga, jungle, swamp.
const RAYLEIGH_NORMAL: ([f32; 3], f32) = ([0.31, 0.67, 1.00], 0.0005);
const RAYLEIGH_RAIN: ([f32; 3], f32) = ([0.31, 0.67, 1.00], 0.0005);
const RAYLEIGH_ARID: ([f32; 3], f32) = ([0.65, 0.80, 1.00], 0.0003);
const RAYLEIGH_SNOWY: ([f32; 3], f32) = ([0.50, 0.81, 1.00], 0.0010);
const RAYLEIGH_TAIGA: ([f32; 3], f32) = ([0.31, 0.85, 1.00], 0.0006);
const RAYLEIGH_JUNGLE: ([f32; 3], f32) = ([0.40, 0.95, 1.00], 0.0013);
const RAYLEIGH_SWAMP: ([f32; 3], f32) = ([0.67, 1.00, 0.94], 0.0014);

const MIE_MORNING: f32 = 0.0070;
const MIE_NOON: f32 = 0.0001;
const MIE_EVENING: f32 = 0.0050;
const MIE_MIDNIGHT: f32 = 0.0050;
const MIE_RAIN: f32 = 0.030;
const MIE_SNOW: f32 = 0.015;
const MIE_BLUE_HOUR: f32 = 0.0020;

/// `OverworldFogParameters`.
#[derive(Clone, Copy, Debug)]
pub struct FogParameters {
    pub rayleigh_scattering_coeff: Vec3,
    pub mie_scattering_coeff: Vec3,
    pub mie_extinction_coeff: Vec3,
}

impl FogParameters {
    pub fn new(weather: &PackWeather, t: &TimeOfDay, biome: &BiomeEnv, rain: f32) -> Self {
        let coeff = |(c, d): ([f32; 3], f32)| from_srgb(c) * d;

        // Rayleigh coefficient
        let normal = coeff(RAYLEIGH_NORMAL);
        let mut rayleigh = normal * biome.temperate()
            + coeff(RAYLEIGH_ARID) * biome.arid
            + coeff(RAYLEIGH_SNOWY) * biome.snowy
            + coeff(RAYLEIGH_TAIGA) * biome.taiga
            + coeff(RAYLEIGH_JUNGLE) * biome.jungle
            + coeff(RAYLEIGH_SWAMP) * biome.swamp; // no pale garden in b1.7.3
        // rain
        rayleigh = (rayleigh * (1.0 + weather.humidity * weather.temperature)).lerp(coeff(RAYLEIGH_RAIN), rain * biome.may_rain);

        // Mie coefficient: more of it late in the sunset / blue hour
        let blue_hour = linear_step(0.05, 1.0, (-190.0 * sqr(t.sun_dir.y + 0.07283)).exp());
        let mut mie = MIE_MORNING * t.sunrise + MIE_NOON * t.noon + MIE_EVENING * t.sunset + MIE_MIDNIGHT * t.midnight + MIE_BLUE_HOUR * blue_hour;
        // Weather influence
        let wet = mie + 8.0 * MIE_NOON * sqr((weather.humidity / 0.8).clamp(0.0, 1.0));
        let k = rain * biome.may_rain;
        mie = wet + (MIE_RAIN - wet) * k;
        let k = rain * biome.may_snow;
        mie += (MIE_SNOW - mie) * k;

        let mie_albedo = 0.9 + (0.5 - 0.9) * rain * biome.may_rain;
        let mut mie_scattering = Vec3::splat(mie_albedo * mie);
        let mut mie_extinction = Vec3::splat(mie);

        // DESERT_SANDSTORM: `desert_sandstorm` is rain in a desert (the pack eases it over 60 s)
        let sandstorm = rain * biome.may_sandstorm;
        let density = 0.2;
        mie_scattering += sandstorm * density * 0.5;
        mie_extinction += sandstorm * density * Vec3::new(0.2, 0.27, 0.45);

        Self { rayleigh_scattering_coeff: rayleigh, mie_scattering_coeff: mie_scattering, mie_extinction_coeff: mie_extinction }
    }

    /// The three vec3s as vec4s (w = 0), the layout of `OverworldFogParameters` in a uniform block.
    pub fn pack(&self) -> [[f32; 4]; 3] {
        [self.rayleigh_scattering_coeff, self.mie_scattering_coeff, self.mie_extinction_coeff].map(|v| v.extend(0.0).to_array())
    }
}

/// `OVERWORLD_FOG_INTENSITY` (the WGSL has the same constant).
pub const INTENSITY: f32 = OVERWORLD_FOG_INTENSITY;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::biome::Biome;

    fn at(angle: f32, rain: f32, biome: Biome) -> FogParameters {
        let t = TimeOfDay::new(angle);
        let mut b = BiomeEnv::default();
        b.update(biome, 0.8, 0.4, 0.02);
        let w = PackWeather::new(1000.0, &t, &b, 0.0);
        FogParameters::new(&w, &t, &b, rain)
    }

    #[test]
    fn clear_noon_is_thin_and_blue_rain_and_dawn_are_thick() {
        let noon = at(0.0, 0.0, Biome::Plains);
        let r = noon.rayleigh_scattering_coeff;
        assert!(r.z > r.y && r.y > r.x && r.x > 0.0, "{r:?}"); // blue scatters most
        assert!(noon.mie_extinction_coeff.x < 0.001);
        assert!(at(0.76, 0.0, Biome::Plains).mie_extinction_coeff.x > 0.005); // morning haze
        let wet = at(0.0, 1.0, Biome::Plains);
        assert!((wet.mie_extinction_coeff.x - MIE_RAIN).abs() < 1e-6 && (wet.mie_scattering_coeff.x - 0.5 * MIE_RAIN).abs() < 1e-6);
        assert!(at(0.0, 1.0, Biome::Desert).mie_extinction_coeff.z > at(0.0, 1.0, Biome::Desert).mie_extinction_coeff.x); // sandstorm tint
        assert!(at(0.0, 0.0, Biome::Plains).pack()[0][3] == 0.0);
    }
}
