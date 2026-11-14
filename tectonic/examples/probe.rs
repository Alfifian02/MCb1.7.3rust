// Diagnostic (`cargo run --release --example probe -- 2 3 4`): measures the artifacts the player reported on Cascades' raw density.
use tectonic::Tectonic;
use std::collections::HashMap;

struct World { t: Tectonic, h: usize, cache: HashMap<(i32,i32), Vec<f32>> }
impl World {
    fn new(seed: i64) -> Self { let t = Tectonic::cascades(seed); let h = t.height(); World { t, h, cache: HashMap::new() } }
    fn d(&mut self, x: i32, y: usize, z: i32) -> f32 {
        let (cx, cz) = (x.div_euclid(16), z.div_euclid(16));
        if !self.cache.contains_key(&(cx,cz)) { let c = self.t.chunk(cx, cz); self.cache.insert((cx,cz), c); }
        let c = &self.cache[&(cx,cz)];
        c[((x.rem_euclid(16) * 16 + z.rem_euclid(16)) as usize) * self.h + y]
    }
}

fn main() {
    let seeds: Vec<i64> = std::env::args().skip(1).map(|s| s.parse().unwrap()).collect();
    let seeds = if seeds.is_empty() { vec![1, 2, 3] } else { seeds };
    for seed in seeds {
        let mut w = World::new(seed);
        let h = w.h;
        let (n, ox, oz) = (20i32, -10i32, -10i32);
        let mut cols = 0u64; let mut land = 0u64;
        let mut hollow = 0u64; let mut hollow_big = 0u64; let mut cliff8 = 0u64; let mut cliff16 = 0u64;
        let mut seam = 0u64; let mut seams = 0u64;
        let mut tops: HashMap<(i32,i32), i32> = HashMap::new();
        let mut roofed_air_land = 0u64; // air blocks above sea with solid within 24 above them (overhang/tunnel) in land columns
        let mut y_hist = vec![0u64; 12];
        for cx in ox..ox+n { for cz in oz..oz+n {
            for lx in 0..16 { for lz in 0..16 {
                let (x, z) = (cx*16+lx, cz*16+lz);
                let mut top = -1i32;
                for y in (0..h).rev() { if w.d(x,y,z) > 0.0 { top = y as i32; break; } }
                tops.insert((x,z), top);
                cols += 1;
                if top >= 64 { land += 1; }
                if top >= 64 {
                    // first air below the top skin
                    let mut y = top - 1; let mut skin = 1;
                    while y >= 0 && w.d(x,y as usize,z) > 0.0 { skin += 1; y -= 1; }
                    if y >= 0 {
                        // air run
                        let mut run = 0; let mut yy = y;
                        while yy >= 0 && w.d(x,yy as usize,z) <= 0.0 { run += 1; yy -= 1; }
                        if yy >= 0 && skin <= 10 && run >= 3 && y >= 64 { hollow += 1; if run >= 8 { hollow_big += 1; } let b = ((top as usize).min(383))/32; y_hist[b.min(11)] += 1; }
                        let _ = roofed_air_land;
                    }
                }
            }}
        }}
        for (&(x,z), &t) in &tops {
            for (dx,dz) in [(1,0),(0,1)] {
                if let Some(&u) = tops.get(&(x+dx,z+dz)) { if t>=64 || u>=64 { let dd=(t-u).abs(); if dd>=8 {cliff8+=1;} if dd>=16 {cliff16+=1;} if x.rem_euclid(16)==15 && dx==1 || z.rem_euclid(16)==15 && dz==1 { seams+=1; if dd>=8 {seam+=1;} } } }
            }
        }
        println!("seed {seed}: cols {cols} land {land} ({:.0}%)  hollow cols {hollow} ({:.1}% of land) big {hollow_big}  cliff>=8 edges {cliff8} >=16 {cliff16}  seam edges>=8: {seam}/{seams}", 100.0*land as f64/cols as f64, 100.0*hollow as f64/land.max(1) as f64);
        println!("   hollow by top y/32: {:?}", y_hist);
    }
}
