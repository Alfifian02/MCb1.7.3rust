package org.fossify.camera.extensions

import android.content.Context
import org.fossify.camera.helpers.Config
import org.fossify.commons.extensions.hasPermission
import org.fossify.commons.helpers.PERMISSION_ACCESS_COARSE_LOCATION
import org.fossify.commons.helpers.PERMISSION_ACCESS_FINE_LOCATION
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

val Context.config: Config get() = Config.newInstance(applicationContext)

fun Context.getOutputMediaFilePath(isPhoto: Boolean): String {
    val mediaStorageDir = File(config.savePhotosFolder)

    if (!mediaStorageDir.exists()) {
        if (!mediaStorageDir.mkdirs()) {
            return ""
        }
    }

    val extension = if (isPhoto) "jpg" else "mp4"
    return getUniqueFile(mediaStorageDir, getRandomMediaName(isPhoto), extension).path
}

/**
 * The timestamp in the default file name only has a one second resolution. Two pictures taken
 * within the same second (timer, hardware key, quick taps) would otherwise overwrite each other.
 */
private fun getUniqueFile(directory: File, baseName: String, extension: String): File {
    var file = File(directory, "$baseName.$extension")
    var counter = 1
    while (file.exists() && counter < MAX_UNIQUE_NAME_ATTEMPTS) {
        file = File(directory, "${baseName}_$counter.$extension")
        counter++
    }
    return file
}

private const val MAX_UNIQUE_NAME_ATTEMPTS = 1000

fun Context.getOutputMediaFileName(isPhoto: Boolean): String {
    val mediaName = getRandomMediaName(isPhoto)
    return if (isPhoto) {
        "$mediaName.jpg"
    } else {
        "$mediaName.mp4"
    }
}

fun getRandomMediaName(isPhoto: Boolean): String {
    val timestamp = SimpleDateFormat("yyyyMMdd_HHmmss", Locale.getDefault()).format(Date())
    return if (isPhoto) {
        "IMG_$timestamp"
    } else {
        "VID_$timestamp"
    }
}

fun Context.checkLocationPermission(): Boolean {
    return hasPermission(PERMISSION_ACCESS_FINE_LOCATION) || hasPermission(
        PERMISSION_ACCESS_COARSE_LOCATION
    )
}
