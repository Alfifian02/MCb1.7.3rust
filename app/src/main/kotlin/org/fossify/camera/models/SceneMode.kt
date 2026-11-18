package org.fossify.camera.models

import androidx.annotation.StringRes
import androidx.camera.extensions.ExtensionMode
import org.fossify.camera.R

/**
 * Photo scene modes backed by the vendor camera extensions (CameraX Extensions API).
 * Only the modes reported as available by the device are shown to the user.
 */
enum class SceneMode(val extensionMode: Int, @StringRes val stringResId: Int) {
    OFF(ExtensionMode.NONE, R.string.scene_off),
    AUTO(ExtensionMode.AUTO, R.string.scene_auto),
    HDR(ExtensionMode.HDR, R.string.scene_hdr),
    NIGHT(ExtensionMode.NIGHT, R.string.scene_night),
    PORTRAIT(ExtensionMode.BOKEH, R.string.scene_portrait),
    FACE_RETOUCH(ExtensionMode.FACE_RETOUCH, R.string.scene_face_retouch)
}
