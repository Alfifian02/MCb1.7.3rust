//! M2 player physics. Swept-AABB against voxel blocks.
//! Y-up. World bounded to a single chunk in X,Z; Y to [0,H).

use glam::Vec3;

/// Vanilla-ish player AABB: 0.6 wide, 1.8 tall, 0.6 deep.
pub const HALF: Vec3 = Vec3::new(0.3, 0.9, 0.3);

pub struct Player {
    pub pos: Vec3,   // feet position (bottom-center axis-aligned)
    pub vel: Vec3,
    pub on_ground: bool,
}

/// Block lookup signature. Returns `Some(block_id)` if the cell is solid,
/// `None` if the query is out of bounds. Anything non-zero counts as solid.
pub type BlockQuery<'a> = &'a dyn Fn(i32, i32, i32) -> Option<u8>;

/// Integrate one frame. `dt` is seconds, capped by the caller.
pub fn step(player: &mut Player, dt: f32, get: BlockQuery<'_>) {
    // Apply gravity to vertical velocity first.
    player.vel.y -= 23.0 * dt;
    // Cap fall speed so a long stall doesn't kill the player.
    if player.vel.y < -55.0 { player.vel.y = -55.0; }

    // Sweep each axis independently.
    player.pos = sweep_axis(player.pos, player.vel * dt, get, &mut player.vel);
    player.on_ground = ground_test(player.pos, get);
}

fn ground_test(pos: Vec3, get: BlockQuery<'_>) -> bool {
    // Test the four corners of the bottom face just below current pos.
    let h = HALF;
    let below = pos + Vec3::new(0.0, -0.001, 0.0);
    let corners = [
        (below.x - h.x, below.y, below.z - h.z),
        (below.x + h.x, below.y, below.z - h.z),
        (below.x - h.x, below.y, below.z + h.z),
        (below.x + h.x, below.y, below.z + h.z),
    ];
    corners.iter().any(|&(x, y, z)| {
        let cx = x.floor() as i32;
        let cy = y.floor() as i32;
        let cz = z.floor() as i32;
        matches!(get(cx, cy, cz), Some(b) if b > 0)
    })
}

fn sweep_axis(pos: Vec3, delta: Vec3, get: BlockQuery<'_>, vel: &mut Vec3) -> Vec3 {
    let mut p = pos;
    // X axis
    let mut try_p = p + Vec3::new(delta.x, 0.0, 0.0);
    if aabb_hits_solid(try_p, get) {
        try_p.x = snap(p.x, delta.x, |x| aabb_hits_solid_x(p.y, p.z, x, get));
        vel.x = 0.0;
    }
    p = try_p;
    // Z axis
    let mut try_p = p + Vec3::new(0.0, 0.0, delta.z);
    if aabb_hits_solid(try_p, get) {
        try_p.z = snap(p.z, delta.z, |z| aabb_hits_solid_z(p.y, p.x, z, get));
        vel.z = 0.0;
    }
    p = try_p;
    // Y axis
    let mut try_p = p + Vec3::new(0.0, delta.y, 0.0);
    if aabb_hits_solid(try_p, get) {
        try_p.y = snap(p.y, delta.y, |y| aabb_hits_solid_y(p.x, p.z, y, get));
        vel.y = 0.0;
    }
    try_p
}

fn aabb_hits_solid(p: Vec3, get: BlockQuery<'_>) -> bool {
    let h = HALF;
    let (x0, x1) = (p.x - h.x, p.x + h.x);
    let (y0, y1) = (p.y - h.y, p.y + h.y);
    let (z0, z1) = (p.z - h.z, p.z + h.z);
    let (cx0, cx1) = (x0.floor() as i32, x1.floor() as i32);
    let (cy0, cy1) = (y0.floor() as i32, y1.floor() as i32);
    let (cz0, cz1) = (z0.floor() as i32, z1.floor() as i32);
    for cy in cy0..=cy1 {
        for cz in cz0..=cz1 {
            for cx in cx0..=cx1 {
                if matches!(get(cx, cy, cz), Some(b) if b > 0) {
                    return true;
                }
            }
        }
    }
    false
}

fn aabb_hits_solid_x(y: f32, z: f32, x: f32, get: BlockQuery<'_>) -> bool {
    aabb_hits_solid(Vec3::new(x, y, z), get)
}
fn aabb_hits_solid_z(y: f32, x: f32, z: f32, get: BlockQuery<'_>) -> bool {
    aabb_hits_solid(Vec3::new(x, y, z), get)
}
fn aabb_hits_solid_y(x: f32, z: f32, y: f32, get: BlockQuery<'_>) -> bool {
    aabb_hits_solid(Vec3::new(x, y, z), get)
}

/// Binary search for the first `t in [0,|delta|]` that *would* collide, then snap to just before.
fn snap<F: Fn(f32) -> bool>(start: f32, delta: f32, hits: F) -> f32 {
    if delta == 0.0 { return start; }
    let target = start + delta;
    let mut lo = start;
    let mut hi = target;
    // 12 iterations of bisection is plenty for voxel resolution.
    for _ in 0..12 {
        let m = 0.5 * (lo + hi);
        if hits(m) { hi = m; } else { lo = m; }
    }
    lo
}
