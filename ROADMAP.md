# Roadmap: Minecraft b1.7.3 -> Android in Rust

Fresh start. One milestone per session. Vanilla names. Vulkan-only.
Own simple save format (M7 dropped McRegion). Client-only networking.

## Milestones
| #   | Name | Deliverable |
|-----|------|-------------|
| M0  | Skeleton | Buildable APK, solid color on screen, lifecycle correct |
| M1  | Blocks + chunks | Hand-built 16x16x128 stone chunk visible |
| M2  | Camera + physics | First-person walk, jump, AABB collision |
| M3  | Overworld gen | Beta-1.7 noise: grass/dirt/stone, sea level 64, ores |
| M4  | Biomes + trees + caves | All b1.7.3 biomes + worldgen populate step |
| M5  | Block interaction | Raycast pick, break, place, light update (done, compiled and ran on an Android device: see Changelog) |
| M6  | Inventory + crafting | Survival inv, hotbar, crafting grid, recipes (done, compiled and ran on an Android device: see Changelog) |
| M7  | Save | Own format (not McRegion, by decision): per-chunk RLE files + `level`; autosave, save on pause/exit, resume (done, compiled and ran on an Android device: see Changelog) |
| M8  | Entities + AI | Mobs + item entities |
| M9  | Audio | Positional OGG, music stubs |
| M10 | Multiplayer | Full b1.7.3 client protocol |
| M11 | Nether | Hell biomes, portals, ghast, zombie pigman |
| M12 | Touch UX | Landscape; move stick, look drag, jump button, hotbar tap, pause menu |
| M13 | Optimization | Frustum culling, greedy meshing, profiler |
| M14 | Polish | Splash, main menu, settings, lang |

## Build
- Workspace at `mc-rs/`
- `cargo test --lib` runs the unit tests: 47 before block metadata, 52 with it, 56 with swimming, health, food and frustum culling (Android host such as Termux; see README).
- `cargo run` boots a desktop window (Metal / Vulkan / GL depending on host).
- APK via GitHub Actions `cargo apk build --release` -> `MinecraftB173Rust.apk`.
- A long-lived debug keystore at `keystore/debug.keystore` signs the release
  APK in CI. The same keystore is what `cargo apk build --release` expects at
  `~/.android/debug.keystore` with password `android` if you build locally.

## Source-of-truth rule

Anything that cannot be diffed against the b1.7.3 Java sources or the
decompiled `minecraft/` tree is marked `// UNVERIFIED:` in the code. PRs
that fix UNVERIFIED items should drop the marker and link the source line
they matched.

## Next step

M8 (entities + AI). M7 (save) is written but not compiled: run `cargo test --lib` first. Furnaces smelt (see the furnace entry), so the whole tool chain up to diamond is reachable. M6 is written and the crate type-checks and tests on a host, but nothing has run on a device (M5 included). M4 is done: caves and populate match the real classes bit for
bit (`tools/golden/`). Still approximate in populate (see README): light model, no metadata, no tile entities, no block
ticks (liquids/sand). Sapling growth, fluid flow and falling sand are in M6b (block updates). Ice Desert exists in
BiomeGenBase but climate never selects it; b1.7.3 has no ravines.

## Changelog
- `done` entities cast shadows: dropped items, mobs and falling blocks (written WITHOUT a Rust toolchain: not compiled, not run; the vl.rs test gains one assert, not run; no FPS numbers):
    - Before, only chunk meshes were drawn into the shadow map, so nothing that moves threw a shadow. `gpu/pipeline.rs`: a second `Depth32Float` map `ent_view` (same size, same `shadow_vp`, bound as binding 2 of the shadow group). `lib.rs`: an `entity_shadow_pass` draws the whole item buffer (`item_mesh`, all entities) into it with `shadow_pipeline`, EVERY frame, because the terrain map is cached (`ShadowCache`) and a walking mob would leave a trail in it; it is cleared once more after the last entity is gone (`ent_shadow_dirty`; starts true because a fresh depth texture reads 0 = all shadow).
    - `render/vl.rs` `fs_march`: the surface test samples both maps with the same lookup point and bias and is lit only if both say lit (`min`). The light-shaft march reads it too (`min` of the two taps per sample), so an item or mob leaves a dark shaft-shaped volume along the light; cost: one more depth tap per march sample (10 per half-resolution pixel), the first lever if it shows in the frame time is to skip the tap while `ent_shadow_dirty` is false.
    - Consequences (UNVERIFIED on a device): the entity map is drawn at the terrain map's resolution and distortion, so a 0.25 item shadow is a few texels near the eye and blocky far away; the entity pass costs a 1024^2 clear every frame while any entity exists; foliage tiles are skipped by `vs_shadow`, so a dropped sapling or flower casts nothing. ponytail: no culling of the entity draw; items are flat full-brightness cubes, so their own sun-lit faces are shaded only by the map.
- `done` FIX acne everywhere + sun-facing faces going dark when the player walks up (written WITHOUT a Rust toolchain: not compiled, not run; +1 test `surface_bias_pulls_the_reference_toward_the_sun` not run; the margins below come from a 2-D Python model of `fs_march`'s lookup, not from a device):
    - Root cause, one character in `render/vl.rs` `fs_march`: the surface lookup ADDED its depth bias (`c.z * 0.5 + 0.25 + bias`). The sampler is `LessEqual` (lit when ref <= stored) and a point nearer the sun has the smaller depth, so a positive bias pushes the reference away from the sun and every face into its own shadow. Now `- bias`. The line that was meant to help, the normal offset, only pulls the reference toward the sun by `texel x n.L`; the bias's 0.05 block floor outweighed it wherever the map's texel is < ~0.12 block (`distort()` makes texels small near the map centre, which is the eye within 4 blocks, see `ShadowCache`). That is one cause for both reports: a sun-facing dirt wall self-shadowed inside ~11 blocks of the centre and was lit beyond it (dark as you approach), and flat faces had a negative margin at every distance (acne). With the minus sign the modelled margin is positive for every n.L in 0.02..1 and every distance 0..63 (texel quantisation only).
    - The grazing fade of `vl-grazing-acne-followup.patch` stays, but its premise ("no bias fixes its acne") was the sign bug. Its real job is the ceiling now marked `ponytail:` in the shader: `distort()` is per vertex, so over a 1-block face the shadow depth is interpolated in distorted screen space and errs by ~0.02 block x tan(angle) within ~2 blocks of the map centre; below n.L ~0.3 that beats the bias. If acne is gone on a device, narrow or delete the `smoothstep(0.05, 0.3, nl)` line (the ground then stays bright to a lower sun).
    - Not touched: the shaft march's own lookup (`+ u.light.w`, `DEPTH_BIAS`) samples points in air, where no surface can shadow itself; only the surface test had the wrong sign. `gpu/shadow_test.rs` is not in `gpu/mod.rs` and tests the deleted chunk-shader lookup; it never runs.
- `done` FIX tree shade + stripes on dirt in the light-shaft pass (written WITHOUT a Rust toolchain: not compiled, not run; +1 test `leaves_do_not_shade_like_a_roof` not run, -1 test `pillar_casts_a_shadow_away_from_the_sun` with the code it tested):
    - Dark blocky square under a tree = vanilla sky light: leaves have `Block.lightOpacity` 1, so every cell under a canopy lost one sky level per leaf, flat per face, and the shadow map then darkened the same spot again. `render/mesh.rs`: `bright` now takes a cell as open sky (15) when only air, plants and leaves lie above it (`open`), so the canopy's shade comes from the shadow map alone. A stone roof still dims it. The light engine itself is untouched (mob spawning, grass and the golden tests still see vanilla light); only the drawn brightness changes.
    - Deleted the baked shadow ray that this replaced and that nothing could reach any more: `mesh::build`'s `sun` argument, `occ`, `solid`, `skyl`, `NO_SUN`, `ChunkManager::set_sun` + its field, `sky::sun_key`.
    - Diagonal stripes on dirt (and the lit/dark bands on grass edges) = shadow acne in the surface shadow of `render/vl.rs`: the pass had no normal, only a flat 1-block bias, and the shadow pass culls back faces, so a face turned away from the sun, or parallel to it (every X face: the sun moves in the YZ plane), was compared against a depth that is not its own. Now `fs_march` rebuilds the face normal from the depth buffer (`face_normal`, nearer neighbour per axis, snapped to an axis because every face is axis aligned); `n.L <= 0` is shadowed outright (like the old `N.L`), a lit face looks the map up `NORMAL_BIAS` (1) texel along its normal with a depth bias of 0.05 block + texel x slope (slope capped at 4). A texel's size in blocks follows `distort()` (0.0125 at the eye, ~1.5 at the map edge), so the bias scales with the distance. `SURFACE_BIAS` is gone.
    - Consequences (UNVERIFIED on a device): X faces and every face turned from the sun now take the full `SHADOW_DARK` (0.55): lower it if the shaded sides look too heavy; a pixel on the edge between two faces can pick the neighbour's normal (a 1-pixel line, blurred by the composite); the normal costs 4 extra depth taps per shadowed pixel at half resolution.
- `done` TEMPORARY reset-world button (written WITHOUT a Rust toolchain: not compiled, not run, +1 test `reset_needs_two_taps` not run): a dark-red button under resume in the pause menu (a hollow square: the HUD has no text). The first tap arms it (bright red), the second confirms; resuming or pausing again disarms it. `App::reset_world` deletes the save folder, builds a new `ChunkManager` for `SEED` (the seed's terrain, no edits), preloads, finds the spawn, and resets player, camera, time, weather, inventory, health, mobs, drops, furnaces, block updates and the shadow cache. ponytail: preload runs on the render thread (a short freeze). Remove it with the M14 menu: `PointerRole::Reset`, `LayoutRects.reset`, `TouchUi.reset*`, the HUD block under the resume button, `App::reset_world`.
- `done` shadows moved from the chunk shader into the volumetric pass, and darker (written WITHOUT a Rust toolchain: not compiled, not run; no new test: the shader math needs a GPU, and `gpu/shadow_test.rs`, which tested the chunk shader's lookup, is deleted):
    - `gpu/pipeline.rs`: `fs_main` no longer reads the shadow map (no normal from derivatives, no `N.L` term, no lookup): texture x vertex light, then fog. The map is still drawn (`vs_shadow`) for the volumetric pass.
    - `render/vl.rs`: the half-resolution march also looks up the shadow map at the pixel's own depth (+ `SURFACE_BIAS` ~1 block along the light) and writes it to the green channel; the composite blurs it with the shafts' 4 taps and darkens the frame by `dark = shadow x SHADOW_DARK (0.55) x sun strength` through the existing blend (`a = 1 - (1 - k)(1 - dark)`, glow unchanged). The old shadow darkened 25%. `Params.shadow` is set from `sky::shadow_strength` in `lib.rs`, so no shadow at night or in rain, as before.
    - Consequences (UNVERIFIED on a device): shadow edges are as soft as the half-resolution target (blur + depth-edge bleed); faces turned away from the sun are no longer darkened by `N.L` (only the baked 0.5/0.6/0.8 face shades remain); no shadows under water (the volumetric pass is off there); acne on grazing ground depends on `SURFACE_BIAS`; tune `SHADOW_DARK` / `SURFACE_BIAS`.
- `done` render shape + vertical distance + per-section draws (written WITHOUT a Rust toolchain: not compiled, not run, +1 test `render_shape_and_sections` not run; no FPS numbers):
    - `world/chunks.rs`: a chunk's indices are sorted by 16-block section (`by_section`, one counting pass at mesh time, same vertex buffer). `ChunkManager::draw_ranges` tests each section's box (one block taller each end) against the render shape and the frustum, merges touching sections into one `draw_indexed` range, skips empty ones. Before, one 128-tall box per chunk decided everything.
    - `Shape::Cylinder` = circular horizontal distance (exact in blocks from the eye to the section box, not just the chunk ring); `Shape::Sphere` = 3D distance <= the same radius (the terrain fog is by 3D distance, so the cut-off hides in it). Both are capped by `RENDER_VERT` (3 chunks) above and below the eye. `lib.rs`: `RENDER_SHAPE` (Sphere), `RENDER_VERT`; no settings screen yet (M14).
    - Loading, light and the shadow pass are unchanged (the shadow pass still draws whole meshes in its 64-block box). ponytail: sections out of range still cost memory; far above or below the eye the fog does not hide a vertical cut-off (the fog colour is the horizon's).
    - UNVERIFIED: that it compiles; FPS gain on a device; a face on a section border may sit in the neighbouring section, covered by the one-block margin.
- `done` render cost pass for shadows + light shafts + sun glare (written WITHOUT a Rust toolchain: not compiled, not run, +1 test `shadow_cache_redraws_only_when_stale` not run; no FPS numbers):
    - Shadow map is cached (`render::camera::ShadowCache`): it was redrawn every frame, all chunks in a 128-tall box. Now it is redrawn only when the eye moved > 4 blocks, the light turned > 0.5 degrees, a mesh changed size / an unload happened (`ChunkManager::mesh_gen`), or it was unused the frame before. Between redraws the terrain and the shafts read it with the matrix + light it was drawn with (`upload_uniforms` takes the cached centre), so nothing swims.
    - Shadow pass culls back faces (the mesher drops hidden faces, so the depth map is the same).
    - Light shafts: `shadow_vp * inverse_view` is one CPU-side matrix (one mat4 per sample, was two), `pow(t, 1.5)` is `t * sqrt(t)`; the blend (`LightShafts::composite`) now runs inside the HUD pass, saving one full-screen load/store of the frame. The chunk pass discards its depth when the shafts are off.
    - Sun glare: the quad's half-size 200 -> 90 (`GLARE_R`): the alpha is < 2% past ~40 degrees, and the quad is drawn over the whole sky before the terrain covers it, so it shaded ~5x more pixels than it showed. Terrain fog is left as is (a `length` + smoothstep per pixel, only past 0.6 x far).
    - ponytail: a mesh rebuilt with the same index count (the time-of-day relight) does not refresh the map; an edit that keeps the count shows at the next sun / movement refresh (<= ~2 s). Levers left, in order: `SHADOW_RES` 1024 -> 512, 10 -> 6 march samples in `fs_march`, march at quarter resolution, no moon shafts (`Params.active` only for the sun), a render scale below native.
- `done` FIX black screen from the entry below: the glare branch of the sky shader used the Rust constant `GLARE` inside the WGSL text (no such name: shader compile failed, init panicked, the frame stayed black) and sampled the texture after a branch on a varying. The glare is now its own entry point `fs_glare` and pipeline (`glare_pl`) in `render/sky.rs`, `GLARE` is substituted into the source (`@GLARE@`). Terrain fog (`gpu/pipeline.rs`) was read and is valid WGSL. Still not compiled or run here.
- `done` terrain fog + sun glare (port of AstraLex `NormalFog` / `sunGlare.glsl`; written WITHOUT a Rust toolchain: not compiled, not run; +1 assert in `sky.rs` tests):
    - `gpu/pipeline.rs`: `fs_main` fades to the horizon fog colour (`sky::fog_color`, also the clear colour and the dome's fog) by `1 - (far - d) * 5 / (density * far)`, smoothstepped; `far` = `RENDER_DIST * 16`, density 2.0 (+50% per unit of rain), off under water. `ChunkPipeline::set_fog` writes the new `fog`/`fogp` uniforms (called from `lib.rs` after `upload_uniforms`).
    - `render/sky.rs`: a glare quad (alpha blend, before the sun) shaded per pixel by `VoL^8` as in `sunGlare.glsl`, warm `lightCol`, faded by `sunVisibility` and rain.
    - Not ported: the pack's exponential fog term (about 0.4% at 64 blocks in clear weather), `GetFogColor` (HDR; the existing vanilla fog colour is used), glare over terrain (the pack adds it to the whole frame; here the sky pass draws it, so terrain hides it), underwater fog.
    - UNVERIFIED: `GLARE` (1.2) and density 2.0 look on a device.
- `done` volumetric light (port of AstraLex `volumetricLight.glsl` + the `LIGHT_SHAFT` part of `composite1.glsl`; written WITHOUT a Rust toolchain: not compiled, not run; +3 tests in `render/vl.rs` not run, their numbers come from a line-by-line Python transcription of the GLSL):
    - `render/vl.rs` (new): `params()` = every per-frame scalar of the pack (`lightCol`, `sunVisibility`, `shadowFade`, `lightShaftTime`, rain/night multipliers, `endurance`, `vlPower`), CPU side, pure. `LightShafts::draw` = two passes after the terrain, before the HUD: a half-resolution march (10 samples, `pow(i + dither + 0.714, 1.5) * minDistFactor`, fov falloff, `sqrt(vl * visibility)`) through the sun's shadow map into an RGBA8 target, then a full-screen pass (4-tap blur, square, `NdotU` direction term, colour, additive/mixed blend) with blend `rgb + dst * (1 - a)`.
    - `lib.rs`: the shadow pass now also runs while the light is above the horizon (the sun at sunrise/sunset/rain, the moon at night: `lightVec` flips at timeAngle 0.5325 / 0.9675 like the pack), with that light as the pass's direction; the terrain still takes its shadow only from `sun_strength`. `gpu/context.rs`: the depth texture is also a texture binding. `gpu/pipeline.rs`: `SHADOW_DISTORT` and `shadow_layout` are public.
    - Deviations (all in the file header): `InterleavedGradientNoise` instead of blue noise, mc-rs's own shadow lookup (distort + depth) and a march that ends at `SHADOW_RADIUS` (outside the map a sample counts as lit), no coloured shadows, `DEPTH_BIAS` ~0.25 block. `LIGHT_SHAFT` / `STRENGTH` in `vl.rs` stand for the pack's `#define`s.
    - Not ported: underwater shafts (off while the eye is in water), `SMOKER_LIGHT_SHAFT` (needs `noise.png`), End shafts, cave fade (the pack's `isEyeInCave` is 0 above y = 5).
    - UNVERIFIED: brightness on a device (the pack's numbers assume its HDR scene + tone map; if the glow is too strong lower `STRENGTH`), cost of the march on low-end GPUs (half resolution is the first lever, then fewer samples), and the second shadow pass at night.
- `done` real block textures (written WITHOUT a Rust toolchain: not compiled, +1 test and 1 changed test not run; `tools/gen_terrain.py` ran, its tiles were looked at):
    - `assets/terrain.rgba` = the real `terrain.png` (256x256, from the jar) with the biome tint baked in: grass top, tall grass, fern, reeds x the grass colour, leaves x the foliage colour,
      spruce 0x619961, birch 0x80A755 (own tile 255, the png has no birch leaf tile); water, lava and ice made opaque. Climate is one fixed point (temperature 0.8, rainfall 0.4).
    - `render/atlas.rs`: the atlas texture is 256 x 288, the png on top and under it the old flat-colour strip (items, mobs, falling blocks and blocks without a tile keep it).
      `terrain_tile(id, meta, side)` = `getBlockTextureFromSideAndMetadata` of the blocks the world has (grass, log, leaves, sandstone, cactus, furnace, workbench, pumpkin, wool,
      slabs, tnt, ...); `face_uv` picks it per face (the mesher's face order -> the Java side), `terrain_uv` insets a hair so Nearest never reads the next tile.
    - `render/mesh.rs`: faces use `face_uv`; plants are full-height 0.9-wide crossed quads (`renderCrossedSquares`) with the cut-out texture (`cross_shape` still sizes pick and collision).
    - `gpu/pipeline.rs`: the shader drops texels with alpha < 0.5 (plants, glass); the shadow pass finds foliage by terrain tile (`PLANT` is built from `terrain_tile`).
    - Not done: biome tint per block (needs a colour vertex attribute), grass side overlay (tile 38), animated water/lava/fire, translucent water/ice/glass, mipmaps (distant tiles shimmer), furnace/pumpkin/chest
      facing (front is always +Z until metadata carries it), item sprites and mob/GUI textures (M14).
- `done` M6b block updates (`world/ticks.rs`, hooks in `world/chunks.rs`, `world/gen/populate.rs`, `render/items.rs`, `lib.rs`; written WITHOUT a Rust toolchain: not compiled, not run, expect a compile fix or two; no new tests yet):
  - `ChunkManager` logs every notifying write (`set_block` / `set_block_meta`: x, y, z, old id, new id); `set_quiet` is the `setBlockAndMetadata` / `setBlockMetadata` write that does not. `Ticks::step` (20 Hz, next to the furnaces) runs the scheduled
    ticks (`TickUpdates`: BTreeSet by due time, dedup set, <= 1000 per tick, skipped unless the cell's +-8 area is loaded), then 80 random ticks per lit final chunk within 9 chunks with the Java LCG (`World.tick`), then replays the log:
    `onBlockAdded`, `onBlockRemoval`, `onNeighborBlockChange` of the six neighbours, until it is empty.
  - Water/lava: `BlockFlowing.updateTick` (levels, downward flow with +8, sources from 2 neighbours for water, lava creeps 2 levels and 3 of 4 ticks held back, `calculateFlowCost` / `getOptimalFlowDirections`), `BlockStationary` waking up,
    `checkForHarden` (obsidian / cobblestone), tick rates 5 / 30. Items in the way drop (water only).
  - Sand, gravel: schedule 3 ticks, `EntityFallingSand` as `ticks::Falling` (gravity 0.04, drag 0.98, <= 64, drawn as a 0.98 flat box), lands or drops as an item.
  - `BlockLeaves` decay (bit 8, 4 steps from a log, `onBlockRemoval` of logs +-4 and leaves +-1 sets the bit), `BlockSapling` (bit 8 then `growTree` through `populate::grow_sapling` on a 2x2 chunk `Region`; the 4 chunks are re-lit),
    `BlockFlower.canBlockStay` for flowers, tall grass, dead bush, mushrooms, crops (they drop and vanish), `BlockGrass` spread/death, `BlockCrops` growth + `getGrowthRate`, `BlockFarmland`, `BlockReed`, `BlockCactus`.
  - Not ported: fire (lava does not ignite), mushroom spread, snow/ice, rain wetting farmland, falling sand when chunks are far (vanilla drops it instantly), trampling farmland, saving pending ticks (vanilla does not).
  - UNVERIFIED: mushroom `canBlockStay` light limit (only the opaque ground is checked), the `Block.tickOnLoad` list was read from the `setTickOnLoad(true)` calls.
- `done` real-time shadows (port of shaderLABS/Shadow-Tutorial, written on a Linux host: `render::camera`, `world::sky` and `gpu::pipeline` compiled and tested there (14 tests, incl. a headless lavapipe render, `gpu/shadow_test.rs`); `lib.rs` edits NOT compiled; not run on a device):
    - `gpu/pipeline.rs`: shadow map `Depth32Float` 1024^2 + comparison sampler (group 1), `vs_shadow` (= `shadow.vsh`: `distort()`, foliage parked off-screen via a tile bitmask built from `chunk::is_plant`), `fs_main` (= `gbuffers_terrain.fsh`: face normal from position derivatives, `SHADOW_BRIGHTNESS`, `sqrt(N.L)` lit mix, strength fade). `camera::shadow_view_proj` (ortho 64 blocks around the eye), `sky::sun_dir` / `shadow_strength`; `lib.rs` runs the extra depth pass only while the strength is > 0 (never at night or in rain).
    - Replaces the baked shadow ray: `lib.rs` no longer calls `set_sun`, so `mesh::build`'s `occ` code, `ChunkManager::set_sun` and `sky::sun_key` are dead and can go.
    - Not ported: colored shadows (no translucent pass yet: glass and water are opaque cubes) and `SHADOW_DISTORT_ENABLED`/`COLORED_SHADOWS` toggles. Block light is not exempt from shadow (the vertex `light` merges sky and block light, the pack only darkens the sky part).
    - Deviation: `NORMAL_BIAS` offsets in blocks (one texel of the distorted map x `SHADOW_BIAS`); the pack's offset is 1/R of that and left flat ground full of acne in the test.
    - UNVERIFIED: 64-block radius, 1024 map and the 37-degree test scene on real hardware; cost of the second pass on low-end GPUs (Back-face culling in the shadow pipeline is the first thing to try).
- `done` all overworld mobs (written WITHOUT a Rust toolchain: not compiled, 3 tests in `mobs.rs` not run; `tools/gen_names.py` now also writes `RESIST`):
    - `world/mobs.rs`: the pig's `Pig` became `Mob` + `Kind` with `spec` = (`setSize`, health, `moveSpeed`, `attackStrength`): Pig, Cow, Sheep (`getRandomFleeceColor`),
      Chicken (egg every 6000..12000 ticks, `motionY x 0.6` fall), Wolf (neutral until hit), Squid (`EntitySquid` swim vector), Zombie, PigZombie (400..800
      ticks of anger, 0.95 speed), Giant (6x zombie), Skeleton (`attackEntity` arrows every 30 ticks, `EntityArrow` 4 damage), Creeper (30-tick fuse at 3 blocks,
      `Explosion.doExplosionA` rays, power 3, resistance = `RESIST`/hardness x 5, /5), Spider (leaps at 2..6, gives up in light, climbs walls), Slime (size
      1/2/4, health size^2, hops, four halves on death, damages only above size 1). Melee is `EntityMob.attackEntity` (dist < 2, 20 ticks apart). Zombies and
      skeletons catch fire in sun (`onLivingUpdate`, 1 damage a second). Loot = `getDropItemId` x `rand(3)` (skeleton arrows + bones, sheep 1 wool of its colour,
      squid 1..3 ink, slime balls at size 1). Despawn = `func_27021_X`.
    - Spawning (UNVERIFIED, not `SpawnerAnimals`): animals on lit grass every 400 ticks + a burst at start (cap 12; sheep 12, pig 10, chicken 10, cow 8), monsters
      every 40 ticks in the dark (`light <= rand(8)`, cap 10; slimes only in `func_997_a(987234911)` chunks below y 16), squid in water y 46..62 (cap 3), 24..48
      blocks from the player.
    - `Chunks::light` (= `World.getBlockLightValue`), `lib.rs`: `Ctx`/`Ev` hooks (`Hurt` -> `Vitals.hurt`, so armor works; `Boom` -> `App::explode`: blocks
      cleared, 30% drop, player damage), tap-to-hit now for every mob (`Mobs::hit`).
    - `render/items.rs`: `parts` = the b1.7.3 models (`ModelQuadruped`, `ModelBiped`/`ModelZombie`/`ModelSkeleton`, `ModelCreeper`, `ModelChicken`, spider, squid,
      wolf, slime from memory) as flat wool-coloured boxes (red when hurt, white fuse flash); arrows are small boxes.
    - Not done: Ghast and the zombie-pigman spawn (M11), wolf spawning (needs biomes), taming, shearing, milking, saddles, A*, sounds, saving mobs, explosion
      exposure/knockback, arrows sticking in blocks, a skeleton/spider jockey, the textures (M14).
- `done` cheap table fills (written WITHOUT a Rust toolchain: not compiled, `table_fills` test not run; `craft.rs`, `items.rs`, `vitals.rs`, `lib.rs`):
    `RecipesDyes` (shapeless, `ShapelessRecipes.matches`: `Recipe.w == 0`), `RecipesIngots`, `RecipesArmor` (all 20 pieces; chain wants fire), cookie, bucket;
    food `heal_amount` for apple, bread, golden apple, fish, cookie; raw fish smelts; lava bucket burns 20000 and leaves an empty bucket;
    buckets stack 1. Armor: `Inventory.slots` is 40 long (36..40 = `armorInventory`, helmet at 39), `SlotArmor` rules in `Screen::armor_ok`,
    `Vitals.armor` / `absorb` = `EntityPlayer.damageEntity` (25ths with a carried remainder), worn pieces wear by the damage dealt (`lib.rs`).
    - Save format changed (inventory 36 -> 40 slots): an old `level` file fails to decode and starts a new game.
    - Not done: filling/emptying buckets (no fluid flow), armor model and HUD armor bar, shift-click, a pumpkin stack onto the head (needs a single pumpkin in hand).
    - UNVERIFIED: the mode toggle moved to the top right (152, 8) to clear the armor column; armor tile colours are invented.
- `done` item-name tooltip (written WITHOUT a Rust toolchain: not compiled, +2 tests not run; `tools/gen_names.py` ran and its output was
  spot-checked against `lang/en_US.lang`): `GuiContainer.drawScreen` shows `translateNamedKey(getItemName())` in a 75% black box (3 units of
  padding) for the hovered slot when the cursor is empty. Touch has no hover, so it shows while a finger is down: the stack under it, or the
  picked-up stack (a tap picks the slot's stack up, which would otherwise hide it). `render/font.rs` (generated: `default.png` glyphs +
  `FontRenderer` widths, ASCII 32..126 only), `world/names.rs` (generated: `setBlockName`/`setItemName` + lang), `items::name` (wool, dye,
  slab, charcoal by damage), `HudPipeline::push_text` / `text_width` / `push_tooltip`, hud quad cap 2048 -> 4096.
    - Not done: tooltips in the hotbar/in play (b1.7.3 has none), non-ASCII, text anywhere else (that is M14), item sprites.
    - UNVERIFIED: the tooltip spot (34 units above the finger, below it near the top edge) is invented; vanilla uses mouse + (12, -12).
- `done` sun, moon, stars, sunrise glow and weather (written WITHOUT a Rust toolchain, then compiled and ran on an Android device; the 5 new tests not run; the Java reference
  numbers behind 3 of them were generated from the real classes, `tools/golden/G.java` `sky()`; +5 tests):
    - `world/sky.rs`: `Weather` = `World.updateWeather` (rain/thunder timers on a `java.util.Random`, strengths +-0.01 per tick; started clear, not
      saved, a loaded world starts clear), `skylight_subtracted` / `sky_color` now take the rain and thunder strengths, `fog_color` (horizon colour
      `func_4096_a` mixed with the sky, rain/thunder darkening), `sunrise_color`, `star_brightness`, `star_vertices` (`renderStars`, `Random(10842)`).
      Golden: celestial angle, horizon colour, sunrise glow, star brightness, skylight with weather, 1M ticks of weather for 2 seeds, the star quads.
    - `render/sky.rs` (new) + `assets/sky.rgba`: one pass drawn first in the frame, depth never tested or written, around the viewer (view matrix keeps
      only the rotation, far plane 2 x 128): dome (16 above, fog 0..0.8 x far by distance), sunrise fan (alpha blend), sun 60 / moon 40 wide
      (the real `terrain/sun.png` / `moon.png`, additive, alpha `1 - rain`), stars (additive, brightness `star_brightness x (1 - rain)`), dark plane 16 below.
      The frame now clears to the fog colour, not the sky colour. `lib.rs`: `App.sky`, `App.weather`, `weather_ticks` (whole ticks run, set to the loaded time).
    - ponytail: vanilla draws no sky below NORMAL view distance and mixes the fog by `1 - (1/(4 - renderDistance))^0.25`; mc-rs always draws it with
      NORMAL's numbers (mix 0.24, far 128), though terrain reaches 64. The brightness term of the fog colour (darker in caves) is left out.
    - UNVERIFIED: surface gamma. Colours are written as in vanilla (framebuffer values); on an sRGB surface (wgpu's usual pick) they come out lighter,
      like the old clear colour did. Sun/moon use an sRGB texture like the block atlas.
    - Not done: rain/snow streaks (`renderRainSnow`; `rain.png`/`snow.png` are in the jar), rain particles and sound, lightning (`EntityLightningBolt`)
      and the sky flash, snow layers and ice from weather, clouds, fog on terrain, water/lava fog, the weather is not saved. A new world waits
      12000..180000 ticks (10..150 minutes) for the first rain; to see it sooner, call `weather.tick()` in a loop at start-up until `weather.rain(1.0) > 0`.
- `done` M7 fix: autosave no longer interrupts play every 5 s (compiled and ran on an Android device). `App::save` called `close_screen()`
  unconditionally, and `close_screen` also runs `touch.set_screen(false)`, so each autosave (screen closed) reset the touch state and cut
  the held move stick / look drag / dig. Now `save` closes a screen only when one is open (`lib.rs`, one `if`). Pause and exit saves are
  unchanged (they still turn an open screen into dropped items first).
    - UNVERIFIED: `input/touch_ui.rs` was not in the zip, so that `set_screen(false)` resets the pointer state is inferred from the call
      site, not read. If actions still drop, look there first.
    - Still open: autosave writes the level + up to 24 chunks on the render thread; on a slow phone that can hitch a frame. If so, lower
      `AUTOSAVE_CHUNKS` or move the writes to a thread.
- `done` M7 save (written WITHOUT a Rust toolchain, then compiled and ran on an Android device; tests not run; +2 tests). Decision: it does
  not follow b1.7.3 (no McRegion, NBT, zlib): the aim is fewer lines and a smaller, faster save, not a loadable vanilla world.
    - `world/save.rs` (new): `encode_chunk`/`decode_chunk` = populated flag + run-length-coded block ids + metadata nibbles (runs of
      1..=255), `Level` encode/decode (time, position, spawn, look, hotbar slot, health/air/fire, 36 slots, furnaces, dropped items),
      `write` = temp file + rename, so a kill mid-write keeps the old file. Decoding checks every length, stack size and float and
      answers `None` for anything off (chunk: generated again; level: new game); it never panics.
    - Only chunks that differ from the seed are written: `Entry.dirty` is set by an edit and by `populate` (on all 4 chunks it writes
      into), cleared by a write. Untouched chunks are regenerated, light and height map are recomputed on load. `ChunkManager::with_dir`
      scans the folder once into `saved`; `stream` reads saved chunks (8 per frame) instead of asking a worker; `flush(budget)` writes
      dirty ones; unloading writes a dirty chunk first. `preload(cx, cz)` now takes the centre chunk (it was `lo, hi`).
    - `lib.rs`: the world resumes where the player stood (`Level` -> `App::restore`); a new world still searches a spawn (`find_spawn`).
      Autosave every 5 s of play (level + 24 chunks, skipped with a screen open), and a full save on `Pause` and `TerminateWindow`,
      which matters: `TerminateWindow` drops the whole `App`, so before this a trip to the home screen lost the world. An open screen is
      closed first, so its cursor and grid become dropped items. Folder: `internal_data_path()/world-<seed hex>/` (`c.<cx>.<cz>`, `level`).
      To start a new world, clear the app's data (no menu until M14).
    - This also ends the old ponytail note in `stream` (an unloaded chunk was populated again on return).
    - Not saved: pause state, mining progress, velocity of dropped items (they come back at rest), the sky/light, anything a menu would
      choose. Furnace facing is still lost when a furnace lights (`set_block` clears metadata).
    - Known ceiling: a hard kill (not a normal pause/exit) can lose up to 5 s of play, and can leave a chunk border with a tree cut off
      if a populated chunk was written before its neighbour. Chunk writes and reads run on the render thread (a few small files per frame).
    - Tests: `chunk_and_level_round_trip` (codec, run splitting, every corrupt case), `edits_survive_a_restart` (flush, new manager loads
      it, corrupt file forgotten).
- `done` M13 frustum culling (host-checked: type-checks, 48 tests pass; compiled and ran on an Android device; the FPS gain is still unmeasured):
    - `render/camera.rs`: `Frustum::from_view_proj(proj * view)` takes the six planes (Gribb-Hartmann, depth 0..1 so the near plane is row 2),
      `intersects_aabb` is the usual conservative test against the corner furthest along each plane normal. Test covers ahead, behind,
      sides, above/below, past the far plane, the camera inside a box, the wide 2:1 shape and a 90 degree turn.
    - `world/chunks.rs`: `meshes_where(visible)` yields the meshes whose box (16 x `H` x 16 at the chunk key) passes the test. `lib.rs` draws
      only those. Only the terrain draw is culled: dropped items, the outline and the HUD are tiny.
    - Not done on purpose: tightening the box to each chunk's real height (it is the full 128), occlusion culling, greedy meshing, a profiler.
      Culling also trims only draw calls and GPU vertex work; chunk meshing and generation still run for the whole ring.
- `done` sword, hoe, shears (host-checked like M6: crate type-checks, 47 tests pass, compiled and ran on an Android device):
    - `craft.rs`: swords (ids 267/268/272/276/283, 1.5x on everything, 15x and harvest on web, 2 wear per block), hoes (290..294, no wear
      from digging, 1 per tilling), shears (359, 238 uses: 15x and harvest on web, 15x on leaves, 5x on wool, wear only on leaves and web).
      Recipes `RecipesWeapons` x 5, hoe x 5 from `RecipesTools`, shears from two iron ingots. Wear is `wear_on_break(held, block)`.
    - `lib.rs`: a hoe tills dirt, or grass with air above and not from below, into farmland (60) and wears 1 (`ItemHoe.onItemUse`);
      shears on leaves drop the leaves block itself (`BlockLeaves.harvestBlock`) via `Drops::spawn_stack`.
    - Not done: farmland reverting to dirt, its 15/16 height, crops (so a hoe only makes farmland), sheep, bow and arrow.
- `done` mushroom stew + eating (`world/items.rs`, `world/craft.rs`, `world/vitals.rs`, `lib.rs`; written WITHOUT a Rust toolchain, then compiled and ran on an Android device; tests not run, +1 test and extra asserts):
    `ItemFood.onItemRightClick` / `ItemSoup`: a tap that neither places a block nor opens a workbench/furnace uses the held item (as
    `Minecraft.clickMouse` -> `sendUseItem` does, aimed at a block or not). Food uses up one and `Vitals::heal`s (`EntityLiving.heal`: nothing when dead,
    capped at 20, damage window back to 10); eating at full health still uses it up. Stew (282) heals 10, stacks to 1 and leaves an empty bowl (281).
    Recipes: bowl (3 planks in a V, x4) and stew (red + brown mushroom + bowl, either order). Only the stew is in `heal_amount`; apple, bread, pork, golden
    apple and fish are one line each (ids in the comment) once mobs, crops or chests exist. Not done: eating animation and sound, the hold-to-eat delay
    (b1.7.3 has none: eating is instant).
- `done` health, damage, death (`world/vitals.rs`, `lib.rs`; written WITHOUT a Rust toolchain, then compiled and ran on an Android device; test not run, +1 test):
    `Vitals` = `EntityLiving.attackEntityFrom` (20 health, the 10-tick damage window: an equal or smaller hit inside it is ignored, a bigger one
    pays the difference), `Entity.updateFallState` + `EntityLiving.fall` (`ceil(fall - 3)`, water cancels it), drowning (300 air, then 2 damage every
    20 ticks once it runs out, `isInsideOfMaterial`), lava (4 damage per window + 600 ticks of fire, 1 damage per second while burning, water puts
    it out) and the void (eye below -64). Frame-based fall check, everything else on the 20 Hz tick. `physics::step` now returns (water, lava).
    HUD: 10 hearts above the hotbar, air bubbles while the eye is under water. Death (`App::die`): the open screen and the whole inventory drop where
    he died, the pause menu turns red and its resume button respawns at the first spawn point with full health and a clean `Vitals`.
    b1.7.3 has no hunger and no natural regeneration (only Peaceful heals), so health stays down until food is ported. Not done: suffocation in
    blocks, hurt flash/blink, knockback, burning and hurt sounds, fire overlay, death camera roll, items burning in lava, per-difficulty damage,
    bed spawn, health in the save (M7).
- `done` fluids are not solid + swimming (`world/physics.rs`; written WITHOUT a Rust toolchain, then compiled and ran on an Android device; test not run, +1 test):
    the player used to stand on water and lava because physics treated every id > 0 as solid. Ids 8..=11 are now passable; `step` takes the
    held jump flag and ports `Entity.handleWaterMovement`/`handleLavaMovement` (box shrunk 0.4 top and bottom) and
    `EntityLiving.moveEntityWithHeading`: no gravity in a fluid, vertical drag 0.8 (lava 0.5) and 0.02 blocks/tick sink, held jump +0.04/tick,
    wall hop 0.3/tick when 0.6 higher is free. Simplified: every fluid cell is a source block (flow levels need metadata), horizontal speed is
    scaled to the terminal swim speed (2.0 m/s water, 0.8 lava) instead of accumulating. Not done: no flow push, no splash/bubbles, no breath
    or lava damage (next: health), dropped items still treat fluids as solid.
- `done` block metadata (written WITHOUT a Rust toolchain, then compiled and ran on an Android device; tests not run; the Java side was run). Foundation for wood
  and leaf species, wool colours, slabs, stairs, doors and the bed, so the chunk format does not change again when they arrive:
    - `world/chunk.rs`: `Nibbles` = `NibbleArray` (4 bits per cell, even cell index = low nibble, same index as the blocks), 16 KB
      per chunk, bytes identical to the McRegion `Data` tag (M7 writes `bytes()` as is). `world/chunks.rs`: `Entry.data`,
      `ChunkManager::meta`, `set_block_meta` (= `Chunk.setBlockIDWithMetadata`) and `set_block` (= `setBlockID`: nothing on the same
      id, metadata cleared when the id changes). `render/mesh.rs` reads only its own chunk's metadata.
    - `world/gen/populate.rs`: `Region` carries the 2x2 metadata; `set` clears it like `setBlock`, `set_meta` writes it like
      `setBlockAndMetadata`. Written where the Java writes: birch leaves/logs 2 (`WorldGenForest`), spruce 1 (`WorldGenTaiga1/2`), tall
      grass 1 or 2 (rainforest, `WorldGenTallGrass`), pumpkin facing 0..3. Oak and big trees stay 0. No Random draw changed.
    - Golden: `tools/golden/G.java`'s fake `World` keeps a `NibbleArray` per chunk with the Java setter semantics and prints a `META` line
      (FNV of each chunk's packed nibbles) after every `POP` line. Regenerated: every old line is byte for byte unchanged; 6 of the 11
      cases have non-zero metadata. `populate_matches_java` compares the Rust bytes with those hashes.
    - `world/items.rs`: `damage_dropped(block, meta)` (`Block.damageDropped`), `spawn_block(block, meta, pos)` (lib.rs reads the
      metadata before it clears the cell), `placed_meta` (`Item.getPlacedBlockMetadata`; leaves get bit 8), `stack_tile`; double
      slab drops 2 single slabs (`BlockStep`). `lib.rs` places with `set_block_meta`.
    - `render/atlas.rs`: the atlas is 16x32 (`TILES_W`/`TILES_H`, `gpu/pipeline.rs` follows); tiles 256.. are variants picked by
      `tile_of(id, meta)`: spruce/birch logs and leaves, the 15 coloured wools (`EntitySheep.fleeceColorTable`). Hotbar, inventory and
      dropped items use `stack_tile`, so a red wool stack is red. Log and leaf colours are UNVERIFIED stand-ins until M14 textures.
    - Not done: slab/stair/door/bed shapes, rotation on placement (no `onBlockPlacedBy`: furnace and pumpkin facing is stored by worldgen
      only), leaf decay and sapling growth, saving the data (M7), blocks the player cannot make yet (no wool dye or slab recipes).
      Note for M7: `ChunkManager::set_block` clears metadata, so the furnace lit/unlit flip loses the facing once furnaces have one.
- `done` touch spread (fix from a device report: with only tap = left click, a stack could not be split across a grid):
  a picked-up stack floats above the finger (`TouchUi::cursor_pos`), and dragging it over other slots puts one item into each
  slot it enters, the start slot included (`Screen::drop_one`: empty or same item with room, never a swap, never an output
  slot). A press that lifts where it began is still a tap, and the "1" toggle stays. The yellow cursor box is gone. The
  gesture state machine is in `lib.rs` (`on_screen_event`) and only `drop_one` is unit-tested; 47 tests pass, compiled and ran on an Android device.
- `done` furnace + smelting (same host checks as M6: crate type-checks, 46 tests pass, compiled and ran on an Android device):
    - `world/craft.rs`: `Furnace` = `TileEntityFurnace.updateEntity` (fuel used up when the fire starts, 200 ticks per item, output stacks
      to 64, relights from the next fuel without a flip), `FurnaceRecipes` (iron/gold/diamond ore, sand -> glass, cobble -> stone, clay -> brick,
      cactus -> green dye, log -> charcoal) and `getItemBurnTime` (wood blocks 300, stick/sapling 100, coal 1600). `ContainerFurnace` slots and the
      `SlotFurnace` rule: the output takes nothing, right click takes half.
    - `lib.rs`: furnaces live in a map by block position, created on first use, ticked at 20 Hz only while their chunk is loaded; the block swaps
      61 <-> 62 (lit, light 13) when the fire flips. Using a furnace opens its screen (flame and arrow bars). Breaking one spills its three slots.
    - Atlas colours for the new blocks (glass, wool, metal blocks, workbench, furnaces, snow block, glowstone); iron/gold/diamond tools had none.
    - Not done: the furnace contents are not saved (M7), furnace facing (no metadata), fuel/lava bucket, raw pork and fish.
- `done` M6 crafting + tools (written on a Linux host: the whole crate type-checks and its 45 tests pass against a stubbed
  `android-activity`; compiled and ran on an Android device):
    - `world/craft.rs` (new): `EnumToolMaterial` (wood/stone/iron/diamond/gold), pickaxe/axe/shovel ids 256..=286, `getStrVsBlock`,
      `ItemPickaxe.canHarvestBlock` (obsidian level 3, diamond/gold/redstone level 2, iron/lapis level 1, else rock/iron),
      `ItemSpade.canHarvestBlock` (snow). 26 shaped recipes (`RecipesTools` loop for 5 materials x 3 tools, planks, sticks,
      workbench, chest, furnace, torch, sandstone, snow/clay blocks, glowstone, wool); the matcher is `ShapedRecipes.matches`.
    - `Screen`: `ContainerPlayer` (2x2) and `ContainerWorkbench` (3x3). Touch has no mouse, so a tap is a left click and a toggle
      button ("1") turns taps into right clicks (put one, take half). Result slot as in `SlotCrafting`: refuses input, a take uses up
      one of each ingredient. Closing (or tapping outside the panel with the cursor full) throws the cursor stack and the grid out
      along the look direction (`dropPlayerItem`, 40 tick pickup delay).
    - `items.rs`: `Inventory` is 36 slots (`mainInventory`; pickups already fill the hotbar first), `ItemStack.damage` is `u16` (tool
      wear), tools stack to 1, `Inventory::damage` = `ItemStack.damageItem` (breaks past maxUses), `Drops::throw`.
    - `dig.rs`: `strength` and `Dig::tick` take the held item: `canHarvestBlock` false = the 1/100 rate, else tool efficiency / hardness / 30,
      with the /5 for water and air. `can_harvest` = material half (`harvestable_by_hand`) or the held tool. `lib.rs` breaks the block,
      wears the tool, and spawns the drops only if `can_harvest`: **stone by hand now drops nothing**, like the original.
    - Touch: inventory button left of pause; using a workbench (tap) opens the 3x3 screen instead of placing. While a screen is open the move
      stick, look, jump, hotbar and crosshair are hidden and every press is a tap. `hud.rs` quad capacity 512 -> 2048 for the screen.
    - Not done: smelting (so no iron/gold ingots), hoes, swords, shears (web and leaves still need them), armor slots, shift-click,
      item sprites (tools are a handle plus a head shape in the material colour), the other ~100 `CraftingManager` recipes.
    - The `ponytail` note in the drops entry below ("bare hands harvest everything") is superseded by this entry.
- `done` drops + hotbar inventory (`world/items.rs`, `render/items.rs`; the logic is unit-tested on a Linux host in a scratch crate,
  `lib.rs`/wgpu NOT compiled here: ):
    - Drops port `Block.dropBlockAsItem`: `quantityDropped`, then per item one `nextFloat` (chance 1.0) and `idDropped`, then the 3
      position draws, in the Java order on one `JavaRandom`. Table covers every block worldgen makes plus the metadata-free extras
      (stone -> cobblestone, grass/farmland -> dirt, gravel -> flint 1/10, coal/diamond/lapis(4-8 dye:4)/redstone(4-5) ores, leaves ->
      sapling 1/20, tall grass -> seeds 1/8, web, clay x4, snow block x4, glowstone dust 2-4, reeds, furnace, signs, redstone torch;
      none for fluids, ice, glass, bookshelf, TNT, spawner, snow layer, dead bush). Not yet: doors, bed, crops, slabs, stairs (need metadata).
    - `EntityItem`: 0.25 box, 20 Hz, gravity 0.04, drag 0.98, ground friction 0.588 (ice 0.9604), 10 tick pickup delay, 6000 tick life, pickup when
      the player's box grown 1.0 in x/z touches it. Collision tests the box centre line (ponytail, see items.rs). Max 256 entities.
    - `Inventory`: 9 hotbar slots, `addItemStackToInventory` (top up a matching stack, else first empty, partial adds), max stack 64 (16 snowball, 1 sign/door/bed).
      Starts empty; a tap places the selected item if it is a block (id < 256) and uses one up. Full hotbar leaves the item on the ground.
    - Bare hands harvest everything for now (vanilla `canHarvestBlock` would drop nothing from stone without a pickaxe; no tools/crafting yet).
    - Open: placing plants has no `canBlockStay` check; items are drawn as cubes, full brightness, no spin; no 27-slot inventory screen (M6).
- `done` hardness-based digging (compiled and ran on an Android device): `world/dig.rs` ports `Block.blockStrength` and the `PlayerControllerSP` damage
  counter at 20 Hz (hardness table from Block.java; bare hand: rock/iron/snow/web dig 3.3x slower; 5x slower when airborne or
  head under water; instant for hardness 0; 5-tick wait after a break; bedrock/portal unbreakable). Touch: hold still on the
  right half for 0.4 s to start digging, keep holding to finish (aiming elsewhere restarts the damage); progress bar under the
  crosshair (vanilla's crack textures need M14). Tap still places. No tools or drops yet (inventory).
- `done` faster world loading (compiled and ran on an Android device): chunk map uses a cheap integer hasher instead of SipHash; light work is bounded by
  10 ms per frame instead of a cell count and lights up to 2 chunks per frame; seam recomputation skips cells above the tallest
  column and block light when neither chunk has any; `App::init` (off the render thread) lights and meshes the spawn area
  (>= 24 chunk meshes or 8 s) before the first frame. Still open: populate and meshing run on the render thread.
- `done` plants drawn (written WITHOUT a Rust toolchain, then compiled and ran on an Android device; tests not run): flowers, mushrooms, tall grass, dead bush
  and reeds are two crossed double-sided quads (`chunk::cross_shape`, sized from each block's bounds; flat colours until M14
  textures), lit by their own cell, and pickable/breakable as a whole cell. Still not solid. Snow layer (78) is still not drawn.
- `done` day/night cycle (written WITHOUT a Rust toolchain, then compiled and ran on an Android device; tests not run):
    - `world/sky.rs`: ports of `calculateCelestialAngle`, `calculateSkylightSubtracted`, the sky colour (`func_4079_a` +
      `getSkyColorByTemp`, AWT HSB maths). 24000 ticks per day at 20 ticks/s, new world starts at tick 0 (sunrise). 2 tests.
    - `ChunkManager::set_sky_sub` (0..=11) marks every mesh stale when it changes; `mesh::build` takes `sky_sub` and uses
      `max(sky - sky_sub, block)` per cell, so night is dark but lava/torches stay lit. `temperature_at` feeds the sky colour.
    - Not done: sun, moon and stars are not drawn (done later: see the sun/moon/weather entry), no fog, no rain/thunder terms, time is not saved (M7) and not adjustable.
- `done` M5 block interaction (written WITHOUT a Rust toolchain, then compiled and ran on an Android device; tests not run, ):
    - `world/pick.rs`: port of `World.func_28105_a` + `Block.collisionRayTrace` (f64, 4.0 reach), place cell/replaceable/player-overlap rules. 3 tests.
    - `world/chunks/light.rs`: port of the b1.7.3 light engine (sky + block light, region queue, `MetadataChunkBlock` relaxation,
      `Chunk.func_1003_g` relight, `generateSkylightMap`). Chunks light once final; edits go through `ChunkManager::set_block`. 1 test.
    - `chunk.rs`: `light_opacity`/`light_value` tables derived from Block.java, `brightness`, `height_map`. `mesh.rs`: faces use the
      neighbour cell's brightness x vanilla face shade (1.0/0.5/0.8/0.6); `push_box`. `render/outline.rs`: selection outline.
    - Touch: tap on the right half places, hold breaks (repeat 0.25 s), drag looks. Crosshair added. Hotbar slot = block placed.
    - Simplifications: instant break, no drops/consumption (M6), no day/night (`skylightSubtracted` = 0), plants/liquids not pickable,
      light seams across a not-yet-final neighbour are fixed by the seam strips when it is lit.
- `done` M4d caves, trees, populate (compiled and tested on a Linux host with rustc 1.85; compiled and ran on an Android device):
    - `gen/caves.rs`, `gen/populate.rs` (new), `OverworldGenerator::generate` now ends with the caves; `populate_ores`
      and `next_u31` deleted. `mobSpawnerNoise` is the 8th noise stack (after the 7 terrain ones).
    - `world/chunks.rs`: raw ring is now `RENDER_DIST + 2`, populate (1 chunk/frame, nearest first) needs the 2x2 raw
      chunks, a chunk is meshed once populate ran on it and its -X/-Z/-X-Z neighbours (`is_final`); a populate that
      touches meshed chunks marks them for a rebuild. Spawn area is `preload(-1, 2)`.
    - Golden: `G.java` has a fake `World` (2x2 chunks, column light model, no-op springs) and prints RAW/POP hashes plus a
      block table; 3 seeds, 11 cases (taiga+snow, forest, lava lake, dungeon, pumpkin, mushrooms, cactus, clay, big trees).
    - Plants are neither meshed nor solid (`is_plant`); atlas colours for the new blocks. Spawn search skips trees.
    - The M4f chunk manager compiled first try on rustc 1.85; its 3 tests pass.
- `done` M4f chunk manager (`world/chunks.rs`; written without a Rust toolchain, NOT compiled, tested or run on
  a device yet, so  on the first CI run):
    - Replaces the 48x48 super-chunk (gone from `lib.rs`, `mesh.rs`, `populate_ores`). Chunks live in a
      `HashMap<(cx, cz), Entry>`; each has its own vertex/index buffer (empty chunks have none).
    - Ring: circle of `RENDER_DIST` (4) chunks is meshed and drawn, `RENDER_DIST + 1` is generated (a chunk is
      meshed only when its 4 neighbours exist, so seams cull correctly), unload beyond `RENDER_DIST + 2`.
    - Generation runs on 1-2 `std::thread` workers, each with its own generator (result does not depend on
      the worker: `generate()` reseeds per chunk). At most `2 x workers` requests in flight, rebuilt nearest-first
      from the player position every frame. Meshing + upload stay on the render thread, 2 chunks per frame.
    - Physics reads blocks through `ChunkManager::block`; an unloaded chunk reads as solid (invisible wall,
      no falling into void). Spawn: the 3x3 chunks around the origin are generated in `App::init`.
    - `populate_ores` is chunk-local (veins clipped at chunk edges; still UNVERIFIED, still replaced in M4d).
    - Tests: mesher edge culling (4 sides), block lookup across negative chunk coords, ring load/unload.
    - Not done on purpose: frustum culling (M13; the draw loop has a marker comment), greedy meshing, mesh on
      workers, saving chunks.
- `aef24f7` M3e-atlas-fix (1/2): 16x16 atlas + panic catcher in init.
- `2a5497f` M3e-atlas-fix (2/2): Uint32 indices. **Black screen root cause**: the
  48x128x48 super-chunk pushes the per-frame index count past `u16::MAX`, so
  `IndexFormat::Uint16` silently clipped the draw and wgpu presented black.
  Atlas size was a red herring.
- `ad33e0d` M12: Touch UX overlay (HUD + d-pad + look-stick + pause menu).
  Files: `src/input/{mod,touch_ui}.rs`, `src/render/hud.rs`,
  `src/lib.rs`, `ROADMAP.md`. Region-based touch UI state machine plus a
  separate 2D orthographic wgpu pass for the HUD overlay (alpha-blended,
  depth-less, runs after the chunk pass). 9 unit tests, 15/15 pass.
  Build status:
    - `cargo test --lib`: 15/15 pass on the host.
    - `cargo build --target aarch64-linux-android --lib --release`: clean,
      4.5 MB `libmc_rs.so`, ELF confirmed AArch64.
    - `cargo apk build --release`: not run locally (this host is aarch64
      Android, the SDK build-tools are x86_64 glibc ELFs). The CI workflow
      at `.github/workflows/ci.yml` runs the same command on a GitHub-hosted
      ubuntu-22.04 runner — that APK build is the end-to-end check.
  Verified against b1.7.3 sources:
    - Hotbar cell count (9), 20 px cell width, 22 px tall, centred horizontally.
      `GuiIngame.java` lines 58-62.
    - `moveStrafe` / `moveForward` semantics ({-1,0,1}). `MovementInputFromOptions.java`
      lines 67-83.
    - Jump velocity 8.4 m/s when on_ground. Preserved from the M2 baseline.
  UNVERIFIED (no b1.7.3 source — b1.7.3 PC has no touch UX at all):
    - Move-stick radius, look-zone split (left/right half), jump button position.
    - Pause and resume button rectangles.
  Follow-up: cross-check the touch rectangles against Pocket Edition 0.x
  or any later touch-based Minecraft client. Run `cargo apk build --release`
  on the CI runner and verify on a phone.
- `done` M4a-c: faithful Random/noise, climate biomes, terrain + surface. Golden test now
  passes (it failed before M4e because of the float constants below).
- `done` M4e fix pass (built and unit-tested on a host with a stubbed android-activity;
  NOT yet run on a device):
    - 48x48 super-chunk was indexed with the 16-deep `(x<<11)|(z<<7)|y` shifts; the bits
      overlap, so 1536 of 2304 columns overwrote each other (scrambled terrain). Now
      `(x*depth+z)*128+y` in lib.rs, mesh.rs, overworld.rs. Test: single block = one cube.
    - Jump never fired: `ground_test` probed under the box centre, not the feet. Test:
      standing on a floor is on_ground and can jump.
    - Camera sat 2.52 above the feet (1.62 added to the box centre). Now 1.62 above feet.
    - `camera.aspect` was never initialised (1.0); set at init, and the swapchain is
      re-synced to the native window size on resize/insets/focus events.
    - Status + nav bars hidden via JNI (`immersive.rs`) on window creation and focus gain.
    - Climate noise scales: `(double)0.025F` / `(double)0.05F` as in the Java.
  Confirmed on a device (screenshot): terrain renders correctly and blocks are square, so the
  stretch is gone. Still open: caves; populate; faithful ores; HUD does not yet avoid the
  display cutout.
    - Black strip at the left edge (display cutout area): fixed in `immersive.rs` with
      `layoutInDisplayCutoutMode = SHORT_EDGES` (Android 9+). Not yet seen on a device.
- `pending` M4d: caves, trees, populate (next, see "Next step" above).
