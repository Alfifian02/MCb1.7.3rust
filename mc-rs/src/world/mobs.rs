//! M8 mobs: pig, cow, sheep, chicken, wolf, squid, zombie, zombie pigman, giant, skeleton, creeper, spider, slime.
//! One `Mob` and one AI for all: `EntityCreature.updatePlayerActionState` (pick a cell with `func_31026_E`, walk to it) over
//! `EntityLiving.updatePlayerActionState` (random turning, look at a player within 8, `func_27021_X` despawn), at 20 Hz, with
//! the body moved every frame by `physics::step_box`. The kinds differ by `spec` (size, health, `moveSpeed`, `attackStrength`),
//! `loot` (`dropFewItems`) and the few special cases below: `EntityMob.attackEntity` melee, skeleton arrows (`EntityArrow`),
//! creeper fuse and `Explosion`, spider leap and wall climbing, slime hopping and splitting, squid swimming, chicken egg and
//! slow fall, zombie/skeleton burning in daylight, zombie pigman and wolf anger.
//! ponytail: `Pathfinder` (A*, `world::path`) chases a target, brings a tame wolf to its owner and, for the "smart animals" below, walks every
//! pig / cow / sheep / chicken to its wander cell; every other mob's wandering is a straight walk to the chosen cell, and a blocked mob hops, so it climbs one-block steps but does not go round walls there.
//! NOT b1.7.3 (`Mobs::smart`, `animalAi` in options.txt, on by default): smart animals only walk to cells they can stand in and reach (no cliffs, water,
//! lava or cactus), a stray one drifts back to its herd, and one that is hit runs from the blow for 4 to 6 s at 1.5 x its speed. Light is not read by the wander weights. No sounds, no death
//! animation, no fall/lava damage, no breeding, mobs are not saved. A wolf can be tamed with a bone, sits, follows its owner and is put beside them; a pig takes a saddle and
//! the player rides it (it keeps its own AI, as in b1.7.3: no steering). Walk speed is the terminal speed
//! of `moveFlying` (`moveSpeed` x 0.1 x 0.98 against 0.546 drag), reached the way the Java does, by momentum (see `step`: 0.546 a tick on the
//! ground, 0.91 in the air, so a mob eases into a walk, glides to a stop and keeps its run over a hop), and the body trails the head
//! (`body_yaw` = `renderYawOffset`, `head_pitch` = `faceEntity`'s pitch): both closer to b1.7.3 than the old instant speed and stiff turn.
//! test, and knock the player back not at all. Normal difficulty. The Nether's pig zombies and ghasts spawn (`Mobs::nether`); the ghast (`ai_ghast`, `EntityGhast`) and its `Fireball` are here, modelled (`render/items.rs`), skinned and sounded.
//! NOT b1.7.3 either (same switch, `Mobs::smart`): calm animals graze, root, peck and rest (`Mob::idle`). A cow or sheep on grass lowers its head and
//! chews 3 to 8 s, a pig roots on grass or dirt 2 to 5 s, a hen pecks the ground in short 1 to 2 s runs, and a cow, sheep or pig now and then stands
//! still for 8 to 20 s; each walk has its own gait (a stroll at 0.6 to 1.0 x the walk, a hen sometimes dashes at 1.5 x). Eating, rooting or resting
//! ends when a person comes within 4 blocks or when the animal is hit. The head is drawn dipped (`Mob::head`, `Mob::munch`, `render/items.rs`).
//! ponytail: the grass is not eaten (no block write from `Mobs`, and b1.7.3 sheep never regrow wool anyway), no lying down, no sounds, no baby animals.
//! UNVERIFIED: every number above by eye, and that it compiles.
//! UNVERIFIED: spawn rules and rates (vanilla spawns animals while generating chunks and every tick per chunk for monsters),
//! the arrow gravity 0.03, the model boxes (the spider/wolf/squid ones from memory) and every colour (flat stand-ins).

use std::sync::atomic::{AtomicU32, Ordering};

use glam::Vec3;

use crate::world::biome::Biome;
use crate::world::chunk::brightness;
use crate::world::craft::{self, Kind as Tool};
use crate::world::dig;
use crate::world::gen::noise::JavaRandom;
use crate::world::path;
use crate::world::items::{Drops, ItemStack};
use crate::world::names::RESIST;
use crate::world::physics::{self, BlockQuery, Player};

/// Render buffer sizes: most mobs and arrows alive at once.
pub const MAX_MOBS: usize = 48;
pub const MAX_ARROWS: usize = 16;
/// Animals within `NEAR` blocks of the player (the spawn ring's outer edge), at most.
const CAP_ANIMALS: usize = 12;
/// NOT b1.7.3 (it tops animals up every 400 ticks against a cap on the whole loaded area): `Mobs::spawn` tries every `ANIMAL_EVERY` ticks,
/// so a herd that was killed or walked off is back in seconds; the ceiling is the old 12 but counted around the player, so far strays no
/// longer hold the refill back. UNVERIFIED by eye: every number here.
const ANIMAL_EVERY: u32 = 100;
const NEAR: f32 = 48.0;
/// A new pack starts no nearer than this to another animal (herds spread out), and none of a kind that already has `CAP_KIND` within `NEAR`.
const SPACING: f32 = 12.0;
const CAP_KIND: usize = 5;
const ANIMALS: [Kind; 5] = [Pig, Cow, Sheep, Chicken, Wolf];
/// `Mobs::count_near`'s radius for a count that is not local (monsters, squid: the old global caps).
const ANYWHERE: f32 = f32::INFINITY;
const CAP_MONSTERS: usize = 10;
const CAP_SQUID: usize = 3;
/// `EntityGhast.getMaxSpawnedInChunk` is 1 a spawn. ponytail: no chunk accounting, so this many in the whole loaded Nether. UNVERIFIED by eye.
const CAP_GHAST: usize = 2;
/// Render buffer size: most fireballs alive at once.
pub const MAX_FIREBALLS: usize = 8;
const TICK: f32 = 0.05;
/// m/s per unit of `moveSpeed`: 0.7 walks at 3 m/s.
const WALK: f32 = 3.0 / 0.7;
/// Smart animals (not b1.7.3): a hit animal runs for `PANIC_TICKS` + 0..40 ticks at `PANIC_SPEED` x its walk. UNVERIFIED by eye.
const PANIC_TICKS: u16 = 80;
const PANIC_SPEED: f32 = 1.5;
/// NOT b1.7.3 (`animalAi`): a wild wolf runs down a sheep at this x its walk (0.3 x 1.8 beats a panicking sheep's 0.25 x 1.5). A wolf hunts
/// for `HUNT_TICKS` out of every `3 * HUNT_TICKS` (offset by its place in the list), within `HUNT_RANGE` blocks, and bites for 2 each second;
/// a sheep with a wild wolf within `FLEE_RANGE` runs from it. UNVERIFIED by eye.
const HUNT_SPEED: f32 = 1.8;
const HUNT_TICKS: u32 = 200;
const HUNT_RANGE: f32 = 14.0;
const FLEE_RANGE: f32 = 8.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind { Pig, Cow, Sheep, Chicken, Wolf, Squid, Zombie, PigZombie, Giant, Skeleton, Creeper, Spider, Slime, Ghast }
use Kind::*;

/// (width, height, health, moveSpeed, attackStrength, a flat colour as wool metadata: unused since the mobs have their skins).
fn spec(k: Kind) -> (f32, f32, i32, f32, i32, u8) {
    match k {
        Pig => (0.9, 0.9, 10, 0.7, 0, 6),
        Cow => (0.9, 1.3, 10, 0.7, 0, 12),
        Sheep => (0.9, 1.3, 10, 0.7, 0, 0),
        Chicken => (0.3, 0.4, 4, 0.7, 0, 0),
        Wolf => (0.8, 0.8, 8, 1.1, 2, 8),
        Squid => (0.95, 0.95, 10, 0.7, 0, 11),
        Zombie => (0.6, 1.8, 20, 0.5, 5, 13),
        PigZombie => (0.6, 1.8, 20, 0.5, 5, 6),
        Giant => (3.6, 10.8, 20, 0.5, 50, 13),
        Skeleton => (0.6, 1.8, 20, 0.7, 2, 8),
        Creeper => (0.6, 1.8, 20, 0.7, 0, 5),
        Spider => (1.4, 0.9, 20, 0.8, 2, 15),
        Slime => (0.6, 0.6, 1, 0.7, 0, 5),
        Ghast => (4.0, 4.0, 10, 0.7, 0, 0),
    }
}

pub struct Mob {
    /// Unique per mob (`Entity.entityId`): what a `Fireball` remembers of the ghast that fired it.
    pub id: u32,
    pub kind: Kind,
    /// `pos` is the centre of the box.
    pub body: Player,
    /// `rotationYaw`, degrees; the mob moves along (-sin, cos).
    pub yaw: f32,
    pub health: i32,
    /// `field_704_R` / `field_705_Q`: leg swing phase and amount.
    pub limb: f32,
    pub limb_amt: f32,
    /// `hurtTime`: ticks of red flash left.
    pub hurt: u8,
    /// Model scale: slime size 1/2/4, giant 6.
    pub size: f32,
    /// Sheep fleece colour (cloth metadata).
    pub color: u8,
    /// Sheep: shorn (`EntitySheep.getSheared`); no wool on death, bare for good.
    pub sheared: bool,
    /// Creeper: ticks of fuse burnt.
    pub fuse: u8,
    /// Wolf: tamed by a bone (`isWolfTamed`; the owner is the one player) and sitting (`isWolfSitting`).
    pub tame: bool,
    pub sitting: bool,
    /// Wolf, NOT b1.7.3: the coat (`COAT_*`, the skin it wears) and the voice (`WOLF_CRY`, one of `VOICES`).
    pub coat: u8,
    pub voice: u8,
    /// Wolf, NOT b1.7.3 (`animalAi`): the sheep a wild wolf is hunting, set by `Mobs::hunt`.
    prey: Option<Vec3>,
    /// Sheep, NOT b1.7.3 (`animalAi`): the cell it has just grazed; `Mobs::update` turns it into `Ev::Graze`.
    bit: Option<(i32, i32, i32)>,
    /// Pig: wearing a saddle, and carrying the player (`riddenByEntity`).
    pub saddled: bool,
    pub ridden: bool,
    pub half: Vec3,
    age: u32,
    yaw_vel: f32,
    look: Option<i32>,
    goal: Option<(f32, f32)>,
    fspeed: f32,
    jumping: bool,
    wet: bool,
    /// Wolf: it got wet and has not shaken itself dry yet (`isWolfShaking` before the shake begins).
    damp: bool,
    blocked: bool,
    life: u8,
    last: i32,
    target: bool,
    shot: bool,
    lit: bool,
    pub anger: i32,
    attack_time: u8,
    hop: i32,
    egg: i32,
    fire: u16,
    free: u8,
    swim: Vec3,
    /// `pathToEntity` and the index of the point being walked to (`PathEntity.currentPathIndex`).
    path: Vec<(i32, i32, i32)>,
    path_i: usize,
    /// Smart animals (`Mobs::herd` sets it each tick): the path is a wander / flee walk, not a chase.
    smart: bool,
    wander: bool,
    /// Ticks left of running from `scare`, the place of the blow (`damage`).
    panic: u16,
    scare: Vec3,
    /// The nearest mob of its kind within 24 blocks (`Mobs::herd`), what a stray animal drifts back to.
    mate: Option<Vec3>,
    /// NOT b1.7.3 (smart animals): ticks left of grazing / rooting / pecking (head down, still) and of resting (still, head up).
    graze: u16,
    rest: u16,
    /// Walk speed of the current wander walk, in units of `moveSpeed` (a stroll 0.6..1.0, a hen's dash 1.5).
    gait: f32,
    /// 0 = head up .. 1 = head down on the ground (eased every frame), and the chewing / pecking phase the renderer bobs it by.
    pub head: f32,
    pub munch: f32,
    /// `renderYawOffset`: where the body points. It trails `yaw` (the head) and follows the way the mob moves, within 75 degrees of the head.
    pub body_yaw: f32,
    /// `rotationPitch` of a mob looking at the player (radians, down is positive), eased toward `look_pitch` every frame.
    pub head_pitch: f32,
    look_pitch: f32,
    /// Sound state: `livingSoundTime`, the cry a hit left to be heard (`Mobs::update` takes it), a slime in the air, a zombie pigman just
    /// angered (`becomeAngryAt`) and the ticks left of its `randomSoundDelay`.
    talk: i32,
    cry: Option<Cry>,
    airborne: bool,
    enraged: bool,
    growl: u8,
    /// Ghast: `attackCounter` (charging from 0 to 20, -40 after a shot; the shot's flame skin shows above 10), `prevAttackCounter`, the waypoint it
    /// drifts to and `courseChangeCooldown`.
    pub charge: i32,
    pub prev_charge: i32,
    /// Ghast: ticks it has existed, as a frame-by-frame float (`ModelGhast.setRotationAngles`' `var3`: the tentacles sway by it).
    pub anim: f32,
    wp: Vec3,
    course: i32,
}

/// What a mob says: `getLivingSound` now and then, `getHurtSound` on a fresh hit, `getDeathSound` on the killing one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cry { Say, Hurt, Die }

/// NOT b1.7.3: the wolf's coats, the order of the skins in `wolf.rgba` (`atlas::skin::WOLF0 + coat * 3 + state`; `tools/gen_wolf.py`).
pub const COAT_PALE: u8 = 0;
pub const COAT_ASHEN: u8 = 1;
pub const COAT_BLACK: u8 = 2;
pub const COAT_CHESTNUT: u8 = 3;
pub const COAT_SNOWY: u8 = 4;
pub const COAT_WOODS: u8 = 5;

/// NOT b1.7.3: where wolves spawn: the forest and the taiga (b1.7.3's lists) and the snowy birch grove.
pub fn wolf_land(b: Biome) -> bool {
    matches!(b, Biome::Forest | Biome::Taiga | Biome::SnowyBirchGrove)
}

/// NOT b1.7.3: the coat a wolf born in biome `b` wears (`roll` is 0..8): the woods wolf in the forests, the snowy one in the grove, and in
/// the taiga the pale one half the time, the ashen a quarter, the black and chestnut an eighth each; the pale one elsewhere.
pub fn coat_for(b: Biome, roll: i32) -> u8 {
    use Biome::*;
    match b {
        Forest | SeasonalForest | AutumnalForest | OakWoodland | WetWoodland => COAT_WOODS,
        SnowyBirchGrove => COAT_SNOWY,
        Taiga => match roll {
            4 | 5 => COAT_ASHEN,
            6 => COAT_BLACK,
            7 => COAT_CHESTNUT,
            _ => COAT_PALE,
        },
        _ => COAT_PALE,
    }
}

/// NOT b1.7.3: the seven voices of a wolf (the sound variants of 1.21.5, `assets/sound/wolf/<voice>/`), one per wolf for good; each has the
/// b1.7.3 cries, in the order `cry` picks them: bark, panting, whine, growl, hurt, death.
pub const VOICES: usize = 7;
macro_rules! voices {
    ($($v:literal),*) => { [$([concat!("mob.wolf.", $v, ".bark"), concat!("mob.wolf.", $v, ".panting"), concat!("mob.wolf.", $v, ".whine"),
        concat!("mob.wolf.", $v, ".growl"), concat!("mob.wolf.", $v, ".hurt"), concat!("mob.wolf.", $v, ".death")]),*] };
}
const WOLF_CRY: [[&str; 6]; VOICES] = voices!("classic", "puglin", "sad", "angry", "grumpy", "big", "cute");

/// `getLivingSound` / `getHurtSound` / `getDeathSound` and `getSoundVolume` of each kind (b1.7.3 names; `EntityLiving`'s default for the
/// hurt and the death is `random.hurt`, for the living sound none). `wolf`: the low two bits 0 bark, 1 panting, 2 whine, 3 growl, picked by
/// the caller; the rest the wolf's `voice`.
fn cry(k: Kind, c: Cry, wolf: u8) -> Option<(&'static str, f32)> {
    let vol = match k { Cow | Wolf | Squid => 0.4, Slime => 0.6, Ghast => 10.0, _ => 1.0 };
    let n = match (k, c) {
        (Pig, Cry::Die) => "mob.pigdeath",
        (Pig, _) => "mob.pig",
        (Cow, Cry::Say) => "mob.cow",
        (Cow, _) => "mob.cowhurt",
        (Sheep, _) => "mob.sheep",
        (Chicken, Cry::Say) => "mob.chicken",
        (Chicken, _) => "mob.chickenhurt",
        (Wolf, Cry::Say) => WOLF_CRY[(wolf >> 2) as usize % VOICES][wolf as usize & 3],
        (Wolf, Cry::Hurt) => WOLF_CRY[(wolf >> 2) as usize % VOICES][4],
        (Wolf, Cry::Die) => WOLF_CRY[(wolf >> 2) as usize % VOICES][5],
        (Squid, _) => return None,
        (Zombie, Cry::Say) => "mob.zombie",
        (Zombie, Cry::Hurt) => "mob.zombiehurt",
        (Zombie, Cry::Die) => "mob.zombiedeath",
        (PigZombie, Cry::Say) => "mob.zombiepig.zpig",
        (PigZombie, Cry::Hurt) => "mob.zombiepig.zpighurt",
        (PigZombie, Cry::Die) => "mob.zombiepig.zpigdeath",
        (Skeleton, Cry::Say) => "mob.skeleton",
        (Skeleton, _) => "mob.skeletonhurt",
        (Creeper, Cry::Say) | (Giant, Cry::Say) => return None,
        (Creeper, Cry::Hurt) => "mob.creeper",
        (Creeper, Cry::Die) => "mob.creeperdeath",
        (Spider, Cry::Die) => "mob.spiderdeath",
        (Spider, _) => "mob.spider",
        (Slime, Cry::Say) => return None,
        (Slime, _) => "mob.slime",
        (Giant, _) => "random.hurt",
        (Ghast, Cry::Say) => "mob.ghast.moan",
        (Ghast, Cry::Hurt) => "mob.ghast.scream",
        (Ghast, Cry::Die) => "mob.ghast.death",
    };
    Some((n, vol))
}

pub enum Ev {
    /// A sound at a place: name, where, volume, pitch factor and whether the usual `(r - r) * 0.2` spread applies (`Audio::cry`).
    Snd(&'static str, Vec3, f32, f32, bool),
    /// Damage to the player (before armor).
    Hurt(i32),
    /// A creeper went off here.
    Boom(Vec3),
    Shoot(Vec3, Vec3),
    /// A ghast fires: where the fireball starts, the vector it is aimed along (centre to centre; `Fireball::new` jitters it) and the ghast's `Mob::id`.
    Fire(Vec3, Vec3, u32),
    /// A fireball burst here: `lib.rs` makes the power 1 explosion with fire (`World.newExplosion(null, x, y, z, 1.0F, true)`).
    Burst(Vec3),
    /// A fireball's smoke puff (`spawnParticle("smoke", x, y + 0.5, z)` every tick): where.
    Smoke(Vec3),
    /// `random.bow` as a skeleton shoots (`false`), `random.bowhit` as an arrow hits a block, a mob or the player (`true`), here; `lib.rs` plays it.
    Bow(Vec3, bool),
    /// A player's arrow stopped against a block here.
    Landed(Vec3),
    Egg(Vec3),
    /// A thrown egg hit something here: 1 in 8 hatches a chicken (`EntityEgg.onUpdate`).
    Hatch(Vec3),
    /// NOT b1.7.3: a sheep grazed the cell it stands in: the tall grass in it goes, or else the grass block under it turns to dirt (which
    /// the grass spread covers again).
    Graze((i32, i32, i32)),
}

pub struct Ctx<'a> {
    pub get: BlockQuery<'a>,
    pub light: &'a dyn Fn(i32, i32, i32) -> u8,
    /// `World.isDaytime`.
    pub day: bool,
    /// The player's box centre and eye.
    pub player: Vec3,
    pub eye: Vec3,
    /// NOT b1.7.3: the biome of a column (the wolf lists and coats of `Mobs::spawn`).
    pub biome: &'a dyn Fn(i32, i32) -> Biome,
    /// NOT b1.7.3: how far a mob sees the player, as a fraction of 16 blocks (`biomelife::sight`, the fog; `biomeRules`).
    pub sight: f32,
}

fn norm(mut a: f32) -> f32 {
    while a < -180.0 { a += 360.0; }
    while a >= 180.0 { a -= 360.0; }
    a
}

fn cell(p: Vec3) -> (i32, i32, i32) {
    (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32)
}

fn solid(b: Option<u8>) -> bool {
    b.map_or(false, |b| b > 0 && !(8..=11).contains(&b))
}

/// `EntityLiving.canEntityBeSeen`: no solid block on the segment.
fn sees(get: BlockQuery<'_>, a: Vec3, b: Vec3) -> bool {
    let d = b - a;
    let n = (d.length() * 2.0) as i32;
    (1..n).all(|i| {
        let (x, y, z) = cell(a + d * (i as f32 / n as f32));
        !solid(get(x, y, z))
    })
}

/// `World.func_675_a`: the share of a grid of points over the box `lo..hi` (steps of 1 / (2 x side + 1) of each side, walked in f32 as the
/// Java does) from which the segment to `at` crosses no block. ponytail: `sees` marches 0.5-block steps and counts every non-liquid block.
fn exposure(get: BlockQuery<'_>, at: Vec3, lo: Vec3, hi: Vec3) -> f32 {
    let marks = |a: usize| {
        let step = 1.0 / (((hi[a] - lo[a]) as f64) * 2.0 + 1.0);
        let (mut v, mut t) = (Vec::new(), 0.0f32);
        while t <= 1.0 {
            v.push(lo[a] + (hi[a] - lo[a]) * t);
            t = (t as f64 + step) as f32;
        }
        v
    };
    let (xs, ys, zs) = (marks(0), marks(1), marks(2));
    let mut seen = 0;
    for &x in &xs {
        for &y in &ys {
            for &z in &zs {
                seen += sees(get, Vec3::new(x, y, z), at) as usize;
            }
        }
    }
    seen as f32 / (xs.len() * ys.len() * zs.len()) as f32
}

/// `Explosion.doExplosionA`'s pass over one entity: its damage and its push in m/s, or `None` out of range. `feet` is `posX/Y/Z`
/// (the box's bottom for a mob, its middle for a boat or a cart), `lo..hi` its box. The size is doubled first (`explosionSize *= 2`).
pub fn blast(get: BlockQuery<'_>, at: Vec3, power: f32, feet: Vec3, lo: Vec3, hi: Vec3) -> Option<(i32, Vec3)> {
    let size = power * 2.0;
    let d = feet - at;
    let r = d.length() / size;
    if r > 1.0 {
        return None;
    }
    let e = (1.0 - r) * exposure(get, at, lo, hi);
    Some((((e * e + e) / 2.0 * 8.0 * size + 1.0) as i32, d.normalize_or_zero() * e * 20.0))
}

/// How far the head (`yaw`) leads the body (`body_yaw`), degrees in -75..=75: what the renderer turns the head by.
pub fn lead(yaw: f32, body_yaw: f32) -> f32 {
    norm(yaw - body_yaw).clamp(-75.0, 75.0)
}

/// Pig, cow, sheep and chicken: the animals the smart walk and panic are for (a wolf has its own AI, a squid swims).
fn animal(k: Kind) -> bool {
    matches!(k, Pig | Cow | Sheep | Chicken)
}

/// The cell a mob `h` cells tall can stand in in column (x, z), within 3 cells of `y`, highest first: a solid floor that is not a cactus, and
/// nothing but air in its body cells (so never in water or lava).
fn stand_y(get: BlockQuery<'_>, x: i32, y: i32, z: i32, h: i32) -> Option<i32> {
    (y - 3..=y + 3).rev().find(|&f| solid(get(x, f - 1, z)) && get(x, f - 1, z) != Some(81) && (0..h).all(|i| get(x, f + i, z) == Some(0)))
}

impl Mob {
    /// `feet` is the cell the mob stands in (its bottom face is the cell's floor).
    pub fn new(kind: Kind, x: f32, feet: f32, z: f32, yaw: f32, rng: &mut JavaRandom) -> Self {
        let (mut w, mut h, mut hp, ..) = spec(kind);
        let size = match kind {
            Slime => (1 << rng.next_int_bound(3)) as f32,
            Giant => 6.0,
            _ => 1.0,
        };
        if kind == Slime {
            (w, h, hp) = (0.6 * size, 0.6 * size, (size * size) as i32);
        }
        let half = Vec3::new(w / 2.0, h / 2.0, w / 2.0);
        let color = if kind == Sheep {
            let r = rng.next_int_bound(100);
            if r < 5 { 15 } else if r < 10 { 7 } else if r < 15 { 8 } else if r < 18 { 12 } else if rng.next_int_bound(500) == 0 { 6 } else { 0 }
        } else {
            0
        };
        let pos = Vec3::new(x, feet + half.y + 0.01, z);
        // ponytail: a process-wide counter (as `Cart::new` has), so callers of `new` need no id plumbing; it wraps after 2^32 mobs.
        static NEXT: AtomicU32 = AtomicU32::new(0);
        Self { id: NEXT.fetch_add(1, Ordering::Relaxed), kind, body: Player { pos, vel: Vec3::ZERO, on_ground: false }, yaw, health: hp, limb: 0.0, limb_amt: 0.0, hurt: 0, size, color, sheared: false,
            fuse: 0, tame: false, sitting: false, prey: None, bit: None, coat: COAT_PALE, voice: if kind == Wolf { ((x.to_bits() ^ z.to_bits().rotate_left(11)) % VOICES as u32) as u8 } else { 0 }, saddled: false, ridden: false, half, age: 0, yaw_vel: 0.0, look: None, goal: None, fspeed: 0.0, jumping: false, wet: false, damp: false, blocked: false, life: 0,
            last: 0, target: false, shot: false, lit: false, anger: 0, attack_time: 0, hop: 0, egg: rng.next_int_bound(6000) + 6000, fire: 0,
            free: 0, swim: Vec3::ZERO, path: Vec::new(), path_i: 0, smart: false, wander: false, panic: 0, scare: Vec3::ZERO, mate: None, graze: 0, rest: 0, gait: 1.0, head: 0.0, munch: 0.0, body_yaw: yaw, head_pitch: 0.0, look_pitch: 0.0, talk: 0, cry: None, airborne: false, enraged: false, growl: 0, charge: 0, prev_charge: 0, anim: 0.0, wp: pos, course: 0 }
    }

    /// `EntityWolf.interact`, a bone that took: tamed, sitting, full health, calm.
    pub fn tame_now(&mut self) {
        (self.tame, self.sitting, self.health, self.anger, self.target, self.goal) = (true, true, 20, 0, false, None);
    }

    /// `setPathToEntity(null)` and `isJumping = false`: stop where it stands.
    pub fn stop(&mut self) {
        (self.goal, self.fspeed, self.jumping) = (None, 0.0, false);
        self.path.clear();
    }

    /// `EntityCreature.updatePlayerActionState` with a target: a path to the player, made when there is none and again 1 time in 20
    /// (`getPathToEntity(.., 16)`), and the goal is the point being walked to; points are passed within `width * 2` of them. With
    /// no path (walled in, or beyond 16) or at its end, the goal is the player themselves (the old straight walk).
    fn chase(&mut self, rng: &mut JavaRandom, c: &Ctx) {
        self.chase_at(rng, c, c.player);
    }

    /// `chase` after any point (a hunting wolf's sheep).
    fn chase_at(&mut self, rng: &mut JavaRandom, c: &Ctx, at: Vec3) {
        let (lo, w, h) = (self.aabb().0, self.half.x * 2.0, self.half.y * 2.0);
        let size = ((w + 1.0).floor() as i32, (h + 1.0).floor() as i32, (w + 1.0).floor() as i32);
        if self.path.is_empty() || rng.next_int_bound(20) == 0 {
            let target = ((at.x - w / 2.0).floor() as i32, (at.y - physics::HALF.y).floor() as i32, (at.z - w / 2.0).floor() as i32);
            self.path = path::find(c.get, (lo.x.floor() as i32, lo.y.floor() as i32, lo.z.floor() as i32), target, size, 16.0).unwrap_or_default();
            self.path_i = 0;
        }
        self.follow(size, w, Some((at.x, at.z)));
    }

    /// Walk the path: points are passed within `w * 2` of them, and the goal is the next one, or `end` when the path has run out.
    fn follow(&mut self, size: (i32, i32, i32), w: f32, end: Option<(f32, f32)>) {
        let at = |&(x, _, z): &(i32, i32, i32)| (x as f32 + size.0 as f32 * 0.5, z as f32 + size.2 as f32 * 0.5);
        while let Some(p) = self.path.get(self.path_i).map(at) {
            let (dx, dz) = (p.0 - self.body.pos.x, p.1 - self.body.pos.z);
            if dx * dx + dz * dz >= (w * 2.0) * (w * 2.0) {
                break;
            }
            self.path_i += 1;
        }
        self.goal = self.path.get(self.path_i).map(at).or(end);
    }

    /// The calm / scared walk of a smart animal (see `pick_dest`): follow the path it has and end it at its last point, then choose the next
    /// one when it is time: 1 tick in 80 when calm, every fourth tick while it panics (at most two path searches each).
    fn roam(&mut self, rng: &mut JavaRandom, c: &Ctx) {
        self.panic = self.panic.saturating_sub(1);
        // NOT b1.7.3: a grazing or resting animal stays where it is until its time is up or a person comes within 4 blocks.
        if self.graze > 0 || self.rest > 0 {
            if (c.player - self.body.pos).length_squared() < 16.0 {
                (self.graze, self.rest) = (0, 0);
            } else {
                if self.graze == 1 && self.kind == Sheep {
                    self.bit = Some(cell(self.body.pos - Vec3::Y * (self.half.y - 0.1)));
                }
                (self.graze, self.rest) = (self.graze.saturating_sub(1), self.rest.saturating_sub(1));
                return;
            }
        }
        let w = self.half.x * 2.0;
        let size = ((w + 1.0).floor() as i32, (self.half.y * 2.0 + 1.0).floor() as i32, (w + 1.0).floor() as i32);
        if self.wander && self.path_i >= self.path.len() {
            (self.wander, self.goal) = (false, None);
            self.path.clear();
        }
        if !self.wander && self.panic == 0 && rng.next_int_bound(70) == 0 {
            self.idle(rng, c, false);
        }
        if self.graze == 0 && self.rest == 0 && !self.wander && ((self.panic > 0 && self.panic % 4 == 0) || (self.panic == 0 && rng.next_int_bound(80) == 0)) {
            self.pick_dest(rng, c, size);
        }
        if self.wander {
            self.follow(size, w, None);
        }
    }

    /// NOT b1.7.3: what a calm smart animal does with a pause, where it stands (`arrived`: it has just ended a walk, so it is more likely to eat).
    /// A cow or sheep on grass grazes (60..160 ticks), a pig on grass or dirt roots (40..100), a hen on any solid ground pecks (15..40); else
    /// 1 pause in 8 of a cow, sheep or pig is a rest (160..400 ticks). ponytail: the grass stays (see the module doc).
    fn idle(&mut self, rng: &mut JavaRandom, c: &Ctx, arrived: bool) {
        if (c.player - self.body.pos).length_squared() < 16.0 {
            return; // no settling down with a person within 4 blocks
        }
        let bite = if arrived { 3 } else { 1 };
        let (x, y, z) = cell(self.body.pos - Vec3::Y * (self.half.y - 0.1));
        let ground = (c.get)(x, y - 1, z);
        let eats = match self.kind {
            Cow | Sheep if ground == Some(2) => Some(60 + rng.next_int_bound(100)),
            Pig if matches!(ground, Some(2 | 3)) => Some(40 + rng.next_int_bound(60)),
            Chicken if solid(ground) => Some(15 + rng.next_int_bound(25)),
            _ => None,
        };
        match eats {
            Some(n) if rng.next_int_bound(4) < bite => self.graze = n as u16,
            _ if matches!(self.kind, Cow | Sheep | Pig) && rng.next_int_bound(8) == 0 => self.rest = 160 + rng.next_int_bound(240) as u16,
            _ => {}
        }
    }

    /// A cell a smart animal walks to, and the path there. Calm: of 10 random cells within 6 blocks the best by `func_31026_E`'s weights (grass 10,
    /// anything else -0.5), but only cells it can stand in, and +4 for the ones nearer its herd when it has strayed more than 6 blocks off. Scared:
    /// of 6 cells 6..10 blocks off, within 60 degrees of straight away from the blow, the farthest from it. Of the best two the first one the
    /// path-finder reaches is taken; none reachable = it stays put.
    fn pick_dest(&mut self, rng: &mut JavaRandom, c: &Ctx, size: (i32, i32, i32)) {
        let (here, scared) = (self.body.pos, self.panic > 0);
        let floor = (here.y - self.half.y).floor() as i32;
        let mut cands: Vec<(f32, (i32, i32, i32))> = Vec::new();
        for _ in 0..(if scared { 6 } else { 10 }) {
            let (dx, dz) = if scared {
                let away = here - self.scare;
                let a = away.z.atan2(away.x) + (rng.next_float() - 0.5) * 2.1;
                let r = 6.0 + rng.next_float() * 4.0;
                (a.cos() * r, a.sin() * r)
            } else {
                (rng.next_int_bound(13) as f32 - 6.0, rng.next_int_bound(13) as f32 - 6.0)
            };
            let (x, z) = ((here.x + dx).floor() as i32, (here.z + dz).floor() as i32);
            let Some(y) = stand_y(c.get, x, floor, z, size.1) else { continue };
            let spot = Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5);
            let mut w = if scared { (spot - self.scare).length() } else if (c.get)(x, y - 1, z) == Some(2) { 10.0 } else { -0.5 };
            if let (Some(m), false) = (self.mate, scared) {
                if (m - here).length() > 6.0 && (m - spot).length() < (m - here).length() - 2.0 {
                    w += 4.0;
                }
            }
            cands.push((w, (x, y, z)));
        }
        cands.sort_by(|a, b| b.0.total_cmp(&a.0));
        let lo = self.aabb().0;
        let start = (lo.x.floor() as i32, lo.y.floor() as i32, lo.z.floor() as i32);
        for &(_, dest) in cands.iter().take(2) {
            if let Some(p) = path::find(c.get, start, dest, size, 16.0) {
                (self.path, self.path_i, self.wander) = (p, 0, true);
                // NOT b1.7.3: each walk has its own gait; a hen sometimes dashes.
                self.gait = if self.kind == Chicken && rng.next_int_bound(4) == 0 { 1.5 } else { 0.6 + rng.next_float() * 0.4 };
                return;
            }
        }
    }

    /// `EntityWolf.getPathOrWalkableBlock`, the far branch: beside the owner, on the ring of the 5 x 5 around them, in a cell with
    /// a solid floor and two free cells above. ponytail: vanilla does this only when no path is found within 16; with no
    /// path-finder it is done whenever the owner is more than 16 away.
    fn teleport_to(&mut self, c: &Ctx) {
        let (x0, z0) = (c.player.x.floor() as i32 - 2, c.player.z.floor() as i32 - 2);
        let y = (c.player.y - physics::HALF.y).floor() as i32;
        for dx in 0..=4 {
            for dz in 0..=4 {
                let (x, z) = (x0 + dx, z0 + dz);
                if (dx < 1 || dz < 1 || dx > 3 || dz > 3) && solid((c.get)(x, y - 1, z)) && !solid((c.get)(x, y, z)) && !solid((c.get)(x, y + 1, z)) {
                    self.body.pos = Vec3::new(x as f32 + 0.5, y as f32 + self.half.y + 0.01, z as f32 + 0.5);
                    self.body.vel = Vec3::ZERO;
                    self.goal = None;
                    return;
                }
            }
        }
    }

    pub fn aabb(&self) -> (Vec3, Vec3) {
        (self.body.pos - self.half, self.body.pos + self.half)
    }

    /// `EntityLiving.func_27021_X`: false when it despawns (more than 128 blocks away, or 1/800 after 600 ticks beyond 32).
    fn old(&mut self, rng: &mut JavaRandom, d2: f32) -> bool {
        self.age += 1;
        if self.tame || self.ridden {
            return false; // `EntityWolf.canDespawn`; a mount stays
        }
        if d2 > 16384.0 {
            return true;
        }
        if self.age > 600 && rng.next_int_bound(800) == 0 {
            if d2 < 1024.0 { self.age = 0 } else { return true }
        }
        false
    }

    /// One 20 Hz tick of AI. Returns false when the mob despawns (or, for a creeper, has gone off).
    fn tick(&mut self, rng: &mut JavaRandom, c: &Ctx, ev: &mut Vec<Ev>) -> bool {
        self.life = self.life.saturating_sub(1);
        self.hurt = self.hurt.saturating_sub(1);
        self.free = self.free.saturating_sub(1);
        self.attack_time = self.attack_time.saturating_sub(1);
        let (k, pos) = (self.kind, self.body.pos);
        let (d, (cx, cy, cz)) = (c.player - pos, cell(pos));
        let (d2, dist) = (d.length_squared(), d.length());
        // EntityZombie / EntitySkeleton.onLivingUpdate: daylight on bare sky sets fire; fire is 1 damage a second.
        if matches!(k, Zombie | Skeleton) {
            let b = brightness((c.light)(cx, cy, cz));
            if c.day && b > 0.5 && (cy + 1..128).all(|y| matches!((c.get)(cx, y, cz), Some(0))) && rng.next_float() * 30.0 < (b - 0.4) * 2.0 {
                self.fire = 160;
            }
        }
        if self.fire > 0 {
            self.fire -= 1;
            if self.wet {
                // `Entity.moveEntity`: "random.fizz" at 0.7, pitch 1.6 (ponytail: `Ev::Snd`'s spread is +-0.32, the Java's +-0.4).
                ev.push(Ev::Snd("random.fizz", pos, 0.7, 1.6, true));
                self.fire = 0;
            } else if self.fire % 20 == 0 {
                self.health -= 1;
                self.hurt = 10;
                self.cry = Some(if self.health <= 0 { Cry::Die } else { Cry::Hurt });
            }
        }
        // `BlockCactus.onEntityCollidedWithBlock`: 1 damage per tick of contact (the 10-tick window paces it), no anger, no knockback.
        if physics::touching(pos, self.half, c.get, 81) {
            self.hit(1, pos);
        }
        if k == Wolf && self.wet {
            self.sitting = false; // `EntityWolf.updatePlayerActionState`: in water it stands up
            self.damp = true;
        } else if k == Wolf && self.damp && self.body.on_ground && self.path_i >= self.path.len() {
            // `EntityWolf.onLivingUpdate`: out of the water, on its feet and not walking, it shakes; `onUpdate` makes the sound as it starts.
            // ponytail: only the sound; there is no shaking animation, and rain does not wet it. Lava counts as wet, as `wet` does.
            self.damp = false;
            ev.push(Ev::Snd("mob.wolf.shake", pos, 0.4, 1.0, true));
        }
        // EntityCreature.findPlayerToAttack per kind; anger (zombie pigman, wolf) sets `target` itself.
        self.anger = (self.anger - 1).max(0);
        let see = sees(c.get, pos, c.eye);
        if self.target {
            self.target = d2 <= 1024.0 && (self.anger > 0 || !matches!(k, PigZombie | Wolf));
        } else {
            self.target = see && d2 < 256.0 * c.sight * c.sight && match k {
                Zombie | Giant | Skeleton | Creeper | Slime => true,
                Spider => (c.light)(cx, cy, cz) < 8,
                _ => false,
            };
        }
        let hop = self.hop;
        let alive = match k {
            Squid => self.ai_squid(rng, d2),
            Slime => self.ai_slime(rng, c, d2),
            Ghast => self.ai_ghast(rng, c, d2, ev),
            _ => self.ai_walk(rng, c, d2),
        };
        if !alive {
            return false;
        }
        if k == Slime {
            // `EntitySlime`: a hop of a size 2 or 4 slime squelches low (`* 0.8`), a size 4 one landing high (`/ 0.8`); volume 0.6.
            if self.jumping && self.hop > hop && self.size > 1.0 {
                ev.push(Ev::Snd("mob.slime", pos, 0.6, 0.8, true));
            }
            if self.body.on_ground && self.airborne && self.size > 2.0 {
                ev.push(Ev::Snd("mob.slime", pos, 0.6, 1.25, true));
            }
            self.airborne = !self.body.on_ground;
        }
        // `faceEntity`: while it looks at the player or hunts them the head tips up or down to their eye (at most 40 degrees).
        self.look_pitch = if (self.target || self.look.is_some()) && !matches!(k, Squid | Slime) {
            let h = (d.x * d.x + d.z * d.z).sqrt().max(0.5);
            (-(c.eye.y - (pos.y + self.half.y * 0.8)).atan2(h)).clamp(-0.7, 0.7)
        } else {
            0.0
        };
        if k == Wolf && self.tame && !self.sitting && d2 > 256.0 {
            self.teleport_to(c);
        }
        if k == Chicken {
            self.egg -= 1;
            if self.egg <= 0 {
                self.egg = rng.next_int_bound(6000) + 6000;
                ev.push(Ev::Egg(pos));
                ev.push(Ev::Snd("mob.chickenplop", pos, 1.0, 1.0, true));
            }
        }
        if !self.target {
            self.lit = false;
            self.fuse = self.fuse.saturating_sub(1);
            self.shot = false;
            return true;
        }
        // attackEntity
        self.shot = false;
        let face = (d.z.atan2(d.x)).to_degrees() - 90.0;
        match k {
            Skeleton if see && dist < 10.0 => {
                if self.attack_time == 0 {
                    let from = Vec3::new(pos.x, pos.y - self.half.y + 1.4, pos.z);
                    let mut dir = c.eye - Vec3::Y * 0.2 - from;
                    dir.y += dist * 0.2; // `setArrowHeading(dx, dy + dist x 0.2, dz, 0.6, 12.0)`
                    let noise = Vec3::new(rng.next_float() - 0.5, rng.next_float() - 0.5, rng.next_float() - 0.5) * 0.3;
                    ev.push(Ev::Shoot(from, (dir.normalize() + noise) * 0.6));
                    self.attack_time = 30;
                }
                self.yaw = face;
                self.shot = true;
            }
            Creeper => {
                if see && ((!self.lit && dist < 3.0) || (self.lit && dist < 7.0)) {
                    self.lit = true;
                    if self.fuse == 0 {
                        ev.push(Ev::Snd("random.fuse", pos, 1.0, 0.5, false)); // `timeSinceIgnited == 0`: the hiss starts, at pitch 0.5
                    }
                    self.fuse += 1;
                    self.shot = true;
                    if self.fuse >= 30 {
                        ev.push(Ev::Boom(pos));
                        return false;
                    }
                } else {
                    self.lit = false;
                    self.fuse = self.fuse.saturating_sub(1);
                }
            }
            Slime if self.size > 1.0 && dist < 0.6 * self.size + 0.4 && see => {
                ev.push(Ev::Hurt(self.size as i32));
                if self.attack_time == 0 {
                    self.attack_time = 10; // `onCollideWithPlayer` plays it when `attackEntityFrom` took: a new hit every 10 ticks
                    ev.push(Ev::Snd("mob.slimeattack", pos, 1.0, 1.0, true));
                }
            }
            Zombie | PigZombie | Giant | Wolf | Spider => {
                let bright = (c.light)(cx, cy, cz) >= 8;
                if k == Spider && bright && rng.next_int_bound(100) == 0 {
                    self.target = false; // a spider gives up in the light
                } else if k == Spider && dist > 2.0 && dist < 6.0 && rng.next_int_bound(10) == 0 {
                    if self.body.on_ground {
                        // The leap: 0.4 up, 0.4 along, in blocks per tick.
                        let h = (d.x * d.x + d.z * d.z).sqrt().max(1e-4);
                        self.body.vel = Vec3::new(d.x / h * 8.0 + self.body.vel.x * 0.2, 8.0, d.z / h * 8.0 + self.body.vel.z * 0.2);
                        self.free = 10;
                    }
                } else if dist < 2.0 + (self.half.x - 0.3).max(0.0) && self.attack_time == 0 {
                    self.attack_time = 20;
                    ev.push(Ev::Hurt(spec(k).4));
                }
            }
            _ => {}
        }
        true
    }

    /// The pig AI (see the module doc), plus walking after the player when `target` is set.
    fn ai_walk(&mut self, rng: &mut JavaRandom, c: &Ctx, d2: f32) -> bool {
        let (k, pos, get) = (self.kind, self.body.pos, c.get);
        if k == Wolf && self.sitting {
            self.stop(); // `isMovementCeased`
            return true;
        }
        if self.target && self.shot {
            (self.goal, self.fspeed, self.jumping) = (None, 0.0, false); // ranged attackers and a lit creeper stand and face
            return true;
        }
        let hunting = self.prey.is_some() && !self.target && !self.tame;
        let chasing = self.target || hunting || (k == Wolf && self.tame && d2 > 25.0);
        if chasing && self.wander {
            (self.wander, self.path_i) = (false, 0);
            self.path.clear();
        }
        if !chasing && !self.wander {
            self.path.clear();
        }
        if let (true, Some(p)) = (hunting, self.prey) {
            self.chase_at(rng, c, p); // NOT b1.7.3 (`animalAi`): a wild wolf after a sheep
        } else if chasing {
            self.chase(rng, c); // a tame wolf more than 5 blocks off follows its owner the same way
        } else if self.smart && animal(k) {
            self.roam(rng, c);
        } else if (self.goal.is_none() && rng.next_int_bound(80) == 0) || rng.next_int_bound(80) == 0 {
            // `func_31026_E`: of 10 random cells within 6 x 3 x 6 the best weight, grass 10.
            let mut best: Option<(f32, (i32, i32))> = None;
            for _ in 0..10 {
                let x = (pos.x + rng.next_int_bound(13) as f32 - 6.0).floor() as i32;
                let y = (pos.y - self.half.y + rng.next_int_bound(7) as f32 - 3.0).floor() as i32;
                let z = (pos.z + rng.next_int_bound(13) as f32 - 6.0).floor() as i32;
                let w = if get(x, y - 1, z) == Some(2) { 10.0 } else { -0.5 };
                if best.map_or(true, |b| w > b.0) {
                    best = Some((w, (x, z)));
                }
            }
            // The cell's height is ignored (no path-finder to climb to it); only a blocked mob hops.
            self.goal = best.map(|(_, (x, z))| (x as f32 + 0.5, z as f32 + 0.5));
        }
        let speed = match k {
            PigZombie if self.target => 0.95,
            _ if hunting => spec(k).3 * HUNT_SPEED,
            _ if self.panic > 0 => spec(k).3 * PANIC_SPEED,
            _ if self.wander => spec(k).3 * self.gait, // only a smart animal wanders by path
            _ => spec(k).3,
        };
        if let (Some((gx, gz)), true) = (self.goal, self.goal.is_some() && (self.target || hunting || self.panic > 0 || rng.next_int_bound(100) != 0)) {
            self.jumping = false;
            let (dx, dz) = (gx - pos.x, gz - pos.z);
            // A point of a path is passed by `chase` (within width x 2); only a bare goal (a wander cell, the player) ends here.
            if self.path_i >= self.path.len() && dx * dx + dz * dz < (self.half.x * 4.0).powi(2) {
                self.goal = None; // the path is finished
                self.fspeed = 0.0;
            } else {
                self.yaw += norm(dz.atan2(dx).to_degrees() - 90.0 - self.yaw).clamp(-30.0, 30.0);
                self.fspeed = speed * WALK;
            }
            // ponytail: vanilla hops when `isCollidedHorizontally && !hasPath()`; with no path-finder a blocked mob hops.
            self.jumping |= self.blocked;
            // `EntityCreature`: jump when the next point is higher than the feet.
            self.jumping |= self.path.get(self.path_i).is_some_and(|p| p.1 > (self.body.pos.y - self.half.y + 0.5).floor() as i32);
            self.jumping |= rng.next_float() < 0.8 && self.wet;
        } else {
            // EntityLiving.updatePlayerActionState: stand, look at a player within 8, or turn at random.
            self.goal = None;
            if self.wander {
                (self.wander, self.path_i) = (false, 0); // a pause (or the path's end): the next walk is chosen afresh
                self.path.clear();
                if self.panic == 0 {
                    self.idle(rng, c, true); // NOT b1.7.3: it may graze, root, peck or rest here
                }
            }
            if self.old(rng, d2) {
                return false;
            }
            self.fspeed = 0.0;
            if self.graze > 0 || self.rest > 0 {
                (self.look, self.yaw_vel) = (None, 0.0); // NOT b1.7.3: an animal eating or resting does not turn
            } else {
                if rng.next_float() < 0.02 {
                    if d2 < 64.0 { self.look = Some(10 + rng.next_int_bound(20)) } else { self.yaw_vel = (rng.next_float() - 0.5) * 20.0 }
                }
                if let Some(t) = self.look {
                    let d = c.player - pos;
                    self.yaw += norm((d.z.atan2(d.x)).to_degrees() - 90.0 - self.yaw).clamp(-10.0, 10.0);
                    self.look = (t > 0 && d2 <= 64.0).then_some(t - 1);
                } else {
                    if rng.next_float() < 0.05 { self.yaw_vel = (rng.next_float() - 0.5) * 20.0 }
                    self.yaw += self.yaw_vel;
                }
            }
            // Vanilla leaves `isJumping` as it was; a mob that last hopped would hop for ever, so it is cleared here.
            self.jumping = self.wet && rng.next_float() < 0.8;
        }
        self.yaw_vel *= 0.9;
        true
    }

    /// `EntitySquid.onLivingUpdate`: a new random swim vector (blocks per tick) every ~50 ticks, or when out of water.
    fn ai_squid(&mut self, rng: &mut JavaRandom, d2: f32) -> bool {
        if rng.next_int_bound(50) == 0 || !self.wet || self.swim == Vec3::ZERO {
            let f = rng.next_float() * std::f32::consts::TAU;
            self.swim = Vec3::new(f.cos() * 0.2, -0.1 + rng.next_float() * 0.2, f.sin() * 0.2);
        }
        self.yaw = (-self.swim.x).atan2(self.swim.z).to_degrees();
        !self.old(rng, d2)
    }

    /// `EntityGhast.updatePlayerActionState`: drift to a waypoint 16 blocks about (a new one when it is reached or 60 away), a push of 0.1 along the
    /// way every 2..6 ticks if the course is clear, and, with the player within 64 and in sight, face them, charge for 20 ticks and fire; the
    /// counter then sits at -40. The Java's `targetedEntity` / `aggroCooldown` only matter with several players: `getClosestPlayerToEntity(100)` is
    /// the player whenever the 64 test passes. The quirk of the Java is kept: the push and the clearance scan use the distance to the OLD waypoint.
    /// ponytail: the despawn on Peaceful is `Mobs::update`'s `hostile`; nothing else but the player can be a target.
    fn ai_ghast(&mut self, rng: &mut JavaRandom, c: &Ctx, d2: f32, ev: &mut Vec<Ev>) -> bool {
        if self.old(rng, d2) {
            return false;
        }
        self.prev_charge = self.charge;
        let (pos, half) = (self.body.pos, self.half);
        let to = self.wp - pos;
        let dist = to.length();
        if dist < 1.0 || dist > 60.0 {
            let mut r = || (rng.next_float() * 2.0 - 1.0) * 16.0;
            self.wp = pos + Vec3::new(r(), r(), r());
        }
        let due = self.course <= 0;
        self.course -= 1;
        if due {
            self.course += rng.next_int_bound(5) + 2;
            // `isCourseTraversable`: the box stepped `dist - 1` times along (new waypoint - here) / OLD distance, none of the steps in a block.
            let step = (self.wp - pos) / dist.max(1e-4);
            let clear = (1i32..).take_while(|&i| (i as f32) < dist).all(|i| !physics::aabb_hits_solid(pos + step * i as f32, half, c.get));
            if clear {
                self.body.vel += to / dist.max(1e-4) * 2.0; // 0.1 blocks a tick
            } else {
                self.wp = pos;
            }
        }
        if d2 < 64.0 * 64.0 {
            let d = c.player - pos;
            self.yaw = -d.x.atan2(d.z).to_degrees();
            // `canEntityBeSeen`: from its eye (0.85 of its height above the feet) to the player's.
            if sees(c.get, pos + Vec3::Y * (half.y * 2.0 * 0.85 - half.y), c.eye) {
                // `getSoundVolume` is 10: heard from far off.
                if self.charge == 10 {
                    ev.push(Ev::Snd("mob.ghast.charge", pos, 10.0, 1.0, true));
                }
                self.charge += 1;
                if self.charge == 20 {
                    ev.push(Ev::Snd("mob.ghast.fireball", pos, 10.0, 1.0, true));
                    let (s, co) = self.yaw.to_radians().sin_cos();
                    // `getLook(1.0)` is level here (a ghast has no pitch): 4 blocks along the heading, half its height (+ 0.5) up from its feet.
                    ev.push(Ev::Fire(Vec3::new(pos.x - s * 4.0, pos.y + 0.5, pos.z + co * 4.0), d, self.id));
                    self.charge = -40;
                }
            } else if self.charge > 0 {
                self.charge -= 1;
            }
        } else {
            self.yaw = -self.body.vel.x.atan2(self.body.vel.z).to_degrees();
            if self.charge > 0 {
                self.charge -= 1;
            }
        }
        true
    }

    /// `EntitySlime.updatePlayerActionState`: face the player, and hop every 10..30 ticks (a third of that when hunting).
    fn ai_slime(&mut self, rng: &mut JavaRandom, c: &Ctx, d2: f32) -> bool {
        if self.old(rng, d2) {
            return false;
        }
        if self.target {
            let d = c.player - self.body.pos;
            self.yaw += norm((d.z.atan2(d.x)).to_degrees() - 90.0 - self.yaw).clamp(-10.0, 10.0);
        } else if rng.next_float() < 0.05 {
            self.yaw += (rng.next_float() - 0.5) * 20.0;
        }
        if self.body.on_ground {
            self.hop -= 1;
            if self.hop <= 0 {
                self.hop = rng.next_int_bound(20) + 10;
                if self.target { self.hop /= 3 }
                self.jumping = true;
                self.fspeed = 1.5 + 0.5 * self.size; // ponytail: `moveFlying` in the air accumulates; this is a fixed hop speed
            } else {
                self.jumping = false;
                self.fspeed = 0.0;
            }
        }
        true
    }

    /// Per-frame movement along the heading the last tick chose.
    fn step(&mut self, dt: f32, get: BlockQuery<'_>) {
        let ticks = dt * 20.0;
        let (s, c) = self.yaw.to_radians().sin_cos();
        if self.kind == Ghast {
            // `physics::step_flying` below: no gravity, and the motion (the course pushes of `ai_ghast`, a knockback) drags by the medium.
        } else if self.free > 0 {
            let drag = 0.546f32.powf(ticks); // knocked back or leaping: no steering until it ends
            self.body.vel.x *= drag;
            self.body.vel.z *= drag;
        } else if self.kind == Squid {
            self.body.vel = if self.wet { self.swim * 20.0 } else { Vec3::new(0.0, self.body.vel.y, 0.0) };
        } else {
            let (tx, tz) = (-s * self.fspeed, c * self.fspeed);
            if self.wet {
                (self.body.vel.x, self.body.vel.z) = (tx, tz); // `step_box` scales the swim speed itself
            } else {
                // `moveEntityWithHeading` adds to the motion and drags it by 0.546 on the ground, 0.91 in the air, every tick: the speed
                // builds up toward the walk speed and dies away when it stops, and a hop keeps its run. Same top speed as before.
                let keep = (if self.body.on_ground { 0.546f32 } else { 0.91 }).powf(ticks);
                self.body.vel.x = tx + (self.body.vel.x - tx) * keep;
                self.body.vel.z = tz + (self.body.vel.z - tz) * keep;
                if tx == 0.0 && self.body.vel.x.abs() < 0.02 { self.body.vel.x = 0.0 }
                if tz == 0.0 && self.body.vel.z.abs() < 0.02 { self.body.vel.z = 0.0 }
            }
        }
        if self.kind == Spider && self.blocked && self.target {
            self.body.vel.y = 4.0; // `isOnLadder` while against a wall: climbs 0.2 a tick
        }
        let (vx, vz) = (self.body.vel.x, self.body.vel.z);
        let before = self.body.pos;
        let (w, l) = if self.kind == Ghast { physics::step_flying(&mut self.body, self.half, dt, get) } else { physics::step_box(&mut self.body, self.half, dt, self.jumping, get) };
        self.wet = w || l;
        self.blocked = self.fspeed > 0.0 && ((vx != 0.0 && self.body.vel.x == 0.0) || (vz != 0.0 && self.body.vel.z == 0.0));
        if self.kind == Chicken && self.body.vel.y < -2.3 {
            self.body.vel.y = -2.3; // `motionY *= 0.6` while falling
        }
        let moved = ((self.body.pos.x - before.x).powi(2) + (self.body.pos.z - before.z).powi(2)).sqrt() / ticks;
        self.limb_amt += ((moved * 4.0).min(1.0) - self.limb_amt) * (1.0 - 0.6f32.powf(ticks));
        self.limb += self.limb_amt * ticks;
        // `EntityLiving.onUpdate`: the body turns toward the way it moves (0.3 of the gap a tick, not while knocked back), then is held within
        // 75 degrees of the head, so the head leads a turn and the body follows. Per frame here, not per tick.
        let (mx, mz) = (self.body.pos.x - before.x, self.body.pos.z - before.z);
        let heading = if self.free == 0 && (mx * mx + mz * mz).sqrt() / ticks > 0.05 { mz.atan2(mx).to_degrees() - 90.0 } else { self.body_yaw };
        self.body_yaw += norm(heading - self.body_yaw) * (1.0 - 0.7f32.powf(ticks));
        self.body_yaw = self.yaw - norm(self.yaw - self.body_yaw).clamp(-75.0, 75.0);
        if self.kind == Ghast {
            self.body_yaw = self.yaw; // `renderYawOffset = rotationYaw`
            self.anim += ticks;
        }
        self.head_pitch += (self.look_pitch - self.head_pitch) * (1.0 - 0.7f32.powf(ticks));
        // NOT b1.7.3: the head eases down while it grazes, roots or pecks (0.5 s), and the chewing phase runs while it is down.
        let down = if self.graze > 0 { 1.0 } else { 0.0 };
        self.head += (down - self.head) * (1.0 - 0.8f32.powf(ticks));
        if self.head > 0.5 {
            self.munch += ticks * if self.kind == Chicken { 0.9 } else { 0.3 };
        }
    }

    /// `EntityLiving.onEntityUpdate`'s talking: each tick `rand(1000) < livingSoundTime++`, and then the mob says its piece and waits (the
    /// `getTalkInterval`: 80 ticks, 120 for animals and squid). Also a zombie pigman's grunt `randomSoundDelay` after it was angered.
    /// `snd` is the sound random of `Mobs`, not the AI's, so a mob never behaves differently for being heard.
    fn talk(&mut self, snd: &mut JavaRandom, ev: &mut Vec<Ev>) {
        let pos = self.body.pos;
        let t = self.talk;
        self.talk += 1;
        if snd.next_int_bound(1000) < t {
            self.talk = -if matches!(self.kind, Pig | Cow | Sheep | Chicken | Wolf | Squid) { 120 } else { 80 };
            // `EntityWolf.getLivingSound`: growl when angry, else 1 in 3 pants (or whines, tamed and under 10 health), else bark.
            let wolf = if self.anger > 0 && !self.tame { 3 } else if snd.next_int_bound(3) == 0 { if self.tame && self.health < 10 { 2 } else { 1 } } else { 0 };
            if let Some((n, v)) = cry(self.kind, Cry::Say, wolf | self.voice << 2) {
                ev.push(Ev::Snd(n, pos, v, 1.0, true));
            }
        }
        if self.enraged {
            (self.enraged, self.growl) = (false, snd.next_int_bound(40) as u8);
        }
        if self.growl > 0 {
            self.growl -= 1;
            if self.growl == 0 {
                ev.push(Ev::Snd("mob.zombiepig.zpigangry", pos, 2.0, 1.8, true));
            }
        }
    }

    /// `EntityLiving.attackEntityFrom`: a hit inside the 10-tick window only pays the difference; the first one knocks back.
    /// A zombie pigman or wolf hit turns on its attacker.
    pub fn damage(&mut self, dmg: i32, from: Vec3, rng: &mut JavaRandom) {
        if self.kind == Wolf {
            self.sitting = false; // `EntityWolf.attackEntityFrom`
        }
        // A tamed wolf hit by its owner (the only player there is) does not turn on them.
        if self.health > 0 && matches!(self.kind, PigZombie | Wolf) && !(self.kind == Wolf && self.tame) {
            self.anger = if self.kind == Wolf { 1 << 20 } else { 400 + rng.next_int_bound(400) };
            self.target = true;
            self.enraged = self.kind == PigZombie;
        }
        // Smart animals (not b1.7.3) run from the blow.
        if self.smart && animal(self.kind) && self.health > 0 && !self.ridden {
            (self.panic, self.scare) = (PANIC_TICKS + rng.next_int_bound(40) as u16, from);
            (self.wander, self.goal) = (false, None);
            (self.graze, self.rest) = (0, 0);
            self.path.clear();
        }
        self.hit(dmg, from);
    }

    /// The damage window of `attackEntityFrom` without the attacker's anger. `from` at the mob's own place = no
    /// knockback, which is the `attackEntityFrom(null, n)` of a cactus.
    fn hit(&mut self, dmg: i32, from: Vec3) {
        if self.health <= 0 {
            return;
        }
        self.age = 0;
        if self.life > 10 {
            if dmg <= self.last { return }
            self.health -= dmg - self.last;
            self.last = dmg;
        } else {
            self.last = dmg;
            self.life = 20;
            self.health -= dmg;
            self.hurt = 10;
            self.cry = Some(if self.health <= 0 { Cry::Die } else { Cry::Hurt }); // only a fresh hit is heard, not the top-up inside the window
            let (dx, dz) = (from.x - self.body.pos.x, from.z - self.body.pos.z);
            let d = (dx * dx + dz * dz).sqrt();
            if d > 1e-4 {
                // `knockBack`: motion / 2 - direction x 0.4 (blocks per tick), up 0.4 at most.
                self.body.vel.x = self.body.vel.x / 2.0 - dx / d * 8.0;
                self.body.vel.z = self.body.vel.z / 2.0 - dz / d * 8.0;
                self.body.vel.y = (self.body.vel.y / 2.0 + 8.0).min(8.0);
                self.free = 10;
            }
        }
    }

    /// `dropFewItems`: `getDropItemId` x `rand(3)`; skeleton also bones, sheep one wool, squid 1..=3 ink, slime balls only at size 1.
    fn loot(&self, rng: &mut JavaRandom) -> Vec<ItemStack> {
        let s = |id: u16, damage: u16, n: i32| (0..n).map(move |_| ItemStack { id, count: 1, damage });
        let n = rng.next_int_bound(3);
        match self.kind {
            Pig => s(319, 0, n).collect(),
            PigZombie => s(320, 0, n).collect(),
            Cow => s(334, 0, n).collect(),
            Chicken | Zombie => s(288, 0, n).collect(),
            Creeper | Ghast => s(289, 0, n).collect(),
            Spider => s(287, 0, n).collect(),
            Slime if self.size == 1.0 => s(341, 0, n).collect(),
            Skeleton => s(262, 0, n).chain(s(352, 0, rng.next_int_bound(3))).collect(),
            Sheep => s(35, self.color as u16, !self.sheared as i32).collect(),
            Squid => s(351, 0, n + 1).collect(),
            _ => Vec::new(),
        }
    }
}

/// `EntityArrow` fired by a skeleton: blocks per tick, 4 damage on the player, gone when it meets a block.
pub struct Arrow {
    pub pos: Vec3,
    vel: Vec3,
    age: u16,
    /// Shot by the player (`ItemBow`): hurts mobs, not the player, and lands as a pick-up-able item.
    pub player: bool,
    /// 0 = an arrow; `gear::SNOWBALL` / `gear::EGG` = a thrown item (`EntitySnowball` / `EntityEgg`: the same flight, no damage, gone on a hit).
    pub kind: u16,
}

impl Arrow {
    fn tick(&mut self, c: &Ctx, ev: &mut Vec<Ev>) -> bool {
        self.age += 1;
        self.pos += self.vel;
        let (x, y, z) = cell(self.pos);
        let on_block = solid((c.get)(x, y, z));
        if self.age > 600 || on_block {
            if self.kind == 0 && on_block {
                ev.push(Ev::Bow(self.pos - self.vel, true));
            }
            // ponytail: vanilla sticks in the block until picked up; the player's arrow drops as an item where it stopped.
            match self.kind {
                0 if self.player => ev.push(Ev::Landed(self.pos - self.vel)),
                crate::world::gear::EGG => ev.push(Ev::Hatch(self.pos - self.vel)),
                _ => {}
            }
            return false;
        }
        let d = self.pos - c.player;
        if !self.player && d.x.abs() < 0.4 && d.z.abs() < 0.4 && d.y.abs() < 1.0 {
            ev.push(Ev::Hurt(4));
            ev.push(Ev::Bow(self.pos, true));
            return false;
        }
        self.vel = self.vel * 0.99 - Vec3::Y * 0.03;
        true
    }
}

/// `EntityFireball` of a ghast: a 1 x 1 x 1 box whose bottom centre is `pos` (`posY`; the explosion is made there), `vel` in blocks per tick like
/// `Arrow`. It starts at rest and speeds up by `acc` (0.1 a tick along its aim) with 0.95 drag, to about 1.9 blocks a tick (0.8 in water); the
/// Java never times it out (an unloaded chunk freezes it), so here it is gone beyond 128 blocks of the player or outside the loaded world.
/// Hitting a block, the player or a mob makes it burst (`Ev::Burst`); the hit mob takes `damage(0)` (a snowball's knock). The ghast it came
/// from (`owner`, its `Mob::id`) is not hit for its first 25 ticks, any other ghast is; boats and carts are hit too (`Mobs::targets`). The player turns it with `Mobs::deflect`.
/// ponytail: the block test marches the segment in steps of 0.25 (a corner grazed between two steps can be missed).
pub struct Fireball {
    pub pos: Vec3,
    pub vel: Vec3,
    acc: Vec3,
    age: u16,
    /// `EntityFireball.field_9397_j`: the `Mob::id` of the ghast that fired it.
    owner: u32,
}

impl Fireball {
    /// `EntityFireball(world, owner, dx, dy, dz)`: each component of the aim gets a Gaussian of 0.4, then the acceleration is that vector's direction x 0.1.
    fn new(pos: Vec3, aim: Vec3, owner: u32, rng: &mut JavaRandom) -> Self {
        let a = aim + Vec3::new(rng.next_gaussian() as f32, rng.next_gaussian() as f32, rng.next_gaussian() as f32) * 0.4;
        Self { pos, vel: Vec3::ZERO, acc: a.normalize_or_zero() * 0.1, age: 0, owner }
    }

    /// The box a fireball is picked by (`getCollisionBorderSize` 1.0 round its own box).
    fn pick_box(&self) -> (Vec3, Vec3) {
        (self.pos + Vec3::new(-1.5, -1.0, -1.5), self.pos + Vec3::new(1.5, 2.0, 1.5))
    }

    /// `EntityFireball.onUpdate`: look along the move for a block and for an entity (boxes grown 0.3), burst at the first; else move, accelerate, drag.
    /// Returns false when it is gone.
    fn tick(&mut self, c: &Ctx, list: &mut [Mob], targets: &[(Vec3, Vec3)], rng: &mut JavaRandom, ev: &mut Vec<Ev>) -> bool {
        self.age += 1;
        let (x, y, z) = cell(self.pos);
        if (c.player - self.pos).length_squared() > 128.0 * 128.0 || (c.get)(x, y, z).is_none() {
            return false; // far away or out of the loaded world (the Java would leave it frozen there)
        }
        let n = ((self.vel.length() / 0.25).ceil() as i32).max(1);
        // `rayTraceBlocks`: the first step (0 = the cell it is in) that is in a solid cell, as a fraction of the move.
        let block = (0..=n).map(|i| i as f32 / n as f32).find(|&t| {
            let (x, y, z) = cell(self.pos + self.vel * t);
            solid((c.get)(x, y, z))
        });
        let d = self.vel * block.unwrap_or(1.0);
        let grown = |lo: Vec3, hi: Vec3| ray_box(self.pos, d, lo - Vec3::splat(0.3), hi + Vec3::splat(0.3)).filter(|&t| t <= 1.0);
        let mut best: Option<(f32, Option<usize>)> = None; // (t, mob index; None = the player, a boat or a cart)
        let (plo, phi) = (c.player - physics::HALF, c.player + physics::HALF);
        if let Some(t) = grown(plo, phi) {
            best = Some((t, None));
        }
        for (i, m) in list.iter().enumerate() {
            if m.health <= 0 || (m.id == self.owner && self.age < 25) {
                continue;
            }
            let (lo, hi) = m.aabb();
            if let Some(t) = grown(lo, hi).filter(|&t| best.map_or(true, |b| t < b.0)) {
                best = Some((t, Some(i)));
            }
        }
        // A boat or a minecart (`canBeCollidedWith`) bursts it like the player does; the blast (`App::explode`) is what hurts them.
        for &(lo, hi) in targets {
            if let Some(t) = grown(lo, hi).filter(|&t| best.map_or(true, |b| t < b.0)) {
                best = Some((t, None));
            }
        }
        if block.is_some() || best.is_some() {
            if let Some((_, Some(i))) = best {
                list[i].damage(0, self.pos, rng); // `attackEntityFrom(owner, 0)`
            }
            ev.push(Ev::Burst(self.pos));
            return false;
        }
        self.pos += self.vel;
        let (x, y, z) = cell(self.pos + Vec3::Y * 0.5);
        let wet = matches!((c.get)(x, y, z), Some(8 | 9));
        self.vel = (self.vel + self.acc) * if wet { 0.8 } else { 0.95 };
        ev.push(Ev::Smoke(self.pos + Vec3::Y * 0.5));
        true
    }
}

/// (`getAttackStrength`, wear) of hitting with `held`: `ItemSword` 4 + material, `ItemTool` 2 + material, anything else 1
/// (`Item.getDamageVsEntity`); material bonus wood 0, stone 1, iron 2, diamond 3, gold 0. Swords wear 1, tools 2, a hoe none.
pub fn attack(held: Option<u16>) -> (i32, u16) {
    match held.and_then(craft::tool) {
        Some((Tool::Sword, m)) => (4 + [0, 1, 2, 3, 0][m], 1),
        Some((Tool::Hoe, _)) => (1, 0),
        Some((_, m)) => (2 + [0, 1, 2, 3, 0][m], 2),
        None => (1, 0),
    }
}

/// Distance along the ray `o + t d` to the box `lo..hi`, if it hits within `t >= 0`.
pub(crate) fn ray_box(o: Vec3, d: Vec3, lo: Vec3, hi: Vec3) -> Option<f32> {
    let (mut t0, mut t1) = (0.0f32, f32::MAX);
    for i in 0..3 {
        if d[i].abs() < 1e-8 {
            if o[i] < lo[i] || o[i] > hi[i] { return None }
        } else {
            let (a, b) = ((lo[i] - o[i]) / d[i], (hi[i] - o[i]) / d[i]);
            t0 = t0.max(a.min(b));
            t1 = t1.min(a.max(b));
        }
    }
    (t0 <= t1).then_some(t0)
}

/// Grass with two air cells above, in column (x, z): the y of the grass.
fn grass_top(get: BlockQuery<'_>, x: i32, z: i32) -> Option<i32> {
    let y = (1..127).rev().find(|&y| get(x, y, z) != Some(0))?;
    (get(x, y, z) == Some(2) && get(x, y + 1, z) == Some(0) && get(x, y + 2, z) == Some(0)).then_some(y)
}

/// `Block.blockResistance` (hardness x 5 unless `setResistance` was called).
fn resistance(id: u8) -> f32 {
    match RESIST.binary_search_by_key(&id, |e| e.0) {
        Ok(i) => RESIST[i].1,
        _ if (8..=11).contains(&id) => 500.0,
        _ => dig::hardness(id).max(0.0) * 5.0,
    }
}

pub struct Mobs {
    pub list: Vec<Mob>,
    pub arrows: Vec<Arrow>,
    pub fireballs: Vec<Fireball>,
    /// Boxes (min, max) of the boats and carts a fireball can hit; `App` refills it each frame.
    pub targets: Vec<(Vec3, Vec3)>,
    pub rng: JavaRandom,
    /// The random of the sounds (`Mob::talk`), apart from `rng` so that hearing a mob changes nothing it does.
    snd: JavaRandom,
    seed: i64,
    acc: f32,
    clock: u32,
    burst: u32,
    /// Nothing spawns at all (the tests set it).
    pub no_spawn: bool,
    /// The Nether (`BiomeGenHell.spawnableMonsterList`): no animals, no squid and no overworld monsters, only pig zombies.
    pub nether: bool,
    /// `GameSettings.difficulty`: 0 Peaceful removes the monsters and spawns none.
    pub difficulty: u8,
    /// Smart animals (not b1.7.3, `animalAi` in options.txt): walk by path, drift back to the herd, run when hit.
    pub smart: bool,
}

/// `IMob`: what Peaceful removes.
fn hostile(k: Kind) -> bool {
    matches!(k, Zombie | Skeleton | Creeper | Spider | Slime | Giant | PigZombie | Ghast)
}

impl Mobs {
    pub fn new(seed: i64) -> Self {
        Self { list: Vec::new(), arrows: Vec::new(), fireballs: Vec::new(), targets: Vec::new(), rng: JavaRandom::new(seed ^ 0x5049_47), snd: JavaRandom::new(seed ^ 0x534E_44), seed, acc: 0.0, clock: 0, burst: 200, no_spawn: false, nether: false, difficulty: 2, smart: true }
    }

    /// NOT b1.7.3 (`animalAi`, proposal 5, predators and grazers): a wild, calm, standing wolf in its hunting third of the time picks the
    /// nearest sheep within `HUNT_RANGE` (`Mob::prey`, which `ai_walk` runs after) and bites it for 2 when it is within 1.5 blocks (a second
    /// apart); a sheep with such a wolf within `FLEE_RANGE` runs from it. A sheep a wolf kills drops what it always drops.
    fn hunt(&mut self) {
        let flat = |a: Vec3, b: Vec3| (a.x - b.x).hypot(a.z - b.z);
        let sheep: Vec<(usize, Vec3)> = if self.smart { self.list.iter().enumerate().filter(|(_, m)| m.kind == Sheep && m.health > 0).map(|(i, m)| (i, m.body.pos)).collect() } else { Vec::new() };
        let mut bites = Vec::new();
        let mut wolves = Vec::new();
        for (i, m) in self.list.iter_mut().enumerate() {
            m.prey = None;
            let hungry = (self.clock / HUNT_TICKS + i as u32) % 3 == 0;
            if m.kind != Wolf || m.tame || m.sitting || m.target || m.anger > 0 || !hungry || sheep.is_empty() {
                continue;
            }
            let at = m.body.pos;
            wolves.push(at);
            let near = sheep.iter().filter(|(_, p)| flat(*p, at) < HUNT_RANGE && (p.y - at.y).abs() < 4.0).min_by(|a, b| flat(a.1, at).total_cmp(&flat(b.1, at)));
            if let Some(&(j, p)) = near {
                m.prey = Some(p);
                if flat(p, at) < 1.5 && (p.y - at.y).abs() < 1.5 && m.attack_time == 0 {
                    m.attack_time = 20;
                    bites.push((j, at));
                }
            }
        }
        for &(j, p) in &sheep {
            if let Some(&w) = wolves.iter().filter(|w| flat(**w, p) < FLEE_RANGE).min_by(|a, b| flat(**a, p).total_cmp(&flat(**b, p))) {
                let m = &mut self.list[j];
                (m.panic, m.scare, m.graze, m.rest) = (m.panic.max(30), w, 0, 0);
            }
        }
        for (j, from) in bites {
            self.list[j].damage(2, from, &mut self.rng);
        }
    }

    /// Before each AI tick: the `smart` switch and each animal's nearest mate of its kind within 24 blocks (`Mob::mate`, the herd's pull).
    fn herd(&mut self) {
        let at: Vec<(Kind, Vec3)> = if self.smart { self.list.iter().map(|m| (m.kind, m.body.pos)).collect() } else { Vec::new() };
        for (i, m) in self.list.iter_mut().enumerate() {
            let (kind, here) = (m.kind, m.body.pos);
            m.smart = self.smart;
            m.mate = if self.smart && animal(kind) {
                at.iter()
                    .enumerate()
                    .filter(|&(j, &(k, p))| j != i && k == kind && (p - here).length_squared() < 576.0)
                    .map(|(_, &(_, p))| p)
                    .min_by(|a, b| (*a - here).length_squared().total_cmp(&(*b - here).length_squared()))
            } else {
                None
            };
        }
    }

    /// `Chunk.func_997_a(987234911)`: slimes spawn in one chunk in ten.
    fn slime_chunk(&self, cx: i32, cz: i32) -> bool {
        let s = self.seed
            .wrapping_add(cx.wrapping_mul(cx).wrapping_mul(4987142) as i64)
            .wrapping_add(cx.wrapping_mul(5947611) as i64)
            .wrapping_add((cz.wrapping_mul(cz) as i64).wrapping_mul(4392871))
            .wrapping_add(cz.wrapping_mul(389711) as i64);
        JavaRandom::new(s ^ 987234911).next_int_bound(10) == 0
    }

    /// `getCanSpawnHere` per kind at the cell (x, y, z) (the cell the mob would stand in).
    pub(crate) fn can_spawn(&mut self, c: &Ctx, kind: Kind, x: i32, y: i32, z: i32) -> bool {
        let (below, feet, head) = ((c.get)(x, y - 1, z), (c.get)(x, y, z), (c.get)(x, y + 1, z));
        let light = (c.light)(x, y, z);
        match kind {
            Pig | Cow | Sheep | Chicken | Wolf => below == Some(2) && feet == Some(0) && head == Some(0) && light > 8,
            Squid => matches!(feet, Some(8 | 9)) && matches!(head, Some(8 | 9)) && (46..63).contains(&y),
            // `EntityPigZombie.getCanSpawnHere`: no light test (the Nether has none), a free box, no liquid.
            // ponytail: the ground is still asked for (the Java spawns in mid-air and lets it fall).
            PigZombie if self.nether => solid(below) && feet == Some(0) && head == Some(0),
            // `EntityGhast.getCanSpawnHere`: 1 try in 20, not on Peaceful, and room for its 4 x 4 x 4 box (5 x 5 x 5 cells round (x, y, z), air
            // only), no ground. ponytail: the Java allows any block with no collision box (fire, grass) in the room and no liquid.
            Ghast => self.rng.next_int_bound(20) == 0 && self.difficulty > 0 && (x - 2..=x + 2).all(|i| (z - 2..=z + 2).all(|k| (y..=y + 4).all(|j| (c.get)(i, j, k) == Some(0)))),
            _ => {
                let dark = light as i32 <= self.rng.next_int_bound(8);
                let ground = solid(below) && feet == Some(0) && head == Some(0);
                dark && ground && (kind != Slime || (self.rng.next_int_bound(10) == 0 && self.slime_chunk(x >> 4, z >> 4) && y < 16))
            }
        }
    }

    /// How many of `kinds` stand within `r` blocks of `at`, level (`ANYWHERE`: all of them).
    fn count_near(&self, kinds: &[Kind], at: Vec3, r: f32) -> usize {
        self.list.iter().filter(|m| kinds.contains(&m.kind) && (m.body.pos.x - at.x).powi(2) + (m.body.pos.z - at.z).powi(2) < r * r).count()
    }

    /// A pack of 1..=4 of `kind` around the cell (x, y, z); animals are counted around the player, the rest everywhere.
    fn pack(&mut self, c: &Ctx, kind: Kind, (x, y, z): (i32, i32, i32), cap: usize) {
        let n = if kind == Ghast { 1 } else { 1 + self.rng.next_int_bound(4) }; // `getMaxSpawnedInChunk`
        for _ in 0..n {
            let (px, pz) = (x + self.rng.next_int_bound(6) - self.rng.next_int_bound(6), z + self.rng.next_int_bound(6) - self.rng.next_int_bound(6));
            let py = match kind {
                Pig | Cow | Sheep | Chicken | Wolf => grass_top(c.get, px, pz).map_or(-1, |g| g + 1),
                _ => y,
            };
            if py < 1 || (c.player - Vec3::new(px as f32, py as f32, pz as f32)).length() < 24.0 || !self.can_spawn(c, kind, px, py, pz) {
                continue;
            }
            let kinds: &[Kind] = match kind {
                Pig | Cow | Sheep | Chicken | Wolf => &ANIMALS,
                Squid => &[Squid],
                Ghast => &[Ghast],
                _ => &[Zombie, Skeleton, Creeper, Spider, Slime, Giant, PigZombie],
            };
            let r = if ANIMALS.contains(&kind) { NEAR } else { ANYWHERE };
            if self.count_near(kinds, c.player, r) < cap && self.list.len() < MAX_MOBS {
                let yaw = self.rng.next_float() * 360.0;
                let mut m = Mob::new(kind, px as f32 + 0.5, py as f32, pz as f32 + 0.5, yaw, &mut self.rng);
                if kind == Wolf {
                    m.coat = coat_for((c.biome)(px, pz), self.rng.next_int_bound(8));
                }
                self.list.push(m);
            }
        }
    }

    /// `SpawnerAnimals`: animals every `ANIMAL_EVERY` ticks and squid every 400 (both with a burst at start), monsters every 40, 24..48 blocks away.
    fn spawn(&mut self, c: &Ctx) {
        let n = |l: &[Mob], ks: &[Kind]| l.iter().filter(|m| ks.contains(&m.kind)).count();
        let ring = |r: &mut JavaRandom| {
            let (a, d) = (r.next_float() * std::f32::consts::TAU, 24.0 + r.next_float() * 24.0);
            ((c.player.x + a.cos() * d).floor() as i32, (c.player.z + a.sin() * d).floor() as i32)
        };
        let burst = self.burst > 0;
        self.burst = self.burst.saturating_sub(1);
        if !self.nether && (burst || self.clock % ANIMAL_EVERY == 0) {
            // The fuller the surroundings, the rarer the attempt: the herds ease into the cap instead of hitting it.
            let near = self.count_near(&ANIMALS, c.player, NEAR);
            if near < CAP_ANIMALS && self.rng.next_int_bound(CAP_ANIMALS as i32) as usize >= near {
                let (x, z) = ring(&mut self.rng);
                // Weights of the plains list: sheep 12, pig 10, chicken 10, cow 8. NOT b1.7.3 (it gives wolves to the forest and the taiga, 2 of
                // 42): the snowy birch grove has them too (`wolf_land`).
                let wolf = wolf_land((c.biome)(x, z)) && self.rng.next_int_bound(21) == 0;
                let kind = if wolf { Wolf } else { match self.rng.next_int_bound(40) { 0..=11 => Sheep, 12..=21 => Pig, 22..=31 => Chicken, _ => Cow } };
                let apart = self.count_near(&ANIMALS, Vec3::new(x as f32, 0.0, z as f32), SPACING) == 0;
                if let Some(y) = grass_top(c.get, x, z).filter(|_| apart && self.count_near(&[kind], c.player, NEAR) < CAP_KIND) {
                    self.pack(c, kind, (x, y + 1, z), CAP_ANIMALS);
                }
            }
        }
        if !self.nether && (burst || self.clock % 400 == 0) && n(&self.list, &[Squid]) < CAP_SQUID {
            let (x, z) = ring(&mut self.rng);
            let y = 46 + self.rng.next_int_bound(17);
            self.pack(c, Squid, (x, y, z), CAP_SQUID);
        }
        let monsters: &[Kind] = if self.nether { &[PigZombie] } else { &[Zombie, Skeleton, Creeper, Spider, Slime] };
        let room = n(&self.list, monsters) < CAP_MONSTERS || (self.nether && n(&self.list, &[Ghast]) < CAP_GHAST);
        if self.clock % 40 == 0 && self.difficulty > 0 && room {
            for _ in 0..8 {
                let (x, z) = ring(&mut self.rng);
                let y = ((c.player.y as i32) + self.rng.next_int_bound(41) - 20).clamp(1, 126);
                // `BiomeGenHell.spawnableMonsterList`: ghast 10, pig zombie 10 (a ghast has its own cap, so a crowd of pigmen does not keep it out).
                let kind = if self.nether { if self.rng.next_int_bound(2) == 0 && n(&self.list, &[Ghast]) < CAP_GHAST { Ghast } else { PigZombie } } else { [Spider, Zombie, Skeleton, Creeper, Slime][self.rng.next_int_bound(5) as usize] };
                let before = self.list.len();
                self.pack(c, kind, (x, y, z), if kind == Ghast { CAP_GHAST } else { CAP_MONSTERS });
                if self.list.len() > before {
                    break;
                }
            }
        }
    }

    /// Advance `dt` seconds: AI at 20 Hz, movement per frame, spawns, arrows and the drops of the dead. Returns what the
    /// player must suffer or the world must do (`Hurt`, `Boom`).
    pub fn update(&mut self, dt: f32, c: &Ctx, drops: &mut Drops) -> Vec<Ev> {
        let mut ev = Vec::new();
        if self.difficulty == 0 {
            self.list.retain(|m| !hostile(m.kind));
        }
        self.acc += dt;
        while self.acc >= TICK {
            self.acc -= TICK;
            self.clock += 1;
            self.herd();
            self.hunt();
            let (rng, snd) = (&mut self.rng, &mut self.snd);
            self.list.retain_mut(|m| {
                let alive = m.tick(rng, c, &mut ev);
                if let Some(cell) = m.bit.take() {
                    ev.push(Ev::Graze(cell));
                }
                if alive {
                    m.talk(snd, &mut ev);
                }
                alive
            });
            self.arrows.retain_mut(|a| a.tick(c, &mut ev));
            let (list, rng, targets) = (&mut self.list, &mut self.rng, &self.targets);
            self.fireballs.retain_mut(|f| f.tick(c, list, targets, rng, &mut ev));
            // A player's arrow inside a mob's box (grown 0.3, like `EntityArrow`) hurts it and is gone.
            let (list, rng) = (&mut self.list, &mut self.rng);
            self.arrows.retain(|a| {
                let hit = a.player.then(|| list.iter().position(|m| {
                    let (lo, hi) = m.aabb();
                    a.pos.cmpge(lo - Vec3::splat(0.3)).all() && a.pos.cmple(hi + Vec3::splat(0.3)).all()
                })).flatten();
                hit.map_or(true, |i| {
                    // A snowball or egg does 0 damage but still knocks back (`attackEntityFrom(thrower, 0)`).
                    list[i].damage(if a.kind == 0 { crate::world::gear::ARROW_DAMAGE } else { 0 }, a.pos - a.vel, rng);
                    if a.kind == crate::world::gear::EGG {
                        ev.push(Ev::Hatch(a.pos));
                    }
                    if a.kind == 0 {
                        ev.push(Ev::Bow(a.pos, true));
                    }
                    false
                })
            });
            if !self.no_spawn {
                self.spawn(c);
            }
        }
        for m in &mut self.list {
            m.step(dt, c.get);
            // What a hit since the last frame made it cry out (a kill too: it is removed below).
            if let Some(k) = m.cry.take() {
                if let Some((n, v)) = cry(m.kind, k, m.voice << 2) {
                    ev.push(Ev::Snd(n, m.body.pos, v, 1.0, true));
                }
            }
        }
        let mut out = Vec::new();
        for e in ev {
            match e {
                Ev::Shoot(pos, vel) if self.arrows.len() < MAX_ARROWS => {
                    self.arrows.push(Arrow { pos, vel, age: 0, player: false, kind: 0 });
                    out.push(Ev::Bow(pos, false));
                }
                Ev::Fire(at, aim, owner) if self.fireballs.len() < MAX_FIREBALLS => {
                    let f = Fireball::new(at, aim, owner, &mut self.rng);
                    self.fireballs.push(f);
                }
                Ev::Landed(p) => drops.spawn_stack(ItemStack { id: crate::world::gear::ARROW, count: 1, damage: 0 }, cell(p)),
                Ev::Egg(p) => drops.spawn_stack(ItemStack { id: 344, count: 1, damage: 0 }, cell(p)),
                Ev::Hatch(p) => {
                    if self.rng.next_int_bound(8) == 0 {
                        let n = if self.rng.next_int_bound(32) == 0 { 4 } else { 1 };
                        for _ in 0..n {
                            let m = Mob::new(Chicken, p.x, p.y, p.z, 0.0, &mut self.rng);
                            self.list.push(m);
                        }
                    }
                }
                Ev::Hurt(_) | Ev::Boom(_) | Ev::Burst(_) | Ev::Smoke(_) | Ev::Snd(..) | Ev::Graze(_) | Ev::Bow(..) => out.push(e),
                Ev::Shoot(..) | Ev::Fire(..) => {}
            }
        }
        let mut i = 0;
        while i < self.list.len() {
            if self.list[i].health > 0 {
                i += 1;
                continue;
            }
            let m = self.list.swap_remove(i);
            let p = m.body.pos;
            for s in m.loot(&mut self.rng) {
                drops.spawn_stack(s, cell(p));
            }
            if m.kind == Slime && m.size > 1.0 {
                // `EntitySlime.setEntityDead`: four of half the size.
                for j in 0..4 {
                    if self.list.len() < MAX_MOBS {
                        let (ox, oz) = ((j % 2) as f32 - 0.5, (j / 2) as f32 - 0.5);
                        let mut s = Mob::new(Slime, p.x + ox * m.size / 4.0, p.y - m.half.y, p.z + oz * m.size / 4.0, self.rng.next_float() * 360.0, &mut self.rng);
                        let size = m.size / 2.0;
                        (s.size, s.health, s.half) = (size, (size * size) as i32, Vec3::new(0.3 * size, 0.3 * size, 0.3 * size));
                        self.list.push(s);
                    }
                }
            }
        }
        out
    }

    /// The nearest mob on the ray within `reach`: (index, distance).
    pub fn pick(&self, eye: Vec3, dir: Vec3, reach: f32) -> Option<(usize, f32)> {
        self.list.iter().enumerate().filter_map(|(i, m)| {
            let (lo, hi) = m.aabb();
            ray_box(eye, dir, lo, hi).filter(|&t| t <= reach).map(|t| (i, t))
        }).min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// The nearest fireball on the ray within `reach` (`Entity.canBeCollidedWith`, the box grown by `getCollisionBorderSize`): (index, distance).
    pub fn pick_fireball(&self, eye: Vec3, dir: Vec3, reach: f32) -> Option<(usize, f32)> {
        self.fireballs.iter().enumerate().filter_map(|(i, f)| {
            let (lo, hi) = f.pick_box();
            ray_box(eye, dir, lo, hi).filter(|&t| t <= reach).map(|t| (i, t))
        }).min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// `EntityFireball.attackEntityFrom(player, ..)`: the motion becomes the player's look (a unit vector, blocks per tick) and the acceleration
    /// a tenth of it, so a punched fireball goes back the way it came. No damage, no tool wear.
    pub fn deflect(&mut self, i: usize, look: Vec3) {
        let f = &mut self.fireballs[i];
        (f.vel, f.acc) = (look, look * 0.1);
    }

    /// `ItemBow`: a player's arrow leaves `pos` with `vel` (blocks per tick).
    /// A player's arrow (`kind` 0) or thrown snowball / egg.
    pub fn shoot(&mut self, pos: Vec3, vel: Vec3, kind: u16) {
        if self.arrows.len() < MAX_ARROWS {
            self.arrows.push(Arrow { pos, vel, age: 0, player: true, kind });
        }
    }

    pub fn hit(&mut self, i: usize, dmg: i32, from: Vec3) {
        self.list[i].damage(dmg, from, &mut self.rng);
    }

    /// Retro Sprint's `target.addVelocity` (m/s here): added to the motion as it is, so the hit's own `knockBack` that follows halves it
    /// when it lands (a hit inside the damage window keeps it whole, as in the mod). No steering for 10 ticks, like after a knockback.
    pub fn shove(&mut self, i: usize, v: Vec3) {
        let m = &mut self.list[i];
        m.body.vel += v;
        m.free = m.free.max(10);
    }

    /// `Explosion.doExplosionA`: 16 x 16 x 16 rays from the surface of a cube, each losing strength to the blocks it crosses
    /// (resistance / 5 + 0.3, x 0.3) and 0.225 a step; the blocks left with strength are destroyed. Also hurts the mobs
    /// in range (`blast`): hurt, then pushed along the way from the centre (`motion += direction x impact`). `exploder` is false for a
    /// fireball's (`newExplosion(null, ..)`), whose hit has no `knockBack`: the push is all they get. Returns the cells to clear.
    pub fn explode(&mut self, get: BlockQuery<'_>, at: Vec3, power: f32, exploder: bool) -> Vec<(i32, i32, i32)> {
        let mut cells = std::collections::BTreeSet::new();
        for i in 0..16 {
            for j in 0..16 {
                for k in 0..16 {
                    if !(matches!(i, 0 | 15) || matches!(j, 0 | 15) || matches!(k, 0 | 15)) {
                        continue;
                    }
                    let d = Vec3::new(i as f32, j as f32, k as f32) / 15.0 * 2.0 - Vec3::ONE;
                    let (d, mut p) = (d / d.length() * 0.3, at);
                    let mut s = power * (0.7 + self.rng.next_float() * 0.6);
                    // Radium `world.explosions` (`performRayCast`): a 0.3 step re-enters the same cell ~3 times, so the cell
                    // and its cost are kept from the last step; the strength still drops on every step, as in the Java.
                    let mut last: Option<((i32, i32, i32), f32)> = None;
                    while s > 0.0 {
                        let c = cell(p);
                        match last {
                            Some((k, cost)) if k == c => s -= cost,
                            _ => {
                                let cost = get(c.0, c.1, c.2).filter(|&b| b > 0).map_or(0.0, |id| (resistance(id) / 5.0 + 0.3) * 0.3);
                                last = Some((c, cost));
                                s -= cost;
                                // New cell only: a repeat step has less strength left, so it was inserted on entry or never.
                                if s > 0.0 {
                                    cells.insert(c);
                                }
                            }
                        }
                        p += d;
                        s -= 0.225;
                    }
                }
            }
        }
        let rng = &mut self.rng;
        for m in &mut self.list {
            let (lo, hi) = m.aabb();
            if let Some((dmg, push)) = blast(get, at, power, m.body.pos - Vec3::Y * m.half.y, lo, hi) {
                m.damage(dmg, if exploder { at } else { m.body.pos }, rng);
                if !exploder {
                    m.scare = at; // it still runs from the blast
                }
                m.body.vel += push;
                m.free = m.free.max(10);
            }
        }
        cells.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bedrock at y = 0, stone 1..=8, grass at 9, air above, 64 x 64 wide.
    fn flat(x: i32, y: i32, z: i32) -> Option<u8> {
        if !(0..64).contains(&x) || !(0..64).contains(&z) || y < 0 { return None }
        Some(match y { 0 => 7, 1..=8 => 1, 9 => 2, _ => 0 })
    }

    fn ctx<'a>(player: Vec3, light: &'a dyn Fn(i32, i32, i32) -> u8) -> Ctx<'a> {
        Ctx { get: &flat, light, day: false, player, eye: player + Vec3::Y * 0.6, biome: &|_, _| Biome::Plains, sight: 1.0 }
    }

    /// NOT b1.7.3: animals top up every `ANIMAL_EVERY` ticks (more in the first 400 than the one pack the old 400-tick rule made), never past
    /// `CAP_ANIMALS` around the player (a stray walking in is the slack) and no pack of a kind with `CAP_KIND` around, and a pack starts `SPACING` from the others.
    #[test]
    fn animals_refill_fast_never_crowd_and_keep_apart() {
        let wide = |_: i32, y: i32, _: i32| Some(match y { 0 => 7, 1..=8 => 1, 9 => 2, _ => 0 });
        let l = |_, _, _| 15u8;
        let player = Vec3::new(0.5, 11.0, 0.5);
        let c = Ctx { get: &wide, light: &l, day: true, player, eye: player + Vec3::Y * 0.6, biome: &|_, _| Biome::Plains, sight: 1.0 };
        let flat = |a: Vec3, b: Vec3| ((a.x - b.x).powi(2) + (a.z - b.z).powi(2)).sqrt();
        for seed in 1..=6 {
            let (mut m, mut drops) = (Mobs::new(seed), Drops::new(1));
            m.burst = 0;
            let (mut early, mut apart, mut top) = (0, f32::MAX, (0, 0));
            for t in 1..=3000 {
                let before: Vec<Vec3> = m.list.iter().map(|a| a.body.pos).collect();
                m.update(TICK, &c, &mut drops);
                let near = m.count_near(&ANIMALS, player, NEAR);
                top = (top.0.max(near), top.1.max([Pig, Cow, Sheep, Chicken].map(|k| m.count_near(&[k], player, NEAR)).into_iter().max().unwrap()));
                if t == 400 {
                    early = near;
                }
                // A mob of age 0 that was nowhere before is new: the nearest animal of before to the middle of the pack.
                let fresh: Vec<Vec3> = m.list.iter().filter(|a| a.age == 0 && before.iter().all(|&b| flat(b, a.body.pos) > 1.5)).map(|a| a.body.pos).collect();
                if !fresh.is_empty() {
                    let mid = fresh.iter().sum::<Vec3>() / fresh.len() as f32;
                    apart = before.iter().fold(apart, |d, &b| d.min(flat(b, mid)));
                }
            }
            eprintln!("seed {seed}: early {early}, top {top:?}, apart {apart}");
            assert!(early >= 5, "seed {seed}: {early} animals after 400 ticks (the old rule gave at most one pack of 4)");
            assert!(top.0 <= CAP_ANIMALS + 3 && top.1 <= CAP_KIND + 3, "seed {seed}: crowded, {top:?}");
            assert!(apart >= SPACING - 8.0, "seed {seed}: a pack began {apart} blocks from another herd");
        }
    }

    /// A pig on grass settles, wanders off its start and stays on the ground; a hit hurts, knocks back and kills after
    /// 10 damage (swords: 4 + material); the ray finds it; a distant player despawns it.
    #[test]
    fn pig_wanders_is_hit_and_dies() {
        let mut rng = JavaRandom::new(7);
        let l = |_, _, _| 15u8;
        let c = ctx(Vec3::new(32.0, 11.0, 52.0), &l);
        let mut p = Mob::new(Pig, 32.5, 10.0, 32.5, 0.0, &mut rng);
        let (start, mut far, mut ev) = (p.body.pos, 0.0f32, Vec::new());
        for _ in 0..2000 {
            assert!(p.tick(&mut rng, &c, &mut ev));
            for _ in 0..3 { p.step(TICK / 3.0, &flat) }
            far = far.max((p.body.pos - start).length());
            assert!(p.body.pos.y > 9.9 && p.body.pos.y < 11.0, "y = {}", p.body.pos.y);
        }
        assert!(far > 1.0, "never moved");
        assert!(ev.is_empty());
        assert_eq!(ray_box(Vec3::new(0.0, 0.0, -2.0), Vec3::Z, Vec3::splat(-0.45), Vec3::splat(0.45)), Some(1.55));
        assert!(ray_box(Vec3::new(2.0, 0.0, -2.0), Vec3::Z, Vec3::splat(-0.45), Vec3::splat(0.45)).is_none());
        assert_eq!(attack(None), (1, 0));
        assert_eq!(attack(Some(268)), (4, 1)); // wooden sword
        assert_eq!(attack(Some(257)), (4, 2)); // iron pickaxe: 2 + 2
        p.damage(4, p.body.pos + Vec3::X, &mut rng);
        assert_eq!(p.health, 6);
        assert!(p.body.vel.x < 0.0 && p.hurt == 10);
        p.damage(4, p.body.pos, &mut rng); // inside the window, not bigger: ignored
        assert_eq!(p.health, 6);
        p.damage(6, p.body.pos, &mut rng); // bigger: pays the difference
        assert_eq!(p.health, 4);
        let far_player = ctx(Vec3::new(232.0, 10.0, 32.0), &l);
        let mut q = Mob::new(Pig, 32.5, 10.0, 32.5, 0.0, &mut rng);
        assert!((0..50).any(|_| !q.tick(&mut rng, &far_player, &mut ev)));
    }

    /// A mob in a cactus cell loses 1 health, once per 10-tick window, and is neither knocked back nor angered.
    #[test]
    fn cactus_hurts_a_mob_without_knockback_or_anger() {
        let cact = |x: i32, y: i32, z: i32| if (x, z) == (32, 32) && y == 10 { Some(81) } else { flat(x, y, z) };
        let l = |_, _, _| 15u8;
        let c = Ctx { get: &cact, light: &l, day: false, player: Vec3::new(5.0, 20.0, 5.0), eye: Vec3::new(5.0, 20.6, 5.0), biome: &|_, _| Biome::Plains, sight: 1.0 };
        let mut rng = JavaRandom::new(5);
        let mut w = Mob::new(Wolf, 32.5, 10.0, 32.5, 0.0, &mut rng);
        let mut ev = Vec::new();
        w.tick(&mut rng, &c, &mut ev);
        assert_eq!((w.health, w.anger, w.body.vel.x), (7, 0, 0.0), "8 - 1, calm, not pushed");
        w.tick(&mut rng, &c, &mut ev);
        assert_eq!(w.health, 7, "inside the damage window, an equal hit is ignored");
        for _ in 0..12 { w.tick(&mut rng, &c, &mut ev); }
        assert!(w.health < 7, "the window ran out and the cactus hurts again");
    }

    /// Retro Sprint's shove adds to the motion and stops the steering; the hit's `knockBack` that follows halves it (a hit inside
    /// the damage window leaves it whole).
    #[test]
    fn a_shove_is_halved_by_the_knockback_that_follows() {
        let mut ms = Mobs::new(1);
        let mut rng = JavaRandom::new(4);
        ms.list.push(Mob::new(Cow, 32.5, 10.0, 32.5, 0.0, &mut rng));
        ms.shove(0, Vec3::new(0.0, 0.0, 24.0));
        assert_eq!((ms.list[0].body.vel.z, ms.list[0].free), (24.0, 10));
        ms.hit(0, 1, Vec3::new(32.5, 10.0, 31.5)); // the player stands at -z of the cow: pushed to +z by 8 m/s more
        assert_eq!(ms.list[0].body.vel.z, 24.0 / 2.0 + 8.0, "halved, plus the knockback's 8 m/s");
        ms.hit(0, 1, Vec3::new(32.5, 10.0, 31.5)); // inside the window: no second knockback
        assert_eq!(ms.list[0].body.vel.z, 20.0);
    }

    /// Snowballs and eggs fly like arrows, hurt for 0 and are gone on a block; only the egg asks to hatch.
    #[test]
    fn a_fresh_hit_cries_a_kill_dies_and_every_name_is_in_the_sound_table() {
        let mut rng = JavaRandom::new(5);
        let mut cow = Mob::new(Cow, 32.5, 10.0, 32.5, 0.0, &mut rng);
        cow.damage(2, cow.body.pos + Vec3::X, &mut rng);
        assert_eq!((cow.cry.take(), cry(Cow, Cry::Hurt, 0)), (Some(Cry::Hurt), Some(("mob.cowhurt", 0.4))));
        cow.damage(5, cow.body.pos, &mut rng); // inside the window and bigger: pays the difference, no new cry
        assert!(cow.cry.is_none());
        let mut sheep = Mob::new(Sheep, 32.5, 10.0, 32.5, 0.0, &mut rng);
        sheep.damage(99, sheep.body.pos + Vec3::X, &mut rng);
        assert_eq!(sheep.cry, Some(Cry::Die));
        // A wolf that got wet shakes once, when it is out of the water and standing: volume 0.4, with the usual spread.
        let (mut w, mut ev) = (Mob::new(Wolf, 32.5, 10.0, 32.5, 0.0, &mut rng), Vec::new());
        let l = |_, _, _| 0u8;
        let far = ctx(Vec3::new(32.5, 11.0, 80.5), &l);
        w.wet = true;
        w.tick(&mut rng, &far, &mut ev);
        assert!(w.damp && ev.is_empty());
        w.wet = false;
        w.body.on_ground = true;
        (0..3).for_each(|_| { w.tick(&mut rng, &far, &mut ev); });
        assert_eq!(ev.iter().filter(|e| matches!(e, Ev::Snd("mob.wolf.shake", _, v, _, true) if *v == 0.4)).count(), 1);
        // A burning mob that is in water fizzes once and is out; a dry one burns on.
        let mut z = Mob::new(Zombie, 32.5, 10.0, 32.5, 0.0, &mut rng);
        (z.fire, z.wet) = (50, true);
        ev.clear();
        z.tick(&mut rng, &far, &mut ev);
        assert!(z.fire == 0 && matches!(ev[..], [Ev::Snd("random.fizz", _, v, p, true)] if v == 0.7 && p == 1.6));
        (z.fire, z.wet) = (50, false);
        ev.clear();
        z.tick(&mut rng, &far, &mut ev);
        assert!(z.fire == 49 && !ev.iter().any(|e| matches!(e, Ev::Snd("random.fizz", ..))));
        // The fuse hisses once, at pitch 0.5 with no spread.
        let l = |_, _, _| 0u8;
        let near = ctx(Vec3::new(32.5, 11.0, 33.5), &l);
        let (mut k, mut ev) = (Mob::new(Creeper, 32.5, 10.0, 32.5, 0.0, &mut rng), Vec::new());
        (0..40).any(|_| !k.tick(&mut rng, &near, &mut ev));
        assert_eq!(ev.iter().filter(|e| matches!(e, Ev::Snd("random.fuse", _, _, p, false) if *p == 0.5)).count(), 1);
        for kind in [Pig, Cow, Sheep, Chicken, Wolf, Squid, Zombie, PigZombie, Giant, Skeleton, Creeper, Spider, Slime, Ghast] {
            for c in [Cry::Say, Cry::Hurt, Cry::Die] {
                for w in 0..4 * VOICES as u8 {
                    assert!(cry(kind, c, w).map_or(true, |(n, v)| crate::audio::exists(n) && v > 0.0), "{kind:?} {c:?}");
                }
            }
        }
    }

    /// `random.bowhit` where an arrow meets a block, a mob or the player (not when it just times out in the air); a snowball is silent.
    #[test]
    fn an_arrow_sounds_where_it_hits() {
        let l = |_, _, _| 0u8;
        let c = ctx(Vec3::new(5.0, 20.0, 5.0), &l);
        let arrow = |player| Arrow { pos: Vec3::new(32.5, 9.5, 32.5), vel: Vec3::new(0.0, -0.5, 0.0), age: 0, player, kind: 0 };
        let mut ev = Vec::new();
        assert!(!arrow(false).tick(&c, &mut ev) && matches!(ev[..], [Ev::Bow(_, true)])); // a skeleton's, on a block
        ev.clear();
        assert!(!arrow(true).tick(&c, &mut ev) && matches!(ev[..], [Ev::Bow(_, true), Ev::Landed(_)]));
        ev.clear();
        let mut late = arrow(false);
        (late.age, late.pos.y) = (600, 20.0);
        assert!(!late.tick(&c, &mut ev) && ev.is_empty()); // gone in the air: silent
        let near = ctx(Vec3::new(32.5, 14.6, 32.5), &l);
        Arrow { pos: Vec3::new(32.5, 15.0, 32.5), ..arrow(false) }.tick(&near, &mut ev);
        assert!(matches!(ev[..], [Ev::Hurt(4), Ev::Bow(_, true)]));
        // A player's arrow in a mob's box: the hit sounds once; a skeleton's shot sounds as it leaves.
        let (mut ms, mut rng) = (Mobs::new(1), JavaRandom::new(3));
        ms.list.push(Mob::new(Pig, 5.5, 20.0, 5.5, 0.0, &mut rng));
        ms.arrows.push(Arrow { pos: Vec3::new(5.5, 20.0, 5.5), vel: Vec3::ZERO, age: 0, player: true, kind: 0 });
        let mut drops = crate::world::items::Drops::new(1);
        let n = |out: &[Ev]| out.iter().filter(|e| matches!(e, Ev::Bow(_, true))).count();
        assert_eq!(n(&ms.update(0.05, &c, &mut drops)), 1);
    }

    #[test]
    fn thrown_items_land_quietly_and_an_egg_hatches() {
        let l = |_, _, _| 0u8;
        let c = ctx(Vec3::new(5.0, 20.0, 5.0), &l);
        let ball = |kind| Arrow { pos: Vec3::new(32.5, 9.5, 32.5), vel: Vec3::new(0.0, -0.5, 0.0), age: 0, player: true, kind };
        let mut ev = Vec::new();
        assert!(!ball(crate::world::gear::SNOWBALL).tick(&c, &mut ev) && ev.is_empty());
        assert!(!ball(crate::world::gear::EGG).tick(&c, &mut ev) && matches!(ev[..], [Ev::Hatch(_)]));
        let mut rng = JavaRandom::new(3);
        let mut pig = Mob::new(Pig, 32.5, 10.0, 32.5, 0.0, &mut rng);
        let h = pig.health;
        pig.damage(0, Vec3::new(30.0, 10.0, 32.5), &mut rng);
        assert_eq!(pig.health, h);
    }

    /// Hostiles: a skeleton in range shoots, a creeper next to the player lights its fuse and goes off after 30 ticks,
    /// a zombie bites for 5; an explosion clears dirt-like blocks but not bedrock.
    #[test]
    fn hostiles_attack_and_explode() {
        let mut rng = JavaRandom::new(3);
        let l = |_, _, _| 0u8;
        let c = ctx(Vec3::new(32.5, 11.0, 36.5), &l);
        let mut s = Mob::new(Skeleton, 32.5, 10.0, 32.5, 0.0, &mut rng);
        let mut ev = Vec::new();
        s.tick(&mut rng, &c, &mut ev);
        assert!(ev.iter().any(|e| matches!(e, Ev::Shoot(..))));
        let mut z = Mob::new(Zombie, 32.5, 10.0, 35.0, 0.0, &mut rng);
        ev.clear();
        z.tick(&mut rng, &c, &mut ev);
        assert!(ev.iter().any(|e| matches!(e, Ev::Hurt(5))));
        let near = ctx(Vec3::new(32.5, 11.0, 33.5), &l);
        let mut k = Mob::new(Creeper, 32.5, 10.0, 32.5, 0.0, &mut rng);
        ev.clear();
        let gone = (0..40).any(|_| !k.tick(&mut rng, &near, &mut ev));
        assert!(gone && ev.iter().any(|e| matches!(e, Ev::Boom(_))));
        let mut m = Mobs::new(1);
        let cells = m.explode(&flat, Vec3::new(32.5, 1.5, 32.5), 3.0, true);
        assert!(!cells.is_empty() && cells.iter().all(|c| c.1 > 0), "{} cells", cells.len()); // bedrock at y 0 holds
        assert!(m.explode(&flat, Vec3::new(32.5, 20.5, 32.5), 3.0, true).len() > 20); // open air: a sphere of cells
    }

    /// Radium's per-cell cache must not change a ray: same cells as the plain one-lookup-per-step loop, same random draws.
    #[test]
    fn explosion_cache_matches_the_plain_ray() {
        let plain = |seed: i64, at: Vec3, power: f32| {
            let (mut rng, mut cells) = (JavaRandom::new(seed ^ 0x5049_47), std::collections::BTreeSet::new());
            for i in 0..16 { for j in 0..16 { for k in 0..16 {
                if !(matches!(i, 0 | 15) || matches!(j, 0 | 15) || matches!(k, 0 | 15)) { continue; }
                let d = Vec3::new(i as f32, j as f32, k as f32) / 15.0 * 2.0 - Vec3::ONE;
                let (d, mut p) = (d / d.length() * 0.3, at);
                let mut s = power * (0.7 + rng.next_float() * 0.6);
                while s > 0.0 {
                    let c = cell(p);
                    if let Some(id) = flat(c.0, c.1, c.2).filter(|&b| b > 0) { s -= (resistance(id) / 5.0 + 0.3) * 0.3; }
                    if s > 0.0 { cells.insert(c); }
                    p += d;
                    s -= 0.225;
                }
            }}}
            cells.into_iter().collect::<Vec<_>>()
        };
        for (seed, at, power) in [(1, Vec3::new(32.5, 1.5, 32.5), 3.0), (2, Vec3::new(10.2, 9.7, 40.9), 4.0), (3, Vec3::new(32.5, 20.5, 32.5), 4.0)] {
            assert_eq!(Mobs::new(seed).explode(&flat, at, power, true), plain(seed, at, power));
        }
    }

    /// Slimes split into four halves, and a sheep drops its colour of wool.
    #[test]
    fn slime_splits_and_sheep_drop_wool() {
        let mut rng = JavaRandom::new(5);
        let mut m = Mobs::new(1);
        m.burst = 0;
        let mut s = Mob::new(Slime, 32.5, 10.0, 32.5, 0.0, &mut rng);
        (s.size, s.health) = (4.0, 0);
        m.list.push(s);
        let l = |_, _, _| 15u8;
        let c = ctx(Vec3::new(5.0, 11.0, 5.0), &l);
        m.update(TICK, &c, &mut Drops::new(1));
        assert_eq!(m.list.len(), 4);
        assert!(m.list.iter().all(|s| s.kind == Slime && s.size == 2.0 && s.health == 4));
        let mut sheep = Mob::new(Sheep, 1.5, 10.0, 1.5, 0.0, &mut rng);
        sheep.color = 6;
        assert_eq!(sheep.loot(&mut rng).iter().map(|s| (s.id, s.damage)).collect::<Vec<_>>(), vec![(35, 6)]);
    }

    /// A bone tames a wolf (1 in 3, health 20, calm, sitting); sitting it stays put, standing it follows its owner beyond 5
    /// blocks and is put beside them beyond 16; a saddle goes on a pig once and a tap then mounts it; neither despawns.
    /// NOT b1.7.3: a wolf keeps one of seven voices for good, and its hurt and death cries are that voice's.
    #[test]
    fn a_wolf_has_a_voice_of_its_own() {
        let mut rng = JavaRandom::new(3);
        let heard: std::collections::HashSet<u8> = (0..40).map(|i| Mob::new(Wolf, i as f32 * 1.5 + 0.5, 64.0, 3.5, 0.0, &mut rng).voice).collect();
        assert!(heard.len() >= 4 && heard.iter().all(|&v| (v as usize) < VOICES), "{heard:?}");
        assert_eq!(cry(Wolf, Cry::Hurt, 3 << 2).map(|c| c.0), Some("mob.wolf.angry.hurt"));
        assert_eq!(cry(Wolf, Cry::Say, 2 | 6 << 2).map(|c| c.0), Some("mob.wolf.cute.whine"));
        assert_eq!(cry(Pig, Cry::Say, 6 << 2).map(|c| c.0), Some("mob.pig"), "only a wolf has a voice");
    }

    #[test]
    fn bone_tames_a_wolf_and_a_saddled_pig_is_a_mount() {
        use crate::world::gear::{befriend, Befriend, BONE, SADDLE};
        let l = |_, _, _| 15u8;
        let mut rng = JavaRandom::new(11);
        let item = |id| Some(ItemStack { id, count: 1, damage: 0 });
        let mut w = Mob::new(Wolf, 10.5, 10.0, 10.5, 0.0, &mut rng);
        assert!((0..60).any(|_| befriend(&mut w, item(BONE), &mut rng) == Some(Befriend::Tamed)));
        assert!(w.tame && w.sitting && w.health == 20 && w.anger == 0);
        let (mut ev, near) = (Vec::new(), ctx(Vec3::new(20.5, 10.9, 10.5), &l));
        let start = w.body.pos;
        for _ in 0..200 {
            assert!(w.tick(&mut rng, &near, &mut ev));
            for _ in 0..3 { w.step(TICK / 3.0, &flat) }
        }
        assert!((w.body.pos - start).length() < 0.05, "sitting: it did not move");
        assert_eq!(befriend(&mut w, None, &mut rng), Some(Befriend::Stood));
        for _ in 0..300 {
            w.tick(&mut rng, &near, &mut ev);
            for _ in 0..3 { w.step(TICK / 3.0, &flat) }
        }
        assert!((w.body.pos.x - 20.5).abs() < 6.5, "it followed: x = {}", w.body.pos.x);
        w.damage(2, near.player, &mut rng);
        assert!(w.anger == 0 && !w.target, "a tame wolf hit by its owner stays calm");
        let gone = ctx(Vec3::new(60.5, 10.9, 10.5), &l);
        w.tick(&mut rng, &gone, &mut ev);
        assert!((w.body.pos.x - 60.5).abs() < 3.5 && (w.body.pos.z - 10.5).abs() < 3.5, "put beside the owner: {:?}", w.body.pos);
        let mut p = Mob::new(Pig, 30.5, 10.0, 30.5, 0.0, &mut rng);
        assert_eq!(befriend(&mut p, None, &mut rng), None, "no saddle, no ride");
        assert_eq!(befriend(&mut p, item(SADDLE), &mut rng), Some(Befriend::Saddled));
        assert_eq!(befriend(&mut p, item(SADDLE), &mut rng), Some(Befriend::Mounted), "a saddled pig is mounted by the next tap");
        assert!(p.ridden);
        let far = ctx(Vec3::new(232.0, 10.0, 32.0), &l);
        assert!((0..50).all(|_| p.tick(&mut rng, &far, &mut ev)), "a mount does not despawn");
    }

    /// A tame wolf 10 blocks off with a wall between (open at z = 15) walks round it to its owner: the path-finder, not a straight
    /// walk into the wall.
    #[test]
    fn tame_wolf_walks_round_a_wall_to_its_owner() {
        let walled = |x: i32, y: i32, z: i32| if x == 10 && z <= 14 && (10..=12).contains(&y) { Some(1) } else { flat(x, y, z) };
        let l = |_, _, _| 15u8;
        let c = Ctx { get: &walled, light: &l, day: false, player: Vec3::new(15.5, 10.9, 5.5), eye: Vec3::new(15.5, 11.5, 5.5), biome: &|_, _| Biome::Plains, sight: 1.0 };
        let mut rng = JavaRandom::new(21);
        let mut w = Mob::new(Wolf, 5.5, 10.0, 5.5, 0.0, &mut rng);
        w.tame = true;
        let (mut ev, mut near, mut round) = (Vec::new(), f32::MAX, false);
        for _ in 0..2400 {
            w.tick(&mut rng, &c, &mut ev);
            for _ in 0..3 { w.step(TICK / 3.0, &walled) }
            near = near.min((w.body.pos - c.player).length());
            round |= w.body.pos.z > 14.5;
        }
        assert!(round, "it went round the end of the wall");
        assert!(near < 5.5, "and came to its owner: closest {near}");
    }

    /// Smart animals (not b1.7.3) only walk to cells they can reach: on a plateau five blocks above the ground a pig wanders for two minutes
    /// and never steps off it (the old straight walk would).
    #[test]
    fn a_smart_pig_never_walks_off_a_cliff() {
        let island = |x: i32, y: i32, z: i32| if ((28..=36).contains(&x) && (28..=36).contains(&z)) || y <= 4 { flat(x, y, z) } else { flat(x, y, z).map(|_| 0) };
        let l = |_, _, _| 15u8;
        let player = Vec3::new(32.5, 11.0, 52.5);
        let c = Ctx { get: &island, light: &l, day: false, player, eye: player + Vec3::Y * 0.6, biome: &|_, _| Biome::Plains, sight: 1.0 };
        let mut rng = JavaRandom::new(11);
        let mut p = Mob::new(Pig, 32.5, 10.0, 32.5, 0.0, &mut rng);
        p.smart = true;
        let (start, mut far, mut ev) = (p.body.pos, 0.0f32, Vec::new());
        for _ in 0..2400 {
            p.tick(&mut rng, &c, &mut ev);
            for _ in 0..3 { p.step(TICK / 3.0, &island) }
            far = far.max((p.body.pos - start).length());
            assert!(p.body.pos.y > 9.9 && p.body.pos.x > 27.5 && p.body.pos.x < 37.5 && p.body.pos.z > 27.5 && p.body.pos.z < 37.5, "left the plateau: {:?}", p.body.pos);
        }
        assert!(far > 1.0, "never moved");
    }

    /// A hit animal runs: five seconds after a blow from 2 blocks off it is well over 9 blocks from where the blow came.
    #[test]
    fn a_hit_animal_runs_from_the_blow() {
        let l = |_, _, _| 15u8;
        let c = ctx(Vec3::new(32.0, 11.0, 60.0), &l);
        let mut rng = JavaRandom::new(3);
        let mut cow = Mob::new(Cow, 32.5, 10.0, 32.5, 0.0, &mut rng);
        cow.smart = true;
        let blow = cow.body.pos + Vec3::X * 2.0;
        cow.damage(1, blow, &mut rng);
        let mut ev = Vec::new();
        for _ in 0..100 {
            cow.tick(&mut rng, &c, &mut ev);
            for _ in 0..3 { cow.step(TICK / 3.0, &flat) }
        }
        assert!((cow.body.pos - blow).length() > 9.0, "it stayed near the blow: {:?}", cow.body.pos);
    }

    /// A sheep 28 blocks from its mate wanders back to within 12 blocks of it in two minutes.
    #[test]
    fn a_stray_animal_drifts_back_to_its_herd() {
        let l = |_, _, _| 15u8;
        let c = ctx(Vec3::new(30.0, 11.0, 12.0), &l);
        let mate = Vec3::new(40.5, 10.9, 10.5);
        let mut rng = JavaRandom::new(5);
        let mut m = Mob::new(Sheep, 12.5, 10.0, 10.5, 0.0, &mut rng);
        (m.smart, m.mate) = (true, Some(mate));
        let mut ev = Vec::new();
        for _ in 0..2400 {
            m.tick(&mut rng, &c, &mut ev);
            for _ in 0..3 { m.step(TICK / 3.0, &flat) }
        }
        assert!((m.body.pos - mate).length() < 12.0, "still {} blocks from the herd", (m.body.pos - mate).length());
    }

    /// NOT b1.7.3: a cow on grass grazes for 3 to 8 s standing still, its head eased down; a person within 4 blocks lifts the head, a blow ends it.
    #[test]
    fn a_cow_grazes_still_head_down_and_stops_for_a_visitor_or_a_blow() {
        let l = |_, _, _| 15u8;
        let far = ctx(Vec3::new(32.0, 11.0, 60.0), &l);
        let mut rng = JavaRandom::new(9);
        let mut cow = Mob::new(Cow, 32.5, 10.0, 32.5, 0.0, &mut rng);
        cow.smart = true;
        for _ in 0..64 {
            if cow.graze > 0 { break }
            cow.idle(&mut rng, &far, true);
            cow.rest = 0;
        }
        assert!((60..160).contains(&cow.graze), "never started grazing: {}", cow.graze);
        let (start, mut ev) = (cow.body.pos, Vec::new());
        for _ in 0..30 {
            cow.tick(&mut rng, &far, &mut ev);
            for _ in 0..3 { cow.step(TICK / 3.0, &flat) }
        }
        assert!((cow.body.pos.x - start.x).abs() < 0.01 && (cow.body.pos.z - start.z).abs() < 0.01, "walked while grazing");
        assert!(cow.head > 0.9, "head not down: {}", cow.head);
        let near = ctx(Vec3::new(32.5, 11.0, 34.0), &l);
        cow.tick(&mut rng, &near, &mut ev);
        assert_eq!(cow.graze, 0, "it kept grazing with a person 1.5 blocks away");
        for _ in 0..30 {
            cow.tick(&mut rng, &near, &mut ev);
            for _ in 0..3 { cow.step(TICK / 3.0, &flat) }
        }
        assert!(cow.head < 0.5, "head still down: {}", cow.head);
        cow.graze = 100;
        cow.damage(1, cow.body.pos + Vec3::X * 2.0, &mut rng);
        assert_eq!((cow.graze, cow.rest), (0, 0), "a blow must end the meal");
    }

    /// NOT b1.7.3: a pig roots (40..100 ticks), a hen pecks (15..40), a cow or sheep sometimes rests (160..400) instead; a zombie and a wolf do none of it.
    #[test]
    fn each_kind_has_its_own_pause() {
        let l = |_, _, _| 15u8;
        let c = ctx(Vec3::new(32.0, 11.0, 60.0), &l);
        let mut rng = JavaRandom::new(21);
        let mut pause = |kind: Kind| {
            let (mut g, mut r) = (0u16, 0u16);
            for _ in 0..400 {
                let mut m = Mob::new(kind, 32.5, 10.0, 32.5, 0.0, &mut rng);
                m.idle(&mut rng, &c, true);
                (g, r) = (g.max(m.graze), r.max(m.rest));
            }
            (g, r)
        };
        let pig = pause(Pig);
        assert!((40..100).contains(&pig.0) && pig.1 >= 160, "pig {pig:?}");
        let hen = pause(Chicken);
        assert!((15..40).contains(&hen.0) && hen.1 == 0, "hen {hen:?}");
        let cow = pause(Cow);
        assert!((60..160).contains(&cow.0) && (160..400).contains(&cow.1), "cow {cow:?}");
        assert_eq!(pause(Zombie), (0, 0));
        assert_eq!(pause(Wolf), (0, 0));
    }

    /// NOT b1.7.3: a resting animal stands still and does not turn.
    #[test]
    fn a_resting_animal_stays_put_and_does_not_turn() {
        let l = |_, _, _| 15u8;
        let c = ctx(Vec3::new(32.0, 11.0, 60.0), &l);
        let mut rng = JavaRandom::new(2);
        let mut sheep = Mob::new(Sheep, 32.5, 10.0, 32.5, 0.0, &mut rng);
        (sheep.smart, sheep.rest) = (true, 100);
        let (start, mut ev) = (sheep.body.pos, Vec::new());
        for _ in 0..99 {
            sheep.tick(&mut rng, &c, &mut ev);
            for _ in 0..3 { sheep.step(TICK / 3.0, &flat) }
        }
        assert_eq!(sheep.rest, 1);
        assert_eq!(sheep.yaw, 0.0);
        assert!((sheep.body.pos.x - start.x).abs() < 0.01 && (sheep.body.pos.z - start.z).abs() < 0.01);
    }

    /// NOT b1.7.3: each walk has its own gait (0.6 to 1.0 x the walk) and only a hen ever dashes (1.5 x).
    #[test]
    fn walks_have_their_own_gait_and_only_hens_dash() {
        let l = |_, _, _| 15u8;
        let c = ctx(Vec3::new(32.0, 11.0, 60.0), &l);
        let mut rng = JavaRandom::new(4);
        let mut gaits = |kind: Kind| {
            let mut v = Vec::new();
            for _ in 0..200 {
                let mut m = Mob::new(kind, 32.5, 10.0, 32.5, 0.0, &mut rng);
                m.smart = true;
                m.pick_dest(&mut rng, &c, (1, 2, 1));
                if m.wander { v.push(m.gait) }
            }
            v
        };
        let cow = gaits(Cow);
        assert!(cow.len() > 100 && cow.iter().all(|g| (0.6..=1.0).contains(g)), "cow gaits {cow:?}");
        assert!(cow.iter().cloned().fold(0.0, f32::max) - cow.iter().cloned().fold(2.0, f32::min) > 0.2, "every cow walk had the same gait");
        let hen = gaits(Chicken);
        assert!(hen.iter().any(|&g| g == 1.5) && hen.iter().any(|&g| g < 1.0), "hen gaits {hen:?}");
    }

    /// `moveEntityWithHeading`: a walking mob eases up to its speed (1 tick: 1 - 0.546 of it), reaches it, and glides to a stop instead of cutting off.
    #[test]
    fn a_mob_eases_into_a_walk_and_glides_to_a_stop() {
        let mut rng = JavaRandom::new(5);
        let mut p = Mob::new(Pig, 32.5, 10.0, 8.5, 0.0, &mut rng);
        for _ in 0..6 { p.step(TICK, &flat) } // settle on the ground
        p.fspeed = 3.0;
        p.step(TICK, &flat);
        assert!(p.body.vel.z > 0.5 && p.body.vel.z < 2.0, "first tick {}", p.body.vel.z);
        for _ in 0..12 { p.step(TICK, &flat) }
        assert!(p.body.vel.z > 2.9, "top speed {}", p.body.vel.z);
        p.fspeed = 0.0;
        p.step(TICK, &flat);
        assert!(p.body.vel.z > 0.5, "stopped dead: {}", p.body.vel.z);
        for _ in 0..12 { p.step(TICK, &flat) }
        assert_eq!(p.body.vel.z, 0.0);
    }

    /// `renderYawOffset`: a turned head leads, the body follows the way it walks (not snapping) and stays within 75 degrees of the head.
    #[test]
    fn the_body_trails_the_head_and_follows_the_walk() {
        let mut rng = JavaRandom::new(6);
        let mut p = Mob::new(Pig, 32.5, 10.0, 32.5, 0.0, &mut rng);
        for _ in 0..6 { p.step(TICK, &flat) }
        p.yaw = 90.0; // the head turns; the body stands
        p.step(TICK, &flat);
        assert!((p.body_yaw - 15.0).abs() < 0.01 && lead(p.yaw, p.body_yaw) == 75.0, "body {}", p.body_yaw);
        p.fspeed = 3.0; // and walks the way the head points (yaw 90 is -x)
        p.step(TICK, &flat);
        assert!(p.body_yaw > 15.0 && p.body_yaw < 90.0, "snapped: {}", p.body_yaw);
        for _ in 0..20 { p.step(TICK, &flat) }
        assert!(norm(p.body_yaw - 90.0).abs() < 5.0, "body {}", p.body_yaw);
    }

    /// `faceEntity`: a mob that looks at a player above it tips its head up (negative), and level with it keeps it level.
    #[test]
    fn a_mob_looking_at_the_player_tips_its_head_toward_their_eyes() {
        let l = |_, _, _| 15u8;
        let up = ctx(Vec3::new(32.5, 13.0, 36.0), &l);
        let mut rng = JavaRandom::new(8);
        let mut p = Mob::new(Pig, 32.5, 10.0, 32.5, 0.0, &mut rng);
        let mut ev = Vec::new();
        for _ in 0..6 { p.step(TICK, &flat) }
        p.look = Some(30);
        p.tick(&mut rng, &up, &mut ev);
        for _ in 0..30 { p.step(TICK, &flat) }
        assert!(p.head_pitch < -0.1, "head pitch {}", p.head_pitch);
    }

    /// NOT b1.7.3 (`biomeRules`, proposal 2, fog is cover): a zombie 12 blocks off notices the player in clear air and not in a thick fog,
    /// where it sees under 8 blocks.
    #[test]
    fn fog_shortens_how_far_a_mob_notices() {
        let l = |_, _, _| 15u8;
        let player = Vec3::new(32.5, 10.9, 20.5);
        let (mut clear, mut foggy) = (ctx(player, &l), ctx(player, &l));
        foggy.sight = crate::world::biomelife::sight(0.01);
        let mut rng = JavaRandom::new(4);
        for (c, want) in [(&clear, true), (&foggy, false)] {
            let mut z = Mob::new(Zombie, 20.5, 10.0, 20.5, 0.0, &mut rng);
            z.tick(&mut rng, c, &mut Vec::new());
            assert_eq!(z.target, want, "sight {}", c.sight);
        }
        clear.sight = 1.0;
    }

    /// NOT b1.7.3: wolves are born where `wolf_land` says, in the coat of the biome, and never on the plains.
    #[test]
    fn wolves_are_born_in_forests_taigas_and_groves() {
        assert!(wolf_land(Biome::Forest) && wolf_land(Biome::Taiga) && wolf_land(Biome::SnowyBirchGrove) && !wolf_land(Biome::Plains));
        assert_eq!((coat_for(Biome::Forest, 0), coat_for(Biome::WetWoodland, 3), coat_for(Biome::SnowyBirchGrove, 7), coat_for(Biome::Plains, 7)), (COAT_WOODS, COAT_WOODS, COAT_SNOWY, COAT_PALE));
        let taiga: Vec<u8> = (0..8).map(|r| coat_for(Biome::Taiga, r)).collect();
        assert_eq!(taiga, [0, 0, 0, 0, COAT_ASHEN, COAT_ASHEN, COAT_BLACK, COAT_CHESTNUT]);
        let wide = |_: i32, y: i32, _: i32| Some(match y { 0 => 7, 1..=8 => 1, 9 => 2, _ => 0 });
        let l = |_, _, _| 15u8;
        let player = Vec3::new(0.5, 11.0, 0.5);
        for (biome, coat, any) in [(Biome::Forest, COAT_WOODS, true), (Biome::SnowyBirchGrove, COAT_SNOWY, true), (Biome::Plains, COAT_PALE, false)] {
            let at = |_: i32, _: i32| biome;
            let c = Ctx { get: &wide, light: &l, day: true, player, eye: player + Vec3::Y * 0.6, biome: &at, sight: 1.0 };
            let mut wolves = 0;
            for seed in 1..=8 {
                let (mut m, mut drops) = (Mobs::new(seed), Drops::new(1));
                m.smart = false;
                for _ in 0..1500 {
                    m.update(TICK, &c, &mut drops);
                }
                wolves += m.list.iter().filter(|a| a.kind == Wolf).inspect(|w| assert_eq!(w.coat, coat, "{biome:?}")).count();
            }
            assert_eq!(wolves > 0, any, "{biome:?}: {wolves} wolves in 8 worlds");
        }
    }

    /// NOT b1.7.3 (`animalAi`, proposal 5): in its hunting third a wild wolf runs a sheep down and bites it; a sheep runs from it; a tame
    /// wolf, a wolf with the switch off and a wolf out of its hunting time leave the sheep alone.
    #[test]
    fn a_wild_wolf_hunts_a_sheep() {
        let l = |_, _, _| 15u8;
        let c = ctx(Vec3::new(60.5, 11.0, 60.5), &l);
        let run = |smart: bool, tame: bool, clock: u32| {
            let mut rng = JavaRandom::new(9);
            let (mut m, mut drops) = (Mobs::new(2), Drops::new(1));
            (m.no_spawn, m.smart, m.clock) = (true, smart, clock);
            let mut wolf = Mob::new(Wolf, 20.5, 10.0, 20.5, 0.0, &mut rng);
            wolf.tame = tame;
            m.list.push(wolf);
            m.list.push(Mob::new(Sheep, 26.5, 10.0, 20.5, 0.0, &mut rng));
            let mut fled = false;
            for _ in 0..180 {
                m.update(TICK, &c, &mut drops);
                fled |= m.list.iter().any(|s| s.kind == Sheep && s.panic > 0);
            }
            let sheep = m.list.iter().find(|s| s.kind == Sheep).map_or(0, |s| s.health);
            (sheep, fled)
        };
        let full = Mob::new(Sheep, 0.5, 10.0, 0.5, 0.0, &mut JavaRandom::new(1)).health;
        let (hurt, fled) = run(true, false, 0);
        assert!(hurt < full && fled, "the sheep has {hurt} of {full} health and fled: {fled}");
        assert_eq!(run(true, true, 0), (full, false), "a tame wolf does not hunt");
        assert_eq!(run(false, false, 0), (full, false), "animalAi off: no hunt");
        assert_eq!(run(true, false, HUNT_TICKS - 1 + HUNT_TICKS), (full, false), "a wolf rests two thirds of the time");
    }

    /// `BiomeGenHell`: the Nether spawns pig zombies and ghasts (each with its own cap) and nothing else, in the light that would keep every overworld monster away; the
    /// same floor in the overworld spawns nobody (no grass for animals, too bright for monsters); Peaceful spawns none.
    #[test]
    fn the_nether_spawns_pig_zombies_and_ghasts_only() {
        let floor = |_: i32, y: i32, _: i32| Some(match y { 0 => 7, 1..=9 => 87, _ => 0 });
        let l = |_, _, _| 15u8;
        let player = Vec3::new(0.5, 11.0, 0.5);
        let c = Ctx { get: &floor, light: &l, day: false, player, eye: player + Vec3::Y * 0.6, biome: &|_, _| Biome::Plains, sight: 1.0 };
        let run = |nether: bool, difficulty: u8| {
            let (mut m, mut drops) = (Mobs::new(5), Drops::new(1));
            (m.nether, m.difficulty) = (nether, difficulty);
            for _ in 0..2000 {
                m.update(TICK, &c, &mut drops);
            }
            m.list.iter().map(|a| a.kind).collect::<Vec<_>>()
        };
        let hell = run(true, 2);
        assert!(!hell.is_empty() && hell.iter().all(|&k| k == PigZombie || k == Ghast), "{hell:?}");
        assert!(hell.iter().filter(|&&k| k != Ghast).count() <= CAP_MONSTERS && hell.iter().filter(|&&k| k == Ghast).count() <= CAP_GHAST, "{hell:?}");
        assert!(run(false, 2).is_empty(), "the overworld has no use for this floor");
        assert!(run(true, 0).is_empty(), "Peaceful");
    }

    /// `EntityGhast.updatePlayerActionState`: a player in sight within 64 blocks makes it face them, count 1 a tick and, at 20, ask for a fireball
    /// 4 blocks along its heading (the counter then sits at -40); out of sight the counter runs back down; a ghast drifts (never falls) and
    /// leaves a pile of gunpowder 0..2 when it dies.
    #[test]
    fn a_ghast_charges_twenty_ticks_then_fires_and_never_falls() {
        let l = |_, _, _| 15u8;
        let player = Vec3::new(32.5, 10.9, 52.5);
        let c = ctx(player, &l);
        let mut rng = JavaRandom::new(3);
        let mut g = Mob::new(Ghast, 32.5, 30.0, 32.5, 0.0, &mut rng);
        let mut ev = Vec::new();
        for t in 1..=19 {
            assert!(g.tick(&mut rng, &c, &mut ev), "tick {t}");
            assert_eq!(g.charge, t);
        }
        assert!(!ev.iter().any(|e| matches!(e, Ev::Fire(..))), "not yet");
        g.tick(&mut rng, &c, &mut ev);
        let fired: Vec<_> = ev.iter().filter_map(|e| if let Ev::Fire(at, aim, _) = e { Some((*at, *aim)) } else { None }).collect();
        assert_eq!(fired.len(), 1);
        assert_eq!(g.charge, -40);
        let snd = |n: &str| ev.iter().filter(|e| matches!(e, Ev::Snd(m, _, v, _, true) if *m == n && *v == 10.0)).count();
        assert_eq!((snd("mob.ghast.charge"), snd("mob.ghast.fireball")), (1, 1), "each once, at counter 10 and 20");
        let (at, aim) = fired[0];
        assert!((at.z - g.body.pos.z - 4.0).abs() < 0.2 && (at.x - g.body.pos.x).abs() < 0.2, "4 blocks toward +z: {at:?}");
        assert!((at.y - g.body.pos.y - 0.5).abs() < 1e-4);
        assert!(aim.z > 15.0 && aim.y < 0.0, "aimed at the player: {aim:?}");
        // Behind a wall it loses count instead.
        let wall = |x: i32, y: i32, z: i32| if (40..44).contains(&z) { Some(1) } else { flat(x, y, z) };
        let c2 = Ctx { get: &wall, light: &l, day: false, player, eye: player + Vec3::Y * 0.6, biome: &|_, _| Biome::Plains, sight: 1.0 };
        g.charge = 5;
        g.tick(&mut rng, &c2, &mut Vec::new());
        assert_eq!(g.charge, 4);
        // Gravity is cancelled: a second of frames at rest and it has not dropped. Dying leaves 0..2 gunpowder.
        g.body.vel = Vec3::ZERO;
        let y0 = g.body.pos.y;
        for _ in 0..20 {
            g.step(0.05, &flat);
        }
        assert!((g.body.pos.y - y0).abs() < 0.5, "{} -> {}", y0, g.body.pos.y);
        let loot = g.loot(&mut rng);
        assert!(loot.len() <= 2 && loot.iter().all(|s| s.id == 289), "{}", loot.len());
    }

    /// `EntityFireball`: from rest it speeds up by 0.1 a tick with 0.95 drag (limit 1.9), bursts where a block is in its way (the place it was at the
    /// start of the tick), also at the player and at a mob (which is hit for 0), turns round when punched, and a ghast does not burst its own for 25 ticks.
    #[test]
    fn a_fireball_accelerates_bursts_on_what_it_meets_and_turns_when_punched() {
        let l = |_, _, _| 15u8;
        let far = Vec3::new(5.5, 40.0, 5.5);
        let c = ctx(far, &l);
        let mut rng = JavaRandom::new(9);
        let mut list: Vec<Mob> = Vec::new();
        // Down onto the grass (the top of it is y = 10).
        let mut f = Fireball::new(Vec3::new(20.5, 20.0, 20.5), Vec3::new(0.0, -30.0, 0.0), u32::MAX, &mut rng);
        let mut ev = Vec::new();
        f.tick(&c, &mut list, &[], &mut rng, &mut ev);
        assert!(matches!(ev[..], [Ev::Smoke(p)] if (p.y - 20.5).abs() < 1e-3), "a puff half a block above it each tick: {}", ev.len());
        let mut f = Fireball::new(Vec3::new(20.5, 20.0, 20.5), Vec3::new(0.0, -30.0, 0.0), u32::MAX, &mut rng);
        let mut ev = Vec::new();
        let mut speeds = Vec::new();
        let mut alive = true;
        for _ in 0..60 {
            alive = f.tick(&c, &mut list, &[], &mut rng, &mut ev);
            speeds.push(f.vel.length());
            if !alive { break }
        }
        assert!(!alive);
        assert!(speeds[0] > 0.09 && speeds[0] < 0.11 && speeds[1] > speeds[0], "{speeds:?}");
        assert!(speeds.iter().all(|&s| s < 1.95), "{speeds:?}");
        let at = ev.iter().find_map(|e| if let Ev::Burst(p) = e { Some(*p) } else { None }).expect("it burst");
        assert!(at.y > 9.0 && at.y < 13.0 && (at.x - 20.5).abs() < 2.0, "{at:?}");
        // At the player 20 blocks off, level.
        let player = Vec3::new(32.5, 15.9, 52.5);
        let c = ctx(player, &l);
        let mut f = Fireball { pos: Vec3::new(32.5, 15.5, 32.5), vel: Vec3::ZERO, acc: Vec3::new(0.0, 0.0, 0.1), age: 0, owner: u32::MAX };
        let mut ev = Vec::new();
        for _ in 0..60 {
            if !f.tick(&c, &mut list, &[], &mut rng, &mut ev) { break }
        }
        let at = ev.iter().find_map(|e| if let Ev::Burst(p) = e { Some(*p) } else { None }).expect("it burst");
        assert!((at - player).length() < 4.0, "{at:?}");
        // At a pig: the pig is hit for 0 (it still turns and is knocked) and the fireball is gone.
        let mut pig = Mob::new(Pig, 32.5, 15.0, 45.5, 0.0, &mut rng);
        pig.life = 0;
        let mut list = vec![pig];
        let mut f = Fireball { pos: Vec3::new(32.5, 15.5, 32.5), vel: Vec3::ZERO, acc: Vec3::new(0.0, 0.0, 0.1), age: 0, owner: u32::MAX };
        let mut ev = Vec::new();
        for _ in 0..60 {
            if !f.tick(&ctx(Vec3::new(5.5, 40.0, 5.5), &l), &mut list, &[], &mut rng, &mut ev) { break }
        }
        assert!(ev.iter().any(|e| matches!(e, Ev::Burst(_))) && list[0].health == 10 && list[0].hurt > 0, "{}", list[0].health);
        // Punched: it goes the way the player looks, accelerating along it.
        let mut m = Mobs::new(1);
        m.fireballs.push(Fireball { pos: Vec3::new(20.5, 20.0, 20.5), vel: Vec3::ZERO, acc: Vec3::new(0.0, -0.1, 0.0), age: 0, owner: u32::MAX });
        assert_eq!(m.pick_fireball(Vec3::new(20.5, 20.5, 17.5), Vec3::Z, 4.0).map(|p| p.0), Some(0));
        m.deflect(0, Vec3::X);
        let c = ctx(far, &l);
        let x0 = m.fireballs[0].pos.x;
        m.fireballs[0].tick(&c, &mut [], &[], &mut rng, &mut Vec::new());
        assert!(m.fireballs[0].pos.x > x0 + 0.9 && m.fireballs[0].vel.x > 0.9, "{:?}", m.fireballs[0].vel);
        // Its own ghast is passed for 25 ticks, then it counts.
        let g = Mob::new(Ghast, 20.5, 20.0, 20.5, 0.0, &mut rng);
        let mut list = vec![g];
        let mut f = Fireball { pos: Vec3::new(20.5, 20.0, 20.5), vel: Vec3::ZERO, acc: Vec3::new(0.0, -0.1, 0.0), age: 0, owner: list[0].id };
        let mut ev = Vec::new();
        f.tick(&c, &mut list, &[], &mut rng, &mut ev);
        assert!(!ev.iter().any(|e| matches!(e, Ev::Burst(_))), "a young fireball passes its ghast");
        f.age = 30;
        f.tick(&c, &mut list, &[], &mut rng, &mut ev);
        assert!(ev.iter().any(|e| matches!(e, Ev::Burst(_))), "an old one hits it");
        // Another ghast is hit at once: only the owner is passed.
        let mut ev = Vec::new();
        let mut f = Fireball { pos: Vec3::new(20.5, 20.0, 20.5), vel: Vec3::ZERO, acc: Vec3::new(0.0, -0.1, 0.0), age: 0, owner: list[0].id + 1_000_000 };
        f.tick(&c, &mut list, &[], &mut rng, &mut ev);
        assert!(ev.iter().any(|e| matches!(e, Ev::Burst(_))), "a young fireball hits another ghast");
    }

    /// b1.7.3 `EntityFireball`: a boat or a minecart (`canBeCollidedWith`) stops it like a mob does.
    #[test]
    fn a_fireball_bursts_on_a_boat_or_cart() {
        let l = |_, _, _| 15u8;
        let c = ctx(Vec3::new(5.5, 40.0, 5.5), &l);
        let mut rng = JavaRandom::new(4);
        let boat = [(Vec3::new(31.75, 15.2, 40.75), Vec3::new(33.25, 15.8, 42.25))];
        let go = |targets: &[(Vec3, Vec3)]| {
            let mut f = Fireball { pos: Vec3::new(32.5, 15.5, 32.5), vel: Vec3::ZERO, acc: Vec3::new(0.0, 0.0, 0.1), age: 0, owner: u32::MAX };
            let (mut ev, mut rng) = (Vec::new(), JavaRandom::new(4));
            let at = (0..80).find_map(|_| { f.tick(&c, &mut [], targets, &mut rng, &mut ev).then_some(()).map_or(Some(f.pos.z), |_| None) });
            at
        };
        let _ = &mut rng;
        let z = go(&boat).expect("it burst");
        assert!(z > 39.0 && z < 41.0, "{z}");
        assert!(go(&[]).map_or(true, |z| z > 42.5), "without it the ball flies on");
    }

    /// b1.7.3 `Explosion.doExplosionA`: size doubled, damage `(e^2 + e) / 2 x 8 x size + 1` with `e = (1 - distance / size) x exposure`, and the
    /// entity pushed along the way from the centre by `e` blocks a tick; a wall between takes the exposure (and the push) away.
    #[test]
    fn a_blast_hurts_and_pushes_by_distance_and_cover() {
        let at = Vec3::new(32.5, 11.0, 32.5);
        let (lo, hi) = (Vec3::new(32.2, 10.0, 36.2), Vec3::new(32.8, 11.8, 36.8));
        let feet = Vec3::new(32.5, 10.0, 36.5);
        let (dmg, push) = blast(&flat, at, 3.0, feet, lo, hi).expect("in range");
        // r = |(0, -1, 4)| / 6; e = 1 - r; the box is in the open.
        let e = 1.0 - 17f32.sqrt() / 6.0;
        assert_eq!(dmg, ((e * e + e) / 2.0 * 8.0 * 6.0 + 1.0) as i32);
        assert!(push.z > 0.0 && push.y < 0.0 && (push.length() - e * 20.0).abs() < 1e-3, "{push:?}");
        assert!(blast(&flat, at, 3.0, feet + Vec3::Z * 3.0, lo + Vec3::Z * 3.0, hi + Vec3::Z * 3.0).is_none(), "out of range");
        let walled = |x: i32, y: i32, z: i32| if z == 34 && y >= 9 { Some(1) } else { flat(x, y, z) };
        let (dmg2, push2) = blast(&walled, at, 3.0, feet, lo, hi).unwrap();
        assert!(dmg2 == 1 && push2.length() < 1e-3, "{dmg2} {push2:?}");
    }

    /// b1.7.3: a creeper's or TNT's blast (`exploder`) knocks a mob back and pushes it; a fireball's (`null`) only pushes it.
    #[test]
    fn a_fireball_blast_pushes_without_a_knockback() {
        let at = Vec3::new(32.5, 11.0, 32.5);
        let run = |exploder: bool| {
            let mut rng = JavaRandom::new(5);
            let mut m = Mobs::new(1);
            let mut cow = Mob::new(Cow, 32.5, 10.4, 33.5, 0.0, &mut rng);
            cow.health = 100;
            m.list.push(cow);
            m.explode(&flat, at, 1.0, exploder);
            m.list[0].body.vel
        };
        let (hit, bare) = (run(true), run(false));
        assert!(bare.z > 0.0 && hit.z > bare.z + 1.0, "knockBack adds to the push: {hit:?} {bare:?}");
    }

    /// NOT b1.7.3 (`animalAi`, proposal 5): a sheep that finishes grazing says which cell it ate.
    #[test]
    fn a_grazing_sheep_reports_the_cell_it_ate() {
        let l = |_, _, _| 15u8;
        let c = ctx(Vec3::new(60.5, 11.0, 60.5), &l);
        let mut rng = JavaRandom::new(1);
        let (mut m, mut drops) = (Mobs::new(2), Drops::new(1));
        (m.no_spawn, m.smart) = (true, true);
        let mut sheep = Mob::new(Sheep, 20.5, 10.0, 20.5, 0.0, &mut rng);
        sheep.graze = 3;
        m.list.push(sheep);
        let ate: Vec<(i32, i32, i32)> = (0..10).flat_map(|_| m.update(TICK, &c, &mut drops)).filter_map(|e| if let Ev::Graze(c) = e { Some(c) } else { None }).collect();
        assert_eq!(ate, [(20, 10, 20)], "once, in the cell it stands in");
    }
}
