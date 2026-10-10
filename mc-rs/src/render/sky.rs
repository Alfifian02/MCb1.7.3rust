//! Sky pass, a port of `RenderGlobal.renderSky`: the sky dome, the sunrise glow, the sun and moon, the stars and the
//! dark plane under the horizon, in the vanilla order. It is drawn first into the frame, without writing depth, around
//! the viewer (the view matrix keeps only the camera rotation), so the terrain simply paints over it.
//! The sun and moon are the real `terrain/sun.png` and `terrain/moon.png` packed into `assets/sky.rgba`
//! (64x64 RGBA: sun at the top left, moon at the top right, a white tile at the bottom left for flat colours).
//! Dome and under-plane use the fog of `EntityRenderer.setupFog(-1)`: linear from 0 to 0.8 x the far plane,
//! by distance from the eye, towards the fog colour. Vanilla's cloud layer and rain/snow are not drawn (see ROADMAP).

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};
use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::ops::Range;
use wgpu::util::DeviceExt;

use crate::render::camera::FirstPersonCamera;
use crate::world::gen::noise::{mh_cos, mh_sin};
use crate::world::sky;

const ATLAS: &[u8] = include_bytes!("../../assets/sky.rgba");
const ATLAS_SIZE: u32 = 64;
/// Far plane of vanilla's NORMAL view distance (`256 >> 1`); the sky projection reaches twice that, like `gluPerspective`.
const FAR: f32 = 128.0;
/// Centre of the all-white tile in the atlas.
const WHITE: [f32; 2] = [0.25, 0.75];
/// Room for the dome (6), glow (48), glare (6), sun + moon (12) and under plane (6) vertices.
const MAX_VERTS: usize = 128;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct Vertex {
    pos: [f32; 3],
    uv: [f32; 2],
    color: [f32; 4],
    /// 1 = fogged (dome, under plane), 0 = not.
    fog: f32,
}

impl Vertex {
    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 12, shader_location: 1 },
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 20, shader_location: 2 },
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 36, shader_location: 3 },
            ],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct Uniforms {
    view: [[f32; 4]; 4],
    proj: [[f32; 4]; 4],
    /// Fog colour (rgb).
    fog: [f32; 4],
    /// Fog start and end (x, y).
    range: [f32; 4],
    /// Multiplies every colour: white, or the star brightness.
    tint: [f32; 4],
    /// View-space direction toward the sun (xyz), for the glare.
    sunv: [f32; 4],
}

/// `SUN_GLARE_DAY * 0.1 * SUNGLARE_OUTWATER_STRENGTH * 0.01` of the pack is an HDR add; here the halo is an alpha blend
/// (an add would clip on the pale sky), so this is its own number. UNVERIFIED: on a device.
const GLARE: f32 = 1.2;
/// Half-size of the glare quad at the sun's distance (100): reaches ~42 degrees from the sun, where the glare alpha is < 2%
/// (it is drawn over the whole sky and then covered by terrain, so every pixel it spans costs a shaded fragment).
const GLARE_R: f32 = 90.0;
/// `lightColor.glsl` morning light (236, 184, 132) / 255.
const GLARE_RGB: [f32; 3] = [0.93, 0.72, 0.52];

const SHADER_SRC: &str = r#"
struct U { view: mat4x4<f32>, proj: mat4x4<f32>, fog: vec4<f32>, range: vec4<f32>, tint: vec4<f32>, sunv: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var tex: texture_2d<f32>;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) vpos: vec3<f32>,
    @location(3) fog: f32,
};

@vertex
fn vs_main(@location(0) p: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32>, @location(3) fog: f32) -> VsOut {
    var o: VsOut;
    let v = u.view * vec4<f32>(p, 1.0);
    o.pos = u.proj * v;
    o.uv = uv;
    o.color = color;
    o.vpos = v.xyz;
    o.fog = fog;
    return o;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    if (in.fog > 1.5) {
        // gbuffers glare, `sunGlare.glsl`: VoL^8, then visfactor / (1 - (1 - visfactor) * VoL) - visfactor, times VoL.
        let s = dot(normalize(in.vpos), u.sunv.xyz);
        if (s <= 0.0) { return vec4<f32>(0.0); }
        var v = s * s; v = v * v; v = v * v;
        let g = (0.2 / (1.0 - 0.8 * v) - 0.2) * s;
        return vec4<f32>(in.color.rgb, clamp(g * GLARE, 0.0, 1.0) * in.color.a);
    }
    var c = textureSample(tex, samp, in.uv) * in.color * u.tint;
    if (in.fog > 0.5) {
        let f = clamp((u.range.y - length(in.vpos)) / (u.range.y - u.range.x), 0.0, 1.0);
        c = vec4<f32>(mix(u.fog.rgb, c.rgb, f), c.a);
    }
    return c;
}
"#;

/// What the sky needs to know about this frame (all from `world::sky`).
pub struct SkyFrame {
    pub angle: f32,
    pub rain: f32,
    pub sky: [f32; 3],
    pub fog: [f32; 3],
}

/// One frame of dynamic geometry in draw order, as ranges into one vertex list (triangle lists).
struct Geometry {
    verts: Vec<Vertex>,
    dome: Range<u32>,
    glow: Range<u32>,
    glare: Range<u32>,
    bodies: Range<u32>,
    under: Range<u32>,
}

fn quad(v: &mut Vec<Vertex>, p: [[f32; 3]; 4], uv: [[f32; 2]; 4], color: [f32; 4], fog: f32) {
    for i in [0, 1, 2, 0, 2, 3] {
        v.push(Vertex { pos: p[i], uv: uv[i], color, fog });
    }
}

/// The sun or moon quad: corners `[x sign, z sign, u, v]` at height `y`, side `2 * s`, turned with the sky by `rot`.
fn body(v: &mut Vec<Vertex>, rot: Mat4, s: f32, y: f32, pts: [[f32; 4]; 4], color: [f32; 4], u0: f32) {
    let p = pts.map(|q| rot.transform_point3(Vec3::new(q[0] * s, y, q[1] * s)).to_array());
    let uv = pts.map(|q| [u0 + q[2] * 0.5, q[3] * 0.5]);
    quad(v, p, uv, color, 0.0);
}

fn geometry(f: &SkyFrame) -> Geometry {
    let mut v: Vec<Vertex> = Vec::with_capacity(MAX_VERTS);
    // glSkyList / glSkyList2: a flat plane 16 above / below the eye, 64 x 64 quads from -384 to 448 in vanilla.
    let (lo, hi) = (-384.0_f32, 448.0_f32);
    let plane = |y: f32| [[lo, y, lo], [hi, y, lo], [hi, y, hi], [lo, y, hi]];
    quad(&mut v, plane(16.0), [WHITE; 4], [f.sky[0], f.sky[1], f.sky[2], 1.0], 1.0);
    let dome = 0..v.len() as u32;

    // Sunrise / sunset glow: a fan standing on the horizon, opaque in the middle and clear at the rim.
    let start = v.len() as u32;
    if let Some(c) = sky::sunrise_color(f.angle) {
        let m = Mat4::from_rotation_x(FRAC_PI_2) * Mat4::from_rotation_z(if f.angle > 0.5 { PI } else { 0.0 });
        let at = |p: [f32; 3]| m.transform_point3(Vec3::from(p)).to_array();
        let centre = Vertex { pos: at([0.0, 100.0, 0.0]), uv: WHITE, color: c, fog: 0.0 };
        let rim = |i: u32| {
            let a = i as f32 * PI * 2.0 / 16.0;
            let (s, co) = (mh_sin(a), mh_cos(a));
            Vertex { pos: at([s * 120.0, co * 120.0, -co * 40.0 * c[3]]), uv: WHITE, color: [c[0], c[1], c[2], 0.0], fog: 0.0 }
        };
        for i in 0..16 {
            v.extend([centre, rim(i), rim(i + 1)]);
        }
    }
    let glow = start..v.len() as u32;

    // Sun glare: one big quad around the sun, shaded per pixel by the angle to it. `sunVisibility` fades it at the horizon
    // and the night, rain removes it (`1.1 - rainFactor`).
    let start = v.len() as u32;
    let rot = Mat4::from_rotation_x(f.angle * TAU);
    let vis = (rot.transform_vector3(Vec3::Y).y + 0.0625).clamp(0.0, 0.125) * 8.0 * (1.0 - f.rain);
    if vis > 0.0 {
        let p = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]].map(|c| rot.transform_point3(Vec3::new(c[0] * GLARE_R, 100.0, c[1] * GLARE_R)).to_array());
        quad(&mut v, p, [WHITE; 4], [GLARE_RGB[0], GLARE_RGB[1], GLARE_RGB[2], vis], 2.0);
    }
    let glare = start..v.len() as u32;

    // Sun (30 wide half-size) and moon (20), opposite each other, turned about the X axis by the sun angle.
    let start = v.len() as u32;
    let white = [1.0, 1.0, 1.0, 1.0 - f.rain];
    body(&mut v, rot, 30.0, 100.0, [[-1.0, -1.0, 0.0, 0.0], [1.0, -1.0, 1.0, 0.0], [1.0, 1.0, 1.0, 1.0], [-1.0, 1.0, 0.0, 1.0]], white, 0.0);
    body(&mut v, rot, 20.0, -100.0, [[-1.0, 1.0, 1.0, 1.0], [1.0, 1.0, 0.0, 1.0], [1.0, -1.0, 0.0, 0.0], [-1.0, -1.0, 1.0, 0.0]], white, 0.5);
    let bodies = start..v.len() as u32;

    // The stars are drawn here (own buffer), then the dark plane under the horizon.
    let start = v.len() as u32;
    quad(&mut v, plane(-16.0), [WHITE; 4], [f.sky[0] * 0.2 + 0.04, f.sky[1] * 0.2 + 0.04, f.sky[2] * 0.6 + 0.1, 1.0], 1.0);
    let under = start..v.len() as u32;
    Geometry { verts: v, dome, glow, glare, bodies, under }
}

pub struct SkyRenderer {
    opaque: wgpu::RenderPipeline,
    alpha: wgpu::RenderPipeline,
    additive: wgpu::RenderPipeline,
    main_uniforms: wgpu::Buffer,
    star_uniforms: wgpu::Buffer,
    main_bind: wgpu::BindGroup,
    star_bind: wgpu::BindGroup,
    dynamic: wgpu::Buffer,
    stars: wgpu::Buffer,
    star_count: u32,
    stars_on: bool,
    /// Dome, glow, sun + moon, under plane: set by `update`, read by `draw`.
    ranges: [Range<u32>; 5],
}

impl SkyRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, surface_format: wgpu::TextureFormat) -> Self {
        let size = wgpu::Extent3d { width: ATLAS_SIZE, height: ATLAS_SIZE, depth_or_array_layers: 1 };
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sky_atlas"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            ATLAS,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(ATLAS_SIZE * 4), rows_per_image: Some(ATLAS_SIZE) },
            size,
        );
        let tex_view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let uniforms = || device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sky_uniform"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (main_uniforms, star_uniforms) = (uniforms(), uniforms());

        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky_bind_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let bind = |buf: &wgpu::Buffer| device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky_bind"),
            layout: &bind_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&tex_view) },
            ],
        });
        let (main_bind, star_bind) = (bind(&main_uniforms), bind(&star_uniforms));

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("sky_shader"), source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()) });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("sky_pipeline_layout"), bind_group_layouts: &[&bind_layout], push_constant_ranges: &[] });
        // The frame pass has a depth buffer, so the pipelines declare one too: never tested, never written.
        let make = |label: &str, blend: wgpu::BlendState| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), buffers: &[Vertex::layout()], compilation_options: wgpu::PipelineCompilationOptions::default() },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState { format: surface_format, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        // GL_SRC_ALPHA, GL_ONE: what the sun, moon and stars are drawn with.
        let add = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add };
        let opaque = make("sky_opaque", wgpu::BlendState::REPLACE);
        let alpha = make("sky_alpha", wgpu::BlendState::ALPHA_BLENDING);
        let additive = make("sky_additive", wgpu::BlendState { color: add, alpha: add });

        let dynamic = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sky_dynamic"),
            size: (MAX_VERTS * std::mem::size_of::<Vertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut sv: Vec<Vertex> = Vec::new();
        for q in sky::star_vertices().chunks(4) {
            quad(&mut sv, [q[0], q[1], q[2], q[3]].map(|p| p.map(|c| c as f32)), [WHITE; 4], [1.0; 4], 0.0);
        }
        let stars = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("sky_stars"), contents: bytemuck::cast_slice(&sv), usage: wgpu::BufferUsages::VERTEX });

        Self { opaque, alpha, additive, main_uniforms, star_uniforms, main_bind, star_bind, dynamic, stars, star_count: sv.len() as u32, stars_on: false, ranges: [0..0, 0..0, 0..0, 0..0, 0..0] }
    }

    /// Upload this frame's matrices, colours and geometry.
    pub fn update(&mut self, queue: &wgpu::Queue, camera: &FirstPersonCamera, f: &SkyFrame) {
        let (mut view, _) = camera.build_view_proj();
        view.w_axis = Vec4::W; // the sky travels with the viewer: keep the rotation, drop the position
        let proj = Mat4::perspective_rh(camera.fov_y, camera.aspect, camera.znear, FAR * 2.0);
        let mut u = Uniforms {
            view: view.to_cols_array_2d(),
            proj: proj.to_cols_array_2d(),
            fog: [f.fog[0], f.fog[1], f.fog[2], 1.0],
            range: [0.0, FAR * 0.8, 0.0, 0.0],
            tint: [1.0; 4],
            sunv: view.transform_vector3(Mat4::from_rotation_x(f.angle * TAU).transform_vector3(Vec3::Y)).normalize().extend(0.0).to_array(),
        };
        queue.write_buffer(&self.main_uniforms, 0, bytemuck::bytes_of(&u));
        // glColor4f(b, b, b, b) with the additive blend: the stars fade in at dusk and out in rain.
        let b = sky::star_brightness(f.angle) * (1.0 - f.rain);
        u.tint = [b; 4];
        queue.write_buffer(&self.star_uniforms, 0, bytemuck::bytes_of(&u));
        self.stars_on = b > 0.0;
        let g = geometry(f);
        queue.write_buffer(&self.dynamic, 0, bytemuck::cast_slice(&g.verts));
        self.ranges = [g.dome, g.glow, g.glare, g.bodies, g.under];
    }

    /// Draw the sky into the frame pass, before any terrain.
    pub fn draw(&self, rp: &mut wgpu::RenderPass<'_>) {
        let [dome, glow, glare, bodies, under] = &self.ranges;
        rp.set_vertex_buffer(0, self.dynamic.slice(..));
        rp.set_bind_group(0, &self.main_bind, &[]);
        rp.set_pipeline(&self.opaque);
        rp.draw(dome.clone(), 0..1);
        if !glow.is_empty() {
            rp.set_pipeline(&self.alpha);
            rp.draw(glow.clone(), 0..1);
        }
        if !glare.is_empty() {
            rp.set_pipeline(&self.alpha);
            rp.draw(glare.clone(), 0..1);
        }
        rp.set_pipeline(&self.additive);
        rp.draw(bodies.clone(), 0..1);
        if self.stars_on {
            rp.set_bind_group(0, &self.star_bind, &[]);
            rp.set_vertex_buffer(0, self.stars.slice(..));
            rp.draw(0..self.star_count, 0..1);
            rp.set_bind_group(0, &self.main_bind, &[]);
            rp.set_vertex_buffer(0, self.dynamic.slice(..));
        }
        rp.set_pipeline(&self.opaque);
        rp.draw(under.clone(), 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mean(v: &[Vertex]) -> Vec3 {
        v.iter().map(|p| Vec3::from(p.pos)).sum::<Vec3>() / v.len() as f32
    }

    /// Noon: sun straight up, moon straight down; midnight: swapped. The glow only exists near the horizon,
    /// and everything fits the dynamic buffer.
    #[test]
    fn sun_and_moon_trade_places() {
        let frame = |angle| SkyFrame { angle, rain: 0.0, sky: [0.5; 3], fog: [0.5; 3] };
        let centres = |g: &Geometry| {
            let s = g.bodies.start as usize;
            (mean(&g.verts[s..s + 6]), mean(&g.verts[s + 6..g.bodies.end as usize]))
        };
        let noon = geometry(&frame(0.0));
        let (sun, moon) = centres(&noon);
        assert!((sun - Vec3::new(0.0, 100.0, 0.0)).length() < 1e-3 && (moon - Vec3::new(0.0, -100.0, 0.0)).length() < 1e-3);
        let (sun, moon) = centres(&geometry(&frame(0.5)));
        assert!(sun.y < -99.0 && moon.y > 99.0);
        let dusk = geometry(&frame(0.25));
        assert!(noon.glow.is_empty() && dusk.glow.len() == 48);
        assert_eq!((noon.dome.len(), noon.under.len(), noon.bodies.len()), (6, 6, 12));
        assert!(noon.glare.len() == 6 && geometry(&frame(0.5)).glare.is_empty()); // sun glare: day only
        assert!(dusk.verts.len() <= MAX_VERTS && ATLAS.len() == (ATLAS_SIZE * ATLAS_SIZE * 4) as usize);
    }
}
