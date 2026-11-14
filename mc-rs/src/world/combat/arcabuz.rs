//! C8: the arcabuz's gun (`Arcabuz`, `ArcabuzState`, `ArcabuzShot`, `ArcabuzMuzzle`). **NOT b1.7.3.**
//! **UNVERIFIED: written without compiling or running anything.**
//!
//! The left button is the bayonet (an ordinary melee weapon: `Profile::Arcabuz`, the catalog). The right button is a three-step gun:
//! 1. **reload**: hold the special button 50 ticks (letting go cancels it), and one round of ammo is paid when it ends;
//! 2. **light the fuse**: press once loaded; the fuse burns 20 ticks, nothing can be done meanwhile;
//! 3. **the shot**: when it ends the gun fires everything in the barrel's cone (6.5 blocks, 32 x 24 degrees, through walls as the mod's
//!    does not check), up to 10 targets, three times the melee damage each, a knockback of the gun's own profile, 6 seconds of fire,
//!    and a kick that pushes the shooter back and shakes the view.
//!
//! Pure like the rest of `combat`: the state is two counters and a flag, the caller (`lib.rs`) pays the ammo and does the shot.
//!
//! Deviations, one line each to change:
//! * **Ammo** is gunpowder (id `AMMO`, one a load): the mod spends its Firestone, which is dropped with the Nether Reactor.
//! * The three enchantments of the gun (`MECHA_CORTA`, `DRY_POWDER`, `MUNGUS`, `REY_DE_LAS_MOSCAS`) are dropped with the rest, so the
//!   fuse and the reload are the base 20 and 50 ticks, and there is no shell and no swarm.
//! * The state is one for the player, not one per stack (the mod keeps it in the item's component): it only counts while the gun is in
//!   hand (`drop_reload` cancels a reload when it is put away; a lit fuse waits for it), and a loaded gun stays loaded.
//! * No creative-mode free ammo (the port has no creative), and no per-shot wear (the mod's shot does not damage the item either).

use glam::Vec3;

/// `ArcabuzState.FUSE_TICKS` and `RELOAD_TICKS`.
pub const FUSE_TICKS: u32 = 20;
pub const RELOAD_TICKS: u32 = 50;
/// `ArcabuzShot`: three times the melee damage to each of at most ten targets, set on fire for six seconds (the mob's fire is 1 a second).
pub const SHOT_MULTIPLIER: f32 = 3.0;
pub const MAX_TARGETS: usize = 10;
pub const FIRE_TICKS: u16 = 120;
/// `ArcabuzMuzzle`: where the blast starts (blocks along the look from the eye) and how hard the shot kicks the shooter back (blocks a
/// tick, along the look, on the ground plane).
pub const MUZZLE_FORWARD: f32 = 1.05;
pub const RECOIL: f32 = 0.25;
/// Gunpowder, one a load.
pub const AMMO: u16 = 289;
/// `ArcabuzMuzzle.LAVA_PER_TARGET` / `LAVA_WITHOUT_TARGETS`: the specks of the blast, along the line to each target or in a ball.
pub const SPECKS_PER_TARGET: usize = 4;
pub const SPECKS_WITHOUT_TARGET: usize = 6;

/// What a press of the special button did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Press {
    /// The fuse is lit: 20 ticks to the shot.
    Lit,
    /// A reload began and ends in 50 ticks if the button stays down.
    Reloading,
    /// Empty and nothing to load it with.
    NoAmmo,
    /// The fuse burns or a reload runs: nothing to do.
    Busy,
}

/// What a tick of the gun produced.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tick {
    Idle,
    /// The fuse ran out: fire now.
    Fire,
    /// The reload ended and the ammo was there: the caller pays it.
    Loaded,
    /// The reload was let go of (or the ammo went): back to empty.
    Cancelled,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct State {
    loaded: bool,
    /// Ticks the fuse still burns; 0 = not lit.
    fuse: u32,
    /// Ticks the reload still needs; 0 = not reloading.
    reload: u32,
}

impl State {
    pub fn loaded(&self) -> bool {
        self.loaded
    }

    pub fn burning(&self) -> bool {
        self.fuse > 0
    }

    pub fn reloading(&self) -> bool {
        self.reload > 0
    }

    /// `ArcabuzState.canReload`.
    fn can_reload(&self) -> bool {
        !self.loaded && !self.burning() && !self.reloading()
    }

    /// How far the reload has got, 0..1 (0 when none runs): what the charge bar shows.
    pub fn reload_progress(&self) -> f32 {
        if self.reloading() { 1.0 - self.reload as f32 / RELOAD_TICKS as f32 } else { 0.0 }
    }

    /// How much of the fuse is left, 0..1 (0 when not lit).
    pub fn fuse_left(&self) -> f32 {
        self.fuse as f32 / FUSE_TICKS as f32
    }

    /// `Arcabuz.resolve`: loaded and idle lights the fuse (the round is already in the barrel), empty and idle starts a reload if there is
    /// ammo to pay it with. There is no cooldown gate: the press may be a reload, which has none of its own.
    pub fn press(&mut self, has_ammo: bool) -> Press {
        if self.loaded && !self.burning() && !self.reloading() {
            (self.loaded, self.fuse) = (false, FUSE_TICKS);
            Press::Lit
        } else if self.can_reload() {
            if !has_ammo {
                return Press::NoAmmo;
            }
            self.reload = RELOAD_TICKS;
            Press::Reloading
        } else {
            Press::Busy
        }
    }

    /// One 20 Hz tick while the gun is in hand (`Arcabuz.tick`): the fuse first, then the reload, which only runs while `button` is down.
    pub fn tick(&mut self, button: bool, has_ammo: bool) -> Tick {
        if self.fuse > 0 {
            self.fuse -= 1;
            return if self.fuse == 0 { Tick::Fire } else { Tick::Idle };
        }
        if self.reload == 0 {
            return Tick::Idle;
        }
        if !button {
            self.reload = 0;
            return Tick::Cancelled;
        }
        self.reload -= 1;
        if self.reload > 0 {
            return Tick::Idle;
        }
        if has_ammo {
            self.loaded = true;
            Tick::Loaded
        } else {
            Tick::Cancelled
        }
    }

    /// The gun left the hand: a reload in progress is lost (the button cannot be held on an item that is not there).
    pub fn drop_reload(&mut self) {
        self.reload = 0;
    }
}

/// `ArcabuzShot.damageFor`: `attack` is the weapon's own damage (the player's point included, 6).
pub fn shot_damage(attack: f32) -> f32 {
    attack * SHOT_MULTIPLIER
}

/// `Arcabuz.aimDeviation`: 0 straight ahead, 1 at right angles, 2 behind. A target dead on the crosshair is the one the shot prefers.
pub fn aim_deviation(eye: Vec3, look: Vec3, target: Vec3) -> f32 {
    let to = target - eye;
    let len = to.length();
    if len <= 1e-6 { 0.0 } else { 1.0 - to.dot(look) / len }
}

/// `Arcabuz.targetsInCone` + `ArcabuzShot.capped`: `found` are `(index, aim deviation, squared distance)` of what is in the cone, in any
/// order; the best aimed first, the nearest breaking a tie, at most `MAX_TARGETS`.
pub fn order(mut found: Vec<(usize, f32, f32)>) -> Vec<usize> {
    found.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.2.total_cmp(&b.2)));
    found.into_iter().take(MAX_TARGETS).map(|f| f.0).collect()
}

/// The kick: `-look` on the ground plane, `RECOIL` blocks a tick (the look is a unit vector, so a shot fired up or down kicks less).
pub fn recoil(look: Vec3) -> (f32, f32) {
    (-look.x * RECOIL, -look.z * RECOIL)
}

/// Where the blast starts.
pub fn muzzle(eye: Vec3, look: Vec3) -> Vec3 {
    eye + look * MUZZLE_FORWARD
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(s: &mut State, n: u32, button: bool) -> Vec<Tick> {
        (0..n).map(|_| s.tick(button, true)).collect()
    }

    #[test]
    fn an_empty_gun_reloads_in_fifty_ticks_if_the_button_stays_down_and_then_is_loaded() {
        let mut s = State::default();
        assert_eq!(s.press(true), Press::Reloading);
        assert!(s.reloading() && !s.loaded());
        let t = run(&mut s, RELOAD_TICKS, true);
        assert_eq!(t.iter().filter(|&&x| x == Tick::Loaded).count(), 1);
        assert_eq!(t.last(), Some(&Tick::Loaded), "on the fiftieth tick, not before");
        assert!(s.loaded() && !s.reloading());
        assert_eq!(s.tick(true, true), Tick::Idle);
    }

    #[test]
    fn letting_go_cancels_the_reload_and_nothing_is_left_half_done() {
        let mut s = State::default();
        s.press(true);
        run(&mut s, 20, true);
        assert!((s.reload_progress() - 0.4).abs() < 1e-6);
        assert_eq!(s.tick(false, true), Tick::Cancelled);
        assert_eq!((s.loaded(), s.reloading(), s.reload_progress()), (false, false, 0.0));
        assert_eq!(s.press(true), Press::Reloading, "and it can start again");
    }

    #[test]
    fn a_reload_needs_ammo_at_the_press_and_at_the_end() {
        let mut s = State::default();
        assert_eq!(s.press(false), Press::NoAmmo);
        assert!(!s.reloading());
        s.press(true);
        for _ in 0..RELOAD_TICKS - 1 {
            assert_eq!(s.tick(true, true), Tick::Idle);
        }
        assert_eq!(s.tick(true, false), Tick::Cancelled, "the ammo went while it loaded");
        assert!(!s.loaded());
    }

    #[test]
    fn a_loaded_gun_lights_its_fuse_burns_twenty_ticks_and_fires_once_empty() {
        let mut s = State::default();
        s.press(true);
        run(&mut s, RELOAD_TICKS, true);
        assert_eq!(s.press(true), Press::Lit);
        assert!(s.burning() && !s.loaded());
        assert_eq!(s.fuse_left(), 1.0);
        let t = run(&mut s, FUSE_TICKS, false); // the button is not needed for the fuse
        assert_eq!(t.iter().filter(|&&x| x == Tick::Fire).count(), 1);
        assert_eq!(t.last(), Some(&Tick::Fire));
        assert_eq!((s.burning(), s.loaded(), s.fuse_left()), (false, false, 0.0));
        assert_eq!(s.press(true), Press::Reloading, "empty again");
    }

    #[test]
    fn nothing_else_can_be_pressed_while_the_fuse_burns_or_a_reload_runs() {
        let mut s = State::default();
        s.press(true);
        assert_eq!(s.press(true), Press::Busy);
        run(&mut s, RELOAD_TICKS, true);
        s.press(true);
        assert_eq!(s.press(true), Press::Busy);
        assert_eq!(s.tick(false, true), Tick::Idle, "a lit fuse ignores the button");
    }

    #[test]
    fn putting_the_gun_away_loses_a_reload_but_keeps_a_loaded_barrel() {
        let mut s = State::default();
        s.press(true);
        run(&mut s, 10, true);
        s.drop_reload();
        assert!(!s.reloading() && !s.loaded());
        s.press(true);
        run(&mut s, RELOAD_TICKS, true);
        s.drop_reload();
        assert!(s.loaded());
    }

    #[test]
    fn the_shot_is_three_times_the_melee_damage_and_the_best_aimed_go_first_to_ten() {
        assert_eq!(shot_damage(6.0), 18.0);
        let eye = Vec3::new(0.0, 1.6, 0.0);
        let look = Vec3::Z;
        let dead_on = aim_deviation(eye, look, Vec3::new(0.0, 1.6, 5.0));
        let aside = aim_deviation(eye, look, Vec3::new(3.0, 1.6, 4.0));
        let behind = aim_deviation(eye, look, Vec3::new(0.0, 1.6, -2.0));
        assert!(dead_on.abs() < 1e-6 && (aside - 0.2).abs() < 1e-5 && (behind - 2.0).abs() < 1e-6);
        assert_eq!(aim_deviation(eye, look, eye), 0.0);
        // best aimed first, the nearer one breaking a tie
        assert_eq!(order(vec![(0, 0.1, 4.0), (1, 0.0, 25.0), (2, 0.0, 9.0)]), [2, 1, 0]);
        let many: Vec<(usize, f32, f32)> = (0..15).map(|i| (i, 0.0, i as f32)).collect();
        assert_eq!(order(many), (0..10).collect::<Vec<_>>());
        assert!(order(Vec::new()).is_empty());
    }

    #[test]
    fn the_kick_goes_back_along_the_look_and_the_muzzle_is_ahead_of_the_eye() {
        let (x, z) = recoil(Vec3::new(0.0, 0.0, 1.0));
        assert!(x.abs() < 1e-6 && (z + 0.25).abs() < 1e-6);
        let (x, z) = recoil(Vec3::new(0.0, 1.0, 0.0));
        assert_eq!((x, z), (0.0, 0.0), "straight up kicks nothing sideways");
        let m = muzzle(Vec3::new(1.0, 2.0, 3.0), Vec3::X);
        assert!((m - Vec3::new(2.05, 2.0, 3.0)).length() < 1e-6);
    }
}
