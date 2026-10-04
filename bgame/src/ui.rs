//! HUD layout (shared by input hit-testing and drawing) and a tiny 2D batch.
use bcore::block::{tile, Face};

#[derive(Clone, Copy, Debug)]
pub struct Rect { pub x: f32, pub y: f32, pub w: f32, pub h: f32 }

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self { Self { x, y, w, h } }
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
    fn inset(&self, k: f32) -> Rect { Rect::new(self.x + k, self.y + k, self.w - 2.0 * k, self.h - 2.0 * k) }
}

pub struct Layout {
    pub hotbar: [Rect; 9],
    pub jump: Rect,
    pub place: Rect,
    pub brk: Rect,
    pub fly: Rect,
    pub w: f32,
    pub h: f32,
}

impl Layout {
    pub fn new(w: f32, h: f32) -> Self {
        let m = w.min(h);
        let slot = (m / 10.0).clamp(28.0, 90.0);
        let gap = 4.0;
        let total = 9.0 * slot + 8.0 * gap;
        let x0 = (w - total) * 0.5;
        let y0 = h - slot - 8.0;
        let mut hotbar = [Rect::new(0.0, 0.0, 0.0, 0.0); 9];
        for (i, r) in hotbar.iter_mut().enumerate() {
            *r = Rect::new(x0 + i as f32 * (slot + gap), y0, slot, slot);
        }
        let b = m * 0.17;
        let g = b * 0.2;
        Layout {
            hotbar,
            jump: Rect::new(w - b - g, h - b - g, b, b),
            place: Rect::new(w - 2.0 * b - 2.0 * g, h - b - g, b, b),
            brk: Rect::new(w - b - g, h - 2.0 * b - 2.0 * g, b, b),
            fly: Rect::new(w - b * 0.8 - g, g, b * 0.8, b * 0.8),
            w,
            h,
        }
    }
}

/// x, y in pixels; uv < 0 means "flat colour, no texture".
#[repr(C)]
#[derive(Clone, Copy)]
pub struct UiVert { pub x: f32, pub y: f32, pub u: f32, pub v: f32, pub col: [u8; 4] }

#[derive(Default)]
pub struct UiBatch { pub verts: Vec<UiVert> }

impl UiBatch {
    pub fn clear(&mut self) { self.verts.clear(); }

    fn quad(&mut self, r: Rect, uv: [f32; 4], col: [u8; 4]) {
        let v = |x, y, u, v| UiVert { x, y, u, v, col };
        let (x1, y1) = (r.x + r.w, r.y + r.h);
        self.verts.extend_from_slice(&[
            v(r.x, r.y, uv[0], uv[1]), v(x1, r.y, uv[2], uv[1]), v(x1, y1, uv[2], uv[3]),
            v(r.x, r.y, uv[0], uv[1]), v(x1, y1, uv[2], uv[3]), v(r.x, y1, uv[0], uv[3]),
        ]);
    }

    pub fn rect(&mut self, r: Rect, col: [u8; 4]) { self.quad(r, [-1.0; 4], col); }

    pub fn tile(&mut self, r: Rect, t: u8, col: [u8; 4]) {
        let (tx, ty) = ((t % 16) as f32, (t / 16) as f32);
        let (e0, e1) = (0.02, 0.98);
        self.quad(r, [(tx + e0) / 16.0, (ty + e0) / 16.0, (tx + e1) / 16.0, (ty + e1) / 16.0], col);
    }
}

pub struct HudState<'a> {
    pub sel: usize,
    pub hotbar: &'a [u8; 9],
    pub touch_ui: bool,
    pub stick: Option<((f32, f32), (f32, f32))>,
    /// Current view distance in chunks, shown as small squares top-left.
    pub view_radius: i32,
}

pub fn build(b: &mut UiBatch, l: &Layout, s: &HudState) {
    // crosshair
    let (cx, cy) = (l.w * 0.5, l.h * 0.5);
    b.rect(Rect::new(cx - 9.0, cy - 1.0, 18.0, 2.0), [255, 255, 255, 220]);
    b.rect(Rect::new(cx - 1.0, cy - 9.0, 2.0, 18.0), [255, 255, 255, 220]);

    // view-distance pips
    for i in 0..s.view_radius.max(0) {
        b.rect(Rect::new(10.0 + i as f32 * 14.0, 10.0, 10.0, 10.0), [255, 255, 255, 150]);
    }

    // hotbar
    for (i, r) in l.hotbar.iter().enumerate() {
        b.rect(*r, [0, 0, 0, 110]);
        if i == s.sel {
            let t = 3.0;
            let c = [255, 255, 255, 235];
            b.rect(Rect::new(r.x - t, r.y - t, r.w + 2.0 * t, t), c);
            b.rect(Rect::new(r.x - t, r.y + r.h, r.w + 2.0 * t, t), c);
            b.rect(Rect::new(r.x - t, r.y, t, r.h), c);
            b.rect(Rect::new(r.x + r.w, r.y, t, r.h), c);
        }
        b.tile(r.inset(r.w * 0.15), tile(s.hotbar[i], Face::PosX), [255, 255, 255, 255]);
    }

    if s.touch_ui {
        b.rect(l.jump, [255, 255, 255, 70]);
        b.rect(l.place, [80, 220, 80, 80]);
        b.rect(l.brk, [230, 70, 70, 80]);
        b.rect(l.fly, [80, 200, 255, 80]);
        if let Some(((ax, ay), (bx, by))) = s.stick {
            b.rect(Rect::new(ax - 40.0, ay - 40.0, 80.0, 80.0), [255, 255, 255, 40]);
            b.rect(Rect::new(bx - 20.0, by - 20.0, 40.0, 40.0), [255, 255, 255, 110]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_do_not_overlap_hotbar() {
        for (w, h) in [(1600.0, 720.0), (2400.0, 1080.0), (1280.0, 720.0), (960.0, 540.0)] {
            let l = Layout::new(w, h);
            let last = l.hotbar[8];
            assert!(last.x + last.w < l.place.x, "{w}x{h}");
            assert!(l.hotbar[0].x > 0.0);
        }
    }

    #[test]
    fn batch_quad_has_six_verts() {
        let mut b = UiBatch::default();
        b.rect(Rect::new(0.0, 0.0, 1.0, 1.0), [0; 4]);
        assert_eq!(b.verts.len(), 6);
        assert_eq!(std::mem::size_of::<UiVert>(), 20);
    }
}
