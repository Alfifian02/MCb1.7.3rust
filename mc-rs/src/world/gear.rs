//! Bucket, bow, fishing rod, shears and dye on mobs: the item uses of `ItemBucket`, `ItemBow`, `ItemFishingRod` +
//! `EntityFish`, `EntitySheep.interact` and `ItemDye.saddleEntity`. Pure rules: `lib.rs` does the world edits.

use glam::Vec3;

use crate::world::craft::SHEARS;
use crate::world::gen::noise::JavaRandom;
use crate::world::items::ItemStack;
use crate::world::mobs::{Kind, Mob};
use crate::world::physics::BlockQuery;

pub const BUCKET: u16 = 325;
pub const WATER_BUCKET: u16 = 326;
pub const LAVA_BUCKET: u16 = 327;
pub const MILK_BUCKET: u16 = 335;
pub const BOW: u16 = 261;
pub const ARROW: u16 = 262;
pub const SNOWBALL: u16 = 332;
pub const EGG: u16 = 344;
pub const ROD: u16 = 346;
const RAW_FISH: u16 = 349;
/// `EntityArrow`: launch speed in blocks per tick (`setArrowHeading(.., 1.5F, 1.0F)`), damage on a mob.
pub const ARROW_SPEED: f32 = 1.5;
pub const ARROW_DAMAGE: i32 = 4;

/// `ItemBucket.onItemRightClick` with an empty bucket: only a water or lava source (metadata 0) fills it.
pub fn bucket_fill(block: u8, meta: u8) -> Option<u16> {
    match (block, meta) {
        (8 | 9, 0) => Some(WATER_BUCKET),
        (10 | 11, 0) => Some(LAVA_BUCKET),
        _ => None,
    }
}

/// The flowing block (`Block.waterMoving` 8, `lavaMoving` 10) a full bucket pours; `None` for an empty or milk bucket.
pub fn bucket_pour(held: u16) -> Option<u8> {
    match held {
        WATER_BUCKET => Some(8),
        LAVA_BUCKET => Some(10),
        _ => None,
    }
}

/// `ItemBow.onItemRightClick`: the arrow's start velocity along the look direction. ponytail: vanilla adds gaussian
/// spread (0.0075 per axis); `rng` gives it here, so a golden test needs the Java seed.
pub fn arrow_velocity(forward: Vec3, rng: &mut JavaRandom) -> Vec3 {
    let mut g = || rng.next_gaussian() as f32 * 0.0075;
    (forward.normalize() + Vec3::new(g(), g(), g())) * ARROW_SPEED
}

/// What `use_on_mob` did to the held stack.
#[derive(Debug, PartialEq)]
pub enum Used {
    /// Shears: one use of wear; the wool to drop.
    Sheared(Vec<ItemStack>),
    /// Dye: one item used up.
    Dyed,
    /// Empty bucket on a cow: becomes a milk bucket.
    Milked,
}

/// `EntitySheep.interact` (shears: 2 + `nextInt(3)` wool of the fleece colour, then bare for good: b1.7.3 has no regrowth),
/// `ItemDye.saddleEntity` (an unsheared sheep takes the dye's colour `~damage & 15`, unless it already has it) and
/// `EntityCow.interact` (bucket -> milk). `None` = nothing happened (the tap attacks instead).
pub fn use_on_mob(m: &mut Mob, held: ItemStack, rng: &mut JavaRandom) -> Option<Used> {
    match (m.kind, held.id) {
        (Kind::Sheep, SHEARS) if !m.sheared => {
            m.sheared = true;
            let n = 2 + rng.next_int_bound(3);
            Some(Used::Sheared((0..n).map(|_| ItemStack { id: 35, count: 1, damage: m.color as u16 }).collect()))
        }
        (Kind::Sheep, 351) if !m.sheared && m.color != !(held.damage as u8) & 15 => {
            m.color = !(held.damage as u8) & 15;
            Some(Used::Dyed)
        }
        (Kind::Cow, BUCKET) => Some(Used::Milked),
        _ => None,
    }
}

/// What a tap did to a wolf or a pig (`EntityWolf.interact`, `ItemSaddle.saddleEntity`, `EntityPig.interact`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Befriend {
    /// A bone was used up and the wolf is tame (hearts) / the bone was used up and nothing happened (smoke).
    Tamed,
    Failed,
    /// Raw or cooked pork was used up: +3 health.
    Healed,
    /// The owner's tap on a tame wolf: it sits down / stands up. No item is used.
    Sat,
    Stood,
    /// A saddle was used up on a pig.
    Saddled,
    /// The player climbs onto a saddled pig.
    Mounted,
}

pub const BONE: u16 = 352;
pub const BOAT: u16 = 333;
pub const SADDLE: u16 = 329;

/// A tap with `held` on a mob that may be befriended or mounted; `None` = nothing here (the tap attacks, or `use_on_mob` runs).
/// A wild wolf that is angry takes no bone (the tap attacks it); a tame one takes pork while hurt, otherwise the tap toggles sitting.
pub fn befriend(m: &mut Mob, held: Option<ItemStack>, rng: &mut JavaRandom) -> Option<Befriend> {
    let id = held.map(|h| h.id);
    match m.kind {
        Kind::Wolf if !m.tame => (id == Some(BONE) && m.anger == 0).then(|| {
            if rng.next_int_bound(3) == 0 {
                m.tame_now();
                Befriend::Tamed
            } else {
                Befriend::Failed
            }
        }),
        Kind::Wolf if matches!(id, Some(319 | 320)) && m.health < 20 => {
            m.health = (m.health + 3).min(20);
            Some(Befriend::Healed)
        }
        Kind::Wolf => {
            m.sitting = !m.sitting;
            m.stop();
            Some(if m.sitting { Befriend::Sat } else { Befriend::Stood })
        }
        Kind::Pig if id == Some(SADDLE) && !m.saddled => {
            m.saddled = true;
            Some(Befriend::Saddled)
        }
        Kind::Pig if m.saddled && !m.ridden => {
            m.ridden = true;
            Some(Befriend::Mounted)
        }
        _ => None,
    }
}

/// `EntityFish`: the bobber. 20 Hz `tick`s; `pos` is the corner of its 0.25 box.
// ponytail: it never hooks a mob (vanilla pulls the mob toward you, wear 3) and has no box collision, only the
// cell it is in; the cast starts at the eye instead of 0.16 to the side.
pub struct Hook {
    pub pos: Vec3,
    vel: Vec3,
    ground: bool,
    in_ground: u16,
    catchable: u16,
}

fn is_water(b: Option<u8>) -> bool {
    matches!(b, Some(8 | 9))
}

impl Hook {
    /// `EntityFish(world, player)`: 1.5 blocks per tick along the look direction, 0.1 below the eye.
    pub fn cast(eye: Vec3, forward: Vec3, rng: &mut JavaRandom) -> Self {
        Self { pos: eye - Vec3::new(0.0, 0.1, 0.0), vel: arrow_velocity(forward, rng), ground: false, in_ground: 0, catchable: 0 }
    }

    /// True once a fish is on the line (`ticksCatchable > 0`): reel in now.
    pub fn bite(&self) -> bool {
        self.catchable > 0
    }

    /// One `EntityFish.onUpdate`. `rain`: the sky is open above it and it rains (a bite is then 1 in 300, not 1 in 500).
    /// False once it has lain on the ground for 1200 ticks.
    pub fn tick(&mut self, get: BlockQuery<'_>, rng: &mut JavaRandom, rain: bool) -> bool {
        let cell = |p: Vec3| get(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        let solid = |b: Option<u8>| b.is_some_and(|b| b != 0 && !(8..=11).contains(&b));
        if self.ground {
            if solid(cell(self.pos)) {
                self.in_ground += 1;
                return self.in_ground < 1200;
            }
            self.ground = false;
            self.vel *= Vec3::new(rng.next_float() * 0.2, rng.next_float() * 0.2, rng.next_float() * 0.2);
            self.in_ground = 0;
        }
        if solid(cell(self.pos + self.vel)) {
            self.ground = true;
            return true;
        }
        self.pos += self.vel;
        let mut drag = 0.92;
        // Five slices of the box; the share that is under water.
        let wet = (0..5).filter(|k| is_water(cell(self.pos + Vec3::new(0.0, 0.25 * (*k as f32 + 1.0) / 5.0, 0.0)))).count() as f32 / 5.0;
        if wet > 0.0 {
            if self.catchable > 0 {
                self.catchable -= 1;
            } else if rng.next_int_bound(if rain { 300 } else { 500 }) == 0 {
                self.catchable = rng.next_int_bound(30) as u16 + 10;
                self.vel.y -= 0.2;
            }
        }
        if self.catchable > 0 {
            self.vel.y -= rng.next_float() * rng.next_float() * rng.next_float() * 0.2;
        }
        self.vel.y += 0.04 * (wet * 2.0 - 1.0);
        if wet > 0.0 {
            drag *= 0.9;
            self.vel.y *= 0.8;
        }
        self.vel *= drag;
        true
    }

    /// `EntityFish.catchFish`: (the raw fish and the velocity it flies to the player with, wear of the rod). A bite gives
    /// a fish and wear 1, a hook on the ground wear 2, an empty cast 0.
    pub fn catch(&self, player: Vec3) -> (Option<(ItemStack, Vec3)>, u16) {
        let fish = (self.catchable > 0).then(|| {
            let d = player - self.pos;
            (ItemStack { id: RAW_FISH, count: 1, damage: 0 }, d * 0.1 + Vec3::new(0.0, d.length().sqrt() * 0.08, 0.0))
        });
        (fish, if self.ground { 2 } else { fish.is_some() as u16 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(id: u16, damage: u16) -> ItemStack {
        ItemStack { id, count: 1, damage }
    }

    #[test]
    fn bucket_rules() {
        assert_eq!((bucket_fill(9, 0), bucket_fill(8, 3), bucket_fill(11, 0), bucket_fill(1, 0)), (Some(326), None, Some(327), None));
        assert_eq!((bucket_pour(326), bucket_pour(327), bucket_pour(325), bucket_pour(335)), (Some(8), Some(10), None, None));
    }

    #[test]
    fn sheep_and_cow() {
        let mut r = JavaRandom::new(1);
        let mut sheep = Mob::new(Kind::Sheep, 0.0, 10.0, 0.0, 0.0, &mut r);
        sheep.color = 0;
        assert_eq!(use_on_mob(&mut sheep, st(351, 15), &mut r), None, "white dye: already white (colour 0)");
        assert_eq!(use_on_mob(&mut sheep, st(351, 1), &mut r), Some(Used::Dyed));
        assert_eq!(sheep.color, 14, "red dye -> red wool (`~1 & 15`)");
        let Some(Used::Sheared(w)) = use_on_mob(&mut sheep, st(SHEARS, 0), &mut r) else { panic!("shears") };
        assert!((2..=4).contains(&w.len()) && w.iter().all(|s| s.id == 35 && s.damage == 14));
        assert!(use_on_mob(&mut sheep, st(SHEARS, 0), &mut r).is_none() && use_on_mob(&mut sheep, st(351, 4), &mut r).is_none(), "bare");
        let mut cow = Mob::new(Kind::Cow, 0.0, 10.0, 0.0, 0.0, &mut r);
        assert_eq!(use_on_mob(&mut cow, st(BUCKET, 0), &mut r), Some(Used::Milked));
        assert_eq!(use_on_mob(&mut cow, st(SHEARS, 0), &mut r), None);
    }

    #[test]
    fn arrow_flies_forward() {
        let v = arrow_velocity(Vec3::new(0.0, 0.0, -1.0), &mut JavaRandom::new(7));
        assert!((v.length() - ARROW_SPEED).abs() < 0.05 && v.z < -1.4);
    }

    #[test]
    fn hook_floats_and_bites() {
        // Water 60..64 over a stone floor at y 59: the bobber settles near the surface and eventually has a bite.
        let get = |_: i32, y: i32, _: i32| Some(if y >= 60 && y < 64 { 9 } else if y < 60 { 1 } else { 0 });
        let mut r = JavaRandom::new(3);
        let mut h = Hook::cast(Vec3::new(0.5, 64.6, 0.5), Vec3::new(0.0, -0.2, -1.0), &mut r);
        let mut bit = false;
        for _ in 0..4000 {
            assert!(h.tick(&get, &mut r, false));
            bit |= h.bite();
            if bit { break }
        }
        assert!(bit, "a bite within 4000 ticks (1 in 500 per tick)");
        let (fish, wear) = h.catch(Vec3::new(0.5, 65.0, 8.5));
        assert_eq!((fish.map(|f| f.0.id), wear), (Some(349), 1));
        assert!(fish.unwrap().1.y > 0.0, "the fish arcs up toward the player");
        // On dry ground it lies there and wears the rod by 2.
        let dry = |_: i32, y: i32, _: i32| Some(if y < 60 { 1 } else { 0 });
        let mut d = Hook::cast(Vec3::new(0.5, 61.0, 0.5), Vec3::new(0.0, -1.0, 0.0), &mut r);
        (0..30).for_each(|_| { d.tick(&dry, &mut r, false); });
        assert_eq!(d.catch(Vec3::ZERO), (None, 2));
    }
}
