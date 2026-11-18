package org.fossify.camera.views

import android.animation.ValueAnimator
import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.os.Handler
import android.os.Looper
import android.view.ViewGroup
import android.view.animation.DecelerateInterpolator
import org.fossify.commons.extensions.getProperPrimaryColor

class FocusCircleView(context: Context) : ViewGroup(context) {
    private val density = resources.displayMetrics.density
    private val circleRadius = CIRCLE_RADIUS_DP * density

    private var mDrawCircle = false
    private val mHandler = Handler(Looper.getMainLooper())
    private var mLastCenterX = 0f
    private var mLastCenterY = 0f
    private var mScale = 1f

    private val mPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        color = context.getProperPrimaryColor()
        strokeWidth = STROKE_WIDTH_DP * density
    }

    private val hideRunnable = Runnable { toggleCircle(false) }

    // The ring starts a bit larger and "locks" onto the tapped point.
    private val scaleAnimator = ValueAnimator.ofFloat(START_SCALE, 1f).apply {
        duration = SCALE_DURATION
        interpolator = DecelerateInterpolator()
        addUpdateListener {
            mScale = it.animatedValue as Float
            invalidate()
        }
    }

    init {
        setWillNotDraw(false)
    }

    fun setStrokeColor(color: Int) {
        mPaint.color = color
    }

    fun drawFocusCircle(x: Float, y: Float) {
        mLastCenterX = x
        mLastCenterY = y
        toggleCircle(true)

        scaleAnimator.cancel()
        scaleAnimator.start()

        mHandler.removeCallbacks(hideRunnable)
        mHandler.postDelayed(hideRunnable, CIRCLE_DURATION)
    }

    private fun toggleCircle(show: Boolean) {
        mDrawCircle = show
        invalidate()
    }

    override fun onDetachedFromWindow() {
        scaleAnimator.cancel()
        mHandler.removeCallbacks(hideRunnable)
        super.onDetachedFromWindow()
    }

    override fun onLayout(changed: Boolean, l: Int, t: Int, r: Int, b: Int) {}

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        if (mDrawCircle) {
            canvas.drawCircle(mLastCenterX, mLastCenterY, circleRadius * mScale, mPaint)
        }
    }

    private companion object {
        private const val CIRCLE_RADIUS_DP = 32f
        private const val STROKE_WIDTH_DP = 1.5f
        private const val CIRCLE_DURATION = 900L
        private const val SCALE_DURATION = 250L
        private const val START_SCALE = 1.4f
    }
}
