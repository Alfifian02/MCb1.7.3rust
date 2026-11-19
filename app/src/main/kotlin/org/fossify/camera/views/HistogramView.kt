package org.fossify.camera.views

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.util.AttributeSet
import android.view.View

/** Draws a normalized (0..1) luminance histogram, shadows on the left and highlights on the right. */
class HistogramView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
) : View(context, attrs) {

    private companion object {
        private const val BACKGROUND_ALPHA = 102
        private const val BAR_ALPHA = 210
    }

    private var values = FloatArray(0)

    private val backgroundPaint = Paint().apply {
        color = Color.argb(BACKGROUND_ALPHA, 0, 0, 0)
    }

    private val barPaint = Paint().apply {
        color = Color.WHITE
        alpha = BAR_ALPHA
    }

    fun setHistogram(newValues: FloatArray) {
        values = newValues
        invalidate()
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val width = width.toFloat()
        val height = height.toFloat()
        canvas.drawRect(0f, 0f, width, height, backgroundPaint)
        if (values.isEmpty()) {
            return
        }

        val barWidth = width / values.size
        values.forEachIndexed { index, value ->
            val left = index * barWidth
            canvas.drawRect(left, height - value * height, left + barWidth, height, barPaint)
        }
    }
}
