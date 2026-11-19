package org.fossify.camera.ui

import android.graphics.Color
import android.graphics.drawable.GradientDrawable
import android.view.Gravity
import android.view.ViewGroup
import android.widget.LinearLayout
import android.widget.TextView

/** A horizontal row of pill shaped options of which one is highlighted. */
class OptionChipRow<T>(
    private val container: LinearLayout,
    private val selectedColor: () -> Int,
    private val labelOf: (T) -> String,
) {
    private companion object {
        private const val BACKGROUND_ALPHA = 102
        private const val CORNER_DP = 16
        private const val PADDING_DP = 10
        private const val MARGIN_DP = 4
        private const val MIN_WIDTH_DP = 44
        private const val MIN_HEIGHT_DP = 32
        private const val TEXT_SIZE_SP = 13f
    }

    private val chips = mutableListOf<TextView>()
    private var items = emptyList<T>()

    fun show(newItems: List<T>, selected: T, onSelect: (T) -> Unit) {
        container.removeAllViews()
        chips.clear()
        items = newItems
        newItems.forEach { item ->
            val chip = createChip(labelOf(item)) {
                highlight(item)
                onSelect(item)
            }
            container.addView(chip)
            chips.add(chip)
        }
        highlight(selected)
    }

    private fun highlight(selected: T) {
        chips.forEachIndexed { index, chip ->
            chip.setTextColor(if (items[index] == selected) selectedColor() else Color.WHITE)
        }
    }

    private fun createChip(label: String, onClick: () -> Unit): TextView {
        val density = container.resources.displayMetrics.density
        val background = GradientDrawable().apply {
            shape = GradientDrawable.RECTANGLE
            cornerRadius = CORNER_DP * density
            setColor(Color.argb(BACKGROUND_ALPHA, 0, 0, 0))
        }

        return TextView(container.context).apply {
            text = label
            gravity = Gravity.CENTER
            textSize = TEXT_SIZE_SP
            minWidth = (MIN_WIDTH_DP * density).toInt()
            minHeight = (MIN_HEIGHT_DP * density).toInt()
            val padding = (PADDING_DP * density).toInt()
            setPadding(padding, 0, padding, 0)
            this.background = background
            layoutParams = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT,
                ViewGroup.LayoutParams.WRAP_CONTENT
            ).apply {
                val margin = (MARGIN_DP * density).toInt()
                marginStart = margin
                marginEnd = margin
            }
            setOnClickListener { onClick() }
        }
    }
}
