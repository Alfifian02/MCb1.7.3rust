//! Sky, a port of Photon's `d0_sky_map` + `d4a_generate_sky_sh` and the sky branch of `d4_deferred_shading` (WGSL in
//! `sky.wgsl` / `sky_sh.wgsl`, Photon Shaders by SixthSurge, see `LICENSE-PHOTON`). Replaces the vanilla dome, sunrise
//! fan, sun/moon quads, glare and star quads of `RenderGlobal.renderSky`.
//!
//! Per frame, before the scene: `encode` renders the 192 x 108 sky map (the whole sky around the viewer from the
//! precomputed atmosphere LUT, `assets/scattering.dat`, with the light colours in column 191) and sums it into 9 SH
//! coefficients (`sh`). `draw` then paints the full-resolution sky (atmosphere, stars, sun, moon) over every pixel the
//! scene left at depth 1; it is the stand-in for d4's sky branch until the deferred pass exists (milestone 1), so it
//! tone-maps itself (see `stand_in_tonemap`; milestone 5 removes that).
//!
//! For d4 (milestone 1): `WGSL` is the library (paste it into the deferred module and call `draw_sky`,
//! `atmosphere_scattering`, `project_sky`, `sh_evaluate_irradiance`), `sky_map` the texture (rgba16f, light colour at
//! (191, 0), ambient colour at (191, 1)), `sh` the buffer (9 coefficients + irradiance up, `vec4` each), and
//! `uniform_buffer` the `Sky` block of group 0, `bind_group()` with its layout `layout`.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};
use std::f32::consts::TAU;
use wgpu::util::DeviceExt;

use crate::render::camera::FirstPersonCamera;
use crate::world::sky;

/// The library part of the shader (group 0 only).
pub const WGSL: &str = include_str!("sky.wgsl");
const SH_WGSL: &str = include_str!("sky_sh.wgsl");
const LUT_DAT: &[u8] = include_bytes!("../../assets/scattering.dat");
const NOISE: &[u8] = include_bytes!("../../assets/noise.rgba");
/// The sun and the full moon, 32 x 32 each side by side (`tools/gen_celestial.py`).
const CELESTIAL: &[u8] = include_bytes!("../../assets/celestial.rgba");
/// The LUT of `image/atmosphere/scattering.dat`: RGB16F, x = 2 x 16 (nu; Rayleigh half, Mie half), y = 64 (mu), z = 32 (mu_s).
const LUT_SIZE: (u32, u32, u32) = (32, 64, 32);
const NOISE_SIZE: u32 = 512;
pub const SKY_MAP: (u32, u32) = (192, 108);

/// What the sky needs to know about this frame.
pub struct SkyFrame {
    /// `world::sky::celestial_angle`: 0 = noon, 0.5 = midnight.
    pub angle: f32,
    pub rain: f32,
    /// Seconds of play, for the stars' twinkle (`frameTimeCounter`).
    pub seconds: f32,
}

/// The `Sky` block of `sky.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct Params {
    inv_vp: [[f32; 4]; 4],
    sun_dir: [f32; 4],
    moon_dir: [f32; 4],
    light_dir: [f32; 4],
    celestial: [[f32; 4]; 4],
    /// time_sunrise, time_noon, time_sunset, time_midnight
    times: [f32; 4],
    /// rainStrength, sunAngle, seconds, atmosphere_saturation_boost_amount
    misc: [f32; 4],
    /// legacy sky (`fs_legacy`): dome colour, fog colour; zero unless `set_legacy` wrote them after `update`.
    zenith: [f32; 4],
    horizon: [f32; 4],
    /// legacy sky: star colour, alpha = star brightness (`set_legacy`); then the view-projection without translation.
    stars: [f32; 4],
    vp: [[f32; 4]; 4],
}

/// The pack's Iris uniforms (`shaders.properties`) from mc-rs's sun angle. The pack's `sunAngle` is 0 at sunrise, ours 0 at noon;
/// our sun path lies in the YZ plane, so "east" (`sun_dir.x > 0`, the rising half) is `angle > 0.5` here.
pub fn params(f: &SkyFrame, camera: &FirstPersonCamera) -> Params {
    let sun = sky::sun_dir(f.angle);
    let sun_angle = (f.angle + 0.25).fract();
    let light = if sun_angle < 0.5 { sun } else { -sun };
    let (mut view, proj) = camera.build_view_proj();
    view.w_axis = Vec4::W; // the sky travels with the viewer: keep the rotation, drop the position
    let y = sun.y;
    let me_fade = if y < 0.18 { 0.37 + 1.2 * (-y).max(0.0) } else { 1.7 };
    let me = (1.0 - me_fade * (y - 0.18).abs()).clamp(0.0, 1.0).powi(2);
    let rising = f.angle > 0.5;
    let blue_hour = (-150.0 * (y + 0.07283) * (y + 0.07283)).exp();
    let v4 = |v: Vec3| v.extend(0.0).to_array();
    Params {
        inv_vp: (proj * view).inverse().to_cols_array_2d(),
        sun_dir: v4(sun),
        moon_dir: v4(-sun),
        light_dir: v4(light),
        celestial: Mat4::from_rotation_x(-f.angle * TAU).to_cols_array_2d(),
        times: [if rising { me } else { 0.0 }, if y > 0.0 { 1.0 - me } else { 0.0 }, if rising { 0.0 } else { me }, if y < 0.0 { 1.0 - me } else { 0.0 }],
        misc: [f.rain, sun_angle, f.seconds, 0.10 + 0.20 * blue_hour],
        zenith: [0.0; 4],
        horizon: [0.0; 4],
        stars: [1.0, 1.0, 1.0, 0.0],
        vp: (proj * view).to_cols_array_2d(),
    }
}

/// `RenderGlobal.renderSky`'s sunrise/sunset fan as 16 triangles of `[x, y, z, r, g, b, a]` (empty while the sun is high or low): centre
/// 100 out towards the sun's horizon point with the glow colour, the ring of radius 120 (rising 40 * alpha on the sun's side) fully
/// transparent. The Java builds it with three `glRotatef`; written out, the ring is `h * 120 cos f + up * 40 a cos f + side * 120 sin f`
/// with `h` the horizontal direction of the sun. Colours are linearised (`photon::legacy_lin`) like the rest of the legacy sky.
pub fn fan_vertices(angle: f32, rain: f32, sun: Vec3) -> Vec<[f32; 7]> {
    let (Some(c), h) = (sky::sunrise_color(angle), Vec3::new(sun.x, 0.0, sun.z).normalize_or_zero()) else { return Vec::new() };
    if h == Vec3::ZERO { return Vec::new(); }
    let rgb = crate::render::photon::legacy_lin([c[0], c[1], c[2]]);
    let a = c[3] * (1.0 - rain);
    let side = Vec3::Y.cross(h);
    let ring = |f: f32| h * (120.0 * f.cos()) + Vec3::Y * (40.0 * a * f.cos()) + side * (120.0 * f.sin());
    let v = |p: Vec3, alpha: f32| [p.x, p.y, p.z, rgb[0], rgb[1], rgb[2], alpha];
    let centre = v(h * 100.0, a);
    (0..16).flat_map(|j| {
        let (f0, f1) = (j as f32 * TAU / 16.0, (j + 1) as f32 * TAU / 16.0);
        [centre, v(ring(f0), 0.0), v(ring(f1), 0.0)]
    }).collect()
}

/// Stars and sunrise fan of the Console-Edition sky (see `fs_geo` in `sky.wgsl`).
struct Geo {
    stars: wgpu::RenderPipeline,
    fan: wgpu::RenderPipeline,
    star_v: wgpu::Buffer,
    star_i: wgpu::Buffer,
    star_n: u32,
    fan_v: wgpu::Buffer,
    fan_n: std::sync::atomic::AtomicU32,
    /// The sun and moon quads (`vs_body`): 12 corners, a texture bind group (group 2; group 1 is the SH's, so an empty one fills it).
    body: wgpu::RenderPipeline,
    body_v: wgpu::Buffer,
    body_bind: wgpu::BindGroup,
    empty_bind: wgpu::BindGroup,
    /// `f32` bits of the star brightness; 0 = skip the draw.
    star_alpha: std::sync::atomic::AtomicU32,
}

/// Everything the sky owns on the GPU.
pub struct SkyRenderer {
    pub uniform_buffer: wgpu::Buffer,
    pub layout: wgpu::BindGroupLayout,
    bind: wgpu::BindGroup,
    pub sky_map: wgpu::TextureView,
    pub sh: wgpu::Buffer,
    d0: wgpu::RenderPipeline,
    d4a: wgpu::ComputePipeline,
    d4a_bind: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    frame: std::sync::atomic::AtomicU32,
    geo: Option<Geo>,
}

impl SkyRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, surface_format: wgpu::TextureFormat) -> Self {
        Self::with_look(device, queue, surface_format, crate::LEGACY)
    }

    /// `legacy`: the sky pass draws the Console-Edition sky (`fs_legacy`) instead of Photon's atmosphere.
    pub fn with_look(device: &wgpu::Device, queue: &wgpu::Queue, surface_format: wgpu::TextureFormat, legacy: bool) -> Self {
        let tex = |label: &str, size: wgpu::Extent3d, dimension: wgpu::TextureDimension, format: wgpu::TextureFormat, data: &[u8]| {
            device.create_texture_with_data(
                queue,
                &wgpu::TextureDescriptor { label: Some(label), size, mip_level_count: 1, sample_count: 1, dimension, format, usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] },
                wgpu::util::TextureDataOrder::LayerMajor,
                data,
            )
        };
        // The file is RGB16F; there is no 3-channel half format, so pad every texel with an alpha of 1.0 (half 0x3C00).
        let lut_data: Vec<u8> = LUT_DAT.chunks(6).flat_map(|t| t.iter().copied().chain([0x00, 0x3C])).collect();
        let lut = tex("sky_lut", wgpu::Extent3d { width: LUT_SIZE.0, height: LUT_SIZE.1, depth_or_array_layers: LUT_SIZE.2 }, wgpu::TextureDimension::D3, wgpu::TextureFormat::Rgba16Float, &lut_data);
        let noise = tex("sky_noise", wgpu::Extent3d { width: NOISE_SIZE, height: NOISE_SIZE, depth_or_array_layers: 1 }, wgpu::TextureDimension::D2, wgpu::TextureFormat::Rgba8Unorm, NOISE);
        let sampler = |mode| device.create_sampler(&wgpu::SamplerDescriptor { address_mode_u: mode, address_mode_v: mode, address_mode_w: mode, mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let (lut_samp, noise_samp) = (sampler(wgpu::AddressMode::ClampToEdge), sampler(wgpu::AddressMode::Repeat));

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor { label: Some("sky_uniform"), size: std::mem::size_of::<Params>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let all = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE;
        let texture = |binding, dim| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: all,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: dim, multisampled: false },
            count: None,
        };
        let samp = |binding| wgpu::BindGroupLayoutEntry { binding, visibility: all, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: all, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
                texture(1, wgpu::TextureViewDimension::D3),
                samp(2),
                texture(3, wgpu::TextureViewDimension::D2),
                samp(4),
            ],
        });
        let view = |t: &wgpu::Texture| t.create_view(&Default::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky_bind"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view(&lut)) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&lut_samp) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&view(&noise)) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&noise_samp) },
            ],
        });

        let map_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sky_map"),
            size: wgpu::Extent3d { width: SKY_MAP.0, height: SKY_MAP.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let sky_map = view(&map_tex);
        let sh = device.create_buffer(&wgpu::BufferDescriptor { label: Some("sky_sh"), size: 160, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("sky_shader"), source: wgpu::ShaderSource::Wgsl(format!("{WGSL}\n{SH_WGSL}").into()) });
        let pl = |label: &str, layouts: &[&wgpu::BindGroupLayout]| device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(label), bind_group_layouts: layouts, push_constant_ranges: &[] });
        let render = |label: &str, fs: &str, format: wgpu::TextureFormat, depth: Option<wgpu::DepthStencilState>| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&pl(label, &[&layout])),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some(fs), targets: &[Some(format.into())], compilation_options: Default::default() }),
            primitive: Default::default(),
            depth_stencil: depth,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let d0 = render("sky_d0", "fs_d0", wgpu::TextureFormat::Rgba16Float, None);
        // Only where nothing was drawn: the triangle sits at depth 1.0, the frame clears to 1.0 and the terrain writes less.
        let pipeline = render("sky_pass", if legacy { "fs_legacy" } else { "fs_sky" }, surface_format, Some(wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth32Float, depth_write_enabled: false, depth_compare: wgpu::CompareFunction::LessEqual, stencil: Default::default(), bias: Default::default() }));

        let sh_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky_sh_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None }, count: None },
            ],
        });
        let d4a_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky_sh_bind"),
            layout: &sh_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&sky_map) }, wgpu::BindGroupEntry { binding: 1, resource: sh.as_entire_binding() }],
        });
        let d4a = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some("sky_d4a"), layout: Some(&pl("sky_d4a", &[&layout, &sh_layout])), module: &shader, entry_point: Some("cs_sh"), compilation_options: Default::default(), cache: None });

        let geo = legacy.then(|| {
            let verts: Vec<[f32; 3]> = sky::star_vertices().iter().map(|p| p.map(|c| c as f32)).collect();
            let idx: Vec<u32> = (0..verts.len() as u32 / 4).flat_map(|s| [4 * s, 4 * s + 1, 4 * s + 2, 4 * s, 4 * s + 2, 4 * s + 3]).collect();
            let buf = |label, data: &[u8], usage| device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: data, usage });
            let depth = || Some(wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth32Float, depth_write_enabled: false, depth_compare: wgpu::CompareFunction::LessEqual, stencil: Default::default(), bias: Default::default() });
            let geo_pipe = |label: &str, vs: &str, fs: &str, blend: wgpu::BlendState, groups: &[&wgpu::BindGroupLayout], stride: u64, attrs: &[wgpu::VertexAttribute]| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pl(label, groups)),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some(vs), buffers: &[wgpu::VertexBufferLayout { array_stride: stride, step_mode: wgpu::VertexStepMode::Vertex, attributes: attrs }], compilation_options: Default::default() },
                fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some(fs), targets: &[Some(wgpu::ColorTargetState { format: surface_format, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
                primitive: Default::default(),
                depth_stencil: depth(),
                multisample: Default::default(),
                multiview: None,
                cache: None,
            });
            // The sun and moon: two squares of two triangles, corner (+-1, +-1) and which body (0 sun, 1 moon).
            let quad = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
            let body_verts: Vec<[f32; 3]> = [0.0, 1.0].iter().flat_map(|&k| quad.map(|(x, y)| [x, y, k])).collect();
            let additive = wgpu::BlendState { color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add }, alpha: wgpu::BlendComponent::OVER };
            let empty = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("sky_empty"), entries: &[] });
            let body_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("sky_body_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
                    wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                ],
            });
            let body_tex = tex("sky_body", wgpu::Extent3d { width: 64, height: 32, depth_or_array_layers: 1 }, wgpu::TextureDimension::D2, wgpu::TextureFormat::Rgba8UnormSrgb, CELESTIAL);
            let empty_bind = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("sky_empty_bind"), layout: &empty, entries: &[] });
            let body_samp = sampler(wgpu::AddressMode::ClampToEdge);
            let body_view = body_tex.create_view(&Default::default());
            let body_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sky_body_bind"),
                layout: &body_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&body_view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&body_samp) }],
            });
            let pos = wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 };
            let col = wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 12, shader_location: 1 };
            Geo {
                stars: geo_pipe("sky_stars", "vs_stars", "fs_geo", wgpu::BlendState::ALPHA_BLENDING, &[&layout], 12, &[pos]),
                fan: geo_pipe("sky_fan", "vs_fan", "fs_geo", wgpu::BlendState::ALPHA_BLENDING, &[&layout], 28, &[pos, col]),
                body: geo_pipe("sky_body", "vs_body", "fs_body", additive, &[&layout, &empty, &body_layout], 12, &[pos]),
                body_v: buf("sky_body_v", bytemuck::cast_slice(&body_verts), wgpu::BufferUsages::VERTEX),
                body_bind,
                empty_bind,
                star_v: buf("sky_star_v", bytemuck::cast_slice(&verts), wgpu::BufferUsages::VERTEX),
                star_i: buf("sky_star_i", bytemuck::cast_slice(&idx), wgpu::BufferUsages::INDEX),
                star_n: idx.len() as u32,
                fan_v: device.create_buffer(&wgpu::BufferDescriptor { label: Some("sky_fan_v"), size: 48 * 28, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }),
                fan_n: Default::default(),
                star_alpha: Default::default(),
            }
        });

        Self { uniform_buffer, layout, bind, sky_map, sh, d0, d4a, d4a_bind, pipeline, frame: Default::default(), geo }
    }

    /// The group 0 bind group (uniform, LUT, noise and their samplers) for passes that include `WGSL`.
    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind
    }

    /// Upload this frame's sun, moon and camera.
    pub fn update(&self, queue: &wgpu::Queue, camera: &FirstPersonCamera, f: &SkyFrame) {
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&params(f, camera)));
    }

    /// Legacy sky colours (linear working space). Call after `update`, which rewrites the whole block.
    /// Also the star brightness (`World.getStarBrightness` x (1 - rain)) and the sunrise fan of this `angle`.
    pub fn set_legacy(&self, queue: &wgpu::Queue, zenith: [f32; 3], horizon: [f32; 3], angle: f32, rain: f32) {
        use std::sync::atomic::Ordering::Relaxed;
        let stars = sky::star_brightness(angle) * (1.0 - rain);
        let v = [[zenith[0], zenith[1], zenith[2], 0.0], [horizon[0], horizon[1], horizon[2], 0.0], [1.0, 1.0, 1.0, stars]];
        queue.write_buffer(&self.uniform_buffer, std::mem::offset_of!(Params, zenith) as u64, bytemuck::bytes_of(&v));
        if let Some(g) = &self.geo {
            let fan = fan_vertices(angle, rain, sky::sun_dir(angle));
            if !fan.is_empty() {
                queue.write_buffer(&g.fan_v, 0, bytemuck::cast_slice(&fan));
            }
            g.fan_n.store(fan.len() as u32, Relaxed);
            g.star_alpha.store(stars.to_bits(), Relaxed);
        }
    }

    /// d0 and d4a: the sky map, then its SH. Outside any render pass, before the scene.
    pub fn encode(&self, enc: &mut wgpu::CommandEncoder) {
        // The map and SH do not depend on the camera and the sun moves slowly: refresh every 4th frame (the first call always runs).
        if self.frame.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 4 != 0 { return; }
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sky_d0"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &self.sky_map, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rp.set_pipeline(&self.d0);
            rp.set_bind_group(0, &self.bind, &[]);
            rp.draw(0..3, 0..1);
        }
        let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("sky_d4a"), timestamp_writes: None });
        cp.set_pipeline(&self.d4a);
        cp.set_bind_group(0, &self.bind, &[]);
        cp.set_bind_group(1, &self.d4a_bind, &[]);
        cp.dispatch_workgroups(1, 1, 1);
    }

    /// The sky over every pixel still at depth 1, inside the scene pass after the opaque geometry.
    /// Legacy only, right after `draw`: the sunrise fan, the sun and moon, then the stars (vanilla order). Same pass, same read-only depth.
    pub fn draw_extras(&self, rp: &mut wgpu::RenderPass<'_>) {
        use std::sync::atomic::Ordering::Relaxed;
        let Some(g) = &self.geo else { return };
        rp.set_bind_group(0, &self.bind, &[]);
        let n = g.fan_n.load(Relaxed);
        if n > 0 {
            rp.set_pipeline(&g.fan);
            rp.set_vertex_buffer(0, g.fan_v.slice(..));
            rp.draw(0..n, 0..1);
        }
        rp.set_pipeline(&g.body);
        rp.set_bind_group(1, &g.empty_bind, &[]);
        rp.set_bind_group(2, &g.body_bind, &[]);
        rp.set_vertex_buffer(0, g.body_v.slice(..));
        rp.draw(0..12, 0..1);
        if f32::from_bits(g.star_alpha.load(Relaxed)) > 0.0 {
            rp.set_pipeline(&g.stars);
            rp.set_vertex_buffer(0, g.star_v.slice(..));
            rp.set_index_buffer(g.star_i.slice(..), wgpu::IndexFormat::Uint32);
            rp.draw_indexed(0..g.star_n, 0, 0..1);
        }
    }

    pub fn draw(&self, rp: &mut wgpu::RenderPass<'_>) {
        rp.set_pipeline(&self.pipeline);
        rp.set_bind_group(0, &self.bind, &[]);
        rp.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sunrise fan exists only near the horizon, is centred 100 out towards the sun's horizon point, fades to nothing at its rim.
    #[test]
    fn fan_hugs_the_sun_at_the_horizon() {
        assert!(fan_vertices(0.0, 0.0, sky::sun_dir(0.0)).is_empty(), "no fan at noon");
        let sun = sky::sun_dir(0.25); // sunset: the sun sits on the horizon
        let fan = fan_vertices(0.25, 0.0, sun);
        assert_eq!(fan.len(), 16 * 3);
        let h = Vec3::new(sun.x, 0.0, sun.z).normalize();
        let c = Vec3::new(fan[0][0], fan[0][1], fan[0][2]);
        assert!((c - h * 100.0).length() < 1e-3 && fan[0][6] > 0.0);
        assert!(fan.chunks(3).all(|t| t[1][6] == 0.0 && t[2][6] == 0.0 && t[0][6] == fan[0][6]));
        assert!(fan_vertices(0.25, 1.0, sun).iter().all(|v| v[6] == 0.0), "rain hides the glow");
    }

    /// The celestial texture is the sun and the moon, 32 x 32 each: bright at their middle, black at the corners (additive blending).
    #[test]
    fn celestial_texture_is_sun_and_moon() {
        assert_eq!(CELESTIAL.len(), 64 * 32 * 4);
        let px = |x: usize, y: usize| &CELESTIAL[(y * 64 + x) * 4..][..3];
        assert!(px(16, 16).iter().all(|&c| c > 150) && px(48, 16).iter().all(|&c| c > 100));
        assert!(px(0, 0).iter().all(|&c| c < 8) && px(32, 0).iter().all(|&c| c < 8));
    }

    /// Both sky fragment entries (Photon's and the Console-Edition one) parse and validate; no GPU needed.
    #[test]
    fn sky_shader_validates_with_legacy_entry() {
        let src = format!("{WGSL}\n{SH_WGSL}");
        let m = naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&src)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&m).unwrap();
        for entry in ["fs_sky", "fs_legacy", "fs_geo", "vs_stars", "vs_fan", "vs_body", "fs_body"] {
            assert!(m.entry_points.iter().any(|e| e.name == entry), "{entry} missing");
        }
    }

    /// Reads `n` bytes of a GPU buffer back.
    fn read(dev: &wgpu::Device, q: &wgpu::Queue, enc: wgpu::CommandEncoder, src: &wgpu::Buffer, n: u64) -> Vec<f32> {
        let out = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: n, usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let mut enc = enc;
        enc.copy_buffer_to_buffer(src, 0, &out, 0, n);
        q.submit([enc.finish()]);
        out.slice(..).map_async(wgpu::MapMode::Read, |_| ());
        dev.poll(wgpu::PollType::Wait).unwrap();
        let v = bytemuck::cast_slice(&out.slice(..).get_mapped_range()).to_vec();
        v
    }

    /// The sky at `angle` as an 8-bit image of `w x h` (RGBA) plus the SH buffer (10 x 4 floats); `None` without an adapter.
    fn render(angle: f32, rain: f32, pitch: f32) -> Option<(Vec<u8>, Vec<f32>)> {
        let inst = wgpu::Instance::default();
        let ad = pollster::block_on(inst.request_adapter(&Default::default())).ok()?;
        let (dev, q) = pollster::block_on(ad.request_device(&Default::default())).ok()?;
        let (w, h) = (256u32, 128u32);
        let fmt = wgpu::TextureFormat::Rgba8Unorm;
        let sky = SkyRenderer::with_look(&dev, &q, fmt, false); // the Photon sky, whatever `LEGACY` says
        let mut cam = FirstPersonCamera::spawn_at(0.0, 70.0, 0.0);
        (cam.aspect, cam.pitch) = (w as f32 / h as f32, pitch);
        sky.update(&q, &cam, &SkyFrame { angle, rain, seconds: 3.0 });
        let target = dev.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: fmt, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC, view_formats: &[] });
        let depth = dev.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Depth32Float, usage: wgpu::TextureUsages::RENDER_ATTACHMENT, view_formats: &[] });
        let mut enc = dev.create_command_encoder(&Default::default());
        sky.encode(&mut enc);
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &target.create_view(&Default::default()), resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::RED), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &depth.create_view(&Default::default()), depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }), stencil_ops: None }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            sky.draw(&mut rp);
        }
        let img = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: (w * h * 4) as u64, usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &target, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &img, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) } },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        let sh = read(&dev, &q, enc, &sky.sh, 160);
        img.slice(..).map_async(wgpu::MapMode::Read, |_| ());
        dev.poll(wgpu::PollType::Wait).unwrap();
        let pixels = img.slice(..).get_mapped_range().to_vec();
        Some((pixels, sh))
    }

    /// The times of day the pack distinguishes, and the sun/moon vectors (pure CPU).
    #[test]
    fn params_follow_the_sun() {
        let cam = FirstPersonCamera::spawn_at(0.0, 70.0, 0.0);
        let p = |angle| params(&SkyFrame { angle, rain: 0.0, seconds: 0.0 }, &cam);
        let noon = p(0.0);
        assert!(noon.sun_dir[1] > 0.999 && noon.moon_dir[1] < -0.999 && noon.light_dir == noon.sun_dir);
        assert_eq!((noon.times[1], noon.times[3]), (1.0, 0.0)); // noon only
        let sunset = p(0.25); // sun on the horizon, going down: the light is already the moon's (sunAngle 0.5)
        assert!(sunset.times[2] > 0.5 && sunset.times[0] == 0.0 && sunset.light_dir == sunset.moon_dir);
        let sunrise = p(0.75);
        assert!(sunrise.times[0] > 0.5 && sunrise.times[2] == 0.0 && sunrise.light_dir == sunrise.sun_dir);
        assert!(p(0.5).times[3] == 1.0 && p(0.5).misc[1] == 0.75);
    }

    /// Runs the real shaders on whatever adapter exists (lavapipe is enough; skipped without one): the sky is finite, blue at
    /// noon and dark at midnight, the SH of the noon sky points up (irradiance up > 0, DC term > 0), and every pixel was covered.
    #[test]
    fn sky_renders_and_sh_is_sane() {
        let Some((day, sh_day)) = render(0.0, 0.0, -0.4) else { return };
        let (night, sh_night) = render(0.5, 0.0, -0.4).unwrap();
        assert!(sh_day.iter().chain(&sh_night).all(|v| v.is_finite()), "NaN or inf in the SH: {sh_day:?}");
        let mean = |img: &[u8], c: usize| img.chunks(4).map(|p| p[c] as f32).sum::<f32>() / (img.len() / 4) as f32;
        assert!(day.chunks(4).all(|p| !(p[0] == 255 && p[1] == 0 && p[2] == 0)), "some pixels kept the clear colour");
        assert!(mean(&day, 2) > mean(&day, 0) + 20.0, "noon sky is not blue: r {} b {}", mean(&day, 0), mean(&day, 2));
        assert!(mean(&night, 2) < mean(&day, 2) * 0.5, "midnight is not darker than noon");
        assert!(sh_day[0] > 0.0 && sh_day[9 * 4 + 2] > 0.0, "no skylight at noon: {sh_day:?}");
        assert!(sh_day[9 * 4 + 2] > sh_night[9 * 4 + 2] * 5.0, "the night sky lights as much as the day sky");
    }
}
