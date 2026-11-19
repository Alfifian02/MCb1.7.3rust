package org.fossify.camera.helpers

/**
 * Runs several pictures after each other: a burst (back to back) or a time lapse (with a pause).
 * It knows nothing about the camera, so the timing logic can be unit tested.
 */
class CaptureSequencer(
    private val scheduler: Scheduler,
    private val captureOne: (onFinished: (success: Boolean) -> Unit) -> Unit,
    private val listener: Listener,
) {
    interface Scheduler {
        fun postDelayed(delayMs: Long, task: () -> Unit)
        fun cancelPending()
    }

    interface Listener {
        /** [total] is null when the sequence only ends when the user stops it. */
        fun onProgress(taken: Int, total: Int?)
        fun onFinished(taken: Int, completedAll: Boolean)
    }

    companion object {
        const val MAX_TIMELAPSE_FRAMES = 1000
    }

    var isRunning = false
        private set

    // Callbacks of a sequence that was stopped must not touch the next one.
    private var generation = 0
    private var taken = 0
    private var frames = 0
    private var intervalMs = 0L
    private var showTotal = false

    fun startBurst(frameCount: Int) = start(frameCount, 0L, true)

    fun startTimelapse(pauseMs: Long) = start(MAX_TIMELAPSE_FRAMES, pauseMs, false)

    fun stop() {
        if (isRunning) {
            finish(completedAll = false)
        }
    }

    private fun start(frameCount: Int, pauseMs: Long, total: Boolean) {
        if (isRunning || frameCount <= 0) {
            return
        }

        isRunning = true
        generation++
        taken = 0
        frames = frameCount
        intervalMs = pauseMs
        showTotal = total
        captureNext(generation)
    }

    private fun captureNext(expected: Int) {
        if (isRunning && expected == generation) {
            captureOne { success -> onCaptured(expected, success) }
        }
    }

    private fun onCaptured(expected: Int, success: Boolean) {
        if (!isRunning || expected != generation) {
            return
        }

        if (!success) {
            finish(completedAll = false)
            return
        }

        taken++
        listener.onProgress(taken, if (showTotal) frames else null)
        when {
            taken >= frames -> finish(completedAll = true)
            intervalMs <= 0L -> captureNext(expected)
            else -> scheduler.postDelayed(intervalMs) { captureNext(expected) }
        }
    }

    private fun finish(completedAll: Boolean) {
        isRunning = false
        generation++
        scheduler.cancelPending()
        listener.onFinished(taken, completedAll)
    }
}
