//! Sky and weather: world time -> sun angle -> light subtracted from sky light, the sky and fog colours, the
//! sunrise glow, star brightness and star positions, and the rain/thunder timers that dim all of it.
//! Ports `WorldProvider.calculateCelestialAngle` / `calcSunriseSunsetColors` / `func_4096_a` (horizon colour),
//! `World.calculateSkylightSubtracted`, `World.func_4079_a` (sky colour), `World.getStarBrightness`,
//! `World.updateWeather`, `BiomeGenBase.getSkyColorByTemp`, the fog colour of `EntityRenderer.updateFogColor`
//! and `RenderGlobal.renderStars`. One day is 24000 ticks (20 per second, 20 minutes).
//! Checked against the real classes by `tools/golden/G.java` (`CEL`, `SUBL`, `WEAT`/`WEAH`, `STARS` lines).
//! Not ported: the lightning flash (`field_27172_i`), the cave-darkening of the fog colour (`fogColor1`), water
//! and lava fog, clouds, and drawing rain/snow (`renderRainSnow`); thunder strikes need `EntityLightningBolt`.

use crate::world::gen::noise::{mh_cos, mh_sin, JavaRandom};
use std::f32::consts::PI;

pub const TICKS_PER_DAY: u64 = 24000;

/// Sun angle in [0, 1): 0 = noon, 0.5 = midnight. `partial` is the fraction of the current tick.
pub fn celestial_angle(time: u64, partial: f32) -> f32 {
    let mut a = ((time % TICKS_PER_DAY) as f32 + partial) / TICKS_PER_DAY as f32 - 0.25;
    if a < 0.0 {
        a += 1.0;
    }
    if a > 1.0 {
        a -= 1.0;
    }
    let linear = a;
    let curved = 1.0 - (((a as f64 * std::f64::consts::PI).cos() + 1.0) / 2.0) as f32;
    linear + (curved - linear) / 3.0
}

/// Sky light taken away by the time of day and the weather: 0 (day) ..= 11 (night). `rain` and `thunder` are
/// the strengths `Weather::rain` / `Weather::thunder` return (`func_27162_g` / `func_27166_f`).
pub fn skylight_subtracted(angle: f32, rain: f32, thunder: f32) -> u8 {
    let mut v = (1.0 - (mh_cos(angle * PI * 2.0) * 2.0 + 0.5)).clamp(0.0, 1.0);
    v = 1.0 - v;
    v = (v as f64 * (1.0 - (rain * 5.0) as f64 / 16.0)) as f32;
    v = (v as f64 * (1.0 - (thunder * 5.0) as f64 / 16.0)) as f32;
    v = 1.0 - v;
    (v * 11.0) as u8
}

/// `World.updateWeather`: the rain and thunder timers (`WorldInfo.rainTime` / `thunderTime`, in ticks) flip the
/// two flags, and the smoothed strengths chase them by 0.01 per tick (5 seconds from clear to full). A new world
/// starts clear with both timers at 0, so the first draws pick the time to the first rain / thunder. Call `tick`
/// once per game tick; `rain` / `thunder` interpolate with the fraction of the current tick.
pub struct Weather {
    rng: JavaRandom,
    raining: bool,
    rain_time: i32,
    thundering: bool,
    thunder_time: i32,
    prev_rain: f32,
    rain: f32,
    prev_thunder: f32,
    thunder: f32,
}

impl Weather {
    /// `seed` feeds the `java.util.Random` that picks the timers (the Java `World.rand` is unseeded).
    pub fn new(seed: i64) -> Self {
        Self { rng: JavaRandom::new(seed), raining: false, rain_time: 0, thundering: false, thunder_time: 0, prev_rain: 0.0, rain: 0.0, prev_thunder: 0.0, thunder: 0.0 }
    }

    pub fn tick(&mut self) {
        // Thunder first, then rain: the draw order is part of the reference.
        if self.thunder_time <= 0 {
            self.thunder_time = if self.thundering { self.rng.next_int_bound(12000) + 3600 } else { self.rng.next_int_bound(168000) + 12000 };
        } else {
            self.thunder_time -= 1;
            if self.thunder_time <= 0 {
                self.thundering = !self.thundering;
            }
        }
        if self.rain_time <= 0 {
            self.rain_time = if self.raining { self.rng.next_int_bound(12000) + 12000 } else { self.rng.next_int_bound(168000) + 12000 };
        } else {
            self.rain_time -= 1;
            if self.rain_time <= 0 {
                self.raining = !self.raining;
            }
        }
        self.prev_rain = self.rain;
        self.rain = ((self.rain as f64) + if self.raining { 0.01 } else { -0.01 }) as f32;
        self.rain = self.rain.clamp(0.0, 1.0);
        self.prev_thunder = self.thunder;
        self.thunder = ((self.thunder as f64) + if self.thundering { 0.01 } else { -0.01 }) as f32;
        self.thunder = self.thunder.clamp(0.0, 1.0);
    }

    /// `World.func_27162_g`: rain strength 0..=1.
    pub fn rain(&self, partial: f32) -> f32 {
        self.prev_rain + (self.rain - self.prev_rain) * partial
    }

    /// `World.func_27166_f`: thunder strength, which only counts while it rains.
    pub fn thunder(&self, partial: f32) -> f32 {
        (self.prev_thunder + (self.thunder - self.prev_thunder) * partial) * self.rain(partial)
    }
}

/// `java.awt.Color.HSBtoRGB` as 0..=1 floats, quantised to 8 bits like the AWT int result.
fn hsb_to_rgb(h: f32, s: f32, b: f32) -> [f32; 3] {
    let q8 = |v: f32| (v * 255.0 + 0.5) as i32 as f32 / 255.0;
    if s == 0.0 {
        return [q8(b); 3];
    }
    let h6 = (h - h.floor()) * 6.0;
    let f = h6 - h6.floor();
    let (p, q, t) = (b * (1.0 - s), b * (1.0 - s * f), b * (1.0 - s * (1.0 - f)));
    let [r, g, bl] = match h6 as i32 {
        0 => [b, t, p],
        1 => [q, b, p],
        2 => [p, b, t],
        3 => [p, q, b],
        4 => [t, p, b],
        _ => [b, p, q],
    };
    [q8(r), q8(g), q8(bl)]
}

/// Greys `c` towards its luminance (weights 0.3/0.59/0.11) scaled by `grey`; `strength` 1 gives `1 - 12/16` of the colour.
fn grey_towards(c: [f32; 3], strength: f32, grey: f32) -> [f32; 3] {
    let l = (c[0] * 0.3 + c[1] * 0.59 + c[2] * 0.11) * grey;
    let keep = 1.0 - strength * (12.0 / 16.0);
    c.map(|v| v * keep + l * (1.0 - keep))
}

/// Sky colour (RGB, 0..=1) for a sun `angle` over ground of climate `temperature` (0..=1) with the given rain and
/// thunder strengths (`World.func_4079_a`).
pub fn sky_color(angle: f32, temperature: f32, rain: f32, thunder: f32) -> [f32; 3] {
    let k = (mh_cos(angle * PI * 2.0) * 2.0 + 0.5).clamp(0.0, 1.0);
    let t = (temperature / 3.0).clamp(-1.0, 1.0);
    let mut c = hsb_to_rgb(224.0 / 360.0 - t * 0.05, 0.5 + t * 0.1, 1.0).map(|v| v * k);
    if rain > 0.0 {
        c = grey_towards(c, rain, 0.6);
    }
    if thunder > 0.0 {
        c = grey_towards(c, thunder, 0.2);
    }
    c
}

/// Colour of the sky at the horizon (`WorldProvider.func_4096_a`): pale blue by day, nearly black at night.
fn horizon_color(angle: f32) -> [f32; 3] {
    let k = (mh_cos(angle * PI * 2.0) * 2.0 + 0.5).clamp(0.0, 1.0);
    [192.0 / 255.0 * (k * 0.94 + 0.06), 216.0 / 255.0 * (k * 0.94 + 0.06), 1.0 * (k * 0.91 + 0.09)]
}

/// Fog colour, which is also what the frame is cleared to (`EntityRenderer.updateFogColor`): the horizon colour
/// pulled a little towards the sky colour, darkened by rain and thunder.
/// ponytail: vanilla draws no sky at all below the NORMAL view distance and mixes by `1 - (1/(4 - renderDistance))^0.25`;
/// mc-rs always draws it and uses NORMAL's (renderDistance 1) numbers. The `fogColor1` brightness term (darker fog
/// in caves) is left out.
pub fn fog_color(angle: f32, sky: [f32; 3], rain: f32, thunder: f32) -> [f32; 3] {
    let mix = (1.0 - ((1.0_f32 / 3.0) as f64).powf(0.25)) as f32;
    let h = horizon_color(angle);
    let mut c = [h[0] + (sky[0] - h[0]) * mix, h[1] + (sky[1] - h[1]) * mix, h[2] + (sky[2] - h[2]) * mix];
    if rain > 0.0 {
        let (rg, b) = (1.0 - rain * 0.5, 1.0 - rain * 0.4);
        c = [c[0] * rg, c[1] * rg, c[2] * b];
    }
    if thunder > 0.0 {
        c = c.map(|v| v * (1.0 - thunder * 0.5));
    }
    c
}

/// Orange glow along the horizon at sunrise and sunset (`WorldProvider.calcSunriseSunsetColors`): RGBA, or `None`
/// while the sun is far from the horizon.
pub fn sunrise_color(angle: f32) -> Option<[f32; 4]> {
    let (band, centre) = (0.4_f32, -0.0_f32);
    let c = mh_cos(angle * PI * 2.0) - 0.0;
    if c < centre - band || c > centre + band {
        return None;
    }
    let a = (c - centre) / band * 0.5 + 0.5;
    let mut alpha = 1.0 - (1.0 - mh_sin(a * PI)) * 0.99;
    alpha *= alpha;
    Some([a * 0.3 + 0.7, a * a * 0.7 + 0.2, a * a * 0.0 + 0.2, alpha])
}

/// How bright the stars are, 0 by day up to 0.5 at midnight (`World.getStarBrightness`); the renderer also
/// multiplies by `1 - rain`.
pub fn star_brightness(angle: f32) -> f32 {
    let v = (1.0 - (mh_cos(angle * PI * 2.0) * 2.0 + 12.0 / 16.0)).clamp(0.0, 1.0);
    v * v * 0.5
}

/// The star quads of `RenderGlobal.renderStars`: 4 corners per star, on a sphere of radius 100 around the viewer.
/// 1500 random directions are drawn from `Random(10842)`; those outside the unit ball are skipped.
pub fn star_vertices() -> Vec<[f64; 3]> {
    let mut rng = JavaRandom::new(10842);
    let mut out = Vec::new();
    for _ in 0..1500 {
        let mut x = (rng.next_float() * 2.0 - 1.0) as f64;
        let mut y = (rng.next_float() * 2.0 - 1.0) as f64;
        let mut z = (rng.next_float() * 2.0 - 1.0) as f64;
        let size = (0.25 + rng.next_float() * 0.25) as f64;
        let d = x * x + y * y + z * z;
        if d < 1.0 && d > 0.01 {
            let inv = 1.0 / d.sqrt();
            (x, y, z) = (x * inv, y * inv, z * inv);
            let (cx, cy, cz) = (x * 100.0, y * 100.0, z * 100.0);
            let (s1, c1) = x.atan2(z).sin_cos();
            let (s2, c2) = (x * x + z * z).sqrt().atan2(y).sin_cos();
            let (s3, c3) = (rng.next_double() * std::f64::consts::PI * 2.0).sin_cos();
            for i in 0..4 {
                let u = (((i & 2) as i32) - 1) as f64 * size;
                let v = ((((i + 1) & 2) as i32) - 1) as f64 * size;
                let (a, b) = (u * c3 - v * s3, v * c3 + u * s3);
                let (up, back) = (a * s2, -(a * c2));
                out.push([cx + (back * s1 - b * c1), cy + up, cz + (b * s1 + back * c1)]);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference numbers from the real classes (tools/golden/G.java -> golden.txt).
    const GOLDEN: &str = include_str!("../../../tools/golden/golden.txt");

    fn lines(tag: &'static str) -> impl Iterator<Item = Vec<&'static str>> {
        GOLDEN.lines().filter(move |l| l.starts_with(tag)).map(|l| l.split_whitespace().collect())
    }
    fn hex(s: &str) -> f32 {
        f32::from_bits(u32::from_str_radix(s, 16).unwrap())
    }
    fn close(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-5, "{a} vs {b}");
    }

    /// Time 0 is sunrise in daylight, noon and midday are full light, midnight is the darkest (11);
    /// every tick of the day stays inside 0..=11 and the in-between values occur.
    #[test]
    fn day_night_cycle() {
        let sub = |t| skylight_subtracted(celestial_angle(t, 1.0), 0.0, 0.0);
        assert_eq!((sub(0), sub(6000), sub(18000)), (0, 0, 11));
        assert!((celestial_angle(18000, 0.0) - 0.5).abs() < 1e-6 && celestial_angle(6000, 0.0).abs() < 1e-6);
        let all: Vec<u8> = (0..TICKS_PER_DAY).step_by(10).map(sub).collect();
        assert!(all.iter().all(|&s| s <= 11) && (1..=10).all(|v| all.contains(&v)));
        assert_eq!(sub(5), sub(5 + TICKS_PER_DAY)); // wraps every day
    }

    /// Black at midnight, a blue-ish day sky at noon (blue channel full, red lowest).
    #[test]
    fn sky_colour_follows_the_sun() {
        assert_eq!(sky_color(0.5, 0.5, 0.0, 0.0), [0.0; 3]);
        let [r, g, b] = sky_color(0.0, 0.5, 0.0, 0.0);
        assert!(b == 1.0 && r < g && g < b, "{r} {g} {b}");
        // Colder ground (lower temperature) shifts the hue, so the colour differs.
        assert_ne!(sky_color(0.0, 0.0, 0.0, 0.0), sky_color(0.0, 1.0, 0.0, 0.0));
    }

    /// Rain and thunder each darken the sky, the fog and the sky light, step by step.
    #[test]
    fn rain_and_thunder_dim_the_sky() {
        let sum = |c: [f32; 3]| c[0] + c[1] + c[2];
        let clear = sky_color(0.0, 0.5, 0.0, 0.0);
        let (rainy, storm) = (sky_color(0.0, 0.5, 1.0, 0.0), sky_color(0.0, 0.5, 1.0, 1.0));
        assert!(sum(clear) > sum(rainy) && sum(rainy) > sum(storm));
        let fog = |r, t| sum(fog_color(0.0, clear, r, t));
        assert!(fog(0.0, 0.0) > fog(1.0, 0.0) && fog(1.0, 0.0) > fog(1.0, 1.0));
        let sub = |r, t| skylight_subtracted(0.0, r, t);
        assert!(sub(0.0, 0.0) == 0 && sub(1.0, 0.0) > 0 && sub(1.0, 1.0) > sub(1.0, 0.0));
    }

    /// Celestial angle, horizon colour, sunrise glow and star brightness against the real `WorldProvider` / `World`,
    /// and the sky light with rain and thunder against `World.calculateSkylightSubtracted`.
    #[test]
    fn sky_matches_java() {
        let mut n = 0;
        for t in lines("CEL") {
            let time: u64 = t[1].parse().unwrap();
            let angle = celestial_angle(time, 0.25);
            close(angle, hex(t[2]));
            let h = horizon_color(angle);
            (0..3).for_each(|i| close(h[i], hex(t[3 + i])));
            let (want, star) = if t[6] == "none" { (None, t[7]) } else { (Some([hex(t[6]), hex(t[7]), hex(t[8]), hex(t[9])]), t[10]) };
            match (sunrise_color(angle), want) {
                (None, None) => {}
                (Some(a), Some(b)) => (0..4).for_each(|i| close(a[i], b[i])),
                (a, b) => panic!("sunrise glow {a:?} vs {b:?} at tick {time}"),
            }
            close(star_brightness(angle), hex(star));
            n += 1;
        }
        assert!(n >= 10);
        for t in lines("SUBL") {
            let time: u64 = t[1].parse().unwrap();
            let want: u8 = t[4].parse().unwrap();
            assert_eq!(skylight_subtracted(celestial_angle(time, 1.0), hex(t[2]), hex(t[3])), want, "{t:?}");
            n += 1;
        }
        assert!(n >= 60);
    }

    /// A million ticks of `World.updateWeather` from a fresh world, two seeds: every tick's flags, timers and
    /// strengths hash the same as in Java, and so do the spot checks and the number of rain / thunder starts.
    #[test]
    fn weather_matches_java() {
        for seed in [0xCAFEBABE_i64, 12345] {
            let spot: Vec<Vec<&str>> = lines("WEAT").filter(|t| t[1] == seed.to_string()).collect();
            assert_eq!(spot.len(), 4);
            let mut w = Weather::new(seed);
            let (mut h, mut rains, mut thunders, mut was_rain, mut was_thunder) = (0xcbf29ce484222325_u64, 0u32, 0u32, false, false);
            for tick in 1..=1_000_000u32 {
                w.tick();
                let v = [w.raining as u64, w.thundering as u64, w.rain_time as u32 as u64, w.thunder_time as u32 as u64, w.rain(1.0).to_bits() as u64, w.thunder(1.0).to_bits() as u64];
                for x in v {
                    h ^= x;
                    h = h.wrapping_mul(0x100000001b3);
                }
                rains += (w.raining && !was_rain) as u32;
                thunders += (w.thundering && !was_thunder) as u32;
                (was_rain, was_thunder) = (w.raining, w.thundering);
                if let Some(t) = spot.iter().find(|t| t[2] == tick.to_string()) {
                    assert_eq!(t[3..7].join(" "), format!("{} {} {} {}", w.raining as u8, w.thundering as u8, w.rain_time, w.thunder_time), "tick {tick}");
                    close(w.rain(1.0), hex(t[7]));
                    close(w.rain(0.5), hex(t[8]));
                }
            }
            let end = lines("WEAH").find(|t| t[1] == seed.to_string()).unwrap();
            assert_eq!(format!("{h:x}"), end[2], "seed {seed}");
            assert_eq!((rains, thunders), (end[3].parse().unwrap(), end[4].parse().unwrap()), "seed {seed}");
        }
    }

    /// The star quads against `RenderGlobal.renderStars` (count, coordinate sums, the first two stars).
    #[test]
    fn stars_match_java() {
        let t = lines("STARS").next().unwrap();
        let v = star_vertices();
        assert_eq!(v.len().to_string(), t[1]);
        for k in 0..3 {
            let want: f64 = t[2 + k].parse().unwrap();
            let sum: f64 = v.iter().map(|p| p[k]).sum();
            assert!((sum - want).abs() < 1e-6, "axis {k}: {sum} vs {want}");
        }
        for i in 0..8 {
            for k in 0..3 {
                let want: f64 = t[5 + i * 3 + k].parse().unwrap();
                assert!((v[i][k] - want).abs() < 1e-9, "vertex {i} axis {k}");
            }
        }
    }
}
