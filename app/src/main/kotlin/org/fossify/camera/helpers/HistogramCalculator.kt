package org.fossify.camera.helpers

/** Luminance histogram of ARGB pixels. */
object HistogramCalculator {
    const val BINS = 256

    private const val RED_SHIFT = 16
    private const val GREEN_SHIFT = 8
    private const val CHANNEL_MASK = 0xFF
    private const val RED_WEIGHT = 299
    private const val GREEN_WEIGHT = 587
    private const val BLUE_WEIGHT = 114
    private const val WEIGHT_SUM = 1000

    fun fromArgb(pixels: IntArray): IntArray {
        val bins = IntArray(BINS)
        for (pixel in pixels) {
            val red = (pixel shr RED_SHIFT) and CHANNEL_MASK
            val green = (pixel shr GREEN_SHIFT) and CHANNEL_MASK
            val blue = pixel and CHANNEL_MASK
            val luma = (red * RED_WEIGHT + green * GREEN_WEIGHT + blue * BLUE_WEIGHT) / WEIGHT_SUM
            bins[luma.coerceIn(0, BINS - 1)]++
        }
        return bins
    }

    /**
     * Scales the bins to 0..1. The first and last bin are ignored when looking for the peak,
     * otherwise a few pure black or white pixels would flatten the rest of the graph.
     */
    fun normalize(bins: IntArray): FloatArray {
        val inner = (1 until BINS - 1).maxOfOrNull { bins[it] } ?: 0
        val peak = if (inner > 0) inner else bins.maxOrNull() ?: 0
        if (peak == 0) {
            return FloatArray(bins.size)
        }

        return FloatArray(bins.size) { (bins[it].toFloat() / peak).coerceAtMost(1f) }
    }
}
