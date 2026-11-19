#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#include <errno.h>
#include <sched.h>
#include <stdatomic.h>
#include <stdint.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <sys/syscall.h>
#include "hires_runtime.h"

/*
 * Scheduling support for the Android front end (roadmap stages A2 and A3).
 *
 *  - Thread registry: the emulation thread, the 3D raster workers and the 2D workers register
 *    themselves so the app can hand their TIDs to ADPF (PerformanceHintManager) or pin them.
 *  - Affinity: an optional CPU mask applied to registered threads (also to threads that register later).
 *  - 2D worker count: the app computes a big-core-aware count and the core reads it through
 *    nds_configured_2d_threads() (debug/control.c).
 *  - Frame work time: wall time of a frame excluding the pacer's sleep, which is the "actual work
 *    duration" ADPF wants. It is measured around pacer_run(), once per emulated frame.
 *
 * Every entry point is wait-free for the emulation thread: it only touches atomics.
 */

#define PERF_MAX_THREADS 32u
#define PERF_GAP_NS 500000000ull

typedef struct {
    _Atomic int32_t tid;
    _Atomic uint32_t role;
} perf_slot_t;

static perf_slot_t perf_slots[PERF_MAX_THREADS];
static _Atomic uint64_t perf_affinity;
static _Atomic unsigned perf_2d_threads = ~0u;

static _Atomic uint64_t perf_last_exit_ns;
static _Atomic uint32_t perf_work_us;
static _Atomic uint32_t perf_work_seq;

static int32_t perf_gettid(void)
{
    return (int32_t)syscall(SYS_gettid);
}

static uint64_t perf_now_ns(void)
{
    struct timespec ts;

    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0)
        return 0;
    return (uint64_t)ts.tv_sec * 1000000000ull + (uint64_t)ts.tv_nsec;
}

static int perf_alive(int32_t tid)
{
    char path[48];
    char *p = path;
    static const char prefix[] = "/proc/self/task/";
    unsigned v = (unsigned)tid;
    char digits[12];
    unsigned n = 0;

    memcpy(p, prefix, sizeof prefix - 1u);
    p += sizeof prefix - 1u;
    do { digits[n++] = (char)('0' + v % 10u); v /= 10u; } while (v != 0u && n < sizeof digits);
    while (n != 0u) *p++ = digits[--n];
    *p = 0;
    return access(path, F_OK) == 0;
}

int recon_perf_pin_thread(int32_t tid, uint64_t mask)
{
    cpu_set_t set;
    unsigned cpu;
    unsigned count = 0;

    CPU_ZERO(&set);
    if (mask == 0u) {
        /* Restore "any CPU that is online". */
        long online = sysconf(_SC_NPROCESSORS_CONF);
        if (online < 1) online = 1;
        if (online > 64) online = 64;
        for (cpu = 0; cpu < (unsigned)online; cpu++) { CPU_SET(cpu, &set); count++; }
    } else {
        for (cpu = 0; cpu < 64u; cpu++)
            if (mask & (1ull << cpu)) { CPU_SET(cpu, &set); count++; }
    }
    if (count == 0u)
        return -1;
    return sched_setaffinity((pid_t)tid, sizeof set, &set) == 0 ? 0 : -1;
}

void recon_perf_register_thread(unsigned role)
{
    int32_t self = perf_gettid();
    unsigned i;
    unsigned free_slot = PERF_MAX_THREADS;
    uint64_t mask;

    if (self <= 0)
        return;
    for (i = 0; i < PERF_MAX_THREADS; i++) {
        int32_t t = atomic_load_explicit(&perf_slots[i].tid, memory_order_acquire);

        if (t == self) {
            atomic_fetch_or_explicit(&perf_slots[i].role, role, memory_order_relaxed);
            goto pin;
        }
        if (free_slot == PERF_MAX_THREADS && (t == 0 || !perf_alive(t)))
            free_slot = i;
    }
    if (free_slot == PERF_MAX_THREADS)
        return;
    atomic_store_explicit(&perf_slots[free_slot].role, role, memory_order_relaxed);
    atomic_store_explicit(&perf_slots[free_slot].tid, self, memory_order_release);
pin:
    mask = atomic_load_explicit(&perf_affinity, memory_order_relaxed);
    if (mask != 0u)
        (void)recon_perf_pin_thread(self, mask);
}

unsigned recon_perf_thread_ids(int32_t *out, unsigned max, unsigned roles_mask)
{
    unsigned i;
    unsigned n = 0;

    for (i = 0; i < PERF_MAX_THREADS && n < max; i++) {
        int32_t t = atomic_load_explicit(&perf_slots[i].tid, memory_order_acquire);

        if (t == 0)
            continue;
        if (!(atomic_load_explicit(&perf_slots[i].role, memory_order_relaxed) & roles_mask))
            continue;
        if (!perf_alive(t)) {
            atomic_store_explicit(&perf_slots[i].tid, 0, memory_order_release);
            continue;
        }
        out[n++] = t;
    }
    return n;
}

void recon_perf_set_affinity(uint64_t mask)
{
    unsigned i;

    atomic_store_explicit(&perf_affinity, mask, memory_order_relaxed);
    for (i = 0; i < PERF_MAX_THREADS; i++) {
        int32_t t = atomic_load_explicit(&perf_slots[i].tid, memory_order_acquire);

        if (t != 0 && perf_alive(t))
            (void)recon_perf_pin_thread(t, mask);
    }
}

void recon_perf_set_2d_threads(unsigned n)
{
    atomic_store_explicit(&perf_2d_threads, n, memory_order_release);
}

unsigned recon_perf_2d_threads(void)
{
    return atomic_load_explicit(&perf_2d_threads, memory_order_acquire);
}

/* Emulation thread, at the top of pacer_run(): everything since the previous exit was work. */
void recon_perf_pacer_enter(void)
{
    uint64_t now = perf_now_ns();
    uint64_t last = atomic_load_explicit(&perf_last_exit_ns, memory_order_relaxed);

    if (last != 0u && now > last && now - last < PERF_GAP_NS) {
        uint64_t us = (now - last) / 1000u;

        atomic_store_explicit(&perf_work_us, us > 0xffffffffull ? 0xffffffffu : (uint32_t)us,
                              memory_order_relaxed);
        atomic_fetch_add_explicit(&perf_work_seq, 1u, memory_order_release);
    }
}

/* Emulation thread, when pacer_run() returns (after any sleep). */
void recon_perf_pacer_exit(void)
{
    atomic_store_explicit(&perf_last_exit_ns, perf_now_ns(), memory_order_relaxed);
}

uint64_t recon_perf_last_work(void)
{
    uint32_t seq = atomic_load_explicit(&perf_work_seq, memory_order_acquire);
    uint32_t us = atomic_load_explicit(&perf_work_us, memory_order_relaxed);

    return ((uint64_t)seq << 32) | us;
}

/*
 * PGO support (roadmap stage A1). In a -fprofile-generate build the LLVM profile runtime is linked
 * into this library, but its symbols are hidden from the app's JNI library and the runtime only
 * writes at normal process exit, which an Android app rarely reaches. The app calls this when a
 * game closes. In any other build the weak symbols are null and nothing happens.
 */
extern int __llvm_profile_write_file(void) __attribute__((weak));
extern void __llvm_profile_set_filename(const char *name) __attribute__((weak));

int recon_profile_flush(const char *path)
{
    if (__llvm_profile_write_file == 0)
        return -1;
    if (path != 0 && __llvm_profile_set_filename != 0)
        __llvm_profile_set_filename(path);
    return __llvm_profile_write_file();
}
