//! Controller vibration (Controllify's `RumbleManager`, one motor): `InputManager.getInputDevice(id)` -> its `Vibrator` ->
//! `vibrate(ms)`. Only instance calls, like `immersive.rs`; the amplitude is folded into the duration (`vibrate(long)` has no
//! amplitude and the static `VibrationEffect` factory is not worth another JNI shape). UNVERIFIED: not built or run on a device.

use android_activity::AndroidApp;
use jni::objects::{Global, JObject};
use jni::{jni_sig, jni_str, sys, JValue, JavaVM};

/// Buzz the controller `device` for about `ms` (scaled by `amp` / 255). Best effort: a controller without a motor does nothing.
pub fn buzz(app: &AndroidApp, device: i32, ms: u32, amp: u8) {
    let ms = (ms as f32 * amp as f32 / 255.0).max(10.0) as i64;
    let (vm, activity) = (app.vm_as_ptr(), app.activity_as_ptr());
    // SAFETY: both pointers come from `app`, which outlives this call.
    if let Err(e) = unsafe { vibrate(vm, activity, device, ms) } {
        log::debug!("rumble: {e:?}");
    }
}

unsafe fn vibrate(vm: *mut core::ffi::c_void, activity: *mut core::ffi::c_void, device: i32, ms: i64) -> jni::errors::Result<()> {
    let vm = unsafe { JavaVM::from_raw(vm.cast::<sys::JavaVM>()) };
    vm.attach_current_thread(|env| {
        let raw = activity as sys::jobject;
        let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw)? };
        let name = env.new_string("input")?;
        let im = env
            .call_method(&*activity, jni_str!("getSystemService"), jni_sig!((name: java.lang.String) -> java.lang.Object), &[JValue::Object(&name)])?
            .l()?;
        let dev = env
            .call_method(&im, jni_str!("getInputDevice"), jni_sig!((id: jint) -> android.view.InputDevice), &[JValue::Int(device)])?
            .l()?;
        // API 31+: the device's vibrator manager; before: `InputDevice.getVibrator` (deprecated since 31, still there).
        let vib = match env.call_method(&dev, jni_str!("getVibratorManager"), jni_sig!(() -> android.os.VibratorManager), &[]) {
            Ok(m) => {
                let m = m.l()?;
                env.call_method(&m, jni_str!("getDefaultVibrator"), jni_sig!(() -> android.os.Vibrator), &[])?.l()?
            }
            Err(_) => {
                env.exception_clear();
                env.call_method(&dev, jni_str!("getVibrator"), jni_sig!(() -> android.os.Vibrator), &[])?.l()?
            }
        };
        env.call_method(&vib, jni_str!("vibrate"), jni_sig!((ms: jlong) -> void), &[JValue::Long(ms)])?;
        Ok(())
    })
}
