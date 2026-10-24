#!/usr/bin/env python3
"""Bake the Structory datapack (Stardust Labs, 1.20) into `mc-rs/assets/structory.bin`, the file `world/gen/structory.rs` reads.

    python3 tools/gen_structory.py [Structory-1_20.zip | datapack dir] [out.bin]

Python stdlib only. What it does, in the order of the file:
  * every template (`.nbt`) a pool names: size, one u16 per cell (a modern block state, `SKIP` where the NBT lists none), its jigsaw
    blocks and its chests / spawners;
  * the modern block states used, each with the Beta (id, metadata) it becomes (`to_beta`): Beta has no stone bricks, walls, panes,
    carpets, ..., so those turn into the nearest Beta block (see the table; unknown names become air and are listed at the end);
  * the 6 processor lists, resolved per state (rule order kept, `random_*` predicates as a 16 bit chance) so the game needs no
    block-state matching;
  * the 60 template pools, the 15 structures (biome tags folded onto this world's biomes, `BIOMES`) and the 5 structure sets;
  * the 39 loot tables, items mapped to Beta items (`ITEMS`), what Beta lacks dropped.
`--check` (default) re-reads the file with the same reader the game uses (`structory_proto.py`) and fails on a dangling reference.
Layout: all little endian; see `Out` and `structory.rs::Data::parse` (they are written side by side).
"""
import gzip, io, json, os, struct, sys, zipfile, collections

SKIP = 0xFFFF
NONE16 = 0xFFFF
# Direction order of Minecraft's `Direction.from3DDataValue`.
DIRS = {'down': 0, 'up': 1, 'north': 2, 'south': 3, 'west': 4, 'east': 5}
# Biome indices = `Biome as u8` in mc-rs/src/world/biome.rs (checked by a test in structory.rs).
B = dict(Rainforest=0, Swamp=1, SeasonalForest=2, Forest=3, Savanna=4, Shrubland=5, Taiga=6, Desert=7, Plains=8, Tundra=9, Ocean=10,
         Beach=11, Mountains=12, AutumnalForest=13, Chaparral=14, Marsh=15, OakWoodland=16, WetWoodland=17, SnowyBirchGrove=18)
# Modern biome (or Terralith / Blooming stand-in) -> this world's biomes. The Blooming biomes take the place of the vanilla biome
# they are closest to: woods -> OakWoodland, spruce-birch woods -> AutumnalForest, jungle / dark forest -> WetWoodland, savanna and
# scrub -> Chaparral, swamps -> Marsh, cherry / sakura -> SnowyBirchGrove.
BIOMES = {
    'plains': ['Plains'], 'sunflower_plains': ['Plains'], 'savanna': ['Savanna', 'Chaparral'],
    'forest': ['Forest', 'OakWoodland'], 'birch_forest': ['Forest', 'OakWoodland'], 'old_growth_birch_forest': ['Forest', 'AutumnalForest'],
    'dark_forest': ['SeasonalForest', 'WetWoodland'], 'sparse_jungle': ['Rainforest', 'WetWoodland'], 'jungle': ['Rainforest', 'WetWoodland'],
    'bamboo_jungle': ['Rainforest', 'WetWoodland'], 'taiga': ['Taiga', 'AutumnalForest'], 'snowy_taiga': ['Taiga', 'Tundra'],
    'old_growth_pine_taiga': ['Taiga', 'AutumnalForest'], 'old_growth_spruce_taiga': ['Taiga', 'AutumnalForest'], 'desert': ['Desert'],
    'mangrove_swamp': ['Swamp', 'Marsh'], 'swamp': ['Swamp', 'Marsh'],
    'ocean': ['Ocean'], 'cold_ocean': ['Ocean'], 'lukewarm_ocean': ['Ocean'], 'deep_ocean': ['Ocean'], 'deep_cold_ocean': ['Ocean'],
    'deep_lukewarm_ocean': ['Ocean'], 'warm_ocean': ['Ocean'],
    'terralith:sakura_grove': ['SnowyBirchGrove'], 'terralith:lavender_forest': ['SnowyBirchGrove'], 'terralith:moonlight_grove': ['SnowyBirchGrove'],
    'terralith:brushland': ['Shrubland', 'Chaparral'], 'terralith:shrubland': ['Shrubland', 'Chaparral'], 'terralith:cold_shrubland': ['Shrubland'],
    'terralith:forested_highlands': ['AutumnalForest'], 'terralith:shield': ['AutumnalForest'], 'terralith:siberian_taiga': ['Taiga'],
    'terralith:lush_valley': ['OakWoodland'], 'terralith:highlands': ['Plains'], 'terralith:alpine_highlands': ['Plains'],
    'terralith:desert_oasis': ['Desert'], 'terralith:lush_desert': ['Desert'], 'terralith:sandstone_valley': ['Desert'],
    'terralith:rocky_jungle': ['Rainforest'], 'terralith:amethyst_rainforest': ['Rainforest'],
    '#terralith:has_structure/fortified_village': ['Plains', 'Savanna'], '#c:savanna': ['Savanna', 'Chaparral'], '#c:plains': ['Plains'],
    '#minecraft:is_plains': ['Plains'], '#c:taiga': ['Taiga'], '#minecraft:is_taiga': ['Taiga'],
}

# ---------------------------------------------------------------- NBT
class R:
    def __init__(s, b): s.b, s.i = b, 0
    def u(s, n, f):
        v = struct.unpack_from(f, s.b, s.i)[0]; s.i += n; return v
    def tag(s, t):
        if t == 1: return s.u(1, '>b')
        if t == 2: return s.u(2, '>h')
        if t == 3: return s.u(4, '>i')
        if t == 4: return s.u(8, '>q')
        if t == 5: return s.u(4, '>f')
        if t == 6: return s.u(8, '>d')
        if t == 7:
            n = s.u(4, '>i'); v = s.b[s.i:s.i + n]; s.i += n; return v
        if t == 8:
            n = s.u(2, '>H'); v = s.b[s.i:s.i + n].decode('utf8', 'replace'); s.i += n; return v
        if t == 9:
            tt = s.u(1, '>b'); n = s.u(4, '>i'); return [s.tag(tt) for _ in range(n)]
        if t == 10:
            d = {}
            while True:
                tt = s.u(1, '>b')
                if tt == 0: return d
                n = s.u(2, '>H'); k = s.b[s.i:s.i + n].decode(); s.i += n
                d[k] = s.tag(tt)
        if t == 11:
            n = s.u(4, '>i'); v = struct.unpack_from('>%di' % n, s.b, s.i); s.i += 4 * n; return list(v)
        if t == 12:
            n = s.u(4, '>i'); v = struct.unpack_from('>%dq' % n, s.b, s.i); s.i += 8 * n; return list(v)
        raise ValueError(t)

def nbt(b):
    r = R(gzip.decompress(b)); assert r.u(1, '>b') == 10
    n = r.u(2, '>H'); r.i += n
    return r.tag(10)

# ---------------------------------------------------------------- the pack
class Pack:
    """Files of the datapack by path under `data/structory/`, from a zip or a directory."""
    def __init__(s, path):
        s.f = {}
        if os.path.isdir(path):
            for root, _, fs in os.walk(path):
                for n in fs:
                    p = os.path.join(root, n)
                    s.f[os.path.relpath(p, path).replace(os.sep, '/')] = p
            s.rd = lambda k: open(s.f[k], 'rb').read()
        else:
            z = zipfile.ZipFile(path)
            for n in z.namelist():
                if not n.endswith('/'): s.f[n] = n
            s.rd = lambda k: z.read(s.f[k])
        # a zip made from the folder may carry one leading directory
        s.root = ''
        roots = sorted({k[:k.index('data/structory/')] for k in s.f if 'data/structory/' in k and 'overlay' not in k}, key=len)
        s.root = roots[0] if roots else ''
    def get(s, rel): return s.rd(s.root + 'data/structory/' + rel)
    def has(s, rel): return (s.root + 'data/structory/' + rel) in s.f
    def list(s, pre, ext):
        p = s.root + 'data/structory/' + pre
        return sorted(k[len(s.root + 'data/structory/'):] for k in s.f if k.startswith(p) and k.endswith(ext))
    def json(s, rel): return json.loads(s.get(rel))

# ---------------------------------------------------------------- modern state -> Beta (id, meta)
COLORS = {'white': 0, 'orange': 1, 'magenta': 2, 'light_blue': 3, 'yellow': 4, 'lime': 5, 'pink': 6, 'gray': 7, 'light_gray': 8, 'cyan': 9,
          'purple': 10, 'blue': 11, 'brown': 12, 'green': 13, 'red': 14, 'black': 15}
FACE_N = {'north': 0, 'east': 1, 'south': 2, 'west': 3}
# Beta metadata of a thing that faces `facing` (chest, furnace, ladder, dispenser, piston: 2 N, 3 S, 4 W, 5 E).
FACE_NSWE = {'north': 2, 'south': 3, 'west': 4, 'east': 5}
# Torch / lever / button hang on a wall: 1 = the -X wall, 2 = +X, 3 = -Z, 4 = +Z. A modern `facing` points away from the wall.
WALL_OF_FACING = {'east': 1, 'west': 2, 'south': 3, 'north': 4}

def wood_species(n):
    # Beta logs / leaves / saplings: 0 oak, 1 spruce, 2 birch. Dark oak and mangrove are dark -> spruce; the rest are oak.
    if n.startswith(('spruce', 'dark_oak', 'mangrove')): return 1
    if n.startswith('birch'): return 2
    return 0

STONE_LIKE = ('stone', 'granite', 'diorite', 'andesite', 'deepslate', 'blackstone', 'basalt', 'tuff', 'calcite', 'cobblestone', 'brick', 'mud_brick',
              'prismarine', 'purpur', 'quartz', 'end_stone')

def stone_block(n):
    """The solid cube of a stone-ish name: (id, meta)."""
    if 'mossy' in n: return (48, 0)
    if n in ('stone', 'smooth_stone', 'polished_andesite', 'polished_diorite', 'polished_granite', 'andesite', 'diorite', 'granite',
             'chiseled_stone_bricks', 'polished_deepslate', 'chiseled_deepslate', 'deepslate', 'smooth_basalt', 'polished_basalt', 'basalt',
             'tuff', 'calcite', 'polished_blackstone', 'smooth_quartz', 'quartz_block'):
        return (1, 0)
    if n in ('bricks', 'brick', 'mud_bricks'): return (45, 0)
    if 'sandstone' in n: return (24, 0)
    return (4, 0)   # cobblestone, stone bricks (plain / cracked), deepslate bricks / tiles, blackstone, ...

def copper_block(n):
    # waxed_[weathered_|exposed_|oxidized_]cut_copper*: orange -> gold block, exposed -> sandstone, weathered / oxidized -> mossy cobble
    if 'oxidized' in n or 'weathered' in n: return (48, 0)
    if 'exposed' in n: return (24, 0)
    return (41, 0)

def slab_meta(n):
    if 'sandstone' in n: return 1
    if n.endswith(('oak_slab', 'spruce_slab', 'birch_slab', 'jungle_slab', 'acacia_slab', 'mangrove_slab', 'cherry_slab', 'bamboo_slab',
                   'crimson_slab', 'warped_slab')) or 'planks' in n or 'petrified' in n: return 2
    if 'copper' in n and ('exposed' in n): return 1
    if n in ('smooth_stone_slab', 'stone_slab'): return 0
    return 3

def to_beta(name, p):
    """(id, meta) or None for air. `p` = the state's properties (strings)."""
    n = name.split(':', 1)[1] if ':' in name else name
    g = p.get
    # potted plants stand where the pot was
    if n.startswith('potted_'):
        return to_beta('minecraft:' + n[7:], {})
    if n in ('air', 'cave_air', 'void_air', 'structure_void', 'barrier', 'light'): return None
    if n == 'jigsaw' or n == 'structure_block': return None
    if n == 'water': return (9, 0)
    if n == 'lava': return (11, 0)
    # --- plants
    if n in ('grass', 'short_grass', 'tall_grass'): return None if g('half') == 'upper' else (31, 1)
    if n in ('fern', 'large_fern'): return None if g('half') == 'upper' else (31, 2)
    if n == 'dead_bush': return (32, 0)
    if n in ('dandelion', 'oxeye_daisy', 'azure_bluet', 'white_tulip', 'lily_of_the_valley', 'sunflower', 'cornflower', 'allium'):
        return (37, 0) if n != 'sunflower' or g('half') != 'upper' else None
    if n in ('poppy', 'red_tulip', 'orange_tulip', 'pink_tulip', 'blue_orchid', 'wither_rose', 'peony', 'rose_bush', 'lilac'):
        return None if g('half') == 'upper' else (38, 0)
    if n == 'brown_mushroom': return (39, 0)
    if n == 'red_mushroom': return (40, 0)
    if n.endswith('_sapling'): return (6, wood_species(n))
    if n == 'sugar_cane': return (83, 0)
    if n == 'cactus': return (81, 0)
    if n in ('wheat', 'carrots', 'potatoes', 'beetroots'): return (59, min(7, int(g('age', '7'))))
    if n in ('pumpkin',): return (86, 0)
    if n in ('melon',): return (86, 0)
    if n in ('carved_pumpkin', 'jack_o_lantern'):
        return (86 if n == 'carved_pumpkin' else 91, {'south': 0, 'west': 1, 'north': 2, 'east': 3}.get(g('facing', 'south'), 0))
    if n == 'cobweb': return (30, 0)
    if n.endswith('_leaves'): return (18, wood_species(n) | 8)   # | 8: no decay (the datapack's leaves are persistent in spirit)
    if n == 'mushroom_stem': return (35, 0)
    if n.endswith('_stem'): return None       # melon / pumpkin stems
    if n.endswith(('_log', '_wood', '_hyphae')) and not n.startswith(('crimson', 'warped')):
        base = n.replace('stripped_', '')
        return (17, wood_species(base))
    if n.endswith('_planks') or n in ('bamboo_planks',): return (5, 0)
    # --- ground
    if n == 'grass_block' or n in ('moss_block', 'mycelium'): return (2, 0)
    if n in ('dirt', 'coarse_dirt', 'rooted_dirt', 'podzol', 'mud', 'packed_mud', 'dirt_path', 'muddy_mangrove_roots', 'soul_soil'): return (3, 0)
    if n == 'farmland': return (60, 7 if g('moisture') == '7' else 0)
    if n in ('sand', 'red_sand'): return (12, 0)
    if n == 'gravel': return (13, 0)
    if n == 'clay' or n.endswith('terracotta'): return (82, 0)
    if n == 'snow': return (78, max(0, min(7, int(g('layers', '1')) - 1)))
    if n in ('snow_block', 'powder_snow'): return (80, 0)
    if n == 'ice': return (79, 0)
    if n == 'obsidian': return (49, 0)
    if n == 'iron_ore': return (15, 0)
    if n in ('sandstone', 'smooth_sandstone', 'cut_sandstone', 'chiseled_sandstone', 'red_sandstone', 'smooth_red_sandstone', 'cut_red_sandstone',
             'chiseled_red_sandstone'): return (24, 0)
    # --- colours
    if n.endswith(('_carpet', '_banner', '_wall_banner')) or n == 'moss_carpet': return None     # a floor covering / a flag: nothing
    if n.endswith('_candle') or n == 'candle': return (50, 5)
    for c, m in COLORS.items():
        if n in (c + '_wool', c + '_concrete', c + '_concrete_powder'): return (35, m)
        if n in (c + '_stained_glass', c + '_stained_glass_pane'): return (20, 0)
        if n == c + '_bed':
            d = {'south': 0, 'west': 1, 'north': 2, 'east': 3}[g('facing', 'south')]
            return (26, d | (8 if g('part') == 'head' else 0))
    if n in ('glass', 'tinted_glass'): return (20, 0)
    if n in ('glass_pane',): return (20, 0)
    if n == 'iron_bars' or n == 'chain': return (85, 0)
    if n == 'hay_block': return (35, 4)
    if n == 'red_mushroom_block': return (35, 14)
    if n == 'brown_mushroom_block': return (35, 12)
    if n == 'coal_block': return (35, 15)
    if n == 'redstone_block': return (35, 14)
    if n == 'dried_kelp_block': return (35, 13)
    if n in ('honey_block', 'honeycomb_block', 'beehive', 'bee_nest'): return (35, 1) if 'honey' in n else (5, 0)
    if n == 'gold_block' or n == 'raw_gold_block': return (41, 0)
    if n in ('iron_block', 'anvil', 'chipped_anvil', 'damaged_anvil', 'raw_iron_block', 'lightning_rod'): return (85, 0) if n == 'lightning_rod' else (42, 0)
    if n in ('verdant_froglight', 'pearlescent_froglight', 'ochre_froglight', 'glowstone', 'sea_lantern', 'shroomlight'): return (89, 0)
    if n == 'tnt': return (46, 0)
    if n in ('bookshelf', 'chiseled_bookshelf'): return (47, 0)
    if n == 'crafting_table' or n in ('cartography_table', 'fletching_table', 'smithing_table', 'loom', 'stonecutter', 'grindstone'): return (58, 0)
    if n == 'note_block': return (25, 0)
    if n == 'jukebox': return (84, 0)
    if n in ('lantern', 'soul_lantern', 'campfire', 'soul_campfire', 'torch', 'soul_torch'): return (50, 5)
    if n in ('soul_fire', 'fire'): return None
    # --- wall-mounted
    if n in ('wall_torch', 'soul_wall_torch'): return (50, WALL_OF_FACING.get(g('facing', 'north'), 5))
    if n == 'redstone_torch': return (76, 5)
    if n == 'redstone_wall_torch': return (76, WALL_OF_FACING.get(g('facing', 'north'), 5))
    if n == 'ladder': return (65, FACE_NSWE.get(g('facing', 'north'), 2))
    if n == 'lever':
        f = g('face', 'wall')
        return (69, WALL_OF_FACING.get(g('facing', 'north'), 1) if f == 'wall' else 5)
    if n.endswith('_button'):
        return (77, WALL_OF_FACING[g('facing', 'north')]) if g('face', 'wall') == 'wall' else None
    if n.endswith('_pressure_plate'): return (72 if n.split('_')[0] in ('oak', 'spruce', 'birch', 'jungle', 'acacia', 'dark', 'mangrove', 'cherry') else 70, 0)
    if n.endswith('_wall_sign') or n.endswith('wall_hanging_sign'):
        return (68, {'north': 2, 'south': 3, 'west': 4, 'east': 5}.get(g('facing', 'north'), 2))
    if n.endswith('_sign') or n.endswith('hanging_sign'): return (63, int(g('rotation', '0')))
    if n == 'redstone_wire': return (55, 0)
    if n in ('repeater', 'comparator'):
        return (93, {'north': 0, 'east': 1, 'south': 2, 'west': 3}.get(g('facing', 'north'), 0) | ((int(g('delay', '1')) - 1) << 2 if n == 'repeater' else 0))
    if n in ('piston', 'sticky_piston'):
        f = {'down': 0, 'up': 1, 'north': 2, 'south': 3, 'west': 4, 'east': 5}.get(g('facing', 'north'), 2)
        return (33 if n == 'piston' else 29, f)
    if n in ('dispenser', 'dropper', 'observer'):
        return (23, FACE_NSWE.get(g('facing', 'north'), 2))
    if n in ('rail',): return (66, {'north_south': 0, 'east_west': 1, 'ascending_east': 2, 'ascending_west': 3, 'ascending_north': 4, 'ascending_south': 5,
                                    'south_east': 6, 'south_west': 7, 'north_west': 8, 'north_east': 9}.get(g('shape', 'north_south'), 0))
    if n in ('powered_rail', 'detector_rail', 'activator_rail'):
        return (27 if n != 'detector_rail' else 28, {'north_south': 0, 'east_west': 1, 'ascending_east': 2, 'ascending_west': 3, 'ascending_north': 4,
                                                      'ascending_south': 5}.get(g('shape', 'north_south'), 0) | (8 if g('powered') == 'true' else 0))
    if n in ('furnace', 'smoker', 'blast_furnace'): return (61, FACE_NSWE.get(g('facing', 'north'), 2))
    if n in ('chest', 'trapped_chest', 'barrel', 'ender_chest'): return (54, 0)
    if n == 'spawner': return (52, 0)
    # --- doors, trapdoors, stairs, slabs, fences
    if n.endswith('_door'):
        iron = n.startswith('iron')
        f = g('facing', 'north'); opened = g('open') == 'true'
        if not opened: d = (FACE_N[f] + 1) & 3                         # closed: the leaf stands on the `facing` edge
        else:                                                           # open: it swings to the hinge side
            right = g('hinge') == 'right'
            leaf = {'east': 0 if right else 2, 'south': 1 if right else 3, 'west': 2 if right else 0, 'north': 3 if right else 1}[f]
            d = leaf | 4
        return (71 if iron else 64, d | (8 if g('half') == 'upper' else 0))
    if n.endswith('_trapdoor'):
        f = g('facing', 'north')
        return (96, {'north': 0, 'south': 1, 'east': 3, 'west': 2}[f] | (4 if g('open') == 'true' else 0))
    if n.endswith('_stairs'):
        wood = n.split('_')[0] in ('oak', 'spruce', 'birch', 'jungle', 'acacia', 'dark', 'mangrove', 'cherry', 'bamboo') or 'sandstone' in n or 'copper' in n
        return (53 if wood else 67, {'east': 0, 'west': 1, 'south': 2, 'north': 3}.get(g('facing', 'east'), 0))
    if n.endswith('_slab'):
        t = g('type', 'bottom')
        return ((43 if t == 'double' else 44), slab_meta(n))
    if n.endswith(('_fence', '_fence_gate', '_wall')): return (85, 0)
    # --- stone family (checked last: `stone_bricks`, `mossy_cobblestone`, `cut_copper`, ...)
    if 'copper' in n: return copper_block(n)
    if any(k in n for k in STONE_LIKE): return stone_block(n)
    return 'UNKNOWN'

# ---------------------------------------------------------------- items for the loot tables (Beta ids, items +256)
ITEMS = {
    'iron_ingot': (265, 0), 'gold_ingot': (266, 0), 'stick': (280, 0), 'string': (287, 0), 'feather': (288, 0), 'gunpowder': (289, 0),
    'wheat': (296, 0), 'bread': (297, 0), 'leather': (334, 0), 'flint': (318, 0), 'arrow': (262, 0), 'bow': (261, 0), 'coal': (263, 0),
    'diamond': (264, 0), 'redstone': (331, 0), 'book': (340, 0), 'paper': (339, 0), 'bone': (352, 0), 'bone_meal': (351, 15), 'ink_sac': (351, 0),
    'lapis_lazuli': (351, 4), 'iron_shovel': (256, 0), 'iron_pickaxe': (257, 0), 'iron_axe': (258, 0), 'iron_hoe': (292, 0), 'iron_sword': (267, 0),
    'stone_pickaxe': (274, 0), 'stone_axe': (275, 0), 'stone_shovel': (273, 0), 'stone_sword': (272, 0), 'iron_helmet': (306, 0), 'iron_chestplate': (307, 0),
    'iron_leggings': (308, 0), 'iron_boots': (309, 0), 'leather_helmet': (298, 0), 'leather_chestplate': (299, 0), 'leather_leggings': (300, 0),
    'leather_boots': (301, 0), 'golden_apple': (322, 0), 'enchanted_golden_apple': (322, 0), 'porkchop': (319, 0), 'wheat_seeds': (295, 0),
    'sand': (12, 0), 'sandstone': (24, 0), 'cactus': (81, 0), 'dead_bush': (32, 0), 'grass': (31, 1), 'fern': (31, 2), 'cobweb': (30, 0), 'tnt': (46, 0),
    'bookshelf': (47, 0), 'pumpkin': (86, 0), 'jack_o_lantern': (91, 0), 'oxeye_daisy': (37, 0), 'poppy': (38, 0), 'dandelion': (37, 0), 'dark_oak_log': (17, 1),
    'dark_oak_planks': (5, 0), 'candle': (50, 0), 'raw_iron': (265, 0), 'raw_gold': (266, 0), 'bowl': (281, 0), 'apple': (260, 0), 'cooked_porkchop': (320, 0),
    'cookie': (357, 0), 'bucket': (325, 0), 'saddle': (329, 0), 'clay_ball': (337, 0), 'brick': (336, 0), 'snowball': (332, 0), 'sugar': (353, 0),
    'egg': (344, 0), 'compass': (345, 0), 'clock': (347, 0), 'map': (358, 0), 'shears': (359, 0), 'fishing_rod': (346, 0), 'cobblestone': (4, 0),
    'stone': (1, 0), 'gold_block': (41, 0), 'iron_block': (42, 0), 'oak_log': (17, 0), 'spruce_log': (17, 1), 'birch_log': (17, 2), 'torch': (50, 0),
    'oak_planks': (5, 0), 'sponge': (19, 0), 'glass': (20, 0), 'clay': (82, 0), 'obsidian': (49, 0), 'gravel': (13, 0), 'dirt': (3, 0),
}

def num(v, hi=False):
    """A loot number: a constant, or {min,max} (nested ones: the end asked for)."""
    if isinstance(v, dict):
        if 'min' in v: return num(v['max'] if hi else v['min'], hi)
        return num(v.get('value', v.get('n', 1)), hi)
    return int(v)

class Loot:
    """Beta loot tables: pools of (weight, id, damage, min, max, chance of 256) entries."""
    def __init__(s, pack):
        s.pack, s.names, s.tables = pack, [], []
    def index(s, name):
        if name not in s.names:
            s.names.append(name); s.tables.append(s.build(name))
        return s.names.index(name)
    @staticmethod
    def count(fns):
        lo, hi = 1, 1
        for f in fns or []:
            if f.get('function') in ('minecraft:set_count', 'set_count'):
                c = f['count']
                lo, hi = num(c), num(c, True)
        return max(1, min(64, lo)), max(1, min(64, hi))
    def build(s, name):
        rel = 'loot_tables/' + name.split(':', 1)[1] + '.json'
        if not s.pack.has(rel): return []
        d = s.pack.json(rel); pools = []
        def entries_of(es):
            out = []
            for e in es:
                t = e.get('type', '').split(':')[-1]
                if t == 'item':
                    it = ITEMS.get(e['name'].split(':', 1)[1])
                    if not it: continue
                    chance = 256
                    for c in e.get('conditions', []) or []:
                        if c.get('condition', '').endswith('random_chance'): chance = int(float(c.get('chance', 1)) * 256)
                    lo, hi = s.count(e.get('functions'))
                    out.append((int(e.get('weight', 1)), it[0], it[1], lo, hi, min(256, chance)))
                elif t in ('alternatives', 'group', 'sequence'):
                    out += entries_of(e.get('children', []))
                elif t == 'loot_table' and isinstance(e.get('value'), str):
                    out += s.entries_by_name(e['value'])
            return out
        for p in d.get('pools', []):
            r = p.get('rolls', 1)
            lo, hi = num(r), num(r, True)
            es = entries_of(p.get('entries', []))
            if es: pools.append((max(0, lo), max(0, hi), es))
        return pools
    def entries_by_name(s, name):
        t = s.build(name)
        return [e for (_, _, es) in t for e in es]

# ---------------------------------------------------------------- output
class Out:
    def __init__(s): s.b = bytearray()
    def u8(s, v): s.b += struct.pack('<B', v)
    def u16(s, v): s.b += struct.pack('<H', v)
    def u32(s, v): s.b += struct.pack('<I', v)
    def i16(s, v): s.b += struct.pack('<h', v)

def main():
    src = sys.argv[1] if len(sys.argv) > 1 else 'Structory-1_20.zip'
    dst = sys.argv[2] if len(sys.argv) > 2 else os.path.join(os.path.dirname(__file__), '..', 'mc-rs', 'assets', 'structory.bin')
    pk = Pack(src)
    pools_json = {p[len('worldgen/template_pool/'):-5]: pk.json(p) for p in pk.list('worldgen/template_pool/', '.json')}
    procs_json = {p[len('worldgen/processor_list/'):-5]: pk.json(p) for p in pk.list('worldgen/processor_list/', '.json')}
    pool_names = sorted(pools_json)
    proc_names = sorted(procs_json)
    pool_id = {'structory:' + n: i for i, n in enumerate(pool_names)}
    proc_id = {'structory:' + n: i for i, n in enumerate(proc_names)}

    # ---- block states
    states, state_ix = [], {}
    def state(name, props=None):
        props = tuple(sorted((props or {}).items()))
        k = (name, props)
        if k not in state_ix:
            state_ix[k] = len(states); states.append(k)
        return state_ix[k]
    state('minecraft:air')            # index 0
    unknown = collections.Counter()
    def parse_state(s):
        # "minecraft:oak_log[axis=y]" or {"Name":..,"Properties":{..}}
        if isinstance(s, str):
            if '[' in s:
                nm, rest = s.split('[', 1)
                pr = dict(kv.split('=') for kv in rest.rstrip(']').split(',') if kv)
            else: nm, pr = s, {}
            return state(nm, pr)
        return state(s['Name'], s.get('Properties', {}))

    # ---- templates
    strs, str_ix = [], {}
    def string(s):
        if s not in str_ix: str_ix[s] = len(strs); strs.append(s)
        return str_ix[s]
    tmpl_ix, tmpls = {}, []
    def template(loc):
        if loc in tmpl_ix: return tmpl_ix[loc]
        rel = 'structures/' + loc.split(':', 1)[1] + '.nbt'
        d = nbt(pk.get(rel))
        sx, sy, sz = d['size']
        assert max(sx, sy, sz) < 256, loc
        pal = [parse_state(p) for p in d['palette']]
        cells = [SKIP] * (sx * sy * sz)
        jig, ents = [], []
        for b in d['blocks']:
            x, y, z = b['pos']; st = pal[b['state']]
            name = states[st][0]
            if name == 'minecraft:jigsaw':
                nb = b['nbt']
                front, top = dict(states[st][1])['orientation'].split('_')
                jig.append((x, y, z, DIRS[front], DIRS[top], 0 if nb.get('joint', 'rollable') == 'rollable' else 1,
                            string(nb['name']), string(nb['target']), nb['pool'], nb.get('final_state', 'minecraft:air')))
                st = parse_state(nb.get('final_state', 'minecraft:air'))
            cells[(y * sz + z) * sx + x] = st
            nb = b.get('nbt')
            if nb and states[st][0].split(':')[-1] in ('chest', 'trapped_chest', 'barrel') and nb.get('LootTable'):
                ents.append((x, y, z, 0, nb['LootTable']))
            elif nb and states[st][0].split(':')[-1] == 'spawner':
                sd = (nb.get('SpawnData') or {}).get('entity', {}).get('id') or 'minecraft:zombie'
                ents.append((x, y, z, 1, sd))
        tmpl_ix[loc] = len(tmpls)
        tmpls.append((loc, (sx, sy, sz), cells, jig, ents))
        return tmpl_ix[loc]

    # ---- pools
    pool_out = []
    for n in pool_names:
        pj = pools_json[n]; els = []
        fb = pj.get('fallback', 'minecraft:empty')
        for e in pj['elements']:
            x = e['element']; t = x['element_type'].split(':')[1]
            kind = {'single_pool_element': 1, 'legacy_single_pool_element': 2}.get(t, 0)   # feature_pool_element = empty
            if kind and not pk.has('structures/' + x['location'].split(':', 1)[1] + '.nbt'):
                print('missing template, element dropped:', x['location']); kind = 0
            tix = template(x['location']) if kind else NONE16
            proc = x.get('processors', 'minecraft:empty'); proc = proc if isinstance(proc, str) else 'minecraft:empty'
            els.append((e['weight'], kind, tix, proc_id.get(proc, 255), 1 if x.get('projection') == 'terrain_matching' else 0))
        pool_out.append((els, pool_id.get(fb, NONE16)))
    for t in tmpls:
        for j in t[3]:
            if j[8] != 'minecraft:empty' and j[8] not in pool_id: print('dangling pool (jigsaw skipped in game):', t[0], j[8])
    # states of processor outputs
    # ---- processors, resolved per state. A list = its processors in order; a processor = for each state the rules whose input it
    # matches, in rule order (the first rule that passes wins, `random_*` ones draw only when the block matches).
    def input_chance(pred, st):
        """None = this state does not match, 65535 = always, else a 16 bit chance."""
        nm, props = states[st]
        t = pred['predicate_type'].split(':')[-1]
        if nm != pred.get('block'): return None
        if t == 'block_match': return 65535
        if t == 'random_blockstate_match' and dict(props).get(pred['property']) != pred['value']: return None
        if t in ('random_block_match', 'random_blockstate_match'): return min(65534, int(pred['probability'] * 65536))
        raise ValueError(t)
    def loc_beta(pred):
        t = pred['predicate_type'].split(':')[-1]
        if t == 'always_true': return 255
        assert t == 'block_match', t
        r = to_beta(pred['block'], {})
        return 255 if r == 'UNKNOWN' else (0 if r is None else r[0])
    for n in proc_names:                       # every output state exists before a table is made (tables cover all states)
        for p in procs_json[n]['processors']:
            for r in p.get('rules', []): parse_state(r['output_state'])
    proc_out = []
    for n in proc_names:
        procs = []
        for p in procs_json[n]['processors']:
            per = collections.OrderedDict()
            for st in range(len(states)):
                lst = []
                for r in p.get('rules', []):
                    pr = input_chance(r['input_predicate'], st)
                    if pr is not None: lst.append((pr, loc_beta(r['location_predicate']), parse_state(r['output_state'])))
                if lst: per[st] = lst
            procs.append(per)
        proc_out.append(procs)

    # ---- structures
    def biome_mask(ref):
        names = []
        def expand(r):
            if isinstance(r, dict): r = r['id']
            if r.startswith('#structory:'):
                tag = pk.json('tags/worldgen/biome/' + r[len('#structory:'):] + '.json')
                for v in tag['values']: expand(v)
            else:
                k = r.replace('minecraft:', '') if not r.startswith(('terralith:', '#')) else r
                if k.startswith('#minecraft:'): k = k
                names.append(k)
        expand(ref)
        mask = 0; lost = []
        for k in names:
            if k in BIOMES:
                for b in BIOMES[k]: mask |= 1 << B[b]
            else: lost.append(k)
        return mask, lost
    structs, lost_all = [], set()
    for p in pk.list('worldgen/structure/', '.json'):
        d = pk.json(p); nm = os.path.basename(p)[:-5]
        mask, lost = biome_mask(d['biomes']); lost_all |= set(lost)
        assert d['type'] == 'minecraft:jigsaw' and d['start_height']['absolute'] is not None
        ad = d.get('terrain_adaptation', 'none')
        structs.append(dict(name=nm, mask=mask, pool=pool_id[d['start_pool']], size=d['size'], adapt={'none': 0, 'beard_thin': 1, 'beard_box': 2, 'bury': 3, 'encapsulate': 3}[ad],
                            y=d['start_height']['absolute'], dist=d['max_distance_from_center'], hack=1 if d['use_expansion_hack'] else 0))
    st_ix = {s['name']: i for i, s in enumerate(structs)}
    sets = []
    for p in pk.list('worldgen/structure_set/', '.json'):
        d = pk.json(p); pl = d['placement']
        assert pl['type'] == 'minecraft:random_spread'
        sets.append(dict(entries=[(st_ix[e['structure'].split(':')[1]], e['weight']) for e in d['structures']], salt=pl['salt'], spacing=pl['spacing'],
                         sep=pl['separation'], tri=1 if pl.get('spread_type') == 'triangular' else 0, freq=int(round(pl.get('frequency', 1.0) * 65535))))

    # ---- loot
    loot = Loot(pk)
    for t in tmpls:
        for e in t[4]:
            if e[3] == 0: loot.index(e[4])
    # ---- Beta of every state
    beta_of = lambda st: to_beta(states[st][0], dict(states[st][1]))
    beta = []
    for st in range(len(states)):
        r = beta_of(st)
        if r == 'UNKNOWN':
            unknown[states[st][0]] += 1; r = None
        beta.append((0, 0) if r is None else r)
    # ---- write
    o = Out(); o.b += b'STRY'; o.u8(1)
    o.u16(len(states))
    for i in range(len(states)): o.u8(beta[i][0]); o.u8(beta[i][1])
    # strings: jigsaw names and targets
    o.u16(len(strs))
    for s_ in strs:
        bs = s_.encode(); o.u8(len(bs)); o.b += bs
    o.u16(len(tmpls))
    for loc, (sx, sy, sz), cells, jig, ents in tmpls:
        o.u8(sx); o.u8(sy); o.u8(sz)
        for c in cells: o.u16(c)
        o.u16(len(jig))
        for (x, y, z, f, t, jt, nm, tg, pl, fs) in jig:
            o.u8(x); o.u8(y); o.u8(z); o.u8(f); o.u8(t); o.u8(jt)
            o.u16(nm); o.u16(tg)
            o.u16(pool_id.get(pl, NONE16))
        o.u16(len(ents))
        for (x, y, z, k, a) in ents:
            o.u8(x); o.u8(y); o.u8(z); o.u8(k)
            if k == 0: o.u16(loot.names.index(a))
            else:
                mob = {'minecraft:skeleton': 0, 'minecraft:zombie': 1, 'minecraft:spider': 3, 'minecraft:cave_spider': 3}.get(a, 1)
                o.u16(mob)
    o.u16(len(proc_out))
    for procs in proc_out:
        o.u8(len(procs))
        for per in procs:
            o.u16(len(per))
            for st, lst in per.items():
                o.u16(st); o.u8(len(lst))
                for (pr, loc, out) in lst:
                    o.u16(pr); o.u8(loc); o.u16(out)
    o.u16(len(pool_out))
    for els, fb in pool_out:
        o.u16(fb); o.u8(len(els))
        for (w, kind, tix, proc, proj) in els:
            o.u8(w); o.u8(kind); o.u16(tix); o.u8(proc); o.u8(proj)
    o.u8(len(structs))
    for s_ in structs:
        o.u32(s_['mask']); o.u16(s_['pool']); o.u8(s_['size']); o.u8(s_['adapt']); o.i16(s_['y']); o.u8(s_['dist']); o.u8(s_['hack'])
    o.u8(len(sets))
    for s_ in sets:
        o.u32(s_['salt'] & 0xFFFFFFFF); o.u8(s_['spacing']); o.u8(s_['sep']); o.u8(s_['tri']); o.u16(s_['freq']); o.u8(len(s_['entries']))
        for (ix, w) in s_['entries']: o.u8(ix); o.u8(w)
    o.u8(len(loot.tables))
    for pools in loot.tables:
        o.u8(len(pools))
        for (lo, hi, es) in pools:
            o.u8(min(255, lo)); o.u8(min(255, hi)); o.u8(len(es))
            for (w, i, dmg, a, b, ch) in es:
                o.u8(min(255, w)); o.u16(i); o.u16(dmg); o.u8(a); o.u8(b); o.u16(ch)
    os.makedirs(os.path.dirname(os.path.abspath(dst)), exist_ok=True)
    open(dst, 'wb').write(bytes(o.b))
    cells_total = sum(len(t[2]) for t in tmpls)
    print('wrote', dst, len(o.b), 'bytes:', len(tmpls), 'templates,', cells_total, 'cells,', len(states), 'states,', len(pool_out), 'pools,',
          len(structs), 'structures,', len(sets), 'sets,', len(loot.tables), 'loot tables')
    for s_ in structs:
        print('  %-26s biomes %s' % (s_['name'], [k for k, v in B.items() if s_['mask'] >> v & 1]))
    if lost_all: print('biome names with no stand-in (left out):', sorted(lost_all))
    if unknown: print('block names turned into air (no Beta block, not in the table):', dict(unknown.most_common()))

if __name__ == '__main__':
    main()
