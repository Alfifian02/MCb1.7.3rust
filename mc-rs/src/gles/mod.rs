//! The OpenGL ES 3.2 renderer (`renderer.rs`): a second renderer, not wgpu's GL backend. EGL + GLES are called directly (`sys.rs`), so a
//! phone whose Vulkan driver is slow can run the same frame on its (usually better tuned) GLES driver, with the same optimisations as
//! the Vulkan one: the 12-byte chunk vertex, one shared quad index buffer, chunk pages (one VAO per page), the layer order solid /
//! cut-out / water, MSAA resolved in tile memory (`EXT_multisampled_render_to_texture`, else a renderbuffer blit with the samples
//! invalidated), alpha-to-coverage, dynamic resolution, one final pass with the HUD. Written 3.0-safe: 3.2 is asked for first, then 3.1; only flat chunk draws (`flat`) need 3.1.

pub mod flat;
pub mod renderer;
pub mod shaders;
pub mod sys;
pub mod timer;

/// The HUD's vertex capacity in quads, both renderers.
pub const HUD_QUADS: usize = 6144;

static WANT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// The renderer option the running app started with (0 Auto, 1 Vulkan, 2 OpenGL ES); Video Settings shows "(restart)" when it differs.
pub fn set_want(n: u8) {
    WANT.store(n, std::sync::atomic::Ordering::Relaxed);
}

pub fn want() -> u8 {
    WANT.load(std::sync::atomic::Ordering::Relaxed)
}

use core::ffi::{c_char, c_void};
use std::cell::Cell;
use std::ffi::CString;
use std::sync::{Arc, Mutex};
use sys::*;

/// A buffer of at least this many bytes is written through the staging buffer (`Gl::write`); the per-frame uniforms and the HUD stay direct.
const STAGED_MIN: usize = 256 * 1024;

/// Buffers dropped on the render thread's side are freed at the next frame start (a `Drop` has no GL context to call).
#[derive(Default)]
pub struct DelQ(Mutex<Vec<u32>>);

/// A GL buffer object. Freed (through `DelQ`) when dropped.
pub struct Buf {
    pub id: u32,
    /// The size of its data store in bytes: `Gl::write` stages the writes into a big one.
    pub size: usize,
    q: Arc<DelQ>,
}

impl Drop for Buf {
    fn drop(&mut self) {
        if let Ok(mut v) = self.q.0.lock() {
            v.push(self.id);
        }
    }
}

/// A linked program and the locations of the uniforms asked for, in order (-1 = optimised out).
pub struct Prog {
    pub id: u32,
    pub loc: Vec<i32>,
}

pub struct Gl {
    egl: Egl,
    dpy: *mut c_void,
    surf: *mut c_void,
    ctx: *mut c_void,
    f: Fns,
    current: Cell<bool>,
    pub version: (i32, i32),
    pub name: String,
    pub max_samples: i32,
    pub ms_rtt: Option<(FbTexMs, RbMs)>,
    /// ES 3.1's compute entry points; `None` on a 3.0 context.
    pub compute: Option<Compute>,
    /// Flat chunk draws (`flat`): 0 off (not asked for, or the context cannot), else `flatDraws` of options.txt. Read by `Pages::put`, which
    /// gives every mesh whole blocks and a block table only when this is on.
    pub flat: u8,
    /// `glGetQueryObjectui64vEXT` when the context has `GL_EXT_disjoint_timer_query` (`timer::GpuTimer`); `None` without it.
    pub query_fn: Option<QueryU64>,
    /// The staging buffer `write` copies big writes from.
    stage: Uint,
    size: Cell<(u32, u32)>,
    pub dq: Arc<DelQ>,
}

// SAFETY: the context is used by one thread at a time (made current by `ensure`); the pointers are EGL handles.
unsafe impl Send for Gl {}

fn cstr(p: *const u8) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: glGetString returns a NUL-terminated string.
    unsafe { std::ffi::CStr::from_ptr(p as *const c_char) }.to_string_lossy().into_owned()
}

impl Gl {
    /// EGL display, an ES 3.2 context (then 3.1, then 3.0, the least there is; flat chunk draws need 3.1's compute) and a window surface on `window` (an `ANativeWindow*`); the context is current
    /// on the calling thread.
    pub fn new(window: *mut c_void) -> Result<Self, String> {
        let egl = Egl::open()?;
        // SAFETY: EGL calls with valid arguments; every handle is checked.
        unsafe {
            let dpy = (egl.get_display)(std::ptr::null_mut());
            if dpy.is_null() || (egl.initialize)(dpy, std::ptr::null_mut(), std::ptr::null_mut()) == 0 {
                return Err("eglInitialize failed".into());
            }
            (egl.bind_api)(EGL_OPENGL_ES_API);
            let attrs = [EGL_SURFACE_TYPE, EGL_WINDOW_BIT, EGL_RENDERABLE_TYPE, EGL_ES3_BIT, EGL_RED_SIZE, 8, EGL_GREEN_SIZE, 8, EGL_BLUE_SIZE, 8, EGL_ALPHA_SIZE, 0, EGL_NONE];
            let (mut cfg, mut n) = (std::ptr::null_mut(), 0);
            if (egl.choose_config)(dpy, attrs.as_ptr(), &mut cfg, 1, &mut n) == 0 || n < 1 {
                return Err("no ES3 window config".into());
            }
            let mut ctx = std::ptr::null_mut();
            let mut version = (3, 2);
            for minor in [2, 1, 0] {
                let a = [EGL_CTX_MAJOR, 3, EGL_CTX_MINOR, minor, EGL_NONE];
                ctx = (egl.create_context)(dpy, cfg, std::ptr::null_mut(), a.as_ptr());
                if !ctx.is_null() {
                    version = (3, minor);
                    break;
                }
            }
            if ctx.is_null() {
                return Err(format!("eglCreateContext: {:#x}", (egl.get_error)()));
            }
            let surf = (egl.create_window_surface)(dpy, cfg, window, [EGL_NONE].as_ptr());
            if surf.is_null() {
                return Err(format!("eglCreateWindowSurface: {:#x}", (egl.get_error)()));
            }
            if (egl.make_current)(dpy, surf, surf, ctx) == 0 {
                return Err(format!("eglMakeCurrent: {:#x}", (egl.get_error)()));
            }
            (egl.swap_interval)(dpy, 1);
            let get = |n: &str| (egl.get_proc)(n.as_ptr() as *const c_char);
            let f = Fns::load(&get)?;
            let (name, glver) = (cstr((f.GetString)(RENDERER)), cstr((f.GetString)(VERSION)));
            let mut n_ext = 0;
            (f.GetIntegerv)(NUM_EXTENSIONS, &mut n_ext);
            let has = |e: &str| (0..n_ext as u32).any(|i| cstr((f.GetStringi)(EXTENSIONS, i)) == e);
            let ms_rtt = if has("GL_EXT_multisampled_render_to_texture") {
                let (a, b) = (get("glFramebufferTexture2DMultisampleEXT\0"), get("glRenderbufferStorageMultisampleEXT\0"));
                (!a.is_null() && !b.is_null()).then(|| (std::mem::transmute::<*const c_void, FbTexMs>(a), std::mem::transmute::<*const c_void, RbMs>(b)))
            } else {
                None
            };
            let mut max_samples = 1;
            (f.GetIntegerv)(MAX_SAMPLES, &mut max_samples);
            let compute = if version >= (3, 1) {
                let (a, b) = (get("glDispatchCompute\0"), get("glMemoryBarrier\0"));
                (!a.is_null() && !b.is_null()).then(|| Compute {
                    dispatch: std::mem::transmute::<*const c_void, unsafe extern "C" fn(Uint, Uint, Uint)>(a),
                    barrier: std::mem::transmute::<*const c_void, unsafe extern "C" fn(u32)>(b),
                })
            } else {
                None
            };
            // A page's block table is one uniform block of 16 bytes a block (`flat::BLOCK_TABLE_BYTES`); ES 3.0 only promises 16 KiB.
            let mut max_ubo = 0;
            (f.GetIntegerv)(MAX_UNIFORM_BLOCK_SIZE, &mut max_ubo);
            let want = crate::gpu::flat::want();
            let flat = if want == 0 {
                0
            } else if compute.is_some() && max_ubo as u32 >= self::flat::BLOCK_TABLE_BYTES {
                want
            } else {
                log::info!("flat chunk draws: asked for ({want}), this GL context cannot (ES {}.{}, compute {}, uniform block {max_ubo} bytes), off", version.0, version.1, compute.is_some());
                0
            };
            // The scene pass's GPU time (`timer`): the extension's 64-bit read; the rest of the query API is core in ES 3.0.
            let query_fn = if has("GL_EXT_disjoint_timer_query") {
                let p = get("glGetQueryObjectui64vEXT\0");
                (!p.is_null()).then(|| std::mem::transmute::<*const c_void, QueryU64>(p))
            } else {
                None
            };
            let mut stage = 0;
            (f.GenBuffers)(1, &mut stage);
            log::info!("GLES init: {name} | {glver} | context {}.{}, max samples {max_samples}, tile-memory MSAA {}, GPU timer {}", version.0, version.1, ms_rtt.is_some(), query_fn.is_some());
            let gl = Self { egl, dpy, surf, ctx, f, current: Cell::new(true), version, name, max_samples, ms_rtt, compute, flat, query_fn, stage, size: Cell::new((0, 0)), dq: Arc::new(DelQ::default()) };
            gl.size.set(gl.query_size());
            Ok(gl)
        }
    }

    /// Makes the context current on the calling thread if it is not (the app is built on an init thread and drawn on the main one).
    fn ensure(&self) {
        if !self.current.get() {
            // SAFETY: handles from `new`; the previous thread released the context (`release`).
            unsafe { (self.egl.make_current)(self.dpy, self.surf, self.surf, self.ctx) };
            self.current.set(true);
        }
    }

    /// Lets go of the context so another thread can take it (the end of the init thread's work).
    pub fn release(&self) {
        if self.current.get() {
            // SAFETY: releasing the context this thread holds.
            unsafe { (self.egl.make_current)(self.dpy, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) };
            self.current.set(false);
        }
    }

    /// The GL entry points, with the context current on this thread.
    pub fn f(&self) -> &Fns {
        self.ensure();
        &self.f
    }

    fn query_size(&self) -> (u32, u32) {
        let (mut w, mut h) = (0, 0);
        // SAFETY: queries of the surface this struct owns.
        unsafe {
            (self.egl.query_surface)(self.dpy, self.surf, EGL_WIDTH, &mut w);
            (self.egl.query_surface)(self.dpy, self.surf, EGL_HEIGHT, &mut h);
        }
        (w.max(1) as u32, h.max(1) as u32)
    }

    /// The window surface's size now (it follows rotation).
    pub fn surface_size(&self) -> (u32, u32) {
        self.query_size()
    }

    /// Present; blocks on vsync (swap interval 1), which is what dynamic resolution reads as the GPU wait.
    pub fn swap(&self) {
        self.ensure();
        // SAFETY: the surface and display of this context.
        unsafe { (self.egl.swap_buffers)(self.dpy, self.surf) };
    }

    /// Frees what `Buf::drop` queued; returns the ids (the renderer drops the VAOs that point at them).
    pub fn flush_deletes(&self) -> Vec<u32> {
        let ids = self.dq.0.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default();
        if !ids.is_empty() {
            // SAFETY: names made by `GenBuffers` on this context.
            unsafe { (self.f().DeleteBuffers)(ids.len() as i32, ids.as_ptr()) };
        }
        ids
    }

    /// A buffer of `size` bytes holding `data` (or uninitialised when `data` is empty) for `usage`. Uploads go through
    /// `COPY_WRITE_BUFFER`, which is not part of any VAO's state.
    pub fn buffer(&self, size: usize, data: &[u8], usage: Enum) -> Buf {
        let f = self.f();
        let mut id = 0;
        // SAFETY: a fresh name, then a data store of `size` bytes initialised from `data` when it is that long.
        unsafe {
            (f.GenBuffers)(1, &mut id);
            (f.BindBuffer)(COPY_WRITE_BUFFER, id);
            let p = if data.len() >= size && size > 0 { data.as_ptr() as *const c_void } else { std::ptr::null() };
            (f.BufferData)(COPY_WRITE_BUFFER, size as isize, p, usage);
        }
        Buf { id, size, q: self.dq.clone() }
    }

    /// Overwrites `bytes` at `offset`. A write into a big buffer (a chunk mesh into an arena page: up to 12 MiB, read by every frame's draws)
    /// goes through a staging buffer and `glCopyBufferSubData`. `glBufferSubData` writes the store from the CPU at once, so when draws still
    /// queued on the GPU read that buffer the driver has to wait for them or clone the whole store first (Adreno and Mali both do one of
    /// the two, "buffer ghosting"): a hitch of a few ms on every frame that uploads a mesh. The copy is a command in the GPU's own queue,
    /// ordered after the draws that came before it, so nothing waits and nothing is cloned (the wgpu path's `write_buffer` does the same).
    pub fn write(&self, b: &Buf, offset: u64, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let f = self.f();
        // SAFETY: the caller keeps `offset + len` inside the buffer's store; the staging store is made as long as `bytes`.
        unsafe {
            if b.size >= STAGED_MIN {
                (f.BindBuffer)(COPY_READ_BUFFER, self.stage);
                (f.BufferData)(COPY_READ_BUFFER, bytes.len() as isize, std::ptr::null(), STREAM_DRAW);
                (f.BufferSubData)(COPY_READ_BUFFER, 0, bytes.len() as isize, bytes.as_ptr() as *const c_void);
                (f.BindBuffer)(COPY_WRITE_BUFFER, b.id);
                (f.CopyBufferSubData)(COPY_READ_BUFFER, COPY_WRITE_BUFFER, 0, offset as isize, bytes.len() as isize);
            } else {
                (f.BindBuffer)(COPY_WRITE_BUFFER, b.id);
                (f.BufferSubData)(COPY_WRITE_BUFFER, offset as isize, bytes.len() as isize, bytes.as_ptr() as *const c_void);
            }
        }
    }

    /// Streams a whole buffer: the store is orphaned (so the driver never waits for the GPU's last read of it) and refilled.
    pub fn stream(&self, b: &Buf, size: usize, bytes: &[u8]) {
        let f = self.f();
        // SAFETY: a new data store of `size` bytes, then `bytes` (<= size) copied into it.
        unsafe {
            (f.BindBuffer)(COPY_WRITE_BUFFER, b.id);
            (f.BufferData)(COPY_WRITE_BUFFER, size as isize, std::ptr::null(), STREAM_DRAW);
            if !bytes.is_empty() {
                (f.BufferSubData)(COPY_WRITE_BUFFER, 0, bytes.len() as isize, bytes.as_ptr() as *const c_void);
            }
        }
    }

    /// Compiles and links one compute shader (ES 3.1).
    pub fn compute_program(&self, src: &str, uniforms: &[&str]) -> Result<Prog, String> {
        let f = self.f();
        // SAFETY: a shader object made, checked and deleted here; the string is a NUL-terminated copy.
        unsafe {
            let s = (f.CreateShader)(COMPUTE_SHADER);
            let c = CString::new(src).map_err(|e| e.to_string())?;
            (f.ShaderSource)(s, 1, &c.as_ptr(), std::ptr::null());
            (f.CompileShader)(s);
            let mut ok = 0;
            (f.GetShaderiv)(s, COMPILE_STATUS, &mut ok);
            if ok == 0 {
                let mut buf = vec![0u8; 2048];
                (f.GetShaderInfoLog)(s, 2048, std::ptr::null_mut(), buf.as_mut_ptr() as *mut c_char);
                return Err(format!("compute shader: {}", String::from_utf8_lossy(&buf).trim_end_matches('\0')));
            }
            let p = (f.CreateProgram)();
            (f.AttachShader)(p, s);
            (f.LinkProgram)(p);
            (f.DeleteShader)(s);
            (f.GetProgramiv)(p, LINK_STATUS, &mut ok);
            if ok == 0 {
                let mut buf = vec![0u8; 2048];
                (f.GetProgramInfoLog)(p, 2048, std::ptr::null_mut(), buf.as_mut_ptr() as *mut c_char);
                return Err(format!("compute link: {}", String::from_utf8_lossy(&buf).trim_end_matches('\0')));
            }
            let loc = uniforms.iter().map(|n| CString::new(*n).map_or(-1, |c| (f.GetUniformLocation)(p, c.as_ptr()))).collect();
            Ok(Prog { id: p, loc })
        }
    }

    /// Compiles and links `vs` + `fs`; `uniforms` are looked up by name (in that order). The blocks `U` and `S` are bound to points 0 and 1.
    pub fn program(&self, vs: &str, fs: &str, uniforms: &[&str]) -> Result<Prog, String> {
        let f = self.f();
        // SAFETY: shader objects are made, checked and deleted here; strings are NUL-terminated copies.
        unsafe {
            let compile = |kind: Enum, src: &str| -> Result<u32, String> {
                let s = (f.CreateShader)(kind);
                let c = CString::new(src).map_err(|e| e.to_string())?;
                (f.ShaderSource)(s, 1, &c.as_ptr(), std::ptr::null());
                (f.CompileShader)(s);
                let mut ok = 0;
                (f.GetShaderiv)(s, COMPILE_STATUS, &mut ok);
                if ok == 0 {
                    let mut buf = vec![0u8; 2048];
                    (f.GetShaderInfoLog)(s, 2048, std::ptr::null_mut(), buf.as_mut_ptr() as *mut c_char);
                    return Err(format!("shader: {}", String::from_utf8_lossy(&buf).trim_end_matches('\0')));
                }
                Ok(s)
            };
            let (v, fr) = (compile(VERTEX_SHADER, vs)?, compile(FRAGMENT_SHADER, fs)?);
            let p = (f.CreateProgram)();
            (f.AttachShader)(p, v);
            (f.AttachShader)(p, fr);
            (f.LinkProgram)(p);
            (f.DeleteShader)(v);
            (f.DeleteShader)(fr);
            let mut ok = 0;
            (f.GetProgramiv)(p, LINK_STATUS, &mut ok);
            if ok == 0 {
                let mut buf = vec![0u8; 2048];
                (f.GetProgramInfoLog)(p, 2048, std::ptr::null_mut(), buf.as_mut_ptr() as *mut c_char);
                return Err(format!("link: {}", String::from_utf8_lossy(&buf).trim_end_matches('\0')));
            }
            // `B`: the block table of a page (flat chunk draws), bound by the draw loop at point 2.
            for (name, point) in [(c"U", 0), (c"S", 1), (c"B", 2)] {
                let i = (f.GetUniformBlockIndex)(p, name.as_ptr());
                if i != u32::MAX {
                    (f.UniformBlockBinding)(p, i, point);
                }
            }
            let loc = uniforms.iter().map(|n| CString::new(*n).map_or(-1, |c| (f.GetUniformLocation)(p, c.as_ptr()))).collect();
            Ok(Prog { id: p, loc })
        }
    }
}

impl Drop for Gl {
    fn drop(&mut self) {
        // SAFETY: tearing down what `new` made; the context must be current to be destroyed cleanly.
        unsafe {
            (self.egl.make_current)(self.dpy, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
            (self.egl.destroy_surface)(self.dpy, self.surf);
            (self.egl.destroy_context)(self.dpy, self.ctx);
        }
    }
}
