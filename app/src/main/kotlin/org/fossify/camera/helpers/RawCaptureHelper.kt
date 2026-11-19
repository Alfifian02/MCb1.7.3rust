package org.fossify.camera.helpers

import android.content.ContentResolver
import android.content.ContentValues
import android.os.Environment
import android.provider.MediaStore
import androidx.camera.core.CameraInfo
import androidx.camera.core.ImageCapture
import org.fossify.camera.extensions.getRandomMediaName

/**
 * RAW (DNG) capture. This relies on the RAW output format of CameraX's ImageCapture, which only
 * exists on cameras that advertise it. Everything RAW specific lives here so it stays in one place.
 */
object RawCaptureHelper {
    private const val DNG_MIME_TYPE = "image/x-adobe-dng"
    private const val EXTERNAL_VOLUME = "external"

    fun isSupported(cameraInfo: CameraInfo): Boolean {
        return ImageCapture.getImageCaptureCapabilities(cameraInfo)
            .supportedOutputFormats
            .contains(ImageCapture.OUTPUT_FORMAT_RAW)
    }

    fun configure(builder: ImageCapture.Builder) {
        builder.setOutputFormat(ImageCapture.OUTPUT_FORMAT_RAW)
    }

    /** DNG files always go to DCIM through the MediaStore, whatever the save folder setting is. */
    fun buildOutputOptions(contentResolver: ContentResolver): ImageCapture.OutputFileOptions {
        val values = ContentValues().apply {
            put(MediaStore.MediaColumns.DISPLAY_NAME, getRandomMediaName(isPhoto = true))
            put(MediaStore.MediaColumns.MIME_TYPE, DNG_MIME_TYPE)
            put(MediaStore.MediaColumns.RELATIVE_PATH, Environment.DIRECTORY_DCIM)
        }

        return ImageCapture.OutputFileOptions.Builder(
            contentResolver,
            MediaStore.Images.Media.getContentUri(EXTERNAL_VOLUME),
            values
        ).build()
    }
}
