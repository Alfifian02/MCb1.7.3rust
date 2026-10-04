#!/usr/bin/env python3
"""Membuat mc-android/src/font.rs: font bitmap 7x14 (ASCII 32..126) dari DejaVu Sans Mono 11px.
  python3 tools/gen_font.py /usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf mc-android/src/font.rs
"""
import sys
from PIL import Image, ImageDraw, ImageFont

W, H = 7, 14
f = ImageFont.truetype(sys.argv[1], 11)
o = ['//! DIHASILKAN oleh tools/gen_font.py. Font bitmap 7x14, ASCII 32..=126.',
     '//! Tiap baris = 7 bit, bit paling kiri = bit ke-6.', '',
     f'pub const W: usize = {W};', f'pub const H: usize = {H};', '',
     f'pub static GLYPHS: [[u8; {H}]; 95] = [']
for code in range(32, 127):
    im = Image.new('L', (W, H), 0)
    ImageDraw.Draw(im).text((0, 0), chr(code), font=f, fill=255)
    rows = []
    for y in range(H):
        v = 0
        for x in range(W):
            if im.getpixel((x, y)) > 110:
                v |= 1 << (W - 1 - x)
        rows.append(v)
    o.append('    [' + ', '.join(f'0x{r:02x}' for r in rows) + f'],  // {chr(code)!r}')
o += ['];', '']
open(sys.argv[2], 'w').write('\n'.join(o))
