//! Hide the status + navigation bars (sticky immersive mode).
//!
//! NativeActivity has no NDK call for this, so go through JNI:
//! `getWindow().getDecorView().setSystemUiVisibility(flags)`. It is deprecated since
//! API 30 but still honoured on Android 11-14, and works on min_sdk 26. Views may only be
//! touched from the Java main thread, hence `run_on_java_main_thread`.

use android_activity::AndroidApp;
use core::ffi::c_void;
use jni::objects::{Global, JObject};
use jni::{jni_sig, jni_str, sys, JValue, JavaVM};

/// LAYOUT_STABLE | LAYOUT_HIDE_NAVIGATION | LAYOUT_FULLSCREEN
/// | HIDE_NAVIGATION | FULLSCREEN | IMMERSIVE_STICKY.
/// STICKY makes the bars re-hide by themselves after a swipe-from-edge reveal.
const FLAGS: i32 = 0x100 | 0x200 | 0x400 | 0x2 | 0x4 | 0x1000;

/// Ask the Java main thread to hide both bars. Cheap and idempotent: call it on window
/// creation and whenever focus returns (the system may have brought the bars back).
pub fn hide_system_bars(app: &AndroidApp) {
    let app2 = app.clone();
    app.run_on_java_main_thread(Box::new(move || {
        // SAFETY: both pointers come from `app2`, which this closure keeps alive.
        let (vm, activity) = (app2.vm_as_ptr(), app2.activity_as_ptr());
        if let Err(e) = unsafe { hide(vm, activity) } {
            log::error!("hide_system_bars failed: {e:?}");
        }
        // Separate attach so a failure here (Android < 9 has no such field) cannot affect the above.
        if let Err(e) = unsafe { use_cutout(vm, activity) } {
            log::warn!("cutout mode not set (needs Android 9+): {e:?}");
        }
    }));
}

unsafe fn hide(vm: *mut c_void, activity: *mut c_void) -> jni::errors::Result<()> {
    let vm = unsafe { JavaVM::from_raw(vm.cast::<sys::JavaVM>()) };
    vm.attach_current_thread(|env| {
        let raw = activity as sys::jobject;
        // The Activity global ref is owned by android-activity: borrow it, never delete it.
        let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw)? };
        let window = env
            .call_method(&*activity, jni_str!("getWindow"), jni_sig!(() -> android.view.Window), &[])?
            .l()?;
        let decor = env
            .call_method(&window, jni_str!("getDecorView"), jni_sig!(() -> android.view.View), &[])?
            .l()?;
        env.call_method(
            &decor,
            jni_str!("setSystemUiVisibility"),
            jni_sig!((flags: jint) -> void),
            &[JValue::Int(FLAGS)],
        )?;
        Ok(())
    })
}

/// LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES: draw into the notch area instead of leaving a
/// black strip. Without it Android keeps the window out of the cutout (the black bar on the left).
const CUTOUT_SHORT_EDGES: i32 = 1;

unsafe fn use_cutout(vm: *mut c_void, activity: *mut c_void) -> jni::errors::Result<()> {
    let vm = unsafe { JavaVM::from_raw(vm.cast::<sys::JavaVM>()) };
    vm.attach_current_thread(|env| {
        let raw = activity as sys::jobject;
        let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw)? };
        let window = env
            .call_method(&*activity, jni_str!("getWindow"), jni_sig!(() -> android.view.Window), &[])?
            .l()?;
        let attrs = env
            .call_method(
                &window,
                jni_str!("getAttributes"),
                jni_sig!(() -> android.view.WindowManager::LayoutParams),
                &[],
            )?
            .l()?;
        env.set_field(
            &attrs,
            jni_str!("layoutInDisplayCutoutMode"),
            jni_sig!(jint),
            JValue::Int(CUTOUT_SHORT_EDGES),
        )?;
        env.call_method(
            &window,
            jni_str!("setAttributes"),
            jni_sig!((attrs: android.view.WindowManager::LayoutParams) -> void),
            &[JValue::Object(&attrs)],
        )?;
        Ok(())
    })
}
