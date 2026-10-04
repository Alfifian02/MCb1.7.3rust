//! Generator dunia Overworld (Fase 5a): biome, terrain, permukaan, gua.
//! `populate` (pohon, bijih, danau, dll.) menyusul di 5b.
pub mod biome;
pub mod caves;
pub mod chunk_manager;
pub mod golden;
pub mod simplex;
pub mod terrain;

use crate::chunk::{ChunkEffect, HeightCtx};
use golden::{fnv1a64, fnv1a64_f64, GoldenRow, GOLDEN};
use terrain::{OverworldGenerator, Stage};

/// Bandingkan satu chunk hasil generator dengan data golden dari Java asli, tahap demi tahap.
/// Pesan kesalahan menyebut tahap pertama yang berbeda (suhu -> kelembapan -> terrain -> permukaan -> akhir).
pub fn check_golden_row(gen: &mut OverworldGenerator, row: &GoldenRow) -> Result<(), String> {
    let &(seed, cx, cz, h_temp, h_hum, h_ter, h_sur, hb, hh, hs, hbio) = row;
    let mut seen: Vec<(&'static str, u64, u64)> = Vec::new();
    let mut chunk = gen.generate_with(cx, cz, &mut |st| match st {
        Stage::Climate(t, h) => {
            seen.push(("suhu", fnv1a64_f64(t), h_temp));
            seen.push(("kelembapan", fnv1a64_f64(h), h_hum));
        }
        Stage::Terrain(b) => seen.push(("terrain", fnv1a64(b), h_ter)),
        Stage::Surface(b) => seen.push(("permukaan", fnv1a64(b), h_sur)),
    });
    let mut fx: Vec<ChunkEffect> = Vec::new();
    let outside = |_x: i32, _z: i32| 0;
    let ctx = HeightCtx { self_loaded: false, outside: &outside };
    chunk.init_skylight(false, &ctx, &mut fx);

    let mut names = String::new();
    for b in gen.manager.load_block_generator_data(cx * 16, cz * 16, 16, 16) {
        let n = b.props().name;
        names.push(n.chars().next().unwrap());
        names.push_str(&n.len().to_string());
        names.push(',');
    }
    seen.push(("blok+gua", fnv1a64(&chunk.blocks), hb));
    seen.push(("heightmap", fnv1a64(&chunk.height_map), hh));
    seen.push(("skylight", fnv1a64(&chunk.skylight.data), hs));
    seen.push(("biome", fnv1a64(names.as_bytes()), hbio));
    for (name, got, want) in seen {
        if got != want {
            return Err(format!("seed {seed} ({cx},{cz}): tahap '{name}' beda"));
        }
    }
    Ok(())
}

/// Jalankan semua baris golden untuk satu seed.
pub fn check_golden_seed(seed: i64) -> Result<(), String> {
    let mut gen = OverworldGenerator::new(seed);
    for row in GOLDEN.iter().filter(|r| r.0 == seed) {
        check_golden_row(&mut gen, row)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_seed_12345() {
        check_golden_seed(12345).unwrap();
    }

    #[test]
    fn golden_seed_negatif() {
        check_golden_seed(-987654321).unwrap();
    }

    #[test]
    fn golden_seed_nol() {
        check_golden_seed(0).unwrap();
    }

    #[test]
    fn generator_deterministik_dan_urutan_bebas() {
        let mut a = OverworldGenerator::new(777);
        let mut b = OverworldGenerator::new(777);
        let c1 = a.generate(3, -4);
        let _ = b.generate(0, 0);
        let c2 = b.generate(3, -4);
        assert_eq!(c1.blocks, c2.blocks);
    }
}
