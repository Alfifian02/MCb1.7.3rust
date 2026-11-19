package org.fossify.camera.implementations

import android.annotation.SuppressLint
import android.location.Location
import androidx.camera.video.FileDescriptorOutputOptions
import androidx.camera.video.FileOutputOptions
import androidx.camera.video.MediaStoreOutputOptions
import androidx.camera.video.PendingRecording
import androidx.camera.video.Recorder
import androidx.camera.video.Recording
import androidx.camera.video.VideoCapture
import androidx.camera.video.VideoRecordEvent
import androidx.core.content.ContextCompat
import org.fossify.camera.R
import org.fossify.camera.extensions.config
import org.fossify.camera.helpers.CameraErrorHandler
import org.fossify.camera.helpers.MediaOutputHelper
import org.fossify.camera.helpers.MediaSoundHelper
import org.fossify.camera.models.MediaOutput
import org.fossify.commons.activities.BaseSimpleActivity
import org.fossify.commons.extensions.toast

/** Starts, pauses and stops a recording on the bound [VideoCapture] and reports what happens. */
class VideoRecorder(
    private val activity: BaseSimpleActivity,
    private val mediaOutputHelper: MediaOutputHelper,
    private val mediaSoundHelper: MediaSoundHelper,
    private val cameraErrorHandler: CameraErrorHandler,
    private val listener: CameraXPreviewListener,
    private val isThirdPartyIntent: Boolean,
    private val locationProvider: () -> Location?,
) {
    var videoCapture: VideoCapture<Recorder>? = null

    private val config = activity.config
    private val mainExecutor = ContextCompat.getMainExecutor(activity)
    private var currentRecording: Recording? = null
    private var recordingState: VideoRecordEvent? = null
    private var isPaused = false

    fun toggle() {
        if (currentRecording == null || recordingState is VideoRecordEvent.Finalize) {
            if (config.isSoundEnabled) {
                mediaSoundHelper.playStartVideoRecordingSound(onPlayComplete = { start() })
                listener.onVideoRecordingStarted()
            } else {
                start()
            }
        } else {
            currentRecording?.stop()
            currentRecording = null
            isPaused = false
        }
    }

    fun togglePause() {
        val recording = currentRecording ?: return
        if (isPaused) {
            recording.resume()
        } else {
            recording.pause()
        }
    }

    fun stop() {
        currentRecording?.stop()
    }

    @SuppressLint("MissingPermission", "NewApi")
    private fun start() {
        val capture = videoCapture
        if (capture == null) {
            activity.toast(R.string.camera_open_error)
            return
        }

        val recording = prepareRecording(capture)

        // Third party apps asking for a video always expect sound.
        val audioEnabled = config.recordAudio || isThirdPartyIntent
        currentRecording = (if (audioEnabled) recording.withAudioEnabled() else recording)
            .start(mainExecutor, ::onRecordEvent)
    }

    private fun prepareRecording(capture: VideoCapture<Recorder>): PendingRecording {
        val location = if (config.savePhotoVideoLocation) locationProvider() else null
        val recorder = capture.output
        return when (val mediaOutput = mediaOutputHelper.getVideoMediaOutput()) {
            is MediaOutput.FileDescriptorMediaOutput -> {
                val options = FileDescriptorOutputOptions.Builder(mediaOutput.fileDescriptor)
                    .setLocation(location)
                    .build()
                recorder.prepareRecording(activity, options)
            }

            is MediaOutput.FileMediaOutput -> {
                val options = FileOutputOptions.Builder(mediaOutput.file)
                    .setLocation(location)
                    .build()
                recorder.prepareRecording(activity, options)
            }

            is MediaOutput.MediaStoreOutput -> {
                val options = MediaStoreOutputOptions.Builder(
                    activity.contentResolver,
                    mediaOutput.contentUri
                )
                    .setContentValues(mediaOutput.contentValues)
                    .setLocation(location)
                    .build()
                recorder.prepareRecording(activity, options)
            }
        }
    }

    private fun onRecordEvent(recordEvent: VideoRecordEvent) {
        recordingState = recordEvent
        when (recordEvent) {
            is VideoRecordEvent.Start -> {
                isPaused = false
                listener.onVideoRecordingStarted()
            }

            is VideoRecordEvent.Status -> {
                listener.onVideoDurationChanged(recordEvent.recordingStats.recordedDurationNanos)
            }

            is VideoRecordEvent.Pause -> {
                isPaused = true
                listener.onVideoRecordingPaused()
            }

            is VideoRecordEvent.Resume -> {
                isPaused = false
                listener.onVideoRecordingResumed()
            }

            is VideoRecordEvent.Finalize -> onFinalized(recordEvent)
        }
    }

    private fun onFinalized(event: VideoRecordEvent.Finalize) {
        isPaused = false
        if (config.isSoundEnabled) {
            mediaSoundHelper.playStopVideoRecordingSound()
        }

        listener.onVideoRecordingStopped()
        if (event.hasError()) {
            cameraErrorHandler.handleVideoRecordingError(event.error)
        } else {
            listener.onMediaSaved(event.outputResults.outputUri)
        }
    }
}
