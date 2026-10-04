//! Voxel ray traversal (Amanatides & Woo DDA).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub block: [i32; 3],
    /// Empty cell the ray passed through just before the hit (where a new block goes).
    pub prev: [i32; 3],
}

pub fn cast(origin: [f32; 3], dir: [f32; 3], max_dist: f32, solid: &dyn Fn(i32, i32, i32) -> bool) -> Option<Hit> {
    let mut p = [origin[0].floor() as i32, origin[1].floor() as i32, origin[2].floor() as i32];
    if solid(p[0], p[1], p[2]) {
        return None;
    }
    let mut step = [0i32; 3];
    let mut t_delta = [f32::INFINITY; 3];
    let mut t_max = [f32::INFINITY; 3];
    for i in 0..3 {
        if dir[i] > 0.0 {
            step[i] = 1;
            t_delta[i] = 1.0 / dir[i];
            t_max[i] = (p[i] as f32 + 1.0 - origin[i]) * t_delta[i];
        } else if dir[i] < 0.0 {
            step[i] = -1;
            t_delta[i] = -1.0 / dir[i];
            t_max[i] = (origin[i] - p[i] as f32) * t_delta[i];
        }
    }
    loop {
        let a = if t_max[0] < t_max[1] && t_max[0] < t_max[2] { 0 } else if t_max[1] < t_max[2] { 1 } else { 2 };
        if t_max[a] > max_dist {
            return None;
        }
        let prev = p;
        p[a] += step[a];
        t_max[a] += t_delta[a];
        if solid(p[0], p[1], p[2]) {
            return Some(Hit { block: p, prev });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_block_below() {
        let s = |_x: i32, y: i32, _z: i32| y == 10;
        let h = cast([0.5, 14.5, 0.5], [0.0, -1.0, 0.0], 8.0, &s).unwrap();
        assert_eq!(h.block, [0, 10, 0]);
        assert_eq!(h.prev, [0, 11, 0]);
    }

    #[test]
    fn hits_wall_along_x() {
        let s = |x: i32, _y: i32, _z: i32| x == 5;
        let h = cast([0.5, 0.5, 0.5], [1.0, 0.0, 0.0], 8.0, &s).unwrap();
        assert_eq!(h.block, [5, 0, 0]);
        assert_eq!(h.prev, [4, 0, 0]);
    }

    #[test]
    fn out_of_reach_is_none() {
        let s = |x: i32, _y: i32, _z: i32| x == 20;
        assert!(cast([0.5, 0.5, 0.5], [1.0, 0.0, 0.0], 5.0, &s).is_none());
    }

    #[test]
    fn diagonal_ray_finds_target() {
        let s = |x: i32, y: i32, z: i32| x == 3 && y == 3 && z == 3;
        let d = 1.0 / 3f32.sqrt();
        assert_eq!(cast([0.5, 0.5, 0.5], [d, d, d], 8.0, &s).unwrap().block, [3, 3, 3]);
    }
}
