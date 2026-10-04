use crate::input::Controls;
use crate::world::{SecKey, World};
use bcore::block::*;
use bcore::player::{MoveInput, Player};
use bcore::raycast::{self, Hit};
use bcore::worldgen;
use crate::math::forward as forward_dir;

pub const SEED: u32 = 1234;
pub const VIEW_RADIUS: i32 = 3;
pub const REACH: f32 = 5.0;
pub const HOTBAR: [u8; 9] = [STONE, COBBLE, DIRT, GRASS, PLANKS, LOG, SAND, GRAVEL, LEAVES];
const REPEAT: f32 = 0.25;

pub struct Game {
    pub world: World,
    pub player: Player,
    pub sel: usize,
    pub target: Option<Hit>,
    break_cd: f32,
    place_cd: f32,
}

impl Game {
    pub fn new() -> Self {
        let world = World::new(SEED, VIEW_RADIUS);
        let y = worldgen::height_at(SEED, 0, 0) as f32 + 3.0;
        Self { world, player: Player::new([0.5, y, 0.5]), sel: 0, target: None, break_cd: 0.0, place_cd: 0.0 }
    }

    /// Advances the simulation. Returns the sections whose meshes need rebuilding.
    pub fn update(&mut self, c: &Controls, dt: f32) -> Vec<SecKey> {
        let p = &mut self.player;
        p.yaw += c.look_yaw;
        p.pitch = (p.pitch + c.look_pitch).clamp(-1.55, 1.55);
        self.sel = (self.sel as i32 + c.hotbar_delta).rem_euclid(9) as usize;
        if let Some(i) = c.hotbar_set {
            self.sel = i.min(8);
        }
        if c.fly_toggle {
            p.flying = !p.flying;
            p.vel = [0.0; 3];
        }
        let mv = MoveInput { forward: c.move_y, strafe: c.move_x, jump: c.jump, sneak: c.sneak, sprint: c.sprint };
        let w = &self.world;
        p.update(&mv, dt, &|x, y, z| w.is_solid(x, y, z), &|x, y, z| w.is_water(x, y, z));

        let (eye, dir) = (p.eye(), forward_dir(p.yaw, p.pitch));
        self.target = raycast::cast(eye, dir, REACH, &|x, y, z| w.is_target(x, y, z));

        self.break_cd -= dt;
        self.place_cd -= dt;
        if !c.brk { self.break_cd = 0.0; }
        if !c.place { self.place_cd = 0.0; }

        let mut dirty = Vec::new();
        if let Some(hit) = self.target {
            if c.brk && self.break_cd <= 0.0 {
                let [x, y, z] = hit.block;
                if self.world.block(x, y, z) != BEDROCK {
                    dirty.extend(self.world.set_block(x, y, z, AIR));
                }
                self.break_cd = REPEAT;
            } else if c.place && self.place_cd <= 0.0 {
                let [x, y, z] = hit.prev;
                if self.can_place(x, y, z) {
                    dirty.extend(self.world.set_block(x, y, z, HOTBAR[self.sel]));
                }
                self.place_cd = REPEAT;
            }
        }
        dirty.sort_unstable();
        dirty.dedup();
        dirty
    }

    fn can_place(&self, x: i32, y: i32, z: i32) -> bool {
        if !(0..128).contains(&y) || !matches!(self.world.block(x, y, z), AIR | WATER) {
            return false;
        }
        let (lo, hi) = self.player.aabb();
        let inside = lo[0] < x as f32 + 1.0 && hi[0] > x as f32
            && lo[1] < y as f32 + 1.0 && hi[1] > y as f32
            && lo[2] < z as f32 + 1.0 && hi[2] > z as f32;
        !inside
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn break_then_place_edits_world() {
        let mut g = Game::new();
        let top = worldgen::height_at(SEED, 0, 0) as i32;
        g.player.flying = true;
        g.player.pos = [0.5, top as f32 + 3.0, 0.5]; // eye must be within REACH of the ground
        g.player.pitch = -1.55;

        let none = Controls::default();
        g.update(&none, 0.016);
        assert_eq!(g.target.map(|h| h.block), Some([0, top, 0]));

        let dirty = g.update(&Controls { brk: true, ..none }, 0.016);
        assert!(!dirty.is_empty());
        assert_eq!(g.world.block(0, top, 0), AIR);

        g.update(&none, 0.3);
        let placed = g.update(&Controls { place: true, ..none }, 0.016);
        assert!(!placed.is_empty());
        assert_eq!(g.world.block(0, top, 0), HOTBAR[0]);
    }

    #[test]
    fn cannot_place_inside_player() {
        let mut g = Game::new();
        g.player.pos = [0.5, 80.0, 0.5];
        assert!(!g.can_place(0, 80, 0));
        assert!(g.can_place(3, 80, 3));
    }

    #[test]
    fn bedrock_is_unbreakable_and_hotbar_wraps() {
        let mut g = Game::new();
        g.update(&Controls { hotbar_delta: -1, ..Default::default() }, 0.0);
        assert_eq!(g.sel, 8);
        g.update(&Controls { hotbar_delta: 2, ..Default::default() }, 0.0);
        assert_eq!(g.sel, 1);
    }

    #[test]
    fn edge_edit_marks_neighbour_section_dirty() {
        let mut w = World::new(1, 1);
        let d = w.set_block(0, 65, 0, STONE);
        assert!(d.contains(&(0, 0, 4)) && d.contains(&(-1, 0, 4)) && d.contains(&(0, -1, 4)));
    }
}
