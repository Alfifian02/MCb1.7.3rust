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

/// The display's current refresh rate in Hz (`WindowManager.getDefaultDisplay().getRefreshRate()`: deprecated since API 30, still
/// right on 8-14), or `None` if the call fails. Not a view call, so any thread may make it; ask again when focus returns, because the
/// panel may have switched mode (60 / 90 / 120 Hz).
pub fn refresh_hz(app: &AndroidApp) -> Option<f32> {
    // SAFETY: both pointers come from `app`, which outlives the call.
    let r = unsafe { read_refresh(app.vm_as_ptr(), app.activity_as_ptr()) };
    r.map_err(|e| log::warn!("refresh rate not read: {e:?}")).ok().filter(|hz| hz.is_finite() && *hz >= 30.0)
}

unsafe fn read_refresh(vm: *mut c_void, activity: *mut c_void) -> jni::errors::Result<f32> {
    let vm = unsafe { JavaVM::from_raw(vm.cast::<sys::JavaVM>()) };
    vm.attach_current_thread(|env| {
        let raw = activity as sys::jobject;
        let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw)? };
        let windows = env
            .call_method(&*activity, jni_str!("getWindowManager"), jni_sig!(() -> android.view.WindowManager), &[])?
            .l()?;
        let display = env
            .call_method(&windows, jni_str!("getDefaultDisplay"), jni_sig!(() -> android.view.Display), &[])?
            .l()?;
        env.call_method(&display, jni_str!("getRefreshRate"), jni_sig!(() -> jfloat), &[])?.f()
    })
}

/// `PowerManager.isSustainedPerformanceModeSupported()` then `Window.setSustainedPerformanceMode(true)` (PPSSPP's `PpssppActivity`): the
/// system holds clocks the chip can keep up, so a long session does not fall off a thermal cliff. Costs some peak speed on a short one.
/// UNVERIFIED: not built or run on a device.
unsafe fn sustained(vm: *mut c_void, activity: *mut c_void, on: bool) -> jni::errors::Result<()> {
    let vm = unsafe { JavaVM::from_raw(vm.cast::<sys::JavaVM>()) };
    vm.attach_current_thread(|env| {
        let raw = activity as sys::jobject;
        let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw)? };
        let name = env.new_string("power")?;
        let pm = env
            .call_method(&*activity, jni_str!("getSystemService"), jni_sig!((name: java.lang.String) -> java.lang.Object), &[JValue::Object(&name)])?
            .l()?;
        if !env.call_method(&pm, jni_str!("isSustainedPerformanceModeSupported"), jni_sig!(() -> jboolean), &[])?.z()? {
            return Ok(());
        }
        let window = env
            .call_method(&*activity, jni_str!("getWindow"), jni_sig!(() -> android.view.Window), &[])?
            .l()?;
        env.call_method(&window, jni_str!("setSustainedPerformanceMode"), jni_sig!((enable: jboolean) -> void), &[JValue::Bool(on)])?;
        Ok(())
    })
}

/// Settings > Performance > Sustained Perf: switch the mode on or off (a window call, so on the Java main thread). A device without
/// support only logs.
pub fn set_sustained(app: &AndroidApp, on: bool) {
    let app2 = app.clone();
    app.run_on_java_main_thread(Box::new(move || {
        // SAFETY: both pointers come from `app2`, which this closure keeps alive.
        if let Err(e) = unsafe { sustained(app2.vm_as_ptr(), app2.activity_as_ptr(), on) } {
            log::warn!("sustained performance mode not set: {e:?}");
        }
    }));
}

/// `ANativeWindow_setFrameRate(window, hz, DEFAULT)`: tells the compositor the rate the game really runs at (0 = no preference), so a
/// 120 Hz panel is not driven faster than a 40 fps cap needs. The NDK call is API 30; this app's minimum is 26, so it is looked up with
/// `dlsym` and skipped where it is missing. Ask again for every new window. UNVERIFIED: not built or run on a device.
pub fn set_frame_rate(app: &AndroidApp, hz: f32) {
    type Fn = unsafe extern "C" fn(*mut c_void, f32, i8) -> i32;
    extern "C" {
        fn dlsym(handle: *mut c_void, name: *const core::ffi::c_char) -> *mut c_void;
    }
    let Some(win) = app.native_window() else { return };
    // SAFETY: `dlsym(RTLD_DEFAULT = null, ..)` only reads the symbol table; the pointer is a real `ANativeWindow_setFrameRate` or null,
    // and `win` keeps the window alive for the call.
    unsafe {
        let p = dlsym(core::ptr::null_mut(), c"ANativeWindow_setFrameRate".as_ptr());
        if p.is_null() {
            return;
        }
        let f: Fn = core::mem::transmute::<*mut c_void, Fn>(p);
        let r = f(win.ptr().as_ptr().cast(), hz, 0);
        log::info!("frame rate hint {hz} Hz -> {r}");
    }
}
