//! Present chain, the whole post stage (it replaces the Photon HDR chain: exposure, bloom, TAA, grade, DOF...). The world is drawn into an
//! 8-bit sRGB `scene` at `RENDER_SCALE` (linear Rec. 2020, what the sky and chunk shaders write; the hardware encodes and decodes sRGB), then
//! ONE full-screen pass at surface size: (FXAA) + CAS + bilinear upscale + vignette onto the swap chain, with the HUD drawn into the same
//! pass. There used to be a second pass (FXAA + CAS into an `ldr` texture, then the upscale): that was a full-screen write and read of
//! memory a phone GPU pays for in bandwidth. With MSAA (`scene_samples() > 1`) the scene pass resolves into `scene` in tile memory and the
//! FXAA branch is compiled out; without it (the adapter has no 4x for the scene formats) FXAA runs in this same pass.
//! FXAA = the 3.9 console variant (4 corner taps, 4 along the edge, none on flat areas); CAS sharpens what the upscale softens, on the same
//! taps. Both work in `sqrt` space, the cheap perceptual one. The taps are one SCENE texel apart, whatever the output size.

use std::sync::atomic::{AtomicU32, Ordering};
use wgpu::{BindGroup, Buffer, Device, Queue, RenderPipeline, TextureView};

/// Format of the scene: 4 bytes a pixel (the old HDR scene was 8, with a mip chain and 5 full-screen passes).
pub const SCENE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// MSAA wanted for the scene pass. On a tiling GPU (most Android chips) the samples live in tile memory and the resolve happens when the
/// tile is written out, so 4x costs little bandwidth; it replaces the FXAA pass. `pick_samples` falls back to 1 when the adapter has no
/// such sample count for the scene's colour and depth formats.
pub const MSAA_WANT: u32 = 4;

/// Sample count of every scene-pass pipeline and of the colour / depth targets (`Gpu::create_depth`, `Present`). Set once by
/// `Gpu::try_init` before any of them is built; 1 until then (the headless tests run with 1).
static SCENE_SAMPLES: AtomicU32 = AtomicU32::new(1);

pub fn scene_samples() -> u32 {
    SCENE_SAMPLES.load(Ordering::Relaxed)
}

pub fn set_scene_samples(n: u32) {
    SCENE_SAMPLES.store(n.max(1), Ordering::Relaxed);
}

/// `MSAA_WANT` if the adapter can render to the scene and depth formats with it, else 1.
pub fn pick_samples(adapter: &wgpu::Adapter) -> u32 {
    let ok = |f| {
        let t = adapter.get_texture_format_features(f);
        t.allowed_usages.contains(wgpu::TextureUsages::RENDER_ATTACHMENT) && t.flags.sample_count_supported(MSAA_WANT)
    };
    if ok(SCENE_FORMAT) && ok(wgpu::TextureFormat::Depth32Float) { MSAA_WANT } else { 1 }
}

/// The state of a scene pipeline's `multisample` (alpha-to-coverage on for the cut-out layers: MSAA does not smooth a `discard` edge, but
/// the coverage mask made from the texel's alpha does).
pub fn multisample(alpha_to_coverage: bool) -> wgpu::MultisampleState {
    multisample_of(scene_samples(), alpha_to_coverage)
}

fn multisample_of(count: u32, alpha_to_coverage: bool) -> wgpu::MultisampleState {
    wgpu::MultisampleState { count, mask: !0, alpha_to_coverage_enabled: alpha_to_coverage && count > 1 }
}

const SHADER: &str = r#"
struct U { px: vec4<f32> }; // 1 / scene size, 1 / surface size
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var src: texture_2d<f32>;

const SRGB_SURFACE: bool = @SRGB@;
const FXAA: bool = @FXAA@; // false when the scene pass was multisampled
const CAS_SHARPNESS: f32 = 0.5;
const VIGNETTE: f32 = 0.04; // b1.7.3-style half-strength vignette (`Settings::legacy`: 0.08 * 0.5)
const R2020_TO_XYZ = mat3x3<f32>(vec3<f32>(0.6369736, 0.1446172, 0.1688585), vec3<f32>(0.2627066, 0.6779996, 0.0592938), vec3<f32>(0.0, 0.0280728, 1.0608437));
const XYZ_TO_709 = mat3x3<f32>(vec3<f32>(3.2406, -1.5372, -0.4986), vec3<f32>(-0.9689, 1.8758, 0.0415), vec3<f32>(0.0557, -0.2040, 1.0570));

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let q = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(q * 2.0 - 1.0, 0.0, 1.0);
}

fn tap(uv: vec2<f32>) -> vec3<f32> { return sqrt(max(textureSampleLevel(src, samp, uv, 0.0).rgb, vec3<f32>(0.0))); }
fn luma(c: vec3<f32>) -> f32 { return dot(c, vec3<f32>(0.299, 0.587, 0.114)); }

// The one full-screen pass: (FXAA +) CAS on the scene, bilinear upscale, Rec. 2020 -> 709, vignette, sRGB. `uv` is the surface pixel's
// place in the scene; the taps are one scene texel apart, so at a render scale below 1 CAS sharpens exactly what the upscale softens.
@fragment
fn fs_fin(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = p.xy * u.px.zw;
    let o = u.px.xy;
    let m = tap(uv);
    let nw = tap(uv + vec2<f32>(-o.x, -o.y));
    let ne = tap(uv + vec2<f32>(o.x, -o.y));
    let sw = tap(uv + vec2<f32>(-o.x, o.y));
    let se = tap(uv + vec2<f32>(o.x, o.y));
    var f = m;
    if FXAA {
        let l_nw = luma(nw); let l_ne = luma(ne); let l_sw = luma(sw); let l_se = luma(se);
        let lo = min(luma(m), min(min(l_nw, l_ne), min(l_sw, l_se)));
        let hi = max(luma(m), max(max(l_nw, l_ne), max(l_sw, l_se)));
        if hi - lo >= max(0.04, hi * 0.125) { // an edge: blur along it (FXAA 3.9 console)
            var dir = vec2<f32>(-((l_nw + l_ne) - (l_sw + l_se)), (l_nw + l_sw) - (l_ne + l_se));
            let reduce = max((l_nw + l_ne + l_sw + l_se) * (0.25 * 0.125), 1.0 / 128.0);
            dir = clamp(dir / (min(abs(dir.x), abs(dir.y)) + reduce), vec2<f32>(-8.0), vec2<f32>(8.0)) * o;
            let a = 0.5 * (tap(uv + dir * (1.0 / 3.0 - 0.5)) + tap(uv + dir * (2.0 / 3.0 - 0.5)));
            let b = a * 0.5 + 0.25 * (tap(uv - dir * 0.5) + tap(uv + dir * 0.5));
            f = select(b, a, luma(b) < lo || luma(b) > hi);
        }
    }
    // CAS (FidelityFX): the sharpening weight falls where the neighbourhood has no headroom to the signal limits; the kernel is the
    // four corner taps already fetched (a rotated cross), the centre is the FXAA result (or the plain scene sample).
    let mn = min(min(min(nw, ne), min(sw, se)), f);
    let mx = max(max(max(nw, ne), max(sw, se)), f);
    var w = clamp(min(mn, vec3<f32>(1.0) - mx) / max(mx, vec3<f32>(1e-4)), vec3<f32>(0.0), vec3<f32>(1.0));
    w = (vec3<f32>(1.0) - (vec3<f32>(1.0) - w) * (vec3<f32>(1.0) - w)) * (-1.0 / mix(8.0, 5.0, CAS_SHARPNESS));
    let g = clamp(((nw + ne + sw + se) * w + f) / (vec3<f32>(1.0) + 4.0 * w), vec3<f32>(0.0), vec3<f32>(1.0));
    var c = clamp((g * g * R2020_TO_XYZ) * XYZ_TO_709, vec3<f32>(0.0), vec3<f32>(1.0));
    let k = uv.x * uv.y;
    c = c * pow(max(16.0 * (k - uv.x) * (k - uv.y), 1e-4), VIGNETTE);
    // The swap chain is sRGB on most phones (the hardware encodes); otherwise encode here.
    if !SRGB_SURFACE { c = select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, 12.92 * c, c <= vec3<f32>(0.0031308)); }
    return vec4<f32>(c, 1.0);
}
"#;

/// The shader with its two switches spliced in.
fn shader_source(srgb: bool, fxaa: bool) -> String {
    SHADER.replace("@SRGB@", &srgb.to_string()).replace("@FXAA@", &fxaa.to_string())
}

pub struct Present {
    /// The 1-sample scene the final pass reads (the resolve target when multisampled).
    scene: TextureView,
    /// The multisampled colour the scene pass draws into; never stored (`StoreOp::Discard`), so it stays in tile memory.
    msaa: Option<TextureView>,
    fin: RenderPipeline,
    layout: wgpu::BindGroupLayout,
    samp: wgpu::Sampler,
    ubuf: Buffer,
    bind: BindGroup,
}

fn target(device: &Device, label: &str, (w, h): (u32, u32), samples: u32) -> TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format: SCENE_FORMAT,
            // The multisampled one is never read; the 1-sample one is the final pass's input.
            usage: if samples > 1 { wgpu::TextureUsages::RENDER_ATTACHMENT } else { wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING },
            view_formats: &[],
        })
        .create_view(&Default::default())
}

impl Present {
    /// `scene` = the internal size (`Gpu::scene`), `surface` = the swap chain's.
    pub fn new(device: &Device, queue: &Queue, scene: (u32, u32), surface: (u32, u32), surface_format: wgpu::TextureFormat) -> Self {
        let src = shader_source(surface_format.is_srgb(), scene_samples() == 1);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("present"), source: wgpu::ShaderSource::Wgsl(src.into()) });
        let entry = |binding, visibility, ty| wgpu::BindGroupLayoutEntry { binding, visibility, ty, count: None };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("present_layout"),
            entries: &[
                entry(0, wgpu::ShaderStages::FRAGMENT, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }),
                entry(1, wgpu::ShaderStages::FRAGMENT, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
                entry(2, wgpu::ShaderStages::FRAGMENT, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }),
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("present_pl"), bind_group_layouts: &[&layout], push_constant_ranges: &[] });
        let fin = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fs_fin"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs_fin"), targets: &[Some(surface_format.into())], compilation_options: Default::default() }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let samp = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, address_mode_u: wgpu::AddressMode::ClampToEdge, address_mode_v: wgpu::AddressMode::ClampToEdge, ..Default::default() });
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("present_u"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let (scene_v, msaa, bind) = Self::size(device, queue, &layout, &samp, &ubuf, scene, surface);
        Self { scene: scene_v, msaa, fin, layout, samp, ubuf, bind }
    }

    /// Targets, bind group and the size uniform for a (new) size.
    fn size(device: &Device, queue: &Queue, layout: &wgpu::BindGroupLayout, samp: &wgpu::Sampler, ubuf: &Buffer, scene: (u32, u32), surface: (u32, u32)) -> (TextureView, Option<TextureView>, BindGroup) {
        let samples = scene_samples();
        let s = target(device, "scene", scene, 1);
        let msaa = (samples > 1).then(|| target(device, "scene_msaa", scene, samples));
        let px = [1.0 / scene.0.max(1) as f32, 1.0 / scene.1.max(1) as f32, 1.0 / surface.0.max(1) as f32, 1.0 / surface.1.max(1) as f32];
        queue.write_buffer(ubuf, 0, bytemuck::bytes_of(&px));
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("present_bind"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(samp) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&s) },
            ],
        });
        (s, msaa, bind)
    }

    pub fn resize(&mut self, device: &Device, queue: &Queue, scene: (u32, u32), surface: (u32, u32)) {
        (self.scene, self.msaa, self.bind) = Self::size(device, queue, &self.layout, &self.samp, &self.ubuf, scene, surface);
    }

    /// The 1-sample scene (what the final pass reads). The scene pass draws into `scene_attachment` instead.
    pub fn scene_view(&self) -> &TextureView {
        &self.scene
    }

    /// The scene pass's colour attachment cleared to `clear`: the multisampled target resolved into the scene and thrown away, or (1
    /// sample) the scene itself, stored.
    pub fn scene_attachment(&self, clear: wgpu::Color) -> wgpu::RenderPassColorAttachment<'_> {
        match &self.msaa {
            Some(m) => wgpu::RenderPassColorAttachment { view: m, resolve_target: Some(&self.scene), depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Discard } },
            None => wgpu::RenderPassColorAttachment { view: &self.scene, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store } },
        }
    }

    /// The final pass; `hud` draws into it, over the finished frame (so the HUD is neither sharpened nor vignetted).
    pub fn finish(&self, enc: &mut wgpu::CommandEncoder, out: &TextureView, hud: impl FnOnce(&mut wgpu::RenderPass<'_>)) {
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("present"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: out, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        rp.set_pipeline(&self.fin);
        rp.set_bind_group(0, &self.bind, &[]);
        rp.draw(0..3, 0..1);
        hud(&mut rp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant (sRGB swap chain or not, FXAA in or out) parses and validates; no GPU needed.
    #[test]
    fn present_shader_validates() {
        for srgb in [true, false] {
            for fxaa in [true, false] {
                let src = shader_source(srgb, fxaa);
                let m = naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&src)));
                naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&m).unwrap();
            }
        }
    }

    /// One full-screen pass, no intermediate texture: the pass pair this replaced is gone.
    #[test]
    fn the_present_chain_is_one_pass() {
        let src = include_str!("present.rs");
        assert!(!src.contains(concat!("fn ", "fs_post")) && !src.contains(concat!("ldr", ": TextureView")));
        assert_eq!(src.matches(concat!("begin_render", "_pass(")).count(), 1);
    }

    /// Alpha-to-coverage only means something with samples; with one it must stay off (wgpu accepts it, GLES drivers may not).
    #[test]
    fn coverage_needs_samples() {
        assert!(!multisample_of(1, true).alpha_to_coverage_enabled);
        assert!(multisample_of(4, true).alpha_to_coverage_enabled && !multisample_of(4, false).alpha_to_coverage_enabled);
    }
}
