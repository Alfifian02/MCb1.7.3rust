#include <jni.h>
#include <EGL/egl.h>
#include <stdint.h>

/*
 * GL_EXT_disjoint_timer_query for the render thread.
 *
 * android.opengl has no Java binding for this extension, so the entry points are
 * resolved with eglGetProcAddress. Every function must be called on the thread
 * that owns the current GL context. Nothing here waits for the GPU.
 */

#define GL_TIME_ELAPSED_EXT 0x88BF
#define GL_QUERY_RESULT_EXT 0x8866
#define GL_QUERY_RESULT_AVAILABLE_EXT 0x8867
#define GL_GPU_DISJOINT_EXT 0x8FBB

typedef void (*gen_fn)(int32_t, uint32_t *);
typedef void (*begin_fn)(uint32_t, uint32_t);
typedef void (*end_fn)(uint32_t);
typedef void (*get_uint_fn)(uint32_t, uint32_t, uint32_t *);
typedef void (*get_u64_fn)(uint32_t, uint32_t, uint64_t *);
typedef void (*get_int_fn)(uint32_t, int32_t *);

static gen_fn p_gen;
static begin_fn p_begin;
static end_fn p_end;
static get_uint_fn p_get_uint;
static get_u64_fn p_get_u64;
static get_int_fn p_get_int;

JNIEXPORT jboolean JNICALL
Java_com_seedlessds_core_SeedlessCore_gpuTimerInit(JNIEnv *env, jclass cls)
{
    (void)env;
    (void)cls;
    p_gen = (gen_fn)eglGetProcAddress("glGenQueriesEXT");
    p_begin = (begin_fn)eglGetProcAddress("glBeginQueryEXT");
    p_end = (end_fn)eglGetProcAddress("glEndQueryEXT");
    p_get_uint = (get_uint_fn)eglGetProcAddress("glGetQueryObjectuivEXT");
    p_get_u64 = (get_u64_fn)eglGetProcAddress("glGetQueryObjectui64vEXT");
    p_get_int = (get_int_fn)eglGetProcAddress("glGetIntegerv");
    return (p_gen && p_begin && p_end && p_get_uint && p_get_u64 && p_get_int) ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT jboolean JNICALL
Java_com_seedlessds_core_SeedlessCore_gpuTimerGen(JNIEnv *env, jclass cls, jintArray ids)
{
    uint32_t q[16];
    jint n;
    jint i;
    jint out[16];

    (void)cls;
    if (!p_gen || !ids)
        return JNI_FALSE;
    n = (*env)->GetArrayLength(env, ids);
    if (n <= 0 || n > 16)
        return JNI_FALSE;
    p_gen(n, q);
    for (i = 0; i < n; i++)
        out[i] = (jint)q[i];
    (*env)->SetIntArrayRegion(env, ids, 0, n, out);
    return JNI_TRUE;
}

JNIEXPORT void JNICALL
Java_com_seedlessds_core_SeedlessCore_gpuTimerBegin(JNIEnv *env, jclass cls, jint id)
{
    (void)env;
    (void)cls;
    if (p_begin)
        p_begin(GL_TIME_ELAPSED_EXT, (uint32_t)id);
}

JNIEXPORT void JNICALL
Java_com_seedlessds_core_SeedlessCore_gpuTimerEnd(JNIEnv *env, jclass cls)
{
    (void)env;
    (void)cls;
    if (p_end)
        p_end(GL_TIME_ELAPSED_EXT);
}

JNIEXPORT jboolean JNICALL
Java_com_seedlessds_core_SeedlessCore_gpuTimerAvailable(JNIEnv *env, jclass cls, jint id)
{
    uint32_t v = 0;

    (void)env;
    (void)cls;
    if (p_get_uint)
        p_get_uint((uint32_t)id, GL_QUERY_RESULT_AVAILABLE_EXT, &v);
    return v != 0 ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT jlong JNICALL
Java_com_seedlessds_core_SeedlessCore_gpuTimerResultNs(JNIEnv *env, jclass cls, jint id)
{
    uint64_t v = 0;

    (void)env;
    (void)cls;
    if (p_get_u64)
        p_get_u64((uint32_t)id, GL_QUERY_RESULT_EXT, &v);
    return (jlong)v;
}

/* Reading the flag clears it. */
JNIEXPORT jboolean JNICALL
Java_com_seedlessds_core_SeedlessCore_gpuTimerDisjoint(JNIEnv *env, jclass cls)
{
    int32_t v = 0;

    (void)env;
    (void)cls;
    if (p_get_int)
        p_get_int(GL_GPU_DISJOINT_EXT, &v);
    return v != 0 ? JNI_TRUE : JNI_FALSE;
}
