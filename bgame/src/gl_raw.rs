//! Creates an EGL context + window surface from raw handles (used by the Android build).
use glutin::config::{ConfigTemplateBuilder, GlConfig};
use glutin::context::{ContextApi, ContextAttributesBuilder, NotCurrentGlContext, PossiblyCurrentContext, Version};
use glutin::display::{Display, DisplayApiPreference, GlDisplay};
use glutin::surface::{GlSurface, Surface, SurfaceAttributesBuilder, SwapInterval, WindowSurface};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use std::num::NonZeroU32;

pub struct GlTarget {
    // drop order: context before surface before display
    pub context: PossiblyCurrentContext,
    pub surface: Surface<WindowSurface>,
    pub display: Display,
}

pub fn create(
    raw_display: RawDisplayHandle,
    raw_window: RawWindowHandle,
    w: u32,
    h: u32,
) -> Result<(GlTarget, glow::Context), Box<dyn std::error::Error>> {
    unsafe {
        let display = Display::new(raw_display, DisplayApiPreference::Egl)?;
        let template = ConfigTemplateBuilder::new()
            .with_depth_size(16)
            .compatible_with_native_window(raw_window)
            .build();
        let config = display
            .find_configs(template)?
            .reduce(|a, b| if b.num_samples() < a.num_samples() { b } else { a })
            .ok_or("no EGL config")?;
        let ctx_attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::Gles(Some(Version::new(2, 0))))
            .build(Some(raw_window));
        let not_current = display.create_context(&config, &ctx_attrs)?;
        let surf_attrs = SurfaceAttributesBuilder::<WindowSurface>::new().build(
            raw_window,
            NonZeroU32::new(w.max(1)).unwrap(),
            NonZeroU32::new(h.max(1)).unwrap(),
        );
        let surface = display.create_window_surface(&config, &surf_attrs)?;
        let context = not_current.make_current(&surface)?;
        let _ = surface.set_swap_interval(&context, SwapInterval::Wait(NonZeroU32::new(1).unwrap()));
        let gl = glow::Context::from_loader_function_cstr(|s| display.get_proc_address(s));
        Ok((GlTarget { context, surface, display }, gl))
    }
}
