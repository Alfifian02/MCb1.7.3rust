//! MapGenBase + MapGenCaves (the last step of `ChunkProviderGenerate.provideChunk`).
//!
//! Caves only read the world seed and write inside the one chunk being generated: every chunk in
//! a 17x17 square around it gets a chance to start tunnels, and the part that crosses into this
//! chunk is carved. b1.7.3 has no ravines.
//!
//! Float maths follows the Java (`f32` with the `MathHelper` sin/cos table, `f64` where the Java
//! widens), and the quirks are kept: the carve loop reads the block one cell above the cell its
//! `y` refers to (`idx` starts at `y_hi` while `iy` starts at `y_hi - 1`).

use crate::world::gen::noise::{ifloor, mh_cos, mh_sin, JavaRandom};
use crate::world::gen::overworld::block;
use std::f32::consts::PI;

/// MapGenBase.field_1306_a: how many chunks away a tunnel may start.
const RANGE: i32 = 8;

/// Carve caves into chunk (cx, cz); `blocks` is its 16x128x16 array.
pub fn carve(seed: i64, cx: i32, cz: i32, blocks: &mut [u8]) {
    let mut rand = JavaRandom::new(seed);
    let a = rand.next_long() / 2 * 2 + 1;
    let b = rand.next_long() / 2 * 2 + 1;
    for x in cx - RANGE..=cx + RANGE {
        for z in cz - RANGE..=cz + RANGE {
            rand.set_seed((x as i64).wrapping_mul(a).wrapping_add((z as i64).wrapping_mul(b)) ^ seed);
            starts(&mut rand, x, z, cx, cz, blocks);
        }
    }
}

/// MapGenCaves.func_868_a: the tunnels that start in chunk (sx, sz).
fn starts(r: &mut JavaRandom, sx: i32, sz: i32, cx: i32, cz: i32, blocks: &mut [u8]) {
    // nextInt(nextInt(nextInt(40) + 1) + 1), innermost first.
    let a = r.next_int_bound(40) + 1;
    let b = r.next_int_bound(a) + 1;
    let mut count = r.next_int_bound(b);
    if r.next_int_bound(15) != 0 {
        count = 0;
    }
    for _ in 0..count {
        let x = (sx * 16 + r.next_int_bound(16)) as f64;
        let h = r.next_int_bound(120) + 8;
        let y = r.next_int_bound(h) as f64;
        let z = (sz * 16 + r.next_int_bound(16)) as f64;
        let mut n = 1;
        if r.next_int_bound(4) == 0 {
            // A big round room (func_870_a), then 0-3 ordinary tunnels from the same point.
            let radius = 1.0 + r.next_float() * 6.0;
            tunnel(r, cx, cz, blocks, x, y, z, radius, 0.0, 0.0, -1, -1, 0.5);
            n += r.next_int_bound(4);
        }
        for _ in 0..n {
            let yaw = r.next_float() * PI * 2.0;
            let pitch = (r.next_float() - 0.5) * 2.0 / 8.0;
            let radius = r.next_float() * 2.0 + r.next_float();
            tunnel(r, cx, cz, blocks, x, y, z, radius, yaw, pitch, 0, 0, 1.0);
        }
    }
}

/// MapGenCaves.releaseEntitySkin: walk one tunnel (or room) and carve the part inside chunk (cx, cz).
/// `step`/`steps` of 0 and -1 mean "pick them here", as in the Java. `main` is only used to seed
/// this tunnel's own Random (and the branches').
#[allow(clippy::too_many_arguments)]
fn tunnel(main: &mut JavaRandom, cx: i32, cz: i32, blocks: &mut [u8], mut x: f64, mut y: f64, mut z: f64,
          radius: f32, mut yaw: f32, mut pitch: f32, mut step: i32, mut steps: i32, vscale: f64) {
    let (mid_x, mid_z) = ((cx * 16 + 8) as f64, (cz * 16 + 8) as f64);
    let (mut yaw_rate, mut pitch_rate) = (0.0_f32, 0.0_f32);
    let mut rnd = JavaRandom::new(main.next_long());
    if steps <= 0 {
        let m = RANGE * 16 - 16;
        steps = m - rnd.next_int_bound(m / 4);
    }
    let mut room = false;
    if step == -1 {
        step = steps / 2;
        room = true;
    }
    let split_at = rnd.next_int_bound(steps / 2) + steps / 4;
    let steep = rnd.next_int_bound(6) == 0;
    while step < steps {
        let r1 = 1.5 + (mh_sin(step as f32 * PI / steps as f32) * radius * 1.0) as f64;
        let r2 = r1 * vscale;
        let cp = mh_cos(pitch);
        let sp = mh_sin(pitch);
        x += (mh_cos(yaw) * cp) as f64;
        y += sp as f64;
        z += (mh_sin(yaw) * cp) as f64;
        pitch *= if steep { 0.92 } else { 0.7 };
        pitch += pitch_rate * 0.1;
        yaw += yaw_rate * 0.1;
        pitch_rate *= 0.9;
        yaw_rate *= 12.0 / 16.0;
        pitch_rate += (rnd.next_float() - rnd.next_float()) * rnd.next_float() * 2.0;
        yaw_rate += (rnd.next_float() - rnd.next_float()) * rnd.next_float() * 4.0;
        if !room && step == split_at && radius > 1.0 {
            let r_a = rnd.next_float() * 0.5 + 0.5;
            tunnel(main, cx, cz, blocks, x, y, z, r_a, yaw - PI * 0.5, pitch / 3.0, step, steps, 1.0);
            let r_b = rnd.next_float() * 0.5 + 0.5;
            tunnel(main, cx, cz, blocks, x, y, z, r_b, yaw + PI * 0.5, pitch / 3.0, step, steps, 1.0);
            return;
        }
        if room || rnd.next_int_bound(4) != 0 {
            let (dx, dz) = (x - mid_x, z - mid_z);
            let left = (steps - step) as f64;
            let reach = (radius + 2.0 + 16.0) as f64;
            if dx * dx + dz * dz - left * left > reach * reach {
                return;
            }
            let lim = 16.0 + r1 * 2.0;
            if x >= mid_x - lim && z >= mid_z - lim && x <= mid_x + lim && z <= mid_z + lim {
                let x_lo = (ifloor(x - r1) - cx * 16 - 1).max(0);
                let x_hi = (ifloor(x + r1) - cx * 16 + 1).min(16);
                let y_lo = (ifloor(y - r2) - 1).max(1);
                let y_hi = (ifloor(y + r2) + 1).min(120);
                let z_lo = (ifloor(z - r1) - cz * 16 - 1).max(0);
                let z_hi = (ifloor(z + r1) - cz * 16 + 1).min(16);
                if !touches_water(blocks, x_lo, x_hi, y_lo, y_hi, z_lo, z_hi) {
                    carve_ellipsoid(blocks, cx, cz, (x, y, z), (r1, r2), (x_lo, x_hi, y_lo, y_hi, z_lo, z_hi));
                    if room {
                        break;
                    }
                }
            }
        }
        step += 1;
    }
}

/// The Java's "is there water in or on the shell of the box" scan, jump and all.
fn touches_water(blocks: &[u8], x_lo: i32, x_hi: i32, y_lo: i32, y_hi: i32, z_lo: i32, z_hi: i32) -> bool {
    let mut ix = x_lo;
    while ix < x_hi {
        let mut iz = z_lo;
        while iz < z_hi {
            let mut iy = y_hi + 1;
            while iy >= y_lo - 1 {
                if (0..128).contains(&iy) {
                    let b = blocks[((ix * 16 + iz) * 128 + iy) as usize];
                    if b == block::WATER_MOVING || b == block::WATER {
                        return true;
                    }
                    // Only the shell matters: from the inside jump straight to the floor.
                    if iy != y_lo - 1 && ix != x_lo && ix != x_hi - 1 && iz != z_lo && iz != z_hi - 1 {
                        iy = y_lo;
                    }
                }
                iy -= 1;
            }
            iz += 1;
        }
        ix += 1;
    }
    false
}

fn carve_ellipsoid(blocks: &mut [u8], cx: i32, cz: i32, (x, y, z): (f64, f64, f64), (r1, r2): (f64, f64),
                   (x_lo, x_hi, y_lo, y_hi, z_lo, z_hi): (i32, i32, i32, i32, i32, i32)) {
    for ix in x_lo..x_hi {
        let dx = ((ix + cx * 16) as f64 + 0.5 - x) / r1;
        for iz in z_lo..z_hi {
            let dz = ((iz + cz * 16) as f64 + 0.5 - z) / r1;
            if dx * dx + dz * dz >= 1.0 {
                continue;
            }
            // Java quirk kept: `i` starts one cell above `iy` (see the module docs).
            let mut i = ((ix * 16 + iz) * 128 + y_hi) as usize;
            let mut grass = false;
            for iy in (y_lo..y_hi).rev() {
                let dy = (iy as f64 + 0.5 - y) / r2;
                if dy > -0.7 && dx * dx + dy * dy + dz * dz < 1.0 {
                    let b = blocks[i];
                    if b == block::GRASS {
                        grass = true;
                    }
                    if b == block::STONE || b == block::DIRT || b == block::GRASS {
                        if iy < 10 {
                            blocks[i] = block::LAVA_MOVING;
                        } else {
                            blocks[i] = block::AIR;
                            if grass && blocks[i - 1] == block::DIRT {
                                blocks[i - 1] = block::GRASS;
                            }
                        }
                    }
                }
                i -= 1;
            }
        }
    }
}
