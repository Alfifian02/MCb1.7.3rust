package org.fossify.camera.views

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.util.AttributeSet
import android.view.View
import org.fossify.camera.models.GridType
import kotlin.math.max
import kotlin.math.min

/**
 * Draws a composition grid (rule of thirds, 4x4, golden ratio) on top of the viewfinder.
 *
 * The view covers the whole preview area. When the preview is letterboxed (not full screen),
 * [setContentSize] makes sure the lines are drawn only over the part that shows the camera image.
 */
class GridOverlayView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = 0,
) : View(context, attrs, defStyleAttr) {

    private val density = resources.displayMetrics.density
    private val linePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        color = Color.argb(LINE_ALPHA, 255, 255, 255)
        strokeWidth = density
        setShadowLayer(2f * density, 0f, 0f, Color.argb(SHADOW_ALPHA, 0, 0, 0))
    }

    private val contentRect = RectF()
    private var gridType = GridType.OFF

    // Portrait aspect ratio (width / height) of the camera image, or null if it fills the view.
    private var contentRatio: Float? = null

    fun setGridType(type: GridType) {
        if (gridType != type) {
            gridType = type
            invalidate()
        }
    }

    /**
     * @param width width of the camera resolution (any orientation)
     * @param height height of the camera resolution (any orientation)
     * @param isFullScreen true if the preview fills the view and is cropped instead of letterboxed
     */
    fun setContentSize(width: Int, height: Int, isFullScreen: Boolean) {
        contentRatio = if (isFullScreen || width <= 0 || height <= 0) {
            null
        } else {
            min(width, height).toFloat() / max(width, height).toFloat()
        }
        invalidate()
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val fractions = gridType.fractions
        if (fractions.isEmpty() || width == 0 || height == 0) {
            return
        }

        computeContentRect()
        for (fraction in fractions) {
            val x = contentRect.left + contentRect.width() * fraction
            val y = contentRect.top + contentRect.height() * fraction
            canvas.drawLine(x, contentRect.top, x, contentRect.bottom, linePaint)
            canvas.drawLine(contentRect.left, y, contentRect.right, y, linePaint)
        }
    }

    private fun computeContentRect() {
        val viewWidth = width.toFloat()
        val viewHeight = height.toFloat()
        val ratio = contentRatio
        if (ratio == null) {
            contentRect.set(0f, 0f, viewWidth, viewHeight)
            return
        }

        if (viewWidth / viewHeight > ratio) {
            val contentWidth = viewHeight * ratio
            val left = (viewWidth - contentWidth) / 2f
            contentRect.set(left, 0f, left + contentWidth, viewHeight)
        } else {
            val contentHeight = viewWidth / ratio
            val top = (viewHeight - contentHeight) / 2f
            contentRect.set(0f, top, viewWidth, top + contentHeight)
        }
    }

    private companion object {
        private const val LINE_ALPHA = 170
        private const val SHADOW_ALPHA = 140
    }
}
