//! Achievements and Statistics: `AchievementList`, `Achievement`, `StatList`, `StatBase` formats and the counting of `StatFileWriter`
//! (`readStat` / `hasAchievementUnlocked` / `func_27181_b`), with the original stat ids (`statId`) as the keys. UNVERIFIED: not compiled, not run.
//! Not b1.7.3: the file (`<root>/stats`, own format; the Java's is a checksummed JSON that `StatsSyncher` merges), and the text of the
//! inventory hint (a touch screen has no 'E' key). One counter map serves every stat, so there is no `StatList` object graph.

use std::collections::HashMap;

pub const START_GAME: u32 = 1000;
pub const CREATE_WORLD: u32 = 1001;
pub const LOAD_WORLD: u32 = 1002;
pub const LEAVE_GAME: u32 = 1004;
pub const MINUTES: u32 = 1100;
/// `distanceWalkedStat` .. `distanceByPigStat` are 2000..=2008 (see `Dist`), `jumpStat` .. `fishCaughtStat` follow.
pub const JUMP: u32 = 2010;
pub const DROP: u32 = 2011;
pub const DAMAGE_DEALT: u32 = 2020;
pub const DAMAGE_TAKEN: u32 = 2021;
pub const DEATHS: u32 = 2022;
pub const MOB_KILLS: u32 = 2023;
pub const FISH: u32 = 2025;
const ACH: u32 = 5_242_880;
const MINE: u32 = 16_777_216;
const CRAFT: u32 = 16_842_752;
const USE: u32 = 16_908_288;
const BREAK: u32 = 16_973_824;

/// `Dist` stat ids, offsets from 2000.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dist { Walk, Swim, Fall, Climb, Fly, Dive, Cart, Boat, Pig }

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Fmt { Plain, Time, Dist }

/// `field_25187_b`: the General list, in registration order (id, `stat.*` text, format).
pub const GENERAL: [(u32, &str, Fmt); 23] = [
    (1000, "Times played", Fmt::Plain), (1001, "Worlds played", Fmt::Plain), (1002, "Saves loaded", Fmt::Plain),
    (1003, "Multiplayer joins", Fmt::Plain), (1004, "Games quit", Fmt::Plain), (1100, "Minutes Played", Fmt::Time),
    (2000, "Distance Walked", Fmt::Dist), (2001, "Distance Swum", Fmt::Dist), (2002, "Distance Fallen", Fmt::Dist),
    (2003, "Distance Climbed", Fmt::Dist), (2004, "Distance Flown", Fmt::Dist), (2005, "Distance Dove", Fmt::Dist),
    (2006, "Distance by Minecart", Fmt::Dist), (2007, "Distance by Boat", Fmt::Dist), (2008, "Distance by Pig", Fmt::Dist),
    (2010, "Jumps", Fmt::Plain), (2011, "Items Dropped", Fmt::Plain), (2020, "Damage Dealt", Fmt::Plain),
    (2021, "Damage Taken", Fmt::Plain), (2022, "Number of Deaths", Fmt::Plain), (2023, "Mob Kills", Fmt::Plain),
    (2024, "Player Kills", Fmt::Plain), (2025, "Fish Caught", Fmt::Plain)
];

pub struct Ach {
    pub name: &'static str,
    pub desc: &'static str,
    pub col: i32,
    pub row: i32,
    /// The item (or block) id of `theItemStack`.
    pub icon: u16,
    pub parent: Option<usize>,
    pub special: bool,
}

pub const OPEN_INVENTORY: usize = 0;
pub const MINE_WOOD: usize = 1;
pub const BUILD_WORKBENCH: usize = 2;
pub const BUILD_PICKAXE: usize = 3;
pub const BUILD_FURNACE: usize = 4;
pub const ACQUIRE_IRON: usize = 5;
pub const BUILD_HOE: usize = 6;
pub const MAKE_BREAD: usize = 7;
pub const BAKE_CAKE: usize = 8;
pub const BUILD_BETTER_PICKAXE: usize = 9;
pub const COOK_FISH: usize = 10;
pub const ON_A_RAIL: usize = 11;
pub const BUILD_SWORD: usize = 12;
pub const KILL_ENEMY: usize = 13;
pub const KILL_COW: usize = 14;
pub const FLY_PIG: usize = 15;

const fn a(name: &'static str, desc: &'static str, col: i32, row: i32, icon: u16, parent: Option<usize>, special: bool) -> Ach {
    Ach { name, desc, col, row, icon, parent, special }
}

/// `AchievementList`, same order and places; the texts are `lang/stats_US.lang`'s.
pub const ACHS: [Ach; 16] = [
    a("Taking Inventory", "Tap the inventory button.", 0, 0, 340, None, false), // UNVERIFIED text: the Java says to press 'E'
    a("Getting Wood", "Attack a tree until a block of wood pops out", 2, 1, 17, Some(0), false),
    a("Benchmarking", "Craft a workbench with four blocks of planks", 4, -1, 58, Some(1), false),
    a("Time to Mine!", "Use planks and sticks to make a pickaxe", 4, 2, 270, Some(2), false),
    a("Hot Topic", "Construct a furnace out of eight stone blocks", 3, 4, 62, Some(3), false),
    a("Acquire Hardware", "Smelt an iron ingot", 1, 4, 265, Some(4), false),
    a("Time to Farm!", "Use planks and sticks to make a hoe", 2, -3, 290, Some(2), false),
    a("Bake Bread", "Turn wheat into bread", -1, -3, 297, Some(6), false),
    a("The Lie", "Wheat, sugar, milk and eggs!", 0, -5, 354, Some(6), false),
    a("Getting an Upgrade", "Construct a better pickaxe", 6, 2, 274, Some(3), false),
    a("Delicious Fish", "Catch and cook fish!", 2, 6, 350, Some(4), false),
    a("On A Rail", "Travel by minecart at least 1 km from where you started", 2, 3, 66, Some(5), true),
    a("Time to Strike!", "Use planks and sticks to make a sword", 6, -1, 268, Some(2), false),
    a("Monster Hunter", "Attack and destroy a monster", 8, -1, 352, Some(12), false),
    a("Cow Tipper", "Harvest some leather", 7, -3, 334, Some(12), false),
    a("When Pigs Fly", "Fly a pig off a cliff", 8, -4, 329, Some(14), true),
];

/// `Block.disableStats` (`getEnableStats`): these have no "mined" stat.
pub fn mineable(id: u16) -> bool {
    !matches!(id, 7..=11 | 18 | 26 | 51 | 52 | 55 | 59 | 63 | 64 | 68 | 71 | 83 | 92..=94 | 96)
}

/// `StatList.replaceAllSimilarBlocks`: the block whose counter these share.
pub fn canon(id: u16) -> u16 {
    match id {
        91 => 86,
        62 => 61,
        74 => 73,
        76 => 75,
        40 => 39,
        43 => 44,
        2 | 60 => 3,
        _ => id,
    }
}

/// Which of the three counters of an item (or block) id: crafted, used, and mined (blocks) or depleted (items).
#[derive(Clone, Copy)]
pub enum Kind { Mined, Crafted, Used, Depleted }

#[derive(Default, PartialEq, Debug)]
pub struct Stats {
    map: HashMap<u32, u32>,
    /// Centimetres short of a whole one, per `Dist` (the Java adds whole `Math.round` cm each tick).
    rem: [f32; 9],
}

impl Stats {
    pub fn get(&self, stat: u32) -> u32 {
        self.map.get(&stat).copied().unwrap_or(0)
    }

    pub fn has(&self, a: usize) -> bool {
        self.get(ACH + a as u32) > 0
    }

    /// `func_27181_b`: the parent is taken (or there is none), so this one can be earned and is drawn lit.
    pub fn open(&self, a: usize) -> bool {
        ACHS[a].parent.map_or(true, |p| self.has(p))
    }

    /// `EntityPlayerSP.addStat` for a plain stat.
    pub fn add(&mut self, stat: u32, n: u32) {
        if n > 0 {
            let v = self.map.entry(stat).or_insert(0);
            *v = v.saturating_add(n);
        }
    }

    /// `addStat(achievement, 1)`: counted only once the parent is taken. True when it is newly taken (the toast).
    pub fn earn(&mut self, a: usize) -> bool {
        let new = self.open(a) && !self.has(a);
        if self.open(a) {
            self.add(ACH + a as u32, 1);
        }
        new
    }

    /// Distance in centimetres, fractions carried (`addMovementStat` rounds each tick; this rounds nothing away).
    pub fn walk(&mut self, d: Dist, cm: f32) {
        let r = &mut self.rem[d as usize];
        *r += cm;
        let n = r.floor();
        *r -= n;
        self.add(2000 + d as u32, n as u32);
    }

    fn key(kind: Kind, id: u16) -> u32 {
        let id = if id < 256 { canon(id) } else { id };
        id as u32 + match kind { Kind::Mined => MINE, Kind::Crafted => CRAFT, Kind::Used => USE, Kind::Depleted => BREAK }
    }

    pub fn count(&self, kind: Kind, id: u16) -> u32 {
        self.get(Self::key(kind, id))
    }

    pub fn bump(&mut self, kind: Kind, id: u16, n: u32) {
        if !matches!(kind, Kind::Mined) || mineable(id) {
            self.add(Self::key(kind, id), n);
        }
    }

    /// The ids of the Blocks (`blocks`) or Items tab with any count, by id (`SorterStats*` break ties that way).
    pub fn listed(&self, blocks: bool) -> Vec<u16> {
        let ks = if blocks { [Kind::Mined, Kind::Crafted, Kind::Used] } else { [Kind::Depleted, Kind::Crafted, Kind::Used] };
        let mut v: Vec<u16> = self
            .map
            .iter()
            .filter(|e| *e.1 > 0 && (MINE..BREAK + 65536).contains(e.0))
            .map(|e| ((*e.0 - MINE) % 65536) as u16)
            .filter(|&id| (id < 256) == blocks && canon(id) == id && ks.iter().any(|&k| self.count(k, id) > 0))
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// `[magic, count, (id, value)...]`, little endian.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = b"MCS1".to_vec();
        w.extend((self.map.len() as u32).to_le_bytes());
        for (k, v) in &self.map {
            w.extend(k.to_le_bytes());
            w.extend(v.to_le_bytes());
        }
        w
    }

    pub fn decode(b: &[u8]) -> Option<Stats> {
        let b = b.strip_prefix(b"MCS1")?;
        let n = u32::from_le_bytes(b.get(..4)?.try_into().ok()?) as usize;
        let body = b.get(4..)?;
        if body.len() != n.checked_mul(8)? {
            return None;
        }
        let map = body.chunks_exact(8).map(|c| (u32::from_le_bytes(c[..4].try_into().unwrap()), u32::from_le_bytes(c[4..].try_into().unwrap()))).collect();
        Some(Stats { map, rem: [0.0; 9] })
    }
}

/// `StatBase.func_27084_a` through the stat's `IStatType` (`StatTypeSimple` / `Time` / `Distance`).
pub fn fmt(f: Fmt, v: u32) -> String {
    let two = |x: f64, u: &str| format!("{x:.2} {u}");
    match f {
        Fmt::Plain => {
            let s = v.to_string();
            let mut out = String::new();
            for (i, c) in s.chars().enumerate() {
                if i > 0 && (s.len() - i) % 3 == 0 {
                    out.push(',');
                }
                out.push(c);
            }
            out
        }
        Fmt::Time => {
            let (s, m) = (v as f64 / 20.0, v as f64 / 1200.0);
            let (h, d) = (m / 60.0, m / 1440.0);
            if d / 365.0 > 0.5 { two(d / 365.0, "y") } else if d > 0.5 { two(d, "d") } else if h > 0.5 { two(h, "h") } else if m > 0.5 { two(m, "m") } else { format!("{s:?} s") }
        }
        Fmt::Dist => {
            let (m, km) = (v as f64 / 100.0, v as f64 / 100_000.0);
            if km > 0.5 { two(km, "km") } else if m > 0.5 { two(m, "m") } else { format!("{v} cm") }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Gating, merging, formats and the file round-trip, as the Java's rules give them.
    #[test]
    fn stats_like_java() {
        let mut s = Stats::default();
        assert!(!s.earn(BUILD_WORKBENCH), "no parent taken: not counted");
        assert!(s.earn(OPEN_INVENTORY) && !s.earn(OPEN_INVENTORY));
        assert!(s.earn(MINE_WOOD) && s.earn(BUILD_WORKBENCH));
        assert!(s.open(BUILD_PICKAXE) && !s.has(BUILD_PICKAXE));
        s.bump(Kind::Mined, 2, 3); // grass counts as dirt
        s.bump(Kind::Mined, 18, 1); // leaves have no stat
        s.bump(Kind::Crafted, 280, 4);
        assert_eq!((s.count(Kind::Mined, 3), s.listed(true), s.listed(false)), (3, vec![3], vec![280]));
        s.walk(Dist::Walk, 0.6);
        s.walk(Dist::Walk, 0.6);
        assert_eq!(s.get(2000), 1);
        assert_eq!(Stats::decode(&s.encode()), Some(Stats { rem: [0.0; 9], map: s.map.clone() }));
        assert!(Stats::decode(&s.encode()[..9]).is_none());
        assert_eq!((fmt(Fmt::Plain, 1234567).as_str(), fmt(Fmt::Dist, 250).as_str(), fmt(Fmt::Dist, 150_000).as_str()), ("1,234,567", "2.50 m", "1.50 km"));
        assert_eq!((fmt(Fmt::Time, 40).as_str(), fmt(Fmt::Time, 1800).as_str()), ("2.0 s", "1.50 m"));
    }
}
