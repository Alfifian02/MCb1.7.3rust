use bcore::chunk::{Chunk, W};
use bcore::worldgen;
use std::collections::HashMap;

pub struct World {
    pub seed: u32,
    pub chunks: HashMap<(i32, i32), Chunk>,
}

impl World {
    pub fn new(seed: u32, radius: i32) -> Self {
        let mut chunks = HashMap::new();
        for cx in -radius..=radius {
            for cz in -radius..=radius {
                chunks.insert((cx, cz), worldgen::generate(seed, cx, cz));
            }
        }
        Self { seed, chunks }
    }

    /// Block at world coordinates; unloaded chunks read as air.
    pub fn block(&self, x: i32, y: i32, z: i32) -> u8 {
        let w = W as i32;
        match self.chunks.get(&(x.div_euclid(w), z.div_euclid(w))) {
            Some(c) => c.get(x.rem_euclid(w), y, z.rem_euclid(w)),
            None => 0,
        }
    }
}
