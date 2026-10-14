// M12 - 2D HUD overlay renderer.
//
// SOURCE-OF-TRUTH NOTE (per GPT-5.5 porting guide, rule #1, #2, #8):
//   - b1.7.3 Java sources contain GuiIngame (hotbar), GuiIngameMenu (pause
//     menu) and MovementInputFromOptions (keyboard movement).
//     See minecraft/src/net/minecraft/src/GuiIngame.java:
//       - hotbar background: (var6/2 - 91, var7 - 22) size 182x22
//       - selection: (var6/2 - 91 - 1 + currentItem*20, var7 - 22 - 1) 24x22
//     The hotbar cell-size (20 px) in build_hud matches that layout.
//   - D-pad, look-stick, pause-button are NOT in b1.7.3 Java. They are a
//     Pocket Edition-style touch UX invented for this port. Marked
//     UNVERIFIED in touch_ui.rs.
//   - The Android target build was interrupted before completion and was
//     not tested. Verify with `cargo apk build --release` on the CI
//     runner before declaring M12 done.
//
// Renders a flat-colour orthographic overlay on top of the 3D scene. The HUD
// is a separate render pass that does not touch the chunk pipeline. It
// writes directly to the swap-chain view (no depth buffer).
//
// The HUD owns a small dynamic vertex buffer (capacity ~64 quads). Each quad
// is six vertices (two triangles), each vertex is (px, py, r, g, b, a).
// The shader is trivial: pass-through, no lighting, alpha-blended.

use bytemuck::{Pod, Zeroable};

use crate::render::font;

use wgpu::{BlendState, Buffer, BufferUsages, ColorTargetState, ColorWrites, Device, FragmentState, FrontFace, MultisampleState, PipelineCompilationOptions, PipelineLayoutDescriptor, PolygonMode, PrimitiveState, PrimitiveTopology, Queue, RenderPipeline, ShaderModuleDescriptor, ShaderStages, TextureFormat, VertexAttribute, VertexBufferLayout, VertexFormat, VertexState, VertexStepMode};

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct HudVertex {
    pub pos: [f32; 2],
    pub color: [f32; 4],
    /// Texel position in 0..1 of the items sheet, or `NO_UV` for a flat colour.
    pub uv: [f32; 2],
}

pub const NO_UV: [f32; 2] = [-1.0, -1.0];

/// The pack's `gui/widgets.png` over `gui/icons.png` (256 x 512, `tools/gen_gui.py`). In the HUD texture it sits under the items sheet
/// (rows 256..768), then `terrain.png`, then the container panels. `GuiIngame`'s sprites keep their b1.7.3 places.
pub const GUI: &[u8] = include_bytes!("../../assets/gui.rgba");
/// `gui/container/{inventory,crafting_table,furnace,generic_54}.png` stacked, 256 x 1024 (`tools/gen_gui.py`).
pub const CONTAINERS: &[u8] = include_bytes!("../../assets/containers.rgba");
/// The touch d-pad's sprites (Stone D-pad pack, 256 x 256, `tools/gen_dpad.py`): the last 256 rows of the HUD texture.
pub const DPAD_SHEET: &[u8] = include_bytes!("../../assets/dpad.rgba");
const SHEET_H: f32 = 2304.0;
/// Where `terrain.png` (untinted, 256 x 256) starts in the HUD texture: block icons are drawn from it (`push_cube`, `push_tile`).
const TERRAIN_Y: f32 = 768.0;

/// Sprites as `(x, y, w, h)` in pixels of the whole HUD texture: items 0..256, widgets.png 256..512, icons.png 512..768, terrain.png
/// 768..1024, then the container panels (inventory 1024, crafting table 1280, furnace 1536, chest 1792), then the d-pad sheet (2048).
pub mod sprites {
    pub type Sprite = (f32, f32, f32, f32);
    pub const HOTBAR: Sprite = (0.0, 256.0, 182.0, 22.0);
    pub const SELECTOR: Sprite = (0.0, 278.0, 24.0, 24.0);
    pub const CROSSHAIR: Sprite = (0.0, 512.0, 16.0, 16.0);
    pub const HEART_BG: Sprite = (16.0, 512.0, 9.0, 9.0);
    pub const HEART_FULL: Sprite = (52.0, 512.0, 9.0, 9.0);
    pub const HEART_HALF: Sprite = (61.0, 512.0, 9.0, 9.0);
    pub const ARMOR_EMPTY: Sprite = (16.0, 521.0, 9.0, 9.0);
    pub const ARMOR_HALF: Sprite = (25.0, 521.0, 9.0, 9.0);
    pub const ARMOR_FULL: Sprite = (34.0, 521.0, 9.0, 9.0);
    pub const BUBBLE: Sprite = (16.0, 530.0, 9.0, 9.0);
    pub const BUBBLE_POP: Sprite = (25.0, 530.0, 9.0, 9.0);
    /// The 176 x 166 panels (a chest's rows are cut from `CHEST_Y`, see `lib.rs`); the furnace's flame is at (176, 0) of its panel
    /// (14 x 14) and its arrow at (176, 14) (24 x 16).
    pub const INVENTORY: Sprite = (0.0, 1024.0, 176.0, 166.0);
    pub const CRAFTING: Sprite = (0.0, 1280.0, 176.0, 166.0);
    pub const FURNACE: Sprite = (0.0, 1536.0, 176.0, 166.0);
    pub const CHEST_Y: f32 = 1792.0;
    /// Touch d-pad (`tools/gen_dpad.py`). Directions in the order up, down, left, right, up_left, up_right, down_left, down_right:
    /// the arrows are 22 x 22, the four corners 18 x 18. Pressed ones are 22 rows lower; sneak / jump are 64 x 64.
    pub const DPAD: [Sprite; 8] = dpad_row(2048.0);
    pub const DPAD_PRESSED: [Sprite; 8] = dpad_row(2070.0);
    pub const SNEAK: Sprite = (0.0, 2112.0, 64.0, 64.0);
    pub const SNEAK_ON: Sprite = (64.0, 2112.0, 64.0, 64.0);
    pub const JUMP: Sprite = (128.0, 2112.0, 64.0, 64.0);
    pub const JUMP_ON: Sprite = (192.0, 2112.0, 64.0, 64.0);
    const fn dpad_row(y: f32) -> [Sprite; 8] {
        // literals, not `i * 22.0`: float arithmetic in a `const fn` needs Rust 1.82
        [(0.0, y, 22.0, 22.0), (22.0, y, 22.0, 22.0), (44.0, y, 22.0, 22.0), (66.0, y, 22.0, 22.0), (88.0, y, 18.0, 18.0), (110.0, y, 18.0, 18.0), (132.0, y, 18.0, 18.0), (154.0, y, 18.0, 18.0)]
    }
}

impl HudVertex {
    pub fn layout() -> VertexBufferLayout<'static> {
        VertexBufferLayout {
            array_stride: std::mem::size_of::<HudVertex>() as u64,
            step_mode: VertexStepMode::Vertex,
            attributes: &[
                VertexAttribute { format: VertexFormat::Float32x2, offset: 0, shader_location: 0 },
                VertexAttribute { format: VertexFormat::Float32x4, offset: 8, shader_location: 1 },
                VertexAttribute { format: VertexFormat::Float32x2, offset: 24, shader_location: 2 },
            ],
        }
    }
}

// WGSL shader source. Kept as a multi-line raw string so naga reports line
// numbers correctly when validation errors happen on Android Vulkan drivers.
const SHADER_SRC: &str = r#"
struct HudU { size: vec2<f32> };
@group(0) @binding(0) var<uniform> u: HudU;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
};
@group(0) @binding(1) var items: texture_2d<f32>;
@group(0) @binding(2) var items_samp: sampler;

@vertex
fn vs_main(@location(0) vpos: vec2<f32>, @location(1) vcolor: vec4<f32>, @location(2) vuv: vec2<f32>) -> VsOut {
    var o: VsOut;
    o.pos = vec4<f32>(vpos.x * 2.0 / u.size.x - 1.0, 1.0 - vpos.y * 2.0 / u.size.y, 0.0, 1.0);
    o.color = vcolor;
    o.uv = vuv;
    return o;
}

// The post chain ends on the surface and handles an sRGB one (`SRGB_SURFACE`); this pass writes after it, so it has to do the same: the
// sheets and the flat colours are sRGB bytes, and an sRGB surface encodes what it is given, which washed the whole HUD out (too bright).
fn lin(c: vec3<f32>) -> vec3<f32> {
    if !SRGB { return c; }
    return select(c / 12.92, pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c > vec3<f32>(0.04045));
}

// Sharp bilinear: nearest inside a texel, a one-pixel blend only across a texel edge. The sprites are drawn at a fractional scale of the
// screen, where plain nearest makes some texels one pixel wider than their neighbours (the grain on icons and the hotbar).
const SHEET: vec2<f32> = vec2<f32>(256.0, 2304.0); // `SHEET_H`

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let p = in.uv * SHEET;
    let w = max(fwidth(p), vec2<f32>(1e-4)); // derivatives before the early return: uniform control flow
    let e = floor(p + 0.5);
    let q = (e + clamp((p - e) / w, vec2<f32>(-0.5), vec2<f32>(0.5))) / SHEET;
    if in.uv.x < 0.0 { return vec4<f32>(lin(in.color.rgb), in.color.a); }
    let t = textureSampleLevel(items, items_samp, q, 0.0); // `gui/items.png`, the GUI sheet, terrain.png, the panels
    if t.a < 0.5 { discard; }
    return vec4<f32>(lin(t.rgb * in.color.rgb), t.a * in.color.a);
}
"#;
/// The shader with its one constant spliced in: whether the surface encodes sRGB itself.
fn shader_source(srgb: bool) -> String {
    format!("const SRGB: bool = {srgb};\n{SHADER_SRC}")
}

/// Owns the HUD pipeline + dynamic vertex buffer. One instance per surface.
pub struct HudPipeline {
    pub pipeline: RenderPipeline,
    pub vbuf: Buffer,
    pub uniform_buf: Buffer,
    pub bind_group: wgpu::BindGroup,
    pub surface_w: u32,
    pub surface_h: u32,
    pub quad_capacity: usize,
    /// `gui/items.png` as the HUD samples it; the compass and clock tiles are rewritten every tick.
    items_tex: wgpu::Texture,
}

impl HudPipeline {
    /// `TextureFX` for the item sprites in the hotbar and the inventory (the atlas copy is `ChunkPipeline::animate_items`).
    pub fn animate_items(&self, queue: &Queue, tiles: &[(u8, crate::render::anim::Tile)]) {
        for (t, px) in tiles {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &self.items_tex, mip_level: 0, origin: wgpu::Origin3d { x: (*t as u32 % 16) * 16, y: (*t as u32 / 16) * 16, z: 0 }, aspect: wgpu::TextureAspect::All },
                px,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(64), rows_per_image: Some(16) },
                wgpu::Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
            );
        }
    }

    pub fn new(device: &Device, queue: &Queue, surface_format: TextureFormat, surface_w: u32, surface_h: u32) -> Self {
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("hud_shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source(surface_format.is_srgb()).into()),
        });

        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hud_uniform"),
            size: 8,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hud_bind_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }, wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                count: None,
            }, wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            }],
        });
        // The item sprites (`gui/items.png`, 256 x 256), the GUI sheet and `terrain.png` under them, Rgba8Unorm: the shader decodes sRGB itself when the surface is one.
        let items_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("hud_items"),
            size: wgpu::Extent3d { width: 256, height: SHEET_H as u32, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &items_tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &[crate::render::atlas::ITEMS, GUI, crate::render::atlas::TERRAIN, CONTAINERS, DPAD_SHEET].concat(), // items, the GUI sheet, terrain.png, the panels, the d-pad
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(256 * 4), rows_per_image: Some(SHEET_H as u32) },
            wgpu::Extent3d { width: 256, height: SHEET_H as u32, depth_or_array_layers: 1 },
        );
        let items_view = items_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let items_samp = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() }); // Linear only feeds `fs_main`'s sharp-bilinear UV

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hud_bind"),
            layout: &bind_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&items_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&items_samp) },
            ],
        });

        let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("hud_pipeline_layout"),
            bind_group_layouts: &[&bind_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("hud_pipeline"),
            layout: Some(&layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[HudVertex::layout()],
                compilation_options: PipelineCompilationOptions::default(),
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(ColorTargetState {
                    format: surface_format,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: PipelineCompilationOptions::default(),
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // 512 quads = 3072 verts * 24 bytes = 72 KB. The old 64-quad cap
        // (384 verts) was smaller than the hotbar alone, so the controls were
        // truncated and never drawn. Peak HUD is ~1000 verts.
        // 4096 quads: an open inventory screen is 46 slots plus up to 46 stacks with two-digit counts, plus a tooltip (~500).
        let quad_capacity = 6144; // the sign editor's keyboard is ~56 key glyphs, two quads a run (text + shadow), plus the board's lines
        let vbuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hud_vbuf"),
            size: (quad_capacity * 6 * std::mem::size_of::<HudVertex>()) as u64,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let me = Self {
            pipeline,
            vbuf,
            uniform_buf,
            bind_group,
            surface_w,
            surface_h,
            quad_capacity,
            items_tex,
        };
        // Initial uniform write so the first frame is sized correctly.
        let bytes = [surface_w as f32, surface_h as f32];
        queue.write_buffer(&me.uniform_buf, 0, bytemuck::cast_slice(&bytes));
        me
    }

    pub fn resize(&mut self, queue: &Queue, w: u32, h: u32) {
        self.surface_w = w;
        self.surface_h = h;
        let bytes = [w as f32, h as f32];
        queue.write_buffer(&self.uniform_buf, 0, bytemuck::cast_slice(&bytes));
    }

    /// An items.png sprite (`Item.iconIndex` = `icon`) in the box (x, y, w, h), top-left origin; the alpha cuts it out.
    pub fn push_sprite(out: &mut Vec<HudVertex>, x: f32, y: f32, w: f32, h: f32, icon: u8) {
        let (u0, v0) = ((icon % 16) as f32 / 16.0, (icon / 16) as f32 * 16.0 / SHEET_H);
        let (u0, v0) = (u0 + 0.5 / 256.0, v0 + 0.5 / SHEET_H); // half a texel in: `fs_main`'s sharp bilinear blends across the sprite's edge into its neighbour
        let (u1, v1) = (u0 + 15.0 / 256.0, v0 + 15.0 / SHEET_H);
        let c = [1.0; 4];
        let v = |px: f32, py: f32, u: f32, vv: f32| HudVertex { pos: [px, py], color: c, uv: [u, vv] };
        let (tl, tr, br, bl) = (v(x, y, u0, v0), v(x + w, y, u1, v0), v(x + w, y + h, u1, v1), v(x, y + h, u0, v1));
        out.extend([tl, tr, br, tl, br, bl]);
    }

    /// A sprite of the HUD texture (`sprites`) stretched over the box (x, y, w, h), top-left origin; the alpha cuts it out.
    pub fn push_gui(out: &mut Vec<HudVertex>, x: f32, y: f32, w: f32, h: f32, (sx, sy, sw, sh): sprites::Sprite) {
        // A hair inside the sprite, so nearest sampling never picks up its neighbour.
        let (u0, u1) = ((sx + 0.01) / 256.0, (sx + sw - 0.01) / 256.0);
        let (v0, v1) = ((sy + 0.01) / SHEET_H, (sy + sh - 0.01) / SHEET_H);
        let c = [1.0; 4];
        let v = |px: f32, py: f32, u: f32, vv: f32| HudVertex { pos: [px, py], color: c, uv: [u, vv] };
        let (tl, tr, br, bl) = (v(x, y, u0, v0), v(x + w, y, u1, v0), v(x + w, y + h, u1, v1), v(x, y + h, u0, v1));
        out.extend([tl, tr, br, tl, br, bl]);
    }

    /// A `terrain.png` tile (`vspan` of its height from the top: a slab's side is half) on the quad `p` (corners in order tl, tr, br, bl),
    /// multiplied by `rgb`.
    /// `(ix, iy)` are the texels cut off the left / right and top / bottom: 0.5 keeps `fs_main`'s sharp bilinear (nearest inside a texel,
    /// a blend across its edge) from reading the NEIGHBOURING tile of `terrain.png`, which drew a line round every block icon; more
    /// trims a tile's own transparent border (the cactus).
    fn push_tile_quad(out: &mut Vec<HudVertex>, p: [[f32; 2]; 4], tile: u16, vspan: f32, rgb: [f32; 3], (ix, iy): (f32, f32)) {
        let (tx, ty) = ((tile % 16) as f32 * 16.0, (tile / 16) as f32 * 16.0);
        let (u0, u1) = ((tx + ix) / 256.0, (tx + 16.0 - ix) / 256.0);
        let (v0, v1) = ((TERRAIN_Y + ty + iy) / SHEET_H, (TERRAIN_Y + ty + 16.0 * vspan - iy) / SHEET_H);
        let c = [rgb[0], rgb[1], rgb[2], 1.0];
        let v = |q: [f32; 2], u: f32, vv: f32| HudVertex { pos: q, color: c, uv: [u, vv] };
        let (tl, tr, br, bl) = (v(p[0], u0, v0), v(p[1], u1, v0), v(p[2], u1, v1), v(p[3], u0, v1));
        out.extend([tl, tr, br, tl, br, bl]);
    }

    /// A `gui/widgets.png` button (`GuiButton`): `state` 0 disabled, 1 normal, 2 highlighted (rows 46 / 66 / 86 of the sheet, 200 x 20),
    /// scaled so its height is `h`. The two ends (4 sprite pixels) keep their shape, the middle is the flat strip at 96..104 stretched,
    /// so a square (the touch buttons) and a wide bar both work.
    pub fn push_button(out: &mut Vec<HudVertex>, x: f32, y: f32, w: f32, h: f32, state: u8) {
        let sy = 256.0 + 46.0 + 20.0 * state.min(2) as f32; // widgets.png starts at row 256 of the texture
        let cap = (4.0 * h / 20.0).min(w * 0.5);
        Self::push_gui(out, x, y, cap, h, (0.0, sy, 4.0, 20.0));
        Self::push_gui(out, x + w - cap, y, cap, h, (196.0, sy, 4.0, 20.0));
        if w > 2.0 * cap {
            Self::push_gui(out, x + cap, y, w - 2.0 * cap, h, (96.0, sy, 8.0, 20.0));
        }
    }

    /// `s` centred on `cx` with its top at `y` (`GuiScreen.drawCenteredString`).
    pub fn push_text_centered(out: &mut Vec<HudVertex>, cx: f32, y: f32, k: f32, s: &str, color: [f32; 4]) {
        Self::push_text(out, cx - Self::text_width(s) as f32 * k * 0.5, y, k, s, color);
    }

    /// A terrain tile flat in the box (x, y, w, h): what the inventory shows of a sapling, a torch, a ladder, a rail.
    pub fn push_tile(out: &mut Vec<HudVertex>, x: f32, y: f32, w: f32, h: f32, tile: u16, rgb: [f32; 3]) {
        Self::push_tile_quad(out, [[x, y], [x + w, y], [x + w, y + h], [x, y + h]], tile, 1.0, rgb, (0.5, 0.5));
    }

    /// A block as the console and `RenderBlocks.renderBlockOnInventory` draw it: a cube seen from above at the usual isometric angle, the
    /// top and the two sides in the box (x, y, w, h), lit 1.0 / 0.8 / 0.6 (top, left, right). `faces` are (tile, tint) of top, left, right;
    /// `overlay` is a second, tinted tile over the left and right side (the grass strip); `height` is 1.0 for a cube, 0.5 for a slab;
    /// `cut` is how many texels of each tile's transparent border to leave out (1 for the cactus, whose faces are 14 wide).
    /// The cube is 30 degrees down and 45 around: 1.414 wide, 1.573 tall, so its box is 0.9 as wide as it is tall (a square box stretched it).
    // UNVERIFIED: which two sides face the viewer (here south on the left, east on the right) and the light factors.
    pub fn push_cube(out: &mut Vec<HudVertex>, (x, y, w, h): (f32, f32, f32, f32), faces: [(u16, [f32; 3]); 3], overlay: Option<(u16, [f32; 3])>, height: f32, cut: f32) {
        const A: f32 = 0.2247; // half the top rhombus's height, of the cube's; the side edge is 1 - 2 A
        let (x, w) = (x + w * 0.05, w * 0.9);
        let (cx, d) = (x + w * 0.5, (1.0 - 2.0 * A) * h * (1.0 - height)); // d: how far a slab's top sits below a cube's
        let (t, tl, tr, c) = ([cx, y + d], [x, y + d + A * h], [x + w, y + d + A * h], [cx, y + d + 2.0 * A * h]);
        let (bl, br, b) = ([x, y + h - A * h], [x + w, y + h - A * h], [cx, y + h]);
        let lit = |rgb: [f32; 3], k: f32| [rgb[0] * k, rgb[1] * k, rgb[2] * k];
        let (top, side) = ((0.5 + cut, 0.5 + cut), (0.5 + cut, 0.5));
        Self::push_tile_quad(out, [t, tr, c, tl], faces[0].0, 1.0, lit(faces[0].1, 1.0), top);
        Self::push_tile_quad(out, [tl, c, b, bl], faces[1].0, height, lit(faces[1].1, 0.8), side);
        Self::push_tile_quad(out, [c, tr, br, b], faces[2].0, height, lit(faces[2].1, 0.6), side);
        if let Some((tile, rgb)) = overlay {
            Self::push_tile_quad(out, [tl, c, b, bl], tile, height, lit(rgb, 0.8), side);
            Self::push_tile_quad(out, [c, tr, br, b], tile, height, lit(rgb, 0.6), side);
        }
    }

    /// Append a single quad (top-left origin) to `out`. Two CCW triangles.
    pub fn push_quad(out: &mut Vec<HudVertex>, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
        let tl = HudVertex { pos: [x, y], color, uv: NO_UV };
        let tr = HudVertex { pos: [x + w, y], color, uv: NO_UV };
        let br = HudVertex { pos: [x + w, y + h], color, uv: NO_UV };
        let bl = HudVertex { pos: [x, y + h], color, uv: NO_UV };
        out.push(tl); out.push(tr); out.push(br);
        out.push(tl); out.push(br); out.push(bl);
    }

    /// Width of `s` in GUI units (`FontRenderer.getStringWidth`); ASCII 32..=126 only, anything else counts as `?`.
    pub fn text_width(s: &str) -> u32 {
        s.bytes().map(|b| font::WIDTH[glyph(b)] as u32).sum()
    }

    /// `FontRenderer.drawStringWithShadow`: `s` with its top-left at `(x, y)`, one font pixel = `k` px, the shadow one
    /// pixel down-right in a quarter of the colour. Runs of lit pixels in a row are one quad.
    pub fn push_text(out: &mut Vec<HudVertex>, x: f32, y: f32, k: f32, s: &str, color: [f32; 4]) {
        let shadow = [color[0] * 0.25, color[1] * 0.25, color[2] * 0.25, color[3]];
        Self::push_glyphs(out, x + k, y + k, k, s, shadow);
        Self::push_glyphs(out, x, y, k, s, color);
    }

    /// `s` in `color` with a thin black outline `o` px wide: the text in black at the 8 offsets round it, then the text itself. Replaces
    /// the shadow (the outline already separates the glyphs from the panel).
    pub fn push_text_outlined(out: &mut Vec<HudVertex>, x: f32, y: f32, k: f32, o: f32, s: &str, color: [f32; 4]) {
        for (dx, dy) in [(-o, -o), (0.0, -o), (o, -o), (-o, 0.0), (o, 0.0), (-o, o), (0.0, o), (o, o)] {
            Self::push_glyphs(out, x + dx, y + dy, k, s, [0.0, 0.0, 0.0, color[3]]);
        }
        Self::push_glyphs(out, x, y, k, s, color);
    }

    fn push_glyphs(out: &mut Vec<HudVertex>, x: f32, y: f32, k: f32, s: &str, c: [f32; 4]) {
        let mut cx = x;
        for b in s.bytes() {
            let g = glyph(b);
            for (row, bits) in font::GLYPHS[g].iter().enumerate() {
                let mut px = 0;
                while px < 8 {
                    if bits >> px & 1 == 0 {
                        px += 1;
                        continue;
                    }
                    let run = (px..8).take_while(|p| bits >> p & 1 == 1).count();
                    Self::push_quad(out, cx + px as f32 * k, y + row as f32 * k, run as f32 * k, k, c);
                    px += run;
                }
            }
            cx += font::WIDTH[g] as f32 * k;
        }
    }

    /// The `GuiContainer` item-name tooltip: `s` on a 75% black box (3 units of padding), the box's top-left at
    /// `(x, y)`; returns nothing, the caller picks the spot.
    pub fn push_tooltip(out: &mut Vec<HudVertex>, x: f32, y: f32, k: f32, s: &str) {
        Self::push_quad(out, x, y, (Self::text_width(s) as f32 + 6.0) * k, 14.0 * k, [0.0, 0.0, 0.0, 0.75]);
        Self::push_text(out, x + 3.0 * k, y + 3.0 * k, k, s, [1.0; 4]);
    }

    /// Seven-segment number (there is no text rendering yet, M14). `h` is the digit height in px;
    /// each digit advances by `0.71 * h`.
    pub fn push_number(out: &mut Vec<HudVertex>, x: f32, y: f32, h: f32, n: u32, color: [f32; 4]) {
        // Bits a..g = top, top-right, bottom-right, bottom, bottom-left, top-left, middle.
        const SEG: [u8; 10] = [63, 6, 91, 79, 102, 109, 125, 7, 127, 111];
        let (w, t) = (h * 0.5, h * 0.14);
        let half = h * 0.5 + t * 0.5;
        for (i, c) in n.to_string().bytes().enumerate() {
            let dx = x + i as f32 * (w + t * 1.5);
            let segs = [
                (dx, y, w, t),
                (dx + w - t, y, t, half),
                (dx + w - t, y + h * 0.5 - t * 0.5, t, half),
                (dx, y + h - t, w, t),
                (dx, y + h * 0.5 - t * 0.5, t, half),
                (dx, y, t, half),
                (dx, y + (h - t) * 0.5, w, t),
            ];
            for (bit, &(sx, sy, sw, sh)) in segs.iter().enumerate() {
                if SEG[(c - b'0') as usize] >> bit & 1 == 1 {
                    Self::push_quad(out, sx, sy, sw, sh, color);
                }
            }
        }
    }

    /// A stack count as `RenderItem.renderItemOverlayIntoGUI` draws it: the font's own digits with the shadow, right-aligned so the
    /// text ends at (x + 17, ..) of the 16 unit item box at (x, y), and 9 units down; `k` px per unit.
    pub fn push_count(out: &mut Vec<HudVertex>, x: f32, y: f32, k: f32, n: u32) {
        let s = n.to_string();
        Self::push_text(out, x + (17.0 - Self::text_width(&s) as f32) * k, y + 9.0 * k, k, &s, [1.0; 4]);
    }

    /// Outlined rect (filled with `fill`, bordered by `border`, `border_px` pixels thick).
    pub fn push_outlined_quad(
        out: &mut Vec<HudVertex>,
        x: f32, y: f32, w: f32, h: f32,
        fill: [f32; 4], border: [f32; 4], border_px: f32,
    ) {
        let bp = border_px.max(1.0);
        Self::push_quad(out, x, y, w, bp, border);
        Self::push_quad(out, x, y + h - bp, w, bp, border);
        Self::push_quad(out, x, y, bp, h, border);
        Self::push_quad(out, x + w - bp, y, bp, h, border);
        Self::push_quad(out, x + bp, y + bp, w - bp * 2.0, h - bp * 2.0, fill);
    }

    /// Filled disc (24-segment triangle fan). Used by the d-pad buttons.
    pub fn push_disc(
        out: &mut Vec<HudVertex>,
        cx: f32, cy: f32, r: f32, fill: [f32; 4],
    ) {
        let segments = 24;
        let center = HudVertex { pos: [cx, cy], color: fill, uv: NO_UV };
        for i in 0..segments {
            let a0 = (i as f32 / segments as f32) * std::f32::consts::TAU;
            let a1 = ((i + 1) as f32 / segments as f32) * std::f32::consts::TAU;
            let p0 = HudVertex { pos: [cx + a0.cos() * r, cy + a0.sin() * r], color: fill, uv: NO_UV };
            let p1 = HudVertex { pos: [cx + a1.cos() * r, cy + a1.sin() * r], color: fill, uv: NO_UV };
            out.push(center); out.push(p0); out.push(p1);
        }
    }

    /// Annulus (ring) for the look-stick visualisation.
    pub fn push_ring(
        out: &mut Vec<HudVertex>,
        cx: f32, cy: f32, r_outer: f32, r_inner: f32, color: [f32; 4],
    ) {
        let segments = 32;
        for i in 0..segments {
            let a0 = (i as f32 / segments as f32) * std::f32::consts::TAU;
            let a1 = ((i + 1) as f32 / segments as f32) * std::f32::consts::TAU;
            let c0 = a0.cos();
            let s0 = a0.sin();
            let c1 = a1.cos();
            let s1 = a1.sin();
            let p0_in = HudVertex { pos: [cx + c0 * r_inner, cy + s0 * r_inner], color, uv: NO_UV };
            let p0_out = HudVertex { pos: [cx + c0 * r_outer, cy + s0 * r_outer], color, uv: NO_UV };
            let p1_in = HudVertex { pos: [cx + c1 * r_inner, cy + s1 * r_inner], color, uv: NO_UV };
            let p1_out = HudVertex { pos: [cx + c1 * r_outer, cy + s1 * r_outer], color, uv: NO_UV };
            out.push(p0_in); out.push(p0_out); out.push(p1_out);
            out.push(p0_in); out.push(p1_out); out.push(p1_in);
        }
    }
}

#[cfg(test)]
mod tests {
    /// The HUD shader validates for an sRGB and a plain surface.
    #[test]
    fn hud_shader_validates() {
        for srgb in [false, true] {
            let src = shader_source(srgb);
            let m = naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&src)));
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&m).unwrap();
        }
    }

    /// A button is three sprites (two ends and a middle), two when it is too narrow for a middle, all inside the box and on the widgets rows.
    #[test]
    fn button_is_ends_and_middle() {
        let mut v = Vec::new();
        HudPipeline::push_button(&mut v, 10.0, 20.0, 200.0, 20.0, 1);
        assert_eq!(v.len(), 18);
        assert!(v.iter().all(|p| (10.0..=210.0).contains(&p.pos[0]) && (20.0..=40.0).contains(&p.pos[1])));
        assert!(v.iter().all(|p| p.uv[1] >= 256.0 / SHEET_H && p.uv[1] <= 352.0 / SHEET_H), "widgets rows");
        let mut s = Vec::new();
        HudPipeline::push_button(&mut s, 0.0, 0.0, 8.0, 20.0, 2);
        assert_eq!(s.len(), 12, "two caps of 4 fill 8 wide");
    }

    /// A cube icon is three quads (the overlay adds two) and stays inside its box; a slab's sides are half as tall.
    #[test]
    fn cube_icon_fits_its_box() {
        let faces = [(0u16, [1.0; 3]), (3, [1.0; 3]), (3, [1.0; 3])];
        let mut v = Vec::new();
        HudPipeline::push_cube(&mut v, (10.0, 20.0, 16.0, 16.0), faces, None, 1.0, 0.0);
        assert_eq!(v.len(), 18);
        assert!(v.iter().all(|p| (10.0..=26.0).contains(&p.pos[0]) && (20.0..=36.0).contains(&p.pos[1])));
        assert!(v.iter().all(|p| p.uv[0] >= 0.0 && p.uv[1] >= TERRAIN_Y / SHEET_H && p.uv[1] <= 1.0), "uv inside the terrain rows");
        let side_h = |v: &[HudVertex]| v[6..12].iter().map(|p| p.pos[1]).fold(f32::MIN, f32::max) - v[6..12].iter().map(|p| p.pos[1]).fold(f32::MAX, f32::min);
        let mut s = Vec::new();
        HudPipeline::push_cube(&mut s, (10.0, 20.0, 16.0, 16.0), faces, Some((38, [0.5; 3])), 0.5, 0.0);
        assert_eq!(s.len(), 30);
        assert!(side_h(&s) < side_h(&v), "a slab is lower");
        // The cube is 0.9 as wide as tall (not square), and no texel of a face reaches its tile's edge (no line from the neighbour).
        let xs = v.iter().map(|p| p.pos[0]);
        assert!(((xs.clone().fold(f32::MIN, f32::max) - xs.fold(f32::MAX, f32::min)) / 16.0 - 0.9).abs() < 1e-4);
        let mut c = Vec::new();
        HudPipeline::push_cube(&mut c, (10.0, 20.0, 16.0, 16.0), faces, None, 1.0, 1.0);
        assert!(c[0].uv[0] > v[0].uv[0], "the cactus leaves its transparent border out");
    }
    /// The container sheet is four 256-row panels; each has its frame at the top left and the furnace its flame and arrow at (176, 0).
    #[test]
    fn containers_sheet_has_the_panels() {
        assert_eq!(CONTAINERS.len(), 256 * 1024 * 4);
        let a = |x: usize, y: usize| CONTAINERS[((y - 1024) * 256 + x) * 4 + 3];
        for (_, y, _, _) in [sprites::INVENTORY, sprites::CRAFTING, sprites::FURNACE, (0.0, sprites::CHEST_Y, 0.0, 0.0)] {
            assert!(a(10, y as usize + 10) > 0, "panel at row {y} is empty");
        }
        let f = sprites::FURNACE.1 as usize;
        assert!(a(182, f + 7) > 0 && a(186, f + 20) > 0, "flame and arrow");
    }

    /// The GUI sheet is widgets.png over icons.png, and the sprites' places hold a picture (the hotbar's frame, a full heart, the bubble).
    #[test]
    fn gui_sheet_has_the_sprites() {
        assert_eq!(GUI.len(), 256 * 512 * 4);
        assert_eq!(DPAD_SHEET.len(), 256 * 256 * 4);
        // Every d-pad sprite holds a picture (its place in the sheet is the last 256 rows of the texture).
        let d = |x: f32, y: f32| DPAD_SHEET[((y as usize - 2048) * 256 + x as usize) * 4 + 3];
        for (x, y, w, h) in sprites::DPAD.into_iter().chain(sprites::DPAD_PRESSED).chain([sprites::SNEAK, sprites::SNEAK_ON, sprites::JUMP, sprites::JUMP_ON]) {
            assert!((0..(w * h) as usize).any(|i| d(x + (i % w as usize) as f32, y + (i / w as usize) as f32) > 0), "d-pad sprite at {x},{y} is empty");
        }
        let a = |x: f32, y: f32| GUI[((y as usize - 256) * 256 + x as usize) * 4 + 3]; // GUI starts at row 256 of the texture
        for (x, y, w, h) in [sprites::HOTBAR, sprites::SELECTOR, sprites::HEART_FULL, sprites::HEART_BG, sprites::BUBBLE, sprites::ARMOR_FULL, sprites::CROSSHAIR] {
            let seen = (0..(w * h) as usize).any(|i| a(x + (i % w as usize) as f32, y + (i / w as usize) as f32) > 0);
            assert!(seen, "sprite at {x},{y} is empty");
        }
    }
    use super::*;

    fn approx(a: f32, b: f32) {
        assert!((a - b).abs() < 0.001, "{a} != {b}");
    }

    /// The outlined text is the plain glyphs (white) over 8 black copies at most `o` px away: nothing leaves that margin.
    #[test]
    fn outlined_text_is_white_over_a_thin_black_rim() {
        let (mut plain, mut out) = (Vec::new(), Vec::new());
        HudPipeline::push_glyphs(&mut plain, 100.0, 50.0, 3.0, "Crafting", [1.0; 4]);
        HudPipeline::push_text_outlined(&mut out, 100.0, 50.0, 3.0, 1.0, "Crafting", [1.0; 4]);
        assert_eq!(out.len(), plain.len() * 9);
        assert!(out[out.len() - plain.len()..].iter().all(|p| p.color[..3] == [1.0; 3]), "white on top");
        assert!(out[..plain.len() * 8].iter().all(|p| p.color[..3] == [0.0; 3]), "black below");
        let (lo, hi) = (|v: &[HudVertex]| v.iter().map(|p| p.pos[0]).fold(f32::MAX, f32::min), |v: &[HudVertex]| v.iter().map(|p| p.pos[0]).fold(f32::MIN, f32::max));
        assert!((lo(&plain) - lo(&out)).abs() <= 1.001 && (hi(&out) - hi(&plain)).abs() <= 1.001);
    }

    #[test]
    fn push_quad_adds_six_vertices() {
        let mut v = Vec::new();
        HudPipeline::push_quad(&mut v, 0.0, 0.0, 10.0, 5.0, [1.0; 4]);
        assert_eq!(v.len(), 6);
    }

    #[test]
    fn push_outlined_quad_five_subquads() {
        let mut v = Vec::new();
        HudPipeline::push_outlined_quad(&mut v, 0.0, 0.0, 10.0, 10.0, [1.0; 4], [0.0; 4], 1.0);
        // 4 border strips + 1 inner fill = 5 quads = 30 vertices.
        assert_eq!(v.len(), 30);
    }

    #[test]
    fn push_disc_topology() {
        let mut v = Vec::new();
        HudPipeline::push_disc(&mut v, 0.0, 0.0, 1.0, [1.0; 4]);
        // 24 triangles -> 72 verts.
        approx(v.len() as f32, 72.0);
    }

    #[test]
    fn push_ring_topology() {
        let mut v = Vec::new();
        HudPipeline::push_ring(&mut v, 0.0, 0.0, 2.0, 1.0, [1.0; 4]);
        // 32 segments * 2 triangles * 3 verts = 192.
        approx(v.len() as f32, 192.0);
    }
}

/// Index into `font::GLYPHS` / `WIDTH` (chars 32..=126; the rest draw as `?`).
fn glyph(b: u8) -> usize {
    (if (32..=126).contains(&b) { b } else { b'?' } - 32) as usize
}

#[cfg(test)]
mod text_tests {
    use super::*;

    #[test]
    fn text_is_measured_and_drawn() {
        assert_eq!(HudPipeline::text_width(""), 0);
        assert_eq!(HudPipeline::text_width(" "), 4); // FontRenderer: space is 4 wide
        assert_eq!(HudPipeline::text_width("\u{e9}"), HudPipeline::text_width("?"));
        let mut v = Vec::new();
        HudPipeline::push_tooltip(&mut v, 0.0, 0.0, 2.0, "Iron Pickaxe");
        assert!(v.len() > 6 * 20 && v.len() % 6 == 0);
        let right = (HudPipeline::text_width("Iron Pickaxe") as f32 + 6.0) * 2.0;
        assert!(v.iter().all(|p| p.pos[0] >= 0.0 && p.pos[0] <= right + 2.0)); // + the 1 px shadow
    }
}
