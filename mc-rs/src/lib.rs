//! mc-rs: Minecraft b1.7.3 in Rust for Android.
//!
//! M0 - Skeleton. Boots, renders a solid color, lifecycle correct.
//! No game code yet. See ROADMAP.md.

use android_activity::{AndroidApp, MainEvent, PollEvent};
use core::ffi::c_void;
use std::time::Duration;

// ANativeWindow C API (subset we need). Linked through `android` lib.
#[repr(C)]
struct WindowBuffer {
    width: i32,
    height: i32,
    stride: i32,
    format: i32,
    bits: *mut c_void,
    reserved: [u32; 6],
}

#[link(name = "android")]
extern "C" {
    fn ANativeWindow_setBuffersGeometry(
        window: *mut c_void,
        width: i32,
        height: i32,
        format: i32,
    ) -> i32;
    fn ANativeWindow_lock(
        window: *mut c_void,
        out: *mut WindowBuffer,
        dirty: *mut c_void,
    ) -> i32;
    fn ANativeWindow_unlockAndPost(window: *mut c_void) -> i32;
}

const WINDOW_FORMAT_RGBA_8888: i32 = 1;

// RGBA8888 little-endian: 0xAABBGGRR. Solid Minecraft grass-green for now.
const BG: u32 = 0xFF_50_C0_50; // A=255, B=0x50, G=0xC0, R=0x50 -> green

fn fill(buf: &mut WindowBuffer, color: u32) {
    if buf.bits.is_null() || buf.width <= 0 || buf.height <= 0 {
        return;
    }
    let w = buf.width as usize;
    let h = buf.height as usize;
    let stride = buf.stride as usize;
    let px = unsafe { std::slice::from_raw_parts_mut(buf.bits as *mut u32, stride * h) };
    for y in 0..h {
        for x in 0..w {
            px[y * stride + x] = color;
        }
    }
}

fn draw(app: &AndroidApp) {
    let Some(window) = app.native_window() else { return };
    let ptr = window.ptr().as_ptr() as *mut c_void;
    unsafe {
        ANativeWindow_setBuffersGeometry(ptr, 0, 0, WINDOW_FORMAT_RGBA_8888);
        let mut buf = WindowBuffer {
            width: 0,
            height: 0,
            stride: 0,
            format: 0,
            bits: core::ptr::null_mut(),
            reserved: [0; 6],
        };
        if ANativeWindow_lock(ptr, &mut buf, core::ptr::null_mut()) != 0 {
            return;
        }
        fill(&mut buf, BG);
        ANativeWindow_unlockAndPost(ptr);
    }
}

#[no_mangle]
fn android_main(app: AndroidApp) {
    let mut running = true;
    let mut redraw = true;
    while running {
        app.poll_events(Some(Duration::from_millis(100)), |event| {
            if let PollEvent::Main(main_event) = event {
                match main_event {
                    MainEvent::InitWindow { .. }
                    | MainEvent::WindowResized { .. }
                    | MainEvent::RedrawNeeded { .. } => {
                        redraw = true;
                    }
                    MainEvent::Destroy { .. } => running = false,
                    _ => {}
                }
            }
        });
        if redraw {
            draw(&app);
            redraw = false;
        }
    }
}
