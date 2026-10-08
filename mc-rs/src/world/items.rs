//! Items, block drops and the hotbar inventory. Ports of `ItemStack`, `InventoryPlayer.addItemStackToInventory`,
//! `Block.dropBlockAsItem` with the `idDropped` / `quantityDropped` / `damageDropped` overrides, and `EntityItem`
//! (20 Hz motion, 5 min lifetime, pickup after 10 ticks). Item ids follow Java: below 256 = the block with that
//! id, from 256 up = `Item.shiftedIndex` (256 + n).

use glam::Vec3;

use crate::world::chunk::is_plant;
use crate::world::dig::TICK;
use crate::world::gen::noise::JavaRandom;
use crate::world::physics::{self, BlockQuery};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ItemStack {
    pub id: u16,
    pub count: u8,
    /// `ItemStack.itemDamage`: only lapis (dye 4) is needed until blocks have metadata.
    pub damage: u8,
}

/// `Item.maxStackSize`: sign, doors and bed 1, snowball 16, everything else 64.
pub fn max_stack(id: u16) -> u8 {
    match id {
        323 | 324 | 330 | 355 => 1,
        332 => 16,
        _ => 64,
    }
}

/// Atlas tile (block id) whose flat colour stands in for an item until M14 has item sprites.
// UNVERIFIED: colours only, picked from the existing atlas.
pub fn tile(id: u16) -> u8 {
    match id {
        0..=255 => id as u8,
        263 | 356 => 7,  // coal, repeater
        264 => 56,       // diamond
        287 | 330 => 4,  // string, iron door
        295 => 31,       // seeds
        296 | 338 => 83, // wheat, sugar cane
        318 => 13,       // flint
        323 | 324 => 5,  // sign, wooden door
        331 => 73,       // redstone
        332 => 78,       // snowball
        337 => 82,       // clay
        348 => 14,       // glowstone dust
        351 => 21,       // lapis dye
        355 => 38,       // bed
        _ => 255,
    }
}

// ---- Block.idDropped / quantityDropped / damageDropped ----
// ponytail: blocks whose drop reads metadata (doors, bed, crops, slabs, stairs) are missing: worldgen does not
// make them and nothing can place them yet. Add them together with block metadata.

fn quantity(b: u8, r: &mut JavaRandom) -> i32 {
    match b {
        // fluids, glass, TNT, bookshelf, fire, spawner, snow layer, ice, portal
        8..=11 | 20 | 46 | 47 | 51 | 52 | 78 | 79 | 90 => 0,
        18 => (r.next_int_bound(20) == 0) as i32, // leaves: a sapling one time in 20
        21 => 4 + r.next_int_bound(5),            // lapis
        73 | 74 => 4 + r.next_int_bound(2),       // redstone ore
        80 | 82 => 4,                             // snow block, clay
        89 => 2 + r.next_int_bound(3),            // glowstone
        _ => 1,
    }
}

/// `idDropped`; 0 or -1 = nothing.
fn id_dropped(b: u8, r: &mut JavaRandom) -> i32 {
    match b {
        1 => 4,                                                       // stone -> cobblestone
        2 | 60 => 3,                                                  // grass, farmland -> dirt
        13 => if r.next_int_bound(10) == 0 { 318 } else { 13 },       // gravel -> flint 1 in 10
        16 => 263,                                                    // coal ore -> coal
        21 => 351,                                                    // lapis ore -> dye
        56 => 264,                                                    // diamond ore -> diamond
        18 => 6,                                                      // leaves -> sapling
        30 => 287,                                                    // web -> string
        31 => if r.next_int_bound(8) == 0 { 295 } else { -1 },        // tall grass -> seeds 1 in 8
        32 => -1,                                                     // dead bush
        52 => 0,                                                      // spawner
        55 | 73 | 74 => 331,                                          // redstone
        61 | 62 => 61,                                                // lit furnace -> furnace
        63 | 68 => 323,                                               // signs
        75 | 76 => 76,                                                // redstone torches
        80 => 332,                                                    // snow block -> snowball
        82 => 337,                                                    // clay
        83 => 338,                                                    // reeds
        89 => 348,                                                    // glowstone -> dust
        _ => b as i32,
    }
}

fn damage_dropped(b: u8) -> u8 {
    if b == 21 { 4 } else { 0 }
}

// ---- Inventory ----

/// Hotbar only: there is no inventory screen yet, so 27 hidden slots would swallow pickups.
/// ponytail: M6 makes this 36 (`InventoryPlayer.mainInventory`); `add` and the HUD already index by slot.
pub const SLOTS: usize = 9;

/// Starts empty, like a new survival player.
#[derive(Default)]
pub struct Inventory {
    pub slots: [Option<ItemStack>; SLOTS],
}

impl Inventory {
    /// `addItemStackToInventory`: top up the first matching non-full stack, else take the first empty slot,
    /// until `s` is empty or nothing fits. `s.count` is what is left; true if any of it was taken.
    pub fn add(&mut self, s: &mut ItemStack) -> bool {
        let start = s.count;
        while s.count > 0 {
            let fits = |o: &Option<ItemStack>| o.is_some_and(|t| t.id == s.id && t.damage == s.damage && t.count < max_stack(t.id));
            let Some(i) = self.slots.iter().position(fits).or_else(|| self.slots.iter().position(Option::is_none)) else { break };
            let t = self.slots[i].get_or_insert(ItemStack { count: 0, ..*s });
            let n = s.count.min(max_stack(s.id) - t.count);
            t.count += n;
            s.count -= n;
        }
        s.count < start
    }

    /// Use up one item of `slot` (`--stackSize`); an emptied stack becomes `None`.
    pub fn consume(&mut self, slot: usize) {
        if let Some(s) = &mut self.slots[slot] {
            s.count -= 1;
            if s.count == 0 {
                self.slots[slot] = None;
            }
        }
    }
}

// ---- Dropped items (EntityItem) ----

/// Half the 0.25 x 0.25 box of `EntityItem.setSize`.
const R: f32 = 0.125;
/// Entities kept at once; the oldest goes first. Vanilla has no cap.
pub const MAX_ITEMS: usize = 256;

pub struct ItemEntity {
    /// Centre of the box, and where it was one tick ago (the renderer interpolates).
    pub pos: Vec3,
    pub prev: Vec3,
    /// Blocks per tick.
    vel: Vec3,
    pub stack: ItemStack,
    pub age: u32,
    delay: u32,
    on_ground: bool,
}

/// Anything an item cannot pass: not air, plants, water or lava. An unloaded chunk reads as solid (`ChunkManager::block`).
fn solid(get: BlockQuery<'_>, c: [i32; 3]) -> bool {
    get(c[0], c[1], c[2]).is_some_and(|b| b != 0 && !is_plant(b) && !(8..=11).contains(&b))
}

fn cell(p: Vec3) -> [i32; 3] {
    [p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32]
}

impl ItemEntity {
    /// One 20 Hz `EntityItem.onUpdate`; false once it is gone (lava, 5 min old, fell out of the world).
    // ponytail: collision tests the box centre line, not all 8 corners, so an item can overhang an edge by
    // up to 0.125; movement per tick is capped under one block so it cannot tunnel. Upgrade: sweep the AABB.
    // Lava deletes the item at once; vanilla bounces it for a few ticks first.
    fn step(&mut self, get: BlockQuery<'_>) -> bool {
        self.delay = self.delay.saturating_sub(1);
        self.prev = self.pos;
        let here = cell(self.pos);
        if matches!(get(here[0], here[1], here[2]), Some(10 | 11)) {
            return false;
        }
        self.vel.y -= 0.04;
        // pushOutOfBlocks (simplified): a block placed on top of it lifts it onto the block.
        if solid(get, here) {
            self.pos.y = here[1] as f32 + 1.0 + R;
        }
        self.on_ground = false;
        for a in [1usize, 0, 2] {
            let d = self.vel[a].clamp(-0.99, 0.99);
            if d == 0.0 {
                continue;
            }
            let mut p = self.pos;
            p[a] += d;
            let mut probe = p;
            probe[a] += R.copysign(d);
            let c = cell(probe);
            if solid(get, c) {
                // Flush against the face, 1e-3 clear of it so the probe sits in the free cell next tick.
                p[a] = if d > 0.0 { c[a] as f32 - R - 1e-3 } else { c[a] as f32 + 1.0 + R + 1e-3 };
                self.on_ground |= a == 1 && d < 0.0;
                self.vel[a] = 0.0;
            }
            self.pos = p;
        }
        // 0.1 * 0.1 * 58.8 = 0.6 * 0.98 = 0.588; ice has slipperiness 0.98.
        let mut f = 0.98;
        if self.on_ground {
            let below = cell(self.pos - Vec3::new(0.0, R + 0.01, 0.0));
            f = if get(below[0], below[1], below[2]) == Some(79) { 0.98 * 0.98 } else { 0.588 };
        }
        self.vel.x *= f;
        self.vel.z *= f;
        self.vel.y *= 0.98;
        self.age += 1;
        self.age < 6000 && self.pos.y > -64.0
    }

    /// `onCollideWithPlayer`: the player's box grown by 1.0 in x and z (not y) touches this item, after the
    /// 10 tick delay. Takes what fits; true when the whole stack is gone. `player` is the centre of its box.
    fn picked_up(&mut self, player: Vec3, inv: &mut Inventory) -> bool {
        let d = (self.pos - player).abs();
        let reach = physics::HALF.x + 1.0 + R;
        if self.delay > 0 || d.x >= reach || d.z >= reach || d.y >= physics::HALF.y + R {
            return false;
        }
        inv.add(&mut self.stack);
        self.stack.count == 0
    }
}

pub struct Drops {
    pub items: Vec<ItemEntity>,
    /// `World.rand`: drawn in the same order as the Java drop code.
    rng: JavaRandom,
    /// Stands in for the unseeded `Math.random()` the `EntityItem` constructor uses for its first motion.
    fx: JavaRandom,
    /// Time left over from the last 20 Hz tick.
    acc: f32,
}

impl Drops {
    pub fn new(seed: i64) -> Self {
        Self { items: Vec::new(), rng: JavaRandom::new(seed), fx: JavaRandom::new(seed ^ 0xF00D), acc: 0.0 }
    }

    /// How far into the next tick we are, 0..1, for drawing between `prev` and `pos`.
    pub fn alpha(&self) -> f32 {
        (self.acc / TICK).min(1.0)
    }

    /// `Block.dropBlockAsItem` for block `block` just broken at `cell`: `quantityDropped` entities of one item
    /// each, placed at random in the middle 0.7 of the cell, hopping up and sideways, 10 ticks before pickup.
    // ponytail: bare hands harvest every block. Vanilla `canHarvestBlock` makes stone, ores etc. drop nothing
    // without the right tool; there are no tools (and no crafting) yet, so that rule would leave the player with
    // no way to ever get cobblestone. Add the check when tools exist (dig.rs already has the by-hand table).
    pub fn spawn_block(&mut self, block: u8, (x, y, z): (i32, i32, i32)) {
        for _ in 0..quantity(block, &mut self.rng) {
            self.rng.next_float(); // `nextFloat() <= chance` with chance 1.0: always true, but it draws
            let id = id_dropped(block, &mut self.rng);
            if id <= 0 {
                continue;
            }
            let mut offset = || ((self.rng.next_float() * 0.7) as f64 + (1.0f32 - 0.7) as f64 * 0.5) as f32;
            let pos = Vec3::new(x as f32 + offset(), y as f32 + offset(), z as f32 + offset());
            let mut jitter = || (self.fx.next_double() * 0.2f32 as f64 - 0.1f32 as f64) as f32;
            let vel = Vec3::new(jitter(), 0.2, jitter());
            if self.items.len() >= MAX_ITEMS {
                self.items.remove(0);
            }
            let stack = ItemStack { id: id as u16, count: 1, damage: damage_dropped(block) };
            self.items.push(ItemEntity { pos, prev: pos, vel, stack, age: 0, delay: 10, on_ground: false });
        }
    }

    /// Advance by `dt` seconds in 20 Hz ticks; items near `player` (centre of its box) go into `inv`.
    pub fn tick(&mut self, dt: f32, get: BlockQuery<'_>, player: Vec3, inv: &mut Inventory) {
        self.acc += dt;
        while self.acc >= TICK {
            self.acc -= TICK;
            self.items.retain_mut(|e| e.step(get) && !e.picked_up(player, inv));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Java drop rules, `addItemStackToInventory`, and an item falling, landing and being picked up.
    #[test]
    fn drops_inventory_and_item_physics() {
        let mut d = Drops::new(1);
        let ids = |d: &Drops| d.items.iter().map(|e| (e.stack.id, e.stack.damage)).collect::<Vec<_>>();
        d.spawn_block(1, (0, 10, 0)); // stone
        d.spawn_block(2, (0, 10, 0)); // grass
        d.spawn_block(79, (0, 10, 0)); // ice: nothing
        d.spawn_block(32, (0, 10, 0)); // dead bush: nothing
        assert_eq!(ids(&d), [(4, 0), (3, 0)]);
        d.spawn_block(21, (0, 10, 0)); // lapis: 4..=8 dye with damage 4
        let n = d.items.len() - 2;
        assert!((4..=8).contains(&n) && ids(&d)[2..].iter().all(|&i| i == (351, 4)));

        // Floor at y <= 9. With the player far away every item comes to rest on top of it.
        let floor = |_: i32, y: i32, _: i32| Some(if y <= 9 { 1 } else { 0 });
        let mut inv = Inventory::default();
        d.tick(2.0, &floor, Vec3::new(100.0, 11.0, 0.0), &mut inv);
        assert!(d.items.iter().all(|e| (e.pos.y - (10.0 + R)).abs() < 0.01 && e.on_ground));
        // The player walks over: everything is picked up, equal items merge into one stack.
        d.tick(0.1, &floor, Vec3::new(0.5, 10.9, 0.5), &mut inv);
        assert!(d.items.is_empty());
        assert_eq!(inv.slots[0], Some(ItemStack { id: 4, count: 1, damage: 0 }));
        assert_eq!(inv.slots[2].map(|s| (s.id, s.count as usize)), Some((351, n)));

        // 70 cobblestone: one full stack and a 6; a full hotbar takes nothing; consume empties a slot.
        let mut inv = Inventory::default();
        let mut s = ItemStack { id: 4, count: 70, damage: 0 };
        assert!(inv.add(&mut s) && s.count == 0);
        assert_eq!((inv.slots[0].unwrap().count, inv.slots[1].unwrap().count), (64, 6));
        for slot in 1..SLOTS {
            inv.slots[slot] = Some(ItemStack { id: 5, count: 64, damage: 0 });
        }
        let mut s = ItemStack { id: 4, count: 1, damage: 0 };
        assert!(inv.add(&mut s) == false && s.count == 1);
        inv.slots[3] = Some(ItemStack { id: 5, count: 1, damage: 0 });
        inv.consume(3);
        assert_eq!(inv.slots[3], None);
    }
}
