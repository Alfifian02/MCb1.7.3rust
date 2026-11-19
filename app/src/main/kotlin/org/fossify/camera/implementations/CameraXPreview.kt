package org.fossify.camera.implementations

import android.annotation.SuppressLint
import android.content.Context
import android.hardware.SensorManager
import android.hardware.display.DisplayManager
import android.os.Handler
import android.os.Looper
import android.util.Range
import android.util.Rational
import android.util.Size
import android.view.Display
import android.view.GestureDetector
import android.view.GestureDetector.SimpleOnGestureListener
import android.view.MotionEvent
import android.view.OrientationEventListener
import android.view.ScaleGestureDetector
import android.view.Surface
import androidx.camera.core.Camera
import androidx.camera.core.CameraSelector
import androidx.camera.core.CameraState
import androidx.camera.core.DisplayOrientedMeteringPointFactory
import androidx.camera.core.FocusMeteringAction
import androidx.camera.core.ImageCapture
import androidx.camera.core.ImageCapture.Builder
import androidx.camera.core.ImageCapture.CAPTURE_MODE_MAXIMIZE_QUALITY
import androidx.camera.core.ImageCapture.CAPTURE_MODE_MINIMIZE_LATENCY
import androidx.camera.core.ImageCapture.FLASH_MODE_AUTO
import androidx.camera.core.ImageCapture.FLASH_MODE_OFF
import androidx.camera.core.ImageCapture.FLASH_MODE_ON
import androidx.camera.core.Preview
import androidx.camera.core.UseCase
import androidx.camera.core.UseCaseGroup
import androidx.camera.core.ViewPort
import androidx.camera.core.ZoomState
import androidx.camera.core.resolutionselector.ResolutionSelector
import androidx.camera.core.resolutionselector.ResolutionSelector.PREFER_CAPTURE_RATE_OVER_HIGHER_RESOLUTION
import androidx.camera.core.resolutionselector.ResolutionSelector.PREFER_HIGHER_RESOLUTION_OVER_CAPTURE_RATE
import androidx.camera.core.resolutionselector.ResolutionStrategy
import androidx.camera.core.resolutionselector.ResolutionStrategy.FALLBACK_RULE_CLOSEST_LOWER_THEN_HIGHER
import androidx.camera.extensions.ExtensionsManager
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.video.FallbackStrategy
import androidx.camera.video.Quality
import androidx.camera.video.QualitySelector
import androidx.camera.video.Recorder
import androidx.camera.video.VideoCapture
import androidx.camera.view.PreviewView
import androidx.camera.view.PreviewView.ScaleType
import androidx.core.content.ContextCompat
import androidx.core.view.doOnLayout
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LiveData
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.Observer
import androidx.window.layout.WindowMetricsCalculator
import com.bumptech.glide.load.ImageHeaderParser.UNKNOWN_ORIENTATION
import org.fossify.camera.R
import org.fossify.camera.extensions.checkLocationPermission
import org.fossify.camera.extensions.config
import org.fossify.camera.extensions.toAppFlashMode
import org.fossify.camera.extensions.toCameraSelector
import org.fossify.camera.extensions.toCameraXFlashMode
import org.fossify.camera.extensions.toCameraXQuality
import org.fossify.camera.extensions.toLensFacing
import org.fossify.camera.helpers.CameraErrorHandler
import org.fossify.camera.helpers.CaptureSequencer
import org.fossify.camera.helpers.FLASH_ALWAYS_ON
import org.fossify.camera.helpers.FLASH_AUTO
import org.fossify.camera.helpers.FLASH_OFF
import org.fossify.camera.helpers.FLASH_ON
import org.fossify.camera.helpers.ImageQualityManager
import org.fossify.camera.helpers.MediaOutputHelper
import org.fossify.camera.helpers.MediaSizeStore
import org.fossify.camera.helpers.MediaSoundHelper
import org.fossify.camera.helpers.PinchToZoomOnScaleGestureListener
import org.fossify.camera.helpers.RawCaptureHelper
import org.fossify.camera.helpers.SimpleLocationManager
import org.fossify.camera.helpers.VideoQualityManager
import org.fossify.camera.interfaces.MyPreview
import org.fossify.camera.models.CaptureMode
import org.fossify.camera.models.DriveMode
import org.fossify.camera.models.ManualSettings
import org.fossify.camera.models.MySize
import org.fossify.camera.models.ProCapabilities
import org.fossify.camera.models.ResolutionOption
import org.fossify.camera.models.SceneMode
import org.fossify.camera.models.VideoFrameRate
import org.fossify.commons.activities.BaseSimpleActivity
import org.fossify.commons.extensions.toast
import org.fossify.commons.helpers.PERMISSION_ACCESS_FINE_LOCATION
import kotlin.math.abs

class CameraXPreview(
    private val activity: BaseSimpleActivity,
    private val previewView: PreviewView,
    private val mediaSoundHelper: MediaSoundHelper,
    private val mediaOutputHelper: MediaOutputHelper,
    private val cameraErrorHandler: CameraErrorHandler,
    private val listener: CameraXPreviewListener,
    private val isThirdPartyIntent: Boolean,
    initInPhotoMode: Boolean,
) : MyPreview, DefaultLifecycleObserver {

    companion object {
        // Auto focus is 1/6 of the area.
        private const val AF_SIZE = 1.0f / 6.0f
        private const val AE_SIZE = AF_SIZE * 1.5f
        private const val CAMERA_MODE_SWITCH_WAIT_TIME = 500L
    }

    private val config = activity.config
    private val contentResolver = activity.contentResolver
    private val mainExecutor = ContextCompat.getMainExecutor(activity)
    private val displayManager =
        activity.getSystemService(Context.DISPLAY_SERVICE) as DisplayManager
    private val windowMetricsCalculator = WindowMetricsCalculator.getOrCreate()
    private val videoQualityManager = VideoQualityManager(activity)
    private val imageQualityManager = ImageQualityManager(activity)
    private val mediaSizeStore = MediaSizeStore(config)

    private val orientationEventListener =
        object : OrientationEventListener(activity, SensorManager.SENSOR_DELAY_NORMAL) {
            @SuppressLint("RestrictedApi")
            override fun onOrientationChanged(orientation: Int) {
                if (orientation == UNKNOWN_ORIENTATION) {
                    return
                }

                val rotation = when (orientation) {
                    in 45 until 135 -> Surface.ROTATION_270
                    in 135 until 225 -> Surface.ROTATION_180
                    in 225 until 315 -> Surface.ROTATION_90
                    else -> Surface.ROTATION_0
                }

                if (lastRotation != rotation) {
                    preview?.targetRotation = rotation
                    imageCapture?.targetRotation = rotation
                    videoCapture?.targetRotation = rotation
                    lastRotation = rotation
                }
            }
        }
    private val cameraHandler = Handler(Looper.getMainLooper())
    private val photoModeRunnable = Runnable {
        if (imageCapture == null) {
            isPhotoCapture = true
            if (!isThirdPartyIntent) { // we don't want to store the state for 3rd party intents
                config.initPhotoMode = true
            }
            startCamera()
        } else {
            listener.onInitPhotoMode()
        }
    }
    private val videoModeRunnable = Runnable {
        if (videoCapture == null) {
            isPhotoCapture = false
            if (!isThirdPartyIntent) { // we don't want to store the state for 3rd party intents
                config.initPhotoMode = false
            }
            startCamera()
        } else {
            listener.onInitVideoMode()
        }
    }

    private var preview: Preview? = null
    private var cameraProvider: ProcessCameraProvider? = null
    private var imageCapture: ImageCapture? = null
    private var videoCapture: VideoCapture<Recorder>? = null
    private var camera: Camera? = null
    private val videoRecorder = VideoRecorder(
        activity, mediaOutputHelper, mediaSoundHelper, cameraErrorHandler, listener, isThirdPartyIntent
    ) { simpleLocationManager?.getLocation() }
    private val photoCapturer = PhotoCapturer(
        activity, mediaOutputHelper, mediaSoundHelper, cameraErrorHandler, listener
    ) { simpleLocationManager?.getLocation() }
    private var cameraSelector = if (config.alwaysOpenBackCamera) {
        CameraSelector.DEFAULT_BACK_CAMERA
    } else {
        config.lastUsedCameraLens.toCameraSelector()
    }
    private var flashMode = FLASH_MODE_OFF
    private var isPhotoCapture = initInPhotoMode
    private var lastRotation = 0
    private var lastCameraStartTime = 0L
    private var simpleLocationManager: SimpleLocationManager? = null
    private var extensionsManager: ExtensionsManager? = null
    private var activeSceneMode = SceneMode.OFF
    private var proCapabilities: ProCapabilities? = null
    private var isRawActive = false
    private var manualSettings = ManualSettings()

    // Bursts and time lapses: the handler token lets pending frames be cancelled in one call.
    private val sequenceToken = Any()
    private val sequenceScheduler = object : CaptureSequencer.Scheduler {
        override fun postDelayed(delayMs: Long, task: () -> Unit) {
            cameraHandler.postDelayed({ task() }, sequenceToken, delayMs)
        }

        override fun cancelPending() {
            cameraHandler.removeCallbacksAndMessages(sequenceToken)
        }
    }
    private val sequenceListener = object : CaptureSequencer.Listener {
        override fun onProgress(taken: Int, total: Int?) {
            listener.onCaptureSequenceProgress(taken, total)
        }

        override fun onFinished(taken: Int, completedAll: Boolean) {
            listener.onCaptureSequenceFinished(taken, completedAll)
        }
    }
    private val captureSequencer = CaptureSequencer(
        scheduler = sequenceScheduler,
        captureOne = { onFinished -> takePicture(onFinished) },
        listener = sequenceListener
    )

    // Observers are kept so they can be detached when the camera is rebound. Otherwise every
    // camera restart added another observer (duplicated error toasts, leaked callbacks).
    private var cameraStateLiveData: LiveData<CameraState>? = null
    private var zoomStateLiveData: LiveData<ZoomState>? = null

    private val cameraStateObserver = Observer<CameraState> { cameraState ->
        val error = cameraState.error
        if (error == null) {
            when (cameraState.type) {
                CameraState.Type.OPENING,
                CameraState.Type.OPEN -> {
                    listener.setHasFrontAndBackCamera(hasFrontCamera() && hasBackCamera())
                    listener.setCameraAvailable(true)
                }

                CameraState.Type.PENDING_OPEN,
                CameraState.Type.CLOSING,
                CameraState.Type.CLOSED -> {
                    listener.setCameraAvailable(false)
                }
            }
        } else if (error.code == CameraState.ERROR_STREAM_CONFIG && activeSceneMode != SceneMode.OFF) {
            // The vendor extension can't be configured with the selected resolution.
            handleSceneModeFailure()
        } else {
            listener.setCameraAvailable(false)
            cameraErrorHandler.handleCameraError(error)
        }
    }

    private val zoomStateObserver = Observer<ZoomState> { zoomState ->
        listener.onZoomStateChanged(
            minRatio = zoomState.minZoomRatio,
            maxRatio = zoomState.maxZoomRatio,
            zoomRatio = zoomState.zoomRatio
        )
    }

    init {
        if (config.alwaysOpenBackCamera) {
            config.lastUsedCameraLens = CameraSelector.LENS_FACING_BACK
        }

        if (config.turnFlashOffAtStartup) {
            config.flashlightState = FLASH_OFF
        }

        bindToLifeCycle()
    }

    private fun bindToLifeCycle() {
        activity.lifecycle.addObserver(this)
    }

    private fun startCamera(switching: Boolean = false) {
        captureSequencer.stop()
        val cameraProviderFuture = ProcessCameraProvider.getInstance(activity.applicationContext)
        cameraProviderFuture.addListener({
            try {
                val provider = cameraProviderFuture.get()
                cameraProvider = provider
                ensureExtensionsManager(provider) { initializeAndBind(provider, switching) }
            } catch (e: Exception) {
                showStartError(switching)
            }
        }, mainExecutor)
    }

    private fun initializeAndBind(provider: ProcessCameraProvider, switching: Boolean) {
        try {
            imageQualityManager.initSupportedQualities()
            videoQualityManager.initSupportedQualities(provider)
            if (bindCameraUseCases()) {
                setupCameraObservers()
            }
        } catch (e: Exception) {
            showStartError(switching)
        }
    }

    private fun showStartError(switching: Boolean) {
        val errorMessage =
            if (switching) R.string.camera_switch_error else R.string.camera_open_error
        activity.toast(errorMessage)
    }

    /**
     * Loads the CameraX extensions (HDR, night, portrait...) once. The camera still starts
     * normally if the device doesn't support them or loading fails.
     */
    private fun ensureExtensionsManager(provider: ProcessCameraProvider, onReady: () -> Unit) {
        if (extensionsManager != null) {
            onReady()
            return
        }

        try {
            val extensionsFuture =
                ExtensionsManager.getInstanceAsync(activity.applicationContext, provider)
            extensionsFuture.addListener({
                extensionsManager = try {
                    extensionsFuture.get()
                } catch (e: Exception) {
                    null
                }
                onReady()
            }, mainExecutor)
        } catch (e: Exception) {
            onReady()
        }
    }

    /**
     * @return false if the camera had to be restarted because the scene mode couldn't be bound.
     */
    private fun bindCameraUseCases(): Boolean {
        val cameraProvider =
            cameraProvider ?: throw IllegalStateException("Camera initialization failed.")

        updateActiveFeatures(readProCapabilities(cameraProvider))

        val resolution = if (isPhotoCapture) {
            imageQualityManager.getUserSelectedResolution(cameraSelector).also {
                listener.displaySelectedResolution(it.toResolutionOption())
            }
        } else {
            val selectedQuality = videoQualityManager.getUserSelectedQuality(cameraSelector).also {
                listener.displaySelectedResolution(it.toResolutionOption())
            }
            MySize(selectedQuality.width, selectedQuality.height)
        }

        listener.adjustPreviewView(resolution.requiresCentering())
        listener.onPreviewResolutionChanged(
            width = resolution.width,
            height = resolution.height,
            isFullScreen = resolution.isFullScreen
        )

        val isFullSize = resolution.isFullScreen
        previewView.scaleType = if (isFullSize) ScaleType.FILL_CENTER else ScaleType.FIT_CENTER
        val rotation = previewView.display.rotation
        val targetResolution = Size(resolution.width, resolution.height)

        val previewUseCase = buildPreview(targetResolution, rotation)
        val captureUseCase = getCaptureUseCase(targetResolution, rotation)

        cameraProvider.unbindAll()
        val useCaseGroup = if (isFullSize) {
            val metrics = windowMetricsCalculator.computeCurrentWindowMetrics(activity).bounds
            val screenWidth = metrics.width()
            val screenHeight = metrics.height()
            val viewPort = ViewPort.Builder(Rational(screenWidth, screenHeight), rotation).build()

            UseCaseGroup.Builder()
                .addUseCase(previewUseCase)
                .addUseCase(captureUseCase)
                .setViewPort(viewPort)
                .build()
        } else {
            null
        }

        val bind = { selector: CameraSelector ->
            if (useCaseGroup != null) {
                cameraProvider.bindToLifecycle(activity, selector, useCaseGroup)
            } else {
                cameraProvider.bindToLifecycle(activity, selector, previewUseCase, captureUseCase)
            }
        }

        camera = try {
            bind(getBindingSelector())
        } catch (e: IllegalArgumentException) {
            if (isRawActive) {
                handleRawFailure()
                return false
            }

            if (activeSceneMode == SceneMode.OFF) {
                throw e
            }

            handleSceneModeFailure()
            return false
        }

        preview = previewUseCase
        setupZoomAndFocus()
        setFlashlightState(config.flashlightState)
        applyManualSettings()
        return true
    }

    /** Decides whether RAW and a scene mode are used for this bind. */
    private fun updateActiveFeatures(capabilities: ProCapabilities?) {
        val rawWanted = isPhotoCapture && !isThirdPartyIntent && config.rawEnabled
        isRawActive = rawWanted && capabilities?.rawSupported == true
        // Vendor extensions and RAW can't be combined.
        activeSceneMode = if (isRawActive) SceneMode.OFF else resolveSceneMode()
    }

    @Suppress("SwallowedException")
    private fun readProCapabilities(provider: ProcessCameraProvider): ProCapabilities? {
        proCapabilities = try {
            ProCapabilitiesReader.read(provider.getCameraInfo(cameraSelector))
        } catch (e: IllegalArgumentException) {
            null
        }
        return proCapabilities
    }

    private fun handleRawFailure() {
        config.rawEnabled = false
        isRawActive = false
        activity.toast(R.string.raw_unsupported)
        startCamera()
    }

    private fun applyManualSettings() {
        val control = camera?.cameraControl ?: return
        val effective = if (activeSceneMode == SceneMode.OFF) manualSettings else ManualSettings()
        Camera2ManualControl.apply(control, effective)
    }

    /** The scene mode that is actually applied: only photos, only if the device supports it. */
    private fun resolveSceneMode(): SceneMode {
        val wanted = config.sceneMode
        val manager = extensionsManager
        if (!isPhotoCapture || manager == null || wanted == SceneMode.OFF) {
            return SceneMode.OFF
        }

        return if (isSceneModeAvailable(manager, wanted)) wanted else SceneMode.OFF
    }

    private fun isSceneModeAvailable(manager: ExtensionsManager, mode: SceneMode): Boolean {
        return try {
            manager.isExtensionAvailable(cameraSelector, mode.extensionMode)
        } catch (e: Exception) {
            false
        }
    }

    private fun getAvailableSceneModes(): List<SceneMode> {
        val manager = extensionsManager ?: return emptyList()
        val extensionModes = SceneMode.entries.filter {
            it != SceneMode.OFF && isSceneModeAvailable(manager, it)
        }

        // Standard mode is only a meaningful choice if there is something to choose from.
        return if (extensionModes.isEmpty()) emptyList() else listOf(SceneMode.OFF) + extensionModes
    }

    private fun getBindingSelector(): CameraSelector {
        val manager = extensionsManager
        return if (activeSceneMode == SceneMode.OFF || manager == null) {
            cameraSelector
        } else {
            manager.getExtensionEnabledCameraSelector(cameraSelector, activeSceneMode.extensionMode)
        }
    }

    private fun handleSceneModeFailure() {
        config.sceneMode = SceneMode.OFF
        activeSceneMode = SceneMode.OFF
        activity.toast(R.string.scene_mode_unsupported)
        startCamera()
    }

    private fun buildPreview(resolution: Size, rotation: Int): Preview {
        return Preview.Builder()
            .setTargetRotation(rotation)
            .setResolutionSelector(getResolutionSelector(resolution))
            .build().apply {
                setSurfaceProvider(previewView.surfaceProvider)
            }
    }

    private fun getCaptureUseCase(resolution: Size, rotation: Int): UseCase {
        return if (isPhotoCapture) {
            buildImageCapture(resolution, rotation).also {
                imageCapture = it
                videoCapture = null
                videoRecorder.videoCapture = null
            }
        } else {
            buildVideoCapture().also {
                videoCapture = it
                videoRecorder.videoCapture = it
                imageCapture = null
            }
        }
    }

    private fun buildImageCapture(resolution: Size, rotation: Int): ImageCapture {
        val builder = Builder()
            .setCaptureMode(getCaptureMode())
            .setFlashMode(flashMode)
            .setJpegQuality(config.photoQuality)
            .setTargetRotation(rotation)
            .setResolutionSelector(getResolutionSelector(resolution))
        if (isRawActive) {
            RawCaptureHelper.configure(builder)
        }

        return builder.build()
    }

    private fun getCaptureMode(): Int {
        return when (config.captureMode) {
            CaptureMode.MINIMIZE_LATENCY -> CAPTURE_MODE_MINIMIZE_LATENCY
            CaptureMode.MAXIMIZE_QUALITY -> CAPTURE_MODE_MAXIMIZE_QUALITY
        }
    }

    private fun getResolutionSelector(resolution: Size): ResolutionSelector {
        return ResolutionSelector.Builder()
            .setResolutionStrategy(ResolutionStrategy(resolution, FALLBACK_RULE_CLOSEST_LOWER_THEN_HIGHER))
            .setResolutionFilter { supportedSizes, rotationDegrees ->
                // Sort by closest image ratio
                supportedSizes.sortedBy {
                    size -> abs(size.width / size.height.toFloat() - resolution.width / resolution.height.toFloat())
                }
            }
            .setAllowedResolutionMode(getAllowedResolutionMode())
            .build()
    }

    private fun getAllowedResolutionMode(): Int {
        return when (config.captureMode) {
            CaptureMode.MINIMIZE_LATENCY -> PREFER_CAPTURE_RATE_OVER_HIGHER_RESOLUTION
            CaptureMode.MAXIMIZE_QUALITY -> PREFER_HIGHER_RESOLUTION_OVER_CAPTURE_RATE
        }
    }

    private fun buildVideoCapture(): VideoCapture<Recorder> {
        val qualitySelector = QualitySelector.from(
            videoQualityManager.getUserSelectedQuality(cameraSelector).toCameraXQuality(),
            FallbackStrategy.higherQualityOrLowerThan(Quality.SD),
        )

        val recorder = Recorder.Builder()
            .setQualitySelector(qualitySelector)
            .build()

        val builder = VideoCapture.Builder(recorder)
        val capabilities = proCapabilities
        val frameRate = config.videoFrameRate
        val fps = frameRate.fps
        if (fps != null && (capabilities == null || frameRate in capabilities.frameRates)) {
            builder.setTargetFrameRate(Range(fps, fps))
        }

        if (config.videoStabilization && capabilities?.stabilizationSupported != false) {
            builder.setVideoStabilizationEnabled(true)
        }

        return builder.build()
    }

    private fun setupCameraObservers() {
        val hasFlashUnit = camera?.cameraInfo?.hasFlashUnit() ?: false

        // Most front cameras have no flash unit, the UI lights up the screen instead.
        val usesScreenFlash = !hasFlashUnit && isPhotoCapture && isFrontCameraInUse()
        listener.onScreenFlashModeChanged(usesScreenFlash)
        if (usesScreenFlash && config.flashlightState in listOf(FLASH_AUTO, FLASH_ALWAYS_ON)) {
            setFlashlightState(FLASH_OFF)
        }

        listener.setFlashAvailable(hasFlashUnit || usesScreenFlash)
        listener.onChangeCamera(isFrontCameraInUse())
        reportExposureState()
        proCapabilities?.let { capabilities ->
            // Vendor extensions don't take part in Camera2 interop.
            val manual = capabilities.manual.takeIf { activeSceneMode == SceneMode.OFF }
            listener.onProCapabilitiesChanged(capabilities.copy(manual = manual))
        }

        if (isPhotoCapture) {
            listener.onSceneModesAvailable(getAvailableSceneModes(), activeSceneMode)
            listener.onInitPhotoMode()
        } else {
            listener.onSceneModesAvailable(emptyList(), SceneMode.OFF)
            listener.onInitVideoMode()
        }

        cameraStateLiveData?.removeObserver(cameraStateObserver)
        cameraStateLiveData = camera?.cameraInfo?.cameraState
        cameraStateLiveData?.observe(activity, cameraStateObserver)

        zoomStateLiveData?.removeObserver(zoomStateObserver)
        zoomStateLiveData = camera?.cameraInfo?.zoomState
        zoomStateLiveData?.observe(activity, zoomStateObserver)
    }

    private fun reportExposureState() {
        val exposureState = camera?.cameraInfo?.exposureState
        val range = exposureState?.exposureCompensationRange
        if (exposureState == null || range == null ||
            !exposureState.isExposureCompensationSupported ||
            (range.lower == 0 && range.upper == 0)
        ) {
            listener.onExposureStateChanged(false, 0, 0, 0, 0f)
            return
        }

        listener.onExposureStateChanged(
            supported = true,
            minIndex = range.lower,
            maxIndex = range.upper,
            currentIndex = exposureState.exposureCompensationIndex,
            stepEv = exposureState.exposureCompensationStep.toFloat()
        )
    }

    private fun hasBackCamera(): Boolean {
        return cameraProvider?.hasCamera(CameraSelector.DEFAULT_BACK_CAMERA) ?: false
    }

    private fun hasFrontCamera(): Boolean {
        return cameraProvider?.hasCamera(CameraSelector.DEFAULT_FRONT_CAMERA) ?: false
    }

    private fun isFrontCameraInUse(): Boolean {
        return cameraSelector == CameraSelector.DEFAULT_FRONT_CAMERA
    }

    @SuppressLint("ClickableViewAccessibility")
    // source: https://stackoverflow.com/a/60095886/10552591
    private fun setupZoomAndFocus() {
        val scaleGesture = camera?.let {
            ScaleGestureDetector(
                activity,
                PinchToZoomOnScaleGestureListener(it.cameraInfo, it.cameraControl)
            )
        }
        val gestureDetector = GestureDetector(activity, object : SimpleOnGestureListener() {
            override fun onDown(event: MotionEvent): Boolean {
                listener.onTouchPreview()
                return super.onDown(event)
            }

            override fun onSingleTapConfirmed(event: MotionEvent): Boolean {
                return camera?.cameraInfo?.let {
                    val display = displayManager.getDisplay(Display.DEFAULT_DISPLAY)
                    val width = previewView.width.toFloat()
                    val height = previewView.height.toFloat()
                    val factory = DisplayOrientedMeteringPointFactory(display, it, width, height)
                    val xPos = event.x
                    val yPos = event.y
                    val autoFocusPoint = factory.createPoint(xPos, yPos, AF_SIZE)
                    val autoExposurePoint = factory.createPoint(xPos, yPos, AE_SIZE)
                    val focusMeteringAction =
                        FocusMeteringAction.Builder(autoFocusPoint, FocusMeteringAction.FLAG_AF)
                            .addPoint(autoExposurePoint, FocusMeteringAction.FLAG_AE)
                            .disableAutoCancel()
                            .build()
                    camera?.cameraControl?.startFocusAndMetering(focusMeteringAction)
                    listener.onFocusCamera(event.rawX, event.rawY)
                    true
                } ?: false
            }
        })
        previewView.setOnTouchListener { _, event ->
            val handledGesture = gestureDetector.onTouchEvent(event)
            val handledScaleGesture = scaleGesture?.onTouchEvent(event)
            handledGesture || handledScaleGesture ?: false
        }
    }

    override fun onStart(owner: LifecycleOwner) {
        orientationEventListener.enable()
        previewView.doOnLayout {
            if (owner.lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED)) {
                startCamera()
            }
        }
    }

    override fun onResume(owner: LifecycleOwner) {
        super.onResume(owner)
        if (config.savePhotoVideoLocation) {
            if (simpleLocationManager == null) {
                simpleLocationManager = SimpleLocationManager(activity)
            }
            requestLocationUpdates()
        }
    }

    private fun requestLocationUpdates() {
        activity.apply {
            if (checkLocationPermission()) {
                simpleLocationManager?.requestLocationUpdates()
            } else {
                handlePermission(PERMISSION_ACCESS_FINE_LOCATION) { _ ->
                    if (checkLocationPermission()) {
                        simpleLocationManager?.requestLocationUpdates()
                    } else {
                        config.savePhotoVideoLocation = false
                    }
                }
            }
        }
    }

    override fun onPause(owner: LifecycleOwner) {
        super.onPause(owner)
        simpleLocationManager?.dropLocationUpdates()
    }

    override fun onStop(owner: LifecycleOwner) {
        captureSequencer.stop()
        orientationEventListener.disable()
    }

    override fun isInPhotoMode(): Boolean {
        return isPhotoCapture
    }

    override fun showChangeResolution() {
        val selectedResolution = if (isPhotoCapture) {
            imageQualityManager.getUserSelectedResolution(cameraSelector).toResolutionOption()
        } else {
            videoQualityManager.getUserSelectedQuality(cameraSelector).toResolutionOption()
        }

        val resolutions = if (isPhotoCapture) {
            imageQualityManager.getSupportedResolutions(cameraSelector)
                .map { it.toResolutionOption() }
        } else {
            videoQualityManager.getSupportedQualities(cameraSelector)
                .map { it.toResolutionOption() }
        }

        if (resolutions.size > 2) {
            listener.showImageSizes(
                selectedResolution = selectedResolution,
                resolutions = resolutions,
                isPhotoCapture = isPhotoCapture,
                isFrontCamera = isFrontCameraInUse()
            ) { index, changed ->
                mediaSizeStore.storeSize(isPhotoCapture, isFrontCameraInUse(), index)
                if (changed) {
                    videoRecorder.stop()
                    startCamera()
                }
            }
        } else {
            toggleResolutions(resolutions)
        }
    }

    private fun toggleResolutions(resolutions: List<ResolutionOption>) {
        if (resolutions.size >= 2) {
            val currentIndex =
                mediaSizeStore.getCurrentSizeIndex(isPhotoCapture, isFrontCameraInUse())

            val nextIndex = if (currentIndex >= resolutions.lastIndex) {
                0
            } else {
                currentIndex + 1
            }

            mediaSizeStore.storeSize(isPhotoCapture, isFrontCameraInUse(), nextIndex)
            videoRecorder.stop()
            startCamera()
        }
    }

    override fun toggleFrontBackCamera() {
        val newCameraSelector = if (isFrontCameraInUse()) {
            CameraSelector.DEFAULT_BACK_CAMERA
        } else {
            CameraSelector.DEFAULT_FRONT_CAMERA
        }

        cameraSelector = newCameraSelector
        config.lastUsedCameraLens = newCameraSelector.toLensFacing()
        startCamera(switching = true)
    }

    override fun setZoomRatio(ratio: Float) {
        val state = camera?.cameraInfo?.zoomState?.value ?: return
        val clamped = ratio.coerceIn(state.minZoomRatio, state.maxZoomRatio)
        camera?.cameraControl?.setZoomRatio(clamped)
    }

    override fun zoomBy(factor: Float) {
        val state = camera?.cameraInfo?.zoomState?.value ?: return
        setZoomRatio(state.zoomRatio * factor)
    }

    override fun setExposureIndex(index: Int) {
        camera?.cameraControl?.setExposureCompensationIndex(index)
    }

    override fun setSceneMode(mode: SceneMode) {
        if (mode == config.sceneMode && mode == activeSceneMode) {
            return
        }

        config.sceneMode = mode
        startCamera()
    }

    override fun setVideoFrameRate(rate: VideoFrameRate) {
        if (rate != config.videoFrameRate) {
            config.videoFrameRate = rate
            restartIfNeeded(!isPhotoCapture)
        }
    }

    override fun setVideoStabilization(enabled: Boolean) {
        if (enabled != config.videoStabilization) {
            config.videoStabilization = enabled
            restartIfNeeded(!isPhotoCapture)
        }
    }

    override fun setRawEnabled(enabled: Boolean) {
        if (enabled != config.rawEnabled) {
            config.rawEnabled = enabled
            restartIfNeeded(isPhotoCapture)
        }
    }

    override fun setManualSettings(settings: ManualSettings) {
        manualSettings = settings
        applyManualSettings()
    }

    override fun isCaptureSequenceRunning() = captureSequencer.isRunning

    override fun stopCaptureSequence() = captureSequencer.stop()

    private fun restartIfNeeded(needed: Boolean) {
        if (needed) {
            videoRecorder.stop()
            startCamera()
        }
    }

    override fun handleFlashlightClick() {
        if (isPhotoCapture) {
            listener.showFlashOptions(true)
        } else {
            toggleFlashlight()
        }
    }

    private fun toggleFlashlight() {
        val newFlashMode = if (isPhotoCapture) {
            when (flashMode) {
                FLASH_MODE_OFF -> FLASH_MODE_ON
                FLASH_MODE_ON -> FLASH_MODE_AUTO
                else -> FLASH_MODE_OFF
            }
        } else {
            when (flashMode) {
                FLASH_MODE_OFF -> FLASH_MODE_ON
                else -> FLASH_MODE_OFF
            }
        }
        setFlashlightState(newFlashMode.toAppFlashMode())
    }

    override fun setFlashlightState(state: Int) {
        var flashState = state
        if (isPhotoCapture) {
            camera?.cameraControl?.enableTorch(flashState == FLASH_ALWAYS_ON)
        } else {
            camera?.cameraControl?.enableTorch(flashState == FLASH_ON || flashState == FLASH_ALWAYS_ON)
            // reset to the FLASH_ON for video capture
            if (flashState == FLASH_ALWAYS_ON) {
                flashState = FLASH_ON
            }
        }
        val newFlashMode = flashState.toCameraXFlashMode()
        flashMode = newFlashMode
        imageCapture?.flashMode = newFlashMode

        config.flashlightState = flashState
        listener.onChangeFlashMode(flashState)
    }

    override fun tryTakePicture() {
        // Third party apps ask for exactly one JPEG.
        val driveMode = if (isThirdPartyIntent) DriveMode.SINGLE else config.driveMode
        when (driveMode) {
            DriveMode.SINGLE -> takePicture(null)
            DriveMode.BURST -> captureSequencer.startBurst(config.burstSize.frames)
            DriveMode.TIMELAPSE -> captureSequencer.startTimelapse(config.timelapseInterval.millis)
        }
    }

    /** [onDone] is always called on the main thread, with whether the picture was saved. */
    private fun takePicture(onDone: ((Boolean) -> Unit)?) {
        val capture = imageCapture
        if (capture == null) {
            activity.toast(R.string.camera_open_error)
            onDone?.invoke(false)
            return
        }

        photoCapturer.take(capture, isRawActive, isFrontCameraInUse(), onDone)
    }

    override fun initPhotoMode() {
        debounceChangeCameraMode(photoModeRunnable)
    }

    override fun initVideoMode() {
        debounceChangeCameraMode(videoModeRunnable)
    }

    private fun debounceChangeCameraMode(cameraModeRunnable: Runnable) {
        val currentTime = System.currentTimeMillis()
        if (currentTime - lastCameraStartTime > CAMERA_MODE_SWITCH_WAIT_TIME) {
            cameraModeRunnable.run()
        } else {
            cameraHandler.removeCallbacks(photoModeRunnable)
            cameraHandler.removeCallbacks(videoModeRunnable)
            cameraHandler.postDelayed(cameraModeRunnable, CAMERA_MODE_SWITCH_WAIT_TIME)
        }
        lastCameraStartTime = currentTime
    }

    override fun toggleRecording() = videoRecorder.toggle()

    override fun toggleRecordingPause() = videoRecorder.togglePause()
}
