//! The OpenGL ES frame: the same two passes as the Vulkan renderer (`App::render_frame`), drawn with GLES calls.
//! Scene pass at `gpu::context::scene_size` (dynamic resolution), MSAA kept in tile memory when the driver allows, depth thrown away
//! at the end of the pass; then ONE full-screen pass at surface size (CAS + upscale + vignette + sRGB) with the HUD blended over it.
//! Conventions: GL clip depth is -1..1, so every view-projection gets `depth_fix`; images are uploaded as they are and sampled with the
//! same uv, so nothing is flipped; the window surface is plain RGBA8 and the final shader encodes sRGB.

use std::collections::HashMap;
use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec4};

use super::shaders::{self, Chunk};
use super::sys::*;
use super::HUD_QUADS;
use super::{Buf, Gl, Prog};
use crate::gpu::gfx::{Drawn, Gfx, WorldDraw};
use crate::gpu::pipeline::Uniforms;
use crate::render::{atlas, hud, present, sky, weather};
use crate::world::chunks::Layer;

/// GL clip z = 2 z - w: turns the wgpu / Vulkan projection (depth 0..1, what `FirstPersonCamera` builds) into GL's -1..1.
pub fn depth_fix() -> Mat4 {
    Mat4::from_cols(Vec4::X, Vec4::Y, Vec4::new(0.0, 0.0, 2.0, 0.0), Vec4::new(0.0, 0.0, -1.0, 1.0))
}

/// The `S` block of the sky shaders (std140).
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct SkyU {
    inv_vp: [[f32; 4]; 4],
    sun_dir: [f32; 4],
    moon_dir: [f32; 4],
    celestial: [[f32; 4]; 4],
    misc: [f32; 4],
    zenith: [f32; 4],
    horizon: [f32; 4],
    stars: [f32; 4],
    vp: [[f32; 4]; 4],
}

/// A vertex layout: stride and (location, components, type, integer, normalised, offset).
struct Layout {
    stride: i32,
    attrs: &'static [(u32, i32, Enum, bool, bool, usize)],
}

const CHUNK12: Layout = Layout { stride: 12, attrs: &[(0, 3, UNSIGNED_INT, true, false, 0)] };
const FLOAT32: Layout = Layout { stride: 32, attrs: &[(0, 3, FLOAT, false, false, 0), (1, 2, FLOAT, false, false, 12), (2, 1, FLOAT, false, false, 20), (3, 1, FLOAT, false, false, 24), (4, 1, FLOAT, false, false, 28)] };
const POS3: Layout = Layout { stride: 12, attrs: &[(0, 3, FLOAT, false, false, 0)] };
const FAN: Layout = Layout { stride: 28, attrs: &[(0, 3, FLOAT, false, false, 0), (1, 4, FLOAT, false, false, 12)] };
const WEATHER: Layout = Layout { stride: 36, attrs: &[(0, 3, FLOAT, false, false, 0), (1, 2, FLOAT, false, false, 12), (2, 4, FLOAT, false, false, 20)] };
const HUD: Layout = Layout { stride: 32, attrs: &[(0, 2, FLOAT, false, false, 0), (1, 4, FLOAT, false, false, 8), (2, 2, FLOAT, false, false, 24)] };
const LAYOUTS: [&Layout; 6] = [&CHUNK12, &FLOAT32, &POS3, &FAN, &WEATHER, &HUD];
const L_CHUNK: usize = 0;
const L_FLOAT: usize = 1;
const L_POS: usize = 2;
const L_FAN: usize = 3;
const L_WEATHER: usize = 4;
const L_HUD: usize = 5;

/// Texture units: each sampler is set once, so a pass only binds what it needs.
const U_ATLAS: u32 = 0;
const U_BODY: u32 = 1;
const U_WEATHER: u32 = 2;
const U_HUD: u32 = 3;
const U_SCENE: u32 = 4;
const U_PANO: u32 = 5;

const HUD_VERT: usize = 32;

struct Targets {
    scene: u32,
    /// What the scene pass draws into (the multisampled one, or the scene itself).
    draw: u32,
    depth: u32,
    /// Without tile-memory MSAA: the multisampled colour and the framebuffer the scene is resolved into.
    ms_color: u32,
    resolve: u32,
    size: (u32, u32),
}

#[derive(Copy, Clone, PartialEq)]
enum Blend {
    Off,
    Alpha,
    Add,
    Premul,
}

pub struct Renderer {
    pub gl: Gl,
    size: (u32, u32),
    samples: u32,
    fast: bool,
    chunk: Vec<(Chunk, Prog)>,
    present: Prog,
    hud: Prog,
    pano: Prog,
    dome: Prog,
    fan: Prog,
    stars: Prog,
    body: Prog,
    weather: Prog,
    ubo_frame: Buf,
    ubo_sky: Buf,
    atlas: u32,
    hud_tex: u32,
    body_tex: u32,
    snow_tex: u32,
    rain_tex: u32,
    pano_tex: u32,
    t: Targets,
    vaos: HashMap<(u32, u32, usize), u32>,
    empty_vao: u32,
    hud_buf: Buf,
    fan_buf: Buf,
    star_v: Buf,
    star_i: Buf,
    star_n: i32,
    body_v: Buf,
    weather_buf: Buf,
    born: Instant,
    /// The pipeline state last set, so a pass does not repeat it.
    cur_prog: u32,
    cur_vao: u32,
    blend: Blend,
}

fn tex_params(f: &Fns, target: Enum, min: i32, mag: i32, wrap: i32) {
    // SAFETY: parameters of the texture bound to `target`.
    unsafe {
        (f.TexParameteri)(target, TEXTURE_MIN_FILTER, min);
        (f.TexParameteri)(target, TEXTURE_MAG_FILTER, mag);
        (f.TexParameteri)(target, TEXTURE_WRAP_S, wrap);
        (f.TexParameteri)(target, TEXTURE_WRAP_T, wrap);
    }
}

impl Renderer {
    /// `window` = the `ANativeWindow*`. The context is current on the calling thread; `release` it before the app moves to the render thread.
    pub fn new(window: *mut core::ffi::c_void) -> Result<Self, String> {
        let gl = Gl::new(window)?;
        let weak = present::weak_gpu(&gl.name);
        let samples = present::resolve(present::msaa_want(), weak, |n| n as i32 <= gl.max_samples);
        let fast = present::resolve_post(present::post_want(), weak);
        present::set_scene_samples(samples);
        present::set_fast_post(fast);
        log::info!("GLES init: scene MSAA x{samples}, final pass {}", if fast { "fast (no FXAA/CAS)" } else { "full" });
        let a2c = samples > 1;
        let mut chunk = Vec::new();
        for c in Chunk::ALL {
            let s = shaders::chunk(c, a2c);
            chunk.push((c, gl.program(&s.vs, &s.fs, &["u_tex", "u_origin"]).map_err(|e| format!("{c:?}: {e}"))?));
        }
        let mk = |s: shaders::Src, u: &[&str], what: &str| gl.program(&s.vs, &s.fs, u).map_err(|e| format!("{what}: {e}"));
        let present = mk(shaders::present(samples == 1, fast), &["u_src", "u_px"], "present")?;
        let hud = mk(shaders::hud(hud::SHEET_H), &["u_items", "u_size"], "hud")?;
        let pano = mk(shaders::panorama(), &["u_tex", "u_m", "u_t"], "panorama")?;
        let dome = mk(shaders::sky_dome(), &[], "sky dome")?;
        let fan = mk(shaders::sky_geo(true), &[], "sky fan")?;
        let stars = mk(shaders::sky_geo(false), &[], "stars")?;
        let body = mk(shaders::sky_body(), &["u_body"], "sun and moon")?;
        let weather = mk(shaders::weather(weather::SNOW_OPACITY, weather::RAIN_OPACITY), &["u_tex", "u_vp", "u_eye", "u_rain", "u_snow"], "weather")?;

        let f = gl.f();
        let ubo_frame = gl.buffer(std::mem::size_of::<Uniforms>(), &[], DYNAMIC_DRAW);
        let ubo_sky = gl.buffer(std::mem::size_of::<SkyU>(), &[], DYNAMIC_DRAW);
        // SAFETY: binds the two uniform blocks to points 0 and 1, and sets each sampler to its texture unit once.
        unsafe {
            (f.BindBufferBase)(UNIFORM_BUFFER, 0, ubo_frame.id);
            (f.BindBufferBase)(UNIFORM_BUFFER, 1, ubo_sky.id);
            for (_, p) in &chunk {
                (f.UseProgram)(p.id);
                (f.Uniform1i)(p.loc[0], U_ATLAS as i32);
            }
            let unit = |p: &Prog, i: usize, u: u32| {
                (f.UseProgram)(p.id);
                (f.Uniform1i)(p.loc[i], u as i32);
            };
            unit(&present, 0, U_SCENE);
            unit(&hud, 0, U_HUD);
            unit(&pano, 0, U_PANO);
            unit(&body, 0, U_BODY);
            unit(&weather, 0, U_WEATHER);
        }
        let texture = |unit: u32, target: Enum| -> u32 {
            let mut t = 0;
            // SAFETY: a fresh texture bound to `unit`.
            unsafe {
                (f.GenTextures)(1, &mut t);
                (f.ActiveTexture)(TEXTURE0 + unit);
                (f.BindTexture)(target, t);
            }
            t
        };
        let up = |w: i32, h: i32, level: i32, bytes: &[u8]| {
            // SAFETY: `bytes` is a whole RGBA8 image of w x h.
            unsafe { (f.TexSubImage2D)(TEXTURE_2D, level, 0, 0, w, h, RGBA, UNSIGNED_BYTE, bytes.as_ptr() as *const _) };
        };
        // The terrain atlas with its mips (sRGB: sampling decodes, the scene encodes).
        let atlas_t = texture(U_ATLAS, TEXTURE_2D);
        // SAFETY: immutable storage for the whole mip chain, then each level written once.
        unsafe { (f.TexStorage2D)(TEXTURE_2D, atlas::MIPS as i32, SRGB8_ALPHA8, atlas::ATLAS_W as i32, atlas::ATLAS_H as i32) };
        let (mut level, mut w, mut h) = (atlas::atlas_rgba(), atlas::ATLAS_W, atlas::ATLAS_H);
        for l in 0..atlas::MIPS {
            up(w as i32, h as i32, l as i32, &level);
            if l + 1 < atlas::MIPS {
                level = atlas::mip(&level, w, h);
                (w, h) = (w / 2, h / 2);
            }
        }
        // Post: Fast (weak GPUs) reads one mip per sample instead of blending two: half the texture-unit work.
        tex_params(f, TEXTURE_2D, if fast { LINEAR_MIPMAP_NEAREST } else { LINEAR_MIPMAP_LINEAR }, NEAREST, REPEAT);
        // The HUD sheet (plain RGBA8: the HUD writes sRGB bytes straight to a non-sRGB surface).
        let hud_t = texture(U_HUD, TEXTURE_2D);
        // SAFETY: as above, one level.
        unsafe { (f.TexStorage2D)(TEXTURE_2D, 1, RGBA8, 256, hud::SHEET_H as i32) };
        up(256, hud::SHEET_H as i32, 0, &hud::sheet_bytes());
        tex_params(f, TEXTURE_2D, LINEAR, LINEAR, CLAMP_TO_EDGE);
        let body_t = texture(U_BODY, TEXTURE_2D);
        // SAFETY: as above.
        unsafe { (f.TexStorage2D)(TEXTURE_2D, 1, SRGB8_ALPHA8, 64, 32) };
        up(64, 32, 0, sky::CELESTIAL);
        tex_params(f, TEXTURE_2D, LINEAR, LINEAR, CLAMP_TO_EDGE);
        let snow_t = texture(U_WEATHER, TEXTURE_2D);
        // SAFETY: as above.
        unsafe { (f.TexStorage2D)(TEXTURE_2D, 1, RGBA8, 64, 256) };
        up(64, 256, 0, include_bytes!("../../assets/snow.rgba"));
        tex_params(f, TEXTURE_2D, NEAREST, NEAREST, REPEAT);
        let rain_t = texture(U_WEATHER, TEXTURE_2D); // both weather textures share the unit; each is bound at draw time
        // SAFETY: as above.
        unsafe { (f.TexStorage2D)(TEXTURE_2D, 1, RGBA8, 64, 256) };
        up(64, 256, 0, include_bytes!("../../assets/rain.rgba"));
        tex_params(f, TEXTURE_2D, NEAREST, NEAREST, REPEAT);
        let pano_t = texture(U_PANO, TEXTURE_2D_ARRAY);
        let face = crate::render::panorama::FACE as i32;
        // SAFETY: six faces of face x face RGBA8 in one array texture.
        unsafe {
            (f.TexStorage3D)(TEXTURE_2D_ARRAY, 1, RGBA8, face, face, 6);
            (f.TexSubImage3D)(TEXTURE_2D_ARRAY, 0, 0, 0, 0, face, face, 6, RGBA, UNSIGNED_BYTE, crate::render::panorama::PANORAMA.as_ptr() as *const _);
            (f.TexParameteri)(TEXTURE_2D_ARRAY, TEXTURE_MIN_FILTER, LINEAR);
            (f.TexParameteri)(TEXTURE_2D_ARRAY, TEXTURE_MAG_FILTER, LINEAR);
            (f.TexParameteri)(TEXTURE_2D_ARRAY, TEXTURE_WRAP_S, CLAMP_TO_EDGE);
            (f.TexParameteri)(TEXTURE_2D_ARRAY, TEXTURE_WRAP_T, CLAMP_TO_EDGE);
        }
        let mut empty_vao = 0;
        // SAFETY: a VAO with no attributes, for the full-screen triangles.
        unsafe { (f.GenVertexArrays)(1, &mut empty_vao) };

        // Stars, the sunrise fan and the sun / moon quads: the legacy sky's geometry (`render/sky.rs`).
        let star_verts: Vec<[f32; 3]> = crate::world::sky::star_vertices().iter().map(|p| p.map(|c| c as f32)).collect();
        let star_idx: Vec<u32> = (0..star_verts.len() as u32 / 4).flat_map(|s| [4 * s, 4 * s + 1, 4 * s + 2, 4 * s, 4 * s + 2, 4 * s + 3]).collect();
        let quad = [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
        let body_verts: Vec<[f32; 3]> = [0.0f32, 1.0].iter().flat_map(|&k| quad.map(|(x, y)| [x, y, k])).collect();
        let sv: &[u8] = bytemuck::cast_slice(&star_verts);
        let si: &[u8] = bytemuck::cast_slice(&star_idx);
        let bv: &[u8] = bytemuck::cast_slice(&body_verts);
        let star_v = gl.buffer(sv.len(), sv, STATIC_DRAW);
        let star_i = gl.buffer(si.len(), si, STATIC_DRAW);
        let body_v = gl.buffer(bv.len(), bv, STATIC_DRAW);
        let fan_buf = gl.buffer(48 * 28, &[], DYNAMIC_DRAW);
        let hud_buf = gl.buffer(HUD_QUADS * 6 * HUD_VERT, &[], STREAM_DRAW);
        let weather_buf = gl.buffer(2 * weather::MAX_VERTS * std::mem::size_of::<weather::Vertex>(), &[], STREAM_DRAW);

        let size = gl.surface_size();
        let mut r = Self {
            t: Targets { scene: 0, draw: 0, depth: 0, ms_color: 0, resolve: 0, size: (0, 0) },
            gl, size, samples, fast, chunk, present, hud, pano, dome, fan, stars, body, weather, ubo_frame, ubo_sky,
            atlas: atlas_t, hud_tex: hud_t, body_tex: body_t, snow_tex: snow_t, rain_tex: rain_t, pano_tex: pano_t,
            vaos: HashMap::new(), empty_vao, hud_buf, fan_buf, star_v, star_i, star_n: star_idx.len() as i32, body_v, weather_buf,
            born: Instant::now(), cur_prog: 0, cur_vao: 0, blend: Blend::Off,
        };
        r.targets()?;
        Ok(r)
    }

    pub fn gfx(&self) -> Gfx<'_> {
        Gfx::Gl(&self.gl)
    }

    pub fn surface(&self) -> (u32, u32) {
        self.size
    }

    /// (Re)makes the scene targets for the current surface size and render scale; falls back to one sample if the framebuffer is incomplete.
    fn targets(&mut self) -> Result<(), String> {
        let (sw, sh) = crate::gpu::context::scene_size(self.size.0, self.size.1);
        let f = self.gl.f();
        let t = &mut self.t;
        // SAFETY: old objects are deleted by name, new ones created and attached; every framebuffer is checked.
        unsafe {
            (f.DeleteFramebuffers)(1, &t.draw);
            (f.DeleteFramebuffers)(1, &t.resolve);
            (f.DeleteRenderbuffers)(1, &t.depth);
            (f.DeleteRenderbuffers)(1, &t.ms_color);
            (f.DeleteTextures)(1, &t.scene);
            let mut scene = 0;
            (f.GenTextures)(1, &mut scene);
            (f.ActiveTexture)(TEXTURE0 + U_SCENE);
            (f.BindTexture)(TEXTURE_2D, scene);
            (f.TexStorage2D)(TEXTURE_2D, 1, SRGB8_ALPHA8, sw as i32, sh as i32);
            tex_params(f, TEXTURE_2D, LINEAR, LINEAR, CLAMP_TO_EDGE);
            let mut depth = 0;
            (f.GenRenderbuffers)(1, &mut depth);
            (f.BindRenderbuffer)(RENDERBUFFER, depth);
            let (mut fbo, mut resolve, mut ms_color) = (0, 0, 0);
            (f.GenFramebuffers)(1, &mut fbo);
            (f.BindFramebuffer)(FRAMEBUFFER, fbo);
            let n = self.samples as i32;
            match (n > 1, self.gl.ms_rtt) {
                (true, Some((fb_tex_ms, rb_ms))) => {
                    // Tile-memory MSAA: the texture is the resolve target, the samples are never stored.
                    rb_ms(RENDERBUFFER, n, DEPTH_COMPONENT32F, sw as i32, sh as i32);
                    fb_tex_ms(FRAMEBUFFER, COLOR_ATTACHMENT0, TEXTURE_2D, scene, 0, n);
                }
                (true, None) => {
                    (f.RenderbufferStorageMultisample)(RENDERBUFFER, n, DEPTH_COMPONENT32F, sw as i32, sh as i32);
                    (f.GenRenderbuffers)(1, &mut ms_color);
                    (f.BindRenderbuffer)(RENDERBUFFER, ms_color);
                    (f.RenderbufferStorageMultisample)(RENDERBUFFER, n, SRGB8_ALPHA8, sw as i32, sh as i32);
                    (f.FramebufferRenderbuffer)(FRAMEBUFFER, COLOR_ATTACHMENT0, RENDERBUFFER, ms_color);
                    (f.GenFramebuffers)(1, &mut resolve);
                    (f.BindFramebuffer)(FRAMEBUFFER, resolve);
                    (f.FramebufferTexture2D)(FRAMEBUFFER, COLOR_ATTACHMENT0, TEXTURE_2D, scene, 0);
                    (f.BindFramebuffer)(FRAMEBUFFER, fbo);
                }
                (false, _) => {
                    (f.RenderbufferStorage)(RENDERBUFFER, DEPTH_COMPONENT32F, sw as i32, sh as i32);
                    (f.FramebufferTexture2D)(FRAMEBUFFER, COLOR_ATTACHMENT0, TEXTURE_2D, scene, 0);
                }
            }
            (f.BindRenderbuffer)(RENDERBUFFER, depth);
            (f.FramebufferRenderbuffer)(FRAMEBUFFER, DEPTH_ATTACHMENT, RENDERBUFFER, depth);
            let ok = (f.CheckFramebufferStatus)(FRAMEBUFFER) == FRAMEBUFFER_COMPLETE;
            *t = Targets { scene, draw: fbo, depth, ms_color, resolve, size: (sw, sh) };
            (f.BindFramebuffer)(FRAMEBUFFER, 0);
            if !ok {
                if self.samples > 1 {
                    log::warn!("GLES: the x{} scene framebuffer is incomplete, using one sample", self.samples);
                    self.samples = 1;
                    return self.targets();
                }
                return Err("GLES: the scene framebuffer is incomplete".into());
            }
        }
        Ok(())
    }

    /// A new surface size (rotation, first frame): targets and the final pass's pixel sizes.
    pub fn resize(&mut self, w: u32, h: u32) {
        self.size = (w.max(1), h.max(1));
        self.rescale();
    }

    /// The scene targets at the current scale (dynamic resolution; the surface stays as it is).
    pub fn rescale(&mut self) {
        if let Err(e) = self.targets() {
            log::error!("{e}");
        }
    }

    /// One animated block-atlas tile per entry (and its mips): `render::anim`.
    pub fn animate(&self, tiles: &[(u8, crate::render::anim::Tile)], items: bool) {
        let f = self.gl.f();
        let y0 = if items { atlas::ITEMS_Y as u32 } else { 0 };
        // SAFETY: sub-image writes inside the atlas's mip chain.
        unsafe {
            (f.ActiveTexture)(TEXTURE0 + U_ATLAS);
            (f.BindTexture)(TEXTURE_2D, self.atlas);
            for (t, px) in tiles {
                let (x, y) = ((*t as u32 % 16) * 16, y0 + (*t as u32 / 16) * 16);
                let (mut level, mut n) = (px.to_vec(), 16usize);
                for l in 0..atlas::MIPS {
                    (f.TexSubImage2D)(TEXTURE_2D, l as i32, (x >> l) as i32, (y >> l) as i32, n as i32, n as i32, RGBA, UNSIGNED_BYTE, level.as_ptr() as *const _);
                    if l + 1 < atlas::MIPS {
                        level = atlas::mip(&level, n, n);
                        n /= 2;
                    }
                }
            }
        }
    }

    /// The compass and clock in the HUD sheet.
    pub fn animate_hud(&self, tiles: &[(u8, crate::render::anim::Tile)]) {
        let f = self.gl.f();
        // SAFETY: 16 x 16 writes inside the HUD texture.
        unsafe {
            (f.ActiveTexture)(TEXTURE0 + U_HUD);
            (f.BindTexture)(TEXTURE_2D, self.hud_tex);
            for (t, px) in tiles {
                (f.TexSubImage2D)(TEXTURE_2D, 0, (*t as i32 % 16) * 16, (*t as i32 / 16) * 16, 16, 16, RGBA, UNSIGNED_BYTE, px.as_ptr() as *const _);
            }
        }
    }

    fn use_prog(&mut self, p: u32) {
        if self.cur_prog != p {
            // SAFETY: a program made by `Gl::program`.
            unsafe { (self.gl.f().UseProgram)(p) };
            self.cur_prog = p;
        }
    }

    fn bind_vao(&mut self, vb: u32, ib: u32, layout: usize) {
        let id = match self.vaos.get(&(vb, ib, layout)) {
            Some(&v) => v,
            None => {
                let f = self.gl.f();
                let (mut v, l) = (0, LAYOUTS[layout]);
                // SAFETY: a VAO recording the attribute pointers into `vb` and the element buffer `ib`.
                unsafe {
                    (f.GenVertexArrays)(1, &mut v);
                    (f.BindVertexArray)(v);
                    (f.BindBuffer)(ARRAY_BUFFER, vb);
                    for &(loc, n, ty, int, norm, off) in l.attrs {
                        (f.EnableVertexAttribArray)(loc);
                        if int {
                            (f.VertexAttribIPointer)(loc, n, ty, l.stride, off as *const _);
                        } else {
                            (f.VertexAttribPointer)(loc, n, ty, norm as u8, l.stride, off as *const _);
                        }
                    }
                    if ib != 0 {
                        (f.BindBuffer)(ELEMENT_ARRAY_BUFFER, ib);
                    }
                    (f.BindBuffer)(ARRAY_BUFFER, 0);
                }
                self.cur_vao = v;
                self.vaos.insert((vb, ib, layout), v);
                return;
            }
        };
        if self.cur_vao != id {
            // SAFETY: a VAO from the cache.
            unsafe { (self.gl.f().BindVertexArray)(id) };
            self.cur_vao = id;
        }
    }

    fn bind_empty(&mut self) {
        if self.cur_vao != self.empty_vao {
            // SAFETY: the attribute-less VAO.
            unsafe { (self.gl.f().BindVertexArray)(self.empty_vao) };
            self.cur_vao = self.empty_vao;
        }
    }

    /// Frees dropped buffers and the VAOs that pointed at them.
    fn sweep(&mut self) {
        let dead = self.gl.flush_deletes();
        if dead.is_empty() {
            return;
        }
        let f = self.gl.f();
        let gone: Vec<(u32, u32, usize)> = self.vaos.keys().filter(|k| dead.contains(&k.0) || dead.contains(&k.1)).copied().collect();
        for k in gone {
            if let Some(v) = self.vaos.remove(&k) {
                // SAFETY: a VAO this renderer made.
                unsafe { (f.DeleteVertexArrays)(1, &v) };
                if self.cur_vao == v {
                    self.cur_vao = 0;
                }
            }
        }
    }

    fn set_blend(&mut self, b: Blend) {
        if self.blend == b {
            return;
        }
        let f = self.gl.f();
        // SAFETY: fixed-function blend state.
        unsafe {
            match b {
                Blend::Off => (f.Disable)(BLEND),
                Blend::Alpha => {
                    (f.Enable)(BLEND);
                    (f.BlendFuncSeparate)(SRC_ALPHA, ONE_MINUS_SRC_ALPHA, ONE, ONE_MINUS_SRC_ALPHA);
                }
                Blend::Add => {
                    (f.Enable)(BLEND);
                    (f.BlendFuncSeparate)(SRC_ALPHA, ONE, ONE, ONE_MINUS_SRC_ALPHA);
                }
                Blend::Premul => {
                    (f.Enable)(BLEND);
                    (f.BlendFunc)(ONE, ONE_MINUS_SRC_ALPHA);
                }
            }
        }
        self.blend = b;
    }

    fn chunk_prog(&self, c: Chunk) -> (u32, i32) {
        let p = &self.chunk.iter().find(|(k, _)| *k == c).expect("every chunk program is built").1;
        (p.id, p.loc[1])
    }

    /// Draws one chunk layer's runs with program `c`.
    fn runs(&mut self, d: &WorldDraw, layer: Layer, c: Chunk) {
        let (prog, origin_loc) = self.chunk_prog(c);
        self.use_prog(prog);
        let mut last: Option<u32> = None;
        let mut page: Option<u32> = None;
        d.chunks.for_each_run(layer, |r| {
            if page != Some(r.page) {
                self.bind_vao(r.vbuf.gl().id, r.quads.gl().id, L_CHUNK);
                page = Some(r.page);
            }
            if last != Some(r.slot) {
                // SAFETY: the chunk origin uniform of the bound program.
                unsafe { (self.gl.f().Uniform3f)(origin_loc, r.origin[0], r.origin[1], r.origin[2]) };
                last = Some(r.slot);
            }
            // SAFETY: indices `first..end` of the bound shared quad buffer (byte offset = first * 4).
            unsafe { (self.gl.f().DrawElements)(TRIANGLES, (r.end - r.first) as i32, UNSIGNED_INT, (r.first as usize * 4) as *const _) };
        });
    }

    fn tiles(&mut self, d: &WorldDraw, water: bool, c: Chunk) {
        let Some(l) = d.lod else { return };
        let (prog, _) = self.chunk_prog(c);
        self.use_prog(prog);
        l.for_each_tile(water, |vb, ib, n| {
            self.bind_vao(vb.gl().id, ib.gl().id, L_FLOAT);
            // SAFETY: `n` indices of the bound element buffer.
            unsafe { (self.gl.f().DrawElements)(TRIANGLES, n as i32, UNSIGNED_INT, std::ptr::null()) };
        });
    }

    fn draw_buf(&mut self, vb: &crate::gpu::gfx::Buf, ib: &crate::gpu::gfx::Buf, n: u32) {
        self.bind_vao(vb.gl().id, ib.gl().id, L_FLOAT);
        // SAFETY: `n` indices of the bound element buffer.
        unsafe { (self.gl.f().DrawElements)(TRIANGLES, n as i32, UNSIGNED_INT, std::ptr::null()) };
    }

    /// One world frame: scene pass, final pass with the HUD, swap.
    pub fn draw_world(&mut self, d: &WorldDraw) -> Drawn {
        let now = self.gl.surface_size();
        if now != self.size {
            return Drawn::Resize(now.0, now.1);
        }
        self.sweep();
        let fix = depth_fix();
        let f = *self.gl.f();
        // The frame's uniforms (the wgpu `ChunkPipeline::frame`, with GL's clip depth) and the sky's.
        let u = Uniforms {
            view_proj: (fix * d.proj * d.view).to_cols_array_2d(),
            eye: d.eye.extend(d.sky_sub).to_array(),
            fog: [d.fog[0], d.fog[1], d.fog[2], d.far],
            fogp: [d.start, d.wr, 0.0, 0.0],
        };
        self.gl.write(&self.ubo_frame, 0, bytemuck::bytes_of(&u));
        let sun = crate::world::sky::sun_dir(d.angle);
        let mut view = d.view;
        view.w_axis = Vec4::W; // the sky travels with the viewer
        let svp = fix * d.proj * view;
        let stars = crate::world::sky::star_brightness(d.angle) * (1.0 - d.rain);
        let su = SkyU {
            inv_vp: svp.inverse().to_cols_array_2d(),
            sun_dir: sun.extend(0.0).to_array(),
            moon_dir: (-sun).extend(0.0).to_array(),
            celestial: Mat4::from_rotation_x(-d.angle * std::f32::consts::TAU).to_cols_array_2d(),
            misc: [d.rain, 0.0, d.seconds, 0.0],
            zenith: [d.zenith[0], d.zenith[1], d.zenith[2], 0.0],
            horizon: [d.horizon[0], d.horizon[1], d.horizon[2], 0.0],
            stars: [1.0, 1.0, 1.0, stars],
            vp: svp.to_cols_array_2d(),
        };
        self.gl.write(&self.ubo_sky, 0, bytemuck::bytes_of(&su));
        let fan = sky::fan_vertices(d.angle, d.rain, sun);
        self.gl.write(&self.fan_buf, 0, bytemuck::cast_slice(&fan));
        let hud_n = d.hud.len().min(HUD_QUADS * 6);
        self.gl.stream(&self.hud_buf, HUD_QUADS * 6 * HUD_VERT, bytemuck::cast_slice(&d.hud[..hud_n]));

        let (sw, sh) = self.t.size;
        // SAFETY: the scene pass: bind, clear, fixed state.
        unsafe {
            (f.BindFramebuffer)(FRAMEBUFFER, self.t.draw);
            (f.Viewport)(0, 0, sw as i32, sh as i32);
            (f.DepthMask)(TRUE);
            (f.ClearColor)(d.fog[0], d.fog[1], d.fog[2], 1.0);
            (f.ClearDepthf)(1.0);
            (f.Clear)(COLOR_BUFFER_BIT | DEPTH_BUFFER_BIT);
            (f.Enable)(DEPTH_TEST);
            (f.DepthFunc)(LESS);
            (f.Enable)(CULL_FACE);
            (f.CullFace)(BACK);
            (f.Disable)(BLEND);
            (f.ActiveTexture)(TEXTURE0 + U_ATLAS);
            (f.BindTexture)(TEXTURE_2D, self.atlas);
        }
        self.blend = Blend::Off;
        let a2c = self.samples > 1;
        // Opaque: chunks near to far, the far terrain (pushed back so it loses every tie with a real chunk), cut-outs, entities, outline.
        self.runs(d, Layer::Solid, Chunk::Solid);
        if d.lod.is_some() {
            // SAFETY: depth bias of the far terrain (wgpu: constant 8, slope 1).
            unsafe {
                (f.Enable)(POLYGON_OFFSET_FILL);
                (f.PolygonOffset)(1.0, 8.0);
                if a2c {
                    (f.Enable)(SAMPLE_ALPHA_TO_COVERAGE);
                }
            }
            self.tiles(d, false, Chunk::Lod);
            // SAFETY: back to no bias.
            unsafe { (f.Disable)(POLYGON_OFFSET_FILL) };
        }
        // SAFETY: alpha-to-coverage for the cut-out layers (leaves, plants, glass, entities).
        unsafe {
            if a2c {
                (f.Enable)(SAMPLE_ALPHA_TO_COVERAGE);
            }
        }
        self.runs(d, Layer::Cutout, Chunk::Cutout);
        if d.item_indices > 0 || d.outline_indices > 0 {
            let (p, _) = self.chunk_prog(Chunk::Entity);
            self.use_prog(p);
            if d.item_indices > 0 {
                self.draw_buf(&d.items.vbuf, &d.items.ibuf, d.item_indices);
            }
            if d.outline_indices > 0 {
                self.draw_buf(&d.outline.vbuf, &d.outline.ibuf, d.outline_indices);
            }
        }
        // SAFETY: the sky is read-only against depth (only pixels still at 1) and blends.
        unsafe {
            (f.Disable)(SAMPLE_ALPHA_TO_COVERAGE);
            (f.Disable)(CULL_FACE);
            (f.DepthMask)(FALSE);
            (f.DepthFunc)(LEQUAL);
        }
        if !d.nether {
            self.use_prog(self.dome.id);
            self.bind_empty();
            // SAFETY: one triangle, no attributes.
            unsafe { (f.DrawArrays)(TRIANGLES, 0, 3) };
            self.set_blend(Blend::Alpha);
            if !fan.is_empty() {
                self.use_prog(self.fan.id);
                self.bind_vao(self.fan_buf.id, 0, L_FAN);
                // SAFETY: the fan's vertices.
                unsafe { (f.DrawArrays)(TRIANGLES, 0, fan.len() as i32) };
            }
            self.set_blend(Blend::Add);
            self.use_prog(self.body.id);
            self.bind_vao(self.body_v.id, 0, L_POS);
            // SAFETY: sun and moon, 12 vertices.
            unsafe { (f.DrawArrays)(TRIANGLES, 0, 12) };
            if stars > 0.0 {
                self.set_blend(Blend::Alpha);
                self.use_prog(self.stars.id);
                self.bind_vao(self.star_v.id, self.star_i.id, L_POS);
                // SAFETY: the star quads' indices.
                unsafe { (f.DrawElements)(TRIANGLES, self.star_n, UNSIGNED_INT, std::ptr::null()) };
            }
        }
        // Water last: blended over everything, no depth write; the far sheets first.
        self.set_blend(Blend::Alpha);
        // SAFETY: depth test as the opaque pass, no writes.
        unsafe { (f.DepthFunc)(LESS) };
        if d.lod.is_some() {
            self.tiles(d, true, Chunk::LodWater);
        }
        self.runs(d, Layer::Water, Chunk::Water);
        if let Some((snow, rain, sun_y)) = d.precip {
            self.rain(d, snow, rain, sun_y, fix);
        }
        // SAFETY: end of the pass: the depth is never read again, and (without tile-memory MSAA) the samples resolve into the scene.
        unsafe {
            if self.t.ms_color != 0 {
                (f.BindFramebuffer)(READ_FRAMEBUFFER, self.t.draw);
                (f.BindFramebuffer)(DRAW_FRAMEBUFFER, self.t.resolve);
                (f.BlitFramebuffer)(0, 0, sw as i32, sh as i32, 0, 0, sw as i32, sh as i32, COLOR_BUFFER_BIT, NEAREST as u32);
                (f.InvalidateFramebuffer)(READ_FRAMEBUFFER, 2, [COLOR_ATTACHMENT0, DEPTH_ATTACHMENT].as_ptr());
            } else {
                (f.InvalidateFramebuffer)(FRAMEBUFFER, 1, [DEPTH_ATTACHMENT].as_ptr());
            }
        }
        self.finish(d.hud.len().min(HUD_QUADS * 6), None);
        let t = Instant::now();
        self.gl.swap();
        Drawn::Done(t.elapsed())
    }

    fn rain(&mut self, d: &WorldDraw, snow: &[weather::Vertex], rain: &[weather::Vertex], sun_y: f32, fix: Mat4) {
        let max = weather::MAX_VERTS;
        let (snow, rain) = (&snow[..snow.len().min(max)], &rain[..rain.len().min(max)]);
        if snow.is_empty() && rain.is_empty() {
            return;
        }
        let vsz = std::mem::size_of::<weather::Vertex>();
        self.gl.write(&self.weather_buf, 0, bytemuck::cast_slice(snow));
        self.gl.write(&self.weather_buf, (max * vsz) as u64, bytemuck::cast_slice(rain));
        let (rc, sc) = weather::colors(sun_y);
        let vp = fix * d.proj * d.view;
        self.set_blend(Blend::Premul);
        self.use_prog(self.weather.id);
        let l = self.weather.loc.clone();
        let f = *self.gl.f();
        // SAFETY: the weather program's uniforms, then one draw per kind (snow first, like the Java).
        unsafe {
            (f.UniformMatrix4fv)(l[1], 1, FALSE, vp.to_cols_array().as_ptr());
            (f.Uniform4f)(l[2], d.eye.x, d.eye.y, d.eye.z, 0.0);
            (f.Uniform4f)(l[3], rc[0], rc[1], rc[2], 0.0);
            (f.Uniform4f)(l[4], sc[0], sc[1], sc[2], 0.0);
            (f.ActiveTexture)(TEXTURE0 + U_WEATHER);
        }
        self.bind_vao(self.weather_buf.id, 0, L_WEATHER);
        // SAFETY: draws inside the buffer written above.
        unsafe {
            if !snow.is_empty() {
                (f.BindTexture)(TEXTURE_2D, self.snow_tex);
                (f.DrawArrays)(TRIANGLES, 0, snow.len() as i32);
            }
            if !rain.is_empty() {
                (f.BindTexture)(TEXTURE_2D, self.rain_tex);
                (f.DrawArrays)(TRIANGLES, max as i32, rain.len() as i32);
            }
        }
    }

    /// The final pass: the scene (unless `scene` is false: the menu) upscaled with CAS, vignette and sRGB, then the HUD over it.
    fn finish(&mut self, hud_verts: usize, menu: Option<bool>) {
        let (w, h) = self.size;
        let f = *self.gl.f();
        // SAFETY: the default framebuffer, full-screen triangle, then blended HUD quads.
        unsafe {
            (f.BindFramebuffer)(FRAMEBUFFER, 0);
            (f.Viewport)(0, 0, w as i32, h as i32);
            (f.Disable)(DEPTH_TEST);
            (f.Disable)(CULL_FACE);
            (f.DepthMask)(TRUE);
            (f.ClearColor)(0.0, 0.0, 0.0, 1.0);
            (f.Clear)(COLOR_BUFFER_BIT);
        }
        self.set_blend(Blend::Off);
        match menu {
            None => {
                let (sw, sh) = self.t.size;
                let l = self.present.loc.clone();
                self.use_prog(self.present.id);
                // SAFETY: the scene texture on its unit, the size uniform, one triangle.
                unsafe {
                    (f.ActiveTexture)(TEXTURE0 + U_SCENE);
                    (f.BindTexture)(TEXTURE_2D, self.t.scene);
                    (f.Uniform4f)(l[1], 1.0 / sw as f32, 1.0 / sh as f32, 1.0 / w as f32, 1.0 / h as f32);
                }
                self.bind_empty();
                // SAFETY: as above.
                unsafe { (f.DrawArrays)(TRIANGLES, 0, 3) };
            }
            Some(true) => {
                let secs = self.born.elapsed().as_secs_f32();
                let u = crate::render::panorama::Panorama::params(secs, w as f32, h as f32);
                let l = self.pano.loc.clone();
                self.use_prog(self.pano.id);
                // SAFETY: the panorama's matrix and tangents, one triangle.
                unsafe {
                    (f.UniformMatrix4fv)(l[1], 1, FALSE, u.as_ptr());
                    (f.Uniform4f)(l[2], u[16], u[17], 0.0, 0.0);
                    (f.ActiveTexture)(TEXTURE0 + U_PANO);
                    (f.BindTexture)(TEXTURE_2D_ARRAY, self.pano_tex);
                }
                self.bind_empty();
                // SAFETY: as above.
                unsafe { (f.DrawArrays)(TRIANGLES, 0, 3) };
            }
            Some(false) => {}
        }
        if hud_verts > 0 {
            self.set_blend(Blend::Alpha);
            self.use_prog(self.hud.id);
            // SAFETY: the HUD sheet on its unit, the surface size, the vertices streamed this frame.
            unsafe {
                (f.Uniform2f)(self.hud.loc[1], w as f32, h as f32);
                (f.ActiveTexture)(TEXTURE0 + U_HUD);
                (f.BindTexture)(TEXTURE_2D, self.hud_tex);
            }
            self.bind_vao(self.hud_buf.id, 0, L_HUD);
            // SAFETY: `hud_verts` vertices of the buffer.
            unsafe { (f.DrawArrays)(TRIANGLES, 0, hud_verts as i32) };
        }
    }

    /// A menu frame (`menu::Menu`): the panorama (title screen) under the HUD, no world.
    pub fn draw_menu(&mut self, pano: bool, hud_verts: &[hud::HudVertex]) -> Drawn {
        let now = self.gl.surface_size();
        if now != self.size {
            return Drawn::Resize(now.0, now.1);
        }
        self.sweep();
        let n = hud_verts.len().min(HUD_QUADS * 6);
        self.gl.stream(&self.hud_buf, HUD_QUADS * 6 * HUD_VERT, bytemuck::cast_slice(&hud_verts[..n]));
        self.finish(n, Some(pano));
        let t = Instant::now();
        self.gl.swap();
        Drawn::Done(t.elapsed())
    }

    pub fn fast(&self) -> bool {
        self.fast
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GL's clip depth is -1..1: the fix maps the wgpu near / far planes (0, 1) to (-1, 1) and keeps x, y, w.
    #[test]
    fn depth_fix_maps_zero_one_to_minus_one_one() {
        let p = Mat4::perspective_rh(1.0, 1.5, 0.1, 100.0);
        for (z, want) in [(-0.1f32, -1.0f32), (-100.0, 1.0)] {
            let c = depth_fix() * p * Vec4::new(0.3, 0.2, z, 1.0);
            assert!((c.z / c.w - want).abs() < 1e-4, "{z}: {}", c.z / c.w);
            let o = p * Vec4::new(0.3, 0.2, z, 1.0);
            assert!((c.x - o.x).abs() < 1e-6 && (c.y - o.y).abs() < 1e-6 && (c.w - o.w).abs() < 1e-6);
        }
    }

    /// The vertex layouts match the Rust vertex types they read.
    #[test]
    fn layouts_match_the_vertex_types() {
        assert_eq!(CHUNK12.stride as usize, embeddium::vertex::STRIDE);
        assert_eq!(FLOAT32.stride as usize, std::mem::size_of::<crate::gpu::pipeline::Vertex>());
        assert_eq!(WEATHER.stride as usize, std::mem::size_of::<weather::Vertex>());
        assert_eq!(HUD.stride as usize, std::mem::size_of::<hud::HudVertex>());
        assert_eq!(std::mem::size_of::<SkyU>(), 288);
        assert_eq!(std::mem::size_of::<Uniforms>(), 112);
    }

    /// Every program's source assembles with each define, and the cut-out ones get alpha-to-coverage only when multisampled.
    #[test]
    fn chunk_sources_carry_their_switches() {
        for c in Chunk::ALL {
            let (a, b) = (shaders::chunk(c, true), shaders::chunk(c, false));
            assert!(a.fs.contains("#define A2C 1") && b.fs.contains("#define A2C 0"));
            assert_eq!(a.vs.contains("#define COMPACT"), c.compact());
            assert_eq!(a.vs.contains("#define WATER"), c.water());
            assert!(!a.fs.contains('@'), "an unspliced constant");
        }
    }
}
