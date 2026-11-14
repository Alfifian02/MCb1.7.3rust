#!/usr/bin/env python3
"""Rewrites mc-rs/assets/terrain.rgba from Bedrock Developer Art (midnightcherry, v2.6.6, Bedrock layout: textures/blocks/*.png with the
old names `planks_oak`, `wool_colored_white`, `rail_normal`, ...): every slot of b1.7.3's terrain.png (256x256, 16x16 tiles) that has a
counterpart in the pack gets the pack's texture, UNTINTED like gen_terrain.py / gen_terrain_pack.py leave it (the mesher tints grass,
foliage and the grass fringe per vertex, `world/climate.rs`; the pack's grass top, tall grass, fern and leaves are grey, like the png's).
Run from the repo root:   python3 tools/gen_terrain_bedrock.py "Bedrock_Developer_Art.zip"   (or the unzipped folder; needs Pillow).
Same contract as gen_terrain_pack.py (one table, slot -> source; animated strips give their first frame; slots with no counterpart keep
what is in terrain.rgba). Chests: the single chest tiles come from blocks/chest_*.png; the pack has no large-chest tiles, so the four
halves (41 / 42 front, 57 / 58 back) are cut from entity/chest/double_normal.png (lid + body, 30 x 15 px, scaled to 32 x 16).
LICENCE: the pack's licence.txt allows private use and modification only; do NOT commit or redistribute the generated terrain.rgba.
"""
import io, os, sys, zipfile
from PIL import Image

SRC = sys.argv[1] if len(sys.argv) > 1 else "Bedrock_Developer_Art.zip"
OUT = "mc-rs/assets/terrain.rgba"

if os.path.isdir(SRC):
    TEX = next(r for r, _, f in os.walk(SRC) if r.replace("\\", "/").endswith("textures/blocks") and "stone.png" in f)[: -len("blocks")]
    def read(p):
        try:
            return open(os.path.join(TEX, *p.split("/")), "rb").read()
        except FileNotFoundError:
            raise KeyError(p)
else:
    z = zipfile.ZipFile(SRC)
    names = {n.replace("\\", "/"): n for n in z.namelist()}
    ROOT = next(n for n in names if n.endswith("textures/blocks/stone.png"))[: -len("blocks/stone.png")]
    def read(p):
        return z.read(names[ROOT + p])

open_png = lambda p: Image.open(io.BytesIO(read(p))).convert("RGBA")
terrain = Image.frombytes("RGBA", (256, 256), open(OUT, "rb").read())  # slots without a counterpart stay as they are


def blk(name):
    """A block texture as 16x16: animation strips give their first frame, 32-wide (double resolution) sheets are box-filtered down."""
    im = open_png("blocks/" + name + ".png")
    w = im.width
    if im.height != w:
        im = im.crop((0, 0, w, w))
    return im if w == 16 else im.resize((16, 16), Image.BOX)


def solid(im):
    """`Block.graphicsLevel` off (\"fast leaves\"): the holes become a darker leaf, the cube is opaque."""
    px = [p for p in im.getdata() if p[3] > 0]
    g = sum(p[0] for p in px) // len(px) // 2
    bg = Image.new("RGBA", im.size, (g, g, g, 255))
    bg.alpha_composite(im)
    return bg


def alpha(im, a):
    im = im.copy()
    im.putalpha(a)
    return im


def dust_cross():
    """Tile 164 is the wire's cross; the pack has it ready made."""
    return blk("redstone_dust_cross")


def double_chest(back):
    """Large chest: the lid and body faces of `entity/chest/double_normal.png` (128 x 64, the 1.7 layout: lid box 30 x 5 x 14 at (0, 0), body
    30 x 10 x 14 at (0, 19)) stacked to 30 x 15 and scaled to 32 x 16; returns the two halves (left, right) as seen from the face."""
    e = open_png("entity/chest/double_normal.png")
    x = 58 if back else 14                       # front faces start at u 14, back faces at u 14 + 30 + 14
    face = Image.new("RGBA", (30, 15))
    face.paste(e.crop((x, 14, x + 30, 14 + 5)), (0, 0))
    face.paste(e.crop((x, 33, x + 30, 33 + 10)), (0, 5))
    face = face.resize((32, 16), Image.NEAREST)
    return face.crop((0, 0, 16, 16)), face.crop((16, 0, 32, 16))


# slot -> a block texture name, or a function giving the 16x16 tile
MAP = {
    0: "grass_top", 1: "stone", 2: "dirt", 3: "grass_block_side", 4: "planks_oak",
    5: "stone_slab_side", 6: "stone_slab_top", 7: "brick", 8: "tnt_side", 9: "tnt_top", 10: "tnt_bottom", 11: "web",
    12: "flower_rose", 13: "flower_dandelion", 14: "portal", 15: "sapling_oak",
    16: "cobblestone", 17: "bedrock", 18: "sand", 19: "gravel", 20: "log_oak", 21: "log_oak_top", 22: "iron_block",
    23: "gold_block", 24: "diamond_block", 25: "chest_top", 26: "chest_side", 27: "chest_front",
    28: "mushroom_red", 29: "mushroom_brown", 31: "fire_0",
    32: "gold_ore", 33: "iron_ore", 34: "coal_ore", 35: "bookshelf", 36: "cobblestone_mossy", 37: "obsidian",
    38: "grass_block_side_overlay", 39: "tallgrass", 43: "crafting_table_top", 44: "furnace_front_off",
    45: "furnace_side", 46: "dispenser_front", 47: "fire_1",
    48: "sponge", 49: "glass", 50: "diamond_ore", 51: "redstone_ore", 52: "leaves_oak",
    53: lambda: solid(blk("leaves_oak")), 55: "deadbush", 56: "fern",
    59: "crafting_table_side", 60: "crafting_table_front", 61: "furnace_front_on", 62: "furnace_top", 63: "sapling_spruce",
    64: "wool_colored_white", 65: "mob_spawner", 66: "snow", 67: "ice", 68: "grass_side_snowed", 69: "cactus_top", 70: "cactus_side",
    71: "cactus_bottom", 72: "clay", 73: "reeds", 74: "jukebox_side", 75: "jukebox_top", 79: "sapling_birch",
    80: "torch_on", 81: "door_wood_lower", 82: "door_iron_lower", 83: "ladder", 84: "trapdoor", 86: "farmland_wet",
    87: "farmland_dry", 96: "lever", 97: "door_wood_upper", 98: "door_iron_upper", 99: "redstone_torch_on", 102: "pumpkin_top",
    103: "netherrack", 104: "soul_sand", 105: "glowstone", 106: "piston_top_sticky", 107: "piston_top_normal", 108: "piston_side",
    109: "piston_bottom", 110: "piston_inner", 112: "rail_normal_turned", 113: "wool_colored_black", 114: "wool_colored_gray",
    115: "redstone_torch_off", 116: "log_spruce", 117: "log_birch", 118: "pumpkin_side", 119: "pumpkin_face_off",
    120: "pumpkin_face_on", 121: "cake_top", 122: "cake_side", 123: "cake_inner", 124: "cake_bottom",  # BlockCake: bottom is +3, the cut face +2
    128: "rail_normal", 129: "wool_colored_red", 130: "wool_colored_pink", 131: "repeater_off", 133: lambda: solid(blk("leaves_spruce")),
    134: "bed_feet_top", 135: "bed_head_top", 144: "lapis_block", 145: "wool_colored_green", 146: "wool_colored_lime",
    147: "repeater_on", 149: "bed_feet_end", 150: "bed_feet_side", 151: "bed_head_side", 152: "bed_head_end",
    160: "lapis_ore", 161: "wool_colored_brown", 162: "wool_colored_yellow", 163: "rail_golden", 164: dust_cross,
    165: lambda: blk("redstone_dust_line"), 176: "sandstone_top",
    177: "wool_colored_blue", 178: "wool_colored_light_blue", 179: "rail_golden_powered", 192: "sandstone_normal",
    193: "wool_colored_purple", 194: "wool_colored_magenta", 195: "rail_detector",
    205: lambda: alpha(blk("water_still"), 138),  # translucent, drawn as such (the mesher does not tint water)
    206: lambda: alpha(blk("water_flow"), 138), 208: "sandstone_bottom", 209: "wool_colored_cyan", 210: "wool_colored_orange",
    225: "wool_colored_silver", 237: "lava_still", 238: "lava_flow",  # lava is drawn as an opaque cube
    255: lambda: solid(blk("leaves_birch")),  # unused in the png: birch leaves (the mesher tints it)
}
for i in range(10):  # the crack overlay lives in textures/environment/ in the Bedrock layout
    MAP[240 + i] = (lambda i: lambda: open_png("environment/destroy_stage_%d.png" % i).crop((0, 0, 16, 16)))(i)
for i in range(8):
    MAP[88 + i] = "wheat_stage_%d" % i

missing = []
for slot, src in sorted(MAP.items()):
    try:
        im = src() if callable(src) else blk(src)
    except KeyError:
        missing.append((slot, src))
        continue
    if slot in (237, 238, 67):  # lava and ice are drawn as opaque cubes
        im = alpha(im, 255)
    terrain.paste(im, ((slot % 16) * 16, (slot // 16) * 16))

# large chests: 41 / 42 = the two halves of the front, 57 / 58 = of the back (`world::chest::tile`)
try:
    for (a, b), back in (((41, 42), False), ((57, 58), True)):
        left, right = double_chest(back)
        terrain.paste(left, ((a % 16) * 16, (a // 16) * 16))
        terrain.paste(right, ((b % 16) * 16, (b // 16) * 16))
except KeyError:
    missing.append(("41/42/57/58", "entity/chest/double_normal.png"))

open(OUT, "wb").write(terrain.tobytes())
print("wrote", OUT, "slots from the pack:", len(MAP) - len(missing) + 4)
if missing:
    print("NOT FOUND in the pack (slot kept):", missing)
