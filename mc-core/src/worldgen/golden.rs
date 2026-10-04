//! DIHASILKAN oleh tools/gen_golden.py dari ChunkProviderGenerate asli b1.7.3 (tools/GenDump.java).
//! Hash FNV-1a 64 atas byte blok, heightmap, data skylight, dan string biome (huruf pertama + panjang nama).

/// (seed, cx, cz, suhu, kelembapan, terrain mentah, setelah permukaan, blok akhir, heightmap, skylight, biome)
pub type GoldenRow = (i64, i32, i32, u64, u64, u64, u64, u64, u64, u64, u64);

pub static GOLDEN: [GoldenRow; 15] = [
    (12345, 0, 0, 0x465d6952656e8df6, 0x28c31cf8df2ec325, 0x89fd438a9df0c135, 0xed5f3c87f3c8df60, 0xb4d54cd144e6093c, 0x29d74da237439923, 0xa7caee763e31fa6d, 0x3ba0c61bb6a8ff35),
    (12345, -1, -1, 0xbe5af4834f910bc2, 0x28c31cf8df2ec325, 0x9fcde18550730e5a, 0xe8d473b11d5caa3a, 0xa629e23420a61713, 0xcf9f75ed0b8a671a, 0xe8e5d236fbe521ab, 0x1f88c53993098f25),
    (12345, 5, 7, 0x6290a692e36eb591, 0x28c31cf8df2ec325, 0xd8d8f5d39c18c64a, 0x5d8de658d6d7488e, 0x7475e2fb75ba2d2c, 0xc86570f86485dcd6, 0x660b8602d09dbfe7, 0x1f88c53993098f25),
    (12345, -9, 3, 0x1401eebd8a743278, 0x43cf36e3ee59651c, 0x9cc657cf01faf54f, 0x1fc2dddca6685dd3, 0xf5ce7faf6a2d02b9, 0x6d681426f593f5a1, 0x401a85747334e1ae, 0x681bbaa3b82fd9ed),
    (12345, 20, -14, 0x1316c6746166316d, 0x10caa3b5f5ed80fb, 0xb67c8914ef5c6d57, 0x6f7e7ae1a672e7f1, 0x2c77197998163cda, 0x0dae0c21602ec1cf, 0xddcd6b646bbe2a40, 0xc4be694e7f71d384),
    (-987654321, 0, 0, 0x96bdc0d940914ed2, 0x64dfa2fcaf0363af, 0x9ed44992794d6935, 0x96fa4dee635d09cf, 0x96fa4dee635d09cf, 0x2794e7a27f95b725, 0xb7abc475e73390ad, 0xc2308f0046d2e09b),
    (-987654321, -1, -1, 0xdf6c7c3d46523393, 0x4e714a7747281538, 0x31852100e937350d, 0xae43576668552ad7, 0xae43576668552ad7, 0x2794e7a27f95b725, 0xd6e3844f51671f25, 0xd2d7f4f9c8d32253),
    (-987654321, 5, 7, 0x02f6215557a0e228, 0xb55f5f196cd3eb87, 0x265cad8fda7ef1e3, 0x9175e252190e7cd3, 0x3af43984dbd51dd5, 0x32b3fd7e6853cc03, 0x2d054c31f3d0d20a, 0x05977bf676060c31),
    (-987654321, -9, 3, 0x912f4ba72b5eaa0a, 0x4c005f7c019b92ad, 0x667ad39344a8bf60, 0x2c232a3a44d48cde, 0x38bf268e6194f494, 0xb9880b30431e0c09, 0x0a4108b9b468563f, 0x21a385ce20335925),
    (-987654321, 20, -14, 0xe75cc2c76175a0f2, 0x49626065204c1e23, 0xf9b803733b4b5f1c, 0xbf4bf3ca5caa9419, 0xac4f8720b1ef61ee, 0x5bb2dcd4425cdcdc, 0xee57defaa3b760fa, 0x5b7e0833f58f5bc9),
    (0, 0, 0, 0x687bc952f833bec5, 0x2e8af22e240db30d, 0x9de98354feb952a0, 0x60117da3930452a2, 0x60117da3930452a2, 0x93c6e6292e81d1ba, 0x9731d8678f9d4eea, 0x4a0b802ab1bec325),
    (0, -1, -1, 0x37a893b935d5e9ee, 0x0c9a379390cd21fd, 0xcb448789d06b8e45, 0xd1a2482b27791bdd, 0xd1a2482b27791bdd, 0x487d4aa760621c8d, 0x7dd6732c7658d9f3, 0x4a0b802ab1bec325),
    (0, 5, 7, 0x5ab513370ef5e135, 0x5b64c538541bf211, 0xb9ee2a5aecc6c6d6, 0xba04905e06fb6624, 0xbadd51eaaa02ba35, 0x733840240787638c, 0x8c73595832e21acf, 0x4a0b802ab1bec325),
    (0, -9, 3, 0xaec6b0e82ca1fe55, 0x56b1a1f08c913783, 0x3ab548f3180130f5, 0x70e13d5aa092705c, 0xa1804f64f4976b4d, 0x2794e7a27f95b725, 0x5de1b7a0f8fbbbe6, 0x3de5d88ec5bd2f03),
    (0, 20, -14, 0x727353359ad6d962, 0x8c1199e02767e1d9, 0x0ac23202b87ef6f0, 0xfd05e527cf0047d8, 0xfd05e527cf0047d8, 0x421a702dae70810c, 0x928d991272193308, 0x4a0b802ab1bec325),
];

/// FNV-1a 64-bit, sama dengan GenDump.java.
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// FNV-1a 64-bit atas bit double (big-endian), sama dengan hexD di GenDump.java.
pub fn fnv1a64_f64(data: &[f64]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for v in data {
        for b in v.to_bits().to_be_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}
