package com.seedlessds.app.emu

import android.opengl.EGL14
import android.opengl.EGLExt
import android.os.Build

/**
 * Even presentation times for 60 fps content (roadmap A5, experimental, off by default).
 *
 * With EGL_ANDROID_presentation_time each frame can be given the time it should appear. Frames are
 * placed on a 16.67 ms grid that follows the emulation's own cadence, so on a 90 or 120 Hz panel
 * the frames are not shown at whichever vsync happens to follow a late or early swap. The grid
 * re-syncs to the clock whenever it drifts more than a frame or two, so it cannot build up delay.
 *
 * Swappy (Android Frame Pacing library) is the other route; it needs the games-frame-pacing AAR,
 * which is an extra dependency, so this uses the EGL extension that is already on every device.
 *
 * Render thread only, with the context current.
 */
internal class PresentPacer {
    private var supported = 0          // 0 unknown, 1 yes, -1 no
    private var next = 0L

    /** Call right before eglSwapBuffers. */
    fun beforeSwap(enabled: Boolean) {
        if (!enabled) { next = 0L; return }
        if (Build.VERSION.SDK_INT < 18 || supported < 0) return
        try {
            val dpy = EGL14.eglGetCurrentDisplay()
            val surface = EGL14.eglGetCurrentSurface(EGL14.EGL_DRAW)
            if (supported == 0) {
                val ext = EGL14.eglQueryString(dpy, EGL14.EGL_EXTENSIONS) ?: ""
                supported = if (ext.contains("EGL_ANDROID_presentation_time")) 1 else -1
                if (supported < 0) return
            }
            val now = System.nanoTime()
            var t = if (next == 0L) now else next
            if (t < now - PERIOD_NS || t - now > 2 * PERIOD_NS) t = now
            next = t + PERIOD_NS
            EGLExt.eglPresentationTimeANDROID(dpy, surface, t)
        } catch (_: Throwable) {
            supported = -1
        }
    }

    private companion object {
        const val PERIOD_NS = 16_666_667L
    }
}
