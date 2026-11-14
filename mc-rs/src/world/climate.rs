//! `ColorizerGrass` / `ColorizerFoliage`: the biome colour of a block column from its temperature and humidity, read from the
//! two 256 x 256 colour maps of the jar (`misc/grasscolor.png`, `foliagecolor.png`, written by `tools/gen_terrain.py`).

const GRASS: &[u8] = include_bytes!("../../assets/grasscolor.rgb");
const FOLIAGE: &[u8] = include_bytes!("../../assets/foliagecolor.rgb");
/// `ColorizerFoliage.getFoliageColorPine` / `getFoliageColorBirch`.
pub const PINE: u32 = 0x61_99_61;
pub const BIRCH: u32 = 0x80_A7_55;

/// Both maps are indexed the same way: `(1 - t) * 255` across, `(1 - h * t) * 255` down (`h` is scaled by `t` first).
fn lookup(map: &[u8], t: f64, h: f64) -> u32 {
    let (i, j) = (((1.0 - t) * 255.0) as usize, ((1.0 - h * t) * 255.0) as usize);
    let p = (j << 8 | i) * 3;
    (map[p] as u32) << 16 | (map[p + 1] as u32) << 8 | map[p + 2] as u32
}

/// `ColorizerGrass.getGrassColor` as `r << 16 | g << 8 | b`.
pub fn grass(t: f64, h: f64) -> u32 {
    lookup(GRASS, t.clamp(0.0, 1.0), h.clamp(0.0, 1.0))
}

/// `ColorizerFoliage.getFoliageColor`.
pub fn foliage(t: f64, h: f64) -> u32 {
    lookup(FOLIAGE, t.clamp(0.0, 1.0), h.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hot dry corner is the map's top-left (savanna yellow-green), a cold one its right edge; both stay 24 bit.
    #[test]
    fn colours_come_from_the_maps() {
        assert_eq!(GRASS.len(), 256 * 256 * 3);
        assert_eq!(grass(1.0, 0.0), (GRASS[0] as u32) << 16 | (GRASS[1] as u32) << 8 | GRASS[2] as u32);
        assert_ne!(grass(1.0, 0.0), grass(0.0, 0.0));
        assert!(foliage(0.8, 0.4) < 1 << 24 && foliage(0.8, 0.4) != 0);
    }
}
