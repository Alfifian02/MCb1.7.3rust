package org.fossify.camera.models

import androidx.annotation.StringRes
import org.fossify.camera.R

enum class VolumeButtonAction(@StringRes val stringResId: Int) {
    NONE(R.string.volume_action_none),
    SHUTTER(R.string.volume_action_shutter),
    ZOOM(R.string.volume_action_zoom)
}
