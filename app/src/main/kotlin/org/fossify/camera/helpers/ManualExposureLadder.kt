package org.fossify.camera.helpers

import java.util.Locale
import kotlin.math.abs
import kotlin.math.roundToInt

/** The familiar ISO and shutter speed values, limited to what the sensor supports. */
@Suppress("MagicNumber")
object ManualExposureLadder {
    private const val NANOS_PER_SECOND = 1_000_000_000L

    private val ISO_STOPS = listOf(
        50, 64, 80, 100, 125, 160, 200, 250, 320, 400, 500, 640, 800,
        1000, 1250, 1600, 2000, 2500, 3200, 4000, 5000, 6400, 8000, 10000, 12800
    )

    private val FAST_DENOMINATORS = listOf(8000, 4000, 2000, 1000, 500, 250, 125, 60, 30, 15, 8, 4, 2)
    private val SLOW_SECONDS = listOf(1, 2, 4, 8, 15, 30)

    private val SHUTTER_STOPS_NS: List<Long> = buildList {
        FAST_DENOMINATORS.forEach { add(NANOS_PER_SECOND / it) }
        SLOW_SECONDS.forEach { add(it * NANOS_PER_SECOND) }
    }

    fun isoSteps(min: Int, max: Int): List<Int> {
        val inRange = ISO_STOPS.filter { it in min..max }
        return inRange.ifEmpty { listOf(min) }
    }

    fun shutterSteps(minNs: Long, maxNs: Long): List<Long> {
        val inRange = SHUTTER_STOPS_NS.filter { it in minNs..maxNs }
        return inRange.ifEmpty { listOf(minNs) }
    }

    fun formatShutter(ns: Long): String {
        return when {
            ns <= 0L -> "-"
            ns >= NANOS_PER_SECOND ->
                String.format(Locale.ROOT, "%d\"", (ns / NANOS_PER_SECOND.toDouble()).roundToInt())

            else -> "1/${(NANOS_PER_SECOND / ns.toDouble()).roundToInt()}"
        }
    }

    /** Index of the entry of [steps] that is closest to [target], 0 for an empty list. */
    fun closestIndex(steps: List<Long>, target: Long): Int {
        return steps.indices.minByOrNull { abs(steps[it] - target) } ?: 0
    }
}
