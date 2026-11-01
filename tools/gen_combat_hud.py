#!/usr/bin/env python3
"""Writes mc-rs/assets/combat_hud.rgba (256 x 64 RGBA): Cortes Combat's HUD sprites (`textures/gui/sprites/hud/*.png`), the block of the HUD
texture at `hud::COMBAT_Y`. Places (keep in step with `hud::sprites::cc`):
  y 0:  crosshair (0), crosshair_aim (16), crosshair_hit (32), shield_indicator_full (48, 16 x 16), shield_indicator_disabled (64, 16 x 16)
  y 16: charge_bar (0, 30 x 15), charge_bar_emtpy (32, 30 x 15), cooldown_corchetes (64, 23 x 15)
`fs_main` throws away every texel under alpha 0.5, and most of these sprites are fainter than that (the brackets are 38 / 255, the orbs 102
and 55, the empty bar 88), so each lit texel is stored at alpha 255 and the sprite's own alpha is a constant in `hud::sprites::cc`, drawn as
the vertex alpha. A sprite has one alpha only (checked here), so nothing is lost. Transparent texels are white, so the sharp bilinear's
blend across a sprite's edge does not darken it.
Run from the repo root: python3 tools/gen_combat_hud.py path/to/cortes-combat/src/main/resources/assets/cortescombat/textures/gui/sprites/hud
(needs Pillow). Prints the alphas to put in `cc::ALPHA_*`."""
import sys
from PIL import Image

src = sys.argv[1] if len(sys.argv) > 1 else "cortes-combat/src/main/resources/assets/cortescombat/textures/gui/sprites/hud"
PLACES = [("crosshair", 0, 0), ("crosshair_aim", 16, 0), ("crosshair_hit", 32, 0), ("crosshair_shield_indicator_full", 48, 0),
          ("crosshair_shield_indicator_disabled", 64, 0), ("charge_bar", 0, 16), ("charge_bar_emtpy", 32, 16), ("cooldown_corchetes", 64, 16)]
out = Image.new("RGBA", (256, 64), (255, 255, 255, 0))
for name, x, y in PLACES:
    im = Image.open(f"{src}/{name}.png").convert("RGBA")
    alphas = {p[3] for p in im.getdata() if p[3] > 0}
    assert len(alphas) == 1, f"{name} has more than one alpha: {sorted(alphas)}"
    a = alphas.pop()
    for yy in range(im.height):
        for xx in range(im.width):
            r, g, b, pa = im.getpixel((xx, yy))
            if pa > 0:
                out.putpixel((x + xx, y + yy), (r, g, b, 255))
    print(f"{name}: {im.width} x {im.height} at ({x}, {y}), alpha {a} / 255 = {a / 255:.3f}")
open("mc-rs/assets/combat_hud.rgba", "wb").write(out.tobytes())
print("wrote mc-rs/assets/combat_hud.rgba")
