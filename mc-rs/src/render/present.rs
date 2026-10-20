//! Present chain, the whole post stage (it replaces the Photon HDR chain: exposure, bloom, TAA, grade, DOF...). The world is drawn into an
//! 8-bit sRGB `scene` at `RENDER_SCALE` (linear Rec. 2020, what the sky and chunk shaders write; the hardware encodes and decodes sRGB), then
//!  - pass A, scene size: FXAA (3.9 console variant: 4 corner taps, 4 along the edge, none on flat areas) + CAS (sharpens what the
//!    upscale softens, on the same taps) -> `ldr`, linear Rec. 709. Both work in `sqrt` space, the cheap perceptual one.
//!  - pass B, surface size: bilinear upscale + vignette onto the swap chain, and the HUD is drawn into the same pass.

use wgpu::{BindGroup, Buffer, Device, Queue, RenderPipeline, TextureView};

/// Format of the scene and the intermediate `ldr`: 4 bytes a pixel (the old HDR scene was 8, with a mip chain and 5 full-screen passes).
pub const SCENE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

const SHADER: &str = r#"
struct U { px: vec4<f32> }; // 1 / scene size, 1 / surface size
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var src: texture_2d<f32>;

const SRGB_SURFACE: bool = @SRGB@;
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

@fragment
fn fs_post(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let o = u.px.xy;
    let uv = p.xy * o;
    let m = tap(uv);
    let nw = tap(uv + vec2<f32>(-o.x, -o.y));
    let ne = tap(uv + vec2<f32>(o.x, -o.y));
    let sw = tap(uv + vec2<f32>(-o.x, o.y));
    let se = tap(uv + vec2<f32>(o.x, o.y));
    let l_nw = luma(nw); let l_ne = luma(ne); let l_sw = luma(sw); let l_se = luma(se);
    let lo = min(luma(m), min(min(l_nw, l_ne), min(l_sw, l_se)));
    let hi = max(luma(m), max(max(l_nw, l_ne), max(l_sw, l_se)));
    var f = m;
    if hi - lo >= max(0.04, hi * 0.125) { // an edge: blur along it (FXAA 3.9 console)
        var dir = vec2<f32>(-((l_nw + l_ne) - (l_sw + l_se)), (l_nw + l_sw) - (l_ne + l_se));
        let reduce = max((l_nw + l_ne + l_sw + l_se) * (0.25 * 0.125), 1.0 / 128.0);
        dir = clamp(dir / (min(abs(dir.x), abs(dir.y)) + reduce), vec2<f32>(-8.0), vec2<f32>(8.0)) * o;
        let a = 0.5 * (tap(uv + dir * (1.0 / 3.0 - 0.5)) + tap(uv + dir * (2.0 / 3.0 - 0.5)));
        let b = a * 0.5 + 0.25 * (tap(uv - dir * 0.5) + tap(uv + dir * 0.5));
        f = select(b, a, luma(b) < lo || luma(b) > hi);
    }
    // CAS (FidelityFX): the sharpening weight falls where the neighbourhood has no headroom to the signal limits; the kernel is the
    // four corner taps already fetched (a rotated cross), the centre is the FXAA result.
    let mn = min(min(min(nw, ne), min(sw, se)), f);
    let mx = max(max(max(nw, ne), max(sw, se)), f);
    var w = clamp(min(mn, vec3<f32>(1.0) - mx) / max(mx, vec3<f32>(1e-4)), vec3<f32>(0.0), vec3<f32>(1.0));
    w = (vec3<f32>(1.0) - (vec3<f32>(1.0) - w) * (vec3<f32>(1.0) - w)) * (-1.0 / mix(8.0, 5.0, CAS_SHARPNESS));
    let g = clamp(((nw + ne + sw + se) * w + f) / (vec3<f32>(1.0) + 4.0 * w), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(clamp((g * g * R2020_TO_XYZ) * XYZ_TO_709, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}

@fragment
fn fs_fin(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = p.xy * u.px.zw;
    let k = uv.x * uv.y;
    var c = textureSampleLevel(src, samp, uv, 0.0).rgb * pow(max(16.0 * (k - uv.x) * (k - uv.y), 1e-4), VIGNETTE);
    // The swap chain is sRGB on most phones (the hardware encodes); otherwise encode here.
    if !SRGB_SURFACE { c = select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, 12.92 * c, c <= vec3<f32>(0.0031308)); }
    return vec4<f32>(c, 1.0);
}
"#;

pub struct Present {
    scene: TextureView,
    ldr: TextureView,
    post: RenderPipeline,
    fin: RenderPipeline,
    layout: wgpu::BindGroupLayout,
    samp: wgpu::Sampler,
    ubuf: Buffer,
    bind_a: BindGroup,
    bind_b: BindGroup,
}

fn target(device: &Device, label: &str, (w, h): (u32, u32)) -> TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SCENE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

impl Present {
    /// `scene` = the internal size (`Gpu::scene`), `surface` = the swap chain's.
    pub fn new(device: &Device, queue: &Queue, scene: (u32, u32), surface: (u32, u32), surface_format: wgpu::TextureFormat) -> Self {
        let src = SHADER.replace("@SRGB@", if surface_format.is_srgb() { "true" } else { "false" });
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
        let pipe = |fs: &str, format: wgpu::TextureFormat| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(fs),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some(fs), targets: &[Some(format.into())], compilation_options: Default::default() }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let samp = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, address_mode_u: wgpu::AddressMode::ClampToEdge, address_mode_v: wgpu::AddressMode::ClampToEdge, ..Default::default() });
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("present_u"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let (scene_v, ldr, bind_a, bind_b) = Self::size(device, queue, &layout, &samp, &ubuf, scene, surface);
        Self { scene: scene_v, ldr, post: pipe("fs_post", SCENE_FORMAT), fin: pipe("fs_fin", surface_format), layout, samp, ubuf, bind_a, bind_b }
    }

    /// Targets, bind groups and the size uniform for a (new) size.
    fn size(device: &Device, queue: &Queue, layout: &wgpu::BindGroupLayout, samp: &wgpu::Sampler, ubuf: &Buffer, scene: (u32, u32), surface: (u32, u32)) -> (TextureView, TextureView, BindGroup, BindGroup) {
        let (s, l) = (target(device, "scene", scene), target(device, "ldr", scene));
        let px = [1.0 / scene.0.max(1) as f32, 1.0 / scene.1.max(1) as f32, 1.0 / surface.0.max(1) as f32, 1.0 / surface.1.max(1) as f32];
        queue.write_buffer(ubuf, 0, bytemuck::bytes_of(&px));
        let bind = |v: &TextureView| device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("present_bind"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(samp) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(v) },
            ],
        });
        let (a, b) = (bind(&s), bind(&l));
        (s, l, a, b)
    }

    pub fn resize(&mut self, device: &Device, queue: &Queue, scene: (u32, u32), surface: (u32, u32)) {
        (self.scene, self.ldr, self.bind_a, self.bind_b) = Self::size(device, queue, &self.layout, &self.samp, &self.ubuf, scene, surface);
    }

    /// Where the world is drawn.
    pub fn scene_view(&self) -> &TextureView {
        &self.scene
    }

    /// Both passes; `hud` draws into pass B, over the finished frame (so the HUD is neither sharpened nor vignetted).
    pub fn finish(&self, enc: &mut wgpu::CommandEncoder, out: &TextureView, hud: impl FnOnce(&mut wgpu::RenderPass<'_>)) {
        let pass = |enc: &mut wgpu::CommandEncoder, view: &TextureView, pipe: &RenderPipeline, bind: &BindGroup, f: &mut dyn FnMut(&mut wgpu::RenderPass<'_>)| {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("present"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rp.set_pipeline(pipe);
            rp.set_bind_group(0, bind, &[]);
            rp.draw(0..3, 0..1);
            f(&mut rp);
        };
        pass(enc, &self.ldr, &self.post, &self.bind_a, &mut |_| {});
        let mut hud = Some(hud);
        pass(enc, out, &self.fin, &self.bind_b, &mut |rp| hud.take().map_or((), |h| h(rp)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both variants (sRGB swap chain or not) parse and validate; no GPU needed.
    #[test]
    fn present_shader_validates() {
        for srgb in ["true", "false"] {
            let src = SHADER.replace("@SRGB@", srgb);
            let m = naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&src)));
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&m).unwrap();
        }
    }
}
