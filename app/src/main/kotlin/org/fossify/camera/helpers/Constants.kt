package org.fossify.camera.helpers

const val ORIENT_PORTRAIT = 0
const val ORIENT_LANDSCAPE_LEFT = 1
const val ORIENT_LANDSCAPE_RIGHT = 2

// shared preferences
const val SAVE_PHOTOS = "save_photos"
const val SOUND = "sound"
const val VOLUME_BUTTONS_AS_SHUTTER = "volume_buttons_as_shutter"
const val FLIP_PHOTOS = "flip_photos"
const val LAST_USED_CAMERA = "last_used_camera_3"
const val LAST_USED_CAMERA_LENS = "last_used_camera_lens"
const val FLASHLIGHT_STATE = "flashlight_state"
const val INIT_PHOTO_MODE = "init_photo_mode"
const val BACK_PHOTO_RESOLUTION_INDEX = "back_photo_resolution_index_3"
const val BACK_VIDEO_RESOLUTION_INDEX = "back_video_resolution_index_3"
const val FRONT_PHOTO_RESOLUTION_INDEX = "front_photo_resolution_index_3"
const val FRONT_VIDEO_RESOLUTION_INDEX = "front_video_resolution_index_3"
const val SAVE_PHOTO_METADATA = "save_photo_metadata"
const val SAVE_PHOTO_VIDEO_LOCATION = "save_photo_video_location"
const val PHOTO_QUALITY = "photo_quality"
const val CAPTURE_MODE = "capture_mode"
const val TIMER_MODE = "timer_mode"
const val MAX_BRIGHTNESS = "max_brightness"
const val GRID_TYPE = "grid_type"
const val SHOW_LEVEL = "show_level"
const val VOLUME_BUTTONS_ACTION = "volume_buttons_action"
const val HAPTIC_FEEDBACK = "haptic_feedback"
const val RECORD_AUDIO_ENABLED = "record_audio_enabled"
const val ALWAYS_OPEN_BACK_CAMERA = "always_open_back_camera"
const val TURN_FLASH_OFF_AT_STARTUP = "turn_flash_off_at_startup"
const val SCENE_MODE = "scene_mode"
const val VIDEO_FRAME_RATE = "video_frame_rate"
const val VIDEO_STABILIZATION = "video_stabilization"
const val DRIVE_MODE = "drive_mode"
const val BURST_SIZE = "burst_size"
const val TIMELAPSE_INTERVAL = "timelapse_interval"
const val RAW_ENABLED = "raw_enabled"
const val SHOW_HISTOGRAM = "show_histogram"

const val FLASH_OFF = 0
const val FLASH_ON = 1
const val FLASH_AUTO = 2
const val FLASH_ALWAYS_ON = 3

fun compensateDeviceRotation(orientation: Int) = when (orientation) {
    ORIENT_LANDSCAPE_LEFT -> 270
    ORIENT_LANDSCAPE_RIGHT -> 90
    else -> 0
}
