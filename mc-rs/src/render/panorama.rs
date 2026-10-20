//! The title screen's panorama (`GuiMainMenu.drawPanorama`, the cube map of 1.8+): six square faces (`assets/panorama.rgba`,
//! `tools/gen_panorama.py`) in one 2D-array texture, drawn by a single full-screen triangle that turns each pixel's view ray into
//! (face, uv). The camera pans (-0.1 degree a tick = 2 a second, as `-panoramaTimer * 0.1`) and the pitch rocks a little.
//! Faces are in `GuiMainMenu`'s order: 0 +z, 1 +x, 2 -z, 3 -x, 4 -y, 5 +y, each as `Rk` turns the quad at z = 1.

use glam::{Mat3, Mat4};
use std::f32::consts::PI;
use std::time::Instant;
use wgpu::*;

/// Side of a face in `PANORAMA` (`N` in `tools/gen_panorama.py`).
const FACE: u32 = 512;
const PANORAMA: &[u8] = include_bytes!("../../assets/panorama.rgba");
/// Vertical field of view in degrees (the `CubeMap` of 1.14+).
const FOV: f32 = 85.0;

const SHADER: &str = r#"
struct U { m: mat4x4<f32>, t: vec4<f32> }; // view ray -> cube direction, tan(fov / 2) per axis
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var tex: texture_2d_array<f32>;
@group(0) @binding(2) var samp: sampler;
struct V { @builtin(position) p: vec4<f32>, @location(0) e: vec2<f32> };

@vertex
fn vs(@builtin(vertex_index) i: u32) -> V {
    let q = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0;
    return V(vec4<f32>(q, 0.0, 1.0), q * u.t.xy);
}

@fragment
fn fs(v: V) -> @location(0) vec4<f32> {
    let w = (u.m * vec4<f32>(v.e, -1.0, 0.0)).xyz;
    let a = abs(w);
    var f = 0u;
    var c = w; // w turned so that the face's own +z points at the pixel
    if a.z >= a.x && a.z >= a.y {
        if w.z < 0.0 { f = 2u; c = vec3<f32>(-w.x, w.y, -w.z); }
    } else if a.x >= a.y {
        if w.x > 0.0 { f = 1u; c = vec3<f32>(-w.z, w.y, w.x); } else { f = 3u; c = vec3<f32>(w.z, w.y, -w.x); }
    } else if w.y > 0.0 { f = 5u; c = vec3<f32>(w.x, -w.z, w.y); } else { f = 4u; c = vec3<f32>(w.x, w.z, -w.y); }
    let uv = (c.xy / c.z + 1.0) * 0.5;
    return vec4<f32>(textureSampleLevel(tex, samp, uv, f, 0.0).rgb, 1.0);
}
"#;

pub struct Panorama {
    pipe: RenderPipeline,
    bind: BindGroup,
    ubuf: Buffer,
    born: Instant,
}

impl Panorama {
    pub fn new(device: &Device, queue: &Queue, surface_format: TextureFormat) -> Self {
        // sRGB data in an sRGB texture: sampling decodes it and an sRGB surface encodes it again (a plain one gets the raw bytes).
        let format = if surface_format.is_srgb() { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm };
        let size = Extent3d { width: FACE, height: FACE, depth_or_array_layers: 6 };
        let tex = device.create_texture(&TextureDescriptor {
            label: Some("panorama"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: Origin3d::ZERO, aspect: TextureAspect::All },
            PANORAMA,
            TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(FACE * 4), rows_per_image: Some(FACE) },
            size,
        );
        let view = tex.create_view(&TextureViewDescriptor { dimension: Some(TextureViewDimension::D2Array), ..Default::default() });
        let samp = device.create_sampler(&SamplerDescriptor { mag_filter: FilterMode::Linear, min_filter: FilterMode::Linear, ..Default::default() });
        let ubuf = device.create_buffer(&BufferDescriptor { label: Some("panorama_u"), size: 80, usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST, mapped_at_creation: false });
        let entry = |binding, visibility, ty| BindGroupLayoutEntry { binding, visibility, ty, count: None };
        let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("panorama_bind"),
            entries: &[
                entry(0, ShaderStages::VERTEX_FRAGMENT, BindingType::Buffer { ty: BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }),
                entry(1, ShaderStages::FRAGMENT, BindingType::Texture { sample_type: TextureSampleType::Float { filterable: true }, view_dimension: TextureViewDimension::D2Array, multisampled: false }),
                entry(2, ShaderStages::FRAGMENT, BindingType::Sampler(SamplerBindingType::Filtering)),
            ],
        });
        let bind = device.create_bind_group(&BindGroupDescriptor {
            label: Some("panorama_bind"),
            layout: &layout,
            entries: &[
                BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                BindGroupEntry { binding: 1, resource: BindingResource::TextureView(&view) },
                BindGroupEntry { binding: 2, resource: BindingResource::Sampler(&samp) },
            ],
        });
        let shader = device.create_shader_module(ShaderModuleDescriptor { label: Some("panorama"), source: ShaderSource::Wgsl(SHADER.into()) });
        let pipe = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("panorama"),
            layout: Some(&device.create_pipeline_layout(&PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&layout], push_constant_ranges: &[] })),
            vertex: VertexState { module: &shader, entry_point: Some("vs"), buffers: &[], compilation_options: PipelineCompilationOptions::default() },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(ColorTargetState { format: surface_format, blend: None, write_mask: ColorWrites::ALL })],
                compilation_options: PipelineCompilationOptions::default(),
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        Self { pipe, bind, ubuf, born: Instant::now() }
    }

    /// Draws the panorama over the whole `w` x `h` target (first in the pass; the HUD goes over it).
    pub fn draw(&self, queue: &Queue, rp: &mut RenderPass, w: f32, h: f32) {
        let t = self.born.elapsed().as_secs_f32();
        // `GuiMainMenu`: rotate(180, x), then pitch, then yaw. Its roll of 90 degrees is not here: it stood the pack's faces on their side.
        // The pitch is 5 degrees down +-10 (vanilla's 20 +-25 looks mostly at the floor in this pack).
        let m = Mat3::from_rotation_x(PI + (5.0 + (t / 20.0).sin() * 10.0).to_radians()) * Mat3::from_rotation_y((-2.0 * t).to_radians());
        let ty = (FOV / 2.0).to_radians().tan();
        let mut u = [0f32; 20];
        u[..16].copy_from_slice(&Mat4::from_mat3(m.transpose()).to_cols_array()); // the shader needs the inverse
        u[16..18].copy_from_slice(&[ty * w / h, ty]);
        queue.write_buffer(&self.ubuf, 0, bytemuck::cast_slice(&u));
        rp.set_pipeline(&self.pipe);
        rp.set_bind_group(0, &self.bind, &[]);
        rp.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn asset_matches_face_and_shader_validates() {
        assert_eq!(super::PANORAMA.len(), 6 * (super::FACE * super::FACE * 4) as usize, "re-run tools/gen_panorama.py with N = FACE");
        let m = naga::front::wgsl::parse_str(super::SHADER).unwrap_or_else(|e| panic!("{}", e.emit_to_string(super::SHADER)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&m).unwrap();
    }
}
