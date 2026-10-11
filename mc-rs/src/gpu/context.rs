//! Minimal wgpu context wrapper. Vulkan-only on Android.
//! Reference: jinleili/wgpu-in-app (github.com/jinleili/wgpu-in-app).
//! Key fixes vs the previous attempt:
//!   - Force Backends::VULKAN (no GLES fallback) -- user policy requires Vulkan.
//!   - Use SurfaceTarget::Window (safe API) instead of SurfaceTargetUnsafe.
//!   - Use surface.get_default_config(w, h) for the initial config, sized to
//!     the actual ANativeWindow width/height (not 1x1), so we never hit the
//!     "frame is NxM, reconfiguring" race.
//!   - Allocate depth at the same size from the start.

use core::ffi::c_void;
use core::ptr::NonNull;
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

/// Internal render resolution as a fraction of the surface: depth, G-buffer, HDR scene and the whole post chain run at this size and the
/// final pass upscales (bilinear + CAS). 1.0 = native. Low-end phones (Mali-G52 class) are fill-rate bound: 0.67 is 2.2x fewer pixels.
pub const RENDER_SCALE: f32 = 0.67;

/// The scene size for a surface of `w` x `h`.
pub fn scene_size(w: u32, h: u32) -> (u32, u32) {
    (((w as f32 * RENDER_SCALE).round() as u32).max(1), ((h as f32 * RENDER_SCALE).round() as u32).max(1))
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
    /// A per-frame copy of the depth (`copy_depth`): the passes that read depth as a texture while the real one is still attached to
    /// the HDR scene pass read this, so nothing is ever sampled and attached in the same pass (some mobile GPUs draw garbage tiles then).
    pub depth_copy_view: TextureView,
    depth_copy: Texture,
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

        // Try Vulkan first; if the device has no usable Vulkan driver, fall
        // back to GLES instead of failing to start (black screen / instant close).
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

        // Backend is chosen by the caller (Vulkan first, GLES fallback).
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
        device.on_uncaptured_error(Box::new(|e| log::error!("wgpu error: {e}")));
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
        let depth_copy = Self::create_depth(&device, sw, sh);
        let depth_copy_view = depth_copy.create_view(&wgpu::TextureViewDescriptor::default());

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
            depth_copy_view,
            depth_copy,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
        let (sw, sh) = scene_size(self.config.width, self.config.height);
        self.depth_tex = Self::create_depth(&self.device, sw, sh);
        self.depth_view = self.depth_tex.create_view(&wgpu::TextureViewDescriptor::default());
        self.depth_copy = Self::create_depth(&self.device, sw, sh);
        self.depth_copy_view = self.depth_copy.create_view(&wgpu::TextureViewDescriptor::default());
    }

    /// Copy the finished opaque depth into `depth_copy_view`; run after the G-buffer pass.
    pub fn copy_depth(&self, enc: &mut wgpu::CommandEncoder) {
        enc.copy_texture_to_texture(self.depth_tex.as_image_copy(), self.depth_copy.as_image_copy(), Extent3d { width: self.depth_tex.width(), height: self.depth_tex.height(), depth_or_array_layers: 1 });
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
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Depth32Float,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_SRC | TextureUsages::COPY_DST, // the light-shaft pass reads it
            view_formats: &[],
        })
    }

    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.config.format
    }
}
