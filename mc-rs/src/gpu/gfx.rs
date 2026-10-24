//! What the world code (chunk pages, far terrain, entity and outline buffers) needs of a renderer: make a buffer, write a buffer. The
//! frame owns either wgpu (Vulkan) or the OpenGL ES renderer (`gles`); `Buf` and `Gfx` are the two-way switch, so `ChunkManager`, `Lod`,
//! `ItemMesh` and `Outline` build the same data for both and only the draw loops differ.

use crate::gles;
use wgpu::util::DeviceExt;

pub enum Buf {
    Wgpu(wgpu::Buffer),
    Gl(gles::Buf),
}

impl Buf {
    /// The wgpu buffer (the wgpu draw loops only ever see wgpu buffers).
    pub fn wgpu(&self) -> &wgpu::Buffer {
        match self {
            Buf::Wgpu(b) => b,
            Buf::Gl(_) => unreachable!("a GL buffer in a wgpu pass"),
        }
    }

    /// The GL buffer (likewise).
    pub fn gl(&self) -> &gles::Buf {
        match self {
            Buf::Gl(b) => b,
            Buf::Wgpu(_) => unreachable!("a wgpu buffer in a GL pass"),
        }
    }
}

#[derive(Clone, Copy)]
pub enum Gfx<'a> {
    Wgpu(&'a wgpu::Device, &'a wgpu::Queue),
    Gl(&'a gles::Gl),
}

impl Gfx<'_> {
    /// A static vertex buffer holding `bytes`.
    pub fn vertex(&self, bytes: &[u8]) -> Buf {
        match self {
            Gfx::Wgpu(d, _) => Buf::Wgpu(d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("vbuf"), contents: bytes, usage: wgpu::BufferUsages::VERTEX })),
            Gfx::Gl(g) => Buf::Gl(g.buffer(bytes.len(), bytes, gles::sys::STATIC_DRAW)),
        }
    }

    /// A static index buffer holding `bytes` (u32 indices).
    pub fn index(&self, bytes: &[u8]) -> Buf {
        match self {
            Gfx::Wgpu(d, _) => Buf::Wgpu(d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("ibuf"), contents: bytes, usage: wgpu::BufferUsages::INDEX })),
            Gfx::Gl(g) => Buf::Gl(g.buffer(bytes.len(), bytes, gles::sys::STATIC_DRAW)),
        }
    }

    /// An empty buffer of `size` bytes that `write` / `stream` fill.
    pub fn writable(&self, size: u64, index: bool) -> Buf {
        match self {
            Gfx::Wgpu(d, _) => Buf::Wgpu(d.create_buffer(&wgpu::BufferDescriptor {
                label: Some("dyn_buf"),
                size,
                usage: (if index { wgpu::BufferUsages::INDEX } else { wgpu::BufferUsages::VERTEX }) | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })),
            Gfx::Gl(g) => Buf::Gl(g.buffer(size as usize, &[], gles::sys::DYNAMIC_DRAW)),
        }
    }

    pub fn write(&self, b: &Buf, offset: u64, bytes: &[u8]) {
        match self {
            Gfx::Wgpu(_, q) => q.write_buffer(b.wgpu(), offset, bytes),
            Gfx::Gl(g) => g.write(b.gl(), offset, bytes),
        }
    }

    /// Rewrites a per-frame buffer of `size` bytes from its start (GL orphans the store first).
    pub fn stream(&self, b: &Buf, size: u64, bytes: &[u8]) {
        match self {
            Gfx::Wgpu(_, q) => q.write_buffer(b.wgpu(), 0, bytes),
            Gfx::Gl(g) => g.stream(b.gl(), size as usize, bytes),
        }
    }
}

/// Everything one world frame draws, whichever renderer draws it (`App::render_frame` builds it, `Wg::draw_world` / `gles::renderer::Renderer::draw_world`
/// consume it): the CPU side (culling, meshes, uniforms' inputs) is shared, only the passes differ.
pub struct WorldDraw<'a> {
    pub chunks: &'a crate::world::chunks::ChunkManager,
    /// The far terrain, only when it is drawn this frame.
    pub lod: Option<&'a crate::world::lod::Lod>,
    pub items: &'a crate::render::items::ItemMesh,
    pub item_indices: u32,
    pub outline: &'a crate::render::outline::Outline,
    pub outline_indices: u32,
    /// Halos and motes (`render::look`): the sprite buffers and how many indices of them to draw (0 = none this frame).
    pub glow: &'a crate::render::look::GlowMesh,
    pub glow_indices: u32,
    /// The frame's look: the grade of the final pass and the chunk shaders' tones (`Look::OFF` changes nothing).
    pub look: crate::render::look::Look,
    pub hud: &'a [crate::render::hud::HudVertex],
    pub camera: &'a crate::render::camera::FirstPersonCamera,
    pub view: glam::Mat4,
    pub proj: glam::Mat4,
    pub eye: glam::Vec3,
    /// `ChunkPipeline::frame`: sky light taken away / 15, fog colour, far plane (0 = off, < 0 = exp), where the fog starts, water radius.
    pub sky_sub: f32,
    pub fog: [f32; 3],
    pub far: f32,
    pub start: f32,
    pub wr: f32,
    pub nether: bool,
    /// `SkyRenderer`: the celestial angle, rain, seconds of play, dome colour (flash applied) and horizon (fog) colour.
    pub angle: f32,
    pub rain: f32,
    pub seconds: f32,
    pub zenith: [f32; 3],
    pub horizon: [f32; 3],
    /// The biome's haze, clouds, star tint and fog density (`world::atmos`); `Frame::OFF` in the Nether.
    pub atmos: crate::world::atmos::Frame,
    /// Rain and snow quads, when it rains (`weather::build`), and the sun's height for their colour.
    pub precip: Option<(&'a [crate::render::weather::Vertex], &'a [crate::render::weather::Vertex], f32)>,
}

/// What a world frame came to.
pub enum Drawn {
    /// Presented; the time spent waiting for the swap chain / vsync (not CPU work).
    Done(std::time::Duration),
    /// The surface is not the size the targets were made for: resize, draw next frame.
    Resize(u32, u32),
    /// Nothing was presented (the surface was lost).
    Skipped,
}
