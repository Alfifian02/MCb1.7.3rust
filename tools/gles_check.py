"""Compile + link every GLES program of mc-rs/src/gles/shaders.rs on Mesa (EGL surfaceless, GLES 3.x) and draw a lit quad
through the chunk shader. Mirrors the assembly in shaders.rs by reading its raw strings (run: python3 tools/gles_check.py)."""
import ctypes as C, os, re, sys
os.environ.setdefault("EGL_PLATFORM", "surfaceless")
src = open(os.path.join(os.path.dirname(__file__), "../mc-rs/src/gles/shaders.rs")).read()
raw = re.findall(r'r#"(.*?)"#', src, re.S)
named = dict(re.findall(r'const (\w+): &str = r#"(.*?)"#;', src, re.S))
HEAD_V = "#version 300 es\nprecision highp float;\nprecision highp int;\n"
HEAD_F = HEAD_V + "precision highp sampler2D;\nprecision highp sampler2DArray;\n"
FULL_VS = "layout(location = 0) in vec3 unused;\nvoid main() { vec2 q = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2)); gl_Position = vec4(q * 2.0 - 1.0, 0.0, 1.0); }\n"
def fmt(s): return s.replace("{{", "{").replace("}}", "}")
def splice(s):
    return (s.replace("@TW@", "16").replace("@TH@", "%d" % (ATLAS_H // 16)).replace("@MIPS@", "3").replace("@IY@", "288")
            .replace("@CY@", "%d" % CHAR_Y).replace("@AW@", "256").replace("@AH@", "%d" % ATLAS_H)
            .replace("@SB@", repr(0.5 ** 2.2)).replace("@SZ@", repr(0.8 ** 2.2)).replace("@SX@", repr(0.6 ** 2.2)))
ITEMS_Y = 288; ART_Y = ITEMS_Y + 256; SIGN_Y = ART_Y + 256; CHAR_Y = SIGN_Y + 32; ATLAS_H = ART_Y + 512
progs = {}
for name, defs, frag in [("solid", "#define COMPACT\n", "#define FS_SOLID\n"), ("cutout", "#define COMPACT\n", "#define FS_CUT\n"),
                         ("entity", "", "#define FS_CUT_FLAT\n"), ("lod", "", "#define FS_CUT_FLAT\n"),
                         ("water", "#define COMPACT\n#define WATER\n", ""), ("lod_water", "#define WATER\n", "")]:
    for a2c in (0, 1):
        progs[f"chunk_{name}_a2c{a2c}"] = (HEAD_V + defs + named["CHUNK_COMMON"] + named["CHUNK_VS"],
            splice(HEAD_F + defs + frag + f"#define A2C {a2c}\n" + named["CHUNK_COMMON"] + named["CHUNK_FS"]))
present_fs = [r for r in raw if "enhanced" in r][0]
for fxaa, fast in [(1, 0), (0, 0), (0, 1)]:
    progs[f"present_{fxaa}{fast}"] = (HEAD_V + FULL_VS, HEAD_F + f"#define FXAA {fxaa}\n#define FAST {fast}\n" + present_fs)
hv = [r for r in raw if "a_pos" in r][0]
hf = fmt([r for r in raw if "SHEET" in r][0]).replace("{sheet_h:.1}", "2864.0")
progs["hud"] = (HEAD_V + hv, HEAD_F + hf)
pv = [r for r in raw if "u_t.xy" in r][0]; pf = [r for r in raw if "sampler2DArray u_tex" in r][0]
progs["pano"] = (HEAD_V + pv, HEAD_F + pf)
wv = [r for r in raw if "SLANTED" in r or "tilt" in r][0]
wf = fmt([r for r in raw if "is_rain" in r][0]).replace("{rain_opacity:?}", "0.25").replace("{snow_opacity:?}", "0.75")
progs["weather"] = (HEAD_V + wv, HEAD_F + wf)
SB = named["SKY_BLOCK"]; GC = re.search(r'const GEO_CLIP: &str = "(.*?)";', src).group(1).replace("\\n", "\n")
dv = [r for r in raw if "v_ndc = p" in r][0]; df = [r for r in raw if "inv_vp * vec4(v_ndc" in r][0]
progs["sky_dome"] = (HEAD_V + SB + dv, HEAD_F + SB + df)
for nm, vs in [("fan", "layout(location = 0) in vec3 a_p;\nlayout(location = 1) in vec4 a_col;\nout vec4 v_col;\nvoid main() { gl_Position = geo_clip(a_p); v_col = a_col; }\n"),
               ("stars", "layout(location = 0) in vec3 a_p;\nout vec4 v_col;\nvoid main() { mat3 m = mat3(celestial[0].xyz, celestial[1].xyz, celestial[2].xyz); gl_Position = geo_clip(transpose(m) * a_p); v_col = stars; }\n")]:
    progs["sky_" + nm] = (HEAD_V + SB + GC + vs, HEAD_F + "in vec4 v_col;\nlayout(location = 0) out vec4 o_color;\nvoid main() { o_color = v_col; }\n")
bv = [r for r in raw if "bool moon" in r][0]; bf = [r for r in raw if "u_body" in r][0]
progs["sky_body"] = (HEAD_V + SB + GC + bv, HEAD_F + SB + bf)

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
gl.glGetString.restype = C.c_char_p
print("GL:", gl.glGetString(0x1F02).decode(), "|", gl.glGetString(0x1F01).decode())
def compile_(kind, s):
    o = gl.glCreateShader(kind); b = C.c_char_p(s.encode()); gl.glShaderSource(o, 1, C.byref(b), None); gl.glCompileShader(o)
    ok = C.c_int(); gl.glGetShaderiv(o, 0x8B81, C.byref(ok))
    if not ok.value:
        buf = C.create_string_buffer(4096); gl.glGetShaderInfoLog(o, 4096, None, buf); return o, buf.value.decode()
    return o, None
bad = 0
for name, (vs, fs) in sorted(progs.items()):
    p = gl.glCreateProgram(); err = None
    for kind, s in ((0x8B31, vs), (0x8B30, fs)):
        o, e = compile_(kind, s)
        if e: err = ("VS " if kind == 0x8B31 else "FS ") + e; break
        gl.glAttachShader(p, o)
    if not err:
        gl.glLinkProgram(p); ok = C.c_int(); gl.glGetProgramiv(p, 0x8B82, C.byref(ok))
        if not ok.value:
            buf = C.create_string_buffer(4096); gl.glGetProgramInfoLog(p, 4096, None, buf); err = "LINK " + buf.value.decode()
    print(("FAIL " if err else "ok   ") + name); 
    if err: bad += 1; print(err)
if bad: sys.exit(1)

# ---- draw test: one lit white quad through the solid chunk program into an sRGB8_ALPHA8 framebuffer; the centre must come out white. ----
import struct
def gen(fn):
    v = C.c_uint(); fn(1, C.byref(v)); return v.value
W = H = 32
tex = gen(gl.glGenTextures); gl.glBindTexture(0x0DE1, tex); gl.glTexStorage2D(0x0DE1, 1, 0x8C43, W, H)
fbo = gen(gl.glGenFramebuffers); gl.glBindFramebuffer(0x8D40, fbo); gl.glFramebufferTexture2D(0x8D40, 0x8CE0, 0x0DE1, tex, 0)
assert gl.glCheckFramebufferStatus(0x8D40) == 0x8CD5
atlas = gen(gl.glGenTextures); gl.glActiveTexture(0x84C0); gl.glBindTexture(0x0DE1, atlas)
gl.glTexStorage2D(0x0DE1, 3, 0x8C43, 256, ATLAS_H)
white = (C.c_ubyte * (256 * ATLAS_H * 4))(*([255] * (256 * ATLAS_H * 4)))
gl.glTexSubImage2D(0x0DE1, 0, 0, 0, 256, ATLAS_H, 0x1908, 0x1401, white)
gl.glTexParameteri(0x0DE1, 0x2801, 0x2600); gl.glTexParameteri(0x0DE1, 0x2800, 0x2600)
vs, fs = progs["chunk_solid_a2c0"]
p = gl.glCreateProgram()
for kind, s in ((0x8B31, vs), (0x8B30, fs)):
    o, e = compile_(kind, s); assert not e, e; gl.glAttachShader(p, o)
gl.glLinkProgram(p); gl.glUseProgram(p)
gl.glGetUniformBlockIndex.restype = C.c_uint
gl.glUniformBlockBinding(p, gl.glGetUniformBlockIndex(p, b"U"), 0)
gl.glUniform1i(gl.glGetUniformLocation(p, b"u_tex"), 0)
gl.glUniform3f.argtypes = [C.c_int, C.c_float, C.c_float, C.c_float]; gl.glUniform3f(gl.glGetUniformLocation(p, b"u_origin"), 0.0, 0.0, 0.0)
ubo = gen(gl.glGenBuffers); gl.glBindBuffer(0x8A11, ubo)
u = struct.pack("16f", 1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1) + struct.pack("4f", 0, 0, 0, 0) + struct.pack("4f", 0.1, 0.2, 0.3, 0) + struct.pack("4f", 0, 0, 0, 0)
gl.glBufferData(0x8A11, len(u), u, 0x88E8); gl.glBindBufferBase(0x8A11, 0, ubo)
def vert(x, y): return struct.pack("<4H2H4B", int((x + 8) * 2048), int((y + 8) * 256), 16384, 240, 32768, 32768, 255, 255, 255, 255)
quad = b"".join(vert(x, y) for x, y in [(-.5, -.5), (.5, -.5), (.5, .5), (-.5, -.5), (.5, .5), (-.5, .5)])
vao = gen(gl.glGenVertexArrays); gl.glBindVertexArray(vao)
vb = gen(gl.glGenBuffers); gl.glBindBuffer(0x8892, vb); gl.glBufferData(0x8892, len(quad), quad, 0x88E4)
gl.glEnableVertexAttribArray(0); gl.glVertexAttribIPointer(0, 4, 0x1403, 16, C.c_void_p(0))
gl.glEnableVertexAttribArray(1); gl.glVertexAttribIPointer(1, 2, 0x1403, 16, C.c_void_p(8))
gl.glEnableVertexAttribArray(2); gl.glVertexAttribPointer(2, 4, 0x1401, 1, 16, C.c_void_p(12))
gl.glViewport(0, 0, W, H); gl.glClearColor(C.c_float(0), C.c_float(0), C.c_float(0), C.c_float(1)) if False else None
gl.glClearColor.argtypes = [C.c_float] * 4; gl.glClearColor(0, 0, 0, 1); gl.glClear(0x4000)
gl.glDrawArrays(4, 0, 6)
px = (C.c_ubyte * 4)(); gl.glReadPixels(W // 2, H // 2, 1, 1, 0x1908, 0x1401, px)
corner = (C.c_ubyte * 4)(); gl.glReadPixels(1, 1, 1, 1, 0x1908, 0x1401, corner)
print("centre", list(px), "corner", list(corner), "GL error", hex(gl.glGetError()))
assert min(px[0], px[1], px[2]) >= 250, "the lit white quad should be white"
assert max(corner[0], corner[1], corner[2]) == 0, "outside the quad stays cleared"
print("draw test ok")
