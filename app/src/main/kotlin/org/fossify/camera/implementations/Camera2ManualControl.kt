package org.fossify.camera.implementations

import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraMetadata
import android.hardware.camera2.CaptureRequest
import androidx.camera.camera2.interop.Camera2CameraControl
import androidx.camera.camera2.interop.Camera2CameraInfo
import androidx.camera.camera2.interop.CaptureRequestOptions
import androidx.camera.camera2.interop.ExperimentalCamera2Interop
import androidx.camera.core.CameraControl
import androidx.camera.core.CameraInfo
import org.fossify.camera.models.ManualCapabilities
import org.fossify.camera.models.ManualSettings
import org.fossify.camera.models.WhiteBalanceMode

/**
 * ISO, shutter speed and white balance. CameraX has no API for these, so they go straight to
 * Camera2 through the interop module. Whether a phone honours them is up to its camera HAL.
 */
@androidx.annotation.OptIn(markerClass = [ExperimentalCamera2Interop::class])
object Camera2ManualControl {
    private const val NANOS_PER_SECOND = 1_000_000_000L
    private const val SLOWEST_VIDEO_FPS = 30

    /** @return null if the camera exposes neither manual exposure nor white balance presets. */
    @Suppress("SwallowedException")
    fun readCapabilities(cameraInfo: CameraInfo): ManualCapabilities? {
        return try {
            val info = Camera2CameraInfo.from(cameraInfo)
            val capabilities =
                info.getCameraCharacteristic(CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES)
            val hasManualSensor = capabilities
                ?.contains(CameraMetadata.REQUEST_AVAILABLE_CAPABILITIES_MANUAL_SENSOR) == true
            val isoRange = info.getCameraCharacteristic(CameraCharacteristics.SENSOR_INFO_SENSITIVITY_RANGE)
            val shutterRange =
                info.getCameraCharacteristic(CameraCharacteristics.SENSOR_INFO_EXPOSURE_TIME_RANGE)
            val whiteBalance = WhiteBalanceMode.availableFrom(
                info.getCameraCharacteristic(CameraCharacteristics.CONTROL_AWB_AVAILABLE_MODES)
                    ?: IntArray(0)
            )

            val exposureSupported = hasManualSensor && isoRange != null && shutterRange != null
            if (!exposureSupported && whiteBalance.size <= 1) {
                return null
            }

            val shutterMax = shutterRange?.upper ?: 0L
            ManualCapabilities(
                exposureSupported = exposureSupported,
                isoMin = isoRange?.lower ?: 0,
                isoMax = isoRange?.upper ?: 0,
                shutterMinNs = shutterRange?.lower ?: 0L,
                shutterMaxNs = shutterMax,
                videoShutterMaxNs = minOf(shutterMax, NANOS_PER_SECOND / SLOWEST_VIDEO_FPS),
                whiteBalanceModes = whiteBalance
            )
        } catch (e: IllegalArgumentException) {
            // Not a Camera2 backed camera (e.g. a vendor extension), nothing to control.
            null
        }
    }

    /** Replaces all manual overrides. Default settings hand control back to the camera. */
    @Suppress("SwallowedException")
    fun apply(cameraControl: CameraControl, settings: ManualSettings) {
        try {
            val options = CaptureRequestOptions.Builder()
            if (settings.manualExposure && settings.iso > 0 && settings.shutterNs > 0L) {
                options.setCaptureRequestOption(
                    CaptureRequest.CONTROL_AE_MODE,
                    CameraMetadata.CONTROL_AE_MODE_OFF
                )
                options.setCaptureRequestOption(CaptureRequest.SENSOR_SENSITIVITY, settings.iso)
                options.setCaptureRequestOption(CaptureRequest.SENSOR_EXPOSURE_TIME, settings.shutterNs)
            }

            if (settings.whiteBalance != WhiteBalanceMode.AUTO) {
                options.setCaptureRequestOption(
                    CaptureRequest.CONTROL_AWB_MODE,
                    settings.whiteBalance.camera2Mode
                )
            }

            Camera2CameraControl.from(cameraControl).setCaptureRequestOptions(options.build())
        } catch (e: IllegalArgumentException) {
            // Not a Camera2 backed camera, the manual settings simply have no effect.
        }
    }
}
