//! Player health and damage: `EntityLiving.attackEntityFrom` (the 10-tick damage window), `Entity.updateFallState` /
//! `EntityLiving.fall`, drowning (`air`), lava and burning, and falling out of the world (`Entity.onEntityUpdate`).
//! b1.7.3 has no hunger and no natural regeneration (only Peaceful heals), so health only goes back up on respawn
//! until food exists.
//! ponytail: no suffocation in blocks, no damage flash or knockback, no burning/hurt sounds (all later).

use crate::world::dig::TICK;

pub const MAX_HEALTH: i32 = 20;
pub const MAX_AIR: i32 = 300;

/// What the world says about the player this frame.
#[derive(Clone, Copy, Default)]
pub struct Env {
    /// Vertical movement this frame (negative = fell).
    pub dy: f32,
    pub on_ground: bool,
    /// The box touches water / lava (`handleWaterMovement`, `handleLavaMovement`).
    pub water: bool,
    pub lava: bool,
    /// The eye is inside a water cell (`isInsideOfMaterial(water)`), and its height (`Entity.posY` of a player).
    pub eye_in_water: bool,
    pub eye_y: f32,
}

pub struct Vitals {
    pub health: i32,
    pub air: i32,
    pub fire: i32,
    /// `heartsLife`: 20 right after a hit, counts down; while above 10 only a bigger hit than `last_damage` counts.
    hearts_life: i32,
    last_damage: i32,
    fall: f32,
    acc: f32,
}

impl Default for Vitals {
    fn default() -> Self {
        Self { health: MAX_HEALTH, air: MAX_AIR, fire: 0, hearts_life: 0, last_damage: 0, fall: 0.0, acc: 0.0 }
    }
}

impl Vitals {
    pub fn dead(&self) -> bool {
        self.health <= 0
    }

    /// `EntityLiving.attackEntityFrom` for a damage source with no attacker. False if nothing was taken.
    pub fn hurt(&mut self, dmg: i32) -> bool {
        if self.health <= 0 {
            return false;
        }
        if self.hearts_life > 10 {
            if dmg <= self.last_damage {
                return false;
            }
            self.health -= dmg - self.last_damage;
        } else {
            self.hearts_life = 20;
            self.health -= dmg;
        }
        self.last_damage = dmg;
        true
    }

    /// `updateFallState` (every frame) and `EntityLiving.fall`: landing after `d` blocks costs `ceil(d - 3)`;
    /// water cancels the fall.
    fn land(&mut self, e: &Env) {
        if e.water {
            self.fall = 0.0;
        }
        if e.on_ground {
            let dmg = (self.fall - 3.0).ceil() as i32;
            self.fall = 0.0;
            if dmg > 0 {
                self.hurt(dmg);
            }
        } else if e.dy < 0.0 {
            self.fall -= e.dy;
        }
    }

    /// One 20 Hz tick of `Entity.onEntityUpdate` + `EntityLiving.onEntityUpdate`.
    fn tick(&mut self, e: &Env) {
        if self.hearts_life > 0 {
            self.hearts_life -= 1;
        }
        if e.water {
            self.fire = 0;
        }
        if self.fire > 0 {
            if self.fire % 20 == 0 {
                self.hurt(1);
            }
            self.fire -= 1;
        }
        if e.lava {
            // setOnFireFromLava: 4 damage (the damage window lets one through every 10 ticks) and 30 s of fire.
            self.hurt(4);
            self.fire = 600;
        }
        if e.eye_y < -64.0 {
            self.hurt(1000);
        }
        if e.eye_in_water {
            self.air -= 1;
            if self.air == -20 {
                self.air = 0;
                self.hurt(2);
            }
            self.fire = 0;
        } else {
            self.air = MAX_AIR;
        }
    }

    /// Advance by `dt` seconds: the fall is judged now, everything else on 20 Hz ticks.
    pub fn update(&mut self, dt: f32, e: Env) {
        self.land(&e);
        self.acc += dt;
        while self.acc >= TICK {
            self.acc -= TICK;
            self.tick(&e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 10 block fall costs 7 (ceil(10 - 3)); water cancels it; the damage window ignores equal hits and tops up
    /// bigger ones; drowning starts after 300 + 20 ticks and then costs 2 every 20.
    #[test]
    fn damage_like_java() {
        let mut v = Vitals::default();
        for _ in 0..10 { v.update(0.0, Env { dy: -1.0, ..Env::default() }); }
        v.update(0.0, Env { on_ground: true, ..Env::default() });
        assert_eq!(v.health, 13);

        let mut v = Vitals::default();
        for _ in 0..10 { v.update(0.0, Env { dy: -1.0, ..Env::default() }); }
        v.update(0.0, Env { on_ground: true, water: true, ..Env::default() });
        assert_eq!(v.health, 20);

        let mut v = Vitals::default();
        assert!(v.hurt(4) && !v.hurt(4) && v.hurt(5));
        assert_eq!(v.health, 15);

        let mut v = Vitals::default();
        let under = Env { eye_in_water: true, ..Env::default() };
        for _ in 0..319 { v.tick(&under); }
        assert_eq!(v.health, 20);
        v.tick(&under);
        assert_eq!(v.health, 18);
        for _ in 0..20 { v.tick(&under); }
        assert_eq!(v.health, 16);
        v.tick(&Env::default());
        assert_eq!(v.air, MAX_AIR);
    }
}
