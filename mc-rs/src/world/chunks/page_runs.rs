//! Solid draws grouped by vertex arena page within distance bands.
//!
//! From Starling's `gfx/page_runs.rs` (`THIRD_PARTY_NOTICES.md`). mc-rs pages are 12 MiB (`PAGE_VERTS`), so the ring spreads over a few of
//! them, and a list sorted only by distance switches page (`set_vertex_buffer`) every few chunks. Grouping the chunks of one band of
//! distance by page keeps the list front to back to within the band (what the depth test needs to reject hidden fragments early) with one
//! vertex buffer switch per page and band.
//!
//! The picture does not change: solid draws write depth and are tested against it, so their order only decides which of two fragments at
//! the same depth stays, and chunks do not share faces.

/// The width of a band, in blocks.
const BAND: f64 = 64.0;

/// One chunk's draws: `start..start + len` of the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Run {
    key: u64,
    start: u32,
    len: u32,
}

#[derive(Default)]
pub(super) struct PageRuns {
    runs: Vec<Run>,
}

impl PageRuns {
    /// The draws `start..start + len` of a chunk `distance` from the camera (squared, as bits of an `f64`) in arena `page`; runs come
    /// nearest first.
    pub fn push(&mut self, distance: u64, page: u32, start: usize, len: usize) {
        if len > 0 {
            let band = (f64::from_bits(distance).sqrt() / BAND) as u64;
            self.runs.push(Run { key: band << 32 | u64::from(page), start: start as u32, len: len as u32 });
        }
    }

    /// Reorders `draws[base..]`, which the pushed runs cover in order, so that each band's runs in one page are adjacent (in distance
    /// order).
    /// Changed from Starling: `draws` is one slice of whatever a draw is (Starling's are two parallel arrays, the draws and their pages),
    /// and the runs are not handed back: the draws are all the caller reads.
    pub fn regroup<T: Copy>(&mut self, draws: &mut [T], base: usize) {
        let page = |r: &Run| r.key as u32;
        if self.runs.windows(2).all(|w| page(&w[0]) == page(&w[1])) {
            return;
        }
        // Stable: within a band and page the runs stay nearest first.
        self.runs.sort_by_key(|r| r.key);
        let mut scratch: Vec<T> = Vec::with_capacity(draws.len() - base);
        for r in &self.runs {
            scratch.extend_from_slice(&draws[r.start as usize..(r.start + r.len) as usize]);
        }
        draws[base..].copy_from_slice(&scratch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(blocks: f64) -> u64 {
        (blocks * blocks).to_bits()
    }

    /// Runs of (distance in blocks, page, draws), as the list builder pushes them: a draw is (run number, its number in the run, page).
    fn list(runs: &[(f64, u32, u32)]) -> (PageRuns, Vec<(u32, u32, u32)>) {
        let mut grouped = PageRuns::default();
        let mut draws = Vec::new();
        for (n, &(distance, page, count)) in runs.iter().enumerate() {
            let start = draws.len();
            for k in 0..count {
                draws.push((n as u32, k, page));
            }
            grouped.push(bits(distance), page, start, count as usize);
        }
        (grouped, draws)
    }

    #[test]
    fn a_band_draws_each_page_once_keeping_the_order_inside_it() {
        let (mut grouped, mut draws) = list(&[(10.0, 1, 2), (20.0, 0, 1), (30.0, 1, 3), (40.0, 0, 2), (70.0, 1, 1)]);
        grouped.regroup(&mut draws, 0);
        let order: Vec<_> = draws.iter().map(|d| (d.0, d.1)).collect();
        // Band 0 (under 64 blocks): page 0 (runs 1, 3), then page 1 (runs 0, 2); band 1: run 4.
        assert_eq!(order, [(1, 0), (3, 0), (3, 1), (0, 0), (0, 1), (2, 0), (2, 1), (2, 2), (4, 0)]);
        let pages: Vec<_> = draws.iter().map(|d| d.2).collect();
        assert_eq!(pages, [0, 0, 0, 1, 1, 1, 1, 1, 1]);
    }

    #[test]
    fn bands_stay_in_distance_order() {
        let (mut grouped, mut draws) = list(&[(10.0, 1, 1), (100.0, 0, 1), (200.0, 0, 1)]);
        grouped.regroup(&mut draws, 0);
        let order: Vec<_> = draws.iter().map(|d| d.0).collect();
        assert_eq!(order, [0, 1, 2]);
    }

    #[test]
    fn one_page_and_an_offset_list() {
        // One page: nothing moves.
        let (mut grouped, mut draws) = list(&[(10.0, 3, 2), (90.0, 3, 1)]);
        let before = draws.clone();
        grouped.regroup(&mut draws, 0);
        assert_eq!(draws, before);
        // A list after other draws: starts are absolute, `base` is where the grouped part begins.
        let mut grouped = PageRuns::default();
        let mut draws = vec![(9, 0, 5), (7, 0, 1), (8, 0, 0)];
        grouped.push(bits(5.0), 1, 1, 1);
        grouped.push(bits(6.0), 0, 2, 1);
        grouped.regroup(&mut draws, 1);
        assert_eq!(draws, [(9, 0, 5), (8, 0, 0), (7, 0, 1)]);
        // Empty runs are left out.
        let mut grouped = PageRuns::default();
        grouped.push(bits(1.0), 0, 0, 0);
        grouped.regroup(&mut [(0u32, 0u32, 0u32); 0], 0);
    }
}
