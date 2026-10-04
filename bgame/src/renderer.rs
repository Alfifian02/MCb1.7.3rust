use crate::math::{self, Mat4};
use crate::ui::UiBatch;
use crate::world::{SecKey, World};
use bcore::{chunk::H, mesh};
use glow::HasContext;
use std::collections::HashMap;

// 512x512 raw RGBA atlas (16x16 tiles of 32px). Regenerate with tools/convert_atlas.py.
const ATLAS: &[u8] = include_bytes!("../assets/terrain.rgba");
const ATLAS_SIZE: i32 = 512;
const _: () = assert!(ATLAS.len() == (ATLAS_SIZE * ATLAS_SIZE * 4) as usize);

const VS: &str = "
attribute vec4 a_pos;
attribute vec4 a_uv;
uniform mat4 u_mvp;
uniform vec3 u_off;
uniform vec2 u_fog;
varying vec2 v_uv;
varying float v_shade;
varying float v_fog;
void main() {
    gl_Position = u_mvp * vec4(a_pos.xyz + u_off, 1.0);
    float tile = a_uv.z;
    vec2 t = vec2(mod(tile, 16.0), floor(tile / 16.0));
    v_uv = (t + mix(vec2(0.02), vec2(0.98), a_uv.xy)) / 16.0;
    v_shade = a_uv.w / 255.0;
    v_fog = clamp((gl_Position.w - u_fog.x) / (u_fog.y - u_fog.x), 0.0, 1.0);
}";

const FS: &str = "
precision mediump float;
uniform sampler2D u_tex;
varying vec2 v_uv;
varying float v_shade;
varying float v_fog;
void main() {
    vec4 c = texture2D(u_tex, v_uv);
    if (c.a < 0.5) discard;
    vec3 sky = vec3(0.53, 0.71, 0.95);
    gl_FragColor = vec4(mix(c.rgb * v_shade, sky, v_fog), 1.0);
}";

const LINE_VS: &str = "
attribute vec3 a_pos;
uniform mat4 u_mvp;
uniform vec3 u_off;
void main() { gl_Position = u_mvp * vec4(a_pos + u_off, 1.0); }";

const LINE_FS: &str = "
precision mediump float;
void main() { gl_FragColor = vec4(0.0, 0.0, 0.0, 1.0); }";

const UI_VS: &str = "
attribute vec2 a_pos;
attribute vec2 a_uv;
attribute vec4 a_col;
uniform vec2 u_screen;
varying vec2 v_uv;
varying vec4 v_col;
void main() {
    gl_Position = vec4(a_pos.x / u_screen.x * 2.0 - 1.0, 1.0 - a_pos.y / u_screen.y * 2.0, 0.0, 1.0);
    v_uv = a_uv;
    v_col = a_col;
}";

const UI_FS: &str = "
precision mediump float;
uniform sampler2D u_tex;
varying vec2 v_uv;
varying vec4 v_col;
void main() {
    float use_tex = step(0.0, v_uv.x);
    vec4 t = texture2D(u_tex, max(v_uv, vec2(0.0)));
    gl_FragColor = mix(v_col, t * v_col, use_tex);
}";

struct Section {
    vbo: glow::Buffer,
    ibo: glow::Buffer,
    count: i32,
    origin: [f32; 3],
    min_y: f32,
}

pub struct Renderer {
    gl: glow::Context,
    tex: glow::Texture,
    // world
    prog: glow::Program,
    u_mvp: Option<glow::UniformLocation>,
    u_off: Option<glow::UniformLocation>,
    u_tex: Option<glow::UniformLocation>,
    u_fog: Option<glow::UniformLocation>,
    a_pos: u32,
    a_uv: u32,
    // block outline
    line_prog: glow::Program,
    l_mvp: Option<glow::UniformLocation>,
    l_off: Option<glow::UniformLocation>,
    l_pos: u32,
    line_vbo: glow::Buffer,
    // ui
    ui_prog: glow::Program,
    ui_screen: Option<glow::UniformLocation>,
    ui_tex: Option<glow::UniformLocation>,
    ui_pos: u32,
    ui_uv: u32,
    ui_col: u32,
    ui_vbo: glow::Buffer,
    sections: HashMap<SecKey, Section>,
    order: Vec<(f32, SecKey)>, // scratch: visible sections sorted front-to-back
}

fn bytes<T>(v: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v)) }
}

fn compile(gl: &glow::Context, vs: &str, fs: &str) -> glow::Program {
    unsafe {
        let prog = gl.create_program().expect("program");
        for (kind, src) in [(glow::VERTEX_SHADER, vs), (glow::FRAGMENT_SHADER, fs)] {
            let sh = gl.create_shader(kind).expect("shader");
            gl.shader_source(sh, src);
            gl.compile_shader(sh);
            if !gl.get_shader_compile_status(sh) {
                panic!("shader error: {}", gl.get_shader_info_log(sh));
            }
            gl.attach_shader(prog, sh);
        }
        gl.link_program(prog);
        if !gl.get_program_link_status(prog) {
            panic!("link error: {}", gl.get_program_info_log(prog));
        }
        prog
    }
}

fn outline_vertices() -> Vec<f32> {
    let (a, b) = (-0.003f32, 1.003f32);
    let c = |i: usize| [if i & 1 == 0 { a } else { b }, if i & 2 == 0 { a } else { b }, if i & 4 == 0 { a } else { b }];
    let mut v = Vec::new();
    for (i, j) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
        v.extend_from_slice(&c(i));
        v.extend_from_slice(&c(j));
    }
    v
}

impl Renderer {
    pub fn new(gl: glow::Context) -> Self {
        unsafe {
            let prog = compile(&gl, VS, FS);
            let line_prog = compile(&gl, LINE_VS, LINE_FS);
            let ui_prog = compile(&gl, UI_VS, UI_FS);

            let tex = gl.create_texture().expect("texture");
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.tex_image_2d(
                glow::TEXTURE_2D, 0, glow::RGBA as i32, ATLAS_SIZE, ATLAS_SIZE, 0,
                glow::RGBA, glow::UNSIGNED_BYTE, Some(ATLAS),
            );
            for (p, v) in [
                (glow::TEXTURE_MIN_FILTER, glow::NEAREST),
                (glow::TEXTURE_MAG_FILTER, glow::NEAREST),
                (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
                (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
            ] {
                gl.tex_parameter_i32(glow::TEXTURE_2D, p, v as i32);
            }

            let line_vbo = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(line_vbo));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes(&outline_vertices()), glow::STATIC_DRAW);
            let ui_vbo = gl.create_buffer().unwrap();

            Renderer {
                u_mvp: gl.get_uniform_location(prog, "u_mvp"),
                u_off: gl.get_uniform_location(prog, "u_off"),
                u_tex: gl.get_uniform_location(prog, "u_tex"),
                u_fog: gl.get_uniform_location(prog, "u_fog"),
                a_pos: gl.get_attrib_location(prog, "a_pos").expect("a_pos"),
                a_uv: gl.get_attrib_location(prog, "a_uv").expect("a_uv"),
                l_mvp: gl.get_uniform_location(line_prog, "u_mvp"),
                l_off: gl.get_uniform_location(line_prog, "u_off"),
                l_pos: gl.get_attrib_location(line_prog, "a_pos").expect("l_pos"),
                ui_screen: gl.get_uniform_location(ui_prog, "u_screen"),
                ui_tex: gl.get_uniform_location(ui_prog, "u_tex"),
                ui_pos: gl.get_attrib_location(ui_prog, "a_pos").expect("ui_pos"),
                ui_uv: gl.get_attrib_location(ui_prog, "a_uv").expect("ui_uv"),
                ui_col: gl.get_attrib_location(ui_prog, "a_col").expect("ui_col"),
                gl, tex, prog, line_prog, line_vbo, ui_prog, ui_vbo,
                sections: HashMap::new(),
                order: Vec::new(),
            }
        }
    }

    fn upload(&mut self, key: SecKey, m: &mesh::Mesh) {
        if m.indices.is_empty() {
            return;
        }
        unsafe {
            let gl = &self.gl;
            let vbo = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes(&m.vertices), glow::STATIC_DRAW);
            let ibo = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ibo));
            gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, bytes(&m.indices), glow::STATIC_DRAW);
            self.sections.insert(key, Section {
                vbo, ibo, count: m.indices.len() as i32,
                origin: [(key.0 * 16) as f32, 0.0, (key.1 * 16) as f32],
                min_y: (key.2 * 16) as f32,
            });
        }
    }

    fn drop_section(&mut self, key: SecKey) {
        if let Some(old) = self.sections.remove(&key) {
            unsafe {
                self.gl.delete_buffer(old.vbo);
                self.gl.delete_buffer(old.ibo);
            }
        }
    }

    /// Replaces every section of a chunk with freshly built meshes (from the streaming workers).
    pub fn set_chunk_meshes(&mut self, cx: i32, cz: i32, sections: Vec<(usize, mesh::Mesh)>) {
        self.remove_chunk(cx, cz);
        for (sy, m) in &sections {
            self.upload((cx, cz, *sy), m);
        }
    }

    pub fn remove_chunk(&mut self, cx: i32, cz: i32) {
        for sy in 0..H / mesh::SECTION {
            self.drop_section((cx, cz, sy));
        }
    }

    /// Synchronous re-mesh of a few sections (block edits must show up instantly).
    pub fn remesh(&mut self, world: &World, keys: &[SecKey]) {
        for &key in keys {
            self.drop_section(key);
            let (cx, cz, sy) = key;
            let Some(chunk) = world.chunks.get(&(cx, cz)) else { continue };
            let m = mesh::build_section(chunk, sy, &|nx, ny, nz| world.block(cx * 16 + nx, ny, cz * 16 + nz));
            self.upload(key, &m);
        }
    }

    unsafe fn reset_attribs(&self) {
        for i in 0..4 {
            self.gl.disable_vertex_attrib_array(i);
        }
    }

    pub fn draw(&mut self, size: (u32, u32), mvp: &Mat4, cam: [f32; 3], fog: (f32, f32)) {
        let planes = math::frustum(mvp);
        let mut order = std::mem::take(&mut self.order);
        order.clear();
        for (k, s) in &self.sections {
            let min = [s.origin[0], s.min_y, s.origin[2]];
            let max = [min[0] + 16.0, min[1] + 16.0, min[2] + 16.0];
            if !math::aabb_visible(&planes, min, max) {
                continue;
            }
            let d = [min[0] + 8.0 - cam[0], min[1] + 8.0 - cam[1], min[2] + 8.0 - cam[2]];
            order.push((d[0] * d[0] + d[1] * d[1] + d[2] * d[2], *k));
        }
        order.sort_unstable_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        let gl = &self.gl;
        unsafe {
            gl.viewport(0, 0, size.0 as i32, size.1 as i32);
            gl.clear_color(0.53, 0.71, 0.95, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
            gl.disable(glow::BLEND);
            gl.enable(glow::DEPTH_TEST);
            gl.enable(glow::CULL_FACE);
            self.reset_attribs();
            gl.use_program(Some(self.prog));
            gl.uniform_matrix_4_f32_slice(self.u_mvp.as_ref(), false, mvp);
            gl.uniform_2_f32(self.u_fog.as_ref(), fog.0, fog.1);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.tex));
            gl.uniform_1_i32(self.u_tex.as_ref(), 0);
            gl.enable_vertex_attrib_array(self.a_pos);
            gl.enable_vertex_attrib_array(self.a_uv);
            for (_, k) in &order {
                let s = &self.sections[k];
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(s.vbo));
                gl.vertex_attrib_pointer_f32(self.a_pos, 4, glow::UNSIGNED_BYTE, false, 8, 0);
                gl.vertex_attrib_pointer_f32(self.a_uv, 4, glow::UNSIGNED_BYTE, false, 8, 4);
                gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(s.ibo));
                gl.uniform_3_f32(self.u_off.as_ref(), s.origin[0], s.origin[1], s.origin[2]);
                gl.draw_elements(glow::TRIANGLES, s.count, glow::UNSIGNED_SHORT, 0);
            }
        }
        self.order = order;
    }

    pub fn draw_outline(&self, mvp: &Mat4, block: [i32; 3]) {
        let gl = &self.gl;
        unsafe {
            self.reset_attribs();
            gl.use_program(Some(self.line_prog));
            gl.uniform_matrix_4_f32_slice(self.l_mvp.as_ref(), false, mvp);
            gl.uniform_3_f32(self.l_off.as_ref(), block[0] as f32, block[1] as f32, block[2] as f32);
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.line_vbo));
            gl.enable_vertex_attrib_array(self.l_pos);
            gl.vertex_attrib_pointer_f32(self.l_pos, 3, glow::FLOAT, false, 12, 0);
            gl.line_width(2.0);
            gl.draw_arrays(glow::LINES, 0, 24);
        }
    }

    pub fn draw_ui(&self, batch: &UiBatch, size: (u32, u32)) {
        if batch.verts.is_empty() {
            return;
        }
        let gl = &self.gl;
        unsafe {
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::CULL_FACE);
            gl.enable(glow::BLEND);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            self.reset_attribs();
            gl.use_program(Some(self.ui_prog));
            gl.uniform_2_f32(self.ui_screen.as_ref(), size.0 as f32, size.1 as f32);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.tex));
            gl.uniform_1_i32(self.ui_tex.as_ref(), 0);
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.ui_vbo));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes(&batch.verts), glow::STREAM_DRAW);
            gl.enable_vertex_attrib_array(self.ui_pos);
            gl.enable_vertex_attrib_array(self.ui_uv);
            gl.enable_vertex_attrib_array(self.ui_col);
            gl.vertex_attrib_pointer_f32(self.ui_pos, 2, glow::FLOAT, false, 20, 0);
            gl.vertex_attrib_pointer_f32(self.ui_uv, 2, glow::FLOAT, false, 20, 8);
            gl.vertex_attrib_pointer_f32(self.ui_col, 4, glow::UNSIGNED_BYTE, true, 20, 16);
            gl.draw_arrays(glow::TRIANGLES, 0, batch.verts.len() as i32);
            gl.disable(glow::BLEND);
        }
    }
}
