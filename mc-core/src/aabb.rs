//! Port AxisAlignedBB. Pool Java dibuang: `Copy`, semua operasi "FromPool" kini mengembalikan nilai baru di stack.
//! `offset_in_place` meniru `offset` Java yang memodifikasi objek itu sendiri.

use crate::vec3::Vec3;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Aabb {
    pub min_x: f64,
    pub min_y: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub max_z: f64,
}

/// Padanan MovingObjectPosition. `Entity` memakai id (indeks entity) sebagai ganti referensi objek.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HitResult {
    Tile { x: i32, y: i32, z: i32, side: i32, hit: Vec3 },
    Entity { id: u32, hit: Vec3 },
}

impl Aabb {
    #[inline]
    pub const fn new(min_x: f64, min_y: f64, min_z: f64, max_x: f64, max_y: f64, max_z: f64) -> Self {
        Self { min_x, min_y, min_z, max_x, max_y, max_z }
    }

    pub fn add_coord(self, dx: f64, dy: f64, dz: f64) -> Aabb {
        let mut b = self;
        if dx < 0.0 { b.min_x += dx; }
        if dx > 0.0 { b.max_x += dx; }
        if dy < 0.0 { b.min_y += dy; }
        if dy > 0.0 { b.max_y += dy; }
        if dz < 0.0 { b.min_z += dz; }
        if dz > 0.0 { b.max_z += dz; }
        b
    }

    pub fn expand(self, dx: f64, dy: f64, dz: f64) -> Aabb {
        Aabb::new(
            self.min_x - dx, self.min_y - dy, self.min_z - dz,
            self.max_x + dx, self.max_y + dy, self.max_z + dz,
        )
    }

    /// Java: getOffsetBoundingBox (kotak baru)
    pub fn offset_box(self, dx: f64, dy: f64, dz: f64) -> Aabb {
        Aabb::new(
            self.min_x + dx, self.min_y + dy, self.min_z + dz,
            self.max_x + dx, self.max_y + dy, self.max_z + dz,
        )
    }

    /// Java: offset (mengubah dirinya sendiri)
    pub fn offset_in_place(&mut self, dx: f64, dy: f64, dz: f64) {
        self.min_x += dx; self.min_y += dy; self.min_z += dz;
        self.max_x += dx; self.max_y += dy; self.max_z += dz;
    }

    /// Java: func_28195_e (menyusut: min + d, max - d)
    pub fn contract(self, dx: f64, dy: f64, dz: f64) -> Aabb {
        Aabb::new(
            self.min_x + dx, self.min_y + dy, self.min_z + dz,
            self.max_x - dx, self.max_y - dy, self.max_z - dz,
        )
    }

    pub fn calculate_x_offset(&self, o: &Aabb, mut d: f64) -> f64 {
        if o.max_y > self.min_y && o.min_y < self.max_y && o.max_z > self.min_z && o.min_z < self.max_z {
            if d > 0.0 && o.max_x <= self.min_x {
                let v = self.min_x - o.max_x;
                if v < d { d = v; }
            }
            if d < 0.0 && o.min_x >= self.max_x {
                let v = self.max_x - o.min_x;
                if v > d { d = v; }
            }
        }
        d
    }

    pub fn calculate_y_offset(&self, o: &Aabb, mut d: f64) -> f64 {
        if o.max_x > self.min_x && o.min_x < self.max_x && o.max_z > self.min_z && o.min_z < self.max_z {
            if d > 0.0 && o.max_y <= self.min_y {
                let v = self.min_y - o.max_y;
                if v < d { d = v; }
            }
            if d < 0.0 && o.min_y >= self.max_y {
                let v = self.max_y - o.min_y;
                if v > d { d = v; }
            }
        }
        d
    }

    pub fn calculate_z_offset(&self, o: &Aabb, mut d: f64) -> f64 {
        if o.max_x > self.min_x && o.min_x < self.max_x && o.max_y > self.min_y && o.min_y < self.max_y {
            if d > 0.0 && o.max_z <= self.min_z {
                let v = self.min_z - o.max_z;
                if v < d { d = v; }
            }
            if d < 0.0 && o.min_z >= self.max_z {
                let v = self.max_z - o.min_z;
                if v > d { d = v; }
            }
        }
        d
    }

    #[inline]
    pub fn intersects(&self, o: &Aabb) -> bool {
        o.max_x > self.min_x && o.min_x < self.max_x
            && o.max_y > self.min_y && o.min_y < self.max_y
            && o.max_z > self.min_z && o.min_z < self.max_z
    }

    pub fn is_vec_inside(&self, v: Vec3) -> bool {
        v.x > self.min_x && v.x < self.max_x
            && v.y > self.min_y && v.y < self.max_y
            && v.z > self.min_z && v.z < self.max_z
    }

    pub fn average_edge_length(&self) -> f64 {
        ((self.max_x - self.min_x) + (self.max_y - self.min_y) + (self.max_z - self.min_z)) / 3.0
    }

    fn in_yz(&self, v: Vec3) -> bool {
        v.y >= self.min_y && v.y <= self.max_y && v.z >= self.min_z && v.z <= self.max_z
    }
    fn in_xz(&self, v: Vec3) -> bool {
        v.x >= self.min_x && v.x <= self.max_x && v.z >= self.min_z && v.z <= self.max_z
    }
    fn in_xy(&self, v: Vec3) -> bool {
        v.x >= self.min_x && v.x <= self.max_x && v.y >= self.min_y && v.y <= self.max_y
    }

    /// Port func_1169_a (raycast ke kotak). Sisi: 0=-Y 1=+Y 2=-Z 3=+Z 4=-X 5=+X.
    /// Koordinat blok di hasil = 0,0,0 seperti Java; pemanggil mengisinya.
    pub fn calculate_intercept(&self, from: Vec3, to: Vec3) -> Option<HitResult> {
        let c = [
            from.intermediate_with_x(to, self.min_x).filter(|v| self.in_yz(*v)), // sisi 4
            from.intermediate_with_x(to, self.max_x).filter(|v| self.in_yz(*v)), // sisi 5
            from.intermediate_with_y(to, self.min_y).filter(|v| self.in_xz(*v)), // sisi 0
            from.intermediate_with_y(to, self.max_y).filter(|v| self.in_xz(*v)), // sisi 1
            from.intermediate_with_z(to, self.min_z).filter(|v| self.in_xy(*v)), // sisi 2
            from.intermediate_with_z(to, self.max_z).filter(|v| self.in_xy(*v)), // sisi 3
        ];
        const SIDE: [i32; 6] = [4, 5, 0, 1, 2, 3];

        // Urutan evaluasi dan "<" ketat sama dengan Java (yang pertama menang saat seri).
        let mut best: Option<usize> = None;
        for (i, cand) in c.iter().enumerate() {
            if let Some(v) = cand {
                match best {
                    None => best = Some(i),
                    Some(b) => {
                        if from.square_distance_to(*v) < from.square_distance_to(c[b].unwrap()) {
                            best = Some(i);
                        }
                    }
                }
            }
        }
        best.map(|i| HitResult::Tile { x: 0, y: 0, z: 0, side: SIDE[i], hit: c[i].unwrap() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabrakan_x() {
        let blok = Aabb::new(1.0, 0.0, 0.0, 2.0, 1.0, 1.0);
        let pemain = Aabb::new(0.0, 0.0, 0.0, 0.6, 1.8, 0.6);
        // bergerak +X sejauh 1.0, terhenti 0.4 di depan blok
        assert!((blok.calculate_x_offset(&pemain, 1.0) - 0.4).abs() < 1e-12);
    }

    #[test]
    fn raycast_sisi() {
        let b = Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        let r = b.calculate_intercept(Vec3::new(-1.0, 0.5, 0.5), Vec3::new(2.0, 0.5, 0.5));
        match r {
            Some(HitResult::Tile { side, hit, .. }) => {
                assert_eq!(side, 4);
                assert!((hit.x - 0.0).abs() < 1e-12);
            }
            _ => panic!("harus kena"),
        }
    }

    #[test]
    fn irisan() {
        let a = Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        assert!(a.intersects(&Aabb::new(0.5, 0.5, 0.5, 2.0, 2.0, 2.0)));
        assert!(!a.intersects(&Aabb::new(1.0, 0.0, 0.0, 2.0, 1.0, 1.0))); // bersentuhan saja = tidak
    }
}
