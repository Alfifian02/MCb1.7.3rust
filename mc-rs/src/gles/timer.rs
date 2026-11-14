//! The scene pass's GPU time on the OpenGL ES renderer: the twin of `gpu::timing` (wgpu timestamps), read from `GL_EXT_disjoint_timer_query`.
//! A query brackets the pass; its result is read a few frames later and only when the driver says it is ready, so the CPU never waits for it
//! (`Backend::gpu_ms` hands it to `Drs::tick` and to the debug screen's `gpu` figure). Until this existed that figure was blank on this
//! renderer and the dynamic resolution judged the frame by its swap wait, which vsync pads. Nothing here runs on a context without the extension.

use super::sys::*;

/// Queries in flight at once: a result lands two or three frames after its pass, so spare slots keep every frame timed.
const SLOTS: usize = 6;

pub struct GpuTimer {
    ids: [Uint; SLOTS],
    /// Per slot: a query was ended and its result is not read yet.
    pending: [bool; SLOTS],
    /// The slot the next pass uses (the oldest in the ring).
    next: usize,
    /// A query is open (between `begin` and `end`).
    open: bool,
    result: QueryU64,
}

/// A query result in milliseconds. `None` for a disjoint period (the GPU clock changed or the context was lost: the number means nothing)
/// and for a zero or absurd one (a driver that does not fill it in).
fn millis(ns: u64, disjoint: bool) -> Option<f32> {
    (!disjoint && ns > 0 && ns < 1_000_000_000).then(|| ns as f32 / 1.0e6)
}

impl GpuTimer {
    pub fn new(f: &Fns, result: QueryU64) -> Self {
        let mut ids = [0; SLOTS];
        // SAFETY: fresh query names of the current context.
        unsafe { (f.GenQueries)(SLOTS as i32, ids.as_mut_ptr()) };
        Self { ids, pending: [false; SLOTS], next: 0, open: false, result }
    }

    /// Opens the query of the pass about to be drawn. A slot whose result is still unread is not reused: that frame goes untimed.
    pub fn begin(&mut self, f: &Fns) {
        if self.pending[self.next] {
            return;
        }
        // SAFETY: a query of this context that is not in use.
        unsafe { (f.BeginQuery)(TIME_ELAPSED_EXT, self.ids[self.next]) };
        self.open = true;
    }

    /// Closes the query `begin` opened (nothing when it opened none).
    pub fn end(&mut self, f: &Fns) {
        if !self.open {
            return;
        }
        // SAFETY: closes the open `TIME_ELAPSED_EXT` query.
        unsafe { (f.EndQuery)(TIME_ELAPSED_EXT) };
        (self.pending[self.next], self.open) = (true, false);
        self.next = (self.next + 1) % SLOTS;
    }

    /// Pushes the milliseconds of every ended query whose result is ready, oldest first. Never waits.
    pub fn collect(&mut self, f: &Fns, out: &mut Vec<f32>) {
        let mut disjoint = 0;
        // SAFETY: reading the flag clears it; what finished before a disjoint event is dropped below.
        unsafe { (f.GetIntegerv)(GPU_DISJOINT_EXT, &mut disjoint) };
        for k in 0..SLOTS {
            let s = (self.next + k) % SLOTS;
            if !self.pending[s] {
                continue;
            }
            let mut ready = 0;
            // SAFETY: an ended query of this context.
            unsafe { (f.GetQueryObjectuiv)(self.ids[s], QUERY_RESULT_AVAILABLE, &mut ready) };
            if ready == 0 {
                continue;
            }
            let mut ns = 0u64;
            // SAFETY: the same query, whose result is ready, so the read does not wait.
            unsafe { (self.result)(self.ids[s], QUERY_RESULT, &mut ns) };
            self.pending[s] = false;
            out.extend(millis(ns, disjoint != 0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_result_counts_only_when_it_is_whole_and_believable() {
        assert_eq!(millis(8_500_000, false), Some(8.5));
        assert_eq!(millis(8_500_000, true), None, "a disjoint period");
        assert_eq!(millis(0, false), None, "a driver that does not fill it in");
        assert_eq!(millis(5_000_000_000, false), None, "five seconds is not a frame");
    }
}
