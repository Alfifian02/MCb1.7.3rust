package com.seedlessds.app.emu

import android.content.Context
import android.os.Build
import android.os.PowerManager
import android.view.Surface
import android.view.Window
import java.io.File

/** Hints to Android about the workload, and CPU topology detection. */
object PerfHints {

    /**
     * Asks the system to hold CPU/GPU clocks at a level it can sustain, which avoids
     * sudden thermal throttling. Does nothing where the device does not support it.
     * It can lower peak clocks, so it is opt-in.
     */
    fun sustained(window: Window, ctx: Context, on: Boolean) {
        if (!on || Build.VERSION.SDK_INT < 24) return
        try {
            val pm = ctx.getSystemService(Context.POWER_SERVICE) as? PowerManager ?: return
            if (pm.isSustainedPerformanceModeSupported) window.setSustainedPerformanceMode(true)
        } catch (_: Throwable) {
        }
    }

    /**
     * Tells the compositor the content runs at 60 fps so it picks a matching refresh
     * rate (60 or a multiple) instead of a rate that causes judder. Seamless-only on API 31+.
     */
    fun frameRate(surface: Surface) {
        if (Build.VERSION.SDK_INT < 30 || !surface.isValid) return
        try {
            if (Build.VERSION.SDK_INT >= 31) {
                surface.setFrameRate(
                    60f,
                    Surface.FRAME_RATE_COMPATIBILITY_FIXED_SOURCE,
                    Surface.CHANGE_FRAME_RATE_ONLY_IF_SEAMLESS,
                )
            } else {
                surface.setFrameRate(60f, Surface.FRAME_RATE_COMPATIBILITY_FIXED_SOURCE)
            }
        } catch (_: Throwable) {
        }
    }

    /**
     * Number of "fast" cores: those whose max frequency is at least 70% of the fastest core.
     * Returns 0 when the topology cannot be read (some devices hide cpufreq from apps).
     */
    val bigCores: Int by lazy {
        try {
            val n = Runtime.getRuntime().availableProcessors()
            val freqs = (0 until n).mapNotNull { i ->
                val f = File("/sys/devices/system/cpu/cpu$i/cpufreq/cpuinfo_max_freq")
                if (f.canRead()) f.readText().trim().toLongOrNull() else null
            }
            if (freqs.size < 2) 0 else {
                val top = freqs.max()
                freqs.count { it * 10 >= top * 7 }
            }
        } catch (_: Throwable) {
            0
        }
    }
}
