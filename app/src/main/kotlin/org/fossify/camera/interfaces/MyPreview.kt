package org.fossify.camera.interfaces

import org.fossify.camera.models.ManualSettings
import org.fossify.camera.models.SceneMode
import org.fossify.camera.models.VideoFrameRate

interface MyPreview {

    fun isInPhotoMode(): Boolean

    fun setFlashlightState(state: Int)

    fun toggleFrontBackCamera()

    fun handleFlashlightClick()

    fun tryTakePicture()

    fun toggleRecording()

    fun toggleRecordingPause()

    fun setZoomRatio(ratio: Float)

    /** Multiplies the current zoom ratio by [factor], clamped to what the camera supports. */
    fun zoomBy(factor: Float)

    /** @param index exposure compensation index, in steps (see the exposure state callback) */
    fun setExposureIndex(index: Int)

    fun setSceneMode(mode: SceneMode)

    fun initPhotoMode()

    fun initVideoMode()

    fun showChangeResolution()

    fun setVideoFrameRate(rate: VideoFrameRate)

    fun setVideoStabilization(enabled: Boolean)

    /** Takes effect on the next photo; the camera is rebound if the format has to change. */
    fun setRawEnabled(enabled: Boolean)

    fun setManualSettings(settings: ManualSettings)

    /** True while a burst or time lapse is running. */
    fun isCaptureSequenceRunning(): Boolean

    fun stopCaptureSequence()
}
