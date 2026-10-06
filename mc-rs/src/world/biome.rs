//! Biome registry. Beta-1.7's overworld has 8 biomes. Vanilla equivalent:
//! net.minecraft.world.level.biome.Biomes. Names are vanilla (Plains, Forest, etc).
//!
//! `top_block` and `filler_block` are block ids (Block.stone=1, Block.dirt=3,
//! Block.grass=2, Block.sand=12). These come from Beta-1.7's BiomeGenBase.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Biome {
    Plains,
    Desert,
    Forest,
    Taiga,
    Rainforest,
    SeasonalForest,
    Tundra,
    Swamp,
    // Sky and Hell are separate dimensions in Beta-1.7, not part of overworld.
}

impl Biome {
    pub const ALL: [Biome; 8] = [
        Biome::Plains, Biome::Desert, Biome::Forest, Biome::Taiga,
        Biome::Rainforest, Biome::SeasonalForest, Biome::Tundra, Biome::Swamp,
    ];

    /// Block id of the top layer in this biome.
    pub fn top_block(self) -> u8 {
        match self {
            Biome::Plains | Biome::Forest | Biome::Taiga
            | Biome::Rainforest | Biome::SeasonalForest | Biome::Swamp => 2, // grass
            Biome::Desert => 12, // sand
            Biome::Tundra => 2,  // grass (snow falls on top in populate, not in terrain)
        }
    }

    /// Block id of the layer just under the top.
    pub fn filler_block(self) -> u8 {
        match self {
            Biome::Plains | Biome::Forest | Biome::Taiga
            | Biome::Rainforest | Biome::SeasonalForest | Biome::Swamp => 3, // dirt
            Biome::Desert => 12, // sand
            Biome::Tundra => 3,
        }
    }
}
