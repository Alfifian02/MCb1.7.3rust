//! Port BiomeGenBase (data yang dipakai generator; daftar spawn menyusul di fase entity).

use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Biome {
    Rainforest,
    Swampland,
    SeasonalForest,
    Forest,
    Savanna,
    Shrubland,
    Taiga,
    Desert,
    Plains,
    IceDesert,
    Tundra,
    Hell,
    Sky,
}

pub struct BiomeProps {
    pub name: &'static str,
    pub color: i32,
    pub top_block: u8,
    pub filler_block: u8,
    /// Java: field_6502_q (warna rumput/daun basis)
    pub foliage_color: i32,
    pub enable_snow: bool,
    pub enable_rain: bool,
}

const GRASS: u8 = 2;
const DIRT: u8 = 3;
const SAND: u8 = 12;

const fn p(name: &'static str, color: i32, foliage: i32, snow: bool, rain: bool, top: u8, fill: u8) -> BiomeProps {
    BiomeProps { name, color, top_block: top, filler_block: fill, foliage_color: foliage, enable_snow: snow, enable_rain: rain }
}

static PROPS: [BiomeProps; 13] = [
    p("Rainforest", 588342, 2094168, false, true, GRASS, DIRT),
    p("Swampland", 522674, 9154376, false, true, GRASS, DIRT),
    p("Seasonal Forest", 10215459, 5169201, false, true, GRASS, DIRT),
    p("Forest", 353825, 5159473, false, true, GRASS, DIRT),
    p("Savanna", 14278691, 5169201, false, true, GRASS, DIRT),
    p("Shrubland", 10595616, 5169201, false, true, GRASS, DIRT),
    p("Taiga", 3060051, 8107825, true, true, GRASS, DIRT),
    // desert dan iceDesert: topBlock = fillerBlock = pasir (diatur generateBiomeLookup di Java)
    p("Desert", 16421912, 5169201, false, false, SAND, SAND),
    p("Plains", 16767248, 5169201, false, true, GRASS, DIRT),
    p("Ice Desert", 16772499, 12899129, true, false, SAND, SAND),
    p("Tundra", 5762041, 12899129, true, true, GRASS, DIRT),
    p("Hell", 16711680, 5169201, false, false, GRASS, DIRT),
    p("Sky", 8421631, 5169201, false, false, GRASS, DIRT),
];

impl Biome {
    #[inline]
    pub fn props(self) -> &'static BiomeProps {
        &PROPS[self as usize]
    }

    pub fn can_spawn_lightning(self) -> bool {
        let p = self.props();
        if p.enable_snow { false } else { p.enable_rain }
    }

    /// Java: getBiome(float, float)
    pub fn from_climate(t: f32, h: f32) -> Biome {
        let h = h * t;
        if t < 0.1 {
            Biome::Tundra
        } else if h < 0.2 {
            if t < 0.5 {
                Biome::Tundra
            } else if t < 0.95 {
                Biome::Savanna
            } else {
                Biome::Desert
            }
        } else if h > 0.5 && t < 0.7 {
            Biome::Swampland
        } else if t < 0.5 {
            Biome::Taiga
        } else if t < 0.97 {
            if h < 0.35 { Biome::Shrubland } else { Biome::Forest }
        } else if h < 0.45 {
            Biome::Plains
        } else if h < 0.9 {
            Biome::SeasonalForest
        } else {
            Biome::Rainforest
        }
    }

    /// Java: getBiomeFromLookup(temperature, humidity), tabel 64x64.
    pub fn from_lookup(temperature: f64, humidity: f64) -> Biome {
        static TABLE: OnceLock<Box<[Biome; 4096]>> = OnceLock::new();
        let t = TABLE.get_or_init(|| {
            let mut tb = Box::new([Biome::Plains; 4096]);
            for a in 0..64usize {
                for b in 0..64usize {
                    tb[a + b * 64] = Biome::from_climate(a as f32 / 63.0f32, b as f32 / 63.0f32);
                }
            }
            tb
        });
        let a = (temperature * 63.0) as i32;
        let b = (humidity * 63.0) as i32;
        t[(a + b * 64) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iklim_dasar() {
        assert_eq!(Biome::from_climate(0.05, 0.5), Biome::Tundra);
        assert_eq!(Biome::from_climate(1.0, 0.0), Biome::Desert);
        assert_eq!(Biome::from_climate(1.0, 1.0), Biome::Rainforest);
        assert_eq!(Biome::from_lookup(0.3, 0.9), Biome::Taiga);
    }

    #[test]
    fn permukaan_gurun_pasir() {
        assert_eq!(Biome::Desert.props().top_block, 12);
        assert_eq!(Biome::Forest.props().top_block, 2);
        assert_eq!(Biome::Forest.props().filler_block, 3);
    }
}
