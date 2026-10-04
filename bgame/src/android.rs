//! Android entry point. Talks to NativeActivity directly (no winit) so we get raw gamepad
//! sticks, triggers, hat axes and button keycodes, plus multitouch.
use crate::engine::Engine;
use crate::gamepad::{Axes, PadButton};
use crate::gl_raw::{self, GlTarget};
use crate::input::{Input, Phase};
use android_activity::input::{Axis, InputEvent, KeyAction, Keycode, MotionAction, Source};
use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};
use glutin::context::PossiblyCurrentGlContext;
use glutin::surface::GlSurface;
use raw_window_handle::{AndroidDisplayHandle, AndroidNdkWindowHandle, RawDisplayHandle, RawWindowHandle};
use std::num::NonZeroU32;
use std::time::Duration;

fn pad_button(k: Keycode) -> Option<PadButton> {
    Some(match k {
        Keycode::ButtonA => PadButton::A,
        Keycode::ButtonB => PadButton::B,
        Keycode::ButtonX => PadButton::X,
        Keycode::ButtonY => PadButton::Y,
        Keycode::ButtonL1 => PadButton::L1,
        Keycode::ButtonR1 => PadButton::R1,
        Keycode::ButtonL2 => PadButton::L2,
        Keycode::ButtonR2 => PadButton::R2,
        Keycode::ButtonThumbl => PadButton::L3,
        Keycode::ButtonThumbr => PadButton::R3,
        Keycode::ButtonStart => PadButton::Start,
        Keycode::ButtonSelect => PadButton::Select,
        Keycode::ButtonMode => PadButton::Mode,
        Keycode::DpadUp => PadButton::DUp,
        Keycode::DpadDown => PadButton::DDown,
        Keycode::DpadLeft => PadButton::DLeft,
        Keycode::DpadRight => PadButton::DRight,
        _ => return None,
    })
}

fn handle(ev: &InputEvent, input: &mut Input) -> InputStatus {
    match ev {
        InputEvent::MotionEvent(m) => {
            if matches!(m.source(), Source::Touchscreen | Source::Mouse | Source::Stylus | Source::Touchpad) {
                match m.action() {
                    MotionAction::Down | MotionAction::PointerDown => {
                        let p = m.pointer_at_index(m.pointer_index());
                        input.touch(p.pointer_id() as u64, Phase::Down, p.x(), p.y());
                    }
                    MotionAction::Up | MotionAction::PointerUp => {
                        let p = m.pointer_at_index(m.pointer_index());
                        input.touch(p.pointer_id() as u64, Phase::Up, p.x(), p.y());
                    }
                    MotionAction::Move => {
                        for p in m.pointers() {
                            input.touch(p.pointer_id() as u64, Phase::Move, p.x(), p.y());
                        }
                    }
                    MotionAction::Cancel => input.touch_cancel_all(),
                    _ => {}
                }
            } else {
                // Joystick / gamepad: one pointer carrying every axis.
                let p = m.pointer_at_index(0);
                let ax = |a: Axis| p.axis_value(a);
                input.pad_axes(Axes {
                    lx: ax(Axis::X),
                    ly: ax(Axis::Y),
                    rx: ax(Axis::Z),
                    ry: ax(Axis::Rz),
                    lt: ax(Axis::Ltrigger).max(ax(Axis::Brake)),
                    rt: ax(Axis::Rtrigger).max(ax(Axis::Gas)),
                    hat_x: ax(Axis::HatX),
                    hat_y: ax(Axis::HatY),
                });
            }
            InputStatus::Handled
        }
        InputEvent::KeyEvent(k) => {
            let down = match k.action() {
                KeyAction::Down => true,
                KeyAction::Up => false,
                _ => return InputStatus::Unhandled,
            };
            match pad_button(k.key_code()) {
                Some(b) => {
                    input.pad_button(b, down);
                    InputStatus::Handled
                }
                None => InputStatus::Unhandled, // Back, volume etc. keep system behaviour
            }
        }
        _ => InputStatus::Unhandled,
    }
}

#[no_mangle]
fn android_main(app: AndroidApp) {
    let mut engine = Engine::new();
    let mut target: Option<GlTarget> = None;
    let mut last_size = (0u32, 0u32);
    let mut quit = false;

    while !quit {
        let timeout = if target.is_some() { Some(Duration::ZERO) } else { None };
        app.poll_events(timeout, |event| {
            if let PollEvent::Main(main) = event {
                match main {
                    MainEvent::InitWindow { .. } => {
                        if target.is_none() {
                            if let Some(win) = app.native_window() {
                                let mut wh = AndroidNdkWindowHandle::empty();
                                wh.a_native_window = win.ptr().as_ptr() as *mut std::ffi::c_void;
                                let (w, h) = (win.width() as u32, win.height() as u32);
                                match gl_raw::create(
                                    RawDisplayHandle::Android(AndroidDisplayHandle::empty()),
                                    RawWindowHandle::AndroidNdk(wh),
                                    w,
                                    h,
                                ) {
                                    Ok((t, gl)) => {
                                        engine.resize(w, h);
                                        engine.gfx_ready(gl);
                                        target = Some(t);
                                        last_size = (w, h);
                                    }
                                    Err(e) => eprintln!("EGL init failed: {e}"),
                                }
                            }
                        }
                    }
                    // The window is released right after this callback returns, so tear down EGL here.
                    MainEvent::TerminateWindow { .. } => {
                        engine.gfx_lost();
                        if let Some(t) = target.take() {
                            let _ = t.context.make_not_current();
                        }
                    }
                    MainEvent::Destroy => quit = true,
                    _ => {}
                }
            }
        });

        if let Ok(mut it) = app.input_events_iter() {
            while it.next(|e| handle(e, &mut engine.input)) {}
        }

        if let Some(t) = &target {
            if let Some(win) = app.native_window() {
                let size = (win.width() as u32, win.height() as u32);
                if size != last_size {
                    last_size = size;
                    engine.resize(size.0, size.1);
                    if let (Some(w), Some(h)) = (NonZeroU32::new(size.0), NonZeroU32::new(size.1)) {
                        t.surface.resize(&t.context, w, h);
                    }
                }
            }
            engine.frame();
            let _ = t.surface.swap_buffers(&t.context);
        }
    }
}
