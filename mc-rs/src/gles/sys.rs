//! Raw EGL + OpenGL ES 3 entry points, loaded at run time (`dlopen` of libEGL, then `eglGetProcAddress`): no link-time dependency, no
//! new crate, and the crate still links on a host without GLES. Only what the renderer calls is listed.

use core::ffi::{c_char, c_int, c_void};

pub type Enum = u32;
pub type Uint = u32;
pub type Int = i32;
pub type Sizei = i32;

extern "C" {
    fn dlopen(name: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
}

pub const COLOR_BUFFER_BIT: u32 = 0x4000;
pub const DEPTH_BUFFER_BIT: u32 = 0x100;
pub const TRIANGLES: Enum = 4;
pub const ARRAY_BUFFER: Enum = 0x8892;
pub const ELEMENT_ARRAY_BUFFER: Enum = 0x8893;
pub const UNIFORM_BUFFER: Enum = 0x8A11;
pub const STREAM_DRAW: Enum = 0x88E0;
pub const DYNAMIC_DRAW: Enum = 0x88E8;
pub const STATIC_DRAW: Enum = 0x88E4;
pub const FLOAT: Enum = 0x1406;
pub const UNSIGNED_BYTE: Enum = 0x1401;
pub const UNSIGNED_SHORT: Enum = 0x1403;
pub const UNSIGNED_INT: Enum = 0x1405;
pub const TEXTURE_2D: Enum = 0x0DE1;
pub const TEXTURE_2D_ARRAY: Enum = 0x8C1A;
pub const TEXTURE0: Enum = 0x84C0;
pub const TEXTURE_MIN_FILTER: Enum = 0x2801;
pub const TEXTURE_MAG_FILTER: Enum = 0x2800;
pub const TEXTURE_WRAP_S: Enum = 0x2802;
pub const TEXTURE_WRAP_T: Enum = 0x2803;
pub const NEAREST: Int = 0x2600;
pub const LINEAR: Int = 0x2601;
pub const LINEAR_MIPMAP_LINEAR: Int = 0x2703;
pub const REPEAT: Int = 0x2901;
pub const CLAMP_TO_EDGE: Int = 0x812F;
pub const RGBA: Enum = 0x1908;
pub const RGBA8: Enum = 0x8058;
pub const SRGB8_ALPHA8: Enum = 0x8C43;
pub const DEPTH_COMPONENT32F: Enum = 0x8CAC;
pub const FRAMEBUFFER: Enum = 0x8D40;
pub const READ_FRAMEBUFFER: Enum = 0x8CA8;
pub const DRAW_FRAMEBUFFER: Enum = 0x8CA9;
pub const RENDERBUFFER: Enum = 0x8D41;
pub const COLOR_ATTACHMENT0: Enum = 0x8CE0;
pub const DEPTH_ATTACHMENT: Enum = 0x8D00;
pub const FRAMEBUFFER_COMPLETE: Enum = 0x8CD5;
pub const NO_ERROR: Enum = 0;
pub const VERTEX_SHADER: Enum = 0x8B31;
pub const FRAGMENT_SHADER: Enum = 0x8B30;
pub const COMPILE_STATUS: Enum = 0x8B81;
pub const LINK_STATUS: Enum = 0x8B82;
pub const BLEND: Enum = 0x0BE2;
pub const CULL_FACE: Enum = 0x0B44;
pub const DEPTH_TEST: Enum = 0x0B71;
pub const POLYGON_OFFSET_FILL: Enum = 0x8037;
pub const SAMPLE_ALPHA_TO_COVERAGE: Enum = 0x809E;
pub const BACK: Enum = 0x0405;
pub const LESS: Enum = 0x0201;
pub const LEQUAL: Enum = 0x0203;
pub const SRC_ALPHA: Enum = 0x0302;
pub const ONE_MINUS_SRC_ALPHA: Enum = 0x0303;
pub const ONE: Enum = 1;
pub const ZERO: Enum = 0;
pub const VERSION: Enum = 0x1F02;
pub const RENDERER: Enum = 0x1F01;
pub const EXTENSIONS: Enum = 0x1F03;
pub const NUM_EXTENSIONS: Enum = 0x821D;
pub const MAX_SAMPLES: Enum = 0x8D57;
pub const FALSE: u8 = 0;
pub const TRUE: u8 = 1;

macro_rules! gl_api {
    ($( $name:ident ( $($ty:ty),* ) $(-> $ret:ty)? ; )*) => {
        #[allow(non_snake_case)]
        #[derive(Clone, Copy)]
        pub struct Fns { $( pub $name: unsafe extern "C" fn($($ty),*) $(-> $ret)?, )* }
        impl Fns {
            /// # Safety
            /// `get` must return the entry point of the named function of the current context (or null).
            pub unsafe fn load(get: &dyn Fn(&str) -> *const c_void) -> Result<Self, String> {
                Ok(Self { $( $name: {
                    let p = get(concat!("gl", stringify!($name), "\0"));
                    if p.is_null() { return Err(concat!("missing gl", stringify!($name)).to_string()); }
                    std::mem::transmute::<*const c_void, unsafe extern "C" fn($($ty),*) $(-> $ret)?>(p)
                }, )* })
            }
        }
    };
}

gl_api! {
    Clear(u32);
    ClearColor(f32, f32, f32, f32);
    ClearDepthf(f32);
    Enable(Enum);
    Disable(Enum);
    BlendFunc(Enum, Enum);
    BlendFuncSeparate(Enum, Enum, Enum, Enum);
    DepthFunc(Enum);
    DepthMask(u8);
    CullFace(Enum);
    Viewport(Int, Int, Sizei, Sizei);
    PolygonOffset(f32, f32);
    GenBuffers(Sizei, *mut Uint);
    DeleteBuffers(Sizei, *const Uint);
    BindBuffer(Enum, Uint);
    BindBufferBase(Enum, Uint, Uint);
    BufferData(Enum, isize, *const c_void, Enum);
    BufferSubData(Enum, isize, isize, *const c_void);
    GenVertexArrays(Sizei, *mut Uint);
    DeleteVertexArrays(Sizei, *const Uint);
    BindVertexArray(Uint);
    EnableVertexAttribArray(Uint);
    VertexAttribPointer(Uint, Int, Enum, u8, Sizei, *const c_void);
    VertexAttribIPointer(Uint, Int, Enum, Sizei, *const c_void);
    GenTextures(Sizei, *mut Uint);
    DeleteTextures(Sizei, *const Uint);
    BindTexture(Enum, Uint);
    ActiveTexture(Enum);
    TexParameteri(Enum, Enum, Int);
    TexStorage2D(Enum, Sizei, Enum, Sizei, Sizei);
    TexStorage3D(Enum, Sizei, Enum, Sizei, Sizei, Sizei);
    TexSubImage2D(Enum, Int, Int, Int, Sizei, Sizei, Enum, Enum, *const c_void);
    TexSubImage3D(Enum, Int, Int, Int, Int, Sizei, Sizei, Sizei, Enum, Enum, *const c_void);
    CreateShader(Enum) -> Uint;
    ShaderSource(Uint, Sizei, *const *const c_char, *const Int);
    CompileShader(Uint);
    GetShaderiv(Uint, Enum, *mut Int);
    GetShaderInfoLog(Uint, Sizei, *mut Sizei, *mut c_char);
    DeleteShader(Uint);
    CreateProgram() -> Uint;
    AttachShader(Uint, Uint);
    LinkProgram(Uint);
    GetProgramiv(Uint, Enum, *mut Int);
    GetProgramInfoLog(Uint, Sizei, *mut Sizei, *mut c_char);
    DeleteProgram(Uint);
    UseProgram(Uint);
    GetUniformLocation(Uint, *const c_char) -> Int;
    GetUniformBlockIndex(Uint, *const c_char) -> Uint;
    UniformBlockBinding(Uint, Uint, Uint);
    Uniform1i(Int, Int);
    Uniform2f(Int, f32, f32);
    Uniform3f(Int, f32, f32, f32);
    Uniform4f(Int, f32, f32, f32, f32);
    UniformMatrix4fv(Int, Sizei, u8, *const f32);
    GenFramebuffers(Sizei, *mut Uint);
    DeleteFramebuffers(Sizei, *const Uint);
    BindFramebuffer(Enum, Uint);
    FramebufferTexture2D(Enum, Enum, Enum, Uint, Int);
    FramebufferRenderbuffer(Enum, Enum, Enum, Uint);
    CheckFramebufferStatus(Enum) -> Enum;
    GenRenderbuffers(Sizei, *mut Uint);
    DeleteRenderbuffers(Sizei, *const Uint);
    BindRenderbuffer(Enum, Uint);
    RenderbufferStorage(Enum, Enum, Sizei, Sizei);
    RenderbufferStorageMultisample(Enum, Sizei, Enum, Sizei, Sizei);
    BlitFramebuffer(Int, Int, Int, Int, Int, Int, Int, Int, u32, Enum);
    InvalidateFramebuffer(Enum, Sizei, *const Enum);
    DrawArrays(Enum, Int, Sizei);
    DrawElements(Enum, Sizei, Enum, *const c_void);
    GetString(Enum) -> *const u8;
    GetStringi(Enum, Uint) -> *const u8;
    GetIntegerv(Enum, *mut Int);
    GetError() -> Enum;
    PixelStorei(Enum, Int);
}

/// `GL_EXT_multisampled_render_to_texture`: the samples live in tile memory and resolve when the pass ends.
pub type FbTexMs = unsafe extern "C" fn(Enum, Enum, Enum, Uint, Int, Sizei);
pub type RbMs = unsafe extern "C" fn(Enum, Sizei, Enum, Sizei, Sizei);

pub const EGL_NONE: i32 = 0x3038;
pub const EGL_SURFACE_TYPE: i32 = 0x3033;
pub const EGL_WINDOW_BIT: i32 = 4;
pub const EGL_RENDERABLE_TYPE: i32 = 0x3040;
pub const EGL_ES3_BIT: i32 = 0x40;
pub const EGL_RED_SIZE: i32 = 0x3024;
pub const EGL_GREEN_SIZE: i32 = 0x3023;
pub const EGL_BLUE_SIZE: i32 = 0x3022;
pub const EGL_ALPHA_SIZE: i32 = 0x3021;
pub const EGL_WIDTH: i32 = 0x3057;
pub const EGL_HEIGHT: i32 = 0x3056;
pub const EGL_CTX_MAJOR: i32 = 0x3098;
pub const EGL_CTX_MINOR: i32 = 0x30FB;
pub const EGL_OPENGL_ES_API: u32 = 0x30A0;

/// EGL, from libEGL.
pub struct Egl {
    pub get_display: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    pub initialize: unsafe extern "C" fn(*mut c_void, *mut i32, *mut i32) -> u32,
    pub bind_api: unsafe extern "C" fn(u32) -> u32,
    pub choose_config: unsafe extern "C" fn(*mut c_void, *const i32, *mut *mut c_void, i32, *mut i32) -> u32,
    pub create_context: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *const i32) -> *mut c_void,
    pub create_window_surface: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *const i32) -> *mut c_void,
    pub make_current: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void) -> u32,
    pub swap_buffers: unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32,
    pub swap_interval: unsafe extern "C" fn(*mut c_void, i32) -> u32,
    pub query_surface: unsafe extern "C" fn(*mut c_void, *mut c_void, i32, *mut i32) -> u32,
    pub destroy_surface: unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32,
    pub destroy_context: unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32,
    pub get_error: unsafe extern "C" fn() -> i32,
    pub get_proc: unsafe extern "C" fn(*const c_char) -> *const c_void,
}

impl Egl {
    pub fn open() -> Result<Self, String> {
        // SAFETY: plain dlopen / dlsym of libEGL's documented entry points, each cast to its documented signature.
        unsafe {
            let mut h = dlopen(c"libEGL.so".as_ptr(), 2);
            if h.is_null() {
                h = dlopen(c"libEGL.so.1".as_ptr(), 2);
            }
            if h.is_null() {
                return Err("no libEGL".into());
            }
            macro_rules! sym {
                ($n:literal) => {{
                    let p = dlsym(h, concat!($n, "\0").as_ptr() as *const c_char);
                    if p.is_null() {
                        return Err(concat!("missing ", $n).into());
                    }
                    std::mem::transmute(p)
                }};
            }
            Ok(Self {
                get_display: sym!("eglGetDisplay"),
                initialize: sym!("eglInitialize"),
                bind_api: sym!("eglBindAPI"),
                choose_config: sym!("eglChooseConfig"),
                create_context: sym!("eglCreateContext"),
                create_window_surface: sym!("eglCreateWindowSurface"),
                make_current: sym!("eglMakeCurrent"),
                swap_buffers: sym!("eglSwapBuffers"),
                swap_interval: sym!("eglSwapInterval"),
                query_surface: sym!("eglQuerySurface"),
                destroy_surface: sym!("eglDestroySurface"),
                destroy_context: sym!("eglDestroyContext"),
                get_error: sym!("eglGetError"),
                get_proc: sym!("eglGetProcAddress"),
            })
        }
    }
}

pub const COPY_WRITE_BUFFER: Enum = 0x8F37;
