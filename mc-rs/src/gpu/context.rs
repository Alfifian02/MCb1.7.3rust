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
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
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
/// This is where it starts (and stays while Dynamic Res is off); `Drs` moves it.
pub const START_PCT: u32 = 67;
#[cfg(test)]
pub const RENDER_SCALE: f32 = START_PCT as f32 / 100.0;
pub const SCALE_MIN: u32 = 50;
pub const SCALE_MAX: u32 = 100;
static SCALE_PCT: AtomicU32 = AtomicU32::new(START_PCT);

/// Frames the swap chain may queue ahead (`SurfaceConfiguration::desired_maximum_frame_latency`). 2 is wgpu's default and what the
/// game always used; 1 shortens the finger-to-picture path by up to one frame but lets the CPU and the GPU overlap less, so a frame
/// that needs more than about half the refresh period may drop to the next step down. `frameLatency:1` in `options.txt`; read once,
/// before the surface is configured. The OpenGL ES renderer ignores it (EGL owns its queue).
static FRAME_LATENCY: AtomicU32 = AtomicU32::new(2);

pub fn set_frame_latency(n: u32) {
    FRAME_LATENCY.store(n.clamp(1, 3), Ordering::Relaxed);
}

pub fn set_scale_pct(p: u32) {
    SCALE_PCT.store(p.clamp(SCALE_MIN, SCALE_MAX), Ordering::Relaxed);
}

pub fn scale_pct() -> u32 {
    SCALE_PCT.load(Ordering::Relaxed)
}

/// The scene size for a surface of `w` x `h` at the current scale.
pub fn scene_size(w: u32, h: u32) -> (u32, u32) {
    let s = scale_pct() as f32 / 100.0;
    (((w as f32 * s).round() as u32).max(1), ((h as f32 * s).round() as u32).max(1))
}

const STEP: u32 = 5;
const WINDOW: u32 = 30; // frames averaged: half a second at 60 Hz
/// The target frame rate is the panel's refresh rate (`Drs::set_refresh_hz`, from the display) clamped to `60..=MAX_TARGET_HZ`: chasing
/// 120 Hz would sink the scene to `SCALE_MIN` on any phone that cannot draw 120 fps, and a blurry 120 is worse than a clean 60-90.
/// ponytail: one number for the cap; the upgrade is to follow the `Performance` option too.
const MIN_TARGET_HZ: f32 = 60.0;
const MAX_TARGET_HZ: f32 = 90.0;
const BASE_WAIT: u32 = 20; // clean windows before a probe up (10 s at 60 fps)
const MAX_WAIT: u32 = 240;
const HOLD: u32 = 3; // clean windows after a probe up that show the new level holds
// With the scene pass's GPU time (`gpu::timing`) there is no guessing from frame time and CPU idle: the budgets are shares of the target frame time (`Drs::target`, the panel's rate).
const GPU_DOWN: f32 = 0.85; // a window averaging over this share steps down (the final pass and the driver need the rest of the frame)
const GPU_UP: f32 = 0.70; // a probe up only when the scene pass is predicted to stay under this share at the next level
const GPU_WAIT: u32 = 4; // windows with that headroom before the probe (2 s at 60 fps)

/// Dynamic resolution (roadmap performance step 4): moves the scene scale by `STEP` percent between `SCALE_MIN` and `SCALE_MAX`.
/// vsync hides spare GPU time (a frame is 16.7 ms either way), so there is no "fast" to read: it steps DOWN when a window of frames
/// averages over its target frame time (`MIN_TARGET_HZ`..`MAX_TARGET_HZ`, the panel's rate) by 20% while the CPU sat idle for a good part of it (GPU-bound; fewer pixels do nothing for a CPU-bound frame, which
/// also never counts as clean), and probes UP after `wait` clean windows. When the adapter has timestamp queries, a window is judged on the scene
/// pass's GPU time instead (`gpu_window`). A drop right after a probe means the level above does not hold: `wait`
/// doubles; a probe that holds halves it again.
pub struct Drs {
    pct: u32,
    n: u32,
    total: f32,
    cpu: f32,
    gpu: f32,
    timed: u32,
    clean: u32,
    wait: u32,
    up: bool,
    /// Seconds a frame may take before it counts as slow (before the 20% margin).
    target: f32,
}

impl Drs {
    pub fn new(pct: u32) -> Self {
        Self::at(pct, MIN_TARGET_HZ)
    }

    /// Starts at `pct` aiming at the display's `hz`.
    pub fn at(pct: u32, hz: f32) -> Self {
        Self { pct, n: 0, total: 0.0, cpu: 0.0, gpu: 0.0, timed: 0, clean: 0, wait: BASE_WAIT, up: false, target: target_secs(hz) }
    }

    /// The display's refresh rate became known (or changed: a 60 / 90 / 120 Hz switch); the scale and the windows in progress stay.
    pub fn set_refresh_hz(&mut self, hz: f32) {
        self.target = target_secs(hz);
    }

    /// One frame: `total` seconds in `render_frame` (swap-chain wait included), `cpu` of them CPU work, `gpu` the scene pass's GPU seconds
    /// when a timestamp arrived this frame (`None`: not measured). A window in which at least half the frames were timed is judged
    /// on the GPU time alone (`gpu_window`); otherwise on `total` and `cpu` as before. `Some(percent)` when the scale changes.
    pub fn tick(&mut self, total: f32, cpu: f32, gpu: Option<f32>) -> Option<u32> {
        self.total += total.min(0.05); // a load hitch counts as at most 3 frames
        self.cpu += cpu.min(0.05);
        if let Some(g) = gpu {
            self.gpu += g.min(0.05);
            self.timed += 1;
        }
        self.n += 1;
        if self.n < WINDOW {
            return None;
        }
        let (total, cpu) = (self.total / self.n as f32, self.cpu / self.n as f32);
        let measured = (self.timed * 2 >= self.n).then(|| self.gpu / self.timed as f32);
        (self.n, self.total, self.cpu, self.gpu, self.timed) = (0, 0.0, 0.0, 0.0, 0);
        if let Some(g) = measured {
            return self.gpu_window(g);
        }
        let old = self.pct;
        let slow = total > self.target * 1.2;
        if slow && cpu < total * 0.75 {
            if self.up && self.clean < HOLD {
                self.wait = (self.wait * 2).min(MAX_WAIT);
            }
            (self.up, self.clean) = (false, 0);
            self.pct = self.pct.saturating_sub(STEP).max(SCALE_MIN);
        } else if slow {
            self.clean = 0;
        } else {
            self.clean += 1;
            if self.up && self.clean == HOLD {
                (self.up, self.wait) = (false, (self.wait / 2).max(BASE_WAIT));
            }
            if self.clean >= self.wait && self.pct < SCALE_MAX {
                (self.pct, self.up, self.clean) = (self.pct + STEP, true, 0);
            }
        }
        (self.pct != old).then_some(self.pct)
    }

    /// One window judged on the scene pass's average GPU time `g` (seconds). Over budget: down one step, whatever the CPU did.
    /// Under it: a probe up only after `GPU_WAIT` windows in which the next level is predicted to fit (the pixel count grows with the
    /// square of the scale; vertex work does not, so this overestimates the cost and errs on the safe side). The gap between
    /// `GPU_DOWN` and `GPU_UP` keeps it from bouncing between two levels.
    fn gpu_window(&mut self, g: f32) -> Option<u32> {
        let old = self.pct;
        (self.up, self.wait) = (false, BASE_WAIT); // the untimed path starts fresh if the timestamps stop
        if g > self.target * GPU_DOWN {
            self.clean = 0;
            self.pct = self.pct.saturating_sub(STEP).max(SCALE_MIN);
        } else {
            let next = ((self.pct + STEP).min(SCALE_MAX)) as f32 / self.pct as f32;
            if self.pct < SCALE_MAX && g * next * next < self.target * GPU_UP {
                self.clean += 1;
                if self.clean >= GPU_WAIT {
                    (self.pct, self.clean) = ((self.pct + STEP).min(SCALE_MAX), 0);
                }
            } else {
                self.clean = 0;
            }
        }
        (self.pct != old).then_some(self.pct)
    }
}

/// The frame time `Drs` aims at for a panel of `hz` (NaN, zero and nonsense fall back to 60 Hz).
fn target_secs(hz: f32) -> f32 {
    1.0 / if hz.is_finite() { hz.clamp(MIN_TARGET_HZ, MAX_TARGET_HZ) } else { MIN_TARGET_HZ }
}

/// The gaps between presented frames of one second, for the 99th percentile (`render_frame` logs it and the debug screen shows it): the
/// average FPS hides the hitches a person feels, the worst frame is one bad luck, p99 is the slowest of every hundred.
#[derive(Default)]
pub struct FrameTimes(Vec<f32>);

impl FrameTimes {
    pub fn push(&mut self, secs: f32) {
        if self.0.len() < 1024 {
            self.0.push(secs); // ponytail: 1024 gaps is 8 s at 120 Hz; the second is read long before
        }
    }

    /// The 99th percentile (nearest rank) of what was pushed since the last call, in seconds; 0 when nothing was. Clears.
    pub fn take_p99(&mut self) -> f32 {
        self.0.sort_by(f32::total_cmp);
        let p = self.0.len().checked_sub(1).map_or(0.0, |last| self.0[(self.0.len() * 99).div_ceil(100).saturating_sub(1).min(last)]);
        self.0.clear();
        p
    }
}

#[cfg(test)]
mod pacing_tests {
    use super::*;

    #[test]
    fn p99_is_the_hundredth_slowest_and_the_target_follows_the_panel() {
        let mut t = FrameTimes::default();
        assert_eq!(t.take_p99(), 0.0, "nothing pushed");
        for _ in 0..99 {
            t.push(0.0167);
        }
        t.push(0.1); // one hitch in a hundred frames is the worst frame, not yet p99
        assert_eq!(t.take_p99(), 0.0167);
        for _ in 0..98 {
            t.push(0.0167);
        }
        (t.push(0.1), t.push(0.1)); // two in a hundred: the 99th is a hitch
        assert_eq!(t.take_p99(), 0.1);
        assert_eq!(t.take_p99(), 0.0, "cleared");
        assert_eq!((target_secs(60.0), target_secs(f32::NAN), target_secs(0.0)), (1.0 / 60.0, 1.0 / 60.0, 1.0 / 60.0));
        assert_eq!((target_secs(90.0), target_secs(120.0)), (1.0 / 90.0, 1.0 / 90.0), "capped");
        let run = |d: &mut Drs, windows: u32, total: f32| (0..windows * WINDOW).filter_map(|_| d.tick(total, 0.004, None)).last();
        // 90 Hz panel, GPU-bound at 60 fps: the scene steps down (on a 60 Hz target this frame time is on target).
        assert_eq!(run(&mut Drs::at(START_PCT, 90.0), 1, 0.0167), Some(START_PCT - STEP));
        assert_eq!(run(&mut Drs::at(START_PCT, 60.0), 1, 0.0167), None);
        let mut d = Drs::new(START_PCT);
        d.set_refresh_hz(90.0);
        assert_eq!(run(&mut d, 1, 0.0167), Some(START_PCT - STEP));
        // The GPU-time path aims at the same target: 12 ms is under 85% of 16.7 ms but over 85% of 11.1 ms.
        let timed = |d: &mut Drs| (0..WINDOW).filter_map(|_| d.tick(0.0167, 0.006, Some(0.012))).last();
        assert_eq!(timed(&mut Drs::at(START_PCT, 60.0)), None);
        assert_eq!(timed(&mut Drs::at(START_PCT, 90.0)), Some(START_PCT - STEP));
    }
}

#[cfg(test)]
mod drs_tests {
    use super::*;

    #[test]
    fn steps_down_when_gpu_bound_and_probes_up_with_backoff() {
        assert_eq!((RENDER_SCALE * 100.0).round() as u32, START_PCT);
        let run = |d: &mut Drs, windows: u32, total: f32, cpu: f32| (0..windows * WINDOW).filter_map(|_| d.tick(total, cpu, None)).last();
        let mut d = Drs::new(START_PCT);
        // vsync-locked 60 Hz with the CPU mostly idle: nothing moves until the probe.
        assert_eq!(run(&mut d, BASE_WAIT - 1, 0.0167, 0.006), None);
        assert_eq!(run(&mut d, 1, 0.0167, 0.006), Some(START_PCT + STEP));
        // The probe fails at once (46 fps, GPU-bound): back down, and the next probe waits twice as long.
        assert_eq!(run(&mut d, 1, 0.022, 0.008), Some(START_PCT));
        assert_eq!(run(&mut d, 2 * BASE_WAIT - 1, 0.0167, 0.006), None);
        assert_eq!(run(&mut d, 1, 0.0167, 0.006), Some(START_PCT + STEP));
        // CPU-bound slow frames neither lower the resolution nor count as clean.
        let mut c = Drs::new(START_PCT);
        assert_eq!(run(&mut c, 3 * BASE_WAIT, 0.03, 0.028), None);
        // The floor holds.
        assert_eq!(run(&mut Drs::new(SCALE_MIN), 4, 0.03, 0.008), None);
    }

    #[test]
    fn gpu_time_steps_down_over_budget_and_probes_up_only_with_headroom() {
        // Vsync-clean frame times and an idle CPU throughout: only the GPU time speaks.
        let run = |d: &mut Drs, windows: u32, gpu: f32| (0..windows * WINDOW).filter_map(|_| d.tick(0.0167, 0.006, Some(gpu))).last();
        let mut d = Drs::new(START_PCT);
        // 15 ms of 16.7 is over 85%: down at the first window (the old path would have waited for a missed vsync).
        assert_eq!(run(&mut d, 1, 0.015), Some(START_PCT - STEP));
        // 12 ms fits, but 5% more scale is predicted at ~14 ms: no headroom, so it holds however long it runs.
        assert_eq!(run(&mut d, 10, 0.012), None);
        // 8 ms has headroom (predicted ~9.3 ms): the probe comes after GPU_WAIT windows, not before.
        assert_eq!(run(&mut d, GPU_WAIT - 1, 0.008), None);
        assert_eq!(run(&mut d, 1, 0.008), Some(START_PCT));
        // A window with too few timed frames (here 10 of 30) falls back to the frame-time path.
        let mut e = Drs::new(START_PCT);
        let sparse = (0..WINDOW).filter_map(|i| e.tick(0.022, 0.008, (i % 3 == 0).then_some(0.001))).last();
        assert_eq!(sparse, Some(START_PCT - STEP), "GPU-bound by frame time, as before");
        // The floor and the ceiling hold.
        assert_eq!(run(&mut Drs::new(SCALE_MIN), 4, 0.03), None);
        assert_eq!(run(&mut Drs::new(SCALE_MAX), 2 * GPU_WAIT, 0.002), None);
    }
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
    /// Never read: held so the `Instance` lives as long as the `Surface` and the `Device` made from it.
    #[allow(dead_code)]
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
        let (fxaa, fast) = crate::render::present::pick_final(&adapter, samples);
        crate::render::present::set_fast_post(fast);
        crate::render::present::set_fxaa(fxaa);
        log::info!("M3 init: scene MSAA x{samples}, final pass {}, FXAA {fxaa}", if fast { "fast (no FXAA/CAS)" } else { "full" });

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("mc-rs"),
                required_features: crate::gpu::timing::GpuTimer::features(&adapter), // scene-pass timestamps for `Drs`, if the adapter has them
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
        config.desired_maximum_frame_latency = FRAME_LATENCY.load(Ordering::Relaxed);
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
        self.rescale();
    }

    /// The depth at the current `scene_size` (dynamic resolution: the surface stays as it is).
    pub fn rescale(&mut self) {
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
