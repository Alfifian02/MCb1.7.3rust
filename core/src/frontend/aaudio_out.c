#include <dlfcn.h>
#include <math.h>
#include <stdatomic.h>
#include <stdint.h>
#include <string.h>
#include "hires_runtime.h"
#include "platform/android/audio.h"
#include "frontend/aaudio_out.h"

/*
 * Optional AAudio output (roadmap stage A5). OpenSL ES stays the default and the fallback.
 *
 * Why this is a side path and not a replacement: the OpenSL engine is also what the microphone
 * recorder is created from, and the rest of the audio code (audio_out.c, nds.c) drives its player
 * through vtable calls. So the OpenSL engine and player are still created; when AAudio opens, the
 * OpenSL player is stopped and the mixed audio goes to a ring buffer that AAudio's data callback
 * drains. If AAudio is unavailable, or fails later, the OpenSL player is started again.
 *
 * libaaudio.so is loaded with dlopen so the app keeps minSdk 21. Only API 28+ is accepted, because
 * from there on the system converts 44.1 kHz to the device rate itself.
 *
 * Constants and signatures below are copied from <aaudio/AAudio.h>; no header is included on
 * purpose, since its functions are marked as API 26+ and would not compile against minSdk 21.
 */

typedef struct AAudioStreamStruct AAudioStream;
typedef struct AAudioStreamBuilderStruct AAudioStreamBuilder;
typedef int32_t aaudio_result_t;

enum {
    AAUDIO_OK = 0,
    AAUDIO_ERROR_DISCONNECTED = -899,
    AAUDIO_DIRECTION_OUTPUT = 0,
    AAUDIO_FORMAT_PCM_I16 = 1,
    AAUDIO_SHARING_MODE_SHARED = 1,
    AAUDIO_PERFORMANCE_MODE_LOW_LATENCY = 12,
    AAUDIO_USAGE_GAME = 14,
    AAUDIO_CALLBACK_RESULT_CONTINUE = 0,
    AAUDIO_STREAM_STATE_STARTED = 4,
};

typedef int32_t (*data_cb_t)(AAudioStream *, void *, void *, int32_t);
typedef void (*error_cb_t)(AAudioStream *, void *, aaudio_result_t);

typedef struct {
    aaudio_result_t (*create_builder)(AAudioStreamBuilder **);
    void (*set_direction)(AAudioStreamBuilder *, int32_t);
    void (*set_sharing)(AAudioStreamBuilder *, int32_t);
    void (*set_rate)(AAudioStreamBuilder *, int32_t);
    void (*set_channels)(AAudioStreamBuilder *, int32_t);
    void (*set_format)(AAudioStreamBuilder *, int32_t);
    void (*set_performance)(AAudioStreamBuilder *, int32_t);
    void (*set_usage)(AAudioStreamBuilder *, int32_t);
    void (*set_data_cb)(AAudioStreamBuilder *, data_cb_t, void *);
    void (*set_error_cb)(AAudioStreamBuilder *, error_cb_t, void *);
    aaudio_result_t (*open)(AAudioStreamBuilder *, AAudioStream **);
    aaudio_result_t (*delete_builder)(AAudioStreamBuilder *);
    aaudio_result_t (*request_start)(AAudioStream *);
    aaudio_result_t (*request_stop)(AAudioStream *);
    aaudio_result_t (*close)(AAudioStream *);
    int32_t (*get_rate)(AAudioStream *);
    int32_t (*get_channels)(AAudioStream *);
    int32_t (*get_format)(AAudioStream *);
    int32_t (*get_burst)(AAudioStream *);
    int32_t (*get_buffer_size)(AAudioStream *);
    aaudio_result_t (*set_buffer_size)(AAudioStream *, int32_t);
} aaudio_api_t;

/* One ring of interleaved stereo int16. The emulation thread writes, the audio callback reads. */
#define RING_SAMPLES 65536u
#define RING_MASK (RING_SAMPLES - 1u)

static aaudio_api_t api;
static int api_state;               /* 0 unknown, 1 loaded, -1 unavailable */
static int requested_backend;       /* recon_audio_backend_set() */
static AAudioStream *stream;
static _Atomic int running;         /* stream is started and the callback should consume */
static _Atomic int needs_reopen;    /* error callback saw a disconnect */
static _Atomic uint32_t ring_write;
static _Atomic uint32_t ring_read;
static _Atomic uint32_t limit_samples;
static _Atomic uint32_t gain_q15 = 32768u;
static int16_t ring[RING_SAMPLES];
static int active;                  /* AAudio owns playback (OpenSL player is stopped) */

void recon_audio_backend_set(int backend)
{
    requested_backend = backend;
}

static int load_api(void)
{
    void *lib;

    if (api_state != 0)
        return api_state > 0;
    api_state = -1;
    lib = dlopen("libaaudio.so", RTLD_NOW | RTLD_LOCAL);
    if (lib == NULL)
        return 0;
#define BIND(field, name) \
    do { *(void **)&api.field = dlsym(lib, name); if (api.field == NULL) return 0; } while (0)
    BIND(create_builder, "AAudio_createStreamBuilder");
    BIND(set_direction, "AAudioStreamBuilder_setDirection");
    BIND(set_sharing, "AAudioStreamBuilder_setSharingMode");
    BIND(set_rate, "AAudioStreamBuilder_setSampleRate");
    BIND(set_channels, "AAudioStreamBuilder_setChannelCount");
    BIND(set_format, "AAudioStreamBuilder_setFormat");
    BIND(set_performance, "AAudioStreamBuilder_setPerformanceMode");
    BIND(set_data_cb, "AAudioStreamBuilder_setDataCallback");
    BIND(set_error_cb, "AAudioStreamBuilder_setErrorCallback");
    BIND(open, "AAudioStreamBuilder_openStream");
    BIND(delete_builder, "AAudioStreamBuilder_delete");
    BIND(request_start, "AAudioStream_requestStart");
    BIND(request_stop, "AAudioStream_requestStop");
    BIND(close, "AAudioStream_close");
    BIND(get_rate, "AAudioStream_getSampleRate");
    BIND(get_channels, "AAudioStream_getChannelCount");
    BIND(get_format, "AAudioStream_getFormat");
    BIND(get_burst, "AAudioStream_getFramesPerBurst");
    BIND(get_buffer_size, "AAudioStream_getBufferSizeInFrames");
    BIND(set_buffer_size, "AAudioStream_setBufferSizeInFrames");
#undef BIND
    /* Usage is API 28; it is only a hint, so a missing symbol is not fatal. */
    *(void **)&api.set_usage = dlsym(lib, "AAudioStreamBuilder_setUsage");
    api_state = 1;
    return 1;
}

static int32_t data_callback(AAudioStream *s, void *user, void *audio, int32_t frames)
{
    int16_t *out = (int16_t *)audio;
    uint32_t want = (uint32_t)frames * 2u;
    uint32_t r = atomic_load_explicit(&ring_read, memory_order_relaxed);
    uint32_t w = atomic_load_explicit(&ring_write, memory_order_acquire);
    uint32_t have = w - r;
    uint32_t n = have < want ? have : want;
    uint32_t gain = atomic_load_explicit(&gain_q15, memory_order_relaxed);
    uint32_t i;

    (void)s;
    (void)user;
    if (!atomic_load_explicit(&running, memory_order_relaxed)) {
        memset(out, 0, (size_t)want * sizeof(int16_t));
        return AAUDIO_CALLBACK_RESULT_CONTINUE;
    }
    if (gain == 32768u) {
        for (i = 0; i < n; i++)
            out[i] = ring[(r + i) & RING_MASK];
    } else {
        for (i = 0; i < n; i++)
            out[i] = (int16_t)(((int32_t)ring[(r + i) & RING_MASK] * (int32_t)gain) >> 15);
    }
    if (n < want)
        memset(out + n, 0, (size_t)(want - n) * sizeof(int16_t));   /* underrun: silence */
    atomic_store_explicit(&ring_read, r + n, memory_order_release);

    /* Same bookkeeping as the OpenSL queue callback: samples played, whether or not they were silence. */
    __atomic_fetch_add(&PLATFORM_AUDIO->accumulated, (uint64_t)want, __ATOMIC_RELAXED);
    return AAUDIO_CALLBACK_RESULT_CONTINUE;
}

static void error_callback(AAudioStream *s, void *user, aaudio_result_t error)
{
    (void)s;
    (void)user;
    /* Reopening from inside this callback deadlocks; the emulation thread does it. */
    if (error == AAUDIO_ERROR_DISCONNECTED)
        atomic_store_explicit(&needs_reopen, 1, memory_order_release);
}

static void sl_play_state(unsigned state)
{
    platform_audio_t *g = PLATFORM_AUDIO;
    void *obj = g->play_itf;

    if (obj == NULL)
        return;
    {
        void **table = *(void ***)obj;
        ((long (*)(void *, unsigned))table[0])(obj, state);
    }
}

static void close_stream(void)
{
    AAudioStream *s = stream;

    stream = NULL;
    atomic_store_explicit(&running, 0, memory_order_release);
    if (s != NULL) {
        (void)api.request_stop(s);
        (void)api.close(s);
    }
}

static int open_stream(void)
{
    AAudioStreamBuilder *b = NULL;
    AAudioStream *s = NULL;
    aaudio_result_t r;
    int32_t burst;

    if (api.create_builder(&b) != AAUDIO_OK || b == NULL)
        return 0;
    api.set_direction(b, AAUDIO_DIRECTION_OUTPUT);
    api.set_sharing(b, AAUDIO_SHARING_MODE_SHARED);
    api.set_rate(b, 44100);
    api.set_channels(b, 2);
    api.set_format(b, AAUDIO_FORMAT_PCM_I16);
    api.set_performance(b, AAUDIO_PERFORMANCE_MODE_LOW_LATENCY);
    if (api.set_usage != NULL)
        api.set_usage(b, AAUDIO_USAGE_GAME);
    api.set_data_cb(b, data_callback, NULL);
    api.set_error_cb(b, error_callback, NULL);
    r = api.open(b, &s);
    (void)api.delete_builder(b);
    if (r != AAUDIO_OK || s == NULL)
        return 0;
    if (api.get_rate(s) != 44100 || api.get_channels(s) != 2 || api.get_format(s) != AAUDIO_FORMAT_PCM_I16) {
        (void)api.close(s);
        return 0;
    }
    /* Two bursts keeps latency low; the ring, not the device buffer, absorbs frame-time jitter. */
    burst = api.get_burst(s);
    if (burst > 0)
        (void)api.set_buffer_size(s, burst * 2);
    stream = s;
    return 1;
}

static void sync_limit(void)
{
    platform_audio_t *g = PLATFORM_AUDIO;
    uint32_t limit = g->buffer_count * g->buffer_samples;

    if (limit < 2u * PLATFORM_AUDIO_SAMPLES_PER_FRAME) limit = 2u * PLATFORM_AUDIO_SAMPLES_PER_FRAME;
    if (limit > RING_SAMPLES - 2u * PLATFORM_AUDIO_SAMPLES_PER_FRAME)
        limit = RING_SAMPLES - 2u * PLATFORM_AUDIO_SAMPLES_PER_FRAME;
    atomic_store_explicit(&limit_samples, limit, memory_order_relaxed);
}

static void reset_ring(void)
{
    uint32_t w = atomic_load_explicit(&ring_write, memory_order_relaxed);

    atomic_store_explicit(&ring_read, w, memory_order_release);
}

int aaudio_out_is_active(void)
{
    return active;
}

int aaudio_out_open(void)
{
    if (requested_backend != 1 || active)
        return active;
    if (!load_api() || !open_stream())
        return 0;
    sync_limit();
    reset_ring();
    atomic_store_explicit(&needs_reopen, 0, memory_order_relaxed);
    atomic_store_explicit(&running, 1, memory_order_release);
    if (api.request_start(stream) != AAUDIO_OK) {
        close_stream();
        return 0;
    }
    sl_play_state(1);   /* SL_PLAYSTATE_STOPPED: the OpenSL player stays idle while AAudio plays. */
    active = 1;
    return 1;
}

void aaudio_out_set_volume_mb(int16_t millibel)
{
    uint32_t gain;

    if (millibel <= -5000) {
        gain = 0u;                      /* 0x8000 (mute) and anything below -50 dB */
    } else if (millibel >= 0) {
        gain = 32768u;
    } else {
        /* audio_out_set_volume_db() stores 2000 * log10(level / 100), so the factor is 10^(mB / 2000). */
        gain = (uint32_t)(powf(10.0f, (float)millibel / 2000.0f) * 32768.0f + 0.5f);
        if (gain > 32768u) gain = 32768u;
    }
    atomic_store_explicit(&gain_q15, gain, memory_order_relaxed);
}

void aaudio_out_resume(void)
{
    if (!active)
        return;
    if (atomic_exchange_explicit(&needs_reopen, 0, memory_order_acq_rel)) {
        close_stream();
        if (!open_stream()) {
            active = 0;
            sl_play_state(3);
            return;
        }
        if (api.request_start(stream) != AAUDIO_OK) {
            close_stream();
            active = 0;
            sl_play_state(3);
            return;
        }
    }
    aaudio_out_set_volume_mb((int16_t)PLATFORM_AUDIO->volume_millibel);
    sync_limit();
    reset_ring();
    atomic_store_explicit(&running, 1, memory_order_release);
    sl_play_state(1);   /* restart_playback just started the OpenSL player; keep it idle. */
}

void aaudio_out_pause(void)
{
    if (!active)
        return;
    atomic_store_explicit(&running, 0, memory_order_release);
    reset_ring();
}

void aaudio_out_close(void)
{
    if (!active && stream == NULL)
        return;
    close_stream();
    active = 0;
}

/* Emulation thread, once per frame. Returns 1 when the samples were taken (or dropped on purpose). */
int aaudio_out_write(const void *pcm, uint32_t samples)
{
    uint32_t w;
    uint32_t r;
    uint32_t i;
    const int16_t *in = (const int16_t *)pcm;

    if (!active)
        return 0;
    if (atomic_load_explicit(&needs_reopen, memory_order_acquire)) {
        close_stream();
        if (open_stream() && api.request_start(stream) == AAUDIO_OK) {
            atomic_store_explicit(&needs_reopen, 0, memory_order_relaxed);
            reset_ring();
            atomic_store_explicit(&running, 1, memory_order_release);
        } else {
            close_stream();
            active = 0;
            sl_play_state(3);        /* back to OpenSL ES */
            return 0;
        }
    }
    w = atomic_load_explicit(&ring_write, memory_order_relaxed);
    r = atomic_load_explicit(&ring_read, memory_order_acquire);
    if ((w - r) + samples > atomic_load_explicit(&limit_samples, memory_order_relaxed))
        return 1;                   /* ring is as deep as the chosen latency allows: drop, like a full queue */
    for (i = 0; i < samples; i++)
        ring[(w + i) & RING_MASK] = in[i];
    atomic_store_explicit(&ring_write, w + samples, memory_order_release);
    return 1;
}
