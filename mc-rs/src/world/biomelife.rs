//! NOT b1.7.3 (`biomeRules` in options.txt, on by default): what a biome does to play, from `roadmap/design.md` ("Ideas waiting": fog is
//! cover, heat and cold are stamina, seasons in the forests, every biome has one export). Pure functions and numbers; `lib.rs` feeds them
//! the biome, the light and the day, and `stamina`, `mobs`, `items` and the foliage tint read the answers. UNVERIFIED by eye: every number.
//!
//! Built here: proposal 2 (`sight`), 1 (`air`, `Air`), 4 (`season`, `foliage`, `leaf_odds`) and the parts of 6 that need no new block
//! (`Body::fed`, `Body::fur`; the marsh's clay is in `gen::populate`). Also the Gauges' `tint` and the clock's almanac line (`almanac`, `dusk_in`), `design.md` "QoL without a new meter". Proposal 5 lives in `mobs` (`animalAi`); 3 and the landmarks are not built.

use crate::world::biome::Biome::{self, *};
use crate::world::sky;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU8, Ordering};

/// Proposal 2, fog is cover: how far a mob sees you, as a fraction of its usual 16 blocks, in a fog of exp² `density` (`atmos`: plains
/// 0.0018, forest 0.003, wet woodland 0.006, marsh 0.0075, swamp 0.01). Clear air below 0.004 is 1; the thickest fog leaves 40%.
pub fn sight(density: f32) -> f32 {
    1.0 - 0.6 * ((density - 0.004) / 0.007).clamp(0.0, 1.0)
}

/// Proposal 1: how hot a biome is in open daylight, 0..=1 (the desert, savanna, shrubland and chaparral).
pub fn heat(b: Biome) -> f32 {
    match b {
        Desert => 1.0,
        Chaparral => 0.8,
        Savanna => 0.6,
        Shrubland => 0.5,
        _ => 0.0,
    }
}

/// Proposal 1: how cold a biome is away from a fire, 0..=1 (tundra, taiga and the snowy grove; the ice desert is never selected).
pub fn chill(b: Biome) -> f32 {
    match b {
        Tundra => 1.0,
        SnowyBirchGrove => 0.8,
        Taiga => 0.7,
        _ => 0.0,
    }
}

/// Block light from which a torch, a furnace or lava counts as a fire at hand (a torch gives 14 and fades by 1 a block: about 3 blocks).
pub const FIRE_LIGHT: u8 = 11;
/// Ticks a meal keeps the cold off (5 minutes).
pub const FED_TICKS: u32 = 6000;

/// What the player stands in, for `air`.
#[derive(Clone, Copy, Default, Debug)]
pub struct Body {
    /// Open daylight on the head, 0..=1: the day's light times the sky light at the head (shade, a roof and night give none).
    pub sun: f32,
    /// A fire at hand (`FIRE_LIGHT`).
    pub fire: bool,
    /// Ate within `FED_TICKS`: food for the cold (proposal 6: the autumnal forest's apples, the farms' bread, the cooked meat).
    pub fed: bool,
    /// Worn armor pieces of metal (chain, iron, gold, diamond) and of leather, the furs (proposal 6).
    pub metal: u8,
    pub fur: u8,
}

/// The weather on the body: `hot` and `cold`, 0 or more (stamina: a sprint costs `1 + 0.6 hot` times, recovery is `1 - 0.5 cold` times).
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Air {
    pub hot: f32,
    pub cold: f32,
}

/// Proposal 1: the desert, savanna, shrubland and chaparral drain stamina faster in open daylight (metal armor makes it worse by 10% a
/// piece); tundra, taiga and the snowy grove slow its recovery away from a fire (leather halves it at four pieces, a meal by half).
pub fn air(b: Biome, w: Body) -> Air {
    let hot = (heat(b) * w.sun * (1.0 + 0.1 * w.metal as f32)).min(1.5);
    let cold = chill(b) * if w.fire { 0.0 } else { 1.0 } * (1.0 - 0.125 * w.fur.min(4) as f32) * if w.fed { 0.5 } else { 1.0 };
    Air { hot, cold }
}

/// Gauges (NOT b1.7.3, UNVERIFIED): the colour the stamina's grey overlay takes. White in clean air, warmer with `hot`, cooler with `cold`
/// (each counts up to 1). Reads `Air` and nothing else, so the HUD joins the weather with no state of its own.
pub fn tint(a: Air) -> [f32; 3] {
    let (h, c) = (a.hot.clamp(0.0, 1.0), a.cold.clamp(0.0, 1.0));
    let (warm, cool) = ([1.0, 0.7, 0.4], [0.55, 0.8, 1.0]);
    std::array::from_fn(|i| 1.0 + h * (warm[i] - 1.0) + c * (cool[i] - 1.0))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Season {
    Spring,
    Summer,
    Autumn,
    Winter,
}

impl Season {
    pub fn name(self) -> &'static str {
        ["Spring", "Summer", "Autumn", "Winter"][self as usize]
    }
}

/// A season lasts this many days (a day is 24000 ticks, 20 minutes: an hour a season, a year of four hours).
pub const SEASON_DAYS: u64 = 3;

/// Proposal 4: the season of a world that has run `ticks` (a new world starts in spring).
pub fn season(ticks: u64) -> Season {
    match ticks / 24000 / SEASON_DAYS % 4 {
        0 => Season::Spring,
        1 => Season::Summer,
        2 => Season::Autumn,
        _ => Season::Winter,
    }
}

/// Almanac (NOT b1.7.3, UNVERIFIED): the clock item (`pocketSundial`, 347), which now also reads the calendar.
pub const CLOCK: u16 = 347;

/// Ticks to the next dusk on a clear sky: the first moment the sky takes 4 light away, the line `App::sleep_tick` wakes a sleeper at (so
/// it is also when a bed works). `None` when it is dark already. Steps of 5 s (100 ticks), which is finer than the minutes it is shown in.
pub fn dusk_in(ticks: u64) -> Option<u64> {
    let dark = |t: u64| sky::skylight_subtracted(sky::celestial_angle(t, 0.0), 0.0, 0.0) >= 4;
    if dark(ticks) {
        return None;
    }
    (1..=sky::TICKS_PER_DAY / 100).map(|i| i * 100).find(|&d| dark(ticks + d))
}

/// The line a held clock shows: the day, the season (only where `seasons` exist: `biomeRules`) and the minutes to dusk, or night.
pub fn almanac(ticks: u64, seasons: bool) -> String {
    let day = ticks / sky::TICKS_PER_DAY + 1;
    let head = if seasons { format!("Day {day}, {}", season(ticks).name()) } else { format!("Day {day}") };
    match dusk_in(ticks) {
        Some(d) => format!("{head}, dusk in {} min", d.div_ceil(1200)), // 1200 ticks a minute
        None => format!("{head}, night"),
    }
}

/// What `almanac_now` reads, set once a frame by `App` like `SEASON` is (the tooltip that shows it is made far from the world): the world's
/// tick (negative: no almanac, as in the Nether where the clock only spins) and whether the seasons exist.
static NOW: AtomicI64 = AtomicI64::new(-1);
static SEASONS: AtomicBool = AtomicBool::new(false);

pub fn set_now(ticks: Option<u64>, seasons: bool) {
    NOW.store(ticks.map_or(-1, |t| t as i64), Ordering::Relaxed);
    SEASONS.store(seasons, Ordering::Relaxed);
}

/// The almanac line for this frame, `None` where there is no almanac.
pub fn almanac_now() -> Option<String> {
    u64::try_from(NOW.load(Ordering::Relaxed)).ok().map(|t| almanac(t, SEASONS.load(Ordering::Relaxed)))
}

/// The season the foliage tint of new meshes reads (`chunks::biome_tints`, worker threads too), so `set_season` tells the caller when
/// every loaded mesh must restart. Summer is the colour the climate gives, and what a world with the rules off shows.
static SEASON: AtomicU8 = AtomicU8::new(1);

/// Make `s` the season of the foliage; true when it was another one.
pub fn set_season(s: Season) -> bool {
    SEASON.swap(s as u8, Ordering::Relaxed) != s as u8
}

pub fn current() -> Season {
    match SEASON.load(Ordering::Relaxed) {
        0 => Season::Spring,
        2 => Season::Autumn,
        3 => Season::Winter,
        _ => Season::Summer,
    }
}

/// `a` towards `b` by `t`, per channel of a packed `0xRRGGBB`.
fn mix(a: u32, b: u32, t: f32) -> u32 {
    (0..3).rev().fold(0, |c, i| {
        let (x, y) = (((a >> (i * 8)) & 255) as f32, ((b >> (i * 8)) & 255) as f32);
        c << 8 | (x + (y - x) * t).round() as u32
    })
}

/// Proposal 4: the oak foliage of the seasonal forest and the autumnal forest follows the season: fresher in spring, as the climate gives
/// it in summer, orange in autumn, brown in winter. Spruce and birch leaves keep their fixed colours.
pub fn foliage(base: u32, b: Biome, s: Season) -> u32 {
    let own = if b == AutumnalForest { 0.5 } else { 1.0 }; // the autumnal forest is copper to begin with
    match (b, s) {
        (SeasonalForest | AutumnalForest, Season::Spring) => mix(base, 0x7FCB4A, 0.25),
        (SeasonalForest | AutumnalForest, Season::Autumn) => mix(base, 0xD9701E, 0.6 * own),
        (SeasonalForest | AutumnalForest, Season::Winter) => mix(base, 0x8A7355, 0.55),
        _ => base,
    }
}

/// Proposal 4 and 6: what a broken leaf gives, as odds of 1 in n: (sapling, apple). b1.7.3's sapling is 1 in 20 and it has no apple; here
/// an oak leaf gives an apple 1 in 200, and the two forests have seasons: saplings fall in spring and autumn, apples ripen in summer and
/// peak in autumn (the autumnal forest, the apple's biome, at twice the rate).
pub fn leaf_odds(b: Biome, s: Season) -> (i32, i32) {
    match (b, s) {
        (SeasonalForest, Season::Spring) => (12, 400),
        (SeasonalForest, Season::Summer) => (20, 150),
        (SeasonalForest, Season::Autumn) => (14, 60),
        (SeasonalForest, Season::Winter) => (40, 800),
        (AutumnalForest, Season::Spring) => (12, 300),
        (AutumnalForest, Season::Summer) => (20, 100),
        (AutumnalForest, Season::Autumn) => (14, 30),
        (AutumnalForest, Season::Winter) => (40, 600),
        _ => (20, 200),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Clear air is 1, the marsh and swamp shorten a mob's sight, nothing takes it under 40%.
    #[test]
    fn fog_is_cover() {
        assert_eq!((sight(0.0018), sight(0.003), sight(0.004)), (1.0, 1.0, 1.0));
        assert!(sight(0.006) < 1.0 && sight(0.0075) < sight(0.006) && sight(0.01) < sight(0.0075));
        assert!((sight(0.5) - 0.4).abs() < 1e-6);
        assert!((16.0 * sight(0.01)) < 9.0, "a swamp mob sees under 9 blocks");
    }

    /// Heat needs sun and a hot biome; cold needs a cold biome and no fire, and furs and a meal soften it.
    #[test]
    fn heat_and_cold() {
        let day = Body { sun: 1.0, ..Body::default() };
        assert_eq!(air(Desert, day), Air { hot: 1.0, cold: 0.0 });
        assert_eq!(air(Desert, Body::default()).hot, 0.0, "night and shade are cool");
        assert_eq!(air(Plains, day), Air::default());
        assert!(air(Savanna, Body { metal: 4, ..day }).hot > air(Savanna, day).hot, "metal armor is hot");
        let cold = air(Tundra, Body::default());
        assert_eq!(cold, Air { hot: 0.0, cold: 1.0 });
        assert_eq!(air(Tundra, Body { fire: true, ..Body::default() }).cold, 0.0);
        assert!(air(Tundra, Body { fur: 4, ..Body::default() }).cold == 0.5 && air(Tundra, Body { fed: true, ..Body::default() }).cold == 0.5);
        assert!(air(Taiga, Body::default()).cold < air(Tundra, Body::default()).cold);
        assert_eq!(air(Tundra, Body { fur: 9, fed: true, ..Body::default() }).cold, 0.25, "four pieces count");
    }

    // Gauges: clean air leaves the overlay white, heat turns it warm (red over blue) and cold turns it cool.
    #[test]
    fn the_overlay_tint_follows_the_air() {
        assert_eq!(tint(Air::default()), [1.0; 3]);
        let (h, c) = (tint(Air { hot: 1.0, cold: 0.0 }), tint(Air { hot: 0.0, cold: 1.0 }));
        assert!(h[0] > h[2] && c[2] > c[0]);
        assert_eq!(tint(Air { hot: 9.0, cold: 0.0 }), h, "capped at 1");
    }

    // Almanac: dusk is where the sky first takes 4 light away (the bed's line), found by 5 s steps; the line has the day, the season and the wait.
    #[test]
    fn the_almanac_counts_to_dusk_and_names_the_day() {
        let d = dusk_in(0).expect("morning is light");
        assert!((9000..14000).contains(&d) && d % 100 == 0, "{d}");
        assert_eq!(dusk_in(18000), None, "midnight is dark");
        assert!(almanac(0, true).starts_with("Day 1, Spring, dusk in "));
        assert_eq!(almanac(0, false).split(',').count(), 2, "no season where the seasons are off");
        assert_eq!(almanac(24000 * 3 + 18000, true), "Day 4, Summer, night");
        assert_eq!(almanac(24000 * 9, true).split(',').nth(1), Some(" Winter"));
    }

    /// Three days a season, four seasons, spring first; the foliage follows and summer is the plain climate colour.
    #[test]
    fn seasons_turn() {
        let day = 24000;
        assert_eq!(season(0), Season::Spring);
        assert_eq!(season(3 * day - 1), Season::Spring);
        assert_eq!((season(3 * day), season(6 * day), season(9 * day), season(12 * day)), (Season::Summer, Season::Autumn, Season::Winter, Season::Spring));
        let base = 0x48B518;
        assert_eq!(foliage(base, SeasonalForest, Season::Summer), base);
        assert_eq!(foliage(base, Plains, Season::Autumn), base, "only the two forests turn");
        let fall = foliage(base, SeasonalForest, Season::Autumn);
        assert!((fall >> 16) & 255 > (base >> 16) & 255 && (fall >> 8) & 255 < (base >> 8) & 255, "greener becomes redder: {fall:06x}");
        assert!(foliage(base, AutumnalForest, Season::Autumn) != fall, "the copper wood turns less");
        assert_eq!(mix(0x000000, 0xFFFFFF, 0.5), 0x808080);
    }

    /// The seasonal flag reports a change once.
    #[test]
    fn the_season_flag_reports_a_change() {
        // (a global: put back at once, summer being what every other test reads)
        assert!(set_season(Season::Spring) && current() == Season::Spring);
        assert!(!set_season(Season::Spring));
        assert!(set_season(Season::Summer) && current() == Season::Summer);
    }

    /// Autumn is the apple's peak, and the autumnal forest gives more of them than the seasonal one all year.
    #[test]
    fn apples_peak_in_autumn() {
        for b in [SeasonalForest, AutumnalForest] {
            let apples: Vec<i32> = [Season::Spring, Season::Summer, Season::Autumn, Season::Winter].iter().map(|&s| leaf_odds(b, s).1).collect();
            assert_eq!(apples.iter().copied().min(), Some(apples[2]), "{b:?}: autumn is the best");
            assert_eq!(apples.iter().copied().max(), Some(apples[3]), "{b:?}: winter the worst");
        }
        for s in [Season::Spring, Season::Summer, Season::Autumn, Season::Winter] {
            assert!(leaf_odds(AutumnalForest, s).1 < leaf_odds(SeasonalForest, s).1);
        }
        assert_eq!(leaf_odds(Plains, Season::Winter), (20, 200), "elsewhere the odds do not turn");
    }
}
