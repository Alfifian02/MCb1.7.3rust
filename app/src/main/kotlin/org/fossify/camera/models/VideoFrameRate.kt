package org.fossify.camera.models

/** Target frame rate for video. [AUTO] leaves the choice to the camera. */
enum class VideoFrameRate(val fps: Int?) {
    AUTO(null),
    FPS_30(30),
    FPS_60(60);

    companion object {
        /** The rates worth offering for a camera whose frame rate ranges reach [maxFps]. */
        fun supportedFor(maxFps: Int): List<VideoFrameRate> {
            return entries.filter { rate -> rate.fps?.let { it <= maxFps } ?: true }
        }
    }
}
