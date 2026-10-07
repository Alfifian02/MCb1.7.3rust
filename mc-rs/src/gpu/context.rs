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

        // Force Vulkan. User policy: "require Vulkan". Some Android Vulkan
        // drivers had issues under wgpu 26, but the GLES fallback path is
        // even less reliable on Android, so we go Vulkan-only here.
        // Build the instance with a display handle tied to the Android
        // window. new_with_display_handle is what jinleili/wgpu-in-app uses
        // and is required on Android -- a display-less instance can fail
        // create_surface on some Vulkan drivers with no error message.
        let window = AndroidWindow(nn);
        let instance = Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..wgpu::InstanceDescriptor::new_with_display_handle(Box::new(window.clone()))
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
                required_limits: wgpu::Limits::downlevel_defaults(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| format!("request_device: {e:?}"))?;
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
        let depth_tex = Self::create_depth(&device, width, height);
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
        self.depth_tex = Self::create_depth(&self.device, self.config.width, self.config.height);
        self.depth_view = self.depth_tex.create_view(&wgpu::TextureViewDescriptor::default());
    }

    fn create_depth(device: &Device, width: u32, height: u32) -> Texture {
        device.create_texture(&TextureDescriptor {
            label: Some("depth"),
            size: Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Depth32Float,
            usage: TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
    }

    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.config.format
    }
}
