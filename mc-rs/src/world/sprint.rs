//! Sprinting: a port of Retro Sprint (periut, b1.7.3 through Ornithe / Babric mixins), the parts mc-rs has.
//! Read from its sources: `ClientPlayerEntityMixin` (start / stop), `LivingEntityMixin` (speed, jump), `PlayerEntityMixin` (hit),
//! `GameRendererMixin*` (field of view), `EntityMixin` (dust). Nothing here touches the world: `lib.rs` feeds `tick` once per 20 Hz tick
//! and reads the numbers back, so the whole state machine runs on a plain host (`cargo test -p mc-rs --lib sprint`).
//!
//! Not ported: the network packets (no multiplayer), the STAPI / RetroAPI key binding (a keyboard has no events in mc-rs yet),
//! hunger (`canSprint` is a constant `true` in the mod and b1.7.3 has no food bar; `In::can` carries what mc-rs does forbid).

/// The mod asks `movementForward >= 0.8` with a keyboard's 1.0. mc-rs normalises the d-pad's diagonals to 0.707, so 0.7 is "forward held"
/// for an arrow, a corner and a stick pushed up.
pub const FWD_MIN: f32 = 0.7;
/// `sprintStartTicks`: a second press of forward within 7 ticks (0.35 s) of the first starts the sprint.
pub const WINDOW: u8 = 7;
/// `setSprinting(true)` arms `ticksSinceSprintingChanged` = 600: a sprint ends by itself after 30 s (press again to go on).
pub const LIMIT: u16 = 600;
/// `GameRendererMixinCaptureVar.method_1305`: the field of view x 1.15 while sprinting; it follows by half the gap each tick.
pub const FOV: f32 = 1.15;
/// Ground drag of b1.7.3 (`0.6 * 0.91`) and air drag (`0.91`), per tick.
const GROUND_DRAG: f32 = 0.546;
const AIR_DRAG: f32 = 0.91;
/// `LivingEntityMixin.increaseMovementSpeed`: +0.035 blocks/tick along the facing each tick (while slower than 0.25). Against the
/// ground drag that settles at 0.035 / (1 - 0.546) = 0.077 blocks/tick over what the input gives, so the sprint adds this many m/s
/// to the walk (4.3 m/s -> 5.8 m/s, x1.35). mc-rs sets the walking velocity from the input every frame, so only this part is kept.
pub const ADD: f32 = 0.035 / (1.0 - GROUND_DRAG) * 20.0;
/// `addedSpeedOnSprintJump`: +0.2 blocks/tick along the facing on a jump, then the drag takes it back (x0.91 a tick in the air).
pub const JUMP: f32 = 0.2 * 20.0;
/// `PlayerEntityMixin.applyKnockback`: the target gets 1.2 blocks/tick away from where the player looks and 0.1 up (m/s here).
pub const KNOCK: f32 = 1.2 * 20.0;
pub const KNOCK_UP: f32 = 0.1 * 20.0;

/// What one tick of the game says about the player.
#[derive(Clone, Copy, Default, Debug)]
pub struct In {
    /// `input.movementForward` after sneaking scaled it (sneaking x0.3 is under `FWD_MIN`, so it ends a sprint like in the mod).
    pub fwd: f32,
    /// The sprint key is held (`input.unused`): sprints at once, no double tap.
    pub key: bool,
    pub ground: bool,
    /// `horizontalCollision`: a wall stopped the move on x or z since the last tick.
    pub blocked: bool,
    /// `isInFluid(Material.WATER)`.
    pub water: bool,
    /// False while riding, asleep or dead.
    pub can: bool,
}

pub struct Sprint {
    on: bool,
    /// Ticks left of the 30 s (`ticksSinceSprintingChanged`).
    left: u16,
    /// `sprintStartTicks`.
    start: u8,
    /// `prevMovementForward`.
    prev: f32,
    /// The forward push in m/s: `ADD` while it runs, more after a jump, back to `ADD` by the drag.
    extra: f32,
    /// `movementFovMultiplier`: 1.0 .. `FOV`, eased every frame by `fov_step`.
    pub fov: f32,
}

impl Default for Sprint {
    /// The mod's `movementFovMultiplier` starts at 0 (the view pops in on the first frame); this one starts at 1.
    fn default() -> Self {
        Self { on: false, left: 0, start: 0, prev: 0.0, extra: 0.0, fov: 1.0 }
    }
}

impl Sprint {
    pub fn on(&self) -> bool {
        self.on
    }

    /// `setSprinting`.
    fn set(&mut self, on: bool) {
        self.on = on;
        self.left = if on { LIMIT } else { 0 };
        self.extra = if on { ADD } else { 0.0 };
    }

    pub fn stop(&mut self) {
        self.set(false);
    }

    /// One 20 Hz tick: `ClientPlayerEntityMixin.tickSprintingMovement`.
    pub fn tick(&mut self, i: In) {
        if self.left > 0 {
            self.left -= 1;
            if self.left == 0 {
                self.set(false);
            }
        }
        self.start = self.start.saturating_sub(1);
        let forward = i.fwd >= FWD_MIN;
        // `bl3`: forward went down and up again (the press edge).
        let pressed = self.prev < FWD_MIN && forward;
        if i.can && i.ground && forward && !self.on {
            if i.key {
                self.set(true);
            } else if self.start == 0 {
                self.start = WINDOW;
            } else if pressed {
                self.set(true);
                self.start = 0;
            }
        }
        if self.on && (!forward || i.blocked || !i.can || i.water) {
            self.set(false);
        }
        self.prev = i.fwd;
        // `velocity` drag between ticks: the jump's push dies by the air or ground drag, never under the plain sprint's.
        if self.on {
            self.extra = (self.extra * if i.ground { GROUND_DRAG } else { AIR_DRAG }).max(ADD);
        }
    }

    /// m/s along the facing to add to the walking velocity (0 when not sprinting).
    pub fn add(&self) -> f32 {
        if self.on { self.extra } else { 0.0 }
    }

    /// The jump impulse fired this frame (`LivingEntityMixin.addedSpeedOnSprintJump`).
    pub fn jump(&mut self) {
        if self.on {
            self.extra += JUMP;
        }
    }

    /// `PlayerEntityMixin.onAttack`: a hit while sprinting is the strong one and ends the sprint. True if it was one.
    pub fn attack(&mut self) -> bool {
        let was = self.on;
        self.set(false);
        was
    }

    /// The field of view multiplier follows its target by half the gap per tick (`movementFovMultiplier += (target - it) * 0.5`).
    pub fn fov_step(&mut self, dt: f32) {
        let target = if self.on { FOV } else { 1.0 };
        self.fov += (target - self.fov) * (1.0 - 0.5f32.powf(dt * 20.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GO: In = In { fwd: 1.0, key: false, ground: true, blocked: false, water: false, can: true };
    const STILL: In = In { fwd: 0.0, ..GO };

    fn run(s: &mut Sprint, steps: &[In]) {
        steps.iter().for_each(|&i| s.tick(i));
    }

    /// The constants are the mod's: sprinting is 1.35 x the walk (5.8 m/s against 4.3), the jump pushes 4 m/s, the hit 24 m/s.
    #[test]
    fn numbers_come_from_the_mod() {
        let walk = 0.1 / (1.0 - GROUND_DRAG) * 20.0; // the settled walk: 0.1 blocks/tick through the same drag
        assert!(((walk + ADD) / walk - 1.35).abs() < 0.01, "{}", (walk + ADD) / walk);
        assert!((ADD - 1.54).abs() < 0.01 && JUMP == 4.0 && KNOCK == 24.0 && KNOCK_UP == 2.0);
    }

    /// A second press within 7 ticks sprints; one held press, however long, never does; a late second press only arms again.
    #[test]
    fn double_tap_starts_a_sprint_and_a_hold_does_not() {
        let mut s = Sprint::default();
        run(&mut s, &[GO, STILL, GO]);
        assert!(s.on(), "tap, release, tap");
        let mut s = Sprint::default();
        run(&mut s, &[GO; 100]);
        assert!(!s.on(), "holding forward is no double tap");
        let mut s = Sprint::default();
        run(&mut s, &[GO]);
        run(&mut s, &[STILL; 10]);
        run(&mut s, &[GO]);
        assert!(!s.on(), "the second press came after the window");
        run(&mut s, &[STILL, GO]);
        assert!(s.on(), "but that press armed a new window");
    }

    /// The key sprints at once; only on the ground; and a sprint needs `can`.
    #[test]
    fn the_key_sprints_on_the_ground_only() {
        let mut s = Sprint::default();
        s.tick(In { key: true, ground: false, ..GO });
        assert!(!s.on());
        s.tick(In { key: true, can: false, ..GO });
        assert!(!s.on());
        s.tick(In { key: true, ..GO });
        assert!(s.on());
    }

    /// Forward released (or sneaking: x0.3), a wall, water, riding / sleeping all end it; the arrows' diagonal (0.707) keeps it.
    #[test]
    fn it_ends_like_the_mod_says() {
        for end in [In { fwd: 0.0, ..GO }, In { fwd: 0.3, ..GO }, In { blocked: true, ..GO }, In { water: true, ..GO }, In { can: false, ..GO }] {
            let mut s = Sprint::default();
            s.tick(In { key: true, ..GO });
            assert!(s.on());
            s.tick(end);
            assert!(!s.on() && s.add() == 0.0, "{end:?}");
        }
        let mut s = Sprint::default();
        s.tick(In { key: true, fwd: std::f32::consts::FRAC_1_SQRT_2, ..GO });
        s.tick(In { fwd: std::f32::consts::FRAC_1_SQRT_2, ..GO });
        assert!(s.on(), "forward-left on the d-pad still sprints");
    }

    /// 600 ticks and it runs out, even with forward held; the key (or a new double tap) starts the next one.
    #[test]
    fn a_sprint_lasts_thirty_seconds() {
        let mut s = Sprint::default();
        s.tick(In { key: true, ..GO });
        run(&mut s, &[GO; 599]);
        assert!(s.on());
        s.tick(GO);
        assert!(!s.on(), "600 ticks after it started");
        s.tick(In { key: true, ..GO });
        assert!(s.on());
    }

    /// The push is `ADD`; a jump adds `JUMP`, the air drag takes it back to `ADD` in a few ticks, the ground faster; it is 0 when not sprinting.
    #[test]
    fn a_sprint_jump_pushes_and_the_drag_takes_it_back() {
        let mut s = Sprint::default();
        s.jump();
        assert_eq!(s.add(), 0.0, "no push without a sprint");
        s.tick(In { key: true, ..GO });
        assert!((s.add() - ADD).abs() < 1e-4, "a plain sprint pushes by ADD, the drag never goes under it");
        s.jump();
        assert!((s.add() - (ADD + JUMP)).abs() < 1e-4);
        s.tick(In { ground: false, ..GO });
        let one = s.add();
        assert!((one - (ADD + JUMP) * AIR_DRAG).abs() < 1e-3, "{one}");
        for _ in 0..40 {
            s.tick(In { ground: false, ..GO });
        }
        assert!((s.add() - ADD).abs() < 1e-4, "never below the plain sprint's push");
        s.jump();
        s.tick(GO);
        assert!(s.add() < (ADD + JUMP) * 0.6, "the ground drag is much stronger");
    }

    /// A hit while sprinting is the strong one and stops the sprint; the next one is plain.
    #[test]
    fn a_sprint_hit_ends_the_sprint() {
        let mut s = Sprint::default();
        s.tick(In { key: true, ..GO });
        assert!(s.attack() && !s.on() && !s.attack());
    }

    /// The field of view goes to 1.15 x and back by half the gap per tick, at any frame rate.
    #[test]
    fn the_view_widens_and_comes_back() {
        let (mut a, mut b) = (Sprint::default(), Sprint::default());
        a.tick(In { key: true, ..GO });
        b.tick(In { key: true, ..GO });
        for _ in 0..20 {
            a.fov_step(0.05);
        }
        for _ in 0..60 {
            b.fov_step(1.0 / 60.0);
        }
        assert!((a.fov - b.fov).abs() < 1e-3 && (a.fov - FOV).abs() < 1e-3, "{} {}", a.fov, b.fov);
        a.stop();
        a.fov_step(0.05);
        assert!((a.fov - (FOV + 1.0) / 2.0).abs() < 1e-3, "half the gap in one tick: {}", a.fov);
        for _ in 0..20 {
            a.fov_step(0.05);
        }
        assert!((a.fov - 1.0).abs() < 1e-3);
    }
}
