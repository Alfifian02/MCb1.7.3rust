//! Per-biome sky, fog, haze, clouds and stars, blended and eased in real time. Pure CPU, tested on a host.
//!
//! Every biome has a `Look` (see `of`): the DAY colours of its sky top and horizon (= fog), a haze band, an exponential fog density, the
//! cloud tint / cover / speed, the star tint / brightness and how fast the sky climbs from horizon to top. Nothing here knows the time:
//! the vanilla curves of `world::sky` (day/night, rain, thunder) still run and the look only scales them by `target / vanilla noon`, so
//! night stays dark and rain stays grey in every biome and at noon the sky is exactly the target colour.
//!
//! Smooth in space and time: `update` averages the looks of 9 columns (centre + a ring of 48 blocks, so a border is felt before it is
//! crossed), four times a second, and the frame's values chase that goal with a 2 s time constant in real seconds (frame rate
//! independent). A teleport or a dimension change needs no special case except `reset`, which makes the next `update` jump.
//! The clouds' drift is integrated here (`cloud_x`), so a biome with faster clouds changes their speed, never their place.

use crate::render::color::legacy_lin;
use crate::world::biome::Biome;
use crate::world::sky;

const N: usize = 21;
// Indices into a look.
const Z: usize = 0; // sky top, day colour (3)
const H: usize = 3; // horizon = fog, day colour (3)
const HZ: usize = 6; // haze band colour (3)
const HA: usize = 9; // haze band strength 0..1
const DENS: usize = 10; // exp² fog density, per block
const CL: usize = 11; // cloud tint (3)
const COVER: usize = 14; // 0..1
const SPEED: usize = 15; // x vanilla's 0.03 blocks a tick
const ST: usize = 16; // star tint (3)
const SMUL: usize = 19; // star brightness x
const CURVE: usize = 20; // >1: the top colour is reached higher up

/// Seconds the frame needs to cover 63% of a change of biome.
const TAU: f32 = 2.0;
/// `RenderGlobal.renderClouds`: the layer height (`WorldProvider.getCloudHeight`); the shader gives it 4 blocks of thickness.
pub const CLOUD_Y: f32 = 108.0;
/// 12-block cells drifting along +x at vanilla's 0.03 blocks a tick.
const DRIFT: f32 = 0.6;
const CLOUD_ALPHA: f32 = 0.82;
/// The temperature the vanilla sky colour is evaluated at: only its shape over the day is used, the biome gives the colour.
const REF_TEMP: f32 = 0.5;

/// (offset x, offset z, weight) of the 9 sampled columns; the weights add up to 1.
const RING: [(i32, i32, f32); 9] = [
    (0, 0, 0.28),
    (48, 0, 0.09),
    (-48, 0, 0.09),
    (0, 48, 0.09),
    (0, -48, 0.09),
    (34, 34, 0.09),
    (-34, 34, 0.09),
    (34, -34, 0.09),
    (-34, -34, 0.09),
];

#[rustfmt::skip]
const fn mk(z: [f32; 3], h: [f32; 3], hz: [f32; 3], ha: f32, dens: f32, cl: [f32; 3], cover: f32, speed: f32, st: [f32; 3], smul: f32, curve: f32) -> [f32; N] {
    [z[0], z[1], z[2], h[0], h[1], h[2], hz[0], hz[1], hz[2], ha, dens, cl[0], cl[1], cl[2], cover, speed, st[0], st[1], st[2], smul, curve]
}

/// The look of a biome. Colours are vanilla's framebuffer values (gamma), like `world::sky`.
#[rustfmt::skip]
fn of(b: Biome) -> [f32; N] {
    use Biome::*;
    match b {
        //                 sky top             horizon / fog        haze colour         ha    dens    cloud tint          cover spd  star tint           smul curve
        Plains          => mk([0.38, 0.62, 1.00], [0.78, 0.86, 1.00], [0.90, 0.92, 1.00], 0.25, 0.0018, [1.00, 1.00, 1.00], 0.45, 1.0, [1.00, 1.00, 1.00], 1.0, 1.0),
        Forest          => mk([0.50, 0.46, 0.95], [0.84, 0.62, 0.76], [0.95, 0.70, 0.82], 0.45, 0.0030, [0.86, 0.76, 0.92], 0.50, 1.0, [0.80, 0.72, 1.00], 1.0, 1.0), // lavender dusk
        Rainforest      => mk([0.30, 0.95, 0.80], [0.36, 0.86, 0.72], [0.55, 1.00, 0.78], 0.50, 0.0055, [0.78, 1.00, 0.90], 0.65, 0.8, [0.55, 1.00, 0.80], 0.8, 0.9), // teal
        Swamp           => mk([0.36, 0.80, 0.70], [0.47, 0.74, 0.56], [0.62, 0.85, 0.55], 0.60, 0.0100, [0.70, 0.85, 0.76], 0.85, 0.6, [0.55, 0.90, 0.60], 0.5, 0.8), // murky green
        SeasonalForest  => mk([0.98, 0.84, 0.46], [0.98, 0.72, 0.40], [1.00, 0.80, 0.40], 0.55, 0.0045, [1.00, 0.88, 0.62], 0.50, 1.0, [1.00, 0.85, 0.55], 0.8, 1.1), // amber
        Savanna         => mk([0.55, 0.80, 0.90], [1.00, 0.74, 0.36], [1.00, 0.72, 0.30], 0.60, 0.0030, [1.00, 0.90, 0.72], 0.30, 1.3, [1.00, 0.90, 0.60], 1.0, 1.3),
        Shrubland       => mk([0.62, 0.62, 0.92], [0.97, 0.72, 0.66], [1.00, 0.78, 0.66], 0.40, 0.0035, [1.00, 0.86, 0.82], 0.35, 1.2, [1.00, 0.80, 0.80], 1.0, 1.1), // rose
        Taiga           => mk([0.40, 0.62, 0.80], [0.66, 0.80, 0.84], [0.78, 0.90, 0.92], 0.50, 0.0050, [0.82, 0.88, 0.92], 0.70, 0.9, [0.70, 0.90, 1.00], 1.0, 1.0), // steel mist
        Desert          => mk([0.60, 0.88, 0.80], [0.98, 0.84, 0.52], [1.00, 0.80, 0.40], 0.55, 0.0016, [0.96, 0.96, 0.82], 0.15, 1.4, [1.00, 0.92, 0.62], 1.0, 1.5), // turquoise to gold
        Tundra          => mk([0.50, 0.78, 0.98], [0.82, 0.92, 1.00], [0.90, 0.97, 1.00], 0.50, 0.0040, [0.92, 0.96, 1.00], 0.55, 0.8, [0.75, 0.95, 1.00], 1.0, 0.9), // ice
        Ocean           => mk([0.20, 0.50, 0.95], [0.58, 0.82, 0.98], [0.75, 0.92, 1.00], 0.35, 0.0016, [1.00, 1.00, 1.00], 0.60, 1.5, [0.80, 0.90, 1.00], 1.0, 1.2),
        Beach           => mk([0.42, 0.82, 0.95], [0.96, 0.92, 0.78], [1.00, 0.94, 0.74], 0.40, 0.0020, [1.00, 0.98, 0.92], 0.35, 1.2, [1.00, 0.97, 0.85], 1.0, 1.3),
        Mountains       => mk([0.22, 0.44, 0.90], [0.66, 0.80, 0.96], [0.80, 0.88, 1.00], 0.25, 0.0010, [0.96, 0.98, 1.00], 0.40, 1.6, [0.85, 0.92, 1.00], 1.0, 1.1), // thin, crisp air
        AutumnalForest  => mk([0.96, 0.64, 0.44], [0.96, 0.56, 0.36], [1.00, 0.66, 0.38], 0.55, 0.0050, [1.00, 0.78, 0.62], 0.55, 1.0, [1.00, 0.75, 0.55], 0.8, 1.1), // copper
        Chaparral       => mk([0.70, 0.64, 0.90], [1.00, 0.78, 0.58], [1.00, 0.74, 0.50], 0.50, 0.0030, [1.00, 0.90, 0.78], 0.30, 1.3, [1.00, 0.88, 0.70], 1.0, 1.3),
        Marsh           => mk([0.50, 0.88, 0.80], [0.62, 0.86, 0.70], [0.78, 1.00, 0.76], 0.50, 0.0075, [0.88, 1.00, 0.90], 0.75, 0.7, [0.70, 1.00, 0.80], 0.6, 0.9),
        OakWoodland     => mk([0.42, 0.74, 0.96], [0.78, 0.92, 0.80], [0.90, 1.00, 0.78], 0.40, 0.0035, [0.96, 1.00, 0.94], 0.50, 1.0, [0.90, 1.00, 0.90], 1.0, 1.0),
        WetWoodland     => mk([0.26, 0.80, 0.92], [0.40, 0.84, 0.76], [0.55, 0.98, 0.82], 0.50, 0.0060, [0.80, 0.98, 0.94], 0.65, 0.8, [0.55, 0.95, 0.95], 0.8, 0.9),
        SnowyBirchGrove => mk([0.62, 0.66, 0.98], [0.98, 0.80, 0.88], [1.00, 0.86, 0.92], 0.50, 0.0045, [1.00, 0.90, 0.95], 0.50, 0.9, [1.00, 0.85, 0.95], 1.0, 1.0), // blossom pink
    }
}

/// What the renderers read each frame (`WorldDraw::atmos`). Colours with a `linear` note are already in the frame's working space.
#[derive(Copy, Clone, Debug)]
pub struct Frame {
    /// Sky top and fog colour, vanilla gamma values (`lib.rs` adds the lightning flash and converts, like it always did).
    pub sky: [f32; 3],
    pub fog: [f32; 3],
    /// rgb (linear) and strength of the band of haze just over the horizon.
    pub haze: [f32; 4],
    /// rgb (linear, already lit for the time of day and rain) and alpha of the clouds.
    pub cloud: [f32; 4],
    /// Cloud threshold (higher = fewer), layer height, drift along +x in blocks, sky gradient exponent.
    pub cloudp: [f32; 4],
    /// `exp(-(d * density)²)` is what the fog leaves of a thing d blocks away, on top of the linear fog.
    pub density: f32,
    /// Star tint (linear rgb) and brightness multiplier.
    pub star: [f32; 4],
}

impl Frame {
    /// No haze, no clouds, plain white stars (the Nether, whose own colours the caller sets).
    pub const OFF: Frame = Frame { sky: [0.0; 3], fog: [0.0; 3], haze: [0.0; 4], cloud: [0.0; 4], cloudp: [1.0, CLOUD_Y, 0.0, 1.0], density: 0.0, star: [1.0, 1.0, 1.0, 1.0] };
}

pub struct Atmos {
    cur: [f32; N],
    goal: [f32; N],
    since: f32,
    ready: bool,
    cloud_x: f32,
}

impl Default for Atmos {
    fn default() -> Self {
        Self::new()
    }
}

impl Atmos {
    pub fn new() -> Self {
        Self { cur: of(Biome::Plains), goal: of(Biome::Plains), since: 0.0, ready: false, cloud_x: 0.0 }
    }

    /// The next `update` jumps to the look under the player instead of easing there (a new world, a dimension change).
    pub fn reset(&mut self) {
        self.ready = false;
    }

    /// `dt` real seconds; `at` the player's block column; `biome` the biome of a column.
    pub fn update(&mut self, dt: f32, at: (i32, i32), mut biome: impl FnMut(i32, i32) -> Biome) {
        self.since += dt;
        if !self.ready || self.since >= 0.25 {
            self.since = 0.0;
            self.goal = [0.0; N];
            for (dx, dz, w) in RING {
                let l = of(biome(at.0 + dx, at.1 + dz));
                self.goal.iter_mut().zip(l).for_each(|(g, v)| *g += v * w);
            }
        }
        let k = if self.ready { 1.0 - (-dt / TAU).exp() } else { 1.0 };
        self.cur.iter_mut().zip(self.goal).for_each(|(c, g)| *c += (g - *c) * k);
        self.ready = true;
        self.cloud_x += dt * DRIFT * self.cur[SPEED];
    }

    /// This frame's sky, fog, haze, clouds and stars for the sun `angle` (`world::sky::celestial_angle`) and the weather.
    pub fn frame(&self, angle: f32, rain: f32, thunder: f32) -> Frame {
        let c = &self.cur;
        let v3 = |i: usize| [c[i], c[i + 1], c[i + 2]];
        let sky_now = sky::sky_color(angle, REF_TEMP, rain, thunder);
        let sky_noon = sky::sky_color(0.0, REF_TEMP, 0.0, 0.0);
        let fog_now = sky::fog_color(angle, sky_now, rain, thunder);
        let fog_noon = sky::fog_color(0.0, sky_noon, 0.0, 0.0);
        // vanilla now x (this biome's noon colour / vanilla's noon colour): the same day curve, the biome's hue
        let tint = |now: [f32; 3], noon: [f32; 3], goal: [f32; 3]| -> [f32; 3] { std::array::from_fn(|i| (now[i] * goal[i] / noon[i].max(0.05)).min(1.0)) };
        let (top, fog) = (tint(sky_now, sky_noon, v3(Z)), tint(fog_now, fog_noon, v3(H)));
        // how much daylight the sky has (0 at night, less in rain): the haze and the stars' neighbours follow it
        let lum = |s: [f32; 3]| s[0] * 0.3 + s[1] * 0.59 + s[2] * 0.11;
        let day = (lum(sky_now) / lum(sky_noon)).clamp(0.0, 1.0);
        let haze = legacy_lin(v3(HZ).map(|v| v * day));
        // clouds: vanilla's white by time and weather, the biome tint, a little of the dusk colour of the fog
        let base = sky::cloud_color(angle, rain, thunder);
        let tinted = v3(CL);
        let cloud = legacy_lin(std::array::from_fn(|i| base[i] * tinted[i] * 0.8 + fog[i] * 0.2));
        let cover = (c[COVER] + 0.4 * rain).min(1.0);
        let star = legacy_lin(v3(ST));
        Frame {
            sky: top,
            fog,
            haze: [haze[0], haze[1], haze[2], c[HA] * (1.0 - 0.5 * rain)],
            cloud: [cloud[0], cloud[1], cloud[2], CLOUD_ALPHA],
            cloudp: [0.66 - 0.28 * cover, CLOUD_Y, self.cloud_x, c[CURVE]],
            density: c[DENS] * (1.0 + 1.5 * rain),
            star: [star[0], star[1], star[2], c[SMUL]],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(b: Biome) -> impl FnMut(i32, i32) -> Biome {
        move |_, _| b
    }

    #[test]
    fn noon_is_the_biomes_colour_and_night_is_dark() {
        let mut a = Atmos::new();
        a.update(0.016, (0, 0), all(Biome::Desert));
        let noon = a.frame(0.0, 0.0, 0.0);
        let want = of(Biome::Desert);
        for i in 0..3 {
            assert!((noon.sky[i] - want[Z + i]).abs() < 0.01, "sky {:?} vs {:?}", noon.sky, &want[Z..3]);
            assert!((noon.fog[i] - want[H + i]).abs() < 0.01, "fog {:?}", noon.fog);
        }
        let night = a.frame(0.5, 0.0, 0.0);
        assert!(night.sky.iter().all(|&v| v < 0.05) && night.haze[..3].iter().all(|&v| v < 0.01), "{night:?}");
        let storm = a.frame(0.0, 1.0, 1.0);
        assert!(storm.sky[1] < noon.sky[1] * 0.5 && storm.density > noon.density && storm.cloudp[0] < noon.cloudp[0]);
    }

    #[test]
    fn crossing_a_border_eases_over_seconds_without_a_jump() {
        let g = Z + 1; // the green of the sky top: Plains 0.62, Swamp 0.80
        let mut a = Atmos::new();
        a.update(0.016, (0, 0), all(Biome::Plains)); // the first update jumps
        let start = a.cur[g];
        let (goal, mut last, mut biggest) = (of(Biome::Swamp)[g], start, 0.0_f32);
        for _ in 0..600 {
            a.update(0.016, (0, 0), all(Biome::Swamp)); // ~9.6 s
            biggest = biggest.max((a.cur[g] - last).abs());
            last = a.cur[g];
        }
        assert!((last - goal).abs() < 0.01, "ended at {last}, goal {goal}");
        assert!(biggest < 0.02 * (goal - start).abs(), "one frame moved {biggest}");
        // one 1 s frame or sixty of 1/60 s end up close (the goal is looked up four times a second, so not equal)
        let (mut x, mut y) = (Atmos::new(), Atmos::new());
        x.update(0.0, (0, 0), all(Biome::Plains));
        y.update(0.0, (0, 0), all(Biome::Plains));
        x.update(1.0, (0, 0), all(Biome::Swamp));
        (0..60).for_each(|_| y.update(1.0 / 60.0, (0, 0), all(Biome::Swamp)));
        assert!((x.cur[g] - y.cur[g]).abs() < 0.03, "{} vs {}", x.cur[g], y.cur[g]);
    }

    #[test]
    fn a_ring_feels_a_border_before_it_is_crossed() {
        let g = Z + 1; // Plains 0.62, Forest 0.46
        let mut a = Atmos::new();
        // forest to the east beyond 40 blocks, plains elsewhere: only the (48, 0) sample is in it
        a.update(0.0, (0, 0), |x, _| if x >= 40 { Biome::Forest } else { Biome::Plains });
        let (p, f) = (of(Biome::Plains)[g], of(Biome::Forest)[g]);
        assert!((a.cur[g] - p).abs() > 0.01 && (a.cur[g] - p).abs() < (f - p).abs() * 0.5, "{} between {p} and {f}", a.cur[g]);
    }

    #[test]
    fn clouds_drift_with_the_speed_of_the_biome_and_reset_jumps() {
        let mut a = Atmos::new();
        a.update(0.0, (0, 0), all(Biome::Plains));
        a.update(10.0, (0, 0), all(Biome::Plains));
        assert!((a.frame(0.0, 0.0, 0.0).cloudp[2] - 6.0).abs() < 0.01, "10 s at 0.6 blocks/s");
        a.reset();
        a.update(0.016, (0, 0), all(Biome::Ocean));
        assert!((a.cur[Z] - of(Biome::Ocean)[Z]).abs() < 1e-6, "reset jumps");
    }

    #[test]
    fn every_look_is_sane() {
        use Biome::*;
        for b in [Rainforest, Swamp, SeasonalForest, Forest, Savanna, Shrubland, Taiga, Desert, Plains, Tundra, Ocean, Beach, Mountains, AutumnalForest, Chaparral, Marsh, OakWoodland, WetWoodland, SnowyBirchGrove] {
            let l = of(b);
            assert!(l.iter().all(|v| (0.0..=2.0).contains(v)), "{b:?}: {l:?}");
            assert!(l[DENS] > 0.0 && l[DENS] < 0.02 && l[CURVE] > 0.0, "{b:?}");
        }
    }
}
