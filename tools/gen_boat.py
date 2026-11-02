#!/usr/bin/env python3
"""Bedrock Developer Art `textures/entity/boat/boat_oak.png` (128 x 64, the modern `ModelBoat` layout) -> `mc-rs/assets/boat.rgba` (256 x 64, the
texture on the left, the right half clear): `render::atlas::BOAT`, drawn by `render::items::push_boat`. Run from the repo root:
python3 tools/gen_boat.py <path to Bedrock_Developer_Art.zip or the unpacked pack folder>   (needs Pillow)."""
import io, os, sys, zipfile
from PIL import Image

src = sys.argv[1]
name = "textures/entity/boat/boat_oak.png"
if os.path.isdir(src):
    im = Image.open(os.path.join(src, name))
else:
    z = zipfile.ZipFile(src)
    im = Image.open(io.BytesIO(z.read(next(n for n in z.namelist() if n.endswith(name)))))
im = im.convert("RGBA")
assert im.size == (128, 64), im.size
sheet = Image.new("RGBA", (256, 64), (0, 0, 0, 0))
sheet.paste(im, (0, 0))
open("mc-rs/assets/boat.rgba", "wb").write(sheet.tobytes())
