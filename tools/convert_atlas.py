#!/usr/bin/env python3
"""Convert a classic terrain.png (16x16 tiles) into bgame/assets/terrain.rgba (512x512 raw RGBA).

Classic atlases ship grass top / leaves / grass-side overlay as grayscale (tinted per biome in-game).
We bake a fixed plains tint here so the renderer needs no extra shader work.

Usage: python3 tools/convert_atlas.py path/to/terrain.png bgame/assets/terrain.rgba
Needs: pip install pillow
"""
import sys
from PIL import Image

SIZE, T = 512, 32
GRASS = (145, 189, 89)
FOLIAGE = (119, 171, 47)


def box(t):
    x, y = t % 16, t // 16
    return (x * T, y * T, x * T + T, y * T + T)


def tint(img, rgb):
    r, g, b, a = img.split()
    mul = lambda ch, k: ch.point(lambda v: v * k // 255)
    return Image.merge("RGBA", (mul(r, rgb[0]), mul(g, rgb[1]), mul(b, rgb[2]), a))


src, dst = sys.argv[1], sys.argv[2]
im = Image.open(src).convert("RGBA")
if im.size != (SIZE, SIZE):
    im = im.resize((SIZE, SIZE), Image.NEAREST)

grass_top = tint(im.crop(box(0)), GRASS)
leaves = tint(im.crop(box(52)), FOLIAGE)
side = Image.alpha_composite(im.crop(box(3)), tint(im.crop(box(38)), GRASS))

im.paste(grass_top, box(0)[:2])
im.paste(leaves, box(52)[:2])
im.paste(side, box(3)[:2])

with open(dst, "wb") as f:
    f.write(im.tobytes())
print("wrote", dst, len(im.tobytes()), "bytes")

# preview strip of the tiles the engine uses
ids = [0, 1, 2, 3, 4, 16, 17, 18, 19, 20, 21, 52, 205]
strip = Image.new("RGBA", (T * len(ids), T), (135, 180, 240, 255))
for i, t in enumerate(ids):
    strip.alpha_composite(im.crop(box(t)), (i * T, 0))
strip.resize((strip.width * 2, strip.height * 2), Image.NEAREST).save(dst + ".preview.png")
