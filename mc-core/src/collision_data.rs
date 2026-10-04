//! DIHASILKAN oleh tools/gen_collision.py dari registri b1.7.3 asli. Jangan edit manual.
//! Bentuk tabrakan statis per blok (relatif terhadap sudut blok) dan mask canCollideCheck.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CollisionShape {
    /// getCollisionBoundingBoxFromPool mengembalikan null
    None,
    /// Kotak statis: minX, minY, minZ, maxX, maxY, maxZ
    Box([f64; 6]),
    /// Bergantung pada World/metadata: harus disediakan lewat `CollisionBehaviors`
    Dynamic,
}

pub static COLLISION_SHAPES: [CollisionShape; 96] = [
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 1 BlockStone
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 2 BlockGrass
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 3 BlockDirt
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 4 Block
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 5 Block
    CollisionShape::None,  // 6 BlockSapling
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 7 Block
    CollisionShape::None,  // 8 BlockFlowing
    CollisionShape::None,  // 9 BlockStationary
    CollisionShape::None,  // 10 BlockFlowing
    CollisionShape::None,  // 11 BlockStationary
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 12 BlockSand
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 13 BlockGravel
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 14 BlockOre
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 15 BlockOre
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 16 BlockOre
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 17 BlockLog
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 18 BlockLeaves
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 19 BlockSponge
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 20 BlockGlass
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 21 BlockOre
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 22 Block
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 23 BlockDispenser
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 24 BlockSandStone
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 25 BlockNote
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 0.5625, 1.0]),  // 26 BlockBed
    CollisionShape::None,  // 27 BlockRail
    CollisionShape::None,  // 28 BlockDetectorRail
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 29 BlockPistonBase
    CollisionShape::None,  // 30 BlockWeb
    CollisionShape::None,  // 31 BlockTallGrass
    CollisionShape::None,  // 32 BlockDeadBush
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 33 BlockPistonBase
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 34 BlockPistonExtension
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 35 BlockCloth
    CollisionShape::Dynamic,  // 36 BlockPistonMoving
    CollisionShape::None,  // 37 BlockFlower
    CollisionShape::None,  // 38 BlockFlower
    CollisionShape::None,  // 39 BlockMushroom
    CollisionShape::None,  // 40 BlockMushroom
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 41 BlockOreStorage
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 42 BlockOreStorage
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 43 BlockStep
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 0.5, 1.0]),  // 44 BlockStep
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 45 Block
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 46 BlockTNT
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 47 BlockBookshelf
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 48 Block
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 49 BlockObsidian
    CollisionShape::None,  // 50 BlockTorch
    CollisionShape::None,  // 51 BlockFire
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 52 BlockMobSpawner
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 53 BlockStairs
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 54 BlockChest
    CollisionShape::None,  // 55 BlockRedstoneWire
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 56 BlockOre
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 57 BlockOreStorage
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 58 BlockWorkbench
    CollisionShape::None,  // 59 BlockCrops
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 60 BlockFarmland
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 61 BlockFurnace
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 62 BlockFurnace
    CollisionShape::None,  // 63 BlockSign
    CollisionShape::Dynamic,  // 64 BlockDoor
    CollisionShape::Dynamic,  // 65 BlockLadder
    CollisionShape::None,  // 66 BlockRail
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 67 BlockStairs
    CollisionShape::None,  // 68 BlockSign
    CollisionShape::None,  // 69 BlockLever
    CollisionShape::None,  // 70 BlockPressurePlate
    CollisionShape::Dynamic,  // 71 BlockDoor
    CollisionShape::None,  // 72 BlockPressurePlate
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 73 BlockRedstoneOre
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 74 BlockRedstoneOre
    CollisionShape::None,  // 75 BlockRedstoneTorch
    CollisionShape::None,  // 76 BlockRedstoneTorch
    CollisionShape::None,  // 77 BlockButton
    CollisionShape::Dynamic,  // 78 BlockSnow
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 79 BlockIce
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 80 BlockSnowBlock
    CollisionShape::Box([0.0625, 0.0, 0.0625, 0.9375, 0.9375, 0.9375]),  // 81 BlockCactus
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 82 BlockClay
    CollisionShape::None,  // 83 BlockReed
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 84 BlockJukeBox
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.5, 1.0]),  // 85 BlockFence
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 86 BlockPumpkin
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 87 BlockNetherrack
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 0.875, 1.0]),  // 88 BlockSoulSand
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 89 BlockGlowStone
    CollisionShape::None,  // 90 BlockPortal
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 91 BlockPumpkin
    CollisionShape::Dynamic,  // 92 BlockCake
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 0.125, 1.0]),  // 93 BlockRedstoneRepeater
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 0.125, 1.0]),  // 94 BlockRedstoneRepeater
    CollisionShape::Box([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),  // 95 BlockLockedChest
    CollisionShape::Dynamic,  // 96 BlockTrapDoor
];

/// Bit (meta * 2 + flag) = canCollideCheck(meta, flag). flag = "boleh berhenti di cairan".
pub static COLLIDE_CHECK: [u32; 96] = [
    0xffffffff,  // 1 BlockStone
    0xffffffff,  // 2 BlockGrass
    0xffffffff,  // 3 BlockDirt
    0xffffffff,  // 4 Block
    0xffffffff,  // 5 Block
    0xffffffff,  // 6 BlockSapling
    0xffffffff,  // 7 Block
    0x00000002,  // 8 BlockFlowing
    0x00000002,  // 9 BlockStationary
    0x00000002,  // 10 BlockFlowing
    0x00000002,  // 11 BlockStationary
    0xffffffff,  // 12 BlockSand
    0xffffffff,  // 13 BlockGravel
    0xffffffff,  // 14 BlockOre
    0xffffffff,  // 15 BlockOre
    0xffffffff,  // 16 BlockOre
    0xffffffff,  // 17 BlockLog
    0xffffffff,  // 18 BlockLeaves
    0xffffffff,  // 19 BlockSponge
    0xffffffff,  // 20 BlockGlass
    0xffffffff,  // 21 BlockOre
    0xffffffff,  // 22 Block
    0xffffffff,  // 23 BlockDispenser
    0xffffffff,  // 24 BlockSandStone
    0xffffffff,  // 25 BlockNote
    0xffffffff,  // 26 BlockBed
    0xffffffff,  // 27 BlockRail
    0xffffffff,  // 28 BlockDetectorRail
    0xffffffff,  // 29 BlockPistonBase
    0xffffffff,  // 30 BlockWeb
    0xffffffff,  // 31 BlockTallGrass
    0xffffffff,  // 32 BlockDeadBush
    0xffffffff,  // 33 BlockPistonBase
    0xffffffff,  // 34 BlockPistonExtension
    0xffffffff,  // 35 BlockCloth
    0xffffffff,  // 36 BlockPistonMoving
    0xffffffff,  // 37 BlockFlower
    0xffffffff,  // 38 BlockFlower
    0xffffffff,  // 39 BlockMushroom
    0xffffffff,  // 40 BlockMushroom
    0xffffffff,  // 41 BlockOreStorage
    0xffffffff,  // 42 BlockOreStorage
    0xffffffff,  // 43 BlockStep
    0xffffffff,  // 44 BlockStep
    0xffffffff,  // 45 Block
    0xffffffff,  // 46 BlockTNT
    0xffffffff,  // 47 BlockBookshelf
    0xffffffff,  // 48 Block
    0xffffffff,  // 49 BlockObsidian
    0xffffffff,  // 50 BlockTorch
    0x00000000,  // 51 BlockFire
    0xffffffff,  // 52 BlockMobSpawner
    0xffffffff,  // 53 BlockStairs
    0xffffffff,  // 54 BlockChest
    0xffffffff,  // 55 BlockRedstoneWire
    0xffffffff,  // 56 BlockOre
    0xffffffff,  // 57 BlockOreStorage
    0xffffffff,  // 58 BlockWorkbench
    0xffffffff,  // 59 BlockCrops
    0xffffffff,  // 60 BlockFarmland
    0xffffffff,  // 61 BlockFurnace
    0xffffffff,  // 62 BlockFurnace
    0xffffffff,  // 63 BlockSign
    0xffffffff,  // 64 BlockDoor
    0xffffffff,  // 65 BlockLadder
    0xffffffff,  // 66 BlockRail
    0xffffffff,  // 67 BlockStairs
    0xffffffff,  // 68 BlockSign
    0xffffffff,  // 69 BlockLever
    0xffffffff,  // 70 BlockPressurePlate
    0xffffffff,  // 71 BlockDoor
    0xffffffff,  // 72 BlockPressurePlate
    0xffffffff,  // 73 BlockRedstoneOre
    0xffffffff,  // 74 BlockRedstoneOre
    0xffffffff,  // 75 BlockRedstoneTorch
    0xffffffff,  // 76 BlockRedstoneTorch
    0xffffffff,  // 77 BlockButton
    0xffffffff,  // 78 BlockSnow
    0xffffffff,  // 79 BlockIce
    0xffffffff,  // 80 BlockSnowBlock
    0xffffffff,  // 81 BlockCactus
    0xffffffff,  // 82 BlockClay
    0xffffffff,  // 83 BlockReed
    0xffffffff,  // 84 BlockJukeBox
    0xffffffff,  // 85 BlockFence
    0xffffffff,  // 86 BlockPumpkin
    0xffffffff,  // 87 BlockNetherrack
    0xffffffff,  // 88 BlockSoulSand
    0xffffffff,  // 89 BlockGlowStone
    0xffffffff,  // 90 BlockPortal
    0xffffffff,  // 91 BlockPumpkin
    0xffffffff,  // 92 BlockCake
    0xffffffff,  // 93 BlockRedstoneRepeater
    0xffffffff,  // 94 BlockRedstoneRepeater
    0xffffffff,  // 95 BlockLockedChest
    0xffffffff,  // 96 BlockTrapDoor
];
