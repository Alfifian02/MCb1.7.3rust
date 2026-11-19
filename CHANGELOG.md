# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
### Added
- Composition grid overlay (rule of thirds, 4×4 grid, golden ratio)
- Level indicator (horizon) driven by the accelerometer
- Zoom shortcuts (including ultra wide, if the device supports it) with a live zoom value
- Exposure compensation slider
- Scene modes (auto, HDR, night, portrait, face retouch) on devices that support camera extensions
- Pause and resume video recording
- Option to record videos without audio
- Screen flash for the front camera
- Volume buttons can now also be used to zoom
- Haptic feedback option
- Options to always open with the back camera and to turn the flash off at startup
- "Pro" panel with video frame rate (auto, 30, 60 fps), video stabilization, burst and time lapse
  (photos at a fixed interval), RAW (DNG) capture, manual ISO, shutter speed and white balance, and
  a live luminance histogram. Each option only shows up when the camera supports it
- Unit tests (`app/src/test`) and a `Checks` workflow that runs tests, detekt and lint on every pull request

### Fixed
- Camera failed to start on devices without a front camera
- Photos taken within the same second could overwrite each other
- Duplicated error messages and leaked camera observers after switching camera or mode
- Microphone permission was requested when switching to photo mode
- High resolution sizes were not offered after changing the capture mode until the app was restarted
- Holding a volume button used as shutter also changed the system volume
- Camera mode tabs reappeared after the self timer in apps requesting a photo or video
- Last photo thumbnail is now loaded off the UI thread and cleared when no media is left
- Possible crash when a stored preference value was out of range

### Changed
- The focus indicator now scales with screen density and animates

## [1.5.0] - 2026-01-30
### Added
- Added support for custom fonts
- Option for maximum screen brightness in preview ([#234])

### Changed
- Updated translations

## [1.4.0] - 2025-10-29
### Changed
- Compatibility updates for Android 15 & 16
- Updated translations

## [1.3.1] - 2025-10-05
### Changed
- Updated translations

### Fixed
- Fixed app crash when location service is disabled on Android 10 and prior ([#177])

## [1.3.0] - 2025-09-17
### Changed
- Swapped positions of camera switch and image preview buttons ([#157])
- Updated and animated the switch camera button

## [1.2.0] - 2025-09-09
### Added
- Support high-resolution output modes if the config preference is set to maximize quality. ([#20])

### Changed
- Updated translations

## [1.1.0] - 2025-07-21
### Changed
- Preference category labels now use sentence case
- Updated translations

## [1.0.4] - 2025-06-17
### Changed
- Updated translations

## [1.0.3] - 2025-06-09
### Changed
- Updated translations

### Fixed
- Fixed loud shutter sound on some devices ([#97])

## [1.0.2] - 2025-05-28
### Changed
- Updated translations
- Marked app as beta

### Fixed
- All camera buttons now rotate properly on device rotation
- Buttons are now properly updated switching between photo and video modes

## [1.0.1] - 2024-10-19
### Fixed
- Fix app name in metadata

## [1.0.0] - 2024-10-19
### Added
- Initial release

[#20]: https://github.com/FossifyOrg/Camera/issues/20
[#97]: https://github.com/FossifyOrg/Camera/issues/97
[#157]: https://github.com/FossifyOrg/Camera/issues/157
[#177]: https://github.com/FossifyOrg/Camera/issues/177
[#234]: https://github.com/FossifyOrg/Camera/issues/234

[Unreleased]: https://github.com/FossifyOrg/Camera/compare/1.5.0...HEAD
[1.5.0]: https://github.com/FossifyOrg/Camera/compare/1.4.0...1.5.0
[1.4.0]: https://github.com/FossifyOrg/Camera/compare/1.3.1...1.4.0
[1.3.1]: https://github.com/FossifyOrg/Camera/compare/1.3.0...1.3.1
[1.3.0]: https://github.com/FossifyOrg/Camera/compare/1.2.0...1.3.0
[1.2.0]: https://github.com/FossifyOrg/Camera/compare/1.1.0...1.2.0
[1.1.0]: https://github.com/FossifyOrg/Camera/compare/1.0.4...1.1.0
[1.0.4]: https://github.com/FossifyOrg/Camera/compare/1.0.3...1.0.4
[1.0.3]: https://github.com/FossifyOrg/Camera/compare/1.0.2...1.0.3
[1.0.2]: https://github.com/FossifyOrg/Camera/compare/1.0.1...1.0.2
[1.0.1]: https://github.com/FossifyOrg/Camera/compare/1.0.0...1.0.1
[1.0.0]: https://github.com/FossifyOrg/Camera/releases/tag/1.0.0
