//! The 13 biomes of this world and where they are placed. `Biome::from_params` is the Minecraft 1.18+ placement (the Overworld
//! section of the wiki's "World generation"), folded onto the Beta biomes; `from_climate` is b1.7.3's `BiomeGenBase.getBiome` /
//! `getBiomeFromLookup` (a 2D temperature x humidity table), kept for the golden tests.

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
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
    /// Open sea (ocean and deep ocean). The sea floor is sand.
    Ocean,
    /// The low coast between sea and land: sand.
    Beach,
    /// High, eroded ground: bare stone, snow-capped by the snow line where it is cold.
    Mountains,
}

/// The game's `MIDDLE_BIOMES`, rows temperature 0..4 (frozen to hot), columns humidity 0..4 (dry to wet), on this world's biomes:
/// snowy plains / taiga -> tundra / taiga, plains and sunflower plains -> plains, forest / flower / birch -> forest, dark forest
/// -> seasonal forest, savanna -> savanna, the forest of the hot-temperate row -> shrubland, jungle -> rainforest, desert -> desert.
const MIDDLE: [[Biome; 5]; 5] = {
    use Biome::*;
    [
        [Tundra, Tundra, Tundra, Taiga, Taiga],
        [Plains, Plains, Forest, Taiga, Taiga],
        [Plains, Plains, Forest, Forest, SeasonalForest],
        [Savanna, Savanna, Shrubland, Rainforest, Rainforest],
        [Desert, Desert, Desert, Desert, Desert],
    ]
};

impl Biome {
    /// `BiomeGenBase.getEnableSnow` (taiga, tundra; ice desert is never selected).
    pub fn snows(self) -> bool {
        matches!(self, Biome::Taiga | Biome::Tundra)
    }

    /// `BiomeGenBase.canSpawnLightningBolt`: rain falls where it does not snow and rain is not disabled (desert).
    pub fn rains(self) -> bool {
        !self.snows() && self != Biome::Desert
    }

    /// The overworld table of Minecraft 1.18+ (`OverworldBiomeBuilder`) for one surface point, from the five climate parameters
    /// (about -1..1 each; `w` is weirdness). In the order the game decides:
    /// - continentalness below -0.19 is sea (`Ocean`), up to -0.11 the coast (`Beach`; a hot coast stays desert);
    /// - inland, top erosion level + a valley or low ridge (peaks and valleys `1 - |3|w| - 2|`) in the cool or temperate bands is
    ///   `Swamp`; far inland with low erosion on a high ridge is `Mountains` (rivers are not a biome: `gen::hydro` cuts them);
    /// - anything else is a "middle biome" from `MIDDLE`: temperature (5 bands) x humidity (5 bands), cold at one end, desert at the
    ///   other, so a desert only borders the hot band's neighbour (savanna, shrubland, rainforest) or water, sand and rock.
    ///
    /// Folded onto this world's biomes: the game's variants (ice spikes, sunflower plains, old growth, bamboo, ...), badlands and
    /// plateau biomes have no counterpart yet, and the one `Ocean` stands for ocean, deep ocean and frozen ocean (the ice is the
    /// temperature's). Where the terrain does not follow the parameters (the Tectonic world type) the caller passes inland ones.
    pub fn from_params(t: f64, h: f64, c: f64, e: f64, w: f64) -> Biome {
        let level = |v: f64, edges: &[f64]| edges.iter().filter(|&&x| v >= x).count();
        let (ti, hi) = (level(t, &[-0.45, -0.15, 0.2, 0.55]), level(h, &[-0.35, -0.1, 0.1, 0.3]));
        let ei = level(e, &[-0.78, -0.375, -0.2225, 0.05, 0.45, 0.55]);
        let pv = 1.0 - (3.0 * w.abs() - 2.0).abs();
        if c < -0.19 {
            Biome::Ocean
        } else if c < -0.11 {
            if ti < 4 { Biome::Beach } else { MIDDLE[ti][hi] }
        } else if ei == 6 && pv < -0.2 && (1..=2).contains(&ti) {
            Biome::Swamp
        } else if c > 0.03 && ei <= 2 && pv > 0.2 {
            Biome::Mountains
        } else {
            MIDDLE[ti][hi]
        }
    }

    /// getBiomeFromLookup(temperature, humidity); both in [0, 1].
    #[cfg(test)]
    pub fn from_climate(temperature: f64, humidity: f64) -> Biome {
        // The Java table is indexed on a 64x64 grid, filled with f32 maths.
        let t = (temperature * 63.0) as i32;
        let h = (humidity * 63.0) as i32;
        Self::lookup(t as f32 / 63.0, h as f32 / 63.0)
    }

    /// BiomeGenBase.getBiome(temperature, humidity) as built into the table.
    #[cfg(test)]
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

    /// Block id of the top layer (grass; sand in deserts and on beaches; stone on mountains). Under the sea it is only used from
    /// `SEA_LEVEL - 1` up, so the floor of `Ocean` is its filler.
    pub fn top_block(self) -> u8 {
        match self {
            Biome::Desert | Biome::Beach => 12,
            Biome::Mountains => 1,
            _ => 2,
        }
    }

    /// Block id just under the top layer (dirt; sand in deserts, on beaches, on the sea and river floor; stone on mountains).
    pub fn filler_block(self) -> u8 {
        match self {
            Biome::Desert | Biome::Beach | Biome::Ocean => 12,
            Biome::Mountains => 1,
            _ => 3,
        }
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
            // Not in b1.7.3: the golden never sees them.
            Biome::Ocean | Biome::Beach | Biome::Mountains => u8::MAX,
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

    /// The 1.18+ middle table: the five bands of temperature in order, and the swamp rule.
    #[test]
    fn params_follow_the_bands() {
        let at = |t: f64, h: f64| Biome::from_params(t, h, 0.5, 0.0, 0.45);
        assert_eq!([-0.8, -0.3, 0.0, 0.4, 0.8].map(|t| at(t, -0.5)), [Biome::Tundra, Biome::Plains, Biome::Plains, Biome::Savanna, Biome::Desert]);
        assert_eq!([-0.8, -0.3, 0.0, 0.4, 0.8].map(|t| at(t, 0.5)), [Biome::Taiga, Biome::Taiga, Biome::SeasonalForest, Biome::Rainforest, Biome::Desert]);
        // Swamp: erosion level 6, a low ridge, inland, temperate. Not in the cold, not in the hot band, not out at sea, not on a peak.
        let swamp = |t: f64, c: f64, w: f64| Biome::from_params(t, 0.0, c, 0.8, w);
        assert_eq!(swamp(0.0, 0.5, 0.0), Biome::Swamp);
        assert_ne!(swamp(-0.8, 0.5, 0.0), Biome::Swamp);
        assert_ne!(swamp(0.8, 0.5, 0.0), Biome::Swamp);
        assert_ne!(swamp(0.0, -0.5, 0.0), Biome::Swamp);
        assert_ne!(swamp(0.0, 0.5, 0.7), Biome::Swamp);
    }

    /// Sea, coast, swamp and mountain, in the order `from_params` decides them.
    #[test]
    fn params_place_the_new_biomes() {
        let at = |t: f64, c: f64, e: f64, w: f64| Biome::from_params(t, 0.0, c, e, w);
        assert_eq!(at(0.0, -0.5, 0.0, 0.45), Biome::Ocean);
        assert_eq!(at(0.0, -0.15, 0.0, 0.45), Biome::Beach);
        assert_eq!(at(0.8, -0.15, 0.0, 0.45), Biome::Desert); // a hot coast is desert, not beach
        assert_eq!(at(0.0, 0.5, -0.3, 0.45), Biome::Mountains);
        assert_ne!(at(0.0, 0.0, -0.3, 0.45), Biome::Mountains); // near the coast: not far enough inland
        assert_ne!(at(0.0, 0.5, 0.0, 0.45), Biome::Mountains); // erosion too high (level 3)
        assert_eq!(at(0.0, 0.5, 0.8, 0.0), Biome::Swamp); // the bottom of a valley, flat ground
        assert_eq!((Biome::Ocean.filler_block(), Biome::Beach.top_block(), Biome::Mountains.top_block()), (12, 12, 1));
    }
}
