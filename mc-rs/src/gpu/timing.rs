//! GPU time of the scene pass, from timestamp queries, read back a few frames late so the CPU never waits for it.
//!
//! Port of `gfx/timing.rs` of Horizon (MIT, ludovic111/horizon), cut down for this renderer:
//! - Horizon times a whole frame with extra "marker" passes where the GPU only stamps pass boundaries (Metal). Here the stamps ride on
//!   the scene pass itself (`RenderPassDescriptor::timestamp_writes`), so no pass is added: on a tile-based phone GPU a pass that
//!   loads and stores the swap-chain image would cost more than it measures. Only `Features::TIMESTAMP_QUERY` is needed, not
//!   `TIMESTAMP_QUERY_INSIDE_ENCODERS`.
//! - Only the scene pass is timed, on purpose: it is the part that scales with the dynamic-resolution scale (`gpu::context::Drs`),
//!   and the final pass, which writes the swap-chain image, can stall on the presentation semaphore under vsync and would show
//!   that wait as GPU work.
//!
//! One frame: `begin` picks a free slot, `writes` goes into the scene pass, `resolve` copies the two stamps out after the pass,
//! `submitted` starts the read-back once the encoder is submitted, `collect` (any later frame) returns the finished ones in ms.
//! A frame whose slot is still busy simply goes untimed.

use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};

/// Frames in flight at once: a read-back lands two or three frames after its submit.
const SLOTS: usize = 6;
/// Each slot's two stamps sit at their own aligned offset of the resolve buffer.
const STRIDE: u64 = wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT;
const FREE: u8 = 0;
/// Resolved and submitted, the map request is out.
const PENDING: u8 = 1;
/// The map request finished: `collect` may read it.
const MAPPED: u8 = 2;

struct Slot {
    buffer: wgpu::Buffer,
    state: Arc<AtomicU8>,
}

pub struct GpuTimer {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    slots: Vec<Slot>,
    next: usize,
    period_ns: f32,
}

impl GpuTimer {
    /// The feature timing uses, if the adapter has it (empty: no timing). Ask for this in `request_device`.
    pub fn features(adapter: &wgpu::Adapter) -> wgpu::Features {
        adapter.features() & wgpu::Features::TIMESTAMP_QUERY
    }

    /// `None` when the device was made without the feature or the queue reports no timestamp period.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let period_ns = queue.get_timestamp_period();
        if !(period_ns > 0.0) {
            return None;
        }
        let queries = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("scene timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: 2 * SLOTS as u32,
        });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene timestamp resolve"),
            size: STRIDE * SLOTS as u64,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let slots = (0..SLOTS)
            .map(|_| Slot {
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("scene timestamp read-back"),
                    size: 16,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                state: Arc::new(AtomicU8::new(FREE)),
            })
            .collect();
        Some(Self { queries, resolve, slots, next: 0, period_ns })
    }

    /// Start timing this frame: the slot to pass to `writes`, `resolve` and `submitted`, or `None` when it is still busy.
    pub fn begin(&mut self) -> Option<usize> {
        (self.slots[self.next].state.load(Ordering::Acquire) == FREE).then_some(self.next)
    }

    /// The scene pass's `timestamp_writes`: its start and its end.
    pub fn writes(&self, slot: usize) -> wgpu::RenderPassTimestampWrites<'_> {
        wgpu::RenderPassTimestampWrites {
            query_set: &self.queries,
            beginning_of_pass_write_index: Some(2 * slot as u32),
            end_of_pass_write_index: Some(2 * slot as u32 + 1),
        }
    }

    /// After the timed pass has ended and before `encoder.finish()`: copy its two stamps to the slot's read-back buffer.
    pub fn resolve(&self, encoder: &mut wgpu::CommandEncoder, slot: usize) {
        let (first, at) = (2 * slot as u32, STRIDE * slot as u64);
        encoder.resolve_query_set(&self.queries, first..first + 2, &self.resolve, at);
        encoder.copy_buffer_to_buffer(&self.resolve, at, &self.slots[slot].buffer, 0, 16);
    }

    /// After `queue.submit`: ask for the read-back. The slot stays busy until `collect` has read it.
    pub fn submitted(&mut self, slot: usize) {
        let state = self.slots[slot].state.clone();
        state.store(PENDING, Ordering::Release);
        self.slots[slot].buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            state.store(if r.is_ok() { MAPPED } else { FREE }, Ordering::Release);
        });
        self.next = (slot + 1) % SLOTS;
    }

    /// Scene-pass GPU milliseconds of the frames whose stamps have arrived, oldest first, pushed onto `out`. Never blocks.
    pub fn collect(&mut self, device: &wgpu::Device, out: &mut Vec<f32>) {
        let _ = device.poll(wgpu::PollType::Poll);
        for i in 0..SLOTS {
            let slot = &self.slots[(self.next + i) % SLOTS];
            if slot.state.load(Ordering::Acquire) != MAPPED {
                continue;
            }
            {
                let data = slot.buffer.slice(..).get_mapped_range();
                // Unaligned read: the mapping is not promised to be 8-byte aligned.
                let stamps: [u64; 2] = bytemuck::pod_read_unaligned(&data[..16]);
                if let Some(ms) = ticks_to_ms(stamps, self.period_ns) {
                    out.push(ms);
                }
            }
            slot.buffer.unmap();
            slot.state.store(FREE, Ordering::Release);
        }
    }
}

/// Two stamps (start, end) in timestamp ticks to milliseconds. `None` when the end is before the start (a counter that wrapped or
/// a driver that stamped out of order): that frame goes untimed rather than becoming a huge number.
fn ticks_to_ms([start, end]: [u64; 2], period_ns: f32) -> Option<f32> {
    end.checked_sub(start).map(|t| (t as f64 * f64::from(period_ns) / 1e6) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_become_milliseconds_and_a_backwards_pair_is_dropped() {
        // 52 ns per tick (a typical Mali period): 250 000 ticks = 13 ms.
        let ms = ticks_to_ms([1_000, 251_000], 52.0).unwrap();
        assert!((ms - 13.0).abs() < 1e-4, "{ms}");
        assert_eq!(ticks_to_ms([5, 5], 52.0), Some(0.0));
        assert_eq!(ticks_to_ms([10, 9], 52.0), None);
        // A counter near u64::MAX does not overflow the maths.
        assert!(ticks_to_ms([u64::MAX - 10, u64::MAX], 1.0).is_some());
    }
}
