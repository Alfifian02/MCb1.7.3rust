//! Simple deterministic terrain: bedrock, stone, dirt, grass/sand, sea at y < 64.
use crate::block::*;
use crate::chunk::{Chunk, H, W};
use crate::noise::fbm;

pub const SEA_LEVEL: usize = 64;

pub fn height_at(seed: u32, wx: i32, wz: i32) -> usize {
    let n = fbm(seed, wx as f32 / 96.0, wz as f32 / 96.0, 4);
    (52.0 + n * 40.0).clamp(2.0, (H - 2) as f32) as usize
}

pub fn generate(seed: u32, cx: i32, cz: i32) -> Chunk {
    let mut c = Chunk::new();
    for x in 0..W {
        for z in 0..W {
            let h = height_at(seed, cx * W as i32 + x as i32, cz * W as i32 + z as i32);
            for y in 0..=h {
                let id = if y == 0 {
                    BEDROCK
                } else if y + 4 <= h {
                    STONE
                } else if y < h {
                    DIRT
                } else if h <= SEA_LEVEL + 1 {
                    SAND
                } else {
                    GRASS
                };
                c.set(x, y, z, id);
            }
            for y in (h + 1)..SEA_LEVEL {
                c.set(x, y, z, WATER);
            }
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        let a = generate(42, 3, -7);
        let b = generate(42, 3, -7);
        assert_eq!(a.as_bytes(), b.as_bytes());
    }

    #[test]
    fn has_bedrock_floor_and_sea() {
        let c = generate(1, 0, 0);
        for x in 0..16 { for z in 0..16 { assert_eq!(c.get(x, 0, z), BEDROCK); } }
        assert_eq!(c.get(0, 127, 0), AIR);
    }
}
