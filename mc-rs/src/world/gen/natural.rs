//! The climate of the Natural Temperature mod (Fabric 1.2.1, by Caesius_Leo), ported: temperature and humidity laid out in bands (or
//! rings, or squares) round an equator instead of in noise blobs, so the biomes follow like the Earth's.
//!
//! Ported from the mod's bytecode (decompiled by hand, no source at hand; its licence allows reverse engineering): `MathUtils`,
//! `TemperatureCalculator`, `HumidityCalculator` (the eight modes each: default, simplified, linear, circular, banded, circular6,
//! square6, discrete) and `PainterCore` (the distortion, the waves, the contexts). `tools/natural_proto.py` is the same arithmetic in
//! Python, and `tools/golden/natural.txt` holds cases of the mod's own classes (run on a JVM, the Minecraft noise swapped for the two
//! sine stubs of `stub_noise`): the test at the bottom must give the same two numbers, to a float's precision.
//!
//! Not ported: the dimension filter and `multidimensional_bands` (the Nether has no climate here), `randomize_underground` and
//! `last_bedrock_y_level` (the climate is per column, never underground), the addon modes 101..105 (they fall back to the default
//! mode as the mod does for an unknown one), the mod's own config screen (`menu::Scr::GenSettings` is the new one), Terrablender.
//! UNVERIFIED: the two noises. The mod reads `NormalNoise` (first octave -14, amplitudes -14 x 3, and -7 with 1) on the game's
//! Perlin; `chunk_manager::Normal` is this crate's stand-in, so the bands wobble the same way in shape, not in the same places.
//!
//! The settings of a world are global while it is open (`set` when it loads, `current` for every `WorldChunkManager`: the chunk
//! workers, the far terrain and the spawn search all build their own). ponytail: one world at a time, which is how the game runs.
//! Upgrade: carry an `Arc<NaturalSettings>` down `WorldType`'s call chain instead.

use super::chunk_manager::Normal;
use crate::world::gen::noise::JavaRandom;
use std::sync::{Arc, RwLock};

/// "8.0": a zone value of this means random (the raw noise) and a transition point of this means automatic, as in the mod's config.
pub const AUTO: f64 = 8.0;

/// The mod's `ModConfig`, less what is not ported (see the header). `Default` is the mod's own defaults, with `enabled` off.
#[derive(Clone, Debug, PartialEq)]
pub struct NaturalSettings {
    /// The World-Gen Settings switch: off, the world keeps its Vanilla climate and nothing else here is read.
    pub enabled: bool,
    /// `generation_mode`: 0 default, 1 simplified, 2 linear, 3 / 4 circular (custom / preset), 5 / 6 banded, 7 / 8 circular6,
    /// 9 / 10 square6, 11 / 12 discrete.
    pub mode: i32,
    /// A quarter of the pole to pole distance, in blocks: the unit of every band.
    pub equatorial_distance: f64,
    pub equator_offset: f64,
    pub wave_magnification: f64,
    pub wave_frequency: f64,
    pub wave_amplitude: f64,
    pub looping_world: bool,
    /// 0..100: how much of the climate is the mod's (the rest is the Vanilla noise).
    pub influence: f64,
    pub distortion: i32,
    pub modifier: f64,
    pub mitigation: f64,
    pub temp_zone: [f64; 12],
    pub temp_transition: [f64; 5],
    pub temp_variability: [f64; 12],
    pub humidity_bands: bool,
    pub humidity_bands_width: f64,
    pub humidity_zone: [f64; 12],
    pub humidity_transition: [f64; 5],
    pub humidity_variability: [f64; 12],
    pub extra_bands: bool,
    pub coverage: [f64; 12],
    pub humidity_offset: f64,
}

impl Default for NaturalSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: 12,
            equatorial_distance: 20000.0,
            equator_offset: 0.0,
            wave_magnification: 2.0,
            wave_frequency: 1.0,
            wave_amplitude: 1.4,
            looping_world: false,
            influence: 100.0,
            distortion: 1,
            modifier: 0.0,
            mitigation: 0.0,
            temp_zone: [0.35, 0.35, 0.35, 0.55, 0.9, 0.82, 0.1, -0.1, -0.3, -0.75, -0.75, -0.8],
            temp_transition: [AUTO; 5],
            temp_variability: [0.1, 0.1, 0.1, 0.25, 0.1, 0.25, 0.05, 0.1, 0.15, 0.25, 0.25, 0.3],
            humidity_bands: false,
            humidity_bands_width: 1000.0,
            humidity_zone: [0.7, 0.2, -0.55, -0.5, -0.9, AUTO, AUTO, AUTO, AUTO, 0.5, 0.0, -1.0],
            humidity_transition: [AUTO; 5],
            humidity_variability: [0.35, 0.2, 0.5, 0.3, 0.1, 0.15, 1.0, 1.0, 1.0, 0.6, 0.4, 0.55],
            extra_bands: false,
            coverage: [10.0, 8.0, 10.0, 3.0, 7.0, 9.5, 11.5, 9.0, 10.0, 9.0, 5.0, 8.0],
            humidity_offset: 0.0,
        }
    }
}

fn nums(v: &str, into: &mut [f64]) {
    for (slot, s) in into.iter_mut().zip(v.split(',')) {
        if let Ok(x) = s.trim().parse::<f64>() {
            *slot = x;
        }
    }
}

impl NaturalSettings {
    /// The settings as `key=value` lines (a list is comma separated): a world's `natural` file, see `menu::Info`.
    pub fn to_text(&self) -> String {
        let l = |v: &[f64]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",");
        let b = |v: bool| u8::from(v);
        format!(
            "enabled={}\nmode={}\nequatorial_distance={}\nequator_offset={}\nwave_magnification={}\nwave_frequency={}\nwave_amplitude={}\nlooping_world={}\n\
             influence={}\ndistortion={}\nmodifier={}\nmitigation={}\ntemp_zone={}\ntemp_transition={}\ntemp_variability={}\nhumidity_bands={}\n\
             humidity_bands_width={}\nhumidity_zone={}\nhumidity_transition={}\nhumidity_variability={}\nextra_bands={}\ncoverage={}\nhumidity_offset={}\n",
            b(self.enabled), self.mode, self.equatorial_distance, self.equator_offset, self.wave_magnification, self.wave_frequency,
            self.wave_amplitude, b(self.looping_world), self.influence, self.distortion, self.modifier, self.mitigation, l(&self.temp_zone),
            l(&self.temp_transition), l(&self.temp_variability), b(self.humidity_bands), self.humidity_bands_width, l(&self.humidity_zone),
            l(&self.humidity_transition), l(&self.humidity_variability), b(self.extra_bands), l(&self.coverage), self.humidity_offset
        )
    }

    /// `to_text` read back: a missing or unreadable line keeps the default, the values are put back in range (`sane`).
    pub fn from_text(text: &str) -> Self {
        let mut s = Self::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim();
            let f = || v.parse::<f64>().ok();
            let on = || v == "1";
            match k.trim() {
                "enabled" => s.enabled = on(),
                "mode" => s.mode = v.parse().unwrap_or(s.mode),
                "equatorial_distance" => s.equatorial_distance = f().unwrap_or(s.equatorial_distance),
                "equator_offset" => s.equator_offset = f().unwrap_or(s.equator_offset),
                "wave_magnification" => s.wave_magnification = f().unwrap_or(s.wave_magnification),
                "wave_frequency" => s.wave_frequency = f().unwrap_or(s.wave_frequency),
                "wave_amplitude" => s.wave_amplitude = f().unwrap_or(s.wave_amplitude),
                "looping_world" => s.looping_world = on(),
                "influence" => s.influence = f().unwrap_or(s.influence),
                "distortion" => s.distortion = v.parse().unwrap_or(s.distortion),
                "modifier" => s.modifier = f().unwrap_or(s.modifier),
                "mitigation" => s.mitigation = f().unwrap_or(s.mitigation),
                "temp_zone" => nums(v, &mut s.temp_zone),
                "temp_transition" => nums(v, &mut s.temp_transition),
                "temp_variability" => nums(v, &mut s.temp_variability),
                "humidity_bands" => s.humidity_bands = on(),
                "humidity_bands_width" => s.humidity_bands_width = f().unwrap_or(s.humidity_bands_width),
                "humidity_zone" => nums(v, &mut s.humidity_zone),
                "humidity_transition" => nums(v, &mut s.humidity_transition),
                "humidity_variability" => nums(v, &mut s.humidity_variability),
                "extra_bands" => s.extra_bands = on(),
                "coverage" => nums(v, &mut s.coverage),
                "humidity_offset" => s.humidity_offset = f().unwrap_or(s.humidity_offset),
                _ => {}
            }
        }
        s.sane()
    }

    /// The values put back into what the mod's screen lets through (a hand-edited file cannot break the generator).
    pub fn sane(mut self) -> Self {
        self.mode = self.mode.clamp(0, 12);
        self.equatorial_distance = self.equatorial_distance.clamp(100.0, 1.0e6);
        self.humidity_bands_width = self.humidity_bands_width.clamp(10.0, 1.0e6);
        self.influence = self.influence.clamp(0.0, 100.0);
        self.modifier = self.modifier.clamp(-100.0, 100.0);
        self.mitigation = self.mitigation.clamp(0.0, 100.0);
        self.distortion = self.distortion.clamp(0, 10);
        for v in [&mut self.wave_magnification, &mut self.wave_frequency, &mut self.wave_amplitude] {
            *v = v.clamp(0.0, 100.0);
        }
        let zone = |v: &mut f64| *v = if *v == AUTO { AUTO } else { v.clamp(-1.0, 1.0) };
        self.temp_zone.iter_mut().chain(&mut self.temp_transition).chain(&mut self.humidity_zone).chain(&mut self.humidity_transition).for_each(zone);
        for v in self.temp_variability.iter_mut().chain(&mut self.humidity_variability) {
            *v = v.clamp(0.0, 1.0);
        }
        for v in &mut self.coverage {
            *v = v.clamp(0.0, 100.0);
        }
        self
    }
}

static CURRENT: RwLock<Option<Arc<NaturalSettings>>> = RwLock::new(None);

/// The settings of the world being played (`None` when its switch is off or no world is open): every `WorldChunkManager` built
/// from now on takes them.
pub fn set(s: Option<&NaturalSettings>) {
    if let Ok(mut w) = CURRENT.write() {
        *w = s.filter(|s| s.enabled).cloned().map(Arc::new);
    }
}

pub fn current() -> Option<Arc<NaturalSettings>> {
    CURRENT.read().ok().and_then(|r| r.clone())
}

// ---- MathUtils ----

const K1: u64 = -3335678366873096957i64 as u64;
const K2: u64 = -7046029254386353131i64 as u64;
const C1: u64 = -4658895280553007687i64 as u64;
const C2: u64 = -7723592293110705685i64 as u64;

/// `MathUtils.random01`: a splitmix-style hash of (seed, index, salt) in 0..1.
fn random01(seed: i64, idx: i64, salt: i64) -> f64 {
    let mut h = (seed as u64).wrapping_add((salt as u64).wrapping_mul(K1)).wrapping_add((idx as u64).wrapping_mul(K2));
    h = (h ^ (h >> 30)).wrapping_mul(C1);
    h = (h ^ (h >> 27)).wrapping_mul(C2);
    h ^= h >> 31;
    (h >> 11) as f64 / 9007199254740992.0
}

fn point_position(idx: i64, scale: f64, seed: i64) -> f64 {
    (idx as f64 + (0.25 + 0.5 * random01(seed, idx, 1))) * scale
}

/// `MathUtils.calculateNaturalWave`: a smooth random curve of `x`, one random point per `1 / frequency` blocks, `amp` high.
fn natural_wave(x: f64, amp: f64, freq: f64, seed: i64) -> f64 {
    if amp == 0.0 || !(freq > 0.0) {
        return 0.0;
    }
    let w = 1.0 / freq;
    let mut i = (x / w).floor() as i64;
    if x < point_position(i, w, seed) {
        i -= 1;
    }
    let (p0, p1) = (point_position(i, w, seed), point_position(i + 1, w, seed));
    let v0 = (random01(seed, i, 2) * 2.0 - 1.0) * amp;
    let v1 = (random01(seed, i + 1, 2) * 2.0 - 1.0) * amp;
    let t = (x - p0) / (p1 - p0);
    v0 + (v1 - v0) * (t * t * (3.0 - 2.0 * t))
}

/// The value on the border of zones `i` and `j`: halfway, or the transition point (NaN: automatic), kept between the two zones'.
fn border(a: f64, b: f64, borders: &[f64], i: usize, j: usize) -> f64 {
    let mut mid = (a + b) / 2.0;
    if j == i + 1 && i < borders.len() && !borders[i].is_nan() {
        mid = borders[i];
    }
    a.min(b).max(b.max(a).min(mid))
}

/// `MathUtils.interpolateWithPeaksAndWidth`: zone `j` is `widths[j]` of the 0..1 axis (the first one's peak at its start, the last
/// one's at its end, the others' in the middle), the value goes from peak to peak through the border values.
fn interp(x: f64, vals: &[f64], widths: &[f64], borders: &[f64]) -> f64 {
    let n = vals.len();
    let total: f64 = widths.iter().map(|w| w.max(0.0)).sum();
    if !(total > 0.0) {
        return vals[0];
    }
    let (mut pos, mut val) = (Vec::with_capacity(2 * n), Vec::with_capacity(2 * n));
    let (mut cum, mut prev) = (0.0, None::<usize>);
    for j in 0..n {
        let w = widths[j].max(0.0) / total;
        if w < 1e-9 {
            continue;
        }
        let end = cum + w;
        let peak = if j == 0 { cum } else if j == n - 1 { end } else { (cum + end) / 2.0 };
        if let Some(p) = prev {
            pos.push(cum);
            val.push(border(vals[p], vals[j], borders, p, j));
        }
        pos.push(peak);
        val.push(vals[j]);
        (prev, cum) = (Some(j), end);
    }
    let last = pos.len() - 1;
    if x <= pos[0] {
        return val[0];
    }
    if x >= pos[last] {
        return val[last];
    }
    for k in 0..last {
        if x <= pos[k + 1] {
            return val[k] + (val[k + 1] - val[k]) * ((x - pos[k]) / (pos[k + 1] - pos[k]));
        }
    }
    val[last]
}

// ---- the contexts and the calculators ----

/// `TemperatureContext` and `HumidityContext` in one (the fields the calculators read), made by `climate_with`.
struct Ctx {
    a: f64,
    m: i32,
    /// The raw temperature / humidity noise at the point (`random_t` / `random_u`).
    rnd: f64,
    rnd_u: f64,
    mit: f64,
    modi: f64,
    adj_z: f64,
    adj_x: f64,
    complex_z: f64,
    mad_z: f64,
    jungle: f64,
    desert2: f64,
    desert: f64,
    sav2: f64,
    temp2: f64,
    pole2: f64,
    cold3: f64,
    zp: f64,
    s_pole: f64,
    n_pole: f64,
    radius: f64,
    r: f64,
    r_sq: f64,
    hbw: f64,
    /// The temperature (as a float, as the mod passes it) the humidity default mode looks at.
    t: f64,
}

fn pick(v: f64, rnd: f64) -> f64 {
    if v == AUTO { rnd } else { v }
}

/// The transition points of a custom ring mode: the first one defaults to the centre's value, the others to automatic (NaN).
fn first_or_nan(tp: &[f64], first: f64) -> Vec<f64> {
    tp.iter().enumerate().map(|(k, &v)| if v != AUTO { v } else if k == 0 { first } else { f64::NAN }).collect()
}

fn nan_auto(tp: &[f64]) -> Vec<f64> {
    tp.iter().map(|&v| if v == AUTO { f64::NAN } else { v }).collect()
}

/// Distance from the pole of the folded axis, 0..2a (banded and discrete).
fn fold(c: &Ctx) -> f64 {
    let a = c.a;
    let l2 = ((c.mad_z - a) % (4.0 * a) + 4.0 * a) % (4.0 * a);
    if l2 > 2.0 * a { 4.0 * a - l2 } else { l2 }
}

fn tri(c: &Ctx) -> f64 {
    0.6366197723675814 * (3.141592653589793 * c.adj_z / (2.0 * c.a)).sin().asin()
}

fn outside(c: &Ctx) -> bool {
    c.complex_z < -c.a || c.complex_z > 3.0 * c.a
}

fn modded(v: f64, c: &Ctx) -> f64 {
    (v * c.mit + c.modi).min(1.0).max(-1.0)
}

fn band_wave(c: &Ctx) -> f64 {
    0.6366197723675814 * (3.141592653589793 * c.adj_x / (2.0 * c.hbw)).sin().asin()
}

fn widths6(g: &NaturalSettings) -> Vec<f64> {
    let mut w = g.coverage[..6].to_vec();
    if g.looping_world {
        w[0] /= 2.0;
        w[5] /= 2.0;
    }
    w
}

fn t_default(g: &NaturalSettings, c: &Ctx) -> f64 {
    let (a, mad) = (c.a, c.mad_z);
    let v = if a == 0.0 {
        -1.0
    } else {
        let sine = if mad < 0.0 { false } else if mad <= c.jungle { true } else { !(mad < c.desert2) && !(mad > 4.0 * a) };
        if sine {
            let v = 0.7639437268410976 * (3.141592653589793 * c.adj_z / (2.0 * a)).sin().asin() + 0.2;
            (v * c.mit + c.modi).min(1.0).max(-1.0)
        } else if mad <= c.jungle || mad >= c.desert2 {
            -1.0
        } else {
            let v = 2.0 * (2.5 * (3.141592653589793 * c.adj_z / a).cos()).tanh() + 2.3;
            0.9 * v + 0.1 * c.rnd
        }
    };
    if !g.looping_world && (c.zp <= -a || c.zp >= 3.0 * a) { -1.0 } else { v }
}

fn t_simplified(g: &NaturalSettings, c: &Ctx) -> f64 {
    if c.a == 0.0 {
        -1.0
    } else if g.looping_world || !outside(c) {
        modded(tri(c), c)
    } else {
        -1.0
    }
}

fn t_linear(_: &NaturalSettings, c: &Ctx) -> f64 {
    modded(((1.0 / c.a) * c.adj_z).min(1.0).max(-1.0), c)
}

#[allow(clippy::too_many_arguments)]
/// The ring modes (circular, circular6, square6): `n` zones from the origin outwards, `x` the distance (round or square).
fn ring(g: &NaturalSettings, c: &Ctx, n: usize, custom: i32, preset: i32, defaults: Vec<f64>, preset_vals: Vec<f64>, x: f64) -> f64 {
    let cov = if c.m == preset { vec![1.0; n] } else { g.coverage[..n].to_vec() };
    let vals = if c.m == custom {
        g.temp_zone[..n].iter().map(|&v| pick(v, c.rnd)).collect()
    } else if c.m == preset {
        preset_vals
    } else {
        defaults
    };
    let tp = if c.m == custom {
        first_or_nan(&g.temp_transition[..n - 1], vals[0])
    } else {
        let mut t = vec![f64::NAN; n - 1];
        t[0] = vals[0];
        t
    };
    if c.a == 0.0 { -1.0 } else { interp((x / c.radius).max(0.0).min(1.0), &vals, &cov, &tp) }
}

fn t_circular(g: &NaturalSettings, c: &Ctx) -> f64 {
    ring(g, c, 4, 3, 4, vec![0.0, c.rnd, 1.0, -1.0], vec![0.0, 1.0, -0.5, -1.0], c.r)
}

fn t_circular6(g: &NaturalSettings, c: &Ctx) -> f64 {
    ring(g, c, 6, 7, 8, vec![0.0, c.rnd, 0.35, 1.0, c.rnd, -1.0], vec![0.0, 0.25, 0.4, 1.0, -0.25, -1.0], c.r)
}

fn t_square6(g: &NaturalSettings, c: &Ctx) -> f64 {
    ring(g, c, 6, 9, 10, vec![0.0, c.rnd, 0.35, 1.0, c.rnd, -1.0], vec![0.0, 0.25, 0.4, 1.0, -0.25, -1.0], c.r_sq)
}

fn t_banded(g: &NaturalSettings, c: &Ctx) -> f64 {
    if c.a == 0.0 {
        return -1.0;
    }
    let x = (fold(c) / (2.0 * c.a)).max(0.0).min(1.0);
    let (vals, tp) = if c.m == 6 {
        (vec![0.25, 0.7, 0.5, 0.0, -0.25, -0.7], vec![0.55, 0.55, 0.2, -0.15, -0.45])
    } else {
        (g.temp_zone[..6].iter().map(|&v| pick(v, c.rnd)).collect::<Vec<_>>(), nan_auto(&g.temp_transition))
    };
    let v = modded(interp(x, &vals, &widths6(g), &tp), c);
    if !g.looping_world && outside(c) { vals[5] } else { v }
}

/// The twelve zones of the discrete modes: each a flat value plus its variability times the raw noise.
fn discrete(g: &NaturalSettings, c: &Ctx, vals: &[f64; 12], var: &[f64; 12], rnd: f64, bands: bool) -> f64 {
    if c.a == 0.0 {
        return -1.0;
    }
    let mut w = g.coverage;
    if g.looping_world {
        w[0] /= 2.0;
        w[11] /= 2.0;
    }
    let k = 2.0 * c.a / w.iter().sum::<f64>();
    let f = fold(c);
    let (mut run, mut z) = (0.0, 11);
    for (i, wi) in w[..11].iter().enumerate() {
        run += wi;
        if f < run * k {
            z = i;
            break;
        }
    }
    let mut v = (vals[z] + var[z] * rnd).min(1.0).max(-1.0);
    if bands {
        v = band_wave(c);
    }
    if !g.looping_world && outside(c) { vals[11] } else { v }
}

fn t_discrete(g: &NaturalSettings, c: &Ctx) -> f64 {
    let mut vals = [0.35, 0.35, 0.35, 0.55, 0.9, 0.82, 0.1, -0.1, -0.3, -0.75, -0.75, -0.8];
    let mut var = [0.1, 0.1, 0.1, 0.25, 0.1, 0.25, 0.05, 0.1, 0.15, 0.25, 0.25, 0.3];
    if c.m != 12 {
        vals = g.temp_zone.map(|v| pick(v, c.rnd));
        var = g.temp_variability;
    }
    discrete(g, c, &vals, &var, c.rnd, false)
}

fn temperature(g: &NaturalSettings, c: &Ctx) -> f64 {
    match c.m {
        1 => t_simplified(g, c),
        2 => t_linear(g, c),
        3 | 4 => t_circular(g, c),
        5 | 6 => t_banded(g, c),
        7 | 8 => t_circular6(g, c),
        9 | 10 => t_square6(g, c),
        11 | 12 => t_discrete(g, c),
        _ => t_default(g, c),
    }
}

fn h_default(g: &NaturalSettings, c: &Ctx) -> f64 {
    let (a, mad, u) = (c.a, c.mad_z, c.rnd_u);
    let mid = c.jungle <= mad && mad <= c.desert2;
    let v = if a == 0.0 {
        -1.0
    } else if (mad >= 0.0 && mad <= c.desert) || !(mad < c.sav2 || mad > c.temp2) {
        -1.0
    } else if g.extra_bands && mid {
        0.7 * (0.8 * (2.0 * (6.283185307179586 * c.adj_z / a).cos()).tanh() + 0.15) + 0.3 * u
    } else if mid {
        let s = (6.283185307179586 * c.adj_z / a).cos();
        0.7 * (if s > 0.0 { 1.0 } else if s < 0.0 { -1.0 } else { s }) + 0.3 * u
    } else if c.pole2 <= mad && mad <= c.cold3 {
        -1.0
    } else if (c.desert <= mad && mad <= c.jungle) || (c.desert2 <= mad && mad <= c.sav2) {
        if c.t > 0.65 { u } else { -1.0 }
    } else {
        u
    };
    if !g.looping_world && (c.zp >= c.s_pole || c.zp <= c.n_pole) { -1.0 } else { v }
}

fn h_simplified(g: &NaturalSettings, c: &Ctx) -> f64 {
    let v = if g.humidity_bands { band_wave(c) } else { c.rnd_u };
    if !g.looping_world && outside(c) { -1.0 } else { v }
}

fn h_linear(g: &NaturalSettings, c: &Ctx) -> f64 {
    let v = if g.humidity_bands { band_wave(c) } else { c.rnd_u };
    if c.complex_z < -c.a { -1.0 } else { v }
}

fn h_ring(g: &NaturalSettings, c: &Ctx, n: usize, custom: i32, preset: i32, x: f64, defaults: Vec<f64>) -> f64 {
    let u = c.rnd_u;
    let cov = if c.m == preset { vec![1.0; n] } else { g.coverage[..n].to_vec() };
    let vals = if c.m == custom {
        g.humidity_zone[..n].iter().map(|&v| pick(v, u)).collect()
    } else if c.m == preset {
        vec![u; n]
    } else {
        defaults
    };
    let tp = if c.m == custom {
        first_or_nan(&g.humidity_transition[..n - 1], vals[0])
    } else {
        let mut t = vec![f64::NAN; n - 1];
        t[0] = vals[0];
        t
    };
    let v = if c.a == 0.0 { -1.0 } else { interp((x / c.radius).max(0.0).min(1.0), &vals, &cov, &tp) };
    if g.humidity_bands { band_wave(c) } else { v }
}

fn h_circular(g: &NaturalSettings, c: &Ctx) -> f64 {
    h_ring(g, c, 4, 3, 4, c.r, vec![c.rnd_u, c.rnd_u, c.rnd_u, -1.0])
}

fn h_circular6(g: &NaturalSettings, c: &Ctx) -> f64 {
    h_ring(g, c, 6, 7, 8, c.r, vec![c.rnd_u, c.rnd_u, c.rnd_u, c.rnd_u, c.rnd_u, -1.0])
}

fn h_square6(g: &NaturalSettings, c: &Ctx) -> f64 {
    h_ring(g, c, 6, 9, 10, c.r_sq, vec![c.rnd_u, c.rnd_u, c.rnd_u, c.rnd_u, c.rnd_u, -1.0])
}

fn h_banded(g: &NaturalSettings, c: &Ctx) -> f64 {
    if c.a == 0.0 {
        return -1.0;
    }
    let u = c.rnd_u;
    let x = (fold(c) / (2.0 * c.a)).max(0.0).min(1.0);
    let (vals, tp) = if c.m == 6 {
        (vec![1.0, u, -0.7, u, u, -0.15], vec![f64::NAN, -0.2, -0.2, f64::NAN, f64::NAN])
    } else {
        (g.humidity_zone[..6].iter().map(|&v| pick(v, u)).collect::<Vec<_>>(), nan_auto(&g.humidity_transition))
    };
    let mut v = interp(x, &vals, &widths6(g), &tp);
    if g.humidity_bands {
        v = band_wave(c);
    }
    if !g.looping_world && outside(c) { vals[5] } else { v }
}

fn h_discrete(g: &NaturalSettings, c: &Ctx) -> f64 {
    let u = c.rnd_u;
    let mut vals = [0.7, 0.2, -0.55, -0.5, -0.9, u, u, u, u, 0.5, 0.0, -1.0];
    let mut var = [0.35, 0.2, 0.5, 0.3, 0.1, 0.15, 1.0, 1.0, 1.0, 0.6, 0.4, 0.55];
    if c.m != 12 {
        vals = g.humidity_zone.map(|v| pick(v, u));
        var = g.humidity_variability;
    }
    discrete(g, c, &vals, &var, u, g.humidity_bands)
}

fn humidity(g: &NaturalSettings, c: &Ctx) -> f64 {
    match c.m {
        1 => h_simplified(g, c),
        2 => h_linear(g, c),
        3 | 4 => h_circular(g, c),
        5 | 6 => h_banded(g, c),
        7 | 8 => h_circular6(g, c),
        9 | 10 => h_square6(g, c),
        11 | 12 => h_discrete(g, c),
        _ => h_default(g, c),
    }
}

#[allow(clippy::too_many_arguments)]
/// `PainterCore`: the temperature and humidity at block (x, z) from the raw noise there and the two distortion noises' values at
/// (6x, 6z) (`nl`: the large one, `nd`: the detail one), before the influence blend.
fn climate_with(g: &NaturalSettings, x: i32, z: i32, seed: i64, raw_t: f64, raw_u: f64, nl: f64, nd: f64) -> (f64, f64) {
    let (fx, fz) = (x as f64, z as f64);
    let (a, off) = (g.equatorial_distance, g.equator_offset);
    let wm = if g.wave_magnification != 0.0 { g.wave_magnification } else { 0.01 };
    let amp = g.wave_amplitude * (30.0 * wm);
    let wave_t = natural_wave(fx, amp, g.wave_frequency * (0.001 * (1.0 + raw_t * 0.01) / wm), seed);
    // sic: the mod divides by the wave's *amplitude* setting here, not the frequency's
    let wave_h = natural_wave(fx, amp, g.wave_amplitude * (0.001 * (1.0 + raw_u * 0.01) / wm), seed);
    let dist = (nl * 100.0 + nd * 30.0) * g.distortion as f64;
    let mut c = Ctx {
        a,
        m: g.mode,
        rnd: raw_t,
        rnd_u: raw_u,
        mit: 1.0 - (g.mitigation / 100.0).abs(),
        modi: g.modifier / 100.0,
        adj_z: ((fz + dist) + wave_t) + off,
        adj_x: ((fx + dist) + wave_h) + g.humidity_offset,
        complex_z: ((fz + off) + dist) + wave_t,
        mad_z: (((((fz + off) + dist) + wave_t) % (4.0 * a)) + 4.0 * a) % (4.0 * a),
        jungle: 0.6666666666666666 * a,
        desert2: 1.3333333333333333 * a,
        desert: 0.3333333333333333 * a,
        sav2: 1.6666666666666667 * a,
        temp2: 2.0 * a,
        pole2: 2.6666666666666665 * a,
        cold3: 3.3333333333333335 * a,
        zp: fz + off,
        s_pole: (dist + wave_t) + 3.0 * a,
        n_pole: (dist + wave_t) - 1.0 * a,
        radius: (a + dist) + wave_t,
        r: (fx * fx + fz * fz).sqrt(),
        r_sq: fx.abs().max(fz.abs()),
        hbw: g.humidity_bands_width,
        t: 0.0,
    };
    let t = temperature(g, &c) as f32 as f64;
    c.t = t;
    (t, humidity(g, &c) as f32 as f64)
}

/// The mod's climate for one world: the two distortion noises of its seed and the settings.
pub struct Climate {
    g: Arc<NaturalSettings>,
    seed: i64,
    large: Normal,
    detail: Normal,
}

impl Climate {
    pub fn new(g: Arc<NaturalSettings>, seed: i64) -> Self {
        let mut r = JavaRandom::new(seed); // the mod's one `LegacyRandomSource` feeds both noises, the large one first
        let large = Normal::from_rng(&mut r, -14, &[-14.0, -14.0, -14.0]);
        let detail = Normal::from_rng(&mut r, -7, &[1.0]);
        Self { g, seed, large, detail }
    }

    /// (temperature, humidity), -1..1, at block (x, z) where the Vanilla noise is `raw_t` / `raw_u`: the mod's, blended with the raw
    /// ones by the influence (`CustomTemperatureSampler`).
    pub fn sample(&self, x: i32, z: i32, raw_t: f64, raw_u: f64) -> (f64, f64) {
        let (px, pz) = (x as f64 * 6.0, z as f64 * 6.0);
        let (t, u) = climate_with(&self.g, x, z, self.seed, raw_t, raw_u, self.large.get(px, pz), self.detail.get(px, pz));
        let k = (self.g.influence / 100.0) as f32 as f64;
        (k * t + (1.0 - k) * raw_t, k * u + (1.0 - k) * raw_u)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOLDEN: &str = include_str!("../../../../tools/golden/natural.txt");

    /// `Harness.java`'s stand-ins for the two noises (it cannot load Minecraft's).
    fn stub_noise(x: i32, z: i32) -> (f64, f64) {
        let (px, pz) = (x as f64 * 6.0, z as f64 * 6.0);
        ((px * 0.00013 + pz * 0.00007).sin() * 3.0 + (pz * 0.00011).cos(), (px * 0.011).sin() * (pz * 0.017).cos() * 0.8)
    }

    /// Every case of the golden file (the mod's own `PainterCore` on a JVM) gives the same temperature and humidity.
    #[test]
    fn matches_the_mods_own_classes() {
        let mut n = 0;
        for line in GOLDEN.lines() {
            let mut it = line.split_whitespace();
            let mut f = || it.next().unwrap().parse::<f64>().unwrap();
            let mut g = NaturalSettings::default();
            g.mode = f() as i32;
            (g.equatorial_distance, g.equator_offset) = (f(), f());
            (g.wave_magnification, g.wave_frequency, g.wave_amplitude) = (f(), f(), f());
            g.looping_world = f() == 1.0;
            g.distortion = f() as i32;
            (g.modifier, g.mitigation) = (f(), f());
            g.temp_zone = std::array::from_fn(|_| f());
            g.temp_transition = std::array::from_fn(|_| f());
            g.temp_variability = std::array::from_fn(|_| f());
            g.humidity_bands = f() == 1.0;
            g.humidity_bands_width = f();
            g.humidity_zone = std::array::from_fn(|_| f());
            g.humidity_transition = std::array::from_fn(|_| f());
            g.humidity_variability = std::array::from_fn(|_| f());
            g.extra_bands = f() == 1.0;
            g.coverage = std::array::from_fn(|_| f());
            g.humidity_offset = f();
            let seed: i64 = it.next().unwrap().parse().unwrap();
            let mut f = || it.next().unwrap().parse::<f64>().unwrap();
            let (x, _y, z) = (f() as i32, f(), f() as i32);
            let (rt, ru, want_t, want_h) = (f(), f(), f(), f());
            let (nl, nd) = stub_noise(x, z);
            let (t, h) = climate_with(&g, x, z, seed, rt, ru, nl, nd);
            assert!((t - want_t).abs() < 2e-5 && (h - want_h).abs() < 2e-5, "mode {} ({x}, {z}): {t} {h} vs {want_t} {want_h}", g.mode);
            n += 1;
        }
        assert!(n >= 100, "the golden file is short: {n}");
    }

    #[test]
    fn settings_round_trip() {
        let mut s = NaturalSettings { enabled: true, mode: 5, ..Default::default() };
        s.temp_zone[3] = AUTO;
        s.coverage[11] = 2.5;
        assert_eq!(NaturalSettings::from_text(&s.to_text()), s);
        assert_eq!(NaturalSettings::from_text("mode=99\nequatorial_distance=-5\njunk"), NaturalSettings { mode: 12, equatorial_distance: 100.0, ..Default::default() });
    }

    /// With no distortion and no wave the default (discrete) mode is plain bands of z: zone 6 (0.82) at 1.8 a, the pole's zone 12 (-0.8)
    /// past -a, and the sample is a plain function of the point.
    #[test]
    fn bands_run_cold_to_hot() {
        let g = Arc::new(NaturalSettings { enabled: true, equatorial_distance: 1000.0, distortion: 0, wave_amplitude: 0.0, ..Default::default() });
        let c = Climate::new(g, 42);
        assert!((c.sample(0, 1800, 0.0, 0.0).0 - 0.82).abs() < 1e-6);
        assert!((c.sample(0, -2000, 0.0, 0.0).0 + 0.8).abs() < 1e-6);
        assert_eq!(c.sample(123, -456, 0.2, -0.3), c.sample(123, -456, 0.2, -0.3));
    }
}
