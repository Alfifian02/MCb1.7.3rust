"""Pack-only healing (NOT b1.7.3): paint the bandage's sprite into mc-rs/assets/items.rgba, tile 234 (`combat::catalog::BANDAGE` 426 = `ICON0 + 26`).

The art is `tools/src/bandage_ref.png`, a 36 x 36 screenshot of the sprite in a satchel slot (grey slot, soft edges). The slot colour is made
clear, the soft edge is un-blended back to the black outline, the sprite is cut to its box and fitted to 15 px on its long side (a 16 x 16
tile with a 1 px margin, the size of b1.7.3's own items), by a box filter on premultiplied colour. Alpha is 0 or 255: the HUD shader drops
texels under 0.5. Run AFTER tools/gen_terrain.py and tools/gen_combat_icons.py (both rewrite items.rgba). Idempotent. Needs Pillow and numpy.

  python3 tools/gen_bandage_icon.py
"""
from pathlib import Path
import numpy as np
from PIL import Image

SRC, SHEET, TILE, SLOT, FIT = Path("tools/src/bandage_ref.png"), Path("mc-rs/assets/items.rgba"), 234, 139.0, 15

a = np.asarray(Image.open(SRC).convert("RGB").crop((3, 3, 34, 34))).astype(float)  # inside the slot's frame
alpha = np.clip((np.abs(a - SLOT).max(axis=2) - 6) / 40, 0, 1)  # the slot's grey -> clear, an edge blend -> partial
rgb = a.copy()
for y, x in zip(*np.nonzero((alpha > 0) & (alpha < 1))):
    rgb[y, x] = np.clip((a[y, x] - (1 - alpha[y, x]) * SLOT) / alpha[y, x], 0, 255)  # the outline under the blend is black
ys, xs = np.nonzero(alpha > 0.5)
box = (slice(ys.min(), ys.max() + 1), slice(xs.min(), xs.max() + 1))
h, w = alpha[box].shape
s = FIT / max(w, h)
nw, nh = max(1, round(w * s)), max(1, round(h * s))
pm = np.dstack([rgb[box] * alpha[box][..., None], alpha[box] * 255])  # premultiplied: 3 colour planes and alpha
small = np.dstack([np.asarray(Image.fromarray(pm[..., i].astype(np.float32), "F").resize((nw, nh), Image.BOX)) for i in range(4)])
al = small[..., 3:] / 255
col = np.where(al > 0, small[..., :3] / np.maximum(al, 1e-6) / 1.0, 0)
tile = Image.new("RGBA", (16, 16), (0, 0, 0, 0))
tile.paste(Image.fromarray(np.dstack([col, np.where(al[..., 0] >= 0.5, 255, 0)]).clip(0, 255).astype(np.uint8), "RGBA"), ((16 - nw) // 2, (16 - nh) // 2))
sheet = Image.frombytes("RGBA", (256, 256), SHEET.read_bytes())
sheet.paste(tile, ((TILE % 16) * 16, (TILE // 16) * 16))
SHEET.write_bytes(sheet.tobytes())
print(f"bandage {nw} x {nh} written into {SHEET} (tile {TILE})")
