//! Minimal wgpu context wrapper. Vulkan or OpenGL ES (wgpu's GL backend), Vulkan first; the other is the fallback.
//! Reference: jinleili/wgpu-in-app (github.com/jinleili/wgpu-in-app).
//! Key fixes vs the previous attempt:
//!   - One backend at a time (Vulkan first, OpenGL ES is the fallback).
//!   - Use SurfaceTarget::Window (safe API) instead of SurfaceTargetUnsafe.
//!   - Use surface.get_default_config(w, h) for the initial config, sized to
//!     the actual ANativeWindow width/height (not 1x1), so we never hit the
//!     "frame is NxM, reconfiguring" race.
//!   - Allocate depth at the same size from the start.

use core::ffi::c_void;
use core::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use raw_window_handle::{
    AndroidDisplayHandle, AndroidNdkWindowHandle, HasDisplayHandle, HasWindowHandle,
    RawDisplayHandle, RawWindowHandle,
};
use wgpu::{Adapter, Device, Extent3d, Instance, Queue, Surface, SurfaceConfiguration, Texture, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView};

/// Wrapper for an Android ANativeWindow pointer so we can use it as a wgpu surface target.
struct AndroidWindow(NonNull<c_void>);
// SAFETY: ANativeWindow pointers are not Send by default in the bindings, but
// we only share them with the wgpu instance which knows how to keep them alive.
unsafe impl Send for AndroidWindow {}
unsafe impl Sync for AndroidWindow {}

impl Clone for AndroidWindow {
    fn clone(&self) -> Self {
        AndroidWindow(self.0)
    }
}

impl HasDisplayHandle for AndroidWindow {
    fn display_handle(&self) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        let raw = RawDisplayHandle::Android(AndroidDisplayHandle::new());
        // SAFETY: raw handle is valid for the lifetime of self.
        Ok(unsafe { raw_window_handle::DisplayHandle::borrow_raw(raw) })
    }
}
impl HasWindowHandle for AndroidWindow {
    fn window_handle(&self) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let handle = AndroidNdkWindowHandle::new(self.0);
        let raw = RawWindowHandle::AndroidNdk(handle);
        // SAFETY: raw handle is valid for the lifetime of self.
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(raw) })
    }
}

/// Internal render resolution as a fraction of the surface: the scene pass (colour + depth) runs at this size and the
/// final pass upscales (bilinear + CAS). 1.0 = native. Low-end phones (Mali-G52 class) are fill-rate bound: 0.67 is 2.2x fewer pixels.
pub const RENDER_SCALE: f32 = 0.67;

/// The scene size for a surface of `w` x `h`.
pub fn scene_size(w: u32, h: u32) -> (u32, u32) {
    (((w as f32 * RENDER_SCALE).round() as u32).max(1), ((h as f32 * RENDER_SCALE).round() as u32).max(1))
}

/// `gpuErrorTrace:true` in `options.txt` (ImmediatelyFast's `debug_only_print_additional_error_information`): a wgpu error is logged
/// with the Rust backtrace of the call that made it, so a validation error names the draw or the upload behind it. Off by default
/// (capturing a backtrace is slow).
static TRACE_ERRORS: AtomicBool = AtomicBool::new(false);
static LAST_TRACE: Mutex<Option<Instant>> = Mutex::new(None);

pub fn set_error_trace(on: bool) {
    TRACE_ERRORS.store(on, Ordering::Relaxed);
}

/// Whether the error at `now` gets a backtrace: the flag is on and the last trace was over a second ago (ImmediatelyFast's throttle: a
/// broken frame repeats its error 60 times a second, and a trace is a screenful).
fn wants_trace(on: bool, last: &mut Option<Instant>, now: Instant) -> bool {
    if !on || last.is_some_and(|t| now.duration_since(t) < Duration::from_secs(1)) {
        return false;
    }
    *last = Some(now);
    true
}

#[cfg(test)]
mod trace_tests {
    use super::*;

    #[test]
    fn a_trace_is_at_most_one_a_second_and_only_when_on() {
        let (t0, mut last) = (Instant::now(), None);
        assert!(!wants_trace(false, &mut last, t0), "off");
        assert!(last.is_none());
        assert!(wants_trace(true, &mut last, t0), "the first");
        assert!(!wants_trace(true, &mut last, t0 + Duration::from_millis(900)), "too soon");
        assert!(wants_trace(true, &mut last, t0 + Duration::from_millis(1000)), "a second on");
        assert!(!wants_trace(true, &mut last, t0 + Duration::from_millis(1500)), "counted from the last trace");
    }
}

pub struct Gpu {
    pub instance: Instance,
    pub surface: Surface<'static>,
    pub adapter: Adapter,
    pub device: Device,
    pub queue: Queue,
    pub config: SurfaceConfiguration,
    pub depth_view: TextureView,
    depth_tex: Texture,
}

impl Gpu {
    /// Build a surface from a raw ANativeWindow pointer and the actual physical
    /// size read from the window before init.
    /// # Safety
    /// - `native_ptr` must be a valid, non-null ANativeWindow from android-activity.
    pub async fn from_android_window(
        native_ptr: *mut c_void,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        if native_ptr.is_null() {
            return Err("null ANativeWindow".into());
        }
        let nn = unsafe { NonNull::new_unchecked(native_ptr) };
        let width = width.max(1);
        let height = height.max(1);
        log::info!("M3 init: native window {width}x{height}");

        // Vulkan first; if the device has no usable driver for it, fall back to GLES instead of
        // failing to start (black screen / instant close).
        let mut last_err = String::new();
        for backends in [wgpu::Backends::VULKAN, wgpu::Backends::GL] {
            match Self::try_init(AndroidWindow(nn), backends, width, height).await {
                Ok(g) => return Ok(g),
                Err(e) => {
                    log::warn!("GPU init with {backends:?} failed: {e}");
                    last_err = e;
                }
            }
        }
        Err(last_err)
    }

    async fn try_init(
        window: AndroidWindow,
        backends: wgpu::Backends,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {

        // One backend per try (Vulkan, then GLES).
        let instance = Instance::new(&wgpu::InstanceDescriptor {
            backends,
            ..Default::default()
        });

        let handle: Box<dyn wgpu::WindowHandle> = Box::new(window.clone());
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Window(handle))
            .map_err(|e| format!("create_surface: {e:?}"))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .map_err(|e| format!("adapter request: {e:?}"))?;

        let info = adapter.get_info();
        log::info!(
            "M3 init: adapter={:?} backend={:?} vendor=0x{:x} device=0x{:x}",
            info.name, info.backend, info.vendor, info.device
        );

        // MSAA for the scene pass if the adapter has it for the scene and depth formats (before anything is built with a sample count).
        let samples = crate::render::present::pick_samples(&adapter);
        crate::render::present::set_scene_samples(samples);
        log::info!("M3 init: scene MSAA x{samples}");

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("mc-rs"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| format!("request_device: {e:?}"))?;
        device.on_uncaptured_error(Box::new(|e| {
            let on = TRACE_ERRORS.load(Ordering::Relaxed);
            if wants_trace(on, &mut LAST_TRACE.lock().unwrap_or_else(|p| p.into_inner()), Instant::now()) {
                log::error!("wgpu error: {e}\n{}", std::backtrace::Backtrace::force_capture());
            } else {
                log::error!("wgpu error: {e}");
            }
        }));
        log::info!("M3 init: device + queue acquired");

        let caps = surface.get_capabilities(&adapter);
        log::info!(
            "M3 init: caps: formats={:?} present_modes={:?} alpha_modes={:?}",
            caps.formats, caps.present_modes, caps.alpha_modes
        );

        // Use get_default_config to get the format + present_mode the adapter
        // actually wants. This is what jinleili/wgpu-in-app does and it works
        // on every Android device tested.
        let mut config = surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| "get_default_config returned None".to_string())?;
        // Override present mode to Fifo if available (safer than Mailbox on
        // Android Vulkan drivers that lie about Mailbox support).
        if caps.present_modes.contains(&wgpu::PresentMode::Fifo) {
            config.present_mode = wgpu::PresentMode::Fifo;
        }
        // Android doesn't support view_formats; force empty to avoid validation
        // error (SURFACE_VIEW_FORMATS not in downlevel properties).
        config.view_formats = vec![];
        log::info!(
            "M3 init: config format={:?} view_formats={:?} present_mode={:?}",
            config.format, config.view_formats, config.present_mode
        );

        surface.configure(&device, &config);

        // Depth texture at the real size from the start -- no resize race.
        let (sw, sh) = scene_size(width, height);
        let depth_tex = Self::create_depth(&device, sw, sh);
        let depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());

        // Leak the AndroidWindow wrapper so the surface has a stable owner.
        // The pointer came from android-activity and lives as long as the window does.
        std::mem::forget(window);

        Ok(Self {
            instance,
            surface,
            adapter,
            device,
            queue,
            config,
            depth_view,
            depth_tex,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
        let (sw, sh) = scene_size(self.config.width, self.config.height);
        self.depth_tex = Self::create_depth(&self.device, sw, sh);
        self.depth_view = self.depth_tex.create_view(&wgpu::TextureViewDescriptor::default());
    }

    /// Size of the depth / G-buffer / HDR scene (`scene_size` of the surface).
    pub fn scene(&self) -> (u32, u32) {
        (self.depth_tex.width(), self.depth_tex.height())
    }

    fn create_depth(device: &Device, width: u32, height: u32) -> Texture {
        device.create_texture(&TextureDescriptor {
            label: Some("depth"),
            size: Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: crate::render::present::scene_samples(),
            dimension: TextureDimension::D2,
            format: TextureFormat::Depth32Float,
            // GPUBooster's RBO depth: attachment only (never sampled or copied), which wgpu's GLES backend makes a renderbuffer; nothing else reads it.
            usage: TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
    }

    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.config.format
    }
}
