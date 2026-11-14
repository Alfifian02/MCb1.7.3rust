#!/usr/bin/env python3
"""Rewrites mc-rs/assets/terrain.rgba from a modern-named resource pack (Legacy Restoration, SoftFoxy, Java 26.2): every slot of
b1.7.3's terrain.png (256x256, 16x16 tiles) that has a counterpart in the pack gets the pack's texture, UNTINTED like gen_terrain.py
leaves it (the mesher tints grass, foliage and the grass fringe per vertex, `world/climate.rs`). Run from the repo root:  python3 tools/gen_terrain_pack.py Legacy_Restoration.zip  (needs Pillow).
Slots the pack has no texture for (chests: the pack leaves them to the game; beds, the unused ones) keep the png's own tile.
ponytail: one table, slot -> source; nothing else to configure. Animated sheets (water, lava, fire, portal) use their first frame.
The pack's grass, fern and leaf textures are grey, like the png's; tile 3 is the side with its fringe and tile 38 the grey overlay."""
import io, sys, zipfile
from PIL import Image

PACK = sys.argv[1] if len(sys.argv) > 1 else "Legacy_Restoration.zip"
OUT = "mc-rs/assets/terrain.rgba"
z = zipfile.ZipFile(PACK)
ROOT = next(n for n in z.namelist() if n.endswith("assets/minecraft/textures/block/stone.png"))[: -len("block/stone.png")]
open_png = lambda p: Image.open(io.BytesIO(z.read(ROOT + p))).convert("RGBA")

terrain = Image.frombytes("RGBA", (256, 256), open(OUT, "rb").read())  # the slots the pack does not cover stay as they are

def blk(name):
    """A block texture as 16x16: animation strips give their first frame, 32-wide (double resolution) sheets are box-filtered down."""
    im = open_png("block/" + name + ".png")
    w = im.width
    if im.height != w:
        im = im.crop((0, 0, w, w))
    return im if w == 16 else im.resize((16, 16), Image.BOX)


def solid(im):
    """`Block.graphicsLevel` off ("fast leaves"): the holes become a darker leaf, the cube is opaque."""
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
    """The old redstone wire tile 164 is the cross: the pack's dot with the line across and down."""
    line, c = blk("redstone_dust_line0"), blk("redstone_dust_dot")
    c.alpha_composite(line)
    c.alpha_composite(line.rotate(90))
    return c


# slot -> a block texture name, or a function giving the 16x16 tile
MAP = {
    0: "grass_block_top", 1: "stone", 2: "dirt", 3: "grass_block_side", 4: "oak_planks",
    5: "smooth_stone_slab_side", 6: "smooth_stone", 7: "bricks", 8: "tnt_side", 9: "tnt_top", 10: "tnt_bottom", 11: "cobweb",
    12: "poppy", 13: "dandelion", 14: "nether_portal", 15: "oak_sapling",
    16: "cobblestone", 17: "bedrock", 18: "sand", 19: "gravel", 20: "oak_log", 21: "oak_log_top", 22: "iron_block",
    23: "gold_block", 24: "diamond_block", 28: "red_mushroom", 29: "brown_mushroom", 31: "fire_0",
    32: "gold_ore", 33: "iron_ore", 34: "coal_ore", 35: "bookshelf", 36: "mossy_cobblestone", 37: "obsidian",
    38: "grass_block_side_overlay", 39: "grass", 43: "crafting_table_top", 44: "furnace_front",
    45: "furnace_side", 46: "dispenser_front", 47: "fire_1",
    48: "sponge", 49: "glass", 50: "diamond_ore", 51: "redstone_ore", 52: "oak_leaves",
    53: lambda: solid(blk("oak_leaves")), 55: "dead_bush", 56: "fern",
    59: "crafting_table_side", 60: "crafting_table_front", 61: "furnace_front_on", 62: "furnace_top", 63: "spruce_sapling",
    64: "white_wool", 65: "spawner", 66: "snow", 67: "ice", 68: "grass_block_snow", 69: "cactus_top", 70: "cactus_side",
    71: "cactus_bottom", 72: "clay", 73: "sugar_cane", 74: "jukebox_side", 75: "jukebox_top", 79: "birch_sapling",
    80: "torch", 81: "oak_door_bottom", 82: "iron_door_bottom", 83: "ladder", 84: "oak_trapdoor", 86: "farmland_moist",
    87: "farmland", 96: "lever", 97: "oak_door_top", 98: "iron_door_top", 99: "redstone_torch", 102: "pumpkin_top",
    103: "netherrack", 104: "soul_sand", 105: "glowstone", 106: "piston_top_sticky", 107: "piston_top", 108: "piston_side",
    109: "piston_bottom", 110: "piston_inner", 112: "rail_corner", 113: "black_wool", 114: "gray_wool",
    115: "redstone_torch_off", 116: "spruce_log", 117: "birch_log", 118: "pumpkin_side", 119: "carved_pumpkin",
    120: "jack_o_lantern", 121: "cake_top", 122: "cake_side", 123: "cake_inner", 124: "cake_bottom",  # BlockCake: bottom is +3, the cut face +2
    128: "rail", 129: "red_wool", 130: "pink_wool", 131: "repeater", 133: lambda: solid(blk("spruce_leaves")),
    144: "lapis_block", 145: "green_wool", 146: "lime_wool", 147: "repeater_on", 160: "lapis_ore", 161: "brown_wool",
    162: "yellow_wool", 163: "powered_rail", 164: dust_cross, 165: lambda: blk("redstone_dust_line0").rotate(90),  # the png's wire line runs across 176: "sandstone_top",
    177: "blue_wool", 178: "light_blue_wool", 179: "powered_rail_on", 192: "sandstone", 193: "purple_wool",
    194: "magenta_wool", 195: "detector_rail", 205: lambda: alpha(blk("water_still"), 138),  # translucent, drawn as such
    206: lambda: alpha(blk("water_flow"), 138), 208: "sandstone_bottom", 209: "cyan_wool", 210: "orange_wool",
    225: "light_gray_wool", 237: "lava_still", 238: "lava_flow",  # lava is drawn as an opaque cube
    255: lambda: solid(blk("birch_leaves")),  # unused in the png: birch leaves (the mesher tints it)
}
for i in range(10):
    MAP[240 + i] = "destroy_stage_%d" % i
for i in range(8):
    MAP[88 + i] = "wheat_stage%d" % i

for slot, src in sorted(MAP.items()):
    im = src() if callable(src) else blk(src)
    if slot in (237, 238, 67):  # lava and ice are drawn as opaque cubes
        im = alpha(im, 255)
    terrain.paste(im, ((slot % 16) * 16, (slot // 16) * 16))

open(OUT, "wb").write(terrain.tobytes())
print("wrote", OUT, "slots from the pack:", len(MAP))
