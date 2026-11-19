package org.fossify.camera.helpers

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ManualExposureLadderTest {

    @Test
    fun isoStepsStayInsideTheSensorRange() {
        val steps = ManualExposureLadder.isoSteps(min = 100, max = 800)
        assertEquals(listOf(100, 125, 160, 200, 250, 320, 400, 500, 640, 800), steps)
    }

    @Test
    fun isoStepsFallBackToTheMinimumWhenNothingFits() {
        assertEquals(listOf(7), ManualExposureLadder.isoSteps(min = 7, max = 9))
    }

    @Test
    fun shutterStepsAreSortedFromFastToSlow() {
        val steps = ManualExposureLadder.shutterSteps(minNs = 100_000L, maxNs = 1_000_000_000L)
        assertEquals(steps.sorted(), steps)
        assertTrue(steps.all { it in 100_000L..1_000_000_000L })
        assertEquals(1_000_000_000L, steps.last())
    }

    @Test
    fun shutterIsFormattedAsFractionOrSeconds() {
        assertEquals("1/125", ManualExposureLadder.formatShutter(8_000_000L))
        assertEquals("1/2", ManualExposureLadder.formatShutter(500_000_000L))
        assertEquals("2\"", ManualExposureLadder.formatShutter(2_000_000_000L))
        assertEquals("-", ManualExposureLadder.formatShutter(0L))
    }

    @Test
    fun closestIndexPicksTheNearestValue() {
        val steps = listOf(10L, 20L, 40L)
        assertEquals(1, ManualExposureLadder.closestIndex(steps, 24L))
        assertEquals(2, ManualExposureLadder.closestIndex(steps, 1_000L))
        assertEquals(0, ManualExposureLadder.closestIndex(emptyList(), 5L))
    }
}
