//! Port tabrakan blok dan raycast (World.getCollidingBoundingBoxes, World.rayTraceBlocks, Block.collisionRayTrace).
//! Bentuk statis berasal dari collision_data.rs (dump Java asli). Blok yang bentuknya bergantung pada
//! metadata/tetangga (pintu, tangga, piston, salju, kue, tangga tali, jebakan, rel, ...) mendaftarkan fungsi di
//! `CollisionBehaviors` saat kelasnya diport. Sampai itu terdaftar, bentuk `Dynamic` dianggap tanpa tabrakan.
//! Bagian entity (tabrakan dengan entity lain) ditambahkan di Fase 7.

use crate::aabb::{Aabb, HitResult};
use crate::blocks;
use crate::collision_data::{CollisionShape, COLLIDE_CHECK, COLLISION_SHAPES};
use crate::math;
use crate::vec3::Vec3;
use crate::world::World;

pub type BoundsFn = fn(&World, i32, i32, i32) -> [f32; 6];
pub type CollisionBoxFn = fn(&World, i32, i32, i32) -> Option<Aabb>;
pub type CollidingBoxesFn = fn(&World, i32, i32, i32, &Aabb, &mut Vec<Aabb>);
pub type RayTraceFn = fn(&World, i32, i32, i32, Vec3, Vec3) -> Option<HitResult>;
pub type CanCollideFn = fn(i32, bool) -> bool;

pub struct CollisionBehaviors {
    /// setBlockBoundsBasedOnState
    pub bounds: [Option<BoundsFn>; 256],
    /// getCollisionBoundingBoxFromPool
    pub collision_box: [Option<CollisionBoxFn>; 256],
    /// getCollidingBoundingBoxes (tangga, piston: lebih dari satu kotak)
    pub colliding_boxes: [Option<CollidingBoxesFn>; 256],
    /// collisionRayTrace (pintu, rel, obor, jebakan)
    pub ray_trace: [Option<RayTraceFn>; 256],
    /// canCollideCheck (cairan, tangga)
    pub can_collide: [Option<CanCollideFn>; 256],
}

impl Default for CollisionBehaviors {
    fn default() -> Self {
        Self {
            bounds: [None; 256],
            collision_box: [None; 256],
            colliding_boxes: [None; 256],
            ray_trace: [None; 256],
            can_collide: [None; 256],
        }
    }
}

impl World {
    /// Batas blok saat ini (minX..maxZ, relatif sudut blok), memperhitungkan setBlockBoundsBasedOnState.
    pub fn block_bounds(&self, id: u8, x: i32, y: i32, z: i32) -> [f32; 6] {
        if let Some(f) = self.collision.bounds[id as usize] {
            return f(self, x, y, z);
        }
        match blocks::block(id) {
            Some(d) => d.bounds,
            None => [0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        }
    }

    /// Java: getCollisionBoundingBoxFromPool
    pub fn collision_box(&self, id: u8, x: i32, y: i32, z: i32) -> Option<Aabb> {
        if id == 0 || blocks::block(id).is_none() {
            return None;
        }
        if let Some(f) = self.collision.collision_box[id as usize] {
            return f(self, x, y, z);
        }
        match COLLISION_SHAPES[id as usize - 1] {
            CollisionShape::None => None,
            CollisionShape::Dynamic => None,
            CollisionShape::Box(b) => Some(Aabb::new(
                x as f64 + b[0], y as f64 + b[1], z as f64 + b[2],
                x as f64 + b[3], y as f64 + b[4], z as f64 + b[5],
            )),
        }
    }

    /// Java: canCollideCheck(meta, stopOnLiquid)
    pub fn can_collide_check(&self, id: u8, meta: i32, flag: bool) -> bool {
        if let Some(f) = self.collision.can_collide[id as usize] {
            return f(meta, flag);
        }
        match blocks::block(id) {
            Some(_) => {
                let bit = ((meta & 15) * 2 + flag as i32) as u32;
                COLLIDE_CHECK[id as usize - 1] & (1u32 << bit) != 0
            }
            None => false,
        }
    }

    /// Java: World.getCollidingBoundingBoxes, bagian blok saja (kotak entity: Fase 7).
    pub fn colliding_block_boxes(&self, area: &Aabb, out: &mut Vec<Aabb>) {
        out.clear();
        let x0 = math::floor_double(area.min_x);
        let x1 = math::floor_double(area.max_x + 1.0);
        let y0 = math::floor_double(area.min_y);
        let y1 = math::floor_double(area.max_y + 1.0);
        let z0 = math::floor_double(area.min_z);
        let z1 = math::floor_double(area.max_z + 1.0);
        for x in x0..x1 {
            for z in z0..z1 {
                if !self.chunks.block_exists(x, 64, z) {
                    continue;
                }
                for y in (y0 - 1)..y1 {
                    let id = self.block_id(x, y, z);
                    if id == 0 {
                        continue;
                    }
                    if let Some(f) = self.collision.colliding_boxes[id as usize] {
                        f(self, x, y, z, area, out);
                    } else if let Some(b) = self.collision_box(id, x, y, z) {
                        if area.intersects(&b) {
                            out.push(b);
                        }
                    }
                }
            }
        }
    }

    /// Java: Block.collisionRayTrace (versi dasar). Jarak memakai distanceTo (float), bukan kuadrat.
    pub fn block_ray_trace(&self, id: u8, x: i32, y: i32, z: i32, from: Vec3, to: Vec3) -> Option<HitResult> {
        if let Some(f) = self.collision.ray_trace[id as usize] {
            return f(self, x, y, z, from, to);
        }
        let b = self.block_bounds(id, x, y, z);
        let (min_x, min_y, min_z) = (b[0] as f64, b[1] as f64, b[2] as f64);
        let (max_x, max_y, max_z) = (b[3] as f64, b[4] as f64, b[5] as f64);
        let a = from.add(-(x as f64), -(y as f64), -(z as f64));
        let t = to.add(-(x as f64), -(y as f64), -(z as f64));
        let in_yz = |v: Vec3| v.y >= min_y && v.y <= max_y && v.z >= min_z && v.z <= max_z;
        let in_xz = |v: Vec3| v.x >= min_x && v.x <= max_x && v.z >= min_z && v.z <= max_z;
        let in_xy = |v: Vec3| v.x >= min_x && v.x <= max_x && v.y >= min_y && v.y <= max_y;
        let c = [
            a.intermediate_with_x(t, min_x).filter(|v| in_yz(*v)), // sisi 4
            a.intermediate_with_x(t, max_x).filter(|v| in_yz(*v)), // sisi 5
            a.intermediate_with_y(t, min_y).filter(|v| in_xz(*v)), // sisi 0
            a.intermediate_with_y(t, max_y).filter(|v| in_xz(*v)), // sisi 1
            a.intermediate_with_z(t, min_z).filter(|v| in_xy(*v)), // sisi 2
            a.intermediate_with_z(t, max_z).filter(|v| in_xy(*v)), // sisi 3
        ];
        const SIDE: [i32; 6] = [4, 5, 0, 1, 2, 3];
        let mut best: Option<usize> = None;
        for (i, cand) in c.iter().enumerate() {
            if let Some(v) = cand {
                match best {
                    None => best = Some(i),
                    Some(bi) => {
                        if a.distance_to(*v) < a.distance_to(c[bi].unwrap()) {
                            best = Some(i);
                        }
                    }
                }
            }
        }
        best.map(|i| {
            let hit = c[i].unwrap().add(x as f64, y as f64, z as f64);
            HitResult::Tile { x, y, z, side: SIDE[i], hit }
        })
    }

    fn ray_cell(&self, x: i32, y: i32, z: i32, stop_on_liquid: bool, ignore_no_collision: bool, from: Vec3, to: Vec3) -> Option<HitResult> {
        let id = self.block_id(x, y, z);
        let meta = self.block_metadata(x, y, z) as i32;
        let passes = !ignore_no_collision || id == 0 || self.collision_box(id, x, y, z).is_some();
        if passes && id > 0 && self.can_collide_check(id, meta, stop_on_liquid) {
            self.block_ray_trace(id, x, y, z, from, to)
        } else {
            None
        }
    }

    pub fn ray_trace_blocks(&self, from: Vec3, to: Vec3) -> Option<HitResult> {
        self.ray_trace_blocks_full(from, to, false, false)
    }

    /// Java: func_28105_a (rayTraceBlocks). Maksimal 201 langkah.
    pub fn ray_trace_blocks_full(&self, from: Vec3, to: Vec3, stop_on_liquid: bool, ignore_no_collision: bool) -> Option<HitResult> {
        if from.x.is_nan() || from.y.is_nan() || from.z.is_nan() || to.x.is_nan() || to.y.is_nan() || to.z.is_nan() {
            return None;
        }
        let mut cur = from;
        let tx = math::floor_double(to.x);
        let ty = math::floor_double(to.y);
        let tz = math::floor_double(to.z);
        let mut bx = math::floor_double(cur.x);
        let mut by = math::floor_double(cur.y);
        let mut bz = math::floor_double(cur.z);
        if let Some(h) = self.ray_cell(bx, by, bz, stop_on_liquid, ignore_no_collision, cur, to) {
            return Some(h);
        }
        let mut n = 200i32;
        loop {
            let go = n >= 0;
            n -= 1;
            if !go {
                break;
            }
            if cur.x.is_nan() || cur.y.is_nan() || cur.z.is_nan() {
                return None;
            }
            if bx == tx && by == ty && bz == tz {
                return None;
            }
            let (mut mx, mut my, mut mz) = (true, true, true);
            let (mut nx, mut ny, mut nz) = (999.0f64, 999.0f64, 999.0f64);
            if tx > bx { nx = bx as f64 + 1.0; } else if tx < bx { nx = bx as f64; } else { mx = false; }
            if ty > by { ny = by as f64 + 1.0; } else if ty < by { ny = by as f64; } else { my = false; }
            if tz > bz { nz = bz as f64 + 1.0; } else if tz < bz { nz = bz as f64; } else { mz = false; }
            let (mut tx_t, mut ty_t, mut tz_t) = (999.0f64, 999.0f64, 999.0f64);
            let dx = to.x - cur.x;
            let dy = to.y - cur.y;
            let dz = to.z - cur.z;
            if mx { tx_t = (nx - cur.x) / dx; }
            if my { ty_t = (ny - cur.y) / dy; }
            if mz { tz_t = (nz - cur.z) / dz; }
            let face: i32;
            if tx_t < ty_t && tx_t < tz_t {
                face = if tx > bx { 4 } else { 5 };
                cur.x = nx;
                cur.y += dy * tx_t;
                cur.z += dz * tx_t;
            } else if ty_t < tz_t {
                face = if ty > by { 0 } else { 1 };
                cur.x += dx * ty_t;
                cur.y = ny;
                cur.z += dz * ty_t;
            } else {
                face = if tz > bz { 2 } else { 3 };
                cur.x += dx * tz_t;
                cur.y += dy * tz_t;
                cur.z = nz;
            }
            bx = math::floor_double(cur.x);
            if face == 5 { bx -= 1; }
            by = math::floor_double(cur.y);
            if face == 1 { by -= 1; }
            bz = math::floor_double(cur.z);
            if face == 3 { bz -= 1; }
            if let Some(h) = self.ray_cell(bx, by, bz, stop_on_liquid, ignore_no_collision, cur, to) {
                return Some(h);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::{Chunk, VOLUME};

    fn flat() -> World {
        let mut w = World::new(false, 1);
        for cx in -2..=2 {
            for cz in -2..=2 {
                let mut b = vec![0u8; VOLUME];
                for x in 0..16usize {
                    for z in 0..16usize {
                        for y in 0..60usize {
                            b[(x << 11) | (z << 7) | y] = 1;
                        }
                    }
                }
                w.chunks.add_generated_chunk(Chunk::new(cx, cz, b));
            }
        }
        w
    }

    #[test]
    fn kotak_tabrakan_pemain_di_tanah() {
        let w = flat();
        let area = Aabb::new(8.2, 59.9, 8.2, 8.8, 61.7, 8.8);
        let mut out = Vec::new();
        w.colliding_block_boxes(&area, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].max_y, 60.0);
    }

    #[test]
    fn bunga_tanpa_tabrakan_tapi_batu_ada() {
        let mut w = flat();
        w.set_block(8, 60, 8, 37); // bunga
        let area = Aabb::new(8.2, 60.0, 8.2, 8.8, 61.8, 8.8);
        let mut out = Vec::new();
        w.colliding_block_boxes(&area, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn pagar_lebih_tinggi() {
        let mut w = flat();
        w.set_block(8, 60, 8, 85);
        let area = Aabb::new(8.2, 60.0, 8.2, 8.8, 61.8, 8.8);
        let mut out = Vec::new();
        w.colliding_block_boxes(&area, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].max_y, 61.5);
    }

    #[test]
    fn raycast_dari_atas_kena_permukaan() {
        let w = flat();
        let r = w.ray_trace_blocks(Vec3::new(8.5, 70.5, 8.5), Vec3::new(8.5, 50.5, 8.5));
        match r {
            Some(HitResult::Tile { x, y, z, side, hit }) => {
                assert_eq!((x, y, z, side), (8, 59, 8, 1));
                assert!((hit.y - 60.0).abs() < 1e-9);
            }
            _ => panic!("harus kena"),
        }
    }

    #[test]
    fn raycast_cairan_hanya_jika_diminta() {
        let mut w = flat();
        w.set_block(8, 65, 8, 9); // air diam, metadata 0
        let from = Vec3::new(8.5, 70.5, 8.5);
        let to = Vec3::new(8.5, 50.5, 8.5);
        match w.ray_trace_blocks_full(from, to, false, false) {
            Some(HitResult::Tile { y, .. }) => assert_eq!(y, 59),
            _ => panic!(),
        }
        match w.ray_trace_blocks_full(from, to, true, false) {
            Some(HitResult::Tile { y, hit, .. }) => {
                assert_eq!(y, 65);
                assert!((hit.y - 66.0).abs() < 1e-9);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn raycast_meleset_tidak_kena() {
        let w = flat();
        assert!(w.ray_trace_blocks(Vec3::new(8.5, 70.5, 8.5), Vec3::new(20.5, 70.5, 8.5)).is_none());
    }
}
