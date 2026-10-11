//! The per-frame scalars Photon (SixthSurge) computes in `shaders.properties`, `include/weather/core.glsl`,
//! `include/lighting/colors/{light_color,weather_color}.glsl` and `include/sky/aurora_colors.glsl`, on the CPU: pure
//! functions of the time, the weather and the biome under the player, so the clouds (`render::clouds`) and the fog
//! (`render::fog`) share one copy and the tests can check them.
//! Axes: mc-rs's sun moves in the YZ plane (`world::sky::sun_dir`: noon (0,1,0), dusk (0,0,1)), the pack's in XY, so
//! "morning" is `sun.z < 0` here where it is `sun.x > 0` there. `sunAngle` is OptiFine's (0 at sunrise, 0.5 at sunset).
//! b1.7.3 has no `wetness` (rain strength stands in), one moon phase, no pale garden, no day counter other than ticks / 24000.
//! Settings are the pack's defaults (`settings.glsl`); a constant here is named after its `#define`.

use crate::world::biome::Biome;
use glam::Vec3;

const GOLDEN_RATIO: f32 = 1.618_034;
/// `rec709_to_rec2020` (the pack's working colour space), standard row-major form.
const REC709_TO_REC2020: [[f32; 3]; 3] = [[0.627_425_7, 0.329_289_1, 0.043_350_24], [0.069_027_44, 0.919_578_6, 0.011_361_38], [0.016_366_42, 0.088_028_9, 0.895_684_3]];
/// `sunlight_color` of atmosphere.glsl.
pub const SUNLIGHT_COLOR: Vec3 = Vec3::new(1.051, 0.985, 0.940);

pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
pub fn linear_step(e0: f32, e1: f32, x: f32) -> f32 {
    ((x - e0) / (e1 - e0)).clamp(0.0, 1.0)
}
pub fn dampen(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * (2.0 - x)
}
pub fn sqr(x: f32) -> f32 {
    x * x
}
fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
fn cubic_smooth(x: f32) -> f32 {
    x * x * (3.0 - 2.0 * x)
}
fn pulse(x: f32, center: f32, width: f32) -> f32 {
    let x = (x - center).abs() / width;
    if x > 1.0 { 0.0 } else { 1.0 - cubic_smooth(x) }
}
/// `from_srgb`: gamma 2.2 then into the working space.
pub fn from_srgb(c: [f32; 3]) -> Vec3 {
    let l = c.map(|v| v.powf(2.2));
    let m = REC709_TO_REC2020;
    Vec3::new(m[0][0] * l[0] + m[0][1] * l[1] + m[0][2] * l[2], m[1][0] * l[0] + m[1][1] * l[1] + m[1][2] * l[2], m[2][0] * l[0] + m[2][1] * l[1] + m[2][2] * l[2])
}

/// random.glsl `hash1(float)` and `noise_1d`.
pub fn hash1(p: f32) -> f32 {
    let mut p = (p * 0.1031).fract();
    p *= p + 33.33;
    p *= p + p;
    p.fract()
}
pub fn noise_1d(x: f32) -> f32 {
    let (i, f) = (x.floor(), x - x.floor());
    mix(hash1(i), hash1(i + 1.0), cubic_smooth(f))
}
/// random.glsl `lowbias32`.
pub fn lowbias32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846ca68b);
    x ^= x >> 16;
    x
}

/// `time_*`, `day_factor`, `sun_dir` / `moon_dir` / `light_dir` and `sunAngle` of one frame.
#[derive(Clone, Copy, Debug)]
pub struct TimeOfDay {
    pub sun_dir: Vec3,
    pub moon_dir: Vec3,
    pub light_dir: Vec3,
    pub sun_angle: f32,
    pub sunrise: f32,
    pub noon: f32,
    pub sunset: f32,
    pub midnight: f32,
    pub day_factor: f32,
}

impl TimeOfDay {
    /// `angle` is `world::sky::celestial_angle` (0 = noon).
    pub fn new(angle: f32) -> Self {
        let sun_dir = crate::world::sky::sun_dir(angle);
        let sun_angle = (angle + 0.25).fract();
        let y = sun_dir.y;
        let me_fade = if y < 0.18 { 0.37 + 1.2 * (-y).max(0.0) } else { 1.7 };
        let me_weight = sqr((1.0 - me_fade * (y - 0.18).abs()).clamp(0.0, 1.0));
        let morning = sun_dir.z < 0.0;
        let u = (y.abs() / 0.3).clamp(0.0, 1.0);
        Self {
            sun_dir,
            moon_dir: -sun_dir,
            light_dir: if sun_angle < 0.5 { sun_dir } else { -sun_dir },
            sun_angle,
            sunrise: if morning { me_weight } else { 0.0 },
            noon: if y > 0.0 { 1.0 - me_weight } else { 0.0 },
            sunset: if morning { 0.0 } else { me_weight },
            midnight: if y < 0.0 { 1.0 - me_weight } else { 0.0 },
            day_factor: u * u * (3.0 - 2.0 * u),
        }
    }
}

/// The `biome_*` uniforms: category weights eased over 30 s (`smooth(.., 30, 30)`), climate eased over 1000 s.
#[derive(Clone, Copy, Debug, Default)]
pub struct BiomeEnv {
    pub arid: f32,
    pub snowy: f32,
    pub taiga: f32,
    pub jungle: f32,
    pub swamp: f32,
    pub may_rain: f32,
    pub may_snow: f32,
    pub may_sandstorm: f32,
    /// `temperature * 2 - 1`, `rainfall * 2 - 1`.
    pub temperature: f32,
    pub humidity: f32,
    /// 0 above y 63, up to 1 deep in the dark (`biome_cave`), set by the caller.
    pub cave: f32,
    primed: bool,
}

impl BiomeEnv {
    pub fn temperate(&self) -> f32 {
        1.0 - self.arid - self.snowy - self.taiga - self.jungle - self.swamp
    }
    /// Ease toward the biome under the player. `temperature` / `humidity` are the b1.7.3 climate values in 0..1.
    pub fn update(&mut self, biome: Biome, temperature: f64, humidity: f64, dt: f32) {
        // Biome.getEnableRain / getEnableSnow: desert rains never, taiga and tundra snow.
        let (arid, snowy, taiga, jungle, swamp) = (
            matches!(biome, Biome::Desert | Biome::Savanna) as u8 as f32,
            matches!(biome, Biome::Tundra) as u8 as f32,
            matches!(biome, Biome::Taiga) as u8 as f32,
            matches!(biome, Biome::Rainforest) as u8 as f32,
            matches!(biome, Biome::Swamp) as u8 as f32,
        );
        let snow = (snowy + taiga).min(1.0);
        let rain = if matches!(biome, Biome::Desert) { 0.0 } else { 1.0 - snow };
        let sandstorm = matches!(biome, Biome::Desert) as u8 as f32;
        let (t, h) = (temperature as f32 * 2.0 - 1.0, humidity as f32 * 2.0 - 1.0);
        let k = |tau: f32| if self.primed { 1.0 - (-dt / tau).exp() } else { 1.0 };
        let (k30, k1000) = (k(30.0), k(1000.0));
        let ease = |cur: &mut f32, target: f32, k: f32| *cur += (target - *cur) * k;
        ease(&mut self.arid, arid, k30);
        ease(&mut self.snowy, snowy, k30);
        ease(&mut self.taiga, taiga, k30);
        ease(&mut self.jungle, jungle, k30);
        ease(&mut self.swamp, swamp, k30);
        ease(&mut self.may_rain, rain, k30);
        ease(&mut self.may_snow, snow, k30);
        ease(&mut self.may_sandstorm, sandstorm, k30);
        ease(&mut self.temperature, t, k1000);
        ease(&mut self.humidity, h, k1000);
        self.primed = true;
    }
}

/// `Weather` of weather/core.glsl.
#[derive(Clone, Copy, Debug)]
pub struct PackWeather {
    pub temperature: f32,
    pub humidity: f32,
    pub wind: f32,
}

impl PackWeather {
    /// `world_age` in seconds (the pack: ((worldDay % 128) * 24000 + worldTime) / 20).
    pub fn new(world_age: f32, t: &TimeOfDay, biome: &BiomeEnv, wetness: f32) -> Self {
        let speed = |k: f32| k * GOLDEN_RATIO / 600.0; // * WEATHER_*_VARIATION_SPEED (1.0)
        // RANDOM_WEATHER_VARIATION, BIOME_WEATHER_VARIATION, WEATHER_*_BIAS = 0
        let mut temperature = noise_1d(world_age * speed(0.37) + 2.5);
        temperature -= 0.2 * t.sunrise + 0.2 * t.midnight;
        temperature *= 1.0 + (biome.temperature - 0.6) * 0.1;
        let mut humidity = mix(0.2, 0.8, noise_1d(world_age * speed(0.37) + 46.618));
        humidity *= 1.0 + (biome.humidity + 0.2) * 0.1;
        humidity += wetness;
        let wind = noise_1d(world_age * speed(0.5) + 83.236) + 0.33 * wetness;
        Self { temperature: temperature.clamp(0.0, 1.0), humidity: humidity.clamp(0.0, 1.0), wind: wind.clamp(0.0, 1.0) }
    }
}

/// `get_sun_exposure() * get_sun_tint()`.
pub fn sun_color(t: &TimeOfDay) -> Vec3 {
    let y = t.sun_dir.y;
    let blue_hour = linear_step(0.05, 1.0, (-190.0 * sqr(y + 0.09604)).exp());
    let exposure = 7.0 * (1.0 + 0.5 * (t.sunset + t.sunrise) + 40.0 * blue_hour); // SUN_I = 1
    let evening = Vec3::new(1.05, 0.84, 0.93) * 1.2;
    let morning_evening_tint = Vec3::ONE.lerp(evening, sqr(pulse(y, 0.17, 0.40)));
    let blue_hour_tint = Vec3::ONE.lerp(Vec3::new(0.95, 0.80, 1.0), blue_hour);
    let user = from_srgb([1.0; 3]); // SUN_N* = SUN_M* = SUN_E* = 1
    exposure * morning_evening_tint * blue_hour_tint * user
}
/// `get_moon_exposure() * get_moon_tint()` (one moon phase: `moon_phase_brightness` = 1).
pub fn moon_color(t: &TimeOfDay) -> Vec3 {
    let boost = 1.0 + 0.33 / (1.25 * (-t.sun_dir.y).max(0.1)).clamp(0.0, 1.0);
    0.66 * boost * from_srgb([0.75, 0.83, 1.00])
}
/// `get_weather_color()`.
pub fn weather_color(t: &TimeOfDay, biome: &BiomeEnv) -> Vec3 {
    let s = smoothstep(-0.1, 0.5, t.sun_dir.y);
    let rain = mix(0.033, 0.66, s) * SUNLIGHT_COLOR * Vec3::new(0.49, 0.65, 1.00);
    let snow = mix(0.060, 1.60, s) * SUNLIGHT_COLOR * Vec3::new(0.49, 0.65, 1.00);
    let sand = mix(0.033, 0.66, s) * SUNLIGHT_COLOR * Vec3::new(1.00, 0.83, 0.60);
    rain.lerp(snow, biome.may_snow).lerp(sand, biome.may_sandstorm)
}

/// `aurora_colors[day]`: [bottom, top].
pub fn aurora_colors(day: u32) -> [Vec3; 2] {
    let c = |r, g, b| Vec3::new(r, g, b);
    let t = [
        [c(0.0, 1.0, 0.25), c(0.5, 0.7, 1.0)],
        [c(0.0, 1.0, 0.25), c(0.5, 0.7, 1.0)],
        [c(1.0, 0.0, 0.0), c(1.0, 0.5, 0.7)],
        [c(1.0, 0.25, 1.0), c(0.25, 0.25, 1.0)],
        [c(1.0, 0.5, 1.0), c(0.5, 0.7, 1.0)],
        [c(1.0, 0.5, 1.0), c(0.5, 0.7, 1.0)],
        [c(1.0, 0.1, 0.0), c(1.0, 1.0, 0.25)],
        [c(1.0, 1.0, 1.0), c(1.0, 0.0, 0.0)],
        [c(1.0, 1.0, 0.0), c(0.1, 0.5, 1.0)],
        [c(1.0, 0.25, 1.0), c(0.0, 1.0, 0.25)],
        [c(1.0, 0.7, 1.0) * 1.2, c(0.9, 0.3, 0.9)],
        [c(0.0, 1.0, 0.25), c(0.9, 0.3, 0.9)],
        [c(2.0, 0.8, 0.0), c(1.0, 0.5, 0.0)],
    ];
    t[(lowbias32(day) % 13) as usize]
}
/// `get_aurora_amount()` (AURORA_NORMAL = never, AURORA_SNOW = rarely).
pub fn aurora_amount(t: &TimeOfDay, biome: &BiomeEnv, day: u32) -> f32 {
    let night = smoothstep(0.0, 0.2, -t.sun_dir.y);
    let snow = (lowbias32(day) % 5 == 1) as u8 as f32;
    night * mix(0.0, snow, biome.may_snow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_of_day_weights_make_sense() {
        for i in 0..240 {
            let t = TimeOfDay::new(i as f32 / 240.0);
            assert!((t.sunrise + t.noon + t.sunset + t.midnight - 1.0).abs() < 1e-5, "angle {}", i as f32 / 240.0);
            assert!((0.0..=1.0).contains(&t.day_factor));
        }
        let noon = TimeOfDay::new(0.0);
        assert!(noon.noon > 0.99 && noon.sun_angle == 0.25 && noon.light_dir == noon.sun_dir);
        assert!(TimeOfDay::new(0.5).midnight > 0.99 && TimeOfDay::new(0.5).light_dir == TimeOfDay::new(0.5).moon_dir);
        assert!(TimeOfDay::new(0.76).sunrise > 0.0 && TimeOfDay::new(0.76).sunset == 0.0); // just after dawn
        assert!(TimeOfDay::new(0.24).sunset > 0.0 && TimeOfDay::new(0.24).sunrise == 0.0);
    }

    #[test]
    fn weather_stays_in_range_and_noise_is_the_glsl_one() {
        let mut b = BiomeEnv::default();
        b.update(Biome::Plains, 0.8, 0.4, 0.05);
        let t = TimeOfDay::new(0.0);
        for age in (0..100_000).step_by(997) {
            let w = PackWeather::new(age as f32, &t, &b, 0.0);
            assert!([w.temperature, w.humidity, w.wind].iter().all(|v| (0.0..=1.0).contains(v)));
        }
        // hash1(0) = fract(0) ... = 0; noise_1d(1.0) = hash1(1.0): p = .1031, p *= p + 33.33, p *= p + p.
        let p = 0.1031_f32;
        let p = p * (p + 33.33);
        assert!((hash1(1.0) - (p * (p + p)).fract()).abs() < 1e-6 && (noise_1d(1.0) - hash1(1.0)).abs() < 1e-6);
    }

    #[test]
    fn biome_weights_snap_then_ease() {
        let mut b = BiomeEnv::default();
        b.update(Biome::Desert, 1.0, 0.0, 0.016);
        assert!(b.arid == 1.0 && b.may_rain == 0.0 && b.may_sandstorm == 1.0 && b.temperate() == 0.0);
        b.update(Biome::Tundra, 0.0, 0.5, 1.0);
        assert!(b.arid > 0.9 && b.snowy < 0.1 && b.may_snow < 0.1); // 1 s of a 30 s ease
    }

    #[test]
    fn colours_and_aurora() {
        let noon = TimeOfDay::new(0.0);
        let c = sun_color(&noon);
        assert!((c.x - 7.0).abs() < 0.01 && (c.y - 7.0).abs() < 0.01);
        assert!(moon_color(&TimeOfDay::new(0.5)).z > moon_color(&TimeOfDay::new(0.5)).x);
        let mut snow = BiomeEnv::default();
        snow.update(Biome::Tundra, 0.0, 0.5, 1.0);
        assert!(aurora_amount(&noon, &snow, 1) == 0.0); // never by day
        let nights = (0..200u32).filter(|d| aurora_amount(&TimeOfDay::new(0.5), &snow, *d) > 0.99).count();
        assert!((20..=60).contains(&nights), "{nights} of 200 nights"); // about one in five
        assert_eq!(aurora_colors(7).len(), 2);
    }
}
