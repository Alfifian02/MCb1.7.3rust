package com.seedlessds.app.emu

import android.content.Context
import android.os.Build
import android.os.PerformanceHintManager
import androidx.annotation.RequiresApi
import com.seedlessds.app.AppLog
import com.seedlessds.core.SeedlessCore

/**
 * Scheduling hints for the emulation thread and its workers (roadmap A3).
 *
 *  - ADPF: a PerformanceHintManager session (API 31+) covering the emulation thread, the 3D raster
 *    workers and the 2D workers. Every frame it is told how long the frame really took, measured in
 *    the core around the pacer so the pacer's sleep does not count as work.
 *  - Affinity (optional): the same threads are kept on the fast cores. Some devices ignore or limit
 *    this, and it can raise heat, so it is a separate switch that is off by default.
 *
 * Everything is wrapped in try/catch: OEM implementations differ and a failure here must never
 * reach the emulation. [onFrame] is called by the render thread once per presented frame.
 */
object PerfScheduler {
    private const val TAG = "perf"
    private const val TARGET_NS = 16_666_667L
    private const val REFRESH_FRAMES = 120
    private const val MAX_TIDS = 32

    @Volatile private var enabled = false
    private var appContext: Context? = null
    private var wantAdpf = false
    private var wantPin = false

    private var session: Any? = null
    private var sessionTids = IntArray(0)
    private var adpfFailed = false
    private var pinned = false
    private var lastSeq = 0
    private var frames = 0
    private val tidBuffer = IntArray(MAX_TIDS)

    /** One line for the diagnostics overlay: "ADPF 5 PIN" and so on; empty when nothing is active. */
    @Volatile var status: String = ""
        private set

    /** Call before the game starts and again whenever the settings change. Safe from any thread. */
    @Synchronized
    fun configure(ctx: Context, adpf: Boolean, pin: Boolean) {
        appContext = ctx.applicationContext
        wantAdpf = adpf && Build.VERSION.SDK_INT >= 31
        wantPin = pin
        if (wantPin != pinned) applyPin()
        if (!wantAdpf) closeSession()
        enabled = wantAdpf || wantPin
        if (!enabled) status = ""
        lastSeq = 0
        frames = 0
    }

    /** Render thread, once per frame. */
    fun onFrame() {
        if (!enabled) return
        try {
            if (wantAdpf) {
                if (frames++ % REFRESH_FRAMES == 0) refreshSession()
                report()
            }
        } catch (t: Throwable) {
            adpfFailed = true
            wantAdpf = false
            closeSession()
            AppLog.w(TAG, "ADPF disabled: $t")
        }
    }

    @Synchronized
    fun shutdown() {
        enabled = false
        wantAdpf = false
        closeSession()
        if (pinned) {
            runCatching { SeedlessCore.perfSetAffinity(0L) }
            pinned = false
        }
        status = ""
    }

    private fun applyPin() {
        val mask = PerfHints.fastCoreMask
        try {
            if (wantPin && mask != 0L) {
                SeedlessCore.perfSetAffinity(mask)
                pinned = true
            } else if (pinned) {
                SeedlessCore.perfSetAffinity(0L)
                pinned = false
            }
        } catch (t: Throwable) {
            AppLog.w(TAG, "affinity failed: $t")
        }
        updateStatus()
    }

    private fun report() {
        val sess = session ?: return
        val packed = SeedlessCore.perfLastWork()
        val seq = (packed ushr 32).toInt()
        if (seq == lastSeq) return
        lastSeq = seq
        val micros = packed and 0xFFFFFFFFL
        if (micros == 0L) return
        reportOn(sess, micros * 1000L)
    }

    @RequiresApi(31)
    private fun reportOn(sess: Any, nanos: Long) {
        (sess as PerformanceHintManager.Session).reportActualWorkDuration(nanos)
    }

    private fun refreshSession() {
        val ctx = appContext ?: return
        val count = SeedlessCore.perfThreadIds(tidBuffer)
        if (count <= 0) return
        val tids = tidBuffer.copyOf(count).also { it.sort() }
        if (session != null && tids.contentEquals(sessionTids)) return
        closeSession()
        // Workers appear over time (the 2D pool starts on the first frame it is needed), so the
        // session is rebuilt when the set changes. Some devices cap the thread count, so retry smaller.
        var attempt = tids
        while (attempt.isNotEmpty()) {
            val created = create(ctx, attempt)
            if (created != null) {
                session = created
                sessionTids = tids
                updateStatus(attempt.size)
                return
            }
            attempt = if (attempt.size > 4) attempt.copyOf(4) else if (attempt.size > 1) attempt.copyOf(1) else IntArray(0)
        }
        AppLog.w(TAG, "ADPF: the device refused a hint session")
        wantAdpf = false
        updateStatus()
    }

    @RequiresApi(31)
    private fun createSession(ctx: Context, tids: IntArray): Any? {
        val mgr = ctx.getSystemService(Context.PERFORMANCE_HINT_SERVICE) as? PerformanceHintManager ?: return null
        return mgr.createHintSession(tids, TARGET_NS)
    }

    private fun create(ctx: Context, tids: IntArray): Any? = try {
        if (Build.VERSION.SDK_INT >= 31) createSession(ctx, tids) else null
    } catch (t: Throwable) {
        null
    }

    @RequiresApi(31)
    private fun closeOn(sess: Any) {
        (sess as PerformanceHintManager.Session).close()
    }

    private fun closeSession() {
        val sess = session ?: return
        session = null
        sessionTids = IntArray(0)
        try {
            if (Build.VERSION.SDK_INT >= 31) closeOn(sess)
        } catch (_: Throwable) {
        }
        updateStatus()
    }

    private fun updateStatus(adpfThreads: Int = if (session != null) sessionTids.size else 0) {
        status = buildString {
            if (adpfThreads > 0) append("ADPF ").append(adpfThreads)
            if (pinned) { if (isNotEmpty()) append(' '); append("PIN") }
        }
    }
}
