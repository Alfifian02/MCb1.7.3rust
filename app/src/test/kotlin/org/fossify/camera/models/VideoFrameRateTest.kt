package org.fossify.camera.models

import org.junit.Assert.assertEquals
import org.junit.Test

class VideoFrameRateTest {

    @Test
    fun a30FpsCameraOffersAutoAnd30() {
        assertEquals(
            listOf(VideoFrameRate.AUTO, VideoFrameRate.FPS_30),
            VideoFrameRate.supportedFor(30)
        )
    }

    @Test
    fun a60FpsCameraOffersEverything() {
        assertEquals(VideoFrameRate.entries, VideoFrameRate.supportedFor(120))
    }

    @Test
    fun autoIsAlwaysOffered() {
        assertEquals(listOf(VideoFrameRate.AUTO), VideoFrameRate.supportedFor(0))
    }
}
