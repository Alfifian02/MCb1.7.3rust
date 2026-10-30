"""Writes mc-rs/assets/mapui.rgba (256 x 128 RGBA): the last rows of the HUD texture (`render::hud::sprites::map`).
Left half (0..128, 0..128): the held map's own 128 x 128 pixels, rewritten at run time (`MapData::rgba`), clear here.
Right half: `misc/mapbg.png` (64 x 64, the paper behind the map) at (128, 0) and `misc/mapicons.png` (32 x 32, 4 x 4 markers of 8 x 8) at (128, 64).
Run from the repo root (needs Pillow):  python3 tools/gen_mapui.py [path/to/deobfuscated.jar]"""
import io, sys, zipfile
from PIL import Image

jar = zipfile.ZipFile(sys.argv[1] if len(sys.argv) > 1 else "minecraft/jars/deobfuscated.jar")
png = lambda n: Image.open(io.BytesIO(jar.read(n))).convert("RGBA")
sheet = Image.new("RGBA", (256, 128), (0, 0, 0, 0))
bg, icons = png("misc/mapbg.png"), png("misc/mapicons.png")
assert bg.size == (64, 64) and icons.size == (32, 32)
sheet.paste(bg, (128, 0))
sheet.paste(icons, (128, 64))
open("mc-rs/assets/mapui.rgba", "wb").write(sheet.tobytes())
print("mapui.rgba", len(sheet.tobytes()), "bytes")
