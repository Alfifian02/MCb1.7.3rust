// Cave entrances: mouths in the surface (columns whose top is >=3 under the cave-less terrain) and the air they lead to.
use tectonic::{Tectonic, Config};
fn load(p: &str, seed: i64) -> Tectonic { Tectonic::from_bundle(&std::fs::read_to_string(p).unwrap(), seed, Config::cascades()).unwrap() }
fn main() {
    let a: Vec<String> = std::env::args().collect();
    // usage: probe5 <bundle.json> <same bundle with every cave function set to 1000000> <seed>
    let (tag, seed): (&str, i64) = (&a[1], a[3].parse().unwrap());
    let full = load(&a[1], seed);
    let base = load(&a[2], seed);
    let h = full.height();
    let (n, o) = (20i32, -10i32);
    let w = (n * 16) as usize;
    let id = |x: usize, z: usize, y: usize| (x * w + z) * h + y;
    let mut air = vec![false; w * w * h];
    let mut h0 = vec![-1i32; w * w];
    for cx in 0..n { for cz in 0..n {
        let d = full.chunk(o + cx, o + cz); let b = base.chunk(o + cx, o + cz);
        for lx in 0..16 { for lz in 0..16 {
            let (x, z) = ((cx as usize) * 16 + lx, (cz as usize) * 16 + lz);
            for y in 0..h { air[id(x, z, y)] = d[(lx*16+lz)*h+y] <= 0.0; }
            h0[x * w + z] = (0..h).rev().find(|&y| b[(lx*16+lz)*h+y] > 0.0).map_or(-1, |y| y as i32);
        }}
    }}
    let top = |x: usize, z: usize| (0..h).rev().find(|&y| !air[id(x,z,y)]).map_or(-1, |y| y as i32);
    let mut mouth = vec![false; w * w]; let mut land = 0; let mut mc = 0;
    for x in 0..w { for z in 0..w { let t0 = h0[x*w+z]; if t0 >= 66 { land += 1; if top(x,z) <= t0 - 3 { mouth[x*w+z] = true; mc += 1; } } } }
    // components of mouth columns (8-conn), then air flood below the carved surface from each component
    let mut seen = vec![false; w * w]; let mut comps = 0; let mut big = 0; let mut vols = vec![];
    let mut aseen = vec![false; air.len()];
    for x in 1..w-1 { for z in 1..w-1 { if !mouth[x*w+z] || seen[x*w+z] { continue; }
        let mut st = vec![(x,z)]; seen[x*w+z] = true; let mut cols = vec![];
        while let Some((a,b)) = st.pop() { cols.push((a,b)); for dx in -1i32..=1 { for dz in -1i32..=1 { let (p,q) = ((a as i32+dx) as usize, (b as i32+dz) as usize); if p<w && q<w && mouth[p*w+q] && !seen[p*w+q] { seen[p*w+q]=true; st.push((p,q)); } } } }
        if cols.len() < 4 { continue; }
        comps += 1;
        // air flood from the top of each mouth column, restricted to y < h0-1 (under the cave-less ground) and not escaping to the sky
        let mut vol = 0usize; let mut stack = vec![];
        for &(a,b) in &cols { let y = top(a,b); if y >= 0 { let y = y as usize + 1; if y < h && air[id(a,b,y)] && !aseen[id(a,b,y)] { aseen[id(a,b,y)] = true; stack.push((a,b,y)); } } }
        while let Some((a,b,y)) = stack.pop() { vol += 1; if vol > 20000 { break; }
            for (p,q,r) in [(a.wrapping_sub(1),b,y),(a+1,b,y),(a,b.wrapping_sub(1),y),(a,b+1,y),(a,b,y.wrapping_sub(1)),(a,b,y+1)] {
                if p<w && q<w && r<h && air[id(p,q,r)] && !aseen[id(p,q,r)] && (r as i32) < h0[p*w+q] - 1 { aseen[id(p,q,r)] = true; stack.push((p,q,r)); } } }
        vols.push(vol); if vol >= 1500 { big += 1; }
    }}
    println!("{tag} seed {seed}: land {land} mouth cols {mc} ({:.2}%) mouths {comps} leading to >=1500 air {big}", 100.0*mc as f64/land.max(1) as f64);
}
