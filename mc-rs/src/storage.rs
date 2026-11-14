//! Storage permission, for Import Custom Skin's picker (NOT b1.7.3). A NativeActivity gets no `onActivityResult`, so the system file picker
//! (`ACTION_OPEN_DOCUMENT`) cannot hand its answer back without a Java class; the game asks for the media read permission instead and lists
//! the skin `.png` files of Download / Pictures itself (`skin::found`). Reading a picture by its path works on Android 8-14 once that
//! permission is given: `READ_EXTERNAL_STORAGE` up to API 32, `READ_MEDIA_IMAGES` from 33 (the other one is simply never granted there).
//! `requestPermissions` has no result call-back either, so `granted` is asked again on the next tap of the button.
//! UNVERIFIED: written without a toolchain, not built or run on a device (the JNI calls follow `immersive.rs` / `rumble.rs`).

use android_activity::AndroidApp;
use core::ffi::c_void;
use jni::objects::{Global, JObject, JObjectArray};
use jni::{jni_sig, jni_str, sys, JValue, JavaVM};

const PERMS: [&str; 2] = ["android.permission.READ_MEDIA_IMAGES", "android.permission.READ_EXTERNAL_STORAGE"];

/// Can the app read pictures in shared storage now?
pub fn granted(app: &AndroidApp) -> bool {
    // SAFETY: both pointers come from `app`, which outlives the call.
    unsafe { check(app.vm_as_ptr(), app.activity_as_ptr()) }.unwrap_or_else(|e| {
        log::warn!("storage permission not read: {e:?}");
        false
    })
}

unsafe fn check(vm: *mut c_void, activity: *mut c_void) -> jni::errors::Result<bool> {
    let vm = unsafe { JavaVM::from_raw(vm.cast::<sys::JavaVM>()) };
    vm.attach_current_thread(|env| {
        let raw = activity as sys::jobject;
        let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw)? };
        for p in PERMS {
            let name = env.new_string(p)?;
            let r = env.call_method(&*activity, jni_str!("checkSelfPermission"), jni_sig!((permission: java.lang.String) -> jint), &[JValue::Object(&name)])?.i()?;
            if r == 0 {
                return Ok(true); // PERMISSION_GRANTED
            }
        }
        Ok(false)
    })
}

/// Show the system's permission dialog unless the permission is already given. Cheap: call it whenever a picker found nothing.
pub fn request(app: &AndroidApp) {
    if granted(app) {
        return;
    }
    let app2 = app.clone();
    app.run_on_java_main_thread(Box::new(move || {
        // SAFETY: both pointers come from `app2`, which this closure keeps alive.
        if let Err(e) = unsafe { ask(app2.vm_as_ptr(), app2.activity_as_ptr()) } {
            log::error!("storage permission request failed: {e:?}");
        }
    }));
}

unsafe fn ask(vm: *mut c_void, activity: *mut c_void) -> jni::errors::Result<()> {
    let vm = unsafe { JavaVM::from_raw(vm.cast::<sys::JavaVM>()) };
    vm.attach_current_thread(|env| {
        let raw = activity as sys::jobject;
        let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw)? };
        let class = env.find_class(jni_str!("java/lang/String"))?;
        let list: JObjectArray = env.new_object_array(PERMS.len() as i32, &class, JObject::null())?;
        for (i, p) in PERMS.into_iter().enumerate() {
            let s = env.new_string(p)?;
            list.set_element(env, i, &s)?;
        }
        env.call_method(&*activity, jni_str!("requestPermissions"), jni_sig!((permissions: java.lang.String[], request: jint) -> void), &[JValue::Object(&list), JValue::Int(1)])?;
        Ok(())
    })
}
