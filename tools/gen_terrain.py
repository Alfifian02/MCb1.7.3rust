#!/usr/bin/env python3
"""Writes mc-rs/assets/terrain.rgba: the real b1.7.3 terrain.png (256x256, 16x16 tiles of 16x16) as raw RGBA, with the
biome tint baked in. Run from the repo root: python3 tools/gen_terrain.py  (needs Pillow; reads minecraft/jars/deobfuscated.jar).
ponytail: one climate for the whole world (temperature 0.8, rainfall 0.4, plains); biome tint per block needs a colour
vertex attribute, which is the upgrade path. Tile 255 (unused in the png) holds birch leaves."""
import io, zipfile
from PIL import Image

z = zipfile.ZipFile("minecraft/jars/deobfuscated.jar")
png = lambda n: Image.open(io.BytesIO(z.read(n))).convert("RGBA")
terrain, grass, foliage = png("terrain.png"), png("misc/grasscolor.png"), png("misc/foliagecolor.png")

t, r = 0.8, 0.4  # ColorizerGrass.getGrassColor(temperature, rainfall * temperature)
r *= t
at = (int((1 - t) * 255), int((1 - r) * 255))
GRASS, FOLIAGE = grass.getpixel(at)[:3], foliage.getpixel(at)[:3]
SPRUCE, BIRCH = (0x61, 0x99, 0x61), (0x80, 0xA7, 0x55)  # ColorizerFoliage.getFoliageColorPine / Birch

def tile(n): return terrain.crop(((n % 16) * 16, (n // 16) * 16, (n % 16) * 16 + 16, (n // 16) * 16 + 16)).copy()
def put(n, im): terrain.paste(im, ((n % 16) * 16, (n // 16) * 16))
def tint(im, c):
    px = [(r * c[0] // 255, g * c[1] // 255, b * c[2] // 255, a) for r, g, b, a in im.getdata()]
    out = Image.new("RGBA", im.size); out.putdata(px); return out
def opaque(im): im.putalpha(255); return im

put(255, tint(tile(53), BIRCH))                           # birch leaves (fast leaves, 53, are opaque)
for n in (0, 39, 56, 73): put(n, tint(tile(n), GRASS))    # grass top, tall grass, fern, reeds
put(53, tint(tile(53), FOLIAGE)); put(133, tint(tile(133), SPRUCE))
for n in (205, 237, 67): put(n, opaque(tile(n)))          # water, lava, ice are drawn as opaque cubes
open("mc-rs/assets/terrain.rgba", "wb").write(terrain.tobytes())
print("grass", GRASS, "foliage", FOLIAGE)
