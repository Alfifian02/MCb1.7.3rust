//! Biomes reachable by Beta-1.7 climate. Port of BiomeGenBase.getBiome /
//! getBiomeFromLookup. BiomeGenBase also defines Ice Desert (and Hell/Sky),
//! but getBiome never returns it, so overworld generation can't produce it.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Biome {
    Rainforest,
    Swamp,
    SeasonalForest,
    Forest,
    Savanna,
    Shrubland,
    Taiga,
    Desert,
    Plains,
    Tundra,
}

impl Biome {
    /// getBiomeFromLookup(temperature, humidity); both in [0, 1].
    pub fn from_climate(temperature: f64, humidity: f64) -> Biome {
        // The Java table is indexed on a 64x64 grid, filled with f32 maths.
        let t = (temperature * 63.0) as i32;
        let h = (humidity * 63.0) as i32;
        Self::lookup(t as f32 / 63.0, h as f32 / 63.0)
    }

    /// BiomeGenBase.getBiome(temperature, humidity) as built into the table.
    fn lookup(t: f32, h: f32) -> Biome {
        let h = h * t;
        if t < 0.1 {
            Biome::Tundra
        } else if h < 0.2 {
            if t < 0.5 { Biome::Tundra } else if t < 0.95 { Biome::Savanna } else { Biome::Desert }
        } else if h > 0.5 && t < 0.7 {
            Biome::Swamp
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

    /// Block id of the top layer (grass, or sand in deserts).
    pub fn top_block(self) -> u8 {
        if self == Biome::Desert { 12 } else { 2 }
    }

    /// Block id just under the top layer (dirt, or sand in deserts).
    pub fn filler_block(self) -> u8 {
        if self == Biome::Desert { 12 } else { 3 }
    }

    /// Numbering used by tools/golden/G.java.
    #[cfg(test)]
    pub fn java_code(self) -> u8 {
        match self {
            Biome::Rainforest => 0,
            Biome::Swamp => 1,
            Biome::SeasonalForest => 2,
            Biome::Forest => 3,
            Biome::Savanna => 4,
            Biome::Shrubland => 5,
            Biome::Taiga => 6,
            Biome::Desert => 7,
            Biome::Plains => 8,
            Biome::Tundra => 10,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn climate_corners() {
        assert_eq!(Biome::from_climate(0.0, 0.0), Biome::Tundra);
        assert_eq!(Biome::from_climate(1.0, 0.0), Biome::Desert);
        assert_eq!(Biome::from_climate(1.0, 1.0), Biome::Rainforest);
        assert_eq!(Biome::from_climate(0.3, 0.9), Biome::Taiga);
        assert_eq!(Biome::from_climate(0.6, 0.9), Biome::Swamp);
    }
}
