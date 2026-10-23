//! The 19 biomes of this world and where they are placed (13 in every world, 6 more in a Blooming world). `Biome::from_params` is the Minecraft 1.18+ placement (the Overworld
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
    // The six biomes of the Blooming Biosphere datapack that this world can show (`Biome::blooming`, the Blooming world type only).
    // Tidepools is not ported (all coral and tuff). Substitutions: dark oak / acacia / jungle -> oak, cherry -> birch, mud / podzol /
    // coarse dirt / packed mud -> dirt, the new flowers and double plants -> what Beta has (or nothing).
    /// Cool, damp woods of birch, spruce and oak (the datapack's `autumnal_forest`).
    AutumnalForest,
    /// Warm, dry scrub of oaks and birches (`chaparral`).
    Chaparral,
    /// Flat wetland with a few oaks and lots of grass (`marsh`).
    Marsh,
    /// Temperate oak woods (`oak_woodland`).
    OakWoodland,
    /// Dense warm rainforest (the datapack's `rainforest`; renamed, `Rainforest` is Beta's).
    WetWoodland,
    /// Cold birch grove (the datapack's `snowy_cherry_grove`, cherry trees being birches here).
    SnowyBirchGrove,
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

/// Blooming Biosphere's surface boxes, (biome, [temperature, humidity, continentalness, erosion, weirdness] as (min, max)), generated from
/// `data/minecraft/dimension/overworld.json` (the depth-0 entries of its six land biomes, in its order, repeats dropped).
#[rustfmt::skip]
const BLOOMING: &[(Biome, [(f64, f64); 5])] = &[
    (Biome::AutumnalForest, [(-0.45, -0.15), (0.1, 0.3), (-0.19, 0.3), (-0.2225, 0.05), (-1.0, -0.266667)]),
    (Biome::OakWoodland, [(-0.15, 0.2), (-0.1, 0.1), (-0.19, 0.3), (-0.2225, 0.05), (-1.0, -0.266667)]),
    (Biome::WetWoodland, [(0.2, 0.55), (0.3, 1.0), (-0.19, 1.0), (-0.2225, 0.05), (-1.0, -0.266667)]),
    (Biome::SnowyBirchGrove, [(-1.0, -0.45), (0.1, 0.7), (-0.11, 0.03), (-1.0, -0.375), (-1.0, -0.766667)]),
    (Biome::WetWoodland, [(0.2, 0.55), (0.3, 1.0), (-0.11, 0.03), (-1.0, 0.55), (-1.0, -0.766667)]),
    (Biome::AutumnalForest, [(-0.45, -0.15), (0.1, 0.3), (-0.11, 0.03), (-0.78, 0.55), (-1.0, -0.766667)]),
    (Biome::OakWoodland, [(-0.15, 0.2), (-0.1, 0.1), (-0.11, 0.03), (-0.78, 0.55), (-1.0, -0.766667)]),
    (Biome::Marsh, [(-0.15, 0.55), (-1.0, -0.35), (-0.11, 1.0), (0.55, 1.0), (-1.0, -0.933333)]),
    (Biome::Marsh, [(-0.15, 0.55), (0.7, 1.0), (-0.11, 1.0), (0.55, 1.0), (-1.0, -0.933333)]),
    (Biome::SnowyBirchGrove, [(-1.0, -0.45), (0.1, 0.7), (0.03, 1.0), (-1.0, -0.375), (-1.0, -0.933333)]),
    (Biome::WetWoodland, [(0.2, 0.55), (0.3, 1.0), (0.03, 1.0), (-1.0, 0.55), (-1.0, -0.933333)]),
    (Biome::AutumnalForest, [(-0.45, -0.15), (0.1, 0.3), (0.03, 0.3), (-0.78, 0.45), (-1.0, -0.933333)]),
    (Biome::OakWoodland, [(-0.15, 0.2), (-0.1, 0.1), (0.03, 0.3), (-0.78, 0.45), (-1.0, -0.933333)]),
    (Biome::AutumnalForest, [(-0.45, -0.15), (0.1, 0.3), (0.3, 1.0), (-0.2225, 0.45), (-1.0, -0.933333)]),
    (Biome::OakWoodland, [(-0.15, 0.2), (-0.1, 0.1), (0.3, 1.0), (-0.2225, 0.45), (-1.0, -0.933333)]),
    (Biome::AutumnalForest, [(-0.45, -0.15), (0.1, 0.3), (-0.19, -0.11), (-1.0, 1.0), (-0.933333, -0.766667)]),
    (Biome::OakWoodland, [(-0.15, 0.2), (-0.1, 0.1), (-0.19, -0.11), (-1.0, 1.0), (-0.933333, -0.766667)]),
    (Biome::WetWoodland, [(0.2, 0.55), (0.3, 1.0), (-0.19, 0.03), (-1.0, 1.0), (-0.933333, -0.766667)]),
    (Biome::AutumnalForest, [(-0.45, -0.15), (0.1, 0.3), (-0.11, 1.0), (0.55, 1.0), (-0.933333, -0.4)]),
    (Biome::OakWoodland, [(-0.15, 0.2), (-0.1, 0.1), (-0.11, 1.0), (0.55, 1.0), (-0.933333, -0.4)]),
    (Biome::SnowyBirchGrove, [(-1.0, -0.45), (0.1, 0.7), (0.03, 1.0), (-0.78, -0.375), (-0.933333, -0.766667)]),
    (Biome::WetWoodland, [(0.2, 0.55), (0.3, 1.0), (0.03, 1.0), (-0.78, 1.0), (-0.933333, -0.766667)]),
    (Biome::AutumnalForest, [(-0.45, -0.15), (0.1, 0.3), (0.03, 1.0), (0.05, 0.45), (-0.933333, -0.05)]),
    (Biome::OakWoodland, [(-0.15, 0.2), (-0.1, 0.1), (0.03, 1.0), (0.05, 0.45), (-0.933333, -0.05)]),
    (Biome::SnowyBirchGrove, [(-1.0, -0.45), (0.1, 0.7), (-0.19, 0.03), (-0.78, -0.375), (-0.766667, -0.566667)]),
    (Biome::AutumnalForest, [(-0.45, -0.15), (0.1, 0.3), (-0.19, 0.03), (-0.78, 0.45), (-0.766667, -0.4)]),
    (Biome::OakWoodland, [(-0.15, 0.2), (-0.1, 0.1), (-0.19, 0.03), (-0.78, 0.45), (-0.766667, -0.4)]),
    (Biome::WetWoodland, [(0.2, 0.55), (0.3, 1.0), (-0.19, 0.03), (-0.78, 1.0), (-0.766667, -0.4)]),
    (Biome::AutumnalForest, [(-0.45, -0.15), (0.1, 0.3), (-0.19, 1.0), (0.55, 1.0), (-0.766667, -0.4)]),
    (Biome::OakWoodland, [(-0.15, 0.2), (-0.1, 0.1), (-0.19, 1.0), (0.55, 1.0), (-0.766667, -0.4)]),
    (Biome::WetWoodland, [(0.2, 0.55), (0.3, 1.0), (0.03, 1.0), (-0.375, 1.0), (-0.766667, -0.4)]),
    (Biome::AutumnalForest, [(-0.45, -0.15), (0.1, 0.3), (-0.19, -0.11), (-1.0, 1.0), (-0.566667, -0.4)]),
    (Biome::OakWoodland, [(-0.15, 0.2), (-0.1, 0.1), (-0.19, -0.11), (-1.0, 1.0), (-0.566667, -0.4)]),
    (Biome::WetWoodland, [(0.2, 0.55), (0.3, 1.0), (-0.19, 0.03), (-1.0, 1.0), (-0.566667, -0.4)]),
    (Biome::SnowyBirchGrove, [(-1.0, -0.45), (0.1, 0.7), (-0.11, 0.03), (-1.0, -0.375), (-0.566667, -0.266667)]),
    (Biome::AutumnalForest, [(-0.45, -0.15), (0.1, 0.3), (-0.11, 0.03), (0.45, 1.0), (-0.566667, -0.4)]),
    (Biome::OakWoodland, [(-0.15, 0.2), (-0.1, 0.1), (-0.11, 0.03), (0.45, 1.0), (-0.566667, -0.4)]),
    (Biome::SnowyBirchGrove, [(-1.0, -0.45), (0.1, 0.7), (0.03, 1.0), (-0.78, -0.375), (-0.566667, -0.05)]),
    (Biome::WetWoodland, [(0.2, 0.55), (0.3, 1.0), (0.03, 1.0), (-0.78, 1.0), (-0.566667, -0.4)]),
    (Biome::WetWoodland, [(0.2, 0.55), (0.3, 1.0), (-0.11, 0.8), (-1.0, 0.55), (-0.4, -0.05)]),
    (Biome::AutumnalForest, [(-0.45, -0.15), (0.1, 0.3), (-0.11, 0.03), (-0.78, 0.55), (-0.4, -0.05)]),
    (Biome::OakWoodland, [(-0.15, 0.2), (-0.1, 0.1), (-0.11, 0.03), (-0.78, 0.55), (-0.4, -0.05)]),
    (Biome::Marsh, [(-0.15, 0.55), (-1.0, -0.35), (-0.11, 1.0), (0.55, 1.0), (-0.4, 0.4)]),
    (Biome::Marsh, [(-0.15, 0.55), (0.7, 1.0), (-0.11, 1.0), (0.55, 1.0), (-0.4, 0.4)]),
    (Biome::SnowyBirchGrove, [(-1.0, -0.45), (0.1, 0.7), (0.03, 1.0), (-1.0, -0.375), (-0.4, -0.05)]),
    (Biome::AutumnalForest, [(-0.45, -0.15), (0.1, 0.3), (0.03, 0.3), (-0.78, 0.45), (-0.4, -0.05)]),
    (Biome::OakWoodland, [(-0.15, 0.2), (-0.1, 0.1), (0.03, 0.3), (-0.78, 0.45), (-0.4, -0.05)]),
    (Biome::AutumnalForest, [(-0.45, -0.15), (0.1, 0.3), (0.3, 1.0), (-0.2225, 0.45), (-0.4, -0.05)]),
    (Biome::OakWoodland, [(-0.15, 0.2), (-0.1, 0.1), (0.3, 1.0), (-0.2225, 0.45), (-0.4, -0.05)]),
    (Biome::WetWoodland, [(0.2, 0.55), (0.3, 1.0), (0.8, 1.0), (-1.0, 0.55), (-0.4, -0.266667)]),
    (Biome::AutumnalForest, [(-0.45, -0.15), (0.1, 0.3), (-0.11, 0.8), (-1.0, 0.55), (-0.266667, -0.05)]),
    (Biome::OakWoodland, [(-0.15, 0.2), (-0.1, 0.1), (-0.11, 0.8), (-1.0, 0.55), (-0.266667, -0.05)]),
    (Biome::AutumnalForest, [(-0.45, -0.15), (0.1, 0.3), (0.8, 1.0), (-1.0, 0.45), (-0.266667, -0.05)]),
    (Biome::OakWoodland, [(-0.15, 0.2), (-0.1, 0.1), (0.8, 1.0), (-1.0, 0.45), (-0.266667, -0.05)]),
    (Biome::WetWoodland, [(0.2, 0.55), (0.3, 1.0), (0.8, 1.0), (-1.0, 0.45), (-0.266667, -0.05)]),
    (Biome::Marsh, [(-1.0, 1.0), (-1.0, 1.0), (0.8, 1.0), (0.45, 0.55), (-0.266667, -0.05)]),
    (Biome::Chaparral, [(0.2, 0.55), (-0.35, -0.1), (0.03, 1.0), (-1.0, -0.375), (-0.05, 0.266667)]),
    (Biome::Chaparral, [(0.2, 0.55), (-0.35, -0.1), (-0.11, 1.0), (-1.0, 0.45), (0.05, 0.266667)]),
    (Biome::SnowyBirchGrove, [(-1.0, -0.45), (0.1, 0.3), (0.03, 1.0), (-1.0, -0.375), (0.05, 0.4)]),
    (Biome::Marsh, [(-1.0, 1.0), (-1.0, 1.0), (0.03, 1.0), (0.45, 0.55), (0.05, 0.266667)]),
    (Biome::Chaparral, [(0.2, 0.55), (-0.35, -0.1), (-0.19, 0.3), (-0.2225, 0.45), (0.266667, 1.0)]),
    (Biome::SnowyBirchGrove, [(-1.0, -0.45), (0.1, 0.3), (-0.11, 0.03), (-1.0, -0.375), (0.266667, 0.566667)]),
    (Biome::WetWoodland, [(0.2, 0.55), (0.3, 1.0), (-0.11, 0.03), (-1.0, -0.78), (0.266667, 0.566667)]),
    (Biome::Chaparral, [(0.2, 0.55), (-0.35, -0.1), (-0.11, 0.03), (-0.78, 0.45), (0.266667, 1.0)]),
    (Biome::WetWoodland, [(0.2, 0.55), (0.3, 1.0), (0.03, 1.0), (-1.0, -0.78), (0.266667, 0.4)]),
    (Biome::Chaparral, [(0.2, 0.55), (-0.35, -0.1), (0.03, 0.3), (-0.78, 0.55), (0.266667, 0.4)]),
    (Biome::WetWoodland, [(0.2, 0.55), (0.3, 1.0), (0.3, 1.0), (-0.78, -0.2225), (0.266667, 0.566667)]),
    (Biome::Chaparral, [(0.2, 0.55), (-0.35, -0.1), (0.3, 1.0), (-0.2225, 0.55), (0.266667, 0.4)]),
    (Biome::Chaparral, [(0.2, 0.55), (-0.35, -0.1), (-0.19, -0.11), (-1.0, 0.45), (0.4, 0.566667)]),
    (Biome::Chaparral, [(0.2, 0.55), (-0.35, -0.1), (-0.19, 1.0), (0.55, 1.0), (0.4, 0.933333)]),
    (Biome::SnowyBirchGrove, [(-1.0, -0.45), (0.1, 0.3), (0.03, 1.0), (-0.78, -0.375), (0.4, 0.566667)]),
    (Biome::WetWoodland, [(0.2, 0.55), (0.3, 1.0), (0.03, 1.0), (-0.78, -0.2225), (0.4, 0.566667)]),
    (Biome::Chaparral, [(0.2, 0.55), (-0.35, -0.1), (0.03, 1.0), (0.45, 1.0), (0.4, 0.933333)]),
    (Biome::WetWoodland, [(0.2, 0.55), (0.3, 1.0), (0.3, 1.0), (-0.2225, 0.05), (0.4, 0.933333)]),
    (Biome::Chaparral, [(0.2, 0.55), (-0.35, -0.1), (0.3, 1.0), (0.05, 1.0), (0.4, 0.933333)]),
    (Biome::SnowyBirchGrove, [(-1.0, -0.45), (0.1, 0.3), (-0.19, 0.03), (-0.78, -0.375), (0.566667, 0.766667)]),
    (Biome::Chaparral, [(0.2, 0.55), (-0.35, -0.1), (-0.19, 0.03), (-0.78, 0.45), (0.566667, 0.933333)]),
    (Biome::WetWoodland, [(0.2, 0.55), (0.3, 1.0), (0.03, 1.0), (-0.375, -0.2225), (0.566667, 0.933333)]),
    (Biome::Chaparral, [(0.2, 0.55), (-0.35, -0.1), (-0.19, -0.11), (-1.0, 0.45), (0.766667, 0.933333)]),
    (Biome::SnowyBirchGrove, [(-1.0, -0.45), (0.1, 0.3), (-0.11, 0.03), (-1.0, -0.375), (0.766667, 1.0)]),
    (Biome::WetWoodland, [(0.2, 0.55), (0.3, 1.0), (-0.11, 0.03), (-1.0, -0.78), (0.766667, 1.0)]),
    (Biome::SnowyBirchGrove, [(-1.0, -0.45), (0.1, 0.3), (0.03, 1.0), (-0.78, -0.375), (0.766667, 1.0)]),
    (Biome::WetWoodland, [(0.2, 0.55), (0.3, 1.0), (0.03, 1.0), (-0.78, -0.2225), (0.766667, 0.933333)]),
    (Biome::Marsh, [(-0.15, 0.55), (-1.0, -0.35), (-0.11, 1.0), (0.55, 1.0), (0.933333, 1.0)]),
    (Biome::Marsh, [(-0.15, 0.55), (0.7, 1.0), (-0.11, 1.0), (0.55, 1.0), (0.933333, 1.0)]),
    (Biome::SnowyBirchGrove, [(-1.0, -0.45), (0.1, 0.3), (0.03, 1.0), (-1.0, -0.375), (0.933333, 1.0)]),
    (Biome::WetWoodland, [(0.2, 0.55), (0.3, 1.0), (0.03, 1.0), (-1.0, -0.78), (0.933333, 1.0)]),
    (Biome::Chaparral, [(0.2, 0.55), (-0.35, -0.1), (0.03, 0.3), (-0.78, 0.55), (0.933333, 1.0)]),
    (Biome::WetWoodland, [(0.2, 0.55), (0.3, 1.0), (0.3, 1.0), (-0.78, -0.2225), (0.933333, 1.0)]),
    (Biome::Chaparral, [(0.2, 0.55), (-0.35, -0.1), (0.3, 1.0), (-0.2225, 0.55), (0.933333, 1.0)]),
];

impl Biome {
    /// `BiomeGenBase.getEnableSnow` (taiga, tundra, the snowy birch grove; ice desert is never selected).
    pub fn snows(self) -> bool {
        matches!(self, Biome::Taiga | Biome::Tundra | Biome::SnowyBirchGrove)
    }

    /// `BiomeGenBase.canSpawnLightningBolt`: rain falls where it does not snow and rain is not disabled (desert, and the datapack's
    /// chaparral, which has no precipitation).
    pub fn rains(self) -> bool {
        !self.snows() && !matches!(self, Biome::Desert | Biome::Chaparral)
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

    /// `from_params` of the Blooming world type: the surface boxes of the Blooming Biosphere datapack (its `dimension/overworld.json`,
    /// depth 0, in that file's order) win where the point is inside one, else the table above. The datapack cuts its biomes out of
    /// the game's own middle-table cells, so this is the same placement, folded onto this world's biomes.
    /// ponytail: a box is "inside or not"; the game takes the nearest box of all of them, so it also lets a vanilla box win where a
    /// new one is merely close. Upgrade: a distance over the 7 parameters if the borders look too hard.
    pub fn blooming(t: f64, h: f64, c: f64, e: f64, w: f64) -> Biome {
        let p = [t, h, c, e, w];
        BLOOMING
            .iter()
            .find(|(_, r)| p.iter().zip(r).all(|(v, &(lo, hi))| (lo..=hi).contains(v)))
            .map_or_else(|| Biome::from_params(t, h, c, e, w), |&(b, _)| b)
    }

    /// The datapack's `grass_color` and `foliage_color` (`r << 16 | g << 8 | b`) of a Blooming biome, `None` for the others (their
    /// colours come from the climate maps).
    pub fn colors(self) -> Option<(u32, u32)> {
        match self {
            Biome::AutumnalForest => Some((10076245, 12998969)),
            Biome::Chaparral => Some((11385946, 8758586)),
            Biome::Marsh => Some((6260806, 6982711)),
            Biome::OakWoodland => Some((10665811, 8564020)),
            Biome::WetWoodland => Some((7065392, 4700193)),
            Biome::SnowyBirchGrove => Some((9023837, 8038732)),
            _ => None,
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
            Biome::AutumnalForest | Biome::Chaparral | Biome::Marsh | Biome::OakWoodland | Biome::WetWoodland | Biome::SnowyBirchGrove => u8::MAX,
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

    /// The Blooming table: a point inside a box is a Blooming biome (the first box that holds it), outside all of them it is the
    /// plain table's, the sea stays the sea, and the snowy grove snows.
    #[test]
    fn blooming_boxes_place_their_biomes() {
        for &(b, r) in BLOOMING {
            let m = r.map(|(lo, hi)| (lo + hi) / 2.0);
            assert!(Biome::blooming(m[0], m[1], m[2], m[3], m[4]).colors().is_some(), "{b:?}: the middle of its own box");
            assert!(r[2].0 >= -0.19, "{b:?} reaches the sea");
        }
        // The first box: cool, damp, inland, low erosion, weirdness down to -0.27.
        assert_eq!(Biome::blooming(-0.3, 0.2, 0.1, -0.1, -0.5), Biome::AutumnalForest);
        // The same point in a world without the datapack is a taiga (cell 1, 3 of the middle table).
        assert_eq!(Biome::from_params(-0.3, 0.2, 0.1, -0.1, -0.5), Biome::Taiga);
        // Out at sea nothing changes.
        assert_eq!(Biome::blooming(-0.3, 0.2, -0.5, -0.1, -0.5), Biome::Ocean);
        assert!(Biome::SnowyBirchGrove.snows() && !Biome::SnowyBirchGrove.rains() && Biome::Marsh.rains() && !Biome::Chaparral.rains());
        assert_eq!(Biome::Taiga.colors(), None);
    }
}
