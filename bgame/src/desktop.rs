//! Desktop dev harness (winit): keyboard/mouse/touch drive the same Engine as Android.
//! Keys: WASD move, arrows look, Space jump, Shift sneak, R sprint, F fly, V view distance, Z/LMB break, X/RMB place, 1-9 hotbar.
use crate::engine::Engine;
use crate::input::{Key, Phase};
use glutin::config::{ConfigTemplateBuilder, GlConfig};
use glutin::context::{ContextApi, ContextAttributesBuilder, NotCurrentGlContext, PossiblyCurrentContext, PossiblyCurrentGlContext, Version};
use glutin::display::{GetGlDisplay, GlDisplay};
use glutin::surface::{GlSurface, Surface, SwapInterval, WindowSurface};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::HasRawWindowHandle;
use std::num::NonZeroU32;
use winit::event::{ElementState, Event, MouseButton, TouchPhase, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop, EventLoopWindowTarget};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowBuilder};

struct Gfx {
    context: PossiblyCurrentContext,
    surface: Surface<WindowSurface>,
    window: Window,
}

struct App {
    engine: Engine,
    gfx: Option<Gfx>,
}

type Res<T> = Result<T, Box<dyn std::error::Error>>;

impl App {
    fn resume(&mut self, elwt: &EventLoopWindowTarget<()>) -> Res<()> {
        let template = ConfigTemplateBuilder::new().with_depth_size(16);
        let wb = || WindowBuilder::new().with_title("bgame");
        let (window, gl_config) = DisplayBuilder::new().with_window_builder(Some(wb())).build(elwt, template, |configs| {
            configs.reduce(|a, b| if b.num_samples() < a.num_samples() { b } else { a }).unwrap()
        })?;
        let window = match window {
            Some(w) => w,
            None => glutin_winit::finalize_window(elwt, wb(), &gl_config)?,
        };
        let display = gl_config.display();
        let attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::Gles(Some(Version::new(2, 0))))
            .build(Some(window.raw_window_handle()));
        let not_current = unsafe { display.create_context(&gl_config, &attrs)? };
        let surf_attrs = window.build_surface_attributes(Default::default());
        let surface = unsafe { display.create_window_surface(&gl_config, &surf_attrs)? };
        let context = not_current.make_current(&surface)?;
        let _ = surface.set_swap_interval(&context, SwapInterval::Wait(NonZeroU32::new(1).unwrap()));
        let gl = unsafe { glow::Context::from_loader_function_cstr(|s| display.get_proc_address(s)) };
        let s = window.inner_size();
        self.engine.resize(s.width, s.height);
        self.engine.gfx_ready(gl);
        self.gfx = Some(Gfx { context, surface, window });
        Ok(())
    }

    fn suspend(&mut self) {
        self.engine.gfx_lost();
        if let Some(g) = self.gfx.take() {
            let Gfx { context, surface, window } = g;
            let _ = context.make_not_current();
            drop(surface);
            drop(window);
        }
    }
}

fn map_key(code: KeyCode) -> Option<Key> {
    use KeyCode::*;
    Some(match code {
        KeyW => Key::W,
        KeyA => Key::A,
        KeyS => Key::S,
        KeyD => Key::D,
        Space => Key::Space,
        ShiftLeft | ShiftRight => Key::Shift,
        ArrowLeft => Key::Left,
        ArrowRight => Key::Right,
        ArrowUp => Key::Up,
        ArrowDown => Key::Down,
        KeyF => Key::Fly,
        KeyZ => Key::Break,
        KeyX => Key::Place,
        KeyR => Key::Sprint,
        KeyV => Key::View,
        Digit1 => Key::Num(1),
        Digit2 => Key::Num(2),
        Digit3 => Key::Num(3),
        Digit4 => Key::Num(4),
        Digit5 => Key::Num(5),
        Digit6 => Key::Num(6),
        Digit7 => Key::Num(7),
        Digit8 => Key::Num(8),
        Digit9 => Key::Num(9),
        _ => return None,
    })
}

pub fn run() {
    let event_loop = EventLoop::new().unwrap();
    let mut app = App { engine: Engine::new(), gfx: None };
    event_loop.set_control_flow(ControlFlow::Poll);
    let _ = event_loop.run(move |event, elwt| match event {
        Event::Resumed => {
            if let Err(e) = app.resume(elwt) {
                eprintln!("init failed: {e}");
                elwt.exit();
            }
        }
        Event::Suspended => app.suspend(),
        Event::AboutToWait => {
            if let Some(g) = &app.gfx {
                g.window.request_redraw();
            }
        }
        Event::WindowEvent { event, .. } => match event {
            WindowEvent::CloseRequested => elwt.exit(),
            WindowEvent::Resized(sz) => {
                app.engine.resize(sz.width, sz.height);
                if let (Some(g), Some(w), Some(h)) = (&app.gfx, NonZeroU32::new(sz.width), NonZeroU32::new(sz.height)) {
                    g.surface.resize(&g.context, w, h);
                }
            }
            WindowEvent::Touch(t) => {
                let phase = match t.phase {
                    TouchPhase::Started => Phase::Down,
                    TouchPhase::Moved => Phase::Move,
                    TouchPhase::Ended | TouchPhase::Cancelled => Phase::Up,
                };
                app.engine.input.touch(t.id, phase, t.location.x as f32, t.location.y as f32);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.repeat {
                    return;
                }
                if let PhysicalKey::Code(code) = event.physical_key {
                    if let Some(k) = map_key(code) {
                        app.engine.input.key(k, event.state == ElementState::Pressed);
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let down = state == ElementState::Pressed;
                match button {
                    MouseButton::Left => app.engine.input.key(Key::Break, down),
                    MouseButton::Right => app.engine.input.key(Key::Place, down),
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(g) = &app.gfx {
                    app.engine.frame();
                    let _ = g.surface.swap_buffers(&g.context);
                }
            }
            _ => {}
        },
        _ => {}
    });
}
