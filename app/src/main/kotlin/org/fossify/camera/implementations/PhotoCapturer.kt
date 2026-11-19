package org.fossify.camera.implementations

import android.location.Location
import androidx.camera.core.ImageCapture
import androidx.camera.core.ImageCapture.ERROR_CAPTURE_FAILED
import androidx.camera.core.ImageCapture.Metadata
import androidx.camera.core.ImageCapture.OnImageCapturedCallback
import androidx.camera.core.ImageCaptureException
import androidx.camera.core.ImageProxy
import androidx.core.content.ContextCompat
import org.fossify.camera.extensions.config
import org.fossify.camera.helpers.BitmapUtils
import org.fossify.camera.helpers.CameraErrorHandler
import org.fossify.camera.helpers.ImageSaver
import org.fossify.camera.helpers.ImageUtil
import org.fossify.camera.helpers.MediaOutputHelper
import org.fossify.camera.helpers.MediaSoundHelper
import org.fossify.camera.helpers.RawCaptureHelper
import org.fossify.camera.models.MediaOutput
import org.fossify.commons.activities.BaseSimpleActivity
import org.fossify.commons.helpers.ensureBackgroundThread

/** Takes one picture (JPEG, or DNG for RAW), saves it and reports back on the main thread. */
class PhotoCapturer(
    private val activity: BaseSimpleActivity,
    private val mediaOutputHelper: MediaOutputHelper,
    private val mediaSoundHelper: MediaSoundHelper,
    private val cameraErrorHandler: CameraErrorHandler,
    private val listener: CameraXPreviewListener,
    private val locationProvider: () -> Location?,
) {
    private val config = activity.config
    private val contentResolver = activity.contentResolver
    private val mainExecutor = ContextCompat.getMainExecutor(activity)

    /** [onDone] is called on the main thread with whether the picture was saved. */
    fun take(
        capture: ImageCapture,
        raw: Boolean,
        isFrontCamera: Boolean,
        onDone: ((Boolean) -> Unit)?,
    ) {
        if (raw) {
            takeRaw(capture, onDone)
        } else {
            takeJpeg(capture, isFrontCamera, onDone)
        }
    }

    private fun takeJpeg(capture: ImageCapture, isFrontCamera: Boolean, onDone: ((Boolean) -> Unit)?) {
        val metadata = Metadata().apply {
            isReversedHorizontal = isFrontCamera && config.flipPhotos
            if (config.savePhotoVideoLocation) {
                location = locationProvider()
            }
        }

        val mediaOutput = mediaOutputHelper.getImageMediaOutput()
        capture.takePicture(mainExecutor, object : OnImageCapturedCallback() {
            override fun onCaptureSuccess(image: ImageProxy) {
                listener.shutterAnimation()
                playShutterSoundIfEnabled()
                ensureBackgroundThread {
                    image.use { save(image, mediaOutput, metadata, onDone) }
                }
            }

            override fun onError(exception: ImageCaptureException) {
                handleError(exception)
                onDone?.invoke(false)
            }
        })
    }

    private fun save(
        image: ImageProxy,
        mediaOutput: MediaOutput.ImageCaptureOutput,
        metadata: Metadata,
        onDone: ((Boolean) -> Unit)?,
    ) {
        if (mediaOutput is MediaOutput.BitmapOutput) {
            val imageBytes = ImageUtil.jpegImageToJpegByteArray(image)
            val bitmap = BitmapUtils.makeBitmap(imageBytes)
            activity.runOnUiThread {
                listener.onPhotoCaptureEnd()
                if (bitmap != null) {
                    listener.onImageCaptured(bitmap)
                } else {
                    cameraErrorHandler.handleImageCaptureError(ERROR_CAPTURE_FAILED)
                }
                onDone?.invoke(bitmap != null)
            }
        } else {
            ImageSaver.saveImage(
                contentResolver = contentResolver,
                image = image,
                mediaOutput = mediaOutput,
                metadata = metadata,
                jpegQuality = config.photoQuality,
                saveExifAttributes = config.savePhotoMetadata,
                onImageSaved = { savedUri ->
                    activity.runOnUiThread {
                        listener.onPhotoCaptureEnd()
                        listener.onMediaSaved(savedUri)
                        onDone?.invoke(true)
                    }
                },
                onError = { exception ->
                    handleError(exception)
                    activity.runOnUiThread { onDone?.invoke(false) }
                }
            )
        }
    }

    private fun takeRaw(capture: ImageCapture, onDone: ((Boolean) -> Unit)?) {
        val options = RawCaptureHelper.buildOutputOptions(contentResolver)
        capture.takePicture(options, mainExecutor, object : ImageCapture.OnImageSavedCallback {
            override fun onImageSaved(outputFileResults: ImageCapture.OutputFileResults) {
                listener.shutterAnimation()
                playShutterSoundIfEnabled()
                listener.onPhotoCaptureEnd()
                outputFileResults.savedUri?.let { listener.onMediaSaved(it) }
                onDone?.invoke(true)
            }

            override fun onError(exception: ImageCaptureException) {
                handleError(exception)
                onDone?.invoke(false)
            }
        })
    }

    private fun handleError(exception: ImageCaptureException) {
        listener.onPhotoCaptureEnd()
        cameraErrorHandler.handleImageCaptureError(exception.imageCaptureError)
    }

    private fun playShutterSoundIfEnabled() {
        if (config.isSoundEnabled) {
            mediaSoundHelper.playShutterSound()
        }
    }
}
