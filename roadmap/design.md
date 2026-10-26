# Design rules for what comes after b1.7.3

Once every b1.7.3 system is ported (see [`missing.md`](missing.md)), new content is added by these rules. The goal is the middle picture of the "good update design" diagram: every new feature is joined to several old ones, and old ones keep a job. Nothing here is code; it is a checklist to fill in *before* a feature is written, and a list of what the port already joins.

## The rules

1. **Two hooks minimum.** A new feature must name at least two existing systems it reads from or writes to. A feature with one hook, or none, is not built yet. (Tested by writing the "Hooks" line below; if it cannot be written, the design is not finished.)
2. **No obsolescence.** A new feature may not make an old block, item, mob or tool pointless. If it overlaps one, the old one gets a new role or the new one is cut. Check every item in the same family (tools, food, fuel, blocks of the same material).
3. **Close the loop.** What the feature takes in must come from somewhere (an existing source) and what it makes must go somewhere (an existing sink: crafting, smelting, trading with the world, redstone, a mob, a block's use). No dead ends in either direction.
4. **Reuse the existing tables.** Recipes go into `craft::recipes()`, sounds into the audio table, blocks into the block tables, so everything that reads those (the recipe book, the sounds, the saves) sees the new entry with no extra line. If that is not true, fix the table first.
5. **Say what it is not.** Anything that is not in b1.7.3 is marked **NOT b1.7.3** in its module doc, the README row and the changelog, and gets a switch if it changes how the old game plays (like `animalAi`).
6. **One written line per feature, kept in this file:** `Name: needs (sources) -> makes (sinks); Hooks: A, B; Overlaps: C (what C does now).`

## Original ideas: recommend them

If an idea is original, meaning it is not in vanilla b1.7.3, it is still welcome: just recommend it, with the hooks it would use and a few search words. The player looks for mods that do something similar and uses them as inspiration, so a proposal does not need to be solved in full. Every such idea is still written as a line in this file before any code (rule 6), marked NOT b1.7.3, and given a switch when it changes how the old game plays (rule 5).

## Hook points of the b1.7.3 base

Systems a new feature can attach to. Each one already exists in the port (the module is in brackets).

Light and the day (`sky`, `climate`, `atmos`); weather and biome climate (`climate`, `biome`); block ticks (`ticks`); redstone and rails (`redstone`, `rail`, `minecart`); mobs, their spawning and their pathfinding (`mobs`, `path`); items, durability and armor (`items`, `gear`, `vitals`); crafting and smelting (`craft`); containers (`chest`); the Nether and the portal (`portal`, `gen::nether`); sound (`audio`); the world save (`save`).

## What the port already joins (read from the code on 2026-10-26: check before relying on it)

- **Recipe book -> `craft::recipes()`.** The book lists the one recipe table the grid uses, so the compass, clock and chest cart recipes appeared in it with no line added. This is rule 4 working.
- **Stamina -> `vitals`.** Health and armor set the bar's maximum, damage marks combat, a respawn refills it (`vitals.rs` holds the `Stamina`). Sprint and jumping spend it (`lib.rs` joins `sprint` to it).
- **Immersive pack -> satchel, book, menu, options, fog, clouds, items, player model, touch buttons.** It is the most joined of the extras: the world's switch decides which inventory screen opens, and the satchel tints each slot by item category (`pack::category`).
- **Smart animals -> `path`.** The same A* walks wolves to their owner and keeps calm animals off cliffs and water; the grazing behaviours hang on the same switch (`Mobs::smart`).
- **Atmosphere -> `biome`, `sky`, both renderers.** One editor drives fog and sky colour per biome.

## Where a feature is thin today (my reading of the code; confirm before acting)

- **Shoulder-surf camera:** only `lib.rs` uses it. It is a view, so one hook may be enough, but it reads nothing from the world (no aim assist with the bow or fishing rod, no look at mobs).
- **Chest minecart:** it has a recipe, can be placed and drawn, but holds nothing, so it is a cart that does nothing a plain cart does not (rule 3). Its 27 slots are the first item in `missing.md`.
- **Sprint and stamina vs food:** b1.7.3 has no hunger, so stamina's only inputs are health and armor. Any later hunger or food system must be joined to the bar, not stand beside it.

## Feature lines

Written 2026-10-26 for entry (20) of the gameplay changelog. All are NOT b1.7.3 and all read what the base already has.

- **Apple:** needs oak leaves (decayed or broken, any world; one in 200) -> food (heals 4) and the golden apple recipe; Hooks: leaf decay and breaking (`ticks`, `lib`), food (`items`), crafting (`craft`); Overlaps: none (the item was in b1.7.3 and could not be found).
- **Autumnal forest:** needs its leaves -> 4 times the apples (one in 50) and a second sapling roll (replanting); Hooks: leaf drops, food, saplings and tree growth; Overlaps: seasonal forest (unchanged).
- **Marsh:** needs wet ground -> clay (bricks), reeds (sugar, paper, books), mushrooms (stew), and slimes on the surface at night (slimeballs for the sticky piston); Hooks: world generation (`populate`), mobs (`mobs`), crafting and redstone (`craft`); Overlaps: swamp (unchanged).
- **Chaparral:** needs dry scrub with no rain -> fire catches and jumps more, so wood and wool there are a risk and the grass burns; Hooks: fire (`ticks`), weather (`Biome::rains`), the burn rates of blocks; Overlaps: desert (no fire change there).
- **Wet woodland:** needs a canopy (20 trees a chunk, leaves take light) -> monsters spawn under it by day (light 8..11 passes the dark check); Hooks: light (`light_opacity`), mobs (`can_spawn`), tree generation; Overlaps: rainforest (unchanged).
- **Snowy birch grove:** needs snow and cold -> wolves live there (the forest and taiga list of b1.7.3, which now spawns for real), tamed with bones; Hooks: climate (`snows`), mobs (spawn lists), wolf taming; Overlaps: taiga (same wolves; the grove has birches and no flowers).
- **Oak woodland:** needs trees and grass -> animals come in herds two larger (food, wool, leather sooner); Hooks: mobs (`pack`), crafting (wool, beds); Overlaps: forest (wolves). This is the weakest line: if it still does not earn its place after play, rule 2 says cut the biome or give it a second hook.

## Ideas waiting (proposed 2026-10-26, nothing built)

Each one serves several biomes at once, so the biomes share systems and none stands alone. Proposed order, easiest first: 2, 3, 1, 5, 4, 6. "Inspiration" is a set of search words for finding mods that do something alike.

- **Fog is cover (proposal 2).** needs the thick fog of the marsh, swamp, rainforest and wet woodland (`atmos` already sets fog per biome) -> mobs see you from less far there, and you see them from less far; walking quietly and ambushes become real; Hooks: atmosphere, mob sight (`sees`, the aggro range in `mobs`); Overlaps: none. Inspiration: "fog stealth", "dynamic fog gameplay".
- **A tower that shows from far (proposal 3).** needs one tall landmark per biome that draws in the far view (structures are not in the LoD today) -> the compass (now craftable) can be tuned to the nearest one with one rare material, and a far landmark makes the sparse structures findable; Hooks: LoD (`lod`), compass (`render/anim.rs`, `craft`), structures (`gen::structory`); Overlaps: the compass, which points to the world spawn only. Inspiration: "waystone", "lodestone compass", "structure locator".
- **Heat and cold are stamina (proposal 1).** needs the temperature the biome manager already has (`climate_at`) -> the desert, savanna, shrubland and chaparral drain stamina faster in open daylight; tundra, ice desert, taiga and the snowy grove slow its recovery away from a torch or furnace; Hooks: stamina (`vitals`), day (`sky`), light (torches and furnaces become a second use), armor (`gear`); Overlaps: none. Inspiration: "temperature survival", "heat and cold stamina".
- **Predators and grazers (proposal 5).** needs the smart-animal paths (`path`) and the wolf lists of entry (20) -> wolves of the taiga, forest and grove hunt sheep; sheep graze and keep a meadow short in the plains and oak woodland, so tending a pasture is a thing to do; wool and bones feed beds and taming; Hooks: mobs, `path`, smart animals, wolf taming, crafting; Overlaps: the oak woodland's herds (they would gain a reason). Needs the `animalAi` switch. Inspiration: "predator prey", "grazing animals".
- **Seasons in the forests (proposal 4).** needs a day count (`world_ticks`) -> in the seasonal and autumnal forests the leaf colour follows the season and apples and saplings drop in some seasons more than others; Hooks: atmosphere tints, leaf drops (entry 20), crops (`ticks`); Overlaps: the autumnal forest's flat apple bonus (it would become the autumn peak). Inspiration: "seasons mod crops".
- **Every biome has one export (proposal 6).** needs a table of one specific material per biome (clay of the marsh, sand and cactus of the desert, spruce and furs of the taiga, apples of the autumnal forest) -> another biome's feature needs it (bricks for the desert's buildings, glass for the forests, food for the cold); the table is the real design and is written before the code; Hooks: world generation, crafting, mobs, structures' loot; Overlaps: whatever a biome already gives. Inspiration: "biome exclusive resources".
