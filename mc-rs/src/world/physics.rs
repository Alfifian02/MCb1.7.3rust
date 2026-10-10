//! M2 player physics. Swept-AABB against voxel blocks.
//! Y-up. World bounded to a single chunk in X,Z; Y to [0,H).

use glam::Vec3;

use crate::world::chunk::collision;

/// Vanilla-ish player AABB: 0.6 wide, 1.8 tall, 0.6 deep.
pub const HALF: Vec3 = Vec3::new(0.3, 0.9, 0.3);

pub struct Player {
    pub pos: Vec3,   // feet position (bottom-center axis-aligned)
    pub vel: Vec3,
    pub on_ground: bool,
}

/// Block lookup signature. Returns `Some(block_id)` (raw id, fluids included) or `None` if the query is
/// out of bounds. What an id collides with is `chunk::collision` (fluids, plants, torches: nothing; slab, cactus: a smaller box).
pub type BlockQuery<'a> = &'a dyn Fn(i32, i32, i32) -> Option<u8>;

/// Walk speed in m/s with the stick fully pressed (the callers set the horizontal velocity from it).
pub const WALK_SPEED: f32 = 4.3;
/// Terminal swim speed: `moveFlying` adds 0.02 blocks/tick against a 0.8 drag (water), 0.5 drag (lava).
/// ponytail: the caller sets the horizontal velocity every frame, so this scales it instead of accumulating momentum.
const WATER_SPEED: f32 = 2.0;
const LAVA_SPEED: f32 = 0.8;


/// `Entity.handleWaterMovement` / `handleLavaMovement`: is any cell with an id in `ids` inside the box shrunk 0.4 top and
/// bottom (and `xz` on the sides)? A fluid cell only reaches up to 8/9 of its height (`BlockFluid.getPercentAir(0)`).
/// ponytail: every fluid cell counts as a source block; flowing levels need block metadata (a later milestone).
fn touches(pos: Vec3, half: Vec3, get: BlockQuery<'_>, xz: f32, ids: std::ops::RangeInclusive<u8>) -> bool {
    let (lo, hi) = (pos.y - half.y + 0.401, pos.y + half.y - 0.401);
    for cy in lo.floor() as i32..=hi.floor() as i32 {
        if cy as f32 + 8.0 / 9.0 < lo { continue; }
        for cz in (pos.z - half.z + xz).floor() as i32..=(pos.z + half.z - xz).floor() as i32 {
            for cx in (pos.x - half.x + xz).floor() as i32..=(pos.x + half.x - xz).floor() as i32 {
                if matches!(get(cx, cy, cz), Some(b) if ids.contains(&b)) { return true; }
            }
        }
    }
    false
}

/// Integrate one frame. `dt` is seconds, capped by the caller; `jumping` is the held jump button.
/// In water or lava (`EntityLiving.moveEntityWithHeading`) there is no gravity: the vertical speed is dragged (x0.8 water,
/// x0.5 lava per tick) and sinks 0.02 blocks/tick, a held jump swims up (+0.04 blocks/tick), and pushing against a wall
/// hops out of the fluid (0.3 blocks/tick) when the spot 0.6 higher is free. Returns (touching water, touching lava).
pub fn step(player: &mut Player, dt: f32, jumping: bool, get: BlockQuery<'_>) -> (bool, bool) {
    step_box(player, HALF, dt, jumping, get)
}

/// `step` for a box of half-extents `half` (a pig's is 0.45 on every axis); `pos` is the centre.
pub fn step_box(player: &mut Player, half: Vec3, dt: f32, jumping: bool, get: BlockQuery<'_>) -> (bool, bool) {
    let water = touches(player.pos, half, get, 0.001, 8..=9);
    let lava = !water && touches(player.pos, half, get, 0.1, 10..=11);
    let ticks = dt * 20.0;
    if water || lava {
        let speed = (if water { WATER_SPEED } else { LAVA_SPEED }) / WALK_SPEED;
        player.vel.x *= speed;
        player.vel.z *= speed;
        if jumping { player.vel.y += 0.8 * ticks; }
    } else {
        // Apply gravity to vertical velocity first.
        player.vel.y -= 23.0 * dt;
        // Cap fall speed so a long stall doesn't kill the player.
        if player.vel.y < -55.0 { player.vel.y = -55.0; }
        if jumping && player.on_ground { player.vel.y = 8.4; }
    }

    // Sweep each axis independently.
    let (vx, vz) = (player.vel.x, player.vel.z);
    player.pos = sweep_axis(player.pos, half, player.vel * dt, get, &mut player.vel, player.on_ground && !(water || lava));
    if water || lava {
        player.vel.y = player.vel.y * (if water { 0.8f32 } else { 0.5 }).powf(ticks) - 0.4 * ticks;
        let blocked = (vx != 0.0 && player.vel.x == 0.0) || (vz != 0.0 && player.vel.z == 0.0);
        let up = player.pos + Vec3::new(vx * dt, 0.6, vz * dt);
        if blocked && !aabb_any(up, half, get, |b| b > 0) { player.vel.y = 6.0; }
    }
    player.on_ground = ground_test(player.pos, half, get);
    (water, lava)
}

/// Standing on something: the box, a hair lower, overlaps a collision box (a slab or a cactus top is not at a cell edge).
fn ground_test(pos: Vec3, h: Vec3, get: BlockQuery<'_>) -> bool {
    aabb_hits_solid(pos - Vec3::new(0.0, 0.002, 0.0), h, get)
}

/// `Entity.stepHeight`: a grounded box that is blocked sideways climbs an obstacle up to this high (a slab) without a jump.
const STEP: f32 = 0.5;

fn sweep_axis(pos: Vec3, h: Vec3, delta: Vec3, get: BlockQuery<'_>, vel: &mut Vec3, grounded: bool) -> Vec3 {
    let mut p = pos;
    let hits = |q: Vec3| aabb_hits_solid(q, h, get);
    // One horizontal axis (`axis` 0 = x, 2 = z). Blocked: step up when grounded and the raised box is free, else stop at the wall.
    for axis in [0, 2] {
        let mut d = Vec3::ZERO;
        d[axis] = delta[axis];
        let mut t = p + d;
        if hits(t) {
            if grounded && !hits(t + Vec3::Y * STEP) {
                // Raise by STEP, then settle on whatever is under the new spot (never below where we were).
                let top = t.y + STEP;
                t.y = snap(top, -STEP, |y| hits(Vec3::new(t.x, y, t.z)));
            } else {
                t[axis] = snap(p[axis], delta[axis], |v| { let mut q = p; q[axis] = v; hits(q) });
                vel[axis] = 0.0;
            }
        }
        p = t;
    }
    // Y axis
    let mut try_p = p + Vec3::new(0.0, delta.y, 0.0);
    if aabb_hits_solid(try_p, h, get) {
        try_p.y = snap(p.y, delta.y, |y| aabb_hits_solid(Vec3::new(p.x, y, p.z), h, get));
        vel.y = 0.0;
    }
    try_p
}

/// Does the box at `p` strictly overlap the collision box (`getCollisionBoundingBoxFromPool`) of a block it touches?
fn aabb_hits_solid(p: Vec3, h: Vec3, get: BlockQuery<'_>) -> bool {
    let (lo, hi) = (p - h, p + h);
    for cy in lo.y.floor() as i32..=hi.y.floor() as i32 {
        for cz in lo.z.floor() as i32..=hi.z.floor() as i32 {
            for cx in lo.x.floor() as i32..=hi.x.floor() as i32 {
                let Some(c) = get(cx, cy, cz).and_then(collision) else { continue };
                let (o, b) = (Vec3::new(cx as f32, cy as f32, cz as f32), Vec3::from_slice(&c[..3]));
                let (bl, bh) = (o + b, o + Vec3::from_slice(&c[3..]));
                if hi.x > bl.x && lo.x < bh.x && hi.y > bl.y && lo.y < bh.y && hi.z > bl.z && lo.z < bh.z {
                    return true;
                }
            }
        }
    }
    false
}

/// Does the player box at `p` overlap a cell whose id satisfies `pred`?
fn aabb_any(p: Vec3, h: Vec3, get: BlockQuery<'_>, pred: fn(u8) -> bool) -> bool {
    let (x0, x1) = (p.x - h.x, p.x + h.x);
    let (y0, y1) = (p.y - h.y, p.y + h.y);
    let (z0, z1) = (p.z - h.z, p.z + h.z);
    let (cx0, cx1) = (x0.floor() as i32, x1.floor() as i32);
    let (cy0, cy1) = (y0.floor() as i32, y1.floor() as i32);
    let (cz0, cz1) = (z0.floor() as i32, z1.floor() as i32);
    for cy in cy0..=cy1 {
        for cz in cz0..=cz1 {
            for cx in cx0..=cx1 {
                if matches!(get(cx, cy, cz), Some(b) if pred(b)) {
                    return true;
                }
            }
        }
    }
    false
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Standing on a floor must read as on_ground, otherwise the jump button can never fire.
    #[test]
    fn standing_on_floor_is_on_ground_and_can_jump() {
        let get = |x: i32, y: i32, z: i32| -> Option<u8> {
            if !(0..16).contains(&x) || !(0..16).contains(&z) || y < 0 { return None; }
            Some(if y < 10 { 1 } else { 0 })
        };
        let mut p = Player { pos: Vec3::new(8.5, 10.0 + HALF.y + 0.5, 8.5), vel: Vec3::ZERO, on_ground: false };
        for _ in 0..120 { step(&mut p, 1.0 / 60.0, false, &get); }
        assert!(p.on_ground, "pos.y = {}", p.pos.y);
        step(&mut p, 1.0 / 60.0, true, &get);
        assert!(p.pos.y > 10.0 + HALF.y + 0.05 && !ground_test(p.pos, HALF, &get));
    }

    /// Stone below y = 10, still water (9) for y 10..20, air above: the fluid is not solid.
    fn pool(x: i32, y: i32, z: i32) -> Option<u8> {
        if !(0..16).contains(&x) || !(0..16).contains(&z) || y < 0 { return None; }
        Some(if y < 10 { 1 } else if y < 20 { 9 } else { 0 })
    }

    /// Water is not solid (the player used to stand on its surface): he sinks to the bottom, and holding jump swims him
    /// back up to the surface, where the box leaves the water and he floats about half a block into the top cell.
    #[test]
    fn water_is_not_solid_and_jump_swims_up() {
        let mut p = Player { pos: Vec3::new(8.5, 25.0 + HALF.y, 8.5), vel: Vec3::ZERO, on_ground: false };
        for _ in 0..300 { step(&mut p, 1.0 / 60.0, false, &pool); }
        assert!(p.on_ground && (p.pos.y - HALF.y - 10.0).abs() < 0.05, "feet y = {}", p.pos.y - HALF.y);
        for _ in 0..600 { step(&mut p, 1.0 / 60.0, true, &pool); }
        let feet = p.pos.y - HALF.y;
        assert!((19.0..20.0).contains(&feet), "feet y = {feet}");
    }

    /// A half-high slab is stood on at half a block, a cactus is 1/16 lower and narrower, a torch or a sapling is walked through.
    #[test]
    fn collision_boxes_are_not_the_whole_cell() {
        let floor = |top: u8| move |x: i32, y: i32, z: i32| -> Option<u8> {
            if !(0..16).contains(&x) || !(0..16).contains(&z) || y < 0 { return None; }
            Some(match y { 0..=9 => 1, 10 => top, _ => 0 })
        };
        let rest = |top: u8| {
            let get = floor(top);
            let mut p = Player { pos: Vec3::new(8.5, 14.0, 8.5), vel: Vec3::ZERO, on_ground: false };
            for _ in 0..180 { step(&mut p, 1.0 / 60.0, false, &get); }
            (p.pos.y - HALF.y, p.on_ground)
        };
        let (slab, on) = rest(44);
        assert!(on && (slab - 10.5).abs() < 0.01, "slab: feet y = {slab}");
        let (cactus, on) = rest(81);
        assert!(on && (cactus - (11.0 - 1.0 / 16.0)).abs() < 0.01, "cactus: feet y = {cactus}");
        for plant in [6, 50, 37, 78] {
            assert!(rest(plant).0 < 10.01, "block {plant} must not hold the player up");
        }
        // Beside a cactus (inset 1/16) the player's box fits in the gap a full cell would not leave.
        let get = |x: i32, y: i32, z: i32| -> Option<u8> { Some(if (x, y, z) == (5, 10, 5) { 81 } else { 0 }) };
        assert!(!aabb_hits_solid(Vec3::new(5.0 + 1.0 - 1.0 / 16.0 + 0.31, 10.9 + 0.1, 5.5), HALF, &get));
        assert!(aabb_hits_solid(Vec3::new(5.0 + 1.0 - 1.0 / 16.0 + 0.29, 10.9 + 0.1, 5.5), HALF, &get));
    }

    /// Walking into a slab climbs it without a jump; a full block is still a wall.
    #[test]
    fn a_slab_is_stepped_up_a_full_block_is_not() {
        let at = |top: u8| {
            let get = move |x: i32, y: i32, z: i32| -> Option<u8> {
                if y < 0 { return None; }
                Some(match (x, y) { (_, 0..=9) => 1, (x, 10) if x >= 10 => top, _ => 0 })
            };
            let mut p = Player { pos: Vec3::new(8.5, 10.0 + HALF.y, 8.5), vel: Vec3::ZERO, on_ground: true };
            for _ in 0..120 { p.vel.x = WALK_SPEED; step(&mut p, 1.0 / 60.0, false, &get); }
            (p.pos.x, p.pos.y - HALF.y)
        };
        let (x, y) = at(44);
        assert!(x > 10.5 && (y - 10.5).abs() < 0.01, "slab: x = {x}, feet y = {y}");
        let (x, y) = at(1);
        assert!(x < 9.71 && (y - 10.0).abs() < 0.01, "block: x = {x}, feet y = {y}");
    }
}
