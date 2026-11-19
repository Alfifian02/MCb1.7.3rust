package org.fossify.camera.ui

import android.content.Context
import android.graphics.Color
import android.graphics.drawable.GradientDrawable
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.LinearLayout
import android.widget.TextView
import org.fossify.camera.R
import org.fossify.commons.extensions.beGone
import org.fossify.commons.extensions.beVisible
import kotlin.math.abs
import kotlin.math.ln
import kotlin.math.roundToInt

/** The row of zoom presets (0.5x, 1x, 2x...) above the shutter and its live zoom label. */
class ZoomChipsController(
    private val context: Context,
    private val container: LinearLayout,
    private val host: Host,
) {
    interface Host {
        fun currentRotation(): Float
        fun primaryColor(): Int
        fun onPresetClicked(view: View, ratio: Float)
    }

    private companion object {
        private const val CHIP_TOLERANCE = 0.03f
        private const val RANGE_MARGIN = 0.01f
        private const val ULTRA_WIDE_LIMIT = 0.95f
        private const val DUPLICATE_DISTANCE = 0.05f
        private const val ROUNDING_FACTOR = 10f
        private const val MIN_PRESETS = 2
        private const val BACKGROUND_ALPHA = 102
        private const val CORNER_DP = 16
        private const val PADDING_DP = 8
        private const val MARGIN_DP = 4
        private const val MIN_WIDTH_DP = 44
        private const val MIN_HEIGHT_DP = 32
        private const val TEXT_SIZE_SP = 13f
        private val PRESET_CANDIDATES = listOf(0.5f, 1f, 2f, 3f, 5f, 10f)
    }

    private var zoomMin = 1f
    private var zoomMax = 1f
    private var presets: List<Float> = emptyList()
    private val chips = mutableListOf<TextView>()

    fun onZoomStateChanged(minRatio: Float, maxRatio: Float, zoomRatio: Float) {
        if (minRatio != zoomMin || maxRatio != zoomMax) {
            zoomMin = minRatio
            zoomMax = maxRatio
            rebuild()
        }

        update(zoomRatio)
    }

    fun rotate(degrees: Int) {
        chips.forEach { it.animate().rotation(degrees.toFloat()).start() }
    }

    private fun rebuild() {
        container.removeAllViews()
        chips.clear()

        val candidates = PRESET_CANDIDATES
            .filter { it >= zoomMin - RANGE_MARGIN && it <= zoomMax + RANGE_MARGIN }
            .toMutableList()

        // Devices with an ultra wide lens report a minimum zoom ratio below 1x (e.g. 0.6x).
        val lowest = (zoomMin * ROUNDING_FACTOR).toInt() / ROUNDING_FACTOR
        if (lowest < ULTRA_WIDE_LIMIT && candidates.none { abs(it - lowest) < DUPLICATE_DISTANCE }) {
            candidates.add(lowest)
        }

        presets = candidates.sorted()
        if (presets.size < MIN_PRESETS) {
            container.beGone()
            return
        }

        container.beVisible()
        val rotation = host.currentRotation()
        presets.forEach { preset ->
            val chip = createChip(preset)
            chip.rotation = rotation
            container.addView(chip)
            chips.add(chip)
        }
    }

    private fun createChip(preset: Float): TextView {
        val density = context.resources.displayMetrics.density
        val chipBackground = GradientDrawable().apply {
            shape = GradientDrawable.RECTANGLE
            cornerRadius = CORNER_DP * density
            setColor(Color.argb(BACKGROUND_ALPHA, 0, 0, 0))
        }

        return TextView(context).apply {
            text = formatLabel(preset)
            gravity = Gravity.CENTER
            textSize = TEXT_SIZE_SP
            minWidth = (MIN_WIDTH_DP * density).toInt()
            minHeight = (MIN_HEIGHT_DP * density).toInt()
            val horizontalPadding = (PADDING_DP * density).toInt()
            setPadding(horizontalPadding, 0, horizontalPadding, 0)
            setTextColor(Color.WHITE)
            background = chipBackground
            contentDescription = context.getString(R.string.zoom_level)
            layoutParams = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT,
                ViewGroup.LayoutParams.WRAP_CONTENT
            ).apply {
                val margin = (MARGIN_DP * density).toInt()
                marginStart = margin
                marginEnd = margin
            }
            setOnClickListener { host.onPresetClicked(it, preset) }
        }
    }

    private fun update(zoomRatio: Float) {
        if (chips.isEmpty() || zoomRatio <= 0f) {
            return
        }

        // The chip that is closest to the current zoom (measured in zoom "stops") shows the
        // live value while pinching, the others keep their preset label.
        val nearestIndex = presets.indices.minByOrNull { abs(ln(zoomRatio / presets[it])) } ?: 0
        val primaryColor = host.primaryColor()
        chips.forEachIndexed { index, chip ->
            val preset = presets[index]
            if (index == nearestIndex) {
                val isExact = abs(zoomRatio / preset - 1f) <= CHIP_TOLERANCE
                chip.text = formatLabel(if (isExact) preset else zoomRatio)
                chip.setTextColor(primaryColor)
            } else {
                chip.text = formatLabel(preset)
                chip.setTextColor(Color.WHITE)
            }
        }
    }

    private fun formatLabel(ratio: Float): String {
        val rounded = (ratio * ROUNDING_FACTOR).roundToInt() / ROUNDING_FACTOR
        return if (rounded % 1f == 0f) {
            "${rounded.toInt()}×"
        } else {
            String.format(java.util.Locale.getDefault(), "%.1f×", rounded)
        }
    }
}
