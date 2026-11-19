package org.fossify.camera.implementations

import android.graphics.Bitmap
import android.net.Uri
import org.fossify.camera.models.ProCapabilities
import org.fossify.camera.models.ResolutionOption
import org.fossify.camera.models.SceneMode

interface CameraXPreviewListener {
    fun onInitPhotoMode()
    fun onInitVideoMode()
    fun setCameraAvailable(available: Boolean) {}
    fun setHasFrontAndBackCamera(hasFrontAndBack: Boolean)
    fun setFlashAvailable(available: Boolean)
    fun onChangeCamera(frontCamera: Boolean) {}
    fun shutterAnimation()
    fun onMediaSaved(uri: Uri)
    fun onImageCaptured(bitmap: Bitmap)
    fun onChangeFlashMode(flashMode: Int)
    fun onPhotoCaptureStart()
    fun onPhotoCaptureEnd()
    fun onVideoRecordingStarted()
    fun onVideoRecordingStopped()
    fun onVideoDurationChanged(durationNanos: Long)
    fun onFocusCamera(xPos: Float, yPos: Float)
    fun onTouchPreview()
    fun displaySelectedResolution(resolutionOption: ResolutionOption)
    fun showImageSizes(
        selectedResolution: ResolutionOption,
        resolutions: List<ResolutionOption>,
        isPhotoCapture: Boolean,
        isFrontCamera: Boolean,
        onSelect: (index: Int, changed: Boolean) -> Unit,
    )

    fun showFlashOptions(photoCapture: Boolean)
    fun adjustPreviewView(requiresCentering: Boolean)

    fun onPreviewResolutionChanged(width: Int, height: Int, isFullScreen: Boolean) {}
    fun onZoomStateChanged(minRatio: Float, maxRatio: Float, zoomRatio: Float) {}
    fun onExposureStateChanged(
        supported: Boolean,
        minIndex: Int,
        maxIndex: Int,
        currentIndex: Int,
        stepEv: Float,
    ) {}

    fun onSceneModesAvailable(modes: List<SceneMode>, selected: SceneMode) {}
    fun onScreenFlashModeChanged(enabled: Boolean) {}
    fun onVideoRecordingPaused() {}
    fun onVideoRecordingResumed() {}
    fun onProCapabilitiesChanged(capabilities: ProCapabilities) {}
    fun onCaptureSequenceProgress(taken: Int, total: Int?) {}
    fun onCaptureSequenceFinished(taken: Int, completedAll: Boolean) {}
}
