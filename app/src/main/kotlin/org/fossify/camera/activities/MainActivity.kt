package org.fossify.camera.activities

import android.animation.ObjectAnimator
import android.annotation.SuppressLint
import android.content.Intent
import android.content.res.ColorStateList
import android.graphics.Bitmap
import android.hardware.SensorManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.CountDownTimer
import android.provider.MediaStore
import android.view.*
import android.widget.LinearLayout
import android.widget.PopupMenu
import android.widget.SeekBar
import androidx.constraintlayout.widget.ConstraintSet
import androidx.core.content.ContextCompat
import androidx.core.view.*
import androidx.interpolator.view.animation.FastOutSlowInInterpolator
import androidx.transition.*
import com.bumptech.glide.Glide
import com.bumptech.glide.load.engine.DiskCacheStrategy
import com.bumptech.glide.load.resource.drawable.DrawableTransitionOptions
import com.bumptech.glide.request.RequestOptions
import com.google.android.material.button.MaterialButton
import com.google.android.material.button.MaterialButtonToggleGroup
import com.google.android.material.tabs.TabLayout
import org.fossify.camera.BuildConfig
import org.fossify.camera.R
import org.fossify.camera.databinding.ActivityMainBinding
import org.fossify.camera.extensions.config
import org.fossify.camera.extensions.fadeIn
import org.fossify.camera.extensions.fadeOut
import org.fossify.camera.extensions.setShadowIcon
import org.fossify.camera.extensions.toFlashModeId
import org.fossify.camera.helpers.*
import org.fossify.camera.implementations.CameraXInitializer
import org.fossify.camera.implementations.CameraXPreviewListener
import org.fossify.camera.interfaces.MyPreview
import org.fossify.camera.models.GridType
import org.fossify.camera.models.ProCapabilities
import org.fossify.camera.models.ResolutionOption
import org.fossify.camera.models.SceneMode
import org.fossify.camera.models.TimerMode
import org.fossify.camera.models.VolumeButtonAction
import org.fossify.camera.ui.ProControlsPanel
import org.fossify.camera.ui.ZoomChipsController
import org.fossify.camera.views.FocusCircleView
import org.fossify.commons.extensions.*
import org.fossify.commons.helpers.*
import java.util.Locale
import java.util.concurrent.TimeUnit
import kotlin.math.abs

class MainActivity : SimpleActivity(), PhotoProcessor.MediaSavedListener, CameraXPreviewListener {
    private companion object {
        private const val ANIMATION_DURATION = 500L
        private const val PHOTO_MODE_INDEX = 1
        private const val VIDEO_MODE_INDEX = 0
        private const val MIN_SWIPE_DISTANCE_X = 100
        private const val TIMER_2_SECONDS = 2001
        private const val SWITCH_CAMERA_ROTATION_ANGLE = 180f
        private const val SCREEN_FLASH_DELAY = 350L
        private const val SCREEN_FLASH_SAFETY_TIMEOUT = 4000L
        private const val SCREEN_FLASH_ALPHA = 0.95f
        private const val ZOOM_KEY_FACTOR = 1.06f
        private const val PAUSED_TIMER_ALPHA = 0.5f
    }

    private val binding by viewBinding(ActivityMainBinding::inflate)

    private lateinit var defaultScene: Scene
    private lateinit var flashModeScene: Scene
    private lateinit var timerScene: Scene
    private lateinit var mOrientationEventListener: OrientationEventListener
    private lateinit var mFocusCircleView: FocusCircleView
    private lateinit var mediaSoundHelper: MediaSoundHelper
    private var mPreview: MyPreview? = null
    private var mediaSizeToggleGroup: MaterialButtonToggleGroup? = null
    private var mPreviewUri: Uri? = null
    private var mIsHardwareShutterHandled = false
    private var mLastHandledOrientation = 0
    private var countDownTimer: CountDownTimer? = null
    private var mOriginalBrightness: Float? = null
    private var mScreenFlashMode = false
    private var mIsScreenFlashPending = false
    private var mBrightnessBeforeScreenFlash: Float? = null
    private var mSceneModes: List<SceneMode> = emptyList()
    private var mSelectedSceneMode = SceneMode.OFF
    private lateinit var mZoomChips: ZoomChipsController
    private lateinit var mProPanel: ProControlsPanel
    private var mExposureMinIndex = 0
    private var mExposureStep = 0f
    private val hideScreenFlashRunnable = Runnable { setScreenFlash(false) }

    private val tabSelectedListener = object : TabSelectedListener {
        override fun onTabSelected(tab: TabLayout.Tab) {
            when (tab.position) {
                PHOTO_MODE_INDEX -> mPreview?.initPhotoMode()
                VIDEO_MODE_INDEX -> initVideoModeWithPermission()
                else -> throw IllegalStateException("Unsupported tab position ${tab.position}")
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        useDynamicTheme = false
        super.onCreate(savedInstanceState)
        appLaunched(BuildConfig.APPLICATION_ID)
        requestWindowFeature(Window.FEATURE_NO_TITLE)
        initVariables()
        tryInitCamera()
        supportActionBar?.hide()
        setupOrientationEventListener()

        val windowInsetsController = ViewCompat.getWindowInsetsController(window.decorView)
        windowInsetsController?.systemBarsBehavior =
            WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        windowInsetsController?.hide(WindowInsetsCompat.Type.statusBars())

        if (isOreoMr1Plus()) {
            setShowWhenLocked(true)
            setTurnScreenOn(true)
        } else {
            window.addFlags(
                WindowManager.LayoutParams.FLAG_DISMISS_KEYGUARD or
                        WindowManager.LayoutParams.FLAG_SHOW_WHEN_LOCKED or
                        WindowManager.LayoutParams.FLAG_TURN_SCREEN_ON or
                        WindowManager.LayoutParams.FLAG_FULLSCREEN
            )
        }
    }

    override fun onResume() {
        super.onResume()
        if (hasStorageAndCameraPermissions()) {
            val isInPhotoMode = isInPhotoMode()
            setupPreviewImage(isInPhotoMode)
            mFocusCircleView.setStrokeColor(getProperPrimaryColor())
            updateViewfinderOverlays()
            toggleActionButtons(enabled = true)
            mOrientationEventListener.enable()
        }

        if (::mProPanel.isInitialized) {
            mProPanel.onResume()
        }

        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        mOriginalBrightness = window.updateBrightness(config.maxBrightness, mOriginalBrightness)
        ensureTransparentNavigationBar()
        if (ViewCompat.getWindowInsetsController(window.decorView) == null) {
            window.decorView.systemUiVisibility =
                View.SYSTEM_UI_FLAG_FULLSCREEN or View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
        }
    }

    override fun onPause() {
        super.onPause()
        window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        if (::mProPanel.isInitialized) {
            mProPanel.onPause()
        }

        if (::mFocusCircleView.isInitialized) {
            binding.levelView.stop()
            mIsScreenFlashPending = false
            setScreenFlash(false)
        }

        if (!isAskingPermissions) {
            cancelTimer()
        }

        if (!hasStorageAndCameraPermissions() || isAskingPermissions) {
            return
        }

        mOrientationEventListener.disable()
    }

    override fun onDestroy() {
        super.onDestroy()
        mPreview = null
        mediaSoundHelper.release()
    }

    override fun onBackPressedCompat(): Boolean {
        return closeOptions()
    }

    private fun selectPhotoTab(triggerListener: Boolean = false) {
        if (!triggerListener) {
            removeTabListener()
        }

        binding.cameraModeTab.getTabAt(PHOTO_MODE_INDEX)?.select()
        setTabListener()
    }

    private fun selectVideoTab(triggerListener: Boolean = false) {
        if (!triggerListener) {
            removeTabListener()
        }
        binding.cameraModeTab.getTabAt(VIDEO_MODE_INDEX)?.select()
        setTabListener()
    }

    private fun setTabListener() {
        binding.cameraModeTab.addOnTabSelectedListener(tabSelectedListener)
    }

    private fun removeTabListener() {
        binding.cameraModeTab.removeOnTabSelectedListener(tabSelectedListener)
    }

    private fun ensureTransparentNavigationBar() {
        window.navigationBarColor = ContextCompat.getColor(this, android.R.color.transparent)
    }

    private fun initVariables() {
        mIsHardwareShutterHandled = false
        mediaSoundHelper = MediaSoundHelper(this)
        mediaSoundHelper.loadSounds()
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        val isVolumeKey =
            keyCode == KeyEvent.KEYCODE_VOLUME_DOWN || keyCode == KeyEvent.KEYCODE_VOLUME_UP
        val volumeAction = if (isVolumeKey) config.volumeButtonsAction else VolumeButtonAction.NONE
        return if (keyCode == KeyEvent.KEYCODE_CAMERA && !mIsHardwareShutterHandled) {
            mIsHardwareShutterHandled = true
            shutterPressed()
            true
        } else if (volumeAction == VolumeButtonAction.SHUTTER) {
            // Also swallow the key repeats while the button is held, so the system volume
            // doesn't change while taking a picture.
            if (!mIsHardwareShutterHandled) {
                mIsHardwareShutterHandled = true
                shutterPressed()
            }
            true
        } else if (volumeAction == VolumeButtonAction.ZOOM) {
            val factor = if (keyCode == KeyEvent.KEYCODE_VOLUME_UP) {
                ZOOM_KEY_FACTOR
            } else {
                1f / ZOOM_KEY_FACTOR
            }
            mPreview?.zoomBy(factor)
            true
        } else {
            super.onKeyDown(keyCode, event)
        }
    }

    override fun onKeyUp(keyCode: Int, event: KeyEvent): Boolean {
        if (keyCode == KeyEvent.KEYCODE_CAMERA || keyCode == KeyEvent.KEYCODE_VOLUME_DOWN || keyCode == KeyEvent.KEYCODE_VOLUME_UP) {
            mIsHardwareShutterHandled = false
        }
        return super.onKeyUp(keyCode, event)
    }

    private fun hideIntentButtons() = binding.apply {
        cameraModeHolder.beGone()
        layoutTop.settings.beGone()
        layoutTop.toggleProControls.beGone()
        lastPhotoVideoPreview.beInvisible()
    }

    private fun tryInitCamera() {
        handlePermission(PERMISSION_CAMERA) { grantedCameraPermission ->
            if (grantedCameraPermission) {
                handleStoragePermission {
                    val isInPhotoMode = isInPhotoMode()
                    if (isInPhotoMode) {
                        initializeCamera(true)
                    } else if (!isAudioRecordingEnabled()) {
                        initializeCamera(false)
                    } else {
                        handlePermission(PERMISSION_RECORD_AUDIO) { grantedRecordAudioPermission ->
                            if (grantedRecordAudioPermission) {
                                initializeCamera(false)
                            } else {
                                toast(org.fossify.commons.R.string.no_audio_permissions)
                                if (isThirdPartyIntent()) {
                                    finish()
                                } else {
                                    // re-initialize in photo mode
                                    config.initPhotoMode = true
                                    tryInitCamera()
                                }
                            }
                        }
                    }
                }
            } else {
                toast(org.fossify.commons.R.string.no_camera_permissions)
                finish()
            }
        }
    }

    private fun isInPhotoMode(): Boolean {
        return mPreview?.isInPhotoMode() ?: if (isVideoCaptureIntent()) {
            false
        } else if (isImageCaptureIntent()) {
            true
        } else {
            config.initPhotoMode
        }
    }

    private fun handleStoragePermission(callback: (granted: Boolean) -> Unit) {
        if (isTiramisuPlus()) {
            val mediaPermissionIds =
                mutableListOf(PERMISSION_READ_MEDIA_IMAGES, PERMISSION_READ_MEDIA_VIDEO)
            if (isUpsideDownCakePlus()) {
                mediaPermissionIds.add(PERMISSION_READ_MEDIA_VISUAL_USER_SELECTED)
            }

            handlePartialMediaPermissions(permissionIds = mediaPermissionIds, callback = callback)
        } else {
            handlePermission(PERMISSION_WRITE_STORAGE, callback)
        }
    }

    private fun isThirdPartyIntent() = isVideoCaptureIntent() || isImageCaptureIntent()

    /** Videos requested by other apps always get sound, otherwise the user setting decides. */
    private fun isAudioRecordingEnabled() = config.recordAudio || isVideoCaptureIntent()

    private fun hasAudioPermissionIfNeeded() =
        !isAudioRecordingEnabled() || hasPermission(PERMISSION_RECORD_AUDIO)

    private fun initVideoModeWithPermission() {
        if (!isAudioRecordingEnabled()) {
            mPreview?.initVideoMode()
            return
        }

        handlePermission(PERMISSION_RECORD_AUDIO) { granted ->
            if (granted) {
                mPreview?.initVideoMode()
            } else {
                toast(org.fossify.commons.R.string.no_audio_permissions)
                selectPhotoTab()
                if (isVideoCaptureIntent()) {
                    finish()
                }
            }
        }
    }

    private fun isImageCaptureIntent(): Boolean =
        intent?.action == MediaStore.ACTION_IMAGE_CAPTURE || intent?.action == MediaStore.ACTION_IMAGE_CAPTURE_SECURE

    private fun isVideoCaptureIntent(): Boolean = intent?.action == MediaStore.ACTION_VIDEO_CAPTURE

    private fun createToggleGroup(): MaterialButtonToggleGroup {
        return MaterialButtonToggleGroup(this).apply {
            isSingleSelection = true
            layoutParams = ViewGroup.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.WRAP_CONTENT
            )
        }
    }

    private fun initializeCamera(isInPhotoMode: Boolean) {
        setContentView(binding.root)
        initButtons()
        initModeSwitcher()
        binding.apply {
            defaultScene = Scene(topOptions, layoutTop.defaultIcons)
            flashModeScene = Scene(topOptions, layoutFlash.flashToggleGroup)
            timerScene = Scene(topOptions, layoutTimer.timerToggleGroup)
        }

        WindowCompat.setDecorFitsSystemWindows(window, false)
        ViewCompat.setOnApplyWindowInsetsListener(binding.viewHolder) { _, windowInsets ->
            val safeInsetBottom = windowInsets.displayCutout?.safeInsetBottom ?: 0
            val safeInsetTop = windowInsets.displayCutout?.safeInsetTop ?: 0

            binding.topOptions.updateLayoutParams<ViewGroup.MarginLayoutParams> {
                topMargin = safeInsetTop
            }

            val systemBarsInsets = windowInsets.getInsets(WindowInsetsCompat.Type.systemBars())
            val marginBottom = systemBarsInsets.bottom +
                    resources.getDimensionPixelSize(org.fossify.commons.R.dimen.bigger_margin)

            binding.shutter.updateLayoutParams<ViewGroup.MarginLayoutParams> {
                bottomMargin = marginBottom
            }

            WindowInsetsCompat.CONSUMED
        }

        if (isInPhotoMode) {
            selectPhotoTab()
        } else {
            selectVideoTab()
        }

        val outputUri = intent.extras?.get(MediaStore.EXTRA_OUTPUT) as? Uri
        val isThirdPartyIntent = isThirdPartyIntent()
        mPreview = CameraXInitializer(this).createCameraXPreview(
            binding.previewView,
            listener = this,
            mediaSoundHelper = mediaSoundHelper,
            outputUri = outputUri,
            isThirdPartyIntent = isThirdPartyIntent,
            initInPhotoMode = isInPhotoMode,
        )

        mFocusCircleView = FocusCircleView(this).apply {
            id = View.generateViewId()
        }
        binding.viewHolder.addView(mFocusCircleView)
        updateViewfinderOverlays()

        setupPreviewImage(true)
        initFlashModeTransitionNames()
        initTimerModeTransitionNames()

        if (isThirdPartyIntent) {
            hideIntentButtons()
        }
    }

    private fun initFlashModeTransitionNames() = binding.layoutFlash.apply {
        val baseName = getString(R.string.toggle_flash)
        flashAuto.transitionName = "$baseName$FLASH_AUTO"
        flashOff.transitionName = "$baseName$FLASH_OFF"
        flashOn.transitionName = "$baseName$FLASH_ON"
        flashAlwaysOn.transitionName = "$baseName$FLASH_ALWAYS_ON"
    }

    private fun initTimerModeTransitionNames() = binding.layoutTimer.apply {
        val baseName = getString(R.string.toggle_timer)
        timerOff.transitionName = "$baseName${TimerMode.OFF.name}"
        timer3s.transitionName = "$baseName${TimerMode.TIMER_3.name}"
        timer5s.transitionName = "$baseName${TimerMode.TIMER_5.name}"
        timer10S.transitionName = "$baseName${TimerMode.TIMER_10.name}"
    }

    private fun initButtons() = binding.apply {
        mZoomChips = ZoomChipsController(
            context = this@MainActivity,
            container = zoomPresets,
            host = object : ZoomChipsController.Host {
                override fun currentRotation() = getCurrentRotation()
                override fun primaryColor() = getProperPrimaryColor()
                override fun onPresetClicked(view: View, ratio: Float) {
                    performHaptic(view)
                    mPreview?.setZoomRatio(ratio)
                }
            }
        )
        mProPanel = ProControlsPanel(this@MainActivity, proPanel, histogramView, previewView) { mPreview }
        layoutTop.toggleProControls.setShadowIcon(R.drawable.ic_pro_vector)
        layoutTop.toggleProControls.setOnClickListener { toggleProPanel() }
        timerText.setFactory { layoutInflater.inflate(R.layout.timer_text, null) }
        toggleCamera.setOnClickListener {
            animateCameraToggle()
            mPreview!!.toggleFrontBackCamera()
        }

        lastPhotoVideoPreview.setOnClickListener { showLastMediaPreview() }

        layoutTop.apply {
            toggleFlash.setOnClickListener { mPreview!!.handleFlashlightClick() }
            toggleTimer.setOnClickListener {
                val transitionSet = createTransition()
                TransitionManager.go(timerScene, transitionSet)
                layoutTimer.timerToggleGroup.beVisible()
                layoutTimer.timerToggleGroup.check(config.timerMode.getTimerModeResId())
                layoutTimer.timerToggleGroup.children.forEach { setButtonColors(it as MaterialButton) }
            }

            settings.setShadowIcon(R.drawable.ic_settings_vector)
            settings.setOnClickListener { launchSettings() }
            changeResolution.setOnClickListener { mPreview?.showChangeResolution() }
        }

        shutter.setOnClickListener { shutterPressed() }

        layoutFlash.apply {
            flashOn.setShadowIcon(R.drawable.ic_flash_on_vector)
            flashOn.setOnClickListener { selectFlashMode(FLASH_ON) }

            flashOff.setShadowIcon(R.drawable.ic_flash_off_vector)
            flashOff.setOnClickListener { selectFlashMode(FLASH_OFF) }

            flashAuto.setShadowIcon(R.drawable.ic_flash_auto_vector)
            flashAuto.setOnClickListener { selectFlashMode(FLASH_AUTO) }

            flashAlwaysOn.setShadowIcon(R.drawable.ic_flashlight_vector)
            flashAlwaysOn.setOnClickListener { selectFlashMode(FLASH_ALWAYS_ON) }
        }

        layoutTimer.apply {
            timerOff.setShadowIcon(R.drawable.ic_timer_off_vector)
            timerOff.setOnClickListener { selectTimerMode(TimerMode.OFF) }

            timer3s.setShadowIcon(R.drawable.ic_timer_3_vector)
            timer3s.setOnClickListener { selectTimerMode(TimerMode.TIMER_3) }

            timer5s.setShadowIcon(R.drawable.ic_timer_5_vector)
            timer5s.setOnClickListener { selectTimerMode(TimerMode.TIMER_5) }

            timer10S.setShadowIcon(R.drawable.ic_timer_10_vector)
            timer10S.setOnClickListener { selectTimerMode(TimerMode.TIMER_10) }
        }

        layoutTop.toggleExposure.setShadowIcon(R.drawable.ic_exposure_vector)
        layoutTop.toggleExposure.setOnClickListener { toggleExposureSlider() }
        layoutTop.toggleScene.setShadowIcon(R.drawable.ic_scene_vector)
        layoutTop.toggleScene.setOnClickListener { showSceneModeMenu(it) }

        exposureValue.setOnClickListener { resetExposure() }
        exposureSeekbar.setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
            override fun onProgressChanged(seekBar: SeekBar, progress: Int, fromUser: Boolean) {
                val index = mExposureMinIndex + progress
                updateExposureValueText(index)
                if (fromUser) {
                    mPreview?.setExposureIndex(index)
                }
            }

            override fun onStartTrackingTouch(seekBar: SeekBar) {}

            override fun onStopTrackingTouch(seekBar: SeekBar) {}
        })

        togglePause.setOnClickListener {
            performHaptic(it)
            mPreview?.toggleRecordingPause()
        }

        setTimerModeIcon(config.timerMode)
    }

    private fun animateCameraToggle() {
        ObjectAnimator.ofFloat(binding.toggleCamera, "rotation", 0f, SWITCH_CAMERA_ROTATION_ANGLE)
            .apply {
                duration = ANIMATION_DURATION
                interpolator = FastOutSlowInInterpolator()
                start()
            }
    }

    private fun selectTimerMode(timerMode: TimerMode) {
        config.timerMode = timerMode
        setTimerModeIcon(timerMode)
        closeOptions()
    }

    private fun setTimerModeIcon(timerMode: TimerMode) = binding.layoutTop.toggleTimer.apply {
        setShadowIcon(timerMode.getTimerModeDrawableRes())
        transitionName = "${getString(R.string.toggle_timer)}${timerMode.name}"
    }

    @SuppressLint("ClickableViewAccessibility")
    private fun initModeSwitcher() {
        val gestureDetector = GestureDetectorCompat(this, object : GestureDetectorListener() {
            override fun onDown(e: MotionEvent): Boolean {
                // we have to return true here so ACTION_UP (and onFling) can be dispatched
                return true
            }

            override fun onFling(
                event1: MotionEvent?,
                event2: MotionEvent?,
                velocityX: Float,
                velocityY: Float
            ): Boolean {
                if (event1 == null || event2 == null) {
                    return true
                }

                val deltaX = event1.x - event2.x
                val deltaXAbs = abs(deltaX)

                if (deltaXAbs >= MIN_SWIPE_DISTANCE_X) {
                    if (deltaX > 0) {
                        onSwipeLeft()
                    } else {
                        onSwipeRight()
                    }
                }

                return true
            }
        })

        binding.cameraModeTab.setOnTouchListener { _, event ->
            gestureDetector.onTouchEvent(event)
        }
    }

    private fun onSwipeLeft() {
        if (!isThirdPartyIntent() && binding.cameraModeHolder.isVisible()) {
            selectPhotoTab(triggerListener = true)
        }
    }

    private fun onSwipeRight() {
        if (!isThirdPartyIntent() && binding.cameraModeHolder.isVisible()) {
            selectVideoTab(triggerListener = true)
        }
    }

    private fun selectFlashMode(flashMode: Int) {
        closeOptions()
        mPreview?.setFlashlightState(flashMode)
    }


    private fun showLastMediaPreview() {
        if (mPreviewUri != null) {
            val path =
                applicationContext.getRealPathFromURI(mPreviewUri!!) ?: mPreviewUri!!.toString()
            openPathIntent(path, false, BuildConfig.APPLICATION_ID)
        }
    }

    private fun shutterPressed() {
        performHaptic(binding.shutter)
        when {
            countDownTimer != null -> cancelTimer()
            mPreview?.isCaptureSequenceRunning() == true -> mPreview?.stopCaptureSequence()
            // A screen flash capture is already on its way.
            mIsScreenFlashPending -> Unit
            isInPhotoMode() -> {
                val timerMode = config.timerMode
                if (timerMode == TimerMode.OFF) {
                    capturePhoto()
                } else {
                    scheduleTimer(timerMode)
                }
            }

            else -> mPreview?.toggleRecording()
        }
    }

    private fun cancelTimer() {
        mediaSoundHelper.stopTimerCountdown2SecondsSound()
        countDownTimer?.cancel()
        countDownTimer = null
        resetViewsOnTimerFinish()
    }

    private fun launchSettings() {
        val intent = Intent(applicationContext, SettingsActivity::class.java)
        startActivity(intent)
    }

    override fun onInitPhotoMode() {
        binding.apply {
            shutter.setImageResource(R.drawable.ic_shutter_animated)
            layoutTop.toggleTimer.beVisible()
            layoutTop.toggleTimer.fadeIn()
        }
        closeOptions(keepProPanel = true)
        mProPanel.setPhotoMode(true)
        setupPreviewImage(true)
        selectPhotoTab()
    }

    override fun onInitVideoMode() {
        binding.apply {
            shutter.setImageResource(R.drawable.ic_video_rec_animated)
            layoutTop.toggleTimer.fadeOut()
            layoutTop.toggleTimer.beGone()
        }
        closeOptions(keepProPanel = true)
        mProPanel.setPhotoMode(false)
        setupPreviewImage(false)
        selectVideoTab()
    }

    private fun setupPreviewImage(isPhoto: Boolean) {
        val uri =
            if (isPhoto) MediaStore.Images.Media.EXTERNAL_CONTENT_URI else MediaStore.Video.Media.EXTERNAL_CONTENT_URI
        ensureBackgroundThread {
            // This is a MediaStore query, it must not block the UI thread.
            val lastMediaId = getLatestMediaId(uri)
            runOnUiThread {
                if (isDestroyed) {
                    return@runOnUiThread
                }

                if (lastMediaId == 0L) {
                    // No media (anymore), don't keep showing a stale thumbnail.
                    mPreviewUri = null
                    binding.lastPhotoVideoPreview.setImageDrawable(null)
                } else {
                    loadLastTakenMedia(Uri.withAppendedPath(uri, lastMediaId.toString()))
                }
            }
        }
    }

    private fun loadLastTakenMedia(uri: Uri?) {
        mPreviewUri = uri
        runOnUiThread {
            if (!isDestroyed) {
                val options = RequestOptions()
                    .circleCrop()
                    .diskCacheStrategy(DiskCacheStrategy.NONE)

                Glide.with(this)
                    .load(uri)
                    .apply(options)
                    .transition(DrawableTransitionOptions.withCrossFade())
                    .into(binding.lastPhotoVideoPreview)
            }
        }
    }

    private fun updateViewfinderOverlays() {
        binding.apply {
            val gridType = config.gridType
            gridOverlay.setGridType(gridType)
            gridOverlay.beVisibleIf(gridType != GridType.OFF)

            if (config.showLevel && levelView.isSupported) {
                levelView.setAccentColor(getProperPrimaryColor())
                levelView.beVisible()
                levelView.start()
            } else {
                levelView.stop()
                levelView.beGone()
            }
        }
    }

    private fun performHaptic(view: View) {
        if (!config.hapticFeedback) {
            return
        }

        val feedback = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            HapticFeedbackConstants.CONFIRM
        } else {
            HapticFeedbackConstants.CONTEXT_CLICK
        }
        view.performHapticFeedback(feedback)
    }

    private fun getCurrentRotation(): Float {
        return when (mLastHandledOrientation) {
            ORIENT_LANDSCAPE_LEFT -> 90f
            ORIENT_LANDSCAPE_RIGHT -> -90f
            else -> 0f
        }
    }

    /**
     * Takes a picture. On a front camera without a flash unit and with the flash set to "on",
     * the screen is lit up first so the face is illuminated (screen flash).
     */
    private fun capturePhoto() {
        val preview = mPreview ?: return
        if (mScreenFlashMode && config.flashlightState == FLASH_ON) {
            mIsScreenFlashPending = true
            setScreenFlash(true)
            binding.screenFlash.postDelayed({
                if (mIsScreenFlashPending && !isDestroyed) {
                    mIsScreenFlashPending = false
                    preview.tryTakePicture()
                }
            }, SCREEN_FLASH_DELAY)
        } else {
            preview.tryTakePicture()
        }
    }

    private fun setScreenFlash(enabled: Boolean) {
        val flashView = binding.screenFlash
        flashView.removeCallbacks(hideScreenFlashRunnable)
        if (enabled) {
            if (mBrightnessBeforeScreenFlash == null) {
                mBrightnessBeforeScreenFlash = window.attributes.screenBrightness
            }

            setWindowBrightness(1f)
            flashView.alpha = SCREEN_FLASH_ALPHA
            // Safety net: never leave the screen white if the capture fails silently.
            flashView.postDelayed(hideScreenFlashRunnable, SCREEN_FLASH_SAFETY_TIMEOUT)
        } else {
            flashView.alpha = 0f
            mBrightnessBeforeScreenFlash?.let { setWindowBrightness(it) }
            mBrightnessBeforeScreenFlash = null
        }
    }

    private fun setWindowBrightness(brightness: Float) {
        val params = window.attributes
        params.screenBrightness = brightness
        window.attributes = params
    }

    override fun onScreenFlashModeChanged(enabled: Boolean) {
        mScreenFlashMode = enabled
    }

    override fun onPreviewResolutionChanged(width: Int, height: Int, isFullScreen: Boolean) {
        binding.gridOverlay.setContentSize(width, height, isFullScreen)
    }

    // region zoom
    override fun onZoomStateChanged(minRatio: Float, maxRatio: Float, zoomRatio: Float) {
        mZoomChips.onZoomStateChanged(minRatio, maxRatio, zoomRatio)
    }
    // endregion

    // region exposure
    override fun onExposureStateChanged(
        supported: Boolean,
        minIndex: Int,
        maxIndex: Int,
        currentIndex: Int,
        stepEv: Float,
    ) {
        binding.layoutTop.toggleExposure.beVisibleIf(supported)
        if (!supported) {
            binding.exposureHolder.beGone()
            return
        }

        mExposureMinIndex = minIndex
        mExposureStep = stepEv
        val primaryColor = getProperPrimaryColor()
        binding.exposureSeekbar.apply {
            max = maxIndex - minIndex
            progress = currentIndex - minIndex
            progressTintList = ColorStateList.valueOf(primaryColor)
            thumbTintList = ColorStateList.valueOf(primaryColor)
        }
        updateExposureValueText(currentIndex)
    }

    @SuppressLint("SetTextI18n")
    private fun updateExposureValueText(index: Int) {
        binding.exposureValue.text = if (index == 0) {
            "0 EV"
        } else {
            String.format(Locale.getDefault(), "%+.1f EV", index * mExposureStep)
        }
    }

    private fun toggleExposureSlider() {
        if (binding.exposureHolder.isVisible()) {
            binding.exposureHolder.beGone()
        } else {
            closeOptions()
            binding.exposureHolder.beVisible()
        }
    }

    private fun resetExposure() {
        binding.exposureSeekbar.progress = -mExposureMinIndex
        mPreview?.setExposureIndex(0)
    }
    // endregion

    // region scene modes
    override fun onSceneModesAvailable(modes: List<SceneMode>, selected: SceneMode) {
        mSceneModes = modes
        mSelectedSceneMode = selected
        binding.layoutTop.toggleScene.beVisibleIf(modes.size > 1)
    }

    private fun showSceneModeMenu(anchor: View) {
        closeOptions()
        val popup = PopupMenu(this, anchor)
        mSceneModes.forEachIndexed { index, mode ->
            popup.menu.add(0, index, index, mode.stringResId).apply {
                isCheckable = true
                isChecked = mode == mSelectedSceneMode
            }
        }
        popup.menu.setGroupCheckable(0, true, true)
        popup.setOnMenuItemClickListener { item ->
            mSceneModes.getOrNull(item.itemId)?.let { mode ->
                mSelectedSceneMode = mode
                mPreview?.setSceneMode(mode)
                toast(mode.stringResId)
            }
            true
        }
        popup.show()
    }
    // endregion

    override fun onProCapabilitiesChanged(capabilities: ProCapabilities) {
        mProPanel.onCapabilities(capabilities)
    }

    override fun onCaptureSequenceProgress(taken: Int, total: Int?) {
        binding.videoRecCurrTimer.beVisible()
        binding.videoRecCurrTimer.text = if (total != null) {
            getString(R.string.burst_progress, taken, total)
        } else {
            getString(R.string.timelapse_progress, taken)
        }
    }

    override fun onCaptureSequenceFinished(taken: Int, completedAll: Boolean) {
        binding.videoRecCurrTimer.beGone()
        if (taken > 0) {
            toast(getString(R.string.sequence_finished, taken))
        }
    }

    override fun onVideoRecordingPaused() {
        binding.togglePause.setImageResource(R.drawable.ic_resume_vector)
        binding.togglePause.contentDescription = getString(R.string.resume_recording)
        binding.videoRecCurrTimer.alpha = PAUSED_TIMER_ALPHA
    }

    override fun onVideoRecordingResumed() {
        resetPauseButton()
    }

    private fun resetPauseButton() {
        binding.togglePause.setImageResource(R.drawable.ic_pause_vector)
        binding.togglePause.contentDescription = getString(R.string.pause_recording)
        binding.videoRecCurrTimer.alpha = 1f
    }

    private fun hasStorageAndCameraPermissions(): Boolean {
        return if (isInPhotoMode()) hasPhotoModePermissions() else hasVideoModePermissions()
    }

    private fun hasPhotoModePermissions(): Boolean {
        return if (isTiramisuPlus()) {
            var hasMediaPermission = hasPermission(PERMISSION_READ_MEDIA_IMAGES) || hasPermission(
                PERMISSION_READ_MEDIA_VIDEO
            )
            if (isUpsideDownCakePlus()) {
                hasMediaPermission =
                    hasMediaPermission || hasPermission(PERMISSION_READ_MEDIA_VISUAL_USER_SELECTED)
            }
            hasMediaPermission && hasPermission(PERMISSION_CAMERA)
        } else {
            hasPermission(PERMISSION_WRITE_STORAGE) && hasPermission(PERMISSION_CAMERA)
        }
    }

    private fun hasVideoModePermissions(): Boolean {
        return if (isTiramisuPlus()) {
            var hasMediaPermission = hasPermission(PERMISSION_READ_MEDIA_VIDEO)
            if (isUpsideDownCakePlus()) {
                hasMediaPermission =
                    hasMediaPermission || hasPermission(PERMISSION_READ_MEDIA_VISUAL_USER_SELECTED)
            }
            hasMediaPermission && hasPermission(PERMISSION_CAMERA) && hasAudioPermissionIfNeeded()
        } else {
            hasPermission(PERMISSION_WRITE_STORAGE) && hasPermission(PERMISSION_CAMERA) &&
                    hasAudioPermissionIfNeeded()
        }
    }

    private fun setupOrientationEventListener() {
        mOrientationEventListener = object : OrientationEventListener(
            this, SensorManager.SENSOR_DELAY_NORMAL
        ) {
            override fun onOrientationChanged(orientation: Int) {
                if (isDestroyed) {
                    mOrientationEventListener.disable()
                    return
                }

                val currOrient = when (orientation) {
                    in 75..134 -> ORIENT_LANDSCAPE_RIGHT
                    in 225..289 -> ORIENT_LANDSCAPE_LEFT
                    else -> ORIENT_PORTRAIT
                }

                if (currOrient != mLastHandledOrientation) {
                    val degrees = when (currOrient) {
                        ORIENT_LANDSCAPE_LEFT -> 90
                        ORIENT_LANDSCAPE_RIGHT -> -90
                        else -> 0
                    }

                    animateViews(degrees)
                    mLastHandledOrientation = currOrient
                }
            }
        }
    }

    private fun animateViews(degrees: Int) = binding.apply {
        val views = arrayOf(
            toggleCamera,
            layoutTop.toggleTimer,
            layoutTop.toggleFlash,
            layoutTop.changeResolution,
            layoutTop.toggleExposure,
            layoutTop.toggleScene,
            layoutTop.toggleProControls,
            togglePause,
            shutter,
            layoutTop.settings,
            lastPhotoVideoPreview,
            layoutTimer.timerOff,
            layoutTimer.timer3s,
            layoutTimer.timer5s,
            layoutTimer.timer10S,
            layoutFlash.flashOff,
            layoutFlash.flashAuto,
            layoutFlash.flashOn,
            layoutFlash.flashAlwaysOn
        )
        for (view in views) {
            rotate(view, degrees)
        }
        mediaSizeToggleGroup?.children?.forEach { child ->
            rotate(child, degrees)
        }
        mZoomChips.rotate(degrees)
    }

    private fun rotate(view: View, degrees: Int) =
        view.animate().rotation(degrees.toFloat()).start()

    override fun setHasFrontAndBackCamera(hasFrontAndBack: Boolean) {
        binding.toggleCamera.beVisibleIf(hasFrontAndBack)
    }

    override fun setFlashAvailable(available: Boolean) {
        if (available) {
            binding.layoutTop.toggleFlash.beVisible()
        } else {
            binding.layoutTop.toggleFlash.beGone()
            mPreview?.setFlashlightState(FLASH_OFF)
        }
    }

    override fun onPhotoCaptureStart() {
        toggleActionButtons(enabled = false)
    }

    override fun onPhotoCaptureEnd() {
        mIsScreenFlashPending = false
        setScreenFlash(false)
        toggleActionButtons(enabled = true)
    }

    private fun toggleActionButtons(enabled: Boolean) = binding.apply {
        runOnUiThread {
            shutter.isClickable = enabled
            previewView.isEnabled = enabled
            layoutTop.changeResolution.isEnabled = enabled
            toggleCamera.isClickable = enabled
            layoutTop.toggleFlash.isClickable = enabled
        }
    }

    override fun shutterAnimation() {
        setScreenFlash(false)
        binding.shutterAnimation.alpha = 1.0f
        binding.shutterAnimation.animate().alpha(0f).setDuration(ANIMATION_DURATION).start()
    }

    override fun onMediaSaved(uri: Uri) {
        binding.layoutTop.changeResolution.isEnabled = true
        loadLastTakenMedia(uri)
        if (isImageCaptureIntent()) {
            Intent().apply {
                data = uri
                flags = Intent.FLAG_GRANT_READ_URI_PERMISSION
                setResult(RESULT_OK, this)
            }
            finish()
        } else if (isVideoCaptureIntent()) {
            Intent().apply {
                data = uri
                flags = Intent.FLAG_GRANT_READ_URI_PERMISSION
                setResult(RESULT_OK, this)
            }
            finish()
        }
    }

    override fun onImageCaptured(bitmap: Bitmap) {
        if (isImageCaptureIntent()) {
            Intent().apply {
                putExtra("data", bitmap)
                setResult(RESULT_OK, this)
            }
            finish()
        }
    }

    override fun onChangeFlashMode(flashMode: Int) {
        binding.layoutTop.apply {
            val flashDrawable = when (flashMode) {
                FLASH_OFF -> R.drawable.ic_flash_off_vector
                FLASH_ON -> R.drawable.ic_flash_on_vector
                FLASH_AUTO -> R.drawable.ic_flash_auto_vector
                else -> R.drawable.ic_flashlight_vector
            }
            toggleFlash.setShadowIcon(flashDrawable)
            toggleFlash.transitionName = "${getString(R.string.toggle_flash)}$flashMode"
        }
    }

    override fun onVideoRecordingStarted() {
        binding.apply {
            cameraModeHolder.beInvisible()
            videoRecCurrTimer.beVisible()
            togglePause.beVisible()

            toggleCamera.fadeOut()
            lastPhotoVideoPreview.fadeOut()

            layoutTop.changeResolution.isEnabled = false
            layoutTop.settings.isEnabled = false
            shutter.post {
                if (!isDestroyed) {
                    shutter.isSelected = true
                }
            }
        }
    }

    override fun onVideoRecordingStopped() {
        binding.apply {
            cameraModeHolder.beVisible()

            toggleCamera.fadeIn()
            lastPhotoVideoPreview.fadeIn()

            videoRecCurrTimer.text = 0.getFormattedDuration()
            videoRecCurrTimer.beGone()
            togglePause.beGone()
            resetPauseButton()

            shutter.isSelected = false
            layoutTop.changeResolution.isEnabled = true
            layoutTop.settings.isEnabled = true
        }
    }

    override fun onVideoDurationChanged(durationNanos: Long) {
        val seconds = TimeUnit.NANOSECONDS.toSeconds(durationNanos).toInt()
        binding.videoRecCurrTimer.text = seconds.getFormattedDuration()
    }

    override fun onFocusCamera(xPos: Float, yPos: Float) {
        mFocusCircleView.drawFocusCircle(xPos, yPos)
    }

    override fun onTouchPreview() {
        closeOptions()
    }

    private fun closeOptions(keepProPanel: Boolean = false): Boolean {
        binding.apply {
            val wasProPanelOpen = !keepProPanel && mProPanel.hide()
            val wasExposureOpen = exposureHolder.isVisible()
            if (wasExposureOpen) {
                exposureHolder.beGone()
            }

            if (mediaSizeToggleGroup?.isVisible() == true ||
                layoutFlash.flashToggleGroup.isVisible() || layoutTimer.timerToggleGroup.isVisible()
            ) {
                val transitionSet = createTransition()
                TransitionManager.go(defaultScene, transitionSet)
                mediaSizeToggleGroup?.beGone()
                layoutFlash.flashToggleGroup.beGone()
                layoutTimer.timerToggleGroup.beGone()
                layoutTop.defaultIcons.beVisible()
                return true
            }

            return wasExposureOpen || wasProPanelOpen
        }
    }

    private fun toggleProPanel() {
        if (mProPanel.isVisible) {
            mProPanel.hide()
        } else {
            closeOptions()
            mProPanel.toggle()
        }
    }

    override fun displaySelectedResolution(resolutionOption: ResolutionOption) {
        val imageRes = resolutionOption.imageDrawableResId
        binding.layoutTop.changeResolution.setShadowIcon(imageRes)
        binding.layoutTop.changeResolution.transitionName = "${resolutionOption.buttonViewId}"
    }

    override fun showImageSizes(
        selectedResolution: ResolutionOption,
        resolutions: List<ResolutionOption>,
        isPhotoCapture: Boolean,
        isFrontCamera: Boolean,
        onSelect: (index: Int, changed: Boolean) -> Unit
    ) {
        binding.topOptions.removeView(mediaSizeToggleGroup)
        val mediaSizeToggleGroup = createToggleGroup().apply {
            mediaSizeToggleGroup = this
        }

        binding.topOptions.addView(mediaSizeToggleGroup)

        val onItemClick = { clickedViewId: Int ->
            closeOptions()
            val index = resolutions.indexOfFirst { it.buttonViewId == clickedViewId }
            onSelect.invoke(index, selectedResolution.buttonViewId != clickedViewId)
        }

        val currentDegrees = when (mLastHandledOrientation) {
            ORIENT_LANDSCAPE_LEFT -> 90
            ORIENT_LANDSCAPE_RIGHT -> -90
            else -> 0
        }

        resolutions.forEach {
            val button = createButton(it, onItemClick)
            button.rotation = currentDegrees.toFloat()
            mediaSizeToggleGroup.addView(button)
        }

        mediaSizeToggleGroup.check(selectedResolution.buttonViewId)

        val transitionSet = createTransition()
        val mediaSizeScene = Scene(binding.topOptions, mediaSizeToggleGroup)
        TransitionManager.go(mediaSizeScene, transitionSet)
        binding.layoutTop.defaultIcons.beGone()
        mediaSizeToggleGroup.beVisible()
        mediaSizeToggleGroup.children.map { it as MaterialButton }.forEach(::setButtonColors)
    }

    private fun createButton(
        resolutionOption: ResolutionOption,
        onClick: (clickedViewId: Int) -> Unit
    ): MaterialButton {
        val params = LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT).apply {
            weight = 1f
        }

        return (layoutInflater.inflate(R.layout.layout_button, null) as MaterialButton).apply {
            layoutParams = params
            setShadowIcon(resolutionOption.imageDrawableResId)
            id = resolutionOption.buttonViewId
            transitionName = "${resolutionOption.buttonViewId}"
            setOnClickListener {
                onClick.invoke(id)
            }
        }
    }

    private fun createTransition(): Transition {
        val fadeTransition = Fade()
        return TransitionSet().apply {
            addTransition(fadeTransition)
            this.duration = resources.getInteger(R.integer.icon_anim_duration).toLong()
        }
    }

    override fun showFlashOptions(photoCapture: Boolean) {
        binding.layoutFlash.apply {
            val transitionSet = createTransition()
            TransitionManager.go(flashModeScene, transitionSet)
            flashAuto.beVisibleIf(photoCapture && !mScreenFlashMode)
            flashAlwaysOn.beVisibleIf(photoCapture && !mScreenFlashMode)
            flashToggleGroup.check(config.flashlightState.toFlashModeId())

            flashToggleGroup.beVisible()
            flashToggleGroup.children.forEach { setButtonColors(it as MaterialButton) }
        }
    }

    private fun setButtonColors(button: MaterialButton) {
        val primaryColor = getProperPrimaryColor()
        val states = arrayOf(
            intArrayOf(-android.R.attr.state_checked),
            intArrayOf(android.R.attr.state_checked)
        )
        val iconColors = intArrayOf(
            ContextCompat.getColor(this, org.fossify.commons.R.color.md_grey_white),
            primaryColor
        )
        button.iconTint = ColorStateList(states, iconColors)
    }

    override fun adjustPreviewView(requiresCentering: Boolean) {
        binding.apply {
            val constraintSet = ConstraintSet()
            constraintSet.clone(viewHolder)
            if (requiresCentering) {
                constraintSet.connect(
                    previewView.id,
                    ConstraintSet.TOP,
                    topOptions.id,
                    ConstraintSet.BOTTOM
                )
                constraintSet.connect(
                    previewView.id,
                    ConstraintSet.BOTTOM,
                    cameraModeHolder.id,
                    ConstraintSet.TOP
                )
            } else {
                constraintSet.connect(
                    previewView.id,
                    ConstraintSet.TOP,
                    ConstraintSet.PARENT_ID,
                    ConstraintSet.TOP
                )
                constraintSet.connect(
                    previewView.id,
                    ConstraintSet.BOTTOM,
                    ConstraintSet.PARENT_ID,
                    ConstraintSet.BOTTOM
                )
            }
            constraintSet.applyTo(viewHolder)
        }
    }

    override fun mediaSaved(path: String) {
        rescanPaths(arrayListOf(path)) {
            setupPreviewImage(true)
            Intent(BROADCAST_REFRESH_MEDIA).apply {
                putExtra(REFRESH_PATH, path)
                `package` = "org.fossify.gallery"
                sendBroadcast(this)
            }
        }

        if (isImageCaptureIntent()) {
            setResult(RESULT_OK)
            finish()
        }
    }

    private fun scheduleTimer(timerMode: TimerMode) {
        hideViewsOnTimerStart()
        binding.shutter.setImageState(intArrayOf(R.attr.state_timer_cancel), true)
        binding.timerText.beVisible()
        var playSound = true
        countDownTimer = object : CountDownTimer(timerMode.millisInFuture, 1000) {
            override fun onTick(millisUntilFinished: Long) {
                val seconds = (TimeUnit.MILLISECONDS.toSeconds(millisUntilFinished) + 1).toString()
                binding.timerText.setText(seconds)
                if (playSound && config.isSoundEnabled) {
                    if (millisUntilFinished <= TIMER_2_SECONDS) {
                        mediaSoundHelper.playTimerCountdown2SecondsSound()
                        playSound = false
                    } else {
                        mediaSoundHelper.playTimerCountdownSound()
                    }
                }
            }

            override fun onFinish() {
                cancelTimer()
                capturePhoto()
            }
        }.start()
    }

    private fun hideViewsOnTimerStart() = binding.apply {
        exposureHolder.beGone()
        arrayOf(topOptions, toggleCamera, lastPhotoVideoPreview, cameraModeHolder, zoomPresets)
            .forEach {
                it.fadeOut()
                it.beInvisible()
            }
    }

    private fun resetViewsOnTimerFinish() = binding.apply {
        val views = if (isThirdPartyIntent()) {
            // These stay hidden when another app asked for a picture or video.
            arrayOf(topOptions, toggleCamera, zoomPresets)
        } else {
            arrayOf(topOptions, toggleCamera, lastPhotoVideoPreview, cameraModeHolder, zoomPresets)
        }

        views.forEach {
            it.fadeIn()
            it.beVisible()
        }

        timerText.beGone()
        shutter.setImageState(intArrayOf(-R.attr.state_timer_cancel), true)
    }
}
