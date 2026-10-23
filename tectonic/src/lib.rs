//! Tectonic's terrain, in Rust. Tectonic (Apollo, MIT) is a datapack of ~80 density functions plus a config; this crate reads
//! that data (`terrain.json`, made by `tools/tectonic.py bundle` from the mod's resource pack, overlays applied) and evaluates it
//! the way the game's `NoiseChunk` does, so the terrain is the mod's and not an imitation of it.
//!
//! What is here: Minecraft's `Xoroshiro128++`, `ImprovedNoise` / `PerlinNoise` / `NormalNoise` / `BlendedNoise`, the density-function
//! node types the terrain uses, cubic splines (in `f32`, as the game does), the mod's `config_*` nodes with `ConfigState`'s
//! defaults, and a chunk filler that evaluates `final_density` on the 4 x 8 x 4 cell corners and interpolates between them.
//!
//! What is not: caves, noodle caves, underground rivers and lava tunnels (`tectonic:caves`, `cave/noodle`, `underground_river/total`,
//! `lava_tunnel/total` are stubbed to "nothing here"; mc-rs keeps its own b1.7.3 caves), biomes, surface rules, aquifers, blending
//! with old chunks, and the `ultrasmooth` option.
//!
//! The graph is rewritten once when it is built (`Tectonic::simplify`, after C2ME's density function compiler): wrappers dropped,
//! constants folded, constant `range_choice`s cut, equal nodes merged, all to the bit. Evaluation keeps one memo slot per node, valid
//! for the current column (nodes that do not depend on y) or point (the others), so nothing is hashed or cleared.
//!
//! The `cache_*` / `flat_cache` / `blend_density` wrappers are the identity: the game evaluates the whole tree on cell corners
//! (x, z a multiple of 4, y a multiple of 8), where a quart-resolution cache returns the very same value. Only `interpolated`
//! does something (trilinear between corners), and everything that does not depend on y is computed once per corner column.
//!
//! UNVERIFIED (written from memory of the 1.21 sources, no vanilla jar or toolchain was at hand; `tools/tectonic.py` is the same
//! arithmetic a second time and agrees with this file, which only shows the two copies match each other): the vanilla pieces
//! the pack does not carry: the `Xoroshiro` seeding and `fromHashOf` / `octave_N` forking, `NormalNoise`'s value factor, the
//! legacy octave order of `BlendedNoise`, `shift_x` / `shift_z`, and the noise parameters of `offset`, `jagged`, `temperature`,
//! `vegetation`. If they are right a seed gives the mod's continents; if one is wrong the terrain is still Tectonic-shaped.

use std::collections::HashMap;

const BUNDLE: &str = include_str!("../terrain.json");

// ------------------------------------------------------------------------------------------------ config

/// `ConfigState` (the mod's config) as far as the terrain reads it. `min_y` / `max_y` are in the datapack's frame, where the sea is
/// at y = 63: a shorter world shifts them (see `compact`). Noise scales: multiplier 1 and offset 0, the mod's defaults.
#[derive(Clone, Debug)]
pub struct Config {
    pub min_y: i32,
    pub max_y: i32,
    pub vertical_scale: f64,
    pub elevation_boost: f64,
    pub ocean_offset: f64,
    pub flat_terrain_skew: f64,
    pub rolling_hills: bool,
    pub jungle_pillars: bool,
    pub ocean_depth: f64,
    pub deep_ocean_depth: f64,
    pub continents_scale: f64,
    pub erosion_scale: f64,
    pub ridge_scale: f64,
    pub island_scale: f64,
    pub temperature_scale: f64,
    pub vegetation_scale: f64,
}

impl Default for Config {
    /// The mod's defaults: the 1.18+ overworld, y -64 .. 320.
    fn default() -> Self {
        Config {
            min_y: -64,
            max_y: 320,
            vertical_scale: 1.125,
            elevation_boost: 0.0,
            ocean_offset: -0.8,
            flat_terrain_skew: 0.1,
            rolling_hills: true,
            jungle_pillars: true,
            ocean_depth: -0.22,
            deep_ocean_depth: -0.45,
            continents_scale: 0.13,
            erosion_scale: 0.25,
            ridge_scale: 0.25,
            island_scale: 0.11,
            temperature_scale: 0.25,
            vegetation_scale: 0.25,
        }
    }
}

impl Config {
    /// A world `height` blocks tall (a multiple of 8) whose sea surface is at `sea_level` (water below it), e.g. b1.7.3's 128 / 64.
    /// The datapack's sea is at y = 63, so the frame is shifted to put it there; the mountains of a 384-high world do not fit
    /// in 64 blocks above the sea, so heights above sea level are scaled by 0.3 (`vertical_scale`; the mod's 1.125 saturates
    /// against `max_y` almost everywhere). 0.3 was picked by looking at height maps of the reference evaluator
    /// (`tools/tectonic.py map`): about 1 % of the land reaches the ceiling, the median is ~19 blocks above the sea.
    pub fn compact(height: i32, sea_level: i32) -> Self {
        assert!(height > 0 && height % 8 == 0, "height must be a multiple of 8");
        let min_y = 63 - sea_level;
        Config { min_y, max_y: min_y + height, vertical_scale: 0.3, ..Config::default() }
    }

    fn constant(&self, key: &str) -> Result<f64, String> {
        Ok(match key {
            "vertical_scale" => self.vertical_scale,
            "elevation_boost" => self.elevation_boost,
            "min_y" => self.min_y as f64,
            "max_y" => self.max_y as f64,
            "ocean_offset" => self.ocean_offset,
            "flat_terrain_skew" => self.flat_terrain_skew,
            "rolling_hills" => self.rolling_hills as u8 as f64,
            "jungle_pillars" => self.jungle_pillars as u8 as f64,
            "ocean_depth" => self.ocean_depth,
            "deep_ocean_depth" => self.deep_ocean_depth,
            _ => return Err(format!("unknown config constant {key}")),
        })
    }

    fn scale(&self, key: &str) -> Result<f64, String> {
        Ok(match key {
            "continents" => self.continents_scale,
            "erosion" => self.erosion_scale,
            "ridge" => self.ridge_scale,
            "island" => self.island_scale,
            "temperature" => self.temperature_scale,
            "vegetation" => self.vegetation_scale,
            _ => return Err(format!("unknown config noise {key}")),
        })
    }
}

// ------------------------------------------------------------------------------------------------ JSON (just enough for terrain.json)

enum J {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    fn get(&self, k: &str) -> Option<&J> {
        match self {
            J::Obj(o) => o.iter().find(|(n, _)| n == k).map(|(_, v)| v),
            _ => None,
        }
    }
    fn num(&self) -> Option<f64> {
        match self {
            J::Num(n) => Some(*n),
            _ => None,
        }
    }
    fn str(&self) -> Option<&str> {
        match self {
            J::Str(s) => Some(s),
            _ => None,
        }
    }
    fn arr(&self) -> &[J] {
        match self {
            J::Arr(a) => a,
            _ => &[],
        }
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> u8 {
        self.s.get(self.i).copied().unwrap_or(0)
    }
    fn ws(&mut self) {
        while self.peek().is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn eat(&mut self, c: u8) -> Result<(), String> {
        self.ws();
        if self.peek() == c {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("json: expected '{}' at byte {}", c as char, self.i))
        }
    }
    fn string(&mut self) -> Result<String, String> {
        self.eat(b'"')?;
        let mut out = Vec::new();
        loop {
            let c = self.peek();
            self.i += 1;
            match c {
                0 => return Err("json: unterminated string".into()),
                b'"' => break,
                b'\\' => {
                    let e = self.peek();
                    self.i += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'u' => {
                            let h = std::str::from_utf8(self.s.get(self.i..self.i + 4).ok_or("json: short \\u")?).map_err(|e| e.to_string())?;
                            let c = char::from_u32(u32::from_str_radix(h, 16).map_err(|e| e.to_string())?).unwrap_or('?');
                            self.i += 4;
                            out.extend_from_slice(c.to_string().as_bytes());
                        }
                        other => out.push(other), // \" \\ \/
                    }
                }
                c => out.push(c),
            }
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    }
    fn val(&mut self) -> Result<J, String> {
        self.ws();
        match self.peek() {
            b'{' => {
                self.i += 1;
                let mut o = Vec::new();
                loop {
                    self.ws();
                    if self.peek() == b'}' {
                        self.i += 1;
                        break;
                    }
                    let k = self.string()?;
                    self.eat(b':')?;
                    let v = self.val()?;
                    o.push((k, v));
                    self.ws();
                    if self.peek() == b',' {
                        self.i += 1;
                    }
                }
                Ok(J::Obj(o))
            }
            b'[' => {
                self.i += 1;
                let mut a = Vec::new();
                loop {
                    self.ws();
                    if self.peek() == b']' {
                        self.i += 1;
                        break;
                    }
                    a.push(self.val()?);
                    self.ws();
                    if self.peek() == b',' {
                        self.i += 1;
                    }
                }
                Ok(J::Arr(a))
            }
            b'"' => Ok(J::Str(self.string()?)),
            b't' => self.lit("true", J::Bool(true)),
            b'f' => self.lit("false", J::Bool(false)),
            b'n' => self.lit("null", J::Null),
            _ => {
                let st = self.i;
                while matches!(self.peek(), b'+' | b'-' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                    self.i += 1;
                }
                let t = std::str::from_utf8(&self.s[st..self.i]).map_err(|e| e.to_string())?;
                t.parse::<f64>().map(J::Num).map_err(|_| format!("json: bad value at byte {st}"))
            }
        }
    }
    fn lit(&mut self, word: &str, v: J) -> Result<J, String> {
        if self.s[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(v)
        } else {
            Err(format!("json: bad literal at byte {}", self.i))
        }
    }
}

// ------------------------------------------------------------------------------------------------ random + noise

fn stafford13(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

/// `XoroshiroRandomSource`.
struct Xoro {
    lo: u64,
    hi: u64,
}

impl Xoro {
    fn new(lo: u64, hi: u64) -> Self {
        if lo | hi == 0 {
            return Xoro { lo: 0x9E3779B97F4A7C15, hi: 0x6A09E667F3BCC909 };
        }
        Xoro { lo, hi }
    }
    /// `new XoroshiroRandomSource(seed)`: `RandomSupport.upgradeSeedTo128bit`.
    // UNVERIFIED: the two constants and the Stafford mix are from memory of `RandomSupport`; no vanilla source was at hand.
    fn from_seed(seed: i64) -> Self {
        let m = (seed as u64) ^ 0x6A09E667F3BCC909;
        Xoro::new(stafford13(m), stafford13(m.wrapping_add(0x9E3779B97F4A7C15)))
    }
    /// `PositionalRandomFactory.fromHashOf` with the name's MD5 already split in two (see `tools/tectonic.py`).
    // ponytail: the hashes are computed by the bundler, so a noise name or octave count the bundle does not list cannot be seeded;
    // upgrade is an MD5 here (~50 lines) when someone needs to add noises without re-running the tool.
    // UNVERIFIED: that the game hashes `minecraft:continentalness`-style names (and `octave_-10`) with MD5, big-endian halves.
    fn from_hash(fac: (u64, u64), h: (u64, u64)) -> Self {
        Xoro::new(h.0 ^ fac.0, h.1 ^ fac.1)
    }
    fn next_long(&mut self) -> u64 {
        let (l, m0) = (self.lo, self.hi);
        let n = l.wrapping_add(m0).rotate_left(17).wrapping_add(l);
        let m = m0 ^ l;
        self.lo = l.rotate_left(49) ^ m ^ (m << 21);
        self.hi = m.rotate_left(28);
        n
    }
    fn next_int(&mut self, bound: u32) -> u32 {
        let mut m = (self.next_long() as u32 as u64) * bound as u64;
        let mut n = m as u32;
        if n < bound {
            let j = bound.wrapping_neg() % bound;
            while n < j {
                m = (self.next_long() as u32 as u64) * bound as u64;
                n = m as u32;
            }
        }
        (m >> 32) as u32
    }
    fn next_double(&mut self) -> f64 {
        (self.next_long() >> 11) as f64 * 1.1102230246251565e-16
    }
    /// `forkPositional`.
    fn fork(&mut self) -> (u64, u64) {
        let a = self.next_long();
        let b = self.next_long();
        (a, b)
    }
}

const GRAD: [[f64; 3]; 16] = [
    [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0], [1.0, -1.0, 0.0], [-1.0, -1.0, 0.0],
    [1.0, 0.0, 1.0], [-1.0, 0.0, 1.0], [1.0, 0.0, -1.0], [-1.0, 0.0, -1.0],
    [0.0, 1.0, 1.0], [0.0, -1.0, 1.0], [0.0, 1.0, -1.0], [0.0, -1.0, -1.0],
    [1.0, 1.0, 0.0], [0.0, -1.0, 1.0], [-1.0, 1.0, 0.0], [0.0, -1.0, -1.0],
];

fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

fn smooth(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn wrap(v: f64) -> f64 {
    v - (v / 33554432.0 + 0.5).floor() * 33554432.0
}

/// `ImprovedNoise`.
struct Improved {
    xo: f64,
    yo: f64,
    zo: f64,
    p: [u8; 256],
}

impl Improved {
    fn new(r: &mut Xoro) -> Self {
        let xo = r.next_double() * 256.0;
        let yo = r.next_double() * 256.0;
        let zo = r.next_double() * 256.0;
        let mut p = [0u8; 256];
        for (i, v) in p.iter_mut().enumerate() {
            *v = i as u8;
        }
        for j in 0..256usize {
            let k = r.next_int(256 - j as u32) as usize;
            p.swap(j, j + k);
        }
        Improved { xo, yo, zo, p }
    }

    fn noise(&self, x: f64, y: f64, z: f64, ys: f64, ym: f64) -> f64 {
        let (d, e, f) = (x + self.xo, y + self.yo, z + self.zo);
        let (fi, fj, fk) = (d.floor(), e.floor(), f.floor());
        let (g, h, l) = (d - fi, e - fj, f - fk);
        let o = if ys != 0.0 {
            let m = if ym >= 0.0 && ym < h { ym } else { h };
            (m / ys + 1.0e-7).floor() * ys
        } else {
            0.0
        };
        let (i, j, k) = (fi as i64, fj as i64, fk as i64);
        let p = |n: i64| self.p[(n & 255) as usize] as i64;
        let a = p(i);
        let b = p(i + 1);
        let (k0, k1, m0, m1) = (p(a + j), p(a + j + 1), p(b + j), p(b + j + 1));
        let (dx, dy, dz) = (g, h - o, l);
        let gd = |hh: i64, x: f64, y: f64, z: f64| {
            let q = &GRAD[(p(hh) & 15) as usize];
            q[0] * x + q[1] * y + q[2] * z
        };
        let c000 = gd(k0 + k, dx, dy, dz);
        let c100 = gd(m0 + k, dx - 1.0, dy, dz);
        let c010 = gd(k1 + k, dx, dy - 1.0, dz);
        let c110 = gd(m1 + k, dx - 1.0, dy - 1.0, dz);
        let c001 = gd(k0 + k + 1, dx, dy, dz - 1.0);
        let c101 = gd(m0 + k + 1, dx - 1.0, dy, dz - 1.0);
        let c011 = gd(k1 + k + 1, dx, dy - 1.0, dz - 1.0);
        let c111 = gd(m1 + k + 1, dx - 1.0, dy - 1.0, dz - 1.0);
        let (sx, sy, sz) = (smooth(dx), smooth(h), smooth(dz));
        lerp(
            sz,
            lerp(sy, lerp(sx, c000, c100), lerp(sx, c010, c110)),
            lerp(sy, lerp(sx, c001, c101), lerp(sx, c011, c111)),
        )
    }
}

type Octaves = HashMap<i32, (u64, u64)>;

/// `PerlinNoise` with the 1.18+ initialisation: one hashed random per octave.
struct Perlin {
    amps: Vec<f64>,
    lv: Vec<Option<Improved>>,
    fin: f64,
    fvf: f64,
}

impl Perlin {
    fn new(first: i32, amps: &[f64], fac: (u64, u64), octs: &Octaves) -> Result<Self, String> {
        let mut lv = Vec::with_capacity(amps.len());
        for (i, &a) in amps.iter().enumerate() {
            lv.push(if a != 0.0 {
                let h = octs.get(&(first + i as i32)).ok_or_else(|| format!("no hash for octave_{}", first + i as i32))?;
                Some(Improved::new(&mut Xoro::from_hash(fac, *h)))
            } else {
                None
            });
        }
        let n = amps.len() as i32;
        Ok(Perlin { amps: amps.to_vec(), lv, fin: 2f64.powi(first), fvf: 2f64.powi(n - 1) / (2f64.powi(n) - 1.0) })
    }

    fn value(&self, x: f64, y: f64, z: f64) -> f64 {
        let (mut d, mut e, mut f) = (0.0, self.fin, self.fvf);
        for (a, nz) in self.amps.iter().zip(&self.lv) {
            if let Some(nz) = nz {
                d += a * nz.noise(wrap(x * e), wrap(y * e), wrap(z * e), 0.0, 0.0) * f;
            }
            e *= 2.0;
            f /= 2.0;
        }
        d
    }
}

/// `NormalNoise`: two Perlins, the second sampled at 1.0181268882175227 x the coordinates.
struct Normal {
    a: Perlin,
    b: Perlin,
    vf: f64,
}

impl Normal {
    fn new(r: &mut Xoro, first: i32, amps: &[f64], octs: &Octaves) -> Result<Self, String> {
        let fa = r.fork();
        let a = Perlin::new(first, amps, fa, octs)?;
        let fb = r.fork();
        let b = Perlin::new(first, amps, fb, octs)?;
        let nz: Vec<usize> = (0..amps.len()).filter(|&i| amps[i] != 0.0).collect();
        let span = match (nz.first(), nz.last()) {
            (Some(f), Some(l)) => (l - f) as f64,
            _ => 0.0,
        };
        // NormalNoise.expectedDeviation(octaves) = 0.1 * (1 + 1 / (octaves + 1)), octaves = last - first non-zero amplitude.
        // UNVERIFIED: whether the game passes `last - first` or `last - first + 1` there; the noise amplitude would differ visibly.
        Ok(Normal { a, b, vf: (1.0 / 6.0) / (0.1 * (1.0 + 1.0 / (span + 1.0))) })
    }
    fn value(&self, x: f64, y: f64, z: f64) -> f64 {
        const K: f64 = 1.0181268882175227;
        (self.a.value(x, y, z) + self.b.value(x * K, y * K, z * K)) * self.vf
    }
}

/// `BlendedNoise(0.25, 0.125, 80, 160, 8)`: the old b1.7.3 terrain noise, `minecraft:overworld/base_3d_noise`.
struct Blended {
    lo: Vec<Improved>,
    hi: Vec<Improved>,
    main: Vec<Improved>,
}

impl Blended {
    fn new(r: &mut Xoro) -> Self {
        // PerlinNoise.createLegacyForBlendedNoise: octave 0 is made first, then -1, -2, ...; index n - 1 is octave 0.
        // UNVERIFIED: that order, and the order lo, hi, main, from memory of `BlendedNoise`.
        let mut legacy = |n: usize| {
            let mut v: Vec<Improved> = (0..n).map(|_| Improved::new(r)).collect();
            v.reverse();
            v
        };
        let lo = legacy(16);
        let hi = legacy(16);
        let main = legacy(8);
        Blended { lo, hi, main }
    }

    fn value(&self, x: f64, y: f64, z: f64) -> f64 {
        let (xm, ym) = (684.412 * 0.25, 684.412 * 0.125);
        let (d0, d1, d2) = (x * xm, y * ym, z * xm);
        let (d3, d4, d5) = (d0 / 80.0, d1 / 160.0, d2 / 80.0);
        let d6 = ym * 8.0;
        let d7 = d6 / 160.0;
        let (mut d8, mut d9) = (0.0, 1.0);
        for i in 0..8 {
            d8 += self.main[7 - i].noise(wrap(d3 * d9), wrap(d4 * d9), wrap(d5 * d9), d7 * d9, d4 * d9) / d9;
            d9 /= 2.0;
        }
        let d10 = (d8 / 10.0 + 1.0) / 2.0;
        let (f1, f2) = (d10 >= 1.0, d10 <= 0.0);
        let (mut d9, mut d11, mut d12) = (1.0, 0.0, 0.0);
        for i in 0..16 {
            let (x2, y2, z2) = (wrap(d0 * d9), wrap(d1 * d9), wrap(d2 * d9));
            if !f1 {
                d11 += self.lo[15 - i].noise(x2, y2, z2, d6 * d9, d1 * d9) / d9;
            }
            if !f2 {
                d12 += self.hi[15 - i].noise(x2, y2, z2, d6 * d9, d1 * d9) / d9;
            }
            d9 /= 2.0;
        }
        let (a, b, t) = (d11 / 512.0, d12 / 512.0, d10);
        (if t < 0.0 { a } else if t > 1.0 { b } else { lerp(t, a, b) }) / 128.0
    }
}

// ------------------------------------------------------------------------------------------------ density functions

#[derive(Clone, Copy)]
enum Node {
    Const(f64),
    Y,
    ShiftX,
    ShiftZ,
    Base3d,
    Add(u32, u32),
    Mul(u32, u32),
    Min(u32, u32),
    Max(u32, u32),
    /// `flat_cache`, `cache_2d`, `cache_once`, `cache_all_in_cell`, `blend_density`.
    Pass(u32),
    /// Index into `Tectonic::interps`.
    Interp(usize),
    Abs(u32),
    Square(u32),
    Cube(u32),
    HalfNeg(u32),
    QuarterNeg(u32),
    Squeeze(u32),
    Clamp(u32, f64, f64),
    ClampFn(u32, u32, u32),
    Invert(u32),
    /// from_y, to_y, from_value, to_value.
    Gradient(f64, f64, f64, f64),
    /// input, min_inclusive, max_exclusive, when_in_range, when_out_of_range.
    Range(u32, f64, f64, u32, u32),
    /// noise, xz_scale, y_scale.
    Noise(usize, f64, f64),
    /// noise, xz_scale, y_scale, shift_x, shift_y, shift_z.
    Shifted(usize, f64, f64, u32, u32, u32),
    /// `tectonic:config_noise`: noise, scale, shift_x, shift_z.
    CfgNoise(usize, f64, u32, u32),
    Spline(usize),
}

#[derive(Clone, Copy)]
enum SplineVal {
    Const(f32),
    Sub(usize),
}

#[derive(Clone, Copy)]
struct Point {
    loc: f32,
    der: f32,
    val: SplineVal,
}

#[derive(Clone)]
struct Spline {
    coord: u32,
    pts: Vec<Point>,
}

/// Evaluation memo, one slot per node (no hashing). A node that does not depend on y is valid for the current column (`column`), any
/// other node for the current point (`point`): the value is read back while the slot's stamp equals it. Every node is in exactly one
/// of the two classes, so the two counters (one `tick`) never meet on a slot. Nothing is cleared between columns or points.
struct Memo {
    val: Vec<f64>,
    stamp: Vec<u32>,
    tick: u32,
    col: u32,
    pt: u32,
}

impl Memo {
    fn new(nodes: usize) -> Self {
        // Stamps start at 0 and the counters at 1, so a fresh slot never matches.
        Memo { val: vec![0.0; nodes], stamp: vec![0; nodes], tick: 1, col: 1, pt: 1 }
    }

    /// A new (x, z) column: the y-independent values of the last one are stale.
    fn column(&mut self) {
        self.tick += 1;
        self.col = self.tick;
    }

    /// A new point: the y-dependent values of the last one are stale. Call it for every y, and after `column`.
    fn point(&mut self) {
        self.tick += 1;
        self.pt = self.tick;
    }
}

// The arithmetic of the nodes that `simplify` also folds: one copy, so a folded constant is the very same f64 operations.
fn half_neg(v: f64) -> f64 {
    if v < 0.0 { v * 0.5 } else { v }
}
fn quarter_neg(v: f64) -> f64 {
    if v < 0.0 { v * 0.25 } else { v }
}
fn squeeze(v: f64) -> f64 {
    let c = v.clamp(-1.0, 1.0);
    c / 2.0 - c * c * c / 24.0
}
fn square(v: f64) -> f64 {
    v * v
}
fn cube(v: f64) -> f64 {
    v * v * v
}
fn invert(v: f64) -> f64 {
    1.0 / v
}

/// The corner values of every `interpolated` node of the chunk being filled.
struct Grid {
    on: bool,
    x0: i32,
    z0: i32,
    min_y: i32,
    ny: usize,
    tabs: Vec<Vec<f64>>,
}

impl Grid {
    /// Trilinear interpolation of table `t` at a block (4 x 8 x 4 cells, corners at x0 + 4i, min_y + 8j, z0 + 4k).
    fn lookup(&self, t: usize, x: i32, y: i32, z: i32) -> f64 {
        let (fx, fz, fy) = ((x - self.x0) as f64 / 4.0, (z - self.z0) as f64 / 4.0, (y - self.min_y) as f64 / 8.0);
        let (ix, iz, iy) = ((fx as usize).min(3), (fz as usize).min(3), (fy as usize).min(self.ny - 2));
        let (tx, tz, ty) = (fx - ix as f64, fz - iz as f64, fy - iy as f64);
        let tab = &self.tabs[t];
        let c = |a: usize, b: usize, d: usize| tab[((ix + a) * 5 + iz + b) * self.ny + iy + d];
        lerp(
            tx,
            lerp(tz, lerp(ty, c(0, 0, 0), c(0, 0, 1)), lerp(ty, c(0, 1, 0), c(0, 1, 1))),
            lerp(tz, lerp(ty, c(1, 0, 0), c(1, 0, 1)), lerp(ty, c(1, 1, 0), c(1, 1, 1))),
        )
    }
}

/// Turns the JSON of the density functions into `Node`s. Named functions are compiled once and shared.
struct Builder<'a> {
    f: &'a J,
    stubs: &'a J,
    cfg: &'a Config,
    noise_ix: &'a HashMap<String, usize>,
    nodes: Vec<Node>,
    /// Does the node depend on y (not memoised per column)?
    dep: Vec<bool>,
    ids: HashMap<String, u32>,
    splines: Vec<Spline>,
    interps: Vec<u32>,
}

impl Builder<'_> {
    fn push(&mut self, n: Node, dep: bool) -> u32 {
        self.nodes.push(n);
        self.dep.push(dep);
        (self.nodes.len() - 1) as u32
    }

    fn compile(&mut self, j: &J) -> Result<u32, String> {
        match j {
            J::Num(n) => Ok(self.push(Node::Const(*n), false)),
            J::Str(id) => self.named(id),
            J::Obj(_) => self.obj(j),
            _ => Err("density function: expected a number, a name or an object".into()),
        }
    }

    fn named(&mut self, id: &str) -> Result<u32, String> {
        if let Some(&i) = self.ids.get(id) {
            return Ok(i);
        }
        let r = match id {
            "minecraft:y" => self.push(Node::Y, true),
            "minecraft:shift_x" => self.push(Node::ShiftX, false),
            "minecraft:shift_z" => self.push(Node::ShiftZ, false),
            "minecraft:overworld/base_3d_noise" => self.push(Node::Base3d, true),
            _ => {
                if let Some(v) = self.stubs.get(id).and_then(|v| v.num()) {
                    self.push(Node::Const(v), false)
                } else {
                    let f = self.f;
                    let j = f.get(id).ok_or_else(|| format!("unknown density function {id}"))?;
                    self.compile(j)?
                }
            }
        };
        self.ids.insert(id.to_string(), r);
        Ok(r)
    }

    fn a(&mut self, j: &J, k: &str) -> Result<u32, String> {
        let v = j.get(k).ok_or_else(|| format!("density function: missing field {k}"))?;
        self.compile(v)
    }

    fn n(j: &J, k: &str) -> Result<f64, String> {
        j.get(k).and_then(|v| v.num()).ok_or_else(|| format!("density function: missing number {k}"))
    }

    fn noise(&self, j: &J) -> Result<usize, String> {
        let name = j.get("noise").and_then(|v| v.str()).ok_or("density function: missing noise")?;
        self.noise_ix.get(name).copied().ok_or_else(|| format!("unknown noise {name}"))
    }

    fn obj(&mut self, j: &J) -> Result<u32, String> {
        let t = j.get("type").and_then(|t| t.str()).ok_or("density function without a type")?;
        let t = t.strip_prefix("minecraft:").unwrap_or(t);
        Ok(match t {
            "add" | "mul" | "min" | "max" => {
                let a = self.a(j, "argument1")?;
                let b = self.a(j, "argument2")?;
                let dep = self.dep[a as usize] || self.dep[b as usize];
                let n = match t {
                    "add" => Node::Add(a, b),
                    "mul" => Node::Mul(a, b),
                    "min" => Node::Min(a, b),
                    _ => Node::Max(a, b),
                };
                self.push(n, dep)
            }
            "flat_cache" | "cache_2d" | "cache_once" | "cache_all_in_cell" | "blend_density" => {
                let a = self.a(j, "argument")?;
                let dep = self.dep[a as usize];
                self.push(Node::Pass(a), dep)
            }
            "interpolated" => {
                let a = self.a(j, "argument")?;
                self.interps.push(a);
                let i = self.interps.len() - 1;
                self.push(Node::Interp(i), true)
            }
            "abs" | "square" | "cube" | "half_negative" | "quarter_negative" | "squeeze" | "tectonic:invert" => {
                let a = self.a(j, "argument")?;
                let dep = self.dep[a as usize];
                let n = match t {
                    "abs" => Node::Abs(a),
                    "square" => Node::Square(a),
                    "cube" => Node::Cube(a),
                    "half_negative" => Node::HalfNeg(a),
                    "quarter_negative" => Node::QuarterNeg(a),
                    "squeeze" => Node::Squeeze(a),
                    _ => Node::Invert(a),
                };
                self.push(n, dep)
            }
            "clamp" => {
                let a = self.a(j, "input")?;
                let (lo, hi) = (Self::n(j, "min")?, Self::n(j, "max")?);
                let dep = self.dep[a as usize];
                self.push(Node::Clamp(a, lo, hi), dep)
            }
            "tectonic:config_clamp" => {
                let (a, lo, hi) = (self.a(j, "input")?, self.a(j, "min")?, self.a(j, "max")?);
                let dep = self.dep[a as usize] || self.dep[lo as usize] || self.dep[hi as usize];
                self.push(Node::ClampFn(a, lo, hi), dep)
            }
            "blend_alpha" => self.push(Node::Const(1.0), false), // no old chunks to blend with
            "blend_offset" => self.push(Node::Const(0.0), false),
            "y_clamped_gradient" => {
                let n = Node::Gradient(Self::n(j, "from_y")?, Self::n(j, "to_y")?, Self::n(j, "from_value")?, Self::n(j, "to_value")?);
                self.push(n, true)
            }
            "range_choice" => {
                let a = self.a(j, "input")?;
                let (lo, hi) = (Self::n(j, "min_inclusive")?, Self::n(j, "max_exclusive")?);
                let (i, o) = (self.a(j, "when_in_range")?, self.a(j, "when_out_of_range")?);
                let dep = self.dep[a as usize] || self.dep[i as usize] || self.dep[o as usize];
                self.push(Node::Range(a, lo, hi, i, o), dep)
            }
            "noise" => {
                let (nz, xs, ys) = (self.noise(j)?, Self::n(j, "xz_scale")?, Self::n(j, "y_scale")?);
                self.push(Node::Noise(nz, xs, ys), ys != 0.0)
            }
            "shifted_noise" => {
                let nz = self.noise(j)?;
                let (xs, ys) = (Self::n(j, "xz_scale")?, Self::n(j, "y_scale")?);
                let (sx, sy, sz) = (self.a(j, "shift_x")?, self.a(j, "shift_y")?, self.a(j, "shift_z")?);
                let dep = ys != 0.0 || self.dep[sx as usize] || self.dep[sy as usize] || self.dep[sz as usize];
                self.push(Node::Shifted(nz, xs, ys, sx, sy, sz), dep)
            }
            "spline" => match j.get("spline") {
                Some(J::Num(v)) => self.push(Node::Const(*v as f32 as f64), false),
                Some(s) => {
                    let (i, dep) = self.spline(s)?;
                    self.push(Node::Spline(i), dep)
                }
                None => return Err("spline node without a spline".into()),
            },
            "tectonic:config_constant" => {
                let key = j.get("key").and_then(|k| k.str()).ok_or("config_constant without a key")?;
                let v = self.cfg.constant(key)?;
                self.push(Node::Const(v), false)
            }
            "tectonic:config_noise" => {
                let key = j.get("key").and_then(|k| k.str()).ok_or("config_noise without a key")?;
                let scale = self.cfg.scale(key)?;
                let nz = self.noise(j)?;
                let (sx, sz) = (self.a(j, "shift_x")?, self.a(j, "shift_z")?);
                let dep = self.dep[sx as usize] || self.dep[sz as usize];
                self.push(Node::CfgNoise(nz, scale, sx, sz), dep)
            }
            other => return Err(format!("unsupported density function type {other}")),
        })
    }

    /// One `CubicSpline.Multipoint`; returns its index and whether it depends on y.
    fn spline(&mut self, j: &J) -> Result<(usize, bool), String> {
        let coord = self.a(j, "coordinate")?;
        let mut dep = self.dep[coord as usize];
        let mut pts = Vec::new();
        let pj: &[J] = j.get("points").map_or(&[][..], |p| p.arr());
        for p in pj {
            let (loc, der) = (Self::n(p, "location")? as f32, Self::n(p, "derivative")? as f32);
            let val = match p.get("value") {
                Some(J::Num(v)) => SplineVal::Const(*v as f32),
                Some(v @ J::Obj(_)) => {
                    let (i, d) = self.spline(v)?;
                    dep |= d;
                    SplineVal::Sub(i)
                }
                _ => return Err("spline point without a value".into()),
            };
            pts.push(Point { loc, der, val });
        }
        self.splines.push(Spline { coord, pts });
        Ok((self.splines.len() - 1, dep))
    }
}

/// The value of a node that is a constant.
fn const_of(nodes: &[Node], k: u32) -> Option<f64> {
    match nodes[k as usize] {
        Node::Const(v) => Some(v),
        _ => None,
    }
}

/// A unary node over operand `a` (already in the new list): a constant if `a` is one, else the node itself.
fn fold1(nodes: &[Node], mk: fn(u32) -> Node, a: u32, f: fn(f64) -> f64) -> Node {
    match const_of(nodes, a) {
        Some(v) => Node::Const(f(v)),
        None => mk(a),
    }
}

/// A binary node: a constant if both operands are; else the node, operands in id order when `swap` is allowed (so `a + b` and
/// `b + a` are one node).
fn fold2(nodes: &[Node], mk: fn(u32, u32) -> Node, a: u32, b: u32, swap: bool, f: fn(f64, f64) -> f64) -> Node {
    if let (Some(p), Some(q)) = (const_of(nodes, a), const_of(nodes, b)) {
        return Node::Const(f(p, q));
    }
    if swap && b < a { mk(b, a) } else { mk(a, b) }
}

/// What makes two nodes the same node: the variant and every field (floats by their bits).
fn key(n: &Node) -> [u64; 7] {
    let f = |v: f64| v.to_bits();
    match *n {
        Node::Const(v) => [0, f(v), 0, 0, 0, 0, 0],
        Node::Y => [1, 0, 0, 0, 0, 0, 0],
        Node::ShiftX => [2, 0, 0, 0, 0, 0, 0],
        Node::ShiftZ => [3, 0, 0, 0, 0, 0, 0],
        Node::Base3d => [4, 0, 0, 0, 0, 0, 0],
        Node::Add(a, b) => [5, a as u64, b as u64, 0, 0, 0, 0],
        Node::Mul(a, b) => [6, a as u64, b as u64, 0, 0, 0, 0],
        Node::Min(a, b) => [7, a as u64, b as u64, 0, 0, 0, 0],
        Node::Max(a, b) => [8, a as u64, b as u64, 0, 0, 0, 0],
        Node::Pass(a) => [9, a as u64, 0, 0, 0, 0, 0],
        Node::Interp(t) => [10, t as u64, 0, 0, 0, 0, 0],
        Node::Abs(a) => [11, a as u64, 0, 0, 0, 0, 0],
        Node::Square(a) => [12, a as u64, 0, 0, 0, 0, 0],
        Node::Cube(a) => [13, a as u64, 0, 0, 0, 0, 0],
        Node::HalfNeg(a) => [14, a as u64, 0, 0, 0, 0, 0],
        Node::QuarterNeg(a) => [15, a as u64, 0, 0, 0, 0, 0],
        Node::Squeeze(a) => [16, a as u64, 0, 0, 0, 0, 0],
        Node::Clamp(a, lo, hi) => [17, a as u64, f(lo), f(hi), 0, 0, 0],
        Node::ClampFn(a, lo, hi) => [18, a as u64, lo as u64, hi as u64, 0, 0, 0],
        Node::Invert(a) => [19, a as u64, 0, 0, 0, 0, 0],
        Node::Gradient(a, b, c, d) => [20, f(a), f(b), f(c), f(d), 0, 0],
        Node::Range(a, lo, hi, x, y) => [21, a as u64, f(lo), f(hi), x as u64, y as u64, 0],
        Node::Noise(nz, xs, ys) => [22, nz as u64, f(xs), f(ys), 0, 0, 0],
        Node::Shifted(nz, xs, ys, sx, sy, sz) => [23, nz as u64, f(xs), f(ys), sx as u64, sy as u64, sz as u64],
        Node::CfgNoise(nz, scale, sx, sz) => [24, nz as u64, f(scale), sx as u64, sz as u64, 0, 0],
        // A spline is never merged with another: the index is its identity.
        Node::Spline(s) => [25, s as u64, 0, 0, 0, 0, 0],
    }
}

// ------------------------------------------------------------------------------------------------ the terrain

pub struct Tectonic {
    nodes: Vec<Node>,
    dep: Vec<bool>,
    splines: Vec<Spline>,
    /// The argument node of every `interpolated` node.
    interps: Vec<u32>,
    ids: HashMap<String, u32>,
    root: u32,
    noises: Vec<Normal>,
    noise_ix: HashMap<String, usize>,
    /// `minecraft:offset`, behind `shift_x` / `shift_z`.
    offset: usize,
    blended: Blended,
    min_y: i32,
    height: usize,
}

fn hash(j: Option<&J>) -> Result<(u64, u64), String> {
    let a = j.map_or(&[][..], |j| j.arr());
    let p = |i: usize| a.get(i).and_then(|v| v.str()).and_then(|s| u64::from_str_radix(s, 16).ok()).ok_or("terrain.json: bad hash");
    Ok((p(0)?, p(1)?))
}

impl Tectonic {
    /// The terrain of `seed`. Panics only if the embedded `terrain.json` is broken (the tests build it).
    pub fn new(seed: i64, cfg: Config) -> Self {
        Self::from_bundle(BUNDLE, seed, cfg).expect("terrain.json")
    }

    pub fn from_bundle(json: &str, seed: i64, cfg: Config) -> Result<Self, String> {
        Self::build(json, seed, cfg, true)
    }

    /// `simplify`: run the graph optimiser (always, outside the tests that compare it with the plain graph).
    fn build(json: &str, seed: i64, cfg: Config, simplify: bool) -> Result<Self, String> {
        assert!(cfg.max_y > cfg.min_y && (cfg.max_y - cfg.min_y) % 8 == 0, "height must be a multiple of 8");
        let j = Parser { s: json.as_bytes(), i: 0 }.val()?;
        let field = |k: &str| j.get(k).ok_or_else(|| format!("terrain.json: no \"{k}\""));
        let mut octs = Octaves::new();
        if let J::Obj(o) = field("oct")? {
            for (k, v) in o {
                octs.insert(k.parse::<i32>().map_err(|e| e.to_string())?, hash(Some(v))?);
            }
        }
        // RandomState: every named noise is seeded from the world seed's positional factory and the noise's name.
        let root_fac = Xoro::from_seed(seed).fork();
        let (mut noises, mut noise_ix) = (Vec::new(), HashMap::new());
        if let J::Obj(o) = field("n")? {
            for (name, v) in o {
                let first = v.get("o").and_then(|n| n.num()).ok_or("terrain.json: noise without o")? as i32;
                let amps: Vec<f64> = v.get("a").map_or(&[][..], |a| a.arr()).iter().filter_map(|n| n.num()).collect();
                let mut r = Xoro::from_hash(root_fac, hash(v.get("h"))?);
                noise_ix.insert(name.clone(), noises.len());
                noises.push(Normal::new(&mut r, first, &amps, &octs)?);
            }
        }
        let offset = *noise_ix.get("minecraft:offset").ok_or("terrain.json: no minecraft:offset")?;
        let blended = Blended::new(&mut Xoro::from_hash(root_fac, hash(j.get("terrain"))?));
        let mut b = Builder {
            f: field("f")?,
            stubs: field("stubs")?,
            cfg: &cfg,
            noise_ix: &noise_ix,
            nodes: Vec::new(),
            dep: Vec::new(),
            ids: HashMap::new(),
            splines: Vec::new(),
            interps: Vec::new(),
        };
        let root_name = field("root")?.str().ok_or("terrain.json: root is not a name")?;
        let root = b.named(root_name)?;
        let Builder { nodes, dep, ids, splines, interps, .. } = b;
        let mut t = Tectonic { nodes, dep, splines, interps, ids, root, noises, noise_ix, offset, blended, min_y: cfg.min_y, height: (cfg.max_y - cfg.min_y) as usize };
        if simplify {
            t.simplify();
        }
        Ok(t)
    }

    /// Rewrites the node graph once, after it is built (the ideas of C2ME's density function compiler, `c2me-opts-dfc`, MIT: tree
    /// normalisation, constant folding, branch elimination; here also the identity wrappers and common subexpressions go). Every
    /// rewrite keeps the value of every node to the bit: a fold runs the same f64 operation the evaluator would, a `Range` is cut
    /// only for a constant input or equal arms, an operand swap only for `+` and `*` (commutative to the bit), never for `min` /
    /// `max` (the sign of a zero). Nodes are only ever added after their operands, so one forward pass sees every operand mapped.
    /// Dead nodes stay in the list (a few slots in the memo; `function` may still ask for a named node that nothing uses any more).
    fn simplify(&mut self) {
        let old = std::mem::take(&mut self.nodes);
        let old_dep = std::mem::take(&mut self.dep);
        let old_interps = std::mem::take(&mut self.interps);
        let mut nodes: Vec<Node> = Vec::with_capacity(old.len());
        let mut dep: Vec<bool> = Vec::with_capacity(old.len());
        let mut interps: Vec<u32> = Vec::new();
        let mut seen: HashMap<[u64; 7], u32> = HashMap::new();
        // Old node index -> new node index.
        let mut map: Vec<u32> = Vec::with_capacity(old.len());
        for (i, n) in old.iter().enumerate() {
            let m = |k: u32| map[k as usize];
            let r = match *n {
                Node::Const(_) | Node::Y | Node::ShiftX | Node::ShiftZ | Node::Base3d | Node::Gradient(..) | Node::Noise(..) => *n,
                // The cache wrappers are the identity (see the module doc): refer to the argument itself.
                Node::Pass(a) => {
                    let k = m(a);
                    map.push(k);
                    continue;
                }
                Node::Interp(t) => {
                    let arg = m(old_interps[t]);
                    let found = interps.iter().position(|&a| a == arg);
                    let pos = match found {
                        Some(p) => p,
                        None => {
                            interps.push(arg);
                            interps.len() - 1
                        }
                    };
                    Node::Interp(pos)
                }
                Node::Add(a, b) => fold2(&nodes, Node::Add, m(a), m(b), true, |p, q| p + q),
                Node::Mul(a, b) => fold2(&nodes, Node::Mul, m(a), m(b), true, |p, q| p * q),
                Node::Min(a, b) => fold2(&nodes, Node::Min, m(a), m(b), false, f64::min),
                Node::Max(a, b) => fold2(&nodes, Node::Max, m(a), m(b), false, f64::max),
                Node::Abs(a) => fold1(&nodes, Node::Abs, m(a), f64::abs),
                Node::Square(a) => fold1(&nodes, Node::Square, m(a), square),
                Node::Cube(a) => fold1(&nodes, Node::Cube, m(a), cube),
                Node::HalfNeg(a) => fold1(&nodes, Node::HalfNeg, m(a), half_neg),
                Node::QuarterNeg(a) => fold1(&nodes, Node::QuarterNeg, m(a), quarter_neg),
                Node::Squeeze(a) => fold1(&nodes, Node::Squeeze, m(a), squeeze),
                Node::Invert(a) => fold1(&nodes, Node::Invert, m(a), invert),
                Node::Clamp(a, lo, hi) => match const_of(&nodes, m(a)) {
                    Some(v) => Node::Const(v.max(lo).min(hi)),
                    None => Node::Clamp(m(a), lo, hi),
                },
                Node::ClampFn(a, lo, hi) => match (const_of(&nodes, m(a)), const_of(&nodes, m(lo)), const_of(&nodes, m(hi))) {
                    (Some(v), Some(lo), Some(hi)) => Node::Const(v.max(lo).min(hi)),
                    _ => Node::ClampFn(m(a), m(lo), m(hi)),
                },
                Node::Range(a, lo, hi, inside, outside) => {
                    let (a, inside, outside) = (m(a), m(inside), m(outside));
                    // A constant input picks its arm now; equal arms need no input at all.
                    let pick = match const_of(&nodes, a) {
                        Some(v) => Some(if v >= lo && v < hi { inside } else { outside }),
                        None if inside == outside => Some(inside),
                        None => None,
                    };
                    if let Some(k) = pick {
                        map.push(k);
                        continue;
                    }
                    Node::Range(a, lo, hi, inside, outside)
                }
                Node::Shifted(nz, xs, ys, sx, sy, sz) => Node::Shifted(nz, xs, ys, m(sx), m(sy), m(sz)),
                Node::CfgNoise(nz, scale, sx, sz) => Node::CfgNoise(nz, scale, m(sx), m(sz)),
                Node::Spline(s) => Node::Spline(s),
            };
            // y-dependence from the new operands (never lower than the truth); a spline keeps the builder's answer.
            let d = match r {
                Node::Const(_) | Node::ShiftX | Node::ShiftZ => false,
                Node::Y | Node::Base3d | Node::Gradient(..) | Node::Interp(_) => true,
                Node::Noise(_, _, ys) => ys != 0.0,
                Node::Spline(_) => old_dep[i],
                Node::Add(a, b) | Node::Mul(a, b) | Node::Min(a, b) | Node::Max(a, b) => dep[a as usize] || dep[b as usize],
                Node::Pass(a) | Node::Abs(a) | Node::Square(a) | Node::Cube(a) | Node::HalfNeg(a) | Node::QuarterNeg(a) | Node::Squeeze(a)
                | Node::Invert(a) | Node::Clamp(a, _, _) => dep[a as usize],
                Node::ClampFn(a, lo, hi) => dep[a as usize] || dep[lo as usize] || dep[hi as usize],
                Node::Range(a, _, _, x, y) => dep[a as usize] || dep[x as usize] || dep[y as usize],
                Node::Shifted(_, _, ys, sx, sy, sz) => ys != 0.0 || dep[sx as usize] || dep[sy as usize] || dep[sz as usize],
                Node::CfgNoise(_, _, sx, sz) => dep[sx as usize] || dep[sz as usize],
            };
            let k = key(&r);
            let id = match seen.get(&k) {
                Some(&id) => id,
                None => {
                    nodes.push(r);
                    dep.push(d);
                    seen.insert(k, (nodes.len() - 1) as u32);
                    (nodes.len() - 1) as u32
                }
            };
            map.push(id);
        }
        for sp in &mut self.splines {
            sp.coord = map[sp.coord as usize];
        }
        for v in self.ids.values_mut() {
            *v = map[*v as usize];
        }
        self.root = map[self.root as usize];
        self.nodes = nodes;
        self.dep = dep;
        self.interps = interps;
    }

    /// Number of blocks of a column.
    pub fn height(&self) -> usize {
        self.height
    }

    /// Final density of every block of chunk (cx, cz): solid where it is above 0. Index `(x * 16 + z) * height + y`, y counted
    /// from the bottom of the world (`min_y`). Sea, ice and the surface are the caller's business.
    pub fn chunk(&self, cx: i32, cz: i32) -> Vec<f32> {
        let h = self.height;
        let ny = h / 8 + 1;
        // ponytail: the memo lives for one chunk, so the border columns shared with the neighbours (5 x 5 corners for 4 x 4 cells,
        // ~56 % more 2D work than needed) are computed again by each; upgrade is a column cache across chunks like `OverworldGenerator::cols`.
        let mut m = Memo::new(self.nodes.len());
        let mut g = Grid { on: false, x0: cx * 16, z0: cz * 16, min_y: self.min_y, ny, tabs: Vec::new() };
        // Pass 1: each `interpolated` argument on the 5 x ny x 5 cell corners. Column by column, so the y-independent nodes of a
        // corner column are computed once for every table.
        let mut tabs = vec![vec![0.0; 25 * ny]; self.interps.len()];
        for gx in 0..5usize {
            for gz in 0..5usize {
                m.column();
                let (x, z) = (g.x0 + gx as i32 * 4, g.z0 + gz as i32 * 4);
                for (t, &arg) in self.interps.iter().enumerate() {
                    for gy in 0..ny {
                        m.point();
                        tabs[t][(gx * 5 + gz) * ny + gy] = self.ev(arg, x, self.min_y + gy as i32 * 8, z, &mut m, &g);
                    }
                }
            }
        }
        g.tabs = tabs;
        g.on = true;
        // Pass 2: the whole tree on every block, `interpolated` nodes reading their corners.
        let mut out = vec![0f32; 256 * h];
        for x in 0..16usize {
            for z in 0..16usize {
                m.column();
                for y in 0..h {
                    m.point();
                    let d = self.ev(self.root, g.x0 + x as i32, self.min_y + y as i32, g.z0 + z as i32, &mut m, &g);
                    out[(x * 16 + z) * h + y] = d as f32;
                }
            }
        }
        out
    }

    /// The top solid block (index from the bottom of the world, as in `chunk`) of column (x, z), or -1: the final density at the block
    /// itself, no cell interpolation, scanned down from the top in 8-block steps and then up the last step. For reading the land's
    /// height from far away (`mc-rs`'s rivers), not for placing blocks: an overhang above a gap is not seen.
    pub fn surface(&self, x: i32, z: i32) -> i32 {
        let g = Grid { on: false, x0: 0, z0: 0, min_y: self.min_y, ny: 2, tabs: Vec::new() };
        let mut m = Memo::new(self.nodes.len());
        m.column();
        let top = self.height as i32 - 1;
        let mut solid = |y: i32| {
            m.point();
            self.ev(self.root, x, self.min_y + y, z, &mut m, &g) > 0.0
        };
        let mut y = top;
        while y >= 0 && !solid(y) {
            y -= 8;
        }
        if y < 0 {
            return if solid(0) { 0 } else { -1 };
        }
        while y < top && solid(y + 1) {
            y += 1;
        }
        y
    }

    /// A named `NormalNoise` of the pack at a point (tests, tuning).
    pub fn noise(&self, name: &str, x: f64, y: f64, z: f64) -> Option<f64> {
        self.noise_ix.get(name).map(|&i| self.noises[i].value(x, y, z))
    }

    /// `minecraft:overworld/base_3d_noise` at a point.
    pub fn blended(&self, x: f64, y: f64, z: f64) -> f64 {
        self.blended.value(x, y, z)
    }

    /// A density function the terrain uses, by name, at a block (y in the datapack's frame); `None` if the terrain never uses it.
    pub fn function(&self, name: &str, x: i32, y: i32, z: i32) -> Option<f64> {
        let id = *self.ids.get(name)?;
        let g = Grid { on: false, x0: 0, z0: 0, min_y: self.min_y, ny: 2, tabs: Vec::new() };
        let mut m = Memo::new(self.nodes.len());
        m.column();
        m.point();
        Some(self.ev(id, x, y, z, &mut m, &g))
    }

    fn ev(&self, id: u32, x: i32, y: i32, z: i32, m: &mut Memo, g: &Grid) -> f64 {
        let i = id as usize;
        if let Node::Const(v) = self.nodes[i] {
            return v;
        }
        let now = if self.dep[i] { m.pt } else { m.col };
        if m.stamp[i] == now {
            return m.val[i];
        }
        let v = self.node(i, x, y, z, m, g);
        m.val[i] = v;
        m.stamp[i] = now;
        v
    }

    fn node(&self, i: usize, x: i32, y: i32, z: i32, m: &mut Memo, g: &Grid) -> f64 {
        macro_rules! e {
            ($id:expr) => {
                self.ev($id, x, y, z, m, g)
            };
        }
        let (xf, yf, zf) = (x as f64, y as f64, z as f64);
        match &self.nodes[i] {
            Node::Const(v) => *v,
            Node::Y => yf,
            // `shift_x` / `shift_z`: the `offset` noise at a quarter of the position, times 4.
            // UNVERIFIED: from memory of `DensityFunctions.ShiftA` / `ShiftB` (z, x swapped for ShiftB).
            Node::ShiftX => self.noises[self.offset].value(xf * 0.25, 0.0, zf * 0.25) * 4.0,
            Node::ShiftZ => self.noises[self.offset].value(zf * 0.25, xf * 0.25, 0.0) * 4.0,
            Node::Base3d => self.blended.value(xf, yf, zf),
            Node::Add(a, b) => e!(*a) + e!(*b),
            Node::Mul(a, b) => e!(*a) * e!(*b),
            Node::Min(a, b) => e!(*a).min(e!(*b)),
            Node::Max(a, b) => e!(*a).max(e!(*b)),
            Node::Pass(a) => e!(*a),
            Node::Interp(t) => {
                if g.on {
                    g.lookup(*t, x, y, z)
                } else {
                    e!(self.interps[*t])
                }
            }
            Node::Abs(a) => e!(*a).abs(),
            Node::Square(a) => square(e!(*a)),
            Node::Cube(a) => cube(e!(*a)),
            Node::HalfNeg(a) => half_neg(e!(*a)),
            Node::QuarterNeg(a) => quarter_neg(e!(*a)),
            Node::Squeeze(a) => squeeze(e!(*a)),
            Node::Clamp(a, lo, hi) => e!(*a).max(*lo).min(*hi),
            Node::ClampFn(a, lo, hi) => {
                let v = e!(*a);
                let (lo, hi) = (e!(*lo), e!(*hi));
                v.max(lo).min(hi)
            }
            Node::Invert(a) => invert(e!(*a)),
            Node::Gradient(fy, ty, fv, tv) => {
                let u = ((yf - fy) / (ty - fy)).clamp(0.0, 1.0);
                fv + u * (tv - fv)
            }
            Node::Range(a, lo, hi, inside, outside) => {
                let v = e!(*a);
                if v >= *lo && v < *hi { e!(*inside) } else { e!(*outside) }
            }
            Node::Noise(nz, xs, ys) => self.noises[*nz].value(xf * xs, yf * ys, zf * xs),
            Node::Shifted(nz, xs, ys, sx, sy, sz) => {
                let (dx, dy, dz) = (e!(*sx), e!(*sy), e!(*sz));
                self.noises[*nz].value(xf * xs + dx, yf * ys + dy, zf * xs + dz)
            }
            Node::CfgNoise(nz, scale, sx, sz) => {
                let (dx, dz) = (e!(*sx), e!(*sz));
                self.noises[*nz].value(xf * scale + dx, 0.0, zf * scale + dz)
            }
            Node::Spline(s) => self.spline_at(*s, x, y, z, m, g) as f64,
        }
    }

    fn spline_val(&self, v: &SplineVal, x: i32, y: i32, z: i32, m: &mut Memo, g: &Grid) -> f32 {
        match v {
            SplineVal::Const(c) => *c,
            SplineVal::Sub(s) => self.spline_at(*s, x, y, z, m, g),
        }
    }

    /// `CubicSpline.Multipoint.apply`, in `f32` like the game: Hermite between points, straight lines beyond the ends.
    fn spline_at(&self, s: usize, x: i32, y: i32, z: i32, m: &mut Memo, g: &Grid) -> f32 {
        let sp = &self.splines[s];
        let pts = &sp.pts;
        if pts.is_empty() {
            return 0.0;
        }
        let f = self.ev(sp.coord, x, y, z, m, g) as f32;
        let last = pts.len() - 1;
        // The greatest point whose location is <= f (none: before the first).
        let n = pts.partition_point(|p| p.loc <= f);
        if n == 0 || n == pts.len() {
            let k = if n == 0 { 0 } else { last };
            let v = self.spline_val(&pts[k].val, x, y, z, m, g);
            let d = pts[k].der;
            return if d == 0.0 { v } else { v + d * (f - pts[k].loc) };
        }
        let i = n - 1;
        let (l0, l1) = (pts[i].loc, pts[i + 1].loc);
        let t = (f - l0) / (l1 - l0);
        let y0 = self.spline_val(&pts[i].val, x, y, z, m, g);
        let y1 = self.spline_val(&pts[i + 1].val, x, y, z, m, g);
        let (d0, d1) = (pts[i].der, pts[i + 1].der);
        let a = d0 * (l1 - l0) - (y1 - y0);
        let b = -d1 * (l1 - l0) + (y1 - y0);
        (y0 + t * (y1 - y0)) + t * (1.0 - t) * (a + t * (b - a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, eps: f64, what: &str) {
        assert!((a - b).abs() <= eps, "{what}: {a} vs {b}");
    }

    fn top(c: &[f32], h: usize, x: usize, z: usize) -> i32 {
        (0..h).rev().find(|&y| c[(x * 16 + z) * h + y] > 0.0).map_or(-1, |y| y as i32)
    }

    #[test]
    fn json_reads_what_it_needs() {
        let j = Parser { s: br#"{"a":[1,-2.5e1,"x\"y\u0041"],"b":{"c":true,"d":null}}"#, i: 0 }.val().unwrap();
        let a = j.get("a").unwrap().arr();
        assert_eq!(a[0].num(), Some(1.0));
        assert_eq!(a[1].num(), Some(-25.0));
        assert_eq!(a[2].str(), Some("x\"yA"));
        assert!(matches!(j.get("b").unwrap().get("c"), Some(J::Bool(true))));
        assert!(matches!(j.get("b").unwrap().get("d"), Some(J::Null)));
    }

    /// The same points `tools/tectonic.py golden` prints (the Python copy of this arithmetic).
    #[test]
    fn noise_matches_the_python_reference() {
        let t = Tectonic::new(7, Config { min_y: -1, max_y: 127, ..Config::default() });
        let c = "tectonic:parameter/continentalness";
        close(t.noise(c, 1234.5, 0.0, -987.25).unwrap(), 0.667780386, 1e-7, "continentalness a");
        close(t.noise(c, -3.0, 0.0, 17.0).unwrap(), 0.173585768, 1e-7, "continentalness b");
        close(t.blended(100.0, 70.0, -40.0), 0.028203345, 1e-7, "blended");
        close(t.function("minecraft:shift_x", 64, 0, -32).unwrap(), -0.814071008, 1e-7, "shift_x");
    }

    /// Chunks of the game's world (seed 0xCAFEBABE, 128 high, sea 64): tops and densities from the Python reference.
    #[test]
    fn chunk_matches_the_python_reference() {
        let t = Tectonic::new(0xCAFEBABE, Config::compact(128, 64));
        assert_eq!(t.height(), 128);
        let h = 128;
        let c = t.chunk(0, 0);
        assert_eq!(c.len(), 16 * 16 * h);
        assert_eq!((top(&c, h, 0, 0), top(&c, h, 7, 9), top(&c, h, 15, 15)), (76, 75, 76));
        close(c[(8 * 16 + 8) * h + 70] as f64, 0.197016, 2e-3, "density (8,70,8)");
        close(c[(3 * 16 + 12) * h + 20] as f64, 0.458533, 2e-3, "density (3,20,12)");
        let c = t.chunk(-2, 3);
        assert_eq!((top(&c, h, 0, 0), top(&c, h, 7, 9), top(&c, h, 15, 15)), (68, 68, 68));
        close(c[(8 * 16 + 8) * h + 70] as f64, -0.044689, 2e-3, "density (8,70,8)");
    }

    fn plain(seed: i64, cfg: Config) -> Tectonic {
        Tectonic::build(BUNDLE, seed, cfg, false).unwrap()
    }

    /// The optimised graph answers with the plain graph's bits: whole chunks, surfaces and every named function.
    #[test]
    fn simplified_graph_gives_the_plain_graphs_bits() {
        let cfg = Config::compact(128, 64);
        let (fast, slow) = (Tectonic::new(0xCAFEBABE, cfg.clone()), plain(0xCAFEBABE, cfg));
        assert!(fast.nodes.len() < slow.nodes.len(), "{} vs {}", fast.nodes.len(), slow.nodes.len());
        for (cx, cz) in [(0, 0), (-2, 3), (17, -9)] {
            let (a, b) = (fast.chunk(cx, cz), slow.chunk(cx, cz));
            assert!(a.iter().zip(&b).all(|(p, q)| p.to_bits() == q.to_bits()), "chunk {cx},{cz}");
        }
        for (x, z) in [(0, 0), (-37, 211), (1000, -1000), (5, 5)] {
            assert_eq!(fast.surface(x, z), slow.surface(x, z), "surface {x},{z}");
        }
        let names: Vec<String> = slow.ids.keys().cloned().collect();
        for n in &names {
            for (x, y, z) in [(64, 70, -32), (-300, 20, 411)] {
                let (a, b) = (fast.function(n, x, y, z), slow.function(n, x, y, z));
                assert_eq!(a.map(f64::to_bits), b.map(f64::to_bits), "{n} at {x},{y},{z}");
            }
        }
    }

    #[test]
    fn simplify_drops_the_wrappers() {
        let t = Tectonic::new(1, Config::compact(128, 64));
        assert!(!t.nodes.iter().any(|n| matches!(n, Node::Pass(_))));
        // An `interpolated` argument has one table, however many nodes pointed at it.
        let mut args = t.interps.clone();
        args.sort();
        args.dedup();
        assert_eq!(args.len(), t.interps.len());
    }

    /// One memo across columns and points gives what a fresh memo gives.
    #[test]
    fn memo_follows_the_column_and_the_point() {
        let t = Tectonic::new(5, Config::compact(128, 64));
        let g = Grid { on: false, x0: 0, z0: 0, min_y: t.min_y, ny: 2, tabs: Vec::new() };
        let mut shared = Memo::new(t.nodes.len());
        for (x, z) in [(10, -20), (11, -20), (10, -21)] {
            shared.column();
            for y in [-1, 30, 31, 90] {
                shared.point();
                let a = t.ev(t.root, x, y, z, &mut shared, &g);
                let mut fresh = Memo::new(t.nodes.len());
                fresh.column();
                fresh.point();
                let b = t.ev(t.root, x, y, z, &mut fresh, &g);
                assert_eq!(a.to_bits(), b.to_bits(), "({x},{y},{z})");
            }
        }
    }

    /// Deterministic, solid at the bottom, open at the top, and neighbouring chunks meet (no seam on the shared border).
    #[test]
    fn chunk_is_deterministic_and_seamless() {
        let cfg = Config::compact(128, 64);
        let (a, b) = (Tectonic::new(3, cfg.clone()), Tectonic::new(3, cfg));
        let h = 128;
        let (c0, c1) = (a.chunk(4, -5), a.chunk(5, -5));
        assert_eq!(c0, b.chunk(4, -5));
        for x in 0..16 {
            for z in 0..16 {
                assert!(c0[(x * 16 + z) * h] > 0.0, "the bottom layer is not solid");
                assert!(c0[(x * 16 + z) * h + h - 1] <= 0.0, "the top layer is not air");
            }
        }
        // x = 15 of one chunk and x = 0 of the next are one block apart: their columns differ by one step of a smooth field.
        for z in 0..16 {
            let (p, q) = (top(&c0, h, 15, z), top(&c1, h, 0, z));
            assert!((p - q).abs() <= 6, "seam at z {z}: {p} vs {q}");
        }
    }
}
