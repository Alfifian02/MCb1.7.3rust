"""photon-main/shaders/image/noise.png (512x512) -> mc-rs/assets/noise.rgba (raw RGBA8). Run from the repo root: python3 tools/gen_noise.py [noise.png]
(needs Pillow)."""
import sys
from PIL import Image

im = Image.open(sys.argv[1] if len(sys.argv) > 1 else "photon-main/shaders/image/noise.png").convert("RGBA")
assert im.size == (512, 512), im.size
open("mc-rs/assets/noise.rgba", "wb").write(im.tobytes())
