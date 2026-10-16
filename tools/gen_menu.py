#!/usr/bin/env python3
"""Writes mc-rs/assets/menu.rgba (256 x 272 RGBA): b1.7.3's title/mclogo.png (256 x 256) over gui/background.png (16 x 16 at row 256, the
rest transparent), straight from the original jar, plus mc-rs/assets/splashes.txt (title/splashes.txt). Run from the repo root:
python3 tools/gen_menu.py deobfuscated.jar  (needs Pillow)."""
import io, sys, zipfile
from PIL import Image

z = zipfile.ZipFile(sys.argv[1] if len(sys.argv) > 1 else "deobfuscated.jar")
png = lambda n: Image.open(io.BytesIO(z.read(n))).convert("RGBA")
out = Image.new("RGBA", (256, 272))
out.paste(png("title/mclogo.png"), (0, 0))
out.paste(png("gui/background.png"), (0, 256))
open("mc-rs/assets/menu.rgba", "wb").write(out.tobytes())
open("mc-rs/assets/splashes.txt", "wb").write(z.read("title/splashes.txt"))
print("wrote mc-rs/assets/menu.rgba, splashes.txt")
