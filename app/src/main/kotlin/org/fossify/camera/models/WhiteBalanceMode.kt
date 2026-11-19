package org.fossify.camera.models

import android.hardware.camera2.CameraMetadata
import androidx.annotation.StringRes
import org.fossify.camera.R

enum class WhiteBalanceMode(val camera2Mode: Int, @StringRes val stringResId: Int) {
    AUTO(CameraMetadata.CONTROL_AWB_MODE_AUTO, R.string.wb_auto),
    INCANDESCENT(CameraMetadata.CONTROL_AWB_MODE_INCANDESCENT, R.string.wb_incandescent),
    FLUORESCENT(CameraMetadata.CONTROL_AWB_MODE_FLUORESCENT, R.string.wb_fluorescent),
    DAYLIGHT(CameraMetadata.CONTROL_AWB_MODE_DAYLIGHT, R.string.wb_daylight),
    CLOUDY(CameraMetadata.CONTROL_AWB_MODE_CLOUDY_DAYLIGHT, R.string.wb_cloudy);

    companion object {
        /** Auto is always available, the presets only if the camera lists them. */
        fun availableFrom(camera2Modes: IntArray): List<WhiteBalanceMode> {
            return entries.filter { it == AUTO || it.camera2Mode in camera2Modes }
        }
    }
}
