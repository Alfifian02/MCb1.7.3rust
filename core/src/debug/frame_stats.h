#ifndef SEEDLESSDS_DEBUG_FRAME_STATS_H
#define SEEDLESSDS_DEBUG_FRAME_STATS_H

/*
 * Per-frame timing of the emulation thread.
 *
 * In SeedlessDS the 3D rasteriser runs on the CPU, so the split between CPU
 * time and GPU time decides where optimisation effort goes. This module is the
 * CPU half: it samples, once per emulated frame, the CPU time used by the
 * emulation thread, the CPU time used by the whole process (every thread,
 * including the raster workers), the wall-clock time between frames and the
 * time the emulation thread spent inside save-file ticks.
 *
 * Cost when disabled: one relaxed atomic load per frame. Nothing blocks and
 * nothing allocates. The GPU half lives in the render thread (FramePerf.kt).
 */

#include <stdint.h>

#define FRAME_STATS_RING 600u

typedef struct frame_stats_snapshot {
    uint32_t samples;
    uint32_t emu_cpu_avg_us;
    uint32_t emu_cpu_p99_us;
    uint32_t emu_cpu_max_us;
    uint32_t proc_cpu_avg_us;
    uint32_t proc_cpu_p99_us;
    uint32_t wall_avg_us;
    uint32_t wall_p99_us;
    uint32_t wall_max_us;
    uint32_t io_avg_us;
    uint32_t io_max_us;
    uint32_t io_frames;
} frame_stats_snapshot_t;

void frame_stats_enable(int enabled);
int frame_stats_enabled(void);

/* Emulation thread, once per frame, at frame close. */
void frame_stats_frame_end(void);

/* Emulation thread. frame_stats_mark() returns 0 while disabled. */
uint64_t frame_stats_mark(void);
void frame_stats_io_since(uint64_t mark);

/* Any thread. Returns the number of samples in the window. */
uint32_t frame_stats_snapshot(frame_stats_snapshot_t *out);

#endif
