use bcore::{mesh, worldgen};
use std::time::Instant;

fn main() {
    let t = Instant::now();
    let mut chunks = Vec::new();
    for cx in -4..4 { for cz in -4..4 { chunks.push(worldgen::generate(1234, cx, cz)); } }
    println!("worldgen 64 chunks: {:?}", t.elapsed());

    let t = Instant::now();
    let (mut verts, mut sections) = (0usize, 0usize);
    for c in &chunks {
        for sy in 0..8 {
            let m = mesh::build_section(c, sy, &|_, _, _| 0);
            verts += m.vertices.len();
            sections += 1;
        }
    }
    println!("meshing {sections} sections: {:?}, {} vertices ({} KiB)", t.elapsed(), verts, verts * 8 / 1024);
}
