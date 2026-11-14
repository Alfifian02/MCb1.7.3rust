"""Decode a compressed texture with Mesa (llvmpipe) through EGL + GLES3: an independent reference decoder."""
import ctypes as C, numpy as np, os
os.environ.setdefault("EGL_PLATFORM","surfaceless")
egl=C.CDLL("libEGL.so.1"); gl=C.CDLL("libGLESv2.so.2")
egl.eglGetProcAddress.restype=C.c_void_p
EGL_SURFACELESS_MESA=0x31DD
egl.eglGetPlatformDisplay.restype=C.c_void_p
egl.eglGetPlatformDisplay.argtypes=[C.c_uint,C.c_void_p,C.c_void_p]
dpy=egl.eglGetPlatformDisplay(EGL_SURFACELESS_MESA,None,None)
assert dpy
maj,mnr=C.c_int(),C.c_int()
egl.eglInitialize.argtypes=[C.c_void_p,C.c_void_p,C.c_void_p]
assert egl.eglInitialize(C.c_void_p(dpy),C.byref(maj),C.byref(mnr))
egl.eglBindAPI(0x30A0)
cfgattr=(C.c_int*5)(0x3040,0x40,0x3033,0x1,0x3038) # renderable GLES3, surface pbuf? use none
cfg=C.c_void_p(); n=C.c_int()
egl.eglChooseConfig.argtypes=[C.c_void_p,C.c_void_p,C.c_void_p,C.c_int,C.c_void_p]
# surfaceless: need EGL_NO_CONFIG_KHR
ctxattr=(C.c_int*3)(0x3098,3,0x3038)
egl.eglCreateContext.restype=C.c_void_p
egl.eglCreateContext.argtypes=[C.c_void_p,C.c_void_p,C.c_void_p,C.c_void_p]
egl.eglChooseConfig(C.c_void_p(dpy),(C.c_int*3)(0x3040,0x40,0x3038),C.byref(cfg),1,C.byref(n))
ctx=egl.eglCreateContext(C.c_void_p(dpy),cfg,None,ctxattr)
assert ctx, hex(egl.eglGetError())
egl.eglMakeCurrent.argtypes=[C.c_void_p]*4
assert egl.eglMakeCurrent(C.c_void_p(dpy),None,None,C.c_void_p(ctx)), hex(egl.eglGetError())
gl.glGetString.restype=C.c_char_p


def _sh(kind, src):
    s=gl.glCreateShader(kind)
    b=C.c_char_p(src.encode()); gl.glShaderSource(s,1,C.byref(b),None); gl.glCompileShader(s)
    ok=C.c_int(); gl.glGetShaderiv(s,0x8B81,C.byref(ok))
    if not ok.value:
        buf=C.create_string_buffer(2048); gl.glGetShaderInfoLog(s,2048,None,buf); raise RuntimeError(buf.value.decode())
    return s
_prog=None
def _init():
    global _prog
    if _prog: return
    vs=_sh(0x8B31,"#version 300 es\nvoid main(){vec2 p=vec2(float((gl_VertexID<<1)&2),float(gl_VertexID&2));gl_Position=vec4(p*2.0-1.0,0.0,1.0);}")
    fs=_sh(0x8B30,"#version 300 es\nprecision highp float;uniform sampler2D t;uniform int lvl;out vec4 o;void main(){o=texelFetch(t,ivec2(gl_FragCoord.xy),lvl);}")
    p=gl.glCreateProgram(); gl.glAttachShader(p,vs); gl.glAttachShader(p,fs); gl.glLinkProgram(p); gl.glUseProgram(p)
    vao=C.c_uint(); gl.glGenVertexArrays(1,C.byref(vao)); gl.glBindVertexArray(vao)
    _prog=p

def decode(levels, w, h, fmt):
    """levels: list of bytes (mip chain, level 0 first), returns list of (h,w,4) uint8 arrays."""
    _init()
    tex=C.c_uint(); gl.glGenTextures(1,C.byref(tex)); gl.glBindTexture(0x0DE1,tex)
    for i,d in enumerate(levels):
        lw,lh=max(1,w>>i),max(1,h>>i)
        gl.glCompressedTexImage2D(0x0DE1,i,fmt,lw,lh,0,len(d),d)
        err=gl.glGetError()
        assert err==0, f"GL error {err:#x} at level {i}"
    gl.glTexParameteri(0x0DE1,0x2801,0x2600); gl.glTexParameteri(0x0DE1,0x2800,0x2600)
    gl.glTexParameteri(0x0DE1,0x813D,len(levels)-1)
    out=[]
    for i in range(len(levels)):
        lw,lh=max(1,w>>i),max(1,h>>i)
        rb=C.c_uint(); gl.glGenRenderbuffers(1,C.byref(rb)); gl.glBindRenderbuffer(0x8D41,rb)
        gl.glRenderbufferStorage(0x8D41,0x8058,lw,lh)
        fb=C.c_uint(); gl.glGenFramebuffers(1,C.byref(fb)); gl.glBindFramebuffer(0x8D40,fb)
        gl.glFramebufferRenderbuffer(0x8D40,0x8CE0,0x8D41,rb)
        gl.glViewport(0,0,lw,lh)
        gl.glUniform1i(gl.glGetUniformLocation(_prog,b"lvl"),i)
        gl.glDrawArrays(0x0004,0,3)
        buf=(C.c_ubyte*(lw*lh*4))()
        gl.glReadPixels(0,0,lw,lh,0x1908,0x1401,buf)
        out.append(np.frombuffer(buf,dtype=np.uint8).reshape(lh,lw,4).copy())
        gl.glDeleteFramebuffers(1,C.byref(fb)); gl.glDeleteRenderbuffers(1,C.byref(rb))
    gl.glDeleteTextures(1,C.byref(tex))
    return out
