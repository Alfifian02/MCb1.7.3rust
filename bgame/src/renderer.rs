use crate::math::Mat4;
use crate::world::World;
use bcore::mesh;
use glow::HasContext;

// 512x512 raw RGBA atlas (16x16 tiles of 32px). Regenerate with tools/convert_atlas.py.
const ATLAS: &[u8] = include_bytes!("../assets/terrain.rgba");
const ATLAS_SIZE: i32 = 512;
const _: () = assert!(ATLAS.len() == (ATLAS_SIZE * ATLAS_SIZE * 4) as usize);

const VS: &str = "
attribute vec4 a_pos;
attribute vec4 a_uv;
uniform mat4 u_mvp;
uniform vec3 u_off;
varying vec2 v_uv;
varying float v_shade;
varying float v_fog;
void main() {
    gl_Position = u_mvp * vec4(a_pos.xyz + u_off, 1.0);
    float tile = a_uv.z;
    vec2 t = vec2(mod(tile, 16.0), floor(tile / 16.0));
    v_uv = (t + mix(vec2(0.02), vec2(0.98), a_uv.xy)) / 16.0;
    v_shade = a_uv.w / 255.0;
    v_fog = clamp((gl_Position.w - 28.0) / 24.0, 0.0, 1.0);
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

struct Section {
    vbo: glow::Buffer,
    ibo: glow::Buffer,
    count: i32,
    origin: [f32; 3],
    center: [f32; 3],
}

pub struct Renderer {
    gl: glow::Context,
    prog: glow::Program,
    tex: glow::Texture,
    u_mvp: Option<glow::UniformLocation>,
    u_off: Option<glow::UniformLocation>,
    u_tex: Option<glow::UniformLocation>,
    a_pos: u32,
    a_uv: u32,
    sections: Vec<Section>,
}

fn bytes<T>(v: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v)) }
}

impl Renderer {
    pub fn new(gl: glow::Context, world: &World) -> Self {
        unsafe {
            let prog = gl.create_program().expect("program");
            for (kind, src) in [(glow::VERTEX_SHADER, VS), (glow::FRAGMENT_SHADER, FS)] {
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

            let mut r = Renderer {
                u_mvp: gl.get_uniform_location(prog, "u_mvp"),
                u_off: gl.get_uniform_location(prog, "u_off"),
                u_tex: gl.get_uniform_location(prog, "u_tex"),
                a_pos: gl.get_attrib_location(prog, "a_pos").expect("a_pos"),
                a_uv: gl.get_attrib_location(prog, "a_uv").expect("a_uv"),
                gl, prog, tex, sections: Vec::new(),
            };
            r.upload_world(world);
            r
        }
    }

    unsafe fn upload_world(&mut self, world: &World) {
        for (&(cx, cz), chunk) in &world.chunks {
            for sy in 0..(bcore::chunk::H / mesh::SECTION) {
                let m = mesh::build_section(chunk, sy, &|nx, ny, nz| world.block(cx * 16 + nx, ny, cz * 16 + nz));
                if m.indices.is_empty() {
                    continue;
                }
                let gl = &self.gl;
                let vbo = gl.create_buffer().unwrap();
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
                gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes(&m.vertices), glow::STATIC_DRAW);
                let ibo = gl.create_buffer().unwrap();
                gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ibo));
                gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, bytes(&m.indices), glow::STATIC_DRAW);
                let (ox, oz) = ((cx * 16) as f32, (cz * 16) as f32);
                self.sections.push(Section {
                    vbo, ibo, count: m.indices.len() as i32,
                    origin: [ox, 0.0, oz],
                    center: [ox + 8.0, (sy * 16 + 8) as f32, oz + 8.0],
                });
            }
        }
    }

    pub fn draw(&self, size: (u32, u32), mvp: &Mat4, cam: [f32; 3], fwd: [f32; 3]) {
        let gl = &self.gl;
        unsafe {
            gl.viewport(0, 0, size.0 as i32, size.1 as i32);
            gl.clear_color(0.53, 0.71, 0.95, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
            gl.enable(glow::DEPTH_TEST);
            gl.enable(glow::CULL_FACE);
            gl.use_program(Some(self.prog));
            gl.uniform_matrix_4_f32_slice(self.u_mvp.as_ref(), false, mvp);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.tex));
            gl.uniform_1_i32(self.u_tex.as_ref(), 0);
            gl.enable_vertex_attrib_array(self.a_pos);
            gl.enable_vertex_attrib_array(self.a_uv);
            for s in &self.sections {
                // cheap culling: skip sections entirely behind the camera
                let d = [s.center[0] - cam[0], s.center[1] - cam[1], s.center[2] - cam[2]];
                if d[0] * fwd[0] + d[1] * fwd[1] + d[2] * fwd[2] < -14.0 {
                    continue;
                }
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(s.vbo));
                gl.vertex_attrib_pointer_f32(self.a_pos, 4, glow::UNSIGNED_BYTE, false, 8, 0);
                gl.vertex_attrib_pointer_f32(self.a_uv, 4, glow::UNSIGNED_BYTE, false, 8, 4);
                gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(s.ibo));
                gl.uniform_3_f32(self.u_off.as_ref(), s.origin[0], s.origin[1], s.origin[2]);
                gl.draw_elements(glow::TRIANGLES, s.count, glow::UNSIGNED_SHORT, 0);
            }
        }
    }
}
