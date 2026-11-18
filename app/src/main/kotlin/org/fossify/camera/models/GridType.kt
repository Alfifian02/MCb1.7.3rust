package org.fossify.camera.models

import androidx.annotation.StringRes
import org.fossify.camera.R

/**
 * Composition guides that can be drawn on top of the viewfinder.
 * [fractions] are the relative positions (0..1) of the lines drawn on both axes.
 */
enum class GridType(@StringRes val stringResId: Int, val fractions: FloatArray) {
    OFF(R.string.grid_off, floatArrayOf()),
    THIRDS(R.string.grid_thirds, floatArrayOf(1f / 3f, 2f / 3f)),
    GRID_4X4(R.string.grid_4x4, floatArrayOf(0.25f, 0.5f, 0.75f)),
    GOLDEN_RATIO(R.string.grid_golden_ratio, floatArrayOf(0.382f, 0.618f))
}
