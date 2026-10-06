//! Minimal wgpu context wrapper. Holds the GLES/Metal/Vulkan state
//! plus the surface. M1 keeps this simple - no resource pooling yet.

use core::ffi::c_void;
use core::ptr::NonNull;
use raw_window_handle::{
    AndroidDisplayHandle, AndroidNdkWindowHandle, HasDisplayHandle, HasWindowHandle,
    RawDisplayHandle, RawWindowHandle,
};
use wgpu::{Adapter, Device, Instance, Queue, Surface, SurfaceConfiguration};

/// Wrapper for an Android ANativeWindow pointer so we can use it as a wgpu surface target.
struct AndroidWindow(pub NonNull<c_void>);
// SAFETY: ANativeWindow pointers are not Send by default in the bindings, but
// we only share them with the wgpu instance which knows how to keep them alive.
unsafe impl Send for AndroidWindow {}
unsafe impl Sync for AndroidWindow {}

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
}

impl Gpu {
    /// Build a surface from a raw ANativeWindow pointer.
    /// # Safety
    /// - `native_ptr` must be a valid, non-null ANativeWindow from android-activity.
    pub async fn from_android_window(native_ptr: *mut c_void) -> Result<Self, String> {
        if native_ptr.is_null() {
            return Err("null ANativeWindow".into());
        }
        let nn = unsafe { NonNull::new_unchecked(native_ptr) };
        let window = AndroidWindow(nn);

        let instance = Instance::default();
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::from_window(&window).map_err(|e| format!("surface target: {e:?}"))?)
        }.map_err(|e| e.to_string())?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .map_err(|e| format!("adapter request: {e:?}"))?;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("mc-rs"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_defaults(),
                    memory_hints: wgpu::MemoryHints::default(),
                    trace: wgpu::Trace::Off,
                },
    // no trace_path
            )
            .await
            .map_err(|e| e.to_string())?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);

        let config = SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: 1,
            height: 1,
            present_mode: caps.present_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
        };
        surface.configure(&device, &config);

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
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
    }

    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.config.format
    }
}
