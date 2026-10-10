//! Headless check of the shadow pass (needs a Vulkan/GL adapter, e.g. lavapipe; skipped without one).
use super::{create_index_buffer, create_vertex_buffer, ChunkPipeline, Vertex};
use crate::render::atlas::{atlas_uv, terrain_uv};
use glam::{Mat4, Vec3};

/// `tile` 31 stands for tall grass: its terrain tile, which the shadow pass skips; any other tile is a flat colour.
fn render(strength: f32, tile: u16) -> Option<Vec<f32>> {
    let inst = wgpu::Instance::default();
    let ad = pollster::block_on(inst.request_adapter(&Default::default())).ok()?;
    let (dev, q) = pollster::block_on(ad.request_device(&Default::default())).ok()?;
    let fmt = wgpu::TextureFormat::Rgba8Unorm;
    let pipe = ChunkPipeline::new(&dev, fmt);
    pipe.upload_atlas(&q);
    let quad = |t: u16, a, b, c, d| { let (x, y) = if t == 31 { terrain_uv(39, 0.5, 0.5) } else { atlas_uv(t, 0.0, 0.0) }; [a, b, c, d].map(|p| Vertex { pos: p, uv: [x, y], light: 1.0 }) };
    // 1x1 faces like the real mesher (the distort is per vertex, so huge triangles would interpolate it wrongly).
    let mut verts = vec![];
    for x in -40..40 { for z in -40..40 { let (x, z) = (x as f32, z as f32); verts.extend(quad(2, [x, 0., z], [x + 1., 0., z], [x + 1., 0., z + 1.], [x, 0., z + 1.])); } }
    let ground = verts.len() as u32 / 4 * 12;
    for x in -30..30 { for y in 0..6 { let (x, y) = (x as f32, y as f32); verts.extend(quad(tile, [x, y, 5.], [x + 1., y, 5.], [x + 1., y + 1., 5.], [x, y + 1., 5.])); } }
    let idx: Vec<u32> = (0..verts.len() as u32 / 4).flat_map(|f| { let b = f * 4; [b, b + 1, b + 2, b, b + 2, b + 3, b, b + 2, b + 1, b, b + 3, b + 2] }).collect();
    let (vb, ib) = (create_vertex_buffer(&dev, &verts), create_index_buffer(&dev, &idx));
    let eye = Vec3::new(0.0, 40.0, -4.0);
    let view = Mat4::look_at_rh(eye, Vec3::new(0.0, 0.0, -4.0), Vec3::Z);
    let proj = Mat4::perspective_rh(50f32.to_radians(), 1.0, 0.1, 200.0);
    let sun = Vec3::new(0.0, 0.6, 0.8); // toward +Z, 37 degrees up
    pipe.upload_uniforms(&q, view, proj, sun, strength, eye);
    let target = dev.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: fmt, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC, view_formats: &[] });
    let depth = dev.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Depth32Float, usage: wgpu::TextureUsages::RENDER_ATTACHMENT, view_formats: &[] });
    let buf = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: 64 * 64 * 4, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut enc = dev.create_command_encoder(&Default::default());
    let draw = |rp: &mut wgpu::RenderPass, n: u32| { rp.set_bind_group(0, &pipe.bind_group, &[]); rp.set_vertex_buffer(0, vb.slice(..)); rp.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32); rp.draw_indexed(0..n, 0, 0..1); };
    if strength > 0.0 {
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: None, color_attachments: &[], depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &pipe.shadow_view, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None }), timestamp_writes: None, occlusion_query_set: None });
        rp.set_pipeline(&pipe.shadow_pipeline);
        draw(&mut rp, idx.len() as u32);
    }
    {
        let tv = target.create_view(&Default::default());
        let dv = depth.create_view(&Default::default());
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: None, color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &tv, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })], depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &dv, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None }), timestamp_writes: None, occlusion_query_set: None });
        rp.set_pipeline(&pipe.pipeline);
        rp.set_bind_group(1, &pipe.shadow_bind, &[]);
        draw(&mut rp, ground); // ground only: the wall is seen by the sun alone
    }
    enc.copy_texture_to_buffer(target.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(256), rows_per_image: Some(64) } }, wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 });
    q.submit([enc.finish()]);
    buf.slice(..).map_async(wgpu::MapMode::Read, |_| ());
    dev.poll(wgpu::PollType::Wait).unwrap();
    let d = buf.slice(..).get_mapped_range();
    Some(d.chunks(4).map(|p| p[1] as f32).collect()) // green channel
}

#[test]
fn wall_shades_the_ground_away_from_the_sun() {
    let (Some(on), Some(off)) = (render(1.0, 2), render(0.0, 2)) else { return };
    let (mx, mn) = (on.iter().cloned().fold(0., f32::max), on.iter().cloned().fold(255., f32::min));
    let (ox, on_) = (off.iter().cloned().fold(0., f32::max), off.iter().cloned().fold(255., f32::min));
    assert!(ox / on_ < 1.02, "no shadow pass: flat");
    assert!(mx / mn > 1.15, "shadow darkens part of the ground");
    // Foliage (tile 31, tall grass) casts no shadow: no ground pixel keeps the shadowed value (77) when the wall is foliage.
    let dark = |v: &[f32]| v.iter().filter(|&&x| (74.0..=80.0).contains(&x)).count();
    let plant = render(1.0, 31).unwrap();
    assert!(dark(&on) > 100 && dark(&plant) == 0, "foliage casts nothing");
}
