package org.fossify.camera.helpers

import android.graphics.Bitmap
import android.os.Handler
import android.os.Looper
import androidx.camera.view.PreviewView
import org.fossify.commons.helpers.ensureBackgroundThread

/**
 * Looks at the live preview a couple of times per second and reports its luminance histogram.
 * Reading the preview bitmap avoids a third camera stream, which many phones can't combine with
 * 4K video.
 */
class HistogramSampler(
    private val previewView: PreviewView,
    private val onHistogram: (FloatArray) -> Unit,
) {
    private companion object {
        private const val SAMPLE_INTERVAL_MS = 500L
        private const val SAMPLE_SIZE = 64
    }

    private val handler = Handler(Looper.getMainLooper())
    private var running = false

    private val tick = object : Runnable {
        override fun run() {
            if (running) {
                sample()
                handler.postDelayed(this, SAMPLE_INTERVAL_MS)
            }
        }
    }

    fun start() {
        if (!running) {
            running = true
            handler.post(tick)
        }
    }

    fun stop() {
        running = false
        handler.removeCallbacks(tick)
    }

    private fun sample() {
        val bitmap = previewView.bitmap ?: return
        ensureBackgroundThread {
            val small = Bitmap.createScaledBitmap(bitmap, SAMPLE_SIZE, SAMPLE_SIZE, true)
            val pixels = IntArray(SAMPLE_SIZE * SAMPLE_SIZE)
            small.getPixels(pixels, 0, SAMPLE_SIZE, 0, 0, SAMPLE_SIZE, SAMPLE_SIZE)
            if (small !== bitmap) {
                small.recycle()
            }
            bitmap.recycle()

            val values = HistogramCalculator.normalize(HistogramCalculator.fromArgb(pixels))
            handler.post {
                if (running) {
                    onHistogram(values)
                }
            }
        }
    }
}
