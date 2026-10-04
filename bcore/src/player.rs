//! Player physics: AABB vs voxel collision, gravity, jump, swim, fly. No allocations.
pub const WIDTH: f32 = 0.6;
pub const HEIGHT: f32 = 1.8;
pub const EYE: f32 = 1.62;
const EPS: f32 = 1e-4;

pub type Probe<'a> = &'a dyn Fn(i32, i32, i32) -> bool;

#[derive(Clone, Copy, Default, Debug)]
pub struct MoveInput {
    pub forward: f32,
    pub strafe: f32,
    pub jump: bool,
    pub sneak: bool,
    pub sprint: bool,
}

#[derive(Clone, Debug)]
pub struct Player {
    pub pos: [f32; 3], // feet centre
    pub vel: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub flying: bool,
    pub in_water: bool,
}

impl Player {
    pub fn new(pos: [f32; 3]) -> Self {
        Self { pos, vel: [0.0; 3], yaw: 0.0, pitch: -0.3, on_ground: false, flying: false, in_water: false }
    }

    pub fn eye(&self) -> [f32; 3] {
        [self.pos[0], self.pos[1] + EYE, self.pos[2]]
    }

    pub fn aabb(&self) -> ([f32; 3], [f32; 3]) {
        let h = WIDTH * 0.5;
        ([self.pos[0] - h, self.pos[1], self.pos[2] - h], [self.pos[0] + h, self.pos[1] + HEIGHT, self.pos[2] + h])
    }

    pub fn update(&mut self, inp: &MoveInput, dt: f32, solid: Probe, water: Probe) {
        let n = ((dt * 60.0).ceil() as i32).clamp(1, 8);
        let h = dt / n as f32;
        for _ in 0..n {
            self.step(inp, h, solid, water);
        }
        if self.pos[1] < -30.0 {
            self.pos[1] = 120.0;
            self.vel = [0.0; 3];
        }
    }

    fn step(&mut self, inp: &MoveInput, dt: f32, solid: Probe, water: Probe) {
        let (sy, cy) = self.yaw.sin_cos();
        let mut wx = sy * inp.forward + cy * inp.strafe;
        let mut wz = -cy * inp.forward + sy * inp.strafe;
        let len = (wx * wx + wz * wz).sqrt();
        if len > 1.0 {
            wx /= len;
            wz /= len;
        }
        self.in_water = water(self.pos[0].floor() as i32, (self.pos[1] + 0.2).floor() as i32, self.pos[2].floor() as i32);

        let mut speed = if self.flying {
            if inp.sprint { 16.0 } else { 11.0 }
        } else if inp.sneak {
            1.3
        } else if inp.sprint {
            5.6
        } else {
            4.3
        };
        if self.in_water && !self.flying {
            speed *= 0.55;
        }
        self.vel[0] = wx * speed;
        self.vel[2] = wz * speed;

        if self.flying {
            self.vel[1] = (inp.jump as i32 - inp.sneak as i32) as f32 * 8.0;
        } else if self.in_water {
            self.vel[1] = (self.vel[1] - 6.0 * dt).max(-3.0);
            if inp.jump {
                self.vel[1] = 3.5;
            }
        } else if inp.jump && self.on_ground {
            self.vel[1] = 9.0;
        } else {
            self.vel[1] = (self.vel[1] - 32.0 * dt).max(-50.0);
        }

        self.on_ground = false;
        let dy = self.vel[1] * dt;
        if self.move_axis(1, dy, solid) {
            if dy < 0.0 {
                self.on_ground = true;
            }
            self.vel[1] = 0.0;
        }
        self.move_axis(0, self.vel[0] * dt, solid);
        self.move_axis(2, self.vel[2] * dt, solid);
    }

    /// Moves along one axis, stopping at solid blocks. Returns true if blocked.
    fn move_axis(&mut self, a: usize, d: f32, solid: Probe) -> bool {
        if d == 0.0 {
            return false;
        }
        let (min, max) = self.aabb();
        let mut lo = [0.0f32; 3];
        let mut hi = [0.0f32; 3];
        for i in 0..3 {
            if i == a {
                lo[i] = min[i].min(min[i] + d);
                hi[i] = max[i].max(max[i] + d);
            } else {
                lo[i] = min[i] + EPS;
                hi[i] = max[i] - EPS;
            }
        }
        let b0 = [lo[0].floor() as i32, lo[1].floor() as i32, lo[2].floor() as i32];
        let b1 = [(hi[0] - 1e-5).floor() as i32, (hi[1] - 1e-5).floor() as i32, (hi[2] - 1e-5).floor() as i32];
        let mut allowed = d;
        for bx in b0[0]..=b1[0] {
            for by in b0[1]..=b1[1] {
                for bz in b0[2]..=b1[2] {
                    if !solid(bx, by, bz) {
                        continue;
                    }
                    let blk = [bx, by, bz][a] as f32;
                    if d > 0.0 {
                        let gap = blk - max[a];
                        if gap >= -EPS && gap < allowed {
                            allowed = gap.max(0.0);
                        }
                    } else {
                        let gap = blk + 1.0 - min[a];
                        if gap <= EPS && gap > allowed {
                            allowed = gap.min(0.0);
                        }
                    }
                }
            }
        }
        self.pos[a] += allowed;
        (allowed - d).abs() > 1e-7
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ground(_x: i32, y: i32, _z: i32) -> bool { y < 64 }
    fn dry(_: i32, _: i32, _: i32) -> bool { false }

    #[test]
    fn falls_and_lands() {
        let mut p = Player::new([0.5, 70.0, 0.5]);
        for _ in 0..180 {
            p.update(&MoveInput::default(), 1.0 / 60.0, &ground, &dry);
        }
        assert!((p.pos[1] - 64.0).abs() < 1e-3, "y = {}", p.pos[1]);
        assert!(p.on_ground);
    }

    #[test]
    fn wall_stops_player() {
        let wall = |x: i32, y: i32, _z: i32| y < 64 || x >= 5;
        let mut p = Player::new([0.5, 64.0, 0.5]);
        p.yaw = std::f32::consts::FRAC_PI_2; // forward = +x
        let inp = MoveInput { forward: 1.0, ..Default::default() };
        for _ in 0..180 {
            p.update(&inp, 1.0 / 60.0, &wall, &dry);
        }
        assert!((p.pos[0] - 4.7).abs() < 1e-3, "x = {}", p.pos[0]);
    }

    #[test]
    fn jump_height_is_about_1_25() {
        let mut p = Player::new([0.5, 64.0, 0.5]);
        for _ in 0..10 { p.update(&MoveInput::default(), 1.0 / 60.0, &ground, &dry); }
        let jump = MoveInput { jump: true, ..Default::default() };
        let mut peak = 0.0f32;
        for _ in 0..90 {
            p.update(&jump, 1.0 / 60.0, &ground, &dry);
            peak = peak.max(p.pos[1] - 64.0);
        }
        assert!(peak > 1.1 && peak < 1.45, "peak = {peak}");
    }

    #[test]
    fn low_ceiling_blocks_jump() {
        let low = |_x: i32, y: i32, _z: i32| y < 64 || y == 66;
        let mut p = Player::new([0.5, 64.0, 0.5]);
        let jump = MoveInput { jump: true, ..Default::default() };
        for _ in 0..60 { p.update(&jump, 1.0 / 60.0, &low, &dry); }
        assert!(p.pos[1] + HEIGHT <= 66.0 + 1e-3);
    }
}
