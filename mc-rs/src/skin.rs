//! The player's look, NOT b1.7.3 (b1.7.3 downloads `skins/<name>.png` from minecraft.net, which is gone): the title screen's "Customize Skin"
//! picks a skin tone, a hair colour, a shirt and trousers from short palettes and repaints `mob/char.png` with them. The result is written
//! over skin slot 0 of the block atlas and of the HUD sheet (`Backend::upload_skin`), so the third-person model, the hand and the inventory
//! preview all wear it. The choice is kept in `<root>/skin.txt` (four numbers).
//!
//! How it repaints: each pixel of the original skin is sorted by where it is and what colour it is (`class`: skin, hair, shirt, trousers; eyes,
//! shoes, the mouth and the hat layer are left alone), and its brightness against the brightest pixel of its class (`REF`) is kept, so the
//! shading of the original (`f`) is painted again in the new colour. Index 0 of every palette is the original colour and repaints nothing, so a
//! fresh install shows `mob/char.png` byte for byte.
//! ponytail: no skin file import (a PNG decoder would be a new dependency; `apply` takes any 64 x 32 RGBA, so it is one `load` away).
//! UNVERIFIED: written without a Rust toolchain: not compiled, not run (the same steps were run in a Python twin and looked at).

use crate::render::atlas;
use std::path::Path;

/// Bytes of a 64 x 32 RGBA skin.
pub const BYTES: usize = 64 * 32 * 4;

/// The four things that can be changed, in the order of `Look`'s fields and of the buttons.
pub const SKIN: usize = 0;
pub const HAIR: usize = 1;
pub const SHIRT: usize = 2;
pub const PANTS: usize = 3;

type Palette = &'static [(&'static str, [u8; 3])];

/// Skin tone: the colour of the brightest pixel of the face and the hands (`REF[SKIN]` 170 is the original's).
const TONES: Palette = &[("Steve", [170, 125, 102]), ("Fair", [240, 190, 165]), ("Light", [222, 170, 135]), ("Tan", [198, 140, 90]), ("Olive", [190, 160, 100]), ("Brown", [141, 85, 59]), ("Deep", [100, 62, 42])];
const HAIRS: Palette = &[("Brown", [150, 95, 65]), ("Black", [80, 75, 75]), ("Blonde", [235, 205, 115]), ("Red", [200, 75, 40]), ("Gray", [175, 175, 175]), ("White", [240, 240, 240]), ("Pink", [240, 120, 180]), ("Blue", [70, 110, 210])];
const SHIRTS: Palette = &[("Cyan", [0, 175, 175]), ("Red", [210, 45, 45]), ("Orange", [235, 130, 30]), ("Yellow", [235, 215, 50]), ("Green", [60, 180, 60]), ("Blue", [60, 90, 220]), ("Purple", [150, 70, 200]), ("Pink", [240, 130, 190]), ("White", [235, 235, 235]), ("Black", [70, 70, 70])];
const PANTSES: Palette = &[("Blue", [86, 73, 204]), ("Black", [70, 70, 70]), ("Gray", [150, 150, 150]), ("Brown", [140, 95, 55]), ("Khaki", [200, 180, 120]), ("Green", [60, 130, 60]), ("Red", [190, 50, 50]), ("White", [230, 230, 230])];

const PALETTES: [Palette; 4] = [TONES, HAIRS, SHIRTS, PANTSES];
/// The brightest channel of the brightest pixel of each class in `mob/char.png`: the new colour is painted at this brightness.
const REF: [f32; 4] = [170.0, 150.0, 175.0, 204.0];

/// A choice in each palette (an index; 0 is the original).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Look(pub [u8; 4]);

impl Look {
    /// `<root>/skin.txt`: four numbers, any other text or a number out of range counts as 0.
    pub fn load(root: &Path) -> Self {
        let text = std::fs::read_to_string(root.join("skin.txt")).unwrap_or_default();
        let mut n = text.split_whitespace().map(|w| w.parse::<usize>().unwrap_or(0));
        Self(std::array::from_fn(|p| n.next().filter(|&i| i < PALETTES[p].len()).unwrap_or(0) as u8))
    }

    pub fn save(&self, root: &Path) {
        let [a, b, c, d] = self.0;
        if let Err(e) = std::fs::write(root.join("skin.txt"), format!("{a} {b} {c} {d}\n")) {
            log::warn!("skin.txt: {e}");
        }
    }

    /// The next colour of `part` (`SKIN`..`PANTS`), round to the first again.
    pub fn cycle(&mut self, part: usize) {
        self.0[part] = (self.0[part] + 1) % PALETTES[part].len() as u8;
    }

    /// The name of the colour chosen for `part`.
    pub fn name(&self, part: usize) -> &'static str {
        PALETTES[part][self.0[part] as usize].0
    }

    /// The skin as 64 x 32 RGBA, `BYTES` long.
    pub fn apply(&self) -> Vec<u8> {
        let mut px = base();
        if *self == Self::default() {
            return px;
        }
        let from = px.clone();
        for (i, p) in from.chunks_exact(4).enumerate() {
            let Some(c) = class(i % 64, i / 64, [p[0], p[1], p[2], p[3]]) else { continue };
            if self.0[c] == 0 {
                continue;
            }
            let f = p[0].max(p[1]).max(p[2]) as f32 / REF[c];
            let target = PALETTES[c][self.0[c] as usize].1;
            for k in 0..3 {
                px[i * 4 + k] = (target[k] as f32 * f).round().min(255.0) as u8;
            }
        }
        px
    }
}

/// `mob/char.png` itself: the rows of skin slot 0 in `atlas::ART`.
fn base() -> Vec<u8> {
    let row = atlas::ATLAS_W * 4;
    let top = (atlas::CHAR_Y - atlas::ART_Y) * row;
    (0..32).flat_map(|y| atlas::ART[top + y * row..top + y * row + 64 * 4].iter().copied()).collect()
}

/// Which palette the pixel (`x`, `y`) of the original skin belongs to, if any. The legs are x 0..16, y 16..32 (both share them), the body
/// 16..40 and the arms 40..56, the head x 0..32, y 0..16 (the hat layer at x 32.. is empty in the original and stays so).
fn class(x: usize, y: usize, [r, g, b, a]: [u8; 4]) -> Option<usize> {
    if a == 0 {
        return None;
    }
    let blue = b > g && b > r; // trousers, and the belt line at the foot of the body (the head's eyes are blue too: not repainted)
    let brown = r > g && g > b && r >= 20 && g < 200; // skin and hair (no white, no grey)
    let skinny = g as u32 * 100 > r as u32 * 70; // paler and pinker than hair
    match (x, y) {
        (0..=15, 16..=31) => blue.then_some(PANTS),
        (16..=55, 16..=31) if blue => Some(PANTS),
        (16..=55, 16..=31) if r < 24 && g >= 60 && g.abs_diff(b) < 12 => Some(SHIRT),
        (40..=55, 16..=31) => (brown && skinny).then_some(SKIN),
        (0..=31, 0..=15) => brown.then_some(if skinny { SKIN } else { HAIR }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_original_look_is_the_original_skin() {
        let px = Look::default().apply();
        assert_eq!(px.len(), BYTES);
        assert_eq!(px, base());
        assert!(px.chunks_exact(4).any(|p| p[3] == 255), "the skin is in the sheet");
    }

    /// Hair changes only the head, shirt only the body and the sleeves, and a repaint never touches a transparent pixel.
    #[test]
    fn each_choice_repaints_only_its_own_part() {
        let (orig, h) = (base(), Look([0, 3, 0, 0]).apply());
        let changed: Vec<usize> = (0..64 * 32).filter(|&i| orig[i * 4..i * 4 + 4] != h[i * 4..i * 4 + 4]).collect();
        assert!(changed.len() > 100, "{} pixels", changed.len());
        assert!(changed.iter().all(|&i| i % 64 < 32 && i / 64 < 16), "hair is the head");
        let s = Look([0, 0, 2, 0]).apply();
        assert!((0..64 * 32).filter(|&i| orig[i * 4..i * 4 + 4] != s[i * 4..i * 4 + 4]).all(|i| (16..56).contains(&(i % 64)) && i / 64 >= 16), "shirt is body and arms");
        for look in [Look([6, 7, 9, 7]), Look([1, 1, 1, 1])] {
            let p = look.apply();
            assert!((0..64 * 32).all(|i| orig[i * 4 + 3] == p[i * 4 + 3]), "alpha stays");
        }
    }

    /// The new colour is painted at the brightness of the old pixel: the brightest hair pixel (150, 95, 65) becomes the colour itself.
    #[test]
    fn the_brightest_pixel_becomes_the_chosen_colour() {
        let (orig, p) = (base(), Look([0, 2, 0, 0]).apply());
        let i = (0..64 * 32).find(|&i| orig[i * 4..i * 4 + 3] == [150, 95, 65] && class(i % 64, i / 64, [150, 95, 65, 255]) == Some(HAIR)).expect("a bright hair pixel");
        assert_eq!(p[i * 4..i * 4 + 3], HAIRS[2].1);
    }

    #[test]
    fn the_choice_is_kept_in_a_file() {
        let d = std::env::temp_dir().join(format!("mcrs-skin-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(Look::load(&d), Look::default(), "no file");
        let mut l = Look::default();
        l.cycle(HAIR);
        l.cycle(PANTS);
        l.save(&d);
        assert_eq!(Look::load(&d), Look([0, 1, 0, 1]));
        std::fs::write(d.join("skin.txt"), "99 x 2").unwrap();
        assert_eq!(Look::load(&d), Look([0, 0, 2, 0]), "out of range and junk count as the original");
        for p in 0..4 {
            let mut l = Look::default();
            for _ in 0..PALETTES[p].len() {
                l.cycle(p);
            }
            assert_eq!(l, Look::default(), "a full round comes back");
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}
