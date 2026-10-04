//! DIHASILKAN oleh tools/gen_blocks.py dari registri b1.7.3 asli (96 blok). Jangan edit manual.
//! Perilaku (tick, interaksi) ada di modul terpisah per `BlockKind`; file ini hanya data statis.

use crate::material::Material;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StepSound { Powder, Wood, Gravel, Grass, Stone, Metal, Glass, Cloth, Sand }

impl StepSound {
    /// Nama suara langkah ("step.xxx") dan pitch. Metal = stone dengan pitch 1.5.
    pub fn step(self) -> (&'static str, f32) {
        match self {
            StepSound::Powder => ("step.stone", 1.0),
            StepSound::Wood => ("step.wood", 1.0),
            StepSound::Gravel => ("step.gravel", 1.0),
            StepSound::Grass => ("step.grass", 1.0),
            StepSound::Stone => ("step.stone", 1.0),
            StepSound::Metal => ("step.stone", 1.5),
            StepSound::Glass => ("step.stone", 1.0),
            StepSound::Cloth => ("step.cloth", 1.0),
            StepSound::Sand => ("step.sand", 1.0),
        }
    }
    /// Suara saat blok pecah/ditaruh (Java: stepSoundDir()).
    pub fn break_sound(self) -> &'static str {
        match self {
            StepSound::Powder => "step.stone",
            StepSound::Wood => "step.wood",
            StepSound::Gravel => "step.gravel",
            StepSound::Grass => "step.grass",
            StepSound::Stone => "step.stone",
            StepSound::Metal => "step.stone",
            StepSound::Glass => "random.glass",
            StepSound::Cloth => "step.cloth",
            StepSound::Sand => "step.gravel",
        }
    }
}

/// Kelas Java asal tiap blok; dipakai untuk dispatch perilaku.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlockKind {
    Stone,
    Grass,
    Dirt,
    Plain,
    Sapling,
    Flowing,
    Stationary,
    Sand,
    Gravel,
    Ore,
    Log,
    Leaves,
    Sponge,
    Glass,
    Dispenser,
    SandStone,
    Note,
    Bed,
    Rail,
    DetectorRail,
    PistonBase,
    Web,
    TallGrass,
    DeadBush,
    PistonExtension,
    Cloth,
    PistonMoving,
    Flower,
    Mushroom,
    OreStorage,
    Step,
    TNT,
    Bookshelf,
    Obsidian,
    Torch,
    Fire,
    MobSpawner,
    Stairs,
    Chest,
    RedstoneWire,
    Workbench,
    Crops,
    Farmland,
    Furnace,
    Sign,
    Door,
    Ladder,
    Lever,
    PressurePlate,
    RedstoneOre,
    RedstoneTorch,
    Button,
    Snow,
    Ice,
    SnowBlock,
    Cactus,
    Clay,
    Reed,
    JukeBox,
    Fence,
    Pumpkin,
    Netherrack,
    SoulSand,
    GlowStone,
    Portal,
    Cake,
    RedstoneRepeater,
    LockedChest,
    TrapDoor,
}

#[derive(Clone, Copy, Debug)]
pub struct BlockDef {
    pub id: u8,
    pub kind: BlockKind,
    /// Kunci terjemahan ("tile.xxx"); None untuk blok internal piston.
    pub name: Option<&'static str>,
    pub texture: u16,
    pub material: Material,
    /// -1 = tak bisa dihancurkan
    pub hardness: f32,
    /// Sudah dikali 3 seperti Java (setResistance)
    pub resistance: f32,
    pub light_value: u8,
    pub light_opacity: u8,
    /// Nilai awal. Daun (fancy graphics) dan slab ganda berubah saat runtime.
    pub opaque: bool,
    pub render_as_normal: bool,
    /// 0 = kubus, 1 = silang, 2 = obor, 3 = api, 4 = cairan, 5 = kabel redstone, 6 = tanaman, 7 = pintu, 8 = tangga, 9 = rel, 10 = tangga blok, 11 = pagar, 12 = tuas, 13 = kaktus, 14 = ranjang, 15 = repeater, 16/17 = piston, -1 = khusus
    pub render_type: i8,
    pub collidable: bool,
    pub sound: StepSound,
    /// minX, minY, minZ, maxX, maxY, maxZ
    pub bounds: [f32; 6],
    pub tick_on_load: bool,
    pub is_container: bool,
    /// Java: canBlockGrass[] (sudah dibalik dari material)
    pub can_block_grass: bool,
    /// Java: field_28032_t (tidak memberi tahu tetangga saat metadata berubah)
    pub no_neighbor_notify_on_meta: bool,
    pub enable_stats: bool,
    pub slipperiness: f32,
    pub tick_rate: u8,
}

const DEFS: [BlockDef; 96] = [
    BlockDef { id: 1, kind: BlockKind::Stone, name: Some("tile.stone"), texture: 1, material: Material::Rock, hardness: 1.5, resistance: 3.0e1, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 2, kind: BlockKind::Grass, name: Some("tile.grass"), texture: 3, material: Material::Grass, hardness: 0.6, resistance: 3.0, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Grass, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: true, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 3, kind: BlockKind::Dirt, name: Some("tile.dirt"), texture: 2, material: Material::Ground, hardness: 0.5, resistance: 2.5, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Gravel, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 4, kind: BlockKind::Plain, name: Some("tile.stonebrick"), texture: 16, material: Material::Rock, hardness: 2.0, resistance: 3.0e1, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 5, kind: BlockKind::Plain, name: Some("tile.wood"), texture: 4, material: Material::Wood, hardness: 2.0, resistance: 15.0, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Wood, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 6, kind: BlockKind::Sapling, name: Some("tile.sapling"), texture: 15, material: Material::Plants, hardness: 0.0, resistance: 0.0, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 1, collidable: true, sound: StepSound::Grass, bounds: [0.099999994, 0.0, 0.099999994, 0.9, 0.8, 0.9], tick_on_load: true, is_container: false, can_block_grass: true, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 7, kind: BlockKind::Plain, name: Some("tile.bedrock"), texture: 17, material: Material::Rock, hardness: -1.0, resistance: 1.8e7, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: false, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 8, kind: BlockKind::Flowing, name: Some("tile.water"), texture: 205, material: Material::Water, hardness: 1.0e2, resistance: 5.0e2, light_value: 0, light_opacity: 3, opaque: false, render_as_normal: false, render_type: 4, collidable: true, sound: StepSound::Powder, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: true, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: false, slipperiness: 0.6, tick_rate: 5 },
    BlockDef { id: 9, kind: BlockKind::Stationary, name: Some("tile.water"), texture: 205, material: Material::Water, hardness: 1.0e2, resistance: 5.0e2, light_value: 0, light_opacity: 3, opaque: false, render_as_normal: false, render_type: 4, collidable: true, sound: StepSound::Powder, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: false, slipperiness: 0.6, tick_rate: 5 },
    BlockDef { id: 10, kind: BlockKind::Flowing, name: Some("tile.lava"), texture: 237, material: Material::Lava, hardness: 0.0, resistance: 0.0, light_value: 15, light_opacity: 255, opaque: false, render_as_normal: false, render_type: 4, collidable: true, sound: StepSound::Powder, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: true, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: false, slipperiness: 0.6, tick_rate: 30 },
    BlockDef { id: 11, kind: BlockKind::Stationary, name: Some("tile.lava"), texture: 237, material: Material::Lava, hardness: 1.0e2, resistance: 5.0e2, light_value: 15, light_opacity: 255, opaque: false, render_as_normal: false, render_type: 4, collidable: true, sound: StepSound::Powder, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: true, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: false, slipperiness: 0.6, tick_rate: 30 },
    BlockDef { id: 12, kind: BlockKind::Sand, name: Some("tile.sand"), texture: 18, material: Material::Sand, hardness: 0.5, resistance: 2.5, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Sand, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 3 },
    BlockDef { id: 13, kind: BlockKind::Gravel, name: Some("tile.gravel"), texture: 19, material: Material::Sand, hardness: 0.6, resistance: 3.0, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Gravel, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 3 },
    BlockDef { id: 14, kind: BlockKind::Ore, name: Some("tile.oreGold"), texture: 32, material: Material::Rock, hardness: 3.0, resistance: 15.0, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 15, kind: BlockKind::Ore, name: Some("tile.oreIron"), texture: 33, material: Material::Rock, hardness: 3.0, resistance: 15.0, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 16, kind: BlockKind::Ore, name: Some("tile.oreCoal"), texture: 34, material: Material::Rock, hardness: 3.0, resistance: 15.0, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 17, kind: BlockKind::Log, name: Some("tile.log"), texture: 20, material: Material::Wood, hardness: 2.0, resistance: 1.0e1, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Wood, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 18, kind: BlockKind::Leaves, name: Some("tile.leaves"), texture: 52, material: Material::Leaves, hardness: 0.2, resistance: 1.0, light_value: 0, light_opacity: 1, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Grass, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: true, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: false, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 19, kind: BlockKind::Sponge, name: Some("tile.sponge"), texture: 48, material: Material::Sponge, hardness: 0.6, resistance: 3.0, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Grass, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 20, kind: BlockKind::Glass, name: Some("tile.glass"), texture: 49, material: Material::Glass, hardness: 0.3, resistance: 1.5, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Glass, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 21, kind: BlockKind::Ore, name: Some("tile.oreLapis"), texture: 160, material: Material::Rock, hardness: 3.0, resistance: 15.0, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 22, kind: BlockKind::Plain, name: Some("tile.blockLapis"), texture: 144, material: Material::Rock, hardness: 3.0, resistance: 15.0, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 23, kind: BlockKind::Dispenser, name: Some("tile.dispenser"), texture: 45, material: Material::Rock, hardness: 3.5, resistance: 17.5, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: true, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 4 },
    BlockDef { id: 24, kind: BlockKind::SandStone, name: Some("tile.sandStone"), texture: 192, material: Material::Rock, hardness: 0.8, resistance: 4.0, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 25, kind: BlockKind::Note, name: Some("tile.musicBlock"), texture: 74, material: Material::Wood, hardness: 0.8, resistance: 4.0, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Powder, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: true, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 26, kind: BlockKind::Bed, name: Some("tile.bed"), texture: 134, material: Material::Cloth, hardness: 0.2, resistance: 1.0, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 14, collidable: true, sound: StepSound::Powder, bounds: [0.0, 0.0, 0.0, 1.0, 0.5625, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: false, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 27, kind: BlockKind::Rail, name: Some("tile.goldenRail"), texture: 179, material: Material::Circuits, hardness: 0.7, resistance: 3.5, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 9, collidable: true, sound: StepSound::Metal, bounds: [0.0, 0.0, 0.0, 1.0, 0.125, 1.0], tick_on_load: false, is_container: false, can_block_grass: true, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 28, kind: BlockKind::DetectorRail, name: Some("tile.detectorRail"), texture: 195, material: Material::Circuits, hardness: 0.7, resistance: 3.5, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 9, collidable: true, sound: StepSound::Metal, bounds: [0.0, 0.0, 0.0, 1.0, 0.125, 1.0], tick_on_load: true, is_container: false, can_block_grass: true, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 20 },
    BlockDef { id: 29, kind: BlockKind::PistonBase, name: Some("tile.pistonStickyBase"), texture: 106, material: Material::Piston, hardness: 0.5, resistance: 2.5, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 16, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 30, kind: BlockKind::Web, name: Some("tile.web"), texture: 11, material: Material::Web, hardness: 4.0, resistance: 2.0e1, light_value: 0, light_opacity: 1, opaque: false, render_as_normal: false, render_type: 1, collidable: true, sound: StepSound::Powder, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 31, kind: BlockKind::TallGrass, name: Some("tile.tallgrass"), texture: 39, material: Material::Plants, hardness: 0.0, resistance: 0.0, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 1, collidable: true, sound: StepSound::Grass, bounds: [0.099999994, 0.0, 0.099999994, 0.9, 0.8, 0.9], tick_on_load: true, is_container: false, can_block_grass: true, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 32, kind: BlockKind::DeadBush, name: Some("tile.deadbush"), texture: 55, material: Material::Plants, hardness: 0.0, resistance: 0.0, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 1, collidable: true, sound: StepSound::Grass, bounds: [0.099999994, 0.0, 0.099999994, 0.9, 0.8, 0.9], tick_on_load: true, is_container: false, can_block_grass: true, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 33, kind: BlockKind::PistonBase, name: Some("tile.pistonBase"), texture: 107, material: Material::Piston, hardness: 0.5, resistance: 2.5, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 16, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 34, kind: BlockKind::PistonExtension, name: None, texture: 107, material: Material::Piston, hardness: 0.5, resistance: 2.5, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 17, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 35, kind: BlockKind::Cloth, name: Some("tile.cloth"), texture: 64, material: Material::Cloth, hardness: 0.8, resistance: 4.0, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Cloth, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 36, kind: BlockKind::PistonMoving, name: None, texture: 0, material: Material::Piston, hardness: -1.0, resistance: 0.0, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: -1, collidable: true, sound: StepSound::Powder, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: true, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 37, kind: BlockKind::Flower, name: Some("tile.flower"), texture: 13, material: Material::Plants, hardness: 0.0, resistance: 0.0, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 1, collidable: true, sound: StepSound::Grass, bounds: [0.3, 0.0, 0.3, 0.7, 0.6, 0.7], tick_on_load: true, is_container: false, can_block_grass: true, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 38, kind: BlockKind::Flower, name: Some("tile.rose"), texture: 12, material: Material::Plants, hardness: 0.0, resistance: 0.0, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 1, collidable: true, sound: StepSound::Grass, bounds: [0.3, 0.0, 0.3, 0.7, 0.6, 0.7], tick_on_load: true, is_container: false, can_block_grass: true, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 39, kind: BlockKind::Mushroom, name: Some("tile.mushroom"), texture: 29, material: Material::Plants, hardness: 0.0, resistance: 0.0, light_value: 1, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 1, collidable: true, sound: StepSound::Grass, bounds: [0.3, 0.0, 0.3, 0.7, 0.4, 0.7], tick_on_load: true, is_container: false, can_block_grass: true, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 40, kind: BlockKind::Mushroom, name: Some("tile.mushroom"), texture: 28, material: Material::Plants, hardness: 0.0, resistance: 0.0, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 1, collidable: true, sound: StepSound::Grass, bounds: [0.3, 0.0, 0.3, 0.7, 0.4, 0.7], tick_on_load: true, is_container: false, can_block_grass: true, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 41, kind: BlockKind::OreStorage, name: Some("tile.blockGold"), texture: 23, material: Material::Iron, hardness: 3.0, resistance: 3.0e1, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Metal, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 42, kind: BlockKind::OreStorage, name: Some("tile.blockIron"), texture: 22, material: Material::Iron, hardness: 5.0, resistance: 3.0e1, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Metal, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 43, kind: BlockKind::Step, name: Some("tile.stoneSlab"), texture: 6, material: Material::Rock, hardness: 2.0, resistance: 3.0e1, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 44, kind: BlockKind::Step, name: Some("tile.stoneSlab"), texture: 6, material: Material::Rock, hardness: 2.0, resistance: 3.0e1, light_value: 0, light_opacity: 255, opaque: false, render_as_normal: false, render_type: 0, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 0.5, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 45, kind: BlockKind::Plain, name: Some("tile.brick"), texture: 7, material: Material::Rock, hardness: 2.0, resistance: 3.0e1, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 46, kind: BlockKind::TNT, name: Some("tile.tnt"), texture: 8, material: Material::Tnt, hardness: 0.0, resistance: 0.0, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Grass, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 47, kind: BlockKind::Bookshelf, name: Some("tile.bookshelf"), texture: 35, material: Material::Wood, hardness: 1.5, resistance: 7.5, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Wood, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 48, kind: BlockKind::Plain, name: Some("tile.stoneMoss"), texture: 36, material: Material::Rock, hardness: 2.0, resistance: 3.0e1, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 49, kind: BlockKind::Obsidian, name: Some("tile.obsidian"), texture: 37, material: Material::Rock, hardness: 1.0e1, resistance: 6.0e3, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 50, kind: BlockKind::Torch, name: Some("tile.torch"), texture: 80, material: Material::Circuits, hardness: 0.0, resistance: 0.0, light_value: 14, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 2, collidable: true, sound: StepSound::Wood, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: true, is_container: false, can_block_grass: true, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 51, kind: BlockKind::Fire, name: Some("tile.fire"), texture: 31, material: Material::Fire, hardness: 0.0, resistance: 0.0, light_value: 15, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 3, collidable: false, sound: StepSound::Wood, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: true, is_container: false, can_block_grass: true, no_neighbor_notify_on_meta: true, enable_stats: false, slipperiness: 0.6, tick_rate: 40 },
    BlockDef { id: 52, kind: BlockKind::MobSpawner, name: Some("tile.mobSpawner"), texture: 65, material: Material::Rock, hardness: 5.0, resistance: 25.0, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Metal, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: true, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: false, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 53, kind: BlockKind::Stairs, name: Some("tile.stairsWood"), texture: 4, material: Material::Wood, hardness: 2.0, resistance: 15.0, light_value: 0, light_opacity: 255, opaque: false, render_as_normal: false, render_type: 10, collidable: true, sound: StepSound::Wood, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 54, kind: BlockKind::Chest, name: Some("tile.chest"), texture: 26, material: Material::Wood, hardness: 2.5, resistance: 12.5, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Wood, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: true, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 55, kind: BlockKind::RedstoneWire, name: Some("tile.redstoneDust"), texture: 164, material: Material::Circuits, hardness: 0.0, resistance: 0.0, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 5, collidable: true, sound: StepSound::Powder, bounds: [0.0, 0.0, 0.0, 1.0, 0.0625, 1.0], tick_on_load: false, is_container: false, can_block_grass: true, no_neighbor_notify_on_meta: true, enable_stats: false, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 56, kind: BlockKind::Ore, name: Some("tile.oreDiamond"), texture: 50, material: Material::Rock, hardness: 3.0, resistance: 15.0, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 57, kind: BlockKind::OreStorage, name: Some("tile.blockDiamond"), texture: 24, material: Material::Iron, hardness: 5.0, resistance: 3.0e1, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Metal, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 58, kind: BlockKind::Workbench, name: Some("tile.workbench"), texture: 59, material: Material::Wood, hardness: 2.5, resistance: 12.5, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Wood, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 59, kind: BlockKind::Crops, name: Some("tile.crops"), texture: 88, material: Material::Plants, hardness: 0.0, resistance: 0.0, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 6, collidable: true, sound: StepSound::Grass, bounds: [0.0, 0.0, 0.0, 1.0, 0.25, 1.0], tick_on_load: true, is_container: false, can_block_grass: true, no_neighbor_notify_on_meta: true, enable_stats: false, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 60, kind: BlockKind::Farmland, name: Some("tile.farmland"), texture: 87, material: Material::Ground, hardness: 0.6, resistance: 3.0, light_value: 0, light_opacity: 255, opaque: false, render_as_normal: false, render_type: 0, collidable: true, sound: StepSound::Gravel, bounds: [0.0, 0.0, 0.0, 1.0, 0.9375, 1.0], tick_on_load: true, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 61, kind: BlockKind::Furnace, name: Some("tile.furnace"), texture: 45, material: Material::Rock, hardness: 3.5, resistance: 17.5, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: true, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 62, kind: BlockKind::Furnace, name: Some("tile.furnace"), texture: 45, material: Material::Rock, hardness: 3.5, resistance: 17.5, light_value: 13, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: true, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 63, kind: BlockKind::Sign, name: Some("tile.sign"), texture: 4, material: Material::Wood, hardness: 1.0, resistance: 5.0, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: -1, collidable: true, sound: StepSound::Wood, bounds: [0.25, 0.0, 0.25, 0.75, 1.0, 0.75], tick_on_load: false, is_container: true, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: false, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 64, kind: BlockKind::Door, name: Some("tile.doorWood"), texture: 97, material: Material::Wood, hardness: 3.0, resistance: 15.0, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 7, collidable: true, sound: StepSound::Wood, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: false, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 65, kind: BlockKind::Ladder, name: Some("tile.ladder"), texture: 83, material: Material::Circuits, hardness: 0.4, resistance: 2.0, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 8, collidable: true, sound: StepSound::Wood, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: true, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 66, kind: BlockKind::Rail, name: Some("tile.rail"), texture: 128, material: Material::Circuits, hardness: 0.7, resistance: 3.5, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 9, collidable: true, sound: StepSound::Metal, bounds: [0.0, 0.0, 0.0, 1.0, 0.125, 1.0], tick_on_load: false, is_container: false, can_block_grass: true, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 67, kind: BlockKind::Stairs, name: Some("tile.stairsStone"), texture: 16, material: Material::Rock, hardness: 2.0, resistance: 3.0e1, light_value: 0, light_opacity: 255, opaque: false, render_as_normal: false, render_type: 10, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 68, kind: BlockKind::Sign, name: Some("tile.sign"), texture: 4, material: Material::Wood, hardness: 1.0, resistance: 5.0, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: -1, collidable: true, sound: StepSound::Wood, bounds: [0.25, 0.0, 0.25, 0.75, 1.0, 0.75], tick_on_load: false, is_container: true, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: false, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 69, kind: BlockKind::Lever, name: Some("tile.lever"), texture: 96, material: Material::Circuits, hardness: 0.5, resistance: 2.5, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 12, collidable: true, sound: StepSound::Wood, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: true, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 70, kind: BlockKind::PressurePlate, name: Some("tile.pressurePlate"), texture: 1, material: Material::Rock, hardness: 0.5, resistance: 2.5, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 0, collidable: true, sound: StepSound::Stone, bounds: [0.0625, 0.0, 0.0625, 0.9375, 0.03125, 0.9375], tick_on_load: true, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 20 },
    BlockDef { id: 71, kind: BlockKind::Door, name: Some("tile.doorIron"), texture: 98, material: Material::Iron, hardness: 5.0, resistance: 25.0, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 7, collidable: true, sound: StepSound::Metal, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: false, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 72, kind: BlockKind::PressurePlate, name: Some("tile.pressurePlate"), texture: 4, material: Material::Wood, hardness: 0.5, resistance: 2.5, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 0, collidable: true, sound: StepSound::Wood, bounds: [0.0625, 0.0, 0.0625, 0.9375, 0.03125, 0.9375], tick_on_load: true, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 20 },
    BlockDef { id: 73, kind: BlockKind::RedstoneOre, name: Some("tile.oreRedstone"), texture: 51, material: Material::Rock, hardness: 3.0, resistance: 15.0, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 30 },
    BlockDef { id: 74, kind: BlockKind::RedstoneOre, name: Some("tile.oreRedstone"), texture: 51, material: Material::Rock, hardness: 3.0, resistance: 15.0, light_value: 9, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: true, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 30 },
    BlockDef { id: 75, kind: BlockKind::RedstoneTorch, name: Some("tile.notGate"), texture: 115, material: Material::Circuits, hardness: 0.0, resistance: 0.0, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 2, collidable: true, sound: StepSound::Wood, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: true, is_container: false, can_block_grass: true, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 2 },
    BlockDef { id: 76, kind: BlockKind::RedstoneTorch, name: Some("tile.notGate"), texture: 99, material: Material::Circuits, hardness: 0.0, resistance: 0.0, light_value: 7, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 2, collidable: true, sound: StepSound::Wood, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: true, is_container: false, can_block_grass: true, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 2 },
    BlockDef { id: 77, kind: BlockKind::Button, name: Some("tile.button"), texture: 1, material: Material::Circuits, hardness: 0.5, resistance: 2.5, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 0, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: true, is_container: false, can_block_grass: true, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 20 },
    BlockDef { id: 78, kind: BlockKind::Snow, name: Some("tile.snow"), texture: 66, material: Material::Snow, hardness: 0.1, resistance: 0.5, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 0, collidable: true, sound: StepSound::Cloth, bounds: [0.0, 0.0, 0.0, 1.0, 0.125, 1.0], tick_on_load: true, is_container: false, can_block_grass: true, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 79, kind: BlockKind::Ice, name: Some("tile.ice"), texture: 67, material: Material::Ice, hardness: 0.5, resistance: 2.5, light_value: 0, light_opacity: 3, opaque: false, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Glass, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: true, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.98, tick_rate: 10 },
    BlockDef { id: 80, kind: BlockKind::SnowBlock, name: Some("tile.snow"), texture: 66, material: Material::BuiltSnow, hardness: 0.2, resistance: 1.0, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Cloth, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: true, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 81, kind: BlockKind::Cactus, name: Some("tile.cactus"), texture: 70, material: Material::Cactus, hardness: 0.4, resistance: 2.0, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 13, collidable: true, sound: StepSound::Cloth, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: true, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 82, kind: BlockKind::Clay, name: Some("tile.clay"), texture: 72, material: Material::Clay, hardness: 0.6, resistance: 3.0, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Gravel, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 83, kind: BlockKind::Reed, name: Some("tile.reeds"), texture: 73, material: Material::Plants, hardness: 0.0, resistance: 0.0, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 1, collidable: true, sound: StepSound::Grass, bounds: [0.125, 0.0, 0.125, 0.875, 1.0, 0.875], tick_on_load: true, is_container: false, can_block_grass: true, no_neighbor_notify_on_meta: false, enable_stats: false, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 84, kind: BlockKind::JukeBox, name: Some("tile.jukebox"), texture: 74, material: Material::Wood, hardness: 2.0, resistance: 3.0e1, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: true, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 85, kind: BlockKind::Fence, name: Some("tile.fence"), texture: 4, material: Material::Wood, hardness: 2.0, resistance: 15.0, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 11, collidable: true, sound: StepSound::Wood, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 86, kind: BlockKind::Pumpkin, name: Some("tile.pumpkin"), texture: 102, material: Material::Pumpkin, hardness: 1.0, resistance: 5.0, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Wood, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: true, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 87, kind: BlockKind::Netherrack, name: Some("tile.hellrock"), texture: 103, material: Material::Rock, hardness: 0.4, resistance: 2.0, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Stone, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 88, kind: BlockKind::SoulSand, name: Some("tile.hellsand"), texture: 104, material: Material::Sand, hardness: 0.5, resistance: 2.5, light_value: 0, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Sand, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 89, kind: BlockKind::GlowStone, name: Some("tile.lightgem"), texture: 105, material: Material::Rock, hardness: 0.3, resistance: 1.5, light_value: 15, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Glass, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 90, kind: BlockKind::Portal, name: Some("tile.portal"), texture: 14, material: Material::Portal, hardness: -1.0, resistance: 0.0, light_value: 11, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 0, collidable: true, sound: StepSound::Glass, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: true, no_neighbor_notify_on_meta: false, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 91, kind: BlockKind::Pumpkin, name: Some("tile.litpumpkin"), texture: 102, material: Material::Pumpkin, hardness: 1.0, resistance: 5.0, light_value: 15, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Wood, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: true, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 92, kind: BlockKind::Cake, name: Some("tile.cake"), texture: 121, material: Material::Cake, hardness: 0.5, resistance: 2.5, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 0, collidable: true, sound: StepSound::Cloth, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: true, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: false, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 93, kind: BlockKind::RedstoneRepeater, name: Some("tile.diode"), texture: 6, material: Material::Circuits, hardness: 0.0, resistance: 0.0, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 15, collidable: true, sound: StepSound::Wood, bounds: [0.0, 0.0, 0.0, 1.0, 0.125, 1.0], tick_on_load: false, is_container: false, can_block_grass: true, no_neighbor_notify_on_meta: true, enable_stats: false, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 94, kind: BlockKind::RedstoneRepeater, name: Some("tile.diode"), texture: 6, material: Material::Circuits, hardness: 0.0, resistance: 0.0, light_value: 9, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 15, collidable: true, sound: StepSound::Wood, bounds: [0.0, 0.0, 0.0, 1.0, 0.125, 1.0], tick_on_load: false, is_container: false, can_block_grass: true, no_neighbor_notify_on_meta: true, enable_stats: false, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 95, kind: BlockKind::LockedChest, name: Some("tile.lockedchest"), texture: 26, material: Material::Wood, hardness: 0.0, resistance: 0.0, light_value: 15, light_opacity: 255, opaque: true, render_as_normal: true, render_type: 0, collidable: true, sound: StepSound::Wood, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: true, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: true, slipperiness: 0.6, tick_rate: 10 },
    BlockDef { id: 96, kind: BlockKind::TrapDoor, name: Some("tile.trapdoor"), texture: 84, material: Material::Wood, hardness: 3.0, resistance: 15.0, light_value: 0, light_opacity: 0, opaque: false, render_as_normal: false, render_type: 0, collidable: true, sound: StepSound::Wood, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], tick_on_load: false, is_container: false, can_block_grass: false, no_neighbor_notify_on_meta: true, enable_stats: false, slipperiness: 0.6, tick_rate: 10 },
];

pub static BLOCK_DEFS: [BlockDef; 96] = DEFS;

/// Tabel datar untuk jalur panas (pencahayaan, meshing). Indeks = ID blok (0 = udara).
pub static LIGHT_OPACITY: [u8; 256] = build_light_opacity();
pub static LIGHT_VALUE: [u8; 256] = build_light_value();
pub static OPAQUE: [bool; 256] = build_opaque();

const fn build_light_opacity() -> [u8; 256] {
    let mut t = [0u8; 256];
    let mut i = 0;
    while i < DEFS.len() {
        t[DEFS[i].id as usize] = DEFS[i].light_opacity;
        i += 1;
    }
    t
}

const fn build_light_value() -> [u8; 256] {
    let mut t = [0u8; 256];
    let mut i = 0;
    while i < DEFS.len() {
        t[DEFS[i].id as usize] = DEFS[i].light_value;
        i += 1;
    }
    t
}

const fn build_opaque() -> [bool; 256] {
    let mut t = [false; 256];
    let mut i = 0;
    while i < DEFS.len() {
        t[DEFS[i].id as usize] = DEFS[i].opaque;
        i += 1;
    }
    t
}

/// Cari definisi blok. ID 0 (udara) dan ID > 96 mengembalikan None.
#[inline]
pub fn block(id: u8) -> Option<&'static BlockDef> {
    BLOCK_DEFS.get((id as usize).wrapping_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_berurutan() {
        for (i, d) in BLOCK_DEFS.iter().enumerate() {
            assert_eq!(d.id as usize, i + 1);
        }
        assert_eq!(BLOCK_DEFS.len(), 96);
    }

    #[test]
    fn nilai_dari_java() {
        let stone = block(1).unwrap();
        assert_eq!(stone.hardness, 1.5);
        assert_eq!(stone.resistance, 30.0);
        assert_eq!(block(7).unwrap().hardness, -1.0);
        assert_eq!(block(50).unwrap().light_value, 14); // obor
        assert_eq!(block(10).unwrap().light_value, 15); // lava
        assert_eq!(LIGHT_OPACITY[8], 3);               // air
        assert_eq!(LIGHT_OPACITY[1], 255);
        assert!(OPAQUE[1] && !OPAQUE[20]);             // kaca tidak opak
        assert!(block(0).is_none() && block(97).is_none());
    }
}
