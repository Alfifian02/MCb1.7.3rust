//! DIHASILKAN oleh tools/gen_blocks.py dari registri b1.7.3 asli. Jangan edit manual.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Material {
    Air,
    Grass,
    Ground,
    Wood,
    Rock,
    Iron,
    Water,
    Lava,
    Leaves,
    Plants,
    Sponge,
    Cloth,
    Fire,
    Sand,
    Circuits,
    Glass,
    Tnt,
    Unused4262,
    Ice,
    Snow,
    BuiltSnow,
    Cactus,
    Clay,
    Pumpkin,
    Portal,
    Cake,
    Web,
    Piston,
}

#[derive(Clone, Copy, Debug)]
pub struct MaterialProps {
    pub map_color: u8,
    pub is_liquid: bool,
    /// Java: isSolid()
    pub solid: bool,
    pub can_block_grass: bool,
    /// Java: getIsSolid()
    pub is_solid2: bool,
    pub burns: bool,
    pub ground_cover: bool,
    /// Java: getIsTranslucent() (nama asli menyesatkan; ikuti perilaku Java)
    pub translucent: bool,
    pub harvestable: bool,
    /// 0 = bisa didorong piston, 1 = hancur saat didorong, 2 = tak bisa digerakkan
    pub mobility: u8,
}

pub static MATERIAL_PROPS: [MaterialProps; 28] = [
    MaterialProps { map_color: 0, is_liquid: false, solid: false, can_block_grass: false, is_solid2: false, burns: false, ground_cover: true, translucent: false, harvestable: true, mobility: 0 },  // Air
    MaterialProps { map_color: 1, is_liquid: false, solid: true, can_block_grass: true, is_solid2: true, burns: false, ground_cover: false, translucent: true, harvestable: true, mobility: 0 },  // Grass
    MaterialProps { map_color: 10, is_liquid: false, solid: true, can_block_grass: true, is_solid2: true, burns: false, ground_cover: false, translucent: true, harvestable: true, mobility: 0 },  // Ground
    MaterialProps { map_color: 13, is_liquid: false, solid: true, can_block_grass: true, is_solid2: true, burns: true, ground_cover: false, translucent: true, harvestable: true, mobility: 0 },  // Wood
    MaterialProps { map_color: 11, is_liquid: false, solid: true, can_block_grass: true, is_solid2: true, burns: false, ground_cover: false, translucent: true, harvestable: false, mobility: 0 },  // Rock
    MaterialProps { map_color: 6, is_liquid: false, solid: true, can_block_grass: true, is_solid2: true, burns: false, ground_cover: false, translucent: true, harvestable: false, mobility: 0 },  // Iron
    MaterialProps { map_color: 12, is_liquid: true, solid: false, can_block_grass: true, is_solid2: false, burns: false, ground_cover: true, translucent: false, harvestable: true, mobility: 1 },  // Water
    MaterialProps { map_color: 4, is_liquid: true, solid: false, can_block_grass: true, is_solid2: false, burns: false, ground_cover: true, translucent: false, harvestable: true, mobility: 1 },  // Lava
    MaterialProps { map_color: 7, is_liquid: false, solid: true, can_block_grass: true, is_solid2: true, burns: true, ground_cover: false, translucent: false, harvestable: true, mobility: 1 },  // Leaves
    MaterialProps { map_color: 7, is_liquid: false, solid: false, can_block_grass: false, is_solid2: false, burns: false, ground_cover: false, translucent: false, harvestable: true, mobility: 1 },  // Plants
    MaterialProps { map_color: 3, is_liquid: false, solid: true, can_block_grass: true, is_solid2: true, burns: false, ground_cover: false, translucent: true, harvestable: true, mobility: 0 },  // Sponge
    MaterialProps { map_color: 3, is_liquid: false, solid: true, can_block_grass: true, is_solid2: true, burns: true, ground_cover: false, translucent: true, harvestable: true, mobility: 0 },  // Cloth
    MaterialProps { map_color: 0, is_liquid: false, solid: false, can_block_grass: false, is_solid2: false, burns: false, ground_cover: true, translucent: false, harvestable: true, mobility: 1 },  // Fire
    MaterialProps { map_color: 2, is_liquid: false, solid: true, can_block_grass: true, is_solid2: true, burns: false, ground_cover: false, translucent: true, harvestable: true, mobility: 0 },  // Sand
    MaterialProps { map_color: 0, is_liquid: false, solid: false, can_block_grass: false, is_solid2: false, burns: false, ground_cover: false, translucent: false, harvestable: true, mobility: 1 },  // Circuits
    MaterialProps { map_color: 0, is_liquid: false, solid: true, can_block_grass: true, is_solid2: true, burns: false, ground_cover: false, translucent: false, harvestable: true, mobility: 0 },  // Glass
    MaterialProps { map_color: 4, is_liquid: false, solid: true, can_block_grass: true, is_solid2: true, burns: true, ground_cover: false, translucent: false, harvestable: true, mobility: 0 },  // Tnt
    MaterialProps { map_color: 7, is_liquid: false, solid: true, can_block_grass: true, is_solid2: true, burns: false, ground_cover: false, translucent: true, harvestable: true, mobility: 1 },  // Unused4262
    MaterialProps { map_color: 5, is_liquid: false, solid: true, can_block_grass: true, is_solid2: true, burns: false, ground_cover: false, translucent: false, harvestable: true, mobility: 0 },  // Ice
    MaterialProps { map_color: 8, is_liquid: false, solid: false, can_block_grass: false, is_solid2: false, burns: false, ground_cover: true, translucent: false, harvestable: false, mobility: 1 },  // Snow
    MaterialProps { map_color: 8, is_liquid: false, solid: true, can_block_grass: true, is_solid2: true, burns: false, ground_cover: false, translucent: true, harvestable: false, mobility: 0 },  // BuiltSnow
    MaterialProps { map_color: 7, is_liquid: false, solid: true, can_block_grass: true, is_solid2: true, burns: false, ground_cover: false, translucent: false, harvestable: true, mobility: 1 },  // Cactus
    MaterialProps { map_color: 9, is_liquid: false, solid: true, can_block_grass: true, is_solid2: true, burns: false, ground_cover: false, translucent: true, harvestable: true, mobility: 0 },  // Clay
    MaterialProps { map_color: 7, is_liquid: false, solid: true, can_block_grass: true, is_solid2: true, burns: false, ground_cover: false, translucent: true, harvestable: true, mobility: 1 },  // Pumpkin
    MaterialProps { map_color: 0, is_liquid: false, solid: false, can_block_grass: false, is_solid2: false, burns: false, ground_cover: false, translucent: false, harvestable: true, mobility: 2 },  // Portal
    MaterialProps { map_color: 0, is_liquid: false, solid: true, can_block_grass: true, is_solid2: true, burns: false, ground_cover: false, translucent: true, harvestable: true, mobility: 1 },  // Cake
    MaterialProps { map_color: 3, is_liquid: false, solid: true, can_block_grass: true, is_solid2: true, burns: false, ground_cover: false, translucent: true, harvestable: false, mobility: 1 },  // Web
    MaterialProps { map_color: 11, is_liquid: false, solid: true, can_block_grass: true, is_solid2: true, burns: false, ground_cover: false, translucent: true, harvestable: true, mobility: 2 },  // Piston
];

impl Material {
    #[inline]
    pub fn props(self) -> &'static MaterialProps {
        &MATERIAL_PROPS[self as usize]
    }
    #[inline] pub fn is_liquid(self) -> bool { self.props().is_liquid }
    #[inline] pub fn is_solid(self) -> bool { self.props().solid }
    #[inline] pub fn can_block_grass(self) -> bool { self.props().can_block_grass }
    #[inline] pub fn burns(self) -> bool { self.props().burns }
    #[inline] pub fn mobility(self) -> u8 { self.props().mobility }
}

/// Warna peta (MapColor): (indeks, RGB)
pub static MAP_COLORS: [(u8, u32); 14] = [
    (0, 0),  // airColor
    (1, 8368696),  // grassColor
    (2, 16247203),  // sandColor
    (3, 10987431),  // clothColor
    (4, 16711680),  // tntColor
    (5, 10526975),  // iceColor
    (6, 10987431),  // ironColor
    (7, 31744),  // foliageColor
    (8, 16777215),  // snowColor
    (9, 10791096),  // clayColor
    (10, 12020271),  // dirtColor
    (11, 7368816),  // stoneColor
    (12, 4210943),  // waterColor
    (13, 6837042),  // woodColor
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sifat_dasar() {
        assert!(Material::Water.is_liquid());
        assert!(!Material::Water.is_solid());
        assert!(Material::Wood.burns());
        assert!(!Material::Rock.burns());
        assert_eq!(Material::Portal.mobility(), 2);
        assert_eq!(Material::Piston.mobility(), 2);
    }
}
