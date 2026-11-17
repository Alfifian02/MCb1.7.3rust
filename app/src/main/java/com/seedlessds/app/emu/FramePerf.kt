package com.seedlessds.app.emu

import android.opengl.EXTDisjointTimerQuery
import android.opengl.GLES20
import com.seedlessds.app.AppLog
import com.seedlessds.core.SeedlessCore
import java.util.Locale

/**
 * Frame statistics for the render thread, plus the GPU time of the presentation shader chain.
 *
 * SeedlessDS rasterises 3D on the CPU and only uses the GPU for the shader chain, so "where is
 * the time going" has two separate answers:
 *  - CPU: the emulation thread (native, see frame_stats.c) and the render thread's GL submit.
 *  - GPU: GL_EXT_disjoint_timer_query around the upload and shader chain.
 *
 * GPU timing uses a ring of [SLOTS] queries and never waits. Each frame issues at most one
 * query, and results are read only once the driver reports them as available. If every slot
 * is still in flight the frame simply goes unmeasured. A disjoint event (clock change, power
 * state change) discards the results that were in flight.
 *
 * All methods except [latest] run on the render thread with the GL context current. When
 * disabled every call returns immediately.
 */
class FramePerf {

    class Snapshot(
        val frames: Int,
        val renderCpuAvgMs: Float,
        val renderCpuP99Ms: Float,
        val gpuSamples: Int,
        val gpuAvgMs: Float,
        val gpuP99Ms: Float,
        val gpuMaxMs: Float,
        val periodAvgMs: Float,
        val periodP99Ms: Float,
        val worstMs: Float,
        val swapAvgMs: Float,
        val emuSamples: Int,
        val emuCpuAvgMs: Float,
        val emuCpuP99Ms: Float,
        val emuCpuMaxMs: Float,
        val procCpuAvgMs: Float,
        val ioAvgMs: Float,
        val ioMaxMs: Float,
        val ioFrames: Int,
    ) {
        /** Compact text for the on-screen overlay. avg/p99 in milliseconds. */
        fun overlay(lite: Boolean): String {
            val gpu = if (gpuSamples > 0) fmt("%.1f/%.1f", gpuAvgMs, gpuP99Ms) else "n/a"
            return fmt("EMU %.1f/%.1f  RND %.1f/%.1f", emuCpuAvgMs, emuCpuP99Ms, renderCpuAvgMs, renderCpuP99Ms) + "\n" +
                "GPU $gpu  WORST " + fmt("%.1f", worstMs) + "\n" +
                fmt("IO %.1f/%.1f", ioAvgMs, ioMaxMs) + (if (lite) "  LITE" else "")
        }

        fun logLine(): String =
            fmt("frames=%d emu_cpu=%.2f/%.2f/%.2f proc_cpu=%.2f render_cpu=%.2f/%.2f", frames, emuCpuAvgMs, emuCpuP99Ms, emuCpuMaxMs, procCpuAvgMs, renderCpuAvgMs, renderCpuP99Ms) +
                " " + (if (gpuSamples > 0) fmt("gpu=%.2f/%.2f/%.2f (n=%d)", gpuAvgMs, gpuP99Ms, gpuMaxMs, gpuSamples) else "gpu=n/a") +
                fmt(" period=%.2f/%.2f worst=%.2f swap=%.2f io=%.2f/%.2f (%d frames) ms avg/p99/max", periodAvgMs, periodP99Ms, worstMs, swapAvgMs, ioAvgMs, ioMaxMs, ioFrames)
    }

    /** Last published statistics, null while disabled or before the first publish. Safe from any thread. */
    @Volatile
    var latest: Snapshot? = null
        private set

    var gpuSupported = false
        private set

    private var enabled = false

    private val cpuRing = Ring(WINDOW)
    private val periodRing = Ring(WINDOW)
    private val gpuRing = Ring(WINDOW)
    private var swapSumMs = 0f
    private var swapCount = 0
    private var lastEndNs = 0L
    private var sincePublish = 0
    private var sinceLog = 0

    private val scratch = FloatArray(WINDOW)
    private val stat = FloatArray(3)
    private val nativeStats = IntArray(NATIVE_FIELDS)

    private val queryIds = IntArray(SLOTS)
    private val queryState = IntArray(SLOTS)
    private val queryTainted = BooleanArray(SLOTS)
    private var head = 0
    private var tail = 0
    private var gpuOpen = false
    private val one = IntArray(1)
    private val ns = LongArray(1)

    /** Call when the GL context has just been created. */
    fun onContext(glExtensions: String) {
        gpuSupported = glExtensions.contains("GL_EXT_disjoint_timer_query")
        queryIds.fill(0)
        queryState.fill(FREE)
        queryTainted.fill(false)
        head = 0; tail = 0; gpuOpen = false
    }

    fun setEnabled(on: Boolean) {
        if (on == enabled) return
        enabled = on
        runCatching { SeedlessCore.frameStatsEnable(on) }
        if (on) {
            cpuRing.clear(); periodRing.clear(); gpuRing.clear()
            swapSumMs = 0f; swapCount = 0; lastEndNs = 0L; sincePublish = 0; sinceLog = 0
        } else {
            latest = null
        }
    }

    // ---- GPU timer ring ------------------------------------------------------------------

    private fun gpuReady(): Boolean {
        if (!gpuSupported) return false
        if (queryIds[0] != 0) return true
        return try {
            EXTDisjointTimerQuery.glGenQueriesEXT(SLOTS, queryIds, 0)
            // Reading the flag clears it, so start from a clean state.
            GLES20.glGetIntegerv(EXTDisjointTimerQuery.GL_GPU_DISJOINT_EXT, one, 0)
            queryIds[0] != 0
        } catch (t: Throwable) {
            gpuFailed(t)
            false
        }
    }

    private fun gpuFailed(t: Throwable) {
        gpuSupported = false
        gpuOpen = false
        AppLog.w("perf", "GPU timer disabled: $t")
    }

    /** Start timing. Skipped, without waiting, when every slot is still waiting for its result. */
    fun gpuBegin() {
        if (!enabled || !gpuReady() || queryState[head] != FREE) return
        try {
            EXTDisjointTimerQuery.glBeginQueryEXT(EXTDisjointTimerQuery.GL_TIME_ELAPSED_EXT, queryIds[head])
            gpuOpen = true
        } catch (t: Throwable) {
            gpuFailed(t)
        }
    }

    fun gpuEnd() {
        if (!gpuOpen) return
        gpuOpen = false
        try {
            EXTDisjointTimerQuery.glEndQueryEXT(EXTDisjointTimerQuery.GL_TIME_ELAPSED_EXT)
            queryState[head] = PENDING
            queryTainted[head] = false
            head = (head + 1) % SLOTS
        } catch (t: Throwable) {
            gpuFailed(t)
        }
    }

    /** Reads every result that is already available, oldest first. Never blocks. */
    private fun gpuCollect() {
        if (!gpuSupported || queryIds[0] == 0 || queryState[tail] != PENDING) return
        try {
            GLES20.glGetIntegerv(EXTDisjointTimerQuery.GL_GPU_DISJOINT_EXT, one, 0)
            if (one[0] != 0) for (i in 0 until SLOTS) if (queryState[i] == PENDING) queryTainted[i] = true
            while (queryState[tail] == PENDING) {
                EXTDisjointTimerQuery.glGetQueryObjectuivEXT(queryIds[tail], EXTDisjointTimerQuery.GL_QUERY_RESULT_AVAILABLE_EXT, one, 0)
                if (one[0] == 0) break
                EXTDisjointTimerQuery.glGetQueryObjectui64vEXT(queryIds[tail], EXTDisjointTimerQuery.GL_QUERY_RESULT_EXT, ns, 0)
                if (!queryTainted[tail]) gpuRing.add(ns[0] / 1_000_000f)
                queryState[tail] = FREE
                tail = (tail + 1) % SLOTS
            }
        } catch (t: Throwable) {
            gpuFailed(t)
        }
    }

    // ---- per-frame recording -------------------------------------------------------------

    /**
     * Call once per presented frame, after eglSwapBuffers.
     * [cpuNs] is the render thread's time from the end of the wait for the emulator to the end of
     * draw(), [swapNs] the time inside eglSwapBuffers, [endNs] the elapsedRealtime at the end of draw().
     */
    fun recordFrame(cpuNs: Long, swapNs: Long, endNs: Long) {
        if (!enabled) return
        cpuRing.add(cpuNs / 1_000_000f)
        swapSumMs += swapNs / 1_000_000f
        swapCount++
        if (lastEndNs != 0L) {
            val period = endNs - lastEndNs
            // A pause or a menu is not a slow frame.
            if (period in 1 until GAP_NS) periodRing.add(period / 1_000_000f)
        }
        lastEndNs = endNs
        if (++sincePublish >= PUBLISH_EVERY) publish()
        if (++sinceLog >= LOG_EVERY) {
            sinceLog = 0
            latest?.let { AppLog.i("perf", it.logLine()) }
        }
    }

    /** Call every frame after eglSwapBuffers, drawn or not, to harvest finished GPU queries. */
    fun afterSwap() {
        if (enabled) gpuCollect()
    }

    private fun publish() {
        sincePublish = 0
        cpuRing.stats(scratch, stat); val cpuAvg = stat[0]; val cpuP99 = stat[1]
        periodRing.stats(scratch, stat); val perAvg = stat[0]; val perP99 = stat[1]; val worst = stat[2]
        gpuRing.stats(scratch, stat); val gAvg = stat[0]; val gP99 = stat[1]; val gMax = stat[2]
        val emuN = try { SeedlessCore.frameStats(nativeStats) } catch (t: Throwable) { 0 }
        val n = nativeStats
        latest = Snapshot(
            frames = cpuRing.count,
            renderCpuAvgMs = cpuAvg, renderCpuP99Ms = cpuP99,
            gpuSamples = gpuRing.count, gpuAvgMs = gAvg, gpuP99Ms = gP99, gpuMaxMs = gMax,
            periodAvgMs = perAvg, periodP99Ms = perP99, worstMs = maxOf(worst, n[8] / 1000f),
            swapAvgMs = if (swapCount > 0) swapSumMs / swapCount else 0f,
            emuSamples = emuN,
            emuCpuAvgMs = n[1] / 1000f, emuCpuP99Ms = n[2] / 1000f, emuCpuMaxMs = n[3] / 1000f,
            procCpuAvgMs = n[4] / 1000f,
            ioAvgMs = n[9] / 1000f, ioMaxMs = n[10] / 1000f, ioFrames = n[11],
        )
    }

    private class Ring(private val size: Int) {
        private val values = FloatArray(size)
        private var n = 0
        private var next = 0
        val count: Int get() = n

        fun clear() { n = 0; next = 0 }

        fun add(x: Float) {
            values[next] = x
            next = (next + 1) % size
            if (n < size) n++
        }

        /** out = average, 99th percentile, maximum. All zero while empty. */
        fun stats(scratch: FloatArray, out: FloatArray) {
            if (n == 0) { out[0] = 0f; out[1] = 0f; out[2] = 0f; return }
            var sum = 0f
            for (i in 0 until n) { scratch[i] = values[i]; sum += values[i] }
            java.util.Arrays.sort(scratch, 0, n)
            out[0] = sum / n
            out[1] = scratch[(n * 99 + 99) / 100 - 1]
            out[2] = scratch[n - 1]
        }
    }

    companion object {
        const val SLOTS = 6
        private const val WINDOW = 600
        private const val PUBLISH_EVERY = 30
        private const val LOG_EVERY = 600
        private const val GAP_NS = 500_000_000L
        private const val NATIVE_FIELDS = 12
        private const val FREE = 0
        private const val PENDING = 1

        internal fun fmt(f: String, vararg a: Any): String = String.format(Locale.US, f, *a)
    }
}
