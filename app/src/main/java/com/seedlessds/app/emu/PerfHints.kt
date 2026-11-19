package com.seedlessds.app.emu

import android.app.ActivityManager
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
     * Max frequency per CPU index, for the CPUs whose cpufreq file is readable. Some devices hide
     * cpufreq from apps; the list is then short or empty and callers fall back to old behaviour.
     */
    private val coreFreqs: List<Pair<Int, Long>> by lazy {
        try {
            val n = Runtime.getRuntime().availableProcessors()
            (0 until n).mapNotNull { i ->
                val f = File("/sys/devices/system/cpu/cpu$i/cpufreq/cpuinfo_max_freq")
                val khz = if (f.canRead()) f.readText().trim().toLongOrNull() else null
                if (khz != null) i to khz else null
            }
        } catch (_: Throwable) {
            emptyList()
        }
    }

    /**
     * Number of "fast" cores: those whose max frequency is at least 70% of the fastest core.
     * Returns 0 when the topology cannot be read (some devices hide cpufreq from apps).
     */
    val bigCores: Int by lazy {
        val freqs = coreFreqs.map { it.second }
        if (freqs.size < 2) 0 else {
            val top = freqs.max()
            freqs.count { it * 10 >= top * 7 }
        }
    }

    /**
     * Bit n set = CPU n is a fast core (same 70% rule as [bigCores]). 0 when the topology is unknown
     * or every core is fast, because then there is nothing to choose between.
     */
    val fastCoreMask: Long by lazy {
        if (coreFreqs.size < 2) 0L else {
            val top = coreFreqs.maxOf { it.second }
            var mask = 0L
            var count = 0
            for ((cpu, khz) in coreFreqs) {
                if (cpu < 64 && khz * 10 >= top * 7) { mask = mask or (1L shl cpu); count++ }
            }
            if (count == 0 || count >= coreFreqs.size) 0L else mask
        }
    }

    /**
     * Worker count for the core's 2D compose pool (roadmap A2), or -1 to leave the choice to the core.
     * The core's own rule is "online CPUs - 1, at most 7, only with 6 or more CPUs", which on a
     * big.LITTLE chip also places workers on slow cores that the frame then waits for. Here the pool
     * is sized to the fast cores minus the emulation thread, the same idea as the 3D thread count.
     * Homogeneous chips and devices that hide cpufreq keep the core's rule.
     */
    fun auto2dThreads(): Int {
        val cores = Runtime.getRuntime().availableProcessors()
        val big = bigCores
        if (cores < 6 || big <= 0 || big >= cores) return -1
        return (big - 1).coerceIn(1, 7)
    }

    /**
     * Whether preloading the whole ROM into RAM is reasonable on this device right now: not on
     * low-RAM devices, not while the system is short on memory, and only when the ROM is small
     * next to the memory that is free.
     */
    fun preloadAllowed(ctx: Context, romBytes: Long): Boolean {
        return try {
            val am = ctx.getSystemService(Context.ACTIVITY_SERVICE) as? ActivityManager ?: return true
            if (am.isLowRamDevice) return false
            val info = ActivityManager.MemoryInfo()
            am.getMemoryInfo(info)
            if (info.lowMemory || info.totalMem < MIN_PRELOAD_TOTAL_RAM) return false
            romBytes <= 0L || romBytes * 4 <= info.availMem
        } catch (_: Throwable) {
            true
        }
    }

    // Devices sold as "4 GB" report a little under 4 GiB.
    private const val MIN_PRELOAD_TOTAL_RAM = 3584L * 1024 * 1024
}
