//! M6 block updates: `World.scheduleBlockUpdate` / `TickUpdates` (scheduled ticks), the 80 random ticks per chunk of
//! `World.tick`, and the `onBlockAdded` / `onBlockRemoval` / `onNeighborBlockChange` notifications of the `...WithNotify`
//! setters (the `ChunkManager` logs every notifying write, `step` replays the log). The blocks they drive:
//! water and lava (`BlockFlowing`, `BlockStationary`, hardening), sand and gravel (`BlockSand`, `EntityFallingSand`),
//! leaf decay, saplings, plants that lose their ground, grass spread, crops, farmland, reeds and cactus.
//! Fire (`BlockFire`), mushroom spread, snow and ice forming and melting (`World.tick`, `BlockSnow`, `BlockIce`) and rain on
//! farmland are in. Not ported yet: portal from fire (M11), TNT priming from fire, lava mix sound and smoke, falling sand
//! far from loaded chunks (vanilla drops it instantly), pending ticks are not saved (vanilla does not either).

use std::collections::{BTreeSet, HashSet};

use glam::Vec3;

use crate::world::chunk::{light_opacity, normal_cube, opaque};
use crate::world::chunks::ChunkManager;
use crate::world::gen::noise::JavaRandom;
use crate::world::items::{Drops, ItemStack};
use crate::world::pick::{replaceable, torch_stays, BED_DIR};

/// Chunks around the player whose blocks tick at random (`World.tick`'s active set).
const ACTIVE: i32 = 9;
/// `TickUpdates` runs at most this many scheduled entries per game tick.
const MAX_SCHEDULED: usize = 1000;
/// Falling blocks alive at once (the draw buffer holds this many boxes).
pub const MAX_FALLING: usize = 64;

/// `Block.tickOnLoad`: blocks that get random ticks.
pub fn ticks_at_random(id: u8) -> bool {
    matches!(id, 2 | 6 | 8 | 10 | 18 | 31 | 32 | 37..=40 | 51 | 59 | 60 | 78 | 79 | 81 | 83)
}

/// `Material.isSolid` false: air, fluids, plants, fire, web, snow layer, portal and the "circuit" blocks.
pub fn non_solid(id: u8) -> bool {
    matches!(id, 0 | 6 | 8..=11 | 27 | 28 | 30..=32 | 37..=40 | 50 | 51 | 55 | 59 | 66 | 69 | 70 | 72 | 75..=78 | 90)
}

/// `BlockFlowing.blockBlocksFlow`: doors, sign post, ladder, reeds and every solid block.
fn blocks_flow(id: u8) -> bool {
    matches!(id, 63..=65 | 71 | 83) || !non_solid(id)
}

/// `BlockSand.canFallBelow`: air, fire and fluids.
fn can_fall_below(id: u8) -> bool {
    matches!(id, 0 | 8..=11 | 51)
}

fn bid(w: &ChunkManager, x: i32, y: i32, z: i32) -> u8 {
    w.block_loaded(x, y, z).unwrap_or(0)
}

/// A block of sand or gravel that left its cell (`EntityFallingSand`, 0.98 box, centre `pos`).
pub struct Falling {
    pub pos: Vec3,
    pub prev: Vec3,
    vy: f32,
    pub id: u8,
    age: u32,
    /// Primed TNT (`EntityTNTPrimed`, id 46) also drifts in x / z and counts a fuse down; sand and gravel leave both at 0 / -1.
    vx: f32,
    vz: f32,
    pub fuse: i32,
}

pub struct Ticks {
    now: u64,
    seq: u64,
    /// (due tick, insertion number, x, y, z, block id): `scheduledTickTreeSet`; `pending` is `scheduledTickSet`.
    due: BTreeSet<(u64, u64, i32, i32, i32, u8)>,
    pending: HashSet<(i32, i32, i32, u8)>,
    rng: JavaRandom,
    lcg: i32,
    /// `World.func_27161_C`: rain strength above 0.2.
    wet: bool,
    pub falling: Vec<Falling>,
    /// Where a primed TNT's fuse ran out this tick: the caller makes the power 4 explosion (`EntityTNTPrimed.explode`).
    pub booms: Vec<Vec3>,
    /// `Entity.distanceWalkedModified` / `nextStepDistance` of the player and where the last tick left him (x, z).
    walked: f32,
    next_step: i32,
    last_xz: Option<(f32, f32)>,
}

impl Ticks {
    pub fn new(seed: i64) -> Self {
        Self { now: 0, seq: 0, due: BTreeSet::new(), pending: HashSet::new(), rng: JavaRandom::new(seed ^ 0xB10C), lcg: 0x2545_F491, wet: false, falling: Vec::new(), booms: Vec::new(), walked: 0.0, next_step: 1, last_xz: None }
    }

    /// The step trigger of `Entity.moveEntity` for the player (walked distance x 0.6 past `nextStepDistance`, block under the
    /// feet) and `BlockFarmland.onEntityWalking`: a step on farmland turns it to dirt 1 time in 4. Call once per 20 Hz tick with
    /// the feet position. ponytail: mobs and the fence-under-block rule are skipped; a jump over farmland counts like a step.
    pub fn walk(&mut self, w: &mut ChunkManager, x: f32, feet_y: f32, z: f32) {
        let (px, pz) = self.last_xz.replace((x, z)).unwrap_or((x, z));
        self.walked += (x - px).hypot(z - pz).min(1.0) * 0.6; // min: a respawn or bed teleport is not a walk
        let (cx, cy, cz) = (x.floor() as i32, (feet_y - 0.2).floor() as i32, z.floor() as i32);
        let id = bid(w, cx, cy, cz);
        if self.walked > self.next_step as f32 && id > 0 {
            self.next_step += 1;
            if id == 60 && self.rng.next_int_bound(4) == 0 {
                w.set_block(cx, cy, cz, 3);
            }
        }
    }

    /// `new EntityTNTPrimed` at the cell centre: a hop of 0.2 and a drift of 0.02 in a random direction, fuse 80 ticks, or
    /// `rand(20) + 10` when an explosion set it off (`BlockTNT.onBlockDestroyedByExplosion`). Not capped by `MAX_FALLING`.
    pub fn prime(&mut self, (x, y, z): (i32, i32, i32), short: bool) {
        let (s, c) = (self.rng.next_float() * std::f32::consts::TAU * std::f32::consts::PI / 180.0).sin_cos(); // the Java feeds radians to a degree conversion
        let fuse = if short { self.rng.next_int_bound(20) + 10 } else { 80 };
        let pos = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
        self.falling.push(Falling { pos, prev: pos, vy: 0.2, id: 46, age: 0, vx: -s * 0.02, vz: -c * 0.02, fuse });
    }

    /// `World.scheduleBlockUpdate`: the same block at the same cell is queued once.
    fn schedule(&mut self, x: i32, y: i32, z: i32, id: u8, delay: u64) {
        if id > 0 && self.pending.insert((x, y, z, id)) {
            self.seq += 1;
            self.due.insert((self.now + delay, self.seq, x, y, z, id));
        }
    }

    /// One 20 Hz game tick: scheduled ticks, random ticks, block notifications, falling blocks.
    pub fn step(&mut self, w: &mut ChunkManager, drops: &mut Drops, center: (i32, i32), rain: f32) {
        self.now += 1;
        self.wet = rain > 0.2;
        for _ in 0..MAX_SCHEDULED {
            let Some(&e) = self.due.first().filter(|e| e.0 <= self.now) else { break };
            self.due.remove(&e);
            let (_, _, x, y, z, id) = e;
            self.pending.remove(&(x, y, z, id));
            // checkChunksExist(+-8): the cell's chunks (and so the area around it) must be loaded.
            if [(-8, -8), (8, -8), (-8, 8), (8, 8)].iter().all(|&(dx, dz)| w.block_loaded(x + dx, y, z + dz).is_some()) && bid(w, x, y, z) == id {
                self.update(w, drops, x, y, z, id);
            }
            self.notify(w, drops);
        }
        let (blocks, columns) = w.random_ticks(center, ACTIVE, &mut self.lcg, &mut self.rng);
        for (x, z) in columns {
            self.snow_ice(w, x, z);
            self.notify(w, drops);
        }
        for (x, y, z, id) in blocks {
            if bid(w, x, y, z) == id {
                self.update(w, drops, x, y, z, id);
                self.notify(w, drops);
            }
        }
        self.notify(w, drops);
        self.fall(w, drops);
    }

    /// Replays the notifying writes: the new block's `onBlockAdded`, the old one's `onBlockRemoval`, and
    /// `onNeighborBlockChange` for the six neighbours; their own writes are replayed in turn.
    fn notify(&mut self, w: &mut ChunkManager, drops: &mut Drops) {
        loop {
            let changes = w.take_changes();
            if changes.is_empty() {
                return;
            }
            for (x, y, z, old, new) in changes {
                if old != new {
                    self.removed(w, x, y, z, old);
                    self.added(w, x, y, z, new);
                }
                for (dx, dy, dz) in [(-1, 0, 0), (1, 0, 0), (0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1)] {
                    let (nx, ny, nz) = (x + dx, y + dy, z + dz);
                    let id = bid(w, nx, ny, nz);
                    self.neighbour(w, drops, nx, ny, nz, id);
                }
            }
        }
    }

    fn removed(&mut self, w: &mut ChunkManager, x: i32, y: i32, z: i32, id: u8) {
        // BlockLog / BlockLeaves.onBlockRemoval: leaves around it get the "check decay" bit 8.
        let r = match id {
            17 => 4,
            18 => 1,
            _ => return,
        };
        for (dx, dy, dz) in (-r..=r).flat_map(|a| (-r..=r).flat_map(move |b| (-r..=r).map(move |c| (a, b, c)))) {
            let (lx, ly, lz) = (x + dx, y + dy, z + dz);
            if bid(w, lx, ly, lz) == 18 {
                w.set_quiet(lx, ly, lz, 18, w.meta(lx, ly, lz) | 8);
            }
        }
    }

    fn added(&mut self, w: &mut ChunkManager, x: i32, y: i32, z: i32, id: u8) {
        match id {
            8..=11 => {
                harden(w, x, y, z, id);
                if id == 8 || id == 10 {
                    self.schedule(x, y, z, id, tick_rate(id));
                }
            }
            12 | 13 => self.schedule(x, y, z, id, 3),
            // BlockStep.onBlockAdded: a slab on a slab of the same kind becomes one double slab (43) in the lower cell.
            44 if y > 0 && bid(w, x, y - 1, z) == 44 && w.meta(x, y, z) == w.meta(x, y - 1, z) => {
                let m = w.meta(x, y, z);
                w.set_block(x, y, z, 0);
                w.set_block_meta(x, y - 1, z, 43, m);
            }
            // BlockFire.onBlockAdded (ponytail: no portal from obsidian before M11).
            51 if fire_stays(w, x, y, z) => self.schedule(x, y, z, 51, 40),
            51 => {
                w.set_block(x, y, z, 0);
            }
            _ => {}
        }
    }

    fn neighbour(&mut self, w: &mut ChunkManager, drops: &mut Drops, x: i32, y: i32, z: i32, id: u8) {
        match id {
            8..=11 => {
                harden(w, x, y, z, id);
                // BlockStationary: a still fluid wakes up as a flowing one.
                if matches!(bid(w, x, y, z), 9 | 11) {
                    w.set_quiet(x, y, z, id - 1, w.meta(x, y, z));
                    self.schedule(x, y, z, id - 1, tick_rate(id - 1));
                }
            }
            12 | 13 => self.schedule(x, y, z, id, 3),
            6 | 31 | 32 | 37..=40 | 59 | 81 | 83 => {
                check_stay(w, drops, x, y, z, id);
            }
            // BlockTorch.onNeighborBlockChange: the wall or floor it hangs on is gone, so it drops itself.
            50 if !torch_stays(&|a, b, c| bid(w, a, b, c), (x, y, z), w.meta(x, y, z)) => {
                drops.spawn_block(50, 0, (x, y, z));
                w.set_block(x, y, z, 0);
            }
            // BlockFire / BlockSnow.onNeighborBlockChange: no ground (or fuel) left.
            51 if !fire_stays(w, x, y, z) => {
                w.set_block(x, y, z, 0);
            }
            78 if !snow_ok(bid(w, x, y - 1, z)) => {
                w.set_block(x, y, z, 0); // quantityDropped is 0: nothing drops
            }
            64 | 71 => door_neighbour(w, drops, x, y, z, id),
            96 => trap_neighbour(w, drops, x, y, z),
            // BlockSign.onNeighborBlockChange: a post needs solid ground, a wall sign the wall it hangs on; else it drops the sign.
            63 | 68 if !sign_stays(w, x, y, z, id) => {
                drops.spawn_block(id, 0, (x, y, z));
                w.set_block(x, y, z, 0);
            }
            // BlockCake.onNeighborBlockChange: the ground is not solid any more (quantityDropped 0: nothing drops).
            92 if non_solid(bid(w, x, y - 1, z)) => {
                w.set_block(x, y, z, 0);
            }
            26 => bed_neighbour(w, drops, x, y, z),
            // BlockFarmland: something solid on top turns it back to dirt.
            60 if !non_solid(bid(w, x, y + 1, z)) => {
                w.set_block(x, y, z, 3);
            }
            _ => {}
        }
    }

    /// `Block.updateTick` of a scheduled or random tick.
    fn update(&mut self, w: &mut ChunkManager, drops: &mut Drops, x: i32, y: i32, z: i32, id: u8) {
        match id {
            8 | 10 => self.flow(w, drops, x, y, z, id),
            12 | 13 => {
                if can_fall_below(bid(w, x, y - 1, z)) && y >= 0 && self.falling.len() < MAX_FALLING {
                    // EntityFallingSand removes the block on its first update, which is this same tick.
                    w.set_block(x, y, z, 0);
                    let pos = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                    self.falling.push(Falling { pos, prev: pos, vy: 0.0, id, age: 0, vx: 0.0, vz: 0.0, fuse: -1 });
                }
            }
            2 => grass(w, &mut self.rng, x, y, z),
            18 => leaves(w, drops, x, y, z),
            6 | 31 | 32 | 37 | 38 | 59 => {
                // BlockFlower.updateTick drops a plant that cannot stay; saplings and crops then grow.
                if !check_stay(w, drops, x, y, z, id) {
                    return;
                }
                match id {
                    6 => self.sapling(w, x, y, z),
                    59 => crops(w, &mut self.rng, x, y, z),
                    _ => {}
                }
            }
            60 => farmland(w, &mut self.rng, self.wet, x, y, z),
            39 | 40 => mushroom(w, &mut self.rng, x, y, z, id),
            51 => self.fire(w, x, y, z),
            // BlockSnow / BlockIce.updateTick: block light melts them (ice 11 - lightOpacity 3).
            78 if w.block_light(x, y, z) > 11 => {
                w.set_block(x, y, z, 0);
            }
            79 if w.block_light(x, y, z) > 8 => {
                w.set_block(x, y, z, 9);
            }
            81 | 83 => grow_column(w, x, y, z, id),
            _ => {}
        }
    }

    /// `BlockSapling.updateTick` (the light check of the caller is `getBlockLightValue(y + 1)`, with day/night).
    fn sapling(&mut self, w: &mut ChunkManager, x: i32, y: i32, z: i32) {
        if w.light_level(x, y + 1, z) >= 9 && self.rng.next_int_bound(30) == 0 {
            let m = w.meta(x, y, z);
            if m & 8 == 0 {
                w.set_block_meta(x, y, z, 6, m | 8);
            } else {
                w.grow_tree(x, y, z, m & 3, &mut self.rng);
            }
        }
    }

    /// `BlockFlowing.updateTick`.
    fn flow(&mut self, w: &mut ChunkManager, drops: &mut Drops, x: i32, y: i32, z: i32, id: u8) {
        let (lava, step) = (id == 10, if id == 10 { 2 } else { 1 }); // overworld: lava creeps 2 levels per cell
        let mut d = decay(w, x, y, z, id);
        let mut settle = true;
        if d > 0 {
            let (mut min, mut adjacent) = (-100, 0);
            for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                let v = decay(w, x + dx, y, z + dz, id);
                if v < 0 {
                    continue;
                }
                adjacent += (v == 0) as i32;
                let v = if v >= 8 { 0 } else { v };
                if !(min >= 0 && v >= min) {
                    min = v;
                }
            }
            let mut nd = min + step;
            if nd >= 8 || min < 0 {
                nd = -1;
            }
            let above = decay(w, x, y + 1, z, id);
            if above >= 0 {
                nd = if above >= 8 { above } else { above + 8 };
            }
            // Two source neighbours make a new source (water only); vanilla's second condition reads this cell's own
            // metadata, which is > 0 here, so it never holds.
            if adjacent >= 2 && !lava && !non_solid(bid(w, x, y - 1, z)) {
                nd = 0;
            }
            if lava && d < 8 && nd < 8 && nd > d && self.rng.next_int_bound(4) != 0 {
                nd = d;
                settle = false;
            }
            if nd != d {
                d = nd;
                if nd < 0 {
                    w.set_block(x, y, z, 0);
                } else {
                    w.set_block_meta(x, y, z, id, nd as u8);
                    self.schedule(x, y, z, id, tick_rate(id));
                }
            } else if settle {
                w.set_quiet(x, y, z, id + 1, d as u8);
            }
        } else {
            w.set_quiet(x, y, z, id + 1, 0);
        }
        let below = bid(w, x, y - 1, z);
        if can_displace(w, x, y - 1, z, id) {
            let meta = if d >= 8 { d } else { d + 8 };
            w.set_block_meta(x, y - 1, z, id, meta as u8);
        } else if d >= 0 && (d == 0 || blocks_flow(below)) {
            let dirs = optimal_directions(w, x, y, z, id);
            let nd = if d >= 8 { 1 } else { d + step };
            if nd >= 8 {
                return;
            }
            for (i, (dx, dz)) in [(-1, 0), (1, 0), (0, -1), (0, 1)].into_iter().enumerate() {
                if dirs[i] && can_displace(w, x + dx, y, z + dz, id) {
                    let old = bid(w, x + dx, y, z + dz);
                    if old > 0 && !lava {
                        drops.spawn_block(old, w.meta(x + dx, y, z + dz), (x + dx, y, z + dz));
                    }
                    w.set_block_meta(x + dx, y, z + dz, id, nd as u8);
                }
            }
        }
    }

    /// Falling blocks, one game tick: gravity 0.04, drag 0.98; a block lands on the first cell below that is not
    /// air/fluid/plant, and drops as an item when its cell is taken.
    fn fall(&mut self, w: &mut ChunkManager, drops: &mut Drops) {
        let mut i = 0;
        while i < self.falling.len() {
            let f = &mut self.falling[i];
            if f.id == 46 {
                // `EntityTNTPrimed.onUpdate`: gravity 0.04, drag 0.98, x / z x 0.7 on the ground, then the fuse.
                // ponytail: the 0.98 box is probed as a point at its leading face (centre height for x / z), so a TNT can hang a hair over an edge.
                f.prev = f.pos;
                f.vy -= 0.04;
                let solid = |p: Vec3| !non_solid(bid(w, p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32));
                let ny = f.pos.y + f.vy;
                let ground = f.vy < 0.0 && solid(Vec3::new(f.pos.x, ny - 0.49, f.pos.z));
                if ground {
                    f.pos.y = (ny - 0.49).floor() + 1.49;
                    f.vy = 0.0;
                } else if f.vy > 0.0 && solid(Vec3::new(f.pos.x, ny + 0.49, f.pos.z)) {
                    f.vy = 0.0;
                } else {
                    f.pos.y = ny;
                }
                let nx = f.pos.x + f.vx;
                if solid(Vec3::new(nx + 0.49 * f.vx.signum(), f.pos.y, f.pos.z)) { f.vx = 0.0 } else { f.pos.x = nx }
                let nz = f.pos.z + f.vz;
                if solid(Vec3::new(f.pos.x, f.pos.y, nz + 0.49 * f.vz.signum())) { f.vz = 0.0 } else { f.pos.z = nz }
                (f.vx, f.vy, f.vz) = (f.vx * 0.98, f.vy * 0.98, f.vz * 0.98);
                if ground {
                    (f.vx, f.vz) = (f.vx * 0.7, f.vz * 0.7);
                }
                let boom = f.fuse <= 0;
                f.fuse -= 1;
                if boom {
                    self.booms.push(f.pos);
                    self.falling.swap_remove(i);
                } else {
                    i += 1;
                }
                continue;
            }
            f.age += 1;
            f.prev = f.pos;
            f.vy -= 0.04;
            let ny = f.pos.y + f.vy;
            let (x, z) = (f.pos.x.floor() as i32, f.pos.z.floor() as i32);
            let ground = (ny - 0.49).floor() as i32;
            if ground >= 0 && f.age <= 100 && replaceable(bid(w, x, ground, z)) {
                f.pos.y = ny;
                f.vy *= 0.98;
                i += 1;
                continue;
            }
            let f = self.falling.swap_remove(i);
            let y = (ground + 1).max(0);
            if ground >= -1 && f.age <= 100 && replaceable(bid(w, x, y, z)) && !can_fall_below(bid(w, x, y - 1, z)) {
                w.set_block(x, y, z, f.id);
            } else {
                drops.spawn_stack(ItemStack { id: f.id as u16, count: 1, damage: 0 }, (x, y, z));
            }
        }
    }
}

/// `BlockFluid.tickRate` (water 5, lava 30 in the overworld).
fn tick_rate(id: u8) -> u64 {
    if id < 10 { 5 } else { 30 }
}

/// `BlockFlowing.getFlowDecay`: the fluid level at the cell, -1 when it is not the same fluid (water 8/9, lava 10/11).
fn decay(w: &ChunkManager, x: i32, y: i32, z: i32, id: u8) -> i32 {
    let b = bid(w, x, y, z);
    if (8..=11).contains(&b) && b / 2 == id / 2 { w.meta(x, y, z) as i32 } else { -1 }
}

fn is_source(w: &ChunkManager, x: i32, y: i32, z: i32, id: u8) -> bool {
    decay(w, x, y, z, id) == 0
}

/// `BlockFlowing.liquidCanDisplaceBlock`: not the same fluid, not lava, and not something solid.
fn can_displace(w: &ChunkManager, x: i32, y: i32, z: i32, id: u8) -> bool {
    let b = bid(w, x, y, z);
    if (8..=11).contains(&b) { b / 2 != id / 2 && b < 10 } else { !blocks_flow(b) }
}

/// `BlockFlowing.calculateFlowCost`: steps to the nearest drop within 4 cells (depth-first, not going back).
fn flow_cost(w: &ChunkManager, x: i32, y: i32, z: i32, depth: i32, from: usize, id: u8) -> i32 {
    let mut best = 1000;
    for (dir, (dx, dz)) in [(-1, 0), (1, 0), (0, -1), (0, 1)].into_iter().enumerate() {
        if dir == from ^ 1 {
            continue;
        }
        let (nx, nz) = (x + dx, z + dz);
        if !blocks_flow(bid(w, nx, y, nz)) && !is_source(w, nx, y, nz, id) {
            if !blocks_flow(bid(w, nx, y - 1, nz)) {
                return depth;
            }
            if depth < 4 {
                best = best.min(flow_cost(w, nx, y, nz, depth + 1, dir, id));
            }
        }
    }
    best
}

/// `BlockFlowing.getOptimalFlowDirections`: -x, +x, -z, +z flagged when they lead to the nearest drop.
fn optimal_directions(w: &ChunkManager, x: i32, y: i32, z: i32, id: u8) -> [bool; 4] {
    let mut cost = [1000; 4];
    for (dir, (dx, dz)) in [(-1, 0), (1, 0), (0, -1), (0, 1)].into_iter().enumerate() {
        let (nx, nz) = (x + dx, z + dz);
        if !blocks_flow(bid(w, nx, y, nz)) && !is_source(w, nx, y, nz, id) {
            cost[dir] = if !blocks_flow(bid(w, nx, y - 1, nz)) { 0 } else { flow_cost(w, nx, y, nz, 1, dir, id) };
        }
    }
    let min = *cost.iter().min().unwrap();
    cost.map(|c| c == min)
}

/// `BlockFluid.checkForHarden`: lava next to water (sides or above) becomes obsidian (source) or cobblestone (flow <= 4).
fn harden(w: &mut ChunkManager, x: i32, y: i32, z: i32, id: u8) {
    if id < 10 || bid(w, x, y, z) != id {
        return;
    }
    let wet = [(0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0), (0, 1, 0)].iter().any(|&(dx, dy, dz)| matches!(bid(w, x + dx, y + dy, z + dz), 8 | 9));
    if wet {
        match w.meta(x, y, z) {
            0 => w.set_block(x, y, z, 49),
            1..=4 => w.set_block(x, y, z, 4),
            _ => false,
        };
    }
}

/// `BlockFlower` / mushroom / reed / cactus `canBlockStay`.
fn can_stay(w: &ChunkManager, x: i32, y: i32, z: i32, id: u8) -> bool {
    stay(w, x, y, z, id, false)
}

/// `canPlaceBlockAt` of the same plants: the ground rule without the light rule (the cactus keeps its neighbour test).
pub fn can_place(w: &ChunkManager, x: i32, y: i32, z: i32, id: u8) -> bool {
    stay(w, x, y, z, id, true)
}

fn stay(w: &ChunkManager, x: i32, y: i32, z: i32, id: u8, placing: bool) -> bool {
    let below = bid(w, x, y - 1, z);
    let lit = || placing || w.light_at(x, y, z, 0) >= 8 || w.sees_sky(x, y, z);
    match id {
        6 | 31 | 37 | 38 => lit() && matches!(below, 2 | 3 | 60),
        32 => lit() && below == 12,
        59 => lit() && below == 60,
        // BlockMushroom.canBlockStay: dark enough, on an opaque cube (UNVERIFIED: `opaqueCubeLookup` is `light_opacity == 255` here).
        39 | 40 => (0..128).contains(&y) && (placing || w.light_at(x, y, z, 0) < 13) && light_opacity(below) == 255 && !matches!(below, 10 | 11),
        83 => below == 83 || (matches!(below, 2 | 3) && [(-1, 0), (1, 0), (0, -1), (0, 1)].iter().any(|&(dx, dz)| matches!(bid(w, x + dx, y - 1, z + dz), 8 | 9))),
        81 => [(-1, 0), (1, 0), (0, -1), (0, 1)].iter().all(|&(dx, dz)| non_solid(bid(w, x + dx, y, z + dz))) && matches!(below, 12 | 81),
        _ => true,
    }
}

/// `checkFlowerChange` and friends: a plant that cannot stay drops its items and goes. True when it stayed.
fn check_stay(w: &mut ChunkManager, drops: &mut Drops, x: i32, y: i32, z: i32, id: u8) -> bool {
    if can_stay(w, x, y, z, id) {
        return true;
    }
    drops.spawn_block(id, w.meta(x, y, z), (x, y, z));
    w.set_block(x, y, z, 0);
    false
}

/// `BlockGrass.updateTick`: smothered grass dies (one in four), lit grass spreads to dirt nearby.
fn grass(w: &mut ChunkManager, rng: &mut JavaRandom, x: i32, y: i32, z: i32) {
    if w.light_level(x, y + 1, z) < 4 && light_opacity(bid(w, x, y + 1, z)) > 2 {
        if rng.next_int_bound(4) == 0 {
            w.set_block(x, y, z, 3);
        }
    } else if w.light_level(x, y + 1, z) >= 9 {
        let (tx, ty, tz) = (x + rng.next_int_bound(3) - 1, y + rng.next_int_bound(5) - 3, z + rng.next_int_bound(3) - 1);
        if bid(w, tx, ty, tz) == 3 && w.light_level(tx, ty + 1, tz) >= 4 && light_opacity(bid(w, tx, ty + 1, tz)) <= 2 {
            w.set_block(tx, ty, tz, 2);
        }
    }
}

/// `BlockLeaves.updateTick`: leaves marked with bit 8 stay only within 4 steps (through leaves) of a log.
fn leaves(w: &mut ChunkManager, drops: &mut Drops, x: i32, y: i32, z: i32) {
    let m = w.meta(x, y, z);
    if m & 8 == 0 {
        return;
    }
    // dist: 0 log, -2 leaves not reached yet, -1 anything else; 9x9x9 around the cell.
    let mut dist = [[[-1i8; 9]; 9]; 9];
    for (a, plane) in dist.iter_mut().enumerate() {
        for (b, row) in plane.iter_mut().enumerate() {
            for (c, d) in row.iter_mut().enumerate() {
                match w.block_loaded(x + a as i32 - 4, y + b as i32 - 4, z + c as i32 - 4) {
                    Some(17) => *d = 0,
                    Some(18) => *d = -2,
                    Some(_) => {}
                    None => return, // checkChunksExist failed
                }
            }
        }
    }
    for step in 1..=4 {
        for a in 0..9usize {
            for b in 0..9usize {
                for c in 0..9usize {
                    if dist[a][b][c] != step - 1 {
                        continue;
                    }
                    for (da, db, dc) in [(-1, 0, 0), (1, 0, 0), (0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1)] {
                        let (na, nb, nc) = (a as i32 + da, b as i32 + db, c as i32 + dc);
                        if [na, nb, nc].iter().all(|v| (0..9).contains(v)) && dist[na as usize][nb as usize][nc as usize] == -2 {
                            dist[na as usize][nb as usize][nc as usize] = step;
                        }
                    }
                }
            }
        }
    }
    if dist[4][4][4] >= 0 {
        w.set_quiet(x, y, z, 18, m & !8);
    } else {
        drops.spawn_block(18, m, (x, y, z));
        w.set_block(x, y, z, 0);
    }
}

/// `BlockCrops.updateTick` growth with `getGrowthRate`.
fn crops(w: &mut ChunkManager, rng: &mut JavaRandom, x: i32, y: i32, z: i32) {
    let m = w.meta(x, y, z);
    if m >= 7 || w.light_level(x, y + 1, z) < 9 {
        return;
    }
    let is = |dx: i32, dz: i32| bid(w, x + dx, y, z + dz) == 59;
    let (row, col) = (is(-1, 0) || is(1, 0), is(0, -1) || is(0, 1));
    let diagonal = is(-1, -1) || is(1, -1) || is(1, 1) || is(-1, 1);
    let mut rate = 1.0f32;
    for (dx, dz) in (-1..=1).flat_map(|a| (-1..=1).map(move |b| (a, b))) {
        let mut v = 0.0;
        if bid(w, x + dx, y - 1, z + dz) == 60 {
            v = if w.meta(x + dx, y - 1, z + dz) > 0 { 3.0 } else { 1.0 };
        }
        rate += if (dx, dz) != (0, 0) { v / 4.0 } else { v };
    }
    if diagonal || (row && col) {
        rate /= 2.0;
    }
    if rng.next_int_bound((100.0 / rate) as i32) == 0 {
        w.set_block_meta(x, y, z, 59, m + 1);
    }
}

/// `BlockFarmland.updateTick`: wet when water is within 4 cells (or it rains), else it dries out, then reverts to dirt
/// unless a crop grows on it.
fn farmland(w: &mut ChunkManager, rng: &mut JavaRandom, wet: bool, x: i32, y: i32, z: i32) {
    if rng.next_int_bound(5) != 0 {
        return;
    }
    let water = rained_on(w, wet, x, y + 1, z) || (-4..=4).any(|dx| (-4..=4).any(|dz| (y..=y + 1).any(|dy| matches!(bid(w, x + dx, dy, z + dz), 8 | 9))));
    let m = w.meta(x, y, z);
    if water {
        w.set_block_meta(x, y, z, 60, 7);
    } else if m > 0 {
        w.set_block_meta(x, y, z, 60, m - 1);
    } else if bid(w, x, y + 1, z) != 59 {
        w.set_block(x, y, z, 3);
    }
}

/// `BlockReed` / `BlockCactus.updateTick`: stacks grow up to 3 high, one cell per 16 ticks of the top block.
fn grow_column(w: &mut ChunkManager, x: i32, y: i32, z: i32, id: u8) {
    if bid(w, x, y + 1, z) != 0 || w.block_loaded(x, y + 1, z).is_none() {
        return;
    }
    let height = (1..).take_while(|&h| bid(w, x, y - h, z) == id).count() + 1;
    if height < 3 {
        let m = w.meta(x, y, z);
        if m == 15 {
            w.set_block(x, y + 1, z, id);
            w.set_block_meta(x, y, z, id, 0);
        } else {
            w.set_block_meta(x, y, z, id, m + 1);
        }
    }
}

/// `BlockBed.onNeighborBlockChange`: a half whose partner is gone breaks; only the half without bit 8 drops the bed.
fn bed_neighbour(w: &mut ChunkManager, drops: &mut Drops, x: i32, y: i32, z: i32) {
    let m = w.meta(x, y, z);
    let (dx, dz) = BED_DIR[(m & 3) as usize];
    if m & 8 != 0 {
        if bid(w, x - dx, y, z - dz) != 26 {
            w.set_block(x, y, z, 0);
        }
    } else if bid(w, x + dx, y, z + dz) != 26 {
        w.set_block(x, y, z, 0);
        drops.spawn_block(26, m, (x, y, z));
    }
}

/// `BlockDoor.onNeighborBlockChange` (without redstone, step 5): a half whose partner or floor is gone breaks; the lower half drops the door.
fn door_neighbour(w: &mut ChunkManager, drops: &mut Drops, x: i32, y: i32, z: i32, id: u8) {
    let m = w.meta(x, y, z);
    if m & 8 != 0 {
        if bid(w, x, y - 1, z) != id {
            w.set_block(x, y, z, 0);
        }
        return;
    }
    let mut gone = false;
    if bid(w, x, y + 1, z) != id {
        w.set_block(x, y, z, 0);
        gone = true;
    }
    if !normal_cube(bid(w, x, y - 1, z)) {
        w.set_block(x, y, z, 0);
        gone = true;
        if bid(w, x, y + 1, z) == id {
            w.set_block(x, y + 1, z, 0);
        }
    }
    if gone {
        drops.spawn_block(id, m, (x, y, z));
    }
}

/// `BlockSign.onNeighborBlockChange`: the cell below a post, or the wall a wall sign's metadata names (2 -> +Z, 3 -> -Z, 4 -> +X,
/// 5 -> -X), must be solid.
fn sign_stays(w: &ChunkManager, x: i32, y: i32, z: i32, id: u8) -> bool {
    if id == 63 {
        return !non_solid(bid(w, x, y - 1, z));
    }
    match w.meta(x, y, z) {
        m @ 2..=5 => {
            let (dx, dz) = [(0, 1), (0, -1), (1, 0), (-1, 0)][m as usize - 2];
            !non_solid(bid(w, x + dx, y, z + dz))
        }
        _ => false,
    }
}

/// `BlockTrapDoor.onNeighborBlockChange` (no redstone yet): the wall it hangs on is not a normal cube any more, so it drops itself.
fn trap_neighbour(w: &mut ChunkManager, drops: &mut Drops, x: i32, y: i32, z: i32) {
    let m = w.meta(x, y, z);
    let (dx, dz) = [(0, 1), (0, -1), (1, 0), (-1, 0)][(m & 3) as usize];
    if !normal_cube(bid(w, x + dx, y, z + dz)) {
        w.set_block(x, y, z, 0);
        drops.spawn_block(96, m, (x, y, z));
    }
}

/// `BlockFire.setBurnRate`: (chance to encourage fire, ability to catch fire) of a block id.
fn burn_rate(id: u8) -> (i32, i32) {
    match id {
        5 | 53 | 85 => (5, 20), // planks, wooden stairs, fence
        17 => (5, 5),
        18 | 35 => (30, 60), // leaves, wool
        47 => (30, 20),
        46 => (15, 100),
        31 => (60, 100),
        _ => (0, 0),
    }
}

const SIDES: [(i32, i32, i32); 6] = [(1, 0, 0), (-1, 0, 0), (0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1)];

/// `BlockFire.func_263_h`: some neighbour burns.
fn next_to_fuel(w: &ChunkManager, x: i32, y: i32, z: i32) -> bool {
    SIDES.iter().any(|&(a, b, c)| burn_rate(bid(w, x + a, y + b, z + c)).0 > 0)
}

/// `BlockFire.canPlaceBlockAt`: a solid floor or a burning neighbour.
fn fire_stays(w: &ChunkManager, x: i32, y: i32, z: i32) -> bool {
    normal_cube(bid(w, x, y - 1, z)) || next_to_fuel(w, x, y, z)
}

/// `BlockSnow.canPlaceBlockAt`: an opaque, solid floor (UNVERIFIED: glass, leaves and ice are told apart by hand).
fn snow_ok(below: u8) -> bool {
    below != 0 && opaque(below) && !matches!(below, 18 | 20 | 79) && !non_solid(below)
}

/// `World.canBlockBeRainedOn` (`wet` = rain above 0.2): open sky, nothing solid above, a biome that rains.
fn rained_on(w: &mut ChunkManager, wet: bool, x: i32, y: i32, z: i32) -> bool {
    wet && w.sees_sky(x, y, z) && w.top_solid(x, z) <= y && w.biomes(x, z, 1, 1).first().is_some_and(|b| b.rains())
}

/// `BlockMushroom.updateTick`: one tick in a hundred it seeds a random nearby cell (the Java also draws two unused offsets).
fn mushroom(w: &mut ChunkManager, rng: &mut JavaRandom, x: i32, y: i32, z: i32, id: u8) {
    if rng.next_int_bound(100) != 0 {
        return;
    }
    let (tx, ty, tz) = (x + rng.next_int_bound(3) - 1, y + rng.next_int_bound(2) - rng.next_int_bound(2), z + rng.next_int_bound(3) - 1);
    if bid(w, tx, ty, tz) == 0 && can_stay(w, tx, ty, tz, id) {
        rng.next_int_bound(3);
        rng.next_int_bound(3);
        w.set_block(tx, ty, tz, id);
    }
}

impl Ticks {
    /// `BlockFire.tryToCatchBlockOnFire`. burning TNT is primed (fuse 80).
    fn catch_fire(&mut self, w: &mut ChunkManager, x: i32, y: i32, z: i32, chance: i32, age: i32) {
        if self.rng.next_int_bound(chance) < burn_rate(bid(w, x, y, z)).1 {
            let tnt = bid(w, x, y, z) == 46;
            if self.rng.next_int_bound(age + 10) < 5 && !rained_on(w, self.wet, x, y, z) {
                let m = (age + self.rng.next_int_bound(5) / 4).min(15);
                w.set_block_meta(x, y, z, 51, m as u8);
            } else {
                w.set_block(x, y, z, 0);
            }
            if tnt {
                self.prime((x, y, z), false);
            }
        }
    }

    /// `BlockFire.updateTick` (tick rate 40). The Java keeps going after it removes the fire; every later branch would
    /// remove it again or does nothing, so this returns.
    fn fire(&mut self, w: &mut ChunkManager, x: i32, y: i32, z: i32) {
        let (below, nether) = (bid(w, x, y - 1, z), bid(w, x, y - 1, z) == 87);
        let drenched = !nether && [(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1)].iter().any(|&(a, b)| rained_on(w, self.wet, x + a, y, z + b));
        if !fire_stays(w, x, y, z) || drenched {
            w.set_block(x, y, z, 0);
            return;
        }
        let age = w.meta(x, y, z) as i32;
        if age < 15 {
            w.set_quiet(x, y, z, 51, (age + self.rng.next_int_bound(3) / 2) as u8);
        }
        self.schedule(x, y, z, 51, 40);
        if !nether && !next_to_fuel(w, x, y, z) {
            if !normal_cube(below) || age > 3 {
                w.set_block(x, y, z, 0);
            }
        } else if !nether && burn_rate(below).0 == 0 && age == 15 && self.rng.next_int_bound(4) == 0 {
            w.set_block(x, y, z, 0);
        } else {
            for (i, (a, b, c)) in [(1, 0, 0), (-1, 0, 0), (0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1)].into_iter().enumerate() {
                self.catch_fire(w, x + a, y + b, z + c, if matches!(i, 2 | 3) { 250 } else { 300 }, age);
            }
            for (a, b, c) in (x - 1..=x + 1).flat_map(|a| (z - 1..=z + 1).flat_map(move |b| (y - 1..=y + 4).map(move |c| (a, b, c)))) {
                // here (a, c, b) is the cell and `b` its z, as in the Java's (var8, var10, var9)
                if (a, c, b) == (x, y, z) {
                    continue;
                }
                let chance = 100 + if c > y + 1 { (c - (y + 1)) * 100 } else { 0 };
                let encourage = if bid(w, a, c, b) == 0 { SIDES.iter().map(|&(i, j, k)| burn_rate(bid(w, a + i, c + j, b + k)).0).max().unwrap_or(0) } else { 0 };
                if encourage > 0 {
                    let n = (encourage + 40) / (age + 30);
                    // The Java's second rain test reads (var8 - 1, var10, var4): the fire's own z, kept as is.
                    if n > 0
                        && self.rng.next_int_bound(chance) <= n
                        && !rained_on(w, self.wet, a, c, b)
                        && !rained_on(w, self.wet, a - 1, c, z)
                        && !rained_on(w, self.wet, a + 1, c, b)
                        && !rained_on(w, self.wet, a, c, b - 1)
                        && !rained_on(w, self.wet, a, c, b + 1)
                    {
                        let m = (age + self.rng.next_int_bound(5) / 4).min(15);
                        w.set_block_meta(a, c, b, 51, m as u8);
                    }
                }
            }
        }
    }

    /// The snow and ice part of `World.tick` for one column: snow on the top of a snowy biome while it rains, still water
    /// that is open to the air freezes (both only where block light is under 10).
    fn snow_ice(&mut self, w: &mut ChunkManager, x: i32, z: i32) {
        let y = w.top_solid(x, z);
        if !(0..128).contains(&y) || w.block_light(x, y, z) >= 10 || !w.biomes(x, z, 1, 1).first().is_some_and(|b| b.snows()) {
            return;
        }
        let below = bid(w, x, y - 1, z);
        if self.wet && bid(w, x, y, z) == 0 && snow_ok(below) {
            w.set_block(x, y, z, 78);
        }
        if below == 9 && w.meta(x, y - 1, z) == 0 {
            w.set_block(x, y - 1, z, 79);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burn_table_and_snow_floor() {
        assert_eq!(burn_rate(5), (5, 20));
        assert_eq!(burn_rate(18), (30, 60));
        assert_eq!(burn_rate(1), (0, 0));
        assert!(snow_ok(1) && snow_ok(2) && !snow_ok(0) && !snow_ok(79) && !snow_ok(18) && !snow_ok(8));
        assert!(non_solid(51) && ticks_at_random(51) && ticks_at_random(39) && ticks_at_random(79));
    }

    #[test]
    fn farmland_is_trampled_by_steps_not_by_standing_and_plants_need_their_ground() {
        let mut m = ChunkManager::new(1, 1);
        m.insert((0, 0), vec![0u8; crate::world::chunk::VOLUME]);
        for x in 0..16 {
            m.set_block(x, 40, 5, 60);
        }
        m.set_block(2, 39, 8, 12); // sand
        m.set_block(2, 39, 9, 3); // dirt
        let mut t = Ticks::new(1);
        for _ in 0..100 {
            t.walk(&mut m, 3.5, 41.0, 5.5); // standing still: no step
        }
        assert_eq!(m.block(3, 40, 5), Some(60));
        for i in 0..400 {
            t.walk(&mut m, 0.5 + (i % 30) as f32 * 0.5, 41.0, 5.5); // pacing along the row
        }
        let dirt = (0..16).filter(|&x| m.block(x, 40, 5) == Some(3)).count();
        assert!(dirt > 0 && (0..16).all(|x| matches!(m.block(x, 40, 5), Some(60 | 3))));
        assert!(can_place(&m, 2, 40, 9, 37) && !can_place(&m, 2, 40, 8, 37) && can_place(&m, 2, 40, 8, 32));
        assert!(can_place(&m, 2, 40, 8, 81) && !can_place(&m, 2, 40, 9, 81), "cactus: sand and open sides");
        assert!(can_place(&m, 2, 40, 9, 39) && !can_place(&m, 2, 41, 9, 39), "a mushroom wants an opaque block under it, in any light");
    }

    #[test]
    fn primed_tnt_rests_then_booms_on_its_fuse() {
        let mut m = ChunkManager::new(1, 1);
        m.insert((0, 0), vec![0u8; crate::world::chunk::VOLUME]);
        (0..16).for_each(|x| (0..16).for_each(|z| { m.set_block(x, 39, z, 1); }));
        let (mut t, mut d) = (Ticks::new(1), Drops::new(1));
        t.prime((3, 40, 5), false);
        for _ in 0..80 {
            t.fall(&mut m, &mut d);
        }
        assert!(t.booms.is_empty() && t.falling.len() == 1, "the fuse is 80: it goes off on the 81st tick");
        let p = t.falling[0].pos;
        assert!((p.y - 40.49).abs() < 0.01 && (p.x - 3.5).abs() < 1.5, "rests on the floor, drifts a little");
        t.fall(&mut m, &mut d);
        assert!(t.booms.len() == 1 && t.falling.is_empty());
        t.prime((8, 40, 8), true);
        let n = (1..40).find(|_| { t.fall(&mut m, &mut d); t.falling.is_empty() }).unwrap();
        assert!((10..=30).contains(&n), "short fuse is rand(20) + 10, got {n}");
    }

    #[test]
    fn slab_on_slab_merges_only_when_the_kinds_match() {
        let mut m = ChunkManager::new(1, 1);
        m.insert((0, 0), vec![0u8; crate::world::chunk::VOLUME]);
        let mut t = Ticks::new(1);
        m.set_block_meta(3, 40, 5, 44, 2);
        m.set_block_meta(3, 41, 5, 44, 3); // stone-kind below, wood above: stays two slabs
        t.added(&mut m, 3, 41, 5, 44);
        assert_eq!((m.block(3, 41, 5), m.block(3, 40, 5)), (Some(44), Some(44)));
        m.set_block_meta(3, 41, 5, 44, 2);
        t.added(&mut m, 3, 41, 5, 44);
        assert_eq!((m.block(3, 41, 5), m.block(3, 40, 5), m.meta(3, 40, 5)), (Some(0), Some(43), 2));
    }
}
