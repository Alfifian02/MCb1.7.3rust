package org.fossify.camera.ui

import android.content.Context
import androidx.camera.view.PreviewView
import org.fossify.camera.R
import org.fossify.camera.databinding.LayoutProPanelBinding
import org.fossify.camera.extensions.config
import org.fossify.camera.helpers.HistogramSampler
import org.fossify.camera.helpers.ManualExposureLadder
import org.fossify.camera.interfaces.MyPreview
import org.fossify.camera.models.BurstSize
import org.fossify.camera.models.DriveMode
import org.fossify.camera.models.ManualCapabilities
import org.fossify.camera.models.ManualSettings
import org.fossify.camera.models.ProCapabilities
import org.fossify.camera.models.TimelapseInterval
import org.fossify.camera.models.VideoFrameRate
import org.fossify.camera.models.WhiteBalanceMode
import org.fossify.camera.views.HistogramView
import org.fossify.commons.extensions.beGone
import org.fossify.commons.extensions.beVisible
import org.fossify.commons.extensions.beVisibleIf
import org.fossify.commons.extensions.getProperPrimaryColor
import org.fossify.commons.extensions.isVisible

/**
 * The panel behind the "pro" button: video frame rate and stabilization, burst and time lapse,
 * RAW, manual exposure, white balance and the histogram. It only shows what the camera supports.
 */
class ProControlsPanel(
    context: Context,
    private val binding: LayoutProPanelBinding,
    private val histogramView: HistogramView,
    previewView: PreviewView,
    private val preview: () -> MyPreview?,
) {
    private companion object {
        private const val NANOS_PER_SECOND = 1_000_000_000L
        private const val DEFAULT_VIDEO_FPS = 30
        private const val DEFAULT_ISO = 400
        private const val DEFAULT_SHUTTER_NS = NANOS_PER_SECOND / 60
        private const val MILLIS_PER_SECOND = 1000
    }

    private val config = context.config
    private val selectedColor = { context.getProperPrimaryColor() }
    private val sampler = HistogramSampler(previewView) { histogramView.setHistogram(it) }

    private val frameRateRow = OptionChipRow<VideoFrameRate>(binding.frameRateRow, selectedColor) {
        it.fps?.let { fps -> context.getString(R.string.fps_value, fps) } ?: context.getString(R.string.fps_auto)
    }
    private val driveRow = OptionChipRow<DriveMode>(binding.driveModeRow, selectedColor) {
        context.getString(it.stringResId)
    }
    private val burstRow = OptionChipRow<BurstSize>(binding.burstSizeRow, selectedColor) {
        it.frames.toString()
    }
    private val intervalRow = OptionChipRow<TimelapseInterval>(binding.intervalRow, selectedColor) {
        context.getString(R.string.seconds_short, (it.millis / MILLIS_PER_SECOND).toInt())
    }
    private val whiteBalanceRow = OptionChipRow<WhiteBalanceMode>(binding.whiteBalanceRow, selectedColor) {
        context.getString(it.stringResId)
    }

    private var capabilities: ProCapabilities? = null
    private var isPhotoMode = true
    private var manual = ManualSettings()
    private var isoSteps = emptyList<Int>()
    private var shutterSteps = emptyList<Long>()
    private var refreshing = false

    val isVisible: Boolean get() = binding.root.isVisible()

    init {
        histogramView.beVisibleIf(config.showHistogram)
        binding.stabilizationSwitch.setOnCheckedChangeListener { _, checked ->
            if (!refreshing) preview()?.setVideoStabilization(checked)
        }
        binding.rawSwitch.setOnCheckedChangeListener { _, checked ->
            if (!refreshing) preview()?.setRawEnabled(checked)
        }
        binding.histogramSwitch.setOnCheckedChangeListener { _, checked ->
            if (refreshing) return@setOnCheckedChangeListener
            config.showHistogram = checked
            histogramView.beVisibleIf(checked)
            if (checked) sampler.start() else sampler.stop()
        }
        binding.manualExposureSwitch.setOnCheckedChangeListener { _, checked ->
            if (!refreshing) {
                updateManual(manual.copy(manualExposure = checked))
                refreshManual()
            }
        }
        binding.isoSeekbar.setOnSeekBarChangeListener(ProgressChangeListener { progress ->
            isoSteps.getOrNull(progress)?.let { updateManual(manual.copy(iso = it)) }
        })
        binding.shutterSeekbar.setOnSeekBarChangeListener(ProgressChangeListener { progress ->
            shutterSteps.getOrNull(progress)?.let { updateManual(manual.copy(shutterNs = it)) }
        })
    }

    fun toggle() {
        if (isVisible) {
            hide()
        } else {
            binding.root.beVisible()
        }
    }

    /** @return whether the panel was open. */
    fun hide(): Boolean {
        val wasOpen = isVisible
        binding.root.beGone()
        return wasOpen
    }

    fun setPhotoMode(isPhoto: Boolean) {
        isPhotoMode = isPhoto
        refresh()
    }

    fun onCapabilities(newCapabilities: ProCapabilities) {
        capabilities = newCapabilities
        refresh()
    }

    fun onResume() {
        if (config.showHistogram) sampler.start()
    }

    fun onPause() {
        sampler.stop()
    }

    private fun refresh() {
        refreshing = true
        val caps = capabilities
        binding.sectionVideo.beVisibleIf(!isPhotoMode && caps != null)
        binding.sectionPhoto.beVisibleIf(isPhotoMode)
        binding.histogramSwitch.isChecked = config.showHistogram

        if (caps != null) {
            frameRateRow.show(caps.frameRates, config.videoFrameRate) { preview()?.setVideoFrameRate(it) }
            binding.stabilizationSwitch.beVisibleIf(caps.stabilizationSupported)
            binding.stabilizationSwitch.isChecked = config.videoStabilization
            binding.rawSwitch.beVisibleIf(caps.rawSupported)
            binding.rawSwitch.isChecked = config.rawEnabled
        }

        val driveMode = config.driveMode
        driveRow.show(DriveMode.entries, driveMode) {
            config.driveMode = it
            refresh()
        }
        binding.burstSizeScroll.beVisibleIf(driveMode == DriveMode.BURST)
        binding.intervalScroll.beVisibleIf(driveMode == DriveMode.TIMELAPSE)
        burstRow.show(BurstSize.entries, config.burstSize) { config.burstSize = it }
        intervalRow.show(TimelapseInterval.entries, config.timelapseInterval) { config.timelapseInterval = it }

        refreshManual()
        refreshing = false
    }

    private fun refreshManual() {
        val caps = capabilities?.manual
        binding.sectionManual.beVisibleIf(caps != null)
        if (caps == null) {
            return
        }

        binding.manualExposureSwitch.beVisibleIf(caps.exposureSupported)
        binding.manualExposureSwitch.isChecked = manual.manualExposure
        binding.manualExposureHolder.beVisibleIf(caps.exposureSupported && manual.manualExposure)
        if (caps.exposureSupported) {
            prepareExposureSteps(caps)
        }

        binding.whiteBalanceSection.beVisibleIf(caps.whiteBalanceModes.size > 1)
        whiteBalanceRow.show(caps.whiteBalanceModes, manual.whiteBalance) {
            updateManual(manual.copy(whiteBalance = it))
        }
    }

    private fun prepareExposureSteps(caps: ManualCapabilities) {
        val shutterMax = if (isPhotoMode) {
            caps.shutterMaxNs
        } else {
            minOf(caps.videoShutterMaxNs, NANOS_PER_SECOND / (config.videoFrameRate.fps ?: DEFAULT_VIDEO_FPS))
        }

        isoSteps = ManualExposureLadder.isoSteps(caps.isoMin, caps.isoMax)
        shutterSteps = ManualExposureLadder.shutterSteps(caps.shutterMinNs, shutterMax)

        val isoIndex = ManualExposureLadder.closestIndex(
            isoSteps.map { it.toLong() },
            (if (manual.iso > 0) manual.iso else DEFAULT_ISO).toLong()
        )
        val shutterIndex = ManualExposureLadder.closestIndex(
            shutterSteps,
            if (manual.shutterNs > 0L) manual.shutterNs else DEFAULT_SHUTTER_NS
        )

        binding.isoSeekbar.max = isoSteps.lastIndex
        binding.isoSeekbar.progress = isoIndex
        binding.shutterSeekbar.max = shutterSteps.lastIndex
        binding.shutterSeekbar.progress = shutterIndex
        updateManual(manual.copy(iso = isoSteps[isoIndex], shutterNs = shutterSteps[shutterIndex]))
    }

    private fun updateManual(settings: ManualSettings) {
        manual = settings
        binding.isoValue.text = settings.iso.toString()
        binding.shutterValue.text = ManualExposureLadder.formatShutter(settings.shutterNs)
        preview()?.setManualSettings(settings)
    }
}
