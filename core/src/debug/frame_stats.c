#include "debug/frame_stats.h"

#include <pthread.h>
#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

/* A gap longer than this is a pause, a menu or a state load, not a frame. */
#define FRAME_STATS_GAP_US 500000u

static _Atomic int fs_enabled;
static _Atomic uint32_t fs_count;

static _Atomic uint32_t fs_emu_cpu[FRAME_STATS_RING];
static _Atomic uint32_t fs_proc_cpu[FRAME_STATS_RING];
static _Atomic uint32_t fs_wall[FRAME_STATS_RING];
static _Atomic uint32_t fs_io[FRAME_STATS_RING];

/* Written and read only by the emulation thread. */
static uint64_t fs_prev_emu_ns;
static uint64_t fs_prev_proc_ns;
static uint64_t fs_prev_wall_ns;
static _Atomic uint64_t fs_io_acc_us;
static pthread_t fs_thread;
static int fs_primed;

static uint64_t clock_ns(clockid_t id)
{
    struct timespec ts;

    if (clock_gettime(id, &ts) != 0)
        return 0;
    return (uint64_t)ts.tv_sec * 1000000000ull + (uint64_t)ts.tv_nsec;
}

static uint32_t clamp_u32(uint64_t v)
{
    return v > 0xffffffffull ? 0xffffffffu : (uint32_t)v;
}

void frame_stats_enable(int enabled)
{
    int was = atomic_exchange_explicit(&fs_enabled, enabled != 0, memory_order_relaxed);

    if (enabled != 0 && !was) {
        atomic_store_explicit(&fs_count, 0, memory_order_relaxed);
        /* The emulation thread re-primes on its next frame. */
        fs_primed = 0;
    }
}

int frame_stats_enabled(void)
{
    return atomic_load_explicit(&fs_enabled, memory_order_relaxed);
}

uint64_t frame_stats_mark(void)
{
    if (!atomic_load_explicit(&fs_enabled, memory_order_relaxed))
        return 0;
    return clock_ns(CLOCK_MONOTONIC);
}

void frame_stats_io_since(uint64_t mark)
{
    uint64_t now;

    if (mark == 0)
        return;
    now = clock_ns(CLOCK_MONOTONIC);
    if (now > mark)
        atomic_fetch_add_explicit(&fs_io_acc_us, (now - mark) / 1000u, memory_order_relaxed);
}

void frame_stats_frame_end(void)
{
    uint64_t emu;
    uint64_t proc;
    uint64_t wall;
    uint64_t d_emu;
    uint64_t d_proc;
    uint64_t d_wall;
    uint64_t io_us;
    uint32_t slot;
    pthread_t self;

    if (!atomic_load_explicit(&fs_enabled, memory_order_relaxed))
        return;

    self = pthread_self();
    emu = clock_ns(CLOCK_THREAD_CPUTIME_ID);
    proc = clock_ns(CLOCK_PROCESS_CPUTIME_ID);
    wall = clock_ns(CLOCK_MONOTONIC);

    /* Thread CPU clocks are per thread: a different caller invalidates the delta. */
    if (!fs_primed || !pthread_equal(self, fs_thread)) {
        fs_thread = self;
        fs_primed = 1;
        goto rebase;
    }

    d_emu = emu - fs_prev_emu_ns;
    d_proc = proc - fs_prev_proc_ns;
    d_wall = wall - fs_prev_wall_ns;

    if (d_wall / 1000u > FRAME_STATS_GAP_US)
        goto rebase;

    io_us = atomic_exchange_explicit(&fs_io_acc_us, 0, memory_order_relaxed);
    slot = atomic_load_explicit(&fs_count, memory_order_relaxed) % FRAME_STATS_RING;
    atomic_store_explicit(&fs_emu_cpu[slot], clamp_u32(d_emu / 1000u), memory_order_relaxed);
    atomic_store_explicit(&fs_proc_cpu[slot], clamp_u32(d_proc / 1000u), memory_order_relaxed);
    atomic_store_explicit(&fs_wall[slot], clamp_u32(d_wall / 1000u), memory_order_relaxed);
    atomic_store_explicit(&fs_io[slot], clamp_u32(io_us), memory_order_relaxed);
    atomic_fetch_add_explicit(&fs_count, 1, memory_order_release);

rebase:
    atomic_store_explicit(&fs_io_acc_us, 0, memory_order_relaxed);
    fs_prev_emu_ns = emu;
    fs_prev_proc_ns = proc;
    fs_prev_wall_ns = wall;
}

static int cmp_u32(const void *a, const void *b)
{
    uint32_t x = *(const uint32_t *)a;
    uint32_t y = *(const uint32_t *)b;

    return (x > y) - (x < y);
}

typedef struct series {
    uint32_t avg;
    uint32_t p99;
    uint32_t max;
} series_t;

static series_t reduce(const _Atomic uint32_t *ring, uint32_t n, uint32_t *scratch)
{
    series_t r = {0, 0, 0};
    uint64_t sum = 0;
    uint32_t i;

    if (n == 0)
        return r;
    for (i = 0; i < n; i++) {
        scratch[i] = atomic_load_explicit(&ring[i], memory_order_relaxed);
        sum += scratch[i];
    }
    qsort(scratch, n, sizeof scratch[0], cmp_u32);
    r.avg = (uint32_t)(sum / n);
    r.max = scratch[n - 1];
    r.p99 = scratch[(n * 99u + 99u) / 100u - 1u];
    return r;
}

uint32_t frame_stats_snapshot(frame_stats_snapshot_t *out)
{
    uint32_t scratch[FRAME_STATS_RING];
    uint32_t total;
    uint32_t n;
    uint32_t i;
    uint32_t io_frames = 0;
    uint64_t io_sum = 0;
    uint32_t io_max = 0;
    series_t s;

    if (out == NULL)
        return 0;
    memset(out, 0, sizeof *out);

    total = atomic_load_explicit(&fs_count, memory_order_acquire);
    n = total < FRAME_STATS_RING ? total : FRAME_STATS_RING;
    out->samples = n;
    if (n == 0)
        return 0;

    s = reduce(fs_emu_cpu, n, scratch);
    out->emu_cpu_avg_us = s.avg;
    out->emu_cpu_p99_us = s.p99;
    out->emu_cpu_max_us = s.max;

    s = reduce(fs_proc_cpu, n, scratch);
    out->proc_cpu_avg_us = s.avg;
    out->proc_cpu_p99_us = s.p99;

    s = reduce(fs_wall, n, scratch);
    out->wall_avg_us = s.avg;
    out->wall_p99_us = s.p99;
    out->wall_max_us = s.max;

    for (i = 0; i < n; i++) {
        uint32_t v = atomic_load_explicit(&fs_io[i], memory_order_relaxed);

        io_sum += v;
        if (v > io_max)
            io_max = v;
        if (v != 0)
            io_frames++;
    }
    out->io_avg_us = (uint32_t)(io_sum / n);
    out->io_max_us = io_max;
    out->io_frames = io_frames;
    return n;
}
