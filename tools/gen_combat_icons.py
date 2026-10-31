"""Cortes Combat C6 (NOT b1.7.3): paint the 20 weapon sprites of the mod into mc-rs/assets/items.rgba.

Item id 400 + n (`combat::catalog`) is items.png tile 208 + n (`catalog::ICON0`), tiles 208..=232, all empty in b1.7.3's sheet and
named by no `icons.rs` row, so `render/icons.rs` needs no new line. Run AFTER tools/gen_terrain.py (that script rewrites items.rgba
from the jar and would drop these tiles). The mod's alabarda PNGs are 32 x 32; the sheet's tiles are 16 x 16, so they are
reduced 2:1 by nearest neighbour (UNVERIFIED by eye: a box filter may look better). Idempotent. Needs Pillow.

  python3 tools/gen_combat_icons.py [path/to/cortes-combat/src/main/resources/assets/cortescombat/textures/item]
"""
import sys
from pathlib import Path
from PIL import Image

SRC = Path(sys.argv[1] if len(sys.argv) > 1 else "cortes-combat/src/main/resources/assets/cortescombat/textures/item")
SHEET = Path("mc-rs/assets/items.rgba")
ICON0 = 208
# `combat::catalog`: id = 400 + 5 * family + tier; family order dagger, ropera, alabarda, martillo, shotel; tier order wood, stone, iron, diamond, gold.
FAMILIES = ["daga", "ropera", "alabarda", "martillo", "shotel"]
TIERS = ["wooden", "stone", "iron", "diamond", "golden"]  # the mod's file prefixes (`wooden_daga.png`, `golden_ropera.png`)

sheet = Image.frombytes("RGBA", (256, 256), SHEET.read_bytes())
done = 0
for f, fam in enumerate(FAMILIES):
    for t, tier in enumerate(TIERS):
        p = SRC / f"{tier}_{fam}.png"
        if not p.exists():
            continue  # the combination does not exist (no wooden ropera, no stone war hammer ...)
        im = Image.open(p).convert("RGBA")
        if im.size != (16, 16):
            im = im.resize((16, 16), Image.NEAREST)
        tile = ICON0 + 5 * f + t
        sheet.paste(im, ((tile % 16) * 16, (tile // 16) * 16))
        done += 1
SHEET.write_bytes(sheet.tobytes())
print(f"{done} sprites written into {SHEET} (tiles {ICON0}..={ICON0 + 24})")
assert done == 20, "expected the 20 weapons of combat::catalog"
