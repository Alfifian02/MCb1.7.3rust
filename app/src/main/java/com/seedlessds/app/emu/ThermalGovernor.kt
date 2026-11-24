package com.seedlessds.app.emu

import android.content.Context
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.PowerManager
import androidx.annotation.RequiresApi
import com.seedlessds.app.AppLog
import com.seedlessds.core.SeedlessCore

/**
 * Answers thermal pressure by lowering the 3D resolution (roadmap A4).
 *
 * Dynamic 3D resolution reacts to missed frames, which is already late: by then the SoC is
 * throttling. This one watches the thermal status (API 29+) and the thermal headroom forecast
 * (API 30+) and asks the core to cap the 3D scale a number of steps below its ceiling. The core
 * applies the cap on top of dynamic resolution, so the smaller size wins and neither fights the other.
 *
 * Steps down are immediate. Steps back up are slow, one level per [RECOVER_MS] of calm, so the
 * resolution does not bounce every few seconds. The cap never goes below the floor the core uses
 * for dynamic resolution (2x), and it does nothing when the 3D scale is already at that floor.
 *
 * It evaluates every [TICK_MS] on the main thread and re-reads the setting each time, so switching
 * "Thermal adaptive" in the in-game settings takes effect without restarting the game.
 */
class ThermalGovernor(context: Context) {
    private val app = context.applicationContext
    private val handler = Handler(Looper.getMainLooper())
    private val power = app.getSystemService(Context.POWER_SERVICE) as? PowerManager

    @Volatile private var started = false
    private var status = 0
    private var level = 0
    private var calmSince = 0L
    private var listener: Any? = null

    private val tick = object : Runnable {
        override fun run() {
            if (!started) return
            evaluate()
            handler.postDelayed(this, TICK_MS)
        }
    }

    fun start() {
        if (started || Build.VERSION.SDK_INT < 29 || power == null) return
        started = true
        try {
            register()
        } catch (t: Throwable) {
            AppLog.w(TAG, "thermal listener unavailable: $t")
        }
        handler.postDelayed(tick, TICK_MS)
    }

    fun stop() {
        if (!started) return
        started = false
        handler.removeCallbacks(tick)
        try {
            unregister()
        } catch (_: Throwable) {
        }
        level = 0
        publish()
        summary = ""
    }

    @RequiresApi(29)
    private fun register() {
        val pm = power ?: return
        status = pm.currentThermalStatus
        val l = PowerManager.OnThermalStatusChangedListener { s -> status = s }
        listener = l
        pm.addThermalStatusListener({ r -> handler.post(r) }, l)
    }

    @RequiresApi(29)
    private fun unregister() {
        val pm = power ?: return
        (listener as? PowerManager.OnThermalStatusChangedListener)?.let { pm.removeThermalStatusListener(it) }
        listener = null
    }

    private fun evaluate() {
        if (!SettingsRepo.current.thermalAdaptive) {
            if (level != 0) { level = 0; publish() }
            summary = ""
            return
        }
        val now = android.os.SystemClock.elapsedRealtime()
        val headroom = headroom()
        val wanted = maxOf(levelForStatus(status), levelForHeadroom(headroom))
        if (wanted > level) {
            level = wanted
            calmSince = 0L
            publish()
        } else if (wanted < level) {
            if (calmSince == 0L) calmSince = now
            if (now - calmSince >= RECOVER_MS) {
                level--
                calmSince = now
                publish()
            }
        } else {
            calmSince = 0L
        }
        summary = "TH " + status + (if (level > 0) " -" + level else "")
    }

    /** Thermal headroom 10 s ahead, or NaN when the device does not provide it. Needs API 30. */
    private fun headroom(): Float {
        if (Build.VERSION.SDK_INT < 30) return Float.NaN
        return try {
            power?.getThermalHeadroom(HEADROOM_FORECAST_S) ?: Float.NaN
        } catch (_: Throwable) {
            Float.NaN
        }
    }

    private fun publish() {
        try {
            SeedlessCore.setThermalLevel(level)
        } catch (_: Throwable) {
        }
    }

    private fun levelForStatus(s: Int): Int = when {
        // THERMAL_STATUS_NONE = 0, LIGHT = 1, MODERATE = 2, SEVERE = 3, CRITICAL = 4, EMERGENCY = 5, SHUTDOWN = 6
        s >= 4 -> 3
        s == 3 -> 2
        s >= 1 -> 1
        else -> 0
    }

    // 1.0 means "severe throttling"; the forecast is a hint, so it only ever adds a step or two.
    private fun levelForHeadroom(h: Float): Int = when {
        h.isNaN() -> 0
        h >= 0.95f -> 2
        h >= 0.85f -> 1
        else -> 0
    }

    companion object {
        private const val TAG = "thermal"
        private const val TICK_MS = 5_000L
        private const val RECOVER_MS = 45_000L
        private const val HEADROOM_FORECAST_S = 10

        /** For the diagnostics overlay, for example "TH 2 -1"; empty when off. */
        @Volatile var summary: String = ""
            private set
    }
}
