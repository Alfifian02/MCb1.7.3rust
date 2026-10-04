//! Port MapGenBase + MapGenCaves. Urutan konsumsi RNG dan aritmetika float/double dipertahankan persis.

use crate::jrandom::JRandom;
use crate::math;

const RANGE: i32 = 8;
const PI: f32 = std::f32::consts::PI;

const STONE: u8 = 1;
const GRASS: u8 = 2;
const DIRT: u8 = 3;
const WATER_MOVING: u8 = 8;
const WATER_STILL: u8 = 9;
const LAVA_MOVING: u8 = 10;

pub struct MapGenCaves {
    rand: JRandom,
}

impl Default for MapGenCaves {
    fn default() -> Self {
        Self::new()
    }
}

impl MapGenCaves {
    pub fn new() -> Self {
        Self { rand: JRandom::new(0) }
    }

    /// Java: MapGenBase.func_867_a. Mengukir gua dari chunk-chunk sekitar (radius 8) ke `blocks` chunk (cx, cz).
    pub fn generate(&mut self, world_seed: i64, cx: i32, cz: i32, blocks: &mut [u8]) {
        self.rand.set_seed(world_seed);
        let a = self.rand.next_long() / 2 * 2 + 1;
        let b = self.rand.next_long() / 2 * 2 + 1;
        for i in (cx - RANGE)..=(cx + RANGE) {
            for j in (cz - RANGE)..=(cz + RANGE) {
                self.rand.set_seed((i as i64).wrapping_mul(a).wrapping_add((j as i64).wrapping_mul(b)) ^ world_seed);
                self.cave_source(i, j, cx, cz, blocks);
            }
        }
    }

    /// Java: func_868_a. (sx, sz) = chunk asal gua; (cx, cz) = chunk yang sedang dibuat.
    fn cave_source(&mut self, sx: i32, sz: i32, cx: i32, cz: i32, blocks: &mut [u8]) {
        // Java: nextInt(nextInt(nextInt(40) + 1) + 1)
        let n1 = self.rand.next_int_bound(40) + 1;
        let n2 = self.rand.next_int_bound(n1) + 1;
        let mut count = self.rand.next_int_bound(n2);
        if self.rand.next_int_bound(15) != 0 {
            count = 0;
        }
        for _ in 0..count {
            let x = (sx * 16 + self.rand.next_int_bound(16)) as f64;
            let ya = self.rand.next_int_bound(120) + 8;
            let y = self.rand.next_int_bound(ya) as f64;
            let z = (sz * 16 + self.rand.next_int_bound(16)) as f64;
            let mut tunnels = 1;
            if self.rand.next_int_bound(4) == 0 {
                // ruang gua besar (func_870_a)
                let size = 1.0f32 + self.rand.next_float() * 6.0f32;
                self.carve(cx, cz, blocks, x, y, z, size, 0.0, 0.0, -1, -1, 0.5);
                tunnels += self.rand.next_int_bound(4);
            }
            for _ in 0..tunnels {
                let yaw = self.rand.next_float() * PI * 2.0f32;
                let pitch = (self.rand.next_float() - 0.5f32) * 2.0f32 / 8.0f32;
                let a = self.rand.next_float() * 2.0f32;
                let b = self.rand.next_float();
                let size = a + b;
                self.carve(cx, cz, blocks, x, y, z, size, yaw, pitch, 0, 0, 1.0);
            }
        }
    }

    /// Java: releaseEntitySkin (nama hasil dekompilasi; sebenarnya "carve tunnel").
    #[allow(clippy::too_many_arguments)]
    fn carve(
        &mut self, cx: i32, cz: i32, blocks: &mut [u8],
        mut x: f64, mut y: f64, mut z: f64,
        size: f32, mut yaw: f32, mut pitch: f32,
        mut step: i32, mut steps: i32, vscale: f64,
    ) {
        let cxc = (cx * 16 + 8) as f64;
        let czc = (cz * 16 + 8) as f64;
        let mut yaw_vel = 0.0f32;
        let mut pitch_vel = 0.0f32;
        let mut r = JRandom::new(self.rand.next_long());
        if steps <= 0 {
            let m = RANGE * 16 - 16;
            steps = m - r.next_int_bound(m / 4);
        }
        let mut room = false;
        if step == -1 {
            step = steps / 2;
            room = true;
        }
        let branch_at = r.next_int_bound(steps / 2) + steps / 4;
        let steep = r.next_int_bound(6) == 0;

        while step < steps {
            let radius = 1.5f64 + (math::sin(step as f32 * PI / steps as f32) * size * 1.0f32) as f64;
            let vradius = radius * vscale;
            let cp = math::cos(pitch);
            let sp = math::sin(pitch);
            x += (math::cos(yaw) * cp) as f64;
            y += sp as f64;
            z += (math::sin(yaw) * cp) as f64;
            if steep {
                pitch *= 0.92f32;
            } else {
                pitch *= 0.7f32;
            }
            pitch += pitch_vel * 0.1f32;
            yaw += yaw_vel * 0.1f32;
            pitch_vel *= 0.9f32;
            yaw_vel *= 12.0f32 / 16.0f32;
            {
                let a = r.next_float();
                let b = r.next_float();
                let c = r.next_float();
                pitch_vel += (a - b) * c * 2.0f32;
            }
            {
                let a = r.next_float();
                let b = r.next_float();
                let c = r.next_float();
                yaw_vel += (a - b) * c * 4.0f32;
            }
            if !room && step == branch_at && size > 1.0f32 {
                let s1 = r.next_float() * 0.5f32 + 0.5f32;
                self.carve(cx, cz, blocks, x, y, z, s1, yaw - PI * 0.5f32, pitch / 3.0f32, step, steps, 1.0);
                let s2 = r.next_float() * 0.5f32 + 0.5f32;
                self.carve(cx, cz, blocks, x, y, z, s2, yaw + PI * 0.5f32, pitch / 3.0f32, step, steps, 1.0);
                return;
            }
            if room || r.next_int_bound(4) != 0 {
                let dx = x - cxc;
                let dz = z - czc;
                let remain = (steps - step) as f64;
                let limit = (size + 2.0f32 + 16.0f32) as f64;
                if dx * dx + dz * dz - remain * remain > limit * limit {
                    return;
                }
                if x >= cxc - 16.0 - radius * 2.0
                    && z >= czc - 16.0 - radius * 2.0
                    && x <= cxc + 16.0 + radius * 2.0
                    && z <= czc + 16.0 + radius * 2.0
                {
                    let mut x_min = math::floor_double(x - radius) - cx * 16 - 1;
                    let mut x_max = math::floor_double(x + radius) - cx * 16 + 1;
                    let mut y_min = math::floor_double(y - vradius) - 1;
                    let mut y_max = math::floor_double(y + vradius) + 1;
                    let mut z_min = math::floor_double(z - radius) - cz * 16 - 1;
                    let mut z_max = math::floor_double(z + radius) - cz * 16 + 1;
                    if x_min < 0 { x_min = 0; }
                    if x_max > 16 { x_max = 16; }
                    if y_min < 1 { y_min = 1; }
                    if y_max > 120 { y_max = 120; }
                    if z_min < 0 { z_min = 0; }
                    if z_max > 16 { z_max = 16; }

                    // Lewati jika ada air di dalam kotak (loop for Java dengan variabel y yang dimodifikasi di badan loop)
                    let mut water = false;
                    let mut i = x_min;
                    while !water && i < x_max {
                        let mut j = z_min;
                        while !water && j < z_max {
                            let mut k = y_max + 1;
                            while !water && k >= y_min - 1 {
                                let idx = ((i * 16 + j) * 128 + k) as usize;
                                if k >= 0 && k < 128 {
                                    let b = blocks[idx];
                                    if b == WATER_MOVING || b == WATER_STILL {
                                        water = true;
                                    }
                                    if k != y_min - 1 && i != x_min && i != x_max - 1 && j != z_min && j != z_max - 1 {
                                        k = y_min;
                                    }
                                }
                                k -= 1;
                            }
                            j += 1;
                        }
                        i += 1;
                    }

                    if !water {
                        for i in x_min..x_max {
                            let xd = ((i + cx * 16) as f64 + 0.5 - x) / radius;
                            for j in z_min..z_max {
                                let zd = ((j + cz * 16) as f64 + 0.5 - z) / radius;
                                // Indeks mulai di y_max sementara y geometri mulai di y_max - 1 (off-by-one asli Java)
                                let mut idx = (i * 16 + j) * 128 + y_max;
                                let mut grass_seen = false;
                                if xd * xd + zd * zd < 1.0 {
                                    let mut yy = y_max - 1;
                                    while yy >= y_min {
                                        let yd = (yy as f64 + 0.5 - y) / vradius;
                                        if yd > -0.7 && xd * xd + yd * yd + zd * zd < 1.0 {
                                            let b = blocks[idx as usize];
                                            if b == GRASS {
                                                grass_seen = true;
                                            }
                                            if b == STONE || b == DIRT || b == GRASS {
                                                if yy < 10 {
                                                    blocks[idx as usize] = LAVA_MOVING;
                                                } else {
                                                    blocks[idx as usize] = 0;
                                                    if grass_seen && blocks[idx as usize - 1] == DIRT {
                                                        blocks[idx as usize - 1] = GRASS;
                                                    }
                                                }
                                            }
                                        }
                                        idx -= 1;
                                        yy -= 1;
                                    }
                                }
                            }
                        }
                        if room {
                            break;
                        }
                    }
                }
            }
            step += 1;
        }
    }
}
