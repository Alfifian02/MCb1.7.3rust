//! Photon's clouds (SixthSurge): `program/p0_clouds_prep` (cloud shadow map + cumulus coverage map),
//! `d1_clouds` (volumetric cumulus / altocumulus / congestus, planar cirrus + noctilucent, crepuscular rays and aurora,
//! raymarched for 1 pixel in 16) and `d2_clouds_upscaling` (temporal reprojection + 4x4 checkerboard upscale), with the
//! cloud parameters of `include/weather/clouds.glsl` worked out on the CPU (`CloudsParameters`).
//! The shader is `wgsl/clouds_lib.wgsl` (every cloud include, the atmosphere LUT lookups, cloud shadows) +
//! `wgsl/clouds_passes.wgsl` (the three programs); `render::pack` has the time/weather/biome scalars they read.
//!
//! What this milestone leaves to the others (the interfaces are the arguments of `Clouds::new` / `render`):
//! - the scattering LUT is milestone 2's (d0): `atmosphere_lut` loads the pack's `scattering.dat` the same way so the
//!   clouds run before it lands; hand them the texture M2 builds instead and delete the loader;
//! - the clouds' output (`history`, `data`) is read by d4_deferred_shading / the sky map (milestone 1 / 2) with
//!   `read_clouds_and_aurora`'s recipe (`history` has 3 mips: `ld` = 2 * dampen(1 - 0.1 * pixel age)); until then
//!   `lib.rs` does not run it (`RUN_CLOUDS`);
//! - the previous-frame depth d2 needs (colortex14 in the pack, written by d3_ao) is a small pass of ours
//!   (`fs_depth_history`); GTAO (milestone 6) can take it over;
//! - `taa_offset` is 0 until TAA (milestone 5) jitters the projection; `fix_hand_depth` and the LoD-mod paths are dropped
//!   (no hand pass, no Distant Horizons).
//! Deviations: the apparent distance is clamped to 65000 (the targets are f16: 1e6 would be inf and `mix` makes NaN);
//! `wetness` is the rain strength; `sunPathRotation` is 0 (mc-rs's sun moves in a plane); sea level is 64.
//! Written WITHOUT a Rust toolchain: the WGSL was compiled with Tint (Chromium WebGPU), the Rust was not compiled.

use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Mat4, Vec3};
use wgpu::{BindGroup, BindGroupLayout, CommandEncoder, Device, Queue, RenderPipeline, Sampler, Texture, TextureFormat, TextureView};

use crate::gpu::context::Gpu;
use crate::render::pack::{self, BiomeEnv, PackWeather, TimeOfDay};

/// Whether `lib.rs` runs the clouds each frame. They cost a raymarch for 1/16 of the pixels and nothing reads them
/// until d4 (milestone 1) does: flip it there.
pub const RUN_CLOUDS: bool = false;

/// `CLOUDS_TEMPORAL_UPSCALING`: the cloud target is 1/4 x 1/4 of the frame, one texel of every 4x4 per frame.
pub const UPSCALING: u32 = 4;
/// `colortex11`'s mip levels (`ld` of `read_clouds_and_aurora` is at most 2).
pub const MIPS: u32 = 3;
/// The cloud shadow / coverage map (`size.buffer.colortex8 = 512 512`).
pub const MAP_RES: u32 = 512;

const NOISE: &[u8] = include_bytes!("../../assets/noise.rgba");
const WORLEY_BUBBLY: &[u8] = include_bytes!("../../assets/worley_bubbly.dat");
const WORLEY_SWIRLEY: &[u8] = include_bytes!("../../assets/worley_swirley.dat");
const SCATTERING: &[u8] = include_bytes!("../../assets/scattering.dat");

const SHADER: &str = concat!(include_str!("wgsl/clouds_lib.wgsl"), "\n", include_str!("wgsl/clouds_passes.wgsl"));

// ---------------------------------------------------------------- CloudsParameters (weather/clouds.glsl)

/// `CloudsParameters`, in the order of the GLSL struct.
#[derive(Clone, Copy, Debug)]
pub struct CloudsParameters {
    pub cumulus_congestus_blend: f32,
    pub l0_coverage: [f32; 2],
    pub l0_detail_weights: [f32; 2],
    pub l0_edge_sharpening: [f32; 2],
    pub l0_altitude_scale: f32,
    pub l0_cumulus_stratus_blend: f32,
    pub l0_extinction_coeff: f32,
    pub l0_scattering_coeff: f32,
    pub l0_shadow: f32,
    pub l1_coverage: [f32; 2],
    pub l1_cumulus_stratus_blend: f32,
    pub l1_shadow: f32,
    pub cirrus_amount: f32,
    pub cirrocumulus_amount: f32,
    pub noctilucent_amount: f32,
    pub crepuscular_rays_amount: f32,
}

// settings.glsl defaults used below (the layers' `*_DENSITY`, `*_COVERAGE`, ... are 1 except these)
const CUMULUS_THICKNESS_M: f32 = 1200.0; // CLOUDS_CUMULUS_ALTITUDE * CLOUDS_CUMULUS_THICKNESS
const NOCTILUCENT_RARITY: f32 = 0.70;

fn dot2(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

impl CloudsParameters {
    /// `get_clouds_parameters(weather)`. `world_day` is `world_ticks / 24000`.
    pub fn new(w: &PackWeather, t: &TimeOfDay, rain: f32, world_day: u32) -> Self {
        use pack::{dampen, linear_step, smoothstep, sqr};
        let (temp, hum, wind) = (w.temperature, w.humidity, w.wind);

        // Volumetric layer 0 - cumulus / stratocumulus / stratus
        let l0_blend = (dampen(linear_step(0.5, 1.0, 1.0 - temp)) * dampen(1.0 - wind)).clamp(0.0, 1.0);
        let stratus_sheet = sqr(l0_blend);
        let temperature_weight = 1.0 - 0.15 * linear_step(0.6, 1.0, temp);
        let humidity_weight = 0.4 * hum + 0.5 * sqr(hum);
        let local_variation = [-0.1 * (0.1 + 0.1 * wind), 1.0 * (0.1 + 0.1 * wind)];
        let l0_coverage = local_variation.map(|v| (temperature_weight * humidity_weight + v + 0.3 * stratus_sheet).clamp(0.0, 1.0));

        let wind_torn = linear_step(0.66, 0.9, wind) * (1.0 - l0_blend);
        let a = [0.33 * (1.0 + 0.5 * wind_torn), 0.40 * (1.0 + 0.5 * wind_torn)];
        let l0_detail_weights = [a[0] + (0.07 - a[0]) * sqr(l0_blend), a[1] + (0.10 - a[1]) * l0_blend];
        let l0_edge_sharpening = [3.0 + (2.0 - 3.0) * sqr(l0_blend), 12.0 + (7.0 - 12.0) * sqr(l0_blend)];
        let dynamic_thickness = 0.5 + 0.5 * smoothstep(0.4, 0.6, dot2(l0_coverage, [0.25, 0.75]));
        let l0_altitude_scale = 0.8 / (dynamic_thickness * CUMULUS_THICKNESS_M);

        // Volumetric layer 1 - altocumulus / altostratus / undulatus
        let t1 = linear_step(0.5, 0.66, 1.0 - temp);
        let l1_blend = t1 * t1 * (3.0 - 2.0 * t1);
        let ac = (linear_step(0.4, 1.0, hum) * 0.5 + 0.5) * linear_step(0.4, 0.66, wind) * linear_step(1.0, 0.8, temp);
        let coverage_ac = [ac * 0.5, ac * 1.5];
        let as_ = linear_step(0.25, 0.45, wind * hum);
        let l1_coverage = coverage_ac.map(|c| (c + (as_ - c) * l1_blend).clamp(0.0, 1.0));

        // Planar clouds
        let temperature_weight = 0.6 - 0.4 * sqr(linear_step(0.5, 0.9, temp)) + 0.4 * (1.0 - linear_step(0.0, 0.2, temp));
        let humidity_weight = 1.0 - 0.5 * linear_step(0.5, 0.75, hum);
        let cirrus_amount = (0.5 * temperature_weight * humidity_weight + 0.5 * rain).clamp(0.0, 1.0);
        let temperature_weight = 1.0 - 0.3 * linear_step(0.5, 1.0, temp);
        let humidity_weight = 0.5 + 0.5 * linear_step(0.4, 1.0, hum);
        let cirrocumulus_amount = 0.8 * dampen(temperature_weight * humidity_weight * wind * wind.sqrt());
        let intensity = linear_step(NOCTILUCENT_RARITY, 1.0, pack::hash1((world_day as f32 * 1.618_034).fract()));
        let noctilucent_amount = dampen(intensity);

        // Cumulus congestus replaces layer 0 when it is hot, humid and windy
        let l0_high_coverage = linear_step(0.45, 0.5, dot2(l0_coverage, [0.66, 0.33]));
        let congestus = (1.5 * dampen(dampen(linear_step(0.5, 1.0, temp) * linear_step(0.3, 0.9, hum) * sqr(wind))) * (1.0 - l0_high_coverage)).clamp(0.0, 1.0);

        // Lighting parameters
        let l0_shadow = linear_step(0.7, 1.0, dot2(l1_coverage, [0.25, 0.75]) * (1.0 + l1_blend)) * dampen(t.day_factor);
        let mut ext = (0.05 + (0.1 - 0.05) * smoothstep(0.0, 0.3, t.sun_dir.y.abs())) * (1.0 - 0.33 * rain) * (1.0 - 0.6 * l0_shadow);
        ext *= 1.0 - 0.4 * linear_step(0.8, 1.0, l0_coverage[1] - l0_coverage[1] * l0_blend);
        ext *= 1.0 - 0.5 * l0_blend;
        let scat = ext * (1.0 + (0.66 - 1.0) * rain);

        // Crepuscular rays
        let crepuscular = dampen((2.0 * hum).clamp(0.0, 1.0))
            * linear_step(0.45, 0.7, (1.0 + 0.2 * sqr(l0_blend)) * dot2(l0_coverage, [0.66, 0.33]) + 0.09 * l1_coverage[1]);

        Self {
            cumulus_congestus_blend: congestus,
            l0_coverage,
            l0_detail_weights,
            l0_edge_sharpening,
            l0_altitude_scale,
            l0_cumulus_stratus_blend: l0_blend,
            l0_extinction_coeff: ext,
            l0_scattering_coeff: scat,
            l0_shadow,
            l1_coverage,
            l1_cumulus_stratus_blend: l1_blend,
            l1_shadow: 0.0, // declared by the pack, never set, never read
            cirrus_amount,
            cirrocumulus_amount,
            noctilucent_amount,
            crepuscular_rays_amount: crepuscular,
        }
    }

    /// The five vec4s `cp_a..cp_e` of the WGSL uniform block.
    pub fn pack(&self) -> [[f32; 4]; 5] {
        [
            [self.cumulus_congestus_blend, self.l0_coverage[0], self.l0_coverage[1], self.l0_detail_weights[0]],
            [self.l0_detail_weights[1], self.l0_edge_sharpening[0], self.l0_edge_sharpening[1], self.l0_altitude_scale],
            [self.l0_cumulus_stratus_blend, self.l0_extinction_coeff, self.l0_scattering_coeff, self.l0_shadow],
            [self.l1_coverage[0], self.l1_coverage[1], self.l1_cumulus_stratus_blend, self.l1_shadow],
            [self.cirrus_amount, self.cirrocumulus_amount, self.noctilucent_amount, self.crepuscular_rays_amount],
        ]
    }
}

/// `shadowModelView` for the cloud shadow map: scene space -> light space, +z toward the light. mc-rs's sun moves in the
/// YZ plane, so X is always perpendicular to it. (The pack's is the real shadow camera; only the map's own writer and
/// readers use this one, and they use the same.)
pub fn shadow_view(light_dir: Vec3) -> Mat4 {
    let z = light_dir.normalize();
    let x = if z.x.abs() > 0.99 { Vec3::Y } else { Vec3::X };
    let x = (x - z * x.dot(z)).normalize();
    let y = z.cross(x);
    Mat4::from_mat3(Mat3::from_cols(x, y, z).transpose())
}

// ---------------------------------------------------------------- uniforms

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct U {
    view_inv: [[f32; 4]; 4],
    proj_inv: [[f32; 4]; 4],
    prev_view: [[f32; 4]; 4],
    prev_proj: [[f32; 4]; 4],
    shadow_view: [[f32; 4]; 4],
    shadow_view_inv: [[f32; 4]; 4],
    cam: [f32; 4],
    prev_cam: [f32; 4],
    sun: [f32; 4],
    moon: [f32; 4],
    light: [f32; 4],
    res: [f32; 4],
    taa: [f32; 4],
    t: [f32; 4],
    time: [f32; 4],
    flags: [f32; 4],
    low: [f32; 4],
    sun_rgb: [f32; 4],
    moon_rgb: [f32; 4],
    weather_rgb: [f32; 4],
    aur_amount: [f32; 4],
    aur0: [f32; 4],
    aur1: [f32; 4],
    cp: [[f32; 4]; 5],
    ints: [u32; 4],
}

/// What one frame of clouds needs from the game.
pub struct CloudsFrame {
    pub view: Mat4,
    pub proj: Mat4,
    /// Last frame's `view` / `proj` / eye (the caller keeps them; equal to this frame's on the first).
    pub prev_view: Mat4,
    pub prev_proj: Mat4,
    pub eye: Vec3,
    pub prev_eye: Vec3,
    pub near: f32,
    pub far: f32,
    /// `world::sky::celestial_angle`.
    pub angle: f32,
    pub rain: f32,
    pub world_ticks: f64,
    pub frame: u32,
    /// Seconds since start (`frameTimeCounter`) and since the last frame (`frameTime`).
    pub time_s: f32,
    pub dt: f32,
    pub biome: BiomeEnv,
    /// `taa_offset` (0 until milestone 5).
    pub taa_offset: [f32; 2],
    /// The world was reset or the clock jumped (`world_age_changed`): drop the history.
    pub world_age_changed: bool,
}

// ---------------------------------------------------------------- GPU resources

struct Sized {
    size: (u32, u32),
    low: Texture,
    low_data: Texture,
    /// colortex11 / colortex12, ping-ponged: the frame reads one and writes the other.
    hist: [Texture; 2],
    hist_data: [Texture; 2],
    prev_depth: Texture,
}

pub struct Clouds {
    layout: BindGroupLayout,
    ubuf: wgpu::Buffer,
    samp_repeat: Sampler,
    samp_clamp: Sampler,
    noise: TextureView,
    bubbly: TextureView,
    swirley: TextureView,
    lut: TextureView,
    cloud_map: Texture,
    dummy: TextureView,
    dummy_depth: TextureView,
    prep: RenderPipeline,
    clouds: RenderPipeline,
    upscale: RenderPipeline,
    depth_history: RenderPipeline,
    mip: RenderPipeline,
    sized: Option<Sized>,
    /// Which of `hist` was written last frame (and so is read this frame).
    last: usize,
    shadow_view: Mat4,
}

fn texture(device: &Device, label: &str, size: (u32, u32), format: TextureFormat, mips: u32) -> Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: size.0.max(1), height: size.1.max(1), depth_or_array_layers: 1 },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn volume(device: &Device, queue: &Queue, label: &str, size: (u32, u32, u32), format: TextureFormat, texel_bytes: u32, data: &[u8]) -> TextureView {
    let t = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: size.2 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: &t, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        data,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size.0 * texel_bytes), rows_per_image: Some(size.1) },
        wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: size.2 },
    );
    t.create_view(&wgpu::TextureViewDescriptor::default())
}

/// `scattering.dat` (RGB16F, 32 x 64 x 32) as an Rgba16Float volume (wgpu has no RGB16F; alpha = 1.0). Milestone 2 builds
/// this texture too: pass it to `Clouds::new` instead and drop this.
pub fn atmosphere_lut(device: &Device, queue: &Queue) -> TextureView {
    let mut rgba = Vec::with_capacity(SCATTERING.len() / 6 * 8);
    for px in SCATTERING.chunks_exact(6) {
        rgba.extend_from_slice(px);
        rgba.extend_from_slice(&0x3C00u16.to_le_bytes()); // 1.0 as f16
    }
    volume(device, queue, "atmosphere_lut", (32, 64, 32), TextureFormat::Rgba16Float, 8, &rgba)
}

impl Clouds {
    /// `lut` is the 3D scattering LUT (see `atmosphere_lut`).
    pub fn new(device: &Device, queue: &Queue, lut: TextureView) -> Self {
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty, count: None };
        let tex = |dim| wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: dim, multisampled: false };
        let (d2, d3) = (wgpu::TextureViewDimension::D2, wgpu::TextureViewDimension::D3);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("clouds_layout"),
            entries: &[
                entry(0, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }),
                entry(1, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
                entry(2, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
                entry(3, tex(d2)),
                entry(4, tex(d3)),
                entry(5, tex(d3)),
                entry(6, tex(d3)),
                entry(7, tex(d2)),
                entry(8, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: d2, multisampled: false }),
                entry(9, tex(d2)),
                entry(10, tex(d2)),
                entry(11, tex(d2)),
                entry(12, tex(d2)),
                entry(13, tex(d2)),
            ],
        });
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("clouds_uniform"),
            size: std::mem::size_of::<U>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = |mode| device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: mode,
            address_mode_v: mode,
            address_mode_w: mode,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        // noise.png and the worley volumes: `blur true, clamp false` in their .mcmeta
        let noise = {
            let t = texture(device, "noisetex", (512, 512), TextureFormat::Rgba8Unorm, 1);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &t, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                NOISE,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(512 * 4), rows_per_image: Some(512) },
                wgpu::Extent3d { width: 512, height: 512, depth_or_array_layers: 1 },
            );
            t.create_view(&Default::default())
        };
        let bubbly = volume(device, queue, "worley_bubbly", (64, 64, 64), TextureFormat::R8Unorm, 1, WORLEY_BUBBLY);
        let swirley = volume(device, queue, "worley_swirley", (64, 64, 64), TextureFormat::R8Unorm, 1, WORLEY_SWIRLEY);

        let cloud_map = texture(device, "cloud_map", (MAP_RES, MAP_RES), TextureFormat::Rgba8Unorm, 1); // colortex8 (RGB8)
        let dummy = texture(device, "clouds_dummy", (1, 1), TextureFormat::Rgba16Float, 1).create_view(&Default::default());
        let dummy_depth = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("clouds_dummy_depth"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default());

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("clouds"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("clouds_pl"), bind_group_layouts: &[&layout], push_constant_ranges: &[] });
        let pipeline = |label: &str, vs: &str, fs: &str, targets: &[TextureFormat], constants: &[(&str, f64)]| {
            let targets: Vec<Option<wgpu::ColorTargetState>> = targets.iter().map(|&f| Some(f.into())).collect();
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pl),
                vertex: wgpu::VertexState { module: &module, entry_point: Some(vs), buffers: &[], compilation_options: wgpu::PipelineCompilationOptions { constants, ..Default::default() } },
                fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some(fs), targets: &targets, compilation_options: wgpu::PipelineCompilationOptions { constants, ..Default::default() } }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        use TextureFormat::{Rg16Float, Rgba16Float, Rgba8Unorm};
        // p0 draws the coverage map, so it must not read it back and has no worley detail (`PROGRAM_PREPARE`)
        let prep = pipeline("clouds_prep", "vs_full", "fs_prep", &[Rgba8Unorm], &[("PROGRAM_PREPARE", 1.0), ("USE_LOCAL_COVERAGE_MAP", 0.0)]);
        let clouds = pipeline("clouds_d1", "vs_clouds", "fs_clouds", &[Rgba16Float, Rg16Float], &[]);
        let upscale = pipeline("clouds_d2", "vs_full", "fs_upscale", &[Rgba16Float, Rgba16Float], &[]);
        let depth_history = pipeline("clouds_depth_history", "vs_full", "fs_depth_history", &[Rg16Float], &[]);
        let mip = pipeline("clouds_mip", "vs_full", "fs_mip", &[Rgba16Float], &[]);

        Self {
            layout,
            ubuf,
            samp_repeat: sampler(wgpu::AddressMode::Repeat),
            samp_clamp: sampler(wgpu::AddressMode::ClampToEdge),
            noise,
            bubbly,
            swirley,
            lut,
            cloud_map,
            dummy,
            dummy_depth,
            prep,
            clouds,
            upscale,
            depth_history,
            mip,
            sized: None,
            last: 0,
            shadow_view: Mat4::IDENTITY,
        }
    }

    fn bind(&self, device: &Device, depth: &TextureView, cloud_map: &TextureView, low: &TextureView, low_data: &TextureView, hist: &TextureView, hist_data: &TextureView, prev_depth: &TextureView) -> BindGroup {
        let e = |binding, resource| wgpu::BindGroupEntry { binding, resource };
        use wgpu::BindingResource::{Sampler, TextureView as Tv};
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("clouds_bind"),
            layout: &self.layout,
            entries: &[
                e(0, self.ubuf.as_entire_binding()),
                e(1, Sampler(&self.samp_repeat)),
                e(2, Sampler(&self.samp_clamp)),
                e(3, Tv(&self.noise)),
                e(4, Tv(&self.bubbly)),
                e(5, Tv(&self.swirley)),
                e(6, Tv(&self.lut)),
                e(7, Tv(cloud_map)),
                e(8, Tv(depth)),
                e(9, Tv(low)),
                e(10, Tv(low_data)),
                e(11, Tv(hist)),
                e(12, Tv(hist_data)),
                e(13, Tv(prev_depth)),
            ],
        })
    }

    /// Run p0, d1, d2 and the two helper passes. `depth` is the frame's opaque depth (`gpu.depth_view`, stored by the
    /// chunk pass). After this `history()` / `data()` are this frame's colortex11 / colortex12.
    pub fn render(&mut self, gpu: &Gpu, enc: &mut CommandEncoder, depth: &TextureView, f: &CloudsFrame) {
        let (w, h) = (gpu.config.width, gpu.config.height);
        let low_size = ((w / UPSCALING).max(1), (h / UPSCALING).max(1));
        let mut reset = f.world_age_changed;
        if self.sized.as_ref().map_or(true, |s| s.size != (w, h)) {
            let d = &gpu.device;
            let hist = || texture(d, "clouds_history", (w, h), TextureFormat::Rgba16Float, MIPS);
            self.sized = Some(Sized {
                size: (w, h),
                low: texture(d, "clouds_low", low_size, TextureFormat::Rgba16Float, 1),
                low_data: texture(d, "clouds_low_data", low_size, TextureFormat::Rg16Float, 1),
                hist: [hist(), hist()],
                hist_data: [texture(d, "clouds_data", (w, h), TextureFormat::Rgba16Float, 1), texture(d, "clouds_data", (w, h), TextureFormat::Rgba16Float, 1)],
                prev_depth: texture(d, "clouds_prev_depth", (w / 2, h / 2), TextureFormat::Rg16Float, 1),
            });
            reset = true;
        }
        let s = self.sized.as_ref().unwrap();

        // ---- uniforms
        let t = TimeOfDay::new(f.angle);
        let world_age = (f.world_ticks % (128.0 * 24000.0)) as f32 / 20.0;
        let day = (f.world_ticks / 24000.0) as u32;
        let weather = PackWeather::new(world_age, &t, &f.biome, f.rain);
        let params = CloudsParameters::new(&weather, &t, f.rain, day);
        let aurora = pack::aurora_colors(day);
        let blue_hour = (-150.0 * pack::sqr(t.sun_dir.y + 0.07283)).exp();
        self.shadow_view = shadow_view(t.light_dir);
        let rot = |m: Mat4| Mat4::from_mat3(Mat3::from_mat4(m)); // the pack's scene space has the camera at the origin
        let v4 = |v: Vec3, w: f32| v.extend(w).to_array();
        let u = U {
            view_inv: rot(f.view).inverse().to_cols_array_2d(),
            proj_inv: f.proj.inverse().to_cols_array_2d(),
            prev_view: rot(f.prev_view).to_cols_array_2d(),
            prev_proj: f.prev_proj.to_cols_array_2d(),
            shadow_view: self.shadow_view.to_cols_array_2d(),
            shadow_view_inv: self.shadow_view.transpose().to_cols_array_2d(),
            cam: v4(f.eye, f.eye.y),
            prev_cam: v4(f.prev_eye, f.near),
            sun: v4(t.sun_dir, t.sun_angle),
            moon: v4(t.moon_dir, f.rain),
            light: v4(t.light_dir, f.rain), // wetness = rain strength
            res: [w as f32, h as f32, 1.0 / w as f32, 1.0 / h as f32],
            taa: [f.taa_offset[0], f.taa_offset[1], f.far, f.time_s],
            t: [world_age, f.dt.max(1e-4), t.day_factor, 0.10 + 0.20 * blue_hour],
            time: [t.sunrise, t.noon, t.sunset, t.midnight],
            flags: [1.0, reset as u8 as f32, 1.0, 0.0],
            low: [low_size.0 as f32, low_size.1 as f32, 0.0, 0.0],
            sun_rgb: v4(pack::sun_color(&t), 0.0),
            moon_rgb: v4(pack::moon_color(&t), 0.0),
            weather_rgb: v4(pack::weather_color(&t, &f.biome), 0.0),
            aur_amount: [pack::aurora_amount(&t, &f.biome, day), 0.0, 0.0, 0.0],
            aur0: v4(aurora[0], 0.0),
            aur1: v4(aurora[1], 0.0),
            cp: params.pack(),
            ints: [f.frame, 0, 0, 0],
        };
        gpu.queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&u));

        // ---- passes
        let (prev, cur) = (self.last, 1 - self.last);
        let view = |t: &Texture| t.create_view(&Default::default());
        let level = |t: &Texture, l: u32| t.create_view(&wgpu::TextureViewDescriptor { base_mip_level: l, mip_level_count: Some(1), ..Default::default() });
        let (map_v, low_v, low_data_v) = (view(&self.cloud_map), view(&s.low), view(&s.low_data));
        let (hist_prev, data_prev, prev_depth_v) = (view(&s.hist[prev]), view(&s.hist_data[prev]), view(&s.prev_depth));
        let (hist_cur0, data_cur) = (level(&s.hist[cur], 0), view(&s.hist_data[cur]));
        let d = &gpu.device;
        let dd = &self.dummy;
        let run = |enc: &mut CommandEncoder, label: &str, pipeline: &RenderPipeline, bind: &BindGroup, targets: &[&TextureView]| {
            let attachments: Vec<Option<wgpu::RenderPassColorAttachment>> = targets
                .iter()
                .map(|&v| Some(wgpu::RenderPassColorAttachment { view: v, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } }))
                .collect();
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some(label), color_attachments: &attachments, depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None });
            rp.set_pipeline(pipeline);
            rp.set_bind_group(0, bind, &[]);
            rp.draw(0..3, 0..1);
        };

        // p0: cloud shadow map + coverage map (the map slot is a dummy: it is the target)
        let b = self.bind(d, &self.dummy_depth, dd, dd, dd, dd, dd, dd);
        run(enc, "clouds_p0", &self.prep, &b, &[&map_v]);
        // d1: the raymarch, at 1/4 x 1/4
        let b = self.bind(d, depth, &map_v, dd, dd, dd, dd, dd);
        run(enc, "clouds_d1", &self.clouds, &b, &[&low_v, &low_data_v]);
        // d2: temporal upscale (reads last frame's history, writes this frame's)
        let b = self.bind(d, depth, &map_v, &low_v, &low_data_v, &hist_prev, &data_prev, &prev_depth_v);
        run(enc, "clouds_d2", &self.upscale, &b, &[&hist_cur0, &data_cur]);
        // mip chain of the new history, for the softening of young pixels
        for l in 1..MIPS {
            let src = level(&s.hist[cur], l - 1);
            let dst = level(&s.hist[cur], l);
            let b = self.bind(d, &self.dummy_depth, dd, dd, dd, &src, dd, dd);
            run(enc, "clouds_mip", &self.mip, &b, &[&dst]);
        }
        // 1 - depth for next frame's d2 (d3_ao writes this in the pack)
        let b = self.bind(d, depth, dd, dd, dd, dd, dd, dd);
        run(enc, "clouds_depth_history", &self.depth_history, &b, &[&view(&s.prev_depth)]);

        self.last = cur;
    }

    /// colortex11: this frame's clouds (rgb scattering, a transmittance), 3 mips. None before the first `render`.
    pub fn history(&self) -> Option<TextureView> {
        self.sized.as_ref().map(|s| s.hist[self.last].create_view(&Default::default()))
    }
    /// colortex12: x apparent distance (blocks), y pixel age, z ambient scattering.
    pub fn data(&self) -> Option<TextureView> {
        self.sized.as_ref().map(|s| s.hist_data[self.last].create_view(&Default::default()))
    }
    /// colortex8: xy cloud shadow (`get_cloud_shadows`), z cumulus coverage.
    pub fn shadow_map(&self) -> TextureView {
        self.cloud_map.create_view(&Default::default())
    }
    /// The `shadowModelView` the cloud shadow map was drawn with (d4 needs it for `project_cloud_shadow_map`).
    pub fn shadow_view(&self) -> Mat4 {
        self.shadow_view
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::biome::Biome;

    fn params(angle: f32, age: f32, rain: f32) -> CloudsParameters {
        let t = TimeOfDay::new(angle);
        let mut b = BiomeEnv::default();
        b.update(Biome::Forest, 0.7, 0.5, 0.02);
        CloudsParameters::new(&PackWeather::new(age, &t, &b, rain), &t, rain, 3)
    }

    #[test]
    fn uniform_block_is_what_the_shader_declares() {
        // 6 mat4 + 23 vec4 (cam .. aur1 is 17, cp 5, ints 1)
        assert_eq!(std::mem::size_of::<U>(), 6 * 64 + 23 * 16);
        assert_eq!(std::mem::size_of::<U>() % 16, 0);
        assert!(SHADER.contains("struct U {") && SHADER.contains("cp_e: vec4<f32>,") && SHADER.contains("ints: vec4<u32>,"));
    }

    #[test]
    fn parameters_stay_in_the_ranges_the_shader_assumes() {
        for i in 0..400 {
            let p = params(i as f32 / 400.0, i as f32 * 311.0, if i % 3 == 0 { 1.0 } else { 0.0 });
            let all = p.pack().concat();
            assert!(all.iter().all(|v| v.is_finite()), "{p:?}");
            for v in [p.cumulus_congestus_blend, p.l0_cumulus_stratus_blend, p.l0_shadow, p.l1_cumulus_stratus_blend, p.cirrus_amount, p.cirrocumulus_amount, p.noctilucent_amount, p.crepuscular_rays_amount].into_iter().chain(p.l0_coverage).chain(p.l1_coverage) {
                assert!((0.0..=1.0 + 1e-6).contains(&v), "{v} in {p:?}");
            }
            assert!(p.l0_extinction_coeff > 0.0 && p.l0_extinction_coeff <= 0.1 && p.l0_scattering_coeff <= p.l0_extinction_coeff);
            assert!(p.l0_altitude_scale > 0.8 / 1200.0 - 1e-9 && p.l0_altitude_scale <= 1.6 / 1200.0 + 1e-9);
        }
        // rain adds cirrus and thins the cumulus extinction
        assert!(params(0.0, 5000.0, 1.0).cirrus_amount >= params(0.0, 5000.0, 0.0).cirrus_amount + 0.49);
        assert!(params(0.0, 5000.0, 1.0).l0_extinction_coeff < params(0.0, 5000.0, 0.0).l0_extinction_coeff);
    }

    #[test]
    fn shadow_view_looks_down_the_light() {
        for a in [0.0_f32, 0.1, 0.2, 0.3, 0.7] {
            let l = crate::world::sky::sun_dir(a);
            let m = shadow_view(l);
            let z = m.transform_vector3(l);
            assert!((z - Vec3::Z).length() < 1e-5, "{z:?}"); // the light is +z in light space
            assert!((m.inverse() - m.transpose()).abs_diff_eq(Mat4::ZERO, 1e-5)); // a rotation
        }
    }

    #[test]
    fn lut_conversion_keeps_the_texels() {
        assert_eq!(SCATTERING.len(), 32 * 64 * 32 * 6);
        assert_eq!(NOISE.len(), 512 * 512 * 4);
        assert_eq!((WORLEY_BUBBLY.len(), WORLEY_SWIRLEY.len()), (64 * 64 * 64, 64 * 64 * 64));
    }
}
