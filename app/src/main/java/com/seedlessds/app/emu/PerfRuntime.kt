package com.seedlessds.app.emu

import android.content.Context
import android.os.Build
import com.seedlessds.app.AppLog
import com.seedlessds.core.SeedlessCore
import java.io.File

/** Wires the performance features (A1 to A5) into the emulation lifecycle. */
object PerfRuntime {
    private const val TAG = "perf"
    private var governor: ThermalGovernor? = null

    /** Before the core is created: choices that the core reads while it starts. */
    fun prepare(ctx: Context, s: DsSettings) {
        try {
            SeedlessCore.setAudioBackend(if (s.audioBackend == 1) 1 else 0)
            // A2: -1 from the setting means "sized to the fast cores"; auto2dThreads() -1 means "core decides".
            val n = if (s.threads2d < 0) PerfHints.auto2dThreads() else s.threads2d
            SeedlessCore.set2dThreads(n)
            PerfScheduler.configure(ctx, s.adpfHints, s.pinFastCores)
            AppLog.i(TAG, "2d threads=$n bigCores=${PerfHints.bigCores} audio=${s.audioBackend}")
        } catch (t: Throwable) {
            AppLog.w(TAG, "prepare failed: $t")
        }
    }

    /** Emulation started: begin thermal watching. Hints are re-read from the settings on each start. */
    fun onStarted(ctx: Context) {
        if (Build.VERSION.SDK_INT >= 29 && governor == null)
            governor = ThermalGovernor(ctx).also { it.start() }
    }

    fun onStopping() {
        governor?.stop(); governor = null
        PerfScheduler.shutdown()
    }

    /** PGO training builds: write counters to files/pgo/. Does nothing (and leaves no folder) in normal builds. */
    fun flushProfile(ctx: Context) {
        val dir = File(ctx.filesDir, "pgo")
        dir.mkdirs()
        val r = runCatching { SeedlessCore.profileFlush(File(dir, "recon-%p.profraw").path) }.getOrDefault(-1)
        if (r < 0) dir.delete()
    }
}
