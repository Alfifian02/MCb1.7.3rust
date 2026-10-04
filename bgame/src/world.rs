use bcore::block::{AIR, WATER};
use bcore::chunk::{Chunk, H, W};
use bcore::worldgen;
use std::collections::HashMap;
use std::sync::Arc;

/// (chunk x, chunk z, section y)
pub type SecKey = (i32, i32, usize);

pub struct World {
    pub seed: u32,
    pub chunks: HashMap<(i32, i32), Arc<Chunk>>,
}

impl World {
    pub fn new(seed: u32, radius: i32) -> Self {
        let mut chunks = HashMap::new();
        for cx in -radius..=radius {
            for cz in -radius..=radius {
                chunks.insert((cx, cz), Arc::new(worldgen::generate(seed, cx, cz)));
            }
        }
        Self { seed, chunks }
    }

    pub fn empty(seed: u32) -> Self {
        Self { seed, chunks: HashMap::new() }
    }

    fn split(x: i32, z: i32) -> ((i32, i32), (i32, i32)) {
        let w = W as i32;
        ((x.div_euclid(w), z.div_euclid(w)), (x.rem_euclid(w), z.rem_euclid(w)))
    }

    /// Block at world coordinates; unloaded chunks read as air (used by the mesher).
    pub fn block(&self, x: i32, y: i32, z: i32) -> u8 {
        let (c, l) = Self::split(x, z);
        self.chunks.get(&c).map_or(AIR, |ch| ch.get(l.0, y, l.1))
    }

    /// Collision: solid blocks, the void floor, and the edge of the loaded world.
    pub fn is_solid(&self, x: i32, y: i32, z: i32) -> bool {
        if y < 0 {
            return true;
        }
        if y >= H as i32 {
            return false;
        }
        let (c, l) = Self::split(x, z);
        match self.chunks.get(&c) {
            Some(ch) => !matches!(ch.get(l.0, y, l.1), AIR | WATER),
            None => true,
        }
    }

    pub fn is_water(&self, x: i32, y: i32, z: i32) -> bool {
        self.block(x, y, z) == WATER
    }

    /// Can the crosshair target this block?
    pub fn is_target(&self, x: i32, y: i32, z: i32) -> bool {
        !matches!(self.block(x, y, z), AIR | WATER)
    }

    /// Edits a block; returns every section whose mesh must be rebuilt.
    pub fn set_block(&mut self, x: i32, y: i32, z: i32, id: u8) -> Vec<SecKey> {
        let mut dirty = Vec::new();
        if y < 0 || y >= H as i32 {
            return dirty;
        }
        let (c, l) = Self::split(x, z);
        let Some(ch) = self.chunks.get_mut(&c) else { return dirty };
        Arc::make_mut(ch).set(l.0 as usize, y as usize, l.1 as usize, id); // copy-on-write: streaming jobs keep their snapshot
        let sy = (y / 16) as usize;
        dirty.push((c.0, c.1, sy));
        let mut edge = |cx: i32, cz: i32, sy: usize| dirty.push((cx, cz, sy));
        if l.0 == 0 { edge(c.0 - 1, c.1, sy); }
        if l.0 == 15 { edge(c.0 + 1, c.1, sy); }
        if l.1 == 0 { edge(c.0, c.1 - 1, sy); }
        if l.1 == 15 { edge(c.0, c.1 + 1, sy); }
        if y % 16 == 0 && sy > 0 { edge(c.0, c.1, sy - 1); }
        if y % 16 == 15 && sy + 1 < H / 16 { edge(c.0, c.1, sy + 1); }
        dirty
    }
}
