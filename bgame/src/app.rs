use crate::{math, renderer::Renderer, world::World};
use glutin::config::{ConfigTemplateBuilder, GlConfig};
use glutin::context::{ContextApi, ContextAttributesBuilder, NotCurrentGlContext, PossiblyCurrentContext, PossiblyCurrentGlContext, Version};
use glutin::display::{GetGlDisplay, GlDisplay};
use glutin::surface::{GlSurface, Surface, SwapInterval, WindowSurface};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::HasRawWindowHandle;
use std::num::NonZeroU32;
use std::time::Instant;
use winit::event::{Event, TouchPhase, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop, EventLoopWindowTarget};
use winit::window::{Window, WindowBuilder};

const VIEW_RADIUS: i32 = 3;
const SEED: u32 = 1234;
const SPEED: f32 = 12.0;

// Field order matters: renderer drops first, window last.
struct Gfx {
    renderer: Renderer,
    context: PossiblyCurrentContext,
    surface: Surface<WindowSurface>,
    window: Window,
}

struct App {
    world: World,
    gfx: Option<Gfx>,
    pos: [f32; 3],
    yaw: f32,
    pitch: f32,
    last: Instant,
    size: (u32, u32),
    // (touch id, start, current): left half = move stick
    stick: Option<(u64, (f32, f32), (f32, f32))>,
    // (touch id, last): right half = look
    look: Option<(u64, (f32, f32))>,
}

type Res<T> = Result<T, Box<dyn std::error::Error>>;

impl App {
    fn new() -> Self {
        let world = World::new(SEED, VIEW_RADIUS);
        let y = bcore::worldgen::height_at(SEED, 0, 0) as f32 + 12.0;
        Self { world, gfx: None, pos: [0.0, y, 0.0], yaw: 0.0, pitch: -0.3, last: Instant::now(), size: (1, 1), stick: None, look: None }
    }

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
        self.size = (s.width, s.height);
        let renderer = Renderer::new(gl, &self.world);
        self.last = Instant::now();
        self.gfx = Some(Gfx { renderer, context, surface, window });
        Ok(())
    }

    fn suspend(&mut self) {
        if let Some(g) = self.gfx.take() {
            let Gfx { renderer, context, surface, window } = g;
            drop(renderer);
            let _ = context.make_not_current();
            drop(surface);
            drop(window);
        }
    }

    fn touch(&mut self, id: u64, phase: TouchPhase, x: f32, y: f32) {
        let left = x < self.size.0 as f32 * 0.5;
        match phase {
            TouchPhase::Started => {
                if left && self.stick.is_none() {
                    self.stick = Some((id, (x, y), (x, y)));
                } else if !left && self.look.is_none() {
                    self.look = Some((id, (x, y)));
                }
            }
            TouchPhase::Moved => {
                if let Some(s) = self.stick.as_mut().filter(|s| s.0 == id) {
                    s.2 = (x, y);
                }
                if let Some(l) = self.look.as_mut().filter(|l| l.0 == id) {
                    self.yaw += (x - l.1 .0) * 0.005;
                    self.pitch = (self.pitch - (y - l.1 .1) * 0.005).clamp(-1.5, 1.5);
                    l.1 = (x, y);
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                if self.stick.map_or(false, |s| s.0 == id) { self.stick = None; }
                if self.look.map_or(false, |l| l.0 == id) { self.look = None; }
            }
        }
    }

    fn update(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32().min(0.1);
        self.last = now;
        if let Some((_, a, b)) = self.stick {
            let sx = ((b.0 - a.0) / 120.0).clamp(-1.0, 1.0);
            let sy = ((a.1 - b.1) / 120.0).clamp(-1.0, 1.0); // drag up = forward
            let f = math::forward(self.yaw, self.pitch);
            let r = math::right(self.yaw);
            for i in 0..3 {
                self.pos[i] += (f[i] * sy + r[i] * sx) * SPEED * dt;
            }
        }
    }

    fn frame(&mut self) {
        self.update();
        let Some(g) = self.gfx.as_ref() else { return };
        let s = g.window.inner_size();
        self.size = (s.width, s.height);
        if s.width == 0 || s.height == 0 { return; }
        let proj = math::perspective(1.2, s.width as f32 / s.height as f32, 0.1, 300.0);
        let mvp = math::mul(&proj, &math::view(self.pos, self.yaw, self.pitch));
        g.renderer.draw(self.size, &mvp, self.pos, math::forward(self.yaw, self.pitch));
        let _ = g.surface.swap_buffers(&g.context);
    }
}

pub fn run(event_loop: EventLoop<()>) {
    let mut app = App::new();
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
            if let Some(g) = &app.gfx { g.window.request_redraw(); }
        }
        Event::WindowEvent { event, .. } => match event {
            WindowEvent::CloseRequested => elwt.exit(),
            WindowEvent::Resized(sz) => {
                app.size = (sz.width, sz.height);
                if let (Some(g), Some(w), Some(h)) = (&app.gfx, NonZeroU32::new(sz.width), NonZeroU32::new(sz.height)) {
                    g.surface.resize(&g.context, w, h);
                }
            }
            WindowEvent::Touch(t) => app.touch(t.id, t.phase, t.location.x as f32, t.location.y as f32),
            WindowEvent::RedrawRequested => app.frame(),
            _ => {}
        },
        _ => {}
    });
}
