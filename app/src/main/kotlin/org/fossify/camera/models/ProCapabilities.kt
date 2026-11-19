package org.fossify.camera.models

/** What the sensor allows for manual exposure and white balance. */
data class ManualCapabilities(
    val exposureSupported: Boolean,
    val isoMin: Int,
    val isoMax: Int,
    val shutterMinNs: Long,
    val shutterMaxNs: Long,
    /** Longest shutter time that still fits the frame duration of a video. */
    val videoShutterMaxNs: Long,
    val whiteBalanceModes: List<WhiteBalanceMode>,
)

/** Everything the "pro" panel needs to know about the camera that is currently bound. */
data class ProCapabilities(
    val frameRates: List<VideoFrameRate>,
    val stabilizationSupported: Boolean,
    val rawSupported: Boolean,
    val manual: ManualCapabilities?,
)

/** The user's manual choices. Nothing here is persisted: it resets with the app, like on a DSLR. */
data class ManualSettings(
    val manualExposure: Boolean = false,
    val iso: Int = 0,
    val shutterNs: Long = 0L,
    val whiteBalance: WhiteBalanceMode = WhiteBalanceMode.AUTO,
) {
    val isDefault: Boolean get() = !manualExposure && whiteBalance == WhiteBalanceMode.AUTO
}
