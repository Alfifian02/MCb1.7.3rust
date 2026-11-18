package org.fossify.camera.interfaces

import org.fossify.camera.models.SceneMode

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
}
