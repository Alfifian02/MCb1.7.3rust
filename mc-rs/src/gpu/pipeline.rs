//! Render pipeline for chunk meshes + atlas texture.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::{
    BindGroup, BindGroupLayout, Buffer, BufferUsages, Device, Queue, RenderPipeline,
    Texture, TextureView,
};
use wgpu::util::DeviceExt;

use crate::render::{atlas, camera::shadow_view_proj};
use crate::world::chunk::is_plant;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub light: f32,
    /// `sky * 16 + block` light levels, see `render::mesh::lm`.
    pub lm: f32,
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
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 24, shader_location: 3 },
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
    /// Bit `t` set: atlas tile `t` is foliage and casts no shadow (`EXCLUDE_FOLIAGE`, `block.properties`).
    pub plant: [[u32; 4]; 2],
    /// rgb: fog colour, w: fog distance (the render distance in blocks; 0 = no fog). Written by `set_fog`.
    pub fog: [f32; 4],
    /// x: rain strength.
    pub fogp: [f32; 4],
    /// rgb: photon `light_color`, w: exposure. Written by `set_lighting`.
    pub lcol: [f32; 4],
    /// rgb: `ambient_color`, w: `time_noon`.
    pub amb: [f32; 4],
    /// rgb: `blocklight_color`.
    pub blk: [f32; 4],
}


/// `shadowMapResolution` (photon's is 2048 over twice the distance: the same texels per block, see `render::shadows`).
pub const SHADOW_RES: u32 = 1024;

const PLANT: [[u32; 4]; 2] = {
    let (mut m, mut id) = ([[0u32; 4]; 2], 0usize);
    while id < 256 {
        // Every terrain tile a plant can show (tall grass has three by metadata, the sapling a species per metadata).
        if is_plant(id as u8) {
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
    /// Translucent water (`fs_water`), drawn after the opaque terrain and the entities.
    pub water_pipeline: RenderPipeline,
    pub uniform_buf: Buffer,
    pub atlas_tex: Texture,
    pub atlas_view: TextureView,
    pub bind_group: BindGroup,
    pub bind_layout: BindGroupLayout,
    /// Sun depth pass (`shadow.vsh`), its target, and the group 1 the main pass reads it through.
    pub shadow_pipeline: RenderPipeline,
    pub shadow_view: TextureView,
    /// Same box and matrix as `shadow_view`, but holds only the moving things (items, mobs, falling blocks) and is redrawn every frame.
    pub ent_view: TextureView,
    pub shadow_bind: BindGroup,
    /// Layout of `shadow_bind`, for the light-shaft pass (`render::vl`).
    pub shadow_layout: BindGroupLayout,
}

const SHADER_SRC: &str = r#"
const LEGACY: bool = @LEGACY@; // `lib.rs` LEGACY: Console-Edition lighting (smooth light + ambient occlusion in the G-buffer)
const WATER_LIGHT: f32 = @WL@; // `mesh::WATER_LIGHT`: the water depth rides in the integer part of `light`
// (`photon::WGSL_COMMON` comes first: the uniform block `u`, colour spaces, fog and `photon_light`)
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var tex: texture_2d<f32>;
@group(1) @binding(0) var sh_tex: texture_depth_2d;
@group(1) @binding(1) var sh_samp: sampler_comparison;

@DISTORT@

const TW: u32 = @TW@u;
const TH: f32 = @TH@.0;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) light: f32,
    @location(2) wp: vec3<f32>,
    @location(3) lm: f32,
    @location(4) sk: f32, // sky light in quarter levels, block light in half levels, shade * ambient occlusion (-1: flat face, shade from its normal)
    @location(5) bk: f32,
    @location(6) ao: f32,
};

@vertex
fn vs_main(@location(0) vpos: vec3<f32>, @location(1) vuv: vec2<f32>, @location(2) vlight: f32, @location(3) vlm: f32) -> VsOut {
    var o: VsOut;
    o.pos = u.proj * u.view * vec4<f32>(vpos, 1.0);
    o.uv = vuv;
    o.light = vlight;
    o.wp = vpos;
    o.lm = vlm;
    if vlm >= 256.0 { // smooth vertex (`mesh::SMOOTH_LM`)
        let v = vlm - 256.0;
        o.sk = floor(v / 32.0 + 0.001);
        o.bk = v - o.sk * 32.0;
        o.ao = vlight;
    } else { // flat: `sky * 16 + block`
        let s = floor(vlm / 16.0 + 0.001);
        o.sk = s * 4.0;
        o.bk = (vlm - s * 16.0) * 2.0;
        o.ao = -1.0;
    }
    return o;
}

// shadow.vsh: foliage is parked outside the clip volume; everything else is drawn from the sun with photon's
// `distort_shadow_space`. `shadow_vp` has depth 0..1 and the distortion works on GL clip space (-1..1), so z goes
// there and back: the stored depth is `z_gl * SHADOW_DEPTH_SCALE * 0.5 + 0.5`.
@vertex
fn vs_shadow(@location(0) p: vec3<f32>, @location(1) uv: vec2<f32>) -> @builtin(position) vec4<f32> {
    // Terrain tile under this UV (the strip below the png gives 256 and up: clamped to the unused tile 255).
    let t = min(u32(uv.y * TH) * TW + u32(uv.x * f32(TW)), 255u);
    if ((u.plant[t >> 7u][(t >> 5u) & 3u] >> (t & 31u)) & 1u) == 1u { return vec4<f32>(10.0); }
    let c = u.shadow_vp * vec4<f32>(p, 1.0);
    let d = distort_shadow_space(vec3<f32>(c.xy, c.z * 2.0 - 1.0));
    return vec4<f32>(d.xy, d.z * 0.5 + 0.5, 1.0);
}

// G-buffer: albedo (linear, Rec. 709) and `face * 256 + sky * 16 + block`; `photon::SHADER` `fs_deferred` lights it.
@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let dx = dpdx(in.wp); // derivatives stay in uniform control flow; `face_index` below can then sit behind a per-pixel branch
    let dy = dpdy(in.wp);
    let c = textureSample(tex, samp, in.uv);
    if c.a < 0.5 { discard; } // cut-out textures: plants, glass
    if LEGACY {
        // Face shade and ambient occlusion x the sky/block light (`photon_light`'s legacy branch) x the fog.
        // Smooth faces carry shade * ao already (`in.ao >= 0`); only the flat ones need their face from the derivatives.
        var shade = in.ao;
        if in.ao < 0.0 {
            let face = face_index(in.wp, dx, dy);
            shade = 0.8;
            if face == 2u { shade = 1.0; } else if face == 3u { shade = 0.5; } else if face < 2u { shade = 0.6; }
        }
        // Lit right here (forward), straight into the scene: no G-buffer, no depth copy, no full-screen pass that reads them back.
        // Same maths the deferred pass did (`photon_light` legacy branch, face 6 = shade already in the albedo, then the fog).
        let albedo = to_working(c.rgb * pow(max(shade, 0.0), 2.2));
        let sky = clamp(floor(in.sk + 0.5), 0.0, 60.0) / 60.0;
        let bl = clamp(floor(in.bk + 0.5), 0.0, 30.0) / 30.0;
        return vec4<f32>(apply_fog(photon_light(albedo, 6u, sky, bl, 0.0), length(in.wp - u.eye.xyz)), 1.0);
    }
    return vec4<f32>(c.rgb, f32(face_index(in.wp, dx, dy)) * 256.0 + in.lm);
}

// Translucent water, forward lit with the same `photon_light` (no shadow). `mesh::build_split` packs the water column under
// the face (whole cells) into the integer part of `light`. The texture's alpha (0.54) is the floor: deeper water absorbs, so
// it turns opaque (1 - e^(-0.35 d): 0.3 at 1 block, 0.65 at 3, 0.88 at 6) and darker and bluer, and a grazing view reflects
// more. Shallow water still shows its bed. UNVERIFIED: the constants are tuned by eye, not taken from a source.
@fragment
fn fs_water(in: VsOut) -> @location(0) vec4<f32> {
    let depth = floor(in.light + 0.05); // +0.05: interpolation may land a hair under the integer
    let face = face_index(in.wp, dpdx(in.wp), dpdy(in.wp));
    let c = textureSample(tex, samp, in.uv);
    let absorb = 1.0 - exp(-0.35 * depth);
    let graze = pow(1.0 - abs(normalize(u.eye.xyz - in.wp).y), 3.0);
    let lm = in.lm + 0.01;
    let sky_i = floor(lm / 16.0);
    let albedo = to_working(c.rgb * mix(vec3<f32>(1.0), vec3<f32>(0.45, 0.62, 0.8), absorb));
    var rgb = photon_light(albedo, face, sky_i / 15.0, floor(lm - sky_i * 16.0) / 15.0, 1.0);
    rgb = apply_fog(rgb, length(in.wp - u.eye.xyz));
    return vec4<f32>(rgb, max(c.a, mix(absorb, 1.0, 0.5 * graze)));
}
"#;

/// The chunk shader with its constants spliced in (`legacy` = `lib.rs` LEGACY); a test validates both looks.
fn shader_source(legacy: bool) -> String {
    format!("{}{}", crate::render::photon::WGSL_COMMON, SHADER_SRC)
        .replace("@DISTORT@", &crate::render::shadows::distortion_wgsl())
        .replace("@TW@", &atlas::TILES_W.to_string())
        .replace("@TH@", &(atlas::ATLAS_H / 16).to_string())
        .replace("@LEGACY@", if legacy { "true" } else { "false" })
        .replace("@WL@", &format!("{:?}", crate::render::mesh::WATER_LIGHT))
}

impl ChunkPipeline {
    pub fn new(device: &Device, _surface_format: wgpu::TextureFormat) -> Self {
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
            source: wgpu::ShaderSource::Wgsl(shader_source(crate::LEGACY).into()),
        });

        // Group 1: the shadow map and its comparison sampler: linear = `shadowHardwareFiltering1` (a bilinear compare).
        let mut shadow_desc = wgpu::TextureDescriptor {
            label: Some("shadow_map"),
            size: wgpu::Extent3d { width: SHADOW_RES, height: SHADOW_RES, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };
        let shadow_view = device.create_texture(&shadow_desc).create_view(&wgpu::TextureViewDescriptor::default());
        shadow_desc.label = Some("entity_shadow_map");
        let ent_view = device.create_texture(&shadow_desc).create_view(&wgpu::TextureViewDescriptor::default());
        let shadow_samp = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
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
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&ent_view) },
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
            // Back-face culling: the mesher drops hidden faces, so the nearest face to the sun is always a sun-facing one and the
            // depth map is the same, for half the rasterised triangles. (distort() keeps the winding.)
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
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
                    format: crate::render::photon::HDR_FORMAT,
                    // None, not REPLACE: REPLACE still enables the blend unit (src * 1 + dst * 0), which reads the old framebuffer for every
                    // opaque pixel; a non-finite green left in a tile turned the terrain magenta (Inf * 0 = NaN, zeroed by `sane`).
                    blend: None,
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

        // Water: the same vertices and shader, blended, no depth write (so what is behind it is still drawn and seen), both faces
        // (the surface is seen from below). ponytail: chunks are not sorted back to front; two water chunks in a line blend in map order.
        let water_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("water_pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Vertex::layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_water"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: crate::render::photon::HDR_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self { pipeline, water_pipeline, uniform_buf, atlas_tex, atlas_view, bind_group, bind_layout, shadow_pipeline, shadow_view, ent_view, shadow_bind, shadow_layout }
    }

    /// Terrain fog for this frame: `colour` rgb, `far` blocks (0 = off, e.g. under water), `rain` 0..1. Call after `upload_uniforms`.
    pub fn set_fog(&self, queue: &Queue, colour: [f32; 3], far: f32, rain: f32) {
        let f = [[colour[0], colour[1], colour[2], far], [rain, 0.0, 0.0, 0.0]];
        queue.write_buffer(&self.uniform_buf, std::mem::offset_of!(Uniforms, fog) as u64, bytemuck::bytes_of(&f));
    }

    /// Photon colours for this frame (`render::photon::lighting`). Call after `upload_uniforms`.
    pub fn set_lighting(&self, queue: &Queue, l: &crate::render::photon::Lighting) {
        let v = [
            [l.light_color[0], l.light_color[1], l.light_color[2], l.exposure],
            [l.ambient[0], l.ambient[1], l.ambient[2], l.noon],
            [l.blocklight[0], l.blocklight[1], l.blocklight[2], l.legacy], // w: 1 = Console-Edition lighting (`photon_light`)
        ];
        queue.write_buffer(&self.uniform_buf, std::mem::offset_of!(Uniforms, lcol) as u64, bytemuck::bytes_of(&v));
    }

    /// Returns the sun's view-projection (for culling the shadow pass). `strength` 0 turns shadows off. `shadow_center` and
    /// `sun` are those of the last shadow-map render (`App.shadow`), so the lookup matches the map even when it is reused.
    pub fn upload_uniforms(&self, queue: &Queue, view: Mat4, proj: Mat4, sun: Vec3, strength: f32, shadow_center: Vec3) -> Mat4 {
        let eye = view.inverse().w_axis.truncate();
        let vp = shadow_view_proj(shadow_center, sun);
        let u = Uniforms {
            view: view.to_cols_array_2d(),
            proj: proj.to_cols_array_2d(),
            shadow_vp: vp.to_cols_array_2d(),
            sun: sun.extend(strength).to_array(),
            eye: eye.extend(0.0).to_array(),
            plant: PLANT,
            fog: [0.0; 4], // off until `set_fog`
            fogp: [0.0; 4],
            lcol: [0.0, 0.0, 0.0, 1.0], // dark until `set_lighting`
            amb: [0.0; 4],
            blk: [0.0; 4],
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Both looks parse and validate, and the legacy `fs_main` lights and fogs in place (it no longer writes a G-buffer texel).
    #[test]
    fn chunk_shader_validates_and_legacy_is_forward() {
        for legacy in [false, true] {
            let src = shader_source(legacy);
            let m = naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&src)));
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&m).unwrap();
        }
        assert!(SHADER_SRC.contains("apply_fog(photon_light(albedo, 6u, sky, bl, 0.0)"), "legacy terrain is lit in `fs_main`");
    }
}
