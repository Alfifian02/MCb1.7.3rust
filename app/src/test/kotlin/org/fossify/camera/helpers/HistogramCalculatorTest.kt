package org.fossify.camera.helpers

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class HistogramCalculatorTest {

    @Test
    fun blackAndWhitePixelsLandInTheOuterBins() {
        val black = 0xFF000000.toInt()
        val white = 0xFFFFFFFF.toInt()
        val bins = HistogramCalculator.fromArgb(intArrayOf(black, black, white))

        assertEquals(2, bins[0])
        assertEquals(1, bins[HistogramCalculator.BINS - 1])
        assertEquals(3, bins.sum())
    }

    @Test
    fun pureGreenIsBrighterThanPureBlue() {
        val green = HistogramCalculator.fromArgb(intArrayOf(0xFF00FF00.toInt()))
        val blue = HistogramCalculator.fromArgb(intArrayOf(0xFF0000FF.toInt()))

        assertTrue(green.indexOfFirst { it > 0 } > blue.indexOfFirst { it > 0 })
    }

    @Test
    fun normalizeIgnoresTheExtremeBinsForThePeak() {
        val bins = IntArray(HistogramCalculator.BINS)
        bins[0] = 1_000
        bins[128] = 10
        bins[129] = 5

        val normalized = HistogramCalculator.normalize(bins)
        assertEquals(1f, normalized[128], 0f)
        assertEquals(0.5f, normalized[129], 0f)
        assertEquals(1f, normalized[0], 0f)
    }

    @Test
    fun normalizeOfAnEmptyHistogramIsFlat() {
        val normalized = HistogramCalculator.normalize(IntArray(HistogramCalculator.BINS))
        assertTrue(normalized.all { it == 0f })
    }
}
