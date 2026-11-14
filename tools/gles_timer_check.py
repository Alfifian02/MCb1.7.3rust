"""The two GL pieces of `Gl::write` and `gles::timer` on Mesa (EGL surfaceless, GLES 3.x), with the constants read from `sys.rs`:
1. the staged write (`BufferData` on a staging buffer, `BufferSubData`, `CopyBufferSubData`) leaves a 12 MiB buffer byte for byte as direct writes would;
2. the query ring of `GpuTimer` (`TIME_ELAPSED_EXT`, six slots, `QUERY_RESULT_AVAILABLE`, the disjoint flag) times 40 frames of a real pass without a GL error.
Run: python3 tools/gles_timer_check.py. It cannot say anything about speed (llvmpipe's timer is not a GPU's) nor about the Rust that calls the same GL.
"""
import ctypes as C, os, re, random
os.environ.setdefault("EGL_PLATFORM", "surfaceless")
sys_rs = open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "../mc-rs/src/gles/sys.rs")).read()
def const(n): return int(re.search(r"pub const %s: \w+ = (0x[0-9A-Fa-f]+|\d+);" % n, sys_rs).group(1), 0)
COPY_READ, COPY_WRITE = const("COPY_READ_BUFFER"), const("COPY_WRITE_BUFFER")
STREAM_DRAW, DYNAMIC_DRAW = const("STREAM_DRAW"), const("DYNAMIC_DRAW")
TIME_ELAPSED, QRES, QAVAIL, DISJOINT = const("TIME_ELAPSED_EXT"), const("QUERY_RESULT"), const("QUERY_RESULT_AVAILABLE"), const("GPU_DISJOINT_EXT")
print("constants from sys.rs:", hex(COPY_READ), hex(COPY_WRITE), hex(TIME_ELAPSED), hex(QRES), hex(QAVAIL), hex(DISJOINT))

egl = C.CDLL("libEGL.so.1"); gl = C.CDLL("libGLESv2.so.2")
egl.eglGetPlatformDisplay.restype = C.c_void_p; egl.eglGetPlatformDisplay.argtypes = [C.c_uint, C.c_void_p, C.c_void_p]
dpy = egl.eglGetPlatformDisplay(0x31DD, None, None); egl.eglInitialize.argtypes = [C.c_void_p] * 3
assert egl.eglInitialize(C.c_void_p(dpy), C.byref(C.c_int()), C.byref(C.c_int()))
egl.eglBindAPI(0x30A0); cfg = C.c_void_p(); n = C.c_int()
egl.eglChooseConfig.argtypes = [C.c_void_p, C.c_void_p, C.c_void_p, C.c_int, C.c_void_p]
egl.eglChooseConfig(C.c_void_p(dpy), (C.c_int * 3)(0x3040, 0x40, 0x3038), C.byref(cfg), 1, C.byref(n))
egl.eglCreateContext.restype = C.c_void_p; egl.eglCreateContext.argtypes = [C.c_void_p] * 4
ctx = egl.eglCreateContext(C.c_void_p(dpy), cfg, None, (C.c_int * 5)(0x3098, 3, 0x30FB, 0, 0x3038))
egl.eglMakeCurrent.argtypes = [C.c_void_p] * 4
assert egl.eglMakeCurrent(C.c_void_p(dpy), None, None, C.c_void_p(ctx))
gl.glGetString.restype = C.c_char_p; gl.glGetStringi.restype = C.c_char_p
print("GL:", gl.glGetString(0x1F02).decode(), "|", gl.glGetString(0x1F01).decode())
def err(where):
    e = gl.glGetError(); assert e == 0, f"GL error {hex(e)} at {where}"

# ---- 1. the staged write of Gl::write: bytes must land exactly where a direct glBufferSubData would put them ----
gl.glBufferData.argtypes = [C.c_uint, C.c_ssize_t, C.c_void_p, C.c_uint]
gl.glBufferSubData.argtypes = [C.c_uint, C.c_ssize_t, C.c_ssize_t, C.c_void_p]
gl.glCopyBufferSubData.argtypes = [C.c_uint, C.c_uint, C.c_ssize_t, C.c_ssize_t, C.c_ssize_t]
gl.glMapBufferRange.restype = C.c_void_p; gl.glMapBufferRange.argtypes = [C.c_uint, C.c_ssize_t, C.c_ssize_t, C.c_uint]
b = C.c_uint(); gl.glGenBuffers(1, C.byref(b)); dst = b.value
gl.glGenBuffers(1, C.byref(b)); stage = b.value
SIZE = 12 << 20
gl.glBindBuffer(COPY_WRITE, dst); gl.glBufferData(COPY_WRITE, SIZE, None, DYNAMIC_DRAW); err("alloc")
ref = bytearray(SIZE)
def staged(offset, data):   # the exact call sequence of Gl::write's staged branch
    buf = (C.c_ubyte * len(data)).from_buffer_copy(data)
    gl.glBindBuffer(COPY_READ, stage)
    gl.glBufferData(COPY_READ, len(data), None, STREAM_DRAW)
    gl.glBufferSubData(COPY_READ, 0, len(data), buf)
    gl.glBindBuffer(COPY_WRITE, dst)
    gl.glCopyBufferSubData(COPY_READ, COPY_WRITE, 0, offset, len(data))
    ref[offset:offset + len(data)] = data
random.seed(7)
for off, ln in [(0, 4096), (777_000, 300_000), (SIZE - 1000, 1000), (5_000_000, 1), (100, 12288), (777_000 + 150_000, 5000)]:
    staged(off, bytes(random.getrandbits(8) for _ in range(ln))); err(f"staged write {off}+{ln}")
gl.glBindBuffer(COPY_WRITE, dst)
p = gl.glMapBufferRange(COPY_WRITE, 0, SIZE, 1)  # GL_MAP_READ_BIT
got = bytes((C.c_ubyte * SIZE).from_address(p)); gl.glUnmapBuffer(COPY_WRITE); err("map")
print("staged writes: buffer matches reference ->", got == bytes(ref))
assert got == bytes(ref)

# ---- 2. the GpuTimer ring (a line for line port of gles/timer.rs) around a real pass ----
n_ext = C.c_int(); gl.glGetIntegerv(0x821D, C.byref(n_ext))
exts = [gl.glGetStringi(0x1F03, i).decode() for i in range(n_ext.value)]
has = "GL_EXT_disjoint_timer_query" in exts
print("GL_EXT_disjoint_timer_query on this driver:", has)
if has:
    egl.eglGetProcAddress.restype = C.c_void_p; egl.eglGetProcAddress.argtypes = [C.c_char_p]
    p64 = egl.eglGetProcAddress(b"glGetQueryObjectui64vEXT"); assert p64
    result64 = C.CFUNCTYPE(None, C.c_uint, C.c_uint, C.POINTER(C.c_uint64))(p64)
    SLOTS = 6
    ids = (C.c_uint * SLOTS)(); gl.glGenQueries(SLOTS, ids)
    pending = [False] * SLOTS; state = {"next": 0, "open": False}
    def begin():
        if pending[state["next"]]: return
        gl.glBeginQuery(TIME_ELAPSED, ids[state["next"]]); state["open"] = True
    def end():
        if not state["open"]: return
        gl.glEndQuery(TIME_ELAPSED); pending[state["next"]] = True; state["open"] = False
        state["next"] = (state["next"] + 1) % SLOTS
    def collect(out):
        d = C.c_int(); gl.glGetIntegerv(DISJOINT, C.byref(d))
        for k in range(SLOTS):
            s = (state["next"] + k) % SLOTS
            if not pending[s]: continue
            ready = C.c_uint(); gl.glGetQueryObjectuiv(ids[s], QAVAIL, C.byref(ready))
            if ready.value == 0: continue
            ns = C.c_uint64(); result64(ids[s], QRES, C.byref(ns)); pending[s] = False
            if d.value == 0 and 0 < ns.value < 1_000_000_000: out.append(ns.value / 1e6)
    # a pass worth timing: a 512x512 FBO, clear + a full-screen triangle with a loop shader
    tex = C.c_uint(); gl.glGenTextures(1, C.byref(tex)); gl.glBindTexture(0x0DE1, tex)
    gl.glTexStorage2D(0x0DE1, 1, 0x8058, 512, 512)
    fbo = C.c_uint(); gl.glGenFramebuffers(1, C.byref(fbo)); gl.glBindFramebuffer(0x8D40, fbo)
    gl.glFramebufferTexture2D(0x8D40, 0x8CE0, 0x0DE1, tex, 0); assert gl.glCheckFramebufferStatus(0x8D40) == 0x8CD5
    def sh(kind, s):
        o = gl.glCreateShader(kind); bs = C.c_char_p(s.encode()); gl.glShaderSource(o, 1, C.byref(bs), None); gl.glCompileShader(o); return o
    prog = gl.glCreateProgram()
    gl.glAttachShader(prog, sh(0x8B31, "#version 300 es\nvoid main(){vec2 q=vec2(float((gl_VertexID<<1)&2),float(gl_VertexID&2));gl_Position=vec4(q*2.0-1.0,0.0,1.0);}"))
    gl.glAttachShader(prog, sh(0x8B30, "#version 300 es\nprecision highp float;out vec4 o;void main(){float a=gl_FragCoord.x;for(int i=0;i<200;i++){a=sin(a)*1.1+0.3;}o=vec4(a);}"))
    gl.glLinkProgram(prog); gl.glUseProgram(prog)
    vao = C.c_uint(); gl.glGenVertexArrays(1, C.byref(vao)); gl.glBindVertexArray(vao)
    out, frames_timed = [], 0
    for frame in range(40):
        begin()
        gl.glClear(0x4000)
        for _ in range(8): gl.glDrawArrays(4, 0, 3)
        end()
        gl.glFlush(); before = len(out); collect(out); frames_timed += len(out) > before
    gl.glFinish()
    for _ in range(10): collect(out)
    err("timer ring")
    print(f"timer ring: {len(out)} results from 40 frames, {out[:4]} ms ...")
    assert len(out) >= 30, "most frames should be timed"
print("OK")
