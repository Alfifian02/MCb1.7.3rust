//! Block reach-around: Controlify's `ReachAroundHandler` (and `mixins/feature/reacharound/MinecraftMixin`, which feeds it the result of `Minecraft.pick`).
//!
//! Looking down at the air while standing on a block (the edge of a bridge you are building) makes the block under your feet the target, on its
//! side that faces where you look: using the item then puts a block next to it, in front of your feet, without having to turn round or sneak back.
//! "This is parity with bedrock edition" (Controlify's own tooltip). Like Controlify's, it replaces the pick for everything, so an attack there
//! digs the block you stand on (Controlify's behaviour, not a mc-rs choice).
//!
//! Pure `std`: the world comes in as a closure, the answer goes out as plain numbers, so it can be tested alone.
//!
//! Not ported:
//! * `ServerPolicies.REACH_AROUND` (a server that forbids it, `controlify.toast.reach_around_disallowed`): mc-rs has no server to ask.
//! * `ReachAroundMode`'s four values (OFF, singleplayer only, singleplayer and LAN, everywhere): mc-rs is singleplayer, so they are one switch.
//! * The "unfair advantage" warning of `controlify.gui.reach_around.tooltip.warning`: it is about anti-cheats on other people's servers.

/// `cameraEntity.getXRot() >= 45`: looking at least half way between straight ahead and straight down.
pub const MIN_PITCH_DEG: f32 = 45.0;

/// A player half as wide as mc-rs's box (`physics::HALF.x`); the cells under the feet are the ones it overlaps.
pub const HALF_WIDTH: f32 = 0.3;

/// Where the reach-around points: the block under the feet, its side that faces `face` (`MovingObjectPosition.sideHit`: 2 -Z, 3 +Z, 4 -X, 5 +X),
/// and the centre of the block (`Vec3.atCenterOf`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reach {
    pub pos: (i32, i32, i32),
    pub face: u8,
    pub point: [f64; 3],
}

/// What `canReachAround` and `getReachAroundHitResult` read of the player.
#[derive(Clone, Copy, Debug)]
pub struct Player {
    /// The feet (bottom centre of the box).
    pub feet: [f32; 3],
    /// Radians, 0 looking along -Z, `forward = (sin yaw, ., -cos yaw)` (`Camera`).
    pub yaw: f32,
    /// Radians, positive looking down (`Camera`): vanilla's `xRot`.
    pub pitch: f32,
    pub on_ground: bool,
    /// In a boat or on a pig: "don't want to place blocks while riding an entity".
    pub riding: bool,
}

/// `Entity.getDirection()` for the horizontal part of the look: the axis it is closest to (`Direction.fromYRot`).
pub fn facing(yaw: f32) -> u8 {
    let (fx, fz) = (yaw.sin(), -yaw.cos());
    if fx.abs() > fz.abs() {
        if fx > 0.0 { 5 } else { 4 }
    } else if fz > 0.0 {
        3
    } else {
        2
    }
}

/// `Entity.getOnPos()`: the block that holds the player up. Vanilla asks the world for the colliding block nearest to the feet among those the box
/// stands on (`mainSupportingBlockPos`); the cells under the box are tried in x, z order and the nearest to the feet wins, the first on a tie.
/// `solid(x, y, z)`: something to stand on (not air, not a fluid, not a plant).
pub fn supporting_block(p: &Player, solid: &dyn Fn(i32, i32, i32) -> bool) -> Option<(i32, i32, i32)> {
    let y = (p.feet[1] - 1.0e-3).floor() as i32;
    let (x0, x1) = ((p.feet[0] - HALF_WIDTH).floor() as i32, (p.feet[0] + HALF_WIDTH).floor() as i32);
    let (z0, z1) = ((p.feet[2] - HALF_WIDTH).floor() as i32, (p.feet[2] + HALF_WIDTH).floor() as i32);
    let mut best: Option<((i32, i32, i32), f32)> = None;
    for x in x0..=x1 {
        for z in z0..=z1 {
            if !solid(x, y, z) {
                continue;
            }
            let (dx, dy, dz) = (x as f32 + 0.5 - p.feet[0], y as f32 + 0.5 - p.feet[1], z as f32 + 0.5 - p.feet[2]);
            let d = dx * dx + dy * dy + dz * dz;
            if best.map_or(true, |(_, bd)| d < bd) {
                best = Some(((x, y, z), d));
            }
        }
    }
    best.map(|(c, _)| c)
}

/// `canReachAround` + `getReachAroundHitResult`, for a pick that found nothing (the caller only asks then: "if there is already a valid hit, we
/// don't want to override it"). `enabled` is the setting.
pub fn reach_around(p: &Player, enabled: bool, solid: &dyn Fn(i32, i32, i32) -> bool) -> Option<Reach> {
    if !enabled || p.riding || !p.on_ground || p.pitch < MIN_PITCH_DEG.to_radians() - 1.0e-6 {
        return None;
    }
    let pos = supporting_block(p, solid)?;
    Some(Reach { pos, face: facing(p.yaw), point: [pos.0 as f64 + 0.5, pos.1 as f64 + 0.5, pos.2 as f64 + 0.5] })
}

/// The cell a block put on `face` of `pos` lands in (`BlockPos.relative`), for the tests and for callers that only have the reach.
pub fn placed_at(pos: (i32, i32, i32), face: u8) -> (i32, i32, i32) {
    let (dx, dy, dz) = match face {
        0 => (0, -1, 0),
        1 => (0, 1, 0),
        2 => (0, 0, -1),
        3 => (0, 0, 1),
        4 => (-1, 0, 0),
        _ => (1, 0, 0),
    };
    (pos.0 + dx, pos.1 + dy, pos.2 + dz)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, PI};

    fn floor_at(y: i32) -> impl Fn(i32, i32, i32) -> bool {
        move |_, by, _| by == y
    }

    fn down(deg: f32) -> Player {
        Player { feet: [10.5, 65.0, 10.5], yaw: 0.0, pitch: deg.to_radians(), on_ground: true, riding: false }
    }

    #[test]
    fn it_targets_the_block_under_the_feet_on_the_side_you_face() {
        let r = reach_around(&down(60.0), true, &floor_at(64)).unwrap();
        assert_eq!(r.pos, (10, 64, 10));
        assert_eq!(r.face, 2, "yaw 0 looks along -Z");
        assert_eq!(r.point, [10.5, 64.5, 10.5]);
        assert_eq!(placed_at(r.pos, r.face), (10, 64, 9), "a block goes in front of the feet, on the floor");
    }

    #[test]
    fn it_needs_the_setting_the_ground_a_steep_look_and_no_mount() {
        let w = floor_at(64);
        assert!(reach_around(&down(60.0), false, &w).is_none(), "setting off");
        assert!(reach_around(&down(44.0), true, &w).is_none(), "under 45 degrees");
        assert!(reach_around(&down(45.0), true, &w).is_some(), "45 is enough");
        assert!(reach_around(&Player { on_ground: false, ..down(60.0) }, true, &w).is_none(), "in the air");
        assert!(reach_around(&Player { riding: true, ..down(60.0) }, true, &w).is_none(), "riding");
        assert!(reach_around(&Player { pitch: -1.0, ..down(0.0) }, true, &w).is_none(), "looking up");
    }

    #[test]
    fn it_finds_nothing_over_air() {
        assert!(reach_around(&down(60.0), true, &|_, _, _| false).is_none());
    }

    #[test]
    fn the_four_facings_follow_the_camera_yaw() {
        assert_eq!(facing(0.0), 2, "-Z");
        assert_eq!(facing(FRAC_PI_2), 5, "+X");
        assert_eq!(facing(PI), 3, "+Z");
        assert_eq!(facing(-FRAC_PI_2), 4, "-X");
        assert_eq!(facing(0.3), 2);
        assert_eq!(facing(FRAC_PI_2 + 0.4), 5);
    }

    #[test]
    fn standing_on_an_edge_the_block_nearest_the_feet_holds_you() {
        // Feet at x 10.9: the box spans cells 10 and 11, but only cell 10 has a floor.
        let solid = |x: i32, y: i32, _z: i32| y == 64 && x == 10;
        let p = Player { feet: [10.9, 65.0, 10.5], ..down(70.0) };
        assert_eq!(supporting_block(&p, &solid), Some((10, 64, 10)));
        // Both cells are floor: the nearer centre (11.5 is 0.6 away, 10.5 is 0.4) wins.
        let both = |_x: i32, y: i32, _z: i32| y == 64;
        assert_eq!(supporting_block(&p, &both), Some((10, 64, 10)));
        let q = Player { feet: [10.7, 65.0, 10.5], ..p };
        assert_eq!(supporting_block(&q, &both), Some((10, 64, 10)), "10.5 is 0.2 away, 11.5 is 0.8");
    }

    #[test]
    fn a_player_standing_half_a_block_up_on_a_slab_still_finds_the_floor_cell() {
        // A slab's top is at 64.5: the feet are at 64.5 and the cell under them is 64.
        let p = Player { feet: [3.5, 64.5, 3.5], ..down(60.0) };
        assert_eq!(supporting_block(&p, &floor_at(64)), Some((3, 64, 3)));
    }

    #[test]
    fn placed_at_steps_one_cell_along_the_face() {
        assert_eq!(placed_at((0, 0, 0), 5), (1, 0, 0));
        assert_eq!(placed_at((0, 0, 0), 4), (-1, 0, 0));
        assert_eq!(placed_at((0, 0, 0), 3), (0, 0, 1));
        assert_eq!(placed_at((0, 0, 0), 2), (0, 0, -1));
        assert_eq!(placed_at((0, 0, 0), 1), (0, 1, 0));
        assert_eq!(placed_at((0, 0, 0), 0), (0, -1, 0));
    }
}
