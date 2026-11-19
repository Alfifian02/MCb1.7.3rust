package org.fossify.camera.implementations

import androidx.camera.core.CameraInfo
import androidx.camera.video.Recorder
import org.fossify.camera.helpers.RawCaptureHelper
import org.fossify.camera.models.ProCapabilities
import org.fossify.camera.models.VideoFrameRate

/** Asks the camera what it can do, so the UI only offers what actually works. */
object ProCapabilitiesReader {
    private const val DEFAULT_MAX_FPS = 30

    fun read(cameraInfo: CameraInfo): ProCapabilities {
        val maxFps = cameraInfo.supportedFrameRateRanges.maxOfOrNull { it.upper } ?: DEFAULT_MAX_FPS
        return ProCapabilities(
            frameRates = VideoFrameRate.supportedFor(maxFps),
            stabilizationSupported = Recorder.getVideoCapabilities(cameraInfo).isStabilizationSupported,
            rawSupported = RawCaptureHelper.isSupported(cameraInfo),
            manual = Camera2ManualControl.readCapabilities(cameraInfo)
        )
    }
}
