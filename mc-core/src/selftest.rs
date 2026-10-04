//! Uji mandiri yang bisa dijalankan di perangkat (APK), tanpa `cargo test`.
//! Ini cermin ringkas dari tes unit utama; panic ditangkap sehingga satu kegagalan tidak menghentikan yang lain.

use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::aabb::{Aabb, HitResult};
use crate::blocks;
use crate::chunk::{Chunk, SkyBlock, VOLUME};
use crate::dimension::Dimension;
use crate::jrandom::JRandom;
use crate::nibble::NibbleArray;
use crate::noise::Octaves;
use crate::vec3::Vec3;
use crate::world::World;

pub struct TestResult {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
}

type Check = fn() -> Result<(), String>;

fn eq<T: PartialEq + std::fmt::Debug>(label: &str, got: T, want: T) -> Result<(), String> {
    if got == want {
        Ok(())
    } else {
        Err(format!("{label}: dapat {got:?}, harap {want:?}"))
    }
}

/// Dunia datar 3x3 chunk: batu (ID 1) untuk y < 60.
fn flat_world() -> World {
    let mut w = World::new(false, 1);
    for cx in -1..=1 {
        for cz in -1..=1 {
            let mut b = vec![0u8; VOLUME];
            for x in 0..16usize {
                for z in 0..16usize {
                    for y in 0..60usize {
                        b[(x << 11) | (z << 7) | y] = 1;
                    }
                }
            }
            w.chunks.add_generated_chunk(Chunk::new(cx, cz, b));
        }
    }
    w.chunks.flush_lighting();
    w
}

fn t_random() -> Result<(), String> {
    let mut r = JRandom::new(42);
    eq("nextInt seed 42", r.next_int(), -1170105035)
}

fn t_nibble() -> Result<(), String> {
    let mut n = NibbleArray::new(VOLUME);
    n.set(3, 100, 7, 11);
    n.set(3, 101, 7, 5);
    eq("nibble genap", n.get(3, 100, 7), 11)?;
    eq("nibble ganjil", n.get(3, 101, 7), 5)
}

fn t_blocks() -> Result<(), String> {
    eq("jumlah blok", blocks::BLOCK_DEFS.len(), 96)?;
    eq("hardness batu", blocks::block(1).ok_or("batu hilang")?.hardness, 1.5f32)?;
    eq("hardness bedrock", blocks::block(7).ok_or("bedrock hilang")?.hardness, -1.0f32)?;
    eq("cahaya obor", blocks::block(50).ok_or("obor hilang")?.light_value, 14)
}

fn t_noise() -> Result<(), String> {
    let o = Octaves::new(&mut JRandom::new(42), 4);
    let (mut a, mut b) = (Vec::new(), Vec::new());
    o.generate(&mut a, 0.0, 0.0, 0.0, 5, 17, 5, 684.412, 684.412, 684.412);
    o.generate(&mut b, 0.0, 0.0, 0.0, 5, 17, 5, 684.412, 684.412, 684.412);
    eq("panjang", a.len(), 425)?;
    if a != b {
        return Err("noise tidak deterministik".into());
    }
    if a.iter().any(|v| !v.is_finite()) {
        return Err("noise berisi NaN/inf".into());
    }
    if a.iter().all(|v| *v == 0.0) {
        return Err("noise nol semua".into());
    }
    Ok(())
}

fn t_aabb() -> Result<(), String> {
    let blok = Aabb::new(1.0, 0.0, 0.0, 2.0, 1.0, 1.0);
    let pemain = Aabb::new(0.0, 0.0, 0.0, 0.6, 1.8, 0.6);
    let d = blok.calculate_x_offset(&pemain, 1.0);
    if (d - 0.4).abs() > 1e-9 {
        return Err(format!("x_offset {d}"));
    }
    let a = Vec3::new(1.0, 2.0, 3.0);
    eq("subtract terbalik", a.subtract(Vec3::new(4.0, 6.0, 8.0)), Vec3::new(3.0, 4.0, 5.0))
}

fn t_skylight() -> Result<(), String> {
    let w = flat_world();
    eq("tinggi kolom", w.chunks.height_value(8, 8), 60)?;
    eq("sky di atas", w.chunks.saved_light_value(SkyBlock::Sky, 8, 100, 8), 15)?;
    eq("sky di bawah", w.chunks.saved_light_value(SkyBlock::Sky, 8, 59, 8), 0)
}

fn t_torch() -> Result<(), String> {
    let mut w = flat_world();
    w.set_block_and_metadata(8, 61, 8, 50, 0);
    w.chunks.flush_lighting();
    eq("obor", w.chunks.saved_light_value(SkyBlock::Block, 8, 61, 8), 14)?;
    eq("1 blok", w.chunks.saved_light_value(SkyBlock::Block, 9, 61, 8), 13)?;
    eq("3 blok", w.chunks.saved_light_value(SkyBlock::Block, 8, 61, 11), 11)?;
    w.set_block_and_metadata(8, 61, 8, 0, 0);
    w.chunks.flush_lighting();
    eq("padam", w.chunks.saved_light_value(SkyBlock::Block, 9, 61, 8), 0)
}

fn t_roof() -> Result<(), String> {
    let mut w = flat_world();
    w.set_block_and_metadata(8, 70, 8, 1, 0);
    w.chunks.flush_lighting();
    eq("tinggi atap", w.chunks.height_value(8, 8), 71)?;
    eq("sky bawah atap", w.chunks.saved_light_value(SkyBlock::Sky, 8, 69, 8), 14)?;
    w.set_block_and_metadata(8, 70, 8, 0, 0);
    w.chunks.flush_lighting();
    eq("tinggi pulih", w.chunks.height_value(8, 8), 60)
}

fn t_raycast() -> Result<(), String> {
    let w = flat_world();
    match w.ray_trace_blocks(Vec3::new(8.5, 70.5, 8.5), Vec3::new(8.5, 50.5, 8.5)) {
        Some(HitResult::Tile { x, y, z, side, .. }) => eq("kena", (x, y, z, side), (8, 59, 8, 1)),
        other => Err(format!("hasil tak terduga: {other:?}")),
    }
}

fn t_collision() -> Result<(), String> {
    let mut w = flat_world();
    w.set_block(8, 60, 8, 85); // pagar
    let area = Aabb::new(8.2, 60.0, 8.2, 8.8, 61.8, 8.8);
    let mut out = Vec::new();
    w.colliding_block_boxes(&area, &mut out);
    eq("jumlah kotak", out.len(), 1)?;
    eq("tinggi pagar", out[0].max_y, 61.5)
}

fn mark_tick(w: &mut World, x: i32, y: i32, z: i32) {
    w.set_block_metadata(x, y, z, 9);
}

fn t_tick() -> Result<(), String> {
    let mut w = flat_world();
    w.behaviors.update_tick[12] = Some(mark_tick);
    w.set_block(0, 70, 0, 12);
    w.schedule_block_update(0, 70, 0, 12, 5);
    w.schedule_block_update(0, 70, 0, 12, 5);
    eq("dedup", w.pending_ticks(), 1)?;
    w.world_time = 4;
    w.tick_updates(false);
    eq("belum jatuh tempo", w.block_metadata(0, 70, 0), 0)?;
    w.world_time = 5;
    w.tick_updates(false);
    eq("sudah jatuh tempo", w.block_metadata(0, 70, 0), 9)
}

fn t_langit() -> Result<(), String> {
    eq("Nether", Dimension::Hell.celestial_angle(777, 0.2), 0.5f32)?;
    eq("siang", World::calculate_skylight_subtracted(0.0, 0.0, 0.0), 0)?;
    eq("tengah malam", World::calculate_skylight_subtracted(0.5, 0.0, 0.0), 11)
}

pub fn run_all() -> Vec<TestResult> {
    let checks: Vec<(&'static str, Check)> = vec![
        ("java.util.Random", t_random as Check),
        ("NibbleArray", t_nibble as Check),
        ("tabel 96 blok", t_blocks as Check),
        ("noise Perlin", t_noise as Check),
        ("AABB dan Vec3", t_aabb as Check),
        ("skylight dasar", t_skylight as Check),
        ("cahaya obor", t_torch as Check),
        ("atap dan skylight", t_roof as Check),
        ("raycast blok", t_raycast as Check),
        ("tabrakan pagar", t_collision as Check),
        ("tick terjadwal", t_tick as Check),
        ("sudut langit", t_langit as Check),
    ];
    checks
        .into_iter()
        .map(|(name, f)| match catch_unwind(AssertUnwindSafe(|| f())) {
            Ok(Ok(())) => TestResult { name, ok: true, detail: String::new() },
            Ok(Err(e)) => TestResult { name, ok: false, detail: e },
            Err(_) => TestResult { name, ok: false, detail: "panic (indeks/overflow?)".to_string() },
        })
        .collect()
}
