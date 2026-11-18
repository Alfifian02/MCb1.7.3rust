package org.fossify.camera.views

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.util.AttributeSet
import android.view.View
import kotlin.math.abs
import kotlin.math.atan2
import kotlin.math.hypot
import kotlin.math.roundToInt

/**
 * A simple horizon / spirit level indicator driven by the accelerometer.
 *
 * A line is drawn that always stays parallel to the real horizon. It turns to the accent color
 * when the phone is level (within [LEVEL_TOLERANCE_DEGREES]), also when held in landscape.
 * Nothing is drawn while the phone is lying flat, because the horizon is undefined then.
 */
class LevelView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = 0,
) : View(context, attrs, defStyleAttr), SensorEventListener {

    private val density = resources.displayMetrics.density
    private val sensorManager = context.getSystemService(Context.SENSOR_SERVICE) as SensorManager
    private val accelerometer: Sensor? = sensorManager.getDefaultSensor(Sensor.TYPE_ACCELEROMETER)

    private val linePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeCap = Paint.Cap.ROUND
        strokeWidth = 2f * density
        setShadowLayer(2f * density, 0f, 0f, Color.argb(SHADOW_ALPHA, 0, 0, 0))
    }

    private var accentColor = Color.YELLOW
    private var gravityX = 0f
    private var gravityY = 0f
    private var hasReading = false
    private var isRunning = false

    /** True if the sensor needed by this view exists on the device. */
    val isSupported: Boolean get() = accelerometer != null

    fun setAccentColor(color: Int) {
        accentColor = color
        invalidate()
    }

    fun start() {
        if (isRunning || accelerometer == null) {
            return
        }

        isRunning = sensorManager.registerListener(
            this,
            accelerometer,
            SensorManager.SENSOR_DELAY_UI
        )
    }

    fun stop() {
        if (isRunning) {
            sensorManager.unregisterListener(this)
            isRunning = false
            hasReading = false
        }
    }

    override fun onDetachedFromWindow() {
        stop()
        super.onDetachedFromWindow()
    }

    override fun onSensorChanged(event: SensorEvent) {
        val x = event.values[0]
        val y = event.values[1]
        if (!hasReading) {
            gravityX = x
            gravityY = y
            hasReading = true
        } else {
            // Low-pass filter the gravity vector (not the angle, to avoid wrap-around glitches).
            gravityX += FILTER_ALPHA * (x - gravityX)
            gravityY += FILTER_ALPHA * (y - gravityY)
        }
        invalidate()
    }

    override fun onAccuracyChanged(sensor: Sensor?, accuracy: Int) = Unit

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        if (!hasReading || hypot(gravityX, gravityY) < MIN_IN_PLANE_GRAVITY) {
            return
        }

        // 0 when the phone is upright in portrait, +/-90 in landscape.
        val rollDegrees = Math.toDegrees(atan2(gravityX.toDouble(), gravityY.toDouble())).toFloat()
        val tilt = rollDegrees - (rollDegrees / 90f).roundToInt() * 90f
        val isLevel = abs(tilt) <= LEVEL_TOLERANCE_DEGREES

        val cx = width / 2f
        val cy = height / 2f
        val halfLength = (if (isLevel) LEVEL_HALF_LENGTH_DP else HALF_LENGTH_DP) * density

        linePaint.color = if (isLevel) accentColor else Color.argb(LINE_ALPHA, 255, 255, 255)

        // Fixed reference ticks on both sides of the center.
        val tickInner = (HALF_LENGTH_DP + TICK_GAP_DP) * density
        val tickOuter = tickInner + TICK_LENGTH_DP * density
        canvas.drawLine(cx - tickOuter, cy, cx - tickInner, cy, linePaint)
        canvas.drawLine(cx + tickInner, cy, cx + tickOuter, cy, linePaint)

        // The horizon line: rotated opposite to the phone tilt, so it stays parallel to the real horizon.
        canvas.save()
        canvas.rotate(rollDegrees, cx, cy)
        canvas.drawLine(cx - halfLength, cy, cx + halfLength, cy, linePaint)
        canvas.restore()
    }

    private companion object {
        private const val FILTER_ALPHA = 0.15f
        private const val MIN_IN_PLANE_GRAVITY = 2.5f
        private const val LEVEL_TOLERANCE_DEGREES = 1f
        private const val HALF_LENGTH_DP = 36f
        private const val LEVEL_HALF_LENGTH_DP = 44f
        private const val TICK_GAP_DP = 12f
        private const val TICK_LENGTH_DP = 16f
        private const val LINE_ALPHA = 210
        private const val SHADOW_ALPHA = 160
    }
}
