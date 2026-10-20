//! `GlBufferArena`'s bookkeeping: one big GPU buffer carved into variable-size runs of vertices (or indices), so a region
//! of sections is a handful of buffers instead of one per section. This holds only the allocator, in element units (the
//! host multiplies by its stride): the host does the buffer creation, `write_buffer` uploads and copies, so nothing here
//! needs persistent mapping or multi-draw, and it works the same on Vulkan and GLES.
//!
//! Handles are element offsets; they stay valid until `grow` (the only call that moves anything) returns the moves.
//!
//! ponytail: the segments live in a sorted `Vec`, so `alloc` / `free` are O(segments); a region holds a few thousand at most.

#[derive(Clone, Copy, Debug)]
struct Seg {
    off: u32,
    len: u32,
    free: bool,
}

/// A run that `grow` moved: copy `len` elements from `from` in the old buffer to `to` in the new one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Move {
    pub from: u32,
    pub to: u32,
    pub len: u32,
}

pub struct Arena {
    segs: Vec<Seg>,
    capacity: u32,
    used: u32,
    /// Hard ceiling (elements), from the device's `max_buffer_size`: phones report far less than desktops.
    limit: u32,
}

impl Arena {
    pub fn new(capacity: u32, limit: u32) -> Self {
        assert!(capacity <= limit);
        Self { segs: vec![Seg { off: 0, len: capacity, free: true }], capacity, used: 0, limit }
    }

    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    pub fn used(&self) -> u32 {
        self.used
    }

    pub fn is_empty(&self) -> bool {
        self.used == 0
    }

    /// Best fit; the run is cut from the end of the free segment. `None`: no free segment is big enough (`grow`, then retry).
    pub fn alloc(&mut self, len: u32) -> Option<u32> {
        let mut best: Option<usize> = None;
        for (i, s) in self.segs.iter().enumerate() {
            if s.free && s.len >= len && best.map_or(true, |b| s.len < self.segs[b].len) {
                best = Some(i);
                if s.len == len {
                    break;
                }
            }
        }
        let i = best?;
        self.used += len;
        if self.segs[i].len == len {
            self.segs[i].free = false;
            return Some(self.segs[i].off);
        }
        self.segs[i].len -= len;
        let off = self.segs[i].off + self.segs[i].len;
        self.segs.insert(i + 1, Seg { off, len, free: false });
        Some(off)
    }

    /// Gives the run at `off` back and merges it with free neighbours. Panics on a handle that is not live.
    pub fn free(&mut self, off: u32) {
        let i = self.segs.binary_search_by_key(&off, |s| s.off).expect("free: not an allocation");
        assert!(!self.segs[i].free, "free: already freed");
        self.segs[i].free = true;
        self.used -= self.segs[i].len;
        if i + 1 < self.segs.len() && self.segs[i + 1].free {
            self.segs[i].len += self.segs[i + 1].len;
            self.segs.remove(i + 1);
        }
        if i > 0 && self.segs[i - 1].free {
            self.segs[i - 1].len += self.segs[i].len;
            self.segs.remove(i);
        }
    }

    /// Makes room for `needed` more elements than are free now (`ensureCapacity`): a bigger buffer with the live runs
    /// packed at its end, the free space in one piece in front. Grows by half again when that is more than needed.
    /// The host allocates a buffer of the new `capacity`, copies each `Move` (see `coalesce`), swaps buffers and re-reads its
    /// handles through the moves. `None` when it would pass the limit; nothing changed then (the host starts another arena).
    pub fn grow(&mut self, needed: u32) -> Option<Vec<Move>> {
        let want = needed.saturating_sub(self.capacity - self.used);
        let cap = (self.capacity as u64 + (self.capacity / 2).max(want) as u64).max(self.capacity as u64 + want as u64);
        if cap > self.limit as u64 {
            return None;
        }
        let cap = cap as u32;
        let mut moves = Vec::new();
        let mut segs = Vec::with_capacity(self.segs.len());
        let mut at = cap - self.used;
        if at > 0 {
            segs.push(Seg { off: 0, len: at, free: true });
        }
        for s in self.segs.iter().filter(|s| !s.free) {
            moves.push(Move { from: s.off, to: at, len: s.len });
            segs.push(Seg { off: at, len: s.len, free: false });
            at += s.len;
        }
        self.segs = segs;
        self.capacity = cap;
        Some(moves)
    }

    /// The moves as the fewest copies: neighbours that are contiguous in both buffers become one.
    pub fn coalesce(moves: &[Move]) -> Vec<Move> {
        let mut out: Vec<Move> = Vec::new();
        for &m in moves {
            match out.last_mut() {
                Some(l) if l.from + l.len == m.from && l.to + l.len == m.to => l.len += m.len,
                _ => out.push(m),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn best_fit_and_coalescing() {
        let mut a = Arena::new(100, 1000);
        let x = a.alloc(30).unwrap(); // 70..100
        let y = a.alloc(30).unwrap(); // 40..70
        let z = a.alloc(30).unwrap(); // 10..40
        assert_eq!((x, y, z), (70, 40, 10));
        assert_eq!(a.alloc(11), None); // only 10 free
        a.free(y);
        a.free(x); // merges with y's hole: 40..100 free
        assert_eq!(a.alloc(60), Some(40));
        a.free(z);
        a.free(40);
        assert!(a.is_empty());
        assert_eq!(a.alloc(100), Some(0)); // everything merged back into one run
    }

    /// Growing packs the live runs to the end and their contents follow the moves.
    #[test]
    fn grow_keeps_contents() {
        let mut a = Arena::new(40, 1000);
        let mut mem: Vec<u32> = vec![0; 40];
        let mut live = Vec::new();
        for (n, len) in [10u32, 5, 10, 5].into_iter().enumerate() {
            let off = a.alloc(len).unwrap();
            (off..off + len).for_each(|i| mem[i as usize] = n as u32 + 1);
            live.push((off, len, n as u32 + 1));
        }
        a.free(live[1].0);
        live.remove(1);
        let moves = a.grow(30).unwrap();
        assert_eq!(a.capacity(), 60); // 40 + max(40 / 2, 30 - 15 free)
        let mut grown = vec![0u32; a.capacity() as usize];
        for m in Arena::coalesce(&moves) {
            grown[m.to as usize..(m.to + m.len) as usize].copy_from_slice(&mem[m.from as usize..(m.from + m.len) as usize]);
        }
        for (off, len, tag) in live {
            let m = moves.iter().find(|m| m.from == off).unwrap();
            assert!(grown[m.to as usize..(m.to + len) as usize].iter().all(|&v| v == tag));
        }
        assert!(a.alloc(30).is_some()); // the room asked for is there, in one piece
        assert_eq!(a.used(), 25 + 30);
    }

    #[test]
    fn grow_respects_the_limit() {
        let mut a = Arena::new(10, 12);
        a.alloc(10).unwrap();
        assert!(a.grow(5).is_none());
        assert_eq!(a.capacity(), 10);
    }
}
