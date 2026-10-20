#!/usr/bin/env python3
"""Writes mc-rs/assets/panorama.rgba: the six faces of the title screen's panorama (a resource pack's
assets/minecraft/textures/gui/title/background/panorama_0..5.png), each resized to N x N, stacked (6 * N * N * 4 bytes). N must be
`FACE` in mc-rs/src/render/panorama.rs. Run from the repo root: python3 tools/gen_panorama.py smantri_panorama.zip  (needs Pillow)."""
import io, sys, zipfile
from PIL import Image

N = 512
z = zipfile.ZipFile(sys.argv[1])
face = lambda i: Image.open(io.BytesIO(z.read(f"assets/minecraft/textures/gui/title/background/panorama_{i}.png"))).convert("RGBA").resize((N, N), Image.LANCZOS)
open("mc-rs/assets/panorama.rgba", "wb").write(b"".join(face(i).tobytes() for i in range(6)))
print("wrote mc-rs/assets/panorama.rgba")
