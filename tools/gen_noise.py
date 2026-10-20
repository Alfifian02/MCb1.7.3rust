"""photon-main/shaders/image/noise.png (512x512) -> mc-rs/assets/noise.rgba (raw RGBA8) and mc-rs/assets/noise_b.r8 (its blue
channel, raw R8: photon's blue-noise dither only ever reads `.b`). Run from the repo root: python3 tools/gen_noise.py [noise.png]
(needs Pillow)."""
import sys
from PIL import Image

im = Image.open(sys.argv[1] if len(sys.argv) > 1 else "photon-main/shaders/image/noise.png").convert("RGBA")
assert im.size == (512, 512), im.size
open("mc-rs/assets/noise.rgba", "wb").write(im.tobytes())
open("mc-rs/assets/noise_b.r8", "wb").write(im.getchannel("B").tobytes())
