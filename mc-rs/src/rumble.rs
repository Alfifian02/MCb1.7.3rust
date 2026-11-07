//! Controller vibration on the phone's side (`SDLCommonDriver.updateRumble` -> `SDL_RumbleGamepad(strong, weak, 5000)`): the commands of
//! `input/rumble_fx.rs` (`Cmd`) become `Vibrator` calls on the controller's `InputDevice`.
//!
//! * API 31+: the device's `VibratorManager`. A pad that exposes two vibrators gets the strong level on the first and the weak level on the second
//!   (the order the kernel's force-feedback motors are listed in: UNVERIFIED, a pad that lists them the other way round swaps the two); one with a single
//!   vibrator gets the stronger of the two levels.
//! * Before: `InputDevice.getVibrator`, one motor, the stronger level.
//! * Every level is a `VibrationEffect.createOneShot(ms, amplitude)` (API 26, the minimum), so unlike before the amplitude is a real amplitude and not folded
//!   into the duration. A motor at 0 is cancelled. A pad without amplitude control runs at its one speed (the framework decides).
//!
//! Only instance calls and one static factory, like `immersive.rs` / `storage.rs`. UNVERIFIED: not built or run on a device; the JNI shapes
//! (`call_static_method` with a found class, the int array of `getVibratorIds`) are the ones most likely to need a touch for the `jni 0.22` API.

use crate::input::rumble_fx::Cmd;
use android_activity::AndroidApp;
use jni::objects::{Global, JIntArray, JObject};
use jni::{jni_sig, jni_str, sys, Env, JValue, JavaVM};

/// Send `cmd` to the controller `device` (an Android input device id). Best effort: a controller without a motor does nothing.
pub fn apply(app: &AndroidApp, device: i32, cmd: Cmd) {
    let (vm, activity) = (app.vm_as_ptr(), app.activity_as_ptr());
    // SAFETY: both pointers come from `app`, which outlives this call.
    if let Err(e) = unsafe { run(vm, activity, device, cmd) } {
        log::debug!("rumble: {e:?}");
    }
}

unsafe fn run(vm: *mut core::ffi::c_void, activity: *mut core::ffi::c_void, device: i32, cmd: Cmd) -> jni::errors::Result<()> {
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
        if dev.is_null() {
            return Ok(()); // unplugged since
        }
        // API 31+: the device's vibrator manager; before: `InputDevice.getVibrator` (deprecated since 31, still there).
        match env.call_method(&dev, jni_str!("getVibratorManager"), jni_sig!(() -> android.os.VibratorManager), &[]) {
            Ok(m) => {
                let m = m.l()?;
                managed(env, &m, cmd)
            }
            Err(_) => {
                env.exception_clear();
                let vib = env.call_method(&dev, jni_str!("getVibrator"), jni_sig!(() -> android.os.Vibrator), &[])?.l()?;
                single(env, &vib, cmd)
            }
        }
    })
}

/// The levels of a `Vibrate`: (strong, weak), 0..=255 each.
fn levels(cmd: Cmd) -> Option<(u8, u8, u32)> {
    match cmd {
        Cmd::Vibrate { strong, weak, ms } => Some((strong, weak, ms)),
        Cmd::Cancel => None,
    }
}

/// `VibratorManager`: two vibrators take a level each, one takes the stronger.
fn managed(env: &mut Env<'_>, m: &JObject, cmd: Cmd) -> jni::errors::Result<()> {
    let Some((strong, weak, ms)) = levels(cmd) else {
        env.call_method(m, jni_str!("cancel"), jni_sig!(() -> void), &[])?;
        return Ok(());
    };
    let ids = vibrator_ids(env, m)?;
    if ids.len() >= 2 {
        for (id, amp) in [(ids[0], strong), (ids[1], weak)] {
            let vib = env.call_method(m, jni_str!("getVibrator"), jni_sig!((id: jint) -> android.os.Vibrator), &[JValue::Int(id)])?.l()?;
            single(env, &vib, if amp == 0 { Cmd::Cancel } else { Cmd::Vibrate { strong: amp, weak: amp, ms } })?;
        }
        Ok(())
    } else {
        let vib = env.call_method(m, jni_str!("getDefaultVibrator"), jni_sig!(() -> android.os.Vibrator), &[])?.l()?;
        single(env, &vib, cmd)
    }
}

/// `VibratorManager.getVibratorIds()`. UNVERIFIED shape: the `jni 0.22` array API (`cast_local`, `len`, `get_region`).
fn vibrator_ids(env: &mut Env<'_>, m: &JObject) -> jni::errors::Result<Vec<i32>> {
    let arr = env.call_method(m, jni_str!("getVibratorIds"), jni_sig!(() -> jint[]), &[])?.l()?;
    let arr = env.cast_local::<JIntArray>(arr)?;
    let n = arr.len(env)?;
    let mut ids = vec![0i32; n];
    arr.get_region(env, 0, &mut ids)?;
    Ok(ids)
}

/// One `Vibrator`: `vibrate(createOneShot(ms, max(strong, weak)))`, or `cancel()`.
fn single(env: &mut Env<'_>, vib: &JObject, cmd: Cmd) -> jni::errors::Result<()> {
    let Some((strong, weak, ms)) = levels(cmd) else {
        env.call_method(vib, jni_str!("cancel"), jni_sig!(() -> void), &[])?;
        return Ok(());
    };
    let amp = strong.max(weak).max(1) as i32;
    let class = env.find_class(jni_str!("android/os/VibrationEffect"))?;
    let effect = env
        .call_static_method(&class, jni_str!("createOneShot"), jni_sig!((ms: jlong, amplitude: jint) -> android.os.VibrationEffect), &[JValue::Long(ms as i64), JValue::Int(amp)])?
        .l()?;
    env.call_method(vib, jni_str!("vibrate"), jni_sig!((effect: android.os.VibrationEffect) -> void), &[JValue::Object(&effect)])?;
    Ok(())
}
