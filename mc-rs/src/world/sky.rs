//! Day/night cycle: world time -> sun angle -> light subtracted from sky light, and the sky colour.
//! Ports `WorldProvider.calculateCelestialAngle`, `World.calculateSkylightSubtracted`,
//! `World.func_4079_a` (sky colour) and `BiomeGenBase.getSkyColorByTemp`. One day is 24000 ticks
//! (20 per second, 20 minutes). Rain, thunder and lightning do not exist yet, so their terms (all
//! multiplications by 1 or no-ops with a strength of 0) are left out.

use crate::world::gen::noise::mh_cos;
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

/// Sky light taken away by the time of day: 0 (day) ..= 11 (night).
pub fn skylight_subtracted(angle: f32) -> u8 {
    let mut v = (1.0 - (mh_cos(angle * PI * 2.0) * 2.0 + 0.5)).clamp(0.0, 1.0);
    v = 1.0 - v;
    // (the rain and thunder strengths would scale `v` here; both are 0)
    v = 1.0 - v;
    (v * 11.0) as u8
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

/// Sky colour (RGB, 0..=1) for a sun `angle` over ground of climate `temperature` (0..=1).
pub fn sky_color(angle: f32, temperature: f32) -> [f32; 3] {
    let k = (mh_cos(angle * PI * 2.0) * 2.0 + 0.5).clamp(0.0, 1.0);
    let t = (temperature / 3.0).clamp(-1.0, 1.0);
    hsb_to_rgb(224.0 / 360.0 - t * 0.05, 0.5 + t * 0.1, 1.0).map(|c| c * k)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Time 0 is sunrise in daylight, noon and midday are full light, midnight is the darkest (11);
    /// every tick of the day stays inside 0..=11 and the in-between values occur.
    #[test]
    fn day_night_cycle() {
        let sub = |t| skylight_subtracted(celestial_angle(t, 1.0));
        assert_eq!((sub(0), sub(6000), sub(18000)), (0, 0, 11));
        assert!((celestial_angle(18000, 0.0) - 0.5).abs() < 1e-6 && celestial_angle(6000, 0.0).abs() < 1e-6);
        let all: Vec<u8> = (0..TICKS_PER_DAY).step_by(10).map(sub).collect();
        assert!(all.iter().all(|&s| s <= 11) && (1..=10).all(|v| all.contains(&v)));
        assert_eq!(sub(5), sub(5 + TICKS_PER_DAY)); // wraps every day
    }

    /// Black at midnight, a blue-ish day sky at noon (blue channel full, red lowest).
    #[test]
    fn sky_colour_follows_the_sun() {
        assert_eq!(sky_color(0.5, 0.5), [0.0; 3]);
        let [r, g, b] = sky_color(0.0, 0.5);
        assert!(b == 1.0 && r < g && g < b, "{r} {g} {b}");
        // Colder ground (lower temperature) shifts the hue, so the colour differs.
        assert_ne!(sky_color(0.0, 0.0), sky_color(0.0, 1.0));
    }
}
