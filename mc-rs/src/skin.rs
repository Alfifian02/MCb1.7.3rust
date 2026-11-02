//! The player's look, NOT b1.7.3 (b1.7.3 downloads `skins/<name>.png` from minecraft.net, which is gone): the title screen's "Customize Skin"
//! picks a skin tone, a hair colour, a shirt and trousers from short palettes and repaints `mob/char.png` with them. The result is written
//! over skin slot 0 of the block atlas and of the HUD sheet (`Backend::upload_skin`), so the third-person model, the hand and the inventory
//! preview all wear it. The choice is kept in `<root>/skin.txt` (four numbers).
//!
//! How it repaints: each pixel of the original skin is sorted by where it is and what colour it is (`class`: skin, hair, shirt, trousers; eyes,
//! shoes, the mouth and the hat layer are left alone), and its brightness against the brightest pixel of its class (`REF`) is kept, so the
//! shading of the original (`f`) is painted again in the new colour. Index 0 of every palette is the original colour and repaints nothing, so a
//! fresh install shows `mob/char.png` byte for byte.
//! Import Custom Skin (NOT b1.7.3): `import` takes the newest `.png` of the app's folder in shared storage (`IMPORT_DIR`, set at start-up) or of the
//! private folder, `from_png` makes a 64 x 32 RGBA of it (a 64 x 32 skin as it is; a 64 x 64 one of 1.8 and later by its top half, which holds the
//! head, the body, the right arm and the right leg: `ModelBiped` mirrors them for the left side, like the old skins), and a copy is kept as
//! `<root>/custom_skin.png` (`load_custom`) until Reset. A custom skin replaces the palette look; `png` is the one new dependency.
//! ponytail: 3 px "slim" arms are drawn with the 4 px model, and the 1.8 overlay layers (jacket, sleeves, trousers) are not used.
//! UNVERIFIED: written without a Rust toolchain: not compiled, not run (the same steps were run in a Python twin and looked at).

use crate::render::atlas;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The folder `import` looks in first (the app's folder in shared storage, `Android/data/<package>/files`, which a file manager or `adb push`
/// can reach): set once at start-up, so it is absent on a host and the private folder is the only place.
pub static IMPORT_DIR: OnceLock<PathBuf> = OnceLock::new();

/// The imported skin as it was kept (a copy of the file that was picked) in the private folder.
const CUSTOM: &str = "custom_skin.png";

/// A skin file bigger than this is not a skin.
const MAX_FILE: u64 = 1 << 20;

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

/// The 64 x 32 RGBA skin of a PNG: 64 x 32, or 64 x 64 (its top 32 rows), any colour type and depth.
pub fn from_png(bytes: &[u8]) -> Result<Vec<u8>, &'static str> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut r = dec.read_info().map_err(|_| "Not a PNG file")?;
    let (w, h) = (r.info().width, r.info().height);
    if w != 64 || (h != 32 && h != 64) {
        return Err("Skin must be 64x32 or 64x64"); // checked before the buffer is made: the header is the file's word
    }
    let mut buf = vec![0; r.output_buffer_size()];
    let f = r.next_frame(&mut buf).map_err(|_| "Not a PNG file")?;
    let px = buf[..f.buffer_size()].chunks_exact(f.color_type.samples()).take(64 * 32).flat_map(|p| match f.color_type {
        png::ColorType::Grayscale => [p[0], p[0], p[0], 255],
        png::ColorType::GrayscaleAlpha => [p[0], p[0], p[0], p[1]],
        png::ColorType::Rgb => [p[0], p[1], p[2], 255],
        _ => [p[0], p[1], p[2], p[3]], // Rgba (an indexed one is expanded to Rgb / Rgba)
    });
    Ok(px.collect())
}

/// The kept custom skin, if there is one and it still reads.
pub fn load_custom(root: &Path) -> Option<Vec<u8>> {
    from_png(&std::fs::read(root.join(CUSTOM)).ok()?).ok()
}

/// Reset: forget the custom skin.
pub fn clear_custom(root: &Path) {
    let _ = std::fs::remove_file(root.join(CUSTOM));
}

/// Import Custom Skin: the newest `.png` of `IMPORT_DIR` and of `root`, as a skin and its file name; kept as `root/custom_skin.png`.
pub fn import(root: &Path) -> Result<(Vec<u8>, String), &'static str> {
    let dirs: Vec<&Path> = IMPORT_DIR.get().map(PathBuf::as_path).into_iter().chain([root]).collect();
    import_from(&dirs, root)
}

fn import_from(dirs: &[&Path], root: &Path) -> Result<(Vec<u8>, String), &'static str> {
    let (_, path) = dirs
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flatten()
        .flatten()
        .filter(|e| e.file_name() != CUSTOM && e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("png")))
        .filter_map(|e| Some((e.metadata().ok().filter(|m| m.len() <= MAX_FILE)?.modified().ok()?, e.path())))
        .max_by_key(|(t, _)| *t)
        .ok_or("No .png in the skin folder")?;
    let bytes = std::fs::read(&path).map_err(|_| "Cannot read the file")?;
    let px = from_png(&bytes)?;
    std::fs::write(root.join(CUSTOM), &bytes).map_err(|_| "Cannot keep the skin")?;
    Ok((px, path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned())))
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
pub(crate) mod tests {
    use super::*;

    /// A PNG of `w` x `h` pixels in `color` (8 bit), for the tests here and in `menu`.
    pub(crate) fn png_of(w: u32, h: u32, color: png::ColorType, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(color);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().unwrap().write_image_data(data).unwrap();
        out
    }

    #[test]
    fn a_png_becomes_a_64_by_32_skin() {
        // 64 x 64 RGBA: the top half is the skin, the bottom half is left out.
        let big: Vec<u8> = (0..64 * 64 * 4).map(|i| (i / 4 % 251) as u8 | (i % 4 == 3) as u8 * 255).collect();
        let px = from_png(&png_of(64, 64, png::ColorType::Rgba, &big)).unwrap();
        assert_eq!(px.len(), BYTES);
        assert_eq!(px, big[..BYTES]);
        // 64 x 32 RGB gets full alpha; grey with alpha keeps it.
        let rgb: Vec<u8> = (0..64 * 32 * 3).map(|i| i as u8).collect();
        let px = from_png(&png_of(64, 32, png::ColorType::Rgb, &rgb)).unwrap();
        assert_eq!((px[..4].to_vec(), px[7]), (vec![0, 1, 2, 255], 255));
        let ga: Vec<u8> = (0..64 * 32 * 2).map(|i| if i % 2 == 0 { 9 } else { 77 }).collect();
        assert_eq!(from_png(&png_of(64, 32, png::ColorType::GrayscaleAlpha, &ga)).unwrap()[..4], [9, 9, 9, 77]);
        // Any other size, and anything that is no PNG, is refused.
        assert!(from_png(&png_of(32, 32, png::ColorType::Rgba, &[0; 32 * 32 * 4])).is_err());
        assert!(from_png(&png_of(64, 48, png::ColorType::Rgba, &[0; 64 * 48 * 4])).is_err());
        assert!(from_png(b"not a png at all").is_err());
        assert!(from_png(&[]).is_err());
    }

    #[test]
    fn import_takes_the_newest_png_and_keeps_a_copy() {
        let root = std::env::temp_dir().join(format!("mcrs-skin-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let r: &Path = &root;
        assert_eq!(import_from(&[r], r).unwrap_err(), "No .png in the skin folder");
        std::fs::write(root.join("notes.txt"), "x").unwrap();
        std::fs::write(root.join("bad.PNG"), "x").unwrap();
        assert_eq!(import_from(&[r], r).unwrap_err(), "Not a PNG file");
        let rgba: Vec<u8> = (0..64 * 64).flat_map(|i| [i as u8, 7, 9, 255]).collect();
        std::fs::write(root.join("mine.png"), png_of(64, 64, png::ColorType::Rgba, &rgba)).unwrap();
        std::fs::remove_file(root.join("bad.PNG")).unwrap();
        let (px, name) = import_from(&[r], r).unwrap();
        assert_eq!((px.len(), name.as_str()), (BYTES, "mine.png"));
        assert_eq!(load_custom(&root), Some(px), "kept in the private folder");
        // The kept copy is not a candidate for the next import.
        std::fs::remove_file(root.join("mine.png")).unwrap();
        assert!(import_from(&[r], r).is_err());
        clear_custom(&root);
        assert_eq!(load_custom(&root), None);
        let _ = std::fs::remove_dir_all(&root);
    }

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
