package org.fossify.camera.models

import androidx.annotation.StringRes
import org.fossify.camera.R

/** How the shutter button behaves in photo mode. */
enum class DriveMode(@StringRes val stringResId: Int) {
    SINGLE(R.string.drive_single),
    BURST(R.string.drive_burst),
    TIMELAPSE(R.string.drive_timelapse)
}

enum class BurstSize(val frames: Int) {
    FIVE(5),
    TEN(10),
    TWENTY(20)
}

/** Pause between two pictures of a time lapse. */
enum class TimelapseInterval(val millis: Long) {
    SECOND_1(1_000L),
    SECOND_2(2_000L),
    SECOND_5(5_000L),
    SECOND_10(10_000L),
    SECOND_30(30_000L)
}
