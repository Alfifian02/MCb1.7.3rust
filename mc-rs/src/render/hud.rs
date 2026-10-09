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
}

impl HudVertex {
    pub fn layout() -> VertexBufferLayout<'static> {
        VertexBufferLayout {
            array_stride: std::mem::size_of::<HudVertex>() as u64,
            step_mode: VertexStepMode::Vertex,
            attributes: &[
                VertexAttribute { format: VertexFormat::Float32x2, offset: 0, shader_location: 0 },
                VertexAttribute { format: VertexFormat::Float32x4, offset: 8, shader_location: 1 },
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
};

@vertex
fn vs_main(@location(0) vpos: vec2<f32>, @location(1) vcolor: vec4<f32>) -> VsOut {
    var o: VsOut;
    o.pos = vec4<f32>(vpos.x * 2.0 / u.size.x - 1.0, 1.0 - vpos.y * 2.0 / u.size.y, 0.0, 1.0);
    o.color = vcolor;
    return o;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}
"#;
/// Owns the HUD pipeline + dynamic vertex buffer. One instance per surface.
pub struct HudPipeline {
    pub pipeline: RenderPipeline,
    pub vbuf: Buffer,
    pub uniform_buf: Buffer,
    pub bind_group: wgpu::BindGroup,
    pub surface_w: u32,
    pub surface_h: u32,
    pub quad_capacity: usize,
}

impl HudPipeline {
    pub fn new(device: &Device, queue: &Queue, surface_format: TextureFormat, surface_w: u32, surface_h: u32) -> Self {
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("hud_shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
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
            }],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hud_bind"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
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
        let quad_capacity = 4096;
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

    /// Append a single quad (top-left origin) to `out`. Two CCW triangles.
    pub fn push_quad(out: &mut Vec<HudVertex>, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
        let tl = HudVertex { pos: [x, y], color };
        let tr = HudVertex { pos: [x + w, y], color };
        let br = HudVertex { pos: [x + w, y + h], color };
        let bl = HudVertex { pos: [x, y + h], color };
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
        for (dx, c) in [(k, shadow), (0.0, color)] {
            let mut cx = x + dx;
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
                        Self::push_quad(out, cx + px as f32 * k, y + dx + row as f32 * k, run as f32 * k, k, c);
                        px += run;
                    }
                }
                cx += font::WIDTH[g] as f32 * k;
            }
        }
    }

    /// The `GuiContainer` item-name tooltip: `s` on a 75% black box (3 units of padding), the box's top-left at
    /// `(x, y)`; returns nothing, the caller picks the spot.
    pub fn push_tooltip(out: &mut Vec<HudVertex>, x: f32, y: f32, k: f32, s: &str) {
        Self::push_quad(out, x, y, (Self::text_width(s) as f32 + 6.0) * k, 14.0 * k, [0.0, 0.0, 0.0, 0.75]);
        Self::push_text(out, x + 3.0 * k, y + 3.0 * k, k, s, [1.0; 4]);
    }

    /// Outlined rect (filled with `fill`, bordered by `border`, `border_px`
    /// pixels thick).
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
        let center = HudVertex { pos: [cx, cy], color: fill };
        for i in 0..segments {
            let a0 = (i as f32 / segments as f32) * std::f32::consts::TAU;
            let a1 = ((i + 1) as f32 / segments as f32) * std::f32::consts::TAU;
            let p0 = HudVertex { pos: [cx + a0.cos() * r, cy + a0.sin() * r], color: fill };
            let p1 = HudVertex { pos: [cx + a1.cos() * r, cy + a1.sin() * r], color: fill };
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
            let p0_in = HudVertex { pos: [cx + c0 * r_inner, cy + s0 * r_inner], color };
            let p0_out = HudVertex { pos: [cx + c0 * r_outer, cy + s0 * r_outer], color };
            let p1_in = HudVertex { pos: [cx + c1 * r_inner, cy + s1 * r_inner], color };
            let p1_out = HudVertex { pos: [cx + c1 * r_outer, cy + s1 * r_outer], color };
            out.push(p0_in); out.push(p0_out); out.push(p1_out);
            out.push(p0_in); out.push(p1_out); out.push(p1_in);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) {
        assert!((a - b).abs() < 0.001, "{a} != {b}");
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
