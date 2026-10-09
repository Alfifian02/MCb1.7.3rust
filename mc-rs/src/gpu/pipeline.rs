//! Render pipeline for chunk meshes + atlas texture.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::{
    BindGroup, BindGroupLayout, Buffer, BufferUsages, Device, Queue, RenderPipeline,
    Texture, TextureView,
};
use wgpu::util::DeviceExt;

use crate::render::{atlas, camera::{shadow_view_proj, SHADOW_RADIUS}};
use crate::world::chunk::is_plant;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub light: f32,
}

impl Vertex {
    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 12, shader_location: 1 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 20, shader_location: 2 },
        ],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct Uniforms {
    pub view: [[f32; 4]; 4],
    pub proj: [[f32; 4]; 4],
    pub shadow_vp: [[f32; 4]; 4],
    /// xyz: direction toward the sun, w: shadow strength 0..1 (0 = no shadow pass this frame).
    pub sun: [f32; 4],
    pub eye: [f32; 4],
    /// x: distort factor, y: bias in blocks per undistorted texel, z: shadow brightness.
    pub sh: [f32; 4],
    /// Bit `t` set: atlas tile `t` is foliage and casts no shadow (`EXCLUDE_FOLIAGE`, `block.properties`).
    pub plant: [[u32; 4]; 2],
}

// Shadow-Tutorial `distort.glsl` constants.
pub const SHADOW_RES: u32 = 1024; // shadowMapResolution
const SHADOW_DISTORT: f32 = 0.10; // SHADOW_DISTORT_FACTOR
const SHADOW_BIAS: f32 = 1.0;
const SHADOW_BRIGHTNESS: f32 = 0.75;

const PLANT: [[u32; 4]; 2] = {
    let (mut m, mut id) = ([[0u32; 4]; 2], 0usize);
    while id < 256 {
        // Every terrain tile a plant can show (tall grass has three by metadata). 78 (snow layer) is not drawn and shares
        // its tile with snow blocks, which do cast shadows.
        if is_plant(id as u8) && id != 78 {
            let mut meta = 0;
            while meta < 3 {
                if let Some(t) = atlas::terrain_tile(id as u8, meta, 2) { m[(t >> 7) as usize][((t >> 5) & 3) as usize] |= 1 << (t & 31); }
                meta += 1;
            }
        }
        id += 1;
    }
    m
};

pub struct ChunkPipeline {
    pub pipeline: RenderPipeline,
    pub uniform_buf: Buffer,
    pub atlas_tex: Texture,
    pub atlas_view: TextureView,
    pub bind_group: BindGroup,
    pub bind_layout: BindGroupLayout,
    /// Sun depth pass (`shadow.vsh`), its target, and the group 1 the main pass reads it through.
    pub shadow_pipeline: RenderPipeline,
    pub shadow_view: TextureView,
    pub shadow_bind: BindGroup,
}

const SHADER_SRC: &str = r#"
struct Uniforms {
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    shadow_vp: mat4x4<f32>,
    sun: vec4<f32>,
    eye: vec4<f32>,
    sh: vec4<f32>,
    plant: array<vec4<u32>, 2>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var tex: texture_2d<f32>;
@group(1) @binding(0) var sh_tex: texture_depth_2d;
@group(1) @binding(1) var sh_samp: sampler_comparison;

const TW: u32 = @TW@u;
const TH: f32 = @TH@.0;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) light: f32,
    @location(2) wp: vec3<f32>,
};

@vertex
fn vs_main(@location(0) vpos: vec3<f32>, @location(1) vuv: vec2<f32>, @location(2) vlight: f32) -> VsOut {
    var o: VsOut;
    o.pos = u.proj * u.view * vec4<f32>(vpos, 1.0);
    o.uv = vuv;
    o.light = vlight;
    o.wp = vpos;
    return o;
}

// shadow.vsh: foliage is parked outside the clip volume; everything else is drawn from the sun with `distort()`
// applied. The shadow map's depth is `z * 0.5 + 0.25`: distort.glsl's `z * 0.5` in GL clip space, then 0..1.
@vertex
fn vs_shadow(@location(0) p: vec3<f32>, @location(1) uv: vec2<f32>) -> @builtin(position) vec4<f32> {
    // Terrain tile under this UV (the strip below the png gives 256 and up: clamped to the unused tile 255).
    let t = min(u32(uv.y * TH) * TW + u32(uv.x * f32(TW)), 255u);
    if ((u.plant[t >> 7u][(t >> 5u) & 3u] >> (t & 31u)) & 1u) == 1u { return vec4<f32>(10.0); }
    let c = u.shadow_vp * vec4<f32>(p, 1.0);
    return vec4<f32>(c.xy / (length(c.xy) + u.sh.x), c.z * 0.5 + 0.25, 1.0);
}

// gbuffers_terrain.fsh, per pixel instead of per vertex (there is no normal attribute: a flat face's normal is the
// cross of the position derivatives, turned toward the eye).
@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(tex, samp, in.uv);
    if c.a < 0.5 { discard; } // cut-out textures: plants, glass
    var n = normalize(cross(dpdx(in.wp), dpdy(in.wp)));
    if dot(n, u.eye.xyz - in.wp) < 0.0 { n = -n; }
    var k = 1.0;
    if u.sun.w > 0.0 {
        k = u.sh.z; // facing away from the sun, or in shadow
        let ndl = dot(n, u.sun.xyz);
        if ndl > 0.0 {
            let c0 = u.shadow_vp * vec4<f32>(in.wp, 1.0);
            let f = length(c0.xy) + u.sh.x;
            // computeBias + NORMAL_BIAS, in blocks: the world size of a shadow texel here (distortion stretches it by
            // f^2 / K), pushed along the surface normal. (distort.glsl's own offset is 1/R of this, too small to stop acne.)
            // Acne guard: near the eye the texel is ~1 cm, far less than the depth slope across it (and than the error of the
            // per-vertex distortion), so the offset has a floor that grows as the sun grazes the surface (tan of the angle, <= 4).
            let tn = min(sqrt(1.0 - ndl * ndl) / ndl, 4.0);
            let bias = max(u.sh.y * f * f / u.sh.x, 0.06 * (1.0 + tn));
            let c1 = u.shadow_vp * vec4<f32>(in.wp + n * bias, 1.0);
            let p = vec3<f32>(c1.xy / (length(c1.xy) + u.sh.x) * 0.5 + 0.5, c1.z * 0.5 + 0.25);
            let lit = textureSampleCompareLevel(sh_tex, sh_samp, vec2<f32>(p.x, 1.0 - p.y), p.z);
            k = mix(k, mix(u.sh.z, 1.0, sqrt(ndl)), lit);
        }
        k = mix(1.0, k, u.sun.w);
    }
    return vec4<f32>(c.rgb * in.light * k, c.a);
}
"#;

impl ChunkPipeline {
    pub fn new(device: &Device, surface_format: wgpu::TextureFormat) -> Self {
        // Atlas: `terrain.png` plus the flat-colour strip, `ATLAS_W` x `ATLAS_H` (see `render::atlas`).
        let atlas_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("atlas"),
            size: wgpu::Extent3d { width: atlas::ATLAS_W as u32, height: atlas::ATLAS_H as u32, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let atlas_view = atlas_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniform"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("chunk_bind_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
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

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("chunk_bind"),
            layout: &bind_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&atlas_view) },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("chunk_shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.replace("@TW@", &atlas::TILES_W.to_string()).replace("@TH@", &(atlas::ATLAS_H / 16).to_string()).into()),
        });

        // Group 1: the shadow map and its comparison sampler (nearest = `shadowtex0Nearest`; `Linear` is free PCF).
        let shadow_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow_map"),
            size: wgpu::Extent3d { width: SHADOW_RES, height: SHADOW_RES, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_view = shadow_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let shadow_samp = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            compare: Some(wgpu::CompareFunction::LessEqual), // lit when ref <= stored, i.e. not `stored < ref`
            ..Default::default()
        });
        let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow_bind_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        let shadow_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow_bind"),
            layout: &shadow_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&shadow_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&shadow_samp) },
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("chunk_pipeline_layout"),
            bind_group_layouts: &[&bind_layout, &shadow_layout],
            push_constant_ranges: &[],
        });
        // The shadow pass only needs group 0 (it cannot bind the map it is writing).
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow_pipeline"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("shadow_pipeline_layout"),
                bind_group_layouts: &[&bind_layout],
                push_constant_ranges: &[],
            })),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_shadow"),
                buffers: &[Vertex::layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: None,
            // ponytail: no culling (Back would skip the faces turned from the sun, a cheap speed-up once measured).
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("chunk_pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Vertex::layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self { pipeline, uniform_buf, atlas_tex, atlas_view, bind_group, bind_layout, shadow_pipeline, shadow_view, shadow_bind }
    }

    /// Returns the sun's view-projection (for culling the shadow pass). `strength` 0 turns shadows off.
    pub fn upload_uniforms(&self, queue: &Queue, view: Mat4, proj: Mat4, sun: Vec3, strength: f32) -> Mat4 {
        let eye = view.inverse().w_axis.truncate();
        let vp = shadow_view_proj(eye, sun);
        let u = Uniforms {
            view: view.to_cols_array_2d(),
            proj: proj.to_cols_array_2d(),
            shadow_vp: vp.to_cols_array_2d(),
            sun: sun.extend(strength).to_array(),
            eye: eye.extend(0.0).to_array(),
            sh: [SHADOW_DISTORT, SHADOW_BIAS * 2.0 * SHADOW_RADIUS / SHADOW_RES as f32, SHADOW_BRIGHTNESS, 0.0],
            plant: PLANT,
        };
        queue.write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&u));
        vp
    }

    pub fn upload_atlas(&self, queue: &Queue) {
        let rgba = atlas::atlas_rgba();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.atlas_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(atlas::ATLAS_W as u32 * 4),
                rows_per_image: Some(atlas::ATLAS_H as u32),
            },
            wgpu::Extent3d { width: atlas::ATLAS_W as u32, height: atlas::ATLAS_H as u32, depth_or_array_layers: 1 },
        );
    }
}

#[cfg(test)]
mod shadow_test;

pub fn create_vertex_buffer(device: &Device, verts: &[Vertex]) -> Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("chunk_vbuf"),
        contents: bytemuck::cast_slice(verts),
        usage: BufferUsages::VERTEX,
    })
}

pub fn create_index_buffer(device: &Device, idx: &[u32]) -> Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("chunk_ibuf"),
        contents: bytemuck::cast_slice(idx),
        usage: BufferUsages::INDEX,
    })
}
