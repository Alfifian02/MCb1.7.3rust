#!/usr/bin/env python3
"""Writes mc-rs/assets/celestial.rgba: the sun (left) and the full moon (right), 32x32 each, side by side (64x32 RGBA), from a modern-named
resource pack (Legacy Restoration). Run from the repo root: python3 tools/gen_celestial.py Legacy_Restoration.zip  (needs Pillow).
The pack's moon sheet is 4x2 phases; b1.7.3 has one moon, so the first tile (the full moon) is it."""
import io, sys, zipfile
from PIL import Image

z = zipfile.ZipFile(sys.argv[1] if len(sys.argv) > 1 else "Legacy_Restoration.zip")
root = next(n for n in z.namelist() if n.endswith("textures/environment/sun.png"))[: -len("sun.png")]
png = lambda n: Image.open(io.BytesIO(z.read(root + n))).convert("RGBA")
out = Image.new("RGBA", (64, 32))
out.paste(png("sun.png").resize((32, 32)), (0, 0))
out.paste(png("moon_phases.png").crop((0, 0, 32, 32)), (32, 0))
open("mc-rs/assets/celestial.rgba", "wb").write(out.tobytes())
print("wrote mc-rs/assets/celestial.rgba")
