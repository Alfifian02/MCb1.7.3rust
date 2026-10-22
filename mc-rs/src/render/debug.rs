//! The debug screen (pause menu > "Info"): two translucent panels in the GUI scale, made to read well in a screenshot.
//! Left = where the player is (position, chunk, facing, biome, light, time, the block aimed at); right = what the engine is
//! doing (FPS, frame time, resolution, render distance, chunks, LoD). Each panel is one dark rectangle of one width: a grey
//! label column, then the values, so a value is always found in the same place. Text is the game's own font (`HudPipeline`),
//! one font pixel = one GUI unit = `k` px, the `scaled_gs` scale the menus use (so it follows the GUI Scale option).
//! The rows are built in `lib.rs` (`App::debug_rows`); this file only lays them out. Written WITHOUT a Rust toolchain.

use super::hud::{HudPipeline, HudVertex};

pub const GREY: [f32; 4] = [0.69, 0.69, 0.69, 1.0];
pub const WHITE: [f32; 4] = [1.0; 4];
pub const GREEN: [f32; 4] = [0.33, 1.0, 0.33, 1.0];
pub const YELLOW: [f32; 4] = [1.0, 1.0, 0.33, 1.0];
pub const RED: [f32; 4] = [1.0, 0.33, 0.33, 1.0];
pub const CYAN: [f32; 4] = [0.33, 1.0, 1.0, 1.0];

/// One row of a panel.
pub enum Row {
    /// A heading in yellow, across the whole width.
    Head(String),
    /// A small vertical space.
    Gap,
    /// A grey label and a value in its own colour.
    Kv(&'static str, String, [f32; 4]),
}

/// A row is 10 GUI units tall (the font is 8, one unit above and below); a panel has 3 units of padding at the sides and 1 above
/// and below; the label column and the values are 5 units apart; the panels sit 2 units from the screen edge.
const PITCH: f32 = 10.0;
const PAD: f32 = 3.0;
const GAP: f32 = 4.0;
const LABEL_GAP: f32 = 5.0;
const EDGE: f32 = 2.0;
/// Black at 50%: dark enough for white text over snow or sky, light enough to keep the scene readable behind it.
const BACK: [f32; 4] = [0.0, 0.0, 0.0, 0.5];

/// (width of the label column, panel width, panel height) in GUI units.
fn size(rows: &[Row]) -> (f32, f32, f32) {
    let lab = rows.iter().filter_map(|r| if let Row::Kv(l, ..) = r { Some(HudPipeline::text_width(l)) } else { None }).max().unwrap_or(0) as f32;
    let w = rows
        .iter()
        .map(|r| match r {
            Row::Head(t) => HudPipeline::text_width(t) as f32,
            Row::Kv(_, v, _) => lab + LABEL_GAP + HudPipeline::text_width(v) as f32,
            Row::Gap => 0.0,
        })
        .fold(0.0, f32::max);
    let h: f32 = rows.iter().map(|r| if matches!(r, Row::Gap) { GAP } else { PITCH }).sum();
    (lab, w + 2.0 * PAD, h + 2.0)
}

fn panel(v: &mut Vec<HudVertex>, rows: &[Row], x: f32, y: f32, k: f32) {
    if rows.is_empty() {
        return;
    }
    let (lab, w, h) = size(rows);
    HudPipeline::push_quad(v, x * k, y * k, w * k, h * k, BACK);
    let mut ry = y + 1.0;
    for r in rows {
        match r {
            Row::Gap => ry += GAP,
            Row::Head(t) => {
                HudPipeline::push_text(v, (x + PAD) * k, (ry + 1.0) * k, k, t, YELLOW);
                ry += PITCH;
            }
            Row::Kv(l, val, c) => {
                HudPipeline::push_text(v, (x + PAD) * k, (ry + 1.0) * k, k, l, GREY);
                HudPipeline::push_text(v, (x + PAD + lab + LABEL_GAP) * k, (ry + 1.0) * k, k, val, *c);
                ry += PITCH;
            }
        }
    }
}

/// The two panels for a surface `w` px wide at `k` px per GUI unit: `left` at the top-left, `right` at the top-right. When they
/// would touch (a narrow screen) the right one goes under the left one instead.
pub fn draw(v: &mut Vec<HudVertex>, w: f32, k: f32, left: &[Row], right: &[Row]) {
    let gw = (w / k).ceil();
    let ((_, lw, lh), (_, rw, _)) = (size(left), size(right));
    panel(v, left, EDGE, EDGE, k);
    if gw - EDGE - rw >= EDGE + lw + 4.0 {
        panel(v, right, gw - EDGE - rw, EDGE, k);
    } else {
        panel(v, right, EDGE, EDGE + lh + 4.0, k);
    }
}

/// The compass point of a bearing in degrees (0 = north = -Z, 90 = east = +X, clockwise), 0..360.
pub fn compass(bearing: i32) -> &'static str {
    ["north", "north-east", "east", "south-east", "south", "south-west", "west", "north-west"][((bearing + 22) / 45 % 8) as usize]
}

/// The camera's yaw (radians, 0 looks toward -Z, as `FirstPersonCamera::forward`) as a bearing, 0..360.
pub fn bearing(yaw: f32) -> i32 {
    (yaw.to_degrees().rem_euclid(360.0).round() as i32) % 360
}

/// `World.getWorldTime` as a clock: tick 0 is 06:00 on day 1, 24000 ticks a day.
pub fn clock(ticks: u64) -> String {
    let t = ticks % 24000;
    format!("Day {}  {:02}:{:02}", ticks / 24000 + 1, (t / 1000 + 6) % 24, t % 1000 * 60 / 1000)
}

/// `SeasonalForest` -> `Seasonal Forest`.
pub fn words(name: &str) -> String {
    let mut s = String::new();
    for (i, c) in name.chars().enumerate() {
        if i > 0 && c.is_uppercase() {
            s.push(' ');
        }
        s.push(c);
    }
    s
}

/// Green from 50 fps, yellow from 25, red below.
pub fn fps_color(fps: u32) -> [f32; 4] {
    if fps >= 50 {
        GREEN
    } else if fps >= 25 {
        YELLOW
    } else {
        RED
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_read_right() {
        assert_eq!((compass(0), compass(44), compass(45), compass(90), compass(180), compass(270), compass(359)), ("north", "north-east", "north-east", "east", "south", "west", "north"));
        assert_eq!((bearing(0.0), bearing(std::f32::consts::FRAC_PI_2), bearing(-std::f32::consts::FRAC_PI_2), bearing(std::f32::consts::TAU * 3.0)), (0, 90, 270, 0));
        assert_eq!((clock(0), clock(6000), clock(18000 + 500), clock(24000 * 2 + 12000)), ("Day 1  06:00".into(), "Day 1  12:00".into(), "Day 1  00:30".into(), "Day 3  18:00".into()));
        assert_eq!(words("SeasonalForest"), "Seasonal Forest");
        assert_eq!((fps_color(60), fps_color(30), fps_color(10)), (GREEN, YELLOW, RED));
    }

    /// 'A' and 'B' are 6 units wide in the font: label column 6, value 12, plus the 5 between and 6 of padding; one row of 10 and 2 of margin.
    #[test]
    fn panel_size_follows_the_font() {
        assert_eq!(size(&[Row::Kv("A", "BB".into(), WHITE)]), (6.0, 29.0, 12.0));
        assert_eq!(size(&[Row::Head("A".into()), Row::Gap, Row::Kv("A", "B".into(), WHITE)]), (6.0, 23.0, 26.0));
        let (mut v, rows) = (Vec::new(), [Row::Kv("A", "B".into(), WHITE)]);
        draw(&mut v, 1000.0, 2.0, &rows, &[]);
        draw(&mut v, 1000.0, 2.0, &[], &rows);
        assert!(!v.is_empty() && v.iter().all(|p| p.pos[0] >= 0.0 && p.pos[0] <= 1000.0));
        v.clear();
        draw(&mut v, 200.0, 2.0, &rows, &rows); // gw = 100: both are 23 wide, they fit side by side
        let xs: Vec<f32> = v.iter().map(|p| p.pos[0]).collect();
        assert!(xs.iter().any(|&x| x >= 150.0), "the right panel sits at the right edge");
        v.clear();
        draw(&mut v, 60.0, 2.0, &rows, &rows); // gw = 30: 30 - 2 - 23 < 2 + 23 + 4, so the right panel goes under the left one
        assert!(v.iter().all(|p| p.pos[0] < 60.0));
    }
}
