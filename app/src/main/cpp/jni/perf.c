#include <jni.h>
#include <stddef.h>
#include <stdint.h>

/* Scheduling, thermal and audio-backend entry points for the Kotlin side (stages A1 to A5).
 * The implementations live in the core: platform/perf.c, gpu3d/dynamic_scale.c, frontend/aaudio_out.c. */

#define ROLE_EMU    1u
#define ROLE_WORKER 2u
#define ROLE_2D     4u
#define ROLE_ALL    7u

void recon_perf_register_thread(unsigned role);
unsigned recon_perf_thread_ids(int32_t *out, unsigned max, unsigned roles_mask);
void recon_perf_set_affinity(uint64_t mask);
int recon_perf_pin_thread(int32_t tid, uint64_t mask);
void recon_perf_set_2d_threads(unsigned n);
uint64_t recon_perf_last_work(void);
void recon_thermal_set(unsigned level);
unsigned recon_thermal_level(void);
unsigned recon_thermal_steps(void);
void recon_audio_backend_set(int backend);
int recon_profile_flush(const char *path);

JNIEXPORT void JNICALL
Java_com_seedlessds_core_SeedlessCore_perfRegisterCurrentThread(JNIEnv *env, jclass cls)
{
    (void)env; (void)cls;
    recon_perf_register_thread(ROLE_EMU);
}

/* Fills out[] with the TIDs of the emulation thread and the raster and 2D workers. */
JNIEXPORT jint JNICALL
Java_com_seedlessds_core_SeedlessCore_perfThreadIds(JNIEnv *env, jclass cls, jintArray out)
{
    int32_t tids[32];
    unsigned n;
    jsize cap;

    (void)cls;
    if (out == NULL)
        return 0;
    cap = (*env)->GetArrayLength(env, out);
    n = recon_perf_thread_ids(tids, 32u, ROLE_ALL);
    if ((jsize)n > cap)
        n = (unsigned)cap;
    if (n != 0u)
        (*env)->SetIntArrayRegion(env, out, 0, (jsize)n, (const jint *)tids);
    return (jint)n;
}

JNIEXPORT void JNICALL
Java_com_seedlessds_core_SeedlessCore_perfSetAffinity(JNIEnv *env, jclass cls, jlong mask)
{
    (void)env; (void)cls;
    recon_perf_set_affinity((uint64_t)mask);
}

JNIEXPORT jboolean JNICALL
Java_com_seedlessds_core_SeedlessCore_perfPinThread(JNIEnv *env, jclass cls, jint tid, jlong mask)
{
    (void)env; (void)cls;
    return recon_perf_pin_thread((int32_t)tid, (uint64_t)mask) == 0 ? JNI_TRUE : JNI_FALSE;
}

/* (sequence << 32) | microseconds of the last frame's work; 0 before the first frame. */
JNIEXPORT jlong JNICALL
Java_com_seedlessds_core_SeedlessCore_perfLastWork(JNIEnv *env, jclass cls)
{
    (void)env; (void)cls;
    return (jlong)recon_perf_last_work();
}

/* count < 0: automatic (the core decides); 0: no 2D workers; 100 or more: boss only. */
JNIEXPORT void JNICALL
Java_com_seedlessds_core_SeedlessCore_set2dThreads(JNIEnv *env, jclass cls, jint count)
{
    (void)env; (void)cls;
    recon_perf_set_2d_threads(count < 0 ? ~0u : (unsigned)count);
}

JNIEXPORT void JNICALL
Java_com_seedlessds_core_SeedlessCore_setThermalLevel(JNIEnv *env, jclass cls, jint level)
{
    (void)env; (void)cls;
    recon_thermal_set(level < 0 ? 0u : (unsigned)level);
}

JNIEXPORT jint JNICALL
Java_com_seedlessds_core_SeedlessCore_thermalLevel(JNIEnv *env, jclass cls)
{
    (void)env; (void)cls;
    return (jint)recon_thermal_level();
}

JNIEXPORT jint JNICALL
Java_com_seedlessds_core_SeedlessCore_thermalSteps(JNIEnv *env, jclass cls)
{
    (void)env; (void)cls;
    return (jint)recon_thermal_steps();
}

JNIEXPORT void JNICALL
Java_com_seedlessds_core_SeedlessCore_setAudioBackend(JNIEnv *env, jclass cls, jint backend)
{
    (void)env; (void)cls;
    recon_audio_backend_set((int)backend);
}

/* PGO training builds only (-DSEEDLESS_PGO_MODE=GENERATE); returns -1 otherwise. */
JNIEXPORT jint JNICALL
Java_com_seedlessds_core_SeedlessCore_profileFlush(JNIEnv *env, jclass cls, jstring path)
{
    const char *utf = path != NULL ? (*env)->GetStringUTFChars(env, path, NULL) : NULL;
    int r;

    (void)cls;
    r = recon_profile_flush(utf);
    if (utf != NULL)
        (*env)->ReleaseStringUTFChars(env, path, utf);
    return (jint)r;
}
