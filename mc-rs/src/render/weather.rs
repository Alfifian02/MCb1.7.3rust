//! Milestone 6, part 3: rain and snow. Geometry is `EntityRenderer.renderRainSnow` (b1.7.3, same Random draws, same quads, same
//! per-column alpha and light); shading is Photon's `gbuffers_weather.vsh/.fsh`: `SLANTED_RAIN` tilt in the vertex stage, in the
//! fragment stage `RAIN_OPACITY 0.25` / `SNOW_OPACITY 0.75` with `get_rain_color` / `get_snow_color` (PROGRAM_WEATHER branch),
//! output premultiplied (`rgb *= a`) into the translucent layer (`colortex13`), which the water composite blends over the scene.
//!
//! Drawn in its own pass over the finished scene (`lib.rs`): colour is blended (premultiplied) into the HDR scene, the depth attachment is
//! the terrain's, read-only, so the hardware hides the streaks behind it (`water.rs`' translucent layer is not wired yet).
//!
//! Deviations: `FANCY` is false (radius 5, the Java default `gameSettings.fancyGraphics` off: cheaper on a phone); the sun colour
//! is `sunlight_color` of the pack, not yet attenuated by the atmosphere (milestone 2); no TAA jitter (milestone 5); the particle is
//! hidden behind terrain by the depth test of its pass (the pack compares `depthtex1` in the fragment shader).
//! UNVERIFIED: written WITHOUT a Rust toolchain, never compiled or run.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::{BindGroup, Buffer, Device, Queue, RenderPass, RenderPipeline, TextureView};

use crate::world::biome::Biome;
use crate::world::gen::noise::JavaRandom;

/// `gameSettings.fancyGraphics`: radius 10 instead of 5.
pub const FANCY: bool = false;
const RADIUS: i32 = if FANCY { 10 } else { 5 };
pub(crate) const RAIN_OPACITY: f32 = 0.25;
pub(crate) const SNOW_OPACITY: f32 = 0.75;
/// `sunlight_color` of `include/sky/atmosphere.glsl`.
const SUNLIGHT: Vec3 = Vec3::new(1.051, 0.985, 0.940);
/// Two crossed quads, two triangles each, per column; the most columns is the fancy radius.
pub(crate) const MAX_VERTS: usize = 21 * 21 * 12;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    /// `glColor4f`: (brightness, brightness, brightness, alpha).
    pub tint: [f32; 4],
}

/// What `renderRainSnow` reads from the world.
pub struct Input<'a> {
    /// The viewer (`posX/Y/Z`; the eye, like the player entity of b1.7.3).
    pub eye: Vec3,
    /// `rendererUpdateCount`.
    pub ticks: u32,
    pub partial: f32,
    /// `World.func_27162_g`.
    pub rain: f32,
    /// Biomes of the (2r+1)^2 area from (floor(x) - r, floor(z) - r), x outer, z inner (`ChunkManager::biomes`).
    pub biomes: &'a [Biome],
    /// `World.findTopSolidBlock`.
    pub top_solid: &'a dyn Fn(i32, i32) -> i32,
    /// `World.getLightBrightness`.
    pub brightness: &'a dyn Fn(i32, i32, i32) -> f32,
}

pub fn radius() -> i32 {
    RADIUS
}

fn quad(out: &mut Vec<Vertex>, v: [([f32; 3], [f32; 2]); 4], tint: [f32; 4]) {
    for i in [0, 1, 2, 0, 2, 3] {
        out.push(Vertex { pos: v[i].0, uv: v[i].1, tint });
    }
}

/// The two crossed quads of one column, `y0` below `y1`; `uv_y` is the texture v of a height.
fn column(out: &mut Vec<Vertex>, x: i32, z: i32, y0: i32, y1: i32, u0: f32, u1: f32, uv_y: impl Fn(f32) -> f32, tint: [f32; 4]) {
    let (xf, zf, a, b) = (x as f32, z as f32, y0 as f32, y1 as f32);
    quad(out, [([xf, a, zf + 0.5], [u0, uv_y(a)]), ([xf + 1.0, a, zf + 0.5], [u1, uv_y(a)]), ([xf + 1.0, b, zf + 0.5], [u1, uv_y(b)]), ([xf, b, zf + 0.5], [u0, uv_y(b)])], tint);
    quad(out, [([xf + 0.5, a, zf], [u0, uv_y(a)]), ([xf + 0.5, a, zf + 1.0], [u1, uv_y(a)]), ([xf + 0.5, b, zf + 1.0], [u1, uv_y(b)]), ([xf + 0.5, b, zf], [u0, uv_y(b)])], tint);
}

/// `World`-column hash of `renderRainSnow` (int arithmetic, wraps like Java).
fn hash(x: i32, z: i32) -> i32 {
    x.wrapping_mul(x).wrapping_mul(3121).wrapping_add(x.wrapping_mul(45238971)).wrapping_add(z.wrapping_mul(z).wrapping_mul(418711)).wrapping_add(z.wrapping_mul(13761))
}

/// `renderRainSnow` up to the draw calls: (snow quads, rain quads), world coordinates.
pub fn build(i: &Input) -> (Vec<Vertex>, Vec<Vertex>) {
    let (mut snow, mut rain) = (Vec::new(), Vec::new());
    if i.rain <= 0.0 {
        return (snow, rain);
    }
    let mut rng = JavaRandom::new(0);
    let (px, py, pz) = (i.eye.x as f64, i.eye.y as f64, i.eye.z as f64);
    let (bx, by, bz) = (px.floor() as i32, py.floor() as i32, pz.floor() as i32);
    let r = RADIUS;
    let rf = r as f32;
    // var15 is floor of the interpolated y; the eye already is interpolated, so it equals `by`.
    let y_interp = by;

    let mut n = 0;
    for x in bx - r..=bx + r {
        for z in bz - r..=bz + r {
            let biome = i.biomes[n];
            n += 1;
            if !biome.snows() {
                continue;
            }
            let mut top = (i.top_solid)(x, z);
            if top < 0 {
                top = 0;
            }
            let mut low_light_y = top;
            if top < y_interp {
                low_light_y = y_interp;
            }
            let (mut y0, mut y1) = (by - r, by + r);
            if y0 < top {
                y0 = top;
            }
            if y1 < top {
                y1 = top;
            }
            if y0 != y1 {
                rng.set_seed(hash(x, z) as i64);
                let t = i.ticks as f32 + i.partial;
                let v_scroll = (((i.ticks & 511) as f32) + i.partial) / 512.0;
                let u_off = rng.next_float() + t * 0.01 * rng.next_gaussian() as f32;
                let v_off = rng.next_float() + t * rng.next_gaussian() as f32 * 0.001;
                let (dx, dz) = ((x as f32 + 0.5) as f64 - px, (z as f32 + 0.5) as f64 - pz);
                let d = ((dx * dx + dz * dz).sqrt()) as f32 / rf;
                let l = (i.brightness)(x, low_light_y, z);
                column(&mut snow, x, z, y0, y1, u_off, 1.0 + u_off, |y| y / 4.0 + v_scroll + v_off, [l, l, l, ((1.0 - d * d) * 0.3 + 0.5) * i.rain]);
            }
        }
    }

    n = 0;
    for x in bx - r..=bx + r {
        for z in bz - r..=bz + r {
            let biome = i.biomes[n];
            n += 1;
            if !biome.rains() {
                continue;
            }
            let top = (i.top_solid)(x, z);
            let (mut y0, mut y1) = (by - r, by + r);
            if y0 < top {
                y0 = top;
            }
            if y1 < top {
                y1 = top;
            }
            if y0 != y1 {
                rng.set_seed(hash(x, z) as i64);
                let scroll = ((((i.ticks as i32).wrapping_add(hash(x, z)) & 31) as f32) + i.partial) / 32.0 * (3.0 + rng.next_float());
                let (dx, dz) = ((x as f32 + 0.5) as f64 - px, (z as f32 + 0.5) as f64 - pz);
                let d = ((dx * dx + dz * dz).sqrt()) as f32 / rf;
                let l = (i.brightness)(x, 128, z) * 0.85 + 0.15;
                column(&mut rain, x, z, y0, y1, 0.0, 1.0, |y| y / 4.0 + scroll, [l, l, l, ((1.0 - d * d) * 0.5 + 0.5) * i.rain]);
            }
        }
    }
    (snow, rain)
}

/// `get_rain_color` / `get_snow_color` (PROGRAM_WEATHER) from the sun's height.
pub fn colors(sun_y: f32) -> ([f32; 3], [f32; 3]) {
    let t = ((sun_y + 0.1) / 0.6).clamp(0.0, 1.0);
    let s = t * t * (3.0 - 2.0 * t); // smoothstep(-0.1, 0.5, sun_dir.y)
    let tint = Vec3::new(0.49, 0.65, 1.0);
    let rain = (0.033 + (0.66 - 0.033) * s) * SUNLIGHT * tint;
    let snow = (0.5 + (1.6 - 0.5) * s) * SUNLIGHT * tint;
    (rain.to_array(), snow.to_array())
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct U {
    vp: [[f32; 4]; 4],
    eye: [f32; 4],
    rain_c: [f32; 4],
    snow_c: [f32; 4],
}

const SHADER: &str = r#"
struct U { vp: mat4x4<f32>, eye: vec4<f32>, rain_c: vec4<f32>, snow_c: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var tex: texture_2d<f32>;

struct VOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32>, @location(1) tint: vec4<f32> };

@vertex
fn vs_weather(@location(0) p: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) tint: vec4<f32>) -> VOut {
    // SLANTED_RAIN: scene_pos.xz -= offset * wave * scene_pos.y, in world space (the camera matrix already holds the eye).
    let tilt = 0.25 * vec2<f32>(cos(0.5235987756), sin(0.5235987756));
    let wave = 0.7 + 0.3 * sin(dot(p, vec3<f32>(5.0)));
    let dy = p.y - u.eye.y;
    let w = vec3<f32>(p.x - tilt.x * wave * dy, p.y, p.z - tilt.y * wave * dy);
    var o: VOut;
    o.pos = u.vp * vec4<f32>(w, 1.0);
    o.uv = uv;
    o.tint = tint;
    return o;
}

@fragment
fn fs_weather(in: VOut) -> @location(0) vec4<f32> {
    let base = textureSample(tex, samp, in.uv);
    if base.a < 0.1 { discard; }
    let is_rain = abs(base.r - base.b) > 1e-6;
    var c = select(
        vec4<f32>(u.snow_c.rgb, $SNOW$ * base.a) * in.tint,
        vec4<f32>(u.rain_c.rgb, $RAIN$ * base.a) * in.tint,
        is_rain);
    c = vec4<f32>(c.rgb * c.a, c.a);
    return c;
}
"#;

pub struct Weather {
    pipeline: RenderPipeline,
    ubuf: Buffer,
    vbuf: Buffer,
    binds: [BindGroup; 2], // snow, rain
    counts: [u32; 2],      // snow, rain
}

fn upload(device: &Device, queue: &Queue, bytes: &[u8], w: u32, h: u32) -> TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("weather_tex"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        bytes,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    tex.create_view(&wgpu::TextureViewDescriptor::default())
}

impl Weather {
    /// `format` = the scene colour target the pass blends into; the pass's depth attachment is the game's `Depth32Float`, read-only.
    pub fn new(device: &Device, queue: &Queue, format: wgpu::TextureFormat) -> Self {
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("weather_u"), size: std::mem::size_of::<U>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let vbuf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("weather_v"), size: (2 * MAX_VERTS * std::mem::size_of::<Vertex>()) as u64, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let entry = |binding, visibility, ty| wgpu::BindGroupLayoutEntry { binding, visibility, ty, count: None };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("weather_layout"),
            entries: &[
                entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }),
                entry(1, wgpu::ShaderStages::FRAGMENT, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
                entry(2, wgpu::ShaderStages::FRAGMENT, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }),
            ],
        });
        // GL_NEAREST + GL_REPEAT, like `RenderEngine` for these pngs.
        let samp = device.create_sampler(&wgpu::SamplerDescriptor { address_mode_u: wgpu::AddressMode::Repeat, address_mode_v: wgpu::AddressMode::Repeat, mag_filter: wgpu::FilterMode::Nearest, min_filter: wgpu::FilterMode::Nearest, ..Default::default() });
        let src = SHADER.replace("$SNOW$", &format!("{SNOW_OPACITY:?}")).replace("$RAIN$", &format!("{RAIN_OPACITY:?}"));
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("weather_shader"), source: wgpu::ShaderSource::Wgsl(src.into()) });
        // premultiplied: dst * (1 - a) + rgb
        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("weather"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("weather"), bind_group_layouts: &[&layout], push_constant_ranges: &[] })),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_weather"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 12, shader_location: 1 },
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 20, shader_location: 2 },
                    ],
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs_weather"), targets: &[Some(wgpu::ColorTargetState { format, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })], compilation_options: wgpu::PipelineCompilationOptions::default() }),
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() }, // glDisable(GL_CULL_FACE)
            depth_stencil: Some(wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth32Float, depth_write_enabled: false, depth_compare: wgpu::CompareFunction::Less, stencil: wgpu::StencilState::default(), bias: wgpu::DepthBiasState::default() }),
            multisample: crate::render::present::multisample(false),
            multiview: None,
            cache: None,
        });
        let make = |t: &TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("weather_bind"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&samp) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(t) },
                ],
            })
        };
        let snow_tex = upload(device, queue, include_bytes!("../../assets/snow.rgba"), 64, 256);
        let rain_tex = upload(device, queue, include_bytes!("../../assets/rain.rgba"), 64, 256);
        let binds = [make(&snow_tex), make(&rain_tex)];
        Self { pipeline, ubuf, vbuf, binds, counts: [0; 2] }
    }

    /// Per frame, before the pass: geometry and uniforms. `vp` is the camera's view-projection in world space.
    pub fn update(&mut self, queue: &Queue, vp: Mat4, eye: Vec3, sun_y: f32, snow: &[Vertex], rain: &[Vertex]) {
        let (rc, sc) = colors(sun_y);
        let u = U { vp: vp.to_cols_array_2d(), eye: eye.extend(0.0).to_array(), rain_c: [rc[0], rc[1], rc[2], 0.0], snow_c: [sc[0], sc[1], sc[2], 0.0] };
        queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&u));
        let (snow, rain) = (&snow[..snow.len().min(MAX_VERTS)], &rain[..rain.len().min(MAX_VERTS)]);
        queue.write_buffer(&self.vbuf, 0, bytemuck::cast_slice(snow));
        queue.write_buffer(&self.vbuf, (MAX_VERTS * std::mem::size_of::<Vertex>()) as u64, bytemuck::cast_slice(rain));
        self.counts = [snow.len() as u32, rain.len() as u32];
    }

    /// Inside a pass over the scene, with the terrain's depth attached read-only.
    pub fn draw<'a>(&'a self, rp: &mut RenderPass<'a>) {
        if self.counts == [0, 0] {
            return;
        }
        rp.set_pipeline(&self.pipeline);
        rp.set_vertex_buffer(0, self.vbuf.slice(..));
        let base = MAX_VERTS as u32;
        if self.counts[0] > 0 {
            rp.set_bind_group(0, &self.binds[0], &[]);
            rp.draw(0..self.counts[0], 0..1); // snow first, like the Java
        }
        if self.counts[1] > 0 {
            rp.set_bind_group(0, &self.binds[1], &[]);
            rp.draw(base..base + self.counts[1], 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one check: a flat rainy plain draws a crossed pair of quads (12 vertices) in every column of the (2r+1)^2 square whose
    /// span is not empty, none of them snow; and `nextGaussian` matches the JDK (`new Random(42).nextGaussian()`, from memory of
    /// the JDK docs/runs, not run here).
    #[test]
    fn rain_columns_and_gaussian() {
        let n = ((2 * RADIUS + 1) * (2 * RADIUS + 1)) as usize;
        let biomes = vec![Biome::Plains; n];
        let (top, light) = (|_: i32, _: i32| 60, |_: i32, _: i32, _: i32| 1.0);
        let i = Input { eye: Vec3::new(0.5, 64.0, 0.5), ticks: 7, partial: 0.5, rain: 1.0, biomes: &biomes, top_solid: &top, brightness: &light };
        let (snow, rain) = build(&i);
        assert!(snow.is_empty());
        assert_eq!(rain.len(), n * 12);
        assert!(rain.iter().all(|v| v.tint[3] > 0.0 && v.tint[3] <= 1.0));
        let mut r = JavaRandom::new(42);
        assert!((r.next_gaussian() - 1.1419053154730547).abs() < 1e-12);
    }
}
