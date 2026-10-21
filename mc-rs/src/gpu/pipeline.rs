//! Chunk pipelines: one atlas, one uniform block, one bind group. Layers (Sodium's render passes): `solid` (no `discard`, so the tiler's
//! hidden-surface removal works), `cutout` (alpha test: leaves, plants, glass), `lod` (cutout + depth bias) and translucent `water`.
//! Everything is lit per VERTEX like b1.7.3 (light table, face shade, fog): the fragment shader is one atlas read and two multiplies.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::{BindGroup, Buffer, BufferUsages, Device, Queue, RenderPipeline, Texture};
use wgpu::util::DeviceExt;

use crate::render::{atlas, present::SCENE_FORMAT};

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub light: f32,
    /// `sky * 16 + block` light levels, see `render::mesh::lm`.
    pub lm: f32,
    /// Packed sRGB colour the texel is multiplied by (`render::mesh::WHITE` = none).
    pub tint: f32,
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
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 28, shader_location: 4 },
        ],
        }
    }
}

/// Layout of the 16-byte terrain vertex (`embeddium::vertex`): x y z + 16 bits of light, u v, colour + shade. Chunk meshes only;
/// entities, the outline and the far terrain keep the float `Vertex` above (world space, their own packing of `light`).
pub fn chunk_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: embeddium::vertex::STRIDE as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Uint16x4, offset: 0, shader_location: 0 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Uint16x2, offset: 8, shader_location: 1 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Unorm8x4, offset: 12, shader_location: 2 },
        ],
    }
}

/// Dynamic-offset stride of one chunk origin (`min_uniform_buffer_offset_alignment` of the downlevel limits, as `post.rs`'s `P_STRIDE`).
pub const ORIGIN_STRIDE: u64 = 256;
/// Chunk meshes alive at once: one origin slot each.
pub const MAX_ORIGINS: u32 = 4096;

/// Bind group 1 of the chunk pipelines: the origin (x, y, z, 0) of the chunk a draw belongs to, Sodium's `u_RegionOffset`. Push
/// constants are not on GLES, so it is a uniform with a dynamic offset: one `set_bind_group` per chunk, no vertex data per draw.
pub struct DrawOrigins {
    buf: Buffer,
    pub group: BindGroup,
}

pub fn origin_layout(device: &Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("chunk_origin_layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: None },
            count: None,
        }],
    })
}

impl DrawOrigins {
    fn new(device: &Device, layout: &wgpu::BindGroupLayout) -> Self {
        let buf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("chunk_origins"), size: ORIGIN_STRIDE * MAX_ORIGINS as u64, usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST, mapped_at_creation: false });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("chunk_origin_bind"),
            layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &buf, offset: 0, size: wgpu::BufferSize::new(16) }) }],
        });
        Self { buf, group }
    }

    pub fn write(&self, queue: &Queue, slot: u32, origin: [f32; 3]) {
        queue.write_buffer(&self.buf, slot as u64 * ORIGIN_STRIDE, bytemuck::bytes_of(&[origin[0], origin[1], origin[2], 0.0f32]));
    }

    pub fn offset(slot: u32) -> u32 {
        (slot as u64 * ORIGIN_STRIDE) as u32
    }
}

/// The frame's block, written once by `ChunkPipeline::frame`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct Uniforms {
    /// `proj * view`.
    pub view_proj: [[f32; 4]; 4],
    /// xyz: eye, w: sky light taken away by the time of day / 15.
    pub eye: [f32; 4],
    /// rgb: fog colour (the frame's linear Rec. 2020), w: far plane (0 = no fog, < 0 = `GL_EXP` with density `-w`, eye in water).
    pub fog: [f32; 4],
    /// x: where the linear fog starts as a fraction of the far plane (0 = a quarter), the far terrain keeps its colours (`lod::FOG_START`).
    /// y: radius (blocks) of the real translucent water (`fs_water`): `WATER_RANGE` with the LoD on, 1e9 (all of it) with the LoD off.
    pub fogp: [f32; 4],
}

const SHADER_SRC: &str = r#"
struct U { vp: mat4x4<f32>, eye: vec4<f32>, fog: vec4<f32>, fogp: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var tex: texture_2d<f32>;

const TW: u32 = @TW@u;
const TH: f32 = @TH@.0;
const MIPS: f32 = @MIPS@.0; // `atlas::MIPS`
const ITEMS_Y: f32 = @IY@.0; // `atlas::ITEMS_Y`: the flat strip is the rows 256..ITEMS_Y
const CHAR_Y: f32 = @CY@.0; // `atlas::CHAR_Y`: the player skin, read at mip 0 (a 16 px tile clamp would shift its 4 px strips)
// b1.7.3 `RenderBlocks` face shades (top 1, bottom 0.5, z 0.8, x 0.6) raised to 2.2: vanilla multiplies in gamma space, the frame is linear.
const S_BOT: f32 = @SB@;
const S_Z: f32 = @SZ@;
const S_X: f32 = @SX@;
const R709_TO_XYZ = mat3x3<f32>(vec3<f32>(0.4124, 0.3576, 0.1805), vec3<f32>(0.2126, 0.7152, 0.0722), vec3<f32>(0.0193, 0.1192, 0.9505));
const XYZ_TO_2020 = mat3x3<f32>(vec3<f32>(1.7166084, -0.3556621, -0.2533601), vec3<f32>(-0.6666829, 1.6164776, 0.0157685), vec3<f32>(0.0176422, -0.0427763, 0.94222867));
fn to_working(c: vec3<f32>) -> vec3<f32> { return (c * R709_TO_XYZ) * XYZ_TO_2020; }

// The one atlas read. A minified tile (distance, a slanted face, `RENDER_SCALE`) is filtered bilinear + trilinear, and a tap of that
// footprint within half a texel of the tile edge reads the NEIGHBOURING tile, so the UV is clamped into its own 16-texel tile by half a
// texel of the coarsest mip the sampler will touch. Magnified (Nearest) it changes nothing. The flat-colour strip is read at mip 0.
// `tools/atlas_bleed_check.py` is the numeric check of this clamp. `textureSampleGrad` needs no uniform control flow.
fn atlas_sample_g(uv: vec2<f32>, gx: vec2<f32>, gy: vec2<f32>) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(tex));
    let p = uv * size;
    if (p.y >= 256.0 && p.y < ITEMS_Y) || p.y >= CHAR_Y { return textureSampleLevel(tex, samp, uv, 0.0); }
    let g = max(length(gx * size), length(gy * size)); // texels of mip 0 per pixel = 2^lod
    let pad = 0.5 * exp2(min(ceil(log2(max(g, 1.0))), MIPS - 1.0));
    let o = floor(p / 16.0) * 16.0;
    return textureSampleGrad(tex, samp, clamp(p, o + vec2<f32>(pad), o + vec2<f32>(16.0 - pad)) / size, gx, gy);
}

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) tint: vec3<f32>, // linear colour of the biome / leaf tint
    @location(2) lit: f32,        // light table (x face shade x ambient occlusion on smooth faces), linear
    @location(3) fg: f32,         // fog factor
    @location(4) k: f32,          // < 0: smooth face (shade already in `lit`); >= 0: flat face, shade from the screen; >= 1000: far terrain / water sheet (tile = k - 1000)
    @location(5) wp: vec3<f32>,
};

// b1.7.3 `setupFog`: GL_LINEAR from a quarter of the far plane to the far plane (`fogp.x` moves the start); GL_EXP under water.
fn fog_factor(d: f32) -> f32 {
    if u.fog.w < 0.0 { return 1.0 - exp(u.fog.w * d); }
    if u.fog.w == 0.0 { return 0.0; }
    let s0 = select(0.25, u.fogp.x, u.fogp.x > 0.0);
    return clamp((d - s0 * u.fog.w) / ((1.0 - s0) * u.fog.w), 0.0, 1.0);
}

// `World.lightBrightnessTable`; `sky` and `blk` are light levels / 15, `u.eye.w` the sky light the time of day takes away.
fn brightness(sky: f32, blk: f32) -> f32 {
    let l = max(max(sky - u.eye.w, 0.0), blk);
    return (l / (4.0 - 3.0 * l)) * 0.95 + 0.05;
}

fn vert(vpos: vec3<f32>, vuv: vec2<f32>, vlight: f32, vlm: f32, vtint: f32, water: bool) -> VsOut {
    var o: VsOut;
    let tp = u32(vtint); // exact: the packed colour is an integer below 2^24 (`+ 0.5` would round `WHITE` up to 2^24 = black)
    o.tint = pow(vec3<f32>(f32((tp >> 16u) & 255u), f32((tp >> 8u) & 255u), f32(tp & 255u)) / 255.0, vec3<f32>(2.2));
    o.pos = u.vp * vec4<f32>(vpos, 1.0);
    o.uv = vuv;
    o.wp = vpos;
    o.fg = fog_factor(length(vpos - u.eye.xyz));
    if vlm >= 256.0 && !water { // smooth vertex (`mesh::SMOOTH_LM`): sky in quarter levels, block in half levels, `light` = shade x ambient occlusion
        let v = vlm - 256.0;
        let s = floor(v / 32.0 + 0.001);
        o.lit = pow(brightness(s / 60.0, (v - s * 32.0) / 30.0) * vlight, 2.2);
        o.k = -1.0;
    } else { // flat: `sky * 16 + block`, the face shade comes from the screen (`flat_shade`)
        let s = floor(vlm / 16.0 + 0.001);
        o.lit = pow(brightness(s / 15.0, (vlm - s * 16.0) / 15.0), 2.2);
        o.k = vlight;
    }
    return o;
}

@vertex
fn vs_main(@location(0) p: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) l: f32, @location(3) lm: f32, @location(4) t: f32) -> VsOut {
    return vert(p, uv, l, lm, t, false);
}
@vertex
fn vs_water(@location(0) p: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) l: f32, @location(3) lm: f32, @location(4) t: f32) -> VsOut {
    return vert(p, uv, l, lm, t, true);
}

// The 16-byte chunk vertex (`embeddium::vertex`, Sodium's `CompactChunkVertex`): x, y, z are fixed point relative to the chunk origin of
// `draw` (x, z: 1/2048 from -8; y: 1/256 from -8), `p.w` is the light (bit 15: smooth vertex, then sky quarters << 5 | block halves;
// else sky << 4 | block), `t` the atlas uv x 65536, `c` the tint and, for smooth vertices, shade x ambient occlusion in `c.a`.
struct Draw { origin: vec4<f32> };
@group(1) @binding(0) var<uniform> draw: Draw;

fn chunk_vert(p: vec4<u32>, t: vec2<u32>, c: vec4<f32>, water: bool) -> VsOut {
    var o: VsOut;
    let vpos = draw.origin.xyz + vec3<f32>(f32(p.x) / 2048.0 - 8.0, f32(p.y) / 256.0 - 8.0, f32(p.z) / 2048.0 - 8.0);
    o.tint = pow(c.rgb, vec3<f32>(2.2));
    o.pos = u.vp * vec4<f32>(vpos, 1.0);
    o.uv = vec2<f32>(t) / 65536.0;
    o.wp = vpos;
    o.fg = fog_factor(length(vpos - u.eye.xyz));
    if (p.w & 0x8000u) != 0u && !water {
        let v = p.w & 0x7FFFu;
        o.lit = pow(brightness(f32(v >> 5u) / 60.0, f32(v & 31u) / 30.0) * c.a, 2.2);
        o.k = -1.0;
    } else {
        o.lit = pow(brightness(f32((p.w >> 4u) & 15u) / 15.0, f32(p.w & 15u) / 15.0), 2.2);
        o.k = 0.0;
    }
    return o;
}

@vertex
fn vs_chunk(@location(0) p: vec4<u32>, @location(1) t: vec2<u32>, @location(2) c: vec4<f32>) -> VsOut {
    return chunk_vert(p, t, c, false);
}
@vertex
fn vs_chunk_water(@location(0) p: vec4<u32>, @location(1) t: vec2<u32>, @location(2) c: vec4<f32>) -> VsOut {
    return chunk_vert(p, t, c, true);
}

// The face shade of a flat (non-smooth) quad from the derivatives of its world position (every face is axis aligned).
// Derivatives are taken by the caller, in uniform control flow.
fn flat_shade(wp: vec3<f32>, dx: vec3<f32>, dy: vec3<f32>) -> f32 {
    let c = cross(dx, dy);
    if dot(c, c) < 1e-20 { return 1.0; }
    let a = abs(c);
    if a.x >= a.y && a.x >= a.z { return S_X; }
    if a.y >= a.z { return select(S_BOT, 1.0, c.y * dot(c, u.eye.xyz - wp) > 0.0); }
    return S_Z;
}

// Far terrain (`world::lod`, `k >= 1000`) and its water sheet: the quad's uv is in BLOCKS, v up, and `k - 1000` is its terrain tile. The
// tile repeats once per block (Voxy's `modf` tiling), not once per voxel, so a cell of 4 or 8 blocks shows the texture at block scale.
fn far_uv(uv: vec2<f32>, k: f32) -> vec2<f32> {
    let size = vec2<f32>(textureDimensions(tex));
    let t = floor(k - 1000.0 + 0.5);
    let o = vec2<f32>(t % 16.0, floor(t / 16.0)) * 16.0;
    return (o + clamp(vec2<f32>(fract(uv.x), 1.0 - fract(uv.y)) * 16.0, vec2<f32>(0.02), vec2<f32>(15.98))) / size;
}

fn frag(in: VsOut, cut: bool) -> vec4<f32> {
    let dx = dpdx(in.wp);
    let dy = dpdy(in.wp);
    var uv = in.uv;
    var gx = dpdx(in.uv);
    var gy = dpdy(in.uv);
    if in.k >= 1000.0 {
        uv = far_uv(in.uv, in.k);
        let s = vec2<f32>(textureDimensions(tex));
        gx = gx * 16.0 / s;
        gy = gy * 16.0 / s;
    }
    let c = atlas_sample_g(uv, gx, gy);
    // Cut-out textures: alpha from mip 0 so a distant plant keeps its shape (the mips average alpha). A tile that is not minified (at
    // most one texel of mip 0 per pixel: everything near, and leaves fill most of a forest view) is read at mip 0 with the Nearest
    // magnification filter already, and the tile clamp moves the uv by under half a texel, inside the same texel: `c.a` is that alpha,
    // so the second fetch is only made for a minified tile.
    if cut {
        let size = vec2<f32>(textureDimensions(tex));
        var a = c.a;
        if max(length(gx * size), length(gy * size)) > 1.0 { a = textureSampleLevel(tex, samp, uv, 0.0).a; }
        if a < 0.5 { discard; }
    }
    var lit = in.lit;
    if in.k >= 0.0 { lit = lit * flat_shade(in.wp, dx, dy); }
    var rgb = c.rgb * in.tint;
    // Grass side (tile 3, untinted): `RenderBlocks` draws tile 38 over it, cut out and tinted with the grass colour. Done on the one quad:
    // two coplanar quads can only be told apart by the depth test, which on a phone GPU flickers at a grazing angle.
    if u32(uv.y * TH) * TW + u32(uv.x * f32(TW)) == 3u {
        let ouv = uv + vec2<f32>(3.0 / f32(TW), 2.0 / TH);
        rgb = c.rgb;
        if textureSampleLevel(tex, samp, ouv, 0.0).a >= 0.5 { rgb = atlas_sample_g(ouv, gx, gy).rgb * in.tint; }
    }
    return vec4<f32>(mix(to_working(rgb * lit), u.fog.rgb, in.fg), 1.0);
}

@fragment
fn fs_solid(in: VsOut) -> @location(0) vec4<f32> { return frag(in, false); }
@fragment
fn fs_cut(in: VsOut) -> @location(0) vec4<f32> { return frag(in, true); }

// Translucent water: the texture's own alpha (b1.7.3 and the consoles: no depth absorption, no grazing reflection). A far terrain sheet
// (`lod::SHEET_LIGHT`, `k >= 1000`) is the water beyond the real water's radius; the real water is inside it. Exact complements per pixel,
// so there is never a hole or a double layer between them.
@fragment
fn fs_water(in: VsOut) -> @location(0) vec4<f32> {
    let dx = dpdx(in.wp);
    let dy = dpdy(in.wp);
    let sheet = in.k >= 1000.0;
    if (length(in.wp - u.eye.xyz) > u.fogp.y) != sheet { discard; }
    var uv = in.uv;
    var gx = dpdx(in.uv);
    var gy = dpdy(in.uv);
    if sheet {
        uv = far_uv(in.uv, in.k);
        let s = vec2<f32>(textureDimensions(tex));
        gx = gx * 16.0 / s;
        gy = gy * 16.0 / s;
    }
    var c = atlas_sample_g(uv, gx, gy);
    if sheet { c = vec4<f32>(c.rgb * in.tint, c.a); } // white, but cyan in the LoD debug mode
    return vec4<f32>(mix(to_working(c.rgb * in.lit * flat_shade(in.wp, dx, dy)), u.fog.rgb, in.fg), c.a);
}
"#;

/// The chunk shader with its constants spliced in.
fn shader_source() -> String {
    SHADER_SRC
        .replace("@TW@", &atlas::TILES_W.to_string())
        .replace("@TH@", &(atlas::ATLAS_H / 16).to_string())
        .replace("@MIPS@", &atlas::MIPS.to_string())
        .replace("@IY@", &atlas::ITEMS_Y.to_string())
        .replace("@CY@", &atlas::CHAR_Y.to_string())
        .replace("@SB@", &format!("{:?}", 0.5f32.powf(2.2)))
        .replace("@SZ@", &format!("{:?}", 0.8f32.powf(2.2)))
        .replace("@SX@", &format!("{:?}", 0.6f32.powf(2.2)))
}

pub struct ChunkPipeline {
    /// Opaque terrain (`fs_solid`): no `discard`.
    pub solid: RenderPipeline,
    /// Alpha-tested terrain (`fs_cut`). Chunk meshes (`solid`, `cutout`, `water`) read the 16-byte `chunk_layout` vertex and bind
    /// their origin in group 1; entities, the outline and the far terrain (`entity`, `lod`, `lod_water`) read the float `Vertex`.
    pub cutout: RenderPipeline,
    /// Alpha-tested entities and the outline.
    pub entity: RenderPipeline,
    /// The far terrain's water sheets (`fs_water`).
    pub lod_water: RenderPipeline,
    /// Per-chunk origins of the chunk pipelines (group 1).
    pub origins: DrawOrigins,
    /// `cutout` with a depth bias that pushes the far terrain back, so it loses every tie with the real chunks (`lod.rs`: coplanar walls).
    pub lod: RenderPipeline,
    /// Translucent water (`fs_water`), drawn after everything opaque.
    pub water: RenderPipeline,
    uniform_buf: Buffer,
    atlas_tex: Texture,
    pub bind_group: BindGroup,
}

impl ChunkPipeline {
    pub fn new(device: &Device) -> Self {
        let atlas_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("atlas"),
            size: wgpu::Extent3d { width: atlas::ATLAS_W as u32, height: atlas::ATLAS_H as u32, depth_or_array_layers: 1 },
            mip_level_count: atlas::MIPS,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniform"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let entry = |binding, visibility, ty| wgpu::BindGroupLayoutEntry { binding, visibility, ty, count: None };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("chunk_bind_layout"),
            entries: &[
                entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }),
                entry(1, wgpu::ShaderStages::FRAGMENT, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
                entry(2, wgpu::ShaderStages::FRAGMENT, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }),
            ],
        });
        let atlas_view = atlas_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("chunk_bind"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&atlas_view) },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("chunk_shader"), source: wgpu::ShaderSource::Wgsl(shader_source().into()) });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("chunk_pipeline_layout"), bind_group_layouts: &[&layout], push_constant_ranges: &[] });
        let origin_bgl = origin_layout(device);
        let pl_chunk = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("chunk_origin_pipeline_layout"), bind_group_layouts: &[&layout, &origin_bgl], push_constant_ranges: &[] });
        let origins = DrawOrigins::new(device, &origin_bgl);
        // (label, vertex entry, fragment entry, depth write, depth bias, blend, cull)
        let make = |label: &str, vs: &str, fs: &str, water: bool, bias: wgpu::DepthBiasState, compact: bool| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(if compact { &pl_chunk } else { &pl }),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some(vs), buffers: &[if compact { chunk_layout() } else { Vertex::layout() }], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(fs),
                targets: &[Some(wgpu::ColorTargetState { format: SCENE_FORMAT, blend: water.then_some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState { cull_mode: (!water).then_some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth32Float, depth_write_enabled: !water, depth_compare: wgpu::CompareFunction::Less, stencil: Default::default(), bias }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let none = wgpu::DepthBiasState::default();
        Self {
            solid: make("chunk_solid", "vs_chunk", "fs_solid", false, none, true),
            cutout: make("chunk_cutout", "vs_chunk", "fs_cut", false, none, true),
            entity: make("entity", "vs_main", "fs_cut", false, none, false),
            lod: make("lod", "vs_main", "fs_cut", false, wgpu::DepthBiasState { constant: 8, slope_scale: 1.0, clamp: 0.0 }, false),
            water: make("water", "vs_chunk_water", "fs_water", true, none, true),
            lod_water: make("lod_water", "vs_water", "fs_water", true, none, false),
            origins,
            uniform_buf,
            atlas_tex,
            bind_group,
        }
    }

    /// The frame's uniforms, one write: the camera, the sky light the time of day takes away (/ 15), the fog (`far` 0 = off, < 0 = `GL_EXP`
    /// density; `start` = fraction of `far` where the linear fog begins, 0 = a quarter; `water` = radius of the real water).
    pub fn frame(&self, queue: &Queue, view: Mat4, proj: Mat4, eye: Vec3, sky_sub: f32, fog: [f32; 3], far: f32, start: f32, water: f32) {
        let u = Uniforms {
            view_proj: (proj * view).to_cols_array_2d(),
            eye: eye.extend(sky_sub).to_array(),
            fog: [fog[0], fog[1], fog[2], far],
            fogp: [start, water, 0.0, 0.0],
        };
        queue.write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&u));
    }

    pub fn upload_atlas(&self, queue: &Queue) {
        let mut level = atlas::atlas_rgba();
        let (mut w, mut h) = (atlas::ATLAS_W, atlas::ATLAS_H);
        for l in 0..atlas::MIPS {
            self.write_rgba(queue, l, [0, 0], [w as u32, h as u32], &level);
            if l + 1 < atlas::MIPS {
                level = atlas::mip(&level, w, h);
                (w, h) = (w / 2, h / 2);
            }
        }
    }

    fn write_rgba(&self, queue: &Queue, level: u32, origin: [u32; 2], size: [u32; 2], rgba: &[u8]) {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &self.atlas_tex, mip_level: level, origin: wgpu::Origin3d { x: origin[0], y: origin[1], z: 0 }, aspect: wgpu::TextureAspect::All },
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size[0] * 4), rows_per_image: Some(size[1]) },
            wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        );
    }

    /// One tick of the animated tiles (`render::anim`): each 16 x 16 picture over its atlas tile, and its 8 x 8 / 4 x 4 mips (a
    /// tile is aligned to every level, so the mips never touch a neighbour).
    pub fn animate(&self, queue: &Queue, tiles: &[(u8, crate::render::anim::Tile)]) {
        for (t, px) in tiles {
            self.put_tile(queue, (*t as u32 % 16) * 16, (*t as u32 / 16) * 16, px);
        }
    }

    /// The same for tiles of `gui/items.png` (the compass and the clock), which sit `ITEMS_Y` lower in the atlas.
    pub fn animate_items(&self, queue: &Queue, tiles: &[(u8, crate::render::anim::Tile)]) {
        for (t, px) in tiles {
            self.put_tile(queue, (*t as u32 % 16) * 16, atlas::ITEMS_Y as u32 + (*t as u32 / 16) * 16, px);
        }
    }

    fn put_tile(&self, queue: &Queue, x: u32, y: u32, px: &crate::render::anim::Tile) {
        let (mut level, mut n) = (px.to_vec(), 16usize);
        for l in 0..atlas::MIPS {
            self.write_rgba(queue, l, [x >> l, y >> l], [n as u32, n as u32], &level);
            if l + 1 < atlas::MIPS {
                level = atlas::mip(&level, n, n);
                n /= 2;
            }
        }
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

    /// The shader parses and validates, every entry point is there, and the atlas is read through `atlas_sample_g` only (tile-clamped, no bleed).
    #[test]
    fn chunk_shader_validates() {
        let src = shader_source();
        let m = naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&src)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&m).unwrap();
        for e in ["vs_main", "vs_water", "vs_chunk", "vs_chunk_water", "fs_solid", "fs_cut", "fs_water"] {
            assert!(m.entry_points.iter().any(|p| p.name == e), "{e} missing");
        }
        assert!(!SHADER_SRC.contains("textureSample(tex"));
    }

    /// `WHITE` is 2^24 - 1, the last f32 below 2^24: `+ 0.5` rounds it up to 2^24 and all three colour bytes read 0 (black sides, mobs).
    #[test]
    fn tint_unpack_keeps_white() {
        let w = crate::render::mesh::WHITE;
        assert_eq!(w as u32, 0xFF_FF_FF);
        assert_eq!((w + 0.5) as u32, 1 << 24, "the rounding `vert` must not do");
        assert!(!SHADER_SRC.contains("vtint + 0.5"));
    }

    /// Headless smoke test on whatever adapter exists (lavapipe without a GPU): every pipeline builds, a lit ground quad goes through the
    /// scene pass and both present passes, and the same quad with no light comes out far darker.
    #[test]
    fn frame_renders_headless() {
        let instance = wgpu::Instance::default();
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
            eprintln!("no adapter: skipped");
            return;
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        let pipe = ChunkPipeline::new(&device);
        pipe.upload_atlas(&queue);
        const N: u32 = 64;
        let fmt = wgpu::TextureFormat::Rgba8UnormSrgb;
        let size = wgpu::Extent3d { width: N, height: N, depth_or_array_layers: 1 };
        let tex = |format, usage| device.create_texture(&wgpu::TextureDescriptor { label: None, size, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format, usage, view_formats: &[] });
        let depth = tex(wgpu::TextureFormat::Depth32Float, wgpu::TextureUsages::RENDER_ATTACHMENT).create_view(&Default::default());
        let out = tex(fmt, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
        let out_view = out.create_view(&Default::default());
        let present = crate::render::present::Present::new(&device, &queue, (N, N), (N, N), fmt);
        let eye = Vec3::new(0.0, 4.0, 0.0);
        pipe.frame(&queue, Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::NEG_Z), Mat4::perspective_rh(1.2, 1.0, 0.05, 100.0), eye, 0.0, [0.5; 3], 0.0, 0.0, 12.0);
        let pixel = |lm: f32| -> [u8; 4] {
            let (u0, u1, v1) = (16.0 / atlas::ATLAS_W as f32, 32.0 / atlas::ATLAS_W as f32, 16.0 / atlas::ATLAS_H as f32); // tile 1, stone
            // Chunk-local (origin 0), corners ordered so the shared quad indices (0, 1, 2), (2, 3, 0) face up towards the eye.
            let v = |x: f32, z: f32, u, vv| embeddium::vertex::encode(&embeddium::vertex::Vertex { pos: [x, 0.0, z], uv: [u, vv], light: lm as u16, color: [255; 3], shade: 255 });
            let verts = [v(-8.0, -8.0, u0, 0.0), v(-8.0, 8.0, u0, v1), v(8.0, 8.0, u1, v1), v(8.0, -8.0, u1, 0.0)];
            let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(&verts), usage: BufferUsages::VERTEX });
            let ib = create_index_buffer(&device, &embeddium::quad_index::indices(1));
            pipe.origins.write(&queue, 0, [0.0; 3]);
            let buf = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (N * N * 4) as u64, usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST, mapped_at_creation: false });
            let mut enc = device.create_command_encoder(&Default::default());
            {
                let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: present.scene_view(), resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }), stencil_ops: None }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                rp.set_bind_group(0, &pipe.bind_group, &[]);
                rp.set_vertex_buffer(0, vb.slice(..));
                rp.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                for p in [&pipe.solid, &pipe.cutout, &pipe.entity, &pipe.lod, &pipe.water, &pipe.lod_water] {
                    rp.set_pipeline(p); // all six must be valid with this pass; water blends over: it is skipped by its radius test
                    if std::ptr::eq(p, &pipe.solid) {
                        rp.set_bind_group(1, &pipe.origins.group, &[DrawOrigins::offset(0)]);
                        rp.draw_indexed(0..embeddium::quad_index::index_count(4), 0, 0..1);
                    }
                }
            }
            present.finish(&mut enc, &out_view, |_| {});
            enc.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo { texture: &out, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(N * 4), rows_per_image: Some(N) } },
                size,
            );
            queue.submit(Some(enc.finish()));
            buf.slice(..).map_async(wgpu::MapMode::Read, |r| r.unwrap());
            device.poll(wgpu::PollType::Wait).unwrap();
            let d = buf.slice(..).get_mapped_range();
            let i = ((N / 2 * N + N / 2) * 4) as usize;
            [d[i], d[i + 1], d[i + 2], d[i + 3]]
        };
        let (lit, dark) = (pixel(240.0), pixel(0.0)); // sky light 15 / no light at all
        let y = |p: [u8; 4]| p[0] as u32 + p[1] as u32 + p[2] as u32;
        assert!(y(lit) > 60, "a lit stone top face is not black: {lit:?}");
        assert!(y(dark) * 5 < y(lit), "no light is far darker: {dark:?} vs {lit:?}");
        assert_eq!(lit[3], 255);
    }
}
