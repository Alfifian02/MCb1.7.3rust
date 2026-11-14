//! `SharedQuadIndexBuffer`: every terrain face is a quad of four consecutive vertices, so the triangles that draw it are the
//! same for every mesh: quad `n` is `4n, 4n + 1, 4n + 2, 4n + 2, 4n + 3, 4n`. One buffer of those, shared by all meshes,
//! replaces an index list per mesh: nothing to upload, nothing to allocate, and 6 index elements per 4 vertices only once.
//!
//! This is the bookkeeping and the contents; the host creates the GPU buffer from [`indices`]. Sodium draws a mesh with
//! `baseVertex` = its first vertex and the indices of quad 0; `base_vertex` is not in GLES 3.0 (`DownlevelFlags::BASE_VERTEX`),
//! so the host keeps every mesh's first vertex on a multiple of 4 and draws the index range of its absolute quads instead
//! ([`first_index`] / [`index_count`]): the same triangles, no `base_vertex`, the one buffer for every page of the same size.
//!
//! The pair of triangles is Sodium's `(0, 1, 2)`, `(2, 3, 0)`: the same two triangles, with the same winding, as `(0, 1, 2)`,
//! `(0, 2, 3)`. A quad whose other diagonal is wanted (smooth lighting) or that is seen from both sides is the host's to
//! arrange: rotate its corners, or emit it twice with the corners reversed.

pub const ELEMENTS_PER_QUAD: u32 = 6;
pub const VERTICES_PER_QUAD: u32 = 4;
/// `IndexFormat.INTEGER.getMaxPrimitiveCount()`: `Integer.MAX_VALUE / 4` quads.
pub const MAX_QUADS: u32 = i32::MAX as u32 / VERTICES_PER_QUAD;

/// The index elements of `quads` quads (`IndexFormat.createIndexBuffer`).
pub fn indices(quads: u32) -> Vec<u32> {
    let mut out = vec![0u32; quads as usize * ELEMENTS_PER_QUAD as usize];
    fill(&mut out);
    out
}

/// Writes the pattern into `out`, which holds whole quads (`len % 6 == 0`).
pub fn fill(out: &mut [u32]) {
    assert_eq!(out.len() % ELEMENTS_PER_QUAD as usize, 0, "whole quads only");
    for (n, q) in out.chunks_exact_mut(ELEMENTS_PER_QUAD as usize).enumerate() {
        let v = n as u32 * VERTICES_PER_QUAD;
        q.copy_from_slice(&[v, v + 1, v + 2, v + 2, v + 3, v]);
    }
}

/// The first index element that draws the quad starting at vertex `first_vertex` (a multiple of 4).
pub fn first_index(first_vertex: u32) -> u32 {
    assert_eq!(first_vertex % VERTICES_PER_QUAD, 0, "a mesh starts on a quad");
    (first_vertex / VERTICES_PER_QUAD) * ELEMENTS_PER_QUAD
}

/// How many index elements draw `vertices` vertices (a multiple of 4).
pub fn index_count(vertices: u32) -> u32 {
    assert_eq!(vertices % VERTICES_PER_QUAD, 0, "whole quads only");
    (vertices / VERTICES_PER_QUAD) * ELEMENTS_PER_QUAD
}

/// `getNextSize`: the quad capacity to grow to when `needed` quads are wanted and `have` fit now: double, or `needed` plus 16384,
/// whichever is more, never past [`MAX_QUADS`].
pub fn next_size(have: u32, needed: u32) -> u32 {
    (have as u64 * 2).max(needed as u64 + 16384).min(MAX_QUADS as u64) as u32
}

/// `ensureCapacity` for a host whose buffer grows on demand: how many quads the buffer holds (0 before the first call).
#[derive(Default, Debug)]
pub struct Capacity {
    quads: u32,
}

impl Capacity {
    pub fn quads(&self) -> u32 {
        self.quads
    }

    /// Makes room for `elements` index elements. `Some(n)`: the host makes a buffer of `n` quads (`indices(n)`) and drops the old one;
    /// `None`: it is big enough already. Panics past [`MAX_QUADS`] (Sodium throws).
    pub fn ensure(&mut self, elements: u32) -> Option<u32> {
        let quads = elements / ELEMENTS_PER_QUAD;
        assert!(quads <= MAX_QUADS, "more quads than a u32 index can address");
        if quads <= self.quads {
            return None;
        }
        self.quads = next_size(self.quads, quads);
        Some(self.quads)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two triangles of every quad, as cyclic vertex sequences, are `(0, 1, 2)` and `(0, 2, 3)` of the quad: the winding of the
    /// mesher's own pair, so a mesh that used to draw `[b, b + 1, b + 2, b, b + 2, b + 3]` draws the same pixels.
    #[test]
    fn pattern_is_sodiums_and_keeps_the_winding() {
        let i = indices(3);
        assert_eq!(&i[..6], &[0, 1, 2, 2, 3, 0]);
        assert_eq!(&i[6..12], &[4, 5, 6, 6, 7, 4]);
        assert_eq!(&i[12..], &[8, 9, 10, 10, 11, 8]);
        // Same cyclic order as (b, b+1, b+2) and (b, b+2, b+3): rotate (2, 3, 0) to (0, 2, 3).
        let cyc = |t: [u32; 3]| {
            let m = (0..3).min_by_key(|&k| t[k]).unwrap();
            [t[m], t[(m + 1) % 3], t[(m + 2) % 3]]
        };
        for q in 0..3u32 {
            let b = q * 4;
            let s = &i[q as usize * 6..][..6];
            assert_eq!(cyc([s[0], s[1], s[2]]), [b, b + 1, b + 2]);
            assert_eq!(cyc([s[3], s[4], s[5]]), [b, b + 2, b + 3]);
        }
    }

    /// A run of quads `a..b` of a mesh at vertex `v` is the index range `first_index(v + 4a) .. + index_count(4 (b - a))`, and the
    /// elements there are exactly the vertices `v + 4a ..`: the draw needs no base vertex.
    #[test]
    fn a_runs_indices_name_its_own_vertices() {
        let i = indices(100);
        let (v, a, b) = (40u32, 3u32, 7u32);
        let first = first_index(v + 4 * a) as usize;
        let n = index_count(4 * (b - a)) as usize;
        assert_eq!((first, n), (6 * (10 + 3), 24));
        let run = &i[first..first + n];
        assert!(run.iter().all(|&e| (v + 4 * a..v + 4 * b).contains(&e)));
        assert_eq!(run.iter().min(), Some(&(v + 4 * a)));
        assert_eq!(run.iter().max(), Some(&(v + 4 * b - 1)));
    }

    #[test]
    #[should_panic]
    fn a_mesh_must_start_on_a_quad() {
        first_index(6);
    }

    #[test]
    fn capacity_grows_like_sodium() {
        let mut c = Capacity::default();
        assert_eq!(c.ensure(0), None);
        assert_eq!(c.ensure(6), Some(16385)); // max(0 * 2, 1 + 16384)
        assert_eq!(c.ensure(6 * 16385), None);
        assert_eq!(c.ensure(6 * 16386), Some(32770)); // doubling beats +16384
        assert_eq!(c.quads(), 32770);
        assert_eq!(next_size(MAX_QUADS - 5, MAX_QUADS - 1), MAX_QUADS); // capped
    }

    /// Every vertex of a page of `n` quads is named by exactly one quad.
    #[test]
    fn every_vertex_is_drawn_once_per_triangle_pair() {
        let n = 1000u32;
        let mut seen = vec![0u32; n as usize * 4];
        for &e in &indices(n) {
            seen[e as usize] += 1;
        }
        // corners 0 and 2 are in both triangles, 1 and 3 in one
        assert!(seen.chunks(4).all(|q| q == [2, 1, 2, 1]));
    }
}
