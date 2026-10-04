//! mc-core: logika dunia b1.7.3, tanpa dependensi platform (tanpa GL/Android).
//! Semua yang memengaruhi hasil seed harus bit-exact dengan Java.
pub mod aabb;
pub mod blocks;
pub mod chunk;
pub mod chunk_map;
pub mod collision;
pub mod collision_data;
pub mod dimension;
pub mod jrandom;
pub mod material;
pub mod math;
pub mod nibble;
pub mod noise;
pub mod provider;
pub mod selftest;
pub mod vec3;
pub mod world;
pub mod worldgen;
