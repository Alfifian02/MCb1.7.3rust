package com.seedlessds.app.emu

import android.view.Choreographer
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.unit.dp
import kotlin.math.hypot

/**
 * Right-stick "Pointer mode" (DsSettings.analogStickMode == 2): the right stick moves a stylus cursor
 * over the bottom screen and a button taps it, so stylus-only DS games are playable with a gamepad.
 *
 * Controls: right stick = move, R3 / R2 = tap (hold to drag), L2 = slow down for precise aiming.
 * While active the triggers no longer act as the DS L/R buttons (see EmulatorActivity).
 *
 * Positions are in DS touch coordinates (0..255, 0..191). A Choreographer loop integrates the stick
 * every frame, because Android only delivers joystick events when the axis value changes.
 */
object StylusCursor : Choreographer.FrameCallback {
    const val POINTER_MODE = 2
    private const val DS_W = 255f
    private const val DS_H = 191f
    private const val BASE_SPEED = 200f   // DS pixels per second at full deflection, 100% speed
    private const val SLOW_FACTOR = 0.3f

    var x by mutableStateOf(128f); private set
    var y by mutableStateOf(96f); private set
    var tapping by mutableStateOf(false); private set

    @Volatile var stickX = 0f
    @Volatile var stickY = 0f

    // Bit sets so a pad that reports both a key and an axis for the same trigger cannot fight itself.
    @Volatile private var tapSources = 0
    @Volatile private var slowSources = 0

    private var view: DsGlView? = null
    private var blocked: () -> Boolean = { false }
    private var lastNanos = 0L
    private var running = false
    private var pushedX = -1
    private var pushedY = -1

    fun setTap(source: Int, pressed: Boolean) {
        tapSources = if (pressed) tapSources or source else tapSources and source.inv()
    }

    fun setSlow(source: Int, pressed: Boolean) {
        slowSources = if (pressed) slowSources or source else slowSources and source.inv()
    }

    fun start(glView: DsGlView, isBlocked: () -> Boolean) {
        view = glView
        blocked = isBlocked
        if (running) return
        running = true
        lastNanos = 0L
        Choreographer.getInstance().postFrameCallback(this)
    }

    fun stop() {
        running = false
        Choreographer.getInstance().removeFrameCallback(this)
        release()
        stickX = 0f; stickY = 0f; tapSources = 0; slowSources = 0
    }

    /** Lifts the virtual stylus if it is down. */
    private fun release() {
        if (tapping) { tapping = false; view?.stylusTouch(false, 0, 0) }
        pushedX = -1; pushedY = -1
    }

    override fun doFrame(frameTimeNanos: Long) {
        if (!running) return
        Choreographer.getInstance().postFrameCallback(this)
        val dt = if (lastNanos == 0L) 0f else ((frameTimeNanos - lastNanos) / 1_000_000_000f).coerceAtMost(0.05f)
        lastNanos = frameTimeNanos

        val s = SettingsRepo.current
        val glView = view ?: return
        if (s.analogStickMode != POINTER_MODE || blocked()) {
            tapSources = 0; slowSources = 0; stickX = 0f; stickY = 0f
            release()
            return
        }

        // Radial deadzone, rescaled so movement starts smoothly, with a squared response for fine aiming.
        val dz = s.analogDeadzone.coerceIn(0.1f, 0.9f)
        val mag = hypot(stickX, stickY)
        if (mag > dz && dt > 0f) {
            val strength = ((mag - dz) / (1f - dz)).coerceIn(0f, 1f)
            val speed = BASE_SPEED * (s.pointerSpeed.coerceIn(10, 400) / 100f) *
                (if (slowSources != 0) SLOW_FACTOR else 1f) * strength * strength
            x = (x + stickX / mag * speed * dt).coerceIn(0f, DS_W)
            y = (y + stickY / mag * speed * dt).coerceIn(0f, DS_H)
        }

        val wantTap = tapSources != 0
        if (wantTap) {
            val ix = x.toInt(); val iy = y.toInt()
            if (!tapping || ix != pushedX || iy != pushedY) {
                tapping = true; pushedX = ix; pushedY = iy
                glView.stylusTouch(true, ix, iy)
            }
        } else if (tapping) {
            release()
        }
    }
}

private val CursorDark = Color(0xFF1A1A1A)
private val CursorTapFill = Color(0x8CD4A017)

/** Draws the cursor over the bottom screen. Does not consume touches. */
@Composable
fun StylusCursorOverlay(glView: DsGlView, sizePercent: Int) {
    Canvas(Modifier.fillMaxSize()) {
        val r = glView.bottomScreenRect
        if (r[2] <= 0f || r[3] <= 0f) return@Canvas
        val cx = r[0] + StylusCursor.x / 255f * r[2]
        val cy = r[1] + StylusCursor.y / 191f * r[3]
        val size = 28.dp.toPx() * (sizePercent.coerceIn(50, 300) / 100f)
        drawStylusCursor(Offset(cx, cy), size, StylusCursor.tapping)
    }
}

/**
 * Double ring with a tapered crosshair, drawn as vectors so it stays sharp at any size.
 * [size] is the outer diameter in px; the crosshair centre is the pointer's hotspot.
 */
private fun DrawScope.drawStylusCursor(c: Offset, size: Float, tapping: Boolean) {
    val s = if (tapping) size * 0.88f else size
    // White halo keeps the dark rings visible over dark game scenes.
    drawCircle(Color.White.copy(alpha = 0.85f), radius = 0.5f * s, center = c, style = Stroke(width = 0.025f * s))
    drawCircle(Color.White.copy(alpha = 0.18f), radius = 0.385f * s, center = c)
    if (tapping) drawCircle(CursorTapFill, radius = 0.385f * s, center = c)
    drawCircle(CursorDark, radius = 0.465f * s, center = c, style = Stroke(width = 0.04f * s))
    drawCircle(CursorDark, radius = 0.39f * s, center = c, style = Stroke(width = 0.032f * s))

    val len = 0.21f * s
    val w = 0.03f * s
    val star = Path().apply {
        moveTo(c.x, c.y - len); lineTo(c.x + w, c.y - w)
        lineTo(c.x + len, c.y); lineTo(c.x + w, c.y + w)
        lineTo(c.x, c.y + len); lineTo(c.x - w, c.y + w)
        lineTo(c.x - len, c.y); lineTo(c.x - w, c.y - w)
        close()
    }
    drawPath(star, Color.White.copy(alpha = 0.9f), style = Stroke(width = 0.03f * s))
    drawPath(star, CursorDark)
}
