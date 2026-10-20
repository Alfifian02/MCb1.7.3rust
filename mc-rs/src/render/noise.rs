//! Photon's `image/noise.png` (512x512 RGBA8, uploaded as-is, not sRGB): the `noisetex` of the pack. GTAO reads `.b` for its
//! dither, the water waves read `.y`. One texture, shared.

use wgpu::{Device, Queue, TextureView};

const NOISE: &[u8] = include_bytes!("../../assets/noise.rgba");
pub const SIZE: u32 = 512;

pub fn create(device: &Device, queue: &Queue) -> TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("noisetex"),
        size: wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        NOISE,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(SIZE * 4), rows_per_image: Some(SIZE) },
        wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
    );
    tex.create_view(&wgpu::TextureViewDescriptor::default())
}
