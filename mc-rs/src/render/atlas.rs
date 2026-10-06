//! Texture atlas. M1: a single 2x2 RGBA texture used for every face.
//! Stored as raw bytes in the binary; no asset pipeline yet.

/// RGBA pixel, 4 bytes per pixel.
pub type Pixel = [u8; 4];

/// 2x2 texture, 4 channels, 16 bytes total.
/// Diagonal: top-left = stone-grey, rest = darker shade.
/// (Stone-ish but not committed to b1.7.3 fidelity yet.)
pub const STONE_TEXEL: Pixel = [132, 132, 132, 255];
pub const DARK_TEXEL:  Pixel = [ 86,  86,  86, 255];

/// 2x2 image as `[row][col]` pixels.
pub fn stone_image() -> [[Pixel; 2]; 2] {
    [
        [STONE_TEXEL, DARK_TEXEL],
        [DARK_TEXEL, STONE_TEXEL],
    ]
}

/// Flatten to a single `Vec<u8>` in row-major order.
pub fn stone_image_rgba() -> Vec<u8> {
    let img = stone_image();
    let mut out = Vec::with_capacity(2 * 2 * 4);
    for row in img {
        for px in row {
            out.extend_from_slice(&px);
        }
    }
    out
}
